//! The detail effects of tab 3 (`editing.md` §8.2): `sharpen(a)`, `grain(grain,lighten)` and `glow(i,r)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Sharpen's Amount 0–3
//!   (0.6); Film Grain's Grain 0–50 (10) and Lighten (`false`); Glow's Intensity 0–1 (0.65) and
//!   Radius 0.3–30 (3, on a logarithmic slider), the blur's radius in percent of the picture's
//!   shorter side. Numbers out of range are clamped; `NaN`, a missing or extra field or a
//!   checkbox other than `true` / `false` is refused (the entry is kept and skipped).
//! - **Sizes** are relative to the stored photo's shorter side ([`fx::Space`]), so a thumbnail, the
//!   edit view, a zoomed region and a Save agree: a large render shrunk matches a small one.
//! - **Sharpen** (M12.7): an unsharp mask, `out = in + amount · (in − blur(in))`, with a sampled
//!   Gaussian of [`SHARPEN_SIGMA`] of the shorter side (about 1.2 px on a 12 MP photo), taps in a
//!   fixed order, so a region at 1:1 equals that window of the whole render exactly. It is exact
//!   at 1:1 (`editing.md` §8.2); where that sigma is under [`MIN_SIGMA_PX`] output pixels (a render
//!   well under 1:1, a tile) the mask works at the render's own pixel scale instead, so the
//!   sharpening still shows: an approximation, close to the large render shrunk at the default
//!   amount. Amount 0 changes nothing.
//! - **Film Grain** (M12.7): monochrome grain, one value of [`fx::Noise`] (seeded by the entry,
//!   cells of [`GRAIN_CELL`] of the shorter side, placed in the stored frame) added to every
//!   channel, so the noise adds nothing to R − G; its spread is `Grain / 100 · 0.5 / √3` (uniform
//!   noise of ±`Grain / 200`, as PhotoCraft's Add Noise). With Lighten only the light half of the
//!   grain is added, `max(n, 0)`; where a small render's pixel spans several cells its varying part
//!   fades as [`fx::Noise::attenuation`] says but its mean ([`LIGHT_MEAN`]) stays, so a thumbnail
//!   is as much lighter as the full picture shrunk. Grain 0 changes nothing.
//! - **Glow** (M12.7): the highlights (each pixel weighted by a smoothstep of its luma from
//!   [`GLOW_FROM`] to 1) blurred by a Gaussian whose sigma is Radius percent of the shorter side
//!   ([`fx::Blurred`]), then screened onto the picture by Intensity: it never darkens a pixel, and a
//!   picture darker than [`GLOW_FROM`] everywhere is left as it was. Intensity 0 changes nothing.
//!
//! Sharpen ported from PhotoCraft `crates/algo/src/sharpen.rs` (`unsharp`: the unsharp formula
//! with no threshold) and `crates/algo/src/blur.rs` (`gaussian_kernel`), Film Grain from
//! `crates/algo/src/noise.rs` (`add`: monochrome, its amount scale), all at e4b2868a (MIT OR
//! Apache-2.0; `NOTICE`). Dalica's changes: the sigma relative to the picture instead of pixels;
//! band rows convolved from the 8-bit picture before the pass; smooth value noise in the stored
//! frame ([`fx::Noise`]) instead of a per-pixel hash; the Lighten half. Glow is our own design from
//! `editing.md` §8.2.

use std::fmt;

use super::{Adjust, Band, Operation, ParamError, Pass, Prepare, Reach, Reader, UNIT, clamp, flag, num};
use crate::fx::{self, Blurred, Gaussian, Noise, Space};

/// The range of Sharpen's Amount.
pub const SHARPEN: (f64, f64) = (0.0, 3.0);
/// The range of Film Grain's Grain.
pub const GRAIN: (f64, f64) = (0.0, 50.0);
/// The range of Glow's Intensity.
pub const GLOW_INTENSITY: (f64, f64) = UNIT;
/// The range of Glow's Radius, percent of the shorter side (a logarithmic slider).
pub const GLOW_RADIUS: (f64, f64) = (0.3, 30.0);

/// Sharpen's Gaussian sigma, relative to the stored photo's shorter side.
pub const SHARPEN_SIGMA: f64 = 0.0004;
/// The least sigma Sharpen uses, in output pixels: where the picture's own sigma is smaller (a
/// render under 1:1 of a photo shorter than 1250 px) the mask sharpens at the render's pixel scale.
pub const MIN_SIGMA_PX: f64 = 0.5;
/// The widest Sharpen kernel's radius, in output pixels (a bound for hostile geometry).
pub const MAX_KERNEL_RADIUS: u32 = 1024;
/// Film Grain's lattice cell, relative to the stored photo's shorter side (1.5 px on a 12 MP photo).
pub const GRAIN_CELL: f64 = 0.0005;
/// The mean of the light half of [`fx::Noise`]'s unit field, `E[max(n, 0)]` (measured over 16 M
/// points): what Lighten's grain averages to where a pixel spans many cells.
pub const LIGHT_MEAN: f32 = 0.418;
/// The luma from which a pixel starts to glow.
pub const GLOW_FROM: f64 = 0.5;

