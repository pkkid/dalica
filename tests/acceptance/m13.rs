//! M13 "People and places" (`docs/roadmap.md` → M13 → the *Done when* of Faces and the People
//! group, Suggestions, the People pane and face regions, and Places and geotagging; run by
//! `cargo xtask accept M13`).
//!
//! The live parts run the real binaries on fresh libraries in temporary homes: `dalica --headless`
//! driven over its control channel (`m11_live.rs`'s process and connection, `m9_app.rs`'s waits and
//! lookups) and `dalica-cli mcp` over real stdio. No real face model and no network is ever used:
//! the models are synthetic (`m13_common.rs`: a detector planted in the models folder, a recogniser
//! installed from a local file through the face model dialog), and every connection the app tries
//! goes to a loopback proxy (`HTTPS_PROXY`) that records the host asked for and refuses it.
//!
//! 1. **Faces and the People group** (`m13_faces.rs`): the recogniser installed after its terms
//!    (Add Model disabled until the box is ticked); the face scan fills Unnamed in the background,
//!    frames going on meanwhile and none near its time, the groups following the paint; the Unnamed
//!    grid and a person's section match their fingerprints; a group named in the grid (New Person)
//!    is one undo step; ignored and removed faces stay away over a restart (a rescan) and an index
//!    rebuild, the named group kept; no connection at any point; the whole run's `RUST_LOG=trace`
//!    log holds no name, nickname, rectangle or vector.
//! 2. **Suggestions, the People pane and face regions** (`m13_people.rs`): "Name?" only above the
//!    threshold and ahead of the runner-up; Confirm one step and never suggested again, Reject for
//!    good (also after a restart); a face drawn by hand on a photo turned 90° stored in the stored
//!    frame and shown where it was drawn, after a restart too; rename, merge and delete one step
//!    each; with Store people names in photos on, the names in the files as MWG regions read back
//!    where the faces are; the People pane matches its fingerprint.
//! 3. **Places and geotagging** (`m13_places.rs`): photos dropped on the map geotagged as one undo
//!    step, kept over a restart and an index rebuild; the place in the files (EXIF GPS and its XMP
//!    mirror) within 10⁻⁶° and read back by a new library; Clear Location removes both; the Places
//!    pane matches its fingerprint.
//! 4. **No network without the user's action** (`m13_network.rs`): start-up, scans and the Places
//!    pane with tiles off connect nowhere; tiles only once allowed and while the pane is open, a
//!    place search only on Enter; a model download only once the licence box is ticked.
//! 5. **MCP** (`m13_mcp.rs`): W13 and W14 through `dalica-cli mcp` over stdio, checked on disk.
//! 6. **The checks** (`m13_checks.rs`): the crate suites behind the three areas' *Done when* (the
//!    MWG round-trip tests among them; skipped inside `cargo xtask ci`, whose test step ran them),
//!    the workflow pass rate with its floor of 14 (`cargo xtask stats --check`, skipped there too),
//!    `panic_hunt`, the public-faces corpus floors when the corpus is present (`cargo xtask
//!    test-corpus -p faces` at the review), and M13's budgets: in a debug build the `inference`,
//!    `faces_scan`, `people_places` and `effects` smoke passes check the paths and print their
//!    times; with `DALICA_ACCEPT_RELEASE=1` the release benches judge every row on its budget.
//!
//! Fingerprints are whole windows at 1400 × 900 (`fingerprints/m13-1400x900-*.json`). The manual
//! checks (the chosen models downloaded for real and run on real photos, naming and suggestions on
//! a real library, the map by mouse and touchpad, tiles allowed, a geotagged photo in another
//! application's map) are in `m13.md`.

#[path = "m9_app.rs"]
#[allow(dead_code)]
mod app;
#[path = "m13_checks.rs"]
mod checks;
#[path = "m13_common.rs"]
#[allow(dead_code)]
mod common;
#[path = "m13_faces.rs"]
mod faces;
#[allow(dead_code)]
#[path = "m11_live.rs"]
mod live;
#[path = "m13_mcp.rs"]
mod mcp;
#[path = "m13_network.rs"]
mod network;
#[path = "m13_people.rs"]
mod people;
#[path = "m13_places.rs"]
mod places;

use std::time::Instant;

use app::secs;

#[test]
fn faces_are_found_in_the_background_named_in_one_step_and_kept_without_a_connection_or_a_name_in_the_log() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    faces::faces_and_people(&exe, home.path());
    println!("M13 faces and the People group: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn suggestions_faces_by_hand_people_and_their_regions_in_the_files() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    people::people(&exe, home.path());
    println!("M13 suggestions, the People pane and face regions: passed in {:.1} s", secs(started));
}

#[test]
fn photos_dragged_onto_the_map_are_geotagged_in_one_step_written_to_the_files_and_cleared() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    places::places(&exe, home.path());
    println!("M13 places and geotagging: passed in {:.1} s", secs(started));
}

#[test]
fn no_connection_without_the_users_action() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    network::network(&exe, home.path());
    println!("M13 no network without the user's action: passed in {:.1} s", secs(started));
}

#[test]
fn w13_and_w14_over_stdio() {
    let started = Instant::now();
    let home = tempfile::tempdir().unwrap();
    mcp::w13(home.path());
    mcp::w14(home.path());
    println!("M13 W13 and W14: passed in {:.1} s", secs(started));
}

#[test]
fn the_crate_suites_the_workflows_panic_hunt_the_corpus_and_the_budgets() {
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
    checks::corpus(in_ci);
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
            "(the release budgets, face inference, the face scan over 1 000 photos, the people and places views at 10 000 and every effect's draft, final and Save: run with {}=1)",
            checks::RELEASE
        );
    }
    println!("{rate}");
    println!("suites and workflows {suites:.1} s, panic_hunt {hunt:.1} s, debug benches {bench:.1} s; M13 checks: passed in {:.1} s", secs(started));
}
