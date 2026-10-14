//! The file-job runner (`architecture.md` §5): one way to run every job that writes photo files —
//! Save, Save a Copy, Save As and Export (M8) — in the background, with progress and Cancel, and
//! to bring what it wrote into the library. The pattern follows LightCraft's export batch (its
//! `run_batch_with` and `export_task.rs`): owned inputs per item, items taken in order by up to
//! `workers` threads, each item's panic its own failure, results placed in item order.
//!
//! - **Submitting.** A command builds a [`FileJob`]: its [`JobKind`], its label ("Saving",
//!   "Exporting"), the items (each owns everything its work needs and names its file,
//!   [`FileItem`]) and the work, a function run on a worker thread for one item that writes its
//!   files and says what it did to them ([`Done`], [`FileChange`]). [`Session::start_file_job`]
//!   lists it as a job (`jobs.list`) and starts its threads; the owner thread touches no file.
//! - **Running.** Each worker thread takes the next item, runs the work (a panic is that item's
//!   failure, never the job's end), then reads back the files it replaced or created (size,
//!   modification time, pixel size and date, as a scan reads a file) and sends the result. Cancel
//!   is checked before each item: the items running finish, and no other starts.
//! - **Applying.** [`Session::poll`] applies the results in item order, each once: the files the
//!   item changed come into the library first (a replaced file keeps its photo's id and takes the
//!   new file's size, time, pixel size and date, the way a scan's change path updates a changed
//!   file; a new file in a folder of the library is added as a photo; a new file or folder outside
//!   the library has its folder scanned when the job ends, added as Scan Always when no watched
//!   folder covers it), then the caller's function turns the item's result into ops, applied as
//!   changes from the files (no undo step: Save's way back is Undo Save). The watcher's rescans
//!   and the verification pass wait while a file job runs, and a rescan or a full scan (start-up,
//!   verification, a folder added) running when it starts is put off and walks again once no file
//!   job runs (`Session::put_off_rescan`, `Session::put_off_scans`, M9.28), so no scan reads the
//!   files half-written and applies that over the job's results. Because the model takes the new
//!   size and time of every file the job wrote, the scan after the job finds nothing changed there
//!   and imports nothing from Dalica's own write. The thumbnails follow (`sync_thumbnails`).
//! - **Ending.** When every started item is applied, the job ends (`done`, or `cancelled` when
//!   Cancel left items unstarted) and a [`FileJobEvent`] (the message, the failures with their file
//!   and reason, the folder to show) is kept for the UI's notices ([`Session::file_job_events`]).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{DateTime, PhotoId, Timestamp};
use dalica_image::Format;
use dalica_library::{Op, Photo};
use serde_json::json;

use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::session::Session;

/// The most worker threads one file job runs.
pub const MAX_WORKERS: usize = 8;
/// The most finished-job events kept ([`Session::file_job_events`]); the oldest go first.
pub const MAX_EVENTS: usize = 64;
/// The most item results one [`Session::poll`] applies per job.
pub const RESULTS_PER_POLL: usize = 64;
/// How long [`Session::wait_file_jobs`] sleeps between two polls.
const WAIT_STEP: Duration = Duration::from_millis(2);

/// One item of a file job: owns what its work needs, and names the file it is about (for the
/// progress and the failure reports).
pub trait FileItem: Send + 'static {
    fn file(&self) -> &Path;
}

/// What one item's work did to the files, for the runner to bring into the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileChange {
    /// Photo `photo`'s file at `path` was replaced in place (Save, Undo Save, Revert): it keeps its
    /// id and takes the new file's size, time, pixel size and date.
    Replaced { photo: PhotoId, path: PathBuf },
    /// A new file (a copy, a JPEG beside its source, an exported photo): added as a photo when its
    /// folder is in the library, else its folder is scanned when the job ends.
    Created { path: PathBuf },
    /// A new folder (an export's): scanned when the job ends, added as Scan Always when no watched
    /// folder covers it.
    Folder { path: PathBuf },
}

/// What one item's work gives back: the files it changed and the caller's own result.
#[derive(Clone, Debug, PartialEq)]
pub struct Done<R> {
    pub changes: Vec<FileChange>,
    pub value: R,
}

