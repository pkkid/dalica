//! The blur-based looks of tab 4 (`editing.md` §8.3): `hdr(radius,strength,fade)`,
//! `dream_glow(bloom,brightness,fade)` and `plastic_lens(blur,grain,fade)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5):
//!   - HDR Look: Radius 1.3–80 (20), the blur's radius in thousandths of the picture's shorter
//!     side; Strength 1–7 (3), the `C` of `out = in + C·(in − blur(r))`; Fade 0–100 (0).
//!   - Dream Glow: Bloom 0–50 (25), the blurred copy's radius in thousandths of the shorter side;
//!     Brightness 0–100 (50), the midtone lift; Fade 0–100 (0).
//!   - Plastic Lens: Blur Edges 0–100 (70), Grain 0–100 (30), Fade 0–100 (0).
//!
//!   Numbers out of range are clamped; `NaN` or a missing or extra field is refused (the entry is
//!   kept and skipped).
//! - **Rendering** (M12.10, `VERSION` 1), each faded toward its input by Fade
//!   ([`crate::fx::fade`]; Fade 100 changes nothing):
//!   - HDR Look: `out = in + C · (L − blur(L))` with `C` the Strength and `L` the luma
//!     ([`crate::fx::luma`]), the same amount added to every channel (local contrast without a
//!     colour shift); the blur a Gaussian of sigma Radius/1000 of the shorter side.
//!   - Dream Glow: the photo blurred by a Gaussian of sigma Bloom/1000 of the shorter side,
//!     blended over the photo in overlay ([`crate::fx::Blend::Overlay`]), then a midtone lift
//!     `v + l · v · (1 − v)` with `l = Brightness/100 ×` [`LIFT`] (black and white stay).
//!   - Plastic Lens: a Gaussian of sigma `Blur Edges/100 ×` [`EDGE_SIGMA`] of the shorter side
//!     mixed in from [`EDGE_FROM`] to the corners, a vignette darkening the corners by
//!     [`VIGNETTE`], and monochrome grain ([`crate::fx::Noise`], cell [`GRAIN_CELL`], seeded by the
//!     entry, placed in the stored frame) of `Grain/100 ×` [`GRAIN_AMOUNT`]; the soft edges and
//!     the vignette follow the output picture (after the crop): [`lens`].
//!
//! - **Speed** (M13.39: within the slider's caps, 16 ms for a draft and 100 ms for a final of the
//!   24 MP photo on the 2560 × 1600 screen, `development.md` §8). Every blur is a
//!   [`crate::fx::GridBlur`] (cells about a third of its sigma): on cells wider than a pixel its
//!   grid is made once per render for every band and each grid row a band's rows lie between is
//!   read across once ([`Rows::Grid`], [`crate::fx::GridRows`] since M13.47); on one-pixel cells (HDR Look's fine detail at the screen's
//!   sizes, the drafts' blurs) each band convolves its own rows and the kernel's reach with the
//!   kernel of the grid's box passes ([`Rows::Pixels`]), so no band waits on a full-resolution
//!   blur of the whole picture. Both are exact: a region render equals that window of the whole
//!   render. HDR Look blurs the luma alone (one channel, not three: the luma is linear) and eases
//!   its detail in through a branch-free `tanh` ([`tanh`]); Plastic Lens's grain is `Noise::row`'s,
//!   each lattice row blended across once per band where the stored frame lies square
//!   ([`crate::fx::Noise::lines`], M13.47). Each pass's per-pixel work runs on flat rows of channel values, every loop a
//!   function of its own so the compiler vectorises it.

use std::fmt;

use std::sync::OnceLock;

use rayon::prelude::*;

use super::{Adjust, Band, FADE, Operation, ParamError, Pass, Prepare, Reach, Reader, View, clamp, num};
use crate::fx::{Cells, GridBlur, GridRows, Noise, Space, Window, apart, luma, noise};

/// The range of HDR Look's Radius, thousandths of the shorter side.
pub const HDR_RADIUS: (f64, f64) = (1.3, 80.0);
/// The range of HDR Look's Strength.
pub const HDR_STRENGTH: (f64, f64) = (1.0, 7.0);
/// The range of Dream Glow's Bloom, thousandths of the shorter side.
pub const BLOOM: (f64, f64) = (0.0, 50.0);
/// The range of Dream Glow's Brightness.
pub const BRIGHTNESS: (f64, f64) = (0.0, 100.0);
/// The range of Plastic Lens's Blur Edges.
pub const BLUR_EDGES: (f64, f64) = (0.0, 100.0);
/// The range of Plastic Lens's Grain.
pub const GRAIN: (f64, f64) = (0.0, 100.0);

