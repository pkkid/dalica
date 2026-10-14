//! Watching the Scan Always units for external changes (`data-model.md` §7): events are hints,
//! verification is the truth.
//!
//! - **The watcher** (`dalica_scan::watch::Watching`: the system's events, or the pure-Rust
//!   poller where those cannot be had) runs on a thread of its own ("dalica-watch"), never on the
//!   owner thread. It watches the Scan Always units (the outermost ones; folders set to Remove or
//!   Scan Once inside them are left out) and is restarted when they change: the new watcher is up
//!   before the old one goes, so no change falls between them. [`Session::poll`] sends it a tick
//!   with the session time every [`TICK`] (debouncing and the poller's sweeps run on the session
//!   clock, so tests drive them with virtual time) and takes in the dirty folders it reports.
//! - **Rescans:** dirty folders are verified by an incremental rescan (`watch::rescan`, one per
//!   unit, with the unit's region rules) on a thread of its own, listed as a `scan` job. Its
//!   result is applied through `dalica_scan::diff_rescan` a budget of photos per `poll`, diffed
//!   again whenever something else changed the model in between (the diff is idempotent), so an
//!   edit made meanwhile is never overwritten. Folder renames and moves keep their ids, photos and
//!   stars; a folder moved in from elsewhere brings its stars in its sidecar. Rescans wait while a
//!   scan runs (the scan sees the same changes; a folder it removed is found again by the rescan
//!   after it) and while a file job writes; a rescan running when a file job starts is put off
//!   (`Session::put_off_rescan`), as a full scan is (`Session::put_off_scans`, M9.28). Only
//!   scopes walked to the end remove anything, and a unit that
//!   holds no entries at all (an unmounted volume's empty mount point) removes nothing.
//! - **The verification pass** runs on the session clock: each unit of
//!   `Rules::verification` is rescanned whole (a region scan, [`crate::scanning`]) once
//!   [`FULL_VERIFICATION`] has passed since a scan last walked it, and [`FIRST_VERIFICATION`]
//!   after the session's first poll for units no scan walked since the session opened (changes
//!   made while the app was closed). A unit under a root found offline waits for it; one a
//!   verification could not walk is tried again after [`RETRY`]. With the watcher switched off ([`WatchMode::Off`]) the verification pass
//!   alone finds every change.
//! - **Upkeep:** every folder a scan or rescan hands out has its stale `.dalica-tmp-*` files
//!   removed on that scan's thread ([`clean_temps`], `security.md` §5.8). A photo deleted outside
//!   Dalica leaves the library at once; its sidecar entry is pruned by the writer 30 days later
//!   (`dalica_store::stale`).
//!
//! The owner thread never touches the disk here: the watcher, the rescans and the clean-up all run
//! on worker threads, and their results come back through `poll`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, UNIX_EPOCH};

use dalica_core::Timestamp;
use dalica_library::Op;
use dalica_scan::watch::{self, Backend, DirtyFolder, Fallback, Prefer, Rescan, WatchOptions, Watcher};
use dalica_scan::{Mode, Plan};
use serde_json::json;

use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::scanning::{Budget, PHOTOS_PER_POLL, cost, pictures};
use crate::session::Session;

pub use dalica_scan::watch::FULL_VERIFICATION;

/// How long after the session's first poll the units no scan walked since it opened are verified.
pub const FIRST_VERIFICATION: Duration = Duration::from_secs(60);
/// How soon a unit whose verification could not walk it (offline) is tried again.
pub const RETRY: Duration = Duration::from_secs(60);
/// How often `poll` asks the watcher for dirty folders, on the session clock.
pub const TICK: Duration = Duration::from_millis(100);

/// How the Scan Always units are watched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WatchMode {
    /// Not watched: only the verification pass finds changes.
    Off,
    /// The system's change events where they work, polling elsewhere (the default).
    #[default]
    Native,
    /// Polling only (tests drive it with virtual time).
    Polling,
}

