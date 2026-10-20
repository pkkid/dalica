//! A track's samples, read from an ISO BMFF movie box: where each coded frame (or sound packet)
//! lies in the file, when it is shown, which ones a decoder can start at, and the codec's own
//! configuration.
//!
//! Provenance: written from the public descriptions of ISO/IEC 14496-12 §8 (`mdhd`, `hdlr`,
//! `stsd`, `stts`, `ctts`, `stss`, `stsc`, `stsz`/`stz2`, `stco`/`co64`, `edts`/`elst`), of
//! ISO/IEC 14496-15 (the `avcC` and `hvcC` decoder configuration records: the NAL unit length
//! size and the parameter sets) and of ISO/IEC 14496-1 §7.2.6 and 14496-14 (`esds`: the
//! `ES_Descriptor`, its `DecoderConfigDescriptor` and `DecoderSpecificInfo`), as summarised in
//! open format notes; no code was consulted.
//!
//! Every count is hostile (`security.md` §4.2): a track holds at most [`MAX_SAMPLES`] samples,
//! each at most [`MAX_SAMPLE_BYTES`]; every table is read inside its box (which lies inside the
//! movie box, itself capped at [`Limits::max_header_bytes`] by `read_movie`), its entry count
//! checked against the box's length before anything is reserved, and expanded only as far as the
//! sample count; a parameter set or configuration is at most [`MAX_CONFIG`] bytes.

use super::super::super::bmff::{BoxRef, Boxes, child, path};
use super::super::super::{u16_be, u32_be, u64_be};
use crate::limits::Work;
use crate::{Error, Result};

/// Most samples one track may hold (19 hours at 30 frames a second).
pub const MAX_SAMPLES: usize = 1 << 21;

/// Largest coded frame or sound packet read.
pub const MAX_SAMPLE_BYTES: u32 = 32 << 20;

/// Largest codec configuration, parameter set or `esds` payload kept.
pub const MAX_CONFIG: usize = 64 << 10;

/// Most parameter sets kept from one configuration record.
const MAX_PARAMS: usize = 64;

/// One coded frame or sound packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// Where it lies in the file.
    pub offset: u64,
    pub size: u32,
    /// When it is shown (its composition time, the edit list applied), in the track's timescale.
    pub pts: i64,
    /// A decoder can start here (a sync sample).
    pub sync: bool,
}

/// What the track's sample entry says about its codec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Config {
    /// H.264 (`avc1`, `avc3`): NAL units prefixed by `length_size` bytes, and the SPS and PPS of
    /// the `avcC` record (an `avc3` stream may carry more in band).
    Avc { length_size: usize, params: Vec<Vec<u8>> },
    /// HEVC (`hvc1`, `hev1`): as H.264, with the VPS, SPS and PPS of the `hvcC` record.
    Hevc { length_size: usize, params: Vec<Vec<u8>> },
    /// AAC (`mp4a` with object type 0x40, 0x66–0x68): its `AudioSpecificConfig`.
    Aac { config: Vec<u8> },
    /// Any other codec, by its sample entry code (or the object type of an `mp4a` that is not AAC).
    Other(String),
}

/// One track: its samples in decode order.
#[derive(Clone, Debug)]
pub struct Track {
    /// Ticks per second of every time in [`Track::samples`].
    pub timescale: u64,
    pub samples: Vec<Sample>,
    pub config: Config,
    /// The sample entry's code (`avc1`, `hvc1`, `mp4a`, …).
    pub code: String,
}

impl Track {
    /// A sample's time in milliseconds (negative before the start the edit list sets).
    pub fn millis(&self, pts: i64) -> i64 {
        let ms = i128::from(pts) * 1000 / i128::from(self.timescale.max(1));
        i64::try_from(ms).unwrap_or(if ms < 0 { i64::MIN } else { i64::MAX })
    }
}