/// One item's result as the caller's function gets it, once its files are in the library.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied<R> {
    /// The item's place in the job.
    pub index: usize,
    /// The item's file ([`FileItem::file`]).
    pub file: PathBuf,
    pub value: R,
    /// The photos the item's replaced and created files are now, by path: a replaced file with its
    /// photo's id, a new one with the id it was added under. Files whose folder waits for its
    /// scan are not listed.
    pub photos: Vec<(PathBuf, PhotoId)>,
}

/// An item that failed: its file and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemFailure {
    pub file: PathBuf,
    pub reason: String,
}

/// How a file job ended, for its message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub total: usize,
    /// Items done and applied.
    pub done: usize,
    pub failures: Vec<ItemFailure>,
    /// Cancel left items unstarted.
    pub cancelled: bool,
}

/// What the UI is told when a job ends: its message and the folder to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notice {
    pub message: String,
    pub folder: Option<PathBuf>,
}

/// A finished file job, for the UI's notices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileJobEvent {
    /// Rises by one per event in this session: the UI shows those it has not seen.
    pub seq: u64,
    pub job: JobId,
    pub kind: JobKind,
    pub message: String,
    pub done: usize,
    pub total: usize,
    pub cancelled: bool,
    pub failures: Vec<ItemFailure>,
    pub folder: Option<PathBuf>,
}

/// The work of one item, run on a worker thread.
pub type Work<I, R> = Arc<dyn Fn(I) -> Result<Done<R>, String> + Send + Sync>;
/// The caller's function that turns an applied item into ops (on the owner thread; `Send`, as the
/// session is).
pub type ApplyFn<R> = Box<dyn FnMut(&mut Session, Applied<R>) -> Result<Vec<Op>, String> + Send>;
/// The caller's function that words the job's end (on the owner thread).
pub type FinishFn = Box<dyn FnOnce(&mut Session, &Tally) -> Notice + Send>;

/// A job that writes files: its kind, label, items and work, and what to do with the results.
pub struct FileJob<I: FileItem, R: Send + 'static> {
    kind: JobKind,
    label: String,
    items: Vec<I>,
    workers: usize,
    work: Work<I, R>,
    apply: ApplyFn<R>,
    finish: FinishFn,
}

impl<I: FileItem, R: Send + 'static> FileJob<I, R> {
    /// A job of `kind` labelled `label` ("Exporting": the progress reads "Exporting 12 of 48") that
    /// runs `work` for each of `items` on one thread, applies no ops of its own and ends with
    /// [`default_message`].
    pub fn new(kind: JobKind, label: impl Into<String>, items: Vec<I>, work: impl Fn(I) -> Result<Done<R>, String> + Send + Sync + 'static) -> Self {
        let label = label.into();
        let finish_label = label.clone();
        FileJob {
            kind,
            label,
            items,
            workers: 1,
            work: Arc::new(work),
            apply: Box::new(|_, _| Ok(Vec::new())),
            finish: Box::new(move |_, t| Notice { message: default_message(&finish_label, t), folder: None }),
        }
    }

    /// Runs the items on up to `n` threads (1 ..= [`MAX_WORKERS`]).
    pub fn workers(mut self, n: usize) -> Self {
        self.workers = n.clamp(1, MAX_WORKERS);
        self
    }

    /// The ops each applied item brings (on the owner thread, in item order, once its files are
    /// in the library); an `Err` is that item's failure.
    pub fn on_applied(mut self, f: impl FnMut(&mut Session, Applied<R>) -> Result<Vec<Op>, String> + Send + 'static) -> Self {
        self.apply = Box::new(f);
        self
    }

    /// The message and folder of the job's end.
    pub fn on_finish(mut self, f: impl FnOnce(&mut Session, &Tally) -> Notice + Send + 'static) -> Self {
        self.finish = Box::new(f);
        self
    }
}

/// "Saving: 3 done", "Exporting cancelled: 1 of 3 done", "Exporting: 2 of 3 done, 1 failed".
pub fn default_message(label: &str, t: &Tally) -> String {
    let failed = t.failures.len();
    match (t.cancelled, failed) {
        (true, 0) => format!("{label} cancelled: {} of {} done", t.done, t.total),
        (true, f) => format!("{label} cancelled: {} of {} done, {f} failed", t.done, t.total),
        (false, 0) => format!("{label}: {} done", t.done),
        (false, f) => format!("{label}: {} of {} done, {f} failed", t.done, t.total),
    }
}

