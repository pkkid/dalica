//! File operations (`architecture.md` §4–§5, `data-model.md` §8): moving, copying, renaming and
//! trashing real files, each one library undo step whose undo moves the files back on a worker.
//!
//! - **Driving.** A command module describes its operation with a [`Driver`]: the items (each owns
//!   what its work needs and names the file it is about and where it goes), the work for one item
//!   (on a worker thread, through `fs`; a clash may skip it), the library ops that follow a finished
//!   item (on the owner thread), and the item that reverses it, or why it cannot be reversed
//!   ("Restore it from the Trash"). [`Session::start_file_op`] runs a [`FileOp`] as a file job of
//!   kind [`JobKind::Files`] on M8.3's runner (`filejobs.rs`), one item after another on one thread:
//!   progress, Cancel between items, each item's failure (or panic) its own while the rest go on.
//! - **The journal.** Before the first item moves, the operation is entered in the file-operation
//!   journal with every item `pending` ([`Journal`]); each item is marked `done` once its work
//!   finished, and the operation leaves the journal when its results are in the library (and the
//!   index journal holds them). Every journal call goes through the session's I/O worker, the first
//!   one waited for by the file job's thread, so nothing moves before the entry is written. The
//!   session's journal is `file-ops.json` in the library directory ([`StoreJournal`], `dalica-store`'s
//!   `intents`), opened with the library: its problems become notices, the operation ids continue
//!   past those an earlier session left ([`Session::file_ops_left`], which M9.21 repairs), and a
//!   journal that cannot be opened refuses every operation ([`NoJournal`]). Tests may plug in
//!   [`MemoryJournal`] ([`Session::set_file_op_journal`]).
//! - **Entries travel with their files** (`data-model.md` §8). A driver whose items move, copy or
//!   rename photos says which with [`Driver::carry`] (the photos of one item, by name, from one
//!   folder to another; `None` for a trash or a folder move, whose sidecar is inside it). For each
//!   item done this module then, in order: carries the photos' originals backups and their record
//!   entries on the job's thread right after the item's work ([`dalica_store::carry::backups`]);
//!   applies the item's library ops (their folders' new states go to the I/O worker); and carries
//!   the sidecar entries on the I/O worker after those states ([`Session::carry_entries`],
//!   `SidecarWriter::carry`: the folders' pending writes first, then the entries, then the
//!   destination from the model). Undo and redo carry the same way, with the item that ran. A
//!   carry that fails or leaves an entry behind is reported by [`Session::flush`] like an unsaved
//!   write; a backup that could not travel is logged. A copied photo needs the source's data in
//!   the model too (its ops), or the destination's next write clears the duplicated values.
//! - **Dalica's own changes.** File jobs put off the watcher's rescans while they run (M8.3), and
//!   each finished item's ops put both of its ends in the model (the source gone, the target with
//!   the file's facts) before any rescan reads the folders, so the rescan after the operation finds
//!   nothing to import.
//! - **The undo step.** Each finished item's ops are applied as one batch (all or nothing) as a
//!   command's change, its inverse kept with the item. When the job ends, the items that finished
//!   become one [`FileStep`] on the library undo stack (Cancel keeps what finished as that one
//!   step; nothing finished, no step). Undo runs the reverse items (last first) as a file job in
//!   turn ("Undo Move"); the step stays on the stack while it runs and Undo says why it waits. Each
//!   reversed item applies its kept inverse; the items reversed go to the redo stack as one step and
//!   any it could not reverse stay on the undo stack. Redo runs the items as they were done again
//!   (the name a clash gave included) and applies the ops undo kept. A step with an item that has
//!   no way back cannot be undone and says why
//!   ([`UndoStack::undo_blocked`](crate::undo::UndoStack::undo_blocked)).
//! - **Events.** Each finished operation, undo and redo leaves a [`FileOpEvent`] (its kind and
//!   direction, how many items were done, those skipped and those that failed with file and reason,
//!   the destination) for the UI's notices ([`Session::file_op_events`]).

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use dalica_library::Op;
use dalica_store::carry::Carry;
use dalica_store::intents::{self, Intent, Intents};
use serde::Serialize;

