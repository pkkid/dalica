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
//!   is as much lighter as the full picture shrunk. The fading stops at [`MIN_FADING`], so a render
//!   well under 1:1 (an effect tile) still shows grain: an approximation, close to the large render
//!   shrunk at the default grain. Grain 0 changes nothing.
//! - **Glow** (M12.7): the highlights (each pixel weighted by a smoothstep of its luma from
//!   [`GLOW_FROM`] to 1) blurred by a Gaussian whose sigma is Radius percent of the shorter side,
//!   then screened onto the picture by Intensity: it never darkens a pixel, and a picture darker
//!   than [`GLOW_FROM`] everywhere is left as it was. Intensity 0 changes nothing.
//! - **Speed** (M13.36, the slider's 16 ms draft and 100 ms final): Sharpen reads the band's rows
//!   and their margin into working values once and runs both passes on flat slices in strips of
//!   [`SHARPEN_STRIP`] pixels, its taps in the same order (the output bit for bit M12's). Film
//!   Grain draws its noise a run of [`GRAIN_RUN`] pixels at a time ([`fx::Noise::row_values`]),
//!   each lattice corner hashed once along a run that stays on one row of cells (bit for bit the
//!   same values). Glow's blur is a [`fx::GridBlur`] made once per render (the highlights weighed
//!   before the cells are averaged), whose whole sigma is the closest a grid makes to Radius's,
//!   read back a row at a time; within the fingerprints' tolerance of M12's full-size blur.
//!
//! Sharpen ported from PhotoCraft `crates/algo/src/sharpen.rs` (`unsharp`: the unsharp formula
//! with no threshold) and `crates/algo/src/blur.rs` (`gaussian_kernel`), Film Grain from
//! `crates/algo/src/noise.rs` (`add`: monochrome, its amount scale), all at e4b2868a (MIT OR
//! Apache-2.0; `NOTICE`). Dalica's changes: the sigma relative to the picture instead of pixels;
//! band rows convolved from the 8-bit picture before the pass; smooth value noise in the stored
//! frame ([`fx::Noise`]) instead of a per-pixel hash; the Lighten half. Glow is our own design from
//! `editing.md` §8.2.

use std::fmt;

use super::{Adjust, Band, Operation, ParamError, Pass, Prepare, Reach, Reader, UNIT, View, clamp, flag, num};
use crate::fx::noise::hash01;
use crate::fx::{self, GridBlur, Noise, Space, Window};

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
/// The least share of Film Grain's variation a render keeps where its pixels span many grain cells
/// (a render well under 1:1, a tile): the grain still shows, as a preview should, instead of
/// averaging away; an approximation, close to the large render shrunk at the default grain.
pub const MIN_FADING: f32 = 0.25;
/// The luma from which a pixel starts to glow.
pub const GLOW_FROM: f64 = 0.5;
/// Sharpen blurs a band in strips of this many pixels (a strip of its rows stays in the cache).
pub const SHARPEN_STRIP: usize = 128;
/// Film Grain draws its noise in runs of this many pixels along a row, anchored at its multiples
/// in the whole output ([`Noise::row_values`]).
pub const GRAIN_RUN: u32 = 256;

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
        let Some(line_len) = w.checked_add(2 * r).and_then(|n| n.checked_mul(3)) else { return };
        if w == 0 || rows == 0 {
            return;
        }
        // Every row the band's blur reads, padded by the radius on both sides (the picture's edges
        // and then the window's clamped as `View::get` clamps them), in working values.
        let Some(columns) = view_columns(before, i64::from(band.x0) - r as i64, w + 2 * r) else { return };
        let Some(mut src) = alloc(line_len.saturating_mul(lines)) else { return };
        for (j, line) in src.chunks_exact_mut(line_len).enumerate() {
            view_row(before, i64::from(band.y0) - r as i64 + j as i64, &columns, line);
        }
        // Then strips of `SHARPEN_STRIP` pixels: each line's strip blurred across, then down, and
        // the mask added to the pixel before the pass (the band holds it: the same 8-bit value in
        // working precision). Per output value the taps are added in their order
        // (`Σ kernel[k] · v[k]` from 0) across and then down, so the sums are bit for bit those of
        // a tap-by-tap convolution; a strip stays in the cache and the loops run on flat slices.
        let strip = SHARPEN_STRIP * 3;
        let Some(mut across) = alloc(strip.saturating_mul(lines)) else { return };
        let Some(mut blur) = alloc(strip) else { return };
        for x0 in (0..w).step_by(SHARPEN_STRIP) {
            let n = SHARPEN_STRIP.min(w - x0) * 3;
            for (line, out) in src.chunks_exact(line_len).zip(across.chunks_exact_mut(strip)) {
                let (Some(line), Some(out)) = (line.get(x0 * 3..), out.get_mut(..n)) else { return };
                taps(&self.kernel, line, 3, out);
            }
            for (y, px) in band.pixels.chunks_exact_mut(w).take(rows).enumerate() {
                let (Some(rows), Some(b)) = (across.get(y * strip..), blur.get_mut(..n)) else { return };
                taps(&self.kernel, rows, strip, b);
                let Some(px) = px.as_flattened_mut().get_mut(x0 * 3..x0 * 3 + n) else { return };
                for (p, &b) in px.iter_mut().zip(b.iter()) {
                    *p = (*p + self.amount * (*p - b)).clamp(0.0, 1.0);
                }
            }
        }
    }
}

