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
//!   Every blur is made by each band for its own rows with the blur's reach around them (exact:
//!   a region render equals that window of the whole render), on a grid of cell averages when it
//!   is wide, so its cost stays near the picture's size whatever the radius.

use std::fmt;

use super::{Adjust, Band, FADE, Operation, ParamError, Pass, Prepare, Reach, Reader, View, clamp, num};
use crate::fx::{Blend, Gaussian, Noise, Plane, Space, fade, luma, mix, noise, smoothstep};

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

// Rendering (M12.10).

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

/// The smallest blur three whole-pixel boxes make (radii 0, 0, 1: σ ≈ 0.82 px); a smaller one is
/// this blur mixed in by the share of its variance, so a small render blurs as a large one does.
const MIN_SIGMA: f64 = 0.82;
/// A wide blur runs on a grid of cells about this many times smaller than its sigma.
const CELL_SIGMA: f64 = 3.0;
/// The largest grid cell, in output pixels.
const MAX_CELL: u32 = 32;

impl Adjust for Hdr {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let h = self.clamped();
        if h.fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let blur = BandBlur::new(space.length(h.radius / 1000.0), space.size())?;
        let fine = BandBlur::new(space.length(HDR_FINE), space.size());
        Some(Box::new(HdrPass { blur, fine, strength: h.strength as f32, fade: h.fade }))
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
        let blur = BandBlur::new(space.length(d.bloom / 1000.0), space.size());
        Some(Box::new(DreamGlowPass { blur, lift: (d.brightness / 100.0 * LIFT) as f32, fade: d.fade }))
    }
}

impl Adjust for PlasticLens {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let p = self.clamped();
        if p.fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let blur = BandBlur::new(space.length(p.blur / 100.0 * EDGE_SIGMA), space.size());
        let grain = (p.grain / 100.0 * GRAIN_AMOUNT) as f32;
        let noise = Noise::new(noise::seed(&format!("{}({p})", Self::NAME)), GRAIN_CELL);
        Some(Box::new(PlasticLensPass { blur, grain, noise, space, fade: p.fade }))
    }
}

/// `out = in + C · (L − blur(L))` on the luma `L`, the same added to every channel (local
/// contrast without a colour shift), then Fade.
struct HdrPass {
    blur: BandBlur,
    /// The finest detail left out of the boost (`None`: under a pixel's worth).
    fine: Option<BandBlur>,
    strength: f32,
    fade: f64,
}

impl Pass for HdrPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.blur.reach().max(self.fine.map_or(0, |f| f.reach())))
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        let Some(plane) = self.blur.plane(view, band) else { return };
        let fine = match &self.fine {
            Some(f) => match f.plane(view, band) {
                Some(p) => Some((f, p)),
                None => return,
            },
            None => None,
        };
        each(band, |x, y, input| {
            let blurred = self.blur.read(&plane, input, x, y);
            let sharp = fine.as_ref().map_or(input, |(f, p)| f.read(p, input, x, y));
            let detail = self.strength * (luma(sharp) - luma(blurred));
            fade(input, input.map(|v| v + roll_off(v, detail)), self.fade)
        });
    }
}

/// The blurred copy overlaid on the photo, then the midtone lift, then Fade.
struct DreamGlowPass {
    /// `None`: the blur is under a pixel's worth; the photo overlays itself.
    blur: Option<BandBlur>,
    lift: f32,
    fade: f64,
}

impl Pass for DreamGlowPass {
    fn reach(&self) -> Reach {
        match &self.blur {
            Some(b) => Reach::Pixels(b.reach()),
            None => Reach::Pixel,
        }
    }

    fn run(&self, band: &mut Band<'_>) {
        let plane = match (&self.blur, band.before) {
            (Some(blur), Some(view)) => match blur.plane(view, band) {
                Some(p) => Some(p),
                None => return,
            },
            (Some(_), None) => return,
            (None, _) => None,
        };
        each(band, |x, y, input| {
            let glow = match (&self.blur, &plane) {
                (Some(blur), Some(plane)) => blur.read(plane, input, x, y),
                _ => input,
            };
            let over = Blend::Overlay.apply(input, glow);
            let lifted = over.map(|v| unit(v + self.lift * v * (1.0 - v)));
            fade(input, lifted, self.fade)
        });
    }
}

