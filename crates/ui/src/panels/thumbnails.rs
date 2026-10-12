//! Thumbnails for the light table, never blank (`architecture.md` §6.5): each cell draws the best
//! the session has, the same frame, and never waits.
//!
//! **What a cell draws**, best first ([`Drawn`]):
//! 1. the ladder size that covers it (`Drawn::Image`);
//! 2. else the next smaller size, scaled up while the covering one loads (`Drawn::Upscaled`);
//! 3. else the photo's colour sketch from the index, drawn as a small vertex-coloured mesh with
//!    no texture and no upload (`Drawn::Sketch`);
//! 4. else, only for a photo never scanned, the `gray 38` placeholder (`Drawn::Placeholder`).
//!
//! A photo whose thumbnail failed (a missing or unreadable file) draws the placeholder whatever it
//! has (`Drawn::Failed`; the cell adds the "!" and the reason). A file deleted after its thumbnails
//! were made is noticed when the session next scans or verifies it (the watcher, M5): until then
//! its cached thumbnails show.
//!
//! **The painting API** (fixed by the light-table skeleton). Each frame the light table calls, in
//! order:
//!
//! 1. [`Thumbnails::begin_frame`] once, before anything is painted: notes the theme and the
//!    display's size (for the screen-size ladder entry);
//! 2. [`Thumbnails::paint`] once per cell on screen, with the photo ([`PhotoRef`]), the image's
//!    letterboxed rect in points and its long side in device pixels (which ladder size covers it);
//!    it paints from what is already uploaded or in the index (never the disk, never a decode) and
//!    says what it drew;
//! 3. [`Thumbnails::set_order`] with the sections (the photos in display order, flattened once per
//!    sections generation), then [`Thumbnails::end_frame`] once, after the last cell, with how the
//!    view moved ([`Motion`]).
//!
//! **End of frame.** The view's speed and direction go to the session's prefetch
//! ([`Session::prefetch_thumbnails`]: further ahead the faster it moves, stale prefetch cancelled).
//! While the view moves no faster than [`REQUEST_MAX_SCREENS`] screens a second, every cell that
//! did not draw its covering size asks the session for it ([`Session::thumbnail`], the visible
//! priority) and what the session has in memory is uploaded: at most [`UPLOADS_PER_FRAME`]
//! textures and [`UPLOAD_BYTES_PER_FRAME`] bytes a frame, cells showing their sketch first, the
//! rest the next frame. Faster than that, cells keep their sketch or what is uploaded and nothing
//! is asked at the visible priority, so a fast scroll leaves no backlog of visible jobs behind it.
//! Textures are kept per (photo, ladder size) and evicted once their photo is more than one screen
//! of cells from the view; beyond [`MAX_TEXTURES`] or [`TEXTURE_BYTES`] the farthest go first (a
//! cell on screen never loses its texture); a smaller size goes once its photo drew a covering one.
//!
//! The view's speed is held for [`SPEED_HOLD_SECS`] (decaying), so the still frames between a
//! wheel's steps do not count as stopping. The counters of the last frame ([`Counts`]) go out
//! through `ui.inspect` → `perf`; [`Totals`] sums them over every frame (tests).
//! [`Thumbnails::pending`] says how many cells on screen still wait for their covering size
//! (`ui.settle` waits for it).

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_engine::thumbs::Size;
use dalica_engine::{Refreshed, Session, Thumb};
use dalica_library::Photo;
use dalica_thumbs::Rgba8;
use dalica_thumbs::ladder::{self, Choice};
use dalica_ui_kit::Tokens;
use egui::{Color32, Mesh, Painter, Rect, Shape, pos2};
use serde::Serialize;

use super::light_table::Sections;

