//! Matroska: recognised by the EBML header's DocType and probed through `ebml.rs`.
//!
//! Provenance: written from the public Matroska specification (RFC 9559: the element IDs, the
//! `Segment` → `Info` / `Tracks` / `Tags` / `Cluster` layout, `SeekHead` positions counted from
//! the start of the segment's data, `Duration` in `TimestampScale` units of nanoseconds, `DateUTC`
//! in nanoseconds from 2001-01-01T00:00:00 UTC, `ProjectionPoseRoll` in degrees counter-clockwise)
//! and the Matroska tagging guidelines (`RECORDING_LOCATION` as ISO 6709); no code was consulted.
//!
//! The facts live in the segment's head (`Info`, `Tracks`) and, for the location, in `Tags`,
//! often written after the clusters and reached through the `SeekHead`. The walk of the segment
//! stops at its first `Cluster`: the media data is never read. A segment cut by the end of the
//! bytes is read as far as they go; bytes that stop before `Info` and `Tracks` are in are
//! [`Error::Truncated`], and `Tags` past them are left out.

use dalica_core::DateTime;

use super::ebml::{self, Element, Elements};
use super::{VideoInfo, iso6709, text, three_decimals};
use crate::limits::{Limits, Work};
use crate::{Error, Result};

const EBML_HEADER: u32 = 0x1a45_dfa3;
const DOC_TYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114d_9b74;
const SEEK: u32 = 0x4dbb;
const SEEK_ID: u32 = 0x53ab;
const SEEK_POSITION: u32 = 0x53ac;
const INFO: u32 = 0x1549_a966;
const TIMESTAMP_SCALE: u32 = 0x2a_d7b1;
const DURATION: u32 = 0x4489;
const DATE_UTC: u32 = 0x4461;
const TRACKS: u32 = 0x1654_ae6b;
const TRACK_ENTRY: u32 = 0xae;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const DEFAULT_DURATION: u32 = 0x23_e383;
const VIDEO: u32 = 0xe0;
const PIXEL_WIDTH: u32 = 0xb0;
const PIXEL_HEIGHT: u32 = 0xba;
const PROJECTION: u32 = 0x7670;
const PROJECTION_POSE_ROLL: u32 = 0x7675;
const TAGS: u32 = 0x1254_c367;
const TAG: u32 = 0x7373;
const SIMPLE_TAG: u32 = 0x67c8;
const TAG_NAME: u32 = 0x45a3;
const TAG_STRING: u32 = 0x4487;
const CLUSTER: u32 = 0x1f43_b675;

/// Seconds from 1970-01-01 to `DateUTC`'s origin, 2001-01-01.
const EPOCH_2001: i64 = 978_307_200;

/// Most `SeekHead` entries kept.
const MAX_SEEKS: usize = 64;

/// The DocType Dalica lists.
const MATROSKA: &[u8] = b"matroska";

/// Whether `head` is an EBML header whose DocType is `matroska`. The first bytes of a file may
/// cut the DocType's value: then the bytes there (at least four) must begin `matroska`.
pub fn recognise(head: &[u8]) -> bool {
    let mut work = Work::new(64, "EBML header elements");
    let mut top = Elements::top(head);
    let Ok(Some(header)) = top.next_head(&mut work) else { return false };
    if header.id != EBML_HEADER {
        return false;
    }
    let Ok(mut kids) = Elements::children(head, &header) else { return false };
    while let Ok(Some(e)) = kids.next_head(&mut work) {
        if e.id == DOC_TYPE {
            let Ok(value) = ebml::data(head, &e) else { return false };
            return if e.cut { value.len() >= 4 && MATROSKA.starts_with(value) } else { text(value).as_bytes() == MATROSKA };
        }
    }
    false
}

/// Whether `head` starts an EBML header that its bytes cut (too short to say what it is yet).
pub fn header_is_cut(head: &[u8]) -> bool {
    let mut work = Work::new(1, "EBML header");
    match Elements::top(head).next_head(&mut work) {
        Ok(Some(header)) => header.id == EBML_HEADER && header.cut,
        Err(Error::Truncated) => true,
        _ => false,
    }
}

/// The elements of the segment the facts come from.
#[derive(Default)]
struct Found {
    info: Option<Element>,
    tracks: Option<Element>,
    tags: Option<Element>,
}

impl Found {
    fn slot(&mut self, id: u32) -> Option<&mut Option<Element>> {
        match id {
            INFO => Some(&mut self.info),
            TRACKS => Some(&mut self.tracks),
            TAGS => Some(&mut self.tags),
            _ => None,
        }
    }
}

