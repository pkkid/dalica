//! File operations with the real journal and sidecar carry (`roadmap.md` M9.9; `data-model.md` §8):
//! a fake move through `fs` is in `file-ops.json` before any item runs and leaves it empty once
//! applied; the photo's sidecar entry travels through `Driver::carry` with its unknown keys and a
//! star still waiting in the debounce, and comes back on undo; its originals backup travels too;
//! the journal's ids continue past an operation an earlier session left.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::common::{entry, jpeg, off_owner, options, read_json, shared, wait_files};
use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::watching::WatchMode;
use dalica_engine::{Driver, FileOp, FileOpKind, Ran, Session};
use dalica_fs::Expect;
use dalica_fs::transfer::{self, Outcome};
use dalica_library::Op;
use dalica_store::carry::{Carry, Mode};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(120);

/// `file-ops.json` as the tests read it: per operation, its id and each item's source and state.
type Journal = Vec<(u64, Vec<(PathBuf, String)>)>;

/// The ids of the operations in `file-ops.json`, with each item's source and state.
fn journal(library: &Path) -> Journal {
    let Some(v) = read_json(&library.join("file-ops.json")) else { return Vec::new() };
    v["ops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            let items = o["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| (PathBuf::from(i["from"].as_str().unwrap()), i["state"].as_str().unwrap().to_string()))
                .collect();
            (o["id"].as_u64().unwrap(), items)
        })
        .collect()
}

/// One fake item: move photo `photo`'s file `name` from folder `from` to folder `to`.
#[derive(Clone, Debug)]
struct Item {
    photo: PhotoId,
    from: PathBuf,
    to: PathBuf,
    name: String,
    to_folder: dalica_core::FolderId,
    /// The file and where it goes (the journal's `from` and `to`).
    src: PathBuf,
    dst: PathBuf,
}

impl Item {
    fn path(&self) -> PathBuf {
        self.src.clone()
    }
}

/// The fake operation: moves a photo's file to another folder through `fs` (never replacing one)
/// and names its entry for the carry. Notes, per item, what `file-ops.json` said when it ran.
struct Move {
    library: PathBuf,
    seen: Arc<Mutex<Vec<Journal>>>,
}

impl Driver for Move {
    type Item = Item;
    type Done = ();

    fn source<'a>(&self, item: &'a Item) -> &'a Path {
        &item.src
    }

    fn target<'a>(&self, item: &'a Item) -> Option<&'a Path> {
        Some(&item.dst)
    }

    fn run(&self, item: &Item) -> Result<Ran<()>, String> {
        self.seen.lock().unwrap().push(journal(&self.library));
        let identity = dalica_fs::identity(&item.path()).map_err(|e| e.to_string())?.ok_or("the file is gone")?;
        match transfer::move_file(&item.from, &item.name, identity, &item.to, &item.name).map_err(|e| e.to_string())? {
            Outcome::Done(_) => Ok(Ran::Done(())),
            Outcome::Exists => Ok(Ran::Skipped("the name is taken".into())),
        }
    }

    fn ops(&self, _: &Session, item: &Item, _: &()) -> Result<Vec<Op>, String> {
        Ok(vec![Op::MovePhotos { moves: vec![(item.photo, item.to_folder, item.name.clone())] }])
    }

    fn reverse(&self, item: &Item, _: &()) -> Result<Item, String> {
        Err(format!("{} cannot move back in this test", item.name))
    }

    fn carry(&self, item: &Item, _: &()) -> Option<Carry> {
        Some(Carry::one(Mode::Move, &item.from, &item.name, &item.to, &item.name))
    }
}

struct Lib {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    s: Session,
}