/// The most thumbnail textures kept.
pub const MAX_TEXTURES: usize = 600;
/// The most texture memory kept for thumbnails.
pub const TEXTURE_BYTES: usize = 256 << 20;
/// Textures uploaded per frame at most, so a burst of arrivals stays inside the frame budget.
pub const UPLOADS_PER_FRAME: usize = 24;
/// Bytes uploaded per frame at most (one upload always goes, however large).
pub const UPLOAD_BYTES_PER_FRAME: usize = 4 << 20;
/// Above this speed (screens per second) cells ask nothing at the visible priority: they keep their
/// sketch, and prefetch alone works ahead.
pub const REQUEST_MAX_SCREENS: f64 = 4.0;
/// How long a burst of speed is remembered (seconds, the decay's time constant): a wheel moves the
/// view in steps with still frames between them, which should not count as stopping.
pub const SPEED_HOLD_SECS: f64 = 0.1;
/// Vertices along the long side of a sketch's mesh.
pub const SKETCH_GRID: u32 = 8;
/// Sketch meshes kept decoded, past which those not drawn this frame are dropped.
const MAX_SKETCHES: usize = 4096;
/// Without the display order, a texture not drawn for this many frames is evicted.
const UNORDERED_FRAMES: u64 = 120;

/// The photo a cell shows, with what the thumbnails may need to find its pixels.
#[derive(Clone, Copy)]
pub struct PhotoRef<'a> {
    pub session: &'a Session,
    pub photo: &'a Photo,
}

impl PhotoRef<'_> {
    /// The photo's file, if its folder is known.
    pub fn path(&self) -> Option<PathBuf> {
        self.session.library().folder(self.photo.folder).map(|f| f.path.join(&self.photo.name))
    }
}

/// What [`Thumbnails::paint`] drew into a cell's image rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    /// The ladder size that covers the cell.
    Image,
    /// A smaller ladder size scaled up, while the covering one loads.
    Upscaled,
    /// The colour sketch, while no ladder size is uploaded.
    Sketch,
    /// The placeholder: the photo was never scanned.
    Placeholder,
    /// The placeholder, for good: the photo cannot be shown ([`Thumbnails::failure`] says why).
    Failed,
}

impl Drawn {
    /// Something better is still to come for this cell.
    pub fn waiting(self) -> bool {
        matches!(self, Drawn::Upscaled | Drawn::Sketch | Drawn::Placeholder)
    }
}

/// How the light table's view moved this frame, for prefetch.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    /// The view's top in content points.
    pub offset: f64,
    /// The view's height in points.
    pub view_h: f32,
    /// Points per second, positive scrolling down.
    pub velocity: f64,
}

/// The counters of one frame (`ui.inspect` → `perf`, merged in under these camelCase names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Cells that drew their covering size.
    pub covered_cells: u64,
    /// Cells that drew a smaller size scaled up.
    pub upscaled_cells: u64,
    /// Cells that drew their sketch.
    pub sketch_cells: u64,
    /// Cells that drew the placeholder (never scanned): 0 while scrolling a scanned library.
    pub placeholder_cells: u64,
    /// Cells whose thumbnail failed.
    pub failed_cells: u64,
    /// Thumbnail textures uploaded at the end of the frame, and their bytes.
    pub thumb_uploads: u64,
    pub thumb_upload_bytes: u64,
    /// Images in memory left for a later frame by the upload caps.
    pub thumb_uploads_deferred: u64,
    /// Thumbnail textures alive after the frame, and their bytes.
    pub thumb_textures: u64,
    pub thumb_texture_bytes: u64,
    /// Thumbnail textures evicted at the end of the frame.
    pub thumb_evictions: u64,
}

/// The counters summed over every frame since the thumbnails were made (tests, benches): the
/// cells by what they drew, and the most textures and uploads of any one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub frames: u64,
    pub covered_cells: u64,
    pub upscaled_cells: u64,
    pub sketch_cells: u64,
    pub placeholder_cells: u64,
    pub failed_cells: u64,
    pub uploads: u64,
    pub max_uploads: u64,
    pub max_textures: u64,
    pub max_texture_bytes: u64,
}

impl Totals {
    fn add(&mut self, c: &Counts) {
        self.frames = self.frames.saturating_add(1);
        self.covered_cells = self.covered_cells.saturating_add(c.covered_cells);
        self.upscaled_cells = self.upscaled_cells.saturating_add(c.upscaled_cells);
        self.sketch_cells = self.sketch_cells.saturating_add(c.sketch_cells);
        self.placeholder_cells = self.placeholder_cells.saturating_add(c.placeholder_cells);
        self.failed_cells = self.failed_cells.saturating_add(c.failed_cells);
        self.uploads = self.uploads.saturating_add(c.thumb_uploads);
        self.max_uploads = self.max_uploads.max(c.thumb_uploads);
        self.max_textures = self.max_textures.max(c.thumb_textures);
        self.max_texture_bytes = self.max_texture_bytes.max(c.thumb_texture_bytes);
    }
}

