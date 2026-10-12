//! Offline volumes (`data-model.md` §7, `security.md` §5.9): which watched roots can be reached,
//! and the sidecar writes that wait for the ones that cannot.
//!
//! - **Online checks** run on a worker thread of their own ("dalica-volumes"), never on the owner
//!   thread: [`Session::poll`] sends one every [`CHECK_EVERY`] on the session clock, after each
//!   scan ends and when the watched roots change, and takes in the answers. A hung network share
//!   holds only that thread; until it answers, the last known state stands (a root never checked
//!   counts as online).
//! - **Offline folders** stay in the library and the tree with their photos, rows and cached
//!   thumbnails (scans never remove what is under a root they could not walk);
//!   [`Session::offline_roots`] feeds `TreeOptions::offline_roots`, which marks their rows.
//! - **Queued writes:** an edit to a photo under an offline root is applied and journalled at once
//!   like any other; its sidecar write waits in the writer, which never creates the missing folder
//!   (`dalica-store`'s writer). This module keeps the list of folders whose write waits
//!   ([`Session::pending_writes`]), and when their root comes back marks them dirty again so they
//!   are written within a second. The list is kept in `pending-writes.json` in the library
//!   directory (written through the I/O worker, read by the checker), so a write still waiting
//!   when the app quits is made when the root is next found online, after any restart.
//!
//! - **Mount points** (M5.17): a reachable root still counts as offline when it holds no entries
//!   while the library has photos under it (the empty folder an unmounted volume leaves at its
//!   mount point), or when its device changed since it was last seen with entries and none of the
//!   library's files under it is there (another volume mounted in its place; a replugged drive
//!   with a new device number but the same files is accepted). Device ids are Unix `st_dev`; on
//!   Windows only the first rule applies. The roots found offline are sent to the I/O worker
//!   (`Msg::Hold`): the writer writes nothing under them, even where a folder is there, and the
//!   edit is reported unsaved as for a missing folder.
//! - **Checked right before each write** (M6.43): a root can go between two checks, up to
//!   [`CHECK_EVERY`] apart. So the I/O worker also checks each watched root right before a sidecar
//!   write under it (`Msg::Guard`, `dalica-store`'s `SidecarWriter::guard`): the same probe and
//!   the same rules as the online checks ([`is_library_volume`]), with the device remembered and
//!   the sample paths of the last check. A root that fails gets nothing written; the edit is
//!   reported unsaved, waits in the writer, and lands once the root passes again (at the writer's
//!   next look, or when the next online check finds it offline and later back). The check runs on
//!   the I/O worker; the owner thread only sends what to expect, when it changed.
//!
//! Which edits wait: the writer gets every edit within about a second, so when a root is found
//! offline, the folders under it changed since [`SETTLE`] before the last check that saw it online
//! are counted as waiting (a few already written may be counted, and are rewritten harmlessly:
//! the writer merges and skips unchanged files); edits made while it is offline always are.
//! A waiting folder whose root is back counts until a check made [`SETTLE`] later still finds the
//! root online.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use dalica_core::{FolderId, Timestamp};
use serde::Serialize;
use serde_json::{Value, json};

use dalica_scan::volumes::RootState;
use dalica_store::writer::RootCheck;

use crate::scanning::ScanProgress;
use crate::session::{Msg, Session};

/// How often the roots are checked on the session clock (besides after each scan).
pub const CHECK_EVERY: Duration = Duration::from_secs(10);
/// How long the writer may take to land an edit: edits newer than this before a root was last
/// seen online count as waiting when it goes away; a returned root's writes count until a check
/// this much later.
pub const SETTLE: Duration = Duration::from_secs(5);
/// The list of folders whose sidecar write waits, in the library directory.
pub const PENDING_FILE: &str = "pending-writes.json";
/// The largest `pending-writes.json` read.
const PENDING_CAP: u64 = 16 << 20;
/// Recent checks remembered for [`SETTLE`] look-ups.
const HISTORY: usize = 64;
/// How many of the library's photo paths under a root each check looks for.
pub const SAMPLES: usize = 8;

