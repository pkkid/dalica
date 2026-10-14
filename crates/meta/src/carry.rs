//! Metadata carried to a new file (`data-model.md` §6, `creating.md` §2): Save, Save a Copy, Save
//! As and Export render new pixels, and the new file takes the source's metadata as it applies to
//! those pixels.
//!
//! [`carry`] takes the source file's bytes (JPEG, PNG, WebP or TIFF, raws included) and the new
//! file the encoder wrote (JPEG or PNG, with its own ICC profile if it has one) and returns the new
//! file with:
//!
//! - **EXIF** ([`exif`]): the source's block with Orientation 1 (the pixels are already turned),
//!   the new pixel size, the sRGB colour space, the thumbnail IFD replaced by the given thumbnail
//!   or dropped; edited in place so maker notes keep their offsets. A source without EXIF gives a
//!   block holding just those facts.
//! - **XMP** ([`xmp`]): the source's packet with `tiff:Orientation` 1 and the new size where it
//!   states them, every other property byte for byte. A JPEG target also takes the extended packet
//!   unchanged (a PNG target has no place for it, and it is left out when removing the location
//!   finds GPS in it; [`Carried::dropped`] says so).
//! - **IPTC** ([`iptc`]): kept, as APP13 Photoshop resources in a JPEG and as a "Raw profile type
//!   iptc" text chunk in a PNG, so later write-back finds it.
//! - With [`Carry::remove_location`]: the EXIF GPS IFD and every `exif:GPS*` property gone; place
//!   names (IPTC, `photoshop:City` …) kept, and a place among the owned fields is not written.
//!
//! Then the owned fields the caller passes are merged by [`write_owned`] under the caller's
//! options (all on for copies and exports; the write-back options for Save in place), so the new
//! file reads back as the library has the photo, and the result is checked: it reads back cleanly,
//! orientation 1 and the new size in EXIF and XMP, no GPS when it was removed.
//!
//! [`set_orientation`] is the lossless fallback of a rotate-only Save: a JPEG whose pixels stay as
//! they are takes the composed orientation in IFD0 (and in `tiff:Orientation` when its XMP states
//! one), every other byte unchanged, and passes M6.4's [`verify`].
//!
//! The metadata blocks of the target (none, from Dalica's encoder) are replaced; anything else in
//! it (the ICC profile, the image data) is kept byte for byte. This module never touches the disk.
//!
//! Provenance: written for Dalica from the public JPEG (ITU-T T.81 marker segments), PNG 3rd
//! edition (`eXIf`, `iTXt`, `tEXt`, chunk order and CRC), Exif 2.32 (APP1), XMP Specification
//! Part 3 (APP1 standard and extended XMP, PNG `iTXt`) and Photoshop image resource (APP13)
//! descriptions, and the public description of "Raw profile type" text chunks. No other writer's
//! code was consulted.

pub mod exif;
pub mod iptc;
pub mod xmp;

use std::ops::Range;

use crate::container::jpeg::{EXTENDED_XMP_SIGNATURE, PHOTOSHOP_SIGNATURE, XMP_SIGNATURE, extended_guid};
use crate::container::png::XMP_KEYWORD;
use crate::container::{self, BlockKind, Container, Scan, be_u16, be_u32, take};
use crate::exif::read::walk;
use crate::exif::{IfdKind, TAG_GPS_IFD, TAG_ORIENTATION};
use crate::write::{Changes, WriteOptions, verify, write_owned};
use crate::xmp::JPEG_MAX_PACKET;
use crate::{Error, Limits, Result, read_scan};

use self::exif::MAX_JPEG_EXIF;
use self::iptc::Iptc;

/// The largest metadata block one PNG chunk holds (PNG 3rd edition §5.3: 2³¹ − 1 bytes, less room
/// for a text chunk's keyword and the hex encoding of a raw profile).
pub const MAX_PNG_BLOCK: usize = 0x3fff_0000;

