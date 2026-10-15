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
//!   sidecars like any change, never on the undo stack. A folder at a path the model does not know
//!   that shares photos (by size and modification time) with a known folder the walk has not
//!   reached yet is held back until the walk ends; then it is diffed against the folders gone
//!   (`dalica_scan::diff_found`): one holding most of a gone folder's photos is that folder renamed
//!   or moved on disk (between sessions, `data-model.md` §10.2) and keeps its ids and everything
//!   said about it, as the watcher's rescans keep them. Every other folder lands as it is read.
//!   **Across regions** (the verification pass scans each unit on its own): a merge scan whose walk
//!   is over offers the folders holding photos it found gone to the other merge scans, and keeps
//!   them in the library while one runs; a folder at an unknown path sharing photos with a known
//!   folder where another merge scan still walks is held until every merge walk is over; folders
//!   at unknown paths pair with the folders gone here and those offered. The offered folders no
//!   scan paired leave the library once no merge scan runs (a scan cancelled or put off withdraws
//!   its own).
//! - **Rebuild** scans (`library.rebuild`, and a damaged index at open) build a fresh model to the
//!   side and swap it in at the end, clearing the undo stack (ids change); a cancelled rebuild
//!   keeps the old model. A rebuild of an empty model fills it in place, like a merge. Its thread
//!   first waits for the I/O worker to write everything sent before it (sidecars, `albums.json`),
//!   so it reads what the model said when it started, never a file a pending write would change.
//! - **Put off** ([`Session::put_off_scans`], M9.28): a file job starting lets every running scan
//!   go (its job ends as "put off while files are written"; what it read and did not apply is
//!   dropped, nothing is removed, a rebuild keeps the old model) and starts it again from the start
//!   once no file job runs, so no scan applies a listing older than the job's writes.
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
//!   it out from then on. Options → File Types (`settings.json` → `fileTypes`) go to every scan;
//!   changing them restarts the running scans and scans every unit once
//!   ([`Session::rescan_for_file_types`], M10.5).
//!
//! - **Upkeep while scanning** (M5.17, `security.md` §5.8, `data-model.md` §7): each folder handed
//!   out has its stale `.dalica-tmp-*` files removed on the scan's thread
//!   ([`crate::watching::clean_temps`], through `dalica_fs::remove_stale_temps` at the session
//!   time the scan started). A walked root that turns out to hold no entries at all (the mount
//!   point an unmounted volume leaves behind) removes nothing: its folders stay, as under a root
//!   that cannot be walked, with a warning when the library holds folders there. Every walked unit
//!   counts as verified for the schedule in [`crate::watching`].
//!
//! - **What the files say** (M6.32, `xmp-interop.md` §5, §7): every scan and rescan reads the
//!   owned metadata (caption, tags, rating, place, date) of the photos the model does not know at
//!   their size and modification time, on its own thread ([`dalica_scan::ReadMetadata`]; a rebuild
//!   reads every photo). The diff imports it: a photo Dalica has nothing for takes every value its
//!   file has with no undo step; a value changed in another application while Dalica's did not
//!   comes in through the diff's sync op, whose imports are one undoable step ("Import from
//!   Files"); a field changed on both sides keeps Dalica's value and is noted for Properties
//!   ([`Session::note_changed_elsewhere`]); the synced hashes become those of what was read.
//!   [`Session::apply`] calls [`Session::files_op_before`] / [`Session::files_op_after`] for every
//!   op applied outside a command, so the watcher's rescans get the same. All of it is tagged
//!   [`crate::metadata::Origin::Files`]: never written back. A folder's sync op is diffed again
//!   when the model changed since its diff, so an edit made meanwhile is never overwritten.
//! - **Search facts and first-indexed times** (M9.7, `organizing.md` §8, `roadmap.md` M9):
//!   the same read gives each new or changed photo's search facts (camera make and model, video,
//!   GPS location: `dalica_scan::facts`), which ride in the diff's sync op as `SetFacts` (derived:
//!   no undo step, no revision of their own). Each op from the files that adds photos (a scan's or
//!   the watcher's, applied through [`Session::apply`]) is followed by the derived op recording
//!   when this library first indexed them ([`Library::first_indexed_op`], the session's clock),
//!   journaled with no undo step; a rebuild records none (after a rebuild only the file's time
//!   counts for Recently Updated).
//!
//! - **Albums and photo sorts** (M9.10, `data-model.md` §3, §3.2, §4): what a folder's sidecar
//!   says about albums rides in the op that adds the folder or its photos (`with_albums`): a new
//!   folder's sort, a new photo's memberships, an album the model lacks created first (with its
//!   `albums.json` definition, else the folder's name for it; never one deleted this session); the
//!   watcher's rescans the same ([`Session::rescan_op_with_albums`], from `watching.rs`). A
//!   rebuild's thread waits for the writes sent before it, reads `albums.json`, and the rebuild
//!   ends by creating the albums no folder brought back with their definitions and covers
//!   (`album_definitions`). A folder renamed on disk gets back everything the model said about
//!   it and its photos ([`Session::files_op_after`]). `albums.json` waits while scans apply what
//!   they found and while a rebuild runs, and is written once they are done (`albums_file.rs`).
//!
//! Seams: [`crate::watching`] starts the periodic verification through
//! [`Session::start_verification`]'s units and runs its own incremental rescans of dirty folders
//! ([`Session::busy`] and [`Session::wait_idle`] include them); M5.16 replaces the amortised
//! whole-library thumbnail sync below with one that queues just the photos of each batch, the
//! first screenful first.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use dalica_core::{AlbumId, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::{AlbumInfo, Library, Op};
use dalica_scan::{Event, Failure, FoundFolder, HeaderReader, Known, Plan, Progress, ReadMetadata, Scan, Summary};
use dalica_store::albums::{AlbumsRead, AlbumsReading};
use serde::Serialize;
use serde_json::json;

use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::metadata::{Field, Fields};
use crate::session::{Msg, Session};
use crate::undo::Step;

