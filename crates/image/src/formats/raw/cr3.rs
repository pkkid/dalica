//! CR3 (Canon, ISOBMFF) raws: the largest embedded JPEG, found through the bounded box walk of
//! `formats/bmff.rs` (`formats.md` §2).
//!
//! Provenance: written from public prose descriptions of the CR3 layout (an ISO base media file
//! with major brand `crx `): `moov` holds a Canon `uuid` box (85c0b687-820f-11e0-8111-f4ce462b6a48)
//! whose children include `CMT1` (a TIFF structure with IFD0: orientation, camera), `CMT2` (a
//! TIFF structure whose IFD0 holds the EXIF tags: dates) and `THMB` (a small JPEG: width, height
//! and the JPEG's length, the JPEG 16 bytes into the payload); a top-level `uuid` box
//! (eaf42b5e-1c98-4b88-b9fb-b7dc406e4d16) holds, after 8 bytes, a `PRVW` box (the JPEG's length 12
//! bytes into its payload, the JPEG at 16); and the tracks (`trak`) whose sample description
//! carries a `JPEG` box store a full-size JPEG as their first sample (offset from `co64`/`stco`,
//! length from `stsz`). No code was consulted.
//!
//! Every JPEG found is a candidate; the largest is shown and the next smaller one is the thumbnail
//! fast path (`raf::pick`). The walk stops at `mdat`, so a header read never needs the raw data.

use std::ops::Range;

use dalica_core::DateTime;

