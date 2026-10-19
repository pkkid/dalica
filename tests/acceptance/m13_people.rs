//! Suggestions, the People pane and face regions in the live app (`m13.rs` part 2): `dalica
//! --headless` on six photos with painted faces, the recogniser installed through the face model
//! dialog as in part 1, Ana's faces red and Rui's blue (named on photos 1–3 and 6):
//!
//! - suggestions come only above the threshold and the margin: photo 4's reddish face is "Ana?",
//!   photo 5's reddish face too; photo 4's purple face, as like Ana as Rui, is above the threshold
//!   (Find More lists it) but has no lead, and photo 5's green face is below it: neither gets one;
//! - Confirm makes photo 4's face Ana's as one undo step and it is never suggested again; Reject
//!   takes photo 5's suggestion away for good, also after a restart;
//! - a face drawn by hand (Add a Person Manually in the edit view, its box dragged into place, a
//!   name typed) on a photo turned 90° is one step that makes the person; it is stored in the stored
//!   frame, shows where it was drawn (`faces.list` → `displayed`) and after a restart its box is
//!   where it was on screen;
//! - with Store people names in photos on, renaming Rui writes his new name into the files as MWG
//!   face regions, read back by the independent reader with Ana's, where the faces are;
//! - renaming, merging and deleting a person are one undo step each, and Undo takes each back;
//! - the People pane on photo 4 matches its fingerprint;
//! - no connection, and the whole workflow's `RUST_LOG=trace` log holds no name, rectangle or
//!   vector.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dalica_meta::Limits;
use serde_json::{Value, json};

use crate::app::{self, dialog, flush_until, run, secs};
use crate::common::{self, LARGE, Proxy, Rgb, face_on, faces, person, scan_done, steady_shot, undo_state};
use crate::live::{self, Client};

const RED: Rgb = [220, 40, 40];
const BLUE: Rgb = [40, 40, 220];
/// Nearly Ana's red: "Ana?".
const REDDISH: Rgb = [200, 60, 60];
const RED_TOO: Rgb = [210, 50, 50];
/// As like Ana's red as Rui's blue (cosine about 0.60 and 0.58): no lead.
const PURPLE: Rgb = [130, 40, 130];
/// Like nobody (cosine below 0 with both).
const GREEN: Rgb = [40, 220, 40];
const PHOTO: (u32, u32) = (800, 600);

