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
//! has (`Drawn::Failed`; the cell adds the "!" and the reason).
//!
//! **Rotations and flips** (`ui-layout.md` §5.3, M6): the ladder images and the sketch are the
//! photo upright, as its file shows it; a photo's recipe turns or mirrors that picture. Paint maps
//! what it has through the recipe's net orientation ([`Thumbnails::orientation`], a
//! [`Dihedral`]): the texture's corners take the turned UVs, the sketch's vertices the turned
//! places. So a rotation shows in the very next frame from the same cached pixels, and never waits
//! for a decode. Callers letterbox (or crop) the photo to its displayed size
//! ([`Thumbnails::displayed`]: a landscape turned a quarter is a portrait). The orientation is
//! parsed once per photo and library revision, only for the photos painted, so the per-frame work
//! stays that of the cells on screen. A file deleted after its thumbnails
//! were made is noticed when the session next scans or verifies it (the watcher, M5): until then
//! its cached thumbnails show.
//!
//! **Edits** (M7, `architecture.md` §6.3): the session draws a photo's ladder images and sketch
//! with its recipe's other operations applied (everything but the rotations and flips, which stay
//! paint's). Each texture remembers the edits it shows (their render key); when the photo's recipe
//! changes, its textures are stale but still drawn, turned by the new orientation, never a blank,
//! and the cell asks for the new images until they arrive and replace them; one of another shape
//! (before a crop) is letterboxed inside the new shape, on the placeholder, never stretched to it.
//! A photo with edits shows the size they leave ([`Thumbnails::displayed`]: a crop's shape) and
//! the edited badge ([`Thumbnails::edited`]). The start of each frame hands the session's recipe changes to the
//! thumbnailer ([`Session::update_thumbnail_recipes`]), so the new images are on their way at once.
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
//! did not draw its covering size, or drew a photo's embedded thumbnail standing in for it while the
//! photo's scan runs ([`Session::thumbnail_stand_in`]; the scan's own downscale replaces it), asks
//! the session for it ([`Session::thumbnail`], the visible
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
//! [`Thumbnails::pending`] says how many cells on screen still wait for their covering size or show
//! a stand-in for it (`ui.settle` waits for it, so a settled frame is the same whichever of a
//! photo's embedded-thumbnail and scan jobs finished first).

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use dalica_core::{PhotoId, Timestamp};
use dalica_edit::{Cropping, Dihedral, Recipe, RenderKey};
use dalica_engine::thumbs::Size;
use dalica_engine::{Refreshed, Session, Thumb};
use dalica_library::Photo;
use dalica_thumbs::Rgba8;
use dalica_thumbs::ladder::{self, Choice, Edits};
use dalica_ui_kit::Tokens;
use egui::epaint::Vertex;
use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, TextureId, pos2};
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
/// Photo shapes kept for one library revision, past which the cache starts again (a long scroll
/// through a large library without any change).
const MAX_SHAPES: usize = 1 << 16;

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

/// The net orientation of a photo's recipe: its rotations and flips added up (identity for none,
/// and for a recipe `edit` cannot read). Parses the operation strings: callers cache it.
pub fn recipe_orientation(edits: &[String]) -> Dihedral {
    if edits.is_empty() {
        return Dihedral::IDENTITY;
    }
    Recipe::parse(edits).map(|r| r.orientation()).unwrap_or(Dihedral::IDENTITY)
}

/// What a photo's recipe does to its thumbnail, read once per library revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhotoShape {
    /// Its rotations and flips added up, turned at paint.
    pub turn: Dihedral,
    /// The render key of its other operations (`None`: its pixels are its file's): what the
    /// session's images of it show.
    pub edits: Option<RenderKey>,
    /// The size its edits make of the upright picture (a crop, a straighten, a frame's growth;
    /// `edit::Recipe::shown_size`), before `turn`, when that is not the file's.
    pub kept: Option<(u32, u32)>,
}

