//! Deleting to the trash (`roadmap.md` M9.15; `organizing.md` §9, `security.md` §5.7,
//! `data-model.md` §8), on temporary folders: a deleted photo is in the trash and gone from the
//! library and its albums; undo brings it back with its data where the system's trash can give it
//! back (Linux, Windows), and on macOS undo is disabled with its reason; a drive without a trash
//! fails with an explanation and the file stays; a folder delete takes its non-picture files and
//! subfolders too; both ask first unless "Delete from disk without asking" is on; the owner thread
//! makes no `fs` call; each command's contract and graceful failures.
//!
//! These tests use the real OS trash and put back what they sent there (where the system allows);
//! the one that arms a process-wide fault point holds [`crate::common::exclusive`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FaultLock, entry, exclusive, exists, jpeg, options, read, shared};
use dalica_core::{PhotoId, SystemClock};
use dalica_engine::cmd::delete::{RESTORES, UNDO_UNSUPPORTED, folder_question, no_trash, photos_question};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CmdError, Direction, FileOpKind, Session};
use dalica_fs::Expect;
use dalica_fs::faults::{self, Fault, points};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    _faults: FaultLock,
}

impl Lib {
    /// A library watching `Pictures`: `Lisbon` with `n` photos and a note, `Lisbon/Day2` with one
    /// photo, scanned.
    fn new(n: usize) -> Lib {
        let _faults = shared();
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon.join("Day2")).unwrap();
        for i in 0..n {
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64), Expect::Absent).unwrap();
        }
        dalica_fs::replace(&lisbon.join("notes.txt"), b"not a picture", Expect::Absent).unwrap();
        dalica_fs::replace(&lisbon.join("Day2").join("DAY_0001.jpg"), &jpeg(99), Expect::Absent).unwrap();
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options()).unwrap();
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": dir.path().join("Pictures") })).unwrap();
        s.wait_idle();
        Lib { dir, s, _faults }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Lisbon")
    }

    fn photo(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap().id
    }

    fn folder_id(&self, path: &Path) -> u64 {
        self.s.library().folder_by_path(path).unwrap().get()
    }

    /// Waits for the file jobs, then for the metadata write-backs and sidecar writes the commands
    /// caused: a caption written into a photo's file changes its size in the model, and a folder
    /// listed next holds no temporary file of a write still under way.
    fn wait(&mut self) {
        assert!(self.s.wait_file_jobs(PATIENCE), "the file jobs ended");
        assert!(self.s.flush_metadata(PATIENCE), "the metadata writes ended");
        self.s.flush().unwrap();
    }

    fn last_event(&self) -> dalica_engine::FileOpEvent {
        self.s.file_op_events().last().cloned().unwrap()
    }

    fn set_delete_without_asking(&mut self, on: bool) {
        let mut o = self.s.settings().organize();
        o.delete_without_asking = on;
        self.s.change_settings(|st| st.set_organize(&o));
    }
}

