//! The one-click fixes (`editing.md` §6.3): `autofix()`, `autocontrast()` and `autocolor()`. They
//! store no parameters and analyse the picture at their place in the pipeline at the fixed
//! analysis size ([`crate::analysis`]), so a thumbnail and the full size agree and cropping first
//! changes the outcome.
//!
//! **Stub (M7.2):** typed (no parameters) but they render nothing (`VERSION` 0); M7.13 renders
//! them, here and in `analysis.rs` only.

use std::fmt;

use super::{Adjust, Operation, ParamError, Pass, Prepare};

/// `autofix()`: auto contrast, auto colour and a little fill light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoFix;

/// `autocontrast()`: black and white points and midtones, linked across the channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoContrast;

/// `autocolor()`: removes a colour cast.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoColor;

fn no_params<T: Operation>(params: &str, value: T) -> Result<T, ParamError> {
    if params.is_empty() { Ok(value) } else { Err(ParamError::new(T::NAME, params, "takes no parameters")) }
}

macro_rules! one_click {
    ($t:ident, $name:literal, $label:literal) => {
        impl fmt::Display for $t {
            fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
                Ok(())
            }
        }

        impl Operation for $t {
            const NAME: &'static str = $name;
            const VERSION: u32 = 0;
            const LABEL: &'static str = $label;

            fn parse(params: &str) -> Result<$t, ParamError> {
                no_params(params, $t)
            }
        }

        impl Adjust for $t {
            const ANALYSES: bool = true;

            fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
                None
            }
        }
    };
}

one_click!(AutoFix, "autofix", "Auto Fix");
one_click!(AutoContrast, "autocontrast", "Auto Contrast");
one_click!(AutoColor, "autocolor", "Auto Color");
