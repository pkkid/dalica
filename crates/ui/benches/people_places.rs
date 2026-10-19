//! The people and places views at library scale (`development.md` §8, `faces.md` §8,
//! `roadmap.md` M13.31): the Unnamed grid opening with 10 000 faces, and the Places pane with
//! 10 000 geotagged photos, a frame with its markers grouped. The real app in the headless driver
//! at 1400 × 900. Run it with
//!
//! ```sh
//! cargo xtask bench people_places                                  # release, history
//! cargo bench -p dalica-ui --bench people_places -- --photos 10000 --json
//! ```
//!
//! **The library.** The index-only synthetic library (`dalica_engine::synthetic`, 100 photos per
//! folder) of `--photos` photos (10 000), each with one unnamed face and a geotag, written as an
//! index snapshot and opened (not timed). The faces are planted in groups as the face index would
//! give them after a scan (`panels::faces::model::plant_groups`): sizes falling as 1 500 ÷ k at
//! 10 000 photos (the largest scaled with `--photos`, so a smaller library has as many groups and
//! the grid still overflows the window), the largest first, a long tail of small groups. The places are a trip's: each folder's photos at 20
//! points close together, the folders spread over Europe (2 000 places for 10 000 photos). There
//! are no files: face pictures never arrive, so each face draws its placeholder.
//!
//! Rows (each frame timed whole, `Driver::step`: the session's poll, layout, tessellation; no
//! rasterizing):
//! - `unnamed open`: the frame that opens Unnamed (the faces listed, grouped, laid out and the
//!   visible ones drawn), `--samples` (11) times, the grid made again each time;
//! - `unnamed scroll`: frames while the wheel scrolls the grid down (`--frames`, 120);
//! - `places open`: the frame after all the photos are selected with the Places pane open (their
//!   places worked out, the markers grouped and drawn), `--samples` times;
//! - `places pan`: frames while the map pans across the places, a marker grouping each frame
//!   (`--frames`).
//!
//! Rows carry `p50_ms`, `p95_ms`, `max_ms` and their budgets (`budget_ms` on an opening's median,
//! `budget_p95_ms` on a frame's p95); `cargo xtask budgets` judges them, `--check` makes the bench
//! judge them itself (in release, over 10 000 photos or more), `--plant-ms N` adds N ms to each
//! judged time. Without `--bench` (under `cargo test --benches`) it runs a smoke pass over 1 000
//! photos, two samples and ten frames.

use std::sync::Arc;
use std::time::Duration;

