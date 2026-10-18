//! The effects that change the picture's size, part two (`editing.md` §8.4):
//! `drop_shadow(color,distance,angle,blur,fade,background)` and
//! `museum_matte(outer_color,outer,inner_color,inner)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5):
//!   - Drop Shadow: the shadow colour (`#000000`); Distance 0–30 (15) and Blur 0–100 (25), in
//!     thousandths of the picture's shorter side; Angle 0–360 (90), the direction the shadow
//!     falls in degrees clockwise from pointing up (90: to the right); Fade 0–100 (30); the background
//!     colour (`#ffffff`). The canvas grows to hold the shadow.
//!   - Museum Matte: the outer mat's colour (`#1a0e03`) and thickness 0–100 (25), the inner mat's
//!     colour (`#f0eae4`) and thickness 0–100 (40), thicknesses in thousandths of the shorter
//!     side. Repeatable.
//!
//!   Numbers out of range are clamped; `NaN`, a missing or extra field or a colour that is not
//!   `#rrggbb` is refused (the entry is kept and skipped).
//! - **Rendering** (M12.14, `VERSION` 1): both are frames ([`Reframe`], M12.13's kind): each puts
//!   the picture as it stands at its place among the effects on a larger canvas, in whole pixels,
//!   and paints around it; the photo's pixels are copied unchanged. Sizes are relative to the
//!   shorter side of the picture the frame wraps, in output pixels, so every render size, a zoomed
//!   region and a Save agree.
//!   - Drop Shadow centres the photo on a canvas grown on every side by the shadow's Distance and
//!     Blur and a gap of [`GAP`] thousandths, so the shadow never reaches the canvas's edge and
//!     every corner is the background. The shadow is the photo's rectangle moved by Distance along
//!     Angle (clockwise from up, `offset`) and blurred by a tent of half-width Blur (two box
//!     passes, `tent_kernel`), as PhotoCraft's `shadow_map` builds it; for a rectangle that blur is
//!     separable and has a closed form, so it is two per-render profiles (one per column, one per
//!     row, each pixel averaged over four samples) instead of a blurred plane. Its opacity is
//!     `1 − Fade/100`; it is blended over the background in the shadow colour.
//!   - Museum Matte grows the canvas by the inner and then the outer mat on every side (the two
//!     together rounded to whole pixels, so the photo lands on whole pixels; the line between the
//!     mats anti-aliased where it falls inside a pixel). The outer mat is flat; the inner mat
//!     carries an inner shadow cast by the outer mat's edge, as if lit from the top left: the
//!     opening moved down and right by [`MAT_SHADOW`] and blurred the same way, at [`MAT_OPACITY`],
//!     both limited to the inner mat's thickness. Repeatable: a second matte frames the first.
//!
//! Drop Shadow from PhotoCraft `crates/compose/src/effects.rs` (`shadow_map`, `tent_kernel`,
//! `offset`) at e4b2868a (`NOTICE`), re-derived in closed form for a rectangle; Museum Matte our own
//! design from `editing.md` §8.4.

use std::fmt;

use super::{Band, DEGREES, FADE, Operation, ParamError, Pass, Reader, clamp, hex, num};
use crate::render::{Layout, MAX_EDGE, Placed, Reframe, pixels};

/// The range of Drop Shadow's Distance, thousandths of the shorter side.
pub const DISTANCE: (f64, f64) = (0.0, 30.0);
/// The range of Drop Shadow's Angle, degrees clockwise from pointing up.
pub const ANGLE: (f64, f64) = DEGREES;
/// The range of Drop Shadow's Blur, thousandths of the shorter side.
pub const BLUR: (f64, f64) = (0.0, 100.0);
/// The range of a Museum Matte mat's thickness, thousandths of the shorter side.
pub const THICKNESS: (f64, f64) = (0.0, 100.0);
/// Drop Shadow's gap between the shadow's reach and the canvas's edge, thousandths of the shorter
/// side.
pub const GAP: f64 = 25.0;
/// Museum Matte's inner shadow: its distance and blur, thousandths of the shorter side (each
/// limited so the two together fit in the inner mat), and the direction it falls (degrees clockwise
/// from up: down and to the right).
pub const MAT_SHADOW: (f64, f64, f64) = (6.0, 12.0, 135.0);
/// Museum Matte's inner shadow at its darkest.
pub const MAT_OPACITY: f32 = 0.55;

