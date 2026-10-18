//! Faces, geotags and people read from the folder sidecars by scans and rebuilds (`roadmap.md`
//! M13.45; `data-model.md` §3, §7): a folder whose `.dalica.json` says who is in its photos and
//! where they were taken, scanned into a fresh index by `folders.add`, by `library.rebuild` (in
//! place and built aside) and by the watcher's rescan, gives the model every face it can hold, the
//! removed location and the people; a later write in the folder leaves the sidecar's `faces`, `geo`
//! and `people` as they were, the faces it cannot use too. A rebuild also reads `people.json`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{open, open_with};
use dalica_core::geo::LatLon;
use dalica_core::{PersonId, PhotoId, Rect01, Timestamp, VirtualClock};
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_library::{Face, FaceKey, FacePerson, Geo};
use serde_json::{Value, json};

const NAMES: [&str; 3] = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"];

fn ana() -> PersonId {
    PersonId::parse("p7f3a2c00001").unwrap()
}

fn rui() -> PersonId {
    PersonId::parse("p0b1d0000002").unwrap()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
    Rect01::new(x0, y0, x1, y1).unwrap()
}

/// The folder's sidecar: named, ignored and unnamed faces, one naming a person named nowhere, one
/// with an empty rectangle and an entry that is no face; a place, a removed location; the people.
fn doc() -> Value {
    json!({
        "dalica": { "version": 1 },
        "photos": {
            "IMG_0001.jpg": {
                "faces": [
                    { "rect": [0.41, 0.22, 0.52, 0.38], "person": ana().as_str() },
                    { "rect": [0.3, 0.3, 0.4, 0.4], "person": "p999999999999" },
                    { "rect": [0.7, 0.3, 0.78, 0.44], "person": "ignored", "x-tool": 1 },
                    { "rect": [0.5, 0.5, 0.5, 0.6] },
                    { "rect": [0.1, 0.1, 0.2, 0.2] },
                    "not a face"
                ],
                "geo": [38.7116, -9.1389]
            },
            "IMG_0002.jpg": { "faces": [{ "rect": [0.2, 0.2, 0.4, 0.5], "person": rui().as_str() }], "geo": null },
            "IMG_0003.jpg": { "star": true }
        },
        "people": { ana().as_str(): { "name": "Ana Costa", "nickname": "Aninhas" }, rui().as_str(): { "name": "Rui Lopes" } },
        "otherApp": { "kept": true }
    })
}

/// A folder `dir` holding three photos and the sidecar [`doc`].
fn write_folder(dir: &Path) {
    dalica_fs::create_dir_all(dir).unwrap();
    for (i, name) in NAMES.iter().enumerate() {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(50 + i as u64, 96 + 8 * i as u32, 64), 80, None).unwrap();
        dalica_fs::replace(&dir.join(name), &bytes, Expect::Absent).unwrap();
    }
    dalica_fs::replace(&dir.join(".dalica.json"), &serde_json::to_vec_pretty(&doc()).unwrap(), Expect::Absent).unwrap();
}

fn photo(s: &Session, dir: &Path, name: &str) -> PhotoId {
    let folder = s.library().folder_by_path(dir).expect("the folder is in the library");
    s.library().photo_by_name(folder, name).unwrap().id
}

/// The sidecar's M13 keys: each photo's `faces` and `geo`, and the folder's `people`.
fn m13_keys(v: &Value) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = NAMES
        .iter()
        .flat_map(|n| ["faces", "geo"].map(|k| (format!("{n}/{k}"), v["photos"][n].get(k).cloned().unwrap_or(json!("(none)")))))
        .collect();
    out.push(("people".into(), v["people"].clone()));
    out.push(("otherApp".into(), v["otherApp"].clone()));
    out
}

/// The model holds every usable face, the place, the removed location and the people; a caption
/// written into the folder (on the photo with the most faces) leaves the M13 keys as they were.
fn check(s: &mut Session, dir: &Path, case: &str) {
    let (one, two, three) = (photo(s, dir, NAMES[0]), photo(s, dir, NAMES[1]), photo(s, dir, NAMES[2]));
    let lib = s.library();
    assert_eq!(
        lib.faces(one),
        [
            Face::new(rect(0.41, 0.22, 0.52, 0.38), FacePerson::Person(ana())),
            Face::new(rect(0.7, 0.3, 0.78, 0.44), FacePerson::Ignored),
            Face::new(rect(0.1, 0.1, 0.2, 0.2), FacePerson::Unnamed),
        ],
        "{case}: every face the model can hold, none naming a person named nowhere"
    );
    assert_eq!(lib.faces(two), [Face::new(rect(0.2, 0.2, 0.4, 0.5), FacePerson::Person(rui()))], "{case}");
    assert!(lib.faces(three).is_empty(), "{case}");
    assert_eq!(lib.geo(one), Some(Geo::At(LatLon::new(38.7116, -9.1389).unwrap())), "{case}");
    assert_eq!(lib.geo(two), Some(Geo::Removed), "{case}: the removed location reaches the model");
    assert_eq!(lib.geo(three), None, "{case}");
    let people: Vec<(String, String, String)> = lib.people().map(|(id, p)| (id.to_string(), p.name.clone(), p.nickname.clone())).collect();
    assert_eq!(
        people,
        [(rui().to_string(), "Rui Lopes".into(), String::new()), (ana().to_string(), "Ana Costa".into(), "Aninhas".into())],
        "{case}: the people the faces name, nobody else"
    );
    assert!(lib.person(&PersonId::parse("p999999999999").unwrap()).is_none(), "{case}");

    s.flush().unwrap();
    let before = read(dir);
    assert_eq!(m13_keys(&before), m13_keys(&doc()), "{case}: a scan writes none of these keys");
    s.execute("photo.caption.set", &json!({ "ids": [one], "caption": format!("Tram 28 ({case})") })).unwrap();
    s.flush().unwrap();
    let after = read(dir);
    assert_eq!(after["photos"][NAMES[0]]["caption"], json!(format!("Tram 28 ({case})")), "{case}: the caption is written");
    for ((key, was), (_, now)) in m13_keys(&doc()).into_iter().zip(m13_keys(&after)) {
        assert_eq!(now, was, "{case}: {key} is what it was");
        assert_eq!(now.to_string(), was.to_string(), "{case}: {key} prints the same");
    }
}

