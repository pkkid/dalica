//! The parts of M11's acceptance that run other things (`m11.rs` part 6): the crate suites behind
//! each *Done when* (skipped inside `cargo xtask ci`, whose test step ran them:
//! `dalica_acceptance::SUITES_RAN`), the workflow pass rate with its floor of 11, W07 and W10
//! among them (`cargo xtask stats --check`, skipped there too), `panic_hunt` over every command
//! M11 added, and M11's budgets (`development.md` §8): the `import`, `batch_edit` and `slideshow`
//! benches, in a debug build a smoke pass that checks the paths and prints their times, and with
//! `DALICA_ACCEPT_RELEASE=1` the release benches judged on the budgets `cargo xtask budgets` holds
//! them to (`xtask/src/bench.rs` → `judge_import`, `judge_batch_edit`, `judge_slideshow`).

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

/// The import's budgets and what they hold for (`xtask/src/bench.rs` → `IMPORT_FIRST_MS`,
/// `IMPORT_LIST_MS`, `IMPORT_FILES_PER_S`, `IMPORT_MB_PER_S`, `IMPORT_PHOTOS`, `IMPORT_SIDE_PX`).
const IMPORT_FIRST_MS: f64 = 35.0;
const IMPORT_LIST_MS: f64 = 320.0;
const IMPORT_FILES_PER_S: f64 = 22.9;
const IMPORT_MB_PER_S: f64 = 2.3;
const IMPORT_PHOTOS: u64 = 1_000;
const IMPORT_SIDE_PX: u64 = 2048;
/// Batch Edit's (`BATCH_APPLY_MS`, `BATCH_UNDO_MS`, `BATCH_PHOTOS`).
const BATCH_APPLY_MS: f64 = 7.0;
const BATCH_UNDO_MS: f64 = 5.0;
const BATCH_PHOTOS: u64 = 1_000;
/// The slideshow's (`SLIDESHOW_ADVANCE_MS`, `SLIDESHOW_TRANSITION_P95_MS`, `SLIDESHOW_FRAME_CAP_MS`,
/// `SLIDESHOW_PIXELS`).
const SLIDESHOW_ADVANCE_MS: f64 = 24.0;
const SLIDESHOW_TRANSITION_P95_MS: f64 = 0.3;
const SLIDESHOW_FRAME_CAP_MS: f64 = 16.0;
const SLIDESHOW_PIXELS: u64 = 6000 * 4000;

/// Runs `cargo <args>` (the release variable set only when `release`); panics with its output if
/// it fails. Returns its standard output and error.
fn cargo(args: &[&str], release: bool) -> String {
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

/// `cargo test -p <package> [--features …] --test <suite>…` in debug; panics if it fails or runs
/// nothing.
fn suites(package: &str, features: Option<&str>, list: &[&str]) {
    let mut args = vec!["test", "-p", package];
    if let Some(f) = features {
        args.extend(["--features", f]);
    }
    for s in list {
        args.extend(["--test", s]);
    }
    let out = cargo(&args, false);
    let passed: usize =
        out.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo {}: no test ran:\n{out}", args.join(" "));
    println!("  {passed} passed");
}

/// The crate suites behind M11's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Rooms and import:");
    println!("  cards and drives, listing a source; the import copy and removing a card's copied files (every fault point)");
    suites("dalica-scan", None, &["devices", "import_source"]);
    suites("dalica-fs", Some("faults"), &["import_copy", "remove"]);
    println!("  sources, listing and duplicates; importing, -001, undo; removal only after the question, never automated");
    suites("dalica-engine", None, &["import_list", "import_run", "import_remove"]);
    println!("  the room tabs, the Import room, its destination, importing, the question and the person's own Remove click");
    suites("dalica-ui", None, &["rooms", "import_room", "import_run_ui"]);
    println!("Compare and batch edit:");
    println!("  Batch Edit, Copy and Paste All Edits, Undo All Edits; two preview panes and A│A's variant, its tools");
    suites("dalica-engine", None, &["batch_edit", "preview_panes", "variant", "tuning_commands"]);
    suites("dalica-ui", None, &["compare", "compare_tools", "batch_ui"]);
    suites("dalica-mcp", None, &["edit_tools"]);
    println!("Slideshow and quick preview:");
    println!("  sequencing and transition geometry; what plays and the prefetch under a slowed decoder; the show, its bar, the quick preview");
    suites("dalica-create", None, &["slideshow"]);
    suites("dalica-engine", None, &["slideshow"]);
    suites("dalica-ui", None, &["slideshow", "slideshow_bar", "peek"]);
    suites("dalica-ui-kit", None, &["viewing_widgets"]);
    println!("Photo viewer:");
    println!("  the viewer's session; dalica --view on the real binary; the viewer in the window");
    suites("dalica-engine", None, &["viewer"]);
    suites("dalica", None, &["view"]);
    suites("dalica-ui", None, &["viewer"]);
}

