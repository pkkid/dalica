//! Tuning commands (`roadmap.md` M7.18; `editing.md` §6.5, §7) on a temporary library: a Tuning
//! session of any number of changes is one history step that one undo removes; a second session
//! after another edit appends a new entry, and one right after updates the same entry as its own
//! step; a session updates the photo's Tuning in place past rotations and flips and appends after
//! a retouch or an unknown entry (M8.28); the Basic Fixes Fill Light and the Tuning tab edit the
//! same value; the wands and the
//! colour picker answer from a worker without the owner thread touching `fs`; every command's
//! contract and graceful failures.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, CmdError, Effect, History, JobKind, JobState, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(60);

fn open(library: &Path) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(library, Arc::new(SystemClock), &options).unwrap();
    s.wait_idle();
    s
}

/// A library watching one folder `Lisbon` of `n` `width × height` photos `IMG_0001.jpg`…, ids in
/// name order.
fn setup(n: usize, width: u32, height: u32) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64 + 3, width, height), 90, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    photos.sort();
    (lib, disk, folder, s, photos.into_iter().map(|(_, id)| id).collect())
}

fn edits(s: &Session, id: PhotoId) -> Vec<String> {
    s.library().info(id).edits.clone()
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|t| t.to_string()).collect()
}

fn labels(s: &mut Session, id: PhotoId) -> (String, String) {
    let v = s.execute("photo.history.state", &json!({ "id": id })).unwrap();
    (v["undoLabel"].as_str().unwrap().to_string(), v["redoLabel"].as_str().unwrap().to_string())
}

/// One change of a Tuning session: `values` merged into `{id, session}`.
fn tune(s: &mut Session, id: PhotoId, session: Value, values: Value) -> Value {
    let mut p = json!({ "id": id, "session": session });
    if let (Some(o), Some(v)) = (p.as_object_mut(), values.as_object()) {
        o.extend(v.clone());
    }
    s.execute("photo.history.tuning", &p).unwrap()
}

fn sidecar_entry(s: &mut Session, folder: &Path, name: &str) -> Value {
    s.flush().unwrap();
    let v: Value = std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap());
    v["photos"][name].clone()
}

#[test]
fn a_drag_of_sixty_changes_is_one_step_that_one_undo_removes() {
    let (_lib, _d, folder, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    s.execute("photo.history.apply", &json!({ "id": id, "op": "sepia()" })).unwrap();
    for i in 1..=60 {
        // The slider's f32 values, as the UI sends them.
        let v = tune(&mut s, id, json!("drag-1"), json!({ "fill": f64::from(i as f32 / 100.0) }));
        assert_eq!(v["changed"], json!(true), "change {i}");
        assert_eq!(v["edits"].as_array().unwrap().len(), 2, "change {i} updates the session's entry");
    }
    assert_eq!(edits(&s, id), list(&["sepia()", "tuning(0.6,0,0,0,none)"]));
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo".into()));
    assert_eq!(s.undo_stack().len(), 0, "photo steps never reach the library stack");
    assert_eq!(sidecar_entry(&mut s, &folder, "IMG_0001.jpg")["edits"], json!(edits(&s, id)));

    // One undo removes the whole session; redo gives it back whole.
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["sepia()"]));
    assert_eq!(labels(&mut s, id), ("Undo Sepia".into(), "Redo Tuning".into()));
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["sepia()", "tuning(0.6,0,0,0,none)"]));

    // A session's change that keeps the values takes no step and keeps the redo list.
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap();
    let v = tune(&mut s, id, json!("drag-2"), json!({ "fill": 0.6 }));
    assert_eq!((v["changed"].clone(), v["tuning"]["fill"].clone()), (json!(false), json!(0.6)));
    // A first change that would add a Tuning changing nothing takes no step either.
    let (_l2, _d2, _f2, mut t, other) = setup(1, 96, 64);
    let v = tune(&mut t, other[0], json!(1), json!({ "fill": 0, "temp": 0, "neutral": null }));
    assert_eq!((v["changed"].clone(), v["edits"].clone()), (json!(false), json!([])));
}

