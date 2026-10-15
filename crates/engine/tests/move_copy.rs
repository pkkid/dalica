//! `photo.moveTo` and `photo.copyTo` on temp folders (`roadmap.md` M9.13; `organizing.md` §9,
//! `data-model.md` §8 and §10.3): moved photos keep their star, caption, tags, recipe, album order
//! and originals backup; undo and redo restore the files and the model; the first-time question;
//! Rename and Skip Duplicates; a clash only the disk shows is journalled before the file moves
//! (a move across volumes is `two_volumes.rs`); Cancel half-way; a read-only destination; copies
//! and their undo; the owner thread makes no `fs` call; each command's graceful failures.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::common::{FaultLock, entry, exists, jpeg, off_owner, options, read_opt, shared, virtual_clock};
use dalica_core::{FolderId, PhotoId};
use dalica_engine::fileops::{ItemState, Journal, JournalEntry, OpId};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CmdError, Direction, FileOpEvent, FileOpKind, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

fn write(path: &Path, bytes: &[u8]) {
    let (path, bytes) = (path.to_path_buf(), bytes.to_vec());
    off_owner(move || std::fs::write(path, bytes).unwrap());
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    _faults: FaultLock,
}

impl Lib {
    /// Pictures/Lisbon with IMG_0001–3 and Pictures/Porto with PORTO_0001, scanned, watching off.
    fn new() -> Lib {
        let _faults = shared();
        let dir = tempfile::tempdir().unwrap();
        for (folder, names) in [("Lisbon", &["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"][..]), ("Porto", &["PORTO_0001.jpg"][..])] {
            let path = dir.path().join("Pictures").join(folder);
            dalica_fs::create_dir_all(&path).unwrap();
            for (i, name) in names.iter().enumerate() {
                dalica_fs::replace(&path.join(name), &jpeg(i as u64 + 10 * folder.len() as u64), Expect::Absent).unwrap();
            }
        }
        let library = dir.path().join("library");
        dalica_fs::create_dir_all(&library).unwrap();
        let mut s = Session::open_with(&library, virtual_clock(), &options()).unwrap();
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": dir.path().join("Pictures") })).unwrap();
        s.wait_idle();
        Lib { dir, s, _faults }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.dir.path().join("Pictures").join(name)
    }

    fn folder_id(&self, name: &str) -> FolderId {
        self.s.library().folder_by_path(&self.folder(name)).unwrap()
    }

    fn photo(&self, folder: &str, name: &str) -> PhotoId {
        self.s.library().photo_by_name(self.folder_id(folder), name).unwrap().id
    }

    fn where_is(&self, id: PhotoId) -> (PathBuf, String) {
        let p = self.s.library().photo(id).unwrap();
        (self.s.library().folder(p.folder).unwrap().path.clone(), p.name.clone())
    }

    /// Runs `id` and waits for its file operation: the operation's event.
    fn run(&mut self, id: &str, p: Value) -> FileOpEvent {
        self.s.execute(id, &p).unwrap();
        self.wait()
    }

