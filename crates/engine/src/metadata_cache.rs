//! The photo metadata cache behind `photo.metadata` (`cmd/info.rs`, the Properties pane of
//! `ui-layout.md` §7): what a photo's file says about itself (the owned fields, the EXIF summary and
//! the Show All Metadata listing), read through `dalica-meta` on worker threads.
//!
//! The thread that owns the session never reads a file. A lookup answers from the session's cache
//! ([`MetadataCache`], a field of the `Session`), or queues the file and answers
//! [`Lookup::Pending`]; up to [`WORKERS`] "dalica-metadata" threads read the queue newest first
//! (the photo the user is looking at now comes before the ones they arrowed past), and the next
//! lookup finds the answer. [`Session::poll`] counts the answers that arrived since the last frame
//! (so the UI knows to draw again); [`Session::wait_photo_metadata`] blocks until the readers are
//! idle (CLI, tests).
//!
//! Entries are keyed by one version of a file: its path with the size and modification time the
//! library knows, so a file changed on disk is read again once a scan has seen the change. Each
//! session has its own cache, bounded: at most [`MAX_ENTRIES`] answers and about [`MAX_BYTES`] of
//! text, the least recently used going first; at most [`MAX_QUEUE`] files wait, the oldest request
//! dropped first (it is queued again when asked again). A file that could not be read (missing,
//! too large, unreadable) is answered as failed for [`RETRY`], then read again. When the session
//! closes, the files still waiting are dropped and the readers stop after the file in hand.
//!
//! The "Changed in another application; Dalica's value kept" note (`xmp-interop.md` §5 rule 3) is
//! kept here too, against the file version it was found in ([`Session::note_changed_elsewhere`],
//! for the scan's import and the write-back): a later version of the file (Dalica's own write of
//! its value, or another edit) ends it.

use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, Timestamp};
use dalica_meta::{Limits, Listing, OwnedFields, exif};

use crate::metadata::Fields;
use crate::session::Session;

/// The most answers kept.
pub const MAX_ENTRIES: usize = 256;
/// About the most bytes of text the kept answers hold; the newest is kept even past it.
pub const MAX_BYTES: usize = 64 << 20;
/// The most files waiting to be read; past it the oldest request is dropped.
pub const MAX_QUEUE: usize = 64;
/// Reader threads at most, per session.
pub const WORKERS: usize = 2;
/// The largest file read (the thumbnailer's cap): its metadata may sit anywhere in it.
pub const MAX_FILE: u64 = 1 << 30;
/// How long a failed read is answered as failed before the file is read again.
pub const RETRY: Duration = Duration::from_secs(10);
/// The most "changed in another application" notes kept.
pub const MAX_NOTES: usize = 100_000;

/// One version of a photo file, as the library knows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileKey {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: Timestamp,
}

/// What a file says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileMetadata {
    /// The owned fields by the precedence rules (`xmp-interop.md` §2).
    pub owned: OwnedFields,
    /// Camera, lens, exposure and the other read-only facts of the Properties pane.
    pub summary: exif::Summary,
    /// Every field, for Show All Metadata.
    pub listing: Listing,
    /// Why some blocks, or the whole file, could not be read (a file whose format `meta` does not
    /// read has no fields and says so here).
    pub problems: Vec<String>,
}

/// A lookup's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    /// Queued or being read: ask again.
    Pending,
    Ready(Arc<FileMetadata>),
    /// The file could not be read (until [`RETRY`] has passed).
    Failed(String),
}

enum Slot {
    Queued,
    Reading,
    /// The answer and its size in bytes (for [`MAX_BYTES`]).
    Ready(Arc<FileMetadata>, usize),
    Failed(String, Instant),
}

