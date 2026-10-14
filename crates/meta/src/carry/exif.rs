//! The EXIF block of a rendered copy (`data-model.md` §6): the source's EXIF with Orientation 1,
//! the new pixel size and colour space, the GPS IFD gone when the location is removed, and the
//! thumbnail IFD replaced by a given thumbnail or dropped.
//!
//! The block is edited, never rebuilt from the values it holds: maker notes often hold offsets
//! counted from the TIFF header, so the block keeps the source's layout wherever it can.
//!
//! - **In place** (a JPEG, PNG or WebP source, whose EXIF block holds nothing but metadata; a whole
//!   TIFF file whose maker note would otherwise move): the block is copied up to the end of the
//!   last value it keeps, so every kept IFD, value and maker note stays at its offset. An IFD that
//!   gains entries is appended whole and only the pointer to it changes; an IFD that loses entries
//!   is rewritten in its own place. The bytes of what was removed (the GPS IFD and its values, the
//!   thumbnail, the image-structure tags) are zeroed where nothing kept shares them; for a TIFF
//!   file every byte nothing kept points to is zeroed, since those bytes are pixels.
//! - **Packed** (a TIFF file without a maker note, or one whose metadata is spread past the
//!   container's limit, as when the strips come first): the kept tables and values are copied in
//!   their order with the gaps between them closed, and every offset is moved with them. A maker
//!   note that is itself an IFD whose values sit at offsets counted from the TIFF header, inside
//!   the maker note, has those offsets moved by the same distance; any other maker note is copied
//!   byte for byte.
//!
//! What a rendered copy no longer has is left out: the image-structure tags of IFD0 (strips, tiles,
//! sample layout, compression, sub-IFDs, the DNG tags), the blocks the copy carries on their own
//! (XMP, IPTC, Photoshop resources, the ICC profile) and IFD1 with its thumbnail. ImageWidth and
//! ImageLength, when IFD0 has them, take the new size; PixelXDimension and PixelYDimension are set
//! (an EXIF IFD is added when the block has none), ColorSpace becomes sRGB (output is always sRGB)
//! and an interoperability index other than `R98` becomes `R98`. Everything else, foreign tags
//! included, keeps its bytes.
//!
//! Provenance: written for Dalica from the public TIFF 6.0 (IFD structure, word alignment, the
//! image-structure tags), TIFF/EP and DNG 1.7 (the DNG tag range), and Exif 2.32 (CIPA DC-008: the
//! EXIF, GPS and interoperability IFDs, IFD1 and its JPEG thumbnail) specifications. No other
//! writer's code was consulted.

use std::ops::Range;

use dalica_image::ifd::types;

use crate::exif::read::walk;
use crate::exif::{
    IfdKind, TAG_EXIF_IFD, TAG_GPS_IFD, TAG_ICC, TAG_INTEROP_IFD, TAG_INTEROP_INDEX, TAG_IPTC, TAG_MAKER_NOTE, TAG_ORIENTATION, TAG_PHOTOSHOP,
    TAG_XMP,
};
use crate::{Error, Limits, Result};

pub use crate::exif::edit::MAX_JPEG_EXIF;

const TAG_IMAGE_WIDTH: u16 = 0x0100;
const TAG_IMAGE_LENGTH: u16 = 0x0101;
const TAG_COMPRESSION: u16 = 0x0103;
const TAG_X_RESOLUTION: u16 = 0x011a;
const TAG_Y_RESOLUTION: u16 = 0x011b;
const TAG_RESOLUTION_UNIT: u16 = 0x0128;
const TAG_THUMBNAIL_OFFSET: u16 = 0x0201;
const TAG_THUMBNAIL_LENGTH: u16 = 0x0202;
const TAG_EXIF_VERSION: u16 = 0x9000;
const TAG_COLOR_SPACE: u16 = 0xa001;
const TAG_PIXEL_X: u16 = 0xa002;
const TAG_PIXEL_Y: u16 = 0xa003;

