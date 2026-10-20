//! Import from a camera over PTP (`development.md` §8, `organizing.md` §3, `roadmap.md` M15.34):
//! 300 pictures listed, then copied and verified, from `dalica-scan`'s in-memory responder (the
//! camera the tests use: PTP's commands, data phases and responses over an in-memory transport), as
//! the Import room does it. Run it with
//!
//! ```sh
//! cargo xtask bench camera_import                                      # release: 300 pictures, history
//! cargo bench -p dalica-engine --bench camera_import -- --objects 300 --json
//! ```
//!
//! **The camera.** `--objects` (300) `scenes` JPEGs of `--side` (2048) pixels on their long side,
//! each with a 160-pixel EXIF thumbnail as cameras write (the photos of the `import` bench, written
//! once under this target directory's temporary folder and read back, not timed), in the
//! responder's `DCIM/100CANON` with a minute between their dates, and a text file, which is no
//! picture. Each of `--runs` (3) runs opens a new library on a new thumbnail cache, makes the
//! responder its one camera (listed as connected) and, with frames at 60 a second each calling
//! [`Session::poll`] as the app's would:
//! - opens the camera's storage (`camera.open`) and asks `import.list` until the listing is done:
//!   `list_ms`, every object's info read and every picture's thumbnail asked of the camera;
//! - imports every picture (`import.run` into a new destination folder by date taken, `after:
//!   "leave"`): `import_s` from the command to the frame where `import.status` says `done`, every
//!   picture read from the camera, written through `fs` and read back.
//!
//! Rows (over the runs, p50/p95/max by nearest rank): `camera list <n>` (printed, not budgeted) and
//! `camera import <n>` (`p50_ms`, and from it `files_per_s` and `mb_per_s`, with `bytes`). The
//! responder answers from memory, so the copy is bound by Dalica's own work and the destination
//! disk, not by a cable (a real camera's USB 2 link gives about 30 MB/s at best). Each run's library
//! and destination are removed after it. It exits non-zero when an import does not copy every
//! picture, and with `--check` also when the rate is under its budget (in release, 300 pictures of
//! 2048 pixels or more); `cargo xtask budgets` judges the report itself. Without `--bench` (under
//! `cargo test --benches`) it runs a smoke pass: one run of 20 pictures of 320 pixels.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_core::SystemClock;
use dalica_engine::camera::Opener;
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_scan::devices::{Camera, CameraAccess, CameraStorage};
use dalica_scan::ptp::responder::MemoryCamera;
use dalica_scan::ptp::{Limits, PtpError, Transport, TransportError};
use serde_json::{Value, json};

/// The budget (`development.md` §8): files a second at least the first measurement ÷ 1.5 (the
/// median of three runs, release on two hardware threads, `taskset -c 0,1`, 2026-10-20: 300
/// pictures in 14.3 s, 20.95 files/s, 2.14 MB/s), rounded down to 0.1.
const FILES_PER_S_BUDGET: f64 = 13.9;
/// The pictures the budget holds for, and their long side.
const OBJECTS: usize = 300;
const SIDE: u32 = 2048;
/// The embedded thumbnail's long side, as cameras write it.
const EMBEDDED: u32 = 160;
/// Frames per second the app runs at.
const FPS: f64 = 60.0;
/// The camera's id in the list, and how often the list is listed again.
const ID: &str = "usb-1-4";
const REFRESH: Duration = Duration::from_millis(50);
/// The longest one step may run.
const TIMEOUT: Duration = Duration::from_secs(1800);

struct Args {
    objects: usize,
    side: u32,
    runs: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        objects: if full { OBJECTS } else { 20 },
        side: if full { SIDE } else { 320 },
        runs: if full { 3 } else { 1 },
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--objects" => a.objects = value()?.parse().map_err(|e| format!("--objects: {e}"))?,
            "--side" => a.side = value()?.parse().map_err(|e| format!("--side: {e}"))?,
            "--runs" => a.runs = value()?.parse().map_err(|e| format!("--runs: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --objects N, --side N, --runs N, --check, --plant-ms N, --json)")),
        }
    }
    if !(1..=5000).contains(&a.objects) || !(EMBEDDED..=8192).contains(&a.side) || !(1..=20).contains(&a.runs) {
        return Err(format!("--objects must be 1..=5000, --side {EMBEDDED}..=8192 and --runs 1..=20"));
    }
    if !(0.0..=1000.0).contains(&a.plant_ms) {
        return Err("--plant-ms must be 0..=1000".into());
    }
    Ok(a)
}

