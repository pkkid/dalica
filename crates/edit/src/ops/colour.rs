//! The colour effects of tab 3 (`editing.md` §8.2): `sepia()`, `bw()`, `warmify()`,
//! `tint(color,fade)`, `saturation(a)` and `filtered_bw(color)`.
//!
//! - **Parameters** (M12.1; read with [`super::Reader`], `editing.md` §8.5): Sepia, B&W and Warmify
//!   take none (one click); Tint's colour (`#80cfff`) and Fade 0–100 (0); Saturation's Amount
//!   −1…+1 (0.16; −1 is B&W, 0 the photo); Filtered B&W's filter colour (`#ff8000`, an orange
//!   filter). Numbers out of range are clamped; `NaN`, a missing or extra field or a colour that
//!   is not `#rrggbb` is refused (the entry is kept and skipped).
//! - **Rendering** (M12.6): each is one pixel-only pass ([`super::Reach::Pixel`], fused with its
//!   neighbours) on the renderer's sRGB-encoded working values, `VERSION` 1:
//!   - **Sepia:** the Rec.601 luma through three fixed warm-brown tone curves (powers 0.75, 0.95
//!     and 1.35 of the luma for red, green and blue), so R ≥ G ≥ B everywhere, black stays black,
//!     white stays white and a brighter pixel never comes out darker.
//!   - **B&W:** the Rec.601 luma of the encoded values ([`crate::fx::luma`]).
//!   - **Warmify:** a warming photo filter (`#ec8a00` at 25 % density, keeping the luminance Y) as
//!     one linear-light matrix, `photo_filter_matrix`, then the input's Rec.601 luma restored (the
//!     Luminosity blend's `SetLum`): oranges and skin tones turn golden, blues lose a little, the
//!     brightness stays. Only a colour at the gamut's edge (a channel already at its end) can come
//!     out a little less red-over-blue, since its warmer channel cannot grow.
//!   - **Tint:** desaturate to the Rec.601 luma, then colourise along black → the tint (at its own
//!     luma) → white, straight lines between ([`colourise`]): every coloured pixel takes the
//!     tint's hue and keeps its luma; then Fade toward the input. (`hue_saturation`'s colorize sets
//!     an HSL saturation at the luma as lightness instead; at the tint's full saturation it doubles
//!     the contrast of a channel and folds at mid-grey, so a thumbnail and a full render of the
//!     same edge disagree. The straight line keeps both slopes near 1.)
//!   - **Saturation:** below 0, every pixel moves toward its Rec.601 luma (−1 is B&W); above 0,
//!     Vibrance in linear light (`vibrance_px`, `vibrance_lin`), which lifts the dull colours most
//!     and spares reds and skin.
//!   - **Filtered B&W:** the luma seen through a coloured filter: the Rec.601 weights each scaled
//!     by the filter's transmission of that channel and normalised to sum to 1, so a grey stays
//!     the same grey and a red filter lightens reds and darkens blues (`black_white_gray`'s six
//!     hue weights, taken from one filter colour, reduce to this weighted sum); a black filter
//!     transmits nothing and falls back to plain B&W.
//!
//!   Linear light goes through two tables of 4 096 steps built once ([`crate::fx::to_linear`],
//!   [`crate::fx::to_srgb`]), within 0.01 of a level of the exact curve. `photo_filter_matrix`,
//!   `vibrance_px`, `vibrance_lin` and their constants are ported from PhotoCraft
//!   `crates/compose/src/adjust.rs` and `crates/color/src/convert.rs` at e4b2868a (MIT OR
//!   Apache-2.0, `NOTICE`); the rest is our own.

use std::fmt;
use std::sync::OnceLock;

use super::{Adjust, Band, FADE, Operation, ParamError, Pass, Prepare, Reader, clamp, hex, no_params, num};
use crate::fx;

/// The range of Saturation's Amount.
pub const SATURATION: (f64, f64) = (-1.0, 1.0);

/// `sepia()`: warm brown monochrome.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sepia;

/// `bw()`: luminance greyscale (Ctrl+Shift+B).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bw;

/// `warmify()`: warmer colours, golden skin tones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Warmify;

/// `tint(color,fade)`: desaturate, then colourise toward `color`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub color: [u8; 3],
    /// 0–100: 0 is the effect, 100 the input.
    pub fade: f64,
}

