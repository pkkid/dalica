//! The metadata write-back (`data-model.md` §5, `xmp-interop.md` §1–§5): captions, tags, stars,
//! places and dates the user changes reach the photo files themselves.
//!
//! - **The hook.** After every op the session applies, [`Session::metadata_applied`] is told the
//!   owned values (caption, tags, star, the effective date) of the op's candidate photos from
//!   before it, and records the fields that changed, and by what: a command (undo and redo
//!   included, so every surface reaches it) or the files themselves (scans, rescans and imports,
//!   which are never written back). [`Session::take_metadata_changes`] hands the records out.
//! - **Queueing.** A command's change queues one write per photo with the fields whose option is
//!   on (Options → General → Files: captions, tags and places; stars as ratings; dates) through
//!   `dalica-store`'s [`MetaWriteQueue`], which batches and debounces them per photo.
//! - **Writing.** The queue lives on a worker thread of its own ("dalica-metadata-writes"); the
//!   owner thread never touches the disk. Writes that are due run as a `metadataWrite` job
//!   ("Writing metadata", with progress and Cancel): the worker asks for one, the owner registers
//!   it at its next [`Session::poll`], and the worker writes one file at a time, checking Cancel
//!   before each. Cancel drops the writes not made yet (the folder sidecar keeps their values).
//!   Writes waiting for an offline volume are tried again on their own, without a job.
//! - **Results** come back through `poll`: the new last-synced hashes (a derived op); values another
//!   application changed come back as one undoable import op ("Import Metadata", never written
//!   back); both changed is noted for `photo.metadata` ([`Session::note_changed_elsewhere`]); a file
//!   Dalica wrote while the model knew its version gets its new size and time, so a rescan does
//!   not take Dalica's own write for an outside change; a failed write becomes a
//!   [`MetadataEvent`] (the UI's toasts), naming the file and the fields.
//!
//! [`Session::pending_metadata_writes`] counts the photos still to write (`library.inspect` →
//! `pendingWrites.metadata`); [`Session::flush_metadata`] writes them all now (the CLI, tests). When
//! the session closes, the worker writes what is still queued before it stops.
//!
//! **Before a Save reads a photo** (M9.28): a file job's worker holds a [`WriteFlusher`]
//! ([`Session::metadata_flusher`]) and, through it, has that one photo's queued write made at once
//! on the writer thread, between two writes, and learns the version Dalica's own writes left the
//! file at, starting from the version the library knew ([`WriteFlusher::settle`]). The Save then
//! expects that version, so a caption set just before Save is written and the Save is not refused
//! as "changed on disk"; a version no write of Dalica's explains (a change made elsewhere) is still
//! refused. A write waiting for an offline volume is not tried (the Save of such a photo is refused
//! anyway), and nothing else in the queue is written early.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use dalica_core::{Clock, DateTime, FolderId, PhotoId, Timestamp};
use dalica_library::sync::Field as SyncField;
use dalica_library::{Library, Op, Photo};
use dalica_store::metawrite::{self, FieldOutcome, FileResult, MetaWriteQueue, Request, Status, Values};
use dalica_store::writer::RootCheck;
use serde_json::{Value, json};

use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::session::Session;
use crate::settings::MetadataWrites;
use crate::undo::Step;

/// The most photo entries kept across the recorded changes; past it the oldest changes are dropped
/// (and counted), so a session nobody drains stays bounded.
pub const MAX_PENDING: usize = 1_000_000;
/// The most failed-write events kept ([`Session::metadata_events`]); the oldest go first.
pub const MAX_EVENTS: usize = 64;
/// The job's label while it runs (`jobs.list`, the activity popover).
pub const JOB_LABEL: &str = "Writing metadata";
/// The undo label of values imported from a file another application changed.
pub const IMPORT_LABEL: &str = "Import Metadata";
/// How long the writer sleeps when it has nothing to do (it wakes at once for a message).
const TICK: Duration = Duration::from_millis(100);
/// The most photos whose last own write the writer remembers for [`WriteFlusher::settle`]; past
/// it the oldest are forgotten (a Save of one of them is then refused as changed on disk, never
/// let through).
pub const MAX_OWN_WRITES: usize = 4096;
/// The longest [`WriteFlusher::settle`] waits for the writer (one file write ahead of it, as a
/// rule); past it the Save goes on with the version the library knew.
pub const SETTLE_PATIENCE: Duration = Duration::from_secs(60);

/// File extensions that are never RAW files (so a photo with one never needs an `.xmp` sidecar
/// name worked out). Anything else may be a RAW: the queue tells by the file's content.
const NOT_RAW: [&str; 18] =
    ["jpg", "jpeg", "jpe", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif", "jxl", "psd", "mp4", "mov", "m4v", "mkv"];

/// One owned metadata field (`xmp-interop.md` §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Caption,
    Tags,
    Star,
    Place,
    Date,
}

impl Field {
    pub const ALL: [Field; 5] = [Field::Caption, Field::Tags, Field::Star, Field::Place, Field::Date];

    const fn bit(self) -> u8 {
        1 << self as u8
    }

    /// The field's name in messages ("caption", "tags", "star", "place", "date taken").
    pub fn name(self) -> &'static str {
        self.sync().name()
    }

    /// The field as the write queue and `dalica_library::sync` name it.
    fn sync(self) -> SyncField {
        match self {
            Field::Caption => SyncField::Caption,
            Field::Tags => SyncField::Tags,
            Field::Star => SyncField::Rating,
            Field::Place => SyncField::Place,
            Field::Date => SyncField::Date,
        }
    }

    fn of_sync(f: SyncField) -> Field {
        match f {
            SyncField::Caption => Field::Caption,
            SyncField::Tags => Field::Tags,
            SyncField::Rating => Field::Star,
            SyncField::Place => Field::Place,
            SyncField::Date => Field::Date,
        }
    }

    /// Whether `w` lets this field be written into files.
    fn allowed(self, w: &MetadataWrites) -> bool {
        match self {
            Field::Caption | Field::Tags | Field::Place => w.captions_tags_places,
            Field::Star => w.stars_as_ratings,
            Field::Date => w.dates,
        }
    }
}

/// A set of [`Field`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Fields(u8);

impl Fields {
    pub const NONE: Fields = Fields(0);
    pub const ALL: Fields = Fields(0b1_1111);

    pub fn of(fields: &[Field]) -> Fields {
        Fields(fields.iter().fold(0, |b, f| b | f.bit()))
    }

    pub fn contains(self, f: Field) -> bool {
        self.0 & f.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, f: Field) {
        self.0 |= f.bit();
    }

    /// The fields in the set, in [`Field::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = Field> {
        Field::ALL.into_iter().filter(move |f| self.contains(*f))
    }
}

/// What made a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// A command: the user, an agent, or undo and redo of either. Written back.
    Command,
    /// What a scan or rescan found in the files, or an import: never written back.
    Files,
}

/// The owned fields one applied op changed, per photo (by id, each photo once).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataChange {
    pub origin: Origin,
    pub photos: Vec<(PhotoId, Fields)>,
}

