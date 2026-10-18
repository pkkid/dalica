//! The stored frame as it lies in the output of one render (`data-model.md` §3.3): effect centres,
//! gradients, strokes and noise are placed in the stored frame, and every length is relative to the
//! stored photo's shorter side, so a thumbnail, the edit view, a zoomed region and a Save of the
//! same recipe agree. The crop and the straighten move the stored frame in the output by a
//! similarity (a shift, a turn and one scale), so lengths measured here are the same along every
//! direction. The recipe's rotations and flips come after every effect and do not enter.

use crate::Point;
use crate::ops::{Affine, Prepare};
use crate::render::Geometry;

/// The longest length [`Space::length`] gives, in output pixels: past any picture a render makes
/// (`render::MAX_EDGE` is 2¹⁸), so a hostile relative size stays a bounded number.
pub const MAX_LENGTH: f64 = (1u64 << 20) as f64;

/// The stored frame in the output of one render, before the recipe's rotations and flips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Space {
    /// From normalised stored-frame points to output pixel coordinates (a pixel's centre at
    /// `+ 0.5`), and back.
    to_out: Affine,
    to_stored: Affine,
    /// The stored frame's sides in output pixels.
    sides: (f64, f64),
    width: u32,
    height: u32,
}

impl Space {
    /// The space of the render an operation is prepared for.
    pub fn new(ctx: &Prepare<'_>) -> Option<Space> {
        Space::from_geometry(ctx.geometry, ctx.width, ctx.height)
    }

    /// The space of a `width × height` output (before the rotations and flips) of `geometry`;
    /// `None` when the geometry maps no usable frame (an empty output, a degenerate warp).
    pub fn from_geometry(geometry: &Geometry, width: u32, height: u32) -> Option<Space> {
        if width == 0 || height == 0 {
            return None;
        }
        let (w, h) = (f64::from(width), f64::from(height));
        let out = |p: Point| geometry.stored_to_output(p).map(|q| (q.x * w, q.y * h));
        let o = out(Point::new(0.0, 0.0))?;
        let ex = out(Point::new(1.0, 0.0))?;
        let ey = out(Point::new(0.0, 1.0))?;
        let (ax, ay) = (ex.0 - o.0, ex.1 - o.1);
        let (bx, by) = (ey.0 - o.0, ey.1 - o.1);
        let to_out = Affine { a: ax, b: bx, c: o.0, d: ay, e: by, f: o.1 };
        let to_stored = to_out.inverse()?;
        let sides = (ax.hypot(ay), bx.hypot(by));
        let usable = sides.0.is_finite() && sides.1.is_finite() && sides.0.min(sides.1) > 1e-9;
        usable.then_some(Space { to_out, to_stored, sides, width, height })
    }

    /// The whole output's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The stored photo's shorter side in output pixels: what relative lengths are relative to.
    pub fn short(&self) -> f64 {
        self.sides.0.min(self.sides.1)
    }

    /// A length relative to the stored photo's shorter side, in output pixels: 0 for a negative
    /// or `NaN` one, at most [`MAX_LENGTH`].
    pub fn length(&self, relative: f64) -> f64 {
        let px = super::finite_or(relative, 0.0).max(0.0) * self.short();
        super::finite_or(px, MAX_LENGTH).min(MAX_LENGTH)
    }

    /// Where the normalised stored-frame point `p` lies in the output, in pixel coordinates.
    pub fn to_output(&self, p: Point) -> (f64, f64) {
        self.to_out.apply(p.x, p.y)
    }

    /// The normalised stored-frame point at the output pixel coordinates `(x, y)`.
    pub fn to_stored(&self, x: f64, y: f64) -> Point {
        let (u, v) = self.to_stored.apply(x, y);
        Point::new(u, v)
    }

    /// The normalised stored-frame point `p` in units of the stored photo's shorter side, with the
    /// origin at its top-left corner: the same at every scale and the same along every direction
    /// (for distances, angles and noise).
    pub fn units(&self, p: Point) -> (f64, f64) {
        let s = self.short();
        (p.x * self.sides.0 / s, p.y * self.sides.1 / s)
    }

    /// The centre of the output pixel `(x, y)` (whole-output coordinates) in units of the stored
    /// photo's shorter side ([`Space::units`]).
    pub fn pixel_units(&self, x: u32, y: u32) -> (f64, f64) {
        self.units(self.to_stored(f64::from(x) + 0.5, f64::from(y) + 0.5))
    }

    /// One output pixel in units of the stored photo's shorter side.
    pub fn pixel(&self) -> f64 {
        1.0 / self.short()
    }
}
