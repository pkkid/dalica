//! The crosshair effects of tab 3 (`editing.md` §8.2): `soft_focus(x,y,size,amount)`,
//! `focal_bw(x,y,size,sharpness)` and `graduated_tint(x,y,angle,feather,shade,color)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): the crosshair's point
//!   `x,y`, normalised in the stored frame (`data-model.md` §3.3) and brought into 0..1, the
//!   centre (0.5, 0.5) by default; mapped into the output through [`super::Prepare::geometry`].
//!   - Soft Focus: Size 0–1 (0.3), the sharp circle's radius as a fraction of the picture's shorter
//!     side; Amount 0–1 (0.5), how strongly the outside blurs.
//!   - Focal B&W: Size 0–1 (0.3), as Soft Focus; Sharpness 0–1 (0.5), the circle's edge from soft
//!     (0) to hard (1).
//!   - Graduated Tint: the gradient's start `x,y`; Angle 0–360 (0), the direction from the start
//!     toward the tinted side in degrees clockwise from pointing up (0: a sky; as
//!     [`crate::fx::Gradient`] takes it);
//!     Feather 0–1 (0.25), the ramp from the start to the full tint as a fraction of the shorter
//!     side (0 a hard edge); Shade 0–1 (0.25), how deep the colour; the colour (`#9cc8ff`, a light
//!     blue). Pixels behind the start are unchanged and whites stay white.
//!
//!   Numbers out of range are clamped; `NaN`, a missing or extra field or a colour that is not
//!   `#rrggbb` is refused (the entry is kept and skipped).
//! - **Rendering** (M12.8, our own design on M12.3's [`crate::fx`]): every shape is placed in the
//!   stored frame through [`Space`], so the circle and the gradient land on the same content after
//!   a crop, a straighten, the EXIF orientation and the recipe's rotations and flips, and every
//!   length is relative to the stored photo's shorter side (the same at every render size).
//!   - Soft Focus: a [`Gaussian`] of `Amount × SOFT_SIGMA` of the shorter side, mixed in by the
//!     outside of a [`Circle`] that is sharp out to Size and fully blurred at `Size × (1 +
//!     SOFT_EDGE)`. One [`Reach::Pixels`] pass: each band blurs its own rows with the blur's reach
//!     around them (a plane cut with that margin blurs exactly as the whole picture), so a band
//!     never waits on another (the shared [`crate::fx::Blurred`] would make every band wait on the
//!     first one's blur, which runs on rayon inside a rayon band) and a region render equals that
//!     window of the whole render. A blur wider than `CELL_SIGMA` pixels runs on a grid of cell
//!     averages (a cell about a third of its sigma, at most `MAX_CELL` pixels) read back
//!     bilinearly, with the variance the averaging and the reading add taken off; one under the
//!     smallest box blur (`MIN_SIGMA`) is that blur mixed in by the share of its variance, so every
//!     render size blurs alike. Amount 0 changes nothing.
//!   - Focal B&W: colour inside the circle of Size, the Rec.601 grey ([`crate::fx::luma`]) outside;
//!     the edge fades over `(1 − Sharpness) × Size` (one output pixel at Sharpness 1). Pixel-only.
//!   - Graduated Tint: a [`Gradient`] from the start along the angle over Feather (at least one
//!     output pixel); ahead of the start the colour is multiplied in, by `TINT_FROM` of it at Shade
//!     0 up to all of it at Shade 1, held back where the darkest channel nears white (from
//!     `WHITE_FROM` up, so a white stays white). Pixels behind the start are left exactly as they
//!     were. White as the colour changes nothing. Pixel-only.

use std::fmt;

use super::{Adjust, Band, DEGREES, Operation, ParamError, Pass, Prepare, Reach, Reader, UNIT, clamp, clamp_point, hex, num, num_point};
use crate::frame::Point;
use crate::fx::{Circle, Gaussian, Gradient, Plane, Space, luma, mix, smoothstep};

/// The range of a focal circle's Size: its radius as a fraction of the shorter side.
pub const SIZE: (f64, f64) = UNIT;
/// The range of Soft Focus's Amount.
pub const AMOUNT: (f64, f64) = UNIT;
/// The range of Focal B&W's Sharpness.
pub const SHARPNESS: (f64, f64) = UNIT;
/// The range of Graduated Tint's Angle, degrees clockwise from pointing up.
pub const ANGLE: (f64, f64) = DEGREES;
/// The range of Graduated Tint's Feather, a fraction of the shorter side.
pub const FEATHER: (f64, f64) = UNIT;
/// The range of Graduated Tint's Shade.
pub const SHADE: (f64, f64) = UNIT;

