//! The effect tabs' tile previews (`editing.md` §8.1, `roadmap.md` M12): each tile is the photo
//! with its recipe so far plus one effect at its defaults, made in the background and never
//! written to disk.
//!
//! - [`Session::request_effect_tiles`] asks for a tab's tiles of a photo at a size in device
//!   pixels; [`Session::take_effect_tiles`] (each frame) applies what the worker finished and hands
//!   over the tiles made since, **in grid order** (one worker, the tiles submitted in order).
//! - **The small decode.** The photo file is read and decoded on the worker once per photo, large
//!   enough that the tile's box is covered twice over whatever the photo's shape (its longer side
//!   at most [`MAX_DECODE`]), and kept in memory for the last [`DECODES`] photos: a new recipe or a
//!   new tab makes no new decode.
//! - **A tile** renders the recipe so far with the effect's default entry appended
//!   (`OpInfo::default_entry`) from the small decode, at the scale that covers the box, then cut to
//!   the box's shape around the centre and brought to exactly the size asked for. A picture that
//!   includes a frame (`Geometry::frames`: the effect itself changes the picture's size, as Border,
//!   Drop Shadow, Museum Matte and Widescreen Film do, or the recipe so far holds one) renders
//!   instead whole, at the scale that fits it in the box, centred on [`PLACEHOLDER`], so every
//!   side of the frame shows (M12.29, M13.44). A frame effect's own tile draws its bands at least
//!   [`MIN_BAND`] tile pixels each (Border's and Museum Matte's two bands, Drop Shadow's reach:
//!   its distance and blur together), wider than its defaults where those would be thinner, so
//!   the frame reads at tile size (`readable`); applying the effect still uses its defaults. The
//!   whole recipe renders in each tile so that every stored-frame point and angle of the effect
//!   lands where the edit view puts it (a crop or a rotation before it included). An effect whose
//!   renderer is still a stub (`OpInfo::renders` false) shows the photo as it is.
//! - **Priority.** Tile jobs run at [`Priority::EFFECT_TILES`] on their own worker and only while
//!   the edit view's preview has nothing on its way (`Session::preview_busy` is 0): while it has,
//!   nothing is submitted and the jobs not yet finished are taken back and asked for again later.
//! - **Cancelled** when the photo changes (another photo asked for, or the edit view open on
//!   another photo), **remade** when the photo's recipe or file changes; a result of an older
//!   request is dropped.
//!
//! The owner thread only submits jobs and takes their results: it never reads, decodes or renders.
//! The state lives in the jobs registry (`Jobs::effect_tiles`).

use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dalica_core::PhotoId;
use dalica_edit::ops::Operation;
use dalica_edit::ops::canvas::{self, Border};
use dalica_edit::ops::mats::{self, DropShadow, MuseumMatte};
use dalica_edit::{Geometry, Recipe, Target, Typed, catalog};
use dalica_image::{DecodeOptions, Limits, Orientation, Rgba8};
use dalica_thumbs::ladder::Source;
use dalica_thumbs::{Pool, Priority};

use crate::error::CmdError;
use crate::session::Session;

/// The tile workers: one, so the tiles finish in grid order.
pub const WORKERS: usize = 1;
/// The largest tile, per side, in device pixels.
pub const MAX_TILE: u32 = 1024;
/// The longest side of a small decode.
pub const MAX_DECODE: u32 = 4096;
/// How many times over a small decode covers the tile's box (at least).
pub const COVER: u32 = 2;
/// Photos whose small decode stays in memory.
pub const DECODES: usize = 2;
/// Tiles kept until taken.
pub const MAX_READY: usize = 64;
/// The largest photo file a tile reads.
pub const MAX_FILE: u64 = dalica_thumbs::MAX_FILE;
/// Around a fitted tile: the effect tiles' placeholder grey (`gray 38`, `editing.md` §8.1).
pub const PLACEHOLDER: [u8; 4] = [38, 38, 38, 255];
/// The narrowest band of a frame effect's own tile, in tile pixels (`editing.md` §8.1): 2, and
/// half a pixel to spare for the whole-pixel rounding of the bands and of the fitted scale.
pub const MIN_BAND: f64 = 2.5;
/// Rounds of `readable`'s fixed point (wider bands shrink the fitted photo, which asks for wider
/// bands): each round's change is a small fraction of the last one's.
const BAND_ROUNDS: usize = 6;

