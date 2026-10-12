//! A bounded TIFF/IFD reader, shared by EXIF reading, RAW previews and `dalica-meta`.
//!
//! Provenance: written for Dalica from public descriptions only: the TIFF 6.0 specification
//! (header, IFD and field layout, §2), TIFF Technical Note 1 (SubIFDs), the EXIF 2.32
//! specification (the EXIF and Interoperability IFD pointers) and the DNG 1.4 specification (raw
//! files built on TIFF). No other reader's code was consulted.
//!
//! Every offset and count comes from the file and is treated as hostile (`security.md` §4.1):
//! ranges are checked before they are read, multiplications are checked, the number of entries
//! per IFD and of IFDs per structure is capped, sub-IFD depth is capped, and every IFD offset is
//! remembered so a loop is an error rather than a hang.
//!
//! # Public API (stable: other format modules read through it)
//!
//! - [`Tiff::new`] (a TIFF header), [`Tiff::new_raw`] (also the raw-file header variants),
//!   [`Tiff::headerless`] (a structure whose offsets count from its own first byte, such as a
//!   maker note) and [`Tiff::rebased`].
//! - [`Tiff::ifd`] reads one IFD; [`Tiff::walk`] reads every IFD reachable from one, following the
//!   next-IFD chain and the pointer tags it is given, within the [`Limits`] caps.
//! - Values: [`Tiff::value`], [`Tiff::value_range`], [`Tiff::uint`], [`Tiff::uints`],
//!   [`Tiff::ascii`], and the raw readers [`Tiff::u16_at`], [`Tiff::u32_at`], [`Tiff::bytes`].
//! - Every failure is an [`Error`]: `OutOfRange` (an offset or length outside the data), `Loop`
//!   (an IFD read twice), `TooMany` (a cap reached), `Malformed` (not a TIFF header).

use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Range;

use crate::limits::Limits;
use crate::{Error, Result};

/// Field types (TIFF 6.0 §2, plus EXIF's use of them).
pub mod types {
    pub const BYTE: u16 = 1;
    pub const ASCII: u16 = 2;
    pub const SHORT: u16 = 3;
    pub const LONG: u16 = 4;
    pub const RATIONAL: u16 = 5;
    pub const SBYTE: u16 = 6;
    pub const UNDEFINED: u16 = 7;
    pub const SSHORT: u16 = 8;
    pub const SLONG: u16 = 9;
    pub const SRATIONAL: u16 = 10;
    pub const FLOAT: u16 = 11;
    pub const DOUBLE: u16 = 12;
    pub const IFD: u16 = 13;
}

/// Tags whose values are offsets of further IFDs in the same structure.
pub mod tags {
    /// SubIFDs (TIFF Technical Note 1): reduced-resolution images, raw data, previews.
    pub const SUB_IFDS: u16 = 0x014a;
    /// The EXIF IFD (EXIF 2.32 §4.6.3).
    pub const EXIF_IFD: u16 = 0x8769;
    /// The GPS IFD (EXIF 2.32 §4.6.3).
    pub const GPS_IFD: u16 = 0x8825;
    /// The Interoperability IFD, inside the EXIF IFD (EXIF 2.32 §4.6.3).
    pub const INTEROP_IFD: u16 = 0xa005;
}

/// Bytes per value of a field type; `None` for types this reader does not know.
fn type_size(kind: u16) -> Option<u64> {
    use types::*;
    match kind {
        BYTE | ASCII | SBYTE | UNDEFINED => Some(1),
        SHORT | SSHORT => Some(2),
        LONG | SLONG | FLOAT | IFD => Some(4),
        RATIONAL | SRATIONAL | DOUBLE => Some(8),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Little,
    Big,
}

/// A TIFF structure: the bytes from its header (`II*\0` / `MM\0*`) on. Offsets inside it are
/// relative to its first byte.
#[derive(Clone, Copy, Debug)]
pub struct Tiff<'a> {
    data: &'a [u8],
    order: Order,
    /// The smallest offset an IFD may start at: 8 after a header, 0 in a headerless structure.
    first: u32,
}

