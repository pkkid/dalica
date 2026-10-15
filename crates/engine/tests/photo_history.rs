//! The photo's own history (`roadmap.md` M7.3; `architecture.md` §4, `editing.md` §5.1) on a
//! temporary library: `photo.history.apply`, `.undo`, `.redo` and `.state` walk a photo's recipe
//! and redo list; a new step drops the redo; the "Undo …" / "Redo …" labels at each point; two
//! rotations undo one at a time; an operation Dalica does not know survives undo and redo; the
//! history and its labels survive a restart and a rebuild of the index; photo steps never touch
//! the library undo stack and library steps never walk a photo's history; the sidecar after each
//! command; the owner thread makes no `fs` call (bad parameters: `cmd::history`'s tests).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::common::{jpeg, lisbon, open};
use dalica_core::PhotoId;
use dalica_engine::{CmdError, History, Session};
use serde_json::{Value, json};

/// A library watching one folder `Lisbon` of `n` photos `IMG_0001.jpg`…, with `sidecar` written
/// beside them first (unless `Null`); ids in name order.
fn setup(n: usize, sidecar: &Value) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    lisbon(&(0..n as u64).map(jpeg).collect::<Vec<_>>(), Some(sidecar).filter(|v| !v.is_null()))
}

fn by_name(s: &Session) -> BTreeMap<String, PhotoId> {
    s.library().photos().map(|p| (p.name.clone(), p.id)).collect()
}

fn state(s: &mut Session, id: PhotoId) -> Value {
    s.execute("photo.history.state", &json!({ "id": id })).unwrap()
}

/// The labels the edit panel's buttons show now.
fn labels(s: &mut Session, id: PhotoId) -> (String, String) {
    let v = state(s, id);
    (v["undoLabel"].as_str().unwrap().to_string(), v["redoLabel"].as_str().unwrap().to_string())
}

fn apply(s: &mut Session, id: PhotoId, op: &str) -> Value {
    s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap()
}

fn undo(s: &mut Session, id: PhotoId) -> Value {
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap()
}

fn redo(s: &mut Session, id: PhotoId) -> Value {
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap()
}

/// The photo's recipe and redo list.
fn recipe(s: &Session, id: PhotoId) -> (Vec<String>, Vec<String>) {
    let info = s.library().info(id);
    (info.edits.clone(), info.redo.clone())
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|t| t.to_string()).collect()
}

/// The photo's entry in the folder's sidecar after every pending write landed.
fn sidecar_entry(s: &mut Session, folder: &Path, name: &str) -> Value {
    s.flush().unwrap();
    let v: Value = std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap());
    v["photos"][name].clone()
}

const CROP: &str = "crop(0.1,0.1,0.9,0.9)";
const TUNING: &str = "tuning(0.25,0,0,0,none)";

