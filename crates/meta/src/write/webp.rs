//! WebP write-back (`xmp-interop.md` §4): the `EXIF` and `XMP ` chunks of the extended format, a
//! simple-format file promoted to `VP8X`, the RIFF size fixed, every other chunk byte for byte.
//!
//! - The XMP packet is merged ([`crate::xmp::merge::merge_at`]) and the EXIF block edited in place or
//!   appended ([`crate::exif::edit`]: the place, the date taken and an ImageDescription that is a
//!   copy of the caption); each goes back into the chunk the reader took it from (an `Exif\0\0`
//!   prefix kept when the chunk had one). WebP has no IPTC chunk: caption and tags live in XMP.
//! - A chunk the file lacks goes where the container specification orders it, after the image
//!   data: a new `EXIF` just before the existing `XMP ` chunk, otherwise at the end; a new `XMP `
//!   at the end.
//! - The `VP8X` flags gain the EXIF and XMP bits for the chunks the file now has (no flag is ever
//!   cleared). A simple-format file (one `VP8 ` or `VP8L` chunk, no `VP8X`) gets a `VP8X` chunk
//!   first, with the canvas size read from the image chunk's header and the alpha flag from a
//!   lossless image's header; the image chunk itself is unchanged.
//! - The RIFF size is set for the new body; bytes after the RIFF body stay as they were.
//!   Writing the values the file already holds changes nothing.
//!
//! Provenance: written for Dalica from the public WebP container specification (RIFF layout,
//! chunk padding, `VP8X` flags and canvas size, chunk order), the VP8 key frame header of RFC 6386
//! §9.1 and the lossless bitstream header of RFC 9649 §3.2 (the canvas size and the alpha hint).
//! No other writer's code was consulted.

use std::borrow::Cow;
use std::ops::Range;

use super::{Changes, WriteOptions};
use crate::container::{le_u32, take};
use crate::exif::edit::{ExifEdit, edit as edit_exif};
use crate::xmp::merge::merge_at;
use crate::{Error, Limits, Metadata, Result, Scan};

/// The largest EXIF structure one `EXIF` chunk holds: chunk sizes are 32-bit, and the whole RIFF
/// body must fit them too.
pub const MAX_WEBP_EXIF: usize = 0xffff_fff0;

const HEADER: usize = 12;
/// The `VP8X` flag bits (WebP container specification, "Extended File Format").
const FLAG_ALPHA: u8 = 0x10;
const FLAG_EXIF: u8 = 0x08;
const FLAG_XMP: u8 = 0x04;
const EXIF_PREFIX: &[u8] = b"Exif\0\0";

/// One RIFF chunk.
struct Chunk {
    kind: [u8; 4],
    /// Header, data and the padding byte when the file has it.
    range: Range<usize>,
    data: Range<usize>,
}

