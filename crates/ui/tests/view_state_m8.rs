//! M8's view state, UI commands, the save picker and the stub dialogs (M8.4), driven through the
//! control channel as an agent would: every new field (`export`, `saveExplain`, `revert`,
//! `exportProgress`) round-trips through `ui.state` / `ui.set` (bad values are `bad_params`, ids
//! that do not exist `not_found`, and nothing changes); Ctrl+Shift+S with a tray opens
//! `dialog:export` and Esc closes it; File ▸ Save As… opens the save picker, whose answers are
//! checked by the automation root policy and whose cancel changes nothing; an `export.run` path
//! outside every write root is `path_denied`; the File menu's Save As… and Export rows are live;
//! Save and Revert ask through the host; the new commands fail gracefully; every interactive widget
//! of the new dialogs has a valid, unique id.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_engine::Session;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A session on a library under `dir` holding one folder of 4 scenes photos; their ids and the
/// folder's id.
fn session(dir: &Path) -> (Session, Vec<u64>, u64) {
    let files = dalica_scenes::demo_library(1, 4, 11).unwrap();
    let mut roots = std::collections::BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    // The user's Pictures folder inside the temp dir, so the Export dialog's default location
    // never names the machine's own.
    s.set_pictures_dir(Some(dir.join("Pictures")));
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let photos = found["photos"].as_array().unwrap();
    let ids: Vec<u64> = photos.iter().map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 4);
    let folder = s.library().photo(dalica_core::PhotoId::new(ids[0])).unwrap().folder.get();
    (s, ids, folder)
}

struct T {
    dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    ids: Vec<u64>,
    folder: u64,
}

/// The out folder: the one automation write root of these tests.
fn out(dir: &Path) -> PathBuf {
    dir.join("out")
}

fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids, folder) = session(dir.path());
    std::fs::create_dir_all(out(dir.path())).unwrap();
    let control = Control::new(Config { write_roots: vec![out(dir.path())], ..Config::default() });
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { dir, control, conn: Connection::new(), d, ids, folder }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn code(t: &mut T, method: &str, params: Value) -> &'static str {
    match call(t, method, params.clone()) {
        Ok(v) => panic!("{method} {params} succeeded: {v}"),
        Err(e) => e.code.as_str(),
    }
}

/// The view state without `status` (the status line's text, which a frame may rewrite).
fn state(t: &mut T) -> Value {
    let mut v = ok(t, "ui.state", json!({}));
    v.as_object_mut().unwrap().remove("status");
    v
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    ok(t, "engine.execute", json!({ "command": id, "params": params }))
}

fn run_code(t: &mut T, id: &str, params: Value) -> &'static str {
    code(t, "engine.execute", json!({ "command": id, "params": params }))
}

fn key(t: &mut T, combo: &str) {
    ok(t, "ui.key", json!({ "combo": combo }));
    t.d.settle(FRAMES).unwrap();
}

fn kinds(t: &mut T) -> Vec<String> {
    state(t)["dialogs"].as_array().unwrap().iter().map(|d| d["kind"].as_str().unwrap().to_string()).collect()
}

/// Waits (off the frame loop) for the stubbed picker to show a request.
fn picker_open(t: &T) -> bool {
    (0..2500).any(|_| {
        if t.control.stub().waiting().is_some() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        false
    })
}