    fn wait(&mut self) -> FileOpEvent {
        assert!(self.s.wait_file_jobs(PATIENCE), "the file jobs ended");
        self.s.flush().unwrap();
        self.s.file_op_events().last().cloned().unwrap()
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

fn clean(e: &FileOpEvent) {
    assert!(e.failures.is_empty() && e.skipped.is_empty() && !e.cancelled, "{e:?}");
}

/// `data-model.md` §10.3: moved photos keep star, caption, tags, recipe, album order and their
/// `.dalica-originals/` backup, in the model and in the sidecars; undo and redo restore the files
/// and the model; the owner thread made no `fs` call.
#[test]
fn moved_photos_keep_their_data_album_order_and_backup_and_undo_and_redo_restore_them() {
    let mut l = Lib::new();
    let (lisbon, porto, porto_id) = (l.folder("Lisbon"), l.folder("Porto"), l.folder_id("Porto"));
    let (one, two, three) = (l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg"), l.photo("Lisbon", "IMG_0003.jpg"));
    let porto_photo = l.photo("Porto", "PORTO_0001.jpg");
    l.s.execute("photo.star.set", &json!({ "ids": [one], "on": true })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [one, two], "caption": "Tram 28" })).unwrap();
    l.s.execute("photo.tags.add", &json!({ "ids": [two], "tags": "lisbon, tram" })).unwrap();
    l.s.execute("photo.history.apply", &json!({ "id": two, "op": "sepia()" })).unwrap();
    let album = l.s.execute("album.create", &json!({ "name": "Trams", "ids": [three, porto_photo, one, two] })).unwrap()["album"]
        .as_str()
        .unwrap()
        .to_string();
    let album_id = dalica_core::AlbumId::parse(&album).unwrap();
    let order = l.s.library().album_members(&album_id).to_vec();
    assert_eq!(order, [three, porto_photo, one, two]);
    l.s.flush().unwrap();
    // A saved original with its record, and a key only another program knows.
    {
        let lisbon = lisbon.clone();
        off_owner(move || {
            let originals = dalica_fs::originals::ensure_dir(&lisbon).unwrap();
            std::fs::write(originals.join("IMG_0002.jpg"), b"the original").unwrap();
            let record = json!({ "dalica": { "version": 1 }, "originals": { "IMG_0002.jpg": {
                "recipe": ["sepia()"], "size": 12, "mtime": 1, "width": 4, "height": 3, "digest": "", "state": "done" } } });
            std::fs::write(originals.join(".dalica.json"), serde_json::to_vec(&record).unwrap()).unwrap();
            let path = lisbon.join(".dalica.json");
            let mut root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            root["photos"]["IMG_0001.jpg"]["x-other"] = json!({ "rating": 5 });
            std::fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
        });
    }
    let bytes = read_opt(&lisbon.join("IMG_0001.jpg")).unwrap();
    let model_before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();

    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    let reply = l.s.execute("photo.moveTo", &json!({ "ids": [one, two], "folder": porto_id, "confirm": true })).unwrap();
    assert_eq!((reply["pending"].clone(), reply["total"].clone()), (json!(true), json!(2)));
    assert!(l.s.wait_file_jobs(PATIENCE));
    l.s.flush().unwrap();
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
    let e = l.s.file_op_events().last().cloned().unwrap();
    clean(&e);
    assert_eq!((e.kind, e.direction, e.done, e.destination.clone()), (FileOpKind::Move, Direction::Do, 2, Some(porto.clone())));
    assert_eq!(l.s.undo_stack().len(), steps + 1, "one undo step");

    // The files, the entries and the backup.
    assert_eq!(read_opt(&porto.join("IMG_0001.jpg")).unwrap(), bytes);
    assert!(!exists(&lisbon.join("IMG_0001.jpg")) && !exists(&lisbon.join("IMG_0002.jpg")));
    let first = entry(&porto, "IMG_0001.jpg").unwrap();
    assert_eq!((first["star"].clone(), first["caption"].clone(), first["x-other"].clone()), (json!(true), json!("Tram 28"), json!({ "rating": 5 })));
    assert!(first["albums"].get(&album).is_some(), "{first}");
    let second = entry(&porto, "IMG_0002.jpg").unwrap();
    assert_eq!((second["tags"].clone(), second["edits"].clone()), (json!(["lisbon", "tram"]), json!(["sepia()"])));
    assert_eq!(entry(&lisbon, "IMG_0001.jpg"), None);
    assert_eq!(read_opt(&porto.join(".dalica-originals").join("IMG_0002.jpg")).unwrap(), b"the original");
    assert!(!exists(&lisbon.join(".dalica-originals").join("IMG_0002.jpg")));
    // The model: same ids and data, new folder, same album order.
    let lib = l.s.library();
    assert_eq!(l.where_is(one), (porto.clone(), "IMG_0001.jpg".into()));
    assert!(lib.photo(one).unwrap().star);
    assert_eq!(lib.info(two).tags, ["lisbon", "tram"]);
    assert_eq!(lib.info(two).edits, ["sepia()"]);
    assert_eq!(lib.album_members(&album_id), order);

    // Undo puts the files, the entries, the backup and the model back.
    let e = l.undo();
    clean(&e);
    assert_eq!((e.direction, e.done), (Direction::Undo, 2));
    assert_eq!(read_opt(&lisbon.join("IMG_0001.jpg")).unwrap(), bytes);
    assert!(!exists(&porto.join("IMG_0001.jpg")) && !exists(&porto.join("IMG_0002.jpg")));
    assert_eq!(entry(&lisbon, "IMG_0001.jpg").unwrap()["x-other"], json!({ "rating": 5 }));
    assert_eq!(entry(&porto, "IMG_0002.jpg"), None);
    assert_eq!(read_opt(&lisbon.join(".dalica-originals").join("IMG_0002.jpg")).unwrap(), b"the original");
    assert_eq!(l.where_is(two), (lisbon.clone(), "IMG_0002.jpg".into()));
    assert_eq!(l.s.library().model(), &model_before, "the model as it was (last-changed times included)");
    assert_eq!(l.s.library().album_members(&album_id), order);

    // Redo moves them again.
    let e = l.redo();
    clean(&e);
    assert_eq!((e.direction, e.done), (Direction::Redo, 2));
    assert!(exists(&porto.join("IMG_0002.jpg")) && !exists(&lisbon.join("IMG_0002.jpg")));
    assert_eq!(entry(&porto, "IMG_0002.jpg").unwrap()["edits"], json!(["sepia()"]));
    assert_eq!(l.where_is(two).0, porto);
}

/// The first-time question: asked while "Ask before moving" is on, not after `dontAsk`.
#[test]
fn the_first_time_question_is_asked_until_dont_ask() {
    let mut l = Lib::new();
    let porto = l.folder_id("Porto");
    let (one, two, three) = (l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg"), l.photo("Lisbon", "IMG_0003.jpg"));
    let asked = l.s.execute("photo.moveTo", &json!({ "ids": [one], "folder": porto }));
    let Err(CmdError::Disabled(text)) = asked else { panic!("{asked:?}") };
    assert!(text.starts_with(dalica_engine::cmd::files::MOVE_QUESTION), "{text}");
    assert!(l.s.file_op_events().next().is_none() && !l.s.file_jobs_running(), "nothing started");
    assert!(l.s.settings().organize().ask_before_moving);
    // Confirmed once: moved, and asked again next time.
    clean(&l.run("photo.moveTo", json!({ "ids": [one], "folder": porto, "confirm": true })));
    assert!(matches!(l.s.execute("photo.moveTo", &json!({ "ids": [two], "folder": porto })), Err(CmdError::Disabled(_))));
    // "Don't ask again".
    clean(&l.run("photo.moveTo", json!({ "ids": [two], "folder": porto, "confirm": true, "dontAsk": true })));
    assert!(!l.s.settings().organize().ask_before_moving);
    clean(&l.run("photo.moveTo", json!({ "ids": [three], "folder": porto })));
    assert_eq!(l.where_is(three).0, l.folder("Porto"));
}

/// A clash asks the question naming the files; Rename Duplicates gives `-001`; Skip Duplicates
/// leaves both files untouched.
#[test]
fn a_clash_asks_rename_gives_001_and_skip_leaves_both_files() {
    let mut l = Lib::new();
    let (lisbon, porto) = (l.folder("Lisbon"), l.folder("Porto"));
    // Porto gets a photo named IMG_0001.jpg too (copied there by name).
    let one = l.photo("Lisbon", "IMG_0001.jpg");
    let porto_id = l.folder_id("Porto");
    clean(&l.run("photo.copyTo", json!({ "ids": [one], "folder": porto_id })));
    write(&porto.join("IMG_0001.jpg"), &jpeg(99));
    let theirs = read_opt(&porto.join("IMG_0001.jpg")).unwrap();
    let mine = read_opt(&lisbon.join("IMG_0001.jpg")).unwrap();
    let two = l.photo("Lisbon", "IMG_0002.jpg");

    let asked = l.s.execute("photo.moveTo", &json!({ "ids": [one, two], "folder": porto_id, "confirm": true }));
    let Err(CmdError::Disabled(text)) = asked else { panic!("{asked:?}") };
    assert!(text.starts_with(dalica_engine::cmd::files::CLASH_QUESTION) && text.contains("IMG_0001.jpg") && !text.contains("IMG_0002"), "{text}");
    assert_eq!(dalica_engine::cmd::files::clashing_names(&l.s, &[one, two], porto_id, false), ["IMG_0001.jpg"]);

    // Skip: the clashing photo stays, the other moves; both IMG_0001.jpg files untouched.
    let e = l.run("photo.moveTo", json!({ "ids": [one, two], "folder": porto_id, "confirm": true, "collisions": "skip" }));
    assert_eq!((e.done, e.skipped.len(), e.failures.len()), (1, 1, 0), "{e:?}");
    assert_eq!(e.skipped[0].file, lisbon.join("IMG_0001.jpg"));
    assert_eq!(read_opt(&porto.join("IMG_0001.jpg")).unwrap(), theirs);
    assert_eq!(read_opt(&lisbon.join("IMG_0001.jpg")).unwrap(), mine);
    assert_eq!(l.where_is(one).0, lisbon);
    assert_eq!(l.where_is(two).0, porto);

    // Rename: IMG_0001-001.jpg, with its data.
    l.s.execute("photo.star.set", &json!({ "ids": [one], "on": true })).unwrap();
    clean(&l.run("photo.moveTo", json!({ "ids": [one], "folder": porto_id, "confirm": true, "collisions": "rename" })));
    assert_eq!(l.where_is(one), (porto.clone(), "IMG_0001-001.jpg".into()));
    assert_eq!(read_opt(&porto.join("IMG_0001-001.jpg")).unwrap(), mine);
    assert_eq!(read_opt(&porto.join("IMG_0001.jpg")).unwrap(), theirs);
    assert_eq!(entry(&porto, "IMG_0001-001.jpg").unwrap()["star"], true);
    // Undo moves it back under its own name.
    clean(&l.undo());
    assert_eq!(l.where_is(one), (lisbon.clone(), "IMG_0001.jpg".into()));
    assert_eq!(read_opt(&lisbon.join("IMG_0001.jpg")).unwrap(), mine);
    assert!(!exists(&porto.join("IMG_0001-001.jpg")));
}

/// The journal's records, in order, for checking that a clash's new name is journalled before the
/// file moves there.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<String>>,
    /// Item done → wait for the test (Cancel half-way).
    hold: Mutex<Option<(Sender<()>, Receiver<()>)>>,
}

impl Journal for Recorder {
    fn begin(&self, entry: &JournalEntry) -> Result<(), String> {
        let items: Vec<String> = entry.items.iter().map(|i| format!("{} -> {:?}", i.from.display(), i.to)).collect();
        self.calls.lock().unwrap().push(format!("begin {:?} {}", entry.direction, items.join(", ")));
        Ok(())
    }

