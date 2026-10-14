//! The first-time move question (`organizing.md` §9): asked through the host before
//! `photo.moveTo` runs (a row of [`super::confirm::ASKS`]), "Move these files on disk? The files
//! will be moved, not copied." with [Move] [Cancel] and "Don't ask again"
//! (`check:dialog.dontAsk`), whose state is the view state's `moveFiles.dontAsk`. Move (the
//! default) runs `photo.moveTo` with `"confirm": true`, and with `"dontAsk": true` when the box is
//! ticked, which turns the engine's "Ask before moving" off.
//!
//! It asks once `photo.moveTo` is registered (M9.13) and the call does not already send
//! `dontAsk: true`; M9.19 reads the engine's setting here.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::{self, ButtonKind};
use egui::{Label, RichText, Ui};
use serde_json::Value;

use super::{Button, Dialog, Look, confirm};
use crate::app::App;
use crate::state::MoveQuestion;

/// The dialog's title, its default button and its question.
pub const TITLE: &str = "Move Files";
pub const MOVE: &str = "Move";
pub const TEXT: &str = "Move these files on disk? The files will be moved, not copied.";
/// The checkbox's label and automation id (`control-protocol.md` §7).
pub const DONT_ASK: &str = "Don't ask again";
pub const DONT_ASK_ID: &str = "check:dialog.dontAsk";

/// Whether `photo.moveTo` with `params` asks first: once the command exists, unless `dontAsk` is
/// sent.
pub fn asks(app: &App, params: &Value) -> bool {
    confirm::registered(app, "photo.moveTo") && params.get("dontAsk") != Some(&Value::Bool(true))
}

/// Called when the question opens: the photos (named, else the tray's) and the folder, the box
/// unticked. A call without a folder has no draft (the command refuses it).
pub fn opened(app: &mut App, params: &Value) {
    let named: Option<Vec<PhotoId>> = params.get("ids").and_then(|v| serde_json::from_value(v.clone()).ok());
    let ids = named.unwrap_or_else(|| crate::tray::photos(app.state()));
    let folder = params.get("folder").and_then(Value::as_u64).map(FolderId::new);
    app.state_mut().move_files = folder.map(|folder| MoveQuestion { ids, folder, dont_ask: false });
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: confirm::WIDTH, buttons: vec![Button::cancel(), Button::new("move", MOVE, ButtonKind::Default)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.add(Label::new(RichText::new(TEXT).font(fonts::regular(t.body)).color(t.text)).wrap());
    ui.add_space(10.0);
    let mut ticked = app.state().move_files.as_ref().is_some_and(|q| q.dont_ask);
    if widgets::checkbox(ui, &t, DONT_ASK_ID, DONT_ASK, &mut ticked).changed()
        && let Some(q) = app.state_mut().move_files.as_mut()
    {
        q.dont_ask = ticked;
    }
}

/// Move: runs `photo.moveTo` with `"confirm": true` (and `"dontAsk": true` when the box is ticked).
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let dont_ask = app.state().move_files.as_ref().is_some_and(|q| q.dont_ask);
    let extra: Vec<(&str, Value)> = if dont_ask { vec![("dontAsk", Value::Bool(true))] } else { Vec::new() };
    let (id, params) = confirm::confirmed_params(d, &extra)?;
    app.run(&id, &params)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::dialogs::{self, DialogKind};

    #[test]
    fn the_move_question_keeps_its_photos_folder_and_checkbox() {
        let mut app = App::new(None);
        assert!(!asks(&app, &json!({ "ids": [1], "folder": 2 })), "no command yet: no question");
        let ask = confirm::of(DialogKind::MoveFiles).unwrap();
        assert_eq!(confirm::prompt(ask), json!({ "title": TITLE, "text": TEXT, "buttons": ["Move", "Cancel"], "dialog": "moveFiles" }));
        dialogs::ask(&mut app, ask, &json!({ "ids": [1, 2], "folder": 7 })).unwrap();
        let want = MoveQuestion { ids: vec![PhotoId::new(1), PhotoId::new(2)], folder: FolderId::new(7), dont_ask: false };
        assert_eq!(app.state().move_files, Some(want));
        assert_eq!(dialogs::inspect(&app)["default"], "button:dialog.move");
        // The box rides along as dontAsk; the command does not exist yet, so the answer fails and
        // the question stays.
        app.state_mut().move_files.as_mut().unwrap().dont_ask = true;
        let (id, params) = confirm::confirmed_params(dialogs::top(&app).unwrap(), &[("dontAsk", json!(true))]).unwrap();
        assert_eq!((id.as_str(), params), ("photo.moveTo", json!({ "ids": [1, 2], "folder": 7, "confirm": true, "dontAsk": true })));
        assert!(dialogs::answer(&mut app, None).is_err());
        assert_eq!(app.state().dialogs.len(), 1);
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().move_files, None);
        // Without a folder there is no draft.
        dialogs::ask(&mut app, ask, &json!({ "ids": [1] })).unwrap();
        assert_eq!(app.state().move_files, None);
    }
}
