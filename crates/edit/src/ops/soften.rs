//! Soften with its paint mask (`editing.md` §8.4): `soften(softness,fade;stroke;…)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Softness 0–100 (50)
//!   and Fade 0–100 (50), then the brush strokes, each after a `;`, in the order they were drawn:
//!   `paint,r,x,y,x,y,…` or `erase,r,x,y,…` — the mode, the brush radius `r` relative to the
//!   stored photo's shorter side ([`MIN_RADIUS`]..[`MAX_RADIUS`], as Retouch's), and the stroke's
//!   points (at least one, at most [`MAX_POINTS`]) normalised in the stored frame (`data-model.md`
//!   §3.3) and brought into −1..2 as Retouch's. Without strokes (`soften(50,50)`) Soften applies
//!   to the whole photo; from the first stroke it applies where paint strokes cover minus where
//!   later erase strokes do. More than [`MAX_STROKES`] strokes, a stroke without a mode, a radius
//!   and whole points, `NaN`, or a missing or extra field is refused (the entry is kept and
//!   skipped); numbers out of range are clamped.
//! - **Rendering** (M12.11, `VERSION` 1): the photo blurred by a Gaussian of sigma
//!   `Softness/100 ×` [`SIGMA`] of the shorter side ([`crate::fx::Gaussian`]), the blurred copy
//!   screened over the photo by `Softness/100 ×` [`GLOW`] (a soft glow: detail melts, shadows lift
//!   a little, whites stay white), faded toward the input by Fade ([`crate::fx::fade`]), then
//!   applied by the mask: `out = in + (soft − in) · mask`, so where the mask is 0 the photo is
//!   untouched. Softness 0 or Fade 100 changes nothing.
//! - **Mask** ([`Soften::coverage`]): 1 everywhere without strokes; otherwise the strokes in
//!   order on 0, each its dabs ([`crate::fx::Dabs`], feather [`FEATHER`]) along its stored-frame
//!   points mapped through [`super::Prepare::geometry`] (so they stay on the same content after a
//!   crop, a straighten, an EXIF orientation and the recipe's turns): a paint stroke adds
//!   `s · (1 − v)`, an erase stroke takes `v · s` away (as [`crate::fx::paint`]). The mask is
//!   painted per pixel at the output's resolution, the strokes in parallel over chunks of rows;
//!   an entry that would cost more than [`MAX_WORK`] there (thousands of long strokes of a large
//!   brush) is painted on a grid halved until it fits and sampled bilinearly, so any accepted
//!   entry renders in bounded time and memory. The blur covers only the box where the mask
//!   applies, grown by its reach, so a small stroke on a large photo blurs a small patch; a
//!   region render equals that window of the whole render.

use std::fmt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;

use super::{Adjust, Band, FADE, Operation, ParamError, Pass, Prepare, Reach, Reader, View, clamp, num, num_point};
use crate::frame::Point;
use crate::fx::brush::{MAX_DABS, dab_alpha, dab_spacing};
use crate::fx::{Blend, Dabs, Gaussian, Plane, Space, fade, mix};
use crate::render::Geometry;

/// The range of Softness.
pub const SOFTNESS: (f64, f64) = (0.0, 100.0);
/// The most strokes one `soften` entry holds (as Retouch, `security.md` §8).
pub const MAX_STROKES: usize = 1000;
/// The most points one stroke holds.
pub const MAX_POINTS: usize = 4096;
/// The smallest brush radius, relative to the shorter side.
pub const MIN_RADIUS: f64 = 0.001;
/// The largest brush radius, relative to the shorter side.
pub const MAX_RADIUS: f64 = 0.25;
/// The range of a stroke's points (as Retouch's: a brush may reach past the edge).
pub const POINT: (f64, f64) = (-1.0, 2.0);
/// The range of a stroke's radius.
pub const RADIUS: (f64, f64) = (MIN_RADIUS, MAX_RADIUS);

/// Whether a stroke paints where Soften applies or erases it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Paint,
    Erase,
}

