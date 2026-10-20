//! M15 "Video, localisation, OS integration and Release 1.0" (`docs/roadmap.md` → M15 → the
//! *Done when* of Video, Localisation and accessibility, OS integration and Release 1.0; run by
//! `cargo xtask accept M15`).
//!
//! The live parts run the real binaries on fresh libraries in temporary homes: `dalica --headless`
//! driven over its control channel (`m11_live.rs`'s process and connection on a port the OS picks,
//! `m9_app.rs`'s waits and lookups, `m14_common.rs`'s painted photos and steady shots) and
//! `dalica-cli mcp` over real stdio. What only a person's input or a system service can show runs
//! in process on `dalica_ui`'s headless `Driver` with the recording `Os` or the in-memory PTP
//! camera. Every photo and clip is procedural (`m15_clips.rs`: H.264 with every macroblock I_PCM,
//! muxed by hand into MP4, MOV and Matroska).
//!
//! 1. **Video** (`m15_video.rs`): a folder with an MP4, a MOV and an MKV lists three videos of
//!    2 s whose cells show their posters, the Videos filter shows only videos, Export writes their
//!    first frames and counts the clip it cannot play, the slideshow counts a clip and plays it to
//!    its end, a hostile container is listed and refused without a panic; the player plays,
//!    pauses, steps a frame and trims, Save Frame as Picture writes the frame shown into Video
//!    Frames, the trim survives a restart; an MPEG-4 Part 2 clip and (in a build without the
//!    `hevc` feature) an HEVC one show "This video can't be played" with Open With.
//! 2. **Languages and accessibility** (`m15_i18n.rs`): English and Thai switched without a restart
//!    in the library, the edit view and Options, keeping the view, the selection and the undo
//!    history; plurals for 0, 1, 2 and 5; the menu bar inside the window at 900 × 600; the
//!    pseudo-locale leaves nothing unbracketed but user data; F10 and the arrows walk the menu bar
//!    and Tab cycles a dialog's controls.
//! 3. **The OS services** (`m15_os.rs`): through the recording `Os`, a person's Locate on Disk,
//!    folder icon, Open in External Editor, Open With ▸, Copy Path and Ctrl+C hand the right paths;
//!    over automation the launching ones are refused whatever was granted, and a headless app has
//!    no clipboard.
//! 4. **One Dalica** (`m15_instance.rs`): a second `dalica --view FILE` hands FILE to the first and
//!    exits 0; the first's viewer shows it.
//! 5. **A camera** (`m15_camera.rs`): the in-memory PTP responder in the Import room, its pictures
//!    imported byte for byte with the camera's dates as their times.
//! 6. **MCP** (`m15_mcp.rs`): W17 through `dalica-cli mcp` over stdio, checked on disk.
//! 7. **The checks** (`m15_checks.rs`): the crate suites behind the four areas' *Done when*
//!    (skipped inside `cargo xtask ci`, whose test step ran them) with the AccessKit audit, the
//!    keyboard walk and the cut-and-wrap sweeps, HEVC with its feature, `cargo xtask i18n`, the
//!    workflow pass rate with its floor of 17 (`cargo xtask stats --check`), `cargo xtask parity`
//!    in its review form, the SBOMs, `panic_hunt`, and M15's budgets: in a debug build the
//!    `video_probe`, `video_play`, `camera_import`, `a11y_frame` and `i18n_frame` smoke passes check
//!    the paths and print their times; with `DALICA_ACCEPT_RELEASE=1` the release benches judge
//!    every row on its budget.
//!
//! Fingerprints are whole windows at 1400 × 900 (`fingerprints/m15-1400x900-*.json`): the
//! library with video cells, the player, and the library in English and (with the fallback faces,
//! `CRAFT_FONTS_DIR`) in Thai. The release dry run that signs every Windows and macOS artifact
//! with throwaway identities is the Release workflow's (`.github/workflows/release.yml`) on a
//! packaging pull request; the checks by hand on each OS (a screen reader, an input method, the
//! real programs, a drag out, the default viewer, a real camera, real sound, the Thai review, the
//! signed packages) are in `m15.md`.

#[path = "m9_app.rs"]
#[allow(dead_code)]
mod app;
#[path = "m15_camera.rs"]
mod camera;
#[path = "m15_checks.rs"]
mod checks;
#[path = "m15_clips.rs"]
mod clips;
#[path = "m14_common.rs"]
#[allow(dead_code)]
mod common;
#[path = "m15_i18n.rs"]
mod i18n;
#[path = "m15_instance.rs"]
mod instance;
#[allow(dead_code)]
#[path = "m11_live.rs"]
mod live;
#[path = "m15_mcp.rs"]
mod mcp;
#[path = "m15_os.rs"]
mod os;
#[path = "m15_video.rs"]
mod video;

use std::time::Instant;

use app::secs;

#[test]
fn videos_listed_filtered_exported_played_trimmed_saved_and_shown_in_a_slideshow() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    video::videos(&exe, home.path());
    println!("M15 video: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn english_and_thai_without_a_restart_the_pseudo_locale_and_the_keyboard() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    i18n::languages(&exe, home.path());
    println!("M15 languages and the keyboard: passed in {:.1} s", secs(started));
}

#[test]
fn the_os_services_by_hand_through_the_recording_os_and_refused_over_automation() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    os::by_hand(&home.path().join("window"));
    os::over_automation(&exe, &home.path().join("headless"));
    println!("M15 OS services: passed in {:.1} s", secs(started));
}

#[test]
fn a_second_dalica_view_hands_its_file_to_the_first_and_exits() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let home = tempfile::tempdir().unwrap();
    instance::handoff(&exe, home.path());
    println!("M15 one Dalica: passed in {:.1} s", secs(started));
}

#[test]
fn a_camera_lists_its_pictures_and_imports_them_verified_with_their_times() {
    let started = Instant::now();
    let home = tempfile::tempdir().unwrap();
    camera::import(home.path());
    println!("M15 camera import: passed in {:.1} s", secs(started));
}

#[test]
fn w17_over_stdio() {
    let started = Instant::now();
    let home = tempfile::tempdir().unwrap();
    mcp::w17(home.path());
    println!("M15 W17: passed in {:.1} s", secs(started));
}

#[test]
fn the_crate_suites_the_workflows_parity_panic_hunt_and_the_budgets() {
    let started = Instant::now();
    let in_ci = dalica_acceptance::suites_ran();
    let rate = if in_ci {
        "the crate suites and the workflows: run by the ci test step and its stats --check".to_string()
    } else {
        checks::crate_suites();
        checks::workflows()
    };
    let suites = secs(started);
    let t = Instant::now();
    checks::catalogs();
    checks::parity();
    let sbom = tempfile::tempdir().unwrap();
    checks::sbom(sbom.path());
    let tools = secs(t);
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
            "(the release budgets, 1 000 clips probed, a 1080p clip's poster, step and play, 300 pictures from a camera, the library's frame at 100 000 photos with AccessKit on and in the pseudo-locale: run with {}=1)",
            checks::RELEASE
        );
    }
    println!("{rate}");
    println!(
        "suites and workflows {suites:.1} s, i18n, parity and SBOM {tools:.1} s, panic_hunt {hunt:.1} s, debug benches {bench:.1} s; M15 checks: passed in {:.1} s",
        secs(started)
    );
}
