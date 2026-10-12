//! Configure Quick Tags (`organizing.md` §5.2): opened by `quicktags.configure` (the gear of the
//! Tags pane's quick tags). The draft is `{useRecent, slots}`: "Use the top two buttons for recently
//! used tags" and the ten slots' tags (`""`: unassigned). OK runs `tags.quick.set` once.
//!
//! The fields are drawn as read-only lines for now; the checkbox, the ten text fields and [Fill
//! Empty with Common Tags] replace [`body`] later in this milestone.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Configure Quick Tags";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;
/// The number of quick tags.
pub const SLOTS: usize = 10;

/// The dialog, its draft starting with the recent-tags option on and no slot assigned (the stored
/// quick tags fill it once the engine keeps them).
pub fn dialog(_app: &App) -> Dialog {
    let mut d = Dialog::new(DialogKind::QuickTags);
    if let Value::Object(o) = json!({ "useRecent": true, "slots": vec![""; SLOTS] }) {
        d.fields = o;
    }
    d
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("ok", "OK", ButtonKind::Default)] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::draft_lines(app, ui, d, &["useRecent", "slots"]);
}

/// OK: `tags.quick.set {slots, useRecent}`.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let f = &d.fields;
    let slots = f.get("slots").cloned().unwrap_or_else(|| json!(vec![""; SLOTS]));
    let use_recent = f.get("useRecent").and_then(Value::as_bool).unwrap_or(true);
    app.run("tags.quick.set", &json!({ "slots": slots, "useRecent": use_recent }))
}
