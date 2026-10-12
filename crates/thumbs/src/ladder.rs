//! The thumbnail ladder (`architecture.md` §6.5): which cached size a cell draws, what to prefetch
//! while the view scrolls, and the two jobs that fill the [`Cache`].
//!
//! - **Sizes** are device pixels on the long side: 128, 256, 512, 1024 and the screen-size preview
//!   ([`Size`]). A cell draws the smallest size that covers it ([`covering`]); while that one
//!   loads, the next smaller one it has, scaled up ([`choose`]).
//! - **[`scan`]**, run for every photo a scan adds or changes: one decode of the photo makes the
//!   128 and 256 entries and the colour sketch. Entries already in the cache are not made again.
//! - **[`fetch`]**, run on demand: one size into memory, read from the cache when it is there,
//!   else decoded from the photo and stored.
//! - **[`prefetch`]**: the order to load the cells around the visible ones, further ahead the
//!   faster the view moves.
//!
//! Both jobs run on pool workers, never on the thread that owns the session. Entries are JPEG at
//! [`QUALITY`] (fastest to decode); the [`Key`] of a photo is content-addressed (§6.4), and the
//! screen-size entry's key also holds the screen size, so a different display never reads a
//! preview made for another.

use std::io::Cursor;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::pack::{Cache, Key, QUALITY, Size};
use crate::{Rgba8, sketch};

/// The version of the thumbnail renderer: part of every [`Key`], bumped when the same photo would
/// render differently (a new resampler, colour management), so old entries are never read again.
pub const RENDERER_VERSION: u64 = 1;

/// The screen-size preview's long side when the display is not known yet.
pub const DEFAULT_SCREEN: u32 = 2560;

/// The largest screen-size preview made (an 8K display's long side).
pub const MAX_SCREEN: u32 = 7680;

/// Seconds of scrolling the prefetch looks ahead beyond one screen.
pub const LOOKAHEAD_SECS: f64 = 0.75;

/// The furthest the prefetch reaches ahead, in screens.
pub const MAX_AHEAD_SCREENS: usize = 4;

/// The sizes a scan makes for every photo.
pub const SCAN_SIZES: [Size; 2] = [Size::S128, Size::S256];

/// Why a ladder job failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The photo could not be read or decoded.
    #[error(transparent)]
    Photo(#[from] crate::Error),
    #[error("cannot encode the thumbnail: {0}")]
    Encode(String),
    #[error("the thumbnail cache failed: {0}")]
    Cache(String),
    #[error("cancelled")]
    Cancelled,
}

impl From<crate::pack::Error> for Error {
    fn from(e: crate::pack::Error) -> Error {
        Error::Cache(e.to_string())
    }
}

/// One photo file as the library knows it: what its cache key is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    /// Bytes on disk.
    pub file_size: u64,
    /// Modification time, milliseconds since the epoch.
    pub mtime_ms: i64,
}

impl Source {
    /// The photo's content-addressed key (§6.4) as the default [`Render`] makes it: path, size,
    /// modification time, the recipe (none yet: edits arrive with the edit view) and
    /// [`RENDERER_VERSION`].
    pub fn key(&self) -> Key {
        self.key_for(&Render::default())
    }

    /// The photo's key when rendered with `render`: the render options join the recipe hash, so
    /// entries made one way are never read the other way.
    pub fn key_for(&self, render: &Render) -> Key {
        Key::new(&self.path, self.file_size, i128::from(self.mtime_ms) * 1_000_000, render.recipe_hash(), RENDERER_VERSION)
    }
}

/// How the ladder renders a photo, besides its size: what goes into [`Source::key_for`] and the
/// decode options of [`scan_with`] and [`fetch_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Render {
    /// Apply embedded ICC profiles (View → Use Color Management, on by default).
    pub color_managed: bool,
}

impl Default for Render {
    fn default() -> Render {
        Render { color_managed: true }
    }
}

impl Render {
    /// The part of the cache key these options make: 0 for the defaults (the key entries made
    /// before render options existed already have), a fixed tag otherwise.
    pub fn recipe_hash(&self) -> u64 {
        if self.color_managed { 0 } else { 0x636d_2d6f_6666_0001 }
    }

    /// The decode options.
    pub fn decode_options(&self) -> dalica_image::DecodeOptions {
        dalica_image::DecodeOptions { color_managed: self.color_managed }
    }
}

