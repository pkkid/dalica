//! Options ▸ People (`organizing.md` §6.1, `faces.md` §3–§5, `menus-and-shortcuts.md` §5 People),
//! on M8.5's form rows, its options from the engine's catalogue (`options.list {tab: "people"}`),
//! and below them the face models through `faces.models.*`:
//!
//! ```text
//!                  Faces:  [x] Find faces in my photos
//!                              Faces are found on this computer; nothing is sent anywhere.
//!                          [x] Suggest names
//!  Suggestion confidence:  [85] ──────o──
//!                          Higher values make fewer, more certain suggestions.
//!    Grouping confidence:  [85] ──────o──
//!                          Higher values make smaller, more certain groups of unnamed faces.
//!  ─────────────────────────────────────────────────────────
//!                  Files:  [ ] Store people names in photos
//!                              Writes the names of the people in a photo into its file …
//!  ─────────────────────────────────────────────────────────
//!             Face model:  YuNet (face detector)        Installed            [Remove]
//!                          SFace (face recogniser)      Installed · in use   [Remove]
//!                          AuraFace v1 (face recogniser)  261 MB  Not downloaded  [Download…]
//!                          [Add a Model File…]  or drop a .onnx file on the window
//! ```
//!
//! - The options go into the dialog's draft ([`super::general::set`]) and Apply or OK sends them in
//!   its one `options.set`; the confidence sliders move by 5 within 50 … 95 (a drag snaps to the
//!   steps, a typed number is rounded to one). Once applied, Find faces turned on starts the scan
//!   again and offers the face model dialog when no recogniser is installed; turned off it offers
//!   to delete the face data ([`applied`], `crate::commands::face_model::applied`).
//! - **Face model** is not an option: each model of `faces.models.list` with its role and state,
//!   and [Download…] (the face model dialog on it, `faceModel.open {entry}`), [Remove] (asks,
//!   `faceModel.remove {id}`) or [Use] (an installed recogniser not in use,
//!   `faces.models.select {id}`) at once; then [Add a Model File…] (`faceModel.addFile`).
//!
//! Automation ids: `check:option.people.findFaces`, `check:option.people.suggestNames`,
//! `check:option.people.storeNamesInPhotos`, the sliders `slider:option.people.suggestionConfidence`
//! and `slider:option.people.groupingConfidence` with their fields `…/value`; a model's row
//! `text:option.people.model/<id>` and its `button:option.people.model/<id>/download`,
//! `/remove` and `/use`; `button:option.people.addFile`; `text:option.people.models` when there is
//! nothing to list (no models folder, or still being read).

use dalica_engine::options::{Kind, Tab};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Form, SliderSpec, StopsSlider, Target, form_group, hint, stops_slider, text_button};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Layout, RichText, Ui};
use serde_json::{Map, Value, json};

use super::file_types::{self, Opt, TRACK_MAX, shown, slider_id};
use super::general;
use crate::app::App;
use crate::commands::face_model::{self as models, models_of, text};
use crate::dialogs::face_model::{display_name, size_text, state_text};

/// The settings keys of the tab.
pub const FIND_FACES: &str = "people.findFaces";
pub const SUGGEST_NAMES: &str = "people.suggestNames";
pub const SUGGESTION_CONFIDENCE: &str = "people.suggestionConfidence";
pub const GROUPING_CONFIDENCE: &str = "people.groupingConfidence";
pub const STORE_NAMES: &str = "people.storeNamesInPhotos";

/// The form's group labels.
const FACES: &str = "Faces:";
const FILES: &str = "Files:";
const MODEL: &str = "Face model:";

/// Add a Model File…'s id, and the line beside it.
pub const ADD_FILE_ID: &str = "button:option.people.addFile";
pub const DROP_HINT: &str = "or drop a .onnx file on the window";
/// The line shown instead of the models when there are none to list.
pub const MODELS_ID: &str = "text:option.people.models";
pub const NO_MODELS: &str = "Face models cannot be added here: this session has no models folder.";
pub const READING: &str = "Reading the models folder…";

/// The automation id of model `id`'s row.
pub fn model_id(id: &str) -> String {
    format!("text:option.people.model/{id}")
}

/// The automation id of model `id`'s button `action` (`download`, `remove`, `use`).
pub fn model_button(id: &str, action: &str) -> String {
    format!("button:option.people.model/{id}/{action}")
}

/// The tab's options with the values in effect.
pub fn rows(app: &App) -> Vec<Opt> {
    file_types::rows(app, Tab::People)
}

/// What a click on a model's row asked for.
enum Asked {
    Download(String),
    Remove(String),
    Use(String),
    AddFile,
}

