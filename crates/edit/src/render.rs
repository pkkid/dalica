//! The CPU renderer (`architecture.md` §6.3): a photo's decoded sRGB pixels and its recipe in, the
//! edited picture out, at any size. It is the oracle for thumbnails, the edit view, Save and Export,
//! and any faster path must match it.
//!
//! - [`render`] gives the whole picture at a [`Target`] size; [`render_region`] gives a window of it
//!   at a scale (a zoomed view), equal to that window of the whole render.
//! - **Pipeline order** (`data-model.md` §3.1, §3.3): the source turned upright by its EXIF
//!   orientation, then the geometry in the stored frame (the last `straighten`, then the last
//!   `crop`, wherever they stand in the recipe) and the scale, as one resample; then every other
//!   known operation in the order applied ([`crate::ops::Adjust`]); then the recipe's rotations and
//!   flips, pixel-exactly. Rotations and flips move pixels without resampling and every stored
//!   coordinate is in the stored frame, so applying them last gives the picture "geometry first"
//!   describes. Legacy and unknown operations, known operations without a module (the effects
//!   until M12) and parameters a module refuses are skipped and kept in the recipe.
//! - **Frames** (M12.13, [`Reframe`], `render/reframe.rs`): an effect that changes the picture's
//!   size (Widescreen Film, Border; Drop Shadow and Museum Matte from M12.14) puts the picture as it
//!   stands at its place among the effects on a canvas of its own size and paints it; the effects
//!   after it work on the canvas. [`Geometry`] knows every frame, so the output's size, the
//!   displayed size and the point maps include them: an effect's stored-frame centre lands on the
//!   photo where the frames before it put it, a later crop still crops the photo and the rotations
//!   and flips turn the framed picture. A region render maps its window back through each frame.
//!   A frame whose canvas would pass the limits at the source's resolution is skipped and kept.
//! - **Resampling:** an output pixel that maps onto exactly one source pixel (the identity, the
//!   EXIF orientations, the rotations and flips at full size) is copied; otherwise each output
//!   pixel averages bilinear taps over its footprint, after a box reduction of the source by the
//!   whole part of the footprint when it covers two or more source pixels. Averaging is on the
//!   sRGB-encoded values, alpha included.
//! - **Working precision:** the operations run on `f32` values (sRGB-encoded, 0..1, alpha apart) in
//!   bands of [`BAND_ROWS`] rows, in parallel through `rayon`. Consecutive operations that read only
//!   their own pixel ([`crate::ops::Reach::Pixel`]) run fused on one band and are rounded to 8 bits
//!   once; an operation that reads its neighbours, or a picture to analyse, starts a new run.
//! - **Analysis proxy:** an operation that analyses the picture ([`crate::ops::Adjust::ANALYSES`])
//!   is given the picture at its place in the pipeline rendered at [`crate::analysis::PROXY_EDGE`],
//!   so every size of render reaches the same analysis.
//! - **Limits:** an output (or a window and its margin) over [`MAX_PIXELS`] or [`MAX_EDGE`], a scale
//!   over [`MAX_SCALE`], and non-finite or empty targets are refused before anything is allocated;
//!   buffers are reserved fallibly.

use dalica_image::{Orientation, Rgba8};
use std::sync::Arc;

use rayon::prelude::*;

use crate::frame::Point;
// `v / 255` for every 8-bit value, as `f32` (the same values the division gives, looked up).
use crate::fx::UNIT8 as UNIT;
use crate::ops::orient::dihedral_to_input;
use crate::ops::{self, Adjust, Affine, Band, Pass, Prepare, Reach, Typed, View, Warp};
use crate::{Dihedral, Error, Recipe, Result};

mod reframe;

pub use reframe::{Layout, Placed, Reframe, pixels};

/// The most pixels one render produces or works on (1 GiB of RGBA).
pub const MAX_PIXELS: u64 = 1 << 28;
/// The longest side of one render's output.
pub const MAX_EDGE: u32 = 1 << 18;
/// The largest scale (output pixels per source pixel) a render accepts.
pub const MAX_SCALE: f64 = 64.0;
/// Rows per band of the operations' working buffers.
pub const BAND_ROWS: usize = 32;
/// The most a footprint is reduced by before sampling (larger reductions sample more taps).
const MAX_REDUCTION: u32 = 4096;
/// The most bilinear taps per output pixel along each side.
const MAX_TAPS: u32 = 4;

/// The decoded pixels of a photo as stored in the file, and its EXIF orientation.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub pixels: &'a Rgba8,
    pub orientation: Orientation,
}

impl<'a> Source<'a> {
    pub fn new(pixels: &'a Rgba8, orientation: Orientation) -> Source<'a> {
        Source { pixels, orientation }
    }

    /// Pixels that are already upright.
    pub fn upright(pixels: &'a Rgba8) -> Source<'a> {
        Source { pixels, orientation: Orientation::default() }
    }
}

/// The size of a whole render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// The picture at the source's resolution (Save, Export at full size).
    Full,
    /// The largest size that fits in `width × height` without enlarging (thumbnails, previews).
    Fit { width: u32, height: u32 },
    /// Output pixels per source pixel (0 < scale ≤ [`MAX_SCALE`]).
    Scale(f64),
}

/// A window of the edited picture at a scale, in the displayed picture's pixels (after the
/// recipe's rotations and flips): `x`, `y` is its top-left pixel. A window reaching past the
/// picture is cut to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub scale: f64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The geometric stage of a recipe for one source: the stored frame (the source after its EXIF
/// orientation), the straighten and the crop in it, the frames that change the picture's size
/// (Widescreen Film, Border, …; [`Reframe`]) where they stand among the effects, and the recipe's
/// rotations and flips. It maps normalised points between the stored frame and the output (for
/// retouch strokes and effect centres, and for the tools' clicks), and gives the output's size.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    exif: Dihedral,
    raw: (u32, u32),
    stored: (f64, f64),
    straighten: Option<ops::straighten::Straighten>,
    crop: Option<ops::crop::Crop>,
    /// The size after straighten and crop, in stored-frame pixels.
    warped: (f64, f64),
    /// From the warped picture's pixel coordinates to the stored frame's.
    to_stored: Affine,
    orientation: Dihedral,
    /// The frames within the limits, in the order applied.
    frames: Arc<[Frame]>,
    /// How many of `frames` the sizes and the point maps include: all of them, but in the geometry
    /// an effect standing before a frame is prepared with.
    shown: usize,
    /// The scale the point maps lay the frames out at, in whole pixels (1: the source's
    /// resolution; a render's own scale for the geometry its effects are prepared with).
    at_scale: f64,
}

/// One frame of a [`Geometry`]: the `nth` size-changing entry of the recipe's effects (counting
/// those past the limits, which are skipped) and its parameters.
#[derive(Clone, Debug, PartialEq)]
struct Frame {
    nth: usize,
    op: Typed,
}

/// The picture's size at each frame of one render: the photo (after the geometry), then each
/// frame's input and layout in order.
struct Stages {
    photo: (u32, u32),
    frames: Vec<((u32, u32), Layout)>,
}

impl Stages {
    /// The picture's size after `stage` frames.
    fn size(&self, stage: usize) -> (u32, u32) {
        match stage.checked_sub(1) {
            None => self.photo,
            Some(k) => self.frames.get(k).map_or(self.photo, |(_, l)| l.size),
        }
    }

    fn last(&self) -> (u32, u32) {
        self.size(self.frames.len())
    }
}

