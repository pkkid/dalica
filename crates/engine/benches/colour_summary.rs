//! Colour summaries at library scale (`organizing.md` §8.1, `development.md` §8, `roadmap.md`
//! M16.11). Run it with
//!
//! ```sh
//! cargo xtask bench colour_summary                                  # release: 100 k photos, history
//! cargo bench -p dalica-engine --bench colour_summary -- --json
//! ```
//!
//! **The library.** The index-only synthetic library of `--photos` (100 000) photos in folders of
//! 100 (`dalica_engine::synthetic`): every photo has its sketch in the index and none has a colour
//! summary, as a library indexed before M16. There are no photo files.
//!
//! **At open.** Each of `--runs` (5) runs writes the library afresh (not timed) and opens it with
//! thumbnails on (a cache in the run's folder, the default workers), as the app opens it. Opening
//! finds the photos with a sketch and no summary and queues their summaries on the thumbnail pool,
//! `thumbs::COLOUR_BATCH` (1 024) per background job, beside the scan job of each photo (whose
//! file is missing: it fails at once, as a moved drive's would). The open must not wait on them
//! (`docs/development.md` §8, scans never block): the bench checks that the open returns with
//! fewer summaries than photos. Then `Session::poll` runs every millisecond until every photo has
//! its summary.
//!
//! **Per sketch.** `dalica_thumbs::sketch::colours` of every stored sketch of the library, one
//! thread, `--rounds` (7) times: what each sketch made by a scan (on its worker) or read back at
//! open costs.
//!
//! Rows (p50/p95/max by nearest rank; `cargo xtask budgets` judges the medians):
//!
//! - `open <n>`: `Session::open_with` with the summaries queued (printed with `colours_at_open`,
//!   the summaries in the library when it returned, and `colour_jobs`), not budgeted: the cold
//!   start's budget covers the open (`cold_start`).
//! - `summaries <n>`: from the open's start to the last summary applied. Budget
//!   [`SUMMARIES_BUDGET_MS`].
//! - `summary per sketch`: microseconds per sketch (`p50_us`), a round's time over its sketches.
//!   Budget [`PER_SKETCH_BUDGET_US`].
//!
//! With `--check` the bench exits non-zero when a budget is missed (in release, over 100 000
//! photos); `--plant-ms N` adds N ms to each run's summaries and to each sketch's summary. Files go
//! under this target directory's temporary folder and are removed afterwards. Without `--bench`
//! (under `cargo test --benches`) it runs a smoke pass: 2 000 photos, one run, one round.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_engine::synthetic::{self, Spec};
use dalica_engine::thumbs::COLOUR_BATCH;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use serde_json::{Value, json};

/// The budgets (`development.md` §8), judged on the median: 1.5 × the first measurement (the median
/// of three runs, release on two hardware threads, `taskset -c 0,1`, 2026-11-06: every summary of
/// 100 000 photos applied 962.6 ms after the open began, one sketch's summary 3.47 µs), rounded up
/// to 5 ms and to 0.5 µs. `xtask/src/bench.rs` holds the same numbers.
const SUMMARIES_BUDGET_MS: f64 = 1445.0;
const PER_SKETCH_BUDGET_US: f64 = 5.5;
/// The library the budgets hold for.
const PHOTOS: usize = 100_000;
/// Photos per folder: the synthetic 100 k preset's 100.
const PER_FOLDER: usize = 100;
/// The longest a run may take to apply every summary.
const WAIT: Duration = Duration::from_secs(600);

