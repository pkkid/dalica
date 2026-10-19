//! Legacy data on disk (`data-model.md` §9, §10.11; M14.12): the first write of a folder whose only
//! sidecar is the previous organizer's, the legacy ids in `albums.json` and `people.json`, and the
//! rebuild. The legacy file is a fixture written by the test under the reader's own name
//! (`dalica_sidecar::legacy::sidecar_names`); it must come out byte-identical with its time.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use dalica_core::{AlbumId, FolderId, PersonId, PhotoId, PosKey, Rect01, Timestamp};
use dalica_fs::atomic::is_temp_name;
use dalica_fs::faults::{Fault, arm, disarm_all, points, run_at};
use dalica_library::{
    AlbumInfo, Face, FacePerson, Folder, FolderInfo, FolderSidecar, LegacyOriginal, Library, Op, Person, Photo, PhotoInfo, Project,
};
use dalica_sidecar::legacy::sidecar_names;
use dalica_sidecar::{FILE_NAME, Limits, Sidecar};
use dalica_store::carry::{Carry, Mode};
use dalica_store::legacy::{self, FirstWrite};
use dalica_store::mapping::{self, FOLDER_DATA_M14, FolderState, LEGACY_ORIGINAL_FIELDS, LIBRARY_DATA_M14, PHOTO_DATA_M14, Source};
use dalica_store::rebuild::{import_library, import_sidecars};
use dalica_store::{LegacyWrite, Outcome, SidecarWriter, albums, people, snapshot, write_folder};
use serde_json::{Value, json};

use crate::common::NOW;

const LEGACY_TEXT: &[u8] = b"[a.jpg]\r\nstar=yes\r\ncaption=Tram 28\r\n[b.jpg]\r\nkeywords=lisbon\r\n";

fn album() -> AlbumId {
    AlbumId::parse("a91c0e").unwrap()
}

fn person() -> PersonId {
    PersonId::parse("p7f3a2c").unwrap()
}

fn rect() -> Rect01 {
    Rect01::new(0.1, 0.2, 0.3, 0.4).unwrap()
}

/// A folder at `root` with photos `a.jpg`, `b.jpg` and `c.jpg` and a legacy sidecar, holding what
/// a scan of the legacy sidecar gives the model (applied as the scan's imports are): `a.jpg`'s
/// caption, `b.jpg`'s tags and recipe (one unmapped `legacy:` entry in its place), an album and a
/// person with their legacy ids, `a.jpg` in the album and the person's face on `b.jpg`, the
/// folder's description, and the fact `legacy`.
fn legacy_folder(root: &Path) -> (Library, FolderId, [PhotoId; 3], PathBuf) {
    let legacy_file = root.join(&sidecar_names()[0]);
    std::fs::write(&legacy_file, LEGACY_TEXT).unwrap();
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.to_path_buf() } }).unwrap();
    let ids = [lib.next_photo_id(), lib.next_photo_id(), lib.next_photo_id()];
    let photos = ids
        .iter()
        .zip(["a.jpg", "b.jpg", "c.jpg"])
        .map(|(&id, name)| {
            std::fs::write(root.join(name), b"jpeg").unwrap();
            Photo { id, folder: f, name: name.into(), size: 4, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false }
        })
        .collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    let a = PhotoInfo { caption: "Tram 28".into(), ..PhotoInfo::default() };
    let b = PhotoInfo { tags: vec!["lisbon".into()], edits: vec!["legacy:radblur(1,0.5)".into(), "bw".into()], ..PhotoInfo::default() };
    let ops = vec![
        Op::SetPhotoInfo { photos: vec![(ids[0], a), (ids[1], b)] },
        Op::SetFolderInfo { folders: vec![(f, FolderInfo { description: "Lisbon trip".into(), ..FolderInfo::default() })] },
        Op::CreateAlbums { albums: vec![(album(), AlbumInfo::named("Trams"))] },
        Op::AddMembers { members: vec![(album(), ids[0], PosKey::parse("a0").unwrap())] },
        Op::AddPerson { id: person(), person: Person::named("Ana Costa") },
        Op::SetFaces { faces: vec![(ids[1], vec![Face::new(rect(), FacePerson::Person(person()))])] },
        Op::SetLegacyIds { albums: vec![(album(), Some("4f2a91".into()))], people: vec![(person(), Some("c0ffee12".into()))] },
        Op::SetFolderSidecars { folders: vec![(f, FolderSidecar::Legacy)] },
    ];
    lib.apply(Op::Batch { ops }).unwrap();
    (lib, f, ids, legacy_file)
}

