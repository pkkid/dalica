//! Confirmations: the questions of commands whose UI asks first (`control-protocol.md` §5.5,
//! `data-model.md` §1.5), asked through the host. [`ASKS`] is the one table: the host opens the
//! question when such a command runs without `"confirm": true` and its question applies
//! ([`super::asking`]), `engine.execute` answers `needs_confirm` with the same title, text and
//! buttons ([`prompt`]), and the default button runs the command again with `"confirm": true`.
//!
//! The rows: Clear Tray (`tray.clear` with held photos), Remove Folder (`folder.remove`, the folder
//! context menu's Remove from Dalica…; the dialog names the folder) and Watch Drive (`folders.set`
//! setting a volume root or the home folder to Scan Always). The Folder Manager asks Watch Drive
//! too, when such a folder's Scan Always radio is clicked: that dialog has no command, and its
//! default button sets the radio in the Folder Manager's draft instead
//! ([`super::folder_manager::drive_confirmed`]); Cancel leaves the radio as it was.
//!
//! To route another command's question through the host: give the command a `confirm` parameter,
//! add its [`DialogKind`] variant (the `<kind>` of `dialog:<kind>`), a row here and the variant to
//! the arms of `super::{look, body, act}` that send confirmations to this module.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogCommand, DialogKind, Look, folder_manager};
use crate::app::App;
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
}

/// The Remove Folder question when the folder is not known.
pub const REMOVE_QUESTION: &str = "Remove this folder and its subfolders from Dalica? The files stay on your disk.";
/// The Remove Folder dialog's title and default button.
pub const REMOVE_TITLE: &str = "Remove Folder";
/// The Watch Drive dialog's title and default button.
pub const DRIVE_TITLE: &str = "Watch Drive";

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
    },
];

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
    json!({ "title": ask.title, "text": ask.text, "buttons": [ask.label, "Cancel"], "dialog": ask.kind.name() })
}

pub fn look(d: &Dialog) -> Look {
    let (title, action, label) = of(d.kind).map_or(("Confirm", "ok", "OK"), |a| (a.title, a.action, a.label));
    Look { title: title.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new(action, label, ButtonKind::Default)] }
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
    let c = d.command.as_ref().ok_or_else(|| CmdError::Internal(format!("the {} confirmation has no command", d.kind.name())))?;
    let mut params = match &c.params {
        Value::Object(o) => o.clone(),
        _ => Map::new(),
    };
    params.insert("confirm".into(), Value::Bool(true));
    app.run(&c.id, &Value::Object(params))
}