/// Soft edges (the blur mixed in toward the corners), the vignette, the grain, then Fade.
struct PlasticLensPass {
    /// `None`: Blur Edges 0 (or under a pixel's worth).
    blur: Option<BandBlur>,
    grain: f32,
    noise: Noise,
    space: Space,
    fade: f64,
}

impl Pass for PlasticLensPass {
    fn reach(&self) -> Reach {
        match &self.blur {
            Some(b) => Reach::Pixels(b.reach()),
            None => Reach::Pixel,
        }
    }

    fn run(&self, band: &mut Band<'_>) {
        let plane = match (&self.blur, band.before) {
            (Some(blur), Some(view)) => match blur.plane(view, band) {
                Some(p) => Some(p),
                None => return,
            },
            (Some(_), None) => return,
            (None, _) => None,
        };
        let size = self.space.size();
        each(band, |x, y, input| {
            let (soft, shade) = lens(size, x, y);
            let mut out = input;
            if let (Some(blur), Some(plane), true) = (&self.blur, &plane, soft > 0.0) {
                out = mix(out, blur.read(plane, input, x, y), soft);
            }
            let g = if self.grain > 0.0 { self.grain * self.noise.at(&self.space, x, y, 0) } else { 0.0 };
            out = out.map(|v| unit(v * shade + g));
            fade(input, out, self.fade)
        });
    }
}

/// Plastic Lens's weights at the output pixel `(x, y)` of a `size` output (after the crop): how
/// much of the edge blur is mixed in (0 in the middle, 1 in the corners) and the vignette's
/// factor (1 in the middle, `1 − VIGNETTE` in the corners). Both go by the distance from the
/// picture's centre on an ellipse through its corners (0 at the centre, 1 in a corner), so they
/// fit any shape and size.
pub fn lens(size: (u32, u32), x: u32, y: u32) -> (f32, f32) {
    let (hw, hh) = (f64::from(size.0.max(1)) / 2.0, f64::from(size.1.max(1)) / 2.0);
    let (dx, dy) = ((f64::from(x) + 0.5 - hw) / hw, (f64::from(y) + 0.5 - hh) / hh);
    let d = dx.hypot(dy) / std::f64::consts::SQRT_2;
    let soft = smoothstep(EDGE_FROM, 1.0, d);
    let shade = 1.0 - VIGNETTE * smoothstep(VIGNETTE_FROM, 1.0, d);
    (soft as f32, shade as f32)
}

/// `f(x, y, pixel)` for every pixel of the band (whole-output coordinates), in place.
fn each(band: &mut Band<'_>, f: impl Fn(u32, u32, [f32; 3]) -> [f32; 3]) {
    let (x0, y0, w) = (band.x0, band.y0, band.width as usize);
    if w == 0 {
        return;
    }
    for (ly, row) in band.pixels.chunks_mut(w).enumerate() {
        let y = y0.saturating_add(u32::try_from(ly).unwrap_or(u32::MAX));
        for (lx, px) in row.iter_mut().enumerate() {
            let x = x0.saturating_add(u32::try_from(lx).unwrap_or(u32::MAX));
            *px = f(x, y, *px);
        }
    }
}

/// `detail` added to `v` eased into the room left to black or white, `room · tanh(detail / room)`:
/// a small detail is added as it is, a large one approaches black or white without clipping (so a
/// halo keeps its shape, and a pixel averaging an edge is boosted as the edge's pixels are).
fn roll_off(v: f32, detail: f32) -> f32 {
    let v = unit(v);
    let room = if detail >= 0.0 { 1.0 - v } else { v };
    if room <= 0.0 || !detail.is_finite() {
        return 0.0;
    }
    room * (detail / room).tanh()
}

fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// A Gaussian blur of the picture as it was before a pass, made by each band for its own rows
/// with the blur's reach around them (a plane cut with that margin blurs exactly as the whole
/// picture's, [`Gaussian`]), so no band waits on another and a region render equals that window of
/// the whole render. A blur wider than `CELL_SIGMA` pixels runs on a grid of cell averages (a cell
/// about a third of its sigma, at most `MAX_CELL` pixels) read back bilinearly, so a band reads
/// its rows and the reach around them once and blurs a small grid.
#[derive(Clone, Copy, Debug)]
struct BandBlur {
    /// The blur on the grid.
    gaussian: Gaussian,
    /// The grid's cell side in output pixels (1: the blur runs on the pixels themselves).
    cell: u32,
    /// How much of the blur is mixed in (below 1 only for a blur under `MIN_SIGMA`).
    strength: f32,
}

