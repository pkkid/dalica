//! A Gaussian blur by three box passes along each axis, over a [`Plane`] (RGB) or a [`Mono`] (one
//! channel) of pixels placed in the whole output.
//!
//! - **Size:** [`Gaussian::new`] takes the sigma in output pixels (an effect gets it from a
//!   relative size through [`super::Space::length`]); a sigma that is not a number or is at most
//!   0.3 px blurs nothing, and one wider than twice the output's longer side counts as that (a box
//!   wider than the picture blurs no flatter).
//! - **Edges:** each pass clamps at the plane's edges. A plane cut from the picture with a margin
//!   of [`Gaussian::reach`] pixels on every side that is not the picture's edge blurs, inside the
//!   margin, to exactly the values of the whole picture's blur: the running sums are kept in
//!   integers (fixed point, 2⁻²⁰), so they do not depend on where a row starts.
//! - **Speed** (M13.34): the passes run on strips of [`STRIP`] rows, then of [`STRIP`] columns,
//!   each laid out so that a pass steps along the strip with one running sum per row (or column)
//!   and channel side by side: the inner loop runs over flat slices and vectorises, the clamped
//!   edges are handled by clamping the step a pass reads rather than each tap, and a strip stays
//!   in the cache for its three passes (no turned copies of the whole plane). The sums stay exact
//!   integers (held in `f64`, exact below 2⁵³), each divided by a multiply that rounds exactly as
//!   the division did ([`Divide`]), so the planes are bit for bit those of M12's passes.
//! - **Passes:** [`Blurred`] blurs a pass's `before` view once per render (by the first band to
//!   run) for every band to read; the pass's [`crate::ops::Reach`] is
//!   `Reach::Pixels(gaussian.reach())`. The bands run in the renderer's `rayon` pool, so the blur
//!   is made through [`apart`]: on a pool of its own ([`blur_pool`]) from a plain thread the band
//!   waits on, since a band thread that waited inside the pool could steal another band of the
//!   same pass and ask for the blur it is making (a deadlock). Every other pass whose one-time
//!   setup inside a band runs `rayon` work makes it through [`apart`] too (Soften's mask and blur,
//!   Retouch's healed patches, [`super::GridBlur`]'s grids).
//!
//! Ported from LightCraft `crates/raster/src/blur.rs` (`box_radii`, `radii_for`, `gaussian`, the
//! running-sum box passes) at revision 0da998f2 (MIT OR Apache-2.0; `NOTICE`). Dalica's changes:
//! integer running sums instead of `f32` ones (a region equals the whole render exactly), the
//! passes run in strips of rows and of columns (M13.34), priming a running sum costs the line's
//! length at most rather than the box's, and allocations are fallible.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;

use crate::ops::View;

/// The largest sigma the box radii are made for (LightCraft's limit: wider boxes blur no flatter).
pub const MAX_SIGMA: f64 = 65_536.0;

/// The rows (or columns) a pass runs over at once: one running sum per row (or column) and
/// channel, the strip small enough to stay in the cache.
pub const STRIP: usize = 8;

/// Fixed-point units per working value during the box passes (2²⁰).
const ONE: f32 = 1_048_576.0;
/// The values a pass keeps, ±this (far outside any working value; it keeps them inside `i32`).
const LIMIT: f32 = 2_000.0;
/// The most pixels one box covers (`2r + 1`). [`box_radii`] of [`MAX_SIGMA`] makes about 2¹⁷, so
/// every box of a [`Gaussian`] is inside it; with every value inside `i32` a box's sum stays
/// inside ±2⁴⁹, where [`Divide`] is exact.
const MAX_COUNT: usize = 1 << 18;

/// RGB values in working precision over a rectangle of the whole output.
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    /// The top-left pixel in the whole output.
    pub x0: u32,
    pub y0: u32,
    pub width: u32,
    pub height: u32,
    /// `width × height` pixels, row-major.
    pub data: Vec<[f32; 3]>,
}

