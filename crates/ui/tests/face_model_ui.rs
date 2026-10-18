//! The face model dialog, Options ▸ People, Remove Faces and the Faces filter (M13.27,
//! `faces.md` §3, `organizing.md` §2 and §6.1, `menus-and-shortcuts.md` §5 People), headless, on a
//! library of two folders of scenes photos whose session has a models folder of its own and an
//! injected transport (never the network) and synthetic models (never a real one):
//!
//! - Download stays disabled until "I have read these terms and accept them for my own use" is
//!   ticked, and nothing is requested before; then the download shows its progress, stops on ×
//!   keeping what it fetched, and once it completes the model is installed and Options ▸ People
//!   shows it in use; the dialog's files download one after another; a control client without
//!   `--automation-allow-external` starts no download;
//! - the dialog is offered by the first open of People in a run and by Find faces turned on
//!   without a model;
//! - Add a Model File… installs a local file after the dialog showed its terms, and Options ▸
//!   People's status shows it; Remove asks first, then removes it (its file and manifest are gone);
//! - the confidence sliders move by 5 within 50 … 95 and survive a restart; Find faces turned off
//!   offers to delete the face data;
//! - the Folder Manager's Remove Faces drops the found faces of the folders turned off, and Keep
//!   Faces keeps them; Write People Names to Photos… asks about the selection;
//! - the Faces filter shows only the photos with faces;
//! - at 900 × 600 the dialog and the tab fit; the dialog and the tab match their fingerprints at
//!   1400 × 900 (`tests/fingerprints/dialog-face-model-1400x900.json`,
//!   `dialog-options-people-1400x900.json`; `DALICA_BLESS=1` stores new ones; renders and the
//!   screenshots of each state at both sizes go to `target/test-output/ui/`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, check_region, check_small, new_driver, put_jpeg, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_engine::face_models::SharedTransport;
use dalica_faces::hash::sha256_hex;
use dalica_faces::synthetic;
use dalica_fetch::{Body, Limits, MemoryTransport, Reply, Request, Served, Transport, TransportError};
use dalica_scan::Places;
use dalica_ui::control::{self, Config, Connection, Control, serve};
use dalica_ui::dialogs::folder_manager::FACES_ID;
use dalica_ui::dialogs::{DialogKind, face_model, options::people};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(120);
const URL: &str = "https://models.test/weights/mine.onnx";
const MINE: &str = "my-recogniser";
const DOWNLOAD: &str = "button:dialog.download";
const OPTIONS: &str = "dialog:options";
const DIALOG: &str = "dialog:faceModel";

/// A recogniser that passes the self-test (16 numbers per face).
fn good_model() -> Vec<u8> {
    shaped(16)
}

/// A recogniser of `dim` numbers per face, its output declared as `[1, dim]` as exported
/// recognisers declare it (`crates/engine/tests/face_models.rs`).
fn shaped(dim: u64) -> Vec<u8> {
    use dalica_faces::synthetic::{len_field, node_bytes, value_info_bytes, varint_field, weight_bytes};
    let model = synthetic::tiny_embedder_model(dim as usize);
    let graph = dalica_faces::graph::load(&model).unwrap();
    let mut g = Vec::new();
    for n in &graph.nodes {
        len_field(1, &node_bytes(n), &mut g);
    }
    let mut weights: Vec<_> = graph.weights.iter().collect();
    weights.sort_by(|a, b| a.0.cmp(b.0));
    for (name, w) in weights {
        len_field(5, &weight_bytes(name, w), &mut g);
    }
    for (name, shape) in &graph.inputs {
        let dims: Vec<Result<u64, &str>> = shape.iter().map(|&d| Ok(d as u64)).collect();
        len_field(11, &value_info_bytes(name, 1, &dims), &mut g);
    }
    for name in &graph.outputs {
        len_field(12, &value_info_bytes(name, 1, &[Ok(1), Ok(dim)]), &mut g);
    }
    let mut ops = Vec::new();
    len_field(1, b"", &mut ops);
    varint_field(2, 13, &mut ops);
    let mut out = Vec::new();
    varint_field(1, 8, &mut out);
    len_field(2, b"test", &mut out);
    len_field(7, &g, &mut out);
    len_field(8, &ops, &mut out);
    out
}

/// A `catalog.json` in `models` naming one recogniser at [`URL`] pinned to `bytes`.
fn catalog(models: &Path, bytes: &[u8]) {
    catalog_of(models, &[(MINE, URL, bytes, 16)]);
}