/// Soft Focus's blur at Amount 1: its sigma as a fraction of the stored photo's shorter side.
pub const SOFT_SIGMA: f64 = 0.015;
/// The smallest blur three whole-pixel boxes make (radii 0, 0, 1: σ ≈ 0.82 px); a smaller one is
/// this blur mixed in by the share of its variance.
const MIN_SIGMA: f64 = 0.82;
/// A wide Soft Focus blur runs on a grid of cells about this many times smaller than its sigma.
const CELL_SIGMA: f64 = 3.0;
/// The largest grid cell, in output pixels.
const MAX_CELL: u32 = 32;
/// Soft Focus's edge: from sharp at Size to fully blurred at `Size × (1 + SOFT_EDGE)`.
pub const SOFT_EDGE: f64 = 0.5;
/// How much of Graduated Tint's colour is multiplied in at Shade 0 (all of it at Shade 1).
pub const TINT_FROM: f64 = 0.2;
/// Graduated Tint holds back where the darkest channel is above this, fully at white (1).
pub const WHITE_FROM: f32 = 0.8;

/// The picture's centre, where a crosshair starts.
const CENTRE: Point = Point { x: 0.5, y: 0.5 };

/// `soft_focus(x,y,size,amount)`: the picture blurred outside a sharp circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftFocus {
    pub centre: Point,
    pub size: f64,
    pub amount: f64,
}

impl Default for SoftFocus {
    fn default() -> SoftFocus {
        SoftFocus { centre: CENTRE, size: 0.3, amount: 0.5 }
    }
}

/// `focal_bw(x,y,size,sharpness)`: colour inside a circle, B&W outside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocalBw {
    pub centre: Point,
    pub size: f64,
    pub sharpness: f64,
}

impl Default for FocalBw {
    fn default() -> FocalBw {
        FocalBw { centre: CENTRE, size: 0.3, sharpness: 0.5 }
    }
}

/// `graduated_tint(x,y,angle,feather,shade,color)`: a coloured gradient from its start along its
/// angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraduatedTint {
    pub start: Point,
    pub angle: f64,
    pub feather: f64,
    pub shade: f64,
    pub color: [u8; 3],
}

impl Default for GraduatedTint {
    fn default() -> GraduatedTint {
        GraduatedTint { start: CENTRE, angle: 0.0, feather: 0.25, shade: 0.25, color: [0x9c, 0xc8, 0xff] }
    }
}

impl fmt::Display for SoftFocus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num_point(self.centre), num(self.size), num(self.amount))
    }
}

impl Operation for SoftFocus {
    const NAME: &'static str = "soft_focus";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Soft Focus";

    fn parse(params: &str) -> Result<SoftFocus, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let s = SoftFocus { centre: r.point("the centre")?, size: r.number("the size", SIZE)?, amount: r.number("the amount", AMOUNT)? };
        r.end()?;
        Ok(s)
    }

    fn clamped(self) -> SoftFocus {
        SoftFocus { centre: clamp_point(self.centre), size: clamp(self.size, SIZE), amount: clamp(self.amount, AMOUNT) }
    }
}

impl fmt::Display for FocalBw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{}", num_point(self.centre), num(self.size), num(self.sharpness))
    }
}

impl Operation for FocalBw {
    const NAME: &'static str = "focal_bw";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Focal B&W";

    fn parse(params: &str) -> Result<FocalBw, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let b = FocalBw { centre: r.point("the centre")?, size: r.number("the size", SIZE)?, sharpness: r.number("the sharpness", SHARPNESS)? };
        r.end()?;
        Ok(b)
    }

    fn clamped(self) -> FocalBw {
        FocalBw { centre: clamp_point(self.centre), size: clamp(self.size, SIZE), sharpness: clamp(self.sharpness, SHARPNESS) }
    }
}

impl fmt::Display for GraduatedTint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},{}", num_point(self.start), num(self.angle), num(self.feather), num(self.shade), hex(self.color))
    }
}

impl Operation for GraduatedTint {
    const NAME: &'static str = "graduated_tint";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Graduated Tint";

