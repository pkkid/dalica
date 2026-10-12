//! Scans as jobs (`architecture.md` §5, `data-model.md` §7): each scan walks its roots on a thread
//! of its own through [`dalica_scan::scan_streaming`] and is listed in the job registry
//! ([`crate::jobs`]) as a `scan` (or `rebuild`) job with a cancel flag.
//!
//! The walk hands out each folder's photos as soon as the folder is read. [`Session::poll`] takes
//! them in and applies at most [`PHOTOS_PER_POLL`] photos per call (each folder diffed against the
//! model as its turn comes, so a command run in between is never overwritten), so folders and
//! thumbnails fill in progressively while the walk goes on; each landed batch queues thumbnail
//! generation. When the walk ends and everything it found is applied, folders under the roots it
//! walked to the end that hold no photos any more leave the library (the verification pass), and
//! the job closes with what it found.
//!
//! - **Merge** scans (`folders.add`, rescans) apply to the live model: journaled and written to the
//!   sidecars like any change, never on the undo stack.
//! - **Rebuild** scans (`library.rebuild`, and a damaged index at open) build a fresh model to the
//!   side and swap it in at the end, clearing the undo stack (ids change); a cancelled rebuild
//!   keeps the old model. A rebuild of an empty model fills it in place, like a merge.
//! - **Cancel** ([`Session::cancel_jobs`]): the walk stops before its next file; the folder being
//!   applied is finished, folders read but not yet applied are dropped, nothing is removed, and the
//!   job ends as `cancelled` with what it indexed.
//! - **Failures** (files whose guarded header read failed) are reported once as scan warnings and
//!   given back to the next scan, which does not read them again until their size or modification
//!   time changes.
//!
//! - **What a scan walks** follows the Folder Manager's rules ([`crate::folders`]): a merge scan
//!   walks one region (a folder without the folders inside it that set a mode of their own, which
//!   are scanned on their own, or removed), a rebuild every unit ([`dalica_scan::Rules::everything`]).
//!   A folder the rules do not index when its turn comes (it was removed meanwhile, or it lies in
//!   a removed folder a rebuild walks for a unit inside it) is dropped, never applied. A Scan Once
//!   unit a finished scan walked is marked indexed (frozen); [`Session::start_verification`] leaves
//!   it out from then on. Options → File Types (`settings.json` → `fileTypes`) go to every scan.
//!
//! - **Upkeep while scanning** (M5.17, `security.md` §5.8, `data-model.md` §7): each folder handed
//!   out has its stale `.dalica-tmp-*` files removed on the scan's thread
//!   ([`crate::watching::clean_temps`], through `dalica_fs::remove_stale_temps` at the session
//!   time the scan started). A walked root that turns out to hold no entries at all (the mount
//!   point an unmounted volume leaves behind) removes nothing: its folders stay, as under a root
//!   that cannot be walked, with a warning when the library holds folders there. Every walked unit
//!   counts as verified for the schedule in [`crate::watching`].
//!
//! Seams: [`crate::watching`] starts the periodic verification through
//! [`Session::start_verification`]'s units and runs its own incremental rescans of dirty folders
//! ([`Session::busy`] and [`Session::wait_idle`] include them); M5.16 replaces the amortised
//! whole-library thumbnail sync below with one that queues just the photos of each batch, the
//! first screenful first.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use dalica_library::{Library, Op};
use dalica_scan::{Event, Failure, FoundFolder, HeaderReader, Plan, Progress, Scan, Summary};
use serde::Serialize;
use serde_json::json;

use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::session::{Msg, Session};

/// The most photos [`Session::poll`] applies in one call: one `diff` batch.
pub const PHOTOS_PER_POLL: usize = dalica_scan::BATCH;
/// While scans land, the whole-library thumbnail sync runs when the photos applied since the last
/// one reach 1/`SYNC_RATIO` of the library (and after each scan's first batch), so its cost stays
/// a small multiple of the photos added.
const SYNC_RATIO: usize = 8;

/// What a background scan is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanKind {
    /// New or rescanned roots: merge into the model.
    Merge,
    /// Rebuild the whole model from the roots.
    Rebuild,
}

