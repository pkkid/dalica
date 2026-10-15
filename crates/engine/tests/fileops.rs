//! File operations on the undo stack (`roadmap.md` M9.3; `architecture.md` §4–§5, `data-model.md`
//! §8) with a fake operation that renames photos in their folder through `fs`: it runs as a file
//! job and applies once as one undo step; undo runs its reverse as a job (the step staying on the
//! stack meanwhile) and redo runs it again; Cancel between files keeps what finished as one step; a
//! failure is reported with its reason while the rest go on, and a skipped item is reported as
//! skipped; every operation is in the file-operation journal before anything moves and leaves it
//! once applied, and one whose entry cannot be written moves nothing; a step without a way back
//! says why; the watcher imports nothing for the paths Dalica changed; the owner thread makes no
//! `fs` call; and `rebase_folders` moves the watched-folder entries at and under a folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::common::{exists, jpeg, off_owner, options, shared};
use dalica_core::{PhotoId, SystemClock, Timestamp, VirtualClock};
use dalica_engine::fileops::{ItemState, Journal, JournalEntry, MemoryJournal, OpId};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CmdError, Direction, Driver, FileOp, FileOpKind, JobKind, JobState, Ran, Session};
use dalica_fs::Expect;
use dalica_library::Op;
use serde_json::json;

const PATIENCE: Duration = Duration::from_secs(120);

/// The names in `folder` that are photos (no sidecar, no temporaries), sorted.
fn names(folder: &Path) -> Vec<String> {
    let folder = folder.to_path_buf();
    let mut v: Vec<String> = off_owner(move || {
        std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".jpg")).collect()
    });
    v.sort();
    v
}

/// Holds items in `run` until let through, and counts those that arrived.
#[derive(Default)]
struct Gate {
    state: Mutex<(usize, usize)>,
    cv: Condvar,
}

impl Gate {
    fn shut() -> Arc<Gate> {
        Arc::new(Gate::default())
    }

    fn open() -> Arc<Gate> {
        let g = Gate::default();
        g.state.lock().unwrap().0 = usize::MAX;
        Arc::new(g)
    }

    /// Lets the first `n` arrivals through.
    fn allow(&self, n: usize) {
        self.state.lock().unwrap().0 = n;
        self.cv.notify_all();
    }

    /// Lets every arrival so far through and holds the next.
    fn hold_next(&self) {
        let mut st = self.state.lock().unwrap();
        st.0 = st.1;
    }

    fn arrived(&self) -> usize {
        self.state.lock().unwrap().1
    }

    fn pass(&self) {
        let mut st = self.state.lock().unwrap();
        let me = st.1;
        st.1 += 1;
        while me >= st.0 {
            st = self.cv.wait(st).unwrap();
        }
    }

