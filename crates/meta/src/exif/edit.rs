//! Editing an EXIF block in place (`xmp-interop.md` §2–§3): the date taken (DateTimeOriginal and
//! DateTimeDigitized with their OffsetTime and SubSecTime), the place (GPS latitude and longitude
//! with their references) and the ImageDescription copy of the caption.
//!
//! EXIF is edited in place or appended, never repacked, because maker notes often hold offsets
//! counted from the TIFF header, so nothing that is there may move:
//!
//! - a value equal to the one the entry holds is left where it is, even when other entries share
//!   its bytes (so writing the same values twice gives the same block);
//! - a changed value no larger than the old one is written where the old one was (the rest of
//!   its bytes zeroed); a value of four bytes or fewer goes into its entry;
//! - a new or larger value is appended at the end of the block (word aligned) and only the entry's
//!   offset changes;
//! - an IFD that gains an entry is appended whole (its out-of-line values stay where they are) and
//!   only the pointer to it changes: the EXIF or GPS IFD pointer in IFD0, or the header's IFD0
//!   offset; an IFD that loses entries is rewritten in its own place;
//! - the old bytes of a value Dalica replaced elsewhere or removed are zeroed (so a removed place
//!   does not linger in the file), but only when no other entry's value, IFD, header or thumbnail
//!   shares them;
//! - every other tag, IFD, maker note and thumbnail keeps its bytes and its offset.
//!
//! Decisions for the fields:
//!
//! - Dates: both DateTimeOriginal and DateTimeDigitized get the new date; OffsetTimeOriginal /
//!   OffsetTimeDigitized and SubSecTimeOriginal / SubSecTimeDigitized are set from the date's
//!   offset and fraction, and removed when it has none, so the block reads back exactly the date
//!   asked for. A new EXIF IFD starts with ExifVersion 0232.
//! - Place: latitude and longitude as degree/minute/second rationals (whole degrees and minutes,
//!   seconds in millionths: about 3·10⁻¹⁰° of rounding), the references `N`/`S` and `E`/`W`.
//!   GPSVersionID 2.3.0.0 is added when the GPS IFD has none; an existing one is kept. Removing the
//!   place removes the four latitude/longitude tags and nothing else.
//! - Caption: an ImageDescription that reads as a caption (not a camera placeholder) is a copy of
//!   it, so it follows the caption: replaced by a new caption, removed with a cleared one (else the
//!   precedence rules would read the old text back as the caption). A placeholder, or no
//!   ImageDescription, is left alone; Dalica never adds one.
//!
//! Provenance: written for Dalica from the public TIFF 6.0 (IFD structure, word alignment) and
//! Exif 2.32 (CIPA DC-008: the date, offset, sub-second and GPS tags and their formats)
//! specifications. No other writer's code was consulted.

use std::ops::Range;

use dalica_core::DateTime;
use dalica_image::ifd::types;

use super::read::{entry_text, is_placeholder, walk};
use super::*;
use crate::fields::{DateTaken, Place, clean_text};
use crate::write::Changes;
use crate::{Error, Limits, Result};

/// The largest TIFF structure one JPEG APP1 segment holds: 65,533 payload bytes, less the
/// `Exif\0\0` header (`xmp-interop.md` §4).
pub const MAX_JPEG_EXIF: usize = 65_533 - 6;

/// ExifVersion, written as `0232` in a new EXIF IFD.
const TAG_EXIF_VERSION: u16 = 0x9000;

/// The block Dalica starts from when a file has no EXIF: a little-endian header and an empty IFD0.
const EMPTY: [u8; 14] = *b"II*\0\x08\0\0\0\0\0\0\0\0\0";

/// What to change in EXIF. `None` leaves a field as the block has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExifEdit {
    /// Followed by an ImageDescription that is a copy of the caption (see the module docs).
    pub caption: Option<Option<String>>,
    pub date_taken: Option<DateTaken>,
    /// `Some(None)` removes the place.
    pub place: Option<Option<Place>>,
}