/// A metadata write that failed (`xmp-interop.md` §1 principle 4): the file is as it was and the
/// folder sidecar keeps the values. The UI shows each as a toast.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataEvent {
    /// Unique in the process and rising, so a reader remembers the last one it saw.
    pub seq: u64,
    pub photo: PhotoId,
    pub path: PathBuf,
    /// The fields not written (empty when the whole file failed before any field was tried).
    pub fields: Vec<Field>,
    /// Why, in words.
    pub reason: String,
}

impl MetadataEvent {
    /// "Could not write the caption and tags into IMG_0001.jpg: the file is read-only; the folder
    /// sidecar keeps the value".
    pub fn message(&self) -> String {
        let file = self.path.file_name().map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let names: Vec<&str> = self.fields.iter().map(|f| f.name()).collect();
        let what = match names.as_slice() {
            [] => "metadata".to_string(),
            [one] => format!("the {one}"),
            [first @ .., last] => format!("the {} and {last}", first.join(", ")),
        };
        format!("Could not write {what} into {file}: {}", self.reason)
    }
}

/// The numbers of the next [`MetadataEvent`], across sessions.
static NEXT_EVENT: AtomicU64 = AtomicU64::new(1);

/// The hook's state: the changes recorded and not yet taken, and the write-back.
#[derive(Default)]
pub(crate) struct Metadata {
    changes: VecDeque<MetadataChange>,
    /// Photo entries in `changes`.
    entries: usize,
    /// Changes dropped to keep within [`MAX_PENDING`].
    dropped: u64,
    writer: Writer,
}

impl std::fmt::Debug for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metadata")
            .field("changes", &self.changes.len())
            .field("dropped", &self.dropped)
            .field("queued", &self.writer.queued.len())
            .field("running", &self.writer.worker.is_some())
            .finish_non_exhaustive()
    }
}

impl Metadata {
    /// Records `change`, then drops the oldest changes (counting them) while more than `cap` photo
    /// entries are kept; the newest change is always kept.
    fn record(&mut self, change: MetadataChange, cap: usize) {
        self.entries = self.entries.saturating_add(change.photos.len());
        self.changes.push_back(change);
        while self.entries > cap && self.changes.len() > 1 {
            if let Some(old) = self.changes.pop_front() {
                self.entries = self.entries.saturating_sub(old.photos.len());
                self.dropped = self.dropped.saturating_add(1);
            }
        }
    }
}

/// The owner thread's side of the write-back.
#[derive(Default)]
struct Writer {
    worker: Option<Worker>,
    /// Photos sent to the worker and not written yet, with the number of the message that last
    /// sent each.
    queued: HashMap<PhotoId, u64>,
    /// The number of the last message that queued writes.
    sent: u64,
    /// The photos of `queued` whose last write waits for their volume (or their folder) to come
    /// back: the worker tries them again on its own, so they do not keep the session busy
    /// ([`Session::metadata_writes_busy`]). Always a subset of `queued`.
    waiting: HashSet<PhotoId>,
    /// The worker stopped (its channel closed): what is queued can no longer land, so it does not
    /// keep the session busy either.
    stopped: bool,
    /// The options the worker has.
    options: Option<metawrite::Options>,
    /// The roots found offline and the root checks, as the session last gave them (`offline.rs`).
    held: Vec<PathBuf>,
    guards: Vec<(PathBuf, RootCheck)>,
    /// The job running, if any.
    job: Option<Running>,
    /// The failed writes, oldest first, at most [`MAX_EVENTS`].
    events: VecDeque<MetadataEvent>,
}

/// The job writing now, and what it did so far.
struct Running {
    id: JobId,
    cancel: Arc<AtomicBool>,
    done: u64,
    written: u64,
    unchanged: u64,
    imported: u64,
    conflicts: u64,
    failed: u64,
    skipped: u64,
    waiting: u64,
}

impl Running {
    fn new(id: JobId, cancel: Arc<AtomicBool>) -> Running {
        Running { id, cancel, done: 0, written: 0, unchanged: 0, imported: 0, conflicts: 0, failed: 0, skipped: 0, waiting: 0 }
    }

    /// Counts one file's result.
    fn count(&mut self, r: &FileResult) {
        let slot = match &r.status {
            Status::Written(_) => &mut self.written,
            Status::Unchanged | Status::Gone => &mut self.unchanged,
            Status::Skipped(_) => &mut self.skipped,
            Status::Failed(_) => &mut self.failed,
            Status::Waiting(_) => &mut self.waiting,
            Status::Requeued(_) => return,
        };
        *slot = slot.saturating_add(1);
        self.done = self.done.saturating_add(1);
        if r.imports().next().is_some() {
            self.imported = self.imported.saturating_add(1);
        }
        if r.fields.iter().any(|(_, o)| matches!(o, FieldOutcome::Conflict { .. })) {
            self.conflicts = self.conflicts.saturating_add(1);
        }
    }

    fn summary(&self) -> Value {
        json!({
            "written": self.written, "unchanged": self.unchanged, "imported": self.imported, "conflicts": self.conflicts,
            "failed": self.failed, "skipped": self.skipped, "waiting": self.waiting,
        })
    }
}

