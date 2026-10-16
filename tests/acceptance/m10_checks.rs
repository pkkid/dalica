//! The parts of M10's acceptance that run other things (`m10.rs` part 4): the crate suites behind
//! each *Done when* (the keymap conflict tests among them), the workflow pass rate and its floor
//! (`cargo xtask stats --check`), `cargo xtask parity --p0`, `panic_hunt`, and the P0 budgets M10
//! added: `scan_throughput` and `cold_start` (idle memory), in a debug build a smoke pass that
//! checks the path and prints its times, and with `DALICA_ACCEPT_RELEASE=1` the release benches
//! judged on the budgets `cargo xtask budgets` holds them to (`development.md` §8). The budgets of
//! the earlier P0 scenarios are asserted by their milestones' acceptance (M4, M5, M8, M9) and all
//! together by `cargo xtask budgets` at the review.

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

/// The scan's budget, photos per second until every embedded thumbnail is available, and the
/// photos it holds for (`xtask/src/accept.rs` → `SCAN_PHOTOS_PER_S`, `SCAN_PHOTOS`).
pub const SCAN_PHOTOS_PER_S: f64 = 500.0;
pub const SCAN_PHOTOS: u64 = 5_000;
/// The cold start's budget on the median, idle memory's in every run, and the library they hold
/// for (`xtask/src/accept.rs` → `COLD_START_MS`, `IDLE_MIB`, `COLD_START_PHOTOS`).
pub const COLD_START_MS: f64 = 2000.0;
pub const IDLE_MIB: f64 = 600.0;
pub const COLD_START_PHOTOS: u64 = 100_000;

/// Runs `cargo <args>` (the release variable set only when `release`); panics with its output if
/// it fails. Returns its standard output and error.
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

/// `cargo test -p <package> --test <suite>…` in debug; panics if it fails or runs nothing.
fn suites(package: &str, list: &[&str]) {
    let mut args = vec!["test", "-p", package];
    for s in list {
        args.extend(["--test", s]);
    }
    let out = cargo(&args, false);
    let passed: usize =
        out.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo {}: no test ran:\n{out}", args.join(" "));
    println!("  {passed} passed");
}

/// The crate suites behind M10's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Preferences and shortcuts:");
    println!("  options.list and options.set with unknown keys kept, File Types keeping data, the keymap's conflicts, Star on F8, Reset");
    suites("dalica-engine", &["options_set", "file_types", "keymap"]);
    suites("dalica-store", &["stale"]);
    println!("  the Options dialog (OK, Cancel, Apply), UI scale and font size, File Types and Export tabs, the shortcut editor");
    suites("dalica-ui", &["options_dialog", "ui_scale", "options_tabs", "shortcut_editor"]);
    println!("MCP:");
    println!("  library.inspect on three surfaces; batch and replies that wait");
    suites("dalica-engine", &["library_inspect"]);
    suites("dalica-ui", &["control_inspect", "control_batch", "control_wait"]);
    println!("  the server, its tools, resources, roots and permissions, connect mode against a fake control server");
    suites("dalica-mcp", &["all"]);
    println!("  dalica-cli mcp on real stdio, the stdio smoke run of W02 and W03, dalica-cli run --connect");
    suites("dalica-cli", &["mcp_cli", "mcp_process", "run_connect"]);
    println!("Release 0.5:");
    println!("  the P0 formats measured");
    suites("dalica-image", &["p0_formats"]);
}

/// `cargo xtask stats --check`: the workflow tests over MCP, their pass rate printed and the floor
/// (the nine P0 workflows) held. Returns the report's first line (`workflows: 9 / 12 passing …`).
pub fn workflows() -> String {
    let out = cargo(&["xtask", "stats", "--check"], false);
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("workflows:") || l.starts_with("  ") || l.starts_with("stats:")).collect();
    for l in &lines {
        println!("  {l}");
    }
    let head = lines.iter().find(|l| l.starts_with("workflows:")).map_or_else(|| panic!("stats printed no pass rate:\n{out}"), |l| l.to_string());
    assert!(head.contains("(0 failing"), "every workflow that runs passes: {head}");
    head
}

/// `cargo xtask parity --p0`: every P0 row ✅, but the rows awaiting the M10.R manual check.
/// Returns its last line.
pub fn parity_p0() -> String {
    let out = cargo(&["xtask", "parity", "--p0"], false);
    let tail = out.lines().rev().find(|l| l.starts_with("parity --p0")).unwrap_or_default().to_string();
    for l in out.lines().filter(|l| l.starts_with("  ") || l.starts_with("awaiting") || l.starts_with("parity --p0")) {
        println!("  {l}");
    }
    tail
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M10's
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

/// The report's row whose name starts with `prefix` (`"scan "`, `"cold start "`).
fn row<'a>(report: &'a Value, prefix: &str) -> &'a Value {
    report["rows"]
        .as_array()
        .and_then(|r| r.iter().find(|r| r["name"].as_str().is_some_and(|n| n.starts_with(prefix))))
        .unwrap_or_else(|| panic!("no row `{prefix}…`: {report}"))
}

