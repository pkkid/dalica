//! A video's poster, its frame step and its playing frames (`development.md` §8, `editing.md` §11,
//! `roadmap.md` M15.34; the decoders of `docs/video-decoders.md`). Run it with
//!
//! ```sh
//! cargo xtask bench video_play                                         # release, history
//! cargo bench -p dalica-engine --bench video_play -- --json
//! cargo bench -p dalica-engine --features dalica-image/hevc --bench video_play -- --json   # HEVC too
//! ```
//!
//! **The clips.** Two H.264 clips made here, as the tests make theirs (an encoder of our own,
//! ITU-T H.264 Baseline, CAVLC, POC type 2; ISO/IEC 14496-12 boxes muxed by hand, with a silent
//! AAC-LC sound track as long as the clip): `h264 1080p` (1920 × 1080, 30 frames a second, 5 s, a
//! key frame every second) and `h264 4k` (3840 × 2160, 60 frames a second, 3 s, a key frame every
//! second). Each key frame is an IDR of I_PCM macroblocks (a different picture each second), every
//! other frame a P frame whose macroblocks are all skipped, so the decoder predicts every pixel
//! from the frame before (motion compensation over the whole picture) and Dalica's own path converts
//! and scales every frame as it would a phone's. The corpus job's phone clips (`corpus/video`, `cargo
//! xtask corpus video`) are measured too when fetched: `IMG_6851.MOV` (iPhone 13, H.264 High
//! 1080p30), `BTS_iPhone12mini_4K60.MOV` (H.264 High 4K60) and, built with HEVC
//! (`--features dalica-image/hevc`), `IMG_0445.MOV` (HEVC Main 1080p30) and `M-6_Fun_Fact.MOV`
//! (HEVC Main 10 HLG 1080p60); their rows are printed (`phone: true`), never budgeted, since the
//! budgets hold on every machine and the corpus is not on every one. A codec this build does not
//! decode has no rows; the report lists it under `not_played` with the reason.
//!
//! **The library.** Each clip in a folder of its own under a new library on a temporary thumbnail
//! cache, added and scanned (not timed). Then, per clip:
//! - `poster <clip>`: Refresh Thumbnails of its folder (`section.refreshThumbs`) to the session idle
//!   with the clip's 256 thumbnail in memory: the poster frame found (the first lit frame within
//!   10 s), decoded and its thumbnails made and written; once on each of the thumbnail pool's
//!   workers first (not timed: a worker's first poster also touches a frame's memory for the first
//!   time, about 14 ms more at 1080p), then `--samples` (5) times, median;
//! - `open <clip>`: `video.open` to the player's first frame (`video.state` ready with a frame);
//!   printed, not budgeted;
//! - `step <clip>`: `video.step {frames: 1}` to the next frame shown (`video.state` asked every
//!   0.1 ms), `--steps` (30) times from the first frame, median;
//! - `play frame <clip>`: `video.play`, then the owner thread's frames at 60 a second of real time
//!   for `--play-ms` (2 000) or to the clip's end, each one `Session::poll`, the position and the
//!   frame to show, as the edit view's frame asks them (`Session::video_frame`); the frames shown and
//!   `dropped` (decoded too late to show) are counted; p50/p95/max by nearest rank;
//! - `decode frame <clip>`: the player's own decoding, alone: a [`FrameSource`] opened as the
//!   player's worker opens it (at most [`PLAY_SIDE`] on the long side, half the cores as threads,
//!   at most eight) and each frame of the clip (up to `--decode-frames`, 150) asked in turn, timed
//!   from the request to the upright 8-bit sRGB frame; p50/p95/max. A clip plays at its rate only
//!   while this stays under its frame time, and a 60 Hz window shows each frame only when it stays
//!   under 16 ms.
//!
//! **Budgets** ([`BUDGETS`], `cargo xtask budgets`): each procedural clip's poster and step
//! (medians) and its playing frame and decoded frame (p95) within 1.5 × their first measurement,
//! the playing frame (the owner thread's, as the window's frame while a video plays) never above
//! [`FRAME_CAP_MS`]. A decoded frame slower than the clip's frame time (`frame_time_ms`) means the
//! clip cannot play at its rate on that machine: the play row's `dropped` counts it. `--check` makes the bench judge them itself (in
//! release), `--plant-ms N` adds N ms to each judged time. Without `--bench` (under `cargo test
//! --benches`) it runs a smoke pass: two small clips, one sample, a few steps and frames.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::video::PLAY_SIDE;
use dalica_engine::{CacheDir, Options, Session, Thumb, ThumbOptions};
use dalica_image::Limits;
use dalica_image::formats::video::frames::{FrameOptions, FrameSource, MAX_THREADS};
use serde_json::{Value, json};

