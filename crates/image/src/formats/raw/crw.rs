//! CRW (Canon CIFF) raws: the JPEG preview found through the CIFF heaps (`formats.md` §2).
//!
//! Provenance: written from the public description of the Camera Image File Format (CIFF 1.0):
//! a header (byte order `II`/`MM`, the header's length, the `HEAPCCDR` signature) followed by the
//! root heap, which runs to the end of the file. A heap ends with a 32-bit offset (from the
//! heap's start) of its record table: a 16-bit count, then 10-byte records (16-bit type, 32-bit
//! size, 32-bit offset from the heap's start). The type's top two bits say where the data lies
//! (0x4000: inside the record itself), bits 11–13 its kind (0x2800 and 0x3000: a sub-heap), and
//! the low 14 bits name it: 0x2007 the full-size JPEG, 0x2008 a thumbnail, 0x1810 the image
//! specification (its fourth 32-bit value is the rotation in degrees), 0x080a the camera make
//! and model (two NUL-terminated strings), 0x180e the time stamp (seconds since 1970, local
//! time). No code was consulted.
//!
//! The root heap's record table is found from the end of the file, so a partial read cannot know
//! where it is: unless the table ends exactly at the trailing offset the bytes are taken as
//! [`Error::Truncated`] and the caller reads on. Heaps nest at most [`MAX_DEPTH`] deep and every
//! record read is counted.

use std::ops::Range;

use dalica_core::DateTime;

use super::raf::{Previews, decode_main, header_of, pick, span, thumbnail_of};
use crate::decode::Decoded;
use crate::limits::{Budget, Limits, Work};
use crate::sniff::RawKind;
use crate::{Error, Exif, Format, Header, Orientation, Result};

/// Whether this build shows these raws' previews.
pub const DECODES: bool = true;

/// The tile text for a raw without a usable preview (`formats.md` §2: the generic RAW tile).
pub const REASON: &str = "This RAW file has no preview Dalica can show";

const FORMAT: Format = Format::Raw(RawKind::Crw);

/// Deepest nesting of heaps followed (the root heap is depth 0).
pub const MAX_DEPTH: usize = 8;

const TAG_JPEG: u16 = 0x2007;
const TAG_THUMBNAIL: u16 = 0x2008;
const TAG_IMAGE_SPEC: u16 = 0x1810;
const TAG_MAKE_MODEL: u16 = 0x080a;
const TAG_TIME_STAMP: u16 = 0x180e;

/// Whether `head` starts this container.
pub fn recognise(head: &[u8]) -> bool {
    // Byte order "II" (or "MM"), a header length, then the "HEAPCCDR" signature.
    (head.starts_with(b"II") || head.starts_with(b"MM")) && head.get(6..14) == Some(b"HEAPCCDR")
}

/// What the heaps hold that the header needs.
#[derive(Debug, Default)]
struct Found {
    candidates: Vec<Range<usize>>,
    rotation: Option<i32>,
    make_model: Option<Range<usize>>,
    time: Option<u32>,
}

struct Ciff<'a> {
    bytes: &'a [u8],
    little: bool,
}