fn num(r: &Value, key: &str) -> f64 {
    r[key].as_f64().unwrap_or_else(|| panic!("no `{key}`: {r}"))
}

/// What `scan_throughput` measured: (photos, seconds to the embedded thumbnails, photos per second,
/// seconds to every thumbnail).
fn scan_numbers(r: &Value) -> (f64, f64, f64, f64) {
    let row = row(r, "scan ");
    let (n, scan, embedded) = (num(row, "photos"), num(row, "scan_s"), num(row, "embedded"));
    assert_eq!(embedded, n, "every photo has its embedded thumbnail: {row}");
    (n, scan, n / scan.max(f64::MIN_POSITIVE), num(row, "thumbs_s"))
}

/// What `cold_start` measured: (photos, median cold start ms, slowest ms, largest idle MiB).
fn cold_numbers(r: &Value) -> (u64, f64, f64, Option<f64>) {
    let photos = r["photos"].as_u64().unwrap_or(0);
    let (start, memory) = (row(r, "cold start "), row(r, "idle memory "));
    (photos, num(start, "p50_ms"), num(start, "max_ms"), memory["rss_mib"].as_f64())
}

/// The two benches' paths in a debug build (their smoke passes: 200 photos scanned; one cold start
/// over 2 000 photos with a short idle): every row measured and printed, nothing judged.
pub fn budgets_debug() {
    let r = report(&cargo(&["test", "-p", "dalica-engine", "--bench", "scan_throughput", "--", "--json"], false));
    assert_eq!(r["mode"], "debug", "{r}");
    let (n, scan, per_s, thumbs) = scan_numbers(&r);
    println!("  scan_throughput (debug smoke): {n} photos in {scan:.2} s, {per_s:.0} photos/s; every thumbnail after {thumbs:.1} s");
    let r = report(&cargo(&["test", "-p", "dalica-ui", "--bench", "cold_start", "--", "--json"], false));
    assert_eq!(r["mode"], "debug", "{r}");
    let (photos, p50, max, rss) = cold_numbers(&r);
    let rss = rss.map_or_else(|| "not measured here".to_string(), |m| format!("{m:.0} MiB"));
    println!("  cold_start (debug smoke): {photos} photos, cold start {p50:.0} ms (slowest {max:.0} ms), idle memory {rss}");
}

/// The two budgets in release (`DALICA_ACCEPT_RELEASE=1`), as `cargo xtask budgets` judges them:
/// 5 000 JPEGs scanned at ≥ 500 photos per second; the 100 k library's median cold start ≤ 2 s and
/// its idle memory ≤ 600 MiB in every run (read on Linux and macOS). Returns the lines to print.
pub fn budgets_release() -> Vec<String> {
    let r = report(&cargo(&["bench", "-p", "dalica-engine", "--bench", "scan_throughput", "--", "--json"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let (n, scan, per_s, thumbs) = scan_numbers(&r);
    assert!(n >= SCAN_PHOTOS as f64, "the budget holds for {SCAN_PHOTOS} photos: {r}");
    assert!(per_s >= SCAN_PHOTOS_PER_S, "{n} photos in {scan:.2} s: {per_s:.0} photos/s, under the {SCAN_PHOTOS_PER_S} photos/s budget");
    let scan_line =
        format!("scan throughput: {n} photos in {scan:.2} s, {per_s:.0} photos/s ≥ {SCAN_PHOTOS_PER_S} (every thumbnail after {thumbs:.1} s)");
    let r = report(&cargo(&["bench", "-p", "dalica-ui", "--bench", "cold_start", "--", "--json"], true));
    assert_eq!(r["mode"], "release", "{r}");
    let (photos, p50, max, rss) = cold_numbers(&r);
    assert!(photos >= COLD_START_PHOTOS, "the budgets hold for {COLD_START_PHOTOS} photos: {r}");
    assert!(p50 <= COLD_START_MS, "cold start: median {p50:.0} ms, over the {COLD_START_MS} ms budget");
    let memory = match rss {
        Some(m) => {
            assert!(m <= IDLE_MIB, "idle memory: {m:.0} MiB, over the {IDLE_MIB} MiB budget");
            format!("idle memory {m:.0} MiB ≤ {IDLE_MIB} MiB")
        }
        None if cfg!(any(target_os = "linux", target_os = "macos")) => panic!("idle memory not measured: {r}"),
        None => "idle memory not measured on this system".to_string(),
    };
    vec![scan_line, format!("cold start: {photos} photos, median {p50:.0} ms ≤ {COLD_START_MS} ms (slowest {max:.0} ms); {memory}")]
}
