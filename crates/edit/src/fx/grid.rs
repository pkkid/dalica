//! A wide Gaussian blur on a coarse grid of cell means ([`GridBlur`], M13.34), made once per
//! render for every band, so its cost stays near the picture's size whatever the radius.
//!
//! - **Grid:** cells of about a third of the sigma ([`CELL_SIGMA`]; at most [`MAX_CELL`] pixels;
//!   1 pixel for a blur under `CELL_SIGMA` pixels, where the grid is the pixels themselves),
//!   anchored at multiples of the cell in the whole output. Each cell's mean is summed straight
//!   from the window's 8-bit rows (the picture's edge pixels repeated past it, as
//!   [`crate::ops::View::get`] reads them); [`GridBlur::rgb_weighted`] weighs each pixel first
//!   (Glow's highlights), [`GridBlur::luma`] keeps one channel (Rec.601, [`super::luma`]).
//! - **Blur:** the grid is blurred by the box passes ([`Gaussian`]) with the sigma left once the
//!   averaging and the bilinear read are taken off (they add about `cell² / 4` to the variance),
//!   and read back bilinearly at a pixel's centre ([`GridBlur::read`], [`GridBlur::read_row`]). A
//!   blur under the smallest box blur ([`MIN_SIGMA`]) is that blur mixed in by the share of its
//!   variance ([`GridBlur::strength`], [`GridBlur::mix`]), so a small render blurs as a large
//!   one does once shrunk.
//! - **Along a band** (M13.47, from the effects' own readers of M13.36–M13.40): [`GridBlur::rows`]
//!   and [`GridBlur::luma_rows`] read a grid back along a band's rows ([`GridRows`]) with the values
//!   of `read` and `read_luma`, bit for bit: the place of each pixel between its two grid columns
//!   is worked out once per band, each grid row the band reads is spread across its pixels once,
//!   and each output row is the mix of the two it lies between, a flat loop.
//! - **Once per render:** the grid is made by the first band that asks for it (in a `OnceLock`,
//!   through [`super::apart`], as [`super::Blurred`] is) over the whole window the render holds,
//!   and every band reads it. A pass's [`crate::ops::Reach`] is `Reach::Pixels(blur.reach())`:
//!   over a region window and that margin the grid gives exactly the whole render's values inside
//!   the region (the cells under the margin's outer edge are partial, and the grid's own reach,
//!   the bilinear read and that partial cell stay inside the margin).
//!
//! Our own code: the grid blur HDR Look, Dream Glow, Plastic Lens and Soft Focus each made for
//! their bands (M12.10, M12.12), generalised.

use std::sync::OnceLock;

use rayon::prelude::*;

use super::blur::{Gaussian, Mono, Plane, apart};
use super::{UNIT8, luma, mix};
use crate::ops::View;

/// The smallest blur three whole-pixel boxes make (radii 0, 0, 1: σ ≈ 0.82 px); a smaller one is
/// this blur mixed in by the share of its variance.
pub const MIN_SIGMA: f64 = 0.82;
/// A blur runs on a grid of cells about this many times smaller than its sigma.
pub const CELL_SIGMA: f64 = 3.0;
/// The largest grid cell, in output pixels.
pub const MAX_CELL: u32 = 32;

/// The RGBA8 pixels of a render's window placed in the whole output: what a grid is made from
/// (a pass's `before` view, [`Window::of`]).
#[derive(Clone, Copy, Debug)]
pub struct Window<'a> {
    data: &'a [u8],
    x0: u32,
    y0: u32,
    width: u32,
    height: u32,
    full: (u32, u32),
}

