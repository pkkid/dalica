//! Moving folders (`roadmap.md` M9.15; `organizing.md` §9, `data-model.md` §8), on temporary
//! folders: a moved folder keeps its photos' ids and data, its sidecar, its non-picture files and
//! its subfolders (their library folders follow); a move onto a library folder of the same name
//! merges photo by photo, keeping both folders' photos and sidecar entries, and the emptied source
//! goes to the trash only when nothing else is left in it; a forced cross-volume move verifies the
//! copy before the source goes to the trash; undo and redo; the owner thread makes no `fs` call;
//! the command's contract and graceful failures.
//!
//! These tests use the real OS trash and put back what they sent there (where the system allows),
//! and arm process-wide fault points, so they run one at a time ([`serial`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use dalica_core::{FolderId, PhotoId, SystemClock};
use dalica_engine::cmd::delete::RESTORES;
use dalica_engine::cmd::folder_ops::{ALREADY_THERE, INTO_ITSELF, NOT_IN_LIBRARY};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, CmdError, Direction, Effect, FileOpKind, History, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_fs::faults::{self, Fault, points};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// One test at a time: fault points armed for the job's thread are process-wide.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    faults::disarm_all();
    guard
}

/// Runs `f` on a helper thread: the test's own file access never counts against the owner.
fn off_owner<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().unwrap())
}

fn exists(path: &Path) -> bool {
    let path = path.to_path_buf();
    off_owner(move || path.exists())
}

fn read(path: &Path) -> Vec<u8> {
    let path = path.to_path_buf();
    off_owner(move || std::fs::read(path).unwrap())
}

/// The names in `folder`, sorted (Dalica's own files included).
fn names(folder: &Path) -> Vec<String> {
    let folder = folder.to_path_buf();
    let mut v: Vec<String> =
        off_owner(move || std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect());
    v.sort();
    v
}

/// The names in `folder` with their sizes (for failure messages).
fn sizes(folder: &Path) -> Vec<(String, u64)> {
    let folder = folder.to_path_buf();
    let mut v: Vec<(String, u64)> = off_owner(move || {
        std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.metadata().unwrap().len()))
            .collect()
    });
    v.sort();
    v
}

fn entry(folder: &Path, name: &str) -> Option<Value> {
    let path = folder.join(".dalica.json");
    let v: Value = off_owner(move || serde_json::from_slice(&std::fs::read(path).ok()?).ok())?;
    v.get("photos").and_then(|p| p.get(name)).cloned()
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap()
}

fn options() -> Options {
    Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } }
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
}

impl Lib {
    /// A library watching `Pictures`, made of `files`: (path under `Pictures`, bytes), scanned.
    fn new(files: &[(&str, Vec<u8>)]) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let pictures = dir.path().join("Pictures");
        for (path, bytes) in files {
            let path = pictures.join(path);
            dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
            dalica_fs::replace(&path, bytes, Expect::Absent).unwrap();
        }
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options()).unwrap();
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": pictures })).unwrap();
        s.wait_idle();
        Lib { dir, s }
    }

    /// `Trips/Lisbon` (two photos, a note, `Day2` with a photo) and `Archive` (one photo).
    fn trips() -> Lib {
        Lib::new(&[
            ("Trips/Lisbon/IMG_0001.jpg", jpeg(1)),
            ("Trips/Lisbon/IMG_0002.jpg", jpeg(2)),
            ("Trips/Lisbon/notes.txt", b"not a picture".to_vec()),
            ("Trips/Lisbon/Day2/DAY_0001.jpg", jpeg(3)),
            ("Archive/OLD_0001.jpg", jpeg(4)),
        ])
    }

    fn path(&self, rel: &str) -> PathBuf {
        rel.split('/').filter(|c| !c.is_empty()).fold(self.dir.path().join("Pictures"), |p, c| p.join(c))
    }

    fn folder(&self, rel: &str) -> FolderId {
        self.s.library().folder_by_path(&self.path(rel)).unwrap()
    }

    fn photo(&self, rel: &str, name: &str) -> PhotoId {
        let f = self.folder(rel);
        self.s.library().photo_by_name(f, name).unwrap().id
    }

    fn folder_of(&self, id: PhotoId) -> PathBuf {
        let p = self.s.library().photo(id).unwrap();
        self.s.library().folder(p.folder).unwrap().path.clone()
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

    fn move_folder(&mut self, p: Value) {
        self.s.execute("folder.move", &p).unwrap();
        self.wait();
        let e = self.last_event();
        assert!(e.failures.is_empty(), "{:?}", e.failures);
    }
}

