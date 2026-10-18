//! The job registry (`architecture.md` §5, `control-protocol.md` §6.13): every long piece of work
//! the session runs on a worker (today scans and rebuilds; imports, exports, saves and batch edits
//! later) is listed here with its kind, label, state, progress, cancel flag and, once finished, its
//! result or error.
//!
//! The registry is plain data owned by the session's thread: workers never touch it. The module
//! that runs a job ([`crate::scanning`] for scans) registers it with [`Jobs::start`], hands the
//! worker the job's cancel flag, reports progress with [`Jobs::progress`] as results come back
//! through [`Session::poll`], and closes it with [`Jobs::finish`]. Finished jobs stay listed for
//! [`KEEP`] on the session clock, and the registry holds at most [`CAP`] entries (the oldest
//! finished ones go first; a running job is never dropped).
//!
//! The control methods `jobs.list`, `jobs.wait` and `jobs.cancel` (M5.14) and the activity
//! popover (M5.19) read it through [`Session::jobs`], [`Session::job`], [`Session::wait_job`] and
//! [`Session::cancel_jobs`].
//!
//! - **Answered jobs** (M12.5): a job whose worker needs nothing more from the session than to end
//!   it ([`Jobs::start_answered`]: `photo.effect.pick`) gets a channel instead of a module of its
//!   own: the worker sends its [`Answer`] once, and the registry ends the job with it when it next
//!   prunes (every [`Session::poll`]). A worker that stops without answering ends its job as
//!   failed.
//! - **The effect tiles' state** (M12.5, `effect_tiles.rs`) is kept here too
//!   ([`Jobs::effect_tiles`]): the registry of the session's background work is the one field of
//!   [`Session`] that module may reach without a field of its own.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use dalica_core::Timestamp;
use serde::Serialize;
use serde_json::Value;

use crate::error::CmdError;
use crate::session::Session;

/// A job's number, unique in its session (from 1).
pub type JobId = u64;

/// How long a finished job stays listed.
pub const KEEP: Duration = Duration::from_secs(10 * 60);
/// The most entries the registry keeps.
pub const CAP: usize = 64;

/// What a job does (`control-protocol.md` §6.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Scan,
    /// Copying photos in from a card, a drive or a folder (`import_run.rs`, `organizing.md` §3).
    Import,
    Export,
    Save,
    BatchEdit,
    MetadataWrite,
    Rebuild,
    Duplicates,
    Faces,
    Collage,
    /// A short look at one photo for the edit view (suggested crops; the Tuning wands).
    Analysis,
    /// A file operation: photos or folders moved, copied, renamed or sent to the trash, or such an
    /// operation undone or redone (`fileops.rs`).
    Files,
    /// A download over the network: a face model (`face_models.rs`), map tiles (`map_tiles.rs`).
    Network,
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    /// Done, failed or cancelled: nothing more will change.
    pub fn is_finished(self) -> bool {
        matches!(self, JobState::Done | JobState::Failed | JobState::Cancelled)
    }
}

/// How far a job has got: `done` of `total` (a scan's total rises as the walk finds photos).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct JobProgress {
    pub done: u64,
    pub total: u64,
}

/// One job as `jobs.list` shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEntry {
    pub job: JobId,
    pub kind: JobKind,
    /// What the activity popover shows ("Scanning: 1,204 pictures found").
    pub label: String,
    pub state: JobState,
    pub progress: JobProgress,
    pub cancellable: bool,
    /// When it started, in milliseconds on the session clock since the session opened.
    pub started_ms: u64,
    /// When it finished, likewise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// How a job ended, for [`Jobs::finish`].
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Done(Value),
    /// Stopped by its cancel flag; what it completed stays, and is described here.
    Cancelled(Value),
    Failed(String),
}

/// What the worker of an answered job sends when it ends: how, and the job's final label.
pub type Answer = (Outcome, String);

/// The label of an answered job whose worker stopped without answering.
pub const LOST: &str = "The work stopped without an answer";

struct Entry {
    info: JobEntry,
    cancel: Arc<AtomicBool>,
    finished_at: Option<Timestamp>,
    /// An answered job's channel, until it answers.
    answer: Option<Receiver<Answer>>,
}

/// The session's jobs, oldest first.
pub struct Jobs {
    entries: Vec<Entry>,
    next: JobId,
    /// When the session opened: `startedMs` counts from here.
    epoch: Timestamp,
    /// The effect tiles being made (`effect_tiles.rs`).
    pub(crate) effect_tiles: crate::effect_tiles::EffectTiles,
}