/// What the session's thread and its readers share.
#[derive(Default)]
struct Cache {
    slots: HashMap<FileKey, Slot>,
    /// Files waiting, the newest request at the back (read first).
    queue: VecDeque<FileKey>,
    /// Answered files, the most recently used at the back.
    lru: VecDeque<FileKey>,
    bytes: usize,
    /// Reader threads running.
    workers: usize,
    /// Files being read now.
    reading: usize,
    /// Answers that arrived since the last [`MetadataCache::take_arrived`].
    arrived: usize,
    /// The session closed: readers stop and their answers are dropped.
    closed: bool,
}

#[derive(Default)]
struct Shared {
    cache: Mutex<Cache>,
    /// Signalled when a reader answers or stops.
    idle: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One session's cache, its readers and its notes (the `metadata_cache` field of the `Session`).
#[derive(Default)]
pub struct MetadataCache {
    shared: Arc<Shared>,
    /// Fields changed in another application, by path, with the file version they were found in.
    notes: HashMap<PathBuf, (u64, Timestamp, Fields)>,
}

impl Drop for MetadataCache {
    /// Drops the files still waiting; a reader finishes the file in hand and stops.
    fn drop(&mut self) {
        let mut c = self.shared.lock();
        c.closed = true;
        while let Some(key) = c.queue.pop_front() {
            c.slots.remove(&key);
        }
    }
}

impl Cache {
    /// Queues `key` as the newest request, dropping the oldest past [`MAX_QUEUE`].
    fn enqueue(&mut self, key: &FileKey) {
        self.queue.retain(|k| k != key);
        self.queue.push_back(key.clone());
        self.slots.insert(key.clone(), Slot::Queued);
        while self.queue.len() > MAX_QUEUE {
            if let Some(old) = self.queue.pop_front() {
                self.slots.remove(&old);
            }
        }
    }

    /// Marks `key` as the most recently used answer.
    fn touch(&mut self, key: &FileKey) {
        if let Some(i) = self.lru.iter().position(|k| k == key)
            && let Some(k) = self.lru.remove(i)
        {
            self.lru.push_back(k);
        }
    }

    /// Forgets the answer for `key`.
    fn forget(&mut self, key: &FileKey) {
        if let Some(Slot::Ready(_, n)) = self.slots.remove(key) {
            self.bytes = self.bytes.saturating_sub(n);
        }
        self.lru.retain(|k| k != key);
    }

    /// Keeps the answer a reader brought, then drops the least recently used past the bounds.
    fn answer(&mut self, key: FileKey, slot: Slot) {
        self.reading = self.reading.saturating_sub(1);
        if self.closed {
            return;
        }
        self.arrived = self.arrived.saturating_add(1);
        // Dropped from the queue while it was read: keep the answer anyway, it is fresh.
        self.forget(&key);
        if let Slot::Ready(_, n) = &slot {
            self.bytes = self.bytes.saturating_add(*n);
        }
        self.slots.insert(key.clone(), slot);
        self.lru.push_back(key);
        while (self.lru.len() > MAX_ENTRIES || self.bytes > MAX_BYTES) && self.lru.len() > 1 {
            if let Some(old) = self.lru.front().cloned() {
                self.forget(&old);
            }
        }
    }
}

impl MetadataCache {
    /// The answer for `key`, queuing it (and starting a reader) when there is none yet.
    pub fn lookup(&self, key: &FileKey) -> Lookup {
        let mut c = self.shared.lock();
        let found = match c.slots.get(key) {
            Some(Slot::Ready(m, _)) => Some(Lookup::Ready(Arc::clone(m))),
            Some(Slot::Failed(e, at)) if at.elapsed() < RETRY => Some(Lookup::Failed(e.clone())),
            Some(Slot::Reading) => return Lookup::Pending,
            Some(Slot::Queued | Slot::Failed(..)) | None => None,
        };
        if let Some(answer) = found {
            c.touch(key);
            return answer;
        }
        c.forget(key);
        c.enqueue(key);
        self.start_workers(&mut c);
        Lookup::Pending
    }

