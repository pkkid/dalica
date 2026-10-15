//! Crop and straighten commands (`roadmap.md` M7.17; `editing.md` §5.1, §6.1–§6.2) on a temporary
//! library: a crop and a straighten are one step each of the photo's history; crop, then tune,
//! then crop again keeps the tuning and undo brings the first crop back; the custom ratios are
//! added, listed and removed across restarts, and the last ratio is remembered; the suggestion job
//! answers three rectangles in the stored frame without the owner thread touching `fs`; every
//! command's contract and graceful failures.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common::open;
use dalica_core::PhotoId;
use dalica_engine::{CmdError, Effect, History, JobKind, JobState, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(60);

/// A library watching one folder `Lisbon` of `n` `width × height` photos `IMG_0001.jpg`…, ids in
/// name order.
fn setup(n: usize, width: u32, height: u32) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64 + 7, width, height), 85, None).unwrap();
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

fn labels(s: &mut Session, id: PhotoId) -> (String, String) {
    let v = s.execute("photo.history.state", &json!({ "id": id })).unwrap();
    (v["undoLabel"].as_str().unwrap().to_string(), v["redoLabel"].as_str().unwrap().to_string())
}

fn crop(s: &mut Session, id: PhotoId, rect: [f64; 4]) -> Value {
    s.execute("photo.crop.apply", &json!({ "id": id, "rect": rect })).unwrap()
}

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|t| t.to_string()).collect()
}

fn sidecar_entry(s: &mut Session, folder: &Path, name: &str) -> Value {
    s.flush().unwrap();
    let v: Value = std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap());
    v["photos"][name].clone()
}

const TUNING: &str = "tuning(0.25,0,0,0,none)";

#[test]
fn crop_then_tune_then_recrop_undo_restores_the_first_crop_and_keeps_the_tuning() {
    let (_lib, _d, folder, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    let v = crop(&mut s, id, [0.1, 0.1, 0.8, 0.8]);
    assert_eq!((v["changed"].clone(), v["undoLabel"].clone()), (json!(true), json!("Undo Crop")));
    s.execute("photo.history.apply", &json!({ "id": id, "op": TUNING })).unwrap();
    // The Crop tool re-opened on the whole photo, a new rectangle applied: appended, the tuning kept.
    let v = crop(&mut s, id, [0.25, 0.0, 0.5, 1.0]);
    assert_eq!(v["edits"], json!(["crop(0.1,0.1,0.9,0.9)", TUNING, "crop(0.25,0,0.75,1)"]));
    assert_eq!(labels(&mut s, id), ("Undo Crop".into(), "Redo".into()));
    assert_eq!(s.undo_stack().len(), 0, "photo steps never reach the library stack");

    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["crop(0.1,0.1,0.9,0.9)", TUNING]), "the first crop counts again and the tuning stays");
    assert_eq!(labels(&mut s, id), ("Undo Tuning".into(), "Redo Crop".into()));
    s.execute("photo.history.redo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["crop(0.1,0.1,0.9,0.9)", TUNING, "crop(0.25,0,0.75,1)"]));
    assert_eq!(sidecar_entry(&mut s, &folder, "IMG_0001.jpg")["edits"], json!(edits(&s, id)));

    // The rectangle that counts already: no step. Undo all and the photo is whole again.
    let v = crop(&mut s, id, [0.25, 0.0, 0.5, 1.0]);
    assert_eq!(v["changed"], json!(false));
    assert_eq!(edits(&s, id).len(), 3);
    for _ in 0..3 {
        s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    }
    assert!(edits(&s, id).is_empty());
    // A whole-photo crop on an uncropped photo changes nothing; after a crop it is a step that
    // gives the whole photo back.
    assert_eq!(crop(&mut s, id, [0.0, 0.0, 1.0, 1.0])["changed"], json!(false));
    assert_eq!(labels(&mut s, id).1, "Redo Crop", "a step not taken keeps the redo list");
    crop(&mut s, id, [0.2, 0.2, 0.5, 0.5]);
    assert_eq!(crop(&mut s, id, [0.0, 0.0, 1.0, 1.0])["changed"], json!(true));
    assert_eq!(edits(&s, id), list(&["crop(0.2,0.2,0.7,0.7)", "crop(0,0,1,1)"]));
}