/// Each new field round-trips through `ui.set` and `ui.state`; `export` merges field by field;
/// resetting them gives back the defaults and moves nothing else.
#[test]
fn every_m8_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup((1400, 900));
    let defaults = state(&mut t);
    for field in ["export", "saveExplain", "revert", "exportProgress"] {
        assert_eq!(defaults[field], Value::Null, "{field}");
    }
    let job = ok(&mut t, "jobs.list", json!({}))[0]["job"].as_u64().expect("the folder's scan is listed");
    let export = json!({
        "ids": [t.ids[1], t.ids[0]], "section": null, "location": "/tmp/Exports", "folderName": "Lisbon Weekend",
        "numbers": true, "originalSize": false, "resize": "1280", "quality": "custom", "custom": 60,
        "videos": "fullVideo", "watermark": true, "watermarkText": "© Ana Costa 2026"
    });
    let full = json!({
        "export": export,
        "saveExplain": { "dontShow": true },
        "revert": { "photos": [t.ids[2]] },
        "exportProgress": { "job": job, "shown": true }
    });
    let v = ok(&mut t, "ui.set", full.clone());
    for field in ["export", "saveExplain", "revert", "exportProgress"] {
        assert_eq!(v[field], full[field], "{field}");
    }
    assert_eq!(state(&mut t)["export"], export);
    // `export` merges field by field.
    for (field, value) in [
        ("quality", json!("maximum")),
        ("resize", json!("abc")),
        ("originalSize", json!(true)),
        ("section", json!(format!("folder:{}", t.folder))),
        ("ids", json!([])),
        ("watermarkText", json!("")),
    ] {
        let v = ok(&mut t, "ui.set", json!({ "export": { field: value.clone() } }));
        assert_eq!(v["export"][field], value, "{field}");
        assert_eq!(v["export"]["folderName"], "Lisbon Weekend", "{field} keeps the others");
    }
    // A partial draft from nothing takes the dialog's defaults; repeated ids are kept once.
    ok(&mut t, "ui.set", json!({ "export": null }));
    let last = t.ids[3];
    let v = ok(&mut t, "ui.set", json!({ "export": { "ids": [last, last] } }));
    assert_eq!(
        v["export"],
        json!({ "ids": [last], "section": null, "location": "", "folderName": "", "numbers": false, "originalSize": false,
                "resize": "1600", "quality": "automatic", "custom": 75, "videos": "firstFrame", "watermark": false, "watermarkText": "" })
    );
    // The progress window, shown: drawn without a backdrop, so the window under it keeps its
    // input (a click selects a photo); hidden: gone.
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.widgets().iter().any(|w| w.id == "dialog:exportProgress"));
    assert!(!t.d.widgets().iter().any(|w| w.id.ends_with("/backdrop")));
    let photo = t.ids[0];
    t.d.click(&format!("thumb:{photo}")).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(state(&mut t)["selection"]["ids"], json!([photo]), "the window under it takes clicks");
    let v = ok(&mut t, "ui.set", json!({ "exportProgress": { "job": job, "shown": false } }));
    assert_eq!(v["exportProgress"], json!({ "job": job, "shown": false }));
    t.d.settle(FRAMES).unwrap();
    assert!(!t.d.widgets().iter().any(|w| w.id == "dialog:exportProgress"));
    run(&mut t, "select.none", json!({}));
    // Back to the defaults: nothing else moved.
    let back = ok(&mut t, "ui.set", json!({ "export": null, "saveExplain": null, "revert": null, "exportProgress": null, "folder": null }));
    let mut back = back.clone();
    back.as_object_mut().unwrap().remove("status");
    assert_eq!(back, defaults);
}

