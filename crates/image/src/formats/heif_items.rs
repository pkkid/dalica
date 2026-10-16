//! The HEIF container's items, read without an HEVC decoder (`formats.md` §3): what a build
//! without the `heif` feature still knows about a HEIC photo, so it is listed, sorted and searched
//! by its date and size like any other file while its tile says why it is not shown.
//!
//! Provenance: written from the public description of the HEIF item boxes (ISO/IEC 23008-12 §9,
//! and ISO/IEC 14496-12 §8.11 for the `meta` boxes it builds on, as summarised in many open format
//! notes): `pitm` names the primary item; `iinf` lists `infe` entries (versions 2 and 3: item id,
//! protection index, item type); `iloc` gives each item's extents (versions 0–2, offset, length,
//! base offset and index sizes of 0, 4 or 8 bytes, a construction method from version 1); `iprp`
//! holds the `ipco` property boxes and the `ipma` associations (1-based indices, 7 or 15 bits,
//! the top bit "essential"); `ispe` is the image's size, `irot` its anticlockwise quarter turns
//! and `imir` its mirror axis; an `Exif` item starts with a 4-byte offset to its TIFF header. No
//! code was consulted.
//!
//! Every size is hostile (`security.md` §4.2): the box walk is `formats/bmff.rs`'s (nesting,
//! boxes per level and steps capped), every field read is checked against its box, offsets are
//! added with checked arithmetic, entry loops are capped at [`MAX_ENTRIES`] and the EXIF block at
//! [`MAX_EXIF`] (the only allocation, for an item in several extents). Only the primary item's
//! size and transforms and the first `Exif` item are read; the XMP item is not (later work).

use std::borrow::Cow;
use std::ops::Range;

use crate::formats::bmff::{BoxRef, Boxes};
use crate::limits::{Limits, Work};
use crate::{Error, Format, Header, Orientation, Result};

/// Most entries read from one `iloc` or `ipma` box.
pub const MAX_ENTRIES: usize = 65_536;

/// Largest EXIF item read (a JPEG's EXIF segment holds at most 64 KiB; room is left for large
/// maker notes).
pub const MAX_EXIF: usize = 4 * 1024 * 1024;

/// What the container says about its primary image.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Primary {
    width: u32,
    height: u32,
    orientation: Orientation,
}

fn damaged(what: &str) -> Error {
    Error::Malformed(format!("HEIF: {what}"))
}

/// Big-endian fields read from one box's payload; reading past it is damage, never truncation
/// (the box itself lies inside the bytes).
struct Fields<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Fields<'a> {
    fn of(bytes: &'a [u8], b: &BoxRef) -> Result<Fields<'a>> {
        let body = bytes.get(b.body.clone()).ok_or_else(|| damaged("a box past the end of its parent"))?;
        Ok(Fields { bytes: body, pos: 0 })
    }

    /// An unsigned field of `n` bytes (0 reads nothing and gives 0; at most 8).
    fn uint(&mut self, n: usize) -> Result<u64> {
        let end = self.pos.checked_add(n).filter(|_| n <= 8).ok_or_else(|| damaged("a field too wide"))?;
        let field = self.bytes.get(self.pos..end).ok_or_else(|| damaged("a box too short for its fields"))?;
        self.pos = end;
        Ok(field.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b)))
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.uint(1)? as u8)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(self.uint(2)? as u16)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(self.uint(4)? as u32)
    }

    /// A full box's version (its flags returned beside it).
    fn version(&mut self) -> Result<(u8, u32)> {
        let v = self.u32()?;
        Ok(((v >> 24) as u8, v & 0x00ff_ffff))
    }

    fn kind(&mut self) -> Result<[u8; 4]> {
        Ok(self.u32()?.to_be_bytes())
    }
}

