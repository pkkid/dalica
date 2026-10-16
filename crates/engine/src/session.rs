//! The `Session` (`architecture.md` §4, §5): one open library — its model, undo stack, watched
//! folders, jobs and command registry — owned by one thread (the UI thread in the app, the main
//! thread in the CLI).
//!
//! That thread never touches the disk once the session is open. Everything that does — the index
//! journal and snapshots, sidecar writes (debounced on the clock), `folders.json`, scans — happens
//! on worker threads: the I/O worker started by [`Session::open`], and one thread per scan. Their
//! results come back through [`Session::poll`] (each UI frame) or [`Session::wait_idle`] (the CLI
//! and tests); [`Session::flush`] blocks until every pending write has landed.
//!
//! This file keeps the core: opening, [`Session::execute`], applying ops, undo, the I/O worker and
//! the per-frame [`Session::poll`]. Each other concern is a module of its own with its state in a
//! field here and its methods in an `impl Session` block there, so the tasks that grow them never
//! share a file:
//!
//! | Module | Concern |
//! |---|---|
//! | [`crate::folders`] | the watched folders and `folders.json`; the first-run question |
//! | [`crate::scanning`] | scans as jobs, applied progressively |
//! | [`crate::jobs`] | the job registry (`jobs.list`, `jobs.wait`, `jobs.cancel`) |
//! | [`crate::watching`] | external changes (stub until M5.17) |
//! | [`crate::offline`] | offline volumes (stub until M5.13) |
//! | [`crate::thumbs`] | the thumbnail ladder |
//! | [`crate::metadata`] | the metadata hook and the write-back into photo files (a worker thread of its own) |
//! | [`crate::metadata_cache`] | what photo files say, read on workers for `photo.metadata` |
//! | [`crate::history`] | the photo's own history; the recipe hook |
//! | `preview` | the edit view's preview renders (stub until M7.11) |
//! | [`crate::filejobs`] | jobs that write photo files (Save, Save a Copy, Save As, Export) and bring what they wrote into the library |
//! | `save_recovery` | repairing an interrupted Save, at open and when a scan reads a pending record (stub until M8.18) |
//! | [`crate::fileops`] | file operations (move, copy, rename, trash) as file jobs and library undo steps |
//! | `fileops_recovery` | repairing an interrupted file operation at open (stub until M9.21) |
//! | `albums_file` | `albums.json`, read at open and written after album changes (stub until M9.10) |
//! | [`crate::search`] | search mode's state; the query worker (stub until M9.11) |
//! | `duplicates` | Find Duplicates (stub until M9.16) |
//!
//! **A read-only session** ([`Session::open_read_only`], `--read-only`; [`Session::make_read_only`])
//! refuses every write at two seams of this file and nowhere else: [`Session::execute`] refuses
//! the commands that write files (`Effect::Files`: saves, exports, file operations) with
//! [`CmdError::ReadOnly`], and [`Session::send`] keeps every write from the I/O worker (journal,
//! snapshots, sidecars, `folders.json`, `settings.json`, `albums.json`, carries and queued work),
//! so changes stay in memory; metadata changes are never written back into photo files, and the
//! repairs at open (interrupted saves and file operations) do not run. The library's lock is
//! still taken; thumbnails still go to the thumbnail cache, outside the photo folders.
//!
//! Thumbnails are made by the thumbnailer's own workers as scans land; their sketches come back
//! through the same calls and are applied as derived ops: journaled, but never on the undo stack
//! and never written to a sidecar.

use std::collections::{BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use dalica_core::{Clock, FolderId};
use dalica_fs::{Expect, LibraryLock};
use dalica_library::{Library, Op};
use dalica_store::{FolderState, Recovery, SidecarWriter, Store};
use serde_json::Value;

use crate::error::{CmdError, OpenError};
use crate::fileops::Direction;
pub use crate::folders::FOLDERS_FILE;
use crate::folders::Folders;
use crate::history::Histories;
use crate::jobs::Jobs;
use crate::keymap::Keymap;
use crate::metadata::{Metadata, Origin};
use crate::metadata_cache::MetadataCache;
use crate::offline::Offline;
use crate::preview::Preview;
use crate::registry::{CommandInfo, CommandSpec, Effect, History};
use crate::scanning::{PHOTOS_PER_POLL, ScanKind, Scanning};
use crate::settings::Settings;
use crate::thumbs::{Thumb, ThumbOptions, ThumbStats, Thumbs};
use crate::undo::{Entry, Step, UndoStack};
use crate::watching::Watching;

/// A snapshot is taken after this many ops (the store's own threshold, counted here so the worker
/// is sent a copy of the model only when one is due).
const SNAPSHOT_EVERY: u64 = dalica_store::SNAPSHOT_OPS;
/// How often the I/O worker wakes to write sidecars that became due.
const TICK: Duration = Duration::from_millis(100);
/// The most commands one [`Session::execute_group`] runs.
pub const MAX_GROUP_STEPS: usize = 256;

/// Work for the I/O worker.
pub(crate) enum Msg {
    Journal(Vec<Op>),
    Dirty(Vec<(FolderId, Option<FolderState>)>),
    /// Atomically replace one of the library directory's own files (`folders.json`, `settings.json`).
    Write(PathBuf, Vec<u8>),
    Snapshot(Box<Library>),
    /// The roots found offline: the writer writes nothing under them (`offline.rs`).
    Hold(Vec<PathBuf>),
    /// The watched roots and their checks, run right before each sidecar write under them
    /// (`offline.rs`, M6.43).
    Guard(Vec<(PathBuf, dalica_store::writer::RootCheck)>),
    Flush(Sender<()>),
    /// Runs a piece of work on the I/O worker, in order with the writes before it (the
    /// file-operation journal, `fileops.rs`).
    Run(Box<dyn FnOnce() + Send>),
    /// Carries photos' sidecar entries with their files, in order with the sidecar writes
    /// (`fileops.rs`, `dalica-store`'s `SidecarWriter::carry`): the two folders' ids, `None` for a
    /// folder the library does not have.
    Carry {
        carry: dalica_store::carry::Carry,
        from: Option<FolderId>,
        to: Option<FolderId>,
        /// Set when the carry fails or leaves an entry behind (the operation then stays in the
        /// file-operation journal, `fileops.rs`).
        unfinished: Option<Arc<std::sync::atomic::AtomicBool>>,
    },
}

/// How a session is opened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Where thumbnails are cached, how many workers make them, how much memory they keep.
    pub thumbs: ThumbOptions,
}

