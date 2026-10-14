//! Save and export (`development.md` §8, `roadmap.md` M8.24). Run it with
//!
//! ```sh
//! cargo xtask bench save_export                                   # release: 24 MP saves, the 200-photo export
//! cargo bench -p dalica-engine --bench save_export -- --json
//! taskset -c 0,1 target/release/deps/save_export-* --bench        # as on the budgets job's runner
//! ```
//!
//! Rows (times are p50 / p95 / max by nearest rank; the budgets, 1.5 × the first measurement, are
//! judged on the median by `cargo xtask budgets`, which runs this bench on the budgets job):
//!
//! - `save`: Save of a `--width × --height` JPEG (6000 × 4000 by default) with a Tuning edit:
//!   `create::save::save`, the path `photo.save` runs on its worker: the photo read, its edits
//!   rendered at full size, encoded at the original's quality with a fresh EXIF thumbnail, the
//!   original moved into `.dalica-originals/`, the new file written and verified, the record marked
//!   done. Each sample saves a fresh copy (written first, not timed). Budget [`SAVE_BUDGET_MS`].
//! - `save rotate-only`: the same JPEG saved with only a quarter turn: the lossless rotation in
//!   the DCT domain (M8.14; the 6000 × 4000 JPEG is whole MCUs), journaled and verified like a
//!   rendered Save. Budget [`ROTATE_BUDGET_MS`].
//! - `export 200 mixed`: the mixed tray of `tests/export_mixed.rs` (200 photos of every format,
//!   125 of them edited, 56 with GPS; `--scale` 5: 1500 to 2400 pixels on the long side) in a
//!   library, exported by `export.run` at 1600 pixels, Normal, with a watermark and the location
//!   removed: from the command to the end of its job (two photos at a time). `per_photo_ms` is
//!   the median divided by 200. Budget [`EXPORT_BUDGET_MS`].
//!
//! Files go under `--dir` (default: this target directory's temporary folder) and are removed
//! afterwards. Without `--bench` (under `cargo test --benches`) it runs a smoke pass on small photos
//! (`--scale` 1, 600 × 400 saves, one sample each).