#[test]
fn apply_three_undo_two_redo_one_and_a_new_step_drops_the_redo() {
    let (_lib, _d, folder, mut s, ids) = setup(2, &Value::Null);
    let id = ids[0];
    assert_eq!(labels(&mut s, id), ("Undo".into(), "Redo".into()));
    assert_eq!(state(&mut s, id)["canUndo"], json!(false));

    let v = apply(&mut s, id, CROP);
    assert_eq!(v, json!({ "id": id, "edits": [CROP], "redo": [], "canUndo": true, "canRedo": false, "undoLabel": "Undo Crop", "redoLabel": "Redo" }));
    apply(&mut s, id, TUNING);
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo".into()));
    apply(&mut s, id, "sepia()");
    assert_eq!(labels(&mut s, id), ("Undo Sepia".into(), "Redo".into()));
    assert_eq!(recipe(&s, id), (list(&[CROP, TUNING, "sepia()"]), vec![]));

    assert_eq!(undo(&mut s, id), json!({ "id": id, "undone": "Sepia" }));
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo Sepia".into()));
    assert_eq!(undo(&mut s, id), json!({ "id": id, "undone": "Tuning" }));
    assert_eq!(labels(&mut s, id), ("Undo Crop".into(), "Redo Tuning".into()));
    assert_eq!(recipe(&s, id), (list(&[CROP]), list(&["sepia()", TUNING])));
    let entry = sidecar_entry(&mut s, &folder, "IMG_0001.jpg");
    assert_eq!((entry["edits"].clone(), entry["redo"].clone()), (json!([CROP]), json!(["sepia()", TUNING])), "the sidecar after undo");

    assert_eq!(redo(&mut s, id), json!({ "id": id, "redone": "Tuning" }));
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo Sepia".into()));
    assert_eq!(recipe(&s, id), (list(&[CROP, TUNING]), list(&["sepia()"])));

    // A new step drops what could still be redone.
    apply(&mut s, id, "autofix()");
    assert_eq!(recipe(&s, id), (list(&[CROP, TUNING, "autofix()"]), vec![]));
    assert_eq!(labels(&mut s, id), ("Undo Auto Fix".into(), "Redo".into()));
    assert_eq!(s.execute("photo.history.redo", &json!({ "id": id })), Err(CmdError::Disabled("Nothing to redo.".into())));
    let entry = sidecar_entry(&mut s, &folder, "IMG_0001.jpg");
    assert_eq!(entry["edits"], json!([CROP, TUNING, "autofix()"]));
    assert!(entry.get("redo").is_none_or(|r| r == &json!([])), "{entry}");

    // Undo all the way, then redo all the way, is exact.
    for _ in 0..3 {
        undo(&mut s, id);
    }
    assert_eq!(recipe(&s, id), (vec![], list(&["autofix()", TUNING, CROP])));
    assert_eq!(s.execute("photo.history.undo", &json!({ "id": id })), Err(CmdError::Disabled("Nothing to undo.".into())));
    for _ in 0..3 {
        redo(&mut s, id);
    }
    assert_eq!(recipe(&s, id), (list(&[CROP, TUNING, "autofix()"]), vec![]));
    assert!(s.library().info(ids[1]).is_empty(), "the other photo untouched");
}

/// `data-model.md` §3.1: a second rotation merges into the first, yet the history undoes them
/// one at a time and redoes them one at a time; a flip that cancels comes back on undo.
#[test]
fn two_rotations_undo_one_at_a_time() {
    let (_lib, _d, folder, mut s, ids) = setup(1, &Value::Null);
    let id = ids[0];
    apply(&mut s, id, "sepia()");
    apply(&mut s, id, "rotate(90)");
    apply(&mut s, id, "rotate(90)");
    assert_eq!(recipe(&s, id).0, list(&["sepia()", "rotate(180)"]));
    assert_eq!(labels(&mut s, id).0, "Undo Rotate");
    assert_eq!(sidecar_entry(&mut s, &folder, "IMG_0001.jpg")["edits"], json!(["sepia()", "rotate(180)"]));

    undo(&mut s, id);
    assert_eq!(recipe(&s, id), (list(&["sepia()", "rotate(90)"]), list(&["rotate(90)"])));
    assert_eq!(labels(&mut s, id), ("Undo Rotate".into(), "Redo Rotate".into()));
    undo(&mut s, id);
    assert_eq!(recipe(&s, id), (list(&["sepia()"]), list(&["rotate(90)", "rotate(90)"])));
    redo(&mut s, id);
    assert_eq!(recipe(&s, id), (list(&["sepia()", "rotate(90)"]), list(&["rotate(90)"])));
    redo(&mut s, id);
    assert_eq!(recipe(&s, id), (list(&["sepia()", "rotate(180)"]), vec![]));

    // Four quarter turns leave no entry; undo brings back the three before the last.
    apply(&mut s, id, "rotate(90)");
    apply(&mut s, id, "rotate(90)");
    assert_eq!(recipe(&s, id).0, list(&["sepia()"]));
    assert_eq!(labels(&mut s, id).0, "Undo Rotate", "the step undone is the rotation, not the sepia");
    undo(&mut s, id);
    assert_eq!(recipe(&s, id).0, list(&["sepia()", "rotate(270)"]));
    for want in [&["sepia()", "rotate(180)"][..], &["sepia()", "rotate(90)"], &["sepia()"], &[]] {
        undo(&mut s, id);
        assert_eq!(recipe(&s, id).0, list(want));
    }
    assert_eq!(recipe(&s, id).1.len(), 5);

    // The edit view's Ctrl+R goes through apply as well; the edit view's flips too.
    apply(&mut s, id, "flip_h()");
    apply(&mut s, id, "flip_h()");
    assert_eq!(recipe(&s, id), (vec![], vec![]));
    assert_eq!(labels(&mut s, id), ("Undo Flip".into(), "Redo".into()));
    undo(&mut s, id);
    assert_eq!(recipe(&s, id), (list(&["flip_h()"]), list(&["flip_h()"])));
    redo(&mut s, id);
    assert_eq!(recipe(&s, id), (vec![], vec![]));
}