/// One uploaded ladder image.
struct Texture {
    handle: egui::TextureHandle,
    /// The session's image it was made from (to notice a newer one); not kept alive.
    source: Weak<Rgba8>,
    bytes: usize,
    /// The frame it was last drawn in.
    drawn: u64,
}

/// A sketch, decoded once into the colours of its mesh.
struct SketchMesh {
    sketch: dalica_library::Sketch,
    cols: u32,
    rows: u32,
    colours: Vec<Color32>,
    drawn: u64,
}

/// One cell painted this frame.
#[derive(Debug, Clone, Copy)]
struct Cell {
    id: PhotoId,
    need: u32,
    drawn: Drawn,
}

/// The photos in display order, flattened from the sections.
#[derive(Default)]
struct Order {
    generation: Option<u64>,
    ids: Vec<PhotoId>,
    index: HashMap<PhotoId, usize>,
}

pub struct Thumbnails {
    ctx: Option<egui::Context>,
    textures: HashMap<(PhotoId, Size), Texture>,
    texture_bytes: usize,
    sketches: HashMap<PhotoId, SketchMesh>,
    /// Why the failed photos drawn lately failed.
    failed: HashMap<PhotoId, String>,
    order: Order,
    /// The cells of this frame, in paint order.
    cells: Vec<Cell>,
    frame: u64,
    /// The display's long side in device pixels, for the screen-size entry.
    screen: u32,
    /// This frame's counters, then the last finished frame's.
    counting: Counts,
    counts: Counts,
    totals: Totals,
    pending: usize,
    /// The clock of this frame and of the last one that ended (seconds).
    now: f64,
    ended: Option<f64>,
    /// The view's velocity (points per second), bursts held for [`SPEED_HOLD_SECS`].
    speed: f64,
    /// `gray 38` from the theme of the current frame.
    placeholder: Color32,
    /// The session's thumbnail generation the textures were made in.
    made_in: u64,
}

impl Thumbnails {
    pub fn new() -> Thumbnails {
        Thumbnails {
            ctx: None,
            textures: HashMap::new(),
            texture_bytes: 0,
            made_in: 0,
            sketches: HashMap::new(),
            failed: HashMap::new(),
            order: Order::default(),
            cells: Vec::new(),
            frame: 0,
            screen: ladder::DEFAULT_SCREEN,
            counting: Counts::default(),
            counts: Counts::default(),
            totals: Totals::default(),
            pending: 0,
            now: 0.0,
            ended: None,
            speed: 0.0,
            placeholder: Tokens::default().placeholder,
        }
    }

    /// Starts a frame: notes the theme and, when the display's size is known, tells the session
    /// (the screen-size ladder entry). Returns the textures alive.
    pub fn begin_frame(&mut self, ctx: &egui::Context, session: &mut Session, t: &Tokens) -> usize {
        self.placeholder = t.placeholder;
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
        self.frame = self.frame.wrapping_add(1);
        self.cells.clear();
        // Thumbnails made again (colour management switched, a refresh): their textures go, so the
        // cells ask for the new images.
        let generation = session.thumbnail_generation();
        if generation != self.made_in {
            match session.thumbnails_refreshed_since(self.made_in) {
                Refreshed::Nothing => {}
                Refreshed::All => {
                    self.textures.clear();
                    self.texture_bytes = 0;
                }
                Refreshed::Photos(ids) => {
                    for id in ids {
                        for size in Size::ALL {
                            self.remove(id, size);
                        }
                    }
                }
            }
            self.made_in = generation;
        }
        self.counting = Counts::default();
        self.now = ctx.input(|i| i.time);
        let monitor = ctx.input(|i| i.viewport().monitor_size.map(|s| s.x.max(s.y) * i.pixels_per_point()));
        if let Some(long) = monitor.filter(|l| l.is_finite() && *l >= 1.0) {
            let long = (long.round() as u32).clamp(1, ladder::MAX_SCREEN);
            if long != self.screen {
                self.screen = long;
                session.set_screen_size(long);
            }
        }
        self.textures.len()
    }

