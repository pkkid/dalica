//! `people.json` (`data-model.md` §4): every person and the rejected suggestions round-trip with the
//! keys Dalica does not know kept; a lost file is rebuilt from the sidecars' people; a damaged one
//! is set aside; a failed write leaves the old or the new file; hostile files never panic.

use std::path::Path;

use dalica_core::{FolderId, PersonId, PhotoId, Rect01, Timestamp};
use dalica_fs::faults::{arm, disarm_all};
use dalica_library::{Face, FaceKey, FacePerson, Folder, Library, Op, Person, Photo};
use dalica_store::people::{self, PeopleRead, PeopleReading, PeopleState, PeopleWrite, PersonDef, Rejected};
use dalica_store::rebuild::import_library;
use dalica_store::{Outcome, write_folder};
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::common::{FAULTS, NOW, cases};

fn pid(s: &str) -> PersonId {
    PersonId::parse(s).unwrap()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
    Rect01::new(x0, y0, x1, y1).unwrap()
}

/// A library of one folder at `root` with photos `a.jpg` and `b.jpg`, Ana (a nickname and a
/// thumbnail) on both and Rui on `b.jpg`.
fn library(root: &Path) -> (Library, FolderId, [PhotoId; 2]) {
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.to_path_buf() } }).unwrap();
    let ids = [lib.next_photo_id(), lib.next_photo_id()];
    let photos = ids
        .iter()
        .zip(["a.jpg", "b.jpg"])
        .map(|(&id, name)| Photo { id, folder: f, name: name.into(), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false })
        .collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    let ana = Person { name: "Ana Costa".into(), nickname: "Aninhas".into(), thumbnail: None };
    lib.apply(Op::AddPerson { id: pid("p7f3a2c"), person: ana }).unwrap();
    lib.apply(Op::AddPerson { id: pid("p0b1d"), person: Person::named("Rui") }).unwrap();
    let faces = vec![
        (ids[0], vec![Face::new(rect(0.1, 0.1, 0.3, 0.3), FacePerson::Person(pid("p7f3a2c")))]),
        (
            ids[1],
            vec![
                Face::new(rect(0.5, 0.5, 0.7, 0.7), FacePerson::Person(pid("p7f3a2c"))),
                Face::new(rect(0.1, 0.1, 0.2, 0.2), FacePerson::Person(pid("p0b1d"))),
            ],
        ),
    ];
    lib.apply(Op::SetFaces { faces }).unwrap();
    lib.apply(Op::SetPersonThumbnail { id: pid("p7f3a2c"), thumbnail: Some(FaceKey { photo: ids[1], rect: rect(0.5, 0.5, 0.7, 0.7) }) }).unwrap();
    (lib, f, ids)
}

fn read(dir: &Path) -> PeopleReading {
    match people::read(dir, NOW) {
        PeopleRead::Read(r) => r,
        other => panic!("{other:?}"),
    }
}

fn json_of(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(people::path(dir)).unwrap()).unwrap()
}