impl<'a> Window<'a> {
    /// `width × height` RGBA8 pixels (`data`, row-major) at `(x0, y0)` of a `full` output; `None`
    /// when it is empty, `data` is not that size, or it is not inside the output.
    pub fn new(data: &'a [u8], x0: u32, y0: u32, width: u32, height: u32, full: (u32, u32)) -> Option<Window<'a>> {
        let n = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        let inside = x0.checked_add(width).is_some_and(|x1| x1 <= full.0) && y0.checked_add(height).is_some_and(|y1| y1 <= full.1);
        (n > 0 && data.len() == n && inside).then_some(Window { data, x0, y0, width, height, full })
    }

    /// The window a pass's `before` view holds.
    pub fn of(view: &View<'a>) -> Option<Window<'a>> {
        Window::new(view.data, view.x0, view.y0, view.width, view.height, (view.full_width, view.full_height))
    }

    /// The row of the window holding the output row `y`, clamped into the picture and then into
    /// the window (as [`View::get`] reads).
    fn row(&self, y: i64) -> &'a [u8] {
        let y = y.clamp(0, i64::from(self.full.1.max(1)) - 1);
        let ly = (y - i64::from(self.y0)).clamp(0, i64::from(self.height.max(1)) - 1) as usize;
        let w = self.width as usize * 4;
        self.data.get(ly * w..(ly + 1) * w).unwrap_or(&[])
    }

    /// The column of the window holding the output column `x`, clamped likewise.
    fn column(&self, x: i64) -> usize {
        let x = x.clamp(0, i64::from(self.full.0.max(1)) - 1);
        (x - i64::from(self.x0)).clamp(0, i64::from(self.width.max(1)) - 1) as usize
    }
}

/// A Gaussian blur on a grid of cell means, made once per render.
#[derive(Debug)]
pub struct GridBlur {
    /// The blur on the grid.
    gaussian: Gaussian,
    /// The cell's side in output pixels (1: the grid is the pixels).
    cell: u32,
    /// How much of the blur is mixed in (below 1 only for a blur under `MIN_SIGMA`).
    strength: f32,
    rgb: OnceLock<Option<Plane>>,
    mono: OnceLock<Option<Mono>>,
}

impl GridBlur {
    /// A blur of `sigma` output pixels over a `size` output; `None` when it changes nothing.
    pub fn new(sigma: f64, size: (u32, u32)) -> Option<GridBlur> {
        if !sigma.is_finite() || sigma <= 0.0 {
            return None;
        }
        let cell = (sigma / CELL_SIGMA).floor().clamp(1.0, f64::from(MAX_CELL));
        let on_grid = (sigma * sigma - if cell > 1.0 { cell * cell / 4.0 } else { 0.0 }).max(0.0).sqrt() / cell;
        let grid = (size.0.div_ceil(cell as u32).max(1), size.1.div_ceil(cell as u32).max(1));
        let gaussian = Gaussian::new(on_grid.max(MIN_SIGMA), grid);
        let strength = (on_grid / gaussian.sigma().max(1e-9)).powi(2).min(1.0) as f32;
        if gaussian.is_identity() || strength < 1e-4 {
            return None;
        }
        Some(GridBlur { gaussian, cell: cell as u32, strength, rgb: OnceLock::new(), mono: OnceLock::new() })
    }

    /// The blur on the grid.
    pub fn gaussian(&self) -> Gaussian {
        self.gaussian
    }

    /// The cell's side in output pixels.
    pub fn cell(&self) -> u32 {
        self.cell
    }

    /// How much of the blur [`GridBlur::mix`] mixes in.
    pub fn strength(&self) -> f32 {
        self.strength
    }

    /// The standard deviation of the whole blur in output pixels: the grid's blur, the cells'
    /// averaging and the bilinear read, mixed in by the strength.
    pub fn sigma(&self) -> f64 {
        let c = f64::from(self.cell);
        let spread = self.gaussian.sigma() * c;
        let extra = if self.cell > 1 { c * c / 4.0 } else { 0.0 };
        (f64::from(self.strength) * (spread * spread + extra)).sqrt()
    }

    /// How far a blurred pixel reads, in output pixels: the grid's reach, the bilinear read and
    /// the cells' extent.
    pub fn reach(&self) -> u32 {
        self.gaussian.reach().saturating_add(3).saturating_mul(self.cell)
    }

