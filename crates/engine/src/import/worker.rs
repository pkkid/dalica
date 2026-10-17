//! The listing worker (`organizing.md` §3 items 1–3): one thread per listing runs `dalica-scan`'s
//! [`list_source`] over the source and decides, file by file, whether each picture is a duplicate.
//!
//! A candidate is a **duplicate** when a library photo, or an earlier candidate of the same listing,
//! has its size and MD5 digest and its bytes compare equal in full (so a digest collision is never a
//! duplicate). The library's side is the [`Plan`] the owner made from the library alone when the
//! source was opened: its photos by recorded size, each with its digest when the index holds one
//! that still fits the file. A photo of a candidate's size without one is read here (through `fs`,
//! at most [`MAX_FILE`] bytes) and its digest goes back to the owner, which keeps it in the index as
//! M9's Find Duplicates does (derived data, no undo step).
//!
//! Everything the worker finds goes into the listing's [`Shared`] state under a short lock; the
//! owner reads it there when a command asks and never waits on the disk. The thumbnails go into the
//! listing's memory-only cache ([`super::thumbs`]), never into the thumbnail cache on disk.

use std::collections::HashMap;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dalica_core::{PhotoId, Timestamp};
use dalica_library::ContentDigest;
use dalica_scan::Warning;
use dalica_scan::import::{Candidate, ListEvent, ListOptions, ListProgress, list_source};

use super::thumbs::ThumbCache;

/// The largest library file read to make its digest (as Find Duplicates).
pub const MAX_FILE: u64 = crate::duplicates::MAX_FILE;

/// Most warnings a listing keeps (the rest are counted).
pub const MAX_WARNINGS: usize = 256;

/// Most bytes compared between two files (`dalica-scan`'s most bytes read of one file).
const MAX_COMPARE: u64 = dalica_scan::import::MAX_READ;

/// Bytes compared at a time.
const CHUNK: usize = 64 * 1024;

/// A library photo a candidate may duplicate.
#[derive(Clone, Debug)]
pub(crate) struct LibraryFile {
    pub id: PhotoId,
    pub path: PathBuf,
    /// Its digest: from the index when it still fits the file, else made here when first needed.
    pub digest: Option<String>,
    /// A digest was tried here (made or not): never read twice.
    pub tried: bool,
}

/// The library's photos by recorded size.
pub(crate) type Plan = HashMap<u64, Vec<LibraryFile>>;

/// What a duplicate repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateOf {
    /// A photo of the library.
    Photo(PhotoId),
    /// An earlier candidate of the listing (its id).
    Candidate(u64),
}

/// One picture of the source as the listing keeps it (its thumbnail is in the cache).
#[derive(Clone, Debug)]
pub(crate) struct Found {
    pub candidate: Candidate,
    pub duplicate: Option<DuplicateOf>,
}

/// Where a listing is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListState {
    /// Walking the source or reading its files.
    #[default]
    Loading,
    /// Every file was looked at.
    Done,
    /// Stopped before the end (the listing was dropped).
    Cancelled,
    /// The worker stopped unexpectedly.
    Failed,
}

impl ListState {
    pub fn id(self) -> &'static str {
        match self {
            ListState::Loading => "loading",
            ListState::Done => "done",
            ListState::Cancelled => "cancelled",
            ListState::Failed => "failed",
        }
    }
}

/// What the worker has found so far, read by the owner under a short lock.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    pub state: ListState,
    /// The walk ended ([`ListEvent::Walked`]).
    pub walked: bool,
    pub progress: ListProgress,
    /// Batches of pictures received.
    pub batches: u64,
    /// The pictures, in listing order (a picture's id is its place here).
    pub found: Vec<Found>,
    /// The source holds more than the listing looked at.
    pub truncated: bool,
    /// The first [`MAX_WARNINGS`] warnings, and how many there were.
    pub warnings: Vec<Warning>,
    pub warning_count: u64,
    /// Library digests made here that the owner has not kept yet.
    pub digests: Vec<(PhotoId, ContentDigest)>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The listing's thread body: lists `root` into `shared` and `thumbs` until done or `options.cancel`
/// is set.
pub(crate) fn run(root: PathBuf, options: ListOptions, plan: Plan, shared: Arc<Mutex<Shared>>, thumbs: Arc<Mutex<ThumbCache>>) {
    let cancel = options.cancel.clone();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut dups = Duplicates { plan, seen: HashMap::new(), cancel: cancel.clone() };
        list_source(&root, &options, |event| {
            handle(event, &mut dups, &shared, &thumbs);
            true
        })
    }));
    let mut s = lock(&shared);
    match outcome {
        Ok(listing) => {
            s.state = if listing.cancelled { ListState::Cancelled } else { ListState::Done };
            s.truncated = listing.truncated;
            s.progress = listing.progress;
        }
        Err(_) => {
            log::error!("the import listing of {} stopped unexpectedly", root.display());
            s.state = ListState::Failed;
        }
    }
}