impl ExifEdit {
    /// The EXIF part of a write: the caption, the date taken and the place.
    pub fn of(changes: &Changes) -> ExifEdit {
        ExifEdit { caption: changes.caption.clone(), date_taken: changes.date_taken.clone(), place: changes.place }
    }

    pub fn is_empty(&self) -> bool {
        self.caption.is_none() && self.date_taken.is_none() && self.place.is_none()
    }
}

/// `block` (a TIFF structure from its header on, or `None` when the file has no EXIF) with `edit`
/// written in place or appended (see the module docs). `Ok(None)` when there is no block and
/// nothing that needs one (only a caption, or removing a place).
///
/// `max_len` is the container's limit on the block ([`MAX_JPEG_EXIF`] for JPEG): an edit that
/// would grow the block past it fails with [`Error::TooMany`] naming the field that does not fit,
/// and the caller keeps the file as it was. `Err` too when the block cannot be read, so nothing in
/// it would be lost.
pub fn edit(block: Option<&[u8]>, edit: &ExifEdit, max_len: usize, limits: &Limits) -> Result<Option<Vec<u8>>> {
    let old_len = block.map_or(0, <[u8]>::len);
    let too_big = |out: &Option<Vec<u8>>| out.as_ref().is_some_and(|o| o.len() > max_len && o.len() > old_len);
    let out = apply(block, edit, limits)?;
    if !too_big(&out) {
        return Ok(out);
    }
    // Name the first field that does not fit, in the order they are written.
    let caption = ExifEdit { caption: edit.caption.clone(), ..ExifEdit::default() };
    if too_big(&apply(block, &caption, limits)?) {
        return Err(Error::TooMany("EXIF bytes for the caption"));
    }
    let date = ExifEdit { date_taken: edit.date_taken.clone(), ..caption };
    if too_big(&apply(block, &date, limits)?) {
        return Err(Error::TooMany("EXIF bytes for the date taken"));
    }
    Err(Error::TooMany("EXIF bytes for the place"))
}