enum Request {
    /// Watch these roots (the options leave folders out); replaces the current watcher.
    Start {
        roots: Vec<PathBuf>,
        options: WatchOptions,
    },
    Tick {
        seq: u64,
        now: Timestamp,
    },
}

enum Answer {
    Started {
        backends: Vec<(PathBuf, Backend)>,
        fallbacks: Vec<Fallback>,
    },
    /// The dirty folders that settled; `failed`: the watcher panicked and was dropped (whatever
    /// it knew is lost, so its roots are verified whole).
    Ticked {
        seq: u64,
        dirty: Vec<DirtyFolder>,
        backends: Vec<(PathBuf, Backend)>,
        failed: bool,
    },
}

struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Answer>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// Lets the thread end on its own (the owner never waits for it while running).
    fn detach(mut self) {
        self.thread = None;
    }
}

impl Drop for Worker {
    /// Stops the watcher thread and, unless detached, waits for it, so its watches are gone (and
    /// the folders free to move, on Windows) once the session is.
    fn drop(&mut self) {
        let (tx, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.tx, tx));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// One unit's incremental rescan, as the rescan thread sends it.
struct Checked {
    unit: PathBuf,
    rescan: Rescan,
    /// The unit holds no entries at all: its scope was dropped, nothing is removed.
    empty: bool,
    /// Stale temporary files that could not be removed.
    problems: Vec<String>,
}

/// The running rescan, as the owner sees it.
struct RescanJob {
    job: JobId,
    /// `None` once the thread ended (or the job was cancelled).
    rx: Option<Receiver<Checked>>,
    queue: VecDeque<Checked>,
    /// The unit being applied and the ops still to apply.
    current: Option<Checked>,
    pending: VecDeque<Op>,
    /// The library revision after the last diff or op of ours: another value means the model
    /// changed meanwhile and the unit is diffed again.
    revision: u64,
    units: usize,
    done: usize,
    folders: usize,
    changed: usize,
    cancelled: bool,
    /// The job's cancel flag, which the thread's walks check.
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The dirty folders it verifies, reported again when a file job puts it off
    /// (`Session::put_off_rescan`).
    dirty: Vec<DirtyFolder>,
}

impl Drop for RescanJob {
    /// Stops the rescan thread at its next file and waits for it.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.rx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl RescanJob {
    fn receive(&mut self, block: bool) {
        let Some(rx) = self.rx.take() else { return };
        loop {
            let next = if block { rx.recv().map_err(|_| TryRecvError::Disconnected) } else { rx.try_recv() };
            match next {
                Ok(c) => self.queue.push_back(c),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        self.rx = Some(rx);
    }

    fn is_complete(&self) -> bool {
        self.rx.is_none() && self.queue.is_empty() && self.current.is_none() && self.pending.is_empty()
    }

    fn progress(&self) -> JobProgress {
        let n = |v: usize| u64::try_from(v).unwrap_or(u64::MAX);
        JobProgress { done: n(self.done), total: n(self.units.max(self.done)) }
    }
}

/// What the session knows of the watched units' changes.
pub(crate) struct Watching {
    mode: WatchMode,
    worker: Option<Worker>,
    /// The watcher thread could not be started or went away: verification only.
    broken: bool,
    /// The roots, exclusions and mode last sent to the watcher.
    sent: Option<(Vec<PathBuf>, Vec<PathBuf>, WatchMode)>,
    backends: Vec<(PathBuf, Backend)>,
    fallbacks: Vec<Fallback>,
    /// The tick in flight.
    in_flight: Option<u64>,
    next_seq: u64,
    answered: u64,
    last_tick: Option<Timestamp>,
    /// Dirty folders reported and not verified yet.
    dirty: Vec<DirtyFolder>,
    /// Dirty folders reported since the session opened.
    reported: u64,
    rescan: Option<RescanJob>,
    /// When each unit's next verification is due; a unit not listed is due
    /// [`FIRST_VERIFICATION`] after `first_poll`.
    due: BTreeMap<PathBuf, Timestamp>,
    first_poll: Option<Timestamp>,
}

impl Default for Watching {
    fn default() -> Watching {
        Watching {
            mode: WatchMode::default(),
            worker: None,
            broken: false,
            sent: None,
            backends: Vec::new(),
            fallbacks: Vec::new(),
            in_flight: None,
            next_seq: 1,
            answered: 0,
            last_tick: None,
            dirty: Vec::new(),
            reported: 0,
            rescan: None,
            due: BTreeMap::new(),
            first_poll: None,
        }
    }
}

impl std::fmt::Debug for Watching {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watching").field("mode", &self.mode).field("backends", &self.backends).field("dirty", &self.dirty).finish_non_exhaustive()
    }
}

/// The watcher thread: answers requests in order until the session goes away. A new watcher is
/// started before the old one is dropped, so the two overlap instead of leaving a gap.
fn watcher(rx: Receiver<Request>, tx: Sender<Answer>) {
    let mut current: Option<watch::Watching> = None;
    while let Ok(request) = rx.recv() {
        let answer = match request {
            Request::Start { roots, options } => match catch_unwind(AssertUnwindSafe(|| watch::Watching::start(&roots, options))) {
                Ok(w) => {
                    let answer = Answer::Started { backends: w.roots(), fallbacks: w.fallbacks().to_vec() };
                    drop(current.replace(w));
                    answer
                }
                Err(_) => {
                    drop(current.take());
                    let fallbacks = roots.into_iter().map(|root| Fallback { root, reason: "the watcher failed to start".into() }).collect();
                    Answer::Started { backends: Vec::new(), fallbacks }
                }
            },
            Request::Tick { seq, now } => match current.as_mut().map(|w| catch_unwind(AssertUnwindSafe(|| (w.poll(now), w.roots())))) {
                None => Answer::Ticked { seq, dirty: Vec::new(), backends: Vec::new(), failed: false },
                Some(Ok((dirty, backends))) => Answer::Ticked { seq, dirty, backends, failed: false },
                Some(Err(_)) => {
                    drop(current.take());
                    Answer::Ticked { seq, dirty: Vec::new(), backends: Vec::new(), failed: true }
                }
            },
        };
        if tx.send(answer).is_err() {
            return;
        }
    }
}

/// Removes `folder`'s stale atomic-replace temporaries (`security.md` §5.8) as of the session time
/// `now`, through `dalica_fs::remove_stale_temps`. Returns what could not be done. Call it from a
/// scan's thread, never the owner's.
pub(crate) fn clean_temps(folder: &Path, now: Timestamp) -> Vec<String> {
    let at = u64::try_from(now.as_millis()).ok().and_then(|ms| UNIX_EPOCH.checked_add(Duration::from_millis(ms))).unwrap_or(UNIX_EPOCH);
    match dalica_fs::remove_stale_temps(folder, at) {
        Ok(done) => {
            for p in &done.removed {
                log::info!("removed the stale temporary file {}", p.display());
            }
            done.errors.iter().map(|e| format!("a stale temporary file could not be removed: {e}")).collect()
        }
        Err(e) => vec![format!("stale temporary files not looked at: {e}")],
    }
}

/// Whether `root` holds no entries at all (or cannot be listed): an unmounted volume leaves its
/// mount point like that, so nothing under it may be removed. Call it off the owner thread.
pub(crate) fn holds_nothing(root: &Path) -> bool {
    let limits = dalica_fs::walk::Limits { max_depth: 1, ..dalica_fs::walk::Limits::default() };
    dalica_fs::walk(root, limits, |_| false).map_or(true, |w| w.entries.is_empty())
}

/// One unit's dirty folders and its region's exclusions.
type UnitWork = (PathBuf, Vec<PathBuf>, Vec<DirtyFolder>);

/// The rescan thread: each unit's dirty folders verified in turn, results sent as they come.
fn rescan_units(units: Vec<UnitWork>, known: &BTreeSet<PathBuf>, options: &dalica_scan::Options, now: Timestamp, tx: &Sender<Checked>) {
    for (unit, excluded, dirty) in units {
        let mut opts = options.clone();
        opts.excluded = excluded;
        let mut rescan = watch::rescan(known, std::slice::from_ref(&unit), &dirty, &opts);
        let empty = rescan.verified.iter().any(|s| s.folder == unit) && holds_nothing(&unit);
        if empty {
            rescan.verified.retain(|s| s.folder != unit);
            rescan.scan.walked.retain(|w| *w != unit);
        }
        let mut problems = Vec::new();
        for f in &rescan.scan.folders {
            problems.extend(clean_temps(&f.path, now).into_iter().map(|p| format!("{}: {p}", f.path.display())));
        }
        let cancelled = rescan.scan.cancelled;
        if tx.send(Checked { unit, rescan, empty, problems }).is_err() || cancelled {
            return;
        }
    }
}

impl Session {
    /// Takes in what the watcher reported since the last frame, sends it a tick when one is due,
    /// starts the rescan of the dirty folders (once no scan runs) and applies a budget of its
    /// result, and starts the verification of the units that are due. Returns how many things it
    /// handled. Never blocks and never touches the disk.
    pub(crate) fn poll_watching(&mut self) -> usize {
        let now = self.clock.now();
        self.watching.first_poll.get_or_insert(now);
        let mut n = self.take_watch_answers();
        self.sync_watcher();
        self.send_tick(now, false);
        n += self.advance_rescan(PHOTOS_PER_POLL, false);
        self.start_rescan();
        n += self.verify_if_due(now);
        n
    }