impl Default for Tint {
    fn default() -> Tint {
        Tint { color: [0x80, 0xcf, 0xff], fade: 0.0 }
    }
}

/// `saturation(a)`: toward B&W (−1) or vivid (+1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saturation {
    pub amount: f64,
}

impl Default for Saturation {
    fn default() -> Saturation {
        Saturation { amount: 0.16 }
    }
}

/// `filtered_bw(color)`: B&W through a filter of `color`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilteredBw {
    pub color: [u8; 3],
}

impl Default for FilteredBw {
    fn default() -> FilteredBw {
        FilteredBw { color: [0xff, 0x80, 0x00] }
    }
}

macro_rules! one_click {
    ($t:ident, $name:literal, $label:literal) => {
        impl fmt::Display for $t {
            fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
                Ok(())
            }
        }

        impl Operation for $t {
            const NAME: &'static str = $name;
            const VERSION: u32 = 1;
            const LABEL: &'static str = $label;

            fn parse(params: &str) -> Result<$t, ParamError> {
                no_params(params, $t)
            }
        }
    };
}

one_click!(Sepia, "sepia", "Sepia");
one_click!(Bw, "bw", "B&W");
one_click!(Warmify, "warmify", "Warmify");

impl fmt::Display for Tint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", hex(self.color), num(self.fade))
    }
}

impl Operation for Tint {
    const NAME: &'static str = "tint";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Tint";

    fn parse(params: &str) -> Result<Tint, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let t = Tint { color: r.colour("the colour")?, fade: r.number("fade", FADE)? };
        r.end()?;
        Ok(t)
    }

    fn clamped(self) -> Tint {
        Tint { fade: clamp(self.fade, FADE), ..self }
    }
}

impl fmt::Display for Saturation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&num(self.amount))
    }
}

impl Operation for Saturation {
    const NAME: &'static str = "saturation";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Saturation";

    fn parse(params: &str) -> Result<Saturation, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let s = Saturation { amount: r.number("the amount", SATURATION)? };
        r.end()?;
        Ok(s)
    }

    fn clamped(self) -> Saturation {
        Saturation { amount: clamp(self.amount, SATURATION) }
    }
}

impl fmt::Display for FilteredBw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(self.color))
    }
}

impl Operation for FilteredBw {
    const NAME: &'static str = "filtered_bw";
    const VERSION: u32 = 1;
    const LABEL: &'static str = "Filtered B&W";

    fn parse(params: &str) -> Result<FilteredBw, ParamError> {
        let mut r = Reader::new(Self::NAME, params);
        let b = FilteredBw { color: r.colour("the filter colour")? };
        r.end()?;
        Ok(b)
    }
}

impl Adjust for Sepia {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        Some(Box::new(ColourPass::Sepia(sepia_curves())))
    }
}

impl Adjust for Bw {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        Some(Box::new(ColourPass::Grey(REC601)))
    }
}

impl Adjust for Warmify {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let m = photo_filter_matrix(WARMING.map(|v| f32::from(v) / 255.0), WARMING_DENSITY, true);
        Some(Box::new(ColourPass::Matrix(m, linear())))
    }
}

impl Adjust for Tint {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let t = self.clamped();
        if t.fade >= FADE.1 {
            return None;
        }
        Some(Box::new(ColourPass::Tint { color: t.color.map(|v| f32::from(v) / 255.0), fade: t.fade }))
    }
}

impl Adjust for Saturation {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        let a = self.clamped().amount as f32;
        if a == 0.0 {
            None
        } else if a < 0.0 {
            Some(Box::new(ColourPass::Desaturate(1.0 + a)))
        } else {
            Some(Box::new(ColourPass::Vibrance(a, linear())))
        }
    }
}

impl Adjust for FilteredBw {
    fn prepare(&self, _ctx: &Prepare<'_>) -> Option<Box<dyn Pass>> {
        Some(Box::new(ColourPass::Grey(filter_weights(self.color))))
    }
}

/// The Rec.601 luma weights (`editing.md` §8.2), as [`fx::luma`] uses them.
pub const REC601: [f32; 3] = [0.299, 0.587, 0.114];

