//! The light table at library scale (`development.md` §6, §8): a synthetic index-only library
//! (`dalica_engine::synthetic`) shown by the real app in the headless driver, scrolled with the
//! wheel at a fixed speed. Reports the UI frame time (layout + tessellation, as `ui.inspect` →
//! `perf` measures it) by nearest rank, and the light-table cells visited per frame, at a tenth of
//! the size and at the full size: the cells visited must not grow with the library. Run it with
//!
//! ```sh
//! cargo xtask bench library_bench                          # release, 10 k and 100 k photos, history
//! cargo bench -p dalica-ui --bench library_bench -- --photos 20000 --frames 100 --json
//! ```
//!
//! **Rows** (their names are the keys `xtask bench` and `xtask perf` compare runs by):
//! - `scroll light table <n>`: every frame of the scroll, at `<n>` photos (`10k`, `100k`);
//! - `sections refresh <n>`: rebuilding the light table's sections (`Sections::refresh`, once per
//!   library revision) from the session's library;
//! - `layout <n>`: laying out every section (`Layout::new`, once per width, size or revision);
//!   each sample is the mean of a batch of layouts.
//!
//! **The scroll.** The window is 1400 × 900 at 1×. The scroll starts at four places spread over the
//! library (the top, then the sections a quarter, half and three quarters of the way down, reached
//! with `section.scrollTo` and left to settle; those frames are not measured) and from each one
//! scrolls down [`SPEED`] points per second: a wheel event of `2 × SPEED / 60` points every other
//! frame (the driver delivers a scroll and its end in two frames), `--frames` scrolls per place.
//! The library holds no files, so each thumbnail job fails at once and every cell shows its
//! placeholder; the UI work per cell is the same.
//!
//! Without `--bench` (under `cargo test --benches`, or `cargo run` of the bench target) it runs a
//! smoke pass at 1 k and 10 k photos.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_engine::synthetic::{self, Spec};
use dalica_library::TreeOptions;
use dalica_ui::panels::light_table::Sections;
use dalica_ui::panels::light_table::layout::{Layout, Spacing};
use dalica_ui::{App, Driver};
use egui::vec2;
use serde_json::{Value, json};

/// Scroll speed, points per second: about four screens a second, a brisk flick.
const SPEED: f32 = 3600.0;
/// The window, in pixels at 1×.
const WINDOW: (u32, u32) = (1400, 900);
/// Photos per folder on average: the synthetic 100 k preset's 100.
const PER_FOLDER: usize = 100;
/// Frames a jump or the first frames may take to settle.
const SETTLE: usize = 60 * 60;
/// Repetitions of the one-off rows (`sections refresh`, `layout`).
const REPS: usize = 7;
/// Layouts per `layout` sample.
const LAYOUT_BATCH: u32 = 50;

struct Args {
    photos: usize,
    frames: usize,
    json: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args { photos: if full { 100_000 } else { 10_000 }, frames: if full { 300 } else { 30 }, json: false };
    let mut it = all.iter();
    let number = |v: Option<&String>, what: &str| v.and_then(|v| v.parse::<usize>().ok()).ok_or_else(|| format!("{what} takes a number"));
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--photos" => a.photos = number(it.next(), "--photos")?,
            "--frames" => a.frames = number(it.next(), "--frames")?,
            "--json" => a.json = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --frames N, --json)")),
        }
    }
    if !(PER_FOLDER * 10..=synthetic::MAX_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be {}..={}", PER_FOLDER * 10, synthetic::MAX_PHOTOS));
    }
    if !(1..=100_000).contains(&a.frames) {
        return Err("--frames must be 1..=100000".into());
    }
    Ok(a)
}

/// `100000` → `100k`, `2500` → `2.5k`, `999` → `999`.
fn label(n: usize) -> String {
    match n {
        n if n >= 1000 && n % 1000 == 0 => format!("{}k", n / 1000),
        n if n >= 1000 => format!("{}k", n as f64 / 1000.0),
        n => n.to_string(),
    }
}

