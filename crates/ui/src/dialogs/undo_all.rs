//! Picture ▸ Undo All Edits… (`editing.md` §9): "Remove all edits from %d pictures?" [Remove Edits]
//! [Cancel], naming the saved photos (which keep their saved pixels; Undo Save gives the original
//! back) when there are. Opened by `crate::commands::batch::UNDO_ALL` for the tray's photos (the
//! open photo in the edit view); Remove Edits runs `edit.undoAll` for them with `"confirm": true`:
//! one library undo step, or in the edit view one step of the open photo's history.
//!
//! Its draft (`dialogs[].fields`): `ids` (or `id` with `history: true`) and `question`, the text it
//! asks, worded by the engine (`dalica_engine::cmd::batch::question`).

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{Label, RichText, Ui};
use serde_json::{Map, Value};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Undo All Edits";
/// The Remove Edits button.
pub const REMOVE: &str = "Remove Edits";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;
/// Remove Edits on a draft that names no photo.
pub const NOTHING: &str = "Select some photos first.";
/// The command Remove Edits runs.
const COMMAND: &str = "edit.undoAll";

/// A dialog of this kind with an empty draft.
pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::UndoAllEdits)
}

/// Opens the question with `draft` (`ids` or `id` and `history`, and `question`). Replies
/// `{dialog: "undoAllEdits"}`.
pub fn open(app: &mut App, draft: Map<String, Value>) -> Result<Value, CmdError> {
    let mut d = dialog();
    d.fields = draft;
    super::open(app, d)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::new("removeEdits", REMOVE, ButtonKind::Default), Button::cancel()] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let text = d.fields.get("question").and_then(Value::as_str).unwrap_or_default();
    ui.add(Label::new(RichText::new(text).font(fonts::regular(t.body)).color(t.text)).wrap());
}

/// Remove Edits: `edit.undoAll` for the photos of the draft, confirmed.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "removeEdits" => {
            let mut params: Map<String, Value> =
                d.fields.iter().filter(|(k, _)| matches!(k.as_str(), "ids" | "id" | "history")).map(|(k, v)| (k.clone(), v.clone())).collect();
            // A draft naming no photo (opened empty by a script) removes nothing.
            if !params.contains_key("ids") && !params.contains_key("id") {
                return Err(CmdError::Disabled(NOTHING.into()));
            }
            params.insert("confirm".into(), Value::Bool(true));
            app.run(COMMAND, &Value::Object(params))
        }
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::dialogs;

    #[test]
    fn the_question_counts_the_edited_photos_and_remove_edits_removes_them_in_one_step() {
        let (_dir, s, ids) = session_with_photos(3);
        let mut app = App::new(Some(s));
        let o = crate::commands::opener(COMMAND).unwrap();
        assert!(matches!((o.open)(&mut app, &json!({ "ids": [ids[0]] })), Err(CmdError::Disabled(_))), "no edits to remove");
        app.run("edit.apply", &json!({ "ids": [ids[0], ids[1]], "op": "autofix" })).unwrap();
        assert_eq!((o.open)(&mut app, &json!({ "ids": ids })).unwrap(), json!({ "dialog": "undoAllEdits" }));
        let inspect = dialogs::inspect(&app);
        assert_eq!(inspect["fields"]["question"], "Remove all edits from 2 pictures?");
        assert_eq!(inspect["default"], "button:dialog.removeEdits");
        dialogs::answer(&mut app, None).unwrap();
        assert!(app.state().dialogs.is_empty());
        let lib = app.session().unwrap().library();
        assert!(ids.iter().all(|id| lib.info(*id).edits.is_empty()));
        app.run("edit.undo", &json!({})).unwrap();
        let lib = app.session().unwrap().library();
        assert!(!lib.info(ids[0]).edits.is_empty() && !lib.info(ids[1]).edits.is_empty(), "one step brings both back");
        for bad in [json!({ "ids": 1 }), json!({ "id": ids[0] }), json!({ "x": 1 }), json!([])] {
            assert!(matches!((o.open)(&mut app, &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!((o.open)(&mut app, &json!({ "ids": [9999] })), Err(CmdError::NotFound(_))));
        // Opened empty (by a script), Remove Edits removes nothing and the question stays.
        dialogs::open(&mut app, dialog()).unwrap();
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))));
        assert_eq!(app.state().dialogs.len(), 1);
    }
}
