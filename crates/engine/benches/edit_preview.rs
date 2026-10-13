//! The edit view's preview renders (`development.md` §8, `roadmap.md` M7.11). Run it with
//!
//! ```sh
//! cargo xtask bench edit_preview                                   # release, 24 MP JPEGs
//! cargo bench -p dalica-engine --bench edit_preview -- --json
//! cargo bench -p dalica-engine --bench edit_preview -- --check --plant-ms 20   # must fail
//! ```
//!
//! Four synthetic JPEGs of `--width × --height` (6000 × 4000, 24 MP, by default) are written first
//! (not timed); the preview is fitted to `--fit` (2560 × 1600: the screen-size preview). Rows:
//!
//! - `open cold`: a session with an empty thumbnail cache opens a photo in the edit view; the time
//!   from `open_preview` to the poll that takes its final preview (the file decoded to the screen
//!   size, rendered, its histogram counted). Budget 150 ms.
//! - `open cached`: the same in a new session on the cache the cold opens filled (the ladder's
//!   screen entry read instead of the file decoded). Budget 150 ms.
//! - `drag`: a Tuning drag of [`DRAG_STEPS`] steps on the open photo: each step sets a new
//!   candidate and times its draft (`draft_*`, budget 16 ms) and its final (`final_*`, budget
//!   100 ms).
//! - `next previous`: forward through the photos and back, with the next and previous prefetched
//!   while each is shown (as the edit view does); the time from `open_preview` to its final preview.
//!   Budget 50 ms.
//!
//! Times are p50 / p95 / max by nearest rank; the budgets are judged on p95 (the median for the
//! opens, which have few samples and each a fresh session). With `--bench` (`cargo bench`, `cargo xtask bench`) or `--check` the
//! bench exits non-zero when one is missed, in a release build only; `--plant-ms N` adds `N` ms to
//! every judged time (a planted slowdown the budgets job must catch). Without `--bench` (`cargo test
//! --benches`) it runs a smoke pass on small photos and judges nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions, UpdateKind};
use dalica_fs::Expect;
use serde_json::{Value, json};

const OPEN_BUDGET_MS: f64 = 150.0;
const DRAFT_BUDGET_MS: f64 = 16.0;
const FINAL_BUDGET_MS: f64 = 100.0;
const STEP_BUDGET_MS: f64 = 50.0;
/// Photos written (the open one and its neighbours).
const PHOTOS: usize = 4;
/// Cold and cached opens measured.
const OPENS: usize = 7;
/// Steps of the Tuning drag.
const DRAG_STEPS: usize = 40;
const WAIT: Duration = Duration::from_secs(120);

struct Args {
    width: u32,
    height: u32,
    fit: (u32, u32),
    dir: PathBuf,
    json: bool,
    judge: bool,
    plant: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        width: if full { 6000 } else { 600 },
        height: if full { 4000 } else { 400 },
        fit: if full { (2560, 1600) } else { (256, 160) },
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
        judge: full,
        plant: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        let number = |v: String| v.parse::<u32>().map_err(|e| format!("{arg}: {e}"));
        match arg.as_str() {
            "--width" => a.width = number(value()?)?,
            "--height" => a.height = number(value()?)?,
            "--fit" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--fit takes WxH")?;
                a.fit = (w.parse().map_err(|e| format!("--fit: {e}"))?, h.parse().map_err(|e| format!("--fit: {e}"))?);
            }
            "--dir" => a.dir = PathBuf::from(value()?),
            "--json" => a.json = true,
            "--check" => a.judge = true,
            "--plant-ms" => a.plant = value()?.parse().ok().filter(|p: &f64| p.is_finite() && *p >= 0.0).ok_or("--plant-ms takes milliseconds")?,
            _ => {}
        }
    }
    if a.width == 0 || a.height == 0 || a.width > 20_000 || a.height > 20_000 {
        return Err("--width and --height take 1 … 20000".into());
    }
    Ok(a)
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn row(name: &str, prefix: &str, mut times: Vec<f64>) -> Value {
    times.sort_by(f64::total_cmp);
    json!({
        "name": name,
        format!("{prefix}p50_ms"): rank(&times, 50),
        format!("p95_{prefix}ms"): rank(&times, 95),
        format!("max_{prefix}ms"): times.last().copied().unwrap_or(0.0),
        "samples": times.len(),
    })
}

fn options(cache: &Path) -> Options {
    Options { thumbs: ThumbOptions { cache: CacheDir::At(cache.to_path_buf()), ..ThumbOptions::default() } }
}

