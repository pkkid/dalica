//! Confirmations: the questions of commands whose UI asks first (`control-protocol.md` §5.5,
//! `data-model.md` §1.5), asked through the host. [`ASKS`] is the one table: the host opens the
//! question when such a command runs without `"confirm": true` and its question applies
//! ([`super::asking`]), `engine.execute` answers `needs_confirm` with the same title, text and
//! buttons ([`prompt`]), and the default button runs the command again with `"confirm": true`.
//!
//! The rows: Clear Tray (`tray.clear` with held photos), Remove Folder (`folder.remove`, the folder
//! context menu's Remove from Dalica…; the dialog names the folder), Watch Drive (`folders.set`
//! setting a volume root or the home folder to Scan Always), and M8's Save explanation
//! (`photo.save`) and Revert question (`photo.revert`), which [`super::save_explain`] and
//! [`super::revert`] draw and answer. The Folder Manager asks Watch Drive
//! too, when such a folder's Scan Always radio is clicked: that dialog has no command, and its
//! default button sets the radio in the Folder Manager's draft instead
//! ([`super::folder_manager::drive_confirmed`]); Cancel leaves the radio as it was.
//!
//! M9's rows: Delete Album (`album.delete`; the dialog names the album when the UI knows it),
//! Remove from Album (`album.remove`, [`super::remove_from_album`]), the delete questions
//! (`photo.delete`, `folder.delete`, [`super::delete`]) and the first-time move question
//! (`photo.moveTo`, [`super::move_confirm`], with "Don't ask again"). Each asks only once its
//! command is registered ([`registered`]): until then running it is the engine's
//! `unknown_command`, never a question whose answer could not run. The settings that skip them
//! ("Remove from albums without asking", "Delete without asking", "Ask before moving") are the
//! engine's (M9.3) and are read where those modules ask.
//!
//! M10's row: Reset Folder Data (`folder.resetData`, the [Reset folder data] button of a damaged
//! folder's banner, `crate::panels::light_table::folder_banner`), asked only for a folder whose
//! sidecar is damaged; the dialog names the folder, says what the reset does and that the damaged
//! copy stays beside the photos, with [Reset] [Cancel].
//!
//! M11's row: Stop Import (`room.close` on the Import room while an import job copies, the tab's
//! ×): "Stop importing?" [Stop Import] [Keep Importing] (its Cancel button, [`cancel_label`]); Stop
//! Import cancels the job and closes the room (`crate::commands::rooms`).
//!
//! M13.27's rows: Remove Faces (`faces.removeInFolders`, asked after the Folder Manager's OK turned
//! face finding off for folders with faces found; [Remove Faces] [Keep Faces]), Write People Names
//! (`faces.writeXmp` on the selection), Remove Face Model (`faceModel.remove`, Options ▸ People's
//! Remove) and Delete Face Data (`faceModel.dropData`, Find faces in my photos turned off; [Delete
//! Face Data] [Keep]); their rows are `crate::commands::face_model`'s.
//!
//! To route another command's question through the host: give the command a `confirm` parameter,
//! add its [`DialogKind`] variant (the `<kind>` of `dialog:<kind>`), a row here and the variant to
//! the arms of `super::{look, body, act}` that send confirmations to this module.

use dalica_engine::CmdError;
use dalica_engine::cmd::folder_data;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogCommand, DialogKind, Look, delete, folder_manager, move_confirm, remove_from_album, revert, save_explain};
use crate::app::App;
use crate::commands::{face_model, rooms};
use crate::tray;

/// The width of a confirmation (points).
pub const WIDTH: f32 = 360.0;