/// `out[i] = Σ kernel[k] · src[k · stride + i]`, the taps added in their order from 0 (the same
/// sums a tap-by-tap convolution makes), [`LANES`] outputs at a time held in registers.
fn taps(kernel: &[f32], src: &[f32], stride: usize, out: &mut [f32]) {
    let (head, tail) = out.as_chunks_mut::<LANES>();
    for (c, o) in head.iter_mut().enumerate() {
        let mut acc = [0.0f32; LANES];
        for (k, &weight) in kernel.iter().enumerate() {
            let at = k * stride + c * LANES;
            let Some(v) = src.get(at..at + LANES).and_then(|v| <&[f32; LANES]>::try_from(v).ok()) else { return };
            for (a, &p) in acc.iter_mut().zip(v) {
                *a += weight * p;
            }
        }
        *o = acc;
    }
    let done = head.len() * LANES;
    for (i, o) in tail.iter_mut().enumerate() {
        let mut acc = 0.0f32;
        for (k, &weight) in kernel.iter().enumerate() {
            acc += weight * src.get(k * stride + done + i).copied().unwrap_or(0.0);
        }
        *o = acc;
    }
}

/// The outputs [`taps`] sums at once.
const LANES: usize = 16;

/// For `n` output columns from `x0`, the byte offset in `view`'s rows of the pixel each reads,
/// clamped into the picture and then into the window as [`View::get`] clamps.
fn view_columns(view: &View<'_>, x0: i64, n: usize) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    let last = i64::from(view.full_width.max(1)) - 1;
    let wlast = i64::from(view.width.max(1)) - 1;
    out.extend((0..n as i64).map(|i| {
        let x = (x0 + i).clamp(0, last);
        (x - i64::from(view.x0)).clamp(0, wlast) as usize * 4
    }));
    Some(out)
}

