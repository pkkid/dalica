//! Options ▸ Export (`menus-and-shortcuts.md` §5 Export, `creating.md` §2, `design-system.md`
//! §11.3): the defaults the Export dialog starts from, on M8.5's form rows:
//!
//! ```text
//! Default export location:  [Pictures/Dalica/Exports unless chosen. ] [Browse…]
//!  ───────────────────────────────────────────────────────────
//!        Size and quality:  [x] Resize to
//!                               Off: use the original size.
//!                    Size:  [1600] px ──|──|──|─o|────|───      (disabled while Resize to is off)
//!           Image quality:  [Automatic ▾]
//!          Custom quality:  [  75] ─────────o──────             (enabled for Custom)
//!  ───────────────────────────────────────────────────────────
//!                 Privacy:  [ ] Remove location from exported photos
//!  ───────────────────────────────────────────────────────────
//!  Default watermark text:  [                                  ]
//! ```
//!
//! The rows are the catalogue's (`dalica_engine::options`, through [`super::file_types::rows`]); a
//! change goes into the draft ([`super::general::set`]) and Apply sends it in the dialog's one
//! `options.set`. The engine's `export.defaults` reads these settings each time, so what is applied
//! here is what the next Export dialog starts from. The size slider is the Export dialog's (its
//! stops at 480 … 3840, `crate::dialogs::export::STOPS`), the quality dropdown its presets with
//! their descriptions.
//!
//! - **Default export location** shows the folder chosen (the default's description while there is
//!   none); it is chosen with **Browse…**, the folder picker ([`browse_request`]), which starts in
//!   the folder shown and puts the folder chosen into the draft by running [`FOLDER_COMMAND`]; a
//!   cancel changes nothing. Browse… is disabled while another file dialog is open or there is no
//!   picker.
//! - **Default watermark text** is typed; it is kept to the catalogue's length.
//!
//! Automation ids (`control-protocol.md` §7): an on/off option `check:option.<key>`, the location
//! `field:option.export.location` and its `button:option.export.location/browse`, the size and the
//! custom quality's sliders `slider:option.export.size`, `slider:option.export.customQuality`
//! (their number fields `…/value`), the quality dropdown `choice:option.export.quality` with its
//! items `choice:option.export.quality/<automatic|normal|maximum|minimum|custom>`, the watermark
//! text `field:option.export.watermarkText`.

use std::path::PathBuf;

use dalica_engine::options::{Kind, Tab};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{Form, TextField, dropdown, dropdown_item, form_group, hint, text_button, tooltip};
use egui::Ui;
use serde_json::{Map, Value, json};

use super::file_types::{Opt, check, number, rows, shown};
use super::general;
use crate::app::App;
use crate::dialogs::export::{STOPS, preset};
use crate::picker::FolderRequest;
use crate::state::Quality;
use crate::tl;

/// The tab's groups, in order.
pub const GROUPS: [&str; 4] = ["Location", "Size and quality", "Privacy", "Watermark"];

/// The keys the tab treats specially.
pub const LOCATION: &str = "export.location";
pub const RESIZE: &str = "export.resize";
pub const SIZE: &str = "export.size";
pub const QUALITY: &str = "export.quality";
pub const CUSTOM: &str = "export.customQuality";
pub const WATERMARK: &str = "export.watermarkText";

/// The command the location's folder picker runs with the folder chosen (`{key, path}`): it puts
/// the folder into the open dialog's draft.
pub const FOLDER_COMMAND: &str = "app.preferences.folder";
/// The title of the folder picker Browse… opens.
pub const BROWSE_TITLE: &str = "Default Export Location";
/// The automation id of the location's Browse….
pub const BROWSE_ID: &str = "button:option.export.location/browse";
/// The space kept for Browse… beside the location (points).
const BROWSE_W: f32 = 90.0;

/// The folder picker Browse… opens for folder option `key` showing `value`: it starts in that
/// folder, and the folder chosen runs [`FOLDER_COMMAND`] with `key` (over automation it is checked
/// as a place to write).
pub fn browse_request(key: &str, value: &Value) -> FolderRequest {
    let shown = value.as_str().map(PathBuf::from).filter(|p| p.is_absolute());
    FolderRequest { title: BROWSE_TITLE, folder: shown, command: FOLDER_COMMAND, params: json!({ "key": key }), output: true }
}