/// Scan progress as `library.inspect` → `scan` shows it: the running scans summed, or the last
/// finished scan's totals when none runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub running: bool,
    /// Folders found and not yet read, plus folders read and not yet applied.
    pub folders_queued: usize,
    /// Photos the walks listed.
    pub found: usize,
    /// Photos applied to the library.
    pub indexed: usize,
}

/// What a scanning thread sends.
enum Streamed {
    Event(Event),
    /// The walk's summary, and the walked roots that hold no entries at all.
    End(Summary, Vec<PathBuf>),
}

/// The session's scans.
#[derive(Default)]
pub(crate) struct Scanning {
    running: Vec<ScanJob>,
    /// The failures in effect, given back to every scan.
    failures: Vec<Failure>,
    /// The header reader scans use; `None` for `dalica-image`'s.
    reader: Option<HeaderReader>,
    applied_since_sync: usize,
    synced_photos: usize,
    last: ScanProgress,
}

/// One running scan, as the owner sees it.
struct ScanJob {
    job: JobId,
    kind: ScanKind,
    /// The roots it walks and the subtrees it leaves out.
    plan: Plan,
    /// `None` once the walk ended, or was let go when the job was cancelled.
    rx: Option<Receiver<Streamed>>,
    /// Folders read and not yet diffed.
    queue: VecDeque<FoundFolder>,
    /// The folder whose ops are being applied, and the ops still to apply.
    current: Option<FoundFolder>,
    pending: VecDeque<Op>,
    /// Every folder the walk handed out.
    seen: HashSet<PathBuf>,
    walker: Progress,
    indexed: usize,
    folders: usize,
    summary: Option<Summary>,
    /// Walked roots that hold no entries at all: nothing under them is removed.
    empty: Vec<PathBuf>,
    /// The thread went away without a summary.
    lost: bool,
    /// Cancelled: nothing more is taken in.
    stopped: bool,
    /// A rebuild's model, swapped in at the end.
    fresh: Option<Library>,
    synced: bool,
}

impl ScanJob {
    fn new(job: JobId, kind: ScanKind, plan: Plan, rx: Receiver<Streamed>, fresh: Option<Library>) -> ScanJob {
        ScanJob {
            job,
            kind,
            plan,
            rx: Some(rx),
            queue: VecDeque::new(),
            current: None,
            pending: VecDeque::new(),
            seen: HashSet::new(),
            walker: Progress::default(),
            indexed: 0,
            folders: 0,
            summary: None,
            empty: Vec::new(),
            lost: false,
            stopped: false,
            fresh,
            synced: false,
        }
    }

    /// Takes in what the walk sent: everything waiting, or (with `block`) everything until it ends.
    fn receive(&mut self, block: bool, warnings: &mut Vec<String>) {
        let Some(rx) = self.rx.take() else { return };
        loop {
            let message = if block { rx.recv().map_err(|_| TryRecvError::Disconnected) } else { rx.try_recv() };
            match message {
                Ok(Streamed::Event(e)) => self.event(e, warnings),
                Ok(Streamed::End(summary, empty)) => {
                    self.summary = Some(summary);
                    self.empty = empty;
                    return;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.lost = true;
                    return;
                }
            }
        }
        self.rx = Some(rx);
    }

    fn event(&mut self, event: Event, warnings: &mut Vec<String>) {
        match event {
            Event::Folder(folder) => {
                self.seen.insert(folder.path.clone());
                self.queue.push_back(folder);
            }
            Event::Progress(p) => self.walker = p,
            Event::Failed(f) => warnings.push(format!("{}: {}", f.path.display(), f.reason)),
            Event::Warning(w) => warnings.push(format!("{}: {}", w.path.display(), w.reason)),
        }
    }

    /// Cancelled: let the walk go (it stops at its next file) and drop what is not applied yet,
    /// except the rest of the folder being applied.
    fn stop(&mut self) {
        self.stopped = true;
        self.rx = None;
        self.queue.clear();
    }

    /// Everything taken in is applied and nothing more comes.
    fn is_complete(&self) -> bool {
        (self.stopped || self.summary.is_some() || self.lost) && self.queue.is_empty() && self.pending.is_empty() && self.current.is_none()
    }

    /// After the model was replaced: the folder being applied is diffed again against the new one.
    fn rediff(&mut self) {
        self.pending.clear();
        if let Some(folder) = self.current.take() {
            self.queue.push_front(folder);
        }
    }

