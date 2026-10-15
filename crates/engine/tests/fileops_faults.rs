//! A file operation interrupted at any step loses nothing (`roadmap.md` M9.21; `data-model.md` §8
//! and §10.5, `architecture.md` §8).
//!
//! For each operation — a move on one volume, a move across volumes, a copy, a batch rename, a
//! delete, a folder move on one volume and across volumes — every fault point its steps reach is
//! armed, as an Error and as a Crash: `fs`'s points on the file job's thread (the transfer, the
//! trash, a backup's move), and the atomic-write points of the file-operation journal and of the
//! sidecar writes on the I/O worker, at each of their writes in turn. Each case runs in a child
//! process (this test binary run again with the case in [`CASE`]), so a Crash is a real stop: the
//! child exits at the step, before anything after it is written. The parent then reopens the
//! library: the repair runs at open, and afterwards every photo's bytes are at a path the library
//! lists (once for a move, a rename and a folder move; once or twice for a copy; at most once for a
//! delete, and otherwise gone from the folders), each with its caption and star in the model and
//! its sidecar entry (a key only another program knows included) and its originals backup beside
//! it; nothing is left in the journal; the owner thread made no `fs` call.
//!
//! The full sweep (`DALICA_FULL_TESTS=1`, the review gate) arms every point, each child making its
//! library through the session; a task's gate makes each operation's library once ([`template`])
//! and copies its folder into every case, and arms one point per class (a step before the operation's commit, the commit, a step after it), each as an
//! Error and as a Crash, and follows a point's writes after the last item twice instead of
//! [`MAX_NTH`] times: `dalica-fs` and `dalica-store` sweep every point of a move, a copy and an
//! atomic write on their own. The children of all the sweeps share [`SLOTS`], one per processor.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use dalica_core::{Timestamp, VirtualClock};
use dalica_engine::fileops::{ItemState, Journal, JournalEntry, OpId, StoreJournal};
use dalica_engine::fileops_recovery::RepairOutcome;
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_fs::faults::{self, Fault, points};
use dalica_store::intents::Intents;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[allow(dead_code)]
mod common;

use common::{entry, full, jpeg};

const PATIENCE: Duration = Duration::from_secs(120);
/// The environment variable that makes this binary's [`child`] run one case.
const CASE: &str = "DALICA_FILEOPS_FAULT_CASE";
/// The child stopped at the armed step.
const CRASHED: i32 = 70;
/// The armed step was never reached.
const NOT_REACHED: i32 = 71;
/// The most writes of one point counted after the last item (the full sweep).
const MAX_NTH: usize = 16;
/// The same in a task's gate.
const QUICK_NTH: usize = 2;

/// The operations of the sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Operation {
    Move,
    MoveAcross,
    Copy,
    Rename,
    Delete,
    MoveFolder,
    MoveFolderAcross,
}

/// Where a fault point is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum On {
    /// The file job's thread (armed for whichever thread reaches it first; only that one does).
    Job,
    /// The I/O worker (armed on that thread, counting its writes).
    Io,
}

/// One case.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Case {
    op: Operation,
    point: String,
    crash: bool,
    on: On,
    /// Armed when the operation's journal entry is written (`None`), or once item `k` is done.
    after: Option<usize>,
    /// On the I/O worker: the `nth` time it reaches the point from then on.
    nth: usize,
    root: PathBuf,
}

impl Case {
    fn name(&self) -> String {
        let when = self.after.map_or("at the start".to_string(), |k| format!("after item {k}"));
        format!("{:?} {} {} {:?} {when} #{}", self.op, self.point, if self.crash { "Crash" } else { "Error" }, self.on, self.nth)
    }
}

impl Operation {
    fn items(self) -> usize {
        match self {
            Operation::MoveFolder | Operation::MoveFolderAcross => 1,
            _ => 2,
        }
    }

