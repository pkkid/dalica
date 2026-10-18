//! Blend modes and Fade on the renderer's working values (sRGB-encoded, 0..1), per channel.
//!
//! - [`Blend`]: `base` is the picture below, `top` the layer blended onto it; the formulas are the
//!   W3C *Compositing and Blending* ones (normal, multiply, screen, overlay, soft light). Values
//!   are brought into 0..1 first (`NaN` counts as 0), so the result is in 0..1 too.
//! - [`fade`]: `editing.md` §8.1: Fade 0..100 blends an effect's result back toward its input,
//!   `alpha = 1 − Fade/100` (Fade 0 gives the effect, 100 the input).
//!
//! Our own code from the public formulas.

/// How a layer is blended onto the picture below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Blend {
    /// The layer itself.
    #[default]
    Normal,
    /// `base · top`: darkens.
    Multiply,
    /// `1 − (1 − base)(1 − top)`: lightens.
    Screen,
    /// Multiply in the shadows of the base, screen in its highlights (doubled).
    Overlay,
    /// A gentle overlay: darkens or lightens the base by the layer, never to black or white.
    SoftLight,
}

impl Blend {
    pub const ALL: [Blend; 5] = [Blend::Normal, Blend::Multiply, Blend::Screen, Blend::Overlay, Blend::SoftLight];

    /// One channel.
    pub fn channel(self, base: f32, top: f32) -> f32 {
        let (b, t) = (unit(base), unit(top));
        match self {
            Blend::Normal => t,
            Blend::Multiply => b * t,
            Blend::Screen => screen(b, t),
            Blend::Overlay => {
                if b <= 0.5 {
                    2.0 * b * t
                } else {
                    screen(2.0 * b - 1.0, t)
                }
            }
            Blend::SoftLight => {
                if t <= 0.5 {
                    b - (1.0 - 2.0 * t) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
                    b + (2.0 * t - 1.0) * (d - b)
                }
            }
        }
    }

    /// Every channel.
    pub fn apply(self, base: [f32; 3], top: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| self.channel(base[c], top[c]))
    }
}

fn screen(b: f32, t: f32) -> f32 {
    1.0 - (1.0 - b) * (1.0 - t)
}

fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// `from` moved toward `to` by `t` (0 gives `from`, 1 gives `to`; `t` brought into 0..1, `NaN`
/// counts as 0).
pub fn mix(from: [f32; 3], to: [f32; 3], t: f32) -> [f32; 3] {
    let t = unit(t);
    std::array::from_fn(|c| from[c] + (to[c] - from[c]) * t)
}

/// An effect's result `effect` faded toward its `input` by `fade` (0..100, brought into range;
/// `NaN` counts as 0): `input + (effect − input) · (1 − fade/100)`.
pub fn fade(input: [f32; 3], effect: [f32; 3], fade: f64) -> [f32; 3] {
    let f = if fade.is_nan() { 0.0 } else { fade.clamp(0.0, 100.0) };
    mix(input, effect, (1.0 - f / 100.0) as f32)
}
