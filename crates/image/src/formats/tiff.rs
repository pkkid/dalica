//! TIFF: a stub until M5.6 fills it with the first full-resolution page through the pinned `image`/`tiff` decoders (8/16-bit integer and float; grey, RGB, CMYK, palette), the thumbnail IFD as the fast path and the ICC tag into the colour stage (`formats.md` §1).
//!
//! Until then every entry point answers [`Error::Unsupported`], and the file is listed with the
//! tile text [`REASON`]. The entry points and their contracts are in `formats/mod.rs`.

use std::ops::Range;

use crate::decode::Decoded;
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Result};

/// Whether this build decodes TIFF files.
pub const DECODES: bool = false;

/// The tile text for a TIFF file this build does not show.
pub const REASON: &str = "TIFF files aren't supported yet";

pub fn header(_bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(Format::Tiff))
}

pub fn decode(_bytes: &[u8], _header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(Format::Tiff))
}

pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Err(Error::Unsupported(Format::Tiff))
}