/// One tile made: the photo with its recipe so far and the effect at its defaults.
#[derive(Clone, Debug)]
pub struct EffectTile {
    pub photo: PhotoId,
    pub tab: u8,
    /// Its place in the tab's grid, 0-based, row by row.
    pub place: u8,
    /// The effect as the recipe names it (`sepia`).
    pub name: &'static str,
    /// The request it answers (each request, and each remake, takes a new one).
    pub generation: u64,
    /// The photo's recipe the tile was made on (the effect not included).
    pub recipe: Vec<String>,
    /// Exactly the size asked for.
    pub image: Arc<Rgba8>,
}

/// Counters of the tiles' work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectTileStats {
    /// Requests that asked for new tiles (a new photo, tab, size or recipe).
    pub requests: u64,
    /// Photo files decoded small.
    pub decodes: u64,
    /// Tiles rendered.
    pub renders: u64,
    /// Tiles asked for and never handed over because the photo changed or the tiles were closed.
    pub cancelled: u64,
    /// Results of older requests dropped.
    pub stale: u64,
    /// Jobs taken back because the edit view's preview had work on its way.
    pub yielded: u64,
    pub failures: u64,
}

/// What a job does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Work {
    Decode,
    Tile(u8),
}

/// What a job answers.
enum Done {
    Decoded { file: Source, cover: u32, result: Result<Arc<Rgba8>, String> },
    Tile(Result<Rgba8, String>),
    Panicked(String),
}

/// One tile to make.
#[derive(Clone, Debug)]
struct Job {
    place: u8,
    name: &'static str,
    /// The effect at its defaults, as the recipe writes it.
    entry: String,
}

/// What the last request asked for.
struct Want {
    photo: PhotoId,
    file: Source,
    tab: u8,
    size: (u32, u32),
    recipe: Vec<String>,
    generation: u64,
    /// Tiles not submitted yet, in grid order.
    queue: VecDeque<Job>,
}

/// The tiles' state (`Jobs::effect_tiles`).
#[derive(Default)]
pub(crate) struct EffectTiles {
    /// Started on the first request.
    pool: Option<Pool<u64, Done>>,
    want: Option<Want>,
    /// Small decodes, most recently used first: the file, the longer tile side they cover
    /// [`COVER`] times, the pixels (upright).
    decodes: VecDeque<(Source, u32, Arc<Rgba8>)>,
    /// Jobs submitted and not yet received, by key (unique per submission): their generation,
    /// what they do and, for a tile, the job to ask for again when taken back.
    outstanding: HashMap<u64, (u64, Work, Option<Job>)>,
    next_key: u64,
    generation: u64,
    ready: VecDeque<EffectTile>,
    stats: EffectTileStats,
}

impl EffectTiles {
    fn pool(&mut self) -> &Pool<u64, Done> {
        self.pool.get_or_insert_with(|| Pool::new(WORKERS))
    }

    fn submit(&mut self, generation: u64, what: Work, job: Option<Job>, work: impl FnOnce(&AtomicBool) -> Done + Send + 'static) {
        let key = self.next_key;
        self.next_key = self.next_key.wrapping_add(1);
        let wrapped = move |cancel: &AtomicBool| {
            catch_unwind(AssertUnwindSafe(|| work(cancel))).unwrap_or_else(|panic| {
                let message =
                    panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                Done::Panicked(message)
            })
        };
        self.pool().submit(key, Priority::EFFECT_TILES, wrapped);
        self.outstanding.insert(key, (generation, what, job));
    }

    /// Cancels every job on its way and returns the tiles among them.
    fn cancel_all(&mut self) -> Vec<Job> {
        let mut tiles = Vec::new();
        let mut keys: Vec<u64> = self.outstanding.keys().copied().collect();
        keys.sort_unstable();
        for key in keys {
            if let Some((_, _, job)) = self.outstanding.remove(&key) {
                if let Some(pool) = &self.pool {
                    pool.cancel(&key);
                }
                tiles.extend(job);
            }
        }
        tiles
    }

    /// Drops the request and everything it asked for; counts its tiles as cancelled.
    fn drop_want(&mut self) {
        let on_the_way = self.cancel_all().len();
        if let Some(want) = self.want.take() {
            let queued = want.queue.len();
            self.stats.cancelled = self.stats.cancelled.saturating_add(u64::try_from(queued + on_the_way).unwrap_or(u64::MAX));
        }
        self.ready.clear();
    }

