//! The parts of M14's acceptance that run other things (`m14.rs` part 4): the crate suites behind
//! each *Done when* (skipped inside `cargo xtask ci`, whose test step ran them:
//! `dalica_acceptance::SUITES_RAN`), the workflow pass rate with its floor of 16, W15 and W16 among
//! them (`cargo xtask stats --check`, skipped there too), `panic_hunt` over every command M14 added,
//! and M14's budgets (`development.md` §8, M14.22): in a debug build the `collage`, `legacy_scan`
//! and `collage_canvas` smoke passes check the paths and print their times; with
//! `DALICA_ACCEPT_RELEASE=1` the release benches judge themselves with `--check` on the budgets
//! their rows carry (the ones `cargo xtask budgets` holds them to): each style's layout of 200
//! pictures, a pile of 50 photos rendered at 3000 px and its peak memory, the first scans of 10 000
//! photos with only the legacy sidecar and with `.dalica.json`, a 64 MiB legacy sidecar, and the
//! Collage room's frame with 100 pictures while one is dragged (p95, never above 16 ms).

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";
/// The workflows required now (W01 … W16).
const WORKFLOW_FLOOR: u64 = 16;

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

fn passed(out: &str) -> usize {
    out.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum()
}

/// `cargo test -p <package> --test <suite>…` in debug; panics if it fails or runs nothing.
fn suites(package: &str, list: &[&str]) {
    let mut args = vec!["test", "-p", package];
    for s in list {
        args.extend(["--test", s]);
    }
    let out = cargo(&args, false);
    let n = passed(&out);
    assert!(n > 0, "cargo {}: no test ran:\n{out}", args.join(" "));
    println!("  {n} passed");
}

/// The crate suites behind M14's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("The collage room:");
    println!(
        "  the document and its recipe under limits, the six layouts (the same for a seed, Scramble and Shuffle), the mosaics, the render at 3000 px"
    );
    suites("dalica-create", &["collage_doc", "collage_layouts", "collage_mosaic", "collage_render"]);
    println!("  the collage session and its history, Create Collage, Save Draft and Edit Collage (never replacing, the Collages project)");
    suites("dalica-engine", &["collage", "collage_output"]);
    suites("dalica-library", &["tree"]);
    println!("  the collage widgets and icons; the room, the canvas (never waiting on the disk), the entry points, the Projects group");
    suites("dalica-ui-kit", &["collage_widgets", "icons_collage"]);
    suites("dalica-ui", &["collage_room", "collage_canvas", "collage_entry", "tree_projects"]);
    println!("  over automation a collage is written only inside the write roots");
    let out = cargo(&["test", "-p", "dalica-ui", "--lib", "control::"], false);
    assert!(passed(&out) > 0, "the control channel's tests ran:\n{out}");
    println!("  {} passed", passed(&out));
    println!("Legacy sidecars:");
    println!("  the read-only reader, its limits and hostile files, the originals folder; the edit mapping; legacy facts in the model");
    suites("dalica-sidecar", &["legacy", "originals"]);
    suites("dalica-edit", &["legacy"]);
    suites("dalica-library", &["legacy"]);
    println!("  legacy folders on scan, the first write by exact name never replacing, legacy originals for Undo Save and Revert");
    suites("dalica-scan", &["legacy", "originals"]);
    suites("dalica-fs", &["create"]);
    suites("dalica-store", &["legacy"]);
    suites("dalica-create", &["save_legacy"]);
    println!(
        "  the legacy session (the sources, the option off and on, the notice) and its originals; the notice, the option, the edit panel's line in the window"
    );
    suites("dalica-engine", &["legacy", "legacy_originals"]);
    suites("dalica-ui", &["legacy_ui"]);
}

/// `cargo xtask stats --check`: the workflow tests over MCP, their pass rate printed and the floor
/// (sixteen, W15 and W16 among them) held. Returns the report's first line.
pub fn workflows() -> String {
    let out = cargo(&["xtask", "stats", "--check"], false);
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("workflows:") || l.starts_with("  ") || l.starts_with("stats:")).collect();
    for l in &lines {
        println!("  {l}");
    }
    let head = lines.iter().find(|l| l.starts_with("workflows:")).map_or_else(|| panic!("stats printed no pass rate:\n{out}"), |l| l.to_string());
    assert!(head.contains("(0 failing"), "every workflow that runs passes: {head}");
    for w in ["w15", "w16"] {
        assert!(!lines.iter().any(|l| l.starts_with("  ") && l.contains(w)), "{w} runs and passes:\n{out}");
    }
    let passing: u64 = head.strip_prefix("workflows: ").and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!(passing >= WORKFLOW_FLOOR, "the floor of {WORKFLOW_FLOOR}: {head}");
    head
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M14's
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

