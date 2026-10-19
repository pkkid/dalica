//! The Frame Mosaic layout (`creating.md` §3): the frame centre (the first picture unless one is
//! set) in a large central block at about its own ratio, the other pictures in mosaics of the
//! bands around it.
//!
//! The bands follow the page's long side first. Seen with the long side across: one other picture
//! gets one band at the end of the page (the centre takes the rest); two get a band at each end;
//! three add a band above the centre; four or more add a band below it too. The pictures go to the
//! bands in slot order, as many to each as its share of the bands' area (at least one), and each
//! band is partitioned as the Mosaic is ([`super::mosaic::partition`]).
//!
//! The centre block spans 0.55 to 0.75 of each of the page's sides (0.4 to 0.75 once the bands
//! hold eight pictures or more, their blocks then being small). Up to 24 bands' pictures, a few
//! sizes in that range are tried and the one whose blocks come nearest their pictures' ratios is
//! kept (a band may hold a single picture, which then has to take the band's shape); past that
//! the centre spans about 0.6 of the page each way, turned toward its picture's ratio. Either way
//! it is the largest block: a size that would leave a band's block as large is not used, and 0.7
//! each way is the fallback, larger than any band.

use super::mosaic::{self, Rect};
use super::{Ctx, Slot, sane_aspect};

/// The centre block's side as a fraction of the page's, before it turns toward its picture.
const CENTRE: f64 = 0.6;
/// The largest fraction of a page's side the centre block spans.
const CENTRE_MAX: f64 = 0.75;
/// The smallest, with few bands' pictures and with many.
const CENTRE_MIN_FEW: f64 = 0.55;
const CENTRE_MIN_MANY: f64 = 0.4;
/// From how many bands' pictures on the smaller minimum applies.
const MANY: usize = 8;
/// Up to how many bands' pictures a few centre sizes are tried.
const SEARCH: usize = 24;
/// The fallback: a centre block of this fraction each way is larger than any band.
const CENTRE_SAFE: f64 = 0.7;

/// The Frame Mosaic: the centre block and the bands' mosaics.
pub(super) fn layout(ctx: &Ctx) -> Vec<Slot> {
    mosaic::finish(ctx, |gap| expanded(ctx, gap))
}

/// One rectangle per slot, tiling the page inset by half of `gap`.
fn expanded(ctx: &Ctx, gap: f64) -> Vec<Rect> {
    let n = ctx.aspects.len();
    let page = Rect::page(ctx.page_ratio).inset(gap / 2.0);
    if n <= 1 {
        return vec![page; n];
    }
    let centre = ctx.frame_centre.filter(|&c| c < n).unwrap_or(0);
    // A centre the user set is that picture, wherever swaps took it; the default centre is the
    // first slot, which a swap gives to another picture as it would any slot.
    let centre_aspect = sane_aspect(if ctx.frame_centre.is_some() { ctx.aspects.get(centre) } else { ctx.slot_aspects.get(centre) }.copied());
    let others: Vec<usize> = (0..n).filter(|&i| i != centre).collect();
    let largest = |rects: &[Rect]| {
        let area = |i: usize| rects.get(i).map_or(0.0, |r| r.inset(gap / 2.0).area());
        others.iter().all(|&i| area(i) < area(centre))
    };
    let min = if others.len() >= MANY { CENTRE_MIN_MANY } else { CENTRE_MIN_FEW };
    if others.len() <= SEARCH {
        // Few pictures, so a band may hold a single one: of a few centre sizes, the one whose
        // blocks come nearest their pictures' ratios (the centre's counted twice).
        let (pu, pv) = centre_fractions(page, centre_aspect, min);
        let sizes = [min, (min + CENTRE_MAX) / 2.0, CENTRE_MAX];
        let mut best: Option<(f64, Vec<Rect>)> = None;
        for fu in sizes.iter().copied().chain([pu]) {
            for fv in sizes.iter().copied().chain([pv]) {
                let rects = arrange(ctx, page, centre, &others, fu, fv);
                if !largest(&rects) {
                    continue;
                }
                let off = |i: usize, aspect: f64| rects.get(i).map_or(0.0, |r| (r.ratio() / aspect).ln().abs());
                let score =
                    2.0 * off(centre, centre_aspect) + others.iter().map(|&i| off(i, sane_aspect(ctx.slot_aspects.get(i).copied()))).sum::<f64>();
                if best.as_ref().is_none_or(|(b, _)| score < *b) {
                    best = Some((score, rects));
                }
            }
        }
        if let Some((_, rects)) = best {
            return rects;
        }
    } else {
        let (fu, fv) = centre_fractions(page, centre_aspect, min);
        let rects = arrange(ctx, page, centre, &others, fu, fv);
        if largest(&rects) {
            return rects;
        }
    }
    arrange(ctx, page, centre, &others, CENTRE_SAFE, CENTRE_SAFE)
}