/// `hdr(radius,strength,fade)`: large-radius local contrast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hdr {
    /// Thousandths of the shorter side.
    pub radius: f64,
    pub strength: f64,
    pub fade: f64,
}

impl Default for Hdr {
    fn default() -> Hdr {
        Hdr { radius: 20.0, strength: 3.0, fade: 0.0 }
    }
}

/// `dream_glow(bloom,brightness,fade)`: a blurred copy blended in overlay and a midtone lift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DreamGlow {
    /// Thousandths of the shorter side.
    pub bloom: f64,
    pub brightness: f64,
    pub fade: f64,
}

impl Default for DreamGlow {
    fn default() -> DreamGlow {
        DreamGlow { bloom: 25.0, brightness: 50.0, fade: 0.0 }
    }
}

/// `plastic_lens(blur,grain,fade)`: soft edges, grain and a vignette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlasticLens {
    pub blur: f64,
    pub grain: f64,
    pub fade: f64,
}

impl Default for PlasticLens {
    fn default() -> PlasticLens {
        PlasticLens { blur: 70.0, grain: 30.0, fade: 0.0 }
    }
}

impl fmt::Display for Hdr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num(self.radius), num(self.strength), num(self.fade))
    }
}

impl Operation for Hdr {
    const NAME: &'static str = "hdr";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "HDR Look";

    fn parse(params: &str) -> Result<Hdr, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let h = Hdr { radius: r.number("the radius", HDR_RADIUS)?, strength: r.number("the strength", HDR_STRENGTH)?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(h)
    }

    fn clamped(self) -> Hdr {
        Hdr { radius: clamp(self.radius, HDR_RADIUS), strength: clamp(self.strength, HDR_STRENGTH), fade: clamp(self.fade, FADE) }
    }
}

impl fmt::Display for DreamGlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num(self.bloom), num(self.brightness), num(self.fade))
    }
}

impl Operation for DreamGlow {
    const NAME: &'static str = "dream_glow";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Dream Glow";

    fn parse(params: &str) -> Result<DreamGlow, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let d =
            DreamGlow { bloom: r.number("the bloom", BLOOM)?, brightness: r.number("the brightness", BRIGHTNESS)?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(d)
    }

    fn clamped(self) -> DreamGlow {
        DreamGlow { bloom: clamp(self.bloom, BLOOM), brightness: clamp(self.brightness, BRIGHTNESS), fade: clamp(self.fade, FADE) }
    }
}

impl fmt::Display for PlasticLens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num(self.blur), num(self.grain), num(self.fade))
    }
}

impl Operation for PlasticLens {
    const NAME: &'static str = "plastic_lens";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Plastic Lens";

    fn parse(params: &str) -> Result<PlasticLens, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let p = PlasticLens { blur: r.number("blur edges", BLUR_EDGES)?, grain: r.number("the grain", GRAIN)?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(p)
    }

    fn clamped(self) -> PlasticLens {
        PlasticLens { blur: clamp(self.blur, BLUR_EDGES), grain: clamp(self.grain, GRAIN), fade: clamp(self.fade, FADE) }
    }
}

// Rendering (M12.10; M13.39: within the slider's caps).

/// HDR Look's finest detail, left out of the boost: a blur of this sigma (a fraction of the
/// shorter side) stands for the photo in `in − blur(r)`, so the boost lifts the picture's local
/// contrast rather than its grain and pixel edges, alike at every render size.
pub const HDR_FINE: f64 = 0.0015;
/// Dream Glow's midtone lift at Brightness 100: `v + LIFT · v · (1 − v)` (black and white stay).
pub const LIFT: f64 = 0.8;
/// Plastic Lens's edge blur at Blur Edges 100: its sigma as a fraction of the shorter side.
pub const EDGE_SIGMA: f64 = 0.015;
/// Plastic Lens's soft edges and vignette, by the distance from the picture's centre ([`lens`]:
/// 0 at the centre, 1 in the corners): sharp out to `EDGE_FROM`, fully blurred at 1.
pub const EDGE_FROM: f64 = 0.3;
/// The vignette: unchanged out to `VIGNETTE_FROM`, darkened by `VIGNETTE` (a fraction) in the
/// corners.
pub const VIGNETTE_FROM: f64 = 0.4;
pub const VIGNETTE: f64 = 0.4;
/// Plastic Lens's grain at Grain 100: the noise's standard deviation in working values.
pub const GRAIN_AMOUNT: f64 = 0.15;
/// The grain's lattice cell, a fraction of the shorter side.
pub const GRAIN_CELL: f64 = 0.0012;

/// HDR Look's roll-off takes `tanh` as 1 past this (`1 − tanh(9)` is 3 · 10⁻⁸).
const TANH_SPAN: f32 = 9.0;

