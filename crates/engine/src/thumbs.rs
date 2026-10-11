//! The session's thumbnails (`architecture.md` §5, §6.5): the size ladder in the packed disk cache,
//! scan-time generation, answers that never wait, and direction-aware prefetch.
//!
//! - **After a scan** adds or changes photos, a background job per photo makes its 128 and 256
//!   entries and its colour sketch from one decode ([`dalica_thumbs::ladder::scan`]). Sketches
//!   come back to the owner, which applies them in batches as derived ops (journaled, never on the
//!   undo stack, no sidecar write). Opening a library checks every photo the same way; a photo
//!   whose entries are cached is not decoded again.
//! - **512, 1024 and the screen-size preview** are made on demand ([`dalica_thumbs::ladder::fetch`]).
//! - **[`Session::thumbnail`](crate::Session::thumbnail)** answers at once from memory: the size
//!   that covers the cell, else the next smaller one to scale up, else the sketch, else
//!   [`Thumb::NotScanned`], and queues what is missing at the visible priority.
//! - **[`Session::prefetch_thumbnails`](crate::Session::prefetch_thumbnails)** loads the cells
//!   around the visible ones, in the scroll direction and further ahead the faster the view moves;
//!   prefetch the view has moved away from is cancelled.
//! - The **cache directory** comes from the session's options: Dalica's cache folder
//!   (`<cache>/thumbs`, [`default_cache_dir`]) in the apps, a temporary folder removed with the
//!   session in tests and demos. A cache another process holds, or one that cannot be opened, falls
//!   back to a temporary folder (with a notice).
//!
//! The owner thread only submits jobs, moves finished images into a byte-budgeted memory cache
//! and answers from it: it never touches the disk and never decodes.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::PhotoId;
use dalica_library::{Library, Photo, Sketch};
use dalica_thumbs::ladder::{self, Choice, Source};
pub use dalica_thumbs::pack::Size;
use dalica_thumbs::pack::{Cache, Key, Maintenance};
use dalica_thumbs::{Lru, Pool, Priority, Rgba8, Submitted};

/// Bytes of decoded thumbnails kept in memory by default.
pub const MEMORY: usize = 192 << 20;
/// Sketches applied per op.
pub const SKETCH_BATCH: usize = 512;
/// Finished jobs handled per [`Session::poll`](crate::Session::poll), so a frame stays short.
pub const POLL_MAX: usize = 256;
/// Bytes of scan images on their way to the owner; past it, scans send their sketch only, so a
/// large scan cannot pile images up faster than the owner takes them.
const IN_FLIGHT: usize = 96 << 20;
/// How long [`Thumbs::wait`] waits without any job finishing before it gives up on the rest.
const STALL: Duration = Duration::from_secs(120);

/// The cache folder the apps set for sessions opened with [`CacheDir::Default`].
static PROCESS_CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Sets (or clears) the thumbnail cache folder used by sessions opened with [`CacheDir::Default`]
/// from now on. The apps set it to [`default_cache_dir`] when they open the user's library (a demo
/// keeps a temporary cache).
pub fn set_process_cache_dir(dir: Option<PathBuf>) {
    *PROCESS_CACHE.lock().unwrap_or_else(PoisonError::into_inner) = dir;
}

fn process_cache_dir() -> Option<PathBuf> {
    PROCESS_CACHE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The environment variable that replaces Dalica's cache folder (tests, and anyone who wants the
/// cache elsewhere); `thumbs` goes inside it.
pub const CACHE_DIR_VAR: &str = "DALICA_CACHE_DIR";

/// The thumbnail cache in Dalica's cache folder: `$DALICA_CACHE_DIR/thumbs` when that is an
/// absolute folder, else the platform's (`control-protocol.md` §2.1): Linux
/// `$XDG_CACHE_HOME/dalica/thumbs` (`~/.cache` without it), macOS
/// `~/Library/Caches/Dalica/thumbs`, Windows `%LOCALAPPDATA%\Dalica\cache\thumbs`. `None` when
/// the environment (read through `var`) names no absolute folder.
pub fn default_cache_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let abs = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
    let cache = if let Some(dir) = abs(CACHE_DIR_VAR) {
        Some(dir)
    } else if cfg!(windows) {
        abs("LOCALAPPDATA").map(|d| d.join("Dalica").join("cache"))
    } else if cfg!(target_os = "macos") {
        abs("HOME").map(|h| h.join("Library").join("Caches").join("Dalica"))
    } else {
        abs("XDG_CACHE_HOME").or_else(|| abs("HOME").map(|h| h.join(".cache"))).map(|c| c.join("dalica"))
    };
    cache.map(|c| c.join("thumbs"))
}