/// The centre block's fractions of the page's long and short side: about [`CENTRE`] each way,
/// turned toward `aspect`, each within `min ..= CENTRE_MAX`.
fn centre_fractions(page: Rect, aspect: f64, min: f64) -> (f64, f64) {
    let wide = page.w() >= page.h();
    let (long, short) = if wide { (page.w(), page.h()) } else { (page.h(), page.w()) };
    // The picture's ratio and the page's, long side over short side of the page.
    let along = if wide { aspect } else { 1.0 / aspect };
    let q = (along / sane_aspect(Some(long / short))).sqrt();
    let q = if q.is_finite() && q > 0.0 { q } else { 1.0 };
    ((CENTRE * q).clamp(min, CENTRE_MAX), (CENTRE / q).clamp(min, CENTRE_MAX))
}

/// The centre block of `fu` by `fv` of the page's long and short side, the bands around it and
/// their mosaics: one rectangle per slot.
fn arrange(ctx: &Ctx, page: Rect, centre: usize, others: &[usize], fu: f64, fv: f64) -> Vec<Rect> {
    let n = ctx.aspects.len();
    let mut out = vec![page; n];
    let (centre_rect, bands) = bands(page, others.len(), fu, fv);
    if let Some(r) = out.get_mut(centre) {
        *r = centre_rect;
    }
    let counts = shares(&bands.iter().map(Rect::area).collect::<Vec<_>>(), others.len());
    let mut next = 0;
    for (b, (band, count)) in bands.iter().zip(counts).enumerate() {
        let slots = others.get(next..next + count).unwrap_or(&[]);
        next += count;
        let aspects: Vec<f64> = slots.iter().map(|&i| sane_aspect(ctx.slot_aspects.get(i).copied())).collect();
        for (&i, r) in slots.iter().zip(mosaic::partition(*band, &aspects, ctx.seed, b as u64 + 1)) {
            if let Some(slot) = out.get_mut(i) {
                *slot = r;
            }
        }
    }
    out
}

/// The centre block and the bands for `m` (at least one) other pictures, in the order the
/// pictures fill them, tiling `page`. Worked out with the long side across (`u`) and the short
/// side down (`v`), then turned back for a tall page.
fn bands(page: Rect, m: usize, fu: f64, fv: f64) -> (Rect, Vec<Rect>) {
    let wide = page.w() >= page.h();
    let (u0, u1, v0, v1) = if wide { (page.x0, page.x1, page.y0, page.y1) } else { (page.y0, page.y1, page.x0, page.x1) };
    let (lu, lv) = (u1 - u0, v1 - v0);
    let rect = |a: f64, b: f64, c: f64, d: f64| if wide { Rect { x0: a, x1: b, y0: c, y1: d } } else { Rect { x0: c, x1: d, y0: a, y1: b } };
    if m <= 1 {
        let cut = u0 + fu * lu;
        return (rect(u0, cut, v0, v1), vec![rect(cut, u1, v0, v1)]);
    }
    let side = (1.0 - fu) / 2.0 * lu;
    let (left, right) = (u0 + side, u1 - side);
    let (start, end) = (rect(u0, left, v0, v1), rect(right, u1, v0, v1));
    if m == 2 {
        return (rect(left, right, v0, v1), vec![start, end]);
    }
    if m == 3 {
        let cut = v0 + (1.0 - fv) * lv;
        return (rect(left, right, cut, v1), vec![rect(left, right, v0, cut), start, end]);
    }
    let edge = (1.0 - fv) / 2.0 * lv;
    let (top, bottom) = (v0 + edge, v1 - edge);
    (rect(left, right, top, bottom), vec![rect(left, right, v0, top), start, end, rect(left, right, bottom, v1)])
}

/// `m` pictures shared among bands of `areas` as their areas go, at least one each (largest
/// remainder; ties to the earlier band).
fn shares(areas: &[f64], m: usize) -> Vec<usize> {
    let total: f64 = areas.iter().sum();
    let quota: Vec<f64> = areas.iter().map(|&a| if total > 0.0 { a / total * m as f64 } else { m as f64 / areas.len().max(1) as f64 }).collect();
    let mut counts: Vec<usize> = quota.iter().map(|&q| (q.floor() as usize).max(1)).collect();
    let pick = |counts: &[usize], more: bool| -> Option<usize> {
        let room = |i: usize| quota.get(i).copied().unwrap_or(0.0) - counts.get(i).copied().unwrap_or(0) as f64;
        let candidates = (0..counts.len()).filter(|&i| more || counts.get(i).is_some_and(|&c| c > 1));
        if more {
            candidates.max_by(|&a, &b| room(a).total_cmp(&room(b)).then(b.cmp(&a)))
        } else {
            candidates.min_by(|&a, &b| room(a).total_cmp(&room(b)).then(b.cmp(&a)))
        }
    };
    while counts.iter().sum::<usize>() > m {
        let Some(i) = pick(&counts, false) else { break };
        if let Some(c) = counts.get_mut(i) {
            *c -= 1;
        }
    }
    while counts.iter().sum::<usize>() < m {
        let Some(i) = pick(&counts, true) else { break };
        if let Some(c) = counts.get_mut(i) {
            *c += 1;
        }
    }
    counts
}
