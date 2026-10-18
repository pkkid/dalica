//! Seeded noise placed in the stored frame: a function of the stored-frame point (in units of the
//! stored photo's shorter side, [`super::Space::units`]), the entry's seed and a channel, so the
//! same point has the same noise in a thumbnail, the edit view, a zoomed region and a Save.
//!
//! - **Field:** smooth value noise: a value at each corner of a square lattice of side `cell`
//!   (hashed from the corner, the channel and the seed), blended across the cell with a smoothstep.
//!   [`Noise::value`] has mean 0 and a standard deviation of 1 over the picture (the lattice values
//!   are uniform in −1..1 and the blend's variance is known: [`SPREAD`]).
//! - **Seed:** [`seed`] hashes an entry's parameter text, so two entries with the same parameters
//!   draw the same noise and any change of a parameter draws another.
//! - **Pixels:** [`Noise::at`] samples the field at an output pixel's centre and, where a pixel
//!   spans more than a cell, scales the value down by `cell / pixel` (the spread of the mean of the
//!   cells under the pixel), so a small render's grain is as strong as a large render's grain
//!   after it is shrunk.
//! - **Rows** (M13.34): [`Noise::row`] and [`Noise::row_values`] fill a row of pixels at once,
//!   for the effects that draw noise at every pixel: the stored-frame point steps along the row
//!   by one affine step per pixel, a lattice corner is hashed once per cell the row crosses, and
//!   the smoothstep blend runs in `f32`. They agree with [`Noise::at`] and [`Noise::value`] within
//!   10⁻⁴.
//! - **Runs and bands** (M13.47, from the effects' own samplers of M13.36–M13.40): the same values
//!   as the rows, bit for bit, made faster where the stored frame lies square in the output.
//!   [`Noise::run_values`] and [`Noise::run`] give a run of at most [`RUN`] pixels: where it stays
//!   on one row of lattice cells (an upright render that is not straightened, its shorter side at
//!   least about 420 pixels for cells of a thousandth of it) each lattice corner along it is
//!   hashed once, in a loop that vectorises (Grain, Widescreen Film). [`Noise::lines`] serves a
//!   band's rows ([`Lines`]): each lattice row the band lies between is hashed and blended across
//!   the band once, and a row is the blend of its two (Plastic Lens).
//!
//! Ported from PhotoCraft `crates/algo/src/noise.rs` (`hash01`) at revision e4b2868a and LightCraft
//! `crates/pipeline/src/finish.rs` (`grain_noise`: the smoothstep blend of lattice values) at
//! revision 0da998f2 (MIT OR Apache-2.0; `NOTICE`). Dalica's changes: `hash01` gives the lattice
//! values (instead of LightCraft's own hash), coordinates are in the stored frame in `f64`, the
//! field is scaled to a unit spread, and the attenuation for pixels larger than a cell.

use super::space::Space;

/// The standard deviation of the smoothstep blend of lattice values uniform in −1..1, over the
/// picture: `(26/35) / √3` (the mean over a cell of `(1 − s)² + s²` is 26/35 along each axis, for
/// `s` the smoothstep of the position in the cell, and a uniform value's variance is 1/3).
pub const SPREAD: f64 = 26.0 / 35.0 / 1.732_050_807_568_877_2;

/// The smallest cell, in units of the stored photo's shorter side (a millionth: finer than any
/// render's pixel; it also keeps lattice indices inside `i32`).
pub const MIN_CELL: f64 = 1e-6;

/// Smooth value noise of one seed and cell size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    seed: u32,
    cell: f64,
}

impl Noise {
    /// Noise drawn by `seed` with lattice cells `cell` wide, relative to the stored photo's
    /// shorter side (brought into [`MIN_CELL`]..1; `NaN` counts as the smallest).
    pub fn new(seed: u32, cell: f64) -> Noise {
        let cell = if cell.is_nan() { MIN_CELL } else { cell.clamp(MIN_CELL, 1.0) };
        Noise { seed, cell }
    }

