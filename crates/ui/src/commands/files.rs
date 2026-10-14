//! Moving photos into a new folder (`organizing.md` §9): Move to New Folder… and Split Folder Here…
//! open the new-folder dialog (`crate::dialogs::new_folder`), whose OK runs the engine's
//! `photo.moveToNewFolder {ids, name, …}` (M9.13).
//!
//! - **Move to New Folder…** (File menu, the photo menu): the opener of `photo.moveToNewFolder`
//!   ([`MOVE_TO_NEW_FOLDER`], a [`super::Opener`]): a click or `ui.menu.invoke` opens the dialog
//!   for the selection (photos of one folder), while `engine.execute` runs the engine command with
//!   its input.
//! - `photo.splitFolder {id?}` (the photo menu's Split Folder Here…): the dialog for the photo
//!   named (else the focused or first selected) and every photo after it in its folder, in the
//!   light table's order.
//!
//! The move and copy questions, the collision question and the file notices are
//! `crate::dialogs::{move_confirm, copy_confirm, collision}` and M9.19's.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::Value;

use super::{Check, Opener, UiCommand, parse_object};
use crate::app::App;
use crate::dialogs::new_folder;
use crate::state::{NewFolderDraft, Section};

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "photo.splitFolder",
    label: "Split Folder Here…",
    menu: "",
    keys: &[],
    params: "{\"id\":PhotoId=focused}: open the new-folder dialog for this photo and every photo after it in its folder (replies {dialog: \"newFolder\"}; its OK runs photo.moveToNewFolder)",
    run: split_folder,
}];

pub const CHECKS: &[(&str, Check)] = &[("photo.splitFolder", can_split)];

/// File ▸ Move to New Folder…: opens the new-folder dialog for the selection.
pub const MOVE_TO_NEW_FOLDER: Opener =
    Opener { id: "photo.moveToNewFolder", dialog: "newFolder", keys: &[], open: move_to_new_folder, check: can_move_to_new_folder };

/// Why there is nothing to move.
pub const NOTHING: &str = "Select some photos first.";
/// Why photos of several folders cannot move into one new folder.
pub const SEVERAL_FOLDERS: &str = "Select photos of one folder.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    ids: Option<Vec<PhotoId>>,
}

/// The photos Move to New Folder… moves: those named, else the selection; all of one folder.
fn moved(app: &App, params: &Value) -> Result<Vec<PhotoId>, CmdError> {
    let p: Ids = parse_object(params)?;
    let ids = match p.ids {
        Some(ids) => super::albums::known_photos(app, ids)?,
        None => app.state().selection.ids.clone(),
    };
    if ids.is_empty() {
        return Err(CmdError::Disabled(NOTHING.into()));
    }
    let library = app.session().map(dalica_engine::Session::library);
    let mut folders = ids.iter().filter_map(|id| library.and_then(|l| l.photo(*id)).map(|p| p.folder));
    let first = folders.next();
    if first.is_none() || folders.any(|f| Some(f) != first) {
        return Err(CmdError::Disabled(SEVERAL_FOLDERS.into()));
    }
    Ok(ids)
}

fn can_move_to_new_folder(app: &App, params: &Value) -> Result<(), String> {
    moved(app, params).map(|_| ()).map_err(|e| e.to_string())
}

fn move_to_new_folder(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let ids = moved(app, params)?;
    new_folder::open(app, NewFolderDraft { ids, ..NewFolderDraft::default() })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Split {
    id: Option<PhotoId>,
}

/// The photos Split Folder Here… moves: the photo named (else the focused or first selected) and
/// every photo after it in its folder, as the light table shows them (the library's order of the
/// folder before the light table has drawn it).
fn split(app: &App, params: &Value) -> Result<Vec<PhotoId>, CmdError> {
    let p: Split = parse_object(params)?;
    let sel = &app.state().selection;
    let id = p.id.or(sel.anchor).or_else(|| sel.ids.first().copied()).ok_or_else(|| CmdError::Disabled(super::albums::NO_PHOTO.into()))?;
    let library = app.session().map(dalica_engine::Session::library);
    let folder: FolderId = library.and_then(|l| l.photo(id)).map(|p| p.folder).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let shown = app.panels().and_then(|p| p.sections.get(Section::Folder(folder))).map(|s| s.photos.clone());
    let order = shown.unwrap_or_else(|| library.map(|l| l.photos().filter(|p| p.folder == folder).map(|p| p.id).collect()).unwrap_or_default());
    let at = order.iter().position(|p| *p == id).ok_or_else(|| CmdError::Disabled("That photo is not shown in its folder.".into()))?;
    Ok(order.get(at..).map(<[PhotoId]>::to_vec).unwrap_or_default())
}

fn can_split(app: &App, params: &Value) -> Result<(), String> {
    split(app, params).map(|_| ()).map_err(|e| e.to_string())
}

/// `photo.splitFolder {id?}`: the new-folder dialog for the photo and every photo after it.
fn split_folder(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let ids = split(app, params)?;
    new_folder::open(app, NewFolderDraft { ids, ..NewFolderDraft::default() })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::dialogs;

    #[test]
    fn split_folder_takes_the_photo_and_every_photo_after_it() {
        let (_dir, s, ids) = session_with_photos(4);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("photo.splitFolder", &json!({})), Err(CmdError::Disabled(_))), "nothing selected");
        let order = split(&app, &json!({ "id": ids[0] })).unwrap();
        assert_eq!(order.len(), 4, "the whole folder from its first photo");
        let at = order[2];
        assert_eq!(app.run("photo.splitFolder", &json!({ "id": at })).unwrap(), json!({ "dialog": "newFolder" }));
        assert_eq!(app.state().new_folder.as_ref().map(|d| d.ids.clone()), Some(order[2..].to_vec()));
        dialogs::cancel(&mut app).unwrap();
        // The focused photo without one named.
        app.run("select.set", &json!({ "ids": [order[3]] })).unwrap();
        app.run("photo.splitFolder", &json!({})).unwrap();
        assert_eq!(app.state().new_folder.as_ref().map(|d| d.ids.clone()), Some(vec![order[3]]));
        dialogs::cancel(&mut app).unwrap();
        for bad in [json!({ "id": "x" }), json!({ "ids": [1] }), json!(7)] {
            assert!(matches!(app.run("photo.splitFolder", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(app.run("photo.splitFolder", &json!({ "id": 9999 })), Err(CmdError::NotFound(_))));
        assert!(app.state().dialogs.is_empty());
    }

    #[test]
    fn move_to_new_folder_opens_the_dialog_for_photos_of_one_folder() {
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        assert_eq!(can_move_to_new_folder(&app, &json!({})), Err(NOTHING.into()));
        app.run("select.set", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert_eq!(can_move_to_new_folder(&app, &json!({})), Ok(()));
        assert_eq!((MOVE_TO_NEW_FOLDER.open)(&mut app, &json!({})).unwrap(), json!({ "dialog": "newFolder" }));
        assert_eq!(app.state().new_folder.as_ref().map(|d| (d.ids.clone(), d.name.as_str())), Some((vec![ids[0], ids[1]], "New Folder")));
        dialogs::cancel(&mut app).unwrap();
        assert!(matches!((MOVE_TO_NEW_FOLDER.open)(&mut app, &json!({ "ids": [9999] })), Err(CmdError::NotFound(_))));
        assert!(matches!((MOVE_TO_NEW_FOLDER.open)(&mut app, &json!({ "name": "x" })), Err(CmdError::BadParams(_))));
    }
}
