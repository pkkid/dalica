//! The photo viewer's Settings (`viewing.md` §3.3; ⋯ ▸ Settings…, Tools ▸ Configure Photo Viewer…,
//! `viewer.settings`): two tabs, File Types (which file types Dalica opens by default, M15.24) and
//! Interface (the mouse wheel, "Hide controls automatically", "Translucent background"), [OK]
//! [Cancel]. The tabs draw Options ▸ Photo Viewer's two parts
//! ([`crate::dialogs::options::viewer`]).
//!
//! It opens on Interface, the part used most from the viewer. Its draft is `dialogs[].fields`:
//! `tab` (`"fileTypes"` or `"interface"`), `values`, the options changed (by settings key; a
//! change back to the saved value is taken out), and `types`, the file types checked once File
//! Types was changed (absent: the types last chosen). OK sends the values in one `options.set` and
//! then, when `types` differs from the types last chosen, runs `viewer.setDefault {types}` (nothing
//! changed: it just closes); the viewer reads the options at once. A failed OK keeps the dialog
//! open with the reason on the status line. Cancel drops them.
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
use crate::tl;

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
    Look {
        title: tl!("Photo Viewer Settings"),
        width: WIDTH,
        buttons: vec![Button::new("ok", tl!("OK"), ButtonKind::Default).enabled(ok), Button::cancel()],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let tab = d.fields.get("tab").and_then(Value::as_str).filter(|s| TABS.iter().any(|(v, _, _)| v == s)).unwrap_or("interface").to_string();
    let mut chosen = tab.clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (value, id, label) in TABS {
            if widgets::text_button(ui, &t, id, &super::options::tab_label(label), chosen == value).clicked() {
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
    let listing = if chosen == "fileTypes" { part::listing(app, ui.ctx()) } else { part::Listing::default() };
    let selected = part::ids_of(d.fields.get("types")).unwrap_or_else(|| part::saved(app));
    let mut types: Option<Vec<String>> = None;
    let width = ui.available_width();
    let labels: Vec<String> = part::GROUPS.iter().map(|g| super::options::general::group_label(g)).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut changes = Vec::new();
    ui.allocate_ui(vec2(width, BODY_H), |ui| {
        ui.set_min_size(vec2(width, BODY_H));
        let form = Form::new(ui, &t, &labels);
        if chosen == "fileTypes" {
            if let Some(next) = part::file_types(ui, &t, &form, &listing, &selected) {
                types = Some(next);
            }
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
    if let Some(types) = types {
        d.fields.insert("types".into(), json!(types));
    }
}

/// OK: the changed options in one `options.set`.
pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "ok" => {
            let values = values(d);
            let options = if values.is_empty() { json!({}) } else { app.run("options.set", &json!({ "values": values }))? };
            match part::ids_of(d.fields.get("types")).filter(|t| *t != part::saved(app)) {
                Some(types) => Ok(json!({ "options": options, "fileTypes": part::set_default(app, &types)? })),
                None => Ok(options),
            }
        }
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
