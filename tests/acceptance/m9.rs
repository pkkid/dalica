//! M9 "Albums, search and file operations" (`docs/roadmap.md` → M9 → the *Done when* of Albums,
//! Search and filters, and File operations; run by `cargo xtask accept M9`).
//!
//! Each part runs the real `dalica` binary in `--headless` mode on a fresh library in a temporary
//! home, driven over its control channel as an agent would (`m6_live.rs`'s process and
//! connection, `m9_app.rs`'s waits and lookups):
//!
//! 1. **Albums** (`m9_albums.rs`): an album over three folders in a custom order; `album.create`,
//!    `album.info.set`, `album.add`, `album.reorder`, `album.remove` (refused without `confirm`),
//!    `album.setCover`, Sort Album By and Reverse Order (`section.sort`) and `album.delete`
//!    (refused without `confirm`), each undone and redone with the album's order and fields
//!    checked, the members' sidecar entries on disk; a reorder by dragging in the album's section,
//!    undone and redone; the album in the tree and its section (`fingerprints/m9-*-albums.json`);
//!    `data-model.md` §10.12: the app closed, `albums.json`, `index.log` and `index.snap` deleted,
//!    the library rebuilt: the album and its id come back with the exact order and its name.
//! 2. **Search and filters** (`m9_search.rs`): the grammar over `library.query` (words, prefixes
//!    and case, `-word`, `tag:`, `camera:`, every date spelling, pixel sizes, album and folder
//!    names, a folder's description, an unknown `name:value`, `person:` and `color:` reported
//!    unavailable); the filters and the age alone and with text; search mode from `search.set`
//!    and from the toolbar's field: the banner's numbers equal the matches' files on disk and its
//!    text reads as `ui-layout.md` §9 writes it, in the singular too, only the matching sections
//!    listed (`fingerprints/m9-*-search.json`); Save Search as Album in light-table order and its
//!    undo; Show All; Find Duplicates' groups, largest first, and its banner.
//! 3. **File operations** (`m9_files.rs`): `data-model.md` §10.3, §10.7 and §10.2, and every
//!    operation with its undo, in the window too: the drag pill (`fingerprints/m9-*-drag.json`),
//!    the first-time move question (`m9-*-move.json`) and "Don't ask again", the collision
//!    question from Copy and Paste (`m9-*-collision.json`), Rename Files from F2
//!    (`m9-*-rename.json`), the delete question from the Delete key (`m9-*-delete.json`) and the
//!    trash; and a move across two file systems that copies, verifies, then sends the source to
//!    the trash, undone: the live app between its temporary folder and `/dev/shm` where they are
//!    two devices, else the engine in this process with the cross-volume path forced (it prints
//!    which ran).
//! 4. **The crate suites, run as they are** (`m9_checks.rs`): every M9 suite behind the three
//!    areas' *Done when*, the grammar tests and the 100 k index and worker among them. Inside
//!    `cargo xtask ci` its test step already ran them, so they are skipped there
//!    (`dalica_acceptance::SUITES_RAN`). `panic_hunt -- --ignored` always runs. Then **the
//!    `search` bench**: in a debug build its smoke pass (2 000 photos) checks the path and prints
//!    its times; with `DALICA_ACCEPT_RELEASE=1` the synthetic 100 k library in release, each
//!    median within the budgets `cargo xtask budgets` holds it to (index build ≤ 630 ms, one word
//!    ≤ 1.7 ms, three words and a filter ≤ 0.6 ms, a `-word` ≤ 5.1 ms, `search.set` to applied
//!    ≤ 22.3 ms).
//!
//! The manual checks (an album from three folders reordered by hand, a search of your own photos,
//! photos dragged onto a folder and undone, a batch renamed, a delete to the trash restored) are
//! in `m9.md`.

#[path = "m9_albums.rs"]
mod albums;
#[path = "m9_app.rs"]
mod app;
#[path = "m9_checks.rs"]
mod checks;
#[path = "m9_files.rs"]
mod files;
#[allow(dead_code)]
#[path = "m6_live.rs"]
mod live;
#[path = "m9_search.rs"]
mod search;

use std::time::Instant;

use app::secs;

#[test]
fn albums_their_undo_and_an_order_that_survives_a_rebuild_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    albums::albums(&exe, home.path());
    println!("M9 albums: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn search_the_grammar_the_banner_and_duplicates_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    search::search(&exe, home.path());
    println!("M9 search: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn file_operations_their_undo_and_the_data_lessons_in_the_live_app() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let build = secs(started);
    let home = tempfile::tempdir().unwrap();
    files::file_operations(&exe, home.path());
    println!("M9 file operations: passed in {:.1} s (build {build:.1} s)", secs(started));
}

#[test]
fn a_move_across_two_file_systems_copies_verifies_then_trashes() {
    let started = Instant::now();
    let exe = live::dalica_exe();
    let which = files::two_volumes(&exe);
    println!("M9 two volumes ({which}): passed in {:.1} s", secs(started));
}

#[test]
fn the_crate_suites_panic_hunt_and_the_search_budgets() {
    let started = Instant::now();
    if !dalica_acceptance::suites_ran() {
        checks::crate_suites();
    }
    let suites = secs(started);
    let t = Instant::now();
    checks::panic_hunt();
    let hunt = secs(t);
    let t = Instant::now();
    checks::search_debug();
    let bench = secs(t);
    if std::env::var_os(checks::RELEASE).is_some_and(|v| v == "1") {
        let t = Instant::now();
        let [index, word, filter, not, applied] = checks::search_release();
        println!(
            "release budgets ({:.1} s), 100 k photos: index build {index:.1} ms, one word {word:.3} ms, three words and a filter {filter:.3} ms, a -word {not:.3} ms, search.set to applied {applied:.2} ms",
            secs(t)
        );
    } else {
        println!("(the release budgets, the search of 100 k photos: run with {}=1)", checks::RELEASE);
    }
    println!("suites {suites:.1} s, panic_hunt {hunt:.1} s, debug bench {bench:.1} s; M9 checks: passed in {:.1} s", secs(started));
}