use crate::error::CmdError;
use crate::filejobs::{Applied, Done, FileItem, FileJob, ItemFailure, Notice, Tally, default_message};
use crate::jobs::{JobId, JobKind};
use crate::session::{Msg, Session};
use crate::undo::FileStep;

/// The most finished-operation events kept ([`Session::file_op_events`]); the oldest go first.
pub const MAX_EVENTS: usize = 64;

/// A file operation's number in its session (from 1); also its journal id.
pub type OpId = u64;

/// What a file operation does to the files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileOpKind {
    Move,
    Copy,
    Rename,
    Trash,
    MoveFolder,
    RenameFolder,
    TrashFolder,
}

impl FileOpKind {
    /// The progress label: "Moving 3 of 10".
    pub fn progress(self) -> &'static str {
        match self {
            FileOpKind::Move => "Moving",
            FileOpKind::Copy => "Copying",
            FileOpKind::Rename => "Renaming",
            FileOpKind::Trash => "Moving to the Trash",
            FileOpKind::MoveFolder => "Moving folders",
            FileOpKind::RenameFolder => "Renaming folders",
            FileOpKind::TrashFolder => "Moving folders to the Trash",
        }
    }
}

/// Which way an operation runs: done the first time, undone, or done again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Do,
    Undo,
    Redo,
}

/// What one item's work did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ran<D> {
    /// Done; what the work found (facts read back, the name a clash gave, the trash entry).
    Done(D),
    /// Left alone on purpose (Skip Duplicates), with the reason the notice gives.
    Skipped(String),
}

/// One kind of file operation (a command module's: moving photos, renaming, trashing, …).
pub trait Driver: Send + Sync + 'static {
    /// One item: owns everything its work needs.
    type Item: Clone + fmt::Debug + Send + Sync + 'static;
    /// What an item's work gives back.
    type Done: Send + 'static;

    /// The file or folder the item is about (progress, failures, the journal's `from`).
    fn source<'a>(&self, item: &'a Self::Item) -> &'a Path;
    /// Where it goes, if anywhere (the journal's `to`).
    fn target<'a>(&self, item: &'a Self::Item) -> Option<&'a Path>;
    /// Does the item, on a worker thread (through `fs`; never replacing a file).
    fn run(&self, item: &Self::Item) -> Result<Ran<Self::Done>, String>;
    /// The library ops a first run of `item` brings (on the owner thread; no file access), applied
    /// as one batch. Undo and redo apply the inverses kept instead.
    fn ops(&self, s: &Session, item: &Self::Item, done: &Self::Done) -> Result<Vec<Op>, String>;
    /// The item that reverses `item` as done, or why it cannot be reversed.
    fn reverse(&self, item: &Self::Item, done: &Self::Done) -> Result<Self::Item, String>;
    /// `item` as it was done, which redo runs again (the name a clash gave): `item` by default.
    fn settled(&self, item: &Self::Item, done: &Self::Done) -> Self::Item {
        let _ = done;
        item.clone()
    }
    /// The photos whose sidecar entries and originals backups travel with `item` as done (`done`
    /// gives the name a clash chose): `Carry::one(Mode::Move, from_folder, name, to_folder,
    /// new_name)` for a move, `Mode::Copy` for a copy, `Carry::rename` for a rename; several names
    /// for a folder merge. Called on the job's thread for every item done in any direction (undo
    /// passes its reverse item); see the module docs for what follows. `None` (the default): the
    /// item carries nothing.
    fn carry(&self, item: &Self::Item, done: &Self::Done) -> Option<Carry> {
        let _ = (item, done);
        None
    }
}

/// A file operation to start: its driver, kind, undo label ("Move": "Undo Move"), items and the
/// folder the notice names.
pub struct FileOp<D: Driver> {
    driver: Arc<D>,
    kind: FileOpKind,
    label: &'static str,
    items: Vec<D::Item>,
    destination: Option<PathBuf>,
}

impl<D: Driver> FileOp<D> {
    pub fn new(driver: D, kind: FileOpKind, label: &'static str, items: Vec<D::Item>) -> Self {
        FileOp { driver: Arc::new(driver), kind, label, items, destination: None }
    }