/// Fills `info` from the Matroska file in `bytes`.
pub fn probe(bytes: &[u8], info: &mut VideoInfo, _limits: &Limits, work: &mut Work) -> Result<()> {
    let mut top = Elements::top(bytes);
    let header = top.next(work)?.ok_or(Error::Truncated)?;
    if header.id != EBML_HEADER {
        return Err(Error::Malformed("no EBML header".into()));
    }
    let doc = Elements::children(bytes, &header)?.find(DOC_TYPE, work)?;
    let doc = doc.map(|e| ebml::data(bytes, &e).map(text)).transpose()?;
    if doc.as_deref().map(str::as_bytes) != Some(MATROSKA) {
        return Err(Error::Malformed("not a Matroska file".into()));
    }
    let segment = loop {
        match top.next_head(work)? {
            Some(e) if e.id == SEGMENT => break e,
            Some(_) => {}
            None => return Err(Error::Truncated),
        }
    };

    // The segment's head, up to its first cluster.
    let mut found = Found::default();
    let mut seeks: Vec<(u32, u64)> = Vec::new();
    let mut cut = segment.cut;
    let mut kids = Elements::children(bytes, &segment)?;
    loop {
        match kids.next(work) {
            Ok(Some(e)) if e.id == CLUSTER => break,
            Ok(Some(e)) if e.id == SEEK_HEAD => read_seeks(bytes, &e, &mut seeks, work)?,
            Ok(Some(e)) => {
                if let Some(slot) = found.slot(e.id)
                    && slot.is_none()
                {
                    *slot = Some(e);
                }
            }
            Ok(None) => break,
            Err(Error::Truncated) => {
                cut = true;
                break;
            }
            Err(e) => return Err(e),
        }
    }

    // What the seek head points at past the walk (the tags after the clusters).
    for (id, pos) in seeks {
        let needed = id != TAGS;
        let Some(slot) = found.slot(id) else { continue };
        if slot.is_some() {
            continue;
        }
        let at = usize::try_from(pos).ok().and_then(|p| segment.body.start.checked_add(p));
        let Some(at) = at.filter(|&a| a < segment.body.end) else {
            // Past the bytes of a cut segment: not read yet; past a whole one: a bad entry.
            cut |= needed && segment.cut;
            continue;
        };
        match Elements::within(bytes, &segment, at).and_then(|mut e| e.next(work)) {
            Ok(Some(e)) if e.id == id => *slot = Some(e),
            Err(Error::Truncated) => cut |= needed,
            Err(Error::TooMany(what)) => return Err(Error::TooMany(what)),
            _ => {}
        }
    }
    if cut && (found.info.is_none() || found.tracks.is_none()) {
        return Err(Error::Truncated);
    }

    if let Some(e) = &found.info {
        read_info(bytes, e, info, work)?;
    }
    if let Some(e) = &found.tracks {
        read_tracks(bytes, e, info, work)?;
    }
    if let Some(e) = &found.tags {
        read_tags(bytes, e, info, work)?;
    }
    Ok(())
}

/// The `SeekHead` entries that point at `Info`, `Tracks` or `Tags`.
fn read_seeks(bytes: &[u8], head: &Element, seeks: &mut Vec<(u32, u64)>, work: &mut Work) -> Result<()> {
    let mut entries = Elements::children(bytes, head)?;
    while let Some(seek) = entries.next(work)? {
        if seek.id != SEEK {
            continue;
        }
        let (mut id, mut pos) = (None, None);
        let mut fields = Elements::children(bytes, &seek)?;
        while let Some(f) = fields.next(work)? {
            match f.id {
                SEEK_ID => id = Some(ebml::uint(bytes, &f)?),
                SEEK_POSITION => pos = Some(ebml::uint(bytes, &f)?),
                _ => {}
            }
        }
        if let (Some(id), Some(pos)) = (id.and_then(|i| u32::try_from(i).ok()), pos)
            && matches!(id, INFO | TRACKS | TAGS)
            && seeks.len() < MAX_SEEKS
        {
            seeks.push((id, pos));
        }
    }
    Ok(())
}

/// Duration and date from `Info`.
fn read_info(bytes: &[u8], e: &Element, info: &mut VideoInfo, work: &mut Work) -> Result<()> {
    let (mut scale, mut duration, mut date) = (1_000_000u64, None, None);
    let mut fields = Elements::children(bytes, e)?;
    while let Some(f) = fields.next(work)? {
        match f.id {
            TIMESTAMP_SCALE => scale = ebml::uint(bytes, &f)?,
            DURATION => duration = Some(ebml::float(bytes, &f)?),
            DATE_UTC => date = Some(ebml::int(bytes, &f)?),
            _ => {}
        }
    }
    info.duration_ms = duration.and_then(|d| {
        // Ticks of `scale` nanoseconds, to milliseconds.
        let ms = (d * scale as f64 / 1e6).round();
        (scale > 0 && ms.is_finite() && (0.0..9.0e15).contains(&ms)).then_some(ms as u64)
    });
    info.created = date.and_then(|ns| DateTime::from_unix_secs(ns.div_euclid(1_000_000_000).checked_add(EPOCH_2001)?));
    Ok(())
}