    /// The small decode of `file` covering `side` [`COVER`] times, if one is in memory.
    fn decode_of(&mut self, file: &Source, side: u32) -> Option<Arc<Rgba8>> {
        let at = self.decodes.iter().position(|(f, cover, _)| f == file && *cover >= side)?;
        let entry = self.decodes.remove(at)?;
        let image = Arc::clone(&entry.2);
        self.decodes.push_front(entry);
        Some(image)
    }

    fn keep_decode(&mut self, file: Source, cover: u32, image: Arc<Rgba8>) {
        self.decodes.retain(|(f, _, _)| *f != file);
        self.decodes.push_front((file, cover, image));
        self.decodes.truncate(DECODES);
    }
}

/// The file of photo `id` as the decode cache keys it.
fn file_of(s: &Session, id: PhotoId) -> Option<Source> {
    let photo = s.library.photo(id)?;
    let folder = s.library.folder(photo.folder)?;
    Some(Source { path: folder.path.join(&photo.name), file_size: photo.size, mtime_ms: photo.mtime.as_millis() })
}

/// The tiles of tab `tab`, in grid order.
fn jobs_of(tab: u8) -> VecDeque<Job> {
    catalog::effects(tab).filter_map(|(op, e)| op.default_entry().map(|entry| Job { place: e.place, name: op.name, entry })).collect()
}

/// The longer side a decode needs so that a `size` box is covered [`COVER`] times by a photo of
/// `width × height` stored pixels, whichever way it is turned.
fn decode_edge(width: u32, height: u32, size: (u32, u32)) -> u32 {
    let (long, short) = (u64::from(width.max(height)), u64::from(width.min(height).max(1)));
    let want = u64::from(size.0.max(size.1)).saturating_mul(u64::from(COVER));
    let edge = want.saturating_mul(long).div_ceil(short);
    u32::try_from(edge.clamp(1, u64::from(MAX_DECODE))).unwrap_or(MAX_DECODE)
}

/// Reads and decodes `file` small, upright, covering a `side × side` box [`COVER`] times.
fn decode(file: &Source, side: u32, options: &DecodeOptions, limits: &Limits) -> Result<Rgba8, String> {
    let bytes = match dalica_fs::read(&file.path, MAX_FILE) {
        Ok(Some(r)) => r.bytes,
        Ok(None) => return Err("the file is missing".into()),
        Err(e) => return Err(e.to_string()),
    };
    let header = dalica_image::read_header(&bytes, limits).map_err(|e| e.reason())?;
    let edge = decode_edge(header.width, header.height, (side, side));
    dalica_image::decode_thumbnail_with(&bytes, edge, options, limits).map_err(|e| e.reason())
}

/// `image` cut to the shape of a `size` box around its centre and brought to exactly that size.
fn cover(image: &Rgba8, size: (u32, u32)) -> Result<Rgba8, String> {
    let (w, h) = (u64::from(image.width), u64::from(image.height));
    let (bw, bh) = (u64::from(size.0.max(1)), u64::from(size.1.max(1)));
    if w == 0 || h == 0 {
        return Err("an empty picture".into());
    }
    // The largest window of the box's shape inside the picture.
    let (cw, ch) = if w * bh > h * bw { ((h * bw).div_ceil(bh).min(w), h) } else { (w, (w * bh).div_ceil(bw).min(h)) };
    let (x0, y0) = ((w - cw) / 2, (h - ch) / 2);
    let mut cut = Vec::new();
    let bytes = usize::try_from(cw * ch * 4).map_err(|_| "a tile too large".to_string())?;
    cut.try_reserve_exact(bytes).map_err(|_| "no memory for a tile".to_string())?;
    for y in y0..y0 + ch {
        let start = usize::try_from(y * w * 4 + x0 * 4).map_err(|_| "a picture too large".to_string())?;
        let row = image.data.get(start..start + usize::try_from(cw * 4).unwrap_or(0)).ok_or_else(|| "a picture shorter than its size".to_string())?;
        cut.extend_from_slice(row);
    }
    let window = Rgba8 { width: u32::try_from(cw).unwrap_or(1), height: u32::try_from(ch).unwrap_or(1), data: cut };
    if (window.width, window.height) == size {
        return Ok(window);
    }
    dalica_image::resize::resize(&window, size.0, size.1).map_err(|e| e.reason())
}