/// A command whose UI asks first, and its question.
pub struct Ask {
    /// The command; it takes `"confirm": true` to go ahead.
    pub command: &'static str,
    pub kind: DialogKind,
    pub title: &'static str,
    pub text: &'static str,
    /// The default button: its action (`button:dialog.<action>`) and label.
    pub action: &'static str,
    pub label: &'static str,
    /// Whether running the command with these parameters asks (some ask only sometimes: Clear asks
    /// only when held photos would go).
    pub asks: fn(&App, &Value) -> bool,
    /// The question for these parameters when it names what it asks about (Remove Folder names the
    /// folder); `None`, or a `None` answer: [`Ask::text`].
    pub text_for: Option<fn(&App, &Value) -> Option<String>>,
    /// Its other buttons besides the default one and Cancel, as (action, label): Revert's Undo
    /// Save.
    pub extra: &'static [(&'static str, &'static str)],
}

/// The Remove Folder question when the folder is not known.
pub const REMOVE_QUESTION: &str = "Remove this folder and its subfolders from Dalica? The files stay on your disk.";
/// The Remove Folder dialog's title and default button.
pub const REMOVE_TITLE: &str = "Remove Folder";
/// The Watch Drive dialog's title and default button.
pub const DRIVE_TITLE: &str = "Watch Drive";
/// The Delete Album dialog's title and default button, and its question when the album's name is
/// not known (`organizing.md` §4.3).
pub const DELETE_ALBUM_TITLE: &str = "Delete Album";
pub const DELETE_ALBUM_QUESTION: &str = "Delete this album? The pictures in it are not deleted.";

/// Every command that asks through the host.
pub static ASKS: &[Ask] = &[
    Ask {
        command: "tray.clear",
        kind: DialogKind::ClearTray,
        title: tray::CLEAR_TITLE,
        text: tray::CLEAR_QUESTION,
        action: "clearTray",
        label: tray::CLEAR_TITLE,
        asks: tray_clear_asks,
        text_for: None,
        extra: &[],
    },
    Ask {
        command: "folder.remove",
        kind: DialogKind::RemoveFolder,
        title: REMOVE_TITLE,
        text: REMOVE_QUESTION,
        action: "removeFolder",
        label: REMOVE_TITLE,
        asks: folder_remove_asks,
        text_for: Some(folder_remove_text),
        extra: &[],
    },
    Ask {
        command: "folders.set",
        kind: DialogKind::WatchDrive,
        title: DRIVE_TITLE,
        text: dalica_engine::folders::DRIVE_QUESTION,
        action: "watchDrive",
        label: DRIVE_TITLE,
        asks: folder_manager::watches_a_drive,
        text_for: None,
        extra: &[],
    },
    Ask {
        command: "photo.save",
        kind: DialogKind::SaveExplain,
        title: save_explain::TITLE,
        text: save_explain::TEXT,
        action: "save",
        label: save_explain::TITLE,
        asks: save_explain::asks,
        text_for: None,
        extra: &[],
    },
    Ask {
        command: "photo.revert",
        kind: DialogKind::Revert,
        title: revert::TITLE,
        text: revert::TEXT,
        action: "revert",
        label: revert::TITLE,
        asks: revert::asks,
        text_for: None,
        extra: &[("undoSave", revert::UNDO_SAVE)],
    },
    Ask {
        command: "album.delete",
        kind: DialogKind::DeleteAlbum,
        title: DELETE_ALBUM_TITLE,
        text: DELETE_ALBUM_QUESTION,
        action: "deleteAlbum",
        label: DELETE_ALBUM_TITLE,
        asks: album_delete_asks,
        text_for: Some(album_delete_text),
        extra: &[],
    },
    Ask {
        command: "album.remove",
        kind: DialogKind::RemoveFromAlbum,
        title: remove_from_album::TITLE,
        text: remove_from_album::TEXT,
        action: "remove",
        label: remove_from_album::REMOVE,
        asks: remove_from_album::asks,
        text_for: Some(remove_from_album::text_for),
        extra: &[],
    },
    Ask {
        command: "photo.delete",
        kind: DialogKind::DeletePhotos,
        title: delete::PHOTOS_TITLE,
        text: delete::PHOTOS_TEXT,
        action: "moveToTrash",
        label: delete::MOVE_TO_TRASH,
        asks: delete::photos_ask,
        text_for: Some(delete::photos_text),
        extra: &[],
    },
    Ask {
        command: "folder.delete",
        kind: DialogKind::DeleteFolder,
        title: delete::FOLDER_TITLE,
        text: delete::FOLDER_TEXT,
        action: "moveToTrash",
        label: delete::MOVE_TO_TRASH,
        asks: delete::folder_asks,
        text_for: Some(delete::folder_text),
        extra: &[],
    },
    Ask {
        command: "photo.moveTo",
        kind: DialogKind::MoveFiles,
        title: move_confirm::TITLE,
        text: move_confirm::TEXT,
        action: "move",
        label: move_confirm::MOVE,
        asks: move_confirm::asks,
        text_for: None,
        extra: &[],
    },
    Ask {
        command: "folder.resetData",
        kind: DialogKind::ResetFolderData,
        title: folder_data::RESET_TITLE,
        text: folder_data::RESET_QUESTION,
        action: "reset",
        label: folder_data::RESET_BUTTON,
        asks: reset_asks,
        text_for: Some(reset_text),
        extra: &[],
    },
    Ask {
        command: "room.close",
        kind: DialogKind::StopImport,
        title: rooms::STOP_TITLE,
        text: rooms::STOP_QUESTION,
        action: "stopImport",
        label: rooms::STOP_TITLE,
        asks: rooms::close_asks,
        text_for: None,
        extra: &[],
    },
    // M13.27's questions (`crate::commands::face_model`).
    face_model::REMOVE_FACES,
    face_model::WRITE_NAMES,
    face_model::REMOVE_MODEL,
    face_model::DROP_DATA,
];