impl Geometry {
    /// The geometry of `recipe` on a source of `width × height` stored pixels with EXIF orientation
    /// `exif`. A warp whose module gives no usable size or map is skipped, and so is a frame whose
    /// canvas would pass [`MAX_EDGE`] or [`MAX_PIXELS`] at the source's resolution.
    pub fn new(recipe: &Recipe, exif: Orientation, width: u32, height: u32) -> Geometry {
        let exif = Dihedral::from_exif(exif);
        let (sw, sh) = exif.display_size(width, height);
        let stored = (f64::from(sw), f64::from(sh));
        let (mut straightens, mut crops) = (Vec::new(), Vec::new());
        for op in recipe.pipeline() {
            match op.typed() {
                Some(Ok(Typed::Straighten(s))) => straightens.push(s),
                Some(Ok(Typed::Crop(c))) => crops.push(c),
                _ => {}
            }
        }
        let straighten = ops::straighten::combine(&straightens);
        let crop = ops::crop::combine(&crops);
        let mut g = Geometry {
            exif,
            raw: (width, height),
            stored,
            straighten: None,
            crop: None,
            warped: stored,
            to_stored: Affine::IDENTITY,
            orientation: recipe.orientation(),
            frames: Arc::new([]),
            shown: 0,
            at_scale: 1.0,
        };
        if let Some(s) = straighten
            && g.push_warp(&s)
        {
            g.straighten = Some(s);
        }
        if let Some(c) = crop
            && g.push_warp(&c)
        {
            g.crop = Some(c);
        }
        // The frames, laid out at the source's resolution: one past the limits is skipped.
        let mut frames = Vec::new();
        let mut size = g.photo_pixels(1.0).ok();
        for (nth, t) in frame_ops(recipe).enumerate() {
            let Some(input) = size else { break };
            let layout = reframe_of(&t).and_then(|f| f.layout(input.0, input.1));
            if let Some(l) = layout.filter(|l| within_limits(l.size)) {
                frames.push(Frame { nth, op: t });
                size = Some(l.size);
            }
        }
        g.shown = frames.len();
        g.frames = frames.into();
        g
    }

    /// Adds a warp after the ones already there; false (and nothing changed) when its size or map
    /// is unusable.
    fn push_warp<W: Warp>(&mut self, w: &W) -> bool {
        let (iw, ih) = self.warped;
        let (ow, oh) = w.output_size(iw, ih);
        let map = self.to_stored.then_after(w.to_input(iw, ih));
        let usable = ow.is_finite() && oh.is_finite() && ow >= 1e-6 && oh >= 1e-6 && map.is_finite() && map.inverse().is_some();
        if usable {
            self.warped = (ow, oh);
            self.to_stored = map;
        }
        usable
    }

    /// The size after straighten, crop and the frames, before the rotations and flips, at the
    /// source's resolution (stored-frame pixels; whole pixels once a frame is in).
    pub fn size(&self) -> (f64, f64) {
        if self.shown == 0 {
            return self.warped;
        }
        match self.stages(1.0) {
            Ok(s) => {
                let (w, h) = s.last();
                (f64::from(w), f64::from(h))
            }
            Err(_) => self.warped,
        }
    }

    /// The photo's size after straighten and crop, before any frame, in stored-frame pixels.
    pub fn photo_size(&self) -> (f64, f64) {
        self.warped
    }

    /// How many frames (effects that change the picture's size) the geometry includes.
    pub fn frames(&self) -> usize {
        self.shown
    }

    /// The recipe's rotations and flips.
    pub fn orientation(&self) -> Dihedral {
        self.orientation
    }

    /// The photo after straighten and crop at `scale`, in whole output pixels (each side at least
    /// 1 and at most [`MAX_EDGE`]).
    fn photo_pixels(&self, scale: f64) -> Result<(u32, u32)> {
        let side = |v: f64| -> Result<u32> {
            let px = (v * scale).round().max(1.0);
            if !px.is_finite() || px > f64::from(MAX_EDGE) {
                return Err(Error::TooLarge { width: saturate(self.warped.0 * scale), height: saturate(self.warped.1 * scale) });
            }
            Ok(px as u32)
        };
        Ok((side(self.warped.0)?, side(self.warped.1)?))
    }

    /// The picture's size at each shown frame at `scale`.
    fn stages(&self, scale: f64) -> Result<Stages> {
        let photo = self.photo_pixels(scale)?;
        let mut frames = Vec::new();
        let mut size = photo;
        for f in self.frames.iter().take(self.shown) {
            let too_large = || Error::TooLarge { width: u64::from(size.0), height: u64::from(size.1) };
            let layout = reframe_of(&f.op).and_then(|r| r.layout(size.0, size.1)).ok_or_else(too_large)?;
            let (w, h) = layout.size;
            if w == 0 || h == 0 || w > MAX_EDGE || h > MAX_EDGE {
                return Err(Error::TooLarge { width: u64::from(w), height: u64::from(h) });
            }
            frames.push((size, layout));
            size = layout.size;
        }
        Ok(Stages { photo, frames })
    }

    /// The output size at `scale` before the rotations and flips (each side at most [`MAX_EDGE`]).
    pub fn output_size(&self, scale: f64) -> Result<(u32, u32)> {
        // Only the window a render works on is allocated, and checked against `MAX_PIXELS`.
        Ok(self.stages(scale)?.last())
    }

    /// The displayed size at `scale`.
    pub fn display_size(&self, scale: f64) -> Result<(u32, u32)> {
        let (w, h) = self.output_size(scale)?;
        Ok(self.orientation.display_size(w, h))
    }

    /// Where the normalised stored-frame point `p` lands in the output before the rotations and
    /// flips, normalised (on the photo where the frames put it).
    pub fn stored_to_output(&self, p: Point) -> Option<Point> {
        let (x, y) = self.to_stored.inverse()?.apply(p.x * self.stored.0, p.y * self.stored.1);
        self.photo_to_output(finite(Point::new(x / self.warped.0, y / self.warped.1))?)
    }

    /// The normalised stored-frame point under the normalised output point `p` (before the
    /// rotations and flips); a point on a frame gives a point outside the photo.
    pub fn output_to_stored(&self, p: Point) -> Option<Point> {
        let p = self.output_to_photo(p)?;
        let (x, y) = self.to_stored.apply(p.x * self.warped.0, p.y * self.warped.1);
        finite(Point::new(x / self.stored.0, y / self.stored.1))
    }

    /// Where the photo (after straighten and crop) lies in the output before the rotations and
    /// flips, normalised: the whole output until a frame is in, a part of it inside a Border, and
    /// past its edges where Widescreen Film crops it.
    pub fn photo_area(&self) -> Option<crate::Rect> {
        let a = self.photo_to_output(Point::new(0.0, 0.0))?;
        let b = self.photo_to_output(Point::new(1.0, 1.0))?;
        Some(crate::Rect::new(a.x, a.y, b.x, b.y))
    }

    /// A normalised point of the photo (after straighten and crop) through the shown frames.
    fn photo_to_output(&self, p: Point) -> Option<Point> {
        if self.shown == 0 {
            return Some(p);
        }
        let stages = self.stages(self.at_scale).ok()?;
        let mut q = p;
        for ((iw, ih), l) in &stages.frames {
            let x = (q.x * f64::from(*iw) + l.at.0 as f64) / f64::from(l.size.0);
            let y = (q.y * f64::from(*ih) + l.at.1 as f64) / f64::from(l.size.1);
            q = Point::new(x, y);
        }
        finite(q)
    }

    /// A normalised output point back through the shown frames onto the photo.
    fn output_to_photo(&self, p: Point) -> Option<Point> {
        if self.shown == 0 {
            return Some(p);
        }
        let stages = self.stages(self.at_scale).ok()?;
        let mut q = p;
        for ((iw, ih), l) in stages.frames.iter().rev() {
            let x = (q.x * f64::from(l.size.0) - l.at.0 as f64) / f64::from(*iw);
            let y = (q.y * f64::from(l.size.1) - l.at.1 as f64) / f64::from(*ih);
            q = Point::new(x, y);
        }
        finite(q)
    }

    /// Where the normalised stored-frame point `p` shows on the displayed picture.
    pub fn to_display(&self, p: Point) -> Option<Point> {
        Some(self.stored_to_output(p)?.transform(self.orientation))
    }

    /// The normalised stored-frame point under the normalised displayed point `p`.
    pub fn to_stored(&self, p: Point) -> Option<Point> {
        self.output_to_stored(p.transform(self.orientation.inverse()))
    }