/// Where a session keeps its thumbnail cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CacheDir {
    /// The folder the app set ([`set_process_cache_dir`]), else a temporary one.
    #[default]
    Default,
    /// A new temporary folder, removed when the session closes.
    Temp,
    /// This folder.
    At(PathBuf),
    /// No thumbnails: answers are the sketch or [`Thumb::NotScanned`], and nothing is generated.
    Off,
}

/// The thumbnail options of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbOptions {
    pub cache: CacheDir,
    /// Bytes of decoded thumbnails kept in memory.
    pub memory: usize,
    /// Worker threads (at least one).
    pub workers: usize,
    /// The long side of the display, for the screen-size preview (device pixels).
    pub screen: u32,
}

impl Default for ThumbOptions {
    fn default() -> ThumbOptions {
        ThumbOptions { cache: CacheDir::Default, memory: MEMORY, workers: dalica_thumbs::default_workers(), screen: ladder::DEFAULT_SCREEN }
    }
}

/// What a cell can draw now.
#[derive(Clone, Debug, PartialEq)]
pub enum Thumb {
    /// A ladder image: it covers the cell when `covers`; otherwise it is the next smaller size, to
    /// scale up while the covering one loads.
    Image { size: Size, image: Arc<Rgba8>, covers: bool },
    /// No ladder image in memory yet: the colour sketch, drawn the same frame.
    Sketch(Sketch),
    /// Never scanned (or its photo could not be read): the placeholder.
    NotScanned,
}

/// The thumbnailer's numbers (`ui.inspect`, tests, benches).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThumbStats {
    /// Jobs queued or running.
    pub jobs: usize,
    /// Of those, prefetch jobs.
    pub prefetching: usize,
    /// Photos decoded (scan and fetch jobs that had to read the photo).
    pub decodes: u64,
    /// Entries read from the disk cache.
    pub loads: u64,
    /// Prefetch jobs cancelled because the view moved away.
    pub cancelled: u64,
    /// Photos with their 128 and 256 entries made.
    pub scanned: usize,
    /// Photos whose thumbnails failed.
    pub failed: usize,
    /// Bytes of decoded thumbnails in memory.
    pub memory: usize,
}

/// One job, de-duplicated by the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Job {
    Scan(PhotoId),
    Fetch(PhotoId, Size),
    Maintain,
}

/// A finished job.
enum Done {
    Scanned { key: Key, result: Result<ladder::Scanned, ladder::Error>, in_flight: usize },
    Fetched { key: Key, size: Size, result: Result<ladder::Fetched, ladder::Error> },
    Maintained(Result<Maintenance, String>),
    Panicked(String),
}

/// What the owner knows of one photo.
struct PhotoState {
    key: Key,
    /// Its 128 and 256 entries are in the cache.
    scanned: bool,
    failed: Option<String>,
}

/// The session's thumbnailer (owned by the session's thread).
pub(crate) struct Thumbs {
    /// `None` when thumbnails are off or no cache could be opened.
    pool: Option<Pool<Job, Done>>,
    cache: Option<Arc<Cache>>,
    temp: Option<dalica_fs::TempDir>,
    photos: HashMap<PhotoId, PhotoState>,
    memory: Lru<(PhotoId, Size), (Key, Arc<Rgba8>)>,
    /// Jobs queued or running, with the highest priority asked for.
    outstanding: HashMap<Job, Priority>,
    scans: usize,
    prefetching: HashSet<Job>,
    sketches: Vec<(PhotoId, Option<Sketch>)>,
    in_flight: Arc<AtomicUsize>,
    maintain_due: bool,
    screen: u32,
    limits: dalica_image::Limits,
    stats: ThumbStats,
}

