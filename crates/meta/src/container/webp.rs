//! WebP: the RIFF chunks (WebP container specification). `EXIF` and `XMP ` chunks of the `VP8X`
//! extended format, `ICCP`; the RIFF header and `VP8X` are structure a metadata write changes (the
//! size and the flags).

use std::borrow::Cow;

use super::{BlockKind, Container, Scan, le_u32, strip_exif_prefix, take};
use crate::{Error, Limits, Result};

const HEADER: usize = 12;

pub(crate) fn scan<'a>(b: &'a [u8], limits: &Limits) -> Result<Scan<'a>> {
    let mut out = Scan::new(Container::WebP);
    let riff = le_u32(b, 4)? as usize;
    let end = riff.checked_add(8).ok_or(Error::Truncated)?;
    if end > b.len() {
        return Err(Error::Truncated);
    }
    if end < HEADER {
        return Err(Error::Malformed("a RIFF size smaller than its header".into()));
    }
    out.span(BlockKind::Structure, 0..HEADER, true);
    let riff = b.get(..end).unwrap_or_default();
    let mut at = HEADER;
    let mut chunks = 0usize;
    while at < end {
        chunks += 1;
        if chunks > limits.max_segments {
            return Err(Error::TooMany("WebP chunks"));
        }
        let kind = take(riff, at, 4)?;
        let len = le_u32(riff, at + 4)? as usize;
        let data = take(riff, at + 8, len)?;
        // Odd sizes are padded to even; a missing pad byte at the very end is tolerated.
        let padded = (at + 8 + len + (len & 1)).min(end);
        let range = at..padded;
        match kind {
            b"VP8X" => out.span(BlockKind::Structure, range, true),
            b"EXIF" => {
                let first = out.exif.is_none();
                if first {
                    out.exif = Some(Cow::Borrowed(strip_exif_prefix(data)));
                }
                out.span(BlockKind::Exif, range, first);
            }
            b"XMP " => {
                let first = out.xmp.is_none();
                if first {
                    out.xmp = Some(Cow::Borrowed(data));
                }
                out.span(BlockKind::Xmp, range, first);
            }
            b"ICCP" => out.span(BlockKind::Icc, range, false),
            _ => {}
        }
        at = padded;
    }
    Ok(out)
}
