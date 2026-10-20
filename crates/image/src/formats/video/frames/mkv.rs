//! A Matroska track's samples: the blocks of its clusters, read element by element from the file.
//!
//! Provenance: written from the public Matroska specification (RFC 9559: `Segment` → `Info`
//! (`TimestampScale`), `Tracks` → `TrackEntry` (`TrackNumber`, `TrackType`, `CodecID`,
//! `CodecPrivate`, `ContentEncodings`), `Cluster` → `Timestamp`, `SimpleBlock` and `BlockGroup` →
//! `Block` / `ReferenceBlock`; a block's header: the track number as a variable-length integer, a
//! signed 16-bit timestamp relative to the cluster's, its flags (keyframe, the lacing kind), then
//! for laced blocks a frame count and Xiph, EBML or fixed sizes) and the codec mappings of the
//! Matroska codec specifications (`V_MPEG4/ISO/AVC` and `V_MPEGH/ISO/HEVC` carry the `avcC` and
//! `hvcC` records as their private data, `A_AAC` the `AudioSpecificConfig`); no code was consulted.
//!
//! Only element headers and block headers are read while indexing (each read at most
//! [`HEAD`] bytes); `Info` and `Tracks` are read whole, each at most [`MAX_HEAD_ELEMENT`] bytes, and
//! parsed with `ebml.rs`. Every element is counted by one [`Work`]; a cluster of unknown size ends
//! at the next element that cannot be its child; a file cut short is indexed as far as it goes.

use super::super::ebml::{self, Elements};
use super::super::{VideoInfo, probe_as};
use super::Input;
use super::track::{Config, MAX_CONFIG, MAX_SAMPLE_BYTES, MAX_SAMPLES, Sample, Track, avc_config, hevc_config};
use crate::limits::{Limits, Work};
use crate::sniff::VideoKind;
use crate::{Error, Result};

const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_a966;
const TIMESTAMP_SCALE: u32 = 0x2a_d7b1;
const TRACKS: u32 = 0x1654_ae6b;
const TRACK_ENTRY: u32 = 0xae;
const TRACK_NUMBER: u32 = 0xd7;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63a2;
const CONTENT_ENCODINGS: u32 = 0x6d80;
const CLUSTER: u32 = 0x1f43_b675;
const TIMESTAMP: u32 = 0xe7;
const SIMPLE_BLOCK: u32 = 0xa3;
const BLOCK_GROUP: u32 = 0xa0;
const BLOCK: u32 = 0xa1;
const REFERENCE_BLOCK: u32 = 0xfb;

/// Elements that end a cluster of unknown size (the segment's other children).
const SEGMENT_CHILDREN: [u32; 8] = [CLUSTER, 0x1c53_bb6b, 0x1254_c367, 0x1043_a770, 0x1941_a469, 0x114d_9b74, INFO, TRACKS];

/// Bytes read for one element or block header.
const HEAD: u32 = 24;

/// Largest `Info` or `Tracks` element read.
const MAX_HEAD_ELEMENT: u64 = 4 << 20;

/// Most elements the index of one file reads.
const MAX_ELEMENTS: usize = 1 << 23;

/// A variable-length integer at the start of `b`: its length and value (marker kept for IDs).
fn vint(b: &[u8], keep_marker: bool, max_len: usize) -> Result<(usize, u64)> {
    let first = *b.first().ok_or(Error::Truncated)?;
    if first == 0 {
        return Err(Error::Malformed("an EBML number with no length marker".into()));
    }
    let len = first.leading_zeros() as usize + 1;
    if len > max_len {
        return Err(Error::Malformed(format!("an EBML number of {len} bytes")));
    }
    let rest = b.get(1..len).ok_or(Error::Truncated)?;
    let lead = if keep_marker { first } else { first & 0xffu8.checked_shr(len as u32).unwrap_or(0) };
    Ok((len, rest.iter().fold(u64::from(lead), |v, &x| (v << 8) | u64::from(x))))
}

/// One element header read from the file: its ID, where its data starts, and its size (`None`:
/// unknown).
struct Head {
    id: u32,
    data: u64,
    size: Option<u64>,
}

