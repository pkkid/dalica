//! PNG write-back (`xmp-interop.md` §4): XMP in an uncompressed `iTXt` chunk under the keyword
//! `XML:com.adobe.xmp`, the place and the date in `eXIf`, every other chunk copied byte for byte.
//!
//! - The XMP packet is merged ([`crate::xmp::merge::merge_at`], `xmp:MetadataDate` from
//!   [`WriteOptions::metadata_date`]) and written as an uncompressed `iTXt` (the
//!   form the XMP specification asks for, so the packet can be found and edited in place) where
//!   the chunk that held it stood, whatever its form was (`iTXt`, `tEXt`, `zTXt` or a
//!   "Raw profile type xmp" text).
//! - The EXIF block is edited in place or appended ([`crate::exif::edit`]): the place, the date
//!   taken and an ImageDescription that is a copy of the caption. It goes back into the chunk that
//!   held it: `eXIf`, or a "Raw profile type exif" text in the same chunk type, compression and
//!   hex layout (with its `Exif\0\0` prefix when it had one).
//! - IPTC is written only where the file already keeps it, a "Raw profile type iptc" or `8bim`
//!   text (the PNG specification has no IPTC chunk) holding a bare IIM stream (as ImageMagick
//!   writes it) or Photoshop resources; otherwise the caption and tags live in XMP. A file with
//!   more than one such profile has every copy that holds IPTC rewritten, each in its own form, so
//!   no copy keeps an old caption for a reader that picks another.
//! - A block the file lacks goes into a new chunk just before the first `IDAT`, where every reader
//!   finds it before the image data: `eXIf` first, then the XMP `iTXt`.
//! - Every chunk written gets its CRC computed again; every other chunk, and anything after `IEND`,
//!   keeps its bytes and its order. Writing the values the file already holds changes nothing.
//!
//! Provenance: written for Dalica from the PNG 3rd edition specification (chunk layout, CRC-32,
//! `eXIf`, `iTXt`, `tEXt`, `zTXt`), the XMP Specification Part 3 (XMP in `iTXt`) and the public
//! description of "Raw profile type" text chunks (a name line, a decimal length, hex digits). No
//! other writer's code was consulted.

use std::borrow::Cow;
use std::ops::Range;

use super::{Changes, WriteOptions};
use crate::container::png::{XMP_KEYWORD, inflate, raw_profile};
use crate::container::{BlockKind, Scan, Span, be_u32, take};
use crate::exif::edit::{ExifEdit, edit as edit_exif};
use crate::iptc::RESOURCE_IPTC;
use crate::iptc::read::{is_iim, resources};
use crate::iptc::write::{IptcEdit, write_resources, write_stream};
use crate::xmp::merge::merge_at;
use crate::{Error, Limits, Metadata, Result};

/// The largest EXIF structure one `eXIf` chunk holds: chunk lengths stop at 2³¹ − 1 (less room for
/// an `Exif\0\0` prefix).
pub const MAX_PNG_EXIF: usize = 0x7fff_ffff - 6;

const SIGNATURE_LEN: usize = 8;

/// The prefix some writers put before the TIFF header of an EXIF block.
const EXIF_PREFIX: &[u8] = b"Exif\0\0";

/// One chunk of the file.
struct Chunk {
    kind: [u8; 4],
    /// Length, type, data and CRC.
    range: Range<usize>,
}

/// The new bytes of `bytes` (walked as `scan`) with `changes` written.
pub(super) fn write(bytes: &[u8], scan: &Scan<'_>, _before: &Metadata, changes: &Changes, options: &WriteOptions) -> Result<Vec<u8>> {
    let limits = &options.limits;
    let (chunks, end) = walk(bytes, limits)?;
    // Chunks replaced (keyed by where the old one starts) and chunks added before the first IDAT.
    let mut replace: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut insert: Vec<Vec<u8>> = Vec::new();

    let edit = ExifEdit::of(changes);
    if !edit.is_empty() {
        let old = scan.exif.as_deref();
        if let Some(new) = edit_exif(old, &edit, MAX_PNG_EXIF, limits)?.filter(|n| Some(n.as_slice()) != old) {
            match rewritable(scan, &[BlockKind::Exif]) {
                Some(span) => replace.push((span.range.start, rewrite_block(bytes, span, &new, true, limits)?)),
                None => insert.push(chunk(b"eXIf", &new)?),
            }
        }
    }

    let edit = IptcEdit::of(changes);
    if !edit.is_empty() {
        // Every IPTC-holding profile, so no copy keeps an old caption for a reader that picks a
        // later one: the first is the one read, a later one is rewritten only when it holds IPTC.
        let profiles = scan.spans.iter().filter(|s| s.rewritable && matches!(s.kind, BlockKind::Iptc | BlockKind::Photoshop));
        for (n, span) in profiles.enumerate() {
            let old = profile_block(bytes, span, limits)?;
            let new = if is_iim(&old) {
                write_stream(Some(&old), &edit, limits)?.bytes
            } else if n == 0 || holds_iptc(&old, limits) {
                write_resources(Some(&old), &edit, limits)?.bytes
            } else {
                continue;
            };
            if new != old {
                replace.push((span.range.start, rewrite_block(bytes, span, &new, false, limits)?));
            }
        }
    }

    if let Some(merged) = merge_at(scan.xmp.as_deref(), changes, options.metadata_date.as_ref(), limits)? {
        let new = xmp_chunk(merged.bytes())?;
        match rewritable(scan, &[BlockKind::Xmp]) {
            Some(span) => replace.push((span.range.start, new)),
            None => insert.push(new),
        }
    }

    if replace.is_empty() && insert.is_empty() {
        return Ok(bytes.to_vec());
    }
    let insert_at = chunks.iter().position(|c| &c.kind == b"IDAT").unwrap_or(chunks.len().saturating_sub(1));
    let extra: usize = replace.iter().map(|(_, c)| c.len()).chain(insert.iter().map(Vec::len)).sum();
    let mut out = Vec::with_capacity(bytes.len().saturating_add(extra));
    out.extend_from_slice(bytes.get(..SIGNATURE_LEN).ok_or(Error::Truncated)?);
    for (i, c) in chunks.iter().enumerate() {
        if i == insert_at {
            insert.iter().for_each(|n| out.extend_from_slice(n));
        }
        match replace.iter().find(|(at, _)| *at == c.range.start) {
            Some((_, new)) => out.extend_from_slice(new),
            None => out.extend_from_slice(bytes.get(c.range.clone()).ok_or(Error::Truncated)?),
        }
    }
    // Anything after IEND stays as it was.
    out.extend_from_slice(bytes.get(end..).unwrap_or_default());
    Ok(out)
}