/// One open library. Its fields are `pub(crate)` so each concern's module (see the table above)
/// adds its own `impl Session` block.
pub struct Session {
    pub(crate) dir: PathBuf,
    pub(crate) library: Library,
    pub(crate) undo: UndoStack,
    pub(crate) clock: Arc<dyn Clock>,
    registry: Vec<CommandSpec>,
    by_id: HashMap<&'static str, usize>,
    io: Option<Sender<Msg>>,
    worker: Option<JoinHandle<()>>,
    io_errors: Arc<Mutex<Vec<String>>>,
    collected: Option<Vec<Op>>,
    /// The op a command asked its undo step to use instead of the collected inverses.
    undo_op: Option<Op>,
    /// While a command runs (not while it undoes or redoes a step, nor while a file operation's
    /// job brings its results): [`Session::apply`] stamps each op with the photos' last-changed
    /// time ([`Library::stamped`], Recently Updated).
    stamping: bool,
    /// Inside [`Session::execute_group`]: the steps its commands made so far, kept here instead of
    /// on the undo stack until the group ends.
    group: Option<Vec<Step>>,
    recovery: Vec<Recovery>,
    pub(crate) scan_warnings: Vec<String>,
    pub(crate) ops_since_snapshot: u64,
    settings: Settings,
    keymap: Keymap,
    pub(crate) notices: Vec<String>,
    pub(crate) thumbs: Thumbs,
    /// The watched folders (`folders.rs`).
    pub(crate) folders: Folders,
    /// Running scans and what they remember (`scanning.rs`).
    pub(crate) scanning: Scanning,
    /// The job registry (`jobs.rs`).
    pub(crate) jobs: Jobs,
    /// External changes (`watching.rs`).
    pub(crate) watching: Watching,
    /// Which roots are reachable (`offline.rs`).
    pub(crate) offline: Offline,
    /// What the metadata hook was told (`metadata.rs`).
    pub(crate) metadata: Metadata,
    /// What photo files say, read on workers for `photo.metadata` (`metadata_cache.rs`).
    pub(crate) metadata_cache: MetadataCache,
    /// The photo histories' memory of merged steps, and the recipe hook (`history.rs`).
    pub(crate) histories: Histories,
    /// The edit view's preview renders (`preview.rs`).
    pub(crate) preview: Preview,
    /// The suggested-crop jobs running (`cmd/crop.rs`).
    pub(crate) crop_suggestions: crate::cmd::crop::Suggestions,
    /// The Tuning wand and colour picker jobs running (`cmd/tuning.rs`).
    pub(crate) tuning_wands: crate::cmd::tuning::Wands,
    /// The jobs that write photo files, and their events (`filejobs.rs`).
    pub(crate) file_jobs: crate::filejobs::FileJobs,
    /// The user's Pictures folder as known when the library opened: the default export location
    /// lies under it (`settings.rs`).
    pub(crate) pictures_dir: Option<PathBuf>,
    /// File operations: the journal writer and their events (`fileops.rs`).
    pub(crate) fileops: crate::fileops::FileOps,
    /// Search mode (`search.rs`).
    pub(crate) search: crate::search::SearchState,
    /// Find Duplicates (`duplicates.rs`).
    pub(crate) duplicates: crate::duplicates::Duplicates,
    /// Nothing is written (see the module's notes).
    read_only: bool,
    _lock: LibraryLock,
}

impl Session {
    /// Opens (or creates) the library in `dir`: takes its lock (another process holding it gives
    /// [`OpenError::Locked`]), loads the index — repairing what it can, reported by
    /// [`Session::recovery`] — and the watched folders, and starts the I/O worker. If the index had
    /// to be dropped, a rebuild from the watched folders starts in the background.
    /// Call it off the UI thread. Thumbnails use the default [`Options`]: the cache folder the app
    /// set ([`crate::thumbs::set_process_cache_dir`]), else a temporary one.
    pub fn open(dir: &Path, clock: Arc<dyn Clock>) -> Result<Session, OpenError> {
        Session::open_with(dir, clock, &Options::default())
    }

    /// [`Session::open`] with `options`. A thumbnail cache that cannot be opened never fails the
    /// open: a temporary one is used instead, and [`Session::notices`] says why.
    pub fn open_with(dir: &Path, clock: Arc<dyn Clock>, options: &Options) -> Result<Session, OpenError> {
        Session::open_inner(dir, clock, options, false)
    }

    /// [`Session::open_with`], read-only (`--read-only`): nothing is written to disk but the
    /// library's lock, file commands are refused with [`CmdError::ReadOnly`], and the repairs at
    /// open are left for a session that may write (see the module's notes). A library folder that
    /// does not exist yet is still made, as for any session.
    pub fn open_read_only(dir: &Path, clock: Arc<dyn Clock>, options: &Options) -> Result<Session, OpenError> {
        Session::open_inner(dir, clock, options, true)
    }

