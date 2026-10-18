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

    /// The factor [`Noise::at`] scales by in `space`: 1 while a pixel is at most a cell, then
    /// `cell / pixel`.
    pub fn attenuation(&self, space: &Space) -> f32 {
        let pixel = space.pixel();
        if pixel.is_finite() && pixel > self.cell { (self.cell / pixel) as f32 } else { 1.0 }
    }
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
