//! RAF (Fujifilm) raws: the embedded JPEG preview the RAF header points at (`formats.md` §2), and
//! the preview choice the other containers of M5.10 share ([`pick`], [`header_of`],
//! [`decode_main`]).
//!
//! Provenance: written from public prose descriptions of the RAF container: a 16-byte
//! `FUJIFILMCCD-RAW ` signature, a format version, the camera's id and name, then a directory of
//! big-endian (offset, length) pairs whose first pair, at byte 84, locates a complete EXIF JPEG
//! rendered by the camera. No code was consulted.
//!
//! The JPEG carries the photo's EXIF (orientation, dates, camera) and usually its own thumbnail,
//! which becomes the thumbnail fast path. An offset past the end of the bytes is
//! [`Error::Truncated`], so a partial header read reads on.

use std::ops::Range;

use crate::decode::Decoded;
use crate::formats::{jpeg, u32_be};
use crate::limits::{Budget, Limits};
use crate::sniff::RawKind;
use crate::{Error, Format, Header, Orientation, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = true;

/// The tile text for a raw without a usable preview (`formats.md` §2: the generic RAW tile).
pub const REASON: &str = "This RAW file has no preview Dalica can show";

const FORMAT: Format = Format::Raw(RawKind::Raf);

/// Where the JPEG's (offset, length) pair lies in the header.
const JPEG_POINTER: usize = 84;

/// Whether `head` starts this container.
pub fn recognise(head: &[u8]) -> bool {
    head.starts_with(b"FUJIFILMCCD-RAW")
}

fn previews(bytes: &[u8], limits: &Limits) -> Result<Previews> {
    if !recognise(bytes) {
        return Err(Error::Malformed("not a RAF file".into()));
    }
    let offset = u32_be(bytes, JPEG_POINTER)?;
    let len = u32_be(bytes, JPEG_POINTER + 4)?;
    let jpeg = span(u64::from(offset), u64::from(len))?;
    pick(bytes, &[jpeg], limits)?.ok_or(Error::Unsupported(FORMAT))
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    Ok(header_of(FORMAT, &previews(bytes, limits)?, None, limits))
}

pub fn decode(bytes: &[u8], header: &Header, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    decode_main(bytes, header, &previews(bytes, limits)?, budget, limits)
}

pub fn embedded_thumbnail(bytes: &[u8], limits: &Limits) -> Result<Option<Range<usize>>> {
    thumbnail_of(header(bytes, limits))
}

// The preview choice shared by the RAW containers of M5.10 (CR3, RAF, CRW, MRW, X3F). It lives
// here rather than in `raw/mod.rs` so the tasks filling `raw/` never share a file.

/// Most embedded JPEGs looked at in one file.
pub(crate) const MAX_PREVIEWS: usize = 64;

/// An embedded JPEG: where it lies in the file and its own header.
#[derive(Clone, Debug)]
pub(crate) struct Preview {
    pub range: Range<usize>,
    pub header: Header,
    /// Its first byte is not the 0xFF of the SOI marker and is read as one ([`pick_repairing`]).
    pub repaired: bool,
}

/// The largest embedded JPEG (what the file shows) and the next smaller one (the thumbnail fast
/// path), if any.
#[derive(Clone, Debug)]
pub(crate) struct Previews {
    pub main: Preview,
    pub thumb: Option<Range<usize>>,
}

/// `len` bytes from `offset`, as a range (checked; it may still lie past the end of the bytes).
pub(crate) fn span(offset: u64, len: u64) -> Result<Range<usize>> {
    let end = offset.checked_add(len).ok_or(Error::OutOfRange)?;
    let start = usize::try_from(offset).map_err(|_| Error::Truncated)?;
    let end = usize::try_from(end).map_err(|_| Error::Truncated)?;
    Ok(start..end)
}

/// Chooses among candidate byte ranges. A candidate over [`Limits::max_thumbnail`] bytes, not
/// starting with a JPEG SOI marker or without a readable JPEG header is passed over; one running
/// past the end of the bytes is [`Error::Truncated`] (a partial read must read on to know which
/// is largest). The largest by pixels (then bytes) is the preview; the largest smaller one is the
/// thumbnail. `Ok(None)`: no usable JPEG.
pub(crate) fn pick(bytes: &[u8], candidates: &[Range<usize>], limits: &Limits) -> Result<Option<Previews>> {
    choose(bytes, candidates, limits, false)
}

/// [`pick`] for containers whose cameras store the JPEG with a wrong first byte (`?? D8 FF`
/// instead of `FF D8 FF`; Minolta's MRW): such a candidate is read as if that byte were 0xFF,
/// through a copy. A repaired JPEG is never offered as the thumbnail range, which callers read
/// from the file as it is.
pub(crate) fn pick_repairing(bytes: &[u8], candidates: &[Range<usize>], limits: &Limits) -> Result<Option<Previews>> {
    choose(bytes, candidates, limits, true)
}

/// Whether `data` starts like a JPEG: `Some(false)` as it is, `Some(true)` once its first byte is
/// set to 0xFF (only when `repair` allows it), `None` not at all.
fn soi(data: &[u8], repair: bool) -> Option<bool> {
    if data.starts_with(b"\xff\xd8\xff") {
        Some(false)
    } else if repair && data.len() > 3 && data.get(1..3) == Some(b"\xd8\xff") {
        Some(true)
    } else {
        None
    }
}

/// `data` with its first byte set to 0xFF, in `buf` (fallibly reserved by the caller).
fn with_soi(data: &[u8], mut buf: Vec<u8>) -> Vec<u8> {
    buf.clear();
    buf.extend_from_slice(data);
    if let Some(b) = buf.first_mut() {
        *b = 0xff;
    }
    buf
}

fn choose(bytes: &[u8], candidates: &[Range<usize>], limits: &Limits, repair: bool) -> Result<Option<Previews>> {
    if candidates.len() > MAX_PREVIEWS {
        return Err(Error::TooMany("embedded previews"));
    }
    let mut found: Vec<Preview> = Vec::new();
    for range in candidates {
        if range.is_empty() || range.len() as u64 > limits.max_thumbnail || found.iter().any(|p| p.range == *range) {
            continue;
        }
        let data = bytes.get(range.clone()).ok_or(Error::Truncated)?;
        let header = match soi(data, repair) {
            None => continue,
            Some(false) => jpeg::header(data, limits),
            Some(true) => {
                let mut buf = Vec::new();
                buf.try_reserve_exact(data.len()).map_err(|_| Error::TooLarge { width: 0, height: 0 })?;
                jpeg::header(&with_soi(data, buf), limits)
            }
        };
        if let Ok(header) = header {
            found.push(Preview { range: range.clone(), header, repaired: soi(data, repair) == Some(true) });
        }
    }
    let pixels = |p: &Preview| u64::from(p.header.width) * u64::from(p.header.height);
    let Some(main) = found.iter().max_by_key(|p| (pixels(p), p.range.len())).cloned() else { return Ok(None) };
    let smaller = found.iter().filter(|p| pixels(p) < pixels(&main) && !p.repaired);
    let thumb = smaller.max_by_key(|p| (pixels(p), p.range.len())).map(|p| p.range.clone());
    Ok(Some(Previews { main, thumb }))
}

/// The raw's header: the preview's size; orientation, dates and camera from `exif` (the raw's
/// own TIFF block and its offset in the file) when the container has one, else from the
/// preview's EXIF; the thumbnail is the next smaller preview, else an EXIF thumbnail.
pub(crate) fn header_of(format: Format, p: &Previews, exif: Option<(&[u8], usize)>, limits: &Limits) -> Header {
    let jh = &p.main.header;
    let at = p.main.range.start;
    let mut h = Header {
        format,
        width: jh.width,
        height: jh.height,
        orientation: jh.orientation,
        exif: jh.exif.clone(),
        exif_error: jh.exif_error.clone(),
        thumbnail: jh.thumbnail.as_ref().map(|r| r.start + at..r.end + at),
    };
    if exif.is_some() {
        let bare = Header { orientation: Orientation::UPRIGHT, exif: None, exif_error: None, thumbnail: None, ..h.clone() };
        let own = bare.with_exif(exif, limits);
        h = Header { thumbnail: own.thumbnail.clone().or(h.thumbnail), ..own };
    }
    if let Some(t) = &p.thumb {
        h.thumbnail = Some(t.clone());
    }
    h
}

/// Decodes the main preview; its orientation is the raw's.
pub(crate) fn decode_main(bytes: &[u8], header: &Header, p: &Previews, budget: &mut Budget, limits: &Limits) -> Result<Decoded> {
    let data = bytes.get(p.main.range.clone()).ok_or(Error::Truncated)?;
    let mut decoded = if p.main.repaired {
        let copy = with_soi(data, budget.vec(data.len(), 0u8)?);
        let decoded = jpeg::decode(&copy, &p.main.header, budget, limits);
        budget.refund(copy.len() as u64);
        decoded?
    } else {
        jpeg::decode(data, &p.main.header, budget, limits)?
    };
    decoded.orientation = header.orientation;
    Ok(decoded)
}

/// `embedded_thumbnail` from a header: a raw without a preview has no thumbnail either.
pub(crate) fn thumbnail_of(header: Result<Header>) -> Result<Option<Range<usize>>> {
    match header {
        Ok(h) => Ok(h.thumbnail),
        Err(Error::Unsupported(_)) => Ok(None),
        Err(e) => Err(e),
    }
}