    /// The folder the operation's notice names ("Moved 3 photos to Lisbon").
    pub fn destination(mut self, folder: PathBuf) -> Self {
        self.destination = Some(folder);
        self
    }
}

/// An item's state in the file-operation journal (`data-model.md` §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    Pending,
    /// A cross-volume move whose copy is in place and verified while its source is not yet in the
    /// trash.
    Placed,
    Done,
}

/// One item as the journal records it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JournalItem {
    pub from: PathBuf,
    pub to: Option<PathBuf>,
    pub state: ItemState,
}

/// One operation as the journal records it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JournalEntry {
    pub id: OpId,
    pub kind: FileOpKind,
    pub direction: Direction,
    pub items: Vec<JournalItem>,
}

/// The file-operation journal's writer (`file-ops.json`, M9.9). Called on the session's I/O
/// worker, never on the owner thread.
pub trait Journal: Send + Sync + 'static {
    /// Enters an operation, every item `pending`, before anything moves.
    fn begin(&self, entry: &JournalEntry) -> Result<(), String>;
    /// Item `index` of operation `op` reached `state`.
    fn item(&self, op: OpId, index: usize, state: ItemState) -> Result<(), String>;
    /// Operation `op` is over and its results are in the library: it leaves the journal.
    fn end(&self, op: OpId) -> Result<(), String>;
}

/// The stub journal: the entries in memory (until M9.9 writes `file-ops.json`).
#[derive(Debug, Default)]
pub struct MemoryJournal {
    entries: Mutex<BTreeMap<OpId, JournalEntry>>,
}

impl MemoryJournal {
    /// The operations entered and not ended, by id.
    pub fn entries(&self) -> Vec<JournalEntry> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).values().cloned().collect()
    }
}

impl Journal for MemoryJournal {
    fn begin(&self, entry: &JournalEntry) -> Result<(), String> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).insert(entry.id, entry.clone());
        Ok(())
    }

    fn item(&self, op: OpId, index: usize, state: ItemState) -> Result<(), String> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let item = entries.get_mut(&op).and_then(|e| e.items.get_mut(index)).ok_or_else(|| format!("no item {index} of operation {op}"))?;
        item.state = state;
        Ok(())
    }

    fn end(&self, op: OpId) -> Result<(), String> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).remove(&op);
        Ok(())
    }
}

/// The journal `file-ops.json` in the library directory (`dalica-store`'s [`Intents`]): what the
/// session uses once the library is open.
#[derive(Debug)]
pub struct StoreJournal {
    intents: Mutex<Intents>,
}

impl StoreJournal {
    pub fn new(intents: Intents) -> StoreJournal {
        StoreJournal { intents: Mutex::new(intents) }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Intents) -> Result<T, intents::Error>) -> Result<T, String> {
        f(&mut self.intents.lock().unwrap_or_else(PoisonError::into_inner)).map_err(|e| e.to_string())
    }
}

fn store_kind(kind: FileOpKind) -> intents::Kind {
    match kind {
        FileOpKind::Move => intents::Kind::Move,
        FileOpKind::Copy => intents::Kind::Copy,
        FileOpKind::Rename => intents::Kind::Rename,
        FileOpKind::Trash => intents::Kind::Trash,
        FileOpKind::MoveFolder => intents::Kind::MoveFolder,
        FileOpKind::RenameFolder => intents::Kind::RenameFolder,
        FileOpKind::TrashFolder => intents::Kind::TrashFolder,
    }
}

fn store_state(state: ItemState) -> intents::State {
    match state {
        ItemState::Pending => intents::State::Pending,
        ItemState::Placed => intents::State::Placed,
        ItemState::Done => intents::State::Done,
    }
}

impl Journal for StoreJournal {
    fn begin(&self, entry: &JournalEntry) -> Result<(), String> {
        let intent = Intent {
            id: entry.id,
            kind: store_kind(entry.kind),
            direction: match entry.direction {
                Direction::Do => intents::Direction::Do,
                Direction::Undo => intents::Direction::Undo,
                Direction::Redo => intents::Direction::Redo,
            },
            items: entry.items.iter().map(|i| intents::Item { from: i.from.clone(), to: i.to.clone(), state: store_state(i.state) }).collect(),
        };
        self.with(|j| j.begin(intent))
    }