impl Thumbs {
    /// Opens the cache `options` name (off the UI thread) and starts the workers. Returns what
    /// went wrong on the way, for the session's notices.
    pub(crate) fn open(options: &ThumbOptions) -> (Thumbs, Vec<String>) {
        let mut notices = Vec::new();
        let mut temp = None;
        let at = match &options.cache {
            CacheDir::Off => None,
            CacheDir::At(dir) => Some(dir.clone()),
            CacheDir::Default => process_cache_dir(),
            CacheDir::Temp => None,
        };
        let mut cache = at.and_then(|dir| match Cache::open(&dir, dalica_thumbs::pack::Options::default()) {
            Ok(c) => Some(c),
            Err(e) => {
                notices.push(format!("the thumbnail cache {} cannot be used ({e}); thumbnails are kept in a temporary folder", dir.display()));
                None
            }
        });
        if cache.is_none() && options.cache != CacheDir::Off {
            match dalica_fs::TempDir::new("dalica-thumbs").map_err(|e| e.to_string()).and_then(|t| {
                let c = Cache::open(t.path(), dalica_thumbs::pack::Options::default()).map_err(|e| e.to_string())?;
                Ok((t, c))
            }) {
                Ok((t, c)) => {
                    temp = Some(t);
                    cache = Some(c);
                }
                Err(e) => notices.push(format!("no thumbnail cache could be made ({e}); thumbnails are off")),
            }
        }
        let pool = cache.as_ref().map(|_| Pool::new(options.workers.max(1)));
        let thumbs = Thumbs {
            pool,
            cache: cache.map(Arc::new),
            temp,
            photos: HashMap::new(),
            memory: Lru::new(options.memory),
            outstanding: HashMap::new(),
            scans: 0,
            prefetching: HashSet::new(),
            sketches: Vec::new(),
            in_flight: Arc::new(AtomicUsize::new(0)),
            maintain_due: false,
            screen: options.screen.clamp(1, ladder::MAX_SCREEN),
            limits: dalica_image::Limits::default(),
            stats: ThumbStats::default(),
        };
        (thumbs, notices)
    }

    /// The cache folder, if thumbnails are on.
    pub(crate) fn cache_dir(&self) -> Option<&std::path::Path> {
        self.cache.as_deref().map(Cache::dir)
    }

    /// Brings the thumbnailer in line with the library after it was opened or scanned: a photo it
    /// has not seen, or whose file changed, gets a background scan job (its sketch too when it has
    /// none or the file changed); photos that are gone are forgotten. One pass over the library.
    pub(crate) fn sync(&mut self, library: &Library) {
        self.photos.retain(|id, _| library.photo(*id).is_some());
        for photo in library.photos() {
            let Some(source) = source(library, photo) else { continue };
            let key = source.key();
            let changed = match self.photos.get(&photo.id) {
                Some(st) if st.key == key => continue,
                Some(_) => true,
                None => false,
            };
            if changed {
                self.forget_images(photo.id);
            }
            self.photos.insert(photo.id, PhotoState { key, scanned: false, failed: None });
            let want_sketch = changed || library.sketch(photo.id).is_none();
            self.submit_scan(photo.id, source, want_sketch, Priority::BACKGROUND);
        }
        self.maintain_due = true;
    }

    /// Starts tracking one photo the last [`Thumbs::sync`] did not see (an undo brought it back).
    fn track(&mut self, library: &Library, id: PhotoId, priority: Priority) {
        let Some(source) = library.photo(id).and_then(|p| source(library, p)) else { return };
        self.photos.insert(id, PhotoState { key: source.key(), scanned: false, failed: None });
        self.submit_scan(id, source, library.sketch(id).is_none(), priority);
    }

    fn forget_images(&mut self, id: PhotoId) {
        for size in Size::ALL {
            self.memory.remove(&(id, size));
        }
    }

