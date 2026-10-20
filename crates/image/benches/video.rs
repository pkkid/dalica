//! Probing videos (`development.md` §8, `formats.md` §1, `roadmap.md` M15.34): what a scan reads of
//! each video, its container's facts without a frame decoded, over 1 000 files. Run it with
//!
//! ```sh
//! cargo xtask bench video_probe                                        # release, history
//! cargo bench -p dalica-image --bench video_probe -- --json
//! ```
//!
//! **The files.** `--files` (1 000) containers made here and written under this target directory's
//! temporary folder (removed after; not timed), in thirds as phones write them: MP4 with the movie
//! box after the media data (Android), QuickTime `.MOV` with it after too and a location and
//! creation date in its user data (iPhone), and Matroska. Each is a 30-second clip at 30 frames a
//! second with sound: 900 video samples (a key frame each second) and 1 407 AAC packets, so each
//! sample table is as long as a phone's; the samples are a few bytes each (a probe never reads
//! them). The H.264 parameter sets name a 1920 × 1080 picture.
//!
//! **The probe.** [`video::probe_file`] on every file, as the scan asks it (the `ftyp` and `moov`
//! boxes read, the media data skipped; a Matroska file's first megabyte), `--rounds` (7) times over
//! all of them, the files in the page cache (they were just written). Each round must give every
//! file its 30 s, its size and its sound. Row `probe <n>`: a round's time (`p50_ms`, `p95_ms`,
//! `max_ms` by nearest rank over the rounds) and `per_file_us` from the median.
//!
//! `cargo xtask budgets` judges the median within [`PROBE_BUDGET_MS`]; `--check` makes the bench
//! judge it itself (in release, over 1 000 files or more), `--plant-ms N` adds N ms to it. Without
//! `--bench` (under `cargo test --benches`) it runs a smoke pass: 30 files, two rounds.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_image::Limits;
use dalica_image::formats::video;
use serde_json::{Value, json};

/// The budget (`development.md` §8): 1.5 × the first measurement (the median of three runs,
/// release on two hardware threads, `taskset -c 0,1`, 2026-10-20: 6.65 ms a round of 1 000 files),
/// rounded up to 1 ms.
const PROBE_BUDGET_MS: f64 = 10.0;
/// The files the budget holds for.
const FILES: usize = 1_000;
/// A clip's length, frame rate and key frame interval.
const SECONDS: u32 = 30;
const FPS: u32 = 30;

struct Args {
    files: usize,
    rounds: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args { files: if full { FILES } else { 30 }, rounds: if full { 7 } else { 2 }, json: false, check: false, plant_ms: 0.0 };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--files" => a.files = value()?.parse().map_err(|e| format!("--files: {e}"))?,
            "--rounds" => a.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --files N, --rounds N, --check, --plant-ms N, --json)")),
        }
    }
    if !(3..=100_000).contains(&a.files) || !(1..=100).contains(&a.rounds) || !(0.0..=1000.0).contains(&a.plant_ms) {
        return Err("--files must be 3..=100000, --rounds 1..=100 and --plant-ms 0..=1000".into());
    }
    Ok(a)
}

// --- The parameter sets ----------------------------------------------------------------------------

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
    fn trailing(mut self) -> Vec<u8> {
        self.u(1, 1);
        while self.n != 0 {
            self.u(1, 0);
        }
        self.out
    }
}

fn nal(kind: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = vec![(3 << 5) | kind];
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

/// Baseline, level 4, 1920 × 1080 (120 × 68 macroblocks, 8 rows cropped).
fn sps() -> Vec<u8> {
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 40).ue(0);
    b.ue(0).ue(2).ue(1).u(1, 0);
    b.ue(119).ue(67).u(1, 1).u(1, 1);
    b.u(1, 1).ue(0).ue(0).ue(0).ue(4);
    b.u(1, 0);
    nal(7, &b.trailing())
}

fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0);
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0);
    nal(8, &b.trailing())
}

