//! Import from a card (`development.md` §8, `roadmap.md` M11.23): listing, then importing, 1 000
//! `scenes` JPEGs from a temporary card, as the Import room does it. Run it with
//!
//! ```sh
//! cargo xtask bench import                                         # release: 1 000 JPEGs, history
//! cargo bench -p dalica-engine --bench import -- --photos 1000 --json
//! cargo bench -p dalica-engine --bench import -- --dir /fast/disk --side 1024 --runs 5
//! ```
//!
//! `--photos` synthetic `scenes` JPEGs (1 000; `--side` pixels on their long side, 2048 by default,
//! each with a 160-pixel EXIF thumbnail as cameras write) in folders of about 100 are written
//! under `--dir` and moved into a card's `DCIM` folder (not timed; by default this target
//! directory's temporary folder, on the disk the build runs on). Each of `--runs` runs (3) then
//! opens a new library whose device list is that card's mount folder (the `DALICA_DEVICE_ROOTS`
//! hook) and, with frames at 60 per second each calling [`Session::poll`] as the app's would:
//! - opens the card (`import.openSource`) and asks `import.list` (and polls) every millisecond,
//!   finer than the room's frames: `first_ms` is the time until the first screenful
//!   ([`SCREENFUL`] cells, the first candidates in capture order) all have their thumbnails, which
//!   the room draws at its next frame; `list_s` until the listing is done;
//! - imports every candidate (`import.run` into a new destination folder named by text, `after:
//!   "leave"`): `import_s` from the command to the frame where `import.status` says `done`, every
//!   copy written and verified byte for byte, with `files_per_s` and `mb_per_s` from it;
//!   `library_s` until every copy is also a library photo.
//!
//! Rows (each over the runs, p50/p95/max by nearest rank): `first thumbnails <n>` (`p50_ms`),
//! `list <n>` (`p50_ms`), `import <n>` (`p50_ms`, and from it `files_per_s` and `mb_per_s`, with
//! `bytes`) and `in library <n>` (`p50_ms`, printed, not budgeted). The card's files are read just after they were
//! written, so they are in the page cache (dropping it needs root); each run's library and
//! destination are removed after it, the card at the end. It exits non-zero when an import does not
//! copy every photo, and with `--check` also when a budget is missed (in release, over 1 000 photos
//! of 2048 pixels or more); `cargo xtask budgets` judges the report itself. Without `--bench` (under
//! `cargo test --benches`) it runs a smoke pass: one run over 60 photos of 320 pixels.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::SystemClock;
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement, release on two cores, the
/// median of three runs (2026-10-17: the first screenful 23.2 ms, the listing 211 ms, the import
/// 29.1 s: 34.4 files/s, 3.56 MB/s), times rounded up and rates down.
const FIRST_BUDGET_MS: f64 = 35.0;
const LIST_BUDGET_MS: f64 = 320.0;
const FILES_PER_S_BUDGET: f64 = 22.9;
const MB_PER_S_BUDGET: f64 = 2.3;
/// The photos the budgets hold for.
const PHOTOS: usize = 1_000;
/// The first screenful of the Import room at 1400 × 900, as the light table's (`first_thumbnails`).
const SCREENFUL: usize = 28;
/// The embedded thumbnail's long side, as cameras write it.
const EMBEDDED: u32 = 160;
/// Photos per card folder on average (`100CANON` holds up to 9 999; cameras start a new one
/// sooner on a date change).
const PER_FOLDER: usize = 100;
/// Frames per second the app runs at.
const FPS: f64 = 60.0;
/// How often the listing is asked, a second: finer than a frame, so the first screenful's time is
/// not rounded up to a frame (the room shows it at the next frame).
const ASK: f64 = 1000.0;
/// How often the device list is listed again (the app's is 2 s; the card is there from the start).
const DEVICES: Duration = Duration::from_millis(50);
/// The longest one step may run.
const TIMEOUT: Duration = Duration::from_secs(1800);

