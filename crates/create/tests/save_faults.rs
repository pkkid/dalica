//! A Save, a second Save, Undo Save and Revert interrupted at every step never lose the original,
//! and `save::recover` leaves the folder wholly saved or wholly original (`roadmap.md` M8.18;
//! `data-model.md` §6 and §10.5, `architecture.md` §8).
//!
//! Each operation is first run once with every `dalica-fs` fault point recording when it is
//! reached, which gives its steps in order (an atomic replace's points come once for the pending
//! record, once for the new file and once for the record marked done). Then, for every step and
//! both faults (an I/O error, a simulated crash that stops at once), the operation runs again on
//! a fresh folder with that one step failing, and:
//!
//! - right after, the original's bytes are at the photo's name, or in `.dalica-originals/` with
//!   an entry of the originals record naming their digest;
//! - after `recover` the folder is one of the operation's whole states (the original with no
//!   entry, or a saved file with its `done` entry and the original kept, byte for byte what an
//!   uninterrupted run writes), no temporary file is left, and a second `recover` finds nothing.
//!
//! Run with `cargo test -p dalica-create --test all save_faults::` (the dev-dependency turns the
//! fault points on for every test build; they are armed per thread, so each operation is its own
//! test and they run in parallel). Quick mode runs one case per class (an error before the
//! commit, an error after it, a crash); full mode (`DALICA_FULL_TESTS=1`) every step and fault.
//! The starting folders are made once per process and copied, with their modification times,
//! into each case's own folder.

use crate::common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::SystemTime;

use common::*;
use dalica_core::Timestamp;
use dalica_create::save::recover::{self, Outcome, Recovered};
use dalica_create::save::{self, Error, Options, Seen, digest};
use dalica_fs::TempDir;
use dalica_fs::faults::{self, Fault};
use dalica_meta::Changes;
use dalica_sidecar::originals::{Originals, State};

const FIRST: &[&str] = &["autofix()"];
const SECOND: &[&str] = &["tuning(0.25,0,0,0,none)"];
const TURN: &[&str] = &["rotate(90)"];

/// Options for a repair beginning now (temporary files written before it are removed).
fn repair_options() -> Options {
    Options { threads: 2, ..Options::new(Timestamp::from_system_time(SystemTime::now())) }
}

/// The photo: small, since only the steps matter here.
fn original() -> Vec<u8> {
    jpeg(11, 64, 48, 80, 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// The first Save of an unsaved photo.
    Save,
    /// A rotate-only first Save through the dispatch (the lossless path).
    Turn,
    /// A second Save of a photo saved with `FIRST`.
    ReSave,
    UndoSave,
    Revert,
}

impl Op {
    /// Whether the folder starts saved with `FIRST`.
    fn starts_saved(self) -> bool {
        matches!(self, Op::ReSave | Op::UndoSave | Op::Revert)
    }

    fn run(self, dir: &Path) -> Result<(), Error> {
        let none = Changes::default();
        match self {
            Op::Save => save::in_place(dir, NAME, &recipe(FIRST), &none, &options()).map(drop),
            Op::Turn => save::save(dir, NAME, &recipe(TURN), &none, &options()).map(drop),
            Op::ReSave => save::in_place(dir, NAME, &recipe(SECOND), &none, &options()).map(drop),
            Op::UndoSave => save::undo(dir, NAME, &options()).map(drop),
            Op::Revert => save::revert(dir, NAME, &options()).map(drop),
        }
    }
}

/// A folder ready for an operation: the photo, saved with `FIRST` when the operation needs it.
struct Fixture {
    tmp: TempDir,
    original: Vec<u8>,
    /// The file at the photo's name as the library last saw it, before the operation.
    seen: Seen,
}

impl Fixture {
    fn new(op: Op) -> Fixture {
        let tmp = TempDir::new("dalica-save-faults").unwrap();
        start(op).restore(tmp.path());
        let seen = seen(&tmp.path().join(NAME));
        Fixture { tmp, original: original(), seen }
    }

    fn dir(&self) -> &Path {
        self.tmp.path()
    }

    fn recover(&self) -> Recovered {
        let seen = BTreeMap::from([(NAME.to_string(), self.seen)]);
        recover::recover(self.dir(), &seen, &repair_options()).unwrap()
    }
}

/// A folder's files (relative path, bytes, modification time), to copy into fresh folders.
struct Snapshot(Vec<(PathBuf, Vec<u8>, SystemTime)>);

impl Snapshot {
    fn take(dir: &Path) -> Snapshot {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>, SystemTime)>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let path = e.path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    let modified = e.metadata().unwrap().modified().unwrap();
                    out.push((path.strip_prefix(root).unwrap().to_path_buf(), std::fs::read(&path).unwrap(), modified));
                }
            }
        }
        let mut files = Vec::new();
        walk(dir, dir, &mut files);
        Snapshot(files)
    }

    fn restore(&self, dir: &Path) {
        for (rel, bytes, modified) in &self.0 {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(*modified).unwrap();
        }
    }
}