/// The span of the block of one of `kinds` that the reader used (the rewritable one).
fn rewritable<'s>(scan: &'s Scan<'_>, kinds: &[BlockKind]) -> Option<&'s Span> {
    scan.spans.iter().find(|s| s.rewritable && kinds.contains(&s.kind))
}

/// Every chunk up to and including `IEND`, and where `IEND` ends.
fn walk(b: &[u8], limits: &Limits) -> Result<(Vec<Chunk>, usize)> {
    let mut out = Vec::new();
    let mut at = SIGNATURE_LEN;
    loop {
        if out.len() >= limits.max_segments {
            return Err(Error::TooMany("PNG chunks"));
        }
        let len = be_u32(b, at)?;
        if len > 0x7fff_ffff {
            return Err(Error::Malformed("a PNG chunk length past 2³¹ − 1".into()));
        }
        let kind: [u8; 4] = take(b, at + 4, 4)?.try_into().map_err(|_| Error::Truncated)?;
        let end = at.checked_add(12).and_then(|e| e.checked_add(len as usize)).ok_or(Error::Truncated)?;
        take(b, at, end - at)?;
        out.push(Chunk { kind, range: at..end });
        if &kind == b"IEND" {
            return Ok((out, end));
        }
        at = end;
    }
}

/// A chunk of `kind` holding `data`, with its CRC.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Result<Vec<u8>> {
    let len = chunk_len(data.len())?;
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(out.get(4..).unwrap_or_default());
    out.extend_from_slice(&crc.to_be_bytes());
    Ok(out)
}

/// A chunk's length field: at most 2³¹ − 1 (PNG 3rd edition §5.3); past it the write fails.
fn chunk_len(len: usize) -> Result<u32> {
    u32::try_from(len).ok().filter(|&l| l <= 0x7fff_ffff).ok_or(Error::TooMany("bytes in one PNG chunk"))
}

/// The XMP packet as an uncompressed `iTXt` chunk with no language and no translated keyword.
fn xmp_chunk(packet: &[u8]) -> Result<Vec<u8>> {
    let mut data = Vec::with_capacity(XMP_KEYWORD.len() + 5 + packet.len());
    data.extend_from_slice(XMP_KEYWORD.as_bytes());
    // The keyword's end, not compressed, method 0, an empty language, an empty translated keyword.
    data.extend_from_slice(&[0, 0, 0, 0, 0]);
    data.extend_from_slice(packet);
    chunk(b"iTXt", &data)
}

/// The chunk at `span` holding `block` (EXIF when `exif`, else IPTC or Photoshop resources) in
/// place of what it held, in the same form: `eXIf`, or a raw profile text chunk.
fn rewrite_block(bytes: &[u8], span: &Span, block: &[u8], exif: bool, limits: &Limits) -> Result<Vec<u8>> {
    let old = bytes.get(span.range.clone()).ok_or(Error::Truncated)?;
    let kind: [u8; 4] = old.get(4..8).and_then(|k| k.try_into().ok()).ok_or(Error::Truncated)?;
    let data = old.get(8..old.len().saturating_sub(4)).ok_or(Error::Truncated)?;
    if &kind == b"eXIf" {
        let prefix = if data.starts_with(EXIF_PREFIX) { EXIF_PREFIX } else { &[] };
        return chunk(b"eXIf", &[prefix, block].concat());
    }
    let text = TextChunk::parse(&kind, data, limits)?;
    let decoded = raw_profile(&text.text, limits)?;
    let prefix = if exif && decoded.starts_with(EXIF_PREFIX) { EXIF_PREFIX } else { &[] };
    let name = profile_name(&text.text).unwrap_or("generic");
    chunk(&kind, &text.with_text(&profile_text(name, &[prefix, block].concat())))
}

