//! The slideshow's frames (`development.md` §8, `roadmap.md` M11.23): the real app in the headless
//! driver plays a slideshow of camera-size JPEGs in real time, every transition in turn, and
//! times its frames. Run it with
//!
//! ```sh
//! cargo xtask bench slideshow                                      # release: 24 MP photos, history
//! cargo bench -p dalica-ui --bench slideshow -- --width 6000 --height 4000 --advances 5 --json
//! ```
//!
//! `--photos` `scenes` JPEGs (6, `--width × --height`: 6000 × 4000, a 24 MP camera's) are written
//! to a folder and scanned into a new library on the system clock (not timed). The app runs in the
//! headless driver at 1400 × 900, 1×, one frame every 1/60 s of real time as a display would ask
//! for them, Options ▸ Slideshow at its defaults (3 s display time, captions on) with Loop on. For
//! each transition but Cut (Dissolve, Wipe, Circle, Push, Rectangle, Pan and Zoom) a show of the
//! folder starts from its first photo (`section.slideshow`) and plays `--advances` changes (5); the
//! next photo's screen-size preview is made while the one before it shows (the engine's
//! `Slideshow`: the other preview pane and the prefetch), so a change never waits on the disk.
//! Each frame is timed whole (`Driver::step`: the session's poll, the show brought up to the
//! clock, layout, texture uploads, tessellation; no rasterizing, as a GPU does that):
//! - `advance`: the frame where the show moves on to the next photo, the first frame of its
//!   transition (both photos' textures already uploaded: the incoming photo's while the one
//!   before it showed, M11.29);
//! - `transition frame`: every other frame while a change is under way (both photos painted where
//!   the transition places them), judged on its p95, never above 16 ms whatever the first
//!   measurement;
//! - `transition frame <id>`: the same per transition (printed, not budgeted);
//! - `upload frame`: a frame outside a change that uploaded a texture, the next photo's preview
//!   once it is made, during the display time (printed, not budgeted: nothing moves but Pan and
//!   Zoom then, and the show does not wait on it).
//!
//! `waits` (on the `advance` row) counts the changes whose incoming preview was not ready when they
//! began, over every show: it must be 0. Rows carry `p50_ms`, `p95_ms`, `max_ms` and their budgets
//! (`budget_ms` on the advance's median, `budget_p95_ms` on a transition frame's p95);
//! `cargo xtask budgets` judges them. It exits non-zero when a change waited, and with `--check`
//! also when a budget is missed (in release, at the full photo size or larger). Without `--bench`
//! (under `cargo test --benches`) it runs a smoke pass: three photos of 640 × 427, one advance per
//! transition.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::Session;
use dalica_ui::commands::slideshow as cmd;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement, release on two cores
/// (2026-10-17, the median of three runs: the advance's median 16.0 ms, a transition frame's p95
/// 0.199 ms), rounded up; a
/// transition frame within [`FRAME_CAP_MS`] at p95 whatever that measurement was.
const ADVANCE_BUDGET_MS: f64 = 24.0;
const TRANSITION_BUDGET_MS: f64 = 0.3;
const FRAME_CAP_MS: f64 = 16.0;
/// The photo size the budgets hold for: a 24 MP camera's.
const FULL: (u32, u32) = (6000, 4000);
/// The window, in pixels at 1×.
const WINDOW: (u32, u32) = (1400, 900);
/// The transitions timed, by their Options ids (Cut has no transition frames).
const TRANSITIONS: [&str; 6] = ["dissolve", "wipe", "circle", "push", "rectangle", "panAndZoom"];
/// Frames per second the display asks for.
const FPS: f64 = 60.0;
/// The longest one show may run.
const TIMEOUT: Duration = Duration::from_secs(600);

