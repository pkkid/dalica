//! The Contact Sheet layout (`creating.md` §3): a grid of equal cells inside a margin, each holding
//! one whole picture (with its border, when the style has one) above a line for its file name.
//! The grid's columns and rows are chosen as the Grid's are ([`super::grid::columns_rows`]), for
//! cells whose picture part is near the pictures' typical shape; the sheet's spacing is its own
//! (the style has no Grid spacing setting). [`names`] answers where each name goes, for the render
//! and the canvas.

use dalica_core::{PhotoId, Rect01};

use super::grid::{self, Rect};
use super::{Ctx, Slot, sane_aspect};
use crate::collage::doc::{Border, Collage};

/// The margin round the sheet, as a fraction of the page's short side.
pub const MARGIN: f64 = 0.03;
/// The space between two cells, as a fraction of the page's short side.
pub const GAP: f64 = 0.015;
/// The name line's height, as a fraction of a cell's height (the picture has the rest).
pub const NAME_LINE: f64 = 0.16;

/// Where one picture's file name goes, normalised to the page: a line under the picture, as wide
/// as its cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NameLine {
    /// The picture's position in the Clips.
    pub clip: usize,
    pub id: PhotoId,
    /// The line's centre as a fraction of the page's width and height.
    pub centre: [f64; 2],
    /// The line's width and height as fractions of the page's width and height.
    pub size: [f64; 2],
}

/// One cell: the part for the picture and the line under it, in page units.
#[derive(Clone, Copy, Debug)]
struct Cell {
    picture: Rect,
    name: Rect,
}

/// The cells of `n` pictures of the typical shape `shape` on a page of `ratio`, in reading order.
fn cells(n: usize, ratio: f64, shape: f64) -> Vec<Cell> {
    if n == 0 {
        return Vec::new();
    }
    let short = ratio.min(1.0);
    let (mut margin, mut gap) = (MARGIN * short, GAP * short);
    let area = |margin: f64| Rect::new(margin, margin, ratio - 2.0 * margin, 1.0 - 2.0 * margin);
    // A cell's shape when its picture part has the pictures' shape.
    let (cols, rows) = grid::columns_rows(n, ratio, shape * (1.0 - NAME_LINE));
    let gaps_fit = |margin: f64, gap: f64| {
        let a = area(margin);
        a.w - gap * (cols - 1) as f64 > a.w * grid::MAX_GAP_SHARE && a.h - gap * (rows - 1) as f64 > a.h * grid::MAX_GAP_SHARE
    };
    if !gaps_fit(margin, gap) {
        // A crowded sheet: the cells touch, so the pictures keep their room.
        gap = 0.0;
        if !gaps_fit(margin, gap) {
            margin = 0.0;
        }
    }
    let rects = grid::grid_rects(area(margin), cols, rows, gap, n, false).unwrap_or_default();
    rects
        .into_iter()
        .map(|c| {
            let line = c.h * NAME_LINE;
            Cell { picture: Rect::new(c.x, c.y, c.w, c.h - line), name: Rect::new(c.x, c.bottom() - line, c.w, line) }
        })
        .collect()
}

/// The slot of a whole picture of `aspect` with `border`, the picture and its border together as
/// large as `area` allows, centred in it.
fn whole(area: &Rect, aspect: f64, border: Border, ratio: f64) -> Slot {
    let a = sane_aspect(Some(aspect));
    // The picture's size when its height is 1, and the border's widths for it.
    let short = a.min(1.0);
    let (side, bottom) = (border.sides() * short, border.bottom() * short);
    let (outer_w, outer_h) = (a + 2.0 * side, 1.0 + side + bottom);
    let Some((_, oh)) = grid::fit(outer_w, outer_h, area.w, area.h, false) else {
        return area.slot(ratio, 0.0, Rect01::FULL);
    };
    let k = oh / outer_h;
    let (w, h) = (a * k, k);
    let [cx, cy] = area.centre();
    let top = cy - oh / 2.0 + side * k;
    Rect::new(cx - w / 2.0, top, w, h).slot(ratio, 0.0, Rect01::FULL)
}

/// One slot per picture of `ctx`, in its order.
pub(super) fn layout(ctx: &Ctx) -> Vec<Slot> {
    let r = ctx.page_ratio;
    let cells = cells(ctx.aspects.len(), r, grid::typical_aspect(ctx.aspects));
    cells.iter().zip(ctx.aspects).map(|(cell, &aspect)| whole(&cell.picture, aspect, ctx.settings.border, r)).collect()
}

/// Where each picture's file name goes on a contact sheet of `doc` (whatever its style), in slot
/// order; `aspects` as for [`super::layout`].
pub fn names(doc: &Collage, aspects: &[f64]) -> Vec<NameLine> {
    let order = doc.assignment();
    let ratios: Vec<f64> = order.iter().map(|&i| sane_aspect(aspects.get(i).copied())).collect();
    let r = sane_aspect(Some(doc.page.ratio()));
    let cells = cells(order.len(), r, grid::typical_aspect(&ratios));
    order
        .iter()
        .zip(cells)
        .filter_map(|(&clip, cell)| {
            let c = doc.clips.get(clip)?;
            let slot = cell.name.slot(r, 0.0, Rect01::FULL);
            Some(NameLine { clip, id: c.id, centre: slot.centre, size: slot.size })
        })
        .collect()
}
