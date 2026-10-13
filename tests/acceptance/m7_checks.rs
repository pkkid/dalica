//! The parts of M7's acceptance that run other things: the crate suites behind each *Done when*,
//! `panic_hunt`, and the `edit_preview` bench (a small debug pass that prints its times; the 24 MP
//! budgets in release with `DALICA_ACCEPT_RELEASE=1`).

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

/// The budgets of `development.md` §8 the `edit_preview` bench judges (`crates/engine/benches/
/// edit_preview.rs`), in milliseconds.
pub const OPEN_MS: f64 = 150.0;
pub const DRAFT_MS: f64 = 16.0;
pub const FINAL_MS: f64 = 100.0;
pub const STEP_MS: f64 = 50.0;

/// Runs `cargo <args>`; panics with its output if it fails. Returns its standard output. A debug
/// run never sees [`RELEASE`] (`tuning_tab` would then insist on a release build); `release` sets it.
pub fn cargo(args: &[&str], release: bool) -> String {
    let started = Instant::now();
    let mut cmd = dalica_acceptance::cargo();
    cmd.args(args);
    if release {
        cmd.env(RELEASE, "1");
    } else {
        cmd.env_remove(RELEASE);
    }
    let out = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "cargo {}:\n{stdout}\n{stderr}", args.join(" "));
    println!("cargo {}: ok ({:.1} s)", args.join(" "), started.elapsed().as_secs_f64());
    stdout + &stderr
}

/// Runs `cargo test <args>`; panics with its output if it fails or runs nothing. Returns its
/// output.
pub fn cargo_test(args: &[&str], release: bool) -> String {
    let all: Vec<&str> = ["test"].iter().chain(args).copied().collect();
    let out = cargo(&all, release);
    let passed: usize =
        out.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo {}: no test ran:\n{out}", all.join(" "));
    println!("  {passed} passed");
    out
}

/// `cargo test -p <package> --test <suite>…` in debug.
fn suites(package: &str, list: &[&str]) {
    let mut args = vec!["-p", package];
    for s in list {
        args.extend(["--test", s]);
    }
    cargo_test(&args, false);
}

/// The crate suites behind M7's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Edit view shell:");
    println!("  the coordinate frame, the photo's history (restart and rebuild), M7's settings");
    suites("dalica-edit", &["frame", "recipe", "geometry"]);
    suites("dalica-engine", &["photo_history", "settings_m7"]);
    println!("  the view's state and frame, entering, leaving and navigation, zoom maths and the canvas");
    suites("dalica-ui", &["view_state_m7", "edit_navigation", "zoom_maths", "edit_canvas", "edit_panel"]);
    suites("dalica-ui-kit", &["edit_widgets", "input", "icons_edit"]);
    println!("  the preview renders (draft before final, refined window, prefetch, histogram, cancel)");
    suites("dalica-engine", &["preview"]);
    println!("Basic fixes:");
    println!("  the renderer, pipeline order, versions; fingerprints and two sizes per operation");
    suites("dalica-edit", &["render", "crop", "straighten", "auto", "suggest"]);
    println!("  thumbnails = edit::render of the recipe; the edited cells");
    suites("dalica-thumbs", &["recipe"]);
    suites("dalica-engine", &["edited_thumbs", "crop_commands"]);
    suites("dalica-ui", &["edited_cells", "crop_tool", "straighten_tool"]);
    println!("Tuning and retouch:");
    println!("  Tuning and Retouch: fingerprints per control and per operation, two sizes, one step per session");
    suites("dalica-edit", &["tone", "retouch"]);
    suites("dalica-engine", &["tuning_commands"]);
    suites("dalica-ui", &["tuning_tab", "retouch_tool", "widget_ids"]);
}

