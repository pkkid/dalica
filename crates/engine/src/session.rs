//! The `Session` (`architecture.md` §4, §5): one open library — its model, undo stack, watched
//! folders and command registry — owned by one thread (the UI thread in the app, the main thread
//! in the CLI).
//!
//! That thread never touches the disk once the session is open. Everything that does — the index
//! journal and snapshots, sidecar writes (debounced on the clock), `folders.json`, scans — happens
//! on worker threads: the I/O worker started by [`Session::open`], and one thread per scan. Their
//! results come back through [`Session::poll`] (each UI frame) or [`Session::wait_idle`] (the CLI
//! and tests); [`Session::flush`] blocks until every pending write has landed.

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
use dalica_scan::Scan;
use dalica_store::{FolderState, Recovery, SidecarWriter, Store};
use serde_json::Value;

use crate::error::{CmdError, OpenError};
use crate::keymap::Keymap;
use crate::registry::{CommandInfo, CommandSpec, History};
use crate::settings::Settings;
use crate::undo::{Step, UndoStack};

/// The watched-folders file in the library directory (`data-model.md` §4).
pub const FOLDERS_FILE: &str = "folders.json";
/// A snapshot is taken after this many ops (the store's own threshold, counted here so the worker
/// is sent a copy of the model only when one is due).
const SNAPSHOT_EVERY: u64 = dalica_store::SNAPSHOT_OPS;
/// How often the I/O worker wakes to write sidecars that became due.
const TICK: Duration = Duration::from_millis(100);

/// Work for the I/O worker.
enum Msg {
    Journal(Vec<Op>),
    Dirty(Vec<(FolderId, Option<FolderState>)>),
    /// Atomically replace one of the library directory's own files (`folders.json`, `settings.json`).
    Write(PathBuf, Vec<u8>),
    Snapshot(Box<Library>),
    Flush(Sender<()>),
}

/// What a background scan was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanKind {
    /// New or rescanned roots: merge into the model.
    Merge,
    /// Rebuild the whole model from the roots.
    Rebuild,
}

/// One open library.
pub struct Session {
    dir: PathBuf,
    library: Library,
    roots: Vec<PathBuf>,
    undo: UndoStack,
    clock: Arc<dyn Clock>,
    registry: Vec<CommandSpec>,
    by_id: HashMap<&'static str, usize>,
    io: Option<Sender<Msg>>,
    worker: Option<JoinHandle<()>>,
    io_errors: Arc<Mutex<Vec<String>>>,
    scans: Vec<(ScanKind, Receiver<Scan>)>,
    collected: Option<Vec<Op>>,
    recovery: Vec<Recovery>,
    scan_warnings: Vec<String>,
    ops_since_snapshot: u64,
    settings: Settings,
    keymap: Keymap,
    notices: Vec<String>,
    _lock: LibraryLock,
}

impl Session {
    /// Opens (or creates) the library in `dir`: takes its lock (another process holding it gives
    /// [`OpenError::Locked`]), loads the index — repairing what it can, reported by
    /// [`Session::recovery`] — and the watched folders, and starts the I/O worker. If the index had
    /// to be dropped, a rebuild from the watched folders starts in the background.
    /// Call it off the UI thread.
    pub fn open(dir: &Path, clock: Arc<dyn Clock>) -> Result<Session, OpenError> {
        dalica_fs::create_dir_all(dir)?;
        let lock = LibraryLock::acquire(dir)?;
        let opened = Store::open(dir, clock.now())?;
        let (roots, folders_note) = read_roots(&dir.join(FOLDERS_FILE), clock.now());
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
        let mut session = Session {
            dir: dir.to_path_buf(),
            library: opened.library,
            roots,
            undo: UndoStack::default(),
            clock,
            registry,
            by_id,
            io: Some(tx),
            worker: Some(worker),
            io_errors,
            scans: Vec::new(),
            collected: None,
            recovery,
            scan_warnings: Vec::new(),
            ops_since_snapshot: 0,
            settings,
            keymap,
            notices: settings_note.into_iter().collect(),
            _lock: lock,
        };
        if session.recovery.iter().any(|r| matches!(r, Recovery::RebuildNeeded { .. })) && !session.roots.is_empty() {
            session.start_scan(session.roots.clone(), ScanKind::Rebuild);
        }
        Ok(session)
    }