impl PhotoShape {
    /// A photo without a recipe.
    pub const PLAIN: PhotoShape = PhotoShape { turn: Dihedral::IDENTITY, edits: None, kept: None };
}

/// The shape of a photo whose recipe is `edits` and whose upright picture is `width × height`
/// (an unreadable recipe shows the photo as its file has it). Parses the recipe: callers cache it.
pub fn shape_of(edits: &[String], width: u32, height: u32) -> PhotoShape {
    if edits.is_empty() {
        return PhotoShape::PLAIN;
    }
    let Ok(recipe) = Recipe::parse(edits) else { return PhotoShape::PLAIN };
    let turn = recipe.orientation();
    let Some(pixels) = Edits::from_recipe(&recipe, dalica_edit::ops::version) else { return PhotoShape { turn, ..PhotoShape::PLAIN } };
    // The cell counts a crop and a straighten, and grows with a frame (Border, Drop Shadow, …):
    // the shape of the picture its thumbnail shows, before the turn the cell paints it with.
    let kept = Some(pixels.recipe.shown_size(dalica_image::Orientation::default(), width, height, Cropping::Counted))
        .filter(|&k| k != (width, height) && k.0 > 0 && k.1 > 0);
    PhotoShape { turn, edits: Some(pixels.key), kept }
}

