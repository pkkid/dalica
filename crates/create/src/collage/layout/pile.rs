//! The Picture Pile layout (`creating.md` §3): the pictures scattered over the page, each whole,
//! turned within ±[`MAX_TURN`]°, stacked in slot order.
//!
//! The page is cut into one cell per picture (the Grid's rows and columns,
//! [`super::grid::columns_rows`]; a last row with fewer pictures shares the page's width among
//! them), the seed deals the cells out to the pictures, and each picture is centred near its cell
//! with an area of [`COVER`] times the page's area over the count, so neighbours overlap and the
//! pile covers the page for any count. A picture (with its border) never leaves the page: one too large for it is shrunk and
//! one near an edge is pushed back in. The user's moves apply over this ([`super::layout`]).
//!
//! Every number comes from the seed through integer arithmetic and the four basic operations and
//! square roots (exact in IEEE 754); the turn's sine and cosine are short series of our own, not
//! the system's, so a seed lays out the same on every system.

use dalica_core::Rect01;

use super::grid;
use super::{Ctx, Slot, sane_aspect};
use crate::collage::seed::Rng;

/// The largest turn the layout gives a picture, in degrees either way.
pub const MAX_TURN: f64 = 15.0;
/// A picture's area in shares of the page (the page's area over the count), before the size's
/// own spread.
pub const COVER: f64 = 2.0;
/// How far a picture's size spreads, either way, as a fraction of it.
pub const SIZE_SPREAD: f64 = 0.1;
/// How far a picture's centre strays from its cell's, as a fraction of the cell.
pub const JITTER: f64 = 0.25;

/// The seed's streams, one per purpose ([`Rng::stream`]).
const CELLS: u64 = 0;
const SIZES: u64 = 1;
const TURNS: u64 = 2;
const OFFSETS: u64 = 3;

/// Sine and cosine of `deg` degrees, for `|deg|` up to [`MAX_TURN`]: Taylor series to the 11th
/// power (error far below 1e-15 there), so every system computes the same numbers.
fn sin_cos(deg: f64) -> (f64, f64) {
    let x = deg.clamp(-MAX_TURN, MAX_TURN) * (std::f64::consts::PI / 180.0);
    let x2 = x * x;
    let sin = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))));
    let cos = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0 * (1.0 - x2 / 56.0 * (1.0 - x2 / 90.0 * (1.0 - x2 / 132.0)))));
    (sin, cos)
}

/// The cells of `n` pictures over a page of `ratio` (page units: the height is 1), as centre and
/// size, in reading order.
fn cells(n: usize, ratio: f64, shape: f64) -> Vec<([f64; 2], [f64; 2])> {
    let (cols, rows) = grid::columns_rows(n, ratio, shape);
    let ch = 1.0 / rows as f64;
    (0..n)
        .map(|i| {
            let (row, col) = (i / cols, i % cols);
            let in_row = (n - row * cols).min(cols);
            let cw = ratio / in_row as f64;
            ([cw * (col as f64 + 0.5), ch * (row as f64 + 0.5)], [cw, ch])
        })
        .collect()
}

/// One slot per picture of `ctx`, in its order.
pub(super) fn layout(ctx: &Ctx) -> Vec<Slot> {
    let n = ctx.aspects.len();
    if n == 0 {
        return Vec::new();
    }
    let r = ctx.page_ratio;
    let mut cells = cells(n, r, grid::typical_aspect(ctx.aspects));
    Rng::stream(ctx.seed, CELLS).shuffle(&mut cells);
    let (mut sizes, mut turns, mut offsets) = (Rng::stream(ctx.seed, SIZES), Rng::stream(ctx.seed, TURNS), Rng::stream(ctx.seed, OFFSETS));
    let border = ctx.settings.border;
    cells
        .iter()
        .zip(ctx.aspects)
        .map(|(&([cx, cy], [cw, ch]), &aspect)| {
            let a = sane_aspect(Some(aspect));
            let spread = sizes.range(1.0 - SIZE_SPREAD, 1.0 + SIZE_SPREAD);
            let angle = turns.range(-MAX_TURN, MAX_TURN);
            let (dx, dy) = (offsets.range(-JITTER, JITTER) * cw, offsets.range(-JITTER, JITTER) * ch);
            // The picture's size in page units: COVER shares of the page, the same for every
            // cell (a last row's wider cells place their pictures, they do not enlarge them).
            let mut h = (COVER * spread * r / n as f64 / a).sqrt();
            let mut w = a * h;
            // Half the turned box of the picture and its border (the border taken all round at
            // its widest, so the bottom of an instant border stays on the page too).
            let (sin, cos) = sin_cos(angle);
            let edge = border.sides().max(border.bottom());
            let half = |w: f64, h: f64| {
                let b = edge * w.min(h);
                let (bw, bh) = (w + 2.0 * b, h + 2.0 * b);
                ((bw * cos.abs() + bh * sin.abs()) / 2.0, (bw * sin.abs() + bh * cos.abs()) / 2.0)
            };
            let (hx, hy) = half(w, h);
            let shrink = (r / (2.0 * hx)).min(1.0 / (2.0 * hy)).min(1.0);
            if shrink < 1.0 {
                // A hair under the page, so rounding never puts a corner past its edge.
                w *= shrink * (1.0 - 1e-9);
                h *= shrink * (1.0 - 1e-9);
            }
            let (hx, hy) = half(w, h);
            let x = (cx + dx).clamp(hx, (r - hx).max(hx));
            let y = (cy + dy).clamp(hy, (1.0 - hy).max(hy));
            Slot { centre: [x / r, y], size: [w / r, h], angle, shown: Rect01::FULL }
        })
        .collect()
}