fn avcc() -> Vec<u8> {
    let (sps, pps) = (sps(), pps());
    let mut avcc = vec![1, 66, 0xc0, 40, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(&sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(&pps);
    avcc
}

/// The samples: video (size, ticks of 1/30 000 s), then sound packets of 1 024 at 48 kHz.
fn samples() -> (Vec<u32>, usize) {
    let frames = (SECONDS * FPS) as usize;
    let video = (0..frames).map(|k| if k % FPS as usize == 0 { 24 } else { 8 }).collect();
    let packets = (u64::from(SECONDS) * 48_000).div_ceil(1024) as usize;
    (video, packets)
}

// --- MP4 and QuickTime ------------------------------------------------------------------------------

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
/// 2026-10-19 14:02:00 UTC in seconds since 1904.
const RECORDED: u32 = 3_875_263_320;

#[allow(clippy::too_many_arguments)]
fn trak(
    id: u32,
    handler: &[u8; 4],
    entry: &[u8],
    timescale: u32,
    delta: u32,
    sizes: &[u32],
    syncs: Option<u32>,
    size: (u32, u32),
    at: &mut u32,
) -> Vec<u8> {
    let total = delta * sizes.len() as u32;
    let mut tkhd = vec![0, 0, 0, 3];
    tkhd.extend(be(RECORDED));
    tkhd.extend(be(RECORDED));
    tkhd.extend(be(id));
    tkhd.extend([0u8; 4]);
    tkhd.extend(be((u64::from(total) * 600 / u64::from(timescale)) as u32));
    tkhd.extend([0u8; 16]);
    tkhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(size.0 << 16));
    tkhd.extend(be(size.1 << 16));
    let mut mdhd = vec![0u8; 4];
    mdhd.extend(be(RECORDED));
    mdhd.extend(be(RECORDED));
    mdhd.extend(be(timescale));
    mdhd.extend(be(total));
    mdhd.extend([0x55, 0xc4, 0, 0]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend(handler);
    hdlr.extend([0u8; 13]);
    let mut stsd = be(1).to_vec();
    stsd.extend(entry);
    let stts = [be(1), be(sizes.len() as u32), be(delta)].concat();
    let mut stsz = be(0).to_vec();
    stsz.extend(be(sizes.len() as u32));
    let mut stco = be(sizes.len() as u32).to_vec();
    for s in sizes {
        stsz.extend(be(*s));
        stco.extend(be(*at));
        *at += *s;
    }
    let stsc = [be(1), be(1), be(1), be(1)].concat();
    let mut stbl = [full(b"stsd", &stsd), full(b"stts", &stts), full(b"stsc", &stsc), full(b"stsz", &stsz), full(b"stco", &stco)].concat();
    if let Some(every) = syncs {
        let keys: Vec<u32> = (0..sizes.len() as u32).filter(|k| k % every == 0).map(|k| k + 1).collect();
        let mut stss = be(keys.len() as u32).to_vec();
        keys.iter().for_each(|k| stss.extend(be(*k)));
        stbl.extend(full(b"stss", &stss));
    }
    let tkhd = bx(b"tkhd", &tkhd);
    let mdia = bx(b"mdia", &[bx(b"mdhd", &mdhd), full(b"hdlr", &hdlr), bx(b"minf", &bx(b"stbl", &stbl))].concat());
    bx(b"trak", &[tkhd, mdia].concat())
}

fn avc1() -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 16]);
    p.extend(1920u16.to_be_bytes());
    p.extend(1080u16.to_be_bytes());
    p.extend(be(0x48_0000));
    p.extend(be(0x48_0000));
    p.extend([0u8; 4]);
    p.extend([0, 1]);
    p.extend([0u8; 32]);
    p.extend([0, 0x18, 0xff, 0xff]);
    p.extend(bx(b"avcC", &avcc()));
    bx(b"avc1", &p)
}

fn mp4a() -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 2, 0, 16, 0, 0, 0, 0]);
    p.extend(be(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x90, 6, 1, 2];
    p.extend(full(b"esds", &esds));
    bx(b"mp4a", &p)
}

