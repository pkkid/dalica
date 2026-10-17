//! Options ▸ Photo Viewer (`viewing.md` §3.3, `menus-and-shortcuts.md` §5 Photo Viewer), on M8.5's
//! form rows, its options from the engine's catalogue (`options.list {tab: "viewer"}`):
//!
//! ```text
//!  File Types:  ( ) Use Dalica as the default viewer for these file types:
//!               [ ] JPEG  [ ] PNG  [ ] GIF  [ ] BMP  [ ] TIFF  [ ] WebP  [ ] HEIC  [ ] RAW
//!               [Current] [Select All] [Select None]
//!               (•) Don't use Dalica as a viewer           (all disabled: Arrives with OS integration)
//!  ───────────────────────────────────────────────────────────
//!   Interface:  (•) Mouse wheel zooms (hold Ctrl to navigate)
//!               ( ) Mouse wheel moves through photos (hold Ctrl to zoom)
//!               [x] Hide controls automatically
//!               [x] Translucent background
//! ```
//!
//! Registering Dalica as the default viewer needs the OS integration, so the File Types part is
//! drawn disabled with the catalogue's reason as its tooltip. The Interface options are live: a
//! change goes into the draft ([`super::general::set`]) and the dialog's Apply or OK sends it in
//! its one `options.set`; the viewer reads them every frame. The viewer's own Settings dialog
//! (`crate::dialogs::viewer_settings`) draws the same two parts ([`file_types`], [`interface`]).
//!
//! Automation ids (`control-protocol.md` §7): the wheel's radios
//! `choice:option.viewer.wheel/zoom` and `choice:option.viewer.wheel/navigate`, the checkboxes
//! `check:option.viewer.hideControls` and `check:option.viewer.translucent`; the disabled File
//! Types part `choice:option.viewer.defaultViewer/on` and `/off`, a type
//! `check:option.viewer.fileType/<type>` and the buttons `button:option.viewer.defaultViewer/<name>`
//! (`current`, `selectAll`, `selectNone`).

use dalica_engine::options::Tab;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{Form, checkbox, form_group, radio, text_button, tooltip};
use egui::Ui;
use serde_json::{Map, Value};

use super::file_types::{self, Opt, shown};
use super::general;
use crate::app::App;

/// The tab's groups, in order.
pub const GROUPS: [&str; 2] = ["File Types", "Interface"];
/// The settings keys of the tab.
pub const DEFAULT_VIEWER: &str = "viewer.defaultViewer";
pub const WHEEL: &str = "viewer.wheel";
/// The file types the File Types part lists.
pub const TYPES: [&str; 8] = ["JPEG", "PNG", "GIF", "BMP", "TIFF", "WebP", "HEIC", "RAW"];
/// The File Types part's radios.
pub const USE_DALICA: &str = "Use Dalica as the default viewer for these file types:";
pub const DONT_USE: &str = "Don't use Dalica as a viewer";
/// Why the File Types part is greyed when the catalogue gives no reason.
pub const LATER: &str = "Arrives with OS integration";

/// The tab's options with the values in effect.
pub fn rows(app: &App) -> Vec<Opt> {
    file_types::rows(app, Tab::PhotoViewer)
}

/// Draws the tab's body: both parts; changes go into the Options draft.
pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let t = *app.tokens();
    let labels: Vec<String> = GROUPS.iter().map(|g| format!("{g}:")).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let form = Form::new(ui, &t, &labels);
    file_types(ui, &t, &form, &opts);
    ui.separator();
    ui.add_space(4.0);
    for (i, value) in interface(ui, &t, &form, &opts, &draft) {
        if let Some(o) = opts.get(i) {
            general::set(app, &o.row, value);
        }
    }
}

/// The File Types part, drawn disabled with the catalogue's reason.
pub fn file_types(ui: &mut Ui, t: &Tokens, form: &Form, opts: &[Opt]) {
    let reason = opts.iter().find(|o| o.row.key == DEFAULT_VIEWER).and_then(|o| o.row.reason.clone()).unwrap_or_else(|| LATER.to_string());
    let on = format!("choice:option.{DEFAULT_VIEWER}/on");
    let off = format!("choice:option.{DEFAULT_VIEWER}/off");
    form_group(ui, false, |ui| {
        form.row(ui, t, &format!("{}:", GROUPS[0]), |ui| {
            tooltip(radio(ui, t, &on, USE_DALICA, false), t, &reason, None);
            ui.horizontal_wrapped(|ui| {
                for ty in TYPES {
                    let mut checked = false;
                    tooltip(checkbox(ui, t, &format!("check:option.viewer.fileType/{ty}"), ty, &mut checked), t, &reason, None);
                }
            });
            ui.horizontal(|ui| {
                for (name, label) in [("current", "Current"), ("selectAll", "Select All"), ("selectNone", "Select None")] {
                    tooltip(text_button(ui, t, &format!("button:option.{DEFAULT_VIEWER}/{name}"), label, false), t, &reason, None);
                }
            });
            tooltip(radio(ui, t, &off, DONT_USE, true), t, &reason, None);
        });
    });
}

/// The Interface part: the wheel's two radios and the two checkboxes; returns the values changed
/// this pass, by option.
pub fn interface(ui: &mut Ui, t: &Tokens, form: &Form, opts: &[Opt], draft: &Map<String, Value>) -> Vec<(usize, Value)> {
    let mut changes = Vec::new();
    form.row(ui, t, &format!("{}:", GROUPS[1]), |ui| {
        for (i, o) in opts.iter().enumerate().filter(|(_, o)| o.row.group == GROUPS[1]) {
            let value = shown(&o.row, draft);
            match o.row.kind.as_str() {
                "bool" => {
                    if let Some(v) = file_types::check(ui, t, &o.row, value) {
                        changes.push((i, v));
                    }
                }
                _ => {
                    for (choice, label) in &o.row.choices {
                        let r =
                            form_group(ui, o.row.reason.is_none(), |ui| radio(ui, t, &general::item_id(&o.row.key, choice), label, value == choice));
                        if r.clicked() && value != choice {
                            changes.push((i, choice.clone()));
                        }
                    }
                }
            }
        }
    });
    changes
}
