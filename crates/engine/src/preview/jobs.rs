//! What the preview's workers do (`architecture.md` §5–§6): read a photo's screen-size source (the
//! ladder's screen entry, else the file decoded under the limits), keep the ladder's entry made,
//! decode the whole photo for a zoomed window, and render. Never called on the owner thread.
//!
//! **Speed on two cores** (`development.md` §8; the budgets job runs on a 2-vCPU runner). Measured
//! with `taskset -c 0,1` on a 6000 × 4000 JPEG: the whole decode takes about 50 ms, but the
//! renderer's fractional resample is the expensive part: 24 MP to 2560 pixels about 290 ms, a
//! 2560-pixel source to a 2400 × 1600 box about 220 ms, while a render at the source's own scale
//! (`Target::Full`) takes 55–80 ms. So the preview never asks the renderer for a fractional
//! reduction of a picture it can reduce itself:
//!
//! - a source larger than twice the screen size is reduced by exact halves ([`halve`], a 2 × 2
//!   box average in parallel bands), to between one and two screen sizes;
//! - the preview is rendered at the largest power-of-two scale of its source (1, ½, ¼ …) that
//!   still covers the box, never above the source's resolution: a recipe that neither crops nor
//!   straightens renders at `Target::Full` from the source halved that many times (a *base*, kept
//!   with the source); one that crops or straightens renders from the source at that scale. The
//!   edit view draws the preview fitted to its box, as it draws any texture;
//! - a recipe with no operations is its base, with no render at all.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use dalica_edit::{Geometry, Recipe, Region, Target};
use dalica_image::{Limits, Orientation, Rgba8};
use dalica_thumbs::ladder::{self, Render, Source};
use dalica_thumbs::pack::{Cache, Size};
use rayon::prelude::*;

use super::histogram::Histogram;

/// The largest photo file a preview reads.
pub const MAX_FILE: u64 = dalica_thumbs::MAX_FILE;
/// Halved copies kept per source.
pub const BASES: usize = 4;
/// The most halvings asked for (a 65 535-pixel side halves to one pixel in 16).
pub const MAX_HALVINGS: u32 = 16;

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

/// The picture halved: each output pixel the rounded mean of a 2 × 2 block (an odd last row or
/// column is dropped). `None` when a side is under 2 pixels or the buffer is not the picture.
pub fn halve(image: &Rgba8) -> Option<Rgba8> {
    let (w, h) = (image.width as usize, image.height as usize);
    if w < 2 || h < 2 || w.checked_mul(h)?.checked_mul(4)? != image.data.len() {
        return None;
    }
    let (ow, oh) = (w / 2, h / 2);
    let stride = w * 4;
    let mut out = Vec::new();
    out.try_reserve_exact(ow * oh * 4).ok()?;
    out.resize(ow * oh * 4, 0u8);
    out.par_chunks_mut(ow * 4).enumerate().for_each(|(y, row)| {
        let (Some(r0), Some(r1)) = (image.data.get(2 * y * stride..(2 * y + 1) * stride), image.data.get((2 * y + 1) * stride..(2 * y + 2) * stride))
        else {
            return;
        };
        let (top, bottom) = (r0.as_chunks::<8>().0, r1.as_chunks::<8>().0);
        for ((px, a), b) in row.as_chunks_mut::<4>().0.iter_mut().zip(top).zip(bottom) {
            for c in 0..4 {
                let sum = u16::from(a[c]) + u16::from(a[c + 4]) + u16::from(b[c]) + u16::from(b[c + 4]);
                px[c] = ((sum + 2) / 4) as u8;
            }
        }
    });
    Some(Rgba8 { width: ow as u32, height: oh as u32, data: out })
}

/// A photo's screen-size source and its halved copies (bases). Only workers lock the bases.
pub(crate) struct Screen {
    pub image: Arc<Rgba8>,
    bases: Mutex<Vec<(u32, Arc<Rgba8>)>>,
}

impl Screen {
    pub fn new(image: Arc<Rgba8>) -> Screen {
        Screen { image, bases: Mutex::new(Vec::new()) }
    }

    /// The source halved `n` times (made from the nearest copy there is, and kept), or halved as
    /// often as it can be when it is too small for `n`.
    fn base(&self, n: u32) -> Arc<Rgba8> {
        if n == 0 {
            return Arc::clone(&self.image);
        }
        let mut bases = self.bases.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, b)) = bases.iter().find(|(k, _)| *k == n) {
            return Arc::clone(b);
        }
        let (mut at, mut image) =
            bases.iter().filter(|(k, _)| *k < n).max_by_key(|(k, _)| *k).map_or((0, Arc::clone(&self.image)), |(k, b)| (*k, Arc::clone(b)));
        while at < n {
            match halve(&image) {
                Some(half) => {
                    image = Arc::new(half);
                    at += 1;
                }
                None => break,
            }
        }
        if at > 0 && !bases.iter().any(|(k, _)| *k == at) {
            bases.insert(0, (at, Arc::clone(&image)));
            bases.truncate(BASES);
        }
        image
    }
}

/// `file` upright at between one and two screen sizes (smaller photos whole): the ladder's screen
/// entry when the cache has it, else the photo decoded and halved (a photo whose whole decode would
/// pass `max_full` bytes is decoded to the screen size by the decoder).
pub(crate) fn read(file: &Source, ladder: &Ladder, max_full: u64) -> Result<(Rgba8, Origin), String> {
    if let Some(cache) = &ladder.cache
        && let Ok(Some(bytes)) = cache.get(Size::Screen, ladder.key(file))
        && let Ok(image) = ladder::decode_entry(&bytes, &ladder.limits)
    {
        return Ok((image, Origin::Cache));
    }
    let bytes = read_file(file)?;
    let options = ladder.render.decode_options();
    let header = dalica_image::read_header(&bytes, &ladder.limits).map_err(|e| e.reason())?;
    let screen = ladder.screen();
    if full_bytes(header.width, header.height) <= max_full {
        let mut image = dalica_image::decode_with(&bytes, &options, &ladder.limits).map_err(|e| e.reason())?;
        while image.width.max(image.height) / 2 >= screen {
            match halve(&image) {
                Some(half) => image = half,
                None => break,
            }
        }
        return Ok((image, Origin::Decoded));
    }
    let image = dalica_image::decode_thumbnail_with(&bytes, screen, &options, &ladder.limits).map_err(|e| e.reason())?;
    Ok((image, Origin::Decoded))
}

