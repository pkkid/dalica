//! The face model dialog's commands and the window's view of the face models (`faces.md` §3,
//! `organizing.md` §6.1; the engine's `faces.models.*` are M13.16's):
//!
//! - `faceModel.open {entry?, file?}` opens the face model dialog (`crate::dialogs::face_model`) on
//!   a model of `faces.models.list`, on a local model file (which it starts inspecting), or, with
//!   neither, on the files Dalica needs to find faces ([`starter`]). Options ▸ People's Download…,
//!   turning on Find faces in my photos without a model ([`applied`]) and the first open of the
//!   People pane in a run ([`frame`]) open it.
//! - `faceModel.addFile {path?}` is Add a Model File…: without a path it opens the open-file picker
//!   (the ONNX filter), whose answer runs it again with the file; with one it opens the dialog on
//!   that file. A `.onnx` file dropped on the window does the same ([`dropped_model`]).
//! - `faceModel.remove {id, confirm}` is Options ▸ People's Remove: it asks first ("Remove the face
//!   model …?"), then runs `faces.models.remove {id}`.
//! - `faceModel.dropData {confirm}` is the offer made when Find faces in my photos is turned off:
//!   "Also delete the face data …?" [Delete Face Data] [Keep], then `faces.enable {on: false,
//!   dropEmbeddings: true}`.
//! - The questions of this task are rows of [`crate::dialogs::confirm::ASKS`] defined here:
//!   [`REMOVE_FACES`] (the Folder Manager's Remove Faces, `faces.removeInFolders`), [`WRITE_NAMES`]
//!   (Tools ▸ Experimental ▸ Write People Names to Photos…, `faces.writeXmp` on the selection),
//!   [`REMOVE_MODEL`] and [`DROP_DATA`].
//!
//! **The models as the window sees them** ([`Models`], [`list`]): `faces.models.list`'s reply,
//! read again only while the engine's face model workers are idle (so the read never waits for the
//! models folder) and only when what it answers for changed (the folder read again, a download
//! started or ended, another model chosen). Nothing here touches the disk or the network: the
//! engine reads, downloads and installs on its workers.
//!
//! **Downloads one after another** ([`frame`]): the dialog's Download starts the first of its files
//! not installed yet; when that download ends well the next starts, until every file of the dialog
//! is installed. A failed download stops the queue with its reason in the dialog; a stopped one
//! keeps what it fetched (the next Download resumes it).

use std::path::{Path, PathBuf};

use dalica_engine::{CmdError, JobState};
use dalica_library::FacePerson;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse_object};
use crate::app::App;
use crate::dialogs::confirm::{Ask, registered};
use crate::dialogs::{DialogKind, face_model};
use crate::picker::{MODEL_FILTERS, OpenRequest};
use crate::state::{FaceModelDraft, Pane};

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "faceModel.open",
        label: "Face Model…",
        menu: "",
        keys: &[],
        params: "{entry?, file?}: open the face model dialog on a model of faces.models.list, on a local model file, or (neither) on the files Dalica needs to find faces",
        run: open,
    },
    UiCommand {
        id: "faceModel.addFile",
        label: "Add a Model File…",
        menu: "",
        keys: &[],
        params: "{path?}: Add a Model File…: without a path, pick a local .onnx model file; with one, open the face model dialog on it",
        run: add_file,
    },
    UiCommand {
        id: "faceModel.remove",
        label: "Remove Face Model",
        menu: "",
        keys: &[],
        params: "{\"id\":string, \"confirm\"?:bool}: Options ▸ People's Remove: asks first, then faces.models.remove {id}",
        run: remove,
    },
    UiCommand {
        id: "faceModel.dropData",
        label: "Delete Face Data",
        menu: "",
        keys: &[],
        params: "{\"confirm\"?:bool}: the offer made when Find faces in my photos is turned off: asks first, then faces.enable {on: false, dropEmbeddings: true}",
        run: drop_data,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("faceModel.addFile", can_add)];

/// The title of the open-file picker of Add a Model File….
pub const PICK_TITLE: &str = "Add a Model File";

