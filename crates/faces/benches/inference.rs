//! Face inference (`development.md` §8, `faces.md` §8, `roadmap.md` M13.31): detection on a 640 px
//! picture and one embedding per candidate recogniser, on seeded synthetic networks of the
//! candidate models' shapes (`dalica_faces::synthetic`: YuNet 2023mar's, SFace 2021dec's and
//! AuraFace v1's input, outputs, operators and layer pattern, with about their weights). Reserved
//! decision 5 has not chosen a model, and no model is ever downloaded for a bench: the weights are
//! seeded noise, so the time is what a real model of that shape costs on this interpreter. Run it
//! with
//!
//! ```sh
//! cargo xtask bench inference                                        # release, history
//! cargo bench -p dalica-faces --bench inference -- --samples 21 --json
//! ```
//!
//! Each network is written as an ONNX file and loaded back through the loader a downloaded model
//! goes through, with its candidate's manifest (`known::yunet`, `known::sface`,
//! `known::auraface`), once (`load_ms`, printed). Then, single-threaded as the scan's workers run
//! it, `--samples` (21) times each:
//! - `detect 640`: `Detector::detect` on a 640 × 427 photo-like picture (a 3:2 photo at the
//!   detector's 640 px; letterboxed, run, decoded, suppressed), the network reporting two planted
//!   faces as the scan bench's does;
//! - `embed sface` and `embed auraface`: `Embedder::embed` of one aligned 112 × 112 face.
//!
//! Rows carry `p50_ms`, `p95_ms`, `max_ms`, their `budget_ms` (judged on the median), the network's
//! `weights` and multiply-adds (`gmacs`). `cargo xtask budgets` judges the medians; `--check` makes
//! the bench judge them itself (in release), `--plant-ms N` adds N ms to each judged median.
//! Without `--bench` (under `cargo test --benches`) it runs a smoke pass: a 160 px detector input
//! and the SFace shape, one sample, no AuraFace.

use std::time::{Duration, Instant};