fn apply(block: Option<&[u8]>, edit: &ExifEdit, limits: &Limits) -> Result<Option<Vec<u8>>> {
    let needs_block = edit.date_taken.is_some() || matches!(edit.place, Some(Some(_)));
    let base = match block {
        Some(b) => b,
        None if needs_block => &EMPTY[..],
        None => return Ok(None),
    };
    let mut ed = Editor::new(base, limits)?;
    let mut ifd0 = ed.table(IfdKind::Image)?;

    if let Some(caption) = &edit.caption
        && ed.description_is_caption
    {
        match caption.as_deref().and_then(|c| clean_text(c, limits.max_string)) {
            Some(c) => ed.set(&mut ifd0, TAG_IMAGE_DESCRIPTION, types::ASCII, &ascii(&c))?,
            None => ed.remove(&mut ifd0, TAG_IMAGE_DESCRIPTION),
        }
    }

    if let Some(date) = &edit.date_taken {
        let mut exif = ed.table(IfdKind::Exif)?;
        if exif.at.is_none() {
            ed.set(&mut exif, TAG_EXIF_VERSION, types::UNDEFINED, b"0232")?;
        }
        let text = ascii(&exif_date(date.date_time()));
        let offset = date.offset_text().map(|o| ascii(&o));
        let subsec = date.subsec().map(ascii);
        for (date_tag, offset_tag, subsec_tag) in [
            (TAG_DATE_TIME_ORIGINAL, TAG_OFFSET_TIME_ORIGINAL, TAG_SUB_SEC_TIME_ORIGINAL),
            (TAG_DATE_TIME_DIGITIZED, TAG_OFFSET_TIME_DIGITIZED, TAG_SUB_SEC_TIME_DIGITIZED),
        ] {
            ed.set(&mut exif, date_tag, types::ASCII, &text)?;
            match &offset {
                Some(o) => ed.set(&mut exif, offset_tag, types::ASCII, o)?,
                None => ed.remove(&mut exif, offset_tag),
            }
            match &subsec {
                Some(s) => ed.set(&mut exif, subsec_tag, types::ASCII, s)?,
                None => ed.remove(&mut exif, subsec_tag),
            }
        }
        if let Some(at) = ed.commit(exif)? {
            let pointer = ed.u32(at);
            ed.set(&mut ifd0, TAG_EXIF_IFD, types::LONG, &pointer)?;
        }
    }

    if let Some(place) = &edit.place {
        let mut gps = ed.table(IfdKind::Gps)?;
        match place {
            Some(p) => {
                if !gps.entries.iter().any(|e| e.tag == TAG_GPS_VERSION) {
                    ed.set(&mut gps, TAG_GPS_VERSION, types::BYTE, &[2, 3, 0, 0])?;
                }
                let lat_ref: &[u8] = if p.latitude() < 0.0 { b"S\0" } else { b"N\0" };
                let lon_ref: &[u8] = if p.longitude() < 0.0 { b"W\0" } else { b"E\0" };
                let lat = ed.dms(p.latitude());
                let lon = ed.dms(p.longitude());
                ed.set(&mut gps, TAG_GPS_LATITUDE_REF, types::ASCII, lat_ref)?;
                ed.set(&mut gps, TAG_GPS_LATITUDE, types::RATIONAL, &lat)?;
                ed.set(&mut gps, TAG_GPS_LONGITUDE_REF, types::ASCII, lon_ref)?;
                ed.set(&mut gps, TAG_GPS_LONGITUDE, types::RATIONAL, &lon)?;
            }
            None => {
                for tag in [TAG_GPS_LATITUDE_REF, TAG_GPS_LATITUDE, TAG_GPS_LONGITUDE_REF, TAG_GPS_LONGITUDE] {
                    ed.remove(&mut gps, tag);
                }
            }
        }
        if let Some(at) = ed.commit(gps)? {
            let pointer = ed.u32(at);
            ed.set(&mut ifd0, TAG_GPS_IFD, types::LONG, &pointer)?;
        }
    }

    if let Some(at) = ed.commit(ifd0)? {
        let pointer = ed.u32(at);
        ed.write_at(4, &pointer);
    }
    Ok(Some(ed.out))
}

/// `YYYY:MM:DD HH:MM:SS`, the EXIF date format.
fn exif_date(d: DateTime) -> String {
    let day = d.date();
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", day.year(), day.month(), day.day(), d.hour(), d.minute(), d.second())
}

/// An ASCII value: the text's bytes and the NUL EXIF ends it with.
fn ascii(text: &str) -> Vec<u8> {
    let mut b = text.as_bytes().to_vec();
    b.push(0);
    b
}

/// Bytes per value of a field type this editor writes or reads back.
fn type_size(kind: u16) -> Option<usize> {
    match kind {
        types::BYTE | types::ASCII | types::SBYTE | types::UNDEFINED => Some(1),
        types::SHORT | types::SSHORT => Some(2),
        types::LONG | types::SLONG | types::FLOAT | types::IFD => Some(4),
        types::RATIONAL | types::SRATIONAL | types::DOUBLE => Some(8),
        _ => None,
    }
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// One IFD entry as it lies in the block: the value field holds the value (four bytes or fewer)
/// or its offset, in the block's byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Raw {
    tag: u16,
    kind: u16,
    count: u32,
    field: [u8; 4],
}

/// One IFD being edited.
#[derive(Debug)]
struct Table {
    kind: IfdKind,
    /// Where it lies; `None` for an IFD the block does not have yet.
    at: Option<u32>,
    /// How many entries its place holds.
    room: usize,
    entries: Vec<Raw>,
    next: u32,
    dirty: bool,
}

/// Who a claimed byte range belongs to: an entry's value, or structure (`None`: the header, an
/// IFD table, the thumbnail).
type Owner = Option<(IfdKind, u16)>;

