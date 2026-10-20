//! The edit view's preview renders (`editing.md` §3, §4, §5.2; `architecture.md` §5–§6), all off
//! the owner thread on the preview's own workers, the open photos' at the edit view's priority:
//!
//! - **Two panes** (M11.10). The preview has [`PANES`] panes, 0 and 1: compare shows two photos
//!   (A│B) or one photo twice (A│A). Each pane has its photo, its recipe, its tool's candidate, its
//!   zoomed window and its generation; both render at the edit view's priority, the focused pane
//!   ([`Session::set_preview_focus`]) a step above the other so its jobs go first. The calls
//!   without a pane (`open_preview`, `preview` …) are pane 0's; [`Session::close_preview`] closes
//!   both.
//! - **What a pane shows** ([`PaneShows`]): the photo's own recipe, A│A's variant of it
//!   (`variant.rs`; the photo's own recipe while no variant of it is open), or the photo unedited
//!   (before/after: its source, no render at all).
//! - **The source.** Per open photo, its screen-size picture (the ladder's unedited screen entry
//!   when the thumbnail cache has it, else the file decoded under the limits and halved to between
//!   one and two screen sizes, then stored as that entry in the background) is kept in memory: the
//!   last [`SOURCES`] photos', never an open pane's. Two panes on one photo share it.
//! - **The screen preview** of a pane's recipe ([`Session::open_preview_in`]) at the largest
//!   power-of-two scale of its source (1, ½, ¼ …) that still covers the pane's box in device
//!   pixels, never above the source's resolution; the edit view draws it fitted to the box. A
//!   change of the recipe a pane shows (any history step, of the photo or of its variant) renders
//!   that pane again, and only that one. Why powers of two: see `preview/jobs.rs` (the renderer's
//!   fractional resample is what a 2-core machine cannot afford).
//! - **A tool's candidate** ([`Session::set_preview_candidate_in`]) renders as a
//!   [`Quality::Draft`], at a lower scale only when the measured cost of a full render would miss
//!   [`DRAFT_TARGET_MS`], then as the [`Quality::Final`] (a full-scale draft is the final).
//! - **Zoomed past the preview** ([`Session::set_preview_window_in`]): the whole photo is decoded
//!   (kept while zoomed, when it fits in [`MAX_FULL_BYTES`]; shared by two panes on one photo) and
//!   the visible window rendered at full resolution with `edit::render_region` ("refining" without
//!   blocking).
//! - **The neighbours** ([`Session::prefetch_previews`]): their sources are read and their previews
//!   rendered at the prefetch priority, so the next or previous photo opens without a decode.
//! - **A video** (M15.20) shows its poster frame as its source (`jobs.rs`); one this build cannot
//!   play answers its reason ("This video can't be played (…)") without being read.
//! - **Histograms**: each preview shown carries the 256-bin RGB [`Histogram`] of its pixels.
//! - Every request takes a new **generation**; a result of an older one is dropped (and its job
//!   cancelled when it has not started). Closing a pane cancels everything it asked for (what the
//!   other pane still needs of the same photo stays).
//!
//! The owner thread only submits jobs and takes their results in [`Session::poll`]: it never
//! decodes, renders or touches the disk.

mod histogram;
mod jobs;

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_core::{Clock, PhotoId, Timestamp};
use dalica_edit::{Recipe, Region};
use dalica_image::Rgba8;
use dalica_thumbs::ladder::{Render, Source};
use dalica_thumbs::{Pool, Priority, Submitted};

pub use self::histogram::{BINS, Histogram};
pub use self::jobs::halve;
use self::jobs::{Ask, Kind, Ladder, Origin, Rendered, Screen};
use crate::session::Session;

/// The preview's worker threads (renders run their bands in parallel on top of these).
pub const WORKERS: usize = 2;
/// The preview's panes: 0 (the only one outside compare, the left or top one in it) and 1.
pub const PANES: usize = 2;
/// Photos whose screen-size source stays in memory (the open ones and their neighbours).
pub const SOURCES: usize = 4;
/// Prefetched previews kept ready.
pub const READY: usize = 4;
/// Neighbours prefetched at most.
pub const MAX_PREFETCH: usize = 4;
/// The largest full decode kept for a zoomed view, in bytes of RGBA (inside the memory budget of
/// `architecture.md` §5).
pub const MAX_FULL_BYTES: u64 = 1 << 30;
/// A draft is rendered smaller when a full render is expected to take longer than this: half the
/// 16 ms frame, the rest left for the frame's own work and the wait for a worker (at 12 ms the
/// budgets runner's drafts reached p95 12.3 ms).
pub const DRAFT_TARGET_MS: f64 = 8.0;
/// The smallest draft scale.
pub const MIN_DRAFT_SCALE: f64 = 0.25;
/// The largest box a preview covers, per side.
pub const MAX_FIT: u32 = 16_384;
/// The most pixels of a refined window.
pub const MAX_WINDOW_PIXELS: u64 = 1 << 26;
/// Updates kept until taken.
pub const MAX_UPDATES: usize = 256;
/// The priority of the pane not focused: the edit view's, one step below the focused pane's.
const SIDE: Priority = Priority(Priority::EDIT.0 - 1);

/// The quality of a preview shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// A tool's candidate rendered quickly while it changes (maybe at a lower scale).
    Draft,
    /// The preview at the box's full size.
    Final,
}

/// What a pane shows of its photo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneShows {
    /// The photo's own recipe (or a tool's candidate for it).
    #[default]
    Edited,
    /// A│A's variant of the photo (or a tool's candidate for it); the photo's own recipe while no
    /// variant of it is open.
    Variant,
    /// The photo unedited: its source, whatever the recipe or a candidate (before/after).
    Unedited,
}

/// The preview the edit view shows.
#[derive(Clone, Debug)]
pub struct PreviewImage {
    pub photo: PhotoId,
    pub generation: u64,
    pub quality: Quality,
    pub image: Arc<Rgba8>,
    pub histogram: Arc<Histogram>,
    /// The recipe it shows (the photo's, its variant's, the tool's candidate, or none).
    pub recipe: Vec<String>,
}

/// The visible window of a zoomed view at full resolution.
#[derive(Clone, Debug)]
pub struct Refined {
    pub photo: PhotoId,
    pub generation: u64,
    pub region: Region,
    pub image: Arc<Rgba8>,
}

/// What an update brought.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateKind {
    Draft,
    Final,
    Refined,
}

