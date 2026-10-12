//! Stale sidecar entries through the writer (`data-model.md` §7, `roadmap.md` M5.17): a photo
//! deleted outside Dalica leaves the model at once, its sidecar entry is marked on the folder's
//! next write, survives 29 days and is pruned by the first write after 30; a file that comes back,
//! a file the scans skip and another program's entry are never pruned.

use std::path::Path;
use std::time::Duration;

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo};
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::stale::{MISSING_KEY, PRUNE_AFTER};
use dalica_store::{Outcome, SidecarWriter};
use serde_json::Value;

const T0: Timestamp = Timestamp::from_millis(1_790_000_000_000);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn days(n: u32) -> Timestamp {
    T0.saturating_add(DAY * n)
}

/// A folder holding `names` on disk and in the model.
fn folder(dir: &Path, names: &[&str]) -> (Library, FolderId, Vec<PhotoId>) {
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: dir.to_path_buf() } }).unwrap();
    let mut ids = Vec::new();
    for name in names {
        std::fs::write(dir.join(name), name.as_bytes()).unwrap();
        let id = lib.next_photo_id();
        let photo = Photo { id, folder: f, name: (*name).into(), size: 1, mtime: Timestamp::EPOCH, width: 64, height: 64, date: None, star: false };
        lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();
        ids.push(id);
    }
    (lib, f, ids)
}

fn star(lib: &mut Library, w: &mut SidecarWriter, id: PhotoId, on: bool, now: Timestamp) -> Outcome {
    lib.apply(Op::SetStars { stars: vec![(id, on)] }).unwrap();
    let f = lib.photo(id).unwrap().folder;
    w.mark_dirty(f);
    w.flush(lib, now).remove(&f).unwrap()
}

fn json(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(".dalica.json")).unwrap()).unwrap()
}

fn star_of(dir: &Path, name: &str) -> Field<bool> {
    Sidecar::parse(&std::fs::read(dir.join(".dalica.json")).unwrap(), &Limits::default()).unwrap().star(name)
}

/// The file is deleted outside Dalica and the scan removes the photo: the folder's next write
/// marks the entry; it survives 29 days of writes and the first write after 30 prunes it.
#[test]
fn a_missing_photos_entry_survives_29_days_and_is_gone_after_31() {
    let dir = tempfile::tempdir().unwrap();
    let (mut lib, f, ids) = folder(dir.path(), &["a.jpg", "b.jpg"]);
    let mut w = SidecarWriter::new();
    star(&mut lib, &mut w, ids[0], true, T0);
    star(&mut lib, &mut w, ids[1], true, T0);

    std::fs::remove_file(dir.path().join("b.jpg")).unwrap();
    lib.apply(Op::RemovePhotos { ids: vec![ids[1]] }).unwrap();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, T0)[&f], Outcome::Written, "the mark is written");
    let doc = json(dir.path());
    assert_eq!(doc["dalica"][MISSING_KEY]["b.jpg"], Value::String("2026-09-21T14:13:20Z".into()));
    assert_eq!(star_of(dir.path(), "b.jpg"), Field::Value(true), "the entry is kept");

    // Writes for other reasons, 29 days on: still there, mark unchanged.
    for d in [1, 15, 29] {
        star(&mut lib, &mut w, ids[0], d % 2 == 0, days(d));
        assert_eq!(star_of(dir.path(), "b.jpg"), Field::Value(true), "day {d}");
        assert_eq!(json(dir.path())["dalica"][MISSING_KEY]["b.jpg"], doc["dalica"][MISSING_KEY]["b.jpg"]);
    }
    // No write, no pruning: the time alone changes nothing on disk.
    let before = std::fs::read(dir.path().join(".dalica.json")).unwrap();
    assert!(w.flush(&lib, days(31)).is_empty());
    assert_eq!(std::fs::read(dir.path().join(".dalica.json")).unwrap(), before);
    // The folder's next write after 30 days prunes the entry and its mark.
    star(&mut lib, &mut w, ids[0], true, days(31));
    let doc = json(dir.path());
    assert!(doc["photos"].get("b.jpg").is_none(), "{doc}");
    assert!(doc["dalica"].get(MISSING_KEY).is_none(), "{doc}");
    assert_eq!(star_of(dir.path(), "a.jpg"), Field::Value(true));
    assert_eq!(PRUNE_AFTER, DAY * 30);
}

/// A file restored before the 30 days end is found again by the scan: its star is read from the
/// kept entry and the mark goes on the next write.
#[test]
fn a_file_that_comes_back_keeps_its_entry() {
    let dir = tempfile::tempdir().unwrap();
    let (mut lib, f, ids) = folder(dir.path(), &["a.jpg", "b.jpg"]);
    let mut w = SidecarWriter::new();
    star(&mut lib, &mut w, ids[1], true, T0);
    let bytes = std::fs::read(dir.path().join("b.jpg")).unwrap();
    std::fs::remove_file(dir.path().join("b.jpg")).unwrap();
    let gone = lib.photo(ids[1]).unwrap().clone();
    lib.apply(Op::RemovePhotos { ids: vec![ids[1]] }).unwrap();
    w.mark_dirty(f);
    w.flush(&lib, days(10));
    assert!(json(dir.path())["dalica"][MISSING_KEY].get("b.jpg").is_some());

    // Restored from the trash; the scan adds it again with the star its entry kept.
    std::fs::write(dir.path().join("b.jpg"), bytes).unwrap();
    let id = lib.next_photo_id();
    lib.apply(Op::AddPhotos { photos: vec![Photo { id, star: true, ..gone }] }).unwrap();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, days(20))[&f], Outcome::Written);
    assert!(json(dir.path())["dalica"].get(MISSING_KEY).is_none());
    star(&mut lib, &mut w, ids[0], true, days(400));
    assert_eq!(star_of(dir.path(), "b.jpg"), Field::Value(true), "never pruned");
}

/// Only Dalica's entries whose file is gone are ever marked: a file on disk the model skips (too
/// small, a type turned off) and another program's entry stay, whatever the time.
#[test]
fn skipped_files_and_other_programs_entries_are_never_pruned() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("tiny.png"), b"x").unwrap();
    std::fs::write(
        dir.path().join(".dalica.json"),
        br#"{"dalica": {"version": 1}, "photos": {"tiny.png": {"star": true}, "theirs.jpg": {"rating": 5}, "..": {"star": true}}}"#,
    )
    .unwrap();
    let (mut lib, _f, ids) = folder(dir.path(), &["a.jpg"]);
    let mut w = SidecarWriter::new();
    for d in [0, 40, 400] {
        star(&mut lib, &mut w, ids[0], d != 40, days(d));
        let doc = json(dir.path());
        assert!(doc["dalica"].get(MISSING_KEY).is_none(), "day {d}: {doc}");
        for name in ["tiny.png", "theirs.jpg", ".."] {
            assert!(doc["photos"].get(name).is_some(), "day {d}: {name}");
        }
    }
}
