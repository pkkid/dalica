//! The photo viewer's Settings (`viewing.md` §3.3; ⋯ ▸ Settings…, Tools ▸ Configure Photo Viewer…,
//! `viewer.settings`): two tabs, File Types (drawn disabled, "Arrives with OS integration") and
//! Interface (the mouse wheel, "Hide controls automatically", "Translucent background"), [OK]
//! [Cancel]. The tabs draw Options ▸ Photo Viewer's two parts
//! ([`crate::dialogs::options::viewer`]).
//!
//! It opens on Interface, the part that acts in this build. Its draft is `dialogs[].fields`:
//! `tab` (`"fileTypes"` or `"interface"`) and `values`, the options changed (by settings key; a
//! change back to the saved value is taken out). OK sends them in one `options.set` (nothing
//! changed: it just closes), which the viewer reads at once; a failed OK keeps the dialog open with
//! the reason on the status line. Cancel drops them.
//!
//! Automation ids: the tabs `tab:viewerSettings.fileTypes` and `tab:viewerSettings.interface`; the
//! rows as in Options ▸ Photo Viewer.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::{self, ButtonKind, Form};
use egui::{Ui, vec2};
use serde_json::{Map, Value, json};

use super::options::viewer as part;
use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Photo Viewer Settings";
/// The dialog's width, and the height of a tab's body (points).
pub const WIDTH: f32 = 520.0;
pub const BODY_H: f32 = 200.0;
/// The tabs: their draft value, widget id and label.
pub const TABS: [(&str, &str, &str); 2] =
    [("fileTypes", "tab:viewerSettings.fileTypes", "File Types"), ("interface", "tab:viewerSettings.interface", "Interface")];

/// A dialog of this kind on Interface with nothing changed.
pub fn dialog() -> Dialog {
    let mut d = Dialog::new(DialogKind::ViewerSettings);
    d.fields.insert("tab".into(), json!("interface"));
    d.fields.insert("values".into(), json!({}));
    d
}

/// The options changed in `d`'s draft.
fn values(d: &Dialog) -> Map<String, Value> {
    d.fields.get("values").and_then(Value::as_object).cloned().unwrap_or_default()
}

/// OK is enabled while the dialog has its draft (one opened without it, by `ui.set`, has nothing
/// it could set).
pub fn look(_app: &App, d: &Dialog) -> Look {
    let ok = d.fields.get("values").is_some_and(Value::is_object);
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::new("ok", "OK", ButtonKind::Default).enabled(ok), Button::cancel()] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let tab = d.fields.get("tab").and_then(Value::as_str).filter(|s| TABS.iter().any(|(v, _, _)| v == s)).unwrap_or("interface").to_string();
    let mut chosen = tab.clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (value, id, label) in TABS {
            if widgets::text_button(ui, &t, id, label, chosen == value).clicked() {
                chosen = value.to_string();
            }
        }
    });
    if chosen != tab {
        d.fields.insert("tab".into(), json!(chosen));
    }
    let Some(mut draft) = d.fields.get("values").and_then(Value::as_object).cloned() else { return };
    ui.add_space(12.0);
    let opts = part::rows(app);
    let width = ui.available_width();
    let labels: Vec<String> = part::GROUPS.iter().map(|g| format!("{g}:")).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut changes = Vec::new();
    ui.allocate_ui(vec2(width, BODY_H), |ui| {
        ui.set_min_size(vec2(width, BODY_H));
        let form = Form::new(ui, &t, &labels);
        if chosen == "fileTypes" {
            part::file_types(ui, &t, &form, &opts);
        } else {
            changes = part::interface(ui, &t, &form, &opts, &draft);
        }
    });
    for (i, value) in changes {
        let Some(o) = opts.get(i) else { continue };
        if value == o.row.saved {
            draft.remove(&o.row.key);
        } else {
            draft.insert(o.row.key.clone(), value);
        }
    }
    d.fields.insert("values".into(), Value::Object(draft));
}

/// OK: the changed options in one `options.set`.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "ok" => {
            let values = values(d);
            if values.is_empty() { Ok(json!({})) } else { app.run("options.set", &json!({ "values": values })) }
        }
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