    /// Starts readers while fewer than [`WORKERS`] run and files wait. A reader that cannot be
    /// started fails the files waiting (they are tried again after [`RETRY`]).
    fn start_workers(&self, c: &mut Cache) {
        while c.workers < WORKERS && c.workers < c.queue.len() {
            let shared = Arc::clone(&self.shared);
            match std::thread::Builder::new().name("dalica-metadata".into()).spawn(move || work(&shared)) {
                Ok(_) => c.workers = c.workers.saturating_add(1),
                Err(e) => {
                    if c.workers == 0 {
                        let message = format!("cannot start a metadata reader: {e}");
                        while let Some(key) = c.queue.pop_front() {
                            c.reading = c.reading.saturating_add(1);
                            c.answer(key, Slot::Failed(message.clone(), Instant::now()));
                        }
                    }
                    return;
                }
            }
        }
    }

    /// The note for `key`'s file: the fields changed in another application in this version of it.
    pub fn note(&self, key: &FileKey) -> Fields {
        match self.notes.get(&key.path) {
            Some((size, mtime, fields)) if (*size, *mtime) == (key.size, key.mtime) => *fields,
            _ => Fields::NONE,
        }
    }

    /// Files queued or being read now.
    pub fn pending(&self) -> usize {
        let c = self.shared.lock();
        c.queue.len().saturating_add(c.reading)
    }

    /// How many answers arrived since the last call.
    pub fn take_arrived(&self) -> usize {
        std::mem::take(&mut self.shared.lock().arrived)
    }