    fn open_inner(dir: &Path, clock: Arc<dyn Clock>, options: &Options, read_only: bool) -> Result<Session, OpenError> {
        dalica_fs::create_dir_all(dir)?;
        let lock = LibraryLock::acquire(dir)?;
        let opened = Store::open(dir, clock.now())?;
        let (folders, folders_note) = Folders::read(&dir.join(FOLDERS_FILE), clock.now());
        let mut recovery = opened.report;
        recovery.extend(folders_note);
        let io_errors = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        let worker = {
            let (clock, errors) = (Arc::clone(&clock), Arc::clone(&io_errors));
            let store = opened.store;
            std::thread::Builder::new()
                .name("dalica-io".into())
                .spawn(move || io_worker(store, rx, clock.as_ref(), &errors))
                .map_err(|e| OpenError::Thread(e.to_string()))?
        };
        let registry = crate::cmd::specs();
        let by_id: HashMap<&'static str, usize> = registry.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
        let (settings, settings_note) = Settings::load(dir, clock.now());
        let mut keymap = Keymap::from_specs(&registry).unwrap_or_else(|e| {
            log::error!("a default shortcut does not parse: {e}");
            Keymap::default()
        });
        let overrides = settings.keymap(&|c| keymap.knows(c));
        keymap.set_overrides(overrides);
        let (thumbs, thumb_notes) = Thumbs::open(&options.thumbs);
        let mut notices: Vec<String> = settings_note.into_iter().collect();
        // Stored values of the wrong type: the defaults are used, and the notice says which.
        notices.extend(settings.problems());
        notices.extend(thumb_notes);
        let jobs = Jobs::new(clock.now());
        // Read here, off the UI thread (the XDG user-dirs file may move it).
        let pictures_dir = dalica_scan::Places::system().pictures;
        let mut session = Session {
            dir: dir.to_path_buf(),
            library: opened.library,
            undo: UndoStack::default(),
            clock,
            registry,
            by_id,
            io: Some(tx),
            worker: Some(worker),
            io_errors,
            collected: None,
            undo_op: None,
            stamping: false,
            group: None,
            recovery,
            scan_warnings: Vec::new(),
            ops_since_snapshot: 0,
            settings,
            keymap,
            notices,
            thumbs,
            folders,
            scanning: Scanning::default(),
            jobs,
            watching: Watching::default(),
            offline: Offline::default(),
            metadata: Metadata::default(),
            metadata_cache: MetadataCache::default(),
            histories: Histories::default(),
            preview: Preview::default(),
            crop_suggestions: crate::cmd::crop::Suggestions::default(),
            tuning_wands: crate::cmd::tuning::Wands::default(),
            file_jobs: crate::filejobs::FileJobs::default(),
            pictures_dir,
            fileops: crate::fileops::FileOps::default(),
            search: crate::search::SearchState::default(),
            duplicates: crate::duplicates::Duplicates::default(),
            read_only,
            _lock: lock,
        };
        session.configure_thumbnails();
        session.sync_thumbnails();
        if session.recovery.iter().any(|r| matches!(r, Recovery::RebuildNeeded { .. })) && !session.folders.roots.is_empty() {
            session.start_scan(session.folders.roots.clone(), ScanKind::Rebuild);
        }
        // The repairs at open write files; a read-only session leaves them for the next session
        // that may write (its file commands are refused, so no journal is needed either).
        if !session.read_only {
            session.recover_saves_at_open();
            session.open_file_op_journal();
            session.recover_file_ops_at_open();
        }
        session.read_albums_at_open();
        Ok(session)
    }