/// IFD0 tags that describe how the source's own pixels are stored (TIFF 6.0, TIFF/EP): a rendered
/// copy has other pixels, so they are left out. 0x0201/0x0202 here are a TIFF's old-style JPEG
/// stream, not a thumbnail.
const IMAGE_STRUCTURE: &[u16] = &[
    0x00fe, 0x00ff, 0x0102, 0x0103, 0x0106, 0x0107, 0x0108, 0x0109, 0x010a, 0x0111, 0x0115, 0x0116, 0x0117, 0x0118, 0x0119, 0x011c, 0x0122, 0x0123,
    0x0124, 0x0125, 0x012d, 0x013d, 0x0140, 0x0142, 0x0143, 0x0144, 0x0145, 0x014a, 0x0152, 0x0153, 0x0154, 0x0155, 0x015b, 0x0200, 0x0201, 0x0202,
    0x0203, 0x0205, 0x0206, 0x0207, 0x0208, 0x0209, 0x0212, 0x828d, 0x828e, 0x9217, 0x935c,
];

/// The DNG tags (DNG 1.0–1.7): they describe raw data the copy does not have.
const DNG_TAGS: Range<u16> = 0xc612..0xcd4a;

/// The value `R98` an interoperability index of an sRGB file holds.
const R98: [u8; 4] = *b"R98\0";

/// What to carry and where it goes.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    /// The copy's pixel size.
    pub width: u32,
    pub height: u32,
    /// Leave the GPS IFD out.
    pub remove_location: bool,
    /// A JPEG thumbnail for IFD1; `None` leaves the copy without one.
    pub thumbnail: Option<&'a [u8]>,
    /// The largest block the target holds ([`MAX_JPEG_EXIF`] for one JPEG APP1).
    pub max_len: usize,
    /// The block is a whole TIFF file (the source's own EXIF structure): bytes nothing kept points
    /// to are pixels and never reach the copy.
    pub whole_file: bool,
}

/// The carried block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub bytes: Vec<u8>,
    /// Whether the thumbnail asked for is in it (it is left out when the block would pass
    /// [`Options::max_len`] with it).
    pub thumbnail: bool,
}

/// The EXIF block of a rendered copy, from the source's block (a TIFF structure from its header
/// on). `Err` when the block cannot be walked (so nothing in it would be carried wrongly) or when
/// what must be kept does not fit [`Options::max_len`].
pub fn carry(block: &[u8], o: &Options<'_>, limits: &Limits) -> Result<Block> {
    if u32::try_from(block.len()).is_err() {
        return Err(Error::TooMany("EXIF bytes"));
    }
    let src = Source::read(block, o.remove_location, limits)?;
    // A metadata-only block keeps its layout; a TIFF file keeps it only for its maker note.
    let first = if !o.whole_file {
        Some(src.build(block, o, Mode::InPlace { strict: false })?)
    } else if src.maker_note().is_some() && src.end(true) <= o.max_len.min(MAX_JPEG_EXIF) {
        Some(src.build(block, o, Mode::InPlace { strict: true })?)
    } else {
        None
    };
    let out = match first {
        Some(out) if out.len() <= o.max_len => out,
        _ => src.build(block, o, Mode::Packed)?,
    };
    if out.len() > o.max_len {
        return Err(Error::TooMany("EXIF bytes"));
    }
    Ok(with_thumbnail(out, &src, o))
}

/// How the kept parts are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// At their offsets in the source. `strict` leaves out entries whose value cannot be accounted
    /// for (an unknown type, an IFD nobody follows), as a packed block must.
    InPlace {
        strict: bool,
    },
    Packed,
}

/// One IFD entry as it lies in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Raw {
    tag: u16,
    kind: u16,
    count: u32,
    /// The value (four bytes or fewer) or its offset, in the block's byte order.
    field: [u8; 4],
    /// Where an out-of-line value lies in the source.
    value: Option<Range<usize>>,
    /// Whether the entry's bytes are all known: a known type, not an IFD this module does not
    /// follow.
    known: bool,
    /// A pointer to an IFD this module follows (EXIF, GPS, interoperability).
    pointer: bool,
}

/// One IFD as it lies in the source.
#[derive(Clone, Debug)]
struct Table {
    kind: IfdKind,
    at: usize,
    /// Entries its place holds.
    room: usize,
    entries: Vec<Raw>,
}

