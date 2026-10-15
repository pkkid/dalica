//! JPEG write-back (`xmp-interop.md` §3–§4): only the APP1 EXIF segment, the APP1 standard-XMP
//! segment and the APP13 Photoshop segments are rewritten; every other segment (APP0, ICC and MPF
//! in APP2, extended XMP, comments, tables, the frame) and the scan data are copied byte for byte.
//!
//! - **EXIF** ([`exif::edit::edit`]): the date taken, the place and the ImageDescription copy of
//!   the caption, edited in place or appended, never repacked. The block must fit one APP1
//!   ([`MAX_JPEG_EXIF`]) or the write fails naming the field. The segment keeps its own six-byte
//!   `Exif\0\0` header.
//! - **XMP** ([`merge_at`]): the standard packet merged, its padding absorbing the change. When
//!   the packet no longer fits one APP1 ([`JPEG_MAX_PACKET`]) its padding goes first; if it still
//!   does not fit, the write fails naming the field, and nothing else is ever dropped. Extended
//!   XMP is never written: its chunks and the `xmpNote:HasExtendedXMP` declaration stay as they
//! - **IPTC** ([`iptc::write::write_resources`]): resource 0x0404 and its digest 0x0425 inside the
//!   Photoshop resources, every other resource byte for byte. IPTC is written whenever the caption
//!   or tags change (as `xmp-interop.md` §9 proposes), except that a file without IPTC gets none
//!   for an edit that only removes values. Resources split over several APP13 segments are
//!   rejoined in the first one's place, and split again at the segment limit when one segment
//!   cannot hold them (readers join consecutive Photoshop segments).
//! - **New segments** go where readers look for them: EXIF right after SOI and its APP0 (JFIF,
//!   JFXX) segments, XMP right after the EXIF segment, APP13 after the last APP0–APP12 segment of
//!   the leading run (so before an Adobe APP14, the tables and the frame); and never after an MPF
//!   index.
//! - **MPF** ([`mpf_moved`]): the index locates the other images of a multi-picture file (a
//!   phone's or camera's preview, after the primary image's EOI) by offsets counted from its own
//!   APP2 segment, so a write must not move them relative to it. New segments therefore go before
//!   the index, and a write whose rewritten segments after the index change size is refused,
//!   naming the field, when the index lists other images (or cannot be read and the file holds
//!   data after its EOI). The index itself is copied byte for byte.
//! - A block that already holds the values is left as it is, so writing the same values twice
//!   gives the same bytes.
//!
//! `xmp:MetadataDate` is the time of the write the caller passes ([`WriteOptions::metadata_date`]);
//! this crate reads no clock.
//!
//! Provenance: written for Dalica from ITU-T T.81 (marker segments, at most 65,535 bytes counting
//! the length field), the Exif 2.32 specification (APP1 after SOI), the XMP Specification Part 3
//! (the standard packet in one APP1) and the public description of Photoshop image resources in
//! APP13, and CIPA DC-007 (the MP Index IFD and its MP entries, offsets from the MP header). No
//! other writer's code was consulted.

use std::ops::Range;

use super::{Changes, WriteOptions};
use crate::container::jpeg::{PHOTOSHOP_SIGNATURE, XMP_SIGNATURE};
use crate::container::{BlockKind, Scan, be_u16, take};
use crate::exif::edit::{ExifEdit, MAX_JPEG_EXIF};
use crate::fields::{DateTaken, clean_tags, clean_text};
use crate::iptc::write::IptcEdit;
use crate::xmp::JPEG_MAX_PACKET;
use crate::xmp::merge::merge_at;
use crate::{Error, Limits, Metadata, Result, exif, iptc};

/// The most payload one marker segment holds: 65,535 less the two-byte length field.
pub const MAX_SEGMENT: usize = 65_533;

const SOI_LEN: usize = 2;
/// Bytes before an MPF segment's MP header: FF E2, the length field and `MPF\0`.
const MPF_HEADER_AT: usize = 8;
const APP0: u8 = 0xe0;
const APP1: u8 = 0xe1;
const APP12: u8 = 0xec;
const APP13: u8 = 0xed;
const COM: u8 = 0xfe;
const SOS: u8 = 0xda;
const EOI: u8 = 0xd9;