/// A moved folder keeps its photos' ids and data, its sidecar, its note and its subfolder; undo
/// puts it back and redo moves it again; the owner thread makes no `fs` call.
#[test]
fn a_moved_folder_keeps_ids_sidecar_and_subfolders() {
    let _serial = serial();
    let mut l = Lib::trips();
    let (one, day) = (l.photo("Trips/Lisbon", "IMG_0001.jpg"), l.photo("Trips/Lisbon/Day2", "DAY_0001.jpg"));
    let (lisbon, day2) = (l.folder("Trips/Lisbon"), l.folder("Trips/Lisbon/Day2"));
    l.s.execute("photo.star.set", &json!({ "ids": [one, day], "on": true })).unwrap();
    // A watched-folder entry of its own under it (scanned once): it follows the folder.
    l.s.execute("folders.set", &json!({ "changes": [{ "path": l.path("Trips/Lisbon/Day2"), "mode": "once" }] })).unwrap();
    l.s.wait_idle();
    l.wait();
    let before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let archive = l.folder("Archive");
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();

    let reply = l.s.execute("folder.move", &json!({ "folder": lisbon, "to": archive })).unwrap();
    assert_eq!((reply["merge"].clone(), reply["total"].clone()), (json!(false), json!(1)));
    l.wait();
    let moved = l.path("Archive/Lisbon");
    assert!(!exists(&l.path("Trips/Lisbon")));
    assert_eq!(names(&moved), [".dalica.json", "Day2", "IMG_0001.jpg", "IMG_0002.jpg", "notes.txt"]);
    assert_eq!(read(&moved.join("IMG_0001.jpg")), jpeg(1));
    let lib = l.s.library();
    assert_eq!((lib.folder_by_path(&moved), lib.folder_by_path(&moved.join("Day2"))), (Some(lisbon), Some(day2)), "the folders keep their ids");
    assert_eq!((l.folder_of(one), l.folder_of(day)), (moved.clone(), moved.join("Day2")), "the photos keep theirs");
    assert!(lib.photo(one).unwrap().star && lib.photo(day).unwrap().star, "and their data");
    assert_eq!(entry(&moved, "IMG_0001.jpg").unwrap()["star"], true, "the sidecar went with it");
    assert_eq!(entry(&moved.join("Day2"), "DAY_0001.jpg").unwrap()["star"], true);
    let rooted = |l: &Lib, p: &Path| l.s.roots().iter().any(|r| r == p);
    assert!(rooted(&l, &moved.join("Day2")) && !rooted(&l, &l.path("Trips/Lisbon/Day2")), "its watched-folder entry followed");
    assert_eq!((l.s.undo_stack().len(), l.s.undo_stack().undo_label()), (steps + 1, Some("Move Folder")));
    let e = l.last_event();
    assert_eq!((e.kind, e.direction, e.done, e.destination.clone()), (FileOpKind::MoveFolder, Direction::Do, 1, Some(l.path("Archive"))));
    let done = l.s.library().model().clone();

    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    assert!(!exists(&moved));
    assert_eq!(names(&l.path("Trips/Lisbon")), [".dalica.json", "Day2", "IMG_0001.jpg", "IMG_0002.jpg", "notes.txt"]);
    assert_eq!(l.s.library().model(), &before);
    assert!(rooted(&l, &l.path("Trips/Lisbon/Day2")) && !rooted(&l, &moved.join("Day2")), "and came back");
    l.s.execute("edit.redo", &json!({})).unwrap();
    l.wait();
    assert!(exists(&moved.join("Day2").join("DAY_0001.jpg")));
    assert!(rooted(&l, &moved.join("Day2")));
    // And `folders.json` says so: the entry under its new path, with its rule.
    l.s.flush().unwrap();
    let file = off_owner({
        let path = l.dir.path().join("library").join("folders.json");
        move || serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap()
    });
    let entries = file["folders"].as_array().unwrap();
    let day2_entry = entries.iter().find(|e| e["path"] == json!(moved.join("Day2"))).unwrap_or_else(|| panic!("{file}"));
    assert_eq!(day2_entry["mode"], "once");
    assert!(!entries.iter().any(|e| e["path"] == json!(l.path("Trips/Lisbon/Day2"))), "{file}");
    assert_eq!(l.s.library().model(), &done);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);

    // Into a watched folder by its path ("path", as the folder picker answers): back up a level.
    let trips = l.path("Trips");
    l.move_folder(json!({ "folder": lisbon, "path": trips }));
    assert_eq!(l.folder_of(day), trips.join("Lisbon").join("Day2"));
}

