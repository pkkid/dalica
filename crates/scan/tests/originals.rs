//! A folder's originals record as the scan reads it (`data-model.md` §6, §7): read on the worker
//! while the walk still skips `.dalica-originals`, turned into derived `SetSaved` ops, hostile
//! names kept but never used, a damaged record set aside and reported, a link never followed.

use std::path::Path;

use crate::common::put;
use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo, Project, SaveState, Saved, SavedAs};
use dalica_scan::originals::{self, Originals};
use dalica_scan::scan;
use dalica_scenes::{Scene, jpeg};
use dalica_sidecar::{Limits, Sidecar};

const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

fn record(dir: &Path, text: &str) {
    put(&dir.join(".dalica-originals").join(".dalica.json"), text.as_bytes());
}

/// A library holding `dir` with photos `a.jpg`, `b.jpg` and `c.jpg`.
fn library(dir: &Path) -> (Library, FolderId, Vec<PhotoId>) {
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: dir.to_path_buf() } }).unwrap();
    let ids: Vec<PhotoId> = (0..3).map(|_| lib.next_photo_id()).collect();
    let photos = ids
        .iter()
        .zip(["a.jpg", "b.jpg", "c.jpg"])
        .map(|(&id, name)| Photo { id, folder: f, name: name.into(), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false })
        .collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    (lib, f, ids)
}

const RECORD: &str = r#"{
  "dalica": { "version": 1 },
  "originals": {
    "a.jpg": { "recipe": ["rotate(90)", "frobnicate(7)"], "size": 2048, "mtime": 1780000000123, "width": 16, "height": 12,
               "digest": "md5:00112233445566778899aabbccddeeff", "state": "done", "time": 1790000000000 },
    "b.jpg": { "size": 10, "mtime": 5, "state": "pending" },
    "../x": { "size": 1, "mtime": 0, "state": "done" },
    "CON": { "size": 1, "mtime": 0, "state": "done" },
    "c.jpg": { "size": "big", "mtime": 0, "state": "done" },
    "gone.jpg": { "size": 1, "mtime": 0, "state": "done" }
  }
}
"#;

fn saved_a() -> Saved {
    Saved {
        recipe: vec!["rotate(90)".into(), "frobnicate(7)".into()],
        size: 2048,
        mtime: Timestamp::from_millis(1_780_000_000_123),
        width: 16,
        height: 12,
        digest: "md5:00112233445566778899aabbccddeeff".into(),
        state: SaveState::Done,
        time: Some(Timestamp::from_millis(1_790_000_000_000)),
    }
}

#[test]
fn the_record_becomes_derived_saved_records_and_the_walk_still_skips_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let photo = jpeg(&Scene::new(1, 16, 16), 80, None).unwrap();
    put(&d.join("a.jpg"), &photo);
    put(&d.join(".dalica-originals/a.jpg"), &photo);
    record(d, RECORD);
    let before = std::fs::read(originals::record_path(d)).unwrap();

    // The walk lists the photo, never the kept original or the record.
    let s = scan(&[d.to_path_buf()], None);
    let listed: Vec<&str> = s.folders.iter().flat_map(|f| f.photos.iter().map(|p| p.name.as_str())).collect();
    assert_eq!(listed, ["a.jpg"]);

    let found = originals::read(d, NOW);
    let Originals::Read { saved, notes } = &found else { panic!("{found:?}") };
    let names: Vec<&str> = saved.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["a.jpg", "b.jpg", "gone.jpg"], "plain names with readable entries only");
    assert_eq!(saved[0].1, saved_a());
    assert_eq!(found.pending(), ["b.jpg"]);
    for bad in ["/originals/..~1x", "/originals/CON", "/originals/c.jpg/size"] {
        assert!(notes.iter().any(|n| n.contains(bad)), "{bad} in {notes:?}");
    }
    // Reading wrote nothing.
    assert_eq!(std::fs::read(originals::record_path(d)).unwrap(), before);

    // The op sets the records of the folder's photos only, as derived data.
    let (mut lib, f, ids) = library(d);
    let revision = lib.revision();
    let op = originals::ops(&lib, f, &found).unwrap();
    assert!(op.is_derived());
    lib.apply(op).unwrap();
    assert_eq!(lib.revision(), revision, "loading records moves no cache");
    assert_eq!(lib.saved(ids[0]), Some(&saved_a()));
    assert_eq!(lib.saved(ids[1]).map(|s| s.state), Some(SaveState::Pending));
    assert_eq!(lib.saved(ids[2]), None, "a wrong-type entry is not used");
    assert!(lib.can_undo_save(ids[0]) && !lib.can_undo_save(ids[1]));
    // Reading again changes nothing.
    assert_eq!(originals::ops(&lib, f, &originals::read(d, NOW)), None);

    // A record that lost an entry (Undo Save elsewhere) clears it; one gone entirely clears all.
    record(d, r#"{"originals": {"b.jpg": { "size": 10, "mtime": 5, "state": "done" }}}"#);
    lib.apply(originals::ops(&lib, f, &originals::read(d, NOW)).unwrap()).unwrap();
    assert_eq!((lib.saved(ids[0]), lib.saved(ids[1]).map(|s| s.state)), (None, Some(SaveState::Done)));
    std::fs::remove_file(originals::record_path(d)).unwrap();
    assert_eq!(originals::read(d, NOW), Originals::Absent);
    lib.apply(originals::ops(&lib, f, &Originals::Absent).unwrap()).unwrap();
    assert_eq!(lib.saved_all().count(), 0);
    std::fs::remove_dir_all(d.join(".dalica-originals")).unwrap();
    assert_eq!(originals::read(d, NOW), Originals::Absent);
}

