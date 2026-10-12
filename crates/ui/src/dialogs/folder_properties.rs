//! Folder Properties (`organizing.md` §10): opened by `section.properties` (Folder ▸ Edit
//! Description…, a double-click on a tree row or a section title). The draft is
//! `{folder, name, date, useOldest, place, description}`: the folder's id and name (shown, read-only
//! until renaming arrives), its date (`null`: none set) or "use the date of the oldest photo", its
//! place and its description. OK runs `folder.info.set` once (one undo step); Cancel changes
//! nothing.
//!
//! The fields are drawn as read-only lines for now; the dialog's own layout (the date picker, the
//! text fields, Music for slideshows) replaces [`body`] later in this milestone.

use dalica_core::FolderId;
use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Folder Properties";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;

/// The dialog for `folder`, its draft filled from what the library knows (its name; the date,
/// place and description the model does not hold yet start empty).
pub fn dialog(app: &App, folder: FolderId) -> Dialog {
    let name = app
        .session()
        .and_then(|s| s.library().folder(folder))
        .map(|f| f.path.file_name().map_or_else(|| f.path.display().to_string(), |n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let mut d = Dialog::new(DialogKind::FolderProperties);
    d.fields = draft(folder, &name);
    d
}

/// A fresh draft for `folder` called `name`.
pub fn draft(folder: FolderId, name: &str) -> Map<String, Value> {
    let v = json!({ "folder": folder, "name": name, "date": null, "useOldest": true, "place": "", "description": "" });
    match v {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("ok", "OK", ButtonKind::Default)] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::draft_lines(app, ui, d, &["name", "date", "useOldest", "place", "description"]);
}

/// OK: `folder.info.set {id, description, place, date}`, `date` being `"oldest"` while "use the
/// date of the oldest photo" is on.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let f = &d.fields;
    let id = f.get("folder").cloned().ok_or_else(|| CmdError::Internal("the Folder Properties draft has no folder".into()))?;
    let date =
        if f.get("useOldest").and_then(Value::as_bool).unwrap_or(true) { json!("oldest") } else { f.get("date").cloned().unwrap_or(Value::Null) };
    let text = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    app.run("folder.info.set", &json!({ "id": id, "description": text("description"), "place": text("place"), "date": date }))
}