struct Args {
    photos: usize,
    side: u32,
    runs: usize,
    dir: PathBuf,
    json: bool,
    check: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { PHOTOS } else { 60 },
        side: if full { 2048 } else { synthetic::DEFAULT_FILE_SIDE },
        runs: if full { 3 } else { 1 },
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
        check: false,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--side" => a.side = value()?.parse().map_err(|e| format!("--side: {e}"))?,
            "--runs" => a.runs = value()?.parse().map_err(|e| format!("--runs: {e}"))?,
            "--dir" => a.dir = PathBuf::from(value()?),
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --side PX, --runs N, --dir PATH, --check, --json)")),
        }
    }
    if !(SCREENFUL..=dalica_scan::import::MAX_FILES.min(synthetic::MAX_SCANNED_PHOTOS)).contains(&a.photos) {
        return Err(format!("--photos must be {SCREENFUL}..={}", dalica_scan::import::MAX_FILES.min(synthetic::MAX_SCANNED_PHOTOS)));
    }
    if !(EMBEDDED..=synthetic::FILE_SIDE.1).contains(&a.side) {
        return Err(format!("--side must be {EMBEDDED}..={} (the embedded thumbnail is {EMBEDDED} px)", synthetic::FILE_SIDE.1));
    }
    if !(1..=50).contains(&a.runs) {
        return Err("--runs must be 1..=50".into());
    }
    Ok(a)
}

/// Sleeps until step `n` of a run that started at `start` is due, `per_s` steps a second.
fn pace(start: Instant, n: u64, per_s: f64) {
    let due = start + Duration::from_secs_f64(n as f64 / per_s);
    if let Some(wait) = due.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
}

/// Runs steps (each one [`Session::poll`], then `done`), `per_s` a second, until `done` answers
/// true; the time it took.
fn steps_until(s: &mut Session, what: &str, per_s: f64, mut done: impl FnMut(&mut Session) -> bool) -> Duration {
    let start = Instant::now();
    let mut frame = 0u64;
    loop {
        s.poll();
        if done(s) {
            return start.elapsed();
        }
        assert!(start.elapsed() < TIMEOUT, "{what}: no end after {} s", TIMEOUT.as_secs());
        frame += 1;
        pace(start, frame, per_s);
    }
}

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

/// One run's times and the bytes copied.
struct Run {
    first: Duration,
    list: Duration,
    import: Duration,
    library: Duration,
    bytes: u64,
}

/// One run on a new library: open the card, list it, import it.
fn one(dir: &Path, mounts: &Path, dcim: &Path, n: usize, k: usize) -> Run {
    let work = dir.join(format!("run-{k}"));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(work.join("cache")), ..ThumbOptions::default() } };
    let mut s = Session::open_with(&work.join("library"), Arc::new(SystemClock), &options).unwrap();
    s.set_import_device_roots(Some(std::env::join_paths([mounts]).unwrap()), DEVICES);
    // The device list is made off the owner thread: wait until it shows the card (not timed; the
    // room lists the card before the user can click it).
    let shown = Instant::now();
    while !run(&mut s, "import.sources", json!({}))["sources"].as_array().is_some_and(|v| v.iter().any(|d| d["path"] == json!(dcim))) {
        assert!(shown.elapsed() < TIMEOUT, "the card never showed");
        std::thread::sleep(Duration::from_millis(5));
    }

    let start = Instant::now();
    run(&mut s, "import.openSource", json!({ "device": dcim }));
    let mut first = None;
    let list = steps_until(&mut s, "the listing", ASK, |s| {
        let v = run(s, "import.list", json!({ "limit": SCREENFUL }));
        let cells = v["candidates"].as_array().cloned().unwrap_or_default();
        if first.is_none() && cells.len() >= SCREENFUL && cells.iter().all(|c| c["thumbnail"] == true) {
            first = Some(start.elapsed());
        }
        first.is_some() && v["state"] != "loading"
    });
    let listed = run(&mut s, "import.list", json!({ "limit": 0 }));
    assert_eq!((listed["state"].clone(), listed["total"].clone()), (json!("done"), json!(n)), "{listed}");

    let destination = work.join("Pictures");
    let v = run(&mut s, "import.run", json!({ "destination": destination, "folderName": "Card", "after": "leave" }));
    assert_eq!(v["total"], json!(n), "{v}");
    let import = steps_until(&mut s, "the import", FPS, |s| run(s, "import.status", json!({}))["state"] == "done");
    let library = import + steps_until(&mut s, "the copies in the library", FPS, |s| s.library().photos().count() >= n);
    let status = run(&mut s, "import.status", json!({}));
    assert_eq!((status["imported"].clone(), status["failed"].clone()), (json!(n), json!([])), "{status}");
    let bytes = status["bytes"].as_u64().unwrap_or(0);
    drop(s);
    let _ = std::fs::remove_dir_all(&work);
    Run { first: first.unwrap_or_default(), list, import, library, bytes }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