/// The header of a HEIF file read from its items, without decoding: the primary item's `ispe`
/// size as stored, its `irot`/`imir` transforms (in association order) as the orientation, and
/// EXIF from the first `Exif` item (its orientation tag is ignored: the container's transforms
/// are the ones that count, as when decoding). No embedded thumbnail is offered.
///
/// Errors: [`Error::Truncated`] only when the bytes stop inside a top-level box (a partial read,
/// so the caller reads on) and `meta` or the EXIF item lies past them;
/// [`Error::Unsupported`]`(Avif)` for an AV1 primary item; anything this reader cannot use (no
/// `meta`, `pitm` or `ispe`, a damaged box, an item past the end of a file that is all there) is
/// [`Error::Malformed`] or [`Error::TooMany`].
pub fn header(bytes: &[u8], limits: &Limits) -> Result<Header> {
    let mut work = Work::new(limits.max_segments, "ISOBMFF boxes");
    let (meta, cut) = top_level(bytes, &mut work)?;
    let mut boxes = Boxes::children(bytes, &meta, 4)?;
    let (mut pitm, mut iinf, mut iloc, mut iprp) = (None, None, None, None);
    while let Some(b) = boxes.next(&mut work)? {
        let slot = match &b.kind {
            b"pitm" => &mut pitm,
            b"iinf" => &mut iinf,
            b"iloc" => &mut iloc,
            b"iprp" => &mut iprp,
            _ => continue,
        };
        slot.get_or_insert(b);
    }
    let pitm = pitm.ok_or_else(|| damaged("no primary item"))?;
    let primary_id = {
        let mut f = Fields::of(bytes, &pitm)?;
        match f.version()?.0 {
            0 => u32::from(f.u16()?),
            _ => f.u32()?,
        }
    };
    let (primary_type, exif_id) = match &iinf {
        Some(iinf) => items(bytes, iinf, primary_id, &mut work)?,
        None => (None, None),
    };
    if primary_type == Some(*b"av01") {
        return Err(Error::Unsupported(Format::Avif));
    }
    let iprp = iprp.ok_or_else(|| damaged("no item properties"))?;
    let p = primary(bytes, &iprp, primary_id, &mut work)?;
    // A size past the limits could never be shown: the tile, rather than "too large".
    limits.check_size(p.width, p.height, 4)?;
    let header =
        Header { format: Format::Heif, width: p.width, height: p.height, orientation: p.orientation, exif: None, exif_error: None, thumbnail: None };
    let block = match (exif_id, &iloc) {
        (Some(id), Some(iloc)) => exif_block(bytes, iloc, id, cut)?,
        _ => None,
    };
    let mut h = match block {
        Some(Ok(item)) => match tiff_of(&item) {
            Some(tiff) => header.with_exif(Some((tiff, 0)), limits),
            None => Header { exif_error: Some(damaged("an EXIF item without its TIFF header")), ..header },
        },
        Some(Err(e)) => Header { exif_error: Some(e), ..header },
        None => header,
    };
    // Nothing of the EXIF block stands in for the picture, and it does not turn it.
    h.orientation = p.orientation;
    h.thumbnail = None;
    if let Some(e) = h.exif.as_mut() {
        e.strip_thumbnail = None;
    }
    Ok(h)
}

/// The `meta` box, and whether the bytes stop inside a top-level box (a partial read, or a file
/// cut short).
fn top_level(bytes: &[u8], work: &mut Work) -> Result<(BoxRef, bool)> {
    let mut top = Boxes::top(bytes);
    let mut meta = None;
    let cut = loop {
        match top.next(work) {
            Ok(Some(b)) if &b.kind == b"meta" => {
                meta.get_or_insert(b);
            }
            Ok(Some(_)) => {}
            Ok(None) => break false,
            Err(Error::Truncated) => break true,
            // Damage after `meta`: what was read stands.
            Err(_) if meta.is_some() => break false,
            Err(e) => return Err(e),
        }
    };
    match meta {
        Some(m) => Ok((m, cut)),
        None if cut => Err(Error::Truncated),
        None => Err(damaged("no meta box")),
    }
}

/// The primary item's type and the first `Exif` item's id, from `iinf`'s `infe` entries
/// (versions 2 and 3; older entries name files, not images).
fn items(bytes: &[u8], iinf: &BoxRef, primary: u32, work: &mut Work) -> Result<(Option<[u8; 4]>, Option<u32>)> {
    // A full box, then an entry count of 16 bits (version 0) or 32.
    let skip = 4 + if bytes.get(iinf.body.start) == Some(&0) { 2 } else { 4 };
    let mut entries = Boxes::children(bytes, iinf, skip)?;
    let (mut kind, mut exif) = (None, None);
    while let Some(b) = entries.next(work)? {
        if &b.kind != b"infe" {
            continue;
        }
        let mut f = Fields::of(bytes, &b)?;
        let id = match f.version()?.0 {
            0 | 1 => continue,
            2 => u32::from(f.u16()?),
            _ => f.u32()?,
        };
        let _protection = f.u16()?;
        let item_type = f.kind()?;
        if id == primary {
            kind.get_or_insert(item_type);
        }
        if &item_type == b"Exif" {
            exif.get_or_insert(id);
        }
    }
    Ok((kind, exif))
}

