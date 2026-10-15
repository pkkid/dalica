//! The parts of M8's acceptance that run other things: the crate suites behind each *Done when*
//! (the per-format suite of M8.23 and the 200-photo mixed export of M8.24 among them),
//! `panic_hunt`, the quality estimate's error, and the `save_export` bench (a small debug pass that
//! prints its times; the 24 MP saves and the 200-photo export against their budgets in release with
//! `DALICA_ACCEPT_RELEASE=1`).

use std::time::Instant;

use dalica_image::encode::{self, Channels, Image, Metadata, Pixels, Subsampling};
use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

/// The budgets of `development.md` §8 the `save_export` bench is judged on, medians in
/// milliseconds (`crates/engine/benches/save_export.rs`, `xtask/src/accept.rs` → `BUDGETS`).
pub const SAVE_MS: f64 = 1050.0;
pub const ROTATE_SAVE_MS: f64 = 815.0;
pub const EXPORT_200_MS: f64 = 24000.0;

/// Runs `cargo <args>`; panics with its output if it fails. Returns its standard output and error.
/// A debug run never sees [`RELEASE`]; `release` sets it.
pub fn cargo(args: &[&str], release: bool) -> String {
    let started = Instant::now();
    let out = dalica_acceptance::cargo_split(args, |cmd| {
        if release {
            cmd.env(RELEASE, "1");
        } else {
            cmd.env_remove(RELEASE);
        }
    })
    .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "cargo {}:\n{stdout}\n{stderr}", args.join(" "));
    println!("cargo {}: ok ({:.1} s)", args.join(" "), started.elapsed().as_secs_f64());
    stdout + &stderr
}

/// Runs `cargo test <args>`; panics with its output if it fails or runs nothing.
pub fn cargo_test(args: &[&str]) -> String {
    let all: Vec<&str> = ["test"].iter().chain(args).copied().collect();
    let out = cargo(&all, false);
    let passed: usize =
        out.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo {}: no test ran:\n{out}", all.join(" "));
    println!("  {passed} passed");
    out
}

/// `cargo test -p <package> --test <suite>…` in debug.
fn suites(package: &str, list: &[&str]) -> String {
    let mut args = vec!["-p", package];
    for s in list {
        args.extend(["--test", s]);
    }
    cargo_test(&args)
}

/// The crate suites behind M8's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Save to disk:");
    println!("  the model's saved record and Exported mark, the originals backup and new files, the record on disk");
    suites("dalica-library", &["m8_fields", "ops", "tree"]);
    suites("dalica-fs", &["originals", "create"]);
    suites("dalica-sidecar", &["originals"]);
    suites("dalica-store", &["m8_fields", "fields"]);
    suites("dalica-scan", &["originals"]);
    println!("  the encoders, the quality estimate, lossless JPEG transforms, metadata carried to a new file");
    suites("dalica-image", &["encode", "jpeg_quality", "jpeg_transform"]);
    suites("dalica-meta", &["carry"]);
    println!("  Save in place, Undo Save, Revert, lossless and beside saves, copies; a fault at every step of a Save");
    suites("dalica-create", &["output", "save", "save_lossless", "save_beside", "save_faults"]);
    println!("  the commands, the file-job runner, settings, repair of an interrupted Save");
    suites("dalica-engine", &["save", "save_copy", "save_recovery", "filejobs", "settings_m8"]);
    println!("  every format of formats.md §1 saves as specified (M8.23)");
    suites("dalica-engine", &["save_formats"]);
    println!("  the header's count and Save, the explanation, Undo Save, the Revert question, Save As");
    suites("dalica-ui", &["save_ui", "save_as", "section_header", "view_state_m8"]);
    suites("dalica-ui-kit", &["form_widgets", "icons_output"]);
    println!("Export:");
    println!("  planning, files, the watermark; the command and its job; the dialog, the progress window, Projects");
    suites("dalica-create", &["export", "watermark"]);
    suites("dalica-engine", &["export"]);
    suites("dalica-ui", &["export_dialog", "export_progress", "tree_projects", "notices"]);
    println!("  a 200-photo mixed tray exported whole, and a Cancel half-way leaving only complete files (M8.24)");
    let out = suites("dalica-engine", &["export_mixed", "--", "--nocapture"]);
    if let Some(line) = out.lines().find(|l| l.starts_with("mixed export:")) {
        println!("  {line}");
    }
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M8's included, hostile params, never a panic.
pub fn panic_hunt() {
    dalica_acceptance::panic_hunt().unwrap();
}

