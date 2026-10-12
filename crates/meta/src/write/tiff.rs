//! TIFF write-back (`xmp-interop.md` §3–§4): XMP (tag 700) and IPTC (tag 33723, and resource
//! 0x0404 of the Photoshop resources in tag 34377 when the file keeps IPTC there) by the
//! append-only rule. The place and the date go to the XMP mirrors only; the EXIF and GPS IFDs are
//! never written.
//!
//! The append-only rule: nothing outside IFD0's table and the old values of the tags Dalica writes
//! changes or moves, so strips, tiles, maker notes and every other offset in the file stay valid.
//!
//! - A new value no larger than the old one is written where the old one was (the rest of its bytes
//!   zeroed); a larger or new one is appended at the end of the file (word aligned) and the old
//!   bytes are zeroed, so an old caption does not linger.
//! - IFD0 is rewritten in its own place when its table does not grow; one that gains an entry is
//!   appended whole and only the header's IFD0 offset changes. Entries keep their bytes, their
//!   order and the next-IFD offset; new ones go in tag order.
//! - ImageDescription (tag 270) follows the caption (§3) when it reads as one: a new caption is
//!   appended and pointed to, a cleared caption removes the entry. Its old bytes lie outside what
//!   the rule lets change, so they stay where they were, no longer referenced.
//! - Types are kept: an existing value keeps its field type (a LONG IPTC stream padded to whole
//!   words with zeros, which readers take as padding); a new XMP value is BYTE and a new IPTC
//!   stream LONG, as most TIFF writers store them.
//! - Before anything is written in place, every IFD (IFD0's chain, the EXIF, GPS, Interoperability
//!   and sub-IFDs), every value and every strip or tile is located; a file where IFD0's table or
//!   the values Dalica rewrites share bytes with any of them, or with each other, is refused, so an
//!   in-place write can never touch image data.
//! - An IFD0 holding one of these tags twice is refused (which one a reader takes is not defined).
//!   Writing the values the file already holds changes nothing.
//!
//! Provenance: written for Dalica from the public TIFF 6.0 specification (header, IFD structure,
//! field types, word alignment, strips and tiles), the XMP Specification Part 3 (tag 700) and the
//! public descriptions of tag 33723 (IPTC-NAA) and 34377 (Photoshop image resources). No other
//! writer's code was consulted.

use std::ops::Range;

use dalica_image::ifd::{Entry, Ifd, Tiff, Visited, types};

use super::{Changes, WriteOptions};
use crate::exif::{TAG_EXIF_IFD, TAG_GPS_IFD, TAG_IMAGE_DESCRIPTION, TAG_INTEROP_IFD, TAG_IPTC, TAG_PHOTOSHOP, TAG_XMP};
use crate::fields::clean_text;
use crate::iptc::RESOURCE_IPTC;
use crate::iptc::read::resources;
use crate::iptc::write::{IptcEdit, write_resources, write_stream};
use crate::xmp::merge::merge_at;
use crate::{Error, Limits, Metadata, Result, Scan};

/// SubIFDs (TIFF Technical Note 1): followed when locating everything the file references.
const TAG_SUB_IFDS: u16 = 0x014a;
/// Pairs of (offsets, byte counts) tags that locate image data: strips, tiles and the JPEG
/// interchange stream of old-style JPEG compression or an EXIF thumbnail.
const DATA_TAGS: [(u16, u16); 3] = [(0x0111, 0x0117), (0x0144, 0x0145), (0x0201, 0x0202)];
/// The most strips or tiles of one image located before a write.
const MAX_DATA_PIECES: usize = 1 << 20;
/// The tags whose old values are Dalica's to overwrite or zero.
const OWNED_VALUES: [u16; 3] = [TAG_XMP, TAG_IPTC, TAG_PHOTOSHOP];

/// A value to write into IFD0, or `None` to remove the entry.
struct Change {
    tag: u16,
    kind: u16,
    value: Option<Vec<u8>>,
}