impl BandBlur {
    /// A blur of `sigma` output pixels over a `size` output; `None` when it changes nothing.
    fn new(sigma: f64, size: (u32, u32)) -> Option<BandBlur> {
        if !sigma.is_finite() || sigma <= 0.0 {
            return None;
        }
        // The averaging and the bilinear read add about cell²/4 to the variance, taken off the
        // grid's.
        let cell = (sigma / CELL_SIGMA).floor().clamp(1.0, f64::from(MAX_CELL));
        let on_grid = (sigma * sigma - if cell > 1.0 { cell * cell / 4.0 } else { 0.0 }).max(0.0).sqrt() / cell;
        let grid = (size.0.div_ceil(cell as u32).max(1), size.1.div_ceil(cell as u32).max(1));
        let gaussian = Gaussian::new(on_grid.max(MIN_SIGMA), grid);
        let strength = (on_grid / gaussian.sigma().max(1e-9)).powi(2).min(1.0) as f32;
        if gaussian.is_identity() || strength < 1e-4 {
            return None;
        }
        Some(BandBlur { gaussian, cell: cell as u32, strength })
    }

    /// How far a blurred pixel reads, in output pixels: the grid's reach, the bilinear read and
    /// the cells' extent.
    fn reach(&self) -> u32 {
        self.gaussian.reach().saturating_add(3).saturating_mul(self.cell)
    }

    /// The blurred grid over the band's rows and the reach around them, within the window `view`
    /// holds; `None` when a buffer cannot be had.
    fn plane(&self, view: &View<'_>, band: &Band<'_>) -> Option<Plane> {
        if band.width == 0 || band.rows == 0 || self.cell == 0 {
            return None;
        }
        let reach = self.reach();
        let y1 = band.y0.saturating_add(band.rows);
        let top = band.y0.saturating_sub(reach).max(view.y0) / self.cell;
        let bottom = y1.saturating_add(reach).min(view.y0.saturating_add(view.height)).div_ceil(self.cell);
        let left = view.x0 / self.cell;
        let right = view.x0.saturating_add(view.width).div_ceil(self.cell);
        let mut plane = Plane::from_fn(left, top, right.saturating_sub(left), bottom.saturating_sub(top), |cx, cy| self.cell_mean(view, cx, cy))?;
        self.gaussian.apply(&mut plane).then_some(plane)
    }

    /// The grid cell `(cx, cy)`: the mean of its pixels (the picture's edge pixels repeated past
    /// it, as [`View::get`] reads them).
    fn cell_mean(&self, view: &View<'_>, cx: u32, cy: u32) -> [f32; 3] {
        if self.cell == 1 {
            return view.get(i64::from(cx), i64::from(cy));
        }
        let c = i64::from(self.cell);
        let (x0, y0) = (i64::from(cx) * c, i64::from(cy) * c);
        let mut sum = [0.0f32; 3];
        for y in y0..y0 + c {
            for x in x0..x0 + c {
                let p = view.get(x, y);
                sum = [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]];
            }
        }
        let n = (c * c) as f32;
        sum.map(|v| v / n)
    }

    /// The blur at the output pixel `(x, y)` whose own value is `input`: the grid read back at
    /// the pixel's centre, bilinearly, mixed in by the blur's strength.
    fn read(&self, plane: &Plane, input: [f32; 3], x: u32, y: u32) -> [f32; 3] {
        let blurred = if self.cell == 1 {
            plane.get(i64::from(x), i64::from(y))
        } else {
            let c = f64::from(self.cell);
            let (u, v) = ((f64::from(x) + 0.5) / c - 0.5, (f64::from(y) + 0.5) / c - 0.5);
            let (i, j) = (u.floor(), v.floor());
            let (tx, ty) = ((u - i) as f32, (v - j) as f32);
            let (i, j) = (i as i64, j as i64);
            let top = mix(plane.get(i, j), plane.get(i + 1, j), tx);
            let bottom = mix(plane.get(i, j + 1), plane.get(i + 1, j + 1), tx);
            mix(top, bottom, ty)
        };
        mix(input, blurred, self.strength)
    }
}
