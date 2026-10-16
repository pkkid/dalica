//! M10 "Preferences, MCP and Release 0.5" (`docs/roadmap.md` → M10 → the *Done when* of
//! Preferences and shortcuts, MCP and workflows, and Release 0.5; run by `cargo xtask accept M10`).
//!
//! The live parts run the real binaries on fresh libraries in temporary homes: `dalica --headless`
//! driven over its control channel (`m6_live.rs`'s process and connection, `m9_app.rs`'s waits and
//! lookups) and `dalica-cli mcp` over real stdio:
//!
//! 1. **Preferences and shortcuts** (`m10_prefs.rs`): every option the build can change set with
//!    one `options.set` and read back by `options.list`, across a restart, while the keys of a
//!    later version in `settings.json` stay verbatim, then every default back; Star rebound to F8
//!    through `app.setShortcut` and F8 pressed headless starring the selected photo (Ctrl+8 no
//!    longer does); Ctrl+1 for Star refused naming Small Thumbnails in Library, then taken with
//!    `replace`; both keys kept across a restart; Reset All in the shortcut editor (Ctrl+/)
//!    restoring every row of `keymap.list` to its default.
//! 2. **The looks** (`m10_looks.rs`): Options opened with Ctrl+, and its four tabs (General, File
//!    Types, Export, Keyboard Shortcuts: the shortcut editor), and the window at UI scale 150 %
//!    with font size Large, each matched against `fingerprints/m10-1400x900-*.json`; each tab fits
//!    a 900 × 600 window.
//! 3. **MCP, both modes** (`m10_mcp.rs`): W02 and W01 through `dalica-cli mcp` over stdio, in
//!    headless mode and in connect mode against `dalica --headless --control`, each checked on
//!    disk, and the two outcomes equal.
//! 4. **The checks** (`m10_checks.rs`): the crate suites behind the three areas' *Done when* (the
//!    keymap conflict tests, the stdio smoke run among them; skipped inside `cargo xtask ci`, whose
//!    test step ran them: `dalica_acceptance::SUITES_RAN`), the workflow pass rate with its floor
//!    (`cargo xtask stats --check`, skipped there too), `cargo xtask parity --p0`, `panic_hunt`,
//!    and the P0 budgets M10 added: in a debug build the `scan_throughput` and `cold_start` smoke
//!    passes check the paths and print their times; with `DALICA_ACCEPT_RELEASE=1` the release
//!    benches are judged (5 000 JPEGs at ≥ 500 photos/s; the 100 k library's median cold start
//!    ≤ 2 s, idle memory ≤ 600 MiB).
//!
//! The manual checks (Options at 150 % and Large, a File Types round trip, a rebind with a
//! conflict, Reset All, `dalica-cli mcp --demo` in Claude Code, connect mode to a running app, the
//! Windows touchpad pinch, the macOS build used by hand) are in `m10.md`.

#[path = "m9_app.rs"]
#[allow(dead_code)]
mod app;
#[path = "m10_checks.rs"]
mod checks;
#[allow(dead_code)]
#[path = "m6_live.rs"]
mod live;
#[path = "m10_looks.rs"]
mod looks;
#[path = "m10_mcp.rs"]
mod mcp;
#[path = "m10_prefs.rs"]
mod prefs;

use std::time::Instant;

use app::secs;

#[test]
fn options_round_trip_star_on_f8_a_conflict_and_reset_all_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    prefs::preferences(&exe, home.path());
    println!("M10 preferences and shortcuts: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn the_options_tabs_the_shortcut_editor_and_the_window_at_150_and_large() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    looks::looks(&exe, home.path());
    println!("M10 looks: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn w01_and_w02_over_stdio_agree_in_headless_and_connect_mode() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    let (headless, connect) = mcp::agree(&exe, home.path());
    println!("M10 MCP modes: passed in {:.1} s (headless {headless:.1} s, connect {connect:.1} s)", secs(started));
}

#[test]
fn the_crate_suites_the_workflows_parity_p0_panic_hunt_and_the_budgets() {
    let started = Instant::now();
    let rate = if dalica_acceptance::suites_ran() {
        "the workflows: run by the ci test step and its stats --check".to_string()
    } else {
        checks::crate_suites();
        checks::workflows()
    };
    let suites = secs(started);
    let t = Instant::now();
    let parity = checks::parity_p0();
    let parity_s = secs(t);
    let t = Instant::now();
    checks::panic_hunt();
    let hunt = secs(t);
    let t = Instant::now();
    checks::budgets_debug();
    let bench = secs(t);
    if std::env::var_os(checks::RELEASE).is_some_and(|v| v == "1") {
        let t = Instant::now();
        let lines = checks::budgets_release();
        println!("release budgets ({:.1} s):", secs(t));
        for l in lines {
            println!("  {l}");
        }
    } else {
        println!("(the release budgets, 5 000 photos scanned and the 100 k cold start: run with {}=1)", checks::RELEASE);
    }
    println!("{rate}");
    println!("{parity}");
    println!(
        "suites and workflows {suites:.1} s, parity {parity_s:.1} s, panic_hunt {hunt:.1} s, debug benches {bench:.1} s; M10 checks: passed in {:.1} s",
        secs(started)
    );
}
