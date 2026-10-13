//! The tone and colour stage: `tuning(fill,highlights,shadows,temp,neutral)` (`editing.md` §7) and
//! `fill_light(a)` (§6.5; the same Fill Light as Tuning's `fill`).
//!
//! One pixel-only pass, our own curves, on the sRGB-encoded working values:
//!
//! 1. **Colour.** The Neutral colour's gains (each channel scaled so the picked colour becomes the
//!    grey of its own luma, each gain kept within ¼…4) and the Color Temperature's gains (warmer:
//!    more red and a little green, less blue; cooler the other way; normalised so a grey keeps its
//!    luma), as one multiplication per channel.
//! 2. **Tone**, on the luma (Rec. 709 weights), as one curve through a table: Fill Light lifts with
//!    a curve that never darkens and lifts the shadows most; then Highlights
//!    brightens only what lies above the middle (0.5) and Shadows darkens only what lies below it,
//!    each with a curve that meets the identity smoothly at the middle and keeps black and white
//!    in place. A pixel is scaled by the curve's ratio, which keeps its hue.
//!
//! `fill_light(a)` renders exactly as `tuning(a,0,0,0,none)`; Dalica writes `tuning` and keeps a
//! `fill_light` it reads as written (`data-model.md` §3.1).
//!
//! Parameters: numbers out of range (and ±infinity) are brought to the nearest end of their range
//! when read; `NaN`, a missing or extra field, or a neutral that is not `#rrggbb` or `none` is
//! refused (the entry is then kept and skipped, as for every module).

use std::fmt;

use super::{Adjust, Band, Operation, ParamError, Pass, Prepare};

/// The range of Fill Light (`fill` and `fill_light`).
pub const FILL: (f64, f64) = (0.0, 1.0);
/// The range of Highlights.
pub const HIGHLIGHTS: (f64, f64) = (0.0, 0.5);
/// The range of Shadows.
pub const SHADOWS: (f64, f64) = (0.0, 0.5);
/// The range of Color Temperature (cooler … warmer).
pub const TEMP: (f64, f64) = (-1.0, 1.0);

/// The whole Tuning tab as one operation (`editing.md` §7). The default changes nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tuning {
    /// Fill Light, 0…1: lifts the shadows and the foreground.
    pub fill: f64,
    /// Highlights, 0…0.5: brightens bright areas only.
    pub highlights: f64,
    /// Shadows, 0…0.5: darkens dark areas only.
    pub shadows: f64,
    /// Color Temperature, −1 (cooler) … +1 (warmer).
    pub temp: f64,
    /// The picked colour to make grey (`#rrggbb`), or `None` (`none`).
    pub neutral: Option<[u8; 3]>,
}

/// `fill_light(a)`, the Basic Fixes slider as older sidecars store it: Tuning with only `fill`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FillLight {
    /// 0…1.
    pub amount: f64,
}

impl FillLight {
    /// The Tuning this renders as.
    pub fn as_tuning(&self) -> Tuning {
        Tuning { fill: self.amount, ..Tuning::default() }
    }
}

/// `v` brought into `range` (`NaN` to its low end); `-0` becomes `0` so it prints as `0`.
fn clamp(v: f64, range: (f64, f64)) -> f64 {
    if v.is_nan() { range.0 } else { v.clamp(range.0, range.1) + 0.0 }
}

/// A number field: finite or infinite values are clamped into `range`; `NaN` and text that is not
/// a number are refused.
fn number(op: &'static str, params: &str, field: &str, what: &str, range: (f64, f64)) -> Result<f64, ParamError> {
    match field.trim().parse::<f64>() {
        Ok(v) if !v.is_nan() => Ok(clamp(v, range)),
        _ => Err(ParamError::new(op, params, format!("{what} is not a number"))),
    }
}

/// `#rrggbb` (either case) or `none`.
fn neutral(params: &str, field: &str) -> Result<Option<[u8; 3]>, ParamError> {
    let field = field.trim();
    if field == "none" {
        return Ok(None);
    }
    let bad = || ParamError::new(Tuning::NAME, params, "the neutral colour is not #rrggbb or none");
    let hex = field.strip_prefix('#').filter(|h| h.len() == 6 && h.is_ascii()).ok_or_else(bad)?;
    let byte = |i: usize| hex.get(i..i + 2).filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit())).and_then(|s| u8::from_str_radix(s, 16).ok());
    match (byte(0), byte(2), byte(4)) {
        (Some(r), Some(g), Some(b)) => Ok(Some([r, g, b])),
        _ => Err(bad()),
    }
}