    /// Runs the command `id` with JSON `params`: checks that it is enabled, runs it with panics
    /// caught, and records exactly one undo step (for commands on the library history that changed
    /// something). The only way library state changes.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CmdError> {
        let spec = *self.spec(id).ok_or_else(|| CmdError::UnknownCommand(id.to_string()))?;
        (spec.enabled)(self, params).map_err(|d| CmdError::Disabled(d.0))?;
        self.collected = Some(Vec::new());
        let result = catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params)));
        let mut inverses = self.collected.take().unwrap_or_default();
        if spec.history == History::Library && !inverses.is_empty() {
            let op = if inverses.len() == 1 {
                inverses.pop().unwrap_or(Op::Batch { ops: Vec::new() })
            } else {
                inverses.reverse();
                Op::Batch { ops: inverses }
            };
            self.undo.push(Step { label: spec.label, op });
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

    /// Applies `op` to the model, journals it, marks the folders it touched for a sidecar write, and
    /// (inside `execute`) collects its inverse for the undo step. Commands change the library
    /// only through this.
    pub(crate) fn apply(&mut self, op: Op) -> Result<Op, CmdError> {
        let inverse = self.library.apply(op.clone()).map_err(|e| match e {
            dalica_library::Error::UnknownFolder(_) | dalica_library::Error::UnknownPhoto(_) => CmdError::NotFound(e.to_string()),
            other => CmdError::BadParams(other.to_string()),
        })?;
        self.send(Msg::Journal(vec![op]));
        let rev = self.library.revision();
        let touched: BTreeSet<FolderId> = self.all_folder_ids().into_iter().filter(|f| self.library.folder_revision(*f) == rev).collect();
        let states = touched.into_iter().map(|f| (f, dalica_store::mapping::folder_state(&self.library, f))).collect();
        self.send(Msg::Dirty(states));
        if let Some(c) = &mut self.collected {
            c.push(inverse.clone());
        }
        self.ops_since_snapshot += 1;
        if self.ops_since_snapshot >= SNAPSHOT_EVERY {
            self.ops_since_snapshot = 0;
            self.send(Msg::Snapshot(Box::new(self.library.clone())));
        }
        Ok(inverse)
    }

    fn all_folder_ids(&self) -> Vec<FolderId> {
        self.library.folders().map(|f| f.id).collect()
    }

    fn send(&mut self, msg: Msg) {
        if let Some(io) = &self.io
            && io.send(msg).is_err()
        {
            self.io_errors.lock().unwrap_or_else(PoisonError::into_inner).push("the I/O worker stopped".into());
        }
    }

    /// Undoes the last library step. `Ok(None)` when there is nothing to undo.
    pub(crate) fn undo(&mut self) -> Result<Option<&'static str>, CmdError> {
        let Some(step) = self.undo.pop_undo() else { return Ok(None) };
        match self.apply(step.op.clone()) {
            Ok(redo) => {
                self.undo.push_redo(Step { label: step.label, op: redo });
                Ok(Some(step.label))
            }
            Err(e) => {
                self.undo.push_undo(step);
                Err(e)
            }
        }
    }

    /// Redoes the last undone step. `Ok(None)` when there is nothing to redo.
    pub(crate) fn redo(&mut self) -> Result<Option<&'static str>, CmdError> {
        let Some(step) = self.undo.pop_redo() else { return Ok(None) };
        match self.apply(step.op.clone()) {
            Ok(undo) => {
                self.undo.push_undo(Step { label: step.label, op: undo });
                Ok(Some(step.label))
            }
            Err(e) => {
                self.undo.push_redo(step);
                Err(e)
            }
        }
    }

    /// Adds a watched folder (saved in `folders.json`) and scans it in the background.
    pub(crate) fn add_root(&mut self, path: PathBuf) -> bool {
        let new = !self.roots.contains(&path);
        if new {
            self.roots.push(path.clone());
            let json =
                serde_json::json!({ "version": 1, "folders": self.roots.iter().map(|p| serde_json::json!({ "path": p })).collect::<Vec<_>>() });
            let mut bytes = serde_json::to_vec_pretty(&json).unwrap_or_default();
            bytes.push(b'\n');
            self.send(Msg::Write(self.dir.join(FOLDERS_FILE), bytes));
        }
        self.start_scan(vec![path], ScanKind::Merge);
        new
    }

    /// Rebuilds the model from the watched folders in the background (`library.rebuild`).
    pub(crate) fn start_rebuild(&mut self) {
        self.start_scan(self.roots.clone(), ScanKind::Rebuild);
    }

    fn start_scan(&mut self, roots: Vec<PathBuf>, kind: ScanKind) {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("dalica-scan".into()).spawn(move || {
            let _ = tx.send(dalica_scan::scan(&roots, None));
        });
        match spawned {
            Ok(_) => self.scans.push((kind, rx)),
            Err(e) => self.scan_warnings.push(format!("cannot start a scan: {e}")),
        }
    }

    /// Applies the results of finished background work (scans). Returns how many finished.
    pub fn poll(&mut self) -> usize {
        let mut done = Vec::new();
        self.scans.retain(|(kind, rx)| match rx.try_recv() {
            Ok(scan) => {
                done.push((*kind, scan));
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        let n = done.len();
        for (kind, scan) in done {
            self.apply_scan(kind, &scan);
        }
        n
    }

    /// Blocks until all background work (scans) has finished and its results are applied.
    pub fn wait_idle(&mut self) {
        for (kind, rx) in std::mem::take(&mut self.scans) {
            if let Ok(scan) = rx.recv() {
                self.apply_scan(kind, &scan);
            }
        }
    }

    fn apply_scan(&mut self, kind: ScanKind, scan: &Scan) {
        self.scan_warnings.extend(scan.warnings.iter().map(|w| format!("{}: {}", w.path.display(), w.reason)));
        match kind {
            ScanKind::Merge => {
                let ops = dalica_scan::diff(&mut self.library, scan);
                for op in ops {
                    if let Err(e) = self.apply(op) {
                        self.scan_warnings.push(format!("scan result not applied: {e}"));
                    }
                }
            }
            ScanKind::Rebuild => {
                let mut fresh = Library::new();
                for op in dalica_scan::diff(&mut fresh, scan) {
                    if let Err(e) = fresh.apply(op) {
                        self.scan_warnings.push(format!("rebuild: {e}"));
                    }
                }
                // The new model replaces the old one; ids may differ, so the undo stack goes.
                self.library = fresh;
                self.undo.clear();
                self.ops_since_snapshot = 0;
                self.send(Msg::Snapshot(Box::new(self.library.clone())));
            }
        }
    }

    /// Waits for background work, then until every journal record and sidecar write has landed.
    /// `Err(Unsaved)` lists writes that failed (they stay pending and are retried).
    pub fn flush(&mut self) -> Result<(), CmdError> {
        self.wait_idle();
        let (ack, done) = mpsc::channel();
        self.send(Msg::Flush(ack));
        let _ = done.recv();
        let errors = std::mem::take(&mut *self.io_errors.lock().unwrap_or_else(PoisonError::into_inner));
        if errors.is_empty() { Ok(()) } else { Err(CmdError::Unsaved(errors.join("; "))) }
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The watched folders.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
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

    /// Whether background work (a scan) is still running.
    pub fn busy(&self) -> bool {
        !self.scans.is_empty()
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

impl Drop for Session {
    /// Stops the I/O worker after it has written everything pending.
    fn drop(&mut self) {
        drop(self.io.take());
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// The watched folders from `folders.json`; a damaged file is kept aside and reported.
fn read_roots(path: &Path, now: dalica_core::Timestamp) -> (Vec<PathBuf>, Option<Recovery>) {
    let bytes = match dalica_fs::read(path, 64 << 20) {
        Ok(Some(r)) => r.bytes,
        Ok(None) => return (Vec::new(), None),
        Err(e) => return (Vec::new(), Some(Recovery::RebuildNeeded { reason: format!("folders.json: {e}") })),
    };
    let parsed: Option<Vec<PathBuf>> = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| v.get("folders")?.as_array()?.iter().map(|f| f.get("path")?.as_str().map(PathBuf::from)).collect());
    match parsed {
        Some(roots) => (roots, None),
        None => {
            let stamp = dalica_core::DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
            let kept = dalica_fs::set_aside(path, &format!("corrupt-{stamp}Z")).map(|p| p.display().to_string()).unwrap_or_default();
            (Vec::new(), Some(Recovery::RebuildNeeded { reason: format!("folders.json was damaged and kept as {kept}; add your folders again") }))
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
    let write = |writer: &mut SidecarWriter, states: &HashMap<FolderId, Option<FolderState>>, all: bool| {
        let state_of = |f: FolderId| states.get(&f).cloned().flatten();
        let outcomes = if all { writer.flush_with(state_of, clock.now()) } else { writer.write_due_with(state_of, clock.now()) };
        for (folder, outcome) in outcomes {
            match outcome {
                dalica_store::Outcome::Failed(e) => fail(format!("sidecar of folder {folder}: {e}")),
                dalica_store::Outcome::SetAside(p) => {
                    fail(format!("folder {folder}: damaged sidecar kept as {}; the folder is read-only", p.display()))
                }
                _ => {}
            }
        }
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
