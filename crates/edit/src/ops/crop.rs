//! `crop(x0,y0,x1,y1)` (`editing.md` §6.1, `data-model.md` §3.1): a normalised rectangle of the
//! straightened picture in the stored frame (`data-model.md` §3.3); only the last crop counts.
//!
//! - **Parameters:** four finite numbers, the corners in order. Values outside 0..1 are clamped into
//!   the picture; a rectangle that is reversed or empty once clamped, a value that is not a finite
//!   number, or any other count is refused (the entry is kept and skipped).
//! - **Rendering:** the rectangle of the straightened picture, at least one pixel wide and high at
//!   any size (a thinner rectangle is widened about its centre, inside the picture). A crop on whole
//!   pixels at full size copies them exactly.

use std::fmt;

use super::{Affine, Operation, ParamError, Warp};
use crate::Rect;

/// `crop(x0,y0,x1,y1)`: the part of the picture to keep, normalised, corners in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Crop {
    pub rect: Rect,
}

impl Default for Crop {
    /// The whole picture.
    fn default() -> Crop {
        Crop { rect: Rect::new(0.0, 0.0, 1.0, 1.0) }
    }
}

impl Crop {
    /// The crop of `rect`, clamped into the picture; `None` when it is not finite, reversed or
    /// empty.
    pub fn new(rect: Rect) -> Option<Crop> {
        let r = Crop { rect }.clamped().rect;
        let finite = [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite());
        (finite && r.x0 < r.x1 && r.y0 < r.y1).then_some(Crop { rect: r })
    }

    /// The rectangle in pixels of a `width × height` picture, `(x0, y0, x1, y1)`, at least one
    /// pixel (or the whole picture, if smaller) along each side.
    pub fn pixels(&self, width: f64, height: f64) -> (f64, f64, f64, f64) {
        let side = |a: f64, b: f64, len: f64| -> (f64, f64) {
            let (mut p0, mut p1) = (a * len, b * len);
            let min = len.min(1.0);
            if p1 - p0 < min {
                let c = (p0 + p1) / 2.0;
                p0 = (c - min / 2.0).clamp(0.0, (len - min).max(0.0));
                p1 = p0 + min;
            }
            (p0, p1)
        };
        let (x0, x1) = side(self.rect.x0, self.rect.x1, width);
        let (y0, y1) = side(self.rect.y0, self.rect.y1, height);
        (x0, y0, x1, y1)
    }
}

/// A number as the shortest text that reads back to it (`-0` as `0`).
pub(crate) fn number(v: f64) -> String {
    if v == 0.0 { "0".to_owned() } else { v.to_string() }
}

/// A finite number written plainly (digits, sign, point, exponent; no spaces, `inf` or `NaN`).
pub(crate) fn read_number(text: &str) -> Option<f64> {
    let plain = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'));
    let v: f64 = text.parse().ok()?;
    (plain && v.is_finite()).then_some(v)
}

impl fmt::Display for Crop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let r = self.rect;
        write!(f, "{},{},{},{}", number(r.x0), number(r.y0), number(r.x1), number(r.y1))
    }
}

impl Operation for Crop {
    const NAME: &'static str = "crop";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Crop";

    fn parse(params: &str) -> Result<Crop, ParamError> {
        let refuse = |why: &str| ParamError::new(Self::NAME, params, why);
        let values: Vec<f64> = params.split(',').map(read_number).collect::<Option<_>>().ok_or_else(|| refuse("takes four finite numbers"))?;
        let [x0, y0, x1, y1] = values[..] else {
            return Err(refuse("takes four finite numbers"));
        };
        Crop::new(Rect::new(x0, y0, x1, y1)).ok_or_else(|| refuse("the corners must be in order and the rectangle not empty"))
    }

    /// Each coordinate brought into 0..1 (not-a-number stays, and [`Crop::new`] refuses it).
    fn clamped(self) -> Crop {
        let c = |v: f64| v.clamp(0.0, 1.0);
        let r = self.rect;
        Crop { rect: Rect::new(c(r.x0), c(r.y0), c(r.x1), c(r.y1)) }
    }
}

impl Warp for Crop {
    fn output_size(&self, width: f64, height: f64) -> (f64, f64) {
        let (x0, y0, x1, y1) = self.pixels(width, height);
        (x1 - x0, y1 - y0)
    }

    fn to_input(&self, width: f64, height: f64) -> Affine {
        let (x0, y0, _, _) = self.pixels(width, height);
        Affine::translate(x0, y0)
    }
}

/// The crop that counts among the recipe's crops, in recipe order: the last one.
pub fn combine(crops: &[Crop]) -> Option<Crop> {
    crops.last().cloned()
}