/// The budgets (`development.md` §8): per row and statistic, 1.5 × the first measurement (the
/// median of three runs, release on two hardware threads, `taskset -c 0,1`, 2026-10-20: the
/// posters 16.8 and 32.7 ms, the steps 7.28 and 14.5 ms, a playing frame 0.036 and 0.032 ms, a
/// decoded frame 13.4 and 25.5 ms at p95), the posters and steps rounded up to 1 ms, the frames to
/// 0.5 ms; a playing frame never above [`FRAME_CAP_MS`].
const BUDGETS: [(&str, &str, f64); 8] = [
    ("poster h264 1080p", "p50_ms", 26.0),
    ("step h264 1080p", "p50_ms", 11.0),
    ("play frame h264 1080p", "p95_ms", 0.5),
    ("decode frame h264 1080p", "p95_ms", 20.5),
    ("poster h264 4k", "p50_ms", 50.0),
    ("step h264 4k", "p50_ms", 22.0),
    ("play frame h264 4k", "p95_ms", 0.5),
    ("decode frame h264 4k", "p95_ms", 38.5),
];
/// A playing video's frame on the owner thread: within a 60 Hz frame at p95, whatever the first
/// measurement.
const FRAME_CAP_MS: f64 = 16.0;
/// Frames per second the edit view's window runs at.
const FPS: f64 = 60.0;
/// The longest one wait may run.
const PATIENCE: Duration = Duration::from_secs(300);

/// A clip made here: its row name, file, size, frame rate, frame count.
struct Made {
    name: &'static str,
    file: &'static str,
    size: (usize, usize),
    fps: u32,
    frames: usize,
}

/// The procedural clips the budgets hold for, and the smoke pass's.
const MADE: [Made; 2] = [
    Made { name: "h264 1080p", file: "clip-1080p.mp4", size: (1920, 1080), fps: 30, frames: 150 },
    Made { name: "h264 4k", file: "clip-4k.mp4", size: (3840, 2160), fps: 60, frames: 180 },
];
const SMOKE: [Made; 2] = [
    Made { name: "h264 1080p", file: "clip-1080p.mp4", size: (320, 180), fps: 30, frames: 40 },
    Made { name: "h264 4k", file: "clip-4k.mp4", size: (640, 360), fps: 60, frames: 40 },
];
/// The phone clips measured when fetched: row name, file under `corpus/video`.
const PHONE: [(&str, &str); 4] = [
    ("phone h264 1080p", "IMG_6851.MOV"),
    ("phone h264 4k", "BTS_iPhone12mini_4K60.MOV"),
    ("phone hevc 1080p", "IMG_0445.MOV"),
    ("phone hevc hdr 1080p60", "M-6_Fun_Fact.MOV"),
];

