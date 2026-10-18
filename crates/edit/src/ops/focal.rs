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
//!   - Soft Focus: a Gaussian of `Amount × SOFT_SIGMA` of the shorter side, mixed in by the
//!     outside of a [`Circle`] that is sharp out to Size and fully blurred at `Size × (1 +
//!     SOFT_EDGE)`. One [`Reach::Pixels`] pass on M13.34's [`GridBlur`] (M13.37): a grid of cell
//!     means (a cell about a third of the sigma; the pixels themselves under three pixels of
//!     sigma) blurred once per render by the first band that needs it, over the whole window the
//!     render holds, with the variance the averaging and the bilinear read add taken off, and one
//!     under the smallest box blur mixed in by the share of its variance, so every render size
//!     blurs alike; a region render equals that window of the whole render. Each row solves the
//!     circle once (its squared distance is a quadratic along the row): the sharp core is left
//!     as it is without reading the blur, the edge is weighed pixel by pixel, and the rest takes
//!     the blur in full on flat runs, read along the band's rows by [`crate::fx::GridRows`]
//!     (M13.47: each column's taps once per band, each grid row spread across the band once, bit
//!     for bit [`GridBlur::read`]). Amount 0 changes nothing.
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
use crate::fx::{Circle, Gradient, GridBlur, Space, Window, luma, mix, smoothstep};

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
        let blur = GridBlur::new(space.length(s.amount * SOFT_SIGMA), space.size())?;
        let circle = focal_circle(&space, s.centre, s.size, s.size * SOFT_EDGE);
        let rows = Rows::new(space, s.centre, s.size, s.size * SOFT_EDGE);
        Some(Box::new(SoftFocusPass { blur, circle, rows }))
    }
}

/// Soft Focus over the bands: the blur mixed in outside the circle. The blur is made once per
/// render ([`GridBlur`], by the first band that needs it, over the whole window the render holds)
/// and read back row by row where a row leaves the sharp core.
struct SoftFocusPass {
    blur: GridBlur,
    /// The circle, for a band's corners.
    circle: Circle,
    /// The circle along each row.
    rows: Rows,
}

/// The circle of [`focal_circle`] along each output row. Along a row the stored frame moves by a
/// similarity, so a pixel's squared distance from the centre is a quadratic in its column: solved
/// once per row, it gives the columns surely inside the sharp core (the mask exactly 1, the pixel
/// unchanged) and those that may lie under the circle at all (outside them the mask is exactly 0,
/// the blur mixed in fully); between the two the mask is that quadratic's distance through the
/// edge, the same in every band and region.
struct Rows {
    space: Space,
    /// The centre in units of the stored photo's shorter side ([`Space::units`]).
    centre: (f64, f64),
    /// The sharp core's radius and the outer one, in the same units, as [`Circle::new`] makes
    /// them for [`focal_circle`].
    hard: f64,
    outer: f64,
}

/// The columns `lo..=hi` of a row; empty when `lo > hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Span {
    lo: i64,
    hi: i64,
}

impl Span {
    const NONE: Span = Span { lo: 1, hi: 0 };

    fn contains(self, x: i64) -> bool {
        self.lo <= x && x <= self.hi
    }
}

/// How far, in output pixels, a row's spans keep from the radii they are solved for: far more
/// than the rounding of the solution, so a column inside the core span has its mask exactly 1 and
/// one outside the outer span exactly 0.
const SPAN_MARGIN: f64 = 2.0;

/// One row of the circle: the squared distance `a x² + 2 b x + c` of the column `x`, and its
/// spans.
struct Row {
    a: f64,
    b: f64,
    c: f64,
    /// The columns whose mask is exactly 1.
    core: Span,
    /// The columns whose mask may be above 0.
    under: Span,
}

impl Rows {
    fn new(space: Space, centre: Point, size: f64, edge: f64) -> Rows {
        // As `focal_circle` and `Circle::new` place them.
        let outer = size + edge.max(space.pixel());
        let radius = if outer.is_finite() { outer.clamp(0.0, 1e6) } else { 0.0 };
        let feather = (outer - size) / outer;
        let feather = if feather.is_nan() { 0.0 } else { feather.clamp(0.0, 1.0) };
        Rows { space, centre: space.units(centre), hard: radius * (1.0 - feather), outer: radius }
    }