impl Plane {
    /// The rectangle at `(x0, y0)` of `width × height` pixels, each `f(x, y)` (whole-output
    /// coordinates), made in parallel; `None` when it is empty or its buffer cannot be had.
    pub fn from_fn(x0: u32, y0: u32, width: u32, height: u32, f: impl Fn(u32, u32) -> [f32; 3] + Sync) -> Option<Plane> {
        let n = (width as usize).checked_mul(height as usize).filter(|n| *n > 0)?;
        let data = alloc(n, [0.0f32; 3])?;
        let mut plane = Plane { x0, y0, width, height, data };
        plane.data.par_chunks_mut(width as usize).enumerate().for_each(|(j, row)| {
            let y = y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
            for (i, px) in row.iter_mut().enumerate() {
                *px = f(x0.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)), y);
            }
        });
        Some(plane)
    }

    /// The window `view` holds, each pixel through `f` (read row by row from the view's bytes:
    /// the same values as [`View::get`]).
    pub fn from_view(view: &View<'_>, f: impl Fn([f32; 3]) -> [f32; 3] + Sync) -> Option<Plane> {
        let (w, h) = (view.width as usize, view.height as usize);
        let n = w.checked_mul(h).filter(|n| *n > 0)?;
        let mut data = alloc(n, [0.0f32; 3])?;
        data.par_chunks_mut(w).enumerate().for_each(|(j, row)| {
            let bytes = j.checked_mul(w * 4).and_then(|at| view.data.get(at..at.checked_add(w * 4)?));
            match bytes {
                Some(bytes) => {
                    for (px, b) in row.iter_mut().zip(bytes.as_chunks::<4>().0) {
                        *px = f([f32::from(b[0]) / 255.0, f32::from(b[1]) / 255.0, f32::from(b[2]) / 255.0]);
                    }
                }
                // A view shorter than its window reads as black, as `View::get` does.
                None => row.fill(f([0.0; 3])),
            }
        });
        Some(Plane { x0: view.x0, y0: view.y0, width: view.width, height: view.height, data })
    }

    /// The pixel at `(x, y)` of the whole output, clamped into the plane.
    pub fn get(&self, x: i64, y: i64) -> [f32; 3] {
        let lx = (x - i64::from(self.x0)).clamp(0, i64::from(self.width.max(1)) - 1);
        let ly = (y - i64::from(self.y0)).clamp(0, i64::from(self.height.max(1)) - 1);
        let i = usize::try_from(ly * i64::from(self.width) + lx).ok();
        i.and_then(|i| self.data.get(i).copied()).unwrap_or([0.0; 3])
    }
}

/// One channel in working precision over a rectangle of the whole output (a luma, a mask).
#[derive(Clone, Debug, PartialEq)]
pub struct Mono {
    /// The top-left pixel in the whole output.
    pub x0: u32,
    pub y0: u32,
    pub width: u32,
    pub height: u32,
    /// `width × height` values, row-major.
    pub data: Vec<f32>,
}

impl Mono {
    /// The rectangle at `(x0, y0)` of `width × height` values, each `f(x, y)` (whole-output
    /// coordinates), made in parallel; `None` when it is empty or its buffer cannot be had.
    pub fn from_fn(x0: u32, y0: u32, width: u32, height: u32, f: impl Fn(u32, u32) -> f32 + Sync) -> Option<Mono> {
        let n = (width as usize).checked_mul(height as usize).filter(|n| *n > 0)?;
        let data = alloc(n, 0.0f32)?;
        let mut mono = Mono { x0, y0, width, height, data };
        mono.data.par_chunks_mut(width as usize).enumerate().for_each(|(j, row)| {
            let y = y0.saturating_add(u32::try_from(j).unwrap_or(u32::MAX));
            for (i, v) in row.iter_mut().enumerate() {
                *v = f(x0.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)), y);
            }
        });
        Some(mono)
    }

    /// The value at `(x, y)` of the whole output, clamped into the rectangle.
    pub fn get(&self, x: i64, y: i64) -> f32 {
        let lx = (x - i64::from(self.x0)).clamp(0, i64::from(self.width.max(1)) - 1);
        let ly = (y - i64::from(self.y0)).clamp(0, i64::from(self.height.max(1)) - 1);
        let i = usize::try_from(ly * i64::from(self.width) + lx).ok();
        i.and_then(|i| self.data.get(i).copied()).unwrap_or(0.0)
    }
}

