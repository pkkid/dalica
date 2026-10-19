//! A bounded EBML reader (the binary format under Matroska).
//!
//! Provenance: written from the public specifications, RFC 8794 (Extensible Binary Meta
//! Language) §4–§6 and the element tables of RFC 9559 (Matroska); no code was consulted. An
//! element is a variable-length ID (1–4 bytes; the count of leading zero bits of its first byte
//! says how many more follow, and the marker bit stays in the ID), a variable-length data size
//! (1–8 bytes, the marker bit removed; every value bit set means "unknown size"), then its data.
//!
//! Every size is hostile (`security.md` §4.2), as in `bmff.rs`: an element must lie inside its
//! parent (one running past the end of the bytes is [`Error::Truncated`], so a caller reading a
//! prefix can read on; one running past its parent inside the bytes is [`Error::Malformed`]); an
//! element of unknown size runs to its parent's end and no further; nesting is capped at
//! [`MAX_DEPTH`], elements per level at [`MAX_ELEMENTS`], and every element read is counted by
//! the caller's [`Work`]. Nothing is allocated: elements are ranges of the bytes.

use std::ops::Range;

use crate::limits::Work;
use crate::{Error, Result};

/// The first four bytes of every EBML document: the EBML header's ID.
pub const EBML_MAGIC: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];

/// Deepest nesting of elements followed (top-level elements are depth 0).
pub const MAX_DEPTH: usize = 12;

/// Most elements read in one level.
pub const MAX_ELEMENTS: usize = 4096;

/// One element: its ID (marker bit kept, as the specifications write IDs) and where it and its
/// data lie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub id: u32,
    /// The whole element, header included.
    pub range: Range<usize>,
    /// The data, after the header.
    pub body: Range<usize>,
    pub depth: usize,
    /// Its data size was "unknown": it runs to its parent's end.
    pub unknown_size: bool,
    /// Its data runs past the end of the bytes and was cut there ([`Elements::next_head`]).
    pub cut: bool,
}

/// The elements laid end to end in one range of the bytes.
#[derive(Debug)]
pub struct Elements<'a> {
    bytes: &'a [u8],
    pos: usize,
    end: usize,
    depth: usize,
    count: usize,
}

/// A variable-length integer at `at`: its length and its value, the marker bit kept (`id`) or
/// removed. `Err(Truncated)` past the bytes; `Malformed` for a first byte of zero or a length past
/// `max_len`.
fn vint(bytes: &[u8], at: usize, max_len: usize, keep_marker: bool) -> Result<(usize, u64)> {
    let first = *bytes.get(at).ok_or(Error::Truncated)?;
    if first == 0 {
        return Err(Error::Malformed("an EBML number with no length marker".into()));
    }
    let len = first.leading_zeros() as usize + 1;
    if len > max_len {
        return Err(Error::Malformed(format!("an EBML number of {len} bytes")));
    }
    let rest = bytes.get(at + 1..at + len).ok_or(Error::Truncated)?;
    let lead = if keep_marker { first } else { first & 0xffu8.checked_shr(len as u32).unwrap_or(0) };
    let value = rest.iter().fold(u64::from(lead), |v, &b| (v << 8) | u64::from(b));
    Ok((len, value))
}