/// One preview shown, with the pane it was shown in and the time from its request to the poll
/// that took it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewUpdate {
    pub pane: usize,
    pub photo: PhotoId,
    pub generation: u64,
    pub kind: UpdateKind,
    pub ms: f64,
}

/// Counters of the preview's work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreviewStats {
    /// Photo files decoded to the screen size.
    pub decodes: u64,
    /// Screen entries read from the thumbnail cache.
    pub cache_loads: u64,
    /// Whole photos decoded for a zoomed view.
    pub full_decodes: u64,
    /// Previews and windows rendered.
    pub renders: u64,
    /// Previews asked for (each a new generation, superseding the one before).
    pub requests: u64,
    /// Results of older requests dropped.
    pub stale: u64,
    /// Jobs cancelled (superseded, or the photo closed).
    pub cancelled: u64,
    pub failures: u64,
}

/// A preview job, by what it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// A photo's source, and its preview when asked.
    Load(PhotoId),
    /// A pane's preview of a generation (a draft or not).
    Render(usize, u64, bool),
    /// A photo's whole decode (for the panes zoomed on it).
    Full(PhotoId),
    /// A pane's refined window of a generation.
    Region(usize, u64),
    /// Storing a photo's screen entry in the thumbnail cache.
    Store(PhotoId),
}

/// A preview job's work, as the pool runs it.
type Work = Box<dyn FnOnce(&AtomicBool) -> Done + Send>;
/// A job kept so it can be offered to the pool again (`Preview::deferred`).
type Slot = Arc<Mutex<Option<Work>>>;

enum Done {
    Loaded { photo: PhotoId, file: Source, render: Render, result: Result<(Arc<Screen>, Origin), String>, frame: Option<Result<Rendered, String>> },
    Rendered { generation: u64, result: Result<Rendered, String> },
    Full { file: Source, result: Result<Arc<Rgba8>, String> },
    Region { generation: u64, region: Region, result: Result<Arc<Rgba8>, String> },
    Stored { file: Source, render: Render, result: Result<Option<Arc<Rgba8>>, String> },
    Panicked(String),
}

/// What a pane asked for last.
struct Want {
    generation: u64,
    recipe: Vec<String>,
    started: Instant,
    draft: bool,
}

/// An open pane.
struct Open {
    photo: PhotoId,
    file: Source,
    fit: (u32, u32),
    shows: PaneShows,
    candidate: Option<Vec<String>>,
    want: Want,
    frame: Option<PreviewImage>,
    window: Option<Region>,
    full: Option<Arc<Rgba8>>,
    refined: Option<Refined>,
    /// The window's request: generation and when.
    window_asked: (u64, Instant),
    failure: Option<String>,
}

/// The preview renders' state (`Session::preview`).
#[derive(Default)]
pub(crate) struct Preview {
    /// Started on the first open.
    pool: Option<Pool<Key, Done>>,
    panes: [Option<Open>; PANES],
    /// The focused pane: its jobs go first.
    focus: usize,
    /// Screen-size sources, most recently used first.
    sources: VecDeque<(Source, Render, Arc<Screen>)>,
    /// Prefetched previews with the file they were made from, most recently made first: a file
    /// changed since (a Save, an Undo Save, a Revert, an outside edit) never matches.
    ready: VecDeque<(PhotoId, Source, Rendered)>,
    /// Source loads on their way, with their priority.
    loading: HashMap<PhotoId, Priority>,
    prefetching: HashSet<PhotoId>,
    outstanding: HashSet<Key>,
    /// Jobs whose key was still running in the pool, cancelled, when they were submitted: the pool
    /// keeps one job per key and a cancelled one delivers nothing, so they are offered again on
    /// each poll until the old one has stopped (`outstanding` holds them meanwhile).
    deferred: Vec<(Key, Priority, Slot)>,
    generation: u64,
    /// Milliseconds per output megapixel of the last renders.
    cost: Option<f64>,
    /// The last box asked for (prefetch renders at it).
    fit: Option<(u32, u32)>,
    updates: VecDeque<PreviewUpdate>,
    stats: PreviewStats,
    /// Finished jobs are left unapplied by [`Session::poll`] (tests: [`Session::hold_preview`]).
    held: bool,
    /// Tests: a slow decoder ([`Session::plant_decode_delay`]): the clock it is measured on and
    /// the delay. Off (`None`) unless a test plants it.
    decode_delay: Option<(Arc<dyn Clock>, Duration)>,
    /// When each decoding job (a source load, a whole decode) was last submitted, while a decode
    /// delay is planted.
    asked: HashMap<Key, Timestamp>,
    /// Finished decoding jobs whose result is held until the planted delay has passed on the
    /// clock: when it is due, its key and its result.
    delayed: Vec<(Timestamp, Key, Done)>,
}