/// The undo label of what a scan imported from the files (a value changed in another application).
pub const IMPORT_LABEL: &str = "Import from Files";

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
    /// A rebuild's thread first: what `albums.json` says (`albums_file.rs`).
    Albums(AlbumsRead),
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
    /// The files the model knows, at the library revision they were listed at: scans read the
    /// metadata of the others.
    known: Option<(u64, Arc<Known>)>,
    /// Scans a file job put off ([`Session::put_off_scans`]): started again, in order, once no
    /// file job runs.
    put_off: Vec<(Plan, ScanKind)>,
    /// A rebuild's op is being applied to the live model (a rebuild of an empty model fills it in
    /// place): the photos it adds get no first-indexed time.
    rebuilding: bool,
    /// Scans are applying what they found ([`Session::poll_scans`]): `albums.json` waits.
    applying: bool,
    /// `albums.json` (`albums_file.rs`): kept here because the scans decide when it may be written.
    pub(crate) albums_file: crate::albums_file::AlbumsFile,
    /// The merge scans whose walk still runs, with what they walk: a folder another merge scan
    /// finds at a path the model does not know may be one of theirs, moved there on disk.
    walking: Vec<(JobId, Plan)>,
    /// Folders holding photos that a merge scan, its walk ended, found gone where it looked, with
    /// the path they had: another merge scan pairs a folder moved into its own region with them
    /// (`dalica_scan::diff_found`), and while one runs they stay in the library; once none runs,
    /// those still at that path leave it.
    gone: Vec<(JobId, FolderId, PathBuf)>,
}

impl Scanning {
    /// Whether scans are applying what they found right now.
    pub(crate) fn applying(&self) -> bool {
        self.applying
    }

    /// Whether a rebuild runs or waits to run again after a file job.
    pub(crate) fn rebuild_pending(&self) -> bool {
        self.running.iter().any(|j| j.kind == ScanKind::Rebuild) || self.put_off.iter().any(|(_, k)| *k == ScanKind::Rebuild)
    }
}

/// What [`Session::files_op_before`] learns of an op from the files, for
/// [`Session::files_op_after`].
pub(crate) struct FromFiles {
    /// For a diff's sync op: the fields of each photo changed both in another application and in
    /// Dalica since the last sync, judged on the model before the op.
    conflicts: Vec<(PhotoId, Fields)>,
    /// The photos the op adds: their first-indexed time is recorded once it applied.
    added: Vec<PhotoId>,
    /// The folders the op takes out and puts back at a new path (renamed or moved on disk): what
    /// the model said about them and their photos is put back once it applied.
    moved: Vec<FolderId>,
}

/// The folders `op` takes out and puts back in the same batch: a folder renamed or moved on disk
/// (`dalica_scan::diff`), which keeps its id and its photos' ids.
fn moved_folders(op: &Op) -> Vec<FolderId> {
    let Op::Batch { ops } = op else { return Vec::new() };
    let removed: Vec<FolderId> = ops.iter().filter_map(|o| if let Op::RemoveFolder { id } = o { Some(*id) } else { None }).collect();
    ops.iter().filter_map(|o| if let Op::AddFolder { folder } = o { Some(folder.id) } else { None }).filter(|id| removed.contains(id)).collect()
}

/// What `inverse` (the inverse of an op that took folders `moved` out and put them back) holds
/// about those folders and their photos besides the folders and photos themselves: what the user
/// said about them (info, stars aside, recipes, dates, memberships, covers, sort, project, copies
/// saved beside), and the index-only data (facts, digests, times, synced hashes, saved records,
/// sketches). One batch to put it all back.
fn put_back(inverse: &Op, moved: &[FolderId]) -> Option<Op> {
    let mut stack = vec![inverse];
    let mut out = Vec::new();
    while let Some(op) = stack.pop() {
        let Op::Batch { ops } = op else { continue };
        match ops.first() {
            // The inverse of taking a folder out: the folder, what was said about it, its photos and
            // what was said about them.
            Some(Op::AddFolder { folder }) if moved.contains(&folder.id) => {
                out.extend(ops.iter().skip(1).filter(|o| !matches!(o, Op::AddFolder { .. } | Op::AddPhotos { .. })).cloned());
            }
            _ => stack.extend(ops),
        }
    }
    (!out.is_empty()).then_some(Op::Batch { ops: out })
}

/// The photos `op` adds (in nested batches too; walked without recursion).
fn added_photos(op: &Op) -> Vec<PhotoId> {
    let mut out = Vec::new();
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::AddPhotos { photos } => out.extend(photos.iter().map(|p| p.id)),
            Op::Batch { ops } => stack.extend(ops),
            _ => {}
        }
    }
    out
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
    /// The job's cancel flag, which the walk checks before each file.
    cancel: Arc<AtomicBool>,
    synced: bool,
    /// The library revision after the current folder's diff or the last op of ours: another value
    /// means the model changed meanwhile, and the folder's sync op is diffed again.
    revision: u64,
    /// A rebuild's reading of `albums.json` (`None` for a merge scan, or until it arrives): the
    /// definitions of the albums it brings back, their covers, and the order for folders whose
    /// sidecar cannot be read.
    albums: Option<AlbumsReading>,
    /// A merge scan's folders at paths the model does not know that share photos (by size and
    /// modification time) with a known folder the walk has not found yet: maybe that folder moved
    /// on disk, so they wait until the walk ends and the folders gone are known.
    held: Vec<FoundFolder>,
    /// The photos of the known folders where the scan looks, or another merge scan still walks,
    /// by (size, modification time): made for the first folder at an unknown path.
    known_keys: Option<HashMap<(u64, Timestamp), Vec<FolderId>>>,
    /// The end of the walk was taken into account ([`Session::note_walk_end`]).
    walk_noted: bool,
}