    fn item(&self, op: OpId, index: usize, state: ItemState) -> Result<(), String> {
        self.with(|j| j.item(op, index, store_state(state)))
    }

    fn end(&self, op: OpId) -> Result<(), String> {
        self.with(|j| j.end(op))
    }
}

/// The journal of a library whose `file-ops.json` could not be opened: every operation is refused
/// with the reason, so nothing moves without a journal.
#[derive(Debug)]
pub struct NoJournal(pub String);

impl Journal for NoJournal {
    fn begin(&self, _: &JournalEntry) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn item(&self, _: OpId, _: usize, _: ItemState) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn end(&self, _: OpId) -> Result<(), String> {
        Err(self.0.clone())
    }
}

/// A finished file operation, undo or redo, for the UI's notices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileOpEvent {
    /// Rises by one per event in this session: the UI shows those it has not seen.
    pub seq: u64,
    pub op: OpId,
    pub job: JobId,
    pub kind: FileOpKind,
    pub direction: Direction,
    /// The undo label ("Move").
    pub label: &'static str,
    pub total: usize,
    /// Items done (moved, copied, renamed, trashed; or reversed).
    pub done: usize,
    /// Items left alone on purpose, with the reason.
    pub skipped: Vec<ItemFailure>,
    /// Items that failed, with the file and the reason.
    pub failures: Vec<ItemFailure>,
    /// Cancel left items unstarted.
    pub cancelled: bool,
    /// The folder the operation's notice names.
    pub destination: Option<PathBuf>,
}

/// The session's file operations: the journal writer and the finished operations' events.
pub(crate) struct FileOps {
    next: OpId,
    journal: Arc<dyn Journal>,
    events: VecDeque<FileOpEvent>,
    next_seq: u64,
    /// The operations an earlier session left in the journal (for M9.21's repair).
    left: Vec<Intent>,
}

impl Default for FileOps {
    fn default() -> Self {
        FileOps { next: 1, journal: Arc::new(MemoryJournal::default()), events: VecDeque::new(), next_seq: 0, left: Vec::new() }
    }
}

/// One item with the library ops undoing (or redoing) it: what a [`FileStep`] holds per item.
#[derive(Clone, Debug)]
struct Entry<I> {
    /// The item as done (redo runs it again).
    forward: I,
    /// The item that reverses it, or why there is none.
    back: Result<I, String>,
    /// The batch to apply when the item next runs the other way (the inverse of what its last run
    /// applied); `None` when that run changed nothing in the library.
    op: Option<Op>,
}

/// A file step's items (or a running operation's) with their driver.
struct Steps<D: Driver> {
    driver: Arc<D>,
    kind: FileOpKind,
    label: &'static str,
    destination: Option<PathBuf>,
    entries: Vec<Entry<D::Item>>,
}

impl<D: Driver> Clone for Steps<D> {
    fn clone(&self) -> Self {
        Steps {
            driver: Arc::clone(&self.driver),
            kind: self.kind,
            label: self.label,
            destination: self.destination.clone(),
            entries: self.entries.clone(),
        }
    }
}

/// A file step's items whatever their driver, as the undo stack keeps them.
pub(crate) trait Batch: Send + Sync {
    /// How many items.
    fn len(&self) -> usize;
    /// Why the step cannot run in `dir` (undoing: an item without a way back).
    fn blocked(&self, dir: Direction) -> Option<String>;
    /// Starts the step's undo or redo as a file job; `since` is the undo stack's count of new
    /// steps now.
    fn start(&self, s: &mut Session, step: u64, dir: Direction, since: u64) -> JobId;
}

impl<D: Driver> Batch for Steps<D> {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn blocked(&self, dir: Direction) -> Option<String> {
        match dir {
            Direction::Undo => self.entries.iter().find_map(|e| e.back.as_ref().err().cloned()),
            Direction::Do | Direction::Redo => None,
        }
    }

    fn start(&self, s: &mut Session, step: u64, dir: Direction, since: u64) -> JobId {
        let mut steps = self.clone();
        // Undone last item first: what an item needed (a folder made before the photos moved
        // into it) is reversed after it.
        if dir == Direction::Undo {
            steps.entries.reverse();
        }
        launch(s, steps, dir, Some((step, since)))
    }
}