/// One IFD entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    /// The value itself when it fits in four bytes, else its offset (in file byte order).
    raw: [u8; 4],
}

/// One IFD: its entries in file order and the offset of the next IFD (0 for none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ifd {
    pub offset: u32,
    pub entries: Vec<Entry>,
    pub next: u32,
}

impl Ifd {
    pub fn get(&self, tag: u16) -> Option<&Entry> {
        self.entries.iter().find(|e| e.tag == tag)
    }
}

/// Remembers the IFDs read from one structure, to detect loops and enforce the caps.
#[derive(Debug, Default)]
pub struct Visited {
    offsets: HashSet<u32>,
}

impl Visited {
    /// IFDs read so far.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }
}

impl<'a> Tiff<'a> {
    /// Checks the header; `Err(NotTiff)` unless `data` starts with a TIFF header.
    pub fn new(data: &'a [u8]) -> Result<Tiff<'a>> {
        let order = match data.get(..4) {
            Some(b"II*\0") => Order::Little,
            Some(b"MM\0*") => Order::Big,
            _ => return Err(Error::Malformed("not a TIFF header".into())),
        };
        Ok(Tiff { data, order, first: 8 })
    }

    /// Like [`Tiff::new`], also accepting the TIFF header variants of camera raws, whose magic
    /// number differs but whose IFDs are TIFF's: ORF (`IIRO`, `IIRS`, `MMOR`) and RW2 (`IIU\0`).
    pub fn new_raw(data: &'a [u8]) -> Result<Tiff<'a>> {
        let order = match data.get(..4) {
            Some(b"II*\0" | b"IIRO" | b"IIRS" | b"IIU\0") => Order::Little,
            Some(b"MM\0*" | b"MMOR") => Order::Big,
            _ => return Err(Error::Malformed("not a TIFF header".into())),
        };
        Ok(Tiff { data, order, first: 8 })
    }

    /// A structure without a header: offsets count from the first byte of `data`, values are in
    /// the given byte order, and an IFD may start anywhere (vendor maker notes). [`Tiff::first_ifd`]
    /// means nothing here.
    pub fn headerless(data: &'a [u8], little_endian: bool) -> Tiff<'a> {
        Tiff { data, order: if little_endian { Order::Little } else { Order::Big }, first: 0 }
    }

    /// This structure's bytes from `base` on, as a headerless structure in the same byte order
    /// (for blocks whose offsets count from their own start).
    pub fn rebased(&self, base: u64) -> Result<Tiff<'a>> {
        let base = usize::try_from(base).map_err(|_| Error::OutOfRange)?;
        Ok(Tiff { data: self.data.get(base..).ok_or(Error::OutOfRange)?, order: self.order, first: 0 })
    }

    /// The bytes this structure's offsets count from.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn is_little_endian(&self) -> bool {
        self.order == Order::Little
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn u16_at(&self, at: u64) -> Result<u16> {
        let b = self.bytes(at, 2)?;
        let b = [*b.first().unwrap_or(&0), *b.get(1).unwrap_or(&0)];
        Ok(match self.order {
            Order::Little => u16::from_le_bytes(b),
            Order::Big => u16::from_be_bytes(b),
        })
    }

    pub fn u32_at(&self, at: u64) -> Result<u32> {
        let b = self.bytes(at, 4)?;
        let b = [*b.first().unwrap_or(&0), *b.get(1).unwrap_or(&0), *b.get(2).unwrap_or(&0), *b.get(3).unwrap_or(&0)];
        Ok(self.u32_from(b))
    }

    fn u32_from(&self, b: [u8; 4]) -> u32 {
        match self.order {
            Order::Little => u32::from_le_bytes(b),
            Order::Big => u32::from_be_bytes(b),
        }
    }

    /// `len` bytes at `at`, or `Err(OutOfRange)` if any of them lies outside the structure.
    pub fn bytes(&self, at: u64, len: u64) -> Result<&'a [u8]> {
        let end = at.checked_add(len).ok_or(Error::OutOfRange)?;
        let (at, end) = (usize::try_from(at).map_err(|_| Error::OutOfRange)?, usize::try_from(end).map_err(|_| Error::OutOfRange)?);
        self.data.get(at..end).ok_or(Error::OutOfRange)
    }

    /// The offset of IFD0.
    pub fn first_ifd(&self) -> Result<u32> {
        self.u32_at(4)
    }

    /// Reads the IFD at `offset`, recording it in `visited`; `Err(Loop)` if it was read before,
    /// `Err(TooMany)` past the IFD or entry caps, `Err(OutOfRange)` if it does not fit.
    pub fn ifd(&self, offset: u32, visited: &mut Visited, limits: &Limits) -> Result<Ifd> {
        if offset < self.first {
            return Err(Error::OutOfRange);
        }
        if !visited.offsets.insert(offset) {
            return Err(Error::Loop);
        }
        if visited.offsets.len() > limits.max_ifds {
            return Err(Error::TooMany("IFDs"));
        }
        let count = usize::from(self.u16_at(u64::from(offset))?);
        if count > limits.max_ifd_entries {
            return Err(Error::TooMany("IFD entries"));
        }
        let start = u64::from(offset) + 2;
        let table = self.bytes(start, 12 * count as u64 + 4)?;
        let mut entries = Vec::with_capacity(count);
        for &[t0, t1, k0, k1, c0, c1, c2, c3, r0, r1, r2, r3] in table.as_chunks::<12>().0.iter().take(count) {
            let (tag, kind, count) = (self.pair_u16([t0, t1]), self.pair_u16([k0, k1]), self.u32_from([c0, c1, c2, c3]));
            entries.push(Entry { tag, kind, count, raw: [r0, r1, r2, r3] });
        }
        let next = self.u32_at(start + 12 * count as u64)?;
        Ok(Ifd { offset, entries, next })
    }

    fn pair_u16(&self, b: [u8; 2]) -> u16 {
        match self.order {
            Order::Little => u16::from_le_bytes(b),
            Order::Big => u16::from_be_bytes(b),
        }
    }

    /// The bytes of an entry's value, inline or at its offset; `Ok(None)` for unknown types.
    pub fn value(&self, e: &Entry) -> Result<Option<Cow<'a, [u8]>>> {
        let Some(size) = type_size(e.kind) else { return Ok(None) };
        let len = size.checked_mul(u64::from(e.count)).ok_or(Error::OutOfRange)?;
        if len <= 4 {
            let inline = e.raw.get(..len as usize).unwrap_or_default();
            return Ok(Some(Cow::Owned(inline.to_vec())));
        }
        self.bytes(u64::from(self.u32_from(e.raw)), len).map(|b| Some(Cow::Borrowed(b)))
    }