#[test]
fn a_second_session_after_a_crop_appends_a_new_entry() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    for v in [0.1, 0.2, 0.25] {
        tune(&mut s, id, json!("one"), json!({ "highlights": v }));
    }
    tune(&mut s, id, json!("one"), json!({ "neutral": "#C0B0A0" }));
    assert_eq!(edits(&s, id), list(&["tuning(0,0.25,0,0,#c0b0a0)"]));
    s.execute("photo.crop.apply", &json!({ "id": id, "rect": [0.1, 0.1, 0.8, 0.8] })).unwrap();
    let v = tune(&mut s, id, json!("two"), json!({ "shadows": 0.1 }));
    assert_eq!(v["edits"], json!(["tuning(0,0.25,0,0,#c0b0a0)", "crop(0.1,0.1,0.9,0.9)", "tuning(0,0,0.1,0,none)"]));
    assert_eq!(v["tuning"]["shadows"], json!(0.1));
    tune(&mut s, id, json!("two"), json!({ "temp": -0.5 }));
    assert_eq!(edits(&s, id).len(), 3);
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["tuning(0,0.25,0,0,#c0b0a0)", "crop(0.1,0.1,0.9,0.9)"]));
    assert_eq!(labels(&mut s, id), ("Undo Crop".into(), "Redo Tuning".into()));
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap();

    // A third session straight after updates the last entry, as a step of its own that undoes to
    // the second session's values.
    for v in [0.2, 0.3] {
        tune(&mut s, id, json!("three"), json!({ "fill": v }));
    }
    tune(&mut s, id, json!("three"), json!({ "neutral": [0.5, 0.5, 0.25] }));
    assert_eq!(edits(&s, id).get(2).map(String::as_str), Some("tuning(0.3,0,0.1,-0.5,#808040)"));
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id).get(2).map(String::as_str), Some("tuning(0,0,0.1,-0.5,none)"));
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo Tuning".into()));
    // Clearing the neutral colour with "none" or null.
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap();
    let v = tune(&mut s, id, json!("four"), json!({ "neutral": "none" }));
    assert_eq!(v["tuning"], json!({ "fill": 0.3, "highlights": 0.0, "shadows": 0.1, "temp": -0.5, "neutral": null, "rgb": null }));
    tune(&mut s, id, json!("four"), json!({ "neutral": "#102030" }));
    let v = tune(&mut s, id, json!("four"), json!({ "neutral": null }));
    assert_eq!(v["tuning"]["neutral"], Value::Null);
}

#[test]
fn fill_light_from_basic_fixes_and_from_tuning_edit_the_same_value() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    // The Basic Fixes slider: one drag, one session.
    for v in [0.1, 0.3, 0.4] {
        tune(&mut s, id, json!("basic-1"), json!({ "fill": v }));
    }
    // The Tuning tab opens on the same value and moves another slider: still one entry.
    let v = tune(&mut s, id, json!("tab-1"), json!({}));
    assert_eq!((v["changed"].clone(), v["tuning"]["fill"].clone()), (json!(false), json!(0.4)));
    tune(&mut s, id, json!("tab-1"), json!({ "highlights": 0.2 }));
    tune(&mut s, id, json!("tab-1"), json!({ "fill": 0.15 }));
    assert_eq!(edits(&s, id), list(&["tuning(0.15,0.2,0,0,none)"]));
    // Back on Basic Fixes the slider shows the Tuning tab's value, and moving it keeps the rest.
    let v = tune(&mut s, id, json!("basic-2"), json!({ "fill": 0.5 }));
    assert_eq!(v["tuning"]["fill"], json!(0.5));
    assert_eq!(edits(&s, id), list(&["tuning(0.5,0.2,0,0,none)"]));
    // Three sessions, three steps.
    for expected in [&["tuning(0.15,0.2,0,0,none)"][..], &["tuning(0.4,0,0,0,none)"], &[]] {
        s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
        assert_eq!(edits(&s, id), list(expected));
    }

    // An older sidecar's fill_light(a) is Tuning with only `fill`: a session updates it in place.
    let (_l2, _d2, _f2, mut t, other) = setup(1, 96, 64);
    let id = other[0];
    t.execute("photo.history.apply", &json!({ "id": id, "op": "fill_light(0.3)" })).unwrap();
    let v = tune(&mut t, id, json!("tab"), json!({}));
    assert_eq!(v["tuning"]["fill"], json!(0.3));
    tune(&mut t, id, json!("tab"), json!({ "fill": 0.3 }));
    assert_eq!(edits(&t, id), list(&["fill_light(0.3)"]), "the same value rewrites nothing");
    tune(&mut t, id, json!("tab"), json!({ "temp": 0.5 }));
    assert_eq!(edits(&t, id), list(&["tuning(0.3,0,0,0.5,none)"]));
    t.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&t, id), list(&["fill_light(0.3)"]));
}

