//! The colour remaps of tabs 4 and 5 (`editing.md` §8.3–§8.4): `invert()`, `infrared(fade)`,
//! `duotone(first,second,brightness,contrast,fade)` and `boost(strength)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Invert Colors takes
//!   none (one click); Infrared Film's Fade 0–100 (0); Duo-Tone's first colour (`#004488`, the
//!   shadows), second colour (`#ffff00`, the highlights), Brightness −95…95 (0), Contrast 0–100
//!   (20) and Fade 0–100 (0); Boost's Strength 0–100 (50). Numbers out of range are clamped;
//!   `NaN`, a missing or extra field or a colour that is not `#rrggbb` is refused (the entry is
//!   kept and skipped).
//! - **Rendering** (M12.9): each is one pixel-only pass ([`super::Reach::Pixel`], fused with its
//!   neighbours) on the renderer's sRGB-encoded working values, `VERSION` 1:
//!   - **Invert Colors:** every channel `1 − v`; twice is the identity.
//!   - **Infrared Film:** a B&W channel mix of the encoded values with the weights
//!     [`INFRARED`] (red 0.3, green 1.3, blue −0.6; they sum to 1, so a grey stays the same
//!     grey): foliage, green over blue, comes out lighter than its luma and a blue sky darker;
//!     then Fade toward the input.
//!   - **Duo-Tone:** the Rec.601 luma, through Brightness as a power curve (`l^(2^(−b/50))`, black
//!     and white fixed; +95 lifts the mid-tones, −95 sinks them), then Contrast as an S-curve with
//!     fixed ends ([`modern_contrast`] at `contrast / 128`), mapped onto the straight line from the
//!     first colour (black) to the second (white) (PhotoCraft's `sample_stops` with two stops);
//!     then Fade toward the input.
//!   - **Boost:** Vibrance in linear light by `strength / 100` ([`super::colour::vibrance_lin`]:
//!     dull colours gain most, reds and skin least), then each channel through the S-curve
//!     [`modern_contrast`] at `strength / 2 / 128`. Strength 0 is the identity.
//!
//!   Linear light goes through two tables of 4 096 steps built once (as in `colour`), and the
//!   contrast curves through a table per effect built at prepare. [`modern_contrast`] is ported
//!   from PhotoCraft `crates/compose/src/adjust.rs` at e4b2868a and the two-stop gradient follows
//!   `crates/compose/src/effects.rs` (`sample_stops`) there (MIT OR Apache-2.0, `NOTICE`); the
//!   rest is our own.

use std::fmt;
use std::sync::OnceLock;

use super::colour::vibrance_lin;
use super::{Adjust, Band, FADE, Operation, ParamError, Pass, Prepare, Reader, clamp, hex, no_params, num};
use crate::fx;

/// The range of Duo-Tone's Brightness.
pub const BRIGHTNESS: (f64, f64) = (-95.0, 95.0);
/// The range of Duo-Tone's Contrast.
pub const CONTRAST: (f64, f64) = (0.0, 100.0);
/// The range of Boost's Strength.
pub const STRENGTH: (f64, f64) = (0.0, 100.0);

/// `invert()`: the negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Invert;

/// `infrared(fade)`: B&W infrared, foliage bright and skies dark.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Infrared {
    pub fade: f64,
}

/// `duotone(first,second,brightness,contrast,fade)`: the tones mapped onto the gradient from the
/// first colour (black) to the second (white).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duotone {
    pub first: [u8; 3],
    pub second: [u8; 3],
    pub brightness: f64,
    pub contrast: f64,
    pub fade: f64,
}

impl Default for Duotone {
    fn default() -> Duotone {
        Duotone { first: [0x00, 0x44, 0x88], second: [0xff, 0xff, 0x00], brightness: 0.0, contrast: 20.0, fade: 0.0 }
    }
}

/// `boost(strength)`: stronger colour and contrast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boost {
    pub strength: f64,
}

impl Default for Boost {
    fn default() -> Boost {
        Boost { strength: 50.0 }
    }
}

impl fmt::Display for Invert {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl Operation for Invert {
    const NAME: &'static str = "invert";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Invert Colors";

    fn parse(params: &str) -> Result<Invert, ParamError> {
        no_params(params, Invert)
    }
}

impl fmt::Display for Infrared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.fade))
    }
}

impl Operation for Infrared {
    const NAME: &'static str = "infrared";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Infrared Film";

