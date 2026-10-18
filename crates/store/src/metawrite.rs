//! The metadata write queue (`data-model.md` §5, `xmp-interop.md` §1, §5, §7): captions, tags,
//! stars, places, dates and people names written into the photo files themselves, so other
//! applications see them, without ever harming a file.
//!
//! The engine queues a [`Request`] for every photo whose owned field an op changed (undo and redo
//! included). Requests are batched per photo (the fields of several ops go out in one write, with
//! the latest values) and debounced: a photo is written once it has been quiet for [`DEBOUNCE`].
//! The writes run on whichever thread drives the queue (the engine's job worker; never the UI
//! thread), one file at a time ([`MetaWriteQueue::write`], or [`MetaWriteQueue::write_due`] /
//! [`MetaWriteQueue::flush`]); each gives a [`FileResult`] for the engine: written, imported,
//! conflict, failed with its reason, skipped.
//!
//! One write ([`write_file`]):
//!
//! 1. **Re-read** the file through `fs` (its identity, times and whether it may be written) and its
//!    metadata through `meta`.
//! 2. **Decide per field** with the rules of `xmp-interop.md` §5 (`dalica_library::sync`), from the
//!    file's value, Dalica's value and the last-synced hash: write, import, keep, or both changed
//!    (reported, Dalica's value kept). Only the fields an op changed are written, each only when
//!    its option is on; a value changed elsewhere comes back as an import, never overwritten.
//! 3. **Merge-write** through `meta` (`write_owned`, which merges into the existing metadata and
//!    verifies the result: owned fields read back, every other byte unchanged, pixels untouched).
//! 4. **Replace atomically** through `fs` (temporary file, `fsync`, rename, read back), refusing a
//!    file that changed since it was read: the request is re-queued and the file re-read next time.
//! 5. **Restore the modification time** (and access time) unless "Update file modification time on
//!    metadata changes" is on.
//! 6. **Hand back the synced hashes**: the hashes of the file's values as now written, for the
//!    engine's `SetSynced`.
//!
//! **RAW files** are never written: with "Write XMP sidecars for RAW files" on, their fields go into
//! the `.xmp` sidecar `meta` names (`xmp_sidecar`); a damaged one is kept aside as
//! `<name>.xmp.corrupt-<time>` and replaced. **Formats Dalica cannot write** (GIF, BMP, HEIC,
//! video …) are skipped with the reason, and **read-only files** fail with it: either way the folder
//! sidecar keeps the values. **Offline volumes:** a file under a held root ([`MetaWriteQueue::hold`])
//! or one whose guarded root fails its check right before the write ([`MetaWriteQueue::guard`],
//! M6.43), or whose folder is not there, waits and is written when its volume returns.
//!
//! **People names** (M13.19, `xmp-interop.md` §6) ride in a request of their own kind
//! ([`PeopleWrite`]): the photo's named faces as MWG face regions, written while "Store people
//! names in photos" is on ([`Options::people_names`]) or for Write People Names to Photos…
//! ([`PeopleWrite::always`]). The library keeps no last-synced hash of them, so what is new in a
//! file is told by overlap instead: a face entry of the file that overlaps (IoU ≥ 0.5) none of the
//! rectangles Dalica knows on the photo ([`PeopleWrite::known`]: its faces now and before the
//! changes being written, named or not) came from another application since Dalica last read it.
//! Such a file is not given Dalica's faces: the people outcome is an import ([`FileResult::faces`]
//! holds the file's), the engine brings the new faces in and writes again, so a write never removes
//! a face another application added (`xmp-interop.md` §1 principle 3).
//!
//! The queue takes the time from its caller (`now`, the session's clock) and reads no clock: it
//! debounces with it and writes it as `xmp:MetadataDate`.

mod file;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::{PhotoId, Rect01, Timestamp};
use dalica_fs::Times;
use dalica_library::Synced;
pub use dalica_library::sync::{Field, Values};
pub use dalica_meta::fields::FaceRegion;
pub use dalica_meta::write::{NamedFace, NamedFaces};

pub use self::file::write_file;
use crate::writer::RootCheck;