impl Mode {
    /// The word an entry writes.
    pub fn word(self) -> &'static str {
        match self {
            Mode::Paint => "paint",
            Mode::Erase => "erase",
        }
    }
}

/// The modes by their words.
pub const MODES: [(&str, Mode); 2] = [("paint", Mode::Paint), ("erase", Mode::Erase)];

/// One brush stroke in the stored frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stroke {
    pub mode: Mode,
    /// Relative to the shorter side.
    pub radius: f64,
    pub points: Vec<Point>,
}

impl Stroke {
    /// The stroke with its radius and points brought into range (`NaN` to the low end).
    pub fn clamped(self) -> Stroke {
        let c = |p: Point| Point::new(clamp(p.x, POINT), clamp(p.y, POINT));
        Stroke { mode: self.mode, radius: clamp(self.radius, RADIUS), points: self.points.into_iter().map(c).collect() }
    }
}

/// `soften(softness,fade;stroke;…)`: a soft glow blur, everywhere or where painted.
#[derive(Clone, Debug, PartialEq)]
pub struct Soften {
    pub softness: f64,
    pub fade: f64,
    pub strokes: Vec<Stroke>,
}

impl Default for Soften {
    fn default() -> Soften {
        Soften { softness: 50.0, fade: 50.0, strokes: Vec::new() }
    }
}

impl fmt::Display for Soften {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", num(self.softness), num(self.fade))?;
        for s in &self.strokes {
            write!(f, ";{},{}", s.mode.word(), num(s.radius))?;
            for p in &s.points {
                write!(f, ",{}", num_point(*p))?;
            }
        }
        Ok(())
    }
}

impl Operation for Soften {
    const NAME: &'static str = "soften";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Soften";

    fn parse(params: &str) -> Result<Soften, ParamError> {
        let refuse = |why: &str| ParamError::new(Self::NAME, params, why);
        let mut parts = params.split(';');
        let mut r = Reader::new(Self::NAME, parts.next().unwrap_or(""));
        let softness = r.number("the softness", SOFTNESS)?;
        let fade = r.number("fade", FADE)?;
        r.end()?;
        let mut strokes = Vec::new();
        for text in parts {
            if strokes.len() >= MAX_STROKES {
                return Err(refuse("holds more strokes than one entry may"));
            }
            let mut r = Reader::new(Self::NAME, text);
            let mode = r.choice("the brush mode", &MODES)?;
            let radius = r.number("the brush radius", RADIUS)?;
            // The points: pairs to the end of the stroke.
            let fields = text.split(',').count().saturating_sub(2);
            if fields == 0 || fields % 2 != 0 || fields / 2 > MAX_POINTS {
                return Err(refuse("each stroke takes a mode, a radius and whole points"));
            }
            let mut points = Vec::with_capacity(fields / 2);
            for _ in 0..fields / 2 {
                let x = r.number("a point", POINT)?;
                let y = r.number("a point", POINT)?;
                points.push(Point::new(x, y));
            }
            r.end()?;
            strokes.push(Stroke { mode, radius, points });
        }
        Ok(Soften { softness, fade, strokes })
    }

    fn clamped(self) -> Soften {
        Soften {
            softness: clamp(self.softness, SOFTNESS),
            fade: clamp(self.fade, FADE),
            strokes: self.strokes.into_iter().map(Stroke::clamped).collect(),
        }
    }
}

// Rendering (M12.11).

/// The blur's sigma at Softness 100, relative to the stored photo's shorter side.
pub const SIGMA: f64 = 0.03;
/// How much of the glow (the blurred copy screened over the photo) shows at Softness 100.
pub const GLOW: f64 = 0.5;
/// The fraction of a brush dab's radius that fades (LightCraft's default brush feather).
pub const FEATHER: f64 = 0.5;
/// The most work one entry's mask may cost (dab pixels stamped plus pixels combined): past it the
/// mask is painted on a coarser grid (halved until it fits) and sampled bilinearly.
pub const MAX_WORK: f64 = (1u64 << 28) as f64;