struct Args {
    full: bool,
    samples: usize,
    steps: usize,
    play_ms: u64,
    decode_frames: usize,
    phones: bool,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        full,
        samples: if full { 5 } else { 1 },
        steps: if full { 30 } else { 3 },
        play_ms: if full { 2000 } else { 300 },
        decode_frames: if full { 150 } else { 10 },
        phones: full,
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--steps" => a.steps = value()?.parse().map_err(|e| format!("--steps: {e}"))?,
            "--play-ms" => a.play_ms = value()?.parse().map_err(|e| format!("--play-ms: {e}"))?,
            "--decode-frames" => a.decode_frames = value()?.parse().map_err(|e| format!("--decode-frames: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--no-phones" => a.phones = false,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => {
                return Err(format!(
                    "unknown argument `{other}` (use --samples N, --steps N, --play-ms N, --decode-frames N, --no-phones, --check, --plant-ms N, --json)"
                ));
            }
        }
    }
    if !(1..=100).contains(&a.samples) || !(1..=1000).contains(&a.steps) || !(100..=60_000).contains(&a.play_ms) {
        return Err("--samples must be 1..=100, --steps 1..=1000 and --play-ms 100..=60000".into());
    }
    if !(1..=10_000).contains(&a.decode_frames) || !(0.0..=1000.0).contains(&a.plant_ms) {
        return Err("--decode-frames must be 1..=10000 and --plant-ms 0..=1000".into());
    }
    Ok(a)
}

// --- An H.264 writer: IDR pictures of I_PCM macroblocks, P pictures of skipped ones ---------------

#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn u(&mut self, bits: u32, v: u64) -> &mut Bits {
        for i in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((v >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                (self.acc, self.n) = (0, 0);
            }
        }
        self
    }
    fn ue(&mut self, v: u64) -> &mut Bits {
        let v1 = v + 1;
        let len = 64 - v1.leading_zeros();
        self.u(len - 1, 0).u(len, v1)
    }
    fn se(&mut self, v: i64) -> &mut Bits {
        self.ue(if v > 0 { (2 * v - 1) as u64 } else { (-2 * v) as u64 })
    }
    fn align(&mut self) -> &mut Bits {
        while self.n != 0 {
            self.u(1, 0);
        }
        self
    }
    /// Whole bytes, at a byte boundary.
    fn bytes(&mut self, b: &[u8]) -> &mut Bits {
        debug_assert_eq!(self.n, 0);
        self.out.extend_from_slice(b);
        self
    }
    fn trailing(mut self) -> Vec<u8> {
        self.u(1, 1);
        self.align();
        self.out
    }
}

/// A NAL unit with its header and emulation prevention.
fn nal(ref_idc: u8, kind: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + rbsp.len() / 64 + 1);
    out.push((ref_idc << 5) | kind);
    let mut zeros = 0;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        zeros = if b == 0 { zeros + 1 } else { 0 };
    }
    out
}

/// Baseline at level 5.1, frame_num of 4 bits, POC type 2, one reference frame; the picture
/// cropped to `w` × `h` from whole macroblocks.
fn sps(w: usize, h: usize) -> Vec<u8> {
    let (mw, mh) = (w.div_ceil(16), h.div_ceil(16));
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 51).ue(0);
    b.ue(0).ue(2).ue(1).u(1, 0);
    b.ue(mw as u64 - 1).ue(mh as u64 - 1).u(1, 1).u(1, 1);
    let (right, bottom) = ((mw * 16 - w) / 2, (mh * 16 - h) / 2);
    if right + bottom > 0 {
        b.u(1, 1).ue(0).ue(right as u64).ue(0).ue(bottom as u64);
    } else {
        b.u(1, 0);
    }
    b.u(1, 0);
    nal(3, 7, &b.trailing())
}

fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0);
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0);
    nal(3, 8, &b.trailing())
}

/// The luma of key picture `key` at macroblock (`mx`, `my`): a diagonal ramp that moves each second.
fn luma(key: usize, mx: usize, my: usize) -> u8 {
    (40 + (mx * 3 + my * 5 + key * 37) % 180) as u8
}

