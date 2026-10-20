//! Options ▸ Slideshow (`viewing.md` §1.5, `menus-and-shortcuts.md` §5 Slideshow), on M8.5's form
//! rows, its options from the engine's catalogue (`options.list {tab: "slideshow"}`):
//!
//! ```text
//!     Slideshow:  [ ] Loop slideshow
//!                 [x] Show captions
//!  Display time:  [ 3] s ──o──────────
//!    Transition:  [Dissolve ▾]
//!  ───────────────────────────────────────────────────────────
//!         Sound:  [ ] Play music during slideshows
//!  Music folder:  [No folder             ] [Browse…]
//!                 Every MP3, OGG, FLAC and WAV file in it plays in name order, looping.
//!                 [ ] Play video sound
//! ```
//!
//! Every row is live: a change goes into the draft ([`super::general::set`]) and the dialog's Apply
//! or OK sends it in its one `options.set`; the show playing and the slideshow bar read the same
//! settings (the bar's Captions, Transition and Display time change them too, through their
//! commands). The Sound group (M15.27): Play music, the music folder (chosen with Browse…, the
//! folder picker off the UI thread, starting in the folder shown; its answer goes into the draft
//! through `app.preferences.folder`) and Play video sound; on a system without a sound output
//! ([`dalica_engine::sound::Sound::status`]) the three are drawn disabled with that reason ("No
//! sound output on this system") as their tooltip ([`sound_reason`]).
//!
//! Automation ids (`control-protocol.md` §7): `check:option.slideshow.loop`,
//! `check:option.slideshow.captions`, the display time's slider `slider:option.slideshow.displayTime`
//! and its field `slider:option.slideshow.displayTime/value`, the transition's dropdown
//! `choice:option.slideshow.transition` and its items `choice:option.slideshow.transition/<id>`,
//! and the Sound group's `check:option.slideshow.music`, `field:option.slideshow.musicFolder` with
//! `button:option.slideshow.musicFolder/browse`, `check:option.slideshow.videoSound`.

use dalica_engine::options::Tab;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{Form, TextField, dropdown, dropdown_item, form_group, hint, text_button, tooltip};
use egui::Ui;
use serde_json::{Map, Value};

use super::file_types::{self, Opt, shown};
use super::general::{self, FOLDER_COMMAND, Row};
use crate::app::App;
use crate::picker::FolderRequest;
use crate::tl;

/// The settings keys of the tab.
pub const LOOP: &str = "slideshow.loop";
pub const CAPTIONS: &str = "slideshow.captions";
pub const DISPLAY_TIME: &str = "slideshow.displayTime";
pub const TRANSITION: &str = "slideshow.transition";
pub const MUSIC: &str = "slideshow.music";
pub const MUSIC_FOLDER: &str = "slideshow.musicFolder";
pub const VIDEO_SOUND: &str = "slideshow.videoSound";

/// The display time's slider stops (seconds).
const DISPLAY_STOPS: [f64; 6] = [1.0, 3.0, 5.0, 10.0, 30.0, 60.0];
/// The options of the Sound group.
pub const SOUND_KEYS: [&str; 3] = [MUSIC, MUSIC_FOLDER, VIDEO_SOUND];
/// The title of the music folder's picker.
pub const MUSIC_TITLE: &str = "Music Folder";

/// Why the Sound group is greyed: the session's sound output has none on this system (`None`
/// while it plays, or no library is open).
pub fn sound_reason(app: &App) -> Option<String> {
    app.session().and_then(|s| s.sound().status().err())
}

/// The tab's options with the values in effect; on a system without a sound output the Sound
/// group carries why.
pub fn rows(app: &App) -> Vec<Opt> {
    let mut opts = file_types::rows(app, Tab::Slideshow);
    if let Some(why) = sound_reason(app) {
        for o in opts.iter_mut().filter(|o| SOUND_KEYS.contains(&o.row.key.as_str())) {
            o.row.reason.get_or_insert_with(|| why.clone());
        }
    }
    opts
}

/// The music folder's picker, starting in the folder `value` names: the folder chosen goes into
/// the draft (a folder to read music from).
pub fn music_request(value: &Value) -> FolderRequest {
    let folder = value.as_str().map(std::path::PathBuf::from).filter(|p| p.is_absolute());
    FolderRequest { title: MUSIC_TITLE, folder, command: FOLDER_COMMAND, params: serde_json::json!({ "key": MUSIC_FOLDER }), output: false }
}