/// How long a photo must be quiet (no new request) before it is written, so typing a caption or
/// tagging in bursts writes the file once.
pub const DEBOUNCE: Duration = Duration::from_secs(1);
/// How long a waiting write (its volume offline) waits before the queue looks again on its own.
/// A new request, or a change of the held roots, ends the wait.
pub const WAIT_RETRY: Duration = Duration::from_secs(10);
/// How often a write that failed on the disk (an I/O error, a write cut short) is tried before
/// it fails for good.
pub const MAX_TRIES: u32 = 3;
/// The largest photo file the queue rewrites (it is held in memory while it is merged).
pub const MAX_FILE: u64 = 1 << 30;

/// One photo to write: which fields an op changed, and Dalica's values for all of them now.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub photo: PhotoId,
    /// The photo file.
    pub path: PathBuf,
    /// The fields to write (those an op changed). The others are only read (and imported when
    /// changed elsewhere).
    pub fields: BTreeSet<Field>,
    /// Dalica's values (the model's: the folder sidecar's), every field; the date is the photo's
    /// effective date.
    pub values: Values,
    /// The photo's last-synced hashes as the model has them (`None`: never synced).
    pub synced: Option<Synced>,
    /// Set only by a scan that found the photo without an entry in its folder sidecar (rule 1):
    /// every value the file has is imported, nothing is written.
    pub first_scan: bool,
    /// The names of the other RAW photos of the photo's folder, as the scan recognised them (so two
    /// RAWs sharing a stem never share an `.xmp` sidecar). The photo's own format is read from
    /// the file.
    pub raws: Vec<String>,
    /// The people names to write (`None`: they are only read).
    pub people: Option<PeopleWrite>,
}

/// The people names of one photo to write as MWG face regions (`xmp-interop.md` §6).
#[derive(Clone, Debug, PartialEq)]
pub struct PeopleWrite {
    /// The photo's named faces now, with its upright pixel size (unnamed and ignored faces are
    /// never written).
    pub faces: NamedFaces,
    /// Every face rectangle Dalica knows on the photo, named or not, now and before the changes
    /// being written: a face entry of the file overlapping none of them is new from another
    /// application, and comes in before anything is written over it.
    pub known: Vec<Rect01>,
    /// Written whatever "Store people names in photos" says (Write People Names to Photos…).
    pub always: bool,
}

impl PeopleWrite {
    /// The file's face entries that overlap none of the rectangles Dalica knows (IoU below
    /// [`NEW_FACE_IOU`]).
    pub fn new_in<'a>(&self, file: &'a [FaceRegion]) -> Vec<&'a FaceRegion> {
        new_faces(file, &self.known)
    }

    /// This write, joined with a later one of the same photo: the later faces, every rectangle
    /// either knew, and written always if either is.
    fn join(self, later: PeopleWrite) -> PeopleWrite {
        let mut known = self.known;
        for r in later.known {
            if !known.contains(&r) {
                known.push(r);
            }
        }
        PeopleWrite { faces: later.faces, known, always: self.always || later.always }
    }
}

/// How much a face entry of a file must overlap a face Dalica knows to be that face (intersection
/// over union, as `xmp-interop.md` §6 matches entries).
pub const NEW_FACE_IOU: f64 = 0.5;

/// The faces of `file` that overlap none of `known` (IoU below [`NEW_FACE_IOU`]): new from another
/// application.
pub fn new_faces<'a>(file: &'a [FaceRegion], known: &[Rect01]) -> Vec<&'a FaceRegion> {
    file.iter().filter(|f| !known.iter().any(|k| dalica_meta::xmp::regions::iou(&f.rect, k) >= NEW_FACE_IOU)).collect()
}

impl Request {
    /// A request to write `fields` of `photo` at `path`, with Dalica's `values`.
    pub fn new(photo: PhotoId, path: PathBuf, fields: impl IntoIterator<Item = Field>, values: Values) -> Request {
        Request { photo, path, fields: fields.into_iter().collect(), values, synced: None, first_scan: false, raws: Vec::new(), people: None }
    }
}

/// The write options of Options → General → Files (`xmp-interop.md` §2) and the limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Write captions, tags and places into photo files (on).
    pub captions_tags_places: bool,
    /// Write stars as ratings (off).
    pub stars_as_ratings: bool,
    /// Write date changes to files (on).
    pub dates: bool,
    /// Write XMP sidecars for RAW files (off).
    pub raw_xmp_sidecars: bool,
    /// Update file modification time on metadata changes (off: the time is put back).
    pub update_modification_time: bool,
    /// Store people names in photos (Options → People, off): named faces as MWG face regions.
    pub people_names: bool,
    /// Metadata parse limits (`security.md` §4.2).
    pub limits: dalica_meta::Limits,
    /// The largest file rewritten.
    pub max_file: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            captions_tags_places: true,
            stars_as_ratings: false,
            dates: true,
            raw_xmp_sidecars: false,
            update_modification_time: false,
            people_names: false,
            limits: dalica_meta::Limits::default(),
            max_file: MAX_FILE,
        }
    }
}