/// The output row `y` of `view` at `columns` ([`view_columns`]) as working values, three per
/// pixel, into `out` (the values [`View::get`] reads; black where the view is short).
fn view_row(view: &View<'_>, y: i64, columns: &[usize], out: &mut [f32]) {
    let y = y.clamp(0, i64::from(view.full_height.max(1)) - 1);
    let ly = (y - i64::from(view.y0)).clamp(0, i64::from(view.height.max(1)) - 1) as usize;
    let w = view.width as usize * 4;
    let row = view.data.get(ly * w..(ly + 1) * w).unwrap_or(&[]);
    let out = out.as_chunks_mut::<3>().0;
    // The columns between the clamped ends follow each other: read straight along the row there.
    let (first, last) = (columns.first().copied().unwrap_or(0), columns.last().copied().unwrap_or(0));
    let lead = columns.iter().take_while(|&&c| c == first).count().saturating_sub(1);
    let tail = columns.iter().rev().take_while(|&&c| c == last).count().saturating_sub(1);
    let middle = columns.len().saturating_sub(lead + tail);
    let start = columns.get(lead).copied().unwrap_or(0);
    let straight =
        columns.get(lead..lead + middle).is_some_and(|m| m.iter().enumerate().all(|(i, &c)| c == start + 4 * i)) && row.len() >= start + 4 * middle;
    let px = |at: usize| match row.get(at..at + 3) {
        Some(&[r, g, b]) => [UNIT_VALUE[usize::from(r)], UNIT_VALUE[usize::from(g)], UNIT_VALUE[usize::from(b)]],
        _ => [0.0; 3],
    };
    if !straight {
        for (o, &at) in out.iter_mut().zip(columns) {
            *o = px(at);
        }
        return;
    }
    for (o, &at) in out.iter_mut().zip(columns).take(lead) {
        *o = px(at);
    }
    let bytes = row.get(start..start + 4 * middle).unwrap_or(&[]);
    for (o, b) in out.iter_mut().skip(lead).zip(bytes.as_chunks::<4>().0) {
        *o = [UNIT_VALUE[usize::from(b[0])], UNIT_VALUE[usize::from(b[1])], UNIT_VALUE[usize::from(b[2])]];
    }
    for (o, &at) in out.iter_mut().zip(columns).skip(lead + middle) {
        *o = px(at);
    }
}

/// `v / 255` for every 8-bit value, as `f32` (the values the division gives, looked up).
const UNIT_VALUE: [f32; 256] = {
    let mut t = [0.0f32; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = i as f32 / 255.0;
        i += 1;
    }
    t
};

fn alloc(n: usize) -> Option<Vec<f32>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, 0.0);
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
        let fading = noise.attenuation(&space).max(MIN_FADING);
        Some(Box::new(GrainPass { space, noise, seed, spread, fading, lighten: self.lighten }))
    }
}

/// Monochrome grain, a function of the output pixel's place in the stored frame.
struct GrainPass {
    space: Space,
    noise: Noise,
    /// The noise's seed.
    seed: u32,
    spread: f32,
    /// [`Noise::attenuation`] in this render.
    fading: f32,
    lighten: bool,
}

impl Pass for GrainPass {
    fn run(&self, band: &mut Band<'_>) {
        let w = band.width as usize;
        if w == 0 {
            return;
        }
        let mut noise = [0.0f32; GRAIN_RUN as usize];
        let mut corners = Corners::default();
        let x1 = band.x0.saturating_add(band.width);
        for (j, row) in band.pixels.chunks_exact_mut(w).enumerate() {
            let y = band.y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
            // The row in runs anchored at multiples of `GRAIN_RUN` in the whole output, so a
            // region's pixels draw exactly the whole render's values.
            let mut start = band.x0 - band.x0 % GRAIN_RUN;
            while start < x1 {
                let end = start.saturating_add(GRAIN_RUN).min(x1);
                let Some(run) = noise.get_mut(..(end - start) as usize) else { break };
                self.noise_run(start, y, run, &mut corners);
                let skip = band.x0.saturating_sub(start) as usize;
                let at = start.max(band.x0) - band.x0;
                let Some(pixels) = row.get_mut(at as usize..(end - band.x0) as usize) else { break };
                let run = run.get_mut(skip..).unwrap_or(&mut []);
                // The light half's mean stays as its variation fades (never below 0: it only
                // lightens).
                if self.lighten {
                    for n in run.iter_mut() {
                        *n = (LIGHT_MEAN + (n.max(0.0) - LIGHT_MEAN) * self.fading) * self.spread;
                    }
                } else {
                    for n in run.iter_mut() {
                        *n = *n * self.fading * self.spread;
                    }
                }
                for (p, &d) in pixels.iter_mut().zip(run.iter()) {
                    *p = p.map(|v| (v + d).clamp(0.0, 1.0));
                }
                start = end;
            }
        }
    }
}