#[test]
fn a_damaged_record_is_set_aside_and_reported_and_the_model_keeps_its_records() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    record(d, RECORD);
    let (mut lib, f, ids) = library(d);
    lib.apply(originals::ops(&lib, f, &originals::read(d, NOW)).unwrap()).unwrap();
    let damaged = b"{\"originals\": {\"a.jpg\": ";
    put(&originals::record_path(d), damaged);
    let found = originals::read(d, NOW);
    let Originals::SetAside { why, kept: Some(kept) } = &found else { panic!("{found:?}") };
    assert!(why.contains("JSON"), "{why}");
    let kept_name = kept.file_name().unwrap().to_string_lossy().into_owned();
    assert!(kept_name.starts_with(".dalica.json.corrupt-") && kept_name.ends_with('Z'), "{kept_name}");
    assert_eq!(kept.parent(), Some(d.join(".dalica-originals").as_path()));
    assert_eq!(std::fs::read(kept).unwrap(), damaged, "kept byte for byte");
    assert!(!originals::record_path(d).exists(), "a later Save starts a new record");
    // Never remove what was not seen.
    assert_eq!(originals::ops(&lib, f, &found), None);
    assert_eq!(lib.saved(ids[0]), Some(&saved_a()));
    // Read again: the record is gone, so the next read finds none.
    assert_eq!(originals::read(d, NOW), Originals::Absent);
}

#[test]
fn a_file_or_a_link_in_place_of_the_originals_folder_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("photos");
    put(&d.join(".dalica-originals"), b"not a folder");
    assert!(matches!(originals::read(&d, NOW), Originals::Unreadable(_)));
    let (lib, f, _) = library(&d);
    assert_eq!(originals::ops(&lib, f, &originals::read(&d, NOW)), None);
    #[cfg(unix)]
    {
        let elsewhere = dir.path().join("elsewhere");
        record(&elsewhere, RECORD);
        let e = dir.path().join("linked");
        std::fs::create_dir_all(&e).unwrap();
        std::os::unix::fs::symlink(elsewhere.join(".dalica-originals"), e.join(".dalica-originals")).unwrap();
        assert!(matches!(originals::read(&e, NOW), Originals::Unreadable(_)), "{:?}", originals::read(&e, NOW));
        // A link in place of the record itself is refused too.
        let r = dir.path().join("record-link");
        std::fs::create_dir_all(r.join(".dalica-originals")).unwrap();
        std::os::unix::fs::symlink(originals::record_path(&elsewhere), originals::record_path(&r)).unwrap();
        assert!(matches!(originals::read(&r, NOW), Originals::Unreadable(_)));
        assert_eq!(std::fs::read_to_string(originals::record_path(&elsewhere)).unwrap(), RECORD);
    }
}

#[test]
fn saved_as_and_the_project_mark_are_read_as_new_photos_and_folders_take_them() {
    let text = r#"{"folder": {"project": "exported"}, "photos": {
        "a.CR2": {"savedAs": {"name": "a.jpg", "recipe": ["sepia()"]}},
        "b.CR2": {"savedAs": {"name": "../b.jpg"}},
        "c.CR2": {"savedAs": {"name": "CON"}},
        "d.CR2": {"savedAs": 7}
    }}"#;
    let s = Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(originals::saved_as(&s, "a.CR2"), Some(SavedAs { name: "a.jpg".into(), recipe: vec!["sepia()".into()] }));
    for name in ["b.CR2", "c.CR2", "d.CR2", "missing.CR2"] {
        assert_eq!(originals::saved_as(&s, name), None, "{name}");
    }
    assert_eq!(originals::project(&s), Some(Project::Exported));
    for odd in [r#"{"folder": {"project": "album"}}"#, r#"{"folder": {"project": true}}"#, r#"{"folder": 3}"#, "{}"] {
        let s = Sidecar::parse(odd.as_bytes(), &Limits::default()).unwrap();
        assert_eq!(originals::project(&s), None, "{odd}");
    }
}

