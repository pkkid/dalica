//! Normalised geometry (`data-model.md` §3): resolution-independent rectangles in 0..1, stored as
//! `[x0, y0, x1, y1]` with at most six decimals. Which frame they are measured in (before or after
//! Dalica's own rotations) is still open (`data-model.md` §3.3); this module does not care.

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Decimals kept in every stored coordinate.
pub const DECIMALS: i32 = 6;

const SCALE: f64 = 1_000_000.0;

/// `v` rounded to [`DECIMALS`] decimals, with `-0.0` turned into `0.0`.
pub fn round6(v: f64) -> f64 {
    (v * SCALE).round() / SCALE + 0.0
}

/// A rectangle in normalised coordinates: `0 <= x0 < x1 <= 1` and `0 <= y0 < y1 <= 1`, every value
/// rounded to six decimals. It is never empty and never outside the image.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 4]", into = "[f64; 4]")]
pub struct Rect01 {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

/// A rectangle in whole pixels of an image: left, top, width, height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect01 {
    /// The whole image.
    pub const FULL: Rect01 = Rect01 { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 };

    /// Rounds each value to six decimals, then checks the invariants.
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Rect01, Error> {
        let [x0, y0, x1, y1] = [x0, y0, x1, y1].map(round6);
        let in_range = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
        if !(in_range(x0) && in_range(y0) && in_range(x1) && in_range(y1)) {
            return Err(Error::InvalidRect(format!("[{x0}, {y0}, {x1}, {y1}] is outside 0..1")));
        }
        if x0 >= x1 || y0 >= y1 {
            return Err(Error::InvalidRect(format!("[{x0}, {y0}, {x1}, {y1}] is empty or inverted")));
        }
        Ok(Rect01 { x0, y0, x1, y1 })
    }

    pub fn x0(&self) -> f64 {
        self.x0
    }

    pub fn y0(&self) -> f64 {
        self.y0
    }

    pub fn x1(&self) -> f64 {
        self.x1
    }

    pub fn y1(&self) -> f64 {
        self.y1
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// The centre point `(x, y)`.
    pub fn center(&self) -> (f64, f64) {
        ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    /// Whether the point lies inside, edges included.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        (self.x0..=self.x1).contains(&x) && (self.y0..=self.y1).contains(&y)
    }

    /// The pixels this rectangle covers in a `width × height` image: edges rounded to the nearest
    /// pixel, at least one pixel wide and tall, always inside the image. `None` for an empty image.
    pub fn to_pixels(&self, width: u32, height: u32) -> Option<PixelRect> {
        let (x, w) = span(self.x0, self.x1, width)?;
        let (y, h) = span(self.y0, self.y1, height)?;
        Some(PixelRect { x, y, width: w, height: h })
    }

    /// The normalised form of a pixel rectangle that lies inside a `width × height` image.
    pub fn from_pixels(px: PixelRect, width: u32, height: u32) -> Result<Rect01, Error> {
        let right = px.x.checked_add(px.width);
        let bottom = px.y.checked_add(px.height);
        let fits = matches!((right, bottom), (Some(r), Some(b)) if r <= width && b <= height);
        if width == 0 || height == 0 || px.width == 0 || px.height == 0 || !fits {
            return Err(Error::InvalidPixels(format!("{px:?} in a {width} × {height} image")));
        }
        let (w, h) = (f64::from(width), f64::from(height));
        let (r, b) = (f64::from(px.x) + f64::from(px.width), f64::from(px.y) + f64::from(px.height));
        Rect01::new(f64::from(px.x) / w, f64::from(px.y) / h, r / w, b / h)
    }
}

/// Start and length in pixels of the normalised span `a..b` on an axis of `size` pixels.
fn span(a: f64, b: f64, size: u32) -> Option<(u32, u32)> {
    if size == 0 {
        return None;
    }
    let s = f64::from(size);
    // Every value is in 0..=size after clamping, so the casts cannot truncate.
    let start = (a * s).round().clamp(0.0, s - 1.0) as u32;
    let end = (b * s).round().clamp(0.0, s) as u32;
    if end > start {
        return Some((start, end - start));
    }
    // Narrower than a pixel: the pixel that holds its centre.
    let center = ((a + b) / 2.0 * s).floor().clamp(0.0, s - 1.0) as u32;
    Some((center, 1))
}

impl TryFrom<[f64; 4]> for Rect01 {
    type Error = Error;

