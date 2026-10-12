//! Rename Tag (the Tags pane's tag menu, `menus-and-shortcuts.md` §4): opened by `tag.rename
//! {tag}`. The draft is `{from, to}`: the tag and its new name (starting as the tag). OK runs
//! `tags.rename` once, which renames the tag on every photo (merging onto an existing tag).
//!
//! The fields are drawn as read-only lines for now; the text field and the confirmation replace
//! [`body`] later in this milestone.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Rename Tag";
/// The dialog's width (points).
pub const WIDTH: f32 = 360.0;

/// The dialog renaming `tag`.
pub fn dialog(tag: &str) -> Dialog {
    let mut d = Dialog::new(DialogKind::RenameTag);
    if let Value::Object(o) = json!({ "from": tag, "to": tag }) {
        d.fields = o;
    }
    d
}

/// The new name in the draft, trimmed.
fn to(d: &Dialog) -> &str {
    d.fields.get("to").and_then(Value::as_str).unwrap_or("").trim()
}

/// OK is disabled while the new name is empty.
pub fn look(_app: &App, d: &Dialog) -> Look {
    let ok = Button::new("ok", "OK", ButtonKind::Default).enabled(!to(d).is_empty());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), ok] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::draft_lines(app, ui, d, &["from", "to"]);
}

/// OK: `tags.rename {from, to}`.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let from = d.fields.get("from").cloned().unwrap_or(Value::Null);
    app.run("tags.rename", &json!({ "from": from, "to": to(d) }))
}
