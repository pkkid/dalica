//! The face model dialog (`faces.md` §3.1): for each model file its licence, whether commercial use
//! is allowed, what it was trained on (or that this is not known), its size and the site it comes
//! from; **Download** stays disabled until "I have read these terms and accept them for my own use"
//! is ticked, then shows the download's progress with its stop button; **Open page** asks the
//! window to open the model's page and puts its address on the clipboard; **Add a Model File…**
//! (an `.onnx` dropped on the window comes here too) shows a local file with the same terms before
//! it is installed. Offered by Options ▸ People's Download…, by turning on Find faces in my photos
//! without a model and by the first open of People (`crate::commands::face_model`).
//!
//! ```text
//! Face Model
//!   To find faces, Dalica needs these model files. …
//!   ┌──────────────────────────────────────────────────────┐
//!   │ YuNet (face detector)  version 2023mar · 232 KB  Not downloaded │
//!   │ Licence: MIT · commercial use allowed                │
//!   │ Trained on WIDER FACE, …                             │
//!   │ From github.com                          [Open page] │
//!   │ ──────────────────────────────────────────────────── │
//!   │ SFace (face recogniser) …                            │
//!   └──────────────────────────────────────────────────────┘
//!   Commercial use describes the weights' licence only; …
//!   [x] I have read these terms and accept them for my own use
//!   Nothing is downloaded until you accept. …
//!   Downloading SFace (face recogniser): 12.3 of 38.7 MB ━━━━──── ×
//!                          [Add a Model File…]  [Download] [Cancel]
//! ```
//!
//! Its draft is the view state's `faceModel` ([`FaceModelDraft`]: the model offered or the local
//! file, the box ticked, the progress), and the dialog's fields: [`MODELS`] (the files it offers,
//! fixed when it opens), [`QUEUE`] (Download was pressed: the files download one after another),
//! [`JOB`] (the download running or last run), [`ERROR`] (why the last download failed) and, for a
//! local file, [`INSPECT`] (the `faces.models.inspect` job that says what it is) and [`FILE`] (the
//! file it opened on, which `ui.set` cannot change as it can the draft).
//!
//! - **Download** (`button:dialog.download`) runs `faces.models.download {id, acknowledged: true}`
//!   for the first file not installed yet, and the dialog stays open showing the progress
//!   (`progress:dialog.download`, its × `button:dialog.stop` runs `faces.models.cancel`); the
//!   next file follows when one ends well. Cancel reads Close while a download runs (it goes on
//!   in the activity popover) and once every file is installed.
//! - **Add Model** (`button:dialog.install`, a local file) runs `faces.models.install {path,
//!   acknowledged: true}` once the file was looked at and can be used, and closes.
//! - Under a control request, Download, Add Model and opening on a local file run only with
//!   `--automation-allow-external` (the engine's own rule for `faces.models.*`). Add Model then
//!   installs only the file the dialog opened on (checked against the automation read roots, its
//!   links resolved, when automation opened it: `control-protocol.md` §5.7), and only while that
//!   file lies, as written, inside a read root or a watched folder (or in `--demo`): a draft
//!   changed by `ui.set` after the dialog opened reads nothing ([`automation_may_install`]).
//! - A model whose licence does not plainly allow commercial use, or that is not built in (a
//!   `catalog.json` model, a local file), shows its licence and notice in `caution`.
//!
//! Ids: the rows `text:dialog.model/<id>`, their `button:dialog.page/<id>`, the box
//! `check:dialog.accept`, the buttons `button:dialog.addFile`, `button:dialog.download`,
//! `button:dialog.install`, `button:dialog.cancel`.

use dalica_engine::CmdError;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, Target, checkbox_row, progress_row, text_button};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Color32, Layout, RichText, Ui, vec2};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::commands::face_model::{self as models, model, starter, text};
use crate::state::FaceModelDraft;

pub const TITLE: &str = "Face Model";
/// The licence box's text.
pub const ACCEPT: &str = "I have read these terms and accept them for my own use";
/// The licence box's id.
pub const ACCEPT_ID: &str = "check:dialog.accept";
/// The download's progress row and its stop button.
pub const PROGRESS_ID: &str = "progress:dialog.download";
pub const STOP_ID: &str = "button:dialog.stop";
/// The dialog's width (points).
pub const WIDTH: f32 = 560.0;
/// Why Download and Add Model wait.
pub const NEEDS_ACCEPT: &str = "Tick \"I have read these terms and accept them for my own use\" first.";

