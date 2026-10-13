//! `straighten(deg)` (`editing.md` §6.2, `data-model.md` §3.1): a turn of −10…+10° about the
//! centre in the stored frame, scaled up so no empty corner shows. The renderer straightens before
//! it crops, whatever their order in the recipe.
//!
//! **Stub (M7.2):** the parameters stay text and the straighten renders nothing (`VERSION` 0); M7.7
//! types and renders it, here only.

use std::fmt;

use super::{Affine, Operation, ParamError, Warp};

/// `straighten(…)`, its parameter text kept verbatim until M7.7.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Straighten {
    params: String,
}

impl fmt::Display for Straighten {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.params)
    }
}

impl Operation for Straighten {
    const NAME: &'static str = "straighten";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Straighten";

    fn parse(params: &str) -> Result<Straighten, ParamError> {
        Ok(Straighten { params: params.to_owned() })
    }
}

impl Warp for Straighten {
    fn output_size(&self, width: f64, height: f64) -> (f64, f64) {
        (width, height)
    }

    fn to_input(&self, _width: f64, _height: f64) -> Affine {
        Affine::IDENTITY
    }
}

/// The straighten that counts among the recipe's straightens, in recipe order: the last one.
pub fn combine(straightens: &[Straighten]) -> Option<Straighten> {
    straightens.last().cloned()
}
