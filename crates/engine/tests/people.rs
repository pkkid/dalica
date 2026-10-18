//! Faces and people as data (`roadmap.md` M13.15; `organizing.md` §6, `faces.md` §9): naming,
//! ignoring, moving and removing faces, a face drawn by hand, the people and their commands, each
//! one undo step; what survives a restart and a rebuild (`people.json` written by
//! `people_file.rs`); `person:` and the Faces filter; face tiles made off the owner thread; people
//! data withheld from automation without the permission; each command's own failures.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{lisbon, open, read_json};
use dalica_core::{PersonId, PhotoId, Rect01, SystemClock};
use dalica_engine::automation::Policy;
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_library::{Face, FacePerson, FaceRef, Person};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(120);

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
    Rect01::new(x0, y0, x1, y1).unwrap()
}

/// A 160 × 120 JPEG of scene `seed`.
fn photo_bytes(seed: u64) -> Vec<u8> {
    crate::common::jpeg_of(seed, 160, 120)
}

/// A sidecar giving each of the first `n` photos two unnamed faces.
fn two_faces_each(n: usize) -> Value {
    let photos: serde_json::Map<String, Value> = (1..=n)
        .map(|i| (format!("IMG_{i:04}.jpg"), json!({ "faces": [{ "rect": [0.1, 0.1, 0.3, 0.4] }, { "rect": [0.6, 0.2, 0.8, 0.5] }] })))
        .collect();
    json!({ "dalica": { "version": 1 }, "photos": photos })
}

/// The faces of every photo and the people, as the model has them: what undo must give back.
type Snapshot = (Vec<(PhotoId, Vec<Face>)>, Vec<(PersonId, Person)>);

fn snapshot(s: &Session) -> Snapshot {
    let lib = s.library();
    (lib.faces_all().map(|(id, f)| (id, f.to_vec())).collect(), lib.people().map(|(id, p)| (id.clone(), p.clone())).collect())
}

fn ids_of(faces: &Value) -> Vec<String> {
    faces["faces"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap().to_string()).collect()
}

fn photo_named(s: &Session, dir: &Path, name: &str) -> PhotoId {
    let folder = s.library().folder_by_path(dir).expect("the folder is in the library");
    s.library().photo_by_name(folder, name).unwrap().id
}

fn person(s: &Session, name: &str) -> PersonId {
    s.library().person_by_name(name).cloned().unwrap_or_else(|| panic!("a person called {name}"))
}

fn undo(s: &mut Session) {
    s.execute("edit.undo", &json!({})).unwrap();
}

fn redo(s: &mut Session) {
    s.execute("edit.redo", &json!({})).unwrap();
}

/// Twelve faces named as one group: one undo step that takes them all back and redo gives them
/// again; the names, the nickname and a person with no face come back after a restart and after a
/// rebuild of a lost index (from the sidecars and `people.json`).
#[test]
fn naming_a_group_of_twelve_faces_is_one_step_and_survives_a_restart_and_a_rebuild() {
    let photos: Vec<Vec<u8>> = (0..6).map(photo_bytes).collect();
    let (lib, _disk, dir, mut s, _) = lisbon(&photos, Some(&two_faces_each(6)));
    let unnamed = s.execute("faces.list", &json!({ "unnamed": true })).unwrap();
    let group = ids_of(&unnamed);
    assert_eq!(group.len(), 12);
    let before = snapshot(&s);
    let steps = s.undo_stack().len();
    let r = s.execute("faces.name", &json!({ "faces": group, "name": "Ana Costa", "create": true, "nickname": "Aninhas" })).unwrap();
    assert_eq!((r["created"].clone(), r["changed"].clone()), (json!(true), json!(12)));
    assert_eq!(s.undo_stack().len(), steps + 1, "one step");
    let ana = person(&s, "Ana Costa");
    assert_eq!(r["person"], json!(ana));
    assert_eq!(s.library().person_faces(&ana).len(), 12);
    assert_eq!(s.library().unnamed_count(), 0);
    let after = snapshot(&s);
    undo(&mut s);
    assert_eq!(snapshot(&s), before, "one undo takes all twelve and the new person back");
    redo(&mut s);
    assert_eq!(snapshot(&s), after);
    let list = s.execute("people.list", &json!({})).unwrap();
    assert_eq!(list["people"][0]["section"], json!(format!("person:{ana}")));
    assert_eq!((list["people"][0]["count"].clone(), list["people"][0]["faces"].clone()), (json!(6), json!(12)));
    // A person with no face lives in `people.json` only.
    let bea = s.execute("people.create", &json!({ "name": "Bea", "nickname": "B" })).unwrap()["person"].clone();
    s.flush().unwrap();
    let file = read_json(&lib.path().join("people.json")).expect("people.json is written");
    assert_eq!(file["people"][ana.as_str()]["nickname"], json!("Aninhas"));
    assert_eq!(file["people"][bea.as_str().unwrap()]["name"], json!("Bea"));

    // A restart.
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(snapshot(&s).0, after.0, "the faces after a restart");
    assert_eq!(s.library().person(&ana).map(|p| p.nickname.as_str()), Some("Aninhas"));

    // The index lost and rebuilt from the folders and `people.json`.
    drop(s);
    for name in ["index.log", "index.snap"] {
        let _ = std::fs::remove_file(lib.path().join(name));
    }
    s = open(lib.path());
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    let lib_ = s.library();
    assert_eq!(lib_.person(&ana).map(|p| (p.name.as_str(), p.nickname.as_str())), Some(("Ana Costa", "Aninhas")));
    assert_eq!(lib_.person_faces(&ana).len(), 12, "every face named after the rebuild");
    assert_eq!(lib_.person(&PersonId::parse(bea.as_str().unwrap()).unwrap()).map(|p| p.name.as_str()), Some("Bea"));
    let one = photo_named(&s, &dir, "IMG_0001.jpg");
    assert_eq!(s.library().faces(one).len(), 2);
}