fn stamp(path: &Path) -> (Vec<u8>, SystemTime) {
    (std::fs::read(path).unwrap(), std::fs::metadata(path).unwrap().modified().unwrap())
}

fn sidecar_json(folder: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(folder.join(FILE_NAME)).unwrap()).unwrap()
}

/// Every name in `folder` that is a temporary file of `dalica-fs`.
fn temps(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .filter(|n| is_temp_name(n))
        .map(|n| n.to_string_lossy().into())
        .collect()
}

#[test]
fn starring_one_photo_writes_every_photos_data_and_leaves_the_legacy_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut lib, f, ids, legacy_file) = legacy_folder(dir.path());
    let before = stamp(&legacy_file);
    lib.apply(Op::SetStars { stars: vec![(ids[2], true)] }).unwrap();
    let mut w = SidecarWriter::new();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, NOW)[&f], Outcome::Written);
    assert_eq!(w.take_legacy().into_iter().collect::<Vec<_>>(), [(f, LegacyWrite::Converted)]);
    assert!(w.converted(f));
    let j = sidecar_json(dir.path());
    assert_eq!(j["photos"]["a.jpg"]["caption"], "Tram 28");
    assert_eq!(j["photos"]["a.jpg"]["albums"], json!({"a91c0e": "a0"}));
    assert_eq!(j["photos"]["b.jpg"]["tags"], json!(["lisbon"]));
    assert_eq!(j["photos"]["b.jpg"]["edits"], json!(["legacy:radblur(1,0.5)", "bw"]), "an unmapped entry stays in its place");
    assert_eq!(j["photos"]["b.jpg"]["faces"][0]["person"], "p7f3a2c");
    assert_eq!(j["photos"]["c.jpg"]["star"], true, "the change itself");
    assert_eq!(j["folder"]["description"], "Lisbon trip");
    assert_eq!(j["albums"]["a91c0e"]["name"], "Trams");
    assert_eq!(stamp(&legacy_file), before, "the legacy file is byte-identical with its time");

    // From now on the folder is an ordinary one: the next change merges into Dalica's own file, and
    // that file alone gives a fresh model everything.
    lib.apply(Op::SetCaptions { captions: vec![(ids[1], "Alfama".into())] }).unwrap();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, NOW)[&f], Outcome::Written);
    assert!(w.take_legacy().is_empty(), "reported once");
    assert_eq!(sidecar_json(dir.path())["photos"]["b.jpg"]["caption"], "Alfama");
    let mut fresh = Library::new();
    fresh.apply(Op::AddFolder { folder: Folder { id: f, path: dir.path().to_path_buf() } }).unwrap();
    let photos: Vec<Photo> = lib.photos_in(f).into_iter().map(|p| Photo { star: false, ..p.clone() }).collect();
    fresh.apply(Op::AddPhotos { photos }).unwrap();
    for op in import_sidecars(&fresh, NOW).ops {
        fresh.apply(op).unwrap();
    }
    assert!(fresh.photo(ids[2]).unwrap().star);
    assert_eq!(fresh.info(ids[0]).caption, "Tram 28");
    assert_eq!(fresh.info(ids[1]).edits, lib.info(ids[1]).edits);
    assert_eq!(stamp(&legacy_file), before);
}

#[test]
fn a_dalica_json_made_by_another_program_is_never_replaced() {
    // There before the write (made between the scan's read and the write).
    let dir = tempfile::tempdir().unwrap();
    let (mut lib, f, ids, legacy_file) = legacy_folder(dir.path());
    let before = stamp(&legacy_file);
    let foreign = b"{\"photos\": {\"a.jpg\": {\"star\": true}}}\n";
    std::fs::write(dir.path().join(FILE_NAME), foreign).unwrap();
    lib.apply(Op::SetStars { stars: vec![(ids[2], true)] }).unwrap();
    let mut w = SidecarWriter::new();
    w.mark_dirty(f);
    assert!(matches!(&w.flush(&lib, NOW)[&f], Outcome::Failed(m) if m.contains("appeared")));
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), foreign);
    assert_eq!(w.take_legacy().into_iter().collect::<Vec<_>>(), [(f, LegacyWrite::Appeared)]);
    assert!(!w.is_dirty(f) && !w.converted(f), "not retried: the folder is read again");

    // Made while the write runs, after its first look and before its last check.
    let dir = tempfile::tempdir().unwrap();
    let (lib, f, _, _) = legacy_folder(dir.path());
    let late = dir.path().join(FILE_NAME);
    let planted = late.clone();
    run_at(points::FSYNC, move || std::fs::write(&planted, b"{}\n").unwrap());
    let state = mapping::folder_state(&lib, f).unwrap();
    assert_eq!(legacy::first_write(&state, NOW), FirstWrite::Appeared);
    disarm_all();
    assert_eq!(std::fs::read(&late).unwrap(), b"{}\n");
    assert!(temps(dir.path()).is_empty(), "{:?}", temps(dir.path()));
    assert_eq!(stamp(&legacy_file), before);
}