/// The primary item's `ispe` size and its `irot`/`imir` transforms as an orientation.
fn primary(bytes: &[u8], iprp: &BoxRef, id: u32, work: &mut Work) -> Result<Primary> {
    let mut props = Vec::new();
    let mut indices = None;
    let mut children = Boxes::children(bytes, iprp, 0)?;
    while let Some(b) = children.next(work)? {
        match &b.kind {
            b"ipco" if props.is_empty() => {
                let mut ipco = Boxes::children(bytes, &b, 0)?;
                while let Some(p) = ipco.next(work)? {
                    props.push(p);
                }
            }
            b"ipma" if indices.is_none() => indices = associations(bytes, &b, id)?,
            _ => {}
        }
    }
    let indices = indices.ok_or_else(|| damaged("no properties for the primary item"))?;
    let mut size = None;
    let mut turn = Turn::NONE;
    for index in indices {
        // Index 0 means "no property".
        let Some(p) = index.checked_sub(1).and_then(|i| props.get(usize::from(i))) else { continue };
        let mut f = Fields::of(bytes, p)?;
        match &p.kind {
            b"ispe" if size.is_none() => {
                f.version()?;
                size = Some((f.u32()?, f.u32()?));
            }
            b"irot" => turn = turn.then(Turn::anticlockwise(f.u8()? & 3)),
            b"imir" => turn = turn.then(if f.u8()? & 1 == 0 { Turn::TOP_BOTTOM } else { Turn::LEFT_RIGHT }),
            _ => {}
        }
    }
    let (width, height) = size.ok_or_else(|| damaged("no ispe for the primary item"))?;
    Ok(Primary { width, height, orientation: turn.orientation() })
}

/// The property indices `ipma` associates with item `id` (`None` when it lists none for it).
fn associations(bytes: &[u8], ipma: &BoxRef, id: u32) -> Result<Option<Vec<u16>>> {
    let mut f = Fields::of(bytes, ipma)?;
    let (version, flags) = f.version()?;
    let count = f.u32()?;
    for _ in 0..(count as usize).min(MAX_ENTRIES) {
        let item = if version < 1 { u32::from(f.u16()?) } else { f.u32()? };
        let n = f.u8()?;
        let mut list = Vec::with_capacity(usize::from(n));
        for _ in 0..n {
            let index = if flags & 1 == 1 { f.u16()? & 0x7fff } else { u16::from(f.u8()? & 0x7f) };
            list.push(index);
        }
        if item == id {
            return Ok(Some(list));
        }
    }
    if count as usize > MAX_ENTRIES {
        return Err(Error::TooMany("HEIF property associations"));
    }
    Ok(None)
}

/// Item `id`'s bytes from its `iloc` extents: `None` when it has no entry or is not in this file
/// at plain offsets (construction method 0, data reference 0); `Some(Err)` when it is unusable
/// as EXIF (too large, empty). An extent past the bytes is [`Error::Truncated`] when `cut` (read
/// on), else damage.
fn exif_block<'a>(bytes: &'a [u8], iloc: &BoxRef, id: u32, cut: bool) -> Result<Option<Result<Cow<'a, [u8]>>>> {
    let mut f = Fields::of(bytes, iloc)?;
    let version = f.version()?.0;
    if version > 2 {
        return Ok(None);
    }
    let sizes = f.u16()?;
    let width = |nibble: u16| match nibble {
        0 | 4 | 8 => Ok(usize::from(nibble)),
        _ => Err(damaged("an iloc field of an unknown width")),
    };
    let (offset_size, length_size, base_size) = (width(sizes >> 12)?, width((sizes >> 8) & 15)?, width((sizes >> 4) & 15)?);
    let index_size = if version == 0 { 0 } else { width(sizes & 15)? };
    let count = if version < 2 { u32::from(f.u16()?) } else { f.u32()? };
    for _ in 0..(count as usize).min(MAX_ENTRIES) {
        let item = if version < 2 { u32::from(f.u16()?) } else { f.u32()? };
        let method = if version == 0 { 0 } else { f.u16()? & 15 };
        let data_ref = f.u16()?;
        let base = f.uint(base_size)?;
        let extents = f.u16()?;
        let mut ranges = Vec::new();
        for _ in 0..extents {
            f.uint(index_size)?;
            let start = base.checked_add(f.uint(offset_size)?).ok_or_else(|| damaged("an extent offset overflows"))?;
            let len = f.uint(length_size)?;
            if item == id {
                ranges.push((start, len));
            }
        }
        if item != id {
            continue;
        }
        if method != 0 || data_ref != 0 {
            return Ok(None);
        }
        return gather(bytes, &ranges, cut).map(Some);
    }
    Ok(None)
}