/// `image`, no larger than a `size` box, centred on [`PLACEHOLDER`] filling it.
fn letterbox(image: &Rgba8, size: (u32, u32)) -> Result<Rgba8, String> {
    let (bw, bh) = (size.0.max(1), size.1.max(1));
    let (w, h) = (image.width.min(bw), image.height.min(bh));
    let bytes = usize::try_from(u64::from(bw) * u64::from(bh) * 4).map_err(|_| "a tile too large".to_string())?;
    let mut data = Vec::new();
    data.try_reserve_exact(bytes).map_err(|_| "no memory for a tile".to_string())?;
    data.extend(std::iter::repeat_n(PLACEHOLDER, bytes / 4).flatten());
    let (x0, y0) = ((bw - w) / 2, (bh - h) / 2);
    let (row, src_row) = (usize::try_from(bw).unwrap_or(0) * 4, usize::try_from(image.width).unwrap_or(0) * 4);
    let (x0, y0, w) = (usize::try_from(x0).unwrap_or(0), usize::try_from(y0).unwrap_or(0), usize::try_from(w).unwrap_or(0) * 4);
    for y in 0..usize::try_from(h).unwrap_or(0) {
        let from = image.data.get(y * src_row..y * src_row + w).ok_or_else(|| "a picture shorter than its size".to_string())?;
        let start = (y0 + y) * row + x0 * 4;
        let to = data.get_mut(start..start + w).ok_or_else(|| "a tile shorter than its size".to_string())?;
        to.copy_from_slice(from);
    }
    Ok(Rgba8 { width: bw, height: bh, data })
}

/// The geometry of `recipe` on `decode`.
fn geometry_of(recipe: &Recipe, decode: &Rgba8) -> Geometry {
    Geometry::new(recipe, Orientation::default(), decode.width, decode.height)
}

/// `texts` parsed as one recipe.
fn parse<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Recipe, String> {
    Recipe::parse(texts).map_err(|e| e.to_string())
}

/// The frame effect `entry` (at its defaults) as its own tile draws it: each band at least
/// [`MIN_BAND`] pixels once the picture of `recipe` (the recipe so far) with the frame is fitted
/// in a `size` box. Border's and Museum Matte's bands are widened one by one (a band of 0 stays
/// none); Drop Shadow's distance and blur together, in proportion, so the shadow keeps its
/// direction and softness. Within each parameter's range; any other entry comes back as it is.
fn readable(decode: &Rgba8, recipe: &[String], entry: &str, size: (u32, u32)) -> Result<String, String> {
    let typed = parse([entry])?.pipeline().next().and_then(|op| op.typed()).and_then(Result::ok);
    let (mut lengths, max) = match &typed {
        Some(Typed::Border(b)) => (vec![b.outer, b.inner], canvas::THICKNESS.1),
        Some(Typed::MuseumMatte(m)) => (vec![m.outer, m.inner], mats::THICKNESS.1),
        Some(Typed::DropShadow(d)) => (vec![d.distance + d.blur], mats::DISTANCE.1 + mats::BLUR.1),
        _ => return Ok(entry.to_string()),
    };
    let defaults = lengths.clone();
    let write = |lengths: &[f64]| -> String {
        let at = |i: usize| lengths.get(i).copied().unwrap_or(0.0);
        match &typed {
            Some(Typed::Border(b)) => format!("{}({})", Border::NAME, Border { outer: at(0), inner: at(1), ..*b }),
            Some(Typed::MuseumMatte(m)) => format!("{}({})", MuseumMatte::NAME, MuseumMatte { outer: at(0), inner: at(1), ..*m }),
            Some(Typed::DropShadow(d)) => {
                let reach = d.distance + d.blur;
                let k = if reach > 0.0 { at(0) / reach } else { 1.0 };
                let d = DropShadow { distance: (d.distance * k).min(mats::DISTANCE.1), blur: (d.blur * k).min(mats::BLUR.1), ..*d };
                format!("{}({})", DropShadow::NAME, d)
            }
            _ => entry.to_string(),
        }
    };
    // The shorter side of the picture the frame wraps, in decode pixels.
    let (iw, ih) = geometry_of(&parse(recipe.iter().map(String::as_str))?, decode).size();
    let short = iw.min(ih);
    let (bw, bh) = (f64::from(size.0), f64::from(size.1));
    for _ in 0..BAND_ROUNDS {
        let text = write(&lengths);
        let framed = geometry_of(&parse(recipe.iter().map(String::as_str).chain(std::iter::once(text.as_str())))?, decode);
        let (w, h) = framed.size();
        let (dw, dh) = if framed.orientation().swaps() { (h, w) } else { (w, h) };
        // The tile's scale, and the thousandths of the wrapped picture's shorter side that make
        // `MIN_BAND` tile pixels.
        let scale = (bw / dw).min(bh / dh);
        let least = MIN_BAND * 1000.0 / (short * scale);
        if !least.is_finite() {
            break;
        }
        let wider: Vec<f64> = defaults.iter().map(|&v| if v > 0.0 { v.max(least).min(max) } else { v }).collect();
        if wider == lengths {
            break;
        }
        lengths = wider;
    }
    Ok(write(&lengths))
}

