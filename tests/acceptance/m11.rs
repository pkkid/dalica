//! M11 "Import, compare and viewing" (`docs/roadmap.md` → M11 → the *Done when* of Rooms and import,
//! Compare and batch edit, Slideshow and quick preview, and Photo viewer; run by
//! `cargo xtask accept M11`).
//!
//! The live parts run the real binaries on fresh libraries in temporary homes: `dalica --headless`
//! driven over its control channel (`m11_live.rs`'s process and connection, which takes the extra
//! arguments and environment M11 needs, and `m9_app.rs`'s waits and lookups), `dalica --view FILE
//! --headless`, and `dalica-cli mcp` over real stdio:
//!
//! 1. **Rooms and import** (`m11_import.rs`): a simulated camera card (a temporary mount folder
//!    named by `DALICA_DEVICE_ROOTS`) opened in the Import room with Ctrl+M and imported into
//!    Date taken folders with every copy verified (bytes and time); a planted name clash gets
//!    `-001` and the planted file is untouched; the removal question comes only once the import is
//!    over and verified, and is refused over the control channel on every route; the same card
//!    again lists every picture as a duplicate and copies nothing. The room and its tabs match
//!    their fingerprint.
//! 2. **Compare and batch edit** (`m11_edit.rs`): Picture ▸ Batch Edit ▸ Auto Fix on 100 photos is
//!    one undo step and one Ctrl+Z restores every recipe (library and sidecar); Auto Fix and Auto
//!    Contrast on A│A's right side stay off the photo until Right is kept, which is one step of
//!    its history. A│A matches its fingerprint.
//! 3. **Slideshow** (`m11_slideshow.rs`): in the live app the keys, the wheel and the bar of
//!    `viewing.md` §1.3–§1.4, the bar matching its fingerprint; in the app on a virtual clock
//!    (`dalica_ui::Driver`) each transition at its midpoint matching its fingerprint, and 30
//!    changes with every decode slowed to 1.5 s (`Session::plant_decode_delay`), none waiting.
//! 4. **Photo viewer** (`m11_viewer.rs`): `dalica --view FILE --headless` opens the viewer on the
//!    photo (matching its fingerprint), steps through its folder, stars it into the folder's
//!    sidecar and opens it in the edit view.
//! 5. **MCP** (`m11_mcp.rs`): W10 and W07 through `dalica-cli mcp` over stdio, checked on disk.
//! 6. **The checks** (`m11_checks.rs`): the crate suites behind the four areas' *Done when*
//!    (skipped inside `cargo xtask ci`, whose test step ran them), the workflow pass rate with its
//!    floor of 11 (`cargo xtask stats --check`, skipped there too), `panic_hunt`, and M11's budgets:
//!    in a debug build the `import`, `batch_edit` and `slideshow` smoke passes check the paths and
//!    print their times (the slideshow's zero waits asserted); with `DALICA_ACCEPT_RELEASE=1` the
//!    release benches are judged as `cargo xtask budgets` judges them.
//!
//! Fingerprints are whole windows at 1400 × 900 (`fingerprints/m11-1400x900-*.json`). The manual
//! checks (a real card or USB stick, a slideshow on a real screen, the viewer from the file
//! manager's Open with and its translucent backdrop, Ctrl+Alt over a thumbnail) are in `m11.md`.

#[path = "m9_app.rs"]
#[allow(dead_code)]
mod app;
#[path = "m11_checks.rs"]
mod checks;
#[path = "m11_edit.rs"]
mod edit;
#[path = "m11_import.rs"]
mod import;
#[allow(dead_code)]
#[path = "m11_live.rs"]
mod live;
#[path = "m11_mcp.rs"]
mod mcp;
#[path = "m11_slideshow.rs"]
mod slideshow;
#[path = "m11_viewer.rs"]
mod viewer;

use std::time::Instant;

use app::secs;

#[test]
fn a_card_imports_into_dated_folders_and_a_second_import_skips_every_photo() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    import::import(&exe, home.path());
    println!("M11 import: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn a_batch_of_100_is_one_undo_step_and_a_version_kept_from_a_a_is_one_history_step() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    edit::edit(&exe, home.path());
    println!("M11 compare and batch edit: passed in {:.1} s", secs(started));
}

#[test]
fn the_slideshow_keys_wheel_and_bar_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    slideshow::keys_wheel_and_bar(&exe, home.path());
    println!("M11 slideshow in the live app: passed in {:.1} s", secs(started));
}

#[test]
fn each_transition_at_its_midpoint_and_no_change_waits_on_a_slowed_decoder() {
    let started = Instant::now();
    slideshow::transitions_and_a_slowed_decoder();
    println!("M11 transitions and prefetch: passed in {:.1} s", secs(started));
}

#[test]
fn dalica_view_opens_a_photo_steps_its_folder_stars_it_and_opens_it_in_the_edit_view() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    viewer::viewer(&exe, home.path());
    println!("M11 viewer: passed in {:.1} s", secs(started));
}

#[test]
fn w07_and_w10_over_stdio() {
    let started = Instant::now();
    let home = tempfile::tempdir().unwrap();
    mcp::workflows(home.path());
    println!("M11 workflows: passed in {:.1} s", secs(started));
}

#[test]
fn the_crate_suites_the_workflows_panic_hunt_and_the_budgets() {
    let started = Instant::now();
    let rate = if dalica_acceptance::suites_ran() {
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
        println!("(the release budgets, 1 000 photos imported, Auto Fix on 1 000 and 24 MP slideshows: run with {}=1)", checks::RELEASE);
    }
    println!("{rate}");
    println!("suites and workflows {suites:.1} s, panic_hunt {hunt:.1} s, debug benches {bench:.1} s; M11 checks: passed in {:.1} s", secs(started));
}