impl Options {
    /// Whether `field` may be written.
    pub fn allows(&self, field: Field) -> bool {
        match field {
            Field::Caption | Field::Tags | Field::Place => self.captions_tags_places,
            Field::Rating => self.stars_as_ratings,
            Field::Date => self.dates,
        }
    }
}

/// What happened to one field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldOutcome {
    /// Dalica's value is now in the file.
    Written,
    /// The file already held Dalica's value.
    Unchanged,
    /// Another application changed it and Dalica's value did not change since: the file's value
    /// ([`FileResult::file`]) is to be imported (one undoable op), nothing was written back.
    Imported,
    /// Both changed since the last sync: Dalica's value is kept (and was written when `written`);
    /// the file's value before the write is in [`FileResult::file`].
    Conflict { written: bool },
    /// Not written, and why (its option is off, the format cannot hold it …). The sidecar keeps it.
    Skipped(String),
    /// The write failed, and why. The file is as it was; the sidecar keeps the value.
    Failed(String),
}

/// What happened to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The photo, or its RAW's `.xmp` sidecar (the path), was replaced or created.
    Written(PathBuf),
    /// Nothing needed writing (imports may still have come back).
    Unchanged,
    /// Nothing can be written into this file (its format, a RAW without the sidecar option): the
    /// folder sidecar keeps the values.
    Skipped(String),
    /// The write failed (a read-only file, metadata that cannot be read, a field the format cannot
    /// take …): the file is as it was, the folder sidecar keeps the values.
    Failed(String),
    /// Its volume is offline (a held root, a guarded root failing its check, its folder not
    /// there): still queued, written on return.
    Waiting(String),
    /// Not done yet (the file changed between read and write, a disk error to retry, the time not
    /// put back yet): still queued, the file re-read on the next pass.
    Requeued(String),
    /// The file is not there (its folder is): nothing to write.
    Gone,
}

/// The result of writing one photo, for the engine.
#[derive(Clone, Debug, PartialEq)]
pub struct FileResult {
    pub photo: PhotoId,
    pub path: PathBuf,
    pub status: Status,
    /// What happened to each field written, imported or in conflict, in [`Field::ALL`] order.
    pub fields: Vec<(Field, FieldOutcome)>,
    /// The file's values as read before the write (the values to import, the other side of a
    /// conflict); `None` when it was not read.
    pub file: Option<Values>,
    /// The new last-synced hashes (of the file's values as now on disk), for `SetSynced`; `None`
    /// when the file was not read.
    pub synced: Option<Synced>,
    /// A damaged `.xmp` sidecar kept aside under this name.
    pub set_aside: Option<PathBuf>,
    /// Something to know that is not a failure (the modification time could not be put back).
    pub note: Option<String>,
    /// What happened to the people names, when the request asked to write them: written,
    /// unchanged, skipped (the option off), failed, or `Imported` (the file holds faces new from
    /// another application: nothing was written over them, [`FileResult::faces`] holds them).
    pub people: Option<FieldOutcome>,
    /// The file's face regions as read before the write (with a people outcome).
    pub faces: Option<Vec<FaceRegion>>,
    /// The request's [`PeopleWrite::always`], for a write made again after an import.
    pub people_always: bool,
    /// Why a re-queued write is tried again, for the queue's count.
    pub(crate) retry: Option<Retry>,
}

/// Why a write is re-queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Retry {
    /// The file changed between read and write: always read again.
    Changed,
    /// A disk error (or a write cut short): tried [`MAX_TRIES`] times, then failed.
    Disk,
    /// The file is written, its time is not back yet: tried [`MAX_TRIES`] times, then given up
    /// with a note.
    Time,
}

impl FileResult {
    pub(crate) fn new(request: &Request, status: Status) -> FileResult {
        FileResult {
            photo: request.photo,
            path: request.path.clone(),
            status,
            fields: Vec::new(),
            file: None,
            synced: None,
            set_aside: None,
            note: None,
            people: None,
            faces: None,
            people_always: request.people.as_ref().is_some_and(|p| p.always),
            retry: None,
        }
    }