    /// The blurred grid of the window's RGB means; every later call answers the first one's grid
    /// (a pass makes one per render, [`GridBlur::rgb_weighted`]'s included). `None` when its
    /// buffers cannot be had.
    pub fn rgb(&self, window: Window<'_>) -> Option<&Plane> {
        self.rgb.get_or_init(|| apart(|| self.blurred(self.means(window)?))).as_ref()
    }

    /// The blurred grid of the means of the window's pixels each through `weight` (in working
    /// values); every later call answers the first one's grid ([`GridBlur::rgb`]'s included).
    pub fn rgb_weighted(&self, window: Window<'_>, weight: impl Fn([f32; 3]) -> [f32; 3] + Sync) -> Option<&Plane> {
        self.rgb.get_or_init(|| apart(|| self.blurred(self.weighted_means(window, &weight)?))).as_ref()
    }

    /// The blurred grid of the window's Rec.601 lumas; every later call answers the first one's.
    pub fn luma(&self, window: Window<'_>) -> Option<&Mono> {
        self.mono
            .get_or_init(|| {
                apart(|| {
                    let means = self.means(window)?;
                    let mut mono = Mono::from_fn(means.x0, means.y0, means.width, means.height, |x, y| luma(means.get(i64::from(x), i64::from(y))))?;
                    self.gaussian.apply_mono(&mut mono).then_some(mono)
                })
            })
            .as_ref()
    }

    /// The blur at the centre of the output pixel `(x, y)`, read from `grid` (made by this blur)
    /// bilinearly; not yet mixed by the strength ([`GridBlur::mix`]).
    pub fn read(&self, grid: &Plane, x: u32, y: u32) -> [f32; 3] {
        if self.cell == 1 {
            return grid.get(i64::from(x), i64::from(y));
        }
        let (i, tx) = self.tap(x);
        let (j, ty) = self.tap(y);
        let top = mix(grid.get(i, j), grid.get(i + 1, j), tx);
        let bottom = mix(grid.get(i, j + 1), grid.get(i + 1, j + 1), tx);
        mix(top, bottom, ty)
    }