/// Bad values are `bad_params`, photos, folders and jobs that do not exist `not_found`; nothing
/// changes either way.
#[test]
fn bad_m8_values_are_refused_and_change_nothing() {
    let mut t = setup((1400, 900));
    let before = state(&mut t);
    let long = "x".repeat(5000);
    for bad in [
        json!({ "export": { "quality": "best" } }),
        json!({ "export": { "custom": 0 } }),
        json!({ "export": { "custom": 101 } }),
        json!({ "export": { "custom": "75" } }),
        json!({ "export": { "resize": 1600 } }),
        json!({ "export": { "resize": "1".repeat(17) } }),
        json!({ "export": { "folderName": long } }),
        json!({ "export": { "watermarkText": long } }),
        json!({ "export": { "location": "/".repeat(4097) } }),
        json!({ "export": { "videos": "gif" } }),
        json!({ "export": { "section": "album:x" } }),
        json!({ "export": { "zzz": 1 } }),
        json!({ "export": [1] }),
        json!({ "saveExplain": { "dontShow": "yes" } }),
        json!({ "saveExplain": { "zzz": true } }),
        json!({ "revert": { "photos": "all" } }),
        json!({ "revert": { "photos": [-1] } }),
        json!({ "exportProgress": { "job": 1 } }),
        json!({ "exportProgress": { "job": "one", "shown": true } }),
        json!({ "exportProgress": 3 }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad} changed nothing");
    }
    for bad in [
        json!({ "export": { "ids": [999_999] } }),
        json!({ "export": { "section": "folder:999999" } }),
        json!({ "revert": { "photos": [999_999] } }),
        json!({ "exportProgress": { "job": 999_999, "shown": true } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "not_found", "{bad}");
        assert_eq!(state(&mut t), before, "{bad} changed nothing");
    }
}

/// Ctrl+Shift+S with a tray opens `dialog:export` with a draft for the tray's photos; Esc closes it
/// and drops the draft. Without a tray it opens nothing.
#[test]
fn ctrl_shift_s_with_a_tray_opens_the_export_dialog_and_esc_closes_it() {
    let mut t = setup((1400, 900));
    key(&mut t, "Ctrl+Shift+S");
    assert!(kinds(&mut t).is_empty(), "an empty tray exports nothing");
    let (a, b) = (t.ids[0], t.ids[1]);
    run(&mut t, "select.set", json!({ "ids": [a, b] }));
    t.d.settle(FRAMES).unwrap();
    key(&mut t, "Ctrl+Shift+S");
    assert_eq!(kinds(&mut t), ["export"]);
    let v = ok(&mut t, "ui.inspect", json!({}));
    assert_eq!((v["dialog"]["kind"].as_str(), v["dialog"]["title"].as_str()), (Some("export"), Some("Export to Folder")));
    assert_eq!(v["export"]["ids"], json!([a, b]));
    assert!(!v["export"]["folderName"].as_str().unwrap().is_empty(), "the photos' folder names it");
    let widgets = t.d.widgets();
    assert!(widgets.iter().any(|w| w.id == "dialog:export"));
    // The default location (under the Pictures folder): Export is enabled; with no location, not.
    assert_eq!(PathBuf::from(v["export"]["location"].as_str().unwrap()), t.dir.path().join("Pictures").join("Dalica").join("Exports"));
    assert!(widgets.iter().find(|w| w.id == "button:dialog.export").unwrap().enabled);
    ok(&mut t, "ui.set", json!({ "export": { "location": "" } }));
    t.d.settle(FRAMES).unwrap();
    assert!(!t.d.widgets().iter().find(|w| w.id == "button:dialog.export").unwrap().enabled, "no location, no Export");
    key(&mut t, "Escape");
    assert!(kinds(&mut t).is_empty());
    assert_eq!(state(&mut t)["export"], Value::Null);
    // The menu row opens it too, and Cancel closes it the same way.
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "File/Export Pictures to Folder" })), json!({ "dialog": "export" }));
    assert_eq!(ok(&mut t, "ui.dialog.cancel", json!({})), json!({}));
    assert_eq!((kinds(&mut t), state(&mut t)["export"].clone()), (Vec::<String>::new(), Value::Null));
}