/// The quality estimate's error (`data-model.md` §6, Save's quality matching): JPEGs of known
/// quality written by `scenes` (another encoder) and by our own, at 4:2:0 and 4:4:4, read back
/// from their quantisation tables. Returns the largest error and how many files were read.
pub fn quality_estimate_error() -> (u8, usize) {
    let scene = dalica_scenes::Scene::new(8, 256, 192);
    let rgb = scene.render().unwrap();
    let image = Image { width: rgb.width, height: rgb.height, channels: Channels::Rgb, pixels: Pixels::U8(&rgb.data) };
    let (mut worst, mut files) = (0u8, 0usize);
    for q in [30u8, 50, 60, 70, 75, 80, 85, 90, 92, 95, 98] {
        let theirs = dalica_scenes::jpeg(&scene, q, None).unwrap();
        let ours = [Subsampling::S420, Subsampling::S444].map(|s| encode::jpeg(&image, q, s, &Metadata::default()).unwrap());
        for bytes in std::iter::once(&theirs).chain(&ours) {
            let estimate = dalica_image::jpeg_quality::quality(bytes).unwrap_or_else(|| panic!("quality {q}: no estimate"));
            worst = worst.max(estimate.abs_diff(q));
            files += 1;
        }
    }
    assert!(worst <= 2, "the quality estimate is within 2: {worst}");
    (worst, files)
}

/// The JSON report a bench printed (its line starting with `{`).
fn report(out: &str) -> Value {
    out.lines()
        .rev()
        .find_map(|l| l.trim().starts_with('{').then(|| serde_json::from_str(l.trim()).ok()).flatten())
        .unwrap_or_else(|| panic!("no JSON report:\n{out}"))
}

fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["rows"].as_array().and_then(|r| r.iter().find(|r| r["name"] == name)).unwrap_or_else(|| panic!("no row `{name}`: {report}"))
}

fn ms(r: &Value, key: &str) -> f64 {
    r[key].as_f64().unwrap_or_else(|| panic!("no `{key}`: {r}"))
}

/// The bench's rows as lines, with the budget each is judged on; the three medians.
fn print_rows(report: &Value) -> [f64; 3] {
    let (save, rotate, export) = (row(report, "save"), row(report, "save rotate-only"), row(report, "export 200 mixed"));
    let judged = [ms(save, "p50_ms"), ms(rotate, "p50_ms"), ms(export, "p50_ms")];
    println!(
        "save_export ({}, {} threads, {} × {} JPEG saves, the tray at scale {}):",
        report["mode"].as_str().unwrap_or("?"),
        report["threads"],
        report["width"],
        report["height"],
        report["scale"]
    );
    println!("  save              p50 {:8.1} ms  max {:8.1} ms  (budget {SAVE_MS} ms on p50)", judged[0], ms(save, "max_ms"));
    println!("  save rotate-only  p50 {:8.1} ms  max {:8.1} ms  (budget {ROTATE_SAVE_MS} ms on p50)", judged[1], ms(rotate, "max_ms"));
    println!(
        "  export 200 mixed  p50 {:8.1} ms  max {:8.1} ms  ({:.1} ms per photo; budget {EXPORT_200_MS} ms on p50)",
        judged[2],
        ms(export, "max_ms"),
        ms(export, "per_photo_ms")
    );
    judged
}

/// The `save_export` bench's path in a debug build (its smoke pass: 600 × 400 saves and the tray
/// at scale 1, one sample each): every row measured and printed, nothing judged (the times of an
/// unoptimised build say nothing about the budgets).
pub fn save_export_debug() {
    let out = cargo(&["test", "-p", "dalica-engine", "--bench", "save_export", "--", "--json"], false);
    let r = report(&out);
    assert_eq!(r["mode"], "debug", "{r}");
    let judged = print_rows(&r);
    assert!(judged.iter().all(|t| t.is_finite() && *t > 0.0), "every row measured: {judged:?}");
}

/// The budgets in release (`DALICA_ACCEPT_RELEASE=1`): the `save_export` bench in full (a 24 MP
/// JPEG saved with a Tuning edit and with a quarter turn, seven times each; the 200-photo mixed
/// tray at scale 5 exported three times), each median within its budget. Returns the medians.
pub fn release_budgets() -> [f64; 3] {
    let out = cargo(&["bench", "-p", "dalica-engine", "--bench", "save_export", "--", "--json"], true);
    let r = report(&out);
    assert_eq!((r["mode"].as_str(), r["width"].as_u64(), r["height"].as_u64()), (Some("release"), Some(6000), Some(4000)), "{r}");
    let judged = print_rows(&r);
    for (what, t, budget) in
        [("save", judged[0], SAVE_MS), ("save rotate-only", judged[1], ROTATE_SAVE_MS), ("export 200 mixed", judged[2], EXPORT_200_MS)]
    {
        assert!(t <= budget, "{what}: median {t:.1} ms, over the {budget} ms budget");
    }
    judged
}