/// What an item's work gives the owner thread: what it did, and the entries to carry after its ops.
struct Worked<T> {
    ran: Ran<T>,
    carry: Option<Carry>,
}

/// One job item: its place, the item to run (an `Err` fails with its reason) and the file it is
/// about.
struct Slot<I> {
    index: usize,
    item: Result<I, String>,
    file: PathBuf,
}

impl<I: Send + 'static> FileItem for Slot<I> {
    fn file(&self) -> &Path {
        &self.file
    }
}

/// Calls the journal on the I/O worker and waits for it (from a file job's thread).
#[derive(Clone)]
struct Writer {
    io: Option<Sender<Msg>>,
    journal: Arc<dyn Journal>,
}

impl Writer {
    fn call(&self, f: impl FnOnce(&dyn Journal) -> Result<(), String> + Send + 'static) -> Result<(), String> {
        let stopped = || "the I/O worker stopped".to_string();
        let io = self.io.as_ref().ok_or_else(stopped)?;
        let (tx, rx) = mpsc::channel();
        let journal = Arc::clone(&self.journal);
        io.send(Msg::Run(Box::new(move || {
            let _ = tx.send(f(journal.as_ref()));
        })))
        .map_err(|_| stopped())?;
        rx.recv().unwrap_or_else(|_| Err("the file-operation journal could not be written".into()))
    }
}

/// A running operation (or undo, or redo), as the owner thread follows it.
struct Run<D: Driver> {
    op: OpId,
    dir: Direction,
    /// The file step it undoes or redoes, with the undo stack's count of new steps when it started.
    step: Option<(u64, u64)>,
    /// The items in run order.
    steps: Steps<D>,
    /// Per item in run order: the entry it became once done.
    results: Vec<Option<Entry<D::Item>>>,
    skipped: Vec<ItemFailure>,
    job: JobId,
}

/// The ops as one op (all or nothing): `None` for none.
fn batch(mut ops: Vec<Op>) -> Option<Op> {
    match ops.len() {
        0 => None,
        1 => ops.pop(),
        _ => Some(Op::Batch { ops }),
    }
}

impl<D: Driver> Run<D> {
    /// One item's result, on the owner thread: its library ops applied and its entry kept. The
    /// item's files are done either way; a library change that fails is the item's failure, and
    /// its entry is kept without one so that undo still moves the files back.
    fn applied(&mut self, s: &mut Session, a: Applied<Worked<D::Done>>) -> Result<Vec<Op>, String> {
        let Applied { index, file, value: Worked { ran, carry }, .. } = a;
        let done = match ran {
            Ran::Done(d) => d,
            Ran::Skipped(reason) => {
                self.skipped.push(ItemFailure { file, reason });
                return Ok(Vec::new());
            }
        };
        let Some(entry) = self.steps.entries.get(index).cloned() else { return Err(format!("no item {index} in this operation")) };
        let driver = Arc::clone(&self.steps.driver);
        let apply = |s: &mut Session, op: Option<Op>| -> Result<Option<Op>, String> {
            match op {
                None => Ok(None),
                Some(op) => s.apply_own(op).map(Some).map_err(|e| e.to_string()),
            }
        };
        let (forward, back, applied) = match self.dir {
            Direction::Do => {
                let ops = driver.ops(s, &entry.forward, &done);
                let applied = ops.and_then(|ops| apply(s, batch(ops)));
                (driver.settled(&entry.forward, &done), driver.reverse(&entry.forward, &done), applied)
            }
            Direction::Undo => (entry.forward, entry.back, apply(s, entry.op)),
            Direction::Redo => {
                let applied = apply(s, entry.op);
                (driver.settled(&entry.forward, &done), driver.reverse(&entry.forward, &done), applied)
            }
        };
        let (op, failed) = match applied {
            Ok(op) => (op, None),
            Err(e) => (None, Some(format!("the files were changed, but the library could not follow: {e}"))),
        };
        if let Some(slot) = self.results.get_mut(index) {
            *slot = Some(Entry { forward, back, op });
        }
        // After the ops (their folders' states are on their way to the I/O worker): the entries
        // follow the files, even when the library could not.
        if let Some(carry) = carry {
            s.carry_entries(carry);
        }
        match failed {
            None => Ok(Vec::new()),
            Some(reason) => Err(reason),
        }
    }

