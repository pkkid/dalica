//! Tag commands and quick tags (`roadmap.md` M6.15, `organizing.md` §5.2): tagging 1 000 photos
//! is one undo step; a rename merges onto an existing tag and undoes exactly; partial counts over a
//! mixed selection; toggling; quick tags by slot and by their ten keys, which conflict with no
//! other default binding on any platform; tags reach the folder's sidecar.
//!
//! Timings: `cargo test --release -p dalica-engine --test all tags:: -- --nocapture` prints them.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use dalica_core::{Clock, PhotoId, SystemClock};
use dalica_engine::keymap::{Keymap, Platform};
use dalica_engine::synthetic::{self, Spec};
use dalica_engine::{CmdError, Context, Key, Session};
use serde_json::{Value, json};

fn clock() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

/// An index-only library of `n` photos in `folders` folders, ids in light-table order.
fn session(dir: &Path, n: usize, folders: usize) -> (Session, Vec<PhotoId>) {
    let mut s = synthetic::open(dir, &Spec::new(n, folders, 7), clock()).unwrap();
    s.wait_idle();
    let mut ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    ids.sort();
    assert_eq!(ids.len(), n);
    (s, ids)
}

fn tags(s: &Session, id: PhotoId) -> Vec<String> {
    s.library().tags_of(id).to_vec()
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e:?}"))
}

#[test]
fn tagging_a_thousand_photos_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, ids) = session(dir.path(), 1_000, 10);
    let before = s.library().model().clone();
    let t = Instant::now();
    let r = run(&mut s, "photo.tags.add", json!({ "ids": ids, "tags": "lisbon, tram" }));
    let add = t.elapsed();
    assert_eq!(r["changed"], json!(1_000));
    assert_eq!(s.undo_stack().len(), 1);
    assert_eq!(s.undo_stack().undo_label(), Some("Add Tags"));
    assert_eq!(s.library().tag_index().count("lisbon"), 1_000);
    assert!(ids.iter().all(|id| tags(&s, *id) == ["lisbon", "tram"]));
    let after = s.library().model().clone();
    let t = Instant::now();
    run(&mut s, "edit.undo", json!({}));
    let undo = t.elapsed();
    assert_eq!(s.library().model(), &before);
    assert!(s.library().tag_index().is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.library().model(), &after);
    // Removing from all of them is one step too.
    let r = run(&mut s, "photo.tags.remove", json!({ "ids": ids, "tags": ["TRAM"] }));
    assert_eq!(r["changed"], json!(1_000));
    assert_eq!(s.undo_stack().len(), 2);
    assert_eq!(s.library().tag_index().count("tram"), 0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.library().model(), &after);
    println!("tags: add 2 tags to 1 000 photos {add:?}, undo {undo:?}");
}

#[test]
fn a_rename_merges_onto_an_existing_tag_and_undoes_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, ids) = session(dir.path(), 4, 1);
    run(&mut s, "photo.tags.add", json!({ "ids": [ids[0]], "tags": "beach, sun" }));
    run(&mut s, "photo.tags.add", json!({ "ids": [ids[1]], "tags": "Shore, beach" }));
    // Added after "Shore" is in use, so it takes that spelling.
    run(&mut s, "photo.tags.add", json!({ "ids": [ids[2]], "tags": "shore" }));
    assert_eq!(tags(&s, ids[2]), ["Shore"]);
    let (before, index_before, steps) = (s.library().model().clone(), s.library().tag_index().clone(), s.undo_stack().len());
    // Without confirm it only asks, naming how many photos it touches.
    let asked = s.execute("tags.rename", &json!({ "from": "BEACH", "to": "shore" }));
    assert!(matches!(&asked, Err(CmdError::Disabled(m)) if m.contains("2 photos") && m.contains("confirm")), "{asked:?}");
    assert_eq!(s.library().model(), &before);
    let r = run(&mut s, "tags.rename", json!({ "from": "BEACH", "to": "shore", "confirm": true }));
    assert_eq!(r, json!({ "to": "Shore", "photos": 2, "merged": 1 }));
    assert_eq!(tags(&s, ids[0]), ["Shore", "sun"], "takes the spelling in use");
    assert_eq!(tags(&s, ids[1]), ["Shore"], "holds the merged tag once");
    assert_eq!(tags(&s, ids[2]), ["Shore"], "photos without the old tag are untouched");
    assert_eq!(s.library().tag_index().count("beach"), 0);
    assert_eq!(s.library().tag_index().count("shore"), 3);
    assert_eq!(s.undo_stack().len(), steps + 1);
    let after = s.library().model().clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.library().model(), &before);
    assert_eq!(s.library().tag_index(), &index_before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.library().model(), &after);
    // A case-only rename respells everywhere; renaming to the spelling in use changes nothing.
    let r = run(&mut s, "tags.rename", json!({ "from": "shore", "to": "SHORE", "confirm": true }));
    assert_eq!(r["photos"], json!(3));
    assert!([ids[0], ids[1], ids[2]].iter().all(|id| tags(&s, *id).contains(&"SHORE".to_string())));
    let steps = s.undo_stack().len();
    let r = run(&mut s, "tags.rename", json!({ "from": "shore", "to": "SHORE" }));
    assert_eq!(r["photos"], json!(0));
    assert_eq!(s.undo_stack().len(), steps, "nothing changed, no step");
}