/// Says whether a watched root can be reached. Runs on the checker thread; the default is
/// `dalica_scan::volumes::online`. Tests plant gated or recording ones.
pub type OnlineCheck = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// Looks at a reachable root (with sample paths of the library's photos under it). Runs on the
/// checker thread; the default is `dalica_scan::volumes::root_state`. Tests plant others.
pub type RootProbe = Arc<dyn Fn(&Path, &[PathBuf]) -> RootState + Send + Sync>;

/// `library.inspect` → `pendingWrites` (`control-protocol.md` §5.8): writes still to make.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingWrites {
    /// Folders whose sidecar write waits for an offline root.
    pub sidecars: usize,
    /// Metadata writes into files (none before M6).
    pub metadata: usize,
    /// Index writes (journalled at once: none wait).
    pub index: usize,
    /// Why they wait.
    pub error: Option<String>,
}

enum Request {
    /// Read `pending-writes.json` (at this time, for naming a damaged one).
    Load(PathBuf, Timestamp),
    Check {
        seq: u64,
        /// Each root with sample paths of the library's photos under it.
        roots: Vec<(PathBuf, Vec<PathBuf>)>,
        check: OnlineCheck,
        probe: RootProbe,
    },
}

enum Answer {
    /// The folders `pending-writes.json` listed.
    Loaded(Vec<PathBuf>),
    Checked {
        seq: u64,
        /// Each root, whether it is reachable, and what the probe saw of it if so.
        roots: Vec<(PathBuf, bool, Option<RootState>)>,
    },
}

struct Checker {
    tx: Sender<Request>,
    rx: Receiver<Answer>,
}

#[derive(Clone, Copy, Debug)]
struct Root {
    online: bool,
    /// When the last check that found it online was sent.
    seen_online: Option<Timestamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wait {
    /// Its root is offline (or not checked since a restart).
    Waiting,
    /// Its root came back and it was marked dirty at this time.
    Released(Timestamp),
}

/// Which roots are reachable, and the folders whose sidecar write waits.
pub(crate) struct Offline {
    checker: Option<Checker>,
    started: bool,
    check: OnlineCheck,
    roots: BTreeMap<PathBuf, Root>,
    /// The check in flight: (seq, sent at, library revision when sent).
    in_flight: Option<(u64, Timestamp, u64)>,
    next_seq: u64,
    answered: u64,
    last_sent: Option<Timestamp>,
    /// A check was asked for (after a scan, or by `refresh_volumes`).
    wanted: bool,
    /// The roots of the last check.
    watched: Vec<PathBuf>,
    /// Scans running, and their progress, at the last poll (to see one end).
    scan: (bool, ScanProgress),
    /// (sent at, revision) of recent checks, oldest first; `floor` stands for older ones.
    history: VecDeque<(Timestamp, u64)>,
    floor: u64,
    seen_revision: u64,
    pending: BTreeMap<FolderId, Wait>,
    /// What `pending-writes.json` holds; `None` until it was read (never written before that).
    saved: Option<Vec<PathBuf>>,
    probe: RootProbe,
    /// Each root's device when it was last seen online with entries.
    devices: BTreeMap<PathBuf, u64>,
    /// The roots last sent to the writer as held.
    held: Option<Vec<PathBuf>>,
    /// The sample paths each root was last checked with.
    samples: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// What the writer was last told to expect of each root: (root, device, samples).
    guarded: Option<Vec<Expected>>,
}

/// What a root must look like right before a write under it: (root, device remembered, samples).
type Expected = (PathBuf, Option<u64>, Vec<PathBuf>);

impl Default for Offline {
    fn default() -> Offline {
        Offline {
            checker: None,
            started: false,
            check: Arc::new(dalica_scan::volumes::online),
            roots: BTreeMap::new(),
            in_flight: None,
            next_seq: 1,
            answered: 0,
            last_sent: None,
            wanted: false,
            watched: Vec::new(),
            scan: (false, ScanProgress::default()),
            history: VecDeque::new(),
            floor: 0,
            seen_revision: 0,
            pending: BTreeMap::new(),
            saved: None,
            probe: Arc::new(dalica_scan::volumes::root_state),
            devices: BTreeMap::new(),
            held: None,
            samples: BTreeMap::new(),
            guarded: None,
        }
    }
}

impl std::fmt::Debug for Offline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offline").field("roots", &self.roots).field("pending", &self.pending).finish_non_exhaustive()
    }
}