use dalica_faces::align::Rgb;
use dalica_faces::detect::{Detector, probe_picture};
use dalica_faces::embed::Embedder;
use dalica_faces::known;
use dalica_faces::runtime::Model;
use dalica_faces::synthetic::{self, Planted, Shaped};
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement (the median of three runs,
/// release on two cores, 2026-10-18: detection 41.3 ms, SFace 45.2 ms, AuraFace 230.3 ms),
/// rounded up to 0.5 ms (detection, SFace) and 5 ms (AuraFace).
const DETECT_BUDGET_MS: f64 = 62.5;
const SFACE_BUDGET_MS: f64 = 68.0;
const AURAFACE_BUDGET_MS: f64 = 350.0;
/// The detector's input side the budgets hold for (the candidate's).
const SIDE: usize = 640;
/// The picture searched: a 3:2 photo at the detector's side.
const PICTURE: (usize, usize) = (640, 427);
/// The two faces the detector reports (stride-16 cells, box sides in cells), as the scan bench's.
pub const PLANTED: [Planted; 2] = [Planted { row: 6, col: 8, side: 5.0 }, Planted { row: 6, col: 18, side: 5.0 }];
/// The networks' seeds.
const SEED: u32 = 2026;

struct Args {
    samples: usize,
    side: usize,
    auraface: bool,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a =
        Args { samples: if full { 21 } else { 1 }, side: if full { SIDE } else { 160 }, auraface: full, json: false, check: false, plant_ms: 0.0 };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--side" => a.side = value()?.parse().map_err(|e| format!("--side: {e}"))?,
            "--no-auraface" => a.auraface = false,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => {
                return Err(format!("unknown argument `{other}` (use --samples N, --side PX, --no-auraface, --check, --plant-ms N, --json)"));
            }
        }
    }
    if !(1..=1000).contains(&a.samples) {
        return Err("--samples must be 1..=1000".into());
    }
    if !(32..=2048).contains(&a.side) || !a.side.is_multiple_of(32) {
        return Err("--side must be a multiple of 32 in 32..=2048".into());
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

fn row(name: &str, times: &[Duration], budget: f64, shaped: &Shaped, load: Duration, extra: Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let mut r = json!({
        "name": name, "samples": t.len(), "p50_ms": rank(&t, 50), "p95_ms": rank(&t, 95), "max_ms": t.last().copied().unwrap_or(0.0),
        "budget_ms": budget, "weights": shaped.weights, "gmacs": shaped.macs as f64 / 1e9, "load_ms": ms(load),
    });
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

/// `shaped` written as an ONNX file and loaded back, and how long the load took.
fn load(shaped: &Shaped) -> (Model, Duration) {
    let bytes = synthetic::encode(&shaped.graph);
    let t = Instant::now();
    let model = Model::from_bytes(&bytes).expect("the network loads");
    (model, t.elapsed())
}

fn detect(a: &Args) -> Value {
    let shaped = synthetic::yunet_shaped(SEED, a.side, &PLANTED);
    let mut manifest = known::yunet();
    manifest.input.width = a.side as u32;
    manifest.input.height = a.side as u32;
    let (model, took) = load(&shaped);
    let detector = Detector::new(model, &manifest).expect("a detector of YuNet's shape");
    let (w, h) = (PICTURE.0 * a.side / SIDE, PICTURE.1 * a.side / SIDE);
    let pixels = probe_picture(w, h);
    let img = Rgb { data: &pixels, width: w, height: h };
    let opts = detector.options();
    let mut times = Vec::with_capacity(a.samples);
    let mut found = 0;
    for _ in 0..a.samples {
        let t = Instant::now();
        let faces = detector.detect(&img, &opts).expect("detection runs");
        times.push(t.elapsed());
        found = faces.len();
    }
    assert_eq!(found, PLANTED.len(), "the network reports the planted faces");
    row(&format!("detect {}", a.side), &times, DETECT_BUDGET_MS, &shaped, took, json!({ "faces": found, "picture_px": [w, h] }))
}

fn embed(a: &Args, name: &str, shaped: &Shaped, manifest: &dalica_faces::manifest::ModelManifest, budget: f64) -> Value {
    let (model, took) = load(shaped);
    let embedder = Embedder::new(model, manifest).expect("a recogniser of the candidate's shape");
    let (w, h) = embedder.input_size();
    let crop = probe_picture(w, h);
    let mut times = Vec::with_capacity(a.samples);
    for _ in 0..a.samples {
        let t = Instant::now();
        let v = embedder.embed(&crop).expect("one embedding");
        times.push(t.elapsed());
        assert_eq!(v.len(), embedder.dimension());
    }
    row(&format!("embed {name}"), &times, budget, shaped, took, json!({ "dim": embedder.dimension() }))
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("inference: {e}");
            std::process::exit(2);
        }
    };
    let mut rows = vec![detect(&a)];
    rows.push(embed(&a, "sface", &synthetic::sface_shaped(SEED), &known::sface(), SFACE_BUDGET_MS));
    if a.auraface {
        rows.push(embed(&a, "auraface", &synthetic::auraface_shaped(SEED), &known::auraface(), AURAFACE_BUDGET_MS));
    }
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "inference",
        "mode": mode,
        "threads": 1,
        "side": a.side,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<16} {:>8} {:>9} {:>9} {:>9} {:>11} {:>7}", "row", "samples", "p50 ms", "p95 ms", "max ms", "weights", "GMACs");
        for r in &rows {
            println!(
                "{:<16} {:>8} {:>9.3} {:>9.3} {:>9.3} {:>11} {:>7.3}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
                r["weights"].as_u64().unwrap_or(0),
                r["gmacs"].as_f64().unwrap_or(0.0),
            );
        }
    }
    if a.check && mode == "release" && a.side == SIDE && a.auraface {
        let mut missed = Vec::new();
        for r in &rows {
            let (p50, budget) = (r["p50_ms"].as_f64().unwrap_or(f64::NAN) + a.plant_ms, r["budget_ms"].as_f64().unwrap_or(0.0));
            if p50.is_nan() || p50 > budget {
                missed.push(format!("{}: median {p50:.2} ms, over the {budget} ms budget", r["name"].as_str().unwrap_or("?")));
            }
        }
        for m in &missed {
            eprintln!("inference: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