    /// The job ended: the step recorded or moved, the event kept, the journal entry ended.
    fn finish(self, s: &mut Session, tally: &Tally) -> Notice {
        let Run { op, dir, step, steps, results, skipped, job } = self;
        let mut done = Vec::new();
        let mut rest = Vec::new();
        for (entry, result) in steps.entries.iter().zip(results) {
            match result {
                Some(e) => done.push(e),
                None => rest.push(entry.clone()),
            }
        }
        let count = done.len();
        // Back in the order the items were first done.
        if dir == Direction::Undo {
            done.reverse();
            rest.reverse();
        }
        let file_step = |entries: Vec<Entry<D::Item>>, id: u64| FileStep {
            id,
            label: steps.label,
            batch: Arc::new(Steps { entries, ..steps.clone() }),
            running: None,
        };
        match (dir, step) {
            (Direction::Do, _) => {
                if !done.is_empty() {
                    s.undo.push_files(file_step(done, op));
                }
            }
            (_, Some((id, since))) => {
                let rest: Option<Arc<dyn Batch>> = (!rest.is_empty()).then(|| Arc::new(Steps { entries: rest, ..steps.clone() }) as Arc<dyn Batch>);
                let moved = (!done.is_empty()).then(|| file_step(done, op));
                s.undo.file_step_ended(id, dir == Direction::Undo, rest, moved, since);
            }
            (_, None) => {}
        }
        let total = steps.entries.len();
        let event = FileOpEvent {
            seq: 0,
            op,
            job,
            kind: steps.kind,
            direction: dir,
            label: steps.label,
            total,
            done: count,
            skipped: skipped.clone(),
            failures: tally.failures.clone(),
            cancelled: tally.cancelled,
            destination: steps.destination.clone(),
        };
        s.push_file_op_event(event);
        let journal = Arc::clone(&s.fileops.journal);
        s.send(Msg::Run(Box::new(move || {
            if let Err(e) = journal.end(op) {
                log::warn!("file operation {op}: not ended in the journal: {e}");
            }
        })));
        let shown = Tally { total, done: count, failures: tally.failures.clone(), cancelled: tally.cancelled };
        let mut message = default_message(&job_label(dir, steps.kind, steps.label), &shown);
        if !skipped.is_empty() {
            message.push_str(&format!(", {} skipped", skipped.len()));
        }
        Notice { message, folder: steps.destination }
    }
}

/// "Moving", "Undo Move", "Redo Move".
fn job_label(dir: Direction, kind: FileOpKind, label: &str) -> String {
    match dir {
        Direction::Do => kind.progress().to_string(),
        Direction::Undo => format!("Undo {label}"),
        Direction::Redo => format!("Redo {label}"),
    }
}