    /// Where an entry's value lies when it does not fit in the entry (more than four bytes), as a
    /// checked range of the data; `Ok(None)` for inline values and unknown types.
    pub fn value_range(&self, e: &Entry) -> Result<Option<Range<usize>>> {
        let Some(size) = type_size(e.kind) else { return Ok(None) };
        let len = size.checked_mul(u64::from(e.count)).ok_or(Error::OutOfRange)?;
        if len <= 4 {
            return Ok(None);
        }
        let at = u64::from(self.u32_from(e.raw));
        self.bytes(at, len)?;
        // `bytes` succeeded, so both ends fit in `usize`.
        Ok(Some(at as usize..(at + len) as usize))
    }

    /// Every value of a SHORT, LONG or IFD entry; `Err(TooMany)` past `max` values, `Ok` empty
    /// for other types.
    pub fn uints(&self, e: &Entry, max: usize, what: &'static str) -> Result<Vec<u32>> {
        if !matches!(e.kind, types::SHORT | types::LONG | types::IFD) {
            return Ok(Vec::new());
        }
        if e.count as usize > max {
            return Err(Error::TooMany(what));
        }
        let value = self.value(e)?.unwrap_or_default();
        Ok(match e.kind {
            types::SHORT => value.as_chunks::<2>().0.iter().map(|&b| u32::from(self.pair_u16(b))).collect(),
            _ => value.as_chunks::<4>().0.iter().map(|&b| self.u32_from(b)).collect(),
        })
    }