/// A row over the runs: p50, p95 and max of `times`, its budget, then `extra`.
fn row(name: String, times: &[Duration], budget: Option<f64>, extra: Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let mut r =
        json!({ "name": name, "samples": t.len(), "p50_ms": rank(&t, 50), "p95_ms": rank(&t, 95), "max_ms": t.last().copied().unwrap_or(0.0) });
    if let (Some(b), Some(o)) = (budget, r.as_object_mut()) {
        o.insert("budget_ms".into(), json!(b));
    }
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

fn bench(a: &Args) -> Vec<Value> {
    let n = a.photos;
    let dir = a.dir.join(format!("import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let spec = Spec::new(n, n.div_ceil(PER_FOLDER), synthetic::SEED);
    let written = Instant::now();
    synthetic::write_photos_with(&dir, &spec, a.side, Some(EMBEDDED)).unwrap();
    // The card: a mount folder holding a volume with a DCIM folder, the photos' folders inside.
    let mounts = dir.join("mounts");
    let dcim = mounts.join("CARD").join("DCIM");
    std::fs::create_dir_all(dcim.parent().unwrap()).unwrap();
    std::fs::rename(synthetic::photos_dir(&dir), &dcim).unwrap();
    eprintln!("import: {n} photos of {} px written in {:.1} s to the card {}", a.side, written.elapsed().as_secs_f64(), dcim.display());

    let runs: Vec<Run> = (0..a.runs).map(|k| one(&dir, &mounts, &dcim, n, k)).collect();
    let _ = std::fs::remove_dir_all(&dir);
    let bytes = runs.first().map_or(0, |r| r.bytes);
    let times = |f: fn(&Run) -> Duration| runs.iter().map(f).collect::<Vec<_>>();
    let import = row(
        format!("import {n}"),
        &times(|r| r.import),
        None,
        json!({ "photos": n, "bytes": bytes, "budget_files_per_s": FILES_PER_S_BUDGET, "budget_mb_per_s": MB_PER_S_BUDGET }),
    );
    let p50_s = import["p50_ms"].as_f64().unwrap_or(0.0) / 1000.0;
    let mut import = import;
    if let Some(o) = import.as_object_mut() {
        o.insert("files_per_s".into(), json!(n as f64 / p50_s.max(1e-9)));
        o.insert("mb_per_s".into(), json!(bytes as f64 / 1e6 / p50_s.max(1e-9)));
    }
    vec![
        row(format!("first thumbnails {n}"), &times(|r| r.first), Some(FIRST_BUDGET_MS), json!({ "cells": SCREENFUL })),
        row(format!("list {n}"), &times(|r| r.list), Some(LIST_BUDGET_MS), json!({ "photos": n })),
        import,
        row(format!("in library {n}"), &times(|r| r.library), None, json!({ "photos": n })),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("import: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "import",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "side_px": a.side,
        "runs": a.runs,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
    }
    let mut missed = Vec::new();
    if a.check && mode == "release" && a.photos >= PHOTOS && a.side >= 2048 {
        for r in &rows {
            if let (Some(p50), Some(budget)) = (r["p50_ms"].as_f64(), r["budget_ms"].as_f64())
                && p50 > budget
            {
                missed.push(format!("{}: median {p50:.1} ms, over the {budget} ms budget", r["name"].as_str().unwrap_or("?")));
            }
            for (key, budget, unit) in [("files_per_s", FILES_PER_S_BUDGET, "files/s"), ("mb_per_s", MB_PER_S_BUDGET, "MB/s")] {
                if let Some(rate) = r[key].as_f64()
                    && rate < budget
                {
                    missed.push(format!("{}: {rate:.2} {unit}, under the {budget} {unit} budget", r["name"].as_str().unwrap_or("?")));
                }
            }
        }
    }
    for m in &missed {
        eprintln!("import: over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
}