impl Jobs {
    /// An empty registry whose times count from `epoch`.
    pub fn new(epoch: Timestamp) -> Jobs {
        Jobs { entries: Vec::new(), next: 1, epoch, effect_tiles: crate::effect_tiles::EffectTiles::default() }
    }

    /// Registers a running job and returns its id and the cancel flag to give its worker.
    pub fn start(&mut self, kind: JobKind, label: impl Into<String>, cancellable: bool, now: Timestamp) -> (JobId, Arc<AtomicBool>) {
        let job = self.next;
        self.next = self.next.saturating_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let info = JobEntry {
            job,
            kind,
            label: label.into(),
            state: JobState::Running,
            progress: JobProgress::default(),
            cancellable,
            started_ms: self.ms(now),
            finished_ms: None,
            result: None,
            error: None,
        };
        self.entries.push(Entry { info, cancel: Arc::clone(&cancel), finished_at: None, answer: None });
        self.prune(now);
        (job, cancel)
    }

    /// Registers a running job whose worker ends it: the worker gets the cancel flag and the
    /// sender, and sends its [`Answer`] once; the registry ends the job with it at its next prune
    /// (every [`Session::poll`]).
    pub fn start_answered(
        &mut self,
        kind: JobKind,
        label: impl Into<String>,
        cancellable: bool,
        now: Timestamp,
    ) -> (JobId, Arc<AtomicBool>, Sender<Answer>) {
        let (job, cancel) = self.start(kind, label, cancellable, now);
        let (tx, rx) = mpsc::channel();
        if let Some(e) = self.entry_mut(job) {
            e.answer = Some(rx);
        }
        (job, cancel, tx)
    }

    /// Whether `job` is an answered job still waiting for its worker's answer.
    pub fn awaits_answer(&self, job: JobId) -> bool {
        self.entries.iter().any(|e| e.info.job == job && e.answer.is_some())
    }

    /// The running answered jobs of `kind` labelled `label` (a cap on how many run at once).
    pub fn answered_running(&self, kind: JobKind, label: &str) -> usize {
        self.entries.iter().filter(|e| e.answer.is_some() && e.info.kind == kind && e.info.label == label).count()
    }

    /// Ends the answered jobs whose workers answered (or stopped without answering); returns how
    /// many. Never blocks.
    pub fn collect(&mut self, now: Timestamp) -> usize {
        let mut ended = Vec::new();
        for e in &mut self.entries {
            let Some(rx) = &e.answer else { continue };
            let answer = match rx.try_recv() {
                Ok(a) => a,
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => (Outcome::Failed(LOST.into()), LOST.to_string()),
            };
            e.answer = None;
            ended.push((e.info.job, answer));
        }
        let n = ended.len();
        for (job, (outcome, label)) in ended {
            self.finish_one(job, outcome, label, now);
        }
        n
    }

    /// Updates a running job's progress and label. A finished or unknown job is left alone.
    pub fn progress(&mut self, job: JobId, progress: JobProgress, label: impl Into<String>) {
        if let Some(e) = self.entry_mut(job)
            && !e.info.state.is_finished()
        {
            e.info.progress = progress;
            e.info.label = label.into();
        }
    }

    /// Closes a job with its outcome and final label. A job already finished keeps its first end.
    pub fn finish(&mut self, job: JobId, outcome: Outcome, label: impl Into<String>, now: Timestamp) {
        self.finish_one(job, outcome, label, now);
        self.prune(now);
    }

    fn finish_one(&mut self, job: JobId, outcome: Outcome, label: impl Into<String>, now: Timestamp) {
        let ms = self.ms(now);
        if let Some(e) = self.entry_mut(job)
            && !e.info.state.is_finished()
        {
            let (state, result, error) = match outcome {
                Outcome::Done(v) => (JobState::Done, Some(v), None),
                Outcome::Cancelled(v) => (JobState::Cancelled, Some(v), None),
                Outcome::Failed(m) => (JobState::Failed, None, Some(m)),
            };
            e.info.state = state;
            e.info.result = result;
            e.info.error = error;
            e.info.label = label.into();
            e.info.finished_ms = Some(ms);
            e.finished_at = Some(now);
            e.answer = None;
        }
    }