/// Rows of the mask painted together on one thread.
const CHUNK: usize = 32;

impl Soften {
    /// Where this entry applies over the `window` (`x0, y0, width, height` in output pixels) of a
    /// `width × height` output (before the recipe's rotations and flips) of `geometry`: 1 where it
    /// applies fully, 0 where not, row-major. Without strokes it applies everywhere; otherwise the
    /// strokes' dabs combine in order from 0 ([`crate::fx::brush`]). `None` when the geometry maps
    /// no usable frame, the window is empty or larger than [`crate::render::MAX_PIXELS`], or a
    /// buffer cannot be had. The UI's brush overlay and the tests read the same mask the
    /// renderer uses.
    pub fn coverage(&self, geometry: &Geometry, width: u32, height: u32, window: (u32, u32, u32, u32)) -> Option<Vec<f32>> {
        let (x0, y0, w, h) = window;
        let n = u64::from(w).checked_mul(u64::from(h)).filter(|n| *n > 0 && *n <= crate::render::MAX_PIXELS)?;
        if self.strokes.is_empty() {
            return alloc(usize::try_from(n).ok()?, 1.0);
        }
        Brush::new(&self.strokes, geometry, width, height)?.window(x0, y0, w, h)
    }
}

impl Adjust for Soften {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let s = clamp(self.softness, SOFTNESS) / 100.0;
        let fade = clamp(self.fade, FADE);
        if s <= 0.0 || fade >= 100.0 {
            return None;
        }
        let space = Space::new(ctx)?;
        let gaussian = Gaussian::new(space.length(s * SIGMA), (ctx.width, ctx.height));
        let brush = if self.strokes.is_empty() { None } else { Some(Brush::new(&self.strokes, ctx.geometry, ctx.width, ctx.height)?) };
        Some(Box::new(SoftenPass { gaussian, glow: (s * GLOW) as f32, fade, brush, done: OnceLock::new() }))
    }
}

/// Soften prepared for one render.
struct SoftenPass {
    gaussian: Gaussian,
    glow: f32,
    fade: f64,
    /// `None`: everywhere.
    brush: Option<Brush>,
    /// The mask and the blurred picture over the `before` view, made by the first band to run.
    done: OnceLock<Option<Made>>,
}

/// What every band of one render reads.
struct Made {
    /// The view's window and its mask at output resolution (`None`: everywhere).
    x0: u32,
    y0: u32,
    width: u32,
    mask: Option<Vec<f32>>,
    /// The blurred picture over where the mask applies (and the blur's reach); `None` when it
    /// applies nowhere.
    plane: Option<Plane>,
}

impl Made {
    fn mask(&self, x: u32, y: u32) -> f32 {
        let Some(mask) = &self.mask else { return 1.0 };
        let (Some(lx), Some(ly)) = (x.checked_sub(self.x0), y.checked_sub(self.y0)) else { return 0.0 };
        let i = (ly as usize).checked_mul(self.width as usize).and_then(|i| i.checked_add(lx as usize));
        i.and_then(|i| mask.get(i)).copied().unwrap_or(0.0)
    }
}

impl SoftenPass {
    fn make(&self, view: &View<'_>) -> Option<Made> {
        let (x0, y0, w, h) = (view.x0, view.y0, view.width, view.height);
        let mask = match &self.brush {
            Some(brush) => Some(brush.window(x0, y0, w, h)?),
            None => None,
        };
        // The box where the mask applies, in output pixels, grown by the blur's reach.
        let bounds = match &mask {
            None => Some((x0, y0, x0.saturating_add(w), y0.saturating_add(h))),
            Some(m) => applied_box(m, w as usize).map(|(a, b, c, d)| (x0 + a, y0 + b, x0 + c, y0 + d)),
        };
        let plane = match bounds {
            None => None,
            Some((ax, ay, bx, by)) => {
                let reach = self.gaussian.reach();
                let (ax, ay) = (ax.saturating_sub(reach).max(x0), ay.saturating_sub(reach).max(y0));
                let (bx, by) = (bx.saturating_add(reach).min(x0.saturating_add(w)), by.saturating_add(reach).min(y0.saturating_add(h)));
                let mut plane = Plane::from_fn(ax, ay, bx.saturating_sub(ax), by.saturating_sub(ay), |x, y| view.get(i64::from(x), i64::from(y)))?;
                if !self.gaussian.apply(&mut plane) {
                    return None;
                }
                Some(plane)
            }
        };
        Some(Made { x0, y0, width: w, mask, plane })
    }
}