/// Frame `k` of a clip of `size` with a key frame every `gop`, length-prefixed.
fn frame(k: usize, gop: usize, size: (usize, usize)) -> Vec<u8> {
    let (mw, mh) = (size.0.div_ceil(16), size.1.div_ceil(16));
    let idr = k.is_multiple_of(gop);
    let mut b = Bits::default();
    if idr {
        b.ue(0).ue(7).ue(0).u(4, 0);
        b.ue((k / gop % 2) as u64).u(1, 0).u(1, 0);
        b.se(0).ue(1);
        let chroma = [128u8; 128];
        for my in 0..mh {
            for mx in 0..mw {
                b.ue(25).align();
                b.bytes(&[luma(k / gop, mx, my); 256]).bytes(&chroma);
            }
        }
    } else {
        // A P slice: no override, no list modification, sliding-window marking, every
        // macroblock skipped (one mb_skip_run).
        b.ue(0).ue(5).ue(0).u(4, ((k % gop) % 16) as u64);
        b.u(1, 0).u(1, 0).u(1, 0);
        b.se(0).ue(1);
        b.ue((mw * mh) as u64);
    }
    let s = nal(if idr { 3 } else { 2 }, if idr { 5 } else { 1 }, &b.trailing());
    let mut out = (s.len() as u32).to_be_bytes().to_vec();
    out.extend(s);
    out
}

// --- An MP4 writer -------------------------------------------------------------------------------

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn full(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    bx(kind, &[&[0, 0, 0, 0][..], payload].concat())
}

fn be(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];
const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

struct Track {
    id: u32,
    handler: [u8; 4],
    entry: Vec<u8>,
    timescale: u32,
    delta: u32,
    syncs: Option<Vec<u32>>,
    size: (u32, u32),
    sizes: Vec<u32>,
}

fn trak(t: &Track, at: &mut u64) -> Vec<u8> {
    let total = t.delta * t.sizes.len() as u32;
    let mut tkhd = vec![0, 0, 0, 3];
    tkhd.extend([0u8; 8]);
    tkhd.extend(be(t.id));
    tkhd.extend([0u8; 4]);
    tkhd.extend(be((u64::from(total) * 1000 / u64::from(t.timescale)) as u32));
    tkhd.extend([0u8; 16]);
    tkhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(t.size.0 << 16));
    tkhd.extend(be(t.size.1 << 16));
    let mut mdhd = vec![0u8; 8];
    mdhd.extend(be(t.timescale));
    mdhd.extend(be(total));
    mdhd.extend([0x55, 0xc4, 0, 0]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend(t.handler);
    hdlr.extend([0u8; 13]);
    let mut stsd = be(1).to_vec();
    stsd.extend(&t.entry);
    let stts = [be(1), be(t.sizes.len() as u32), be(t.delta)].concat();
    let mut stsz = be(0).to_vec();
    stsz.extend(be(t.sizes.len() as u32));
    let mut co64 = be(t.sizes.len() as u32).to_vec();
    for s in &t.sizes {
        stsz.extend(be(*s));
        co64.extend(at.to_be_bytes());
        *at += u64::from(*s);
    }
    let stsc = [be(1), be(1), be(1), be(1)].concat();
    let mut stbl = [full(b"stsd", &stsd), full(b"stts", &stts), full(b"stsc", &stsc), full(b"stsz", &stsz), full(b"co64", &co64)].concat();
    if let Some(syncs) = &t.syncs {
        let mut stss = be(syncs.len() as u32).to_vec();
        syncs.iter().for_each(|s| stss.extend(be(*s)));
        stbl.extend(full(b"stss", &stss));
    }
    let mdia = bx(b"mdia", &[full(b"mdhd", &mdhd), full(b"hdlr", &hdlr), bx(b"minf", &bx(b"stbl", &stbl))].concat());
    // tkhd is version 0 with its flags (enabled, in movie) already in its first word.
    let tkhd_box = bx(b"tkhd", &tkhd);
    bx(b"trak", &[tkhd_box, mdia].concat())
}

fn avc1(w: u16, h: u16, sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 16]);
    p.extend(w.to_be_bytes());
    p.extend(h.to_be_bytes());
    p.extend(be(0x48_0000));
    p.extend(be(0x48_0000));
    p.extend([0u8; 4]);
    p.extend([0, 1]);
    p.extend([0u8; 32]);
    p.extend([0, 0x18, 0xff, 0xff]);
    let mut avcc = vec![1, 66, 0xc0, 51, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(pps);
    p.extend(bx(b"avcC", &avcc));
    bx(b"avc1", &p)
}

/// AAC-LC mono at 48 kHz.
fn mp4a() -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
    p.extend(full(b"esds", &esds));
    bx(b"mp4a", &p)
}