/// The key of `size`'s entry for the photo whose key is `photo`: the photo's own key, except for
/// the screen-size preview, whose key also holds `screen` (its long side).
pub fn entry_key(photo: Key, size: Size, screen: u32) -> Key {
    if size != Size::Screen {
        return photo;
    }
    // FNV-1a over the photo key and the screen size: stable on every platform.
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for b in photo.0.to_le_bytes().into_iter().chain(screen.to_le_bytes()) {
        h = (h ^ u128::from(b)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    Key(h)
}

/// The long side of `size` in pixels, with `screen` for the screen-size preview (clamped to
/// 1..=[`MAX_SCREEN`]).
pub fn pixels(size: Size, screen: u32) -> u32 {
    size.pixels().unwrap_or_else(|| screen.clamp(1, MAX_SCREEN))
}

/// The ladder sizes by their long side, smallest first (the screen size sorts by its pixels).
fn by_pixels(screen: u32) -> [Size; 5] {
    let mut sizes = Size::ALL;
    sizes.sort_by_key(|&s| (pixels(s, screen), s));
    sizes
}

/// The size that covers a cell whose long side is `need` device pixels: the smallest ladder size
/// at least that long, or the largest one when none is (sizes above the ladder are a full decode,
/// which the edit view makes).
pub fn covering(need: u32, screen: u32) -> Size {
    let sizes = by_pixels(screen);
    sizes.iter().copied().find(|&s| pixels(s, screen) >= need).unwrap_or(sizes[4])
}

/// What a cell draws from the sizes it has in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A size that covers the cell (the covering one, or a larger one already there).
    Covers(Size),
    /// The largest size it has below the covering one, to scale up while that one loads.
    Smaller(Size),
    /// No ladder size yet.
    Nothing,
}

/// The best of the sizes `has` says are in memory for a cell of `need` device pixels: the
/// smallest that covers it, else the largest smaller one.
pub fn choose(need: u32, screen: u32, has: impl Fn(Size) -> bool) -> Choice {
    let want = pixels(covering(need, screen), screen);
    let sizes = by_pixels(screen);
    if let Some(s) = sizes.iter().copied().find(|&s| pixels(s, screen) >= want && has(s)) {
        return Choice::Covers(s);
    }
    match sizes.iter().rev().copied().find(|&s| pixels(s, screen) < want && has(s)) {
        Some(s) => Choice::Smaller(s),
        None => Choice::Nothing,
    }
}

/// The cells to prefetch around the visible ones (`visible`, indices into a list of `len` cells),
/// nearest first: when the view moves (`velocity` in cells per second, positive towards higher
/// indices), one screen plus [`LOOKAHEAD_SECS`] of travel ahead (at most [`MAX_AHEAD_SCREENS`]),
/// then half a screen behind; when it stands still, one screen on each side, alternating.
pub fn prefetch(visible: Range<usize>, len: usize, velocity: f64) -> Vec<usize> {
    let start = visible.start.min(len);
    let end = visible.end.clamp(start, len);
    let screen = (end - start).max(1);
    let speed = if velocity.is_finite() { velocity.abs() } else { 0.0 };
    if speed < 1e-9 {
        let mut out = Vec::with_capacity(screen * 2);
        for i in 0..screen {
            if let Some(after) = end.checked_add(i).filter(|&a| a < len) {
                out.push(after);
            }
            if let Some(before) = start.checked_sub(i + 1) {
                out.push(before);
            }
        }
        return out;
    }
    let max_extra = screen.saturating_mul(MAX_AHEAD_SCREENS - 1);
    // `as` saturates for floats; the cap keeps it small.
    let extra = ((speed * LOOKAHEAD_SECS).ceil() as usize).min(max_extra);
    let ahead = screen + extra;
    let behind = screen.div_ceil(2);
    let forward = velocity > 0.0;
    let after = end..end.saturating_add(if forward { ahead } else { behind }).min(len);
    let before = (start.saturating_sub(if forward { behind } else { ahead })..start).rev();
    if forward { after.chain(before).collect() } else { before.chain(after).collect() }
}

/// What [`scan`] made.
#[derive(Clone, Debug, PartialEq)]
pub struct Scanned {
    /// The 128 and 256 images, when the photo was decoded (empty when both entries were already in
    /// the cache).
    pub images: Vec<(Size, Rgba8)>,
    /// The colour sketch, when asked for.
    pub sketch: Option<Vec<u8>>,
    /// Whether the photo itself was decoded (at most once per scan).
    pub decoded: bool,
}

/// The scan job for one photo: makes its 128 and 256 entries unless the cache has both, from one
/// decode of the photo, and its colour sketch when `want_sketch`. A photo whose entries are
/// already cached is not decoded; its sketch is then made from the cached 128 entry.
pub fn scan(cache: &Cache, source: &Source, want_sketch: bool, limits: &dalica_image::Limits, cancel: &AtomicBool) -> Result<Scanned, Error> {
    scan_with(cache, source, &Render::default(), want_sketch, limits, cancel)
}

/// [`scan`] rendered with `render` (its entries under [`Source::key_for`]).
pub fn scan_with(
    cache: &Cache,
    source: &Source,
    render: &Render,
    want_sketch: bool,
    limits: &dalica_image::Limits,
    cancel: &AtomicBool,
) -> Result<Scanned, Error> {
    let key = source.key_for(render);
    if SCAN_SIZES.iter().all(|&s| cache.contains(s, key)) {
        if !want_sketch {
            return Ok(Scanned { images: Vec::new(), sketch: None, decoded: false });
        }
        if let Some(small) = load(cache, Size::S128, key, limits)? {
            let sketch = sketch::encode(&small).map_err(|e| Error::Encode(e.to_string()))?;
            return Ok(Scanned { images: vec![(Size::S128, small)], sketch: Some(sketch), decoded: false });
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let large = crate::thumbnail_with(&source.path, 256, &render.decode_options(), limits)?;
    let small = dalica_image::resize::fit(&large, 128).map_err(|e| Error::Photo(e.into()))?;
    let sketch = if want_sketch { Some(sketch::encode(&small).map_err(|e| Error::Encode(e.to_string()))?) } else { None };
    let mut images = Vec::with_capacity(2);
    for (size, img) in [(Size::S128, &small), (Size::S256, &large)] {
        images.push((size, store(cache, size, key, img, limits)?));
    }
    Ok(Scanned { images, sketch, decoded: true })
}

/// What [`fetch`] brought into memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Fetched {
    pub image: Rgba8,
    /// Whether the photo itself was decoded (the cache did not have the entry).
    pub decoded: bool,
}

/// The fetch job: `size` of the photo, from the cache when it has the entry, else decoded from
/// the photo (to fit `size`, never enlarged) and stored. `screen` is the screen-size preview's long
/// side.
pub fn fetch(cache: &Cache, source: &Source, size: Size, screen: u32, limits: &dalica_image::Limits, cancel: &AtomicBool) -> Result<Fetched, Error> {
    fetch_with(cache, source, &Render::default(), size, screen, limits, cancel)
}

/// [`fetch`] rendered with `render` (its entry under [`Source::key_for`]).
pub fn fetch_with(
    cache: &Cache,
    source: &Source,
    render: &Render,
    size: Size,
    screen: u32,
    limits: &dalica_image::Limits,
    cancel: &AtomicBool,
) -> Result<Fetched, Error> {
    let key = entry_key(source.key_for(render), size, screen);
    if let Some(image) = load(cache, size, key, limits)? {
        return Ok(Fetched { image, decoded: false });
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let image = crate::thumbnail_with(&source.path, pixels(size, screen), &render.decode_options(), limits)?;
    Ok(Fetched { image: store(cache, size, key, &image, limits)?, decoded: true })
}

/// Stores `image` as the `size` entry of `key` and returns the entry's pixels as a later read
/// gives them (the JPEG decoded again), so an image looks the same whether it was just made or
/// read from the cache.
fn store(cache: &Cache, size: Size, key: Key, image: &Rgba8, limits: &dalica_image::Limits) -> Result<Rgba8, Error> {
    let bytes = encode_jpeg(image)?;
    cache.put(size, key, &bytes)?;
    decode_entry(&bytes, limits)
}

/// The cached entry of `key` at `size`, decoded; `None` when there is none or it does not decode
/// (a damaged entry is made again).
fn load(cache: &Cache, size: Size, key: Key, limits: &dalica_image::Limits) -> Result<Option<Rgba8>, Error> {
    Ok(cache.get(size, key)?.and_then(|bytes| decode_entry(&bytes, limits).ok()))
}

/// A cache entry's pixels.
pub fn decode_entry(bytes: &[u8], limits: &dalica_image::Limits) -> Result<Rgba8, Error> {
    dalica_image::decode(bytes, limits).map_err(|e| Error::Photo(e.into()))
}

/// The JPEG of an entry at [`QUALITY`]. Alpha is dropped: entries are opaque, like photos.
pub fn encode_jpeg(image: &Rgba8) -> Result<Vec<u8>, Error> {
    let pixels = u64::from(image.width) * u64::from(image.height);
    if image.width == 0 || image.height == 0 || u64::try_from(image.data.len()).ok() != pixels.checked_mul(4) {
        return Err(Error::Encode(format!("{} × {} with {} bytes is not an RGBA image", image.width, image.height, image.data.len())));
    }
    let rgb: Vec<u8> = image.data.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
    let mut out = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut out, QUALITY)
        .write_image(&rgb, image.width, image.height, ExtendedColorType::Rgb8)
        .map_err(|e| Error::Encode(e.to_string()))?;
    Ok(out.into_inner())
}