    fn across(self) -> bool {
        matches!(self, Operation::MoveAcross | Operation::MoveFolderAcross)
    }

    /// The fault points the operation's last item reaches on the job's thread.
    fn job_points(self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = match self {
            Operation::Move | Operation::Rename => faults::MOVE_POINTS.to_vec(),
            Operation::MoveAcross => {
                let mut v = faults::COPY_POINTS.to_vec();
                v.extend(faults::FINISH_POINTS);
                // The photo's copy takes the switch, so its backup moves on one volume.
                v.extend([points::MOVE_CROSS_VOLUME, points::COPY_FLIP, points::TRASH_NO_TRASH, points::MOVE_RENAME, points::MOVE_FSYNC_DIR]);
                v
            }
            Operation::Copy => {
                let mut v = faults::COPY_POINTS.to_vec();
                v.push(points::COPY_FLIP);
                v
            }
            Operation::Delete => vec![points::TRASH_NO_TRASH, points::TRASH_SEND],
            Operation::MoveFolder => faults::FOLDER_MOVE_POINTS.to_vec(),
            Operation::MoveFolderAcross => {
                let mut v = faults::TREE_POINTS.to_vec();
                v.extend([points::MOVE_CROSS_VOLUME, points::COPY_FLIP, points::TRASH_NO_TRASH]);
                v
            }
        };
        let mut seen = Vec::new();
        v.retain(|p| {
            let new = !seen.contains(p);
            seen.push(*p);
            new
        });
        if !full() {
            let quick = self.quick_points();
            v.retain(|p| quick.contains(p));
        }
        v
    }

    /// A task's gate: one job-thread point per class, a step before the commit, the commit and a
    /// step after it (a delete has only its two).
    fn quick_points(self) -> [&'static str; 3] {
        match self {
            Operation::Move | Operation::Rename => [points::MOVE_CHECK, points::MOVE_RENAME, points::MOVE_VERIFY],
            Operation::MoveAcross => [points::COPY_WRITE, points::COPY_COMMIT, points::TRASH_SEND],
            Operation::Copy => [points::COPY_WRITE, points::COPY_COMMIT, points::MOVE_VERIFY],
            Operation::Delete => [points::TRASH_NO_TRASH, points::TRASH_SEND, points::TRASH_SEND],
            Operation::MoveFolder => [points::FOLDER_CHECK, points::FOLDER_RENAME, points::FOLDER_FSYNC_DIR],
            Operation::MoveFolderAcross => [points::TREE_WRITE, points::TREE_COMMIT, points::TRASH_SEND],
        }
    }

    /// How many times its photo `i` is listed after the repair when items before `done` had
    /// finished before the fault: a copy made is listed twice, a photo trashed not at all; for the
    /// others the repair decides.
    fn listed(self, i: usize, done: usize) -> std::ops::RangeInclusive<usize> {
        match self {
            Operation::Copy if i < done => 2..=2,
            Operation::Copy => 1..=2,
            Operation::Delete if i < done => 0..=0,
            Operation::Delete => 0..=1,
            _ => 1..=1,
        }
    }

    /// The photos it is about (names under `Pictures`).
    fn photos(self) -> &'static [&'static str] {
        match self {
            Operation::Rename => &["Lisbon/IMG_0002.jpg", "Porto/PORTO_0001.jpg"],
            _ => &["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg"],
        }
    }
}

/// Every fault point by name.
fn point(name: &str) -> &'static str {
    let mut all: Vec<&'static str> = faults::ATOMIC_POINTS.to_vec();
    all.extend(faults::COPY_POINTS);
    all.extend(faults::MOVE_POINTS);
    all.extend(faults::FINISH_POINTS);
    all.extend(faults::FOLDER_MOVE_POINTS);
    all.extend(faults::TREE_POINTS);
    all.extend([points::MOVE_CROSS_VOLUME, points::COPY_FLIP, points::TRASH_NO_TRASH, points::TRASH_SEND]);
    all.into_iter().find(|p| *p == name).unwrap_or_else(|| panic!("no fault point {name}"))
}

