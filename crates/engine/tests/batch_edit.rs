//! Batch Edit, Copy and Paste All Edits, and Undo All Edits (`roadmap.md` M11.9; `editing.md`
//! §5.1 and §9) on temporary libraries: Auto Fix on 100 photos is one library undo step and one
//! `edit.undo` restores every recipe and redo list; two Batch Rotate Clockwise merge per photo;
//! Paste on five photos replaces their recipes and keeps an unknown operation of the copied
//! recipe; Paste with `history` is a step of that photo's history and not of the library; Undo All
//! Edits asks, empties the unsaved recipes, undo brings them back and a saved photo's file is
//! untouched; Sepia answers `disabled` with its reason; inside a group the change is the group's;
//! each command's contract and graceful failures.

use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::cmd::batch::{ARRIVES_WITH_EFFECTS, NO_EDITS, NO_EDITS_TO_COPY, NOTHING_COPIED};
use dalica_engine::{CmdError, Session};
use dalica_library::Recipe;
use serde_json::{Value, json};

use crate::common::{jpeg, lisbon, read};

fn recipe(s: &Session, id: PhotoId) -> Recipe {
    s.library().info(id).recipe()
}

fn edits(s: &Session, id: PhotoId) -> Vec<String> {
    s.library().info(id).edits.clone()
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|t| t.to_string()).collect()
}

fn photos(n: u64) -> Vec<Vec<u8>> {
    (1..=n).map(jpeg).collect()
}

#[test]
fn auto_fix_on_100_photos_is_one_step_and_one_undo_restores_every_recipe_and_redo_list() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos(100), None);
    // Some photos hold edits and a redo list already.
    for id in ids.iter().step_by(7) {
        s.execute("photo.history.apply", &json!({ "id": id, "op": "rotate(90)" })).unwrap();
        s.execute("photo.history.apply", &json!({ "id": id, "op": "autocolor()" })).unwrap();
        s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    }
    let before: Vec<Recipe> = ids.iter().map(|id| recipe(&s, *id)).collect();
    let steps = s.undo_stack().len();

    let v = s.execute("edit.apply", &json!({ "ids": ids, "op": "autofix" })).unwrap();
    assert_eq!(v["operation"], json!("autofix()"));
    assert_eq!(v["ids"].as_array().unwrap().len(), 100);
    assert_eq!(s.undo_stack().len(), steps + 1, "one library step");
    assert_eq!(s.undo_stack().undo_label(), Some("Auto Fix"), "the menu reads \"Undo Auto Fix\"");
    for (id, old) in ids.iter().zip(&before) {
        let now = recipe(&s, *id);
        assert_eq!(now.edits.last().map(String::as_str), Some("autofix()"));
        assert_eq!(now.edits.len(), old.edits.len() + 1);
        assert!(now.redo.is_empty(), "a new edit clears the redo list");
    }

    s.execute("edit.undo", &json!({})).unwrap();
    let after: Vec<Recipe> = ids.iter().map(|id| recipe(&s, *id)).collect();
    assert_eq!(after, before, "one undo restores all 100 recipes and redo lists exactly");
    assert_eq!(s.undo_stack().redo_label(), Some("Auto Fix"));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(ids.iter().all(|id| edits(&s, *id).last().map(String::as_str) == Some("autofix()")));
}

#[test]
fn two_batch_rotations_merge_per_photo() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos(3), None);
    s.execute("photo.history.apply", &json!({ "id": ids[1], "op": "autofix()" })).unwrap();
    for _ in 0..2 {
        s.execute("edit.apply", &json!({ "ids": ids[..2], "op": "rotate", "params": "90" })).unwrap();
    }
    assert_eq!(edits(&s, ids[0]), list(&["rotate(180)"]));
    assert_eq!(edits(&s, ids[1]), list(&["autofix()", "rotate(180)"]));
    assert!(edits(&s, ids[2]).is_empty());
    assert_eq!(s.undo_stack().undo_label(), Some("Rotate"));
    // The full form of the entry is the same operation.
    s.execute("edit.apply", &json!({ "ids": [ids[0]], "op": "rotate(180)" })).unwrap();
    assert!(edits(&s, ids[0]).is_empty(), "a whole turn removes the rotation");
}