impl Adjust for Hdr {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let h = self.clamped();
        if h.fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let blur = Blur::new(GridBlur::new(space.length(h.radius / 1000.0), space.size())?);
        let fine = GridBlur::new(space.length(HDR_FINE), space.size()).map(Blur::new);
        Some(Box::new(HdrPass { blur, fine, strength: h.strength as f32, alpha: alpha(h.fade) }))
    }
}

impl Adjust for DreamGlow {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let d = self.clamped();
        if d.fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        // Bloom 0 (or a blur under a pixel's worth) overlays the photo on itself.
        let blur = GridBlur::new(space.length(d.bloom / 1000.0), space.size()).map(Blur::new);
        Some(Box::new(DreamGlowPass { blur, lift: (d.brightness / 100.0 * LIFT) as f32, alpha: alpha(d.fade) }))
    }
}

impl Adjust for PlasticLens {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let p = self.clamped();
        if p.fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let blur = GridBlur::new(space.length(p.blur / 100.0 * EDGE_SIGMA), space.size()).map(Blur::new);
        let grain = (p.grain / 100.0 * GRAIN_AMOUNT) as f32;
        let seed = noise::seed(&format!("{}({p})", Self::NAME));
        Some(Box::new(PlasticLensPass { blur, grain, noise: Noise::new(seed, GRAIN_CELL), space, alpha: alpha(p.fade) }))
    }
}

/// How much of an effect Fade keeps: `1 − Fade/100` ([`crate::fx::fade`]'s weight).
fn alpha(fade: f64) -> f32 {
    unit((1.0 - fade.clamp(0.0, 100.0) / 100.0) as f32)
}

/// `out = in + C · (L − blur(L))` on the luma `L`, the same added to every channel (local
/// contrast without a colour shift), then Fade. Both blurs keep the luma alone (it is linear: the
/// luma of the blurred picture is the blur of its luma).
struct HdrPass {
    blur: Blur<1>,
    /// The finest detail left out of the boost (`None`: under a pixel's worth).
    fine: Option<Blur<1>>,
    strength: f32,
    alpha: f32,
}

impl Pass for HdrPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.blur.grid.reach().max(self.fine.as_ref().map_or(0, |f| f.grid.reach())))
    }

    fn run(&self, band: &mut Band<'_>) {
        let w = band.width as usize;
        let Some(view) = band.before else { return };
        let Some(mut wide) = Rows::<1>::new(&self.blur, view, band) else { return };
        let mut fine = match &self.fine {
            Some(f) => match Rows::<1>::new(f, view, band) {
                Some(rows) => Some((f.grid.strength(), rows)),
                None => return,
            },
            None => None,
        };
        let (Some(mut blurred), Some(mut sharp), Some(mut detail)) = (zeros(w, 0.0f32), zeros(w, 0.0f32), zeros(w.saturating_mul(3), 0.0f32)) else {
            return;
        };
        let sw = self.blur.grid.strength();
        for (ly, row) in band.pixels.chunks_mut(w.max(1)).enumerate() {
            let y = offset(band.y0, ly);
            wide.row(y, &mut blurred);
            let sf = match &mut fine {
                Some((strength, rows)) => {
                    rows.row(y, &mut sharp);
                    *strength
                }
                None => 0.0,
            };
            let fine = fine.as_ref().map(|_| (sharp.as_slice(), sf));
            hdr_detail(row, &blurred, sw, fine, self.strength, detail.as_chunks_mut::<3>().0);
            hdr_boost(row.as_flattened_mut(), &detail, self.alpha);
        }
    }
}

/// Each pixel's detail, `C · (fine − blurred)` on the luma (the blurs mixed in by their
/// strengths), for each of its channels.
#[inline(never)]
fn hdr_detail(row: &[[f32; 3]], blurred: &[f32], sw: f32, fine: Option<(&[f32], f32)>, strength: f32, detail: &mut [[f32; 3]]) {
    match fine {
        Some((sharp, sf)) => {
            for (((px, &b), &s), d) in row.iter().zip(blurred).zip(sharp).zip(detail) {
                let l = luma(*px);
                *d = [strength * ((l + (s - l) * sf) - (l + (b - l) * sw)); 3];
            }
        }
        None => {
            for ((px, &b), d) in row.iter().zip(blurred).zip(detail) {
                let l = luma(*px);
                *d = [strength * (l - (l + (b - l) * sw)); 3];
            }
        }
    }
}

/// [`boost`] along a row (each loop of the passes is a function of its own: inside a pass's
/// `run` the compiler does not vectorise them).
#[inline(never)]
fn hdr_boost(row: &mut [f32], detail: &[f32], alpha: f32) {
    for (v, &d) in row.iter_mut().zip(detail) {
        *v = boost(*v, d, alpha);
    }
}

