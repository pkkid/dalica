//! Rotations and flips on a temporary library (`roadmap.md` M6.14; `organizing.md` §9,
//! `data-model.md` §3.1): `photo.rotate.cw` / `photo.rotate.ccw` and `edit.flipH` / `edit.flipV`
//! append to the recipe through `edit`'s rules as one library undo step for any number of photos;
//! undo and redo are exact; the sidecar after the command and after undo; four rotations leave no
//! `edits` entry; the displayed size turns; the shortcuts; rotations surviving a restart and a
//! rebuild of the index. (The unknown operation that survives four rotations, and the full recipe,
//! are unit tests in `cmd/rotate.rs`, which can put any recipe in the model.)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::common::open;
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Context, Key, Session};
use dalica_fs::Expect;
use dalica_library::PhotoInfo;
use serde_json::{Value, json};

const COMMANDS: [&str; 4] = ["photo.rotate.cw", "photo.rotate.ccw", "edit.flipH", "edit.flipV"];

/// A library watching one folder `Lisbon` of `n` 96 × 64 photos `IMG_0001.jpg`…; ids in name order.
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

fn infos(s: &Session) -> BTreeMap<String, PhotoInfo> {
    s.library().photos().map(|p| (p.name.clone(), s.library().info(p.id).clone())).filter(|(_, i)| !i.is_empty()).collect()
}

fn edits(s: &Session, id: PhotoId) -> Vec<String> {
    s.library().info(id).edits.clone()
}

/// The folder's sidecar after every pending write landed (`Null` when there is none).
fn sidecar(s: &mut Session, folder: &Path) -> Value {
    s.flush().unwrap();
    std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
}

/// Each photo's `"edits"` in the sidecar, by name.
fn sidecar_edits(v: &Value) -> BTreeMap<String, Value> {
    v["photos"].as_object().map(|o| o.iter().filter_map(|(k, e)| Some((k.clone(), e.get("edits")?.clone()))).collect()).unwrap_or_default()
}

fn displayed(s: &mut Session, id: PhotoId) -> (u64, u64) {
    let v = s.execute("photo.inspect", &json!({ "ids": [id] })).unwrap();
    let d = &v[0]["displayed"];
    (d["width"].as_u64().unwrap(), d["height"].as_u64().unwrap())
}

#[test]
fn rotations_merge_and_four_leave_no_edits_entry_in_the_model_or_the_sidecar() {
    let (_l, _d, folder, mut s, ids) = setup(2);
    let id = ids[0];
    let mut steps = Vec::new();
    for (cmd, want, size) in [
        ("photo.rotate.cw", vec!["rotate(90)"], (64, 96)),
        ("photo.rotate.cw", vec!["rotate(180)"], (96, 64)),
        ("photo.rotate.ccw", vec!["rotate(90)"], (64, 96)),
        ("photo.rotate.ccw", vec![], (96, 64)),
    ] {
        s.execute(cmd, &json!({ "ids": [id] })).unwrap();
        assert_eq!(edits(&s, id), want, "{cmd}");
        assert_eq!(displayed(&mut s, id), size, "{cmd}");
        steps.push(s.library().model().clone());
    }
    assert!(s.library().info(id).is_empty(), "a net zero turn leaves nothing said about the photo");
    for _ in 0..4 {
        s.execute("photo.rotate.cw", &json!({ "ids": [id] })).unwrap();
    }
    assert!(s.library().info(id).is_empty(), "four quarter turns clockwise leave no entry");
    let v = sidecar(&mut s, &folder);
    assert!(sidecar_edits(&v).is_empty(), "no edits key: {v}");
    assert_eq!(s.undo_stack().len(), 8);
    assert_eq!(s.undo_stack().undo_label(), Some("Rotate Clockwise"));
}

#[test]
fn flips_cancel_in_pairs_and_mix_with_rotations() {
    let (_l, _d, _f, mut s, ids) = setup(1);
    let id = ids[0];
    s.execute("edit.flipH", &json!({ "ids": [id] })).unwrap();
    assert_eq!(displayed(&mut s, id), (96, 64), "a flip keeps the size");
    s.execute("edit.flipH", &json!({ "ids": [id] })).unwrap();
    assert!(edits(&s, id).is_empty(), "two flip_h() in a row cancel");
    for cmd in ["edit.flipH", "photo.rotate.cw", "edit.flipV", "edit.flipH", "photo.rotate.ccw"] {
        s.execute(cmd, &json!({ "ids": [id] })).unwrap();
    }
    assert_eq!(edits(&s, id), ["flip_h()", "rotate(90)", "flip_v()", "flip_h()", "rotate(270)"], "only the last operation merges");
    assert_eq!(displayed(&mut s, id), (96, 64));
}

