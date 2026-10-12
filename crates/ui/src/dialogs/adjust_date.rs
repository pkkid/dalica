//! Adjust Date and Time (`organizing.md` §9): opened by `photo.adjustDate` (Tools ▸ Adjust Date and
//! Time…) for the selected photos. The draft is `{ids, current, mode, shift, set}`: the photos, the
//! first one's date as the library knows it (`null`: none), `"shift"` (every photo by the same
//! amount, the default) or `"set"` (all to one date and time), the shift in seconds and the date to
//! set (`"YYYY-MM-DDTHH:MM:SS"`). OK runs `photo.dates.set` once (one undo step).
//!
//! The fields are drawn as read-only lines for now; the date and time pickers replace [`body`]
//! later in this milestone.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

/// The title for one photo; several add " — N pictures".
pub const TITLE: &str = "Adjust Picture Date";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;

/// The dialog for `ids`, its draft starting from the first photo's date.
pub fn dialog(app: &App, ids: &[PhotoId]) -> Dialog {
    let current = ids.first().and_then(|id| app.session()?.library().photo(*id)?.date);
    let mut d = Dialog::new(DialogKind::AdjustDate);
    let v = json!({ "ids": ids, "current": current, "mode": "shift", "shift": 0, "set": current });
    if let Value::Object(o) = v {
        d.fields = o;
    }
    d
}

/// The photos of the draft.
fn ids(fields: &Map<String, Value>) -> Vec<Value> {
    fields.get("ids").and_then(Value::as_array).cloned().unwrap_or_default()
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let n = ids(&d.fields).len();
    let title = if n > 1 { format!("{TITLE} — {n} pictures") } else { TITLE.to_string() };
    Look { title, width: WIDTH, buttons: vec![Button::cancel(), Button::new("ok", "OK", ButtonKind::Default)] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::draft_lines(app, ui, d, &["current", "mode", "shift", "set"]);
}

/// OK: `photo.dates.set {ids, shift}` or `{ids, set}`.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let f = &d.fields;
    let ids = ids(f);
    match f.get("mode").and_then(Value::as_str) {
        Some("set") => {
            let set = f.get("set").filter(|v| v.is_string()).cloned().ok_or_else(|| CmdError::Disabled("Choose a date first.".into()))?;
            app.run("photo.dates.set", &json!({ "ids": ids, "set": set }))
        }
        _ => app.run("photo.dates.set", &json!({ "ids": ids, "shift": f.get("shift").cloned().unwrap_or(json!(0)) })),
    }
}
