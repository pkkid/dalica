//! The edit view's preview renders (`editing.md` §3, §5.2; `architecture.md` §5–§6), all off the
//! owner thread on the preview's own workers, the open photo's at the edit view's priority:
//!
//! - **The source.** Per open photo, its screen-size picture (the ladder's unedited screen entry
//!   when the thumbnail cache has it, else the file decoded under the limits and halved to between
//!   one and two screen sizes, then stored as that entry in the background) is kept in memory: the
//!   last [`SOURCES`] photos'.
//! - **The screen preview** of the photo's recipe ([`Session::open_preview`]) at the largest
//!   power-of-two scale of its source (1, ½, ¼ …) that still covers the photo area's box in device
//!   pixels, never above the source's resolution; the edit view draws it fitted to the box. A
//!   change of the photo's recipe (any history step) renders it again. Why powers of two: see
//!   `preview/jobs.rs` (the renderer's fractional resample is what a 2-core machine cannot afford).
//! - **A tool's candidate** ([`Session::set_preview_candidate`]) renders as a [`Quality::Draft`],
//!   at a lower scale only when the measured cost of a full render would miss
//!   [`DRAFT_TARGET_MS`], then as the [`Quality::Final`] (a full-scale draft is the final).
//! - **Zoomed past the preview** ([`Session::set_preview_window`]): the whole photo is decoded (kept
//!   while zoomed, when it fits in [`MAX_FULL_BYTES`]) and the visible window rendered at full
//!   resolution with `edit::render_region` ("refining" without blocking).
//! - **The neighbours** ([`Session::prefetch_previews`]): their sources are read and their previews
//!   rendered at the prefetch priority, so the next or previous photo opens without a decode.
//! - **Histograms**: each preview shown carries the 256-bin RGB [`Histogram`] of its pixels.
//! - Every request takes a new **generation**; a result of an older one is dropped (and its job
//!   cancelled when it has not started). Closing the photo cancels everything it asked for.
//!
//! The owner thread only submits jobs and takes their results in [`Session::poll`]: it never
//! decodes, renders or touches the disk.

mod histogram;
mod jobs;

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use dalica_core::PhotoId;
use dalica_edit::{Recipe, Region};
use dalica_image::Rgba8;
use dalica_thumbs::ladder::{Render, Source};
use dalica_thumbs::{Pool, Priority};

pub use self::histogram::{BINS, Histogram};
pub use self::jobs::halve;
use self::jobs::{Ask, Ladder, Origin, Rendered, Screen};
use crate::session::Session;

/// The preview's worker threads (renders run their bands in parallel on top of these).
pub const WORKERS: usize = 2;
/// Photos whose screen-size source stays in memory (the open one and its neighbours).
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

/// The quality of a preview shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// A tool's candidate rendered quickly while it changes (maybe at a lower scale).
    Draft,
    /// The preview at the box's full size.
    Final,
}

/// The preview the edit view shows.
#[derive(Clone, Debug)]
pub struct PreviewImage {
    pub photo: PhotoId,
    pub generation: u64,
    pub quality: Quality,
    pub image: Arc<Rgba8>,
    pub histogram: Arc<Histogram>,
    /// The recipe it shows (the photo's, or the tool's candidate).
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

/// One preview shown, with the time from its request to the poll that took it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewUpdate {
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
    /// The open photo's preview of a generation.
    Render(u64, bool),
    /// The open photo's whole decode.
    Full(PhotoId),
    /// A refined window of a generation.
    Region(u64),
    /// Storing a photo's screen entry in the thumbnail cache.
    Store(PhotoId),
}

enum Done {
    Loaded { photo: PhotoId, file: Source, render: Render, result: Result<(Arc<Screen>, Origin), String>, frame: Option<Result<Rendered, String>> },
    Rendered { generation: u64, result: Result<Rendered, String> },
    Full { file: Source, result: Result<Arc<Rgba8>, String> },
    Region { generation: u64, region: Region, result: Result<Arc<Rgba8>, String> },
    Stored { file: Source, render: Render, result: Result<Option<Arc<Rgba8>>, String> },
    Panicked(String),
}

/// What the open photo asked for last.
struct Want {
    generation: u64,
    recipe: Vec<String>,
    started: Instant,
    draft: bool,
}

