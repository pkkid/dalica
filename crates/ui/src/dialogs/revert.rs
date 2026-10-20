//! The Revert question (`data-model.md` §6, File ▸ Revert…): asked through the host before
//! `photo.revert` runs (a row of [`super::confirm::ASKS`]), with [Revert] [Undo Save] [Cancel].
//! The photos it would revert are the view state's `revert.photos`. Revert runs `photo.revert`
//! with `"confirm": true`; Undo Save runs it with `"action": "undoSave"` too. Cancel is the safe
//! choice: there is no default button, so Enter reverts nothing.
//!
//! It asks only when there is something to revert (the command's `enabled` accepts the photos
//! named: one of them saved in place, its folder online); otherwise the reason goes to the status
//! line without a question.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, Look, confirm};
use crate::app::App;
use crate::state::RevertQuestion;

/// The dialog's title and its Revert button.
pub const TITLE: &str = "Revert";
/// The Undo Save button.
pub const UNDO_SAVE: &str = "Undo Save";
/// The question (`data-model.md` §6).
pub const TEXT: &str = dalica_engine::cmd::save::REVERT_QUESTION;

/// Whether `photo.revert` with `params` asks first: whenever it would revert something (run with
/// `"confirm": true` to go ahead).
pub fn asks(app: &App, params: &Value) -> bool {
    let Some(s) = app.session() else { return false };
    s.spec(COMMAND).is_some_and(|spec| (spec.enabled)(s, params).is_ok())
}

/// The command it asks about.
const COMMAND: &str = "photo.revert";

/// Called when the question opens: the photos named, else those the tray shows (what the command
/// acts on without `ids`).
pub fn opened(app: &mut App, params: &Value) {
    let named: Option<Vec<PhotoId>> = params.get("ids").and_then(|v| serde_json::from_value(v.clone()).ok());
    let photos = named.unwrap_or_else(|| crate::tray::photos(app.state()));
    app.state_mut().revert = Some(RevertQuestion { photos });
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: confirm::text(TITLE),
        width: confirm::WIDTH,
        buttons: vec![
            Button::new("revert", confirm::text(TITLE), ButtonKind::Other),
            Button::new("undoSave", confirm::text(UNDO_SAVE), ButtonKind::Other),
            Button::cancel(),
        ],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(Label::new(RichText::new(confirm::text(TEXT)).font(fonts::regular(t.body)).color(t.text)).wrap());
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
    use crate::app::tests::session_with_photos;
    use crate::dialogs::{self, DialogKind};

    #[test]
    fn photo_revert_asks_with_three_buttons_and_no_default_only_when_there_is_a_save() {
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        // Never saved: no question, the reason on the status line.
        assert!(matches!(app.run("photo.revert", &json!({ "ids": [ids[0]] })), Err(CmdError::Disabled(_))));
        assert!(app.state().dialogs.is_empty());
        assert!(!app.state().status.is_empty());
        // Without a library nothing asks either.
        assert!(!asks(&App::new(None), &json!({ "ids": [1] })));
        let ask = confirm::of(DialogKind::Revert).unwrap();
        assert_eq!(confirm::prompt(ask)["buttons"], json!(["Revert", "Undo Save", "Cancel"]));
        // The dialog itself, opened as the host opens it: three buttons, no default.
        dialogs::ask(&mut app, ask, &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert_eq!(dialogs::top(&app).map(|d| d.kind), Some(DialogKind::Revert));
        assert_eq!(app.state().revert, Some(RevertQuestion { photos: ids.clone() }));
        let inspect = dialogs::inspect(&app);
        assert_eq!(inspect["buttons"], json!(["button:dialog.revert", "button:dialog.undoSave", "button:dialog.cancel"]));
        assert_eq!(inspect["default"], Value::Null, "Enter reverts nothing");
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))));
        // Its buttons run the command confirmed; with nothing saved it refuses and the question stays.
        assert!(matches!(dialogs::answer(&mut app, Some("revert")), Err(CmdError::Disabled(_))));
        assert!(matches!(dialogs::answer(&mut app, Some("undoSave")), Err(CmdError::Disabled(_))));
        assert_eq!(app.state().dialogs.len(), 1);
        // Cancel closes it and clears its draft.
        dialogs::cancel(&mut app).unwrap();
        assert_eq!((app.state().dialogs.len(), app.state().revert.clone()), (0, None));
        // Without ids it names the tray's photos (none here).
        dialogs::ask(&mut app, ask, &json!({})).unwrap();
        assert_eq!(app.state().revert, Some(RevertQuestion { photos: vec![] }));
    }
}