#[test]
fn people_json_round_trips_with_unknown_keys_kept() {
    let photos = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (lib, _, _) = library(photos.path());
    let state = people::state(&lib);
    assert_eq!(people::write(dir.path(), &state, NOW), PeopleWrite::Written);
    let back = read(dir.path());
    assert_eq!(back.people, state.people);
    assert_eq!(back.people[&pid("p7f3a2c")].thumbnail, Some((photos.path().join("b.jpg"), rect(0.5, 0.5, 0.7, 0.7))));
    assert!(back.rejected.is_empty() && back.notes.is_empty(), "{back:?}");
    assert_eq!(people::write(dir.path(), &state, NOW), PeopleWrite::Unchanged, "the same people change no byte");
    // Sorted keys at every level, two-space indentation, a final newline.
    let text = std::fs::read_to_string(people::path(dir.path())).unwrap();
    assert!(
        text.starts_with("{\n  \"dalica\": {\n    \"version\": 1\n  },\n  \"people\": {\n    \"p0b1d\": {\n      \"name\": \"Rui\"\n    },"),
        "{text}"
    );
    assert!(text.ends_with("}\n"));

    // Keys and entries Dalica does not know stay, and so do the rejected suggestions it is not
    // given; one it is given replaces the usable ones, keeping their unknown keys.
    let mut doc = json_of(dir.path());
    doc["x-top"] = json!({"kept": true});
    doc["people"]["p7f3a2c"]["x-entry"] = json!(1);
    doc["people"]["p7f3a2c"]["thumbnail"]["x-thumb"] = json!(2);
    doc["people"]["not-an-id"] = json!({"name": "Zé"});
    doc["people"]["p0000ff"] = json!({"name": "  "});
    let porto = photos.path().join("c.jpg").to_str().unwrap().to_string();
    doc["rejected"] = json!([
        {"photo": porto, "rect": [0.1, 0.2, 0.3, 0.4], "person": "p0b1d", "x-why": "twins"},
        {"photo": porto, "rect": [0.5, 0.5, 0.4, 0.6], "person": "p0b1d"},
        "junk",
    ]);
    std::fs::write(people::path(dir.path()), serde_json::to_vec(&doc).unwrap()).unwrap();
    let back = read(dir.path());
    assert_eq!(back.rejected, [Rejected { photo: porto.clone().into(), rect: rect(0.1, 0.2, 0.3, 0.4), person: pid("p0b1d") }]);
    assert_eq!(back.notes.len(), 4, "two people entries and two rejected items not used: {:?}", back.notes);
    assert!(back.notes.iter().all(|n| !n.contains("Ana") && !n.contains("Zé")), "notes never carry names: {:?}", back.notes);
    assert_eq!(people::write(dir.path(), &state, NOW), PeopleWrite::Written);
    let written = json_of(dir.path());
    for (pointer, want) in [
        ("/x-top/kept", json!(true)),
        ("/people/p7f3a2c/x-entry", json!(1)),
        ("/people/p7f3a2c/thumbnail/x-thumb", json!(2)),
        ("/people/not-an-id/name", json!("Zé")),
        ("/people/p0000ff/name", json!("  ")),
        ("/rejected/0/x-why", json!("twins")),
        ("/rejected/2", json!("junk")),
    ] {
        assert_eq!(written.pointer(pointer), Some(&want), "{pointer}");
    }
    let mut with_rejected = state.clone();
    let rejected = vec![
        Rejected { photo: porto.clone().into(), rect: rect(0.1, 0.2, 0.3, 0.4), person: pid("p0b1d") },
        Rejected { photo: porto.clone().into(), rect: rect(0.6, 0.6, 0.8, 0.8), person: pid("p7f3a2c") },
    ];
    with_rejected.rejected = Some(rejected.clone());
    assert_eq!(people::write(dir.path(), &with_rejected, NOW), PeopleWrite::Written);
    let written = json_of(dir.path());
    assert_eq!(written["rejected"][0]["x-why"], "twins", "a pair written again keeps its keys");
    assert_eq!(written["rejected"].as_array().map(Vec::len), Some(4), "two pairs, then the two unusable items kept");
    assert_eq!(read(dir.path()).rejected, rejected);

    // A person the model no longer has goes; their rejected pairs go when the state says so.
    let mut fewer = with_rejected.clone();
    fewer.people.remove(&pid("p0b1d"));
    fewer.rejected = Some(rejected[1..].to_vec());
    assert_eq!(people::write(dir.path(), &fewer, NOW), PeopleWrite::Written);
    let back = read(dir.path());
    assert_eq!((back.people.len(), back.rejected), (1, rejected[1..].to_vec()));
}

