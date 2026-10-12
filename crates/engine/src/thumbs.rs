//! The session's thumbnails (`architecture.md` §5, §6): the size ladder in the packed disk cache,
//! scan-time generation, answers that never wait, direction-aware prefetch, and the cache's upkeep.
//!
//! - **After a scan** adds or changes photos, a background job per photo makes its 128 and 256
//!   entries and its colour sketch from one decode ([`dalica_thumbs::ladder::scan_with`]). Sketches
//!   come back to the owner, which applies them in batches as derived ops (journaled, never on the
//!   undo stack, no sidecar write). Opening a library checks every photo the same way; a photo
//!   whose entries are cached is not decoded again. [`Thumbs::sync`] looks only at the folders that
//!   changed since it last ran (their revision or path), so a scan landing batch after batch costs
//!   a pass over the folders and the new photos, not over the library's keys.
//! - **The first screenful first:** a cell on screen ([`Session::thumbnail`](crate::Session::thumbnail))
//!   raises its photo's scan to the visible priority, and while the photo has nothing in memory a
//!   quicker job above it decodes the file's embedded thumbnail (EXIF / camera preview), shown at
//!   once when it covers the cell; the scan's proper downscale replaces it.
//! - **512, 1024 and the screen-size preview** are made on demand ([`dalica_thumbs::ladder::fetch_with`]).
//! - **[`Session::thumbnail`](crate::Session::thumbnail)** answers at once from memory: the size
//!   that covers the cell, else the next smaller one to scale up, else the sketch, else
//!   [`Thumb::NotScanned`], and queues what is missing at the visible priority.
//! - **[`Session::prefetch_thumbnails`](crate::Session::prefetch_thumbnails)** loads the cells
//!   around the visible ones, in the scroll direction and further ahead the faster the view moves;
//!   prefetch the view has moved away from is cancelled.
//! - **Colour management** (View → Use Color Management, `view.colorManagement`, saved in
//!   `settings.json`) chooses the decode options and joins the cache key
//!   ([`dalica_thumbs::ladder::Render`]); switching it forgets the images in memory, so the cells on
//!   screen are made again (or read from the cache, if they were made that way before).
//! - **A photo that cannot be shown** answers its `formats.md` §1 reason
//!   ([`Session::thumbnail_failure`](crate::Session::thumbnail_failure)), remembered by its file
//!   (path, size, modification time) and not tried again until the file changes. A photo whose
//!   folder cannot be reached (an offline volume) is not a failure: it keeps its cached entries,
//!   draws what it has, and is never made again until its folder is back.
//! - **Upkeep** ([`dalica_thumbs::upkeep`]): after scans settle, a background job compacts packs
//!   past 25 % dead; at start and whenever a worker sees the cache past its size (the cache size
//!   setting, 2 GiB by default), a pruning job takes it down to 80 %, largest sizes and oldest
//!   entries first, keeping the entries of photos whose folder cannot be reached. Entries of photos
//!   that left the library, or whose file changed, are made dead as the session notices.
//! - The **cache directory** comes from the session's options: Dalica's cache folder
//!   (`<cache>/thumbs`, [`default_cache_dir`]) in the apps, a temporary folder removed with the
//!   session in tests and demos. A cache another process holds, or one that cannot be opened, falls
//!   back to a temporary folder (with a notice).
//!
//! The owner thread only submits jobs, moves finished images into a byte-budgeted memory cache
//! and answers from it: it never touches the disk, never decodes and never takes the cache's locks.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{FolderId, PhotoId};
use dalica_library::{Library, Photo, Sketch};
use dalica_thumbs::ladder::{self, Choice, Render, Source};
pub use dalica_thumbs::pack::Size;
use dalica_thumbs::pack::{Cache, Key};
use dalica_thumbs::upkeep::{self, Budget};
use dalica_thumbs::{Lru, Pool, Priority, Rgba8, Submitted};

use crate::session::Session;

/// Bytes of decoded thumbnails kept in memory by default.
pub const MEMORY: usize = 192 << 20;
/// Bytes of embedded thumbnails kept in memory while their photos' scans run.
const QUICK_MEMORY: usize = 32 << 20;
/// Sketches applied per op.
pub const SKETCH_BATCH: usize = 512;
/// Finished jobs handled per [`Session::poll`](crate::Session::poll), so a frame stays short.
pub const POLL_MAX: usize = 256;
/// Bytes of scan images on their way to the owner; past it, scans send their sketch only, so a
/// large scan cannot pile images up faster than the owner takes them.
const IN_FLIGHT: usize = 96 << 20;
/// How long [`Thumbs::wait`] waits without any job finishing before it gives up on the rest.
const STALL: Duration = Duration::from_secs(120);
/// How long a photo whose folder could not be reached waits before a cell may ask for it again.
pub const UNAVAILABLE_RETRY: Duration = Duration::from_secs(30);
/// The embedded-thumbnail job runs just above the visible cells' scans.
const QUICK: Priority = Priority(Priority::VISIBLE.0 + 5);

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
    /// The cache's disk budget in bytes, in place of the cache size setting (tests and benches;
    /// not clamped). `None`: the setting.
    pub budget: Option<u64>,
}

impl Default for ThumbOptions {
    fn default() -> ThumbOptions {
        ThumbOptions {
            cache: CacheDir::Default,
            memory: MEMORY,
            workers: dalica_thumbs::default_workers(),
            screen: ladder::DEFAULT_SCREEN,
            budget: None,
        }
    }
}