/// A Gaussian blur made of three box passes (Kovesi / Wells).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gaussian {
    radii: [usize; 3],
}

impl Gaussian {
    /// A blur of `sigma` output pixels in a `full` (width, height) output.
    pub fn new(sigma: f64, full: (u32, u32)) -> Gaussian {
        let cap = (f64::from(full.0.max(full.1)) * 2.0).clamp(1.0, MAX_SIGMA);
        if sigma.is_nan() || sigma <= 0.3 {
            return Gaussian { radii: [0; 3] };
        }
        Gaussian { radii: box_radii(sigma.min(cap)) }
    }

    /// The radius of each box pass.
    pub fn radii(&self) -> [usize; 3] {
        self.radii
    }

    /// How far a blurred pixel reads, in output pixels: the boxes' radii added up (about 3σ).
    pub fn reach(&self) -> u32 {
        u32::try_from(self.radii.iter().sum::<usize>()).unwrap_or(u32::MAX)
    }

    /// The standard deviation the boxes make (whole-pixel boxes snap small sigmas to a few steps).
    pub fn sigma(&self) -> f64 {
        self.radii.iter().map(|&r| (r as f64) * (r as f64 + 1.0) / 3.0).sum::<f64>().sqrt()
    }

    /// Whether the blur leaves every picture as it is.
    pub fn is_identity(&self) -> bool {
        self.radii == [0; 3]
    }

    /// Blurs `plane` in place; false when a working buffer cannot be had (the plane unchanged, or
    /// black when memory ran out midway).
    pub fn apply(&self, plane: &mut Plane) -> bool {
        self.is_identity() || blur_flat::<3>(self.radii, plane.data.as_flattened_mut(), plane.width as usize, plane.height as usize)
    }

    /// Blurs `mono` in place, as [`Gaussian::apply`] blurs each channel of a plane; false when a
    /// working buffer cannot be had.
    pub fn apply_mono(&self, mono: &mut Mono) -> bool {
        self.is_identity() || blur_flat::<1>(self.radii, &mut mono.data, mono.width as usize, mono.height as usize)
    }
}

/// A pass's blurred `before` view, made once per render by the first band that asks for it.
#[derive(Debug)]
pub struct Blurred {
    gaussian: Gaussian,
    done: OnceLock<Option<Plane>>,
}

impl Blurred {
    pub fn new(gaussian: Gaussian) -> Blurred {
        Blurred { gaussian, done: OnceLock::new() }
    }

    pub fn gaussian(&self) -> Gaussian {
        self.gaussian
    }

    /// The window `view` holds, each pixel through `f`, blurred; every later call answers the
    /// first one's plane (a pass makes one per render). `None` when its buffers cannot be had.
    pub fn get(&self, view: &View<'_>, f: impl Fn([f32; 3]) -> [f32; 3] + Sync) -> Option<&Plane> {
        self.done
            .get_or_init(|| {
                apart(|| {
                    let mut plane = Plane::from_view(view, &f)?;
                    self.gaussian.apply(&mut plane).then_some(plane)
                })
            })
            .as_ref()
    }
}