/// The dialog's fields.
pub const MODELS: &str = "models";
pub const QUEUE: &str = "queue";
pub const JOB: &str = "job";
pub const ERROR: &str = "error";
pub const INSPECT: &str = "inspect";
/// The local file the dialog opened on.
pub const FILE: &str = "file";

/// What the dialog says above the files.
pub const INTRO: &str = "To find faces, Dalica needs these model files. Each is downloaded once from its own site and stays on this computer.";
pub const INTRO_ONE: &str = "Read this face model's terms before it is downloaded. It stays on this computer.";
pub const INTRO_FILE: &str = "Read this face model's terms before it is added. The file is copied into Dalica's models folder.";
/// What it says under the box.
pub const NOTHING_YET: &str = "Nothing is downloaded until you accept. Your photos and faces never leave this computer.";

/// The automation id of model `id`'s row.
pub fn row_id(id: &str) -> String {
    format!("text:dialog.model/{id}")
}

/// The automation id of model `id`'s Open page.
pub fn page_id(id: &str) -> String {
    format!("button:dialog.page/{id}")
}

/// Opens the dialog with `draft`: on a local file it starts looking at the file, else it offers
/// the model named or the files finding faces needs. Replies `{dialog: "faceModel"}`.
pub fn open(app: &mut App, draft: FaceModelDraft) -> Result<Value, CmdError> {
    super::can_open(app, DialogKind::FaceModel).map_err(CmdError::Disabled)?;
    let mut dialog = Dialog::new(DialogKind::FaceModel);
    if let Some(path) = &draft.file {
        models::external_allowed()?;
        if let Some(file) = path.to_str() {
            dialog.fields.insert(FILE.into(), json!(file));
        }
        let session = app.session_mut().ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
        match session.execute("faces.models.inspect", &json!({ "path": path })) {
            Ok(v) => dialog.fields.insert(INSPECT.into(), v.get("job").cloned().unwrap_or(Value::Null)),
            Err(e) => dialog.fields.insert(ERROR.into(), json!(e.to_string())),
        };
    } else {
        let ids = match &draft.entry {
            Some(id) => vec![id.clone()],
            None => models::list(app).map(|l| starter(&l)).unwrap_or_default(),
        };
        dialog.fields.insert(MODELS.into(), json!(ids));
    }
    let reply = super::open(app, dialog)?;
    app.state_mut().face_model = Some(draft);
    Ok(reply)
}

fn queued(d: &Dialog) -> bool {
    d.fields.get(QUEUE) == Some(&Value::Bool(true))
}

/// The inspection of a local file, once it is done: the job's result.
fn inspected(app: &App, d: &Dialog) -> Option<Value> {
    let job = d.fields.get(INSPECT).and_then(Value::as_u64)?;
    app.session()?.job(job).filter(|e| e.state == dalica_engine::JobState::Done).and_then(|e| e.result)
}

/// Download (or Add Model for a file) is enabled once the licence box is ticked, and not while a
/// download runs; Cancel reads Close while one runs and once every file is installed.
pub fn look(app: &App, d: &Dialog) -> Look {
    let draft = app.state().face_model.as_ref();
    let accepted = draft.is_some_and(|f| f.accepted);
    let running = draft.is_some_and(|f| f.progress.is_some()) || queued(d);
    if draft.is_some_and(|f| f.file.is_some()) {
        let usable = inspected(app, d).is_some_and(|r| r.get("refused").is_none_or(Value::is_null));
        return Look {
            title: TITLE.into(),
            width: WIDTH,
            buttons: vec![Button::new("install", "Add Model", ButtonKind::Default).enabled(accepted && usable), Button::cancel()],
        };
    }
    let list = app.face_models.last();
    let ids: Vec<&str> = d.fields.get(MODELS).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let all_installed = !ids.is_empty() && list.is_some_and(|l| ids.iter().all(|id| model(l, id).is_some_and(|m| text(m, "state") == "installed")));
    let close = if running || all_installed { Button::close() } else { Button::cancel() };
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![
            Button::new("addFile", "Add a Model File…", ButtonKind::Other).enabled(!running && app.can_pick().is_ok()),
            Button::new("download", "Download", ButtonKind::Default).enabled(accepted && !running).stays_open(),
            close,
        ],
    }
}