    /// Sets the cancel flag of `job` (or, with `None`, of every running cancellable job) and returns
    /// the jobs it was set on. Its worker stops at its next boundary (a scan: before the next file)
    /// and the job ends as cancelled at the next [`Session::poll`]. A finished job is not listed;
    /// an unknown one (or one already expired) is `NotFound`.
    pub fn cancel(&mut self, job: Option<JobId>, now: Timestamp) -> Result<Vec<JobId>, CmdError> {
        if let Some(id) = job
            && self.get(id, now).is_none()
        {
            return Err(CmdError::NotFound(format!("no job {id}")));
        }
        let mut out = Vec::new();
        for e in &self.entries {
            if job.is_none_or(|id| id == e.info.job) && e.info.cancellable && !e.info.state.is_finished() {
                e.cancel.store(true, Ordering::SeqCst);
                out.push(e.info.job);
            }
        }
        Ok(out)
    }

    /// Whether `job`'s cancel flag is set.
    pub fn cancel_requested(&self, job: JobId) -> bool {
        self.entries.iter().find(|e| e.info.job == job).is_some_and(|e| e.cancel.load(Ordering::SeqCst))
    }

    /// The jobs listed at `now`, oldest first: running ones and those finished less than [`KEEP`]
    /// ago, at most [`CAP`].
    pub fn list(&self, now: Timestamp) -> Vec<JobEntry> {
        self.entries.iter().filter(|e| !expired(e, now)).map(|e| e.info.clone()).collect()
    }

    /// One job, if it is still listed at `now`.
    pub fn get(&self, job: JobId, now: Timestamp) -> Option<JobEntry> {
        self.entries.iter().find(|e| e.info.job == job && !expired(e, now)).map(|e| e.info.clone())
    }

    /// Whether any job is still running.
    pub fn any_running(&self) -> bool {
        self.entries.iter().any(|e| !e.info.state.is_finished())
    }