/// Runs `make`, a pass's one-time setup reached from inside a band (in a `OnceLock`), on
/// [`blur_pool`] from a plain thread the caller only waits on, and answers what it made: `None`
/// when `make` does, or when the pool or the thread cannot be started.
///
/// A band thread that ran `rayon` work itself while making the setup would, waiting on that work,
/// steal another band of the same pass, which then waits on the lock this thread holds (or on
/// another render's setup whose maker waits on this one): the render hangs. Made apart, the setup
/// still runs in parallel; the bands that ask meanwhile wait for it, and the caller's thread
/// blocks outside every pool, so it runs nothing else.
pub fn apart<T: Send>(make: impl FnOnce() -> Option<T> + Send) -> Option<T> {
    let pool = blur_pool()?;
    std::thread::scope(|s| {
        let waiter = std::thread::Builder::new().name("dalica-setup-wait".into()).spawn_scoped(s, || pool.install(make)).ok()?;
        waiter.join().ok().flatten()
    })
}

/// The pool [`apart`] runs one-time setups on ([`Blurred`]'s blurs among them), apart from the
/// renderer's (as many threads as the machine's cores); `None` when it cannot be started.
pub fn blur_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| rayon::ThreadPoolBuilder::new().thread_name(|i| format!("dalica-blur-{i}")).build().ok()).as_ref()
}

/// Box radii approximating a Gaussian of `sigma` with three passes (Kovesi / Wells).
pub fn box_radii(sigma: f64) -> [usize; 3] {
    let sigma = if sigma.is_nan() { 0.0 } else { sigma.clamp(0.0, MAX_SIGMA) };
    let n = 3.0f64;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let wlf = wl as f64;
    let m_ideal = (12.0 * sigma * sigma - n * wlf * wlf - 4.0 * n * wlf - 3.0 * n) / (-4.0 * wlf - 4.0);
    let m = m_ideal.round() as i64;
    std::array::from_fn(|i| usize::try_from(((if (i as i64) < m { wl } else { wu }) - 1) / 2).unwrap_or(0))
}

/// A working value in fixed point (2⁻²⁰; `NaN` as 0, brought into ±[`LIMIT`], rounded half away
/// from zero in `f32`), as an integer held in `f64`. The same integer as M12's
/// `(v ± 0.5) as i32`; the truncation is made by [`ROUND`] rather than a saturating cast, so a
/// row of them vectorises.
fn to_fixed(v: f32) -> f64 {
    let v = if v.is_nan() { 0.0 } else { v.clamp(-LIMIT, LIMIT) * ONE };
    let t = f64::from(if v >= 0.0 { v + 0.5 } else { v - 0.5 });
    // `t` toward zero: the nearest integer, one closer to zero where that is farther out.
    let near = (t + ROUND) - ROUND;
    if t >= 0.0 {
        if near > t { near - 1.0 } else { near }
    } else if near < t {
        near + 1.0
    } else {
        near
    }
}

/// An integer inside `i32` held in `f64`, as the bits of an `f32` (its two's complement: the low
/// half of the bits of `v + ROUND`).
fn to_bits(v: f64) -> f32 {
    f32::from_bits((v + ROUND).to_bits() as u32)
}

/// The integer [`to_bits`] keeps in `v`.
fn from_bits(v: f32) -> f64 {
    f64::from(v.to_bits() as i32)
}