/// The header of an EXIF APP1 that Dalica starts.
const EXIF_HEADER: &[u8] = b"Exif\0\0";

/// The new bytes of `bytes` (walked as `scan`) with `changes` written.
pub(super) fn write(bytes: &[u8], scan: &Scan<'_>, before: &Metadata, changes: &Changes, options: &WriteOptions) -> Result<Vec<u8>> {
    let _ = before;
    let (limits, metadata_date) = (&options.limits, options.metadata_date.as_ref());
    let segments = segments(bytes, limits)?;
    let span = |kind: BlockKind| scan.spans.iter().find(|s| s.kind == kind && s.rewritable).map(|s| s.range.clone());
    let exif_span = span(BlockKind::Exif);
    let xmp_span = span(BlockKind::Xmp);
    let photoshop: Vec<Range<usize>> =
        scan.spans.iter().filter(|s| s.kind == BlockKind::Photoshop && s.rewritable).map(|s| s.range.clone()).collect();
    // No new segment goes after an MPF index (CIPA DC-007): the images it lists would move.
    let mpf = scan.spans.iter().find(|s| s.kind == BlockKind::Mpf).map(|s| s.range.clone());
    let cap = |at: usize| mpf.as_ref().map_or(at, |r| at.min(r.start));
    let exif_at = cap(after_app0(&segments));
    let mut edits: Vec<Edit> = Vec::new();

    if let Some(block) = new_exif(scan.exif.as_deref(), changes, limits)? {
        // An existing segment keeps its header bytes (the sixth is padding some writers fill).
        let header = exif_span.as_ref().and_then(|r| bytes.get(r.start + 4..r.start + 4 + EXIF_HEADER.len())).unwrap_or(EXIF_HEADER);
        let segment = segment(APP1, &[header, &block])?;
        edits.push(match &exif_span {
            Some(r) => Edit::replace(r.clone(), segment),
            None => Edit::insert(exif_at, Order::Exif, segment),
        });
    }

    if let Some(packet) = new_xmp(scan.xmp.as_deref(), changes, metadata_date, limits)? {
        let segment = segment(APP1, &[XMP_SIGNATURE, &packet])?;
        edits.push(match &xmp_span {
            Some(r) => Edit::replace(r.clone(), segment),
            // Right after the EXIF segment: the one there is, or the one inserted above.
            None => Edit::insert(cap(exif_span.as_ref().map_or(exif_at, |r| r.end)), Order::Xmp, segment),
        });
    }

    if let Some(resources) = new_photoshop(scan, changes, limits)? {
        let mut pieces = Vec::new();
        for piece in resources.chunks(MAX_SEGMENT - PHOTOSHOP_SIGNATURE.len()) {
            pieces.extend(segment(APP13, &[PHOTOSHOP_SIGNATURE, piece])?);
        }
        match photoshop.split_first() {
            Some((first, rest)) => {
                edits.push(Edit::replace(first.clone(), pieces));
                edits.extend(rest.iter().map(|r| Edit::replace(r.clone(), Vec::new())));
            }
            None => edits.push(Edit::insert(cap(app13_at(&segments, exif_at)), Order::Photoshop, pieces)),
        }
    }

    if let Some(mpf) = &mpf
        && mpf_moved(bytes, mpf, &edits, &segments, limits)
    {
        return Err(Error::Refused { field: first_field(changes), why: "it would move the other images this file's MPF index locates".into() });
    }
    Ok(apply(bytes, edits))
}

/// Whether `edits` would move the images the MPF index in `mpf` locates: a size change after the
/// index's segment, where the index lists an image other than the primary, or cannot be read
/// while the file holds data after its EOI (where such images live).
fn mpf_moved(bytes: &[u8], mpf: &Range<usize>, edits: &[Edit], segments: &[Segment], limits: &Limits) -> bool {
    let (grown, shrunk) = edits
        .iter()
        .filter(|e| e.range.start >= mpf.end)
        .fold((0usize, 0usize), |(g, s), e| (g.saturating_add(e.with.len()), s.saturating_add(e.range.len())));
    if grown == shrunk {
        return false;
    }
    match mpf_offsets(bytes, mpf) {
        Some(offsets) => offsets.iter().any(|&o| o != 0),
        None => data_after_eoi(bytes, segments.last().map_or(SOI_LEN, |s| s.range.end), limits),
    }
}

