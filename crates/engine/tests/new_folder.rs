//! `photo.moveToNewFolder` on temp folders (`roadmap.md` M9.13; `organizing.md` §9,
//! `data-model.md` §8): Move to New Folder and a split (one photo and every photo after it) make a
//! folder beside the photos' folder with its Folder Properties and move the photos into it, one
//! undo step each; undo moves them back and sends the folder, empty again, to the trash; a taken
//! name takes the next free one; a folder holding something else stays; the owner thread makes no
//! `fs` call; the command's graceful failures.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::{exists, off_owner, shared, wait_files};
use dalica_core::{Date, FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, CmdError, FileOpEvent, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use serde_json::{Value, json};

fn sidecar(folder: &Path) -> Option<Value> {
    let path = folder.join(".dalica.json");
    off_owner(move || std::fs::read(path).ok()).map(|b| serde_json::from_slice(&b).unwrap())
}

fn clean(e: &FileOpEvent) {
    assert!(e.failures.is_empty() && e.skipped.is_empty() && !e.cancelled, "{e:?}");
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
}

impl Lib {
    /// Pictures/Lisbon with IMG_0001–4 (and `extra` folders with one photo each), the watched root
    /// `root` (relative to the temp folder), scanned, watching off.
    fn new(root: &str, extra: &[&str]) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 1..=4u64 {
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 96, 64), 80, None).unwrap();
            dalica_fs::replace(&lisbon.join(format!("IMG_000{i}.jpg")), &bytes, Expect::Absent).unwrap();
        }
        for name in extra {
            let folder = dir.path().join("Pictures").join(name);
            dalica_fs::create_dir_all(&folder).unwrap();
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40, 96, 64), 80, None).unwrap();
            dalica_fs::replace(&folder.join("OTHER.jpg"), &bytes, Expect::Absent).unwrap();
        }
        let library = dir.path().join("library");
        dalica_fs::create_dir_all(&library).unwrap();
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&library, clock, &options).unwrap();
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": dir.path().join(root) })).unwrap();
        s.wait_idle();
        Lib { dir, s }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.dir.path().join("Pictures").join(name)
    }

    fn folder_id(&self, name: &str) -> Option<FolderId> {
        self.s.library().folder_by_path(&self.folder(name))
    }

    fn photos(&self) -> Vec<PhotoId> {
        let id = self.folder_id("Lisbon").unwrap();
        let mut p: Vec<(String, PhotoId)> = self.s.library().photos_in(id).into_iter().map(|p| (p.name.clone(), p.id)).collect();
        p.sort();
        p.into_iter().map(|(_, id)| id).collect()
    }

    fn folder_of(&self, id: PhotoId) -> PathBuf {
        let p = self.s.library().photo(id).unwrap();
        self.s.library().folder(p.folder).unwrap().path.clone()
    }

    fn wait(&mut self) -> FileOpEvent {
        wait_files(&mut self.s);
        self.s.flush().unwrap();
        self.s.file_op_events().last().cloned().unwrap()
    }

    fn run(&mut self, p: Value) -> (Value, FileOpEvent) {
        let reply = self.s.execute("photo.moveToNewFolder", &p).unwrap();
        (reply, self.wait())
    }

    fn undo(&mut self) -> FileOpEvent {
        self.s.execute("edit.undo", &json!({})).unwrap();
        self.wait()
    }

    fn redo(&mut self) -> FileOpEvent {
        self.s.execute("edit.redo", &json!({})).unwrap();
        self.wait()
    }
}

/// Move to New Folder: the folder beside the source with its info and the photos (their data
/// kept), one step; undo moves them back and removes the folder; redo makes it again. The owner
/// thread made no `fs` call.
#[test]
fn move_to_new_folder_is_one_step_and_undo_removes_the_folder() {
    let _faults = shared();
    let mut l = Lib::new("Pictures", &[]);
    let ids = l.photos();
    l.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    let steps = l.s.undo_stack().len();
    let trams = l.folder("Trams");

    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    let (reply, e) = l.run(json!({ "ids": [ids[0], ids[1]], "name": "Trams", "date": "2026-06-04", "useOldest": false,
        "place": "Lisbon", "description": "Rides on the 28" }));
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
    clean(&e);
    assert_eq!((reply["total"].clone(), reply["folder"].clone()), (json!(2), json!(trams)));
    assert_eq!(e.done, 3, "the folder and two photos");
    assert_eq!(e.destination, Some(trams.clone()));
    assert_eq!(l.s.undo_stack().len(), steps + 1, "one undo step");
    let id = l.folder_id("Trams").unwrap();
    assert_eq!(reply["folderId"], json!(id));
    let info = l.s.library().folder_info(id);
    assert_eq!(
        (info.date, info.place.as_str(), info.description.as_str()),
        (Some("2026-06-04".parse::<Date>().unwrap()), "Lisbon", "Rides on the 28")
    );
    assert_eq!(l.folder_of(ids[0]), trams);
    assert_eq!(l.folder_of(ids[2]), l.folder("Lisbon"));
    assert!(exists(&trams.join("IMG_0001.jpg")) && !exists(&l.folder("Lisbon").join("IMG_0001.jpg")));
    let written = sidecar(&trams).unwrap();
    assert_eq!(written["folder"]["place"], "Lisbon");
    assert_eq!(written["photos"]["IMG_0001.jpg"]["caption"], "Tram 28");

    let e = l.undo();
    clean(&e);
    assert_eq!(e.done, 3);
    assert!(!exists(&trams), "the empty folder went to the trash");
    assert!(l.folder_id("Trams").is_none());
    assert_eq!(l.folder_of(ids[0]), l.folder("Lisbon"));
    assert!(exists(&l.folder("Lisbon").join("IMG_0001.jpg")));
    assert_eq!(sidecar(&l.folder("Lisbon")).unwrap()["photos"]["IMG_0001.jpg"]["caption"], "Tram 28");
    assert_eq!(l.s.library().info(ids[0]).caption, "Tram 28");

    clean(&l.redo());
    assert_eq!(l.folder_id("Trams"), Some(id));
    assert_eq!(l.folder_of(ids[1]), trams);
    assert_eq!(l.s.library().folder_info(id).place, "Lisbon");
}

