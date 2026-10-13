//! What the preview's workers do (`architecture.md` §5–§6): read a photo's screen-size source (the
//! ladder's screen entry, else the file decoded under the limits), keep the ladder's entry made,
//! decode the whole photo for a zoomed window, and render. Never called on the owner thread.
//!
//! **Speed** (`development.md` §8, measured by the `edit_preview` bench): a 24 MP JPEG decodes
//! whole in about 50 ms, and the renderer brings it to the screen size in about 55 ms, where the
//! decoder's own downscale to a size takes about 140 ms more; so a photo not in the cache is
//! decoded whole, its first preview rendered straight from that decode, and its screen-size source
//! made from it afterwards ([`reduce`]). Re-rendering a 2560-pixel preview spends most of its time
//! resampling the source to the box: a [`Screen`] keeps the source resampled to the box's scale
//! (a *base*, made once per scale by [`make_base`]), from which a recipe that does not crop or
//! straighten renders without resampling, to the same pixels.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use dalica_edit::{Geometry, Recipe, Region, Target};
use dalica_image::{Limits, Orientation, Rgba8};
use dalica_thumbs::ladder::{self, Render, Source};
use dalica_thumbs::pack::{Cache, Size};

use super::histogram::Histogram;

/// The largest photo file a preview reads.
pub const MAX_FILE: u64 = dalica_thumbs::MAX_FILE;
/// Bases kept per source (the box's scale, a draft's, and a rotation by 90° asks for another).
pub const BASES: usize = 3;

/// How the session's thumbnail ladder makes its screen-size entry: the cache (`None` when
/// thumbnails are off), the rendering, the screen size and the decode limits.
#[derive(Clone)]
pub(crate) struct Ladder {
    pub cache: Option<Arc<Cache>>,
    pub render: Render,
    pub screen: u32,
    pub limits: Limits,
}

impl Ladder {
    /// The key of `file`'s unedited screen-size entry.
    fn key(&self, file: &Source) -> dalica_thumbs::pack::Key {
        ladder::entry_key(file.key_for(&self.render), Size::Screen, self.screen)
    }

    fn screen(&self) -> u32 {
        ladder::pixels(Size::Screen, self.screen)
    }
}

/// Where a source came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The ladder's screen entry, read from the cache.
    Cache,
    /// The photo file, decoded.
    Decoded,
    /// Already in the preview's memory.
    Memory,
}

/// A photo's screen-size source and its bases. Only workers lock the bases.
pub(crate) struct Screen {
    pub image: Arc<Rgba8>,
    bases: Mutex<Vec<(u64, Arc<Rgba8>)>>,
}

impl Screen {
    pub fn new(image: Arc<Rgba8>) -> Screen {
        Screen { image, bases: Mutex::new(Vec::new()) }
    }

    fn base(&self, scale: f64) -> Option<Arc<Rgba8>> {
        let bases = self.bases.lock().unwrap_or_else(PoisonError::into_inner);
        bases.iter().find(|(s, _)| *s == scale.to_bits()).map(|(_, b)| Arc::clone(b))
    }
}

/// What reading a photo gave.
pub(crate) enum Read {
    /// Its screen-size source.
    Screen(Rgba8, Origin),
    /// The whole photo, larger than the screen size (its source is made from it by [`reduce`]).
    Full(Rgba8),
}

/// `file` upright: the ladder's screen entry when the cache has it, else the whole photo decoded
/// (a photo whose whole decode would pass `max_full` bytes is decoded to the screen size).
pub(crate) fn read(file: &Source, ladder: &Ladder, max_full: u64) -> Result<Read, String> {
    if let Some(cache) = &ladder.cache
        && let Ok(Some(bytes)) = cache.get(Size::Screen, ladder.key(file))
        && let Ok(image) = ladder::decode_entry(&bytes, &ladder.limits)
    {
        return Ok(Read::Screen(image, Origin::Cache));
    }
    let bytes = read_file(file)?;
    let options = ladder.render.decode_options();
    let header = dalica_image::read_header(&bytes, &ladder.limits).map_err(|e| e.reason())?;
    let screen = ladder.screen();
    if full_bytes(header.width, header.height) <= max_full {
        let full = dalica_image::decode_with(&bytes, &options, &ladder.limits).map_err(|e| e.reason())?;
        if full.width.max(full.height) <= screen {
            return Ok(Read::Screen(full, Origin::Decoded));
        }
        return Ok(Read::Full(full));
    }
    let image = dalica_image::decode_thumbnail_with(&bytes, screen, &options, &ladder.limits).map_err(|e| e.reason())?;
    Ok(Read::Screen(image, Origin::Decoded))
}

/// The whole photo brought to the screen size by the renderer.
pub(crate) fn reduce(full: &Rgba8, ladder: &Ladder) -> Result<Rgba8, String> {
    let screen = ladder.screen();
    dalica_edit::render(dalica_edit::Source::upright(full), &Recipe::new(), Target::Fit { width: screen, height: screen }).map_err(|e| e.to_string())
}

/// Stores `image` as `file`'s unedited screen-size entry, so the next open reads it.
pub(crate) fn store(file: &Source, ladder: &Ladder, image: &Rgba8) -> Result<(), String> {
    let Some(cache) = &ladder.cache else { return Ok(()) };
    let bytes = ladder::encode_jpeg(image).map_err(|e| e.to_string())?;
    cache.put(Size::Screen, ladder.key(file), &bytes).map_err(|e| e.to_string())
}

fn full_bytes(width: u32, height: u32) -> u64 {
    u64::from(width).saturating_mul(u64::from(height)).saturating_mul(4)
}