/// The first track of `handler` (`vide` or `soun`) in the movie box of `bytes` (the `ftyp` and
/// `moov` boxes `read_movie` reads), or `None` when there is none.
pub fn find(bytes: &[u8], handler: &[u8; 4], work: &mut Work) -> Result<Option<Track>> {
    let moov = Boxes::top(bytes).find(b"moov", None, work)?.ok_or(Error::Truncated)?;
    let movie_scale = match child(bytes, &moov, 0, b"mvhd", work)? {
        Some(mvhd) => {
            let b = body(bytes, &mvhd)?;
            u64::from(if b.first() == Some(&1) { u32_be(b, 20)? } else { u32_be(b, 12)? })
        }
        None => 0,
    };
    let mut tracks = Boxes::children(bytes, &moov, 0)?;
    while let Some(trak) = tracks.next(work)? {
        if &trak.kind != b"trak" {
            continue;
        }
        let Some(mdia) = child(bytes, &trak, 0, b"mdia", work)? else { continue };
        let Some(hdlr) = child(bytes, &mdia, 0, b"hdlr", work)? else { continue };
        if body(bytes, &hdlr)?.get(8..12) != Some(&handler[..]) {
            continue;
        }
        return read_track(bytes, &trak, &mdia, movie_scale, work).map(Some);
    }
    Ok(None)
}

fn body<'a>(bytes: &'a [u8], b: &BoxRef) -> Result<&'a [u8]> {
    bytes.get(b.body.clone()).ok_or(Error::Truncated)
}

/// The table of a full box: its entry count (at `count_at`) checked against the entries the box
/// can hold (`entry` bytes each, starting at `first`).
fn table(b: &[u8], count_at: usize, first: usize, entry: usize) -> Result<(usize, &[u8])> {
    let count = u32_be(b, count_at)? as usize;
    let room = b.len().saturating_sub(first) / entry.max(1);
    if count > room {
        return Err(Error::Malformed(format!("a sample table of {count} entries in {} bytes", b.len())));
    }
    Ok((count, b.get(first..).unwrap_or_default()))
}

