//! `photo.caption.set` on a temporary library (`roadmap.md` M6.14; `organizing.md` §5.3,
//! `data-model.md` §3): the caption trimmed, capped and cleared by "", one undo step for any
//! number of photos and none when nothing changes, undo and redo exact, the sidecar after the
//! command and after undo, and captions surviving a restart and a rebuild of the index. Also the
//! data loss reported on #237 (a rebuild, then a write, took the M6 fields out of the sidecar) and
//! a merge scan of a folder whose sidecar holds every M6 field.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::common::open;
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use dalica_library::{MAX_CAPTION, PhotoInfo};
use serde_json::{Value, json};

/// A library watching one folder `Lisbon` of `n` photos `IMG_0001.jpg`…; ids in name order.
fn setup(n: usize) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = by_name(&s).into_values().collect();
    (lib, disk, folder, s, ids)
}

fn by_name(s: &Session) -> BTreeMap<String, PhotoId> {
    s.library().photos().map(|p| (p.name.clone(), p.id)).collect()
}

/// What the user said about each photo, by name (ids change in a rebuild).
fn infos(s: &Session) -> BTreeMap<String, PhotoInfo> {
    s.library().photos().map(|p| (p.name.clone(), s.library().info(p.id).clone())).filter(|(_, i)| !i.is_empty()).collect()
}

/// The folder's sidecar after every pending write landed (`Null` when there is none).
fn sidecar(s: &mut Session, folder: &Path) -> Value {
    s.flush().unwrap();
    std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
}

fn captions(v: &Value) -> BTreeMap<String, String> {
    v["photos"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, e)| Some((k.clone(), e.get("caption")?.as_str()?.to_string()))).collect())
        .unwrap_or_default()
}

#[test]
fn a_caption_is_trimmed_and_set_on_every_photo_in_one_step_and_undo_redo_are_exact() {
    let (_l, _d, folder, mut s, ids) = setup(3);
    let m0 = s.library().model().clone();
    let v = s.execute("photo.caption.set", &json!({ "ids": [ids[0], ids[2], ids[0]], "caption": "  Tram 28 at sunset \n" })).unwrap();
    assert_eq!(v, json!({ "ids": [ids[0], ids[2]], "caption": "Tram 28 at sunset", "changed": 2 }));
    assert_eq!(s.library().caption(ids[0]), "Tram 28 at sunset");
    assert_eq!(s.library().caption(ids[1]), "");
    assert_eq!(s.undo_stack().len(), 1);
    assert_eq!(s.undo_stack().undo_label(), Some("Set Caption"));
    let after = sidecar(&mut s, &folder);
    assert_eq!(
        captions(&after),
        BTreeMap::from([("IMG_0001.jpg".into(), "Tram 28 at sunset".into()), ("IMG_0003.jpg".into(), "Tram 28 at sunset".into())])
    );
    let m1 = s.library().model().clone();

    // The same caption again changes nothing and records no step.
    let v = s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28 at sunset" })).unwrap();
    assert_eq!(v["changed"], json!(0));
    assert_eq!(s.undo_stack().len(), 1);

    // A different caption on a mixed selection, then "" clears.
    s.execute("photo.caption.set", &json!({ "ids": [ids[0], ids[1]], "caption": "Alfama" })).unwrap();
    let m2 = s.library().model().clone();
    s.execute("photo.caption.set", &json!({ "ids": [ids[0], ids[1], ids[2]], "caption": "   " })).unwrap();
    assert!(infos(&s).is_empty(), "an empty caption clears it: {:?}", infos(&s));
    assert!(captions(&sidecar(&mut s, &folder)).is_empty());

    for want in [&m2, &m1, &m0] {
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.library().model(), want);
    }
    let undone = sidecar(&mut s, &folder);
    assert!(undone["photos"].as_object().is_none_or(|o| o.is_empty()), "after undo no caption and no empty entry: {undone}");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    assert_eq!(sidecar(&mut s, &folder), after, "redo writes the same sidecar again");
}

#[test]
fn a_caption_past_the_cap_is_cut_at_a_character_boundary() {
    let (_l, _d, _f, mut s, ids) = setup(1);
    // "é" is two bytes: the cap falls in the middle of one.
    let long = format!("a{}", "é".repeat(MAX_CAPTION));
    let v = s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": long })).unwrap();
    let got = s.library().caption(ids[0]);
    assert!(got.len() <= MAX_CAPTION && got.len() >= MAX_CAPTION - 1, "{}", got.len());
    assert!(long.starts_with(got));
    assert_eq!(v["caption"], json!(got));
    // Newlines inside a caption are kept (a caption may run over several lines).
    s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "line one\nline two" })).unwrap();
    assert_eq!(s.library().caption(ids[0]), "line one\nline two");
}