/// The whole photo, upright, when its pixels fit in `max_bytes` of RGBA.
pub(crate) fn full(file: &Source, ladder: &Ladder, max_bytes: u64) -> Result<Rgba8, String> {
    let bytes = read_file(file)?;
    let header = dalica_image::read_header(&bytes, &ladder.limits).map_err(|e| e.reason())?;
    if full_bytes(header.width, header.height) > max_bytes {
        return Err(format!("{} × {} is more than the preview keeps in memory", header.width, header.height));
    }
    dalica_image::decode_with(&bytes, &ladder.render.decode_options(), &ladder.limits).map_err(|e| e.reason())
}

fn read_file(file: &Source) -> Result<Vec<u8>, String> {
    match dalica_fs::read(&file.path, MAX_FILE) {
        Ok(Some(r)) => Ok(r.bytes),
        Ok(None) => Err("the file is missing".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// A preview rendered on a worker.
#[derive(Clone, Debug)]
pub(crate) struct Rendered {
    pub image: Arc<Rgba8>,
    pub histogram: Arc<Histogram>,
    /// The recipe it shows, as stored.
    pub recipe: Vec<String>,
    /// The box it was fitted to (before the draft scale).
    pub fit: (u32, u32),
    /// The draft scale (1: full size).
    pub scale: f64,
    /// The time the render and its histogram took on the worker.
    pub ms: f64,
    /// The base scale that would have spared the resample (for [`make_base`]).
    pub wants_base: Option<f64>,
    /// Rendered from a screen-size source (not from a whole decode).
    pub from_screen: bool,
}

/// What to render: the recipe (parsed and as stored) fitted to a box at a scale.
#[derive(Clone, Debug)]
pub(crate) struct Ask {
    pub recipe: Recipe,
    pub texts: Vec<String>,
    pub fit: (u32, u32),
    pub scale: f64,
}

impl Ask {
    fn target(&self) -> Target {
        let side = |v: u32| ((f64::from(v) * self.scale).round().max(1.0)) as u32;
        Target::Fit { width: side(self.fit.0), height: side(self.fit.1) }
    }
}

/// The scale a render of `ask` from a `width × height` source resamples it by (the box's fitted
/// scale times the draft scale), when its recipe neither crops nor straightens and the scale is a
/// reduction: a base at that scale renders it without resampling.
pub(crate) fn base_scale(ask: &Ask, width: u32, height: u32) -> Option<f64> {
    if width == 0 || height == 0 || !(ask.scale.is_finite() && ask.scale > 0.0) {
        return None;
    }
    let geometry = Geometry::new(&ask.recipe, Orientation::default(), width, height);
    let (w, h) = geometry.size();
    if (w - f64::from(width)).abs() > 1e-9 || (h - f64::from(height)).abs() > 1e-9 {
        return None;
    }
    let (dw, dh) = if geometry.orientation().swaps() { (h, w) } else { (w, h) };
    // As `Target::Fit` computes it, so a full-size render through a base is the same picture.
    let s = (f64::from(ask.fit.0) / dw).min(f64::from(ask.fit.1) / dh).min(1.0) * ask.scale;
    (s.is_finite() && s > 0.0 && s < 1.0).then_some(s)
}

/// `source` (upright; with its bases when it is a [`Screen`]) with `ask`'s recipe, fitted to its
/// box at its scale, and its histogram.
pub(crate) fn render(source: &Rgba8, screen: Option<&Screen>, ask: &Ask) -> Result<Rendered, String> {
    let started = Instant::now();
    let base_at = base_scale(ask, source.width, source.height);
    let base = base_at.and_then(|s| screen.and_then(|sc| sc.base(s)));
    let image = match &base {
        // The base is the source resampled exactly as the fitted render would: render it at 1.
        Some(base) => dalica_edit::render(dalica_edit::Source::upright(base), &ask.recipe, Target::Full),
        None => match base_at {
            Some(scale) => dalica_edit::render(dalica_edit::Source::upright(source), &ask.recipe, Target::Scale(scale)),
            None => dalica_edit::render(dalica_edit::Source::upright(source), &ask.recipe, ask.target()),
        },
    }
    .map_err(|e| e.to_string())?;
    let histogram = Histogram::of(&image);
    Ok(Rendered {
        image: Arc::new(image),
        histogram: Arc::new(histogram),
        recipe: ask.texts.clone(),
        fit: ask.fit,
        scale: ask.scale,
        ms: started.elapsed().as_secs_f64() * 1000.0,
        wants_base: if base.is_none() && screen.is_some() { base_at } else { None },
        from_screen: screen.is_some(),
    })
}

/// Makes `screen`'s base at `scale` (the source resampled as a fitted render resamples it).
pub(crate) fn make_base(screen: &Screen, scale: f64) -> Result<(), String> {
    if screen.base(scale).is_some() {
        return Ok(());
    }
    let base = dalica_edit::render(dalica_edit::Source::upright(&screen.image), &Recipe::new(), Target::Scale(scale)).map_err(|e| e.to_string())?;
    let mut bases = screen.bases.lock().unwrap_or_else(PoisonError::into_inner);
    bases.retain(|(s, _)| *s != scale.to_bits());
    bases.insert(0, (scale.to_bits(), Arc::new(base)));
    bases.truncate(BASES);
    Ok(())
}

/// A window of the edited photo at full resolution.
pub(crate) fn region(full: &Rgba8, recipe: &Recipe, region: Region) -> Result<Rgba8, String> {
    dalica_edit::render_region(dalica_edit::Source::upright(full), recipe, region).map_err(|e| e.to_string())
}
