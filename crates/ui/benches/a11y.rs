//! The AccessKit tree's cost (`development.md` §8, `ui-layout.md` → Keyboard and screen readers,
//! `roadmap.md` M15.34): the library's frame at 100 000 photos with AccessKit on (a screen reader
//! asked for the tree: egui's tree update built, `a11y::describe` gathering the window's shape and
//! its plugin reshaping the update) against the same frames with it off. The real app in the
//! headless driver at 1400 × 900. Run it with
//!
//! ```sh
//! cargo xtask bench a11y_frame                                         # release, history
//! cargo bench -p dalica-ui --bench a11y_frame -- --json
//! ```
//!
//! **The frames.** The synthetic index-only library of `--photos` (100 000) photos in folders of
//! 100 (`dalica_engine::synthetic`, as `library_bench`), settled at the top. From each of four
//! places (the top, then the sections a quarter, half and three quarters of the way down, reached
//! with `section.scrollTo`), the light table is scrolled down at 3 600 points a second for
//! `--frames` (120) wheel events, two frames each (the wheel's and its end's), once with AccessKit
//! off and once with it on, in turn (off first at even places, on first at odd ones); before each
//! pass the view goes back to the place and AccessKit is turned on or off, and the frames settle
//! (not timed: the first frame with AccessKit on builds the whole tree once). So both see the same
//! frames. Each frame is timed whole (`Driver::step`: the session's poll, layout, the AccessKit
//! tree when on, tessellation, texture uploads).
//!
//! Rows: `frame off <n>` and `frame on <n>` (p50/p95/max by nearest rank), and `overhead <n>`,
//! their difference at the median (`p50_ms`) and at the 95th percentile (`p95_ms`). `cargo xtask
//! budgets` judges the frame on (p95, within 1.5 × its first measurement and the 8 ms frame budget)
//! and the overhead at p95 (within [`OVERHEAD_BUDGET_MS`], never above [`OVERHEAD_CAP_MS`]);
//! `--check` makes the bench judge them itself (in release, over 100 000 photos), `--plant-ms N`
//! adds N ms to each frame on (so to the overhead too). Without `--bench` (under `cargo test
//! --benches`) it runs a smoke pass: 2 000 photos, ten wheel events a place.

use std::sync::Arc;
use std::time::Duration;

use dalica_engine::synthetic::{self, Spec};
use dalica_ui::{App, Driver};
use egui::vec2;
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement (the median of three runs,
/// release on two hardware threads, `taskset -c 0,1`, 2026-10-20: a frame with AccessKit on
/// 1.23 ms at p95, its overhead 0.234 ms), rounded up to 0.5 ms; the
/// frame never above the 8 ms frame budget, the overhead never above [`OVERHEAD_CAP_MS`].
const FRAME_BUDGET_MS: f64 = 2.0;
const OVERHEAD_BUDGET_MS: f64 = 0.5;
const FRAME_CAP_MS: f64 = 8.0;
const OVERHEAD_CAP_MS: f64 = 1.0;
/// The library the budgets hold for.
const PHOTOS: usize = 100_000;
/// Photos per folder: the synthetic 100 k preset's 100.
const PER_FOLDER: usize = 100;
/// Scroll speed, points per second (`library_bench`'s).
const SPEED: f32 = 3600.0;
/// The window, in pixels at 1×.
const WINDOW: (u32, u32) = (1400, 900);
/// Frames a jump or a switch may take to settle.
const SETTLE: usize = 60 * 60;

struct Args {
    photos: usize,
    frames: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args { photos: if full { PHOTOS } else { 2_000 }, frames: if full { 120 } else { 10 }, json: false, check: false, plant_ms: 0.0 };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--frames" => a.frames = value()?.parse().map_err(|e| format!("--frames: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --frames N, --check, --plant-ms N, --json)")),
        }
    }
    if !(PER_FOLDER * 10..=synthetic::MAX_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be {}..={}", PER_FOLDER * 10, synthetic::MAX_PHOTOS));
    }
    if !(1..=100_000).contains(&a.frames) || !(0.0..=1000.0).contains(&a.plant_ms) {
        return Err("--frames must be 1..=100000 and --plant-ms 0..=1000".into());
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

fn sorted(times: &[Duration]) -> Vec<f64> {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    t
}

fn row(name: String, t: &[f64], extra: Value) -> Value {
    let mut r = json!({ "name": name, "samples": t.len(), "p50_ms": rank(t, 50), "p95_ms": rank(t, 95), "max_ms": t.last().copied().unwrap_or(0.0) });
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

/// `100000` → `100k`.
fn label(n: usize) -> String {
    if n >= 1000 && n.is_multiple_of(1000) { format!("{}k", n / 1000) } else { n.to_string() }
}

/// Turns AccessKit on or off for the frames after.
fn set(d: &mut Driver, on: bool) {
    if on {
        d.ctx().enable_accesskit();
    } else {
        d.ctx().disable_accesskit();
    }
}

/// The nodes of the AccessKit tree update of one frame run outside the driver (whose step keeps
/// no output), with AccessKit on: the check that the timed frames built it.
fn tree_nodes(d: &mut Driver) -> usize {
    let ctx = d.ctx().clone();
    ctx.enable_accesskit();
    let (w, h, native) = d.size();
    let mut input = dalica_ui_kit::raw_input(w, h, native);
    let ppp = native * ctx.zoom_factor();
    input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(w as f32 / ppp, h as f32 / ppp)));
    input.time = None;
    let mut out = ctx.run_ui(input, |ui| d.app_mut().frame(ui));
    out.textures_delta.clear();
    out.platform_output.accesskit_update.map_or(0, |u| u.nodes.len())
}