/// The source block, walked.
struct Source {
    little: bool,
    /// IFD0, EXIF, GPS (unless removed) and interoperability, with the entries a copy may keep.
    tables: Vec<Table>,
    /// What a copy never keeps: values of left-out entries, the GPS IFD when removed, IFD1 and the
    /// thumbnail.
    removed: Vec<Range<usize>>,
}

impl Source {
    fn read(block: &[u8], remove_location: bool, limits: &Limits) -> Result<Source> {
        let w = walk(block, limits)?;
        let t = &w.tiff;
        let little = t.is_little_endian();
        let mut tables = Vec::new();
        let mut removed = Vec::new();
        for (kind, ifd) in &w.ifds {
            let at = ifd.offset as usize;
            let table = at..at.saturating_add(2 + 12 * ifd.entries.len() + 4);
            let drop_all = *kind == IfdKind::Thumbnail || (*kind == IfdKind::Gps && remove_location);
            let mut entries = Vec::with_capacity(ifd.entries.len());
            for (i, e) in ifd.entries.iter().enumerate() {
                let field_at = at + 2 + 12 * i + 8;
                let f = block.get(field_at..field_at + 4).ok_or(Error::OutOfRange)?;
                let field = [*f.first().unwrap_or(&0), *f.get(1).unwrap_or(&0), *f.get(2).unwrap_or(&0), *f.get(3).unwrap_or(&0)];
                let size = type_size(e.kind);
                // A value outside the block is not carried: nothing could read it.
                let value = match t.value_range(e) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let pointer = matches!((kind, e.tag), (IfdKind::Image, TAG_EXIF_IFD | TAG_GPS_IFD) | (IfdKind::Exif, TAG_INTEROP_IFD));
                let known = size.is_some() && (e.kind != types::IFD || pointer);
                let raw = Raw { tag: e.tag, kind: e.kind, count: e.count, field, value, known, pointer };
                if drop_all || !kept(*kind, &raw, remove_location) {
                    removed.extend(raw.value.clone());
                } else {
                    entries.push(raw);
                }
            }
            if drop_all {
                removed.push(table);
                if *kind == IfdKind::Thumbnail {
                    let uint = |tag| ifd.get(tag).and_then(|e| t.uint(e)).map(|v| v as usize);
                    if let (Some(at), Some(len)) = (uint(TAG_THUMBNAIL_OFFSET), uint(TAG_THUMBNAIL_LENGTH)) {
                        removed.push(at..at.saturating_add(len).min(block.len()));
                    }
                }
                continue;
            }
            tables.push(Table { kind: *kind, at, room: ifd.entries.len(), entries });
        }
        // A pointer whose IFD was not walked (zero, or the GPS IFD removed) points nowhere.
        let walked: Vec<IfdKind> = tables.iter().map(|t| t.kind).collect();
        for table in &mut tables {
            table.entries.retain(|e| !e.pointer || walked.contains(&pointed(e.tag)));
        }
        Ok(Source { little, tables, removed })
    }

    fn table(&self, kind: IfdKind) -> Option<&Table> {
        self.tables.iter().find(|t| t.kind == kind)
    }

    fn maker_note(&self) -> Option<&Raw> {
        self.table(IfdKind::Exif).and_then(|t| t.entries.iter().find(|e| e.tag == TAG_MAKER_NOTE))
    }

    /// Every byte range a copy keeps: the header, the kept IFDs (their whole place) and the
    /// out-of-line values of their kept entries.
    fn kept(&self, strict: bool) -> Vec<Range<usize>> {
        let mut out = Vec::with_capacity(1 + self.tables.len() * 8);
        out.push(0..8);
        for t in &self.tables {
            out.push(t.at..t.at + 2 + 12 * t.room + 4);
            out.extend(t.entries.iter().filter(|e| e.known || !strict).filter_map(|e| e.value.clone()));
        }
        merge(out)
    }

    /// The end of the last kept byte.
    fn end(&self, strict: bool) -> usize {
        self.kept(strict).last().map_or(8, |r| r.end)
    }