/// Starts `steps` running in `dir` as a file job (one thread, items in order).
fn launch<D: Driver>(s: &mut Session, steps: Steps<D>, dir: Direction, step: Option<(u64, u64)>) -> JobId {
    let op = s.fileops.next;
    s.fileops.next = s.fileops.next.saturating_add(1);
    let driver = Arc::clone(&steps.driver);
    let slots: Vec<Slot<D::Item>> = steps
        .entries
        .iter()
        .enumerate()
        .map(|(index, e)| {
            let item = match dir {
                Direction::Do | Direction::Redo => Ok(e.forward.clone()),
                Direction::Undo => e.back.clone(),
            };
            let file = item.as_ref().unwrap_or(&e.forward);
            Slot { index, file: driver.source(file).to_path_buf(), item }
        })
        .collect();
    let entry = JournalEntry {
        id: op,
        kind: steps.kind,
        direction: dir,
        items: slots
            .iter()
            .map(|slot| JournalItem {
                from: slot.file.clone(),
                to: slot.item.as_ref().ok().and_then(|i| driver.target(i)).map(Path::to_path_buf),
                state: ItemState::Pending,
            })
            .collect(),
    };
    let writer = Writer { io: s.io_sender(), journal: Arc::clone(&s.fileops.journal) };
    let clock = Arc::clone(&s.clock);
    // The entry is written once, before the first item's work; an operation whose entry could not
    // be written moves nothing.
    let begun: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
    let work_driver = Arc::clone(&driver);
    let work = move |slot: Slot<D::Item>| -> Result<Done<Worked<D::Done>>, String> {
        {
            let mut b = begun.lock().unwrap_or_else(PoisonError::into_inner);
            if b.is_none() {
                let entry = entry.clone();
                *b = Some(writer.call(move |j| j.begin(&entry)));
            }
            if let Some(Err(e)) = b.as_ref() {
                return Err(format!("the file-operation journal could not be written: {e}"));
            }
        }
        let Slot { index, item, .. } = slot;
        let item = item?;
        let ran = work_driver.run(&item)?;
        let carry = match &ran {
            Ran::Done(done) => work_driver.carry(&item, done),
            Ran::Skipped(_) => None,
        };
        // The backups follow the files at once, on this thread (`data-model.md` §8).
        if let Some(c) = &carry {
            match dalica_store::carry::backups(c, clock.now()) {
                Ok(out) => {
                    for r in out.refused {
                        log::warn!("file operation {op}: the originals backup of {} stayed in {}: {}", r.name, c.from.display(), r.reason);
                    }
                }
                Err(e) => log::warn!("file operation {op}: originals backups not carried: {e}"),
            }
        }
        if let Err(e) = writer.call(move |j| j.item(op, index, ItemState::Done)) {
            log::warn!("file operation {op}: item {index} not marked done in the journal: {e}");
        }
        Ok(Done { changes: Vec::new(), value: Worked { ran, carry } })
    };
    let total = steps.entries.len();
    let label = job_label(dir, steps.kind, steps.label);
    let run = Run { op, dir, step, steps, results: (0..total).map(|_| None).collect(), skipped: Vec::new(), job: 0 };
    let shared = Arc::new(Mutex::new(Some(run)));
    let (on_applied, on_finish) = (Arc::clone(&shared), Arc::clone(&shared));
    let job = FileJob::new(JobKind::Files, label, slots, work)
        .on_applied(move |s: &mut Session, a: Applied<Worked<D::Done>>| {
            let mut guard = on_applied.lock().unwrap_or_else(PoisonError::into_inner);
            match guard.as_mut() {
                Some(run) => run.applied(s, a),
                None => Err("the operation has ended".into()),
            }
        })
        .on_finish(move |s: &mut Session, tally: &Tally| {
            let run = on_finish.lock().unwrap_or_else(PoisonError::into_inner).take();
            match run {
                Some(run) => run.finish(s, tally),
                None => Notice::default(),
            }
        });
    let id = s.start_file_job(job);
    if let Some(run) = shared.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
        run.job = id;
    }
    id
}

impl Session {
    /// Starts file operation `op` as a file job ([`JobKind::Files`]) and returns its job id: its
    /// items run in order on a worker thread, each finished item's library ops are applied by
    /// [`Session::poll`], and when it ends the finished items become one library undo step.
    /// Never touches the disk.
    pub fn start_file_op<D: Driver>(&mut self, op: FileOp<D>) -> JobId {
        let FileOp { driver, kind, label, items, destination } = op;
        let entries = items.into_iter().map(|forward| Entry { forward, back: Err("not done".into()), op: None }).collect();
        launch(self, Steps { driver, kind, label, destination, entries }, Direction::Do, None)
    }

    /// The finished file operations, undos and redos, oldest first (at most [`MAX_EVENTS`]): the
    /// UI turns the ones it has not seen (by [`FileOpEvent::seq`]) into notices.
    pub fn file_op_events(&self) -> impl Iterator<Item = &FileOpEvent> {
        self.fileops.events.iter()
    }

    /// Uses `journal` as the file-operation journal from now on (the session opens `file-ops.json`
    /// itself; tests plug in a [`MemoryJournal`]).
    pub fn set_file_op_journal(&mut self, journal: Arc<dyn Journal>) {
        self.fileops.journal = journal;
    }

