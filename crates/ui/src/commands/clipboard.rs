//! Edit ▸ Cut, Copy and Paste (`menus-and-shortcuts.md` §1.2): for moving and copying files between
//! folders inside Dalica. Cut and Copy keep the photos in the view state's `clipboard`; Paste puts
//! them into the current folder: after Cut it runs `photo.moveTo {ids, folder}` (which asks the
//! first-time move question through the host) and empties the clipboard once the move ran, after
//! Copy it asks "Copy %d files to %s?" (`crate::dialogs::copy_confirm`), whose Copy runs
//! `photo.copyTo`.
//!
//! - `photo.cut {ids?}` (Ctrl+X in the library), `photo.copy {ids?}` (Ctrl+C): the photos named,
//!   else the selection; disabled with none.
//! - `photo.paste {folder?}` (Ctrl+V): into the folder named, else the current folder; disabled in
//!   an album, with nothing cut or copied, and until the engine's move and copy exist (M9.13).
//!
//! Putting paths and images on the system clipboard for other applications is OS integration
//! (M15). M9.23 adds the drag and drop that shares these commands' engine calls.

use dalica_core::{FolderId, PhotoId};
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::state::{ClipOp, Clipboard};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "photo.cut",
        label: "Cut",
        menu: "Edit/Cut",
        keys: &[(Context::Library, "Ctrl+X")],
        params: "{\"ids\":[PhotoId]=selection}: keep the photos to move with Paste",
        run: cut,
    },
    UiCommand {
        id: "photo.copy",
        label: "Copy",
        menu: "Edit/Copy",
        keys: &[(Context::Library, "Ctrl+C")],
        params: "{\"ids\":[PhotoId]=selection}: keep the photos to copy with Paste",
        run: copy,
    },
    UiCommand {
        id: "photo.paste",
        label: "Paste",
        menu: "Edit/Paste",
        keys: &[(Context::Library, "Ctrl+V")],
        params: "{\"folder\":FolderId=current}: move (after Cut) or copy (after Copy, asking first) the kept photos into the folder",
        run: paste,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("photo.cut", can_keep), ("photo.copy", can_keep), ("photo.paste", can_paste)];

/// Why Cut or Copy cannot run.
pub const NOTHING_SELECTED: &str = "Select some photos first.";
/// Why Paste cannot run.
pub const NOTHING_KEPT: &str = "Cut or copy some photos first.";
pub const IN_ALBUM: &str = "Paste into a folder, not an album.";
pub const NO_FOLDER: &str = "Select a folder first.";
pub const NOT_YET: &str = "Not available yet.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Keep {
    ids: Option<Vec<PhotoId>>,
}

/// The photos Cut or Copy keeps: those named, else the selection.
fn kept(app: &App, params: &Value) -> Result<Vec<PhotoId>, CmdError> {
    let p: Keep = parse_object(params)?;
    let ids = match p.ids {
        Some(ids) => super::albums::known_photos(app, ids)?,
        None => app.state().selection.ids.clone(),
    };
    if ids.is_empty() { Err(CmdError::Disabled(NOTHING_SELECTED.into())) } else { Ok(ids) }
}

fn can_keep(app: &App, params: &Value) -> Result<(), String> {
    kept(app, params).map(|_| ()).map_err(|e| e.to_string())
}

fn keep(app: &mut App, params: &Value, op: ClipOp) -> Result<Value, CmdError> {
    let ids = kept(app, params)?;
    let n = ids.len();
    app.state_mut().clipboard = Some(Clipboard { op, ids });
    Ok(json!({ "op": op, "count": n }))
}

fn cut(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    keep(app, params, ClipOp::Cut)
}

