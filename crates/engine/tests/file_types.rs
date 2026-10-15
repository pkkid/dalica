//! Options → File Types through `options.set` (`roadmap.md` M10.5; `menus-and-shortcuts.md` §5,
//! `data-model.md` §7) on temporary folders: turning PNG off starts one scan and the PNGs leave the
//! library; 31 days later on a virtual clock, after a sidecar write in their folder, their entries
//! are still on disk and not marked missing (their files are there); turning PNG back on brings
//! them back with their stars, captions and recipes.

use std::collections::BTreeMap;
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::{JobKind, Session};
use dalica_fs::Expect;
use serde_json::json;

use crate::common::{jpeg, open_with, read_json, virtual_clock};

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