impl Preview {
    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.generation
    }

    fn pane(&self, pane: usize) -> Option<&Open> {
        self.panes.get(pane).and_then(Option::as_ref)
    }

    fn pane_mut(&mut self, pane: usize) -> Option<&mut Open> {
        self.panes.get_mut(pane).and_then(Option::as_mut)
    }

    /// Whether a pane has photo `id` open.
    fn holds(&self, id: PhotoId) -> bool {
        self.panes.iter().flatten().any(|o| o.photo == id)
    }

    /// The panes open on `file`.
    fn panes_on(&self, file: &Source) -> Vec<usize> {
        (0..PANES).filter(|p| self.pane(*p).is_some_and(|o| o.file == *file)).collect()
    }

    /// The priority of pane `pane`'s jobs: the edit view's for the focused one, a step below for
    /// the other.
    fn priority(&self, pane: usize) -> Priority {
        if pane == self.focus { Priority::EDIT } else { SIDE }
    }

    fn submit(&mut self, key: Key, priority: Priority, work: impl FnOnce(&AtomicBool) -> Done + Send + 'static) {
        let wrapped: Work = Box::new(move |cancel: &AtomicBool| {
            catch_unwind(AssertUnwindSafe(|| work(cancel))).unwrap_or_else(|panic| {
                let message =
                    panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                Done::Panicked(message)
            })
        });
        if let Some((clock, _)) = &self.decode_delay
            && matches!(key, Key::Load(_) | Key::Full(_))
        {
            self.asked.insert(key, clock.now());
        }
        // A key already outstanding is running or queued for us: its result is on its way.
        let fresh = self.outstanding.insert(key);
        self.offer(key, priority, Arc::new(Mutex::new(Some(wrapped))), fresh);
    }

    /// Hands `slot`'s job to the pool. When the pool is still running a cancelled job of the same
    /// key (`fresh`: nothing of ours is), it takes no new one and the old one delivers nothing, so
    /// the job waits in `deferred`.
    fn offer(&mut self, key: Key, priority: Priority, slot: Slot, fresh: bool) {
        let pool = self.pool.get_or_insert_with(|| Pool::new(WORKERS));
        let job = Arc::clone(&slot);
        let submitted = pool.submit(key, priority, move |cancel: &AtomicBool| match job.lock().ok().and_then(|mut w| w.take()) {
            Some(work) => work(cancel),
            None => Done::Panicked("a preview job ran twice".into()),
        });
        if submitted == Submitted::Running && fresh {
            self.deferred.push((key, priority, slot));
        }
    }

    /// Offers the deferred jobs again; those cancelled meanwhile are dropped.
    fn retry_deferred(&mut self) {
        if self.deferred.is_empty() {
            return;
        }
        for (key, priority, slot) in std::mem::take(&mut self.deferred) {
            if self.outstanding.contains(&key) {
                self.offer(key, priority, slot, true);
            }
        }
    }

    /// When a finished job of `key` may be applied under the planted decode delay (`None`: now).
    fn due(&self, key: &Key) -> Option<Timestamp> {
        let (_, delay) = self.decode_delay.as_ref()?;
        self.asked.get(key).map(|at| at.saturating_add(*delay))
    }

    /// Whether every job outstanding has finished and only waits for the planted decode delay.
    fn only_delayed(&self) -> bool {
        self.outstanding.iter().all(|k| self.delayed.iter().any(|(_, held, _)| held == k))
    }

    fn cancel(&mut self, key: Key) {
        if self.outstanding.remove(&key)
            && let Some(pool) = &self.pool
            && pool.cancel(&key)
        {
            self.stats.cancelled += 1;
        }
    }

    /// Cancels the jobs `which` picks.
    fn cancel_where(&mut self, which: impl Fn(&Key) -> bool) {
        let keys: Vec<Key> = self.outstanding.iter().filter(|k| which(k)).copied().collect();
        for key in keys {
            if let Key::Load(id) = key {
                self.loading.remove(&id);
                self.prefetching.remove(&id);
            }
            self.cancel(key);
        }
    }

    fn source_of(&mut self, file: &Source, render: Render) -> Option<Arc<Screen>> {
        let at = self.sources.iter().position(|(f, r, _)| f == file && *r == render)?;
        let entry = self.sources.remove(at)?;
        let image = Arc::clone(&entry.2);
        self.sources.push_front(entry);
        Some(image)
    }

    fn keep_source(&mut self, file: Source, render: Render, image: Arc<Screen>) {
        self.sources.retain(|(f, r, _)| !(*f == file && *r == render));
        self.sources.push_front((file, render, image));
        let open: Vec<Source> = self.panes.iter().flatten().map(|o| o.file.clone()).collect();
        while self.sources.len() > SOURCES {
            // An open pane's source is never the one dropped.
            match self.sources.iter().rposition(|(f, _, _)| !open.contains(f)) {
                Some(at) => {
                    self.sources.remove(at);
                }
                None => break,
            }
        }
    }

    fn keep_ready(&mut self, photo: PhotoId, file: Source, rendered: Rendered) {
        self.ready.retain(|(p, _, _)| *p != photo);
        self.ready.push_front((photo, file, rendered));
        self.ready.truncate(READY);
    }

    /// Remembers what a full-size render from a source took (a render from a whole decode, or a
    /// smaller draft, says nothing about the next one).
    fn note_cost(&mut self, r: &Rendered) {
        if r.scale >= 1.0 && r.ms.is_finite() {
            self.cost = Some(r.ms);
        }
    }

    /// The draft scale: 1 when the last full-size render made [`DRAFT_TARGET_MS`] (or none was
    /// measured yet), else half or, at the least, [`MIN_DRAFT_SCALE`] (a quarter of the pixels, or
    /// a sixteenth): fixed steps, so a whole drag draws from one base.
    fn draft_scale(&self) -> f64 {
        let Some(expected) = self.cost else { return 1.0 };
        if !expected.is_finite() || expected <= DRAFT_TARGET_MS {
            1.0
        } else if expected * 0.25 <= DRAFT_TARGET_MS {
            0.5
        } else {
            MIN_DRAFT_SCALE
        }
    }

    fn push_update(&mut self, update: PreviewUpdate) {
        self.updates.push_back(update);
        while self.updates.len() > MAX_UPDATES {
            self.updates.pop_front();
        }
    }
}

/// The file of photo `id` as the cache key sees it.
fn file_of(session: &Session, id: PhotoId) -> Option<Source> {
    let photo = session.library.photo(id)?;
    let folder = session.library.folder(photo.folder)?;
    Some(Source { path: folder.path.join(&photo.name), file_size: photo.size, mtime_ms: photo.mtime.as_millis() })
}

/// What photo `id`'s file is to the preview: a still, a video with a poster frame, or a video
/// this build cannot play (with why).
fn kind_of(session: &Session, id: PhotoId) -> Kind {
    let library = &session.library;
    if !crate::video::is_video(library, id) {
        return Kind::Still;
    }
    if crate::thumbs::has_poster(library, id) {
        return Kind::Poster;
    }
    let why = library.video_facts(id).and_then(|f| dalica_thumbs::video::why_not(&f.video_codec));
    Kind::Unplayable(why.unwrap_or_else(|| crate::video::CANNOT_PLAY.to_string()))
}

fn parse(texts: &[String]) -> Recipe {
    Recipe::parse(texts).unwrap_or_else(|e| {
        log::warn!("preview: a recipe the renderer cannot read ({e}); shown unedited");
        Recipe::new()
    })
}