    /// The same geometry with only its first `frames` frames, its point maps laid out at `scale`:
    /// what an effect standing before the next frame is prepared with.
    fn staged(&self, frames: usize, scale: f64) -> Geometry {
        Geometry { shown: frames.min(self.frames.len()), at_scale: scale, ..self.clone() }
    }

    /// The scale a target asks for.
    fn scale_for(&self, target: Target) -> Result<f64> {
        let size = self.size();
        let (dw, dh) = if self.orientation.swaps() { (size.1, size.0) } else { size };
        let scale = match target {
            Target::Full => 1.0,
            Target::Scale(s) => s,
            Target::Fit { width, height } => {
                if width == 0 || height == 0 {
                    return Err(Error::BadTarget("an empty box to fit"));
                }
                let mut s = (f64::from(width) / dw).min(f64::from(height) / dh).min(1.0);
                // A frame's bands are whole pixels at every scale, so a framed picture can come out
                // a pixel larger than its share: shrink until it fits.
                for _ in 0..8 {
                    if self.shown == 0 || check_scale(s).is_err() {
                        break;
                    }
                    let (w, h) = self.display_size(s)?;
                    if w <= width && h <= height {
                        break;
                    }
                    s *= (f64::from(width) / f64::from(w)).min(f64::from(height) / f64::from(h)) * (1.0 - 1e-6);
                }
                s
            }
        };
        check_scale(scale)?;
        Ok(scale)
    }

    /// The map from output pixel coordinates at `scale` (before the frames and the rotations and
    /// flips) to the source's stored pixels.
    fn output_to_raw(&self, scale_x: f64, scale_y: f64) -> Affine {
        let to_raw = dihedral_to_input(self.exif, f64::from(self.raw.0), f64::from(self.raw.1));
        to_raw.then_after(self.to_stored).then_after(Affine::scale(1.0 / scale_x, 1.0 / scale_y))
    }
}

/// The recipe's size-changing entries that render, in the order applied.
fn frame_ops(recipe: &Recipe) -> impl Iterator<Item = Typed> + '_ {
    recipe.pipeline().filter_map(|op| op.typed()?.ok()).filter(|t| reframe_of(t).is_some())
}

/// Whether a canvas of `size` is within [`MAX_EDGE`] and [`MAX_PIXELS`].
fn within_limits(size: (u32, u32)) -> bool {
    let (w, h) = size;
    w >= 1 && h >= 1 && w <= MAX_EDGE && h <= MAX_EDGE && u64::from(w) * u64::from(h) <= MAX_PIXELS
}

fn finite(p: Point) -> Option<Point> {
    (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

fn saturate(v: f64) -> u64 {
    if v.is_finite() && v > 0.0 { v.min(u64::MAX as f64) as u64 } else { 0 }
}

fn check_scale(scale: f64) -> Result<()> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(Error::BadTarget("a scale must be finite and positive"));
    }
    if scale > MAX_SCALE {
        return Err(Error::BadTarget("a scale over the most a render enlarges"));
    }
    Ok(())
}

/// The whole edited picture at `target`.
pub fn render(source: Source<'_>, recipe: &Recipe, target: Target) -> Result<Rgba8> {
    check_source(source.pixels)?;
    let geometry = Geometry::new(recipe, source.orientation, source.pixels.width, source.pixels.height);
    if source.pixels.width == 0 || source.pixels.height == 0 {
        // Nothing to resample: the empty picture with its displayed size.
        return geometry.exif.then(geometry.orientation).apply(source.pixels.clone());
    }
    let scale = geometry.scale_for(target)?;
    let (width, height) = geometry.display_size(scale)?;
    render_region(source, recipe, Region { scale, x: 0, y: 0, width, height })
}

/// A window of the edited picture at a scale (a zoomed view): the same pixels as that window of
/// [`render`] at `Target::Scale(region.scale)`.
pub fn render_region(source: Source<'_>, recipe: &Recipe, region: Region) -> Result<Rgba8> {
    check_source(source.pixels)?;
    check_scale(region.scale)?;
    if source.pixels.width == 0 || source.pixels.height == 0 {
        return Err(Error::BadTarget("an empty picture has no region"));
    }
    let plan = Plan::new(source, recipe);
    let (fw, fh) = plan.geometry.output_size(region.scale)?;
    let orientation = plan.geometry.orientation;
    let (dw, dh) = orientation.display_size(fw, fh);
    // The window cut to the picture, in displayed pixels, then in output pixels before the turn.
    let x1 = u64::from(region.x).saturating_add(u64::from(region.width)).min(u64::from(dw));
    let y1 = u64::from(region.y).saturating_add(u64::from(region.height)).min(u64::from(dh));
    if u64::from(region.x) >= x1 || u64::from(region.y) >= y1 {
        return Err(Error::BadTarget("the region is outside the picture"));
    }
    let (x1, y1) = (x1 as u32 - 1, y1 as u32 - 1);
    let back = orientation.inverse();
    let (ax, ay) = back.map_pixel(region.x, region.y, dw, dh);
    let (bx, by) = back.map_pixel(x1, y1, dw, dh);
    let win = Win { x0: ax.min(bx), y0: ay.min(by), w: ax.abs_diff(bx) + 1, h: ay.abs_diff(by) + 1 };
    let out = plan.render_stored(plan.steps.len(), region.scale, win, false)?;
    orientation.apply(out)
}

fn check_source(img: &Rgba8) -> Result<()> {
    let expected = (img.width as usize).checked_mul(img.height as usize).and_then(|n| n.checked_mul(4));
    if expected != Some(img.data.len()) {
        return Err(Error::BadImage { width: img.width, height: img.height, len: img.data.len() });
    }
    Ok(())
}

/// A window of the output before the rotations and flips, in its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Win {
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
}

impl Win {
    fn pixels(&self) -> u64 {
        u64::from(self.w) * u64::from(self.h)
    }

    /// The window grown by `margin` on every side, cut to a `full` picture.
    fn grown(self, margin: u32, full: (u32, u32)) -> Win {
        let x0 = self.x0.saturating_sub(margin);
        let y0 = self.y0.saturating_sub(margin);
        let x1 = self.x0.saturating_add(self.w).saturating_add(margin).min(full.0);
        let y1 = self.y0.saturating_add(self.h).saturating_add(margin).min(full.1);
        Win { x0, y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
    }

    /// The window `self` of a frame's canvas back on the frame's `input` (the part of it the input
    /// covers), in the input's pixels; `None` when the input lands nowhere in it.
    fn back_through(self, layout: Layout, input: (u32, u32)) -> Option<Win> {
        let x0 = (i64::from(self.x0) - layout.at.0).max(0);
        let y0 = (i64::from(self.y0) - layout.at.1).max(0);
        let x1 = (i64::from(self.x0) + i64::from(self.w) - layout.at.0).min(i64::from(input.0));
        let y1 = (i64::from(self.y0) + i64::from(self.h) - layout.at.1).min(i64::from(input.1));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let px = |v: i64| u32::try_from(v).ok();
        Some(Win { x0: px(x0)?, y0: px(y0)?, w: px(x1 - x0)?, h: px(y1 - y0)? })
    }
}

/// A recipe's render for one source: the geometry and the other operations, typed, in order.
struct Plan<'a> {
    source: Source<'a>,
    geometry: Geometry,
    steps: Vec<Step>,
}

/// One step of the effects stage: an operation run over the picture, or a frame that puts the
/// picture on a canvas of its own size (the `k`th of the geometry's frames).
enum Step {
    Adjust(Typed),
    Frame(usize),
}

/// One operation of the adjust stage, prepared or still to be prepared on the picture at its place.
enum Slot<'t> {
    Ready(Option<Box<dyn Pass>>),
    OnProxy(&'t Typed),
}