    /// The outcome of `field`, if it was written, imported or in conflict.
    pub fn field(&self, field: Field) -> Option<&FieldOutcome> {
        self.fields.iter().find(|(f, _)| *f == field).map(|(_, o)| o)
    }

    /// The fields whose file value is to be imported.
    pub fn imports(&self) -> impl Iterator<Item = Field> + '_ {
        self.fields.iter().filter(|(_, o)| *o == FieldOutcome::Imported).map(|(f, _)| *f)
    }
}

/// A queued photo.
#[derive(Clone, Debug)]
struct Pending {
    request: Request,
    /// When it was last asked for (or re-queued): due [`DEBOUNCE`] later.
    since: Timestamp,
    /// When the queue last found its volume offline: looked at again [`WAIT_RETRY`] later.
    waiting: Option<Timestamp>,
    /// Failed tries on the disk so far.
    tries: u32,
}

/// The queue of photos whose metadata is to be written.
#[derive(Default)]
pub struct MetaWriteQueue {
    pending: BTreeMap<PhotoId, Pending>,
    /// The synced hashes this queue produced last, until the model's catch up: a request queued
    /// before the engine applied them still carries the old ones.
    synced: HashMap<PhotoId, Synced>,
    /// The times a file had before a write this queue started and has not finished (the time not
    /// put back yet), so a retry restores them.
    times: HashMap<PhotoId, Times>,
    /// Roots found offline: nothing under them is written.
    held: Vec<PathBuf>,
    /// Roots checked right before each write under them.
    guards: Vec<(PathBuf, RootCheck)>,
}