    fn submit_scan(&mut self, id: PhotoId, source: Source, want_sketch: bool, priority: Priority) {
        let Some(cache) = self.cache.clone() else { return };
        let (limits, in_flight) = (self.limits, Arc::clone(&self.in_flight));
        self.submit(Job::Scan(id), priority, move |cancel| {
            let key = source.key();
            let mut result = ladder::scan(&cache, &source, want_sketch, &limits, cancel);
            let mut bytes = 0;
            if let Ok(s) = &mut result {
                bytes = s.images.iter().map(|(_, i)| i.data.len()).sum();
                if in_flight.fetch_add(bytes, Ordering::SeqCst).saturating_add(bytes) > IN_FLIGHT {
                    in_flight.fetch_sub(bytes, Ordering::SeqCst);
                    s.images.clear();
                    bytes = 0;
                }
            }
            Done::Scanned { key, result, in_flight: bytes }
        });
    }

    fn submit_fetch(&mut self, library: &Library, id: PhotoId, size: Size, priority: Priority) -> Option<Job> {
        let job = Job::Fetch(id, size);
        let source = library.photo(id).and_then(|p| source(library, p))?;
        let cache = self.cache.clone()?;
        let (limits, screen) = (self.limits, self.screen);
        self.submit(job, priority, move |cancel| {
            let key = source.key();
            Done::Fetched { key, size, result: ladder::fetch(&cache, &source, size, screen, &limits, cancel) }
        });
        Some(job)
    }

    /// Submits `work` unless `job` is already on its way at `priority` or higher. A panic in the
    /// job comes back as [`Done::Panicked`].
    fn submit(&mut self, job: Job, priority: Priority, work: impl FnOnce(&AtomicBool) -> Done + Send + 'static) {
        let before = self.outstanding.get(&job).copied();
        if before.is_some_and(|p| p >= priority) {
            return;
        }
        let Some(pool) = &self.pool else { return };
        let wrapped = move |cancel: &AtomicBool| {
            catch_unwind(AssertUnwindSafe(|| work(cancel))).unwrap_or_else(|panic| {
                let message =
                    panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                Done::Panicked(message)
            })
        };
        match pool.submit(job, priority, wrapped) {
            Submitted::Queued => {
                if matches!(job, Job::Scan(_)) && before.is_none() {
                    self.scans += 1;
                }
                self.outstanding.insert(job, priority);
            }
            Submitted::Merged => {
                self.outstanding.insert(job, priority.max(before.unwrap_or(priority)));
            }
            // Its result is coming (or it was cancelled and nothing comes: the next request asks
            // again).
            Submitted::Running => {
                if let Some(p) = self.outstanding.get_mut(&job) {
                    *p = (*p).max(priority);
                }
            }
        }
    }

    /// What photo `id` draws in a cell whose long side is `need` device pixels, now; queues the
    /// covering size (and a smaller one first when nothing is in memory) at the visible priority.
    pub(crate) fn request(&mut self, library: &Library, id: PhotoId, need: u32) -> Thumb {
        if library.photo(id).is_none() {
            return Thumb::NotScanned;
        }
        if !self.photos.contains_key(&id) {
            self.track(library, id, Priority::VISIBLE);
        }
        let Some(st) = self.photos.get(&id) else { return fallback(library, id) };
        let (key, scanned, failed) = (st.key, st.scanned, st.failed.is_some());
        let want = ladder::covering(need, self.screen);
        let found = self.best(id, key, need);
        let covers = matches!(found, Some((_, _, true)));
        if !failed && !covers {
            if !scanned {
                if let Some(source) = library.photo(id).and_then(|p| source(library, p)) {
                    self.submit_scan(id, source, library.sketch(id).is_none(), Priority::VISIBLE);
                }
            } else {
                if found.is_none()
                    && let Some(smaller) =
                        ladder::SCAN_SIZES.iter().rev().copied().find(|s| ladder::pixels(*s, self.screen) < ladder::pixels(want, self.screen))
                {
                    self.submit_fetch(library, id, smaller, Priority::VISIBLE);
                }
                self.submit_fetch(library, id, want, Priority::VISIBLE);
            }
        }
        match found {
            Some((size, image, covers)) => Thumb::Image { size, image, covers },
            None => fallback(library, id),
        }
    }

