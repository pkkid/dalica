//! Rename Files (`organizing.md` §9): File ▸ Rename… and F2 (`photo.rename` without a name: the
//! selection) open it. Its draft is the view state's `renameFiles` ([`RenameDraft`]): "%d files
//! selected. Enter a new name:", the name, "Add the date taken to the name" and "Add the image size
//! to the name". Rename (the default, enabled while the name is not blank) runs `photo.rename
//! {ids, name, addDate, addSize}`; several files get `-001`, `-002` … in display order (the
//! engine's, M9.14).
//!
//! A stub until M9.20: the body shows the draft as text (the example line follows the typing
//! then).

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::RenameDraft;

/// The dialog's title, its default button and its width (points).
pub const TITLE: &str = "Rename Files";
pub const RENAME: &str = "Rename";
pub const WIDTH: f32 = 420.0;
/// The checkboxes' labels.
pub const ADD_DATE: &str = "Add the date taken to the name";
pub const ADD_SIZE: &str = "Add the image size to the name";

/// Opens the dialog with `draft`. Replies `{dialog: "renameFiles"}`.
pub fn open(app: &mut App, draft: RenameDraft) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::RenameFiles))?;
    app.state_mut().rename_files = Some(draft.sanitized());
    Ok(reply)
}

/// "%d files selected. Enter a new name:" (one file: "1 file selected.").
pub fn heading(n: usize) -> String {
    let noun = if n == 1 { "file" } else { "files" };
    format!("{n} {noun} selected. Enter a new name:")
}

/// Why Rename cannot run with `draft`, if it cannot: a blank name, no photos.
pub fn ready(draft: &RenameDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        Err("Enter a new name.".into())
    } else if draft.ids.is_empty() {
        Err("Select some photos first.".into())
    } else {
        Ok(())
    }
}

/// `photo.rename`'s parameters for `draft`.
pub fn run_params(draft: &RenameDraft) -> Value {
    json!({ "ids": draft.ids, "name": draft.name.trim(), "addDate": draft.add_date, "addSize": draft.add_size })
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let ok = app.state().rename_files.as_ref().is_some_and(|d| ready(d).is_ok());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("rename", RENAME, ButtonKind::Default).enabled(ok)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let Some(d) = app.state().rename_files.clone() else { return };
    let tick = |on: bool| if on { "[x]" } else { "[ ]" };
    let lines =
        [heading(d.ids.len()), format!("Name: {}", d.name), format!("{} {ADD_DATE}", tick(d.add_date)), format!("{} {ADD_SIZE}", tick(d.add_size))];
    super::text_lines(app, ui, &lines);
}

/// Rename: renames the photos.
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().rename_files.clone().ok_or_else(|| CmdError::Internal("Rename Files has no draft".into()))?;
    ready(&draft).map_err(CmdError::Disabled)?;
    app.run("photo.rename", &run_params(&draft))
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;
    use crate::dialogs;

    #[test]
    fn rename_runs_photo_rename_with_the_draft() {
        assert_eq!(heading(1), "1 file selected. Enter a new name:");
        assert_eq!(heading(3), "3 files selected. Enter a new name:");
        let d = RenameDraft { ids: vec![PhotoId::new(1), PhotoId::new(2)], name: " Lisbon ".into(), add_date: true, add_size: false };
        assert_eq!(run_params(&d), json!({ "ids": [1, 2], "name": "Lisbon", "addDate": true, "addSize": false }));
        let mut app = App::new(None);
        open(&mut app, RenameDraft { ids: vec![PhotoId::new(1)], ..RenameDraft::default() }).unwrap();
        let v = dialogs::inspect(&app);
        assert_eq!((v["kind"].as_str(), v["default"].as_str()), (Some("renameFiles"), Some("button:dialog.rename")));
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))), "Rename waits for a name");
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().rename_files, None);
    }
}