/// File ▸ Save As… opens the save picker (the stub over automation): its answers are checked as new
/// files (inside a write root or a watched folder, never over a file), and a cancel changes
/// nothing.
#[test]
fn save_as_opens_the_save_picker_and_cancel_changes_nothing() {
    let mut t = setup((1400, 900));
    let photo = t.ids[0];
    run(&mut t, "select.set", json!({ "ids": [photo] }));
    let item = |t: &mut T| {
        let tree = ok(t, "ui.menu.tree", json!({}));
        let file = tree.as_array().unwrap().iter().find(|m| m["label"] == "File").unwrap().clone();
        file["children"].as_array().unwrap().iter().find(|i| i["id"] == "photo.saveAs").unwrap().clone()
    };
    let row = item(&mut t);
    assert_eq!((row["live"].clone(), row["enabled"].clone(), row["dialog"].clone()), (json!(true), json!(false), json!("filePicker")));
    assert_eq!(code(&mut t, "ui.menu.invoke", json!({ "path": "File/Save As" })), "disabled", "no edits yet");
    run(&mut t, "photo.history.apply", json!({ "id": photo, "op": "rotate(90)" }));
    assert_eq!(item(&mut t)["enabled"], json!(true));
    let before = state(&mut t);
    let library = run(&mut t, "library.query", json!({ "limit": 1000 }));
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "File/Save As" })), json!({ "dialog": "filePicker" }));
    assert!(picker_open(&t), "the picker opened");
    let waiting = ok(&mut t, "ui.inspect", json!({}))["filePicker"].clone();
    assert_eq!((&waiting["command"], &waiting["kind"], &waiting["title"]), (&json!("photo.saveTo"), &json!("save"), &json!("Save As")));
    assert_eq!(waiting["params"], json!({ "id": photo }));
    assert!(waiting["name"].as_str().unwrap().ends_with(".jpg"), "{waiting}");
    // One dialog at a time.
    assert_eq!(item(&mut t)["enabled"], json!(false));
    // Answers outside the write roots and watched folders, or over a file, are refused; the picker
    // stays open.
    let elsewhere = t.dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    assert_eq!(code(&mut t, "ui.filePicker", json!({ "path": elsewhere.join("copy.jpg") })), "path_denied");
    let out_dir = out(t.dir.path());
    std::fs::write(out_dir.join("taken.jpg"), b"x").unwrap();
    assert_eq!(code(&mut t, "ui.filePicker", json!({ "path": out_dir.join("taken.jpg") })), "path_denied");
    assert_eq!(code(&mut t, "ui.filePicker", json!({ "path": "copy.jpg" })), "bad_params");
    assert!(t.control.stub().waiting().is_some(), "a refused answer leaves the picker open");
    // Cancel: nothing written, nothing changed.
    assert_eq!(ok(&mut t, "ui.filePicker", json!({ "cancel": true })), json!({}));
    t.d.settle(FRAMES).unwrap();
    assert!(t.control.stub().waiting().is_none());
    assert_eq!(state(&mut t), before);
    assert_eq!(run(&mut t, "library.query", json!({ "limit": 1000 })), library);
    assert_eq!(std::fs::read_dir(out(t.dir.path())).unwrap().count(), 1, "only taken.jpg");
    assert_eq!(item(&mut t)["enabled"], json!(true), "the picker closed");
    // A good answer runs photo.saveTo {id, path} (the engine's command arrives with M8.17).
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "id": "photo.saveAs" })), json!({ "dialog": "filePicker" }));
    assert!(picker_open(&t));
    match call(&mut t, "ui.filePicker", json!({ "path": out_dir.join("copy.png") })) {
        Ok(_) => {}
        Err(e) => assert_eq!(e.code.as_str(), "unknown_command", "{e}"),
    }
    assert!(t.control.stub().waiting().is_none());
}