/// One tile: `recipe` with `entry` appended, rendered from `decode` to cover `size`, then cut to it;
/// or, when the picture includes a frame (the effect's or the recipe's), the whole picture fitted
/// in `size` and centred on [`PLACEHOLDER`], a frame effect's bands widened by [`readable`].
fn render_tile(decode: &Rgba8, recipe: &[String], entry: &str, size: (u32, u32)) -> Result<Rgba8, String> {
    let texts = recipe.iter().map(String::as_str).chain(std::iter::once(entry));
    let whole = parse(texts)?;
    let geometry = geometry_of(&whole, decode);
    if geometry.frames() > 0 {
        let entry = readable(decode, recipe, entry, size)?;
        let recipe = parse(recipe.iter().map(String::as_str).chain(std::iter::once(entry.as_str())))?;
        // `Target::Fit` never enlarges and keeps a frame's whole-pixel bands inside the box; a
        // decode smaller than the box (a tiny photo) is brought up to fill it on one side.
        let image = dalica_edit::render(dalica_edit::Source::upright(decode), &recipe, Target::Fit { width: size.0, height: size.1 })
            .map_err(|e| e.to_string())?;
        let (w, h) = (u64::from(image.width.max(1)), u64::from(image.height.max(1)));
        let (bw, bh) = (u64::from(size.0), u64::from(size.1));
        let image = if w < bw && h < bh {
            // The larger fitted size, one side reaching the box.
            let (fw, fh) = if w * bh >= h * bw { (bw, (h * bw / w).clamp(1, bh)) } else { ((w * bh / h).clamp(1, bw), bh) };
            let (fw, fh) = (u32::try_from(fw).unwrap_or(size.0), u32::try_from(fh).unwrap_or(size.1));
            dalica_image::resize::resize(&image, fw, fh).map_err(|e| e.reason())?
        } else {
            image
        };
        return letterbox(&image, size);
    }
    let recipe = whole;
    let (w, h) = geometry.size();
    let (dw, dh) = if geometry.orientation().swaps() { (h, w) } else { (w, h) };
    let scale = (f64::from(size.0) / dw).max(f64::from(size.1) / dh).min(dalica_edit::render::MAX_SCALE);
    if !(scale.is_finite() && scale > 0.0) {
        return Err("the photo has no size to cover the tile".into());
    }
    let image = dalica_edit::render(dalica_edit::Source::upright(decode), &recipe, Target::Scale(scale)).map_err(|e| e.to_string())?;
    cover(&image, size)
}

/// How long [`Session::wait_effect_tiles`] sleeps between two polls.
const WAIT_STEP: Duration = Duration::from_millis(2);

