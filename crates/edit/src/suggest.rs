//! Suggested crops (`editing.md` §6.1): three proposed rectangles in a ratio, from a proxy of the
//! current picture. Our own method, on a small analysis grid (the proxy box-averaged so that its
//! longer side is at most [`GRID_EDGE`] cells):
//!
//! - **Subject.** A saliency map adds each cell's colour distinctness (its distance, in an
//!   opponent colour space, from the picture's median colour) to half its local contrast (its
//!   lightness against a wide blur). The connected region around the strongest peak (cells over
//!   half the peak) is the subject, unless the picture is flat (the peak is under [`FLAT`]) or the
//!   region is background rather than a subject (more than 40 % of the picture, or spanning nine
//!   tenths of its width or height). The subject's weighted centre is put on a thirds point of a
//!   rectangle of the ratio, choosing the thirds point and the size (the largest fit down to 60 %
//!   of it) that land it closest while keeping the region inside.
//! - **Horizon.** The strongest long, near-horizontal lightness edge (a line across the whole
//!   width at a slope of at most about 7°, along which at least 60 % of the columns step the same
//!   way) is put on the upper or lower third line, across the subject when there is one.
//! - **Centred.** The largest centred rectangle of the ratio ([`crop_geom::ratio_rect`]).
//! - **Faces.** When the photo has faces (their rectangles given by the caller, mapped to the
//!   picture shown), a rectangle holding every face, the faces' eyes (about [`EYES`] down each face,
//!   the faces' mean weighted by their areas) on its upper third line and the faces centred across
//!   it, choosing the size (the largest fit down to 60 % of it) that lands the eyes closest while
//!   keeping every face inside.
//!
//! They are ranked faces, subject, horizon, centred; then the subject on another thirds point and
//! smaller centred rectangles fill the three places. Every rectangle kept overlaps every other by
//! at most [`MAX_OVERLAP`] (intersection over union), so the three are distinct. The method is
//! deterministic: the same proxy, size, ratio and faces give the same rectangles, and without faces
//! the faces' rectangle is simply not there.
//!
//! Rectangles are normalised to the picture the proxy shows (`0..1`, top-left origin); the caller
//! maps them to the stored frame (`data-model.md` §3.3) as it does any rectangle of the displayed
//! picture.
//!
//! [`crop_geom::ratio_rect`]: crate::crop_geom::ratio_rect

use dalica_image::Rgba8;

use crate::crop_geom::ratio_rect;
use crate::{Error, Rect, Result};

/// How many rectangles [`suggest`] proposes.
pub const COUNT: usize = 3;

/// The longer side of the analysis grid, in cells (smaller proxies are used as they are).
pub const GRID_EDGE: u32 = 96;

/// The largest intersection over union two kept suggestions may have.
pub const MAX_OVERLAP: f64 = 0.8;

/// Below this saliency peak (colour units 0–1) the picture is flat: no subject.
pub const FLAT: f32 = 0.06;

/// The sizes tried for a placed rectangle, as fractions of the ratio's largest fit.
const SCALES: [f64; 9] = [1.0, 0.95, 0.9, 0.85, 0.8, 0.75, 0.7, 0.65, 0.6];

/// How far down a face's rectangle its eyes lie, as a fraction of its height.
pub const EYES: f64 = 0.4;

/// The most faces [`suggest`] looks at (a photo holds at most 256).
pub const MAX_FACES: usize = 256;

/// The sizes of the centred rectangles, largest first.
const CENTRED: [f64; 6] = [1.0, 0.8, 0.64, 0.5, 0.4, 0.32];

/// Why a rectangle was proposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Every face inside, their eyes on the upper third line.
    Faces,
    /// The salient subject sits on a thirds point.
    Subject,
    /// The horizon lies on a third line.
    Horizon,
    /// Centred on the picture.
    Centred,
}

/// One proposed crop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Suggestion {
    /// Normalised to the picture, inside `0..1`, of the asked ratio on a `width × height` picture.
    pub rect: Rect,
    pub reason: Reason,
}

/// What the analysis found, in normalised picture coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Analysis {
    pub subject: Option<Subject>,
    pub horizon: Option<Horizon>,
}

/// The salient region: its weighted centre and its bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subject {
    pub x: f64,
    pub y: f64,
    pub bounds: Rect,
}

/// A near-horizontal line: `y` at the picture's centre column, `slope` in normalised `y` per
/// normalised `x`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Horizon {
    pub y: f64,
    pub slope: f64,
}