/// A deleted photo is in the trash and gone from the library and its album; undo puts it back with
/// its id and data (Linux, Windows), redo trashes it again; on macOS undo says why it cannot.
#[test]
fn a_deleted_photo_goes_to_the_trash_and_undo_brings_it_back_with_its_data() {
    let mut l = Lib::new(3);
    let (one, two) = (l.photo("IMG_0001.jpg"), l.photo("IMG_0002.jpg"));
    l.s.execute("photo.star.set", &json!({ "ids": [two], "on": true })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [two], "caption": "The tram" })).unwrap();
    l.s.execute("photo.tags.add", &json!({ "ids": [two], "tags": "lisbon" })).unwrap();
    let album = l.s.execute("album.create", &json!({ "name": "Trip", "ids": [one, two] })).unwrap()["album"].clone();
    l.wait();
    let path = l.lisbon().join("IMG_0002.jpg");
    let bytes = read(&path);
    let before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();

    // Asks first: nothing changes.
    let asked = l.s.execute("photo.delete", &json!({ "ids": [two] }));
    match asked {
        Err(CmdError::Disabled(m)) => assert!(m.contains(&photos_question(1)) && m.contains("confirm"), "{m}"),
        other => panic!("asked first: {other:?}"),
    }
    assert!(!l.s.file_jobs_running());
    assert_eq!(l.s.library().model(), &before);

    let reply = l.s.execute("photo.delete", &json!({ "ids": [two], "confirm": true })).unwrap();
    assert_eq!((reply["pending"].clone(), reply["total"].clone()), (json!(true), json!(1)));
    l.wait();
    assert!(!exists(&path), "the file left its folder");
    assert!(l.s.library().photo(two).is_none(), "the photo left the library");
    let album_id: dalica_core::AlbumId = serde_json::from_value(album).unwrap();
    assert_eq!(l.s.library().album_members(&album_id), [one], "and its album");
    assert_eq!(l.s.undo_stack().len(), steps + 1, "one step");
    assert_eq!(l.s.undo_stack().undo_label(), Some("Delete"));
    let e = l.last_event();
    assert_eq!((e.kind, e.direction, e.done, e.total), (FileOpKind::Trash, Direction::Do, 1, 1));
    assert!(e.failures.is_empty(), "{:?}", e.failures);
    let deleted = l.s.library().model().clone();

    if RESTORES {
        assert_eq!(l.s.execute("edit.undo", &json!({})).unwrap(), json!({ "undone": "Delete" }));
        l.wait();
        let e = l.last_event();
        assert!(e.failures.is_empty(), "restored: {:?}", e.failures);
        assert_eq!(read(&path), bytes, "the file is back where it was");
        assert_eq!(l.s.library().model(), &before, "with its id, data and album place");
        assert!(l.s.library().photo(two).unwrap().star);
        l.s.flush().unwrap();
        let e = entry(&l.lisbon(), "IMG_0002.jpg").unwrap();
        assert_eq!((e["star"].clone(), e["caption"].clone()), (json!(true), json!("The tram")), "{e}");
        // Redo trashes it again; undo puts it back (and the trash is left as it was).
        l.s.execute("edit.redo", &json!({})).unwrap();
        l.wait();
        assert!(!exists(&path));
        assert_eq!(l.s.library().model(), &deleted);
        l.s.execute("edit.undo", &json!({})).unwrap();
        l.wait();
        assert_eq!(read(&path), bytes);
        assert_eq!(l.s.library().model(), &before);
    } else {
        assert_eq!(l.s.undo_stack().undo_blocked().as_deref(), Some(UNDO_UNSUPPORTED));
        assert_eq!(l.s.execute("edit.undo", &json!({})), Err(CmdError::Disabled(UNDO_UNSUPPORTED.into())));
        assert_eq!(l.s.library().model(), &deleted);
    }
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
}

/// Several photos are one step; each goes in its own item.
#[test]
fn several_photos_are_one_step() {
    let mut l = Lib::new(3);
    let ids = [l.photo("IMG_0001.jpg"), l.photo("IMG_0003.jpg")];
    let before = l.s.library().model().clone();
    match l.s.execute("photo.delete", &json!({ "ids": ids })) {
        Err(CmdError::Disabled(m)) => assert!(m.contains(&photos_question(2)), "{m}"),
        other => panic!("asked first: {other:?}"),
    }
    l.s.execute("photo.delete", &json!({ "ids": [ids[0], ids[1], ids[0]], "confirm": true })).unwrap();
    l.wait();
    assert_eq!(l.last_event().done, 2, "a repeated id counts once");
    assert!(!exists(&l.lisbon().join("IMG_0001.jpg")) && !exists(&l.lisbon().join("IMG_0003.jpg")));
    assert!(exists(&l.lisbon().join("IMG_0002.jpg")));
    if RESTORES {
        l.s.execute("edit.undo", &json!({})).unwrap();
        l.wait();
        assert_eq!(l.s.library().model(), &before);
        assert!(exists(&l.lisbon().join("IMG_0001.jpg")) && exists(&l.lisbon().join("IMG_0003.jpg")));
    }
}

