//! `retouch(sx,sy,tx,ty,r;…)` (`editing.md` §6.4, `data-model.md` §3.1): patches copied from a
//! source point to a target point, in the stored frame, mapped through the crop and straighten
//! ([`super::Prepare::geometry`]).
//!
//! **Stub (M7.2):** the parameters stay text and retouch renders nothing (`VERSION` 0); M7.14 types
//! and renders it, here only.

use std::fmt;

use super::{Adjust, Operation, ParamError, Pass, Prepare};

/// `retouch(…)`, its parameter text kept verbatim until M7.14.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Retouch {
    params: String,
}

impl fmt::Display for Retouch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.params)
    }
}

impl Operation for Retouch {
    const NAME: &'static str = "retouch";
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Retouch";

    fn parse(params: &str) -> Result<Retouch, ParamError> {
        Ok(Retouch { params: params.to_owned() })
    }
}

impl Adjust for Retouch {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        None
    }
}