impl Offline {
    fn offline_roots(&self) -> impl Iterator<Item = &PathBuf> {
        self.roots.iter().filter(|(_, r)| !r.online).map(|(p, _)| p)
    }

    fn is_offline(&self, path: &Path) -> bool {
        self.offline_roots().any(|r| path.starts_with(r))
    }

    /// The library revision at `t`, or an earlier one: the latest check sent by then.
    fn revision_at(&self, t: Timestamp) -> u64 {
        self.history.iter().rev().find(|(sent, _)| *sent <= t).map_or(self.floor, |(_, rev)| *rev)
    }
}

/// Whether a reachable root, as the probe saw it, is the library's volume: not an empty folder
/// where the library has photos, and not on another device than the one `remembered` with none of
/// the library's files there. The online checks and the writer's check before each write
/// (M6.43) both decide by this.
pub(crate) fn is_library_volume(remembered: Option<u64>, st: RootState) -> bool {
    if st.empty && !st.known {
        return false;
    }
    !matches!((st.device, remembered), (Some(now), Some(was)) if now != was && !st.known)
}

/// The writer's check of one root: the probe and [`is_library_volume`], run on the I/O worker.
/// A probe that panics fails the check (the write waits; nothing is lost).
fn root_check(probe: &RootProbe, device: Option<u64>, samples: Vec<PathBuf>) -> RootCheck {
    let probe = Arc::clone(probe);
    Arc::new(move |root: &Path| catch_unwind(AssertUnwindSafe(|| probe(root, &samples))).is_ok_and(|st| is_library_volume(device, st)))
}

/// The checker thread: answers requests in order until the session goes away.
fn checker(rx: Receiver<Request>, tx: Sender<Answer>) {
    while let Ok(request) = rx.recv() {
        let answer = match request {
            Request::Load(path, now) => Answer::Loaded(load_pending(&path, now)),
            Request::Check { seq, roots, check, probe } => {
                // A check or probe that panics counts as offline: the writes wait, nothing is lost.
                let roots = roots
                    .into_iter()
                    .map(|(r, samples)| {
                        let online = catch_unwind(AssertUnwindSafe(|| check(&r))).unwrap_or(false);
                        let state = if online { catch_unwind(AssertUnwindSafe(|| probe(&r, &samples))).ok() } else { None };
                        (r, online && state.is_some(), state)
                    })
                    .collect();
                Answer::Checked { seq, roots }
            }
        };
        if tx.send(answer).is_err() {
            return;
        }
    }
}

/// Reads `pending-writes.json`; a damaged one is kept aside (`.corrupt-<time>`) and read as empty.
fn load_pending(path: &Path, now: Timestamp) -> Vec<PathBuf> {
    let bytes = match dalica_fs::read(path, PENDING_CAP) {
        Ok(Some(read)) => read.bytes,
        Ok(None) => return Vec::new(),
        Err(e) => {
            log::warn!("{PENDING_FILE}: {e}");
            return Vec::new();
        }
    };
    let parsed: Option<Vec<PathBuf>> = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| v.get("folders")?.as_array()?.iter().map(|p| p.as_str().map(PathBuf::from)).collect());
    match parsed {
        Some(list) => list,
        None => {
            let stamp = dalica_core::DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_default();
            let kept = dalica_fs::set_aside(path, &format!("corrupt-{stamp}Z"));
            log::warn!("{PENDING_FILE} was damaged and kept aside ({kept:?}); waiting writes are found again when their roots are checked");
            Vec::new()
        }
    }
}