/// `data-model.md` §4: `people.json` is rebuildable from the sidecars. With it gone, the people
/// come back from the folders' `"people"` (names and nicknames; the thumbnails and the rejected
/// suggestions start empty) and the file is written anew from the model.
#[test]
fn a_lost_people_json_is_rebuilt_from_the_sidecars() {
    let photos = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (lib, f, ids) = library(photos.path());
    assert_eq!(write_folder(&dalica_store::mapping::folder_state(&lib, f).unwrap(), NOW), Outcome::Written);
    assert_eq!(people::write(dir.path(), &people::state(&lib), NOW), PeopleWrite::Written);
    std::fs::remove_file(people::path(dir.path())).unwrap();

    let (mut rebuilt, _, _) = {
        let mut l = Library::new();
        let f = l.next_folder_id();
        l.apply(Op::AddFolder { folder: Folder { id: f, path: photos.path().to_path_buf() } }).unwrap();
        let photos = ["a.jpg", "b.jpg"]
            .iter()
            .map(|n| Photo {
                id: l.next_photo_id(),
                folder: f,
                name: (*n).into(),
                size: 1,
                mtime: Timestamp::EPOCH,
                width: 4,
                height: 3,
                date: None,
                star: false,
            })
            .collect::<Vec<_>>();
        let ids: Vec<PhotoId> = photos.iter().map(|p| p.id).collect();
        l.apply(Op::AddPhotos { photos }).unwrap();
        (l, f, ids)
    };
    let import = import_library(&rebuilt, dir.path(), NOW);
    assert!(import.library_problems.is_empty() && import.problems.is_empty(), "{import:?}");
    for op in import.ops {
        rebuilt.apply(op).unwrap();
    }
    let ana = rebuilt.person(&pid("p7f3a2c")).unwrap();
    assert_eq!((ana.name.as_str(), ana.nickname.as_str(), ana.thumbnail), ("Ana Costa", "Aninhas", None));
    assert_eq!(rebuilt.person(&pid("p0b1d")).map(|p| p.name.as_str()), Some("Rui"));
    assert_eq!(rebuilt.faces(ids[1]), lib.faces(ids[1]));
    assert_eq!(people::write(dir.path(), &people::state(&rebuilt), NOW), PeopleWrite::Written);
    let back = read(dir.path());
    assert_eq!(back.people.keys().collect::<Vec<_>>(), [&pid("p0b1d"), &pid("p7f3a2c")]);
    assert!(back.rejected.is_empty());
}

/// Two people of one name (from two libraries' folders) are one person: the second's faces are
/// read as the first's, and the import says so by id.
#[test]
fn a_name_held_by_another_person_is_that_person() {
    let photos = tempfile::tempdir().unwrap();
    let doc = json!({
        "people": {"p0001": {"name": "Ana"}, "p0002": {"name": "ANA"}},
        "photos": {
            "a.jpg": {"faces": [{"rect": [0.1, 0.1, 0.2, 0.2], "person": "p0001"}]},
            "b.jpg": {"faces": [{"rect": [0.3, 0.3, 0.4, 0.4], "person": "p0002"}]}
        }
    });
    std::fs::write(photos.path().join(".dalica.json"), doc.to_string()).unwrap();
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: photos.path().to_path_buf() } }).unwrap();
    let ids = [lib.next_photo_id(), lib.next_photo_id()];
    let list = ids
        .iter()
        .zip(["a.jpg", "b.jpg"])
        .map(|(&id, n)| Photo { id, folder: f, name: n.into(), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false })
        .collect();
    lib.apply(Op::AddPhotos { photos: list }).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let import = import_library(&lib, dir.path(), NOW);
    assert_eq!(import.library_problems, ["person p0002 has the name of person p0001 (ignoring case): their faces are p0001's"]);
    for op in import.ops {
        lib.apply(op).unwrap();
    }
    assert_eq!(lib.people().count(), 1);
    assert_eq!(lib.faces(ids[1])[0].person, FacePerson::Person(pid("p0001")));
}

#[test]
fn a_damaged_people_json_is_set_aside_and_written_anew() {
    let photos = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (lib, _, _) = library(photos.path());
    for damaged in [&b"{\"people\": "[..], b"[1, 2]", b"\xff\xfe"] {
        std::fs::write(people::path(dir.path()), damaged).unwrap();
        let PeopleRead::SetAside { kept: Some(kept), .. } = people::read(dir.path(), NOW) else { panic!("set aside") };
        assert_eq!(std::fs::read(&kept).unwrap(), damaged, "kept as it was");
        std::fs::remove_file(kept).unwrap();
        std::fs::write(people::path(dir.path()), damaged).unwrap();
        let PeopleWrite::SetAsideAndWritten { kept: Some(kept), .. } = people::write(dir.path(), &people::state(&lib), NOW) else {
            panic!("written")
        };
        assert_eq!(std::fs::read(&kept).unwrap(), damaged);
        assert_eq!(read(dir.path()).people.len(), 2);
        std::fs::remove_file(kept).unwrap();
    }
}