impl Ciff<'_> {
    fn u16(&self, at: usize) -> Result<u16> {
        match self.bytes.get(at..at.saturating_add(2)) {
            Some(&[a, b]) => Ok(if self.little { u16::from_le_bytes([a, b]) } else { u16::from_be_bytes([a, b]) }),
            _ => Err(Error::Truncated),
        }
    }

    fn u32(&self, at: usize) -> Result<u32> {
        match self.bytes.get(at..at.saturating_add(4)) {
            Some(&[a, b, c, d]) => Ok(if self.little { u32::from_le_bytes([a, b, c, d]) } else { u32::from_be_bytes([a, b, c, d]) }),
            _ => Err(Error::Truncated),
        }
    }

    /// Reads the heap `heap` (a range of the file) and the heaps inside it.
    fn heap(&self, heap: Range<usize>, depth: usize, found: &mut Found, work: &mut Work) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(Error::TooMany("nested CIFF heaps"));
        }
        let malformed = |what: &str| Error::Malformed(format!("CIFF heap: {what}"));
        let tail = heap.end.checked_sub(4).filter(|&t| t >= heap.start).ok_or_else(|| malformed("too short"))?;
        let table = heap.start.checked_add(self.u32(tail)? as usize).ok_or(Error::OutOfRange)?;
        let count = usize::from(self.u16(table)?);
        let records_end = table.checked_add(2 + 10 * count).ok_or(Error::OutOfRange)?;
        if depth == 0 && records_end != tail {
            // The root table must end where the trailing offset starts; anything else is a
            // partial read (or a damaged file, once the whole file has been read).
            return Err(Error::Truncated);
        }
        if records_end > tail {
            return Err(malformed("the record table runs past the heap"));
        }
        for i in 0..count {
            work.step()?;
            let at = table + 2 + 10 * i;
            let (kind, size, offset) = (self.u16(at)?, self.u32(at + 2)?, self.u32(at + 6)?);
            if kind & 0xc000 == 0x4000 {
                continue; // data inside the record: nothing this reader needs
            }
            let data = span(heap.start as u64 + u64::from(offset), u64::from(size))?;
            if data.start < heap.start || data.end > tail {
                return Err(malformed("a record lies outside its heap"));
            }
            match (kind & 0x3800, kind & 0x3fff) {
                (0x2800 | 0x3000, _) => self.heap(data, depth + 1, found, work)?,
                (_, TAG_JPEG | TAG_THUMBNAIL) => found.candidates.push(data),
                (_, TAG_IMAGE_SPEC) if found.rotation.is_none() && data.len() >= 16 => {
                    found.rotation = Some(self.u32(data.start + 12)? as i32);
                }
                (_, TAG_MAKE_MODEL) if found.make_model.is_none() => found.make_model = Some(data),
                (_, TAG_TIME_STAMP) if found.time.is_none() && data.len() >= 4 => found.time = Some(self.u32(data.start)?),
                _ => {}
            }
        }
        Ok(())
    }
}

fn walk(bytes: &[u8], limits: &Limits) -> Result<Found> {
    if !recognise(bytes) {
        return Err(Error::Malformed("not a CRW file".into()));
    }
    let ciff = Ciff { bytes, little: bytes.starts_with(b"II") };
    let header_len = ciff.u32(2)? as usize;
    if header_len < 14 || header_len > bytes.len() {
        return Err(if header_len < 14 { Error::Malformed(format!("CIFF header of {header_len} bytes")) } else { Error::Truncated });
    }
    let mut found = Found::default();
    let mut work = Work::new(limits.max_segments, "CIFF records");
    ciff.heap(header_len..bytes.len(), 0, &mut found, &mut work)?;
    Ok(found)
}

/// The EXIF orientation of a CIFF rotation (degrees clockwise to display); `None` for others.
fn orientation(degrees: i32) -> Option<Orientation> {
    match degrees.rem_euclid(360) {
        0 => Orientation::new(1),
        90 => Orientation::new(6),
        180 => Orientation::new(3),
        270 => Orientation::new(8),
        _ => None,
    }
}

fn previews(bytes: &[u8], found: &Found, limits: &Limits) -> Result<Previews> {
    pick(bytes, &found.candidates, limits)?.ok_or(Error::Unsupported(FORMAT))
}

pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let found = walk(bytes, limits)?;
    let mut h = header_of(FORMAT, &previews(bytes, &found, limits)?, None, limits);
    let text = |s: &[u8]| Some(String::from_utf8_lossy(s.get(..s.len().min(limits.max_string))?).trim().to_string()).filter(|s| !s.is_empty());
    let mut names = found.make_model.as_ref().and_then(|r| bytes.get(r.clone())).unwrap_or_default().split(|&b| b == 0);
    let (make, model) = (names.next().and_then(text), names.next().and_then(text));
    let turned = found.rotation.and_then(orientation);
    if let Some(o) = turned {
        h.orientation = o;
    }
    let exif = Exif {
        orientation: turned.map(Orientation::get),
        date_time_original: found.time.and_then(|t| DateTime::from_unix_secs(i64::from(t))),
        make,
        model,
        ..Exif::default()
    };
    if exif != Exif::default() {
        h.exif = Some(exif);
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