fn read(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(".dalica.json")).unwrap()).unwrap()
}

/// `folders.add`, then `library.rebuild` of a lost index (filling the empty model in place), then
/// `library.rebuild` of the full model (built aside), each followed by a write.
#[test]
fn faces_geotags_and_people_come_in_with_a_scan_and_a_rebuild_and_a_write_keeps_them() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let dir = disk.path().join("Lisbon");
    write_folder(&dir);
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": dir })).unwrap();
    s.wait_idle();
    check(&mut s, &dir, "folders.add");

    // The index lost: the rebuild fills the empty model in place.
    drop(s);
    for name in ["index.log", "index.snap"] {
        let _ = std::fs::remove_file(lib.path().join(name));
    }
    let mut s = open(lib.path());
    assert_eq!(s.library().people().count(), 0, "a fresh index");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    check(&mut s, &dir, "library.rebuild in place");

    // A rebuild of the full model, built aside and swapped in.
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    check(&mut s, &dir, "library.rebuild aside");
}

/// A rebuild reads `people.json` too: a person with no face in a watched folder comes back, the
/// names and nicknames are `people.json`'s, and the thumbnail it names is set where the face is
/// the person's.
#[test]
fn a_rebuild_reads_people_json_for_nicknames_thumbnails_and_people_without_faces() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let dir = disk.path().join("Lisbon");
    write_folder(&dir);
    let bea = PersonId::parse("p00000000bea0").unwrap();
    let file = json!({
        "dalica": { "version": 1 },
        "people": {
            ana().as_str(): { "name": "Ana Costa", "nickname": "Ninha" },
            rui().as_str(): { "name": "Rui Lopes", "thumbnail": { "photo": dir.join(NAMES[1]), "rect": [0.2, 0.2, 0.4, 0.5] } },
            bea.as_str(): { "name": "Bea" }
        }
    });
    std::fs::write(lib.path().join("people.json"), serde_json::to_vec(&file).unwrap()).unwrap();
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": dir })).unwrap();
    s.wait_idle();
    for case in ["in place", "aside"] {
        if case == "in place" {
            drop(s);
            for name in ["index.log", "index.snap"] {
                let _ = std::fs::remove_file(lib.path().join(name));
            }
            s = open(lib.path());
        }
        s.execute("library.rebuild", &json!({})).unwrap();
        s.wait_idle();
        let two = photo(&s, &dir, NAMES[1]);
        let l = s.library();
        assert_eq!(l.person(&ana()).map(|p| p.nickname.as_str()), Some("Ninha"), "{case}: people.json's nickname");
        assert_eq!(l.person(&rui()).and_then(|p| p.thumbnail), Some(FaceKey { photo: two, rect: rect(0.2, 0.2, 0.4, 0.5) }), "{case}");
        assert_eq!(l.person(&bea).map(|p| p.name.as_str()), Some("Bea"), "{case}: a person with no face comes back");
        assert_eq!(l.faces(two), [Face::new(rect(0.2, 0.2, 0.4, 0.5), FacePerson::Person(rui()))], "{case}");
    }
}

/// The same folder brought in by the (polling) watcher's rescan: moved into a watched folder from
/// outside it, it gives the model the same, and a write keeps the keys.
#[test]
fn a_folder_brought_in_by_the_watchers_rescan_keeps_its_faces_geotags_and_people() {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_system_time(std::time::SystemTime::now())));
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root: PathBuf = disk.path().join("Trips");
    dalica_fs::create_dir_all(&root).unwrap();
    let mut s = open_with(lib.path(), clock.clone());
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    s.set_watching(dalica_engine::watching::WatchMode::Polling);
    let settle = |s: &mut Session| {
        for _ in 0..4 {
            clock.advance(Duration::from_secs(1));
            assert!(s.wait_watching(Duration::from_secs(60)), "the watcher answered");
            s.wait_idle();
        }
    };
    settle(&mut s);
    let away = disk.path().join("Lisbon");
    write_folder(&away);
    let dir = root.join("Lisbon");
    std::fs::rename(&away, &dir).unwrap();
    settle(&mut s);
    check(&mut s, &dir, "watcher");
}