    /// Runs the command `id` with JSON `params`: checks that it is enabled, runs it with panics
    /// caught, and records exactly one undo step (for commands on the library history that changed
    /// something). The only way library state changes.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CmdError> {
        let spec = *self.spec(id).ok_or_else(|| CmdError::UnknownCommand(id.to_string()))?;
        if self.read_only && spec.effect == Effect::Files {
            return Err(CmdError::ReadOnly(format!("{} writes files, and this session is read-only (--read-only)", spec.label)));
        }
        (spec.enabled)(self, params).map_err(|d| CmdError::Disabled(d.0))?;
        self.collected = Some(Vec::new());
        self.undo_op = None;
        self.folders.change = None;
        let stamping = std::mem::replace(&mut self.stamping, true);
        let result = catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params)));
        self.stamping = stamping;
        let mut inverses = self.collected.take().unwrap_or_default();
        // A command that changed something may undo with an op of its own (`set_undo_op`); the
        // last-changed times its ops stamped are put back with it.
        if let Some(op) = self.undo_op.take().filter(|_| !inverses.is_empty()) {
            let mut times = last_changed_in(inverses.iter().rev());
            inverses = vec![if times.is_empty() {
                op
            } else {
                times.insert(0, op);
                Op::Batch { ops: times }
            }];
        }
        // The folder rules the command replaced (`folders.rs`): a rules-only change is a step too.
        let modes = self.folders.change.take().filter(|m| !m.is_empty());
        if spec.history == History::Library && (!inverses.is_empty() || modes.is_some()) {
            let op = if inverses.len() == 1 {
                inverses.pop().unwrap_or(Op::Batch { ops: Vec::new() })
            } else {
                inverses.reverse();
                Op::Batch { ops: inverses }
            };
            let step = Step { label: spec.label, op, modes };
            match &mut self.group {
                Some(group) => group.push(step),
                None => self.undo.push(step),
            }
        }
        match result {
            Ok(r) => r,
            Err(panic) => {
                let message =
                    panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                log::error!("command {id} panicked: {message}");
                Err(CmdError::Internal(format!("{id}: {message}")))
            }
        }
    }

    /// Runs several library commands as **one** library undo step labelled `label` (an agent's "set
    /// the star, the caption and the tags" is one Edit ▸ Undo): `steps` are `(command id, params)`,
    /// run in order through [`Session::execute`], and their results are returned in that order.
    ///
    /// Before any runs, every step is checked: its command must exist, change the library alone
    /// (`Effect::Library` on the library history: no file operations, queries or view commands), and
    /// be enabled for its params; at most [`MAX_GROUP_STEPS`] steps. When a step fails, the steps
    /// already run (and what the failing one changed) are undone at once, nothing is left on the
    /// undo stack, and its error is returned with the step named. A group that changes nothing
    /// records no step. Not re-entrant: a group inside a group is an internal error.
    pub fn execute_group(&mut self, steps: &[(&str, Value)], label: &'static str) -> Result<Vec<Value>, CmdError> {
        if self.group.is_some() {
            return Err(CmdError::Internal("a group of commands cannot run inside another".into()));
        }
        if steps.len() > MAX_GROUP_STEPS {
            return Err(CmdError::Limit(format!("at most {MAX_GROUP_STEPS} commands in one group")));
        }
        for (n, (id, params)) in steps.iter().enumerate() {
            let spec = *self.spec(id).ok_or_else(|| CmdError::UnknownCommand((*id).to_string()))?;
            if spec.effect != Effect::Library || spec.history != History::Library {
                return Err(in_step(CmdError::BadParams("only commands that change the library alone can be grouped".into()), n, id));
            }
            (spec.enabled)(self, params).map_err(|d| in_step(CmdError::Disabled(d.0), n, id))?;
        }
        self.group = Some(Vec::new());
        let mut results = Vec::with_capacity(steps.len());
        let mut failed = None;
        for (n, (id, params)) in steps.iter().enumerate() {
            match self.execute(id, params) {
                Ok(r) => results.push(r),
                Err(e) => {
                    failed = Some(in_step(e, n, id));
                    break;
                }
            }
        }
        let done = self.group.take().unwrap_or_default();
        if let Some(e) = failed {
            self.roll_back(done);
            return Err(e);
        }
        if let Some(step) = one_step(done, label) {
            self.undo.push(step);
        }
        Ok(results)
    }

    /// Undoes `done` (a failed group's steps), last first, as a command's change, recording no step.
    fn roll_back(&mut self, done: Vec<Step>) {
        let saved = self.collected.replace(Vec::new());
        for step in done.into_iter().rev() {
            if let Err(e) = self.apply_step(step.op) {
                log::error!("undoing a failed group's step {}: {e}", step.label);
                self.scan_warnings.push(format!("{} could not be undone: {e}", step.label));
            }
            if let Some(modes) = step.modes {
                self.swap_modes(modes);
            }
        }
        self.collected = saved;
    }

    /// Applies `op` to the model, journals it, marks the folders it touched for a sidecar write, and
    /// (inside `execute`) collects its inverse for the undo step, then tells the metadata hook which
    /// photos' owned fields it changed (`metadata.rs`) and the recipe hook which photos' recipes it
    /// changed (`history.rs`). Commands change the library only through this.
    pub(crate) fn apply(&mut self, op: Op) -> Result<Op, CmdError> {
        // A command's change counts for Recently Updated (`organizing.md` §4.1), unless its op
        // carries its own times (a command that stamped it, an undo putting old times back).
        let op = if self.stamping && !has_last_changed(&op) { self.library.stamped(op, self.clock.now()) } else { op };
        // An op from the files (outside `execute`): what it imports and its conflicts (`scanning.rs`).
        let files = if self.collected.is_none() { self.files_op_before(&op) } else { None };
        let before = self.all_folder_ids();
        let owned_before = crate::metadata::snapshot(&self.library, &crate::metadata::candidates(&self.library, &op));
        let recipes_before = crate::history::recipes_before(&self.library, &op);
        let inverse = self.library.apply(op.clone()).map_err(|e| match e {
            dalica_library::Error::UnknownFolder(_) | dalica_library::Error::UnknownPhoto(_) => CmdError::NotFound(e.to_string()),
            other => CmdError::BadParams(other.to_string()),
        })?;
        // A scan's op may bring a pending Save record (`save_recovery.rs`), repaired by a write.
        if self.collected.is_none() && !self.read_only {
            self.note_save_records(&op);
        }
        // `albums.json` (`albums_file.rs`) and the search index (`search.rs`) follow every op.
        self.albums_applied(&op);
        self.search_applied(&op);
        self.send(Msg::Journal(vec![op]));
        let rev = self.library.revision();
        let touched: BTreeSet<FolderId> = self.all_folder_ids().into_iter().filter(|f| self.library.folder_revision(*f) == rev).collect();
        let mut states: Vec<(FolderId, Option<FolderState>)> =
            touched.into_iter().map(|f| (f, dalica_store::mapping::folder_state(&self.library, f))).collect();
        // Folders the op removed: the I/O worker drops their pending writes (a folder deleted
        // outside Dalica never comes back to be written into).
        states.extend(self.left_library(before));
        self.send(Msg::Dirty(states));
        if let Some(c) = &mut self.collected {
            c.push(inverse.clone());
        }
        self.count_op();
        // Inside `execute` (undo and redo included) the change is a command's; outside it, a
        // scan's or the watcher's.
        // A read-only session writes nothing back into photo files: its changes count as the files'.
        let origin = if self.collected.is_some() && !self.read_only { Origin::Command } else { Origin::Files };
        self.metadata_applied(owned_before, origin);
        self.recipes_applied(recipes_before);
        if let Some(conflicts) = files {
            self.files_op_after(conflicts, &inverse);
        }
        Ok(inverse)
    }

    /// Inside `execute`: the command's undo step applies `op` instead of the inverses of the ops it
    /// applied (Adjust Date undoes with the dates the photos had, written out, so the undo still
    /// puts them back after a rescan read the written date back as the file's own). Ignored
    /// outside `execute`, and when the command changed nothing.
    pub(crate) fn set_undo_op(&mut self, op: Op) {
        if self.collected.is_some() {
            self.undo_op = Some(op);
        }
    }

    /// Applies `op` as a command's change (`Origin::Command`, the files' import hooks left out)
    /// outside `execute`, and returns its inverse without collecting it: a file operation's results
    /// as its job brings them (`fileops.rs`), the operation keeping the inverse for its undo step.
    pub(crate) fn apply_own(&mut self, op: Op) -> Result<Op, CmdError> {
        let saved = self.collected.replace(Vec::new());
        let stamping = std::mem::replace(&mut self.stamping, false);
        let result = self.apply(op);
        self.stamping = stamping;
        self.collected = saved;
        result
    }

    /// A sender to the I/O worker, for work that must run there in order with the session's
    /// writes (the file-operation journal); `None` once it stopped.
    pub(crate) fn io_sender(&self) -> Option<Sender<Msg>> {
        self.io.clone()
    }

    /// Applies a derived op ([`Op::is_derived`]: sketches) and journals it, without an undo step
    /// and without marking any folder for a sidecar write (it bumps neither revision they follow).
    pub(crate) fn apply_derived(&mut self, op: Op) -> Result<(), CmdError> {
        if !op.is_derived() {
            return Err(CmdError::Internal("only derived data may change outside a command".into()));
        }
        self.library.apply(op.clone()).map_err(|e| CmdError::BadParams(e.to_string()))?;
        self.send(Msg::Journal(vec![op]));
        self.count_op();
        Ok(())
    }

    /// Counts one journaled op and sends the worker a snapshot when one is due.
    fn count_op(&mut self) {
        self.ops_since_snapshot += 1;
        if self.ops_since_snapshot >= SNAPSHOT_EVERY {
            self.ops_since_snapshot = 0;
            self.send(Msg::Snapshot(Box::new(self.library.clone())));
        }
    }

    /// Applies the sketches the thumbnailer made, [`crate::thumbs::SKETCH_BATCH`] per op; those of
    /// photos that are gone, or unchanged, are left out.
    fn apply_sketches(&mut self) {
        let mut list = self.thumbs.take_sketches();
        list.retain(|(id, sketch)| self.library.photo(*id).is_some() && self.library.sketch(*id) != sketch.as_ref());
        for batch in list.chunks(crate::thumbs::SKETCH_BATCH) {
            if let Err(e) = self.apply_derived(Op::SetSketches { sketches: batch.to_vec() }) {
                self.scan_warnings.push(format!("sketches not applied: {e}"));
            }
        }
    }

    fn all_folder_ids(&self) -> Vec<FolderId> {
        self.library.folders().map(|f| f.id).collect()
    }

    /// Of the folders in `before`, those no longer in the library, each with no state: told to the
    /// I/O worker, they leave its pending writes.
    pub(crate) fn left_library(&self, before: Vec<FolderId>) -> impl Iterator<Item = (FolderId, Option<FolderState>)> + use<'_> {
        before.into_iter().filter(|f| self.library.folder(*f).is_none()).map(|f| (f, None))
    }

    pub(crate) fn send(&mut self, msg: Msg) {
        // The metadata writer holds and guards the same roots as the sidecar writer.
        match &msg {
            Msg::Hold(roots) => self.metadata_hold(roots.clone()),
            Msg::Guard(roots) => self.metadata_guard(roots.clone()),
            _ => {}
        }
        // A read-only session keeps every write in memory: the worker only holds, guards and
        // answers flushes. Work queued to it is dropped unrun (a caller waiting on its answer
        // hears that the worker did not take it).
        if self.read_only && !matches!(msg, Msg::Hold(_) | Msg::Guard(_) | Msg::Flush(_)) {
            return;
        }
        if let Some(io) = &self.io
            && io.send(msg).is_err()
        {
            self.io_errors.lock().unwrap_or_else(PoisonError::into_inner).push("the I/O worker stopped".into());
        }
    }

    /// Applies an undo or redo step's op; the empty batch of a rules-only step changes nothing and
    /// is not journaled.
    fn apply_step(&mut self, op: Op) -> Result<Op, CmdError> {
        if matches!(&op, Op::Batch { ops } if ops.is_empty()) {
            return Ok(op);
        }
        let op = crate::cmd::dates::without_file_dates(&self.library, op);
        // An undo or redo puts the times back that its op holds; it is no change of its own.
        let stamping = std::mem::replace(&mut self.stamping, false);
        let result = self.apply(op);
        self.stamping = stamping;
        result
    }

    /// Undoes the last library step. `Ok(None)` when there is nothing to undo. A file step starts
    /// its reverse as a file job and stays on the stack until it ends (`fileops.rs`); `Disabled`
    /// with the reason while it runs or when it cannot be undone.
    pub(crate) fn undo(&mut self) -> Result<Option<&'static str>, CmdError> {
        match self.undo.last(false) {
            None => return Ok(None),
            Some(Entry::Files(f)) => {
                let f = f.clone();
                return self.run_file_step(f, Direction::Undo);
            }
            Some(Entry::Step(_)) => {}
        }
        let Some(Entry::Step(step)) = self.undo.pop_undo() else { return Ok(None) };
        match self.apply_step(step.op.clone()) {
            Ok(redo) => {
                let modes = step.modes.clone().map(|m| self.swap_modes(m));
                self.undo.push_redo(Step { label: step.label, op: redo, modes });
                Ok(Some(step.label))
            }
            Err(e) => {
                self.undo.push_undo(step);
                Err(e)
            }
        }
    }

    /// Redoes the last undone step. `Ok(None)` when there is nothing to redo. A file step runs its
    /// items again as a file job, as for [`Session::undo`].
    pub(crate) fn redo(&mut self) -> Result<Option<&'static str>, CmdError> {
        match self.undo.last(true) {
            None => return Ok(None),
            Some(Entry::Files(f)) => {
                let f = f.clone();
                return self.run_file_step(f, Direction::Redo);
            }
            Some(Entry::Step(_)) => {}
        }
        let Some(Entry::Step(step)) = self.undo.pop_redo() else { return Ok(None) };
        match self.apply_step(step.op.clone()) {
            Ok(undo) => {
                let modes = step.modes.clone().map(|m| self.swap_modes(m));
                self.undo.push_undo(Step { label: step.label, op: undo, modes });
                Ok(Some(step.label))
            }
            Err(e) => {
                self.undo.push_redo(step);
                Err(e)
            }
        }
    }

    /// Applies the results of finished background work: up to [`PHOTOS_PER_POLL`] photos of what
    /// the scans found (ending the scans that are complete), what the watcher and the online
    /// checks reported, and up to [`crate::thumbs::POLL_MAX`] thumbnail jobs (their sketches in
    /// batches); then the metadata hook's turn, the photo metadata answers and the edit view's
    /// preview renders that arrived, and drops the jobs that expired. Returns how many things it
    /// handled. Call it each frame; it never blocks and never touches the disk.
    pub fn poll(&mut self) -> usize {
        let mut n = self.poll_scans(PHOTOS_PER_POLL);
        n += self.poll_watching();
        n += self.poll_offline();
        n += self.thumbs.poll(&self.library, crate::thumbs::POLL_MAX);
        self.apply_sketches();
        n += self.poll_metadata();
        n += self.poll_photo_metadata();
        n += self.poll_preview();
        n += self.poll_crop_suggestions();
        n += self.poll_tuning_wands();
        n += self.poll_file_jobs();
        n += self.poll_search();
        n += self.poll_duplicates();
        self.jobs.prune(self.clock.now());
        n
    }

    /// Blocks until all background work — file jobs, scans, then the thumbnails and sketches they started —
    /// has finished and its results are applied.
    pub fn wait_idle(&mut self) {
        // File jobs first: the folders they wrote outside the library are scanned when they end.
        self.finish_file_jobs();
        self.wait_scans();
        self.thumbs.wait(&self.library);
        self.apply_sketches();
    }

    /// Waits for running scans, applies the sketches already made (it does not wait for thumbnails
    /// still being made: they are a cache), then waits until every journal record and sidecar write
    /// has landed. `Err(Unsaved)` lists writes that failed (they stay pending and are retried).
    pub fn flush(&mut self) -> Result<(), CmdError> {
        self.wait_scans();
        self.thumbs.poll(&self.library, usize::MAX);
        self.apply_sketches();
        let (ack, done) = mpsc::channel();
        self.send(Msg::Flush(ack));
        let _ = done.recv();
        let errors = std::mem::take(&mut *self.io_errors.lock().unwrap_or_else(PoisonError::into_inner));
        if errors.is_empty() { Ok(()) } else { Err(CmdError::Unsaved(errors.join("; "))) }
    }

    /// Whether this session writes nothing (`--read-only`; see the module's notes).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Makes an open session read-only from now on, for good (a demo opened before the switch
    /// applied, a session handed over already open). Writes already sent still land; the next
    /// ones stay in memory.
    pub fn make_read_only(&mut self) {
        self.read_only = true;
    }

    /// Uses `dir` as the user's Pictures folder (tests, demos): the default export location lies
    /// under it ([`Session::export_location`]).
    pub fn set_pictures_dir(&mut self, dir: Option<PathBuf>) {
        self.pictures_dir = dir;
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    pub fn undo_stack(&self) -> &UndoStack {
        &self.undo
    }

    /// What opening the library repaired.
    pub fn recovery(&self) -> &[Recovery] {
        &self.recovery
    }

    /// Other things opening the library repaired or could not read (a damaged `settings.json`).
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// What scans could not read, oldest first.
    pub fn scan_warnings(&self) -> &[String] {
        &self.scan_warnings
    }

    /// What photo `id` draws now in a cell whose long side is `need` device pixels, from memory
    /// (never waiting, never touching the disk): the ladder size that covers it, else the next
    /// smaller one to scale up, else its colour sketch, else [`Thumb::NotScanned`]. What is missing
    /// is queued at the visible priority; ask again after [`Session::poll`].
    pub fn thumbnail(&mut self, id: dalica_core::PhotoId, need: u32) -> Thumb {
        self.thumbs.request(&self.library, id, need)
    }

    /// Prefetches thumbnails around the visible cells: `order` is the photos in display order,
    /// `visible` the indices on screen, `velocity` the scroll speed in cells per second (positive
    /// towards the end of `order`), `need` the cells' long side in device pixels. Loads ahead in
    /// the scroll direction (further the faster), then a little behind; prefetch the view has left
    /// is cancelled. Returns the photos in the order they were queued.
    pub fn prefetch_thumbnails(
        &mut self,
        order: &[dalica_core::PhotoId],
        visible: std::ops::Range<usize>,
        velocity: f64,
        need: u32,
    ) -> Vec<dalica_core::PhotoId> {
        self.thumbs.prefetch(&self.library, order, visible, velocity, need)
    }

    /// Why photo `id` has no thumbnail, if its thumbnail failed.
    pub fn thumbnail_failure(&self, id: dalica_core::PhotoId) -> Option<&str> {
        self.thumbs.failure(id)
    }

    /// Thumbnail jobs queued or running.
    pub fn thumbnails_pending(&self) -> usize {
        self.thumbs.pending()
    }

    /// The thumbnailer's numbers.
    pub fn thumbnail_stats(&self) -> ThumbStats {
        self.thumbs.stats()
    }

    /// The thumbnail cache folder, if thumbnails are on.
    pub fn thumbnail_cache_dir(&self) -> Option<&Path> {
        self.thumbs.cache_dir()
    }

    /// The display's long side in device pixels: the size of the screen-size preview.
    pub fn set_screen_size(&mut self, long_side: u32) {
        self.thumbs.set_screen(long_side);
    }

    /// The shortcuts in effect.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Changes the keymap (through `f`) and saves the user's overrides in `settings.json`.
    pub(crate) fn change_keymap<R>(&mut self, f: impl FnOnce(&mut Keymap) -> R) -> R {
        let r = f(&mut self.keymap);
        let keymap = &self.keymap;
        self.settings.set_keymap(keymap.overrides(), &|c| keymap.knows(c));
        let bytes = self.settings.print();
        self.send(Msg::Write(self.dir.join(crate::settings::FILE), bytes));
        r
    }

    /// The settings as loaded and changed since (`settings.json`).
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings (through `f`) and, if anything changed, saves `settings.json` through
    /// the I/O worker (atomically, off the calling thread). The window and view state use it.
    pub fn change_settings<R>(&mut self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let before = self.settings.clone();
        let r = f(&mut self.settings);
        if self.settings != before {
            let bytes = self.settings.print();
            self.send(Msg::Write(self.dir.join(crate::settings::FILE), bytes));
        }
        r
    }

    /// Adds the UI's commands (`UI_COMMANDS`) to the keymap with their default keys for this
    /// platform, so they can be bound, rebound through `app.setShortcut` and conflict-checked like
    /// the engine's; the user's stored bindings for them apply from now on. Adding the same list
    /// again changes nothing. `Err` (and no change) when a default key does not parse.
    pub fn add_ui_keys(&mut self, commands: &[(&str, crate::keymap::DefaultKeys)]) -> Result<(), String> {
        self.keymap.add_commands(commands.iter().copied(), crate::keymap::Platform::current())?;
        let keymap = &self.keymap;
        let overrides = self.settings.keymap(&|c| keymap.knows(c));
        self.keymap.set_overrides(overrides);
        Ok(())
    }

    pub fn spec(&self, id: &str) -> Option<&CommandSpec> {
        self.by_id.get(id).and_then(|&i| self.registry.get(i))
    }

    pub fn specs(&self) -> &[CommandSpec] {
        &self.registry
    }

    /// The registry as `engine.commands` lists it, with `enabled` evaluated now (for `{}` params).
    pub fn commands(&self) -> Vec<CommandInfo> {
        self.registry
            .iter()
            .map(|s| CommandInfo {
                id: s.id,
                label: s.label,
                menu: s.menu,
                shortcut: crate::keymap::first_default(s.keys, crate::keymap::Platform::current()),
                params: s.params,
                enabled: (s.enabled)(self, &Value::Null).is_ok(),
                ui: false,
                checked: None,
                live: true,
                effect: s.effect,
                history: s.history,
            })
            .collect()
    }
}