#[test]
fn straighten_is_one_step_and_only_the_last_counts() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    let v = s.execute("photo.straighten.apply", &json!({ "id": id, "degrees": 2.5 })).unwrap();
    assert_eq!(
        (v["edits"].clone(), v["undoLabel"].clone(), v["changed"].clone()),
        (json!(["straighten(2.5)"]), json!("Undo Straighten"), json!(true))
    );
    crop(&mut s, id, [0.1, 0.1, 0.5, 0.5]);
    // A nudge of 0.1° from the slider's f32 is kept to a thousandth of a degree.
    let v = s.execute("photo.straighten.apply", &json!({ "id": id, "degrees": f64::from(-0.1f32) })).unwrap();
    assert_eq!(v["edits"], json!(["straighten(2.5)", "crop(0.1,0.1,0.6,0.6)", "straighten(-0.1)"]));
    assert_eq!(s.execute("photo.straighten.apply", &json!({ "id": id, "degrees": -0.1 })).unwrap()["changed"], json!(false));
    s.execute("photo.history.undo", &json!({ "id": id })).unwrap();
    assert_eq!(edits(&s, id), list(&["straighten(2.5)", "crop(0.1,0.1,0.6,0.6)"]));
    assert_eq!(labels(&mut s, id), ("Undo Crop".into(), "Redo Straighten".into()));
    // Back to level on a photo with no straighten takes no step; the limits are accepted.
    let (_l2, _d2, _f2, mut t, other) = setup(1, 96, 64);
    assert_eq!(t.execute("photo.straighten.apply", &json!({ "id": other[0], "degrees": 0 })).unwrap()["changed"], json!(false));
    t.execute("photo.straighten.apply", &json!({ "id": other[0], "degrees": -10 })).unwrap();
    t.execute("photo.straighten.apply", &json!({ "id": other[0], "degrees": 10 })).unwrap();
    assert_eq!(edits(&t, other[0]), list(&["straighten(-10)", "straighten(10)"]));
}