    /// The best image in memory for a cell of `need` pixels: (size, image, covers).
    fn best(&mut self, id: PhotoId, key: Key, need: u32) -> Option<(Size, Arc<Rgba8>, bool)> {
        let screen = self.screen;
        for _ in 0..Size::ALL.len() {
            let choice = ladder::choose(need, screen, |s| self.memory.contains(&(id, s)));
            let (size, covers) = match choice {
                Choice::Covers(s) => (s, true),
                Choice::Smaller(s) => (s, false),
                Choice::Nothing => return None,
            };
            match self.memory.get(&(id, size)) {
                Some((k, image)) if *k == ladder::entry_key(key, size, screen) => return Some((size, Arc::clone(image), covers)),
                // Made for another screen size: drop it and look again.
                _ => {
                    self.memory.remove(&(id, size));
                }
            }
        }
        None
    }

    /// Prefetches around the visible cells (`visible`, indices into `order`, the photos in display
    /// order), in the scroll direction (`velocity` in cells per second, positive towards the end);
    /// cancels prefetch the view has moved away from. Returns the photos in the order they were
    /// queued.
    pub(crate) fn prefetch(&mut self, library: &Library, order: &[PhotoId], visible: Range<usize>, velocity: f64, need: u32) -> Vec<PhotoId> {
        let plan: Vec<PhotoId> = ladder::prefetch(visible, order.len(), velocity).into_iter().filter_map(|i| order.get(i).copied()).collect();
        let want = ladder::covering(need, self.screen);
        let mut next = HashSet::new();
        for &id in &plan {
            if library.photo(id).is_none() {
                continue;
            }
            if !self.photos.contains_key(&id) {
                self.track(library, id, Priority::PREFETCH);
            }
            let Some(st) = self.photos.get(&id) else { continue };
            if st.failed.is_some() {
                continue;
            }
            if !st.scanned {
                // Raise its background scan; a scan is never cancelled.
                if let Some(source) = library.photo(id).and_then(|p| source(library, p)) {
                    let want_sketch = library.sketch(id).is_none();
                    self.submit_scan(id, source, want_sketch, Priority::PREFETCH);
                }
                continue;
            }
            if matches!(ladder::choose(need, self.screen, |s| self.memory.contains(&(id, s))), Choice::Covers(_)) {
                continue;
            }
            if let Some(job) = self.submit_fetch(library, id, want, Priority::PREFETCH) {
                next.insert(job);
            }
        }
        let stale: Vec<Job> = self.prefetching.difference(&next).copied().collect();
        for job in stale {
            if self.outstanding.get(&job) == Some(&Priority::PREFETCH)
                && let Some(pool) = &self.pool
                && pool.cancel(&job)
            {
                self.outstanding.remove(&job);
                self.stats.cancelled += 1;
            }
        }
        next.retain(|j| self.outstanding.get(j) == Some(&Priority::PREFETCH));
        self.prefetching = next;
        plan
    }

    /// Handles up to `max` finished jobs. Returns how many.
    pub(crate) fn poll(&mut self, library: &Library, max: usize) -> usize {
        let mut n = 0;
        while n < max {
            let Some((job, done)) = self.pool.as_ref().and_then(Pool::try_recv) else { break };
            self.handle(library, job, done);
            n += 1;
        }
        self.maintain_when_idle();
        n
    }

    /// Waits until every job has finished and is handled (scans, fetches, then the cache upkeep
    /// they make due).
    pub(crate) fn wait(&mut self, library: &Library) {
        let mut last = Instant::now();
        loop {
            self.maintain_when_idle();
            if self.outstanding.is_empty() {
                return;
            }
            let Some(pool) = &self.pool else { return };
            match pool.recv_timeout(Duration::from_millis(50)) {
                Some((job, done)) => {
                    self.handle(library, job, done);
                    last = Instant::now();
                }
                None if last.elapsed() > STALL => {
                    log::warn!("thumbnails: {} jobs made no progress for {} s; no longer waiting for them", self.outstanding.len(), STALL.as_secs());
                    self.outstanding.clear();
                    self.scans = 0;
                    return;
                }
                None => {}
            }
        }
    }