/// Disabled with no photo, and too many photos (`cmd::caption`'s tests: unknown photos and bad
/// parameters).
#[test]
fn disabled_and_too_many_change_nothing() {
    let (_l, _d, _f, mut s, ids) = setup(1);
    let m = s.library().model().clone();
    let spec = *s.spec("photo.caption.set").unwrap();
    assert!((spec.enabled)(&s, &json!({ "ids": [], "caption": "x" })).is_err());
    assert!((spec.enabled)(&s, &json!({ "ids": [ids[0]], "caption": "x" })).is_ok());
    let too_many: Vec<u64> = vec![ids[0].get(); 100_001];
    assert!(matches!(s.execute("photo.caption.set", &json!({ "ids": too_many, "caption": "x" })), Err(CmdError::Limit(_))));
    assert_eq!(s.library().model(), &m);
    assert_eq!(s.undo_stack().len(), 0);
}

#[test]
fn captions_survive_a_restart_and_a_rebuild_of_the_index() {
    let (lib, _d, folder, mut s, ids) = setup(3);
    s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids[2]], "caption": "Miradouro 🌅" })).unwrap();
    let want = infos(&s);
    assert_eq!(want.len(), 2);
    let written = captions(&sidecar(&mut s, &folder));
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(infos(&s), want, "after a restart (journal and snapshot)");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(by_name(&s).len(), 3);
    // Another change in the folder after the rebuild rewrites its sidecar: the captions stay.
    let other = by_name(&s)["IMG_0002.jpg"];
    s.execute("photo.star.set", &json!({ "ids": [other], "on": true })).unwrap();
    assert_eq!(captions(&sidecar(&mut s, &folder)), written, "a write after a rebuild keeps the captions in the sidecar");
    assert_eq!(infos(&s).into_iter().map(|(n, i)| (n, i.caption)).collect::<BTreeMap<_, _>>(), written, "after a rebuild of the index");
}

/// The data loss reported on #237, exactly: captions, a rebuild of the index, then a star on
/// another photo of the folder (which rewrites its sidecar). Before the fix the rebuilt model had
/// no captions and the write took them out of `.dalica.json`.
#[test]
fn captions_then_a_rebuild_then_a_star_on_another_photo_keep_the_captions_in_the_sidecar() {
    let (_l, _d, folder, mut s, ids) = setup(3);
    s.execute("photo.caption.set", &json!({ "ids": [ids[0], ids[2]], "caption": "Tram 28" })).unwrap();
    let before = captions(&sidecar(&mut s, &folder));
    assert_eq!(before.len(), 2);
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let other = by_name(&s)["IMG_0002.jpg"];
    s.execute("photo.star.set", &json!({ "ids": [other], "on": true })).unwrap();
    let after = sidecar(&mut s, &folder);
    assert_eq!(after["photos"]["IMG_0002.jpg"]["star"], json!(true));
    assert_eq!(captions(&after), before, "{after}");
}

/// A merge scan (`folders.add`) of a folder whose sidecar already holds every M6 field, values
/// Dalica cannot use and keys it does not know: the model takes every usable field, and the
/// first write afterwards (a star) changes that key only, keeping every other byte's meaning.
#[test]
fn a_folder_added_with_a_sidecar_brings_every_field_and_a_write_keeps_the_rest() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Porto");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..3 {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let file = json!({
        "dalica": { "version": 1 },
        "folder": { "description": "Ribeira", "date": "2025-05-01", "place": "Porto", "hidden": true, "otherApp": [1, 2] },
        "photos": {
            "IMG_0001.jpg": { "caption": "Ponte", "tags": ["bridge", "river"], "hidden": true,
                              "edits": ["rotate(90)", "future_op(3)"], "redo": ["sepia()"], "date": "2025-05-01T10:00:00", "x-other": true },
            "IMG_0002.jpg": { "caption": ["not", "text"], "tags": "bridge", "date": "May" },
        },
    });
    dalica_fs::replace(&folder.join(".dalica.json"), serde_json::to_vec_pretty(&file).unwrap().as_slice(), Expect::Absent).unwrap();
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = by_name(&s);
    let one = s.library().info(ids["IMG_0001.jpg"]).clone();
    assert_eq!(
        one,
        PhotoInfo {
            caption: "Ponte".into(),
            tags: vec!["bridge".into(), "river".into()],
            hidden: true,
            edits: vec!["rotate(90)".into(), "future_op(3)".into()],
            redo: vec!["sepia()".into()],
            date: "2025-05-01T10:00:00".parse().ok(),
        }
    );
    assert!(s.library().info(ids["IMG_0002.jpg"]).is_empty(), "values of the wrong type are not used");
    let fid = s.library().folder_by_path(&folder).unwrap();
    let fi = s.library().folder_info(fid).clone();
    assert_eq!((fi.description.as_str(), fi.place.as_str(), fi.hidden), ("Ribeira", "Porto", true));
    assert_eq!(s.undo_stack().len(), 0, "what a scan brings is no undo step");

    s.execute("photo.star.set", &json!({ "ids": [ids["IMG_0003.jpg"]], "on": true })).unwrap();
    s.flush().unwrap();
    let mut after: Value = serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(after["photos"]["IMG_0003.jpg"], json!({ "star": true }));
    after["photos"].as_object_mut().unwrap().remove("IMG_0003.jpg");
    assert_eq!(after, file, "every other value, the unusable ones and the unknown keys included, is as it was");
}