/// The new bytes of `bytes` (walked as `scan`, read as `before`) with `changes` written.
pub(super) fn write(bytes: &[u8], scan: &Scan<'_>, before: &Metadata, changes: &Changes, options: &WriteOptions) -> Result<Vec<u8>> {
    let limits = &options.limits;
    let tiff = Tiff::new(bytes)?;
    let first = tiff.first_ifd()?;
    let ifd0 = tiff.ifd(first, &mut Visited::default(), &limits.ifd())?;
    for tag in [TAG_XMP, TAG_IPTC, TAG_PHOTOSHOP, TAG_IMAGE_DESCRIPTION] {
        if ifd0.entries.iter().filter(|e| e.tag == tag).count() > 1 {
            return Err(Error::Malformed(format!("IFD0 holds tag {tag} twice; not rewriting it")));
        }
    }
    let wanted = wanted(&tiff, &ifd0, scan, before, changes, options)?;
    if wanted.is_empty() {
        return Ok(bytes.to_vec());
    }
    let table = table_range(first, ifd0.entries.len());
    let owned: Vec<Range<usize>> = owned_values(&tiff, &ifd0)?;
    check_disjoint(&tiff, first, &table, &owned, limits)?;
    apply(bytes, &tiff, &ifd0, table, &wanted)
}

/// What to write: the merged XMP, the IPTC stream and resources, and ImageDescription; only the
/// values that differ from the file's.
fn wanted(tiff: &Tiff<'_>, ifd0: &Ifd, scan: &Scan<'_>, before: &Metadata, changes: &Changes, options: &WriteOptions) -> Result<Vec<Change>> {
    let limits = &options.limits;
    let mut out = Vec::new();
    let kind_of = |tag: u16, new: u16| ifd0.get(tag).map_or(new, |e| e.kind);

    if let Some(merged) = merge_at(scan.xmp.as_deref(), changes, options.metadata_date.as_ref(), limits)? {
        out.push(Change { tag: TAG_XMP, kind: kind_of(TAG_XMP, types::BYTE), value: Some(merged.into_bytes()) });
    }

    let edit = IptcEdit::of(changes);
    if !edit.is_empty() {
        let old_value = |tag: u16| -> Result<Option<Vec<u8>>> {
            Ok(match ifd0.get(tag) {
                Some(e) => tiff.value(e)?.map(|v| v.into_owned()),
                None => None,
            })
        };
        let in_resources = match scan.photoshop.as_deref() {
            Some(ps) => resources(ps, limits)?.iter().any(|r| r.id == RESOURCE_IPTC),
            None => false,
        };
        let bare = ifd0.get(TAG_IPTC).is_some();
        // The bare stream: updated where it is, or started when nothing else holds IPTC and there
        // is something to write (removing what is not there needs no stream).
        if bare || (!in_resources && adds(&edit, limits)) {
            let kind = kind_of(TAG_IPTC, types::LONG);
            let new = padded(write_stream(scan.iptc.as_deref(), &edit, limits)?.bytes, kind)?;
            if old_value(TAG_IPTC)?.as_deref() != Some(new.as_slice()) {
                out.push(Change { tag: TAG_IPTC, kind, value: Some(new) });
            }
        }
        if in_resources {
            let kind = kind_of(TAG_PHOTOSHOP, types::UNDEFINED);
            let new = padded(write_resources(scan.photoshop.as_deref(), &edit, limits)?.bytes, kind)?;
            if old_value(TAG_PHOTOSHOP)?.as_deref() != Some(new.as_slice()) {
                out.push(Change { tag: TAG_PHOTOSHOP, kind, value: Some(new) });
            }
        }
    }

    // ImageDescription follows the caption when it is a copy of one (not a camera placeholder).
    if let Some(caption) = &changes.caption
        && let Some(old) = &before.sources.exif.caption
    {
        match caption.as_deref().and_then(|c| clean_text(c, limits.max_string)) {
            Some(new) if new == *old => {}
            Some(new) => {
                let mut value = new.into_bytes();
                value.push(0);
                out.push(Change { tag: TAG_IMAGE_DESCRIPTION, kind: types::ASCII, value: Some(value) });
            }
            None => out.push(Change { tag: TAG_IMAGE_DESCRIPTION, kind: types::ASCII, value: None }),
        }
    }
    Ok(out)
}

/// Whether an IPTC edit puts anything into a stream (a caption or at least one tag).
fn adds(edit: &IptcEdit<'_>, limits: &Limits) -> bool {
    edit.caption.flatten().and_then(|c| clean_text(c, limits.max_string)).is_some() || edit.keywords.is_some_and(|k| !k.is_empty())
}

/// `value` padded with zeros to whole units of field type `kind`.
fn padded(mut value: Vec<u8>, kind: u16) -> Result<Vec<u8>> {
    let unit = unit(kind)?;
    while !value.len().is_multiple_of(unit) {
        value.push(0);
    }
    Ok(value)
}