/// The EXIF block a source without one starts from: a little-endian header and an empty IFD0.
const EMPTY_EXIF: [u8; 14] = *b"II*\0\x08\0\0\0\0\0\0\0\0\0";

/// What a new file is and what goes into it besides the source's metadata.
#[derive(Clone, Debug)]
pub struct Carry<'a> {
    /// The new file's pixel size, as stored (its orientation is 1).
    pub width: u32,
    pub height: u32,
    /// A JPEG thumbnail for the EXIF thumbnail IFD (Save); `None` drops the source's (Export).
    pub thumbnail: Option<&'a [u8]>,
    /// "Remove location": the EXIF GPS IFD and every `exif:GPS*` XMP property left out.
    pub remove_location: bool,
    /// The owned fields to merge after carrying (`write_owned`); a place is ignored when the
    /// location is removed.
    pub changes: Changes,
    /// The write options the owned fields pass through, the time of the write and the limits.
    pub options: WriteOptions,
}

/// The new file and what could not be carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    pub bytes: Vec<u8>,
    /// Whether the thumbnail asked for went in (it is left out when the EXIF block would not fit
    /// one JPEG segment with it).
    pub thumbnail: bool,
    /// What the target has no room or no place for ("extended XMP").
    pub dropped: Vec<&'static str>,
}

/// The new file `target` (a JPEG or PNG the encoder wrote from the source's pixels) with the
/// metadata of `source` carried over and the owned fields merged; see the module docs.
///
/// `Err` when either file's container cannot be walked, when the source's EXIF, XMP or IPTC is
/// unreadable (nothing would be carried wrongly or silently lost), when a block does not fit the
/// target, or when the result fails its check.
pub fn carry(source: &[u8], target: &[u8], c: &Carry<'_>) -> Result<Carried> {
    let limits = &c.options.limits;
    let kind = Container::detect(target).ok_or(Error::Unknown)?;
    if !matches!(kind, Container::Jpeg | Container::Png) {
        return Err(Error::Unsupported(kind));
    }
    let jpeg = kind == Container::Jpeg;
    let scan = container::scan(source, limits)?;
    let before = read_scan(&scan, limits);
    if let Some(p) = before.problems.iter().find(|p| matches!(p.block, BlockKind::Exif | BlockKind::Xmp | BlockKind::Photoshop | BlockKind::Iptc)) {
        return Err(Error::Malformed(format!("its {:?} block is unreadable ({}); not carrying it", p.block, p.error)));
    }
    let mut dropped = Vec::new();

    let options = exif::Options {
        width: c.width,
        height: c.height,
        remove_location: c.remove_location,
        thumbnail: c.thumbnail,
        max_len: if jpeg { MAX_JPEG_EXIF } else { MAX_PNG_BLOCK },
        whole_file: scan.container == Container::Tiff,
    };
    let exif = exif::carry(scan.exif.as_deref().unwrap_or(&EMPTY_EXIF), &options, limits)?;
    let max_xmp = if jpeg { JPEG_MAX_PACKET } else { MAX_PNG_BLOCK };
    let packet = match scan.xmp.as_deref() {
        Some(p) => xmp::carry(p, c.width, c.height, c.remove_location, max_xmp, limits)?,
        None => None,
    };
    let extended = match scan.extended_xmp.as_deref() {
        Some(ext) if jpeg && packet.is_some() && !(c.remove_location && xmp::has_gps(ext, limits)?) => Some(ext),
        Some(_) => {
            dropped.push("extended XMP");
            None
        }
        None => None,
    };
    if scan.problems.iter().any(|p| p.block == BlockKind::ExtendedXmp) && !dropped.contains(&"extended XMP") {
        dropped.push("extended XMP");
    }
    let iptc = iptc::carry(&scan, limits)?;
    let blocks = Blocks { exif: &exif.bytes, xmp: packet.as_deref(), extended, iptc };
    let assembled = if jpeg { into_jpeg(target, blocks, limits)? } else { into_png(target, blocks, limits)? };

    let mut changes = c.changes.clone();
    if c.remove_location {
        changes.place = None;
    }
    let bytes = write_owned(&assembled, &changes, &c.options)?;
    check(&bytes, c, limits)?;
    Ok(Carried { bytes, thumbnail: exif.thumbnail, dropped })
}

