//! Finding the metadata blocks inside a file: one walker per container (`xmp-interop.md` §4).
//!
//! Each walker reports where every metadata block lies ([`Span`]: the whole segment or chunk,
//! headers included) and hands out the blocks' payloads ([`Scan`]). Writers rewrite only the spans
//! marked `rewritable`; the verification of every write compares the bytes outside those spans
//! before and after ([`payload`]).
//!
//! Provenance: written for Dalica from public descriptions only: ITU-T T.81 (JPEG marker
//! structure), the XMP Specification Part 3 (APP1 standard and extended XMP, PNG `iTXt`, WebP and
//! TIFF tag 700), the Exif 2.32 specification (APP1, tag 33723 IPTC in TIFF), the ICC
//! specification (APP2 `ICC_PROFILE`), CIPA DC-007 (APP2 MPF), the Photoshop file format
//! description of image resource blocks (APP13), the PNG 3rd edition specification (`eXIf`,
//! `iTXt`, `tEXt`, `zTXt`, `iCCP`) and the WebP RIFF container specification. The "Raw profile
//! type" text chunks are read from their public description (a name line, a decimal length and hex
//! digits). No other reader's code was consulted.

pub mod jpeg;
pub mod png;
pub mod tiff;
pub mod webp;

use std::borrow::Cow;
use std::ops::Range;

use crate::{Error, Limits, Problem, Result};

/// The containers whose metadata Dalica reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Container {
    Jpeg,
    Png,
    WebP,
    /// TIFF, and the TIFF-structured raws (read only).
    Tiff,
}

impl Container {
    /// The container from the first bytes, never from a file name.
    pub fn detect(bytes: &[u8]) -> Option<Container> {
        match bytes {
            [0xff, 0xd8, 0xff, ..] => Some(Container::Jpeg),
            [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', ..] => Some(Container::Png),
            [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some(Container::WebP),
            [b'I', b'I', b'*', 0, ..] | [b'M', b'M', 0, b'*', ..] => Some(Container::Tiff),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Container::Jpeg => "JPEG",
            Container::Png => "PNG",
            Container::WebP => "WebP",
            Container::Tiff => "TIFF",
        }
    }
}

/// What a metadata block holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    /// A TIFF-structured EXIF block (JPEG APP1, PNG `eXIf`, WebP `EXIF`), or a TIFF file's EXIF
    /// and GPS IFDs.
    Exif,
    /// The standard XMP packet.
    Xmp,
    /// A chunk of extended XMP (JPEG only; never written).
    ExtendedXmp,
    /// Photoshop image resources (JPEG APP13, TIFF tag 34377), where IPTC lives as resource 0x0404.
    Photoshop,
    /// A bare IPTC-IIM stream (TIFF tag 33723, a PNG raw profile).
    Iptc,
    /// An ICC profile (JPEG APP2, PNG `iCCP`, WebP `ICCP`, TIFF tag 34675).
    Icc,
    /// The Multi-Picture Format index (JPEG APP2 `MPF`).
    Mpf,
    /// Another PNG text chunk.
    Text,
    /// A JPEG comment (COM).
    Comment,
    /// Container structure a metadata write may change: the WebP RIFF header and `VP8X` chunk,
    /// the TIFF header's IFD0 pointer and IFD0 itself.
    Structure,
}

/// Where one metadata block lies in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub kind: BlockKind,
    /// The whole segment or chunk: marker or chunk header, payload, CRC and padding.
    pub range: Range<usize>,
    /// Whether a metadata write may rewrite it. Everything else must survive byte for byte.
    pub rewritable: bool,
}

/// A text chunk or comment that is not itself a metadata block, kept for the listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub keyword: String,
    pub value: String,
}

/// The metadata blocks of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scan<'a> {
    pub container: Container,
    /// Every metadata block, in file order.
    pub spans: Vec<Span>,
    /// The TIFF-structured EXIF block, from its `II`/`MM` header on (a TIFF file is its own).
    pub exif: Option<Cow<'a, [u8]>>,
    /// The standard XMP packet.
    pub xmp: Option<Cow<'a, [u8]>>,
    /// The extended XMP packet, reassembled and checked (JPEG only).
    pub extended_xmp: Option<Vec<u8>>,
    /// Photoshop image resources (`8BIM` blocks).
    pub photoshop: Option<Cow<'a, [u8]>>,
    /// A bare IPTC-IIM stream.
    pub iptc: Option<Cow<'a, [u8]>>,
    /// Text chunks and comments.
    pub texts: Vec<Text>,
    /// Blocks found but refused (an extended XMP that does not match, a text chunk past the
    /// inflate cap …). The walk itself succeeded.
    pub problems: Vec<Problem>,
}

impl<'a> Scan<'a> {
    fn new(container: Container) -> Scan<'a> {
        Scan {
            container,
            spans: Vec::new(),
            exif: None,
            xmp: None,
            extended_xmp: None,
            photoshop: None,
            iptc: None,
            texts: Vec::new(),
            problems: Vec::new(),
        }
    }

    /// The IPTC-IIM stream: the bare one, else resource 0x0404 of the Photoshop resources.
    pub fn iptc_stream(&self, limits: &Limits) -> Result<Option<&[u8]>> {
        if let Some(iptc) = self.iptc.as_deref() {
            return Ok(Some(iptc));
        }
        let Some(ps) = self.photoshop.as_deref() else { return Ok(None) };
        let resources = crate::iptc::read::resources(ps, limits)?;
        Ok(resources.into_iter().find(|r| r.id == crate::iptc::RESOURCE_IPTC).map(|r| r.data))
    }

    fn span(&mut self, kind: BlockKind, range: Range<usize>, rewritable: bool) {
        self.spans.push(Span { kind, range, rewritable });
    }
}