/// The offsets of the MP entries in the MPF segment at `mpf` (CIPA DC-007 §5.2: the MP Index IFD's
/// MPEntry tag, 16 bytes per image, the offset in bytes 8–11; 0 for the primary image), or `None`
/// when the index cannot be read. Everything read lies inside the segment.
fn mpf_offsets(bytes: &[u8], mpf: &Range<usize>) -> Option<Vec<u32>> {
    let p = bytes.get(mpf.start.checked_add(MPF_HEADER_AT)?..mpf.end)?;
    let big = match p.get(..4)? {
        b"MM\0*" => true,
        b"II*\0" => false,
        _ => return None,
    };
    let u = |at: usize, n: usize| -> Option<usize> {
        let b = p.get(at..at.checked_add(n)?)?;
        let fold = |acc: usize, &x: &u8| (acc << 8) | usize::from(x);
        Some(if big { b.iter().fold(0, fold) } else { b.iter().rev().fold(0, fold) })
    };
    let ifd = u(4, 4)?;
    for k in 0..u(ifd, 2)? {
        let entry = ifd.checked_add(2 + 12 * k)?;
        if u(entry, 2)? != 0xb002 {
            continue;
        }
        let (len, data) = (u(entry + 4, 4)?, u(entry + 8, 4)?);
        p.get(data..data.checked_add(len)?)?;
        return (0..len / 16).map(|i| u(data + 16 * i + 8, 4).and_then(|o| u32::try_from(o).ok())).collect();
    }
    None
}

/// Whether the file holds bytes after its primary image's EOI, walking from the first SOS at or
/// after `at` (ITU-T T.81 B.1): scans and the segments between them, entropy-coded data skipped by
/// its markers (FF00 stuffing and RSTn are data). A file that ends without an EOI holds none.
fn data_after_eoi(b: &[u8], mut at: usize, limits: &Limits) -> bool {
    for _ in 0..limits.max_segments {
        // The marker: FF, any fill bytes, the code.
        if b.get(at) != Some(&0xff) {
            return false;
        }
        while b.get(at + 1) == Some(&0xff) {
            at += 1;
        }
        let Some(&marker) = b.get(at + 1) else { return false };
        match marker {
            EOI => return b.len() > at + 2,
            0x01 | 0xd0..=0xd8 => at += 2,
            _ => {
                let Ok(len) = be_u16(b, at + 2) else { return false };
                at += 2 + usize::from(len);
                if marker == SOS {
                    // Entropy-coded data, up to the next marker that is not stuffing or a restart.
                    loop {
                        let Some(next) = b.get(at..).and_then(|rest| rest.iter().position(|&x| x == 0xff)) else { return false };
                        at += next;
                        match b.get(at + 1) {
                            Some(0x00 | 0xd0..=0xd7) => at += 2,
                            Some(_) => break,
                            None => return false,
                        }
                    }
                }
            }
        }
    }
    false
}

/// The first field a change asks for, in the order they are written, for a refusal's message.
fn first_field(changes: &Changes) -> &'static str {
    if changes.caption.is_some() {
        "caption"
    } else if changes.tags.is_some() {
        "tags"
    } else if changes.star.is_some() {
        "star"
    } else if changes.place.is_some() {
        "place"
    } else {
        "date taken"
    }
}

/// The new EXIF block, or `None` when it stays as it is.
fn new_exif(old: Option<&[u8]>, changes: &Changes, limits: &Limits) -> Result<Option<Vec<u8>>> {
    let edit = ExifEdit::of(changes);
    if edit.is_empty() {
        return Ok(None);
    }
    Ok(exif::edit::edit(old, &edit, MAX_JPEG_EXIF, limits)?.filter(|new| Some(new.as_slice()) != old))
}

