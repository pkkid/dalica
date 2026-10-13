//! The maths the Crop and Straighten tools share (`editing.md` §6.1–§6.2): a ratio's largest
//! centred rectangle, Rotate (portrait ↔ landscape about the centre, kept inside), dragging an
//! edge, a corner or the inside with the ratio locked unless Manual, a minimum size, and keeping a
//! crop inside a turned picture. The app, the CLI, the control channel and MCP all call these, so
//! they agree.
//!
//! Ported from LightCraft `crates/geom/src/crop.rs` (`max_inscribed_scale`, `crop_fit_angle`,
//! `constrained`, `with_angle`, `is_within_image`) and `crates/geom/src/crop_drag.rs`
//! (`drag_crop`, `CropHandle`) at revision 76b53778 (MIT OR Apache-2.0; `NOTICE`). Dalica's
//! changes: written over [`crate::Rect`] and [`crate::Point`] (LightCraft's `Rect` helpers are the
//! private [`Px`] below); the angle is limited by the caller, not here; `rotate` (the Crop panel's
//! Rotate button) and `ratio_rect` are new; no serde.
//!
//! **Frames.** A [`CropBox`] is a rectangle normalised to a `w × h` picture (`x / w`, `y / h`) in a
//! frame turned by `angle` degrees (clockwise on screen) about the picture's centre, and must stay
//! inside the turned picture. Dalica's `straighten` scales the turned picture up until it fills its
//! frame (`ops::straighten`), so the stored crop, measured on the straightened picture, uses
//! `angle` 0: its bounds are the unit square. A non-zero angle is for a tool that shows the turned
//! picture without that scale. Ratios are width / height **in pixels**.

use crate::{Point, Rect};

/// Smallest crop side, as a fraction of the picture's shorter side.
pub const MIN_CROP_FRACTION: f64 = 0.02;

/// Most elongated locked crop shape (width:height or its inverse) a drag supports.
pub const MAX_RATIO: f64 = 20.0;

/// A crop rectangle normalised to a `w × h` picture, in the frame turned by `angle` degrees
/// (clockwise on screen) about the picture's centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropBox {
    pub rect: Rect,
    pub angle: f64,
}

impl Default for CropBox {
    fn default() -> CropBox {
        CropBox { rect: Rect::new(0.0, 0.0, 1.0, 1.0), angle: 0.0 }
    }
}