/// Under a control request, whether Add Model may install `path`: it must be the file the dialog
/// opened on ([`FILE`], which automation could only open on after the read-root check with links
/// resolved), and lie, as written, inside an automation read root or a watched folder, or the
/// session be the demo's. Nothing here touches the disk (this runs on the UI thread). The person's
/// own clicks are not checked.
fn automation_may_install(app: &App, d: &Dialog, path: &std::path::Path) -> Result<(), CmdError> {
    let Some(config) = crate::control::automated() else { return Ok(()) };
    if d.fields.get(FILE).and_then(Value::as_str).map(std::path::Path::new) != Some(path) {
        return Err(CmdError::Disabled(format!(
            "{} is not the file this dialog opened on; open the dialog on it (faceModel.addFile {{path}})",
            path.display()
        )));
    }
    let watched = app.session().map(|s| s.roots().to_vec()).unwrap_or_default();
    if !config.demo && !config.read_roots.iter().chain(&watched).any(|r| path.starts_with(r)) {
        return Err(CmdError::Disabled(format!(
            "{} is outside every automation read root (--automation-read-root) and every watched folder",
            path.display()
        )));
    }
    Ok(())
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    let draft = app.state().face_model.clone().unwrap_or_default();
    match action {
        "download" | "install" if !draft.accepted => Err(CmdError::Disabled(NEEDS_ACCEPT.into())),
        "download" | "install" if let Err(e) = models::external_allowed() => Err(e),
        "download" => {
            models::set_field(app, ERROR, Value::Null);
            let reply = models::start_next(app, None)?;
            if reply.is_null() {
                app.state_mut().status = "Every file is installed.".into();
            }
            Ok(reply)
        }
        "install" => {
            let path = draft.file.ok_or_else(|| CmdError::Internal("the face model dialog has no file".into()))?;
            automation_may_install(app, d, &path)?;
            let reply = app.run("faces.models.install", &json!({ "path": path, "acknowledged": true }))?;
            app.state_mut().status = "Adding the face model…".into();
            Ok(reply)
        }
        "addFile" => app.run("faceModel.addFile", &json!({})),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}

/// A download-style size: decimal megabytes ("38.7 MB", "232 KB").
pub fn size_text(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) if b >= 10_000_000 => format!("{} MB", (b as f64 / 1e6).round() as u64),
        Some(b) if b >= 1_000_000 => format!("{:.1} MB", b as f64 / 1e6),
        Some(b) => format!("{} KB", (b as f64 / 1e3).round().max(1.0) as u64),
        None => String::new(),
    }
}

/// "face detector" / "face recogniser".
pub fn role_text(m: &Value) -> &'static str {
    match text(m, "role") {
        "detector" => "face detector",
        _ => "face recogniser",
    }
}

/// What a model says when it does not say what it was trained on.
pub const TRAINED_UNKNOWN: &str = "What it was trained on is not known.";

/// A model's name with its role ("SFace (face recogniser)"): the name, which names the role for
/// the known models, else the name and the role; `id` when it has no name.
pub fn display_name(m: &Value, id: &str) -> String {
    let name = text(m, "name");
    match name {
        "" => format!("{id} ({})", role_text(m)),
        n if n.contains('(') => n.to_string(),
        n => format!("{n} ({})", role_text(m)),
    }
}

/// "Licence: Apache-2.0 · commercial use allowed".
pub fn licence_text(m: &Value) -> String {
    let licence = m.get("licence").unwrap_or(&Value::Null);
    let name = licence.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("Unknown licence");
    let commercial = match licence.get("commercial").and_then(Value::as_str) {
        Some("yes") => "commercial use allowed",
        Some("no") => "not for commercial use",
        _ => "commercial use not stated",
    };
    format!("Licence: {name} · {commercial}")
}

/// "Installed · in use", "Not downloaded", "Partly downloaded (12.3 MB)", "Downloading…".
pub fn state_text(m: &Value) -> String {
    let partial = || size_text(m.get("partialBytes").and_then(Value::as_u64));
    match text(m, "state") {
        "installed" if m.get("inUse") == Some(&Value::Bool(true)) => "Installed · in use".into(),
        "installed" => "Installed".into(),
        "downloading" => "Downloading…".into(),
        "partial" => format!("Partly downloaded ({})", partial()),
        _ => "Not downloaded".into(),
    }
}