#[test]
fn a_crash_before_the_commit_leaves_no_file_and_one_after_it_the_whole_file() {
    let twin = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (lib, f, _, legacy_file) = legacy_folder(dir.path());
    let before = stamp(&legacy_file);
    let state = mapping::folder_state(&lib, f).unwrap();
    // The bytes a clean first write gives.
    assert_eq!(legacy::first_write(&FolderState { path: twin.path().to_path_buf(), ..state.clone() }, NOW), FirstWrite::Converted);
    let whole = std::fs::read(twin.path().join(FILE_NAME)).unwrap();

    arm(points::WRITE, Fault::Crash);
    assert!(matches!(write_folder(&state, NOW), Outcome::Failed(_)));
    disarm_all();
    assert!(!dir.path().join(FILE_NAME).exists(), "before the commit: no .dalica.json");

    for t in temps(dir.path()) {
        std::fs::remove_file(dir.path().join(t)).unwrap();
    }
    arm(points::FSYNC_DIR, Fault::Crash);
    assert!(matches!(write_folder(&state, NOW), Outcome::Failed(_)));
    disarm_all();
    let after = std::fs::read(dir.path().join(FILE_NAME)).unwrap();
    assert_eq!(after, whole, "after the commit: the whole file");
    assert!(Sidecar::parse(&after, &Limits::default()).is_ok());
    assert_eq!(stamp(&legacy_file), before);
}

#[test]
fn a_carry_into_a_legacy_only_folder_writes_its_whole_first_file_first() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let (lib, f, _, legacy_file) = legacy_folder(dest.path());
    let before = stamp(&legacy_file);
    std::fs::write(src.path().join(FILE_NAME), br#"{"photos": {"x.jpg": {"star": true, "other": 1}}}"#).unwrap();
    std::fs::write(dest.path().join("x.jpg"), b"jpeg").unwrap();
    let from = FolderId::new(99);
    let mut to_state = mapping::folder_state(&lib, f).unwrap();
    to_state.photos.push(("x.jpg".into(), mapping::PhotoState { star: true, ..Default::default() }));
    let from_state = FolderState { id: from, path: src.path().to_path_buf(), sidecar: FolderSidecar::Dalica, ..FolderState::default() };
    let state_of = |id: FolderId| if id == f { Some(to_state.clone()) } else { Some(from_state.clone()) };
    let mut w = SidecarWriter::new();
    let report = w.carry(&Carry::one(Mode::Move, src.path(), "x.jpg", dest.path(), "x.jpg"), from, f, state_of, NOW);
    assert!(report.carried.is_ok(), "{report:?}");
    let j = sidecar_json(dest.path());
    assert_eq!(j["photos"]["x.jpg"]["other"], 1, "the carried entry");
    assert_eq!(j["photos"]["a.jpg"]["caption"], "Tram 28", "and every photo the folder had");
    assert_eq!(w.take_legacy().get(&f), Some(&LegacyWrite::Converted));
    assert_eq!(stamp(&legacy_file), before);
}

