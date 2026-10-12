//! JPEG: the segments before the first scan (ITU-T T.81 B.1). APP1 EXIF and XMP, extended-XMP
//! chunks (reassembled by GUID and bounded), APP2 ICC and MPF, APP13 Photoshop resources and
//! comments. The scan data and everything after it are image data.

use std::borrow::Cow;

use super::{BlockKind, Container, Scan, Text, be_u16, be_u32, take, text_utf8_or_latin1};
use crate::{Error, Limits, Problem, Result};

/// APP1 identifier of the standard XMP packet (XMP Part 3, 1.1.3).
pub const XMP_SIGNATURE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// APP1 identifier of an extended-XMP chunk (XMP Part 3, 1.1.3.1).
pub const EXTENDED_XMP_SIGNATURE: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
/// APP13 identifier of Photoshop image resources.
pub const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";
/// APP2 identifier of an ICC profile chunk.
pub const ICC_SIGNATURE: &[u8] = b"ICC_PROFILE\0";
/// APP2 identifier of the Multi-Picture Format index.
pub const MPF_SIGNATURE: &[u8] = b"MPF\0";

const APP1: u8 = 0xe1;
const APP2: u8 = 0xe2;
const APP13: u8 = 0xed;
const COM: u8 = 0xfe;
const SOS: u8 = 0xda;
const EOI: u8 = 0xd9;

/// One extended-XMP chunk: the GUID, the full length it declares, its offset and its data.
struct Chunk<'a> {
    guid: &'a [u8],
    full: u32,
    offset: u32,
    data: &'a [u8],
}

pub(crate) fn scan<'a>(b: &'a [u8], limits: &Limits) -> Result<Scan<'a>> {
    let mut out = Scan::new(Container::Jpeg);
    let mut chunks: Vec<Chunk<'a>> = Vec::new();
    let mut photoshop: Vec<&'a [u8]> = Vec::new();
    let mut at = 2usize;
    let mut segments = 0usize;
    loop {
        segments += 1;
        if segments > limits.max_segments {
            return Err(Error::TooMany("JPEG segments"));
        }
        if *take(b, at, 1)?.first().unwrap_or(&0) != 0xff {
            return Err(Error::Malformed("expected a JPEG marker".into()));
        }
        // Fill bytes: any number of 0xFF before the marker code.
        while take(b, at + 1, 1)?.first() == Some(&0xff) {
            at += 1;
        }
        let start = at;
        let marker = *take(b, at + 1, 1)?.first().unwrap_or(&0);
        match marker {
            SOS | EOI => break,
            0x01 | 0xd0..=0xd8 => {
                at += 2;
                continue;
            }
            _ => {}
        }
        let len = usize::from(be_u16(b, at + 2)?);
        if len < 2 {
            return Err(Error::Malformed("a JPEG segment shorter than its length field".into()));
        }
        let payload = take(b, at + 4, len - 2)?;
        let end = at + 2 + len;
        let range = start..end;
        match marker {
            APP1 if payload.starts_with(b"Exif\0") => {
                let first = out.exif.is_none();
                if first {
                    out.exif = Some(Cow::Borrowed(payload.get(6..).unwrap_or_default()));
                }
                out.span(BlockKind::Exif, range, first);
            }
            APP1 if payload.starts_with(XMP_SIGNATURE) => {
                let first = out.xmp.is_none();
                if first {
                    out.xmp = Some(Cow::Borrowed(payload.get(XMP_SIGNATURE.len()..).unwrap_or_default()));
                }
                out.span(BlockKind::Xmp, range, first);
            }
            APP1 if payload.starts_with(EXTENDED_XMP_SIGNATURE) => {
                let p = payload.get(EXTENDED_XMP_SIGNATURE.len()..).unwrap_or_default();
                match (p.get(..32), be_u32(p, 32), be_u32(p, 36), p.get(40..)) {
                    (Some(guid), Ok(full), Ok(offset), Some(data)) => chunks.push(Chunk { guid, full, offset, data }),
                    _ => out
                        .problems
                        .push(Problem { block: BlockKind::ExtendedXmp, error: Error::Malformed("an extended XMP chunk header is cut short".into()) }),
                }
                out.span(BlockKind::ExtendedXmp, range, false);
            }
            APP2 if payload.starts_with(ICC_SIGNATURE) => out.span(BlockKind::Icc, range, false),
            APP2 if payload.starts_with(MPF_SIGNATURE) => out.span(BlockKind::Mpf, range, false),
            APP13 if payload.starts_with(PHOTOSHOP_SIGNATURE) => {
                photoshop.push(payload.get(PHOTOSHOP_SIGNATURE.len()..).unwrap_or_default());
                out.span(BlockKind::Photoshop, range, true);
            }
            COM => {
                let text = text_utf8_or_latin1(payload.get(..payload.len().min(limits.max_string)).unwrap_or_default());
                out.texts.push(Text { keyword: "Comment".into(), value: text });
                out.span(BlockKind::Comment, range, false);
            }
            _ => {}
        }
        at = end;
    }
    // Photoshop resources split over several APP13 segments are one block.
    out.photoshop = match photoshop.as_slice() {
        [] => None,
        [one] => Some(Cow::Borrowed(*one)),
        many => Some(Cow::Owned(many.concat())),
    };
    if !chunks.is_empty() {
        match reassemble(out.xmp.as_deref(), &chunks, limits) {
            Ok(packet) => out.extended_xmp = Some(packet),
            Err(error) => out.problems.push(Problem { block: BlockKind::ExtendedXmp, error }),
        }
    }
    Ok(out)
}