    fn wait_arrived(&self, n: usize) {
        let start = Instant::now();
        while self.arrived() < n {
            assert!(start.elapsed() < PATIENCE, "items arrived");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// One fake item: rename `from` to `to` (same folder), photo `photo`.
#[derive(Clone, Debug)]
struct Item {
    photo: PhotoId,
    from: PathBuf,
    to: PathBuf,
    fail: bool,
    skip: bool,
}

/// What a rename did: the name it got and the file's time.
struct Renamed {
    name: String,
    mtime: Timestamp,
}

/// The fake operation: renames a photo's file in its folder through `fs` (read, a new file that
/// never replaces one, the time put back, the old one removed), on the job's thread.
struct Rename {
    gate: Arc<Gate>,
    journal: Option<Arc<MemoryJournal>>,
    /// Per run: whether the journal held the operation with this item pending when it ran.
    journaled: Arc<Mutex<Vec<bool>>>,
    /// Undoing it is impossible, for this reason.
    no_way_back: Option<&'static str>,
}

impl Rename {
    fn new(gate: Arc<Gate>) -> Rename {
        Rename { gate, journal: None, journaled: Arc::default(), no_way_back: None }
    }
}

impl Driver for Rename {
    type Item = Item;
    type Done = Renamed;

    fn source<'a>(&self, item: &'a Item) -> &'a Path {
        &item.from
    }

    fn target<'a>(&self, item: &'a Item) -> Option<&'a Path> {
        Some(&item.to)
    }

    fn run(&self, item: &Item) -> Result<Ran<Renamed>, String> {
        self.gate.pass();
        if let Some(j) = &self.journal {
            let pending = j.entries().iter().any(|e| e.items.iter().any(|i| i.from == item.from && i.state == ItemState::Pending));
            self.journaled.lock().unwrap().push(pending);
        }
        if item.fail {
            return Err("a planted failure".into());
        }
        if item.skip {
            return Ok(Ran::Skipped("a file with that name is already there".into()));
        }
        let read = dalica_fs::read(&item.from, 64 << 20).map_err(|e| e.to_string())?.ok_or("the file is gone")?;
        let times = dalica_fs::stat(&item.from).map_err(|e| e.to_string())?.ok_or("the file is gone")?.times;
        let folder = item.to.parent().ok_or("no folder")?;
        let name = item.to.file_name().and_then(|n| n.to_str()).ok_or("no name")?;
        let created = dalica_fs::create::new_file(folder, name, &read.bytes).map_err(|e| e.to_string())?;
        let target = folder.join(&created.name);
        dalica_fs::set_times(&target, &times, created.identity).map_err(|e| e.to_string())?;
        dalica_fs::remove_own_file(&item.from).map_err(|e| e.to_string())?;
        let mtime = Timestamp::from_system_time(dalica_fs::stat(&target).map_err(|e| e.to_string())?.ok_or("the copy is gone")?.times.modified);
        Ok(Ran::Done(Renamed { name: created.name, mtime }))
    }

    fn ops(&self, s: &Session, item: &Item, done: &Renamed) -> Result<Vec<Op>, String> {
        let photo = s.library().photo(item.photo).ok_or("the photo left the library")?;
        Ok(vec![Op::UpdatePhotos { photos: vec![dalica_library::Photo { name: done.name.clone(), mtime: done.mtime, ..photo.clone() }] }])
    }

    fn reverse(&self, item: &Item, done: &Renamed) -> Result<Item, String> {
        if let Some(reason) = self.no_way_back {
            return Err(reason.into());
        }
        let now = item.to.with_file_name(&done.name);
        Ok(Item { photo: item.photo, from: now, to: item.from.clone(), fail: false, skip: false })
    }

    fn settled(&self, item: &Item, done: &Renamed) -> Item {
        Item { to: item.to.with_file_name(&done.name), ..item.clone() }
    }
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
}

fn write_photos(lisbon: &Path, n: usize) {
    dalica_fs::create_dir_all(lisbon).unwrap();
    for i in 0..n {
        dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64), Expect::Absent).unwrap();
    }
}

impl Lib {
    /// A library watching `Pictures` with `n` photos in `Pictures/Lisbon`, scanned.
    fn new(n: usize) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        write_photos(&dir.path().join("Pictures").join("Lisbon"), n);
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options()).unwrap();
        s.execute("folders.add", &json!({ "path": dir.path().join("Pictures") })).unwrap();
        s.wait_idle();
        Lib { dir, s }
    }

    fn lisbon(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Lisbon")
    }

    fn photo(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap().id
    }

    fn name_of(&self, id: PhotoId) -> String {
        self.s.library().photo(id).unwrap().name.clone()
    }

    /// Items renaming `IMG_000n.jpg` to `Tram-n.jpg` for each n.
    fn items(&self, ns: &[usize]) -> Vec<Item> {
        ns.iter()
            .map(|n| {
                let from = self.lisbon().join(format!("IMG_{n:04}.jpg"));
                Item {
                    photo: self.photo(&format!("IMG_{n:04}.jpg")),
                    from,
                    to: self.lisbon().join(format!("Tram-{n}.jpg")),
                    fail: false,
                    skip: false,
                }
            })
            .collect()
    }

    fn wait(&mut self) {
        assert!(self.s.wait_file_jobs(PATIENCE), "the file jobs ended");
    }

    fn last_event(&self) -> dalica_engine::FileOpEvent {
        self.s.file_op_events().last().cloned().unwrap()
    }
}