/// The label of `kind`'s Cancel button: "Cancel", or Stop Import's "Keep Importing".
pub fn cancel_label(kind: DialogKind) -> &'static str {
    match kind {
        DialogKind::StopImport => rooms::KEEP_IMPORTING,
        DialogKind::RemoveFaces => face_model::KEEP_FACES,
        DialogKind::DropFaceData => face_model::KEEP_DATA,
        _ => "Cancel",
    }
}

/// The damaged library folder `params.folder` names, by its name: `folder.resetData` asks only
/// then (an unknown, bad or undamaged folder is the command's to refuse).
fn reset_folder(app: &App, params: &Value) -> Option<String> {
    let session = app.session()?;
    let id = dalica_core::FolderId::new(params.get("folder")?.as_u64()?);
    session.damaged_folder(id)?;
    let folder = session.library().folder(id)?;
    Some(folder.path.file_name().map_or_else(|| folder.path.display().to_string(), |n| n.to_string_lossy().into_owned()))
}

fn reset_asks(app: &App, params: &Value) -> bool {
    registered(app, "folder.resetData") && reset_folder(app, params).is_some()
}

fn reset_text(app: &App, params: &Value) -> Option<String> {
    reset_folder(app, params).map(|name| {
        format!(
            "Reset the data of the folder \"{name}\"? Dalica writes a new data file from what it knows about these photos. The damaged file stays beside the photos, untouched."
        )
    })
}

/// Whether `command` is one of the open library's: a question is asked only for a command whose
/// answer can run.
pub fn registered(app: &App, command: &str) -> bool {
    app.session().is_some_and(|s| s.spec(command).is_some())
}

/// The user album `params.album` names, if it is one.
fn named_album(params: &Value) -> Option<dalica_core::AlbumId> {
    params.get("album")?.as_str().and_then(|a| dalica_core::AlbumId::parse(a).ok())
}

/// `album.delete` asks when it names an album (a missing or bad one is the command's to refuse).
fn album_delete_asks(app: &App, params: &Value) -> bool {
    registered(app, "album.delete") && named_album(params).is_some()
}

fn album_delete_text(app: &App, params: &Value) -> Option<String> {
    let name = crate::commands::albums::album_name(app, &named_album(params)?)?;
    Some(format!("Delete the album \"{name}\"? The pictures in it are not deleted."))
}

