//! The copy question (`organizing.md` §9): copying never asks in the engine, so the UI asks "Copy
//! %d files to %s?" with [Copy] [Cancel] before it runs `photo.copyTo` (Paste after Copy, a drag
//! with Ctrl held). [`open`] asks it; its draft is the view state's `copyFiles {ids, folder}`.
//! Copy (the default) runs `photo.copyTo {ids, folder}`.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{Label, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look, confirm};
use crate::app::App;
use crate::state::{CopyQuestion, FileOp};
use crate::{tl, tln};

/// The dialog's title and its default button.
pub const TITLE: &str = "Copy Files";
pub const COPY: &str = "Copy";

/// Asks whether to copy `ids` into `folder`. Replies `{dialog: "copyFiles"}`.
pub fn open(app: &mut App, ids: Vec<PhotoId>, folder: FolderId) -> Result<Value, CmdError> {
    let reply = super::open(app, Dialog::new(DialogKind::CopyFiles))?;
    app.state_mut().copy_files = Some(CopyQuestion { ids, folder });
    Ok(reply)
}

/// "Copy %d files to %s?" (one file: "Copy 1 file to %s?").
pub fn question(n: usize, folder: &str) -> String {
    tln!(n, "Copy {n} file to {folder}?", "Copy {n} files to {folder}?", folder = folder)
}

/// The folder's name as the question writes it.
fn folder_name(app: &App, folder: FolderId) -> String {
    let f = app.session().and_then(|s| s.library().folder(folder));
    f.map_or_else(
        || tl!("folder {folder}", folder = folder),
        |f| f.path.file_name().map_or_else(|| f.path.display().to_string(), |n| n.to_string_lossy().into_owned()),
    )
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: tl!("Copy Files"),
        width: confirm::WIDTH,
        buttons: vec![Button::cancel(), Button::new("copy", tl!(ctx = "verb", "Copy"), ButtonKind::Default)],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    let text = app.state().copy_files.as_ref().map_or_else(String::new, |q| question(q.ids.len(), &folder_name(app, q.folder)));
    ui.add(Label::new(RichText::new(text).font(fonts::regular(t.body)).color(t.text)).wrap());
}

/// Copy: runs `photo.copyTo {ids, folder}`, the collision question first when names clash.
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let q = app.state().copy_files.clone().ok_or_else(|| CmdError::Internal("the copy question has no draft".into()))?;
    super::collision::run_or_ask(app, FileOp::Copy, json!({ "ids": q.ids, "folder": q.folder }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs;

    #[test]
    fn the_copy_question_names_the_count_and_the_folder() {
        assert_eq!(question(1, "Lisbon"), "Copy 1 file to Lisbon?");
        assert_eq!(question(3, "Lisbon"), "Copy 3 files to Lisbon?");
        let mut app = App::new(None);
        assert_eq!(open(&mut app, vec![PhotoId::new(4)], FolderId::new(2)).unwrap(), json!({ "dialog": "copyFiles" }));
        assert_eq!(app.state().copy_files, Some(CopyQuestion { ids: vec![PhotoId::new(4)], folder: FolderId::new(2) }));
        assert_eq!(dialogs::inspect(&app)["buttons"], json!(["button:dialog.cancel", "button:dialog.copy"]));
        assert!(open(&mut app, vec![], FolderId::new(2)).is_err(), "one at a time");
        // Without a library the copy cannot run: the question stays with the reason.
        assert!(dialogs::answer(&mut app, None).is_err());
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().copy_files, None);
    }
}