/// `data-model.md` §10.4: an operation Dalica does not know keeps its text and its place through
/// undo and redo; a step after it never merges across it.
#[test]
fn an_unknown_operation_in_the_middle_survives_undo_and_redo() {
    let sidecar = json!({
        "dalica": { "version": 1 },
        "photos": { "IMG_0001.jpg": { "edits": ["rotate(90)", "future_op(1;2)", "legacy:redeye(1)"], "redo": ["glow(0.4,3.0)"], "x-other": 1 } },
    });
    let (_lib, _d, folder, mut s, ids) = setup(1, &sidecar);
    let id = ids[0];
    assert_eq!(labels(&mut s, id), ("Undo Edit".into(), "Redo Glow".into()));
    apply(&mut s, id, "rotate(90)");
    assert_eq!(recipe(&s, id), (list(&["rotate(90)", "future_op(1;2)", "legacy:redeye(1)", "rotate(90)"]), vec![]));
    apply(&mut s, id, "bw()");
    undo(&mut s, id);
    undo(&mut s, id);
    assert_eq!(recipe(&s, id), (list(&["rotate(90)", "future_op(1;2)", "legacy:redeye(1)"]), list(&["bw()", "rotate(90)"])));
    // Undo reaches the unknown entries themselves and moves them like any other.
    assert_eq!(undo(&mut s, id)["undone"], json!("Edit"));
    assert_eq!(undo(&mut s, id)["undone"], json!("Edit"));
    assert_eq!(recipe(&s, id), (list(&["rotate(90)"]), list(&["bw()", "rotate(90)", "legacy:redeye(1)", "future_op(1;2)"])));
    let entry = sidecar_entry(&mut s, &folder, "IMG_0001.jpg");
    assert_eq!(entry["redo"], json!(["bw()", "rotate(90)", "legacy:redeye(1)", "future_op(1;2)"]));
    assert_eq!(entry["x-other"], json!(1), "another field of the entry kept");
    for _ in 0..4 {
        redo(&mut s, id);
    }
    assert_eq!(recipe(&s, id), (list(&["rotate(90)", "future_op(1;2)", "legacy:redeye(1)", "rotate(90)", "bw()"]), vec![]));
    assert_eq!(
        sidecar_entry(&mut s, &folder, "IMG_0001.jpg")["edits"],
        json!(["rotate(90)", "future_op(1;2)", "legacy:redeye(1)", "rotate(90)", "bw()"])
    );
}

#[test]
fn the_history_and_its_labels_survive_a_restart_and_a_rebuild() {
    let (lib, _d, folder, mut s, ids) = setup(2, &Value::Null);
    let id = ids[1];
    for op in [CROP, "rotate(90)", TUNING, "sepia()"] {
        apply(&mut s, id, op);
    }
    undo(&mut s, id);
    undo(&mut s, id);
    let want = state(&mut s, id);
    assert_eq!((want["undoLabel"].clone(), want["redoLabel"].clone()), (json!("Undo Rotate"), json!("Redo Tuning")));
    s.flush().unwrap();
    drop(s);

    let mut s = open(lib.path());
    assert_eq!(state(&mut s, id), want, "after a restart");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let id = by_name(&s)["IMG_0002.jpg"];
    let mut again = want.clone();
    again["id"] = json!(id);
    assert_eq!(state(&mut s, id), again, "after a rebuild of the index");
    // The history goes on from there.
    redo(&mut s, id);
    assert_eq!(recipe(&s, id), (list(&[CROP, "rotate(90)", TUNING]), list(&["sepia()"])));
    assert_eq!(sidecar_entry(&mut s, &folder, "IMG_0002.jpg")["edits"], json!([CROP, "rotate(90)", TUNING]));
}