/// Runs frames (each one [`Session::poll`], then `done`) at 60 a second until `done` answers true;
/// the time it took.
fn frames_until(s: &mut Session, what: &str, mut done: impl FnMut(&mut Session) -> bool) -> Duration {
    let start = Instant::now();
    let tick = Duration::from_secs_f64(1.0 / FPS);
    let mut next = start;
    loop {
        s.poll();
        if done(s) {
            return start.elapsed();
        }
        assert!(start.elapsed() < TIMEOUT, "{what}: no end after {} s", TIMEOUT.as_secs());
        next += tick;
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

/// The camera holding `pictures`, a minute apart from 08:00 on 2026-06-04, and a text file; its
/// storage's id.
fn camera(pictures: &[(String, Vec<u8>)]) -> (MemoryCamera, u32) {
    let mut cam = MemoryCamera::new("Canon", "Canon EOS R6");
    let storage = cam.add_storage("SD1", 64 << 30);
    let dcim = cam.add_folder(storage, 0, "DCIM");
    let folder = cam.add_folder(storage, dcim, "100CANON");
    for (i, (name, bytes)) in pictures.iter().enumerate() {
        cam.add_file(storage, folder, name, &format!("20260604T{:02}{:02}00", 8 + i / 60 % 12, i % 60), bytes.clone());
    }
    cam.add_file(storage, folder, "NOTES.TXT", "20260604T200000", b"not a picture".to_vec());
    (cam, storage)
}

/// Makes `cam` the session's one camera, listed as connected, its session opened on the first call
/// from a worker.
fn attach(s: &mut Session, cam: MemoryCamera, storage: u32) {
    let (transport, _handle) = cam.connect();
    let slot = Mutex::new(Some(transport));
    let opener: Opener = Arc::new(move |_id: &str| {
        let t = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take().ok_or(PtpError::Transport(TransportError::Disconnected))?;
        dalica_scan::ptp::Session::open(Box::new(t) as Box<dyn Transport>, Limits::default())
    });
    let listed = Camera {
        id: ID.into(),
        vendor: "Canon".into(),
        model: "Canon EOS R6".into(),
        serial: "0123456789".into(),
        storages: vec![CameraStorage { id: storage, name: "SD1".into(), capacity: 64 << 30, free: 32 << 30 }],
        access: CameraAccess::Open,
    };
    s.set_camera_backend(Some((Box::new(move || vec![listed.clone()]), opener)), REFRESH);
}

/// One run's times and the bytes copied.
struct Run {
    list: Duration,
    import: Duration,
    bytes: u64,
}

/// One run on a new library: open the camera, list it, import it.
fn one(dir: &Path, pictures: &[(String, Vec<u8>)], k: usize) -> Run {
    let n = pictures.len();
    let work = dir.join(format!("run-{k}"));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(work.join("cache")), ..ThumbOptions::default() } };
    let mut s = Session::open_with(&work.join("library"), Arc::new(SystemClock), &options).expect("the library opens");
    let (cam, storage) = camera(pictures);
    attach(&mut s, cam, storage);
    // The camera list is made off the owner thread: wait until it shows the camera (not timed; the
    // room lists it before the user can click it).
    let shown = Instant::now();
    while run(&mut s, "camera.list", json!({}))["cameras"].as_array().is_none_or(|c| c.is_empty()) {
        assert!(shown.elapsed() < TIMEOUT, "the camera never showed");
        std::thread::sleep(Duration::from_millis(5));
    }
    let start = Instant::now();
    while s.execute("camera.open", &json!({ "id": ID, "storage": storage })).is_err() {
        assert!(start.elapsed() < TIMEOUT, "the camera never opened");
        std::thread::sleep(Duration::from_millis(1));
    }
    let list = start.elapsed() + frames_until(&mut s, "the listing", |s| run(s, "import.list", json!({ "limit": 0 }))["state"] != "loading");
    let listed = run(&mut s, "import.list", json!({ "limit": 0 }));
    assert_eq!((listed["state"].clone(), listed["total"].clone()), (json!("done"), json!(n)), "{listed}");

    let destination = work.join("Pictures");
    let v = run(&mut s, "import.run", json!({ "destination": destination, "folderName": { "dateTaken": true }, "after": "leave" }));
    assert_eq!(v["total"], json!(n), "{v}");
    let import = frames_until(&mut s, "the import", |s| run(s, "import.status", json!({}))["state"] == "done");
    let status = run(&mut s, "import.status", json!({}));
    assert_eq!((status["imported"].clone(), status["failed"].clone()), (json!(n), json!([])), "{status}");
    let bytes = status["bytes"].as_u64().unwrap_or(0);
    drop(s);
    let _ = std::fs::remove_dir_all(&work);
    Run { list, import, bytes }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn row(name: String, times: &[Duration], extra: Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let mut r =
        json!({ "name": name, "samples": t.len(), "p50_ms": rank(&t, 50), "p95_ms": rank(&t, 95), "max_ms": t.last().copied().unwrap_or(0.0) });
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

fn bench(a: &Args) -> Vec<Value> {
    let n = a.objects;
    let dir = tempfile::Builder::new().prefix("camera-import-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let spec = Spec::new(n, 1, synthetic::SEED);
    let written = Instant::now();
    let folders = synthetic::write_photos_with(dir.path(), &spec, a.side, Some(EMBEDDED)).expect("the pictures are written");
    let mut files: Vec<_> = folders
        .iter()
        .flat_map(|f| std::fs::read_dir(f).expect("a folder of pictures").filter_map(Result::ok).map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    assert_eq!(files.len(), n, "every picture written");
    let pictures: Vec<(String, Vec<u8>)> =
        files.iter().enumerate().map(|(i, p)| (format!("IMG_{:04}.JPG", i + 1), std::fs::read(p).expect("a picture read back"))).collect();
    let _ = std::fs::remove_dir_all(synthetic::photos_dir(dir.path()));
    eprintln!("camera_import: {n} pictures of {} px made in {:.1} s", a.side, written.elapsed().as_secs_f64());

    let runs: Vec<Run> = (0..a.runs).map(|k| one(dir.path(), &pictures, k)).collect();
    let bytes = runs.first().map_or(0, |r| r.bytes);
    let import = row(format!("camera import {n}"), &runs.iter().map(|r| r.import).collect::<Vec<_>>(), json!({ "objects": n, "bytes": bytes }));
    let p50_s = import["p50_ms"].as_f64().unwrap_or(0.0) / 1000.0;
    let mut import = import;
    if let Some(o) = import.as_object_mut() {
        o.insert("files_per_s".into(), json!(n as f64 / p50_s.max(1e-9)));
        o.insert("mb_per_s".into(), json!(bytes as f64 / 1e6 / p50_s.max(1e-9)));
        o.insert("budget_files_per_s".into(), json!(FILES_PER_S_BUDGET));
    }
    vec![row(format!("camera list {n}"), &runs.iter().map(|r| r.list).collect::<Vec<_>>(), json!({ "objects": n })), import]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("camera_import: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "camera_import",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "objects": a.objects,
        "side_px": a.side,
        "runs": a.runs,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
    }
    if a.check && mode == "release" && a.objects >= OBJECTS && a.side >= SIDE {
        let import = &rows[1];
        let secs = import["p50_ms"].as_f64().unwrap_or(f64::INFINITY) / 1000.0 + a.plant_ms * a.objects as f64 / 1000.0;
        let rate = a.objects as f64 / secs.max(1e-9);
        if rate < FILES_PER_S_BUDGET {
            eprintln!("camera_import: over budget: {rate:.1} files/s, under the {FILES_PER_S_BUDGET} files/s budget");
            std::process::exit(1);
        }
    }
}