impl fmt::Display for Tuning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{},{},{},", self.fill, self.highlights, self.shadows, self.temp)?;
        match self.neutral {
            Some([r, g, b]) => write!(f, "#{r:02x}{g:02x}{b:02x}"),
            None => f.write_str("none"),
        }
    }
}

impl fmt::Display for FillLight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.amount)
    }
}

impl Operation for Tuning {
    const NAME: &'static str = "tuning";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Tuning";

    /// Five fields: `fill,highlights,shadows,temp,neutral`.
    fn parse(params: &str) -> Result<Tuning, ParamError> {
        let fields: Vec<&str> = params.split(',').collect();
        let [fill, highlights, shadows, temp, colour] = fields.as_slice() else {
            return Err(ParamError::new(Self::NAME, params, "takes fill,highlights,shadows,temp,neutral"));
        };
        Ok(Tuning {
            fill: number(Self::NAME, params, fill, "fill", FILL)?,
            highlights: number(Self::NAME, params, highlights, "highlights", HIGHLIGHTS)?,
            shadows: number(Self::NAME, params, shadows, "shadows", SHADOWS)?,
            temp: number(Self::NAME, params, temp, "temp", TEMP)?,
            neutral: neutral(params, colour)?,
        })
    }

    fn clamped(self) -> Tuning {
        Tuning {
            fill: clamp(self.fill, FILL),
            highlights: clamp(self.highlights, HIGHLIGHTS),
            shadows: clamp(self.shadows, SHADOWS),
            temp: clamp(self.temp, TEMP),
            neutral: self.neutral,
        }
    }
}

impl Operation for FillLight {
    const NAME: &'static str = "fill_light";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Fill Light";

    fn parse(params: &str) -> Result<FillLight, ParamError> {
        Ok(FillLight { amount: number(Self::NAME, params, params, "the amount", FILL)? })
    }

    fn clamped(self) -> FillLight {
        FillLight { amount: clamp(self.amount, FILL) }
    }
}

impl Adjust for Tuning {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let t = self.clamped();
        let gains = mul(neutral_gains(t.neutral), temp_gains(t.temp));
        let curve = (t.fill > 0.0 || t.highlights > 0.0 || t.shadows > 0.0).then(|| Curve::new(t.fill, t.highlights, t.shadows));
        if gains == [1.0; 3] && curve.is_none() {
            return None;
        }
        Some(Box::new(TonePass { gains, curve }))
    }
}

impl Adjust for FillLight {
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        self.as_tuning().prepare(ctx)
    }
}