fn head(input: &mut Input, at: u64, buf: &mut Vec<u8>) -> Result<Head> {
    let n = input.len().saturating_sub(at).min(u64::from(HEAD)) as u32;
    input.read_at(at, n, buf)?;
    let (id_len, id) = vint(buf, true, 4)?;
    let (size_len, size) = vint(buf.get(id_len..).unwrap_or_default(), false, 8)?;
    let unknown = size == (1u64 << (7 * size_len)) - 1;
    Ok(Head { id: id as u32, data: at + (id_len + size_len) as u64, size: (!unknown).then_some(size) })
}

/// The facts and the first track of `handler` (`vide` or `soun`) of the Matroska file in `input`.
pub fn open(input: &mut Input, handler: &[u8; 4], limits: &Limits, work: &mut Work) -> Result<(VideoInfo, Option<Track>)> {
    let info = facts(input, limits)?;
    let want = if handler == b"vide" { 1 } else { 2 };
    let mut buf = Vec::new();
    let len = input.len();
    // The EBML header, then the segment.
    let ebml = head(input, 0, &mut buf)?;
    let mut at = ebml.data.saturating_add(ebml.size.ok_or_else(|| Error::Malformed("an EBML header of unknown size".into()))?);
    let segment = loop {
        work.step()?;
        let h = head(input, at, &mut buf)?;
        if h.id == SEGMENT {
            break h;
        }
        at = h.data.saturating_add(h.size.ok_or_else(|| Error::Malformed("an element of unknown size before the segment".into()))?);
        if at >= len {
            return Err(Error::Truncated);
        }
    };
    let end = segment.size.map_or(len, |s| segment.data.saturating_add(s).min(len));
    let mut scale: u64 = 1_000_000;
    let mut chosen: Option<(u64, Config, String)> = None;
    let mut samples: Vec<Sample> = Vec::new();
    let mut steps = 0usize;
    let mut at = segment.data;
    while at < end {
        steps += 1;
        if steps > MAX_ELEMENTS {
            return Err(Error::TooMany("elements in a Matroska file"));
        }
        let Ok(h) = head(input, at, &mut buf) else { break };
        let next = h.size.map(|s| h.data.saturating_add(s));
        match h.id {
            INFO | TRACKS => {
                let size = h.size.ok_or_else(|| Error::Malformed("segment information of unknown size".into()))?;
                if size > MAX_HEAD_ELEMENT {
                    return Err(Error::TooMany("bytes in Matroska segment information"));
                }
                let whole = (h.data - at).saturating_add(size);
                input.read_at(at, u32::try_from(whole).unwrap_or(u32::MAX), &mut buf)?;
                let bytes = std::mem::take(&mut buf);
                if h.id == INFO {
                    scale = timestamp_scale(&bytes, work)?.unwrap_or(scale);
                } else if chosen.is_none() {
                    chosen = pick_track(&bytes, want, work)?;
                }
                buf = bytes;
            }
            CLUSTER => {
                let Some((number, _, _)) = &chosen else {
                    // Clusters before the tracks: nothing to index them against.
                    at = match next {
                        Some(n) => n,
                        None => break,
                    };
                    continue;
                };
                let stop = next.map_or(end, |n| n.min(end));
                let reached = cluster(input, h.data, stop, h.size.is_none(), *number, &mut samples, &mut steps, &mut buf)?;
                at = if h.size.is_some() { stop } else { reached };
                continue;
            }
            _ => {}
        }
        at = match next {
            Some(n) => n,
            None => break,
        };
    }
    let Some((_, config, code)) = chosen else { return Ok((info, None)) };
    // Block times are in `scale` nanoseconds: the track's ticks are nanoseconds.
    for s in &mut samples {
        s.pts = s.pts.saturating_mul(i64::try_from(scale).unwrap_or(i64::MAX));
    }
    Ok((info, Some(Track { timescale: 1_000_000_000, samples, config, code })))
}