impl<'a> Elements<'a> {
    /// The top-level elements of `bytes`.
    pub fn top(bytes: &'a [u8]) -> Elements<'a> {
        Elements { bytes, pos: 0, end: bytes.len(), depth: 0, count: 0 }
    }

    /// The elements in `parent`'s data. `Err(TooMany)` past [`MAX_DEPTH`].
    pub fn children(bytes: &'a [u8], parent: &Element) -> Result<Elements<'a>> {
        Elements::within(bytes, parent, parent.body.start)
    }

    /// The elements in `parent`'s data from `pos` on (a seek target).
    pub fn within(bytes: &'a [u8], parent: &Element, pos: usize) -> Result<Elements<'a>> {
        let depth = parent.depth + 1;
        if depth > MAX_DEPTH {
            return Err(Error::TooMany("nested EBML elements"));
        }
        if !(parent.body.start..=parent.body.end).contains(&pos) {
            return Err(Error::Malformed("a position outside its element".into()));
        }
        Ok(Elements { bytes, pos, end: parent.body.end, depth, count: 0 })
    }

    /// The next element, `Ok(None)` at the end of the range.
    pub fn next(&mut self, work: &mut Work) -> Result<Option<Element>> {
        self.read(work, false)
    }

    /// The next element, its data cut at the end of the bytes if it runs past them (a Matroska
    /// segment read from the first bytes of a file); [`Element::cut`] says so.
    pub fn next_head(&mut self, work: &mut Work) -> Result<Option<Element>> {
        self.read(work, true)
    }

    fn read(&mut self, work: &mut Work, cut_ok: bool) -> Result<Option<Element>> {
        if self.pos >= self.end {
            return Ok(None);
        }
        self.count += 1;
        if self.count > MAX_ELEMENTS {
            return Err(Error::TooMany("EBML elements in one level"));
        }
        work.step()?;
        let start = self.pos;
        let len = self.bytes.len();
        // Cut by the end of the bytes: truncation; by the parent's end: damage.
        let past = |at: usize| if at > len { Error::Truncated } else { Error::Malformed("an EBML element runs past its parent".into()) };
        let fits = |at: usize| if at > self.end { Err(past(at)) } else { Ok(()) };
        let within = |r: Result<(usize, u64)>| match r {
            Err(Error::Truncated) if self.end < len => Err(past(self.end)),
            other => other,
        };
        let view = self.bytes.get(..len.min(self.end)).unwrap_or_default();
        let (id_len, id) = within(vint(view, start, 4, true))?;
        let (size_len, size) = within(vint(view, start + id_len, 8, false))?;
        let header = id_len + size_len;
        fits(start + header)?;
        let unknown = size == (1u64 << (7 * size_len)) - 1;
        let id = u32::try_from(id).map_err(|_| Error::Malformed("an EBML ID past 32 bits".into()))?;
        let data = start + header;
        let mut end = if unknown { self.end } else { usize::try_from(size).ok().and_then(|s| data.checked_add(s)).unwrap_or(usize::MAX) };
        let mut cut = false;
        if cut_ok && end > len && self.end >= len {
            (end, cut) = (len, true);
        }
        fits(end)?;
        self.pos = end;
        Ok(Some(Element { id, range: start..end, body: data..end, depth: self.depth, unknown_size: unknown, cut }))
    }

    /// The first element with `id` from here on.
    pub fn find(&mut self, id: u32, work: &mut Work) -> Result<Option<Element>> {
        while let Some(e) = self.next(work)? {
            if e.id == id {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }
}

/// An element's data.
pub fn data<'a>(bytes: &'a [u8], e: &Element) -> Result<&'a [u8]> {
    bytes.get(e.body.clone()).ok_or(Error::Truncated)
}

/// An unsigned integer element (0–8 bytes).
pub fn uint(bytes: &[u8], e: &Element) -> Result<u64> {
    let d = data(bytes, e)?;
    if d.len() > 8 {
        return Err(Error::Malformed(format!("an unsigned integer of {} bytes", d.len())));
    }
    Ok(d.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b)))
}

/// A signed integer element (0–8 bytes, two's complement).
pub fn int(bytes: &[u8], e: &Element) -> Result<i64> {
    let d = data(bytes, e)?;
    let n = d.len();
    if n > 8 {
        return Err(Error::Malformed(format!("a signed integer of {n} bytes")));
    }
    let raw = d.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b));
    let shift = 64 - 8 * n as u32;
    // Sign-extend from the top bit of the stored bytes.
    Ok(if n == 0 { 0 } else { ((raw << shift) as i64) >> shift })
}

