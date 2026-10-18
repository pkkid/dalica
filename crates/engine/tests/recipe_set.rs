//! `edit.recipe.set` (`roadmap.md` M10.9; `mcp.md` §4.2 `set_recipe`, `data-model.md` §3.1) on a
//! temporary library: a recipe set on two photos is one step of each photo's own history, written
//! to the sidecar, and one undo per photo gives its whole recipe back; the string and list forms
//! are the same recipe; an operation Dalica cannot read is refused by name and kept, and replaced
//! with `dropUnknown`; a bad parameter, or one photo that would lose an operation, changes no photo.

use crate::common::{jpeg, lisbon};
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use serde_json::{Value, json};

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|t| t.to_string()).collect()
}

fn edits(s: &Session, id: PhotoId) -> Vec<String> {
    s.library().info(id).edits.clone()
}

fn set(s: &mut Session, p: Value) -> Result<Value, CmdError> {
    s.execute("edit.recipe.set", &p)
}

/// Three photos; the third's sidecar entry holds a legacy entry and an unknown one.
fn setup() -> (tempfile::TempDir, tempfile::TempDir, std::path::PathBuf, Session, Vec<PhotoId>) {
    let sidecar = json!({
        "photos": { "IMG_0003.jpg": { "edits": ["rotate(90)", "legacy:redeye(1)", "future_op(1;2)"] } }
    });
    lisbon(&[jpeg(1), jpeg(2), jpeg(3)], Some(&sidecar))
}

#[test]
fn a_set_on_two_photos_is_one_step_of_each_history_and_undo_restores_both() {
    let (_lib, _disk, folder, mut s, ids) = setup();
    let (a, b) = (ids[0], ids[1]);
    s.execute("photo.history.apply", &json!({ "id": a, "op": "crop(0.1,0.1,0.9,0.9)" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": a, "op": "sepia()" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": b, "op": "rotate(270)" })).unwrap();
    let library_steps = s.undo_stack().len();

    let v = set(&mut s, json!({ "ids": [a, b], "recipe": "autofix()|rotate(90)|rotate(90)|bw()", "label": "Paste All Edits" })).unwrap();
    let new = list(&["autofix()", "rotate(90)", "rotate(90)", "bw()"]);
    assert_eq!(edits(&s, a), new, "verbatim: the two rotations are not merged");
    assert_eq!(edits(&s, b), new);
    assert_eq!(v["photos"][0]["changed"], json!(true));
    assert_eq!(v["photos"][1]["undoLabel"], json!("Undo B&W"));
    assert_eq!(s.undo_stack().len(), library_steps, "photo steps never reach the library stack");
    s.flush().unwrap();
    let sidecar: Value = serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(sidecar["photos"]["IMG_0002.jpg"]["edits"], json!(new));

    // The same recipe again takes no step.
    let v = set(&mut s, json!({ "ids": [a], "recipe": new })).unwrap();
    assert_eq!(v["photos"][0]["changed"], json!(false));

    // One undo per photo gives its whole recipe back; redo the whole new one.
    s.execute("photo.history.undo", &json!({ "id": a })).unwrap();
    s.execute("photo.history.undo", &json!({ "id": b })).unwrap();
    assert_eq!(edits(&s, a), list(&["crop(0.1,0.1,0.9,0.9)", "sepia()"]));
    assert_eq!(edits(&s, b), list(&["rotate(270)"]));
    s.execute("photo.history.redo", &json!({ "id": a })).unwrap();
    assert_eq!(edits(&s, a), new);
    s.execute("photo.history.undo", &json!({ "id": a })).unwrap();
    s.execute("photo.history.undo", &json!({ "id": a })).unwrap();
    assert_eq!(edits(&s, a), list(&["crop(0.1,0.1,0.9,0.9)"]), "the steps before it are untouched");

    // The list form, with entries and {op, params}, is the same recipe; an empty one clears.
    let ops = json!(["autofix()", { "op": "rotate", "params": "90" }, { "op": "rotate", "params": "90" }, { "op": "bw" }]);
    set(&mut s, json!({ "ids": [a], "recipe": ops })).unwrap();
    assert_eq!(edits(&s, a), new);
    let v = set(&mut s, json!({ "ids": [a], "recipe": "" })).unwrap();
    assert_eq!(edits(&s, a), Vec::<String>::new());
    assert_eq!(v["photos"][0]["undoLabel"], json!("Undo Edit"));
    s.execute("photo.history.undo", &json!({ "id": a })).unwrap();
    assert_eq!(edits(&s, a), new);
}

