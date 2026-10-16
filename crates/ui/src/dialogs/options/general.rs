//! Options ▸ General (`menus-and-shortcuts.md` §5 General, `design-system.md` §11.3–§11.5): the
//! Interface, Files and Import groups, filled from `options.list {tab: "general"}` (with no library
//! open, the catalogue's rows on an empty settings file), on M8.5's form rows:
//!
//! ```text
//!              Interface:  [x] Use animations
//!                              Smooth transitions when rotating, applying edits and previewing.
//!                          [x] Show tooltips
//!                          [ ] Use system title bar          (Windows and Linux)
//!                              Applies at next launch.
//!                          [ ] Single click on the photo returns to the library
//!               UI scale:  [Auto ▾]
//!                          Auto follows the system's scale and the monitor.
//!           UI font size:  [Small ▾]
//!               Language:  [System ▾]                        (disabled: Arrives with translations)
//!  ───────────────────────────────────────────────────────────
//!                  Files:  [x] Detect duplicates while importing   (disabled: Arrives with Import)
//!                          [ ] Delete from disk without asking
//!                          …
//!  ───────────────────────────────────────────────────────────
//! Save imported pictures in:  [The Pictures folder      ] [Browse…]   (disabled: Arrives with Import)
//! ```
//!
//! Each group's on/off options are checkboxes under the group's label, every other option a row of
//! its own labelled with its name: UI scale and font size and Language as dropdowns, a folder as
//! its path and Browse…. An option whose feature does not exist yet is drawn disabled with its
//! reason as the tooltip; an option of another platform is left out. The body scrolls (by the
//! wheel) when the dialog is shorter than the tab.
//!
//! A change goes into the draft (`options.values`, by settings key) unless it puts the saved value
//! back, which takes the key out, so Apply is enabled exactly while the draft differs from the
//! saved settings. UI scale and font size also set the preview (`options.previewScale`,
//! `options.previewFontSize`), which [`crate::scale`] shows at once.
//!
//! Automation ids (`control-protocol.md` §7): an on/off option `check:option.<key>`
//! (`check:option.interface.tooltips`), a dropdown `choice:option.<key>` and its items
//! `choice:option.<key>/<value>` (`choice:option.interface.uiScale/150`,
//! `choice:option.interface.fontSize/large`), a folder's path `field:option.<key>` and its Browse…
//! `button:option.<key>/browse`. A folder option becomes available with Import; until then its
//! Browse… is drawn disabled.

use dalica_engine::keymap::Platform;
use dalica_engine::options::{Tab, on_tab};
use dalica_engine::settings::Settings;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{Form, TextField, checkbox_row, dropdown, dropdown_item, form_group, hint, text_button, tooltip};
use egui::Ui;
use serde_json::{Map, Value, json};

use crate::app::App;
use crate::state::{FontChoice, ScaleChoice};

/// The tab's groups, in order.
pub const GROUPS: [&str; 3] = ["Interface", "Files", "Import"];

/// The settings keys whose choice previews live.
pub const UI_SCALE: &str = "interface.uiScale";
pub const FONT_SIZE: &str = "interface.fontSize";

/// One option of the tab, as `options.list` gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub key: String,
    pub group: String,
    pub label: String,
    pub hint: String,
    /// `bool`, `scale`, `choice`, `folder` (`int` and `text` are drawn as a choice of their value).
    pub kind: String,
    /// The value saved (in effect).
    pub saved: Value,
    /// Why it cannot be changed in this build, `None` when it can.
    pub reason: Option<String>,
    /// A dropdown's values and labels.
    pub choices: Vec<(Value, String)>,
}

