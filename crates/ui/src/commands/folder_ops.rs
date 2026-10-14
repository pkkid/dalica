//! Folder operations (`organizing.md` §9): Folder ▸ Move… opens the folder picker for where the
//! current folder goes ([`MOVE`], the opener of `folder.move`, a [`super::Opener`]); the folder
//! chosen runs `folder.move` with it. Over automation the picker is the in-app stub
//! (`ui.inspect` → `filePicker {command: "folder.move", kind: "folder"}`, answered with
//! `ui.filePicker {path}`), and `engine.execute` runs the engine command with its input. Folder ▸
//! Delete… asks through the host (`crate::dialogs::delete`). M9.20 turns the folder chosen into the
//! library folder the engine's `folder.move {folder, to}` takes, and keeps the picker to library
//! folders.

use dalica_core::FolderId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Opener, parse_object};
use crate::app::App;
use crate::picker::FolderRequest;

/// Folder ▸ Move…: opens the folder picker for the current folder's new place.
pub const MOVE: Opener = Opener { id: "folder.move", dialog: crate::menus::FILE_PICKER, keys: &[], open, check };

/// The folder picker's title.
pub const TITLE: &str = "Move Folder";
/// Why there is no folder to move.
pub const NO_FOLDER: &str = "Select a folder first.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    folder: Option<FolderId>,
}

/// The folder moved: the one named, else the current folder (`crate::menus::current_folder`); it
/// must be the library's.
fn moved(app: &App, params: &Value) -> Result<FolderId, CmdError> {
    let p: Move = parse_object(params)?;
    let folder = p.folder.or_else(|| crate::menus::current_folder(app)).ok_or_else(|| CmdError::Disabled(NO_FOLDER.into()))?;
    if !app.session().is_some_and(|s| s.library().folder(folder).is_some()) {
        return Err(CmdError::NotFound(format!("no folder {folder}")));
    }
    Ok(folder)
}

fn check(app: &App, params: &Value) -> Result<(), String> {
    moved(app, params).map_err(|e| e.to_string())?;
    app.can_pick()
}

/// Opens the folder picker, starting in the folder's parent. Replies `{dialog: "filePicker"}`.
fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let folder = moved(app, params)?;
    let start = app.session().and_then(|s| s.library().folder(folder)).and_then(|f| f.path.parent().map(std::path::Path::to_path_buf));
    let request = FolderRequest { title: TITLE, folder: start, command: "folder.move", params: json!({ "folder": folder }), output: false };
    app.pick_folder_for(request).map_err(CmdError::Disabled)?;
    Ok(json!({ "dialog": crate::menus::FILE_PICKER }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::picker::ScriptedPicker;

    #[test]
    fn move_folder_opens_the_folder_picker_for_the_current_folder() {
        let (_dir, s, ids) = session_with_photos(1);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let mut app = App::with_picker(Some(s), Arc::new(ScriptedPicker::new()));
        assert_eq!(check(&app, &json!({})), Err(NO_FOLDER.into()));
        app.state_mut().folder = Some(folder);
        assert_eq!(check(&app, &json!({})), Ok(()));
        assert_eq!(open(&mut app, &json!({})).unwrap(), json!({ "dialog": "filePicker" }));
        let r = app.folder_request().cloned().unwrap();
        assert_eq!((r.title, r.command, r.params), (TITLE, "folder.move", json!({ "folder": folder })));
        assert!(check(&app, &json!({})).is_err(), "one file dialog at a time");
        // Without a picker it says why; bad params and unknown folders are refused.
        let (_dir2, s2, _) = session_with_photos(1);
        let mut bare = App::new(Some(s2));
        assert!(matches!(open(&mut bare, &json!({ "folder": folder })), Err(CmdError::Disabled(_)) | Err(CmdError::NotFound(_))));
        assert!(matches!(open(&mut bare, &json!({ "folder": "x" })), Err(CmdError::BadParams(_))));
        assert!(matches!(open(&mut bare, &json!({ "folder": 9999 })), Err(CmdError::NotFound(_))));
    }
}