/// The new standard XMP packet fitted to one APP1, or `None` when it stays as it is.
fn new_xmp(old: Option<&[u8]>, changes: &Changes, metadata_date: Option<&DateTaken>, limits: &Limits) -> Result<Option<Vec<u8>>> {
    let Some(merged) = merge_at(old, changes, metadata_date, limits)? else { return Ok(None) };
    merged.fit(JPEG_MAX_PACKET).map(Some).ok_or_else(|| Error::TooMany(xmp_overflow(old, changes, metadata_date, limits)))
}

/// The field that pushes the packet past one APP1: the first, in the order they are written,
/// that no longer fits once added to the ones before it.
fn xmp_overflow(old: Option<&[u8]>, changes: &Changes, metadata_date: Option<&DateTaken>, limits: &Limits) -> &'static str {
    let fits = |c: &Changes| match merge_at(old, c, metadata_date, limits) {
        Ok(Some(m)) => m.min_len() <= JPEG_MAX_PACKET,
        _ => true,
    };
    let mut so_far = Changes { caption: changes.caption.clone(), ..Changes::default() };
    if !fits(&so_far) {
        return "XMP bytes for the caption";
    }
    so_far.tags.clone_from(&changes.tags);
    if !fits(&so_far) {
        return "XMP bytes for the tags";
    }
    so_far.star = changes.star;
    if !fits(&so_far) {
        return "XMP bytes for the star";
    }
    so_far.place = changes.place;
    if !fits(&so_far) {
        return "XMP bytes for the place";
    }
    "XMP bytes for the date taken"
}

/// The new Photoshop resources, or `None` when they stay as they are.
fn new_photoshop(scan: &Scan<'_>, changes: &Changes, limits: &Limits) -> Result<Option<Vec<u8>>> {
    let edit = IptcEdit::of(changes);
    if edit.is_empty() || (scan.iptc_stream(limits)?.is_none() && only_removes(&edit, limits)) {
        return Ok(None);
    }
    let old = scan.photoshop.as_deref();
    let written = iptc::write::write_resources(old, &edit, limits)?;
    Ok(Some(written.bytes).filter(|new| Some(new.as_slice()) != old))
}

/// Whether an IPTC edit only clears values (nothing to write into a file without IPTC).
fn only_removes(edit: &IptcEdit<'_>, limits: &Limits) -> bool {
    let caption = edit.caption.flatten().and_then(|c| clean_text(c, limits.max_string)).is_some();
    let keywords = edit.keywords.is_some_and(|k| !clean_tags(k.iter().map(String::as_str), limits.max_string, limits.max_tags).is_empty());
    !caption && !keywords
}

/// A marker segment: `FF marker`, the length, then the parts. `Err` past [`MAX_SEGMENT`].
fn segment(marker: u8, parts: &[&[u8]]) -> Result<Vec<u8>> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    let field = u16::try_from(len + 2).ok().filter(|_| len <= MAX_SEGMENT).ok_or(Error::TooMany("bytes for one JPEG segment"))?;
    let mut out = Vec::with_capacity(len + 4);
    out.extend([0xff, marker]);
    out.extend(field.to_be_bytes());
    for p in parts {
        out.extend_from_slice(p);
    }
    Ok(out)
}

/// One marker segment before the scan.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Segment {
    marker: u8,
    range: Range<usize>,
}

/// The marker segments between SOI and the first SOS (or EOI), walked as the reader walks them.
fn segments(b: &[u8], limits: &Limits) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    let mut at = SOI_LEN;
    loop {
        if out.len() >= limits.max_segments {
            return Err(Error::TooMany("JPEG segments"));
        }
        if take(b, at, 1)?.first() != Some(&0xff) {
            return Err(Error::Malformed("expected a JPEG marker".into()));
        }
        while take(b, at + 1, 1)?.first() == Some(&0xff) {
            at += 1;
        }
        let marker = *take(b, at + 1, 1)?.first().unwrap_or(&0);
        match marker {
            SOS | EOI => return Ok(out),
            0x01 | 0xd0..=0xd8 => {
                out.push(Segment { marker, range: at..at + 2 });
                at += 2;
                continue;
            }
            _ => {}
        }
        let len = usize::from(be_u16(b, at + 2)?);
        if len < 2 {
            return Err(Error::Malformed("a JPEG segment shorter than its length field".into()));
        }
        take(b, at + 2, len)?;
        out.push(Segment { marker, range: at..at + 2 + len });
        at += 2 + len;
    }
}