/// Asks the window to open `url` in the browser and puts it on the clipboard, saying so on the
/// status line (the window opens pages once it has the OS integration).
fn open_page(app: &mut App, ctx: &egui::Context, url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    ctx.open_url(egui::OpenUrl::new_tab(url));
    ctx.copy_text(url.to_string());
    app.state_mut().status = format!("The page's address is on the clipboard: {url}");
}

/// The tallest the list of files gets in a window `screen_h` points tall: the dialog keeps its
/// title, intro, box, notes and buttons in view, and the list scrolls.
fn list_height(screen_h: f32) -> f32 {
    (screen_h - 330.0).clamp(120.0, 380.0)
}

fn label(ui: &mut Ui, s: &str, size: f32, color: Color32) {
    ui.add(egui::Label::new(RichText::new(s).font(fonts::regular(size)).color(color)).wrap());
}

/// One model: its name, role, size and `state` (none for a local file); its licence (`caution`
/// when it is not built in or does not plainly allow commercial use) and terms; what it was
/// trained on; its site with Open page. Returns the page to open when Open page was clicked.
fn model_row(ui: &mut Ui, t: &Tokens, m: &Value, id: &str, size: Option<u64>, state: Option<&str>) -> Option<String> {
    let caution = m.get("caution") == Some(&Value::Bool(true));
    let terms = if caution { t.caution } else { t.text_label };
    let mut page = None;
    let top = ui.cursor().min;
    ui.horizontal(|ui| {
        ui.label(RichText::new(display_name(m, id)).font(fonts::semibold(t.body)).color(t.text));
        let size = size_text(size.or_else(|| m.get("sizeBytes").and_then(Value::as_u64)));
        let version = text(m, "version");
        let what: Vec<String> =
            [(!version.is_empty()).then(|| format!("version {version}")), (!size.is_empty()).then_some(size)].into_iter().flatten().collect();
        ui.label(RichText::new(what.join(" · ")).font(fonts::regular(t.small)).color(t.text_dim));
        if let Some(state) = state {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(state).font(fonts::regular(t.small)).color(t.text_label));
            });
        }
    });
    label(ui, &licence_text(m), t.body, terms);
    let notice = m.get("licence").and_then(|l| l.get("notice")).and_then(Value::as_str).unwrap_or_default();
    if !notice.is_empty() {
        label(ui, notice, t.small, if caution { t.caution } else { t.text_dim });
    }
    let trained = text(m, "trainedOn");
    label(ui, if trained.is_empty() { TRAINED_UNKNOWN } else { trained }, t.small, t.text_dim);
    let url = m.get("page").and_then(Value::as_str).unwrap_or_default().to_string();
    let site = text(m, "site");
    if !site.is_empty() || !url.is_empty() {
        ui.horizontal(|ui| {
            let from = if site.is_empty() { url.clone() } else { format!("From {site}") };
            ui.label(RichText::new(from).font(fonts::regular(t.small)).color(t.text_dim));
            if url.starts_with("https://") {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, t, &page_id(id), "Open page", false).on_hover_text(url.as_str()).clicked() {
                        page = Some(url.clone());
                    }
                });
            }
        });
    }
    let rect = egui::Rect::from_min_max(top, egui::pos2(ui.max_rect().max.x, ui.cursor().min.y));
    let label = match state {
        Some(state) => format!("{} · {} · {state}", text(m, "name"), licence_text(m)),
        None => format!("{} · {}", text(m, "name"), licence_text(m)),
    };
    widgets::register(ui.ctx(), &row_id(id), Role::Other, &label, Target::Painted { rect, clip: ui.clip_rect() }, false);
    page
}

/// A model the list does not know (an id given by hand): its id and why nothing can be shown.
fn unknown_row(ui: &mut Ui, t: &Tokens, id: &str) {
    let top = ui.cursor().min;
    ui.label(RichText::new(id).font(fonts::semibold(t.body)).color(t.text));
    label(ui, "Dalica knows no face model by this name.", t.small, t.caution);
    let rect = egui::Rect::from_min_max(top, egui::pos2(ui.max_rect().max.x, ui.cursor().min.y));
    widgets::register(ui.ctx(), &row_id(id), Role::Other, id, Target::Painted { rect, clip: ui.clip_rect() }, false);
}