/// The file's facts, from its first bytes (doubling from 1 MiB up to the header limit).
fn facts(input: &mut Input, limits: &Limits) -> Result<VideoInfo> {
    let len = input.len();
    let mut want: u64 = 1 << 20;
    let mut bytes = Vec::new();
    loop {
        let n = want.min(limits.max_header_bytes).min(len);
        input.read_at(0, u32::try_from(n).unwrap_or(u32::MAX), &mut bytes)?;
        let complete = n >= len.min(limits.max_header_bytes);
        match probe_as(VideoKind::Matroska, &bytes, limits) {
            Err(Error::Truncated) if !complete => want = want.saturating_mul(2),
            result => return result,
        }
    }
}

fn timestamp_scale(bytes: &[u8], work: &mut Work) -> Result<Option<u64>> {
    let Some(info) = Elements::top(bytes).next(work)? else { return Ok(None) };
    Ok(Elements::children(bytes, &info)?
        .find(TIMESTAMP_SCALE, work)?
        .map(|e| ebml::uint(bytes, &e))
        .transpose()?
        .filter(|&s| s > 0 && s <= 1_000_000_000))
}

/// The first track of type `want` (1 video, 2 audio): its number, configuration and codec ID.
fn pick_track(bytes: &[u8], want: u64, work: &mut Work) -> Result<Option<(u64, Config, String)>> {
    let Some(tracks) = Elements::top(bytes).next(work)? else { return Ok(None) };
    let mut entries = Elements::children(bytes, &tracks)?;
    while let Some(entry) = entries.next(work)? {
        if entry.id != TRACK_ENTRY {
            continue;
        }
        let (mut number, mut kind, mut codec, mut private, mut encoded) = (0u64, 0u64, String::new(), None, false);
        let mut fields = Elements::children(bytes, &entry)?;
        while let Some(f) = fields.next(work)? {
            match f.id {
                TRACK_NUMBER => number = ebml::uint(bytes, &f)?,
                TRACK_TYPE => kind = ebml::uint(bytes, &f)?,
                CODEC_ID => codec = super::super::text(ebml::data(bytes, &f)?),
                CODEC_PRIVATE => {
                    let d = ebml::data(bytes, &f)?;
                    if d.len() > MAX_CONFIG {
                        return Err(Error::TooMany("bytes in a Matroska codec configuration"));
                    }
                    private = Some(d.to_vec());
                }
                CONTENT_ENCODINGS => encoded = true,
                _ => {}
            }
        }
        if kind != want || number == 0 {
            continue;
        }
        let config = match (codec.as_str(), private, encoded) {
            (_, _, true) => Config::Other(format!("{codec} (compressed in the container)")),
            ("V_MPEG4/ISO/AVC", Some(p), _) => avc_config(&p)?,
            ("V_MPEGH/ISO/HEVC", Some(p), _) => hevc_config(&p)?,
            (c, Some(p), _) if c.starts_with("A_AAC") && !p.is_empty() => Config::Aac { config: p },
            _ => Config::Other(codec.clone()),
        };
        return Ok(Some((number, config, codec)));
    }
    Ok(None)
}

/// Indexes the blocks of `number` in the cluster whose data spans `start..stop` (an unknown size:
/// up to the first element that cannot be its child). Returns where the cluster ended.
#[allow(clippy::too_many_arguments)]
fn cluster(
    input: &mut Input,
    start: u64,
    stop: u64,
    unknown: bool,
    number: u64,
    samples: &mut Vec<Sample>,
    steps: &mut usize,
    buf: &mut Vec<u8>,
) -> Result<u64> {
    let mut time: i64 = 0;
    let mut at = start;
    while at < stop {
        *steps += 1;
        if *steps > MAX_ELEMENTS {
            return Err(Error::TooMany("elements in a Matroska file"));
        }
        let Ok(h) = head(input, at, buf) else { return Ok(stop) };
        if unknown && SEGMENT_CHILDREN.contains(&h.id) {
            return Ok(at);
        }
        let Some(size) = h.size else { return Err(Error::Malformed("a block of unknown size".into())) };
        let next = h.data.saturating_add(size);
        if next > input.len() {
            // A cut file: what is complete is indexed.
            return Ok(stop);
        }
        match h.id {
            TIMESTAMP => {
                input.read_at(h.data, u32::try_from(size.min(8)).unwrap_or(8), buf)?;
                time = i64::try_from(buf.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b))).unwrap_or(i64::MAX);
            }
            SIMPLE_BLOCK => block(input, h.data, size, number, time, None, samples, buf)?,
            BLOCK_GROUP => {
                let (mut found, mut referenced) = (None, false);
                let mut p = h.data;
                while p < next {
                    *steps += 1;
                    if *steps > MAX_ELEMENTS {
                        return Err(Error::TooMany("elements in a Matroska file"));
                    }
                    let Ok(c) = head(input, p, buf) else { break };
                    let Some(csize) = c.size else { break };
                    match c.id {
                        BLOCK => found = Some((c.data, csize)),
                        REFERENCE_BLOCK => referenced = true,
                        _ => {}
                    }
                    p = c.data.saturating_add(csize);
                }
                if let Some((data, size)) = found {
                    block(input, data, size, number, time, Some(!referenced), samples, buf)?;
                }
            }
            _ => {}
        }
        at = next;
    }
    Ok(stop)
}

