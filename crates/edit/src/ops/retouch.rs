//! `retouch(sx,sy,tx,ty,r;…)` (`editing.md` §6.4, `data-model.md` §3.1): patches copied from a
//! source point to a target point, in the stored frame (`data-model.md` §3.3), mapped through the
//! crop and straighten ([`super::Prepare::geometry`]).
//!
//! - **Parameters:** strokes separated by `;`, each five finite numbers: the source point, the
//!   target point (normalised, stored frame) and the radius (relative to the stored photo's shorter
//!   side, so the same in every frame). Points are brought into −1..2 (anything further out is off
//!   the picture all the same) and the radius into [`MIN_RADIUS`]..[`MAX_RADIUS`]; a value that is
//!   not a finite number, a stroke without exactly five, or more than [`MAX_STROKES`] strokes is
//!   refused (the entry is kept and skipped). No strokes (`retouch()`) changes nothing.
//! - **Rendering:** the strokes apply in order, each on the picture the strokes before it left. A
//!   patch is healed: it copies the source's detail and keeps the target's tone, `src + blur(target −
//!   src)` with the difference measured on the patch's surroundings only (frequency separation), and
//!   blends in with a feathered edge. Values are the renderer's sRGB-encoded working values. A
//!   stroke whose target lies outside the picture (or outside the crop) draws nothing; reads past
//!   the edge take the nearest edge pixel.
//! - **Reads:** a pixel depends only on the strokes over it, their sources, and the earlier strokes
//!   drawn where those read, so the pass's [`Reach`] is the longest such chain (each stroke reads
//!   across its patch's box and on to its source) rather than the whole picture: a region render
//!   grows its window only that far. Strokes whose box misses the window being rendered are left
//!   out of it.
//!
//! Ported from LightCraft `crates/pipeline/src/spots.rs` (`apply` in Heal mode, and the reasoning of
//! `window_for_reads`) and `crates/raster/src/blur.rs` (`box_radii`, the three box passes) at
//! revision 76b53778 (MIT OR Apache-2.0; `NOTICE`). Dalica's changes: one source point per stroke
//! instead of a dab path and an offset; a fixed feather (LightCraft's default 50 %) and full
//! opacity; sRGB-encoded working values instead of scene-linear light; the window is grown by a
//! uniform reach (the renderer's [`Reach::Pixels`]) instead of per spot; the healed patches are
//! computed once per render into a buffer the bands copy from; no Clone, Remove or Fill modes.

use std::fmt;
use std::sync::OnceLock;

use super::crop::{number, read_number};
use super::{Adjust, Band, Operation, ParamError, Pass, Prepare, Reach, View};
use crate::frame::Point;
use crate::fx::apart;

/// The most strokes one `retouch` entry holds (`security.md` §8).
pub const MAX_STROKES: usize = 1000;
/// The smallest radius, relative to the shorter side.
pub const MIN_RADIUS: f64 = 0.001;
/// The largest radius, relative to the shorter side.
pub const MAX_RADIUS: f64 = 0.1;
/// How far outside the picture a stored point may lie (−`POINT_SLACK`..1 + `POINT_SLACK`).
const POINT_SLACK: f64 = 1.0;
/// The part of the radius at full strength (LightCraft's default feather, 50 %: full strength
/// inside 0.6 r, none beyond r).
const INNER: f32 = 0.6;
/// The patch's box reaches this many radii around the target: the stroke plus a radius of
/// surroundings the tone is measured on.
const BOX_RADII: f64 = 2.0;

/// One stroke: the patch at `target` copies from `source`, `radius` relative to the shorter side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stroke {
    pub source: Point,
    pub target: Point,
    pub radius: f64,
}

impl Stroke {
    /// The stroke with its points and radius brought into range (not-a-number stays).
    pub fn clamped(self) -> Stroke {
        let c = |v: f64| v.clamp(-POINT_SLACK, 1.0 + POINT_SLACK);
        Stroke {
            source: Point::new(c(self.source.x), c(self.source.y)),
            target: Point::new(c(self.target.x), c(self.target.y)),
            radius: self.radius.clamp(MIN_RADIUS, MAX_RADIUS),
        }
    }

    fn is_finite(&self) -> bool {
        [self.source.x, self.source.y, self.target.x, self.target.y, self.radius].iter().all(|v| v.is_finite())
    }
}