fn open(library: &Path) -> Session {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_790_000_000_000)));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(library, clock, &options).unwrap();
    s.set_watching(WatchMode::Off);
    s
}

/// What the library said about a photo before the operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Reference {
    /// Its path under `Pictures` before.
    rel: String,
    bytes: Vec<u8>,
    caption: String,
    /// A key only another program knows, in its sidecar entry.
    other: Option<Value>,
    /// Its originals backup's bytes.
    backup: Option<Vec<u8>>,
}

const OTHER: &str = "x-other";

/// `Pictures/Lisbon` (IMG_0001, IMG_0002 with a saved original and a foreign key), `Pictures/Porto`
/// (PORTO_0001) and `Pictures/Archive` (OLD_0001), every photo starred and captioned through the
/// session (the foreign key and the original written behind its back), and `refs.json`, what it
/// says about each photo: the session.
fn prepare(root: &Path) -> Session {
    let pictures = root.join("Pictures");
    let files = [("Lisbon", "IMG_0001.jpg", 1), ("Lisbon", "IMG_0002.jpg", 2), ("Porto", "PORTO_0001.jpg", 3), ("Archive", "OLD_0001.jpg", 4)];
    for (folder, name, seed) in files {
        let dir = pictures.join(folder);
        dalica_fs::create_dir_all(&dir).unwrap();
        dalica_fs::replace(&dir.join(name), &jpeg(seed), Expect::Absent).unwrap();
    }
    let mut s = open(&root.join("library"));
    s.execute("folders.add", &json!({ "path": pictures })).unwrap();
    s.wait_idle();
    let ids: Vec<_> = s.library().photos().map(|p| p.id).collect();
    s.execute("photo.star.set", &json!({ "ids": ids, "on": true })).unwrap();
    let mut refs = Vec::new();
    for (folder, name, _) in files {
        let f = s.library().folder_by_path(&pictures.join(folder)).unwrap();
        let id = s.library().photo_by_name(f, name).unwrap().id;
        let caption = format!("The caption of {name}");
        s.execute("photo.caption.set", &json!({ "ids": [id], "caption": caption })).unwrap();
        refs.push(Reference { rel: format!("{folder}/{name}"), bytes: Vec::new(), caption, other: None, backup: None });
    }
    assert!(s.flush_metadata(PATIENCE));
    s.flush().unwrap();
    // IMG_0002: a saved original with its record, and a key only another program knows.
    let lisbon = pictures.join("Lisbon");
    let originals = dalica_fs::originals::ensure_dir(&lisbon).unwrap();
    std::fs::write(originals.join("IMG_0002.jpg"), b"the original of IMG_0002").unwrap();
    let record = json!({ "dalica": { "version": 1 }, "originals": { "IMG_0002.jpg": {
        "recipe": ["sepia()"], "size": 24, "mtime": 1, "width": 4, "height": 3, "digest": "", "state": "done" } } });
    std::fs::write(originals.join(".dalica.json"), serde_json::to_vec(&record).unwrap()).unwrap();
    let sidecar = lisbon.join(".dalica.json");
    let mut root_json: Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
    let other = json!({ "rating": 5, "by": "another program" });
    root_json["photos"]["IMG_0002.jpg"][OTHER] = other.clone();
    std::fs::write(&sidecar, serde_json::to_vec(&root_json).unwrap()).unwrap();
    for r in &mut refs {
        r.bytes = std::fs::read(pictures.join(&r.rel)).unwrap();
        if r.rel == "Lisbon/IMG_0002.jpg" {
            r.other = Some(other.clone());
            r.backup = Some(b"the original of IMG_0002".to_vec());
        }
    }
    std::fs::write(root.join("refs.json"), serde_json::to_vec(&refs).unwrap()).unwrap();
    s
}

