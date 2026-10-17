//! A│A's variant (`roadmap.md` M11.10; `editing.md` §4) on a temporary library: a scratch history
//! of one photo held in the session only. Auto Fix on the variant leaves the photo's sidecar as it
//! was; keeping the right side is one step of the photo's history that one undo removes; keeping
//! the left drops the variant; a restart has none; graceful failures.

use std::path::Path;

use crate::common::{jpeg, lisbon, open};
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use serde_json::{Value, json};

const CROP: &str = "crop(0.1,0.1,0.9,0.9)";

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|t| t.to_string()).collect()
}

/// The folder's sidecar, as bytes, after every pending write landed.
fn sidecar(s: &mut Session, folder: &Path) -> Vec<u8> {
    s.flush().unwrap();
    std::fs::read(folder.join(".dalica.json")).unwrap_or_default()
}

fn on_variant(s: &mut Session, cmd: &str, id: PhotoId, extra: Value) -> Result<Value, CmdError> {
    let mut p = json!({ "id": id, "variant": true });
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    s.execute(cmd, &p)
}

fn variant_edits(s: &mut Session, id: PhotoId) -> Value {
    on_variant(s, "photo.history.state", id, json!({})).unwrap()["edits"].clone()
}

/// Auto Fix (and its undo and redo) on the variant walks the variant's recipe only: the photo's
/// recipe, its history and its sidecar stay as they were, and the variant's pane is not the photo.
#[test]
fn auto_fix_on_the_variant_leaves_the_photos_sidecar_unchanged() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1), jpeg(2)], None);
    let id = ids[0];
    s.execute("photo.history.apply", &json!({ "id": id, "op": CROP })).unwrap();
    let before = sidecar(&mut s, &folder);
    assert!(!before.is_empty(), "the crop was written");

    let opened = s.execute("edit.variant.open", &json!({ "id": id })).unwrap();
    assert_eq!(opened["started"], json!(true));
    assert_eq!(opened["variant"], json!(true));
    assert_eq!(opened["edits"], json!([CROP]), "a copy of the photo's recipe");
    assert_eq!(opened["undoLabel"], json!("Undo Crop"));
    assert_eq!(s.variant().photo(), Some(id));

    let v = on_variant(&mut s, "photo.history.apply", id, json!({ "op": "autofix()" })).unwrap();
    assert_eq!(v["edits"], json!([CROP, "autofix()"]));
    assert_eq!(v["undoLabel"], json!("Undo Auto Fix"));
    assert_eq!(s.library().info(id).edits, list(&[CROP]), "the photo's recipe is untouched");
    assert_eq!(s.photo_history(id).unwrap().undo_text(), "Undo Crop");
    assert_eq!(sidecar(&mut s, &folder), before, "nothing of the variant reached the sidecar");

    // Its own undo and redo.
    assert_eq!(on_variant(&mut s, "photo.history.undo", id, json!({})).unwrap()["undone"], json!("Auto Fix"));
    assert_eq!(variant_edits(&mut s, id), json!([CROP]));
    assert_eq!(on_variant(&mut s, "photo.history.state", id, json!({})).unwrap()["redoLabel"], json!("Redo Auto Fix"));
    assert_eq!(on_variant(&mut s, "photo.history.redo", id, json!({})).unwrap()["redone"], json!("Auto Fix"));
    assert_eq!(variant_edits(&mut s, id), json!([CROP, "autofix()"]));
    assert_eq!(s.library().info(id).recipe().redo, Vec::<String>::new());
    assert_eq!(sidecar(&mut s, &folder), before);

    // A step of the photo's own history leaves the variant as it is.
    s.execute("photo.history.apply", &json!({ "id": id, "op": "sepia()" })).unwrap();
    assert_eq!(variant_edits(&mut s, id), json!([CROP, "autofix()"]));
    // Opening it again keeps it.
    let again = s.execute("edit.variant.open", &json!({ "id": id })).unwrap();
    assert_eq!((again["started"].clone(), again["edits"].clone()), (json!(false), json!([CROP, "autofix()"])));
}