/// `sharpen(a)`: an unsharp mask, accurate at 1:1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sharpen {
    pub amount: f64,
}

impl Default for Sharpen {
    fn default() -> Sharpen {
        Sharpen { amount: 0.6 }
    }
}

/// `grain(grain,lighten)`: monochrome film grain; with `lighten`, only light grain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    pub grain: f64,
    pub lighten: bool,
}

impl Default for Grain {
    fn default() -> Grain {
        Grain { grain: 10.0, lighten: false }
    }
}

/// `glow(i,r)`: the highlights bloom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    pub intensity: f64,
    /// Percent of the shorter side.
    pub radius: f64,
}

impl Default for Glow {
    fn default() -> Glow {
        Glow { intensity: 0.65, radius: 3.0 }
    }
}

impl fmt::Display for Sharpen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.amount))
    }
}

impl Operation for Sharpen {
    const NAME: &'static str = "sharpen";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Sharpen";

    fn parse(params: &str) -> Result<Sharpen, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let s = Sharpen { amount: r.number("the amount", SHARPEN)? };
        r.end()?;
        Ok(s)
    }

    fn clamped(self) -> Sharpen {
        Sharpen { amount: clamp(self.amount, SHARPEN) }
    }
}

impl fmt::Display for Grain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", num(self.grain), flag(self.lighten))
    }
}

impl Operation for Grain {
    const NAME: &'static str = "grain";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Film Grain";

    fn parse(params: &str) -> Result<Grain, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let g = Grain { grain: r.number("the grain", GRAIN)?, lighten: r.flag("lighten")? };
        r.end()?;
        Ok(g)
    }

    fn clamped(self) -> Grain {
        Grain { grain: clamp(self.grain, GRAIN), ..self }
    }
}

impl fmt::Display for Glow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", num(self.intensity), num(self.radius))
    }
}

impl Operation for Glow {
    const NAME: &'static str = "glow";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Glow";

    fn parse(params: &str) -> Result<Glow, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let g = Glow { intensity: r.number("the intensity", GLOW_INTENSITY)?, radius: r.number("the radius", GLOW_RADIUS)? };
        r.end()?;
        Ok(g)
    }

    fn clamped(self) -> Glow {
        Glow { intensity: clamp(self.intensity, GLOW_INTENSITY), radius: clamp(self.radius, GLOW_RADIUS) }
    }
}

impl Adjust for Sharpen {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let amount = clamp(self.amount, SHARPEN);
        if amount <= 0.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let kernel = kernel(space.length(SHARPEN_SIGMA).max(MIN_SIGMA_PX))?;
        let radius = u32::try_from(kernel.len() / 2).ok()?;
        Some(Box::new(SharpenPass { amount: amount as f32, kernel, radius }))
    }
}

/// A sampled, normalised Gaussian of `sigma` pixels, `2r + 1` taps for `r = ⌈3σ⌉`; `None` when the
/// sigma is under [`MIN_SIGMA_PX`] or not a number, or the radius over [`MAX_KERNEL_RADIUS`].
fn kernel(sigma: f64) -> Option<Vec<f32>> {
    if sigma.is_nan() || sigma < MIN_SIGMA_PX {
        return None;
    }
    let r = (sigma * 3.0).ceil();
    if !r.is_finite() || r > f64::from(MAX_KERNEL_RADIUS) {
        return None;
    }
    let r = r as i64;
    let k: Vec<f64> = (-r..=r).map(|i| (-((i * i) as f64) / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f64 = k.iter().sum();
    (sum > 0.0).then(|| k.into_iter().map(|v| (v / sum) as f32).collect())
}

/// The unsharp mask over a band, reading the picture before it.
struct SharpenPass {
    amount: f32,
    kernel: Vec<f32>,
    radius: u32,
}

impl Pass for SharpenPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.radius)
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(before) = band.before else { return };
        let (w, rows, r) = (band.width as usize, band.rows as usize, self.radius as usize);
        let Some(lines) = rows.checked_add(2 * r) else { return };
        let Some(padded) = w.checked_add(2 * r) else { return };
        let Some(mut across) = alloc(w.saturating_mul(lines)) else { return };
        let Some(mut line) = alloc(padded) else { return };
        let (x0, y0) = (i64::from(band.x0), i64::from(band.y0));
        let ri = r as i64;
        // Each row the band's blur reads, blurred across (in a fixed tap order).
        for (j, out) in across.chunks_exact_mut(w.max(1)).enumerate() {
            let y = y0 - ri + j as i64;
            for (i, p) in line.iter_mut().enumerate() {
                *p = before.get(x0 - ri + i as i64, y);
            }
            for (x, o) in out.iter_mut().enumerate() {
                *o = convolve(&self.kernel, |k| line.get(x + k).copied().unwrap_or([0.0; 3]));
            }
        }
        // Then down, and the mask added.
        for y in 0..rows {
            for x in 0..w {
                let blur = convolve(&self.kernel, |k| across.get((y + k) * w + x).copied().unwrap_or([0.0; 3]));
                let orig = before.get(x0 + x as i64, y0 + y as i64);
                if let Some(p) = band.pixels.get_mut(y * w + x) {
                    *p = std::array::from_fn(|c| (orig[c] + self.amount * (orig[c] - blur[c])).clamp(0.0, 1.0));
                }
            }
        }
    }
}