    /// Switches the watcher (the default is [`WatchMode::Native`]); [`WatchMode::Off`] leaves only
    /// the verification pass. Takes effect at the next poll; dirty folders already reported are
    /// still verified.
    pub fn set_watching(&mut self, mode: WatchMode) {
        if self.watching.mode != mode {
            self.watching.mode = mode;
            self.watching.sent = None;
        }
    }

    /// How the units are watched.
    pub fn watching(&self) -> WatchMode {
        self.watching.mode
    }

    /// The roots the watcher watches and how, as of its last answer.
    pub fn watched_roots(&self) -> &[(PathBuf, Backend)] {
        &self.watching.backends
    }

    /// Roots polled instead of watched through the system's events, and why.
    pub fn watch_fallbacks(&self) -> &[Fallback] {
        &self.watching.fallbacks
    }

    /// Dirty folders the watcher reported since the session opened.
    pub fn watch_reports(&self) -> u64 {
        self.watching.reported
    }

    /// Asks the watcher for its dirty folders at the current session time and waits (up to
    /// `timeout`) for the answer, applying it as `poll` would and starting their rescan. Returns
    /// whether it came. For the CLI and tests: the wait blocks the caller (the watcher itself runs
    /// on its own thread); `wait_idle` then waits for the rescan.
    pub fn wait_watching(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let now = self.clock.now();
        self.watching.first_poll.get_or_insert(now);
        self.take_watch_answers();
        self.sync_watcher();
        let mut mine: Option<u64> = None;
        loop {
            if mine.is_none() && self.watching.in_flight.is_none() {
                mine = self.send_tick(now, true);
                if mine.is_none() {
                    return false;
                }
            }
            if mine.is_some_and(|m| self.watching.answered >= m) {
                self.start_rescan();
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(worker) = &self.watching.worker else { return false };
            match worker.rx.recv_timeout(left) {
                Ok(answer) => {
                    self.watch_answer(answer);
                }
                Err(RecvTimeoutError::Timeout) => return false,
                Err(RecvTimeoutError::Disconnected) => {
                    self.watcher_gone();
                    return false;
                }
            }
        }
    }

    /// A file job is starting: the running rescan, if any, is let go with its results not applied
    /// yet, and its dirty folders are reported again, to be verified once no file job runs.
    ///
    /// A rescan that overlaps a file job reads some files before the job writes them and others
    /// after, while the job's results reach the model only when the owner applies them: applied
    /// in either order, the rescan would put a version the model has moved past back into it (a
    /// saved photo's old size and time, an originals record without the entry a Save just wrote)
    /// or take away what the job's own result still needs (Revert of a record the rescan already
    /// dropped). Verified after the job, the folders hold what the job's results put in the
    /// model, so the rescan finds nothing to change.
    pub(crate) fn put_off_rescan(&mut self) {
        let Some(mut job) = self.watching.rescan.take() else { return };
        // The thread stops at its next file and is not waited for (as Cancel does).
        job.cancel.store(true, Ordering::Relaxed);
        job.rx = None;
        job.thread = None;
        let mut dirty = std::mem::take(&mut job.dirty);
        dirty.append(&mut self.watching.dirty);
        self.watching.dirty = dirty;
        let now = self.clock.now();
        let result = json!({ "folders": job.folders, "units": job.done, "changed": job.changed, "cancelled": true });
        self.jobs.finish(job.job, Outcome::Cancelled(result), "Check put off while files are written", now);
    }

    /// Whether a rescan of dirty folders is running (counted by [`Session::busy`]).
    pub(crate) fn rescanning(&self) -> bool {
        self.watching.rescan.is_some()
    }

    /// Blocks until the running rescan (and the one the dirty folders start, if no scan runs)
    /// has ended and everything it found is applied. Returns whether there was one.
    pub(crate) fn wait_rescans(&mut self) -> bool {
        let mut any = false;
        loop {
            self.start_rescan();
            let Some(job) = &mut self.watching.rescan else { return any };
            any = true;
            job.receive(true);
            self.advance_rescan(usize::MAX, true);
        }
    }

    /// A scan walked `plan` to the end: the units it covered are verified now.
    pub(crate) fn note_verified(&mut self, plan: &Plan) {
        let next = self.clock.now().saturating_add(FULL_VERIFICATION);
        for unit in self.folders.rules.units() {
            if plan.roots.iter().any(|r| unit.starts_with(r)) && !plan.excluded.iter().any(|x| unit.starts_with(x)) {
                self.watching.due.insert(unit, next);
            }
        }
    }

    /// Starts the verification of every unit whose time has come, unless something is being
    /// scanned or a file job is writing. Returns how many it started.
    fn verify_if_due(&mut self, now: Timestamp) -> usize {
        if self.busy() || self.file_jobs_running() {
            return 0;
        }
        let first = self.watching.first_poll.unwrap_or(now).saturating_add(FIRST_VERIFICATION);
        let units = self.folders.rules.verification();
        // A unit under a root found offline waits for it (the online checks run every 10 s).
        let offline = self.offline_roots();
        let due: Vec<PathBuf> = units
            .iter()
            .filter(|u| now >= self.watching.due.get(*u).copied().unwrap_or(first) && !offline.iter().any(|r| u.starts_with(r)))
            .cloned()
            .collect();
        if due.is_empty() {
            return 0;
        }
        self.watching.due.retain(|u, _| units.contains(u));
        for unit in &due {
            // Provisional: a scan that walks it sets the full interval.
            self.watching.due.insert(unit.clone(), now.saturating_add(RETRY));
            self.start_region_scan(unit);
        }
        due.len()
    }

    /// The roots the watcher should watch (the outermost Scan Always units) and the folders it
    /// leaves out (those inside them set to Remove or Scan Once that hold no Scan Always unit).
    fn watch_plan(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let rules = &self.folders.rules;
        let always = rules.with_mode(Mode::Always);
        let roots: Vec<PathBuf> = always.iter().filter(|u| !always.iter().any(|o| o != *u && u.starts_with(o))).cloned().collect();
        let excluded = rules
            .iter()
            .filter(|(p, r)| {
                matches!(r.mode, Some(Mode::Remove | Mode::Once))
                    && roots.iter().any(|root| p.starts_with(root))
                    && !always.iter().any(|a| a.starts_with(p))
            })
            .map(|(p, _)| p.to_path_buf())
            .collect();
        (roots, excluded)
    }

    /// Starts the watcher thread once (unless watching is off), and sends it the roots whenever
    /// they changed.
    fn sync_watcher(&mut self) {
        let mode = self.watching.mode;
        if mode == WatchMode::Off {
            if let Some(w) = self.watching.worker.take() {
                w.detach();
                self.watching.sent = None;
                self.watching.in_flight = None;
                self.watching.backends.clear();
                self.watching.fallbacks.clear();
            }
            return;
        }
        if self.watching.worker.is_none() {
            if self.watching.broken {
                return;
            }
            let (req_tx, req_rx) = mpsc::channel();
            let (ans_tx, ans_rx) = mpsc::channel();
            match std::thread::Builder::new().name("dalica-watch".into()).spawn(move || watcher(req_rx, ans_tx)) {
                Ok(thread) => {
                    self.watching.worker = Some(Worker { tx: req_tx, rx: ans_rx, thread: Some(thread) });
                    self.watching.sent = None;
                    self.watching.in_flight = None;
                }
                Err(e) => {
                    let message = format!("cannot start the folder watcher: {e}; changes are found by the verification pass only");
                    log::warn!("{message}");
                    self.scan_warnings.push(message);
                    self.watching.broken = true;
                    return;
                }
            }
        }
        let (roots, excluded) = self.watch_plan();
        if self.watching.sent.as_ref().is_some_and(|(r, x, m)| *r == roots && *x == excluded && *m == mode) {
            return;
        }
        let prefer = if mode == WatchMode::Polling { Prefer::Polling } else { Prefer::Native };
        let options = WatchOptions { prefer, excluded: excluded.clone(), ..WatchOptions::default() };
        let sent = self.watching.worker.as_ref().is_some_and(|w| w.tx.send(Request::Start { roots: roots.clone(), options }).is_ok());
        if sent {
            self.watching.sent = Some((roots, excluded, mode));
        } else {
            self.watcher_gone();
        }
    }

    /// Sends a tick at `now` when none is in flight and one is due (or `force`). Returns its number.
    fn send_tick(&mut self, now: Timestamp, force: bool) -> Option<u64> {
        let w = &mut self.watching;
        let worker = w.worker.as_ref()?;
        if w.in_flight.is_some() || w.sent.is_none() {
            return None;
        }
        if !force && w.last_tick.is_some_and(|t| now.since(t) < TICK) {
            return None;
        }
        let seq = w.next_seq;
        if worker.tx.send(Request::Tick { seq, now }).is_err() {
            self.watcher_gone();
            return None;
        }
        w.next_seq = seq.saturating_add(1);
        w.in_flight = Some(seq);
        w.last_tick = Some(now);
        Some(seq)
    }

    /// Takes in every answer waiting. Returns how many dirty folders came.
    fn take_watch_answers(&mut self) -> usize {
        let mut n = 0;
        while let Some(next) = self.watching.worker.as_ref().map(|w| w.rx.try_recv()) {
            match next {
                Ok(answer) => n += self.watch_answer(answer),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.watcher_gone();
                    break;
                }
            }
        }
        n
    }

