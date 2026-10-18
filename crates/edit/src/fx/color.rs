//! Luma and linear light for the effects, on the renderer's sRGB-encoded working values.
//!
//! - [`luma`]: Rec.601 weights (0.299, 0.587, 0.114) on the encoded values, as B&W uses them
//!   (`editing.md` §8.2).
//! - [`to_linear`] / [`to_srgb`]: the sRGB transfer curve (IEC 61966-2-1) and its inverse, for an
//!   effect that works in linear light; values outside 0..1 keep the curve's sign and slope
//!   (mirrored), `NaN` gives 0.
//!
//! Our own code from the public formulas.

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
