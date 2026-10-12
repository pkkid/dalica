//! X3F (Sigma) raws: the embedded JPEG previews listed in the file's directory (`formats.md` §2).
//!
//! Provenance: written from public prose descriptions of the X3F container: the signature
//! `FOVb`, little-endian numbers, and a directory found through the file's last four bytes (its
//! offset): `SECd`, a version, an entry count, then 12-byte entries (offset, length, a 4-byte
//! section type). Image sections (`IMAG`, `IMA2`) start with `SECi`, a version, the image type,
//! the data format (18: JPEG), columns, rows and row stride (28 bytes), then the data. No code
//! was consulted.
//!
//! The directory is found from the end of the file, so a partial read cannot know where it is:
//! unless its offset points at `SECd` the bytes are taken as [`Error::Truncated`] and the caller
//! reads on.

use std::ops::Range;

use super::raf::{MAX_PREVIEWS, Previews, decode_main, header_of, pick, span, thumbnail_of};
use crate::decode::Decoded;
use crate::limits::{Budget, Limits, Work};
use crate::sniff::RawKind;
use crate::{Error, Format, Header, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = true;

/// The tile text for a raw without a usable preview (`formats.md` §2: the generic RAW tile).
pub const REASON: &str = "This RAW file has no preview Dalica can show";

const FORMAT: Format = Format::Raw(RawKind::X3f);

/// Most directory entries read.
pub const MAX_ENTRIES: usize = 4096;

/// The image section header before the data.
const SECTION_HEADER: usize = 28;
/// The data format of a JPEG image section.
const FORMAT_JPEG: u32 = 18;

/// Whether `head` starts this container.
pub fn recognise(head: &[u8]) -> bool {
    head.starts_with(b"FOVb")
}

fn u32_le(b: &[u8], at: usize) -> Result<u32> {
    match b.get(at..at.saturating_add(4)) {
        Some(&[a, b2, c, d]) => Ok(u32::from_le_bytes([a, b2, c, d])),
        _ => Err(Error::Truncated),
    }
}

fn candidates(bytes: &[u8], limits: &Limits) -> Result<Vec<Range<usize>>> {
    if !recognise(bytes) {
        return Err(Error::Malformed("not an X3F file".into()));
    }
    let tail = bytes.len().checked_sub(4).filter(|&t| t >= 4).ok_or(Error::Truncated)?;
    let dir = u32_le(bytes, tail)? as usize;
    if dir >= tail || bytes.get(dir..dir + 4) != Some(b"SECd") {
        return Err(Error::Truncated);
    }
    let count = u32_le(bytes, dir + 8)? as usize;
    if count > MAX_ENTRIES {
        return Err(Error::TooMany("X3F directory entries"));
    }
    let mut work = Work::new(limits.max_segments, "X3F directory entries");
    let mut out = Vec::new();
    for i in 0..count {
        work.step()?;
        let at = dir + 12 + 12 * i;
        let section = span(u64::from(u32_le(bytes, at)?), u64::from(u32_le(bytes, at + 4)?))?;
        if !matches!(bytes.get(at + 8..at + 12), Some(b"IMAG" | b"IMA2")) {
            continue;
        }
        if section.len() < SECTION_HEADER {
            return Err(Error::Malformed("an X3F image section shorter than its header".into()));
        }
        if bytes.get(section.start..section.start + 4).ok_or(Error::Truncated)? != b"SECi" {
            return Err(Error::Malformed("an X3F image section without SECi".into()));
        }
        if u32_le(bytes, section.start + 12)? == FORMAT_JPEG && out.len() < MAX_PREVIEWS {
            out.push(section.start + SECTION_HEADER..section.end);
        }
    }
    Ok(out)
}

fn previews(bytes: &[u8], limits: &Limits) -> Result<Previews> {
    pick(bytes, &candidates(bytes, limits)?, limits)?.ok_or(Error::Unsupported(FORMAT))
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    Ok(header_of(FORMAT, &previews(bytes, limits)?, None, limits))
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    decode_main(bytes, header, &previews(bytes, limits)?, budget, limits)
}

pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    thumbnail_of(header(bytes, limits))
}