/// M8.28: the photo's Tuning is shown after later edits; a session updates it in place while only
/// rotations and flips follow it (one step, one undo), and appends once a step that depends on its
/// pixels (a retouch) follows.
#[test]
fn a_session_updates_the_tuning_in_place_past_rotations_and_appends_after_a_retouch() {
    let (_lib, _d, folder, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    // Fill Light 0.3 on Basic Fixes, then a rotation in the edit view.
    tune(&mut s, id, json!("basic"), json!({ "fill": 0.3 }));
    s.execute("photo.history.apply", &json!({ "id": id, "op": "rotate(90)" })).unwrap();
    let v = tune(&mut s, id, json!("tab"), json!({}));
    assert_eq!((v["changed"].clone(), v["tuning"]["fill"].clone()), (json!(false), json!(0.3)), "the tab shows 0.3");
    // Dragging Highlights: one entry, updated in place, one step.
    for h in [0.05, 0.1, 0.2] {
        let v = tune(&mut s, id, json!("tab"), json!({ "fill": 0.3, "highlights": h }));
        assert_eq!(v["tuning"]["highlights"], json!(h));
    }
    assert_eq!(edits(&s, id), list(&["tuning(0.3,0.2,0,0,none)", "rotate(90)"]));
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo".into()));
    assert_eq!(sidecar_entry(&mut s, &folder, "IMG_0001.jpg")["edits"], json!(edits(&s, id)));
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["tuning(0.3,0,0,0,none)", "rotate(90)"]), "one undo, the rotation kept");
    assert_eq!(labels(&mut s, id), ("Undo Rotate".into(), "Redo Tuning".into()));
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["tuning(0.3,0.2,0,0,none)", "rotate(90)"]));

    // Flips and a library rotation are position-free too.
    s.execute("edit.flipH", &json!({ "ids": [id] })).unwrap();
    s.execute("photo.rotate.cw", &json!({ "ids": [id] })).unwrap();
    let v = tune(&mut s, id, json!("tab-2"), json!({ "shadows": 0.1 }));
    assert_eq!(v["edits"], json!(["tuning(0.3,0.2,0.1,0,none)", "rotate(90)", "flip_h()", "rotate(90)"]));

    // A retouch depends on the Tuning's pixels: the sliders still show it, and a new drag appends
    // a new entry from the defaults.
    s.execute("photo.history.apply", &json!({ "id": id, "op": "retouch(0.2,0.2,0.6,0.6,0.05)" })).unwrap();
    let v = tune(&mut s, id, json!("tab-3"), json!({}));
    assert_eq!((v["changed"].clone(), v["tuning"]["fill"].clone()), (json!(false), json!(0.3)));
    let v = tune(&mut s, id, json!("tab-3"), json!({ "highlights": 0.05 }));
    assert_eq!(v["edits"].as_array().map(Vec::len), Some(6));
    assert_eq!(v["edits"][0], json!("tuning(0.3,0.2,0.1,0,none)"));
    assert_eq!(v["edits"][5], json!("tuning(0,0.05,0,0,none)"));
    assert_eq!(v["tuning"]["highlights"], json!(0.05));
    tune(&mut s, id, json!("tab-3"), json!({ "highlights": 0.1 }));
    assert_eq!(edits(&s, id).len(), 6);
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id).len(), 5);
    assert_eq!(labels(&mut s, id), ("Undo Retouch".into(), "Redo Tuning".into()));
}