    /// Applies one answer of the watcher. Returns how many dirty folders it brought.
    fn watch_answer(&mut self, answer: Answer) -> usize {
        match answer {
            Answer::Started { backends, fallbacks } => {
                self.watching.backends = backends;
                self.watching.fallbacks = fallbacks;
                0
            }
            Answer::Ticked { seq, dirty, backends, failed } => {
                if self.watching.in_flight == Some(seq) {
                    self.watching.in_flight = None;
                }
                self.watching.answered = self.watching.answered.max(seq);
                if failed {
                    // Whatever the watcher had not reported is lost: its roots are verified whole,
                    // and a new watcher starts at the next poll.
                    log::warn!("the folder watcher failed; its folders are verified whole");
                    if let Some((roots, ..)) = self.watching.sent.take() {
                        self.watching.dirty.extend(roots.into_iter().map(|path| DirtyFolder { path, subtree: true }));
                    }
                    return 0;
                }
                if !backends.is_empty() {
                    self.watching.backends = backends;
                }
                let n = dirty.len();
                self.watching.reported = self.watching.reported.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
                self.watching.dirty.extend(dirty);
                n
            }
        }
    }

    /// The watcher thread went away: changes are found by the verification pass only.
    fn watcher_gone(&mut self) {
        if let Some(w) = self.watching.worker.take() {
            w.detach();
            let message = "the folder watcher stopped; changes are found by the verification pass only".to_string();
            log::warn!("{message}");
            self.scan_warnings.push(message);
        }
        self.watching.broken = true;
        self.watching.sent = None;
        self.watching.in_flight = None;
    }