    fn label(&self) -> String {
        let found = pictures(self.walker.found.max(self.indexed));
        match self.kind {
            ScanKind::Merge => format!("Scanning: {found} found"),
            ScanKind::Rebuild => format!("Rebuilding the library: {found} found"),
        }
    }

    fn progress(&self) -> JobProgress {
        let done = u64::try_from(self.indexed).unwrap_or(u64::MAX);
        let total = u64::try_from(self.walker.found).unwrap_or(u64::MAX).max(done);
        JobProgress { done, total }
    }
}

/// The photos one op adds, changes or removes (at least 1): what it costs from the poll's budget.
pub(crate) fn cost(op: &Op) -> usize {
    match op {
        Op::AddPhotos { photos } | Op::UpdatePhotos { photos } => photos.len().max(1),
        Op::RemovePhotos { ids } => ids.len().max(1),
        _ => 1,
    }
}

/// What one poll may still apply.
pub(crate) struct Budget {
    left: usize,
    used: bool,
}

impl Budget {
    pub(crate) fn new(photos: usize) -> Budget {
        Budget { left: photos, used: false }
    }

    /// Whether `n` more fits (the first piece of a poll always does).
    pub(crate) fn fits(&self, n: usize) -> bool {
        !self.used || n <= self.left
    }

    pub(crate) fn take(&mut self, n: usize) -> bool {
        if !self.fits(n) {
            return false;
        }
        self.left = self.left.saturating_sub(n);
        self.used = true;
        true
    }
}

/// "1 picture", "1,204 pictures".
pub(crate) fn pictures(n: usize) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && digits.len().saturating_sub(i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if n == 1 { format!("{grouped} picture") } else { format!("{grouped} pictures") }
}

impl Session {
    /// Starts scans on threads of their own, listed as jobs; returns the last one's id. A merge
    /// scans the region of each of `roots`, one job each; a rebuild walks every unit (`roots` are
    /// the units: its plan comes from the rules).
    pub(crate) fn start_scan(&mut self, roots: Vec<PathBuf>, kind: ScanKind) -> Option<JobId> {
        match kind {
            ScanKind::Rebuild => {
                let plan = self.folders.rules.everything();
                Some(self.start_plan(plan, kind))
            }
            ScanKind::Merge => roots.iter().map(|r| self.start_region_scan(r)).last(),
        }
    }

    /// Scans the region of `folder` (`folder` without the folders inside it that set a mode) in
    /// the background, merging what it finds; returns the job.
    pub(crate) fn start_region_scan(&mut self, folder: &Path) -> JobId {
        let plan = self.folders.rules.region(folder);
        self.start_plan(plan, ScanKind::Merge)
    }

    /// Starts the verification pass: a scan of each Scan Always unit and of each Scan Once unit not
    /// indexed yet (a frozen Scan Once unit is never rescanned). Returns the jobs.
    pub fn start_verification(&mut self) -> Vec<JobId> {
        self.folders.rules.verification().iter().map(|u| self.start_region_scan(u)).collect()
    }

    /// Starts one scan of `plan` on a thread of its own, listed as a job; returns its id.
    fn start_plan(&mut self, plan: Plan, kind: ScanKind) -> JobId {
        let now = self.clock.now();
        let job_kind = if kind == ScanKind::Rebuild { JobKind::Rebuild } else { JobKind::Scan };
        let (job, cancel) = self.jobs.start(job_kind, "Starting", true, now);
        // A rebuild builds its model to the side, unless there is nothing to keep meanwhile.
        let fresh = (kind == ScanKind::Rebuild && self.library.folders().next().is_some()).then(Library::new);
        if kind == ScanKind::Rebuild && fresh.is_none() {
            self.undo.clear();
        }
        let mut options = self.scan_options();
        options.cancel = Some(cancel);
        options.excluded = plan.excluded.clone();
        let (tx, rx) = mpsc::channel();
        let walk = plan.roots.clone();
        let spawned = std::thread::Builder::new().name("dalica-scan".into()).spawn(move || {
            let summary = dalica_scan::scan_streaming(&walk, &options, |e| {
                if let Event::Folder(f) = &e {
                    for problem in crate::watching::clean_temps(&f.path, now) {
                        let warning = dalica_scan::Warning { path: f.path.clone(), reason: problem };
                        if tx.send(Streamed::Event(Event::Warning(warning))).is_err() {
                            return false;
                        }
                    }
                }
                tx.send(Streamed::Event(e)).is_ok()
            });
            let empty = summary.walked.iter().filter(|r| crate::watching::holds_nothing(r)).cloned().collect();
            let _ = tx.send(Streamed::End(summary, empty));
        });
        match spawned {
            Ok(_) => {
                let scan = ScanJob::new(job, kind, plan, rx, fresh);
                self.jobs.progress(job, scan.progress(), scan.label());
                self.scanning.running.push(scan);
            }
            Err(e) => {
                let message = format!("cannot start a scan: {e}");
                self.scan_warnings.push(message.clone());
                self.jobs.finish(job, Outcome::Failed(message), "Scan failed", now);
            }
        }
        job
    }