/// The full sweep's library ([`prepare`], in the child) or a task's gate's: [`template`]'s copy
/// (made by the parent) added to a new library and scanned, its sidecars bringing the stars, the
/// captions and the foreign key.
fn setup(root: &Path) -> Session {
    if full() {
        return prepare(root);
    }
    let mut s = open(&root.join("library"));
    s.execute("folders.add", &json!({ "path": root.join("Pictures") })).unwrap();
    s.wait_idle();
    s
}

/// A task's gate: the library of [`prepare`] made once per operation under `root`, without its
/// index (a library keeps absolute paths), to be copied into each case.
fn template(root: &Path) {
    drop(prepare(root));
    std::fs::remove_dir_all(root.join("library")).unwrap();
}

/// Copies the folder `from` into `to`, everything in it included.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

// --- the child ----------------------------------------------------------------------------------

/// Set by the hook that fired on the I/O worker.
static REACHED: AtomicBool = AtomicBool::new(false);

/// Arms `point` on the calling thread for the `nth` time it reaches it: a Crash stops the process
/// right there (before the step); an Error fails that step.
fn arm_nth_here(point: &'static str, nth: usize, crash: bool) {
    fn fire(point: &'static str, crash: bool) {
        REACHED.store(true, Ordering::SeqCst);
        if crash {
            std::process::exit(CRASHED);
        }
        faults::arm(point, Fault::Error);
    }
    fn wait(point: &'static str, left: usize, crash: bool) {
        if left <= 1 {
            faults::run_at(point, move || fire(point, crash));
        } else {
            faults::run_at(point, move || wait(point, left - 1, crash));
        }
    }
    wait(point, nth, crash);
}

/// The session's journal, which arms the case's fault at its moment and stops the process when a
/// Crash on the job's thread fired.
struct Probe {
    inner: StoreJournal,
    case: Case,
    items: usize,
    /// The item the job-thread fault is meant for (the last).
    target: usize,
    target_done: AtomicBool,
}

impl Probe {
    /// The moment `at` came (the entry written: `None`; item `k` done: `Some(k)`): arms what is due.
    fn anchor(&self, at: Option<usize>) {
        let next = at.map_or(0, |k| k + 1);
        let here = at == self.case.after;
        let point = point(&self.case.point);
        // Each photo or folder of a cross-volume operation takes the switch, unless the case's
        // Crash is at the switch itself.
        if self.case.op.across() && next < self.items && !(here && self.case.on == On::Job && point == points::MOVE_CROSS_VOLUME) {
            faults::arm_global(points::MOVE_CROSS_VOLUME, Fault::Error);
        }
        if !here {
            return;
        }
        match self.case.on {
            On::Job => faults::arm_global(point, if self.case.crash { Fault::Crash } else { Fault::Error }),
            On::Io => arm_nth_here(point, self.case.nth, self.case.crash),
        }
    }

    /// A journal call after the target item (`None`: the end) while it never got done: the Crash
    /// fired on the job's thread, and the process stops before anything else is written.
    fn stop_if_crashed(&self, index: Option<usize>) {
        if self.case.on != On::Job || !self.case.crash || self.target_done.load(Ordering::SeqCst) {
            return;
        }
        if index.is_none_or(|i| i > self.target) {
            std::process::exit(CRASHED);
        }
    }
}

impl Journal for Probe {
    fn begin(&self, entry: &JournalEntry) -> Result<(), String> {
        self.anchor(None);
        self.inner.begin(entry)
    }

    fn item(&self, op: OpId, index: usize, state: ItemState) -> Result<(), String> {
        self.stop_if_crashed(Some(index));
        let r = self.inner.item(op, index, state);
        if state == ItemState::Done {
            if index == self.target {
                self.target_done.store(true, Ordering::SeqCst);
            }
            self.anchor(Some(index));
        }
        r
    }