    /// Every IFD reachable from the one at `start`: its next-IFD chain and, depth first, the IFDs
    /// that the `follow` tags of each one point to (and their chains), each with its depth (the
    /// starting chain is depth 1). Strict: a loop, an offset outside the data or a cap of `limits`
    /// reached anywhere is an error. IFDs already in `visited` count as a loop.
    pub fn walk(&self, start: u32, follow: &[u16], limits: &Limits, visited: &mut Visited) -> Result<Vec<(Ifd, usize)>> {
        let mut out = Vec::new();
        let mut stack = vec![(start, 1usize)];
        while let Some((offset, depth)) = stack.pop() {
            if depth > limits.max_ifd_depth {
                return Err(Error::TooMany("IFD depth"));
            }
            let ifd = self.ifd(offset, visited, limits)?;
            if ifd.next != 0 {
                stack.push((ifd.next, depth));
            }
            let mut children = Vec::new();
            for e in ifd.entries.iter().filter(|e| follow.contains(&e.tag)) {
                children.extend(self.uints(e, limits.max_ifds, "IFDs")?.into_iter().filter(|&o| o != 0));
            }
            if !children.is_empty() && depth >= limits.max_ifd_depth {
                return Err(Error::TooMany("IFD depth"));
            }
            stack.extend(children.into_iter().rev().map(|o| (o, depth + 1)));
            out.push((ifd, depth));
        }
        Ok(out)
    }

    /// The first value of a SHORT or LONG entry.
    pub fn uint(&self, e: &Entry) -> Option<u32> {
        match (e.kind, e.count) {
            (_, 0) => None,
            (types::SHORT, _) => {
                let [a, b, _, _] = e.raw;
                Some(u32::from(self.pair_u16([a, b])))
            }
            (types::LONG | types::IFD, _) => Some(self.u32_from(e.raw)),
            _ => None,
        }
    }