    /// [`GridBlur::read`] for the pixels `(x0, y) … (x0 + out.len() − 1, y)`, into `out`: the
    /// same values, two grid rows fetched once.
    pub fn read_row(&self, grid: &Plane, x0: u32, y: u32, out: &mut [[f32; 3]]) {
        let xs = (0..out.len()).map(|i| x0.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)));
        if self.cell == 1 {
            for (o, x) in out.iter_mut().zip(xs) {
                *o = grid.get(i64::from(x), i64::from(y));
            }
            return;
        }
        let (j, ty) = self.tap(y);
        let (top, bottom) = (grid_row(grid, j), grid_row(grid, j + 1));
        let last = i64::from(grid.width.max(1)) - 1;
        let at = |row: &[[f32; 3]], i: i64| row.get((i - i64::from(grid.x0)).clamp(0, last) as usize).copied().unwrap_or([0.0; 3]);
        for (o, x) in out.iter_mut().zip(xs) {
            let (i, tx) = self.tap(x);
            let t = mix(at(top, i), at(top, i + 1), tx);
            let b = mix(at(bottom, i), at(bottom, i + 1), tx);
            *o = mix(t, b, ty);
        }
    }

    /// [`GridBlur::read`] on a [`GridBlur::luma`] grid.
    pub fn read_luma(&self, grid: &Mono, x: u32, y: u32) -> f32 {
        if self.cell == 1 {
            return grid.get(i64::from(x), i64::from(y));
        }
        let (i, tx) = self.tap(x);
        let (j, ty) = self.tap(y);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let top = lerp(grid.get(i, j), grid.get(i + 1, j), tx);
        let bottom = lerp(grid.get(i, j + 1), grid.get(i + 1, j + 1), tx);
        lerp(top, bottom, ty)
    }

    /// `blurred` (a [`GridBlur::read`]) mixed into the pixel's own value `input` by the strength.
    pub fn mix(&self, input: [f32; 3], blurred: [f32; 3]) -> [f32; 3] {
        mix(input, blurred, self.strength)
    }

    /// The grid cell before the pixel coordinate `p` along one axis and the weight of the next.
    fn tap(&self, p: u32) -> (i64, f32) {
        tap(self.cell, p)
    }

    /// A reader of `grid` (an RGB grid this blur made, [`GridBlur::rgb`]) along the rows of `width`
    /// pixels from the output column `x0`: [`GridBlur::read`]'s values, bit for bit (three a
    /// pixel). `None` when its buffers cannot be had.
    pub fn rows<'g>(&self, grid: &'g Plane, x0: u32, width: usize) -> Option<GridRows<'g, 3>> {
        GridRows::new(self, grid.into(), x0, width)
    }

    /// [`GridBlur::rows`] on a [`GridBlur::luma`] grid: [`GridBlur::read_luma`]'s values, bit for
    /// bit (one a pixel).
    pub fn luma_rows<'g>(&self, grid: &'g Mono, x0: u32, width: usize) -> Option<GridRows<'g, 1>> {
        GridRows::new(self, grid.into(), x0, width)
    }

    /// The cells under the window: whole-output cell coordinates of the first, and the count.
    fn cells(&self, window: &Window<'_>) -> (u32, u32, u32, u32) {
        let c = self.cell.max(1);
        let (left, top) = (window.x0 / c, window.y0 / c);
        let right = window.x0.saturating_add(window.width).div_ceil(c);
        let bottom = window.y0.saturating_add(window.height).div_ceil(c);
        (left, top, right.saturating_sub(left), bottom.saturating_sub(top))
    }

    /// For every pixel column under the grid, the window's column it reads.
    fn columns(&self, window: &Window<'_>, left: u32, cols: u32) -> Option<Vec<usize>> {
        let c = i64::from(self.cell.max(1));
        let n = (cols as usize).checked_mul(c as usize)?;
        let mut out = Vec::new();
        out.try_reserve_exact(n).ok()?;
        let x0 = i64::from(left) * c;
        out.extend((0..n as i64).map(|k| window.column(x0 + k)));
        Some(out)
    }

    /// Each cell's RGB mean, summed in integers from the 8-bit values.
    fn means(&self, window: Window<'_>) -> Option<Plane> {
        let (left, top, cols, rows) = self.cells(&window);
        let columns = self.columns(&window, left, cols)?;
        let c = self.cell.max(1) as usize;
        let scale = 1.0 / (c * c) as f32 / 255.0;
        let mut plane = Plane::from_fn(left, top, cols, rows, |_, _| [0.0; 3])?;
        plane.data.par_chunks_mut(cols as usize).enumerate().for_each(|(j, out)| {
            let y0 = (i64::from(top) + j as i64) * c as i64;
            let mut sums = vec![[0u32; 3]; out.len()];
            for y in y0..y0 + c as i64 {
                let row = window.row(y);
                for (sum, xs) in sums.iter_mut().zip(columns.chunks_exact(c)) {
                    for &lx in xs {
                        if let Some(p) = row.get(lx * 4..lx * 4 + 3) {
                            *sum = [sum[0] + u32::from(p[0]), sum[1] + u32::from(p[1]), sum[2] + u32::from(p[2])];
                        }
                    }
                }
            }
            for (o, s) in out.iter_mut().zip(&sums) {
                *o = s.map(|v| v as f32 * scale);
            }
        });
        Some(plane)
    }

    /// Each cell's mean of its pixels through `weight`, summed in `f32` row by row.
    fn weighted_means(&self, window: Window<'_>, weight: &(impl Fn([f32; 3]) -> [f32; 3] + Sync)) -> Option<Plane> {
        let (left, top, cols, rows) = self.cells(&window);
        let columns = self.columns(&window, left, cols)?;
        let c = self.cell.max(1) as usize;
        let n = (c * c) as f32;
        let mut plane = Plane::from_fn(left, top, cols, rows, |_, _| [0.0; 3])?;
        plane.data.par_chunks_mut(cols as usize).enumerate().for_each(|(j, out)| {
            let y0 = (i64::from(top) + j as i64) * c as i64;
            let mut sums = vec![[0.0f32; 3]; out.len()];
            for y in y0..y0 + c as i64 {
                let row = window.row(y);
                for (sum, xs) in sums.iter_mut().zip(columns.chunks_exact(c)) {
                    for &lx in xs {
                        if let Some(p) = row.get(lx * 4..lx * 4 + 3) {
                            let w = weight([UNIT8[usize::from(p[0])], UNIT8[usize::from(p[1])], UNIT8[usize::from(p[2])]]);
                            *sum = [sum[0] + w[0], sum[1] + w[1], sum[2] + w[2]];
                        }
                    }
                }
            }
            for (o, s) in out.iter_mut().zip(&sums) {
                *o = s.map(|v| v / n);
            }
        });
        Some(plane)
    }

    fn blurred(&self, mut plane: Plane) -> Option<Plane> {
        self.gaussian.apply(&mut plane).then_some(plane)
    }
}