/// `cargo xtask stats --check`: the workflow tests over MCP, their pass rate printed and the floor
/// (the eleven workflows M11 requires, W07 and W10 among them) held. Returns the report's first line.
pub fn workflows() -> String {
    let out = cargo(&["xtask", "stats", "--check"], false);
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("workflows:") || l.starts_with("  ") || l.starts_with("stats:")).collect();
    for l in &lines {
        println!("  {l}");
    }
    let head = lines.iter().find(|l| l.starts_with("workflows:")).map_or_else(|| panic!("stats printed no pass rate:\n{out}"), |l| l.to_string());
    assert!(head.contains("(0 failing"), "every workflow that runs passes: {head}");
    // The rows below the head name the workflows ignored until their milestone; W07 and W10 run.
    for w in ["w07", "w10"] {
        assert!(!lines.iter().any(|l| l.starts_with("  ") && l.contains(w)), "{w} runs and passes:\n{out}");
    }
    let passing: u64 = head.strip_prefix("workflows: ").and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!(passing >= 11, "the floor of 11: {head}");
    head
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M11's
/// included, hostile params, never a panic.
pub fn panic_hunt() {
    dalica_acceptance::panic_hunt().unwrap();
}

/// The JSON report a bench printed (its line starting with `{`).
fn report(out: &str) -> Value {
    out.lines()
        .rev()
        .find_map(|l| l.trim().starts_with('{').then(|| serde_json::from_str(l.trim()).ok()).flatten())
        .unwrap_or_else(|| panic!("no JSON report:\n{out}"))
}

/// The report's row named `name`.
fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["rows"].as_array().and_then(|r| r.iter().find(|r| r["name"] == name)).unwrap_or_else(|| panic!("no row `{name}`: {report}"))
}

fn num(r: &Value, key: &str) -> f64 {
    r[key].as_f64().unwrap_or_else(|| panic!("no `{key}`: {r}"))
}

/// What `import` measured: (photos, side, first screenful ms, listing ms, import s, files/s, MB/s).
fn import_numbers(r: &Value) -> (u64, u64, f64, f64, f64, f64, f64) {
    let n = r["photos"].as_u64().unwrap_or(0);
    let side = r["side_px"].as_u64().unwrap_or(0);
    let first = num(row(r, &format!("first thumbnails {n}")), "p50_ms");
    let list = num(row(r, &format!("list {n}")), "p50_ms");
    let import = row(r, &format!("import {n}"));
    let s = num(import, "p50_ms") / 1000.0;
    let bytes = num(import, "bytes");
    let s_pos = s.max(f64::MIN_POSITIVE);
    (n, side, first, list, s, n as f64 / s_pos, bytes / 1e6 / s_pos)
}

/// What `batch_edit` measured: (photos, Auto Fix median ms, its undo's median ms).
fn batch_numbers(r: &Value) -> (u64, f64, f64) {
    let n = r["photos"].as_u64().unwrap_or(0);
    (n, num(row(r, &format!("auto fix {n}")), "p50_ms"), num(row(r, &format!("undo auto fix {n}")), "p50_ms"))
}

/// What `slideshow` measured: (photo pixels, changes, waits, advance median ms, transition frame
/// p95 ms). Every change found its preview ready: no wait, whatever the build.
fn slideshow_numbers(r: &Value) -> (u64, f64, f64, f64, f64) {
    let px = r["photo_px"].as_array().map(|a| a.iter().filter_map(Value::as_u64).product::<u64>()).unwrap_or(0);
    let advance = row(r, "advance");
    let (changes, waits) = (num(advance, "changes"), num(advance, "waits"));
    assert!(changes >= 1.0 && waits == 0.0, "{waits} of {changes} changes waited for their preview (must be 0 of at least 1): {r}");
    (px, changes, waits, num(advance, "p50_ms"), num(row(r, "transition frame"), "p95_ms"))
}