    /// Rebuilds the model from the watched folders in the background (`library.rebuild`).
    pub(crate) fn start_rebuild(&mut self) {
        let plan = self.folders.rules.everything();
        self.start_plan(plan, ScanKind::Rebuild);
    }

    /// The options the next scan gets (owned: they move to its thread): the failures in effect and
    /// Options → File Types. Built on the owner thread without touching the disk.
    pub(crate) fn scan_options(&self) -> dalica_scan::Options {
        let mut options =
            dalica_scan::Options { failures: self.scanning.failures.clone(), types: self.settings().file_types(), ..dalica_scan::Options::default() };
        if let Some(reader) = &self.scanning.reader {
            options.reader = Arc::clone(reader);
        }
        options
    }

    /// The header reader later scans use; `None` for `dalica-image`'s. Tests and benches plant
    /// slow, gated or failing readers through it.
    pub fn set_scan_reader(&mut self, reader: Option<HeaderReader>) {
        self.scanning.reader = reader;
    }

    /// How far the scans have got (`library.inspect` → `scan`).
    pub fn scan_progress(&self) -> ScanProgress {
        if self.scanning.running.is_empty() {
            return self.scanning.last;
        }
        let mut p = ScanProgress { running: true, ..ScanProgress::default() };
        for j in &self.scanning.running {
            p.folders_queued = p.folders_queued.saturating_add(j.walker.folders_queued + j.queue.len() + usize::from(j.current.is_some()));
            p.found = p.found.saturating_add(j.walker.found.max(j.indexed));
            p.indexed = p.indexed.saturating_add(j.indexed);
        }
        p
    }

    /// Whether a scan is still running (the watcher's rescans of dirty folders included).
    /// Thumbnail jobs are counted apart ([`Session::thumbnails_pending`]).
    pub fn busy(&self) -> bool {
        !self.scanning.running.is_empty() || self.rescanning()
    }

    /// Takes in what the scans found and applies up to `photos` of it; ends the scans that are
    /// complete. Returns how many folders were applied and scans ended.
    /// The Folder Manager's browse answers are taken in here too (`folders.rs`).
    pub(crate) fn poll_scans(&mut self, photos: usize) -> usize {
        let browsed = self.poll_browse(false);
        if self.scanning.running.is_empty() {
            return browsed;
        }
        let mut budget = Budget::new(photos);
        let mut n = 0;
        let mut running = Vec::new();
        let mut complete = Vec::new();
        for mut job in std::mem::take(&mut self.scanning.running) {
            job.receive(false, &mut self.scan_warnings);
            if !job.stopped && self.jobs.cancel_requested(job.job) {
                job.stop();
            }
            n += self.advance(&mut job, &mut budget);
            if job.is_complete() {
                complete.push(job);
            } else {
                self.jobs.progress(job.job, job.progress(), job.label());
                running.push(job);
            }
        }
        running.append(&mut self.scanning.running);
        self.scanning.running = running;
        for job in complete {
            if self.end_scan(job) {
                for other in &mut self.scanning.running {
                    if other.fresh.is_none() {
                        other.rediff();
                    }
                }
            }
            n += 1;
        }
        n
    }

    /// Blocks until every scan has ended and everything it found is applied, and every browse
    /// request is answered; then the same for the rescans of the dirty folders the watcher
    /// reported (which wait for scans to end), until neither runs.
    pub(crate) fn wait_scans(&mut self) {
        self.poll_browse(true);
        loop {
            while !self.scanning.running.is_empty() {
                for job in &mut self.scanning.running {
                    job.receive(true, &mut self.scan_warnings);
                }
                self.poll_scans(usize::MAX);
            }
            if !self.wait_rescans() {
                break;
            }
        }
    }

