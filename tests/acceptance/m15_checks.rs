//! The parts of M15's acceptance that run other things (`m15.rs` part 7): the crate suites behind
//! each *Done when* (skipped inside `cargo xtask ci`, whose test step ran them:
//! `dalica_acceptance::SUITES_RAN`), HEVC with its non-default `hevc` feature, the catalogs
//! (`cargo xtask i18n`), the workflow pass rate with its floor of 17, W17 among them
//! (`cargo xtask stats --check`, skipped there too), `cargo xtask parity` in its review form (every
//! cited command and key checked against the registry; no row ✅ without its evidence), the SBOM of
//! every release package, `panic_hunt` over every command M15 added, and M15's budgets
//! (`development.md` §8, M15.34): in a debug build the `video_probe`, `video_play`,
//! `camera_import`, `a11y_frame` and `i18n_frame` smoke passes check the paths and print their
//! times; with `DALICA_ACCEPT_RELEASE=1` the release benches judge themselves with `--check` on the
//! budgets their rows carry (the ones `cargo xtask budgets` holds them to).

use std::time::Instant;

use serde_json::Value;

/// The environment variable that adds the release-build budgets.
pub const RELEASE: &str = "DALICA_ACCEPT_RELEASE";
/// The workflows required now (W01 … W17).
const WORKFLOW_FLOOR: u64 = 17;

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

/// `cargo test -p <package> [--features …] --test <suite>…` in debug; panics if it fails or runs
/// nothing.
fn suites_with(package: &str, features: Option<&str>, list: &[&str]) {
    let mut args = vec!["test", "-p", package];
    if let Some(f) = features {
        args.extend(["--features", f]);
    }
    for s in list {
        args.extend(["--test", s]);
    }
    let out = cargo(&args, false);
    let n = passed(&out);
    assert!(n > 0, "cargo {}: no test ran:\n{out}", args.join(" "));
    println!("  {n} passed");
}

fn suites(package: &str, list: &[&str]) {
    suites_with(package, None, list);
}

/// The crate suites behind M15's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    println!("Video:");
    println!("  MP4, MOV, M4V and MKV recognised by content and probed under limits; frames, posters, steps and sound; hostile and cut containers");
    suites("dalica-image", &["video", "video_frames", "audio"]);
    println!("  HEVC with its non-default feature (its phone clips are the corpus job's; skipped and named when not fetched)");
    suites_with("dalica-image", Some("hevc"), &["video_frames"]);
    println!("  the video facts in the model, on scan and in the store; poster thumbnails; trim in the recipe");
    suites("dalica-library", &["video"]);
    suites("dalica-scan", &["video"]);
    suites("dalica-store", &["video"]);
    suites("dalica-thumbs", &["video"]);
    suites("dalica-edit", &["video_ops"]);
    println!("  Export Clip and first frames, collages and the slideshow's clips and music");
    suites("dalica-create", &["video_output", "export_video"]);
    suites("dalica-engine", &["videos", "video_play", "video_posters", "slideshow_sound", "collage_video", "search"]);
    println!("  the video cells, the player, the Videos filter, the slideshow, Export's row, the video controls and icons");
    suites("dalica-ui-kit", &["video_widgets", "icons_video"]);
    suites("dalica-ui", &["video_tiles", "video_player", "slideshow_video", "export_video"]);
    suites("dalica", &["sound"]);
    println!("Localisation and accessibility:");
    println!("  the catalogs and plurals, English and Thai, the pseudo-locale's sweeps of every view and dialog, cut and wrap at both sizes");
    suites("dalica-ui-kit", &["fonts", "thai_text", "accessibility"]);
    suites("dalica-ui", &["i18n", "i18n_window", "i18n_dialogs", "i18n_views", "i18n_languages"]);
    println!("  the AccessKit audit of every view and dialog, keyboard-only use, UI scale and font size");
    suites("dalica-ui", &["accessibility", "keyboard_only", "scaling"]);
    println!("OS integration:");
    println!("  the recording Os: Locate on Disk, Open With, the external editor, the clipboard, the default viewer; refused over automation");
    suites("dalica-engine", &["os", "default_viewer", "import_camera"]);
    suites("dalica-ui", &["os_menus", "drag_out", "default_viewer", "import_camera"]);
    println!("  the services per OS, the default viewer per OS, one Dalica; cameras over PTP");
    suites("dalica", &["os", "default_viewer", "instance"]);
    suites("dalica-scan", &["ptp"]);
}