    pub fn cell(&self) -> f64 {
        self.cell
    }

    /// The field at `(u, v)` (units of the stored photo's shorter side) for `channel`: mean 0,
    /// standard deviation 1 over the picture, never more than about 2.33 in size. Non-finite
    /// coordinates give 0.
    pub fn value(&self, u: f64, v: f64, channel: u32) -> f32 {
        let (x, y) = (u / self.cell, v / self.cell);
        if !(x.is_finite() && y.is_finite()) {
            return 0.0;
        }
        let (x, y) = (x.clamp(-1e9, 1e9), y.clamp(-1e9, 1e9));
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (i, j) = (x0 as i64, y0 as i64);
        let h = |a: i64, b: i64| 2.0 * f64::from(hash01(a as i32, b as i32, channel, self.seed)) - 1.0;
        let (su, sv) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let top = h(i, j) + (h(i + 1, j) - h(i, j)) * su;
        let bottom = h(i, j + 1) + (h(i + 1, j + 1) - h(i, j + 1)) * su;
        ((top + (bottom - top) * sv) / SPREAD) as f32
    }

    /// The field at the centre of the output pixel `(x, y)` of `space` for `channel`, scaled down
    /// where a pixel spans more than a cell.
    pub fn at(&self, space: &Space, x: u32, y: u32, channel: u32) -> f32 {
        let (u, v) = space.pixel_units(x, y);
        self.value(u, v, channel) * self.attenuation(space)
    }

    /// [`Noise::at`] for the pixels `(x0, y) … (x0 + out.len() − 1, y)` of `space` and `channel`,
    /// into `out` (within 10⁻⁴ of it).
    pub fn row(&self, space: &Space, x0: u32, y: u32, channel: u32, out: &mut [f32]) {
        self.row_values(space, x0, y, channel, out);
        let k = self.attenuation(space);
        if k != 1.0 {
            for v in out.iter_mut() {
                *v *= k;
            }
        }
    }

    /// [`Noise::value`] at the centres of the pixels `(x0, y) … (x0 + out.len() − 1, y)` of
    /// `space` for `channel` (without [`Noise::attenuation`]), into `out` (within 10⁻⁴ of it).
    pub fn row_values(&self, space: &Space, x0: u32, y: u32, channel: u32, out: &mut [f32]) {
        // The pixel centres along the row are an affine function of `x`: the step taken over
        // `STRIDE` pixels (a power of two, so the division is exact) to keep its rounding small.
        const STRIDE: u32 = 1024;
        let (u0, v0) = space.pixel_units(x0, y);
        let (u1, v1) = space.pixel_units(x0.saturating_add(STRIDE), y);
        let span = f64::from(x0.saturating_add(STRIDE) - x0).max(1.0);
        let (x, y) = (u0 / self.cell, v0 / self.cell);
        let (dx, dy) = ((u1 - u0) / span / self.cell, (v1 - v0) / span / self.cell);
        if !(x.is_finite() && y.is_finite() && dx.is_finite() && dy.is_finite()) {
            out.fill(0.0);
            return;
        }
        let spread = SPREAD as f32;
        // The cell last hashed and its corners' values (top-left, top-right, bottom-left,
        // bottom-right).
        let mut cell: Option<(i64, i64)> = None;
        let mut corners = [0.0f32; 4];
        for (i, o) in out.iter_mut().enumerate() {
            let k = i as f64;
            let (px, py) = ((x + dx * k).clamp(-1e9, 1e9), (y + dy * k).clamp(-1e9, 1e9));
            let (ci, cj) = (floor(px), floor(py));
            if cell != Some((ci, cj)) {
                let h = |a: i64, b: i64| 2.0 * hash01(a as i32, b as i32, channel, self.seed) - 1.0;
                corners = [h(ci, cj), h(ci + 1, cj), h(ci, cj + 1), h(ci + 1, cj + 1)];
                cell = Some((ci, cj));
            }
            let (fx, fy) = ((px - ci as f64) as f32, (py - cj as f64) as f32);
            let (su, sv) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
            let top = corners[0] + (corners[1] - corners[0]) * su;
            let bottom = corners[2] + (corners[3] - corners[2]) * su;
            *o = (top + (bottom - top) * sv) / spread;
        }
    }