/// `drop_shadow(color,distance,angle,blur,fade,background)`: the photo floating over a background
/// with its shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropShadow {
    pub color: [u8; 3],
    pub distance: f64,
    pub angle: f64,
    pub blur: f64,
    pub fade: f64,
    pub background: [u8; 3],
}

impl Default for DropShadow {
    fn default() -> DropShadow {
        DropShadow { color: [0, 0, 0], distance: 15.0, angle: 90.0, blur: 25.0, fade: 30.0, background: [0xff, 0xff, 0xff] }
    }
}

/// `museum_matte(outer_color,outer,inner_color,inner)`: a double mat with an inner shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MuseumMatte {
    pub outer_color: [u8; 3],
    pub outer: f64,
    pub inner_color: [u8; 3],
    pub inner: f64,
}

impl Default for MuseumMatte {
    fn default() -> MuseumMatte {
        MuseumMatte { outer_color: [0x1a, 0x0e, 0x03], outer: 25.0, inner_color: [0xf0, 0xea, 0xe4], inner: 40.0 }
    }
}

impl fmt::Display for DropShadow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},{},{}", hex(self.color), num(self.distance), num(self.angle), num(self.blur), num(self.fade), hex(self.background))
    }
}

impl Operation for DropShadow {
    const NAME: &'static str = "drop_shadow";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Drop Shadow";

    fn parse(params: &str) -> Result<DropShadow, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let d = DropShadow {
            color: r.colour("the shadow colour")?,
            distance: r.number("the distance", DISTANCE)?,
            angle: r.number("the angle", ANGLE)?,
            blur: r.number("the blur", BLUR)?,
            fade: r.number("fade", FADE)?,
            background: r.colour("the background colour")?,
        };
        r.end()?;
        Ok(d)
    }

    fn clamped(self) -> DropShadow {
        DropShadow {
            distance: clamp(self.distance, DISTANCE),
            angle: clamp(self.angle, ANGLE),
            blur: clamp(self.blur, BLUR),
            fade: clamp(self.fade, FADE),
            ..self
        }
    }
}

impl fmt::Display for MuseumMatte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{}", hex(self.outer_color), num(self.outer), hex(self.inner_color), num(self.inner))
    }
}

impl Operation for MuseumMatte {
    const NAME: &'static str = "museum_matte";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Museum Matte";

    fn parse(params: &str) -> Result<MuseumMatte, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let m = MuseumMatte {
            outer_color: r.colour("the outer colour")?,
            outer: r.number("the outer thickness", THICKNESS)?,
            inner_color: r.colour("the inner colour")?,
            inner: r.number("the inner thickness", THICKNESS)?,
        };
        r.end()?;
        Ok(m)
    }

    fn clamped(self) -> MuseumMatte {
        MuseumMatte { outer: clamp(self.outer, THICKNESS), inner: clamp(self.inner, THICKNESS), ..self }
    }
}

/// The shift of a shadow `distance` pixels long falling at `angle` degrees clockwise from up (90:
/// to the right), in canvas pixels (y down). After PhotoCraft's `offset`, which takes the light's
/// angle instead and rounds to whole pixels; here the shift stays fractional, so every render size
/// agrees.
fn offset(angle: f64, distance: f64) -> (f64, f64) {
    let (s, c) = angle.to_radians().sin_cos();
    (s * distance, -c * distance)
}