#[test]
fn many_photos_in_one_step_and_undo_redo_restore_model_and_sidecar_exactly() {
    let (_l, _d, folder, mut s, ids) = setup(4);
    s.execute("photo.rotate.cw", &json!({ "ids": [ids[0]] })).unwrap();
    let (m0, file0) = (s.library().model().clone(), sidecar(&mut s, &folder));
    let v = s.execute("photo.rotate.ccw", &json!({ "ids": [ids[0], ids[1], ids[2], ids[1]] })).unwrap();
    assert_eq!(v, json!({ "ids": [ids[0], ids[1], ids[2]], "operation": "rotate(270)" }));
    assert_eq!(s.undo_stack().len(), 2, "three photos, one step");
    assert!(edits(&s, ids[0]).is_empty());
    assert_eq!(edits(&s, ids[1]), ["rotate(270)"]);
    assert_eq!(edits(&s, ids[2]), ["rotate(270)"]);
    assert!(edits(&s, ids[3]).is_empty());
    let (m1, file1) = (s.library().model().clone(), sidecar(&mut s, &folder));
    assert_eq!(
        sidecar_edits(&file1),
        BTreeMap::from([("IMG_0002.jpg".into(), json!(["rotate(270)"])), ("IMG_0003.jpg".into(), json!(["rotate(270)"]))])
    );
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m0);
    assert_eq!(sidecar(&mut s, &folder), file0, "after undo the sidecar is as it was");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    assert_eq!(sidecar(&mut s, &folder), file1);
}

#[test]
fn the_shortcuts_reach_the_commands_in_the_library_and_the_edit_view() {
    let (_l, _d, _f, s, _) = setup(1);
    for (key, cmd) in
        [("Ctrl+R", "photo.rotate.cw"), ("Ctrl+Shift+R", "photo.rotate.ccw"), ("Ctrl+Shift+H", "edit.flipH"), ("Ctrl+Shift+V", "edit.flipV")]
    {
        for context in [Context::Library, Context::EditView] {
            assert_eq!(s.keymap().resolve(&Key::parse(key).unwrap(), context, false).as_deref(), Some(cmd), "{key} in {context:?}");
        }
    }
    assert!(s.keymap().conflicts().is_empty(), "{:?}", s.keymap().conflicts());
}

#[test]
fn disabled_unknown_and_too_many_change_nothing() {
    let (_l, _d, _f, mut s, ids) = setup(1);
    let m = s.library().model().clone();
    for cmd in COMMANDS {
        let spec = *s.spec(cmd).unwrap();
        assert_eq!((spec.enabled)(&s, &json!({ "ids": [] })).map_err(|d| d.0), Err("Select some photos first.".into()), "{cmd}");
        assert!((spec.enabled)(&s, &json!({ "ids": [ids[0]] })).is_ok());
        assert_eq!(s.execute(cmd, &json!({ "ids": [] })), Err(CmdError::Disabled("Select some photos first.".into())));
        for bad in [json!({ "ids": [ids[0], 999] }), json!({ "ids": [0] })] {
            assert!(s.execute(cmd, &bad).is_err(), "{cmd} {bad}");
        }
        let too_many: Vec<u64> = vec![ids[0].get(); 100_001];
        assert!(matches!(s.execute(cmd, &json!({ "ids": too_many })), Err(CmdError::Limit(_))));
    }
    assert_eq!(s.library().model(), &m);
    assert_eq!(s.undo_stack().len(), 0);
}

#[test]
fn rotations_survive_a_restart_and_a_rebuild_of_the_index() {
    let (lib, _d, folder, mut s, ids) = setup(3);
    s.execute("photo.rotate.cw", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
    s.execute("edit.flipV", &json!({ "ids": [ids[1]] })).unwrap();
    let want = infos(&s);
    assert_eq!(want.len(), 2);
    let written = sidecar_edits(&sidecar(&mut s, &folder));
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(infos(&s), want, "after a restart (journal and snapshot)");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let other = by_name(&s)["IMG_0003.jpg"];
    s.execute("photo.star.set", &json!({ "ids": [other], "on": true })).unwrap();
    assert_eq!(sidecar_edits(&sidecar(&mut s, &folder)), written, "a write after a rebuild keeps the recipes in the sidecar");
    assert_eq!(infos(&s), want, "after a rebuild of the index");
    let first = by_name(&s)["IMG_0001.jpg"];
    assert_eq!(displayed(&mut s, first), (64, 96));
}