    /// The factor [`Noise::at`] scales by in `space`: 1 while a pixel is at most a cell, then
    /// `cell / pixel`.
    pub fn attenuation(&self, space: &Space) -> f32 {
        let pixel = space.pixel();
        if pixel.is_finite() && pixel > self.cell { (self.cell / pixel) as f32 } else { 1.0 }
    }
}

/// The most pixels a run of [`Noise::run_values`] takes on its fast path.
pub const RUN: u32 = 256;

/// The lattice corners a run hashes at most: two lattice cells a pixel, and the corner past the
/// last.
const CORNERS: usize = 2 * RUN as usize + 3;

/// [`Noise::run_values`]'s scratch: the hashed corners of two lattice rows and, per cell, its
/// top-left corner, the step to the top-right one and the same below, and which run they are for.
/// One per band (it is about 10 kB), reused run after run: a run on the same lattice cells as the
/// last (the same run one pixel row down, while the rows stay between the same lattice rows)
/// reuses its corners, and one a lattice row further down hashes only its new lower row, so a
/// caller that draws a band a run at a time, row after row, hashes each lattice row about once.
pub struct Corners {
    hashes: [[f32; CORNERS]; 2],
    cells: [[f32; 4]; CORNERS],
    /// The run the corners are for: its first lattice column, how many columns, its upper
    /// lattice row, the channel and the seed.
    made: Option<(i64, usize, i64, u32, u32)>,
    /// Per pixel of the run: its cell (an index into `cells`) and its place across the cell.
    at: [u32; RUN as usize],
    across: [f32; RUN as usize],
}

impl Default for Corners {
    fn default() -> Corners {
        Corners { hashes: [[0.0; CORNERS]; 2], cells: [[0.0; 4]; CORNERS], made: None, at: [0; RUN as usize], across: [0.0; RUN as usize] }
    }
}

impl Noise {
    /// [`Noise::row`] for the pixels `(start, y) … (start + out.len() − 1, y)` of `space` and
    /// `channel`, bit for bit, into `out` ([`Noise::run_values`], then the attenuation).
    pub fn run(&self, space: &Space, start: u32, y: u32, channel: u32, out: &mut [f32], scratch: &mut Corners) {
        self.run_values(space, start, y, channel, out, scratch);
        let k = self.attenuation(space);
        if k != 1.0 {
            for v in out.iter_mut() {
                *v *= k;
            }
        }
    }

