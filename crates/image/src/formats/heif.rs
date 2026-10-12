//! HEIF / HEIC: a stub until M5.10 fills it with HEIF/HEIC through `heic-rs` behind the non-default `heif` feature, in a panic-guarded wrapper (`formats.md` §3). Without the feature the tile says so.
//!
//! Until then every entry point answers [`Error::Unsupported`], and the file is listed with the
//! tile text [`REASON`]. The entry points and their contracts are in `formats/mod.rs`.

use std::ops::Range;

use crate::decode::Decoded;
use crate::limits::{Budget, Limits};
use crate::{Error, Format, Header, Result};

/// Whether this build decodes HEIF / HEIC files.
pub const DECODES: bool = false;

/// The tile text for a HEIF / HEIC file this build does not show.
pub const REASON: &str = "HEIC support isn't included in this build";

pub fn header(_bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(Format::Heif))
}

pub fn decode(_bytes: &[u8], _header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(Format::Heif))
}

pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Err(Error::Unsupported(Format::Heif))
}