/// A file as read back after the work wrote it (on the worker thread).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Facts {
    size: u64,
    mtime: Timestamp,
    /// `None` when the header could not be read (then 0 × 0 and no date, as a scan lists it).
    format: Option<Format>,
    width: u32,
    height: u32,
    date: Option<DateTime>,
}

/// Each changed file as read back, in the order of [`Done::changes`].
type ReadBack = Vec<Result<Facts, String>>;

/// One item's result as a worker sends it.
struct Finished<R> {
    index: usize,
    file: PathBuf,
    result: Result<(Done<R>, ReadBack), String>,
}

/// One running file job, as the owner sees it.
struct Running<R: Send + 'static> {
    job: JobId,
    kind: JobKind,
    label: String,
    total: usize,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Finished<R>>,
    /// Results that came before an earlier item's.
    waiting: BTreeMap<usize, Finished<R>>,
    /// The next item to apply.
    next: usize,
    /// The workers are gone: every started item has sent its result.
    ended: bool,
    tally: Tally,
    /// Folders to scan when the job ends.
    scan: BTreeSet<PathBuf>,
    apply: ApplyFn<R>,
    finish: Option<FinishFn>,
}

/// A running job, whatever its item and result types.
trait Poll {
    fn cancel(&self);
    /// Applies what arrived (up to `budget` results); `true` once the job has ended.
    fn poll(&mut self, s: &mut Session, budget: usize) -> (usize, bool);
}

/// The session's file jobs and their events.
#[derive(Default)]
pub(crate) struct FileJobs {
    running: Vec<Box<dyn Poll + Send>>,
    events: VecDeque<FileJobEvent>,
    next_seq: u64,
}

impl FileJobs {
    /// Sets every running job's cancel flag (the session is closing): the items running finish,
    /// no other starts.
    pub(crate) fn cancel_all(&self) {
        for j in &self.running {
            j.cancel();
        }
    }
}

/// Reads back a file the work wrote: its size and time, and its header as a scan reads it.
fn read_back(path: &Path) -> Result<Facts, String> {
    let stat = dalica_fs::stat(path).map_err(|e| e.to_string())?.filter(|s| s.is_file).ok_or_else(|| "the file is not there".to_string())?;
    let (size, mtime) = (stat.identity.size(), Timestamp::from_system_time(stat.times.modified));
    let header = catch_unwind(AssertUnwindSafe(|| dalica_image::read_header_file(path, &dalica_image::Limits::default())));
    Ok(match header {
        Ok(Ok((h, _))) => {
            let (width, height) = h.display_size();
            let date = h.exif.as_ref().and_then(|e| e.date_time_original);
            Facts { size, mtime, format: Some(h.format), width, height, date }
        }
        _ => Facts { size, mtime, format: None, width: 0, height: 0, date: None },
    })
}

/// The text of a caught panic.
fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default()
}

/// A worker thread: takes the next item until none is left or Cancel is set.
fn worker<I: FileItem, R: Send + 'static>(
    queue: &Mutex<VecDeque<(usize, I)>>,
    cancel: &AtomicBool,
    work: &Work<I, R>,
    tx: &mpsc::Sender<Finished<R>>,
) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let Some((index, item)) = queue.lock().unwrap_or_else(PoisonError::into_inner).pop_front() else { return };
        let file = item.file().to_path_buf();
        let result = match catch_unwind(AssertUnwindSafe(|| work(item))) {
            Ok(Ok(done)) => {
                let facts = done
                    .changes
                    .iter()
                    .map(|c| match c {
                        FileChange::Replaced { path, .. } | FileChange::Created { path } => read_back(path),
                        FileChange::Folder { .. } => Err(String::new()),
                    })
                    .collect();
                Ok((done, facts))
            }
            Ok(Err(reason)) => Err(reason),
            Err(p) => {
                let text = panic_text(p.as_ref());
                log::error!("a file job's work panicked on {}: {text}", file.display());
                Err(format!("internal error: {text}"))
            }
        };
        if tx.send(Finished { index, file, result }).is_err() {
            return;
        }
    }
}