/// The clip `m` written to `path`: `ftyp`, `mdat` (the video's frames, then the sound's packets),
/// `moov`. Written a frame at a time, so a 4K clip's key frames are never all in memory.
fn write_clip(m: &Made, path: &Path) -> std::io::Result<u64> {
    use std::io::Write;
    let gop = m.fps as usize;
    let (sps, pps) = (sps(m.size.0, m.size.1), pps());
    let ms = m.frames as u64 * 1000 / u64::from(m.fps);
    let packets = (ms * 48).div_ceil(1024) as usize;
    let ftyp = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"avc1", b"mp41"].concat());
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(&ftyp)?;
    // A 64-bit mdat header, its size written once the frames are known.
    let mdat_at = ftyp.len() as u64;
    file.write_all(&[0, 0, 0, 1])?;
    file.write_all(b"mdat")?;
    file.write_all(&0u64.to_be_bytes())?;
    let mut sizes = Vec::with_capacity(m.frames);
    for k in 0..m.frames {
        let f = frame(k, gop, m.size);
        sizes.push(f.len() as u32);
        file.write_all(&f)?;
    }
    for _ in 0..packets {
        file.write_all(&SILENT_AAC)?;
    }
    let data: u64 = sizes.iter().map(|s| u64::from(*s)).sum::<u64>() + 4 * packets as u64;
    let tracks = [
        Track {
            id: 1,
            handler: *b"vide",
            entry: avc1(m.size.0 as u16, m.size.1 as u16, &sps, &pps),
            timescale: m.fps * 1000,
            delta: 1000,
            syncs: Some((0..m.frames as u32).filter(|k| (*k as usize).is_multiple_of(gop)).map(|k| k + 1).collect()),
            size: (m.size.0 as u32, m.size.1 as u32),
            sizes,
        },
        Track { id: 2, handler: *b"soun", entry: mp4a(), timescale: 48_000, delta: 1024, syncs: None, size: (0, 0), sizes: vec![4; packets] },
    ];
    let mut at = mdat_at + 16;
    let traks: Vec<u8> = tracks.iter().flat_map(|t| trak(t, &mut at)).collect();
    // Recorded 2026-10-19 14:02:00 UTC (seconds since 1904).
    let mut mvhd = [be(3_875_263_320), be(3_875_263_320)].concat();
    mvhd.extend(be(1000));
    mvhd.extend(be(ms as u32));
    mvhd.extend(be(0x1_0000));
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(3));
    file.write_all(&bx(b"moov", &[full(b"mvhd", &mvhd), traks].concat()))?;
    let mut file = file.into_inner().map_err(|e| e.into_error())?;
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(mdat_at + 8))?;
    file.write_all(&(16 + data).to_be_bytes())?;
    file.sync_all()?;
    Ok(file.metadata()?.len())
}

// --- Measuring -------------------------------------------------------------------------------------

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn row(name: String, times: &[Duration], extra: &Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let mut r =
        json!({ "name": name, "samples": t.len(), "p50_ms": rank(&t, 50), "p95_ms": rank(&t, 95), "max_ms": t.last().copied().unwrap_or(0.0) });
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// Asks `video.state` every 0.1 ms until `done` holds; the last state.
fn until(s: &mut Session, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        let v = run(s, "video.state", json!({}));
        if done(&v) {
            return v;
        }
        assert!(start.elapsed() < PATIENCE, "{what}: {v}");
        std::thread::sleep(Duration::from_micros(100));
    }
}

/// The threads the player's worker decodes with (`video.rs` → `Worker::run`).
fn player_threads() -> usize {
    std::thread::available_parallelism().map_or(0, |n| (n.get() / 2).min(MAX_THREADS))
}

/// One clip to measure: its row name, its file in the library, and whether it is a phone's.
struct Clip {
    name: String,
    path: PathBuf,
    phone: bool,
}