impl Lib {
    /// Pictures/Lisbon with two photos and Pictures/Porto with one of its own, scanned, on a virtual clock.
    /// `before_open` runs on the library directory first.
    fn new(before_open: impl FnOnce(&Path)) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        for (folder, names) in [("Lisbon", &["IMG_0001.jpg", "IMG_0002.jpg"][..]), ("Porto", &["PORTO_0001.jpg"][..])] {
            let path = dir.path().join("Pictures").join(folder);
            dalica_fs::create_dir_all(&path).unwrap();
            for (i, name) in names.iter().enumerate() {
                dalica_fs::replace(&path.join(name), &jpeg(i as u64), Expect::Absent).unwrap();
            }
        }
        let library = dir.path().join("library");
        dalica_fs::create_dir_all(&library).unwrap();
        before_open(&library);
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
        let mut s = Session::open_with(&library, clock.clone(), &options()).unwrap();
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": dir.path().join("Pictures") })).unwrap();
        s.wait_idle();
        Lib { dir, clock, s }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.dir.path().join("Pictures").join(name)
    }

    fn library(&self) -> PathBuf {
        self.dir.path().join("library")
    }

    fn photo(&self, folder: &str, name: &str) -> PhotoId {
        let f = self.s.library().folder_by_path(&self.folder(folder)).unwrap();
        self.s.library().photos().find(|p| p.folder == f && p.name == name).unwrap().id
    }

    fn item(&self, name: &str) -> Item {
        Item {
            photo: self.photo("Lisbon", name),
            from: self.folder("Lisbon"),
            to: self.folder("Porto"),
            name: name.into(),
            to_folder: self.s.library().folder_by_path(&self.folder("Porto")).unwrap(),
            src: self.folder("Lisbon").join(name),
            dst: self.folder("Porto").join(name),
        }
    }

    fn driver(&self) -> (Move, Arc<Mutex<Vec<Journal>>>) {
        let seen = Arc::default();
        (Move { library: self.library(), seen: Arc::clone(&seen) }, seen)
    }

    fn run(&mut self, items: Vec<Item>) {
        let (driver, _) = self.driver();
        self.s.start_file_op(FileOp::new(driver, FileOpKind::Move, "Move", items));
        wait_files(&mut self.s);
        let e = self.s.file_op_events().last().unwrap();
        assert!(e.failures.is_empty() && e.skipped.is_empty(), "{e:?}");
    }
}

/// The operation is in `file-ops.json`, every item pending, before its first item runs, and the
/// journal is empty once the operation's results are in the library.
#[test]
fn file_ops_json_holds_the_operation_before_any_item_runs_and_is_empty_after() {
    let _faults = shared();
    let mut l = Lib::new(|_| {});
    assert!(l.s.file_ops_left().is_empty());
    let (driver, seen) = l.driver();
    let items = vec![l.item("IMG_0001.jpg"), l.item("IMG_0002.jpg")];
    let (a, b) = (items[0].path(), items[1].path());
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    l.s.start_file_op(FileOp::new(driver, FileOpKind::Move, "Move", items));
    assert!(l.s.wait_file_jobs(PATIENCE));
    l.s.flush().unwrap();
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
    let seen = seen.lock().unwrap().clone();
    let first = &seen[0];
    assert_eq!(first.len(), 1, "one operation");
    assert_eq!(first[0].1, [(a.clone(), "pending".to_string()), (b.clone(), "pending".to_string())], "entered before the first item ran");
    assert_eq!(seen[1][0].1[0], (a, "done".to_string()), "the first item marked done before the second ran");
    assert_eq!(read_json(&l.library().join("file-ops.json")).unwrap(), json!({ "version": 1, "ops": [] }));
    let porto = l.folder("Porto");
    assert!(off_owner(move || porto.join("IMG_0002.jpg").exists()));
}