/// The Export dialog's Browse… opens the folder picker (the stub over automation) for the export
/// location: its answer runs `output.export.location`, checked as a place to write, and never adds
/// a watched folder; a cancel changes nothing.
#[test]
fn browse_sets_the_export_location_and_never_adds_a_watched_folder() {
    let mut t = setup((1400, 900));
    let (a, dir) = (t.ids[0], t.dir.path().to_path_buf());
    run(&mut t, "select.set", json!({ "ids": [a] }));
    key(&mut t, "Ctrl+Shift+S");
    assert_eq!(kinds(&mut t), ["export"]);
    let roots = run(&mut t, "folders.list", json!({}));
    let location = state(&mut t)["export"]["location"].clone();
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:dialog.browse" }));
    assert!(picker_open(&t), "Browse… opened the picker");
    let waiting = ok(&mut t, "ui.inspect", json!({}))["filePicker"].clone();
    assert_eq!(
        (&waiting["command"], &waiting["kind"], &waiting["title"]),
        (&json!("output.export.location"), &json!("folder"), &json!("Export Location"))
    );
    // One file dialog at a time: Browse… waits while it is open, which settling reports as busy.
    let s = t.d.settle_for(2 * dalica_ui::headless::QUIET_FRAMES, false);
    assert_eq!(s.busy, ["file dialog: waiting for an answer"], "{s:?}");
    assert!(!t.d.widgets().iter().find(|w| w.id == "button:dialog.browse").unwrap().enabled);
    // A folder outside every write root and watched folder is refused; the picker stays open.
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    assert_eq!(code(&mut t, "ui.filePicker", json!({ "path": elsewhere })), "path_denied");
    assert!(t.control.stub().waiting().is_some());
    // Cancel: nothing changed.
    assert_eq!(ok(&mut t, "ui.filePicker", json!({ "cancel": true })), json!({}));
    t.d.settle(FRAMES).unwrap();
    assert_eq!(state(&mut t)["export"]["location"], location);
    // A folder in the write root (not made yet) becomes the location; no watched folder is added.
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:dialog.browse" }));
    assert!(picker_open(&t));
    let chosen = out(&dir).join("Exports");
    assert_eq!(ok(&mut t, "ui.filePicker", json!({ "path": chosen })), json!({ "location": chosen }));
    t.d.settle(FRAMES).unwrap();
    assert_eq!(state(&mut t)["export"]["location"], json!(chosen));
    assert_eq!(kinds(&mut t), ["export"], "the dialog stays open");
    assert_eq!(run(&mut t, "folders.list", json!({})), roots, "no watched folder added");
    assert!(!chosen.exists(), "nothing made yet");
    // Closed, the location cannot be set.
    key(&mut t, "Escape");
    assert_eq!(run_code(&mut t, "output.export.location", json!({ "path": chosen })), "disabled");
}