struct Open {
    photo: PhotoId,
    file: Source,
    fit: (u32, u32),
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
    open: Option<Open>,
    /// Screen-size sources, most recently used first.
    sources: VecDeque<(Source, Render, Arc<Screen>)>,
    /// Prefetched previews, most recently made first.
    ready: VecDeque<(PhotoId, Rendered)>,
    /// Source loads on their way, with their priority.
    loading: HashMap<PhotoId, Priority>,
    prefetching: HashSet<PhotoId>,
    outstanding: HashSet<Key>,
    generation: u64,
    /// Milliseconds per output megapixel of the last renders.
    cost: Option<f64>,
    /// The last box asked for (prefetch renders at it).
    fit: Option<(u32, u32)>,
    updates: VecDeque<PreviewUpdate>,
    stats: PreviewStats,
}

impl Preview {
    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.generation
    }

    fn submit(&mut self, key: Key, priority: Priority, work: impl FnOnce(&AtomicBool) -> Done + Send + 'static) {
        let pool = self.pool.get_or_insert_with(|| Pool::new(WORKERS));
        let wrapped = move |cancel: &AtomicBool| {
            catch_unwind(AssertUnwindSafe(|| work(cancel))).unwrap_or_else(|panic| {
                let message =
                    panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                Done::Panicked(message)
            })
        };
        pool.submit(key, priority, wrapped);
        self.outstanding.insert(key);
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
        let open = self.open.as_ref().map(|o| o.file.clone());
        while self.sources.len() > SOURCES {
            // The open photo's source is never the one dropped.
            match self.sources.iter().rposition(|(f, _, _)| Some(f) != open.as_ref()) {
                Some(at) => {
                    self.sources.remove(at);
                }
                None => break,
            }
        }
    }

    fn keep_ready(&mut self, photo: PhotoId, rendered: Rendered) {
        self.ready.retain(|(p, _)| *p != photo);
        self.ready.push_front((photo, rendered));
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
}

impl Session {
    /// How the thumbnail ladder makes its screen-size entry.
    fn preview_ladder(&self) -> Ladder {
        let (cache, render, screen, limits) = self.thumbs.screen_source();
        Ladder { cache, render, screen, limits }
    }

    /// Opens photo `id` in the edit view's preview, covering `fit` (the photo area in device
    /// pixels): its source is read (or taken from memory) and its recipe rendered at the edit
    /// view's priority. Opening another photo first closes the one open.
    pub fn open_preview(&mut self, id: PhotoId, fit: (u32, u32)) -> Result<(), PreviewError> {
        let file = file_of(self, id).ok_or(PreviewError::NoPhoto)?;
        if self.preview.open.as_ref().is_some_and(|o| o.photo != id) {
            self.close_preview();
        }
        let fit = clamp_fit(fit);
        self.preview.fit = Some(fit);
        let generation = self.preview.next_generation();
        let recipe = self.library.info(id).edits.clone();
        let reopened = self.preview.open.as_ref().is_some_and(|o| o.photo == id && o.file == file);
        if !reopened {
            self.preview.open = Some(Open {
                photo: id,
                file,
                fit,
                candidate: None,
                want: Want { generation, recipe, started: Instant::now(), draft: false },
                frame: None,
                window: None,
                full: None,
                refined: None,
                window_asked: (0, Instant::now()),
                failure: None,
            });
        } else if let Some(open) = &mut self.preview.open {
            open.fit = fit;
            open.candidate = None;
        }
        self.request_preview(false);
        Ok(())
    }

    /// Closes the open photo: every job it asked for is cancelled and its preview, window and
    /// full decode dropped. Sources and prefetched previews stay in memory.
    pub fn close_preview(&mut self) {
        let Some(open) = self.preview.open.take() else { return };
        let photo = open.photo;
        self.preview.cancel_where(|k| match k {
            Key::Load(id) | Key::Full(id) => *id == photo,
            Key::Render(..) | Key::Region(_) => true,
            Key::Store(_) => false,
        });
    }

    /// The photo area changed size: the preview is made to cover `fit` again.
    pub fn set_preview_fit(&mut self, fit: (u32, u32)) -> Result<(), PreviewError> {
        let fit = clamp_fit(fit);
        let open = self.preview.open.as_mut().ok_or(PreviewError::NotOpen)?;
        if open.fit != fit {
            open.fit = fit;
            self.preview.fit = Some(fit);
            self.request_preview(false);
        }
        Ok(())
    }