/// A drive without a trash: the item fails with an explanation pointing to the file manager, the
/// file stays, the library keeps the photo and there is no step.
#[test]
fn a_drive_without_a_trash_fails_and_the_file_stays() {
    let _faults = exclusive();
    let mut l = Lib::new(2);
    let one = l.photo("IMG_0001.jpg");
    let path = l.lisbon().join("IMG_0001.jpg");
    let before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    faults::arm_global(points::TRASH_NO_TRASH, Fault::Error);
    l.s.execute("photo.delete", &json!({ "ids": [one], "confirm": true })).unwrap();
    l.wait();
    faults::disarm_all();
    let e = l.last_event();
    assert_eq!((e.done, e.failures.len()), (0, 1));
    assert_eq!(e.failures[0].reason, no_trash(&path));
    assert!(e.failures[0].reason.contains("file manager"));
    assert!(exists(&path), "the file stays");
    assert_eq!(l.s.library().model(), &before);
    assert_eq!(l.s.undo_stack().len(), steps);

    // A folder too.
    let lisbon = l.folder_id(&l.lisbon());
    faults::arm_global(points::TRASH_NO_TRASH, Fault::Error);
    l.s.execute("folder.delete", &json!({ "folder": lisbon, "confirm": true })).unwrap();
    l.wait();
    faults::disarm_all();
    assert_eq!(l.last_event().failures[0].reason, no_trash(&l.lisbon()));
    assert!(exists(&l.lisbon().join("notes.txt")));
    assert_eq!(l.s.library().model(), &before);
}

/// A folder delete takes everything in it: its photos, its non-picture files and its subfolders
/// (and their library folders); undo brings it all back. With "Delete from disk without asking" on,
/// it does not ask.
#[test]
fn a_folder_delete_takes_its_non_picture_files_and_subfolders_too() {
    let mut l = Lib::new(2);
    let (lisbon, day2) = (l.lisbon(), l.lisbon().join("Day2"));
    let (lisbon_id, day2_id) = (l.folder_id(&lisbon), l.folder_id(&day2));
    l.s.execute("photo.star.set", &json!({ "ids": [l.photo("DAY_0001.jpg")], "on": true })).unwrap();
    l.wait();
    let before = l.s.library().model().clone();
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    match l.s.execute("folder.delete", &json!({ "folder": lisbon_id })) {
        Err(CmdError::Disabled(m)) => assert!(m.contains(&folder_question("Lisbon")), "{m}"),
        other => panic!("asked first: {other:?}"),
    }
    l.set_delete_without_asking(true);
    l.s.execute("folder.delete", &json!({ "folder": lisbon_id })).unwrap();
    l.wait();
    assert!(!exists(&lisbon), "the folder left with everything in it");
    let lib = l.s.library();
    assert!(lib.folder(dalica_core::FolderId::new(lisbon_id)).is_none() && lib.folder(dalica_core::FolderId::new(day2_id)).is_none());
    assert_eq!(lib.photos().count(), 0);
    assert_eq!(l.s.undo_stack().undo_label(), Some("Delete Folder"));
    assert_eq!(l.last_event().kind, FileOpKind::TrashFolder);
    if RESTORES {
        l.s.execute("edit.undo", &json!({})).unwrap();
        l.wait();
        let e = l.last_event();
        assert!(e.failures.is_empty(), "{:?}", e.failures);
        assert_eq!(read(&lisbon.join("notes.txt")), b"not a picture");
        assert!(exists(&day2.join("DAY_0001.jpg")));
        assert_eq!(l.s.library().model(), &before, "both folders and their photos, ids and data");
    } else {
        assert_eq!(l.s.undo_stack().undo_blocked().as_deref(), Some(UNDO_UNSUPPORTED));
    }
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
}

/// Graceful failures: each command refuses with its own error and changes nothing.
#[test]
fn each_command_s_graceful_failures() {
    let mut l = Lib::new(1);
    let before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let cases: Vec<(&str, Value, &str)> = vec![
        ("photo.delete", json!({ "ids": [] }), "bad_params"),
        ("photo.delete", json!({ "ids": [9999], "confirm": true }), "not_found"),
        ("photo.delete", json!({ "ids": vec![1; 100_001], "confirm": true }), "limit"),
        ("folder.delete", json!({ "folder": 9999, "confirm": true }), "not_found"),
    ];
    for (id, p, code) in cases {
        let r = l.s.execute(id, &p);
        assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{id} {p}: {r:?}");
    }
    assert!(!l.s.file_jobs_running());
    assert_eq!(l.s.library().model(), &before);
    assert_eq!(l.s.undo_stack().len(), steps);
    assert!(exists(&l.lisbon().join("IMG_0001.jpg")));
}