    /// Runs the cache upkeep (compaction, pruning, sync) once the scans a sync started are done.
    fn maintain_when_idle(&mut self) {
        if !self.maintain_due || self.scans > 0 {
            return;
        }
        self.maintain_due = false;
        let Some(cache) = self.cache.clone() else { return };
        self.submit(Job::Maintain, Priority::BACKGROUND, move |_| {
            Done::Maintained(cache.maintain().and_then(|m| cache.sync().map(|()| m)).map_err(|e| e.to_string()))
        });
    }

    fn handle(&mut self, library: &Library, job: Job, done: Done) {
        if self.outstanding.remove(&job).is_some() && matches!(job, Job::Scan(_)) {
            self.scans = self.scans.saturating_sub(1);
        }
        self.prefetching.remove(&job);
        match (job, done) {
            (Job::Scan(id), Done::Scanned { key, result, in_flight }) => {
                self.in_flight.fetch_sub(in_flight, Ordering::SeqCst);
                let Some(known) = self.photos.get(&id).map(|st| st.key) else { return };
                if known != key {
                    // The photo changed since the job was queued: if the library now says what the
                    // job saw, take its result; otherwise scan the file as the library has it.
                    let Some(current) = library.photo(id).and_then(|p| source(library, p)) else { return };
                    if current.key() != key {
                        if let Some(st) = self.photos.get_mut(&id) {
                            st.key = current.key();
                        }
                        self.forget_images(id);
                        self.submit_scan(id, current, true, Priority::BACKGROUND);
                        return;
                    }
                    self.forget_images(id);
                }
                let Some(st) = self.photos.get_mut(&id) else { return };
                st.key = key;
                match result {
                    Ok(scanned) => {
                        st.scanned = true;
                        st.failed = None;
                        self.stats.decodes += u64::from(scanned.decoded);
                        if let Some(sketch) = scanned.sketch.as_deref().and_then(Sketch::new) {
                            self.sketches.push((id, Some(sketch)));
                        }
                        for (size, image) in scanned.images {
                            self.remember(id, size, key, image);
                        }
                    }
                    Err(ladder::Error::Cancelled) => {}
                    Err(e) => st.failed = Some(e.to_string()),
                }
            }
            (Job::Fetch(id, _), Done::Fetched { key, size, result }) => {
                let Some(st) = self.photos.get_mut(&id) else { return };
                if st.key != key {
                    return;
                }
                match result {
                    Ok(fetched) => {
                        if fetched.decoded {
                            self.stats.decodes += 1;
                        } else {
                            self.stats.loads += 1;
                        }
                        let entry = ladder::entry_key(key, size, self.screen);
                        self.remember(id, size, entry, fetched.image);
                    }
                    Err(ladder::Error::Cancelled) => {}
                    Err(e) => st.failed = Some(e.to_string()),
                }
            }
            (_, Done::Maintained(Err(e))) => log::warn!("thumbnail cache upkeep: {e}"),
            (_, Done::Maintained(Ok(m))) => {
                if !m.compacted.is_empty() || !m.pruned.is_empty() {
                    log::info!("thumbnail cache: compacted {:?}, pruned {:?}, {} → {} bytes", m.compacted, m.pruned, m.before, m.after);
                }
            }
            (job, Done::Panicked(message)) => {
                log::error!("thumbnail job {job:?} panicked: {message}");
                if let Job::Scan(id) | Job::Fetch(id, _) = job
                    && let Some(st) = self.photos.get_mut(&id)
                {
                    st.failed = Some(format!("the thumbnailer failed: {message}"));
                }
            }
            (job, _) => log::warn!("thumbnail job {job:?} delivered the wrong result"),
        }
    }

    fn remember(&mut self, id: PhotoId, size: Size, key: Key, image: Rgba8) {
        let bytes = image.data.len();
        self.memory.insert((id, size), (key, Arc::new(image)), bytes);
    }

    /// Sketches made since the last call, oldest first.
    pub(crate) fn take_sketches(&mut self) -> Vec<(PhotoId, Option<Sketch>)> {
        std::mem::take(&mut self.sketches)
    }

    pub(crate) fn failure(&self, id: PhotoId) -> Option<&str> {
        self.photos.get(&id).and_then(|s| s.failed.as_deref())
    }

    pub(crate) fn set_screen(&mut self, screen: u32) {
        self.screen = screen.clamp(1, ladder::MAX_SCREEN);
    }