#[test]
fn legacy_ids_survive_a_restart_and_a_rebuild_of_albums_and_people_json() {
    let photos = tempfile::tempdir().unwrap();
    let libdir = tempfile::tempdir().unwrap();
    let (mut lib, f, ids, _) = legacy_folder(photos.path());
    let original = LegacyOriginal { size: 9, digest: "d1".into(), recipe: vec!["bw".into()] };
    lib.apply(Op::SetLegacyOriginals { originals: vec![(ids[0], Some(original.clone()))] }).unwrap();

    // A restart from a snapshot.
    let path = libdir.path().join(snapshot::FILE);
    snapshot::write(&path, &lib, 7).unwrap();
    let (back, seq) = snapshot::read(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(seq, 7);
    assert_eq!(back.album_by_legacy("4f2a91"), Some(&album()));
    assert_eq!(back.person_by_legacy("c0ffee12"), Some(&person()));
    assert_eq!(back.folder_sidecar(f), FolderSidecar::Legacy);
    assert_eq!(back.legacy_original(ids[0]), Some(&original));

    // albums.json and people.json keep the ids; a rebuild into an empty model brings them back.
    assert_eq!(albums::write(libdir.path(), &albums::state(&lib), NOW), albums::AlbumsWrite::Written);
    assert_eq!(people::write(libdir.path(), &people::state(&lib), NOW), people::PeopleWrite::Written);
    let a: Value = serde_json::from_slice(&std::fs::read(albums::path(libdir.path())).unwrap()).unwrap();
    let p: Value = serde_json::from_slice(&std::fs::read(people::path(libdir.path())).unwrap()).unwrap();
    assert_eq!((&a["albums"]["a91c0e"]["legacy"], &p["people"]["p7f3a2c"]["legacy"]), (&json!("4f2a91"), &json!("c0ffee12")));
    let mut fresh = Library::new();
    fresh.apply(Op::AddFolder { folder: Folder { id: f, path: photos.path().to_path_buf() } }).unwrap();
    let list: Vec<Photo> = lib.photos_in(f).into_iter().cloned().collect();
    fresh.apply(Op::AddPhotos { photos: list }).unwrap();
    let import = import_library(&fresh, libdir.path(), NOW);
    assert!(import.library_problems.is_empty(), "{:?}", import.library_problems);
    for op in import.ops {
        fresh.apply(op).unwrap();
    }
    assert_eq!(fresh.album_by_legacy("4f2a91"), Some(&album()));
    assert_eq!(fresh.person_by_legacy("c0ffee12"), Some(&person()));

    // A write whose state lacks an id (an album the session never saw) never removes it.
    let mut lost = albums::state(&lib);
    lost.legacy.clear();
    albums::write(libdir.path(), &lost, NOW);
    let a: Value = serde_json::from_slice(&std::fs::read(albums::path(libdir.path())).unwrap()).unwrap();
    assert_eq!(a["albums"]["a91c0e"]["legacy"], "4f2a91");
}

#[test]
fn a_rebuild_of_a_legacy_only_folder_keeps_what_the_first_scan_gave() {
    let photos = tempfile::tempdir().unwrap();
    let libdir = tempfile::tempdir().unwrap();
    let (lib, _, _, legacy_file) = legacy_folder(photos.path());
    let before = stamp(&legacy_file);
    albums::write(libdir.path(), &albums::state(&lib), NOW);
    people::write(libdir.path(), &people::state(&lib), NOW);
    // No `.dalica.json`: the scan's reading of the legacy file stands; nothing here changes it.
    let import = import_library(&lib, libdir.path(), NOW);
    assert!(import.ops.is_empty(), "{:?}", import.ops);
    assert!(!photos.path().join(FILE_NAME).exists());
    assert_eq!(stamp(&legacy_file), before);
}

#[test]
fn every_m14_field_is_placed_and_the_collages_mark_round_trips() {
    let LegacyOriginal { size: _, digest: _, recipe: _ } = LegacyOriginal::default();
    assert_eq!(LEGACY_ORIGINAL_FIELDS, [("size", Source::Index), ("digest", Source::Index), ("recipe", Source::Index)]);
    assert_eq!(PHOTO_DATA_M14, [("legacyOriginal", Source::Fields("LEGACY_ORIGINAL_FIELDS"))]);
    match FolderSidecar::None {
        FolderSidecar::None | FolderSidecar::Dalica | FolderSidecar::Legacy | FolderSidecar::Both => {
            assert_eq!(FOLDER_DATA_M14, [("sidecar", Source::Index)]);
        }
    }
    assert_eq!(
        LIBRARY_DATA_M14,
        [("albumLegacy", Source::Albums(dalica_sidecar::albums::LEGACY_KEY)), ("personLegacy", Source::People(people::LEGACY))]
    );
    assert_eq!(FolderState::default().sidecar, FolderSidecar::None);

    let dir = tempfile::tempdir().unwrap();
    let state = FolderState { path: dir.path().to_path_buf(), project: Some(Project::Collages), ..FolderState::default() };
    assert_eq!(write_folder(&state, NOW), Outcome::Written);
    assert_eq!(sidecar_json(dir.path())["folder"]["project"], "collages");
    let s = Sidecar::parse(&std::fs::read(dir.path().join(FILE_NAME)).unwrap(), &Limits::default()).unwrap();
    assert_eq!(mapping::read_folder(&s).project, mapping::Reading::Usable(Some(Project::Collages)));
}