/// What a cell can draw now.
#[derive(Clone, Debug, PartialEq)]
pub enum Thumb {
    /// A ladder image: it covers the cell when `covers`; otherwise it is the next smaller size, to
    /// scale up while the covering one loads. (An embedded thumbnail shown while the photo's scan
    /// runs comes as the size it stands in for.)
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
    /// Photos whose folder could not be reached when last asked for.
    pub unavailable: usize,
    /// Bytes of decoded thumbnails in memory.
    pub memory: usize,
    /// Embedded thumbnails shown while their photos' scans ran.
    pub quick: u64,
    /// The cache's disk bytes after the last upkeep job (0 before one ran).
    pub disk_bytes: u64,
    /// Entries pruning removed, and entries it kept because their photo's folder could not be
    /// reached, since the session opened.
    pub pruned: u64,
    pub kept: u64,
    /// Pruning jobs run.
    pub prunes: u64,
    /// Entries made dead because their photo left the library or its file changed.
    pub forgotten: u64,
}

/// Generations [`Session::thumbnails_refreshed_since`](crate::Session::thumbnails_refreshed_since)
/// remembers.
const REFRESH_LOG: usize = 16;

/// What was made again since a generation ([`Session::thumbnail_generation`](crate::Session::thumbnail_generation)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refreshed {
    Nothing,
    /// Every photo (the rendering changed, or too long ago to say).
    All,
    /// These photos.
    Photos(Vec<PhotoId>),
}

/// One job, de-duplicated by the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Job {
    Scan(PhotoId),
    Fetch(PhotoId, Size),
    /// The embedded thumbnail of a photo on screen.
    Quick(PhotoId),
    /// Compaction, then a check of the budget.
    Maintain,
    Prune,
    /// Making dead the entries of photos that left (numbered: each batch is its own job).
    Forget(u64),
}

/// A finished job.
enum Done {
    Scanned { key: Key, result: Result<ladder::Scanned, ladder::Error>, in_flight: usize, unavailable: bool },
    Fetched { key: Key, size: Size, result: Result<ladder::Fetched, ladder::Error>, unavailable: bool },
    Quick { key: Key, image: Option<Rgba8> },
    Maintained { result: Result<(), String>, over: bool, disk: u64 },
    Pruned(Result<upkeep::Pruned, String>),
    Forgotten(Result<usize, String>),
    Panicked(String),
}

/// What the owner knows of one photo.
struct PhotoState {
    /// Its key under the [`Render`] of `epoch`.
    key: Key,
    /// The rendering epoch its key and `scanned` belong to ([`Thumbs::fresh`] brings them up to
    /// date when the rendering switched since).
    epoch: u32,
    folder: FolderId,
    /// Its 128 and 256 entries are in the cache (under the current render).
    scanned: bool,
    /// Why it cannot be shown (`formats.md` §1), until its file changes.
    failed: Option<String>,
    /// Its folder could not be reached when last asked for (an offline volume).
    unavailable: Option<Instant>,
    /// Its embedded thumbnail was asked for.
    quick_asked: bool,
}

impl PhotoState {
    fn new(key: Key, folder: FolderId, epoch: u32) -> PhotoState {
        PhotoState { key, epoch, folder, scanned: false, failed: None, unavailable: None, quick_asked: false }
    }

    /// Whether a cell may ask for its images now.
    fn askable(&self) -> bool {
        self.failed.is_none() && self.unavailable.is_none_or(|t| t.elapsed() >= UNAVAILABLE_RETRY)
    }
}