/// The folder `op` starts from (the photo, saved with `FIRST` when the operation needs it), made
/// once per process. Never call it with a fault armed.
fn start(op: Op) -> &'static Snapshot {
    static UNSAVED: OnceLock<Snapshot> = OnceLock::new();
    static SAVED: OnceLock<Snapshot> = OnceLock::new();
    let make = |saved: bool| {
        let tmp = TempDir::new("dalica-save-faults").unwrap();
        std::fs::write(tmp.path().join(NAME), original()).unwrap();
        if saved {
            save::in_place(tmp.path(), NAME, &recipe(FIRST), &Changes::default(), &options()).unwrap();
        }
        Snapshot::take(tmp.path())
    };
    if op.starts_saved() { SAVED.get_or_init(|| make(true)) } else { UNSAVED.get_or_init(|| make(false)) }
}

/// What the folder holds for the photo: the file at its name, the kept original and the record's
/// entry (its state, recipe and digest).
#[derive(Clone, PartialEq)]
struct Disk {
    photo: Option<Vec<u8>>,
    kept: Option<Vec<u8>>,
    entry: Option<(State, Vec<String>, String)>,
}

impl Disk {
    fn read(dir: &Path) -> Disk {
        let kept_dir = dir.join(".dalica-originals");
        let entry = std::fs::read(kept_dir.join(".dalica.json")).ok().and_then(|bytes| {
            match Originals::parse(&bytes, &dalica_sidecar::Limits::default()).expect("the record stays readable").entry(NAME) {
                dalica_sidecar::Field::Value(e) => Some((e.state, e.recipe, e.digest)),
                dalica_sidecar::Field::Absent => None,
                other => panic!("an entry Dalica cannot read: {other:?}"),
            }
        });
        Disk { photo: std::fs::read(dir.join(NAME)).ok(), kept: std::fs::read(kept_dir.join(NAME)).ok(), entry }
    }
}

impl fmt::Debug for Disk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short = |b: &Option<Vec<u8>>| b.as_ref().map(|b| format!("{} bytes {}", b.len(), digest(b)));
        f.debug_struct("Disk").field("photo", &short(&self.photo)).field("kept", &short(&self.kept)).field("entry", &self.entry).finish()
    }
}

/// The whole states an operation may end in, from uninterrupted runs.
struct Whole {
    original: Disk,
    /// Saved with `FIRST` (or `TURN` for [`Op::Turn`]).
    saved: Disk,
    /// Saved with `FIRST` then `SECOND` ([`Op::ReSave`]).
    resaved: Option<Disk>,
}

impl Whole {
    /// The whole states of `op`, from uninterrupted runs made once per process.
    fn of(op: Op) -> Whole {
        static SAVED: OnceLock<Disk> = OnceLock::new();
        static TURNED: OnceLock<Disk> = OnceLock::new();
        static RESAVED: OnceLock<Disk> = OnceLock::new();
        let after = |start: Op, run: Op| {
            let f = Fixture::new(start);
            run.run(f.dir()).unwrap();
            Disk::read(f.dir())
        };
        let original = Disk { photo: Some(original()), kept: None, entry: None };
        let saved = if op == Op::Turn { TURNED.get_or_init(|| after(Op::Turn, Op::Turn)) } else { SAVED.get_or_init(|| after(Op::Save, Op::Save)) };
        let resaved = (op == Op::ReSave).then(|| RESAVED.get_or_init(|| after(Op::ReSave, Op::ReSave)).clone());
        Whole { original, saved: saved.clone(), resaved }
    }

    fn allows(&self, d: &Disk) -> bool {
        *d == self.original || *d == self.saved || self.resaved.as_ref() == Some(d)
    }
}

