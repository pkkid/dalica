//! The delete questions (`organizing.md` §9), asked through the host (rows of
//! [`super::confirm::ASKS`]):
//!
//! - **Delete** (`photo.delete`: File ▸ Delete from Disk…, the photo menu, Delete in a folder,
//!   Ctrl+Delete anywhere): "Move the selected picture to the trash?" / "Move the %d selected
//!   pictures to the trash?" with [Move to Trash] [Cancel]; its draft is the view state's
//!   `deletePhotos {ids}`.
//! - **Delete Folder** (`folder.delete`: Folder ▸ Delete…): "Move the folder "%s" and everything in
//!   it (including non-picture files) to the trash?" with [Move to Trash] [Cancel]; its draft is
//!   `deleteFolder {folder}`.
//!
//! Move to Trash (the default) runs the command with `"confirm": true`; deletes always go to the OS
//! trash. Each asks once its command is registered (M9.15); the engine's "Delete without asking"
//! and the explanation shown when a volume has no trash join with M9.20.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use egui::Ui;
use serde_json::Value;

use super::{Dialog, DialogKind, Look, confirm};
use crate::app::App;
use crate::state::{DeleteFolder, DeletePhotos};

/// The photo question's title, and its question when the number is not known.
pub const PHOTOS_TITLE: &str = "Delete";
pub const PHOTOS_TEXT: &str = "Move the selected pictures to the trash?";
/// The folder question's title, and its question when the folder is not known.
pub const FOLDER_TITLE: &str = "Delete Folder";
pub const FOLDER_TEXT: &str = "Move this folder and everything in it (including non-picture files) to the trash?";
/// The default button of both.
pub const MOVE_TO_TRASH: &str = "Move to Trash";

/// The photos `params` names: its `ids`, else the tray's (what the command acts on without them).
fn photos(app: &App, params: &Value) -> Vec<PhotoId> {
    let named: Option<Vec<PhotoId>> = params.get("ids").and_then(|v| serde_json::from_value(v.clone()).ok());
    named.unwrap_or_else(|| crate::tray::photos(app.state()))
}

/// The library folder `params.folder` names, if it is one.
fn folder(app: &App, params: &Value) -> Option<(FolderId, String)> {
    let id = FolderId::new(params.get("folder")?.as_u64()?);
    let f = app.session()?.library().folder(id)?;
    let name = f.path.file_name().map_or_else(|| f.path.display().to_string(), |n| n.to_string_lossy().into_owned());
    Some((id, name))
}

/// Whether `photo.delete` asks first: once the command exists.
pub fn photos_ask(app: &App, _params: &Value) -> bool {
    confirm::registered(app, "photo.delete")
}

/// Whether `folder.delete` asks first: once the command exists, for a folder of the library (a
/// bad one is the command's to refuse).
pub fn folder_asks(app: &App, params: &Value) -> bool {
    confirm::registered(app, "folder.delete") && folder(app, params).is_some()
}

/// "Move the selected picture to the trash?" for one photo, "Move the %d selected pictures to the
/// trash?" for several.
pub fn photos_question(n: usize) -> String {
    match n {
        1 => "Move the selected picture to the trash?".into(),
        n => format!("Move the {n} selected pictures to the trash?"),
    }
}

/// The folder question, naming the folder.
pub fn folder_question(name: &str) -> String {
    format!("Move the folder \"{name}\" and everything in it (including non-picture files) to the trash?")
}

pub fn photos_text(app: &App, params: &Value) -> Option<String> {
    Some(photos_question(photos(app, params).len()))
}

pub fn folder_text(app: &App, params: &Value) -> Option<String> {
    folder(app, params).map(|(_, name)| folder_question(&name))
}

/// Called when a question opens: its draft, the photos or the folder it asks about.
pub fn opened(app: &mut App, kind: DialogKind, params: &Value) {
    match kind {
        DialogKind::DeletePhotos => {
            let ids = photos(app, params);
            app.state_mut().delete_photos = Some(DeletePhotos { ids });
        }
        DialogKind::DeleteFolder => {
            let folder = folder(app, params).map(|(id, _)| DeleteFolder { folder: id });
            app.state_mut().delete_folder = folder;
        }
        _ => {}
    }
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    confirm::look(d)
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    confirm::body(app, ui, d);
}

/// Move to Trash: runs the command with `"confirm": true`.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    confirm::act(app, d, action)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::dialogs;

    #[test]
    fn the_questions_read_as_specified_and_keep_their_drafts() {
        assert_eq!(photos_question(1), "Move the selected picture to the trash?");
        assert_eq!(photos_question(4), "Move the 4 selected pictures to the trash?");
        assert_eq!(folder_question("Lisbon"), "Move the folder \"Lisbon\" and everything in it (including non-picture files) to the trash?");
        let (_dir, s, ids) = session_with_photos(2);
        let folder_id = s.library().photo(ids[0]).unwrap().folder;
        let mut app = App::new(Some(s));
        // Registered (M9.15): both ask; a folder outside the library is the command's to refuse.
        assert!(photos_ask(&app, &json!({ "ids": [ids[0]] })));
        assert!(folder_asks(&app, &json!({ "folder": folder_id })));
        assert!(!folder_asks(&app, &json!({ "folder": 9999 })));
        // The dialogs as the host opens them.
        let ask = confirm::of(DialogKind::DeletePhotos).unwrap();
        assert_eq!(confirm::prompt(ask)["buttons"], json!(["Move to Trash", "Cancel"]));
        dialogs::ask(&mut app, ask, &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert_eq!(app.state().delete_photos, Some(DeletePhotos { ids: ids.clone() }));
        assert_eq!(confirm::question(&app, dialogs::top(&app).unwrap()), photos_question(2));
        assert_eq!(dialogs::inspect(&app)["default"], "button:dialog.moveToTrash");
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().delete_photos, None, "closing clears the draft");
        let ask = confirm::of(DialogKind::DeleteFolder).unwrap();
        dialogs::ask(&mut app, ask, &json!({ "folder": folder_id })).unwrap();
        assert_eq!(app.state().delete_folder, Some(DeleteFolder { folder: folder_id }));
        assert!(confirm::question(&app, dialogs::top(&app).unwrap()).contains("everything in it (including non-picture files)"));
        // Cancel keeps the folder (Move to Trash runs `folder.delete` confirmed: `crates/engine/tests/delete.rs`).
        assert_eq!(app.state().dialogs.len(), 1);
        dialogs::cancel(&mut app).unwrap();
        assert_eq!(app.state().delete_folder, None);
    }
}