impl Horizon {
    /// The line's height at `x`.
    pub fn y_at(&self, x: f64) -> f64 {
        self.y + self.slope * (x - 0.5)
    }
}

/// Three distinct rectangles of ratio `aspect` (width / height in pixels) for a picture of
/// `width × height` pixels, of which `proxy` is a (usually smaller) rendering, best first.
/// `faces` are the photo's face rectangles normalised to that picture (`0..1`, top-left origin);
/// a rectangle that is not finite or holds nothing inside the picture is left out, and so is every
/// one past the first [`MAX_FACES`]. `Err(BadImage)` for a buffer that is not
/// `proxy.width × proxy.height` RGBA pixels or an empty proxy; `Err(BadTarget)` for an empty
/// picture size or a ratio that is not finite and positive.
pub fn suggest(proxy: &Rgba8, width: u32, height: u32, aspect: f64, faces: &[Rect]) -> Result<[Suggestion; COUNT]> {
    if width == 0 || height == 0 {
        return Err(Error::BadTarget("an empty picture"));
    }
    if !aspect.is_finite() || aspect <= 0.0 {
        return Err(Error::BadTarget("a crop ratio must be finite and positive"));
    }
    let found = analyse(proxy)?;
    let faces: Vec<Rect> = faces.iter().take(MAX_FACES).filter_map(|r| clean_face(*r)).collect();
    Ok(place(&found, &faces, f64::from(width), f64::from(height), aspect))
}

/// A face rectangle inside the picture, corners in order; `None` when it is not finite or holds
/// nothing inside the picture.
fn clean_face(r: Rect) -> Option<Rect> {
    if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) {
        return None;
    }
    let c = |v: f64| v.clamp(0.0, 1.0);
    let r = Rect::new(c(r.x0.min(r.x1)), c(r.y0.min(r.y1)), c(r.x0.max(r.x1)), c(r.y0.max(r.y1)));
    (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
}

/// The subject and horizon [`suggest`] works from. `Err(BadImage)` as for [`suggest`].
pub fn analyse(proxy: &Rgba8) -> Result<Analysis> {
    let grid = Grid::of(proxy)?;
    Ok(Analysis { subject: find_subject(&grid), horizon: find_horizon(&grid) })
}

// --- placing ---------------------------------------------------------------------------------

/// A rectangle in picture pixels.
#[derive(Clone, Copy, Debug)]
struct Px {
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
}

impl Px {
    fn to_rect(self, pic: Size) -> Rect {
        let c = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        Rect::new(c(self.x0 / pic.w), c(self.y0 / pic.h), c((self.x0 + self.w) / pic.w), c((self.y0 + self.h) / pic.h))
    }
}

/// A width and height in pixels.
#[derive(Clone, Copy, Debug)]
struct Size {
    w: f64,
    h: f64,
}

impl Size {
    fn scaled(self, s: f64) -> Size {
        Size { w: self.w * s, h: self.h * s }
    }
}

fn place(found: &Analysis, faces: &[Rect], pw: f64, ph: f64, aspect: f64) -> [Suggestion; COUNT] {
    let pic = Size { w: pw, h: ph };
    let r = ratio_rect(pw, ph, aspect);
    let fit = Size { w: ((r.x1 - r.x0) * pw).max(f64::MIN_POSITIVE), h: ((r.y1 - r.y0) * ph).max(f64::MIN_POSITIVE) };
    let centred = |s: f64| {
        let z = fit.scaled(s);
        Px { x0: (pw - z.w) / 2.0, y0: (ph - z.h) / 2.0, w: z.w, h: z.h }
    };

    // Candidates in rank order.
    let mut ranked: Vec<(Px, Reason)> = Vec::new();
    if let Some(px) = face_crop(faces, pic, fit) {
        ranked.push((px, Reason::Faces));
    }
    let subjects = found.subject.map(|s| subject_crops(&s, pic, fit)).unwrap_or_default();
    if let Some(first) = subjects.first() {
        ranked.push((*first, Reason::Subject));
    }
    if let Some(h) = found.horizon {
        ranked.push((horizon_crop(&h, found.subject.as_ref(), pic, fit), Reason::Horizon));
    }
    ranked.push((centred(1.0), Reason::Centred));
    ranked.extend(subjects.iter().skip(1).map(|p| (*p, Reason::Subject)));
    ranked.extend(CENTRED.iter().skip(1).map(|s| (centred(*s), Reason::Centred)));

    let mut kept: Vec<Suggestion> = Vec::with_capacity(COUNT);
    let keep = |kept: &mut Vec<Suggestion>, rect: Rect, reason: Reason| {
        if kept.len() < COUNT && kept.iter().all(|k| iou(k.rect, rect) <= MAX_OVERLAP) {
            kept.push(Suggestion { rect, reason });
        }
    };
    for (px, reason) in ranked {
        keep(&mut kept, px.to_rect(pic), reason);
    }
    // Ever smaller centred rectangles overlap less and less: this always fills the places.
    let mut s = CENTRED.last().copied().unwrap_or(0.32);
    while kept.len() < COUNT && s > 1e-6 {
        s *= 0.8;
        keep(&mut kept, centred(s).to_rect(pic), Reason::Centred);
    }
    let fallback = Suggestion { rect: centred(1.0).to_rect(pic), reason: Reason::Centred };
    let at = |i: usize| kept.get(i).copied().unwrap_or(fallback);
    [at(0), at(1), at(2)]
}

