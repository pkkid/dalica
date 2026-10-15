//! Find Duplicates (`organizing.md` §8.3): identical files only, shown as search mode
//! [`SearchMode::Duplicates`] (`search.rs`), one group per section.
//!
//! - **The plan** (on the owner thread, from the library alone): the photos grouped by their
//!   recorded size; only sizes shared by two or more photos go on. A photo whose digest record
//!   still fits its file ([`dalica_library::Library::content_digest`]: the same size and
//!   modification time) needs no read.
//! - **The job** ([`JobKind::Duplicates`], cancellable, with progress) runs on a coordinator thread
//!   ([`THREAD`]) and up to [`MAX_WORKERS`] workers. It reads every photo without a usable digest
//!   (through `fs`, at most [`MAX_FILE`] bytes) and makes its MD5 digest; each one goes back to the
//!   owner, which keeps it in the index as derived data (`Op::SetDigests`: journaled, never an
//!   undo step, no revision bump). Then the photos are bucketed by their size and digest, and
//!   every bucket is confirmed byte for byte, so a digest collision never groups different files.
//!   A bucket whose photos this session already confirmed as one group (the same photos at the
//!   same size and modification time) is not read again.
//! - **The result**: the groups, largest total first (then the larger file, then by path), each
//!   one's photos by path. When the job ends and the search did not change since it started, the
//!   light table enters search mode `duplicates` with them (the search text, filters and age
//!   cleared); the banner says "12 duplicate groups (31 files)" ([`banner`]). `search.clear`
//!   leaves it, as it leaves any search. A photo that leaves the library leaves its group, and a
//!   group left with one photo goes.
//! - **Cancel** stops the workers before their next file; the digests already made stay cached,
//!   and the light table stays as it was.
//!
//! A file that cannot be read, changed while it was read, or is larger than [`MAX_FILE`] is left
//! out and counted (`skipped`).

use std::collections::{BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use dalica_core::{PhotoId, Timestamp};
use dalica_library::{ContentDigest, Library, Op, SectionFilter};
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::search::{SearchMode, SearchResult};
use crate::session::Session;

/// The largest file Find Duplicates reads (bigger ones are left out and counted as skipped).
pub const MAX_FILE: u64 = 1 << 30;

/// The most worker threads that read and digest files.
pub const MAX_WORKERS: usize = 4;

/// The name of the coordinator thread (workers are named `dalica-duplicates-<n>`).
pub const THREAD: &str = "dalica-duplicates";

/// Digests applied per `SetDigests` op.
const DIGEST_BATCH: usize = 512;

/// Why `search.duplicates` is greyed out while it runs.
pub const RUNNING: &str = "Find Duplicates is already running.";

/// How a file's bytes become its digest: [`md5_digest`], or a test's own (to plant a collision).
pub type DigestFn = Arc<dyn Fn(&[u8]) -> String + Send + Sync>;

/// The digest Find Duplicates keeps: `md5:` and 32 lowercase hex digits (as Save's records).
pub fn md5_digest(bytes: &[u8]) -> String {
    dalica_create::save::digest(bytes)
}

/// The banner's text in duplicates mode: "12 duplicate groups (31 files)".
pub fn banner(groups: usize, files: usize) -> String {
    let g = if groups == 1 { "group" } else { "groups" };
    let f = if files == 1 { "file" } else { "files" };
    format!("{groups} duplicate {g} ({files} {f})")
}

/// What one run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DuplicatesStats {
    /// Photos that share their size with another.
    pub candidates: u64,
    /// Files read to make a digest.
    pub hashed: u64,
    /// Digests taken from the index instead.
    pub cached: u64,
    /// Files read for the byte-for-byte comparison.
    pub compared: u64,
    /// Files left out: unreadable, changed while read, or larger than [`MAX_FILE`].
    pub skipped: u64,
}

impl DuplicatesStats {
    /// Files read in all.
    pub fn reads(&self) -> u64 {
        self.hashed.saturating_add(self.compared)
    }

    fn of(items: &[Item], counters: &Counters) -> DuplicatesStats {
        DuplicatesStats {
            candidates: items.len() as u64,
            hashed: counters.hashed.load(Ordering::Relaxed),
            cached: items.iter().filter(|it| it.cached.is_some()).count() as u64,
            compared: counters.compared.load(Ordering::Relaxed),
            skipped: counters.skipped.load(Ordering::Relaxed),
        }
    }
}