/// The operations between two frames (or before the first, or after the last): they run on the
/// picture of one size, the picture after `stage` frames. A segment after a frame starts with the
/// frame's own pass.
struct Segment<'t> {
    stage: usize,
    slots: Vec<Slot<'t>>,
}

impl<'a> Plan<'a> {
    fn new(source: Source<'a>, recipe: &Recipe) -> Plan<'a> {
        let geometry = Geometry::new(recipe, source.orientation, source.pixels.width, source.pixels.height);
        let mut steps = Vec::new();
        let (mut nth, mut next) = (0, 0);
        for t in recipe.pipeline().filter_map(|op| op.typed()?.ok()).filter(|t| analyses(t).is_some()) {
            if reframe_of(&t).is_none() {
                steps.push(Step::Adjust(t));
                continue;
            }
            // A frame past the limits is skipped (and kept in the recipe).
            if geometry.frames.get(next).is_some_and(|f| f.nth == nth) {
                steps.push(Step::Frame(next));
                next += 1;
            }
            nth += 1;
        }
        Plan { source, geometry, steps }
    }

    /// The proxy scale of the analysing operations.
    fn proxy_scale(&self) -> f64 {
        let long = self.geometry.warped.0.max(self.geometry.warped.1);
        (f64::from(crate::analysis::PROXY_EDGE) / long).min(1.0)
    }

    /// The picture after the geometry and the first `n` steps, at `scale`, cut to `win` (in the
    /// picture of that stage, before the rotations and flips). In a proxy render (`proxy`, always
    /// the whole picture at the proxy scale) an analysing operation analyses the picture as it
    /// stands.
    fn render_stored(&self, n: usize, scale: f64, win: Win, proxy: bool) -> Result<Rgba8> {
        let stages = self.geometry.stages(scale)?;
        // The geometry each stage's operations are prepared with: the frames before them.
        let geos: Vec<Geometry> = (0..=stages.frames.len()).map(|k| self.geometry.staged(k, scale)).collect();
        let geo = |stage: usize| geos.get(stage).unwrap_or(&self.geometry);
        let mut segs = vec![Segment { stage: 0, slots: Vec::new() }];
        for (i, step) in self.steps.iter().take(n).enumerate() {
            let stage = segs.last().map_or(0, |s| s.stage);
            match step {
                Step::Adjust(t) => {
                    let full = stages.size(stage);
                    let ctx = Prepare { width: full.0, height: full.1, scale, proxy: None, geometry: geo(stage) };
                    let wants_proxy = analyses(t).unwrap_or(false);
                    let slot = if wants_proxy && proxy {
                        Slot::OnProxy(t)
                    } else if wants_proxy {
                        // The picture at this operation's place, at the analysis size.
                        let pscale = self.proxy_scale();
                        let pfull = self.geometry.stages(pscale)?.size(stage);
                        let img = self.render_stored(i, pscale, Win { x0: 0, y0: 0, w: pfull.0, h: pfull.1 }, true)?;
                        Slot::Ready(prepare(t, &Prepare { proxy: Some(&img), ..ctx }))
                    } else {
                        Slot::Ready(prepare(t, &ctx))
                    };
                    if let Some(seg) = segs.last_mut() {
                        seg.slots.push(slot);
                    }
                }
                Step::Frame(k) => {
                    let stage = k.saturating_add(1);
                    let Some(&(input, layout)) = stages.frames.get(*k) else { continue };
                    let ctx = Prepare { width: layout.size.0, height: layout.size.1, scale, proxy: None, geometry: geo(stage) };
                    let placed = Placed { ctx: &ctx, input, layout };
                    let pass = self.geometry.frames.get(*k).and_then(|f| reframe_of(&f.op)).and_then(|f| f.prepare(&placed));
                    segs.push(Segment { stage, slots: vec![Slot::Ready(pass)] });
                }
            }
        }
        // From the last segment back: the window each one works on (with the margin its operations
        // read) and the part of the frame's input that lands in it.
        let mut grown: Vec<Option<Win>> = vec![None; segs.len()];
        let mut need = Some(win);
        for (j, seg) in segs.iter().enumerate().rev() {
            let Some(w) = need else { break };
            let g = w.grown(margin(&seg.slots), stages.size(seg.stage));
            if g.pixels() > MAX_PIXELS {
                return Err(Error::TooLarge { width: u64::from(g.w), height: u64::from(g.h) });
            }
            if let Some(slot) = grown.get_mut(j) {
                *slot = Some(g);
            }
            need = match seg.stage.checked_sub(1).and_then(|k| stages.frames.get(k)) {
                Some(&(input, layout)) => g.back_through(layout, input),
                None => None,
            };
        }
        // Then forward: the geometric stage, each segment's operations, each frame's canvas.
        let mut picture: Option<(Rgba8, Win)> = None;
        for (seg, g) in segs.into_iter().zip(grown) {
            let Some(g) = g else { continue };
            let full = stages.size(seg.stage);
            let mut buf = match seg.stage.checked_sub(1).and_then(|k| stages.frames.get(k)) {
                Some(&(input, layout)) => place(picture.take(), input, layout, g)?,
                None => {
                    // The output's own scale on each axis (its sides are whole pixels).
                    let (ww, wh) = self.geometry.warped;
                    let map = self.geometry.output_to_raw(f64::from(full.0) / ww, f64::from(full.1) / wh);
                    resample(self.source.pixels, map, g)?
                }
            };
            let geometry = geo(seg.stage);
            run_slots(&mut buf, g, full, seg.slots, |t, img| {
                prepare(t, &Prepare { width: full.0, height: full.1, scale, proxy: Some(img), geometry })
            })?;
            picture = Some((buf, g));
        }
        let (buf, from) = picture.ok_or(Error::BadTarget("nothing to render"))?;
        cut(buf, from, win)
    }
}

/// How far a segment's operations read beyond their window.
fn margin(slots: &[Slot<'_>]) -> u32 {
    slots.iter().fold(0u32, |margin, slot| match slot {
        Slot::Ready(Some(pass)) => match pass.reach() {
            Reach::Pixel => margin,
            Reach::Pixels(r) => margin.saturating_add(r),
            Reach::Whole => u32::MAX,
        },
        Slot::Ready(None) => margin,
        Slot::OnProxy(_) => u32::MAX,
    })
}

/// The window `win` of a frame's canvas with the frame's input placed on it: the input's pixels
/// where it lands (from `picture`, the input rendered over a window that holds every one of them
/// the canvas window shows), opaque black elsewhere, for the frame's pass to paint. Each byte is
/// written once (M13.47): a row's columns before and after the input black, the input's copied.
fn place(picture: Option<(Rgba8, Win)>, input: (u32, u32), layout: Layout, win: Win) -> Result<Rgba8> {
    let mut data = alloc(win.pixels())?;
    let row = win.w as usize * 4;
    // One row of opaque black, the source of every black run.
    let mut black = alloc(u64::from(win.w))?;
    black.extend(std::iter::repeat_n([0, 0, 0, 255], win.w as usize).flatten());
    let Some((src, from)) = picture else {
        for _ in 0..win.h {
            data.extend_from_slice(&black);
        }
        return Ok(Rgba8 { width: win.w, height: win.h, data });
    };
    // The canvas columns the input covers, cut to the window and to the rendered input.
    let lo = (i64::from(from.x0) + layout.at.0).max(i64::from(win.x0)).max(layout.at.0);
    let hi = (i64::from(from.x0) + i64::from(from.w) + layout.at.0).min(i64::from(win.x0) + i64::from(win.w)).min(layout.at.0 + i64::from(input.0));
    let src_row = from.w as usize * 4;
    let (dst_x, len, src_x) = if lo < hi {
        ((lo - i64::from(win.x0)) as usize * 4, (hi - lo) as usize * 4, (lo - layout.at.0 - i64::from(from.x0)) as usize * 4)
    } else {
        (0, 0, 0)
    };
    for j in 0..i64::from(win.h) {
        let iy = i64::from(win.y0) + j - layout.at.1;
        let inside = len > 0 && iy >= i64::from(from.y0) && iy < i64::from(from.y0) + i64::from(from.h) && iy < i64::from(input.1) && iy >= 0;
        let at = if inside { (iy - i64::from(from.y0)) as usize * src_row + src_x } else { 0 };
        match src.data.get(at..at + len).filter(|_| inside) {
            Some(s) => {
                data.extend_from_slice(black.get(..dst_x).unwrap_or(&[]));
                data.extend_from_slice(s);
                data.extend_from_slice(black.get(..row.saturating_sub(dst_x + len)).unwrap_or(&[]));
            }
            None => data.extend_from_slice(&black),
        }
    }
    Ok(Rgba8 { width: win.w, height: win.h, data })
}

/// The frame kind of a typed operation: the effects that change the picture's size
/// ([`Reframe`]), which the `Adjust` dispatch below leaves out.
fn reframe_of(t: &Typed) -> Option<&dyn Reframe> {
    match t {
        Typed::Widescreen(o) => Some(o),
        Typed::Border(o) => Some(o),
        Typed::DropShadow(o) => Some(o),
        Typed::MuseumMatte(o) => Some(o),
        _ => None,
    }
}

/// Whether an adjust operation analyses the picture; `None` for the geometric operations.
fn analyses(t: &Typed) -> Option<bool> {
    fn of<T: Adjust>(_: &T) -> Option<bool> {
        Some(T::ANALYSES)
    }
    match t {
        Typed::Rotate(_) | Typed::FlipH(_) | Typed::FlipV(_) | Typed::Crop(_) | Typed::Straighten(_) => None,
        Typed::Retouch(o) => of(o),
        Typed::AutoFix(o) => of(o),
        Typed::AutoContrast(o) => of(o),
        Typed::AutoColor(o) => of(o),
        Typed::FillLight(o) => of(o),
        Typed::Tuning(o) => of(o),
        // The effects (M12.1; stubs until their renderers land, M12.6–M12.14).
        Typed::Sharpen(o) => of(o),
        Typed::Sepia(o) => of(o),
        Typed::Bw(o) => of(o),
        Typed::Warmify(o) => of(o),
        Typed::Grain(o) => of(o),
        Typed::Tint(o) => of(o),
        Typed::Saturation(o) => of(o),
        Typed::SoftFocus(o) => of(o),
        Typed::Glow(o) => of(o),
        Typed::FilteredBw(o) => of(o),
        Typed::FocalBw(o) => of(o),
        Typed::GraduatedTint(o) => of(o),
        Typed::Infrared(o) => of(o),
        Typed::PlasticLens(o) => of(o),
        Typed::Hdr(o) => of(o),
        // The frames run through `reframe_of`, not here.
        Typed::Widescreen(_) => Some(false),
        Typed::DreamGlow(o) => of(o),
        Typed::Invert(o) => of(o),
        Typed::Duotone(o) => of(o),
        Typed::Boost(o) => of(o),
        Typed::Soften(o) => of(o),
        Typed::Vignette(o) => of(o),
        Typed::Pixelate(o) => of(o),
        Typed::FocalZoom(o) => of(o),
        Typed::Border(_) => Some(false),
        Typed::DropShadow(_) | Typed::MuseumMatte(_) => Some(false),
    }
}

fn prepare(t: &Typed, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
    match t {
        Typed::Rotate(_) | Typed::FlipH(_) | Typed::FlipV(_) | Typed::Crop(_) | Typed::Straighten(_) => None,
        Typed::Retouch(o) => o.prepare(ctx),
        Typed::AutoFix(o) => o.prepare(ctx),
        Typed::AutoContrast(o) => o.prepare(ctx),
        Typed::AutoColor(o) => o.prepare(ctx),
        Typed::FillLight(o) => o.prepare(ctx),
        Typed::Tuning(o) => o.prepare(ctx),
        // The effects (M12.1; stubs until their renderers land, M12.6–M12.14).
        Typed::Sharpen(o) => o.prepare(ctx),
        Typed::Sepia(o) => o.prepare(ctx),
        Typed::Bw(o) => o.prepare(ctx),
        Typed::Warmify(o) => o.prepare(ctx),
        Typed::Grain(o) => o.prepare(ctx),
        Typed::Tint(o) => o.prepare(ctx),
        Typed::Saturation(o) => o.prepare(ctx),
        Typed::SoftFocus(o) => o.prepare(ctx),
        Typed::Glow(o) => o.prepare(ctx),
        Typed::FilteredBw(o) => o.prepare(ctx),
        Typed::FocalBw(o) => o.prepare(ctx),
        Typed::GraduatedTint(o) => o.prepare(ctx),
        Typed::Infrared(o) => o.prepare(ctx),
        Typed::PlasticLens(o) => o.prepare(ctx),
        Typed::Hdr(o) => o.prepare(ctx),
        Typed::Widescreen(_) => None,
        Typed::DreamGlow(o) => o.prepare(ctx),
        Typed::Invert(o) => o.prepare(ctx),
        Typed::Duotone(o) => o.prepare(ctx),
        Typed::Boost(o) => o.prepare(ctx),
        Typed::Soften(o) => o.prepare(ctx),
        Typed::Vignette(o) => o.prepare(ctx),
        Typed::Pixelate(o) => o.prepare(ctx),
        Typed::FocalZoom(o) => o.prepare(ctx),
        Typed::Border(_) => None,
        Typed::DropShadow(_) | Typed::MuseumMatte(_) => None,
    }
}

/// Runs the adjust stage over `buf` (the window `win` of a `full` output): consecutive
/// [`Reach::Pixel`] passes fused per band, every other pass on its own over the picture as it was
/// before it. `on_proxy` prepares an operation on the picture as it stands (proxy renders).
fn run_slots<'t>(
    buf: &mut Rgba8,
    win: Win,
    full: (u32, u32),
    slots: Vec<Slot<'t>>,
    on_proxy: impl Fn(&'t Typed, &Rgba8) -> Option<Box<dyn Pass>>,
) -> Result<()> {
    let mut fused: Vec<Box<dyn Pass>> = Vec::new();
    for slot in slots {
        let pass = match slot {
            Slot::Ready(pass) => pass,
            Slot::OnProxy(t) => {
                run_fused(buf, win, &fused);
                fused.clear();
                on_proxy(t, buf)
            }
        };
        let Some(pass) = pass else { continue };
        if pass.reach() == Reach::Pixel {
            fused.push(pass);
        } else {
            run_fused(buf, win, &fused);
            fused.clear();
            run_reaching(buf, win, full, pass.as_ref())?;
        }
    }
    run_fused(buf, win, &fused);
    Ok(())
}

/// `v × 255` rounded half away from zero and clamped to 0…255 (NaN gives 0), without a branch or
/// a float-to-integer conversion, so a band's values convert in a loop the compiler vectorises
/// (M13.47; `f32::round` is a library call on the baseline x86-64 target and `as` saturates, both
/// in the way). The value is clamped first (`NaN` and below 0 as 0, past 255 as 255: two selects),
/// then `v + 2²³` rounds it to the nearest whole number, ties to even, held in the float's low
/// mantissa bits (its spacing there is 1); a tie that went down to even goes up instead (`v − r`
/// is exactly ½ only for a tie). The same value as `round`, exactly.
#[inline]
fn to_u8(v: f32) -> u8 {
    const SHIFT: f32 = 8_388_608.0;
    let v = v * 255.0;
    // `v > 0` is false for `NaN`: both selects are the compiler's `max` and `min`.
    let v = if v > 0.0 { v } else { 0.0 };
    let v = if v < 255.0 { v } else { 255.0 };
    let t = v + SHIFT;
    let up = u32::from(v - (t - SHIFT) == 0.5);
    (t.to_bits().wrapping_add(up) & 0xff) as u8
}

/// The rounding `to_u8` replaced, kept as the reference its test compares with.
#[cfg(test)]
fn to_u8_reference(v: f32) -> u8 {
    let v = (v * 255.0).round();
    if v >= 255.0 {
        255
    } else if v > 0.0 {
        v as u8
    } else {
        0
    }
}

/// Runs `passes` over `buf` band by band, each band read into working precision once.
fn run_fused(buf: &mut Rgba8, win: Win, passes: &[Box<dyn Pass>]) {
    // No pass: the 8-bit values go to working precision and back unchanged.
    if passes.is_empty() {
        return;
    }
    run_bands(buf, win, None, |band| {
        for pass in passes {
            pass.run(band);
        }
    });
}

/// Runs one pass that reads beyond its pixel, over the picture as it was before it.
fn run_reaching(buf: &mut Rgba8, win: Win, full: (u32, u32), pass: &dyn Pass) -> Result<()> {
    let mut before = Vec::new();
    before.try_reserve_exact(buf.data.len()).map_err(|_| Error::TooLarge { width: u64::from(buf.width), height: u64::from(buf.height) })?;
    before.extend_from_slice(&buf.data);
    let view = View { data: &before, x0: win.x0, y0: win.y0, width: win.w, height: win.h, full_width: full.0, full_height: full.1 };
    run_bands(buf, win, Some(&view), |band| pass.run(band));
    Ok(())
}

fn run_bands(buf: &mut Rgba8, win: Win, before: Option<&View<'_>>, f: impl Fn(&mut Band<'_>) + Sync) {
    let row = buf.width as usize * 4;
    if row == 0 || buf.data.is_empty() {
        return;
    }
    let width = buf.width;
    // One working buffer per piece of rayon's work, reused band after band (a band of a 3000-pixel
    // row is over a megabyte: made per band, each one was fresh pages to fault in).
    buf.data.par_chunks_mut(row * BAND_ROWS).enumerate().for_each_init(Vec::new, |pixels, (i, chunk)| {
        to_working(chunk, pixels);
        let rows = (chunk.len() / row) as u32;
        let y0 = win.y0.saturating_add(u32::try_from(i * BAND_ROWS).unwrap_or(u32::MAX));
        let mut band = Band { pixels, width, rows, x0: win.x0, y0, before };
        f(&mut band);
        to_bytes(pixels, chunk);
    });
}

/// The RGBA8 pixels `bytes` in working values into `pixels` (made their length), alpha apart.
fn to_working(bytes: &[u8], pixels: &mut Vec<[f32; 3]>) {
    let px = bytes.as_chunks::<4>().0;
    pixels.clear();
    pixels.extend(px.iter().map(|p| [UNIT[usize::from(p[0])], UNIT[usize::from(p[1])], UNIT[usize::from(p[2])]]));
}

/// Working values `pixels` rounded back into the RGBA8 pixels `bytes`, their alpha kept.
fn to_bytes(pixels: &[[f32; 3]], bytes: &mut [u8]) {
    for (dst, src) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
        *dst = [to_u8(src[0]), to_u8(src[1]), to_u8(src[2]), dst[3]];
    }
}

