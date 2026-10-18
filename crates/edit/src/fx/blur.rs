//! A Gaussian blur by three box passes along each axis, over a [`Plane`] of pixels placed in the
//! whole output.
//!
//! - **Size:** [`Gaussian::new`] takes the sigma in output pixels (an effect gets it from a
//!   relative size through [`super::Space::length`]); a sigma that is not a number or is at most
//!   0.3 px blurs nothing, and one wider than twice the output's longer side counts as that (a box
//!   wider than the picture blurs no flatter).
//! - **Edges:** each pass clamps at the plane's edges. A plane cut from the picture with a margin
//!   of [`Gaussian::reach`] pixels on every side that is not the picture's edge blurs, inside the
//!   margin, to exactly the values of the whole picture's blur: the running sums are kept in
//!   integers (fixed point, 2⁻²⁰), so they do not depend on where a row starts.
//! - **Passes:** [`Blurred`] blurs a pass's `before` view once per render (by the first band to
//!   run) for every band to read; the pass's [`crate::ops::Reach`] is
//!   `Reach::Pixels(gaussian.reach())`.
//!
//! Ported from LightCraft `crates/raster/src/blur.rs` (`box_radii`, `radii_for`, `gaussian`, the
//! running-sum box passes) at revision 0da998f2 (MIT OR Apache-2.0; `NOTICE`). Dalica's changes:
//! integer running sums instead of `f32` ones (a region equals the whole render exactly), the
//! vertical passes run on the transposed plane, priming a running sum costs the row's length at
//! most rather than the box's, and allocations are fallible.

use std::sync::OnceLock;

use rayon::prelude::*;

use crate::ops::View;

/// The largest sigma the box radii are made for (LightCraft's limit: wider boxes blur no flatter).
pub const MAX_SIGMA: f64 = 65_536.0;

/// Fixed-point units per working value during the box passes (2²⁰).
const ONE: f32 = 1_048_576.0;
/// The values a pass keeps, ±this (far outside any working value; it keeps them inside `i32`).
const LIMIT: f32 = 2_000.0;

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

    /// The window `view` holds, each pixel through `f`.
    pub fn from_view(view: &View<'_>, f: impl Fn([f32; 3]) -> [f32; 3] + Sync) -> Option<Plane> {
        Plane::from_fn(view.x0, view.y0, view.width, view.height, |x, y| f(view.get(i64::from(x), i64::from(y))))
    }

    /// The pixel at `(x, y)` of the whole output, clamped into the plane.
    pub fn get(&self, x: i64, y: i64) -> [f32; 3] {
        let lx = (x - i64::from(self.x0)).clamp(0, i64::from(self.width.max(1)) - 1);
        let ly = (y - i64::from(self.y0)).clamp(0, i64::from(self.height.max(1)) - 1);
        let i = usize::try_from(ly * i64::from(self.width) + lx).ok();
        i.and_then(|i| self.data.get(i).copied()).unwrap_or([0.0; 3])
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

    /// Blurs `plane` in place; false (and the plane unchanged) when a working buffer cannot be had.
    pub fn apply(&self, plane: &mut Plane) -> bool {
        if self.is_identity() {
            return true;
        }
        let (w, h) = (plane.width as usize, plane.height as usize);
        if w.checked_mul(h) != Some(plane.data.len()) || plane.data.is_empty() {
            return false;
        }
        let Some(mut fixed) = alloc(plane.data.len(), [0i32; 3]) else { return false };
        fixed.par_iter_mut().zip(plane.data.par_iter()).for_each(|(q, p)| *q = p.map(to_fixed));
        rows(&mut fixed, w, self.radii);
        let Some(mut turned) = transpose(&fixed, w, h) else { return false };
        rows(&mut turned, h, self.radii);
        let Some(back) = transpose(&turned, h, w) else { return false };
        plane.data.par_iter_mut().zip(back.par_iter()).for_each(|(p, q)| *p = q.map(|v| v as f32 / ONE));
        true
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
                let mut plane = Plane::from_view(view, f)?;
                self.gaussian.apply(&mut plane).then_some(plane)
            })
            .as_ref()
    }
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

fn to_fixed(v: f32) -> i32 {
    // NaN → 0; `as` saturates, and the clamp keeps every sum far inside `i64`.
    let v = if v.is_nan() { 0.0 } else { v.clamp(-LIMIT, LIMIT) * ONE };
    (if v >= 0.0 { v + 0.5 } else { v - 0.5 }) as i32
}

/// The three box passes along every row of a `width`-wide plane, in parallel.
fn rows(data: &mut [[i32; 3]], width: usize, radii: [usize; 3]) {
    if width == 0 {
        return;
    }
    data.par_chunks_mut(width).for_each(|row| {
        let mut tmp = Vec::with_capacity(row.len());
        for r in radii {
            box_line(row, &mut tmp, r);
        }
    });
}

/// One box pass of radius `r` along `line` (edges clamped), `tmp` as scratch: exact running sums.
fn box_line(line: &mut [[i32; 3]], tmp: &mut Vec<[i32; 3]>, r: usize) {
    let n = line.len();
    if r == 0 || n == 0 {
        return;
    }
    tmp.clear();
    tmp.extend_from_slice(line);
    let last = n - 1;
    let at = |i: usize| tmp.get(i.min(last)).copied().unwrap_or([0; 3]);
    let count = i64::try_from(2 * r + 1).unwrap_or(i64::MAX);
    // The sum over [−r, r] with the edges clamped: (r + 1) times the first pixel, the pixels 1..=r
    // inside the line, and the last pixel for each place past it.
    let first = at(0);
    let mut acc: [i64; 3] = std::array::from_fn(|c| i64::from(first[c]) * (r as i64 + 1));
    for i in 1..=r.min(last) {
        let v = at(i);
        for c in 0..3 {
            acc[c] += i64::from(v[c]);
        }
    }
    if r > last {
        let v = at(last);
        for c in 0..3 {
            acc[c] += i64::from(v[c]) * (r - last) as i64;
        }
    }
    for (x, out) in line.iter_mut().enumerate() {
        *out = std::array::from_fn(|c| divide(acc[c], count));
        let (add, sub) = (at(x.saturating_add(r + 1)), at(x.saturating_sub(r)));
        for c in 0..3 {
            acc[c] += i64::from(add[c]) - i64::from(sub[c]);
        }
    }
}

/// `sum / count` rounded half up.
fn divide(sum: i64, count: i64) -> i32 {
    let q = (sum.saturating_mul(2).saturating_add(count)).div_euclid(count.saturating_mul(2).max(1));
    q.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The `width × height` plane `data` turned about its diagonal (`height × width`).
fn transpose(data: &[[i32; 3]], width: usize, height: usize) -> Option<Vec<[i32; 3]>> {
    let mut out = alloc(data.len(), [0i32; 3])?;
    if height == 0 {
        return Some(out);
    }
    out.par_chunks_mut(height).enumerate().for_each(|(x, row)| {
        for (y, px) in row.iter_mut().enumerate() {
            if let Some(v) = data.get(y * width + x) {
                *px = *v;
            }
        }
    });
    Some(out)
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
        let mut want = Plane::from_view(&view, |p| p).unwrap();
        assert!(g.apply(&mut want));
        assert_eq!(got, want);
        assert_eq!(blurred.get(&view, |_| [0.0; 3]), Some(&want));
    }
}
