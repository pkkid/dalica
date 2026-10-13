//! `straighten(deg)` (`editing.md` §6.2, `data-model.md` §3.1): a turn of −10…+10° about the
//! centre in the stored frame, scaled up so no empty corner shows. The renderer straightens before
//! it crops, whatever their order in the recipe.
//!
//! - **Parameters:** one finite number of degrees, positive turning the picture clockwise on screen
//!   (in the stored frame; it turns the other way when the recipe mirrors the photo,
//!   `data-model.md` §3.3). Values outside −10…+10 are clamped to the range; a value that is not a
//!   finite number, or any other count, is refused (the entry is kept and skipped).
//! - **Rendering:** the picture turned about its centre and enlarged by
//!   [`crate::crop_geom::fill_scale`], the least that covers the picture's own frame, so the output
//!   keeps the input's size and shows no empty corner. 0° is the identity.

use std::fmt;

use super::crop::{number, read_number};
use super::{Affine, Operation, ParamError, Warp};
use crate::crop_geom::fill_scale;

/// The largest turn, in degrees, either way.
pub const MAX_DEGREES: f64 = 10.0;

/// `straighten(deg)`: the turn in degrees, clockwise positive, within ±[`MAX_DEGREES`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Straighten {
    pub degrees: f64,
}

impl Straighten {
    /// The straighten of `degrees`, clamped into the range; `None` when it is not finite.
    pub fn new(degrees: f64) -> Option<Straighten> {
        degrees.is_finite().then(|| Straighten { degrees }.clamped())
    }
}

impl fmt::Display for Straighten {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&number(self.degrees))
    }
}

impl Operation for Straighten {
    const NAME: &'static str = "straighten";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Straighten";

    fn parse(params: &str) -> Result<Straighten, ParamError> {
        read_number(params).and_then(Straighten::new).ok_or_else(|| ParamError::new(Self::NAME, params, "takes one finite number of degrees"))
    }

    fn clamped(self) -> Straighten {
        // `-0` is `0`.
        Straighten { degrees: self.degrees.clamp(-MAX_DEGREES, MAX_DEGREES) + 0.0 }
    }
}

impl Warp for Straighten {
    fn output_size(&self, width: f64, height: f64) -> (f64, f64) {
        (width, height)
    }

    fn to_input(&self, width: f64, height: f64) -> Affine {
        if self.degrees == 0.0 || !self.degrees.is_finite() {
            return Affine::IDENTITY;
        }
        // Output → input: about the centre, shrink by the fill scale, then turn back.
        let k = fill_scale(width, height, self.degrees);
        let (cx, cy) = (width / 2.0, height / 2.0);
        Affine::rotate_about(-self.degrees.to_radians(), cx, cy)
            .then_after(Affine::translate(cx, cy))
            .then_after(Affine::scale(1.0 / k, 1.0 / k))
            .then_after(Affine::translate(-cx, -cy))
    }
}

/// The straighten that counts among the recipe's straightens, in recipe order: the last one.
pub fn combine(straightens: &[Straighten]) -> Option<Straighten> {
    straightens.last().cloned()
}