/// The nearest-rank `p`th percentile of `sorted` (as `perf` computes the 95th).
fn rank<T: Copy + Default>(sorted: &[T], p: usize) -> T {
    let n = sorted.len();
    if n == 0 {
        return T::default();
    }
    sorted.get((n * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or_default()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// A row: p50, p95 and max of `times` (milliseconds), plus `counters`.
fn row(name: String, times: &[Duration], counters: Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    json!({
        "name": name,
        "samples": t.len(),
        "p50_ms": rank(&t, 50),
        "p95_ms": rank(&t, 95),
        "max_ms": t.last().copied().unwrap_or(0.0),
        "counters": counters,
    })
}

/// One size: open the library, time the one-off rows, then scroll.
fn size(photos: usize, frames: usize) -> Vec<Value> {
    let name = label(photos);
    let dir = tempfile::tempdir().expect("a temporary folder");
    let spec = Spec::new(photos, photos / PER_FOLDER, synthetic::SEED);
    let started = Instant::now();
    let session = synthetic::open(dir.path(), &spec, Arc::new(dalica_core::SystemClock)).expect("the synthetic library opens");
    let opened = started.elapsed();
    eprintln!("library_bench: {name} photos in {} folders written and opened in {:.0} ms", spec.folders, ms(opened));

    // The one-off rows, on the session's own library.
    let options = TreeOptions::default();
    let mut refresh = Vec::new();
    let mut sections = Sections::default();
    for _ in 0..REPS {
        sections = Sections::default();
        let t = Instant::now();
        sections.refresh(session.library(), session.roots(), &options);
        refresh.push(t.elapsed());
    }
    let counts: Vec<_> = sections.list.iter().map(|s| (s.id, s.photos.len())).collect();
    let shown: usize = counts.iter().map(|(_, n)| n).sum();
    let mut layout = Vec::new();
    // A layout takes microseconds: each sample is the mean of a batch, so the row is steady
    // enough to compare between runs.
    for _ in 0..REPS {
        let t = Instant::now();
        for _ in 0..LAYOUT_BATCH {
            let l = Layout::new(counts.iter().copied(), 1, 1136.0, 144.0, false, Spacing::default());
            std::hint::black_box(l.height());
        }
        layout.push(t.elapsed() / LAYOUT_BATCH);
    }

    // The app, settled at the top.
    let mut d = Driver::new(App::new(Some(session)), WINDOW.0, WINDOW.1, 1.0);
    d.settle(SETTLE).expect("the first frames settle");
    let order: Vec<_> = d.app().panels().expect("the library is open").sections.list.iter().map(|s| s.id).collect();
    // A section per folder, after Starred Photos (the synthetic library stars some photos).
    assert_eq!(order.iter().filter(|s| s.folder().is_some()).count(), spec.folders, "a section per folder");
    let step = 2.0 * SPEED / 60.0;
    let mut times = Vec::new();
    let mut cells = Vec::new();
    for place in 0..4 {
        if place > 0 {
            let target = order.get(order.len() * place / 4).expect("a section");
            d.run("section.scrollTo", &json!({ "section": target })).expect("section.scrollTo");
            d.wait(250).expect("the jump eases");
            d.settle(SETTLE).expect("the jump settles");
        }
        let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).expect("a cell on screen").id;
        for k in 0..frames {
            d.scroll(if k == 0 { Some(&cell) } else { None }, vec2(0.0, -step)).expect("the wheel scrolls");
            // The scroll ran two frames: the wheel's and its end's.
            let n = d.perf().samples().count();
            for s in d.perf().samples().skip(n.saturating_sub(2)) {
                times.push(s.frame);
                cells.push(s.cells_visited);
            }
        }
    }
    cells.sort_unstable();
    let moved = d.app().panels().map_or(0.0, |p| p.light_table.offset());
    vec![
        row(format!("sections refresh {name}"), &refresh, json!({ "photos": photos, "shown": shown, "sections": counts.len() })),
        row(format!("layout {name}"), &layout, json!({ "sections": counts.len() })),
        row(
            format!("scroll light table {name}"),
            &times,
            json!({
                "photos": photos,
                "cells_visited_min": cells.first().copied().unwrap_or(0),
                "cells_visited_p50": rank(&cells, 50),
                "cells_visited_max": cells.last().copied().unwrap_or(0),
                "speed_pt_per_s": SPEED,
                "end_offset": moved,
            }),
        ),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("library_bench: {e}");
            std::process::exit(2);
        }
    };
    let mut rows = size(a.photos / 10, a.frames);
    rows.extend(size(a.photos, a.frames));
    let scroll = |n: usize| rows.iter().find(|r| r["name"] == format!("scroll light table {}", label(n))).map(|r| r["counters"].clone());
    let (small, big) = (scroll(a.photos / 10).unwrap_or_default(), scroll(a.photos).unwrap_or_default());
    // Flat: the big library visits no more cells per frame than the small one (a frame shows one
    // screen whatever the library's size; the screens differ a little with the folders' sizes).
    let flat = big["cells_visited_max"].as_u64() <= small["cells_visited_max"].as_u64().map(|m| m + m / 10);
    let report = json!({
        "bench": "library_bench",
        "mode": if cfg!(debug_assertions) { "debug" } else { "release" },
        "photos": a.photos,
        "frames": a.frames,
        "cells_flat": flat,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<28} {:>8} {:>9} {:>9} {:>9}   cells/frame (min p50 max)", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            let c = &r["counters"];
            let cells = match c.get("cells_visited_p50") {
                Some(p50) => format!("   {} {p50} {}", c["cells_visited_min"], c["cells_visited_max"]),
                None => String::new(),
            };
            println!(
                "{:<28} {:>8} {:>9.3} {:>9.3} {:>9.3}{cells}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
        println!("cells visited per frame flat from {} to {}: {flat}", label(a.photos / 10), label(a.photos));
    }
    if !flat {
        eprintln!("library_bench: the cells visited per frame grew with the library");
        std::process::exit(1);
    }
}