    fn try_from([x0, y0, x1, y1]: [f64; 4]) -> Result<Rect01, Error> {
        Rect01::new(x0, y0, x1, y1)
    }
}

impl From<Rect01> for [f64; 4] {
    fn from(r: Rect01) -> [f64; 4] {
        [r.x0, r.y0, r.x1, r.y1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rounds_to_six_decimals() {
        let r = Rect01::new(0.1234564, 0.2, 0.9999996, 0.8).unwrap();
        assert_eq!(<[f64; 4]>::from(r), [0.123456, 0.2, 1.0, 0.8]);
        assert_eq!(serde_json::to_string(&r).unwrap(), "[0.123456,0.2,1.0,0.8]");
    }

    #[test]
    fn new_refuses_bad_rectangles() {
        for [a, b, c, d] in [
            [0.5, 0.0, 0.5, 1.0],
            [0.6, 0.0, 0.5, 1.0],
            [-0.1, 0.0, 0.5, 1.0],
            [0.0, 0.0, 1.1, 1.0],
            [f64::NAN, 0.0, 1.0, 1.0],
            [0.0, 0.0, f64::INFINITY, 1.0],
            [0.1, 0.1, 0.1000001, 0.2],
        ] {
            assert!(Rect01::new(a, b, c, d).is_err(), "{:?}", [a, b, c, d]);
        }
        assert!(serde_json::from_str::<Rect01>("[0,0,1]").is_err());
        assert!(serde_json::from_str::<Rect01>("[1,1,0,0]").is_err());
        assert!(serde_json::from_str::<Rect01>("{\"x0\":0}").is_err());
    }

    #[test]
    fn negative_zero_is_stored_as_zero() {
        let r = Rect01::new(-0.0000001, 0.0, 1.0, 1.0).unwrap();
        assert!(r.x0().is_sign_positive());
        assert_eq!(serde_json::to_string(&r).unwrap(), "[0.0,0.0,1.0,1.0]");
    }

    #[test]
    fn pixels_of_the_full_rect_are_the_image() {
        assert_eq!(Rect01::FULL.to_pixels(640, 480), Some(PixelRect { x: 0, y: 0, width: 640, height: 480 }));
        assert_eq!(Rect01::FULL.to_pixels(0, 480), None);
        let tiny = Rect01::new(0.999999, 0.0, 1.0, 0.000001).unwrap();
        assert_eq!(tiny.to_pixels(10, 10), Some(PixelRect { x: 9, y: 0, width: 1, height: 1 }));
        // A sliver at 10.67 px of 44 lies in pixel 10; rounding its left edge would give pixel 11.
        let sliver = Rect01::new(0.24261, 0.0, 0.242612, 1.0).unwrap();
        assert_eq!(sliver.to_pixels(44, 1), Some(PixelRect { x: 10, y: 0, width: 1, height: 1 }));
    }

    #[test]
    fn from_pixels_refuses_rects_outside_the_image() {
        let px = |x, y, width, height| PixelRect { x, y, width, height };
        assert!(Rect01::from_pixels(px(0, 0, 0, 1), 10, 10).is_err());
        assert!(Rect01::from_pixels(px(5, 0, 6, 1), 10, 10).is_err());
        assert!(Rect01::from_pixels(px(u32::MAX, 0, 2, 1), 10, 10).is_err());
        assert!(Rect01::from_pixels(px(0, 0, 1, 1), 0, 10).is_err());
        assert_eq!(Rect01::from_pixels(px(0, 0, 10, 10), 10, 10).unwrap(), Rect01::FULL);
    }

    #[test]
    fn center_and_contains() {
        let r = Rect01::new(0.2, 0.4, 0.6, 0.8).unwrap();
        let (cx, cy) = r.center();
        assert!((cx - 0.4).abs() < 1e-12 && (cy - 0.6).abs() < 1e-12);
        assert!(r.contains(0.2, 0.8) && !r.contains(0.1, 0.5));
    }
}