#[test]
fn partial_counts_over_a_mixed_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, ids) = session(dir.path(), 10, 2);
    let eight = &ids[..8];
    run(&mut s, "photo.tags.add", json!({ "ids": eight, "tags": "tram" }));
    run(&mut s, "photo.tags.add", json!({ "ids": [ids[0], ids[3], ids[5], ids[9]], "tags": "Beach" }));
    run(&mut s, "photo.tags.add", json!({ "ids": [ids[9]], "tags": "bench" }));
    let rev = s.library().revision();
    let r = run(&mut s, "tags.list", json!({ "ids": eight }));
    assert_eq!(
        r,
        json!({ "photos": 8, "tags": [
            { "tag": "Beach", "count": 3, "of": 8, "partial": true, "label": "Beach (3 of 8)" },
            { "tag": "tram", "count": 8, "of": 8, "partial": false, "label": "tram" },
        ]})
    );
    // A prefix filters them, ignoring case; repeated ids count once.
    let r = run(&mut s, "tags.list", json!({ "ids": [ids[0], ids[0], ids[9]], "prefix": "BE" }));
    assert_eq!(r["photos"], json!(2));
    let labels: Vec<&str> = r["tags"].as_array().unwrap().iter().map(|t| t["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Beach", "bench (1 of 2)"]);
    // Without ids: every tag with its count; with a prefix, completion.
    let r = run(&mut s, "tags.list", json!({}));
    assert_eq!(r, json!({ "tags": [{ "tag": "Beach", "count": 4 }, { "tag": "bench", "count": 1 }, { "tag": "tram", "count": 8 }] }));
    let r = run(&mut s, "tags.list", json!({ "prefix": "b" }));
    assert_eq!(r["tags"].as_array().map(Vec::len), Some(2));
    assert_eq!(run(&mut s, "tags.list", json!({ "prefix": "z" })), json!({ "tags": [] }));
    assert_eq!(run(&mut s, "tags.list", json!({ "ids": [] })), json!({ "photos": 0, "tags": [] }));
    // A query changes nothing.
    assert_eq!(s.library().revision(), rev);
}

#[test]
fn toggle_adds_to_all_unless_all_hold_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, ids) = session(dir.path(), 3, 1);
    run(&mut s, "photo.tags.add", json!({ "ids": [ids[0]], "tags": "Tram" }));
    let before = s.library().model().clone();
    let r = run(&mut s, "photo.tags.toggle", json!({ "ids": ids, "tag": "tram" }));
    assert_eq!((r["on"].clone(), r["tag"].clone(), r["changed"].clone()), (json!(true), json!("Tram"), json!(2)));
    assert!(ids.iter().all(|id| tags(&s, *id) == ["Tram"]));
    let r = run(&mut s, "photo.tags.toggle", json!({ "ids": ids, "tag": "TRAM" }));
    assert_eq!((r["on"].clone(), r["changed"].clone()), (json!(false), json!(3)));
    assert!(s.library().tag_index().is_empty());
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.library().model(), &before);
}

