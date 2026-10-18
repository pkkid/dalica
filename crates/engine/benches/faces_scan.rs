//! The background face scan (`development.md` §8, `faces.md` §4 and §8, `roadmap.md` M13.31): faces
//! embedded per second over `scenes` photos, the memory each worker holds, and the owner thread's
//! time per frame while the scan runs, as the headless window's frames would spend it. Run it with
//!
//! ```sh
//! cargo xtask bench faces_scan                                      # release: 1 000 photos, history
//! cargo bench -p dalica-engine --bench faces_scan -- --photos 1000 --json
//! ```
//!
//! **The models.** Reserved decision 5 has not chosen a model and no model is downloaded for a
//! bench: a models folder (installed as the app installs one: `model.onnx` and `face-model.json`
//! in a folder named by the id) holds seeded networks of the candidates' shapes
//! (`dalica_faces::synthetic`): YuNet's at 640 px, which reports two faces on every photo (the
//! planted cells of the `inference` bench), and SFace's recogniser (`--recogniser auraface` for
//! AuraFace's, a series of its own). The time per face is then what a real model of that shape
//! costs.
//!
//! **The photos.** `--photos` (1 000) `scenes` JPEGs of `--side` pixels on their long side (2048,
//! the most the scan decodes, `faces_scan::EDGE`), mixed aspects, in folders of about 100, are
//! written under `--dir` (this target directory's temporary folder) and scanned into a new library
//! on an empty thumbnail cache that makes no thumbnails (not timed). Removed afterwards.
//!
//! **The scan.** The session runs on a virtual clock moved with real time. The window reports
//! input first, so no photo starts while the models load on a worker; once they are loaded the
//! process's resident memory is read (`rss_base_mib`), the clock is moved past the three seconds of
//! quiet after which the user counts as idle, and frames run at 60 per second of real time, each
//! one reporting the user idle (`Session::user_activity`) and calling `Session::poll`, as the app's
//! window does: the scan runs at the idle pace (about half the cores, at most four photos at once,
//! `faces_scan::Pace`). Until every photo has been searched and every face found is embedded:
//! - row `scan <n>`: `faces` embedded, `scan_s` from the first frame at the idle pace,
//!   `faces_per_s` and `photos_per_s`; `workers`, the most photos in progress at once;
//!   `rss_peak_mib`, the largest resident memory read at a frame; and `per_worker_mib`, the peak
//!   above the base shared by the workers (Linux only: `/proc/self/status`; elsewhere 0);
//! - row `frame while scanning`: the owner thread's time per frame (the activity report and the
//!   poll: results taken in, faces written, photos started, groups kept), p50/p95/max by nearest
//!   rank. The UI's own layout and drawing are measured by the ui benches (`people_places`).
//!
//! `cargo xtask budgets` judges faces per second (a planted slowdown counted once per face), the
//! memory per worker, and the frame's p95 (a planted slowdown added), never above 16 ms; `--check`
//! makes the bench judge them itself (in release, over 1 000 photos or more), `--plant-ms N` adds N
//! ms the same way. Without `--bench` (under `cargo test --benches`) it runs a smoke pass over 12
//! photos of 640 px.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{Timestamp, VirtualClock};
use dalica_engine::faces_scan::{IDLE_AFTER, MANIFEST_FILE, MODEL_FILE, UserActivity};
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_faces::known;
use dalica_faces::manifest::ModelManifest;
use dalica_faces::synthetic::{Planted, Shaped};
use serde_json::{Value, json};

/// The budgets (`development.md` §8): faces per second at least the first measurement ÷ 1.5
/// (rounded down), the memory per worker at most 1.5 × the first measurement (rounded up, and never
/// above the 192 MiB the scan reserves per photo, `faces_scan::WORKER_BYTES`), and a frame's p95
/// 1.5 × the first measurement (rounded up to 0.5 ms), never above [`FRAME_CAP_MS`]. Release on two
/// cores, 2026-10-18, the median of three runs: 11.06 faces/s (2 000 in 180.9 s, one worker),
/// 67.1 MiB per worker, a frame's p95 0.201 ms.
const FACES_PER_S_BUDGET: f64 = 7.3;
const WORKER_MIB_BUDGET: f64 = 105.0;
const FRAME_BUDGET_MS: f64 = 0.5;
const FRAME_CAP_MS: f64 = 16.0;
/// The photos the budgets hold for.
const PHOTOS: usize = 1_000;
const SIDE: u32 = 2048;
/// The detector's input side (the candidate's).
const DETECTOR_SIDE: usize = 640;
/// The faces the detector reports on every photo (stride-16 cells of its 640 px input, box sides
/// in cells): inside the picture whatever its aspect, from a 3:1 panorama to a 2:3 portrait.
const PLANTED: [Planted; 2] = [Planted { row: 6, col: 8, side: 5.0 }, Planted { row: 6, col: 18, side: 5.0 }];
/// Photos per folder on average.
const PER_FOLDER: usize = 100;
/// Frames per second the window runs at.
const FPS: f64 = 60.0;
/// The longest the scan may run.
const TIMEOUT: Duration = Duration::from_secs(3600);
/// The networks' seeds (the `inference` bench's).
const SEED: u32 = 2026;