/// The Folder Manager's Remove Faces question (`organizing.md` §2).
pub const REMOVE_FACES: Ask = Ask {
    command: "faces.removeInFolders",
    kind: DialogKind::RemoveFaces,
    title: "Remove Faces",
    text: dalica_engine::cmd::face_scan::REMOVE_QUESTION,
    action: "removeFaces",
    label: "Remove Faces",
    asks: remove_faces_asks,
    text_for: None,
    extra: &[],
};
/// Remove Faces' Cancel button.
pub const KEEP_FACES: &str = "Keep Faces";

/// Write People Names to Photos…'s question (`xmp-interop.md` §6).
pub const WRITE_NAMES: Ask = Ask {
    command: "faces.writeXmp",
    kind: DialogKind::WritePeopleNames,
    title: "Write People Names",
    text: dalica_engine::cmd::face_regions::QUESTION,
    action: "write",
    label: "Write Names",
    asks: write_names_asks,
    text_for: Some(write_names_text),
    extra: &[],
};

/// Remove's question when the model is not known by name.
pub const REMOVE_MODEL_QUESTION: &str =
    "Remove this face model? Its file and the face data worked out with it are deleted; face rectangles and names stay.";

/// Options ▸ People's Remove question.
pub const REMOVE_MODEL: Ask = Ask {
    command: "faceModel.remove",
    kind: DialogKind::RemoveFaceModel,
    title: "Remove Face Model",
    text: REMOVE_MODEL_QUESTION,
    action: "remove",
    label: "Remove",
    asks: remove_model_asks,
    text_for: Some(remove_model_text),
    extra: &[],
};

/// The offer made when Find faces in my photos is turned off (`faces.md` §7).
pub const DROP_DATA: Ask = Ask {
    command: "faceModel.dropData",
    kind: DialogKind::DropFaceData,
    title: "Delete Face Data",
    text: dalica_engine::faces_scan::DROP_OFFER,
    action: "delete",
    label: "Delete Face Data",
    asks: drop_data_asks,
    text_for: None,
    extra: &[],
};
/// Delete Face Data's Cancel button.
pub const KEEP_DATA: &str = "Keep";

fn remove_faces_asks(app: &App, params: &Value) -> bool {
    registered(app, "faces.removeInFolders") && params.get("folders").and_then(Value::as_array).is_some_and(|f| !f.is_empty())
}

/// The photos `faces.writeXmp` names, when it names any.
fn write_names_count(params: &Value) -> Option<usize> {
    params.get("ids").and_then(Value::as_array).map(Vec::len).filter(|n| *n > 0)
}

fn write_names_asks(app: &App, params: &Value) -> bool {
    registered(app, "faces.writeXmp") && write_names_count(params).is_some()
}

fn write_names_text(_: &App, params: &Value) -> Option<String> {
    write_names_count(params).map(dalica_engine::cmd::face_regions::question)
}

/// Whether `params` is an object holding only `keys` (the parameters a question can be asked
/// about; anything else is the command's to refuse).
fn only(params: &Value, keys: &[&str]) -> bool {
    params.is_null() || params.as_object().is_some_and(|o| o.keys().all(|k| keys.contains(&k.as_str())))
}

fn remove_model_asks(app: &App, params: &Value) -> bool {
    registered(app, "faces.models.remove")
        && only(params, &["id", "confirm"])
        && params.get("id").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
}

fn remove_model_text(app: &App, params: &Value) -> Option<String> {
    let id = params.get("id").and_then(Value::as_str)?;
    let list = app.face_models.list.as_ref()?;
    let name = model(list, id)?.get("name").and_then(Value::as_str)?;
    Some(format!("Remove the face model \"{name}\"? Its file and the face data worked out with it are deleted; face rectangles and names stay."))
}

fn drop_data_asks(app: &App, params: &Value) -> bool {
    registered(app, "faces.enable") && only(params, &["confirm"])
}

/// Whether the window may reach outside Dalica for a face model now (a download, or a local file
/// read and installed): always for the person's own clicks; under a control request only with
/// `--automation-allow-external`, as the engine's `faces.models.*` are (`faces.md` §3.4).
pub fn external_allowed() -> Result<(), CmdError> {
    match crate::control::automated() {
        Some(c) if !c.allow_external => Err(CmdError::Disabled(super::map::EXTERNAL.into())),
        _ => Ok(()),
    }
}

