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
//! - **The photo viewer's Delete** ([`ask_viewer`]) asks the photo question for its one file,
//!   with `viewer.delete {path}` as the command: Cancel changes nothing, not even the library.
//!
//! Move to Trash (the default) runs the command with `"confirm": true`; Cancel deletes nothing.
//! Each asks once its command is registered (M9.15), unless Options → "Delete from disk without
//! asking" (`"organize"` → `deleteWithoutAsking`) is on: then the command runs at once, as the
//! engine allows. Deletes always go to the OS trash, never permanently (`security.md` §5.7): a
//! photo or folder on a drive without a trash stays where it is, and the operation's notice gives
//! the engine's explanation (`dalica_engine::cmd::delete::no_trash`, pointing to the file manager)
//! as the reason that item failed. Undo brings the items back from the trash where the system
//! allows it; on macOS Edit ▸ Undo is greyed with the engine's reason.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use egui::Ui;
use serde_json::Value;

use super::{Dialog, DialogKind, Look, confirm};
use crate::app::App;
use crate::state::{DeleteFolder, DeletePhotos};
use crate::{tl, tln};

/// The photo question's title, and its question when the number is not known.
pub const PHOTOS_TITLE: &str = "Delete";
pub const PHOTOS_TEXT: &str = "Move the selected pictures to the trash?";
/// The folder question's title, and its question when the folder is not known.
pub const FOLDER_TITLE: &str = "Delete Folder";
pub const FOLDER_TEXT: &str = "Move this folder and everything in it (including non-picture files) to the trash?";
/// The default button of both.
pub const MOVE_TO_TRASH: &str = "Move to Trash";

/// The photos `params` names: its `ids`, else the tray's (what the command acts on without them).
/// The photo viewer's file (`viewer.delete {path}`) is the library's photo at that path when the
/// library holds it, else none.
fn photos(app: &App, params: &Value) -> Vec<PhotoId> {
    if let Some(path) = params.get("path").and_then(Value::as_str).map(std::path::Path::new) {
        let library = app.session().map(|s| s.library());
        let own = library
            .zip(path.parent())
            .zip(path.file_name())
            .and_then(|((l, folder), name)| l.folder_by_path(folder).and_then(|f| l.photo_by_name(f, &name.to_string_lossy())).map(|p| p.id));
        return own.into_iter().collect();
    }
    let named: Option<Vec<PhotoId>> = params.get("ids").and_then(|v| serde_json::from_value(v.clone()).ok());
    named.unwrap_or_else(|| crate::tray::photos(app.state()))
}

/// Asks the delete question for the photo viewer's file at `path` (`viewing.md` §3.2): "Move the
/// selected picture to the trash?", whose Move to Trash runs `viewer.delete {path, confirm: true}`
/// (the engine adds the folder of a photo outside the library then, never before); with "Delete
/// from disk without asking" on, runs it at once. Replies `{dialog: "deletePhotos"}` when it asks.
pub fn ask_viewer(app: &mut App, path: &std::path::Path) -> Result<Value, CmdError> {
    let params = serde_json::json!({ "path": path });
    if without_asking(app) {
        return app.run("viewer.delete", &serde_json::json!({ "path": path, "confirm": true }));
    }
    let dialog = Dialog {
        command: Some(super::DialogCommand { id: "viewer.delete".into(), params: params.clone() }),
        ..Dialog::new(DialogKind::DeletePhotos)
    };
    let reply = super::open(app, dialog)?;
    opened(app, DialogKind::DeletePhotos, &params);
    Ok(reply)
}

/// The library folder `params.folder` names, if it is one.
fn folder(app: &App, params: &Value) -> Option<(FolderId, String)> {
    let id = FolderId::new(params.get("folder")?.as_u64()?);
    let f = app.session()?.library().folder(id)?;
    let name = f.path.file_name().map_or_else(|| f.path.display().to_string(), |n| n.to_string_lossy().into_owned());
    Some((id, name))
}

/// Whether Options → "Delete from disk without asking" is on: the questions are skipped.
pub fn without_asking(app: &App) -> bool {
    app.session().is_some_and(|s| s.settings().organize().delete_without_asking)
}

/// Whether `photo.delete` asks first: once the command exists, unless "Delete from disk without
/// asking" is on.
pub fn photos_ask(app: &App, _params: &Value) -> bool {
    confirm::registered(app, "photo.delete") && !without_asking(app)
}

/// Whether `folder.delete` asks first: once the command exists, for a folder of the library (a
/// bad one is the command's to refuse), unless "Delete from disk without asking" is on.
pub fn folder_asks(app: &App, params: &Value) -> bool {
    confirm::registered(app, "folder.delete") && folder(app, params).is_some() && !without_asking(app)
}

/// "Move the selected picture to the trash?" for one photo, "Move the %d selected pictures to the
/// trash?" for several; and the folder question, naming the folder: the engine's own texts, the
/// ones its refusal without `"confirm"` gives (the dialog says the same in the language in effect).
pub use dalica_engine::cmd::delete::{folder_question, photos_question};

pub fn photos_text(app: &App, params: &Value) -> Option<String> {
    // The photo viewer's file is one picture, whether the library holds it or not.
    let n = if params.get("path").is_some() { 1 } else { photos(app, params).len() };
    Some(tln!(n, "Move the selected picture to the trash?", "Move the {n} selected pictures to the trash?"))
}

pub fn folder_text(app: &App, params: &Value) -> Option<String> {
    folder(app, params)
        .map(|(_, name)| tl!("Move the folder \"{name}\" and everything in it (including non-picture files) to the trash?", name = name))
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