/// The licence box and the line under it; a refused file's box is disabled.
fn accept_box(app: &mut App, ui: &mut Ui, t: &Tokens, disabled: Option<&str>, note: &str) {
    let mut accepted = app.state().face_model.as_ref().is_some_and(|f| f.accepted);
    let r = checkbox_row(ui, t, ACCEPT_ID, ACCEPT, &mut accepted, Some(note), disabled).response;
    if r.changed()
        && let Some(f) = app.state_mut().face_model.as_mut()
    {
        f.accepted = accepted;
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    ui.spacing_mut().item_spacing.y = 4.0;
    let draft = app.state().face_model.clone().unwrap_or_default();
    let max_h = list_height(ui.ctx().content_rect().height());
    let mut page = None;
    if let Some(path) = &draft.file {
        file_body(app, ui, &t, d, path, max_h, &mut page);
    } else {
        list_body(app, ui, &t, d, &draft, max_h, &mut page);
    }
    if let Some(url) = page {
        open_page(app, ui.ctx(), &url);
    }
}

/// The scroll area the files are drawn in.
fn files_area(ui: &mut Ui, t: &Tokens, id: &str, max_h: f32, add: impl FnOnce(&mut Ui)) {
    let frame = egui::Frame::new().fill(t.inset).stroke(egui::Stroke::new(1.0, t.divider)).inner_margin(egui::Margin::same(8));
    frame.show(ui, |ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::vertical()
            .id_salt(id)
            .max_height(max_h)
            .auto_shrink([false, true])
            .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
            .show(ui, add);
    });
}

#[allow(clippy::too_many_arguments)] // the dialog's parts, passed down from `body`
fn list_body(app: &mut App, ui: &mut Ui, t: &Tokens, d: &mut Dialog, draft: &FaceModelDraft, max_h: f32, page: &mut Option<String>) {
    let list = models::list(app);
    let mut ids: Vec<String> =
        d.fields.get(MODELS).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    if ids.is_empty()
        && let Some(l) = &list
    {
        ids = draft.entry.clone().map_or_else(|| starter(l), |e| vec![e]);
        d.fields.insert(MODELS.into(), json!(ids));
    }
    label(ui, if draft.entry.is_some() { INTRO_ONE } else { INTRO }, t.body, t.text_label);
    let problem = list.as_ref().and_then(|l| l.get("problem")).and_then(Value::as_str).map(str::to_string);
    if let Some(p) = &problem {
        label(ui, p, t.small, t.caution);
    }
    ui.add_space(4.0);
    let empty = Value::Null;
    let l = list.as_ref().unwrap_or(&empty);
    // The list is read again only between downloads: the one running is the session's.
    let downloading = app.session().and_then(|s| s.face_models().downloading().map(str::to_string));
    files_area(ui, t, "dialog:faceModel/files", max_h, |ui| {
        if ids.is_empty() {
            label(ui, "No face models are known here.", t.body, t.text_dim);
        }
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
            }
            match model(l, id) {
                Some(m) => {
                    let state = if downloading.as_deref() == Some(id.as_str()) { "Downloading…".to_string() } else { state_text(m) };
                    if let Some(url) = model_row(ui, t, m, id, None, Some(&state)) {
                        *page = Some(url);
                    }
                }
                None => unknown_row(ui, t, id),
            }
        }
    });
    if let Some(note) = l.get("commercialNote").and_then(Value::as_str) {
        label(ui, note, t.small, t.text_dim);
    }
    ui.add_space(4.0);
    accept_box(app, ui, t, None, NOTHING_YET);
    // The download running: its progress and its stop button; the last one's failure.
    let job = d.fields.get(JOB).and_then(Value::as_u64);
    let running = job.and_then(|j| app.session().and_then(|s| s.job(j))).filter(|e| !e.state.is_finished());
    if let Some(entry) = running {
        ui.add_space(4.0);
        let out = progress_row(ui, t, PROGRESS_ID, &entry.label, draft.progress, Some(STOP_ID), ui.available_width());
        if out.cancel
            && let Some(id) = app.session().and_then(|s| s.face_models().downloading().map(str::to_string))
        {
            let _ = app.run("faces.models.cancel", &json!({ "id": id }));
        }
    }
    if let Some(e) = d.fields.get(ERROR).and_then(Value::as_str).filter(|e| !e.is_empty()) {
        label(ui, e, t.small, t.danger);
    }
    // A download in progress or a model folder being read: keep the frames coming.
    if app.session().is_some_and(|s| !s.face_models().idle()) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
}