impl Pass for SoftenPass {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.gaussian.reach())
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        let Some(made) = self.done.get_or_init(|| self.make(view)) else { return };
        let Some(plane) = &made.plane else { return };
        let (x0, y0, w) = (band.x0, band.y0, band.width as usize);
        if w == 0 {
            return;
        }
        for (ly, row) in band.pixels.chunks_mut(w).enumerate() {
            let y = y0.saturating_add(u32::try_from(ly).unwrap_or(u32::MAX));
            for (lx, px) in row.iter_mut().enumerate() {
                let x = x0.saturating_add(u32::try_from(lx).unwrap_or(u32::MAX));
                let m = made.mask(x, y);
                if m <= 0.0 || m.is_nan() {
                    continue;
                }
                let input = *px;
                let blurred = plane.get(i64::from(x), i64::from(y));
                let glow = mix(blurred, Blend::Screen.apply(input, blurred), self.glow);
                *px = mix(input, fade(input, glow, self.fade), m);
            }
        }
    }
}

/// The box `(x0, y0, x1, y1)` (half-open, window pixels) of the mask's cells above 0, if any.
fn applied_box(mask: &[f32], width: usize) -> Option<(u32, u32, u32, u32)> {
    if width == 0 {
        return None;
    }
    let mut b: Option<(usize, usize, usize, usize)> = None;
    for (y, row) in mask.chunks(width).enumerate() {
        let (Some(first), Some(last)) = (row.iter().position(|v| *v > 0.0), row.iter().rposition(|v| *v > 0.0)) else { continue };
        b = Some(match b {
            None => (first, y, last + 1, y + 1),
            Some((a, t, c, _)) => (a.min(first), t, c.max(last + 1), y + 1),
        });
    }
    let (a, t, c, d) = b?;
    Some((u32::try_from(a).ok()?, u32::try_from(t).ok()?, u32::try_from(c).ok()?, u32::try_from(d).ok()?))
}

/// The strokes of one entry ready to paint for one render: in the stored frame mapped onto a grid
/// that is the output itself, or a coarser one when the strokes would cost more than
/// [`MAX_WORK`] at full resolution.
#[derive(Clone, Debug)]
struct Brush {
    strokes: Vec<Stroke>,
    /// The output's size and the grid's (the stored frame mapped onto `grid` pixels).
    out: (u32, u32),
    grid: (u32, u32),
    space: Space,
}

impl Brush {
    fn new(strokes: &[Stroke], geometry: &Geometry, width: u32, height: u32) -> Option<Brush> {
        Brush::within(strokes, geometry, width, height, MAX_WORK)
    }

    /// The brush on the finest grid (the output, then halved) whose [`work`] is at most `budget`.
    fn within(strokes: &[Stroke], geometry: &Geometry, width: u32, height: u32, budget: f64) -> Option<Brush> {
        let mut k: u32 = 1;
        loop {
            let grid = (width.div_ceil(k).max(1), height.div_ceil(k).max(1));
            let space = Space::from_geometry(geometry, grid.0, grid.1)?;
            if grid == (1, 1) || work(&space, strokes) <= budget {
                return Some(Brush { strokes: strokes.to_vec(), out: (width, height), grid, space });
            }
            k = k.saturating_mul(2);
        }
    }