#[test]
fn quick_tags_apply_by_slot_and_by_key_and_toggle_off() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, ids) = session(dir.path(), 3, 1);
    let r = run(&mut s, "tags.quick.set", json!({ "slots": ["", "", "lisbon", "tram", "family"], "useRecent": true }));
    assert_eq!(r["slots"], json!([null, null, "lisbon", "tram", "family", null, null, null, null, null]));
    // Slots 1 and 2 follow recently added tags; nothing has been added yet.
    assert_eq!(s.execute("tags.quick.1", &json!({ "ids": ids })), Err(CmdError::Disabled("Quick tag 1 is not assigned.".into())));
    let before: Vec<_> = ids.iter().map(|id| s.library().info(*id).clone()).collect();
    // Slot 4 through its keyed command, then through tags.quick.apply: on, then off again.
    let r = run(&mut s, "tags.quick.4", json!({ "ids": [ids[0], ids[1]] }));
    assert_eq!((r["n"].clone(), r["tag"].clone(), r["on"].clone()), (json!(4), json!("tram"), json!(true)));
    assert_eq!(s.library().tag_index().count("tram"), 2);
    let r = run(&mut s, "tags.quick.apply", json!({ "n": 4, "ids": [ids[0], ids[1]] }));
    assert_eq!(r["on"], json!(false));
    let after: Vec<_> = ids.iter().map(|id| s.library().info(*id).clone()).collect();
    assert_eq!(after, before, "on, then off: the tags are as they were");
    assert!(s.library().last_changed(ids[0]).is_some(), "two changes all the same (Recently Updated)");
    assert_eq!(s.undo_stack().len(), 2);
    // Adding a tag makes it recent: slot 1 shows it, Ctrl+Alt+1 applies it.
    run(&mut s, "photo.tags.add", json!({ "ids": [ids[2]], "tags": "Harbour" }));
    assert_eq!(run(&mut s, "tags.quick.get", json!({}))["shown"][0], json!("Harbour"));
    let key = Key::parse("Ctrl+Alt+1").unwrap();
    let id = s.keymap().resolve(&key, Context::Library, false).unwrap();
    run(&mut s, &id, json!({ "ids": ids }));
    assert_eq!(s.library().tag_index().count("harbour"), 3);
    // The quick tags outlive the session (settings.json). The synthetic library has no photo
    // folders, so its sidecar writes wait (`unsaved`); settings.json is written all the same.
    assert!(matches!(s.flush(), Err(CmdError::Unsaved(_))));
    drop(s);
    let mut s = Session::open(&synthetic::library_dir(dir.path()), clock()).unwrap();
    let r = run(&mut s, "tags.quick.get", json!({}));
    assert_eq!(r["shown"], json!(["Harbour", null, "lisbon", "tram", "family", null, null, null, null, null]));
}

#[test]
fn the_ten_quick_tag_keys_conflict_with_nothing() {
    let specs = dalica_engine::cmd::specs();
    for p in Platform::ALL {
        let map = Keymap::from_specs_on(&specs, p).unwrap();
        assert_eq!(map.conflicts(), [], "{p:?}");
        for n in 1..=10 {
            let key = Key::parse(&format!("Ctrl+Alt+{}", n % 10)).unwrap();
            assert_eq!(map.resolve(&key, Context::Library, false), Some(format!("tags.quick.{n}")), "{p:?} {key}");
            assert_eq!(map.resolve(&key, Context::EditView, false), None, "library only: {p:?} {key}");
            let bound: Vec<_> = map.bindings().into_iter().filter(|b| b.key == key).collect();
            assert_eq!(bound.len(), 1, "{p:?} {key}: {bound:?}");
        }
    }
}

#[test]
fn tags_reach_the_folder_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = synthetic::open_scanned(dir.path(), &Spec::new(2, 1, 3), 64, clock()).unwrap();
    s.wait_idle();
    let photo = s.library().photos().next().map(|p| (p.id, p.folder, p.name.clone())).unwrap();
    let (id, folder, name) = photo;
    run(&mut s, "photo.tags.add", json!({ "ids": [id], "tags": "lisbon, Tram" }));
    s.flush().unwrap();
    let path = s.library().folder(folder).unwrap().path.join(dalica_sidecar::FILE_NAME);
    let sidecar: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(sidecar["photos"][&name]["tags"], json!(["lisbon", "Tram"]));
    run(&mut s, "edit.undo", json!({}));
    s.flush().unwrap();
    let sidecar: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(sidecar["photos"][&name].get("tags").is_none(), "{sidecar}");
}