/// Every row of one clip, or why it has none.
fn measure(a: &Args, s: &mut Session, clip: &Clip) -> Result<Vec<Value>, String> {
    let file = clip.path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let id: PhotoId = s.library().photos().find(|p| p.name == file).map(|p| p.id).ok_or_else(|| format!("{file} is not in the library"))?;
    let facts = run(s, "video.inspect", json!({ "id": id }));
    if facts["playable"] != true {
        return Err(facts["reason"].as_str().unwrap_or("not playable").to_string());
    }
    let size = (facts["displayed"]["width"].as_u64().unwrap_or(0), facts["displayed"]["height"].as_u64().unwrap_or(0));
    let fps = facts["frameRateMilli"].as_f64().unwrap_or(0.0) / 1000.0;
    let duration = facts["durationMs"].as_u64().unwrap_or(0);
    let extra = json!({ "width": size.0, "height": size.1, "fps": fps, "codec": facts["videoCodec"], "phone": clip.phone });
    let name = &clip.name;
    let mut rows = Vec::new();

    // The poster and its thumbnails, made again: first once on each of the pool's workers (not
    // timed; a worker's first poster also pays its first touch of a frame's memory, about 14 ms
    // at 1080p), then timed.
    for _ in 0..dalica_thumbs::default_workers() {
        run(s, "section.refreshThumbs", json!({ "ids": [id] }));
        s.wait_idle();
    }
    let mut posters = Vec::new();
    for _ in 0..a.samples {
        let t = Instant::now();
        run(s, "section.refreshThumbs", json!({ "ids": [id] }));
        s.wait_idle();
        let took = t.elapsed();
        assert!(matches!(s.thumbnail(id, 256), Thumb::Image { .. }), "{name}: the poster's 256 thumbnail is made");
        posters.push(took);
    }
    rows.push(row(format!("poster {name}"), &posters, &extra));

    // Open, then step a frame at a time.
    let t = Instant::now();
    run(s, "video.open", json!({ "id": id }));
    let v = until(s, "opened", |v| v["ready"] == true && v["frame"].is_object());
    rows.push(row(format!("open {name}"), &[t.elapsed()], &extra));
    let count = v["frameCount"].as_u64().unwrap_or(0) as usize;
    let mut steps = Vec::new();
    let mut at = v["frame"]["index"].as_u64().unwrap_or(0) as usize;
    for _ in 0..a.steps.min(count.saturating_sub(at + 1)) {
        let t = Instant::now();
        run(s, "video.step", json!({ "frames": 1 }));
        let want = at + 1;
        until(s, "the step's frame", |v| v["pending"] != true && v["frame"]["index"].as_u64() == Some(want as u64));
        steps.push(t.elapsed());
        at = want;
    }
    rows.push(row(format!("step {name}"), &steps, &extra));

    // Play from the start for `--play-ms` of real time, a frame every 1/60 s.
    run(s, "video.seek", json!({ "ms": 0 }));
    until(s, "back at the start", |v| v["pending"] != true && v["frame"]["index"] == 0);
    let dropped_before = run(s, "video.state", json!({}))["dropped"].as_u64().unwrap_or(0);
    let play_ms = a.play_ms.min(duration.saturating_sub(100)).max(100);
    run(s, "video.play", json!({}));
    let started = Instant::now();
    let tick = Duration::from_secs_f64(1.0 / FPS);
    let mut frames = Vec::new();
    let (mut shown, mut last) = (0usize, None);
    let mut next = started;
    while started.elapsed() < Duration::from_millis(play_ms) {
        next += tick;
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let t = Instant::now();
        s.poll();
        let _ = std::hint::black_box(s.video_position());
        let f = s.video_frame();
        frames.push(t.elapsed());
        let index = f.map(|f| f.index);
        if index != last {
            shown += 1;
            last = index;
        }
    }
    let state = run(s, "video.state", json!({}));
    run(s, "video.pause", json!({}));
    let dropped = state["dropped"].as_u64().unwrap_or(0).saturating_sub(dropped_before);
    let mut play_extra = extra.clone();
    if let Some(o) = play_extra.as_object_mut() {
        o.insert("played_ms".into(), json!(play_ms));
        o.insert("shown".into(), json!(shown));
        o.insert("dropped".into(), json!(dropped));
        o.insert("position_ms".into(), state["positionMs"].clone());
    }
    rows.push(row(format!("play frame {name}"), &frames, &play_extra));
    run(s, "video.close", json!({}));

    // The player's decoding alone.
    let options = FrameOptions { max_side: Some(PLAY_SIDE), threads: player_threads() };
    let mut source = FrameSource::open(&clip.path, &Limits::default(), options).map_err(|e| e.reason())?;
    let mut decodes = Vec::new();
    for _ in 0..a.decode_frames.min(source.frame_count()) {
        let t = Instant::now();
        let f = source.next_frame().map_err(|e| e.reason())?;
        decodes.push(t.elapsed());
        if f.is_none() {
            break;
        }
    }
    let mut decode_extra = extra.clone();
    if let Some(o) = decode_extra.as_object_mut() {
        o.insert("threads".into(), json!(options.threads));
        o.insert("max_side".into(), json!(PLAY_SIDE));
        o.insert("frame_time_ms".into(), json!(if fps > 0.0 { 1000.0 / fps } else { 0.0 }));
    }
    rows.push(row(format!("decode frame {name}"), &decodes, &decode_extra));
    Ok(rows)
}

