//! Faces and the People group in the live app (`m13.rs` part 1): `dalica --headless` on a library
//! of fourteen photos with painted faces, every connection it tries counted by the loopback proxy
//! and its log at `RUST_LOG=trace`:
//!
//! - start-up and the folders' scans make no connection; with only a detector the face scan has no
//!   model;
//! - the recogniser installed from a local file through the face model dialog (Add Model waits for
//!   the box); the face scan then runs in the background: frames go on while it works and none
//!   comes near the time it takes, and it fills Unnamed with the painted faces in groups, largest
//!   first (10 red, 8 blue, 6 green, 4 yellow); the Unnamed grid matches its fingerprint;
//! - the red group named in the grid ("Add a name", a new name through New Person) is one undo
//!   step: one Undo leaves all ten unnamed, Redo names them again;
//! - the blue group ignored with its ×, and the faces found in Porto removed (the Folder Manager's
//!   Remove Faces); the person's section matches its fingerprint;
//! - after a restart (the scan runs again over its cache) and after an index rebuild, the person
//!   and her ten faces are there, the ignored faces still ignored and the removed ones still gone,
//!   nothing found twice;
//! - no connection was made at any point; every log line of the three runs is free of the person's
//!   name and nickname, and the face code's lines hold no rectangle or vector.

use std::path::Path;
use std::time::Instant;

use serde_json::json;

use crate::app::{self, click, dialog, run, secs};
use crate::common::{self, LARGE, Proxy, Rgb, faces, groups, person, scan_done, scan_status, steady_shot, undo_state};
use crate::live::{self, Client};

const RED: Rgb = [220, 40, 40];
const BLUE: Rgb = [40, 40, 220];
const GREEN: Rgb = [40, 220, 40];
const YELLOW: Rgb = [220, 220, 40];
/// The person's name and nickname: words no log line would hold by chance.
const NAME: &str = "Zephyrine";
const NICKNAME: &str = "Quillfeather";
/// The photos' size: large enough that the scan takes a while in a debug build.
const PHOTO: (u32, u32) = (1600, 1200);

/// The faces of each kind: (named as `NAME`, unnamed, ignored).
fn tally(c: &mut Client, who: &str) -> (usize, usize, usize) {
    let all = faces(c, json!({ "ignored": true, "limit": 10000 }));
    let named = all.iter().filter(|f| f["person"].as_str() == Some(who)).count();
    let unnamed = all.iter().filter(|f| f["person"].is_null() && f["ignored"] == false).count();
    let ignored = all.iter().filter(|f| f["ignored"] == true).count();
    (named, unnamed, ignored)
}