/// An unknown or legacy entry after the Tuning may depend on its pixels: a session appends, and
/// the unknown entry keeps its text and place.
#[test]
fn an_unknown_entry_after_the_tuning_survives_and_makes_the_session_append() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(3, 96, 64), 90, None).unwrap();
    dalica_fs::replace(&folder.join("IMG_0001.jpg"), &bytes, Expect::Absent).unwrap();
    let sidecar = json!({ "photos": { "IMG_0001.jpg": { "edits": ["tuning(0.3,0,0,0,none)", "rotate(90)", "future_op(1;2)", "flip_v()"] } } });
    dalica_fs::replace(&folder.join(".dalica.json"), serde_json::to_vec_pretty(&sidecar).unwrap().as_slice(), Expect::Absent).unwrap();
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let id = s.library().photos().map(|p| p.id).next().unwrap();
    assert_eq!(edits(&s, id).len(), 4, "the sidecar was read");
    let v = tune(&mut s, id, json!("tab"), json!({}));
    assert_eq!(v["tuning"]["fill"], json!(0.3));
    tune(&mut s, id, json!("tab"), json!({ "temp": 0.5 }));
    assert_eq!(edits(&s, id), list(&["tuning(0.3,0,0,0,none)", "rotate(90)", "future_op(1;2)", "flip_v()", "tuning(0,0,0,0.5,none)"]));
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["tuning(0.3,0,0,0,none)", "rotate(90)", "future_op(1;2)", "flip_v()"]));
    assert_eq!(sidecar_entry(&mut s, &folder, "IMG_0001.jpg")["edits"], json!(edits(&s, id)));
}

/// Starts `command` and waits for its job's result.
fn answer(s: &mut Session, command: &str, p: Value) -> Value {
    let v = s.execute(command, &p).unwrap();
    assert_eq!(v["pending"], json!(true));
    let job = v["job"].as_u64().unwrap();
    let entry = s.wait_job(job, WAIT).unwrap();
    assert_eq!(entry.state, JobState::Done, "{entry:?}");
    assert_eq!(entry.kind, JobKind::Analysis);
    entry.result.unwrap()
}

fn rgb(v: &Value) -> [f64; 3] {
    let a: Vec<f64> = v["rgb"].as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect();
    [a[0], a[1], a[2]]
}

#[test]
fn the_wands_and_the_picker_answer_from_a_worker_without_the_owner_thread_touching_fs() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 300, 200);
    let id = ids[0];
    let ui = dalica_fs::mark_ui_thread();
    let light = answer(&mut s, "photo.tuning.auto", json!({ "id": id, "what": "lighting" }));
    let color = answer(&mut s, "photo.tuning.auto", json!({ "id": id, "what": "color" }));
    let picked = answer(&mut s, "photo.tuning.pick", json!({ "id": id, "x": 0.25, "y": 0.3 }));
    drop(ui);

    assert_eq!((light["id"].clone(), light["what"].clone()), (json!(id), json!("lighting")));
    let (fill, highlights, shadows) = (light["fill"].as_f64().unwrap(), light["highlights"].as_f64().unwrap(), light["shadows"].as_f64().unwrap());
    assert!((0.0..=1.0).contains(&fill) && (0.0..=0.5).contains(&highlights) && (0.0..=0.5).contains(&shadows), "{light}");
    assert_eq!(color["what"], json!("color"));
    for v in [&color, &picked] {
        let hex = v["neutral"].as_str().unwrap();
        assert!(hex.starts_with('#') && hex.len() == 7, "{v}");
        let byte = |i: usize| f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
        let c = rgb(v);
        for (k, i) in [1, 3, 5].into_iter().enumerate() {
            assert!((c[k] - byte(i)).abs() < 1e-3, "{v}");
        }
    }
    assert_eq!((picked["x"].clone(), picked["y"].clone()), (json!(0.25), json!(0.3)));

    // The UI applies what came back as a session change.
    tune(&mut s, id, json!("wand"), json!({ "fill": fill, "highlights": highlights, "shadows": shadows }));
    let v = tune(&mut s, id, json!("wand"), json!({ "neutral": picked["neutral"].clone() }));
    assert_eq!(v["tuning"]["neutral"], picked["neutral"]);
    assert_eq!(edits(&s, id).len(), 1);

    // The answers come from the picture the Tuning entry receives: the session's own entry does
    // not change them.
    tune(&mut s, id, json!("wand"), json!({ "temp": 0.8 }));
    assert_eq!(answer(&mut s, "photo.tuning.pick", json!({ "id": id, "x": 0.25, "y": 0.3 })), picked);
    assert_eq!(answer(&mut s, "photo.tuning.auto", json!({ "id": id, "what": "color" })), color);
    assert_eq!(answer(&mut s, "photo.tuning.auto", json!({ "id": id, "what": "lighting" })), light);
    // An edit before it does.
    s.execute("photo.history.apply", &json!({ "id": id, "op": "sepia()" })).unwrap();
    assert_ne!(answer(&mut s, "photo.tuning.pick", json!({ "id": id, "x": 0.25, "y": 0.3 }))["neutral"], picked["neutral"]);
}