impl Session {
    /// Starts `job`: lists it (`jobs.list`, cancellable) and runs its items on worker threads.
    /// Returns its id; its results are applied by [`Session::poll`]. Never touches the disk.
    pub fn start_file_job<I: FileItem, R: Send + 'static>(&mut self, job: FileJob<I, R>) -> JobId {
        let FileJob { kind, label, items, workers, work, apply, finish } = job;
        // No scan overlaps the job's writes: a rescan or a full scan running now is put off until
        // no file job runs.
        self.put_off_rescan();
        self.put_off_scans();
        let total = items.len();
        let now = self.clock.now();
        let (id, cancel) = self.jobs.start(kind, progress_label(&label, 0, total), true, now);
        self.jobs.progress(id, JobProgress { done: 0, total: total as u64 }, progress_label(&label, 0, total));
        let queue: Arc<Mutex<VecDeque<(usize, I)>>> = Arc::new(Mutex::new(items.into_iter().enumerate().collect()));
        let (tx, rx) = mpsc::channel();
        let mut started = 0usize;
        for n in 0..workers.min(total) {
            let (queue, cancel, work, tx) = (Arc::clone(&queue), Arc::clone(&cancel), Arc::clone(&work), tx.clone());
            let spawned = std::thread::Builder::new().name(format!("dalica-file-job-{n}")).spawn(move || worker(&queue, &cancel, &work, &tx));
            match spawned {
                Ok(_) => started += 1,
                Err(e) => log::warn!("a file job's worker could not start: {e}"),
            }
        }
        drop(tx);
        let mut running = Running {
            job: id,
            kind,
            label,
            total,
            cancel,
            rx,
            waiting: BTreeMap::new(),
            next: 0,
            ended: false,
            tally: Tally { total, ..Tally::default() },
            scan: BTreeSet::new(),
            apply,
            finish: Some(finish),
        };
        if started == 0 && total > 0 {
            // No thread: nothing ran. The job fails at once, every item reported.
            let reason = "no worker thread could start".to_string();
            let failures = queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .drain(..)
                .map(|(_, item)| ItemFailure { file: item.file().to_path_buf(), reason: reason.clone() })
                .collect();
            running.tally.failures = failures;
            running.end(self, Some(reason));
            return id;
        }
        self.file_jobs.running.push(Box::new(running));
        id
    }

    /// Applies the results of the running file jobs and ends those that are complete (from
    /// [`Session::poll`]). Returns how many results it applied.
    pub(crate) fn poll_file_jobs(&mut self) -> usize {
        if self.file_jobs.running.is_empty() {
            return 0;
        }
        let mut running = std::mem::take(&mut self.file_jobs.running);
        let mut n = 0;
        running.retain_mut(|j| {
            let (applied, ended) = j.poll(self, RESULTS_PER_POLL);
            n += applied;
            !ended
        });
        // Jobs a caller's function started meanwhile.
        running.append(&mut self.file_jobs.running);
        self.file_jobs.running = running;
        n
    }

    /// Whether a file job is still running.
    pub fn file_jobs_running(&self) -> bool {
        !self.file_jobs.running.is_empty()
    }

    /// Polls until no file job runs or `timeout` has passed; `false` if time ran out. Blocks the
    /// calling thread (the CLI, tests), never touching the disk on it.
    pub fn wait_file_jobs(&mut self, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            self.poll();
            if self.file_jobs.running.is_empty() {
                return true;
            }
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(WAIT_STEP);
        }
    }

    /// Applies the file jobs' results until none runs, without the rest of [`Session::poll`]
    /// (for [`Session::wait_idle`]).
    pub(crate) fn finish_file_jobs(&mut self) {
        while !self.file_jobs.running.is_empty() {
            if self.poll_file_jobs() == 0 {
                std::thread::sleep(WAIT_STEP);
            }
        }
    }

    /// The finished file jobs, oldest first (at most [`MAX_EVENTS`]): the UI turns the ones it has
    /// not seen (by [`FileJobEvent::seq`]) into notices.
    pub fn file_job_events(&self) -> impl Iterator<Item = &FileJobEvent> {
        self.file_jobs.events.iter()
    }

    /// Brings the files an item changed into the library; returns the photos they are now.
    fn apply_file_changes(
        &mut self,
        changes: &[FileChange],
        facts: &[Result<Facts, String>],
        scan: &mut BTreeSet<PathBuf>,
    ) -> Vec<(PathBuf, PhotoId)> {
        let mut photos = Vec::new();
        for (i, change) in changes.iter().enumerate() {
            let facts = facts.get(i).and_then(|f| f.as_ref().ok());
            match change {
                FileChange::Folder { path } => {
                    scan.insert(path.clone());
                }
                FileChange::Replaced { photo, path } => {
                    let Some(facts) = facts else { continue };
                    if let Some(id) = self.refresh_file(Some(*photo), path, facts, scan) {
                        photos.push((path.clone(), id));
                    }
                }
                FileChange::Created { path } => {
                    let Some(facts) = facts else { continue };
                    if let Some(id) = self.refresh_file(None, path, facts, scan) {
                        photos.push((path.clone(), id));
                    }
                }
            }
        }
        photos
    }

    /// Updates the photo at `path` (by `photo`, else by its name) to `facts`, keeping its id, or
    /// adds it as a new photo when its folder is in the library and no photo has that name. A
    /// file whose folder is not in the library leaves its folder to be scanned. Returns the photo.
    fn refresh_file(&mut self, photo: Option<PhotoId>, path: &Path, facts: &Facts, scan: &mut BTreeSet<PathBuf>) -> Option<PhotoId> {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else { return None };
        let Some(folder) = self.library.folder_by_path(dir) else {
            scan.insert(dir.to_path_buf());
            return None;
        };
        let known = photo
            .and_then(|id| self.library.photo(id))
            .filter(|p| p.folder == folder && p.name == name)
            .or_else(|| self.library.photo_by_name(folder, name))
            .cloned();
        let result = match known {
            Some(old) => {
                let new = Photo { size: facts.size, mtime: facts.mtime, width: facts.width, height: facts.height, date: facts.date, ..old.clone() };
                if new == old {
                    return Some(old.id);
                }
                self.apply(Op::UpdatePhotos { photos: vec![new] }).map(|_| old.id)
            }
            None => {
                // A type Options → File Types leaves out is not listed, as a scan would not list it.
                if facts.format.is_some_and(|f| !self.settings().file_types().allows_format(f)) {
                    return None;
                }
                let id = self.library.next_photo_id();
                let photo = Photo {
                    id,
                    folder,
                    name: name.to_string(),
                    size: facts.size,
                    mtime: facts.mtime,
                    width: facts.width,
                    height: facts.height,
                    date: facts.date,
                    star: false,
                };
                self.apply(Op::AddPhotos { photos: vec![photo] }).map(|_| id)
            }
        };
        match result {
            Ok(id) => Some(id),
            Err(e) => {
                log::warn!("{} not brought into the library: {e}", path.display());
                None
            }
        }
    }

    /// Scans the folders a job wrote outside the library: a folder some watched folder covers is
    /// scanned, any other is added as Scan Always (and scanned). A folder inside another one
    /// scanned here is left to that scan.
    fn scan_written_folders(&mut self, folders: BTreeSet<PathBuf>) {
        let all: Vec<PathBuf> = folders.into_iter().collect();
        for f in all.iter().filter(|f| !all.iter().any(|o| o != *f && f.starts_with(o))) {
            if self.folder_rules().indexes(f) {
                self.start_region_scan(f);
            } else {
                self.add_root(f.clone());
            }
        }
    }

    fn push_file_job_event(&mut self, mut event: FileJobEvent) {
        event.seq = self.file_jobs.next_seq;
        self.file_jobs.next_seq = self.file_jobs.next_seq.saturating_add(1);
        let events = &mut self.file_jobs.events;
        if events.len() >= MAX_EVENTS {
            events.pop_front();
        }
        events.push_back(event);
    }
}