    /// The dirty folders by the Scan Always unit that owns them, with the unit's region
    /// exclusions. A dirty subtree also dirties every Scan Always unit inside it whole; folders
    /// no longer watched (removed, frozen) are dropped.
    fn dirty_by_unit(&self, dirty: &[DirtyFolder]) -> Vec<UnitWork> {
        let rules = &self.folders.rules;
        let always = rules.with_mode(Mode::Always);
        let mut by_unit: BTreeMap<PathBuf, Vec<DirtyFolder>> = BTreeMap::new();
        for d in dirty {
            let e = rules.effective(&d.path);
            if let (Mode::Always, Some(unit)) = (e.mode, e.from) {
                by_unit.entry(unit).or_default().push(d.clone());
            }
            if d.subtree {
                for unit in always.iter().filter(|u| **u != d.path && u.starts_with(&d.path)) {
                    by_unit.entry(unit.clone()).or_default().push(DirtyFolder { path: unit.clone(), subtree: true });
                }
            }
        }
        by_unit
            .into_iter()
            .map(|(unit, mut d)| {
                d.sort();
                d.dedup();
                let excluded = rules.inside(&unit);
                (unit, excluded, d)
            })
            .collect()
    }

    /// Starts the rescan of the dirty folders, unless one runs, a scan does, or a file job is
    /// writing (`Session::put_off_rescan`).
    fn start_rescan(&mut self) {
        if self.watching.rescan.is_some() || self.watching.dirty.is_empty() || !self.scans_idle() || self.file_jobs_running() {
            return;
        }
        let dirty = std::mem::take(&mut self.watching.dirty);
        let units = self.dirty_by_unit(&dirty);
        if units.is_empty() {
            return;
        }
        let now = self.clock.now();
        let folders = units.iter().map(|(_, _, d)| d.len()).sum::<usize>();
        let label = format!("Checking {folders} changed {}", if folders == 1 { "folder" } else { "folders" });
        let (job, cancel) = self.jobs.start(JobKind::Scan, label, true, now);
        let mut options = self.scan_options();
        options.cancel = Some(Arc::clone(&cancel));
        let known = watch::known_folders(&self.library);
        let count = units.len();
        let reported = dirty.clone();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("dalica-rescan".into()).spawn(move || rescan_units(units, &known, &options, now, &tx));
        match spawned {
            Ok(thread) => {
                let r = RescanJob {
                    job,
                    rx: Some(rx),
                    queue: VecDeque::new(),
                    current: None,
                    pending: VecDeque::new(),
                    revision: self.library.revision(),
                    units: count,
                    done: 0,
                    folders,
                    changed: 0,
                    cancelled: false,
                    cancel,
                    thread: Some(thread),
                    dirty: reported,
                };
                self.jobs.progress(job, r.progress(), format!("Checking changed folders: 0 of {count}"));
                self.watching.rescan = Some(r);
            }
            Err(e) => {
                let message = format!("cannot start a rescan: {e}");
                self.scan_warnings.push(message.clone());
                // The folders stay dirty: tried again at the next poll.
                self.watching.dirty = dirty;
                self.jobs.finish(job, Outcome::Failed(message), "Scan failed", now);
            }
        }
    }