    /// Ends the answered jobs that answered ([`Jobs::collect`]), then drops the finished jobs older
    /// than [`KEEP`], then the oldest finished ones beyond [`CAP`].
    pub fn prune(&mut self, now: Timestamp) {
        self.collect(now);
        self.entries.retain(|e| !expired(e, now));
        let mut over = self.entries.len().saturating_sub(CAP);
        if over > 0 {
            self.entries.retain(|e| {
                if over > 0 && e.info.state.is_finished() {
                    over -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }

    fn entry_mut(&mut self, job: JobId) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.info.job == job)
    }

    fn ms(&self, now: Timestamp) -> u64 {
        u64::try_from(now.since(self.epoch).as_millis()).unwrap_or(u64::MAX)
    }
}

fn expired(e: &Entry, now: Timestamp) -> bool {
    e.finished_at.is_some_and(|t| now.since(t) >= KEEP)
}

/// How long [`Session::wait_job`] sleeps between two polls.
const WAIT_STEP: Duration = Duration::from_millis(2);

impl Session {
    /// The jobs listed now (`jobs.list`), oldest first.
    pub fn jobs(&self) -> Vec<JobEntry> {
        self.jobs.list(self.clock.now())
    }

    /// One job, while it is listed.
    pub fn job(&self, job: JobId) -> Option<JobEntry> {
        self.jobs.get(job, self.clock.now())
    }

    /// Cancels `job`, or every cancellable job (`jobs.cancel`); returns those cancelled. What they
    /// completed stays; the next [`Session::poll`] ends them.
    pub fn cancel_jobs(&mut self, job: Option<JobId>) -> Result<Vec<JobId>, CmdError> {
        self.jobs.cancel(job, self.clock.now())
    }

    /// Polls until `job` has finished or `timeout` has passed (`jobs.wait`), and returns its entry:
    /// the final one, or the running one when time ran out. Blocks the calling thread (the CLI,
    /// tests); the UI answers `jobs.wait` across frames with [`Session::job`] instead.
    pub fn wait_job(&mut self, job: JobId, timeout: Duration) -> Result<JobEntry, CmdError> {
        let start = Instant::now();
        loop {
            self.poll();
            let entry = self.job(job).ok_or_else(|| CmdError::NotFound(format!("no job {job}")))?;
            if entry.state.is_finished() || start.elapsed() >= timeout {
                return Ok(entry);
            }
            std::thread::sleep(WAIT_STEP);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_millis(ms)
    }

    #[test]
    fn an_entry_serialises_as_the_control_protocol_shows_it() {
        let mut jobs = Jobs::new(at(1_000));
        let (id, _) = jobs.start(JobKind::BatchEdit, "Editing", true, at(2_840));
        jobs.progress(id, JobProgress { done: 12, total: 48 }, "Editing 12 of 48");
        let v = serde_json::to_value(jobs.get(id, at(3_000)).unwrap()).unwrap();
        assert_eq!(
            v,
            json!({"job": 1, "kind": "batchEdit", "label": "Editing 12 of 48", "state": "running",
                   "progress": {"done": 12, "total": 48}, "cancellable": true, "startedMs": 1840})
        );
        jobs.finish(id, Outcome::Failed("disk full".into()), "Editing failed", at(4_000));
        let v = serde_json::to_value(jobs.get(id, at(4_000)).unwrap()).unwrap();
        assert_eq!((v["state"].clone(), v["error"].clone(), v["finishedMs"].clone()), (json!("failed"), json!("disk full"), json!(3000)));
        // The first end stays.
        jobs.finish(id, Outcome::Done(json!({})), "Edited", at(5_000));
        assert_eq!(jobs.get(id, at(5_000)).unwrap().state, JobState::Failed);
    }

    #[test]
    fn cancel_sets_the_flags_of_running_cancellable_jobs_only() {
        let mut jobs = Jobs::new(at(0));
        let (a, fa) = jobs.start(JobKind::Scan, "a", true, at(0));
        let (b, fb) = jobs.start(JobKind::Save, "b", false, at(0));
        let (c, fc) = jobs.start(JobKind::Scan, "c", true, at(0));
        jobs.finish(c, Outcome::Done(json!({})), "c", at(1));
        assert_eq!(jobs.cancel(Some(b), at(2)).unwrap(), Vec::<JobId>::new());
        assert_eq!(jobs.cancel(None, at(2)).unwrap(), vec![a]);
        assert!(fa.load(Ordering::SeqCst) && !fb.load(Ordering::SeqCst) && !fc.load(Ordering::SeqCst));
        assert!(jobs.cancel_requested(a) && !jobs.cancel_requested(b));
        assert!(matches!(jobs.cancel(Some(99), at(2)), Err(CmdError::NotFound(_))));
    }

    #[test]
    fn finished_jobs_expire_after_ten_minutes_and_running_ones_never() {
        let mut jobs = Jobs::new(at(0));
        let (a, _) = jobs.start(JobKind::Scan, "a", true, at(0));
        let (b, _) = jobs.start(JobKind::Scan, "b", true, at(0));
        jobs.finish(a, Outcome::Done(json!({})), "a", at(1_000));
        let keep = i64::try_from(KEEP.as_millis()).unwrap();
        assert_eq!(jobs.list(at(1_000 + keep - 1)).len(), 2);
        assert_eq!(jobs.list(at(1_000 + keep)).iter().map(|e| e.job).collect::<Vec<_>>(), [b]);
        assert!(jobs.get(a, at(1_000 + keep)).is_none());
        jobs.prune(at(10 * keep));
        assert_eq!(jobs.list(at(10 * keep)).len(), 1);
        assert!(jobs.any_running());
    }

    #[test]
    fn an_answered_job_ends_with_its_workers_answer_or_fails_without_one() {
        let mut jobs = Jobs::new(at(0));
        let (a, _, tx) = jobs.start_answered(JobKind::Analysis, "Picking", true, at(0));
        let (b, _, lost) = jobs.start_answered(JobKind::Analysis, "Picking", true, at(0));
        assert_eq!(jobs.answered_running(JobKind::Analysis, "Picking"), 2);
        jobs.prune(at(1));
        assert!(jobs.awaits_answer(a) && jobs.get(a, at(1)).unwrap().state == JobState::Running, "no answer yet");
        tx.send((Outcome::Done(json!({"color": "#ff0000"})), "Picked".into())).unwrap();
        drop(lost);
        jobs.prune(at(2));
        let (ea, eb) = (jobs.get(a, at(2)).unwrap(), jobs.get(b, at(2)).unwrap());
        assert_eq!((ea.state, ea.label.as_str(), ea.result), (JobState::Done, "Picked", Some(json!({"color": "#ff0000"}))));
        assert_eq!((eb.state, eb.error.as_deref()), (JobState::Failed, Some(LOST)));
        assert!(!jobs.awaits_answer(a) && jobs.answered_running(JobKind::Analysis, "Picking") == 0);
    }

    #[test]
    fn the_registry_caps_at_64_dropping_the_oldest_finished_first() {
        let mut jobs = Jobs::new(at(0));
        let (running, _) = jobs.start(JobKind::Scan, "kept", true, at(0));
        for i in 0..100 {
            let (id, _) = jobs.start(JobKind::Export, format!("{i}"), true, at(i));
            jobs.finish(id, Outcome::Done(json!(i)), "done", at(i));
        }
        let list = jobs.list(at(100));
        assert_eq!(list.len(), CAP);
        assert_eq!(list[0].job, running, "a running job is never dropped");
        assert_eq!(list.last().unwrap().label, "done");
        assert_eq!(list[1].result, Some(json!(37)), "the oldest finished went first");
    }
}