/// It runs as a job and applies once as one undo step; undo runs its reverse as a job and redo runs
/// it again, the model and the files following exactly; the journal holds each operation before
/// its first item runs and is empty once it is applied; the owner thread makes no `fs` call.
#[test]
fn an_operation_is_one_undo_step_whose_undo_and_redo_run_as_jobs() {
    let _faults = shared();
    let mut l = Lib::new(3);
    let (one, two) = (l.photo("IMG_0001.jpg"), l.photo("IMG_0002.jpg"));
    l.s.execute("photo.star.set", &json!({ "ids": [two], "on": true })).unwrap();
    let journal = Arc::new(MemoryJournal::default());
    l.s.set_file_op_journal(journal.clone());
    let before = l.s.library().model().clone();
    let steps = l.s.undo_stack().len();
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();

    let mut driver = Rename::new(Gate::open());
    driver.journal = Some(journal.clone());
    let journaled = Arc::clone(&driver.journaled);
    let items = l.items(&[1, 2, 3]);
    let job = l.s.start_file_op(FileOp::new(driver, FileOpKind::Rename, "Rename", items).destination(l.lisbon()));
    let entry = l.s.job(job).unwrap();
    assert_eq!((entry.kind, entry.label.as_str()), (JobKind::Files, "Renaming 0 of 3"));
    assert_eq!(l.s.undo_stack().len(), steps, "no step until the operation has run");
    l.wait();
    assert_eq!(l.s.job(job).unwrap().state, JobState::Done);
    assert_eq!(names(&l.lisbon()), ["Tram-1.jpg", "Tram-2.jpg", "Tram-3.jpg"]);
    assert_eq!((l.name_of(one), l.name_of(two)), ("Tram-1.jpg".to_string(), "Tram-2.jpg".to_string()), "ids kept");
    assert!(l.s.library().photo(two).unwrap().star, "and their data");
    assert_eq!(l.s.undo_stack().len(), steps + 1, "one step");
    assert_eq!(l.s.undo_stack().undo_label(), Some("Rename"));
    assert_eq!(*journaled.lock().unwrap(), [true, true, true], "in the journal before each item ran");
    let done = l.s.library().model().clone();
    let e = l.last_event();
    assert_eq!((e.kind, e.direction, e.done, e.total, e.cancelled), (FileOpKind::Rename, Direction::Do, 3, 3, false));
    assert!(e.failures.is_empty() && e.skipped.is_empty());
    assert_eq!((e.job, e.destination.as_deref()), (job, Some(l.lisbon().as_path())));
    l.s.flush().unwrap();
    assert!(journal.entries().is_empty(), "the operation left the journal");

    // Undo: the reverse runs as a job; the model and the files are back.
    assert_eq!(l.s.execute("edit.undo", &json!({})).unwrap(), json!({ "undone": "Rename" }));
    let undo_job = l.s.jobs().into_iter().filter(|j| j.kind == JobKind::Files).map(|j| j.job).max().unwrap();
    assert!(undo_job > job);
    assert_eq!(l.s.job(undo_job).unwrap().label, "Undo Rename 0 of 3");
    l.wait();
    assert_eq!(names(&l.lisbon()), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(l.s.library().model(), &before, "the model as it was");
    assert_eq!(l.s.undo_stack().len(), steps);
    assert_eq!(l.s.undo_stack().redo_label(), Some("Rename"));
    let e = l.last_event();
    assert_eq!((e.direction, e.done, e.job), (Direction::Undo, 3, undo_job));

    // Redo: the operation again.
    assert_eq!(l.s.execute("edit.redo", &json!({})).unwrap(), json!({ "redone": "Rename" }));
    l.wait();
    assert_eq!(names(&l.lisbon()), ["Tram-1.jpg", "Tram-2.jpg", "Tram-3.jpg"]);
    assert_eq!(l.s.library().model(), &done);
    assert_eq!((l.s.undo_stack().len(), l.s.undo_stack().redo_len()), (steps + 1, 0));
    assert_eq!(l.last_event().direction, Direction::Redo);
    // And undo once more.
    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    assert_eq!(l.s.library().model(), &before);
    l.s.flush().unwrap();
    assert!(journal.entries().is_empty());
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    drop(ui);
}

/// The step stays on the stack while its undo runs, and Undo says it waits; a new command meanwhile
/// leaves no redo step.
#[test]
fn the_step_stays_on_the_stack_while_its_undo_runs() {
    let _faults = shared();
    let mut l = Lib::new(2);
    let gate = Gate::open();
    let items = l.items(&[1, 2]);
    l.s.start_file_op(FileOp::new(Rename::new(gate.clone()), FileOpKind::Rename, "Rename", items));
    l.wait();
    let steps = l.s.undo_stack().len();
    gate.hold_next();
    l.s.execute("edit.undo", &json!({})).unwrap();
    gate.wait_arrived(3);
    l.s.poll();
    assert_eq!(l.s.undo_stack().len(), steps, "still on the stack");
    assert_eq!(l.s.undo_stack().undo_label(), Some("Rename"));
    assert!(l.s.undo_stack().undo_running().is_some());
    let waiting = "Undo Rename is still running.".to_string();
    assert_eq!(l.s.undo_stack().undo_blocked(), Some(waiting.clone()));
    assert_eq!(l.s.execute("edit.undo", &json!({})), Err(CmdError::Disabled(waiting)));
    // A new command while it runs: the undo still ends, but leaves nothing to redo.
    let star = l.photo("Tram-1.jpg");
    l.s.execute("photo.star.set", &json!({ "ids": [star], "on": true })).unwrap();
    gate.allow(usize::MAX);
    l.wait();
    assert_eq!(names(&l.lisbon()), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    assert_eq!(l.s.undo_stack().redo_len(), 0);
    assert_eq!(l.s.undo_stack().undo_label(), Some("Set Star"), "the rename step left the stack");
    assert!(l.s.undo_stack().undo_blocked().is_none());
}

/// Cancel between files keeps what finished as one step, which undoes alone.
#[test]
fn cancel_between_files_keeps_what_finished_as_one_step() {
    let _faults = shared();
    let mut l = Lib::new(3);
    let steps = l.s.undo_stack().len();
    let gate = Gate::shut();
    let items = l.items(&[1, 2, 3]);
    let job = l.s.start_file_op(FileOp::new(Rename::new(gate.clone()), FileOpKind::Rename, "Rename", items));
    gate.wait_arrived(1);
    assert_eq!(l.s.cancel_jobs(Some(job)).unwrap(), [job]);
    gate.allow(usize::MAX);
    l.wait();
    assert_eq!(l.s.job(job).unwrap().state, JobState::Cancelled);
    assert_eq!(gate.arrived(), 1, "no item started after Cancel");
    assert_eq!(names(&l.lisbon()), ["IMG_0002.jpg", "IMG_0003.jpg", "Tram-1.jpg"]);
    assert_eq!(l.s.undo_stack().len(), steps + 1);
    let e = l.last_event();
    assert_eq!((e.done, e.total, e.cancelled), (1, 3, true));
    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    assert_eq!(names(&l.lisbon()), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"]);
    assert_eq!(l.last_event().done, 1);
    assert_eq!(l.s.undo_stack().len(), steps);
}

/// A failing item is reported with its file and reason while the rest go on; a skipped one is
/// reported as skipped; only the items done are in the step.
#[test]
fn a_failure_is_reported_with_its_reason_while_the_rest_go_on() {
    let _faults = shared();
    let mut l = Lib::new(4);
    let mut items = l.items(&[1, 2, 3, 4]);
    items[1].fail = true;
    items[2].skip = true;
    let failing = items[1].from.clone();
    let job = l.s.start_file_op(FileOp::new(Rename::new(Gate::open()), FileOpKind::Rename, "Rename", items));
    l.wait();
    assert_eq!(names(&l.lisbon()), ["IMG_0002.jpg", "IMG_0003.jpg", "Tram-1.jpg", "Tram-4.jpg"]);
    let e = l.last_event();
    assert_eq!((e.done, e.total), (2, 4));
    assert_eq!(e.failures.len(), 1);
    assert_eq!((e.failures[0].file.as_path(), e.failures[0].reason.as_str()), (failing.as_path(), "a planted failure"));
    assert_eq!(e.skipped.len(), 1);
    assert_eq!(e.skipped[0].reason, "a file with that name is already there");
    let result = l.s.job(job).unwrap().result.unwrap();
    assert_eq!(result["failed"][0]["reason"], "a planted failure");
    assert!(l.s.job(job).unwrap().label.ends_with("1 skipped"), "{}", l.s.job(job).unwrap().label);
    l.s.execute("edit.undo", &json!({})).unwrap();
    l.wait();
    assert_eq!(names(&l.lisbon()), ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg", "IMG_0004.jpg"]);
    assert_eq!(l.last_event().done, 2);
}

/// A journal that cannot be written: nothing moves, every item fails with the reason, no step.
#[test]
fn an_operation_whose_journal_entry_cannot_be_written_moves_nothing() {
    let _faults = shared();
    struct Broken;
    impl Journal for Broken {
        fn begin(&self, _: &JournalEntry) -> Result<(), String> {
            Err("disk full".into())
        }
        fn item(&self, _: OpId, _: usize, _: ItemState) -> Result<(), String> {
            Ok(())
        }
        fn end(&self, _: OpId) -> Result<(), String> {
            Ok(())
        }
    }
    let mut l = Lib::new(2);
    l.s.set_file_op_journal(Arc::new(Broken));
    let steps = l.s.undo_stack().len();
    let gate = Gate::open();
    let items = l.items(&[1, 2]);
    l.s.start_file_op(FileOp::new(Rename::new(gate.clone()), FileOpKind::Rename, "Rename", items));
    l.wait();
    assert_eq!(gate.arrived(), 0, "no item ran");
    assert_eq!(names(&l.lisbon()), ["IMG_0001.jpg", "IMG_0002.jpg"]);
    let e = l.last_event();
    assert_eq!((e.done, e.failures.len()), (0, 2));
    assert!(e.failures[0].reason.contains("disk full"), "{}", e.failures[0].reason);
    assert_eq!(l.s.undo_stack().len(), steps);
}

/// A step with an item that has no way back says why, and Undo changes nothing.
#[test]
fn a_step_that_cannot_be_undone_says_why() {
    let _faults = shared();
    let mut l = Lib::new(1);
    let mut driver = Rename::new(Gate::open());
    driver.no_way_back = Some("Restore it from the Trash.");
    let items = l.items(&[1]);
    l.s.start_file_op(FileOp::new(driver, FileOpKind::Trash, "Delete", items));
    l.wait();
    assert_eq!(l.s.undo_stack().undo_label(), Some("Delete"));
    assert_eq!(l.s.undo_stack().undo_blocked().as_deref(), Some("Restore it from the Trash."));
    let model = l.s.library().model().clone();
    assert_eq!(l.s.execute("edit.undo", &json!({})), Err(CmdError::Disabled("Restore it from the Trash.".into())));
    assert!(!l.s.file_jobs_running());
    assert_eq!(l.s.library().model(), &model);
    assert_eq!(names(&l.lisbon()), ["Tram-1.jpg"]);
}

/// The watcher imports nothing for the paths an operation changed: the rescans wait while it runs,
/// and once it has run the folders hold what the model says, so the model, the ids and the undo
/// stack stay as the operation left them.
#[test]
fn the_watcher_imports_nothing_for_the_paths_dalica_changed() {
    let _faults = shared();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Pictures");
    write_photos(&root.join("Lisbon"), 3);
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000)));
    let mut s = Session::open_with(&dir.path().join("library"), clock.clone(), &options()).unwrap();
    s.set_watching(WatchMode::Polling);
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    let settle = |s: &mut Session| {
        for _ in 0..4 {
            clock.advance(Duration::from_secs(1));
            assert!(s.wait_watching(PATIENCE), "the watcher answered");
            s.wait_idle();
        }
    };
    settle(&mut s);
    let ui = dalica_fs::mark_ui_thread();
    let lisbon = root.join("Lisbon");
    let id = |s: &Session, name: &str| s.library().photos().find(|p| p.name == name).unwrap().id;
    let (one, two) = (id(&s, "IMG_0001.jpg"), id(&s, "IMG_0002.jpg"));
    s.execute("photo.star.set", &json!({ "ids": [two], "on": true })).unwrap();
    let reports = s.watch_reports();
    let gate = Gate::shut();
    let items = [(one, 1), (two, 2)]
        .iter()
        .map(|&(photo, n)| Item {
            photo,
            from: lisbon.join(format!("IMG_{n:04}.jpg")),
            to: lisbon.join(format!("Tram-{n}.jpg")),
            fail: false,
            skip: false,
        })
        .collect();
    s.start_file_op(FileOp::new(Rename::new(gate.clone()), FileOpKind::Rename, "Rename", items));
    // The first file renamed, the second held: the watcher ticks meanwhile.
    gate.allow(1);
    gate.wait_arrived(2);
    for _ in 0..3 {
        clock.advance(Duration::from_secs(1));
        assert!(s.wait_watching(PATIENCE));
        s.poll();
    }
    gate.allow(usize::MAX);
    assert!(s.wait_file_jobs(PATIENCE));
    let after = s.library().model().clone();
    let steps = s.undo_stack().len();
    settle(&mut s);
    assert!(s.watch_reports() > reports, "the watcher saw the renames");
    assert_eq!(s.library().model(), &after, "nothing imported");
    assert_eq!(s.undo_stack().len(), steps);
    assert_eq!(s.undo_stack().undo_label(), Some("Rename"));
    assert_eq!((id(&s, "Tram-1.jpg"), id(&s, "Tram-2.jpg")), (one, two));
    assert!(s.library().photo(two).unwrap().star);
    // Its undo too.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.wait_file_jobs(PATIENCE));
    let undone = s.library().model().clone();
    settle(&mut s);
    assert_eq!(s.library().model(), &undone);
    assert_eq!((id(&s, "IMG_0001.jpg"), id(&s, "IMG_0002.jpg")), (one, two));
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no fs call");
    drop(ui);
    assert!(exists(&lisbon.join("IMG_0002.jpg")));
}

/// `rebase_folders` moves the watched-folder entries at and under a folder (rules and unknown keys)
/// and keeps the others; `folders.json` follows.
#[test]
fn rebase_moves_nested_entries_and_keeps_the_others() {
    let _faults = shared();
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    let (lisbon, porto) = (pictures.join("Trips").join("Lisbon"), pictures.join("Porto"));
    write_photos(&lisbon, 1);
    write_photos(&porto, 1);
    let library = dir.path().join("library");
    let mut s = Session::open_with(&library, Arc::new(SystemClock), &options()).unwrap();
    s.execute("folders.add", &json!({ "path": pictures.join("Trips") })).unwrap();
    s.execute("folders.add", &json!({ "path": porto })).unwrap();
    s.execute("folders.set", &json!({ "changes": [{ "path": lisbon, "mode": "once" }] })).unwrap();
    s.wait_idle();
    let moved = pictures.join("Elsewhere").join("Trips");
    let ui = dalica_fs::mark_ui_thread();
    assert_eq!(s.rebase_folders(&pictures.join("Trips"), &moved), 2);
    assert_eq!(s.rebase_folders(&dir.path().join("nothing"), &moved), 0);
    drop(ui);
    let mut roots = s.roots().to_vec();
    roots.sort();
    assert_eq!(roots, [moved.clone(), moved.join("Lisbon"), porto.clone()]);
    s.flush().unwrap();
    let file: serde_json::Value = serde_json::from_slice(&std::fs::read(library.join("folders.json")).unwrap()).unwrap();
    let paths: Vec<PathBuf> = file["folders"].as_array().unwrap().iter().map(|e| PathBuf::from(e["path"].as_str().unwrap())).collect();
    assert!(paths.contains(&moved.join("Lisbon")) && paths.contains(&porto) && paths.contains(&moved));
    assert!(!paths.iter().any(|p| p.starts_with(pictures.join("Trips"))));
    let entry = file["folders"].as_array().unwrap().iter().find(|e| e["path"] == json!(moved.join("Lisbon"))).unwrap();
    assert_eq!(entry["mode"], "once", "the rule travels");
}
