//! The coordinate frame of stored rectangles and points (`data-model.md` §3.3, reserved decision
//! 11, decided 2026-10-12): the crop, retouch spots, the centres of the focal effects and face
//! rectangles are stored normalised to the photo **after its EXIF orientation and before Dalica's
//! own rotations and flips** (the *stored frame*). What the user sees is that picture turned by the
//! recipe's rotations and flips ([`Recipe::orientation`](crate::Recipe::orientation)), the
//! *displayed frame*; coordinates are converted between the two only to draw them and to read the
//! user's clicks and drags back. A rotation or a flip therefore stays a one-entry recipe change and
//! never rewrites a stored coordinate.
//!
//! Normalised coordinates run from `(0, 0)` at the top-left corner to `(1, 1)` at the bottom-right
//! corner of their frame, `x` across and `y` down, whatever the picture's size, so mapping needs
//! only the transform. A radius relative to the picture's shorter side, and any length relative to
//! it, is the same in both frames. An angle measured in the stored frame (`straighten`) turns the
//! other way on screen when the transform mirrors.
//!
//! The mapping is exact arithmetic on `1 − v` and swaps, so a displayed-then-stored round trip
//! gives the same value back up to the rounding of `1 − v` (at most one unit in the last place
//! of 1, about 1.1 × 10⁻¹⁶); for dyadic values such as `k / 2ⁿ` it is exact. Values outside 0..1
//! and non-finite values are mapped by the same formulas without failing: callers clamp or refuse
//! them where they read them.

use crate::Dihedral;

/// A normalised point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A normalised rectangle from its top-left corner `(x0, y0)` to its bottom-right corner
/// `(x1, y1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    /// Where this point of a picture lands after `d` is applied to the picture (the continuous
    /// counterpart of [`Dihedral::map_pixel`]).
    pub fn transform(self, d: Dihedral) -> Point {
        let x = if d.mirrored() { 1.0 - self.x } else { self.x };
        let y = self.y;
        match d.quarters() {
            1 => Point::new(1.0 - y, x),
            2 => Point::new(1.0 - x, 1.0 - y),
            3 => Point::new(y, 1.0 - x),
            _ => Point::new(x, y),
        }
    }

    /// This stored point as shown on a photo whose recipe turns it by `orientation`
    /// (`Recipe::orientation()`).
    pub fn to_display(self, orientation: Dihedral) -> Point {
        self.transform(orientation)
    }

    /// The stored point under this point of the displayed photo, whose recipe turns it by
    /// `orientation`.
    pub fn to_stored(self, orientation: Dihedral) -> Point {
        self.transform(orientation.inverse())
    }
}

impl Rect {
    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    /// The same rectangle with its corners in order (`x0 ≤ x1`, `y0 ≤ y1`).
    pub fn ordered(self) -> Rect {
        Rect::new(self.x0.min(self.x1), self.y0.min(self.y1), self.x0.max(self.x1), self.y0.max(self.y1))
    }

    /// The rectangle covering the same part of the picture after `d` is applied to it, corners in
    /// order.
    pub fn transform(self, d: Dihedral) -> Rect {
        let a = Point::new(self.x0, self.y0).transform(d);
        let b = Point::new(self.x1, self.y1).transform(d);
        Rect::new(a.x, a.y, b.x, b.y).ordered()
    }

    /// This stored rectangle as shown on a photo whose recipe turns it by `orientation`.
    pub fn to_display(self, orientation: Dihedral) -> Rect {
        self.transform(orientation)
    }

    /// The stored rectangle under this rectangle of the displayed photo, whose recipe turns it by
    /// `orientation`.
    pub fn to_stored(self, orientation: Dihedral) -> Rect {
        self.transform(orientation.inverse())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corners_go_where_the_pixels_go() {
        // The top-left corner of the stored picture, after each transform.
        let corner = Point::new(0.0, 0.0);
        let expected = [
            (Dihedral::IDENTITY, (0.0, 0.0)),
            (Dihedral::ROTATE_90, (1.0, 0.0)),
            (Dihedral::ROTATE_180, (1.0, 1.0)),
            (Dihedral::ROTATE_270, (0.0, 1.0)),
            (Dihedral::FLIP_H, (1.0, 0.0)),
            (Dihedral::FLIP_V, (0.0, 1.0)),
            (Dihedral::TRANSPOSE, (0.0, 0.0)),
            (Dihedral::TRANSVERSE, (1.0, 1.0)),
        ];
        for (d, (x, y)) in expected {
            assert_eq!(corner.to_display(d), Point::new(x, y), "{}", d.name());
        }
    }

    #[test]
    fn hostile_values_map_without_failing() {
        for d in Dihedral::ALL {
            let p = Point::new(f64::NAN, f64::INFINITY).to_display(d);
            assert!(!p.x.is_finite() || !p.y.is_finite(), "{}", d.name());
            let r = Rect::new(-5.0, 7.0, f64::MAX, f64::MIN).to_stored(d);
            assert!(r.x0 <= r.x1 && r.y0 <= r.y1, "{}", d.name());
        }
    }
}