/// One channel `input` with the luma's `detail` added, eased into the room left to black or white
/// (`room · tanh(detail / room)`: a small detail is added as it is, a large one approaches black
/// or white without clipping, so a halo keeps its shape and a pixel averaging an edge is boosted
/// as the edge's pixels are), then faded by `alpha`. Branch-free, so a row of them vectorises: no
/// room gives `0 · tanh(∞ or NaN)`, and [`tanh`] takes both as its span (0 added).
#[inline]
fn boost(input: f32, detail: f32, alpha: f32) -> f32 {
    let v = input.clamp(0.0, 1.0);
    let room = if detail >= 0.0 { 1.0 - v } else { v };
    let roll = (room * tanh(detail.abs() / room)).copysign(detail);
    input + (input + roll - input) * alpha
}

/// `tanh(x)` for `x` from 0 (`NaN` and infinity as `TANH_SPAN`) within 6 · 10⁻⁷, without a branch
/// or a call so a row of them vectorises: Lambert's continued fraction for `tanh` cut after its
/// tenth term, `x · P(x²) / Q(x²)` (the coefficients its convergent gives, divided by the
/// constant term), held at 1 past `TANH_SPAN`.
#[inline]
fn tanh(x: f32) -> f32 {
    const P: [f32; 6] = [1.0, 0.142_857_14, 0.004_678_362_6, 4.914_246_4e-5, 1.560_078_2e-7, 7.273_092e-11];
    const Q: [f32; 6] = [1.0, 0.476_190_47, 0.030_075_188, 5.503_956e-4, 3.276_164_3e-6, 4.800_240_7e-9];
    let x = x.min(TANH_SPAN);
    let z = x * x;
    let p = P[0] + z * (P[1] + z * (P[2] + z * (P[3] + z * (P[4] + z * P[5]))));
    let q = Q[0] + z * (Q[1] + z * (Q[2] + z * (Q[3] + z * (Q[4] + z * Q[5]))));
    (x * p / q).min(1.0)
}

/// The blurred copy overlaid on the photo, then the midtone lift, then Fade.
struct DreamGlowPass {
    /// `None`: the blur is under a pixel's worth; the photo overlays itself.
    blur: Option<Blur<3>>,
    lift: f32,
    alpha: f32,
}

impl Pass for DreamGlowPass {
    fn reach(&self) -> Reach {
        match &self.blur {
            Some(b) => Reach::Pixels(b.grid.reach()),
            None => Reach::Pixel,
        }
    }

    fn run(&self, band: &mut Band<'_>) {
        let w = band.width as usize;
        let Some(blur) = &self.blur else {
            let pixels = band.pixels.as_flattened_mut();
            let Some(own) = zeros(pixels.len(), 0.0f32) else { return };
            glow_row(pixels, &own, 0.0, self.lift, self.alpha);
            return;
        };
        let Some(view) = band.before else { return };
        let Some(mut rows) = Rows::<3>::new(blur, view, band) else { return };
        let Some(mut glow) = zeros(w.saturating_mul(3), 0.0f32) else { return };
        let strength = unit(blur.grid.strength());
        for (ly, row) in band.pixels.chunks_mut(w.max(1)).enumerate() {
            rows.row(offset(band.y0, ly), &mut glow);
            glow_row(row.as_flattened_mut(), &glow, strength, self.lift, self.alpha);
        }
    }
}

/// Dream Glow along a row: each channel's `glow` (mixed in by the blur's `strength`) overlaid on
/// it, lifted by `lift`, faded by `alpha`.
#[inline(never)]
fn glow_row(row: &mut [f32], glow: &[f32], strength: f32, lift: f32, alpha: f32) {
    for (v, &g) in row.iter_mut().zip(glow) {
        let input = *v;
        let (b, t) = (unit(input), unit(input + (g - input) * strength));
        // `Blend::Overlay`, written out.
        let over = if b <= 0.5 { 2.0 * b * t } else { 1.0 - (1.0 - (2.0 * b - 1.0)) * (1.0 - t) };
        let lifted = unit(over + lift * over * (1.0 - over));
        *v = input + (lifted - input) * alpha;
    }
}

/// Soft edges (the blur mixed in toward the corners), the vignette, the grain, then Fade.
struct PlasticLensPass {
    /// `None`: Blur Edges 0 (or under a pixel's worth).
    blur: Option<Blur<3>>,
    grain: f32,
    noise: Noise,
    space: Space,
    alpha: f32,
}

impl Pass for PlasticLensPass {
    fn reach(&self) -> Reach {
        match &self.blur {
            Some(b) => Reach::Pixels(b.grid.reach()),
            None => Reach::Pixel,
        }
    }