/// `tray.clear` asks when no photos are named and it would empty a tray holding held photos.
fn tray_clear_asks(app: &App, params: &Value) -> bool {
    params.get("ids").is_none_or(Value::is_null) && !tray::is_empty(app.state()) && tray::clear_asks(app.state())
}

/// The name of the library folder `params.id` names, if it is one: `folder.remove` asks only then
/// (an unknown or bad id is the command's to refuse).
fn removed_folder(app: &App, params: &Value) -> Option<String> {
    let id = params.get("id")?.as_u64()?;
    let folder = app.session()?.library().folder(dalica_core::FolderId::new(id))?;
    Some(folder.path.file_name().map_or_else(|| folder.path.display().to_string(), |n| n.to_string_lossy().into_owned()))
}

fn folder_remove_asks(app: &App, params: &Value) -> bool {
    removed_folder(app, params).is_some()
}

fn folder_remove_text(app: &App, params: &Value) -> Option<String> {
    removed_folder(app, params).map(|name| format!("Remove the folder \"{name}\" and its subfolders from Dalica? The files stay on your disk."))
}

/// The row of `kind`.
pub fn of(kind: DialogKind) -> Option<&'static Ask> {
    ASKS.iter().find(|a| a.kind == kind)
}

/// The dialog asking `ask`'s question for running its command with `params`.
pub fn dialog(ask: &Ask, params: &Value) -> Dialog {
    let params = if params.is_null() { Value::Object(Map::new()) } else { params.clone() };
    Dialog { command: Some(DialogCommand { id: ask.command.into(), params }), ..Dialog::new(ask.kind) }
}

/// `needs_confirm`'s `prompt` (`control-protocol.md` §5.5): the dialog's title, text and buttons.
pub fn prompt(ask: &Ask) -> Value {
    let buttons: Vec<&str> = std::iter::once(ask.label).chain(ask.extra.iter().map(|(_, label)| *label)).chain([cancel_label(ask.kind)]).collect();
    json!({ "title": ask.title, "text": ask.text, "buttons": buttons, "dialog": ask.kind.name() })
}

/// The parameters a confirmation's button runs its command with: what was asked about, plus
/// `"confirm": true` and `extra` (a button's own parameters, a checkbox's answer).
pub fn confirmed_params(d: &Dialog, extra: &[(&str, Value)]) -> Result<(String, Value), CmdError> {
    let c = d.command.as_ref().ok_or_else(|| CmdError::Internal(format!("the {} confirmation has no command", d.kind.name())))?;
    let mut params = match &c.params {
        Value::Object(o) => o.clone(),
        _ => Map::new(),
    };
    params.insert("confirm".into(), Value::Bool(true));
    for (k, v) in extra {
        params.insert((*k).into(), v.clone());
    }
    Ok((c.id.clone(), Value::Object(params)))
}

pub fn look(d: &Dialog) -> Look {
    let (title, action, label) = of(d.kind).map_or(("Confirm", "ok", "OK"), |a| (a.title, a.action, a.label));
    Look {
        title: title.into(),
        width: WIDTH,
        buttons: vec![Button::new("cancel", cancel_label(d.kind), ButtonKind::Cancel), Button::new(action, label, ButtonKind::Default)],
    }
}

/// The question dialog `d` asks: its row's text, or the one that names what it asks about.
pub fn question(app: &App, d: &Dialog) -> String {
    let Some(ask) = of(d.kind) else { return String::new() };
    let params = d.command.as_ref().map_or(&Value::Null, |c| &c.params);
    ask.text_for.and_then(|f| f(app, params)).unwrap_or_else(|| ask.text.to_string())
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let text = question(app, d);
    ui.label(RichText::new(text).font(fonts::regular(t.body)).color(t.text));
}

/// The default button: runs the command asked about with `"confirm": true`. Watch Drive asked by
/// the Folder Manager's radio (no command) sets the radio in its draft instead.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    if d.command.is_none() && d.kind == DialogKind::WatchDrive {
        return folder_manager::drive_confirmed(app, d);
    }
    let (id, params) = confirmed_params(d, &[])?;
    app.run(&id, &params)
}