    fn item(&self, op: OpId, index: usize, state: ItemState) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("item {op} {index} {state:?}"));
        if let Some((reached, go)) = self.hold.lock().unwrap().take() {
            reached.send(()).unwrap();
            go.recv().unwrap();
        }
        Ok(())
    }

    fn end(&self, op: OpId) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("end {op}"));
        Ok(())
    }

    fn retarget(&self, op: OpId, index: usize, to: &Path) -> Result<(), String> {
        // Called before the file moves there.
        self.calls.lock().unwrap().push(format!("retarget {op} {index} {} {}", to.display(), if to.exists() { "taken" } else { "free" }));
        Ok(())
    }
}

/// A file the library does not show (written after the scan) takes the next free name, and the
/// journal holds that name before the file moves there.
#[test]
fn a_clash_only_the_disk_shows_takes_the_next_name_journalled_first() {
    let mut l = Lib::new();
    let porto = l.folder("Porto");
    let journal = Arc::new(Recorder::default());
    l.s.set_file_op_journal(journal.clone());
    write(&porto.join("IMG_0002.jpg"), b"not in the library");
    let two = l.photo("Lisbon", "IMG_0002.jpg");
    let porto_id = l.folder_id("Porto");
    clean(&l.run("photo.moveTo", json!({ "ids": [two], "folder": porto_id, "confirm": true })));
    assert_eq!(l.where_is(two), (porto.clone(), "IMG_0002-001.jpg".into()));
    assert_eq!(read_opt(&porto.join("IMG_0002.jpg")).unwrap(), b"not in the library");
    let calls = journal.calls.lock().unwrap().clone();
    let at = |prefix: &str| calls.iter().position(|c| c.starts_with(prefix)).unwrap_or_else(|| panic!("{prefix}: {calls:?}"));
    let retarget = at("retarget");
    assert!(calls[retarget].ends_with(&format!("{} free", porto.join("IMG_0002-001.jpg").display())), "{calls:?}");
    assert!(at("begin") < retarget && retarget < at("item"), "{calls:?}");
    // Undo moves it back by the name it got.
    clean(&l.undo());
    assert_eq!(l.where_is(two).1, "IMG_0002.jpg");
    assert!(!exists(&porto.join("IMG_0002-001.jpg")));
}