/// The window `win` of `buf`, which holds the window `from` of the same picture.
fn cut(buf: Rgba8, from: Win, win: Win) -> Result<Rgba8> {
    if from == win {
        return Ok(buf);
    }
    let (dx, dy) = (win.x0.saturating_sub(from.x0) as usize, win.y0.saturating_sub(from.y0) as usize);
    let mut data = alloc(win.pixels())?;
    let src_row = from.w as usize * 4;
    let row = win.w as usize * 4;
    for y in 0..win.h as usize {
        let at = (dy + y) * src_row + dx * 4;
        if let Some(src) = buf.data.get(at..at + row) {
            data.extend_from_slice(src);
        }
    }
    Ok(Rgba8 { width: win.w, height: win.h, data })
}

/// An empty buffer with room for `pixels` RGBA pixels, reserved fallibly.
fn alloc(pixels: u64) -> Result<Vec<u8>> {
    let too_large = || Error::TooLarge { width: pixels, height: 1 };
    if pixels > MAX_PIXELS {
        return Err(too_large());
    }
    let bytes = usize::try_from(pixels * 4).map_err(|_| too_large())?;
    let mut v = Vec::new();
    v.try_reserve_exact(bytes).map_err(|_| too_large())?;
    Ok(v)
}

/// Read access to a plane of pixels with the nearest edge pixel outside it.
trait Plane: Sync {
    fn px(&self, x: i64, y: i64) -> [f32; 4];

