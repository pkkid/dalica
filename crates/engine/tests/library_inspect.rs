//! `library.inspect` (`control-protocol.md` §6.4) on a demo library: every key of the spec, counts
//! that agree with `library.query`, the photo list paged and restricted to a section, hostile
//! params refused, and no people data.

use std::sync::Arc;

use dalica_engine::{CmdError, Session};
use serde_json::{Value, json};

/// A fresh demo library (three folders, sixty photos), opened from `<temp>/dalica-demo-…`.
fn demo() -> (dalica_fs::TempDir, Session) {
    let dir = dalica_engine::demo::fresh_dir().unwrap();
    let s = dalica_engine::demo::open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
    (dir, s)
}

fn inspect(s: &mut Session, p: Value) -> Value {
    s.execute("library.inspect", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn ids(v: &Value) -> Vec<u64> {
    v["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect()
}

#[test]
fn every_key_is_there_and_the_counts_match_the_library() {
    let (_dir, mut s) = demo();
    s.execute("photo.star.set", &json!({ "ids": [1, 2], "on": true })).unwrap();
    s.execute("photo.hide", &json!({ "ids": [5] })).unwrap();
    let v = inspect(&mut s, json!({}));
    for key in
        ["revision", "demo", "watched", "counts", "tree", "sections", "search", "view", "undo", "unsavedEdits", "pendingWrites", "problems", "scan"]
    {
        assert!(v.get(key).is_some(), "{key} in {v}");
    }
    assert!(v.get("photos").is_none(), "photos only when asked");
    assert_eq!((v["revision"].as_u64(), &v["demo"]), (Some(s.library().revision()), &json!(true)));
    assert_eq!(v["watched"].as_array().unwrap().len(), 3);
    assert!(v["watched"].as_array().unwrap().iter().all(|w| w["mode"] == "always" && w["online"] == true), "{}", v["watched"]);

    let all = s.execute("library.query", &json!({ "limit": 10_000 })).unwrap();
    let c = &v["counts"];
    assert_eq!(c["photos"].as_u64(), all["total"].as_u64().map(|t| t + 1), "the hidden photo leaves library.query: {c}");
    assert_eq!((c["folders"].as_u64(), c["albums"].as_u64(), c["hidden"].as_u64(), c["videos"].as_u64()), (Some(3), Some(0), Some(1), Some(0)));

    let tree = &v["tree"];
    assert_eq!((&tree["mode"], &tree["sort"], &tree["reverse"]), (&json!("flat"), &json!("date"), &json!(false)));
    let groups: Vec<&str> = tree["groups"].as_array().unwrap().iter().map(|g| g["group"].as_str().unwrap()).collect();
    assert_eq!(groups, ["albums", "folders"]);
    assert_eq!(
        tree["groups"][0]["rows"][0],
        json!({ "widget": "tree:album:starred", "section": "album:starred", "name": "Starred Photos", "count": 2 })
    );
    let rows = tree["groups"][1]["rows"].as_array().unwrap();
    let folders: Vec<&Value> = rows.iter().filter(|r| r.get("widget").is_some()).collect();
    assert_eq!(folders.len(), 3);
    assert_eq!(folders[0]["widget"], "tree:folder:1", "folders in tree order, numbered from 1");

    let sections = v["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 4);
    assert_eq!((&sections[0]["section"], &sections[0]["shown"]), (&json!("album:starred"), &json!(2)));
    let folder_shown: u64 = sections[1..].iter().map(|s| s["shown"].as_u64().unwrap()).sum();
    let folder_count: u64 = sections[1..].iter().map(|s| s["count"].as_u64().unwrap()).sum();
    assert_eq!((folder_count, folder_shown), (c["photos"].as_u64().unwrap(), all["total"].as_u64().unwrap()), "the hidden photo is not shown");

    assert_eq!(v["search"]["active"], false);
    assert_eq!(v["view"], json!({ "hiddenPictures": false, "smallPictures": false, "caption": "none", "thumbSize": null }));
    assert_eq!(v["undo"]["depth"], 2);
    assert_eq!(v["unsavedEdits"], json!({ "photos": 0, "sections": [] }));
    assert_eq!((&v["pendingWrites"], &v["problems"]), (&Value::Null, &json!([])));
    assert_eq!(v["scan"]["running"], false);

    // A turned photo is an unsaved edit; inspecting changes nothing.
    s.execute("photo.rotate.cw", &json!({ "ids": [3] })).unwrap();
    let (rev, undo) = (s.library().revision(), s.undo_stack().len());
    let v = inspect(&mut s, json!({ "section": "folder:1" }));
    assert_eq!(v["unsavedEdits"], json!({ "photos": 1, "sections": ["folder:1"] }));
    let three = v["photos"].as_array().unwrap().iter().find(|p| p["id"] == 3).unwrap();
    assert_eq!((&three["rotate"], &three["edited"], three["recipe"].as_array().map(Vec::len)), (&json!(90), &json!(true), Some(1)), "{three}");
    assert_eq!(v["sections"].as_array().unwrap().iter().find(|x| x["section"] == "folder:1").unwrap()["edited"], 1);
    assert_eq!((s.library().revision(), s.undo_stack().len()), (rev, undo));
}

#[test]
fn paging_and_a_section_restrict_the_photos() {
    let (_dir, mut s) = demo();
    s.execute("photo.star.set", &json!({ "ids": [7], "on": true })).unwrap();
    let all = inspect(&mut s, json!({ "photos": true }));
    let total: u64 = all["sections"].as_array().unwrap().iter().map(|s| s["shown"].as_u64().unwrap()).sum();
    assert_eq!(all["page"], json!({ "offset": 0, "limit": 200, "total": total }));
    let every = ids(&all);
    assert_eq!(every.len() as u64, total);
    assert_eq!((every[0], &all["photos"][0]["section"]), (7, &json!("album:starred")), "light-table order: the albums first");
    let first = &all["photos"][1];
    for key in [
        "id", "section", "path", "name", "star", "caption", "tags", "hidden", "rotate", "edited", "recipe", "geo", "date", "width", "height",
        "bytes", "video",
    ] {
        assert!(first.get(key).is_some(), "{key} in {first}");
    }
    assert_eq!((&first["id"], &first["section"], &first["rotate"], &first["edited"]), (&json!(1), &json!("folder:1"), &json!(0), &json!(false)));

    let page = inspect(&mut s, json!({ "photos": true, "offset": 10, "limit": 5 }));
    assert_eq!(ids(&page), every[10..15]);
    assert_eq!(page["page"]["total"], total);
    let past = inspect(&mut s, json!({ "photos": true, "offset": 1_000_000 }));
    assert_eq!((ids(&past).len(), &past["page"]["total"]), (0, &json!(total)));

    let folder = inspect(&mut s, json!({ "section": "folder:2" }));
    let shown = folder["sections"].as_array().unwrap().iter().find(|x| x["section"] == "folder:2").unwrap()["shown"].clone();
    assert_eq!(folder["page"]["total"], shown);
    assert!(folder["photos"].as_array().unwrap().iter().all(|p| p["section"] == "folder:2"));
    assert_eq!(ids(&inspect(&mut s, json!({ "photos": "folder:2" }))), ids(&folder));
    assert_eq!(ids(&inspect(&mut s, json!({ "photos": true, "section": "album:starred" }))), [7]);

    for (p, why) in [(json!({ "section": "folder:99" }), "unknown folder"), (json!({ "photos": "album:search" }), "not searching")] {
        assert!(matches!(s.execute("library.inspect", &p), Err(CmdError::NotFound(_))), "{why}: {p}");
    }
}

#[test]
fn hostile_params_are_bad_params_and_no_people_data_shows() {
    let (_dir, mut s) = demo();
    let rev = s.library().revision();
    for p in [
        json!({ "photos": true, "limit": 1001 }),
        json!({ "photos": true, "limit": -1 }),
        json!({ "photos": true, "limit": 1.5 }),
        json!({ "photos": true, "limit": "10" }),
        json!({ "photos": true, "offset": 1_000_000_001u64 }),
        json!({ "photos": true, "offset": u64::MAX }),
        json!({ "photos": true, "offset": -5 }),
        json!({ "offset": 3 }),
        json!({ "limit": 3 }),
        json!({ "photos": 3 }),
        json!({ "photos": "folder:x" }),
        json!({ "photos": false, "section": "folder:1" }),
        json!({ "photos": "folder:1", "section": "folder:2" }),
        json!({ "section": "people:1" }),
        json!({ "people": true }),
        json!(["photos"]),
    ] {
        assert!(matches!(s.execute("library.inspect", &p), Err(CmdError::BadParams(_))), "{p}");
    }
    assert_eq!(s.library().revision(), rev);

    let v = inspect(&mut s, json!({ "photos": true, "limit": 1000 }));
    assert_eq!(v["counts"]["people"], 0);
    assert!(v["tree"]["groups"].as_array().unwrap().iter().all(|g| g["group"] != "people"), "no People group");
    for p in v["photos"].as_array().unwrap() {
        for key in ["people", "faces", "names", "person"] {
            assert!(p.get(key).is_none(), "{key} in {p}");
        }
    }
}