/// A JPEG with IFD0's Orientation set to `orientation` (1–8): the value written into the entry,
/// or IFD0 grown by it, or an EXIF block holding only it when the file has none; the XMP packet's
/// `tiff:Orientation` follows when it has one. Every other byte stays, which M6.4's [`verify`]
/// checks before the bytes are returned.
pub fn set_orientation(bytes: &[u8], orientation: u16, limits: &Limits) -> Result<Vec<u8>> {
    if !(1..=8).contains(&orientation) {
        return Err(Error::Refused { field: "orientation", why: format!("{orientation} is not an orientation (1 to 8)") });
    }
    let scan = container::scan(bytes, limits)?;
    if scan.container != Container::Jpeg {
        return Err(Error::Unsupported(scan.container));
    }
    let before = read_scan(&scan, limits);
    if let Some(p) = before.problems.iter().find(|p| matches!(p.block, BlockKind::Exif | BlockKind::Xmp)) {
        return Err(Error::Malformed(format!("its {:?} block is unreadable ({}); not rewriting it", p.block, p.error)));
    }
    let block = exif::set_orientation(scan.exif.as_deref(), orientation, MAX_JPEG_EXIF, limits)?;
    let mut edits = Vec::new();
    let segment = app(0xe1, &[b"Exif\0\0", &block])?;
    match rewritable(&scan, BlockKind::Exif) {
        Some(r) => edits.push((r, segment)),
        None => {
            let at = after_app0(&segments(bytes, limits)?.0);
            edits.push((at..at, segment));
        }
    }
    if let Some(packet) = scan.xmp.as_deref()
        && let Some(new) = xmp::set_orientation(packet, orientation, JPEG_MAX_PACKET, limits)?
        && let Some(r) = rewritable(&scan, BlockKind::Xmp)
    {
        edits.push((r, app(0xe1, &[XMP_SIGNATURE, &new])?));
    }
    let after = splice(bytes, edits);
    verify(bytes, &after, &Changes::default(), limits)?;
    let read = crate::read(&after, limits)?;
    if read.summary.orientation != u8::try_from(orientation).ok() {
        return Err(Error::Verify("the orientation does not read back as written".into()));
    }
    Ok(after)
}

/// The blocks going into the target.
struct Blocks<'a> {
    exif: &'a [u8],
    xmp: Option<&'a [u8]>,
    extended: Option<&'a [u8]>,
    iptc: Option<Iptc>,
}

/// The metadata blocks a target's own may be replaced by.
fn replaced(kind: BlockKind) -> bool {
    matches!(kind, BlockKind::Exif | BlockKind::Xmp | BlockKind::ExtendedXmp | BlockKind::Photoshop | BlockKind::Iptc)
}

/// The range of the first rewritable block of `kind`.
fn rewritable(scan: &Scan<'_>, kind: BlockKind) -> Option<Range<usize>> {
    scan.spans.iter().find(|s| s.kind == kind && s.rewritable).map(|s| s.range.clone())
}

/// `bytes` with each range replaced (ranges sorted, not overlapping; an empty range inserts).
fn splice(bytes: &[u8], mut edits: Vec<(Range<usize>, Vec<u8>)>) -> Vec<u8> {
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    let mut out = Vec::with_capacity(bytes.len() + edits.iter().map(|(_, b)| b.len()).sum::<usize>());
    let mut at = 0usize;
    for (r, with) in edits {
        if r.start < at {
            continue;
        }
        out.extend_from_slice(bytes.get(at..r.start).unwrap_or_default());
        out.extend_from_slice(&with);
        at = r.end;
    }
    out.extend_from_slice(bytes.get(at..).unwrap_or_default());
    out
}