fn clamp_fit((w, h): (u32, u32)) -> (u32, u32) {
    (w.clamp(1, MAX_FIT), h.clamp(1, MAX_FIT))
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Why a preview request was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PreviewError {
    #[error("no photo is open in the edit view")]
    NotOpen,
    #[error("no such photo")]
    NoPhoto,
    #[error("{0}")]
    BadRecipe(String),
    #[error("bad window: {0}")]
    BadWindow(&'static str),
    #[error("no preview pane {0}: the panes are 0 and 1")]
    BadPane(usize),
}

/// `pane`, when it is one.
fn pane_index(pane: usize) -> Result<usize, PreviewError> {
    if pane < PANES { Ok(pane) } else { Err(PreviewError::BadPane(pane)) }
}

impl Session {
    /// How the thumbnail ladder makes its screen-size entry.
    fn preview_ladder(&self) -> Ladder {
        let (cache, render, screen, limits) = self.thumbs.screen_source();
        Ladder { cache, render, screen, limits }
    }

    /// The recipe pane `open` shows now.
    fn pane_texts(&self, open: &Open) -> Vec<String> {
        match (open.shows, &open.candidate) {
            (PaneShows::Unedited, _) => Vec::new(),
            (_, Some(candidate)) => candidate.clone(),
            (PaneShows::Variant, None) => {
                self.variant_edits(open.photo).map_or_else(|| self.library.info(open.photo).edits.clone(), <[String]>::to_vec)
            }
            (PaneShows::Edited, None) => self.library.info(open.photo).edits.clone(),
        }
    }

    /// Opens photo `id` in pane 0 ([`Session::open_preview_in`]).
    pub fn open_preview(&mut self, id: PhotoId, fit: (u32, u32)) -> Result<(), PreviewError> {
        self.open_preview_in(0, id, fit)
    }

    /// Opens photo `id` in preview pane `pane`, covering `fit` (the pane's photo area in device
    /// pixels): its source is read (or taken from memory) and the recipe the pane shows rendered
    /// at the edit view's priority. Opening another photo in a pane first closes the one open
    /// there, and the pane then shows the new photo's own recipe.
    pub fn open_preview_in(&mut self, pane: usize, id: PhotoId, fit: (u32, u32)) -> Result<(), PreviewError> {
        let pane = pane_index(pane)?;
        let file = file_of(self, id).ok_or(PreviewError::NoPhoto)?;
        if self.preview.pane(pane).is_some_and(|o| o.photo != id) {
            self.close_pane(pane);
        }
        let fit = clamp_fit(fit);
        self.preview.fit = Some(fit);
        let reopened = self.preview.pane(pane).is_some_and(|o| o.photo == id && o.file == file);
        if !reopened {
            let generation = self.preview.next_generation();
            let recipe = self.library.info(id).edits.clone();
            let shows = self.preview.pane(pane).map(|o| o.shows).unwrap_or_default();
            if let Some(slot) = self.preview.panes.get_mut(pane) {
                *slot = Some(Open {
                    photo: id,
                    file,
                    fit,
                    shows,
                    candidate: None,
                    want: Want { generation, recipe, started: Instant::now(), draft: false },
                    frame: None,
                    window: None,
                    full: None,
                    refined: None,
                    window_asked: (0, Instant::now()),
                    failure: None,
                });
            }
        } else if let Some(open) = self.preview.pane_mut(pane) {
            open.fit = fit;
            open.candidate = None;
        }
        self.request_preview(pane, false);
        Ok(())
    }

    /// Closes both panes ([`Session::close_preview_in`]).
    pub fn close_preview(&mut self) {
        for pane in 0..PANES {
            self.close_pane(pane);
        }
    }

    /// Closes pane `pane` (nothing for a pane that is not open, or not a pane): every job it asked
    /// for is cancelled (its photo's source and whole decode only when the other pane is not on the
    /// same photo) and its preview, window and full decode dropped. Sources and prefetched
    /// previews stay in memory.
    pub fn close_preview_in(&mut self, pane: usize) {
        self.close_pane(pane);
    }

    fn close_pane(&mut self, pane: usize) {
        let Some(open) = self.preview.panes.get_mut(pane).and_then(Option::take) else { return };
        let photo = open.photo;
        let shared = self.preview.holds(photo);
        self.preview.cancel_where(|k| match k {
            Key::Load(id) | Key::Full(id) => *id == photo && !shared,
            Key::Render(p, ..) | Key::Region(p, _) => *p == pane,
            Key::Store(_) => false,
        });
    }

    /// Pane 0's photo area changed size ([`Session::set_preview_fit_in`]).
    pub fn set_preview_fit(&mut self, fit: (u32, u32)) -> Result<(), PreviewError> {
        self.set_preview_fit_in(0, fit)
    }

    /// Pane `pane`'s photo area changed size: its preview is made to cover `fit` again.
    pub fn set_preview_fit_in(&mut self, pane: usize, fit: (u32, u32)) -> Result<(), PreviewError> {
        let pane = pane_index(pane)?;
        let fit = clamp_fit(fit);
        let open = self.preview.pane_mut(pane).ok_or(PreviewError::NotOpen)?;
        if open.fit != fit {
            open.fit = fit;
            self.preview.fit = Some(fit);
            self.request_preview(pane, false);
        }
        Ok(())
    }

    /// A tool's candidate for pane 0 ([`Session::set_preview_candidate_in`]).
    pub fn set_preview_candidate(&mut self, recipe: Option<&[String]>) -> Result<(), PreviewError> {
        self.set_preview_candidate_in(0, recipe)
    }

    /// A tool's candidate recipe for pane `pane` (`Some`: the whole recipe as it would be stored),
    /// rendered as a draft and then final; `None` goes back to the recipe the pane shows. A pane
    /// showing its photo unedited keeps the candidate for when it shows the edits again.
    pub fn set_preview_candidate_in(&mut self, pane: usize, recipe: Option<&[String]>) -> Result<(), PreviewError> {
        let pane = pane_index(pane)?;
        if let Some(texts) = recipe {
            Recipe::parse(texts).map_err(|e| PreviewError::BadRecipe(e.to_string()))?;
        }
        let open = self.preview.pane_mut(pane).ok_or(PreviewError::NotOpen)?;
        open.candidate = recipe.map(<[String]>::to_vec);
        let draft = open.candidate.is_some() && open.shows != PaneShows::Unedited;
        self.request_preview(pane, draft);
        Ok(())
    }

    /// What pane `pane` shows of its photo: its own recipe, A│A's variant, or the photo unedited
    /// (before/after). Rendered again at once when it changes.
    pub fn set_preview_shows(&mut self, pane: usize, shows: PaneShows) -> Result<(), PreviewError> {
        let pane = pane_index(pane)?;
        let open = self.preview.pane_mut(pane).ok_or(PreviewError::NotOpen)?;
        if open.shows != shows {
            open.shows = shows;
            self.request_preview(pane, false);
        }
        Ok(())
    }

    /// Focuses pane `pane`: from now on its jobs go before the other pane's.
    pub fn set_preview_focus(&mut self, pane: usize) -> Result<(), PreviewError> {
        self.preview.focus = pane_index(pane)?;
        Ok(())
    }

    /// The focused pane.
    pub fn preview_focus(&self) -> usize {
        self.preview.focus
    }

    /// Pane 0's visible window ([`Session::set_preview_window_in`]).
    pub fn set_preview_window(&mut self, window: Option<Region>) -> Result<(), PreviewError> {
        self.set_preview_window_in(0, window)
    }

    /// Pane `pane`'s visible window while zoomed past the preview (`Some`: in the displayed
    /// picture's pixels at `region.scale` output pixels per pixel of the whole photo), refined at
    /// full resolution in the background; `None` (fitted again) drops the window and the whole
    /// decode.
    pub fn set_preview_window_in(&mut self, pane: usize, window: Option<Region>) -> Result<(), PreviewError> {
        let pane = pane_index(pane)?;
        if let Some(r) = window {
            if !(r.scale.is_finite() && r.scale > 0.0 && r.scale <= dalica_edit::render::MAX_SCALE) {
                return Err(PreviewError::BadWindow("the scale must be finite, positive and at most the renderer's largest"));
            }
            if r.width == 0 || r.height == 0 || u64::from(r.width) * u64::from(r.height) > MAX_WINDOW_PIXELS {
                return Err(PreviewError::BadWindow("the window must hold between one pixel and the most a window may"));
            }
        }
        let open = self.preview.pane_mut(pane).ok_or(PreviewError::NotOpen)?;
        if open.window == window {
            return Ok(());
        }
        open.window = window;
        if window.is_none() {
            open.full = None;
            open.refined = None;
            let photo = open.photo;
            // The whole decode stays on its way while the other pane is zoomed on the same photo.
            let other_zoomed = self.preview.panes.iter().flatten().any(|o| o.photo == photo && o.window.is_some());
            self.preview.cancel_where(|k| matches!(k, Key::Region(p, _) if *p == pane) || (*k == Key::Full(photo) && !other_zoomed));
            return Ok(());
        }
        self.request_window(pane);
        Ok(())
    }

    /// Prefetches the previews of `ids` (the next and previous photos) at the prefetch priority:
    /// their sources read and their recipes rendered at the last box asked for. Prefetch of photos
    /// no longer listed is cancelled; photos open in a pane are not prefetched.
    pub fn prefetch_previews(&mut self, ids: &[PhotoId]) {
        let Some(fit) = self.preview.fit else { return };
        let ladder = self.preview_ladder();
        let wanted: Vec<PhotoId> = ids.iter().copied().filter(|id| !self.preview.holds(*id)).take(MAX_PREFETCH).collect();
        let stale: Vec<PhotoId> = self.preview.prefetching.iter().filter(|id| !wanted.contains(id)).copied().collect();
        for id in stale {
            if self.preview.loading.get(&id) == Some(&Priority::PREFETCH) {
                self.preview.loading.remove(&id);
                self.preview.cancel(Key::Load(id));
            }
            self.preview.prefetching.remove(&id);
        }
        for id in wanted {
            let Some(file) = file_of(self, id) else { continue };
            let texts = self.library.info(id).edits.clone();
            let have = self.preview.sources.iter().any(|(f, r, _)| *f == file && *r == ladder.render);
            let ready = self.preview.ready.iter().any(|(p, f, r)| *p == id && *f == file && r.recipe == texts && r.fit == fit && r.scale >= 1.0);
            if (have && ready) || self.preview.loading.contains_key(&id) {
                continue;
            }
            let ask = Ask { recipe: parse(&texts), texts, fit, scale: 1.0 };
            self.submit_load(id, file, Some(ask), Priority::PREFETCH);
            self.preview.prefetching.insert(id);
        }
    }

    /// Pane 0's preview shown now, if any.
    pub fn preview(&self) -> Option<&PreviewImage> {
        self.preview_in(0)
    }

    /// Pane `pane`'s preview shown now, if any.
    pub fn preview_in(&self, pane: usize) -> Option<&PreviewImage> {
        self.preview.pane(pane).and_then(|o| o.frame.as_ref())
    }

    /// Pane 0's refined window shown now, if zoomed and refined.
    pub fn preview_refined(&self) -> Option<&Refined> {
        self.preview_refined_in(0)
    }

    /// Pane `pane`'s refined window shown now, if zoomed and refined.
    pub fn preview_refined_in(&self, pane: usize) -> Option<&Refined> {
        self.preview.pane(pane).and_then(|o| o.refined.as_ref())
    }

    /// The photo open in pane 0.
    pub fn preview_photo(&self) -> Option<PhotoId> {
        self.preview_photo_in(0)
    }

    /// The photo open in pane `pane`.
    pub fn preview_photo_in(&self, pane: usize) -> Option<PhotoId> {
        self.preview.pane(pane).map(|o| o.photo)
    }

    /// What pane `pane` shows of its photo, while it is open.
    pub fn preview_shows(&self, pane: usize) -> Option<PaneShows> {
        self.preview.pane(pane).map(|o| o.shows)
    }

    /// Why pane 0's preview could not be made.
    pub fn preview_failure(&self) -> Option<&str> {
        self.preview_failure_in(0)
    }

    /// Why pane `pane`'s preview could not be made.
    pub fn preview_failure_in(&self, pane: usize) -> Option<&str> {
        self.preview.pane(pane).and_then(|o| o.failure.as_deref())
    }

    /// The previews shown since the last call, oldest first (at most [`MAX_UPDATES`]).
    pub fn take_preview_updates(&mut self) -> Vec<PreviewUpdate> {
        self.preview.updates.drain(..).collect()
    }

    /// Preview jobs queued or running.
    pub fn preview_pending(&self) -> usize {
        self.preview.outstanding.len()
    }

    /// The open panes' preview jobs queued or running (their photos' sources, their previews,
    /// their whole decodes and their refined windows), what the edit view shows next; prefetching
    /// other photos and storing cache entries are not counted. `ui.settle` waits for these
    /// (`control-protocol.md` §6.12).
    pub fn preview_busy(&self) -> usize {
        if self.preview.panes.iter().all(Option::is_none) {
            return 0;
        }
        self.preview
            .outstanding
            .iter()
            .filter(|k| match k {
                Key::Render(..) | Key::Region(..) => true,
                Key::Load(id) | Key::Full(id) => self.preview.holds(*id),
                Key::Store(_) => false,
            })
            .count()
    }

    pub fn preview_stats(&self) -> PreviewStats {
        self.preview.stats
    }

    /// What the last full-size render of a preview from its source took, in milliseconds: the
    /// draft scale is chosen from it (`None` until one is measured).
    pub fn preview_cost(&self) -> Option<f64> {
        self.preview.cost
    }

    /// Plants the render cost the next draft scale is chosen from (tests: a slow machine).
    #[doc(hidden)]
    pub fn plant_preview_cost(&mut self, ms: f64) {
        self.preview.cost = Some(ms).filter(|c| c.is_finite() && *c >= 0.0);
    }

    /// Tests: while `held`, [`Session::poll`] applies no finished preview job, so the open photo's
    /// preview stays on its way (`preview_busy` above 0) for as many frames as a test needs,
    /// however fast the machine renders it; [`Session::wait_preview`] and
    /// [`Session::wait_preview_update`] still apply them. A frame cannot tell a held result from a
    /// render still running: both are a job outstanding.
    #[doc(hidden)]
    pub fn hold_preview(&mut self, held: bool) {
        self.preview.held = held;
    }

    /// Tests: a slow decoder. While `Some(delay)`, the result of every job that decodes (a photo's
    /// source with its preview, a whole decode) is held until `delay` has passed on the session's
    /// clock since the job was asked for, then applied by the next poll: on a virtual clock the
    /// test decides when the decode "ends", with no real sleep. `None` (the default) turns it off
    /// and lets anything held through at the next poll.
    #[doc(hidden)]
    pub fn plant_decode_delay(&mut self, delay: Option<Duration>) {
        self.preview.decode_delay = delay.map(|d| (Arc::clone(&self.clock), d));
        if self.preview.decode_delay.is_none() {
            self.preview.asked.clear();
        }
    }

    /// Applies a finished job's result, or holds it while the planted decode delay has not passed.
    fn receive(&mut self, key: Key, done: Done) {
        if let (Some(due), Some((clock, _))) = (self.preview.due(&key), &self.preview.decode_delay)
            && clock.now() < due
        {
            self.preview.delayed.push((due, key, done));
            return;
        }
        self.handle_preview(key, done);
    }

    /// Applies the held results whose delay has passed (all of them once the delay is off); a
    /// held job cancelled meanwhile is dropped. Returns how many were applied.
    fn release_delayed(&mut self) -> usize {
        if self.preview.delayed.is_empty() {
            return 0;
        }
        let now = self.preview.decode_delay.as_ref().map(|(clock, _)| clock.now());
        let (due, held): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.preview.delayed).into_iter().partition(|(at, _, _)| now.is_none_or(|n| *at <= n));
        self.preview.delayed = held;
        let mut n = 0;
        for (_, key, done) in due {
            if self.preview.outstanding.contains(&key) {
                self.handle_preview(key, done);
                n += 1;
            } else {
                self.preview.stats.stale += 1;
            }
        }
        n
    }

    /// Waits up to `timeout` until every preview job has finished and been applied (CLI, tests,
    /// benches); with a decode delay planted, results still held for it count as finished.
    /// Returns whether it got there.
    pub fn wait_preview(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.release_delayed();
            self.preview.retry_deferred();
            self.check_preview_recipe();
            if self.preview.only_delayed() {
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let Some(pool) = &self.preview.pool else { return true };
            if let Some((key, done)) = pool.recv_timeout(left.min(Duration::from_millis(20))) {
                self.receive(key, done);
            }
        }
    }

    /// Waits up to `timeout` for the next preview shown, applying results as they come (benches,
    /// tests).
    pub fn wait_preview_update(&mut self, timeout: Duration) -> Option<PreviewUpdate> {
        let until = Instant::now() + timeout;
        loop {
            self.release_delayed();
            if let Some(u) = self.preview.updates.pop_front() {
                return Some(u);
            }
            self.preview.retry_deferred();
            self.check_preview_recipe();
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() || self.preview.only_delayed() && self.preview.updates.is_empty() {
                return self.preview.updates.pop_front();
            }
            let pool = self.preview.pool.as_ref()?;
            if let Some((key, done)) = pool.recv_timeout(left.min(Duration::from_millis(20))) {
                self.receive(key, done);
            }
        }
    }

    /// Applies the preview renders that finished; returns how many. Never blocks and never touches
    /// the disk.
    pub(crate) fn poll_preview(&mut self) -> usize {
        if self.preview.pool.is_none() || self.preview.held {
            return 0;
        }
        let mut n = self.release_delayed();
        self.preview.retry_deferred();
        self.check_preview_recipe();
        while let Some((key, done)) = self.preview.pool.as_ref().and_then(Pool::try_recv) {
            self.receive(key, done);
            n += 1;
        }
        n
    }

    /// The recipe a pane shows changed (a history step of the photo or of its variant) or its
    /// photo left: render that pane again, or close it.
    fn check_preview_recipe(&mut self) {
        for pane in 0..PANES {
            let Some(open) = self.preview.pane(pane) else { continue };
            if self.library.photo(open.photo).is_none() {
                self.close_pane(pane);
                continue;
            }
            if file_of(self, open.photo).is_some_and(|f| f != open.file) {
                // The file changed: read it again (the pane goes on showing what it showed).
                let (id, fit, shows) = (open.photo, open.fit, open.shows);
                self.close_pane(pane);
                if self.open_preview_in(pane, id, fit).is_ok() {
                    let _ = self.set_preview_shows(pane, shows);
                }
                continue;
            }
            if self.pane_texts(open) != open.want.recipe {
                self.request_preview(pane, false);
            }
        }
    }

    /// A new generation of pane `pane`'s preview: its older renders are cancelled, and the preview
    /// is rendered from the source in memory (a draft first when `draft`), taken from a prefetched
    /// one, or rendered once the source is read.
    fn request_preview(&mut self, pane: usize, draft: bool) {
        let ladder = self.preview_ladder();
        let Some(open) = self.preview.pane(pane) else { return };
        let texts = self.pane_texts(open);
        let generation = self.preview.next_generation();
        self.preview.stats.requests += 1;
        self.preview.cancel_where(|k| matches!(k, Key::Render(p, ..) if *p == pane));
        let priority = self.preview.priority(pane);
        let Some(open) = self.preview.pane_mut(pane) else { return };
        open.want = Want { generation, recipe: texts.clone(), started: Instant::now(), draft };
        open.failure = None;
        let (photo, file, fit, windowed) = (open.photo, open.file.clone(), open.fit, open.window.is_some());
        if windowed {
            self.request_window(pane);
        }
        if !draft
            && let Some(at) =
                self.preview.ready.iter().position(|(p, f, r)| *p == photo && *f == file && r.recipe == texts && r.fit == fit && r.scale >= 1.0)
            && let Some((_, _, rendered)) = self.preview.ready.get(at).cloned()
        {
            self.show(pane, generation, Quality::Final, rendered);
            return;
        }
        let Some(screen) = self.preview.source_of(&file, ladder.render) else {
            let ask = Ask { recipe: parse(&texts), texts, fit, scale: 1.0 };
            self.submit_load(photo, file, Some(ask), priority);
            return;
        };
        let scale = if draft { self.preview.draft_scale() } else { 1.0 };
        self.submit_render(pane, generation, draft, screen, Ask { recipe: parse(&texts), texts, fit, scale });
    }

    fn submit_render(&mut self, pane: usize, generation: u64, draft: bool, screen: Arc<Screen>, ask: Ask) {
        let priority = self.preview.priority(pane);
        self.preview
            .submit(Key::Render(pane, generation, draft), priority, move |_| Done::Rendered { generation, result: jobs::render(&screen, &ask) });
    }

    /// Reads photo `id`'s source (from memory when it is there) and renders `ask` from it.
    fn submit_load(&mut self, id: PhotoId, file: Source, ask: Option<Ask>, priority: Priority) {
        if self.preview.loading.get(&id).is_some_and(|p| *p >= priority) {
            return;
        }
        let ladder = self.preview_ladder();
        let have = self.preview.source_of(&file, ladder.render);
        self.preview.loading.insert(id, priority);
        let render = ladder.render;
        let kind = kind_of(self, id);
        self.preview.submit(Key::Load(id), priority, move |_| {
            let result = match have {
                Some(screen) => Ok((screen, Origin::Memory)),
                None => jobs::read(&file, &kind, &ladder, MAX_FULL_BYTES).map(|(image, origin)| (Arc::new(Screen::new(Arc::new(image))), origin)),
            };
            let frame = match (&result, ask) {
                (Ok((screen, _)), Some(ask)) => Some(jobs::render(screen, &ask)),
                _ => None,
            };
            Done::Loaded { photo: id, file, render, result, frame }
        });
    }

    /// Settles a decoded source in the background: fitted to the screen size and stored as the
    /// ladder's screen entry.
    fn submit_store(&mut self, photo: PhotoId, file: &Source, image: Arc<Rgba8>) {
        let (ladder, file) = (self.preview_ladder(), file.clone());
        let render = ladder.render;
        self.preview.submit(Key::Store(photo), Priority::PREFETCH, move |_| Done::Stored {
            result: jobs::settle(&file, &ladder, &image).map(|f| f.map(Arc::new)),
            file,
            render,
        });
    }

    /// Asks for pane `pane`'s refined window: the whole decode first (shared with the other pane
    /// on the same photo), then the window.
    fn request_window(&mut self, pane: usize) {
        let ladder = self.preview_ladder();
        let generation = self.preview.next_generation();
        self.preview.cancel_where(|k| matches!(k, Key::Region(p, _) if *p == pane));
        let priority = self.preview.priority(pane);
        let shared =
            self.preview.pane(pane).and_then(|me| self.preview.panes.iter().flatten().filter(|o| o.file == me.file).find_map(|o| o.full.clone()));
        let Some(open) = self.preview.pane_mut(pane) else { return };
        let Some(region) = open.window else { return };
        open.window_asked = (generation, Instant::now());
        if open.full.is_none() {
            open.full = shared;
        }
        let (photo, file, texts) = (open.photo, open.file.clone(), open.want.recipe.clone());
        match open.full.clone() {
            Some(full) => {
                let recipe = parse(&texts);
                self.preview.submit(Key::Region(pane, generation), priority, move |_| Done::Region {
                    generation,
                    region,
                    result: jobs::region(&full, &recipe, region).map(Arc::new),
                });
            }
            None => {
                if !self.preview.outstanding.contains(&Key::Full(photo)) {
                    let kind = kind_of(self, photo);
                    self.preview.submit(Key::Full(photo), priority, move |_| Done::Full {
                        result: jobs::full(&file, &kind, &ladder, MAX_FULL_BYTES).map(Arc::new),
                        file,
                    });
                }
            }
        }
    }

    /// Shows `rendered` as pane `pane`'s preview of `generation`.
    fn show(&mut self, pane: usize, generation: u64, quality: Quality, rendered: Rendered) {
        let Some(open) = self.preview.pane_mut(pane) else { return };
        let ms = ms_since(open.want.started);
        let photo = open.photo;
        open.frame = Some(PreviewImage { photo, generation, quality, image: rendered.image, histogram: rendered.histogram, recipe: rendered.recipe });
        let kind = if quality == Quality::Draft { UpdateKind::Draft } else { UpdateKind::Final };
        self.preview.push_update(PreviewUpdate { pane, photo, generation, kind, ms });
    }

    fn handle_preview(&mut self, key: Key, done: Done) {
        self.preview.outstanding.remove(&key);
        match done {
            Done::Loaded { photo, file, render, result, frame } => {
                self.preview.loading.remove(&photo);
                self.preview.prefetching.remove(&photo);
                let got = match result {
                    Ok(got) => got,
                    Err(e) => {
                        self.preview.stats.failures += 1;
                        for pane in self.preview.panes_on(&file) {
                            if let Some(open) = self.preview.pane_mut(pane).filter(|o| o.photo == photo) {
                                open.failure = Some(e.clone());
                            }
                        }
                        return;
                    }
                };
                if render != self.preview_ladder().render {
                    self.preview.stats.stale += 1;
                    return;
                }
                let (screen, origin) = got;
                match origin {
                    Origin::Memory => {}
                    Origin::Cache => self.preview.stats.cache_loads += 1,
                    Origin::Decoded => {
                        self.preview.stats.decodes += 1;
                        self.submit_store(photo, &file, Arc::clone(&screen.image));
                    }
                }
                self.preview.keep_source(file.clone(), render, screen);
                let frame = match frame {
                    Some(Ok(r)) => {
                        self.preview.stats.renders += 1;
                        self.preview.note_cost(&r);
                        Some(r)
                    }
                    Some(Err(e)) => {
                        self.preview.stats.failures += 1;
                        log::warn!("preview: {e}");
                        None
                    }
                    None => None,
                };
                if let Some(r) = &frame {
                    self.preview.keep_ready(photo, file.clone(), r.clone());
                }
                for pane in self.preview.panes_on(&file) {
                    let Some(open) = self.preview.pane(pane).filter(|o| o.photo == photo) else { continue };
                    let (generation, wanted, fit, draft) = (open.want.generation, open.want.recipe.clone(), open.fit, open.want.draft);
                    match &frame {
                        Some(r) if r.recipe == wanted && r.fit == fit => self.show(pane, generation, Quality::Final, r.clone()),
                        _ => self.request_preview(pane, draft),
                    }
                    if self.preview.pane(pane).is_some_and(|o| o.window.is_some()) {
                        self.request_window(pane);
                    }
                }
            }
            Done::Rendered { generation, result } => {
                let Key::Render(pane, _, draft) = key else { return };
                let current = self.preview.pane(pane).map(|o| o.want.generation);
                if current != Some(generation) {
                    self.preview.stats.stale += 1;
                    return;
                }
                let rendered = match result {
                    Ok(r) => r,
                    Err(e) => {
                        self.preview.stats.failures += 1;
                        if let Some(open) = self.preview.pane_mut(pane) {
                            open.failure = Some(e);
                        }
                        return;
                    }
                };
                self.preview.stats.renders += 1;
                self.preview.note_cost(&rendered);
                if !draft {
                    self.show(pane, generation, Quality::Final, rendered);
                    return;
                }
                let full_size = rendered.scale >= 1.0;
                let (ask, source) = (
                    Ask { recipe: parse(&rendered.recipe), texts: rendered.recipe.clone(), fit: rendered.fit, scale: 1.0 },
                    self.preview.pane(pane).map(|o| o.file.clone()),
                );
                self.show(pane, generation, Quality::Draft, rendered.clone());
                if full_size {
                    // The draft was the full render: it is the final too.
                    self.show(pane, generation, Quality::Final, rendered);
                } else if let Some(file) = source
                    && let Some(screen) = self.preview.source_of(&file, self.preview_ladder().render)
                {
                    self.submit_render(pane, generation, false, screen, ask);
                }
            }
            Done::Full { file, result } => {
                let zoomed: Vec<usize> =
                    self.preview.panes_on(&file).into_iter().filter(|p| self.preview.pane(*p).is_some_and(|o| o.window.is_some())).collect();
                if zoomed.is_empty() {
                    self.preview.stats.stale += 1;
                    return;
                }
                match result {
                    Ok(full) => {
                        self.preview.stats.full_decodes += 1;
                        for pane in zoomed {
                            if let Some(open) = self.preview.pane_mut(pane) {
                                open.full = Some(Arc::clone(&full));
                            }
                            self.request_window(pane);
                        }
                    }
                    Err(e) => {
                        self.preview.stats.failures += 1;
                        log::warn!("preview: no full-resolution window: {e}");
                    }
                }
            }
            Done::Region { generation, region, result } => {
                let Key::Region(pane, _) = key else { return };
                let Some(open) = self.preview.pane_mut(pane).filter(|o| o.window_asked.0 == generation) else {
                    self.preview.stats.stale += 1;
                    return;
                };
                match result {
                    Ok(image) => {
                        let (photo, ms) = (open.photo, ms_since(open.window_asked.1));
                        open.refined = Some(Refined { photo, generation, region, image });
                        self.preview.stats.renders += 1;
                        self.preview.push_update(PreviewUpdate { pane, photo, generation, kind: UpdateKind::Refined, ms });
                    }
                    Err(e) => {
                        self.preview.stats.failures += 1;
                        log::warn!("preview: {e}");
                    }
                }
            }
            Done::Stored { file, render, result } => match result {
                // The renders from now on draw from the source fitted to the screen size.
                Ok(Some(fitted)) => {
                    if self.preview.sources.iter().any(|(f, r, _)| *f == file && *r == render) {
                        self.preview.keep_source(file, render, Arc::new(Screen::new(fitted)));
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("preview: the screen entry was not stored: {e}"),
            },
            Done::Panicked(message) => {
                self.preview.stats.failures += 1;
                log::error!("preview: a job panicked: {message}");
                if let Key::Load(id) = key {
                    self.preview.loading.remove(&id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    /// A job submitted while the pool still runs a cancelled job of the same key (the photo opened
    /// again while its earlier load runs on) is not lost: it runs once the old one stops, and it is
    /// outstanding meanwhile. Before, the pool took no new job and the cancelled one delivered
    /// nothing, so the key stayed outstanding for ever and the edit view never settled.
    #[test]
    fn a_job_submitted_while_its_cancelled_predecessor_runs_still_delivers() {
        let mut p = Preview::default();
        let key = Key::Store(PhotoId::new(7));
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        p.submit(key, Priority::PREFETCH, move |_| {
            let _ = started_tx.send(());
            let _ = go_rx.recv_timeout(Duration::from_secs(30));
            Done::Panicked("the first job".into())
        });
        started_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        p.cancel(key);
        p.submit(key, Priority::PREFETCH, |_| Done::Panicked("the second job".into()));
        assert!(p.outstanding.contains(&key), "outstanding while it waits");
        go_tx.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(30);
        let delivered = loop {
            p.retry_deferred();
            if let Some(got) = p.pool.as_ref().and_then(|pool| pool.recv_timeout(Duration::from_millis(20))) {
                break Some(got);
            }
            if Instant::now() > until {
                break None;
            }
        };
        let (got, done) = delivered.expect("the second job delivered");
        assert_eq!(got, key);
        assert!(matches!(done, Done::Panicked(ref m) if m == "the second job"), "only the second job delivers");
        assert!(p.deferred.is_empty());
    }

    /// A deferred job cancelled before it could run is dropped, and nothing stays outstanding.
    #[test]
    fn a_deferred_job_cancelled_before_it_runs_is_dropped() {
        let mut p = Preview::default();
        let key = Key::Full(PhotoId::new(3));
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        p.submit(key, Priority::EDIT, move |_| {
            let _ = started_tx.send(());
            let _ = go_rx.recv_timeout(Duration::from_secs(30));
            Done::Panicked("the first job".into())
        });
        started_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        p.cancel(key);
        p.submit(key, Priority::EDIT, |_| Done::Panicked("the second job".into()));
        p.cancel(key);
        go_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        p.retry_deferred();
        assert!(p.outstanding.is_empty() && p.deferred.is_empty());
        assert!(p.pool.as_ref().and_then(|pool| pool.recv_timeout(Duration::from_millis(200))).is_none(), "nothing delivers");
    }
}
