//! The new-folder dialog (`organizing.md` §9): File ▸ Move to New Folder… and the photo menu's Move
//! to New Folder… (`photo.moveToNewFolder` without a name: the selection) and Split Folder Here…
//! (`photo.splitFolder`: the photo and every photo after it in its folder) open Folder Properties
//! for a folder that does not exist yet, made beside the photos' folder. Its draft is the view
//! state's `newFolder` ([`NewFolderDraft`]): Name ("New Folder"), Date with "Use the date of the
//! oldest photo" (on), Place and Description. OK (enabled while the name is not blank) runs
//! `photo.moveToNewFolder {ids, name, date?, useOldest, place, description}` ([`run_params`]).
//!
//! A stub until M9.19: the body shows the draft as text.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::NewFolderDraft;

/// The dialog's title and width (points).
pub const TITLE: &str = "Folder Properties";
pub const WIDTH: f32 = 440.0;
/// "Use the date of the oldest photo".
pub const USE_OLDEST: &str = "Use the date of the oldest photo";

/// Opens the dialog with `draft`. Replies `{dialog: "newFolder"}`.
pub fn open(app: &mut App, draft: NewFolderDraft) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::NewFolder))?;
    app.state_mut().new_folder = Some(draft.sanitized());
    Ok(reply)
}

/// Why OK cannot run with `draft`, if it cannot: a blank name, no photos.
pub fn ready(draft: &NewFolderDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        Err("Name the folder.".into())
    } else if draft.ids.is_empty() {
        Err("Select some photos first.".into())
    } else {
        Ok(())
    }
}

/// `photo.moveToNewFolder`'s parameters for `draft`; the date only when typed and not taken from
/// the oldest photo.
pub fn run_params(draft: &NewFolderDraft) -> Value {
    let mut p = Map::new();
    p.insert("ids".into(), json!(draft.ids));
    p.insert("name".into(), json!(draft.name.trim()));
    if !draft.use_oldest && !draft.date.trim().is_empty() {
        p.insert("date".into(), json!(draft.date.trim()));
    }
    p.insert("useOldest".into(), json!(draft.use_oldest));
    p.insert("place".into(), json!(draft.place));
    p.insert("description".into(), json!(draft.description));
    Value::Object(p)
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let ok = app.state().new_folder.as_ref().is_some_and(|d| ready(d).is_ok());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("ok", "OK", ButtonKind::Default).enabled(ok)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let Some(d) = app.state().new_folder.clone() else { return };
    let tick = if d.use_oldest { "[x]" } else { "[ ]" };
    let lines = [
        format!("Name: {}", d.name),
        format!("Date: {}", d.date),
        format!("{tick} {USE_OLDEST}"),
        format!("Place: {}", d.place),
        format!("Description: {}", d.description),
        format!("{} photos", d.ids.len()),
    ];
    super::text_lines(app, ui, &lines);
}

/// OK: moves the photos into the new folder.
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().new_folder.clone().ok_or_else(|| CmdError::Internal("the new-folder dialog has no draft".into()))?;
    ready(&draft).map_err(CmdError::Disabled)?;
    app.run("photo.moveToNewFolder", &run_params(&draft))
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;
    use crate::dialogs;

    #[test]
    fn ok_moves_the_photos_into_a_new_folder_with_its_information() {
        let d = NewFolderDraft { ids: vec![PhotoId::new(3)], ..NewFolderDraft::default() };
        assert_eq!(run_params(&d), json!({ "ids": [3], "name": "New Folder", "useOldest": true, "place": "", "description": "" }));
        let dated = NewFolderDraft { date: "2026-06-01".into(), use_oldest: false, ..d.clone() };
        assert_eq!(run_params(&dated)["date"], "2026-06-01");
        assert!(ready(&NewFolderDraft::default()).is_err(), "no photos");
        let mut app = App::new(None);
        open(&mut app, d.clone()).unwrap();
        assert_eq!(app.state().new_folder, Some(d));
        assert_eq!(dialogs::inspect(&app)["title"], TITLE);
        // No library: OK fails and the dialog stays.
        assert!(dialogs::answer(&mut app, None).is_err());
        assert_eq!(app.state().dialogs.len(), 1);
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().new_folder, None);
    }
}