/// A rectangle of `size` placed so that `point` (pixels) sits at `anchor` (a fraction of it),
/// then moved inside the picture.
fn put(point: (f64, f64), anchor: (f64, f64), size: Size, pic: Size) -> Px {
    let x0 = (point.0 - anchor.0 * size.w).clamp(0.0, (pic.w - size.w).max(0.0));
    let y0 = (point.1 - anchor.1 * size.h).clamp(0.0, (pic.h - size.h).max(0.0));
    Px { x0, y0, w: size.w, h: size.h }
}

const THIRDS: [(f64, f64); 4] = [(1.0 / 3.0, 1.0 / 3.0), (2.0 / 3.0, 1.0 / 3.0), (1.0 / 3.0, 2.0 / 3.0), (2.0 / 3.0, 2.0 / 3.0)];

/// How far `point` (pixels) is from the nearest thirds point of `r`, in units of `r`'s sides.
fn thirds_error(r: Px, point: (f64, f64)) -> f64 {
    let (u, v) = ((point.0 - r.x0) / r.w, (point.1 - r.y0) / r.h);
    THIRDS.iter().map(|(tx, ty)| (u - tx).hypot(v - ty)).fold(f64::INFINITY, f64::min)
}

/// The share of the normalised `bounds` outside `r`.
fn outside(r: Px, bounds: Rect, pic: Size) -> f64 {
    let (bx0, by0, bx1, by1) = (bounds.x0 * pic.w, bounds.y0 * pic.h, bounds.x1 * pic.w, bounds.y1 * pic.h);
    let area = (bx1 - bx0).max(0.0) * (by1 - by0).max(0.0);
    if area <= 0.0 {
        return 0.0;
    }
    let ix = (bx1.min(r.x0 + r.w) - bx0.max(r.x0)).max(0.0);
    let iy = (by1.min(r.y0 + r.h) - by0.max(r.y0)).max(0.0);
    (1.0 - ix * iy / area).clamp(0.0, 1.0)
}

/// How good a placement is: larger is better, a subject or line off its third costs most, a
/// subject cut off next.
fn score(scale: f64, off_third: f64, cut: f64) -> f64 {
    0.5 * scale - 4.0 * off_third - 2.0 * cut
}

/// Subject placements, best first: for each thirds point its best size.
fn subject_crops(s: &Subject, pic: Size, fit: Size) -> Vec<Px> {
    let point = (s.x * pic.w, s.y * pic.h);
    let mut best: Vec<(f64, Px)> = Vec::with_capacity(THIRDS.len());
    for anchor in THIRDS {
        let mut top: Option<(f64, Px)> = None;
        for sc in SCALES {
            let r = put(point, anchor, fit.scaled(sc), pic);
            let v = score(sc, thirds_error(r, point), outside(r, s.bounds, pic));
            if top.is_none_or(|(t, _)| v > t) {
                top = Some((v, r));
            }
        }
        best.extend(top);
    }
    // A stable sort keeps the thirds points' order between equal scores: deterministic.
    best.sort_by(|a, b| b.0.total_cmp(&a.0));
    best.into_iter().map(|(_, r)| r).collect()
}