fn copy(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    keep(app, params, ClipOp::Copy)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paste {
    folder: Option<FolderId>,
}

/// What Paste does: the clipboard and the folder it goes into.
fn pasted(app: &App, params: &Value) -> Result<(Clipboard, FolderId), CmdError> {
    let p: Paste = parse_object(params)?;
    let exists = |f: FolderId| app.session().is_some_and(|s| s.library().folder(f).is_some());
    if let Some(f) = p.folder.filter(|f| !exists(*f)) {
        return Err(CmdError::NotFound(format!("no folder {f}")));
    }
    let clip = app.state().clipboard.clone().filter(|c| !c.ids.is_empty()).ok_or_else(|| CmdError::Disabled(NOTHING_KEPT.into()))?;
    let folder = match p.folder {
        Some(f) => f,
        None if app.state().selection.section.is_some_and(crate::state::Section::is_album) => return Err(CmdError::Disabled(IN_ALBUM.into())),
        None => crate::menus::current_folder(app).filter(|f| exists(*f)).ok_or_else(|| CmdError::Disabled(NO_FOLDER.into()))?,
    };
    let command = command(clip.op);
    if !app.session().is_some_and(|s| s.spec(command).is_some()) {
        return Err(CmdError::Disabled(NOT_YET.into()));
    }
    Ok((clip, folder))
}

/// The engine command a paste after `op` runs.
fn command(op: ClipOp) -> &'static str {
    match op {
        ClipOp::Cut => "photo.moveTo",
        ClipOp::Copy => "photo.copyTo",
    }
}

fn can_paste(app: &App, params: &Value) -> Result<(), String> {
    pasted(app, params).map(|_| ()).map_err(|e| e.to_string())
}

/// `photo.paste {folder?}`: after Cut moves the photos (asking through the host the first time)
/// and empties the clipboard; after Copy asks "Copy %d files to %s?".
fn paste(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let (clip, folder) = pasted(app, params)?;
    match clip.op {
        ClipOp::Copy => crate::dialogs::copy_confirm::open(app, clip.ids, folder),
        ClipOp::Cut => {
            let reply = app.run(command(clip.op), &json!({ "ids": clip.ids, "folder": folder }))?;
            // A question opened: the photos stay kept until the move runs.
            if reply.get("dialog").is_none() {
                app.state_mut().clipboard = None;
            }
            Ok(reply)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::state::{AlbumSection, Section, Selection};

    #[test]
    fn cut_and_copy_keep_the_selection_and_paste_waits_for_its_commands() {
        let (_dir, s, ids) = session_with_photos(2);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("photo.cut", &json!({})), Err(CmdError::Disabled(_))), "nothing selected");
        assert_eq!(can_paste(&app, &json!({})), Err(NOTHING_KEPT.into()));
        app.run("select.set", &json!({ "ids": [ids[0]] })).unwrap();
        assert_eq!(app.run("photo.cut", &json!({})).unwrap(), json!({ "op": "cut", "count": 1 }));
        assert_eq!(app.state().clipboard, Some(Clipboard { op: ClipOp::Cut, ids: vec![ids[0]] }));
        app.run("photo.copy", &json!({ "ids": [ids[1]] })).unwrap();
        assert_eq!(app.state().clipboard, Some(Clipboard { op: ClipOp::Copy, ids: vec![ids[1]] }));
        // Into the current folder, now that the engine can copy (M9.13).
        assert_eq!(can_paste(&app, &json!({})), Ok(()));
        assert_eq!(can_paste(&app, &json!({ "folder": folder })), Ok(()));
        // In an album there is nowhere to paste.
        app.state_mut().selection = Selection::new(vec![], None, Some(Section::Album(AlbumSection::Starred)), true);
        assert_eq!(can_paste(&app, &json!({})), Err(IN_ALBUM.into()));
        // Graceful failures change nothing.
        let before = app.state().clipboard.clone();
        for (id, bad) in [
            ("photo.cut", json!({ "ids": 1 })),
            ("photo.copy", json!([1])),
            ("photo.paste", json!({ "folder": "x" })),
            ("photo.paste", json!({ "to": 1 })),
        ] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
        assert!(matches!(app.run("photo.copy", &json!({ "ids": [9999] })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("photo.paste", &json!({ "folder": 9999 })), Err(CmdError::NotFound(_))));
        assert_eq!(app.state().clipboard, before);
    }
}