struct Args {
    photos: usize,
    side: u32,
    recogniser: String,
    dir: PathBuf,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { PHOTOS } else { 12 },
        side: if full { SIDE } else { 640 },
        recogniser: "sface".into(),
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--side" => a.side = value()?.parse().map_err(|e| format!("--side: {e}"))?,
            "--recogniser" => a.recogniser = value()?,
            "--dir" => a.dir = PathBuf::from(value()?),
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => {
                return Err(format!(
                    "unknown argument `{other}` (use --photos N, --side PX, --recogniser sface|auraface, --dir PATH, --check, --plant-ms N, --json)"
                ));
            }
        }
    }
    if !(1..=synthetic::MAX_SCANNED_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be 1..={}", synthetic::MAX_SCANNED_PHOTOS));
    }
    if !(256..=synthetic::FILE_SIDE.1).contains(&a.side) {
        return Err(format!("--side must be 256..={}", synthetic::FILE_SIDE.1));
    }
    if !["sface", "auraface"].contains(&a.recogniser.as_str()) {
        return Err("--recogniser must be sface or auraface".into());
    }
    if !(0.0..=1000.0).contains(&a.plant_ms) {
        return Err("--plant-ms must be 0..=1000".into());
    }
    Ok(a)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

/// This process's resident memory in MiB (Linux; `None` elsewhere).
fn rss_mib() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.split_whitespace().next()?.parse().ok()?;
    Some(kb / 1024.0)
}

/// Installs `shaped` as model `manifest.id` in `models`, as an install leaves it.
fn install(models: &Path, mut manifest: ModelManifest, shaped: &Shaped) {
    let bytes = dalica_faces::synthetic::encode(&shaped.graph);
    manifest.sha256 = Some(dalica_faces::hash::sha256_hex(&bytes));
    manifest.size_bytes = Some(bytes.len() as u64);
    let dir = models.join(&manifest.id);
    std::fs::create_dir_all(&dir).expect("the model's folder");
    std::fs::write(dir.join(MODEL_FILE), &bytes).expect("the model");
    std::fs::write(dir.join(MANIFEST_FILE), serde_json::to_vec_pretty(&manifest).expect("the manifest")).expect("the manifest");
}

/// A models folder with the detector of YuNet's shape and the recogniser of `recogniser`'s.
fn models(root: &Path, recogniser: &str) -> (PathBuf, Shaped) {
    let models = root.join("models");
    let mut det = known::yunet();
    det.id = "bench-detector".into();
    install(&models, det, &dalica_faces::synthetic::yunet_shaped(SEED, DETECTOR_SIDE, &PLANTED));
    let (mut m, shaped) = match recogniser {
        "auraface" => (known::auraface(), dalica_faces::synthetic::auraface_shaped(SEED)),
        _ => (known::sface(), dalica_faces::synthetic::sface_shaped(SEED)),
    };
    m.id = format!("bench-{recogniser}");
    install(&models, m, &shaped);
    (models, shaped)
}

/// Sleeps until frame `n` after `start` is due.
fn pace(start: Instant, n: u64) {
    let due = start + Duration::from_secs_f64(n as f64 / FPS);
    if let Some(wait) = due.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
}