/// A file as it was when read: its size and modification time.
type Version = (u64, Timestamp);

/// Find Duplicates' state in the session.
#[derive(Default)]
pub(crate) struct Duplicates {
    running: Option<Running>,
    /// The photos this session confirmed byte for byte, each with the version it was confirmed at,
    /// and the number of its group (the last finished run's groups only).
    confirmed: HashMap<(PhotoId, Version), u64>,
    /// What the last finished (or cancelled) run did.
    stats: DuplicatesStats,
    /// A test's digest instead of [`md5_digest`].
    digest: Option<DigestFn>,
    /// The library revision the groups shown were last checked against.
    checked: u64,
}

impl std::fmt::Debug for Duplicates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Duplicates").field("running", &self.running.as_ref().map(|r| r.job)).field("stats", &self.stats).finish_non_exhaustive()
    }
}

/// The job running.
struct Running {
    job: JobId,
    rx: Receiver<Msg>,
    /// The search generation when it started: a search changed since keeps the light table.
    generation: u64,
    progress: JobProgress,
}

/// One photo of the plan.
#[derive(Clone, Debug)]
struct Item {
    id: PhotoId,
    path: PathBuf,
    /// Its digest record, when it fits the file as the library knows it.
    cached: Option<ContentDigest>,
    /// The group this session confirmed it in, at the version of `cached`.
    group: Option<u64>,
}

/// What the coordinator and its workers send the owner.
enum Msg {
    /// A digest made from a file.
    Hashed(PhotoId, ContentDigest),
    /// One more file read for the comparison, or left out.
    Read,
    /// More files to read: the comparison's.
    More(u64),
    End(End),
}

enum End {
    Done(Found),
    Cancelled(DuplicatesStats),
    Failed(String),
}

/// What a finished run found.
struct Found {
    /// The groups in display order, each photo with the version it was confirmed at.
    groups: Vec<Vec<(PhotoId, Version)>>,
    stats: DuplicatesStats,
    seconds: f64,
}

/// The run's shared counters.
#[derive(Default)]
struct Counters {
    hashed: AtomicU64,
    compared: AtomicU64,
    skipped: AtomicU64,
}

fn bump(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}

/// What the coordinator works with.
struct Ctx {
    cancel: Arc<AtomicBool>,
    digest: DigestFn,
    workers: usize,
    tx: Sender<Msg>,
    counters: Counters,
}

impl Ctx {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn send(&self, msg: Msg) {
        // The owner dropped the job (the session closed): the run ends before its next file.
        if self.tx.send(msg).is_err() {
            self.cancel.store(true, Ordering::SeqCst);
        }
    }
}

/// Reads `path` as it is now: its bytes and version, or `None` when it cannot be read, is not a
/// regular file, is larger than [`MAX_FILE`], or changed between the look and the read.
fn read(path: &Path) -> Option<(Vec<u8>, Version)> {
    let stat = dalica_fs::stat(path).ok()??;
    if !stat.is_file || stat.identity.size() > MAX_FILE {
        return None;
    }
    let read = dalica_fs::read(path, MAX_FILE).ok()??;
    (read.identity == stat.identity).then(|| (read.bytes, (stat.identity.size(), Timestamp::from_system_time(stat.times.modified))))
}

/// Runs `f` over `inputs` on up to `ctx.workers` threads (on this one when none can start),
/// stopping before the next input once cancelled. `None` when cancelled.
fn parallel<T: Sync, R: Send>(ctx: &Ctx, inputs: &[T], f: impl Fn(&T) -> R + Sync) -> Option<Vec<Option<R>>> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new(inputs.iter().map(|_| None).collect());
    let work = || {
        while !ctx.cancelled() {
            let i = next.fetch_add(1, Ordering::Relaxed);
            let Some(input) = inputs.get(i) else { break };
            let r = f(input);
            if let Some(slot) = results.lock().unwrap_or_else(PoisonError::into_inner).get_mut(i) {
                *slot = Some(r);
            }
        }
    };
    std::thread::scope(|scope| {
        let mut started = 0;
        for n in 0..ctx.workers.min(inputs.len()) {
            if std::thread::Builder::new().name(format!("{THREAD}-{n}")).spawn_scoped(scope, work).is_ok() {
                started += 1;
            }
        }
        if started == 0 {
            work();
        }
    });
    if ctx.cancelled() {
        return None;
    }
    Some(results.into_inner().unwrap_or_else(PoisonError::into_inner))
}

