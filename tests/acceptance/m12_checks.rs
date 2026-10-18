//! The parts of M12's acceptance that run other things (`m12.rs` part 7): the crate suites behind
//! each *Done when* (skipped inside `cargo xtask ci`, whose test step ran them:
//! `dalica_acceptance::SUITES_RAN`), the workflow pass rate with its floor of 12, W11 among them
//! (`cargo xtask stats --check`, skipped there too), `panic_hunt` over every command M12 added, and
//! M12's budgets (`development.md` §8, M12.21): the `effects`, `effect_tiles` and `slideshow`
//! benches, in a debug build a smoke pass that checks the paths and prints their times, and with
//! `DALICA_ACCEPT_RELEASE=1` the release benches judged on the budgets `cargo xtask budgets` holds
//! them to (`xtask/src/bench.rs` → `judge_effects`, `judge_effect_tiles`, `judge_slideshow`'s
//! upload frame). The effects and the tiles are judged by their benches' own `--check` against the
//! budgets each report row carries (`budget_ms`, `budget_p95_ms`, the tables `xtask` keeps in step
//! with them), and here again from the rows; the slideshow's upload frame on its budget below. A
//! slider's draft and final per effect are printed, not judged: their 16 ms and 100 ms are M13's
//! speed tasks (the maintainer at M12.R).

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

/// The slideshow's upload frame (`xtask/src/bench.rs` → `SLIDESHOW_UPLOAD_P95_MS`, never above
/// `SLIDESHOW_FRAME_CAP_MS`).
const SLIDESHOW_UPLOAD_P95_MS: f64 = 6.8;
const SLIDESHOW_FRAME_CAP_MS: f64 = 16.0;
const SLIDESHOW_PIXELS: u64 = 6000 * 4000;
/// The photo and the screen the effects' budgets hold for (`EFFECT_PHOTO`, `EFFECT_SCREEN`).
const EFFECT_PHOTO: u64 = 6000 * 4000;
const EFFECT_SCREEN: [u64; 2] = [2560, 1600];
/// The 27 effects.
const EFFECTS: u64 = 27;

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

/// The crate suites behind M12's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Effect tiles and panels:");
    println!("  the catalogue and the recipe syntax; colours; the tiles, slider, swatches, picker and crosshair widgets and their icons");
    suites("dalica-edit", None, &["catalog", "recipe", "fx"]);
    suites("dalica-core", None, &["color"]);
    suites("dalica-ui-kit", None, &["effect_widgets", "icons_effects"]);
    println!("  effect.list, the tiles (no fs call on the owner thread), the colour pick; Batch Edit's five effects");
    suites("dalica-engine", None, &["effects", "batch_edit"]);
    println!("  the grids, the panels, the crosshair, Soften's brush, Batch Edit's menu, edited cells at a frame's size");
    suites("dalica-ui", None, &["effect_tiles", "effect_panel", "effect_crosshair", "soften_brush", "batch_ui", "edited_cells"]);
    println!("Tab 3 effects: a fingerprint and an invariant per effect, the same at two render sizes and in a zoomed region");
    suites("dalica-edit", None, &["colour_effects", "detail_effects", "focal_effects"]);
    println!("Tab 4 and 5 effects: as tab 3; frames growing the picture, Soften's paint mask");
    suites("dalica-edit", None, &["remap_effects", "look_effects", "soften", "stylize_effects", "canvas", "mats"]);
}

/// `cargo xtask stats --check`: the workflow tests over MCP, their pass rate printed and the floor
/// (twelve, W11 among them) held. Returns the report's first line.
pub fn workflows() -> String {
    let out = cargo(&["xtask", "stats", "--check"], false);
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("workflows:") || l.starts_with("  ") || l.starts_with("stats:")).collect();
    for l in &lines {
        println!("  {l}");
    }
    let head = lines.iter().find(|l| l.starts_with("workflows:")).map_or_else(|| panic!("stats printed no pass rate:\n{out}"), |l| l.to_string());
    assert!(head.contains("(0 failing"), "every workflow that runs passes: {head}");
    assert!(!lines.iter().any(|l| l.starts_with("  ") && l.contains("w11")), "w11 runs and passes:\n{out}");
    let passing: u64 = head.strip_prefix("workflows: ").and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!(passing >= 12, "the floor of 12: {head}");
    head
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M12's
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