    /// The carried block before its thumbnail.
    fn build(&self, block: &[u8], o: &Options<'_>, mode: Mode) -> Result<Vec<u8>> {
        let strict = !matches!(mode, Mode::InPlace { strict: false });
        let kept = self.kept(strict);
        let layout = match mode {
            Mode::InPlace { .. } => Layout::in_place(&kept),
            Mode::Packed => Layout::packed(&kept),
        };
        let mut ed = Editor { out: layout.copy(block), little: self.little };
        if let Mode::InPlace { .. } = mode {
            // What was removed is zeroed where nothing kept shares it; in a TIFF file, so is every
            // byte nothing kept points to (pixels).
            let gone = if o.whole_file { std::iter::once(0..ed.out.len()).collect() } else { self.removed.clone() };
            for r in gone {
                for piece in subtract(&r, &kept) {
                    ed.zero(piece);
                }
            }
        }

        // The tables, their entries moved with the layout, edited for the copy.
        let mut tables: Vec<(IfdKind, Option<usize>, usize, Vec<Raw>)> = Vec::new();
        for t in &self.tables {
            let mut entries = Vec::with_capacity(t.entries.len());
            for e in t.entries.iter().filter(|e| e.known || !strict) {
                let mut e = e.clone();
                if let Some(v) = &e.value {
                    e.field = ed.u32(layout.offset(v.start)?)?;
                } else if e.pointer {
                    e.field = ed.u32(layout.offset(ed.word(e.field) as usize)?)?;
                }
                entries.push(e);
            }
            tables.push((t.kind, Some(layout.at(t.at)?), t.room, entries));
        }
        if !tables.iter().any(|t| t.0 == IfdKind::Exif) {
            let version = Raw::inline(TAG_EXIF_VERSION, types::UNDEFINED, 4, *b"0232");
            tables.push((IfdKind::Exif, None, 0, vec![version]));
        }
        for (kind, _, _, entries) in &mut tables {
            match kind {
                IfdKind::Image => {
                    set(entries, Raw::inline(TAG_ORIENTATION, types::SHORT, 1, ed.short(1)));
                    for (tag, v) in [(TAG_IMAGE_WIDTH, o.width), (TAG_IMAGE_LENGTH, o.height)] {
                        if entries.iter().any(|e| e.tag == tag) {
                            set(entries, Raw::inline(tag, types::LONG, 1, ed.u32(v)?));
                        }
                    }
                }
                IfdKind::Exif => {
                    set(entries, Raw::inline(TAG_COLOR_SPACE, types::SHORT, 1, ed.short(1)));
                    set(entries, Raw::inline(TAG_PIXEL_X, types::LONG, 1, ed.u32(o.width)?));
                    set(entries, Raw::inline(TAG_PIXEL_Y, types::LONG, 1, ed.u32(o.height)?));
                }
                IfdKind::Interop if entries.iter().any(|e| e.tag == TAG_INTEROP_INDEX && !(e.count == 4 && e.field == R98)) => {
                    set(entries, Raw::inline(TAG_INTEROP_INDEX, types::ASCII, 4, R98));
                }
                _ => {}
            }
        }

        // Children before parents, so each parent learns where its child went.
        let mut ifd0_at = None;
        for kind in [IfdKind::Interop, IfdKind::Gps, IfdKind::Exif, IfdKind::Image] {
            let Some(i) = tables.iter().position(|t| t.0 == kind) else { continue };
            let Some((_, at, room, entries)) = tables.get(i).cloned() else { continue };
            let new_at = ed.commit(at, room, &entries)?;
            let (parent, tag) = match kind {
                IfdKind::Interop => (IfdKind::Exif, TAG_INTEROP_IFD),
                IfdKind::Gps => (IfdKind::Image, TAG_GPS_IFD),
                IfdKind::Exif => (IfdKind::Image, TAG_EXIF_IFD),
                _ => {
                    ifd0_at = Some(new_at);
                    continue;
                }
            };
            let field = ed.u32(new_at)?;
            if let Some(p) = tables.iter_mut().find(|t| t.0 == parent) {
                set(&mut p.3, Raw::inline(tag, types::LONG, 1, field));
            }
        }
        let ifd0_at = ifd0_at.ok_or_else(|| Error::Malformed("an EXIF block without IFD0".into()))?;
        ed.header(ifd0_at)?;

        if mode == Mode::Packed
            && let Some(note) = self.maker_note()
            && let Some(old) = &note.value
        {
            let new = layout.at(old.start)?;
            if new != old.start {
                ed.move_maker_note(old, new);
            }
        }
        Ok(ed.out)
    }
}