impl Row {
    fn from_json(v: &Value) -> Option<Row> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let key = v.get("key")?.as_str()?.to_string();
        let kind = text("type");
        let choices = match kind.as_str() {
            "scale" => {
                let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
                let (min, max, step) = (n("min"), n("max"), n("step").max(1));
                let fixed = (min..=max).step_by(usize::try_from(step).unwrap_or(1)).take(64).map(|p| (json!(p), format!("{p} %")));
                std::iter::once((json!("auto"), "Auto".to_string())).chain(fixed).collect()
            }
            _ => v
                .get("choices")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(|c| Some((c.get("id")?.clone(), c.get("label")?.as_str()?.to_string()))).collect())
                .unwrap_or_default(),
        };
        let available = v.get("available").and_then(Value::as_bool).unwrap_or(false);
        let reason = (!available).then(|| v.get("reason").and_then(Value::as_str).unwrap_or("Not available in this build").to_string());
        Some(Row {
            key,
            group: text("group"),
            label: text("label"),
            hint: text("hint"),
            kind,
            saved: v.get("value").cloned().unwrap_or(Value::Null),
            reason,
            choices,
        })
    }

    /// Whether it is shown on this platform.
    fn here(v: &Value) -> bool {
        let here = Platform::current().name();
        v.get("platforms").and_then(Value::as_array).is_none_or(|p| p.iter().any(|p| p.as_str() == Some(here)))
    }
}

/// The tab's options on this platform, in the dialog's order: `options.list {tab: "general"}`, or
/// with no library open the catalogue's rows on an empty settings file.
pub fn rows(app: &mut App) -> Vec<Row> {
    let listed = app.run("options.list", &json!({ "tab": Tab::General.id() })).ok();
    let list: Vec<Value> = match listed.as_ref().and_then(|v| v.get("options")).and_then(Value::as_array) {
        Some(rows) => rows.clone(),
        None => {
            let empty = Settings::default();
            on_tab(Tab::General).map(|o| o.to_json(&empty)).collect()
        }
    };
    list.iter().filter(|v| Row::here(v)).filter_map(Row::from_json).collect()
}

/// The automation id of the control of option `key` of kind `kind`.
pub fn control_id(kind: &str, key: &str) -> String {
    match kind {
        "bool" => format!("check:option.{key}"),
        "folder" => format!("field:option.{key}"),
        _ => format!("choice:option.{key}"),
    }
}

/// The automation id of the item for `value` in option `key`'s dropdown.
pub fn item_id(key: &str, value: &Value) -> String {
    match value {
        Value::String(s) => format!("choice:option.{key}/{s}"),
        other => format!("choice:option.{key}/{other}"),
    }
}

/// Puts `value` for `row` into the draft: taken out again when it is the saved value; UI scale and
/// font size also set (or clear) their preview.
pub fn set(app: &mut App, row: &Row, value: Value) {
    let Some(o) = app.state_mut().options.as_mut() else { return };
    let back = value == row.saved;
    if back {
        o.values.remove(&row.key);
    } else {
        o.values.insert(row.key.clone(), value.clone());
    }
    match row.key.as_str() {
        UI_SCALE => o.preview_scale = if back { None } else { serde_json::from_value::<ScaleChoice>(value).ok() },
        FONT_SIZE => o.preview_font_size = if back { None } else { serde_json::from_value::<FontChoice>(value).ok() },
        _ => {}
    }
}

pub fn body(app: &mut App, ui: &mut Ui) {
    let rows = rows(app);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let t = *app.tokens();
    let height = ui.available_height();
    let mut changes: Vec<(usize, Value)> = Vec::new();
    // A solid bar, shown whenever the tab is taller than the dialog, so the rows below are not
    // missed (egui's floating one shows only on hover).
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical()
        .id_salt("dialog:options/general")
        .max_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| changes = form(ui, &t, &rows, &draft));
    for (i, value) in changes {
        if let Some(row) = rows.get(i) {
            set(app, row, value);
        }
    }
}

/// The value of `row` shown: the draft's, else the saved one.
fn shown<'a>(row: &'a Row, draft: &'a Map<String, Value>) -> &'a Value {
    draft.get(&row.key).unwrap_or(&row.saved)
}