/// An unknown name asks New Person first and changes nothing; a name a person holds, in another
/// case, is that person.
#[test]
fn an_unknown_name_asks_first_and_a_known_one_is_that_person() {
    let (_lib, _disk, _dir, mut s, _) = lisbon(&[photo_bytes(1), photo_bytes(2)], Some(&two_faces_each(2)));
    let faces = ids_of(&s.execute("faces.list", &json!({})).unwrap());
    let before = snapshot(&s);
    let asked = s.execute("faces.name", &json!({ "faces": [faces[0]], "name": "Ana" }));
    match asked {
        Err(CmdError::Disabled(why)) => {
            assert!(why.starts_with("needs_person"), "{why}");
            assert!(!why.contains("Ana"), "the name is not repeated");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(snapshot(&s), before);
    assert_eq!(s.undo_stack().len(), 0);
    let ana = s.execute("people.create", &json!({ "name": "Ana" })).unwrap()["person"].clone();
    let r = s.execute("faces.name", &json!({ "faces": [faces[0], faces[1]], "name": "  aNA " })).unwrap();
    assert_eq!((r["person"].clone(), r["created"].clone(), r["changed"].clone()), (ana.clone(), json!(false), json!(2)));
    // By id too; a face already theirs is no change.
    let r = s.execute("faces.name", &json!({ "faces": [faces[1], faces[2]], "person": ana })).unwrap();
    assert_eq!(r["changed"], json!(1));
}

/// Ignore, move, remove, merge, rename, delete, create and Set as Person Thumbnail: each is one
/// undo step that gives back exactly what was, and redo what it did.
#[test]
fn every_change_undoes_and_redoes_in_one_step() {
    let photos: Vec<Vec<u8>> = (0..3).map(photo_bytes).collect();
    let (lib, _disk, _dir, mut s, ids) = lisbon(&photos, Some(&two_faces_each(3)));
    let faces = ids_of(&s.execute("faces.list", &json!({})).unwrap());
    let ana = s.execute("faces.name", &json!({ "faces": [faces[0], faces[1]], "name": "Ana", "create": true })).unwrap()["person"].clone();
    let rui = s.execute("faces.name", &json!({ "faces": [faces[2], faces[3]], "name": "Rui", "create": true })).unwrap()["person"].clone();
    let thumb = format!("{}.1", ids[0]);
    let cases: Vec<(&str, Value)> = vec![
        ("faces.ignore", json!({ "faces": [faces[4], faces[0]] })),
        ("faces.move", json!({ "faces": [faces[0], faces[5]], "person": rui })),
        ("people.setThumbnail", json!({ "face": thumb })),
        ("faces.remove", json!({ "faces": [faces[2], faces[4]] })),
        ("people.rename", json!({ "person": ana, "name": "Ana Costa", "nickname": "Aninhas" })),
        ("people.merge", json!({ "person": rui, "into": ana })),
        ("people.delete", json!({ "person": ana, "confirm": true })),
        ("people.create", json!({ "name": "Carla" })),
    ];
    for (id, params) in cases {
        let (before, steps) = (snapshot(&s), s.undo_stack().len());
        s.execute(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(s.undo_stack().len(), steps + 1, "{id}: one step");
        let after = snapshot(&s);
        assert_ne!(after, before, "{id} changed something");
        undo(&mut s);
        assert_eq!(snapshot(&s), before, "{id}: undo gives back what was");
        redo(&mut s);
        assert_eq!(snapshot(&s), after, "{id}: redo");
    }
    // What the cases leave: Ana deleted (her faces Unnamed), Carla with no face.
    let s_lib = s.library();
    assert!(s_lib.person(&PersonId::parse(ana.as_str().unwrap()).unwrap()).is_none());
    assert_eq!(s_lib.person_by_name("carla").map(|id| s_lib.person_faces(id).len()), Some(0));
    s.flush().unwrap();
    let file = read_json(&lib.path().join("people.json")).unwrap();
    let people = file["people"].as_object().unwrap();
    assert_eq!(people.len(), 1, "deleted and merged people leave people.json: {file}");
    assert!(people.values().all(|p| p["name"] == json!("Carla")));
}

/// A thumbnail set on a face is written to `people.json` with the photo's path and the stored
/// rectangle; a rename keeps unknown keys of the person's entry.
#[test]
fn people_json_holds_the_thumbnail_and_keeps_what_dalica_does_not_know() {
    let (lib, _disk, dir, mut s, ids) = lisbon(&[photo_bytes(7)], Some(&two_faces_each(1)));
    let ana = s.execute("faces.name", &json!({ "faces": [format!("{}.1", ids[0])], "name": "Ana", "create": true })).unwrap()["person"].clone();
    let ana = ana.as_str().unwrap().to_string();
    s.execute("people.setThumbnail", &json!({ "face": format!("{}.1", ids[0]), "person": ana })).unwrap();
    s.flush().unwrap();
    let path = lib.path().join("people.json");
    let mut file = read_json(&path).unwrap();
    assert_eq!(file["people"][&ana]["thumbnail"], json!({ "photo": dir.join("IMG_0001.jpg"), "rect": [0.6, 0.2, 0.8, 0.5] }));
    file["people"][&ana]["x-other"] = json!(7);
    file["otherApp"] = json!(true);
    crate::common::off_owner(|| std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap());
    s.execute("people.rename", &json!({ "person": ana, "name": "Ana Costa" })).unwrap();
    s.flush().unwrap();
    let file = read_json(&path).unwrap();
    assert_eq!(file["people"][&ana]["name"], json!("Ana Costa"));
    assert_eq!((file["people"][&ana]["x-other"].clone(), file["otherApp"].clone()), (json!(7), json!(true)));
}

/// A box drawn on a photo turned 90° clockwise is stored in the stored frame and shows where it
/// was drawn, also after a restart; the sidecar holds the stored rectangle.
#[test]
fn a_face_added_by_hand_on_a_turned_photo_is_stored_in_the_stored_frame() {
    let (lib, _disk, dir, mut s, ids) = lisbon(&[photo_bytes(3)], None);
    s.execute("photo.rotate.cw", &json!({ "ids": [ids[0]] })).unwrap();
    let steps = s.undo_stack().len();
    let r = s.execute("faces.add", &json!({ "id": ids[0], "displayed": [0.1, 0.2, 0.3, 0.6] })).unwrap();
    assert_eq!(s.undo_stack().len(), steps + 1);
    // Turned back by a quarter counter-clockwise: (x, y) → (y, 1 − x).
    assert_eq!(r["rect"], json!([0.2, 0.7, 0.6, 0.9]));
    assert_eq!(s.library().faces(ids[0]), [Face::new(rect(0.2, 0.7, 0.6, 0.9), FacePerson::Unnamed)]);
    let listed = s.execute("faces.list", &json!({ "ids": [ids[0]] })).unwrap();
    assert_eq!(listed["faces"][0]["displayed"], json!([0.1, 0.2, 0.3, 0.6]));
    // Given in the stored frame it is stored as given.
    s.execute("faces.add", &json!({ "id": ids[0], "rect": [0.05, 0.05, 0.15, 0.2] })).unwrap();
    s.flush().unwrap();
    assert_eq!(crate::common::entry(&dir, "IMG_0001.jpg").unwrap()["faces"][0]["rect"], json!([0.2, 0.7, 0.6, 0.9]));
    drop(s);
    let mut s = open(lib.path());
    let listed = s.execute("faces.list", &json!({ "ids": [ids[0]] })).unwrap();
    assert_eq!(listed["faces"][0]["rect"], json!([0.2, 0.7, 0.6, 0.9]));
    assert_eq!(listed["faces"][0]["displayed"], json!([0.1, 0.2, 0.3, 0.6]), "where it was drawn, after a restart");
}

/// On a photo turned, straightened and cropped, `faces.list`'s `displayed` is where the photo's
/// `Geometry` shows the face, normalised to the photo as shown (past 0..1 where the crop cuts the
/// face), and `faces.add {displayed}` on a photo of the same geometry stores it back within 10⁻⁹.
#[test]
fn displayed_faces_go_through_the_whole_geometry() {
    let (_lib, _disk, _dir, mut s, ids) = lisbon(&[photo_bytes(4), photo_bytes(5)], None);
    for id in &ids {
        s.execute("photo.rotate.cw", &json!({ "ids": [id] })).unwrap();
        for op in ["straighten(4)", "crop(0.1,0.15,0.8,0.9)"] {
            s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
        }
    }
    let stored = [rect(0.3, 0.3, 0.45, 0.5), rect(0.4, 0.02, 0.55, 0.2)];
    for r in &stored {
        s.execute("faces.add", &json!({ "id": ids[0], "rect": r })).unwrap();
    }
    let p = s.library().photo(ids[0]).unwrap();
    let recipe = dalica_edit::Recipe::parse(&s.library().info(ids[0]).edits).unwrap();
    let g = dalica_edit::Geometry::new(&recipe, dalica_image::Orientation::default(), p.width, p.height);
    let area = g.photo_area().unwrap().to_display(g.orientation());
    let listed = s.execute("faces.list", &json!({ "ids": [ids[0]] })).unwrap();
    for (n, r) in stored.iter().enumerate() {
        let d: Vec<f64> = listed["faces"][n]["displayed"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let c = g.to_display(dalica_edit::Point::new((r.x0() + r.x1()) / 2.0, (r.y0() + r.y1()) / 2.0)).unwrap();
        let want = [(c.x - area.x0) / (area.x1 - area.x0), (c.y - area.y0) / (area.y1 - area.y0)];
        let got = [(d[0] + d[2]) / 2.0, (d[1] + d[3]) / 2.0];
        assert!((want[0] - got[0]).abs() < 1e-9 && (want[1] - got[1]).abs() < 1e-9, "face {n}: {got:?} is not where Geometry shows it, {want:?}");
        let back = s.execute("faces.add", &json!({ "id": ids[1], "displayed": d })).unwrap();
        let back: Vec<f64> = back["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let r = [r.x0(), r.y0(), r.x1(), r.y1()];
        assert!(back.iter().zip(r).all(|(a, b)| (a - b).abs() < 1e-9), "face {n}: stored back as {back:?}, not {r:?}");
    }
    let cut = &listed["faces"][1]["displayed"];
    assert!(cut.as_array().unwrap().iter().any(|v| !(0.0..=1.0).contains(&v.as_f64().unwrap())), "the crop cuts the second face: {cut}");
}

/// `person:` finds the photos of a person by name and nickname, and the Faces filter the photos
/// with a named or unnamed face (an ignored one does not count); neither is unavailable once the
/// library has people and faces.
#[test]
fn person_search_and_the_faces_filter_find_the_right_photos() {
    let photos: Vec<Vec<u8>> = (0..5).map(photo_bytes).collect();
    let (_lib, _disk, _dir, mut s, ids) = lisbon(&photos, None);
    let query = |s: &mut Session, p: Value| -> (Vec<u64>, Value) {
        let r = s.execute("library.query", &p).unwrap();
        let mut found: Vec<u64> = r["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
        found.sort_unstable();
        (found, r["unavailable"].clone())
    };
    assert_eq!(query(&mut s, json!({ "text": "person:ana" })).1, json!(["person"]), "no people yet");
    let add = |s: &mut Session, id: PhotoId| -> String {
        s.execute("faces.add", &json!({ "id": id, "rect": [0.2, 0.2, 0.4, 0.5] })).unwrap()["face"].as_str().unwrap().into()
    };
    let (a1, a2, _unnamed, ignored) = (add(&mut s, ids[0]), add(&mut s, ids[1]), add(&mut s, ids[2]), add(&mut s, ids[3]));
    s.execute("faces.name", &json!({ "faces": [a1, a2], "name": "Ana Costa", "create": true, "nickname": "Ninha" })).unwrap();
    s.execute("faces.ignore", &json!({ "faces": [ignored] })).unwrap();
    let want = |v: &[usize]| {
        let mut w: Vec<u64> = v.iter().map(|i| ids[*i].get()).collect();
        w.sort_unstable();
        w
    };
    for text in ["person:ana", "person:ninha", "person:costa"] {
        let (found, unavailable) = query(&mut s, json!({ "text": text }));
        assert_eq!(found, want(&[0, 1]), "{text}");
        assert_eq!(unavailable, json!([]), "{text}");
    }
    assert_eq!(query(&mut s, json!({ "text": "person:rui" })).0, Vec::<u64>::new());
    let (found, unavailable) = query(&mut s, json!({ "filters": { "faces": true } }));
    assert_eq!((found, unavailable), (want(&[0, 1, 2]), json!([])), "named and unnamed faces, not ignored ones");
}

/// A person is an album of the photos holding their faces (`person:<id>`): `library.query`,
/// `library.inspect`, `slideshow.list`, Export, `places.list` and `section.refreshThumbs` take the
/// section; `section.sort` keeps its order; `counts.people` counts the people; an unknown person
/// is `not_found`.
#[test]
fn a_person_is_a_section_for_the_light_table_slideshows_and_export() {
    let photos: Vec<Vec<u8>> = (0..4).map(photo_bytes).collect();
    let (_lib, _disk, _dir, mut s, ids) = lisbon(&photos, None);
    let mut faces = Vec::new();
    for id in [ids[2], ids[0]] {
        faces.push(s.execute("faces.add", &json!({ "id": id, "rect": [0.2, 0.2, 0.4, 0.5] })).unwrap()["face"].clone());
    }
    let ana = s.execute("faces.name", &json!({ "faces": faces, "name": "Ana", "create": true })).unwrap()["person"].clone();
    let section = format!("person:{}", ana.as_str().unwrap());
    let mut want = vec![ids[0].get(), ids[2].get()];
    want.sort_unstable();
    let sorted = |v: &Value, key: &str| -> Vec<u64> {
        let mut out: Vec<u64> = v.as_array().unwrap().iter().map(|p| p.get(key).unwrap_or(p).as_u64().unwrap()).collect();
        out.sort_unstable();
        out
    };
    let query = s.execute("library.query", &json!({ "section": section })).unwrap();
    assert_eq!(sorted(&query["photos"], "id"), want);
    let inspect = s.execute("library.inspect", &json!({ "photos": section })).unwrap();
    assert_eq!(sorted(&inspect["photos"], "id"), want);
    assert!(inspect["photos"].as_array().unwrap().iter().all(|p| p["section"] == json!(section)));
    assert_eq!(inspect["counts"]["people"], json!(1));
    let show = s.execute("slideshow.list", &json!({ "section": section })).unwrap();
    assert_eq!(sorted(&show["photos"], ""), want);
    let export = s.execute("export.defaults", &json!({ "section": section })).unwrap();
    assert_eq!((export["count"].clone(), export["folderName"].clone()), (json!(2), json!("Ana")));
    let places = s.execute("places.list", &json!({ "section": section })).unwrap();
    assert_eq!(places["total"], json!(2));
    s.execute("section.refreshThumbs", &json!({ "section": section })).unwrap();
    assert_eq!(s.execute("section.sort", &json!({ "section": section, "by": "name" })).map_err(|e| e.code()).err(), Some("disabled"));
    for (id, p) in [
        ("library.query", json!({ "section": "person:p0000000dead" })),
        ("library.inspect", json!({ "section": "person:p0000000dead" })),
        ("slideshow.list", json!({ "section": "person:p0000000dead" })),
        ("export.defaults", json!({ "section": "person:p0000000dead" })),
        ("section.refreshThumbs", json!({ "section": "person:p0000000dead" })),
    ] {
        assert_eq!(s.execute(id, &p).map_err(|e| e.code()).err(), Some("not_found"), "{id}");
    }
    assert_eq!(s.execute("library.query", &json!({ "section": "person:ana" })).map_err(|e| e.code()).err(), Some("bad_params"));
}

/// A face tile is cut on a worker from the thumbnail ladder, `side × side`, with the colours of
/// the face's square; the owner thread makes no `fs` call asking for it, waiting and taking it.
#[test]
fn a_face_tile_arrives_off_the_owner_thread_and_the_owner_makes_no_fs_call() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let dir: PathBuf = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&dir).unwrap();
    let bytes = crate::common::jpeg_of(11, 400, 300);
    dalica_fs::replace(&dir.join("IMG_0001.jpg"), &bytes, dalica_fs::Expect::Absent).unwrap();
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(lib.path(), Arc::new(SystemClock), &options).unwrap();
    s.execute("folders.add", &json!({ "path": dir })).unwrap();
    s.wait_idle();
    let id = s.library().photos().next().unwrap().id;
    s.execute("faces.add", &json!({ "id": id, "rect": [0.4, 0.3, 0.6, 0.6] })).unwrap();
    let face = FaceRef { photo: id, index: 0 };

    let before = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    assert_eq!(s.request_face_tiles(&[face, face], 48).unwrap(), 1, "asked once");
    assert!(s.wait_face_tiles(WAIT));
    let tiles = s.take_face_tiles();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");
    assert_eq!(tiles.len(), 1);
    let tile = &tiles[0];
    assert_eq!((tile.face, tile.side, tile.image.width, tile.image.height), (face, 48, 48, 48));
    assert_eq!(s.face_tiles().stats().made, 1);

    // The tile's mean colour is the mean of the face's square in the photo (decoded here): the
    // box is 80 × 90 pixels, so the square is 135 pixels around its centre (200, 135).
    let decoded = crate::common::off_owner(|| dalica_thumbs::thumbnail(&dir.join("IMG_0001.jpg"), 400, &dalica_image::Limits::default()).unwrap());
    let mean = |data: &[u8], keep: &dyn Fn(usize) -> bool| -> [f64; 3] {
        let (mut sum, mut n) = ([0.0; 3], 0.0);
        for (_, px) in data.chunks(4).enumerate().filter(|(i, _)| keep(*i)) {
            for c in 0..3 {
                sum[c] += f64::from(px[c]);
            }
            n += 1.0;
        }
        sum.map(|v| v / n)
    };
    let w = decoded.width as usize;
    let square = |i: usize| (133..268).contains(&(i % w)) && (68..203).contains(&(i / w));
    let (want, got) = (mean(&decoded.data, &square), mean(&tile.image.data, &|_| true));
    for c in 0..3 {
        assert!((want[c] - got[c]).abs() < 12.0, "channel {c}: {want:?} {got:?}");
    }

    // Closed before they are taken, tiles are dropped; bad sides are refused; faces the library
    // does not have are passed over.
    assert_eq!(s.request_face_tiles(&[face], 64).unwrap(), 1);
    s.close_face_tiles();
    assert_eq!(s.face_tiles().pending(), 0);
    assert!(s.wait_face_tiles(WAIT));
    assert!(s.take_face_tiles().is_empty());
    assert!(matches!(s.request_face_tiles(&[face], 0), Err(CmdError::BadParams(_))));
    assert!(matches!(s.request_face_tiles(&[face], 4096), Err(CmdError::BadParams(_))));
    assert_eq!(s.request_face_tiles(&[FaceRef { photo: id, index: 9 }, FaceRef { photo: PhotoId::new(999), index: 0 }], 32).unwrap(), 0);
}

/// Without `--automation-allow-people`, `faces.list` and `people.list` reach automation with names
/// `null` and no rectangles, ids and counts kept; with it, as they are; face descriptors never.
#[test]
fn without_the_people_permission_automation_sees_no_names_or_rectangles() {
    let (_lib, _disk, _dir, mut s, _) = lisbon(&[photo_bytes(1)], Some(&two_faces_each(1)));
    let faces = ids_of(&s.execute("faces.list", &json!({})).unwrap());
    s.execute("faces.name", &json!({ "faces": [faces[0]], "name": "Ana", "create": true, "nickname": "Ninha" })).unwrap();
    let (list, people) = (s.execute("faces.list", &json!({})).unwrap(), s.execute("people.list", &json!({})).unwrap());
    let denied = Policy::default();
    let shown = denied.people_reply("faces.list", list.clone());
    assert_eq!(shown["total"], json!(2));
    for f in shown["faces"].as_array().unwrap() {
        assert_eq!(f["name"], Value::Null);
        assert!(f.get("rect").is_none() && f.get("displayed").is_none(), "{f}");
        assert!(f["id"].is_string());
    }
    assert!(shown["faces"][0]["person"].is_string(), "the person's id stays");
    let shown = denied.people_reply("people.list", people.clone());
    assert_eq!((shown["people"][0]["name"].clone(), shown["people"][0]["nickname"].clone()), (Value::Null, Value::Null));
    assert_eq!((shown["people"][0]["count"].clone(), shown["unnamed"].clone()), (json!(1), json!(1)), "counts stay");
    assert!(!shown.to_string().contains("Ana") && !shown.to_string().contains("Ninha"));
    // Another command's reply is untouched; with the permission the people's are too.
    let other = json!({ "name": "Lisbon" });
    assert_eq!(denied.people_reply("albums.list", other.clone()), other);
    let mut allowed = Policy::default();
    allowed.set_allow_people(true);
    assert_eq!(allowed.people_reply("faces.list", list.clone()), list);
    let mut with_embedding = list.clone();
    with_embedding["faces"][0]["embedding"] = json!([0.1, 0.2]);
    assert_eq!(allowed.people_reply("faces.list", with_embedding), list, "descriptors are never returned");
}

/// Without `--automation-allow-people` a search over automation leaves people's names out of the
/// match (`Policy::people_params`): `library.query` with `person:Ana` finds nothing and lists
/// `person` unavailable, the plain word `Ana` finds no photo by her name, `people: [Ana]` (an id)
/// still finds hers, and `slideshow.list {search: "person:Ana"}` plays nothing; with the permission
/// all of them find her photos. A `search.set` through the policy is
/// withheld too, while the window's own `search.set` after it matches names again: the switch is
/// per call.
#[test]
fn without_the_people_permission_searches_leave_names_out_of_the_match() {
    let photos: Vec<Vec<u8>> = (0..3).map(photo_bytes).collect();
    let (_lib, _disk, _dir, mut s, ids) = lisbon(&photos, None);
    let mut faces = Vec::new();
    for id in &ids[..2] {
        faces.push(s.execute("faces.add", &json!({ "id": id, "rect": [0.2, 0.2, 0.4, 0.5] })).unwrap()["face"].clone());
    }
    let ana = s.execute("faces.name", &json!({ "faces": faces, "name": "Ana", "create": true })).unwrap()["person"].clone();
    let run = |s: &mut Session, policy: &Policy, id: &str, params: Value| -> Value {
        let Value::Object(mut p) = params else { panic!("params are an object") };
        policy.people_params(id, &mut p);
        s.execute(id, &Value::Object(p)).unwrap()
    };
    let found = |r: &Value| -> Vec<u64> {
        let mut v: Vec<u64> = r["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
        v.sort_unstable();
        v
    };
    let hers = vec![ids[0].get(), ids[1].get()];
    let mut allowed = Policy::default();
    allowed.set_allow_people(true);
    for (policy, want, unavailable) in [(Policy::default(), vec![], json!(["person"])), (allowed, hers.clone(), json!([]))] {
        let at = format!("allow_people {}", policy.allow_people());
        let r = run(&mut s, &policy, "library.query", json!({ "text": "person:Ana" }));
        assert_eq!((found(&r), r["unavailable"].clone()), (want.clone(), unavailable), "{at}: person:");
        assert_eq!(found(&run(&mut s, &policy, "library.query", json!({ "text": "Ana" }))), want, "{at}: a plain word");
        // A caller asking for names is overruled without the permission.
        let r = run(&mut s, &policy, "library.query", json!({ "text": "ana", "withholdNames": false }));
        assert_eq!(found(&r), want, "{at}: withholdNames from the caller");
        assert_eq!(found(&run(&mut s, &policy, "library.query", json!({ "people": [ana] }))), hers, "{at}: by id");
        let show = run(&mut s, &policy, "slideshow.list", json!({ "search": "person:Ana" }));
        let mut played: Vec<u64> = show["photos"].as_array().unwrap().iter().map(|p| p.as_u64().unwrap()).collect();
        played.sort_unstable();
        assert_eq!(played, want, "{at}: slideshow.list's search");
        run(&mut s, &policy, "search.set", json!({ "text": "person:Ana" }));
        s.wait_search();
        assert_eq!(s.search_inspect()["matches"], json!(want.len()), "{at}: search.set");
    }
    run(&mut s, &Policy::default(), "search.set", json!({ "text": "ana" }));
    s.wait_search();
    assert_eq!(s.search_inspect()["matches"], json!(0), "withheld");
    s.execute("search.set", &json!({ "text": "person:Ana" })).unwrap();
    s.wait_search();
    assert_eq!(s.search_inspect()["matches"], json!(2), "the window's own search matches names");
    let r = s.execute("search.set", &json!({ "text": "", "people": [ana] })).unwrap();
    assert_eq!(r["people"], json!([ana]));
    s.wait_search();
    assert_eq!(s.search_inspect()["matches"], json!(2), "the people filter alone is a search");
    assert!(matches!(s.execute("library.query", &json!({ "people": ["p0f0f"] })), Err(CmdError::NotFound(_))));
    assert!(matches!(s.execute("search.set", &json!({ "people": ["p0f0f"] })), Err(CmdError::NotFound(_))));
    assert!(matches!(s.execute("search.set", &json!({ "people": ["x"] })), Err(CmdError::BadParams(_))));
    let many: Vec<String> = (0..=dalica_library::search::MAX_PEOPLE).map(|i| format!("p{i:04x}")).collect();
    assert!(matches!(s.execute("library.query", &json!({ "people": many })), Err(CmdError::Limit(_))));
}

/// Each command's own failures: ids that are not ids, faces, photos and people the library does
/// not have, rectangles that are no rectangles, a full photo, a face twice, a thumbnail of an
/// unnamed face, a name another person holds, a merge into itself, a delete without its answer
/// and pages out of range. None changes anything.
#[test]
fn each_command_fails_gracefully() {
    let (_lib, _disk, _dir, mut s, ids) = lisbon(&[photo_bytes(1), photo_bytes(2)], Some(&two_faces_each(1)));
    let one = ids[0];
    let ana = s.execute("people.create", &json!({ "name": "Ana" })).unwrap()["person"].clone();
    let rui = s.execute("people.create", &json!({ "name": "Rui" })).unwrap()["person"].clone();
    let face = format!("{one}.0");
    let (before, steps) = (snapshot(&s), s.undo_stack().len());
    let cases: Vec<(&str, Value, &str)> = vec![
        ("faces.name", json!({ "faces": ["12"], "person": ana }), "bad_params"),
        ("faces.name", json!({ "faces": ["x.0"], "person": ana }), "bad_params"),
        ("faces.name", json!({ "faces": [format!("{one}.-1")], "person": ana }), "bad_params"),
        ("faces.name", json!({ "faces": [format!("{one}.9")], "person": ana }), "not_found"),
        ("faces.name", json!({ "faces": ["999.0"], "person": ana }), "not_found"),
        ("faces.name", json!({ "faces": [face], "person": "p0000000dead" }), "not_found"),
        ("faces.name", json!({ "faces": [face], "person": ana, "name": "Ana" }), "bad_params"),
        ("faces.name", json!({ "faces": [face] }), "bad_params"),
        ("faces.name", json!({ "faces": [face], "name": " \u{7} " }), "bad_params"),
        ("faces.name", json!({ "faces": [face], "name": "x".repeat(1025), "create": true }), "limit"),
        ("faces.name", json!({ "faces": [], "person": ana }), "disabled"),
        ("faces.move", json!({ "faces": [face], "person": "p0000000dead" }), "not_found"),
        ("faces.ignore", json!({ "faces": [face, "1.x"] }), "bad_params"),
        ("faces.remove", json!({ "faces": [] }), "disabled"),
        ("faces.add", json!({ "id": one, "rect": [0.5, 0.5, 0.5, 0.6] }), "bad_params"),
        ("faces.add", json!({ "id": one, "rect": [0.5, 0.5, 1.5, 0.6] }), "bad_params"),
        ("faces.add", json!({ "id": one, "rect": [0.1, 0.1, 0.3, 0.4] }), "bad_params"),
        ("faces.add", json!({ "id": one, "rect": [0.1, 0.1, 0.2, 0.2], "displayed": [0.1, 0.1, 0.2, 0.2] }), "bad_params"),
        ("faces.add", json!({ "id": one }), "bad_params"),
        ("faces.add", json!({ "id": 999, "rect": [0.1, 0.1, 0.2, 0.2] }), "not_found"),
        ("faces.add", json!({ "id": one, "rect": [0.1, 0.1, 0.2, 0.2], "person": "p0000000dead" }), "not_found"),
        ("faces.list", json!({ "unnamed": true, "person": ana }), "bad_params"),
        ("faces.list", json!({ "limit": 10_001 }), "bad_params"),
        ("faces.list", json!({ "person": "p0000000dead" }), "not_found"),
        ("faces.list", json!({ "ids": [999] }), "not_found"),
        ("people.list", json!({ "sort": "age" }), "bad_params"),
        ("people.create", json!({ "name": "ana" }), "bad_params"),
        ("people.create", json!({ "name": "" }), "bad_params"),
        ("people.rename", json!({ "person": rui, "name": "ANA" }), "bad_params"),
        ("people.rename", json!({ "person": "p0000000dead", "name": "Bea" }), "not_found"),
        ("people.merge", json!({ "person": ana, "into": ana }), "bad_params"),
        ("people.merge", json!({ "person": ana, "into": "p0000000dead" }), "not_found"),
        ("people.delete", json!({ "person": ana }), "disabled"),
        ("people.delete", json!({ "person": ana, "confirm": false }), "disabled"),
        ("people.setThumbnail", json!({ "face": face }), "bad_params"),
        ("people.setThumbnail", json!({ "face": "1" }), "bad_params"),
    ];
    for (id, params, code) in cases {
        let r = s.execute(id, &params);
        assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{id} {params}: {r:?}");
        assert_eq!(snapshot(&s), before, "{id} {params} changed nothing");
        assert_eq!(s.undo_stack().len(), steps, "{id} {params} recorded no step");
    }
    // A photo holds at most 256 faces.
    for i in 0..254u32 {
        let x = f64::from(i % 16) / 16.0;
        let y = f64::from(i / 16) / 16.0;
        s.execute("faces.add", &json!({ "id": ids[1], "rect": [x, y, x + 0.05, y + 0.05] })).unwrap();
    }
    s.execute("faces.add", &json!({ "id": ids[1], "rect": [0.99, 0.99, 1.0, 1.0] })).unwrap();
    s.execute("faces.add", &json!({ "id": ids[1], "rect": [0.98, 0.98, 0.99, 0.99] })).unwrap();
    assert_eq!(s.library().faces(ids[1]).len(), 256);
    let full = s.execute("faces.add", &json!({ "id": ids[1], "rect": [0.97, 0.97, 0.98, 0.98] }));
    assert_eq!(full.map_err(|e| e.code()).err(), Some("limit"));
    // A thumbnail of another person's face.
    s.execute("faces.name", &json!({ "faces": [face], "person": ana })).unwrap();
    let r = s.execute("people.setThumbnail", &json!({ "face": face, "person": rui }));
    assert_eq!(r.map_err(|e| e.code()).err(), Some("bad_params"));
}
