//! Batch Edit at scale (`development.md` §8, `roadmap.md` M11.23): Auto Fix on 1 000 photos as one
//! step (`edit.apply`, Picture ▸ Batch Edit), then its undo (`edit.undo`). Run it with
//!
//! ```sh
//! cargo xtask bench batch_edit                                     # release: 1 000 photos, history
//! cargo bench -p dalica-engine --bench batch_edit -- --photos 1000 --samples 15 --json
//! ```
//!
//! `--photos` synthetic `scenes` JPEGs (1 000, 320 pixels on their long side, in folders of about
//! 100) are written under this target directory's temporary folder and scanned into a new library
//! with its thumbnails (not timed). Then `--samples` times (15): `edit.apply {ids: every photo, op:
//! "autofix"}`, timed from the command to its answer (each photo's recipe gets the operation, its
//! redo list cleared, the sidecar writes queued for the I/O worker, one library undo step "Auto
//! Fix", the edited thumbnails' jobs queued), then `edit.undo`, timed the same way (every recipe and
//! redo list given back). Between the two the session goes idle (the thumbnails remade, the
//! sidecars written), so each command runs on a quiet owner thread as a click does.
//!
//! Rows (p50/p95/max by nearest rank, with `budget_ms`): `auto fix <n>` and `undo auto fix <n>`.
//! It checks that the apply was one undo step and that the undo gave every recipe back, and exits
//! non-zero when not; with `--check` also when a median misses its budget (in release, over 1 000
//! photos or more). `cargo xtask budgets` judges the report itself. Without `--bench` (under
//! `cargo test --benches`) it runs a smoke pass: 100 photos, two samples.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_library::Recipe;
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement, release on two cores, the
/// median of three runs (2026-10-17: Auto Fix 4.65 ms, its undo 3.31 ms), rounded up to 0.1 ms.
const APPLY_BUDGET_MS: f64 = 7.0;
const UNDO_BUDGET_MS: f64 = 5.0;
/// The photos the budgets hold for.
const PHOTOS: usize = 1_000;
/// Photos per folder on average.
const PER_FOLDER: usize = 100;

struct Args {
    photos: usize,
    samples: usize,
    json: bool,
    check: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args { photos: if full { PHOTOS } else { 100 }, samples: if full { 15 } else { 2 }, json: false, check: false };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --samples N, --check, --json)")),
        }
    }
    if !(1..=synthetic::MAX_SCANNED_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be 1..={}", synthetic::MAX_SCANNED_PHOTOS));
    }
    if !(1..=1000).contains(&a.samples) {
        return Err("--samples must be 1..=1000".into());
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

fn row(name: String, times: &[Duration], budget: f64, photos: usize) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    json!({
        "name": name,
        "photos": photos,
        "samples": t.len(),
        "p50_ms": rank(&t, 50),
        "p95_ms": rank(&t, 95),
        "max_ms": t.last().copied().unwrap_or(0.0),
        "budget_ms": budget,
    })
}

/// The command `id` with `params`, timed from the call to its answer.
fn timed(s: &mut Session, id: &str, params: &Value) -> (Duration, Value) {
    let t = Instant::now();
    let v = s.execute(id, params).unwrap_or_else(|e| panic!("{id}: {e}"));
    (t.elapsed(), v)
}

fn bench(a: &Args) -> Result<Vec<Value>, String> {
    let n = a.photos;
    let dir = tempfile::Builder::new().prefix("batch-edit-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).map_err(|e| e.to_string())?;
    let spec = Spec::new(n, n.div_ceil(PER_FOLDER), synthetic::SEED);
    let written = Instant::now();
    synthetic::write_photos(dir.path(), &spec, synthetic::DEFAULT_FILE_SIDE).map_err(|e| e.to_string())?;
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(dir.path().join("cache")), ..ThumbOptions::default() } };
    let mut s = Session::open_with(&synthetic::library_dir(dir.path()), Arc::new(SystemClock), &options).map_err(|e| e.to_string())?;
    s.execute("folders.add", &json!({ "path": synthetic::photos_dir(dir.path()) })).map_err(|e| e.to_string())?;
    s.wait_idle();
    let ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    if ids.len() != n {
        return Err(format!("{} of {n} photos scanned", ids.len()));
    }
    s.flush().map_err(|e| e.to_string())?;
    eprintln!("batch_edit: {n} photos written and scanned in {:.1} s", written.elapsed().as_secs_f64());

    let before: Vec<Recipe> = ids.iter().map(|id| s.library().info(*id).recipe()).collect();
    let apply = json!({ "ids": ids, "op": "autofix" });
    let (mut applied, mut undone) = (Vec::new(), Vec::new());
    for _ in 0..a.samples {
        let steps = s.undo_stack().len();
        let (t, v) = timed(&mut s, "edit.apply", &apply);
        applied.push(t);
        if v["ids"].as_array().map(Vec::len) != Some(n) || s.undo_stack().len() != steps + 1 || s.undo_stack().undo_label() != Some("Auto Fix") {
            return Err(format!("Auto Fix on {n} photos was not one step \"Auto Fix\": {}", v["ids"].as_array().map_or(0, Vec::len)));
        }
        s.wait_idle();
        s.flush().map_err(|e| e.to_string())?;
        let (t, _) = timed(&mut s, "edit.undo", &json!({}));
        undone.push(t);
        if ids.iter().zip(&before).any(|(id, r)| s.library().info(*id).recipe() != *r) {
            return Err("the undo did not give every recipe back".into());
        }
        s.wait_idle();
        s.flush().map_err(|e| e.to_string())?;
    }
    Ok(vec![row(format!("auto fix {n}"), &applied, APPLY_BUDGET_MS, n), row(format!("undo auto fix {n}"), &undone, UNDO_BUDGET_MS, n)])
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("batch_edit: {e}");
            std::process::exit(2);
        }
    };
    let rows = match bench(&a) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("batch_edit: {e}");
            std::process::exit(1);
        }
    };
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "batch_edit",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
    }
    let mut missed = Vec::new();
    if a.check && mode == "release" && a.photos >= PHOTOS {
        for r in &rows {
            if let (Some(p50), Some(budget)) = (r["p50_ms"].as_f64(), r["budget_ms"].as_f64())
                && p50 > budget
            {
                missed.push(format!("{}: median {p50:.2} ms, over the {budget} ms budget", r["name"].as_str().unwrap_or("?")));
            }
        }
    }
    for m in &missed {
        eprintln!("batch_edit: over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
}