    /// [`Noise::row_values`] for the pixels `(start, y) … (start + out.len() − 1, y)` of `space`
    /// and `channel`, bit for bit, into `out`; `scratch` holds the run's corners.
    ///
    /// Where the run is at most [`RUN`] pixels, stays on one row of lattice cells and steps
    /// forward by at most two cells a pixel (a render that is not turned or mirrored, and pixels
    /// no more than two cells wide: the edit view's final and a Save), each lattice corner along
    /// it is hashed once, in a loop that vectorises, rather than four per cell the row enters; the
    /// stored-frame point, the cell and the smoothstep blend are those `row_values` computes.
    /// Elsewhere it is `row_values` itself. Answers whether the run took the fast path.
    pub fn run_values(&self, space: &Space, start: u32, y: u32, channel: u32, out: &mut [f32], scratch: &mut Corners) -> bool {
        // `row_values`'s affine step along the row.
        const STRIDE: u32 = 1024;
        let cell = self.cell;
        let (u0, v0) = space.pixel_units(start, y);
        let (u1, v1) = space.pixel_units(start.saturating_add(STRIDE), y);
        let span = f64::from(start.saturating_add(STRIDE) - start).max(1.0);
        let (x, v) = (u0 / cell, v0 / cell);
        let (dx, dv) = ((u1 - u0) / span / cell, (v1 - v0) / span / cell);
        let last = out.len().saturating_sub(1) as f64;
        let (x_end, v_end) = (x + dx * last, v + dv * last);
        let inside = |t: f64| t.is_finite() && t.abs() < 1e9;
        let row = floor(v);
        let fast = !out.is_empty()
            && out.len() <= RUN as usize
            && [x, v, x_end, v_end].into_iter().all(inside)
            && dx > 0.0
            && dx <= 2.0
            && floor(v_end) == row;
        if !fast {
            self.row_values(space, start, y, channel, out);
            return false;
        }
        let Corners { hashes, cells, made, at: ats, across } = scratch;
        // The two lattice rows' corners from the first pixel's cell to past the last's, hashed
        // once; then per cell its top-left corner and the step to the top-right one, and the same
        // below (the differences the blend takes, made once per cell). The same run as the last
        // keeps them; one a lattice row down keeps the last lower row as its upper one.
        let first = floor(x);
        let count = usize::try_from(floor(x_end) - first + 2).unwrap_or(0).min(CORNERS);
        let key = (first, count, row, channel, self.seed);
        let hash = |out: &mut [f32], j: i64| {
            for (k, h) in out.iter_mut().enumerate() {
                let i = (first as i32).wrapping_add(k as i32);
                *h = 2.0 * hash01(i, j as i32, channel, self.seed) - 1.0;
            }
        };
        let fresh = match *made {
            Some(m) if m == key => false,
            Some((f, n, j, c, s)) if (f, n, c, s) == (first, count, channel, self.seed) && j.checked_add(1) == Some(row) => {
                hashes.swap(0, 1);
                hash(hashes[1].get_mut(..count).unwrap_or(&mut []), row + 1);
                true
            }
            _ => {
                let [top, bottom] = &mut *hashes;
                hash(top.get_mut(..count).unwrap_or(&mut []), row);
                hash(bottom.get_mut(..count).unwrap_or(&mut []), row + 1);
                true
            }
        };
        if fresh {
            let [top, bottom] = &*hashes;
            let (top, bottom) = (top.get(..count).unwrap_or(&[]), bottom.get(..count).unwrap_or(&[]));
            for (c, ((t, t1), (b, b1))) in cells.iter_mut().zip(top.iter().zip(top.iter().skip(1)).zip(bottom.iter().zip(bottom.iter().skip(1)))) {
                *c = [*t, *t1 - *t, *b, *b1 - *b];
            }
            *made = Some(key);
        }
        let spread = SPREAD as f32;
        let (first, row) = (first as f64, row as f64);
        let smooth = |f: f32| f * f * (3.0 - 2.0 * f);
        // The pixels step forward along the run, so each one's cell is found by stepping past
        // the lattice lines it crossed (exact integers in `f64`: `⌊px⌋` itself).
        let (mut at, mut next) = (0usize, first + 1.0);
        if dv == 0.0 {
            // On a row of the stored frame the run's place between the lattice rows is the same
            // at every pixel. Each pixel's cell and place across it first, then the blend four
            // pixels at a time (M13.47: the same operations, in lanes).
            let sv = smooth((v - row) as f32);
            let (mut cell, mut k) = (0u32, 0.0f64);
            for (a, f) in ats.iter_mut().zip(across.iter_mut()).take(out.len()) {
                let px = x + dx * k;
                k += 1.0;
                while px >= next {
                    next += 1.0;
                    cell += 1;
                }
                (*a, *f) = (cell, (px - (next - 1.0)) as f32);
            }
            blend_run(out, cells, ats, across, sv);
            return true;
        }
        let mut blend = |o: &mut f32, px: f64, sv: f32| {
            while px >= next {
                next += 1.0;
                at += 1;
            }
            let su = smooth((px - (next - 1.0)) as f32);
            let [t0, dt, b0, db] = cells.get(at).copied().unwrap_or([0.0; 4]);
            let (t, b) = (t0 + dt * su, b0 + db * su);
            *o = (t + (b - t) * sv) / spread;
        };
        {
            for (k, o) in out.iter_mut().enumerate() {
                let k = k as f64;
                blend(o, x + dx * k, smooth((v + dv * k - row) as f32));
            }
        }
        true
    }