    /// Blocks until no file is queued or being read, or `timeout` passed; `true` when idle.
    pub fn wait(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        let mut c = self.shared.lock();
        loop {
            if c.queue.is_empty() && c.reading == 0 {
                return true;
            }
            let Some(left) = timeout.checked_sub(start.elapsed()) else { return false };
            c = self.shared.idle.wait_timeout(c, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
    }
}

/// A reader thread: reads the newest waiting file until none waits (or the session closed).
fn work(shared: &Shared) {
    loop {
        let key = {
            let mut c = shared.lock();
            match c.queue.pop_back().filter(|_| !c.closed) {
                Some(key) => {
                    c.slots.insert(key.clone(), Slot::Reading);
                    c.reading = c.reading.saturating_add(1);
                    key
                }
                None => {
                    c.workers = c.workers.saturating_sub(1);
                    shared.idle.notify_all();
                    return;
                }
            }
        };
        #[cfg(feature = "faults")]
        planted::reach(&key.path);
        let slot = match catch_unwind(AssertUnwindSafe(|| read_file(&key.path))) {
            Ok(Ok(m)) => {
                let n = size_of(&m);
                Slot::Ready(Arc::new(m), n)
            }
            Ok(Err(e)) => Slot::Failed(e, Instant::now()),
            Err(_) => {
                log::error!("reading the metadata of a photo panicked");
                Slot::Failed("the file's metadata could not be read".into(), Instant::now())
            }
        };
        shared.lock().answer(key, slot);
        shared.idle.notify_all();
    }
}

/// Planted holds for tests (the `faults` feature): a reader that takes a held file marks it as
/// being read, then waits before reading it until the hold is released, so a test sees a read in
/// flight for as long as it needs instead of hoping a big file is slow.
#[cfg(feature = "faults")]
pub mod planted {
    use std::path::{Path, PathBuf};
    use std::sync::{Condvar, Mutex, PoisonError};

    static HELD: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static RELEASED: Condvar = Condvar::new();

    /// A hold on the reads of one file; dropping it releases them.
    #[must_use = "the reads are released when the hold is dropped"]
    #[derive(Debug)]
    pub struct Hold(PathBuf);

    /// Holds every read of the file at `path` by a metadata reader until the returned [`Hold`] is
    /// dropped.
    pub fn hold_reads_of(path: &Path) -> Hold {
        HELD.lock().unwrap_or_else(PoisonError::into_inner).push(path.to_path_buf());
        Hold(path.to_path_buf())
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(i) = held.iter().position(|p| *p == self.0) {
                held.remove(i);
            }
            RELEASED.notify_all();
        }
    }

    /// Waits while the file at `path` is held (on a reader thread).
    pub(super) fn reach(path: &Path) {
        let held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        let _released = RELEASED.wait_while(held, |held| held.iter().any(|p| p == path)).unwrap_or_else(PoisonError::into_inner);
    }
}

/// Reads one file's metadata (on a reader thread). `Err` when the file itself cannot be read; a
/// file whose metadata `meta` cannot walk is an answer with no fields and the reason in
/// [`FileMetadata::problems`].
pub fn read_file(path: &Path) -> Result<FileMetadata, String> {
    let read = match dalica_fs::read(path, MAX_FILE) {
        Ok(Some(r)) => r,
        Ok(None) => return Err("the file is missing".into()),
        Err(e) => return Err(e.to_string()),
    };
    Ok(parse(&read.bytes))
}

/// What `bytes` (a whole photo file) say about themselves.
pub fn parse(bytes: &[u8]) -> FileMetadata {
    let limits = Limits::default();
    match dalica_meta::scan(bytes, &limits) {
        Ok(scan) => {
            let m = dalica_meta::read_scan(&scan, &limits);
            let problems = m.problems.iter().map(|p| format!("{:?}: {}", p.block, p.error)).collect();
            FileMetadata { owned: m.owned, summary: m.summary, listing: dalica_meta::listing::listing_scan(&scan, &limits), problems }
        }
        Err(e) => FileMetadata { problems: vec![e.to_string()], ..FileMetadata::default() },
    }
}

/// About how many bytes an answer holds.
fn size_of(m: &FileMetadata) -> usize {
    const ROW: usize = 96;
    let text = |s: &Option<String>| s.as_ref().map_or(0, String::len);
    let rows: usize = m.listing.rows.iter().map(|r| ROW + r.group.len() + r.label.len() + r.value.len()).sum();
    let owned = text(&m.owned.caption) + m.owned.tags.iter().map(|t| ROW + t.len()).sum::<usize>();
    let summary = text(&m.summary.make) + text(&m.summary.model) + text(&m.summary.lens);
    let problems: usize = m.problems.iter().map(|p| ROW + p.len()).sum();
    ROW.saturating_mul(4).saturating_add(rows).saturating_add(owned).saturating_add(summary).saturating_add(problems)
}

impl Session {
    /// The version of photo `id`'s file the library knows: its path, size and modification time.
    pub(crate) fn file_key(&self, id: PhotoId) -> Option<FileKey> {
        let p = self.library.photo(id)?;
        let folder = self.library.folder(p.folder)?;
        Some(FileKey { path: folder.path.join(&p.name), size: p.size, mtime: p.mtime })
    }

    /// Records that `fields` of photo `id`'s file were changed in another application while
    /// Dalica's values were kept (`xmp-interop.md` §5 rule 3): `photo.metadata` reports it with the
    /// version of the file the library knows now, until the file changes again. Empty `fields`
    /// clears it. For the scan's import (M6.32) and the write-back (M6.33): call it after the op
    /// that updates the photo's size and time is applied.
    pub fn note_changed_elsewhere(&mut self, id: PhotoId, fields: Fields) {
        let Some(key) = self.file_key(id) else { return };
        let notes = &mut self.metadata_cache.notes;
        if fields.is_empty() {
            notes.remove(&key.path);
        } else if notes.len() < MAX_NOTES || notes.contains_key(&key.path) {
            notes.insert(key.path, (key.size, key.mtime, fields));
        } else {
            log::warn!("more than {MAX_NOTES} photos changed in another application; this one is not noted");
        }
    }

    /// Photo metadata reads queued or running (`photo.metadata`'s readers).
    pub fn photo_metadata_pending(&self) -> usize {
        self.metadata_cache.pending()
    }

