//! CRW (Canon CIFF) raws: a stub until M5.10 fills it with the JPEG preview found through the CIFF heap.
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

const FORMAT: Format = Format::Raw(RawKind::Crw);

/// Whether `head` starts this container.
pub fn recognise(head: &[u8]) -> bool {
    // Byte order "II" (or "MM"), a header length, then the "HEAPCCDR" signature.
    (head.starts_with(b"II") || head.starts_with(b"MM")) && head.get(6..14) == Some(b"HEAPCCDR")
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