/// The coordinator's run: digests, buckets, the byte comparison. `None` when cancelled.
fn find(items: &[Item], ctx: &Ctx) -> Option<Found> {
    let start = Instant::now();
    // Digests of the photos without one.
    let to_hash: Vec<usize> = (0..items.len()).filter(|i| items.get(*i).is_some_and(|it| it.cached.is_none())).collect();
    let made = parallel(ctx, &to_hash, |i| {
        let item = items.get(*i)?;
        let Some((bytes, (size, mtime))) = read(&item.path) else {
            bump(&ctx.counters.skipped);
            ctx.send(Msg::Read);
            return None;
        };
        let record = ContentDigest { size, mtime, digest: (ctx.digest)(&bytes) };
        bump(&ctx.counters.hashed);
        ctx.send(Msg::Hashed(item.id, record.clone()));
        Some(record)
    })?;
    let mut known: Vec<Option<ContentDigest>> = items.iter().map(|it| it.cached.clone()).collect();
    for (i, record) in to_hash.iter().zip(made) {
        if let Some(slot) = known.get_mut(*i) {
            *slot = record.flatten();
        }
    }

    // Buckets of the same size and digest, each in path order (the items are).
    let mut buckets: HashMap<(u64, &str), Vec<usize>> = HashMap::new();
    for (i, record) in known.iter().enumerate() {
        if let Some(r) = record {
            buckets.entry((r.size, r.digest.as_str())).or_default().push(i);
        }
    }
    let mut buckets: Vec<Vec<usize>> = buckets.into_values().filter(|b| b.len() >= 2).collect();
    buckets.sort();
    // A bucket this session already confirmed as one group is not read again.
    let (confirmed, to_compare): (Vec<Vec<usize>>, Vec<Vec<usize>>) = buckets.into_iter().partition(|b| {
        let groups: BTreeSet<Option<u64>> = b.iter().map(|i| items.get(*i).and_then(|it| it.group)).collect();
        groups.len() == 1 && groups.first().is_some_and(Option::is_some)
    });
    ctx.send(Msg::More(to_compare.iter().map(|b| b.len() as u64).sum()));
    let compared = parallel(ctx, &to_compare, |bucket| compare(items, &known, bucket, ctx))?;

    let mut groups: Vec<Vec<usize>> = confirmed;
    groups.extend(compared.into_iter().flatten().flatten());
    let path = |g: &[usize]| g.first().and_then(|i| items.get(*i)).map(|it| it.path.clone());
    let version = |i: &usize| known.get(*i).and_then(Option::as_ref).map(|r| (r.size, r.mtime));
    let size = |g: &[usize]| g.first().and_then(version).map_or(0, |v| v.0);
    let total = |g: &[usize]| size(g).saturating_mul(g.len() as u64);
    groups.sort_by(|a, b| total(b).cmp(&total(a)).then(size(b).cmp(&size(a))).then_with(|| path(a).cmp(&path(b))));
    let groups =
        groups.iter().map(|g| g.iter().filter_map(|i| Some((items.get(*i)?.id, version(i)?))).collect::<Vec<_>>()).filter(|g| g.len() >= 2).collect();
    Some(Found { groups, stats: DuplicatesStats::of(items, &ctx.counters), seconds: start.elapsed().as_secs_f64() })
}

/// The byte-for-byte comparison of one bucket (in path order): its photos split into groups of
/// identical files (a digest collision makes more than one), those of two or more kept, each in
/// path order. The first group's bytes stay in memory; another group's first file is read again
/// to compare (only after a collision).
fn compare(items: &[Item], known: &[Option<ContentDigest>], bucket: &[usize], ctx: &Ctx) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut first: Option<Vec<u8>> = None;
    for &i in bucket {
        if ctx.cancelled() {
            return Vec::new();
        }
        let (Some(item), Some(Some(record))) = (items.get(i), known.get(i)) else { continue };
        // The file must still be the version its digest was made from.
        let bytes = match read(&item.path) {
            Some((bytes, (size, mtime))) if size == record.size && mtime == record.mtime => bytes,
            _ => {
                bump(&ctx.counters.skipped);
                ctx.send(Msg::Read);
                continue;
            }
        };
        bump(&ctx.counters.compared);
        ctx.send(Msg::Read);
        let mut placed = false;
        for (k, group) in groups.iter_mut().enumerate() {
            let same = if k == 0 {
                first.as_deref() == Some(bytes.as_slice())
            } else {
                bump(&ctx.counters.compared);
                group.first().and_then(|j| items.get(*j)).and_then(|rep| read(&rep.path)).is_some_and(|(b, _)| b == bytes)
            };
            if same {
                group.push(i);
                placed = true;
                break;
            }
        }
        if !placed {
            if groups.is_empty() {
                first = Some(bytes);
            }
            groups.push(vec![i]);
        }
    }
    groups.retain(|g| g.len() >= 2);
    groups
}