/// Draws the groups; returns the values changed this pass, by row.
fn form(ui: &mut Ui, t: &Tokens, rows: &[Row], draft: &Map<String, Value>) -> Vec<(usize, Value)> {
    let group_labels: Vec<String> = GROUPS.iter().map(|g| format!("{g}:")).collect();
    let row_labels: Vec<String> = rows.iter().filter(|r| r.kind != "bool").map(|r| format!("{}:", r.label)).collect();
    let labels: Vec<&str> = group_labels.iter().chain(&row_labels).map(String::as_str).collect();
    let form = Form::new(ui, t, &labels);
    let mut changes = Vec::new();
    let mut first = true;
    for group in GROUPS {
        let in_group: Vec<(usize, &Row)> = rows.iter().enumerate().filter(|(_, r)| r.group == group).collect();
        if in_group.is_empty() {
            continue;
        }
        if !first {
            ui.separator();
            ui.add_space(4.0);
        }
        first = false;
        let checks: Vec<&(usize, &Row)> = in_group.iter().filter(|(_, r)| r.kind == "bool").collect();
        if !checks.is_empty() {
            form.row(ui, t, &format!("{group}:"), |ui| {
                for &&(i, row) in &checks {
                    let mut on = shown(row, draft).as_bool().unwrap_or(false);
                    let hint_text = (!row.hint.is_empty()).then_some(row.hint.as_str());
                    let out = checkbox_row(ui, t, &control_id("bool", &row.key), &row.label, &mut on, hint_text, row.reason.as_deref());
                    if out.response.changed() {
                        changes.push((i, json!(on)));
                    }
                }
            });
        }
        for &(i, row) in in_group.iter().filter(|(_, r)| r.kind != "bool") {
            let label = format!("{}:", row.label);
            form_group(ui, row.reason.is_none(), |ui| {
                form.row(ui, t, &label, |ui| match row.kind.as_str() {
                    "folder" => folder(ui, t, row, shown(row, draft)),
                    _ => {
                        if let Some(v) = choice(ui, t, row, shown(row, draft)) {
                            changes.push((i, v));
                        }
                    }
                })
            });
        }
    }
    changes
}

/// A dropdown of `row`'s choices showing `value`; returns the choice picked this pass.
fn choice(ui: &mut Ui, t: &Tokens, row: &Row, value: &Value) -> Option<Value> {
    let text = row.choices.iter().find(|(v, _)| v == value).map_or_else(|| value.to_string(), |(_, l)| l.clone());
    let mut picked = None;
    let r = dropdown(ui, t, &control_id(&row.kind, &row.key), &text, |ui| {
        for (v, label) in &row.choices {
            if dropdown_item(ui, t, &item_id(&row.key, v), label, "", v == value).clicked() {
                picked = Some(v.clone());
            }
        }
    });
    if let Some(reason) = &row.reason {
        tooltip(r, t, reason, None);
    }
    if !row.hint.is_empty() {
        hint(ui, t, &row.hint);
    }
    picked
}

/// A folder option: its path (the hint while it is the default folder) and Browse…, which the
/// folder options of this tab do not have yet (they arrive with Import).
fn folder(ui: &mut Ui, t: &Tokens, row: &Row, value: &Value) {
    let mut path = value.as_str().unwrap_or_default().to_string();
    let placeholder = if row.hint.is_empty() { "The default folder" } else { row.hint.as_str() };
    ui.horizontal(|ui| {
        let browse_w = 90.0;
        let w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(60.0);
        // Shown, not typed in: a folder is chosen with Browse….
        let field = form_group(ui, false, |ui| TextField::new(&control_id("folder", &row.key), placeholder, w).show(ui, t, &mut path));
        let browse = form_group(ui, false, |ui| text_button(ui, t, &format!("button:option.{}/browse", row.key), "Browse…", false));
        if let Some(reason) = &row.reason {
            tooltip(field.response, t, reason, None);
            tooltip(browse, t, reason, None);
        }
    });
}

/// A stub tab's lines: its groups, then the draft's changed values (`key: value`), so the draft is
/// visible on screen and in screenshots (the File Types and Export tabs until M10.11 draws them).
pub fn stub_lines(app: &App, groups: &[&str]) -> Vec<String> {
    let mut lines = vec![groups.join(" · ")];
    if let Some(o) = &app.state().options {
        lines.extend(o.values.iter().map(|(k, v)| format!("{k}: {v}")));
    }
    lines
}