    fn parse(params: &str) -> Result<GraduatedTint, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let g = GraduatedTint {
            start: r.point("the start")?,
            angle: r.number("the angle", ANGLE)?,
            feather: r.number("the feather", FEATHER)?,
            shade: r.number("the shade", SHADE)?,
            color: r.colour("the colour")?,
        };
        r.end()?;
        Ok(g)
    }

    fn clamped(self) -> GraduatedTint {
        GraduatedTint {
            start: clamp_point(self.start),
            angle: clamp(self.angle, ANGLE),
            feather: clamp(self.feather, FEATHER),
            shade: clamp(self.shade, SHADE),
            color: self.color,
        }
    }
}

/// The circle about `centre`: 1 out to `size`, falling to 0 over `edge` (both relative to the
/// shorter side; at least one output pixel, so even a hard edge is smooth).
fn focal_circle(space: &Space, centre: Point, size: f64, edge: f64) -> Circle {
    let outer = size + edge.max(space.pixel());
    Circle::new(space, centre, outer, (outer - size) / outer)
}

impl Adjust for SoftFocus {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let s = self.clamped();
        if s.amount <= 0.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let sigma = space.length(s.amount * SOFT_SIGMA);
        // A wide blur runs on a grid of `cell × cell` pixel averages and is read back bilinearly:
        // the averaging and the reading add about cell²/4 to its variance, taken off the grid's.
        let cell = (sigma / CELL_SIGMA).floor().clamp(1.0, f64::from(MAX_CELL));
        let on_grid = (sigma * sigma - if cell > 1.0 { cell * cell / 4.0 } else { 0.0 }).max(0.0).sqrt() / cell;
        let (w, h) = space.size();
        let grid = (w.div_ceil(cell as u32), h.div_ceil(cell as u32));
        // Whole-pixel boxes blur no less than about 0.8 px: a smaller blur is that one mixed in by
        // the share of its variance, so a small render blurs as much as a large one scaled down.
        let gaussian = Gaussian::new(on_grid.max(MIN_SIGMA), grid);
        let strength = (on_grid / gaussian.sigma().max(1e-9)).powi(2).min(1.0) as f32;
        if gaussian.is_identity() || strength < 1e-4 {
            return None;
        }
        let circle = focal_circle(&space, s.centre, s.size, s.size * SOFT_EDGE);
        Some(Box::new(SoftFocusPass { gaussian, cell: cell as u32, strength, circle }))
    }
}

/// Soft Focus over the bands: the blur mixed in outside the circle. Each band blurs the grid
/// rows its pixels read, with the blur's reach around them, from the picture as it was: a plane
/// cut with that margin blurs exactly as the whole picture's, so a band never waits on another
/// and a region equals that window of the whole render.
struct SoftFocusPass {
    /// The blur on the grid.
    gaussian: Gaussian,
    /// The grid's cell side in output pixels (1: the blur runs on the pixels themselves).
    cell: u32,
    /// How much of the blur is mixed in (below 1 only for a blur under [`MIN_SIGMA`]).
    strength: f32,
    circle: Circle,
}