fn read_track(bytes: &[u8], trak: &BoxRef, mdia: &BoxRef, movie_scale: u64, work: &mut Work) -> Result<Track> {
    let mdhd = child(bytes, mdia, 0, b"mdhd", work)?.ok_or_else(|| Error::Malformed("a track without a media header".into()))?;
    let m = body(bytes, &mdhd)?;
    let timescale = u64::from(match m.first() {
        Some(1) => u32_be(m, 20)?,
        Some(0) => u32_be(m, 12)?,
        _ => return Err(Error::Malformed("a media header of an unknown version".into())),
    });
    if timescale == 0 {
        return Err(Error::Malformed("a track with a timescale of 0".into()));
    }
    let stbl = path(bytes, mdia, &[b"minf", b"stbl"], work)?.ok_or_else(|| Error::Malformed("a track without a sample table".into()))?;
    let mut boxes: [Option<BoxRef>; 8] = Default::default();
    const KINDS: [&[u8; 4]; 8] = [b"stsd", b"stts", b"ctts", b"stss", b"stsc", b"stsz", b"stco", b"co64"];
    let mut compact = false;
    let mut tables = Boxes::children(bytes, &stbl, 0)?;
    while let Some(t) = tables.next(work)? {
        let kind = if &t.kind == b"stz2" { b"stsz" } else { &t.kind };
        if &t.kind == b"stz2" && boxes[5].is_none() {
            compact = true;
        }
        if let Some(i) = KINDS.iter().position(|k| *k == kind)
            && let Some(slot) = boxes.get_mut(i)
            && slot.is_none()
        {
            *slot = Some(t);
        }
    }
    let [stsd, stts, ctts, stss, stsc, stsz, stco, co64] = boxes;
    let missing = |what: &str| Error::Malformed(format!("a sample table without {what}"));
    let stsd = stsd.ok_or_else(|| missing("a sample description"))?;
    let (code, config) = sample_entry(bytes, &stsd, work)?;

    let sizes = sample_sizes(body(bytes, &stsz.ok_or_else(|| missing("sample sizes"))?)?, compact)?;
    let offsets = match (&stco, &co64) {
        (Some(b), _) => chunk_offsets(body(bytes, b)?, false)?,
        (None, Some(b)) => chunk_offsets(body(bytes, b)?, true)?,
        (None, None) => return Err(missing("chunk offsets")),
    };
    let stsc = body(bytes, &stsc.ok_or_else(|| missing("a sample-to-chunk table"))?)?;
    let mut samples = place(&sizes, &offsets, stsc)?;

    // Decode times, then composition offsets.
    let stts = body(bytes, &stts.ok_or_else(|| missing("sample times"))?)?;
    let (entries, t) = table(stts, 4, 8, 8)?;
    let mut dts: i64 = 0;
    let mut i = 0usize;
    'times: for e in 0..entries {
        let n = u32_be(t, e * 8)? as usize;
        let delta = i64::from(u32_be(t, e * 8 + 4)?);
        for _ in 0..n {
            let Some(s) = samples.get_mut(i) else { break 'times };
            s.pts = dts;
            dts = dts.saturating_add(delta);
            i += 1;
        }
    }
    if let Some(ctts) = &ctts {
        let c = body(bytes, ctts)?;
        let (entries, t) = table(c, 4, 8, 8)?;
        let mut i = 0usize;
        'offsets: for e in 0..entries {
            let n = u32_be(t, e * 8)? as usize;
            // Version 0 says unsigned, but writers store negative offsets there too: read both signed.
            let off = i64::from(u32_be(t, e * 8 + 4)? as i32);
            for _ in 0..n {
                let Some(s) = samples.get_mut(i) else { break 'offsets };
                s.pts = s.pts.saturating_add(off);
                i += 1;
            }
        }
    }
    // Sync samples; without the table every sample is one.
    match &stss {
        None => samples.iter_mut().for_each(|s| s.sync = true),
        Some(stss) => {
            let (entries, t) = table(body(bytes, stss)?, 4, 8, 4)?;
            for e in 0..entries {
                let n = u32_be(t, e * 4)? as usize;
                if let Some(s) = n.checked_sub(1).and_then(|i| samples.get_mut(i)) {
                    s.sync = true;
                }
            }
        }
    }
    // The edit list: an empty edit first delays the track, the first media edit sets its start.
    let shift = edit_shift(bytes, trak, timescale, movie_scale, work)?;
    if shift != 0 {
        samples.iter_mut().for_each(|s| s.pts = s.pts.saturating_sub(shift));
    }
    Ok(Track { timescale, samples, config, code })
}

/// Every sample's size, from `stsz` (a fixed size, or one per sample) or `stz2` (4, 8 or 16 bits).
fn sample_sizes(b: &[u8], compact: bool) -> Result<Vec<u32>> {
    let check = |n: usize| {
        if n > MAX_SAMPLES { Err(Error::TooMany("samples in a video track")) } else { Ok(()) }
    };
    if compact {
        let bits = usize::from(*b.get(7).ok_or(Error::Truncated)?);
        let count = u32_be(b, 8)? as usize;
        check(count)?;
        let data = b.get(12..).unwrap_or_default();
        if !matches!(bits, 4 | 8 | 16) || count.saturating_mul(bits).div_ceil(8) > data.len() {
            return Err(Error::Malformed(format!("a compact sample size table of {count} {bits}-bit entries")));
        }
        return Ok((0..count)
            .map(|i| match bits {
                4 => data.get(i / 2).map_or(0, |v| if i % 2 == 0 { u32::from(v >> 4) } else { u32::from(v & 15) }),
                8 => data.get(i).map_or(0, |&v| u32::from(v)),
                _ => u16_be(data, i * 2).map_or(0, u32::from),
            })
            .collect());
    }
    let fixed = u32_be(b, 4)?;
    let count = u32_be(b, 8)? as usize;
    check(count)?;
    if fixed != 0 {
        return Ok(vec![fixed; count]);
    }
    let (_, t) = table(b, 8, 12, 4)?;
    (0..count).map(|i| u32_be(t, i * 4)).collect()
}