/// `Σ kernel[k] · at(k)` per channel, in tap order.
fn convolve(kernel: &[f32], at: impl Fn(usize) -> [f32; 3]) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    for (k, &weight) in kernel.iter().enumerate() {
        let p = at(k);
        for c in 0..3 {
            sum[c] += weight * p[c];
        }
    }
    sum
}

fn alloc(n: usize) -> Option<Vec<[f32; 3]>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, [0.0; 3]);
    Some(out)
}

impl Adjust for Grain {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let grain = clamp(self.grain, GRAIN);
        if grain <= 0.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let seed = fx::noise::seed(&format!("{}({})", Self::NAME, self.clamped()));
        let spread = (grain / 100.0 * 0.5 / 3f64.sqrt()) as f32;
        let noise = Noise::new(seed, GRAIN_CELL);
        let fading = noise.attenuation(&space);
        Some(Box::new(GrainPass { space, noise, spread, fading, lighten: self.lighten }))
    }
}

/// Monochrome grain, a function of the output pixel's place in the stored frame.
struct GrainPass {
    space: Space,
    noise: Noise,
    spread: f32,
    /// [`Noise::attenuation`] in this render.
    fading: f32,
    lighten: bool,
}

impl Pass for GrainPass {
    fn run(&self, band: &mut Band<'_>) {
        let w = band.width.max(1) as usize;
        for (i, p) in band.pixels.iter_mut().enumerate() {
            let x = band.x0.saturating_add(u32::try_from(i % w).unwrap_or(u32::MAX));
            let y = band.y0.saturating_add(u32::try_from(i / w).unwrap_or(u32::MAX));
            let (u, v) = self.space.pixel_units(x, y);
            let n = self.noise.value(u, v, 0);
            // The light half's mean stays as its variation fades (never below 0: it only lightens).
            let n = if self.lighten { LIGHT_MEAN + (n.max(0.0) - LIGHT_MEAN) * self.fading } else { n * self.fading };
            let d = n * self.spread;
            *p = p.map(|v| (v + d).clamp(0.0, 1.0));
        }
    }
}

impl Adjust for Glow {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let g = self.clamped();
        if g.intensity <= 0.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let gaussian = Gaussian::new(space.length(g.radius / 100.0), space.size());
        Some(Box::new(GlowPass { blurred: Blurred::new(gaussian), intensity: g.intensity as f32 }))
    }
}

/// The blurred highlights screened onto the picture.
struct GlowPass {
    blurred: Blurred,
    intensity: f32,
}

/// The part of `p` that glows: `p` weighted by its luma's smoothstep from [`GLOW_FROM`] to 1.
fn highlight(p: [f32; 3]) -> [f32; 3] {
    let k = fx::smoothstep(GLOW_FROM, 1.0, f64::from(fx::luma(p))) as f32;
    p.map(|v| v * k)
}

impl Pass for GlowPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.blurred.gaussian().reach())
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(before) = band.before else { return };
        let Some(plane) = self.blurred.get(before, highlight) else { return };
        let w = band.width.max(1) as usize;
        let (x0, y0) = (i64::from(band.x0), i64::from(band.y0));
        for (i, p) in band.pixels.iter_mut().enumerate() {
            let glow = plane.get(x0 + (i % w) as i64, y0 + (i / w) as i64);
            // Screen, `1 − (1 − p)(1 − t)`, written as `p + t (1 − p)` so it never darkens.
            *p = std::array::from_fn(|c| {
                let t = (self.intensity * glow[c]).clamp(0.0, 1.0);
                let v = p[c].clamp(0.0, 1.0);
                v + t * (1.0 - v)
            });
        }
    }
}