/// Bytes per value of a TIFF field type.
fn unit(kind: u16) -> Result<usize> {
    match kind {
        1 | 2 | 6 | 7 => Ok(1),
        3 | 8 => Ok(2),
        4 | 9 | 11 | 13 => Ok(4),
        5 | 10 | 12 => Ok(8),
        _ => Err(Error::Malformed(format!("an IFD0 metadata tag of unknown field type {kind}"))),
    }
}

/// The bytes of an IFD at `at` with `entries` entries: count, entries and the next-IFD offset.
fn table_range(at: u32, entries: usize) -> Range<usize> {
    let start = at as usize;
    start..start + 2 + 12 * entries + 4
}

/// Where the old out-of-line values of tags 700, 33723 and 34377 lie.
fn owned_values(tiff: &Tiff<'_>, ifd0: &Ifd) -> Result<Vec<Range<usize>>> {
    let mut out = Vec::new();
    for e in ifd0.entries.iter().filter(|e| OWNED_VALUES.contains(&e.tag)) {
        if let Some(r) = tiff.value_range(e)? {
            out.push(r);
        }
    }
    Ok(out)
}

/// Refuses a file where IFD0's table or the old values Dalica may overwrite share a byte with each
/// other or with anything else the file references: the header, every other IFD and value, and
/// the image data of every strip and tile.
fn check_disjoint(tiff: &Tiff<'_>, first: u32, table: &Range<usize>, owned: &[Range<usize>], limits: &Limits) -> Result<()> {
    let refused = || Err(Error::Malformed("IFD0 or its metadata shares bytes with other data; not rewriting it".to_owned()));
    let mut targets: Vec<&Range<usize>> = owned.iter().collect();
    targets.push(table);
    for (i, a) in targets.iter().enumerate() {
        if targets.iter().skip(i + 1).any(|b| overlaps(a, b)) {
            return refused();
        }
    }
    let il = limits.ifd();
    let follow = [TAG_EXIF_IFD, TAG_GPS_IFD, TAG_INTEROP_IFD, TAG_SUB_IFDS];
    let ifds = tiff.walk(first, &follow, &il, &mut Visited::default())?;
    // The header: byte order, magic number and the IFD0 offset.
    const HEADER: Range<usize> = 0..8;
    let mut others: Vec<Range<usize>> = Vec::new();
    others.push(HEADER);
    for (ifd, _) in &ifds {
        let ifd0 = ifd.offset == first;
        if !ifd0 {
            others.push(table_range(ifd.offset, ifd.entries.len()));
        }
        for e in &ifd.entries {
            if ifd0 && OWNED_VALUES.contains(&e.tag) {
                continue;
            }
            if let Some(r) = tiff.value_range(e)? {
                others.push(r);
            }
        }
        for (offsets, counts) in DATA_TAGS {
            let (Some(o), Some(c)) = (ifd.get(offsets), ifd.get(counts)) else { continue };
            others.extend(pieces(tiff, o, c)?);
        }
    }
    if targets.iter().any(|t| others.iter().any(|o| overlaps(t, o))) {
        return refused();
    }
    Ok(())
}