/// The coordinator thread's body: the run, its end sent last.
fn coordinate(items: Vec<Item>, ctx: Ctx) {
    let end = match catch_unwind(AssertUnwindSafe(|| find(&items, &ctx))) {
        Ok(Some(found)) => End::Done(found),
        Ok(None) => End::Cancelled(DuplicatesStats::of(&items, &ctx.counters)),
        Err(_) => End::Failed("the duplicates worker stopped unexpectedly".into()),
    };
    let _ = ctx.tx.send(Msg::End(end));
}

/// The search result that shows `groups` (photos no longer in `library` left out, and groups left
/// with one photo dropped).
fn result_of(library: &Library, groups: Vec<Vec<PhotoId>>, seconds: f64) -> SearchResult {
    let groups: Vec<Vec<PhotoId>> =
        groups.into_iter().map(|g| g.into_iter().filter(|id| library.photo(*id).is_some()).collect::<Vec<_>>()).filter(|g| g.len() >= 2).collect();
    let photos = groups.iter().flatten().filter_map(|id| library.photo(*id));
    let folders: BTreeSet<_> = photos.clone().map(|p| p.folder).collect();
    let bytes = photos.map(|p| p.size).fold(0u64, u64::saturating_add);
    SearchResult {
        filter: Arc::new(SectionFilter::new(groups.iter().flatten().copied())),
        groups,
        folders: folders.len(),
        bytes,
        seconds,
        thread: THREAD.into(),
        ..SearchResult::default()
    }
}

fn progress_label(p: JobProgress) -> String {
    format!("Finding duplicates: {} of {} files", p.done, p.total)
}