/// The worker thread and its two channels.
struct Worker {
    tx: Option<Sender<ToWorker>>,
    rx: Receiver<FromWorker>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Worker {
    /// Ends the worker after it has written what is still queued.
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A photo file's version as the library knows it: size and modification time.
type Version = (u64, Timestamp);

enum ToWorker {
    /// Writes to queue (message number `seq`), asked for at `at` (the debounce counts from there),
    /// each with the version of the file the model knows.
    Queue {
        seq: u64,
        at: Timestamp,
        requests: Vec<(Request, Version)>,
    },
    /// The job to write under, and its Cancel flag.
    Job(JobId, Arc<AtomicBool>),
    Options(metawrite::Options),
    Hold(Vec<PathBuf>),
    Guard(Vec<(PathBuf, RootCheck)>),
    /// Write everything queued now, then answer.
    Flush(Sender<()>),
    /// Write `photo`'s queued request now (unless it waits for its volume), then answer the version
    /// Dalica's own writes left `path` at, starting from `seen` ([`WriteFlusher::settle`]).
    Settle {
        photo: PhotoId,
        path: PathBuf,
        seen: Version,
        ack: Sender<Option<Version>>,
    },
}

/// A handle a file job's worker uses to have one photo's queued metadata write made before it
/// reads the photo ([`Session::metadata_flusher`]). Cloneable and `Send`; it never touches the
/// disk itself.
#[derive(Clone, Debug)]
pub(crate) struct WriteFlusher {
    tx: Sender<ToWorker>,
}

impl WriteFlusher {
    /// On a worker thread (never the owner's): has photo `photo` (at `path`)'s queued write made
    /// now and waits for it (at most `patience`). Returns the version the file is at when that is
    /// `seen` (the version the library knew) or a version Dalica's own writes made from it;
    /// `None` when the file is at a version none of them explains (changed elsewhere, or gone), or
    /// the writer did not answer in time: the caller then keeps expecting `seen`.
    pub(crate) fn settle(&self, photo: PhotoId, path: PathBuf, seen: Version, patience: Duration) -> Option<Version> {
        let (ack, answer) = mpsc::channel();
        self.tx.send(ToWorker::Settle { photo, path, seen, ack }).ok()?;
        answer.recv_timeout(patience).ok().flatten()
    }
}

enum FromWorker {
    /// Writes are due and there is no job to write them under.
    NeedJob,
    Wrote(Box<Wrote>),
    /// Writing `photo` panicked: it left the queue.
    Panicked {
        photo: PhotoId,
        path: PathBuf,
        seen: u64,
    },
    /// The job was cancelled: these photos were not written and left the queue.
    Cancelled {
        job: JobId,
        dropped: Vec<PhotoId>,
        seen: u64,
    },
    /// Nothing is left to write under the job (writes waiting for a volume aside).
    Idle {
        job: JobId,
    },
}

/// One file written (or tried).
struct Wrote {
    job: Option<JobId>,
    result: FileResult,
    /// The file's version before the write, when it was the one the model knew; and after it, when
    /// Dalica's write replaced it.
    before: Option<Version>,
    after: Option<Version>,
    /// Still queued (to try again).
    pending: bool,
    /// The last queueing message the worker had seen.
    seen: u64,
    /// Photos still to write under the job.
    active: usize,
}

/// The owned values of a photo the hook compares before and after an op.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Owned {
    star: bool,
    caption: String,
    tags: Vec<String>,
    /// The effective date: the user's, else the file's.
    date: Option<DateTime>,
}

/// `p`'s owned values in `library`.
pub(crate) fn owned(library: &Library, p: &Photo) -> Owned {
    let info = library.info(p.id);
    Owned { star: p.star, caption: info.caption.clone(), tags: info.tags.clone(), date: info.date.or(p.date) }
}

/// The fields in which `a` and `b` differ.
fn differ(a: &Owned, b: &Owned) -> Fields {
    let mut f = Fields::NONE;
    if a.star != b.star {
        f.insert(Field::Star);
    }
    if a.caption != b.caption {
        f.insert(Field::Caption);
    }
    if a.tags != b.tags {
        f.insert(Field::Tags);
    }
    if a.date != b.date {
        f.insert(Field::Date);
    }
    f
}

/// The photos `op` may change an owned field of, by id, each once (a tag rename: the photos of
/// `library` holding the tag). Photos added or removed are not changed: they enter or leave the
/// library with the values their files and sidecars hold. Walks nested batches without recursion.
pub(crate) fn candidates(library: &Library, op: &Op) -> Vec<PhotoId> {
    let mut out = BTreeSet::new();
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::SetStars { stars } => out.extend(stars.iter().map(|(id, _)| *id)),
            Op::UpdatePhotos { photos } => out.extend(photos.iter().map(|p| p.id)),
            Op::SetCaptions { captions } => out.extend(captions.iter().map(|(id, _)| *id)),
            Op::AddTags { ids, .. } | Op::RemoveTags { ids, .. } => out.extend(ids.iter().copied()),
            Op::RenameTag { from, .. } => {
                let from = dalica_library::tags::fold(from);
                out.extend(library.infos().filter(|(_, i)| i.tags.iter().any(|t| dalica_library::tags::fold(t) == from)).map(|(id, _)| id));
            }
            Op::SetTags { tags } => out.extend(tags.iter().map(|(id, _)| *id)),
            Op::SetDates { dates } => out.extend(dates.iter().map(|(id, _)| *id)),
            Op::SetPhotoInfo { photos } => out.extend(photos.iter().map(|(id, _)| *id)),
            Op::Batch { ops } => stack.extend(ops),
            // Folders and photos entering or leaving the library, hidden flags, recipes, folder
            // info and derived data own no metadata field.
            _ => {}
        }
    }
    out.into_iter().collect()
}

/// The owned values of `ids` in `library` now (photos not in it are left out).
pub(crate) fn snapshot(library: &Library, ids: &[PhotoId]) -> Vec<(PhotoId, Owned)> {
    ids.iter().filter_map(|id| library.photo(*id).map(|p| (*id, owned(library, p)))).collect()
}

/// The write queue's options for the settings' `w`.
fn queue_options(w: &MetadataWrites) -> metawrite::Options {
    metawrite::Options {
        captions_tags_places: w.captions_tags_places,
        stars_as_ratings: w.stars_as_ratings,
        dates: w.dates,
        raw_xmp_sidecars: w.raw_xmp_sidecars,
        update_modification_time: w.update_modification_time,
        ..metawrite::Options::default()
    }
}

/// Whether a file named `name` may be a RAW (its extension is not one of [`NOT_RAW`]).
fn maybe_raw(name: &str) -> bool {
    match dalica_core::names::split_ext(name).1 {
        Some(ext) => !NOT_RAW.iter().any(|n| n.eq_ignore_ascii_case(ext)),
        None => false,
    }
}

/// The stem of `name`, folded for comparing without case.
fn stem_key(name: &str) -> String {
    dalica_core::names::split_ext(name).0.to_lowercase()
}

impl Session {
    /// The hook, called by [`Session::apply`] after each applied op with the owned values of its
    /// candidate photos from before it: records the fields that changed, if any, and queues a
    /// command's changes for writing into the files.
    pub(crate) fn metadata_applied(&mut self, before: Vec<(PhotoId, Owned)>, origin: Origin) {
        let photos: Vec<(PhotoId, Fields)> = before
            .into_iter()
            .filter_map(|(id, old)| {
                let now = owned(&self.library, self.library.photo(id)?);
                Some((id, differ(&old, &now))).filter(|(_, f)| !f.is_empty())
            })
            .collect();
        if photos.is_empty() {
            return;
        }
        if origin == Origin::Command {
            self.queue_metadata_writes(&photos);
        }
        self.metadata.record(MetadataChange { origin, photos }, MAX_PENDING);
    }

    /// Queues a write of each photo's changed fields whose option is on.
    fn queue_metadata_writes(&mut self, photos: &[(PhotoId, Fields)]) {
        let options = self.settings().metadata_writes();
        let wanted: Vec<(PhotoId, BTreeSet<SyncField>)> = photos
            .iter()
            .map(|(id, f)| (*id, f.iter().filter(|f| f.allowed(&options)).map(Field::sync).collect::<BTreeSet<_>>()))
            .filter(|(_, f)| !f.is_empty())
            .collect();
        if wanted.is_empty() {
            return;
        }
        let raws = self.raw_names(wanted.iter().map(|(id, _)| *id));
        let requests: Vec<(Request, Version)> = wanted.into_iter().filter_map(|(id, fields)| self.write_request(id, fields, &raws)).collect();
        if requests.is_empty() {
            return;
        }
        if !self.start_metadata_writer() {
            for (r, _) in &requests {
                let fields = r.fields.iter().map(|f| Field::of_sync(*f)).collect();
                self.metadata_event(
                    r.photo,
                    r.path.clone(),
                    fields,
                    "the metadata writer could not start; the folder sidecar keeps the value".into(),
                );
            }
            return;
        }
        let at = self.clock.now();
        let w = &mut self.metadata.writer;
        w.sent = w.sent.saturating_add(1);
        let seq = w.sent;
        for (r, _) in &requests {
            w.queued.insert(r.photo, seq);
            // A new request ends a wait, as on the worker's side.
            w.waiting.remove(&r.photo);
        }
        w.send(ToWorker::Queue { seq, at, requests });
    }

