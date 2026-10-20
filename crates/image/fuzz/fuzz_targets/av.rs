//! What the clip targets (`video_frames`, `video_sound`) share: the panic hook, the input's chunks,
//! and the muxer that wraps fuzzed streams into an MP4 (the test suite's writer, compiled in).

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::Mutex;

#[path = "../../tests/clip_writer.rs"]
pub mod clip_writer;

use clip_writer::{IDENTITY, TrackSpec, bx, mp4, sound_entry_with, visual_entry};

/// Most chunks an input is cut into.
const MAX_CHUNKS: usize = 64;

/// libFuzzer's hook aborts on every panic, even one Dalica catches around a third-party decoder
/// (`catch_unwind`, `security.md` §4.2). A panic inside a crates.io dependency is let through to
/// that guard (and named once, so a run reports it); a panic anywhere else is a finding.
pub fn let_decoder_panics_through() {
    let abort = std::panic::take_hook();
    let seen: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        if at.contains("/registry/src/") {
            if seen.lock().map(|mut s| s.insert(at.clone())).unwrap_or(false) {
                eprintln!("[caught by Dalica's guard] a dependency panicked at {at}");
            }
            return;
        }
        abort(info);
    }));
}

/// The input cut into chunks, each a 16-bit big-endian length and its bytes (the last one cut
/// short at the end of the input).
pub fn chunks(mut data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while data.len() >= 2 && out.len() < MAX_CHUNKS {
        let n = usize::from(u16::from_be_bytes([data[0], data[1]]));
        let rest = &data[2..];
        let take = n.min(rest.len());
        out.push(&rest[..take]);
        data = &rest[take..];
    }
    out
}

/// The inverse of [`chunks`], for the seeds.
pub fn join(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend((p.len() as u16).to_be_bytes());
        out.extend(*p);
    }
    out
}

/// One NAL unit as an MP4 sample (a 4-byte length first).
fn sample(nal: &[u8], raw: bool) -> Vec<u8> {
    if raw {
        return nal.to_vec();
    }
    let mut s = (nal.len() as u32).to_be_bytes().to_vec();
    s.extend(nal);
    s
}

/// A video track of `params` (H.264's SPS then PPS, or HEVC's VPS, SPS, PPS) and `samples` (each
/// one NAL unit, or with `raw` a whole sample with its own length fields), 25 frames a second;
/// every sample a sync sample, or only the first with `first_sync`.
pub fn video_mp4(hevc: bool, params: &[&[u8]], samples: &[&[u8]], raw: bool, first_sync: bool) -> Vec<u8> {
    let config = if hevc {
        let mut c = vec![1, 1, 0x60, 0, 0, 0, 0x90, 0, 0, 0, 0, 0, 93, 0xf0, 0, 0xfc, 0xfd, 0xf8, 0xf8, 0, 0, 0x0f];
        c.push(params.len() as u8);
        for p in params {
            // The array's NAL unit type, from the unit itself.
            c.push(p.first().map_or(0, |b| (b >> 1) & 0x3f));
            c.extend(1u16.to_be_bytes());
            c.extend((p.len() as u16).to_be_bytes());
            c.extend(*p);
        }
        bx(b"hvcC", &c)
    } else {
        let (sps, pps) = params.split_at(1.min(params.len()));
        let mut c = vec![1, 66, 0xc0, 30, 0xff, 0xe0 | sps.len() as u8];
        for p in sps {
            c.extend((p.len() as u16).to_be_bytes());
            c.extend(*p);
        }
        c.push(pps.len() as u8);
        for p in pps {
            c.extend((p.len() as u16).to_be_bytes());
            c.extend(*p);
        }
        bx(b"avcC", &c)
    };
    let samples: Vec<Vec<u8>> = samples.iter().map(|s| sample(s, raw)).collect();
    let n = samples.len() as u32;
    let video = TrackSpec {
        handler: *b"vide",
        entry: visual_entry(if hevc { b"hvc1" } else { b"avc1" }, 64, 64, &config),
        timescale: 1000,
        samples: samples.iter().map(|s| (s.len() as u32, 40)).collect(),
        syncs: Some(if first_sync { vec![1] } else { (1..=n).collect() }),
        matrix: IDENTITY,
        size: (64, 64),
    };
    mp4(&[(video, samples)])
}

/// A sound track of AAC `packets` under the `AudioSpecificConfig` `asc`, 1,024 frames each at
/// 48 kHz.
pub fn sound_mp4(asc: &[u8], packets: &[&[u8]]) -> Vec<u8> {
    let asc = &asc[..asc.len().min(100)];
    let packets: Vec<Vec<u8>> = packets.iter().map(|p| p.to_vec()).collect();
    let spec = TrackSpec {
        handler: *b"soun",
        entry: sound_entry_with(b"mp4a", asc),
        timescale: 48_000,
        samples: packets.iter().map(|p| (p.len() as u32, 1024)).collect(),
        syncs: None,
        matrix: IDENTITY,
        size: (0, 0),
    };
    mp4(&[(spec, packets)])
}
