//! MRW (Minolta) raws: the JPEG previews found through the file's TIFF block, read with
//! [`crate::ifd`] (`formats.md` §2).
//!
//! Provenance: written from public prose descriptions of the MRW container: the signature
//! `\0MRM`, a big-endian 32-bit length of the header blocks that follow (the raw data starts after
//! them), then blocks of a 4-byte name (`\0PRD`, `\0TTW`, `\0WBG`, `\0RIF`, `\0PAD`), a 32-bit length
//! and the data. `\0TTW` holds a TIFF structure (offsets relative to its first byte) with IFD0
//! (orientation, camera), the EXIF IFD, the camera's maker note (a plain IFD whose tags 0x0088
//! and 0x0089 give the preview JPEG's offset and length, and whose tag 0x0081 holds a JPEG
//! thumbnail inline) and possibly an IFD1 JPEG thumbnail. No code was consulted.
//!
//! The TIFF block is read through `ifd` unchanged: its loop detection, IFD and entry caps apply.
//! A damaged maker note only loses its previews; the largest JPEG found is shown.

use std::ops::Range;

use super::raf::{Previews, decode_main, header_of, pick, span, thumbnail_of};
use crate::decode::Decoded;
use crate::exif::TAG_EXIF_IFD;
use crate::formats::u32_be;
use crate::ifd::{Ifd, Tiff, Visited};
use crate::limits::{Budget, Limits, Work};
use crate::sniff::RawKind;
use crate::{Error, Format, Header, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = true;

/// The tile text for a raw without a usable preview (`formats.md` §2: the generic RAW tile).
pub const REASON: &str = "This RAW file has no preview Dalica can show";

const FORMAT: Format = Format::Raw(RawKind::Mrw);

const TAG_MAKER_NOTE: u16 = 0x927c;
const TAG_PREVIEW_START: u16 = 0x0088;
const TAG_PREVIEW_LENGTH: u16 = 0x0089;
const TAG_THUMBNAIL_IMAGE: u16 = 0x0081;

/// Whether `head` starts this container.
pub fn recognise(head: &[u8]) -> bool {
    head.starts_with(b"\0MRM")
}

/// The TIFF block (`\0TTW`) and the candidate JPEGs, as ranges of the file.
#[derive(Debug, Default)]
struct Found {
    ttw: Option<Range<usize>>,
    candidates: Vec<Range<usize>>,
}

fn walk(bytes: &[u8], limits: &Limits) -> Result<Found> {
    if !recognise(bytes) {
        return Err(Error::Malformed("not an MRW file".into()));
    }
    let end = span(8, u64::from(u32_be(bytes, 4)?))?.end;
    if end > bytes.len() {
        return Err(Error::Truncated);
    }
    let mut found = Found::default();
    let mut work = Work::new(limits.max_segments, "MRW blocks");
    let mut pos = 8;
    while pos < end {
        work.step()?;
        let name = bytes.get(pos..pos + 4).ok_or(Error::Truncated)?;
        let data = span(pos as u64 + 8, u64::from(u32_be(bytes, pos + 4)?))?;
        if data.end > end {
            return Err(Error::Malformed("an MRW block runs past the header".into()));
        }
        if name == b"\0TTW" && found.ttw.is_none() {
            found.ttw = Some(data.clone());
        }
        pos = data.end;
    }
    // Every MRW file has its TIFF block; one without is damaged.
    let ttw = found.ttw.clone().ok_or_else(|| Error::Malformed("an MRW file without its TIFF block".into()))?;
    let block = bytes.get(ttw.clone()).ok_or(Error::Truncated)?;
    for r in tiff_jpegs(block, limits) {
        found.candidates.push(r.start.saturating_add(ttw.start)..r.end.saturating_add(ttw.start));
    }
    Ok(found)
}

/// The JPEGs a TIFF block points at, as ranges of the block: the EXIF IFD1 thumbnail and the
/// maker note's preview and thumbnail. Unreadable parts are skipped.
fn tiff_jpegs(block: &[u8], limits: &Limits) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    if let Some(thumb) = crate::exif::read(block, limits).ok().and_then(|e| e.thumbnail) {
        out.push(thumb);
    }
    let _ = maker_note_jpegs(block, limits, &mut out);
    out
}

/// The offset of entry `tag`'s value in `ifd` (its 4-byte value field read as an offset).
fn value_offset(tiff: &Tiff, ifd: &Ifd, tag: u16) -> Option<u32> {
    let index = ifd.entries.iter().position(|e| e.tag == tag)?;
    tiff.u32_at(u64::from(ifd.offset) + 2 + 12 * index as u64 + 8).ok()
}

fn maker_note_jpegs(block: &[u8], limits: &Limits, out: &mut Vec<Range<usize>>) -> Result<()> {
    let tiff = Tiff::new(block)?;
    let mut visited = Visited::default();
    let ifd0 = tiff.ifd(tiff.first_ifd()?, &mut visited, limits)?;
    let exif_at = ifd0.get(TAG_EXIF_IFD).and_then(|e| tiff.uint(e)).ok_or(Error::OutOfRange)?;
    let exif = tiff.ifd(exif_at, &mut visited, limits)?;
    if limits.max_ifd_depth < 3 {
        return Err(Error::TooMany("IFD depth"));
    }
    let note_at = value_offset(&tiff, &exif, TAG_MAKER_NOTE).ok_or(Error::OutOfRange)?;
    let note = tiff.ifd(note_at, &mut visited, limits)?;
    let start = note.get(TAG_PREVIEW_START).and_then(|e| tiff.uint(e));
    let length = note.get(TAG_PREVIEW_LENGTH).and_then(|e| tiff.uint(e));
    if let (Some(start), Some(length)) = (start, length) {
        out.push(span(u64::from(start), u64::from(length))?);
    }
    if let Some(e) = note.get(TAG_THUMBNAIL_IMAGE).filter(|e| e.count > 4)
        && let Some(at) = value_offset(&tiff, &note, TAG_THUMBNAIL_IMAGE)
    {
        out.push(span(u64::from(at), u64::from(e.count))?);
    }
    Ok(())
}

fn previews(bytes: &[u8], found: &Found, limits: &Limits) -> Result<Previews> {
    pick(bytes, &found.candidates, limits)?.ok_or(Error::Unsupported(FORMAT))
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let found = walk(bytes, limits)?;
    let p = previews(bytes, &found, limits)?;
    let exif = found.ttw.as_ref().and_then(|r| bytes.get(r.clone()).map(|b| (b, r.start)));
    Ok(header_of(FORMAT, &p, exif, limits))
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let found = walk(bytes, limits)?;
    decode_main(bytes, header, &previews(bytes, &found, limits)?, budget, limits)
}

pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    thumbnail_of(header(bytes, limits))
}