/// Draws the tab's body; option changes go into the Options draft, the models' buttons act at once.
pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let list = models::list(app);
    let can_add = app.can_pick().is_ok();
    let t = *app.tokens();
    let height = ui.available_height();
    let mut out = (Vec::new(), None);
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical()
        .id_salt("dialog:options/people")
        .max_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| out = form(ui, &t, &opts, &draft, list.as_ref(), can_add));
    let (changes, asked) = out;
    for (i, value) in changes {
        if let Some(o) = opts.get(i) {
            general::set(app, &o.row, value);
        }
    }
    let result = match asked {
        Some(Asked::Download(id)) => app.run("faceModel.open", &json!({ "entry": id })),
        Some(Asked::Remove(id)) => app.run("faceModel.remove", &json!({ "id": id })),
        Some(Asked::Use(id)) => app.run("faces.models.select", &json!({ "id": id })),
        Some(Asked::AddFile) => app.run("faceModel.addFile", &json!({})),
        None => Ok(Value::Null),
    };
    if let Err(e) = result {
        app.state_mut().status = e.to_string();
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

/// Draws the form; returns the values changed this pass, by option, and what a model's button
/// asked for.
fn form(
    ui: &mut Ui,
    t: &Tokens,
    opts: &[Opt],
    draft: &Map<String, Value>,
    list: Option<&Value>,
    can_add: bool,
) -> (Vec<(usize, Value)>, Option<Asked>) {
    let labels = [FACES.to_string(), label(opts, SUGGESTION_CONFIDENCE), label(opts, GROUPING_CONFIDENCE), FILES.to_string(), MODEL.to_string()];
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
    form.row(ui, t, FACES, |ui| checks(ui, &[FIND_FACES, SUGGEST_NAMES], &mut changes));
    let suggesting = shown_bool(opts, draft, SUGGEST_NAMES);
    for (key, enabled) in [(SUGGESTION_CONFIDENCE, suggesting), (GROUPING_CONFIDENCE, true)] {
        if let Some((i, o)) = find(opts, key) {
            form.row(ui, t, &label(opts, key), |ui| {
                if let Some(v) = confidence(ui, t, o, shown(&o.row, draft), enabled) {
                    changes.push((i, v));
                }
            });
        }
    }
    ui.separator();
    ui.add_space(4.0);
    form.row(ui, t, FILES, |ui| checks(ui, &[STORE_NAMES], &mut changes));
    ui.separator();
    ui.add_space(4.0);
    let asked = form.row(ui, t, MODEL, |ui| model_rows(ui, t, list, can_add));
    (changes, asked)
}

/// The on/off option `key` as shown (the draft's value, else the saved one).
fn shown_bool(opts: &[Opt], draft: &Map<String, Value>, key: &str) -> bool {
    find(opts, key).is_some_and(|(_, o)| shown(&o.row, draft).as_bool().unwrap_or(false))
}

/// The steps a confidence slider stops at: its range by its step (50, 55, … 95).
pub fn confidence_stops(min: u32, max: u32, step: u32) -> Vec<f64> {
    (min..=max).step_by(usize::try_from(step.max(1)).unwrap_or(1)).take(64).map(f64::from).collect()
}

/// A confidence option as M8.5's slider with its number field, moving by the option's step (5)
/// within its range (50 … 95): a drag snaps to the steps, the arrow keys move one step, a typed
/// number is rounded to one. Returns the new value when it changed.
fn confidence(ui: &mut Ui, t: &Tokens, opt: &Opt, value: &Value, enabled: bool) -> Option<Value> {
    let Kind::Int { min, max, step, unit } = opt.spec.kind else { return None };
    let id = slider_id(&opt.row.key);
    let default = opt.spec.default_value().as_f64().unwrap_or(f64::from(max));
    let stops = confidence_stops(min, max, step);
    let spec = SliderSpec {
        id: &id,
        label: &opt.row.label,
        min: f64::from(min),
        max: f64::from(max),
        default,
        step: f64::from(step.max(1)),
        decimals: 0,
        unit,
    };
    let current = value.as_f64().unwrap_or(default);
    let s = StopsSlider { spec, stops: &stops, field: true, track_w: ui.available_width().min(TRACK_MAX) };
    let enabled = enabled && opt.row.reason.is_none();
    let out = form_group(ui, enabled, |ui| stops_slider(ui, t, &s, current));
    if !opt.row.hint.is_empty() {
        form_group(ui, enabled, |ui| hint(ui, t, &opt.row.hint));
    }
    let v = out.value.filter(|v| v.is_finite())?;
    let step = f64::from(step.max(1));
    let snapped = ((v / step).round() * step).clamp(f64::from(min), f64::from(max));
    // In range after the clamp, so the cast is exact.
    let snapped = json!(snapped as u32);
    (snapped != *value).then_some(snapped)
}

/// The face models: one row each with its buttons, then Add a Model File…; returns what a click
/// asked for.
fn model_rows(ui: &mut Ui, t: &Tokens, list: Option<&Value>, can_add: bool) -> Option<Asked> {
    let mut asked = None;
    let folder = list.and_then(|l| l.get("folder")).is_some_and(|f| !f.is_null());
    let problem = list.and_then(|l| l.get("problem")).and_then(Value::as_str);
    let line = |ui: &mut Ui, s: &str| {
        let r = ui.add(egui::Label::new(RichText::new(s).font(fonts::regular(t.body)).color(t.text_dim)).wrap());
        widgets::register(ui.ctx(), MODELS_ID, Role::Other, s, Target::Painted { rect: r.rect, clip: ui.clip_rect() }, false);
    };
    match list {
        None => line(ui, READING),
        Some(_) if !folder => line(ui, problem.unwrap_or(NO_MODELS)),
        Some(l) => {
            for m in models_of(l) {
                if let Some(a) = model_row(ui, t, m, problem.is_none()) {
                    asked = Some(a);
                }
            }
            if let Some(p) = problem {
                ui.add(egui::Label::new(RichText::new(p).font(fonts::regular(t.small)).color(t.caution)).wrap());
            }
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let live = can_add && folder && problem.is_none();
        if form_group(ui, live, |ui| text_button(ui, t, ADD_FILE_ID, "Add a Model File…", false)).clicked() {
            asked = Some(Asked::AddFile);
        }
        ui.label(RichText::new(DROP_HINT).font(fonts::regular(t.small)).color(t.text_dim));
    });
    asked
}

/// One model: its name and role, its state, and its buttons (enabled when the models folder can
/// change). Returns what a click asked for.
fn model_row(ui: &mut Ui, t: &Tokens, m: &Value, live: bool) -> Option<Asked> {
    let id = text(m, "id").to_string();
    if id.is_empty() {
        return None;
    }
    let mut asked = None;
    let state = text(m, "state");
    let caution = m.get("caution") == Some(&Value::Bool(true));
    let r = ui.horizontal(|ui| {
        let name = display_name(m, &id);
        let size = size_text(m.get("sizeBytes").and_then(Value::as_u64));
        ui.label(RichText::new(name).font(fonts::regular(t.body)).color(if caution { t.caution } else { t.text }));
        if state != "installed" && !size.is_empty() {
            ui.label(RichText::new(size).font(fonts::regular(t.small)).color(t.text_dim));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            form_group(ui, live, |ui| match state {
                "installed" => {
                    if text_button(ui, t, &model_button(&id, "remove"), "Remove", false).clicked() {
                        asked = Some(Asked::Remove(id.clone()));
                    }
                    let recogniser = text(m, "role") == "recogniser";
                    if recogniser
                        && m.get("inUse") != Some(&Value::Bool(true))
                        && text_button(ui, t, &model_button(&id, "use"), "Use", false).clicked()
                    {
                        asked = Some(Asked::Use(id.clone()));
                    }
                }
                _ => {
                    let downloadable = text(m, "origin") != "local";
                    let r = form_group(ui, downloadable, |ui| text_button(ui, t, &model_button(&id, "download"), "Download…", false));
                    if r.clicked() {
                        asked = Some(Asked::Download(id.clone()));
                    }
                }
            });
            ui.label(RichText::new(state_text(m)).font(fonts::regular(t.small)).color(t.text_label));
        });
    });
    let label = format!("{} · {}", text(m, "name"), state_text(m));
    widgets::register(ui.ctx(), &model_id(&id), Role::Other, &label, Target::Painted { rect: r.response.rect, clip: ui.clip_rect() }, false);
    asked
}

/// What the tab does once Apply or OK applied the draft `values` (`ok`: the apply succeeded):
/// `crate::commands::face_model::applied`.
pub fn applied(app: &mut App, values: &Map<String, Value>, ok: bool) {
    models::applied(app, values, ok);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_confidence_steps_are_its_range_by_five() {
        assert_eq!(confidence_stops(50, 95, 5), [50.0, 55.0, 60.0, 65.0, 70.0, 75.0, 80.0, 85.0, 90.0, 95.0]);
        assert_eq!(confidence_stops(1, 3, 0), [1.0, 2.0, 3.0], "no step: by one");
        assert!(confidence_stops(0, u32::MAX, 1).len() <= 64, "bounded");
    }
}