    /// The noise along the rows of a band of `width × rows` output pixels at `(x0, y0)` of `space`
    /// for `channel`, made once for the band ([`Lines`]); `None` where the stored frame does not
    /// lie square in the output along the band's first row, or the band's cells are much smaller
    /// than its pixels (more than [`Lines::MOST_CELLS`] a pixel), or its buffers cannot be had.
    pub fn lines(&self, space: &Space, x0: u32, y0: u32, width: u32, rows: u32, channel: u32) -> Option<Lines> {
        // `row_values`'s steps: the row's start and the step over `STRIDE` pixels.
        const STRIDE: u32 = 1024;
        let (w, cell) = (width as usize, self.cell);
        let x1 = x0.saturating_add(STRIDE);
        let span = f64::from(x1 - x0).max(1.0);
        let (u0, _) = space.pixel_units(x0, y0);
        let (u1, _) = space.pixel_units(x1, y0);
        let (x, dx) = (u0 / cell, (u1 - u0) / span / cell);
        if !(x.is_finite() && dx.is_finite()) || dx.abs() > Lines::MOST_CELLS {
            return None;
        }
        // Each column's lattice column and smoothstep weight.
        let mut columns = zeros(w, (0i64, 0.0f32))?;
        for (k, c) in columns.iter_mut().enumerate() {
            let px = (x + dx * k as f64).clamp(-1e9, 1e9);
            let ci = floor(px);
            let fx = (px - ci as f64) as f32;
            *c = (ci, fx * fx * (3.0 - 2.0 * fx));
        }
        let lo = columns.iter().map(|c| c.0).min()?;
        let hi = columns.iter().map(|c| c.0).max()?;
        // Each row's lattice row, where the row starts and steps as the band's first row does and
        // the step does not move down the lattice.
        let mut lattice_rows = zeros(rows as usize, None)?;
        let (mut first, mut last) = (i64::MAX, i64::MIN);
        for (ly, r) in lattice_rows.iter_mut().enumerate() {
            let y = y0.saturating_add(u32::try_from(ly).unwrap_or(u32::MAX));
            let ((ua, va), (ub, vb)) = (space.pixel_units(x0, y), space.pixel_units(x1, y));
            let py = va / cell;
            if ua != u0 || ub != u1 || va != vb || !py.is_finite() {
                continue;
            }
            let py = py.clamp(-1e9, 1e9);
            let cj = floor(py);
            let fy = (py - cj as f64) as f32;
            *r = Some((cj, fy * fy * (3.0 - 2.0 * fy)));
            (first, last) = (first.min(cj), last.max(cj.saturating_add(1)));
        }
        let count = if first <= last { usize::try_from(last - first + 1).ok()? } else { 0 };
        if count as f64 > (lattice_rows.len() as f64 + 2.0) * Lines::MOST_CELLS {
            return None;
        }
        let mut lattice = zeros(usize::try_from(hi - lo + 2).ok()?, 0.0f32)?;
        let mut lines = zeros(count.checked_mul(w)?, 0.0f32)?;
        for (n, line) in lines.chunks_mut(w.max(1)).enumerate() {
            let j = first + n as i64;
            for (m, h) in lattice.iter_mut().enumerate() {
                *h = 2.0 * hash01((lo + m as i64) as i32, j as i32, channel, self.seed) - 1.0;
            }
            for (o, &(ci, su)) in line.iter_mut().zip(&columns) {
                let m = (ci - lo) as usize;
                let (c0, c1) = (lattice.get(m).copied().unwrap_or(0.0), lattice.get(m + 1).copied().unwrap_or(0.0));
                *o = c0 + (c1 - c0) * su;
            }
        }
        Some(Lines { width: w, first, lines, rows: lattice_rows, attenuation: self.attenuation(space) })
    }
}