/// A move carries the photo's sidecar entry: the keys Dalica does not know, and a star made just
/// before the move whose write still waits in the debounce. Its originals backup and record go too.
#[test]
fn a_move_carries_the_entry_its_unknown_keys_a_debounced_star_and_the_backup() {
    let _faults = shared();
    let mut l = Lib::new(|_| {});
    let (lisbon, porto) = (l.folder("Lisbon"), l.folder("Porto"));
    let (one, two) = (l.photo("Lisbon", "IMG_0001.jpg"), l.photo("Lisbon", "IMG_0002.jpg"));
    l.s.execute("photo.star.set", &json!({ "ids": [one], "on": true })).unwrap();
    l.s.flush().unwrap();
    // Keys only another program knows, and a saved original with its record.
    {
        let lisbon = lisbon.clone();
        off_owner(move || {
            let path = lisbon.join(".dalica.json");
            let mut root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            root["photos"]["IMG_0001.jpg"]["x-other"] = json!({ "rating": 5 });
            root["photos"]["IMG_0002.jpg"]["faces"] = json!([{ "person": "p7f3a2c" }]);
            std::fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
            let originals = dalica_fs::originals::ensure_dir(&lisbon).unwrap();
            std::fs::write(originals.join("IMG_0002.jpg"), b"the original").unwrap();
            let record = json!({ "dalica": { "version": 1 }, "originals": { "IMG_0002.jpg": {
                "recipe": ["sepia()"], "size": 12, "mtime": 1, "width": 4, "height": 3, "digest": "", "state": "done", "x-future": 1 } } });
            std::fs::write(originals.join(".dalica.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        });
    }
    // Starred now, within a second of the last write on this clock: the write waits.
    l.clock.advance(Duration::from_millis(100));
    l.s.execute("photo.star.set", &json!({ "ids": [two], "on": true })).unwrap();
    assert_eq!(entry(&lisbon, "IMG_0002.jpg").unwrap().get("star"), None, "still in the debounce");

    l.run(vec![l.item("IMG_0001.jpg"), l.item("IMG_0002.jpg")]);
    l.s.flush().unwrap();
    let first = entry(&porto, "IMG_0001.jpg").unwrap();
    assert_eq!((first["star"].clone(), first["x-other"].clone()), (json!(true), json!({ "rating": 5 })));
    let second = entry(&porto, "IMG_0002.jpg").unwrap();
    assert_eq!(second["star"], true, "the debounced star landed in the destination");
    assert_eq!(second["faces"], json!([{ "person": "p7f3a2c" }]));
    assert_eq!(entry(&lisbon, "IMG_0001.jpg"), None);
    assert_eq!(entry(&lisbon, "IMG_0002.jpg"), None);
    // The backup and its record.
    let (src, dst) = (lisbon.join(".dalica-originals"), porto.join(".dalica-originals"));
    assert_eq!(off_owner(|| std::fs::read(dst.join("IMG_0002.jpg")).unwrap()), b"the original");
    assert!(!off_owner(|| src.join("IMG_0002.jpg").exists()));
    assert_eq!(read_json(&dst.join(".dalica.json")).unwrap()["originals"]["IMG_0002.jpg"]["x-future"], 1);
    assert_eq!(read_json(&src.join(".dalica.json")).unwrap()["originals"].get("IMG_0002.jpg"), None);
    // The model agrees.
    let lib = l.s.library();
    assert!(lib.photo(two).unwrap().star && lib.photo(one).unwrap().star);
    assert_eq!(lib.photo(two).unwrap().folder, lib.folder_by_path(&porto).unwrap());
}

/// An operation an earlier session left in the journal is found at open and repaired (M9.21: here
/// neither of its ends is there, so it is reported lost and leaves the journal), and this session's
/// operations take the ids after it.
#[test]
fn journal_ids_continue_past_an_operation_left_from_an_earlier_session() {
    let _faults = shared();
    let left_from = Arc::new(Mutex::new(PathBuf::new()));
    let lf = Arc::clone(&left_from);
    let mut l = Lib::new(move |library| {
        let from = library.join("elsewhere").join("IMG_9999.jpg");
        *lf.lock().unwrap() = from.clone();
        let old = json!({ "version": 1, "ops": [{ "id": 5, "kind": "copy", "direction": "do",
            "items": [{ "from": from, "to": library.join("there").join("IMG_9999.jpg"), "state": "pending" }] }] });
        std::fs::write(library.join("file-ops.json"), serde_json::to_vec(&old).unwrap()).unwrap();
    });
    assert!(l.s.notices().iter().all(|n| !n.contains("journal")), "{:?}", l.s.notices());
    // The repair at open (the setup waited for it).
    assert!(l.s.wait_file_jobs(PATIENCE));
    l.s.flush().unwrap();
    let repairs: Vec<_> = l.s.file_op_repairs().map(|r| (r.op, r.from.clone(), r.outcome)).collect();
    assert_eq!(repairs, [(5, left_from.lock().unwrap().clone(), dalica_engine::fileops_recovery::RepairOutcome::Lost)]);
    assert!(l.s.file_ops_left().is_empty());
    let (driver, seen) = l.driver();
    let item = l.item("IMG_0001.jpg");
    l.s.start_file_op(FileOp::new(driver, FileOpKind::Move, "Move", vec![item]));
    assert!(l.s.wait_file_jobs(PATIENCE));
    l.s.flush().unwrap();
    let ids: Vec<u64> = seen.lock().unwrap()[0].iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [6], "the ids continue past the operation left");
    assert_eq!(l.s.file_op_events().last().unwrap().op, 6);
    assert_eq!(journal(&l.library()), [], "nothing left");
}

/// A damaged journal is kept aside with a notice, and operations still run.
#[test]
fn a_damaged_journal_is_kept_aside_with_a_notice() {
    let _faults = shared();
    let mut l = Lib::new(|library| std::fs::write(library.join("file-ops.json"), b"{ torn").unwrap());
    assert!(l.s.notices().iter().any(|n| n.contains("file-operation journal was damaged")), "{:?}", l.s.notices());
    let kept: Vec<String> = {
        let library = l.library();
        off_owner(move || std::fs::read_dir(library).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
    };
    assert!(kept.iter().any(|n| n.starts_with("file-ops.json.corrupt-")), "{kept:?}");
    let item = l.item("IMG_0001.jpg");
    l.run(vec![item]);
    assert_eq!(l.s.file_op_events().last().unwrap().done, 1);
}