/// `cargo xtask i18n`: the catalogs read without a fault and every registered language covers what
/// it must; the draft counts printed.
pub fn catalogs() {
    let out = cargo(&["xtask", "i18n"], false);
    for l in out.lines().filter(|l| l.contains("th") || l.contains("draft") || l.starts_with("i18n")) {
        println!("  {l}");
    }
}

/// `cargo xtask stats --check`: the workflow tests over MCP, their pass rate printed and the floor
/// (seventeen, W17 among them) held. Returns the report's first line.
pub fn workflows() -> String {
    let out = cargo(&["xtask", "stats", "--check"], false);
    let lines: Vec<&str> = out.lines().filter(|l| l.starts_with("workflows:") || l.starts_with("  ") || l.starts_with("stats:")).collect();
    for l in &lines {
        println!("  {l}");
    }
    let head = lines.iter().find(|l| l.starts_with("workflows:")).map_or_else(|| panic!("stats printed no pass rate:\n{out}"), |l| l.to_string());
    assert!(head.contains("(0 failing"), "every workflow that runs passes: {head}");
    assert!(!lines.iter().any(|l| l.starts_with("  ") && l.contains("w17")), "w17 runs and passes:\n{out}");
    let passing: u64 = head.strip_prefix("workflows: ").and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!(passing >= WORKFLOW_FLOOR, "the floor of {WORKFLOW_FLOOR}: {head}");
    head
}

/// `cargo xtask parity` in the review's form: the rows, their cited paths, every cited command
/// against the registry and the `KEY-*` rows against the keymap.
pub fn parity() {
    let out = cargo(&["xtask", "parity"], false);
    for l in out.lines().filter(|l| l.starts_with("parity")) {
        println!("  {l}");
    }
}

/// `cargo xtask sbom --out <dir>`: a CycloneDX file for each release package, every component
/// with its licence.
pub fn sbom(dir: &std::path::Path) {
    let out_dir = dir.to_string_lossy().into_owned();
    let out = cargo(&["xtask", "sbom", "--out", &out_dir], false);
    let files: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!files.is_empty(), "an SBOM per package:\n{out}");
    for f in &files {
        let doc: Value = serde_json::from_slice(&std::fs::read(dir.join(f)).unwrap()).unwrap();
        assert_eq!(doc["bomFormat"], "CycloneDX", "{f}");
        let comps = doc["components"].as_array().map_or(0, Vec::len);
        assert!(comps > 0, "{f}: its components");
        println!("  {f}: {comps} components");
    }
}

/// `panic_hunt` (once per review gate; `dalica_acceptance::panic_hunt`): every command, M15's
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
    format!("{:<28} {}", r["name"].as_str().unwrap_or("?"), parts.join(", "))
}

/// M15's benches: (package, bench, what).
const BENCHES: [(&str, &str, &str); 5] = [
    ("dalica-image", "video_probe", "1 000 MP4, MOV and Matroska files probed as a scan reads them"),
    ("dalica-engine", "video_play", "an H.264 clip's poster, its frame step and its playing frames"),
    ("dalica-engine", "camera_import", "a camera's pictures listed, copied and verified over PTP"),
    ("dalica-ui", "a11y_frame", "the library's frame at 100 000 photos with AccessKit on against off"),
    ("dalica-ui", "i18n_frame", "the library's frame at 100 000 photos in the pseudo-locale against English"),
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
/// (`development.md` §8: two cores). Panics with the output if a row misses its budget.
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