impl Raw {
    fn inline(tag: u16, kind: u16, count: u32, field: [u8; 4]) -> Raw {
        Raw { tag, kind, count, field, value: None, known: true, pointer: false }
    }
}

/// Whether a copy keeps an entry of a kept IFD.
fn kept(kind: IfdKind, e: &Raw, remove_location: bool) -> bool {
    if kind != IfdKind::Image {
        return true;
    }
    if remove_location && e.tag == TAG_GPS_IFD {
        return false;
    }
    !IMAGE_STRUCTURE.contains(&e.tag) && !DNG_TAGS.contains(&e.tag) && ![TAG_XMP, TAG_IPTC, TAG_PHOTOSHOP, TAG_ICC].contains(&e.tag)
}

/// The IFD a followed pointer tag points to.
fn pointed(tag: u16) -> IfdKind {
    match tag {
        TAG_EXIF_IFD => IfdKind::Exif,
        TAG_GPS_IFD => IfdKind::Gps,
        _ => IfdKind::Interop,
    }
}

/// Sets `raw` in a sorted entry list: the first entry of its tag replaced (any others removed),
/// else inserted in tag order.
fn set(entries: &mut Vec<Raw>, raw: Raw) {
    let tag = raw.tag;
    match entries.iter().position(|e| e.tag == tag) {
        Some(i) => {
            if let Some(e) = entries.get_mut(i) {
                *e = raw;
            }
            let mut seen = false;
            entries.retain(|e| {
                if e.tag != tag {
                    return true;
                }
                let first = !seen;
                seen = true;
                first
            });
        }
        None => {
            let at = entries.iter().position(|e| e.tag > tag).unwrap_or(entries.len());
            entries.insert(at, raw);
        }
    }
}

/// Bytes per value of a TIFF field type.
fn type_size(kind: u16) -> Option<usize> {
    match kind {
        types::BYTE | types::ASCII | types::SBYTE | types::UNDEFINED => Some(1),
        types::SHORT | types::SSHORT => Some(2),
        types::LONG | types::SLONG | types::FLOAT | types::IFD => Some(4),
        types::RATIONAL | types::SRATIONAL | types::DOUBLE => Some(8),
        _ => None,
    }
}

/// Sorted ranges with overlapping and touching ones joined.
fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// The parts of `r` outside the sorted, joined `kept` ranges.
fn subtract(r: &Range<usize>, kept: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut at = r.start;
    // The kept ranges are sorted and disjoint: start at the first that ends after `r` starts.
    let first = kept.partition_point(|k| k.end <= r.start);
    for k in kept.get(first..).unwrap_or_default() {
        if k.start >= r.end {
            break;
        }
        if k.start > at {
            out.push(at..k.start);
        }
        at = at.max(k.end);
    }
    if at < r.end {
        out.push(at..r.end);
    }
    out
}

/// Where the kept ranges go: `(old range, new start)`, sorted.
struct Layout {
    parts: Vec<(Range<usize>, usize)>,
}

impl Layout {
    /// Every kept byte at its own offset.
    fn in_place(kept: &[Range<usize>]) -> Layout {
        let end = kept.last().map_or(8, |r| r.end);
        Layout { parts: vec![(0..end, 0)] }
    }

    /// The kept ranges one after the other, each at an offset of the same parity as before (so
    /// word-aligned values stay aligned).
    fn packed(kept: &[Range<usize>]) -> Layout {
        let mut parts = Vec::with_capacity(kept.len());
        let mut at = 0usize;
        for r in kept {
            if at % 2 != r.start % 2 {
                at += 1;
            }
            parts.push((r.clone(), at));
            at += r.len();
        }
        Layout { parts }
    }

    /// Where the kept byte `old` goes.
    fn offset(&self, old: usize) -> Result<u32> {
        let i = self.parts.partition_point(|(r, _)| r.start <= old);
        let (r, new) = i.checked_sub(1).and_then(|i| self.parts.get(i)).ok_or(Error::OutOfRange)?;
        if old >= r.end {
            return Err(Error::OutOfRange);
        }
        u32::try_from(new + (old - r.start)).map_err(|_| Error::TooMany("EXIF bytes"))
    }