// JPEG.

/// A marker segment of `parts`; `Err` past the 65,533 payload bytes one segment holds.
fn app(marker: u8, parts: &[&[u8]]) -> Result<Vec<u8>> {
    let len: usize = parts.iter().map(|p| p.len()).sum::<usize>() + 2;
    let len = u16::try_from(len).map_err(|_| Error::TooMany("bytes in one JPEG segment"))?;
    let mut out = Vec::with_capacity(usize::from(len) + 2);
    out.extend([0xff, marker]);
    out.extend(len.to_be_bytes());
    for p in parts {
        out.extend_from_slice(p);
    }
    Ok(out)
}

/// A JPEG segment: its marker and its bytes in the file.
type Segment = (u8, Range<usize>);

/// The segments before the first scan and where the scan starts.
fn segments(b: &[u8], limits: &Limits) -> Result<(Vec<Segment>, usize)> {
    let mut out = Vec::new();
    let mut at = 2usize;
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
            0xda | 0xd9 => return Ok((out, at)),
            0x01 | 0xd0..=0xd8 => {
                out.push((marker, at..at + 2));
                at += 2;
            }
            _ => {
                let len = usize::from(be_u16(b, at + 2)?);
                if len < 2 {
                    return Err(Error::Malformed("a JPEG segment length under 2".into()));
                }
                take(b, at + 2, len)?;
                out.push((marker, at..at + 2 + len));
                at += 2 + len;
            }
        }
    }
}

/// Where new APP1 segments go: after SOI and the APP0 segments that follow it.
fn after_app0(segments: &[Segment]) -> usize {
    segments.iter().take_while(|(m, _)| *m == 0xe0).last().map_or(2, |(_, r)| r.end)
}

fn into_jpeg(target: &[u8], blocks: Blocks<'_>, limits: &Limits) -> Result<Vec<u8>> {
    let scan = container::scan(target, limits)?;
    let gone: Vec<Range<usize>> = scan.spans.iter().filter(|s| replaced(s.kind)).map(|s| s.range.clone()).collect();
    let (segs, sos) = segments(target, limits)?;
    let segs: Vec<Segment> = segs.into_iter().filter(|(_, r)| !gone.iter().any(|g| g.start <= r.start && r.end <= g.end)).collect();
    let app0 = segs.iter().take_while(|(m, _)| *m == 0xe0).count();
    let run = app0 + segs.iter().skip(app0).take_while(|(m, _)| (0xe0..=0xec).contains(m)).count();

    let mut app1 = app(0xe1, &[b"Exif\0\0", blocks.exif])?;
    if let Some(packet) = blocks.xmp {
        app1.extend(app(0xe1, &[XMP_SIGNATURE, packet])?);
    }
    if let (Some(ext), Some(packet)) = (blocks.extended, blocks.xmp) {
        app1.extend(extended_segments(ext, packet)?);
    }
    let mut app13 = Vec::new();
    if let Some(iptc) = blocks.iptc {
        let resources = iptc.into_resources();
        // 65,533 payload bytes a segment, less the signature; readers join consecutive APP13s.
        for part in resources.chunks(65_533 - PHOTOSHOP_SIGNATURE.len()) {
            app13.extend(app(0xed, &[PHOTOSHOP_SIGNATURE, part])?);
        }
    }

    let mut out = Vec::with_capacity(target.len() + app1.len() + app13.len());
    out.extend_from_slice(target.get(..2).ok_or(Error::Truncated)?);
    for (i, (_, r)) in segs.iter().enumerate() {
        if i == app0 {
            out.extend_from_slice(&app1);
        }
        if i == run {
            out.extend_from_slice(&app13);
        }
        out.extend_from_slice(target.get(r.clone()).ok_or(Error::Truncated)?);
    }
    if app0 >= segs.len() {
        out.extend_from_slice(&app1);
    }
    if run >= segs.len() {
        out.extend_from_slice(&app13);
    }
    out.extend_from_slice(target.get(sos..).ok_or(Error::Truncated)?);
    Ok(out)
}