    /// Takes in a unit's result: its warnings, failures and an empty unit's warning.
    fn absorb(&mut self, c: &Checked) {
        self.scan_warnings.extend(c.rescan.scan.warnings.iter().map(|w| format!("{}: {}", w.path.display(), w.reason)));
        self.scan_warnings.extend(c.problems.iter().cloned());
        let fresh = self.remember_rescan_failures(&|p: &Path| c.rescan.covers(p), &c.rescan.scan.failures);
        self.scan_warnings.extend(fresh.iter().map(|f| format!("{}: {}", f.path.display(), f.reason)));
        if c.empty {
            let kept = self.library.folders().filter(|f| f.path.starts_with(&c.unit)).count();
            if kept > 0 {
                self.scan_warnings
                    .push(format!("{}: the folder is empty; its {kept} folders stay in the library (is its volume unmounted?)", c.unit.display()));
            }
        }
    }

    /// Applies what fits in `photos` of the running rescan's results (taking in what the thread
    /// sent, waiting for it with `block`), and closes the job once everything is applied. Returns
    /// how many units it finished.
    fn advance_rescan(&mut self, photos: usize, block: bool) -> usize {
        let Some(mut job) = self.watching.rescan.take() else { return 0 };
        job.receive(block);
        if !job.cancelled && self.jobs.cancel_requested(job.job) {
            // The thread stops at its next file (never waited for); what it found but did not hand
            // over is dropped.
            job.cancelled = true;
            job.rx = None;
            job.thread = None;
            job.queue.clear();
        }
        let mut budget = Budget::new(photos);
        let mut units = 0;
        let mut applied = 0usize;
        loop {
            if job.current.is_none() {
                let Some(next) = job.queue.pop_front() else { break };
                self.absorb(&next);
                job.pending = dalica_scan::diff_rescan(&mut self.library, &next.rescan).into();
                job.revision = self.library.revision();
                job.current = Some(next);
            } else if self.library.revision() != job.revision {
                // Something else changed the model since: diff again (what is applied stays).
                if let Some(c) = &job.current {
                    job.pending = dalica_scan::diff_rescan(&mut self.library, &c.rescan).into();
                }
                job.revision = self.library.revision();
            }
            while let Some(op) = job.pending.front() {
                let n = cost(op);
                if !budget.take(n) {
                    break;
                }
                let Some(op) = job.pending.pop_front() else { break };
                if let Err(e) = self.apply(op) {
                    self.scan_warnings.push(format!("rescan result not applied: {e}"));
                }
                applied = applied.saturating_add(n);
                job.revision = self.library.revision();
            }
            if !job.pending.is_empty() {
                break;
            }
            job.current = None;
            job.done += 1;
            units += 1;
        }
        job.changed = job.changed.saturating_add(applied);
        if applied > 0 {
            self.sync_thumbnails();
        }
        if !job.is_complete() {
            self.jobs.progress(job.job, job.progress(), format!("Checking changed folders: {} of {}", job.done, job.units));
            self.watching.rescan = Some(job);
            return units;
        }
        let now = self.clock.now();
        let result = json!({ "folders": job.folders, "units": job.done, "changed": job.changed, "cancelled": job.cancelled });
        self.jobs.progress(job.job, job.progress(), "Checked changed folders");
        if job.cancelled {
            self.jobs.finish(job.job, Outcome::Cancelled(result), "Check cancelled", now);
        } else {
            let label = format!("Checked changed folders: {} changed", pictures(job.changed));
            self.jobs.finish(job.job, Outcome::Done(result), label, now);
        }
        units
    }
}