    fn at(&self, old: usize) -> Result<usize> {
        self.offset(old).map(|v| v as usize)
    }

    /// The new block: every kept range copied to its place, zeros elsewhere.
    fn copy(&self, block: &[u8]) -> Vec<u8> {
        let len = self.parts.last().map_or(0, |(r, new)| new + r.len());
        let mut out = vec![0u8; len];
        for (r, new) in &self.parts {
            // In place, the whole prefix is copied: unclaimed bytes are dealt with by the caller.
            let src = block.get(r.clone()).unwrap_or_default();
            if let Some(dst) = out.get_mut(*new..*new + src.len()) {
                dst.copy_from_slice(src);
            }
        }
        out
    }
}

/// The block being written, in the source's byte order.
struct Editor {
    out: Vec<u8>,
    little: bool,
}

impl Editor {
    fn u16(&self, v: u16) -> [u8; 2] {
        if self.little { v.to_le_bytes() } else { v.to_be_bytes() }
    }

    /// A SHORT in a four-byte field.
    fn short(&self, v: u16) -> [u8; 4] {
        let [a, b] = self.u16(v);
        [a, b, 0, 0]
    }

    fn u32<T: TryInto<u32>>(&self, v: T) -> Result<[u8; 4]> {
        let v = v.try_into().map_err(|_| Error::TooMany("EXIF bytes"))?;
        Ok(if self.little { v.to_le_bytes() } else { v.to_be_bytes() })
    }

    fn word(&self, b: [u8; 4]) -> u32 {
        if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
    }