    /// Paints `photo` into `rect` (its letterboxed image, in points) for an image `device_px` device
    /// pixels on its long side: the best uploaded ladder size, else its sketch, else the
    /// placeholder. Never touches the disk and never decodes an image.
    pub fn paint(&mut self, painter: &Painter, photo: PhotoRef<'_>, rect: Rect, device_px: f32) -> Drawn {
        let id = photo.photo.id;
        let need = need_of(device_px);
        let frame = self.frame;
        let choice = ladder::choose(need, self.screen, |s| self.textures.contains_key(&(id, s)));
        let texture = match choice {
            Choice::Covers(size) | Choice::Smaller(size) => self.textures.get_mut(&(id, size)),
            Choice::Nothing => None,
        };
        let drawn = if let Some(why) = photo.session.thumbnail_failure(id) {
            // A missing or unreadable file: the placeholder under the cell's "!", whatever was
            // drawn of it before.
            painter.rect_filled(rect, 0, self.placeholder);
            if self.failed.get(&id).is_none_or(|w| w != why) {
                self.failed.insert(id, why.to_string());
            }
            Drawn::Failed
        } else if let Some(tex) = texture {
            tex.drawn = frame;
            painter.image(tex.handle.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            if matches!(choice, Choice::Covers(_)) { Drawn::Image } else { Drawn::Upscaled }
        } else {
            match photo.session.library().sketch(id) {
                Some(sketch) if self.paint_sketch(painter, id, sketch, rect) => Drawn::Sketch,
                _ => {
                    painter.rect_filled(rect, 0, self.placeholder);
                    Drawn::Placeholder
                }
            }
        };
        let c = &mut self.counting;
        let slot = match drawn {
            Drawn::Image => &mut c.covered_cells,
            Drawn::Upscaled => &mut c.upscaled_cells,
            Drawn::Sketch => &mut c.sketch_cells,
            Drawn::Placeholder => &mut c.placeholder_cells,
            Drawn::Failed => &mut c.failed_cells,
        };
        *slot = slot.saturating_add(1);
        self.cells.push(Cell { id, need, drawn });
        drawn
    }

    /// Draws `sketch` over `rect` as a vertex-coloured mesh; `false` when it does not decode.
    fn paint_sketch(&mut self, painter: &Painter, id: PhotoId, sketch: &dalica_library::Sketch, rect: Rect) -> bool {
        let frame = self.frame;
        let fresh = self.sketches.get(&id).is_some_and(|m| m.sketch == *sketch);
        if !fresh {
            let Ok(decoded) = dalica_thumbs::sketch::decode(sketch.as_bytes()) else { return false };
            let (cols, rows) = decoded.size(SKETCH_GRID);
            let colours = decoded.grid(cols, rows).into_iter().map(|[r, g, b, _]| Color32::from_rgb(r, g, b)).collect();
            self.sketches.insert(id, SketchMesh { sketch: *sketch, cols, rows, colours, drawn: frame });
        }
        let Some(m) = self.sketches.get_mut(&id) else { return false };
        m.drawn = frame;
        painter.add(sketch_shape(rect, m.cols, m.rows, &m.colours));
        true
    }

    /// Takes the photos in display order from the light table's sections; flattened again only when
    /// their generation changes.
    pub fn set_order(&mut self, sections: &Sections) {
        if self.order.generation == Some(sections.generation()) {
            return;
        }
        let ids: Vec<PhotoId> = sections.list.iter().flat_map(|s| s.photos.iter().copied()).collect();
        self.order.index = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        self.order.ids = ids;
        self.order.generation = Some(sections.generation());
    }

    /// Ends a frame: prefetch in the scroll direction, requests and uploads for the cells that did
    /// not draw their covering size, eviction, and the frame's counters.
    pub fn end_frame(&mut self, session: &mut Session, motion: &Motion) {
        let jobs_before = session.thumbnails_pending();
        let view_h = if motion.view_h.is_finite() && motion.view_h > 1.0 { f64::from(motion.view_h) } else { 1.0 };
        let velocity = self.hold_speed(motion.velocity);
        let visible = self.visible_range();
        let need = self.cells.iter().map(|c| c.need).max().unwrap_or(1);

        // Prefetch: the velocity in cells per second, from the cells a screen holds.
        if let Some(range) = visible.clone() {
            let cells_per_s = velocity / view_h * self.cells.len().max(1) as f64;
            session.prefetch_thumbnails(&self.order.ids, range, cells_per_s, need);
        }

        // Requests and uploads, while the view moves slowly enough to look at.
        let slow = velocity.abs() / view_h <= REQUEST_MAX_SCREENS;
        let mut arrived: Vec<(PhotoId, Size, Arc<Rgba8>, Drawn)> = Vec::new();
        if slow {
            for cell in self.cells.iter().filter(|c| c.drawn.waiting()) {
                if let Thumb::Image { size, image, .. } = session.thumbnail(cell.id, cell.need) {
                    let current = self.textures.get(&(cell.id, size)).and_then(|t| t.source.upgrade()).is_some_and(|s| Arc::ptr_eq(&s, &image));
                    if !current {
                        arrived.push((cell.id, size, image, cell.drawn));
                    }
                }
            }
        }
        // Cells with nothing better than a sketch first.
        arrived.sort_by_key(|(_, _, _, drawn)| *drawn == Drawn::Upscaled);
        let (mut uploads, mut bytes, mut deferred) = (0usize, 0usize, 0u64);
        for (id, size, image, _) in arrived {
            let len = image.data.len();
            if uploads >= UPLOADS_PER_FRAME || (uploads > 0 && bytes.saturating_add(len) > UPLOAD_BYTES_PER_FRAME) {
                deferred += 1;
                continue;
            }
            if self.upload(id, size, &image) {
                uploads += 1;
                bytes = bytes.saturating_add(len);
            }
        }
        let evicted = self.evict(visible);
        if self.failed.len() > MAX_SKETCHES {
            let drawn: std::collections::HashSet<PhotoId> = self.cells.iter().filter(|c| c.drawn == Drawn::Failed).map(|c| c.id).collect();
            self.failed.retain(|id, _| drawn.contains(id));
        }

        let jobs = session.thumbnails_pending();
        if let Some(ctx) = &self.ctx {
            crate::perf::count(ctx, crate::perf::Counter::ThumbRequests, jobs.saturating_sub(jobs_before) as u64);
        }
        let waiting = jobs > 0 || deferred > 0 || uploads > 0 || !slow;
        self.pending = if waiting { self.cells.iter().filter(|c| c.drawn.waiting()).count() } else { 0 };
        let mut c = self.counting;
        c.thumb_uploads = uploads as u64;
        c.thumb_upload_bytes = bytes as u64;
        c.thumb_uploads_deferred = deferred;
        c.thumb_textures = self.textures.len() as u64;
        c.thumb_texture_bytes = self.texture_bytes as u64;
        c.thumb_evictions = evicted;
        self.counts = c;
        self.totals.add(&c);
    }

    /// The view's velocity with its bursts held: this frame's when it is faster than the last
    /// one decayed over the time between them, else that.
    fn hold_speed(&mut self, velocity: f64) -> f64 {
        let v = if velocity.is_finite() { velocity } else { 0.0 };
        let dt = self.ended.map_or(0.0, |t| self.now - t);
        let decay = if dt.is_finite() && dt > 0.0 { (-dt / SPEED_HOLD_SECS).exp() } else { 1.0 };
        let held = self.speed * decay;
        self.speed = if v.abs() >= held.abs() || !held.is_finite() { v } else { held };
        self.ended = Some(self.now);
        self.speed
    }

    /// Uploads `image` as `id`'s `size` texture. `false` when it is malformed or there is no
    /// context yet.
    fn upload(&mut self, id: PhotoId, size: Size, image: &Arc<Rgba8>) -> bool {
        let Some(ctx) = &self.ctx else { return false };
        let dims = [image.width as usize, image.height as usize];
        if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
            log::warn!("thumbnail of photo {id}: the ladder gave a wrong-sized image");
            return false;
        }
        let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
        let handle = ctx.load_texture(format!("thumb:{id}:{size:?}"), pixels, egui::TextureOptions::LINEAR);
        let bytes = image.data.len();
        let texture = Texture { handle, source: Arc::downgrade(image), bytes, drawn: self.frame };
        if let Some(old) = self.textures.insert((id, size), texture) {
            self.texture_bytes = self.texture_bytes.saturating_sub(old.bytes);
        }
        self.texture_bytes = self.texture_bytes.saturating_add(bytes);
        true
    }