fn rows(report: &Value) -> &[Value] {
    report["rows"].as_array().map_or(&[], Vec::as_slice)
}

/// The report's row named `name`.
fn row<'a>(report: &'a Value, name: &str) -> &'a Value {
    rows(report).iter().find(|r| r["name"] == name).unwrap_or_else(|| panic!("no row `{name}`: {report}"))
}

fn num(r: &Value, key: &str) -> f64 {
    r[key].as_f64().unwrap_or_else(|| panic!("no `{key}`: {r}"))
}

fn size(report: &Value, key: &str) -> [u64; 2] {
    let v: Vec<u64> = report[key].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    [v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0)]
}

/// What `effects` measured: per effect `(name, draft p95, final p95, save p50, save budget)`, and the
/// empty recipe's `(final p95, its budget, save p50, its budget)`.
#[allow(clippy::type_complexity)]
fn effect_numbers(r: &Value) -> (Vec<(String, f64, f64, f64, Option<f64>)>, (f64, Option<f64>, f64, Option<f64>)) {
    let mut out = Vec::new();
    for s in rows(r).iter().filter(|s| s["name"].as_str().is_some_and(|n| n.ends_with(" save") && n != "no effect save")) {
        let name = s["name"].as_str().unwrap().trim_end_matches(" save").to_string();
        let draft = num(row(r, &format!("{name} draft")), "p95_ms");
        let fin = num(row(r, &format!("{name} final")), "p95_ms");
        out.push((name, draft, fin, num(s, "p50_ms"), s["budget_ms"].as_f64()));
    }
    let f = row(r, "no effect final");
    let s = row(r, "no effect save");
    (out, (num(f, "p95_ms"), f["budget_p95_ms"].as_f64(), num(s, "p50_ms"), s["budget_ms"].as_f64()))
}

/// What `effect_tiles` measured: per tab `(tab, first tile p50, last tile p50, budget)`.
fn tile_numbers(r: &Value) -> Vec<(u8, f64, f64, Option<f64>)> {
    [3u8, 4, 5]
        .iter()
        .map(|t| {
            let w = row(r, &format!("tab {t} tiles"));
            (*t, w["first_p50_ms"].as_f64().unwrap_or(f64::NAN), num(w, "p50_ms"), w["budget_ms"].as_f64())
        })
        .collect()
}

/// The three benches' paths in a debug build (their smoke passes: the 27 effects on a 600 × 400
/// photo, each tab's tiles of one, a show of three photos): every row measured and printed; nothing
/// judged.
pub fn budgets_debug() {
    let r = report(&cargo(&["test", "-p", "dalica-edit", "--bench", "effects", "--", "--json"], false));
    assert_eq!((r["mode"].clone(), r["effects"].as_u64()), (Value::from("debug"), Some(EFFECTS)), "{r}");
    let (effects, (fin, _, save, _)) = effect_numbers(&r);
    assert_eq!(effects.len() as u64, EFFECTS, "a save row per effect: {r}");
    let slowest = effects.iter().map(|e| e.3).fold(0.0, f64::max);
    let p = size(&r, "photo_px");
    println!(
        "  effects (debug smoke): the 27 effects on a {} × {} photo, the slowest save {slowest:.0} ms; no effect: final {fin:.1} ms, save {save:.1} ms",
        p[0], p[1]
    );
    let r = report(&cargo(&["test", "-p", "dalica-engine", "--bench", "effect_tiles", "--", "--json"], false));
    assert_eq!(r["mode"], "debug", "{r}");
    let tiles: Vec<String> = tile_numbers(&r).iter().map(|(t, first, last, _)| format!("tab {t} {first:.0}/{last:.0} ms")).collect();
    println!("  effect_tiles (debug smoke): first/last tile {}", tiles.join(", "));
    let r = report(&cargo(&["test", "-p", "dalica-ui", "--bench", "slideshow", "--", "--json"], false));
    assert_eq!(r["mode"], "debug", "{r}");
    println!("  slideshow (debug smoke): upload frame p95 {:.2} ms", num(row(&r, "upload frame"), "p95_ms"));
}