/// `retouch(sx,sy,tx,ty,r;…)`: the strokes in the order they were made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Retouch {
    pub strokes: Vec<Stroke>,
}

impl fmt::Display for Retouch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, s) in self.strokes.iter().enumerate() {
            if i > 0 {
                f.write_str(";")?;
            }
            write!(f, "{},{},{},{},{}", number(s.source.x), number(s.source.y), number(s.target.x), number(s.target.y), number(s.radius))?;
        }
        Ok(())
    }
}

impl Operation for Retouch {
    const NAME: &'static str = "retouch";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Retouch";

    fn parse(params: &str) -> Result<Retouch, ParamError> {
        let refuse = |why: &str| ParamError::new(Self::NAME, params, why);
        if params.is_empty() {
            return Ok(Retouch::default());
        }
        if params.split(';').count() > MAX_STROKES {
            return Err(refuse("holds more strokes than one entry may"));
        }
        let mut strokes = Vec::new();
        for text in params.split(';') {
            let values: Option<Vec<f64>> = text.split(',').map(read_number).collect();
            let Some([sx, sy, tx, ty, r]) = values.as_deref().and_then(|v| <[f64; 5]>::try_from(v).ok()) else {
                return Err(refuse("each stroke takes five finite numbers"));
            };
            strokes.push(Stroke { source: Point::new(sx, sy), target: Point::new(tx, ty), radius: r });
        }
        Ok(Retouch { strokes }.clamped())
    }

    fn clamped(self) -> Retouch {
        Retouch { strokes: self.strokes.into_iter().map(Stroke::clamped).collect() }
    }
}

impl Adjust for Retouch {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let (w, h) = (f64::from(ctx.width), f64::from(ctx.height));
        let out = |p: Point| ctx.geometry.stored_to_output(p).map(|q| (q.x * w, q.y * h));
        // The stored frame's sides in output pixels (the scale and the straighten's enlargement).
        let o = out(Point::new(0.0, 0.0))?;
        let ex = out(Point::new(1.0, 0.0))?;
        let ey = out(Point::new(0.0, 1.0))?;
        let short = (ex.0 - o.0).hypot(ex.1 - o.1).min((ey.0 - o.0).hypot(ey.1 - o.1));
        let mut patches: Vec<Patch> = Vec::new();
        // How far from a pixel of each patch's box its result reads, through the earlier patches
        // it reads from.
        let mut chains: Vec<f64> = Vec::new();
        for s in self.strokes.iter().map(|s| s.clamped()).filter(Stroke::is_finite) {
            let (Some(t), Some(src)) = (out(s.target), out(s.source)) else { continue };
            let r = (s.radius * short).max(1.0);
            let d = (src.0 - t.0, src.1 - t.1);
            let p = Patch { t, d, r };
            if !p.is_finite() {
                continue;
            }
            // A pixel in the box reads the whole box (the tone's blur) and the box moved to the
            // source (bilinear taps): at most across the box, plus the offset, plus the taps. Where
            // it reads, an earlier patch may have drawn, which read further in turn.
            let own = 2.0 * BOX_RADII * r + d.0.hypot(d.1) + 4.0;
            let earlier = patches.iter().zip(&chains).filter(|(q, _)| p.reads_from(q)).map(|(_, e)| *e).fold(0.0, f64::max);
            chains.push(own + earlier);
            patches.push(p);
        }
        if patches.is_empty() {
            return None;
        }
        let far = chains.iter().copied().fold(0.0, f64::max).ceil();
        let reach = if far < f64::from(u32::MAX) { far as u32 } else { u32::MAX };
        Some(Box::new(Heal { patches, reach, full: (ctx.width, ctx.height), done: OnceLock::new() }))
    }
}

/// A stroke in output pixels: the target centre, the offset to the source, the radius.
#[derive(Clone, Copy, Debug)]
struct Patch {
    t: (f64, f64),
    d: (f64, f64),
    r: f64,
}

impl Patch {
    fn is_finite(&self) -> bool {
        [self.t.0, self.t.1, self.d.0, self.d.1, self.r].iter().all(|v| v.is_finite())
    }