/// The extended packet from its chunks (XMP Part 3, 1.1.3.1): only chunks whose GUID is the one
/// the standard packet declares in `xmpNote:HasExtendedXMP`, all declaring the same full length,
/// each inside it, together covering it with no gap, and the standard and extended packets
/// together within `max_xmp`.
fn reassemble(main: Option<&[u8]>, chunks: &[Chunk<'_>], limits: &Limits) -> Result<Vec<u8>> {
    let guid = main.and_then(declared_guid).ok_or_else(|| Error::Malformed("extended XMP that the main packet does not declare".into()))?;
    let mut mine: Vec<&Chunk<'_>> = chunks.iter().filter(|c| c.guid.eq_ignore_ascii_case(guid)).collect();
    let full = mine.first().map(|c| c.full).ok_or_else(|| Error::Malformed("no extended XMP chunk has the declared GUID".into()))?;
    if mine.iter().any(|c| c.full != full) {
        return Err(Error::Malformed("extended XMP chunks disagree on the packet length".into()));
    }
    let full = usize::try_from(full).map_err(|_| Error::TooMany("XMP bytes"))?;
    if full.checked_add(main.map_or(0, <[u8]>::len)).is_none_or(|total| total > limits.max_xmp) {
        return Err(Error::TooMany("XMP bytes"));
    }
    mine.sort_by_key(|c| c.offset);
    let mut covered = 0usize;
    for c in &mine {
        let start = usize::try_from(c.offset).map_err(|_| Error::OutOfRange)?;
        let end = start.checked_add(c.data.len()).ok_or(Error::OutOfRange)?;
        if end > full {
            return Err(Error::OutOfRange);
        }
        if start > covered {
            return Err(Error::Malformed("extended XMP has a gap".into()));
        }
        covered = covered.max(end);
    }
    if covered != full {
        return Err(Error::Malformed("extended XMP is incomplete".into()));
    }
    // Every byte is covered by chunk data, so this allocation is no larger than what the file holds.
    let mut out = vec![0u8; full];
    for c in &mine {
        let start = c.offset as usize;
        if let Some(dst) = out.get_mut(start..start + c.data.len()) {
            dst.copy_from_slice(c.data);
        }
    }
    Ok(out)
}

/// The 32-digit GUID of `xmpNote:HasExtendedXMP` in a standard packet, in attribute
/// (`xmpNote:HasExtendedXMP="…"`) or element (`<xmpNote:HasExtendedXMP>…<`) form.
pub fn declared_guid(packet: &[u8]) -> Option<&[u8]> {
    const NAME: &[u8] = b"HasExtendedXMP";
    let at = packet.windows(NAME.len()).position(|w| w == NAME)? + NAME.len();
    let rest = packet.get(at..)?;
    // Past `="`, `='` or `>`, with optional white space.
    let skip = rest.iter().position(|&c| !matches!(c, b'=' | b'"' | b'\'' | b'>' | b' ' | b'\t' | b'\r' | b'\n'))?;
    let guid = rest.get(skip..skip + 32)?;
    guid.iter().all(u8::is_ascii_hexdigit).then_some(guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_guids() {
        let g = b"0123456789ABCDEF0123456789abcdef";
        let attr = [&b"<x xmpNote:HasExtendedXMP=\""[..], g, b"\"/>"].concat();
        let elem = [&b"<xmpNote:HasExtendedXMP>"[..], g, b"</xmpNote:HasExtendedXMP>"].concat();
        assert_eq!(declared_guid(&attr), Some(&g[..]));
        assert_eq!(declared_guid(&elem), Some(&g[..]));
        assert_eq!(declared_guid(b"xmpNote:HasExtendedXMP=\"short\""), None);
        assert_eq!(declared_guid(b"nothing here"), None);
        assert_eq!(declared_guid(b"HasExtendedXMP"), None);
    }
}