/// Add a Model File… without a file needs the open-file picker.
fn can_add(app: &App, params: &Value) -> Result<(), String> {
    if params.get("path").is_some_and(|p| !p.is_null()) { Ok(()) } else { app.can_pick() }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    file: Option<PathBuf>,
}

fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Open = parse_object(params)?;
    let draft = FaceModelDraft { entry: p.entry, file: p.file, accepted: false, progress: None };
    draft.check().map_err(CmdError::BadParams)?;
    face_model::open(app, draft)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddFile {
    #[serde(default)]
    path: Option<PathBuf>,
}

fn add_file(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: AddFile = parse_object(params)?;
    match p.path {
        Some(path) => {
            if !path.is_absolute() {
                return Err(CmdError::BadParams(format!("path: an absolute path of a model file, not {}", path.display())));
            }
            let draft = FaceModelDraft { entry: None, file: Some(path), accepted: false, progress: None };
            draft.check().map_err(CmdError::BadParams)?;
            face_model::open(app, draft)
        }
        None => {
            let request = OpenRequest { title: PICK_TITLE, filters: MODEL_FILTERS, command: "faceModel.addFile", params: json!({}) };
            app.pick_file_for(request).map_err(CmdError::Disabled)?;
            Ok(json!({ "dialog": "filePicker" }))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    id: String,
    #[serde(default)]
    confirm: bool,
}

/// Runs only confirmed: the question is asked by the dialog host ([`REMOVE_MODEL`]).
fn remove(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Remove = parse_object(params)?;
    if !p.confirm {
        return Err(CmdError::Disabled(format!("{REMOVE_MODEL_QUESTION} Send \"confirm\": true to go ahead.")));
    }
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
    session.execute("faces.models.remove", &json!({ "id": p.id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DropData {
    #[serde(default)]
    confirm: bool,
}

/// Runs only confirmed: the question is asked by the dialog host ([`DROP_DATA`]).
fn drop_data(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: DropData = parse_object(params)?;
    if !p.confirm {
        return Err(CmdError::Disabled(format!("{} Send \"confirm\": true to go ahead.", dalica_engine::faces_scan::DROP_OFFER)));
    }
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled("The library is still opening.".into()))?;
    session.execute("faces.enable", &json!({ "on": false, "dropEmbeddings": true }))
}

/// What the window knows of the face models between frames (never saved): `faces.models.list`'s
/// last reply and what it answered for, and whether the dialog was offered on the People pane.
#[derive(Debug, Default)]
pub struct Models {
    list: Option<Value>,
    seen: Option<Seen>,
    /// The People pane was open on the last frame.
    people_open: bool,
    /// The People pane opened and the dialog may be offered once the list is known.
    want_offer: bool,
    /// The dialog was offered on the People pane this run (once at most).
    offered: bool,
}

/// What a `faces.models.list` reply answered for: the models folder as last read (by its listing's
/// address, which changes whenever it is read again), the download running or last run, and the
/// model chosen with Use.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    listing: usize,
    installed: usize,
    download: Option<u64>,
    downloading: Option<String>,
    selected: Option<String>,
}

impl Models {
    /// The last reply of `faces.models.list`, if one was read.
    pub fn last(&self) -> Option<&Value> {
        self.list.as_ref()
    }
}

/// `faces.models.list` as the window sees it: read again when the engine's face model workers are
/// idle and what it answers for changed, else the last reply. `None` without a library.
pub fn list(app: &mut App) -> Option<Value> {
    let session = app.session()?;
    let fm = session.face_models();
    let seen = Seen {
        listing: fm.listing().map_or(0, |l| std::ptr::from_ref(l) as usize),
        installed: fm.listing().map_or(0, |l| l.installed.len()),
        download: fm.download(),
        downloading: fm.downloading().map(str::to_string),
        selected: session.settings().people().model,
    };
    if fm.idle() && (app.face_models.list.is_none() || app.face_models.seen.as_ref() != Some(&seen)) {
        // Idle: the folder is read, so this answers at once (`faces.md` §3.4).
        let reply = app.session_mut().and_then(|s| s.execute("faces.models.list", &json!({})).ok());
        app.face_models.list = reply;
        app.face_models.seen = Some(seen);
    }
    app.face_models.list.clone()
}

/// The models of a `faces.models.list` reply.
pub fn models_of(list: &Value) -> &[Value] {
    list.get("models").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Model `id` of a `faces.models.list` reply.
pub fn model<'a>(list: &'a Value, id: &str) -> Option<&'a Value> {
    models_of(list).iter().find(|m| m.get("id").and_then(Value::as_str) == Some(id))
}

/// A model's field as text ("" when it has none).
pub fn text<'a>(m: &'a Value, key: &str) -> &'a str {
    m.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn installed(m: &Value) -> bool {
    text(m, "state") == "installed"
}

fn role_installed(list: &Value, role: &str) -> bool {
    models_of(list).iter().any(|m| text(m, "role") == role && installed(m))
}

/// Whether a model can be added here and none is: a models folder that can take one, and no
/// recogniser installed.
pub fn needs_model(list: &Value) -> bool {
    !list.get("folder").is_none_or(Value::is_null) && list.get("problem").is_none_or(Value::is_null) && !role_installed(list, "recogniser")
}

/// The files the dialog offers when it names no model: the detector when none is installed, and
/// the default recogniser when no recogniser is; with both installed, every built-in model.
pub fn starter(list: &Value) -> Vec<String> {
    let builtin: Vec<&Value> = models_of(list).iter().filter(|m| text(m, "origin") == "builtin").collect();
    let mut ids = Vec::new();
    if !role_installed(list, "detector")
        && let Some(m) = builtin.iter().find(|m| text(m, "role") == "detector")
    {
        ids.push(text(m, "id").to_string());
    }
    if !role_installed(list, "recogniser") {
        let recognisers = || builtin.iter().filter(|m| text(m, "role") == "recogniser");
        if let Some(m) = recognisers().find(|m| m.get("default") == Some(&Value::Bool(true))).or_else(|| recognisers().next()) {
            ids.push(text(m, "id").to_string());
        }
    }
    if ids.is_empty() {
        ids = builtin.iter().map(|m| text(m, "id").to_string()).collect();
    }
    ids
}

/// The first model file among the paths of the files `dropped` (a `.onnx` file), the one a drop
/// adds.
pub fn dropped_model<'a>(dropped: impl IntoIterator<Item = &'a Path>) -> Option<PathBuf> {
    dropped.into_iter().find(|p| is_model_file(p)).map(Path::to_path_buf)
}

/// Whether `path` names an ONNX model file (by its extension, any case).
pub fn is_model_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
}