use super::raf::{Previews, decode_main, header_of, pick, span, thumbnail_of};
use crate::decode::Decoded;
use crate::exif::{TAG_DATE_TIME_DIGITIZED, TAG_DATE_TIME_ORIGINAL};
use crate::formats::bmff::{self, BoxRef, Boxes};
use crate::formats::u32_be;
use crate::ifd::{Tiff, Visited};
use crate::limits::{Budget, Limits, Work};
use crate::sniff::RawKind;
use crate::{Error, Format, Header, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = true;

/// The tile text for a raw without a usable preview (`formats.md` §2: the generic RAW tile).
pub const REASON: &str = "This RAW file has no preview Dalica can show";

const FORMAT: Format = Format::Raw(RawKind::Cr3);

/// The `usertype` of Canon's metadata box inside `moov`.
pub const CANON_UUID: [u8; 16] = [0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48];
/// The `usertype` of the top-level box holding the `PRVW` preview.
pub const PREVIEW_UUID: [u8; 16] = [0xea, 0xf4, 0x2b, 0x5e, 0x1c, 0x98, 0x4b, 0x88, 0xb9, 0xfb, 0xb7, 0xdc, 0x40, 0x6e, 0x4d, 0x16];

/// Bytes of a sample description entry searched for its `JPEG` box.
const MAX_ENTRY_SCAN: usize = 4096;

/// Whether `head` starts this container.
pub fn recognise(head: &[u8]) -> bool {
    // An ISOBMFF file whose major brand is `crx `.
    bmff::major_brand(head) == Some(*b"crx ")
}

/// What the walk found: candidate JPEGs and the two TIFF blocks.
#[derive(Debug, Default)]
struct Found {
    candidates: Vec<Range<usize>>,
    cmt1: Option<Range<usize>>,
    cmt2: Option<Range<usize>>,
}

/// A big-endian `u32` `at` bytes into `body`, which must hold it.
fn field(bytes: &[u8], body: &Range<usize>, at: usize) -> Result<u32> {
    if at.checked_add(4).is_none_or(|end| end > body.len()) {
        return Err(Error::Malformed("a CR3 box is too short".into()));
    }
    u32_be(bytes, body.start + at)
}

/// A JPEG whose length is `len_at` bytes into `body` and whose data starts `data_at` bytes in.
fn embedded(bytes: &[u8], body: &Range<usize>, len_at: usize, data_at: usize) -> Result<Range<usize>> {
    let len = field(bytes, body, len_at)?;
    span((body.start + data_at) as u64, u64::from(len))
}

fn walk(bytes: &[u8], limits: &Limits) -> Result<Found> {
    if !recognise(bytes) {
        return Err(Error::Malformed("not a CR3 file".into()));
    }
    let mut work = Work::new(limits.max_segments, "ISOBMFF boxes");
    let mut found = Found::default();
    let (mut moov, mut preview) = (None, None);
    let mut top = Boxes::top(bytes);
    while let Some(b) = top.next(&mut work)? {
        match &b.kind {
            b"moov" => moov = Some(b),
            b"uuid" if b.uuid == Some(PREVIEW_UUID) => preview = Some(b),
            // The raw data follows: nothing the preview needs lies past it.
            b"mdat" => break,
            _ => {}
        }
        if moov.is_some() && preview.is_some() {
            break;
        }
    }
    let moov = moov.ok_or_else(|| Error::Malformed("a CR3 file without moov".into()))?;
    if let Some(uuid) = preview
        && let Some(prvw) = Boxes::children(bytes, &uuid, 8)?.find(b"PRVW", None, &mut work)?
    {
        found.candidates.push(embedded(bytes, &prvw.body, 12, 16)?);
    }
    let mut boxes = Boxes::children(bytes, &moov, 0)?;
    while let Some(b) = boxes.next(&mut work)? {
        match &b.kind {
            b"uuid" if b.uuid == Some(CANON_UUID) => canon(bytes, &b, &mut found, &mut work)?,
            b"trak" => {
                if let Some(jpeg) = track_jpeg(bytes, &b, &mut work)? {
                    found.candidates.push(jpeg);
                }
            }
            _ => {}
        }
    }
    Ok(found)
}

/// The children of Canon's metadata box: the TIFF blocks and the thumbnail.
fn canon(bytes: &[u8], uuid: &BoxRef, found: &mut Found, work: &mut Work) -> Result<()> {
    let mut boxes = Boxes::children(bytes, uuid, 0)?;
    while let Some(b) = boxes.next(work)? {
        match &b.kind {
            b"CMT1" => found.cmt1 = Some(b.body),
            b"CMT2" => found.cmt2 = Some(b.body),
            b"THMB" => found.candidates.push(embedded(bytes, &b.body, 8, 16)?),
            _ => {}
        }
    }
    Ok(())
}

/// The first sample of a track whose sample description carries a `JPEG` box.
fn track_jpeg(bytes: &[u8], trak: &BoxRef, work: &mut Work) -> Result<Option<Range<usize>>> {
    let Some(stbl) = bmff::path(bytes, trak, &[b"mdia", b"minf", b"stbl"], work)? else { return Ok(None) };
    let Some(stsd) = bmff::child(bytes, &stbl, 0, b"stsd", work)? else { return Ok(None) };
    // A full box with an entry count, then the sample entries.
    let Some(entry) = Boxes::children(bytes, &stsd, 8)?.next(work)? else { return Ok(None) };
    if !has_jpeg_box(bytes, &entry.body) {
        return Ok(None);
    }
    let offset = if let Some(co64) = bmff::child(bytes, &stbl, 0, b"co64", work)? {
        if field(bytes, &co64.body, 4)? == 0 {
            return Ok(None);
        }
        (u64::from(field(bytes, &co64.body, 8)?) << 32) | u64::from(field(bytes, &co64.body, 12)?)
    } else if let Some(stco) = bmff::child(bytes, &stbl, 0, b"stco", work)? {
        if field(bytes, &stco.body, 4)? == 0 {
            return Ok(None);
        }
        u64::from(field(bytes, &stco.body, 8)?)
    } else {
        return Ok(None);
    };
    let Some(stsz) = bmff::child(bytes, &stbl, 0, b"stsz", work)? else { return Ok(None) };
    let size = match field(bytes, &stsz.body, 4)? {
        0 if field(bytes, &stsz.body, 8)? == 0 => return Ok(None),
        0 => field(bytes, &stsz.body, 12)?,
        n => n,
    };
    span(offset, u64::from(size)).map(Some)
}

/// Whether a sample entry's payload holds a `JPEG` box: a box header (size, then `JPEG`) whose
/// size fits in what is left. Searched rather than walked, so the entry's fixed fields need not
/// be known; bounded by [`MAX_ENTRY_SCAN`].
fn has_jpeg_box(bytes: &[u8], body: &Range<usize>) -> bool {
    let end = body.end.min(body.start.saturating_add(MAX_ENTRY_SCAN));
    let Some(data) = bytes.get(body.start..end) else { return false };
    data.windows(8).enumerate().any(|(i, w)| match *w {
        [a, b, c, d, b'J', b'P', b'E', b'G'] => (8..=body.len() - i).contains(&(u32::from_be_bytes([a, b, c, d]) as usize)),
        _ => false,
    })
}

/// DateTimeOriginal and DateTimeDigitized from `CMT2`, a TIFF structure whose IFD0 holds the EXIF
/// tags. A damaged block leaves the dates unset.
fn dates(block: &[u8], limits: &Limits) -> (Option<DateTime>, Option<DateTime>) {
    let Ok(tiff) = Tiff::new(block) else { return (None, None) };
    let Ok(ifd) = tiff.first_ifd().and_then(|at| tiff.ifd(at, &mut Visited::default(), limits)) else { return (None, None) };
    let date = |tag| ifd.get(tag).and_then(|e| tiff.ascii(e, 64)).and_then(|s| s.trim().parse::<DateTime>().ok());
    (date(TAG_DATE_TIME_ORIGINAL), date(TAG_DATE_TIME_DIGITIZED))
}

fn previews(bytes: &[u8], found: &Found, limits: &Limits) -> Result<Previews> {
    pick(bytes, &found.candidates, limits)?.ok_or(Error::Unsupported(FORMAT))
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let found = walk(bytes, limits)?;
    let p = previews(bytes, &found, limits)?;
    let exif = found.cmt1.as_ref().and_then(|r| bytes.get(r.clone()).map(|b| (b, r.start)));
    let mut h = header_of(FORMAT, &p, exif, limits);
    if let (Some(e), Some(block)) = (h.exif.as_mut(), found.cmt2.as_ref().and_then(|r| bytes.get(r.clone()))) {
        let (original, digitized) = dates(block, limits);
        e.date_time_original = e.date_time_original.take().or(original);
        e.date_time_digitized = e.date_time_digitized.take().or(digitized);
    }
    Ok(h)
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let found = walk(bytes, limits)?;
    decode_main(bytes, header, &previews(bytes, &found, limits)?, budget, limits)
}

pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    thumbnail_of(header(bytes, limits))
}