#[test]
fn paste_on_five_photos_replaces_their_recipes_and_keeps_an_unknown_operation() {
    let sidecar = json!({
        "photos": {
            "IMG_0001.jpg": { "edits": ["autofix()", "future_op(1;2)", "rotate(90)"] },
            "IMG_0003.jpg": { "edits": ["bw()"], "redo": ["sepia()"] }
        }
    });
    let (_lib, _disk, folder, mut s, ids) = lisbon(&photos(6), Some(&sidecar));
    let copied = list(&["autofix()", "future_op(1;2)", "rotate(90)"]);
    assert_eq!(s.execute("edit.pasteRecipe", &json!({ "ids": [ids[1]] })), Err(CmdError::Disabled(NOTHING_COPIED.into())));
    let v = s.execute("edit.copyRecipe", &json!({ "id": ids[0] })).unwrap();
    assert_eq!(v["recipe"], json!(copied));

    let targets = &ids[1..6];
    let before: Vec<Recipe> = targets.iter().map(|id| recipe(&s, *id)).collect();
    let steps = s.undo_stack().len();
    let v = s.execute("edit.pasteRecipe", &json!({ "ids": targets })).unwrap();
    assert_eq!(v["ids"], json!(targets));
    assert_eq!(v["skipped"], json!([]));
    for id in targets {
        assert_eq!(recipe(&s, *id), Recipe { edits: copied.clone(), redo: vec![] });
    }
    assert_eq!(s.undo_stack().len(), steps + 1, "one library step");
    assert_eq!(s.undo_stack().undo_label(), Some("Paste All Edits"));
    s.flush().unwrap();
    let file: Value = serde_json::from_slice(&read(&folder.join(".dalica.json"))).unwrap();
    assert_eq!(file["photos"]["IMG_0006.jpg"]["edits"], json!(copied), "the unknown operation reaches the sidecar");

    s.execute("edit.undo", &json!({})).unwrap();
    let after: Vec<Recipe> = targets.iter().map(|id| recipe(&s, *id)).collect();
    assert_eq!(after, before, "undo gives each recipe and redo list back");
}

/// `AGENTS.md` §3, "Merge, never replace": a target's entry Dalica cannot read is never dropped
/// silently by Paste, as `edit.recipe.set` never drops it.
#[test]
fn paste_keeps_a_recipe_holding_an_entry_dalica_cannot_read_and_names_it() {
    let sidecar = json!({
        "photos": {
            "IMG_0001.jpg": { "edits": ["autofix()", "future_op(1;2)"] },
            "IMG_0002.jpg": { "edits": ["legacy:redeye(1)", "bw()"] },
            "IMG_0003.jpg": { "edits": ["future_op(1;2)", "rotate(90)"] }
        }
    });
    let (_lib, _disk, folder, mut s, ids) = lisbon(&photos(4), Some(&sidecar));
    s.execute("edit.copyRecipe", &json!({ "id": ids[0] })).unwrap();
    let held = recipe(&s, ids[1]);

    let v = s.execute("edit.pasteRecipe", &json!({ "ids": ids[1..] })).unwrap();
    assert_eq!(v["ids"], json!([ids[2], ids[3]]), "a photo whose unreadable entry the copied recipe holds too takes it");
    assert_eq!(v["skipped"].as_array().unwrap().len(), 1);
    assert_eq!(v["skipped"][0]["id"], json!(ids[1]));
    let reason = v["skipped"][0]["reason"].as_str().unwrap();
    assert!(reason.contains("legacy:redeye(1)"), "{reason}");
    assert_eq!(recipe(&s, ids[1]), held, "its recipe is kept");
    assert_eq!(edits(&s, ids[2]), list(&["autofix()", "future_op(1;2)"]));
    s.flush().unwrap();
    let file: Value = serde_json::from_slice(&read(&folder.join(".dalica.json"))).unwrap();
    assert_eq!(file["photos"]["IMG_0002.jpg"]["edits"], json!(["legacy:redeye(1)", "bw()"]));

    // The history form is refused, and nothing changes.
    let r = s.execute("edit.pasteRecipe", &json!({ "id": ids[1], "history": true }));
    assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("legacy:redeye(1)")), "{r:?}");
    assert_eq!(recipe(&s, ids[1]), held);
}

/// The copied recipe belongs to the session: a new session of the same library has none.
#[test]
fn the_copied_recipe_lasts_as_long_as_the_session() {
    let (lib, _disk, _folder, mut s, ids) = lisbon(&photos(2), None);
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autofix()" })).unwrap();
    s.execute("edit.copyRecipe", &json!({ "id": ids[0] })).unwrap();
    s.flush().unwrap();
    drop(s);
    let mut s = crate::common::open(lib.path());
    assert_eq!(s.execute("edit.pasteRecipe", &json!({ "ids": [ids[1]] })), Err(CmdError::Disabled(NOTHING_COPIED.into())));
}