/// The window's turn, each frame before the panels: a model file dropped on the window opens the
/// dialog on it; the dialog's downloads go on one after another ([`advance`]); the first open of
/// the People pane in a run offers the dialog when finding faces needs a model.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let dropped = ctx.input(|i| dropped_model(i.raw.dropped_files.iter().map(|f| f.path())));
    if let Some(path) = dropped {
        let _ = app.run("faceModel.addFile", &json!({ "path": path }));
    }
    advance(app);
    offer_on_people(app);
}

/// Offers the dialog once in a run, the first time the People pane opens while finding faces is on
/// and needs a model (and no other dialog is open).
fn offer_on_people(app: &mut App) {
    let open = app.state().pane == Some(Pane::People);
    if open && !app.face_models.people_open && !app.face_models.offered {
        app.face_models.want_offer = true;
    }
    app.face_models.people_open = open;
    if !open {
        app.face_models.want_offer = false;
    }
    if !app.face_models.want_offer || !app.state().dialogs.is_empty() {
        return;
    }
    let finding = app.session().is_some_and(|s| s.settings().people().find_faces);
    let Some(list) = list(app) else { return };
    app.face_models.want_offer = false;
    if finding && needs_model(&list) && list.get("download").is_none_or(Value::is_null) {
        app.face_models.offered = true;
        let _ = app.run("faceModel.open", &json!({}));
    }
}