/// Walks a file's container and collects its metadata blocks. Strict: a segment or chunk that
/// runs past the end, a count past the limits or a broken structure is an `Err`.
pub fn scan<'a>(bytes: &'a [u8], limits: &Limits) -> Result<Scan<'a>> {
    match Container::detect(bytes).ok_or(Error::Unknown)? {
        Container::Jpeg => jpeg::scan(bytes, limits),
        Container::Png => png::scan(bytes, limits),
        Container::WebP => webp::scan(bytes, limits),
        Container::Tiff => tiff::scan(bytes, limits),
    }
}

/// The bytes outside the rewritable spans, in file order: what a metadata write must leave
/// unchanged (`xmp-interop.md` §1 principle 2).
pub fn payload<'b>(bytes: &'b [u8], spans: &[Span]) -> Vec<&'b [u8]> {
    outside(bytes.len(), spans).into_iter().filter_map(|r| bytes.get(r)).collect()
}

/// The ranges of a `len`-byte file outside its rewritable spans, in order, none empty.
pub fn outside(len: usize, spans: &[Span]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = spans.iter().filter(|s| s.rewritable).map(|s| s.range.clone()).collect();
    ranges.sort_by_key(|r| r.start);
    let mut out = Vec::new();
    let mut at = 0usize;
    for r in ranges {
        let start = r.start.min(len);
        if start > at {
            out.push(at..start);
        }
        at = at.max(r.end.min(len));
    }
    if len > at {
        out.push(at..len);
    }
    out
}

/// A 64-bit FNV-1a digest of [`payload`], for callers that keep it across a write.
pub fn payload_digest(bytes: &[u8], spans: &[Span]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for piece in payload(bytes, spans) {
        for &b in piece {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Whether two chunk sequences hold the same bytes, however they are split.
pub(crate) fn same_bytes(a: &[&[u8]], b: &[&[u8]]) -> bool {
    let (mut ia, mut ib) = (a.iter().flat_map(|s| s.iter()), b.iter().flat_map(|s| s.iter()));
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x == y => {}
            _ => return false,
        }
    }
}

/// `len` bytes at `at`, or `Truncated`.
pub(crate) fn take(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    let end = at.checked_add(len).ok_or(Error::Truncated)?;
    bytes.get(at..end).ok_or(Error::Truncated)
}

pub(crate) fn be_u16(bytes: &[u8], at: usize) -> Result<u16> {
    let b = take(bytes, at, 2)?;
    Ok(u16::from_be_bytes([*b.first().unwrap_or(&0), *b.get(1).unwrap_or(&0)]))
}

pub(crate) fn be_u32(bytes: &[u8], at: usize) -> Result<u32> {
    let b = take(bytes, at, 4)?;
    Ok(u32::from_be_bytes([*b.first().unwrap_or(&0), *b.get(1).unwrap_or(&0), *b.get(2).unwrap_or(&0), *b.get(3).unwrap_or(&0)]))
}

pub(crate) fn le_u32(bytes: &[u8], at: usize) -> Result<u32> {
    let b = take(bytes, at, 4)?;
    Ok(u32::from_le_bytes([*b.first().unwrap_or(&0), *b.get(1).unwrap_or(&0), *b.get(2).unwrap_or(&0), *b.get(3).unwrap_or(&0)]))
}

/// An EXIF payload without the `Exif\0\0` prefix some writers put before the TIFF header.
pub(crate) fn strip_exif_prefix(b: &[u8]) -> &[u8] {
    match b {
        [b'E', b'x', b'i', b'f', 0, _, rest @ ..] => rest,
        _ => b,
    }
}

/// Text in UTF-8 when it is valid, else Latin-1.
pub(crate) fn text_utf8_or_latin1(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => latin1(b),
    }
}

pub(crate) fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        assert_eq!(Container::detect(b"\xff\xd8\xff\xe0"), Some(Container::Jpeg));
        assert_eq!(Container::detect(b"\x89PNG\r\n\x1a\n"), Some(Container::Png));
        assert_eq!(Container::detect(b"RIFF\0\0\0\0WEBP"), Some(Container::WebP));
        assert_eq!(Container::detect(b"II*\0"), Some(Container::Tiff));
        assert_eq!(Container::detect(b"MM\0*"), Some(Container::Tiff));
        assert_eq!(Container::detect(b"GIF89a"), None);
        assert_eq!(Container::detect(b""), None);
        assert_eq!(scan(b"GIF89a", &Limits::default()), Err(Error::Unknown));
    }

    #[test]
    fn payload_skips_rewritable_spans_only() {
        let bytes = b"aaBBccDDee";
        let spans = [
            Span { kind: BlockKind::Xmp, range: 6..8, rewritable: true },
            Span { kind: BlockKind::Icc, range: 4..6, rewritable: false },
            Span { kind: BlockKind::Exif, range: 2..4, rewritable: true },
        ];
        let p = payload(bytes, &spans);
        assert_eq!(p.concat(), b"aaccee");
        assert!(same_bytes(&p, &[b"aac", b"cee"]));
        assert!(!same_bytes(&p, &[b"aacce"]));
        assert_eq!(
            payload_digest(bytes, &spans),
            payload_digest(
                b"aaXXXXccYee",
                &[Span { kind: BlockKind::Exif, range: 2..6, rewritable: true }, Span { kind: BlockKind::Xmp, range: 8..9, rewritable: true }]
            )
        );
        assert_ne!(payload_digest(bytes, &spans), payload_digest(bytes, &[]));
        // Spans past the end are clipped.
        assert_eq!(payload(b"abc", &[Span { kind: BlockKind::Exif, range: 1..10, rewritable: true }]).concat(), b"a");
    }
}