/// The new bytes of `bytes` (walked as `scan`) with `changes` written.
pub(super) fn write(bytes: &[u8], scan: &Scan<'_>, _before: &Metadata, changes: &Changes, options: &WriteOptions) -> Result<Vec<u8>> {
    let limits = &options.limits;
    let (chunks, end) = walk(bytes, limits)?;
    let first = |kind: &[u8; 4]| chunks.iter().position(|c| &c.kind == kind);
    let (old_exif, old_xmp) = (first(b"EXIF"), first(b"XMP "));

    let edit = ExifEdit::of(changes);
    let exif = if edit.is_empty() {
        None
    } else {
        let old = scan.exif.as_deref();
        edit_exif(old, &edit, MAX_WEBP_EXIF, limits)?.filter(|n| Some(n.as_slice()) != old)
    };
    let exif = match exif {
        Some(block) => {
            let prefixed = old_exif.and_then(|i| chunks.get(i)).and_then(|c| bytes.get(c.data.clone())).is_some_and(|d| d.starts_with(EXIF_PREFIX));
            Some(chunk(b"EXIF", &[if prefixed { EXIF_PREFIX } else { &[] }, &block].concat())?)
        }
        None => None,
    };
    let xmp = match merge_at(scan.xmp.as_deref(), changes, options.metadata_date.as_ref(), limits)? {
        Some(merged) => Some(chunk(b"XMP ", merged.bytes())?),
        None => None,
    };
    if exif.is_none() && xmp.is_none() {
        return Ok(bytes.to_vec());
    }

    let mut body: Vec<Cow<'_, [u8]>> = Vec::with_capacity(chunks.len() + 3);
    let vp8x = first(b"VP8X");
    if vp8x.is_none() {
        body.push(Cow::Owned(promoted(bytes, &chunks)?));
    }
    for (i, c) in chunks.iter().enumerate() {
        if Some(i) == old_exif
            && let Some(e) = &exif
        {
            body.push(Cow::Borrowed(e));
            continue;
        }
        if Some(i) == old_xmp {
            if old_exif.is_none()
                && let Some(e) = &exif
            {
                body.push(Cow::Borrowed(e));
            }
            if let Some(x) = &xmp {
                body.push(Cow::Borrowed(x));
                continue;
            }
        }
        let old = bytes.get(c.range.clone()).ok_or(Error::Truncated)?;
        if old.len() % 2 == 1 {
            // The last chunk's missing padding byte, needed now that a chunk may follow it.
            body.push(Cow::Owned([old, &[0]].concat()));
        } else {
            body.push(Cow::Borrowed(old));
        }
    }
    if old_exif.is_none()
        && old_xmp.is_none()
        && let Some(e) = &exif
    {
        body.push(Cow::Borrowed(e));
    }
    if old_xmp.is_none()
        && let Some(x) = &xmp
    {
        body.push(Cow::Borrowed(x));
    }

    // The flags for the chunks the file now has, in the VP8X chunk (the first one, now always).
    let has = |kind: &[u8; 4]| body.iter().any(|c| c.get(..4) == Some(&kind[..]));
    let flags = if has(b"EXIF") { FLAG_EXIF } else { 0 } | if has(b"XMP ") { FLAG_XMP } else { 0 };
    let at = body.iter().position(|c| c.get(..4) == Some(&b"VP8X"[..])).ok_or_else(|| Error::Malformed("no VP8X chunk".to_owned()))?;
    if let Some(c) = body.get_mut(at) {
        let mut new = c.to_vec();
        let byte = new.get_mut(8).ok_or_else(|| Error::Malformed("a VP8X chunk without its flags".to_owned()))?;
        *byte |= flags;
        *c = Cow::Owned(new);
    }

    let size = riff_size(body.iter().map(|c| c.len()).sum::<usize>())?;
    let tail = bytes.get(end..).unwrap_or_default();
    let mut out = Vec::with_capacity(size as usize + 8 + tail.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(b"WEBP");
    body.iter().for_each(|c| out.extend_from_slice(c));
    out.extend_from_slice(tail);
    Ok(out)
}

/// Every chunk of the RIFF body, and where the body ends.
fn walk(b: &[u8], limits: &Limits) -> Result<(Vec<Chunk>, usize)> {
    let riff = le_u32(b, 4)? as usize;
    let end = riff.checked_add(8).ok_or(Error::Truncated)?;
    if end > b.len() {
        return Err(Error::Truncated);
    }
    if end < HEADER {
        return Err(Error::Malformed("a RIFF size smaller than its header".into()));
    }
    let body = b.get(..end).unwrap_or_default();
    let mut out = Vec::new();
    let mut at = HEADER;
    while at < end {
        if out.len() >= limits.max_segments {
            return Err(Error::TooMany("WebP chunks"));
        }
        let kind: [u8; 4] = take(body, at, 4)?.try_into().map_err(|_| Error::Truncated)?;
        let len = le_u32(body, at + 4)? as usize;
        take(body, at + 8, len)?;
        let data = at + 8..at + 8 + len;
        // Odd sizes are padded to even; a missing pad byte at the very end is tolerated.
        let padded = (data.end + (len & 1)).min(end);
        out.push(Chunk { kind, range: at..padded, data });
        at = padded;
    }
    Ok((out, end))
}

/// The RIFF size field for a body of `chunks` bytes (plus the `WEBP` form type): 32 bits, and
/// even; past it the write fails.
fn riff_size(chunks: usize) -> Result<u32> {
    chunks.checked_add(4).and_then(|s| u32::try_from(s).ok()).filter(|&s| s < u32::MAX).ok_or(Error::TooMany("bytes in one RIFF file"))
}

/// A RIFF chunk of `kind` holding `data`, padded to an even size.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Result<Vec<u8>> {
    let len = u32::try_from(data.len()).ok().filter(|&l| l < u32::MAX).ok_or(Error::TooMany("bytes in one WebP chunk"))?;
    let mut out = Vec::with_capacity(data.len() + 9);
    out.extend_from_slice(kind);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    Ok(out)
}

