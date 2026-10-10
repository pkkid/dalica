//! A bounded TIFF/IFD reader (TIFF 6.0 structure, written from the public specification), shared
//! by EXIF reading here, `dalica-meta` and RAW previews later.
//!
//! Every offset and count comes from the file and is treated as hostile (`security.md` §4.1):
//! ranges are checked before they are read, multiplications are checked, the number of entries
//! per IFD and of IFDs per structure is capped, sub-IFD depth is capped, and every IFD offset is
//! remembered so a loop is an error rather than a hang.

use std::borrow::Cow;
use std::collections::HashSet;

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

impl<'a> Tiff<'a> {
    /// Checks the header; `Err(NotTiff)` unless `data` starts with a TIFF header.
    pub fn new(data: &'a [u8]) -> Result<Tiff<'a>> {
        let order = match data.get(..4) {
            Some(b"II*\0") => Order::Little,
            Some(b"MM\0*") => Order::Big,
            _ => return Err(Error::Malformed("not a TIFF header".into())),
        };
        Ok(Tiff { data, order })
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
        if offset < 8 {
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
}
