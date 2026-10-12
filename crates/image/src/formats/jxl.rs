//! JPEG XL: a stub until M5.8 fills it with JPEG XL through `jxl-oxide`; the decoder applies orientation, the ICC profile goes into the colour stage (`formats.md` §1).
//!
//! Until then every entry point answers [`Error::Unsupported`], and the file is listed with the
//! tile text [`REASON`]. The entry points and their contracts are in `formats/mod.rs`.

use std::ops::Range;

use crate::decode::Decoded;
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Result};

/// Whether this build decodes JPEG XL files.
pub const DECODES: bool = false;

/// The tile text for a JPEG XL file this build does not show.
pub const REASON: &str = "JPEG XL files aren't supported yet";

pub fn header(_bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(Format::JpegXl))
}

pub fn decode(_bytes: &[u8], _header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(Format::JpegXl))
}

pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Err(Error::Unsupported(Format::JpegXl))
}