/// A library watching the photos, on the thumbnail cache `cache`; ids in name order.
fn session(root: &Path, photos: &Path, cache: &Path) -> (Session, Vec<PhotoId>) {
    let library = root.join(format!("library-{}", cache.file_name().and_then(|n| n.to_str()).unwrap_or("x")));
    let mut s = Session::open_with(&library, Arc::new(SystemClock), &options(cache)).unwrap();
    s.execute("folders.add", &json!({ "path": photos })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    (s, ids.into_iter().map(|(_, id)| id).collect())
}

/// The time from now to the final preview of the photo just asked for.
fn until_final(s: &mut Session, started: Instant) -> f64 {
    loop {
        let u = s.wait_preview_update(WAIT).expect("a preview");
        if u.kind == UpdateKind::Final {
            return started.elapsed().as_secs_f64() * 1000.0;
        }
    }
}

fn opens(a: &Args, root: &Path, photos: &Path) -> (Value, Value) {
    let (mut cold, mut cached) = (Vec::new(), Vec::new());
    for i in 0..OPENS {
        let cache = root.join(format!("cache-{i}"));
        let (mut s, ids) = session(root, photos, &cache);
        let t = Instant::now();
        s.open_preview(ids[0], a.fit).unwrap();
        cold.push(until_final(&mut s, t));
        // The screen entry stored, then a new session on that cache.
        assert!(s.wait_preview(WAIT));
        drop(s);
        let (mut s, ids) = session(root, photos, &cache);
        let t = Instant::now();
        s.open_preview(ids[0], a.fit).unwrap();
        cached.push(until_final(&mut s, t));
        assert!(s.wait_preview(WAIT));
    }
    (row("open cold", "", cold), row("open cached", "", cached))
}

fn drag(a: &Args, root: &Path, photos: &Path) -> Value {
    let (mut s, ids) = session(root, photos, &root.join("cache-drag"));
    s.open_preview(ids[0], a.fit).unwrap();
    assert!(s.wait_preview(WAIT));
    s.take_preview_updates();
    let (mut drafts, mut finals) = (Vec::new(), Vec::new());
    for step in 0..DRAG_STEPS {
        let fill = step as f64 / DRAG_STEPS as f64;
        let candidate = vec![format!("tuning({fill:.3},0.1,0.05,0.2,none)")];
        let t = Instant::now();
        s.set_preview_candidate(Some(&candidate)).unwrap();
        loop {
            let u = s.wait_preview_update(WAIT).expect("a preview");
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            match u.kind {
                UpdateKind::Draft => drafts.push(ms),
                UpdateKind::Final => {
                    finals.push(ms);
                    break;
                }
                UpdateKind::Refined => {}
            }
        }
    }
    let mut r = row("drag", "draft_", drafts);
    let f = row("drag", "final_", finals);
    if let (Some(r), Some(f)) = (r.as_object_mut(), f.as_object()) {
        for (k, v) in f {
            if k != "name" && k != "samples" {
                r.insert(k.clone(), v.clone());
            }
        }
    }
    r
}

fn next_previous(a: &Args, root: &Path, photos: &Path) -> Value {
    let (mut s, ids) = session(root, photos, &root.join("cache-steps"));
    let neighbours =
        |i: usize| -> Vec<PhotoId> { [i.checked_add(1), i.checked_sub(1)].into_iter().flatten().filter_map(|j| ids.get(j).copied()).collect() };
    s.open_preview(ids[0], a.fit).unwrap();
    s.prefetch_previews(&neighbours(0));
    assert!(s.wait_preview(WAIT));
    let order: Vec<usize> = (1..ids.len()).chain((0..ids.len() - 1).rev()).collect();
    let mut times = Vec::new();
    for _ in 0..2 {
        for &i in &order {
            s.take_preview_updates();
            let t = Instant::now();
            s.open_preview(ids[i], a.fit).unwrap();
            times.push(until_final(&mut s, t));
            // While the photo is shown, its neighbours are prefetched.
            s.prefetch_previews(&neighbours(i));
            assert!(s.wait_preview(WAIT));
        }
    }
    row("next previous", "", times)
}

fn field(r: &Value, key: &str) -> f64 {
    r[key].as_f64().unwrap_or(f64::INFINITY)
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("edit_preview: {e}");
            std::process::exit(2);
        }
    };
    let root = a.dir.join(format!("edit-preview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let photos = root.join("photos");
    dalica_fs::create_dir_all(&photos).unwrap();
    let written = Instant::now();
    for i in 0..PHOTOS {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64 + 1, a.width, a.height), 90, None).unwrap();
        dalica_fs::replace(&photos.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    eprintln!("{PHOTOS} photos of {} × {} written in {:.1} s", a.width, a.height, written.elapsed().as_secs_f64());

    let (cold, cached) = opens(&a, &root, &photos);
    let rows = vec![cold, cached, drag(&a, &root, &photos), next_previous(&a, &root, &photos)];
    let _ = std::fs::remove_dir_all(&root);

    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let mut missed = Vec::new();
    let checks: [(usize, &str, f64, &str); 5] = [
        (0, "p50_ms", OPEN_BUDGET_MS, "open cold"),
        (1, "p50_ms", OPEN_BUDGET_MS, "open cached"),
        (2, "p95_draft_ms", DRAFT_BUDGET_MS, "drag draft"),
        (2, "p95_final_ms", FINAL_BUDGET_MS, "drag final"),
        (3, "p95_ms", STEP_BUDGET_MS, "next/previous"),
    ];
    for (i, key, budget, what) in checks {
        let judged = rows.get(i).map_or(f64::INFINITY, |r| field(r, key)) + a.plant;
        if judged > budget {
            missed.push(format!("{what}: {key} {judged:.1} ms over the {budget} ms budget"));
        }
    }
    if a.json {
        println!("{}", json!({ "mode": mode, "width": a.width, "height": a.height, "fit": [a.fit.0, a.fit.1], "plant_ms": a.plant, "rows": rows }));
    } else {
        for r in &rows {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
    }
    if !a.judge {
        return;
    }
    if mode != "release" {
        eprintln!("edit_preview: the budgets hold in release only; a {mode} build judges nothing");
        return;
    }
    for m in &missed {
        eprintln!("over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
    eprintln!("edit_preview: every budget held");
}