/// Every face inside, their eyes on the upper third line, centred across: for each size, the
/// placement that puts them there, the best one kept (`None` without faces).
fn face_crop(faces: &[Rect], pic: Size, fit: Size) -> Option<Px> {
    let first = faces.first()?;
    // The faces' bounds, and their eyes' height and centre weighted by their areas.
    let mut bounds = *first;
    let (mut mass, mut eyes, mut across) = (0.0f64, 0.0f64, 0.0f64);
    for f in faces {
        bounds = Rect::new(bounds.x0.min(f.x0), bounds.y0.min(f.y0), bounds.x1.max(f.x1), bounds.y1.max(f.y1));
        let area = ((f.x1 - f.x0) * pic.w * (f.y1 - f.y0) * pic.h).max(f64::MIN_POSITIVE);
        mass += area;
        eyes += area * (f.y0 + EYES * (f.y1 - f.y0)) * pic.h;
        across += area * (f.x0 + f.x1) / 2.0 * pic.w;
    }
    let (ex, ey) = (across / mass, eyes / mass);
    // Centred across the faces' bounds; their weighted centre when the bounds are wider than the
    // rectangle, so the larger faces stay.
    let cx = (bounds.x0 + bounds.x1) / 2.0 * pic.w;
    let mut best: Option<(f64, Px)> = None;
    for sc in SCALES {
        let size = fit.scaled(sc);
        let x = if (bounds.x1 - bounds.x0) * pic.w <= size.w { cx } else { ex };
        let r = put((x, ey), (0.5, 1.0 / 3.0), size, pic);
        let off = ((ey - r.y0) / size.h - 1.0 / 3.0).abs();
        let cut = faces.iter().map(|f| outside(r, *f, pic)).fold(0.0, f64::max);
        // A face cut off costs more than a subject: the crop is for the faces.
        let v = score(sc, off, 2.0 * cut);
        if best.is_none_or(|(b, _)| v > b) {
            best = Some((v, r));
        }
    }
    best.map(|(_, r)| r)
}

/// The horizon on the upper or lower third line, across the subject when there is one.
fn horizon_crop(h: &Horizon, subject: Option<&Subject>, pic: Size, fit: Size) -> Px {
    let cx = subject.map_or(pic.w / 2.0, |s| s.x * pic.w);
    let mut best: Option<(f64, Px)> = None;
    for t in [1.0 / 3.0, 2.0 / 3.0] {
        for sc in SCALES {
            let size = fit.scaled(sc);
            // The line's height under the rectangle's centre, which depends on where it lands.
            let first = put((cx, h.y_at(0.5) * pic.h), (0.5, t), size, pic);
            let mid = first.x0 + size.w / 2.0;
            let hy = h.y_at(mid / pic.w) * pic.h;
            let r = put((mid, hy), (0.5, t), size, pic);
            let off = ((hy - r.y0) / size.h - t).abs();
            let cut = subject.map_or(0.0, |s| outside(r, s.bounds, pic));
            let v = score(sc, off, cut);
            if best.is_none_or(|(b, _)| v > b) {
                best = Some((v, r));
            }
        }
    }
    best.map_or(Px { x0: (pic.w - fit.w) / 2.0, y0: (pic.h - fit.h) / 2.0, w: fit.w, h: fit.h }, |(_, r)| r)
}

/// Intersection over union of two normalised rectangles (1 when both are empty).
fn iou(a: Rect, b: Rect) -> f64 {
    let area = |r: Rect| (r.x1 - r.x0).max(0.0) * (r.y1 - r.y0).max(0.0);
    let ix = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let iy = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let inter = ix * iy;
    let union = area(a) + area(b) - inter;
    if union > 0.0 { inter / union } else { 1.0 }
}

// --- the analysis grid -----------------------------------------------------------------------

/// The proxy box-averaged into at most `GRID_EDGE` cells on its longer side, in an opponent
/// colour space: lightness `(r + g + b) / 3`, red–green `(r − g) / 2`, yellow–blue
/// `(r + g) / 4 − b / 2` (sRGB values 0–1; alpha ignored).
struct Grid {
    w: usize,
    h: usize,
    /// Lightness, red–green, yellow–blue; `w × h` each.
    planes: [Vec<f32>; 3],
}