#[path = "../tests/export_mixed.rs"]
#[allow(dead_code, unused_imports)]
mod mixed;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{Clock, PhotoId, SystemClock};
use dalica_create::save::{self, Saving};
use dalica_edit::Recipe;
use dalica_engine::{CacheDir, JobState, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_meta::Changes;
use mixed::tray;
use serde_json::{Value, json};

/// The budgets (`development.md` §8), judged on the median: 1.5 × the first measurement, taken in
/// release on two cores (`taskset -c 0,1`, as the budgets job's runner) on 2026-10-14: Save 701 ms,
/// the export 16.0 s, and the lossless rotation of a rotate-only Save 303 ms (M9.30; 815 ms before,
/// set from the rendered Save before the lossless rotation landed). `xtask/src/accept.rs` holds
/// the same numbers.
const SAVE_BUDGET_MS: f64 = 1050.0;
const ROTATE_BUDGET_MS: f64 = 455.0;
const EXPORT_BUDGET_MS: f64 = 24000.0;
/// The Tuning edit of the rendered Save.
const TUNING: &str = "tuning(0.300,0.100,0.050,0.200,none)";
const WAIT: Duration = Duration::from_secs(1800);

struct Args {
    width: u32,
    height: u32,
    scale: u32,
    saves: usize,
    exports: usize,
    dir: PathBuf,
    json: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        width: if full { 6000 } else { 600 },
        height: if full { 4000 } else { 400 },
        scale: if full { 5 } else { 1 },
        saves: if full { 7 } else { 1 },
        exports: if full { 3 } else { 1 },
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        let number = |v: String| v.parse::<u32>().map_err(|e| format!("{arg}: {e}"));
        match arg.as_str() {
            "--width" => a.width = number(value()?)?,
            "--height" => a.height = number(value()?)?,
            "--scale" => a.scale = number(value()?)?,
            "--saves" => a.saves = number(value()?)? as usize,
            "--exports" => a.exports = number(value()?)? as usize,
            "--dir" => a.dir = PathBuf::from(value()?),
            "--json" => a.json = true,
            _ => {}
        }
    }
    if a.width == 0 || a.height == 0 || a.width > 20_000 || a.height > 20_000 {
        return Err("--width and --height take 1 … 20000".into());
    }
    if !(1..=10).contains(&a.scale) || !(1..=100).contains(&a.saves) || !(1..=20).contains(&a.exports) {
        return Err("--scale takes 1 … 10, --saves 1 … 100, --exports 1 … 20".into());
    }
    Ok(a)
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn row(name: &str, mut times: Vec<f64>, budget: f64) -> Value {
    times.sort_by(f64::total_cmp);
    json!({
        "name": name,
        "p50_ms": rank(&times, 50),
        "p95_ms": rank(&times, 95),
        "max_ms": times.last().copied().unwrap_or(0.0),
        "samples": times.len(),
        "budget_ms": budget,
    })
}

/// Saves a fresh copy of `jpeg` with `ops` `samples` times; the time of each Save.
fn saves(root: &Path, jpeg: &[u8], ops: &[&str], samples: usize) -> Vec<f64> {
    let recipe = Recipe::parse(ops).unwrap();
    let mut times = Vec::new();
    for i in 0..samples {
        let folder = root.join(format!("save-{}-{i}", ops.len()));
        dalica_fs::create_dir_all(&folder).unwrap();
        dalica_fs::replace(&folder.join("IMG_0001.jpg"), jpeg, Expect::Absent).unwrap();
        let options = save::Options::new(SystemClock.now());
        let t = Instant::now();
        let saved = save::save(&folder, "IMG_0001.jpg", &recipe, &Changes::default(), &options).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        assert!(matches!(saved, Saving::InPlace(_)), "a JPEG saves in place");
        let _ = std::fs::remove_dir_all(&folder);
    }
    times
}

/// The tray in a library with its edits, exported `samples` times; the time of each export job.
fn exports(root: &Path, scale: u32, samples: usize) -> Vec<f64> {
    let mixed = root.join("Photos").join("Mixed");
    dalica_fs::create_dir_all(&mixed).unwrap();
    let written = Instant::now();
    let photos = tray::build(&mixed, scale);
    eprintln!("the mixed tray of {} photos written in {:.1} s", photos.len(), written.elapsed().as_secs_f64());
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&root.join("library"), Arc::new(SystemClock), &options).unwrap();
    s.execute("folders.add", &json!({ "path": root.join("Photos") })).unwrap();
    s.wait_idle();
    let ids: Vec<PhotoId> = photos.iter().map(|p| s.library().photos().find(|q| q.name == p.name).expect("in the library").id).collect();
    for (p, id) in photos.iter().zip(&ids) {
        for op in p.edits {
            s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
        }
    }
    s.wait_idle();
    let mut times = Vec::new();
    for _ in 0..samples {
        let params = json!({
            "ids": ids, "dir": root.join("Out"), "folderName": "Mixed", "size": tray::LONG_SIDE, "quality": "normal",
            "watermark": tray::WATERMARK, "removeLocation": true,
        });
        let t = Instant::now();
        let reply = s.execute("export.run", &params).unwrap();
        let done = s.wait_job(reply["job"].as_u64().unwrap(), WAIT).unwrap();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        let result = done.result.clone().unwrap_or_default();
        assert_eq!(done.state, JobState::Done, "{done:?}");
        assert_eq!(result["written"].as_array().map(Vec::len), Some(photos.len()), "every photo written: {result}");
        s.wait_idle();
    }
    times
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("save_export: {e}");
            std::process::exit(2);
        }
    };
    let root = a.dir.join(format!("save-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    dalica_fs::create_dir_all(&root).unwrap();
    let written = Instant::now();
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(24, a.width, a.height), 90, None).unwrap();
    eprintln!("a {} × {} JPEG written in {:.1} s", a.width, a.height, written.elapsed().as_secs_f64());

    let rows = vec![
        row("save", saves(&root, &jpeg, &[TUNING], a.saves), SAVE_BUDGET_MS),
        row("save rotate-only", saves(&root, &jpeg, &["rotate(90)"], a.saves), ROTATE_BUDGET_MS),
        {
            let mut r = row("export 200 mixed", exports(&root, a.scale, a.exports), EXPORT_BUDGET_MS);
            let per_photo = r["p50_ms"].as_f64().unwrap_or(0.0) / tray::PHOTOS as f64;
            r["per_photo_ms"] = json!(per_photo);
            r
        },
    ];
    let _ = std::fs::remove_dir_all(&root);

    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if a.json {
        println!(
            "{}",
            json!({ "bench": "save_export", "mode": mode, "threads": threads, "width": a.width, "height": a.height, "scale": a.scale, "rows": rows })
        );
    } else {
        for r in &rows {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
    }
}