    fn run(&self, band: &mut Band<'_>) {
        let w = band.width as usize;
        let mut blur = match &self.blur {
            Some(b) => {
                let Some(view) = band.before else { return };
                let Some(rows) = Rows::<3>::new(b, view, band) else { return };
                Some((unit(b.grid.strength()), rows))
            }
            None => None,
        };
        let grain = if self.grain > 0.0 { self.noise.lines(&self.space, band.x0, band.y0, band.width, band.rows, 0) } else { None };
        let (hw, hh) = half(self.space.size());
        // The squared distance across, by column (the same on every row).
        let Some(mut across) = zeros(w, 0.0f32) else { return };
        for (k, a) in across.iter_mut().enumerate() {
            let dx = (offset(band.x0, k) as f32 + 0.5 - hw) / hw;
            *a = dx * dx;
        }
        let w3 = w.saturating_mul(3);
        let (Some(mut noise), Some(mut soft), Some(mut shade)) = (zeros(w, 0.0f32), zeros(w, 0.0f32), zeros(w, 1.0f32)) else { return };
        let (Some(mut grain3), Some(mut soft3), Some(mut shade3), Some(mut blurred)) =
            (zeros(w3, 0.0f32), zeros(w3, 0.0f32), zeros(w3, 1.0f32), zeros(w3, 0.0f32))
        else {
            return;
        };
        for (ly, row) in band.pixels.chunks_mut(w.max(1)).enumerate() {
            let y = offset(band.y0, ly);
            if self.grain > 0.0 {
                if !grain.as_ref().is_some_and(|g| g.row(ly, &mut noise)) {
                    self.noise.row(&self.space, band.x0, y, 0, &mut noise);
                }
                triple(&noise, self.grain, grain3.as_chunks_mut::<3>().0);
            }
            let dy = (y as f32 + 0.5 - hh) / hh;
            lens_weights(&across, dy * dy, &mut soft, &mut shade);
            triple(&soft, 1.0, soft3.as_chunks_mut::<3>().0);
            triple(&shade, 1.0, shade3.as_chunks_mut::<3>().0);
            let strength = match &mut blur {
                Some((strength, rows)) => {
                    rows.row(y, &mut blurred);
                    *strength
                }
                None => 0.0,
            };
            let blurred = blur.as_ref().map(|_| (blurred.as_slice(), strength));
            lens_row(row.as_flattened_mut(), blurred, &soft3, &shade3, &grain3, self.alpha);
        }
    }
}

/// Plastic Lens's weights ([`weights`]) along a row whose squared distance down is `dy2`.
#[inline(never)]
fn lens_weights(across: &[f32], dy2: f32, soft: &mut [f32], shade: &mut [f32]) {
    for ((s, v), &dx2) in soft.iter_mut().zip(shade.iter_mut()).zip(across) {
        (*s, *v) = weights(dx2 + dy2);
    }
}

/// Each of `values` times `scale`, three times over (once a channel).
#[inline(never)]
fn triple(values: &[f32], scale: f32, out: &mut [[f32; 3]]) {
    for (o, &v) in out.iter_mut().zip(values) {
        *o = [v * scale; 3];
    }
}

/// Plastic Lens along a row: the blur (mixed in by its strength, `None` without one) mixed in by
/// the soft edge, the vignette's shade and the grain, then Fade.
#[inline(never)]
fn lens_row(row: &mut [f32], blurred: Option<(&[f32], f32)>, soft: &[f32], shade: &[f32], grain: &[f32], alpha: f32) {
    match blurred {
        Some((blurred, strength)) => {
            for ((((v, &b), &soft), &shade), &g) in row.iter_mut().zip(blurred).zip(soft).zip(shade).zip(grain) {
                let input = *v;
                let edge = input + ((input + (b - input) * strength) - input) * soft;
                let out = unit(edge * shade + g);
                *v = input + (out - input) * alpha;
            }
        }
        None => {
            for (((v, &shade), &g), _) in row.iter_mut().zip(shade).zip(grain).zip(soft) {
                let input = *v;
                let out = unit(input * shade + g);
                *v = input + (out - input) * alpha;
            }
        }
    }
}

/// Plastic Lens's weights at the output pixel `(x, y)` of a `size` output (after the crop): how
/// much of the edge blur is mixed in (0 in the middle, 1 in the corners) and the vignette's
/// factor (1 in the middle, `1 − VIGNETTE` in the corners). Both go by the distance from the
/// picture's centre on an ellipse through its corners (0 at the centre, 1 in a corner), so they
/// fit any shape and size.
pub fn lens(size: (u32, u32), x: u32, y: u32) -> (f32, f32) {
    let (hw, hh) = half(size);
    let (dx, dy) = ((x as f32 + 0.5 - hw) / hw, (y as f32 + 0.5 - hh) / hh);
    weights(dx * dx + dy * dy)
}

