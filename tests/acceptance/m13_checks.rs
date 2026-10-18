//! The parts of M13's acceptance that run other things (`m13.rs` part 6): the crate suites behind
//! each *Done when* (skipped inside `cargo xtask ci`, whose test step ran them:
//! `dalica_acceptance::SUITES_RAN`), the workflow pass rate with its floor of 14, W13 and W14 among
//! them (`cargo xtask stats --check`, skipped there too), `panic_hunt` over every command M13 added,
//! the public-faces corpus floors when the corpus is present (`cargo xtask test-corpus` is the
//! strict run, at the review), and M13's budgets (`development.md` §8, M13.31 and M13.43): in a
//! debug build the `inference`, `faces_scan`, `people_places` and `effects` smoke passes check the
//! paths and print their times; with `DALICA_ACCEPT_RELEASE=1` the release benches judge
//! themselves with `--check` on the budgets their rows carry (the ones `cargo xtask budgets` holds
//! them to): detection and embedding, faces embedded per second, memory per worker and the frame
//! while the scan runs (p95, never above 16 ms), the Unnamed grid and the Places pane at 10 000,
//! and every effect's draft and final (never above 16 and 100 ms) and Save.

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";
/// The workflows required now (W01 … W14).
const WORKFLOW_FLOOR: u64 = 14;

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

/// The crate suites behind M13's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Faces and the People group:");
    println!("  the interpreter and its limits, detection, alignment, embeddings, manifests, the self-test, grouping and suggestions' maths");
    suites("dalica-faces", &["detect", "grouping", "inference", "models"]);
    println!("  consented, resumable, hash-pinned downloads and the HTTPS transport");
    suites("dalica-fetch", &["download", "http"]);
    println!("  places, faces and people in the model, the sidecar, the store and its embeddings cache");
    suites("dalica-core", &["geo"]);
    suites("dalica-library", &["people", "places"]);
    suites("dalica-sidecar", &["fields"]);
    suites("dalica-store", &["faces_cache", "people", "fields"]);
    println!("  faces and people as data, models, the face scan, sidecars read on scan and rebuild, faces in crops and Pan and Zoom");
    suites("dalica-engine", &["people", "face_models", "face_scan", "sidecar_people_places", "crop_commands", "slideshow"]);
    suites("dalica-edit", &["suggest"]);
    suites("dalica-create", &["slideshow"]);
    println!("  face tiles and icons; the Unnamed grid, the People group and person sections, the face model dialog and Options ▸ People");
    suites("dalica-ui-kit", &["face_widgets", "icons_people"]);
    suites("dalica-ui", &["unnamed_faces", "people_tree", "face_model_ui"]);
    println!("Suggestions, the People pane and face regions:");
    println!("  MWG regions (the round-trip tests of xmp-interop.md §8, item 5), suggestions, people names in files");
    suites("dalica-meta", &["regions"]);
    suites("dalica-scan", &["meta"]);
    suites("dalica-engine", &["suggestions", "face_regions"]);
    println!("  the People pane and face boxes, suggestions in the window, people over MCP; people withheld from automation");
    suites("dalica-ui", &["people_pane", "suggestions_ui"]);
    suites("dalica-mcp", &["organize_tools"]);
    let out = cargo(&["test", "-p", "dalica-ui", "--lib", "control::"], false);
    assert!(passed(&out) > 0, "the control channel's tests ran:\n{out}");
    println!("  {} passed", passed(&out));
    println!("Places and geotagging:");
    println!("  the map widget, geotags in the files, tiles and place search, the Places pane, Options ▸ Network");
    suites("dalica-ui-kit", &["map_widget"]);
    suites("dalica-engine", &["places", "map_tiles"]);
    suites("dalica-ui", &["places_pane", "map_tiles_ui"]);
}

/// `cargo xtask stats --check`: the workflow tests over MCP, their pass rate printed and the floor
/// (fourteen, W13 and W14 among them) held. Returns the report's first line.
pub fn workflows() -> String {
    let out = cargo(&["xtask", "stats", "--check"], false);
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("workflows:") || l.starts_with("  ") || l.starts_with("stats:")).collect();
    for l in &lines {
        println!("  {l}");
    }
    let head = lines.iter().find(|l| l.starts_with("workflows:")).map_or_else(|| panic!("stats printed no pass rate:\n{out}"), |l| l.to_string());
    assert!(head.contains("(0 failing"), "every workflow that runs passes: {head}");
    for w in ["w13", "w14"] {
        assert!(!lines.iter().any(|l| l.starts_with("  ") && l.contains(w)), "{w} runs and passes:\n{out}");
    }
    let passing: u64 = head.strip_prefix("workflows: ").and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!(passing >= WORKFLOW_FLOOR, "the floor of {WORKFLOW_FLOOR}: {head}");
    head
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M13's
/// included, hostile params, never a panic.
pub fn panic_hunt() {
    dalica_acceptance::panic_hunt().unwrap();
}