/// The block a raw profile text chunk at `span` holds, decoded.
fn profile_block(bytes: &[u8], span: &Span, limits: &Limits) -> Result<Vec<u8>> {
    let old = bytes.get(span.range.clone()).ok_or(Error::Truncated)?;
    let kind: [u8; 4] = old.get(4..8).and_then(|k| k.try_into().ok()).ok_or(Error::Truncated)?;
    let data = old.get(8..old.len().saturating_sub(4)).ok_or(Error::Truncated)?;
    raw_profile(&TextChunk::parse(&kind, data, limits)?.text, limits)
}

/// Whether Photoshop image resources hold an IPTC stream.
fn holds_iptc(block: &[u8], limits: &Limits) -> bool {
    resources(block, limits).is_ok_and(|r| r.iter().any(|r| r.id == RESOURCE_IPTC))
}

/// A text chunk split into what stays (keyword, flags, language, translated keyword) and its text.
struct TextChunk<'a> {
    /// Everything before the text.
    head: &'a [u8],
    compressed: bool,
    text: Cow<'a, [u8]>,
}

impl<'a> TextChunk<'a> {
    fn parse(kind: &[u8; 4], data: &'a [u8], limits: &Limits) -> Result<TextChunk<'a>> {
        let bad = || Error::Malformed("a PNG text chunk that cannot be rewritten".to_owned());
        let nul = data.iter().position(|&c| c == 0).ok_or_else(bad)?;
        let (head_len, compressed) = match kind {
            b"tEXt" => (nul + 1, false),
            b"zTXt" => (nul + 2, true),
            b"iTXt" => {
                let flag = *data.get(nul + 1).ok_or_else(bad)?;
                let lang_at = nul + 3;
                let lang = data.get(lang_at..).ok_or_else(bad)?.iter().position(|&c| c == 0).ok_or_else(bad)?;
                let translated_at = lang_at + lang + 1;
                let translated = data.get(translated_at..).ok_or_else(bad)?.iter().position(|&c| c == 0).ok_or_else(bad)?;
                (translated_at + translated + 1, flag == 1)
            }
            _ => return Err(bad()),
        };
        let head = data.get(..head_len).ok_or_else(bad)?;
        let body = data.get(head_len..).unwrap_or_default();
        let text = if compressed { Cow::Owned(inflate(body, limits.max_inflate)?) } else { Cow::Borrowed(body) };
        Ok(TextChunk { head, compressed, text })
    }

    /// The chunk's data with `text` in place of the old text, compressed as the old one was.
    fn with_text(&self, text: &[u8]) -> Vec<u8> {
        let body: Cow<'_, [u8]> = if self.compressed { Cow::Owned(fdeflate::compress_to_vec(text)) } else { Cow::Borrowed(text) };
        [self.head, &body].concat()
    }
}

/// The name line of a raw profile text (`exif`, `iptc`, `8bim` …).
fn profile_name(text: &[u8]) -> Option<&str> {
    let start = text.iter().position(|c| !c.is_ascii_whitespace())?;
    let rest = text.get(start..)?;
    let line = rest.get(..rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len()))?;
    std::str::from_utf8(line).ok().map(str::trim).filter(|n| !n.is_empty() && n.len() <= 79)
}

/// A raw profile text: a name line, the length right-aligned in eight characters, then the bytes as
/// lowercase hex digits, 36 bytes a line.
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
fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut n = 0;
        while n < 256 {
            let mut c = n as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                k += 1;
            }
            table[n] = c;
            n += 1;
        }
        table
    };
    !bytes.iter().fold(0xffff_ffffu32, |crc, &b| TABLE[usize::from((crc as u8) ^ b)] ^ (crc >> 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_png_specification() {
        // The CRC every IEND chunk carries, and the standard check value of "123456789".
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn raw_profile_text_round_trips() {
        let data: Vec<u8> = (0..=255u8).collect();
        let text = profile_text("exif", &data);
        assert_eq!(raw_profile(&text, &Limits::default()), Ok(data));
        assert_eq!(profile_name(&text), Some("exif"));
        assert_eq!(profile_name(b"   \n  8bim \n 4\n"), Some("8bim"));
        assert_eq!(profile_name(b"   "), None);
    }

    #[test]
    fn chunk_lengths_stop_at_two_to_the_31() {
        assert_eq!(chunk_len(0x7fff_ffff), Ok(0x7fff_ffff));
        assert_eq!(chunk_len(0x8000_0000), Err(Error::TooMany("bytes in one PNG chunk")));
        assert_eq!(chunk_len(usize::MAX), Err(Error::TooMany("bytes in one PNG chunk")));
    }
}