/// A float element (0, 4 or 8 bytes; empty is 0).
pub fn float(bytes: &[u8], e: &Element) -> Result<f64> {
    match data(bytes, e)? {
        [] => Ok(0.0),
        &[a, b, c, d] => Ok(f64::from(f32::from_be_bytes([a, b, c, d]))),
        &[a, b, c, d, e2, f, g, h] => Ok(f64::from_be_bytes([a, b, c, d, e2, f, g, h])),
        other => Err(Error::Malformed(format!("a float of {} bytes", other.len()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> Work {
        Work::new(10_000, "elements")
    }

    #[test]
    fn numbers_and_sizes() {
        assert_eq!(vint(&[0x81], 0, 8, false), Ok((1, 1)));
        assert_eq!(vint(&[0x40, 0x02], 0, 8, false), Ok((2, 2)));
        assert_eq!(vint(&[0x1a, 0x45, 0xdf, 0xa3], 0, 4, true), Ok((4, 0x1a45_dfa3)));
        assert_eq!(vint(&[0x01, 0, 0, 0, 0, 0, 0, 0x23], 0, 8, false), Ok((8, 0x23)));
        assert_eq!(vint(&[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff], 0, 8, false), Ok((8, (1 << 56) - 1)));
        assert!(matches!(vint(&[0x00, 1], 0, 8, false), Err(Error::Malformed(_))));
        assert!(matches!(vint(&[0x08, 1, 2, 3, 4], 0, 4, true), Err(Error::Malformed(_))));
        assert_eq!(vint(&[0x40], 0, 8, false), Err(Error::Truncated));
        // Integers and floats of every legal length, and the illegal ones.
        let el = |b: &[u8]| (b.to_vec(), Element { id: 0x80, range: 0..b.len(), body: 0..b.len(), depth: 0, unknown_size: false, cut: false });
        let (b, e) = el(&[0xff, 0xfe]);
        assert_eq!((uint(&b, &e), int(&b, &e)), (Ok(0xfffe), Ok(-2)));
        let (b, e) = el(&[]);
        assert_eq!((uint(&b, &e), int(&b, &e), float(&b, &e)), (Ok(0), Ok(0), Ok(0.0)));
        let (b, e) = el(&1.5f32.to_be_bytes());
        assert_eq!(float(&b, &e), Ok(1.5));
        let (b, e) = el(&[0; 9]);
        assert!(uint(&b, &e).is_err() && int(&b, &e).is_err() && float(&b, &e).is_err());
    }

    #[test]
    fn unknown_sizes_run_to_the_parent_end() {
        // A parent of 6 data bytes holding an unknown-size child, then a byte past the parent.
        let bytes = [0x80, 0x86, 0x81, 0xff, 0x82, 0x81, 0x07, 0x00, 0xee];
        let mut top = Elements::top(&bytes[..8]);
        let parent = top.next(&mut work()).unwrap().unwrap();
        assert_eq!(parent.body, 2..8);
        let mut kids = Elements::children(&bytes, &parent).unwrap();
        let child = kids.next(&mut work()).unwrap().unwrap();
        assert!(child.unknown_size);
        assert_eq!(child.body, 4..8, "bounded by its parent, not by the bytes");
        assert_eq!(kids.next(&mut work()), Ok(None));
    }

    /// Thirteen levels of nesting: refused at the cap; 4,097 siblings: refused at the cap.
    #[test]
    fn nesting_and_siblings_are_capped() {
        let mut nested = vec![0x81, 0x80];
        for _ in 0..13 {
            nested = [vec![0x80, 0x80 | u8::try_from(nested.len()).unwrap()], nested].concat();
        }
        let mut w = work();
        let mut at = Elements::top(&nested).next(&mut w).unwrap().unwrap();
        let deep = loop {
            match Elements::children(&nested, &at).and_then(|mut c| c.next(&mut w)) {
                Ok(Some(e)) => at = e,
                other => break other,
            }
        };
        assert_eq!(deep, Err(Error::TooMany("nested EBML elements")));
        assert_eq!(at.depth, MAX_DEPTH);
        let many = [0xecu8, 0x80].repeat(MAX_ELEMENTS + 1);
        let mut it = Elements::top(&many);
        let mut w = Work::new(1 << 20, "elements");
        assert_eq!((0..MAX_ELEMENTS).map(|_| it.next(&mut w).map(|e| e.is_some())).collect::<Result<Vec<_>>>().map(|v| v.len()), Ok(MAX_ELEMENTS));
        assert_eq!(it.next(&mut w), Err(Error::TooMany("EBML elements in one level")));
    }
}
