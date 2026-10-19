//! The Grid layout (`creating.md` §3): rows and columns of equal cells chosen for the count, the
//! page's ratio and the pictures' shapes, the grid spacing between the cells and round the edge,
//! each picture filling its cell (cropped to it, centred). A last row that is not full is centred.
//!
//! [`Rect`] and [`grid_rects`] are ported from LightCraft `crates/engine/src/print/layout.rs`
//! (`Rect`, `Layout::grid_rects_for`) and [`fit`] from `crates/engine/src/print/compose.rs`
//! (`place`) @ 20ccedf5, MIT OR Apache-2.0 (`NOTICE`). Changed from the original: lengths are in
//! page units (the page's height is 1) instead of inches; the grid takes its columns, rows and
//! spacing as arguments instead of a print layout's grid spec, and its spacing also runs round
//! the edge; a partial last row can be centred; `fit` answers a size in page units (no pixel
//! rounding, no quarter turn). The Contact Sheet (`sheet.rs`) uses them too.

use super::{Ctx, Slot, fill, sane_aspect};

/// The tolerance of the score's comparison (a tie keeps fewer columns).
const EPS: f64 = 1e-9;
/// How much of a cell the spacing may take at most, so a crowded page keeps its pictures: the
/// gaps of a row or a column together are at most this fraction of the page's side.
pub const MAX_GAP_SHARE: f64 = 0.5;

/// An axis-aligned rectangle in page units: the page's height is 1, its width the page's ratio.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn centre(&self) -> [f64; 2] {
        [self.x + self.w / 2.0, self.y + self.h / 2.0]
    }
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()
    }
    /// The slot of a picture filling this rectangle on a page of `ratio`: centre and size
    /// normalised to the page.
    pub fn slot(&self, ratio: f64, angle: f64, shown: dalica_core::Rect01) -> Slot {
        let [cx, cy] = self.centre();
        Slot { centre: [cx / ratio, cy], size: [self.w / ratio, self.h], angle, shown }
    }
}

/// The `n` cells of a grid of `cols` × `rows` equal cells over `area` in reading order, `gap`
/// between neighbours (not at the area's edge); a last row with fewer cells is centred when
/// `centre_last`. `None` when the spacing leaves no room for the cells or a number is not usable.
pub fn grid_rects(area: Rect, cols: usize, rows: usize, gap: f64, n: usize, centre_last: bool) -> Option<Vec<Rect>> {
    if cols == 0 || rows == 0 || !area.is_finite() || !(gap.is_finite() && gap >= 0.0) {
        return None;
    }
    let n = n.min(cols.checked_mul(rows)?);
    let cw = (area.w - gap * (cols - 1) as f64) / cols as f64;
    let ch = (area.h - gap * (rows - 1) as f64) / rows as f64;
    if !(cw > 0.0 && ch > 0.0) {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (r, c) = (i / cols, i % cols);
        let in_row = (n - r * cols).min(cols);
        let shift = if centre_last && in_row < cols { (cols - in_row) as f64 * (cw + gap) / 2.0 } else { 0.0 };
        out.push(Rect::new(area.x + shift + c as f64 * (cw + gap), area.y + r as f64 * (ch + gap), cw, ch));
    }
    Some(out)
}

/// The size of a picture of `iw` × `ih` scaled into an area of `aw` × `ah`: the largest that fits
/// whole (`cover` false) or the smallest that covers the area (`cover` true). `None` for an empty
/// area or a picture without size.
pub fn fit(iw: f64, ih: f64, aw: f64, ah: f64, cover: bool) -> Option<(f64, f64)> {
    let usable = |v: f64| v.is_finite() && v > 0.0;
    if !(usable(iw) && usable(ih) && usable(aw) && usable(ah)) {
        return None;
    }
    let scale = if cover { (aw / iw).max(ah / ih) } else { (aw / iw).min(ah / ih) };
    let (w, h) = (iw * scale, ih * scale);
    let (w, h) = if cover { (w.max(aw), h.max(ah)) } else { (w.min(aw), h.min(ah)) };
    (w.is_finite() && h.is_finite()).then_some((w, h))
}

/// The pictures' typical shape: the median aspect ratio (1 for none). Sorting, not logarithms,
/// so every system picks the same.
pub fn typical_aspect(aspects: &[f64]) -> f64 {
    let mut a: Vec<f64> = aspects.iter().map(|&v| sane_aspect(Some(v))).collect();
    a.sort_by(f64::total_cmp);
    match a.len() {
        0 => 1.0,
        n if n % 2 == 1 => a.get(n / 2).copied().unwrap_or(1.0),
        n => (a.get(n / 2 - 1).copied().unwrap_or(1.0) + a.get(n / 2).copied().unwrap_or(1.0)) / 2.0,
    }
}

/// Columns and rows for `n` cells on an area of `ratio` (width over height) whose cells should be
/// near `shape`: the arrangement that shows most of the pictures filling their cells (a cell of
/// another shape crops them) times the share of cells used, fewer columns on a tie. At least one
/// of each.
pub fn columns_rows(n: usize, ratio: f64, shape: f64) -> (usize, usize) {
    let n = n.max(1);
    let (ratio, shape) = (sane_aspect(Some(ratio)), sane_aspect(Some(shape)));
    let mut best = (1, n, f64::NEG_INFINITY);
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let cell = (ratio / cols as f64) / (1.0 / rows as f64);
        let shown = (cell / shape).min(shape / cell);
        let used = n as f64 / (rows * cols) as f64;
        let score = shown * used;
        if score > best.2 + EPS {
            best = (cols, rows, score);
        }
    }
    (best.0, best.1)
}

/// The spacing in page units for `spacing` (a fraction of the page's short side), capped so the
/// gaps of `cols` columns and `rows` rows (between the cells and round the edge) leave the cells
/// at least half of the page.
pub fn gap_for(spacing: f64, ratio: f64, cols: usize, rows: usize) -> f64 {
    let spacing = if spacing.is_finite() { spacing.clamp(0.0, 1.0) } else { 0.0 };
    let gap = spacing * ratio.min(1.0);
    let cap_x = MAX_GAP_SHARE * ratio / (cols as f64 + 1.0);
    let cap_y = MAX_GAP_SHARE / (rows as f64 + 1.0);
    gap.min(cap_x).min(cap_y)
}

/// One slot per picture of `ctx`, in its order.
pub(super) fn layout(ctx: &Ctx) -> Vec<Slot> {
    let n = ctx.aspects.len();
    if n == 0 {
        return Vec::new();
    }
    let r = ctx.page_ratio;
    let (cols, rows) = columns_rows(n, r, typical_aspect(ctx.aspects));
    let gap = gap_for(ctx.settings.spacing, r, cols, rows);
    let area = Rect::new(gap, gap, r - 2.0 * gap, 1.0 - 2.0 * gap);
    let Some(cells) = grid_rects(area, cols, rows, gap, n, true) else {
        return super::simple_grid(ctx);
    };
    cells.iter().zip(ctx.aspects).map(|(cell, &aspect)| cell.slot(r, 0.0, fill(aspect, cell.w / cell.h))).collect()
}
