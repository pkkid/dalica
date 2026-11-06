//! Options → File Types through `options.set` (`roadmap.md` M10.5; `menus-and-shortcuts.md` §5,
//! `data-model.md` §7) on temporary folders: turning PNG off starts one scan and the PNGs leave the
//! library; 31 days later on a virtual clock, after a sidecar write in their folder, their entries
//! are still on disk and not marked missing (their files are there); turning PNG back on brings
//! them back with their stars, captions and recipes. The small-picture threshold hides and shows
//! a 40 px photo in what the library lists. The session names the rescan an applied change
//! started (`Session::rescanning_for_file_types`, M16.22) until its job ends, and only that scan.

use std::collections::BTreeMap;
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::watching::WatchMode;
use dalica_engine::{JobId, JobKind, JobState, Session};
use dalica_fs::Expect;
use serde_json::json;

use crate::common::{jpeg, jpeg_of, lisbon, off_owner, open_with, read_json, virtual_clock};

fn by_name(s: &Session) -> BTreeMap<String, PhotoId> {
    s.library().photos().map(|p| (p.name.clone(), p.id)).collect()
}

fn scans(s: &Session) -> usize {
    s.jobs().iter().filter(|j| j.kind == JobKind::Scan).count()
}

#[test]
fn a_type_turned_off_leaves_the_library_keeps_its_sidecar_entries_and_comes_back_whole() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Harbour");
    dalica_fs::create_dir_all(&folder).unwrap();
    dalica_fs::replace(&folder.join("a.jpg"), &jpeg(1), Expect::Absent).unwrap();
    for (seed, name) in [(2, "b.png"), (3, "c.png")] {
        let png = dalica_scenes::png(&dalica_scenes::Scene::new(seed, 96, 64)).unwrap();
        dalica_fs::replace(&folder.join(name), &png, Expect::Absent).unwrap();
    }
    let clock = virtual_clock();
    let mut s = open_with(lib.path(), clock.clone());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = by_name(&s);
    assert_eq!(ids.keys().collect::<Vec<_>>(), ["a.jpg", "b.png", "c.png"]);
    s.execute("photo.star.set", &json!({ "ids": [ids["b.png"]], "on": true })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids["c.png"]], "caption": "Fishing boats" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids["b.png"], "op": "sepia()" })).unwrap();
    s.flush().unwrap();

    // Off: one scan, and the PNGs leave the library.
    let before = scans(&s);
    let v = s.execute("options.set", &json!({ "values": { "fileTypes.png": false } })).unwrap();
    assert_eq!((&v["changed"], &v["rescan"]), (&json!(["fileTypes.png"]), &json!(true)));
    assert!(v["job"].is_u64(), "{v}");
    assert_eq!(scans(&s), before + 1, "one scan started");
    s.wait_idle();
    assert_eq!(by_name(&s).keys().collect::<Vec<_>>(), ["a.jpg"]);

    // A month on, the folder's sidecar is written again: the PNGs' entries stay, unmarked.
    clock.advance(Duration::from_secs(31 * 24 * 60 * 60));
    s.execute("photo.star.set", &json!({ "ids": [ids["a.jpg"]], "on": true })).unwrap();
    s.flush().unwrap();
    let doc = read_json(&folder.join(".dalica.json")).unwrap();
    assert_eq!(doc["photos"]["a.jpg"]["star"], json!(true), "the write happened: {doc}");
    assert_eq!(doc["photos"]["b.png"]["star"], json!(true), "{doc}");
    assert_eq!(doc["photos"]["c.png"]["caption"], json!("Fishing boats"), "{doc}");
    assert!(doc["dalica"].get("missing").is_none(), "not marked missing: {doc}");

    // On again: everything said about them comes back.
    s.execute("options.set", &json!({ "values": { "fileTypes.png": true } })).unwrap();
    s.wait_idle();
    let ids = by_name(&s);
    assert_eq!(ids.keys().collect::<Vec<_>>(), ["a.jpg", "b.png", "c.png"]);
    let lib = s.library();
    assert!(lib.photo(ids["b.png"]).unwrap().star);
    assert_eq!(lib.caption(ids["c.png"]), "Fishing boats");
    assert_eq!(lib.info(ids["b.png"]).edits, ["sepia()"]);
}

