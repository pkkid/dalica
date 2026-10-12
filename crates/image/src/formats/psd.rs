//! PSD: a stub until M5.8 fills it with the merged composite of PSD and PSB through our own reader (raw and RLE; 8/16/32-bit grey, RGB, CMYK, indexed), the thumbnail resource as the fast path, the ICC resource into the colour stage; layers are never parsed (`formats.md` §1).
//!
//! Until then every entry point answers [`Error::Unsupported`], and the file is listed with the
//! tile text [`REASON`]. The entry points and their contracts are in `formats/mod.rs`.

use std::ops::Range;

use crate::decode::Decoded;
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Result};

/// Whether this build decodes PSD files.
pub const DECODES: bool = false;

/// The tile text for a PSD file this build does not show.
pub const REASON: &str = "PSD files aren't supported yet";

pub fn header(_bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(Format::Psd))
}

pub fn decode(_bytes: &[u8], _header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(Format::Psd))
}

pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Err(Error::Unsupported(Format::Psd))
}