/// The share of a unit step at 0 blurred by a tent of half-width `w` that lies at or left of `t`:
/// the tent's cumulative distribution (two box passes of width `w`, PhotoCraft's `tent_kernel`, in
/// the continuous limit); a hard step when `w` is (nearly) 0.
fn tent_cdf(t: f64, w: f64) -> f64 {
    if w <= 1e-9 {
        return if t >= 0.0 { 1.0 } else { 0.0 };
    }
    let u = (t / w).clamp(-1.0, 1.0);
    if u < 0.0 { 0.5 * (1.0 + u) * (1.0 + u) } else { 1.0 - 0.5 * (1.0 - u) * (1.0 - u) }
}

/// The interval `lo..hi` blurred by a tent of half-width `w`, for each of `n` pixels of a line: the
/// mean of four samples across the pixel (at most [`MAX_EDGE`] pixels; reads past the end are 0).
fn profile(n: u32, lo: f64, hi: f64, w: f64) -> Vec<f32> {
    (0..n.min(MAX_EDGE))
        .map(|p| {
            let at = |k: u32| f64::from(p) + (f64::from(k) + 0.5) / 4.0;
            let sum: f64 = (0..4).map(|k| tent_cdf(at(k) - lo, w) - tent_cdf(at(k) - hi, w)).sum();
            (sum / 4.0).clamp(0.0, 1.0) as f32
        })
        .collect()
}

/// How much of pixel `p` (the interval `p..p + 1`) lies in `lo..hi`.
fn overlap(p: u32, lo: f64, hi: f64) -> f32 {
    let p = f64::from(p);
    ((p + 1.0).min(hi) - p.max(lo)).clamp(0.0, 1.0) as f32
}

/// `c` in working values (0..1).
fn rgb(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| f32::from(v) / 255.0)
}

/// The photo's pixels on the canvas: `x0..x1 × y0..y1`.
#[derive(Clone, Copy, Debug)]
struct Photo {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl Photo {
    fn of(placed: &Placed<'_>) -> Photo {
        let (x0, y0) = (placed.layout.at.0.clamp(0, i64::from(u32::MAX)) as u32, placed.layout.at.1.clamp(0, i64::from(u32::MAX)) as u32);
        Photo { x0, y0, x1: x0.saturating_add(placed.input.0), y1: y0.saturating_add(placed.input.1) }
    }

    fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// Visits each pixel of `band` outside `photo` with its whole-canvas position.
fn around(band: &mut Band<'_>, photo: Photo, mut paint: impl FnMut(u32, u32, &mut [f32; 3])) {
    let width = band.width as usize;
    if width == 0 {
        return;
    }
    let (x0, y0) = (band.x0, band.y0);
    for (i, p) in band.pixels.iter_mut().enumerate() {
        let x = x0.saturating_add((i % width) as u32);
        let y = y0.saturating_add((i / width) as u32);
        if !photo.contains(x, y) {
            paint(x, y, p);
        }
    }
}

impl DropShadow {
    /// The shadow's distance and blur and the canvas's margin around a `width × height` picture,
    /// in pixels.
    fn sizes(&self, width: u32, height: u32) -> (f64, f64, f64) {
        let d = self.clamped();
        let short = f64::from(width.min(height)) / 1000.0;
        let (distance, blur) = (d.distance * short, d.blur * short);
        (distance, blur, distance + blur + GAP * short)
    }
}

impl Reframe for DropShadow {
    fn layout(&self, width: u32, height: u32) -> Option<Layout> {
        let (_, _, margin) = self.sizes(width, height);
        let m = pixels(margin)?;
        let both = m.checked_mul(2)?;
        let size = (width.checked_add(both)?, height.checked_add(both)?);
        Some(Layout { size, at: (i64::from(m), i64::from(m)) })
    }