#[test]
fn paste_with_history_is_a_step_of_the_photos_history_and_not_of_the_library() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos(2), None);
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "crop(0.1,0.1,0.9,0.9)" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autocontrast()" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids[1], "op": "rotate(270)" })).unwrap();
    s.execute("edit.copyRecipe", &json!({ "id": ids[0] })).unwrap();
    let steps = s.undo_stack().len();

    let v = s.execute("edit.pasteRecipe", &json!({ "id": ids[1], "history": true })).unwrap();
    assert_eq!(edits(&s, ids[1]), list(&["crop(0.1,0.1,0.9,0.9)", "autocontrast()"]));
    assert_eq!(v["undoLabel"], json!("Undo Auto Contrast"));
    assert_eq!(s.undo_stack().len(), steps, "no library step");

    s.execute("photo.history.undo", &json!({ "id": ids[1] })).unwrap();
    assert_eq!(edits(&s, ids[1]), list(&["rotate(270)"]), "one photo undo gives the whole recipe back");
}

#[test]
fn undo_all_edits_asks_empties_the_recipes_and_a_saved_photo_keeps_its_file() {
    let (_lib, _disk, folder, mut s, ids) = lisbon(&photos(3), None);
    // Photo 1 is saved with Auto Fix, then turned (an unsaved edit).
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autofix()" })).unwrap();
    let reply = s.execute("photo.save", &json!({ "ids": [ids[0]], "confirm": true })).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    assert!(s.wait_job(job, Duration::from_secs(120)).unwrap().state.is_finished());
    assert!(s.library().saved(ids[0]).is_some(), "photo 1 is saved");
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "rotate(90)" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids[1], "op": "autocolor()" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids[1], "op": "bw()" })).unwrap();
    s.execute("photo.history.undo", &json!({ "id": ids[1] })).unwrap();
    s.flush().unwrap();
    let saved_file = read(&folder.join("IMG_0001.jpg"));
    let before: Vec<Recipe> = ids.iter().map(|id| recipe(&s, *id)).collect();
    let steps = s.undo_stack().len();

    let asked = s.execute("edit.undoAll", &json!({ "ids": ids }));
    let Err(CmdError::Disabled(question)) = asked else { panic!("asks first: {asked:?}") };
    assert!(question.starts_with("Remove all edits from 2 pictures?"), "{question}");
    assert!(question.contains("IMG_0001.jpg") && question.contains("\"confirm\": true"), "{question}");
    assert_eq!(s.undo_stack().len(), steps, "asking changes nothing");

    let v = s.execute("edit.undoAll", &json!({ "ids": ids, "confirm": true })).unwrap();
    assert_eq!(v["ids"], json!(ids[..2]));
    assert_eq!(v["saved"], json!([{ "id": ids[0], "name": "IMG_0001.jpg" }]));
    assert!(ids.iter().all(|id| recipe(&s, *id) == Recipe::default()), "every unsaved recipe and redo list is empty");
    assert_eq!(s.undo_stack().len(), steps + 1);
    assert_eq!(s.undo_stack().undo_label(), Some("Remove Edits"));
    assert!(s.library().saved(ids[0]).is_some(), "the saved record stays");
    s.flush().unwrap();
    assert_eq!(read(&folder.join("IMG_0001.jpg")), saved_file, "the saved photo's file is untouched");

    s.execute("edit.undo", &json!({})).unwrap();
    let after: Vec<Recipe> = ids.iter().map(|id| recipe(&s, *id)).collect();
    assert_eq!(after, before, "undo brings the recipes back");

    // With nothing left to remove it is greyed.
    assert_eq!(s.execute("edit.undoAll", &json!({ "ids": [ids[2]], "confirm": true })), Err(CmdError::Disabled(NO_EDITS.into())));
}

#[test]
fn undo_all_with_history_is_one_step_of_the_photos_history() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos(1), None);
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autofix()" })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "rotate(90)" })).unwrap();
    let steps = s.undo_stack().len();
    assert!(matches!(s.execute("edit.undoAll", &json!({ "id": ids[0], "history": true })), Err(CmdError::Disabled(_))));
    s.execute("edit.undoAll", &json!({ "id": ids[0], "history": true, "confirm": true })).unwrap();
    assert!(edits(&s, ids[0]).is_empty());
    assert_eq!(s.undo_stack().len(), steps, "no library step");
    s.execute("photo.history.undo", &json!({ "id": ids[0] })).unwrap();
    assert_eq!(edits(&s, ids[0]), list(&["autofix()", "rotate(90)"]));
}