fn corpus(file: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/video").join(file);
    path.is_file().then_some(path)
}

fn bench(a: &Args) -> (Vec<Value>, Vec<Value>) {
    let dir = tempfile::Builder::new().prefix("video-play-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let photos = dir.path().join("Photos");
    let mut clips = Vec::new();
    for m in if a.full { &MADE } else { &SMOKE } {
        let folder = photos.join(m.name.replace(' ', "-"));
        std::fs::create_dir_all(&folder).expect("the clip's folder");
        let path = folder.join(m.file);
        let bytes = write_clip(m, &path).expect("the clip is written");
        eprintln!("video_play: {} written, {:.1} MB", m.name, bytes as f64 / 1e6);
        clips.push(Clip { name: m.name.to_string(), path, phone: false });
    }
    let mut not_played = Vec::new();
    if a.phones {
        for (name, file) in PHONE {
            let Some(src) = corpus(file) else {
                not_played.push(json!({ "name": name, "reason": format!("corpus/video/{file} is not fetched (cargo xtask corpus video)") }));
                continue;
            };
            let folder = photos.join(name.replace(' ', "-"));
            std::fs::create_dir_all(&folder).expect("the clip's folder");
            let path = folder.join(file);
            std::fs::copy(&src, &path).expect("the phone clip copied");
            clips.push(Clip { name: name.to_string(), path, phone: true });
        }
    }
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).expect("the library opens");
    run(&mut s, "folders.add", json!({ "path": photos }));
    s.wait_idle();
    let mut rows = Vec::new();
    for clip in &clips {
        match measure(a, &mut s, clip) {
            Ok(r) => rows.extend(r),
            Err(why) => not_played.push(json!({ "name": clip.name, "reason": why })),
        }
    }
    (rows, not_played)
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("video_play: {e}");
            std::process::exit(2);
        }
    };
    let (rows, not_played) = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "video_play",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "full": a.full,
        "play_side": PLAY_SIDE,
        "not_played": not_played,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<34} {:>8} {:>10} {:>10} {:>10}", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            println!(
                "{:<34} {:>8} {:>10.3} {:>10.3} {:>10.3}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
        for n in &not_played {
            println!("not played: {}: {}", n["name"].as_str().unwrap_or(""), n["reason"].as_str().unwrap_or(""));
        }
    }
    if a.check && mode == "release" && a.full {
        let mut missed = Vec::new();
        for (name, stat, budget) in BUDGETS {
            let budget = if name.starts_with("play frame") { budget.min(FRAME_CAP_MS) } else { budget };
            let measured = rows.iter().find(|r| r["name"] == name).and_then(|r| r[stat].as_f64());
            let judged = measured.map_or(f64::NAN, |m| m + a.plant_ms);
            if judged.is_nan() || judged > budget {
                missed.push(format!("{name}: {stat} {judged:.2} ms, over the {budget} ms budget"));
            }
        }
        for m in &missed {
            eprintln!("video_play: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