/// The three box passes of `radii` over `data`, `width × height` pixels of `C` values each
/// (row-major), in place: every row, then every column. False when a working buffer cannot be
/// had (`data` unchanged, or cleared when memory ran out midway) or the sizes do not match.
///
/// Both run on strips ([`STRIP`] rows, then [`STRIP`] columns) laid out so that a pass steps along
/// the strip with one running sum per lane (a lane: one channel of one row or column of the
/// strip), the lanes side by side: the inner loop runs over flat slices of `f64` lanes and
/// vectorises. Between the rows and the columns `data` holds the fixed-point values themselves
/// (as bits, [`to_bits`]; `from_bits` and `to_bits` keep every bit), so no buffer the size of the
/// plane is needed.
fn blur_flat<const C: usize>(radii: [usize; 3], data: &mut [f32], width: usize, height: usize) -> bool {
    let line = width.saturating_mul(C);
    if line == 0
        || height == 0
        || line.checked_mul(height) != Some(data.len())
        || radii.iter().any(|&r| r.saturating_mul(2).saturating_add(1) > MAX_COUNT)
    {
        return false;
    }
    // The column strips' pieces of every row, listed before anything changes.
    let strip = STRIP * C;
    let columns = line.div_ceil(strip);
    let mut pieces: Vec<Vec<&mut [f32]>> = Vec::new();
    if pieces.try_reserve_exact(columns).is_err() {
        return false;
    }
    for _ in 0..columns {
        let mut rows = Vec::new();
        if rows.try_reserve_exact(height).is_err() {
            return false;
        }
        pieces.push(rows);
    }
    let failed = AtomicBool::new(false);
    // Rows: a strip of rows to fixed point, laid out a step per pixel and a lane per row and
    // channel, its three passes, and back into the rows (as bits).
    data.par_chunks_mut(line * STRIP).for_each_init(Scratch::default, |scratch, rows| {
        let lanes = rows.len() / width;
        let Some((a, b, acc, tmp)) = scratch.get(width * lanes, lanes, line) else {
            failed.store(true, Ordering::Relaxed);
            return;
        };
        for (j, row) in rows.chunks_exact(line).enumerate() {
            for (q, &v) in tmp.iter_mut().zip(row) {
                *q = to_fixed(v);
            }
            let at = j * C..(j + 1) * C;
            for (step, px) in a.chunks_exact_mut(lanes).zip(tmp.as_chunks::<C>().0) {
                if let Some(lane) = step.get_mut(at.clone()) {
                    lane.copy_from_slice(px);
                }
            }
        }
        let done = passes(a, b, lanes, radii, acc);
        for (j, row) in rows.chunks_exact_mut(line).enumerate() {
            let at = j * C..(j + 1) * C;
            for (px, step) in tmp.as_chunks_mut::<C>().0.iter_mut().zip(done.chunks_exact(lanes)) {
                if let Some(lane) = step.get(at.clone()) {
                    px.copy_from_slice(lane);
                }
            }
            for (v, &q) in row.iter_mut().zip(tmp.iter()) {
                *v = to_bits(q);
            }
        }
    });
    if failed.load(Ordering::Relaxed) {
        // Only when memory ran out: the rows done hold bits, so the plane is cleared.
        data.fill(0.0);
        return false;
    }
    // Columns: a strip of columns (a step per row, a lane per column and channel), its three
    // passes, and back as working values.
    for row in data.chunks_exact_mut(line) {
        for (list, piece) in pieces.iter_mut().zip(row.chunks_mut(strip)) {
            list.push(piece);
        }
    }
    pieces.into_par_iter().for_each_init(Scratch::default, |scratch, mut rows| {
        let lanes = rows.first().map_or(0, |r| r.len());
        let Some((a, b, acc, _)) = scratch.get(height * lanes, lanes, 0) else {
            failed.store(true, Ordering::Relaxed);
            return;
        };
        for (step, piece) in a.chunks_exact_mut(lanes).zip(&rows) {
            for (q, &v) in step.iter_mut().zip(piece.iter()) {
                *q = from_bits(v);
            }
        }
        let done = passes(a, b, lanes, radii, acc);
        for (piece, step) in rows.iter_mut().zip(done.chunks_exact(lanes)) {
            for (v, &q) in piece.iter_mut().zip(step) {
                // `as f32` rounds the integer as M12's `i32 as f32` did.
                *v = q as f32 / ONE;
            }
        }
    });
    if failed.load(Ordering::Relaxed) {
        data.fill(0.0);
        return false;
    }
    true
}

/// A strip's working buffers, kept between the strips one thread runs (their contents are always
/// written before they are read).
#[derive(Default)]
struct Scratch {
    a: Vec<f64>,
    b: Vec<f64>,
    acc: Vec<f64>,
    tmp: Vec<f64>,
}

