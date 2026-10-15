//! Folder operations (`organizing.md` §9): Folder ▸ Move… (and the folder menu's Move Folder…)
//! opens the folder picker for where the current folder goes ([`MOVE`], the opener of
//! `folder.move`, a [`super::Opener`]); the folder chosen runs the engine's `folder.move {folder,
//! path}` (M9.15: one undo step, the folder keeping its id, so its tree row and section follow it).
//! Over automation the picker is the in-app stub (`ui.inspect` → `filePicker {command:
//! "folder.move", kind: "folder"}`, answered with `ui.filePicker {path}`), and `engine.execute`
//! runs the engine command with its input. Folder ▸ Delete… asks through the host
//! (`crate::dialogs::delete`).
//!
//! - **Library folders only.** A native folder dialog cannot be kept to some folders, so the
//!   engine refuses a place outside the library ("Folders move only within the library: choose a
//!   folder Dalica watches."), and the reason shows on the status line; nothing moves.
//! - **Where the picker opens:** the folder Move Folder last moved a folder into
//!   (`"organize"` → `lastMoveFolder`), while it is still in the library and is not the folder
//!   itself or inside it; else the folder's parent. The last place is taken from the session's
//!   finished folder moves (their `FileOpEvent`s) each time Move Folder opens, and saved in the
//!   settings then, so it is remembered across sessions too.

use std::path::{Path, PathBuf};

use dalica_core::FolderId;
use dalica_engine::{CmdError, Direction, FileOpKind, Session};
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

/// The folder the newest folder move of this session went into, if there was one: a plain move's
/// notice names the folder it went into; a merge's (photos moved, kind `Move`) the merged folder,
/// whose parent it is.
pub fn last_move(s: &Session) -> Option<PathBuf> {
    let e = s.file_op_events().filter(|e| e.label == dalica_engine::cmd::folder_ops::LABEL && e.direction == Direction::Do && e.done > 0).last()?;
    match e.kind {
        FileOpKind::MoveFolder => e.destination.clone(),
        _ => e.destination.as_deref().and_then(Path::parent).map(Path::to_path_buf),
    }
}

/// The last place Move Folder moved a folder into: this session's newest folder move, saved as
/// `lastMoveFolder` when it is newer than the setting; else the setting.
pub fn remembered(app: &mut App) -> Option<PathBuf> {
    let s = app.session_mut()?;
    if let Some(last) = last_move(s)
        && s.settings().organize().last_move_folder.as_ref() != Some(&last)
    {
        s.change_settings(|st| {
            let mut o = st.organize();
            o.last_move_folder = Some(last);
            st.set_organize(&o);
        });
    }
    s.settings().organize().last_move_folder
}

/// Where the picker opens for moving the folder at `path`: `last` while it is a library folder or
/// under a watched folder and is neither `path` nor inside it, else `path`'s parent.
pub fn start(s: &Session, path: &Path, last: Option<PathBuf>) -> Option<PathBuf> {
    let in_library = |p: &Path| s.library().folder_by_path(p).is_some() || s.roots().iter().any(|r| p.starts_with(r));
    last.filter(|l| !l.starts_with(path) && in_library(l)).or_else(|| path.parent().map(Path::to_path_buf))
}

/// Opens the folder picker, starting where [`start`] says. Replies `{dialog: "filePicker"}`.
fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let folder = moved(app, params)?;
    app.can_pick().map_err(CmdError::Disabled)?;
    let last = remembered(app);
    let begin = app.session().and_then(|s| s.library().folder(folder).map(|f| f.path.clone()).and_then(|p| start(s, &p, last)));
    let request = FolderRequest { title: TITLE, folder: begin, command: "folder.move", params: json!({ "folder": folder }), output: false };
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
        let path = s.library().folder(folder).unwrap().path.clone();
        let mut app = App::with_picker(Some(s), Arc::new(ScriptedPicker::new()));
        assert_eq!(check(&app, &json!({})), Err(NO_FOLDER.into()));
        app.state_mut().folder = Some(folder);
        assert_eq!(check(&app, &json!({})), Ok(()));
        assert_eq!(open(&mut app, &json!({})).unwrap(), json!({ "dialog": "filePicker" }));
        let r = app.folder_request().cloned().unwrap();
        assert_eq!((r.title, r.command, r.params), (TITLE, "folder.move", json!({ "folder": folder })));
        assert_eq!(r.folder.as_deref(), path.parent(), "nothing moved yet: the folder's parent");
        assert!(check(&app, &json!({})).is_err(), "one file dialog at a time");
        // Without a picker it says why; bad params and unknown folders are refused.
        let (_dir2, s2, _) = session_with_photos(1);
        let mut bare = App::new(Some(s2));
        assert!(matches!(open(&mut bare, &json!({ "folder": folder })), Err(CmdError::Disabled(_)) | Err(CmdError::NotFound(_))));
        assert!(matches!(open(&mut bare, &json!({ "folder": "x" })), Err(CmdError::BadParams(_))));
        assert!(matches!(open(&mut bare, &json!({ "folder": 9999 })), Err(CmdError::NotFound(_))));
    }

    #[test]
    fn the_picker_starts_at_the_last_place_while_it_is_in_the_library() {
        let (_dir, s, ids) = session_with_photos(1);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let path = s.library().folder(folder).unwrap().path.clone();
        let parent = path.parent().map(Path::to_path_buf);
        assert_eq!(last_move(&s), None, "no folder moved in this session");
        assert_eq!(start(&s, &path, None), parent);
        assert_eq!(start(&s, &path, Some(path.clone())), parent, "not the folder itself");
        assert_eq!(start(&s, &path, Some(path.join("Day 2"))), parent, "not inside it");
        let outside = std::env::temp_dir().join("dalica-not-watched-anywhere");
        assert_eq!(start(&s, &path, Some(outside)), parent, "not outside the library");
        let mut app = App::new(Some(s));
        // The setting is read when nothing moved in this session.
        let watched = app.session().unwrap().roots().first().cloned().unwrap();
        app.session_mut().unwrap().change_settings(|st| {
            let mut o = st.organize();
            o.last_move_folder = Some(watched.clone());
            st.set_organize(&o);
        });
        assert_eq!(remembered(&mut app), Some(watched.clone()));
        // Here the watched folder is the folder itself: no place to move it into.
        assert_eq!(start(app.session().unwrap(), &path, Some(watched)), parent);
    }
}