/// A rectangle in pixels with the helpers the maths needs (LightCraft's `geom::Rect`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Px {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Px {
    pub(crate) const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Px {
        Px { x0, y0, x1, y1 }
    }

    fn from_points(a: Point, b: Point) -> Px {
        Px::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
    }

    fn from_center(c: Point, w: f64, h: f64) -> Px {
        Px::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0)
    }

    fn of(r: Rect, w: f64, h: f64) -> Px {
        Px::new(r.x0 * w, r.y0 * h, r.x1 * w, r.y1 * h)
    }

    fn normalised(self, w: f64, h: f64) -> Rect {
        Rect::new(self.x0 / w, self.y0 / h, self.x1 / w, self.y1 / h)
    }

    pub(crate) fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub(crate) fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    fn center(&self) -> Point {
        Point::new((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    fn translate(&self, dx: f64, dy: f64) -> Px {
        Px::new(self.x0 + dx, self.y0 + dy, self.x1 + dx, self.y1 + dy)
    }

    fn corners(&self) -> [Point; 4] {
        [Point::new(self.x0, self.y0), Point::new(self.x1, self.y0), Point::new(self.x1, self.y1), Point::new(self.x0, self.y1)]
    }

    /// The largest rectangle of ratio `a` inside this one, centred.
    fn fit_aspect(&self, a: f64) -> Px {
        let (w, h) = (self.width(), self.height());
        if !(a.is_finite() && a > 0.0) || w <= 0.0 || h <= 0.0 {
            return *self;
        }
        let (nw, nh) = if w / h > a { (h * a, h) } else { (w, w / a) };
        Px::from_center(self.center(), nw, nh)
    }
}

/// Whether a normalised rectangle has no area (or is not ordered, or not finite).
fn is_empty(r: Rect) -> bool {
    !(r.x1 > r.x0 && r.y1 > r.y0)
}

/// `p` turned by `degrees` (clockwise on screen, `y` down) about `c`.
fn turn(p: Point, degrees: f64, c: Point) -> Point {
    let (s, co) = degrees.to_radians().sin_cos();
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    Point::new(c.x + co * dx - s * dy, c.y + s * dx + co * dy)
}

impl CropBox {
    pub const fn new(rect: Rect, angle: f64) -> CropBox {
        CropBox { rect, angle }
    }

    /// Whether every corner of the crop lies within the turned `w × h` picture.
    pub fn is_within_image(&self, w: f64, h: f64) -> bool {
        let r = Px::of(self.rect, w, h);
        let c = Point::new(w / 2.0, h / 2.0);
        r.corners().iter().all(|p| {
            let q = turn(*p, -self.angle, c);
            q.x >= -1e-6 * w && q.y >= -1e-6 * h && q.x <= w * (1.0 + 1e-6) && q.y <= h * (1.0 + 1e-6)
        })
    }

    /// The crop moved towards the picture's centre until it fits inside the turned picture, else
    /// shrunk about the centre keeping its ratio.
    pub fn constrained(&self, w: f64, h: f64) -> CropBox {
        if self.is_within_image(w, h) {
            return *self;
        }
        let r = Px::of(self.rect, w, h);
        let c = Point::new(w / 2.0, h / 2.0);
        let rc = r.center();
        for i in 1..=32 {
            let t = f64::from(i) / 32.0;
            let nc = Point::new(rc.x + (c.x - rc.x) * t, rc.y + (c.y - rc.y) * t);
            let cand = CropBox { rect: Px::from_center(nc, r.width(), r.height()).normalised(w, h), angle: self.angle };
            if cand.is_within_image(w, h) {
                return cand;
            }
        }
        let s = max_inscribed_scale(w, h, self.angle, r.width(), r.height());
        CropBox { rect: Px::from_center(c, r.width() * s, r.height() * s).normalised(w, h), angle: self.angle }
    }

    /// This crop turned to `angle` on a `w × h` picture: kept when it still fits inside the turned
    /// picture, else the largest crop of its ratio that fits, centred.
    pub fn with_angle(&self, angle: f64, w: f64, h: f64) -> CropBox {
        let cand = CropBox { rect: self.rect, angle };
        if cand.is_within_image(w, h) {
            return cand;
        }
        let r = Px::of(self.rect, w, h);
        // A degenerate crop has no ratio to keep: the picture's own.
        let aspect = Some(r.width() / r.height()).filter(|a| a.is_finite() && *a > 0.0);
        crop_fit_angle(w, h, angle, aspect)
    }
}

/// The largest scale `s` such that a centred `cw·s × ch·s` rectangle fits inside a `w × h`
/// picture turned by `angle` degrees about its centre.
pub fn max_inscribed_scale(w: f64, h: f64, angle: f64, cw: f64, ch: f64) -> f64 {
    let (s, c) = angle.to_radians().abs().sin_cos();
    // The crop's corner (cw/2, ch/2)·scale, turned back into the picture, must satisfy
    // |x|·c + |y|·s ≤ w/2 and |x|·s + |y|·c ≤ h/2.
    let (hx, hy) = (cw / 2.0, ch / 2.0);
    let s1 = (w / 2.0) / (hx * c + hy * s);
    let s2 = (h / 2.0) / (hx * s + hy * c);
    s1.min(s2)
}

/// How much a `w × h` picture turned by `angle` degrees about its centre is enlarged so that it
/// covers its own `w × h` frame: no empty corner shows (`straighten`, `editing.md` §6.2). 1 at 0°;
/// 1 for a degenerate size.
pub fn fill_scale(w: f64, h: f64, angle: f64) -> f64 {
    let s = max_inscribed_scale(w, h, angle, w, h);
    if s.is_finite() && s > 0.0 { (1.0 / s).max(1.0) } else { 1.0 }
}

/// The largest crop of the picture's own ratio (or `aspect`, width / height in pixels) that fits
/// at `angle`, centred.
pub fn crop_fit_angle(w: f64, h: f64, angle: f64, aspect: Option<f64>) -> CropBox {
    let a = aspect.unwrap_or(w / h);
    let base = Px::new(0.0, 0.0, w, h).fit_aspect(a);
    let s = max_inscribed_scale(w, h, angle, base.width(), base.height()).min(1.0);
    let r = Px::from_center(Point::new(w / 2.0, h / 2.0), base.width() * s, base.height() * s);
    CropBox { rect: r.normalised(w, h), angle }
}

/// The largest centred rectangle of ratio `aspect` (width / height in pixels) on a `w × h`
/// picture, normalised: what choosing a size in the Crop panel's dropdown draws. The whole picture
/// when the ratio or the size is not usable.
pub fn ratio_rect(w: f64, h: f64, aspect: f64) -> Rect {
    let full = Rect::new(0.0, 0.0, 1.0, 1.0);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 && aspect.is_finite() && aspect > 0.0) {
        return full;
    }
    let r = crop_fit_angle(w, h, 0.0, Some(aspect)).rect;
    clamp_unit(r)
}

/// The Crop panel's Rotate (`editing.md` §6.1): the crop turned between portrait and landscape
/// about its centre (its pixel width and height swapped), then kept inside the turned picture
/// ([`CropBox::constrained`]: moved towards the centre, else shrunk keeping the new ratio). The
/// crop unchanged for hostile input.
pub fn rotate(b: CropBox, w: f64, h: f64) -> CropBox {
    let r0 = b.rect;
    if ![w, h, b.angle, r0.x0, r0.y0, r0.x1, r0.y1].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 || is_empty(r0) {
        return b;
    }
    let r = Px::of(r0, w, h);
    let swapped = CropBox { rect: Px::from_center(r.center(), r.height(), r.width()).normalised(w, h), angle: b.angle };
    let out = swapped.constrained(w, h);
    if b.angle == 0.0 { CropBox { rect: clamp_unit(out.rect), angle: 0.0 } } else { out }
}

fn clamp_unit(r: Rect) -> Rect {
    Rect::new(r.x0.clamp(0.0, 1.0), r.y0.clamp(0.0, 1.0), r.x1.clamp(0.0, 1.0), r.y1.clamp(0.0, 1.0))
}

/// A grab point on the crop frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
    Top,
    Right,
    Bottom,
    Left,
    /// Drag the whole frame.
    Move,
}