/// Keeping the right side makes the variant's recipe one step of the photo's history ("Undo Auto
/// Fix", named by its last entry), written to the sidecar; one undo gives the photo's recipe
/// before it back. The variant is gone.
#[test]
fn keeping_the_right_side_is_one_history_step_that_one_undo_removes() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1)], None);
    let id = ids[0];
    s.execute("photo.history.apply", &json!({ "id": id, "op": CROP })).unwrap();
    s.execute("edit.variant.open", &json!({ "id": id })).unwrap();
    for op in ["sepia()", "rotate(90)", "autofix()"] {
        on_variant(&mut s, "photo.history.apply", id, json!({ "op": op })).unwrap();
    }
    let undo_depth = s.undo_stack().len();
    let kept = s.execute("edit.variant.close", &json!({ "keep": "right" })).unwrap();
    assert_eq!(kept["kept"], json!("right"));
    assert_eq!(kept["changed"], json!(true));
    assert_eq!(kept["edits"], json!([CROP, "sepia()", "rotate(90)", "autofix()"]));
    assert_eq!(kept["undoLabel"], json!("Undo Auto Fix"));
    assert_eq!(s.variant().photo(), None);
    assert!(matches!(on_variant(&mut s, "photo.history.state", id, json!({})), Err(CmdError::Disabled(_))));
    assert_eq!(s.undo_stack().len(), undo_depth, "a photo step, not a library one");
    let written: Value = serde_json::from_slice(&sidecar(&mut s, &folder)).unwrap();
    assert_eq!(written["photos"]["IMG_0001.jpg"]["edits"], json!([CROP, "sepia()", "rotate(90)", "autofix()"]));

    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(s.library().info(id).edits, list(&[CROP]), "one undo removes the whole kept version");
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap();
    assert_eq!(s.library().info(id).edits, list(&[CROP, "sepia()", "rotate(90)", "autofix()"]));

    // A variant kept unchanged takes no step.
    s.execute("edit.variant.open", &json!({ "id": id })).unwrap();
    let same = s.execute("edit.variant.close", &json!({ "keep": "right" })).unwrap();
    assert_eq!(same["changed"], json!(false));
    assert_eq!(s.photo_history(id).unwrap().undo_text(), "Undo Auto Fix");
}

/// Keeping the left side drops the variant: the photo keeps its recipe and history, and another
/// photo's variant may open.
#[test]
fn keeping_the_left_drops_the_variant() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1), jpeg(2)], None);
    let id = ids[0];
    s.execute("photo.history.apply", &json!({ "id": id, "op": CROP })).unwrap();
    let before = sidecar(&mut s, &folder);
    s.execute("edit.variant.open", &json!({ "id": id })).unwrap();
    on_variant(&mut s, "photo.history.apply", id, json!({ "op": "bw()" })).unwrap();
    let kept = s.execute("edit.variant.close", &json!({ "keep": "left" })).unwrap();
    assert_eq!((kept["kept"].clone(), kept["changed"].clone(), kept["edits"].clone()), (json!("left"), json!(false), json!([CROP])));
    assert_eq!(s.variant().photo(), None);
    assert_eq!(s.photo_history(id).unwrap().undo_text(), "Undo Crop");
    assert_eq!(sidecar(&mut s, &folder), before);
    assert!(matches!(on_variant(&mut s, "photo.history.apply", id, json!({ "op": "bw()" })), Err(CmdError::Disabled(_))));
    assert_eq!(s.execute("edit.variant.open", &json!({ "id": ids[1] })).unwrap()["started"], json!(true));
}