#[allow(clippy::too_many_arguments)] // the dialog's parts, passed down from `body`
fn file_body(app: &mut App, ui: &mut Ui, t: &Tokens, d: &mut Dialog, path: &std::path::Path, max_h: f32, page: &mut Option<String>) {
    label(ui, INTRO_FILE, t.body, t.text_label);
    ui.add_space(4.0);
    let job = d.fields.get(INSPECT).and_then(Value::as_u64);
    let entry = job.and_then(|j| app.session().and_then(|s| s.job(j)));
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mut refused: Option<String> = None;
    files_area(ui, t, "dialog:faceModel/file", max_h, |ui| match &entry {
        Some(e) if e.state == dalica_engine::JobState::Done => {
            let r = e.result.clone().unwrap_or(Value::Null);
            let size = r.get("sizeBytes").and_then(Value::as_u64);
            match r.get("model").filter(|m| m.is_object()) {
                Some(m) => {
                    if let Some(url) = model_row(ui, t, m, text(m, "id"), size, None) {
                        *page = Some(url);
                    }
                }
                None => {
                    ui.label(RichText::new(&name).font(fonts::semibold(t.body)).color(t.text));
                    label(ui, &size_text(size), t.small, t.text_dim);
                }
            }
            let assumptions: Vec<&str> =
                r.get("assumptions").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            if !assumptions.is_empty() {
                ui.add_space(2.0);
                label(ui, "Dalica does not know this model, so it assumes:", t.small, t.text_label);
                for a in assumptions {
                    label(ui, &format!("•  {a}"), t.small, t.text_dim);
                }
            }
            if r.get("installed") == Some(&Value::Bool(true)) {
                label(ui, "This model is installed already.", t.small, t.text_dim);
            }
            refused = r.get("refused").and_then(Value::as_str).map(str::to_string);
            if let Some(why) = &refused {
                label(ui, &format!("This file cannot be used: {why}"), t.body, t.caution);
            }
        }
        Some(e) if e.state.is_finished() => {
            ui.label(RichText::new(&name).font(fonts::semibold(t.body)).color(t.text));
            let why = e.error.clone().unwrap_or_else(|| "it could not be read".into());
            label(ui, &format!("This file cannot be used: {why}"), t.body, t.caution);
            refused = Some(why);
        }
        Some(_) => {
            ui.label(RichText::new(&name).font(fonts::semibold(t.body)).color(t.text));
            label(ui, "Looking at the file…", t.small, t.text_dim);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        None => {
            ui.label(RichText::new(&name).font(fonts::semibold(t.body)).color(t.text));
            let why = d.fields.get(ERROR).and_then(Value::as_str).unwrap_or("The file could not be looked at.");
            label(ui, why, t.body, t.caution);
            refused = Some(why.to_string());
        }
    });
    ui.add_space(4.0);
    let note = "The model is kept on this computer only; Dalica never uploads or shares it.";
    accept_box(app, ui, t, refused.as_deref().map(|_| "This file cannot be used."), note);
    ui.allocate_space(vec2(0.0, 2.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_licences_and_states_read_as_the_dialog_shows_them() {
        assert_eq!(size_text(Some(38_696_353)), "39 MB");
        assert_eq!(size_text(Some(3_869_635)), "3.9 MB");
        assert_eq!(size_text(Some(232_000)), "232 KB");
        assert_eq!(size_text(Some(10)), "1 KB");
        assert_eq!(size_text(None), "");
        let m = json!({ "licence": { "name": "MIT", "commercial": "yes" }, "role": "detector", "state": "installed", "inUse": false });
        assert_eq!(
            (licence_text(&m).as_str(), role_text(&m), state_text(&m).as_str()),
            ("Licence: MIT · commercial use allowed", "face detector", "Installed")
        );
        let m = json!({ "licence": { "commercial": "no" }, "state": "partial", "partialBytes": 1_500_000 });
        assert_eq!(
            (licence_text(&m).as_str(), state_text(&m).as_str()),
            ("Licence: Unknown licence · not for commercial use", "Partly downloaded (1.5 MB)")
        );
        assert_eq!(licence_text(&json!({})), "Licence: Unknown licence · commercial use not stated");
        assert_eq!(state_text(&json!({ "state": "installed", "inUse": true })), "Installed · in use");
        assert_eq!(list_height(900.0), 380.0);
        assert_eq!(list_height(600.0), 270.0);
        assert_eq!(list_height(100.0), 120.0);
    }
}