fn bench(a: &Args) -> Vec<Value> {
    let dir = tempfile::Builder::new().prefix("a11y-frame-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let spec = Spec::new(a.photos, a.photos / PER_FOLDER, synthetic::SEED);
    let session = synthetic::open(dir.path(), &spec, Arc::new(dalica_core::SystemClock)).expect("the synthetic library opens");
    let mut d = Driver::new(App::new(Some(session)), WINDOW.0, WINDOW.1, 1.0);
    d.settle(SETTLE).expect("the first frames settle");
    let order: Vec<_> = d.app().panels().expect("the library is open").sections.list.iter().map(|s| s.id).collect();
    let step = 2.0 * SPEED / 60.0;
    let (mut off, mut on) = (Vec::new(), Vec::new());
    let mut cells = 0u64;
    for place in 0..4 {
        let target = order.get(order.len() * place / 4).expect("a section");
        let modes = if place % 2 == 0 { [false, true] } else { [true, false] };
        for mode in modes {
            set(&mut d, mode);
            d.run("section.scrollTo", &json!({ "section": target })).expect("section.scrollTo");
            d.wait(250).expect("the jump eases");
            d.settle(SETTLE).expect("the jump settles");
            let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).expect("a cell on screen").id;
            for k in 0..a.frames {
                d.scroll(if k == 0 { Some(&cell) } else { None }, vec2(0.0, -step)).expect("the wheel scrolls");
                let n = d.perf().samples().count();
                for s in d.perf().samples().skip(n.saturating_sub(2)) {
                    if mode {
                        on.push(s.update);
                    } else {
                        off.push(s.update);
                    }
                    cells = cells.max(s.cells_visited);
                }
            }
        }
    }
    let nodes = tree_nodes(&mut d);
    assert!(nodes > 100, "AccessKit on gives the window's tree: {nodes} nodes");
    set(&mut d, false);
    let (off, on) = (sorted(&off), sorted(&on));
    let extra = json!({ "photos": a.photos, "window": [WINDOW.0, WINDOW.1], "cells_visited_max": cells, "tree_nodes": nodes });
    let overhead = json!({
        "name": format!("overhead {}", label(a.photos)),
        "samples": on.len().min(off.len()),
        "p50_ms": rank(&on, 50) - rank(&off, 50),
        "p95_ms": rank(&on, 95) - rank(&off, 95),
        "photos": a.photos,
    });
    vec![row(format!("frame off {}", label(a.photos)), &off, extra.clone()), row(format!("frame on {}", label(a.photos)), &on, extra), overhead]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("a11y_frame: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "a11y_frame",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "window": [WINDOW.0, WINDOW.1],
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<18} {:>8} {:>9} {:>9} {:>9}", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            println!(
                "{:<18} {:>8} {:>9.3} {:>9.3} {:>9.3}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
    }
    if a.check && mode == "release" && a.photos >= PHOTOS {
        let p95 = |i: usize| rows[i]["p95_ms"].as_f64().unwrap_or(f64::NAN) + a.plant_ms;
        let mut missed = Vec::new();
        for (what, judged, budget) in
            [("frame on", p95(1), FRAME_BUDGET_MS.min(FRAME_CAP_MS)), ("overhead", p95(2), OVERHEAD_BUDGET_MS.min(OVERHEAD_CAP_MS))]
        {
            if judged.is_nan() || judged > budget {
                missed.push(format!("{what}: p95 {judged:.2} ms, over the {budget} ms budget"));
            }
        }
        for m in &missed {
            eprintln!("a11y_frame: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