/// The noise along a band's rows ([`Noise::lines`]): [`Noise::row`]'s values, bit for bit. Each
/// lattice row the band's rows lie between is hashed and blended across the band's columns once,
/// and a row is the blend of its two (as `Noise::row` blends the four corners of each cell).
pub struct Lines {
    width: usize,
    /// The lattice row of the first line.
    first: i64,
    /// Each lattice row the band's rows lie between, blended across the band's columns.
    lines: Vec<f32>,
    /// Per row of the band: its lattice row and the smoothstep weight of the next, or `None`.
    rows: Vec<Option<(i64, f32)>>,
    /// What `Noise::row` scales by where a pixel spans more than a cell.
    attenuation: f32,
}

impl Lines {
    /// The most lattice cells a pixel spans for the lines to be worth making.
    pub const MOST_CELLS: f64 = 4.0;

    /// The noise along the band's row `ly`, into `out`; false (and `out` as it was) for a row the
    /// lines do not serve (`Noise::row` does).
    pub fn row(&self, ly: usize, out: &mut [f32]) -> bool {
        let Some(Some((j, sv))) = self.rows.get(ly).copied() else { return false };
        let line = |j: i64| {
            let n = usize::try_from(j - self.first).unwrap_or(usize::MAX);
            n.checked_mul(self.width).and_then(|at| self.lines.get(at..at.checked_add(self.width)?)).unwrap_or(&[])
        };
        let spread = SPREAD as f32;
        for ((o, &top), &bottom) in out.iter_mut().zip(line(j)).zip(line(j + 1)) {
            *o = (top + (bottom - top) * sv) / spread;
        }
        if self.attenuation != 1.0 {
            for o in out.iter_mut() {
                *o *= self.attenuation;
            }
        }
        true
    }
}

/// The blend of a run's pixels on one row of the stored frame, `sv` across the lattice rows: each
/// pixel's cell `at` (its corner values in `cells`) and place across it `across`, four pixels at a
/// time so the compiler runs the lanes together (the same operations, value by value, as
/// [`Noise::row_values`]'s blend).
#[inline(never)]
fn blend_run(out: &mut [f32], cells: &[[f32; 4]; CORNERS], at: &[u32; RUN as usize], across: &[f32; RUN as usize], sv: f32) {
    let spread = SPREAD as f32;
    let smooth = |f: f32| f * f * (3.0 - 2.0 * f);
    let lane = |c: [f32; 4], f: f32| {
        let su = smooth(f);
        let (t, b) = (c[0] + c[1] * su, c[2] + c[3] * su);
        (t + (b - t) * sv) / spread
    };
    let corners = |i: u32| cells.get(i as usize).copied().unwrap_or([0.0; 4]);
    let (quads, rest) = out.as_chunks_mut::<4>();
    let n = quads.len() * 4;
    for (o, (a, f)) in quads.iter_mut().zip(at.as_chunks::<4>().0.iter().zip(across.as_chunks::<4>().0)) {
        let c = a.map(corners);
        *o = [lane(c[0], f[0]), lane(c[1], f[1]), lane(c[2], f[2]), lane(c[3], f[3])];
    }
    for (o, (&a, &f)) in rest.iter_mut().zip(at.iter().skip(n).zip(across.iter().skip(n))) {
        *o = lane(corners(a), f);
    }
}

/// `n` copies of `v`, reserved fallibly.
fn zeros<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}

/// `⌊v⌋` of a finite `v` inside ±10⁹, without a call to the library's `floor`.
fn floor(v: f64) -> i64 {
    let t = v as i64;
    if (t as f64) > v { t - 1 } else { t }
}

/// The seed of an entry: FNV-1a of its parameter text (`Operation`'s `Display`, or the effect's
/// name and parameters), so equal entries draw equal noise.
pub fn seed(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// A deterministic hash of a lattice point, a channel and a seed into `[0, 1)`.
pub fn hash01(x: i32, y: i32, c: u32, seed: u32) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ c.wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}