/// Where the point at `(x, y)` of the upright picture (each 0–1 across it) lands once turned by `d`
/// (each 0–1 across the turned picture): the continuous form of [`Dihedral::map_pixel`].
pub fn turn_point(d: Dihedral, x: f32, y: f32) -> (f32, f32) {
    let x = if d.mirrored() { 1.0 - x } else { x };
    match d.quarters() {
        1 => (1.0 - y, x),
        2 => (1.0 - x, 1.0 - y),
        3 => (y, 1.0 - x),
        _ => (x, y),
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
    /// It is the photo's embedded thumbnail standing in while the photo's scan runs
    /// ([`Session::thumbnail_stand_in`]): the scan's own downscale replaces it.
    stand_in: bool,
    /// The edits it shows ([`PhotoShape::edits`] when it was uploaded).
    edits: Option<RenderKey>,
    /// The file it was made from ([`file_key`] when it was uploaded): a Save, an Undo Save, a
    /// Revert or an outside change makes it stale, even when the edits key is the same.
    file: Option<FileKey>,
}

/// A photo's file as the library knows it: its size and modification time.
type FileKey = (u64, Timestamp);

fn file_key(photo: &Photo) -> FileKey {
    (photo.size, photo.mtime)
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
    /// The image rect it was painted into, and the orientation it was turned by.
    rect: Rect,
    turn: Dihedral,
    /// It drew an embedded thumbnail standing in for its ladder image: it keeps asking.
    stand_in: bool,
    /// It drew a texture made for the photo's earlier edits: it keeps asking.
    stale: bool,
}

impl Cell {
    /// Something better is still to come: it waits for its covering size, or shows an embedded
    /// thumbnail standing in for it or a texture of the photo's earlier edits.
    fn better_to_come(&self) -> bool {
        self.drawn.waiting() || self.stand_in || self.stale
    }
}

/// One photo painted in the last frame (tests, inspection): where, turned how, and what.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Painted {
    pub photo: PhotoId,
    /// The image rect, in points (the letterboxed image of a cell; a tray mini's crop); for a stale
    /// texture of another shape, where it was drawn inside that rect.
    pub rect: Rect,
    pub turn: Dihedral,
    pub drawn: Drawn,
    /// It drew a texture of the photo's earlier edits while the new one renders.
    pub stale: bool,
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
    /// The shapes of the photos painted lately, for the library revision they were read in.
    shapes: HashMap<PhotoId, PhotoShape>,
    shapes_revision: Option<u64>,
    /// The last frame whose cells [`Thumbnails::end_frame`] asked for.
    asked_in: Option<u64>,
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
            shapes: HashMap::new(),
            shapes_revision: None,
            asked_in: None,
        }
    }

    /// What `photo`'s recipe does to its thumbnail ([`PhotoShape`]), read once per library revision: the
    /// cache is emptied when the revision moves, and only photos asked about are parsed, so the
    /// cost is that of the cells on screen.
    pub fn shape(&mut self, photo: PhotoRef<'_>) -> PhotoShape {
        let library = photo.session.library();
        let revision = library.revision();
        if self.shapes_revision != Some(revision) {
            self.shapes.clear();
            self.shapes_revision = Some(revision);
        }
        let id = photo.photo.id;
        let edits = &library.info(id).edits;
        if edits.is_empty() {
            return PhotoShape::PLAIN;
        }
        if let Some(s) = self.shapes.get(&id) {
            return *s;
        }
        if self.shapes.len() >= MAX_SHAPES {
            self.shapes.clear();
        }
        let s = shape_of(edits, photo.photo.width, photo.photo.height);
        self.shapes.insert(id, s);
        s
    }

    /// The net orientation of `photo`'s recipe (what its rotations and flips add up to).
    pub fn orientation(&mut self, photo: PhotoRef<'_>) -> Dihedral {
        self.shape(photo).turn
    }

    /// Whether `photo` has edits not yet saved into its file, rotations and flips included: the
    /// edited badge. The same test as the folder header's "n edited" count
    /// (`Library::has_unsaved_edits`), so the two always agree and the badge goes after a Save.
    pub fn edited(&self, photo: PhotoRef<'_>) -> bool {
        photo.session.library().has_unsaved_edits(photo.photo.id)
    }

    /// The size `photo` shows at: what its edits make of its file's displayed size (a crop's
    /// shape, a frame's growth), turned by its recipe (a landscape turned a quarter is a portrait); 0 × 0 when the
    /// file's size is unknown. What callers letterbox or crop it to.
    pub fn displayed(&mut self, photo: PhotoRef<'_>) -> (u32, u32) {
        let shape = self.shape(photo);
        let (w, h) = shape.kept.unwrap_or((photo.photo.width, photo.photo.height));
        shape.turn.display_size(w, h)
    }

    /// Starts a frame: notes the theme and, when the display's size is known, tells the session
    /// (the screen-size ladder entry). Returns the textures alive.
    pub fn begin_frame(&mut self, ctx: &egui::Context, session: &mut Session, t: &Tokens) -> usize {
        self.placeholder = t.placeholder;
        // Recipes changed since the last frame: their new images are queued at once.
        session.update_thumbnail_recipes();
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
        // A frame that painted thumbnails without the light table (the edit view's Photo Tray)
        // never reached `end_frame`: its cells still ask for and upload their better images here,
        // so a recipe changed in the photo's own history shows there too. Only while the library
        // is at the revision those cells were painted in: the uploads are tagged with the edits
        // they were painted with; after a change the next frame paints and asks again.
        let painted_now = self.shapes_revision.is_none_or(|r| r == session.library().revision());
        if !self.cells.is_empty() && self.asked_in != Some(self.frame) && painted_now {
            let (uploads, _, deferred) = self.ask_and_upload(session, true);
            let waiting = session.thumbnails_pending() > 0 || deferred > 0 || uploads > 0;
            self.pending = if waiting { self.cells.iter().filter(|c| c.better_to_come()).count() } else { 0 };
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

    /// Paints `photo` into `rect` (its letterboxed image at its displayed size, in points) for an
    /// image `device_px` device pixels on its long side: the best uploaded ladder size, else its
    /// sketch, else the placeholder, turned by its recipe's orientation. A texture of the photo's
    /// earlier edits, or of its earlier file (before a Save, an Undo Save, a Revert), is drawn only
    /// while none of its current ones is uploaded, at its own shape letterboxed inside `rect`
    /// ([`fit_within`]). Never touches the disk and never decodes an image.
    pub fn paint(&mut self, painter: &Painter, photo: PhotoRef<'_>, rect: Rect, device_px: f32) -> Drawn {
        let id = photo.photo.id;
        let shape = self.shape(photo);
        let turn = shape.turn;
        let need = need_of(device_px);
        let frame = self.frame;
        let file = Some(file_key(photo.photo));
        let current = |t: &HashMap<(PhotoId, Size), Texture>, s: Size| t.get(&(id, s)).is_some_and(|t| t.edits == shape.edits && t.file == file);
        let mut choice = ladder::choose(need, self.screen, |s| current(&self.textures, s));
        let mut stale = false;
        if choice == Choice::Nothing {
            choice = ladder::choose(need, self.screen, |s| self.textures.contains_key(&(id, s)));
            stale = choice != Choice::Nothing;
        }
        let texture = match choice {
            Choice::Covers(size) | Choice::Smaller(size) => self.textures.get_mut(&(id, size)),
            Choice::Nothing => None,
        };
        let mut stand_in = false;
        let mut drawn_at = rect;
        let drawn = if let Some(why) = photo.session.thumbnail_failure(id) {
            stale = false;
            // A missing or unreadable file: the placeholder under the cell's "!", whatever was
            // drawn of it before.
            painter.rect_filled(rect, 0, self.placeholder);
            if self.failed.get(&id).is_none_or(|w| w != why) {
                self.failed.insert(id, why.to_string());
            }
            Drawn::Failed
        } else if let Some(tex) = texture {
            tex.drawn = frame;
            stand_in = tex.stand_in;
            if stale {
                // The earlier edits' picture keeps its own shape (a crop's new one may differ): it
                // is letterboxed inside the new shape, on the placeholder, never stretched.
                let [w, h] = tex.handle.size();
                let (w, h) = turn.display_size(u32::try_from(w).unwrap_or(u32::MAX), u32::try_from(h).unwrap_or(u32::MAX));
                let own = fit_within(rect, w, h);
                if own != rect {
                    painter.rect_filled(rect, 0, self.placeholder);
                    drawn_at = own;
                }
            }
            painter.add(textured(drawn_at, tex.handle.id(), turn));
            if matches!(choice, Choice::Covers(_)) { Drawn::Image } else { Drawn::Upscaled }
        } else {
            stale = false;
            match photo.session.library().sketch(id) {
                Some(sketch) if self.paint_sketch(painter, id, sketch, rect, turn) => Drawn::Sketch,
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
        self.cells.push(Cell { id, need, drawn, rect: drawn_at, turn, stand_in, stale });
        drawn
    }

    /// Draws `sketch` over `rect` as a vertex-coloured mesh turned by `turn`; `false` when it does
    /// not decode.
    fn paint_sketch(&mut self, painter: &Painter, id: PhotoId, sketch: &dalica_library::Sketch, rect: Rect, turn: Dihedral) -> bool {
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
        painter.add(sketch_shape(rect, m.cols, m.rows, &m.colours, turn));
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
        let (uploads, bytes, deferred) = self.ask_and_upload(session, slow);
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
        self.pending = if waiting { self.cells.iter().filter(|c| c.better_to_come()).count() } else { 0 };
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

    /// Asks the session for the better images of this frame's cells that did not draw their
    /// covering size (when `slow`: the view moves slowly enough to look at), and uploads what has
    /// arrived within the per-frame caps. Returns the uploads, their bytes and those left for a
    /// later frame.
    fn ask_and_upload(&mut self, session: &mut Session, slow: bool) -> (usize, usize, u64) {
        self.asked_in = Some(self.frame);
        let mut arrived: Vec<(PhotoId, Size, Arc<Rgba8>, bool, bool)> = Vec::new();
        if slow {
            for cell in self.cells.iter().filter(|c| c.better_to_come()) {
                if let Thumb::Image { size, image, .. } = session.thumbnail(cell.id, cell.need) {
                    let current = self.textures.get(&(cell.id, size)).and_then(|t| t.source.upgrade()).is_some_and(|s| Arc::ptr_eq(&s, &image));
                    if !current {
                        // Cells with nothing better than a sketch, or only a stale texture, first.
                        let first = matches!(cell.drawn, Drawn::Sketch | Drawn::Placeholder) || cell.stale;
                        arrived.push((cell.id, size, image, first, session.thumbnail_stand_in(cell.id)));
                    }
                }
            }
        }
        arrived.sort_by_key(|(_, _, _, first, _)| !first);
        let (mut uploads, mut bytes, mut deferred) = (0usize, 0usize, 0u64);
        for (id, size, image, _, stand_in) in arrived {
            let len = image.data.len();
            if uploads >= UPLOADS_PER_FRAME || (uploads > 0 && bytes.saturating_add(len) > UPLOAD_BYTES_PER_FRAME) {
                deferred += 1;
                continue;
            }
            // The session answers for the photo's recipe as the library has it now: the edits
            // this frame painted it with.
            let edits = self.shapes.get(&id).and_then(|s| s.edits);
            let file = session.library().photo(id).map(file_key);
            if self.upload(id, size, &image, stand_in, edits, file) {
                uploads += 1;
                bytes = bytes.saturating_add(len);
            }
        }
        (uploads, bytes, deferred)
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
    fn upload(&mut self, id: PhotoId, size: Size, image: &Arc<Rgba8>, stand_in: bool, edits: Option<RenderKey>, file: Option<FileKey>) -> bool {
        let Some(ctx) = &self.ctx else { return false };
        let dims = [image.width as usize, image.height as usize];
        if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
            log::warn!("thumbnail of photo {id}: the ladder gave a wrong-sized image");
            return false;
        }
        let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
        let handle = ctx.load_texture(format!("thumb:{id}:{size:?}"), pixels, egui::TextureOptions::LINEAR);
        let bytes = image.data.len();
        let texture = Texture { handle, source: Arc::downgrade(image), bytes, drawn: self.frame, stand_in, edits, file };
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
        // A smaller size goes once its photo drew a covering one of its current edits.
        let covered: Vec<(PhotoId, u32)> = self.cells.iter().filter(|c| c.drawn == Drawn::Image && !c.stale).map(|c| (c.id, c.need)).collect();
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

    /// Cells on screen still waiting for their covering size (or showing an embedded thumbnail
    /// standing in for it), while anything is on its way.
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

    /// What the last frame painted, in paint order (the light table's cells, then the tray's).
    pub fn painted(&self) -> impl Iterator<Item = Painted> + '_ {
        self.cells.iter().map(|c| Painted { photo: c.id, rect: c.rect, turn: c.turn, drawn: c.drawn, stale: c.stale })
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

/// How far a texture's shape may stray from its rect's before it is letterboxed rather than drawn
/// over the whole rect (a ladder image's rounding stays well inside it; a crop's new shape does not).
const SHAPE_SLACK: f32 = 0.02;

/// `rect` narrowed to the shape of a `width × height` picture, centred in it (letterboxed); `rect`
/// itself when the shapes agree within [`SHAPE_SLACK`] or either is empty.
pub fn fit_within(rect: Rect, width: u32, height: u32) -> Rect {
    if width == 0 || height == 0 || !rect.is_positive() || !rect.is_finite() {
        return rect;
    }
    let (w, h) = (width as f32, height as f32);
    let ratio = (w / h) / (rect.width() / rect.height());
    if !ratio.is_finite() || (ratio - 1.0).abs() <= SHAPE_SLACK {
        return rect;
    }
    let scale = (rect.width() / w).min(rect.height() / h);
    Rect::from_center_size(rect.center(), egui::vec2(w * scale, h * scale))
}

/// The point of `rect` at `(x, y)` (each 0–1 across it).
fn at(rect: Rect, (x, y): (f32, f32)) -> Pos2 {
    pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y)
}

/// A texture of the upright picture drawn over `rect` turned by `turn`: one quad whose corners take
/// the UVs of the upright picture's points that land there.
fn textured(rect: Rect, texture: TextureId, turn: Dihedral) -> Shape {
    let back = turn.inverse();
    let mut mesh = Mesh::with_texture(texture);
    for corner in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let (u, v) = turn_point(back, corner.0, corner.1);
        mesh.vertices.push(Vertex { pos: at(rect, corner), uv: pos2(u, v), color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

/// A sketch's colours (`cols` × `rows`, row by row, of the upright picture) spread over `rect` as a
/// mesh turned by `turn`, interpolated smoothly between the vertices; one flat colour when the grid
/// is a single row or column.
fn sketch_shape(rect: Rect, cols: u32, rows: u32, colours: &[Color32], turn: Dihedral) -> Shape {
    let (c, r) = (cols as usize, rows as usize);
    if c < 2 || r < 2 || colours.len() != c * r {
        let mean = colours.first().copied().unwrap_or(Color32::GRAY);
        return Shape::rect_filled(rect, 0, mean);
    }
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(c * r);
    mesh.reserve_triangles((c - 1) * (r - 1) * 2);
    for (j, row) in colours.chunks(c).enumerate() {
        let y = j as f32 / (r - 1) as f32;
        for (i, colour) in row.iter().enumerate() {
            let x = i as f32 / (c - 1) as f32;
            mesh.colored_vertex(at(rect, turn_point(turn, x, y)), *colour);
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
        let Shape::Mesh(mesh) = sketch_shape(rect, 8, 5, &colours, Dihedral::IDENTITY) else { panic!("not a mesh") };
        assert_eq!(mesh.vertices.len(), 40);
        assert_eq!(mesh.indices.len(), 7 * 4 * 2 * 3);
        assert_eq!(mesh.calc_bounds(), rect);
        assert!(mesh.is_valid());
        assert_eq!(mesh.vertices[39].color, Color32::from_gray(39));
        // A grid one row high, or colours that do not fit the grid: one flat rectangle.
        assert!(matches!(sketch_shape(rect, 8, 1, &colours[..8], Dihedral::IDENTITY), Shape::Rect(_)));
        assert!(matches!(sketch_shape(rect, 8, 5, &colours[..3], Dihedral::IDENTITY), Shape::Rect(_)));
        assert!(matches!(sketch_shape(rect, 0, 0, &[], Dihedral::IDENTITY), Shape::Rect(_)));
    }

    #[test]
    fn turned_points_follow_the_pixel_map_of_every_transform() {
        // The corners of a 5 × 3 picture land where `map_pixel` puts its corner pixels.
        let (w, h) = (5u32, 3u32);
        for d in Dihedral::ALL {
            let (tw, th) = d.display_size(w, h);
            for (x, y) in [(0, 0), (w - 1, 0), (w - 1, h - 1), (0, h - 1)] {
                let (px, py) = d.map_pixel(x, y, w, h);
                let (fx, fy) = turn_point(d, x as f32 / (w - 1) as f32, y as f32 / (h - 1) as f32);
                assert_eq!((fx, fy), (px as f32 / (tw - 1) as f32, py as f32 / (th - 1) as f32), "{} at {x},{y}", d.name());
            }
            // There and back.
            let (u, v) = turn_point(d, 0.25, 0.75);
            assert_eq!(turn_point(d.inverse(), u, v), (0.25, 0.75), "{}", d.name());
        }
    }

    #[test]
    fn a_turned_texture_maps_its_corners_and_a_turned_sketch_its_vertices() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(96.0, 144.0));
        let id = TextureId::Managed(7);
        // Upright: the top-left corner shows the texture's top-left.
        let Shape::Mesh(m) = textured(rect, id, Dihedral::IDENTITY) else { panic!("not a mesh") };
        assert_eq!((m.vertices[0].pos, m.vertices[0].uv), (rect.left_top(), pos2(0.0, 0.0)));
        assert_eq!((m.texture_id, m.vertices.len(), m.indices.len()), (id, 4, 6));
        // A quarter turn clockwise: the top-left corner shows what was the bottom-left, the
        // top-right what was the top-left.
        let Shape::Mesh(m) = textured(rect, id, Dihedral::ROTATE_90) else { panic!("not a mesh") };
        assert_eq!(m.vertices[0].uv, pos2(0.0, 1.0));
        assert_eq!(m.vertices[1].uv, pos2(0.0, 0.0));
        // Mirrored left-right: the top-left shows the top-right.
        let Shape::Mesh(m) = textured(rect, id, Dihedral::FLIP_H) else { panic!("not a mesh") };
        assert_eq!(m.vertices[0].uv, pos2(1.0, 0.0));
        // The sketch's first colour (its top-left) lands top-right after a quarter turn.
        let colours: Vec<Color32> = (0..8 * 5).map(|i| Color32::from_gray(i as u8)).collect();
        let Shape::Mesh(mesh) = sketch_shape(rect, 8, 5, &colours, Dihedral::ROTATE_90) else { panic!("not a mesh") };
        assert_eq!((mesh.vertices[0].pos, mesh.vertices[0].color), (rect.right_top(), Color32::from_gray(0)));
        assert_eq!(mesh.calc_bounds(), rect);
        assert!(mesh.is_valid());
    }

    #[test]
    fn recipes_turn_by_their_rotations_and_flips_and_unreadable_ones_not_at_all() {
        let ops = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(recipe_orientation(&[]), Dihedral::IDENTITY);
        assert_eq!(recipe_orientation(&ops(&["rotate(90)"])), Dihedral::ROTATE_90);
        assert_eq!(recipe_orientation(&ops(&["rotate(90)", "flip_h()"])), Dihedral::ROTATE_90.then(Dihedral::FLIP_H));
        assert_eq!(recipe_orientation(&ops(&["autofix()", "legacy:x", "rotate(45)"])), Dihedral::IDENTITY);
        let too_many = vec!["rotate(90)".to_string(); dalica_edit::MAX_OPERATIONS + 1];
        assert_eq!(recipe_orientation(&too_many), Dihedral::IDENTITY);
    }

    #[test]
    fn a_picture_of_another_shape_is_letterboxed_inside_the_rect_and_a_near_one_fills_it() {
        // A 2:1 cell rect (a crop's new shape) and the 3:2 picture before it: full height, centred.
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(144.0, 72.0));
        let fit = fit_within(rect, 600, 400);
        assert_eq!((fit.size(), fit.center()), (vec2(108.0, 72.0), rect.center()));
        // A portrait inside a landscape rect, and a landscape inside a portrait one.
        assert_eq!(fit_within(rect, 300, 400).size(), vec2(54.0, 72.0));
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(72.0, 144.0));
        assert_eq!(fit_within(tall, 600, 400).size(), vec2(72.0, 48.0));
        // A ladder image's rounding (256 × 171 of a 3:2 photo) is the same shape: the whole rect.
        let cell = Rect::from_min_size(pos2(0.0, 0.0), vec2(144.0, 96.0));
        assert_eq!(fit_within(cell, 256, 171), cell);
        assert_eq!(fit_within(cell, 64, 43), cell);
        // Hostile sizes and rects leave the rect as it is.
        for (w, h) in [(0, 400), (600, 0), (0, 0)] {
            assert_eq!(fit_within(cell, w, h), cell);
        }
        let flat = Rect::from_min_size(pos2(0.0, 0.0), vec2(144.0, 0.0));
        assert_eq!(fit_within(flat, 600, 400), flat);
        let wild = Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::INFINITY, 10.0));
        assert_eq!(fit_within(wild, 600, 400), wild);
        let huge = fit_within(cell, u32::MAX, 1);
        assert!(huge.is_finite() && cell.contains_rect(huge), "{huge:?}");
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