#[test]
fn custom_ratios_and_the_last_ratio_survive_a_restart() {
    let (lib, _d, _f, mut s, ids) = setup(1, 96, 64);
    let v = s.execute("crop.ratios.list", &json!({})).unwrap();
    assert!(["us", "metric"].contains(&v["photoSizes"].as_str().unwrap()));
    assert_eq!(v["sizes"].as_array().unwrap().len(), 4);
    let fixed: Vec<&str> = v["fixed"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(fixed, ["Square (1:1)", "4:3 (Standard screen)", "16:10 (Widescreen)", "16:9 (HD video)", "5:3 (Photo frame)"]);
    assert_eq!(v["fixed"][3]["ratio"], json!("16:9"));
    assert_eq!((v["custom"].clone(), v["last"].clone()), (json!([]), Value::Null));

    let v = s.execute("crop.ratios.add", &json!({ "name": "  Panorama ", "width": 3, "height": 1 })).unwrap();
    assert_eq!(v["custom"], json!([{ "name": "Panorama", "ratio": "3:1", "width": 3.0, "height": 1.0 }]));
    s.execute("crop.ratios.add", &json!({ "name": "Cinema", "width": 2.39, "height": 1 })).unwrap();
    s.execute("photo.crop.apply", &json!({ "id": ids[0], "rect": [0.0, 0.25, 1.0, 0.5], "ratio": "3:1" })).unwrap();
    assert_eq!(s.undo_stack().len(), 0, "ratios are settings, not undo steps");
    s.flush().unwrap();
    drop(s);

    let mut s = open(lib.path());
    let v = s.execute("crop.ratios.list", &json!({})).unwrap();
    let names: Vec<&str> = v["custom"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Panorama", "Cinema"]);
    assert_eq!(v["custom"][1]["ratio"], json!("2.39:1"));
    assert_eq!(v["last"], json!("3:1"));
    let v = s.execute("crop.ratios.remove", &json!({ "name": "Panorama" })).unwrap();
    assert_eq!(v["custom"].as_array().unwrap().len(), 1);
    s.flush().unwrap();
    drop(s);

    let mut s = open(lib.path());
    let v = s.execute("crop.ratios.list", &json!({})).unwrap();
    assert_eq!(v["custom"], json!([{ "name": "Cinema", "ratio": "2.39:1", "width": 2.39, "height": 1.0 }]));
    assert_eq!(v["last"], json!("3:1"));
}

#[test]
fn the_ratio_list_follows_the_photo_sizes_setting() {
    let (_lib, _d, _f, mut s, _ids) = setup(1, 96, 64);
    for (sizes, first, last) in
        [(dalica_engine::settings::PhotoSizes::Us, "4 × 6", "8.5 × 11 (Letter)"), (dalica_engine::settings::PhotoSizes::Metric, "10 × 15", "A4")]
    {
        s.change_settings(|st| {
            let mut e = st.edit();
            e.photo_sizes = Some(sizes);
            st.set_edit(&e);
        });
        let v = s.execute("crop.ratios.list", &json!({})).unwrap();
        assert_eq!(v["photoSizes"], json!(sizes.id()));
        assert_eq!((v["sizes"][0]["name"].as_str().unwrap(), v["sizes"][3]["name"].as_str().unwrap()), (first, last));
    }
    let v = s.execute("crop.ratios.list", &json!({})).unwrap();
    assert_eq!(v["sizes"][3], json!({ "name": "A4", "ratio": "210:297", "width": 210.0, "height": 297.0 }));
}

/// The job's result after waiting for it.
fn suggestions(s: &mut Session, id: PhotoId, ratio: Value) -> Value {
    let v = s.execute("photo.crop.suggest", &json!({ "id": id, "ratio": ratio })).unwrap();
    assert_eq!(v["pending"], json!(true));
    let job = v["job"].as_u64().unwrap();
    let entry = s.wait_job(job, WAIT).unwrap();
    assert_eq!(entry.state, JobState::Done, "{entry:?}");
    assert_eq!(entry.kind, JobKind::Analysis);
    entry.result.unwrap()
}

/// The rectangles of a result, `[x, y, w, h]` each.
fn rects(v: &Value) -> Vec<[f64; 4]> {
    let list = v["suggestions"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    list.iter()
        .map(|s| {
            assert!(["subject", "horizon", "centred"].contains(&s["reason"].as_str().unwrap()));
            let r: Vec<f64> = s["rect"].as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect();
            [r[0], r[1], r[2], r[3]]
        })
        .collect()
}

#[test]
fn the_suggestion_job_answers_three_rectangles_without_the_owner_thread_touching_fs() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 300, 200);
    let id = ids[0];
    let ui = dalica_fs::mark_ui_thread();
    let v = suggestions(&mut s, id, json!("4:6"));
    drop(ui);
    assert_eq!(v["id"], json!(id));
    let found = rects(&v);
    for [x, y, w, h] in &found {
        assert!(*x >= 0.0 && *y >= 0.0 && *w > 0.0 && *h > 0.0 && x + w <= 1.0 + 1e-6 && y + h <= 1.0 + 1e-6, "{found:?}");
        // 4:6 in the photo's pixels (300 × 200).
        let aspect = (w * 300.0) / (h * 200.0);
        assert!((aspect - 4.0 / 6.0).abs() < 0.01, "{aspect} for {found:?}");
    }
    assert_ne!(found[0], found[1]);
    assert_ne!(found[1], found[2]);
    // The same photo, ratio and recipe give the same rectangles; a number works as a ratio.
    assert_eq!(rects(&suggestions(&mut s, id, json!(4.0 / 6.0))), found);
    // A crop in the recipe is not what the tool shows: the suggestions are of the whole photo.
    crop(&mut s, id, [0.1, 0.1, 0.3, 0.3]);
    assert_eq!(rects(&suggestions(&mut s, id, json!("4:6"))), found);
}

#[test]
fn suggestions_on_a_turned_photo_come_back_in_the_stored_frame() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 300, 200);
    let id = ids[0];
    s.execute("photo.history.apply", &json!({ "id": id, "op": "rotate(90)" })).unwrap();
    // Displayed 200 × 300: a 3:4 rectangle there is 4:3 in the stored 300 × 200 frame.
    for [_, _, w, h] in rects(&suggestions(&mut s, id, json!("3:4"))) {
        let stored = (w * 300.0) / (h * 200.0);
        assert!((stored - 4.0 / 3.0).abs() < 0.01, "{stored}");
    }
    // A straighten is part of what the tool shows.
    s.execute("photo.straighten.apply", &json!({ "id": id, "degrees": 5 })).unwrap();
    assert_eq!(rects(&suggestions(&mut s, id, json!("1:1"))).len(), 3);
}