/// Settles a decoded source: fitted to the screen size when it is larger (the source the next
/// renders draw from, returned), and stored as `file`'s unedited screen-size entry, so the next
/// open reads it.
pub(crate) fn settle(file: &Source, ladder: &Ladder, image: &Rgba8) -> Result<Option<Rgba8>, String> {
    let screen = ladder.screen();
    let fitted = if image.width.max(image.height) > screen { Some(dalica_image::resize::fit(image, screen).map_err(|e| e.reason())?) } else { None };
    if let Some(cache) = &ladder.cache {
        let bytes = ladder::encode_jpeg(fitted.as_ref().unwrap_or(image)).map_err(|e| e.to_string())?;
        cache.put(Size::Screen, ladder.key(file), &bytes).map_err(|e| e.to_string())?;
    }
    Ok(fitted)
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
    /// The box it covers (before the draft scale).
    pub fit: (u32, u32),
    /// The draft scale (1: full size).
    pub scale: f64,
    /// The time the render and its histogram took on the worker.
    pub ms: f64,
}

/// What to render: the recipe (parsed and as stored) covering a box, at a draft scale (1, ½, ¼).
#[derive(Clone, Debug)]
pub(crate) struct Ask {
    pub recipe: Recipe,
    pub texts: Vec<String>,
    pub fit: (u32, u32),
    pub scale: f64,
}

/// How often a picture whose fitted scale (output pixels per source pixel, at most 1) is `scale`
/// can be halved and still cover the box.
fn halvings(scale: f64) -> u32 {
    let mut n = 0;
    while n < MAX_HALVINGS && scale.is_finite() && scale > 0.0 && scale * f64::from(1u32 << (n + 1)) <= 1.0 {
        n += 1;
    }
    n
}

/// The extra halvings of a draft scale (½: one, ¼: two).
fn draft_halvings(scale: f64) -> u32 {
    if scale >= 1.0 {
        0
    } else if scale >= 0.5 {
        1
    } else {
        2
    }
}

/// The preview of `ask` from `screen`: the recipe rendered at the largest power-of-two scale of
/// the source that still covers the box (times the draft scale), and its histogram.
pub(crate) fn render(screen: &Screen, ask: &Ask) -> Result<Rendered, String> {
    let started = Instant::now();
    let source = &screen.image;
    let geometry = Geometry::new(&ask.recipe, Orientation::default(), source.width, source.height);
    let (w, h) = geometry.size();
    let (dw, dh) = if geometry.orientation().swaps() { (h, w) } else { (w, h) };
    let fit = (f64::from(ask.fit.0) / dw).min(f64::from(ask.fit.1) / dh).min(1.0);
    let n = halvings(fit).saturating_add(draft_halvings(ask.scale)).min(MAX_HALVINGS);
    let unwarped = (w - f64::from(source.width)).abs() < 1e-9 && (h - f64::from(source.height)).abs() < 1e-9;
    let image = if unwarped {
        let base = screen.base(n);
        if ask.recipe.ops().is_empty() {
            base
        } else {
            Arc::new(dalica_edit::render(dalica_edit::Source::upright(&base), &ask.recipe, Target::Full).map_err(|e| e.to_string())?)
        }
    } else {
        let scale = 1.0 / f64::from(1u32 << n);
        Arc::new(dalica_edit::render(dalica_edit::Source::upright(source), &ask.recipe, Target::Scale(scale)).map_err(|e| e.to_string())?)
    };
    let histogram = Histogram::of(&image);
    Ok(Rendered {
        image,
        histogram: Arc::new(histogram),
        recipe: ask.texts.clone(),
        fit: ask.fit,
        scale: ask.scale,
        ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

/// A window of the edited photo at full resolution.
pub(crate) fn region(full: &Rgba8, recipe: &Recipe, region: Region) -> Result<Rgba8, String> {
    dalica_edit::render_region(dalica_edit::Source::upright(full), recipe, region).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_blocks_and_refuses_what_it_cannot_halve() {
        let image = Rgba8 { width: 3, height: 2, data: (0..24).collect() };
        let half = halve(&image).unwrap();
        assert_eq!((half.width, half.height), (1, 1));
        // Pixels 0, 1 (row 0) and 3, 4 (row 1): channel 0 is 0, 4, 12, 16.
        assert_eq!(half.data, vec![8, 9, 10, 11]);
        assert!(halve(&Rgba8 { width: 1, height: 9, data: vec![0; 36] }).is_none());
        assert!(halve(&Rgba8 { width: 4, height: 4, data: vec![0; 3] }).is_none());
    }

    #[test]
    fn halvings_keep_the_box_covered() {
        assert_eq!(halvings(1.0), 0);
        assert_eq!(halvings(0.9), 0);
        assert_eq!(halvings(0.5), 1);
        assert_eq!(halvings(0.49), 1);
        assert_eq!(halvings(0.25), 2);
        assert_eq!(halvings(f64::NAN), 0);
        assert_eq!(halvings(0.0), 0);
        assert_eq!(halvings(1e-300), MAX_HALVINGS);
    }
}