    /// The mask over the output window `(x0, y0, w, h)`, row-major.
    fn window(&self, x0: u32, y0: u32, w: u32, h: u32) -> Option<Vec<f32>> {
        if self.grid == self.out {
            return self.paint(x0, y0, w, h);
        }
        // Bilinear from the grid: an output pixel's centre lies at `(x + 0.5)·sx − 0.5` in grid
        // pixels; the grid window holds every cell a pixel of the window reads.
        let (sx, sy) = (f64::from(self.grid.0) / f64::from(self.out.0.max(1)), f64::from(self.grid.1) / f64::from(self.out.1.max(1)));
        let at = |v: u32, s: f64| (f64::from(v) + 0.5) * s - 0.5;
        let cell = |u: f64, n: u32| -> u32 { u.floor().clamp(0.0, f64::from(n.saturating_sub(1))) as u32 };
        let (gx0, gy0) = (cell(at(x0, sx), self.grid.0), cell(at(y0, sy), self.grid.1));
        let gx1 = cell(at(x0.saturating_add(w.saturating_sub(1)), sx), self.grid.0).saturating_add(1).min(self.grid.0 - 1);
        let gy1 = cell(at(y0.saturating_add(h.saturating_sub(1)), sy), self.grid.1).saturating_add(1).min(self.grid.1 - 1);
        let (gw, gh) = (gx1 - gx0 + 1, gy1 - gy0 + 1);
        let coarse = self.paint(gx0, gy0, gw, gh)?;
        let get = |gx: u32, gy: u32| -> f32 {
            let (lx, ly) = (gx.saturating_sub(gx0).min(gw - 1) as usize, gy.saturating_sub(gy0).min(gh - 1) as usize);
            coarse.get(ly * gw as usize + lx).copied().unwrap_or(0.0)
        };
        let n = (w as usize).checked_mul(h as usize)?;
        let mut out = alloc(n, 0.0)?;
        out.par_chunks_mut(w.max(1) as usize).enumerate().for_each(|(ly, row)| {
            let v = at(y0.saturating_add(u32::try_from(ly).unwrap_or(u32::MAX)), sy);
            let (j0, j1, ty) = taps(v, self.grid.1);
            for (lx, px) in row.iter_mut().enumerate() {
                let u = at(x0.saturating_add(u32::try_from(lx).unwrap_or(u32::MAX)), sx);
                let (i0, i1, tx) = taps(u, self.grid.0);
                let top = get(i0, j0) + (get(i1, j0) - get(i0, j0)) * tx;
                let bottom = get(i0, j1) + (get(i1, j1) - get(i0, j1)) * tx;
                *px = top + (bottom - top) * ty;
            }
        });
        Some(out)
    }