struct Args {
    photos: usize,
    size: (u32, u32),
    advances: u64,
    json: bool,
    check: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { 6 } else { 3 },
        size: if full { FULL } else { (640, 427) },
        advances: if full { 5 } else { 1 },
        json: false,
        check: false,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--width" => a.size.0 = value()?.parse().map_err(|e| format!("--width: {e}"))?,
            "--height" => a.size.1 = value()?.parse().map_err(|e| format!("--height: {e}"))?,
            "--advances" => a.advances = value()?.parse().map_err(|e| format!("--advances: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => return Err(format!("unknown argument `{other}` (use --photos N, --width PX, --height PX, --advances N, --check, --json)")),
        }
    }
    if !(2..=100).contains(&a.photos) {
        return Err("--photos must be 2..=100".into());
    }
    if !(16..=12_000).contains(&a.size.0) || !(16..=12_000).contains(&a.size.1) {
        return Err("--width and --height must be 16..=12000".into());
    }
    if !(1..=100).contains(&a.advances) {
        return Err("--advances must be 1..=100".into());
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

/// Sleeps until frame `n` of a show that started at `start` is due.
fn pace(start: Instant, n: u64) {
    let due = start + Duration::from_secs_f64(n as f64 / FPS);
    if let Some(wait) = due.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
}

/// One show's frames: the advances' and the transition frames' times, and its changes and waits.
struct Played {
    advances: Vec<Duration>,
    frames: Vec<Duration>,
    uploads: Vec<Duration>,
    changes: u64,
    waits: u64,
}

/// Plays `advances` changes of a show of `section` from `first` with `transition`.
fn play(d: &mut Driver, section: &str, first: PhotoId, transition: &str, advances: u64) -> Played {
    let s = d.app_mut().session_mut().expect("the library is open");
    s.execute("options.set", &json!({ "values": { "slideshow.transition": transition } })).expect("the transition is set");
    d.run("section.slideshow", &json!({ "section": section, "ids": [first] })).expect("the show starts");
    let mut out = Played { advances: Vec::new(), frames: Vec::new(), uploads: Vec::new(), changes: 0, waits: 0 };
    let start = Instant::now();
    let mut n = 0u64;
    loop {
        let t = Instant::now();
        d.step();
        let took = t.elapsed();
        let stats = cmd::stats().expect("the show plays");
        let changing = cmd::frame().is_some_and(|f| f.view.change.is_some());
        if stats.changes > out.changes {
            out.advances.push(took);
        } else if changing {
            out.frames.push(took);
        } else if d.perf().last().is_some_and(|s| s.textures_uploaded > 0) {
            out.uploads.push(took);
        }
        out.changes = stats.changes;
        out.waits = stats.waits;
        if out.changes >= advances && !changing {
            break;
        }
        assert!(start.elapsed() < TIMEOUT, "{transition}: {} of {advances} changes after {} s", out.changes, TIMEOUT.as_secs());
        n += 1;
        pace(start, n);
    }
    d.run("slideshow.exit", &json!({})).expect("the show ends");
    d.step();
    out
}

fn bench(a: &Args) -> Vec<Value> {
    let dir = tempfile::Builder::new().prefix("slideshow-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let folder = dir.path().join("photos").join("Trip");
    std::fs::create_dir_all(&folder).expect("the folder");
    let written = Instant::now();
    std::thread::scope(|scope| {
        for k in 0..a.photos {
            let folder = &folder;
            let (w, h) = a.size;
            scope.spawn(move || {
                let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(100 + k as u64, w, h), 90, None).expect("a scenes JPEG");
                std::fs::write(folder.join(format!("IMG_{k:04}.jpg")), bytes).expect("the photo is written");
            });
        }
    });
    let mut s = Session::open(&dir.path().join("library"), Arc::new(SystemClock)).expect("the library opens");
    s.execute("folders.add", &json!({ "path": folder })).expect("folders.add");
    s.wait_idle();
    s.execute("options.set", &json!({ "values": { "slideshow.loop": true } })).expect("Loop on");
    let any = s.library().photos().next().expect("a photo").folder;
    let section = format!("folder:{any}");
    let list = s.execute("slideshow.list", &json!({ "section": section })).expect("slideshow.list");
    let order: Vec<PhotoId> = serde_json::from_value(list["photos"].clone()).expect("the photos");
    assert_eq!(order.len(), a.photos, "every photo plays");
    eprintln!("slideshow: {} photos of {} × {} written and scanned in {:.1} s", a.photos, a.size.0, a.size.1, written.elapsed().as_secs_f64());

    let mut d = Driver::new(App::new(Some(s)), WINDOW.0, WINDOW.1, 1.0);
    d.settle(60 * 60).expect("the library settles");
    let (mut advances, mut frames, mut uploads, mut per, mut changes, mut waits) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0, 0);
    for t in TRANSITIONS {
        let p = play(&mut d, &section, order[0], t, a.advances);
        eprintln!("slideshow: {t}: {} changes, {} waits, {} transition frames", p.changes, p.waits, p.frames.len());
        advances.extend(p.advances.iter().copied());
        frames.extend(p.frames.iter().copied());
        uploads.extend(p.uploads.iter().copied());
        changes += p.changes;
        waits += p.waits;
        per.push(row(&format!("transition frame {t}"), &p.frames, json!({})));
    }
    let mut rows = vec![
        row("advance", &advances, json!({ "budget_ms": ADVANCE_BUDGET_MS, "changes": changes, "waits": waits, "photo_px": [a.size.0, a.size.1] })),
        row("transition frame", &frames, json!({ "budget_p95_ms": TRANSITION_BUDGET_MS.min(FRAME_CAP_MS), "cap_p95_ms": FRAME_CAP_MS })),
    ];
    rows.extend(per);
    rows.push(row("upload frame", &uploads, json!({})));
    rows
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("slideshow: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "slideshow",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "photo_px": [a.size.0, a.size.1],
        "window": [WINDOW.0, WINDOW.1],
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<28} {:>8} {:>9} {:>9} {:>9}", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            println!(
                "{:<28} {:>8} {:>9.3} {:>9.3} {:>9.3}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
    }
    let mut missed = Vec::new();
    let get = |name: &str, key: &str| rows.iter().find(|r| r["name"] == name).and_then(|r| r[key].as_f64()).unwrap_or(f64::NAN);
    let waits = get("advance", "waits");
    if waits != 0.0 {
        missed.push(format!("{waits} changes waited for their preview (must be 0)"));
    }
    if a.check && mode == "release" && a.size.0 * a.size.1 >= FULL.0 * FULL.1 {
        let p50 = get("advance", "p50_ms");
        if p50.is_nan() || p50 > ADVANCE_BUDGET_MS {
            missed.push(format!("advance: median {p50:.2} ms, over the {ADVANCE_BUDGET_MS} ms budget"));
        }
        let p95 = get("transition frame", "p95_ms");
        let budget = TRANSITION_BUDGET_MS.min(FRAME_CAP_MS);
        if p95.is_nan() || p95 > budget {
            missed.push(format!("transition frame: p95 {p95:.3} ms, over the {budget} ms budget"));
        }
    }
    for m in &missed {
        eprintln!("slideshow: over budget: {m}");
    }
    if !missed.is_empty() {
        std::process::exit(1);
    }
}