#[test]
fn a_new_ask_for_the_same_photo_replaces_the_one_before() {
    let (_lib, _d, _f, mut s, ids) = setup(2, 300, 200);
    let first = s.execute("photo.crop.suggest", &json!({ "id": ids[0], "ratio": 1 })).unwrap()["job"].as_u64().unwrap();
    let other = s.execute("photo.crop.suggest", &json!({ "id": ids[1], "ratio": 1 })).unwrap()["job"].as_u64().unwrap();
    let second = s.execute("photo.crop.suggest", &json!({ "id": ids[0], "ratio": 2 })).unwrap()["job"].as_u64().unwrap();
    let done = s.wait_job(second, WAIT).unwrap();
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.result.unwrap()["ratio"], json!(2.0));
    // The first was cancelled, unless it had already answered.
    let first = s.wait_job(first, WAIT).unwrap();
    assert!(matches!(first.state, JobState::Cancelled | JobState::Done), "{first:?}");
    assert_eq!(s.wait_job(other, WAIT).unwrap().state, JobState::Done);
    // jobs.cancel stops one too.
    let job = s.execute("photo.crop.suggest", &json!({ "id": ids[1], "ratio": 1 })).unwrap()["job"].as_u64().unwrap();
    let _ = s.cancel_jobs(Some(job));
    assert!(s.wait_job(job, WAIT).unwrap().state.is_finished());
}

#[test]
fn a_missing_file_fails_the_job_and_nothing_else() {
    let (_lib, _d, folder, mut s, ids) = setup(1, 96, 64);
    std::fs::remove_file(folder.join("IMG_0001.jpg")).unwrap();
    let job = s.execute("photo.crop.suggest", &json!({ "id": ids[0], "ratio": 1 })).unwrap()["job"].as_u64().unwrap();
    let entry = s.wait_job(job, WAIT).unwrap();
    assert_eq!(entry.state, JobState::Failed, "{entry:?}");
    assert!(entry.error.is_some());
}

#[test]
fn the_commands_are_listed_with_their_history() {
    let (_lib, _d, _f, s, _ids) = setup(1, 96, 64);
    let find = |id: &str| s.commands().into_iter().find(|c| c.id == id).unwrap();
    for (id, effect, history) in [
        ("photo.crop.apply", Effect::Library, History::Photo),
        ("photo.straighten.apply", Effect::Library, History::Photo),
        ("crop.ratios.list", Effect::Query, History::None),
        ("crop.ratios.add", Effect::Library, History::None),
        ("crop.ratios.remove", Effect::Library, History::None),
        ("photo.crop.suggest", Effect::Query, History::None),
    ] {
        let c = find(id);
        assert_eq!((c.effect, c.history, c.enabled), (effect, history, true), "{id}");
    }
}