impl Grid {
    fn of(img: &Rgba8) -> Result<Grid> {
        let bad = || Error::BadImage { width: img.width, height: img.height, len: img.data.len() };
        let (iw, ih) = (img.width as usize, img.height as usize);
        let len = iw.checked_mul(ih).and_then(|n| n.checked_mul(4)).ok_or_else(bad)?;
        if len != img.data.len() || len == 0 {
            return Err(bad());
        }
        let long = iw.max(ih);
        let edge = (GRID_EDGE as usize).min(long);
        // At most `long × GRID_EDGE`, far inside `usize` for any buffer that fits in memory.
        let cells = |n: usize| (n.saturating_mul(edge).saturating_add(long / 2) / long).clamp(1, edge);
        let (w, h) = (cells(iw), cells(ih));
        // Which cell each source column and row falls in.
        let col: Vec<usize> = (0..iw).map(|x| (x.saturating_mul(w) / iw).min(w - 1)).collect();
        let mut sums = vec![[0u64; 3]; w * h];
        let mut counts = vec![0u64; w * h];
        for (y, row) in img.data.chunks_exact(iw * 4).enumerate() {
            let cy = (y.saturating_mul(h) / ih).min(h - 1);
            for (x, p) in row.as_chunks::<4>().0.iter().enumerate() {
                let cell = cy * w + col.get(x).copied().unwrap_or(0);
                if let (Some(s), Some(c), [r, g, b, _]) = (sums.get_mut(cell), counts.get_mut(cell), p) {
                    s[0] += u64::from(*r);
                    s[1] += u64::from(*g);
                    s[2] += u64::from(*b);
                    *c += 1;
                }
            }
        }
        let mut planes = [vec![0.0f32; w * h], vec![0.0f32; w * h], vec![0.0f32; w * h]];
        let [lp, ap, bp] = &mut planes;
        for (i, (s, n)) in sums.iter().zip(&counts).enumerate() {
            let n = (*n).max(1) as f64 * 255.0;
            let (r, g, b) = ((s[0] as f64 / n) as f32, (s[1] as f64 / n) as f32, (s[2] as f64 / n) as f32);
            if let (Some(l), Some(a), Some(yb)) = (lp.get_mut(i), ap.get_mut(i), bp.get_mut(i)) {
                *l = (r + g + b) / 3.0;
                *a = (r - g) / 2.0;
                *yb = (r + g) / 4.0 - b / 2.0;
            }
        }
        Ok(Grid { w, h, planes })
    }
}

/// A box blur of radius `r` cells over a `w × h` plane (at the edges, the mean of the cells that
/// exist), rows then columns.
fn blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let pass = |src: &[f32], len: usize, lines: usize, at: &dyn Fn(usize, usize) -> usize| {
        let mut out = vec![0.0f32; src.len()];
        let mut prefix = vec![0.0f64; len + 1];
        for line in 0..lines {
            for i in 0..len {
                let v = f64::from(src.get(at(line, i)).copied().unwrap_or(0.0));
                let before = prefix.get(i).copied().unwrap_or(0.0);
                if let Some(p) = prefix.get_mut(i + 1) {
                    *p = before + v;
                }
            }
            for i in 0..len {
                let (lo, hi) = (i.saturating_sub(r), (i + r + 1).min(len));
                let sum = prefix.get(hi).copied().unwrap_or(0.0) - prefix.get(lo).copied().unwrap_or(0.0);
                if let Some(o) = out.get_mut(at(line, i)) {
                    *o = (sum / (hi - lo).max(1) as f64) as f32;
                }
            }
        }
        out
    };
    let rows = pass(src, w, h, &|y, x| y * w + x);
    pass(&rows, h, w, &|x, y| y * w + x)
}

fn median(v: &[f32]) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s.get(s.len() / 2).copied().unwrap_or(0.0)
}