impl Scratch {
    /// Two buffers of `n` values, `lanes` running sums and a row of `row` values; `None` when they
    /// cannot be had.
    #[allow(clippy::type_complexity)]
    fn get(&mut self, n: usize, lanes: usize, row: usize) -> Option<(&mut [f64], &mut [f64], &mut [f64], &mut [f64])> {
        for (v, n) in [(&mut self.a, n), (&mut self.b, n), (&mut self.acc, lanes), (&mut self.tmp, row)] {
            if v.len() != n {
                v.clear();
                v.try_reserve_exact(n).ok()?;
                v.resize(n, 0.0);
            }
        }
        Some((&mut self.a, &mut self.b, &mut self.acc, &mut self.tmp))
    }
}

/// The three box passes of `radii` along a strip `a` of `lanes` lanes, `b` as the other buffer;
/// answers the one holding the result.
fn passes<'a>(mut a: &'a mut [f64], mut b: &'a mut [f64], lanes: usize, radii: [usize; 3], acc: &mut [f64]) -> &'a [f64] {
    for r in radii {
        if r > 0 {
            box_pass(a, b, lanes, r, acc);
            std::mem::swap(&mut a, &mut b);
        }
    }
    a
}

/// One box pass of radius `r` along `src`, a line of `src.len() / lanes` steps of `lanes` values
/// each, into `dst`, the edges clamped: per lane an exact running sum over the box, rounded half
/// up ([`Divide`]). `acc` holds one running sum per lane.
///
/// The values are integers inside `i32` and the sums integers kept in `f64`: a box covers at most
/// [`MAX_COUNT`] values, so a sum stays inside ±2⁴⁹ and every add is exact (an `f64` holds every
/// integer up to 2⁵³); lanes of `f64` vectorise where `i64` ones would not.
fn box_pass(src: &[f64], dst: &mut [f64], lanes: usize, r: usize, acc: &mut [f64]) {
    let n = src.len().checked_div(lanes).unwrap_or(0);
    if n == 0 || acc.len() != lanes || dst.len() != src.len() {
        return;
    }
    let last = n - 1;
    let step = |i: usize| src.get(i * lanes..(i + 1) * lanes).unwrap_or(&[]);
    let div = Divide::new(r);
    // The sum over [−r, r] with the edges clamped: (r + 1) times the first step, the steps 1..=r
    // inside the line, and the last step once for each place past it.
    for (a, &v) in acc.iter_mut().zip(step(0)) {
        *a = v * (r as f64 + 1.0);
    }
    for i in 1..=r.min(last) {
        for (a, &v) in acc.iter_mut().zip(step(i)) {
            *a += v;
        }
    }
    if r > last {
        for (a, &v) in acc.iter_mut().zip(step(last)) {
            *a += v * (r - last) as f64;
        }
    }
    for (x, out) in dst.chunks_exact_mut(lanes).enumerate() {
        let (add, sub) = (step((x + r + 1).min(last)), step(x.saturating_sub(r)));
        for (((o, a), &p), &m) in out.iter_mut().zip(acc.iter_mut()).zip(add).zip(sub) {
            *o = div.round(*a);
            *a += p - m;
        }
    }
}

/// 1.5 · 2⁵²: added to and taken from a number below 2⁵¹ in size, it leaves the nearest integer.
const ROUND: f64 = 6_755_399_441_055_744.0;

/// A box's sum divided by its `count` values and rounded half up, `⌊(2·sum + count) / d⌋` with
/// `d = 2·count`, exactly, by a multiply, for an integer sum of values inside `i32` and a count
/// up to [`MAX_COUNT`].
///
/// The quotient `t` is below 2³¹ + ½ in size and, when it is not an integer, at least `1 / d` from
/// one. Its product by the rounded reciprocal is within `2³¹ · 2⁻⁵² = 2⁻²¹` of it (the numerator is
/// exact below 2⁵³), less than half of that `1 / d` (`d ≤ 2¹⁹`): so the nearest integer to the
/// product (by [`ROUND`]) is the floor, unless the product lies more than `1 / (2d)` below it,
/// where the floor is one less.
#[derive(Clone, Copy, Debug)]
struct Divide {
    count: f64,
    reciprocal: f64,
    margin: f64,
}