    /// An ASCII value up to its first NUL, at most `max` bytes, as text (lossily).
    pub fn ascii(&self, e: &Entry, max: usize) -> Option<String> {
        if e.kind != types::ASCII {
            return None;
        }
        let value = self.value(e).ok()??;
        let bytes = value.split(|&b| b == 0).next().unwrap_or_default();
        let bytes = bytes.get(..bytes.len().min(max)).unwrap_or_default();
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little-endian TIFF with one IFD at 8 holding `entries` (tag, type, count, value field).
    fn tiff(entries: &[(u16, u16, u32, [u8; 4])], tail: &[u8]) -> Vec<u8> {
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        b.extend((entries.len() as u16).to_le_bytes());
        for (tag, kind, count, raw) in entries {
            b.extend(tag.to_le_bytes());
            b.extend(kind.to_le_bytes());
            b.extend(count.to_le_bytes());
            b.extend(raw);
        }
        b.extend(0u32.to_le_bytes());
        b.extend(tail);
        b
    }

    #[test]
    fn values_inline_out_of_line_and_unknown() {
        let tail_at = 8 + 2 + 12 * 4 + 4;
        let b = tiff(
            &[
                (1, types::SHORT, 1, [7, 0, 0, 0]),
                (2, types::ASCII, 6, (tail_at as u32).to_le_bytes()),
                (3, 99, 1, [1, 2, 3, 4]),
                (4, types::LONG, u32::MAX, [0, 0, 0, 0]),
            ],
            b"hello\0",
        );
        let t = Tiff::new(&b).unwrap();
        let ifd = t.ifd(t.first_ifd().unwrap(), &mut Visited::default(), &Limits::default()).unwrap();
        assert_eq!(t.uint(ifd.get(1).unwrap()), Some(7));
        assert_eq!(t.value(ifd.get(1).unwrap()).unwrap().unwrap().as_ref(), [7, 0]);
        assert_eq!(t.ascii(ifd.get(2).unwrap(), 100).as_deref(), Some("hello"));
        assert_eq!(t.ascii(ifd.get(2).unwrap(), 3).as_deref(), Some("hel"));
        assert_eq!(t.value(ifd.get(3).unwrap()).unwrap(), None);
        // u32::MAX longs: the size overflows nothing but lies far outside the data.
        assert_eq!(t.value(ifd.get(4).unwrap()), Err(Error::OutOfRange));
        assert_eq!(t.uint(&Entry { tag: 5, kind: types::SHORT, count: 0, raw: [1, 0, 0, 0] }), None);
    }

    #[test]
    fn the_entry_cap_and_reading_an_ifd_twice() {
        let b = tiff(&[(1, types::SHORT, 1, [1, 0, 0, 0]), (2, types::SHORT, 1, [2, 0, 0, 0])], &[]);
        let t = Tiff::new(&b).unwrap();
        assert_eq!(t.ifd(8, &mut Visited::default(), &Limits { max_ifd_entries: 1, ..Limits::default() }), Err(Error::TooMany("IFD entries")));
        let mut v = Visited::default();
        assert!(t.ifd(8, &mut v, &Limits::default()).is_ok());
        assert_eq!(t.ifd(8, &mut v, &Limits::default()), Err(Error::Loop));
        assert_eq!(t.bytes(u64::MAX, 2), Err(Error::OutOfRange));
        assert!(Tiff::new(b"MM\0*").is_ok() && Tiff::new(b"MM\0").is_err());
    }

    #[test]
    fn raw_header_variants_and_headerless_structures() {
        for (head, little) in [(&b"IIRO"[..], true), (b"IIRS", true), (b"IIU\0", true), (b"MMOR", false), (b"II*\0", true), (b"MM\0*", false)] {
            let t = Tiff::new_raw(head).unwrap();
            assert_eq!(t.is_little_endian(), little);
        }
        assert!(Tiff::new_raw(b"IIXX").is_err() && Tiff::new(b"IIRO").is_err());
        // A maker-note-like block: an IFD at 6 after a 6-byte preamble, offsets from its start.
        let mut b = b"AOC\0II".to_vec();
        b.extend(1u16.to_le_bytes());
        b.extend([1, 0, 4, 0, 1, 0, 0, 0, 42, 0, 0, 0]);
        b.extend(0u32.to_le_bytes());
        let outer = [&b"pad!"[..], &b].concat();
        let t = Tiff::new_raw(b"II*\0\x08\0\0\0").unwrap();
        assert!(t.rebased(100).is_err());
        let inner = Tiff::headerless(&outer, true).rebased(4).unwrap();
        assert_eq!(inner.data(), &b[..]);
        let ifd = inner.ifd(6, &mut Visited::default(), &Limits::default()).unwrap();
        assert_eq!(inner.uint(ifd.get(1).unwrap()), Some(42));
        // Headed structures still refuse IFDs inside the header.
        let head = tiff(&[], &[]);
        assert_eq!(Tiff::new(&head).unwrap().ifd(6, &mut Visited::default(), &Limits::default()), Err(Error::OutOfRange));
    }

    #[test]
    fn value_ranges_and_value_lists() {
        let tail_at = 8 + 2 + 12 * 3 + 4;
        let b = tiff(
            &[
                (1, types::SHORT, 3, (tail_at as u32).to_le_bytes()),
                (2, types::LONG, 1, [9, 0, 0, 0]),
                (3, types::UNDEFINED, 5, ((tail_at + 6) as u32).to_le_bytes()),
            ],
            &[1, 0, 2, 0, 3, 0, b'a', b'b', b'c', b'd', b'e'],
        );
        let t = Tiff::new(&b).unwrap();
        let ifd = t.ifd(8, &mut Visited::default(), &Limits::default()).unwrap();
        assert_eq!(t.uints(ifd.get(1).unwrap(), 3, "x"), Ok(vec![1, 2, 3]));
        assert_eq!(t.uints(ifd.get(1).unwrap(), 2, "x"), Err(Error::TooMany("x")));
        assert_eq!(t.uints(ifd.get(2).unwrap(), 1, "x"), Ok(vec![9]));
        assert_eq!(t.uints(ifd.get(3).unwrap(), 9, "x"), Ok(vec![]));
        assert_eq!(t.value_range(ifd.get(3).unwrap()), Ok(Some(tail_at + 6..tail_at + 11)));
        assert_eq!(&b[tail_at + 6..tail_at + 11], b"abcde");
        assert_eq!(t.value_range(ifd.get(2).unwrap()), Ok(None));
        let past = Entry { tag: 4, kind: types::UNDEFINED, count: 100, raw: (tail_at as u32).to_le_bytes() };
        assert_eq!(t.value_range(&past), Err(Error::OutOfRange));
        assert_eq!(t.value_range(&Entry { tag: 4, kind: 99, count: 100, raw: [0; 4] }), Ok(None));
    }

    /// A little-endian structure of IFDs at 8, 8 + 18, …, each with one LONG entry
    /// (tag, value) and a next pointer.
    fn chain(ifds: &[(u16, u32, u32)]) -> Vec<u8> {
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        for &(tag, value, next) in ifds {
            b.extend(1u16.to_le_bytes());
            b.extend(tag.to_le_bytes());
            b.extend(types::LONG.to_le_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend(value.to_le_bytes());
            b.extend(next.to_le_bytes());
        }
        b
    }

    #[test]
    fn walking_follows_chains_and_pointers_within_the_caps() {
        let at = |k: u32| 8 + 18 * k;
        // IFD0 → next IFD1; IFD0's SubIFD is IFD2, whose EXIF pointer is IFD3.
        let b = chain(&[(tags::SUB_IFDS, at(2), at(1)), (7, 0, 0), (tags::EXIF_IFD, at(3), 0), (7, 0, 0)]);
        let t = Tiff::new(&b).unwrap();
        let l = Limits::default();
        let walked = t.walk(8, &[tags::SUB_IFDS, tags::EXIF_IFD], &l, &mut Visited::default()).unwrap();
        assert_eq!(walked.iter().map(|(i, d)| (i.offset, *d)).collect::<Vec<_>>(), vec![(at(0), 1), (at(2), 2), (at(3), 3), (at(1), 1)]);
        // Only the tags asked for are followed.
        assert_eq!(t.walk(8, &[], &l, &mut Visited::default()).unwrap().len(), 2);
        assert_eq!(
            t.walk(8, &[tags::SUB_IFDS, tags::EXIF_IFD], &Limits { max_ifd_depth: 2, ..l }, &mut Visited::default()),
            Err(Error::TooMany("IFD depth"))
        );
        assert_eq!(t.walk(8, &[tags::SUB_IFDS, tags::EXIF_IFD], &Limits { max_ifds: 3, ..l }, &mut Visited::default()), Err(Error::TooMany("IFDs")));
        // A pointer back up the tree, and a chain into itself.
        let looped = chain(&[(tags::SUB_IFDS, at(1), 0), (tags::SUB_IFDS, at(0), 0)]);
        assert_eq!(Tiff::new(&looped).unwrap().walk(8, &[tags::SUB_IFDS], &l, &mut Visited::default()), Err(Error::Loop));
        let selfish = chain(&[(7, 0, at(0))]);
        assert_eq!(Tiff::new(&selfish).unwrap().walk(8, &[], &l, &mut Visited::default()), Err(Error::Loop));
        let outside = chain(&[(tags::SUB_IFDS, 0xffff_0000, 0)]);
        assert_eq!(Tiff::new(&outside).unwrap().walk(8, &[tags::SUB_IFDS], &l, &mut Visited::default()), Err(Error::OutOfRange));
        let mut v = Visited::default();
        assert!(v.is_empty());
        t.walk(8, &[], &l, &mut v).unwrap();
        assert_eq!(v.len(), 2);
    }
}