    /// A tool's candidate recipe (`Some`: the whole recipe as it would be stored), rendered as a
    /// draft and then final; `None` goes back to the photo's own recipe.
    pub fn set_preview_candidate(&mut self, recipe: Option<&[String]>) -> Result<(), PreviewError> {
        if let Some(texts) = recipe {
            Recipe::parse(texts).map_err(|e| PreviewError::BadRecipe(e.to_string()))?;
        }
        let open = self.preview.open.as_mut().ok_or(PreviewError::NotOpen)?;
        open.candidate = recipe.map(<[String]>::to_vec);
        let draft = open.candidate.is_some();
        self.request_preview(draft);
        Ok(())
    }

    /// The visible window while zoomed past the preview (`Some`: in the displayed picture's pixels
    /// at `region.scale` output pixels per pixel of the whole photo), refined at full resolution in
    /// the background; `None` (fitted again) drops the window and the whole decode.
    pub fn set_preview_window(&mut self, window: Option<Region>) -> Result<(), PreviewError> {
        if let Some(r) = window {
            if !(r.scale.is_finite() && r.scale > 0.0 && r.scale <= dalica_edit::render::MAX_SCALE) {
                return Err(PreviewError::BadWindow("the scale must be finite, positive and at most the renderer's largest"));
            }
            if r.width == 0 || r.height == 0 || u64::from(r.width) * u64::from(r.height) > MAX_WINDOW_PIXELS {
                return Err(PreviewError::BadWindow("the window must hold between one pixel and the most a window may"));
            }
        }
        let open = self.preview.open.as_mut().ok_or(PreviewError::NotOpen)?;
        if open.window == window {
            return Ok(());
        }
        open.window = window;
        if window.is_none() {
            open.full = None;
            open.refined = None;
            let photo = open.photo;
            self.preview.cancel_where(|k| matches!(k, Key::Region(_)) || *k == Key::Full(photo));
            return Ok(());
        }
        self.request_window();
        Ok(())
    }