fn pending_json(paths: &[PathBuf]) -> Vec<u8> {
    let list: Vec<&str> = paths.iter().filter_map(|p| p.to_str()).collect();
    let mut bytes = serde_json::to_vec_pretty(&json!({ "version": 1, "folders": list })).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

impl Session {
    /// Takes in what the online checks found since the last frame, notes edits to folders under
    /// offline roots, sends a check when one is due, and keeps `pending-writes.json` up to date.
    /// Returns how many roots changed state. Never blocks and never touches the disk.
    pub(crate) fn poll_offline(&mut self) -> usize {
        self.start_checker();
        let mut changed = 0;
        while let Some(next) = self.offline.checker.as_ref().map(|c| c.rx.try_recv()) {
            match next {
                Ok(answer) => changed += self.apply_answer(answer),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    log::warn!("the volume checker stopped; the last known states stand");
                    self.offline.checker = None;
                    self.offline.in_flight = None;
                    break;
                }
            }
        }
        self.note_offline_edits();
        self.send_check_if_due();
        self.save_pending();
        changed
    }

    /// Starts the checker thread once, and asks it for `pending-writes.json` first.
    fn start_checker(&mut self) {
        if self.offline.started {
            return;
        }
        self.offline.started = true;
        self.offline.floor = self.library.revision();
        self.offline.seen_revision = self.library.revision();
        let (req_tx, req_rx) = mpsc::channel();
        let (ans_tx, ans_rx) = mpsc::channel();
        match std::thread::Builder::new().name("dalica-volumes".into()).spawn(move || checker(req_rx, ans_tx)) {
            Ok(_) => {
                let _ = req_tx.send(Request::Load(self.dir.join(PENDING_FILE), self.clock.now()));
                self.offline.checker = Some(Checker { tx: req_tx, rx: ans_rx });
            }
            Err(e) => {
                let message = format!("cannot start the volume checker: {e}; every folder counts as online");
                log::warn!("{message}");
                self.scan_warnings.push(message);
            }
        }
    }

    /// Applies one answer of the checker. Returns how many roots changed state.
    fn apply_answer(&mut self, answer: Answer) -> usize {
        match answer {
            Answer::Loaded(paths) => {
                for p in &paths {
                    if let Some(id) = self.library.folder_by_path(p) {
                        self.offline.pending.entry(id).or_insert(Wait::Waiting);
                    }
                }
                let mut saved = paths;
                saved.sort();
                saved.dedup();
                self.offline.saved = Some(saved);
                0
            }
            Answer::Checked { seq, roots } => {
                let Some((_, sent, revision)) = self.offline.in_flight.filter(|(s, _, _)| *s == seq) else { return 0 };
                self.offline.in_flight = None;
                self.offline.answered = seq;
                let mut changed = 0;
                let answered: BTreeSet<PathBuf> = roots.iter().map(|(p, ..)| p.clone()).collect();
                let judged: Vec<(PathBuf, bool, Option<Root>)> = roots
                    .into_iter()
                    .map(|(root, reachable, state)| {
                        let online = self.judge(&root, reachable, state);
                        let before = self.offline.roots.get(&root).copied();
                        (root, online, before)
                    })
                    .collect();
                for (root, online, before) in &judged {
                    let seen_online = if *online { Some(sent) } else { before.and_then(|b| b.seen_online) };
                    self.offline.roots.insert(root.clone(), Root { online: *online, seen_online });
                }
                self.offline.roots.retain(|p, _| answered.contains(p));
                // The writer learns the held roots (and the devices just seen) before any waiting
                // folder is marked dirty again.
                self.sync_hold();
                self.sync_guard();
                for (root, online, before) in judged {
                    if online {
                        self.release_root(&root, sent);
                    } else if before.is_none_or(|b| b.online) {
                        self.hold_root(&root, before.and_then(|b| b.seen_online));
                    }
                    // A root never checked counted as online.
                    if before.map_or(!online, |b| b.online != online) {
                        changed += 1;
                    }
                }
                self.offline.history.push_back((sent, revision));
                while self.offline.history.len() > HISTORY {
                    if let Some((_, rev)) = self.offline.history.pop_front() {
                        self.offline.floor = rev;
                    }
                }
                changed
            }
        }
    }

    /// Whether a root counts as online: reachable, not an empty folder where the library has
    /// photos, and not on another device with none of the library's files there. Remembers the
    /// device of a root seen online with entries.
    fn judge(&mut self, root: &Path, reachable: bool, state: Option<RootState>) -> bool {
        let Some(st) = state.filter(|_| reachable) else { return false };
        if !is_library_volume(self.offline.devices.get(root).copied(), st) {
            return false;
        }
        if let Some(d) = st.device.filter(|_| !st.empty) {
            self.offline.devices.insert(root.to_path_buf(), d);
        }
        true
    }

    /// Sends the writer the roots found offline when they changed.
    fn sync_hold(&mut self) {
        let off: Vec<PathBuf> = self.offline.offline_roots().cloned().collect();
        if self.offline.held.as_ref() != Some(&off) {
            self.offline.held = Some(off.clone());
            self.send(Msg::Hold(off));
        }
    }

    /// Sends the writer what each root must look like right before a write under it (the device
    /// remembered and the samples of the last check), when that changed. No disk access: the
    /// checks run on the I/O worker.
    fn sync_guard(&mut self) {
        let want: Vec<Expected> =
            self.offline.samples.iter().map(|(root, samples)| (root.clone(), self.offline.devices.get(root).copied(), samples.clone())).collect();
        if self.offline.guarded.as_ref() == Some(&want) {
            return;
        }
        let probe = &self.offline.probe;
        let roots = want.iter().map(|(root, device, samples)| (root.clone(), root_check(probe, *device, samples.clone()))).collect();
        self.offline.guarded = Some(want);
        self.send(Msg::Guard(roots));
    }

    /// `root` answered online to the check sent at `sent`: its waiting folders are marked dirty
    /// again (the writer lands them within a second); those released a while before are done.
    fn release_root(&mut self, root: &Path, sent: Timestamp) {
        let now = self.clock.now();
        let mut release = Vec::new();
        let library = &self.library;
        self.offline.pending.retain(|&id, wait| {
            // A folder that left the library has nothing to write.
            let Some(folder) = library.folder(id) else { return false };
            if !folder.path.starts_with(root) {
                return true;
            }
            match *wait {
                Wait::Waiting => {
                    release.push(id);
                    *wait = Wait::Released(now);
                    true
                }
                Wait::Released(at) => sent < at.saturating_add(SETTLE),
            }
        });
        if !release.is_empty() {
            let states = release.into_iter().map(|f| (f, dalica_store::mapping::folder_state(&self.library, f))).collect();
            self.send(Msg::Dirty(states));
        }
    }

    /// `root` went away (or was never seen online): the folders under it changed since [`SETTLE`]
    /// before it was last seen online wait, and so do any released ones.
    fn hold_root(&mut self, root: &Path, seen_online: Option<Timestamp>) {
        let since = match seen_online {
            Some(t) => self.offline.revision_at(t.saturating_sub(SETTLE)),
            None => self.offline.floor,
        };
        for f in self.library.folders() {
            if f.path.starts_with(root) && (self.library.folder_revision(f.id) > since || self.offline.pending.contains_key(&f.id)) {
                self.offline.pending.insert(f.id, Wait::Waiting);
            }
        }
    }

    /// Edits since the last poll to folders under an offline root wait. One pass over the folders
    /// when the library changed while a root is offline; nothing otherwise.
    fn note_offline_edits(&mut self) {
        let revision = self.library.revision();
        if revision == self.offline.seen_revision {
            return;
        }
        if self.offline.roots.values().any(|r| !r.online) {
            let since = self.offline.seen_revision;
            for f in self.library.folders() {
                if self.library.folder_revision(f.id) > since && self.offline.is_offline(&f.path) {
                    self.offline.pending.insert(f.id, Wait::Waiting);
                }
            }
        }
        self.offline.seen_revision = revision;
    }

    /// Sends a check when one is due: every [`CHECK_EVERY`], after a scan ended, when the roots
    /// changed, or when one was asked for. One at a time.
    fn send_check_if_due(&mut self) {
        let now = self.clock.now();
        let busy = self.busy();
        let progress = self.scan_progress();
        let (was_busy, last) = self.offline.scan;
        let scan_ended = !busy && (was_busy || progress != last);
        self.offline.scan = (busy, progress);
        let roots_changed = self.roots() != self.offline.watched.as_slice();
        self.offline.wanted |= scan_ended || roots_changed;
        let Some(checker) = &self.offline.checker else { return };
        if self.offline.in_flight.is_some() {
            return;
        }
        let due = self.offline.wanted || self.offline.last_sent.is_none_or(|t| now.since(t) >= CHECK_EVERY);
        if !due {
            return;
        }
        let seq = self.offline.next_seq;
        let roots = self.roots().to_vec();
        let with_samples: Vec<(PathBuf, Vec<PathBuf>)> = roots.iter().map(|r| (r.clone(), self.samples(r))).collect();
        self.offline.samples = with_samples.iter().cloned().collect();
        let request = Request::Check { seq, roots: with_samples, check: Arc::clone(&self.offline.check), probe: Arc::clone(&self.offline.probe) };
        if checker.tx.send(request).is_err() {
            return;
        }
        self.offline.next_seq = seq.saturating_add(1);
        self.offline.in_flight = Some((seq, now, self.library.revision()));
        self.offline.last_sent = Some(now);
        self.offline.wanted = false;
        self.offline.watched = roots;
        // The writer checks each root with these samples right before a write under it.
        self.sync_guard();
    }

    /// Writes `pending-writes.json` (through the I/O worker) when the waiting folders changed,
    /// once the file was read.
    fn save_pending(&mut self) {
        let Some(saved) = &self.offline.saved else { return };
        let mut now: Vec<PathBuf> = self.offline.pending.keys().filter_map(|&id| self.library.folder(id)).map(|f| f.path.clone()).collect();
        now.sort();
        now.dedup();
        if &now == saved {
            return;
        }
        let bytes = pending_json(&now);
        self.offline.saved = Some(now);
        self.send(Msg::Write(self.dir.join(PENDING_FILE), bytes));
    }

    /// Checks the roots now and waits (up to `timeout`) for the answer, applying it as
    /// [`Session::poll`] would. Returns whether it came. For the CLI and tests: the wait blocks
    /// the caller (the check itself runs on the checker thread); the UI relies on `poll`.
    pub fn refresh_volumes(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        self.poll_offline();
        // A check sent earlier answers first; then one of our own.
        let mut mine: Option<u64> = None;
        loop {
            if mine.is_none() && self.offline.in_flight.is_none() {
                self.offline.wanted = true;
                self.poll_offline();
                match self.offline.in_flight {
                    Some((seq, _, _)) => mine = Some(seq),
                    None => return false,
                }
            }
            if mine.is_some_and(|m| self.offline.answered >= m) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(checker) = &self.offline.checker else { return false };
            match checker.rx.recv_timeout(left) {
                Ok(answer) => {
                    self.apply_answer(answer);
                    self.poll_offline();
                }
                Err(RecvTimeoutError::Timeout) => return false,
                Err(RecvTimeoutError::Disconnected) => {
                    self.offline.checker = None;
                    self.offline.in_flight = None;
                    return false;
                }
            }
        }
    }

    /// Up to [`SAMPLES`] paths of the library's photos under `root` (one pass over the folders,
    /// once per check).
    fn samples(&self, root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for f in self.library.folders().filter(|f| f.path.starts_with(root)) {
            for p in self.library.photos_in(f.id) {
                if out.len() >= SAMPLES {
                    return out;
                }
                out.push(f.path.join(&p.name));
            }
        }
        out
    }

    /// Plants the probe the online checks use on reachable roots (`None`:
    /// `dalica_scan::volumes::root_state`). It runs on the checker thread, and on the I/O worker
    /// for the check right before each sidecar write.
    pub fn set_root_probe(&mut self, probe: Option<RootProbe>) {
        self.offline.probe = probe.unwrap_or_else(|| Arc::new(dalica_scan::volumes::root_state));
        self.offline.guarded = None;
        self.sync_guard();
    }

    /// Plants the function the online checks use (`None`: `dalica_scan::volumes::online`). It
    /// runs on the checker thread.
    pub fn set_online_check(&mut self, check: Option<OnlineCheck>) {
        self.offline.check = check.unwrap_or_else(|| Arc::new(dalica_scan::volumes::online));
    }

    /// Whether watched root `root` was reachable at the last check (`None`: not checked yet).
    pub fn root_online(&self, root: &Path) -> Option<bool> {
        self.offline.roots.get(root).map(|r| r.online)
    }

    /// The watched roots found offline at the last check (for `TreeOptions::offline_roots`).
    pub fn offline_roots(&self) -> BTreeSet<PathBuf> {
        self.offline.offline_roots().cloned().collect()
    }

    /// Whether folder `id` is under a root found offline.
    pub fn is_folder_offline(&self, id: FolderId) -> bool {
        self.library.folder(id).is_some_and(|f| self.offline.is_offline(&f.path))
    }

    /// The writes still to make (`library.inspect` → `pendingWrites`): `None` when there are none.
    pub fn pending_writes(&self) -> Option<PendingWrites> {
        let sidecars = self.offline.pending.len();
        if sidecars == 0 {
            return None;
        }
        let waiting: BTreeSet<&PathBuf> = self
            .offline
            .pending
            .iter()
            .filter(|(_, w)| **w == Wait::Waiting)
            .filter_map(|(id, _)| self.library.folder(*id))
            .filter_map(|f| self.offline.offline_roots().find(|r| f.path.starts_with(r)))
            .collect();
        let error = if waiting.is_empty() {
            "being written now that their volume is back".to_string()
        } else {
            let list: Vec<String> = waiting.iter().map(|r| r.display().to_string()).collect();
            format!("waiting for offline volumes: {}", list.join(", "))
        };
        Some(PendingWrites { sidecars, metadata: 0, index: 0, error: Some(error) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_are_looked_up_at_or_before_a_time() {
        let mut o = Offline { floor: 3, ..Offline::default() };
        let t = |s: u64| Timestamp::EPOCH.saturating_add(Duration::from_secs(s));
        assert_eq!(o.revision_at(t(5)), 3);
        o.history.extend([(t(10), 7), (t(20), 9)]);
        assert_eq!(o.revision_at(t(9)), 3);
        assert_eq!(o.revision_at(t(10)), 7);
        assert_eq!(o.revision_at(t(19)), 7);
        assert_eq!(o.revision_at(t(25)), 9);
    }

    #[test]
    fn the_pending_file_round_trips_and_damage_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PENDING_FILE);
        assert!(load_pending(&path, Timestamp::EPOCH).is_empty(), "no file: nothing waits");
        let list = vec![dir.path().join("a"), dir.path().join("b c")];
        dalica_fs::replace(&path, &pending_json(&list), dalica_fs::Expect::Absent).unwrap();
        assert_eq!(load_pending(&path, Timestamp::EPOCH), list);
        let id = dalica_fs::identity(&path).unwrap().unwrap();
        dalica_fs::replace(&path, b"{\"folders\": 7", dalica_fs::Expect::Unchanged(id)).unwrap();
        assert!(load_pending(&path, Timestamp::EPOCH).is_empty());
        assert!(!path.exists(), "the damaged file is kept aside");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