impl Session {
    /// Asks for the tiles of effect tab `tab` (3, 4 or 5) of photo `id`, each `size` device pixels
    /// (both sides 1 … [`MAX_TILE`]): the photo with its recipe so far and each effect at its
    /// defaults, made in the background in grid order ([`Session::take_effect_tiles`] hands them
    /// over). The same request again changes nothing; a new one drops what the last one had not
    /// handed over.
    pub fn request_effect_tiles(&mut self, id: PhotoId, tab: u8, size: (u32, u32)) -> Result<(), CmdError> {
        if !catalog::TABS.contains(&tab) {
            return Err(CmdError::BadParams(format!("tab: one of the effect tabs 3, 4 and 5, not {tab}")));
        }
        if !(1..=MAX_TILE).contains(&size.0) || !(1..=MAX_TILE).contains(&size.1) {
            return Err(CmdError::BadParams(format!("size: each side 1 to {MAX_TILE} pixels")));
        }
        let file = file_of(self, id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        let recipe = self.library.info(id).edits.clone();
        let tiles = &self.jobs.effect_tiles;
        if tiles.want.as_ref().is_some_and(|w| w.photo == id && w.file == file && w.tab == tab && w.size == size && w.recipe == recipe) {
            return Ok(());
        }
        self.start_effect_tiles(id, file, tab, size, recipe);
        Ok(())
    }

    /// A new generation of tiles: what the last request had on its way is cancelled (counted when
    /// it was for another photo), and the tiles are queued in grid order.
    fn start_effect_tiles(&mut self, photo: PhotoId, file: Source, tab: u8, size: (u32, u32), recipe: Vec<String>) {
        let tiles = &mut self.jobs.effect_tiles;
        if tiles.want.as_ref().is_some_and(|w| w.photo != photo) {
            tiles.drop_want();
        } else {
            tiles.cancel_all();
        }
        tiles.ready.retain(|t| t.photo == photo && t.tab == tab && t.recipe == recipe && (t.image.width, t.image.height) == size);
        tiles.generation = tiles.generation.wrapping_add(1);
        tiles.stats.requests = tiles.stats.requests.saturating_add(1);
        let generation = tiles.generation;
        tiles.want = Some(Want { photo, file, tab, size, recipe, generation, queue: jobs_of(tab) });
        self.schedule_effect_tiles();
    }

    /// Submits what can run: nothing while the edit view's preview has work on its way (the jobs
    /// not yet received are taken back); else the small decode, or every queued tile once it is in
    /// memory.
    fn schedule_effect_tiles(&mut self) {
        let busy = self.preview_busy() > 0;
        let (options, limits) = (self.thumbs.render().decode_options(), self.thumbs.screen_source().3);
        let tiles = &mut self.jobs.effect_tiles;
        if busy {
            let back = tiles.cancel_all();
            tiles.stats.yielded = tiles.stats.yielded.saturating_add(u64::try_from(back.len()).unwrap_or(u64::MAX));
            if let Some(want) = &mut tiles.want {
                // Taken back in the order asked for, before what was still queued.
                for job in back.into_iter().rev() {
                    want.queue.push_front(job);
                }
            }
            return;
        }
        let Some(want) = &tiles.want else { return };
        if want.queue.is_empty() {
            return;
        }
        let (generation, file, side, recipe) = (want.generation, want.file.clone(), want.size.0.max(want.size.1), want.recipe.clone());
        let size = want.size;
        let Some(decode) = tiles.decode_of(&file, side) else {
            if !tiles.outstanding.values().any(|(g, w, _)| *g == generation && *w == Work::Decode) {
                tiles.submit(generation, Work::Decode, None, move |_| Done::Decoded {
                    result: decode(&file, side, &options, &limits).map(Arc::new),
                    file,
                    cover: side,
                });
            }
            return;
        };
        let queue: Vec<Job> = tiles.want.as_mut().map(|w| w.queue.drain(..).collect()).unwrap_or_default();
        for job in queue {
            let (decode, recipe, entry) = (Arc::clone(&decode), recipe.clone(), job.entry.clone());
            let place = job.place;
            tiles.submit(generation, Work::Tile(place), Some(job), move |cancel| {
                if cancel.load(Ordering::SeqCst) {
                    return Done::Tile(Err("cancelled".into()));
                }
                Done::Tile(render_tile(&decode, &recipe, &entry, size))
            });
        }
    }

    /// Applies what the tile worker finished and starts what can run; returns how many results
    /// were applied. Cancels the tiles when the edit view shows another photo and remakes them
    /// when the photo's recipe or file changed. Never blocks and never touches the disk.
    pub(crate) fn poll_effect_tiles(&mut self) -> usize {
        if let Some(want) = &self.jobs.effect_tiles.want {
            let (photo, tab, size) = (want.photo, want.tab, want.size);
            let shown = (0..crate::preview::PANES).filter_map(|p| self.preview_photo_in(p)).collect::<Vec<_>>();
            match file_of(self, photo) {
                None => self.jobs.effect_tiles.drop_want(),
                Some(_) if !shown.is_empty() && !shown.contains(&photo) => self.jobs.effect_tiles.drop_want(),
                Some(file) => {
                    let recipe = self.library.info(photo).edits.clone();
                    if file != want.file || recipe != want.recipe {
                        self.start_effect_tiles(photo, file, tab, size, recipe);
                    }
                }
            }
        }
        let mut n = 0;
        while let Some((key, done)) = self.jobs.effect_tiles.pool.as_ref().and_then(Pool::try_recv) {
            n += 1;
            self.receive_effect_tile(key, done);
        }
        self.schedule_effect_tiles();
        n
    }

    fn receive_effect_tile(&mut self, key: u64, done: Done) {
        let tiles = &mut self.jobs.effect_tiles;
        let Some((generation, what, job)) = tiles.outstanding.remove(&key) else {
            // Taken back or cancelled meanwhile.
            tiles.stats.stale = tiles.stats.stale.saturating_add(1);
            return;
        };
        match done {
            Done::Decoded { file, cover, result } => match result {
                Ok(image) => {
                    tiles.stats.decodes = tiles.stats.decodes.saturating_add(1);
                    tiles.keep_decode(file, cover, image);
                }
                Err(e) => {
                    tiles.stats.failures = tiles.stats.failures.saturating_add(1);
                    log::warn!("effect tiles: the photo could not be read: {e}");
                    // Nothing more can be made of this request.
                    if let Some(want) = tiles.want.as_mut().filter(|w| w.generation == generation) {
                        want.queue.clear();
                    }
                }
            },
            Done::Tile(result) => {
                let current = tiles.want.as_ref().filter(|w| w.generation == generation);
                let (Some(want), Work::Tile(place), Some(job)) = (current, what, job) else {
                    tiles.stats.stale = tiles.stats.stale.saturating_add(1);
                    return;
                };
                match result {
                    Ok(image) => {
                        tiles.stats.renders = tiles.stats.renders.saturating_add(1);
                        let tile = EffectTile {
                            photo: want.photo,
                            tab: want.tab,
                            place,
                            name: job.name,
                            generation,
                            recipe: want.recipe.clone(),
                            image: Arc::new(image),
                        };
                        tiles.ready.push_back(tile);
                        while tiles.ready.len() > MAX_READY {
                            tiles.ready.pop_front();
                        }
                    }
                    Err(e) => {
                        tiles.stats.failures = tiles.stats.failures.saturating_add(1);
                        log::warn!("effect tiles: {} could not be made: {e}", job.name);
                    }
                }
            }
            Done::Panicked(message) => {
                tiles.stats.failures = tiles.stats.failures.saturating_add(1);
                log::error!("effect tiles: a job panicked: {message}");
            }
        }
    }

    /// The tiles made since the last call, in the order they were made (grid order), after
    /// applying what the worker finished. Each frame of the edit view's effect tabs calls it.
    pub fn take_effect_tiles(&mut self) -> Vec<EffectTile> {
        self.poll_effect_tiles();
        self.jobs.effect_tiles.ready.drain(..).collect()
    }

    /// Tiles of the last request not made yet (queued, or on their way with their decode).
    pub fn effect_tiles_pending(&self) -> usize {
        let tiles = &self.jobs.effect_tiles;
        tiles.want.as_ref().map_or(0, |w| w.queue.len()) + tiles.outstanding.len()
    }

    /// Drops the tiles: what is on its way is cancelled and nothing more is handed over (the edit
    /// view left the effect tabs). Small decodes stay in memory.
    pub fn close_effect_tiles(&mut self) {
        self.jobs.effect_tiles.drop_want();
    }

    pub fn effect_tile_stats(&self) -> EffectTileStats {
        self.jobs.effect_tiles.stats
    }

    /// Polls until every tile of the last request has been made (or could not be) or `timeout`
    /// has passed, keeping the tiles for [`Session::take_effect_tiles`]; returns whether it got
    /// there. Blocks the calling thread (CLI, tests, benches).
    pub fn wait_effect_tiles(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.poll();
            self.poll_effect_tiles();
            if self.effect_tiles_pending() == 0 {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(WAIT_STEP);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decode_covers_the_box_twice_whichever_way_the_photo_turns() {
        // A 3:2 photo and a 78-pixel tile: the shorter side reaches 156.
        assert_eq!(decode_edge(3000, 2000, (78, 48)), 234);
        assert_eq!(decode_edge(2000, 3000, (78, 48)), 234);
        assert_eq!(decode_edge(100, 100, (78, 48)), 156);
        assert_eq!(decode_edge(100_000, 1, (78, 48)), MAX_DECODE);
        assert_eq!(decode_edge(0, 0, (78, 48)), 1, "a photo of no size asks for little and fails in the decoder");
    }

    #[test]
    fn cover_cuts_the_centre_to_the_box_and_sizes_it_exactly() {
        // Columns 0..8 hold their index in red.
        let data = (0..8u8 * 4).flat_map(|i| [i % 8, 0, 0, 255]).collect();
        let image = Rgba8 { width: 8, height: 4, data };
        let tile = cover(&image, (2, 2)).unwrap();
        assert_eq!((tile.width, tile.height), (2, 2));
        // The centre 4 × 4 (columns 2..6) averaged to two columns: 2.5 and 4.5, rounded.
        let reds: Vec<u8> = tile.data.chunks(4).map(|p| p[0]).collect();
        assert!(reds[0] < reds[1] && reds[0] >= 2 && reds[1] <= 5, "{reds:?}");
        assert_eq!(cover(&image, (8, 4)).unwrap().data, image.data, "the same shape and size is the picture itself");
        assert!(cover(&Rgba8 { width: 0, height: 0, data: vec![] }, (2, 2)).is_err());
        assert!(cover(&Rgba8 { width: 4, height: 4, data: vec![0; 3] }, (2, 2)).is_err(), "a buffer that is not the picture");
    }

    #[test]
    fn a_frame_effects_tile_draws_each_band_at_least_two_pixels() {
        // A 3:2 decode as a 78 × 48 tile of a 480 × 320 photo asks for.
        let decode = Rgba8 { width: 234, height: 156, data: [90, 140, 60, 255].repeat(234 * 156) };
        let entry = |name: &str| catalog::lookup(name).and_then(|op| op.default_entry()).unwrap();
        // Each band's tile pixels: its thousandths of the photo's shorter side at the fitted scale.
        let bands = |text: &str, size: (u32, u32)| -> Vec<f64> {
            let recipe = Recipe::parse([text]).unwrap();
            let (w, h) = geometry_of(&recipe, &decode).size();
            let px = 156.0 * (f64::from(size.0) / w).min(f64::from(size.1) / h) / 1000.0;
            match recipe.pipeline().next().and_then(|op| op.typed()).and_then(Result::ok) {
                Some(Typed::Border(b)) => vec![b.outer * px, b.inner * px],
                Some(Typed::MuseumMatte(m)) => vec![m.outer * px, m.inner * px],
                Some(Typed::DropShadow(d)) => vec![(d.distance + d.blur) * px, d.distance / d.blur],
                other => panic!("{other:?}"),
            }
        };
        for name in ["border", "museum_matte", "drop_shadow"] {
            let default = bands(&entry(name), (78, 48));
            let thinnest = if name == "drop_shadow" { default[0] } else { default[0].min(default[1]) };
            assert!(thinnest < 2.0, "{name}: the defaults are thinner than 2 px at 78 × 48: {default:?}");
            let text = readable(&decode, &[], &entry(name), (78, 48)).unwrap();
            let wide = bands(&text, (78, 48));
            if name == "drop_shadow" {
                assert!(wide[0] >= 2.0, "{text}: the shadow reaches {wide:?}");
                assert!((wide[1] - default[1]).abs() < 1e-9, "{text}: distance and blur keep their proportion");
            } else {
                assert!(wide.iter().all(|&b| b >= 2.0), "{text}: {wide:?}");
            }
            // A tile large enough for the defaults draws them.
            assert_eq!(readable(&decode, &[], &entry(name), (1024, 1024)).unwrap(), entry(name), "{name}");
        }
        assert_eq!(readable(&decode, &[], "sepia()", (78, 48)).unwrap(), "sepia()", "not a frame: as it is");
        assert_eq!(readable(&decode, &[], &entry("widescreen"), (78, 48)).unwrap(), entry("widescreen"), "its bars are wide");
    }

    #[test]
    fn a_fitted_tile_of_a_tiny_photo_still_reaches_the_box() {
        // A 12 × 8 decode, smaller than the box: `Target::Fit` does not enlarge, the tile does.
        let decode = Rgba8 { width: 12, height: 8, data: [200, 120, 40, 255].repeat(96) };
        let entry = catalog::lookup("museum_matte").and_then(|op| op.default_entry()).unwrap();
        let tile = render_tile(&decode, &[], &entry, (78, 48)).unwrap();
        assert_eq!((tile.width, tile.height), (78, 48));
        let grey = |x: u32, y: u32| tile.data.get(((y * 78 + x) * 4) as usize..((y * 78 + x) * 4 + 4) as usize) == Some(&PLACEHOLDER[..]);
        assert!(!grey(39, 0) && !grey(39, 47), "the picture reaches the top and bottom");
        assert!(grey(0, 24) && grey(77, 24), "grey at the sides");
        assert!(letterbox(&Rgba8 { width: 4, height: 4, data: vec![0; 3] }, (8, 8)).is_err(), "a buffer that is not the picture");
    }
}