    /// The pixels `(x, y)`, `(x + 1, y)`, `(x, y + 1)`, `(x + 1, y + 1)`.
    fn quad(&self, x: i64, y: i64) -> [[f32; 4]; 4] {
        let (jx, jy) = (x.saturating_add(1), y.saturating_add(1));
        [self.px(x, y), self.px(jx, y), self.px(x, jy), self.px(jx, jy)]
    }
}

/// An RGBA image read as a plane.
struct Raw<'a>(&'a Rgba8);

impl Plane for Raw<'_> {
    fn px(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.min(i64::from(self.0.width) - 1).max(0) as usize;
        let y = y.min(i64::from(self.0.height) - 1).max(0) as usize;
        let at = (y * self.0.width as usize + x) * 4;
        match self.0.data.get(at..at + 4) {
            Some(p) => [f32::from(p[0]), f32::from(p[1]), f32::from(p[2]), f32::from(p[3])],
            None => [0.0; 4],
        }
    }
}

/// A box reduction of a source by `factor`: a window of the reduced picture.
struct Reduced {
    data: Vec<[f32; 4]>,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    full: (u32, u32),
}

impl Plane for Reduced {
    fn px(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.clamp(0, i64::from(self.full.0) - 1);
        let y = y.clamp(0, i64::from(self.full.1) - 1);
        let lx = (x - i64::from(self.x0)).clamp(0, i64::from(self.w) - 1) as usize;
        let ly = (y - i64::from(self.y0)).clamp(0, i64::from(self.h) - 1) as usize;
        self.data.get(ly * self.w as usize + lx).copied().unwrap_or([0.0; 4])
    }

    fn quad(&self, x: i64, y: i64) -> [[f32; 4]; 4] {
        // Inside the window and the picture: two direct reads of two pixels each.
        let (lx, ly) = (x - i64::from(self.x0), y - i64::from(self.y0));
        let inside = lx >= 0
            && ly >= 0
            && lx + 1 < i64::from(self.w)
            && ly + 1 < i64::from(self.h)
            && x + 1 < i64::from(self.full.0)
            && y + 1 < i64::from(self.full.1);
        if inside {
            let at = ly as usize * self.w as usize + lx as usize;
            let below = at + self.w as usize;
            if let (Some(&[a, b]), Some(&[c, d])) = (self.data.get(at..at + 2), self.data.get(below..below + 2)) {
                return [a, b, c, d];
            }
        }
        let (jx, jy) = (x.saturating_add(1), y.saturating_add(1));
        [self.px(x, y), self.px(jx, y), self.px(x, jy), self.px(jx, jy)]
    }
}