/// Size, codecs, rate, rotation and sound from `Tracks`: the first video track and the first
/// sound track.
fn read_tracks(bytes: &[u8], e: &Element, info: &mut VideoInfo, work: &mut Work) -> Result<()> {
    let mut entries = Elements::children(bytes, e)?;
    let mut have_video = false;
    while let Some(entry) = entries.next(work)? {
        if entry.id != TRACK_ENTRY {
            continue;
        }
        let (mut kind, mut codec, mut frame_ns, mut size, mut roll) = (0, None, None, (0, 0), 0.0);
        let mut fields = Elements::children(bytes, &entry)?;
        while let Some(f) = fields.next(work)? {
            match f.id {
                TRACK_TYPE => kind = ebml::uint(bytes, &f)?,
                CODEC_ID => codec = Some(text(ebml::data(bytes, &f)?)),
                DEFAULT_DURATION => frame_ns = Some(ebml::uint(bytes, &f)?),
                VIDEO => (size, roll) = read_video(bytes, &f, work)?,
                _ => {}
            }
        }
        match kind {
            1 if !have_video => {
                have_video = true;
                let (w, h) = size;
                info.size = (w > 0 && h > 0).then_some((w, h));
                info.video_codec = codec.filter(|c| !c.is_empty());
                info.frame_rate = frame_ns.filter(|&n| n > 0).and_then(|n| three_decimals(1e9 / n as f64));
                info.rotation = quarter_turns(roll);
            }
            2 if !info.has_sound => {
                info.has_sound = true;
                info.sound_codec = codec.filter(|c| !c.is_empty());
            }
            _ => {}
        }
    }
    Ok(())
}

/// A video track's pixel size and projection roll.
fn read_video(bytes: &[u8], e: &Element, work: &mut Work) -> Result<((u32, u32), f64)> {
    let (mut w, mut h, mut roll) = (0u32, 0u32, 0.0);
    let mut fields = Elements::children(bytes, e)?;
    while let Some(f) = fields.next(work)? {
        match f.id {
            PIXEL_WIDTH => w = u32::try_from(ebml::uint(bytes, &f)?).unwrap_or(0),
            PIXEL_HEIGHT => h = u32::try_from(ebml::uint(bytes, &f)?).unwrap_or(0),
            PROJECTION => {
                if let Some(r) = Elements::children(bytes, &f)?.find(PROJECTION_POSE_ROLL, work)? {
                    roll = ebml::float(bytes, &r)?;
                }
            }
            _ => {}
        }
    }
    Ok(((w, h), roll))
}

/// Clockwise quarter turns from a counter-clockwise roll in degrees (0 unless a multiple of 90).
fn quarter_turns(roll: f64) -> u8 {
    let q = -roll / 90.0;
    if !q.is_finite() || (q - q.round()).abs() > 1e-6 || q.abs() > 1e6 {
        return 0;
    }
    (q.round() as i64).rem_euclid(4) as u8
}

/// The location from a `RECORDING_LOCATION` simple tag.
fn read_tags(bytes: &[u8], e: &Element, info: &mut VideoInfo, work: &mut Work) -> Result<()> {
    let mut tags = Elements::children(bytes, e)?;
    while let Some(tag) = tags.next(work)? {
        if tag.id != TAG {
            continue;
        }
        let mut simple = Elements::children(bytes, &tag)?;
        while let Some(s) = simple.next(work)? {
            if s.id != SIMPLE_TAG {
                continue;
            }
            let (mut name, mut value) = (None, None);
            let mut fields = Elements::children(bytes, &s)?;
            while let Some(f) = fields.next(work)? {
                match f.id {
                    TAG_NAME => name = Some(text(ebml::data(bytes, &f)?)),
                    TAG_STRING => value = Some(text(ebml::data(bytes, &f)?)),
                    _ => {}
                }
            }
            if name.is_some_and(|n| n.eq_ignore_ascii_case("RECORDING_LOCATION"))
                && let Some(point) = value.as_deref().and_then(iso6709)
            {
                info.location = Some(point);
                return Ok(());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_to_quarter_turns() {
        assert_eq!([0.0, -90.0, 180.0, 90.0, -270.0, 450.0].map(quarter_turns), [0, 1, 2, 3, 3, 3]);
        assert_eq!([45.0, f64::NAN, f64::INFINITY, 1e300].map(quarter_turns), [0, 0, 0, 0]);
    }
}