/// Draws the tab's body; changes go into the Options draft.
pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let t = *app.tokens();
    let can_browse = app.can_pick().is_ok();
    let (changes, browse) = form(ui, &t, &opts, &draft, can_browse);
    for (i, value) in changes {
        if let Some(o) = opts.get(i) {
            general::set(app, &o.row, value);
        }
    }
    if browse
        && let Some((_, o)) = find(&opts, MUSIC_FOLDER)
        && let Err(why) = app.pick_folder_for(music_request(shown(&o.row, &draft)))
    {
        app.state_mut().status = why;
    }
}

/// The option `key` and its place in `opts`.
fn find<'a>(opts: &'a [Opt], key: &str) -> Option<(usize, &'a Opt)> {
    opts.iter().enumerate().find(|(_, o)| o.row.key == key)
}

/// The label of the form row of option `key` (its name and a colon).
fn label(opts: &[Opt], key: &str) -> String {
    find(opts, key).map_or_else(String::new, |(_, o)| general::form_label_of(&o.row))
}

/// Draws the form; returns the values changed this pass, by option, and whether the music
/// folder's Browse… was clicked.
fn form(ui: &mut Ui, t: &Tokens, opts: &[Opt], draft: &Map<String, Value>, can_browse: bool) -> (Vec<(usize, Value)>, bool) {
    let (slideshow, sound) =
        (general::form_label(&tl!(ctx = "options group", "Slideshow")), general::form_label(&tl!(ctx = "options group", "Sound")));
    let labels = [slideshow.clone(), label(opts, DISPLAY_TIME), label(opts, TRANSITION), sound.clone(), label(opts, MUSIC_FOLDER)];
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
    form.row(ui, t, &slideshow, |ui| checks(ui, &[LOOP, CAPTIONS], &mut changes));
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
    form.row(ui, t, &sound, |ui| checks(ui, &[MUSIC], &mut changes));
    let mut browse = false;
    if let Some((_, o)) = find(opts, MUSIC_FOLDER) {
        form.row(ui, t, &label(opts, MUSIC_FOLDER), |ui| browse = folder(ui, t, &o.row, shown(&o.row, draft), can_browse));
    }
    form.row(ui, t, "", |ui| checks(ui, &[VIDEO_SOUND], &mut changes));
    (changes, browse)
}

/// A dropdown of `row`'s choices showing `value`; returns the choice picked this pass.
fn choice(ui: &mut Ui, t: &Tokens, row: &Row, value: &Value) -> Option<Value> {
    let text = row.choices.iter().find(|(v, _)| v == value).map_or_else(|| value.to_string(), |(v, l)| general::choice_text(row, v, l));
    let mut picked = None;
    let r = dropdown(ui, t, &general::control_id(&row.kind, &row.key), &text, |ui| {
        for (v, label) in &row.choices {
            if dropdown_item(ui, t, &general::item_id(&row.key, v), &general::choice_text(row, v, label), "", v == value).clicked() {
                picked = Some(v.clone());
            }
        }
    });
    if let Some(reason) = general::reason_of(row) {
        tooltip(r, t, &reason, None);
    }
    picked.filter(|p| p != value)
}

/// The music folder: its path (shown, not typed in) and Browse…, drawn disabled with the option's
/// reason on a system without a sound output; the hint under them. Answers whether Browse… was
/// clicked.
fn folder(ui: &mut Ui, t: &Tokens, row: &Row, value: &Value, can_browse: bool) -> bool {
    let mut path = value.as_str().unwrap_or_default().to_string();
    let live = row.reason.is_none();
    let clicked = ui
        .horizontal(|ui| {
            let browse_w = 90.0;
            let w = (ui.available_width() - browse_w - ui.spacing().item_spacing.x).max(60.0);
            let field = form_group(ui, false, |ui| {
                TextField::new(&general::control_id("folder", &row.key), &crate::tl!("No folder"), w).show(ui, t, &mut path)
            });
            let browse = form_group(ui, live && can_browse, |ui| {
                text_button(ui, t, &format!("button:option.{}/browse", row.key), &crate::tl!("Browse…"), false)
            });
            if let Some(reason) = general::reason_of(row) {
                tooltip(field.response, t, &reason, None);
                tooltip(browse, t, &reason, None);
                return false;
            }
            browse.clicked()
        })
        .inner;
    if !row.hint.is_empty() {
        form_group(ui, live, |ui| hint(ui, t, &general::hint_of(row)));
    }
    clicked
}