/// The lattice corners [`GrainPass::noise_run`] hashes along one run at most: two lattice cells a
/// pixel, and the corner past the last.
const CORNERS: usize = 2 * GRAIN_RUN as usize + 3;

/// [`GrainPass::noise_run`]'s scratch: the hashed corners of two lattice rows and, per cell, its
/// top-left corner, the step to the top-right one and the same below.
struct Corners {
    hashes: [[f32; CORNERS]; 2],
    cells: [[f32; 4]; CORNERS],
}

impl Default for Corners {
    fn default() -> Corners {
        Corners { hashes: [[0.0; CORNERS]; 2], cells: [[0.0; 4]; CORNERS] }
    }
}

impl GrainPass {
    /// [`Noise::row_values`] for the pixels `(start, y) … (start + out.len() − 1, y)` (at most
    /// [`GRAIN_RUN`]), bit for bit, into `out`; `scratch` holds the run's corners.
    ///
    /// Where the run stays on one row of lattice cells and steps forward by at most two cells a
    /// pixel (a render that is not turned or mirrored, its shorter side at least 1 000 pixels: the
    /// edit view's final and a Save), each lattice corner along it is hashed once, in a loop that
    /// vectorises, rather than four per pixel; the stored-frame point, the cell and the smoothstep
    /// blend are those `row_values` computes. Elsewhere it is `row_values` itself. Answers whether
    /// the run took the fast path.
    fn noise_run(&self, start: u32, y: u32, out: &mut [f32], scratch: &mut Corners) -> bool {
        // `row_values`'s affine step along the row.
        const STRIDE: u32 = 1024;
        let cell = self.noise.cell();
        let (u0, v0) = self.space.pixel_units(start, y);
        let (u1, v1) = self.space.pixel_units(start.saturating_add(STRIDE), y);
        let span = f64::from(start.saturating_add(STRIDE) - start).max(1.0);
        let (x, v) = (u0 / cell, v0 / cell);
        let (dx, dv) = ((u1 - u0) / span / cell, (v1 - v0) / span / cell);
        let last = out.len().saturating_sub(1) as f64;
        let (x_end, v_end) = (x + dx * last, v + dv * last);
        let inside = |t: f64| t.is_finite() && t.abs() < 1e9;
        let row = lattice_floor(v);
        let fast = !out.is_empty()
            && out.len() <= GRAIN_RUN as usize
            && [x, v, x_end, v_end].into_iter().all(inside)
            && dx > 0.0
            && dx <= 2.0
            && lattice_floor(v_end) == row;
        if !fast {
            self.noise.row_values(&self.space, start, y, 0, out);
            return false;
        }
        let Corners { hashes, cells } = scratch;
        // The two lattice rows' corners from the first pixel's cell to past the last's, hashed
        // once; then per cell its top-left corner and the step to the top-right one, and the same
        // below (the differences the blend takes, made once per cell).
        let first = lattice_floor(x);
        let count = usize::try_from(lattice_floor(x_end) - first + 2).unwrap_or(0).min(CORNERS);
        let [top, bottom] = hashes;
        let (top, bottom) = (top.get_mut(..count).unwrap_or(&mut []), bottom.get_mut(..count).unwrap_or(&mut []));
        let (j0, j1) = (row as i32, (row + 1) as i32);
        for (k, (t, b)) in top.iter_mut().zip(bottom.iter_mut()).enumerate() {
            let i = (first as i32).wrapping_add(k as i32);
            *t = 2.0 * hash01(i, j0, 0, self.seed) - 1.0;
            *b = 2.0 * hash01(i, j1, 0, self.seed) - 1.0;
        }
        for (c, ((t, t1), (b, b1))) in cells.iter_mut().zip(top.iter().zip(top.iter().skip(1)).zip(bottom.iter().zip(bottom.iter().skip(1)))) {
            *c = [*t, *t1 - *t, *b, *b1 - *b];
        }
        let spread = fx::noise::SPREAD as f32;
        let (first, row) = (first as f64, row as f64);
        let smooth = |f: f32| f * f * (3.0 - 2.0 * f);
        // The pixels step forward along the run, so each one's cell is found by stepping past
        // the lattice lines it crossed (exact integers in `f64`: `⌊px⌋` itself).
        let (mut at, mut next) = (0usize, first + 1.0);
        let mut blend = |o: &mut f32, px: f64, sv: f32| {
            while px >= next {
                next += 1.0;
                at += 1;
            }
            let su = smooth((px - (next - 1.0)) as f32);
            let [t0, dt, b0, db] = cells.get(at).copied().unwrap_or([0.0; 4]);
            let (t, b) = (t0 + dt * su, b0 + db * su);
            *o = (t + (b - t) * sv) / spread;
        };
        if dv == 0.0 {
            // On a row of the stored frame the run's place between the lattice rows is the same
            // at every pixel.
            let sv = smooth((v - row) as f32);
            for (k, o) in out.iter_mut().enumerate() {
                blend(o, x + dx * k as f64, sv);
            }
        } else {
            for (k, o) in out.iter_mut().enumerate() {
                let k = k as f64;
                blend(o, x + dx * k, smooth((v + dv * k - row) as f32));
            }
        }
        true
    }
}

