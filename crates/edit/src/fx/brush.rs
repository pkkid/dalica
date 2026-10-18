//! Brush dabs along painted strokes, for Soften's mask (`editing.md` §8.4): a stroke is a path of
//! stored-frame points with a radius relative to the stored photo's shorter side and a feather;
//! round dabs are laid along it a quarter of the radius apart, so it covers its path without gaps
//! however fast it was painted.
//!
//! - **One stroke** covers a pixel by the most any of its dabs gives there ([`dab_alpha`]: 1 in
//!   the hard core, falling smoothly to 0 at the radius), so its edge has the dab's profile
//!   whatever the spacing.
//! - **Strokes** apply in order on a coverage that starts at `start` ([`paint`]): a painting
//!   stroke adds `s · (1 − v)`, an erasing one takes `v · s` away.
//! - **Limits:** at most [`MAX_DABS`] dabs a stroke (a longer one is laid further apart), only
//!   dabs that reach the picture are kept, and points that are not finite are skipped.
//!
//! Ported from LightCraft `crates/pipeline/src/masks.rs` (`brush_dabs`, `push_dabs`,
//! `dab_spacing`, `dab_alpha`) at revision 0da998f2 (MIT OR Apache-2.0; `NOTICE`). Dalica's
//! changes: points in the stored frame through [`Space`] with the radius relative to the shorter
//! side (not the longer); no pen pressure, flow, density or auto mask; a stroke's coverage is the
//! most of its dabs rather than flow accumulating; a budget of dabs per stroke instead of per
//! segment; dabs off the picture dropped.

use super::space::Space;
use crate::Point;

/// The most dabs one stroke lays.
pub const MAX_DABS: usize = 1 << 20;

/// Output pixels a point must move from the last dab to lay another (a pointer held still).
const STATIONARY_PX: f64 = 1e-6;

/// Distance between consecutive dabs of a brush of radius `r` (pixels): a quarter of the radius,
/// at least half a pixel.
pub fn dab_spacing(r: f64) -> f64 {
    if r.is_finite() { (r / 4.0).max(0.5) } else { 0.5 }
}

/// How much one dab paints at distance `dd` from its centre: 1 inside the hard core `hard`,
/// smoothly falling to 0 at the radius `r`.
pub fn dab_alpha(dd: f64, r: f64, hard: f64) -> f32 {
    if dd.is_nan() || dd > r {
        0.0
    } else if dd <= hard {
        1.0
    } else {
        (1.0 - super::smoothstep(hard, r, dd)) as f32
    }
}

/// The dabs of one stroke in the output of a render, in output pixel coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Dabs {
    centres: Vec<(f64, f64)>,
    r: f64,
    hard: f64,
    erase: bool,
    /// The box the dabs cover, `(x0, y0, x1, y1)` in output pixels.
    bounds: (f64, f64, f64, f64),
}

impl Dabs {
    /// The stroke through the stored-frame `points` with `radius` relative to the stored photo's
    /// shorter side (at least half an output pixel) and `feather` the fraction of the radius that
    /// fades (0..1); `erase` takes coverage away instead of adding it.
    pub fn new(space: &Space, points: &[Point], radius: f64, feather: f64, erase: bool) -> Dabs {
        let r = space.length(radius).max(0.5);
        let feather = if feather.is_nan() { 0.0 } else { feather.clamp(0.0, 1.0) };
        let hard = r * (1.0 - feather);
        // The path's length in pixels, to spread at most `MAX_DABS` dabs along it.
        let path: Vec<(f64, f64)> = points.iter().map(|p| space.to_output(*p)).filter(|q| q.0.is_finite() && q.1.is_finite()).collect();
        let length: f64 = path
            .windows(2)
            .map(|w| match w {
                [a, b] => (b.0 - a.0).hypot(b.1 - a.1),
                _ => 0.0,
            })
            .sum();
        let spacing = dab_spacing(r).max(length / MAX_DABS as f64);
        let (w, h) = space.size();
        let on_picture = |q: (f64, f64)| q.0 > -r && q.1 > -r && q.0 < f64::from(w) + r && q.1 < f64::from(h) + r;
        let mut centres = Vec::new();
        let mut prev: Option<(f64, f64)> = None;
        for q in path {
            if let Some(p) = prev {
                let dist = (q.0 - p.0).hypot(q.1 - p.1);
                if dist.is_nan() || dist <= STATIONARY_PX {
                    continue;
                }
                let n = (dist / spacing).ceil();
                let n = if n.is_finite() { n.clamp(1.0, MAX_DABS as f64) as usize } else { 1 };
                for k in 1..n {
                    let t = k as f64 / n as f64;
                    let d = (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
                    if centres.len() < MAX_DABS && on_picture(d) {
                        centres.push(d);
                    }
                }
            }
            if centres.len() < MAX_DABS && on_picture(q) {
                centres.push(q);
            }
            prev = Some(q);
        }
        let bounds = centres.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, c| {
            (b.0.min(c.0 - r), b.1.min(c.1 - r), b.2.max(c.0 + r), b.3.max(c.1 + r))
        });
        Dabs { centres, r, hard, erase, bounds }
    }