/// The three budgets in release (`DALICA_ACCEPT_RELEASE=1`), as `cargo xtask budgets` judges them.
/// Returns the lines to print.
pub fn budgets_release() -> Vec<String> {
    let mut lines = Vec::new();
    let r = report(&cargo(&["bench", "-p", "dalica-edit", "--bench", "effects", "--", "--json", "--check"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let (photo, fit) = (size(&r, "photo_px"), size(&r, "fit"));
    assert!(photo[0] * photo[1] >= EFFECT_PHOTO && fit == EFFECT_SCREEN, "the budgets hold for 6000 × 4000 on 2560 × 1600: {r}");
    let (effects, (fin, fin_budget, save, save_budget)) = effect_numbers(&r);
    assert_eq!(effects.len() as u64, EFFECTS, "every effect measured: {r}");
    let mut worst = (String::new(), 0.0f64);
    for (name, _, _, s, budget) in &effects {
        let budget = budget.unwrap_or_else(|| panic!("{name}: its save row carries its budget"));
        assert!(*s <= budget, "{name}: Save median {s:.1} ms, over its {budget} ms budget");
        if s / budget > worst.1 {
            worst = (name.clone(), s / budget);
        }
    }
    let (fin_budget, save_budget) = (fin_budget.expect("the empty final's budget"), save_budget.expect("the empty save's budget"));
    assert!(fin <= fin_budget, "no effect: final p95 {fin:.2} ms, over the {fin_budget} ms budget");
    assert!(save <= save_budget, "no effect: Save median {save:.2} ms, over the {save_budget} ms budget");
    lines.push(format!(
        "effects: the 27 effects' 24 MP Saves within their budgets, the closest {} at {:.0} %; no effect: final p95 {fin:.2} ms ≤ {fin_budget} ms, Save {save:.1} ms ≤ {save_budget} ms",
        worst.0,
        worst.1 * 100.0
    ));
    let over_draft = effects.iter().filter(|e| e.1 > 16.0).count();
    let over_final = effects.iter().filter(|e| e.2 > 100.0).count();
    lines.push(format!("effects (printed, M13's speed tasks): {over_draft} drafts over 16 ms and {over_final} finals over 100 ms at p95"));
    for (name, d, f, s, b) in &effects {
        lines.push(format!("  {name:<16} draft p95 {d:7.2} ms, final p95 {f:7.2} ms, Save {s:8.1} ms (budget {})", b.unwrap_or(f64::NAN)));
    }

    let r = report(&cargo(&["bench", "-p", "dalica-engine", "--bench", "effect_tiles", "--", "--json", "--check"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let photo = size(&r, "photo_px");
    assert!(photo[0] * photo[1] >= EFFECT_PHOTO, "the tiles' budgets hold for 6000 × 4000: {r}");
    let mut parts = Vec::new();
    for (t, _, last, budget) in tile_numbers(&r) {
        let budget = budget.unwrap_or_else(|| panic!("tab {t}: its row carries its budget"));
        assert!(last <= budget, "tab {t} tiles: median {last:.1} ms, over the {budget} ms budget");
        parts.push(format!("tab {t} {last:.1} ms ≤ {budget} ms"));
    }
    lines.push(format!("effect tiles of a 24 MP photo, to the last tile: {}", parts.join("; ")));

    let r = report(&cargo(&["bench", "-p", "dalica-ui", "--bench", "slideshow", "--", "--json"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let px = r["photo_px"].as_array().map(|a| a.iter().filter_map(Value::as_u64).product::<u64>()).unwrap_or(0);
    assert!(px >= SLIDESHOW_PIXELS, "the budget holds for photos of {SLIDESHOW_PIXELS} pixels: {r}");
    let upload = num(row(&r, "upload frame"), "p95_ms");
    let budget = SLIDESHOW_UPLOAD_P95_MS.min(SLIDESHOW_FRAME_CAP_MS);
    assert!(upload <= budget, "the slideshow's upload frame: p95 {upload:.2} ms, over the {budget} ms budget");
    lines.push(format!("slideshow: the upload frame of 24 MP photos p95 {upload:.2} ms ≤ {budget} ms"));
    lines
}