/// The byte ranges an offsets tag and its byte counts tag describe.
fn pieces(tiff: &Tiff<'_>, offsets: &Entry, counts: &Entry) -> Result<Vec<Range<usize>>> {
    let o = tiff.uints(offsets, MAX_DATA_PIECES, "strips or tiles")?;
    let c = tiff.uints(counts, MAX_DATA_PIECES, "strips or tiles")?;
    Ok(o.into_iter().zip(c).map(|(at, len)| at as usize..(at as usize).saturating_add(len as usize)).collect())
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// Writes `changes` into a copy of the file by the append-only rule.
fn apply(bytes: &[u8], tiff: &Tiff<'_>, ifd0: &Ifd, table: Range<usize>, changes: &[Change]) -> Result<Vec<u8>> {
    let little = tiff.is_little_endian();
    let u16b = |v: u16| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let u32b = |v: u32| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let mut out = bytes.to_vec();
    // Each entry as its 12 bytes, from the file.
    let mut entries: Vec<(u16, [u8; 12])> = Vec::with_capacity(ifd0.entries.len() + changes.len());
    for (i, e) in ifd0.entries.iter().enumerate() {
        let at = table.start + 2 + 12 * i;
        let raw: [u8; 12] = bytes.get(at..at + 12).and_then(|b| b.try_into().ok()).ok_or(Error::Truncated)?;
        entries.push((e.tag, raw));
    }

    for change in changes {
        let old = ifd0.get(change.tag);
        let old_range = match old {
            Some(e) => tiff.value_range(e)?,
            None => None,
        };
        // Only the values of the tags in OWNED_VALUES may be overwritten or zeroed.
        let reusable = old_range.filter(|_| OWNED_VALUES.contains(&change.tag));
        let Some(value) = &change.value else {
            entries.retain(|(tag, _)| *tag != change.tag);
            if let Some(r) = reusable {
                zero(&mut out, r);
            }
            continue;
        };
        let count = u32::try_from(value.len() / unit(change.kind)?).map_err(|_| Error::TooMany("bytes in one TIFF value"))?;
        let field: [u8; 4] = if value.len() <= 4 {
            let mut inline = [0u8; 4];
            inline.get_mut(..value.len()).ok_or(Error::Truncated)?.copy_from_slice(value);
            if let Some(r) = reusable {
                zero(&mut out, r);
            }
            inline
        } else if let Some(r) = reusable.as_ref().filter(|r| value.len() <= r.len()) {
            zero(&mut out, r.clone());
            out.get_mut(r.start..r.start + value.len()).ok_or(Error::Truncated)?.copy_from_slice(value);
            u32b(offset(r.start)?)
        } else {
            if let Some(r) = reusable {
                zero(&mut out, r);
            }
            u32b(append(&mut out, value)?)
        };
        let mut raw = [0u8; 12];
        raw[..2].copy_from_slice(&u16b(change.tag));
        raw[2..4].copy_from_slice(&u16b(change.kind));
        raw[4..8].copy_from_slice(&u32b(count));
        raw[8..].copy_from_slice(&field);
        match entries.iter_mut().find(|(tag, _)| *tag == change.tag) {
            Some(slot) => slot.1 = raw,
            None => {
                let at = entries.iter().position(|(tag, _)| *tag > change.tag).unwrap_or(entries.len());
                entries.insert(at, (change.tag, raw));
            }
        }
    }

    let count = u16::try_from(entries.len()).map_err(|_| Error::TooMany("IFD entries"))?;
    let mut new_table = Vec::with_capacity(2 + 12 * entries.len() + 4);
    new_table.extend_from_slice(&u16b(count));
    entries.iter().for_each(|(_, raw)| new_table.extend_from_slice(raw));
    new_table.extend_from_slice(&u32b(ifd0.next));
    if new_table.len() <= table.len() {
        zero(&mut out, table.clone());
        out.get_mut(table.start..table.start + new_table.len()).ok_or(Error::Truncated)?.copy_from_slice(&new_table);
    } else {
        let at = append(&mut out, &new_table)?;
        out.get_mut(4..8).ok_or(Error::Truncated)?.copy_from_slice(&u32b(at));
    }
    Ok(out)
}

/// Appends `value` at the next even offset and returns that offset.
fn append(out: &mut Vec<u8>, value: &[u8]) -> Result<u32> {
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let at = offset(out.len())?;
    offset(out.len().saturating_add(value.len()))?;
    out.extend_from_slice(value);
    Ok(at)
}

/// A file offset as TIFF stores it; files past 4 GiB cannot grow.
fn offset(at: usize) -> Result<u32> {
    u32::try_from(at).map_err(|_| Error::TooMany("bytes in a TIFF file"))
}

fn zero(out: &mut [u8], r: Range<usize>) {
    if let Some(b) = out.get_mut(r) {
        b.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_padding() {
        assert_eq!(unit(types::BYTE), Ok(1));
        assert_eq!(unit(types::LONG), Ok(4));
        assert!(unit(99).is_err());
        assert_eq!(padded(vec![1, 2, 3, 4, 5], types::LONG), Ok(vec![1, 2, 3, 4, 5, 0, 0, 0]));
        assert_eq!(padded(vec![1, 2, 3], types::UNDEFINED), Ok(vec![1, 2, 3]));
        assert!(overlaps(&(0..4), &(3..5)));
        assert!(!overlaps(&(0..4), &(4..5)));
    }

    #[test]
    fn offsets_stop_at_four_gigabytes() {
        assert_eq!(offset(0xffff_ffff), Ok(u32::MAX));
        assert_eq!(offset(0x1_0000_0000), Err(Error::TooMany("bytes in a TIFF file")));
    }

    #[test]
    fn appending_aligns_to_words() {
        let mut out = vec![1, 2, 3];
        assert_eq!(append(&mut out, &[9, 9]), Ok(4));
        assert_eq!(out, [1, 2, 3, 0, 9, 9]);
    }
}