/// The open face model dialog's fields, if it is open.
fn dialog_fields(app: &App) -> Option<&serde_json::Map<String, Value>> {
    app.state().dialogs.iter().find(|d| d.kind == DialogKind::FaceModel).map(|d| &d.fields)
}

/// Sets field `key` of the open face model dialog.
pub fn set_field(app: &mut App, key: &str, value: Value) {
    if let Some(d) = app.state_mut().dialogs.iter_mut().find(|d| d.kind == DialogKind::FaceModel) {
        d.fields.insert(key.into(), value);
    }
}

/// The dialog's downloads: its progress while one runs; once it has ended and the folder is read
/// again, the next file of the dialog not installed yet, until none is left. A failed download
/// stops the queue with its reason; a stopped one ends it.
pub fn advance(app: &mut App) {
    let Some(fields) = dialog_fields(app) else { return };
    let queued = fields.get(face_model::QUEUE) == Some(&Value::Bool(true));
    let job = fields.get(face_model::JOB).and_then(Value::as_u64);
    let Some(session) = app.session() else { return };
    let entry = job.and_then(|j| session.job(j));
    let running = entry.as_ref().is_some_and(|e| !e.state.is_finished());
    let progress = entry.as_ref().filter(|_| running).map(|e| fraction(e.progress.done, e.progress.total));
    let idle = session.face_models().idle();
    if let Some(f) = app.state_mut().face_model.as_mut()
        && f.progress != progress
    {
        f.progress = progress;
    }
    if !queued || running || !idle {
        return;
    }
    match entry.map(|e| (e.state, e.error, e.result)) {
        Some((JobState::Failed, error, _)) => {
            set_field(app, face_model::ERROR, json!(error.unwrap_or_else(|| "The download failed.".into())));
            set_field(app, face_model::QUEUE, json!(false));
        }
        Some((JobState::Cancelled, ..)) => set_field(app, face_model::QUEUE, json!(false)),
        other => {
            // The file just installed is never fetched again, even if the folder says otherwise.
            let done = other.and_then(|(_, _, r)| r).and_then(|r| r.get("id").and_then(Value::as_str).map(str::to_string));
            if let Err(e) = start_next(app, done.as_deref()) {
                set_field(app, face_model::ERROR, json!(e.to_string()));
                set_field(app, face_model::QUEUE, json!(false));
            }
        }
    }
}

/// Starts the download of the dialog's next file not installed yet (never `done`, the one just
/// installed); with none left, ends the queue. Replies the download's reply, `null` when nothing
/// was left.
pub fn start_next(app: &mut App, done: Option<&str>) -> Result<Value, CmdError> {
    let ids: Vec<String> = dialog_fields(app)
        .and_then(|f| f.get(face_model::MODELS))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let list = list(app).unwrap_or(Value::Null);
    let next = ids.into_iter().find(|id| Some(id.as_str()) != done && model(&list, id).is_none_or(|m| !installed(m)));
    let Some(id) = next else {
        set_field(app, face_model::QUEUE, json!(false));
        set_field(app, face_model::JOB, Value::Null);
        return Ok(Value::Null);
    };
    let reply = app.run("faces.models.download", &json!({ "id": id, "acknowledged": true }))?;
    set_field(app, face_model::JOB, reply.get("job").cloned().unwrap_or(Value::Null));
    set_field(app, face_model::QUEUE, json!(true));
    Ok(reply)
}

/// `done` of `total` as 0 … 1 (0 while the total is not known).
fn fraction(done: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (done as f64 / total as f64).clamp(0.0, 1.0) as f32 }
}

/// What Options ▸ People does once its options are applied: Find faces in my photos turned on
/// starts the scan again (a cancelled scan too) and, without a model, offers the face model dialog;
/// turned off, it offers to delete the face data ([`DROP_DATA`]).
pub fn applied(app: &mut App, values: &serde_json::Map<String, Value>, ok: bool) {
    if !ok {
        return;
    }
    match values.get("people.findFaces") {
        Some(Value::Bool(true)) => {
            if let Some(s) = app.session_mut() {
                let _ = s.execute("faces.enable", &json!({ "on": true }));
            }
            if list(app).is_some_and(|l| needs_model(&l) && l.get("download").is_none_or(Value::is_null)) {
                let _ = app.run("faceModel.open", &json!({}));
            }
        }
        Some(Value::Bool(false)) => {
            let _ = app.run("faceModel.dropData", &json!({}));
        }
        _ => {}
    }
}