/// The blocks `bx0..bx1 × by0..by1` of `src` reduced by `factor` (each the mean of the source
/// pixels it covers).
fn reduce(src: &Rgba8, factor: u32, bx0: u32, by0: u32, bx1: u32, by1: u32) -> Result<Reduced> {
    let full = (src.width.div_ceil(factor), src.height.div_ceil(factor));
    let (w, h) = (bx1.saturating_sub(bx0).max(1), by1.saturating_sub(by0).max(1));
    let n = usize::try_from(u64::from(w) * u64::from(h)).map_err(|_| Error::TooLarge { width: u64::from(w), height: u64::from(h) })?;
    if n as u64 > MAX_PIXELS {
        return Err(Error::TooLarge { width: u64::from(w), height: u64::from(h) });
    }
    let mut data: Vec<[f32; 4]> = Vec::new();
    data.try_reserve_exact(n).map_err(|_| Error::TooLarge { width: u64::from(w), height: u64::from(h) })?;
    data.resize(n, [0.0; 4]);
    let stride = src.width as usize * 4;
    data.par_chunks_mut(w as usize).enumerate().for_each(|(j, row)| {
        let by = by0.saturating_add(j as u32);
        let (y0, y1) = (by.saturating_mul(factor).min(src.height), by.saturating_add(1).saturating_mul(factor).min(src.height));
        let x_start = bx0.saturating_mul(factor).min(src.width) as usize;
        let mut sums = vec![[0u64; 4]; row.len()];
        // Source rows in order, each added to the blocks it crosses.
        for y in y0..y1 {
            let at = y as usize * stride;
            let Some(line) = src.data.get(at + x_start * 4..at + stride) else { continue };
            for (k, p) in line.as_chunks::<4>().0.iter().enumerate() {
                let Some(sum) = sums.get_mut(k / factor as usize) else { break };
                for c in 0..4 {
                    sum[c] += u64::from(p[c]);
                }
            }
        }
        for (i, (out, sum)) in row.iter_mut().zip(&sums).enumerate() {
            let bx = bx0.saturating_add(i as u32);
            let (x0, x1) = (bx.saturating_mul(factor).min(src.width), bx.saturating_add(1).saturating_mul(factor).min(src.width));
            let count = (f64::from(x1 - x0) * f64::from(y1 - y0)).max(1.0);
            *out = sum.map(|v| (v as f64 / count) as f32);
        }
    });
    Ok(Reduced { data, x0: bx0, y0: by0, w, h, full })
}

/// Bilinear sample of `plane` at continuous coordinates `(u, v)` (pixel centres at `i + 0.5`).
fn bilinear(plane: &impl Plane, u: f64, v: f64) -> [f32; 4] {
    let (x, y) = (u - 0.5, v - 0.5);
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = ((x - fx) as f32, (y - fy) as f32);
    let (ix, iy) = (fx as i64, fy as i64);
    let [p00, p10, p01, p11] = plane.quad(ix, iy);
    let mut out = [0.0; 4];
    for c in 0..4 {
        let top = p00[c] + (p10[c] - p00[c]) * tx;
        let bottom = p01[c] + (p11[c] - p01[c]) * tx;
        out[c] = top + (bottom - top) * ty;
    }
    out
}

/// Whether `map` sends every pixel centre onto exactly one pixel centre (a whole-pixel shift,
/// turn or mirror at full size).
fn one_to_one(map: &Affine) -> bool {
    let unit = |v: f64| v == 0.0 || v == 1.0 || v == -1.0;
    let whole = |v: f64| v.fract() == 0.0;
    unit(map.a)
        && unit(map.b)
        && unit(map.d)
        && unit(map.e)
        && whole(map.c)
        && whole(map.f)
        && map.a * map.b == 0.0
        && map.d * map.e == 0.0
        && (map.a * map.e - map.b * map.d).abs() == 1.0
}

/// The window `win` of the picture whose pixel coordinates `map` sends into `src`.
fn resample(src: &Rgba8, map: Affine, win: Win) -> Result<Rgba8> {
    let mut data = alloc(win.pixels())?;
    let row = win.w as usize * 4;
    data.resize(row * win.h as usize, 0);
    if row == 0 {
        return Ok(Rgba8 { width: win.w, height: win.h, data });
    }
    if one_to_one(&map) {
        // A whole-pixel shift: rows inside the source are copied as they are (the same bytes the
        // pixel-by-pixel path below gives).
        let shift = map.a == 1.0 && map.e == 1.0 && map.b == 0.0 && map.d == 0.0;
        let (sx, sy) = (i64::from(win.x0) + map.c as i64, i64::from(win.y0) + map.f as i64);
        if shift && sx >= 0 && sy >= 0 && sx + i64::from(win.w) <= i64::from(src.width) && sy + i64::from(win.h) <= i64::from(src.height) {
            let stride = src.width as usize * 4;
            let (sx, sy) = (sx as usize, sy as usize);
            data.par_chunks_mut(row).enumerate().for_each(|(j, line)| {
                let at = (sy + j) * stride + sx * 4;
                if let Some(from) = src.data.get(at..at + row) {
                    line.copy_from_slice(from);
                }
            });
            return Ok(Rgba8 { width: win.w, height: win.h, data });
        }
        let raw = Raw(src);
        data.par_chunks_mut(row).enumerate().for_each(|(j, line)| {
            let y = f64::from(win.y0) + j as f64 + 0.5;
            for (i, out) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = f64::from(win.x0) + i as f64 + 0.5;
                let (u, v) = map.apply(x, y);
                let p = raw.px(u.floor() as i64, v.floor() as i64);
                for c in 0..4 {
                    out[c] = p[c] as u8;
                }
            }
        });
        return Ok(Rgba8 { width: win.w, height: win.h, data });
    }
    // Source pixels per output pixel along each output axis.
    let footprint = map.a.hypot(map.d).max(map.b.hypot(map.e));
    let factor = if footprint >= 2.0 { (footprint.floor() as u32).clamp(1, MAX_REDUCTION) } else { 1 };
    let taps = ((footprint / f64::from(factor)).ceil() as u32).clamp(1, MAX_TAPS);
    if factor == 1 {
        sample_into(&mut data, row, &Raw(src), map, 1.0, taps, win);
    } else {
        // The source pixels the window reads, with a margin for the taps and the bilinear reach.
        let corners = [(win.x0, win.y0), (win.x0 + win.w, win.y0), (win.x0, win.y0 + win.h), (win.x0 + win.w, win.y0 + win.h)];
        let pts = corners.map(|(x, y)| map.apply(f64::from(x), f64::from(y)));
        let pad = footprint + 2.0 * f64::from(factor);
        let lo = |v: f64, n: u32| ((v - pad) / f64::from(factor)).floor().clamp(0.0, f64::from(n.div_ceil(factor))) as u32;
        let hi = |v: f64, n: u32| ((v + pad) / f64::from(factor)).ceil().clamp(0.0, f64::from(n.div_ceil(factor))) as u32;
        let (minx, maxx) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (miny, maxy) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        let reduced = reduce(src, factor, lo(minx, src.width), lo(miny, src.height), hi(maxx, src.width), hi(maxy, src.height))?;
        sample_into(&mut data, row, &reduced, map, f64::from(factor), taps, win);
    }
    Ok(Rgba8 { width: win.w, height: win.h, data })
}

