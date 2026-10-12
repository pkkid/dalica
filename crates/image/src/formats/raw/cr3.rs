//! CR3 (Canon, ISOBMFF) raws: a stub until M5.10 fills it with the embedded preview found through the bounded box walk of `formats/bmff.rs`.
//!
//! [`recognise`] already tells the container apart by content; the other entry points answer
//! [`Error::Unsupported`] with the tile text [`REASON`] until then (contracts in `formats/mod.rs`).

use std::ops::Range;

use crate::decode::Decoded;
use crate::limits::{Budget, Limits};
use crate::sniff::RawKind;
use crate::{Error, Format, Header, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = false;

/// The tile text for a raw this build does not show.
pub const REASON: &str = "RAW previews aren't supported yet";

const FORMAT: Format = Format::Raw(RawKind::Cr3);

/// Whether `head` starts this container.
pub fn recognise(head: &[u8]) -> bool {
    // An ISOBMFF file whose major brand is `crx `.
    crate::formats::bmff::major_brand(head) == Some(*b"crx ")
}

pub fn header(_bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(FORMAT))
}

pub fn decode(_bytes: &[u8], _header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(FORMAT))
}

pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Err(Error::Unsupported(FORMAT))
}