#[test]
fn an_operation_dalica_cannot_read_is_refused_and_kept_and_replaced_with_drop_unknown() {
    let (_lib, _disk, _folder, mut s, ids) = setup();
    let c = ids[2];
    let held = list(&["rotate(90)", "legacy:redeye(1)", "future_op(1;2)"]);
    let Err(CmdError::BadParams(why)) = set(&mut s, json!({ "ids": [c], "recipe": "sepia()" })) else { panic!("refused") };
    assert!(why.contains("legacy:redeye(1)") && why.contains("future_op(1;2)") && why.contains("dropUnknown"), "{why}");
    assert_eq!(edits(&s, c), held);

    // Keeping them, in any place, is allowed; adding one the photo does not hold is not.
    set(&mut s, json!({ "ids": [c], "recipe": ["future_op(1;2)", "sepia()", "legacy:redeye(1)"] })).unwrap();
    assert_eq!(edits(&s, c), list(&["future_op(1;2)", "sepia()", "legacy:redeye(1)"]));
    let Err(CmdError::BadParams(why)) = set(&mut s, json!({ "ids": [c], "recipe": ["future_op(1;2)", "future_op(1;2)", "legacy:redeye(1)"] })) else {
        panic!("a second copy is not held")
    };
    assert!(why.contains("future_op(1;2)"), "{why}");

    let v = set(&mut s, json!({ "ids": [c], "recipe": "sepia()", "dropUnknown": true })).unwrap();
    assert_eq!(edits(&s, c), list(&["sepia()"]));
    assert_eq!(v["photos"][0]["canUndo"], json!(true));
    s.execute("photo.history.undo", &json!({ "id": c })).unwrap();
    assert_eq!(edits(&s, c), list(&["future_op(1;2)", "sepia()", "legacy:redeye(1)"]), "undo gives them back");
}

#[test]
fn a_bad_parameter_or_one_refused_photo_changes_no_photo() {
    let (_lib, _disk, _folder, mut s, ids) = setup();
    let before: Vec<Vec<String>> = ids.iter().map(|id| edits(&s, *id)).collect();
    let model = s.library().model().clone();
    for (recipe, ids) in [
        // A crop its module refuses (reversed), Tuning with NaN, an entry of a newer version.
        (json!("autofix()|crop(0.9,0.1,0.2,0.8)"), json!([ids[0], ids[1]])),
        (json!(["tuning(NaN,0,0,0,none)"]), json!([ids[0], ids[1]])),
        (json!(["autofix()", "rotate(45)"]), json!([ids[0]])),
        // The third photo would lose its legacy entry: the first two do not change either.
        (json!("autofix()"), json!([ids[0], ids[1], ids[2]])),
    ] {
        assert!(matches!(set(&mut s, json!({ "ids": ids, "recipe": recipe })), Err(CmdError::BadParams(_))), "{recipe}");
        assert_eq!(s.library().model(), &model, "{recipe}");
    }
    assert_eq!(ids.iter().map(|id| edits(&s, *id)).collect::<Vec<_>>(), before);
    // Disabled without photos; an unknown photo is not found and changes nothing.
    assert!(matches!(set(&mut s, json!({ "ids": [], "recipe": "bw()" })), Err(CmdError::Disabled(_))));
    assert!(matches!(set(&mut s, json!({ "ids": [ids[0], 999_999], "recipe": "bw()" })), Err(CmdError::NotFound(_))));
    assert_eq!(s.library().model(), &model);
}