/// Every named fault point of `dalica-fs`.
fn all_points() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = faults::ATOMIC_POINTS
        .iter()
        .chain(&faults::BACKUP_POINTS)
        .chain(&faults::RESTORE_POINTS)
        .chain(&faults::NEW_FILE_POINTS)
        .chain(&[faults::points::NEW_NO_LINKS])
        .chain(&faults::NEW_FILE_DIRECT_POINTS)
        .chain(&faults::NEW_FOLDER_POINTS)
        .chain(&faults::APPEND_POINTS)
        .chain(&faults::STALE_POINTS)
        .copied()
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// Records `point` into `log` each time this thread reaches it.
fn record_at(point: &'static str, log: Rc<RefCell<Vec<&'static str>>>) {
    faults::run_at(point, move || {
        log.borrow_mut().push(point);
        record_at(point, log);
    });
}

/// The fault points `run` reaches, in order, each with how many times it was reached so far
/// (1 for its first time).
fn trace(run: impl FnOnce()) -> Vec<(&'static str, usize)> {
    let log = Rc::new(RefCell::new(Vec::new()));
    for p in all_points() {
        record_at(p, Rc::clone(&log));
    }
    run();
    faults::disarm_all();
    let seq = log.borrow().clone();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    seq.into_iter()
        .map(|p| {
            let n = counts.entry(p).or_default();
            *n += 1;
            (p, *n)
        })
        .collect()
}

/// Arms `point` to fail with `fault` the `nth` time this thread reaches it (1 for the next).
fn arm_nth(point: &'static str, nth: usize, fault: Fault) {
    fn wait(point: &'static str, left: usize, fault: Fault) {
        if left == 0 {
            faults::arm(point, fault);
        } else {
            faults::run_at(point, move || wait(point, left - 1, fault));
        }
    }
    if nth <= 1 {
        faults::arm(point, fault);
    } else {
        faults::run_at(point, move || wait(point, nth - 1, fault));
    }
}

/// The temporary files left in the folder and in its `.dalica-originals/`.
fn temporaries(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in [dir.to_path_buf(), dir.join(".dalica-originals")] {
        if let Ok(list) = std::fs::read_dir(&d) {
            out.extend(list.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains(".dalica-tmp-")));
        }
    }
    out
}

/// Right after an interruption: the original's bytes are at the photo's name, or kept with an
/// entry of the record naming their digest.
fn original_is_safe(dir: &Path, original: &[u8]) -> bool {
    let d = Disk::read(dir);
    d.photo.as_deref() == Some(original) || (d.kept.as_deref() == Some(original) && d.entry.is_some_and(|(_, _, dg)| dg == digest(original)))
}