/// `⌊v⌋` of a finite `v` inside ±10⁹, as the noise's row sampler takes it.
fn lattice_floor(v: f64) -> i64 {
    let t = v as i64;
    if (t as f64) > v { t - 1 } else { t }
}

impl Adjust for Glow {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let g = self.clamped();
        if g.intensity <= 0.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let blur = glow_blur(space.length(g.radius / 100.0), space.size());
        Some(Box::new(GlowPass { blur, intensity: g.intensity as f32 }))
    }
}

/// A [`GridBlur`] whose whole sigma ([`GridBlur::sigma`]) comes closest to `sigma`.
fn glow_blur(sigma: f64, size: (u32, u32)) -> Option<GridBlur> {
    let off = |b: &GridBlur| (b.sigma() - sigma).abs();
    let mut best = GridBlur::new(sigma, size)?;
    for k in -10..=25 {
        let Some(next) = GridBlur::new(sigma * (1.0 + f64::from(k) * 0.01), size) else { continue };
        if off(&next) < off(&best) {
            best = next;
        }
    }
    Some(best)
}

/// The blurred highlights screened onto the picture.
struct GlowPass {
    /// `None` where the blur is too small to change anything: the highlights screened as they are.
    blur: Option<GridBlur>,
    intensity: f32,
}

/// The part of `p` that glows: `p` weighted by its luma's smoothstep from [`GLOW_FROM`] to 1 (in
/// `f32`; a luma that is not a number weighs 0).
fn highlight(p: [f32; 3]) -> [f32; 3] {
    let from = GLOW_FROM as f32;
    let t = ((fx::luma(p) - from) / (1.0 - from)).clamp(0.0, 1.0);
    let t = if t.is_nan() { 0.0 } else { t };
    let k = t * t * (3.0 - 2.0 * t);
    p.map(|v| v * k)
}

impl GlowPass {
    /// Screens the blurred highlights `glow` onto the row `row` by the intensity (three values a
    /// pixel each): `1 − (1 − p)(1 − t)`, written as `p + t (1 − p)` so it never darkens.
    fn screen(&self, row: &mut [f32], glow: &[f32]) {
        for (p, &g) in row.iter_mut().zip(glow) {
            let t = (self.intensity * g).clamp(0.0, 1.0);
            let v = p.clamp(0.0, 1.0);
            *p = v + t * (1.0 - v);
        }
    }
}

impl Pass for GlowPass {
    fn reach(&self) -> Reach {
        self.blur.as_ref().map_or(Reach::Pixel, |b| Reach::Pixels(b.reach()))
    }

