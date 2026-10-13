//! The tone and colour stage: `tuning(fill,highlights,shadows,temp,neutral)` (`editing.md` §7) and
//! `fill_light(a)` (§6.5; the same Fill Light as Tuning's `fill`).
//!
//! **Stub (M7.2):** the parameters stay text and both render nothing (`VERSION` 0); M7.8 types and
//! renders them, here only.

use std::fmt;

use super::{Adjust, Operation, ParamError, Pass, Prepare};

/// `tuning(…)`, its parameter text kept verbatim until M7.8.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tuning {
    params: String,
}

/// `fill_light(…)`, its parameter text kept verbatim until M7.8.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FillLight {
    params: String,
}

impl fmt::Display for Tuning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.params)
    }
}

impl fmt::Display for FillLight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.params)
    }
}

impl Operation for Tuning {
    const NAME: &'static str = "tuning";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Tuning";

    fn parse(params: &str) -> Result<Tuning, ParamError> {
        Ok(Tuning { params: params.to_owned() })
    }
}

impl Operation for FillLight {
    const NAME: &'static str = "fill_light";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Fill Light";

    fn parse(params: &str) -> Result<FillLight, ParamError> {
        Ok(FillLight { params: params.to_owned() })
    }
}

impl Adjust for Tuning {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}

impl Adjust for FillLight {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