/// The session's thumbnailer (owned by the session's thread).
pub(crate) struct Thumbs {
    /// `None` when thumbnails are off or no cache could be opened.
    pool: Option<Pool<Job, Done>>,
    cache: Option<Arc<Cache>>,
    temp: Option<dalica_fs::TempDir>,
    photos: HashMap<PhotoId, PhotoState>,
    /// The folders [`Thumbs::sync`] saw: their revision and path, and their photos.
    folders: HashMap<FolderId, (u64, PathBuf)>,
    by_folder: HashMap<FolderId, HashSet<PhotoId>>,
    memory: Lru<(PhotoId, Size), (Key, Arc<Rgba8>)>,
    /// Embedded thumbnails of photos on screen whose scans are still running.
    quick: Lru<PhotoId, (Key, Arc<Rgba8>)>,
    /// Jobs queued or running, with the highest priority asked for.
    outstanding: HashMap<Job, Priority>,
    scans: usize,
    prefetching: HashSet<Job>,
    sketches: Vec<(PhotoId, Option<Sketch>)>,
    in_flight: Arc<AtomicUsize>,
    maintain_due: bool,
    /// A worker saw the cache past its budget.
    over: Arc<AtomicBool>,
    /// The budget in bytes, read by the workers.
    budget: Arc<AtomicU64>,
    /// The budget comes from the session's options, not the setting.
    budget_fixed: bool,
    forget_seq: u64,
    /// Goes up whenever images already handed out are made again (rendering switched, a refresh).
    generation: u64,
    /// The last few generations and what they made again (`None`: everything), oldest first.
    refreshed: std::collections::VecDeque<(u64, Option<Vec<PhotoId>>)>,
    render: Render,
    /// Bumped by each switch of `render`.
    epoch: u32,
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
        // The pack's own pruning is never used (upkeep prunes), so its budget does not matter.
        let pack_options = dalica_thumbs::pack::Options { budget: u64::MAX, ..dalica_thumbs::pack::Options::default() };
        let mut cache = at.and_then(|dir| match Cache::open(&dir, pack_options) {
            Ok(c) => Some(c),
            Err(e) => {
                notices.push(format!("the thumbnail cache {} cannot be used ({e}); thumbnails are kept in a temporary folder", dir.display()));
                None
            }
        });
        if cache.is_none() && options.cache != CacheDir::Off {
            match dalica_fs::TempDir::new("dalica-thumbs").map_err(|e| e.to_string()).and_then(|t| {
                let c = Cache::open(t.path(), pack_options).map_err(|e| e.to_string())?;
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
            folders: HashMap::new(),
            by_folder: HashMap::new(),
            memory: Lru::new(options.memory),
            quick: Lru::new(QUICK_MEMORY),
            outstanding: HashMap::new(),
            scans: 0,
            prefetching: HashSet::new(),
            sketches: Vec::new(),
            in_flight: Arc::new(AtomicUsize::new(0)),
            maintain_due: false,
            over: Arc::new(AtomicBool::new(false)),
            budget: Arc::new(AtomicU64::new(options.budget.unwrap_or(dalica_thumbs::pack::DEFAULT_BUDGET))),
            budget_fixed: options.budget.is_some(),
            forget_seq: 0,
            generation: 0,
            refreshed: std::collections::VecDeque::new(),
            render: Render::default(),
            epoch: 0,
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

    fn budget(&self) -> Budget {
        Budget { bytes: self.budget.load(Ordering::Relaxed), ..Budget::default() }
    }

    /// Takes the cache size setting (unless the session's options fixed the budget); a smaller
    /// budget the cache is already past is pruned to at once.
    pub(crate) fn set_budget(&mut self, bytes: u64) {
        if self.budget_fixed {
            return;
        }
        let bytes = Budget::of(bytes).bytes;
        if self.budget.swap(bytes, Ordering::Relaxed) > bytes {
            self.maintain_due = true;
        }
    }

    pub(crate) fn render(&self) -> Render {
        self.render
    }

    /// Renders with `render` from now on: the images in memory go (the cells on screen ask again
    /// and get the new rendering), and each photo's key becomes its key under it, and its entries
    /// count as not made, the next time the photo is looked at ([`Thumbs::fresh`]: a switch costs
    /// no pass over the library). Failures stay: they are about the file. Jobs already queued
    /// finish the old way; their images are dropped.
    pub(crate) fn set_render(&mut self, render: Render) {
        if render == self.render {
            return;
        }
        self.render = render;
        self.epoch = self.epoch.wrapping_add(1);
        self.bump(None);
        self.memory = Lru::new(self.memory.budget());
        self.quick = Lru::new(QUICK_MEMORY);
    }

    /// Brings photo `id`'s key up to the current rendering, if it was switched since the photo was
    /// last looked at.
    fn fresh(&mut self, library: &Library, id: PhotoId) {
        let (epoch, render) = (self.epoch, self.render);
        if let Some(st) = self.photos.get_mut(&id)
            && st.epoch != epoch
        {
            if let Some(source) = library.photo(id).and_then(|p| source(library, p)) {
                st.key = source.key_for(&render);
            }
            st.epoch = epoch;
            st.scanned = false;
            st.quick_asked = false;
        }
    }

    /// Brings the thumbnailer in line with the library after it was opened or scanned. Only the
    /// folders whose revision or path changed since the last call are looked at: a photo in one of
    /// them it has not seen, or whose file changed, gets a background scan job (its sketch too
    /// when it has none or the file changed); photos that left (and the old entries of changed
    /// files) are forgotten and their cache entries made dead. One pass over the folders, and one
    /// pass over the photos only when a folder changed.
    pub(crate) fn sync(&mut self, library: &Library) {
        let mut changed: HashSet<FolderId> = HashSet::new();
        for f in library.folders() {
            let revision = library.folder_revision(f.id);
            if self.folders.get(&f.id).is_none_or(|(r, p)| *r != revision || *p != f.path) {
                changed.insert(f.id);
                self.folders.insert(f.id, (revision, f.path.clone()));
            }
        }
        let gone: Vec<FolderId> = self.folders.keys().filter(|id| library.folder(**id).is_none()).copied().collect();
        if changed.is_empty() && gone.is_empty() {
            return;
        }
        for id in &gone {
            self.folders.remove(id);
        }
        let mut lists: HashMap<FolderId, HashSet<PhotoId>> = changed.iter().map(|id| (*id, HashSet::new())).collect();
        let mut dead: Vec<Key> = Vec::new();
        for photo in library.photos() {
            let Some(list) = lists.get_mut(&photo.folder) else { continue };
            list.insert(photo.id);
            let Some(source) = source(library, photo) else { continue };
            let key = source.key_for(&self.render);
            let epoch = self.epoch;
            let changed_file = match self.photos.get_mut(&photo.id) {
                Some(st) if st.epoch != epoch && st.key != key => {
                    // Keyed for the other rendering: the same file, its entries not made this way.
                    st.key = key;
                    st.epoch = epoch;
                    st.scanned = false;
                    st.quick_asked = false;
                    st.folder = photo.folder;
                    continue;
                }
                Some(st) if st.key == key => {
                    st.epoch = epoch;
                    st.folder = photo.folder;
                    continue;
                }
                Some(st) => {
                    dead.push(st.key);
                    true
                }
                None => false,
            };
            if changed_file {
                self.forget_images(photo.id);
            }
            self.photos.insert(photo.id, PhotoState::new(key, photo.folder, self.epoch));
            let want_sketch = changed_file || library.sketch(photo.id).is_none();
            self.submit_scan(photo.id, source, want_sketch, Priority::BACKGROUND, false);
        }
        // Photos of the changed and gone folders that are no longer there.
        let seen: HashSet<PhotoId> = lists.values().flatten().copied().collect();
        let mut left: Vec<PhotoId> = Vec::new();
        for folder in changed.iter().chain(&gone) {
            if let Some(ids) = self.by_folder.remove(folder) {
                left.extend(ids.into_iter().filter(|id| !seen.contains(id)));
            }
        }
        for id in left {
            if let Some(st) = self.photos.remove(&id) {
                dead.push(st.key);
            }
            self.forget_images(id);
        }
        self.by_folder.extend(lists);
        if !dead.is_empty() {
            // A key another photo still has stays (a rebuild gives the same files new ids).
            let live: HashSet<Key> = self.photos.values().map(|st| st.key).collect();
            let screen = self.screen;
            let entries: Vec<(Size, Key)> = dead.into_iter().filter(|k| !live.contains(k)).flat_map(|k| entries_of(k, screen)).collect();
            self.forget(entries);
        }
        self.maintain_due = true;
    }

    /// Makes `entries` dead in the cache, on a worker.
    fn forget(&mut self, entries: Vec<(Size, Key)>) {
        if entries.is_empty() {
            return;
        }
        let Some(cache) = self.cache.clone() else { return };
        self.forget_seq = self.forget_seq.wrapping_add(1);
        self.submit(Job::Forget(self.forget_seq), Priority::BACKGROUND, move |_| {
            Done::Forgotten(upkeep::forget(&cache, &entries).map_err(|e| e.to_string()))
        });
    }

    /// Starts tracking one photo the last [`Thumbs::sync`] did not see (an undo brought it back).
    fn track(&mut self, library: &Library, id: PhotoId, priority: Priority) {
        let Some(photo) = library.photo(id) else { return };
        let Some(source) = source(library, photo) else { return };
        self.photos.insert(id, PhotoState::new(source.key_for(&self.render), photo.folder, self.epoch));
        self.submit_scan(id, source, library.sketch(id).is_none(), priority, false);
    }

    fn forget_images(&mut self, id: PhotoId) {
        for size in Size::ALL {
            self.memory.remove(&(id, size));
        }
        self.quick.remove(&id);
    }

    /// Queues the scan of one photo; `refresh` first makes its entries dead, so they are made
    /// again (unless its folder cannot be reached: then nothing is touched).
    fn submit_scan(&mut self, id: PhotoId, source: Source, want_sketch: bool, priority: Priority, refresh: bool) {
        let Some(cache) = self.cache.clone() else { return };
        let (limits, in_flight, render, screen) = (self.limits, Arc::clone(&self.in_flight), self.render, self.screen);
        let (over, budget) = (Arc::clone(&self.over), Arc::clone(&self.budget));
        self.submit(Job::Scan(id), priority, move |cancel| {
            let key = source.key_for(&render);
            if refresh {
                if !reachable(&source.path) {
                    let result = Err(ladder::Error::Photo(dalica_thumbs::Error::Io("its folder cannot be reached".into())));
                    return Done::Scanned { key, result, in_flight: 0, unavailable: true };
                }
                if let Err(e) = upkeep::forget(&cache, &entries_of(key, screen)) {
                    return Done::Scanned { key, result: Err(e.into()), in_flight: 0, unavailable: false };
                }
            }
            let mut result = ladder::scan_with(&cache, &source, &render, want_sketch || refresh, &limits, cancel);
            let unavailable = is_unreachable(&result, &source);
            let mut bytes = 0;
            if let Ok(s) = &mut result {
                bytes = s.images.iter().map(|(_, i)| i.data.len()).sum();
                if in_flight.fetch_add(bytes, Ordering::SeqCst).saturating_add(bytes) > IN_FLIGHT {
                    in_flight.fetch_sub(bytes, Ordering::SeqCst);
                    s.images.clear();
                    bytes = 0;
                }
                note_budget(&cache, &budget, &over, s.decoded);
            }
            Done::Scanned { key, result, in_flight: bytes, unavailable }
        });
    }

    fn submit_fetch(&mut self, library: &Library, id: PhotoId, size: Size, priority: Priority) -> Option<Job> {
        let job = Job::Fetch(id, size);
        let source = library.photo(id).and_then(|p| source(library, p))?;
        let cache = self.cache.clone()?;
        let (limits, screen, render) = (self.limits, self.screen, self.render);
        let (over, budget) = (Arc::clone(&self.over), Arc::clone(&self.budget));
        self.submit(job, priority, move |cancel| {
            let key = source.key_for(&render);
            let result = ladder::fetch_with(&cache, &source, &render, size, screen, &limits, cancel);
            let unavailable = is_unreachable(&result, &source);
            if let Ok(f) = &result {
                note_budget(&cache, &budget, &over, f.decoded);
            }
            Done::Fetched { key, size, result, unavailable }
        });
        Some(job)
    }

    /// Queues the embedded thumbnail of a photo on screen, once.
    fn submit_quick(&mut self, library: &Library, id: PhotoId, need: u32) {
        let Some(source) = library.photo(id).and_then(|p| source(library, p)) else { return };
        if self.cache.is_none() {
            return;
        }
        let (limits, render) = (self.limits, self.render);
        if let Some(st) = self.photos.get_mut(&id) {
            st.quick_asked = true;
        }
        self.submit(Job::Quick(id), QUICK, move |_| Done::Quick {
            key: source.key_for(&render),
            image: embedded(&source.path, need, &render, &limits),
        });
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
    /// covering size (and a smaller one first when nothing is in memory) at the visible priority,
    /// and the embedded thumbnail above them while the photo is not scanned yet.
    pub(crate) fn request(&mut self, library: &Library, id: PhotoId, need: u32) -> Thumb {
        if library.photo(id).is_none() {
            return Thumb::NotScanned;
        }
        if !self.photos.contains_key(&id) {
            self.track(library, id, Priority::VISIBLE);
        }
        self.fresh(library, id);
        let Some(st) = self.photos.get(&id) else { return fallback(library, id) };
        let (key, scanned, askable, quick_asked) = (st.key, st.scanned, st.askable(), st.quick_asked);
        let want = ladder::covering(need, self.screen);
        let found = self.best(id, key, need);
        let covers = matches!(found, Some((_, _, true)));
        if askable && !covers {
            if !scanned {
                if found.is_none() && !quick_asked && !self.quick.contains(&id) {
                    self.submit_quick(library, id, need);
                }
                if let Some(source) = library.photo(id).and_then(|p| source(library, p)) {
                    self.submit_scan(id, source, library.sketch(id).is_none(), Priority::VISIBLE, false);
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
            None => self.quick_image(id, key, need).unwrap_or_else(|| fallback(library, id)),
        }
    }

    /// What photo `id` draws now, without asking for anything (benches and tests).
    pub(crate) fn peek(&mut self, library: &Library, id: PhotoId, need: u32) -> Thumb {
        self.fresh(library, id);
        let Some(key) = self.photos.get(&id).map(|s| s.key) else { return fallback(library, id) };
        match self.best(id, key, need) {
            Some((size, image, covers)) => Thumb::Image { size, image, covers },
            None => self.quick_image(id, key, need).unwrap_or_else(|| fallback(library, id)),
        }
    }

    /// The embedded thumbnail in memory, as the size it stands in for, when it covers the cell.
    fn quick_image(&mut self, id: PhotoId, key: Key, need: u32) -> Option<Thumb> {
        let (k, image) = self.quick.get(&id)?;
        if *k != key || image.width.max(image.height) < need {
            return None;
        }
        Some(Thumb::Image { size: ladder::covering(need, self.screen), image: Arc::clone(image), covers: true })
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
                // Made for another screen size or rendering: drop it and look again.
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
            self.fresh(library, id);
            let Some(st) = self.photos.get(&id) else { continue };
            if !st.askable() {
                continue;
            }
            if !st.scanned {
                // Raise its background scan; a scan is never cancelled.
                if let Some(source) = library.photo(id).and_then(|p| source(library, p)) {
                    let want_sketch = library.sketch(id).is_none();
                    self.submit_scan(id, source, want_sketch, Priority::PREFETCH, false);
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

    /// Makes the thumbnails of `ids` again: their entries are made dead and each photo is scanned
    /// anew (its sketch too), failures tried again. Photos whose folder cannot be reached keep what
    /// they have. Returns how many were queued.
    pub(crate) fn refresh(&mut self, library: &Library, ids: &[PhotoId]) -> usize {
        let mut n = 0;
        for &id in ids {
            let Some(photo) = library.photo(id) else { continue };
            let Some(source) = source(library, photo) else { continue };
            // A scan queued the ordinary way would not make the entries again: replace it.
            if self.outstanding.contains_key(&Job::Scan(id))
                && let Some(pool) = &self.pool
                && pool.cancel(&Job::Scan(id))
            {
                self.outstanding.remove(&Job::Scan(id));
                self.scans = self.scans.saturating_sub(1);
            }
            self.forget_images(id);
            self.photos.insert(id, PhotoState::new(source.key_for(&self.render), photo.folder, self.epoch));
            self.submit_scan(id, source, true, Priority::BACKGROUND, true);
            n += 1;
        }
        if n > 0 {
            self.bump(Some(ids.to_vec()));
        }
        n
    }

    /// A new generation that made `photos` again (`None`: every photo).
    fn bump(&mut self, photos: Option<Vec<PhotoId>>) {
        self.generation = self.generation.saturating_add(1);
        self.refreshed.push_back((self.generation, photos));
        while self.refreshed.len() > REFRESH_LOG {
            self.refreshed.pop_front();
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// What was made again after generation `since`.
    pub(crate) fn refreshed_since(&self, since: u64) -> Refreshed {
        if since >= self.generation {
            return Refreshed::Nothing;
        }
        // Older than the log remembers: everything.
        if self.refreshed.front().is_none_or(|(g, _)| *g > since.saturating_add(1)) {
            return Refreshed::All;
        }
        let mut ids: HashSet<PhotoId> = HashSet::new();
        for (_, photos) in self.refreshed.iter().filter(|(g, _)| *g > since) {
            match photos {
                None => return Refreshed::All,
                Some(p) => ids.extend(p.iter().copied()),
            }
        }
        let mut ids: Vec<PhotoId> = ids.into_iter().collect();
        ids.sort();
        Refreshed::Photos(ids)
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

    /// Runs the cache upkeep once the scans a sync started are done (compaction, then a check of
    /// the budget), and pruning as soon as a worker saw the cache past its budget.
    fn maintain_when_idle(&mut self) {
        if self.over.swap(false, Ordering::Relaxed) {
            self.submit_prune(keep_list(&self.photos, &self.folders, self.screen));
        }
        if !self.maintain_due || self.scans > 0 {
            return;
        }
        self.maintain_due = false;
        let Some(cache) = self.cache.clone() else { return };
        let budget = Arc::clone(&self.budget);
        self.submit(Job::Maintain, Priority::BACKGROUND, move |_| {
            let mut result = Ok(());
            for size in Size::ALL {
                if cache.compaction_due(size)
                    && let Err(e) = cache.compact(size)
                {
                    result = Err(e.to_string());
                }
            }
            let result = result.and_then(|()| cache.sync().map_err(|e| e.to_string()));
            let disk = cache.disk_bytes();
            Done::Maintained { result, over: disk > budget.load(Ordering::Relaxed), disk }
        });
    }

    /// Queues the pruning job; `keep` lists, per folder, the entries of its photos, kept when the
    /// folder cannot be reached.
    fn submit_prune(&mut self, keep: Vec<(PathBuf, Vec<Key>)>) {
        let Some(cache) = self.cache.clone() else { return };
        let budget = self.budget();
        self.submit(Job::Prune, Priority::BACKGROUND, move |_| {
            let mut kept: HashSet<Key> = HashSet::new();
            for (folder, keys) in keep {
                if !folder.is_dir() {
                    kept.extend(keys);
                }
            }
            Done::Pruned(upkeep::prune(&cache, &budget, &|_, k| kept.contains(&k)).map_err(|e| e.to_string()))
        });
    }

    fn handle(&mut self, library: &Library, job: Job, done: Done) {
        if self.outstanding.remove(&job).is_some() && matches!(job, Job::Scan(_)) {
            self.scans = self.scans.saturating_sub(1);
        }
        self.prefetching.remove(&job);
        match (job, done) {
            (Job::Scan(id), Done::Scanned { key, result, in_flight, unavailable }) => {
                self.in_flight.fetch_sub(in_flight, Ordering::SeqCst);
                self.fresh(library, id);
                let Some(known) = self.photos.get(&id).map(|st| st.key) else { return };
                if known != key {
                    let Some(current) = library.photo(id).and_then(|p| source(library, p)).map(|s| s.key_for(&self.render)) else { return };
                    if current == known {
                        // Made the other way (colour management was switched meanwhile): only its
                        // sketch is of use.
                        if let Ok(scanned) = &result
                            && library.sketch(id).is_none()
                            && let Some(sketch) = scanned.sketch.as_deref().and_then(Sketch::new)
                        {
                            self.sketches.push((id, Some(sketch)));
                        }
                        return;
                    }
                    if current != key {
                        // The photo changed since the job was queued: scan the file as the library
                        // has it now.
                        if let Some(st) = self.photos.get_mut(&id) {
                            st.key = current;
                        }
                        self.forget_images(id);
                        if let Some(source) = library.photo(id).and_then(|p| source(library, p)) {
                            self.submit_scan(id, source, true, Priority::BACKGROUND, false);
                        }
                        return;
                    }
                    // The library now says what the job saw: take its result.
                    self.forget_images(id);
                }
                let Some(st) = self.photos.get_mut(&id) else { return };
                st.key = key;
                match result {
                    Ok(scanned) => {
                        st.scanned = true;
                        st.failed = None;
                        st.unavailable = None;
                        self.quick.remove(&id);
                        self.stats.decodes += u64::from(scanned.decoded);
                        if let Some(sketch) = scanned.sketch.as_deref().and_then(Sketch::new) {
                            self.sketches.push((id, Some(sketch)));
                        }
                        for (size, image) in scanned.images {
                            self.remember(id, size, key, image);
                        }
                    }
                    Err(ladder::Error::Cancelled) => {}
                    Err(_) if unavailable => st.unavailable = Some(Instant::now()),
                    Err(e) => st.failed = Some(reason(&e)),
                }
            }
            (Job::Fetch(id, _), Done::Fetched { key, size, result, unavailable }) => {
                self.fresh(library, id);
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
                    Err(_) if unavailable => st.unavailable = Some(Instant::now()),
                    Err(e) => st.failed = Some(reason(&e)),
                }
            }
            (Job::Quick(id), Done::Quick { key, image }) => {
                self.fresh(library, id);
                let Some(st) = self.photos.get(&id) else { return };
                if st.key != key || st.scanned {
                    return;
                }
                if let Some(image) = image {
                    self.stats.quick += 1;
                    let bytes = image.data.len();
                    self.quick.insert(id, (key, Arc::new(image)), bytes);
                }
            }
            (_, Done::Maintained { result, over, disk }) => {
                self.stats.disk_bytes = disk;
                if let Err(e) = result {
                    log::warn!("thumbnail cache upkeep: {e}");
                }
                if over {
                    let keep = keep_list(&self.photos, &self.folders, self.screen);
                    self.submit_prune(keep);
                }
            }
            (_, Done::Pruned(Ok(p))) => {
                self.stats.prunes += 1;
                self.stats.pruned += p.removed;
                self.stats.kept += p.kept;
                self.stats.disk_bytes = p.after;
                if p.removed > 0 {
                    log::info!("thumbnail cache: pruned {} entries ({:?}), kept {}, {} → {} bytes", p.removed, p.sizes, p.kept, p.before, p.after);
                    // Photos whose 128 or 256 entries went are made again when a cell asks.
                    for st in self.photos.values_mut() {
                        st.scanned = false;
                    }
                }
            }
            (_, Done::Pruned(Err(e))) => log::warn!("thumbnail cache pruning: {e}"),
            (_, Done::Forgotten(Ok(n))) => self.stats.forgotten += u64::try_from(n).unwrap_or(u64::MAX),
            (_, Done::Forgotten(Err(e))) => log::warn!("thumbnail cache: {e}"),
            (job, Done::Panicked(message)) => {
                log::error!("thumbnail job {job:?} panicked: {message}");
                if let Job::Scan(id) | Job::Fetch(id, _) = job
                    && let Some(st) = self.photos.get_mut(&id)
                {
                    st.failed = Some("Damaged file".into());
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
            scanned: self.photos.values().filter(|s| s.scanned && s.epoch == self.epoch).count(),
            failed: self.photos.values().filter(|s| s.failed.is_some()).count(),
            unavailable: self.photos.values().filter(|s| s.unavailable.is_some()).count(),
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

/// Every entry a photo whose key is `key` can have.
fn entries_of(key: Key, screen: u32) -> [(Size, Key); 5] {
    Size::ALL.map(|s| (s, ladder::entry_key(key, s, screen)))
}

/// Per folder, the entry keys of its photos: what pruning keeps when the folder cannot be reached
/// (checked on the worker).
fn keep_list(photos: &HashMap<PhotoId, PhotoState>, folders: &HashMap<FolderId, (u64, PathBuf)>, screen: u32) -> Vec<(PathBuf, Vec<Key>)> {
    let mut by: HashMap<FolderId, Vec<Key>> = HashMap::new();
    for st in photos.values() {
        by.entry(st.folder).or_default().extend(entries_of(st.key, screen).map(|(_, k)| k));
    }
    by.into_iter().filter_map(|(f, keys)| folders.get(&f).map(|(_, path)| (path.clone(), keys))).collect()
}

/// After a worker wrote to the cache: notes when it passed its budget (from memory).
fn note_budget(cache: &Cache, budget: &AtomicU64, over: &AtomicBool, wrote: bool) {
    if wrote && cache.disk_bytes() > budget.load(Ordering::Relaxed) {
        over.store(true, Ordering::Relaxed);
    }
}

/// Whether the folder of the photo at `path` can be reached (on a worker).
fn reachable(path: &Path) -> bool {
    path.parent().is_some_and(Path::is_dir)
}

/// Whether `result` failed because the photo's folder cannot be reached (an offline volume): then
/// the photo is not a failure, only unavailable for now.
fn is_unreachable<T>(result: &Result<T, ladder::Error>, source: &Source) -> bool {
    let io = matches!(result, Err(ladder::Error::Photo(dalica_thumbs::Error::Io(_) | dalica_thumbs::Error::Image(dalica_image::Error::Io(_)))));
    io && !reachable(&source.path)
}

/// The tile text of a photo that cannot be shown (`formats.md` §1).
fn reason(e: &ladder::Error) -> String {
    match e {
        ladder::Error::Photo(dalica_thumbs::Error::Image(i)) => i.reason(),
        ladder::Error::Photo(dalica_thumbs::Error::Io(_)) => dalica_image::Error::Io(String::new()).reason(),
        ladder::Error::Encode(_) | ladder::Error::Cache(_) | ladder::Error::Cancelled => "The thumbnail could not be made".into(),
    }
}

/// The embedded thumbnail of the photo at `path` (EXIF or the camera's preview), upright and
/// cropped to the photo's shape (camera thumbnails are often letterboxed), when it is at least
/// `need` pixels on its long side; `None` otherwise or on any trouble. Reads the file's head only.
fn embedded(path: &Path, need: u32, render: &Render, limits: &dalica_image::Limits) -> Option<Rgba8> {
    let (header, bytes) = dalica_image::read_header_file(path, limits).ok()?;
    let thumb = header.thumbnail(&bytes)?;
    let image = dalica_image::decode_with(thumb, &render.decode_options(), limits).ok()?;
    let image = dalica_image::orient::apply(image, header.orientation).ok()?;
    let (w, h) = header.display_size();
    let image = crop_to_aspect(image, w, h)?;
    (image.width.max(image.height) >= need).then_some(image)
}

/// `image` centre-cropped to the aspect of `w × h` (unchanged when within 2 %).
fn crop_to_aspect(image: Rgba8, w: u32, h: u32) -> Option<Rgba8> {
    if w == 0 || h == 0 || image.width == 0 || image.height == 0 {
        return None;
    }
    let (iw, ih) = (u64::from(image.width), u64::from(image.height));
    let (w, h) = (u64::from(w), u64::from(h));
    // Compare iw/ih with w/h without floats: iw·h against ih·w.
    let (a, b) = (iw * h, ih * w);
    if a.abs_diff(b) * 50 <= a.max(b) {
        return Some(image);
    }
    let (cw, ch) = if a > b { ((ih * w / h).max(1), ih) } else { (iw, (iw * h / w).max(1)) };
    let (x0, y0) = ((iw - cw) / 2, (ih - ch) / 2);
    let mut data = Vec::with_capacity(usize::try_from(cw * ch * 4).ok()?);
    for y in y0..y0 + ch {
        let start = usize::try_from((y * iw + x0) * 4).ok()?;
        let end = start.checked_add(usize::try_from(cw * 4).ok()?)?;
        data.extend_from_slice(image.data.get(start..end)?);
    }
    Some(Rgba8 { width: u32::try_from(cw).ok()?, height: u32::try_from(ch).ok()?, data })
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

impl Session {
    /// Applies the thumbnail settings of `settings.json` (View → Use Color Management, the cache
    /// size) before the first sync; called once by [`Session::open_with`].
    pub(crate) fn configure_thumbnails(&mut self) {
        let render = Render { color_managed: self.settings().color_management() };
        let bytes = self.settings().thumbnail_cache_bytes();
        self.thumbs.set_render(render);
        self.thumbs.set_budget(bytes);
        // At start: upkeep (and pruning, if the cache is past its size) once the first scans settle.
        self.thumbs.maintain_due = true;
    }

    /// Whether thumbnails and previews apply embedded colour profiles (View → Use Color
    /// Management).
    pub fn color_management(&self) -> bool {
        self.thumbs.render().color_managed
    }

    /// Switches colour management (`view.colorManagement`): saved in `settings.json`; the cells on
    /// screen are made again the new way. Returns whether it changed.
    pub fn set_color_management(&mut self, on: bool) -> bool {
        if self.color_management() == on && self.settings().color_management() == on {
            return false;
        }
        self.change_settings(|s| s.set_color_management(on));
        self.thumbs.set_render(Render { color_managed: on });
        true
    }

    /// The thumbnail cache's size in bytes (the setting; 2 GiB by default).
    pub fn thumbnail_cache_size(&self) -> u64 {
        self.settings().thumbnail_cache_bytes()
    }

    /// Sets the thumbnail cache's size (clamped to 64 MiB … 1 TiB), saved in `settings.json`; a
    /// cache already past it is pruned in the background.
    pub fn set_thumbnail_cache_size(&mut self, bytes: u64) {
        self.change_settings(|s| s.set_thumbnail_cache_bytes(bytes));
        let bytes = self.settings().thumbnail_cache_bytes();
        self.thumbs.set_budget(bytes);
    }

    /// Makes the thumbnails of `ids` again (`section.refreshThumbs`): see [`Thumbs::refresh`].
    pub(crate) fn refresh_thumbnails(&mut self, ids: &[PhotoId]) -> usize {
        self.thumbs.refresh(&self.library, ids)
    }

    /// Goes up whenever thumbnails already handed out are made again (View → Use Color Management
    /// switched, Folder → Refresh Thumbnails): whoever keeps them (the light table's textures)
    /// drops them and asks again.
    pub fn thumbnail_generation(&self) -> u64 {
        self.thumbs.generation()
    }

    /// Which thumbnails were made again after generation `since` (whose images to drop).
    pub fn thumbnails_refreshed_since(&self, since: u64) -> Refreshed {
        self.thumbs.refreshed_since(since)
    }

    /// What photo `id` draws now in a cell of `need` device pixels, like
    /// [`Session::thumbnail`] but without asking for anything (benches, tests).
    pub fn thumbnail_now(&mut self, id: PhotoId, need: u32) -> Thumb {
        self.thumbs.peek(&self.library, id, need)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{Rgba8, crop_to_aspect, default_cache_dir};

    /// A camera thumbnail letterboxed to 4:3 (black bars above and below a 3:2 picture) is cut
    /// back to the photo's shape; one already of its shape (within 2 %) is left as it is; hostile
    /// sizes give `None`.
    #[test]
    fn embedded_thumbnails_are_cropped_to_the_photos_shape() {
        let (w, h) = (160u32, 120u32);
        let data: Vec<u8> =
            (0..h).flat_map(|y| (0..w).flat_map(move |_| if (7..113).contains(&y) { [200, 100, 50, 255] } else { [0, 0, 0, 255] })).collect();
        let boxed = Rgba8 { width: w, height: h, data };
        let cut = crop_to_aspect(boxed.clone(), 6000, 4000).unwrap();
        assert_eq!((cut.width, cut.height), (160, 106));
        assert!(cut.data.chunks(4).all(|p| p == [200, 100, 50, 255]), "the bars are gone");
        assert_eq!(crop_to_aspect(boxed.clone(), 4000, 3000).unwrap(), boxed, "already 4:3");
        let tall = crop_to_aspect(boxed.clone(), 3000, 4000).unwrap();
        assert_eq!((tall.width, tall.height), (90, 120));
        assert_eq!(crop_to_aspect(boxed.clone(), 0, 10), None);
        assert_eq!(crop_to_aspect(Rgba8 { width: 4, height: 4, data: vec![0; 3] }, 3, 2), None, "short data");
    }

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
