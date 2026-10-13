//! `crop(x0,y0,x1,y1)` (`editing.md` §6.1, `data-model.md` §3.1): a normalised rectangle of the
//! straightened picture in the stored frame (`data-model.md` §3.3); only the last crop counts.
//!
//! **Stub (M7.2):** the parameters stay text and the crop renders nothing (`VERSION` 0); M7.7 types
//! and renders it, here only.

use std::fmt;

use super::{Affine, Operation, ParamError, Warp};

/// `crop(…)`, its parameter text kept verbatim until M7.7.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Crop {
    params: String,
}

impl fmt::Display for Crop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.params)
    }
}

impl Operation for Crop {
    const NAME: &'static str = "crop";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Crop";

    fn parse(params: &str) -> Result<Crop, ParamError> {
        Ok(Crop { params: params.to_owned() })
    }
}

impl Warp for Crop {
    fn output_size(&self, width: f64, height: f64) -> (f64, f64) {
        (width, height)
    }

    fn to_input(&self, _width: f64, _height: f64) -> Affine {
        Affine::IDENTITY
    }
}

/// The crop that counts among the recipe's crops, in recipe order: the last one.
pub fn combine(crops: &[Crop]) -> Option<Crop> {
    crops.last().cloned()
}