/// Warmify's filter colour (a warming orange) and density.
pub const WARMING: [u8; 3] = [0xec, 0x8a, 0x00];
pub const WARMING_DENSITY: f32 = 0.25;

/// Sepia's tone curves: red, green and blue are these powers of the luma.
pub const SEPIA_POWERS: [f32; 3] = [0.75, 0.95, 1.35];

/// The grey weights of Filtered B&W for a filter of colour `filter`: the Rec.601 weights scaled by
/// what the filter lets through of each channel, normalised to sum to 1 (plain Rec.601 for a
/// filter that lets nothing through).
pub fn filter_weights(filter: [u8; 3]) -> [f32; 3] {
    let w: [f32; 3] = std::array::from_fn(|c| REC601[c] * f32::from(filter[c]) / 255.0);
    let sum = w[0] + w[1] + w[2];
    if sum > 1e-4 { w.map(|v| v / sum) } else { REC601 }
}

/// The six colour passes, one per effect.
enum ColourPass {
    /// Sepia's three curves over the luma.
    Sepia(&'static Table<[f32; 3]>),
    /// A grey of these channel weights (B&W, Filtered B&W).
    Grey([f32; 3]),
    /// A matrix on linear light (Warmify).
    Matrix([[f32; 3]; 3], &'static Linear),
    /// The luma colourised through this colour ([`colourise`]), then faded.
    Tint { color: [f32; 3], fade: f64 },
    /// Each pixel's distance from its luma scaled by this factor in 0..1.
    Desaturate(f32),
    /// Vibrance by this amount in 0..1, in linear light.
    Vibrance(f32, &'static Linear),
}

impl Pass for ColourPass {
    fn run(&self, band: &mut Band<'_>) {
        let px = band.pixels.iter_mut();
        match self {
            ColourPass::Sepia(curves) => px.for_each(|p| *p = curves.at(fx::luma(unit3(*p)))),
            ColourPass::Grey(w) => px.for_each(|p| *p = [grey(*p, *w); 3]),
            ColourPass::Matrix(m, lin) => px.for_each(|p| {
                let input = unit3(*p);
                let filtered = lin.encode3(mat3_mul(m, lin.decode3(input)));
                *p = set_luma(filtered, fx::luma(input));
            }),
            ColourPass::Tint { color, fade } => px.for_each(|p| {
                let input = unit3(*p);
                let toned = colourise(fx::luma(input), *color);
                *p = unit3(fx::fade(input, toned, *fade));
            }),
            ColourPass::Desaturate(k) => px.for_each(|p| {
                let c = unit3(*p);
                let g = fx::luma(c);
                *p = unit3(c.map(|v| g + k * (v - g)));
            }),
            ColourPass::Vibrance(v, lin) => px.for_each(|p| *p = lin.encode3(vibrance_lin(lin.decode3(*p), *v))),
        }
    }
}

/// `p` with every channel in 0..1 (`NaN` as 0).
fn unit3(p: [f32; 3]) -> [f32; 3] {
    p.map(unit)
}

fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// The weighted grey of `p`, in 0..1.
fn grey(p: [f32; 3], w: [f32; 3]) -> f32 {
    let c = unit3(p);
    unit(w[0] * c[0] + w[1] * c[1] + w[2] * c[2])
}

/// `c` moved to Rec.601 luma `l` keeping its hue, its channels brought into 0..1 toward the luma
/// (the W3C *Compositing and Blending* `SetLum` and `ClipColor`, Rec.601 weights).
fn set_luma(c: [f32; 3], l: f32) -> [f32; 3] {
    let l = unit(l);
    let d = l - fx::luma(c);
    let c = c.map(|v| v + d);
    let lum = fx::luma(c);
    let (mn, mx) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    let c = if mn < 0.0 && lum - mn > 1e-6 { c.map(|v| lum + (v - lum) * lum / (lum - mn)) } else { c };
    let mx = if mn < 0.0 { c[0].max(c[1]).max(c[2]) } else { mx };
    let c = if mx > 1.0 && mx - lum > 1e-6 { c.map(|v| lum + (v - lum) * (1.0 - lum) / (mx - lum)) } else { c };
    unit3(c)
}

/// The grey of luma `l` colourised toward `tint`: black, then `tint` at its own Rec.601 luma, then
/// white, straight lines between (a grey tint, or a black or white one, gives the grey). Every
/// result has the tint's hue and the luma `l`, and stays in 0..1.
pub fn colourise(l: f32, tint: [f32; 3]) -> [f32; 3] {
    let (l, tint) = (unit(l), unit3(tint));
    let lt = fx::luma(tint);
    let w = if lt <= 1e-6 || lt >= 1.0 - 1e-6 {
        0.0
    } else if l <= lt {
        l / lt
    } else {
        (1.0 - l) / (1.0 - lt)
    };
    unit3(tint.map(|t| l + w * (t - lt)))
}

/// The number of steps of the tables over 0..1.
const STEPS: usize = 4096;

/// A function of 0..1 sampled at `STEPS + 1` points and read with linear interpolation, so a
/// monotonic function stays monotonic.
struct Table<T> {
    samples: Vec<T>,
}

impl<T: Copy + Lerp> Table<T> {
    fn new(f: impl Fn(f32) -> T) -> Table<T> {
        Table { samples: (0..=STEPS).map(|i| f(i as f32 / STEPS as f32)).collect() }
    }

    /// The function at `x` (brought into 0..1, `NaN` as 0).
    fn at(&self, x: f32) -> T {
        let pos = unit(x) * STEPS as f32;
        let i = (pos.floor() as usize).min(STEPS);
        let t = pos - i as f32;
        match (self.samples.get(i), self.samples.get(i + 1)) {
            (Some(a), Some(b)) => a.lerp(*b, t),
            (Some(a), None) => *a,
            _ => T::ZERO,
        }
    }
}

trait Lerp: Sized {
    const ZERO: Self;
    fn lerp(self, to: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    const ZERO: f32 = 0.0;
    fn lerp(self, to: f32, t: f32) -> f32 {
        self + (to - self) * t
    }
}

impl Lerp for [f32; 3] {
    const ZERO: [f32; 3] = [0.0; 3];
    fn lerp(self, to: [f32; 3], t: f32) -> [f32; 3] {
        std::array::from_fn(|c| self[c].lerp(to[c], t))
    }
}

/// Sepia's curves, built once.
fn sepia_curves() -> &'static Table<[f32; 3]> {
    static CURVES: OnceLock<Table<[f32; 3]>> = OnceLock::new();
    CURVES.get_or_init(|| Table::new(|l| SEPIA_POWERS.map(|k| l.powf(k))))
}

/// The sRGB transfer curve and its inverse as tables, built once.
struct Linear {
    decode: Table<f32>,
    encode: Table<f32>,
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
    LINEAR.get_or_init(|| Linear { decode: Table::new(fx::to_linear), encode: Table::new(fx::to_srgb) })
}

// Ported from PhotoCraft `crates/color/src/convert.rs` at e4b2868a (MIT OR Apache-2.0).

/// The D50 white point (ICC PCS).
const D50: [f32; 3] = [0.964_22, 1.0, 0.825_21];

/// Linear sRGB → D50 XYZ (Bradford-adapted, as in the ICC sRGB profile). Rows sum to [`D50`].
const SRGB_TO_XYZ_D50: [[f32; 3]; 3] = [[0.436_074, 0.385_064, 0.143_080], [0.222_504, 0.716_878, 0.060_618], [0.013_932, 0.097_104, 0.714_173]];

/// D50 XYZ → linear sRGB (the inverse of [`SRGB_TO_XYZ_D50`]).
const XYZ_D50_TO_SRGB: [[f32; 3]; 3] = [[3.133_856, -1.616_867, -0.490_615], [-0.978_768, 1.916_142, 0.033_454], [0.071_945, -0.228_991, 1.405_243]];

/// `m · v` for a row-major 3 × 3 matrix.
fn mat3_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

// Ported from PhotoCraft `crates/compose/src/adjust.rs` at e4b2868a (MIT OR Apache-2.0): the photo
// filter's matrix and Vibrance; sRGB decoding through Dalica's `fx`.

/// A photo filter's linear-light RGB matrix: the pixel's D50 XYZ, relative to white, multiplied by
/// the filter colour's (mixed in by `density`), folded into one matrix. With `normalize_y` the map
/// keeps the luminance Y.
pub fn photo_filter_matrix(color: [f32; 3], density: f32, normalize_y: bool) -> [[f32; 3]; 3] {
    let d = if density.is_finite() { density.clamp(0.0, 1.0) } else { 0.0 };
    let xyz = mat3_mul(&SRGB_TO_XYZ_D50, color.map(|v| fx::to_linear(unit(v))));
    let mut s: [f32; 3] = std::array::from_fn(|i| 1.0 - d + d * xyz[i] / D50[i]);
    if normalize_y && s[1] > 1e-6 {
        let y = s[1];
        s = s.map(|v| v / y);
    }
    // XYZ_D50_TO_SRGB · diag(s) · SRGB_TO_XYZ_D50
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| XYZ_D50_TO_SRGB[r][k] * s[k] * SRGB_TO_XYZ_D50[k][c]).sum()))
}