    fn prepare(&self, placed: &Placed<'_>) -> Option<Box<dyn Pass>> {
        let d = self.clamped();
        let (distance, blur, _) = self.sizes(placed.input.0, placed.input.1);
        let photo = Photo::of(placed);
        let (dx, dy) = offset(d.angle, distance);
        let (w, h) = placed.layout.size;
        let across = profile(w, f64::from(photo.x0) + dx, f64::from(photo.x1) + dx, blur);
        let down = profile(h, f64::from(photo.y0) + dy, f64::from(photo.y1) + dy, blur);
        let opacity = (1.0 - d.fade / 100.0).clamp(0.0, 1.0) as f32;
        Some(Box::new(Shadow { photo, across, down, opacity, colour: rgb(d.color), background: rgb(d.background) }))
    }
}

/// Drop Shadow's background and shadow around the photo.
struct Shadow {
    photo: Photo,
    /// The blurred shadow's coverage per canvas column and per row.
    across: Vec<f32>,
    down: Vec<f32>,
    opacity: f32,
    colour: [f32; 3],
    background: [f32; 3],
}

impl Pass for Shadow {
    fn run(&self, band: &mut Band<'_>) {
        around(band, self.photo, |x, y, p| {
            let at = |v: &[f32], i: u32| v.get(i as usize).copied().unwrap_or(0.0);
            let a = self.opacity * at(&self.across, x) * at(&self.down, y);
            for ((v, b), s) in p.iter_mut().zip(self.background).zip(self.colour) {
                *v = b + (s - b) * a;
            }
        });
    }
}

impl MuseumMatte {
    /// The outer and inner mats around a `width × height` picture, in pixels.
    fn mats(&self, width: u32, height: u32) -> (f64, f64) {
        let m = self.clamped();
        let short = f64::from(width.min(height)) / 1000.0;
        (m.outer * short, m.inner * short)
    }
}

impl Reframe for MuseumMatte {
    fn layout(&self, width: u32, height: u32) -> Option<Layout> {
        let (outer, inner) = self.mats(width, height);
        let band = pixels(outer + inner)?;
        let both = band.checked_mul(2)?;
        let size = (width.checked_add(both)?, height.checked_add(both)?);
        Some(Layout { size, at: (i64::from(band), i64::from(band)) })
    }

    fn prepare(&self, placed: &Placed<'_>) -> Option<Box<dyn Pass>> {
        let m = self.clamped();
        let (_, inner) = self.mats(placed.input.0, placed.input.1);
        let photo = Photo::of(placed);
        // The opening in the outer mat: the inner mat's outer edge.
        let open = (f64::from(photo.x0) - inner, f64::from(photo.y0) - inner, f64::from(photo.x1) + inner, f64::from(photo.y1) + inner);
        let short = f64::from(placed.input.0.min(placed.input.1)) / 1000.0;
        let (distance, blur, angle) = MAT_SHADOW;
        let (distance, blur) = (distance * short, blur * short);
        let fit = if distance + blur > inner { inner / (distance + blur) } else { 1.0 };
        let (distance, blur) = (distance * fit, blur * fit);
        let (dx, dy) = offset(angle, distance);
        let (w, h) = placed.layout.size;
        let across = profile(w, open.0 + dx, open.2 + dx, blur);
        let down = profile(h, open.1 + dy, open.3 + dy, blur);
        Some(Box::new(Mats { photo, open, across, down, outer: rgb(m.outer_color), inner: rgb(m.inner_color) }))
    }
}

/// Museum Matte's two mats around the photo, the inner one shaded along the outer one's edge.
struct Mats {
    photo: Photo,
    /// The outer mat's opening: left, top, right, bottom, in canvas pixels.
    open: (f64, f64, f64, f64),
    /// The opening moved and blurred, per canvas column and per row: where the inner mat is lit.
    across: Vec<f32>,
    down: Vec<f32>,
    outer: [f32; 3],
    inner: [f32; 3],
}

impl Pass for Mats {
    fn run(&self, band: &mut Band<'_>) {
        let (l, t, r, b) = self.open;
        around(band, self.photo, |x, y, p| {
            let at = |v: &[f32], i: u32| v.get(i as usize).copied().unwrap_or(0.0);
            let inside = overlap(x, l, r) * overlap(y, t, b);
            let shade = 1.0 - MAT_OPACITY * (1.0 - at(&self.across, x) * at(&self.down, y));
            for ((v, i), o) in p.iter_mut().zip(self.inner).zip(self.outer) {
                *v = i * shade * inside + o * (1.0 - inside);
            }
        });
    }
}
