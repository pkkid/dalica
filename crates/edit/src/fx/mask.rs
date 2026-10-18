//! Masks in output pixels from shapes placed in the stored frame: each answers a weight in 0..1 at
//! an output pixel's centre (whole-output coordinates), so a band, a region and the whole render
//! agree. Distances are measured in units of the stored photo's shorter side
//! ([`Space::units`]), so a circle stays round under a straighten and the same size at every
//! scale; [`Edges`] measures from the output picture's own edges (after the crop).
//!
//! Our own design from `editing.md` §8 (the focal effects' circles, Graduated Tint's gradient,
//! Vignette's and Plastic Lens's edges).

use super::space::Space;
use crate::Point;

/// A disc with a feathered edge: 1 out to `radius × (1 − feather)`, falling smoothly to 0 at
/// `radius` (as a brush dab, [`super::brush::dab_alpha`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    space: Space,
    centre: (f64, f64),
    radius: f64,
    hard: f64,
}

impl Circle {
    /// The disc about the stored-frame point `centre` with `radius` relative to the stored photo's
    /// shorter side and `feather` the fraction of the radius that fades (0..1). A radius that is
    /// not a positive number gives an empty disc; a feather that is not a number counts as 0.
    pub fn new(space: &Space, centre: Point, radius: f64, feather: f64) -> Circle {
        let radius = if radius.is_finite() { radius.clamp(0.0, 1e6) } else { 0.0 };
        let feather = if feather.is_nan() { 0.0 } else { feather.clamp(0.0, 1.0) };
        Circle { space: *space, centre: space.units(centre), radius, hard: radius * (1.0 - feather) }
    }

    /// The weight at the output pixel `(x, y)`.
    pub fn at(&self, x: u32, y: u32) -> f32 {
        let (u, v) = self.space.pixel_units(x, y);
        let d = (u - self.centre.0).hypot(v - self.centre.1);
        if !d.is_finite() || d >= self.radius {
            return 0.0;
        }
        (1.0 - super::smoothstep(self.hard, self.radius, d)) as f32
    }
}

/// A linear gradient: 0 behind the line through `start` square to `angle`, rising smoothly to 1
/// at `feather` ahead of it. The angle (degrees, in the stored frame) is the direction it rises
/// toward, clockwise from up: 0 rises toward the top, 90 toward the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    space: Space,
    start: (f64, f64),
    dir: (f64, f64),
    feather: f64,
}

impl Gradient {
    /// The gradient from the stored-frame point `start`, rising toward `angle` degrees over
    /// `feather` (relative to the stored photo's shorter side; 0 is a hard edge). Numbers that are
    /// not finite count as 0.
    pub fn new(space: &Space, start: Point, angle: f64, feather: f64) -> Gradient {
        let a = super::finite_or(angle, 0.0).rem_euclid(360.0).to_radians();
        let feather = super::finite_or(feather, 0.0).clamp(0.0, 1e6);
        Gradient { space: *space, start: space.units(start), dir: (a.sin(), -a.cos()), feather }
    }

    /// The weight at the output pixel `(x, y)`.
    pub fn at(&self, x: u32, y: u32) -> f32 {
        let (u, v) = self.space.pixel_units(x, y);
        let d = (u - self.start.0) * self.dir.0 + (v - self.start.1) * self.dir.1;
        if !d.is_finite() {
            return 0.0;
        }
        super::smoothstep(0.0, self.feather, d) as f32
    }
}

/// The distance of a pixel from the output picture's nearest edge, relative to its shorter side:
/// 0 on the edge, 0.5 at the middle of the shorter side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges {
    width: f64,
    height: f64,
    short: f64,
}

impl Edges {
    pub fn new(space: &Space) -> Edges {
        let (w, h) = space.size();
        let (width, height) = (f64::from(w), f64::from(h));
        Edges { width, height, short: width.min(height).max(1.0) }
    }

    /// The distance at the centre of the output pixel `(x, y)`; 0 outside the picture.
    pub fn at(&self, x: u32, y: u32) -> f64 {
        let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        let d = px.min(self.width - px).min(py).min(self.height - py);
        d.max(0.0) / self.short
    }
}