    /// For the photos of `ids` that may be RAW files: the names, by folder and folded stem, of the
    /// photos of their folders that may be RAW files too (so two RAWs sharing a stem never share an
    /// `.xmp` sidecar). One pass over the library, only when there is such a photo.
    fn raw_names(&self, ids: impl Iterator<Item = PhotoId>) -> HashMap<(FolderId, String), Vec<String>> {
        let folders: BTreeSet<FolderId> = ids.filter_map(|id| self.library.photo(id)).filter(|p| maybe_raw(&p.name)).map(|p| p.folder).collect();
        let mut out: HashMap<(FolderId, String), Vec<String>> = HashMap::new();
        if folders.is_empty() {
            return out;
        }
        for p in self.library.photos().filter(|p| folders.contains(&p.folder) && maybe_raw(&p.name)) {
            out.entry((p.folder, stem_key(&p.name))).or_default().push(p.name.clone());
        }
        out
    }

    /// The write of `fields` of photo `id` with the model's values now, and the file's version as
    /// the model knows it.
    fn write_request(&self, id: PhotoId, fields: BTreeSet<SyncField>, raws: &HashMap<(FolderId, String), Vec<String>>) -> Option<(Request, Version)> {
        let p = self.library.photo(id)?;
        let folder = self.library.folder(p.folder)?;
        let info = self.library.info(id);
        let values =
            Values { caption: info.caption.clone(), tags: info.tags.clone(), star: p.star, place: None, date: self.library.effective_date(id) };
        let raws =
            raws.get(&(p.folder, stem_key(&p.name))).map(|names| names.iter().filter(|n| **n != p.name).cloned().collect()).unwrap_or_default();
        let request =
            Request { photo: id, path: folder.path.join(&p.name), fields, values, synced: self.library.synced(id).copied(), first_scan: false, raws };
        Some((request, (p.size, p.mtime)))
    }

    /// Starts the writer thread if it is not running, with the options, held roots and root checks
    /// it needs. `false` when it cannot start.
    fn start_metadata_writer(&mut self) -> bool {
        if self.metadata.writer.worker.is_some() {
            return true;
        }
        let (tx, worker_rx) = mpsc::channel();
        let (worker_tx, rx) = mpsc::channel();
        let clock = Arc::clone(&self.clock);
        let spawned = std::thread::Builder::new().name("dalica-metadata-writes".into()).spawn(move || work(worker_rx, worker_tx, clock));
        match spawned {
            Ok(thread) => {
                let options = queue_options(&self.settings().metadata_writes());
                let w = &mut self.metadata.writer;
                w.worker = Some(Worker { tx: Some(tx), rx, thread: Some(thread) });
                w.send(ToWorker::Options(options.clone()));
                w.options = Some(options);
                let (held, guards) = (w.held.clone(), w.guards.clone());
                w.send(ToWorker::Hold(held));
                w.send(ToWorker::Guard(guards));
                true
            }
            Err(e) => {
                log::error!("cannot start the metadata writer: {e}");
                false
            }
        }
    }

    /// The roots found offline changed (`offline.rs`): files under them wait.
    pub(crate) fn metadata_hold(&mut self, roots: Vec<PathBuf>) {
        let w = &mut self.metadata.writer;
        // Other held roots: the worker looks at every waiting photo again, and they count again.
        if w.held != roots {
            w.waiting.clear();
        }
        w.held = roots.clone();
        w.send(ToWorker::Hold(roots));
    }

    /// The root checks changed (`offline.rs`): run right before each write under them.
    pub(crate) fn metadata_guard(&mut self, roots: Vec<(PathBuf, RootCheck)>) {
        let w = &mut self.metadata.writer;
        w.guards = roots.clone();
        w.send(ToWorker::Guard(roots));
    }