/// The `VP8X` chunk a simple-format file gets: no flags yet (the caller sets EXIF and XMP), the
/// alpha flag from a lossless image's header, and the canvas size of its image chunk.
fn promoted(bytes: &[u8], chunks: &[Chunk]) -> Result<Vec<u8>> {
    let image = chunks.first().ok_or_else(|| Error::Malformed("a WebP file without chunks".to_owned()))?;
    let data = bytes.get(image.data.clone()).ok_or(Error::Truncated)?;
    let (width, height, alpha) = match &image.kind {
        b"VP8 " => vp8_size(data)?,
        b"VP8L" => vp8l_size(data)?,
        _ => return Err(Error::Malformed("a simple-format WebP whose first chunk is not an image".to_owned())),
    };
    let mut v = [0u8; 10];
    v[0] = if alpha { FLAG_ALPHA } else { 0 };
    v[4..7].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
    v[7..10].copy_from_slice(&(height - 1).to_le_bytes()[..3]);
    chunk(b"VP8X", &v)
}

/// The size of a lossy key frame (RFC 6386 §9.1): a 3-byte frame tag, the start code
/// `9d 01 2a`, then 14-bit width and height (the top two bits are scaling).
fn vp8_size(d: &[u8]) -> Result<(u32, u32, bool)> {
    let bad = || Error::Malformed("a VP8 chunk that is not a key frame".to_owned());
    let tag = *d.first().ok_or(Error::Truncated)?;
    if tag & 1 != 0 || d.get(3..6) != Some(&[0x9d, 0x01, 0x2a][..]) {
        return Err(bad());
    }
    let w = take(d, 6, 2)?;
    let h = take(d, 8, 2)?;
    let width = u32::from(u16::from_le_bytes([*w.first().unwrap_or(&0), *w.get(1).unwrap_or(&0)]) & 0x3fff);
    let height = u32::from(u16::from_le_bytes([*h.first().unwrap_or(&0), *h.get(1).unwrap_or(&0)]) & 0x3fff);
    if width == 0 || height == 0 {
        return Err(bad());
    }
    Ok((width, height, false))
}

/// The size and alpha hint of a lossless image (RFC 9649 §3.2): the signature `0x2f`, then 14 bits
/// of width − 1, 14 of height − 1 and the `alpha_is_used` bit, least significant first.
fn vp8l_size(d: &[u8]) -> Result<(u32, u32, bool)> {
    if d.first() != Some(&0x2f) {
        return Err(Error::Malformed("a VP8L chunk without its signature".to_owned()));
    }
    let v = le_u32(d, 1)?;
    Ok(((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1, (v >> 28) & 1 == 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riff_sizes_stop_at_four_gigabytes() {
        assert_eq!(riff_size(10), Ok(14));
        assert_eq!(riff_size(0xffff_fffa), Ok(0xffff_fffe));
        assert_eq!(riff_size(0xffff_fffb), Err(Error::TooMany("bytes in one RIFF file")));
        assert_eq!(riff_size(usize::MAX), Err(Error::TooMany("bytes in one RIFF file")));
    }

    #[test]
    fn image_sizes() {
        // A 300 × 200 key frame and a 640 × 480 lossless header with alpha.
        let vp8 = [0x50, 0x02, 0x00, 0x9d, 0x01, 0x2a, 0x2c, 0x01, 0xc8, 0x00];
        assert_eq!(vp8_size(&vp8), Ok((300, 200, false)));
        assert!(vp8_size(&[0x51, 0x02, 0x00, 0x9d, 0x01, 0x2a, 0x2c, 0x01, 0xc8, 0x00]).is_err());
        assert!(vp8_size(&vp8[..8]).is_err());
        let bits: u32 = 639 | 479 << 14 | 1 << 28;
        let vp8l = [&[0x2f][..], &bits.to_le_bytes()].concat();
        assert_eq!(vp8l_size(&vp8l), Ok((640, 480, true)));
        assert!(vp8l_size(&vp8l[..3]).is_err());
        assert!(vp8l_size(&[0x2e, 0, 0, 0, 0]).is_err());
    }
}
