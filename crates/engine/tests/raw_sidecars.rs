//! A RAW's `.xmp` sidecar moves, copies and renames with it (`roadmap.md` M16.18;
//! `xmp-interop.md` §7, `data-model.md` §8): a move takes it and undo and redo bring it back; the
//! JPEG of a RAW+JPEG pair moves alone; a rename names it after the RAW in its own form; a copy
//! gets a byte-identical copy, which undo sends to the trash; a clash names both `-001`; a taken
//! name is never overwritten; a failure before the sidecar's rename, one after it and a crash leave
//! every file readable and the journal repaired. The owner thread makes no `fs` call.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FaultLock, exists, files, jpeg, off_owner, options, read, shared, virtual_clock, wait_files};
use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_engine::fileops::{ItemState, Journal, JournalEntry, OpId, StoreJournal};
use dalica_engine::raw_sidecars::planted;
use dalica_engine::watching::WatchMode;
use dalica_engine::{FileOpEvent, Session};
use dalica_fs::Expect;
use dalica_fs::faults::{Fault, points};
use dalica_store::intents::Intents;
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// The start of a Canon CR3 (recognised as a RAW), then `seed` so that no two are alike.
fn cr3(seed: u64) -> Vec<u8> {
    let mut b = b"\0\0\0\x18ftypcrx \0\0\0\x01 the rest of a Canon RAW file ".to_vec();
    b.extend(seed.to_string().as_bytes());
    b
}

/// A sidecar as a raw converter writes it, with `caption`.
fn xmp(caption: &str) -> Vec<u8> {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{caption}</rdf:li></rdf:Alt></dc:description></rdf:Description></rdf:RDF></x:xmpmeta>\n"
    )
    .into_bytes()
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    _faults: FaultLock,
}