struct Editor {
    out: Vec<u8>,
    little: bool,
    /// The length of the block before the edit: only values inside it can be reused or zeroed.
    original: usize,
    /// Every byte range of the original block something points to, with its owner.
    claimed: Vec<(Owner, Range<usize>)>,
    /// The IFDs of the block: kind, offset, number of entries.
    ifds: Vec<(IfdKind, u32, usize)>,
    /// Whether ImageDescription holds a caption (not a camera placeholder).
    description_is_caption: bool,
}

impl Editor {
    fn new(block: &[u8], limits: &Limits) -> Result<Editor> {
        if u32::try_from(block.len()).is_err() {
            return Err(Error::TooMany("EXIF bytes"));
        }
        let w = walk(block, limits)?;
        let t = &w.tiff;
        let mut claimed: Vec<(Owner, Range<usize>)> = vec![(None, 0..8)];
        let mut ifds = Vec::new();
        for (kind, ifd) in &w.ifds {
            let at = ifd.offset as usize;
            let len = 2 + 12 * ifd.entries.len() + 4;
            claimed.push((None, at..at.saturating_add(len)));
            ifds.push((*kind, ifd.offset, ifd.entries.len()));
            for e in &ifd.entries {
                if let Ok(Some(r)) = t.value_range(e) {
                    claimed.push((Some((*kind, e.tag)), r));
                }
            }
        }
        // The thumbnail IFD1 points to.
        if let (Some(at), Some(len)) =
            (w.get(IfdKind::Thumbnail, 0x0201).and_then(|e| t.uint(e)), w.get(IfdKind::Thumbnail, 0x0202).and_then(|e| t.uint(e)))
        {
            let (at, len) = (at as usize, len as usize);
            claimed.push((None, at..at.saturating_add(len).min(block.len())));
        }
        let text = |tag| w.get(IfdKind::Image, tag).map_or(Ok(None), |e| entry_text(t, e, limits.max_string));
        let (make, model) = (text(TAG_MAKE)?, text(TAG_MODEL)?);
        let description_is_caption = text(TAG_IMAGE_DESCRIPTION)?.is_some_and(|d| !is_placeholder(&d, make.as_deref(), model.as_deref()));
        Ok(Editor { out: block.to_vec(), little: t.is_little_endian(), original: block.len(), claimed, ifds, description_is_caption })
    }

    fn u16(&self, v: u16) -> [u8; 2] {
        if self.little { v.to_le_bytes() } else { v.to_be_bytes() }
    }

    fn u32(&self, v: u32) -> [u8; 4] {
        if self.little { v.to_le_bytes() } else { v.to_be_bytes() }
    }