impl SoftFocusPass {
    /// The grid cell `(cx, cy)`: the mean of its pixels (the picture's edge pixels repeated past
    /// it, as [`crate::ops::View::get`] reads them).
    fn cell_mean(&self, view: &crate::ops::View<'_>, cx: u32, cy: u32) -> [f32; 3] {
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

    /// The blurred grid read back at the centre of the output pixel `(x, y)`, bilinearly.
    fn read(&self, plane: &Plane, x: u32, y: u32) -> [f32; 3] {
        if self.cell == 1 {
            return plane.get(i64::from(x), i64::from(y));
        }
        let c = f64::from(self.cell);
        let (u, v) = ((f64::from(x) + 0.5) / c - 0.5, (f64::from(y) + 0.5) / c - 0.5);
        let (i, j) = (u.floor(), v.floor());
        let (tx, ty) = ((u - i) as f32, (v - j) as f32);
        let (i, j) = (i as i64, j as i64);
        let top = mix(plane.get(i, j), plane.get(i + 1, j), tx);
        let bottom = mix(plane.get(i, j + 1), plane.get(i + 1, j + 1), tx);
        mix(top, bottom, ty)
    }
}

impl Pass for SoftFocusPass {
    fn reach(&self) -> Reach {
        // The grid's own reach, the bilinear read and the cells' extent, in output pixels.
        Reach::Pixels(self.gaussian.reach().saturating_add(3).saturating_mul(self.cell))
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        if band.width == 0 || band.rows == 0 || self.cell == 0 {
            return;
        }
        let (x1, y1) = (band.x0.saturating_add(band.width - 1), band.y0.saturating_add(band.rows - 1));
        // A band wholly inside the sharp core (the disc is convex, so its corners tell) is unchanged.
        if [(band.x0, band.y0), (x1, band.y0), (band.x0, y1), (x1, y1)].iter().all(|&(x, y)| self.circle.at(x, y) >= 1.0) {
            return;
        }
        // The grid rows under the band's rows and the reach around them, within the window the
        // render holds; the grid columns under the window.
        let Reach::Pixels(reach) = self.reach() else { return };
        let top = band.y0.saturating_sub(reach).max(view.y0) / self.cell;
        let bottom = y1.saturating_add(reach).saturating_add(1).min(view.y0.saturating_add(view.height)).div_ceil(self.cell);
        let left = view.x0 / self.cell;
        let right = view.x0.saturating_add(view.width).div_ceil(self.cell);
        let Some(mut plane) = Plane::from_fn(left, top, right.saturating_sub(left), bottom.saturating_sub(top), |cx, cy| {
            if self.cell == 1 { view.get(i64::from(cx), i64::from(cy)) } else { self.cell_mean(view, cx, cy) }
        }) else {
            return;
        };
        if !self.gaussian.apply(&mut plane) {
            return;
        }
        for j in 0..band.rows {
            let y = band.y0.saturating_add(j);
            for i in 0..band.width {
                let x = band.x0.saturating_add(i);
                let outside = (1.0 - self.circle.at(x, y)) * self.strength;
                if outside <= 0.0 {
                    continue;
                }
                let blurred = self.read(&plane, x, y);
                if let Some(px) = band.get_mut(i, j) {
                    *px = mix(*px, blurred, outside);
                }
            }
        }
    }
}

impl Adjust for FocalBw {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let b = self.clamped();
        let space = Space::new(ctx)?;
        let circle = focal_circle(&space, b.centre, b.size, (1.0 - b.sharpness) * b.size);
        Some(Box::new(FocalBwPass { circle }))
    }
}

/// Focal B&W over the bands: grey outside the circle.
struct FocalBwPass {
    circle: Circle,
}

impl Pass for FocalBwPass {
    fn run(&self, band: &mut Band<'_>) {
        for j in 0..band.rows {
            let y = band.y0.saturating_add(j);
            for i in 0..band.width {
                let inside = self.circle.at(band.x0.saturating_add(i), y);
                if inside >= 1.0 {
                    continue;
                }
                if let Some(px) = band.get_mut(i, j) {
                    let l = luma(*px);
                    *px = mix([l; 3], *px, inside);
                }
            }
        }
    }
}

impl Adjust for GraduatedTint {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let g = self.clamped();
        if g.color == [0xff; 3] {
            return None;
        }
        let space = Space::new(ctx)?;
        let gradient = Gradient::new(&space, g.start, g.angle, g.feather.max(space.pixel()));
        // The colour multiplied in, by `TINT_FROM` at Shade 0 up to all of it at Shade 1.
        let depth = TINT_FROM + (1.0 - TINT_FROM) * g.shade;
        let gains = g.color.map(|c| (1.0 - depth * (1.0 - f64::from(c) / 255.0)) as f32);
        Some(Box::new(TintPass { gradient, gains }))
    }
}

/// Graduated Tint over the bands: each channel times its gain ahead of the start.
struct TintPass {
    gradient: Gradient,
    gains: [f32; 3],
}

impl Pass for TintPass {
    fn run(&self, band: &mut Band<'_>) {
        for j in 0..band.rows {
            let y = band.y0.saturating_add(j);
            for i in 0..band.width {
                let ahead = self.gradient.at(band.x0.saturating_add(i), y);
                if ahead <= 0.0 {
                    continue;
                }
                if let Some(px) = band.get_mut(i, j) {
                    let darkest = px[0].min(px[1]).min(px[2]);
                    let keep = smoothstep(f64::from(WHITE_FROM), 1.0, f64::from(darkest)) as f32;
                    let tinted = [px[0] * self.gains[0], px[1] * self.gains[1], px[2] * self.gains[2]];
                    *px = mix(*px, tinted, ahead * (1.0 - keep));
                }
            }
        }
    }
}