    /// The indices in display order of the cells painted this frame (one run of the order).
    fn visible_range(&self) -> Option<Range<usize>> {
        let mut at = self.cells.iter().filter_map(|c| self.order.index.get(&c.id).copied());
        let first = at.next()?;
        let (lo, hi) = at.fold((first, first), |(lo, hi), i| (lo.min(i), hi.max(i)));
        Some(lo..hi.saturating_add(1))
    }

    /// Drops textures and sketch meshes away from the view; returns how many textures went.
    fn evict(&mut self, visible: Option<Range<usize>>) -> u64 {
        let frame = self.frame;
        let before = self.textures.len();
        // A smaller size goes once its photo drew a covering one.
        let covered: Vec<(PhotoId, u32)> = self.cells.iter().filter(|c| c.drawn == Drawn::Image).map(|c| (c.id, c.need)).collect();
        for (id, need) in covered {
            if let Choice::Covers(size) = ladder::choose(need, self.screen, |s| self.textures.contains_key(&(id, s))) {
                let keep = ladder::pixels(size, self.screen);
                for s in Size::ALL {
                    if ladder::pixels(s, self.screen) < keep {
                        self.remove(id, s);
                    }
                }
            }
        }
        // How far each texture's photo is from the view, in cells.
        let screen = visible.as_ref().map_or(0, Range::len);
        let mut ranked: Vec<((PhotoId, Size), usize)> = Vec::with_capacity(self.textures.len());
        let mut gone = Vec::new();
        for (key, tex) in &self.textures {
            if tex.drawn == frame {
                continue;
            }
            let distance = match &visible {
                Some(range) => self
                    .order
                    .index
                    .get(&key.0)
                    .map(|&i| if i < range.start { range.start - i } else { i.saturating_sub(range.end.saturating_sub(1)) }),
                // Without the display order: by age.
                None => Some(usize::try_from(frame.wrapping_sub(tex.drawn)).unwrap_or(usize::MAX)).filter(|&age| age as u64 <= UNORDERED_FRAMES),
            };
            match distance {
                Some(d) if visible.is_none() || d <= screen => ranked.push((*key, d)),
                _ => gone.push(*key),
            }
        }
        for (id, size) in gone {
            self.remove(id, size);
        }
        // Over the caps: the farthest first, never a cell on screen.
        ranked.sort_by_key(|(_, d)| std::cmp::Reverse(*d));
        for ((id, size), _) in ranked {
            if self.textures.len() <= MAX_TEXTURES && self.texture_bytes <= TEXTURE_BYTES {
                break;
            }
            self.remove(id, size);
        }
        if self.sketches.len() > MAX_SKETCHES {
            self.sketches.retain(|_, m| m.drawn == frame);
        }
        before.saturating_sub(self.textures.len()) as u64
    }

