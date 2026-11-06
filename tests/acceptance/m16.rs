//! M16 "Post-1.0 enhancements" (`docs/roadmap.md` → M16 → the *Done when* of Themes and Colour
//! search; run by `cargo xtask accept M16`).
//!
//! The live parts run the real binaries on fresh libraries in temporary homes: `dalica --headless`
//! driven over its control channel (`m11_live.rs`'s process and connection on a port the OS picks,
//! `m9_app.rs`'s waits and lookups, `m14_common.rs`'s painted photos and steady shots), with
//! `dalica-cli run --connect` and `dalica-cli mcp --connect` on the same app (`m16_mcp.rs`). Every
//! photo is procedural.
//!
//! 1. **Themes** (`m16_themes.rs`): each of the fifteen themes chosen from View ▸ Theme ▸, by
//!    `dalica-cli` over the control channel and by `cmd_theme_<id>` over MCP is worn by the next
//!    frame (the toolbar, the light table and the tray paint the theme's tokens); a restart opens
//!    in the last one chosen (Pro (Light)); the Options row previews Studio (Dark) in the next frame
//!    unsaved, and Cancel puts the saved theme back.
//! 2. **Colour search** (`m16_colours.rs`): a folder of seven procedural pictures answers each of
//!    the seven colours with the right one from the search field, from Tools ▸ Experimental ▸
//!    Search for ▸ and through `query_photos`; a B&W effect moves the green field to `bw`; a library
//!    whose snapshot has no summaries gains them from its stored sketches without a scan.
//! 3. **The checks** (`m16_checks.rs`): the crate suites behind both areas' *Done when* (skipped
//!    inside `cargo xtask ci`, whose test step ran them): the contrast tests and a gallery
//!    fingerprint per theme, the main window fingerprinted in every theme at 1400 × 900, the
//!    classifier, the index and the search; `cargo xtask parity` in its review form, `panic_hunt`,
//!    and M16's budgets: in a debug build the `colour_summary`, `colour_search` and `theme_switch`
//!    smoke passes check the paths and print their times; with `DALICA_ACCEPT_RELEASE=1` the
//!    release benches judge every row on its budget.
//!
//! Fingerprints are whole windows at 1400 × 900 (`fingerprints/m16-1400x900-*.json`): the library
//! in Dark, Pro (Light) and Classic, and `color:green`'s results. The checks by hand on each OS
//! (every theme in the real window, the title bar and native dialogs beside it) are in `m16.md`.

#[path = "m9_app.rs"]
#[allow(dead_code)]
mod app;
#[path = "m16_checks.rs"]
mod checks;
#[path = "m16_colours.rs"]
mod colours;
#[path = "m14_common.rs"]
#[allow(dead_code)]
mod common;
#[allow(dead_code)]
#[path = "m11_live.rs"]
mod live;
#[path = "m16_mcp.rs"]
mod mcp;
#[path = "m16_themes.rs"]
mod themes;

use std::time::Instant;

use app::secs;

#[test]
fn every_theme_from_the_menu_the_cli_and_mcp_in_the_next_frame_a_restart_and_the_options_preview() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let dir = tempfile::tempdir().unwrap();
    let home = dalica_engine::automation::plain(&std::fs::canonicalize(dir.path()).unwrap());
    themes::themes(&exe, &home);
    println!("M16 themes: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn the_seven_colours_from_the_field_the_menu_and_mcp_an_effect_and_a_library_without_summaries() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let dir = tempfile::tempdir().unwrap();
    let home = dalica_engine::automation::plain(&std::fs::canonicalize(dir.path()).unwrap());
    colours::colours(&exe, &home);
    println!("M16 colour search: passed in {:.1} s", secs(started));
}

#[test]
fn the_crate_suites_parity_panic_hunt_and_the_budgets() {
    let started = Instant::now();
    if dalica_acceptance::suites_ran() {
        println!("(the crate suites: run by the ci test step)");
    } else {
        checks::crate_suites();
    }
    let suites = secs(started);
    // Parity's `KEY-*` rows are the keymap of Linux and Windows (macOS binds Redo to Cmd+Shift+Z),
    // so it is left out on macOS, as the review's macOS run leaves it out; inside `cargo xtask ci`
    // its own static step checks it.
    let t = Instant::now();
    if dalica_acceptance::suites_ran() {
        println!("(parity: run by the ci static steps)");
    } else if cfg!(target_os = "macos") {
        println!("(parity: its KEY rows are Linux's and Windows' keymap; checked in the Linux run)");
    } else {
        checks::parity();
    }
    let parity = secs(t);
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
        println!(
            "(the release budgets, 100 000 colour summaries at open, color:green over 100 000 photos, a theme switch's frame at 100 000 photos: run with {}=1)",
            checks::RELEASE
        );
    }
    println!(
        "suites {suites:.1} s, parity {parity:.1} s, panic_hunt {hunt:.1} s, debug benches {bench:.1} s; M16 checks: passed in {:.1} s",
        secs(started)
    );
}