    /// Called from [`Session::poll`] each frame: sends the writer changed options, registers the
    /// job it asks for, and applies what it wrote. Never blocks and never touches the disk. Returns
    /// how many things it handled.
    pub(crate) fn poll_metadata(&mut self) -> usize {
        if self.metadata.writer.worker.is_none() {
            return 0;
        }
        let options = queue_options(&self.settings().metadata_writes());
        let w = &mut self.metadata.writer;
        if w.options.as_ref() != Some(&options) {
            w.send(ToWorker::Options(options.clone()));
            w.options = Some(options);
        }
        let mut answers = Vec::new();
        if let Some(worker) = &w.worker {
            loop {
                match worker.rx.try_recv() {
                    Ok(a) => answers.push(a),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if !w.stopped {
                            log::error!("the metadata writer stopped");
                        }
                        w.stopped = true;
                        break;
                    }
                }
            }
        }
        let n = answers.len();
        for a in answers {
            self.metadata_answer(a);
        }
        n
    }

    fn metadata_answer(&mut self, answer: FromWorker) {
        match answer {
            FromWorker::NeedJob => self.start_metadata_job(),
            FromWorker::Wrote(w) => self.metadata_wrote(*w),
            FromWorker::Panicked { photo, path, seen } => {
                self.metadata.writer.done_with(photo, seen);
                self.metadata_event(photo, path, Vec::new(), "the write stopped unexpectedly; the folder sidecar keeps the values".into());
            }
            FromWorker::Cancelled { job, dropped, seen } => {
                let w = &mut self.metadata.writer;
                for photo in &dropped {
                    w.done_with(*photo, seen);
                }
                if let Some(r) = w.job.take_if(|r| r.id == job) {
                    let mut result = r.summary();
                    result["notWritten"] = json!(dropped.len());
                    let now = self.clock.now();
                    self.jobs.finish(job, Outcome::Cancelled(result), "Writing metadata cancelled", now);
                }
            }
            FromWorker::Idle { job } => {
                if let Some(r) = self.metadata.writer.job.take_if(|r| r.id == job) {
                    let now = self.clock.now();
                    self.jobs.finish(job, Outcome::Done(r.summary()), "Metadata written", now);
                }
            }
        }
    }

    /// Registers a job for the writes that are due (or hands the writer the running one again).
    fn start_metadata_job(&mut self) {
        if let Some(r) = &self.metadata.writer.job {
            let msg = ToWorker::Job(r.id, Arc::clone(&r.cancel));
            self.metadata.writer.send(msg);
            return;
        }
        let now = self.clock.now();
        let (id, cancel) = self.jobs.start(JobKind::MetadataWrite, JOB_LABEL, true, now);
        let total = u64::try_from(self.metadata.writer.queued.len()).unwrap_or(u64::MAX);
        self.jobs.progress(id, JobProgress { done: 0, total }, JOB_LABEL);
        let w = &mut self.metadata.writer;
        w.send(ToWorker::Job(id, Arc::clone(&cancel)));
        w.job = Some(Running::new(id, cancel));
    }

    /// Applies one file's result.
    fn metadata_wrote(&mut self, wrote: Wrote) {
        let Wrote { job, result, before, after, pending, seen, active } = wrote;
        let id = result.photo;
        let w = &mut self.metadata.writer;
        if !pending {
            w.done_with(id, seen);
        } else if matches!(result.status, Status::Waiting(_)) && w.queued.contains_key(&id) {
            w.waiting.insert(id);
        } else {
            w.waiting.remove(&id);
        }
        if let Some(r) = self.metadata.writer.job.as_mut().filter(|r| Some(r.id) == job) {
            r.count(&result);
            let progress = JobProgress { done: r.done, total: r.done.saturating_add(u64::try_from(active).unwrap_or(u64::MAX)) };
            let rid = r.id;
            self.jobs.progress(rid, progress, JOB_LABEL);
        }
        if self.library.photo(id).is_none() {
            return;
        }
        self.note_new_version(id, before, after);
        if let Some(s) = result.synced
            && self.library.synced(id) != Some(&s)
            && let Err(e) = self.apply_derived(Op::SetSynced { synced: vec![(id, Some(s))] })
        {
            log::warn!("synced hashes of photo {id} not applied: {e}");
        }
        self.import_from_file(&result);
        let conflicts: Vec<Field> =
            result.fields.iter().filter(|(_, o)| matches!(o, FieldOutcome::Conflict { .. })).map(|(f, _)| Field::of_sync(*f)).collect();
        if !conflicts.is_empty() {
            self.note_changed_elsewhere(id, Fields::of(&conflicts));
        }
        let failed: Vec<(Field, &str)> = result
            .fields
            .iter()
            .filter_map(|(f, o)| match o {
                FieldOutcome::Failed(why) => Some((Field::of_sync(*f), why.as_str())),
                _ => None,
            })
            .collect();
        let reason = match &result.status {
            Status::Failed(why) => Some(why.clone()),
            _ => failed.first().map(|(_, why)| (*why).to_string()),
        };
        if let Some(reason) = reason {
            let fields = failed.iter().map(|(f, _)| *f).collect();
            self.metadata_event(id, result.path.clone(), fields, reason);
        }
    }

    /// Dalica's own write changed the file the model knew: the photo takes its new size and time, so
    /// a rescan does not take the write for a change made elsewhere.
    fn note_new_version(&mut self, id: PhotoId, before: Option<Version>, after: Option<Version>) {
        let (Some(before), Some(after)) = (before, after) else { return };
        let Some(p) = self.library.photo(id) else { return };
        if (p.size, p.mtime) != before || after == before {
            return;
        }
        let mut p = p.clone();
        (p.size, p.mtime) = after;
        if let Err(e) = self.apply(Op::UpdatePhotos { photos: vec![p] }) {
            log::warn!("the new version of photo {id} not applied: {e}");
        }
    }

    /// Values another application changed (and Dalica did not): one undoable op, never written back.
    /// Places have no field in the model yet and stay in the file.
    fn import_from_file(&mut self, result: &FileResult) {
        let (Some(file), id) = (&result.file, result.photo) else { return };
        let mut ops = Vec::new();
        for field in result.imports() {
            match field {
                SyncField::Caption => ops.push(Op::SetCaptions { captions: vec![(id, file.caption.clone())] }),
                SyncField::Tags => ops.push(Op::SetTags { tags: vec![(id, file.tags.clone())] }),
                SyncField::Rating => ops.push(Op::SetStars { stars: vec![(id, file.star)] }),
                SyncField::Date => {
                    let own = self.library.photo(id).and_then(|p| p.date);
                    ops.push(Op::SetDates { dates: vec![(id, file.date.filter(|d| Some(*d) != own))] });
                }
                SyncField::Place => {}
            }
        }
        if ops.is_empty() {
            return;
        }
        let op = if ops.len() == 1 { ops.remove(0) } else { Op::Batch { ops } };
        match self.apply(op) {
            Ok(inverse) => self.undo.push(Step { label: IMPORT_LABEL, op: inverse, modes: None }),
            Err(e) => log::warn!("values photo {id}'s file holds could not be imported: {e}"),
        }
    }

    /// Records a failed write.
    fn metadata_event(&mut self, photo: PhotoId, path: PathBuf, fields: Vec<Field>, reason: String) {
        let seq = NEXT_EVENT.fetch_add(1, Ordering::Relaxed);
        log::warn!("metadata write of {} failed: {reason}", path.display());
        let events = &mut self.metadata.writer.events;
        if events.len() >= MAX_EVENTS {
            events.pop_front();
        }
        events.push_back(MetadataEvent { seq, photo, path, fields, reason });
    }

    /// The metadata writes that failed, oldest first (at most [`MAX_EVENTS`]): the UI turns the
    /// ones it has not seen (by [`MetadataEvent::seq`]) into toasts.
    pub fn metadata_events(&self) -> impl Iterator<Item = &MetadataEvent> {
        self.metadata.writer.events.iter()
    }

    /// The photos whose metadata is still to be written into their files (`library.inspect` →
    /// `pendingWrites.metadata`), waiting ones included.
    pub fn pending_metadata_writes(&self) -> usize {
        self.metadata.writer.queued.len()
    }

    /// Whether a metadata write still needs the owner's [`Session::poll`] to land (M10.54): one
    /// queued (debounced, or due) and not waiting for its volume, or the `metadataWrite` job
    /// running. The writer's job is registered and its results applied by `poll`, so an app that
    /// polls only while it has background work (`App::idle`) must count this as well as
    /// [`Session::busy`] (the scans, which the status line, the activity spinner and `ui.settle`
    /// read as such). Once the writes land it is false again; a write waiting for an offline
    /// volume is retried by the writer on its own and does not count. In memory only, O(1).
    pub fn metadata_writes_busy(&self) -> bool {
        let w = &self.metadata.writer;
        w.worker.is_some() && !w.stopped && (w.job.is_some() || w.queued.len() > w.waiting.len())
    }

    /// Writes every queued photo now, whatever the debounce says, under a job, and applies the
    /// results; `false` if `timeout` passed first (the rest is applied by later polls). Blocks the
    /// calling thread (the CLI, tests) but never touches the disk on it.
    pub fn flush_metadata(&mut self, timeout: Duration) -> bool {
        self.poll_metadata();
        if self.metadata.writer.worker.is_none() {
            return true;
        }
        if self.metadata.writer.job.is_none() && !self.metadata.writer.queued.is_empty() {
            self.start_metadata_job();
        }
        let (ack, done) = mpsc::channel();
        self.metadata.writer.send(ToWorker::Flush(ack));
        let ok = done.recv_timeout(timeout).is_ok();
        self.poll_metadata();
        ok
    }

    /// The handle a file job's worker settles a photo's queued write with before it reads the
    /// photo ([`WriteFlusher::settle`]), starting the writer if it is not running (a write queued
    /// while the job runs must go through it too). `None` when the writer cannot start: nothing
    /// is written behind the job's back then. Never touches the disk.
    pub(crate) fn metadata_flusher(&mut self) -> Option<WriteFlusher> {
        if !self.start_metadata_writer() {
            return None;
        }
        let tx = self.metadata.writer.worker.as_ref()?.tx.as_ref()?.clone();
        Some(WriteFlusher { tx })
    }

    /// The changes the metadata hook recorded since the last call, oldest first, and how many
    /// older ones were dropped to stay within [`MAX_PENDING`] photo entries.
    pub fn take_metadata_changes(&mut self) -> (Vec<MetadataChange>, u64) {
        let m = &mut self.metadata;
        m.entries = 0;
        (m.changes.drain(..).collect(), std::mem::take(&mut m.dropped))
    }
}