fn chunk_offsets(b: &[u8], wide: bool) -> Result<Vec<u64>> {
    let width = if wide { 8 } else { 4 };
    let (count, t) = table(b, 4, 8, width)?;
    if count > MAX_SAMPLES {
        return Err(Error::TooMany("chunks in a video track"));
    }
    (0..count).map(|i| if wide { u64_be(t, i * 8) } else { u32_be(t, i * 4).map(u64::from) }).collect()
}

/// Lays the samples into their chunks: `stsc` runs of (first chunk, samples per chunk).
fn place(sizes: &[u32], offsets: &[u64], stsc: &[u8]) -> Result<Vec<Sample>> {
    let (entries, t) = table(stsc, 4, 8, 12)?;
    let mut samples = Vec::new();
    samples.try_reserve_exact(sizes.len()).map_err(|_| Error::TooMany("samples in a video track"))?;
    let mut next = 0usize;
    for e in 0..entries {
        let first = u32_be(t, e * 12)? as usize;
        let per_chunk = u32_be(t, e * 12 + 4)? as usize;
        let last = if e + 1 < entries { u32_be(t, (e + 1) * 12)? as usize } else { offsets.len().saturating_add(1) };
        if first == 0 || last < first {
            return Err(Error::Malformed("a sample-to-chunk table out of order".into()));
        }
        for chunk in first..last.min(offsets.len().saturating_add(1)) {
            let Some(&base) = offsets.get(chunk - 1) else { break };
            let mut at = base;
            for _ in 0..per_chunk {
                let Some(&size) = sizes.get(next) else { return Ok(samples) };
                samples.push(Sample { offset: at, size, pts: 0, sync: false });
                at = at.saturating_add(u64::from(size));
                next += 1;
            }
        }
    }
    if samples.len() < sizes.len() {
        return Err(Error::Malformed(format!("{} samples, but chunks for {}", sizes.len(), samples.len())));
    }
    Ok(samples)
}

/// The ticks the edit list moves the track's times back by (negative: forward).
fn edit_shift(bytes: &[u8], trak: &BoxRef, timescale: u64, movie_scale: u64, work: &mut Work) -> Result<i64> {
    let Some(elst) = path(bytes, trak, &[b"edts", b"elst"], work)? else { return Ok(0) };
    let b = body(bytes, &elst)?;
    let v1 = b.first() == Some(&1);
    let entry = if v1 { 20 } else { 12 };
    let (entries, t) = table(b, 4, 8, entry)?;
    let mut delay: i64 = 0;
    for e in 0..entries.min(16) {
        let at = e * entry;
        let (duration, media_time) =
            if v1 { (u64_be(t, at)?, u64_be(t, at + 8)? as i64) } else { (u64::from(u32_be(t, at)?), i64::from(u32_be(t, at + 4)? as i32)) };
        if media_time == -1 {
            // An empty edit: the track starts later, by its duration in the movie's timescale.
            if movie_scale > 0 {
                let ticks = u128::from(duration) * u128::from(timescale) / u128::from(movie_scale);
                delay = delay.saturating_add(i64::try_from(ticks).unwrap_or(i64::MAX));
            }
            continue;
        }
        return Ok(media_time.saturating_sub(delay));
    }
    Ok(delay.saturating_neg())
}

/// The first sample entry: its code and the codec configuration it carries.
fn sample_entry(bytes: &[u8], stsd: &BoxRef, work: &mut Work) -> Result<(String, Config)> {
    let entry = Boxes::children(bytes, stsd, 8)?.next(work)?.ok_or_else(|| Error::Malformed("an empty sample description".into()))?;
    let code: String = String::from_utf8_lossy(&entry.kind).trim_end().to_string();
    let config = match &entry.kind {
        b"avc1" | b"avc3" => match child(bytes, &entry, 78, b"avcC", work)? {
            Some(c) => avc_config(body(bytes, &c)?)?,
            None => return Err(Error::Malformed("an H.264 track without its configuration".into())),
        },
        b"hvc1" | b"hev1" => match child(bytes, &entry, 78, b"hvcC", work)? {
            Some(c) => hevc_config(body(bytes, &c)?)?,
            None => return Err(Error::Malformed("an HEVC track without its configuration".into())),
        },
        b"mp4a" => mp4a_config(bytes, &entry, work)?,
        _ => Config::Other(code.clone()),
    };
    Ok((code, config))
}