    fn end(&self, op: OpId) -> Result<(), String> {
        self.stop_if_crashed(None);
        self.inner.end(op)
    }

    fn retarget(&self, op: OpId, index: usize, to: &Path) -> Result<(), String> {
        self.inner.retarget(op, index, to)
    }
}

/// Stops the process when a step on the job's thread crashed and the engine logged it (an
/// originals backup that could not travel): a Crash is a stop, not a failure to report.
struct CrashLogger;

impl log::Log for CrashLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.args().to_string().contains("simulated crash at fault point") {
            std::process::exit(CRASHED);
        }
    }

    fn flush(&self) {}
}

/// Runs this binary's [`child`] for `case`; its exit code and what it printed.
fn spawn(case: &Case) -> (i32, String) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["child", "--exact", "--nocapture", "--test-threads=1"])
        .env(CASE, serde_json::to_string(case).unwrap())
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

/// The child: sets the library up, runs the operation with the case's fault armed, and exits with
/// [`CRASHED`], [`NOT_REACHED`] or 0. Without [`CASE`] (the ordinary test run) it does nothing.
#[test]
fn child() {
    let Ok(text) = std::env::var(CASE) else { return };
    let case: Case = serde_json::from_str(&text).unwrap();
    let mut s = setup(&case.root);
    if case.crash && case.on == On::Job {
        static LOGGER: CrashLogger = CrashLogger;
        log::set_logger(&LOGGER).unwrap();
        log::set_max_level(log::LevelFilter::Warn);
    }
    let library = case.root.join("library");
    let items = case.op.items();
    let probe = Arc::new(Probe {
        inner: StoreJournal::new(Intents::open(&library, Timestamp::from_millis(0)).unwrap().intents),
        target: items - 1,
        items,
        case: case.clone(),
        target_done: AtomicBool::new(false),
    });
    s.set_file_op_journal(probe.clone());
    let pictures = case.root.join("Pictures");
    let folder = |s: &Session, rel: &str| s.library().folder_by_path(&pictures.join(rel)).unwrap();
    let photo = |s: &Session, rel: &str| {
        let (f, n) = rel.split_once('/').unwrap();
        s.library().photo_by_name(folder(s, f), n).unwrap().id
    };
    let ids: Vec<_> = case.op.photos().iter().map(|r| photo(&s, r)).collect();
    let (id, params) = match case.op {
        Operation::Move | Operation::MoveAcross => ("photo.moveTo", json!({ "ids": ids, "folder": folder(&s, "Porto"), "confirm": true })),
        Operation::Copy => ("photo.copyTo", json!({ "ids": ids, "folder": folder(&s, "Porto") })),
        Operation::Rename => ("photo.rename", json!({ "ids": ids, "name": "Trip" })),
        Operation::Delete => ("photo.delete", json!({ "ids": ids, "confirm": true })),
        Operation::MoveFolder | Operation::MoveFolderAcross => {
            ("folder.move", json!({ "folder": folder(&s, "Lisbon"), "to": folder(&s, "Archive") }))
        }
    };
    s.execute(id, &params).unwrap();
    assert!(s.wait_file_jobs(PATIENCE));
    let _ = s.flush_metadata(PATIENCE);
    let _ = s.flush();
    drop(s);
    let reached = match (case.on, case.crash) {
        // A Crash that fired never gets here.
        (On::Job, true) | (On::Io, true) => false,
        (On::Io, false) => REACHED.load(Ordering::SeqCst),
        (On::Job, false) => true,
    };
    std::process::exit(if reached { 0 } else { NOT_REACHED });
}

// --- the parent ---------------------------------------------------------------------------------

fn record(folder: &Path, name: &str) -> Option<Value> {
    let bytes = std::fs::read(folder.join(".dalica-originals").join(".dalica.json")).ok()?;
    serde_json::from_slice::<Value>(&bytes).ok()?.get("originals")?.get(name).cloned()
}

