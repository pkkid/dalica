//! The collage canvas at scale (`development.md` §8, `creating.md` §3, `roadmap.md` M14.22): the
//! Collage room's frame with 100 pictures on the page while one of them is dragged. The real app in
//! the headless driver at 1400 × 900. Run it with
//!
//! ```sh
//! cargo xtask bench collage_canvas                                 # release, history
//! cargo bench -p dalica-ui --bench collage_canvas -- --json
//! ```
//!
//! **The collage.** A library of `--pictures` (100) demo photos (`scenes::demo_library`, 480 × 320,
//! one folder) added and scanned; Create Collage on all of them with a fixed seed, a Picture Pile
//! (the style a drag moves a picture in, every picture turned, with its border and shadow), the
//! room opened and stepped until every picture has its thumbnail (not timed).
//!
//! Rows (each frame timed whole, `Driver::step`: the session's poll, layout, tessellation, texture
//! uploads; no rasterizing):
//! - `drag frame`: the frames while the top picture is dragged across the page, `--drags` (4) drags
//!   of `--frames` (60) pointer moves each, back and forth: the picture follows the pointer each
//!   frame and every other picture is drawn under it;
//! - `drop frame`: the frame each drag ends in (the move given to the engine, one step of the
//!   room's history, the placements laid out again), printed, not budgeted.
//!
//! The `drag frame` row carries `p50_ms`, `p95_ms`, `max_ms` and its budget (`budget_p95_ms`, on
//! its p95, never above 16 ms); `cargo xtask budgets` judges it, `--check` makes the bench judge it
//! itself (in release, with 100 pictures or more), `--plant-ms N` adds N ms to the judged time.
//! Without `--bench` (under `cargo test --benches`) it runs a smoke pass: 12 pictures, one drag of
//! ten moves.

use std::sync::Arc;
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::Session;
use dalica_ui::headless::{Drag, Target};
use dalica_ui::panels::collage::canvas;
use dalica_ui::{App, Driver};
use egui::vec2;
use serde_json::{Value, json};

/// The budget (`development.md` §8): 1.5 × the first measurement (the median of three runs,
/// release on two hardware threads, 2026-10-19: a drag frame's p95 0.167 ms), rounded up to 0.5 ms,
/// never above [`FRAME_CAP_MS`].
const DRAG_BUDGET_MS: f64 = 0.5;
const FRAME_CAP_MS: f64 = 16.0;
/// The collage the budget holds for.
const PICTURES: usize = 100;
/// The window, in pixels at 1×.
const WINDOW: (u32, u32) = (1400, 900);
/// The most frames the room may take to show every picture's thumbnail.
const LOAD_FRAMES: usize = 6000;

struct Args {
    pictures: usize,
    drags: usize,
    frames: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        pictures: if full { PICTURES } else { 12 },
        drags: if full { 4 } else { 1 },
        frames: if full { 60 } else { 10 },
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--pictures" => a.pictures = value()?.parse().map_err(|e| format!("--pictures: {e}"))?,
            "--drags" => a.drags = value()?.parse().map_err(|e| format!("--drags: {e}"))?,
            "--frames" => a.frames = value()?.parse().map_err(|e| format!("--frames: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => {
                return Err(format!("unknown argument `{other}` (use --pictures N, --drags N, --frames N, --check, --plant-ms N, --json)"));
            }
        }
    }
    if !(2..=1000).contains(&a.pictures) {
        return Err("--pictures must be 2..=1000".into());
    }
    // A drag's frames must stay inside the driver's frame record (`perf::WINDOW`).
    if !(1..=100).contains(&a.drags) || !(1..=dalica_ui::perf::WINDOW - 8).contains(&a.frames) {
        return Err(format!("--drags must be 1..=100 and --frames 1..={}", dalica_ui::perf::WINDOW - 8));
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

fn row(name: &str, times: &[Duration], extra: Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let mut r =
        json!({ "name": name, "samples": t.len(), "p50_ms": rank(&t, 50), "p95_ms": rank(&t, 95), "max_ms": t.last().copied().unwrap_or(0.0) });
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

/// A library of `n` demo photos in `dir`, scanned, and their ids.
fn library(dir: &std::path::Path, n: usize) -> (Session, Vec<PhotoId>) {
    let files = dalica_scenes::demo_library(1, n, 7).expect("the demo photos");
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the photos' folder");
        std::fs::write(path, &f.bytes).expect("a photo");
    }
    let folder = dir.join("photos").join(files[0].path.parent().expect("a folder"));
    let mut s = Session::open(&dir.join("library"), Arc::new(SystemClock)).expect("the library opens");
    s.execute("folders.add", &json!({ "path": folder })).expect("the folder added");
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 10_000 })).expect("the photos");
    let ids: Vec<PhotoId> = found["photos"].as_array().expect("photos").iter().filter_map(|p| p["id"].as_u64()).map(PhotoId::new).collect();
    assert_eq!(ids.len(), n, "every photo scanned");
    (s, ids)
}

