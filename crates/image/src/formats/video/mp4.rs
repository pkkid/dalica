//! MP4, M4V and QuickTime movies probed through `bmff.rs`'s bounded box walk.
//!
//! Provenance: written from the public descriptions of the boxes (ISO/IEC 14496-12 §8 as
//! summarised in open format notes, and Apple's QuickTime File Format documentation): `mvhd` and
//! `mdhd` (times in seconds from 1904-01-01 UTC, a timescale and a duration; 64-bit fields in
//! version 1), `tkhd` (its 3 × 3 matrix of 16.16 and 2.30 fixed-point numbers, then the width and
//! height in 16.16), `hdlr` (`vide`, `soun`), `stsd` (the first sample entry's code, and a visual
//! entry's width and height), `stsz`/`stz2` (the sample count in their header), the QuickTime
//! `udta` text atom `©xyz` and the `meta` → `keys` / `ilst` metadata with its
//! `com.apple.quicktime.location.ISO6709` key. No code was consulted.
//!
//! Only the movie box is read; the media data is skipped, and of the sample tables only the
//! sample description's first entry and the sample size box's header are looked at.

use std::io::{Read, Seek, SeekFrom};

use dalica_core::DateTime;

use super::super::bmff::{self, BoxRef, Boxes, child, path};
use super::super::{u16_be, u32_be, u64_be};
use super::{VideoInfo, iso6709, millis, text, three_decimals};
use crate::limits::{Limits, Work};
use crate::{Error, Result};

/// Seconds from 1904-01-01 (the movie formats' origin) to 1970-01-01.
const EPOCH_1904: i64 = 2_082_844_800;

/// The QuickTime metadata key holding the location.
const LOCATION_KEY: &[u8] = b"com.apple.quicktime.location.ISO6709";

/// Most `keys` entries looked at.
const MAX_KEYS: usize = 4096;

/// Fills `info` from the movie box of the ISO BMFF file in `bytes`. A file whose bytes end
/// before the movie box does is [`Error::Truncated`].
pub fn probe(bytes: &[u8], info: &mut VideoInfo, _limits: &Limits, work: &mut Work) -> Result<()> {
    // The movie box, wherever it lies; bytes that end before it are a cut file.
    let moov = Boxes::top(bytes).find(b"moov", None, work)?.ok_or(Error::Truncated)?;
    let body = |b: &BoxRef| bytes.get(b.body.clone()).ok_or(Error::Truncated);

    let mut movie_duration = None;
    if let Some(mvhd) = child(bytes, &moov, 0, b"mvhd", work)? {
        let t = times(body(&mvhd)?)?;
        info.created = t.created;
        movie_duration = t.duration.and_then(|d| millis(d, t.timescale));
    }

    let mut tracks = Boxes::children(bytes, &moov, 0)?;
    let mut video_duration = None;
    let mut have_video = false;
    while let Some(trak) = tracks.next(work)? {
        if &trak.kind != b"trak" {
            continue;
        }
        let Some(mdia) = child(bytes, &trak, 0, b"mdia", work)? else { continue };
        let handler = match child(bytes, &mdia, 0, b"hdlr", work)? {
            Some(h) => body(&h)?.get(8..12).and_then(|k| <[u8; 4]>::try_from(k).ok()),
            None => None,
        };
        let stsd = path(bytes, &mdia, &[b"minf", b"stbl", b"stsd"], work)?;
        let entry = match &stsd {
            Some(stsd) => Boxes::children(bytes, stsd, 8)?.next(work)?,
            None => None,
        };
        match handler.as_ref() {
            Some(b"vide") if !have_video => {
                have_video = true;
                info.video_codec = entry.as_ref().and_then(|e| fourcc(e.kind));
                if let Some(tkhd) = child(bytes, &trak, 0, b"tkhd", work)? {
                    let (size, turns) = track_header(body(&tkhd)?)?;
                    info.size = size;
                    info.rotation = turns;
                }
                // A track header without a size: the sample entry's.
                if info.size.is_none()
                    && let Some(e) = &entry
                {
                    let b = body(e)?;
                    let (w, h) = (u16_be(b, 24).unwrap_or(0), u16_be(b, 26).unwrap_or(0));
                    info.size = (w > 0 && h > 0).then_some((u32::from(w), u32::from(h)));
                }
                let media = match child(bytes, &mdia, 0, b"mdhd", work)? {
                    Some(m) => Some(times(body(&m)?)?),
                    None => None,
                };
                if let Some(m) = &media {
                    video_duration = m.duration.and_then(|d| millis(d, m.timescale));
                }
                let stbl = path(bytes, &mdia, &[b"minf", b"stbl"], work)?;
                let mut samples = None;
                if let Some(stbl) = &stbl {
                    let mut tables = Boxes::children(bytes, stbl, 0)?;
                    while let Some(t) = tables.next(work)? {
                        if &t.kind == b"stsz" || &t.kind == b"stz2" {
                            samples = Some(u32_be(body(&t)?, 8)?);
                            break;
                        }
                    }
                }
                info.frame_rate = match (samples, media) {
                    (Some(n), Some(m)) if n > 0 => {
                        m.duration.filter(|&d| d > 0).and_then(|d| three_decimals(f64::from(n) * m.timescale as f64 / d as f64))
                    }
                    _ => None,
                };
            }
            Some(b"soun") if !info.has_sound => {
                info.has_sound = true;
                info.sound_codec = entry.as_ref().and_then(|e| fourcc(e.kind));
            }
            _ => {}
        }
    }
    info.duration_ms = movie_duration.filter(|&d| d > 0).or(video_duration);
    info.location = location(bytes, &moov, work)?;
    Ok(())
}