    /// At open: `file-ops.json` becomes the journal. Its problems (a damaged file kept aside)
    /// become notices; the operations an earlier session left are kept for the repair
    /// ([`Session::file_ops_left`]) and the ids continue past them. A journal that cannot be opened
    /// refuses every operation ([`NoJournal`]), with a notice.
    pub(crate) fn open_file_op_journal(&mut self) {
        match Intents::open(&self.dir, self.clock.now()) {
            Ok(opened) => {
                for intents::Problem::SetAside { why, kept, kept_ops } in opened.problems {
                    self.notices.push(format!(
                        "The file-operation journal was damaged ({why}); it was kept as {} and {kept_ops} operation(s) in it were read.",
                        kept.display()
                    ));
                }
                self.fileops.left = opened.intents.ops().cloned().collect();
                self.fileops.next = self.fileops.next.max(opened.intents.next_id());
                self.fileops.journal = Arc::new(StoreJournal::new(opened.intents));
            }
            Err(e) => {
                let why = format!("the file-operation journal could not be opened: {e}");
                self.notices.push(format!("{why}; files cannot be moved, copied, renamed or deleted until it can."));
                self.fileops.journal = Arc::new(NoJournal(why));
            }
        }
    }

    /// The operations an earlier session left in `file-ops.json` (interrupted), as found at open.
    pub fn file_ops_left(&self) -> &[Intent] {
        &self.fileops.left
    }

    /// Carries the sidecar entries `carry` names on the I/O worker, in order with the sidecar
    /// writes (`SidecarWriter::carry`). Call it after the ops that put the photos in their new
    /// folder were applied; file operations do it for every item whose [`Driver::carry`] names
    /// photos.
    pub(crate) fn carry_entries(&mut self, carry: Carry) {
        let from = self.library.folder_by_path(&carry.from);
        let to = self.library.folder_by_path(&carry.to);
        self.send(Msg::Carry { carry, from, to });
    }

    /// Undoes (or redoes) file step `step`: starts its reverse (or its items again) as a file job.
    /// `Disabled` with the reason when it is running or cannot be undone.
    pub(crate) fn run_file_step(&mut self, step: FileStep, dir: Direction) -> Result<Option<&'static str>, CmdError> {
        if let Some(reason) = step.blocked(dir) {
            return Err(CmdError::Disabled(reason));
        }
        let since = self.undo.pushes();
        let job = step.batch.start(self, step.id, dir, since);
        self.undo.set_running(step.id, Some(job));
        Ok(Some(step.label))
    }

    fn push_file_op_event(&mut self, mut event: FileOpEvent) {
        let ops = &mut self.fileops;
        event.seq = ops.next_seq;
        ops.next_seq = ops.next_seq.saturating_add(1);
        if ops.events.len() >= MAX_EVENTS {
            ops.events.pop_front();
        }
        ops.events.push_back(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_batches() {
        assert_eq!(job_label(Direction::Do, FileOpKind::Move, "Move"), "Moving");
        assert_eq!(job_label(Direction::Undo, FileOpKind::Move, "Move"), "Undo Move");
        assert_eq!(job_label(Direction::Redo, FileOpKind::Copy, "Copy"), "Redo Copy");
        assert_eq!(batch(Vec::new()), None);
        let one = Op::SetStars { stars: vec![] };
        assert_eq!(batch(vec![one.clone()]), Some(one.clone()));
        assert_eq!(batch(vec![one.clone(), one.clone()]), Some(Op::Batch { ops: vec![one.clone(), one] }));
    }

    #[test]
    fn the_memory_journal_enters_marks_and_ends() {
        let j = MemoryJournal::default();
        let item = JournalItem { from: PathBuf::from("a"), to: Some(PathBuf::from("b")), state: ItemState::Pending };
        j.begin(&JournalEntry { id: 7, kind: FileOpKind::Move, direction: Direction::Do, items: vec![item] }).unwrap();
        j.item(7, 0, ItemState::Done).unwrap();
        assert!(j.item(7, 1, ItemState::Done).is_err());
        assert!(j.item(8, 0, ItemState::Done).is_err());
        assert_eq!(j.entries()[0].items[0].state, ItemState::Done);
        j.end(7).unwrap();
        assert!(j.entries().is_empty());
    }
}