    /// Whether this patch reads pixels `earlier` may have drawn: `earlier`'s box meets this box
    /// or the box moved to the source (with a pixel's slack for rounding and the bilinear taps).
    fn reads_from(&self, earlier: &Patch) -> bool {
        let e = BOX_RADII * self.r + 3.0;
        let f = BOX_RADII * earlier.r + 1.0;
        let meets = |cx: f64, cy: f64| (cx - earlier.t.0).abs() <= e + f && (cy - earlier.t.1).abs() <= e + f;
        meets(self.t.0, self.t.1) || meets(self.t.0 + self.d.0, self.t.1 + self.d.1)
    }

    /// The box the patch works in, in whole-output pixels, cut to `within`.
    fn bounds(&self, within: Rect) -> Option<Rect> {
        let e = BOX_RADII * self.r;
        let lo = |v: f64, min: u32| -> u32 { if v <= f64::from(min) { min } else { v.floor().min(f64::from(u32::MAX)) as u32 } };
        let hi = |v: f64, max: u32| -> u32 { if v >= f64::from(max) { max } else { v.ceil().max(0.0) as u32 } };
        let b = Rect {
            x0: lo(self.t.0 - e, within.x0),
            y0: lo(self.t.1 - e, within.y0),
            x1: hi(self.t.0 + e, within.x1),
            y1: hi(self.t.1 + e, within.y1),
        };
        (b.x0 < b.x1 && b.y0 < b.y1).then_some(b)
    }
}

/// A rectangle of whole-output pixels, `[x0, x1) × [y0, y1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl Rect {
    fn width(&self) -> usize {
        self.x1.saturating_sub(self.x0) as usize
    }

    fn height(&self) -> usize {
        self.y1.saturating_sub(self.y0) as usize
    }

    fn union(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    fn contains(&self, x: i64, y: i64) -> bool {
        x >= i64::from(self.x0) && x < i64::from(self.x1) && y >= i64::from(self.y0) && y < i64::from(self.y1)
    }
}

/// The prepared strokes, healed once per render (by the first band to run) into `done`, through
/// [`apart`] like every one-time setup inside a band (sequential today, so it could not hang, but
/// a later parallel heal cannot either; it measured no slower).
struct Heal {
    patches: Vec<Patch>,
    reach: u32,
    full: (u32, u32),
    done: OnceLock<Option<Healed>>,
}

/// The pixels the strokes changed: the rectangle `at` (the union of the patches' boxes in the
/// rendered window) after every stroke.
struct Healed {
    at: Rect,
    pixels: Vec<[f32; 3]>,
}

impl Healed {
    fn get(&self, x: u32, y: u32) -> Option<[f32; 3]> {
        let (lx, ly) = (x.checked_sub(self.at.x0)? as usize, y.checked_sub(self.at.y0)? as usize);
        if lx >= self.at.width() {
            return None;
        }
        self.pixels.get(ly.checked_mul(self.at.width())?.checked_add(lx)?).copied()
    }
}

impl Pass for Heal {
    fn reach(&self) -> Reach {
        Reach::Pixels(self.reach)
    }

    fn run(&self, band: &mut Band<'_>) {
        let Some(view) = band.before else { return };
        let Some(healed) = self.done.get_or_init(|| apart(|| self.heal(view))) else { return };
        let (y0, y1) = (band.y0.max(healed.at.y0), band.y0.saturating_add(band.rows).min(healed.at.y1));
        let (x0, x1) = (band.x0.max(healed.at.x0), band.x0.saturating_add(band.width).min(healed.at.x1));
        for y in y0..y1 {
            for x in x0..x1 {
                if let (Some(v), Some(px)) = (healed.get(x, y), band.get_mut(x - band.x0, y - band.y0)) {
                    *px = v;
                }
            }
        }
    }
}

impl Heal {
    /// Every stroke in order, on the window `view` holds: the strokes whose box reaches into it,
    /// each reading the picture the strokes before it left. `None` when no stroke touches it (or
    /// its buffer cannot be had).
    fn heal(&self, view: &View<'_>) -> Option<Healed> {
        let window = Rect {
            x0: view.x0,
            y0: view.y0,
            x1: view.x0.saturating_add(view.width).min(self.full.0),
            y1: view.y0.saturating_add(view.height).min(self.full.1),
        };
        // Boxes are cut to the picture first, then to the window, so a patch that matters to the
        // window has the same box in every render (the reach puts all of it in the window).
        let picture = Rect { x0: 0, y0: 0, x1: self.full.0, y1: self.full.1 };
        let boxes: Vec<(Patch, Rect)> = self
            .patches
            .iter()
            .filter_map(|p| {
                let b = p.bounds(picture)?;
                let cut = Rect { x0: b.x0.max(window.x0), y0: b.y0.max(window.y0), x1: b.x1.min(window.x1), y1: b.y1.min(window.y1) };
                (cut.x0 < cut.x1 && cut.y0 < cut.y1).then_some((*p, cut))
            })
            .collect();
        let at = boxes.iter().map(|(_, b)| *b).reduce(Rect::union)?;
        let n = at.width().checked_mul(at.height())?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(n).ok()?;
        for y in at.y0..at.y1 {
            for x in at.x0..at.x1 {
                pixels.push(view.get(i64::from(x), i64::from(y)));
            }
        }
        let mut canvas = Canvas { view, full: self.full, healed: Healed { at, pixels } };
        for (p, b) in &boxes {
            canvas.stroke(p, *b)?;
        }
        Some(canvas.healed)
    }
}