/// The public-faces corpus (`faces.md` §10, `development.md` §11): detection, alignment, embedding
/// agreement, grouping and suggestion precision at their floors, run when `cargo xtask corpus`
/// reports `public-faces` and `face-models` present; otherwise, and inside `ci`, said plainly (the
/// CI corpus job and `cargo xtask test-corpus` are the strict run, at the review).
pub fn corpus(in_ci: bool) {
    if in_ci {
        println!("corpus: not run inside ci (`cargo xtask test-corpus -p faces` is the strict run, at the review)");
        return;
    }
    let status = cargo(&["xtask", "corpus"], false);
    let state = |set: &str| status.lines().find_map(|l| l.trim_start().strip_prefix(set).map(|s| s.trim().to_string()));
    let (faces, models) = (state("corpus/public-faces/"), state("corpus/face-models/"));
    let (Some(faces), Some(models)) = (faces, models) else { panic!("`cargo xtask corpus` lists the public-faces and face-models sets:\n{status}") };
    if !faces.starts_with("present") || !models.starts_with("present") {
        println!(
            "corpus: NOT RUN: corpus/public-faces/ is {faces}, corpus/face-models/ is {models} (`cargo xtask test-corpus -p faces` fetches, verifies and runs them; the review's strict run)"
        );
        return;
    }
    let out = cargo(
        &["test", "--release", "-p", "dalica-faces", "--features", "corpus", "--test", "corpus", "--", "--nocapture", "--test-threads=1"],
        false,
    );
    for l in out.lines().filter(|l| l.contains("floor") || l.contains("measured")) {
        println!("  {l}");
    }
    assert!(out.contains("test result: ok."), "{out}");
    println!("corpus: public-faces {faces}: the detection, grouping and suggestion floors hold");
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
        ["p50_ms", "p95_ms", "faces_per_s", "per_worker_mib", "budget_ms", "budget_p95_ms", "budget_faces_per_s", "budget_per_worker_mib"]
            .iter()
            .filter_map(|k| num(k))
            .collect();
    format!("{:<24} {}", r["name"].as_str().unwrap_or("?"), parts.join(", "))
}

/// M13's benches: (package, bench, what, the debug pass's arguments).
///
/// Two smoke passes run at the size the budgets hold for, not their own default: `inference`'s
/// default 160 px input has 10 stride-16 columns, so its second planted face (column 18) is not
/// there and it fails "the network reports the planted faces"; `people_places`' default 1 000 photos
/// make one group (1 500 ÷ k from the largest), a grid with nothing to scroll, and it fails "the
/// wheel scrolled the grid". Both are reported on M13.32's PR for the lead; the paths are the same.
const BENCHES: [(&str, &str, &str, &[&str]); 4] = [
    ("dalica-faces", "inference", "detection and one embedding per candidate recogniser", &["--side", "640"]),
    ("dalica-engine", "faces_scan", "the background face scan: faces per second, memory per worker, the frame while it runs", &[]),
    ("dalica-ui", "people_places", "the Unnamed grid and the Places pane at library scale", &["--photos", "10000"]),
    ("dalica-edit", "effects", "every effect's draft and final on the screen and its Save", &[]),
];

/// The benches' paths in a debug build (their smoke passes): every row measured and printed;
/// nothing judged.
pub fn budgets_debug() {
    for (package, bench, what, extra) in BENCHES {
        let mut args = vec!["test", "-p", package, "--bench", bench, "--", "--json"];
        args.extend(extra);
        let r = report(&cargo(&args, false));
        assert_eq!(r["mode"], "debug", "{r}");
        let rows = rows(&r);
        assert!(!rows.is_empty(), "{bench}: rows measured: {r}");
        println!("  {bench} (debug smoke, {what}): {} rows", rows.len());
        if bench != "effects" {
            for row in &rows {
                println!("    {}", line(row));
            }
        }
    }
}

/// `cargo bench -p <package> --bench <bench> -- --json --check` in release, on two cores where
/// the system can pin a process (`taskset -c 0,1` on Linux), as the budgets were measured
/// (`development.md` §8: two cores). Unpinned on a many-core machine some point effects' drafts
/// run slower than on two (M13.32's PR); the review's budgets job runs on two-core runners.
/// Panics with the output if a row misses its budget. Returns standard output and error.
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
    for (package, bench, what, _) in BENCHES {
        let r = report(&bench_release(package, bench));
        assert_eq!(r["mode"], "release", "{r}");
        lines.push(format!("{bench} ({what}): every row within its budget"));
        for row in rows(&r) {
            lines.push(format!("  {}", line(&row)));
        }
    }
    lines
}