fn scan(a: &Args) -> Vec<Value> {
    let n = a.photos;
    let dir = a.dir.join(format!("faces-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the bench's folder");
    let spec = Spec::new(n, n.div_ceil(PER_FOLDER), synthetic::SEED);
    let written = Instant::now();
    synthetic::write_photos(&dir, &spec, a.side).expect("the photos");
    let (models, shaped) = models(&dir, &a.recogniser);
    eprintln!("faces_scan: {n} photos of {} px and the models written in {:.1} s under {}", a.side, written.elapsed().as_secs_f64(), dir.display());

    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&synthetic::library_dir(&dir), clock.clone(), &options).expect("the library opens");
    s.execute("folders.add", &json!({ "path": synthetic::photos_dir(&dir) })).expect("folders.add");
    s.wait_idle();
    assert_eq!(s.library().photos().count(), n, "every photo scanned in");

    // The models load while the user is busy, so no photo starts before the base is read.
    s.user_activity(UserActivity::Input);
    s.set_face_models_dir(Some(models));
    let loading = Instant::now();
    while !s.face_scan().has_detector() {
        assert!(loading.elapsed() < TIMEOUT, "the models never loaded: {:?}", s.face_scan().load_error());
        s.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(s.face_scan().stats().started, 0, "nothing started while the user was busy");
    let base = rss_mib();
    clock.advance(IDLE_AFTER + Duration::from_millis(1));

    let expected = (n * PLANTED.len()) as u64;
    let (mut frames, mut workers, mut peak) = (Vec::new(), 0usize, base.unwrap_or(0.0));
    let start = Instant::now();
    let mut last = start;
    let mut k = 0u64;
    let scan_s = loop {
        let now = Instant::now();
        clock.advance(now - last);
        last = now;
        let t = Instant::now();
        s.user_activity(UserActivity::Idle);
        s.poll();
        frames.push(t.elapsed());
        workers = workers.max(s.face_scan().waiting().1);
        if let Some(m) = rss_mib() {
            peak = peak.max(m);
        }
        let stats = s.face_scan().stats();
        if stats.embedded + stats.failed >= expected && s.face_scan().waiting() == (0, 0) {
            break start.elapsed().as_secs_f64();
        }
        assert!(start.elapsed() < TIMEOUT, "{} of {expected} faces after {} s", stats.embedded, TIMEOUT.as_secs());
        k += 1;
        pace(start, k);
    };
    let stats = s.face_scan().stats();
    let unnamed = s.library().unnamed_faces().len();
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!((stats.embedded, stats.failed, unnamed as u64), (expected, 0, expected), "every planted face found and embedded");

    let mut t: Vec<f64> = frames.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let per_worker = match base {
        Some(b) if workers > 0 => (peak - b).max(0.0) / workers as f64,
        _ => 0.0,
    };
    vec![
        json!({
            "name": format!("scan {n}"),
            "photos": n,
            "side_px": a.side,
            "recogniser": a.recogniser,
            "recogniser_weights": shaped.weights,
            "faces": stats.embedded,
            "scan_s": scan_s,
            "faces_per_s": stats.embedded as f64 / scan_s.max(1e-9),
            "photos_per_s": n as f64 / scan_s.max(1e-9),
            "budget_faces_per_s": FACES_PER_S_BUDGET,
            "workers": workers,
            "rss_base_mib": base.unwrap_or(0.0),
            "rss_peak_mib": peak,
            "per_worker_mib": per_worker,
            "budget_per_worker_mib": WORKER_MIB_BUDGET,
        }),
        json!({
            "name": "frame while scanning",
            "samples": t.len(),
            "p50_ms": rank(&t, 50),
            "p95_ms": rank(&t, 95),
            "max_ms": t.last().copied().unwrap_or(0.0),
            "budget_p95_ms": FRAME_BUDGET_MS.min(FRAME_CAP_MS),
            "cap_p95_ms": FRAME_CAP_MS,
        }),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("faces_scan: {e}");
            std::process::exit(2);
        }
    };
    let rows = scan(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "faces_scan",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "side_px": a.side,
        "recogniser": a.recogniser,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        for r in &rows {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
    }
    if a.check && mode == "release" && a.photos >= PHOTOS && a.recogniser == "sface" {
        let mut missed = Vec::new();
        let (scan, frame) = (&rows[0], &rows[1]);
        let faces = scan["faces"].as_f64().unwrap_or(0.0);
        let secs = scan["scan_s"].as_f64().unwrap_or(f64::INFINITY) + a.plant_ms * faces / 1000.0;
        let per_s = faces / secs.max(1e-9);
        if per_s < FACES_PER_S_BUDGET {
            missed.push(format!("{per_s:.1} faces/s, under the {FACES_PER_S_BUDGET} faces/s budget"));
        }
        let mib = scan["per_worker_mib"].as_f64().unwrap_or(f64::INFINITY);
        if mib > WORKER_MIB_BUDGET {
            missed.push(format!("{mib:.0} MiB per worker, over the {WORKER_MIB_BUDGET} MiB budget"));
        }
        let p95 = frame["p95_ms"].as_f64().unwrap_or(f64::INFINITY) + a.plant_ms;
        if p95 > FRAME_BUDGET_MS.min(FRAME_CAP_MS) {
            missed.push(format!("a frame while scanning: p95 {p95:.2} ms, over the {} ms budget", FRAME_BUDGET_MS.min(FRAME_CAP_MS)));
        }
        for m in &missed {
            eprintln!("faces_scan: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