    /// Prefetches the previews of `ids` (the next and previous photos) at the prefetch priority:
    /// their sources read and their recipes rendered at the open photo's box. Prefetch of photos
    /// no longer listed is cancelled.
    pub fn prefetch_previews(&mut self, ids: &[PhotoId]) {
        let Some(fit) = self.preview.fit else { return };
        let open = self.preview.open.as_ref().map(|o| o.photo);
        let ladder = self.preview_ladder();
        let wanted: Vec<PhotoId> = ids.iter().copied().filter(|id| Some(*id) != open).take(MAX_PREFETCH).collect();
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
            let ready = self.preview.ready.iter().any(|(p, r)| *p == id && r.recipe == texts && r.fit == fit && r.scale >= 1.0);
            if (have && ready) || self.preview.loading.contains_key(&id) {
                continue;
            }
            let ask = Ask { recipe: parse(&texts), texts, fit, scale: 1.0 };
            self.submit_load(id, file, Some(ask), Priority::PREFETCH);
            self.preview.prefetching.insert(id);
        }
    }

    /// The preview shown now, if any.
    pub fn preview(&self) -> Option<&PreviewImage> {
        self.preview.open.as_ref().and_then(|o| o.frame.as_ref())
    }

    /// The refined window shown now, if zoomed and refined.
    pub fn preview_refined(&self) -> Option<&Refined> {
        self.preview.open.as_ref().and_then(|o| o.refined.as_ref())
    }

    /// The photo open in the preview.
    pub fn preview_photo(&self) -> Option<PhotoId> {
        self.preview.open.as_ref().map(|o| o.photo)
    }

    /// Why the open photo's preview could not be made.
    pub fn preview_failure(&self) -> Option<&str> {
        self.preview.open.as_ref().and_then(|o| o.failure.as_deref())
    }

    /// The previews shown since the last call, oldest first (at most [`MAX_UPDATES`]).
    pub fn take_preview_updates(&mut self) -> Vec<PreviewUpdate> {
        self.preview.updates.drain(..).collect()
    }

    /// Preview jobs queued or running.
    pub fn preview_pending(&self) -> usize {
        self.preview.outstanding.len()
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

    /// Waits up to `timeout` until every preview job has finished and been applied (CLI, tests,
    /// benches). Returns whether it got there.
    pub fn wait_preview(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.check_preview_recipe();
            if self.preview.outstanding.is_empty() {
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let Some(pool) = &self.preview.pool else { return true };
            if let Some((key, done)) = pool.recv_timeout(left.min(Duration::from_millis(20))) {
                self.handle_preview(key, done);
            }
        }
    }

    /// Waits up to `timeout` for the next preview shown, applying results as they come (benches,
    /// tests).
    pub fn wait_preview_update(&mut self, timeout: Duration) -> Option<PreviewUpdate> {
        let until = Instant::now() + timeout;
        loop {
            if let Some(u) = self.preview.updates.pop_front() {
                return Some(u);
            }
            self.check_preview_recipe();
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() || self.preview.outstanding.is_empty() && self.preview.updates.is_empty() {
                return self.preview.updates.pop_front();
            }
            let pool = self.preview.pool.as_ref()?;
            if let Some((key, done)) = pool.recv_timeout(left.min(Duration::from_millis(20))) {
                self.handle_preview(key, done);
            }
        }
    }

    /// Applies the preview renders that finished; returns how many. Never blocks and never touches
    /// the disk.
    pub(crate) fn poll_preview(&mut self) -> usize {
        if self.preview.pool.is_none() {
            return 0;
        }
        self.check_preview_recipe();
        let mut n = 0;
        while let Some((key, done)) = self.preview.pool.as_ref().and_then(Pool::try_recv) {
            self.handle_preview(key, done);
            n += 1;
        }
        n
    }

    /// The open photo's recipe changed (a history step) or the photo left: render it again, or
    /// close.
    fn check_preview_recipe(&mut self) {
        let Some(open) = &self.preview.open else { return };
        if self.library.photo(open.photo).is_none() {
            self.close_preview();
            return;
        }
        if file_of(self, open.photo).is_some_and(|f| f != open.file) {
            // The file changed: read it again.
            let (id, fit) = (open.photo, open.fit);
            self.close_preview();
            let _ = self.open_preview(id, fit);
            return;
        }
        if open.candidate.is_none() && self.library.info(open.photo).edits != open.want.recipe {
            self.request_preview(false);
        }
    }

    /// A new generation of the open photo's preview: older renders are cancelled, and the
    /// preview is rendered from the source in memory (a draft first when `draft`), taken from a
    /// prefetched one, or rendered once the source is read.
    fn request_preview(&mut self, draft: bool) {
        let ladder = self.preview_ladder();
        let generation = self.preview.next_generation();
        self.preview.cancel_where(|k| matches!(k, Key::Render(..)));
        let Some(open) = self.preview.open.as_mut() else { return };
        let texts = open.candidate.clone().unwrap_or_else(|| self.library.info(open.photo).edits.clone());
        open.want = Want { generation, recipe: texts.clone(), started: Instant::now(), draft };
        open.failure = None;
        let (photo, file, fit) = (open.photo, open.file.clone(), open.fit);
        if open.window.is_some() {
            self.request_window();
        }
        if !draft
            && let Some(at) = self.preview.ready.iter().position(|(p, r)| *p == photo && r.recipe == texts && r.fit == fit && r.scale >= 1.0)
            && let Some((_, rendered)) = self.preview.ready.get(at).cloned()
        {
            self.show(generation, Quality::Final, rendered);
            return;
        }
        let Some(screen) = self.preview.source_of(&file, ladder.render) else {
            let ask = Ask { recipe: parse(&texts), texts, fit, scale: 1.0 };
            self.submit_load(photo, file, Some(ask), Priority::EDIT);
            return;
        };
        let scale = if draft { self.preview.draft_scale() } else { 1.0 };
        self.submit_render(generation, draft, screen, Ask { recipe: parse(&texts), texts, fit, scale });
    }

    fn submit_render(&mut self, generation: u64, draft: bool, screen: Arc<Screen>, ask: Ask) {
        self.preview
            .submit(Key::Render(generation, draft), Priority::EDIT, move |_| Done::Rendered { generation, result: jobs::render(&screen, &ask) });
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
        self.preview.submit(Key::Load(id), priority, move |_| {
            let result = match have {
                Some(screen) => Ok((screen, Origin::Memory)),
                None => jobs::read(&file, &ladder, MAX_FULL_BYTES).map(|(image, origin)| (Arc::new(Screen::new(Arc::new(image))), origin)),
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

    /// Asks for the open photo's refined window: the whole decode first, then the window.
    fn request_window(&mut self) {
        let ladder = self.preview_ladder();
        let generation = self.preview.next_generation();
        self.preview.cancel_where(|k| matches!(k, Key::Region(_)));
        let Some(open) = self.preview.open.as_mut() else { return };
        let Some(region) = open.window else { return };
        open.window_asked = (generation, Instant::now());
        let (photo, file, texts) = (open.photo, open.file.clone(), open.want.recipe.clone());
        match open.full.clone() {
            Some(full) => {
                let recipe = parse(&texts);
                self.preview.submit(Key::Region(generation), Priority::EDIT, move |_| Done::Region {
                    generation,
                    region,
                    result: jobs::region(&full, &recipe, region).map(Arc::new),
                });
            }
            None => {
                if !self.preview.outstanding.contains(&Key::Full(photo)) {
                    self.preview.submit(Key::Full(photo), Priority::EDIT, move |_| Done::Full {
                        result: jobs::full(&file, &ladder, MAX_FULL_BYTES).map(Arc::new),
                        file,
                    });
                }
            }
        }
    }

    /// Shows `rendered` as the open photo's preview of `generation`.
    fn show(&mut self, generation: u64, quality: Quality, rendered: Rendered) {
        let Some(open) = self.preview.open.as_mut() else { return };
        let ms = ms_since(open.want.started);
        let photo = open.photo;
        open.frame = Some(PreviewImage { photo, generation, quality, image: rendered.image, histogram: rendered.histogram, recipe: rendered.recipe });
        let kind = if quality == Quality::Draft { UpdateKind::Draft } else { UpdateKind::Final };
        self.preview.push_update(PreviewUpdate { photo, generation, kind, ms });
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
                        if let Some(open) = self.preview.open.as_mut().filter(|o| o.photo == photo && o.file == file) {
                            open.failure = Some(e);
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
                    self.preview.keep_ready(photo, r.clone());
                }
                let Some(open) = self.preview.open.as_ref().filter(|o| o.photo == photo && o.file == file) else { return };
                let (generation, wanted, fit, draft) = (open.want.generation, open.want.recipe.clone(), open.fit, open.want.draft);
                match frame {
                    Some(r) if r.recipe == wanted && r.fit == fit => self.show(generation, Quality::Final, r),
                    _ => self.request_preview(draft),
                }
                if self.preview.open.as_ref().is_some_and(|o| o.window.is_some()) {
                    self.request_window();
                }
            }
            Done::Rendered { generation, result } => {
                let current = self.preview.open.as_ref().map(|o| o.want.generation);
                if current != Some(generation) {
                    self.preview.stats.stale += 1;
                    return;
                }
                let rendered = match result {
                    Ok(r) => r,
                    Err(e) => {
                        self.preview.stats.failures += 1;
                        if let Some(open) = self.preview.open.as_mut() {
                            open.failure = Some(e);
                        }
                        return;
                    }
                };
                self.preview.stats.renders += 1;
                self.preview.note_cost(&rendered);
                let Key::Render(_, draft) = key else { return };
                if !draft {
                    self.show(generation, Quality::Final, rendered);
                    return;
                }
                let full_size = rendered.scale >= 1.0;
                let (ask, source) = (
                    Ask { recipe: parse(&rendered.recipe), texts: rendered.recipe.clone(), fit: rendered.fit, scale: 1.0 },
                    self.preview.open.as_ref().map(|o| o.file.clone()),
                );
                self.show(generation, Quality::Draft, rendered.clone());
                if full_size {
                    // The draft was the full render: it is the final too.
                    self.show(generation, Quality::Final, rendered);
                } else if let Some(file) = source
                    && let Some(screen) = self.preview.source_of(&file, self.preview_ladder().render)
                {
                    self.submit_render(generation, false, screen, ask);
                }
            }
            Done::Full { file, result } => {
                let Some(open) = self.preview.open.as_mut().filter(|o| o.file == file && o.window.is_some()) else {
                    self.preview.stats.stale += 1;
                    return;
                };
                match result {
                    Ok(full) => {
                        self.preview.stats.full_decodes += 1;
                        open.full = Some(full);
                        self.request_window();
                    }
                    Err(e) => {
                        self.preview.stats.failures += 1;
                        log::warn!("preview: no full-resolution window: {e}");
                    }
                }
            }
            Done::Region { generation, region, result } => {
                let Some(open) = self.preview.open.as_mut().filter(|o| o.window_asked.0 == generation) else {
                    self.preview.stats.stale += 1;
                    return;
                };
                match result {
                    Ok(image) => {
                        let (photo, ms) = (open.photo, ms_since(open.window_asked.1));
                        open.refined = Some(Refined { photo, generation, region, image });
                        self.preview.stats.renders += 1;
                        self.preview.push_update(PreviewUpdate { photo, generation, kind: UpdateKind::Refined, ms });
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
