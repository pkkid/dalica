//! Options ▸ Slideshow (`viewing.md` §1.5, `menus-and-shortcuts.md` §5 Slideshow), on M8.5's form
//! rows, its options from the engine's catalogue (`options.list {tab: "slideshow"}`):
//!
//! ```text
//!     Slideshow:  [ ] Loop slideshow
//!                 [x] Show captions
//!  Display time:  [ 3] s ──o──────────
//!    Transition:  [Dissolve ▾]
//!  ───────────────────────────────────────────────────────────
//!         Sound:  [ ] Play music during slideshows        (disabled: Arrives with sound support)
//!  Music folder:  [The default folder    ] [Browse…]       (disabled)
//!                 Every MP3, OGG, FLAC and WAV file in it plays in name order, looping.
//!                 [ ] Play video sound                    (disabled)
//! ```
//!
//! Loop, Show captions, Display time and Transition are live: a change goes into the draft
//! ([`super::general::set`]) and the dialog's Apply or OK sends it in its one `options.set`; the
//! show playing and the slideshow bar read the same settings (the bar's Captions, Transition and
//! Display time change them too, through their commands). The options that need sound (music, its
//! folder, video sound) are drawn disabled with the catalogue's reason as their tooltip, grouped
//! under Sound.
//!
//! Automation ids (`control-protocol.md` §7): `check:option.slideshow.loop`,
//! `check:option.slideshow.captions`, the display time's slider `slider:option.slideshow.displayTime`
//! and its field `slider:option.slideshow.displayTime/value`, the transition's dropdown
//! `choice:option.slideshow.transition` and its items `choice:option.slideshow.transition/<id>`,
//! and the disabled `check:option.slideshow.music`, `field:option.slideshow.musicFolder` with
//! `button:option.slideshow.musicFolder/browse`, `check:option.slideshow.videoSound`.

use dalica_engine::options::Tab;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{Form, TextField, dropdown, dropdown_item, form_group, hint, text_button, tooltip};
use egui::Ui;
use serde_json::{Map, Value};

use super::file_types::{self, Opt, shown};
use super::general::{self, Row};
use crate::app::App;

/// The settings keys of the tab.
pub const LOOP: &str = "slideshow.loop";
pub const CAPTIONS: &str = "slideshow.captions";
pub const DISPLAY_TIME: &str = "slideshow.displayTime";
pub const TRANSITION: &str = "slideshow.transition";
pub const MUSIC: &str = "slideshow.music";
pub const MUSIC_FOLDER: &str = "slideshow.musicFolder";
pub const VIDEO_SOUND: &str = "slideshow.videoSound";

/// The form's labels.
const SLIDESHOW: &str = "Slideshow:";
const SOUND: &str = "Sound:";
/// The display time's slider stops (seconds).
const DISPLAY_STOPS: [f64; 6] = [1.0, 3.0, 5.0, 10.0, 30.0, 60.0];

/// The tab's options with the values in effect.
pub fn rows(app: &App) -> Vec<Opt> {
    file_types::rows(app, Tab::Slideshow)
}

/// Draws the tab's body; changes go into the Options draft.
pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let t = *app.tokens();
    let changes = form(ui, &t, &opts, &draft);
    for (i, value) in changes {
        if let Some(o) = opts.get(i) {
            general::set(app, &o.row, value);
        }
    }
}

/// The option `key` and its place in `opts`.
fn find<'a>(opts: &'a [Opt], key: &str) -> Option<(usize, &'a Opt)> {
    opts.iter().enumerate().find(|(_, o)| o.row.key == key)
}

/// The label of the form row of option `key` (its name and a colon).
fn label(opts: &[Opt], key: &str) -> String {
    find(opts, key).map_or_else(String::new, |(_, o)| format!("{}:", o.row.label))
}

/// Draws the form; returns the values changed this pass, by option.
fn form(ui: &mut Ui, t: &Tokens, opts: &[Opt], draft: &Map<String, Value>) -> Vec<(usize, Value)> {
    let labels = [SLIDESHOW.to_string(), label(opts, DISPLAY_TIME), label(opts, TRANSITION), SOUND.to_string(), label(opts, MUSIC_FOLDER)];
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let form = Form::new(ui, t, &labels);
    let mut changes = Vec::new();
    let checks = |ui: &mut Ui, keys: &[&str], changes: &mut Vec<(usize, Value)>| {
        for key in keys {
            if let Some((i, o)) = find(opts, key)
                && let Some(v) = file_types::check(ui, t, &o.row, shown(&o.row, draft))
            {
                changes.push((i, v));
            }
        }
    };
    form.row(ui, t, SLIDESHOW, |ui| checks(ui, &[LOOP, CAPTIONS], &mut changes));
    if let Some((i, o)) = find(opts, DISPLAY_TIME) {
        form.row(ui, t, &label(opts, DISPLAY_TIME), |ui| {
            if let Some(v) = file_types::number(ui, t, o, shown(&o.row, draft), &DISPLAY_STOPS, true) {
                changes.push((i, v));
            }
        });
    }
    if let Some((i, o)) = find(opts, TRANSITION) {
        form_group(ui, o.row.reason.is_none(), |ui| {
            form.row(ui, t, &label(opts, TRANSITION), |ui| {
                if let Some(v) = choice(ui, t, &o.row, shown(&o.row, draft)) {
                    changes.push((i, v));
                }
            });
        });
    }
    ui.separator();
    ui.add_space(4.0);
    form.row(ui, t, SOUND, |ui| checks(ui, &[MUSIC], &mut changes));
    if let Some((_, o)) = find(opts, MUSIC_FOLDER) {
        form.row(ui, t, &label(opts, MUSIC_FOLDER), |ui| folder(ui, t, &o.row, shown(&o.row, draft)));
    }
    form.row(ui, t, "", |ui| checks(ui, &[VIDEO_SOUND], &mut changes));
    changes
}

/// A dropdown of `row`'s choices showing `value`; returns the choice picked this pass.
fn choice(ui: &mut Ui, t: &Tokens, row: &Row, value: &Value) -> Option<Value> {
    let text = row.choices.iter().find(|(v, _)| v == value).map_or_else(|| value.to_string(), |(_, l)| l.clone());
    let mut picked = None;
    let r = dropdown(ui, t, &general::control_id(&row.kind, &row.key), &text, |ui| {
        for (v, label) in &row.choices {
            if dropdown_item(ui, t, &general::item_id(&row.key, v), label, "", v == value).clicked() {
                picked = Some(v.clone());
            }
        }
    });
    if let Some(reason) = &row.reason {
        tooltip(r, t, reason, None);
    }
    picked.filter(|p| p != value)
}

/// The music folder: its path and Browse…, drawn disabled with the option's reason while Dalica
/// plays no sound; the hint under them.
fn folder(ui: &mut Ui, t: &Tokens, row: &Row, value: &Value) {
    let mut path = value.as_str().unwrap_or_default().to_string();
    let live = row.reason.is_none();
    ui.horizontal(|ui| {
        let browse_w = 90.0;
        let w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(60.0);
        let field = form_group(ui, false, |ui| TextField::new(&general::control_id("folder", &row.key), "No folder", w).show(ui, t, &mut path));
        let browse = form_group(ui, live, |ui| text_button(ui, t, &format!("button:option.{}/browse", row.key), "Browse…", false));
        if let Some(reason) = &row.reason {
            tooltip(field.response, t, reason, None);
            tooltip(browse, t, reason, None);
        }
    });
    if !row.hint.is_empty() {
        form_group(ui, live, |ui| hint(ui, t, &row.hint));
    }
}