    /// The circle along the output row `y`.
    fn row(&self, y: u32) -> Row {
        let p0 = self.space.pixel_units(0, y);
        let p1 = self.space.pixel_units(1, y);
        let d = (p1.0 - p0.0, p1.1 - p0.1);
        let e = (p0.0 - self.centre.0, p0.1 - self.centre.1);
        let (a, b, c) = (d.0 * d.0 + d.1 * d.1, e.0 * d.0 + e.1 * d.1, e.0 * e.0 + e.1 * e.1);
        let all = Span { lo: i64::MIN, hi: i64::MAX };
        if ![a, b, c, self.hard, self.outer].iter().all(|v| v.is_finite()) || a <= 0.0 {
            // Nothing to solve: every column reads its distance (not a number: mask 0).
            return Row { a, b, c, core: Span::NONE, under: all };
        }
        // The real columns within `r` of the centre.
        let within = |r: f64| {
            let disc = b * b - a * (c - r * r);
            (r > 0.0 && disc >= 0.0).then(|| {
                let s = disc.sqrt();
                ((-b - s) / a, (-b + s) / a)
            })
        };
        let margin = SPAN_MARGIN * a.sqrt();
        let core = within(self.hard - margin).map_or(Span::NONE, |(lo, hi)| Span { lo: lo.ceil() as i64, hi: hi.floor() as i64 });
        let under = within(self.outer + margin).map_or(Span::NONE, |(lo, hi)| Span { lo: lo.floor() as i64, hi: hi.ceil() as i64 });
        Row { a, b, c, core, under }
    }

    /// The mask at the column `x` of `row`: 1 inside the core, falling to 0 at the outer radius.
    fn at(&self, row: &Row, x: u32) -> f32 {
        let i = i64::from(x);
        if row.core.contains(i) {
            return 1.0;
        }
        if !row.under.contains(i) {
            return 0.0;
        }
        let x = f64::from(x);
        let d = (row.a * x * x + 2.0 * row.b * x + row.c).max(0.0).sqrt();
        if !d.is_finite() || d >= self.outer {
            return 0.0;
        }
        (1.0 - smoothstep(self.hard, self.outer, d)) as f32
    }
}

impl Pass for SoftFocusPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.blur.reach())
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        if band.width == 0 || band.rows == 0 {
            return;
        }
        let (x1, y1) = (band.x0.saturating_add(band.width - 1), band.y0.saturating_add(band.rows - 1));
        // A band wholly inside the sharp core (the disc is convex, so its corners tell) is unchanged
        // and never asks for the blur.
        if [(band.x0, band.y0), (x1, band.y0), (band.x0, y1), (x1, y1)].iter().all(|&(x, y)| self.circle.at(x, y) >= 1.0) {
            return;
        }
        let Some(grid) = Window::of(view).and_then(|w| self.blur.rgb(w)) else { return };
        let width = band.width as usize;
        // The grid read along the band's rows (each column's taps found once per band).
        let Some(mut rows) = self.blur.rows(grid, band.x0, width) else { return };
        let mut line = vec![[0.0f32; 3]; width];
        let strength = self.blur.strength();
        let (bx0, bx1) = (i64::from(band.x0), i64::from(x1));
        // A span's columns as offsets into the band: `start..end`.
        let local = |span: Span| {
            if span.lo > span.hi {
                return (0, 0);
            }
            let clip = |x: i64| (x.clamp(bx0, bx1.saturating_add(1)) - bx0) as usize;
            (clip(span.lo), clip(span.hi.saturating_add(1)))
        };
        for (j, pixels) in band.pixels.chunks_exact_mut(width).enumerate() {
            let y = band.y0.saturating_add(j as u32);
            // Fully blurred outside the circle, the edge read pixel by pixel, the core unchanged.
            let row = self.rows.row(y);
            let (u0, u1) = local(row.under);
            let (c0, c1) = local(row.core);
            let (c0, c1) = if c0 < c1 { (c0.max(u0), c1.min(u1)) } else { (u0, u0) };
            if c0 == 0 && c1 == width {
                // A row wholly in the core never reads the blur.
                continue;
            }
            rows.read(y, line.as_flattened_mut());
            for (s, e) in [(0, u0), (u1, width)] {
                let (Some(pixels), Some(blurred)) = (pixels.get_mut(s..e), line.get(s..e)) else { continue };
                for (px, b) in pixels.iter_mut().zip(blurred) {
                    *px = lerp(*px, *b, strength);
                }
            }
            for (s, e) in [(u0, c0), (c1, u1)] {
                let (Some(pixels), Some(blurred)) = (pixels.get_mut(s..e), line.get(s..e)) else { continue };
                for (k, (px, b)) in pixels.iter_mut().zip(blurred).enumerate() {
                    let x = band.x0.saturating_add((s + k) as u32);
                    let outside = (1.0 - self.rows.at(&row, x)) * strength;
                    if outside > 0.0 {
                        *px = lerp(*px, *b, outside);
                    }
                }
            }
        }
    }
}

/// `a` moved toward `b` by `t`, which is already in 0..1 ([`mix`] without its clamp).
fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
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