/// Every file under `dir` with `bytes`.
fn files_with(dir: &Path, bytes: &[u8]) -> Vec<PathBuf> {
    let walk = dalica_fs::walk(dir, dalica_fs::Limits::default(), |_| false).unwrap();
    walk.entries.into_iter().filter(|e| e.is_file && std::fs::read(&e.path).is_ok_and(|b| b == bytes)).map(|e| e.path).collect()
}

/// Reopens the library after `case` ran and checks that nothing was lost.
fn verify(case: &Case) -> Result<Vec<RepairOutcome>, String> {
    let refs: Vec<Reference> = serde_json::from_slice(&std::fs::read(case.root.join("refs.json")).map_err(|e| format!("no refs: {e}"))?).unwrap();
    let library = case.root.join("library");
    let pictures = case.root.join("Pictures");
    let mut s = open(&library);
    let ui = dalica_fs::mark_ui_thread();
    let refused = dalica_fs::guard::refused();
    if !s.wait_file_jobs(PATIENCE) {
        return Err("the repair did not end".into());
    }
    s.wait_idle();
    let flushed = s.flush();
    let repairs: Vec<_> = s.file_op_repairs().map(|r| r.outcome).collect();
    let owner_fs = dalica_fs::guard::refused() - refused;
    drop(ui);
    let mut problems = Vec::new();
    if owner_fs != 0 {
        problems.push(format!("the owner thread made {owner_fs} fs calls"));
    }
    if let Err(e) = flushed {
        problems.push(format!("writes left unsaved: {e}"));
    }
    if !s.file_ops_left().is_empty() {
        problems.push(format!("left to repair: {:?}", s.file_ops_left()));
    }
    if let Ok(bytes) = std::fs::read(library.join("file-ops.json")) {
        let journal: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if journal["ops"].as_array().is_none_or(|ops| !ops.is_empty()) {
            problems.push(format!("the journal still holds {journal}"));
        }
    }
    // What the library lists, with the bytes at each path.
    let lib = s.library();
    let mut listed: Vec<(PathBuf, dalica_core::PhotoId, Vec<u8>)> = Vec::new();
    for p in lib.photos() {
        let path = lib.folder(p.folder).unwrap().path.join(&p.name);
        match std::fs::read(&path) {
            Ok(bytes) => listed.push((path, p.id, bytes)),
            Err(_) => problems.push(format!("{} is listed but not there", path.display())),
        }
    }
    // The items done before the fault: those before the last on the job's thread, all of them after
    // the last item, none when it is the journal's first write.
    let done = match (case.on, case.after) {
        (_, None) => 0,
        (On::Job, Some(k)) => k + 1,
        (On::Io, Some(_)) => case.op.items(),
    };
    for r in &refs {
        let range = match case.op.photos().iter().position(|p| *p == r.rel) {
            Some(i) => case.op.listed(i, done),
            None => 1..=1,
        };
        let at: Vec<_> = listed.iter().filter(|(_, _, b)| *b == r.bytes).collect();
        if !range.contains(&at.len()) {
            problems.push(format!("{} is listed {} times, at {:?}", r.rel, at.len(), at.iter().map(|x| &x.0).collect::<Vec<_>>()));
        }
        if at.is_empty() {
            let still = files_with(&pictures, &r.bytes);
            if !still.is_empty() {
                problems.push(format!("{} is not listed but its bytes are at {still:?}", r.rel));
            }
        }
        for (path, id, _) in at {
            let (folder, name) = (path.parent().unwrap(), path.file_name().unwrap().to_str().unwrap());
            let (info, photo) = (lib.info(*id), lib.photo(*id).unwrap());
            if info.caption != r.caption || !photo.star {
                problems.push(format!("{}: the library shows caption {:?}, star {}", path.display(), info.caption, photo.star));
            }
            match entry(folder, name) {
                None => problems.push(format!("{}: no sidecar entry", path.display())),
                Some(e) => {
                    if e["caption"] != json!(r.caption) {
                        problems.push(format!("{}: the sidecar entry is {e}", path.display()));
                    }
                    if let Some(other) = &r.other
                        && e[OTHER] != *other
                    {
                        problems.push(format!("{}: the sidecar entry lost {OTHER}: {e}", path.display()));
                    }
                }
            }
            if let Some(backup) = &r.backup
                && case.op != Operation::Delete
            {
                let kept = std::fs::read(folder.join(".dalica-originals").join(name)).ok();
                if kept.as_ref() != Some(backup) || record(folder, name).is_none() {
                    let anywhere = files_with(&pictures, backup);
                    problems.push(format!(
                        "{}: its originals backup is not beside it (record {:?}; the backup is at {anywhere:?})",
                        path.display(),
                        record(folder, name)
                    ));
                }
            }
        }
    }
    if problems.is_empty() { Ok(repairs) } else { Err(problems.join("\n  ")) }
}