    fn run(&self, band: &mut Band<'_>) {
        let w = band.width as usize;
        let Some(mut glow) = alloc(w.saturating_mul(3)) else { return };
        let Some(blur) = &self.blur else {
            for row in band.pixels.chunks_exact_mut(w.max(1)) {
                for (g, p) in glow.as_chunks_mut::<3>().0.iter_mut().zip(row.iter()) {
                    *g = highlight(*p);
                }
                self.screen(row.as_flattened_mut(), &glow);
            }
            return;
        };
        let Some(window) = band.before.and_then(Window::of) else { return };
        let Some(grid) = blur.rgb_weighted(window, highlight) else { return };
        let Some(mut reader) = GridRows::new(blur, grid, band.x0, w) else { return };
        let mixed = blur.strength() < 1.0;
        for (j, row) in band.pixels.chunks_exact_mut(w.max(1)).enumerate() {
            let y = band.y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
            let g = glow.as_chunks_mut::<3>().0;
            match &mut reader {
                GridRows::Pixels => blur.read_row(grid, band.x0, y, g),
                GridRows::Cells(r) => r.read(blur, grid, y, g),
            }
            if mixed {
                // A blur under the smallest box blur is mixed in by its share.
                for (g, p) in g.iter_mut().zip(row.iter()) {
                    *g = blur.mix(highlight(*p), *g);
                }
            }
            self.screen(row.as_flattened_mut(), &glow);
        }
    }
}

/// [`GridBlur::read_row`] for every row of a band, made faster for a grid of cells: the place of
/// each pixel between its two grid columns is found once per band, and each row lerps the two grid
/// rows into one before reading across it (the same bilinear read, its two lerps in the other
/// order).
enum GridRows {
    /// The grid is the pixels: [`GridBlur::read_row`] itself.
    Pixels,
    Cells(CellRows),
}

struct CellRows {
    /// Per pixel of the row, the grid's two columns (indices into a grid row) and the weight of
    /// the second.
    taps: Vec<(u32, u32, f32)>,
    /// One grid row, lerped.
    line: Vec<[f32; 3]>,
}

impl GridRows {
    fn new(blur: &GridBlur, grid: &fx::Plane, x0: u32, w: usize) -> Option<GridRows> {
        if blur.cell() <= 1 {
            return Some(GridRows::Pixels);
        }
        let last = i64::from(grid.width.max(1)) - 1;
        let at = |i: i64| (i - i64::from(grid.x0)).clamp(0, last) as u32;
        let mut taps = Vec::new();
        taps.try_reserve_exact(w).ok()?;
        taps.extend((0..w).map(|k| {
            let (i, t) = cell_tap(blur.cell(), x0.saturating_add(u32::try_from(k).unwrap_or(u32::MAX)));
            (at(i), at(i + 1), t)
        }));
        let mut line = Vec::new();
        line.try_reserve_exact(grid.width as usize).ok()?;
        line.resize(grid.width as usize, [0.0; 3]);
        Some(GridRows::Cells(CellRows { taps, line }))
    }
}

impl CellRows {
    /// The blur at the pixels of the output row `y` from the band's left, into `out`.
    fn read(&mut self, blur: &GridBlur, grid: &fx::Plane, y: u32, out: &mut [[f32; 3]]) {
        let (j, ty) = cell_tap(blur.cell(), y);
        let gw = grid.width as usize;
        let last = i64::from(grid.height.max(1)) - 1;
        let row = |j: i64| {
            let lj = (j - i64::from(grid.y0)).clamp(0, last) as usize;
            grid.data.get(lj * gw..(lj + 1) * gw).unwrap_or(&[])
        };
        for ((l, a), b) in self.line.iter_mut().zip(row(j)).zip(row(j + 1)) {
            *l = fx::mix(*a, *b, ty);
        }
        for (o, &(i0, i1, t)) in out.iter_mut().zip(&self.taps) {
            let (a, b) = (self.line.get(i0 as usize).copied().unwrap_or([0.0; 3]), self.line.get(i1 as usize).copied().unwrap_or([0.0; 3]));
            *o = fx::mix(a, b, t);
        }
    }
}