    /// Applies what fits in `budget` of `job`'s folders. Returns how many folders it completed.
    fn advance(&mut self, job: &mut ScanJob, budget: &mut Budget) -> usize {
        let mut folders = 0;
        let mut applied = 0;
        loop {
            if let Some(op) = job.pending.front() {
                let n = cost(op);
                if !budget.take(n) {
                    break;
                }
                let Some(op) = job.pending.pop_front() else { break };
                applied += n;
                let result = match &mut job.fresh {
                    Some(fresh) => fresh.apply(op).map(|_| ()).map_err(|e| e.to_string()),
                    None => self.apply(op).map(|_| ()).map_err(|e| e.to_string()),
                };
                if let Err(e) = result {
                    self.scan_warnings.push(format!("scan result not applied: {e}"));
                }
                continue;
            }
            if let Some(done) = job.current.take() {
                job.indexed = job.indexed.saturating_add(done.photos.len());
                job.folders += 1;
                folders += 1;
                continue;
            }
            let Some(next) = job.queue.front() else { break };
            if !budget.fits(next.photos.len().max(1)) {
                break;
            }
            let Some(folder) = job.queue.pop_front() else { break };
            if !self.folders.rules.indexes(&folder.path) {
                // Removed meanwhile, or inside a removed folder a rebuild walked for a unit in it.
                continue;
            }
            let scan = Scan { folders: vec![folder], ..Scan::default() };
            let ops = match &mut job.fresh {
                Some(fresh) => dalica_scan::diff(fresh, &scan),
                None => dalica_scan::diff(&mut self.library, &scan),
            };
            let folder = scan.folders.into_iter().next();
            if ops.is_empty() {
                // Nothing changed: the diff was the work.
                budget.take(folder.as_ref().map_or(1, |f| f.photos.len().max(1)));
            } else {
                budget.take(1);
            }
            job.current = folder;
            job.pending = ops.into();
        }
        if applied > 0 && job.fresh.is_none() {
            self.scanning.applied_since_sync = self.scanning.applied_since_sync.saturating_add(applied);
            let due = self.scanning.applied_since_sync.saturating_mul(SYNC_RATIO) >= self.scanning.synced_photos;
            if due || !job.synced {
                job.synced = true;
                self.sync_thumbnails();
            }
        }
        folders
    }

    /// Ends a complete scan: the verification pass (or a rebuild's swap), failures remembered, the
    /// thumbnails brought in line, and the job closed. Returns whether the model was replaced.
    fn end_scan(&mut self, mut job: ScanJob) -> bool {
        let summary = job.summary.take();
        let cancelled = job.stopped || summary.as_ref().is_some_and(|s| s.cancelled);
        let mut replaced = false;
        if let Some(s) = &summary {
            self.remember_failures(s);
        }
        if let Some(s) = summary.as_ref().filter(|_| !cancelled) {
            match job.fresh.take() {
                Some(fresh) => {
                    self.library = fresh;
                    self.undo.clear();
                    self.ops_since_snapshot = 0;
                    self.send(Msg::Snapshot(Box::new(self.library.clone())));
                    replaced = true;
                }
                None => {
                    // A walked root holding no entries at all (an unmounted volume's mount point)
                    // counts as one that could not be walked: nothing under it is removed.
                    for root in &job.empty {
                        let kept = self.library.folders().filter(|f| f.path.starts_with(root)).count();
                        if kept > 0 {
                            self.scan_warnings.push(format!(
                                "{}: the folder is empty; its {kept} folders stay in the library (is its volume unmounted?)",
                                root.display()
                            ));
                        }
                    }
                    // Folders under a root walked to the end that hold no photos any more (the
                    // subtrees it left out belong to other scans).
                    let excluded = &job.plan.excluded;
                    let empty = &job.empty;
                    let gone: Vec<_> = self
                        .library
                        .folders()
                        .filter(|f| {
                            s.walked.iter().any(|root| f.path.starts_with(root) && !empty.contains(root))
                                && !excluded.iter().any(|x| f.path.starts_with(x))
                                && !job.seen.contains(&f.path)
                        })
                        .map(|f| f.id)
                        .collect();
                    for id in gone {
                        if let Err(e) = self.apply(Op::RemoveFolder { id }) {
                            self.scan_warnings.push(format!("scan result not applied: {e}"));
                        }
                    }
                }
            }
        }
        if let Some(s) = summary.as_ref().filter(|_| !cancelled) {
            let walked = Plan { roots: s.walked.clone(), excluded: job.plan.excluded.clone() };
            self.note_verified(&walked);
            if self.folders.rules.mark_indexed(&walked) {
                self.save_folders();
            }
        }
        self.sync_thumbnails();
        let found = job.walker.found.max(job.indexed);
        self.scanning.last = ScanProgress { running: false, folders_queued: 0, found, indexed: job.indexed };
        let result = json!({
            "roots": job.plan.roots,
            "folders": job.folders,
            "found": found,
            "indexed": job.indexed,
            "failed": job.walker.failed,
            "walked": summary.as_ref().map(|s| s.walked.clone()).unwrap_or_default(),
            "cancelled": cancelled,
        });
        let what = if job.kind == ScanKind::Rebuild { "Rebuild" } else { "Scan" };
        let (outcome, label) = if job.lost && summary.is_none() && !job.stopped {
            let message = "the scan stopped unexpectedly".to_string();
            self.scan_warnings.push(message.clone());
            (Outcome::Failed(message), format!("{what} failed"))
        } else if cancelled {
            (Outcome::Cancelled(result), format!("{what} cancelled: {} indexed", pictures(job.indexed)))
        } else {
            (Outcome::Done(result), format!("{what} finished: {}", pictures(job.indexed)))
        };
        self.jobs.progress(job.job, job.progress(), label.clone());
        self.jobs.finish(job.job, outcome, label, self.clock.now());
        replaced
    }