/// The variant lives in the session only: after a restart there is none, and the photo's recipe is
/// what it was before the variant opened.
#[test]
fn a_restart_has_no_variant() {
    let (lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1)], None);
    let id = ids[0];
    s.execute("photo.history.apply", &json!({ "id": id, "op": CROP })).unwrap();
    s.execute("edit.variant.open", &json!({ "id": id })).unwrap();
    on_variant(&mut s, "photo.history.apply", id, json!({ "op": "sepia()" })).unwrap();
    let before = sidecar(&mut s, &folder);
    drop(s);
    let mut s = open(lib.path());
    s.wait_idle();
    assert_eq!(s.variant().photo(), None);
    let id = s.library().photos().next().unwrap().id;
    assert_eq!(s.library().info(id).edits, list(&[CROP]));
    assert!(matches!(on_variant(&mut s, "photo.history.state", id, json!({})), Err(CmdError::Disabled(_))));
    assert!(matches!(s.execute("edit.variant.close", &json!({ "keep": "right" })), Err(CmdError::Disabled(_))));
    assert_eq!(sidecar(&mut s, &folder), before);
}

/// Graceful failures: unknown photos, a second photo's variant, closing with none open, bad
/// parameters, steps on a variant that is not open, and a variant whose photo left the library.
#[test]
fn graceful_failures() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&[jpeg(1), jpeg(2)], None);
    let (a, b) = (ids[0], ids[1]);
    assert!(matches!(s.execute("edit.variant.open", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
    assert!(matches!(s.execute("edit.variant.open", &json!({})), Err(CmdError::BadParams(_))));
    assert!(matches!(s.execute("edit.variant.close", &json!({ "keep": "left" })), Err(CmdError::Disabled(_))));
    for cmd in ["photo.history.state", "photo.history.undo", "photo.history.redo"] {
        assert!(matches!(on_variant(&mut s, cmd, a, json!({})), Err(CmdError::Disabled(_))), "{cmd}");
        assert!(matches!(s.execute(cmd, &json!({ "id": a, "variant": "yes" })), Err(CmdError::BadParams(_))), "{cmd}");
    }
    assert!(matches!(on_variant(&mut s, "photo.history.apply", a, json!({ "op": "sepia()" })), Err(CmdError::Disabled(_))));

    s.execute("edit.variant.open", &json!({ "id": a })).unwrap();
    assert!(matches!(s.execute("edit.variant.open", &json!({ "id": b })), Err(CmdError::Disabled(_))), "one variant at a time");
    assert!(matches!(on_variant(&mut s, "photo.history.state", b, json!({})), Err(CmdError::Disabled(_))));
    assert!(matches!(on_variant(&mut s, "photo.history.undo", a, json!({})), Err(CmdError::Disabled(_))), "nothing to undo");
    assert!(matches!(on_variant(&mut s, "photo.history.redo", a, json!({})), Err(CmdError::Disabled(_))), "nothing to redo");
    assert!(matches!(on_variant(&mut s, "photo.history.apply", a, json!({ "op": "future_op(1)" })), Err(CmdError::BadParams(_))));
    for keep in [json!("middle"), json!(1), json!(null)] {
        assert!(matches!(s.execute("edit.variant.close", &json!({ "keep": keep })), Err(CmdError::BadParams(_))), "{keep}");
    }
    assert!(matches!(s.execute("edit.variant.close", &json!({})), Err(CmdError::BadParams(_))));
    assert_eq!(s.variant().photo(), Some(a), "a refused close keeps the variant");

    // The variant's photo leaves the library: its right side cannot be kept, its left can.
    on_variant(&mut s, "photo.history.apply", a, json!({ "op": "sepia()" })).unwrap();
    s.execute("folders.set", &json!({ "changes": [{ "path": folder, "mode": "remove" }] })).unwrap();
    s.wait_idle();
    assert!(s.library().photo(a).is_none());
    assert!(matches!(s.execute("edit.variant.close", &json!({ "keep": "right" })), Err(CmdError::NotFound(_))));
    assert!(matches!(on_variant(&mut s, "photo.history.state", a, json!({})), Err(CmdError::NotFound(_))));
    s.execute("edit.variant.close", &json!({ "keep": "left" })).unwrap();
    assert_eq!(s.variant().photo(), None);
}