/// The picture as the strokes so far left it: the changed rectangle, the window elsewhere.
struct Canvas<'v, 'a> {
    view: &'v View<'a>,
    full: (u32, u32),
    healed: Healed,
}

impl Canvas<'_, '_> {
    /// The pixel at `(x, y)` of the whole output, clamped into the picture.
    fn get(&self, x: i64, y: i64) -> [f32; 3] {
        let cx = x.clamp(0, i64::from(self.full.0.max(1)) - 1);
        let cy = y.clamp(0, i64::from(self.full.1.max(1)) - 1);
        if self.healed.at.contains(cx, cy)
            && let Some(v) = self.healed.get(cx as u32, cy as u32)
        {
            return v;
        }
        self.view.get(cx, cy)
    }

    /// The picture at the continuous point `(x, y)` (a pixel's centre at `+ 0.5`), bilinear.
    fn sample(&self, x: f64, y: f64) -> [f32; 3] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let (ix, iy) = (clamp_i64(x0), clamp_i64(y0));
        let a = self.get(ix, iy);
        let b = self.get(ix.saturating_add(1), iy);
        let c = self.get(ix, iy.saturating_add(1));
        let d = self.get(ix.saturating_add(1), iy.saturating_add(1));
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * tx;
            let bot = c[i] + (d[i] - c[i]) * tx;
            top + (bot - top) * ty
        })
    }

    /// Heals one patch inside its box `b` (which lies in the changed rectangle).
    fn stroke(&mut self, p: &Patch, b: Rect) -> Option<()> {
        let (bw, bh) = (b.width(), b.height());
        let n = bw.checked_mul(bh)?;
        let r = p.r as f32;
        let mut alpha = alloc(n, 0.0f32)?;
        let mut src = alloc(n, [0.0f32; 3])?;
        let mut tgt = alloc(n, [0.0f32; 3])?;
        for (i, (y, x)) in (b.y0..b.y1).flat_map(|y| (b.x0..b.x1).map(move |x| (y, x))).enumerate() {
            let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let dist = (px - p.t.0).hypot(py - p.t.1) as f32;
            if let (Some(a), Some(s), Some(t)) = (alpha.get_mut(i), src.get_mut(i), tgt.get_mut(i)) {
                *a = 1.0 - smooth(INNER * r, r, dist);
                *s = self.sample(px + p.d.0, py + p.d.1);
                *t = self.get(i64::from(x), i64::from(y));
            }
        }
        // The tone from the patch's surroundings only: a normalised blur of (target − source) over
        // the pixels outside the stroke.
        let outside: Vec<f32> = alpha.iter().map(|a| if *a < 0.02 { 1.0 } else { 0.0 }).collect();
        let mut diff: Vec<[f32; 3]> =
            tgt.iter().zip(&src).zip(&outside).map(|((t, s), m)| [(t[0] - s[0]) * m, (t[1] - s[1]) * m, (t[2] - s[2]) * m]).collect();
        let mut weight: Vec<[f32; 3]> = outside.iter().map(|m| [*m; 3]).collect();
        let sigma = (r * 0.6).max(1.0);
        gaussian(&mut diff, bw, bh, sigma);
        gaussian(&mut weight, bw, bh, sigma);
        let at = self.healed.at;
        for (i, (y, x)) in (b.y0..b.y1).flat_map(|y| (b.x0..b.x1).map(move |x| (y, x))).enumerate() {
            let (Some(&a), Some(s), Some(t), Some(dl), Some(m)) = (alpha.get(i), src.get(i), tgt.get(i), diff.get(i), weight.get(i)) else {
                continue;
            };
            if a <= 0.0 {
                continue;
            }
            let m = m[0].max(1e-4);
            let v: [f32; 3] = std::array::from_fn(|c| {
                let healed = s[c] + dl[c] / m;
                (t[c] + (healed - t[c]) * a).clamp(0.0, 1.0)
            });
            let li = (y - at.y0) as usize * at.width() + (x - at.x0) as usize;
            if let Some(px) = self.healed.pixels.get_mut(li) {
                *px = v;
            }
        }
        Some(())
    }
}