    fn read_u16(&self, at: usize) -> Result<u16> {
        let b = self.out.get(at..at.saturating_add(2)).ok_or(Error::OutOfRange)?;
        let b = [*b.first().unwrap_or(&0), *b.get(1).unwrap_or(&0)];
        Ok(if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }

    fn read_u32(&self, at: usize) -> Result<u32> {
        let b = self.out.get(at..at.saturating_add(4)).ok_or(Error::OutOfRange)?;
        Ok(self.word([*b.first().unwrap_or(&0), *b.get(1).unwrap_or(&0), *b.get(2).unwrap_or(&0), *b.get(3).unwrap_or(&0)]))
    }

    fn word(&self, b: [u8; 4]) -> u32 {
        if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
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

    /// Degree/minute/second rationals of a coordinate's magnitude, seconds in millionths.
    fn dms(&self, v: f64) -> Vec<u8> {
        const MICRO: u64 = 1_000_000;
        // At most 180 · 3,600 · 10⁶ < 2⁴⁰: exact in an f64 and in a u64.
        let total = (v.abs().min(180.0) * 3600.0 * MICRO as f64).round() as u64;
        let deg = total / (3600 * MICRO);
        let min = total % (3600 * MICRO) / (60 * MICRO);
        let sec = total % (60 * MICRO);
        let mut out = Vec::with_capacity(24);
        for (num, den) in [(deg, 1), (min, 1), (sec, MICRO)] {
            out.extend(self.u32(u32::try_from(num).unwrap_or(u32::MAX)));
            out.extend(self.u32(u32::try_from(den).unwrap_or(1)));
        }
        out
    }

    /// The IFD of `kind`, read from the block, or a new empty one.
    fn table(&self, kind: IfdKind) -> Result<Table> {
        let Some(&(_, at, count)) = self.ifds.iter().find(|(k, _, _)| *k == kind) else {
            return Ok(Table { kind, at: None, room: 0, entries: Vec::new(), next: 0, dirty: false });
        };
        let start = at as usize + 2;
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let e = start + 12 * i;
            let field = self.out.get(e + 8..e + 12).ok_or(Error::OutOfRange)?;
            entries.push(Raw {
                tag: self.read_u16(e)?,
                kind: self.read_u16(e + 2)?,
                count: self.read_u32(e + 4)?,
                field: [*field.first().unwrap_or(&0), *field.get(1).unwrap_or(&0), *field.get(2).unwrap_or(&0), *field.get(3).unwrap_or(&0)],
            });
        }
        let next = self.read_u32(start + 12 * count)?;
        Ok(Table { kind, at: Some(at), room: count, entries, next, dirty: false })
    }

    /// Where an entry of the original block keeps its value out of line, when it is inside the
    /// original block.
    fn out_of_line(&self, e: &Raw) -> Option<Range<usize>> {
        let len = type_size(e.kind)?.checked_mul(e.count as usize)?;
        if len <= 4 {
            return None;
        }
        let at = self.word(e.field) as usize;
        let end = at.checked_add(len)?;
        (end <= self.original).then_some(at..end)
    }

    /// Whether an entry of the original block keeps exactly `data` out of line.
    fn holds(&self, e: &Raw, data: &[u8]) -> bool {
        self.out_of_line(e).and_then(|r| self.out.get(r)).is_some_and(|old| old == data)
    }

    /// Whether nothing but `owner` points into `r`.
    fn exclusive(&self, owner: (IfdKind, u16), r: &Range<usize>) -> bool {
        self.claimed.iter().all(|(o, c)| *o == Some(owner) || !overlaps(c, r))
    }

    /// Zeroes the old out-of-line value of an entry being replaced elsewhere or removed, when
    /// nothing else shares its bytes.
    fn release(&mut self, kind: IfdKind, e: &Raw) {
        if let Some(r) = self.out_of_line(e)
            && self.exclusive((kind, e.tag), &r)
        {
            self.zero(r);
        }
    }

    /// Appends `data` at the end of the block, word aligned; its offset.
    fn append(&mut self, data: &[u8]) -> Result<u32> {
        if self.out.len() % 2 == 1 {
            self.out.push(0);
        }
        let at = u32::try_from(self.out.len()).map_err(|_| Error::TooMany("EXIF bytes"))?;
        if u32::try_from(self.out.len().saturating_add(data.len())).is_err() {
            return Err(Error::TooMany("EXIF bytes"));
        }
        self.out.extend_from_slice(data);
        Ok(at)
    }

    /// Sets `tag` in `t` to a value of `kind` whose encoded bytes are `data`: inline when it fits
    /// four bytes, else left as it is when the entry already holds it, else where the old value
    /// was when it fits there and is the entry's alone, else
    /// appended.
    fn set(&mut self, t: &mut Table, tag: u16, kind: u16, data: &[u8]) -> Result<()> {
        let size = type_size(kind).ok_or_else(|| Error::Malformed("an unknown field type".into()))?;
        let count = u32::try_from(data.len() / size).map_err(|_| Error::TooMany("EXIF bytes"))?;
        let old = t.entries.iter().position(|e| e.tag == tag);
        let old_entry = old.and_then(|i| t.entries.get(i).copied());
        let field = if data.len() <= 4 {
            if let Some(e) = &old_entry {
                self.release(t.kind, e);
            }
            let mut f = [0u8; 4];
            for (dst, src) in f.iter_mut().zip(data) {
                *dst = *src;
            }
            f
        } else if let Some(e) = old_entry.filter(|e| e.kind == kind && e.count == count && self.holds(e, data)) {
            // The value is already there: nothing changes, even where other entries share its bytes.
            e.field
        } else {
            let reuse = old_entry.as_ref().and_then(|e| self.out_of_line(e)).filter(|r| data.len() <= r.len() && self.exclusive((t.kind, tag), r));
            match reuse {
                Some(r) => {
                    self.write_at(r.start, data);
                    self.zero(r.start + data.len()..r.end);
                    self.u32(u32::try_from(r.start).map_err(|_| Error::OutOfRange)?)
                }
                None => {
                    if let Some(e) = &old_entry {
                        self.release(t.kind, e);
                    }
                    let at = self.append(data)?;
                    self.u32(at)
                }
            }
        };
        let raw = Raw { tag, kind, count, field };
        match old.and_then(|i| t.entries.get_mut(i)) {
            Some(e) => *e = raw,
            None => {
                let at = t.entries.iter().position(|e| e.tag > tag).unwrap_or(t.entries.len());
                t.entries.insert(at, raw);
            }
        }
        t.dirty = true;
        Ok(())
    }

    /// Removes every entry of `tag` from `t`, zeroing their values when nothing else shares them.
    fn remove(&mut self, t: &mut Table, tag: u16) {
        let gone: Vec<Raw> = t.entries.iter().filter(|e| e.tag == tag).copied().collect();
        if gone.is_empty() {
            return;
        }
        for e in &gone {
            self.release(t.kind, e);
        }
        t.entries.retain(|e| e.tag != tag);
        t.dirty = true;
    }

    /// Writes an edited IFD back: in its own place when its entries still fit there (the freed
    /// tail zeroed), else appended. The new offset when it moved.
    fn commit(&mut self, t: Table) -> Result<Option<u32>> {
        if !t.dirty {
            return Ok(None);
        }
        let n = u16::try_from(t.entries.len()).map_err(|_| Error::TooMany("IFD entries"))?;
        let mut table = Vec::with_capacity(2 + 12 * t.entries.len() + 4);
        table.extend(self.u16(n));
        for e in &t.entries {
            table.extend(self.u16(e.tag));
            table.extend(self.u16(e.kind));
            table.extend(self.u32(e.count));
            table.extend(e.field);
        }
        table.extend(self.u32(t.next));
        match t.at {
            Some(at) if t.entries.len() <= t.room => {
                let at = at as usize;
                self.write_at(at, &table);
                let freed = at + table.len();
                self.zero(freed..freed + 12 * (t.room - t.entries.len()));
                Ok(None)
            }
            _ => self.append(&table).map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dms_rounds_to_millionths_of_a_second() {
        let ed = Editor::new(&EMPTY, &Limits::default()).unwrap();
        let parts = |b: Vec<u8>| -> Vec<(u32, u32)> {
            b.chunks(8).map(|c| (u32::from_le_bytes(c[..4].try_into().unwrap()), u32::from_le_bytes(c[4..].try_into().unwrap()))).collect()
        };
        assert_eq!(parts(ed.dms(-33.5)), [(33, 1), (30, 1), (0, 1_000_000)]);
        assert_eq!(parts(ed.dms(151.2093)), [(151, 1), (12, 1), (33_480_000, 1_000_000)]);
        // 59.9999999″ rounds up into the next minute rather than writing 60″.
        assert_eq!(parts(ed.dms(10.0 + 59.0 / 60.0 + 59.999_999_9 / 3600.0)), [(11, 1), (0, 1), (0, 1_000_000)]);
        assert_eq!(parts(ed.dms(180.0)), [(180, 1), (0, 1), (0, 1_000_000)]);
    }

    #[test]
    fn exif_dates() {
        let d = "2026-06-04T07:02:09".parse::<DateTime>().unwrap();
        assert_eq!(exif_date(d), "2026:06:04 07:02:09");
    }
}