/// Cancel after the first item: that item stays moved, as one undo step, and the rest stay.
#[test]
fn cancel_half_way_keeps_the_moved_part_as_one_step() {
    let mut l = Lib::new();
    let (lisbon, porto) = (l.folder("Lisbon"), l.folder("Porto"));
    let ids: Vec<PhotoId> = ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"].iter().map(|n| l.photo("Lisbon", n)).collect();
    let journal = Arc::new(Recorder::default());
    let (reached_tx, reached) = mpsc::channel();
    let (go, go_rx) = mpsc::channel();
    *journal.hold.lock().unwrap() = Some((reached_tx, go_rx));
    l.s.set_file_op_journal(journal.clone());
    let steps = l.s.undo_stack().len();
    let porto_id = l.folder_id("Porto");
    let job = l.s.execute("photo.moveTo", &json!({ "ids": ids, "folder": porto_id, "confirm": true })).unwrap()["job"].as_u64().unwrap();
    reached.recv().unwrap();
    l.s.cancel_jobs(Some(job)).unwrap();
    go.send(()).unwrap();
    let e = l.wait();
    assert!(e.cancelled && e.failures.is_empty(), "{e:?}");
    assert_eq!((e.done, e.total), (1, 3));
    assert_eq!(l.where_is(ids[0]).0, porto);
    assert_eq!(l.where_is(ids[1]).0, lisbon);
    assert!(exists(&porto.join("IMG_0001.jpg")) && exists(&lisbon.join("IMG_0002.jpg")) && exists(&lisbon.join("IMG_0003.jpg")));
    assert_eq!(l.s.undo_stack().len(), steps + 1, "the moved part is one step");
    let e = l.undo();
    assert_eq!(e.done, 1);
    assert_eq!(l.where_is(ids[0]).0, lisbon);
    assert!(exists(&lisbon.join("IMG_0001.jpg")));
}