/// What the tab asks of the app this pass besides draft changes.
enum Ask {
    Browse(String, Value),
}

pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app, Tab::Export);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let t = *app.tokens();
    let can_browse = app.can_pick().is_ok();
    let height = ui.available_height();
    let mut out = (Vec::new(), None);
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical()
        .id_salt("dialog:options/export")
        .max_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| out = form(ui, &t, &opts, &draft, can_browse));
    let (changes, ask) = out;
    for (i, value) in changes {
        if let Some(o) = opts.get(i) {
            general::set(app, &o.row, value);
        }
    }
    if let Some(Ask::Browse(key, value)) = ask
        && let Err(why) = app.pick_folder_for(browse_request(&key, &value))
    {
        app.state_mut().status = why;
    }
}

/// Draws the groups; returns the values changed this pass, by option, and what to ask the app.
fn form(ui: &mut Ui, t: &Tokens, opts: &[Opt], draft: &Map<String, Value>, can_browse: bool) -> (Vec<(usize, Value)>, Option<Ask>) {
    let group_labels: Vec<String> = GROUPS.iter().map(|g| general::group_label(g)).collect();
    let row_labels: Vec<String> = opts.iter().filter(|o| o.row.kind != "bool").map(|o| general::form_label_of(&o.row)).collect();
    let labels: Vec<&str> = group_labels.iter().chain(&row_labels).map(String::as_str).collect();
    let form = Form::new(ui, t, &labels);
    let value_of = |key: &str| opts.iter().find(|o| o.row.key == key).map(|o| shown(&o.row, draft).clone()).unwrap_or(Value::Null);
    let resize = value_of(RESIZE).as_bool().unwrap_or(true);
    let custom = value_of(QUALITY).as_str() == Some(Quality::Custom.name());
    let mut changes = Vec::new();
    let mut ask = None;
    let mut first = true;
    for group in GROUPS {
        let in_group: Vec<(usize, &Opt)> = opts.iter().enumerate().filter(|(_, o)| o.row.group == group).collect();
        if in_group.is_empty() {
            continue;
        }
        if !first {
            ui.separator();
            ui.add_space(4.0);
        }
        first = false;
        let checks: Vec<&(usize, &Opt)> = in_group.iter().filter(|(_, o)| o.row.kind == "bool").collect();
        if !checks.is_empty() {
            form.row(ui, t, &general::group_label(group), |ui| {
                for &&(i, o) in &checks {
                    if let Some(v) = check(ui, t, &o.row, shown(&o.row, draft)) {
                        changes.push((i, v));
                    }
                }
            });
        }
        for &(i, o) in in_group.iter().filter(|(_, o)| o.row.kind != "bool") {
            let value = shown(&o.row, draft);
            let label = general::form_label_of(&o.row);
            let changed = match o.row.key.as_str() {
                SIZE => form_group(ui, resize, |ui| form.row(ui, t, &label, |ui| number(ui, t, o, value, &STOPS, true))),
                CUSTOM => form_group(ui, custom, |ui| form.row(ui, t, &label, |ui| number(ui, t, o, value, &[], true))),
                _ => form.row(ui, t, &label, |ui| match o.spec.kind {
                    Kind::Folder => {
                        if folder(ui, t, o, value, can_browse) {
                            ask = Some(Ask::Browse(o.row.key.clone(), value.clone()));
                        }
                        None
                    }
                    Kind::Text { max_bytes } => text(ui, t, o, value, max_bytes),
                    Kind::Choice(_) => quality(ui, t, o, value),
                    _ => number(ui, t, o, value, &[], true),
                }),
            };
            if let Some(v) = changed {
                changes.push((i, v));
            }
        }
    }
    (changes, ask)
}