/// An `export.run` or `photo.saveTo` path outside every write root and watched folder is
/// `path_denied`, before anything else about the command; one inside passes the policy.
#[test]
fn an_export_path_outside_every_write_root_is_path_denied() {
    let mut t = setup((1400, 900));
    let (photo, dir) = (t.ids[0], t.dir.path().to_path_buf());
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let export = |d: &Path| json!({ "ids": [photo], "dir": d, "folderName": "Out" });
    assert_eq!(run_code(&mut t, "export.run", export(&elsewhere)), "path_denied");
    assert_eq!(run_code(&mut t, "export.run", export(&elsewhere.join("not yet"))), "path_denied");
    assert_eq!(run_code(&mut t, "export.run", export(Path::new("relative"))), "bad_params");
    assert_eq!(run_code(&mut t, "export.run", export(&out(&dir).join("..").join("elsewhere"))), "path_denied");
    assert_eq!(run_code(&mut t, "export.run", json!({ "ids": [photo], "dir": 7 })), "bad_params");
    assert_eq!(run_code(&mut t, "photo.saveTo", json!({ "id": photo, "path": elsewhere.join("a.jpg") })), "path_denied");
    // Inside a write root, or a watched folder, the policy lets it through to the command itself.
    let watched = std::fs::read_dir(dir.join("photos")).unwrap().next().unwrap().unwrap().path();
    for (id, params) in [
        ("export.run", export(&out(&dir).join("Exports"))),
        ("export.run", export(&watched)),
        ("photo.saveTo", json!({ "id": photo, "path": watched.join("Saved As.jpg") })),
        ("photo.saveTo", json!({ "id": photo, "path": out(&dir).join("a.png") })),
    ] {
        match call(&mut t, "engine.execute", json!({ "command": id, "params": params })) {
            Ok(_) => {}
            Err(e) => assert_ne!(e.code.as_str(), "path_denied", "{id}: {e}"),
        }
    }
    // Nothing was written outside.
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

/// The File menu's M8 rows: Save As… and Export are live, enabled by the selection and the tray;
/// Save and Revert ask through the host when their engine commands run without `confirm`.
#[test]
fn the_file_menus_save_and_export_rows_light_up() {
    let mut t = setup((1400, 900));
    let rows = |t: &mut T| -> Vec<Value> {
        let tree = ok(t, "ui.menu.tree", json!({}));
        let file = tree.as_array().unwrap().iter().find(|m| m["label"] == "File").unwrap().clone();
        file["children"].as_array().unwrap().iter().filter(|i| i.get("id").is_some()).cloned().collect()
    };
    let row = |rows: &[Value], id: &str| rows.iter().find(|r| r["id"] == id).unwrap().clone();
    let r = rows(&mut t);
    let export = row(&r, "output.export");
    assert_eq!((export["live"].clone(), export["enabled"].clone(), export["dialog"].clone()), (json!(true), json!(false), json!("export")));
    assert_eq!(export["shortcut"], json!(if cfg!(target_os = "macos") { "Cmd+Shift+S" } else { "Ctrl+Shift+S" }));
    assert_eq!(export["reason"], json!("Put some photos in the tray first."));
    assert_eq!(row(&r, "photo.saveAs")["live"], json!(true));
    assert_eq!(row(&r, "photo.revert")["dialog"], json!("revert"));
    let (a, b) = (t.ids[0], t.ids[1]);
    run(&mut t, "photo.history.apply", json!({ "id": a, "op": "rotate(90)" }));
    run(&mut t, "select.set", json!({ "ids": [a] }));
    let r = rows(&mut t);
    assert_eq!((row(&r, "output.export")["enabled"].clone(), row(&r, "photo.saveAs")["enabled"].clone()), (json!(true), json!(true)));
    run(&mut t, "select.set", json!({ "ids": [a, b] }));
    let r = rows(&mut t);
    assert_eq!((row(&r, "output.export")["enabled"].clone(), row(&r, "photo.saveAs")["enabled"].clone()), (json!(true), json!(false)));
    // engine.commands lists the two UI commands.
    let all = ok(&mut t, "engine.commands", json!({ "filter": "output.export" }));
    assert_eq!(all[0]["ui"], json!(true));
    // Save and Revert ask first: needs_confirm over automation, with the host's prompt.
    for (id, dialog, buttons) in
        [("photo.save", "saveExplain", json!(["Save", "Cancel"])), ("photo.revert", "revert", json!(["Revert", "Undo Save", "Cancel"]))]
    {
        match call(&mut t, "engine.execute", json!({ "command": id, "params": { "ids": [a] } })) {
            Err(e) if e.code.as_str() == "needs_confirm" => {
                let prompt = e.prompt.clone().unwrap();
                assert_eq!((prompt["dialog"].clone(), prompt["buttons"].clone()), (json!(dialog), buttons), "{id}");
            }
            // Revert applies only to a saved photo (M8.15): this one has unsaved edits only.
            Err(e) if id == "photo.revert" => assert_eq!(e.code.as_str(), "disabled", "{id}: {e}"),
            Err(e) => panic!("{id}: {e}"),
            Ok(v) => panic!("{id} ran without asking: {v}"),
        }
    }
}

/// The UI commands refuse bad parameters, ids that do not exist and runs that cannot happen, and
/// change nothing.
#[test]
fn the_new_commands_fail_gracefully() {
    let mut t = setup((1400, 900));
    let before = state(&mut t);
    let folder = format!("folder:{}", t.folder);
    for (id, params, want) in [
        ("output.export", json!({}), "disabled"),
        ("output.export", json!({ "ids": [999_999] }), "not_found"),
        ("output.export", json!({ "section": "folder:999999" }), "not_found"),
        ("output.export", json!({ "section": 12 }), "bad_params"),
        ("output.export", json!({ "ids": [999_999], "section": folder }), "not_found"),
        ("output.export", json!({ "zzz": true }), "bad_params"),
        ("photo.saveAs", json!({}), "disabled"),
        ("photo.saveAs", json!({ "id": 999_999 }), "not_found"),
        ("photo.saveAs", json!({ "id": t.ids[0] }), "disabled"),
        ("photo.saveAs", json!({ "id": [1] }), "bad_params"),
        ("photo.saveAs", json!({ "zzz": 1 }), "bad_params"),
    ] {
        assert_eq!(run_code(&mut t, id, params.clone()), want, "{id} {params}");
        assert_eq!(state(&mut t), before, "{id} {params} changed nothing");
    }
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "output.export", "params": [1] })), "bad_params");
    assert!(t.control.stub().waiting().is_none(), "no picker opened");
}
