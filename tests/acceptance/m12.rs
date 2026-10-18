//! M12 "Effects" (`docs/roadmap.md` → M12 → the *Done when* of Effect tiles and panels, Tab 3
//! effects and Tab 4 and 5 effects; run by `cargo xtask accept M12`).
//!
//! The live parts run the real binaries on fresh libraries in temporary homes: `dalica --headless`
//! driven over its control channel (`m11_live.rs`'s process and connection, `m9_app.rs`'s waits and
//! lookups) and `dalica-cli mcp` over real stdio:
//!
//! 1. **The tile grids** (`m12_tiles.rs`): on a 6 MP photo each effect tab's grid lists its tiles
//!    in the catalogue's order with their labels, the window keeps drawing while their previews are
//!    on their way (frames run with the tiles pending, the UI's frames stay short), and every tile
//!    ends up holding its preview. The three tabs and an open panel (Duo-Tone) match their
//!    whole-window fingerprints, taken once every tile and the final preview are in.
//! 2. **The 27 effects** (`m12_effects.rs`): one photo per effect, each effect applied from its
//!    tile (the four one-click ones) or its panel's Apply (the 23 others, each first opened, changed
//!    and cancelled: Cancel records nothing) is one step of the photo's history named after it
//!    ("Undo Sepia"); every edited thumbnail differs from the unedited photo's; every recipe
//!    survives a restart and an index rebuild.
//! 3. **On the photo** (`m12_focal.rs`): the crosshair of Soft Focus, Focal B&W, Graduated Tint
//!    and Focal Zoom dragged on a photo turned 90° lands, applied, where it was put (in the stored
//!    frame); Soften painted then partly erased softens the saved picture only where the paint
//!    stayed.
//! 4. **A frame's grown size** (`m12_frame.rs`): a Border's photo shows, exports and saves at the
//!    photo grown by the Border's bands on every side, the photo's pixels unchanged inside; the
//!    original is kept in `.dalica-originals/`, and an export after the Save frames it once.
//! 5. **Batch Edit** (`m12_batch.rs`): Picture ▸ Batch Edit ▸ Sepia on 20 photos is one undo step
//!    "Sepia"; one Ctrl+Z takes all 20 back, in the library and the sidecar.
//! 6. **MCP** (`m12_mcp.rs`): W11 through `dalica-cli mcp` over stdio, `list_edit_ops` listing the
//!    27 effects, every one available.
//! 7. **The checks** (`m12_checks.rs`): the crate suites behind the three areas' *Done when* (a
//!    fingerprint and an invariant per effect at two render sizes and in a zoomed region, the
//!    grids, panels, crosshair, brush and Batch Edit; skipped inside `cargo xtask ci`, whose test
//!    step ran them), the workflow pass rate with its floor of 12 (`cargo xtask stats --check`,
//!    skipped there too), `panic_hunt`, and M12's budgets (M12.21): in a debug build the `effects`,
//!    `effect_tiles` and `slideshow` smoke passes check the paths and print their times; with
//!    `DALICA_ACCEPT_RELEASE=1` the release benches are judged as `cargo xtask budgets` judges them
//!    (every effect's 24 MP Save and the empty recipe, the three tabs' tiles, the slideshow's upload
//!    frame). The per-effect draft and final caps on the screen are M13's speed tasks (the
//!    maintainer at M12.R) and are printed, not judged.
//!
//! Fingerprints are whole windows at 1400 × 900 (`fingerprints/m12-1400x900-*.json`). The manual
//! checks (every effect on real photos at Fit and 1:1, the colour picker and the eyedropper, the
//! crosshair dragged with a mouse and a touchpad, Soften painted, a framed photo saved and opened
//! in another application) are in `m12.md`.

#[path = "m9_app.rs"]
#[allow(dead_code)]
mod app;
#[path = "m12_batch.rs"]
mod batch;
#[path = "m12_checks.rs"]
mod checks;
#[path = "m12_common.rs"]
mod common;
#[path = "m12_effects.rs"]
mod effects;
#[path = "m12_focal.rs"]
mod focal;
#[path = "m12_frame.rs"]
mod frame;
#[allow(dead_code)]
#[path = "m11_live.rs"]
mod live;
#[path = "m12_mcp.rs"]
mod mcp;
#[path = "m12_tiles.rs"]
mod tiles;

use std::time::Instant;

use app::secs;

#[test]
fn every_tab_fills_with_previews_without_waiting_and_matches_its_fingerprint() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    tiles::tiles(&exe, home.path());
    println!("M12 tile grids: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn each_of_the_27_effects_is_one_named_step_shown_on_its_thumbnail_and_kept_over_a_restart_and_a_rebuild() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    effects::effects(&exe, home.path());
    println!("M12 the 27 effects: passed in {:.1} s", secs(started));
}

#[test]
fn a_crosshair_on_a_turned_photo_lands_where_it_was_put_and_soften_applies_only_where_painted() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    focal::focal(&exe, home.path());
    println!("M12 crosshair and Soften: passed in {:.1} s", secs(started));
}

#[test]
fn a_border_shows_saves_and_exports_at_the_grown_size() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    frame::frame(&exe, home.path());
    println!("M12 a frame's grown size: passed in {:.1} s", secs(started));
}

#[test]
fn batch_edit_sepia_on_20_photos_is_one_undo_step() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    batch::batch(&exe, home.path());
    println!("M12 Batch Edit ▸ Sepia: passed in {:.1} s", secs(started));
}

#[test]
fn w11_over_stdio() {
    let started = Instant::now();
    let home = tempfile::tempdir().unwrap();
    mcp::w11(home.path());
    println!("M12 W11: passed in {:.1} s", secs(started));
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
        println!(
            "(the release budgets, the 27 effects' 24 MP Saves, the tile grids of a 24 MP photo and the slideshow's upload frame: run with {}=1)",
            checks::RELEASE
        );
    }
    println!("{rate}");
    println!("suites and workflows {suites:.1} s, panic_hunt {hunt:.1} s, debug benches {bench:.1} s; M12 checks: passed in {:.1} s", secs(started));
}
