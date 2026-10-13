//! The parts of M6's acceptance that run other things: the crate suites behind each *Done when*,
//! `panic_hunt`, the metadata corpus, and the latest release dry run read through `gh`.

use std::process::Command;
use std::time::Instant;

use serde_json::Value;

/// Runs `cargo <args>`; panics with its output if it fails. Returns its standard output.
pub fn cargo(args: &[&str]) -> String {
    let started = Instant::now();
    let out = dalica_acceptance::cargo().args(args).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "cargo {}:\n{stdout}\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    println!("cargo {}: ok ({:.1} s)", args.join(" "), started.elapsed().as_secs_f64());
    stdout
}

/// Runs `cargo test <args>`; panics with its output if it fails or runs nothing. Returns how many
/// tests passed.
pub fn cargo_test(args: &[&str]) -> usize {
    let all: Vec<&str> = ["test"].iter().chain(args).copied().collect();
    let stdout = cargo(&all);
    let passed: usize =
        stdout.lines().filter_map(|l| l.strip_prefix("test result: ok. ")).filter_map(|l| l.split(' ').next()?.parse::<usize>().ok()).sum();
    assert!(passed > 0, "cargo {}: no test ran:\n{stdout}", all.join(" "));
    println!("  {passed} passed");
    passed
}

/// `cargo test -p <package> --test <suite>…`.
fn suites(package: &str, list: &[&str]) -> usize {
    let mut args = vec!["-p", package];
    for s in list {
        args.extend(["--test", s]);
    }
    cargo_test(&args)
}

/// The crate suites behind M6's *Done when*, as they are (each suite's file says what it covers).
pub fn crate_suites() {
    // Photo info: the model, the recipe and its geometry, the sidecar and index fields, the
    // commands' contracts (behaviour, undo and redo, `enabled`, hostile params, the sidecar after
    // each, surviving a rebuild), and the library's panes, dialogs and cells.
    println!("Photo info:");
    suites("dalica-library", &["ops", "m6_fields", "tags", "tree"]);
    suites("dalica-edit", &["recipe", "geometry", "frame"]);
    cargo_test(&["-p", "dalica-sidecar"]);
    suites("dalica-store", &["m6_fields", "sidecars"]);
    suites("dalica-engine", &["rotate", "hide", "captions", "folder_info", "tags", "dates", "photo_metadata", "settings_m6", "keymap"]);
    suites(
        "dalica-ui",
        &[
            "rotate",
            "hidden",
            "captions",
            "folder_properties",
            "properties_pane",
            "tags_pane",
            "status_tags",
            "adjust_date",
            "quick_tags",
            "view_state_m6",
            "stand_in",
        ],
    );
    suites("dalica-ui-kit", &["datetime"]);
    // Tags and metadata write-back: xmp-interop.md §8 tests 1–4 and 6–10 (5, face regions, is
    // M13's).
    println!("Tags and metadata write-back (xmp-interop.md §8):");
    println!("  tests 1-4, 6, 7, 10: meta's read, XMP, merge, sidecar, IPTC, EXIF and container write suites");
    suites("dalica-meta", &["read", "xmp", "xmp_merge", "xmp_sidecar", "iptc_write", "exif_edit", "write_jpeg", "write_png_webp_tiff"]);
    println!("  tests 8 and 9: the write queue (conflicts, atomicity), file times with faults, the sync rules");
    suites("dalica-store", &["metawrite"]);
    suites("dalica-fs", &["times"]);
    suites("dalica-library", &["sync"]);
    println!("  import on scan and the write-back end to end");
    suites("dalica-scan", &["meta"]);
    suites("dalica-engine", &["meta_import", "write_back"]);
    suites("dalica-ui", &["notices"]);
    // Release 0.1: the application id, the About window, the release notes, the version.
    println!("Release 0.1:");
    suites("dalica-core", &["app_id", "build"]);
    suites("dalica-ui", &["about", "release_notes"]);
}

/// `panic_hunt -- --ignored`: every command, hostile params, never a panic.
pub fn panic_hunt() {
    cargo_test(&["-p", "dalica-engine", "--test", "panic_hunt", "--", "--ignored"]);
}

/// The metadata corpus (`development.md` §11, `xmp-interop.md` §8 over other applications'
/// files): after a Dalica write of each owned field every byte outside the owned properties is
/// kept. Run when `cargo xtask corpus` reports the `metadata` set present and verified; otherwise,
/// and inside `ci`, said plainly (the CI corpus job is the strict run).
pub fn corpus(in_ci: bool) {
    if in_ci {
        println!("corpus: not run inside ci (the CI corpus job, `cargo xtask test-corpus`, is the strict run)");
        return;
    }
    let status = cargo(&["xtask", "corpus"]);
    let state = status.lines().find_map(|l| l.trim_start().strip_prefix("corpus/metadata/").map(|s| s.trim().to_string()));
    let Some(state) = state else { panic!("`cargo xtask corpus` lists no metadata set:\n{status}") };
    if !state.starts_with("present") {
        println!(
            "corpus: NOT RUN: corpus/metadata/ is {state} (`cargo xtask corpus --all` fetches and verifies it; the CI corpus job is the strict run)"
        );
        return;
    }
    let out = cargo(&["test", "--release", "-p", "dalica-meta", "--features", "corpus", "--test", "corpus", "--", "--nocapture", "--test-threads=1"]);
    for l in out.lines().filter(|l| l.contains(" measured: ") || l.contains("SKIPPED") || l.contains("above its floor")) {
        println!("{l}");
    }
    assert!(out.contains("test result: ok."), "{out}");
    println!("corpus: corpus/metadata/ {state}: every file read; other applications' bytes kept after each Dalica write (floors held)");
}