    /// The strokes painted in order over the grid window `(x0, y0, w, h)` (each pixel sampled at
    /// its centre), row-major.
    fn paint(&self, x0: u32, y0: u32, w: u32, h: u32) -> Option<Vec<f32>> {
        let (wu, hu) = (w as usize, h as usize);
        let mut out = alloc(wu.checked_mul(hu)?, 0.0)?;
        if wu == 0 || hu == 0 {
            return Some(out);
        }
        let failed = AtomicBool::new(false);
        for stroke in &self.strokes {
            let dabs = Dabs::new(&self.space, &stroke.points, stroke.radius, FEATHER, false);
            let r = dabs.radius();
            let hard = r * (1.0 - FEATHER);
            let (r2, hard2) = (r * r, hard * hard);
            let boxes: Vec<Option<(usize, usize, usize, usize)>> =
                dabs.centres().iter().map(|c| window_box((c.0 - r, c.1 - r, c.0 + r, c.1 + r), x0, y0, w, h)).collect();
            let Some((ax, bx)) =
                boxes.iter().flatten().fold(None, |acc: Option<(usize, usize)>, b| Some(acc.map_or((b.0, b.2), |a| (a.0.min(b.0), a.1.max(b.2)))))
            else {
                continue;
            };
            // Each dab listed in every chunk of rows it reaches.
            let chunks = hu.div_ceil(CHUNK);
            let mut lists: Vec<Vec<u32>> = vec![Vec::new(); chunks];
            for (i, b) in boxes.iter().enumerate() {
                let (Some(&(_, ay, _, by)), Ok(i)) = (b.as_ref(), u32::try_from(i)) else { continue };
                for list in lists.iter_mut().take(by.saturating_sub(1) / CHUNK + 1).skip(ay / CHUNK) {
                    list.push(i);
                }
            }
            let mode = stroke.mode;
            let centres = dabs.centres();
            out.par_chunks_mut(wu * CHUNK).enumerate().for_each(|(ci, rows)| {
                let Some(list) = lists.get(ci).filter(|l| !l.is_empty()) else { return };
                let (cy0, n) = (ci * CHUNK, rows.len() / wu);
                let cw = bx - ax;
                let Some(mut cover) = alloc(cw * n, 0.0f32) else {
                    failed.store(true, Ordering::Relaxed);
                    return;
                };
                for &i in list {
                    let (Some(c), Some(Some((dx0, dy0, dx1, dy1)))) = (centres.get(i as usize), boxes.get(i as usize)) else { continue };
                    for ly in (*dy0).max(cy0)..(*dy1).min(cy0 + n) {
                        let dy = f64::from(y0) + ly as f64 + 0.5 - c.1;
                        let dy2 = dy * dy;
                        if dy2 > r2 {
                            continue;
                        }
                        // Only the columns of the dab's disc on this row.
                        let half = (r2 - dy2).sqrt();
                        let column = |v: f64| (v - f64::from(x0) - 0.5).clamp(*dx0 as f64, *dx1 as f64) as usize;
                        let (lo, hi) = (column((c.0 - half).floor()), column((c.0 + half).ceil() + 1.0));
                        let Some(line) = cover.get_mut((ly - cy0) * cw + lo - ax..(ly - cy0) * cw + hi.max(lo) - ax) else { continue };
                        for (lx, v) in (lo..).zip(line.iter_mut()) {
                            let dx = f64::from(x0) + lx as f64 + 0.5 - c.0;
                            let dd2 = dx * dx + dy2;
                            let a = if dd2 <= hard2 { 1.0 } else { dab_alpha(dd2.sqrt(), r, hard) };
                            if a > *v {
                                *v = a;
                            }
                        }
                    }
                }
                for (row, line) in rows.chunks_mut(wu).zip(cover.chunks(cw)) {
                    for (v, &s) in row.iter_mut().skip(ax).zip(line) {
                        if s > 0.0 {
                            *v = match mode {
                                Mode::Paint => *v + s * (1.0 - *v),
                                Mode::Erase => *v * (1.0 - s),
                            };
                        }
                    }
                }
            });
        }
        (!failed.load(Ordering::Relaxed)).then_some(out)
    }
}

/// The two grid cells either side of `u` (grid pixels, a cell's centre at `+ 0.5`) along an axis
/// of `n` cells, and how far between them it lies.
fn taps(u: f64, n: u32) -> (u32, u32, f32) {
    let last = f64::from(n.saturating_sub(1));
    if !u.is_finite() {
        return (0, 0, 0.0);
    }
    let f = u.floor();
    let i0 = f.clamp(0.0, last) as u32;
    let i1 = (f + 1.0).clamp(0.0, last) as u32;
    (i0, i1, (u - f) as f32)
}

/// What painting `strokes` on `space`'s grid costs at most: every dab's box and every pixel of a
/// stroke's box combined.
fn work(space: &Space, strokes: &[Stroke]) -> f64 {
    let (w, h) = space.size();
    let (w, h) = (f64::from(w), f64::from(h));
    strokes
        .iter()
        .map(|s| {
            let r = space.length(s.radius).max(0.5);
            let path: Vec<(f64, f64)> = s.points.iter().map(|p| space.to_output(*p)).filter(|q| q.0.is_finite() && q.1.is_finite()).collect();
            let length: f64 = path.windows(2).map(|q| q.first().zip(q.get(1)).map_or(0.0, |(a, b)| (b.0 - a.0).hypot(b.1 - a.1))).sum();
            let spacing = dab_spacing(r).max(length / MAX_DABS as f64);
            let dabs = (length / spacing + path.len() as f64).min(MAX_DABS as f64);
            let side = |n: f64| (2.0 * r + 2.0).min(n + 2.0);
            let (x0, y0, x1, y1) = path.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, q| {
                (b.0.min(q.0 - r), b.1.min(q.1 - r), b.2.max(q.0 + r), b.3.max(q.1 + r))
            });
            let area = (x1.min(w) - x0.max(0.0)).max(0.0) * (y1.min(h) - y0.max(0.0)).max(0.0);
            let area = if area.is_finite() { area } else { w * h };
            dabs * (side(w) * side(h) + 1.0) + area
        })
        .sum()
}