    pub(crate) fn pending(&self) -> usize {
        self.outstanding.len()
    }

    pub(crate) fn stats(&self) -> ThumbStats {
        ThumbStats {
            jobs: self.outstanding.len(),
            prefetching: self.prefetching.len(),
            scanned: self.photos.values().filter(|s| s.scanned).count(),
            failed: self.photos.values().filter(|s| s.failed.is_some()).count(),
            memory: self.memory.used(),
            ..self.stats
        }
    }
}

impl Drop for Thumbs {
    /// Stops the workers, closes the cache, then removes a temporary cache folder on a thread of its
    /// own (the session may be dropped on the UI thread, which never touches the disk).
    fn drop(&mut self) {
        drop(self.pool.take());
        drop(self.cache.take());
        if let Some(temp) = self.temp.take() {
            let removed = std::thread::Builder::new().name("dalica-thumbs-close".into()).spawn(move || temp.close());
            match removed.map(std::thread::JoinHandle::join) {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(e))) => log::warn!("the temporary thumbnail cache was not removed: {e}"),
                _ => log::warn!("the temporary thumbnail cache was not removed"),
            }
        }
    }
}

/// The sketch, else the placeholder.
fn fallback(library: &Library, id: PhotoId) -> Thumb {
    library.sketch(id).map_or(Thumb::NotScanned, |s| Thumb::Sketch(*s))
}

/// The file of `photo` as its cache key sees it.
fn source(library: &Library, photo: &Photo) -> Option<Source> {
    let folder = library.folder(photo.folder)?;
    Some(Source { path: folder.path.join(&photo.name), file_size: photo.size, mtime_ms: photo.mtime.as_millis() })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::default_cache_dir;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn dalica_cache_dir_replaces_the_platform_cache_folder() {
        const ROOT: &str = if cfg!(windows) { r"C:\t\cache" } else { "/t/cache" };
        const LOCAL: (&str, &str) = ("LOCALAPPDATA", r"C:\Users\a\AppData\Local");
        let all = env(&[("DALICA_CACHE_DIR", ROOT), ("HOME", "/home/a"), ("XDG_CACHE_HOME", "/c"), LOCAL]);
        assert_eq!(default_cache_dir(all), Some(PathBuf::from(ROOT).join("thumbs")));
        // Empty or relative: ignored, the platform folder is used.
        for value in ["", "cache"] {
            let platform = default_cache_dir(env(&[("HOME", "/home/a"), LOCAL]));
            let given: &'static [(&str, &str)] = if value.is_empty() {
                &[("DALICA_CACHE_DIR", ""), ("HOME", "/home/a"), LOCAL]
            } else {
                &[("DALICA_CACHE_DIR", "cache"), ("HOME", "/home/a"), LOCAL]
            };
            assert!(platform.is_some());
            assert_eq!(default_cache_dir(env(given)), platform, "{value:?}");
        }
    }

    #[test]
    fn the_cache_folder_follows_the_platform() {
        if cfg!(windows) {
            assert_eq!(
                default_cache_dir(env(&[("LOCALAPPDATA", r"C:\Users\a\AppData\Local")])),
                Some(PathBuf::from(r"C:\Users\a\AppData\Local\Dalica\cache\thumbs"))
            );
            assert_eq!(default_cache_dir(env(&[])), None);
        } else if cfg!(target_os = "macos") {
            assert_eq!(default_cache_dir(env(&[("HOME", "/Users/a")])), Some(PathBuf::from("/Users/a/Library/Caches/Dalica/thumbs")));
        } else {
            assert_eq!(default_cache_dir(env(&[("HOME", "/home/a")])), Some(PathBuf::from("/home/a/.cache/dalica/thumbs")));
            assert_eq!(default_cache_dir(env(&[("HOME", "/home/a"), ("XDG_CACHE_HOME", "/c")])), Some(PathBuf::from("/c/dalica/thumbs")));
            assert_eq!(default_cache_dir(env(&[("HOME", "relative")])), None, "a relative folder is no cache folder");
            assert_eq!(default_cache_dir(env(&[])), None);
        }
    }
}