/// `e` naming the group's step `n` (counted from 1) and its command; the error's kind is kept.
/// Whether `op` sets last-changed times anywhere in it (nested batches walked without recursion).
fn has_last_changed(op: &Op) -> bool {
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::SetLastChanged { .. } => return true,
            Op::Batch { ops } => stack.extend(ops),
            _ => {}
        }
    }
    false
}

/// The [`Op::SetLastChanged`] ops in `ops`, in the order applying them in turn would apply them
/// (nested batches walked without recursion).
fn last_changed_in<'a>(ops: impl Iterator<Item = &'a Op>) -> Vec<Op> {
    let mut out = Vec::new();
    for op in ops {
        let mut stack = vec![op];
        while let Some(op) = stack.pop() {
            match op {
                Op::SetLastChanged { .. } => out.push(op.clone()),
                Op::Batch { ops } => stack.extend(ops.iter().rev()),
                _ => {}
            }
        }
    }
    out
}

fn in_step(e: CmdError, n: usize, id: &str) -> CmdError {
    let at = |m: String| format!("step {} ({id}): {m}", n.saturating_add(1));
    match e {
        CmdError::UnknownCommand(c) => CmdError::UnknownCommand(c),
        CmdError::BadParams(m) => CmdError::BadParams(at(m)),
        CmdError::Disabled(m) => CmdError::Disabled(at(m)),
        CmdError::NotFound(m) => CmdError::NotFound(at(m)),
        CmdError::Limit(m) => CmdError::Limit(at(m)),
        CmdError::Unsaved(m) => CmdError::Unsaved(at(m)),
        CmdError::ReadOnly(m) => CmdError::ReadOnly(at(m)),
        CmdError::Internal(m) => CmdError::Internal(at(m)),
    }
}