    fn parse(params: &str) -> Result<Infrared, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let i = Infrared { fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(i)
    }

    fn clamped(self) -> Infrared {
        Infrared { fade: clamp(self.fade, FADE) }
    }
}

impl fmt::Display for Duotone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},{}", hex(self.first), hex(self.second), num(self.brightness), num(self.contrast), num(self.fade))
    }
}

impl Operation for Duotone {
    const NAME: &'static str = "duotone";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Duo-Tone";

    fn parse(params: &str) -> Result<Duotone, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let d = Duotone {
            first: r.colour("the first colour")?,
            second: r.colour("the second colour")?,
            brightness: r.number("the brightness", BRIGHTNESS)?,
            contrast: r.number("the contrast", CONTRAST)?,
            fade: r.number("fade", FADE)?,
        };
        r.end()?;
        Ok(d)
    }

    fn clamped(self) -> Duotone {
        Duotone { brightness: clamp(self.brightness, BRIGHTNESS), contrast: clamp(self.contrast, CONTRAST), fade: clamp(self.fade, FADE), ..self }
    }
}

impl fmt::Display for Boost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.strength))
    }
}

impl Operation for Boost {
    const NAME: &'static str = "boost";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Boost";

    fn parse(params: &str) -> Result<Boost, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let b = Boost { strength: r.number("the strength", STRENGTH)? };
        r.end()?;
        Ok(b)
    }

    fn clamped(self) -> Boost {
        Boost { strength: clamp(self.strength, STRENGTH) }
    }
}

impl Adjust for Invert {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        Some(Box::new(RemapPass::Invert))
    }
}

impl Adjust for Infrared {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let i = self.clamped();
        if i.fade >= FADE.1 {
            return None;
        }
        Some(Box::new(RemapPass::Infrared { fade: i.fade }))
    }
}

impl Adjust for Duotone {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let d = self.clamped();
        if d.fade >= FADE.1 {
            return None;
        }
        let gamma = (2.0f64).powf(-d.brightness / 50.0) as f32;
        let k = d.contrast as f32;
        let tone = Curve::new(|l| modern_contrast(l.powf(gamma), k));
        Some(Box::new(RemapPass::Duotone { tone, first: unit_colour(d.first), second: unit_colour(d.second), fade: d.fade }))
    }
}

impl Adjust for Boost {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let s = self.clamped().strength as f32;
        if s <= 0.0 {
            return None;
        }
        let contrast = Curve::new(|v| modern_contrast(v, s * BOOST_CONTRAST));
        Some(Box::new(RemapPass::Boost { vibrance: s / 100.0, contrast, lin: linear() }))
    }
}

/// Infrared Film's channel weights (red, green, blue) on the encoded values; they sum to 1.
pub const INFRARED: [f32; 3] = [0.3, 1.3, -0.6];

/// Boost's contrast per point of Strength, in [`modern_contrast`]'s units (Strength 100 gives 50).
pub const BOOST_CONTRAST: f32 = 0.5;

fn unit_colour(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| f32::from(v) / 255.0)
}

/// The four remap passes, one per effect.
enum RemapPass {
    Invert,
    Infrared {
        fade: f64,
    },
    /// The tone curve (Brightness, then Contrast) over the luma, the gradient's two ends, Fade.
    Duotone {
        tone: Curve,
        first: [f32; 3],
        second: [f32; 3],
        fade: f64,
    },
    /// Vibrance by this amount (0..1] in linear light, then this curve on each channel.
    Boost {
        vibrance: f32,
        contrast: Curve,
        lin: &'static Linear,
    },
}

impl Pass for RemapPass {
    fn run(&self, band: &mut Band<'_>) {
        let px = band.pixels.iter_mut();
        match self {
            RemapPass::Invert => px.for_each(|p| *p = unit3(*p).map(|v| 1.0 - v)),
            RemapPass::Infrared { fade } => px.for_each(|p| {
                let c = unit3(*p);
                let g = unit(INFRARED[0] * c[0] + INFRARED[1] * c[1] + INFRARED[2] * c[2]);
                *p = faded(c, [g; 3], *fade);
            }),
            RemapPass::Duotone { tone, first, second, fade } => px.for_each(|p| {
                let c = unit3(*p);
                let toned = fx::mix(*first, *second, tone.at(fx::luma(c)));
                *p = faded(c, toned, *fade);
            }),
            RemapPass::Boost { vibrance, contrast, lin } => px.for_each(|p| {
                let vivid = lin.encode3(vibrance_lin(lin.decode3(unit3(*p)), *vibrance));
                *p = vivid.map(|v| contrast.at(v));
            }),
        }
    }
}