/// The rows and columns of the `width × rows` window at `(x0, y0)` that the box `b` (`(x0, y0,
/// x1, y1)` in grid pixels) reaches, as half-open ranges `(x0, y0, x1, y1)` of window coordinates
/// (as [`crate::fx::paint`] reads them).
fn window_box(b: (f64, f64, f64, f64), x0: u32, y0: u32, width: u32, rows: u32) -> Option<(usize, usize, usize, usize)> {
    if !(b.0.is_finite() && b.1.is_finite() && b.2.is_finite() && b.3.is_finite()) {
        return None;
    }
    let lo = |v: f64, o: u32, n: u32| -> usize { (v - f64::from(o) - 0.5).floor().clamp(0.0, f64::from(n)) as usize };
    let hi = |v: f64, o: u32, n: u32| -> usize { (v - f64::from(o) + 0.5).ceil().clamp(0.0, f64::from(n)) as usize };
    let (ax, bx) = (lo(b.0, x0, width), hi(b.2, x0, width));
    let (ay, by) = (lo(b.1, y0, rows), hi(b.3, y0, rows));
    (ax < bx && ay < by).then_some((ax, ay, bx, by))
}

fn alloc<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Recipe, render::Geometry};
    use dalica_image::Orientation;

    /// An entry past the budget is painted on a coarser grid and sampled bilinearly: close to the
    /// mask at full resolution, the same between a window and the whole.
    #[test]
    fn a_coarser_grid_paints_nearly_the_same_mask() {
        let (w, h) = (300u32, 200u32);
        let g = Geometry::new(&Recipe::default(), Orientation::default(), w, h);
        let line = |y: f64| (0..40).map(|i| Point::new(0.05 + f64::from(i) * 0.022, y + 0.05 * (f64::from(i) * 0.7).sin())).collect::<Vec<_>>();
        let strokes = vec![
            Stroke { mode: Mode::Paint, radius: 0.12, points: line(0.4) },
            Stroke { mode: Mode::Erase, radius: 0.08, points: line(0.55) },
            Stroke { mode: Mode::Paint, radius: 0.1, points: vec![Point::new(0.7, 0.75)] },
        ];
        let fine = Brush::within(&strokes, &g, w, h, f64::INFINITY).unwrap();
        assert_eq!(fine.grid, (w, h));
        // The budget of a grid a quarter the size: the brush halves twice.
        let quarter = Space::from_geometry(&g, w / 4, h / 4).unwrap();
        let coarse = Brush::within(&strokes, &g, w, h, work(&quarter, &strokes)).unwrap();
        assert_eq!(coarse.grid, (w / 4, h / 4));
        let (a, b) = (fine.window(0, 0, w, h).unwrap(), coarse.window(0, 0, w, h).unwrap());
        let most = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        let mean = a.iter().zip(&b).map(|(x, y)| f64::from((x - y).abs())).sum::<f64>() / a.len() as f64;
        // Tolerance: a feathered edge two grid cells wide sampled bilinearly.
        assert!(most < 0.25 && mean < 0.01, "most {most}, mean {mean}");
        let part = coarse.window(117, 41, 50, 33).unwrap();
        for y in 0..33usize {
            assert_eq!(part[y * 50..(y + 1) * 50], b[(41 + y) * 300 + 117..(41 + y) * 300 + 167]);
        }
    }
}