#[test]
fn sepia_is_disabled_with_its_reason() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos(1), None);
    let p = json!({ "ids": [ids[0]], "op": "sepia" });
    assert_eq!(s.execute("edit.apply", &p), Err(CmdError::Disabled(ARRIVES_WITH_EFFECTS.into())));
    assert_eq!(s.execute("edit.apply", &json!({ "ids": [ids[0]], "op": "sepia()" })), Err(CmdError::Disabled(ARRIVES_WITH_EFFECTS.into())));
    assert!(edits(&s, ids[0]).is_empty());
    assert_eq!(s.undo_stack().len(), 0);
}

#[test]
fn inside_a_group_the_change_is_the_groups_and_a_failing_step_takes_it_back() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos(2), None);
    let steps = [("edit.apply", json!({ "ids": ids, "op": "autofix" })), ("photo.star.set", json!({ "ids": [ids[0]], "on": true }))];
    s.execute_group(&steps, "Set Metadata").unwrap();
    assert_eq!(s.undo_stack().len(), 1, "one step for the group");
    assert_eq!(s.undo_stack().undo_label(), Some("Set Metadata"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(ids.iter().all(|id| edits(&s, *id).is_empty()));

    let failing = [("edit.apply", json!({ "ids": ids, "op": "autofix" })), ("photo.tags.add", json!({ "ids": [999_999], "tags": "x" }))];
    assert!(s.execute_group(&failing, "Set Metadata").is_err());
    assert!(ids.iter().all(|id| edits(&s, *id).is_empty()), "the failed group's batch edit is taken back");
}

#[test]
fn each_command_fails_gracefully() {
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos(2), None);
    let model = s.library().model().clone();
    let cases: Vec<(&str, Value)> = vec![
        ("edit.apply", json!({ "ids": [], "op": "autofix" })),
        ("edit.apply", json!({ "ids": [999_999], "op": "autofix" })),
        ("edit.apply", json!({ "ids": [ids[0]], "op": "nonsense" })),
        ("edit.apply", json!({ "ids": [ids[0]], "op": "legacy:redeye(1)" })),
        ("edit.apply", json!({ "ids": [ids[0]], "op": "crop", "params": "a,b" })),
        ("edit.apply", json!({ "ids": [ids[0]], "op": "trim" })),
        ("edit.apply", json!({ "ids": [ids[0]], "op": "rotate", "params": "45" })),
        ("edit.apply", json!({ "ids": [ids[0]], "op": "rotate(90)", "params": "90" })),
        ("edit.apply", json!({ "ids": [ids[0]], "op": "x".repeat(100_000) })),
        ("edit.copyRecipe", json!({ "id": ids[0] })),
        ("edit.copyRecipe", json!({ "id": 999_999 })),
        ("edit.pasteRecipe", json!({ "ids": [ids[0]] })),
        ("edit.undoAll", json!({ "ids": [ids[0]], "confirm": true })),
        ("edit.undoAll", json!({ "ids": [999_999], "confirm": true })),
        ("edit.undoAll", json!({ "ids": [ids[0]], "history": true, "confirm": true })),
        ("edit.undoAll", json!({ "id": ids[0], "confirm": true })),
    ];
    for (id, p) in &cases {
        let r = s.execute(id, p);
        assert!(matches!(r, Err(CmdError::Disabled(_) | CmdError::NotFound(_) | CmdError::BadParams(_) | CmdError::Limit(_))), "{id} {p}: {r:?}");
    }
    assert_eq!(s.execute("edit.copyRecipe", &json!({ "id": ids[0] })), Err(CmdError::Disabled(NO_EDITS_TO_COPY.into())));
    assert_eq!(s.library().model(), &model, "nothing changed");
    assert_eq!(s.undo_stack().len(), 0);

    // With a recipe copied, Paste's own shapes are checked.
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autofix()" })).unwrap();
    s.execute("edit.copyRecipe", &json!({ "id": ids[0] })).unwrap();
    for p in [json!({ "ids": [ids[1]], "history": true }), json!({ "id": ids[1] }), json!({ "ids": [ids[1]], "confirm": true }), json!({ "ids": [] })]
    {
        let r = s.execute("edit.pasteRecipe", &p);
        assert!(matches!(r, Err(CmdError::Disabled(_) | CmdError::BadParams(_))), "{p}: {r:?}");
    }
    assert!(edits(&s, ids[1]).is_empty());
    // A photo whose recipe already is the copied one takes no step.
    let steps = s.undo_stack().len();
    let v = s.execute("edit.pasteRecipe", &json!({ "ids": [ids[0]] })).unwrap();
    assert_eq!(v["ids"], json!([]));
    assert_eq!(s.undo_stack().len(), steps);
}