impl Writer {
    fn send(&mut self, msg: ToWorker) {
        let Some(tx) = self.worker.as_ref().and_then(|w| w.tx.as_ref()) else { return };
        if tx.send(msg).is_err() {
            log::error!("the metadata writer stopped");
        }
    }

    /// `photo` left the worker's queue as of message `seen`: unless a later message queued it again,
    /// it is no longer pending.
    fn done_with(&mut self, photo: PhotoId, seen: u64) {
        if self.queued.get(&photo).is_some_and(|s| *s <= seen) {
            self.queued.remove(&photo);
            self.waiting.remove(&photo);
        }
    }
}

/// The writer thread's state.
struct Work {
    queue: MetaWriteQueue,
    options: metawrite::Options,
    job: Option<(JobId, Arc<AtomicBool>)>,
    /// A job was asked for and has not come yet.
    asked: bool,
    /// The last queueing message handled.
    seen: u64,
    /// The queued photos (exactly the queue's pending ones), with their paths and the versions the
    /// model knew.
    files: BTreeMap<PhotoId, (PathBuf, Version)>,
    /// Of `files`, those waiting for their volume (as the queue last said).
    waiting: BTreeSet<PhotoId>,
    /// Photos found due, to write one by one (taken from the queue once per batch, not per file).
    ready: VecDeque<PhotoId>,
    /// The last run of Dalica's own writes of each photo: the version the file was at before the
    /// first and after the last (consecutive writes chained), at most [`MAX_OWN_WRITES`] photos.
    own: HashMap<PhotoId, (Version, Version)>,
    /// The photos of `own`, oldest first.
    own_order: VecDeque<PhotoId>,
    tx: Sender<FromWorker>,
    clock: Arc<dyn Clock>,
}

/// The writer thread: owns the queue, writes one file at a time, answers through `tx`. When the
/// session goes, it writes what is still queued and stops.
fn work(rx: Receiver<ToWorker>, tx: Sender<FromWorker>, clock: Arc<dyn Clock>) {
    let mut w = Work {
        queue: MetaWriteQueue::new(),
        options: metawrite::Options::default(),
        job: None,
        asked: false,
        seen: 0,
        files: BTreeMap::new(),
        waiting: BTreeSet::new(),
        ready: VecDeque::new(),
        own: HashMap::new(),
        own_order: VecDeque::new(),
        tx,
        clock,
    };
    loop {
        // Every message first: new requests join the batch, Cancel is seen before the next file.
        loop {
            match rx.try_recv() {
                Ok(m) => w.handle(m),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return w.close(),
            }
        }
        w.check_cancel();
        let now = w.clock.now();
        if let Some(id) = w.next(now) {
            w.write(id, now);
            continue;
        }
        w.idle_if_done();
        match rx.recv_timeout(TICK) {
            Ok(m) => w.handle(m),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return w.close(),
        }
    }
}

impl Work {
    fn handle(&mut self, msg: ToWorker) {
        match msg {
            ToWorker::Queue { seq, at, requests } => {
                self.seen = self.seen.max(seq);
                for (r, version) in requests {
                    // A new request ends a wait (the queue looks again after the debounce).
                    self.waiting.remove(&r.photo);
                    self.files.insert(r.photo, (r.path.clone(), version));
                    self.queue.enqueue(r, at);
                }
            }
            ToWorker::Job(id, cancel) => {
                self.job = Some((id, cancel));
                self.asked = false;
            }
            ToWorker::Options(o) => self.options = o,
            ToWorker::Hold(roots) => {
                // Other held roots: the queue looks at every waiting photo again.
                if roots.as_slice() != self.queue.held() {
                    self.waiting.clear();
                }
                self.queue.hold(roots);
            }
            ToWorker::Guard(roots) => self.queue.guard(roots),
            ToWorker::Flush(ack) => {
                let all: Vec<PhotoId> = self.files.keys().copied().collect();
                for id in all {
                    self.check_cancel();
                    if self.files.contains_key(&id) {
                        self.write(id, self.clock.now());
                    }
                }
                self.idle_if_done();
                let _ = ack.send(());
            }
            ToWorker::Settle { photo, path, seen, ack } => {
                if self.files.contains_key(&photo) && !self.waiting.contains(&photo) {
                    self.write(photo, self.clock.now());
                    self.idle_if_done();
                }
                let _ = ack.send(self.settled(photo, &path, seen));
            }
        }
    }

    /// The version `path` is at, when it is `seen` or a version Dalica's own writes of `photo`
    /// made from `seen`; `None` otherwise.
    fn settled(&self, photo: PhotoId, path: &std::path::Path, seen: Version) -> Option<Version> {
        let now = Work::version(path)?;
        if now == seen {
            return Some(seen);
        }
        self.own.get(&photo).filter(|(before, after)| *before == seen && *after == now).map(|(_, after)| *after)
    }

    /// Remembers that Dalica's write of `photo` took its file from `before` to `after`, chained to
    /// the last one when that ended at `before`.
    fn note_own_write(&mut self, photo: PhotoId, before: Version, after: Version) {
        let first = match self.own.get(&photo) {
            Some((first, last)) if *last == before => *first,
            _ => before,
        };
        if self.own.insert(photo, (first, after)).is_none() {
            self.own_order.push_back(photo);
            while self.own_order.len() > MAX_OWN_WRITES {
                if let Some(old) = self.own_order.pop_front() {
                    self.own.remove(&old);
                }
            }
        }
    }

    /// How many queued photos are not waiting for their volume.
    fn active(&self) -> usize {
        self.files.len().saturating_sub(self.waiting.len())
    }

    /// Forgets `photo` (it left the queue).
    fn forget(&mut self, photo: PhotoId) {
        self.files.remove(&photo);
        self.waiting.remove(&photo);
    }

    /// The next photo to write now: under the job, the next one found due; without a job, only a
    /// waiting one due to be looked at again, and a job is asked for (once) when others are due.
    fn next(&mut self, now: Timestamp) -> Option<PhotoId> {
        if self.ready.is_empty() {
            self.ready = self.queue.due(now).into();
        }
        if self.job.is_none() {
            let retry = self.ready.iter().position(|id| self.waiting.contains(id));
            if let Some(id) = retry.and_then(|i| self.ready.remove(i)) {
                return Some(id);
            }
            if !self.ready.is_empty() && !self.asked {
                self.asked = true;
                let _ = self.tx.send(FromWorker::NeedJob);
            }
            // Looked at again once the job comes.
            self.ready.clear();
            return None;
        }
        while let Some(id) = self.ready.pop_front() {
            if self.files.contains_key(&id) {
                return Some(id);
            }
        }
        None
    }