/// The times of an `mvhd` or `mdhd` payload.
struct Times {
    created: Option<DateTime>,
    timescale: u64,
    /// `None` when every bit is set ("unknown").
    duration: Option<u64>,
}

fn times(b: &[u8]) -> Result<Times> {
    let (created, timescale, duration) = match b.first() {
        Some(1) => {
            let d = u64_be(b, 24)?;
            (u64_be(b, 4)?, u32_be(b, 20)?, (d != u64::MAX).then_some(d))
        }
        Some(0) => {
            let d = u32_be(b, 16)?;
            (u64::from(u32_be(b, 4)?), u32_be(b, 12)?, (d != u32::MAX).then_some(u64::from(d)))
        }
        Some(v) => return Err(Error::Malformed(format!("a movie header of version {v}"))),
        None => return Err(Error::Truncated),
    };
    let created = if created == 0 { None } else { i64::try_from(created).ok().and_then(|s| DateTime::from_unix_secs(s - EPOCH_1904)) };
    Ok(Times { created, timescale: u64::from(timescale), duration })
}

/// The size and the quarter turns of a `tkhd` payload.
fn track_header(b: &[u8]) -> Result<(Option<(u32, u32)>, u8)> {
    let (matrix, size) = match b.first() {
        Some(1) => (52, 88),
        Some(0) => (40, 76),
        Some(v) => return Err(Error::Malformed(format!("a track header of version {v}"))),
        None => return Err(Error::Truncated),
    };
    let fixed = |at: usize| u32_be(b, at).map(|v| v as i32);
    let (a, bb, c, d) = (fixed(matrix)?, fixed(matrix + 4)?, fixed(matrix + 12)?, fixed(matrix + 16)?);
    // x' = a·x + c·y, y' = b·x + d·y, with y pointing down: (0, 1, −1, 0) turns a quarter clockwise.
    let turns = match (a.signum(), bb.signum(), c.signum(), d.signum()) {
        (0, 1, -1, 0) => 1,
        (-1, 0, 0, -1) => 2,
        (0, -1, 1, 0) => 3,
        _ => 0,
    };
    // 16.16 fixed point, rounded to whole pixels.
    let px = |v: u32| (v >> 16) + u32::from(v & 0x8000 != 0);
    let (w, h) = (px(u32_be(b, size)?), px(u32_be(b, size + 4)?));
    Ok(((w > 0 && h > 0).then_some((w, h)), turns))
}

/// A sample entry's code as text (`avc1`, `mp4a`, `ac-3`), if it is printable.
fn fourcc(kind: [u8; 4]) -> Option<String> {
    kind.iter().all(|b| (0x20..0x7f).contains(b)).then(|| String::from_utf8_lossy(&kind).trim_end().to_string()).filter(|s| !s.is_empty())
}