#[test]
fn graceful_failures_change_nothing() {
    let (_lib, _d, _f, mut s, ids) = setup(1, 96, 64);
    let id = ids[0];
    crop(&mut s, id, [0.1, 0.1, 0.5, 0.5]);
    s.execute("crop.ratios.add", &json!({ "name": "Wide", "width": 2, "height": 1 })).unwrap();
    let (model, settings, jobs) = (s.library().model().clone(), s.settings().edit(), s.jobs().len());
    let long = "x".repeat(300);
    let bad: Vec<(&str, Value, &str)> = vec![
        ("photo.crop.apply", json!({}), "bad_params"),
        ("photo.crop.apply", json!({ "id": id }), "bad_params"),
        ("photo.crop.apply", json!({ "rect": [0, 0, 1, 1] }), "bad_params"),
        ("photo.crop.apply", json!({ "id": 999, "rect": [0, 0, 1, 1] }), "not_found"),
        ("photo.crop.apply", json!({ "id": id, "rect": [0, 0, 1] }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": "0,0,1,1" }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": [0.5, 0.5, 0.6, 0.2] }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": [0, 0, 0, 1] }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": [-1, 0, 0.5, 0.5] }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": [0, 0, 1e300, 1] }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": [0, 0, 1, 1], "ratio": "wide" }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": [0, 0, 1, 1], "ratio": 4 }), "bad_params"),
        ("photo.crop.apply", json!({ "id": id, "rect": [0, 0, 1, 1], "extra": 1 }), "bad_params"),
        ("photo.straighten.apply", json!({ "id": id }), "bad_params"),
        ("photo.straighten.apply", json!({ "id": id, "degrees": 10.5 }), "bad_params"),
        ("photo.straighten.apply", json!({ "id": id, "degrees": -1e308 }), "bad_params"),
        ("photo.straighten.apply", json!({ "id": id, "degrees": "2" }), "bad_params"),
        ("photo.straighten.apply", json!({ "id": -1, "degrees": 2 }), "bad_params"),
        ("photo.straighten.apply", json!({ "id": 999, "degrees": 2 }), "not_found"),
        ("crop.ratios.list", json!({ "x": 1 }), "bad_params"),
        ("crop.ratios.add", json!({}), "bad_params"),
        ("crop.ratios.add", json!({ "name": "  ", "width": 2, "height": 1 }), "bad_params"),
        ("crop.ratios.add", json!({ "name": long, "width": 2, "height": 1 }), "limit"),
        ("crop.ratios.add", json!({ "name": "Wide", "width": 3, "height": 1 }), "bad_params"),
        ("crop.ratios.add", json!({ "name": "Zero", "width": 0, "height": 1 }), "bad_params"),
        ("crop.ratios.add", json!({ "name": "Huge", "width": 20000, "height": 20000 }), "bad_params"),
        ("crop.ratios.add", json!({ "name": "Thin", "width": 100, "height": 1 }), "bad_params"),
        ("crop.ratios.add", json!({ "name": "Text", "width": "2", "height": 1 }), "bad_params"),
        ("crop.ratios.remove", json!({}), "bad_params"),
        ("crop.ratios.remove", json!({ "name": "Nope" }), "not_found"),
        ("crop.ratios.remove", json!({ "name": 3 }), "bad_params"),
        ("photo.crop.suggest", json!({ "id": id }), "bad_params"),
        ("photo.crop.suggest", json!({ "ratio": 1 }), "bad_params"),
        ("photo.crop.suggest", json!({ "id": 999, "ratio": 1 }), "not_found"),
        ("photo.crop.suggest", json!({ "id": id, "ratio": 0 }), "bad_params"),
        ("photo.crop.suggest", json!({ "id": id, "ratio": -1 }), "bad_params"),
        ("photo.crop.suggest", json!({ "id": id, "ratio": 50 }), "bad_params"),
        ("photo.crop.suggest", json!({ "id": id, "ratio": "manual" }), "bad_params"),
        ("photo.crop.suggest", json!({ "id": id, "ratio": [4, 6] }), "bad_params"),
        ("photo.crop.suggest", json!({ "id": id, "ratio": 1, "extra": true }), "bad_params"),
    ];
    for (cmd, p, code) in bad {
        let r = s.execute(cmd, &p);
        assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{cmd} {p} gave {r:?}");
        assert_eq!(s.library().model(), &model, "{cmd} changed the model on {p}");
        assert_eq!(s.settings().edit(), settings, "{cmd} changed the settings on {p}");
        assert_eq!(s.jobs().len(), jobs, "{cmd} started a job on {p}");
    }
    for cmd in ["photo.crop.apply", "photo.straighten.apply", "crop.ratios.list", "crop.ratios.add", "crop.ratios.remove", "photo.crop.suggest"] {
        for p in [json!([]), json!("text"), json!(42), json!({ "zzz": 1 })] {
            let r = s.execute(cmd, &p);
            assert!(matches!(r, Err(CmdError::BadParams(_))), "{cmd} {p} gave {r:?}");
        }
    }
    assert_eq!(s.undo_stack().len(), 0);
}

#[test]
fn the_ratio_count_is_capped() {
    let (_lib, _d, _f, mut s, _ids) = setup(1, 96, 64);
    for i in 0..dalica_engine::settings::MAX_CROP_RATIOS {
        s.execute("crop.ratios.add", &json!({ "name": format!("R{i}"), "width": 1 + i % 5, "height": 2 })).unwrap();
    }
    let r = s.execute("crop.ratios.add", &json!({ "name": "One more", "width": 1, "height": 1 }));
    assert!(matches!(r, Err(CmdError::Limit(_))), "{r:?}");
    assert_eq!(s.settings().edit().crop_ratios.len(), dalica_engine::settings::MAX_CROP_RATIOS);
}