/// A folder option: its path (the hint while it is the default folder), shown, not typed in, and
/// Browse…; returns whether Browse… was clicked.
fn folder(ui: &mut Ui, t: &Tokens, o: &Opt, value: &Value, can_browse: bool) -> bool {
    let mut path = value.as_str().unwrap_or_default().to_string();
    let placeholder = if o.row.hint.is_empty() { tl!("The default folder") } else { general::hint_of(&o.row) };
    let available = o.row.reason.is_none();
    ui.horizontal(|ui| {
        let w = (ui.available_width() - BROWSE_W - ui.spacing().item_spacing.x).max(60.0);
        let field = form_group(ui, false, |ui| TextField::new(&general::control_id("folder", &o.row.key), &placeholder, w).show(ui, t, &mut path));
        let browse =
            form_group(ui, available && can_browse, |ui| text_button(ui, t, &format!("button:option.{}/browse", o.row.key), &tl!("Browse…"), false));
        if let Some(reason) = general::reason_of(&o.row) {
            tooltip(field.response, t, &reason, None);
            return tooltip(browse, t, &reason, None).clicked();
        }
        browse.clicked()
    })
    .inner
}

/// A text option, typed in, kept to `max_bytes`; returns the new text when it changed.
fn text(ui: &mut Ui, t: &Tokens, o: &Opt, value: &Value, max_bytes: usize) -> Option<Value> {
    let before = value.as_str().unwrap_or_default().to_string();
    let mut typed = before.clone();
    let w = ui.available_width().max(60.0);
    form_group(ui, o.row.reason.is_none(), |ui| TextField::new(&general::control_id("folder", &o.row.key), "", w).show(ui, t, &mut typed));
    if !o.row.hint.is_empty() {
        hint(ui, t, &general::hint_of(&o.row));
    }
    let kept = cut(&typed, max_bytes);
    (kept != before).then(|| json!(kept))
}

/// At most `max` bytes of `text`, cut at a character boundary.
pub fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// The quality presets, with the Export dialog's descriptions; returns the preset picked.
fn quality(ui: &mut Ui, t: &Tokens, o: &Opt, value: &Value) -> Option<Value> {
    let id = general::control_id("choice", &o.row.key);
    let preset_of = |v: &Value| Quality::ALL.into_iter().find(|q| v.as_str() == Some(q.name()));
    let current = preset_of(value);
    let text = current.map_or_else(|| value.to_string(), |q| crate::dialogs::export::shown(preset(q).0));
    let mut picked = None;
    dropdown(ui, t, &id, &text, |ui| {
        for (v, label) in &o.row.choices {
            let description = preset_of(v).map_or_else(String::new, |q| crate::dialogs::export::shown(preset(q).1));
            if dropdown_item(ui, t, &general::item_id(&o.row.key, v), &general::choice_text(&o.row, v, label), &description, v == value).clicked() {
                picked = Some(v.clone());
            }
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_starts_in_the_folder_shown_and_names_the_option() {
        let r = browse_request(LOCATION, &Value::Null);
        assert_eq!((r.folder, r.command, r.output), (None, FOLDER_COMMAND, true));
        let r = browse_request(LOCATION, &json!("relative/place"));
        assert_eq!(r.folder, None, "only an absolute folder");
        let abs = std::env::temp_dir().join("Exports");
        let r = browse_request(LOCATION, &json!(abs.to_str().unwrap()));
        assert_eq!(r.folder, Some(abs.clone()));
        assert_eq!(r.params_with(&abs), json!({ "key": LOCATION, "path": abs.to_str().unwrap() }));
    }

    #[test]
    fn the_watermark_is_cut_at_a_character_boundary() {
        assert_eq!(cut("© Ana", 2), "©");
        assert_eq!(cut("© Ana", 1), "");
        assert_eq!(cut("abc", 10), "abc");
    }

    #[test]
    fn the_rows_are_the_catalogues_export_options() {
        let opts = rows(&App::new(None), Tab::Export);
        let keys: Vec<&str> = opts.iter().map(|o| o.row.key.as_str()).collect();
        assert_eq!(keys, [LOCATION, RESIZE, SIZE, QUALITY, CUSTOM, "export.removeLocation", WATERMARK]);
        assert!(opts.iter().all(|o| GROUPS.contains(&o.row.group.as_str())), "every option is in a group the tab draws");
    }
}