/// Half of each side of a `size` output (at least half a pixel).
fn half(size: (u32, u32)) -> (f32, f32) {
    (size.0.max(1) as f32 / 2.0, size.1.max(1) as f32 / 2.0)
}

/// [`lens`]'s weights at the squared distance `d2` from the centre (2 in a corner).
#[inline]
fn weights(d2: f32) -> (f32, f32) {
    let d = d2.sqrt() / std::f32::consts::SQRT_2;
    let soft = smooth(EDGE_FROM as f32, d);
    let shade = 1.0 - VIGNETTE as f32 * smooth(VIGNETTE_FROM as f32, d);
    (soft, shade)
}

/// [`crate::fx::smoothstep`] from `e0` (under 1) to 1, written out so a row of it vectorises.
#[inline]
fn smooth(e0: f32, x: f32) -> f32 {
    let t = ((x - e0) / (1.0 - e0)).clamp(0.0, 1.0);
    let t = if t.is_nan() { 0.0 } else { t };
    t * t * (3.0 - 2.0 * t)
}

/// The output coordinate `start + k` (saturating).
fn offset(start: u32, k: usize) -> u32 {
    start.saturating_add(u32::try_from(k).unwrap_or(u32::MAX))
}

/// `n` copies of `v`; `None` when the buffer cannot be had.
fn zeros<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}

#[inline]
fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// A pass's blur: a [`GridBlur`] (cells about a third of its sigma) and, for one-pixel cells, its
/// rows blurred across, made once per render.
struct Blur<const C: usize> {
    grid: GridBlur,
    across: OnceLock<Option<Across<C>>>,
}

impl<const C: usize> Blur<C> {
    fn new(grid: GridBlur) -> Blur<C> {
        Blur { grid, across: OnceLock::new() }
    }
}