impl ScanJob {
    fn new(job: JobId, kind: ScanKind, plan: Plan, rx: Receiver<Streamed>, fresh: Option<Library>, cancel: Arc<AtomicBool>) -> ScanJob {
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
            cancel,
            synced: false,
            revision: 0,
            albums: None,
            held: Vec::new(),
            known_keys: None,
            walk_noted: false,
        }
    }

    /// Takes in what the walk sent: everything waiting, or (with `block`) everything until it ends.
    fn receive(&mut self, block: bool, warnings: &mut Vec<String>) {
        let Some(rx) = self.rx.take() else { return };
        loop {
            let message = if block { rx.recv().map_err(|_| TryRecvError::Disconnected) } else { rx.try_recv() };
            match message {
                Ok(Streamed::Albums(read)) => self.albums_read(read, warnings),
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

    /// A rebuild's reading of `albums.json`: kept for the folders to come; what it could not give
    /// is a warning.
    fn albums_read(&mut self, read: AlbumsRead, warnings: &mut Vec<String>) {
        match read {
            AlbumsRead::Absent => {}
            AlbumsRead::Read(r) => {
                warnings.extend(r.notes.iter().cloned());
                self.albums = Some(r);
            }
            AlbumsRead::Unreadable(why) => warnings.push(format!("{}: {why}", dalica_store::albums::FILE)),
            AlbumsRead::SetAside { why, kept } => {
                let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "it could not be kept aside".into());
                warnings.push(format!("{}: {why}; {kept}", dalica_store::albums::FILE));
            }
        }
    }

    /// Cancelled: let the walk go (it stops at its next file) and drop what is not applied yet,
    /// except the rest of the folder being applied.
    fn stop(&mut self) {
        self.stopped = true;
        self.rx = None;
        self.queue.clear();
        self.held.clear();
    }

    /// Everything taken in is applied and nothing more comes.
    fn is_complete(&self) -> bool {
        (self.stopped || self.summary.is_some() || self.lost)
            && self.queue.is_empty()
            && self.held.is_empty()
            && self.pending.is_empty()
            && self.current.is_none()
    }

    /// Whether `folder` (at a path `lib` does not know) shares a photo with a known folder where
    /// the scan looks that the walk has not found yet, or one where another merge scan still walks
    /// (`walking`): then it may be that folder, moved on disk.
    fn may_be_moved(&mut self, lib: &Library, folder: &FoundFolder, walking: &[(JobId, Plan)]) -> bool {
        let (job, plan) = (self.job, &self.plan);
        let keys = self.known_keys.get_or_insert_with(|| {
            let mut out: HashMap<(u64, Timestamp), Vec<FolderId>> = HashMap::new();
            let covers =
                |plan: &Plan, path: &Path| plan.roots.iter().any(|r| path.starts_with(r)) && !plan.excluded.iter().any(|x| path.starts_with(x));
            let looked: HashSet<FolderId> = lib
                .folders()
                .filter(|f| covers(plan, &f.path) || walking.iter().any(|(id, other)| *id != job && covers(other, &f.path)))
                .map(|f| f.id)
                .collect();
            for p in lib.photos().filter(|p| looked.contains(&p.folder)) {
                let ids = out.entry((p.size, p.mtime)).or_default();
                if !ids.contains(&p.folder) {
                    ids.push(p.folder);
                }
            }
            out
        });
        let seen = &self.seen;
        folder
            .photos
            .iter()
            .any(|p| keys.get(&(p.size, p.mtime)).is_some_and(|ids| ids.iter().any(|id| lib.folder(*id).is_some_and(|f| !seen.contains(&f.path)))))
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
/// A batch (photos added with what their sidecar says about them) costs what its photo ops cost.
pub(crate) fn cost(op: &Op) -> usize {
    match op {
        Op::AddPhotos { photos } | Op::UpdatePhotos { photos } => photos.len().max(1),
        Op::RemovePhotos { ids } => ids.len().max(1),
        Op::Batch { ops } => ops
            .iter()
            .map(|op| match op {
                Op::AddPhotos { photos } | Op::UpdatePhotos { photos } => photos.len(),
                Op::RemovePhotos { ids } => ids.len(),
                _ => 0,
            })
            .fold(0, usize::saturating_add)
            .max(1),
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

/// The engine's name of a library sync field.
fn engine_field(f: dalica_library::sync::Field) -> Field {
    use dalica_library::sync::Field as F;
    match f {
        F::Caption => Field::Caption,
        F::Tags => Field::Tags,
        F::Rating => Field::Star,
        F::Place => Field::Place,
        F::Date => Field::Date,
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
        options.cancel = Some(Arc::clone(&cancel));
        options.excluded = plan.excluded.clone();
        if kind == ScanKind::Rebuild {
            // The rebuilt model knows nothing: every photo's metadata is read.
            options.metadata = ReadMetadata::all();
        }
        let (tx, rx) = mpsc::channel();
        let walk = plan.roots.clone();
        // A rebuild reads the sidecars and `albums.json` as the writes already made say them: its
        // thread waits until the I/O worker has written everything sent before it started.
        let flushed = (kind == ScanKind::Rebuild).then(|| {
            let (ack, done) = mpsc::channel();
            self.send(Msg::Flush(ack));
            (self.dir.clone(), done)
        });
        let spawned = std::thread::Builder::new().name("dalica-scan".into()).spawn(move || {
            // A rebuild brings back the albums' definitions and covers from `albums.json`.
            if let Some((dir, done)) = flushed {
                let _ = done.recv();
                if tx.send(Streamed::Albums(dalica_store::albums::read(&dir, now))).is_err() {
                    return;
                }
            }
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
                if kind == ScanKind::Merge {
                    self.scanning.walking.push((job, plan.clone()));
                }
                let scan = ScanJob::new(job, kind, plan, rx, fresh, cancel);
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

    /// The options the next scan gets (owned: they move to its thread): the failures in effect,
    /// Options → File Types, and the files the model knows (the metadata of the others is read).
    /// Built on the owner thread without touching the disk.
    pub(crate) fn scan_options(&mut self) -> dalica_scan::Options {
        let metadata = ReadMetadata::NewOrChanged(self.known_files());
        let mut options = dalica_scan::Options {
            failures: self.scanning.failures.clone(),
            types: self.settings().file_types(),
            metadata,
            ..dalica_scan::Options::default()
        };
        if let Some(reader) = &self.scanning.reader {
            options.reader = Arc::clone(reader);
            // A planted reader reads every header; the metadata is then read from the files.
            options.bytes_reader = None;
        }
        options
    }

    /// The files the model knows now: listed once per library revision (one pass over the photos).
    fn known_files(&mut self) -> Arc<Known> {
        let revision = self.library.revision();
        match &self.scanning.known {
            Some((r, known)) if *r == revision => Arc::clone(known),
            _ => {
                let known = Arc::new(Known::of(&self.library));
                self.scanning.known = Some((revision, Arc::clone(&known)));
                known
            }
        }
    }

    /// Before an op from the files (a scan's or the watcher's, applied outside a command): for a
    /// diff's sync op, the fields of each photo that changed both in another application and in
    /// Dalica since the last sync, judged on the model before the op; for an op that adds photos,
    /// which (none while a rebuild applies). `None` for any other op.
    pub(crate) fn files_op_before(&self, op: &Op) -> Option<FromFiles> {
        let added = if self.scanning.rebuilding { Vec::new() } else { added_photos(op) };
        let moved = moved_folders(op);
        let Some(parts) = dalica_scan::meta::sync_parts(op) else {
            return (!added.is_empty() || !moved.is_empty()).then_some(FromFiles { conflicts: Vec::new(), added, moved });
        };
        let mut conflicts = Vec::new();
        for (id, file) in parts.synced {
            let Some(file) = file else { continue };
            let fields = dalica_scan::meta::conflicts(&self.library, *id, file);
            if !fields.is_empty() {
                conflicts.push((*id, Fields::of(&fields.into_iter().map(engine_field).collect::<Vec<_>>())));
            }
        }
        Some(FromFiles { conflicts, added, moved })
    }

    /// After an op from the files was applied (`inverse` undoes it): a folder it took out and put
    /// back at a new path (renamed or moved on disk) gets back everything the model said about it
    /// and its photos (`data-model.md` §10.2: captions, tags, recipes, memberships, covers, its
    /// sort …, which taking it out dropped), before any sidecar write could miss them; a sync op's
    /// imports become one undo step ("Import from Files"; the synced hashes stay those of the
    /// files), and the conflicts found before it are noted for Properties ("Changed in another
    /// application; Dalica's value kept"); the photos it added get their first-indexed time
    /// (derived: journaled, no undo step), unless they had one.
    pub(crate) fn files_op_after(&mut self, from: FromFiles, inverse: &Op) {
        // A folder renamed or moved on disk keeps everything said about it and its photos (and
        // their first-indexed times, so they do not show as new).
        if let Some(op) = put_back(inverse, &from.moved)
            && let Err(e) = self.apply(op)
        {
            self.scan_warnings.push(format!("what was said about a moved folder not put back: {e}"));
        }
        if let Some(op) = dalica_scan::meta::undo_of_import(inverse) {
            self.undo.push(Step { label: IMPORT_LABEL, op, modes: None });
        }
        for (id, fields) in from.conflicts {
            self.note_changed_elsewhere(id, fields);
        }
        if !from.added.is_empty() {
            let op = self.library.first_indexed_op(&from.added, self.clock.now());
            if !matches!(&op, Op::SetFirstIndexed { indexed } if indexed.is_empty())
                && let Err(e) = self.apply_derived(op)
            {
                self.scan_warnings.push(format!("first-indexed times not applied: {e}"));
            }
        }
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

    /// Whether a scan is still running or waits to run again after a file job (the watcher's
    /// rescans of dirty folders included). Thumbnail jobs are counted apart
    /// ([`Session::thumbnails_pending`]).
    pub fn busy(&self) -> bool {
        !self.scanning.running.is_empty() || !self.scanning.put_off.is_empty() || self.rescanning()
    }

    /// A file job is starting: every running scan is let go with what it read and did not apply
    /// yet, its job ends as put off, and it starts again from the start once no file job runs
    /// ([`Session::resume_put_off_scans`]), as the watcher's rescans are put off
    /// ([`Session::put_off_rescan`]).
    ///
    /// A scan overlapping a file job lists some files before the job writes them and others
    /// after, while the job's results reach the model only when the owner applies them: a folder
    /// listed before a Save and applied after it would put the saved photo's old size and time
    /// back (the next Undo Save refused as changed on disk) and drop the record the Save just
    /// wrote. Walked again after the job, the folders hold what its results put in the model, so
    /// the scan finds nothing to change. What a scan already applied stays (each folder's diff is
    /// idempotent); nothing is removed for a scan put off, and a rebuild keeps the old model.
    pub(crate) fn put_off_scans(&mut self) {
        for again in self.let_scans_go("put off while files are written", "putOff") {
            if !self.scanning.put_off.contains(&again) {
                self.scanning.put_off.push(again);
            }
        }
    }

    /// Options → File Types or the small-picture threshold changed (`options.set`, M10.5): one
    /// scan of every unit ([`dalica_scan::Rules::everything`], Scan Once units already indexed
    /// too, so a type turned back on comes back everywhere) merges the folders as the new types
    /// list them: photos of a type turned off leave the library, while their sidecar entries stay
    /// on disk, never marked missing as long as their files exist (`dalica-store`'s `stale` asks
    /// the disk, not the listing), so turning the type back on brings back everything said about
    /// them. The scans running list files with the old types: they are let go first (their jobs
    /// end as restarted; what they applied stays, nothing is removed for them), and a rebuild
    /// among them (or put off by a file job) starts again instead of the merge, since it walks
    /// every unit too. While a file job runs, the scan waits with the scans it put off. Returns
    /// the job started, `None` when no folder is watched or a file job runs.
    pub(crate) fn rescan_for_file_types(&mut self) -> Option<JobId> {
        let plan = self.folders.rules.everything();
        if plan.roots.is_empty() {
            return None;
        }
        let stopped = self.let_scans_go("started again for the new file types", "restarted");
        let rebuild = stopped.iter().chain(&self.scanning.put_off).any(|(_, k)| *k == ScanKind::Rebuild);
        // The merge scans a file job put off walk regions of the units: the scan below covers them.
        self.scanning.put_off.retain(|(_, k)| *k == ScanKind::Rebuild);
        if !rebuild {
            self.scanning.put_off.push((plan, ScanKind::Merge));
        }
        if self.resume_put_off_scans() == 0 {
            return None;
        }
        self.scanning.running.last().map(|j| j.job)
    }

    /// Lets every running scan go: its walk stops, what it read and did not apply is dropped
    /// (nothing is removed for it, a rebuild keeps the old model) and its job ends as cancelled,
    /// labelled "Scan <why>" with `<flag>: true` in its result. Returns each scan's plan and kind,
    /// to start again.
    fn let_scans_go(&mut self, why: &str, flag: &str) -> Vec<(Plan, ScanKind)> {
        if self.scanning.running.is_empty() {
            return Vec::new();
        }
        let now = self.clock.now();
        self.scanning.walking.clear();
        let mut stopped = Vec::new();
        for mut job in std::mem::take(&mut self.scanning.running) {
            // What it found gone is found again when it starts again.
            self.scanning.gone.retain(|(id, ..)| *id != job.job);
            // The walk stops before its next file (its sends fail too); it is not waited for.
            job.cancel.store(true, Ordering::Relaxed);
            job.stop();
            job.current = None;
            job.pending.clear();
            job.fresh = None;
            let what = if job.kind == ScanKind::Rebuild { "Rebuild" } else { "Scan" };
            let mut result = json!({
                "roots": job.plan.roots,
                "folders": job.folders,
                "found": job.walker.found.max(job.indexed),
                "indexed": job.indexed,
                "cancelled": true,
            });
            if let Some(r) = result.as_object_mut() {
                r.insert(flag.into(), json!(true));
            }
            let label = format!("{what} {why}");
            self.jobs.progress(job.job, job.progress(), label.clone());
            self.jobs.finish(job.job, Outcome::Cancelled(result), label, now);
            stopped.push((job.plan, job.kind));
        }
        self.sync_thumbnails();
        stopped
    }

    /// Starts the scans a file job put off again, once no file job runs. Returns how many it
    /// started.
    pub(crate) fn resume_put_off_scans(&mut self) -> usize {
        if self.scanning.put_off.is_empty() || self.file_jobs_running() {
            return 0;
        }
        let mut n = 0;
        for (plan, kind) in std::mem::take(&mut self.scanning.put_off) {
            match kind {
                // A rebuild walks every unit as the rules are now.
                ScanKind::Rebuild => self.start_rebuild(),
                ScanKind::Merge => {
                    self.start_plan(plan, kind);
                }
            }
            n += 1;
        }
        n
    }

    /// Takes in what the scans found and applies up to `photos` of it; ends the scans that are
    /// complete. Returns how many folders were applied and scans ended.
    /// The Folder Manager's browse answers are taken in here too (`folders.rs`).
    pub(crate) fn poll_scans(&mut self, photos: usize) -> usize {
        let browsed = self.poll_browse(false) + self.resume_put_off_scans();
        if self.scanning.running.is_empty() {
            // What the scans held for `albums.json` is written once they are done.
            self.write_albums_file();
            return browsed;
        }
        self.scanning.applying = true;
        let n = self.poll_running(photos);
        self.scanning.applying = false;
        self.write_albums_file();
        n
    }

    /// [`Session::poll_scans`]'s work on the running scans.
    fn poll_running(&mut self, photos: usize) -> usize {
        let mut budget = Budget::new(photos);
        let mut n = 0;
        let mut running = Vec::new();
        let mut complete = Vec::new();
        for mut job in std::mem::take(&mut self.scanning.running) {
            job.receive(false, &mut self.scan_warnings);
            if !job.stopped && self.jobs.cancel_requested(job.job) {
                job.stop();
            }
            self.note_walk_end(&mut job);
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
            // Scans a file job put off start again (unless one still runs: then they wait for it).
            self.resume_put_off_scans();
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
                if job.fresh.is_none()
                    && self.library.revision() != job.revision
                    && dalica_scan::meta::sync_parts(op).is_some()
                    && let Some(folder) = &job.current
                {
                    // The model changed since the folder's diff (a command, another scan): what
                    // the files said is decided again, so an edit made meanwhile stands.
                    let scan = Scan { folders: vec![folder.clone()], ..Scan::default() };
                    job.pending = dalica_scan::diff(&mut self.library, &scan).into();
                    job.revision = self.library.revision();
                    continue;
                }
                let Some(op) = job.pending.pop_front() else { break };
                applied += n;
                // What the folder's sidecar says about albums and its photo sort rides in the op
                // that adds the folder or its photos, so no sidecar write can run in between.
                let with = job.current.as_ref().and_then(|folder| {
                    let target = job.fresh.as_ref().unwrap_or(&self.library);
                    match job.kind {
                        ScanKind::Rebuild => with_albums(target, &op, folder, job.albums.as_ref(), &|_| false),
                        ScanKind::Merge => {
                            let file = &self.scanning.albums_file;
                            with_albums(target, &op, folder, file.defs.as_ref(), &|a| file.deleted(target, a))
                        }
                    }
                });
                let result = match with {
                    Some(with) => match self.apply_scan_op(job, with) {
                        Ok(()) => Ok(()),
                        Err(e) => {
                            self.scan_warnings.push(format!("album memberships not applied: {e}"));
                            self.apply_scan_op(job, op)
                        }
                    },
                    None => self.apply_scan_op(job, op),
                };
                job.revision = self.library.revision();
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
            // The walk is over, and every other merge scan's too: the folders held back take their
            // turn, now that the folders gone are known.
            let walk_over = job.rx.is_none();
            let others_walk = self.scanning.walking.iter().any(|(id, _)| *id != job.job);
            if walk_over && !others_walk && job.queue.is_empty() && !job.held.is_empty() {
                job.queue.extend(job.held.drain(..));
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
            // A folder at a path the model does not know may be a known folder moved or renamed on
            // disk (`data-model.md` §10.2): while the walk runs and a known folder sharing its
            // photos is still to be found, it waits; once the walk is over it is paired with a
            // folder gone, as the watcher's rescans pair them, and keeps that folder's ids.
            let unknown = job.kind == ScanKind::Merge && job.fresh.is_none() && self.library.folder_by_path(&folder.path).is_none();
            if unknown && (!walk_over || others_walk) && job.may_be_moved(&self.library, &folder, &self.scanning.walking) {
                job.held.push(folder);
                continue;
            }
            let scan = Scan { folders: vec![folder], ..Scan::default() };
            // A folder at an unknown path pairs with the folders gone where this scan looked (once
            // its walk is over) and with those other merge scans found gone (another Scan Always
            // folder's region: a folder moved from one into the other).
            let gone = match (&job.fresh, unknown) {
                (None, true) => {
                    let mut gone = match (&job.summary, walk_over) {
                        (Some(s), true) => gone_folders(&self.library, s, &job.empty, &job.plan, &job.seen),
                        _ => Vec::new(),
                    };
                    gone.extend(self.gone_elsewhere());
                    gone
                }
                _ => Vec::new(),
            };
            let ops = match &mut job.fresh {
                Some(fresh) => dalica_scan::diff(fresh, &scan),
                None if !gone.is_empty() => match scan.folders.first() {
                    Some(found) => dalica_scan::diff_found(&mut self.library, found, &gone),
                    None => Vec::new(),
                },
                None => dalica_scan::diff(&mut self.library, &scan),
            };
            let folder = scan.folders.into_iter().next();
            job.revision = self.library.revision();
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

    /// A watcher rescan's `op` with what the sidecars of `folders` (the folders it read) say about
    /// albums and photo sorts, as [`with_albums`] gives a scan's; `None` when they say nothing the
    /// op needs. An album deleted this session is never brought back.
    pub(crate) fn rescan_op_with_albums(&self, op: &Op, folders: &[FoundFolder]) -> Option<Op> {
        let file = &self.scanning.albums_file;
        let mut out: Option<Op> = None;
        for folder in folders {
            let current = out.as_ref().unwrap_or(op);
            if let Some(with) = with_albums(&self.library, current, folder, file.defs.as_ref(), &|a| file.deleted(&self.library, a)) {
                out = Some(with);
            }
        }
        out
    }

    /// Applies one of `job`'s ops: to its own model (a rebuild's, built aside), else to the live
    /// one (a rebuild filling an empty model in place records no first-indexed time).
    fn apply_scan_op(&mut self, job: &mut ScanJob, op: Op) -> Result<(), String> {
        match &mut job.fresh {
            Some(fresh) => fresh.apply(op).map(|_| ()).map_err(|e| e.to_string()),
            None => {
                self.scanning.rebuilding = job.kind == ScanKind::Rebuild;
                let applied = self.apply(op).map(|_| ()).map_err(|e| e.to_string());
                self.scanning.rebuilding = false;
                applied
            }
        }
    }

    /// A rebuild's last step (`data-model.md` §4): the albums `albums.json` defines that no folder
    /// brought back (an empty album) are created, the definitions it holds are given, and the
    /// covers it names are set where the photo is a member.
    fn finish_rebuild_albums(&mut self, job: &mut ScanJob) {
        let Some(defs) = job.albums.take() else { return };
        let target = job.fresh.as_ref().unwrap_or(&self.library);
        let (ops, notes) = album_definitions(target, &defs);
        self.scan_warnings.extend(notes);
        for op in ops {
            if let Err(e) = self.apply_scan_op(job, op) {
                self.scan_warnings.push(format!("albums from {} not applied: {e}", dalica_store::albums::FILE));
            }
        }
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
        if job.kind == ScanKind::Rebuild {
            // In place, even a cancelled rebuild gives back every album (its other members come
            // with the next scans); one built aside is dropped when cancelled.
            if job.fresh.is_none() || (summary.is_some() && !cancelled) {
                self.finish_rebuild_albums(&mut job);
            }
            self.scanning.albums_file.changed();
        }
        if let Some(s) = summary.as_ref().filter(|_| !cancelled) {
            match job.fresh.take() {
                Some(fresh) => {
                    let before: Vec<_> = self.library.folders().map(|f| f.id).collect();
                    self.library = fresh;
                    self.scanning.albums_file.see_all(self.library.albums().map(|(id, _)| id));
                    // Folders the rebuilt model no longer has: their pending writes are dropped.
                    let left: Vec<_> = self.left_library(before).collect();
                    if !left.is_empty() {
                        self.send(Msg::Dirty(left));
                    }
                    self.undo.clear();
                    // What merge scans found gone names folders of the model replaced.
                    self.scanning.gone.clear();
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
                    // subtrees it left out belong to other scans). One that held photos stays while
                    // another merge scan runs: it may be a folder moved into that one's region.
                    let gone = gone_folders(&self.library, s, &job.empty, &job.plan, &job.seen);
                    let others = self.scanning.running.iter().any(|j| j.kind == ScanKind::Merge && j.fresh.is_none());
                    for id in gone {
                        let path = self.library.folder(id).map(|f| f.path.clone());
                        if let Some(path) = path.filter(|_| others && !self.library.photos_in(id).is_empty()) {
                            if !self.scanning.gone.iter().any(|(_, g, _)| *g == id) {
                                self.scanning.gone.push((job.job, id, path));
                            }
                            continue;
                        }
                        self.scanning.gone.retain(|(_, g, _)| *g != id);
                        if let Err(e) = self.apply(Op::RemoveFolder { id }) {
                            self.scan_warnings.push(format!("scan result not applied: {e}"));
                        }
                    }
                }
            }
        }
        self.scanning.walking.retain(|(id, _)| *id != job.job);
        if cancelled || summary.is_none() {
            // What a scan that did not finish found gone is not trusted.
            self.scanning.gone.retain(|(id, ..)| *id != job.job);
        }
        if !self.scanning.running.iter().any(|j| j.kind == ScanKind::Merge && j.fresh.is_none()) {
            self.remove_gone_elsewhere();
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

    /// `job`'s walk ended (or was let go): it no longer walks, and a merge scan's folders gone
    /// where it walked to the end that hold photos are offered to the other merge scans
    /// ([`Scanning`]'s `gone`), which may find them moved into their regions.
    fn note_walk_end(&mut self, job: &mut ScanJob) {
        if job.rx.is_some() || job.walk_noted {
            return;
        }
        job.walk_noted = true;
        self.scanning.walking.retain(|(id, _)| *id != job.job);
        if job.stopped || job.kind != ScanKind::Merge || job.fresh.is_some() {
            return;
        }
        let Some(s) = job.summary.as_ref().filter(|s| !s.cancelled) else { return };
        for id in gone_folders(&self.library, s, &job.empty, &job.plan, &job.seen) {
            let Some(folder) = self.library.folder(id) else { continue };
            if !self.library.photos_in(id).is_empty() && !self.scanning.gone.iter().any(|(_, g, _)| *g == id) {
                self.scanning.gone.push((job.job, id, folder.path.clone()));
            }
        }
    }

    /// The folders merge scans found gone that are still where they were (not paired since).
    fn gone_elsewhere(&self) -> Vec<FolderId> {
        let lib = &self.library;
        self.scanning.gone.iter().filter(|(_, id, path)| lib.folder(*id).is_some_and(|f| &f.path == path)).map(|(_, id, _)| *id).collect()
    }

    /// No merge scan runs any more: the folders found gone that no scan paired leave the library.
    fn remove_gone_elsewhere(&mut self) {
        for id in self.gone_elsewhere() {
            if let Err(e) = self.apply(Op::RemoveFolder { id }) {
                self.scan_warnings.push(format!("scan result not applied: {e}"));
            }
        }
        self.scanning.gone.clear();
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

/// The folders of `lib` under the roots `summary` walked to the end (but those that held no entries
/// at all, `empty`), outside the subtrees `plan` leaves out, that the walk did not find (`seen`):
/// gone from disk, or moved there.
fn gone_folders(lib: &Library, summary: &Summary, empty: &[PathBuf], plan: &Plan, seen: &HashSet<PathBuf>) -> Vec<FolderId> {
    lib.folders()
        .filter(|f| {
            summary.walked.iter().any(|root| f.path.starts_with(root) && !empty.contains(root))
                && !plan.excluded.iter().any(|x| f.path.starts_with(x))
                && !seen.contains(&f.path)
        })
        .map(|f| f.id)
        .collect()
}

/// `op`, one of the ops a scan applies for `folder`, with what the folder's sidecar says about
/// albums and the photo sort (`data-model.md` §3, §3.2, §4), or `None` when it says nothing the op
/// needs. Only what the model does not know yet comes in (for folders and photos it knows, the
/// model's word stands, as for everything else a sidecar says; a folder renamed on disk keeps the
/// model's word too, [`Session::files_op_after`]):
/// - a folder the op adds takes its photo sort (`SetFolderSorts` in the same op);
/// - each photo the op adds takes its memberships with their position keys (`AddMembers`); a
///   folder whose sidecar cannot be read takes them from `albums.json`'s copy of the order
///   (`defs`);
/// - an album the model lacks is created (`CreateAlbums`, before the memberships) with its
///   definition from `albums.json` (`defs`) when it has one, else under the name the folder gives
///   it (dated by its newest photo); a membership of an album named nowhere, or of one `skip`
///   names (deleted this session), stays in the sidecar and is not used.
fn with_albums(lib: &Library, op: &Op, folder: &FoundFolder, defs: Option<&AlbumsReading>, skip: &dyn Fn(&AlbumId) -> bool) -> Option<Op> {
    let children: &[Op] = match op {
        Op::Batch { ops } => ops,
        other => std::slice::from_ref(other),
    };
    let added = children.iter().find_map(|o| match o {
        Op::AddFolder { folder: f } if f.path == folder.path => Some(f.id),
        _ => None,
    });
    let id: FolderId = added.or_else(|| lib.folder_by_path(&folder.path))?;
    // Renamed or moved on disk: the folder is taken out and put back in the same op, and what the
    // model said about it comes back with it (`Session::files_op_after`).
    if children.iter().any(|o| matches!(o, Op::RemoveFolder { id: r } if *r == id)) {
        return None;
    }
    let mut extra = Vec::new();
    if added.is_some() && !folder.sort.is_default() {
        extra.push(Op::SetFolderSorts { sorts: vec![(id, folder.sort)] });
    }
    // A folder whose sidecar cannot be read: albums.json's copy of the order, by file name.
    let copied: HashMap<&str, Vec<(AlbumId, PosKey)>> = match defs.filter(|_| folder.sidecar_unread) {
        Some(defs) => {
            let mut out: HashMap<&str, Vec<(AlbumId, PosKey)>> = HashMap::new();
            for (album, def) in &defs.albums {
                for (path, key) in &def.order {
                    if path.parent() == Some(folder.path.as_path())
                        && let Some(name) = path.file_name().and_then(|n| n.to_str())
                    {
                        out.entry(name).or_default().push((album.clone(), key.clone()));
                    }
                }
            }
            out
        }
        None => HashMap::new(),
    };
    // Albums the op creates already (another folder's, in a rescan of several).
    let created: HashSet<&AlbumId> = children
        .iter()
        .filter_map(|o| if let Op::CreateAlbums { albums } = o { Some(albums) } else { None })
        .flat_map(|a| a.iter().map(|(id, _)| id))
        .collect();
    let mut create: BTreeMap<AlbumId, AlbumInfo> = BTreeMap::new();
    let mut members = Vec::new();
    for o in children {
        let Op::AddPhotos { photos } = o else { continue };
        for p in photos.iter().filter(|p| p.folder == id && lib.photo(p.id).is_none()) {
            let from_sidecar = folder.photos.binary_search_by(|f| f.name.as_str().cmp(&p.name)).ok().and_then(|i| folder.photos.get(i));
            let list: Vec<(AlbumId, PosKey)> = match copied.get(p.name.as_str()) {
                Some(list) => list.clone(),
                None => from_sidecar.map(|f| f.albums.iter().map(|(a, k)| (a.clone(), k.clone())).collect()).unwrap_or_default(),
            };
            for (album, key) in list {
                if lib.album(&album).is_none() && !created.contains(&album) && !create.contains_key(&album) {
                    if skip(&album) {
                        continue;
                    }
                    let def = defs.and_then(|d| d.albums.get(&album)).map(|d| d.info.clone());
                    let Some(info) = def.or_else(|| folder.album_names.get(&album).map(AlbumInfo::named)) else { continue };
                    create.insert(album.clone(), info);
                }
                members.push((album, p.id, key));
            }
        }
    }
    let mut ops = Vec::new();
    if !create.is_empty() {
        ops.push(Op::CreateAlbums { albums: create.into_iter().collect() });
    }
    if !members.is_empty() {
        ops.push(Op::AddMembers { members });
    }
    ops.append(&mut extra);
    if ops.is_empty() {
        return None;
    }
    Some(match op.clone() {
        Op::Batch { ops: mut all } => {
            all.append(&mut ops);
            Op::Batch { ops: all }
        }
        other => {
            ops.insert(0, other);
            Op::Batch { ops }
        }
    })
}

/// The ops that give `lib` what `albums.json` (`defs`) says once a rebuild found every folder
/// (`data-model.md` §4): the albums it defines that the folders did not bring back (an empty
/// album, or one whose members are in no watched folder) created, the definitions it holds given,
/// and the covers it names set where the photo is a member; with one note per cover that is not.
fn album_definitions(lib: &Library, defs: &AlbumsReading) -> (Vec<Op>, Vec<String>) {
    let (mut create, mut redefine, mut covers, mut notes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (id, def) in &defs.albums {
        match lib.album(id) {
            None => create.push((id.clone(), def.info.clone())),
            Some(own) if *own != def.info => redefine.push((id.clone(), def.info.clone())),
            Some(_) => {}
        }
        let Some(path) = &def.cover else { continue };
        let photo = path.parent().and_then(|dir| lib.folder_by_path(dir)).zip(path.file_name().and_then(|n| n.to_str()));
        let photo = photo.and_then(|(folder, name)| lib.photo_by_name(folder, name)).map(|p| p.id);
        match photo.filter(|p| lib.member_key(id, *p).is_some()) {
            Some(p) if lib.cover_set(id) != Some(p) => covers.push((id.clone(), Some(p))),
            Some(_) => {}
            None => notes.push(format!("album {id}'s cover is not one of its photos: it shows its first photo")),
        }
    }
    let mut ops = Vec::new();
    if !create.is_empty() {
        ops.push(Op::CreateAlbums { albums: create });
    }
    if !redefine.is_empty() {
        ops.push(Op::SetAlbumInfo { albums: redefine });
    }
    if !covers.is_empty() {
        ops.push(Op::SetCovers { covers });
    }
    (ops, notes)
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

    /// The scan reads a sidecar on its own thread with `dalica-scan`'s `photo_info` / `folder_info`;
    /// the store reads it with `mapping`. They must agree, or the first write after a scan would
    /// change a file nobody touched: on every kind of value (usable, wrong type, past a cap,
    /// untidy tags, absent), the scan's info is what the store's reading gives a new photo.
    #[test]
    fn the_scans_reading_of_a_sidecar_is_the_stores() {
        use dalica_library::{FolderInfo, MAX_CAPTION, MAX_DESCRIPTION, MAX_OPERATION, MAX_PLACE};
        use dalica_sidecar::{Limits, Sidecar};
        use dalica_store::mapping::{PhotoState, read_folder, read_photo};
        use serde_json::Value;

        let long = |n: usize| Value::String("ü".repeat(n));
        let photos = [
            json!({ "caption": "Tram 28", "tags": ["a", " b ", "A", "", "c\nd"], "hidden": true, "edits": ["rotate(90)", "x(1)"], "redo": ["sepia()"], "date": "2026-06-04T19:42:10" }),
            json!({ "caption": 3, "tags": [1], "hidden": "no", "edits": "rotate(90)", "redo": {}, "date": "2026:06:04 19:42:10" }),
            json!({ "caption": long(MAX_CAPTION), "edits": vec!["flip_h()"; 10_001], "date": "not a date" }),
            json!({ "caption": "  padded  ", "redo": ["y".repeat(MAX_OPERATION + 1)], "hidden": false }),
            json!({ "star": true }),
            json!("not an object"),
        ];
        let folders = [
            json!({ "description": "Weekend", "date": "2026-06-04", "place": "Lisbon", "hidden": true }),
            json!({ "description": long(MAX_DESCRIPTION), "place": long(MAX_PLACE), "date": "2026-02-30", "hidden": 1 }),
            json!({ "description": ["x"], "place": "a\tb" }),
            json!([]),
        ];
        for (i, folder) in folders.iter().enumerate() {
            let entries: serde_json::Map<String, Value> = photos.iter().enumerate().map(|(j, p)| (format!("p{j}.jpg"), p.clone())).collect();
            let doc = json!({ "dalica": { "version": 1 }, "folder": folder, "photos": entries });
            let sidecar = Sidecar::parse(&serde_json::to_vec(&doc).unwrap(), &Limits::default()).unwrap();
            assert_eq!(dalica_scan::folder_info(&sidecar), read_folder(&sidecar).merged(&FolderInfo::default()), "folder {i}");
            for j in 0..=photos.len() {
                let name = format!("p{j}.jpg");
                let store = read_photo(&sidecar, &name).merged(&PhotoState::default()).info;
                assert_eq!(dalica_scan::photo_info(&sidecar, &name), store, "photo {j}");
            }
        }
    }

    /// M9's fields the same way: a photo's memberships, the folder's photo sort and the album
    /// names, read by `dalica-scan` on the scan's thread, are what `dalica-store`'s `mapping` gives
    /// a photo and a folder new to the model (usable entries, entries Dalica cannot use, values of
    /// the wrong type, a sort it cannot sort by, names to clean or past the cap).
    #[test]
    fn the_scans_reading_of_albums_and_sorts_is_the_stores() {
        use dalica_library::{MAX_ALBUM_NAME, PhotoSort};
        use dalica_sidecar::{Limits, Sidecar};
        use dalica_store::mapping::{Reading, read_album_names, read_folder, read_photo};
        use serde_json::Value;

        let photos = [
            json!({ "albums": { "a91c0e": "a1", "a77d02": "bz0" } }),
            json!({ "albums": { "a91c0e": "a1", "bad id": "a1", "a1234": "not a key!", "a5678": 3 } }),
            json!({ "albums": ["a91c0e"] }),
            json!({ "albums": {} }),
            json!({ "star": true }),
        ];
        let folders = [
            json!({ "sort": "name", "reverse": true }),
            json!({ "sort": "size" }),
            json!({ "sort": "date", "reverse": false }),
            json!({ "sort": "manual", "reverse": true }),
            json!({ "sort": "modified" }),
            json!({ "sort": 3, "reverse": "yes" }),
            json!({}),
        ];
        let names = [
            json!({ "a91c0e": { "name": "Best of 2026" }, "a77d02": { "name": " Trams\n" } }),
            json!({ "a91c0e": { "name": "x".repeat(MAX_ALBUM_NAME + 10) }, "zz": { "name": "Bad" }, "a1234": { "name": "  " }, "a5678": {} }),
            json!(["not", "an", "object"]),
            json!({}),
        ];
        for (i, folder) in folders.iter().enumerate() {
            for (k, top) in names.iter().enumerate() {
                let entries: serde_json::Map<String, Value> = photos.iter().enumerate().map(|(j, p)| (format!("p{j}.jpg"), p.clone())).collect();
                let doc = json!({ "dalica": { "version": 1 }, "folder": folder, "photos": entries, "albums": top });
                let sidecar = Sidecar::parse(&serde_json::to_vec(&doc).unwrap(), &Limits::default()).unwrap();
                assert_eq!(dalica_scan::folder_sort(&sidecar), read_folder(&sidecar).photo_sort(PhotoSort::default()), "folder {i}");
                let store = match read_album_names(&sidecar).0 {
                    Reading::Usable(n) => n,
                    Reading::Unusable => BTreeMap::new(),
                };
                assert_eq!(dalica_scan::album_names(&sidecar), store, "names {k}");
                for j in 0..=photos.len() {
                    let name = format!("p{j}.jpg");
                    let store = match read_photo(&sidecar, &name).albums {
                        Reading::Usable(m) => m,
                        Reading::Unusable => BTreeMap::new(),
                    };
                    assert_eq!(dalica_scan::photo_albums(&sidecar, &name), store, "photo {j}");
                }
            }
        }
        // The cases are not all empty.
        let doc = json!({ "photos": { "p.jpg": photos[0] }, "albums": names[0], "folder": folders[0] });
        let sidecar = Sidecar::parse(&serde_json::to_vec(&doc).unwrap(), &Limits::default()).unwrap();
        assert_eq!(dalica_scan::photo_albums(&sidecar, "p.jpg").len(), 2);
        assert_eq!(dalica_scan::album_names(&sidecar).values().map(String::as_str).collect::<Vec<_>>(), ["Trams", "Best of 2026"]);
        assert!(dalica_scan::folder_sort(&sidecar).reverse);
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