#[test]
fn the_picker_reads_the_photo_as_shown() {
    let (_lib, _d, _f, mut s, ids) = setup(2, 300, 200);
    let (turned, cropped) = (ids[0], ids[1]);
    let before = answer(&mut s, "photo.tuning.pick", json!({ "id": turned, "x": 0.2, "y": 0.3 }));
    s.execute("photo.history.apply", &json!({ "id": turned, "op": "rotate(90)" })).unwrap();
    // A quarter turn clockwise shows the stored point (x, y) at (1 − y, x).
    let after = answer(&mut s, "photo.tuning.pick", json!({ "id": turned, "x": 0.7, "y": 0.2 }));
    let (a, b) = (rgb(&before), rgb(&after));
    for k in 0..3 {
        assert!((a[k] - b[k]).abs() < 0.05, "{before} vs {after}");
    }
    // A crop shows part of the photo: the centre of what is shown is the rectangle's centre.
    s.execute("photo.crop.apply", &json!({ "id": cropped, "rect": [0.0, 0.2, 0.4, 0.2] })).unwrap();
    let inside = answer(&mut s, "photo.tuning.pick", json!({ "id": cropped, "x": 0.5, "y": 0.5 }));
    s.execute("photo.history.undo", &json!({ "id": cropped })).unwrap();
    let whole = answer(&mut s, "photo.tuning.pick", json!({ "id": cropped, "x": 0.2, "y": 0.3 }));
    let (a, b) = (rgb(&inside), rgb(&whole));
    for k in 0..3 {
        assert!((a[k] - b[k]).abs() < 0.05, "{inside} vs {whole}");
    }
}

#[test]
fn a_new_ask_replaces_the_one_before_and_a_missing_file_fails_the_job() {
    let (_lib, _d, folder, mut s, ids) = setup(2, 300, 200);
    let first = s.execute("photo.tuning.pick", &json!({ "id": ids[0], "x": 0.1, "y": 0.1 })).unwrap()["job"].as_u64().unwrap();
    let wand = s.execute("photo.tuning.auto", &json!({ "id": ids[0], "what": "lighting" })).unwrap()["job"].as_u64().unwrap();
    let second = s.execute("photo.tuning.pick", &json!({ "id": ids[0], "x": 0.9, "y": 0.9 })).unwrap()["job"].as_u64().unwrap();
    let done = s.wait_job(second, WAIT).unwrap();
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.result.unwrap()["x"], json!(0.9));
    let first = s.wait_job(first, WAIT).unwrap();
    assert!(matches!(first.state, JobState::Cancelled | JobState::Done), "{first:?}");
    assert_eq!(s.wait_job(wand, WAIT).unwrap().state, JobState::Done, "another wand is not replaced");
    let job = s.execute("photo.tuning.auto", &json!({ "id": ids[1], "what": "color" })).unwrap()["job"].as_u64().unwrap();
    let _ = s.cancel_jobs(Some(job));
    assert!(s.wait_job(job, WAIT).unwrap().state.is_finished());

    std::fs::remove_file(folder.join("IMG_0002.jpg")).unwrap();
    let job = s.execute("photo.tuning.auto", &json!({ "id": ids[1], "what": "lighting" })).unwrap()["job"].as_u64().unwrap();
    let entry = s.wait_job(job, WAIT).unwrap();
    assert_eq!(entry.state, JobState::Failed, "{entry:?}");
    assert!(entry.error.is_some());
}

