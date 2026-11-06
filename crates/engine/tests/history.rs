//! The History pane's commands (M16.33, issue #726; `editing.md` §5.1): `photo.history` lists a
//! photo's steps as rows, oldest first (Original, then the recipe, then the redo list), with each
//! row's label and detail and the current row marked; `photo.history.goto` moves the photo to a row
//! in one command, both ways, keeping the redoable steps until a new edit replaces them, and never
//! on the library undo stack (bad parameters: the engine's every-command test and `panic_hunt`).

use crate::common::{jpeg, lisbon};
use dalica_core::PhotoId;
use dalica_engine::Session;
use serde_json::{Value, json};

fn apply(s: &mut Session, id: PhotoId, op: &str) {
    s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
}

fn rows(s: &mut Session, id: PhotoId) -> Value {
    s.execute("photo.history", &json!({ "id": id })).unwrap()
}

/// Each row as (label, detail, redoable).
fn shown(v: &Value) -> Vec<(String, Option<String>, bool)> {
    let steps = v["steps"].as_array().unwrap();
    steps.iter().map(|r| (r["label"].as_str().unwrap().to_string(), r["detail"].as_str().map(str::to_string), r["redo"].as_bool().unwrap())).collect()
}

fn row(label: &str, detail: Option<&str>, redo: bool) -> (String, Option<String>, bool) {
    (label.to_string(), detail.map(str::to_string), redo)
}

#[test]
fn three_edits_and_an_undo_list_as_rows_with_the_current_one_marked() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&[jpeg(1)], None);
    let id = ids[0];
    let v = rows(&mut s, id);
    assert_eq!((shown(&v), v["current"].clone()), (vec![row("Original", None, false)], json!(0)), "no edits: Original alone");

    // 96 × 64: half its width is a 3:4 crop.
    apply(&mut s, id, "crop(0,0,0.5,1)");
    apply(&mut s, id, "straighten(1.4)");
    apply(&mut s, id, "sepia()");
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    let v = rows(&mut s, id);
    let want = vec![row("Original", None, false), row("Crop", Some("3:4"), false), row("Straighten", Some("1.4°"), false), row("Sepia", None, true)];
    assert_eq!(shown(&v), want);
    assert_eq!(v["current"], json!(2), "at Straighten; Sepia is redoable");
    assert_eq!((v["steps"][0]["kind"].clone(), v["steps"][3]["op"].clone()), (json!("original"), json!("sepia()")));
}

#[test]
fn goto_moves_both_ways_in_one_command_and_keeps_the_redoable_steps() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&[jpeg(2)], None);
    let id = ids[0];
    for op in ["autocontrast()", "rotate(90)", "tuning(0.3,0,0,0,none)"] {
        apply(&mut s, id, op);
    }
    assert_eq!(s.undo_stack().len(), 0);

    let v = s.execute("photo.history.goto", &json!({ "id": id, "index": 0 })).unwrap();
    assert_eq!((v["current"].clone(), v["moved"].clone()), (json!(0), json!(3)), "Original: every step undone at once");
    assert!(s.library().info(id).edits.is_empty());
    let back = shown(&v);
    assert_eq!(back.len(), 4, "and all three still listed");
    assert!(back.iter().skip(1).all(|(_, _, redo)| *redo), "as redoable");
    assert_eq!(back[3], row("Tuning", Some("Fill Light 0.30"), true));

    let v = s.execute("photo.history.goto", &json!({ "id": id, "index": 2 })).unwrap();
    assert_eq!((v["current"].clone(), v["moved"].clone()), (json!(2), json!(2)));
    assert_eq!(s.library().info(id).edits, vec!["autocontrast()".to_string(), "rotate(90)".to_string()]);
    let v = s.execute("photo.history.goto", &json!({ "id": id, "index": 2 })).unwrap();
    assert_eq!(v["moved"], json!(0), "already there: nothing moves");
    assert!(s.execute("photo.history.goto", &json!({ "id": id, "index": 4 })).is_err(), "past the last row");

    // A new edit replaces the redoable step.
    apply(&mut s, id, "sepia()");
    let labels: Vec<String> = shown(&rows(&mut s, id)).into_iter().map(|(l, _, _)| l).collect();
    assert_eq!(labels, ["Original", "Auto Contrast", "Rotate", "Sepia"]);
    assert_eq!(s.undo_stack().len(), 0, "the library undo stack never saw a step");
}