/// A parameter set of `len` bytes at `at`, checked.
fn param(b: &[u8], at: usize, len: usize) -> Result<Vec<u8>> {
    if len == 0 || len > MAX_CONFIG {
        return Err(Error::Malformed(format!("a parameter set of {len} bytes")));
    }
    Ok(b.get(at..at + len).ok_or(Error::Truncated)?.to_vec())
}

/// An `avcC` record: the length size and the SPS and PPS.
pub fn avc_config(b: &[u8]) -> Result<Config> {
    if b.first() != Some(&1) {
        return Err(Error::Malformed("an H.264 configuration of an unknown version".into()));
    }
    let length_size = usize::from(*b.get(4).ok_or(Error::Truncated)? & 3) + 1;
    let mut params = Vec::new();
    let mut at = 5usize;
    for set in 0..2 {
        let count = usize::from(*b.get(at).ok_or(Error::Truncated)?) & if set == 0 { 0x1f } else { 0xff };
        at += 1;
        for _ in 0..count {
            let len = usize::from(u16_be(b, at)?);
            params.push(param(b, at + 2, len)?);
            at += 2 + len;
            if params.len() > MAX_PARAMS {
                return Err(Error::TooMany("parameter sets"));
            }
        }
    }
    Ok(Config::Avc { length_size, params })
}

/// An `hvcC` record: the length size and the parameter sets of its arrays.
pub fn hevc_config(b: &[u8]) -> Result<Config> {
    if b.len() < 23 {
        return Err(Error::Truncated);
    }
    let length_size = usize::from(*b.get(21).ok_or(Error::Truncated)? & 3) + 1;
    let arrays = usize::from(*b.get(22).ok_or(Error::Truncated)?);
    let mut params = Vec::new();
    let mut at = 23usize;
    for _ in 0..arrays {
        let count = usize::from(u16_be(b, at + 1)?);
        at += 3;
        for _ in 0..count {
            let len = usize::from(u16_be(b, at)?);
            params.push(param(b, at + 2, len)?);
            at += 2 + len;
            if params.len() > MAX_PARAMS {
                return Err(Error::TooMany("parameter sets"));
            }
        }
    }
    Ok(Config::Hevc { length_size, params })
}

/// An `mp4a` sample entry: its `esds` (directly, or in a QuickTime `wave` box), and from it the
/// AAC configuration.
fn mp4a_config(bytes: &[u8], entry: &BoxRef, work: &mut Work) -> Result<Config> {
    let b = body(bytes, entry)?;
    // A sound sample entry is 28 bytes; QuickTime's version 1 adds 16, version 2 adds 36.
    let skip = match u16_be(b, 8).unwrap_or(0) {
        1 => 44,
        2 => 64,
        _ => 28,
    };
    let esds = match child(bytes, entry, skip, b"esds", work)? {
        Some(e) => Some(e),
        None => match child(bytes, entry, skip, b"wave", work)? {
            Some(wave) => child(bytes, &wave, 0, b"esds", work)?,
            None => None,
        },
    };
    let Some(esds) = esds else { return Ok(Config::Other("mp4a".into())) };
    esds_config(body(bytes, &esds)?.get(4..).unwrap_or_default())
}

/// One descriptor at `at` (ISO/IEC 14496-1 §8.3.3: a tag, then a size in 1–4 bytes of 7 bits):
/// its tag and payload range.
fn descriptor(b: &[u8], at: usize) -> Result<(u8, std::ops::Range<usize>)> {
    let tag = *b.get(at).ok_or(Error::Truncated)?;
    let mut size = 0usize;
    let mut pos = at + 1;
    for _ in 0..4 {
        let v = *b.get(pos).ok_or(Error::Truncated)?;
        pos += 1;
        size = (size << 7) | usize::from(v & 0x7f);
        if v & 0x80 == 0 {
            break;
        }
    }
    let end = pos.checked_add(size).filter(|&e| e <= b.len()).ok_or(Error::Truncated)?;
    Ok((tag, pos..end))
}