/// `panic_hunt -- --ignored`: every command, M7's included, hostile params, never a panic.
pub fn panic_hunt() {
    cargo_test(&["-p", "dalica-engine", "--test", "panic_hunt", "--", "--ignored"], false);
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

/// The bench's rows as lines, with the budget each is judged on.
fn print_rows(report: &Value) -> [f64; 5] {
    let (cold, cached, drag, steps) = (row(report, "open cold"), row(report, "open cached"), row(report, "drag"), row(report, "next previous"));
    let judged = [ms(cold, "p50_ms"), ms(cached, "p50_ms"), ms(drag, "p95_draft_ms"), ms(drag, "p95_final_ms"), ms(steps, "p95_ms")];
    println!(
        "edit_preview ({}, {} × {} JPEGs fitted to {}):",
        report["mode"].as_str().unwrap_or("?"),
        report["width"],
        report["height"],
        report["fit"]
    );
    println!("  open cold     p50 {:7.1} ms  max {:7.1} ms  (budget {OPEN_MS} ms on p50)", judged[0], ms(cold, "max_ms"));
    println!("  open cached   p50 {:7.1} ms  max {:7.1} ms  (budget {OPEN_MS} ms on p50)", judged[1], ms(cached, "max_ms"));
    println!("  drag draft    p95 {:7.1} ms  max {:7.1} ms  (budget {DRAFT_MS} ms on p95)", judged[2], ms(drag, "max_draft_ms"));
    println!("  drag final    p95 {:7.1} ms  max {:7.1} ms  (budget {FINAL_MS} ms on p95)", judged[3], ms(drag, "max_final_ms"));
    println!("  next/previous p95 {:7.1} ms  max {:7.1} ms  (budget {STEP_MS} ms on p95)", judged[4], ms(steps, "max_ms"));
    judged
}

/// The `edit_preview` bench's path in a debug build on smaller photos: every row measured and
/// printed, nothing judged (the times of an unoptimised renderer say nothing about the budgets).
pub fn preview_bench_debug() {
    let out = cargo(
        &["test", "-p", "dalica-engine", "--bench", "edit_preview", "--", "--json", "--width", "1200", "--height", "800", "--fit", "640x400"],
        false,
    );
    let r = report(&out);
    assert_eq!(r["mode"], "debug", "{r}");
    let judged = print_rows(&r);
    assert!(judged.iter().all(|t| t.is_finite() && *t > 0.0), "every row measured: {judged:?}");
}

/// The budgets in release (`DALICA_ACCEPT_RELEASE=1`): the `edit_preview` bench in full (24 MP
/// JPEGs, the preview fitted to 2560 × 1600; it exits non-zero on a missed budget and is judged here
/// again), and `tuning_tab`'s Fill Light drag in a release build, whose frames' and drafts' p95 it
/// asserts within 16 ms.
pub fn release_budgets() {
    let out = cargo(&["bench", "-p", "dalica-engine", "--bench", "edit_preview", "--", "--json"], true);
    let r = report(&out);
    assert_eq!((r["mode"].as_str(), r["width"].as_u64(), r["height"].as_u64()), (Some("release"), Some(6000), Some(4000)), "{r}");
    let judged = print_rows(&r);
    for (what, t, budget) in [
        ("open cold", judged[0], OPEN_MS),
        ("open cached", judged[1], OPEN_MS),
        ("drag draft", judged[2], DRAFT_MS),
        ("drag final", judged[3], FINAL_MS),
        ("next/previous", judged[4], STEP_MS),
    ] {
        assert!(t <= budget, "{what}: {t:.1} ms, over the {budget} ms budget");
    }
    let out = cargo(&["test", "--release", "-p", "dalica-ui", "--test", "tuning_tab", "--", "--nocapture"], true);
    let line = out.lines().find(|l| l.starts_with("drag: frames p95")).unwrap_or_else(|| panic!("tuning_tab printed no drag line:\n{out}"));
    assert!(out.contains("test result: ok."), "{out}");
    println!("tuning_tab (release, the Fill Light drag in the UI, each p95 asserted ≤ {DRAFT_MS} ms): {line}");
}