/// The folders among `folders` (and their subfolders) that hold faces the scan found (unnamed or
/// ignored): what Remove Faces would take away.
pub fn with_found_faces(app: &App, folders: &[PathBuf]) -> Vec<PathBuf> {
    let Some(lib) = app.session().map(dalica_engine::Session::library) else { return Vec::new() };
    folders
        .iter()
        .filter(|root| {
            lib.folders()
                .filter(|f| f.path.starts_with(root))
                .any(|f| lib.photos_in(f.id).iter().any(|p| lib.faces(p.id).iter().any(|face| !matches!(face.person, FacePerson::Person(_)))))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_opens_with_its_draft_and_download_waits_for_the_box() {
        let mut app = App::new(None);
        assert!(matches!(app.run("faceModel.open", &json!({ "entry": "a", "file": "/m.onnx" })), Err(CmdError::BadParams(_))));
        assert_eq!(app.run("faceModel.open", &json!({ "entry": "sface" })).unwrap(), json!({ "dialog": "faceModel" }));
        assert_eq!(app.state().face_model.as_ref().and_then(|f| f.entry.clone()).as_deref(), Some("sface"));
        assert!(matches!(crate::dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))), "Download is off until the box is ticked");
        if let Some(f) = app.state_mut().face_model.as_mut() {
            f.accepted = true;
        }
        assert!(matches!(crate::dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))), "no library: nothing to download into");
        crate::dialogs::cancel(&mut app).unwrap();
        assert!(app.state().face_model.is_none(), "closing drops the draft");
        assert!(matches!(app.run("faceModel.addFile", &json!({ "path": "m.onnx" })), Err(CmdError::BadParams(_))), "an absolute path");
        assert!(matches!(app.run("faceModel.addFile", &json!({})), Err(CmdError::Disabled(_))), "no picker here");
    }

    #[test]
    fn a_dropped_model_file_is_the_first_onnx() {
        let paths = |p: &[&'static str]| p.iter().map(|p| Path::new(*p)).collect::<Vec<_>>();
        assert_eq!(dropped_model(paths(&[])), None);
        assert_eq!(dropped_model(paths(&["/a/photo.jpg", "/a/m.ONNX", "/a/n.onnx"])), Some("/a/m.ONNX".into()));
        assert_eq!(dropped_model(paths(&["/a/onnx", "/a/m.onnx.part"])), None);
    }

    #[test]
    fn the_starter_files_are_what_finding_faces_still_needs() {
        let m = |id: &str, role: &str, state: &str, default: bool| json!({ "id": id, "role": role, "origin": "builtin", "state": state, "default": default });
        let list = |models: Vec<Value>| json!({ "folder": "/m", "problem": null, "models": models });
        let none = list(vec![
            m("yunet", "detector", "available", false),
            m("sface", "recogniser", "available", true),
            m("aura", "recogniser", "available", false),
        ]);
        assert_eq!(starter(&none), ["yunet", "sface"]);
        assert!(needs_model(&none));
        let det = list(vec![
            m("yunet", "detector", "installed", false),
            m("sface", "recogniser", "partial", true),
            m("aura", "recogniser", "available", false),
        ]);
        assert_eq!(starter(&det), ["sface"]);
        let all = list(vec![
            m("yunet", "detector", "installed", false),
            m("sface", "recogniser", "available", true),
            m("aura", "recogniser", "installed", false),
        ]);
        assert_eq!(starter(&all), ["yunet", "sface", "aura"], "both kinds installed: every built-in model");
        assert!(!needs_model(&all));
        assert!(!needs_model(&json!({ "folder": null, "problem": "no folder", "models": [] })), "nowhere to put one");
    }
}