/// The grid cell before the pixel coordinate `p` along one axis, for cells of `cell` pixels, and
/// the weight of the next (a pixel's centre placed on the grid's cell centres).
fn tap(cell: u32, p: u32) -> (i64, f32) {
    let c = f64::from(cell.max(1));
    let u = (f64::from(p) + 0.5) / c - 0.5;
    // `⌊u⌋` (`u` is above −0.5) without a call to the library's `floor`.
    let i = u as i64 - i64::from(u < 0.0);
    (i, (u - i as f64) as f32)
}

/// A grid's cells as flat values, row-major, placed in the whole output by cells: an RGB grid
/// ([`Plane`], three values a cell) or a luma grid ([`Mono`], one), for [`GridRows::new`].
#[derive(Clone, Copy, Debug)]
pub struct Cells<'g> {
    pub x0: u32,
    pub y0: u32,
    pub width: u32,
    pub height: u32,
    pub data: &'g [f32],
}

impl<'g> From<&'g Plane> for Cells<'g> {
    fn from(p: &'g Plane) -> Cells<'g> {
        Cells { x0: p.x0, y0: p.y0, width: p.width, height: p.height, data: p.data.as_flattened() }
    }
}

impl<'g> From<&'g Mono> for Cells<'g> {
    fn from(m: &'g Mono) -> Cells<'g> {
        Cells { x0: m.x0, y0: m.y0, width: m.width, height: m.height, data: &m.data }
    }
}

/// A grid read back along a band's rows, `C` values a pixel ([`GridBlur::rows`],
/// [`GridBlur::luma_rows`]): the bilinear read of [`GridBlur::read`] (its lerp across, then its
/// lerp down, in the same order), bit for bit. Each pixel's two grid columns and weight are found
/// once; each grid row the rows read is spread across the pixels once and kept while the next
/// rows read it (two at a time).
pub struct GridRows<'g, const C: usize> {
    /// The grid: `C` values a cell, row-major, and its rows.
    data: &'g [f32],
    y0: u32,
    height: u32,
    /// Values a grid row.
    stride: usize,
    cell: u32,
    /// Per pixel: where its two grid columns start in a grid row, and the weight of the second.
    taps: Vec<(u32, u32, f32)>,
    /// Two grid rows spread across the pixels (`C` values a pixel), and which grid rows they are.
    spread: [(i64, Vec<f32>); 2],
}