/// The room with the pile in front, every picture drawn from its thumbnail.
fn room(a: &Args, dir: &std::path::Path) -> Driver {
    let (s, ids) = library(dir, a.pictures);
    let mut d = Driver::new(App::new(Some(s)), WINDOW.0, WINDOW.1, 1.0);
    d.run("create.collage", &json!({ "ids": ids, "seed": "c011a6e" })).expect("Create Collage");
    d.run("collage.set", &json!({ "style": "pile" })).expect("a pile");
    d.run("collage.open", &json!({})).expect("the room opens");
    d.settle(LOAD_FRAMES).expect("the room settles");
    for _ in 0..LOAD_FRAMES {
        if canvas::waiting(d.ctx()) == 0 {
            break;
        }
        d.step();
    }
    assert_eq!(canvas::waiting(d.ctx()), 0, "every picture has its thumbnail");
    d
}

fn bench(a: &Args) -> Vec<Value> {
    let dir = tempfile::Builder::new().prefix("collage-canvas-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let mut d = room(a, dir.path());
    let top = canvas::item_id(a.pictures as u32 - 1);
    assert!(d.widgets().iter().any(|w| w.id == top), "the top picture is on the canvas");
    let page = d.widgets().into_iter().find(|w| w.id == canvas::CANVAS_ID).expect("the canvas").rect;
    // A third of the page's width and a sixth of its height, left and down, then back.
    let reach = vec2(page[2] / 3.0, page[3] / 6.0);
    let undo = |d: &mut Driver| {
        d.app_mut().session_mut().expect("a session").execute("collage.inspect", &json!({})).expect("the collage")["undo"].as_u64().unwrap_or(0)
    };
    let steps = undo(&mut d);
    let (mut drags, mut drops) = (Vec::new(), Vec::new());
    for k in 0..a.drags {
        let delta = if k % 2 == 0 { -reach } else { reach };
        d.drag(&top, Target::Delta(delta), &Drag { steps: a.frames, ..Drag::default() }).expect("the picture dragged");
        // The drag's frames: the pointer to the picture, the press, one per move, the release
        // (the drop), the pointer away.
        let frames: Vec<Duration> = d.perf().samples().map(|s| s.update).collect();
        let n = frames.len();
        assert!(n >= a.frames + 4, "the drag's frames are recorded");
        drags.extend(&frames[n - 2 - a.frames..n - 2]);
        drops.push(frames[n - 2]);
        d.step();
    }
    assert_eq!(undo(&mut d), steps + a.drags as u64, "each drag one step of the room's history");
    vec![
        row(
            "drag frame",
            &drags,
            json!({ "budget_p95_ms": DRAG_BUDGET_MS.min(FRAME_CAP_MS), "pictures": a.pictures, "textures": canvas::textures(d.ctx()) }),
        ),
        row("drop frame", &drops, json!({ "pictures": a.pictures })),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("collage_canvas: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "collage_canvas",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "pictures": a.pictures,
        "window": [WINDOW.0, WINDOW.1],
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<12} {:>8} {:>9} {:>9} {:>9}", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            println!(
                "{:<12} {:>8} {:>9.3} {:>9.3} {:>9.3}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
    }
    if a.check && mode == "release" && a.pictures >= PICTURES {
        let mut missed = Vec::new();
        for r in &rows {
            let name = r["name"].as_str().unwrap_or("?");
            let Some(budget) = r["budget_p95_ms"].as_f64() else { continue };
            let judged = r["p95_ms"].as_f64().unwrap_or(f64::NAN) + a.plant_ms;
            if judged.is_nan() || judged > budget {
                missed.push(format!("{name}: p95 {judged:.2} ms, over the {budget} ms budget"));
            }
        }
        for m in &missed {
            eprintln!("collage_canvas: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