impl Lib {
    /// `Pictures/Raw` with `IMG_0001.CR3` and its `IMG_0001.xmp`, the pair's `IMG_0001.JPG`, and
    /// `IMG_0002.CR3` with `IMG_0002.CR3.xmp`; `Pictures/Porto` with `PORTO_0001.jpg` and `more`;
    /// scanned, watching off.
    fn new(more: &[(&str, Vec<u8>)]) -> Lib {
        let _faults = shared();
        let dir = tempfile::tempdir().unwrap();
        let (raw, porto) = (dir.path().join("Pictures").join("Raw"), dir.path().join("Pictures").join("Porto"));
        let put = |path: PathBuf, bytes: Vec<u8>| {
            dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
            dalica_fs::replace(&path, &bytes, Expect::Absent).unwrap();
        };
        put(raw.join("IMG_0001.CR3"), cr3(1));
        put(raw.join("IMG_0001.xmp"), xmp("Tram 28"));
        put(raw.join("IMG_0001.JPG"), jpeg(1));
        put(raw.join("IMG_0002.CR3"), cr3(2));
        put(raw.join("IMG_0002.CR3.xmp"), xmp("Ponte"));
        put(porto.join("PORTO_0001.jpg"), jpeg(2));
        for (name, bytes) in more {
            put(porto.join(name), bytes.clone());
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
        self.s.library().photo_by_name(self.folder_id(folder), name).unwrap_or_else(|| panic!("no {folder}/{name}")).id
    }

    /// Runs `id` (the owner thread marked: it may make no `fs` call) and waits for its operation.
    fn run(&mut self, id: &str, p: Value) -> FileOpEvent {
        let ui = dalica_fs::mark_ui_thread();
        let refused = dalica_fs::guard::refused();
        self.s.execute(id, &p).unwrap();
        assert!(self.s.wait_file_jobs(PATIENCE), "the file jobs ended");
        assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
        drop(ui);
        wait_files(&mut self.s);
        self.s.flush().unwrap();
        let e = self.s.file_op_events().last().cloned().unwrap();
        assert!(e.failures.is_empty() && e.skipped.is_empty() && !e.cancelled, "{e:?}");
        e
    }

    fn undo(&mut self) -> FileOpEvent {
        self.run("edit.undo", json!({}))
    }

    fn redo(&mut self) -> FileOpEvent {
        self.run("edit.redo", json!({}))
    }

    fn move_to(&mut self, ids: &[PhotoId], folder: &str, collisions: Option<&str>) -> FileOpEvent {
        let mut p = json!({ "ids": ids, "folder": self.folder_id(folder), "confirm": true });
        if let Some(c) = collisions {
            p["collisions"] = json!(c);
        }
        self.run("photo.moveTo", p)
    }
}

/// Moving `IMG_0001.CR3` takes `IMG_0001.xmp` along, and undo and redo take it back and forth with
/// the RAW; moving only the JPEG of a RAW+JPEG pair leaves the sidecar with the RAW.
#[test]
fn a_moved_raw_takes_its_sidecar_and_undo_and_redo_take_it_back() {
    let mut l = Lib::new(&[]);
    let (raw, porto) = (l.folder("Raw"), l.folder("Porto"));
    let id = l.photo("Raw", "IMG_0001.CR3");
    l.move_to(&[id], "Porto", None);
    assert_eq!(files(&porto), ["IMG_0001.CR3", "IMG_0001.xmp", "PORTO_0001.jpg"]);
    assert_eq!(files(&raw), ["IMG_0001.JPG", "IMG_0002.CR3", "IMG_0002.CR3.xmp"]);
    assert_eq!(read(&porto.join("IMG_0001.xmp")), xmp("Tram 28"));
    l.undo();
    assert_eq!(files(&porto), ["PORTO_0001.jpg"]);
    assert_eq!(files(&raw), ["IMG_0001.CR3", "IMG_0001.JPG", "IMG_0001.xmp", "IMG_0002.CR3", "IMG_0002.CR3.xmp"]);
    l.redo();
    assert_eq!(files(&porto), ["IMG_0001.CR3", "IMG_0001.xmp", "PORTO_0001.jpg"]);
    l.undo();
    // The JPEG of the pair moves alone: the stem sidecar is the RAW's.
    let jpg = l.photo("Raw", "IMG_0001.JPG");
    l.move_to(&[jpg], "Porto", None);
    assert_eq!(files(&porto), ["IMG_0001.JPG", "PORTO_0001.jpg"]);
    assert_eq!(files(&raw), ["IMG_0001.CR3", "IMG_0001.xmp", "IMG_0002.CR3", "IMG_0002.CR3.xmp"]);
    assert!(l.s.notices().iter().all(|n| !n.contains(".xmp")), "{:?}", l.s.notices());
}

/// Renaming names each sidecar after the RAW in its own form (`<stem>.xmp`, `<file>.xmp`), and
/// undo gives both their names back.
#[test]
fn a_renamed_raw_renames_its_sidecar_in_its_own_form() {
    let mut l = Lib::new(&[]);
    let raw = l.folder("Raw");
    let (one, two) = (l.photo("Raw", "IMG_0001.CR3"), l.photo("Raw", "IMG_0002.CR3"));
    l.run("photo.rename", json!({ "ids": [one], "name": "trip" }));
    l.run("photo.rename", json!({ "ids": [two], "name": "trip2" }));
    assert_eq!(files(&raw), ["IMG_0001.JPG", "trip.CR3", "trip.xmp", "trip2.CR3", "trip2.CR3.xmp"]);
    assert_eq!(read(&raw.join("trip2.CR3.xmp")), xmp("Ponte"));
    l.undo();
    l.undo();
    assert_eq!(files(&raw), ["IMG_0001.CR3", "IMG_0001.JPG", "IMG_0001.xmp", "IMG_0002.CR3", "IMG_0002.CR3.xmp"]);
}

/// A copy gets a byte-identical copy of the sidecar and the source keeps its own; undoing the copy
/// sends both copies to the trash.
#[test]
fn a_copied_raw_gets_a_copy_of_its_sidecar() {
    let mut l = Lib::new(&[]);
    let (raw, porto) = (l.folder("Raw"), l.folder("Porto"));
    let two = l.photo("Raw", "IMG_0002.CR3");
    l.run("photo.copyTo", json!({ "ids": [two], "folder": l.folder_id("Porto") }));
    assert_eq!(files(&porto), ["IMG_0002.CR3", "IMG_0002.CR3.xmp", "PORTO_0001.jpg"]);
    assert_eq!(read(&porto.join("IMG_0002.CR3.xmp")), read(&raw.join("IMG_0002.CR3.xmp")));
    l.undo();
    assert_eq!(files(&porto), ["PORTO_0001.jpg"]);
    assert_eq!(read(&raw.join("IMG_0002.CR3.xmp")), xmp("Ponte"));
}

/// A clash that renames the RAW to `-001` names its sidecar the same; a sidecar name already taken
/// at the destination is never overwritten: the source's stays where it was and a notice says so.
#[test]
fn a_clash_names_both_and_a_taken_name_is_never_overwritten() {
    let theirs = xmp("Another program's");
    let mut l = Lib::new(&[("IMG_0001.CR3", cr3(9)), ("IMG_0002.xmp", theirs.clone())]);
    let (raw, porto) = (l.folder("Raw"), l.folder("Porto"));
    let (one, two) = (l.photo("Raw", "IMG_0001.CR3"), l.photo("Raw", "IMG_0002.CR3"));
    l.move_to(&[one], "Porto", Some("rename"));
    assert!(exists(&porto.join("IMG_0001-001.CR3")) && exists(&porto.join("IMG_0001-001.xmp")));
    assert_eq!(read(&porto.join("IMG_0001-001.xmp")), xmp("Tram 28"));
    // `IMG_0002.CR3.xmp` going under the file form is free; renamed to `IMG_0002.CR3` in its stem
    // form it would not be: put a stem sidecar on it first.
    off_owner(|| std::fs::rename(raw.join("IMG_0002.CR3.xmp"), raw.join("IMG_0002.xmp")).unwrap());
    l.move_to(&[two], "Porto", None);
    assert!(exists(&porto.join("IMG_0002.CR3")));
    assert_eq!(read(&porto.join("IMG_0002.xmp")), theirs, "never overwritten");
    assert_eq!(read(&raw.join("IMG_0002.xmp")), xmp("Ponte"), "the source's stays");
    let note = l.s.notices().iter().find(|n| n.contains("IMG_0002.xmp stayed")).expect("a notice");
    assert!(note.contains("already has a file named IMG_0002.xmp"), "{note}");
}

/// A failure of the disk before the sidecar's rename leaves it where it was (the RAW moved; a
/// notice says so), one after it counts as moved (undo takes it back); each operation leaves the
/// journal.
#[test]
fn a_failure_before_or_after_the_sidecars_rename_loses_nothing() {
    let mut l = Lib::new(&[]);
    let (raw, porto) = (l.folder("Raw"), l.folder("Porto"));
    let (one, two) = (l.photo("Raw", "IMG_0001.CR3"), l.photo("Raw", "IMG_0002.CR3"));
    planted::fault_at(&raw.join("IMG_0001.xmp"), points::MOVE_RENAME, Fault::Error);
    l.move_to(&[one], "Porto", None);
    assert_eq!(files(&porto), ["IMG_0001.CR3", "PORTO_0001.jpg"]);
    assert_eq!(read(&raw.join("IMG_0001.xmp")), xmp("Tram 28"));
    assert!(l.s.notices().iter().any(|n| n.contains("IMG_0001.xmp stayed")), "{:?}", l.s.notices());
    planted::fault_at(&raw.join("IMG_0002.CR3.xmp"), points::MOVE_VERIFY, Fault::Error);
    l.move_to(&[two], "Porto", None);
    assert_eq!(read(&porto.join("IMG_0002.CR3.xmp")), xmp("Ponte"));
    assert!(!l.s.notices().iter().any(|n| n.contains("IMG_0002.CR3.xmp stayed")), "{:?}", l.s.notices());
    assert!(!planted::pending(&raw.join("IMG_0001.xmp")) && !planted::pending(&raw.join("IMG_0002.CR3.xmp")), "both failures were reached");
    l.undo();
    assert_eq!(files(&raw), ["IMG_0001.JPG", "IMG_0001.xmp", "IMG_0002.CR3", "IMG_0002.CR3.xmp"]);
    assert_eq!(journal(&l.dir.path().join("library"))["ops"], json!([]));
}

/// The journal of a session that stops (the process killed) once its first operation is entered:
/// nothing after is written.
struct Stopped(StoreJournal);

impl Journal for Stopped {
    fn begin(&self, entry: &JournalEntry) -> Result<(), String> {
        self.0.begin(entry)
    }