impl<'g, const C: usize> GridRows<'g, C> {
    /// A reader of `cells` (a grid `blur` made, `C` values a cell) along the rows of `width`
    /// pixels from the output column `x0`; `None` when the cells are not `C` values each or its
    /// buffers cannot be had.
    pub fn new(blur: &GridBlur, cells: Cells<'g>, x0: u32, width: usize) -> Option<GridRows<'g, C>> {
        let Cells { x0: gx0, y0, width: gw, height, data } = cells;
        let cell = blur.cell;
        let stride = (gw as usize).checked_mul(C)?;
        if stride.checked_mul(height as usize)? != data.len() || C == 0 {
            return None;
        }
        let last = i64::from(gw.max(1)) - 1;
        let at = |i: i64| u32::try_from((i - i64::from(gx0)).clamp(0, last) as usize * C).unwrap_or(u32::MAX);
        let mut taps = Vec::new();
        taps.try_reserve_exact(width).ok()?;
        taps.extend((0..width).map(|k| {
            let x = x0.saturating_add(u32::try_from(k).unwrap_or(u32::MAX));
            if cell <= 1 {
                (at(i64::from(x)), 0, 0.0)
            } else {
                let (i, t) = tap(cell, x);
                (at(i), at(i + 1), t)
            }
        }));
        // One-pixel cells read the grid's values themselves: no row is spread.
        let n = if cell <= 1 { 0 } else { width.checked_mul(C)? };
        let line = || {
            let mut v = Vec::new();
            v.try_reserve_exact(n).ok()?;
            v.resize(n, 0.0f32);
            Some(v)
        };
        Some(GridRows { data, y0, height, stride, cell, taps, spread: [(i64::MIN, line()?), (i64::MIN, line()?)] })
    }

    /// The grid row `j` (clamped into the grid).
    fn grid_row(&self, j: i64) -> &'g [f32] {
        let lj = (j - i64::from(self.y0)).clamp(0, i64::from(self.height.max(1)) - 1) as usize;
        let at = lj.saturating_mul(self.stride);
        self.data.get(at..at.saturating_add(self.stride)).unwrap_or(&[])
    }

    /// The blur at the pixels of the output row `y`, into `out` (`C` values a pixel); not yet
    /// mixed by the strength ([`GridBlur::mix`]).
    pub fn read(&mut self, y: u32, out: &mut [f32]) {
        let out = out.as_chunks_mut::<C>().0;
        if self.cell <= 1 {
            let row = self.grid_row(i64::from(y));
            for (o, &(i0, _, _)) in out.iter_mut().zip(&self.taps) {
                let i0 = i0 as usize;
                if let Some(v) = row.get(i0..i0 + C) {
                    o.copy_from_slice(v);
                }
            }
            return;
        }
        let (j, ty) = tap(self.cell, y);
        if self.spread[0].0 != j {
            if self.spread[1].0 == j {
                self.spread.swap(0, 1);
            } else {
                let row = self.grid_row(j);
                spread_row::<C>(row, &self.taps, &mut self.spread[0].1);
                self.spread[0].0 = j;
            }
        }
        if self.spread[1].0 != j + 1 {
            let row = self.grid_row(j + 1);
            spread_row::<C>(row, &self.taps, &mut self.spread[1].1);
            self.spread[1].0 = j + 1;
        }
        let [(_, top), (_, bottom)] = &self.spread;
        for ((o, &a), &b) in out.as_flattened_mut().iter_mut().zip(top).zip(bottom) {
            *o = a + (b - a) * ty;
        }
    }
}

/// The grid row `row` read across the pixels at `taps`, into `out` (`C` values a pixel).
fn spread_row<const C: usize>(row: &[f32], taps: &[(u32, u32, f32)], out: &mut [f32]) {
    for (o, &(i0, i1, t)) in out.as_chunks_mut::<C>().0.iter_mut().zip(taps) {
        let (i0, i1) = (i0 as usize, i1 as usize);
        if let (Some(a), Some(b)) = (row.get(i0..i0 + C), row.get(i1..i1 + C)) {
            for ((o, &a), &b) in o.iter_mut().zip(a).zip(b) {
                *o = a + (b - a) * t;
            }
        }
    }
}

/// The row of `grid` holding the grid row `j` (clamped into it).
fn grid_row(grid: &Plane, j: i64) -> &[[f32; 3]] {
    let w = grid.width as usize;
    let lj = (j - i64::from(grid.y0)).clamp(0, i64::from(grid.height.max(1)) - 1) as usize;
    grid.data.get(lj * w..(lj + 1) * w).unwrap_or(&[])
}