#[test]
fn the_commands_are_listed_with_their_history() {
    let (_lib, _d, _f, s, _ids) = setup(1, 96, 64);
    let find = |id: &str| s.commands().into_iter().find(|c| c.id == id).unwrap();
    for (id, effect, history) in [
        ("photo.history.tuning", Effect::Library, History::Photo),
        ("photo.tuning.auto", Effect::Query, History::None),
        ("photo.tuning.pick", Effect::Query, History::None),
    ] {
        let c = find(id);
        assert_eq!((c.effect, c.history, c.enabled), (effect, history, true), "{id}");
    }
}

#[test]
fn graceful_failures_change_nothing() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    tune(&mut s, id, json!("s"), json!({ "fill": 0.2 }));
    let (model, jobs) = (s.library().model().clone(), s.jobs().len());
    let long = "x".repeat(65);
    let bad: Vec<(&str, Value, &str)> = vec![
        ("photo.history.tuning", json!({}), "bad_params"),
        ("photo.history.tuning", json!({ "id": id }), "bad_params"),
        ("photo.history.tuning", json!({ "session": "s", "fill": 0.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": 999, "session": "s", "fill": 0.5 }), "not_found"),
        ("photo.history.tuning", json!({ "id": -1, "session": "s", "fill": 0.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "", "fill": 0.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": long, "fill": 0.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": -3, "fill": 0.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": 1.5, "fill": 0.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": null, "fill": 0.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "fill": 1.5 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "fill": -0.1 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "fill": "0.5" }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "highlights": 0.6 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "shadows": 1e308 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "temp": -1.01 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "neutral": "#12345" }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "neutral": "red" }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "neutral": [1, 2, 3] }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "neutral": [0.5, 0.5] }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "neutral": 7 }), "bad_params"),
        ("photo.history.tuning", json!({ "id": id, "session": "s", "fill": 0.5, "extra": 1 }), "bad_params"),
        ("photo.tuning.auto", json!({ "id": id }), "bad_params"),
        ("photo.tuning.auto", json!({ "what": "lighting" }), "bad_params"),
        ("photo.tuning.auto", json!({ "id": id, "what": "colour" }), "bad_params"),
        ("photo.tuning.auto", json!({ "id": id, "what": "Lighting" }), "bad_params"),
        ("photo.tuning.auto", json!({ "id": id, "what": 1 }), "bad_params"),
        ("photo.tuning.auto", json!({ "id": 999, "what": "color" }), "not_found"),
        ("photo.tuning.auto", json!({ "id": id, "what": "color", "extra": true }), "bad_params"),
        ("photo.tuning.pick", json!({ "id": id }), "bad_params"),
        ("photo.tuning.pick", json!({ "id": id, "x": 0.5 }), "bad_params"),
        ("photo.tuning.pick", json!({ "x": 0.5, "y": 0.5 }), "bad_params"),
        ("photo.tuning.pick", json!({ "id": 999, "x": 0.5, "y": 0.5 }), "not_found"),
        ("photo.tuning.pick", json!({ "id": id, "x": 1.5, "y": 0.5 }), "bad_params"),
        ("photo.tuning.pick", json!({ "id": id, "x": 0.5, "y": -0.1 }), "bad_params"),
        ("photo.tuning.pick", json!({ "id": id, "x": "0.5", "y": 0.5 }), "bad_params"),
        ("photo.tuning.pick", json!({ "id": id, "x": 0.5, "y": 0.5, "extra": 0 }), "bad_params"),
    ];
    for (cmd, p, code) in bad {
        let r = s.execute(cmd, &p);
        assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{cmd} {p} gave {r:?}");
        assert_eq!(s.library().model(), &model, "{cmd} changed the model on {p}");
        assert_eq!(s.jobs().len(), jobs, "{cmd} started a job on {p}");
    }
    for cmd in ["photo.history.tuning", "photo.tuning.auto", "photo.tuning.pick"] {
        for p in [json!([]), json!("text"), json!(42), json!({ "zzz": 1 })] {
            let r = s.execute(cmd, &p);
            assert!(matches!(r, Err(CmdError::BadParams(_))), "{cmd} {p} gave {r:?}");
        }
    }
    // Still one step: the failures took none.
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo".into()));
    assert_eq!(s.undo_stack().len(), 0);
}
