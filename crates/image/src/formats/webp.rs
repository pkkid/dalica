//! WebP: a stub until M5.7 fills it with lossy and lossless WebP with alpha through `image-webp`, the first frame of animations, and the `ICCP` chunk into the colour stage (`formats.md` §1).
//!
//! Until then every entry point answers [`Error::Unsupported`], and the file is listed with the
//! tile text [`REASON`]. The entry points and their contracts are in `formats/mod.rs`.

use std::ops::Range;

use crate::decode::Decoded;
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Result};

/// Whether this build decodes WebP files.
pub const DECODES: bool = false;

/// The tile text for a WebP file this build does not show.
pub const REASON: &str = "WebP files aren't supported yet";

pub fn header(_bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(Format::WebP))
}

pub fn decode(_bytes: &[u8], _header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(Format::WebP))
}

pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Err(Error::Unsupported(Format::WebP))
}