/// A [`Blur`] of a pass's `before` view read back along a band's rows, `C` values a pixel (1: its
/// luma, [`GridBlur::luma`]; 3: RGB, [`GridBlur::rgb`]), not yet mixed in by the blur's strength.
enum Rows<'a, const C: usize> {
    /// One-pixel cells (a blur under 6 pixels: HDR Look's fine detail at the sizes a screen
    /// shows, the drafts' blurs): the rows blurred across, blurred down.
    Pixels { across: &'a Across<C>, offset: usize, width: usize },
    /// Wider cells: the grid, made once per render, read back.
    Grid(GridRows<'a, C>),
}

impl<'a, const C: usize> Rows<'a, C> {
    fn new(blur: &'a Blur<C>, view: &View<'_>, band: &Band<'_>) -> Option<Rows<'a, C>> {
        if blur.grid.cell() == 1 {
            let across = blur.across.get_or_init(|| apart(|| Across::new(&blur.grid, view))).as_ref()?;
            let offset = (band.x0.checked_sub(view.x0)? as usize).checked_mul(C)?;
            let width = (band.width as usize).checked_mul(C)?;
            return (offset + width <= across.width).then_some(Rows::Pixels { across, offset, width });
        }
        let window = Window::of(view)?;
        let cells: Cells<'a> = match C {
            1 => blur.grid.luma(window)?.into(),
            3 => blur.grid.rgb(window)?.into(),
            _ => return None,
        };
        GridRows::new(&blur.grid, cells, band.x0, band.width as usize).map(Rows::Grid)
    }

    /// The blur along the output row `y` (one of the band's), into `out` (`C` values a pixel).
    fn row(&mut self, y: u32, out: &mut [f32]) {
        match self {
            Rows::Pixels { across, offset, width } => across.down(y, *offset, *width, out),
            Rows::Grid(l) => l.read(y, out),
        }
    }
}

/// A blur on the pixels themselves (a [`GridBlur`] of one-pixel cells): the kernel of its three
/// box passes applied across every row of the window once per render ([`Across::new`]), then down
/// by each band for its rows ([`Across::down`]), each value one weighted sum in a fixed order, so
/// it depends only on the pixels under the kernel (a region render equals the whole render). The
/// pixels past the picture's edge repeat its edge pixels ([`View::get`]), each channel
/// `byte · (1/255)` as the grid's one-pixel cells take it. The same blur as the box passes within
/// their fixed point's rounding, except within the kernel's reach of the picture's edge, where the
/// passes repeat their own edge values rather than the picture's.
struct Across<const C: usize> {
    kernel: Vec<f32>,
    /// The window's rows, its top row in the whole output, and the whole output's height.
    rows: usize,
    y0: u32,
    full_height: u32,
    /// Values a row (`C` a pixel of the window).
    width: usize,
    data: Vec<f32>,
}

impl<const C: usize> Across<C> {
    fn new(blur: &GridBlur, view: &View<'_>) -> Option<Across<C>> {
        let kernel = boxes(blur.gaussian().radii())?;
        let r = kernel.len() / 2;
        let (w, rows) = (view.width as usize, view.height as usize);
        // The view's column for every pixel a row's kernel reads, clamped as `View::get`; the run
        // of them that steps one by one from the view's column `start` is read straight from the
        // row's bytes, the rest (the repeated edges) through their columns.
        let mut columns = zeros(w.checked_add(2 * r)?, 0usize)?;
        let last_x = i64::from(view.full_width.max(1)) - 1;
        let last_lx = i64::from(view.width.max(1)) - 1;
        for (k, c) in columns.iter_mut().enumerate() {
            let x = (i64::from(view.x0) - r as i64 + k as i64).clamp(0, last_x);
            *c = (x - i64::from(view.x0)).clamp(0, last_lx) as usize;
        }
        let from = columns.iter().zip(columns.iter().skip(1)).position(|(a, b)| b.checked_sub(*a) == Some(1)).unwrap_or(0);
        let start = columns.get(from).copied().unwrap_or(0);
        let steps = columns.iter().skip(from).enumerate().take_while(|&(i, &c)| c == start + i).count();
        let width = w.checked_mul(C)?;
        let mut data = zeros(rows.checked_mul(width)?, 0.0f32)?;
        let line = w * 4;
        data.par_chunks_mut(width.max(1)).enumerate().for_each_init(
            || zeros(columns.len().saturating_mul(C), 0.0f32),
            |values, (ly, out)| {
                let Some(values) = values else { return };
                let bytes = view.data.get(ly * line..(ly + 1) * line).unwrap_or(&[]);
                let pixels = bytes.get(start * 4..).unwrap_or(&[]).as_chunks::<4>().0;
                for (v, p) in values.as_chunks_mut::<C>().0.iter_mut().skip(from).take(steps).zip(pixels) {
                    texel(p, v);
                }
                for (k, (v, &c)) in values.as_chunks_mut::<C>().0.iter_mut().zip(&columns).enumerate() {
                    if k < from || k >= from + steps {
                        match bytes.get(c * 4..c * 4 + 4).and_then(|p| p.as_chunks::<4>().0.first()) {
                            Some(p) => texel(p, v),
                            None => v.fill(0.0),
                        }
                    }
                }
                // The kernel is symmetric: each pair of taps at once, then the middle one.
                let tap = |i: usize| values.get(i * C..).unwrap_or(&[]);
                for (i, &k) in kernel.iter().take(r).enumerate() {
                    accumulate(out, tap(i), tap(2 * r - i), k);
                }
                accumulate(out, tap(r), &[], kernel.get(r).copied().unwrap_or(0.0));
            },
        );
        Some(Across { kernel, rows, y0: view.y0, full_height: view.full_height, width, data })
    }

    /// The blur at the output row `y`, the `width` values from `offset` of the window's rows,
    /// into `out`: the rows the kernel reads (clamped into the picture, then the window) blurred
    /// down.
    fn down(&self, y: u32, offset: usize, width: usize, out: &mut [f32]) {
        out.fill(0.0);
        let r = self.kernel.len() / 2;
        let (last_y, last_ly) = (i64::from(self.full_height.max(1)) - 1, self.rows.max(1) as i64 - 1);
        let line = |i: usize| {
            let row = (i64::from(y) - r as i64 + i as i64).clamp(0, last_y);
            let ly = (row - i64::from(self.y0)).clamp(0, last_ly) as usize;
            let at = ly * self.width + offset;
            self.data.get(at..at + width).unwrap_or(&[])
        };
        for (i, &k) in self.kernel.iter().take(r).enumerate() {
            accumulate(out, line(i), line(2 * r - i), k);
        }
        accumulate(out, line(r), &[], self.kernel.get(r).copied().unwrap_or(0.0));
    }
}

/// `out += k · (a + b)` value by value (`b` empty: `out += k · a`).
#[inline(never)]
fn accumulate(out: &mut [f32], a: &[f32], b: &[f32], k: f32) {
    if b.is_empty() {
        for (o, &a) in out.iter_mut().zip(a) {
            *o += k * a;
        }
    } else {
        for ((o, &a), &b) in out.iter_mut().zip(a).zip(b) {
            *o += k * (a + b);
        }
    }
}

/// A pixel's `C` working values: its Rec.601 luma (1) or its RGB (3), each channel `byte · (1/255)`
/// as a grid's one-pixel cell takes it.
#[inline]
fn texel(p: &[u8; 4], out: &mut [f32]) {
    let scale = 1.0f32 / 255.0;
    let rgb = [f32::from(p[0]) * scale, f32::from(p[1]) * scale, f32::from(p[2]) * scale];
    if out.len() == 1 {
        out.fill(luma(rgb));
    } else {
        for (o, v) in out.iter_mut().zip(rgb) {
            *o = v;
        }
    }
}

/// The kernel of three box passes of `radii` (each the mean of `2r + 1` pixels): their
/// convolution, `2 · Σr + 1` weights adding up to 1; `None` past 4 097 weights.
fn boxes(radii: [usize; 3]) -> Option<Vec<f32>> {
    let len = radii.iter().try_fold(1usize, |n, &r| n.checked_add(r.checked_mul(2)?))?;
    if len > 4097 {
        return None;
    }
    let mut kernel = zeros(len, 0.0f64)?;
    let mut next = zeros(len, 0.0f64)?;
    if let Some(k) = kernel.first_mut() {
        *k = 1.0;
    }
    let mut n = 1;
    for r in radii {
        let wide = 2 * r + 1;
        next.fill(0.0);
        for (j, &k) in kernel.iter().take(n).enumerate() {
            for o in next.iter_mut().skip(j).take(wide) {
                *o += k / wide as f64;
            }
        }
        std::mem::swap(&mut kernel, &mut next);
        n += 2 * r;
    }
    Some(kernel.into_iter().map(|k| k as f32).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::Mono;

    /// `w × h` RGBA pixels of a seeded pattern with edges and gradients.
    fn picture(w: u32, h: u32) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let v = |k: u32| ((x * (7 + k) + y * (13 - k) + (x / 9) * (y / 7) * 31) % 256) as u8;
                [v(0), v(1), v(2), 255]
            })
            .collect()
    }

    /// A one-pixel-cell blur's rows, convolved, are the grid's box passes
    /// ([`GridBlur::luma`], [`GridBlur::rgb`]) within their fixed point's rounding away from the
    /// picture's edge (within the kernel's reach of it the passes repeat their own edge values).
    #[test]
    fn the_convolved_rows_are_the_grids_blur() {
        let (w, h) = (90u32, 70u32);
        let data = picture(w, h);
        let view = View { data: &data, x0: 0, y0: 0, width: w, height: h, full_width: w, full_height: h };
        for sigma in [0.6, 1.5, 3.0, 5.5] {
            let blur = GridBlur::new(sigma, (w, h)).unwrap();
            assert_eq!(blur.cell(), 1);
            let r = blur.gaussian().reach() as usize;
            let window = Window::of(&view).unwrap();
            let (mono, rgb): (Mono, Vec<[f32; 3]>) = (blur.luma(window).unwrap().clone(), blur.rgb(window).unwrap().data.clone());
            let mut pixels = vec![[0.0f32; 3]; (w * 32) as usize];
            let band = Band { pixels: &mut pixels, width: w, rows: 32, x0: 0, y0: 20, before: Some(&view) };
            let (one, three) = (Blur::<1>::new(GridBlur::new(sigma, (w, h)).unwrap()), Blur::<3>::new(GridBlur::new(sigma, (w, h)).unwrap()));
            let (mut luma, mut colour) = (Rows::new(&one, &view, &band).unwrap(), Rows::new(&three, &view, &band).unwrap());
            let (mut a, mut b) = (vec![0.0f32; w as usize], vec![0.0f32; 3 * w as usize]);
            for ly in 0..32usize {
                let y = 20 + ly as u32;
                luma.row(y, &mut a);
                colour.row(y, &mut b);
                for x in r..w as usize - r {
                    let at = y as usize * w as usize + x;
                    assert!((a[x] - mono.data[at]).abs() < 2e-5, "sigma {sigma} at {x},{y}: {} vs {}", a[x], mono.data[at]);
                    for c in 0..3 {
                        assert!((b[3 * x + c] - rgb[at][c]).abs() < 2e-5, "sigma {sigma} at {x},{y}");
                    }
                }
            }
        }
    }

    /// The branch-free roll-off is `room · tanh(detail / room)` within 10⁻⁶, over every room a
    /// pixel can have and every detail, and the input itself where the detail is 0.
    #[test]
    fn the_roll_off_follows_tanh() {
        for vi in 0..=255 {
            let v = vi as f32 / 255.0;
            for di in -1200..=1200 {
                let d = di as f32 / 100.0;
                let room = f64::from(if d >= 0.0 { 1.0 - v } else { v });
                let want = f64::from(v) + if room > 0.0 { room * (f64::from(d) / room).tanh() } else { 0.0 };
                let got = boost(v, d, 1.0);
                assert!((f64::from(got) - want).abs() < 1e-6, "{v} {d}: {got} vs {want}");
            }
            assert_eq!(boost(v, 0.0, 1.0), v);
        }
    }
}