    fn remove(&mut self, id: PhotoId, size: Size) {
        if let Some(t) = self.textures.remove(&(id, size)) {
            self.texture_bytes = self.texture_bytes.saturating_sub(t.bytes);
        }
    }

    /// Why `id` has no thumbnail, if it failed (and was drawn lately).
    pub fn failure(&self, id: PhotoId) -> Option<&str> {
        self.failed.get(&id).map(String::as_str)
    }

    /// Cells on screen still waiting for their covering size, while anything is on its way.
    pub fn pending(&self) -> usize {
        self.pending
    }

    /// The counters of the last finished frame.
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// The counters summed over every frame so far.
    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// Thumbnail textures alive.
    pub fn textures(&self) -> usize {
        self.textures.len()
    }
}

impl Default for Thumbnails {
    fn default() -> Self {
        Thumbnails::new()
    }
}

/// The long side in whole device pixels a cell needs (at least 1).
fn need_of(device_px: f32) -> u32 {
    if device_px.is_finite() && device_px >= 1.0 { device_px.ceil().min(ladder::MAX_SCREEN as f32) as u32 } else { 1 }
}

/// A sketch's colours (`cols` × `rows`, row by row) spread over `rect` as a mesh, interpolated
/// smoothly between the vertices; one flat colour when the grid is a single row or column.
fn sketch_shape(rect: Rect, cols: u32, rows: u32, colours: &[Color32]) -> Shape {
    let (c, r) = (cols as usize, rows as usize);
    if c < 2 || r < 2 || colours.len() != c * r {
        let mean = colours.first().copied().unwrap_or(Color32::GRAY);
        return Shape::rect_filled(rect, 0, mean);
    }
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(c * r);
    mesh.reserve_triangles((c - 1) * (r - 1) * 2);
    for (j, row) in colours.chunks(c).enumerate() {
        let y = rect.top() + rect.height() * j as f32 / (r - 1) as f32;
        for (i, colour) in row.iter().enumerate() {
            let x = rect.left() + rect.width() * i as f32 / (c - 1) as f32;
            mesh.colored_vertex(pos2(x, y), *colour);
        }
    }
    for j in 0..rows - 1 {
        for i in 0..cols - 1 {
            let a = j * cols + i;
            mesh.add_triangle(a, a + 1, a + cols);
            mesh.add_triangle(a + 1, a + cols + 1, a + cols);
        }
    }
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_need_is_whole_device_pixels_and_hostile_sizes_ask_for_one() {
        assert_eq!(need_of(144.0), 144);
        assert_eq!(need_of(144.2), 145);
        for bad in [f32::NAN, f32::NEG_INFINITY, -3.0, 0.0, 0.5] {
            assert_eq!(need_of(bad), 1, "{bad}");
        }
        assert_eq!(need_of(f32::INFINITY), 1);
        assert_eq!(need_of(1e12), ladder::MAX_SCREEN);
    }

    #[test]
    fn a_sketch_mesh_spans_its_rect_with_one_vertex_per_colour() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(144.0, 96.0));
        let colours: Vec<Color32> = (0..8 * 5).map(|i| Color32::from_gray(i as u8)).collect();
        let Shape::Mesh(mesh) = sketch_shape(rect, 8, 5, &colours) else { panic!("not a mesh") };
        assert_eq!(mesh.vertices.len(), 40);
        assert_eq!(mesh.indices.len(), 7 * 4 * 2 * 3);
        assert_eq!(mesh.calc_bounds(), rect);
        assert!(mesh.is_valid());
        assert_eq!(mesh.vertices[39].color, Color32::from_gray(39));
        // A grid one row high, or colours that do not fit the grid: one flat rectangle.
        assert!(matches!(sketch_shape(rect, 8, 1, &colours[..8]), Shape::Rect(_)));
        assert!(matches!(sketch_shape(rect, 8, 5, &colours[..3]), Shape::Rect(_)));
        assert!(matches!(sketch_shape(rect, 0, 0, &[]), Shape::Rect(_)));
    }

    #[test]
    fn a_burst_of_speed_is_held_and_decays() {
        let mut t = Thumbnails::new();
        let at = |t: &mut Thumbnails, now: f64, v: f64| {
            t.now = now;
            t.hold_speed(v)
        };
        assert_eq!(at(&mut t, 0.0, 1000.0), 1000.0);
        // A still frame after a wheel step: held, decayed over one frame.
        let held = at(&mut t, 1.0 / 60.0, 0.0);
        assert!((held - 1000.0 * (-1.0 / 60.0 / SPEED_HOLD_SECS).exp()).abs() < 1e-6, "{held}");
        // A faster frame wins at once, either way.
        assert_eq!(at(&mut t, 2.0 / 60.0, -3000.0), -3000.0);
        // Stopped for a second: next to nothing.
        let mut now = 2.0 / 60.0;
        for _ in 0..60 {
            now += 1.0 / 60.0;
            at(&mut t, now, 0.0);
        }
        assert!(t.speed.abs() < 1.0, "{}", t.speed);
        // Hostile numbers count as standing still.
        for (now, v) in [(now + 0.1, f64::NAN), (f64::NAN, f64::INFINITY), (now + 0.2, f64::NEG_INFINITY)] {
            let held = at(&mut t, now, v);
            assert!(held.is_finite() && held.abs() < 1.0, "{now} {v}: {held}");
        }
    }

    #[test]
    fn only_cells_with_something_better_to_come_are_waiting() {
        assert!(Drawn::Upscaled.waiting() && Drawn::Sketch.waiting() && Drawn::Placeholder.waiting());
        assert!(!Drawn::Image.waiting() && !Drawn::Failed.waiting());
    }
}