struct Args {
    photos: usize,
    runs: usize,
    rounds: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { PHOTOS } else { 2_000 },
        runs: if full { 5 } else { 1 },
        rounds: if full { 7 } else { 1 },
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--runs" => a.runs = value()?.parse().map_err(|e| format!("--runs: {e}"))?,
            "--rounds" => a.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --runs N, --rounds N, --check, --plant-ms N, --json)")),
        }
    }
    if !(PER_FOLDER * 10..=synthetic::MAX_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be {}..={}", PER_FOLDER * 10, synthetic::MAX_PHOTOS));
    }
    if !(1..=100).contains(&a.runs) || !(1..=100).contains(&a.rounds) || !(0.0..=1000.0).contains(&a.plant_ms) {
        return Err("--runs and --rounds must be 1..=100, --plant-ms 0..=1000".into());
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

/// A row of `times` in `unit` (`ms` or `us`), with `extra`'s fields.
fn row(name: String, mut times: Vec<f64>, unit: &str, extra: Value) -> Value {
    times.sort_by(f64::total_cmp);
    let mut r = json!({ "name": name, "samples": times.len() });
    if let Some(o) = r.as_object_mut() {
        o.insert(format!("p50_{unit}"), json!(rank(&times, 50)));
        o.insert(format!("p95_{unit}"), json!(rank(&times, 95)));
        o.insert(format!("max_{unit}"), json!(times.last().copied().unwrap_or(0.0)));
        if let Some(e) = extra.as_object() {
            o.extend(e.clone());
        }
    }
    r
}

/// `100000` → `100k`.
fn label(n: usize) -> String {
    if n >= 1000 && n.is_multiple_of(1000) { format!("{}k", n / 1000) } else { n.to_string() }
}

/// What one run measured: the open, the summaries in the library when it returned, and the time
/// to the last summary applied.
struct Run {
    open: f64,
    at_open: usize,
    all: f64,
}

/// Writes the library of `spec` in a fresh folder and opens it with thumbnails on; polls until
/// every photo has its summary.
fn run(spec: &Spec) -> Run {
    let dir = tempfile::Builder::new().prefix("colour-summary-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    synthetic::write_index(dir.path(), spec).expect("the synthetic library is written");
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::At(dir.path().join("cache")), ..ThumbOptions::default() } };
    let started = Instant::now();
    let mut s = Session::open_with(&synthetic::library_dir(dir.path()), Arc::new(dalica_core::SystemClock), &options).expect("the library opens");
    let open = ms(started.elapsed());
    let photos = s.library().photos().count();
    let at_open = s.library().colours_all().count();
    while s.library().colours_all().count() < photos {
        assert!(started.elapsed() < WAIT, "every summary applied within {WAIT:?}");
        s.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    let all = ms(started.elapsed());
    assert_eq!(s.thumbnail_stats().decodes, 0, "no photo was decoded");
    drop(s);
    Run { open, at_open, all }
}

/// Microseconds per sketch of `colours` over `sketches`, one per round.
fn per_sketch(sketches: &[Vec<u8>], rounds: usize) -> Vec<f64> {
    let mut times = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let started = Instant::now();
        let mut coloured = 0usize;
        for bytes in sketches {
            coloured += usize::from(!std::hint::black_box(dalica_thumbs::sketch::colours(bytes)).is_empty());
        }
        times.push(started.elapsed().as_secs_f64() * 1e6 / sketches.len().max(1) as f64);
        assert_eq!(coloured, sketches.len(), "every sketch has a summary");
    }
    times
}

fn bench(a: &Args) -> Vec<Value> {
    let spec = Spec::new(a.photos, a.photos / PER_FOLDER, synthetic::SEED);
    let runs: Vec<Run> = (0..a.runs).map(|_| run(&spec)).collect();
    let jobs = a.photos.div_ceil(COLOUR_BATCH);
    let at_open = runs.iter().map(|r| r.at_open).max().unwrap_or(0);
    assert!(at_open < a.photos, "the open did not wait on the summaries ({at_open} of {} at open)", a.photos);

    let dir = tempfile::Builder::new().prefix("colour-sketches-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let lib = synthetic::library(dir.path(), &spec).expect("the synthetic library");
    let sketches: Vec<Vec<u8>> = lib.photos().filter_map(|p| lib.sketch(p.id)).map(|s| s.as_bytes().to_vec()).collect();
    assert_eq!(sketches.len(), a.photos, "every photo has a sketch");

    let n = label(a.photos);
    vec![
        row(
            format!("open {n}"),
            runs.iter().map(|r| r.open).collect(),
            "ms",
            json!({ "photos": a.photos, "colours_at_open": at_open, "colour_jobs": jobs }),
        ),
        row(
            format!("summaries {n}"),
            runs.iter().map(|r| r.all).collect(),
            "ms",
            json!({ "photos": a.photos, "colour_jobs": jobs, "budget_ms": SUMMARIES_BUDGET_MS }),
        ),
        row(
            "summary per sketch".to_string(),
            per_sketch(&sketches, a.rounds),
            "us",
            json!({ "sketches": sketches.len(), "budget_us": PER_SKETCH_BUDGET_US }),
        ),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("colour_summary: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "colour_summary",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        for r in &rows {
            println!("{}", serde_json::to_string(r).unwrap_or_default());
        }
    }
    if a.check && mode == "release" && a.photos >= PHOTOS {
        let median = |i: usize, key: &str| rows.get(i).and_then(|r| r[key].as_f64()).unwrap_or(f64::NAN);
        let mut missed = Vec::new();
        let plant = a.plant_ms;
        for (what, judged, budget, unit) in [
            ("summaries", median(1, "p50_ms") + plant, SUMMARIES_BUDGET_MS, "ms"),
            ("summary per sketch", median(2, "p50_us") + plant * 1000.0, PER_SKETCH_BUDGET_US, "us"),
        ] {
            if judged.is_nan() || judged > budget {
                missed.push(format!("{what}: median {judged:.2} {unit}, over the {budget} {unit} budget"));
            }
        }
        for m in &missed {
            eprintln!("colour_summary: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
