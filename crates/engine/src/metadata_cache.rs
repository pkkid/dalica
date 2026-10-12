//! The photo metadata cache behind `photo.metadata` (`cmd/info.rs`, the Properties pane of
//! `ui-layout.md` §7): what a photo's file says about itself (the owned fields, the EXIF summary and
//! the Show All Metadata listing), read through `dalica-meta` on worker threads.
//!
//! The thread that owns the session never reads a file. A lookup answers from the cache, or queues
//! the file and answers [`Lookup::Pending`]; up to [`WORKERS`] "dalica-metadata" threads read the
//! queue newest first (the photo the user is looking at now comes before the ones they arrowed
//! past), and the next lookup finds the answer. [`Session::wait_photo_metadata`] blocks until the
//! readers are idle (CLI, tests).
//!
//! Entries are keyed by one version of a file: its path with the size and modification time the
//! library knows, so a file changed on disk is read again once a scan has seen the change. What a
//! file version says does not depend on the library that asks, so the cache is one per process
//! (like the thumbnail cache folder) and bounded: at most [`MAX_ENTRIES`] answers and about
//! [`MAX_BYTES`] of text, the least recently used going first; at most [`MAX_QUEUE`] files wait,
//! the oldest request dropped first (it is queued again when asked again). A file that could not be
//! read (missing, too large, unreadable) is answered as failed for [`RETRY`], then read again.
//!
//! The "Changed in another application; Dalica's value kept" note (`xmp-interop.md` §5 rule 3) is
//! kept here too, against the file version it was found in ([`Session::note_changed_elsewhere`],
//! for the scan's import and the write-back): a later version of the file (Dalica's own write of
//! its value, or another edit) ends it.

use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError};
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
/// Reader threads at most.
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
    /// Fields changed in another application, by path, with the file version they were found in.
    notes: HashMap<PathBuf, (u64, Timestamp, Fields)>,
}

static CACHE: LazyLock<(Mutex<Cache>, Condvar)> = LazyLock::new(|| (Mutex::new(Cache::default()), Condvar::new()));

fn lock() -> MutexGuard<'static, Cache> {
    CACHE.0.lock().unwrap_or_else(PoisonError::into_inner)
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

/// The answer for `key`, queuing it (and starting a reader) when there is none yet.
pub fn lookup(key: &FileKey) -> Lookup {
    let mut c = lock();
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
    start_workers(&mut c);
    Lookup::Pending
}

/// Starts readers while fewer than [`WORKERS`] run and files wait. A reader that cannot be started
/// fails the files waiting (they are tried again after [`RETRY`]).
fn start_workers(c: &mut Cache) {
    while c.workers < WORKERS && c.workers < c.queue.len() {
        match std::thread::Builder::new().name("dalica-metadata".into()).spawn(work) {
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

/// A reader thread: reads the newest waiting file until none waits.
fn work() {
    loop {
        let key = {
            let mut c = lock();
            match c.queue.pop_back() {
                Some(key) => {
                    c.slots.insert(key.clone(), Slot::Reading);
                    c.reading = c.reading.saturating_add(1);
                    key
                }
                None => {
                    c.workers = c.workers.saturating_sub(1);
                    CACHE.1.notify_all();
                    return;
                }
            }
        };
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
        lock().answer(key, slot);
        CACHE.1.notify_all();
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

/// The note for `key`'s file: the fields changed in another application in this version of it.
pub fn note(key: &FileKey) -> Fields {
    match lock().notes.get(&key.path) {
        Some((size, mtime, fields)) if (*size, *mtime) == (key.size, key.mtime) => *fields,
        _ => Fields::NONE,
    }
}

/// Files queued or being read now.
pub fn pending() -> usize {
    let c = lock();
    c.queue.len().saturating_add(c.reading)
}

/// Blocks until no file is queued or being read, or `timeout` passed; `true` when idle.
pub fn wait(timeout: Duration) -> bool {
    let start = Instant::now();
    let mut c = lock();
    loop {
        if c.queue.is_empty() && c.reading == 0 {
            return true;
        }
        let Some(left) = timeout.checked_sub(start.elapsed()) else { return false };
        c = CACHE.1.wait_timeout(c, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
    }
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
        let mut c = lock();
        if fields.is_empty() {
            c.notes.remove(&key.path);
        } else if c.notes.len() < MAX_NOTES || c.notes.contains_key(&key.path) {
            c.notes.insert(key.path, (key.size, key.mtime, fields));
        } else {
            log::warn!("more than {MAX_NOTES} photos changed in another application; this one is not noted");
        }
    }

    /// Photo metadata reads queued or running (`photo.metadata`'s readers, for every session of
    /// this process).
    pub fn photo_metadata_pending(&self) -> usize {
        pending()
    }

    /// Blocks until the metadata readers are idle or `timeout` passed (CLI, tests); `true` when
    /// idle. Never call it on the UI thread: `photo.metadata` answers `pending` instead.
    pub fn wait_photo_metadata(&self, timeout: Duration) -> bool {
        wait(timeout)
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

    /// The bounds of a private cache (the process-wide one is shared with the other tests).
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
        assert_eq!((c.lru.len(), c.bytes), (MAX_ENTRIES, MAX_ENTRIES * 10));
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
        let k = FileKey { path: PathBuf::from("/nowhere/dalica-cache-test/gone.jpg"), size: 1, mtime: Timestamp::from_millis(1) };
        assert_eq!(lookup(&k), Lookup::Pending);
        assert!(wait(Duration::from_secs(30)));
        assert_eq!(lookup(&k), Lookup::Failed("the file is missing".into()));
    }
}