/// A `catalog.json` in `models` naming recognisers `(id, url, bytes, dim)`.
fn catalog_of(models: &Path, entries: &[(&str, &str, &[u8], u64)]) {
    let entries: Vec<Value> = entries
        .iter()
        .map(|(id, url, bytes, dim)| {
            json!({
                "id": id, "name": "My recogniser", "version": "1", "role": "embedder",
                "url": url, "sha256": sha256_hex(bytes), "sizeBytes": bytes.len(),
                "licence": { "name": "Research use only", "commercial": "no", "notice": "Not for commercial use." },
                "provenance": "Trained on a test set.",
                "output": { "kind": "embedding", "dim": dim },
                "thresholds": { "matchCosine": 0.5 }
            })
        })
        .collect();
    let doc = json!({ "models": entries });
    std::fs::create_dir_all(models).unwrap();
    std::fs::write(models.join("catalog.json"), serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
}

/// A server that sends the first `first` bytes of `bytes`, then holds the connection open until
/// the download is cancelled: a download in progress for as long as the test needs.
struct Gate {
    bytes: Vec<u8>,
    first: usize,
    asked: Mutex<Vec<u64>>,
}

struct GateBody {
    bytes: Vec<u8>,
    pos: usize,
    released: Arc<AtomicBool>,
}

impl Transport for Gate {
    fn get(&self, request: &Request, _: &Limits<'_>) -> Result<Reply, TransportError> {
        self.asked.lock().unwrap().push(request.from());
        let body = GateBody { bytes: self.bytes[..self.first].to_vec(), pos: 0, released: Arc::default() };
        Ok(Reply { status: 200, length: Some(self.bytes.len() as u64), content_range: None, body: Box::new(body) })
    }
}

impl Body for GateBody {
    fn read(&mut self, buf: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError> {
        if self.pos < self.bytes.len() {
            let n = buf.len().min(self.bytes.len() - self.pos);
            buf[..n].copy_from_slice(&self.bytes[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        while !limits.cancel.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.released.store(true, Ordering::SeqCst);
        Err(TransportError::Cancelled)
    }
}

struct Fx {
    dir: tempfile::TempDir,
    picker: ScriptedPicker,
    d: Driver,
}

impl Fx {
    fn models(&self) -> PathBuf {
        self.dir.path().join("models")
    }
}

/// The session on the library in `dir`, its models folder `dir/models`, its downloads through
/// `transport`, the Folder Manager's places planted under `dir/home`.
fn session(dir: &Path, transport: Option<SharedTransport>) -> Session {
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    std::fs::create_dir_all(dir.join("home").join("Pictures")).unwrap();
    s.set_places(Places::under(&dir.join("home")), Vec::new());
    s.set_face_model_transport(transport);
    s.set_face_models_dir(Some(dir.join("models")));
    s
}

/// "Harbour" (three photos) and "Hills" (two), added and scanned, in a 1400 × 900 window with a
/// scripted picker; a `catalog.json` naming [`MINE`] when `bytes` are given.
fn fixture(transport: Option<SharedTransport>, bytes: Option<&[u8]>) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    if let Some(b) = bytes {
        catalog(&dir.path().join("models"), b);
    }
    let photos = dir.path().join("photos");
    for (folder, n, seed) in [("Harbour", 3, 1), ("Hills", 2, 10)] {
        for i in 0..n {
            put_jpeg(&photos.join(folder).join(format!("IMG_{i}.jpg")), seed + i);
        }
    }
    let mut s = session(dir.path(), transport);
    for folder in ["Harbour", "Hills"] {
        s.execute("folders.add", &json!({ "path": photos.join(folder) })).unwrap();
    }
    s.wait_idle();
    assert!(s.wait_face_models(WAIT));
    let picker = ScriptedPicker::new();
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(picker.clone())), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, picker, d }
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap();
    d.settle(FRAMES).unwrap();
}

/// Steps until `done` holds (the engine's workers run on their own threads), then settles.
fn until(d: &mut Driver, what: &str, mut done: impl FnMut(&Driver) -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done(d) {
        assert!(Instant::now() < deadline, "{what}: never happened");
        std::thread::sleep(Duration::from_millis(2));
        d.step();
    }
    d.settle(FRAMES).unwrap();
}

/// The face model workers idle (the models folder read again), then a settled window.
fn models_idle(d: &mut Driver) {
    until(d, "the face models idle", |d| d.app().session().unwrap().face_models().idle());
}

/// Model `id` as `faces.models.list` gives it.
fn listed(d: &mut Driver, id: &str) -> Value {
    let list = d.run("faces.models.list", &json!({})).unwrap();
    list["models"].as_array().unwrap().iter().find(|m| m["id"] == json!(id)).cloned().unwrap_or(Value::Null)
}

fn top_kind(d: &Driver) -> Option<DialogKind> {
    d.state().dialogs.last().map(|dlg| dlg.kind)
}

/// Shots of the window now at 1400 × 900 and at 900 × 600 (`<name>-1400x900`, `-900x600`), the
/// small one checked to fit `id`; the window is large again after.
fn shots(d: &mut Driver, name: &str, id: &str) {
    shot(d, &format!("{name}-1400x900"));
    check_small(d, id);
    shot(d, &format!("{name}-900x600"));
    d.resize(LARGE.0, LARGE.1);
    d.settle(FRAMES).unwrap();
}

/// The photo ids of folder `name`, in order.
fn photos_in(d: &Driver, name: &str) -> Vec<PhotoId> {
    let lib = d.app().session().unwrap().library();
    let folder = lib.folders().find(|f| f.path.ends_with(name)).unwrap().id;
    let mut ids: Vec<PhotoId> = lib.photos().filter(|p| p.folder == folder).map(|p| p.id).collect();
    ids.sort();
    ids
}

fn face_count(d: &Driver, ids: &[PhotoId]) -> usize {
    let lib = d.app().session().unwrap().library();
    ids.iter().map(|id| lib.faces(*id).len()).sum()
}

/// One unnamed face planted on each of `ids`.
fn plant_faces(d: &mut Driver, ids: &[PhotoId]) {
    for id in ids {
        d.run("faces.add", &json!({ "id": id, "rect": [0.2, 0.2, 0.45, 0.5] })).unwrap();
    }
    d.settle(FRAMES).unwrap();
}

#[test]
fn the_dialog_lists_the_files_with_their_terms_and_fits_the_small_window() {
    let mut fx = fixture(None, None);
    let d = &mut fx.d;
    d.run("faceModel.open", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let fields = &d.state().dialogs.last().unwrap().fields;
    assert_eq!(fields[face_model::MODELS], json!(["yunet-2023mar", "sface-2021dec"]), "the detector and the default recogniser");
    for id in ["yunet-2023mar", "sface-2021dec"] {
        let row = widget(d, &face_model::row_id(id)).label;
        assert!(row.contains("Licence: ") && row.contains(" · commercial use "), "{row}");
        assert!(widget(d, &face_model::page_id(id)).enabled, "{id}: Open page");
    }
    assert!(!widget(d, DOWNLOAD).enabled, "Download waits for the box");
    assert!(!widget(d, face_model::ACCEPT_ID).active);
    check_region(d, DIALOG, "dialog-face-model-1400x900");
    shots(d, "face-model-list", DIALOG);
    click(d, face_model::ACCEPT_ID);
    assert!(widget(d, DOWNLOAD).enabled, "ticked: Download is enabled");
    shots(d, "face-model-accepted", DIALOG);
    // Open page puts the address on the clipboard and says so; nothing is fetched.
    click(d, &face_model::page_id("sface-2021dec"));
    assert!(d.state().status.contains("https://"), "{}", d.state().status);
    assert!(!fx.models().join(".downloads").exists(), "nothing was downloaded");
}

#[test]
fn download_waits_for_the_box_shows_its_progress_stops_and_installs() {
    let bytes = good_model();
    let gate = Arc::new(Gate { bytes: bytes.clone(), first: 100, asked: Mutex::new(Vec::new()) });
    let mut fx = fixture(Some(gate.clone() as SharedTransport), Some(&bytes));
    let models = fx.models();
    let d = &mut fx.d;
    d.run("faceModel.open", &json!({ "entry": MINE })).unwrap();
    d.settle(FRAMES).unwrap();
    let row = widget(d, &face_model::row_id(MINE)).label;
    assert!(row.contains("Research use only") && row.contains("not for commercial use"), "{row}");
    assert!(d.click(DOWNLOAD).is_err(), "Download is disabled until the box is ticked");
    assert!(dalica_ui::dialogs::answer(d.app_mut(), Some("download")).is_err());
    d.settle(FRAMES).unwrap();
    assert!(gate.asked.lock().unwrap().is_empty(), "nothing is requested before the terms are accepted");

    click(d, face_model::ACCEPT_ID);
    d.click(DOWNLOAD).unwrap();
    until(d, "the progress shows", |d| {
        try_widget(d, face_model::PROGRESS_ID).is_some() && d.state().face_model.as_ref().and_then(|f| f.progress).is_some_and(|p| p > 0.0)
    });
    assert!(widget(d, face_model::PROGRESS_ID).label.starts_with("Downloading My recogniser"), "{}", widget(d, face_model::PROGRESS_ID).label);
    assert!(!widget(d, DOWNLOAD).enabled, "one download at a time");
    assert_eq!(widget(d, "button:dialog.cancel").label, "Close", "Cancel reads Close while it downloads");
    assert!(widget(d, &face_model::row_id(MINE)).label.contains("Downloading…"), "its state");
    shots(d, "face-model-download", DIALOG);

    // × stops it; what it fetched is kept.
    click(d, face_model::STOP_ID);
    until(d, "the download stops", |d| d.app().session().unwrap().face_models().downloading().is_none());
    models_idle(d);
    assert!(try_widget(d, face_model::PROGRESS_ID).is_none());
    assert_eq!(listed(d, MINE)["state"], json!("partial"));
    assert!(widget(d, DOWNLOAD).enabled, "Download again resumes");

    // The rest from a server that sends it all.
    let server = Arc::new(MemoryTransport::new());
    server.serve(URL, Served::new(bytes.clone()));
    d.app_mut().session_mut().unwrap().set_face_model_transport(Some(server.clone()));
    click(d, DOWNLOAD);
    until(d, "the model is installed", |d| {
        let fm = d.app().session().unwrap().face_models();
        fm.idle() && fm.listing().is_some_and(|l| l.installed.iter().any(|m| m.manifest.id == MINE))
    });
    assert_eq!(server.requests()[0].from, 100, "it resumed");
    assert!(models.join(MINE).join("model.onnx").exists());
    assert_eq!(widget(d, "button:dialog.cancel").label, "Close", "every file installed");
    click(d, "button:dialog.cancel");
    d.run("app.preferences", &json!({ "tab": "people" })).unwrap();
    d.settle(FRAMES).unwrap();
    let status = widget(d, &people::model_id(MINE)).label;
    assert!(status.contains("Installed · in use"), "{status}");
}

#[test]
fn a_local_file_installs_after_the_dialog_and_remove_asks_then_removes_it() {
    let mut fx = fixture(None, None);
    let file = fx.dir.path().join("local.onnx");
    std::fs::write(&file, good_model()).unwrap();
    let models = fx.models();
    let d = &mut fx.d;
    d.run("app.preferences", &json!({ "tab": "people" })).unwrap();
    d.settle(FRAMES).unwrap();
    fx.picker.push_file(&file);
    click(d, people::ADD_FILE_ID);
    until(d, "the file is looked at", |d| {
        top_kind(d) == Some(DialogKind::FaceModel) && d.widgets().iter().any(|w| w.id.starts_with("text:dialog.model/"))
    });
    assert_eq!(d.state().face_model.as_ref().and_then(|f| f.file.clone()), Some(file.clone()));
    let row = d.widgets().into_iter().find(|w| w.id.starts_with("text:dialog.model/")).unwrap();
    let id = row.id.trim_start_matches("text:dialog.model/").to_string();
    assert!(row.label.contains("Unknown licence") || row.label.contains("Unknown"), "{}", row.label);
    assert!(!widget(d, "button:dialog.install").enabled, "Add Model waits for the box");
    shots(d, "face-model-file", DIALOG);
    click(d, face_model::ACCEPT_ID);
    click(d, "button:dialog.install");
    assert_eq!(top_kind(d), Some(DialogKind::Options), "the dialog closed over Options");
    until(d, "the file is installed", |d| {
        let fm = d.app().session().unwrap().face_models();
        fm.idle() && fm.listing().is_some_and(|l| l.installed.iter().any(|m| m.manifest.id == id))
    });
    let status = widget(d, &people::model_id(&id)).label;
    assert!(status.contains("Installed · in use"), "the status shows it: {status}");
    shots(d, "options-people-installed", OPTIONS);

    // Remove asks first; Cancel keeps it, Remove removes it.
    click(d, &people::model_button(&id, "remove"));
    assert_eq!(top_kind(d), Some(DialogKind::RemoveFaceModel));
    shots(d, "face-model-remove", "dialog:removeFaceModel");
    click(d, "button:dialog.cancel");
    models_idle(d);
    assert!(models.join(&id).join("model.onnx").exists(), "Cancel keeps the model");
    click(d, &people::model_button(&id, "remove"));
    click(d, "button:dialog.remove");
    models_idle(d);
    assert!(!models.join(&id).join("model.onnx").exists() && !models.join(&id).join("face-model.json").exists(), "Remove deletes the model");
    assert!(try_widget(d, &people::model_id(&id)).is_none(), "and its row");
}

#[test]
fn the_confidence_sliders_move_by_five_and_survive_a_restart() {
    let mut fx = fixture(None, None);
    let d = &mut fx.d;
    d.run("app.preferences", &json!({ "tab": "people" })).unwrap();
    d.settle(FRAMES).unwrap();
    for (key, id) in [(people::FIND_FACES, "check:option.people.findFaces"), (people::STORE_NAMES, "check:option.people.storeNamesInPhotos")] {
        assert!(widget(d, id).enabled, "{key}");
    }
    check_region(d, OPTIONS, "dialog-options-people-1400x900");
    shots(d, "options-people", OPTIONS);
    let draft = |d: &Driver, key: &str| d.state().options.as_ref().unwrap().values.get(key).cloned();
    let suggestion = "slider:option.people.suggestionConfidence";
    let grouping = "slider:option.people.groupingConfidence";
    // The arrow keys over a slider move it one step of 5, never past 95.
    d.hover(suggestion, 50).unwrap();
    for (key, want) in [("Up", 90), ("Up", 95), ("Up", 95), ("Down", 90)] {
        d.key(key).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(draft(d, people::SUGGESTION_CONFIDENCE), Some(json!(want)), "{key}");
    }
    // A typed number is rounded to a step and kept within 50 … 95.
    for (typed, want) in [("52", 50), ("20", 50), ("73", 75)] {
        d.click(&format!("{grouping}/value")).unwrap();
        d.key("Ctrl+A").unwrap();
        d.type_text(typed).unwrap();
        d.key("Tab").unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(draft(d, people::GROUPING_CONFIDENCE), Some(json!(want)), "typed {typed}");
    }
    click(d, "button:dialog.apply");

    // Find faces turned off: once applied, the offer to delete the face data; Keep keeps it.
    click(d, "check:option.people.findFaces");
    click(d, "button:dialog.apply");
    assert_eq!(top_kind(d), Some(DialogKind::DropFaceData));
    assert_eq!(widget(d, "button:dialog.cancel").label, "Keep");
    shots(d, "options-people-drop-data", "dialog:dropFaceData");
    click(d, "button:dialog.cancel");
    assert_eq!(top_kind(d), Some(DialogKind::Options));
    click(d, "button:dialog.ok");

    // A restart reads them back.
    let dir = fx.dir.path().to_path_buf();
    let app = std::mem::replace(&mut fx.d, new_driver(App::new(None), 200, 200, 1.0)).into_app();
    drop(app);
    let mut d = new_driver(App::new(Some(session(&dir, None))), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    let list = d.run("options.list", &json!({ "tab": "people" })).unwrap();
    let value = |key: &str| list["options"].as_array().unwrap().iter().find(|o| o["key"] == json!(key)).unwrap()["value"].clone();
    assert_eq!(
        (value(people::SUGGESTION_CONFIDENCE), value(people::GROUPING_CONFIDENCE), value(people::FIND_FACES)),
        (json!(90), json!(75), json!(false))
    );
    d.run("app.preferences", &json!({ "tab": "people" })).unwrap();
    d.settle(FRAMES).unwrap();
    // The slider starts from the saved 90: one step up is 95.
    d.hover(suggestion, 50).unwrap();
    d.key("Up").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(draft(&d, people::SUGGESTION_CONFIDENCE), Some(json!(95)), "the slider shows 90 again");
}

#[test]
fn remove_faces_drops_the_folders_faces_and_keep_faces_keeps_them() {
    let mut fx = fixture(None, None);
    let d = &mut fx.d;
    let (harbour, hills) = (photos_in(d, "Harbour"), photos_in(d, "Hills"));
    plant_faces(d, &harbour[..2]);
    plant_faces(d, &hills[..1]);
    let harbour_folder = d.app().session().unwrap().library().folders().find(|f| f.path.ends_with("Harbour")).unwrap().id;
    // The Folder Manager on Harbour, its faces turned off, OK.
    let turn_faces = |d: &mut Driver| {
        d.app_mut().state_mut().folder = Some(harbour_folder);
        d.run("folders.manager", &json!({})).unwrap();
        d.settle(FRAMES).unwrap();
        click(d, FACES_ID);
        click(d, "button:dialog.ok");
    };
    turn_faces(d);
    assert_eq!(top_kind(d), Some(DialogKind::RemoveFaces), "OK asks about the faces found");
    let text = d.state().dialogs.last().unwrap().command.clone().unwrap();
    assert_eq!(text.id, "faces.removeInFolders");
    assert_eq!(widget(d, "button:dialog.cancel").label, "Keep Faces");
    assert_eq!(widget(d, "button:dialog.removeFaces").label, "Remove Faces");
    shots(d, "folder-manager-remove-faces", "dialog:removeFaces");
    click(d, "button:dialog.cancel");
    assert!(d.state().dialogs.is_empty());
    assert_eq!((face_count(d, &harbour), face_count(d, &hills)), (2, 1), "Keep Faces keeps them");

    // On again (no question), off again, and Remove Faces.
    turn_faces(d);
    assert!(d.state().dialogs.is_empty(), "turning faces on asks nothing");
    turn_faces(d);
    assert_eq!(top_kind(d), Some(DialogKind::RemoveFaces));
    click(d, "button:dialog.removeFaces");
    assert_eq!((face_count(d, &harbour), face_count(d, &hills)), (0, 1), "Remove Faces drops Harbour's, Hills keeps its own");
    d.run("edit.undo", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(face_count(d, &harbour), 2, "one undo step brings them back");

    // Picture ▸ Find Faces Again and Tools ▸ Experimental ▸ Write People Names to Photos… act on the
    // selection; the second asks first.
    d.run("select.set", &json!({ "ids": [harbour[0], harbour[1]] })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(dalica_ui::keys::params_for(d.app(), "faces.reset"), json!({ "ids": [harbour[0], harbour[1]] }));
    dalica_ui::keys::run_key(d.app_mut(), "faces.writeXmp").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(top_kind(d), Some(DialogKind::WritePeopleNames));
    let question = d.state().dialogs.last().unwrap().command.clone().unwrap();
    assert_eq!((question.id.as_str(), &question.params["ids"]), ("faces.writeXmp", &json!([harbour[0], harbour[1]])));
    shots(d, "write-people-names", "dialog:writePeopleNames");
    click(d, "button:dialog.write");
    assert!(d.state().dialogs.is_empty(), "Write Names ran: {}", d.state().status);
}

#[test]
fn the_dialog_is_offered_by_the_first_open_of_people_and_by_find_faces_turned_on() {
    let mut fx = fixture(None, None);
    let d = &mut fx.d;
    d.run("pane.people", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(top_kind(d), Some(DialogKind::FaceModel), "People opened without a model offers it");
    assert_eq!(d.state().dialogs.last().unwrap().fields[face_model::MODELS], json!(["yunet-2023mar", "sface-2021dec"]));
    click(d, "button:dialog.cancel");
    for _ in 0..2 {
        d.run("pane.people", &json!({})).unwrap();
        d.settle(FRAMES).unwrap();
    }
    assert!(d.state().dialogs.is_empty(), "once a run");
    // Find faces turned off then on again: once applied, the dialog is offered over Options.
    d.run("faces.enable", &json!({ "on": false, "dropEmbeddings": true })).unwrap();
    d.run("app.preferences", &json!({ "tab": "people" })).unwrap();
    d.settle(FRAMES).unwrap();
    click(d, "check:option.people.findFaces");
    click(d, "button:dialog.apply");
    assert_eq!(top_kind(d), Some(DialogKind::FaceModel));
    assert!(d.app().session().unwrap().settings().people().find_faces);
}

#[test]
fn the_faces_filter_shows_only_the_photos_with_faces() {
    let mut fx = fixture(None, None);
    let d = &mut fx.d;
    let (harbour, hills) = (photos_in(d, "Harbour"), photos_in(d, "Hills"));
    plant_faces(d, &[harbour[1], hills[0]]);
    assert!(widget(d, "button:filter.faces").enabled);
    d.click("button:filter.faces").unwrap();
    d.step();
    d.app_mut().session_mut().unwrap().wait_search();
    d.settle(FRAMES).unwrap();
    assert!(widget(d, "button:filter.faces").active);
    let found: BTreeSet<PhotoId> =
        d.app().session().unwrap().search_state().result.as_ref().map(|r| r.photos().iter().copied().collect()).unwrap_or_default();
    assert_eq!(found, [harbour[1], hills[0]].into_iter().collect());
    let shown: BTreeSet<PhotoId> =
        d.app().panels().unwrap().sections.list.iter().filter(|s| s.folder.is_some()).flat_map(|s| s.photos.iter().copied()).collect();
    assert_eq!(shown, found, "the light table shows only them");
    shots(d, "faces-filter", "region:lightTable");
    d.click("button:filter.faces").unwrap();
    d.step();
    d.app_mut().session_mut().unwrap().wait_search();
    d.settle(FRAMES).unwrap();
    assert!(!d.app().session().unwrap().search_state().active(), "off again: everything");
}

/// A control client without `--automation-allow-external` can tick the box but not start a
/// download: nothing is requested, the reason is on the status line.
#[test]
fn automation_downloads_only_with_the_external_permission() {
    let bytes = good_model();
    let gate = Arc::new(Gate { bytes: bytes.clone(), first: 100, asked: Mutex::new(Vec::new()) });
    let mut fx = fixture(Some(gate.clone() as SharedTransport), Some(&bytes));
    let control = Control::new(Config::default());
    let mut conn = Connection::new();
    let d = &mut fx.d;
    let mut call = |d: &mut Driver, method: &str, params: Value| serve(&control, &mut conn, d, &control::Request::new(method, params));
    call(d, "engine.execute", json!({ "command": "faceModel.open", "params": { "entry": MINE } })).unwrap();
    call(d, "ui.wait", json!({ "ms": 50 })).unwrap();
    call(d, "ui.clickWidget", json!({ "id": face_model::ACCEPT_ID })).unwrap();
    assert!(d.state().face_model.as_ref().is_some_and(|f| f.accepted));
    let _ = call(d, "ui.clickWidget", json!({ "id": DOWNLOAD }));
    d.settle(FRAMES).unwrap();
    assert!(d.state().status.contains("--automation-allow-external"), "{}", d.state().status);
    assert!(gate.asked.lock().unwrap().is_empty(), "nothing was requested");
    assert!(d.app().session().unwrap().face_models().downloading().is_none());
}

/// Download fetches the dialog's files one after another until every one is installed.
#[test]
fn download_fetches_the_dialogs_files_one_after_another() {
    let (a, b) = (good_model(), shaped(8));
    let url_b = "https://models.test/weights/other.onnx";
    let server = Arc::new(MemoryTransport::new());
    server.serve(URL, Served::new(a.clone()));
    server.serve(url_b, Served::new(b.clone()));
    let mut fx = fixture(Some(server.clone() as SharedTransport), None);
    catalog_of(&fx.models(), &[(MINE, URL, &a, 16), ("other-recogniser", url_b, &b, 8)]);
    let models = fx.models();
    let d = &mut fx.d;
    d.run("faces.models.list", &json!({ "refresh": true })).unwrap();
    models_idle(d);
    d.run("faceModel.open", &json!({ "entry": MINE })).unwrap();
    d.settle(FRAMES).unwrap();
    // The dialog's files as the dialog without an entry fixes them, here two of the catalog's.
    d.app_mut().state_mut().dialogs.last_mut().unwrap().fields.insert(face_model::MODELS.into(), json!([MINE, "other-recogniser"]));
    click(d, face_model::ACCEPT_ID);
    d.click(DOWNLOAD).unwrap();
    until(d, "both are installed", |d| {
        let fm = d.app().session().unwrap().face_models();
        fm.idle() && fm.listing().is_some_and(|l| l.installed.len() == 2)
    });
    let asked: Vec<String> = server.requests().iter().map(|r| r.url.clone()).collect();
    assert_eq!(asked, [URL, url_b], "one after the other, each once");
    assert!(models.join("other-recogniser").join("model.onnx").exists());
    assert_eq!(d.state().dialogs.last().unwrap().fields[face_model::QUEUE], json!(false), "the queue ended");
    assert_eq!(widget(d, "button:dialog.cancel").label, "Close");
}