#[test]
fn records_past_the_caps_are_not_used() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let long_digest = "x".repeat(dalica_library::MAX_DIGEST + 1);
    let long_op = "y".repeat(dalica_library::MAX_OPERATION + 1);
    record(
        d,
        &format!(
            r#"{{"originals": {{"a.jpg": {{"size": 1, "mtime": 0, "state": "done", "digest": "{long_digest}"}},
                "b.jpg": {{"size": 1, "mtime": 0, "state": "done", "recipe": ["{long_op}"]}},
                "c.jpg": {{"size": 1, "mtime": 0, "state": "done"}}}}}}"#
        ),
    );
    let found = originals::read(d, NOW);
    let Originals::Read { saved, notes } = &found else { panic!("{found:?}") };
    assert_eq!(saved.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["c.jpg"]);
    assert_eq!(notes.len(), 2, "{notes:?}");
    let (mut lib, f, ids) = library(d);
    lib.apply(originals::ops(&lib, f, &found).unwrap()).unwrap();
    assert_eq!(lib.saved_all().map(|(id, _)| id).collect::<Vec<_>>(), [ids[2]]);
}

/// A scan-only rebuild (`data-model.md` §10.1): with the index gone, a scan of the folders brings
/// back every saved record, copy saved beside and project mark, and a second scan changes nothing.
#[test]
fn a_scan_alone_brings_back_saved_records_copies_and_the_project_mark() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("export");
    let photo = jpeg(&Scene::new(1, 16, 16), 80, None).unwrap();
    for name in ["a.jpg", "b.jpg", "c.jpg", "c-001.jpg"] {
        put(&d.join(name), &photo);
    }
    put(&d.join(".dalica-originals/a.jpg"), &photo);
    record(&d, RECORD);
    put(
        &d.join(".dalica.json"),
        br#"{"folder": {"project": "exported"}, "photos": {
            "c.jpg": {"edits": ["sepia()"], "savedAs": {"name": "c-001.jpg", "recipe": ["sepia()"]}},
            "b.jpg": {"savedAs": {"name": "../b.jpg"}}
        }}"#,
    );
    let s = scan(&[dir.path().to_path_buf()], None);
    let reported: Vec<&str> = s.warnings.iter().map(|w| w.reason.as_str()).collect();
    assert!(reported.iter().any(|r| r.contains("/originals/CON")), "{reported:?}");
    let mut lib = Library::new();
    let ops = dalica_scan::diff(&mut lib, &s);
    assert!(ops.iter().any(|op| matches!(op, Op::SetSaved { .. }) && op.is_derived()), "{ops:?}");
    for op in ops {
        lib.apply(op).unwrap();
    }
    let f = lib.folder_by_path(&d).unwrap();
    let id = |name: &str| lib.photos_in(f).into_iter().find(|p| p.name == name).unwrap().id;
    let (a, b, c) = (id("a.jpg"), id("b.jpg"), id("c.jpg"));
    assert_eq!(lib.saved(a), Some(&saved_a()));
    assert_eq!(lib.saved(b).map(|s| s.state), Some(SaveState::Pending));
    assert_eq!(lib.saved(c), None);
    assert_eq!(lib.saved_as(c), Some(&SavedAs { name: "c-001.jpg".into(), recipe: vec!["sepia()".into()] }));
    assert_eq!(lib.saved_as(b), None, "a copy named ../b.jpg is never used");
    assert_eq!(lib.project(f), Some(Project::Exported));
    assert!(lib.can_undo_save(a) && !lib.has_unsaved_edits(c));
    assert_eq!(lib.unsaved_count(f), 0);
    // A second scan changes nothing.
    assert!(dalica_scan::diff(&mut lib, &scan(&[dir.path().to_path_buf()], None)).is_empty());

    // An Undo Save made elsewhere drops `a.jpg`'s entry: the next scan clears its record only.
    record(&d, r#"{"originals": {"b.jpg": {"size": 10, "mtime": 5, "state": "pending"}}}"#);
    let ops = dalica_scan::diff(&mut lib, &scan(&[dir.path().to_path_buf()], None));
    assert_eq!(ops, vec![Op::SetSaved { saved: vec![(a, None)] }]);
    for op in ops {
        lib.apply(op).unwrap();
    }
    // A damaged record is set aside and reported by the scan, and the model keeps its records.
    record(&d, "{\"originals\"");
    let s = scan(&[dir.path().to_path_buf()], None);
    assert!(s.warnings.iter().any(|w| w.reason.contains("kept as") && w.reason.contains(".dalica.json.corrupt-")), "{:?}", s.warnings);
    assert!(dalica_scan::diff(&mut lib, &s).is_empty());
    assert_eq!(lib.saved(b).map(|s| s.state), Some(SaveState::Pending));
    assert!(!originals::record_path(&d).exists());
}