/// Where a new EXIF segment goes: after SOI and the APP0 segments that follow it.
fn after_app0(segments: &[Segment]) -> usize {
    segments.iter().take_while(|s| s.marker == APP0).last().map_or(SOI_LEN, |s| s.range.end)
}

/// Where a new APP13 goes: after the last APP0–APP12 segment of the run of application segments
/// and comments that follows SOI, and never before `floor` (where a new EXIF and XMP go).
fn app13_at(segments: &[Segment], floor: usize) -> usize {
    segments
        .iter()
        .take_while(|s| (APP0..=APP13).contains(&s.marker) || s.marker == COM)
        .filter(|s| (APP0..=APP12).contains(&s.marker))
        .last()
        .map_or(floor, |s| s.range.end.max(floor))
}

/// The order of the changes at one offset: insertions (EXIF, then XMP, then APP13) before a
/// replacement starting there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Order {
    Exif,
    Xmp,
    Photoshop,
    Replace,
}

/// One change to the file: `range` (empty for an insertion) replaced by `with`.
struct Edit {
    range: Range<usize>,
    order: Order,
    with: Vec<u8>,
}

impl Edit {
    fn replace(range: Range<usize>, with: Vec<u8>) -> Edit {
        Edit { range, order: Order::Replace, with }
    }

    fn insert(at: usize, order: Order, with: Vec<u8>) -> Edit {
        Edit { range: at..at, order, with }
    }
}

/// `bytes` with the edits made. The replaced ranges are whole, distinct segments, so none overlap.
fn apply(bytes: &[u8], mut edits: Vec<Edit>) -> Vec<u8> {
    edits.sort_by_key(|e| (e.range.start, e.order));
    let grow: usize = edits.iter().map(|e| e.with.len()).sum();
    let mut out = Vec::with_capacity(bytes.len().saturating_add(grow));
    let mut at = 0usize;
    for e in edits {
        out.extend_from_slice(bytes.get(at..e.range.start.max(at)).unwrap_or_default());
        out.extend_from_slice(&e.with);
        at = at.max(e.range.end);
    }
    out.extend_from_slice(bytes.get(at..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_capped() {
        assert_eq!(segment(APP1, &[&[0; MAX_SEGMENT]]).map(|s| s.len()), Ok(MAX_SEGMENT + 4));
        assert_eq!(segment(APP1, &[&[0; MAX_SEGMENT], &[0]]), Err(Error::TooMany("bytes for one JPEG segment")));
        assert_eq!(segment(COM, &[b"ab", b"c"]), Ok(vec![0xff, COM, 0, 5, b'a', b'b', b'c']));
    }

    #[test]
    fn insertion_points() {
        let s = |marker: u8, range: Range<usize>| Segment { marker, range };
        // SOI, APP0, APP0, APP2, COM, APP14 (Adobe), DQT.
        let segs = [s(APP0, 2..10), s(APP0, 10..20), s(0xe2, 20..30), s(COM, 30..40), s(0xee, 40..50), s(0xdb, 50..60)];
        assert_eq!(after_app0(&segs), 20);
        assert_eq!(app13_at(&segs, 20), 30);
        // Straight into the tables: both right after SOI.
        let bare = [s(0xdb, 2..10)];
        assert_eq!((after_app0(&bare), app13_at(&bare, 2)), (2, 2));
        // An APP1 after the tables is not part of the leading run.
        let late = [s(APP0, 2..10), s(0xdb, 10..20), s(APP1, 20..30)];
        assert_eq!(app13_at(&late, 10), 10);
    }

    #[test]
    fn edits_apply_in_order() {
        let out = apply(
            b"0123456789",
            vec![
                Edit::replace(2..4, b"R".to_vec()),
                Edit::insert(2, Order::Xmp, b"x".to_vec()),
                Edit::insert(2, Order::Exif, b"e".to_vec()),
                Edit::replace(6..8, Vec::new()),
                Edit::insert(10, Order::Photoshop, b"P".to_vec()),
            ],
        );
        assert_eq!(out, b"01exR4589P");
        assert_eq!(apply(b"abc", Vec::new()), b"abc");
    }
}