fn rows(report: &Value) -> Vec<Value> {
    report["rows"].as_array().cloned().unwrap_or_default()
}

/// One row as printed: its name, median and p95, the other numbers it carries, and its budgets.
fn line(r: &Value) -> String {
    let num = |k: &str| r[k].as_f64().map(|v| format!("{k} {v:.2}"));
    let parts: Vec<String> =
        ["p50_ms", "p95_ms", "peak_mib", "budget_ms", "budget_p95_ms", "budget_peak_mib"].iter().filter_map(|k| num(k)).collect();
    format!("{:<24} {}", r["name"].as_str().unwrap_or("?"), parts.join(", "))
}

/// M14's benches: (package, bench, what).
const BENCHES: [(&str, &str, &str); 3] = [
    ("dalica-create", "collage", "each style's layout and a pile rendered at 3000 px with its peak memory"),
    ("dalica-scan", "legacy_scan", "first scans of legacy-only and of Dalica folders, and the largest legacy sidecar"),
    ("dalica-ui", "collage_canvas", "the Collage room's frame while a picture is dragged"),
];

/// The benches' paths in a debug build (their smoke passes): every row measured and printed;
/// nothing judged.
pub fn budgets_debug() {
    for (package, bench, what) in BENCHES {
        let args = ["test", "-p", package, "--bench", bench, "--", "--json"];
        let r = report(&cargo(&args, false));
        assert_eq!(r["mode"], "debug", "{r}");
        let rows = rows(&r);
        assert!(!rows.is_empty(), "{bench}: rows measured: {r}");
        println!("  {bench} (debug smoke, {what}): {} rows", rows.len());
        for row in &rows {
            println!("    {}", line(row));
        }
    }
}

/// `cargo bench -p <package> --bench <bench> -- --json --check` in release, on two cores where
/// the system can pin a process (`taskset -c 0,1` on Linux), as the budgets were measured
/// (`development.md` §8: two cores). Panics with the output if a row misses its budget. Returns
/// standard output and error.
fn bench_release(package: &str, bench: &str) -> String {
    let started = Instant::now();
    let built = dalica_acceptance::cargo().args(["bench", "--no-run", "-p", package, "--bench", bench]).env(RELEASE, "1").status().unwrap();
    assert!(built.success(), "building the {bench} bench failed");
    let pin = cfg!(target_os = "linux") && std::process::Command::new("taskset").arg("-V").output().is_ok_and(|o| o.status.success());
    let mut cmd = if pin {
        let mut c = std::process::Command::new("taskset");
        c.args(["-c", "0,1"]).arg(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        c.current_dir(dalica_acceptance::workspace_root()).env_remove("DALICA_BLESS");
        c
    } else {
        dalica_acceptance::cargo()
    };
    let out = cmd.args(["bench", "-p", package, "--bench", bench, "--", "--json", "--check"]).env(RELEASE, "1").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{bench} (release{}): a budget missed:\n{text}", if pin { ", two cores" } else { "" });
    println!(
        "cargo bench -p {package} --bench {bench} -- --json --check{}: ok ({:.1} s)",
        if pin { " (taskset -c 0,1)" } else { "" },
        started.elapsed().as_secs_f64()
    );
    text
}

/// The budgets in release (`DALICA_ACCEPT_RELEASE=1`): each bench run with `--check`
/// ([`bench_release`]), which fails the run when a row misses its budget. Returns the lines to
/// print.
pub fn budgets_release() -> Vec<String> {
    let mut lines = Vec::new();
    for (package, bench, what) in BENCHES {
        let r = report(&bench_release(package, bench));
        assert_eq!(r["mode"], "release", "{r}");
        lines.push(format!("{bench} ({what}): every row within its budget"));
        for row in rows(&r) {
            lines.push(format!("  {}", line(&row)));
        }
    }
    lines
}