fn processors() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get()).max(2)
}

/// The children running at once, across every sweep of this process: one per processor.
struct Slots {
    used: Mutex<usize>,
    freed: Condvar,
}

static SLOTS: Slots = Slots { used: Mutex::new(0), freed: Condvar::new() };

impl Slots {
    /// Runs `f` once a slot is free.
    fn with<T>(&self, f: impl FnOnce() -> T) -> T {
        let mut used = self.used.lock().unwrap();
        while *used >= processors() {
            used = self.freed.wait(used).unwrap();
        }
        *used += 1;
        drop(used);
        let out = f();
        *self.used.lock().unwrap() -= 1;
        self.freed.notify_one();
        out
    }
}

/// What one case gave.
struct Outcome {
    case: Case,
    code: i32,
    printed: String,
    /// The repairs the reopened session made, or what was lost.
    verified: Result<Vec<RepairOutcome>, String>,
}

/// Runs `case` in a child (on a copy of `template` in a task's gate), then verifies the library.
fn run(mut case: Case, template: Option<&Path>) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    case.root = dir.path().to_path_buf();
    if let Some(template) = template {
        copy_tree(template, &case.root);
    }
    let (code, printed) = SLOTS.with(|| spawn(&case));
    let verified = verify(&case);
    Outcome { case, code, printed, verified }
}