    /// The file's version now, if it is a regular file.
    fn version(path: &std::path::Path) -> Option<Version> {
        let stat = dalica_fs::stat(path).ok().flatten().filter(|s| s.is_file)?;
        Some((stat.identity.size(), Timestamp::from_system_time(stat.times.modified)))
    }

    /// Writes queued `photo` and reports it.
    fn write(&mut self, photo: PhotoId, now: Timestamp) {
        let Some((path, known)) = self.files.get(&photo).cloned() else { return };
        let found = Work::version(&path);
        let matched = found == Some(known);
        let (queue, options) = (&mut self.queue, &self.options);
        let result = catch_unwind(AssertUnwindSafe(|| queue.write(photo, options, now)));
        let job = self.job.as_ref().map(|(id, _)| *id);
        match result {
            Ok(Some(result)) => {
                let written = match &result.status {
                    Status::Written(p) if *p == path => Work::version(&path),
                    _ => None,
                };
                if let (Some(before), Some(after)) = (found, written) {
                    self.note_own_write(photo, before, after);
                }
                let after = written.filter(|_| matched);
                let pending = self.queue.is_pending(photo);
                if !pending {
                    self.forget(photo);
                } else if matches!(result.status, Status::Waiting(_)) {
                    self.waiting.insert(photo);
                } else {
                    self.waiting.remove(&photo);
                }
                let wrote = Wrote { job, result, before: matched.then_some(known), after, pending, seen: self.seen, active: self.active() };
                let _ = self.tx.send(FromWorker::Wrote(Box::new(wrote)));
            }
            Ok(None) => self.forget(photo),
            Err(_) => {
                log::error!("writing the metadata of {} panicked", path.display());
                self.queue.remove(photo);
                self.forget(photo);
                let _ = self.tx.send(FromWorker::Panicked { photo, path, seen: self.seen });
            }
        }
    }

    /// Cancel was asked: the writes not made yet leave the queue (waiting ones stay), and the job
    /// ends.
    fn check_cancel(&mut self) {
        let Some((job, cancel)) = &self.job else { return };
        if !cancel.load(Ordering::SeqCst) {
            return;
        }
        let job = *job;
        let dropped: Vec<PhotoId> = self.files.keys().filter(|id| !self.waiting.contains(id)).copied().collect();
        for id in &dropped {
            self.queue.remove(*id);
            self.forget(*id);
        }
        self.ready.clear();
        self.job = None;
        self.asked = false;
        let _ = self.tx.send(FromWorker::Cancelled { job, dropped, seen: self.seen });
    }

    /// Ends the job once nothing is left to write under it.
    fn idle_if_done(&mut self) {
        if self.job.is_some() && self.active() == 0 {
            if let Some((job, _)) = self.job.take() {
                let _ = self.tx.send(FromWorker::Idle { job });
            }
            self.asked = false;
        }
    }

    /// The session went: write what is still queued (nobody hears the results; the next session
    /// reads the files again).
    fn close(mut self) {
        let now = self.clock.now();
        let _ = catch_unwind(AssertUnwindSafe(|| self.queue.flush(&self.options, now)));
    }
}

#[cfg(test)]
mod tests {
    use dalica_core::FolderId;

    use super::*;
    use crate::cmd::testing::library;

    /// Photos of a real library, so the test never spells out the photo's fields.
    fn photos() -> (crate::cmd::testing::Dirs, Session, Vec<Photo>) {
        let (dirs, s, ids) = library(3);
        let photos = ids.iter().map(|id| s.library().photo(*id).unwrap().clone()).collect();
        (dirs, s, photos)
    }

    #[test]
    fn candidates_walk_nested_batches_and_list_each_photo_once_by_id() {
        let (_dirs, s, p) = photos();
        let id = |i: usize| p[i].id;
        let op = Op::Batch {
            ops: vec![
                Op::SetStars { stars: vec![(id(2), true), (id(0), true)] },
                Op::Batch { ops: vec![Op::UpdatePhotos { photos: vec![p[1].clone(), p[2].clone()] }, Op::Batch { ops: Vec::new() }] },
                Op::AddPhotos { photos: vec![p[0].clone()] },
                Op::RemovePhotos { ids: vec![PhotoId::new(8)] },
                Op::SetSketches { sketches: vec![(PhotoId::new(7), None)] },
                Op::RemoveFolder { id: FolderId::new(1) },
            ],
        };
        assert_eq!(candidates(s.library(), &op), [id(0), id(1), id(2)]);
        // A deep nesting is walked without recursion.
        let mut deep = Op::SetStars { stars: vec![(PhotoId::new(5), true)] };
        for _ in 0..100_000 {
            deep = Op::Batch { ops: vec![deep] };
        }
        assert_eq!(candidates(s.library(), &deep), [PhotoId::new(5)]);
        // Dropping a deeply nested op recurses; leak it so the test thread's stack is spared.
        std::mem::forget(deep);
    }

    #[test]
    fn candidates_of_every_op_on_owned_fields() {
        let (_dirs, mut s, p) = photos();
        let id = |i: usize| p[i].id;
        let lib = s.library();
        assert_eq!(candidates(lib, &Op::SetCaptions { captions: vec![(id(1), "x".into())] }), [id(1)]);
        assert_eq!(candidates(lib, &Op::AddTags { ids: vec![id(2), id(0)], tags: vec!["a".into()] }), [id(0), id(2)]);
        assert_eq!(candidates(lib, &Op::RemoveTags { ids: vec![id(1)], tags: vec!["a".into()] }), [id(1)]);
        assert_eq!(candidates(lib, &Op::SetTags { tags: vec![(id(0), Vec::new())] }), [id(0)]);
        assert_eq!(candidates(lib, &Op::SetDates { dates: vec![(id(2), None)] }), [id(2)]);
        assert_eq!(candidates(lib, &Op::SetPhotoInfo { photos: vec![(id(1), dalica_library::PhotoInfo::EMPTY)] }), [id(1)]);
        assert_eq!(candidates(lib, &Op::SetHidden { hidden: vec![(id(1), true)] }), Vec::<PhotoId>::new());
        // A rename: the photos holding the tag, any spelling.
        s.execute("photo.tags.add", &serde_json::json!({ "ids": [id(0), id(2)], "tags": ["Beach"] })).unwrap();
        assert_eq!(candidates(s.library(), &Op::RenameTag { from: "beach".into(), to: "Sea".into() }), [id(0), id(2)]);
    }

    #[test]
    fn fields_are_a_set() {
        let mut f = Fields::of(&[Field::Tags, Field::Star, Field::Tags]);
        assert!(f.contains(Field::Star) && f.contains(Field::Tags) && !f.contains(Field::Date));
        f.insert(Field::Caption);
        assert_eq!(f.iter().collect::<Vec<_>>(), [Field::Caption, Field::Tags, Field::Star]);
        assert!(Fields::NONE.is_empty() && Fields::ALL.iter().eq(Field::ALL));
        let (_dirs, s, p) = photos();
        let mut a = owned(s.library(), &p[0]);
        let b = a.clone();
        assert!(differ(&a, &b).is_empty());
        a.star = true;
        a.caption = "c".into();
        a.tags = vec!["t".into()];
        a.date = DateTime::from_unix_secs(0);
        assert_eq!(differ(&a, &b), Fields::of(&[Field::Star, Field::Caption, Field::Tags, Field::Date]));
        for f in Field::ALL {
            assert_eq!(Field::of_sync(f.sync()), f);
        }
    }

