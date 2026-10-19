//! M14 "Collage and legacy sidecars" (`docs/roadmap.md` → M14 → the *Done when* of The collage room
//! and of Legacy sidecars, `docs/data-model.md` §10.11; run by `cargo xtask accept M14`).
//!
//! The live parts run the real binaries on fresh libraries in temporary homes: `dalica --headless`
//! driven over its control channel (`m11_live.rs`'s process and connection on a port the OS picks,
//! `m9_app.rs`'s waits and lookups) and `dalica-cli mcp` over real stdio. Every photo is procedural
//! and every legacy file is written by the test from the documented format, its names from
//! `dalica-sidecar`'s masked constant.
//!
//! 1. **The collage room** (`m14_collage.rs`): the tray, Create ▸ Picture Collage…, a folder header
//!    and a person section open the room with their photos; each of the six styles lays the same
//!    pictures out the same way for the same seed (twice, after a restart, and as
//!    `fingerprints/m14-layouts.json` records them, so on every system); a pile's picture dragged and
//!    turned by its wheel, a grid's two pictures swapped by a drag, Shuffle and Scramble each change
//!    only what they say, one step of the room's history each and none of the library's; the room
//!    with a pile and with a mosaic match their fingerprints; Create Collage renders on a worker
//!    while the window draws, writes a JPEG of the page's ratio at 3000 px and its recipe in the
//!    Collages project, each picture where the canvas showed it; Edit Collage reopens it with its
//!    clips and settings; Close asks, and Save Draft writes a new collage beside it, never
//!    replacing one.
//! 2. **Legacy sidecars** (`m14_legacy.rs`, §10.11): a legacy-only folder shows its stars,
//!    captions, edits, album, person and geotag after the first scan, with the notice once (its
//!    fingerprint); the first change writes `.dalica.json` with all of it, every legacy file
//!    byte-identical; a folder with both files reads only `.dalica.json`; the option off and on;
//!    Undo Save and Revert from legacy originals; hostile legacy files refused or skipped without a
//!    panic; a restart.
//! 3. **MCP** (`m14_mcp.rs`): W15 and W16 through `dalica-cli mcp` over stdio, checked on disk.
//! 4. **The checks** (`m14_checks.rs`): the crate suites behind the two areas' *Done when* (skipped
//!    inside `cargo xtask ci`, whose test step ran them), the workflow pass rate with its floor of
//!    16 (`cargo xtask stats --check`, skipped there too), `panic_hunt`, and M14's budgets: in a
//!    debug build the `collage`, `legacy_scan` and `collage_canvas` smoke passes check the paths and
//!    print their times; with `DALICA_ACCEPT_RELEASE=1` the release benches judge every row on its
//!    budget.
//!
//! Fingerprints are whole windows at 1400 × 900 (`fingerprints/m14-1400x900-*.json`). The manual
//! checks (a collage of real photos in each style, the handle and swaps by mouse and touchpad, the
//! eyedropper, a collage opened in another application; a real folder the previous organizer
//! managed, a change in it and its old file compared byte for byte) are in `m14.md`.

#[path = "m9_app.rs"]
#[allow(dead_code)]
mod app;
#[path = "m14_checks.rs"]
mod checks;
#[path = "m14_collage.rs"]
mod collage;
#[path = "m14_common.rs"]
#[allow(dead_code)]
mod common;
#[path = "m14_legacy.rs"]
mod legacy;
#[allow(dead_code)]
#[path = "m11_live.rs"]
mod live;
#[path = "m14_mcp.rs"]
mod mcp;

use std::time::Instant;

use app::secs;

#[test]
fn the_collage_room_from_its_entry_points_to_the_created_jpeg() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    collage::collage_room(&exe, home.path());
    println!("M14 the collage room: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn a_library_the_previous_organizer_managed_is_read_converted_on_the_first_change_and_reverted() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    legacy::legacy(&exe, home.path());
    println!("M14 legacy sidecars: passed in {:.1} s", secs(started));
}

#[test]
fn w15_and_w16_over_stdio() {
    let started = Instant::now();
    let home = tempfile::tempdir().unwrap();
    mcp::w15(home.path());
    mcp::w16(home.path());
    println!("M14 W15 and W16: passed in {:.1} s", secs(started));
}

#[test]
fn the_crate_suites_the_workflows_panic_hunt_and_the_budgets() {
    let started = Instant::now();
    let in_ci = dalica_acceptance::suites_ran();
    let rate = if in_ci {
        "the workflows: run by the ci test step and its stats --check".to_string()
    } else {
        checks::crate_suites();
        checks::workflows()
    };
    let suites = secs(started);
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
            "(the release budgets, each style's layout of 200, a pile of 50 rendered at 3000 px, the legacy scans of 10 000 photos and of a 64 MiB sidecar, the canvas frame with 100 pictures: run with {}=1)",
            checks::RELEASE
        );
    }
    println!("{rate}");
    println!("suites and workflows {suites:.1} s, panic_hunt {hunt:.1} s, debug benches {bench:.1} s; M14 checks: passed in {:.1} s", secs(started));
}