/// The cases of `op`: every job-thread point of its last item, and every atomic write on the I/O
/// worker when its journal entry is written; those after its last item are found one write at a
/// time (`more`).
fn cases(op: Operation) -> Vec<Case> {
    let items = op.items();
    let last = items.checked_sub(2);
    let mut v = Vec::new();
    let case = |point: &'static str, crash, on, after, nth| Case { op, point: point.to_string(), crash, on, after, nth, root: PathBuf::new() };
    for p in op.job_points() {
        for crash in [false, true] {
            // As an Error the switch is the cross-volume path itself.
            if p == points::MOVE_CROSS_VOLUME && !crash {
                continue;
            }
            v.push(case(p, crash, On::Job, last, 1));
        }
    }
    // A task's gate: before the commit, the commit, after it.
    let atomic: &[&'static str] = if full() { &faults::ATOMIC_POINTS } else { &[points::WRITE, points::RENAME, points::FSYNC_DIR] };
    for &p in atomic {
        for crash in [false, true] {
            v.push(case(p, crash, On::Io, None, 1));
            v.push(case(p, crash, On::Io, Some(items - 1), 1));
        }
    }
    v
}

/// Runs every case of `op` on a few threads; a Crash on the I/O worker after the last item that
/// fired is followed by the next write of its point. Panics with every case that lost something.
fn sweep(op: Operation) {
    let made = tempfile::tempdir().unwrap();
    let template = (!full()).then(|| {
        template(made.path());
        made.path()
    });
    let queue: Mutex<VecDeque<Case>> = Mutex::new(cases(op).into());
    let outcomes: Mutex<Vec<Outcome>> = Mutex::new(Vec::new());
    let busy = AtomicUsize::new(0);
    let threads = processors();
    let max_nth = if full() { MAX_NTH } else { QUICK_NTH };
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().pop_front();
                    let Some(case) = next else {
                        if busy.load(Ordering::SeqCst) == 0 {
                            return;
                        }
                        std::thread::yield_now();
                        continue;
                    };
                    busy.fetch_add(1, Ordering::SeqCst);
                    let o = run(case, template);
                    // The next write of the same point, while this one was reached.
                    if o.case.on == On::Io && o.case.after.is_some() && o.code != NOT_REACHED && o.case.nth < max_nth {
                        let mut more = o.case.clone();
                        more.nth += 1;
                        queue.lock().unwrap().push_back(more);
                    }
                    outcomes.lock().unwrap().push(o);
                    busy.fetch_sub(1, Ordering::SeqCst);
                }
            });
        }
    });
    let outcomes = outcomes.into_inner().unwrap();
    let mut failures = Vec::new();
    let mut fired: BTreeMap<String, usize> = BTreeMap::new();
    for o in &outcomes {
        let expected = match (o.case.on, o.case.crash) {
            (_, true) => [CRASHED, NOT_REACHED].contains(&o.code),
            (On::Io, false) => [0, NOT_REACHED].contains(&o.code),
            (On::Job, false) => o.code == 0,
        };
        if !expected {
            failures.push(format!("{}: the child exited with {}:\n{}", o.case.name(), o.code, o.printed));
        }
        if o.code != NOT_REACHED {
            *fired.entry(o.case.point.clone()).or_default() += 1;
        }
        if let Err(e) = &o.verified {
            failures.push(format!("{}: {e}", o.case.name()));
        }
    }
    // Every job-thread point of the operation was reached, as an Error and as a Crash, and every
    // atomic point at the start.
    for o in outcomes.iter().filter(|o| o.case.nth == 1 && (o.case.on == On::Job || o.case.after.is_none())) {
        if o.code == NOT_REACHED {
            failures.push(format!("{}: never reached", o.case.name()));
        }
    }
    let ran = outcomes.len();
    let reached = outcomes.iter().filter(|o| o.code != NOT_REACHED).count();
    let mut repaired: BTreeMap<String, usize> = BTreeMap::new();
    for r in outcomes.iter().filter_map(|o| o.verified.as_ref().ok()).flatten() {
        *repaired.entry(format!("{r:?}")).or_default() += 1;
    }
    let reopened = outcomes.iter().filter(|o| o.verified.as_ref().is_ok_and(|r| !r.is_empty())).count();
    println!("{op:?}: {ran} cases run, {reached} reached their step, {reopened} repaired at reopen (items: {repaired:?}); by point: {fired:?}");
    assert!(failures.is_empty(), "{op:?}: {} of {ran} cases failed:\n{}", failures.len(), failures.join("\n\n"));
}

#[test]
fn a_move_on_one_volume_interrupted_at_any_step_loses_nothing() {
    sweep(Operation::Move);
}

#[test]
fn a_move_across_volumes_interrupted_at_any_step_loses_nothing() {
    sweep(Operation::MoveAcross);
}

#[test]
fn a_copy_interrupted_at_any_step_loses_nothing() {
    sweep(Operation::Copy);
}

#[test]
fn a_batch_rename_interrupted_at_any_step_loses_nothing() {
    sweep(Operation::Rename);
}

#[test]
fn a_delete_interrupted_at_any_step_loses_nothing() {
    sweep(Operation::Delete);
}

#[test]
fn a_folder_move_on_one_volume_interrupted_at_any_step_loses_nothing() {
    sweep(Operation::MoveFolder);
}

#[test]
fn a_folder_move_across_volumes_interrupted_at_any_step_loses_nothing() {
    sweep(Operation::MoveFolderAcross);
}