/// "Exporting 12 of 48".
fn progress_label(label: &str, done: usize, total: usize) -> String {
    format!("{label} {done} of {total}")
}

impl<R: Send + 'static> Running<R> {
    /// Applies one item's result: its files, then the caller's ops.
    fn apply_one(&mut self, s: &mut Session, f: Finished<R>) {
        let Finished { index, file, result } = f;
        match result {
            Err(reason) => self.tally.failures.push(ItemFailure { file, reason }),
            Ok((done, facts)) => {
                let Done { changes, value } = done;
                let photos = s.apply_file_changes(&changes, &facts, &mut self.scan);
                let ops = (self.apply)(s, Applied { index, file: file.clone(), value, photos });
                let mut failed = None;
                match ops {
                    Ok(ops) => {
                        for op in ops {
                            if let Err(e) = s.apply(op) {
                                failed = Some(e.to_string());
                                break;
                            }
                        }
                    }
                    Err(reason) => failed = Some(reason),
                }
                match failed {
                    Some(reason) => self.tally.failures.push(ItemFailure { file, reason }),
                    None => self.tally.done += 1,
                }
            }
        }
    }

    /// Ends the job: its folders scanned, its entry closed, its event kept.
    fn end(&mut self, s: &mut Session, failed: Option<String>) {
        self.ended = true;
        s.scan_written_folders(std::mem::take(&mut self.scan));
        let applied = self.tally.done + self.tally.failures.len();
        self.tally.cancelled = self.cancel.load(Ordering::SeqCst) && applied < self.total;
        let notice = match self.finish.take() {
            Some(f) => f(s, &self.tally),
            None => Notice { message: default_message(&self.label, &self.tally), folder: None },
        };
        let failures: Vec<serde_json::Value> = self.tally.failures.iter().map(|f| json!({ "file": f.file, "reason": f.reason })).collect();
        let result = json!({
            "total": self.total,
            "done": self.tally.done,
            "failed": failures,
            "cancelled": self.tally.cancelled,
            "folder": notice.folder,
        });
        let outcome = match failed {
            Some(reason) => Outcome::Failed(reason),
            None if self.tally.cancelled => Outcome::Cancelled(result),
            None => Outcome::Done(result),
        };
        let progress = JobProgress { done: applied as u64, total: self.total as u64 };
        s.jobs.progress(self.job, progress, notice.message.clone());
        s.jobs.finish(self.job, outcome, notice.message.clone(), s.clock.now());
        s.push_file_job_event(FileJobEvent {
            seq: 0,
            job: self.job,
            kind: self.kind,
            message: notice.message,
            done: self.tally.done,
            total: self.total,
            cancelled: self.tally.cancelled,
            failures: self.tally.failures.clone(),
            folder: notice.folder,
        });
    }
}