/// A photo step is never a library undo step, and a library step never walks the photo's history.
#[test]
fn the_library_stack_and_the_photo_history_stay_apart() {
    let (_lib, _d, _f, mut s, ids) = setup(2, &Value::Null);
    s.execute("photo.star.toggle", &json!({ "ids": [ids[1]] })).unwrap();
    assert_eq!(s.undo_stack().len(), 1);
    apply(&mut s, ids[0], CROP);
    apply(&mut s, ids[0], "rotate(90)");
    undo(&mut s, ids[0]);
    redo(&mut s, ids[0]);
    assert_eq!((s.undo_stack().len(), s.undo_stack().undo_label(), s.undo_stack().redo_label()), (1, Some("Star / Unstar"), None));
    // Library undo undoes the star, not the photo's edits.
    assert_eq!(s.execute("edit.undo", &json!({})).unwrap(), json!({ "undone": "Star / Unstar" }));
    assert!(!s.library().photo(ids[1]).unwrap().star);
    assert_eq!(recipe(&s, ids[0]).0, list(&[CROP, "rotate(90)"]));
    assert_eq!(s.execute("edit.undo", &json!({})), Err(CmdError::Disabled("Nothing to undo.".into())));
    // A rotation in the library is a library step; the photo's history then undoes its entry.
    s.execute("photo.rotate.cw", &json!({ "ids": [ids[0]] })).unwrap();
    assert_eq!(recipe(&s, ids[0]).0, list(&[CROP, "rotate(180)"]));
    assert_eq!(s.undo_stack().undo_label(), Some("Rotate Clockwise"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(recipe(&s, ids[0]), (list(&[CROP, "rotate(90)"]), vec![]));
    // The edit view's Undo by `edit.undo {id}` is the photo's.
    assert_eq!(s.execute("edit.undo", &json!({ "id": ids[0] })).unwrap(), json!({ "id": ids[0], "undone": "Rotate" }));
    assert_eq!(s.undo_stack().redo_label(), Some("Rotate Clockwise"), "the library's redo untouched");
}

#[test]
fn the_specs_name_their_histories() {
    let (_lib, _d, _f, s, _ids) = setup(1, &Value::Null);
    let history = |id: &str| s.spec(id).unwrap().history;
    assert_eq!(history("photo.history.apply"), History::Photo);
    assert_eq!(history("photo.history.state"), History::None);
    for id in ["photo.history.undo", "photo.history.redo"] {
        assert_eq!(history(id), History::None, "undo itself takes no step");
    }
    let listed: Vec<&str> = s.commands().iter().map(|c| c.id).filter(|c| c.starts_with("photo.history.")).collect();
    assert_eq!(history("photo.history.tuning"), History::Photo);
    assert_eq!(listed, ["photo.history.apply", "photo.history.undo", "photo.history.redo", "photo.history.state", "photo.history.tuning"]);
}

#[test]
fn the_owner_thread_makes_no_fs_call() {
    let (_lib, _d, folder, mut s, ids) = setup(1, &Value::Null);
    let ui = dalica_fs::mark_ui_thread();
    apply(&mut s, ids[0], "autocontrast()");
    apply(&mut s, ids[0], "autocolor()");
    undo(&mut s, ids[0]);
    redo(&mut s, ids[0]);
    state(&mut s, ids[0]);
    s.poll();
    s.flush().unwrap();
    drop(ui);
    assert_eq!(sidecar_entry(&mut s, &folder, "IMG_0001.jpg")["edits"], json!(["autocontrast()", "autocolor()"]));
}
