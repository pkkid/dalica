//! A theme switch's cost (`design-system.md` §2 Themes, `development.md` §8, `roadmap.md` M16.11):
//! the first frame after View ▸ Theme ▸ switches the window's theme, with the library's light table
//! at 100 000 photos, against a frame without a switch at the same place. The real app in the
//! headless driver at 1400 × 900. Run it with
//!
//! ```sh
//! cargo xtask bench theme_switch                                       # release, history
//! cargo bench -p dalica-ui --bench theme_switch -- --json
//! ```
//!
//! **The frames.** The synthetic index-only library of `--photos` (100 000) photos in folders of
//! 100 (`dalica_engine::synthetic`, as `library_bench`), settled at the top. At each of four places
//! (the top, then the sections a quarter, half and three quarters of the way down, reached with
//! `section.scrollTo`), `--rounds` (2) times over every theme in turn (`settings::THEME_COMMANDS`,
//! the theme after the one in effect first): the theme's command runs as its menu row would
//! (`Driver::run`: the command, then one frame), and that frame is the **switch** frame (every text
//! laid out again in the new colours, the style applied); the frames then settle (not timed) and
//! one more frame is the **steady** frame. Each frame is timed whole (`Driver::step`: the session's
//! poll, layout, tessellation, texture uploads). The tokens are checked to wear the theme asked.
//!
//! Rows: `frame steady <n>` and `frame switch <n>` (p50/p95/max by nearest rank), and `overhead
//! <n>`, their difference at the median (`p50_ms`) and at the 95th percentile (`p95_ms`; printed).
//! `cargo xtask budgets` judges the switch frame at p95 (within [`SWITCH_BUDGET_MS`], never above
//! [`SWITCH_CAP_MS`]); `--check` makes the bench judge it itself (in release, over 100 000 photos),
//! `--plant-ms N` adds N ms to it. Without `--bench` (under `cargo test --benches`) it runs a smoke
//! pass: 2 000 photos, one round.

use std::sync::Arc;
use std::time::Duration;

use dalica_engine::settings::THEME_COMMANDS;
use dalica_engine::synthetic::{self, Spec};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// The budget (`development.md` §8): 1.5 × the first measurement at p95 (the median of three
/// runs, release on two hardware threads, `taskset -c 0,1`, 2026-11-06: 1.82 ms), rounded up to 0.5 ms;
/// never above [`SWITCH_CAP_MS`].
const SWITCH_BUDGET_MS: f64 = 3.0;
const SWITCH_CAP_MS: f64 = 16.0;
/// The library the budget holds for.
const PHOTOS: usize = 100_000;
/// Photos per folder: the synthetic 100 k preset's 100.
const PER_FOLDER: usize = 100;
/// The window, in pixels at 1×.
const WINDOW: (u32, u32) = (1400, 900);
/// Frames a jump or a switch may take to settle.
const SETTLE: usize = 60 * 60;

struct Args {
    photos: usize,
    rounds: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args { photos: if full { PHOTOS } else { 2_000 }, rounds: if full { 2 } else { 1 }, json: false, check: false, plant_ms: 0.0 };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--rounds" => a.rounds = value()?.parse().map_err(|e| format!("--rounds: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --rounds N, --check, --plant-ms N, --json)")),
        }
    }
    if !(PER_FOLDER * 10..=synthetic::MAX_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos must be {}..={}", PER_FOLDER * 10, synthetic::MAX_PHOTOS));
    }
    if !(1..=100).contains(&a.rounds) || !(0.0..=1000.0).contains(&a.plant_ms) {
        return Err("--rounds must be 1..=100 and --plant-ms 0..=1000".into());
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

fn sorted(mut t: Vec<f64>) -> Vec<f64> {
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

/// The last frame's whole time, in ms.
fn last_frame(d: &Driver) -> f64 {
    d.perf().samples().last().map_or(f64::NAN, |s| ms(s.update))
}

fn bench(a: &Args) -> Vec<Value> {
    let dir = tempfile::Builder::new().prefix("theme-switch-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let spec = Spec::new(a.photos, a.photos / PER_FOLDER, synthetic::SEED);
    let session = synthetic::open(dir.path(), &spec, Arc::new(dalica_core::SystemClock)).expect("the synthetic library opens");
    let mut d = Driver::new(App::new(Some(session)), WINDOW.0, WINDOW.1, 1.0);
    d.settle(SETTLE).expect("the first frames settle");
    let order: Vec<_> = d.app().panels().expect("the library is open").sections.list.iter().map(|s| s.id).collect();
    let (mut steady, mut switch) = (Vec::new(), Vec::new());
    let mut cells = 0u64;
    for place in 0..4 {
        let target = order.get(order.len() * place / 4).expect("a section");
        d.run("section.scrollTo", &json!({ "section": target })).expect("section.scrollTo");
        d.wait(250).expect("the jump eases");
        d.settle(SETTLE).expect("the jump settles");
        for _ in 0..a.rounds {
            // Every theme once, the one in effect last, so each run is a change.
            let now = d.app().tokens().kind.id();
            let at = THEME_COMMANDS.iter().position(|(id, _)| id.strip_prefix("theme.") == Some(now)).unwrap_or(0);
            for k in 1..=THEME_COMMANDS.len() {
                let (command, _) = THEME_COMMANDS[(at + k) % THEME_COMMANDS.len()];
                d.run(command, &json!({})).expect("the theme command runs");
                switch.push(last_frame(&d));
                assert_eq!(Some(d.app().tokens().kind.id()), command.strip_prefix("theme."), "the frame wore {command}");
                d.settle(SETTLE).expect("the switch settles");
                d.step();
                steady.push(last_frame(&d));
                cells = cells.max(d.perf().samples().last().map_or(0, |s| s.cells_visited));
            }
        }
    }
    let (steady, switch) = (sorted(steady), sorted(switch));
    let extra = json!({ "photos": a.photos, "window": [WINDOW.0, WINDOW.1], "themes": THEME_COMMANDS.len(), "cells_visited_max": cells });
    let overhead = json!({
        "name": format!("overhead {}", label(a.photos)),
        "samples": switch.len().min(steady.len()),
        "p50_ms": rank(&switch, 50) - rank(&steady, 50),
        "p95_ms": rank(&switch, 95) - rank(&steady, 95),
        "photos": a.photos,
    });
    vec![
        row(format!("frame steady {}", label(a.photos)), &steady, extra.clone()),
        row(format!("frame switch {}", label(a.photos)), &switch, extra),
        overhead,
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("theme_switch: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "theme_switch",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "window": [WINDOW.0, WINDOW.1],
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<20} {:>8} {:>9} {:>9} {:>9}", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            println!(
                "{:<20} {:>8} {:>9.3} {:>9.3} {:>9.3}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
    }
    if a.check && mode == "release" && a.photos >= PHOTOS {
        let judged = rows.get(1).and_then(|r| r["p95_ms"].as_f64()).unwrap_or(f64::NAN) + a.plant_ms;
        let budget = SWITCH_BUDGET_MS.min(SWITCH_CAP_MS);
        if judged.is_nan() || judged > budget {
            eprintln!("theme_switch: over budget: frame switch: p95 {judged:.2} ms, over the {budget} ms budget");
            std::process::exit(1);
        }
    }
}