/// An MP4 (`quicktime`: a `.MOV` with its brand, and a location and date in `udta`) with `ftyp`,
/// `mdat`, then `moov`; `seed` moves the location.
fn iso(quicktime: bool, seed: usize) -> Vec<u8> {
    let (video, packets) = samples();
    let brand: &[u8; 4] = if quicktime { b"qt  " } else { b"isom" };
    let mut out = bx(b"ftyp", &[&brand[..], &be(0x200), brand, b"mp41"].concat());
    let data_len: u32 = video.iter().sum::<u32>() + 4 * packets as u32;
    let mut at = out.len() as u32 + 8;
    out.extend(bx(b"mdat", &vec![0u8; data_len as usize]));
    let video_trak = trak(1, b"vide", &avc1(), FPS * 1000, 1000, &video, Some(FPS), (1920, 1080), &mut at);
    let sound_trak = trak(2, b"soun", &mp4a(), 48_000, 1024, &vec![4; packets], None, (0, 0), &mut at);
    let ms = SECONDS * 1000;
    let mut mvhd = [be(RECORDED), be(RECORDED)].concat();
    mvhd.extend(be(1000));
    mvhd.extend(be(ms));
    mvhd.extend(be(0x1_0000));
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(3));
    let mut moov = [full(b"mvhd", &mvhd), video_trak, sound_trak].concat();
    if quicktime {
        // `©xyz`: an ISO 6709 location, as iPhones write it.
        let place = format!("+{:07.4}+{:08.4}+012.000/", 40.0 + (seed % 100) as f64 / 100.0, 70.0 + (seed % 37) as f64 / 10.0);
        let mut xyz = (place.len() as u16).to_be_bytes().to_vec();
        xyz.extend([0x15, 0xc7]);
        xyz.extend(place.as_bytes());
        moov.extend(bx(b"udta", &bx(b"\xa9xyz", &xyz)));
    }
    out.extend(bx(b"moov", &moov));
    out
}

// --- Matroska ---------------------------------------------------------------------------------------

fn ebml_id(id: u32) -> Vec<u8> {
    id.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect()
}

fn el(id: u32, data: &[u8]) -> Vec<u8> {
    let mut v = ebml_id(id);
    v.push(0x01);
    v.extend((data.len() as u64).to_be_bytes().iter().skip(1));
    v.extend_from_slice(data);
    v
}

fn el_uint(id: u32, v: u64) -> Vec<u8> {
    el(id, &v.to_be_bytes())
}

/// A Matroska clip: header, segment information with its duration, the two tracks, then a
/// cluster a second with the video's blocks and the sound's.
fn mkv() -> Vec<u8> {
    let (video, packets) = samples();
    let pixels = [el_uint(0xb0, 1920), el_uint(0xba, 1080)].concat();
    let video_track = [el_uint(0xd7, 1), el_uint(0x83, 1), el(0x86, b"V_MPEG4/ISO/AVC"), el(0x63a2, &avcc()), el(0xe0, &pixels)].concat();
    let sound_track = [el_uint(0xd7, 2), el_uint(0x83, 2), el(0x86, b"A_AAC"), el(0x63a2, &[0x11, 0x90])].concat();
    let tracks = [el(0xae, &video_track), el(0xae, &sound_track)].concat();
    let duration = el(0x4489, &(f64::from(SECONDS) * 1000.0).to_be_bytes());
    let info = [el_uint(0x2a_d7b1, 1_000_000), duration].concat();
    let mut segment = [el(0x1549_a966, &info), el(0x1654_ae6b, &tracks)].concat();
    let block = |track: u8, rel: i16, key: bool, len: usize| {
        let mut b = vec![0x80 | track];
        b.extend(rel.to_be_bytes());
        b.push(if key { 0x80 } else { 0 });
        b.extend(std::iter::repeat_n(0u8, len));
        b
    };
    let mut packet = 0;
    for second in 0..SECONDS as usize {
        let mut cluster = el_uint(0xe7, second as u64 * 1000);
        let first = second * FPS as usize;
        for (k, size) in video.iter().enumerate().skip(first).take(FPS as usize) {
            let rel = ((k - first) as u64 * 1000 / u64::from(FPS)) as i16;
            cluster.extend(el(0xa3, &block(1, rel, k % FPS as usize == 0, *size as usize)));
            while packet < packets && (packet as u64 * 1024 * 1000 / 48_000) < ((k + 1) as u64 * 1000 / u64::from(FPS)) {
                let t = (packet as u64 * 1024 * 1000 / 48_000) as i64 - second as i64 * 1000;
                cluster.extend(el(0xa3, &block(2, t as i16, true, 4)));
                packet += 1;
            }
        }
        segment.extend(el(0x1f43_b675, &cluster));
    }
    let header = el(0x1a45_dfa3, &[el(0x4282, b"matroska"), el_uint(0x4287, 4)].concat());
    [header, el(0x1853_8067, &segment)].concat()
}

