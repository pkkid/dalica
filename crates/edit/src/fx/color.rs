//! Luma and linear light for the effects, on the renderer's sRGB-encoded working values.
//!
//! - [`luma`]: Rec.601 weights (0.299, 0.587, 0.114) on the encoded values, as B&W uses them
//!   (`editing.md` §8.2).
//! - [`to_linear`] / [`to_srgb`]: the sRGB transfer curve (IEC 61966-2-1) and its inverse, for an
//!   effect that works in linear light; values outside 0..1 keep the curve's sign and slope
//!   (mirrored), `NaN` gives 0.
//! - [`linear_light`] (M13.34): the same two curves as tables of [`STEPS`] steps over 0..1, built
//!   once and read with linear interpolation (within 0.01 of a level of the curves), for the
//!   effects that convert every pixel: no `powf` per channel. A table reads its input brought
//!   into 0..1 (`NaN` as 0).
//! - [`UNIT8`] (M13.47): `v / 255` for every 8-bit value, looked up (the same values the division
//!   gives), for the renderer's bands and the grids made from 8-bit pixels.
//!
//! Our own code from the public formulas.

use std::sync::OnceLock;

/// The steps of the [`linear_light`] tables over 0..1.
pub const STEPS: usize = 4096;

/// `v / 255` for every 8-bit value `v`, as `f32`: the values the division gives, looked up.
pub const UNIT8: [f32; 256] = {
    let mut t = [0.0f32; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = i as f32 / 255.0;
        i += 1;
    }
    t
};

/// The Rec.601 luma of an encoded RGB value.
pub fn luma(p: [f32; 3]) -> f32 {
    0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]
}

/// One sRGB-encoded channel in linear light.
pub fn to_linear(v: f32) -> f32 {
    if v.is_nan() {
        return 0.0;
    }
    let a = v.abs();
    let l = if a <= 0.040_45 { a / 12.92 } else { ((a + 0.055) / 1.055).powf(2.4) };
    l.copysign(v)
}

/// One linear-light channel sRGB-encoded.
pub fn to_srgb(v: f32) -> f32 {
    if v.is_nan() {
        return 0.0;
    }
    let a = v.abs();
    let e = if a <= 0.003_130_8 { a * 12.92 } else { 1.055 * a.powf(1.0 / 2.4) - 0.055 };
    e.copysign(v)
}

/// A function of 0..1 sampled at `STEPS + 1` points and read with linear interpolation, so a
/// monotonic function stays monotonic.
#[derive(Debug)]
pub struct Table {
    samples: Vec<f32>,
}

impl Table {
    /// `f` sampled over 0..1.
    pub fn new(f: impl Fn(f32) -> f32) -> Table {
        Table { samples: (0..=STEPS).map(|i| f(i as f32 / STEPS as f32)).collect() }
    }

    /// The function at `x` (brought into 0..1, `NaN` as 0).
    pub fn at(&self, x: f32) -> f32 {
        let pos = if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) } * STEPS as f32;
        let i = (pos as usize).min(STEPS);
        let t = pos - i as f32;
        match (self.samples.get(i), self.samples.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * t,
            (Some(a), None) => *a,
            _ => 0.0,
        }
    }
}

/// The sRGB transfer curve and its inverse as tables ([`linear_light`]).
#[derive(Debug)]
pub struct LinearLight {
    decode: Table,
    encode: Table,
}

impl LinearLight {
    /// An encoded channel (0..1) in linear light, [`to_linear`] by the table.
    pub fn to_linear(&self, v: f32) -> f32 {
        self.decode.at(v)
    }

    /// A linear-light channel (0..1) encoded, [`to_srgb`] by the table.
    pub fn to_srgb(&self, v: f32) -> f32 {
        self.encode.at(v)
    }

    /// An encoded pixel in linear light.
    pub fn to_linear3(&self, p: [f32; 3]) -> [f32; 3] {
        p.map(|v| self.decode.at(v))
    }

    /// A linear-light pixel encoded.
    pub fn to_srgb3(&self, p: [f32; 3]) -> [f32; 3] {
        p.map(|v| self.encode.at(v))
    }
}

/// The shared linear-light tables, built by the first effect that asks.
pub fn linear_light() -> &'static LinearLight {
    static TABLES: OnceLock<LinearLight> = OnceLock::new();
    TABLES.get_or_init(|| LinearLight { decode: Table::new(to_linear), encode: Table::new(to_srgb) })
}