/// The extended XMP packet as APP1 chunks under the GUID the standard packet declares (the
/// packet's MD5, XMP Part 3 §1.1.3.1).
fn extended_segments(ext: &[u8], packet: &[u8]) -> Result<Vec<u8>> {
    let guid = extended_guid(ext);
    if container::jpeg::declared_guid(packet).is_none_or(|g| !g.eq_ignore_ascii_case(&guid)) {
        return Ok(Vec::new());
    }
    let full = u32::try_from(ext.len()).map_err(|_| Error::TooMany("extended XMP bytes"))?;
    // 65,533 payload bytes, less the signature, the GUID, the full length and the offset.
    let room = 65_533 - EXTENDED_XMP_SIGNATURE.len() - 32 - 8;
    let mut out = Vec::new();
    for (i, part) in ext.chunks(room).enumerate() {
        let offset = u32::try_from(i * room).map_err(|_| Error::TooMany("extended XMP bytes"))?;
        out.extend(app(0xe1, &[EXTENDED_XMP_SIGNATURE, &guid, &full.to_be_bytes(), &offset.to_be_bytes(), part])?);
    }
    Ok(out)
}

// PNG.

/// The chunks up to and including `IEND`, as ranges.
fn chunks(b: &[u8], limits: &Limits) -> Result<Vec<([u8; 4], Range<usize>)>> {
    let mut out = Vec::new();
    let mut at = 8usize;
    loop {
        if out.len() >= limits.max_segments {
            return Err(Error::TooMany("PNG chunks"));
        }
        let len = be_u32(b, at)? as usize;
        let kind = take(b, at + 4, 4)?;
        let kind = [*kind.first().unwrap_or(&0), *kind.get(1).unwrap_or(&0), *kind.get(2).unwrap_or(&0), *kind.get(3).unwrap_or(&0)];
        let end = at.checked_add(12).and_then(|e| e.checked_add(len)).ok_or(Error::Truncated)?;
        take(b, at, end - at)?;
        out.push((kind, at..end));
        if &kind == b"IEND" {
            return Ok(out);
        }
        at = end;
    }
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Result<Vec<u8>> {
    let len = u32::try_from(data.len()).ok().filter(|&l| l <= 0x7fff_ffff).ok_or(Error::TooMany("bytes in one PNG chunk"))?;
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend(len.to_be_bytes());
    out.extend(kind);
    out.extend_from_slice(data);
    out.extend(crc32(&[kind, data]).to_be_bytes());
    Ok(out)
}

/// A raw profile text: a name line, the length right-aligned in eight characters, then the bytes
/// as lowercase hex digits, 36 bytes a line.
fn profile_text(name: &str, data: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = format!("\n{name}\n{:8}\n", data.len()).into_bytes();
    out.reserve(data.len() * 2 + data.len() / 36 + 1);
    for line in data.chunks(36) {
        for &b in line {
            out.extend([HEX[usize::from(b >> 4)], HEX[usize::from(b & 0x0f)]]);
        }
        out.push(b'\n');
    }
    out
}

/// CRC-32 as PNG computes it (ISO 3309: polynomial 0xEDB88320, reflected, inverted).
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for part in parts {
        for &b in *part {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
            }
        }
    }
    !crc
}