/// The cases to run for `steps`: in full mode every step with both faults; in quick mode one per
/// class: an error at the first step (before the commit), a crash halfway, an error at the last
/// step (after the commit).
fn sample(steps: &[(&'static str, usize)]) -> Vec<(&'static str, usize, Fault)> {
    if full() {
        return steps.iter().flat_map(|&(point, nth)| [(point, nth, Fault::Error), (point, nth, Fault::Crash)]).collect();
    }
    let at = |i: usize, fault| (steps[i].0, steps[i].1, fault);
    vec![at(0, Fault::Error), at(steps.len() / 2, Fault::Crash), at(steps.len() - 1, Fault::Error)]
}

/// `op` interrupted at each step (see `sample`) keeps the original, and `recover` makes the
/// folder whole.
fn every_step_interrupted(op: Op) {
    let whole = Whole::of(op);
    let steps = {
        let f = Fixture::new(op);
        trace(|| op.run(f.dir()).unwrap())
    };
    assert!(steps.len() >= 7, "{op:?} reaches its steps: {steps:?}");
    for (point, nth, fault) in sample(&steps) {
        let case = format!("{op:?} with {fault:?} at {point} #{nth}");
        let f = Fixture::new(op);
        arm_nth(point, nth, fault);
        let result = op.run(f.dir());
        faults::disarm_all();
        if fault == Fault::Crash {
            assert!(matches!(result, Err(Error::Fs(dalica_fs::Error::Crashed(_)))), "{case}: {result:?}");
        }
        assert!(original_is_safe(f.dir(), &f.original), "{case}: the original is lost: {:?}", Disk::read(f.dir()));

        let repaired = f.recover();
        let after = Disk::read(f.dir());
        assert!(whole.allows(&after), "{case}: not whole after recover ({repaired:?}): {after:?}");
        assert!(repaired.repairs.iter().all(|r| !matches!(r.outcome, Outcome::Left(_))), "{case}: {repaired:?}");
        assert!(temporaries(f.dir()).is_empty(), "{case}: temporary files left: {:?}", temporaries(f.dir()));
        assert!(repaired.problems.is_empty(), "{case}: {:?}", repaired.problems);

        let again = f.recover();
        assert!(again.repairs.is_empty() && again.removed.is_empty(), "{case}: a second recover found {again:?}");
        assert_eq!(Disk::read(f.dir()), after, "{case}");
    }
}

/// One test per operation, so they run in parallel (the fault points are armed per thread).
mod every_step_interrupted_keeps_the_original_and_recover_makes_the_folder_whole {
    use super::{Op, every_step_interrupted};

    #[test]
    fn save() {
        every_step_interrupted(Op::Save);
    }

    #[test]
    fn turn() {
        every_step_interrupted(Op::Turn);
    }

    #[test]
    fn resave() {
        every_step_interrupted(Op::ReSave);
    }

    #[test]
    fn undo_save() {
        every_step_interrupted(Op::UndoSave);
    }

    #[test]
    fn revert() {
        every_step_interrupted(Op::Revert);
    }
}

/// Which way the repair goes, step by step, for a first Save: back while the new file is not in
/// place, forward once it is.
#[test]
fn a_first_save_rolls_back_before_its_commit_and_forward_after_it() {
    let whole = Whole::of(Op::Save);
    let f = Fixture::new(Op::Save);
    let steps = trace(|| Op::Save.run(f.dir()).unwrap());
    // The atomic replace's rename: the first is the pending record, the second the new file, the
    // third the record marked done.
    let rename = dalica_fs::faults::points::RENAME;
    let renames: Vec<usize> = steps.iter().filter(|(p, _)| *p == rename).map(|(_, n)| *n).collect();
    assert_eq!(renames, [1, 2, 3], "{steps:?}");
    let backup_move = dalica_fs::faults::points::BACKUP_FSYNC_DIR;

    for (point, nth, forward) in [(backup_move, 1, false), (rename, 2, false), (rename, 3, true)] {
        let f = Fixture::new(Op::Save);
        arm_nth(point, nth, Fault::Crash);
        assert!(Op::Save.run(f.dir()).is_err());
        faults::disarm_all();
        assert_eq!(Disk::read(f.dir()).entry.map(|e| e.0), Some(State::Pending), "{point} #{nth}");
        let repaired = f.recover();
        let r = &repaired.repairs[0];
        assert_eq!(r.entry.state, State::Pending);
        assert_eq!(r.entry.recipe, FIRST);
        if forward {
            assert_eq!(r.outcome, Outcome::RolledForward, "{point} #{nth}");
            assert_eq!(Disk::read(f.dir()), whole.saved);
        } else {
            assert_eq!(r.outcome, Outcome::RolledBack, "{point} #{nth}");
            assert_eq!(Disk::read(f.dir()), whole.original);
        }
        assert!(r.identity.is_some());
    }
}

/// Without the library's last view of the file, the content decides: a first Save's committed
/// file is rolled forward; a second Save's old saved file (stopped before its commit) does not
/// hold the whole recipe and is rolled back; the committed new one is rolled forward.
#[test]
fn without_the_last_seen_file_the_content_decides() {
    let rename = dalica_fs::faults::points::RENAME;
    let none = BTreeMap::new();
    let whole = Whole::of(Op::ReSave);
    for (op, nth, want) in [(Op::Save, 3, &whole.saved), (Op::ReSave, 2, &whole.original), (Op::ReSave, 3, whole.resaved.as_ref().unwrap())] {
        let f = Fixture::new(op);
        arm_nth(rename, nth, Fault::Crash);
        assert!(op.run(f.dir()).is_err());
        faults::disarm_all();
        let repaired = recover::recover(f.dir(), &none, &repair_options()).unwrap();
        assert_eq!(repaired.repairs.len(), 1, "{op:?} #{nth}: {repaired:?}");
        assert_eq!(&Disk::read(f.dir()), want, "{op:?} #{nth}: {repaired:?}");
    }
}

/// An interrupted repair is repaired by the next one: every step of `recover` failing or crashing
/// (quick mode: one per class, see `sample`), for a first Save stopped after its new file was
/// committed and before it.
#[test]
fn a_repair_interrupted_at_any_step_is_finished_by_the_next() {
    let rename = dalica_fs::faults::points::RENAME;
    let whole = Whole::of(Op::Save);
    for nth in [2, 3] {
        let plant = || {
            let f = Fixture::new(Op::Save);
            arm_nth(rename, nth, Fault::Crash);
            assert!(Op::Save.run(f.dir()).is_err());
            faults::disarm_all();
            f
        };
        let f = plant();
        let steps = trace(|| {
            f.recover();
        });
        assert!(!steps.is_empty(), "the repair writes");
        for (point, n, fault) in sample(&steps) {
            let f = plant();
            arm_nth(point, n, fault);
            let seen = BTreeMap::from([(NAME.to_string(), f.seen)]);
            let _ = recover::recover(f.dir(), &seen, &repair_options());
            faults::disarm_all();
            assert!(
                original_is_safe(f.dir(), &f.original),
                "{fault:?} at {point} #{n} after a Save stopped at rename #{nth}: {:?}",
                Disk::read(f.dir())
            );
            f.recover();
            let after = Disk::read(f.dir());
            assert!(whole.allows(&after), "{fault:?} at {point} #{n} of the repair: {after:?}");
            assert!(temporaries(f.dir()).is_empty(), "{fault:?} at {point} #{n}");
        }
    }
}

/// What the repair leaves alone: a kept original that is not the recorded one, and a pending
/// entry whose original is nowhere; nothing is moved or dropped.
#[test]
fn a_repair_without_the_recorded_original_leaves_everything_as_found() {
    let backup_fsync = dalica_fs::faults::points::BACKUP_FSYNC_DIR;
    let f = Fixture::new(Op::Save);
    arm_nth(backup_fsync, 1, Fault::Crash);
    assert!(Op::Save.run(f.dir()).is_err());
    faults::disarm_all();
    let kept = f.dir().join(".dalica-originals").join(NAME);
    std::fs::write(&kept, b"not the original").unwrap();
    let before = Disk::read(f.dir());
    let repaired = f.recover();
    assert!(matches!(repaired.repairs[0].outcome, Outcome::Left(_)), "{repaired:?}");
    assert_eq!(Disk::read(f.dir()), before);

    std::fs::remove_file(&kept).unwrap();
    let before = Disk::read(f.dir());
    let repaired = f.recover();
    assert!(matches!(&repaired.repairs[0].outcome, Outcome::Left(why) if why.contains("neither")), "{repaired:?}");
    assert_eq!(Disk::read(f.dir()), before);
}

/// `check` reads only and says when a folder needs the repair: a pending entry, an Undo Save that
/// stopped after moving the original back, a temporary file of the record; not a saved folder.
#[test]
fn check_finds_what_needs_the_repair_and_nothing_else() {
    let f = Fixture::new(Op::Save);
    assert!(!recover::check(f.dir()).unwrap(), "never saved");
    let f = Fixture::new(Op::UndoSave);
    assert!(!recover::check(f.dir()).unwrap(), "saved");

    // Undo Save stopped after the move, before the entry was dropped.
    arm_nth(dalica_fs::faults::points::RENAME, 1, Fault::Crash);
    assert!(Op::UndoSave.run(f.dir()).is_err());
    faults::disarm_all();
    assert!(recover::check(f.dir()).unwrap());
    let repaired = f.recover();
    assert_eq!(repaired.repairs[0].outcome, Outcome::RolledBack);
    assert_eq!(repaired.repairs[0].entry.state, State::Done);
    assert!(!recover::check(f.dir()).unwrap());

    let f = Fixture::new(Op::ReSave);
    arm_nth(dalica_fs::faults::points::WRITE, 1, Fault::Crash);
    assert!(Op::ReSave.run(f.dir()).is_err());
    faults::disarm_all();
    assert!(recover::check(f.dir()).unwrap(), "a torn temporary record");
    let repaired = f.recover();
    assert!(repaired.repairs.is_empty());
    assert_eq!(repaired.removed.len(), 1, "{repaired:?}");
    assert!(!recover::check(f.dir()).unwrap());
}

/// A temporary file written after the repair began belongs to a write still running: it stays.
#[test]
fn a_temporary_file_newer_than_the_repair_stays() {
    let f = Fixture::new(Op::Save);
    arm_nth(dalica_fs::faults::points::WRITE, 2, Fault::Crash);
    assert!(Op::Save.run(f.dir()).is_err());
    faults::disarm_all();
    let left = temporaries(f.dir());
    assert_eq!(left.len(), 1, "the new file's torn temporary");
    let early = Options { threads: 2, ..Options::new(Timestamp::from_millis(0)) };
    let seen = BTreeMap::from([(NAME.to_string(), f.seen)]);
    let repaired = recover::recover(f.dir(), &seen, &early).unwrap();
    assert_eq!(repaired.repairs.len(), 1, "the Save is still repaired");
    assert!(repaired.removed.is_empty());
    assert_eq!(temporaries(f.dir()), left);
}