impl std::fmt::Debug for MetaWriteQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaWriteQueue")
            .field("pending", &self.pending.len())
            .field("held", &self.held)
            .field("guarded", &self.guards.iter().map(|(r, _)| r).collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl MetaWriteQueue {
    pub fn new() -> MetaWriteQueue {
        MetaWriteQueue::default()
    }

    /// Queues `request`. A photo already queued is written once: the fields add up, and the
    /// latest values, path, synced hashes and folder RAWs replace the earlier ones. The photo is
    /// due [`DEBOUNCE`] after this call; a waiting photo is looked at again then.
    pub fn enqueue(&mut self, request: Request, now: Timestamp) {
        let photo = request.photo;
        if request.synced.is_some() && self.synced.get(&photo) == request.synced.as_ref() {
            // The model has caught up with what this queue wrote.
            self.synced.remove(&photo);
        }
        match self.pending.get_mut(&photo) {
            Some(p) => {
                let mut fields = std::mem::take(&mut p.request.fields);
                fields.extend(request.fields.iter().copied());
                let people = match (p.request.people.take(), request.people.clone()) {
                    (Some(a), Some(b)) => Some(a.join(b)),
                    (a, b) => b.or(a),
                };
                p.request = Request { fields, people, ..request };
                p.since = now;
                p.waiting = None;
                p.tries = 0;
            }
            None => {
                self.pending.insert(photo, Pending { request, since: now, waiting: None, tries: 0 });
            }
        }
    }

    /// The engine set `photo`'s synced hashes from elsewhere (a scan read the file): they replace
    /// what this queue remembers.
    pub fn note_synced(&mut self, photo: PhotoId, synced: Option<Synced>) {
        match synced {
            Some(s) => self.synced.insert(photo, s),
            None => self.synced.remove(&photo),
        };
    }

    /// Forgets `photo` (it left the library).
    pub fn remove(&mut self, photo: PhotoId) {
        self.pending.remove(&photo);
        self.synced.remove(&photo);
        self.times.remove(&photo);
    }

    /// How many photos are queued (waiting ones included).
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    pub fn is_pending(&self, photo: PhotoId) -> bool {
        self.pending.contains_key(&photo)
    }

    /// The queued photos waiting for their volume, in id order.
    pub fn waiting(&self) -> impl Iterator<Item = PhotoId> + '_ {
        self.pending.iter().filter(|(_, p)| p.waiting.is_some()).map(|(id, _)| *id)
    }

    /// The roots found offline (replacing the earlier list): files under them wait. When the list
    /// changes, waiting photos are looked at again at once.
    pub fn hold(&mut self, roots: Vec<PathBuf>) {
        if roots != self.held {
            self.held = roots;
            for p in self.pending.values_mut() {
                p.waiting = None;
            }
        }
    }

    /// The roots held.
    pub fn held(&self) -> &[PathBuf] {
        &self.held
    }

    /// The roots to check right before each write under them, with their checks (replacing the
    /// earlier list; the sidecar writer's [`RootCheck`]s). A file under a root whose check fails
    /// waits.
    pub fn guard(&mut self, roots: Vec<(PathBuf, RootCheck)>) {
        self.guards = roots;
    }

    /// The queued photos due at `now`, in id order: quiet for [`DEBOUNCE`], or for a waiting one,
    /// [`WAIT_RETRY`] since the queue last looked.
    pub fn due(&self, now: Timestamp) -> Vec<PhotoId> {
        self.pending
            .iter()
            .filter(|(_, p)| match p.waiting {
                Some(t) => now.since(t) >= WAIT_RETRY,
                None => now.since(p.since) >= DEBOUNCE,
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Writes every photo due at `now`.
    pub fn write_due(&mut self, options: &Options, now: Timestamp) -> Vec<FileResult> {
        self.due(now).into_iter().filter_map(|id| self.write(id, options, now)).collect()
    }

    /// Writes every queued photo now, whatever the debounce says (waiting ones are looked at
    /// again).
    pub fn flush(&mut self, options: &Options, now: Timestamp) -> Vec<FileResult> {
        let all: Vec<PhotoId> = self.pending.keys().copied().collect();
        all.into_iter().filter_map(|id| self.write(id, options, now)).collect()
    }

    /// Writes queued `photo` now (whatever the debounce says); `None` if it is not queued. The
    /// engine's job calls it photo by photo, for progress and Cancel.
    pub fn write(&mut self, photo: PhotoId, options: &Options, now: Timestamp) -> Option<FileResult> {
        let pending = self.pending.get(&photo)?;
        let mut request = pending.request.clone();
        if let Some(s) = self.synced.get(&photo) {
            request.synced = Some(*s);
        }
        let remembered = self.times.get(&photo).copied();
        if let Some(root) = self.held.iter().find(|r| request.path.starts_with(r)) {
            let why = format!("{} is offline; the write waits for it", root.display());
            return Some(self.finish(FileResult::new(&request, Status::Waiting(why)), remembered, options, now));
        }
        if let Some(root) = self.failed_root(&request.path) {
            let why =
                format!("{} is not the volume the library knows (unmounted, or another one in its place); the write waits for it", root.display());
            return Some(self.finish(FileResult::new(&request, Status::Waiting(why)), remembered, options, now));
        }
        let (result, times) = file::run(&request, options, now, remembered);
        Some(self.finish(result, times, options, now))
    }

    /// The first guarded root above `path` whose check fails (a panicking check fails).
    fn failed_root(&self, path: &Path) -> Option<PathBuf> {
        self.guards
            .iter()
            .filter(|(root, _)| path.starts_with(root))
            .find(|(root, check)| !catch_unwind(AssertUnwindSafe(|| check(root))).unwrap_or(false))
            .map(|(r, _)| r.clone())
    }

    /// Updates the queue after one write: what stays queued, the synced hashes and times it keeps.
    fn finish(&mut self, mut result: FileResult, times: Option<Times>, options: &Options, now: Timestamp) -> FileResult {
        let photo = result.photo;
        if let Some(s) = result.synced {
            self.synced.insert(photo, s);
        }
        match times.filter(|_| !options.update_modification_time) {
            Some(t) => self.times.insert(photo, t),
            None => self.times.remove(&photo),
        };
        let Some(p) = self.pending.get_mut(&photo) else { return result };
        match &result.status {
            Status::Waiting(_) => p.waiting = Some(now),
            Status::Requeued(why) => {
                p.waiting = None;
                p.since = now;
                // A file that changed under the write is always read again; a disk error, or a
                // time not put back, a few times.
                if matches!(result.retry, Some(Retry::Disk | Retry::Time)) {
                    p.tries = p.tries.saturating_add(1);
                }
                if p.tries >= MAX_TRIES {
                    let why = format!("{why} (tried {MAX_TRIES} times)");
                    if result.retry == Some(Retry::Time) {
                        result.status = Status::Written(result.path.clone());
                        result.note = Some(why);
                    } else {
                        result.status = Status::Failed(why);
                    }
                    self.pending.remove(&photo);
                    self.times.remove(&photo);
                }
            }
            _ => {
                self.pending.remove(&photo);
            }
        }
        result
    }
}