impl<R: Send + 'static> Poll for Running<R> {
    fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn poll(&mut self, s: &mut Session, budget: usize) -> (usize, bool) {
        if self.ended {
            return (0, true);
        }
        // `jobs.cancel` sets the registry's flag, which is this one: the workers see it.
        let mut gone = false;
        loop {
            match self.rx.try_recv() {
                Ok(f) => {
                    self.waiting.insert(f.index, f);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        let mut n = 0;
        let before = self.tally.done + self.tally.failures.len();
        while n < budget {
            // In item order; once the workers are gone, the items never started leave gaps.
            let key = if gone { self.waiting.keys().next().copied() } else { self.waiting.contains_key(&self.next).then_some(self.next) };
            let Some(key) = key else { break };
            let Some(f) = self.waiting.remove(&key) else { break };
            self.next = key.saturating_add(1);
            self.apply_one(s, f);
            n += 1;
        }
        if n > 0 {
            s.sync_thumbnails();
        }
        let applied = self.tally.done + self.tally.failures.len();
        if applied != before {
            s.jobs.progress(
                self.job,
                JobProgress { done: applied as u64, total: self.total as u64 },
                progress_label(&self.label, applied, self.total),
            );
        }
        if gone && self.waiting.is_empty() {
            self.end(s, None);
            return (n, true);
        }
        (n, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_stays_send() {
        fn send<T: Send>() {}
        send::<Session>();
    }

    #[test]
    fn default_messages_say_what_happened() {
        let failure = ItemFailure { file: PathBuf::from("a.jpg"), reason: "disk full".into() };
        let t = |done, failures: Vec<ItemFailure>, cancelled| Tally { total: 3, done, failures, cancelled };
        assert_eq!(default_message("Saving", &t(3, vec![], false)), "Saving: 3 done");
        assert_eq!(default_message("Saving", &t(2, vec![failure.clone()], false)), "Saving: 2 of 3 done, 1 failed");
        assert_eq!(default_message("Exporting", &t(1, vec![], true)), "Exporting cancelled: 1 of 3 done");
        assert_eq!(default_message("Exporting", &t(1, vec![failure], true)), "Exporting cancelled: 1 of 3 done, 1 failed");
        assert_eq!(progress_label("Exporting", 12, 48), "Exporting 12 of 48");
    }

    #[test]
    fn workers_are_clamped() {
        struct Item(PathBuf);
        impl FileItem for Item {
            fn file(&self) -> &Path {
                &self.0
            }
        }
        let job =
            |n| FileJob::new(JobKind::Save, "Saving", vec![Item(PathBuf::from("a"))], |_: Item| Ok(Done { changes: vec![], value: () })).workers(n);
        assert_eq!(job(0).workers, 1);
        assert_eq!(job(3).workers, 3);
        assert_eq!(job(1000).workers, MAX_WORKERS);
    }
}
