//! ISO base media file format (ISO/IEC 14496-12) boxes, shared by CR3, HEIF and AVIF: the `ftyp`
//! brands and a bounded box walk.
//!
//! Provenance: written from the public description of the box structure (ISO/IEC 14496-12 §4.2,
//! as summarised in many open format notes): a box is a 32-bit big-endian size, a four-character
//! type, a 64-bit `largesize` when the size is 1, a 16-byte `usertype` when the type is `uuid`,
//! then its payload; size 0 means "to the end of the enclosing data". No code was consulted.
//!
//! Every size is hostile (`security.md` §4.2): a box must lie inside its parent (one running past
//! the end of the bytes is [`Error::Truncated`], so a partial read can read on; one running past
//! its parent is [`Error::Malformed`]); a size smaller than its own header, which would never
//! advance the walk, is malformed; nesting is capped at [`MAX_DEPTH`], boxes per level at
//! [`MAX_BOXES`], and every box read is counted by the caller's [`Work`]. Nothing is allocated:
//! boxes are ranges of the bytes.

use std::ops::Range;

use super::u32_be;
use crate::limits::Work;
use crate::{Error, Result};

/// Deepest nesting of boxes followed (top-level boxes are depth 0).
pub const MAX_DEPTH: usize = 12;

/// Most boxes read in one level (one parent's children, or the top level).
pub const MAX_BOXES: usize = 4096;

/// The major brand of a file that starts with an `ftyp` box, read from its first 12 bytes.
pub fn major_brand(head: &[u8]) -> Option<[u8; 4]> {
    if head.get(4..8) != Some(b"ftyp") {
        return None;
    }
    match head.get(8..12) {
        Some(&[a, b, c, d]) => Some([a, b, c, d]),
        _ => None,
    }
}

/// The compatible brands of a file that starts with an `ftyp` box (those inside the bytes; none
/// when the box is not there).
pub fn compatible_brands(head: &[u8]) -> &[[u8; 4]] {
    if head.get(4..8) != Some(b"ftyp") {
        return &[];
    }
    let size = u32_be(head, 0).map_or(0, |s| s as usize);
    let end = size.min(head.len());
    head.get(16..end).map_or(&[], |b| b.as_chunks::<4>().0)
}

/// One box: its type, `usertype` for `uuid` boxes, and where it and its payload lie in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxRef {
    pub kind: [u8; 4],
    pub uuid: Option<[u8; 16]>,
    /// The whole box, header included.
    pub range: Range<usize>,
    /// The payload, after the header (and the `usertype`).
    pub body: Range<usize>,
    /// 0 for top-level boxes.
    pub depth: usize,
}

/// The boxes laid end to end in one range of the file, read one at a time with [`Boxes::next`].
#[derive(Debug)]
pub struct Boxes<'a> {
    bytes: &'a [u8],
    pos: usize,
    end: usize,
    depth: usize,
    count: usize,
}