/// The jobs a release dry run must have passed: the builds on the three OSes, the checksums, and
/// their verification on each OS.
const RELEASE_JOBS: [&str; 8] = [
    "build Linux x86_64",
    "build Linux aarch64",
    "build Windows x64",
    "build macOS universal",
    "checksums",
    "verify checksums (ubuntu-latest)",
    "verify checksums (windows-latest)",
    "verify checksums (macos-latest)",
];

/// The step of each `verify checksums` job that checks the artifacts against `SHA256SUMS.txt`.
const VERIFY_STEP: &str = "Verify the artifacts against SHA256SUMS.txt";

/// `gh <args>` → its JSON output, or why it could not run (no `gh`, not signed in, offline).
fn gh(args: &[&str]) -> Result<Value, String> {
    let out = Command::new("gh").args(args).current_dir(dalica_acceptance::workspace_root()).output().map_err(|e| format!("cannot run gh: {e}"))?;
    if !out.status.success() {
        return Err(format!("gh {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("gh {}: not JSON: {e}", args.join(" ")))
}

/// The latest finished release dry run (`.github/workflows/release.yml`): on `main` when it has
/// one, else the newest on any branch (pull requests touching packaging run it), said which. It
/// must have built on Linux, Windows and macOS with its checksums written and verified on all
/// three. Read through `gh`; when `gh` cannot answer (not installed, not signed in, offline), and
/// inside `ci`, this says so plainly instead.
pub fn release_dry_run(in_ci: bool) {
    if in_ci {
        println!("release dry run: not checked inside ci (it reads GitHub through gh; `cargo xtask accept M6` does)");
        return;
    }
    let fields = "databaseId,headBranch,headSha,event,status,conclusion,createdAt,displayTitle,url";
    let runs = |branch: Option<&str>| -> Result<Vec<Value>, String> {
        let mut args = vec!["run", "list", "--workflow", "release.yml", "--limit", "30", "--json", fields];
        if let Some(b) = branch {
            args.extend(["--branch", b]);
        }
        let v = gh(&args)?;
        Ok(v.as_array().cloned().unwrap_or_default().into_iter().filter(|r| r["status"] == "completed").collect())
    };
    let (run, which) = match runs(Some("main")) {
        Err(e) => {
            println!("release dry run: NOT CHECKED: {e}");
            return;
        }
        Ok(main) if !main.is_empty() => (main[0].clone(), "the latest on main".to_string()),
        Ok(_) => match runs(None) {
            Err(e) => {
                println!("release dry run: NOT CHECKED: {e}");
                return;
            }
            Ok(any) => {
                let Some(r) = any.first().cloned() else { panic!("no finished Release workflow run on GitHub at all") };
                let which = format!("no Release run on main yet; the newest finished one, on {} ({})", r["headBranch"], r["event"]);
                (r, which)
            }
        },
    };
    let id = run["databaseId"].to_string();
    let jobs = match gh(&["run", "view", &id, "--json", "jobs"]) {
        Ok(v) => v["jobs"].as_array().cloned().unwrap_or_default(),
        Err(e) => {
            println!("release dry run: NOT CHECKED: {e}");
            return;
        }
    };
    println!(
        "release dry run: {which}: run {id} \"{}\" at {} ({}), {}: {}",
        run["displayTitle"].as_str().unwrap_or_default(),
        run["headSha"].as_str().unwrap_or_default().get(..10).unwrap_or_default(),
        run["createdAt"].as_str().unwrap_or_default(),
        run["conclusion"].as_str().unwrap_or_default(),
        run["url"].as_str().unwrap_or_default()
    );
    assert_eq!(run["conclusion"], "success", "the release dry run {id} did not pass: {run}");
    for name in RELEASE_JOBS {
        let job = jobs.iter().find(|j| j["name"] == name).unwrap_or_else(|| panic!("release run {id} has no job `{name}`"));
        assert_eq!(job["conclusion"], "success", "release run {id}: `{name}`: {}", job["conclusion"]);
        if name.starts_with("verify checksums") {
            let step = job["steps"].as_array().and_then(|s| s.iter().find(|s| s["name"] == VERIFY_STEP));
            let step = step.unwrap_or_else(|| panic!("release run {id}: `{name}` has no step `{VERIFY_STEP}`"));
            assert_eq!(step["conclusion"], "success", "release run {id}: `{name}`: {VERIFY_STEP}");
        }
        println!("  {name}: success");
    }
    println!("  the artifacts were checksummed and verified against SHA256SUMS.txt on Linux, Windows and macOS");
}