/// The suggestions (`faces.suggest`): face → suggested person.
fn suggestions(c: &mut Client) -> Vec<(String, String)> {
    let r = c.exec("faces.suggest", json!({}));
    assert_eq!(r["on"], true, "Suggest names is on: {r}");
    r["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["face"].as_str().unwrap().to_string(), s["person"].as_str().unwrap().to_string()))
        .collect()
}

/// The face's person (`faces.list`).
fn person_of(c: &mut Client, face: &str) -> Option<String> {
    let photo: u64 = face.split('.').next().unwrap().parse().unwrap();
    faces(c, json!({ "ids": [photo] })).into_iter().find(|f| f["id"] == face).and_then(|f| f["person"].as_str().map(str::to_string))
}

fn id_of(c: &mut Client, name: &str) -> String {
    person(c, name).unwrap_or_else(|| panic!("no person {name}"))["id"].as_str().unwrap().to_string()
}

/// One library undo step: `act` adds exactly one, Undo takes it back (`undone` holds), Redo puts it
/// back (`done` holds).
fn one_step(c: &mut Client, what: &str, act: impl FnOnce(&mut Client), done: impl Fn(&mut Client) -> bool, undone: impl Fn(&mut Client) -> bool) {
    let (depth, _) = undo_state(c);
    act(c);
    let (after, label) = undo_state(c);
    assert_eq!(after, depth + 1, "{what} is one undo step ({label})");
    assert!(done(c), "{what}: done");
    run(c, "edit.undo", json!({}));
    assert!(undone(c), "{what}: one Undo takes it back");
    run(c, "edit.redo", json!({}));
    assert!(done(c), "{what}: Redo does it again");
}

/// The face regions in a file: (name, rect).
fn regions(path: &Path) -> Vec<(Option<String>, [f64; 4])> {
    let Ok(read) = dalica_meta::read(&std::fs::read(path).unwrap_or_default(), &Limits::default()) else { return Vec::new() };
    read.owned.faces.iter().map(|f| (f.name.clone(), [f.rect.x0(), f.rect.y0(), f.rect.x1(), f.rect.y1()])).collect()
}

fn names_in(path: &Path) -> BTreeSet<String> {
    regions(path).into_iter().filter_map(|(n, _)| n).collect()
}

pub fn people(exe: &Path, home: &Path) {
    let started = Instant::now();
    let proxy = Proxy::start();
    let models = common::models(home);
    let file = home.join("Downloads").join("my-recogniser.onnx");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, common::recogniser_bytes()).unwrap();
    let family = home.join("Pictures").join("Family");
    let plan: [(Rgb, Rgb); 6] = [(RED, BLUE), (RED, BLUE), (RED, BLUE), (REDDISH, PURPLE), (GREEN, RED_TOO), (RED, BLUE)];
    let paths: Vec<PathBuf> = (1..=6).map(|n| family.join(format!("IMG_{n:04}.jpg"))).collect();
    for (i, (l, r)) in plan.iter().enumerate() {
        app::put(&paths[i], &common::faces_jpeg(PHOTO.0, PHOTO.1, *l, *r), i as u64);
    }
    let launch = || common::launch(&proxy, &models).env("RUST_LOG", "trace");
    let (a, mut cl) = live::start_with(exe, home, launch());
    let c = &mut cl;
    app::resize(c, LARGE);
    run(c, "folders.add", json!({ "path": family }));
    common::install_recogniser(c, &models, &file);
    scan_done(c);
    if dialog(c).is_some() {
        c.ok("ui.dialog.cancel", json!({}));
    }
    let by_path = app::photos(c, &home.join("Pictures"));
    let p: Vec<Value> = (1..=6).map(|n| by_path[&format!("Family/IMG_{n:04}.jpg")].id.clone()).collect();
    let all = faces(c, json!({ "limit": 1000 }));
    assert_eq!(all.len(), 12, "two faces a photo: {all:?}");
    let named = [0usize, 1, 2, 5];
    let ana_faces: Vec<String> = named.iter().map(|i| face_on(&all, &p[*i], true)).collect();
    let rui_faces: Vec<String> = named.iter().map(|i| face_on(&all, &p[*i], false)).collect();
    run(c, "faces.name", json!({ "faces": ana_faces, "name": "Ana", "create": true }));
    run(c, "faces.name", json!({ "faces": rui_faces, "name": "Rui", "create": true }));
    let (ana, rui) = (id_of(c, "Ana"), id_of(c, "Rui"));
    let (p4_red, p4_purple) = (face_on(&all, &p[3], true), face_on(&all, &p[3], false));
    let (p5_green, p5_red) = (face_on(&all, &p[4], true), face_on(&all, &p[4], false));

    // Suggestions: only above the threshold and ahead of the runner-up.
    let r = c.exec("faces.suggest", json!({}));
    let threshold = r["threshold"].as_f64().unwrap_or_else(|| panic!("a threshold: {r}"));
    let mut got = suggestions(c);
    got.sort();
    let mut want = vec![(p4_red.clone(), ana.clone()), (p5_red.clone(), ana.clone())];
    want.sort();
    assert_eq!(got, want, "the two reddish faces are \"Ana?\", nothing else (threshold {threshold})");
    let more = c.exec("faces.findMore", json!({ "person": ana }));
    let score = |face: &str| more["faces"].as_array().unwrap().iter().find(|f| f["face"] == face).and_then(|f| f["score"].as_f64());
    let purple = score(&p4_purple).unwrap_or_else(|| panic!("Find More lists the purple face: {more}"));
    assert!(purple >= threshold, "the purple face is above the threshold ({purple:.3} ≥ {threshold:.3}) yet has no suggestion: no lead over Rui");
    assert!(score(&p5_green).is_none(), "the green face is below even Find More's bar: {more}");
    let people_row = person(c, "Ana").unwrap();
    assert_eq!(people_row["pending"], json!(2), "Ana's row counts her two suggestions: {people_row}");
    println!(
        "  suggestions: two \"Ana?\" at threshold {threshold:.2}; the purple face ({purple:.2}) held back by the margin, the green one below the threshold"
    );

    // Confirm: one step, the face Ana's, never suggested again. Reject: gone for good.
    one_step(
        c,
        "Confirm",
        |c| {
            run(c, "faces.confirm", json!({ "faces": [p4_red] }));
        },
        |c| person_of(c, &p4_red).as_deref() == Some(ana.as_str()),
        |c| person_of(c, &p4_red).is_none(),
    );
    assert!(!suggestions(c).iter().any(|(f, _)| *f == p4_red), "a confirmed face is never suggested again");
    let rejected = run(c, "faces.reject", json!({ "faces": [p5_red] }));
    assert_eq!(rejected["rejected"], json!(1), "{rejected}");
    assert_eq!(suggestions(c), Vec::<(String, String)>::new(), "the rejected pair is gone");

    // A face drawn by hand on photo 6, turned 90°.
    run(c, "photo.history.apply", json!({ "id": p[5], "op": "rotate(90)" }));
    run(c, "view.edit", json!({ "id": p[5] }));
    if c.state()["pane"] != "people" {
        run(c, "pane.people", json!({}));
    }
    app::click(c, "button:faces.addManually");
    let start = manual_rect(c);
    c.ok("ui.dragWidget", json!({ "id": "facebox:manual/se", "dx": 60.0, "dy": 40.0 }));
    c.settle();
    c.ok("ui.dragWidget", json!({ "id": "facebox:manual", "dx": -120.0, "dy": -50.0 }));
    c.settle();
    let drawn = manual_rect(c);
    assert!(
        drawn[0] < start[0] && drawn[1] < start[1] && drawn[2] - drawn[0] > start[2] - start[0],
        "the box was moved and grown: {start:?} → {drawn:?}"
    );
    let on_screen = app::rect_of(c, "facebox:manual").unwrap();
    let (depth, _) = undo_state(c);
    c.ok("ui.clickWidget", json!({ "id": "field:faces.manual" }));
    c.ok("ui.type", json!({ "text": "Eva" }));
    app::key(c, "Enter");
    assert_eq!(undo_state(c).0, depth + 1, "the face and the new person: one undo step");
    let eva = id_of(c, "Eva");
    let hand = faces(c, json!({ "ids": [p[5]] })).into_iter().find(|f| f["person"] == eva.as_str()).expect("Eva's face");
    let close = |v: &Value, b: [f64; 4]| (0..4).all(|i| (v[i].as_f64().unwrap() - b[i]).abs() < 2e-3);
    assert!(close(&hand["displayed"], drawn), "where it was drawn: {} vs {drawn:?}", hand["displayed"]);
    let stored = [drawn[1], 1.0 - drawn[2], drawn[3], 1.0 - drawn[0]];
    assert!(close(&hand["rect"], stored), "in the stored frame, turned back: {} vs {stored:?}", hand["rect"]);
    let hand_id = hand["id"].as_str().unwrap().to_string();
    println!("  a face drawn by hand on a photo turned 90°: shown at {drawn:?}, stored at {}", hand["rect"]);
    run(c, "view.library", json!({}));

    // Store people names in photos: renaming Rui writes the regions.
    run(c, "options.set", json!({ "values": { "people.storeNamesInPhotos": true } }));
    one_step(
        c,
        "Rename",
        |c| {
            run(c, "people.rename", json!({ "person": rui, "name": "Rafael" }));
        },
        |c| person(c, "Rafael").is_some() && person(c, "Rui").is_none(),
        |c| person(c, "Rui").is_some() && person(c, "Rafael").is_none(),
    );
    let want: BTreeSet<String> = ["Ana".to_string(), "Rafael".to_string()].into();
    flush_until(c, || paths[..3].iter().all(|f| names_in(f) == want));
    for f in &paths[..3] {
        assert_eq!(names_in(f), want, "{}: the named faces' regions: {:?}", f.display(), regions(f));
    }
    let listed = faces(c, json!({ "ids": [p[0]] }));
    for (name, rect) in regions(&paths[0]) {
        let face = listed.iter().find(|f| f["name"].as_str() == name.as_deref()).unwrap_or_else(|| panic!("{name:?}: {listed:?}"));
        assert!(
            (0..4).all(|i| (face["rect"][i].as_f64().unwrap() - rect[i]).abs() < 1e-4),
            "{name:?}: the region {rect:?} is the face {}",
            face["rect"]
        );
    }
    println!("  Store people names in photos: Ana's and Rafael's regions read back from the files where their faces are");

    // Merge and delete: one step each.
    one_step(
        c,
        "Merge Eva into Ana",
        |c| {
            run(c, "people.merge", json!({ "person": eva, "into": ana }));
        },
        |c| person(c, "Eva").is_none() && person_of(c, &hand_id).as_deref() == Some(ana.as_str()),
        |c| person(c, "Eva").is_some() && person_of(c, &hand_id).as_deref() == Some(eva.as_str()),
    );
    let rafael_faces = faces(c, json!({ "person": rui })).len();
    let unnamed = faces(c, json!({ "unnamed": true })).len();
    one_step(
        c,
        "Delete Rafael",
        |c| {
            run(c, "people.delete", json!({ "person": rui, "confirm": true }));
        },
        |c| person(c, "Rafael").is_none() && faces(c, json!({ "unnamed": true })).len() == unnamed + rafael_faces,
        |c| person(c, "Rafael").is_some() && faces(c, json!({ "person": rui })).len() == rafael_faces,
    );
    run(c, "edit.undo", json!({}));
    assert!(person(c, "Rafael").is_some(), "Rafael is back for the rest");
    println!("  rename, merge and delete: one undo step each");

    // The People pane on photo 4.
    run(c, "select.set", json!({ "ids": [p[3]] }));
    if c.state()["pane"] != "people" {
        run(c, "pane.people", json!({}));
    }
    steady_shot(c, home, "m13-1400x900-people-pane");
    live::quit(a, cl);
    let mut logs = common::log_of(home);

    // A restart: the rejected pair stays away, the confirmed face is Ana's, the drawn box where it was.
    let (a, mut cl) = live::start_with(exe, home, launch());
    let c = &mut cl;
    app::resize(c, LARGE);
    scan_done(c);
    assert_eq!(person_of(c, &p4_red).as_deref(), Some(ana.as_str()), "the confirmed face is still Ana's");
    assert_eq!(suggestions(c), Vec::<(String, String)>::new(), "after a restart the rejected pair is still not suggested");
    run(c, "view.edit", json!({ "id": p[5] }));
    if c.state()["pane"] != "people" {
        run(c, "pane.people", json!({}));
    }
    let again = app::rect_of(c, &format!("facebox:{hand_id}")).expect("the drawn face's box");
    assert!(
        again.iter().zip(on_screen).all(|(a, b)| (a - b).abs() < 1.5),
        "after a restart the box is where it was drawn: {again:?} vs {on_screen:?}"
    );
    live::quit(a, cl);
    logs.push_str(&common::log_of(home));
    assert_eq!(proxy.connections(), Vec::<String>::new(), "no connection");
    common::privacy(&logs, 2, &["Ana", "Rui", "Rafael", "Eva"], std::slice::from_ref(&family));
    println!("  after a restart: the rejected pair still not suggested, the confirmed face Ana's, the drawn box where it was");
    println!("  M13 suggestions, faces by hand, people and regions in {:.1} s", secs(started));
}

/// The box of Add a Person Manually, as drawn (`ui.state` → `peoplePane.manual.rect`).
fn manual_rect(c: &mut Client) -> [f64; 4] {
    let s = c.state();
    let r = &s["peoplePane"]["manual"]["rect"];
    [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap_or_else(|| panic!("ui.state peoplePane.manual: {}", s["peoplePane"])))
}