impl Session {
    /// Starts Find Duplicates (`search.duplicates`) and returns its job. `Disabled` while one runs.
    pub(crate) fn start_duplicates(&mut self) -> Result<JobId, CmdError> {
        if self.duplicates.running.is_some() {
            return Err(CmdError::Disabled(RUNNING.into()));
        }
        let items = self.duplicate_plan();
        let now = self.clock.now();
        let (job, cancel) = self.jobs.start(JobKind::Duplicates, "Finding duplicates", true, now);
        let progress = JobProgress { done: 0, total: items.iter().filter(|it| it.cached.is_none()).count() as u64 };
        self.jobs.progress(job, progress, progress_label(progress));
        let (tx, rx) = mpsc::channel();
        let digest = self.duplicates.digest.clone().unwrap_or_else(|| Arc::new(md5_digest));
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, MAX_WORKERS);
        let ctx = Ctx { cancel, digest, workers, tx, counters: Counters::default() };
        if let Err(e) = std::thread::Builder::new().name(THREAD.into()).spawn(move || coordinate(items, ctx)) {
            let why = format!("cannot start Find Duplicates: {e}");
            self.jobs.finish(job, Outcome::Failed(why.clone()), "Could not find duplicates", now);
            return Err(CmdError::Internal(why));
        }
        self.duplicates.running = Some(Running { job, rx, generation: self.search.generation, progress });
        Ok(job)
    }

    /// The plan: every photo that shares its recorded size with another, in path order, with its
    /// digest record when that still fits the file, and the group this session confirmed it in.
    fn duplicate_plan(&self) -> Vec<Item> {
        let lib = &self.library;
        let mut by_size: HashMap<u64, Vec<PhotoId>> = HashMap::new();
        for p in lib.photos() {
            by_size.entry(p.size).or_default().push(p.id);
        }
        let mut items: Vec<Item> = by_size
            .into_values()
            .filter(|ids| ids.len() >= 2)
            .flatten()
            .filter_map(|id| {
                let p = lib.photo(id)?;
                let path = lib.folder(p.folder)?.path.join(&p.name);
                let cached = lib.content_digest(id).and(lib.digest_record(id)).cloned();
                let group = cached.as_ref().and_then(|d| self.duplicates.confirmed.get(&(id, (d.size, d.mtime)))).copied();
                Some(Item { id, path, cached, group })
            })
            .collect();
        items.sort_by(|a, b| a.path.cmp(&b.path));
        items
    }

    /// The Find Duplicates job running, if one is.
    pub fn duplicates_job(&self) -> Option<JobId> {
        self.duplicates.running.as_ref().map(|r| r.job)
    }

    /// What the last finished or cancelled Find Duplicates run did.
    pub fn duplicates_stats(&self) -> DuplicatesStats {
        self.duplicates.stats
    }

    /// Test hook: digests made with `digest` instead of MD5 (`None`: MD5 again), so a test can
    /// plant a collision or hold a worker inside a file. Applies to the runs started after it.
    #[doc(hidden)]
    pub fn set_duplicates_digest(&mut self, digest: Option<DigestFn>) {
        self.duplicates.digest = digest;
    }

    /// `library.inspect` → `search` in duplicates mode: the groups (`[{count, size, bytes, ids}]`,
    /// largest total first) and the banner's text; both `null` otherwise.
    pub(crate) fn duplicates_inspect(&self) -> (Value, Value) {
        let Some(r) = self.search.result.as_ref().filter(|_| self.search.mode == Some(SearchMode::Duplicates)) else {
            return (Value::Null, Value::Null);
        };
        let groups: Vec<Value> = r
            .groups
            .iter()
            .map(|g| {
                let size = g.first().and_then(|id| self.library.photo(*id)).map_or(0, |p| p.size);
                json!({ "count": g.len(), "size": size, "bytes": size.saturating_mul(g.len() as u64), "ids": g })
            })
            .collect();
        (Value::Array(groups), json!(banner(r.groups.len(), r.count())))
    }

    /// Applies what the Find Duplicates job sent (from `poll`): its digests into the index, its
    /// progress, and its end; and keeps the groups shown in step with the library. Returns how
    /// many things it handled. Never blocks and never touches the disk.
    pub(crate) fn poll_duplicates(&mut self) -> usize {
        let mut handled = self.prune_duplicates();
        let Some(running) = &mut self.duplicates.running else { return handled };
        let mut digests = Vec::new();
        let mut end = None;
        loop {
            match running.rx.try_recv() {
                Ok(Msg::Hashed(id, record)) => {
                    digests.push((id, Some(record)));
                    running.progress.done = running.progress.done.saturating_add(1);
                }
                Ok(Msg::Read) => running.progress.done = running.progress.done.saturating_add(1),
                Ok(Msg::More(n)) => running.progress.total = running.progress.total.saturating_add(n),
                Ok(Msg::End(e)) => {
                    end = Some(e);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    end = Some(End::Failed("the duplicates worker stopped".into()));
                    break;
                }
            }
            handled += 1;
        }
        let (job, progress) = (running.job, running.progress);
        self.jobs.progress(job, progress, progress_label(progress));
        self.keep_digests(digests);
        if let Some(end) = end {
            handled += 1;
            self.end_duplicates(end);
        }
        handled
    }

    /// Keeps the digests made in the index (derived data); those of photos gone, or unchanged,
    /// are left out.
    fn keep_digests(&mut self, mut digests: Vec<(PhotoId, Option<ContentDigest>)>) {
        digests.retain(|(id, d)| self.library.photo(*id).is_some() && self.library.digest_record(*id) != d.as_ref());
        for batch in digests.chunks(DIGEST_BATCH) {
            if let Err(e) = self.apply_derived(Op::SetDigests { digests: batch.to_vec() }) {
                log::error!("digests not kept: {e}");
            }
        }
    }

    fn end_duplicates(&mut self, end: End) {
        let Some(running) = self.duplicates.running.take() else { return };
        let now = self.clock.now();
        match end {
            End::Done(found) => {
                self.duplicates.stats = found.stats;
                self.duplicates.confirmed =
                    found.groups.iter().zip(1u64..).flat_map(|(g, n)| g.iter().map(move |(id, version)| ((*id, *version), n))).collect();
                let groups: Vec<Vec<PhotoId>> = found.groups.iter().map(|g| g.iter().map(|(id, _)| *id).collect()).collect();
                let result = result_of(&self.library, groups, found.seconds);
                let s = found.stats;
                let summary = json!({
                    "groups": result.groups.len(), "files": result.count(), "bytes": result.bytes,
                    "candidates": s.candidates, "hashed": s.hashed, "cached": s.cached, "compared": s.compared,
                    "skipped": s.skipped, "seconds": found.seconds,
                });
                let label = banner(result.groups.len(), result.count());
                self.jobs.finish(running.job, Outcome::Done(summary), label, now);
                // A search started or cleared since keeps the light table as it is.
                if self.search.generation == running.generation {
                    self.search.enter_duplicates(result);
                    self.duplicates.checked = self.library.revision();
                }
            }
            End::Cancelled(stats) => {
                self.duplicates.stats = stats;
                let summary = json!({ "hashed": stats.hashed, "compared": stats.compared, "skipped": stats.skipped });
                self.jobs.finish(running.job, Outcome::Cancelled(summary), "Find Duplicates cancelled", now);
            }
            End::Failed(why) => {
                log::error!("Find Duplicates failed: {why}");
                self.jobs.finish(running.job, Outcome::Failed(why), "Could not find duplicates", now);
            }
        }
    }

    /// In duplicates mode, after the library changed: photos gone leave their groups. Returns 1
    /// when the groups changed.
    fn prune_duplicates(&mut self) -> usize {
        let revision = self.library.revision();
        if self.search.mode != Some(SearchMode::Duplicates) || self.duplicates.checked == revision {
            return 0;
        }
        self.duplicates.checked = revision;
        let Some(r) = &self.search.result else { return 0 };
        if r.groups.iter().flatten().all(|id| self.library.photo(*id).is_some()) {
            return 0;
        }
        let mut pruned = result_of(&self.library, r.groups.clone(), r.seconds);
        pruned.generation = r.generation;
        self.search.result = Some(pruned);
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_banner_counts_groups_and_files() {
        assert_eq!(banner(12, 31), "12 duplicate groups (31 files)");
        assert_eq!(banner(1, 2), "1 duplicate group (2 files)");
        assert_eq!(banner(0, 0), "0 duplicate groups (0 files)");
    }

    #[test]
    fn photos_that_leave_the_library_leave_their_groups() {
        let (_dirs, mut s, ids) = crate::cmd::testing::library(3);
        let result = result_of(&s.library, vec![ids.clone()], 0.0);
        s.search.enter_duplicates(result);
        s.duplicates.checked = s.library.revision();
        assert_eq!(s.duplicates_inspect().1, json!("1 duplicate group (3 files)"));
        s.apply(Op::RemovePhotos { ids: vec![ids[0]] }).unwrap();
        s.poll();
        let (groups, banner) = s.duplicates_inspect();
        assert_eq!((groups[0]["ids"].clone(), banner), (json!([ids[1], ids[2]]), json!("1 duplicate group (2 files)")));
        s.apply(Op::RemovePhotos { ids: vec![ids[1]] }).unwrap();
        s.poll();
        assert_eq!(s.duplicates_inspect(), (json!([]), json!("0 duplicate groups (0 files)")));
        assert_eq!(s.search_state().mode, Some(SearchMode::Duplicates), "an empty result stays on show");
    }

    #[test]
    fn an_unchanged_query_keeps_the_groups_and_a_search_leaves_them() {
        let (_dirs, mut s, ids) = crate::cmd::testing::library(2);
        s.search.enter_duplicates(result_of(&s.library, vec![ids.clone()], 0.0));
        let v = s.execute("search.set", &json!({ "text": "", "filters": { "starred": false } })).unwrap();
        assert_eq!((v["changed"].clone(), v["mode"].clone(), v["matches"].clone()), (json!(false), json!("duplicates"), json!(2)));
        let v = s.execute("search.set", &json!({ "text": "img" })).unwrap();
        assert_eq!((v["mode"].clone(), v["matches"].clone(), v["groups"].clone()), (json!("search"), json!(null), json!(null)));
        s.search.enter_duplicates(result_of(&s.library, vec![ids], 0.0));
        let v = s.execute("search.clear", &json!({})).unwrap();
        assert_eq!((v["active"].clone(), v["groups"].clone(), v["banner"].clone()), (json!(false), json!(null), json!(null)));
    }

    #[test]
    fn the_digest_is_md5_as_save_records_it() {
        assert_eq!(md5_digest(b""), "md5:d41d8cd98f00b204e9800998ecf8427e");
    }
}