    fn item(&self, _: OpId, _: usize, _: ItemState) -> Result<(), String> {
        Ok(())
    }

    fn end(&self, _: OpId) -> Result<(), String> {
        Ok(())
    }
}

fn journal(library: &Path) -> Value {
    let path = library.join("file-ops.json");
    off_owner(move || std::fs::read(path).map_or(json!({ "ops": [] }), |b| serde_json::from_slice(&b).unwrap()))
}

/// A crash at the sidecar's rename (the RAW moved, its item not yet done) leaves both files
/// readable, and the next open repairs the journal.
#[test]
fn a_crash_at_the_sidecars_rename_leaves_every_file_readable_and_the_journal_repaired() {
    let mut l = Lib::new(&[]);
    let (raw, porto, library) = (l.folder("Raw"), l.folder("Porto"), l.dir.path().join("library"));
    let one = l.photo("Raw", "IMG_0001.CR3");
    let intents = off_owner(|| Intents::open(&library, Timestamp::from_millis(0)).unwrap().intents);
    l.s.set_file_op_journal(Arc::new(Stopped(StoreJournal::new(intents))));
    planted::fault_at(&raw.join("IMG_0001.xmp"), points::MOVE_RENAME, Fault::Crash);
    l.move_to(&[one], "Porto", None);
    assert!(!planted::pending(&raw.join("IMG_0001.xmp")), "the crash was reached");
    assert!(!exists(&porto.join("IMG_0001.xmp")), "the sidecar never moved");
    assert_eq!(journal(&library)["ops"].as_array().map(Vec::len), Some(1), "left in the journal");
    let Lib { dir, s, _faults } = l;
    drop(s);
    let mut s = Session::open_with(&library, virtual_clock(), &options()).unwrap();
    s.set_watching(WatchMode::Off);
    assert!(s.wait_file_jobs(PATIENCE), "the repair ended");
    s.wait_idle();
    s.flush().unwrap();
    assert!(s.file_ops_left().is_empty());
    assert_eq!(journal(&library)["ops"], json!([]));
    let porto_id = s.library().folder_by_path(&porto).unwrap();
    assert_eq!(s.library().photo_by_name(porto_id, "IMG_0001.CR3").map(|p| p.id), Some(one));
    assert_eq!(read(&porto.join("IMG_0001.CR3")), cr3(1));
    assert_eq!(read(&raw.join("IMG_0001.xmp")), xmp("Tram 28"));
    drop(dir);
}
