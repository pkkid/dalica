//! What the effects of tabs 3–5 share (`editing.md` §8, M12.3), so no effect writes its own: each
//! works on the renderer's bands ([`crate::ops::Band`], [`crate::ops::View`], [`crate::ops::Reach`])
//! in the renderer's sRGB-encoded working values (0..1 `f32`).
//!
//! - [`space`]: [`Space`], the stored frame as it lies in the output of one render (from
//!   [`crate::ops::Prepare`]): stored-frame points to output pixels and back, lengths relative to
//!   the stored photo's shorter side in output pixels, and resolution-independent coordinates for
//!   noise and shapes.
//! - [`blur`]: a Gaussian by three box passes ([`Gaussian`]) over a [`Plane`] of pixels placed in
//!   the whole output, exactly the same in a region render (with the margin of its
//!   [`Gaussian::reach`]) as in the whole render; [`Blurred`] blurs a pass's `before` view once per
//!   render for every band; [`apart`] makes any such one-time setup that runs `rayon` work apart
//!   from the bands' pool, so a band never waits on a setup it is making.
//! - [`noise`]: [`Noise`], smooth value noise seeded from an entry's parameters ([`noise::seed`])
//!   and placed in the stored frame, so a thumbnail, the edit view, a zoomed region and a Save
//!   agree; mean 0 and a standard deviation of 1 before its amount.
//! - [`mask`]: per-pixel masks in output pixels from stored-frame shapes: a [`Circle`] with a
//!   feathered edge, a linear [`Gradient`] with its start, angle and feather, and the distance from
//!   the picture's [`Edges`].
//! - [`brush`]: dabs along painted and erased strokes ([`Dabs`], [`paint`]), for Soften's mask.
//! - [`blend`]: the blend modes ([`Blend`]: normal, screen, overlay, soft light, multiply), [`fade`]
//!   (`alpha = 1 − Fade/100` toward the input) and [`mix`].
//! - [`color`]: [`luma`] (Rec.601) and sRGB ↔ linear light ([`to_linear`], [`to_srgb`]).
//!
//! Every size is relative to the stored photo's shorter side ([`Space::length`]), every point a
//! normalised stored-frame [`crate::Point`]; nothing here panics or allocates without a bound,
//! whatever the numbers (`NaN`, infinite, zero, huge).

pub mod blend;
pub mod blur;
pub mod brush;
pub mod color;
pub mod mask;
pub mod noise;
pub mod space;

pub use blend::{Blend, fade, mix};
pub use blur::{Blurred, Gaussian, Plane, apart};
pub use brush::{Dabs, paint};
pub use color::{luma, to_linear, to_srgb};
pub use mask::{Circle, Edges, Gradient};
pub use noise::Noise;
pub use space::Space;

/// `x` smoothly from 0 at `e0` to 1 at `e1` (Hermite); a step at `e0` when the edges meet.
pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    if x.is_nan() {
        return 0.0;
    }
    let span = e1 - e0;
    if span.is_nan() || span.abs() <= 1e-12 {
        return if x > e0 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / span).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `v` when it is a finite number, else `or`.
pub(crate) fn finite_or(v: f64, or: f64) -> f64 {
    if v.is_finite() { v } else { or }
}