/// The three benches' paths in a debug build (their smoke passes: 60 photos imported, Auto Fix on
/// 100, a show of three photos with one change per transition): every row measured and printed;
/// nothing judged but the slideshow's zero waits.
pub fn budgets_debug() {
    let r = report(&cargo(&["test", "-p", "dalica-engine", "--bench", "import", "--", "--json"], false));
    assert_eq!(r["mode"], "debug", "{r}");
    let (n, side, first, list, s, per_s, mb_s) = import_numbers(&r);
    println!(
        "  import (debug smoke): {n} photos of {side} px: first screenful {first:.1} ms, listed in {list:.0} ms, imported in {s:.2} s ({per_s:.0} files/s, {mb_s:.1} MB/s)"
    );
    let r = report(&cargo(&["test", "-p", "dalica-engine", "--bench", "batch_edit", "--", "--json"], false));
    assert_eq!(r["mode"], "debug", "{r}");
    let (n, apply, undo) = batch_numbers(&r);
    println!("  batch_edit (debug smoke): Auto Fix on {n} photos {apply:.2} ms, its undo {undo:.2} ms");
    let r = report(&cargo(&["test", "-p", "dalica-ui", "--bench", "slideshow", "--", "--json"], false));
    assert_eq!(r["mode"], "debug", "{r}");
    let (px, changes, waits, advance, p95) = slideshow_numbers(&r);
    println!(
        "  slideshow (debug smoke): photos of {px} pixels, {changes} changes, {waits} waits; advance {advance:.2} ms, transition frame p95 {p95:.2} ms"
    );
}

/// The three budgets in release (`DALICA_ACCEPT_RELEASE=1`), as `cargo xtask budgets` judges them.
/// Returns the lines to print.
pub fn budgets_release() -> Vec<String> {
    let r = report(&cargo(&["bench", "-p", "dalica-engine", "--bench", "import", "--", "--json"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let (n, side, first, list, s, per_s, mb_s) = import_numbers(&r);
    assert!(n >= IMPORT_PHOTOS && side >= IMPORT_SIDE_PX, "the budgets hold for {IMPORT_PHOTOS} photos of {IMPORT_SIDE_PX} px: {r}");
    assert!(first <= IMPORT_FIRST_MS, "first thumbnails: median {first:.1} ms, over the {IMPORT_FIRST_MS} ms budget");
    assert!(list <= IMPORT_LIST_MS, "the listing: median {list:.0} ms, over the {IMPORT_LIST_MS} ms budget");
    assert!(
        per_s >= IMPORT_FILES_PER_S && mb_s >= IMPORT_MB_PER_S,
        "the import: {per_s:.1} files/s, {mb_s:.2} MB/s, under the {IMPORT_FILES_PER_S} files/s and {IMPORT_MB_PER_S} MB/s budget"
    );
    let import = format!(
        "import: {n} photos of {side} px: first screenful {first:.1} ms ≤ {IMPORT_FIRST_MS} ms, listed in {list:.0} ms ≤ {IMPORT_LIST_MS} ms, imported in {s:.2} s: {per_s:.1} files/s ≥ {IMPORT_FILES_PER_S}, {mb_s:.2} MB/s ≥ {IMPORT_MB_PER_S}"
    );
    let r = report(&cargo(&["bench", "-p", "dalica-engine", "--bench", "batch_edit", "--", "--json"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let (n, apply, undo) = batch_numbers(&r);
    assert!(n >= BATCH_PHOTOS, "the budgets hold for {BATCH_PHOTOS} photos: {r}");
    assert!(apply <= BATCH_APPLY_MS, "Auto Fix on {n}: median {apply:.2} ms, over the {BATCH_APPLY_MS} ms budget");
    assert!(undo <= BATCH_UNDO_MS, "its undo: median {undo:.2} ms, over the {BATCH_UNDO_MS} ms budget");
    let batch = format!("batch edit: Auto Fix on {n} photos {apply:.2} ms ≤ {BATCH_APPLY_MS} ms, its undo {undo:.2} ms ≤ {BATCH_UNDO_MS} ms");
    let r = report(&cargo(&["bench", "-p", "dalica-ui", "--bench", "slideshow", "--", "--json"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let (px, changes, _, advance, p95) = slideshow_numbers(&r);
    assert!(px >= SLIDESHOW_PIXELS, "the budgets hold for photos of {SLIDESHOW_PIXELS} pixels: {r}");
    assert!(advance <= SLIDESHOW_ADVANCE_MS, "advance: median {advance:.2} ms, over the {SLIDESHOW_ADVANCE_MS} ms budget");
    let frame_budget = SLIDESHOW_TRANSITION_P95_MS.min(SLIDESHOW_FRAME_CAP_MS);
    assert!(p95 <= frame_budget, "transition frame: p95 {p95:.3} ms, over the {frame_budget} ms budget");
    let show = format!(
        "slideshow: {changes} changes of 24 MP photos, 0 waits; advance median {advance:.2} ms ≤ {SLIDESHOW_ADVANCE_MS} ms; transition frame p95 {p95:.3} ms ≤ {frame_budget} ms"
    );
    vec![import, batch, show]
}