/// A fault before the file is replaced and one after it, and a crash: the file holds the old
/// people or the new, whole, and the next write lands.
#[test]
fn a_failed_write_leaves_the_old_or_the_new_people() {
    for (point, fault) in FAULTS {
        let photos = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (mut lib, _, _) = library(photos.path());
        assert_eq!(people::write(dir.path(), &people::state(&lib), NOW), PeopleWrite::Written);
        let old = read(dir.path()).people;
        lib.apply(Op::RenamePerson { id: pid("p0b1d"), name: "Rui Lopes".into(), nickname: String::new() }).unwrap();
        arm(point, fault);
        let out = people::write(dir.path(), &people::state(&lib), NOW);
        disarm_all();
        assert!(matches!(out, PeopleWrite::Failed(_)), "{point} {fault:?}: {out:?}");
        let now = read(dir.path()).people;
        assert!(now == old || now == people::state(&lib).people, "{point} {fault:?}: {now:?}");
        assert!(matches!(people::write(dir.path(), &people::state(&lib), NOW), PeopleWrite::Written | PeopleWrite::Unchanged));
        assert_eq!(read(dir.path()).people, people::state(&lib).people, "{point} {fault:?}");
    }
}

#[test]
fn the_state_leaves_out_a_thumbnail_that_is_no_longer_theirs() {
    let photos = tempfile::tempdir().unwrap();
    let (mut lib, _, ids) = library(photos.path());
    lib.apply(Op::SetFaces { faces: vec![(ids[1], Vec::new())] }).unwrap();
    let state: PeopleState = people::state(&lib);
    assert_eq!(state.people[&pid("p7f3a2c")], PersonDef { name: "Ana Costa".into(), nickname: "Aninhas".into(), thumbnail: None });
}

proptest! {
    #![proptest_config(cases(256, 32))]

    /// Any bytes as `people.json` never panic: they read, or are set aside.
    #[test]
    fn hostile_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(people::path(dir.path()), &bytes).unwrap();
        let r = people::read(dir.path(), NOW);
        prop_assert!(matches!(r, PeopleRead::Read(_) | PeopleRead::SetAside { .. }), "{:?}", r);
    }

    /// Any JSON values in the places Dalica reads never panic, and writing keeps what it cannot use.
    #[test]
    fn hostile_values_never_panic(v in prop_oneof![
        Just(json!(null)), Just(json!(1.5)), Just(json!("p7f3a2c")), Just(json!([])), Just(json!({})),
        Just(json!({"name": 7})), Just(json!({"name": "A", "nickname": [1]})), Just(json!({"name": "A", "thumbnail": "x"})),
        Just(json!({"name": "A", "thumbnail": {"photo": "", "rect": [0, 0, 1, 1]}})),
        Just(json!([{"photo": "/x", "rect": [1, 1, 0, 0], "person": "p0001"}])),
    ]) {
        let dir = tempfile::tempdir().unwrap();
        let doc = json!({"people": {"p0001": v.clone(), "p0002": {"name": "B"}}, "rejected": v.clone()});
        std::fs::write(people::path(dir.path()), doc.to_string()).unwrap();
        let PeopleRead::Read(r) = people::read(dir.path(), NOW) else { panic!("readable JSON") };
        prop_assert!(r.people.contains_key(&pid("p0002")));
        let state = PeopleState { people: r.people.clone(), rejected: None };
        prop_assert!(matches!(people::write(dir.path(), &state, NOW), PeopleWrite::Written | PeopleWrite::Unchanged));
        prop_assert_eq!(&json_of(dir.path())["rejected"], &v);
    }
}