impl<'a> Boxes<'a> {
    /// The top-level boxes of `bytes`.
    pub fn top(bytes: &'a [u8]) -> Boxes<'a> {
        Boxes { bytes, pos: 0, end: bytes.len(), depth: 0, count: 0 }
    }

    /// The boxes in `parent`'s payload, starting `skip` bytes into it (a full box's version and
    /// flags, a sample description's entry count). `Err(TooMany)` past [`MAX_DEPTH`].
    pub fn children(bytes: &'a [u8], parent: &BoxRef, skip: usize) -> Result<Boxes<'a>> {
        let depth = parent.depth + 1;
        if depth > MAX_DEPTH {
            return Err(Error::TooMany("nested boxes"));
        }
        let pos = parent.body.start.checked_add(skip).filter(|&p| p <= parent.body.end);
        let pos = pos.ok_or_else(|| Error::Malformed("a box's payload is too short".into()))?;
        Ok(Boxes { bytes, pos, end: parent.body.end, depth, count: 0 })
    }

    /// The next box, `Ok(None)` at the end of the range.
    pub fn next(&mut self, work: &mut Work) -> Result<Option<BoxRef>> {
        if self.pos >= self.end {
            return Ok(None);
        }
        self.count += 1;
        if self.count > MAX_BOXES {
            return Err(Error::TooMany("boxes in one level"));
        }
        work.step()?;
        let start = self.pos;
        // Cut by the end of the bytes: truncation; by the parent's end: damage.
        let past = |at: usize| if at > self.bytes.len() { Error::Truncated } else { Error::Malformed("a box runs past its parent".into()) };
        let fits = |at: usize| if at > self.end { Err(past(at)) } else { Ok(()) };
        fits(start.saturating_add(8))?;
        let kind = match self.bytes.get(start + 4..start + 8) {
            Some(&[a, b, c, d]) => [a, b, c, d],
            _ => return Err(Error::Truncated),
        };
        let (size, mut header) = match u32_be(self.bytes, start)? {
            0 => ((self.end - start) as u64, 8usize),
            1 => {
                fits(start.saturating_add(16))?;
                let (hi, lo) = (u32_be(self.bytes, start + 8)?, u32_be(self.bytes, start + 12)?);
                ((u64::from(hi) << 32) | u64::from(lo), 16)
            }
            n => (u64::from(n), 8),
        };
        let name = || String::from_utf8_lossy(&kind).into_owned();
        if size < header as u64 {
            return Err(Error::Malformed(format!("a {} box of {size} bytes", name())));
        }
        let end = usize::try_from(size).ok().and_then(|s| start.checked_add(s)).unwrap_or(usize::MAX);
        fits(end)?;
        let uuid = if &kind == b"uuid" {
            let at = start + header;
            header += 16;
            if end - start < header {
                return Err(Error::Malformed("a uuid box without its usertype".into()));
            }
            self.bytes.get(at..at + 16).and_then(|b| <[u8; 16]>::try_from(b).ok())
        } else {
            None
        };
        self.pos = end;
        Ok(Some(BoxRef { kind, uuid, range: start..end, body: start + header..end, depth: self.depth }))
    }

    /// The first box of type `kind` (and `usertype` `uuid`, if given) from here on.
    pub fn find(&mut self, kind: &[u8; 4], uuid: Option<&[u8; 16]>, work: &mut Work) -> Result<Option<BoxRef>> {
        while let Some(b) = self.next(work)? {
            if &b.kind == kind && (uuid.is_none() || b.uuid.as_ref() == uuid) {
                return Ok(Some(b));
            }
        }
        Ok(None)
    }
}

/// The first child of `parent` of type `kind`, `skip` bytes into its payload.
pub fn child(bytes: &[u8], parent: &BoxRef, skip: usize, kind: &[u8; 4], work: &mut Work) -> Result<Option<BoxRef>> {
    Boxes::children(bytes, parent, skip)?.find(kind, None, work)
}

/// Follows a path of box types down from `parent` (the first match at each level).
pub fn path(bytes: &[u8], parent: &BoxRef, kinds: &[&[u8; 4]], work: &mut Work) -> Result<Option<BoxRef>> {
    let mut at = parent.clone();
    for kind in kinds {
        match child(bytes, &at, 0, kind, work)? {
            Some(b) => at = b,
            None => return Ok(None),
        }
    }
    Ok(Some(at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    fn work() -> Work {
        Work::new(10_000, "boxes")
    }

    fn all(bytes: &[u8]) -> Result<Vec<BoxRef>> {
        let mut out = Vec::new();
        let mut w = work();
        let mut it = Boxes::top(bytes);
        while let Some(b) = it.next(&mut w)? {
            out.push(b);
        }
        Ok(out)
    }

    #[test]
    fn the_brands_follow_ftyp() {
        assert_eq!(major_brand(b"\0\0\0\x18ftypheic\0\0\0\0"), Some(*b"heic"));
        assert_eq!(major_brand(b"\0\0\0\x18ftyphei"), None);
        assert_eq!(major_brand(b"\0\0\0\x18moovheic"), None);
        assert_eq!(compatible_brands(b"\0\0\0\x18ftypmif1\0\0\0\0mif1avif"), [*b"mif1", *b"avif"]);
        // The box says 20 bytes: one compatible brand, whatever follows.
        assert_eq!(compatible_brands(b"\0\0\0\x14ftypmif1\0\0\0\0mif1avif"), [*b"mif1"]);
        assert!(compatible_brands(b"\0\0\0\x18moovmif1").is_empty());
    }

    #[test]
    fn sizes_and_uuids() {
        let uuid = [7u8; 16];
        let mut u = 40u32.to_be_bytes().to_vec();
        u.extend(b"uuid");
        u.extend(uuid);
        u.extend(b"payload!payload!");
        let mut large = 1u32.to_be_bytes().to_vec();
        large.extend(b"mdat");
        large.extend(20u64.to_be_bytes());
        large.extend(b"abcd");
        let bytes = [bx(b"ftyp", b"heic\0\0\0\0"), u, large, bx(b"free", b"")].concat();
        let boxes = all(&bytes).unwrap();
        let kinds: Vec<_> = boxes.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, [*b"ftyp", *b"uuid", *b"mdat", *b"free"]);
        assert_eq!(boxes[1].uuid, Some(uuid));
        assert_eq!(&bytes[boxes[1].body.clone()], b"payload!payload!");
        assert_eq!(&bytes[boxes[2].body.clone()], b"abcd");
        assert!(boxes[3].body.is_empty());
        let mut it = Boxes::top(&bytes);
        assert_eq!(it.find(b"uuid", Some(&[8; 16]), &mut work()), Ok(None));
        // Size 0: to the end.
        let mut to_end = 0u32.to_be_bytes().to_vec();
        to_end.extend(b"mdat0123");
        assert_eq!(all(&to_end).unwrap()[0].body, 8..12);
    }

    #[test]
    fn children_and_paths() {
        let bytes = bx(b"moov", &[bx(b"mvhd", b"x"), bx(b"trak", &bx(b"mdia", &bx(b"minf", b"leaf")))].concat());
        let moov = all(&bytes).unwrap().remove(0);
        let minf = path(&bytes, &moov, &[b"trak", b"mdia", b"minf"], &mut work()).unwrap().unwrap();
        assert_eq!((minf.depth, &bytes[minf.body.clone()]), (3, &b"leaf"[..]));
        assert_eq!(path(&bytes, &moov, &[b"trak", b"stbl"], &mut work()), Ok(None));
        assert!(Boxes::children(&bytes, &moov, 1000).is_err());
    }

    /// Sizes that never advance, run past the parent or the bytes, or nest too deep are errors.
    #[test]
    fn hostile_sizes_and_nesting() {
        for size in [2u32, 7] {
            let mut b = size.to_be_bytes().to_vec();
            b.extend(b"free0000");
            assert!(matches!(all(&b), Err(Error::Malformed(_))), "size {size}");
        }
        let mut large = 1u32.to_be_bytes().to_vec();
        large.extend(b"mdat");
        large.extend(8u64.to_be_bytes());
        assert!(matches!(all(&large), Err(Error::Malformed(_))));
        large.truncate(12);
        assert_eq!(all(&large), Err(Error::Truncated));
        // Past the end of the bytes: truncated (a partial read reads on); huge: never wraps.
        assert_eq!(all(&bx(b"moov", b"abc")[..9]), Err(Error::Truncated));
        let mut huge = 1u32.to_be_bytes().to_vec();
        huge.extend(b"mdat");
        huge.extend(u64::MAX.to_be_bytes());
        assert_eq!(all(&huge), Err(Error::Truncated));
        // A child running past its parent (but not past the bytes): malformed.
        let mut parent = bx(b"moov", &bx(b"trak", b"0123"));
        parent[11] = 40;
        parent.extend([0u8; 40]);
        let moov = all(&parent[..20]).unwrap().remove(0);
        let mut it = Boxes::children(&parent, &moov, 0).unwrap();
        assert!(matches!(it.next(&mut work()), Err(Error::Malformed(_))));
        // A uuid box too short for its usertype.
        assert!(matches!(all(&bx(b"uuid", b"short")), Err(Error::Malformed(_))));
        // Thirteen levels of nesting: refused at the cap.
        let mut nested = bx(b"leaf", b"");
        for _ in 0..13 {
            nested = bx(b"moov", &nested);
        }
        let mut at = all(&nested).unwrap().remove(0);
        let mut w = work();
        let deep = loop {
            match Boxes::children(&nested, &at, 0).and_then(|mut c| c.next(&mut w)) {
                Ok(Some(b)) => at = b,
                other => break other,
            }
        };
        assert_eq!(deep, Err(Error::TooMany("nested boxes")));
        assert_eq!(at.depth, MAX_DEPTH);
    }

    #[test]
    fn boxes_per_level_and_work_are_capped() {
        let one = bx(b"free", b"");
        let many = one.repeat(MAX_BOXES + 1);
        assert_eq!(all(&many), Err(Error::TooMany("boxes in one level")));
        assert_eq!(all(&one.repeat(MAX_BOXES)).map(|b| b.len()), Ok(MAX_BOXES));
        let mut w = Work::new(3, "ISOBMFF boxes");
        let mut it = Boxes::top(&many);
        assert!(it.next(&mut w).is_ok() && it.next(&mut w).is_ok() && it.next(&mut w).is_ok());
        assert_eq!(it.next(&mut w), Err(Error::TooMany("ISOBMFF boxes")));
    }
}