    #[test]
    fn recorded_changes_stay_within_the_cap_and_the_newest_is_kept() {
        let change =
            |n: u64| MetadataChange { origin: Origin::Command, photos: (0..n).map(|i| (PhotoId::new(i), Fields::of(&[Field::Star]))).collect() };
        let mut m = Metadata::default();
        for _ in 0..4 {
            m.record(change(3), 10);
        }
        // 12 entries over a cap of 10: the oldest change went.
        assert_eq!((m.changes.len(), m.entries, m.dropped), (3, 9, 1));
        m.record(change(50), 10);
        assert_eq!((m.changes.len(), m.entries, m.dropped), (1, 50, 4), "the newest is kept even past the cap");
    }

    #[test]
    fn an_event_names_the_file_and_the_fields() {
        let e = |fields: Vec<Field>| MetadataEvent {
            seq: 1,
            photo: PhotoId::new(1),
            path: PathBuf::from("Lisbon").join("IMG_0001.jpg"),
            fields,
            reason: "the file is read-only".into(),
        };
        assert_eq!(e(vec![Field::Caption]).message(), "Could not write the caption into IMG_0001.jpg: the file is read-only");
        assert_eq!(e(vec![Field::Caption, Field::Tags]).message(), "Could not write the caption and tags into IMG_0001.jpg: the file is read-only");
        assert_eq!(
            e(vec![Field::Caption, Field::Tags, Field::Date]).message(),
            "Could not write the caption, tags and date taken into IMG_0001.jpg: the file is read-only"
        );
        assert_eq!(e(Vec::new()).message(), "Could not write metadata into IMG_0001.jpg: the file is read-only");
    }

    /// The Save's handle (M9.28): one photo's queued write is made at once and the version Dalica's
    /// own writes left the file at comes back; nothing else is written early, a write waiting for an
    /// offline volume is not tried (and nothing waits for it), and a change made elsewhere is never
    /// explained away.
    #[test]
    fn a_flusher_settles_one_photos_write_and_never_explains_an_outside_change() {
        use dalica_core::VirtualClock;
        use dalica_fs::Expect;
        use serde_json::json;

        let patience = Duration::from_secs(120);
        let dir = tempfile::tempdir().unwrap();
        let lisbon = dir.path().join("Pictures").join("Lisbon");
        dalica_fs::create_dir_all(&lisbon).unwrap();
        for i in 0..2u64 {
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 120, 80), 80, None).unwrap();
            dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
        }
        // A clock that never moves: no write is due on its own, only what the handle asks for lands.
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000)));
        let mut s = Session::open(&dir.path().join("library"), clock).unwrap();
        s.execute("folders.add", &json!({ "path": lisbon })).unwrap();
        s.wait_idle();
        let mut photos: Vec<Photo> = s.library().photos().cloned().collect();
        photos.sort_by(|x, y| x.name.cmp(&y.name));
        let (a, b) = (photos[0].clone(), photos[1].clone());
        let (path_a, path_b) = (lisbon.join(&a.name), lisbon.join(&b.name));
        let version = |p: &std::path::Path| Work::version(p).unwrap();
        let seen_a = (a.size, a.mtime);
        assert_eq!(version(&path_a), seen_a);
        let file_b = std::fs::read(&path_b).unwrap();

        // Captions on both photos: two writes queued, neither due.
        s.execute("photo.caption.set", &json!({ "ids": [a.id, b.id], "caption": "Tram 28" })).unwrap();
        assert_eq!(s.pending_metadata_writes(), 2);
        let flusher = s.metadata_flusher().unwrap();
        let settled = flusher.settle(a.id, path_a.clone(), seen_a, patience).unwrap();
        assert_ne!(settled, seen_a, "the caption was written into the file");
        assert_eq!(settled, version(&path_a));
        let read = dalica_meta::read(&std::fs::read(&path_a).unwrap(), &dalica_meta::Limits::default()).unwrap();
        assert_eq!(read.owned.caption.as_deref(), Some("Tram 28"));
        assert_eq!(std::fs::read(&path_b).unwrap(), file_b, "only that photo's write was made");
        // Settled again (the write already made): the same version; from it, itself.
        assert_eq!(flusher.settle(a.id, path_a.clone(), seen_a, patience), Some(settled));
        assert_eq!(flusher.settle(a.id, path_a.clone(), settled, patience), Some(settled));
        s.poll_metadata();
        assert_eq!(s.pending_metadata_writes(), 1, "b's write still waits for its debounce");
        assert_eq!((s.library().photo(a.id).unwrap().size, s.library().photo(a.id).unwrap().mtime), settled, "the model took the new version");

        // A change made elsewhere is not Dalica's: no version explains it.
        let other = dalica_scenes::jpeg(&dalica_scenes::Scene::new(9, 160, 90), 80, None).unwrap();
        let identity = dalica_fs::read(&path_a, u64::MAX).unwrap().unwrap().identity;
        dalica_fs::replace(&path_a, &other, Expect::Unchanged(identity)).unwrap();
        assert_eq!(flusher.settle(a.id, path_a.clone(), settled, patience), None);
        assert_eq!(flusher.settle(a.id, path_a.clone(), seen_a, patience), None);
        assert_eq!(flusher.settle(a.id, lisbon.join("gone.jpg"), seen_a, patience), None, "a missing file");

        // A write waiting for an offline volume is not tried: the answer comes at once, b unchanged.
        s.metadata_hold(vec![lisbon.clone()]);
        let seen_b = (b.size, b.mtime);
        assert_eq!(flusher.settle(b.id, path_b.clone(), seen_b, patience), Some(seen_b));
        assert_eq!(std::fs::read(&path_b).unwrap(), file_b);
    }

    #[test]
    fn raw_names_and_options() {
        assert!(maybe_raw("IMG_0001.CR3") && maybe_raw("a.nef") && maybe_raw("x.DNG"));
        assert!(!maybe_raw("IMG_0001.JPG") && !maybe_raw("a.tiff") && !maybe_raw("noext") && !maybe_raw(".hidden"));
        assert_eq!(stem_key("IMG_0001.CR3"), "img_0001");
        let o = queue_options(&MetadataWrites {
            captions_tags_places: false,
            stars_as_ratings: true,
            update_modification_time: true,
            raw_xmp_sidecars: true,
            dates: false,
        });
        assert!(!o.captions_tags_places && o.stars_as_ratings && o.update_modification_time && o.raw_xmp_sidecars && !o.dates);
        let off = MetadataWrites::default();
        assert!(Field::Caption.allowed(&off) && Field::Date.allowed(&off) && !Field::Star.allowed(&off));
    }
}