    /// The dabs' centres in output pixels.
    pub fn centres(&self) -> &[(f64, f64)] {
        &self.centres
    }

    /// The radius in output pixels.
    pub fn radius(&self) -> f64 {
        self.r
    }

    pub fn erases(&self) -> bool {
        self.erase
    }

    /// How much the stroke covers the output pixel `(x, y)`: the most any dab gives there.
    pub fn alpha(&self, x: u32, y: u32) -> f32 {
        let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        if px < self.bounds.0 || py < self.bounds.1 || px > self.bounds.2 || py > self.bounds.3 {
            return 0.0;
        }
        self.centres.iter().map(|c| dab_alpha((px - c.0).hypot(py - c.1), self.r, self.hard)).fold(0.0, f32::max)
    }
}

/// The coverage of `strokes`, applied in order on `start`, over the `width × rows` window at
/// `(x0, y0)` of the output (a band), row-major; `None` when its buffer cannot be had.
pub fn paint(strokes: &[Dabs], start: f32, x0: u32, y0: u32, width: u32, rows: u32) -> Option<Vec<f32>> {
    let start = if start.is_nan() { 0.0 } else { start.clamp(0.0, 1.0) };
    let n = (width as usize).checked_mul(rows as usize)?;
    let mut out = alloc(n)?;
    out.fill(start);
    let mut cover = alloc(n)?;
    let w = width as usize;
    for stroke in strokes {
        let Some((ax, ay, bx, by)) = window_box(stroke.bounds, x0, y0, width, rows) else { continue };
        for ly in ay..by {
            if let Some(row) = cover.get_mut(ly * w + ax..ly * w + bx) {
                row.fill(0.0);
            }
        }
        // Each dab stamps the most it gives over its own box.
        let r = stroke.r;
        for c in &stroke.centres {
            let Some((dx0, dy0, dx1, dy1)) = window_box((c.0 - r, c.1 - r, c.0 + r, c.1 + r), x0, y0, width, rows) else { continue };
            for ly in dy0..dy1 {
                let py = f64::from(y0) + ly as f64 + 0.5;
                for lx in dx0..dx1 {
                    let px = f64::from(x0) + lx as f64 + 0.5;
                    let a = dab_alpha((px - c.0).hypot(py - c.1), r, stroke.hard);
                    if let Some(v) = cover.get_mut(ly * w + lx)
                        && a > *v
                    {
                        *v = a;
                    }
                }
            }
        }
        for ly in ay..by {
            for lx in ax..bx {
                let i = ly * w + lx;
                let (Some(&s), Some(v)) = (cover.get(i), out.get_mut(i)) else { continue };
                if s > 0.0 {
                    *v = if stroke.erase { *v * (1.0 - s) } else { *v + s * (1.0 - *v) };
                }
            }
        }
    }
    Some(out)
}

/// The rows and columns of the `width × rows` window at `(x0, y0)` that the box `b` (`(x0, y0,
/// x1, y1)` in output pixels) reaches, as half-open ranges of window coordinates.
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

fn alloc(n: usize) -> Option<Vec<f32>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, 0.0);
    Some(out)
}