    fn read_u16(&self, at: usize) -> Option<u16> {
        let b = self.out.get(at..at + 2)?;
        let b = [*b.first()?, *b.get(1)?];
        Some(if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }

    fn read_u32(&self, at: usize) -> Option<u32> {
        let b = self.out.get(at..at + 4)?;
        Some(self.word([*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?]))
    }

    fn write_at(&mut self, at: usize, bytes: &[u8]) {
        if let Some(dst) = self.out.get_mut(at..at.saturating_add(bytes.len())) {
            dst.copy_from_slice(bytes);
        }
    }

    fn zero(&mut self, r: Range<usize>) {
        if let Some(dst) = self.out.get_mut(r) {
            dst.fill(0);
        }
    }

    /// Appends `data` word aligned; its offset.
    fn append(&mut self, data: &[u8]) -> Result<usize> {
        if self.out.len() % 2 == 1 {
            self.out.push(0);
        }
        let at = self.out.len();
        if u32::try_from(at.saturating_add(data.len())).is_err() {
            return Err(Error::TooMany("EXIF bytes"));
        }
        self.out.extend_from_slice(data);
        Ok(at)
    }

    /// The standard header in the block's byte order, pointing at IFD0.
    fn header(&mut self, ifd0: usize) -> Result<()> {
        let magic: &[u8; 4] = if self.little { b"II*\0" } else { b"MM\0*" };
        let pointer = self.u32(ifd0)?;
        self.write_at(0, magic);
        self.write_at(4, &pointer);
        Ok(())
    }

    /// Writes an IFD (no next IFD): in its place when its entries fit there (the freed tail zeroed),
    /// else appended (its old place zeroed). Where it is.
    fn commit(&mut self, at: Option<usize>, room: usize, entries: &[Raw]) -> Result<usize> {
        let n = u16::try_from(entries.len()).map_err(|_| Error::TooMany("IFD entries"))?;
        let mut table = Vec::with_capacity(2 + 12 * entries.len() + 4);
        table.extend(self.u16(n));
        for e in entries {
            table.extend(self.u16(e.tag));
            table.extend(self.u16(e.kind));
            table.extend(self.u32(e.count)?);
            table.extend(e.field);
        }
        table.extend([0u8; 4]);
        match at {
            Some(at) if entries.len() <= room => {
                self.write_at(at, &table);
                let freed = at + table.len();
                self.zero(freed..freed + 12 * (room - entries.len()));
                Ok(at)
            }
            _ => {
                if let Some(at) = at {
                    self.zero(at..at + 2 + 12 * room + 4);
                }
                self.append(&table)
            }
        }
    }

    /// Appends an IFD of `entries` with the given next-IFD pointer bytes; where it is.
    fn append_table(&mut self, entries: &[Raw], next: &[u8]) -> Result<usize> {
        let n = u16::try_from(entries.len()).map_err(|_| Error::TooMany("IFD entries"))?;
        let mut table = Vec::with_capacity(2 + 12 * entries.len() + 4);
        table.extend(self.u16(n));
        for e in entries {
            table.extend(self.u16(e.tag));
            table.extend(self.u16(e.kind));
            table.extend(self.u32(e.count)?);
            table.extend(e.field);
        }
        table.extend_from_slice(next);
        self.append(&table)
    }

    /// A packed maker note that is an IFD whose out-of-line values all lie inside the maker note at
    /// offsets counted from the TIFF header: those offsets moved with it from `old` to `new`.
    /// Anything else is left byte for byte.
    fn move_maker_note(&mut self, old: &Range<usize>, new: usize) {
        let Some(count) = self.read_u16(new).map(usize::from) else { return };
        if count == 0 || 2 + 12 * count + 4 > old.len() {
            return;
        }
        let mut fields = Vec::with_capacity(count);
        for i in 0..count {
            let e = new + 2 + 12 * i;
            let (Some(kind), Some(n), Some(offset)) = (self.read_u16(e + 2), self.read_u32(e + 4), self.read_u32(e + 8)) else { return };
            let Some(len) = type_size(kind).and_then(|s| s.checked_mul(n as usize)) else { return };
            if len <= 4 {
                continue;
            }
            let offset = offset as usize;
            let inside = offset >= old.start + 2 + 12 * count && offset.checked_add(len).is_some_and(|end| end <= old.end);
            if !inside {
                return;
            }
            fields.push((e + 8, offset - old.start + new));
        }
        for (at, offset) in fields {
            if let Ok(v) = self.u32(offset) {
                self.write_at(at, &v);
            }
        }
    }
}

/// The block with IFD0's Orientation set to `orientation`, nothing else changed: the value written
/// into the entry when there is one (an entry of another type or count becomes one SHORT, in the
/// same twelve bytes); else IFD0 appended with the new entry and only the header's pointer changed.
/// No block: a new one holding only the orientation. `Err(TooMany)` past `max_len`.
pub fn set_orientation(block: Option<&[u8]>, orientation: u16, max_len: usize, limits: &Limits) -> Result<Vec<u8>> {
    let Some(block) = block else {
        let mut ed = Editor { out: b"II*\0\x08\0\0\0".to_vec(), little: true };
        let entry = Raw::inline(TAG_ORIENTATION, types::SHORT, 1, ed.short(orientation));
        ed.commit(None, 0, &[entry])?;
        return Ok(ed.out);
    };
    if u32::try_from(block.len()).is_err() {
        return Err(Error::TooMany("EXIF bytes"));
    }
    let w = walk(block, limits)?;
    let little = w.tiff.is_little_endian();
    let Some((_, ifd0)) = w.ifds.iter().find(|(k, _)| *k == IfdKind::Image) else {
        return Err(Error::Malformed("an EXIF block without IFD0".into()));
    };
    let at = ifd0.offset as usize;
    let mut ed = Editor { out: block.to_vec(), little };
    let value = ed.short(orientation);
    if let Some(i) = ifd0.entries.iter().position(|e| e.tag == TAG_ORIENTATION) {
        let e = at + 2 + 12 * i;
        let (kind, count) = (ed.u16(types::SHORT), ed.u32(1u32)?);
        ed.write_at(e + 2, &kind);
        ed.write_at(e + 4, &count);
        ed.write_at(e + 8, &value);
        return Ok(ed.out);
    }
    let mut entries = Vec::with_capacity(ifd0.entries.len() + 1);
    for (i, e) in ifd0.entries.iter().enumerate() {
        let f = at + 2 + 12 * i + 8;
        let field = block.get(f..f + 4).ok_or(Error::OutOfRange)?;
        let field = [*field.first().unwrap_or(&0), *field.get(1).unwrap_or(&0), *field.get(2).unwrap_or(&0), *field.get(3).unwrap_or(&0)];
        entries.push(Raw::inline(e.tag, e.kind, e.count, field));
    }
    set(&mut entries, Raw::inline(TAG_ORIENTATION, types::SHORT, 1, value));
    // The appended IFD0 keeps the old one's next pointer, so IFD1 and the thumbnail stay reachable.
    let next = block.get(at + 2 + 12 * ifd0.entries.len()..at + 6 + 12 * ifd0.entries.len()).ok_or(Error::OutOfRange)?.to_vec();
    let new_at = ed.append_table(&entries, &next)?;
    let pointer = ed.u32(new_at)?;
    ed.write_at(4, &pointer);
    if ed.out.len() > max_len {
        return Err(Error::TooMany("EXIF bytes for the orientation"));
    }
    Ok(ed.out)
}

/// `out` with IFD1 and the thumbnail appended when asked for and they fit.
fn with_thumbnail(out: Vec<u8>, src: &Source, o: &Options<'_>) -> Block {
    let Some(thumb) = o.thumbnail.filter(|t| !t.is_empty()) else { return Block { bytes: out, thumbnail: false } };
    let fallback = out.clone();
    match append_thumbnail(out, src.little, thumb) {
        Ok(bytes) if bytes.len() <= o.max_len => Block { bytes, thumbnail: true },
        _ => Block { bytes: fallback, thumbnail: false },
    }
}

fn append_thumbnail(out: Vec<u8>, little: bool, thumb: &[u8]) -> Result<Vec<u8>> {
    let mut ed = Editor { out, little };
    let ifd0 = ed.read_u32(4).ok_or(Error::Truncated)? as usize;
    let n = usize::from(ed.read_u16(ifd0).ok_or(Error::Truncated)?);
    let at = ed.append(thumb)?;
    let mut resolution = Vec::with_capacity(16);
    for v in [72u32, 1, 72, 1] {
        resolution.extend(ed.u32(v)?);
    }
    let res = ed.append(&resolution)?;
    let entries = [
        Raw::inline(TAG_COMPRESSION, types::SHORT, 1, ed.short(6)),
        Raw::inline(TAG_X_RESOLUTION, types::RATIONAL, 1, ed.u32(res)?),
        Raw::inline(TAG_Y_RESOLUTION, types::RATIONAL, 1, ed.u32(res + 8)?),
        Raw::inline(TAG_RESOLUTION_UNIT, types::SHORT, 1, ed.short(2)),
        Raw::inline(TAG_THUMBNAIL_OFFSET, types::LONG, 1, ed.u32(at)?),
        Raw::inline(TAG_THUMBNAIL_LENGTH, types::LONG, 1, ed.u32(thumb.len())?),
    ];
    let ifd1 = ed.commit(None, 0, &entries)?;
    let next = ed.u32(ifd1)?;
    ed.write_at(ifd0 + 2 + 12 * n, &next);
    Ok(ed.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_merge_and_subtract() {
        assert_eq!(merge(vec![5..9, 0..2, 2..4, 8..12, 20..20]), vec![0..4, 5..12]);
        let kept = merge(vec![2..4, 6..8]);
        assert_eq!(subtract(&(0..10), &kept), vec![0..2, 4..6, 8..10]);
        assert_eq!(subtract(&(2..4), &kept), Vec::<Range<usize>>::new());
        assert_eq!(subtract(&(3..7), &kept), vec![4..6]);
    }

    #[test]
    fn packing_keeps_parity_and_maps_offsets() {
        let l = Layout::packed(&[0..8, 9..13, 100..110]);
        assert_eq!(l.parts, vec![(0..8, 0), (9..13, 9), (100..110, 14)]);
        assert_eq!(l.offset(10).unwrap(), 10);
        assert_eq!(l.offset(105).unwrap(), 19);
        assert!(l.offset(50).is_err());
        assert!(l.offset(110).is_err());
    }

    #[test]
    fn set_replaces_the_first_and_drops_repeats() {
        let r = |tag, v: u8| Raw::inline(tag, types::SHORT, 1, [v, 0, 0, 0]);
        let mut e = vec![r(1, 0), r(5, 0), r(5, 1), r(9, 0)];
        set(&mut e, r(5, 7));
        assert_eq!(e, vec![r(1, 0), r(5, 7), r(9, 0)]);
        set(&mut e, r(3, 3));
        assert_eq!(e.iter().map(|e| e.tag).collect::<Vec<_>>(), [1, 3, 5, 9]);
    }
}