/// Positive Vibrance's saturation boost, fitted to measurements (see [`vibrance_lin`]).
const VIBRANCE_BOOST: [f32; 7] = [0.5001, 0.0887, 0.4374, 0.7573, -0.8113, 0.5154, 2.8026];

/// Vibrance (`v` in −1..1) on a linear-light pixel, keeping its hue. With `t = 1 − min/max` (HSV
/// saturation) and `V = max`: below 0 (`a = −v`) `t' = t (1 − a/4) (1 − a + a t (1 + t) / 2)` and
/// `V' = V − a t (1 − t) (2 − t + t²) V (1 − V)`; above 0 `V' = V + v/4 t (1 − t) (2 − t + t²) V
/// (1 − V)` and a saturation boost that grows as `t` and `V` fall, damped on reds and skin (HSV hue
/// 0–30°, fading out by 45° and in from 300°). The result is brought into 0..1.
pub fn vibrance_lin(c: [f32; 3], v: f32) -> [f32; 3] {
    let c = unit3(c);
    let v = if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) };
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx <= 0.0 || mx - mn <= 1e-9 {
        return c;
    }
    let t = 1.0 - mn / mx;
    let shape = t * (1.0 - t) * (2.0 - t + t * t) * mx * (1.0 - mx).max(0.0);
    let (t2, v2) = if v < 0.0 {
        let a = -v;
        (t * (1.0 - a / 4.0) * (1.0 - a + a * t * (1.0 + t) / 2.0), mx - a * shape)
    } else {
        let [c0, c1, p, q, r0, ra, rt] = VIBRANCE_BOOST;
        let w = vibrance_skin(hsv_hue(c, mx, mn));
        let a = v * (1.0 - w) + 0.64 * v.powf(1.6) * w;
        let boost = (c0 * a + c1 * a * a) * t.powf(p) * (1.0 - t).powf(q) * ((r0 + ra * a + rt * (1.0 - t)) * (1.0 - mx).max(0.0)).exp();
        ((t * (1.0 + boost)).min(1.0), mx + v / 4.0 * shape)
    };
    let m2 = v2 * (1.0 - t2);
    let k = (v2 - m2) / (mx - mn);
    unit3(c.map(|x| m2 + (x - mn) * k))
}

/// How much positive Vibrance treats a hue (degrees) as skin: 1 on 0–30°, fading to 0 by 45° and
/// in again from 300°.
fn vibrance_skin(h: f32) -> f32 {
    if h <= 30.0 {
        1.0
    } else if h < 45.0 {
        (45.0 - h) / 15.0
    } else if h < 300.0 {
        0.0
    } else {
        (h - 300.0) / 60.0
    }
}

/// HSV hue in degrees of `c` (its max `mx` and min `mn` given, `mx > mn`).
fn hsv_hue(c: [f32; 3], mx: f32, mn: f32) -> f32 {
    let d = mx - mn;
    let h = if mx == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    h * 60.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_linear_tables_are_within_a_hundredth_of_a_level_of_the_curve() {
        let lin = linear();
        let worst = (0..=100_000).map(|i| i as f32 / 100_000.0).fold(0.0f32, |w, v| {
            let d = (lin.decode.at(v) - fx::to_linear(v)).abs().max((lin.encode.at(v) - fx::to_srgb(v)).abs());
            w.max(d)
        });
        assert!(worst * 255.0 < 0.01, "{} of a level", worst * 255.0);
    }
}