/// `effect` faded toward `input` by `fade` ([`fx::fade`]); exactly `effect` at Fade 0, so a grey
/// result stays exactly grey.
fn faded(input: [f32; 3], effect: [f32; 3], fade: f64) -> [f32; 3] {
    unit3(if fade > 0.0 { fx::fade(input, effect, fade) } else { effect })
}

/// `p` with every channel in 0..1 (`NaN` as 0).
fn unit3(p: [f32; 3]) -> [f32; 3] {
    p.map(unit)
}

fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// The number of steps of the tables over 0..1.
const STEPS: usize = 4096;

/// A function of 0..1 sampled at `STEPS + 1` points and read with linear interpolation, so a
/// monotonic function stays monotonic.
struct Curve {
    samples: Vec<f32>,
}

impl Curve {
    fn new(f: impl Fn(f32) -> f32) -> Curve {
        Curve { samples: (0..=STEPS).map(|i| unit(f(i as f32 / STEPS as f32))).collect() }
    }

    /// The function at `x` (brought into 0..1, `NaN` as 0).
    fn at(&self, x: f32) -> f32 {
        let pos = unit(x) * STEPS as f32;
        let i = (pos.floor() as usize).min(STEPS);
        let t = pos - i as f32;
        match (self.samples.get(i), self.samples.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * t,
            (Some(a), None) => *a,
            _ => 0.0,
        }
    }
}

/// The sRGB transfer curve and its inverse as tables, built once.
struct Linear {
    decode: Curve,
    encode: Curve,
}

impl Linear {
    fn decode3(&self, p: [f32; 3]) -> [f32; 3] {
        p.map(|v| self.decode.at(v))
    }

    fn encode3(&self, p: [f32; 3]) -> [f32; 3] {
        p.map(|v| self.encode.at(v))
    }
}

fn linear() -> &'static Linear {
    static LINEAR: OnceLock<Linear> = OnceLock::new();
    LINEAR.get_or_init(|| Linear { decode: Curve::new(fx::to_linear), encode: Curve::new(fx::to_srgb) })
}

// Ported from PhotoCraft `crates/compose/src/adjust.rs` at e4b2868a (MIT OR Apache-2.0).

/// A contrast S-curve on 0..1 with fixed ends: two cubic Hermite halves through (0, 0), (½, ½) and
/// (1, 1), slope `1 − k` at the ends and `1 + k` at the middle, `k = contrast / 128` (`contrast`
/// 0 is the identity; brought into 0..128, `NaN` as 0). The input is brought into 0..1.
pub fn modern_contrast(v: f32, contrast: f32) -> f32 {
    let contrast = if contrast.is_nan() { 0.0 } else { contrast.clamp(0.0, 128.0) };
    let v = unit(v);
    if contrast == 0.0 {
        return v;
    }
    let k = contrast / 128.0;
    let (end, mid) = (1.0 - k, 1.0 + k);
    // Cubic Hermite on [0, 0.5]: pinned (0,0) slope `end`, (0.5,0.5) slope `mid`. `t` in [0,1].
    let half = |t: f32| end * 0.5 * (t * t * t - 2.0 * t * t + t) + (-2.0 * t * t * t + 3.0 * t * t) * 0.5 + mid * 0.5 * (t * t * t - t * t);
    if v <= 0.5 { half(v / 0.5) } else { 1.0 - half((1.0 - v) / 0.5) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_contrast_curve_keeps_its_ends_and_rises() {
        for c in [0.0, 10.0, 25.6, 50.0, 128.0, f32::NAN, 1e9, -5.0] {
            assert!(modern_contrast(0.0, c).abs() < 1e-6 && (modern_contrast(1.0, c) - 1.0).abs() < 1e-6, "{c}");
            assert!((modern_contrast(0.5, c) - 0.5).abs() < 1e-6, "{c}");
            let mut last = 0.0;
            for i in 0..=1000 {
                let v = modern_contrast(i as f32 / 1000.0, c);
                assert!(v >= last - 1e-6, "{c}: falls at {i}");
                last = v;
            }
        }
    }
}