fn handle(event: ListEvent, dups: &mut Duplicates, shared: &Mutex<Shared>, thumbs: &Mutex<ThumbCache>) {
    match event {
        ListEvent::Walked(p) => {
            let mut s = lock(shared);
            s.walked = true;
            s.progress = p;
        }
        ListEvent::Batch(batch) => {
            let mut found = Vec::with_capacity(batch.len());
            let mut digests = Vec::new();
            for mut c in batch {
                let id = c.index as u64;
                if let Some(t) = c.thumbnail.take() {
                    lock(thumbs).insert(id, Arc::new(t));
                }
                let duplicate = dups.check(&c, &mut digests);
                found.push(Found { candidate: c, duplicate });
            }
            let mut s = lock(shared);
            s.found.extend(found);
            s.digests.extend(digests);
            s.batches = s.batches.saturating_add(1);
        }
        ListEvent::Progress(p) => lock(shared).progress = p,
        ListEvent::Warning(w) => {
            let mut s = lock(shared);
            s.warning_count = s.warning_count.saturating_add(1);
            if s.warnings.len() < MAX_WARNINGS {
                s.warnings.push(w);
            }
        }
    }
}

/// The duplicate check's memory: the library's plan, and the candidates seen by size and digest.
struct Duplicates {
    plan: Plan,
    /// The candidates that duplicate nothing, by size and digest: what a later one is compared with.
    seen: HashMap<(u64, String), Vec<(u64, PathBuf)>>,
    cancel: Option<Arc<AtomicBool>>,
}

impl Duplicates {
    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
    }

    /// What `c` duplicates: a library photo first, else an earlier candidate. Library digests made
    /// on the way are added to `digests`.
    fn check(&mut self, c: &Candidate, digests: &mut Vec<(PhotoId, ContentDigest)>) -> Option<DuplicateOf> {
        if self.cancelled() {
            return None;
        }
        if let Some(files) = self.plan.get_mut(&c.size) {
            for f in files.iter_mut() {
                if f.digest.is_none() && !f.tried {
                    f.tried = true;
                    if let Some(record) = digest_of(&f.path) {
                        f.digest = Some(record.digest.clone());
                        digests.push((f.id, record));
                    }
                }
                if f.digest.as_deref() == Some(c.digest.as_str()) && same_bytes(&c.path, &f.path) {
                    return Some(DuplicateOf::Photo(f.id));
                }
            }
        }
        let firsts = self.seen.entry((c.size, c.digest.clone())).or_default();
        if let Some((id, _)) = firsts.iter().find(|(_, path)| same_bytes(&c.path, path)) {
            return Some(DuplicateOf::Candidate(*id));
        }
        firsts.push((c.index as u64, c.path.clone()));
        None
    }
}

/// The digest record of the library file at `path` as it is now, or `None` when it cannot be read,
/// is not a regular file, is larger than [`MAX_FILE`] or changed while it was read.
fn digest_of(path: &Path) -> Option<ContentDigest> {
    let stat = dalica_fs::stat(path).ok()??;
    if !stat.is_file || stat.identity.size() > MAX_FILE {
        return None;
    }
    let read = dalica_fs::read(path, MAX_FILE).ok()??;
    (read.identity == stat.identity).then(|| ContentDigest {
        size: stat.identity.size(),
        mtime: Timestamp::from_system_time(stat.times.modified),
        digest: crate::duplicates::md5_digest(&read.bytes),
    })
}

/// Whether the files at `a` and `b` hold the same bytes, compared in full a chunk at a time (never
/// more than [`MAX_COMPARE`] bytes; a file that goes on past that compares unequal). A file that
/// cannot be read compares unequal.
pub(crate) fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(fa), Ok(fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut fa, mut fb) = (fa.take(MAX_COMPARE.saturating_add(1)), fb.take(MAX_COMPARE.saturating_add(1)));
    let (mut ba, mut bb) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    let mut total: u64 = 0;
    loop {
        let (Ok(na), Ok(nb)) = (fill(&mut fa, &mut ba), fill(&mut fb, &mut bb)) else { return false };
        if na != nb || ba.get(..na) != bb.get(..nb) {
            return false;
        }
        if na == 0 {
            return true;
        }
        total = total.saturating_add(na as u64);
        if total > MAX_COMPARE {
            return false;
        }
    }
}

/// Reads into `buf` until it is full or the file ends; the bytes read.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let Some(rest) = buf.get_mut(n..) else { break };
        match r.read(rest) {
            Ok(0) => break,
            Ok(k) => n = n.saturating_add(k),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_compare_equal_only_when_every_byte_matches() {
        let dir = tempfile::tempdir().unwrap();
        let big: Vec<u8> = (0..(CHUNK * 2 + 7)).map(|i| (i % 251) as u8).collect();
        let mut other = big.clone();
        *other.last_mut().unwrap() ^= 1;
        let (a, b, c, d) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"), dir.path().join("d"));
        for (path, bytes) in [(&a, &big[..]), (&b, &big[..]), (&c, &other[..]), (&d, &big[..CHUNK])] {
            dalica_fs::replace(path, bytes, dalica_fs::Expect::Absent).unwrap();
        }
        assert!(same_bytes(&a, &b));
        assert!(!same_bytes(&a, &c), "the last byte differs");
        assert!(!same_bytes(&a, &d), "one is shorter");
        assert!(!same_bytes(&a, &dir.path().join("missing")));
    }
}