/// Runs frames until the face scan is idle; (frames drawn while it worked, its wall time in ms,
/// the window's longest frame and p95 meanwhile in ms).
fn frames_while_scanning(c: &mut Client) -> (u64, f64, f64, f64) {
    c.ok("ui.inspect", json!({ "resetPerf": true }));
    let started = Instant::now();
    let mut working_frames = 0;
    loop {
        let s = c.ok("ui.settle", json!({ "timeoutMs": 17 }));
        let status = scan_status(c);
        if status["state"] == "idle" && status["model"].is_string() {
            break;
        }
        working_frames += s["frames"].as_u64().unwrap_or(0);
        assert!(started.elapsed() < common::PATIENCE, "the face scan never finished: {status}");
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let perf = c.ok("ui.inspect", json!({}))["perf"].clone();
    let max = perf["maxFrameMs"].as_f64().unwrap_or_else(|| panic!("ui.inspect perf: {perf}"));
    let p95 = perf["frameP95Ms"].as_f64().unwrap_or(f64::NAN);
    (working_frames, ms, max, p95)
}

/// Checks the library after a restart or a rebuild: the person and her ten faces, the eight ignored
/// faces, Porto's faces gone, the six green ones unnamed in one group.
fn kept(c: &mut Client, who: &str, when: &str) {
    let status = scan_done(c);
    let p = person(c, NAME).unwrap_or_else(|| panic!("{when}: {NAME} is still there"));
    assert_eq!((p["id"].as_str(), p["count"].as_u64(), p["nickname"].as_str()), (Some(who), Some(10), Some(NICKNAME)), "{when}: {p}");
    assert_eq!(tally(c, who), (10, 6, 8), "{when}: ten named, six unnamed (green), eight ignored (blue): nothing came back");
    let g = groups(c);
    assert_eq!(g.iter().map(Vec::len).collect::<Vec<_>>(), [6], "{when}: only the green group is left to name: {g:?}");
    assert_eq!(status["stats"]["found"], json!(0), "{when}: nothing was found again: {status}");
}

pub fn faces_and_people(exe: &Path, home: &Path) {
    let started = Instant::now();
    let proxy = Proxy::start();
    let models = common::models(home);
    let file = home.join("Downloads").join("my-recogniser.onnx");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, common::recogniser_bytes()).unwrap();
    let party = home.join("Pictures").join("Party");
    let porto = home.join("Pictures").join("Porto");
    let kinds = [
        (common::faces_jpeg(PHOTO.0, PHOTO.1, RED, BLUE), 6),
        (common::faces_jpeg(PHOTO.0, PHOTO.1, RED, GREEN), 4),
        (common::faces_jpeg(PHOTO.0, PHOTO.1, GREEN, BLUE), 2),
    ];
    let mut n = 0;
    for (bytes, count) in &kinds {
        for _ in 0..*count {
            n += 1;
            app::put(&party.join(format!("IMG_{n:04}.jpg")), bytes, n);
        }
    }
    let yellow = common::faces_jpeg(PHOTO.0, PHOTO.1, YELLOW, YELLOW);
    for i in 1..=2u64 {
        app::put(&porto.join(format!("IMG_{i:04}.jpg")), &yellow, 100 + i);
    }
    let launch = || common::launch(&proxy, &models).env("RUST_LOG", "trace");
    let mut logs = String::new();

    let (a, mut cl) = live::start_with(exe, home, launch());
    let c = &mut cl;
    app::resize(c, LARGE);
    run(c, "folders.add", json!({ "path": party }));
    run(c, "folders.add", json!({ "path": porto }));
    let status = scan_status(c);
    assert!(status["model"].is_null() && status["state"] == "noModel", "a detector alone: no face scan: {status}");
    assert_eq!(proxy.connections(), Vec::<String>::new(), "start-up and the scans made no connection");

    // The recogniser, from a local file through the dialog; the scan runs in the background.
    let model = common::install_recogniser(c, &models, &file);
    let (working, ms, max, p95) = frames_while_scanning(c);
    assert!(working >= 3, "frames went on while the face scan worked ({working} in {ms:.0} ms)");
    assert!(max < ms / 2.0, "a frame took {max:.0} ms of the {ms:.0} ms the scan took: the window waited");
    let status = scan_done(c);
    assert_eq!((status["model"].as_str(), status["detector"].clone()), (Some(model.as_str()), json!(true)), "{status}");
    assert_eq!((status["stats"]["searched"].clone(), status["stats"]["found"].clone()), (json!(14), json!(28)), "{status}");
    println!(
        "  the recogniser installed from a file after its terms; the face scan found 28 faces on 14 photos in {ms:.0} ms with {working} frames drawn meanwhile (longest {max:.0} ms, p95 {p95:.0} ms)"
    );
    if dialog(c).is_some() {
        c.ok("ui.dialog.cancel", json!({}));
    }
    let g = groups(c);
    assert_eq!(g.iter().map(Vec::len).collect::<Vec<_>>(), [10, 8, 6, 4], "the groups follow the paint, largest first: {g:?}");
    let unnamed = faces(c, json!({ "unnamed": true, "limit": 1000 }));
    let left = |id: &String| unnamed.iter().find(|f| f["id"] == id.as_str()).and_then(|f| f["rect"][0].as_f64()).unwrap() < 0.5;
    assert!(g[0].iter().all(left), "the ten red faces are the left ones");

    // The Unnamed grid.
    click(c, "tree:person:unnamed");
    if dialog(c).is_some() {
        c.ok("ui.dialog.cancel", json!({}));
    }
    for i in 0..4 {
        assert!(app::widget(c, &format!("group:{i}")).is_some(), "the grid shows group {i}");
    }
    steady_shot(c, home, "m13-1400x900-unnamed");

    // The red group named, through New Person: one undo step.
    let (depth, _) = undo_state(c);
    c.ok("ui.clickWidget", json!({ "id": "field:faces.name/0" }));
    c.ok("ui.type", json!({ "text": NAME }));
    app::key(c, "Enter");
    assert_eq!(dialog(c).as_deref(), Some("newPerson"), "a new name asks New Person");
    c.ok("ui.clickWidget", json!({ "id": "field:dialog.nickname" }));
    c.ok("ui.type", json!({ "text": NICKNAME }));
    click(c, "button:dialog.ok");
    let who = person(c, NAME).unwrap_or_else(|| panic!("{NAME} was made"))["id"].as_str().unwrap().to_string();
    assert_eq!(tally(c, &who), (10, 18, 0), "the whole group named");
    let (after, label) = undo_state(c);
    assert_eq!(after, depth + 1, "naming the group is one undo step ({label})");
    run(c, "edit.undo", json!({}));
    assert_eq!(faces(c, json!({ "unnamed": true, "limit": 1000 })).len(), 28, "one Undo leaves all ten unnamed");
    run(c, "edit.redo", json!({}));
    assert_eq!(tally(c, &who), (10, 18, 0), "Redo names them again");
    println!("  the red group named {NAME} in the Unnamed grid through New Person: one undo step");

    // The blue group ignored with its ×; Porto's faces removed.
    let g = groups(c);
    assert_eq!(g.iter().map(Vec::len).collect::<Vec<_>>(), [8, 6, 4], "{g:?}");
    click(c, "group:0/ignore");
    assert!(app::widget(c, "button:dialog.ignore").is_some(), "× asks first");
    click(c, "button:dialog.ignore");
    let removed = run(c, "faces.removeInFolders", json!({ "folders": [porto], "confirm": true }));
    assert_eq!(removed, json!({ "photos": 2, "removed": 4 }), "the faces found in Porto");
    assert_eq!(tally(c, &who), (10, 6, 8), "ten named, the green six unnamed, the blue eight ignored");

    // The person's section.
    click(c, &format!("tree:person:{who}"));
    steady_shot(c, home, "m13-1400x900-person-section");
    live::quit(a, cl);
    logs.push_str(&common::log_of(home));

    // A restart: the scan runs again over its cache; nothing comes back.
    let (a, mut cl) = live::start_with(exe, home, launch());
    let c = &mut cl;
    kept(c, &who, "after a restart");
    // An index rebuild: the faces and the person come back from the sidecars and people.json.
    run(c, "library.rebuild", json!({ "confirm": true }));
    kept(c, &who, "after an index rebuild");
    live::quit(a, cl);
    logs.push_str(&common::log_of(home));
    println!("  after a restart and after an index rebuild: {NAME} with her ten faces, the eight ignored still ignored, Porto's four still gone");

    assert_eq!(proxy.connections(), Vec::<String>::new(), "no connection at any point: start-up, the scans, the face scan, the rebuild");
    common::privacy(&logs, &[NAME, NICKNAME], &[party.clone(), porto.clone()]);
    println!("  M13 faces and the People group in {:.1} s", secs(started));
}
