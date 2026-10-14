//! The Revert question (`data-model.md` §6, File ▸ Revert…): asked through the host before
//! `photo.revert` runs (a row of [`super::confirm::ASKS`]), with [Revert] [Undo Save] [Cancel].
//! The photos it would revert are the view state's `revert.photos`. Revert runs `photo.revert`
//! with `"confirm": true`; Undo Save runs it with `"action": "undoSave"` too. Cancel is the safe
//! choice: there is no default button, so Enter reverts nothing.
//!
//! A stub until M8.19: the question, and the buttons answered.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, Look, confirm};
use crate::app::App;
use crate::state::RevertQuestion;

/// The dialog's title and its Revert button.
pub const TITLE: &str = "Revert";
/// The Undo Save button.
pub const UNDO_SAVE: &str = "Undo Save";
/// The question (`data-model.md` §6).
pub const TEXT: &str = "Revert to the original? Your edits will be lost. To undo only the last save and keep the edits, choose Undo Save.";

/// Whether `photo.revert` with `params` asks first: always (run with `"confirm": true` to go ahead).
pub fn asks(_app: &App, _params: &Value) -> bool {
    true
}

/// Called when the question opens: the photos named, else those the tray shows (what the command
/// acts on without `ids`).
pub fn opened(app: &mut App, params: &Value) {
    let named: Option<Vec<PhotoId>> = params.get("ids").and_then(|v| serde_json::from_value(v.clone()).ok());
    let photos = named.unwrap_or_else(|| crate::tray::photos(app.state()));
    app.state_mut().revert = Some(RevertQuestion { photos });
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: TITLE.into(),
        width: confirm::WIDTH,
        buttons: vec![Button::new("revert", TITLE, ButtonKind::Other), Button::new("undoSave", UNDO_SAVE, ButtonKind::Other), Button::cancel()],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.label(RichText::new(TEXT).font(fonts::regular(t.body)).color(t.text));
}

/// Revert, or Undo Save: runs `photo.revert` with `"confirm": true` and the action chosen.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let extra: Vec<(&str, Value)> = match action {
        "undoSave" => vec![("action", json!("undoSave"))],
        _ => Vec::new(),
    };
    let (id, params) = confirm::confirmed_params(d, &extra)?;
    app.run(&id, &params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::{self, DialogKind};

    #[test]
    fn photo_revert_asks_with_three_buttons_and_no_default() {
        let mut app = App::new(None);
        assert_eq!(app.run("photo.revert", &json!({ "ids": [3, 4] })).unwrap(), json!({ "dialog": "revert" }));
        assert_eq!(dialogs::top(&app).map(|d| d.kind), Some(DialogKind::Revert));
        assert_eq!(app.state().revert, Some(RevertQuestion { photos: vec![PhotoId::new(3), PhotoId::new(4)] }));
        let inspect = dialogs::inspect(&app);
        assert_eq!(inspect["buttons"], json!(["button:dialog.revert", "button:dialog.undoSave", "button:dialog.cancel"]));
        assert_eq!(inspect["default"], Value::Null, "Enter reverts nothing");
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))));
        let ask = confirm::of(DialogKind::Revert).unwrap();
        assert_eq!(confirm::prompt(ask)["buttons"], json!(["Revert", "Undo Save", "Cancel"]));
        // Cancel closes it and clears its draft.
        dialogs::cancel(&mut app).unwrap();
        assert_eq!((app.state().dialogs.len(), app.state().revert.clone()), (0, None));
        // Without ids it names the tray's photos (none here).
        app.run("photo.revert", &json!({})).unwrap();
        assert_eq!(app.state().revert, Some(RevertQuestion { photos: vec![] }));
    }
}