/// Onto a library folder of the same name the photos merge one by one: a name both use is asked
/// about, then renamed `-001` (or skipped); both folders' photos and entries are kept; the emptied
/// source goes to the trash, and comes back on undo; redo merges to the same names.
#[test]
fn a_merge_keeps_both_folders_photos_and_entries() {
    let _serial = serial();
    let mut l = Lib::new(&[
        ("Old/Lisbon/IMG_0001.jpg", jpeg(1)),
        ("Old/Lisbon/IMG_0002.jpg", jpeg(2)),
        ("Lisbon/IMG_0002.jpg", jpeg(20)),
        ("Lisbon/IMG_0003.jpg", jpeg(30)),
    ]);
    let (source, dest) = (l.path("Old/Lisbon"), l.path("Lisbon"));
    let (one, two) = (l.photo("Old/Lisbon", "IMG_0001.jpg"), l.photo("Old/Lisbon", "IMG_0002.jpg"));
    let theirs = l.photo("Lisbon", "IMG_0002.jpg");
    l.s.execute("photo.star.set", &json!({ "ids": [one], "on": true })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [two], "caption": "Moved" })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [theirs], "caption": "Stayed" })).unwrap();
    l.wait();
    // A key only another program knows: it reaches the destination only through the carry, which
    // must come before the emptied folder goes to the trash.
    off_owner({
        let path = source.join(".dalica.json");
        move || {
            let mut root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            root["photos"]["IMG_0001.jpg"]["x-other"] = json!({ "rating": 5 });
            std::fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
        }
    });
    let before = l.s.library().model().clone();
    // The files as their captions were written back into them.
    let (moved_bytes, their_bytes) = (read(&source.join("IMG_0002.jpg")), read(&dest.join("IMG_0002.jpg")));
    let folder = l.folder("Old/Lisbon");
    let to = l.path("");

    // The clash is asked about first; nothing moves.
    match l.s.execute("folder.move", &json!({ "folder": folder, "to": to })) {
        Err(CmdError::Disabled(m)) => assert!(m.contains("same names (IMG_0002.jpg)") && m.contains("collisions"), "{m}"),
        other => panic!("asked first: {other:?}"),
    }
    assert!(!l.s.file_jobs_running());

    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    let reply = l.s.execute("folder.move", &json!({ "folder": folder, "to": to, "collisions": "rename" })).unwrap();
    assert_eq!((reply["merge"].clone(), reply["total"].clone()), (json!(true), json!(5)), "two photos, settling (two steps), the emptied folder");
    l.wait();
    let e = l.last_event();
    assert!(e.failures.is_empty() && e.skipped.is_empty(), "{e:?}");
    assert_eq!(names(&dest), [".dalica.json", "IMG_0001.jpg", "IMG_0002-001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!((read(&dest.join("IMG_0002-001.jpg")), read(&dest.join("IMG_0002.jpg"))), (moved_bytes, their_bytes));
    assert!(!exists(&source), "the emptied folder went to the trash");
    assert!(l.s.library().folder(folder).is_none());
    let lib = l.s.library();
    assert_eq!((lib.photo(one).unwrap().name.as_str(), lib.photo(two).unwrap().name.as_str()), ("IMG_0001.jpg", "IMG_0002-001.jpg"));
    assert_eq!((l.folder_of(one), l.folder_of(two), l.folder_of(theirs)), (dest.clone(), dest.clone(), dest.clone()));
    assert!(lib.photo(one).unwrap().star);
    assert_eq!((lib.caption(two), lib.caption(theirs)), ("Moved", "Stayed"));
    l.s.flush().unwrap();
    let first = entry(&dest, "IMG_0001.jpg").unwrap();
    assert_eq!((first["star"].clone(), first["x-other"].clone()), (json!(true), json!({ "rating": 5 })), "the entries travelled");
    assert_eq!(entry(&dest, "IMG_0002-001.jpg").unwrap()["caption"], "Moved");
    assert_eq!(entry(&dest, "IMG_0002.jpg").unwrap()["caption"], "Stayed", "and the destination's stayed");
    let merged = l.s.library().model().clone();

    // Undo works everywhere: the emptied folder comes back from the trash where the system can
    // give it back, else it is made again (macOS) and the library writes its sidecar.
    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    let e = l.last_event();
    assert!(e.failures.is_empty(), "{:?}", e.failures);
    assert_eq!(names(&source), [".dalica.json", "IMG_0001.jpg", "IMG_0002.jpg"]);
    assert_eq!(names(&dest), [".dalica.json", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(l.s.library().model(), &before);
    l.s.flush().unwrap();
    assert_eq!(entry(&source, "IMG_0002.jpg").unwrap()["caption"], "Moved");
    assert_eq!(entry(&source, "IMG_0001.jpg").unwrap()["x-other"], json!({ "rating": 5 }), "and came back");
    assert_eq!(entry(&dest, "IMG_0002-001.jpg"), None);
    l.s.execute("edit.redo", &json!({})).unwrap();
    l.wait();
    assert_eq!(names(&dest), [".dalica.json", "IMG_0001.jpg", "IMG_0002-001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(l.s.library().model(), &merged, "on disk: {:?}", sizes(&dest));
    // Back again, so the trash is left as it was (where the system can give items back).
    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    assert_eq!(l.s.library().model(), &before);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
}

/// Skip Duplicates leaves the clashing photo in the source, which then stays (as it does with a
/// non-picture file in it); undo moves the others back.
#[test]
fn a_merge_that_skips_leaves_the_source_with_what_is_left() {
    let _serial = serial();
    let mut l = Lib::new(&[
        ("Old/Lisbon/IMG_0001.jpg", jpeg(1)),
        ("Old/Lisbon/IMG_0002.jpg", jpeg(2)),
        ("Lisbon/IMG_0002.jpg", jpeg(20)),
        ("Other/Porto/PORTO_0001.jpg", jpeg(5)),
        ("Other/Porto/notes.txt", b"kept".to_vec()),
        ("Porto/PORTO_0002.jpg", jpeg(6)),
    ]);
    let ids: Vec<PhotoId> = l.s.library().photos().map(|p| p.id).collect();
    l.s.execute("photo.star.set", &json!({ "ids": ids, "on": true })).unwrap();
    l.wait();
    let before = l.s.library().model().clone();
    let (folder, to) = (l.folder("Old/Lisbon"), l.path(""));
    l.s.execute("folder.move", &json!({ "folder": folder, "to": to, "collisions": "skip" })).unwrap();
    l.wait();
    let e = l.last_event();
    assert_eq!((e.done, e.skipped.len()), (3, 2), "one photo and settling; the clash and the folder that still holds it: {e:?}");
    assert!(e.skipped[1].reason.contains("stays"), "{e:?}");
    assert_eq!(names(&l.path("Old/Lisbon")), [".dalica.json", "IMG_0002.jpg"]);
    assert_eq!(names(&l.path("Lisbon")), [".dalica.json", "IMG_0001.jpg", "IMG_0002.jpg"]);
    assert!(l.s.library().folder(folder).is_some());
    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    assert_eq!(l.s.library().model(), &before);

    // A non-picture file keeps the merged folder in place too.
    let porto = l.folder("Other/Porto");
    l.move_folder(json!({ "folder": porto, "to": to }));
    assert_eq!(names(&l.path("Other/Porto")), [".dalica.json", "notes.txt"]);
    assert_eq!(names(&l.path("Porto")), [".dalica.json", "PORTO_0001.jpg", "PORTO_0002.jpg"]);
}

/// A forced cross-volume move copies the tree and verifies every file before the source goes to
/// the trash: a copy that does not verify leaves the source untouched and nothing at the target.
#[test]
fn a_forced_cross_volume_folder_move_verifies_before_trashing() {
    let _serial = serial();
    let mut l = Lib::trips();
    let (lisbon, archive) = (l.folder("Trips/Lisbon"), l.folder("Archive"));
    let (source, target) = (l.path("Trips/Lisbon"), l.path("Archive/Lisbon"));
    let before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let tree = names(&source);
    let archived = names(&l.path("Archive"));

    // A byte of the copy flipped: the verification catches it.
    faults::arm_global(points::MOVE_CROSS_VOLUME, Fault::Error);
    faults::arm_global(points::COPY_FLIP, Fault::Error);
    l.s.execute("folder.move", &json!({ "folder": lisbon, "to": archive })).unwrap();
    l.wait();
    faults::disarm_all();
    assert_eq!(l.last_event().failures.len(), 1, "{:?}", l.last_event());
    assert_eq!(names(&source), tree, "the source is untouched");
    assert_eq!(read(&source.join("IMG_0002.jpg")), jpeg(2));
    assert_eq!(names(&l.path("Archive")), archived, "nothing at the target");
    assert_eq!(l.s.library().model(), &before);
    assert_eq!(l.s.undo_stack().len(), steps);

    // Verified, then the source to the trash.
    let identity = {
        let source = source.clone();
        off_owner(move || dalica_fs::identity(&source).unwrap().unwrap())
    };
    faults::arm_global(points::MOVE_CROSS_VOLUME, Fault::Error);
    l.move_folder(json!({ "folder": lisbon, "to": archive }));
    faults::disarm_all();
    assert!(!exists(&source), "the source went to the trash");
    assert_eq!(names(&target), tree);
    assert_eq!((read(&target.join("IMG_0001.jpg")), read(&target.join("Day2").join("DAY_0001.jpg"))), (jpeg(1), jpeg(3)));
    assert_eq!(l.s.library().folder_by_path(&target), Some(lisbon));

    // Undo moves the copy back.
    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    assert_eq!(names(&source), tree);
    assert_eq!(l.s.library().model(), &before);

    // Leave the trash as it was: the original tree comes back beside the one undo moved.
    if RESTORES {
        let (aside, src) = (l.path("Trips/Lisbon-undone"), source.clone());
        off_owner(move || std::fs::rename(&src, &aside).unwrap());
        let original = off_owner({
            let trips = l.path("Trips");
            move || std::fs::canonicalize(trips).unwrap().join("Lisbon")
        });
        let t = dalica_fs::trash::Trashed { original, identity, folder: true };
        let back = off_owner(move || dalica_fs::trash::restore(&[t]).unwrap());
        assert_eq!(back, [dalica_fs::trash::Restore::Restored]);
    }
}

/// The command's contract, and graceful failures: each refuses with the right error, says why
/// where it is disabled, and changes nothing.
#[test]
fn the_command_s_contract_and_graceful_failures() {
    let _serial = serial();
    let mut l = Lib::trips();
    let spec = l.s.spec("folder.move").unwrap();
    assert_eq!((spec.effect, spec.history, spec.menu), (Effect::Files, History::Library, "Folder/Move…"));
    let (lisbon, day2, archive) = (l.folder("Trips/Lisbon"), l.folder("Trips/Lisbon/Day2"), l.folder("Archive"));
    let before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let outside = l.dir.path().join("Elsewhere");
    let cases: Vec<(Value, &str)> = vec![
        (json!({}), "bad_params"),
        (json!({ "folder": lisbon }), "bad_params"),
        (json!({ "folder": lisbon, "to": archive, "collisions": "replace" }), "bad_params"),
        (json!({ "folder": lisbon, "to": archive, "zzz": 1 }), "bad_params"),
        (json!({ "folder": lisbon, "to": "relative/path" }), "bad_params"),
        (json!({ "folder": lisbon, "to": l.path("Archive").join("..").join("Trips") }), "bad_params"),
        (json!({ "folder": lisbon, "to": true }), "bad_params"),
        (json!({ "folder": 9999, "to": archive }), "not_found"),
        (json!({ "folder": lisbon, "to": 9999 }), "not_found"),
        (json!({ "folder": lisbon, "to": outside }), "disabled"),
        (json!({ "folder": lisbon, "to": day2 }), "disabled"),
        (json!({ "folder": lisbon, "to": lisbon }), "disabled"),
        (json!({ "folder": lisbon, "to": l.path("Trips") }), "disabled"),
    ];
    for (p, code) in cases {
        let r = l.s.execute("folder.move", &p);
        assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{p}: {r:?}");
    }
    assert_eq!(l.s.execute("folder.move", &json!({ "folder": lisbon, "to": outside })), Err(CmdError::Disabled(NOT_IN_LIBRARY.into())));
    assert_eq!(l.s.execute("folder.move", &json!({ "folder": lisbon, "to": day2 })), Err(CmdError::Disabled(INTO_ITSELF.into())));
    assert_eq!(l.s.execute("folder.move", &json!({ "folder": lisbon, "to": l.path("Trips") })), Err(CmdError::Disabled(ALREADY_THERE.into())));
    assert!(!l.s.file_jobs_running());
    assert_eq!(l.s.library().model(), &before);
    assert_eq!(l.s.undo_stack().len(), steps);

    // A folder of that name on disk that the library does not list: the item fails, nothing moves.
    let empty = l.path("Archive/Lisbon");
    off_owner({
        let empty = empty.clone();
        move || std::fs::create_dir(empty).unwrap()
    });
    l.s.execute("folder.move", &json!({ "folder": lisbon, "to": archive })).unwrap();
    l.wait();
    let e = l.last_event();
    assert_eq!((e.done, e.failures.len()), (0, 1));
    assert!(e.failures[0].reason.contains("already there"), "{}", e.failures[0].reason);
    assert!(exists(&l.path("Trips/Lisbon/IMG_0001.jpg")));
    assert_eq!(l.s.library().model(), &before);
    assert_eq!(l.s.undo_stack().len(), steps);
}