/// Fills `data` (rows of `row` bytes, the window `win`) with `taps × taps` bilinear samples of
/// `plane` per output pixel, the plane holding the source reduced by `factor`.
fn sample_into(data: &mut [u8], row: usize, plane: &impl Plane, map: Affine, factor: f64, taps: u32, win: Win) {
    let k = f64::from(taps);
    let weight = 1.0 / (k * k) as f32;
    data.par_chunks_mut(row).enumerate().for_each(|(j, line)| {
        let y = f64::from(win.y0) + j as f64;
        for (i, out) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = f64::from(win.x0) + i as f64;
            let mut acc = [0.0f32; 4];
            for ty in 0..taps {
                for tx in 0..taps {
                    let (u, v) = map.apply(x + (f64::from(tx) + 0.5) / k, y + (f64::from(ty) + 0.5) / k);
                    let p = bilinear(plane, u / factor, v / factor);
                    for c in 0..4 {
                        acc[c] += p[c] * weight;
                    }
                }
            }
            for c in 0..4 {
                out[c] = to_u8(acc[c] / 255.0);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole-pixel shift copies rows: the same bytes as reading each pixel, inside the source
    /// and, through the per-pixel path, past its edges.
    #[test]
    fn a_whole_pixel_shift_copies_the_pixels_it_reads() {
        let src = image(37, 23);
        let raw = Raw(&src);
        for (dx, dy, win) in [
            (0.0, 0.0, Win { x0: 0, y0: 0, w: 37, h: 23 }),
            (3.0, 5.0, Win { x0: 2, y0: 1, w: 20, h: 10 }),
            (-4.0, 0.0, Win { x0: 0, y0: 0, w: 37, h: 23 }),
            (10.0, 10.0, Win { x0: 20, y0: 5, w: 17, h: 18 }),
        ] {
            let map = Affine { a: 1.0, b: 0.0, c: dx, d: 0.0, e: 1.0, f: dy };
            let out = resample(&src, map, win).unwrap();
            for j in 0..win.h {
                for i in 0..win.w {
                    let p = raw.px(i64::from(win.x0 + i) + dx as i64, i64::from(win.y0 + j) + dy as i64);
                    let at = ((j * win.w + i) * 4) as usize;
                    assert_eq!(&out.data[at..at + 4], &p.map(|v| v as u8)[..], "{dx},{dy} at {i},{j}");
                }
            }
        }
    }

    /// The fast paths give the same bytes: `to_u8` equals the rounding it replaced on every `f32`
    /// near the 8-bit range (and on hostile values), the table equals the division, and an 8-bit
    /// value survives the trip to working precision and back (so a render with no pass may skip
    /// it).
    #[test]
    fn the_fast_conversions_are_exact() {
        for v in 0..=255u8 {
            assert_eq!(UNIT[usize::from(v)].to_bits(), (f32::from(v) / 255.0).to_bits());
            assert_eq!(to_u8(UNIT[usize::from(v)]), v);
        }
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, -0.0, 0.0, f32::MIN_POSITIVE, 1e30, -1e30] {
            assert_eq!(to_u8(x), to_u8_reference(x), "{x}");
        }
        let check = |bits: u32| {
            let x = f32::from_bits(bits);
            assert_eq!(to_u8(x), to_u8_reference(x), "{x}");
            assert_eq!(to_u8(-x), to_u8_reference(-x), "{x}");
        };
        let end = 1.01f32.to_bits();
        if full() {
            // Every f32 from −1.01 to 1.01 (the bit patterns walk through each of them).
            (0..=end).for_each(check);
        } else {
            // Quick: 64 values either side of every rounding edge (k + 0.5) / 255 and of 0 and 1,
            // where the two roundings could part, and every 4099th value in between.
            let edges = (0..=255u16).map(|k| ((f32::from(k) + 0.5) / 255.0).to_bits()).chain([0, 1.0f32.to_bits()]);
            for edge in edges {
                (edge.saturating_sub(64)..=(edge + 64).min(end)).for_each(check);
            }
            (0..=end).step_by(4099).for_each(check);
        }
    }

    /// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
    fn full() -> bool {
        std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
    }

    fn image(w: u32, h: u32) -> Rgba8 {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                data.extend([(x * 7 + y * 3) as u8, (x * x + y) as u8, (y * 11) as u8, 255]);
            }
        }
        Rgba8 { width: w, height: h, data }
    }

    /// Inverts every channel.
    struct Invert;
    impl Pass for Invert {
        fn run(&self, band: &mut Band<'_>) {
            for p in band.pixels.iter_mut() {
                *p = [1.0 - p[0], 1.0 - p[1], 1.0 - p[2]];
            }
        }
    }

    /// A box blur of radius `r`, reading the picture before it.
    struct Blur(u32);
    impl Pass for Blur {
        fn reach(&self) -> Reach {
            Reach::Pixels(self.0)
        }
        fn run(&self, band: &mut Band<'_>) {
            let Some(before) = band.before else { return };
            let r = i64::from(self.0);
            for y in 0..band.rows {
                for x in 0..band.width {
                    let (ax, ay) = (i64::from(band.x0 + x), i64::from(band.y0 + y));
                    let mut sum = [0.0; 3];
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let p = before.get(ax + dx, ay + dy);
                            for c in 0..3 {
                                sum[c] += p[c];
                            }
                        }
                    }
                    let n = ((2 * r + 1) * (2 * r + 1)) as f32;
                    if let Some(p) = band.get_mut(x, y) {
                        *p = [sum[0] / n, sum[1] / n, sum[2] / n];
                    }
                }
            }
        }
    }

    /// Moves every pixel to the mean of the whole picture.
    struct Mean;
    impl Pass for Mean {
        fn reach(&self) -> Reach {
            Reach::Whole
        }
        fn run(&self, band: &mut Band<'_>) {
            let Some(before) = band.before else { return };
            let (w, h) = before.full_size();
            let mut sum = [0.0; 3];
            for y in 0..h {
                for x in 0..w {
                    let p = before.get(i64::from(x), i64::from(y));
                    for c in 0..3 {
                        sum[c] += p[c];
                    }
                }
            }
            let n = (w * h) as f32;
            for p in band.pixels.iter_mut() {
                *p = [(p[0] + sum[0] / n) / 2.0, (p[1] + sum[1] / n) / 2.0, (p[2] + sum[2] / n) / 2.0];
            }
        }
    }

    type MakePass = fn() -> Box<dyn Pass>;

    fn passes() -> Vec<Vec<MakePass>> {
        vec![
            vec![|| Box::new(Invert)],
            vec![|| Box::new(Blur(2)), || Box::new(Invert), || Box::new(Blur(1))],
            vec![|| Box::new(Invert), || Box::new(Mean), || Box::new(Blur(3))],
        ]
    }

    /// The adjust stage run on a window (with the margin the passes ask for) equals that window of
    /// the stage run on the whole picture, for pixel, neighbourhood and whole-picture passes.
    #[test]
    fn a_window_of_the_adjust_stage_equals_the_whole_cut() {
        let full_img = image(70, 90);
        let full = (70, 90);
        let whole = Win { x0: 0, y0: 0, w: 70, h: 90 };
        for make in passes() {
            let slots = || make.iter().map(|m| Slot::Ready(Some(m()))).collect::<Vec<_>>();
            let mut all = full_img.clone();
            run_slots(&mut all, whole, full, slots(), |_, _| None).unwrap();
            for win in [Win { x0: 0, y0: 0, w: 5, h: 5 }, Win { x0: 13, y0: 40, w: 30, h: 33 }, Win { x0: 60, y0: 85, w: 10, h: 5 }] {
                let margin = slots().iter().fold(0u32, |m, s| match s {
                    Slot::Ready(Some(p)) => match p.reach() {
                        Reach::Pixel => m,
                        Reach::Pixels(r) => m.saturating_add(r),
                        Reach::Whole => u32::MAX,
                    },
                    _ => m,
                });
                let grown = win.grown(margin, full);
                let mut part = cut(full_img.clone(), whole, grown).unwrap();
                run_slots(&mut part, grown, full, slots(), |_, _| None).unwrap();
                assert_eq!(cut(part, grown, win).unwrap(), cut(all.clone(), whole, win).unwrap(), "{win:?}");
            }
        }
    }

    #[test]
    fn whole_pixel_maps_are_recognised() {
        assert!(one_to_one(&Affine::IDENTITY));
        assert!(one_to_one(&dihedral_to_input(Dihedral::TRANSPOSE, 7.0, 3.0)));
        assert!(!one_to_one(&Affine::scale(0.5, 0.5)));
        assert!(!one_to_one(&Affine::translate(0.5, 0.0)));
        assert!(!one_to_one(&Affine { a: 1.0, b: 1.0, c: 0.0, d: 0.0, e: 1.0, f: 0.0 }));
    }

    #[test]
    fn a_two_times_reduction_is_the_mean_of_each_block() {
        let img = image(8, 6);
        let out = resample(&img, Affine::scale(2.0, 2.0), Win { x0: 0, y0: 0, w: 4, h: 3 }).unwrap();
        for y in 0..3usize {
            for x in 0..4usize {
                for c in 0..4 {
                    let at = |xx: usize, yy: usize| f32::from(img.data[(yy * 8 + xx) * 4 + c]);
                    let mean = (at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1)) / 4.0;
                    let got = out.data[(y * 4 + x) * 4 + c];
                    assert!((f32::from(got) - mean).abs() <= 0.5, "({x}, {y}) channel {c}: {got} vs {mean}");
                }
            }
        }
    }
}