#[test]
fn the_small_picture_threshold_hides_and_shows_a_40_px_photo() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&[jpeg(1), jpeg_of(2, 40, 40)], None);
    let section = format!("folder:{}", s.library().photo(ids[0]).unwrap().folder);
    let listed = |s: &mut Session, px: u32| -> Vec<String> {
        let v = s.execute("options.set", &json!({ "values": { "fileTypes.smallPictureThreshold": px } })).unwrap();
        s.wait_idle();
        assert_eq!(v["rescan"], json!(v["changed"] != json!([])));
        let q = s.execute("library.query", &json!({ "section": section })).unwrap();
        q["photos"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(listed(&mut s, 64), ["IMG_0001.jpg"], "smaller than 64 on both sides: hidden");
    assert_eq!(listed(&mut s, 32), ["IMG_0001.jpg", "IMG_0002.jpg"], "not smaller than 32: shown");
    assert_eq!(listed(&mut s, 41), ["IMG_0001.jpg"]);
    assert_eq!(s.library().photos().count(), 2, "a small picture stays in the library");
}

fn state(s: &Session, job: JobId) -> JobState {
    s.jobs().iter().find(|j| j.job == job).map(|j| j.state).unwrap()
}

/// Polls until job `job` has ended, the rescan named on every poll until then; then it is not.
fn named_until_it_ends(s: &mut Session, job: JobId) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !state(s, job).is_finished() {
        assert_eq!(s.rescanning_for_file_types(), Some(job), "named while it runs");
        assert!(std::time::Instant::now() < deadline, "the rescan ended");
        s.poll();
    }
    assert_eq!(state(s, job), JobState::Done);
    assert_eq!(s.rescanning_for_file_types(), None, "not once it ended");
}

#[test]
fn the_session_names_the_rescan_a_file_types_change_started_and_no_other_scan() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Harbour");
    dalica_fs::create_dir_all(&folder).unwrap();
    dalica_fs::replace(&folder.join("a.jpg"), &jpeg(1), Expect::Absent).unwrap();
    let png = dalica_scenes::png(&dalica_scenes::Scene::new(2, 96, 64)).unwrap();
    dalica_fs::replace(&folder.join("b.png"), &png, Expect::Absent).unwrap();
    let clock = virtual_clock();
    let mut s = open_with(lib.path(), clock.clone());
    s.set_watching(WatchMode::Polling);

    // A plain scan (Add Folder) is not the rescan.
    let v = s.execute("folders.add", &json!({ "path": folder })).unwrap();
    assert!(s.busy(), "{v}");
    assert_eq!(s.rescanning_for_file_types(), None);
    s.wait_idle();

    // PNG off: named from options.set until its job ends.
    let v = s.execute("options.set", &json!({ "values": { "fileTypes.png": false } })).unwrap();
    let first = v["job"].as_u64().unwrap();
    named_until_it_ends(&mut s, first);

    // A second change while it runs restarts it: the new scan is named, the old one ended.
    let v = s.execute("options.set", &json!({ "values": { "fileTypes.png": true } })).unwrap();
    let old = v["job"].as_u64().unwrap();
    let v = s.execute("options.set", &json!({ "values": { "fileTypes.smallPictureThreshold": 32 } })).unwrap();
    let restarted = v["job"].as_u64().unwrap();
    assert_ne!(restarted, old);
    assert_eq!(state(&s, old), JobState::Cancelled, "the first one was let go");
    named_until_it_ends(&mut s, restarted);
    s.wait_idle();

    // A rescan from the watcher is not it either.
    for _ in 0..2 {
        clock.advance(Duration::from_secs(1));
        assert!(s.wait_watching(Duration::from_secs(60)));
        s.wait_idle();
    }
    let added = folder.join("c.jpg");
    off_owner(move || dalica_fs::replace(&added, &jpeg(3), Expect::Absent).unwrap());
    let mut seen = false;
    for _ in 0..4 {
        clock.advance(Duration::from_secs(1));
        assert!(s.wait_watching(Duration::from_secs(60)));
        if s.busy() {
            seen = true;
            assert_eq!(s.rescanning_for_file_types(), None);
        }
        s.wait_idle();
    }
    assert!(seen, "the watcher's rescan ran");
    assert_eq!(by_name(&s).len(), 3, "the rescan brought c.jpg in");
}