/// The extents' bytes, borrowed when there is one, joined (at most [`MAX_EXIF`]) when several.
fn gather<'a>(bytes: &'a [u8], ranges: &[(u64, u64)], cut: bool) -> Result<Result<Cow<'a, [u8]>>> {
    let mut total = 0u64;
    let mut slices = Vec::with_capacity(ranges.len().min(64));
    for &(start, len) in ranges {
        total = total.saturating_add(len);
        // Length 0 is "the rest of the file", never what an EXIF item is.
        if len == 0 || total > MAX_EXIF as u64 {
            return Ok(Err(Error::TooMany("bytes in the EXIF item")));
        }
        let range: Option<Range<usize>> = (|| {
            let s = usize::try_from(start).ok()?;
            Some(s..s.checked_add(usize::try_from(len).ok()?)?)
        })();
        match range.and_then(|r| bytes.get(r)) {
            Some(s) => slices.push(s),
            None if cut => return Err(Error::Truncated),
            None => return Err(damaged("the EXIF item lies past the end of the file")),
        }
    }
    Ok(match slices.as_slice() {
        [] => Err(damaged("an EXIF item without extents")),
        [one] => Ok(Cow::Borrowed(*one)),
        many => Ok(Cow::Owned(many.concat())),
    })
}

/// The TIFF structure in an `Exif` item: after the 4-byte offset to its header.
fn tiff_of(item: &[u8]) -> Option<&[u8]> {
    let offset = usize::try_from(u32::from_be_bytes(item.get(..4)?.try_into().ok()?)).ok()?;
    item.get(offset.checked_add(4)?..)
}

/// A turn or mirror of the picture as a 2 × 2 matrix of −1, 0, 1 (columns: where the stored
/// right and down directions go on screen), so transforms compose by multiplication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Turn([i8; 4]);

impl Turn {
    const NONE: Turn = Turn([1, 0, 0, 1]);
    const LEFT_RIGHT: Turn = Turn([-1, 0, 0, 1]);
    const TOP_BOTTOM: Turn = Turn([1, 0, 0, -1]);
    /// The EXIF orientations 1–8 in order.
    const EXIF: [Turn; 8] = [
        Turn::NONE,
        Turn::LEFT_RIGHT,
        Turn([-1, 0, 0, -1]),
        Turn::TOP_BOTTOM,
        Turn([0, 1, 1, 0]),
        Turn([0, -1, 1, 0]),
        Turn([0, -1, -1, 0]),
        Turn([0, 1, -1, 0]),
    ];

    /// `irot`: quarter turns anticlockwise (EXIF 8, 3, 6).
    fn anticlockwise(quarters: u8) -> Turn {
        match quarters & 3 {
            1 => Turn::EXIF[7],
            2 => Turn::EXIF[2],
            3 => Turn::EXIF[5],
            _ => Turn::NONE,
        }
    }

    /// This transform, then `next`.
    fn then(self, next: Turn) -> Turn {
        let ([a, b, c, d], [e, f, g, h]) = (self.0, next.0);
        Turn([e * a + f * c, e * b + f * d, g * a + h * c, g * b + h * d])
    }

    fn orientation(self) -> Orientation {
        let value = Turn::EXIF.iter().position(|t| *t == self).map_or(1, |i| i + 1);
        Orientation::new(value as u8).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `irot` and `imir` compose in association order into the EXIF orientation that shows the
    /// stored pixels the same way (the decoder's transforms, `tests/heif.rs` with the feature).
    #[test]
    fn transforms_compose_into_an_orientation() {
        let o = |t: Turn| t.orientation().get();
        assert_eq!([0, 1, 2, 3].map(|q| o(Turn::anticlockwise(q))), [1, 8, 3, 6]);
        assert_eq!((o(Turn::TOP_BOTTOM), o(Turn::LEFT_RIGHT)), (4, 2));
        // A quarter turn anticlockwise, then left and right exchanged: the transverse.
        assert_eq!(o(Turn::anticlockwise(1).then(Turn::LEFT_RIGHT)), 7);
        assert_eq!(o(Turn::LEFT_RIGHT.then(Turn::anticlockwise(1))), 5);
        assert_eq!(o(Turn::anticlockwise(2).then(Turn::anticlockwise(2))), 1);
        for t in Turn::EXIF {
            assert_eq!(t.then(Turn::NONE), t);
        }
    }
}