    /// Keeps the failures in effect: those under the roots `summary` walked to the end are
    /// replaced by its own; the rest stay.
    fn remember_failures(&mut self, summary: &Summary) {
        let failures = &mut self.scanning.failures;
        failures.retain(|f| !summary.walked.iter().any(|root| f.path.starts_with(root)) && !summary.failures.iter().any(|n| n.path == f.path));
        failures.extend(summary.failures.iter().cloned());
        failures.sort_by(|a, b| a.path.cmp(&b.path));
    }

    /// Brings the thumbnailer in line with the whole library (one pass over it).
    pub(crate) fn sync_thumbnails(&mut self) {
        self.thumbs.sync(&self.library);
        self.scanning.synced_photos = self.library.photos().count();
        self.scanning.applied_since_sync = 0;
    }

    /// Whether no scan runs (the watcher's rescans aside).
    pub(crate) fn scans_idle(&self) -> bool {
        self.scanning.running.is_empty()
    }

    /// Keeps the failures in effect after an incremental rescan: those `covers` says it looked at
    /// again are replaced by its own (`found`) there; the rest stay. Returns the ones that are new.
    pub(crate) fn remember_rescan_failures(&mut self, covers: &dyn Fn(&Path) -> bool, found: &[Failure]) -> Vec<Failure> {
        let failures = &mut self.scanning.failures;
        let fresh: Vec<Failure> = found.iter().filter(|f| covers(&f.path) && !failures.iter().any(|o| o.path == f.path)).cloned().collect();
        failures.retain(|f| !covers(&f.path));
        failures.extend(found.iter().filter(|f| covers(&f.path)).cloned());
        failures.sort_by(|a, b| a.path.cmp(&b.path));
        failures.dedup_by(|b, a| a.path == b.path);
        fresh
    }

    /// The failures the scans remember (given back to the next one), sorted by path.
    pub fn scan_failures(&self) -> &[Failure] {
        &self.scanning.failures
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(pictures(0), "0 pictures");
        assert_eq!(pictures(1), "1 picture");
        assert_eq!(pictures(999), "999 pictures");
        assert_eq!(pictures(1_204), "1,204 pictures");
        assert_eq!(pictures(12_345_678), "12,345,678 pictures");
    }

    #[test]
    fn a_budget_always_lets_the_first_piece_through_then_stops_at_its_size() {
        let mut b = Budget::new(500);
        assert!(b.take(800), "the first piece of a poll always fits");
        assert!(!b.fits(1));
        let mut b = Budget::new(500);
        assert!(b.take(300) && b.take(200) && !b.take(1));
    }
}