/// Split Folder Here: the photo and every photo after it, one step; undo removes the folder.
#[test]
fn a_split_moves_the_photo_and_every_photo_after_it_as_one_step() {
    let _faults = shared();
    let mut l = Lib::new("Pictures", &[]);
    let ids = l.photos();
    let after: Vec<PhotoId> = ids[1..].to_vec();
    let steps = l.s.undo_stack().len();
    let (_, e) = l.run(json!({ "ids": after, "name": "", "useOldest": true }));
    clean(&e);
    let split = l.folder("New Folder");
    assert_eq!(l.s.undo_stack().len(), steps + 1);
    assert_eq!(l.folder_of(ids[0]), l.folder("Lisbon"));
    assert!(after.iter().all(|id| l.folder_of(*id) == split));
    assert_eq!(l.s.library().folder_info(l.folder_id("New Folder").unwrap()).date, None, "the oldest photo's date");
    clean(&l.undo());
    assert!(!exists(&split));
    assert!(ids.iter().all(|id| l.folder_of(*id) == l.folder("Lisbon")));
}

/// A name the library holds beside takes the next free name; so does one only the disk shows; the
/// name is sanitized.
#[test]
fn a_taken_name_takes_the_next_free_one_and_names_are_sanitized() {
    let _faults = shared();
    let mut l = Lib::new("Pictures", &["Trams"]);
    let ids = l.photos();
    // Trams is a library folder: planned as Trams-001.
    let (reply, e) = l.run(json!({ "ids": [ids[0]], "name": "Trams" }));
    clean(&e);
    assert_eq!(reply["folder"], json!(l.folder("Trams-001")));
    assert_eq!(l.folder_of(ids[0]), l.folder("Trams-001"));
    // A folder only the disk shows: found when the folder is made.
    let hidden = l.folder("Rides");
    off_owner(|| std::fs::create_dir(&hidden).unwrap());
    let (_, e) = l.run(json!({ "ids": [ids[1]], "name": "../Rides?" }));
    clean(&e);
    assert_eq!(l.folder_of(ids[1]), l.folder("Rides-001"));
    assert!(exists(&l.folder("Rides-001").join("IMG_0002.jpg")));
    assert!(l.folder_id("Rides").is_none(), "the other folder is left alone");
}

/// Undo keeps a folder that holds a file of its own (and the step says why), after moving the
/// photos back.
#[test]
fn undo_keeps_a_folder_that_holds_something_else() {
    let _faults = shared();
    let mut l = Lib::new("Pictures", &[]);
    let ids = l.photos();
    let steps = l.s.undo_stack().len();
    clean(&l.run(json!({ "ids": [ids[0]], "name": "Trams" })).1);
    let trams = l.folder("Trams");
    let note = trams.join("notes.txt");
    off_owner(|| std::fs::write(&note, b"mine").unwrap());
    let e = l.undo();
    assert_eq!((e.done, e.failures.len()), (1, 1), "{e:?}");
    assert!(e.failures[0].reason.contains("not empty"), "{:?}", e.failures[0]);
    assert_eq!(l.folder_of(ids[0]), l.folder("Lisbon"));
    assert!(exists(&note));
    assert!(l.folder_id("Trams").is_some());
    assert_eq!(l.s.undo_stack().len(), steps + 1, "the folder's part stays to undo");
}

/// Graceful failures: unknown photos, photos of two folders, a folder beside a watched root, bad
/// parameters. Nothing moves, no step.
#[test]
fn graceful_failures_change_nothing() {
    let _faults = shared();
    let mut l = Lib::new("Pictures", &["Porto"]);
    let ids = l.photos();
    let porto = l.s.library().photos_in(l.folder_id("Porto").unwrap())[0].id;
    let model = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let cases = [
        (json!({ "ids": [9999], "name": "x" }), "not_found"),
        (json!({ "ids": [], "name": "x" }), "disabled"),
        (json!({ "ids": [ids[0], porto], "name": "x" }), "disabled"),
        (json!({ "ids": [ids[0]] }), "bad_params"),
        (json!({ "ids": [ids[0]], "name": "x", "date": "June", "useOldest": false }), "bad_params"),
        (json!({ "ids": vec![ids[0]; 100_001], "name": "x" }), "limit"),
    ];
    for (p, code) in cases {
        let r = l.s.execute("photo.moveToNewFolder", &p);
        assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{p}: {r:?}");
    }
    assert!(!l.s.file_jobs_running() && l.s.file_op_events().next().is_none(), "nothing started");
    assert_eq!(l.s.library().model(), &model);
    assert_eq!(l.s.undo_stack().len(), steps);

    // A folder beside a watched root would not be watched.
    let mut l = Lib::new("Pictures/Lisbon", &[]);
    let ids = l.photos();
    let r = l.s.execute("photo.moveToNewFolder", &json!({ "ids": [ids[0]], "name": "Trams" }));
    assert!(matches!(&r, Err(CmdError::Disabled(m)) if m.contains("outside")), "{r:?}");
    assert!(!exists(&l.folder("Trams")));
}