impl Handle {
    /// The overlay's numbering: 0..3 corners (clockwise from top-left), 4..7 edges (top, right,
    /// bottom, left), 8 = move.
    pub fn from_index(i: u8) -> Option<Handle> {
        Some(match i {
            0 => Handle::TopLeft,
            1 => Handle::TopRight,
            2 => Handle::BottomRight,
            3 => Handle::BottomLeft,
            4 => Handle::Top,
            5 => Handle::Right,
            6 => Handle::Bottom,
            7 => Handle::Left,
            8 => Handle::Move,
            _ => return None,
        })
    }

    /// A command parameter's name (`"topLeft"`, `"top"`, `"move"`, …).
    pub fn from_name(n: &str) -> Option<Handle> {
        Some(match n {
            "topLeft" => Handle::TopLeft,
            "topRight" => Handle::TopRight,
            "bottomRight" => Handle::BottomRight,
            "bottomLeft" => Handle::BottomLeft,
            "top" => Handle::Top,
            "right" => Handle::Right,
            "bottom" => Handle::Bottom,
            "left" => Handle::Left,
            "move" => Handle::Move,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Handle::TopLeft => "topLeft",
            Handle::TopRight => "topRight",
            Handle::BottomRight => "bottomRight",
            Handle::BottomLeft => "bottomLeft",
            Handle::Top => "top",
            Handle::Right => "right",
            Handle::Bottom => "bottom",
            Handle::Left => "left",
            Handle::Move => "move",
        }
    }
}

/// Drags `handle` of `start` from `grab` to `pointer` (both normalised, in the crop's frame) on a
/// `w × h` picture. `aspect` is the locked width / height ratio in pixels (`None`: Manual).
///
/// For any input, hostile numbers included:
/// - the result lies inside the turned picture;
/// - the edges opposite the handle stay where they were (a move keeps the size);
/// - with `aspect`, the frame keeps that shape on every handle (when `start` had it);
/// - no side gets smaller than [`MIN_CROP_FRACTION`] of the picture's shorter side;
/// - the drag stops at the picture's edge instead of pushing the frame around, and slides along it.
///
/// Hostile input (non-finite numbers, an empty size or crop, a ratio that is not positive) gives
/// `start` back.
pub fn drag(start: CropBox, handle: Handle, grab: Point, pointer: Point, w: f64, h: f64, aspect: Option<f64>) -> CropBox {
    let r0 = start.rect;
    if ![w, h, grab.x, grab.y, pointer.x, pointer.y, start.angle, r0.x0, r0.y0, r0.x1, r0.y1].iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || is_empty(r0)
        || aspect.is_some_and(|a| !(a.is_finite() && a > 0.0))
    {
        return start;
    }
    // Start from a valid frame, then work in pixels of the crop's frame.
    let mut base = start.constrained(w, h);
    if base.angle == 0.0 {
        // Absorb the 1e-6 tolerance of `is_within_image` so the exact test below accepts the start.
        base.rect = clamp_unit(base.rect);
    }
    let sr = Px::of(base.rect, w, h);
    // Moving further than twice the picture never matters; clamping keeps the searches
    // well-conditioned.
    let (dx, dy) = (((pointer.x - grab.x) * w).clamp(-2.0 * w, 2.0 * w), ((pointer.y - grab.y) * h).clamp(-2.0 * h, 2.0 * h));
    // Ratios beyond 20:1 make even the minimum frame larger than the picture; treat them as 20:1.
    let aspect = aspect.map(|a| a.clamp(1.0 / MAX_RATIO, MAX_RATIO));
    // Unturned, the frame must be exactly inside the picture (`is_within_image` tolerates 1e-6,
    // which would leak out as a sliver past the edge and, after any clamp, a shifted ratio).
    let feasible = |r: Px| {
        if base.angle == 0.0 {
            r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= w && r.y1 <= h
        } else {
            CropBox { rect: r.normalised(w, h), angle: base.angle }.is_within_image(w, h)
        }
    };
    let min_side = MIN_CROP_FRACTION * w.min(h);

    let out = match (handle, aspect) {
        (Handle::Move, _) => {
            let slide = |cur: Px, ax: f64, ay: f64| {
                let t = bisect(0.0, 1.0, |t| feasible(cur.translate(ax * t, ay * t)));
                cur.translate(ax * t, ay * t)
            };
            let cur = slide(sr, dx, 0.0);
            slide(cur, 0.0, dy)
        }
        (hd, None) => {
            let (mx0, mx1, my0, my1) = sides(hd);
            let grow = |cur: Px, tx: f64, ty: f64| {
                // `tx`, `ty`: how far the moving edges would go; interpolate towards it.
                let target = Px::new(
                    if mx0 { (cur.x0 + tx).min(cur.x1 - min_side) } else { cur.x0 },
                    if my0 { (cur.y0 + ty).min(cur.y1 - min_side) } else { cur.y0 },
                    if mx1 { (cur.x1 + tx).max(cur.x0 + min_side) } else { cur.x1 },
                    if my1 { (cur.y1 + ty).max(cur.y0 + min_side) } else { cur.y1 },
                );
                let at = |t: f64| lerp(cur, target, t);
                at(bisect(0.0, 1.0, |t| feasible(at(t))))
            };
            let cur = grow(sr, dx, 0.0);
            grow(cur, 0.0, dy)
        }
        (hd, Some(a)) => locked(hd, a, sr, dx, dy, w, h, min_side, &feasible),
    };
    let rect = out.normalised(w, h);
    if [rect.x0, rect.y0, rect.x1, rect.y1].iter().all(|v| v.is_finite()) { CropBox { rect, angle: base.angle } } else { start }
}

/// A drag of handle `hd` (not Move) with the ratio `a` locked.
#[allow(clippy::too_many_arguments)]
fn locked(hd: Handle, a: f64, sr: Px, dx: f64, dy: f64, w: f64, h: f64, min_side: f64, feasible: &dyn Fn(Px) -> bool) -> Px {
    let (mx0, _, my0, _) = sides(hd);
    let (min_w, min_h) = (min_side.max(min_side * a), min_side.max(min_side / a));
    let c = sr.center();
    // The anchor: the opposite corner or edge. `build(n, o)` is the frame of width `n` that keeps
    // it, shifted by `o` along the anchored edge (edge handles only).
    let (target_w, build): (f64, Box<dyn Fn(f64, f64) -> Px>) = match hd {
        Handle::Top | Handle::Bottom => {
            let (ay, sy, edge) = if my0 { (sr.y1, -1.0, sr.y0) } else { (sr.y0, 1.0, sr.y1) };
            let nh = (sy * (edge + dy - ay)).max(min_h);
            (nh * a, Box::new(move |nw, o| vband(c.x + o, ay, sy, nw, a)))
        }
        Handle::Left | Handle::Right => {
            let (ax, sx, edge) = if mx0 { (sr.x1, -1.0, sr.x0) } else { (sr.x0, 1.0, sr.x1) };
            let nw = (sx * (edge + dx - ax)).max(min_w);
            (nw, Box::new(move |nw, o| hband(ax, sx, c.y + o, nw, a)))
        }
        _ => {
            let (ax, sx, ex) = if mx0 { (sr.x1, -1.0, sr.x0) } else { (sr.x0, 1.0, sr.x1) };
            let (ay, sy, ey) = if my0 { (sr.y1, -1.0, sr.y0) } else { (sr.y0, 1.0, sr.y1) };
            let (wx, wy) = (sx * (ex + dx - ax), sy * (ey + dy - ay));
            let nw = ((wx + wy * a) / 2.0).max(min_w);
            (nw, Box::new(move |nw, _| corner_rect(ax, sx, ay, sy, nw, a)))
        }
    };
    let target_w = target_w.min(2.0 * w.max(h * a));
    let slides = matches!(hd, Handle::Top | Handle::Bottom | Handle::Left | Handle::Right);
    let extent = if matches!(hd, Handle::Top | Handle::Bottom) { w } else { h };
    // The frame of width `n`, centred on the anchored edge if it fits, else slid the least distance.
    let place = |n: f64| -> Option<Px> {
        let at = |o: f64| build(n, o);
        if feasible(at(0.0)) {
            return Some(at(0.0));
        }
        if !slides {
            return None;
        }
        const STEPS: u32 = 128;
        let step = extent / f64::from(STEPS);
        for k in 1..=STEPS {
            for sign in [1.0, -1.0] {
                let hi = sign * f64::from(k) * step;
                if feasible(at(hi)) {
                    // The closest feasible offset: bisect between the last infeasible one and this.
                    let (mut bad, mut good) = (sign * f64::from(k - 1) * step, hi);
                    for _ in 0..40 {
                        let mid = (bad + good) / 2.0;
                        if feasible(at(mid)) {
                            good = mid;
                        } else {
                            bad = mid;
                        }
                    }
                    return Some(at(good));
                }
            }
        }
        None
    };
    if place(min_w).is_some() {
        let n = bisect(min_w, target_w.max(min_w), |n| place(n).is_some());
        place(n).unwrap_or(sr)
    } else {
        sr
    }
}

/// Which sides a handle moves: (left, right, top, bottom).
fn sides(h: Handle) -> (bool, bool, bool, bool) {
    use Handle::*;
    (
        matches!(h, TopLeft | BottomLeft | Left),
        matches!(h, TopRight | BottomRight | Right),
        matches!(h, TopLeft | TopRight | Top),
        matches!(h, BottomLeft | BottomRight | Bottom),
    )
}

fn lerp(a: Px, b: Px, t: f64) -> Px {
    let l = |p: f64, q: f64| p + (q - p) * t;
    Px::new(l(a.x0, b.x0), l(a.y0, b.y0), l(a.x1, b.x1), l(a.y1, b.y1))
}

/// The frame of width `nw` and ratio `a` reaching from the corner (`ax`, `ay`) in directions `sx`,
/// `sy`.
fn corner_rect(ax: f64, sx: f64, ay: f64, sy: f64, nw: f64, a: f64) -> Px {
    let nh = nw / a;
    Px::from_points(Point::new(ax, ay), Point::new(ax + sx * nw, ay + sy * nh))
}

/// The frame whose top or bottom edge (at `ay`, growing in direction `sy`) is anchored, centred on
/// `cx`.
fn vband(cx: f64, ay: f64, sy: f64, nw: f64, a: f64) -> Px {
    let nh = nw / a;
    Px::from_points(Point::new(cx - nw / 2.0, ay), Point::new(cx + nw / 2.0, ay + sy * nh))
}

/// The frame whose left or right edge (at `ax`, growing in direction `sx`) is anchored, centred on
/// `cy`.
fn hband(ax: f64, sx: f64, cy: f64, nw: f64, a: f64) -> Px {
    let nh = nw / a;
    Px::from_points(Point::new(ax, cy - nh / 2.0), Point::new(ax + sx * nw, cy + nh / 2.0))
}

/// The largest `t` in `[lo, hi]` for which `ok` holds, given that it holds at `lo` and the good `t`
/// form an interval (the feasible region is convex).
fn bisect(lo: f64, hi: f64, ok: impl Fn(f64) -> bool) -> f64 {
    if ok(hi) {
        return hi;
    }
    let (mut good, mut bad) = (lo, hi);
    for _ in 0..48 {
        let mid = (good + bad) / 2.0;
        if ok(mid) {
            good = mid;
        } else {
            bad = mid;
        }
    }
    good
}