    /// Blocks until this session's metadata readers are idle or `timeout` passed (CLI, tests);
    /// `true` when idle. Never call it on the UI thread: `photo.metadata` answers `pending` instead.
    pub fn wait_photo_metadata(&self, timeout: Duration) -> bool {
        self.metadata_cache.wait(timeout)
    }

    /// Called from [`Session::poll`] each frame: how many metadata answers arrived since the last
    /// frame (a frame that got some draws again).
    pub(crate) fn poll_photo_metadata(&mut self) -> usize {
        self.metadata_cache.take_arrived()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u64) -> FileKey {
        FileKey { path: PathBuf::from(format!("/nowhere/dalica-cache-test/{n}.jpg")), size: n, mtime: Timestamp::from_millis(0) }
    }

    fn ready(n: usize) -> Slot {
        Slot::Ready(Arc::new(FileMetadata::default()), n)
    }

    #[test]
    fn the_queue_and_the_answers_stay_within_their_bounds() {
        let mut c = Cache::default();
        for n in 0..(MAX_QUEUE as u64 + 5) {
            c.enqueue(&key(n));
        }
        assert_eq!(c.queue.len(), MAX_QUEUE);
        assert_eq!(c.queue.front(), Some(&key(5)), "the oldest requests went");
        assert!(!c.slots.contains_key(&key(0)));
        // Asking again moves a request to the back (read first).
        c.enqueue(&key(5));
        assert_eq!((c.queue.back(), c.queue.len()), (Some(&key(5)), MAX_QUEUE));

        let mut c = Cache::default();
        for n in 0..(MAX_ENTRIES as u64 + 3) {
            c.reading += 1;
            c.answer(key(n), ready(10));
        }
        assert_eq!((c.lru.len(), c.bytes, c.arrived), (MAX_ENTRIES, MAX_ENTRIES * 10, MAX_ENTRIES + 3));
        assert!(!c.slots.contains_key(&key(2)) && c.slots.contains_key(&key(3)));
        // A used answer is kept over older ones.
        c.touch(&key(3));
        c.reading += 1;
        c.answer(key(1000), ready(10));
        assert!(c.slots.contains_key(&key(3)) && !c.slots.contains_key(&key(4)));
        // Past the byte budget the oldest go, but the newest is always kept.
        c.reading += 1;
        c.answer(key(2000), ready(MAX_BYTES + 1));
        assert_eq!(c.lru.len(), 1);
        assert_eq!(c.bytes, MAX_BYTES + 1);
        assert_eq!(c.reading, 0);
        // Once closed, answers are dropped.
        c.closed = true;
        c.reading += 1;
        c.answer(key(3000), ready(1));
        assert!(!c.slots.contains_key(&key(3000)));
    }

    #[test]
    fn a_missing_file_fails_and_bytes_meta_cannot_walk_are_an_answer_with_a_problem() {
        assert_eq!(read_file(Path::new("/nowhere/dalica-cache-test/missing.jpg")), Err("the file is missing".into()));
        let m = parse(b"not a photo at all");
        assert_eq!(m.owned, OwnedFields::default());
        assert_eq!(m.problems.len(), 1);
        assert!(parse(&[]).problems.len() == 1);
    }

    #[test]
    fn a_missing_file_is_answered_pending_then_failed() {
        let cache = MetadataCache::default();
        let k = key(1);
        assert_eq!(cache.lookup(&k), Lookup::Pending);
        assert!(cache.wait(Duration::from_secs(30)));
        assert_eq!(cache.take_arrived(), 1);
        assert_eq!(cache.lookup(&k), Lookup::Failed("the file is missing".into()));
        assert_eq!((cache.pending(), cache.take_arrived()), (0, 0));
    }

    #[test]
    fn dropping_the_cache_drops_the_waiting_files() {
        let cache = MetadataCache::default();
        let shared = Arc::clone(&cache.shared);
        for n in 0..10 {
            cache.lookup(&key(n));
        }
        drop(cache);
        let c = shared.lock();
        assert!(c.closed && c.queue.is_empty());
    }
}