/// The grid cell before the pixel coordinate `p` along one axis, for cells of `cell` pixels, and
/// the weight of the next (as [`GridBlur::read`] places a pixel's centre).
fn cell_tap(cell: u32, p: u32) -> (i64, f32) {
    let u = (f64::from(p) + 0.5) / f64::from(cell.max(1)) - 0.5;
    // `⌊u⌋` (`u` is above −0.5).
    let i = u as i64 - i64::from(u < 0.0);
    (i, (u - i as f64) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Recipe, render::Geometry};
    use dalica_image::Orientation;

    /// A small xorshift for the tests' pictures and places.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u32) -> u32 {
            (self.next() % u64::from(n.max(1))) as u32
        }
    }

    /// M12's unsharp mask, tap by tap through `View::get`: the reference the rows must equal.
    fn tap_by_tap(pass: &SharpenPass, before: &View<'_>, x0: u32, y0: u32, w: usize, rows: usize) -> Vec<[f32; 3]> {
        let r = pass.radius as usize;
        let convolve = |at: &dyn Fn(usize) -> [f32; 3]| {
            let mut sum = [0.0f32; 3];
            for (k, &weight) in pass.kernel.iter().enumerate() {
                let p = at(k);
                for c in 0..3 {
                    sum[c] += weight * p[c];
                }
            }
            sum
        };
        let (x0, y0, ri) = (i64::from(x0), i64::from(y0), r as i64);
        let mut across = vec![[0.0f32; 3]; w * (rows + 2 * r)];
        for (j, out) in across.chunks_exact_mut(w).enumerate() {
            let y = y0 - ri + j as i64;
            let line: Vec<[f32; 3]> = (0..w + 2 * r).map(|i| before.get(x0 - ri + i as i64, y)).collect();
            for (x, o) in out.iter_mut().enumerate() {
                *o = convolve(&|k| line[x + k]);
            }
        }
        let mut out = Vec::new();
        for y in 0..rows {
            for x in 0..w {
                let blur = convolve(&|k| across[(y + k) * w + x]);
                let orig = before.get(x0 + x as i64, y0 + y as i64);
                out.push(std::array::from_fn(|c| (orig[c] + pass.amount * (orig[c] - blur[c])).clamp(0.0, 1.0)));
            }
        }
        out
    }

    /// Sharpen's flat rows and strips give bit for bit the tap-by-tap mask: over the whole picture
    /// and region windows, bands at its edges and inside, strips that end short, and kernels of
    /// several radii.
    #[test]
    fn sharpen_rows_equal_the_tap_by_tap_mask() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let (fw, fh) = (SHARPEN_STRIP as u32 * 2 + 37, 53u32);
        for (sigma, amount) in [(0.5, 0.6), (0.8, 3.0), (1.7, 1.2)] {
            let kernel = kernel(sigma).unwrap();
            let pass = SharpenPass { amount, radius: (kernel.len() / 2) as u32, kernel };
            // The whole picture, then windows a region render holds (with a margin that is cut at
            // the picture's edges).
            for (wx, wy, ww, wh) in [(0, 0, fw, fh), (11, 7, 150, 30), (fw - 90, fh - 20, 90, 20)] {
                let data: Vec<u8> = (0..ww * wh * 4).map(|_| rng.below(256) as u8).collect();
                let before = View { data: &data, x0: wx, y0: wy, width: ww, height: wh, full_width: fw, full_height: fh };
                for (bx, by, bw, rows) in [(wx, wy, ww, wh.min(32)), (wx + 3, wy + 2, ww - 5, 4), (wx, wy + wh - 1, ww, 1)] {
                    let (bw, rows) = (bw as usize, rows as usize);
                    let mut pixels: Vec<[f32; 3]> =
                        (0..rows * bw).map(|i| before.get(i64::from(bx) + (i % bw) as i64, i64::from(by) + (i / bw) as i64)).collect();
                    let mut band = Band { pixels: &mut pixels, width: bw as u32, rows: rows as u32, x0: bx, y0: by, before: Some(&before) };
                    pass.run(&mut band);
                    let want = tap_by_tap(&pass, &before, bx, by, bw, rows);
                    let bits = |v: &[[f32; 3]]| v.iter().flatten().map(|f| f.to_bits()).collect::<Vec<_>>();
                    assert_eq!(bits(&pixels), bits(&want), "sigma {sigma}, window ({wx}, {wy}), band ({bx}, {by}) {bw} × {rows}");
                }
            }
        }
    }

    /// Film Grain's runs give bit for bit `Noise::row_values`: the fast path where a run stays on
    /// one row of lattice cells (taken on an upright render at 1:1 and at half size), `row_values`
    /// itself on a straightened, turned or small render.
    #[test]
    fn grain_runs_equal_the_noise_rows() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let cases: [(&[&str], u8, u32, u32, f64); 5] = [
            (&[], 1, 2000, 1500, 1.0),
            (&[], 1, 4000, 3000, 0.5),
            (&["straighten(3)"], 1, 2000, 1500, 0.8),
            (&["crop(0.1,0.2,0.9,0.8)"], 6, 2000, 1500, 0.5),
            (&[], 1, 2000, 1500, 0.1),
        ];
        let mut fast = 0usize;
        for (ops, o, w, h, scale) in cases {
            let g = Geometry::new(&Recipe::parse(ops).unwrap(), Orientation::new(o).unwrap(), w, h);
            let (ow, oh) = g.output_size(scale).unwrap();
            let space = Space::from_geometry(&g, ow, oh).unwrap();
            let noise = Noise::new(77, GRAIN_CELL);
            let pass = GrainPass { space, noise, seed: 77, spread: 0.03, fading: 1.0, lighten: false };
            let mut corners = Corners::default();
            for _ in 0..200 {
                let (start, y) = (rng.below(ow), rng.below(oh));
                let n = (1 + rng.below(GRAIN_RUN)).min(ow - start) as usize;
                let (mut got, mut want) = (vec![0.0f32; n], vec![0.0f32; n]);
                fast += usize::from(pass.noise_run(start, y, &mut got, &mut corners));
                pass.noise.row_values(&pass.space, start, y, 0, &mut want);
                let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
                assert_eq!(bits(&got), bits(&want), "{ops:?} at {scale}: ({start}, {y}) × {n}");
            }
        }
        assert!(fast >= 300, "the fast path ran {fast} times");
    }

    /// Glow's grid is the blur of the asked sigma within 2 % (`glow_blur` picks it so), and its
    /// rows read it as `GridBlur::read` does, within float rounding.
    #[test]
    fn glow_reads_a_grid_of_its_sigma_as_grid_blur_does() {
        let (fw, fh) = (900u32, 600u32);
        let data: Vec<u8> = (0..fw * fh).flat_map(|i| [(i * 7 % 251) as u8, (i * 13 % 256) as u8, (i % 9 * 28) as u8, 255]).collect();
        let view = View { data: &data, x0: 0, y0: 0, width: fw, height: fh, full_width: fw, full_height: fh };
        for sigma in [7.0, 18.0, 60.0, 180.0] {
            let blur = glow_blur(sigma, (fw, fh)).unwrap();
            assert!((blur.sigma() / sigma - 1.0).abs() < 0.02, "{sigma}: {}", blur.sigma());
            assert!(blur.cell() > 1, "{sigma}: a grid of cells");
            let grid = blur.rgb_weighted(Window::of(&view).unwrap(), highlight).unwrap();
            let (x0, w) = (37u32, 700usize);
            let Some(GridRows::Cells(mut rows)) = GridRows::new(&blur, grid, x0, w) else { panic!("{sigma}: cells") };
            let mut got = vec![[0.0f32; 3]; w];
            for y in [0u32, 1, 299, 599] {
                rows.read(&blur, grid, y, &mut got);
                for (i, g) in got.iter().enumerate() {
                    let want = blur.read(grid, x0 + i as u32, y);
                    assert!((0..3).all(|c| (g[c] - want[c]).abs() < 1e-6), "{sigma}: ({}, {y}) {g:?} vs {want:?}", x0 + i as u32);
                }
            }
        }
    }
}