fn into_png(target: &[u8], blocks: Blocks<'_>, limits: &Limits) -> Result<Vec<u8>> {
    let scan = container::scan(target, limits)?;
    let gone: Vec<Range<usize>> = scan.spans.iter().filter(|s| replaced(s.kind)).map(|s| s.range.clone()).collect();
    let all = chunks(target, limits)?;
    let mut new = chunk(b"eXIf", blocks.exif)?;
    if let Some(packet) = blocks.xmp {
        // The keyword, not compressed, method 0, no language, no translated keyword.
        new.extend(chunk(b"iTXt", &[XMP_KEYWORD.as_bytes(), &[0, 0, 0, 0, 0], packet].concat())?);
    }
    if let Some(iptc) = &blocks.iptc {
        let text = profile_text("iptc", iptc.bytes());
        new.extend(chunk(b"tEXt", &[b"Raw profile type iptc\0", &text[..]].concat())?);
    }
    // Before the first IDAT (or IEND), where every reader looks.
    let at = all.iter().position(|(k, _)| k == b"IDAT" || k == b"IEND").unwrap_or(all.len());
    let mut out = Vec::with_capacity(target.len() + new.len());
    out.extend_from_slice(target.get(..8).ok_or(Error::Truncated)?);
    for (i, (_, r)) in all.iter().enumerate() {
        if i == at {
            out.extend_from_slice(&new);
        }
        if !gone.contains(r) {
            out.extend_from_slice(target.get(r.clone()).ok_or(Error::Truncated)?);
        }
    }
    Ok(out)
}

// The check.

/// The carried file reads back cleanly, with orientation 1 and the new size in EXIF and XMP, and
/// no GPS when the location was removed.
fn check(bytes: &[u8], c: &Carry<'_>, limits: &Limits) -> Result<()> {
    let fail = |what: &str| Err(Error::Verify(format!("the carried {what} does not read back")));
    let scan = container::scan(bytes, limits).map_err(|e| Error::Verify(format!("the carried file does not read back: {e}")))?;
    let read = read_scan(&scan, limits);
    if let Some(p) = read.problems.iter().find(|p| replaced(p.block)) {
        return Err(Error::Verify(format!("the carried {:?} block does not read back: {}", p.block, p.error)));
    }
    let Some(block) = scan.exif.as_deref() else { return fail("EXIF") };
    let w = walk(block, limits)?;
    let uint = |kind, tag| w.get(kind, tag).and_then(|e| w.tiff.uint(e));
    if uint(IfdKind::Image, TAG_ORIENTATION) != Some(1) {
        return fail("orientation");
    }
    if uint(IfdKind::Exif, 0xa002) != Some(c.width) || uint(IfdKind::Exif, 0xa003) != Some(c.height) {
        return fail("pixel size");
    }
    if let Some(packet) = scan.xmp.as_deref() {
        if xmp::values(packet, xmp::TIFF, "Orientation", limits)?.iter().any(|v| v != "1") {
            return fail("XMP orientation");
        }
        let w = c.width.to_string();
        if xmp::values(packet, xmp::EXIF, "PixelXDimension", limits)?.iter().any(|v| *v != w) {
            return fail("XMP pixel size");
        }
    }
    if c.remove_location {
        let gps_ifd = w.ifds.iter().any(|(k, _)| *k == IfdKind::Gps) || w.get(IfdKind::Image, TAG_GPS_IFD).is_some();
        let gps_xmp = match scan.xmp.as_deref() {
            Some(p) => xmp::has_gps(p, limits)?,
            None => false,
        };
        if gps_ifd || gps_xmp || read.owned.place.is_some() {
            return Err(Error::Verify("the location was not removed".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_png_specification() {
        assert_eq!(crc32(&[b"IEND"]), 0xae42_6082);
        assert_eq!(crc32(&[b"IE", b"ND"]), 0xae42_6082);
    }

    #[test]
    fn splices_apply_in_order() {
        let out = splice(b"abcdef", vec![(4..5, b"E".to_vec()), (1..1, b"+".to_vec()), (2..3, Vec::new())]);
        assert_eq!(out, b"a+bdEf");
    }

    #[test]
    fn segments_are_capped() {
        assert_eq!(app(0xe1, &[&[0u8; 65_533]]).map(|s| s.len()), Ok(65_537));
        assert!(app(0xe1, &[&[0u8; 65_534]]).is_err());
    }
}