use dalica_core::geo::LatLon;
use dalica_core::{PhotoId, Rect01, SystemClock};
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_library::{Face, FacePerson, Geo, Op};
use dalica_ui::panels::faces::model::plant_groups;
use dalica_ui::state::FaceRef;
use dalica_ui::{App, Driver};
use egui::vec2;
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement (the median of three runs,
/// release on two cores, 2026-10-18: Unnamed opened 21.5 ms (median) and scrolled 0.481 ms (p95),
/// Places opened 1.05 ms (median) and panned 1.12 ms (p95)), rounded up to 0.5 ms.
const UNNAMED_OPEN_BUDGET_MS: f64 = 32.5;
const UNNAMED_SCROLL_BUDGET_MS: f64 = 1.0;
const PLACES_OPEN_BUDGET_MS: f64 = 2.0;
const PLACES_PAN_BUDGET_MS: f64 = 2.0;
/// The library the budgets hold for.
const PHOTOS: usize = 10_000;
/// The window, in pixels at 1×.
const WINDOW: (u32, u32) = (1400, 900);
/// Photos per folder, and places per folder.
const PER_FOLDER: usize = 100;
const PLACES_PER_FOLDER: usize = 20;
/// The largest planted group at [`PHOTOS`]: group `k` (from 1) holds 1 500 ÷ k faces, scaled with
/// the library (`largest_group`).
const LARGEST_GROUP: usize = 1_500;

struct Args {
    photos: usize,
    samples: usize,
    frames: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { PHOTOS } else { 1_000 },
        samples: if full { 11 } else { 2 },
        frames: if full { 120 } else { 10 },
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--photos" => a.photos = value()?.parse().map_err(|e| format!("--photos: {e}"))?,
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--frames" => a.frames = value()?.parse().map_err(|e| format!("--frames: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => {
                return Err(format!("unknown argument `{other}` (use --photos N, --samples N, --frames N, --check, --plant-ms N, --json)"));
            }
        }
    }
    if !(PER_FOLDER..=100_000).contains(&a.photos) {
        return Err(format!("--photos must be {PER_FOLDER}..=100000"));
    }
    if !(1..=1000).contains(&a.samples) || !(1..=10_000).contains(&a.frames) {
        return Err("--samples must be 1..=1000 and --frames 1..=10000".into());
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

/// A small deterministic sequence in `[0, 1)`.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The synthetic library of `photos`, each photo with one unnamed face and a geotag, written as an
/// index snapshot into `dir` and opened. Returns the session and the photos in id order.
fn library(dir: &std::path::Path, photos: usize) -> (Session, Vec<PhotoId>) {
    let spec = Spec::new(photos, photos / PER_FOLDER, synthetic::SEED);
    let mut lib = synthetic::library(dir, &spec).expect("the synthetic library");
    let mut ids: Vec<(dalica_core::FolderId, PhotoId)> = lib.photos().map(|p| (p.folder, p.id)).collect();
    ids.sort_by_key(|(_, id)| *id);
    let mut r = Lcg(synthetic::SEED);
    let mut centres = std::collections::HashMap::new();
    let (mut faces, mut geo) = (Vec::with_capacity(ids.len()), Vec::with_capacity(ids.len()));
    for (i, (folder, id)) in ids.iter().enumerate() {
        let x = 0.2 + 0.4 * r.next();
        let rect = Rect01::new(x, 0.2, x + 0.25, 0.6).expect("a face rectangle");
        faces.push((*id, vec![Face::new(rect, FacePerson::Unnamed)]));
        // A trip per folder somewhere in Europe; its photos at 20 points within a few kilometres.
        let (lat, lon) = *centres.entry(*folder).or_insert_with(|| (37.0 + 22.0 * r.next(), -9.0 + 38.0 * r.next()));
        let k = (i % PLACES_PER_FOLDER) as f64;
        let at = LatLon::new(lat + 0.004 * k, lon + 0.003 * (k * 1.7).sin()).expect("a point in Europe");
        geo.push((*id, Some(Geo::At(at))));
    }
    lib.apply(Op::Batch { ops: vec![Op::SetFaces { faces }, Op::SetGeo { geo }] }).expect("the faces and places");
    let lib_dir = synthetic::library_dir(dir);
    std::fs::create_dir_all(&lib_dir).expect("the library folder");
    dalica_store::snapshot::write(&lib_dir.join(dalica_store::snapshot::FILE), &lib, 0).expect("the index snapshot");
    let roots = json!({ "version": 1, "folders": [{ "path": synthetic::photos_dir(dir) }] });
    std::fs::write(lib_dir.join("folders.json"), serde_json::to_vec_pretty(&roots).expect("folders.json")).expect("folders.json");
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let s = Session::open_with(&lib_dir, Arc::new(SystemClock), &options).expect("the library opens");
    (s, ids.into_iter().map(|(_, id)| id).collect())
}

/// The largest group for a library of `photos`: [`LARGEST_GROUP`] at [`PHOTOS`], in proportion
/// otherwise, so every library makes about as many groups (hundreds: more than the window holds).
fn largest_group(photos: usize) -> usize {
    (LARGEST_GROUP * photos / PHOTOS).max(1)
}

/// The groups the face index would give the faces, by face: group `k` (from 1) takes the next
/// `largest_group` ÷ k faces in photo order, then one face a group.
fn groups(ids: &[PhotoId]) -> Vec<(FaceRef, u64)> {
    let mut out = Vec::with_capacity(ids.len());
    let largest = largest_group(ids.len());
    let (mut group, mut left) = (1u64, largest);
    for id in ids {
        out.push((FaceRef { photo: *id, index: 0 }, group));
        left = left.saturating_sub(1);
        if left == 0 {
            group += 1;
            left = (largest / group as usize).max(1);
        }
    }
    out
}

/// One frame's whole time, as the driver recorded it.
fn step(d: &mut Driver) -> Duration {
    d.step();
    d.perf().last().map_or(Duration::ZERO, |s| s.update)
}

fn bench(a: &Args) -> Vec<Value> {
    let dir = tempfile::Builder::new().prefix("people-places-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let (s, ids) = library(dir.path(), a.photos);
    assert_eq!(s.library().unnamed_faces().len(), a.photos, "a face per photo");
    assert_eq!(s.library().geotagged_photos().len(), a.photos, "a place per photo");
    let planted = groups(&ids);
    let group_count = planted.last().map_or(0, |(_, g)| *g);
    let mut d = Driver::new(App::new(Some(s)), WINDOW.0, WINDOW.1, 1.0);
    d.settle(60 * 60).expect("the library settles");

    // The Unnamed grid: opened with a grid made afresh each time (planting again changes its key).
    let mut opens = Vec::with_capacity(a.samples);
    for _ in 0..a.samples {
        d.app_mut().state_mut().unnamed.open = false;
        d.step();
        plant_groups(&planted);
        d.app_mut().state_mut().unnamed.open = true;
        opens.push(step(&mut d));
    }
    assert!(d.widgets().iter().any(|w| w.id == "group:0"), "the grid shows its groups");
    let top = |d: &Driver| d.widgets().into_iter().find(|w| w.id == "group:0").map_or(f32::NEG_INFINITY, |w| w.rect[1]);
    let before = top(&d);
    let mut scrolls = Vec::with_capacity(a.frames);
    while scrolls.len() < a.frames {
        // The wheel over the window's middle, which the grid fills.
        d.scroll(None, vec2(0.0, -120.0)).expect("the grid scrolls");
        // The wheel's two frames (its move, then its end), as the driver recorded them.
        let frames: Vec<Duration> = d.perf().samples().map(|s| s.update).collect();
        scrolls.extend(frames.iter().rev().take(2));
    }
    scrolls.truncate(a.frames);
    assert!(top(&d) < before, "the wheel scrolled the grid");
    d.app_mut().state_mut().unnamed.open = false;
    d.settle(600).expect("the light table settles");

    // The Places pane over every photo: a different order of the same photos each time, so the
    // places are worked out again.
    d.run("pane.places", &json!({})).expect("the Places pane opens");
    d.run("map.pan", &json!({ "centre": [48.0, 10.0], "zoom": 4 })).expect("the map shows Europe");
    d.settle(600).expect("the pane settles");
    let mut places_open = Vec::with_capacity(a.samples);
    let mut order = ids.clone();
    for _ in 0..a.samples {
        order.rotate_left(1);
        d.run("select.set", &json!({ "ids": order })).expect("every photo selected");
        places_open.push(step(&mut d));
    }
    let markers = d.widgets().iter().filter(|w| w.id.starts_with("marker:")).count();
    let status = d.widgets().into_iter().find(|w| w.id == "text:places.status").map(|w| w.label).unwrap_or_default();
    assert!(markers > 0, "the map shows markers");
    let mut pans = Vec::with_capacity(a.frames);
    for k in 0..a.frames {
        let lon = 10.0 + 0.05 * k as f64;
        d.run("map.pan", &json!({ "centre": [48.0, lon], "zoom": 4 })).expect("the map pans");
        pans.push(step(&mut d));
    }
    vec![
        row("unnamed open", &opens, json!({ "budget_ms": UNNAMED_OPEN_BUDGET_MS, "faces": a.photos, "groups": group_count })),
        row("unnamed scroll", &scrolls, json!({ "budget_p95_ms": UNNAMED_SCROLL_BUDGET_MS })),
        row("places open", &places_open, json!({ "budget_ms": PLACES_OPEN_BUDGET_MS, "photos": a.photos, "status": status })),
        row("places pan", &pans, json!({ "budget_p95_ms": PLACES_PAN_BUDGET_MS, "markers": markers })),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("people_places: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "people_places",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "photos": a.photos,
        "window": [WINDOW.0, WINDOW.1],
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<16} {:>8} {:>9} {:>9} {:>9}", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            println!(
                "{:<16} {:>8} {:>9.3} {:>9.3} {:>9.3}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
    }
    if a.check && mode == "release" && a.photos >= PHOTOS {
        let mut missed = Vec::new();
        for r in &rows {
            let name = r["name"].as_str().unwrap_or("?");
            let (stat, budget) = match (r["budget_ms"].as_f64(), r["budget_p95_ms"].as_f64()) {
                (Some(b), _) => ("p50_ms", b),
                (_, Some(b)) => ("p95_ms", b),
                _ => continue,
            };
            let judged = r[stat].as_f64().unwrap_or(f64::NAN) + a.plant_ms;
            if judged.is_nan() || judged > budget {
                missed.push(format!("{name}: {stat} {judged:.2} ms, over the {budget} ms budget"));
            }
        }
        for m in &missed {
            eprintln!("people_places: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