impl Divide {
    fn new(r: usize) -> Divide {
        let count = (2 * r.min(MAX_COUNT) + 1) as f64;
        Divide { count, reciprocal: 1.0 / (2.0 * count), margin: 0.25 / count }
    }

    fn round(&self, sum: f64) -> f64 {
        let t = (2.0 * sum + self.count) * self.reciprocal;
        let near = (t + ROUND) - ROUND;
        if t < near - self.margin { near - 1.0 } else { near }
    }
}

fn alloc<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The radii keep LightCraft's formula: three boxes of radius r have the variance Σ r(r + 1)/3.
    #[test]
    fn box_radii_approximate_the_gaussian() {
        for sigma in [1.0, 2.5, 7.0, 30.0, 400.0] {
            let g = Gaussian::new(sigma, (100_000, 100_000));
            assert!((g.sigma() - sigma).abs() < 0.6, "{sigma}: {}", g.sigma());
        }
        // Every box of the widest blur fits the divider's bound.
        assert!(box_radii(MAX_SIGMA).iter().all(|&r| 2 * r < MAX_COUNT));
    }

    /// The division rounds as `div_euclid` does, at the ends of the sums a box can make and around
    /// every multiple of the count.
    #[test]
    fn the_division_rounds_half_up_exactly() {
        let exact = |sum: i64, count: i64| (2 * sum + count).div_euclid(2 * count);
        for r in [1usize, 2, 3, 7, 100, 4095, 65_536, (MAX_COUNT - 1) / 2] {
            let count = (2 * r + 1) as i64;
            // Sums of values inside `i32`.
            let end = count * i64::from(i32::MAX);
            let mut sums = vec![-end, -end + 1, end - count, end - 1, 0, 1, -1];
            for k in [-3i64, -1, 0, 1, 2, 1 << 20, -(1 << 30), (1 << 31) - 2] {
                let base = k * count;
                sums.extend([base - count / 2 - 1, base - count / 2, base + count / 2, base + count / 2 + 1, base]);
            }
            let mut seed = 0x2545_f491_4f6c_dd1du64 ^ r as u64;
            for _ in 0..2000 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                sums.push((seed % (2 * end as u64)) as i64 - end);
            }
            for s in sums.into_iter().filter(|s| s.abs() <= end) {
                assert_eq!(Divide::new(r).round(s as f64), exact(s, count) as f64, "{s} / {count}");
            }
        }
    }

    /// [`Blurred`] on a view equals the blur of the same pixels as a plane, and later calls reuse it.
    #[test]
    fn a_blurred_view_is_the_plane_blurred_once() {
        let (w, h) = (23u32, 17u32);
        let data: Vec<u8> = (0..w * h).flat_map(|i| [(i * 7 % 251) as u8, (i * 13 % 256) as u8, (i % 9 * 20) as u8, 255]).collect();
        let view = View { data: &data, x0: 0, y0: 0, width: w, height: h, full_width: w, full_height: h };
        let g = Gaussian::new(2.0, (w, h));
        let blurred = Blurred::new(g);
        let got = blurred.get(&view, |p| p).unwrap().clone();
        let mut want = Plane::from_fn(0, 0, w, h, |x, y| view.get(i64::from(x), i64::from(y))).unwrap();
        assert_eq!(Plane::from_view(&view, |p| p).as_ref(), Some(&want));
        assert!(g.apply(&mut want));
        assert_eq!(got, want);
        assert_eq!(blurred.get(&view, |_| [0.0; 3]), Some(&want));
    }
}