// --- The bench -------------------------------------------------------------------------------------

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

/// Writes the files into `dir`: the third of each kind.
fn write(dir: &Path, n: usize) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).expect("the files' folder");
    let (android, matroska) = (iso(false, 0), mkv());
    (0..n)
        .map(|i| {
            let (name, bytes) = match i % 3 {
                0 => (format!("VID_{i:05}.mp4"), android.clone()),
                1 => (format!("IMG_{i:05}.MOV"), iso(true, i)),
                _ => (format!("clip_{i:05}.mkv"), matroska.clone()),
            };
            let path = dir.join(name);
            std::fs::write(&path, bytes).expect("a file written");
            path
        })
        .collect()
}

/// One probe of every file, checked.
fn round(paths: &[PathBuf], limits: &Limits) -> Duration {
    let started = Instant::now();
    for path in paths {
        let info = video::probe_file(path, limits).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            info.duration_ms == Some(u64::from(SECONDS) * 1000) && info.size == Some((1920, 1080)) && info.has_sound,
            "{}: {info:?}",
            path.display()
        );
        std::hint::black_box(&info);
    }
    started.elapsed()
}

fn bench(a: &Args) -> Value {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("video-probe-{}", std::process::id()));
    let paths = write(&dir, a.files);
    let bytes: u64 = paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let limits = Limits::default();
    // One round to warm the page cache and the allocator, not timed.
    round(&paths, &limits);
    let mut t: Vec<f64> = (0..a.rounds).map(|_| ms(round(&paths, &limits))).collect();
    let _ = std::fs::remove_dir_all(&dir);
    t.sort_by(f64::total_cmp);
    let p50 = rank(&t, 50);
    json!({
        "name": format!("probe {}", a.files),
        "samples": t.len(),
        "p50_ms": p50,
        "p95_ms": rank(&t, 95),
        "max_ms": t.last().copied().unwrap_or(0.0),
        "per_file_us": p50 * 1000.0 / a.files as f64,
        "files": a.files,
        "bytes": bytes,
    })
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("video_probe: {e}");
            std::process::exit(2);
        }
    };
    let row = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "video_probe",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "files": a.files,
        "rows": [row],
    });
    if a.json {
        println!("{report}");
    } else {
        println!(
            "{}: median {:.2} ms a round (p95 {:.2}, max {:.2}), {:.1} µs a file",
            row["name"].as_str().unwrap_or(""),
            row["p50_ms"].as_f64().unwrap_or(0.0),
            row["p95_ms"].as_f64().unwrap_or(0.0),
            row["max_ms"].as_f64().unwrap_or(0.0),
            row["per_file_us"].as_f64().unwrap_or(0.0),
        );
    }
    if a.check && mode == "release" && a.files >= FILES {
        let judged = row["p50_ms"].as_f64().unwrap_or(f64::NAN) + a.plant_ms;
        if judged.is_nan() || judged > PROBE_BUDGET_MS {
            eprintln!("video_probe: over budget: probe {}: median {judged:.2} ms, over the {PROBE_BUDGET_MS} ms budget", a.files);
            std::process::exit(1);
        }
    }
}