/// The luma of sRGB-encoded values (Rec. 709 weights), the value the tone curve reads.
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn mul(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

/// The smallest and largest gain the neutral colour may give a channel.
const NEUTRAL_GAIN: (f32, f32) = (0.25, 4.0);

/// The per-channel gains that make `picked` the grey of its own luma (none: no change).
fn neutral_gains(picked: Option<[u8; 3]>) -> [f32; 3] {
    let Some(p) = picked else { return [1.0; 3] };
    let c = p.map(|v| f32::from(v) / 255.0);
    let y = luma(c);
    if y <= 0.0 {
        // Black carries no colour to correct.
        return [1.0; 3];
    }
    c.map(|v| if v > 0.0 { (y / v).clamp(NEUTRAL_GAIN.0, NEUTRAL_GAIN.1) } else { NEUTRAL_GAIN.1 })
}

/// How much Color Temperature ±1 moves each channel (warmer: red up, a little green, blue down).
const TEMP_SLOPE: [f32; 3] = [0.12, 0.03, -0.15];

/// The per-channel gains of temperature `t` (−1…1), normalised so a grey keeps its luma.
fn temp_gains(t: f64) -> [f32; 3] {
    if t == 0.0 {
        return [1.0; 3];
    }
    let t = t as f32;
    let g = TEMP_SLOPE.map(|s| 1.0 + s * t);
    let y = luma(g);
    if y > 0.0 { g.map(|v| v / y) } else { [1.0; 3] }
}

/// Intervals of the tone table; 0.5 falls on an entry, so the middle is exact.
const STEPS: usize = 1024;

/// The tone curve on the luma, tabulated.
struct Curve {
    table: Vec<f32>,
}

/// Fill Light: `l + 3·fill·l·(1 − l)³`. It never lies below the identity, keeps black and white,
/// rises everywhere (its slope stays above 1 − 3/4), lifts most in absolute terms at a quarter
/// and, relative to the pixel, the darker the more (at most 1 + 3·fill times, so the noise near
/// black is not blown up).
fn fill_curve(l: f64, fill: f64) -> f64 {
    let d = 1.0 - l;
    l + FILL_STRENGTH * fill * l * d * d * d
}

/// Fill Light's strength at 1 (at most 4 keeps the curve rising).
const FILL_STRENGTH: f64 = 3.0;

/// The brightening curve of 0…1 that Highlights and (mirrored) Shadows use: the identity at
/// `amount` 0; rising, never below the identity, slope 1 at 0, and 1 at 1 for every amount.
fn lift(t: f64, amount: f64) -> f64 {
    let p = 1.0 + 4.0 * amount;
    let t = t.clamp(0.0, 1.0);
    2.0 * t - t * t - t * (1.0 - t).powf(p)
}

impl Curve {
    fn new(fill: f64, highlights: f64, shadows: f64) -> Curve {
        let table = (0..=STEPS)
            .map(|i| {
                let l = fill_curve(i as f64 / STEPS as f64, fill);
                let out = if l > 0.5 { 0.5 + 0.5 * lift((l - 0.5) / 0.5, highlights) } else { 0.5 - 0.5 * lift((0.5 - l) / 0.5, shadows) };
                out.clamp(0.0, 1.0) as f32
            })
            .collect();
        Curve { table }
    }

    /// The curve at `l` (0…1), linear between the table's entries.
    fn at(&self, l: f32) -> f32 {
        let x = l.clamp(0.0, 1.0) * STEPS as f32;
        let i = (x.floor() as usize).min(STEPS - 1);
        let frac = x - i as f32;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * frac,
            _ => l,
        }
    }
}

/// The prepared Tuning: channel gains, then the tone curve on the luma.
struct TonePass {
    gains: [f32; 3],
    curve: Option<Curve>,
}

impl Pass for TonePass {
    fn run(&self, band: &mut Band<'_>) {
        for p in band.pixels.iter_mut() {
            let mut c = mul(*p, self.gains).map(|v| v.clamp(0.0, 1.0));
            if let Some(curve) = &self.curve {
                let l = luma(c);
                if l > 1e-6 {
                    let ratio = curve.at(l) / l;
                    c = c.map(|v| (v * ratio).clamp(0.0, 1.0));
                }
            }
            *p = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curves_keep_their_ends_and_their_side_of_the_middle() {
        for amount in [0.0, 0.1, 0.25, 0.5] {
            assert!(lift(0.0, amount).abs() < 1e-12);
            assert!((lift(1.0, amount) - 1.0).abs() < 1e-12);
            let mut last = 0.0;
            for i in 1..=100 {
                let t = f64::from(i) / 100.0;
                let v = lift(t, amount);
                assert!(v >= last - 1e-12 && v >= t - 1e-12, "{amount} at {t}: {v}");
                last = v;
            }
        }
        for fill in [0.0, 0.3, 1.0] {
            assert!(fill_curve(0.0, fill).abs() < 1e-12);
            assert!((fill_curve(1.0, fill) - 1.0).abs() < 1e-12);
        }
        let c = Curve::new(0.0, 0.5, 0.0);
        assert_eq!(c.at(0.5), 0.5);
        assert!((c.at(0.25) - 0.25).abs() < 1e-6);
        assert!(c.at(0.75) > 0.8);
        let c = Curve::new(0.0, 0.0, 0.5);
        assert!((c.at(0.75) - 0.75).abs() < 1e-6);
        assert!(c.at(0.25) < 0.2);
    }

    #[test]
    fn neutral_gains_stay_in_bounds() {
        assert_eq!(neutral_gains(Some([0, 0, 0])), [1.0; 3]);
        assert_eq!(neutral_gains(Some([255, 0, 0])), [NEUTRAL_GAIN.0, NEUTRAL_GAIN.1, NEUTRAL_GAIN.1]);
        assert!(neutral_gains(Some([128, 128, 128])).iter().all(|v| (v - 1.0).abs() < 1e-6));
        assert_eq!(neutral_gains(None), [1.0; 3]);
    }
}