/// A destination the user cannot write: every item fails with its reason and nothing moves.
#[cfg(unix)]
#[test]
fn a_read_only_destination_fails_with_its_reason_and_nothing_moves() {
    use std::os::unix::fs::PermissionsExt;
    let mut l = Lib::new();
    let (lisbon, porto) = (l.folder("Lisbon"), l.folder("Porto"));
    let set = |mode: u32| {
        let porto = porto.clone();
        off_owner(move || std::fs::set_permissions(&porto, std::fs::Permissions::from_mode(mode)).unwrap());
    };
    set(0o555);
    // Root ignores permissions: nothing to show then.
    let probe = porto.join("probe");
    if off_owner(|| std::fs::write(&probe, b"").is_ok()) {
        set(0o755);
        return;
    }
    let ids = [l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg")];
    let model = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let porto_id = l.folder_id("Porto");
    let e = l.run("photo.moveTo", json!({ "ids": ids, "folder": porto_id, "confirm": true }));
    set(0o755);
    assert_eq!((e.done, e.failures.len()), (0, 2), "{e:?}");
    assert!(e.failures[0].reason.to_lowercase().contains("permission"), "{:?}", e.failures[0]);
    assert_eq!(e.failures[0].file, lisbon.join("IMG_0001.jpg"));
    assert!(exists(&lisbon.join("IMG_0001.jpg")) && exists(&lisbon.join("IMG_0002.jpg")));
    assert!(!exists(&porto.join("IMG_0001.jpg")));
    assert_eq!(l.s.library().model(), &model);
    assert_eq!(l.s.undo_stack().len(), steps, "nothing to undo");
}

/// A copy is a new photo with the source's data (and its entry duplicated, which the next write
/// keeps); undo trashes the copy while it is unchanged, redo copies again with the same id, and a
/// copy changed since stays.
#[test]
fn copies_are_new_photos_with_the_sources_data_and_undo_trashes_unchanged_copies() {
    let mut l = Lib::new();
    let (lisbon, porto) = (l.folder("Lisbon"), l.folder("Porto"));
    let one = l.photo("Lisbon", "IMG_0001.jpg");
    l.s.execute("photo.star.set", &json!({ "ids": [one], "on": true })).unwrap();
    l.s.execute("photo.caption.set", &json!({ "ids": [one], "caption": "Tram 28" })).unwrap();
    l.s.execute("photo.tags.add", &json!({ "ids": [one], "tags": "tram" })).unwrap();
    let album = l.s.execute("album.create", &json!({ "name": "Trams", "ids": [one] })).unwrap()["album"].as_str().unwrap().to_string();
    let album_id = dalica_core::AlbumId::parse(&album).unwrap();
    let porto_id = l.folder_id("Porto");
    let bytes = read_opt(&lisbon.join("IMG_0001.jpg")).unwrap();

    let e = l.run("photo.copyTo", json!({ "ids": [one], "folder": porto_id }));
    clean(&e);
    assert_eq!(e.kind, FileOpKind::Copy);
    let copy = l.photo("Porto", "IMG_0001.jpg");
    assert_ne!(copy, one);
    let lib = l.s.library();
    assert!(lib.photo(copy).unwrap().star);
    assert_eq!((lib.info(copy).caption.as_str(), lib.info(copy).tags.clone()), ("Tram 28", vec!["tram".to_string()]));
    assert_eq!(lib.album_members(&album_id), [one, copy]);
    assert_eq!(l.where_is(one).0, lisbon, "the source stays");
    assert_eq!(read_opt(&porto.join("IMG_0001.jpg")).unwrap(), bytes);
    // The duplicated entry survives the destination's writes from the model.
    l.s.execute("photo.tags.add", &json!({ "ids": [copy], "tags": "porto" })).unwrap();
    l.s.flush().unwrap();
    let e2 = entry(&porto, "IMG_0001.jpg").unwrap();
    assert_eq!((e2["star"].clone(), e2["caption"].clone(), e2["tags"].clone()), (json!(true), json!("Tram 28"), json!(["tram", "porto"])));
    assert!(e2["albums"].get(&album).is_some(), "{e2}");
    assert_eq!(entry(&lisbon, "IMG_0001.jpg").unwrap()["caption"], "Tram 28");
    l.s.execute("edit.undo", &json!({})).unwrap(); // the tag

    // Undo trashes the unchanged copy; redo copies it again under the same id.
    let e = l.undo();
    clean(&e);
    assert!(!exists(&porto.join("IMG_0001.jpg")));
    assert!(l.s.library().photo(copy).is_none());
    assert_eq!(l.s.library().album_members(&album_id), [one]);
    clean(&l.redo());
    assert_eq!(l.photo("Porto", "IMG_0001.jpg"), copy);
    assert_eq!(read_opt(&porto.join("IMG_0001.jpg")).unwrap(), bytes);

    // A copy changed since stays, and its undo says why.
    write(&porto.join("IMG_0001.jpg"), &jpeg(77));
    let e = l.undo();
    assert_eq!((e.done, e.failures.len()), (0, 1), "{e:?}");
    assert!(e.failures[0].reason.contains("changed"), "{:?}", e.failures[0]);
    assert!(exists(&porto.join("IMG_0001.jpg")));
    assert!(l.s.library().photo(copy).is_some());
}

/// A copy into the photo's own folder clashes with itself: asked, then `-001`.
#[test]
fn a_copy_into_its_own_folder_asks_then_takes_001() {
    let mut l = Lib::new();
    let lisbon_id = l.folder_id("Lisbon");
    let one = l.photo("Lisbon", "IMG_0001.jpg");
    assert!(matches!(l.s.execute("photo.copyTo", &json!({ "ids": [one], "folder": lisbon_id })), Err(CmdError::Disabled(_))));
    clean(&l.run("photo.copyTo", json!({ "ids": [one], "folder": lisbon_id, "collisions": "rename" })));
    let copy = l.photo("Lisbon", "IMG_0001-001.jpg");
    assert_ne!(copy, one);
    assert!(exists(&l.folder("Lisbon").join("IMG_0001-001.jpg")));
}

/// Graceful failures: unknown photos and folders, empty selections, bad collision rules, too many
/// photos, photos already there; nothing moves and no step is recorded.
#[test]
fn graceful_failures_change_nothing() {
    let mut l = Lib::new();
    let (porto, lisbon) = (l.folder_id("Porto"), l.folder_id("Lisbon"));
    let one = l.photo("Lisbon", "IMG_0001.jpg");
    let model = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let cases = [
        ("photo.moveTo", json!({ "ids": [9999], "folder": porto, "confirm": true }), "not_found"),
        ("photo.moveTo", json!({ "ids": [one], "folder": 9999, "confirm": true }), "not_found"),
        ("photo.moveTo", json!({ "ids": [], "folder": porto, "confirm": true }), "disabled"),
        ("photo.moveTo", json!({ "ids": [one], "folder": lisbon, "confirm": true }), "disabled"),
        ("photo.moveTo", json!({ "ids": [one], "confirm": true }), "bad_params"),
        ("photo.moveTo", json!({ "ids": [one], "folder": porto, "collisions": "overwrite", "confirm": true }), "bad_params"),
        ("photo.moveTo", json!({ "ids": vec![one; 100_001], "folder": porto, "confirm": true }), "limit"),
        ("photo.copyTo", json!({ "ids": [9999], "folder": porto }), "not_found"),
        ("photo.copyTo", json!({ "ids": [one], "folder": 9999 }), "not_found"),
        ("photo.copyTo", json!({ "ids": [], "folder": porto }), "disabled"),
        ("photo.copyTo", json!({ "ids": [one], "folder": porto, "confirm": true }), "bad_params"),
    ];
    for (id, p, code) in cases {
        let r = l.s.execute(id, &p);
        assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{id} {p}: {r:?}");
    }
    assert!(!l.s.file_jobs_running() && l.s.file_op_events().next().is_none(), "nothing started");
    assert_eq!(l.s.library().model(), &model);
    assert_eq!(l.s.undo_stack().len(), steps);
}