/// An `esds` payload (after its version and flags): AAC's `AudioSpecificConfig`, or the object
/// type of another codec.
pub fn esds_config(b: &[u8]) -> Result<Config> {
    let (tag, es) = descriptor(b, 0)?;
    if tag != 3 {
        return Err(Error::Malformed("an elementary stream descriptor of another tag".into()));
    }
    let flags = *b.get(es.start + 2).ok_or(Error::Truncated)?;
    let mut at = es.start + 3;
    if flags & 0x80 != 0 {
        at += 2;
    }
    if flags & 0x40 != 0 {
        at += 1 + usize::from(*b.get(at).ok_or(Error::Truncated)?);
    }
    if flags & 0x20 != 0 {
        at += 2;
    }
    let (tag, dc) = descriptor(b, at)?;
    if tag != 4 {
        return Err(Error::Malformed("a decoder configuration of another tag".into()));
    }
    let object = *b.get(dc.start).ok_or(Error::Truncated)?;
    // MPEG-4 audio, and the three MPEG-2 AAC profiles.
    if !matches!(object, 0x40 | 0x66..=0x68) {
        let name = match object {
            0x69 | 0x6b => "MP3".to_string(),
            0xa5 => "AC-3".to_string(),
            0xa6 => "E-AC-3".to_string(),
            0xad => "Opus".to_string(),
            _ => format!("object type 0x{object:02x}"),
        };
        return Ok(Config::Other(name));
    }
    let after = dc.start + 13;
    if after >= dc.end {
        return Err(Error::Malformed("an AAC track without its configuration".into()));
    }
    let (tag, asc) = descriptor(b, after)?;
    if tag != 5 || asc.is_empty() || asc.len() > MAX_CONFIG {
        return Err(Error::Malformed("an AAC track without its configuration".into()));
    }
    Ok(Config::Aac { config: b.get(asc).ok_or(Error::Truncated)?.to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_records_are_read_and_bounded() {
        // avcC: version 1, profile, compatibility, level, 4-byte lengths, one SPS, one PPS.
        let avcc = [1, 66, 0xc0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 1, 0x68];
        assert_eq!(avc_config(&avcc), Ok(Config::Avc { length_size: 4, params: vec![vec![0x67, 0x42], vec![0x68]] }));
        assert_eq!(avc_config(&avcc[..10]), Err(Error::Truncated));
        let mut empty_set = avcc;
        empty_set[7] = 0;
        assert!(matches!(avc_config(&empty_set), Err(Error::Malformed(_))));
        // esds: ES_Descriptor(3) → DecoderConfigDescriptor(4, AAC) → DecoderSpecificInfo(5).
        let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
        assert_eq!(esds_config(&esds), Ok(Config::Aac { config: vec![0x11, 0x88] }));
        let mut mp3 = esds;
        mp3[7] = 0x6b;
        assert_eq!(esds_config(&mp3), Ok(Config::Other("MP3".into())));
        assert_eq!(esds_config(&esds[..20]), Err(Error::Truncated));
    }

    #[test]
    fn tables_never_claim_more_than_their_box_holds() {
        // 1,000,000 entries claimed in a 16-byte box.
        let b = [0, 0, 0, 0, 0, 0x0f, 0x42, 0x40, 0, 0, 0, 1, 0, 0, 0, 1];
        assert!(matches!(table(&b, 4, 8, 8), Err(Error::Malformed(_))));
        assert!(matches!(sample_sizes(&[0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff], false), Err(Error::TooMany(_))));
        // Chunks of two samples each: 3 sizes over chunks at 100 and 200.
        let stsc = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1];
        let s = place(&[10, 20, 30], &[100, 200], &stsc).unwrap();
        assert_eq!(s.iter().map(|s| s.offset).collect::<Vec<_>>(), [100, 110, 200]);
        assert!(matches!(place(&[10, 20, 30], &[100], &stsc), Err(Error::Malformed(_))));
    }
}