fn find_subject(g: &Grid) -> Option<Subject> {
    let (w, h) = (g.w, g.h);
    let n = w * h;
    let fine: Vec<Vec<f32>> = g.planes.iter().map(|p| blur(p, w, h, 1)).collect();
    let bg: Vec<f32> = g.planes.iter().map(|p| median(p)).collect();
    let wide = blur(fine.first()?, w, h, (w.max(h) / 8).max(2));
    let mut sal = vec![0.0f32; n];
    for (i, s) in sal.iter_mut().enumerate() {
        let c = |p: usize| fine.get(p).and_then(|v| v.get(i)).copied().unwrap_or(0.0);
        let b = |p: usize| bg.get(p).copied().unwrap_or(0.0);
        let distinct = ((c(0) - b(0)).powi(2) + (c(1) - b(1)).powi(2) + (c(2) - b(2)).powi(2)).sqrt();
        let contrast = (c(0) - wide.get(i).copied().unwrap_or(0.0)).abs();
        *s = distinct + 0.5 * contrast;
    }
    let sal = blur(&sal, w, h, 1);
    let (peak_at, peak) = sal.iter().copied().enumerate().fold((0, 0.0f32), |a, (i, v)| if v > a.1 { (i, v) } else { a });
    if peak < FLAT {
        return None;
    }
    // The connected region over half the peak, grown from the peak (4-neighbours).
    let floor = peak * 0.5;
    let mut seen = vec![false; n];
    let mut stack = vec![peak_at];
    if let Some(s) = seen.get_mut(peak_at) {
        *s = true;
    }
    let (mut mass, mut mx, mut my, mut area) = (0.0f64, 0.0f64, 0.0f64, 0usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        let m = f64::from(sal.get(i).copied().unwrap_or(0.0));
        mass += m;
        mx += m * (x as f64 + 0.5);
        my += m * (y as f64 + 0.5);
        area += 1;
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
        let left = if x > 0 { i.checked_sub(1) } else { None };
        let right = if x + 1 < w { Some(i + 1) } else { None };
        let up = if y > 0 { i.checked_sub(w) } else { None };
        let down = if y + 1 < h { Some(i + w) } else { None };
        for j in [left, right, up, down].into_iter().flatten() {
            let fresh = seen.get(j) == Some(&false) && sal.get(j).is_some_and(|v| *v >= floor);
            if fresh {
                if let Some(s) = seen.get_mut(j) {
                    *s = true;
                }
                stack.push(j);
            }
        }
    }
    // Background, not a subject: a large region, or one that spans the picture.
    let spans = (x1 - x0) * 10 >= w * 9 || (y1 - y0) * 10 >= h * 9;
    if mass <= 0.0 || area * 10 > n * 4 || spans {
        return None;
    }
    let (fw, fh) = (w as f64, h as f64);
    Some(Subject { x: mx / mass / fw, y: my / mass / fh, bounds: Rect::new(x0 as f64 / fw, y0 as f64 / fh, x1 as f64 / fw, y1 as f64 / fh) })
}

/// The lightness step across row `y` (fractional) of column `x`: one cell below minus one cell
/// above; `None` off the grid.
fn step(l: &[f32], w: usize, h: usize, x: usize, y: f64) -> Option<f32> {
    let last = h.checked_sub(1)?;
    let sample = |y: f64| -> Option<f32> {
        if !(0.0..=last as f64).contains(&y) {
            return None;
        }
        let (y0, t) = (y.floor() as usize, (y - y.floor()) as f32);
        let a = l.get(y0 * w + x).copied()?;
        let b = l.get((y0 + 1).min(last) * w + x).copied()?;
        Some(a + (b - a) * t)
    };
    Some(sample(y + 1.0)? - sample(y - 1.0)?)
}

fn find_horizon(g: &Grid) -> Option<Horizon> {
    let (w, h) = (g.w, g.h);
    if w < 8 || h < 8 {
        return None;
    }
    let l = blur(g.planes.first()?, w, h, 1);
    let cx = (w as f64 - 1.0) / 2.0;
    // Slopes in cells per cell, up to about 7°.
    const SLOPES: [f64; 7] = [-0.12, -0.08, -0.04, 0.0, 0.04, 0.08, 0.12];
    // How strongly the line through `y0` (at the centre column) steps on average, and the share
    // of the columns that step the same way.
    let strength = |y0: f64, k: f64| -> (f32, f32) {
        let steps: Vec<f32> = (0..w).filter_map(|x| step(&l, w, h, x, y0 + k * (x as f64 - cx))).collect();
        let sum: f32 = steps.iter().sum();
        let along = steps.iter().filter(|d| d.abs() > 0.02 && d.signum() == sum.signum()).count();
        (sum.abs() / w as f32, along as f32 / w as f32)
    };
    let mut best: Option<(f32, usize, f64)> = None;
    for k in SLOPES {
        for y in 1..h - 1 {
            let (s, along) = strength(y as f64, k);
            if along >= 0.6 && s >= 0.05 && best.is_none_or(|(b, _, _)| s > b) {
                best = Some((s, y, k));
            }
        }
    }
    let (s0, y, k) = best?;
    // Between rows: the top of a parabola through the neighbours' strengths.
    let (sm, sp) = (strength(y as f64 - 1.0, k).0, strength(y as f64 + 1.0, k).0);
    let curve = sm - 2.0 * s0 + sp;
    let offset = if curve < 0.0 { f64::from(((sm - sp) / (2.0 * curve)).clamp(-0.5, 0.5)) } else { 0.0 };
    Some(Horizon { y: (y as f64 + offset + 0.5) / h as f64, slope: k * w as f64 / h as f64 })
}