/// A group's steps as one: their ops undone last first, and for each folder the rules from before
/// the first step that changed them. `None` when no step changed anything.
fn one_step(mut done: Vec<Step>, label: &'static str) -> Option<Step> {
    if done.len() <= 1 {
        return done.pop().map(|s| Step { label, ..s });
    }
    let mut rules: Vec<(PathBuf, dalica_scan::Rule)> = Vec::new();
    let mut any_rules = false;
    for modes in done.iter().filter_map(|s| s.modes.as_ref()) {
        any_rules = true;
        for (path, rule) in &modes.0 {
            if !rules.iter().any(|(p, _)| p == path) {
                rules.push((path.clone(), *rule));
            }
        }
    }
    let ops = done.into_iter().rev().map(|s| s.op).collect();
    Some(Step { label, op: Op::Batch { ops }, modes: any_rules.then_some(crate::folders::Modes(rules)) })
}

impl Drop for Session {
    /// Stops the I/O worker after it has written everything pending.
    fn drop(&mut self) {
        // File jobs finish the items they are writing and start no other.
        self.file_jobs.cancel_all();
        drop(self.io.take());
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// The I/O worker: owns the store and the sidecar writer.
fn io_worker(mut store: Store, rx: Receiver<Msg>, clock: &dyn Clock, errors: &Mutex<Vec<String>>) {
    let mut writer = SidecarWriter::new();
    let mut states: HashMap<FolderId, Option<FolderState>> = HashMap::new();
    let fail = |e: String| {
        log::warn!("{e}");
        errors.lock().unwrap_or_else(PoisonError::into_inner).push(e);
    };
    let report = |outcomes: std::collections::BTreeMap<FolderId, dalica_store::Outcome>| {
        for (folder, outcome) in outcomes {
            match outcome {
                dalica_store::Outcome::Failed(e) => fail(format!("sidecar of folder {folder}: {e}")),
                dalica_store::Outcome::SetAside(p) => {
                    fail(format!("folder {folder}: damaged sidecar kept as {}; the folder is read-only", p.display()))
                }
                dalica_store::Outcome::Written | dalica_store::Outcome::Unchanged => {
                    // Landed since: an earlier failed attempt (a folder renamed before its write
                    // came) is no longer unsaved.
                    let earlier = format!("sidecar of folder {folder}: ");
                    errors.lock().unwrap_or_else(PoisonError::into_inner).retain(|e| !e.starts_with(&earlier));
                }
                dalica_store::Outcome::ReadOnly => {}
            }
        }
    };
    let write = |writer: &mut SidecarWriter, states: &HashMap<FolderId, Option<FolderState>>, all: bool| {
        let state_of = |f: FolderId| states.get(&f).cloned().flatten();
        report(if all { writer.flush_with(state_of, clock.now()) } else { writer.write_due_with(state_of, clock.now()) });
    };
    loop {
        match rx.recv_timeout(TICK) {
            Ok(Msg::Journal(ops)) => {
                for op in ops {
                    if let Err(e) = store.append(&op) {
                        fail(format!("journal: {e}"));
                    }
                }
            }
            Ok(Msg::Dirty(list)) => {
                for (folder, state) in list {
                    writer.mark_dirty(folder);
                    states.insert(folder, state);
                }
            }
            Ok(Msg::Write(path, bytes)) => {
                let result =
                    dalica_fs::identity(&path).and_then(|id| dalica_fs::replace(&path, &bytes, id.map_or(Expect::Absent, Expect::Unchanged)));
                if let Err(e) = result {
                    fail(format!("{}: {e}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
                }
            }
            Ok(Msg::Snapshot(library)) => {
                if let Err(e) = store.snapshot(&library) {
                    fail(format!("snapshot: {e}"));
                }
            }
            Ok(Msg::Run(work)) => {
                if catch_unwind(AssertUnwindSafe(work)).is_err() {
                    fail("work on the I/O worker panicked".into());
                }
            }
            Ok(Msg::Carry { carry, from, to, unfinished }) => {
                let now = clock.now();
                let carried = match (from, to) {
                    (Some(from), Some(to)) => {
                        let r = writer.carry(&carry, from, to, |f| states.get(&f).cloned().flatten(), now);
                        report(r.writes);
                        r.carried
                    }
                    // A folder the library does not have is never written by the writer: nothing to
                    // sequence with there; a destination in the library is written from the model
                    // after the carry, as the writer would.
                    (_, to) => {
                        let r = dalica_store::carry::entries(&carry, now);
                        if let Some(to) = to.filter(|_| r.is_ok()) {
                            writer.mark_dirty(to);
                        }
                        r
                    }
                };
                let what = format!("sidecar entries from {} to {}", carry.from.display(), carry.to.display());
                let finished = matches!(&carried, Ok(c) if c.refused.is_empty());
                if let Some(flag) = unfinished.filter(|_| !finished) {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                match carried {
                    Ok(c) => {
                        for r in c.refused {
                            fail(format!("{what}: {} stayed: {}", r.name, r.reason));
                        }
                    }
                    Err(e) => fail(format!("{what}: {e}")),
                }
            }
            Ok(Msg::Hold(roots)) => writer.hold(roots),
            Ok(Msg::Guard(roots)) => writer.guard(roots),
            Ok(Msg::Flush(ack)) => {
                write(&mut writer, &states, true);
                let _ = ack.send(());
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                write(&mut writer, &states, true);
                return;
            }
        }
        write(&mut writer, &states, false);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dalica_core::SystemClock;
    use dalica_sidecar::{Field, Limits, Sidecar};
    use serde_json::json;

    use super::*;
    use crate::cmd::testing::{library, open};

    /// `roadmap.md` M1.13: star 3, undo 1, flush: the sidecar holds exactly 2.
    #[test]
    fn star_three_undo_one_flush_leaves_two_in_the_sidecar() {
        let (dirs, mut s, ids) = library(5);
        for id in &ids[..3] {
            s.execute("photo.star.toggle", &json!({ "ids": [id] })).unwrap();
        }
        s.execute("edit.undo", &json!({})).unwrap();
        s.flush().unwrap();
        let folder = dirs.photos.path().join("Lisbon");
        let sidecar = Sidecar::parse(&std::fs::read(folder.join(".dalica.json")).unwrap(), &Limits::default()).unwrap();
        let starred: Vec<&str> = sidecar.photo_names().filter(|n| sidecar.star(n) == Field::Value(true)).collect();
        assert_eq!(starred, ["IMG_0001.jpg", "IMG_0002.jpg"]);
    }

    #[test]
    fn a_second_session_on_the_same_library_is_locked_out() {
        let (dirs, mut s, ids) = library(2);
        s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
        assert!(matches!(Session::open(dirs.library.path(), Arc::new(SystemClock)), Err(OpenError::Locked(_))));
        drop(s);
        // Once the first is gone, the library opens with everything the journal recorded.
        let s = open(dirs.library.path());
        assert_eq!(s.recovery(), []);
        assert_eq!(s.library().photos().count(), 2);
        assert!(s.library().photo(ids[1]).unwrap().star);
    }

    /// Window and view state written through the session are there after a restart, beside the
    /// keymap, and an unchanged write sends nothing. The owner thread does no file I/O for it.
    #[test]
    fn changed_settings_survive_a_restart() {
        use crate::settings::{ViewState, WindowState};
        let (dirs, mut s, _ids) = library(1);
        let window = WindowState { position: Some([30.0, 40.0]), size: [1200.0, 800.0], maximized: true };
        let view = ViewState { tree_width: Some(300.0), pane: Some("properties".into()), pane_large: false };
        let ui = dalica_fs::mark_ui_thread();
        s.change_settings(|st| {
            st.set_window(&window);
            st.set_view(&view);
        });
        s.flush().unwrap();
        drop(ui);
        let file = dirs.library.path().join(crate::settings::FILE);
        let written = std::fs::metadata(&file).unwrap().modified().unwrap();
        s.change_settings(|st| st.set_view(&view));
        s.flush().unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), written, "nothing changed, nothing written");
        drop(s);
        let s = open(dirs.library.path());
        assert_eq!(s.settings().window(), Some(window));
        assert_eq!(s.settings().view(), view);
    }

    #[test]
    fn a_panicking_command_is_an_internal_error_and_the_session_goes_on() {
        let (_dirs, mut s, ids) = library(1);
        assert!(matches!(s.execute("test.panic", &json!({})), Err(CmdError::Internal(m)) if m.contains("deliberate")));
        s.execute("photo.star.toggle", &json!({ "ids": [ids[0]] })).unwrap();
        assert!(s.library().photo(ids[0]).unwrap().star);
        assert!(matches!(s.execute("no.such.command", &json!({})), Err(CmdError::UnknownCommand(_))));
    }

    /// The owning thread never touches the disk: commands, undo and flush work on a thread marked
    /// as the UI thread, where every `dalica-fs` call would fail.
    #[test]
    fn commands_and_flush_do_no_file_io_on_the_owner_thread() {
        let (dirs, mut s, ids) = library(3);
        let ui = dalica_fs::mark_ui_thread();
        s.execute("photo.star.toggle", &json!({ "ids": [ids[0], ids[2]] })).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.redo", &json!({})).unwrap();
        s.execute("library.query", &json!({ "starred": true })).unwrap();
        s.flush().unwrap();
        drop(ui);
        let folder = dirs.photos.path().join("Lisbon");
        let sidecar = Sidecar::parse(&std::fs::read(folder.join(".dalica.json")).unwrap(), &Limits::default()).unwrap();
        assert_eq!(sidecar.star("IMG_0003.jpg"), Field::Value(true));
    }

    #[test]
    fn commands_are_listed_with_their_specs() {
        let (_dirs, s, _) = library(1);
        let list = s.commands();
        let ids: Vec<&str> = list.iter().map(|c| c.id).collect();
        for id in
            ["folders.add", "library.query", "photo.inspect", "photo.star.toggle", "photo.star.set", "edit.undo", "edit.redo", "library.rebuild"]
        {
            assert!(ids.contains(&id), "{id}");
        }
        let star = list.iter().find(|c| c.id == "photo.star.toggle").unwrap();
        assert_eq!((star.label, star.menu, star.shortcut), ("Star / Unstar", "Picture/Star / Unstar", Some("Ctrl+8")));
        assert!(!list.iter().find(|c| c.id == "edit.undo").unwrap().enabled);
    }
}