/// The location: the `©xyz` text in `udta`, else the QuickTime location key in `meta`.
fn location(bytes: &[u8], moov: &BoxRef, work: &mut Work) -> Result<Option<dalica_core::geo::LatLon>> {
    if let Some(xyz) = path(bytes, moov, &[b"udta", b"\xa9xyz"], work)? {
        let b = bytes.get(xyz.body.clone()).ok_or(Error::Truncated)?;
        // A QuickTime text: a 16-bit length and a 16-bit language, then the text.
        let len = usize::from(u16_be(b, 0).unwrap_or(0));
        let framed = b.get(4..4usize.saturating_add(len)).map(text).and_then(|t| iso6709(&t));
        if let Some(p) = framed.or_else(|| iso6709(&text(b))) {
            return Ok(Some(p));
        }
    }
    let Some(meta) = child(bytes, moov, 0, b"meta", work)? else { return Ok(None) };
    // QuickTime's `meta` has no version and flags; ISO's does.
    let skip = if u32_be(bytes, meta.body.start).is_ok_and(|v| v == 0) { 4 } else { 0 };
    let (Some(keys), Some(ilst)) =
        (Boxes::children(bytes, &meta, skip)?.find(b"keys", None, work)?, Boxes::children(bytes, &meta, skip)?.find(b"ilst", None, work)?)
    else {
        return Ok(None);
    };
    let Some(index) = key_index(bytes.get(keys.body.clone()).ok_or(Error::Truncated)?, work)? else { return Ok(None) };
    let mut items = Boxes::children(bytes, &ilst, 0)?;
    while let Some(item) = items.next(work)? {
        if u32::from_be_bytes(item.kind) != index {
            continue;
        }
        let Some(data) = child(bytes, &item, 0, b"data", work)? else { return Ok(None) };
        let b = bytes.get(data.body.clone()).ok_or(Error::Truncated)?;
        // Type 1 is UTF-8; then a locale; then the value.
        if u32_be(b, 0)? == 1 {
            return Ok(b.get(8..).map(text).and_then(|t| iso6709(&t)));
        }
        return Ok(None);
    }
    Ok(None)
}

/// The 1-based index of the location key in a `keys` payload.
fn key_index(b: &[u8], work: &mut Work) -> Result<Option<u32>> {
    let count = u32_be(b, 4)?;
    let mut at = 8usize;
    for index in 1..=count.min(MAX_KEYS as u32) {
        work.step()?;
        let size = u32_be(b, at)? as usize;
        if size < 8 {
            return Err(Error::Malformed(format!("a metadata key of {size} bytes")));
        }
        let end = at.checked_add(size).ok_or(Error::Truncated)?;
        let key = b.get(at + 8..end).ok_or(Error::Truncated)?;
        if key == LOCATION_KEY {
            return Ok(Some(index));
        }
        at = end;
    }
    Ok(None)
}

/// The `ftyp` and `moov` boxes of the file `file` of `size` bytes, laid end to end, read box by
/// box with the media data skipped (at most [`bmff::MAX_BOXES`] top-level boxes, a movie box of
/// at most [`Limits::max_header_bytes`]).
pub fn read_movie<R: Read + Seek>(file: &mut R, size: u64, limits: &Limits) -> Result<Vec<u8>> {
    let io = |e: std::io::Error| Error::Io(e.to_string());
    let mut out = Vec::new();
    let mut pos = 0u64;
    for _ in 0..bmff::MAX_BOXES {
        if pos >= size {
            break;
        }
        file.seek(SeekFrom::Start(pos)).map_err(io)?;
        let mut head = Vec::with_capacity(16);
        file.by_ref().take(16).read_to_end(&mut head).map_err(io)?;
        let declared = u32_be(&head, 0)?;
        let kind = head.get(4..8).ok_or(Error::Truncated)?;
        let len = match declared {
            0 => size - pos,
            1 => u64_be(&head, 8)?,
            n => u64::from(n),
        };
        if len < 8 {
            return Err(Error::Malformed(format!("a box of {len} bytes")));
        }
        if kind == b"ftyp" || kind == b"moov" {
            if len > limits.max_header_bytes {
                return Err(Error::TooMany("bytes in a movie box"));
            }
            file.seek(SeekFrom::Start(pos)).map_err(io)?;
            let before = out.len();
            file.by_ref().take(len).read_to_end(&mut out).map_err(io)?;
            if (out.len() - before) as u64 != len {
                return Err(Error::Truncated);
            }
            if kind == b"moov" {
                return Ok(out);
            }
        }
        pos = pos.checked_add(len).ok_or(Error::Truncated)?;
    }
    Err(Error::Truncated)
}