/// One block of `size` bytes at `data`: its frames, if it belongs to track `number`.
#[allow(clippy::too_many_arguments)]
fn block(
    input: &mut Input,
    data: u64,
    size: u64,
    number: u64,
    cluster_time: i64,
    key: Option<bool>,
    samples: &mut Vec<Sample>,
    buf: &mut Vec<u8>,
) -> Result<()> {
    // The header and, for laced blocks, every frame size (at most 255 of at most 8 bytes).
    let n = size.min(u64::from(HEAD) + 2048) as u32;
    input.read_at(data, n, buf)?;
    let (tlen, track) = vint(buf, false, 8)?;
    if track != number {
        return Ok(());
    }
    let rel = buf.get(tlen..tlen + 2).ok_or(Error::Truncated)?;
    let rel = match rel {
        &[a, b] => i64::from(i16::from_be_bytes([a, b])),
        _ => return Err(Error::Truncated),
    };
    let flags = *buf.get(tlen + 2).ok_or(Error::Truncated)?;
    let sync = key.unwrap_or(flags & 0x80 != 0);
    let pts = cluster_time.saturating_add(rel);
    let mut at = tlen + 3;
    let lacing = (flags >> 1) & 3;
    let mut sizes: Vec<u64> = Vec::new();
    if lacing != 0 {
        let count = usize::from(*buf.get(at).ok_or(Error::Truncated)?) + 1;
        at += 1;
        match lacing {
            // Xiph: each size but the last as a run of 255s and a final byte.
            1 => {
                for _ in 0..count - 1 {
                    let mut s = 0u64;
                    loop {
                        let b = *buf.get(at).ok_or(Error::Truncated)?;
                        at += 1;
                        s += u64::from(b);
                        if b != 255 {
                            break;
                        }
                    }
                    sizes.push(s);
                }
            }
            // EBML: the first size, then signed differences.
            3 => {
                let (l, first) = vint(buf.get(at..).unwrap_or_default(), false, 8)?;
                at += l;
                sizes.push(first);
                let mut prev = first as i64;
                for _ in 1..count - 1 {
                    let (l, raw) = vint(buf.get(at..).unwrap_or_default(), false, 8)?;
                    at += l;
                    let bias = (1i64 << (7 * l - 1)) - 1;
                    prev = prev.saturating_add(raw as i64 - bias);
                    sizes.push(u64::try_from(prev).map_err(|_| Error::Malformed("a negative laced frame size".into()))?);
                }
            }
            // Fixed: equal sizes.
            _ => {
                let rest = size.saturating_sub(at as u64);
                sizes = vec![rest / count as u64; count - 1];
            }
        }
    }
    let used: u64 = sizes.iter().sum::<u64>() + at as u64;
    let last = size.checked_sub(used).ok_or_else(|| Error::Malformed("laced frames larger than their block".into()))?;
    sizes.push(last);
    let mut offset = data + at as u64;
    for s in sizes {
        if samples.len() >= MAX_SAMPLES {
            return Err(Error::TooMany("samples in a video track"));
        }
        let s32 = u32::try_from(s).ok().filter(|&v| v <= MAX_SAMPLE_BYTES).ok_or(Error::TooMany("bytes in one video sample"))?;
        samples.push(Sample { offset, size: s32, pts, sync });
        offset += s;
    }
    Ok(())
}
