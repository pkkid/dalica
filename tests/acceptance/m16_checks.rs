//! The parts of M16's acceptance that run other things (`m16.rs` part 3): the crate suites behind
//! each *Done when* (skipped inside `cargo xtask ci`, whose test step ran them:
//! `dalica_acceptance::SUITES_RAN`): every theme through the contrast tests, a gallery fingerprint
//! per theme, the window fingerprinted in every theme at 1400 × 900, the switch and the Options
//! preview in process, the colour classifier, the summaries in the index and the store, the sketch's
//! summary and the search; `cargo xtask parity` in its review form; `panic_hunt` over every command
//! M16 added; and M16's budgets (`development.md` §8, M16.11): in a debug build the
//! `colour_summary`, `colour_search` (the search bench with `--colour`) and `theme_switch` smoke
//! passes check the paths and print their times; with `DALICA_ACCEPT_RELEASE=1` the release
//! benches judge themselves with `--check` on the budgets their rows carry (the ones
//! `cargo xtask budgets` holds them to).

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";

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

/// The crate suites behind M16's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Themes:");
    println!("  every theme through the contrast tests; a widget-gallery fingerprint per theme; the palettes");
    suites("dalica-ui-kit", &["contrast", "gallery", "themes"]);
    println!("  the Theme setting, its options row, one command per theme");
    suites("dalica-engine", &["theme", "settings", "options_catalogue"]);
    println!("  switching in the next frame, the menu's checks, the Options preview and Cancel, a restart; the window in every theme");
    suites("dalica-ui", &["theme_switch", "themes", "options_dialog", "menus", "menu_checks", "view_state"]);
    println!("Colour search:");
    println!("  the search colours and the classifier; the summaries in the model, the index, the search and the store");
    suites("dalica-core", &["color_summary"]);
    suites("dalica-library", &["colours", "search"]);
    suites("dalica-store", &["colours"]);
    println!("  the summary made with each sketch; color: end to end, an edit, a library without summaries, Search for ▸");
    suites("dalica-thumbs", &["sketch"]);
    suites("dalica-engine", &["colour_search", "search"]);
    suites("dalica-ui", &["search_ui"]);
}

/// `cargo xtask parity` in the review's form: the rows, their cited paths, every cited command
/// against the registry and the `KEY-*` rows against the keymap.
pub fn parity() {
    let out = cargo(&["xtask", "parity"], false);
    for l in out.lines().filter(|l| l.starts_with("parity")) {
        println!("  {l}");
    }
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M16's
/// theme commands and `search.color` included, hostile params, never a panic.
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
    let parts: Vec<String> = ["p50_ms", "p95_ms", "p50_us", "budget_ms", "budget_p95_ms", "budget_us"].iter().filter_map(|k| num(k)).collect();
    format!("{:<28} {}", r["name"].as_str().unwrap_or("?"), parts.join(", "))
}

/// M16's benches: (package, cargo bench target, its arguments, the bench's name, what).
const BENCHES: [(&str, &str, &[&str], &str, &str); 3] = [
    (
        "dalica-engine",
        "colour_summary",
        &[],
        "colour_summary",
        "a library opened without colour summaries: every summary made from its stored sketch, and one per sketch",
    ),
    ("dalica-engine", "search", &["--colour"], "colour_search", "color:green over the search library with a summary on every photo"),
    ("dalica-ui", "theme_switch", &[], "theme_switch", "the first frame after a theme switch against a frame without one, every theme in turn"),
];

/// The benches' paths in a debug build (their smoke passes): every row measured and printed;
/// nothing judged.
pub fn budgets_debug() {
    for (package, target, extra, bench, what) in BENCHES {
        let mut args = vec!["test", "-p", package, "--bench", target, "--", "--json"];
        args.extend_from_slice(extra);
        let r = report(&cargo(&args, false));
        assert_eq!(r["mode"], "debug", "{r}");
        assert_eq!(r["bench"], bench, "{r}");
        let rows = rows(&r);
        assert!(!rows.is_empty(), "{bench}: rows measured: {r}");
        println!("  {bench} (debug smoke, {what}): {} rows", rows.len());
        for row in &rows {
            println!("    {}", line(row));
        }
    }
}

/// `cargo bench -p <package> --bench <target> -- --json --check <extra>` in release, on two cores
/// where the system can pin a process (`taskset -c 0,1` on Linux), as the budgets were measured
/// (`development.md` §8: two cores). Panics with the output if a row misses its budget.
fn bench_release(package: &str, target: &str, extra: &[&str]) -> String {
    let started = Instant::now();
    let built = dalica_acceptance::cargo().args(["bench", "--no-run", "-p", package, "--bench", target]).env(RELEASE, "1").status().unwrap();
    assert!(built.success(), "building the {target} bench failed");
    let pin = cfg!(target_os = "linux") && std::process::Command::new("taskset").arg("-V").output().is_ok_and(|o| o.status.success());
    let mut cmd = if pin {
        let mut c = std::process::Command::new("taskset");
        c.args(["-c", "0,1"]).arg(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        c.current_dir(dalica_acceptance::workspace_root()).env_remove("DALICA_BLESS");
        c
    } else {
        dalica_acceptance::cargo()
    };
    let out = cmd.args(["bench", "-p", package, "--bench", target, "--", "--json", "--check"]).args(extra).env(RELEASE, "1").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{target} {extra:?} (release{}): a budget missed:\n{text}", if pin { ", two cores" } else { "" });
    println!(
        "cargo bench -p {package} --bench {target} -- --json --check {}{}: ok ({:.1} s)",
        extra.join(" "),
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
    for (package, target, extra, bench, what) in BENCHES {
        let r = report(&bench_release(package, target, extra));
        assert_eq!(r["mode"], "release", "{r}");
        assert_eq!(r["bench"], bench, "{r}");
        lines.push(format!("{bench} ({what}): every row within its budget"));
        for row in rows(&r) {
            lines.push(format!("  {}", line(&row)));
        }
    }
    lines
}