fn clamp_i64(v: f64) -> i64 {
    if v.is_nan() { 0 } else { v.clamp(-1e15, 1e15) as i64 }
}

fn alloc<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Box radii approximating a Gaussian of `sigma` with three passes (Kovesi / Wells).
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0);
    let m = m_ideal.round() as i32;
    std::array::from_fn(|i| (((if (i as i32) < m { wl } else { wu }) - 1) / 2).max(0) as usize)
}

/// A Gaussian-like blur of the `w × h` plane `img` in place: three box passes along each axis, the
/// edges clamped.
fn gaussian(img: &mut [[f32; 3]], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.3 || w == 0 || h == 0 || Some(img.len()) != w.checked_mul(h) {
        return;
    }
    let radii = box_radii(sigma);
    let mut tmp = Vec::new();
    for r in radii {
        for row in img.chunks_mut(w) {
            box_line(row, &mut tmp, r);
        }
    }
    let mut col = Vec::new();
    for r in radii {
        if r == 0 {
            continue;
        }
        for x in 0..w {
            col.clear();
            col.extend((0..h).filter_map(|y| img.get(y * w + x).copied()));
            box_line(&mut col, &mut tmp, r);
            for (y, v) in col.iter().enumerate() {
                if let Some(px) = img.get_mut(y * w + x) {
                    *px = *v;
                }
            }
        }
    }
}

/// One box pass of radius `r` along `line` (edges clamped), `tmp` as scratch: running sums.
fn box_line(line: &mut [[f32; 3]], tmp: &mut Vec<[f32; 3]>, r: usize) {
    let n = line.len();
    if r == 0 || n == 0 {
        return;
    }
    tmp.clear();
    tmp.extend_from_slice(line);
    let last = n - 1;
    let at = |i: usize| tmp.get(i.min(last)).copied().unwrap_or([0.0; 3]);
    let inv = 1.0 / (2 * r + 1) as f32;
    let mut acc = [0.0f32; 3];
    for i in 0..=2 * r {
        let v = at(i.saturating_sub(r));
        acc = [acc[0] + v[0], acc[1] + v[1], acc[2] + v[2]];
    }
    for (x, out) in line.iter_mut().enumerate() {
        *out = [acc[0] * inv, acc[1] * inv, acc[2] * inv];
        let (add, sub) = (at(x + r + 1), at(x.saturating_sub(r)));
        acc = [acc[0] + add[0] - sub[0], acc[1] + add[1] - sub[1], acc[2] + add[2] - sub[2]];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_radii_approximate_the_gaussian() {
        for sigma in [1.0f32, 2.5, 7.0, 30.0] {
            // Three boxes of radius r have the variance Σ r(r + 1)/3.
            let var: f32 = box_radii(sigma).iter().map(|r| (*r * (r + 1)) as f32 / 3.0).sum();
            assert!((var.sqrt() - sigma).abs() < 0.6, "{sigma}: {}", var.sqrt());
        }
    }

    #[test]
    fn the_blur_keeps_a_flat_plane_and_spreads_a_dot() {
        let mut flat = vec![[0.25f32; 3]; 9 * 7];
        gaussian(&mut flat, 9, 7, 2.0);
        assert!(flat.iter().flatten().all(|v| (v - 0.25).abs() < 1e-6));
        let mut dot = vec![[0.0f32; 3]; 21 * 21];
        dot[10 * 21 + 10] = [1.0; 3];
        gaussian(&mut dot, 21, 21, 2.0);
        let sum: f32 = dot.iter().map(|p| p[0]).sum();
        assert!((sum - 1.0).abs() < 1e-4, "{sum}");
        assert!(dot[10 * 21 + 10][0] > dot[10 * 21 + 13][0] && dot[10 * 21 + 13][0] > 0.0);
    }
}
