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
//!   Linear light goes through the shared tables of 4 096 steps ([`crate::fx::linear_light`],
//!   within 0.01 of a level of the exact curves).
//! - **Speed** (M13.38): Warmify and Saturation above 0 (and Boost, in `remap`) run by lanes:
//!   `LANES` pixels at a time, each channel one array, so their maths compiles to vector
//!   instructions; the tables are read as `LaneTable`s (the shared tables' own samples, each step
//!   with its rise to the next, the step found by a float-rounding trick), Vibrance's `exp` by a
//!   polynomial and its `t^p (1 − t)^q` from a table. Each pass stays within one level (in fact a
//!   few hundredths) of its exact maths, `powf` curves included, over every 8-bit grey and a
//!   million random inputs between levels (the module's tests). A 3-D table of the whole function
//!   (33³ or 65³ nodes, tetrahedral) was measured first and missed that by 4 to 12 levels where
//!   the skin hues' weight bends near grey and where Warmify's luma restore clips.
//! - **Provenance:** `photo_filter_matrix`, `vibrance_px`, `vibrance_lin` and their constants are
//!   ported from PhotoCraft `crates/compose/src/adjust.rs` and `crates/color/src/convert.rs` at
//!   e4b2868a (MIT OR Apache-2.0, `NOTICE`); the rest is our own.

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
        Some(Box::new(warmify_pass()))
    }
}

/// Warmify's pass: its matrix on linear light, by lanes.
fn warmify_pass() -> ColourPass {
    ColourPass::Warmify(photo_filter_matrix(WARMING.map(|v| f32::from(v) / 255.0), WARMING_DENSITY, true), lane_linear())
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
        saturation_pass(self.clamped().amount as f32).map(|p| Box::new(p) as Box<dyn Pass>)
    }
}

/// Saturation's pass at amount `a` (−1..1), none at 0.
fn saturation_pass(a: f32) -> Option<ColourPass> {
    if a == 0.0 {
        None
    } else if a < 0.0 {
        Some(ColourPass::Desaturate(1.0 + a))
    } else {
        Vibrance::new(a).map(ColourPass::Vibrance)
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
    /// A matrix on linear light, then the input's luma restored (Warmify).
    Warmify([[f32; 3]; 3], &'static LaneLinear),
    /// The luma colourised through this colour ([`colourise`]), then faded.
    Tint { color: [f32; 3], fade: f64 },
    /// Each pixel's distance from its luma scaled by this factor in 0..1.
    Desaturate(f32),
    /// Positive Vibrance, in linear light.
    Vibrance(Vibrance),
}

impl Pass for ColourPass {
    fn run(&self, band: &mut Band<'_>) {
        let px = band.pixels.iter_mut();
        match self {
            ColourPass::Warmify(m, lin) => by_lanes(band.pixels, |r, g, b| warmify_lanes(m, lin, r, g, b)),
            ColourPass::Vibrance(v) => by_lanes(band.pixels, |r, g, b| {
                v.lin.decode.read3(r, g, b);
                v.lanes(r, g, b);
                v.lin.encode.read3(r, g, b);
            }),
            ColourPass::Sepia(curves) => px.for_each(|p| *p = curves.at(fx::luma(unit3(*p)))),
            ColourPass::Grey(w) => px.for_each(|p| *p = [grey(*p, *w); 3]),
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
/// (the W3C *Compositing and Blending* `SetLum` and `ClipColor`, Rec.601 weights): Warmify's exact
/// maths, which [`warmify_lanes`] runs per lane.
#[cfg(test)]
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

// Ported from PhotoCraft `crates/color/src/convert.rs` at e4b2868a (MIT OR Apache-2.0).

/// The D50 white point (ICC PCS).
const D50: [f32; 3] = [0.964_22, 1.0, 0.825_21];

/// Linear sRGB → D50 XYZ (Bradford-adapted, as in the ICC sRGB profile). Rows sum to [`D50`].
const SRGB_TO_XYZ_D50: [[f32; 3]; 3] = [[0.436_074, 0.385_064, 0.143_080], [0.222_504, 0.716_878, 0.060_618], [0.013_932, 0.097_104, 0.714_173]];

/// D50 XYZ → linear sRGB (the inverse of [`SRGB_TO_XYZ_D50`]).
const XYZ_D50_TO_SRGB: [[f32; 3]; 3] = [[3.133_856, -1.616_867, -0.490_615], [-0.978_768, 1.916_142, 0.033_454], [0.071_945, -0.228_991, 1.405_243]];

/// Warmify on lanes of encoded pixels: the matrix `m` on their linear light, the result brought
/// into 0..1 and encoded, then each input's Rec.601 luma restored ([`set_luma`]'s maths per lane).
#[inline(always)]
fn warmify_lanes(m: &[[f32; 3]; 3], lin: &LaneLinear, r: &mut Lane, g: &mut Lane, b: &mut Lane) {
    let [wr, wg, wb] = REC601;
    let mut luma = [0.0f32; LANES];
    for k in 0..LANES {
        luma[k] = wr * clamp_unit(r[k]) + wg * clamp_unit(g[k]) + wb * clamp_unit(b[k]);
    }
    lin.decode.read3(r, g, b);
    for k in 0..LANES {
        let [x, y, z] = mat3_mul(m, [r[k], g[k], b[k]]);
        (r[k], g[k], b[k]) = (clamp_unit(x), clamp_unit(y), clamp_unit(z));
    }
    lin.encode.read3(r, g, b);
    for k in 0..LANES {
        // `set_luma`: moved to the luma, then brought into 0..1 toward it.
        let l = clamp_unit(luma[k]);
        let d = l - (wr * r[k] + wg * g[k] + wb * b[k]);
        let c = [r[k] + d, g[k] + d, b[k] + d];
        let lum = wr * c[0] + wg * c[1] + wb * c[2];
        let mn = fmin(fmin(c[0], c[1]), c[2]);
        let c = if mn < 0.0 && lum - mn > 1e-6 { c.map(|v| lum + (v - lum) * lum / (lum - mn)) } else { c };
        let mx = fmax(fmax(c[0], c[1]), c[2]);
        let c = if mx > 1.0 && mx - lum > 1e-6 { c.map(|v| lum + (v - lum) * (1.0 - lum) / (mx - lum)) } else { c };
        (r[k], g[k], b[k]) = (clamp_unit(c[0]), clamp_unit(c[1]), clamp_unit(c[2]));
    }
}

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

/// The pixels a lane pass works on at once: its maths runs on each channel of this many pixels
/// as one array (a structure of arrays), which the compiler turns into vector instructions, the
/// tables read value by value.
pub(crate) const LANES: usize = 16;

/// One channel of [`LANES`] pixels.
pub(crate) type Lane = [f32; LANES];

/// `pixels` [`LANES`] at a time through `f` as three channel lanes (the last few padded with
/// black, which `f` may change freely).
#[inline(always)]
pub(crate) fn by_lanes(pixels: &mut [[f32; 3]], f: impl Fn(&mut Lane, &mut Lane, &mut Lane)) {
    for chunk in pixels.chunks_mut(LANES) {
        let (mut r, mut g, mut b) = ([0.0f32; LANES], [0.0f32; LANES], [0.0f32; LANES]);
        for (((p, r), g), b) in chunk.iter().zip(&mut r).zip(&mut g).zip(&mut b) {
            (*r, *g, *b) = (p[0], p[1], p[2]);
        }
        f(&mut r, &mut g, &mut b);
        for (((p, r), g), b) in chunk.iter_mut().zip(&r).zip(&g).zip(&b) {
            *p = [*r, *g, *b];
        }
    }
}

/// A function of 0..1 in [`STEPS`] steps, read [`LANES`] values at a time with linear
/// interpolation: each step holds its value and its rise to the next.
pub(crate) struct LaneTable {
    steps: Box<[[f32; 2]; STEPS]>,
}

impl LaneTable {
    /// `f` sampled over 0..1.
    pub(crate) fn new(f: impl Fn(f32) -> f32) -> LaneTable {
        let mut steps = Box::new([[0.0f32; 2]; STEPS]);
        let mut a = f(0.0);
        for (i, step) in steps.iter_mut().enumerate() {
            let b = f((i + 1) as f32 / STEPS as f32);
            *step = [a, b - a];
            a = b;
        }
        LaneTable { steps }
    }

    /// Every value of `x` (brought into 0..1, `NaN` as 0) through the function.
    #[inline(always)]
    pub(crate) fn read(&self, x: &mut Lane) {
        let (mut at, mut t) = ([0i32; LANES], [0.0f32; LANES]);
        for k in 0..LANES {
            let pos = clamp_unit(x[k]) * STEPS as f32;
            // The step below `pos`: `pos − ½` rounded to a whole number by adding `ROUNDER`, read
            // from the low bits (an exact whole `pos` may land on the step below, at its end).
            let i = (((pos - 0.5 + ROUNDER).to_bits() & 0x1fff) as i32).min(STEPS as i32 - 1);
            (at[k], t[k]) = (i, pos - i as f32);
        }
        for k in 0..LANES {
            let [a, rise] = self.steps[at[k] as usize % STEPS];
            x[k] = a + rise * t[k];
        }
    }

    /// [`LaneTable::read`] on three lanes.
    #[inline(always)]
    pub(crate) fn read3(&self, r: &mut Lane, g: &mut Lane, b: &mut Lane) {
        self.read(r);
        self.read(g);
        self.read(b);
    }
}

/// 1.5 · 2²³: a value within ±2²² added to it is rounded to a whole number (to even on a tie), the
/// sum's low bits holding that number.
const ROUNDER: f32 = 12_582_912.0;

/// The shared linear-light tables ([`fx::linear_light`]) as [`LaneTable`]s: the same samples,
/// read by lanes.
pub(crate) struct LaneLinear {
    pub(crate) decode: LaneTable,
    pub(crate) encode: LaneTable,
}

/// [`LaneLinear`], built once.
pub(crate) fn lane_linear() -> &'static LaneLinear {
    static LINEAR: OnceLock<LaneLinear> = OnceLock::new();
    LINEAR.get_or_init(|| {
        let lin = fx::linear_light();
        LaneLinear { decode: LaneTable::new(|v| lin.to_linear(v)), encode: LaneTable::new(|v| lin.to_srgb(v)) }
    })
}

/// The Taylor series of `2^f` about 0: `(ln 2)^n / n!` for `n` from 1 to 5.
const EXP2_TAYLOR: [f32; 5] = {
    const L: f32 = std::f32::consts::LN_2;
    [L, L * L / 2.0, L * L * L / 6.0, L * L * L * L / 24.0, L * L * L * L * L / 120.0]
};

/// `e^x` on every value of `x`, for `x` in about −80..80: `2^i · 2^f` with `i` the nearest whole
/// number to `x · log₂ e` and `2^f` (`|f| ≤ ½`) by its Taylor series to the fifth power (relative
/// error under 3·10⁻⁶).
#[inline(always)]
fn exp_lanes(x: &mut Lane) {
    for v in x.iter_mut() {
        let y = (*v * std::f32::consts::LOG2_E).clamp(-120.0, 120.0);
        let i = (y + 128.5) as i32 - 128;
        let f = y - i as f32;
        let [k1, k2, k3, k4, k5] = EXP2_TAYLOR;
        let p = 1.0 + f * (k1 + f * (k2 + f * (k3 + f * (k4 + f * k5))));
        *v = p * f32::from_bits(((i + 127) as u32) << 23);
    }
}

/// Positive Vibrance at one amount for a pass over every pixel: [`vibrance_lin`]'s maths on lanes,
/// with what does not change from pixel to pixel made once (the skin hues' amount, its `powf`),
/// the `t^p (1 − t)^q` term read from a table, `exp` by [`exp_lanes`] and the skin weight from the
/// channels' order instead of the hue in degrees. Within a few hundredths of a level of
/// [`vibrance_lin`].
pub(crate) struct Vibrance {
    /// The amount, in 0..1.
    v: f32,
    /// The amount on skin hues, `0.64 v^1.6`.
    skin: f32,
    /// `t^p (1 − t)^q` over the HSV saturation `t`.
    shape: &'static LaneTable,
    /// The linear-light tables, for the passes that convert.
    pub(crate) lin: &'static LaneLinear,
}

impl Vibrance {
    /// Vibrance by `v`, none unless `v` is above 0 (above 1 brought to 1).
    pub(crate) fn new(v: f32) -> Option<Vibrance> {
        if v.is_nan() || v <= 0.0 {
            return None;
        }
        static SHAPE: OnceLock<LaneTable> = OnceLock::new();
        let [_, _, p, q, ..] = VIBRANCE_BOOST;
        let shape = SHAPE.get_or_init(|| LaneTable::new(|t| t.powf(p) * (1.0 - t).powf(q)));
        let v = v.min(1.0);
        Some(Vibrance { v, skin: 0.64 * v.powf(1.6), shape, lin: lane_linear() })
    }

    /// Lanes of linear-light pixels (each channel in 0..1, as the decoding table gives them) with
    /// Vibrance.
    #[inline(always)]
    pub(crate) fn lanes(&self, r: &mut Lane, g: &mut Lane, b: &mut Lane) {
        let [c0, c1, _, _, r0, ra, rt] = VIBRANCE_BOOST;
        let (mut t, mut a, mut e) = ([0.0f32; LANES], [0.0f32; LANES], [0.0f32; LANES]);
        for k in 0..LANES {
            let (cr, cg, cb) = (r[k], g[k], b[k]);
            let mx = fmax(fmax(cr, cg), cb);
            let d = mx - fmin(fmin(cr, cg), cb);
            t[k] = if mx > 0.0 { d / mx } else { 0.0 };
            // The skin weight (`vibrance_skin` of the HSV hue): only where red is the largest
            // channel does the hue lie within 0–60° (green over blue, `x` its sixth of the circle)
            // or 300–360°.
            let x = if d > 1e-9 { (cg - cb) / d } else { 0.0 };
            let w = if x >= 0.0 { clamp_unit(3.0 - 4.0 * x) } else { fmax(1.0 + x, 0.0) };
            a[k] = self.v + (self.skin - self.v) * if mx == cr { w } else { 0.0 };
            e[k] = (r0 + ra * a[k] + rt * (1.0 - t[k])) * (1.0 - mx);
        }
        exp_lanes(&mut e);
        let mut shape = t;
        self.shape.read(&mut shape);
        for k in 0..LANES {
            let (cr, cg, cb) = (r[k], g[k], b[k]);
            let mx = fmax(fmax(cr, cg), cb);
            let mn = fmin(fmin(cr, cg), cb);
            let d = mx - mn;
            let (t, a) = (t[k], a[k]);
            let boost = (c0 * a + c1 * a * a) * shape[k] * e[k];
            let t2 = fmin(t * (1.0 + boost), 1.0);
            let v2 = mx + self.v / 4.0 * (t * (1.0 - t) * (2.0 - t + t * t) * mx * (1.0 - mx));
            let m2 = v2 * (1.0 - t2);
            // A grey (or black) pixel stays as it is.
            let (m2, k2, mn) = if d > 1e-9 { (m2, (v2 - m2) / d, mn) } else { (0.0, 1.0, 0.0) };
            (r[k], g[k], b[k]) = (clamp_unit(m2 + (cr - mn) * k2), clamp_unit(m2 + (cg - mn) * k2), clamp_unit(m2 + (cb - mn) * k2));
        }
    }
}

/// `v` brought into 0..1, `NaN` as 0 (as [`unit`], in two comparisons the compiler keeps
/// branch-free).
#[inline(always)]
fn clamp_unit(v: f32) -> f32 {
    let v = if v > 0.0 { v } else { 0.0 };
    if v < 1.0 { v } else { 1.0 }
}

/// The larger of `a` and `b` (`b` when either is `NaN`: one comparison).
#[inline(always)]
fn fmax(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// The smaller of `a` and `b` (`b` when either is `NaN`: one comparison).
#[inline(always)]
fn fmin(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
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
pub(crate) mod tests {
    use super::*;

    /// Every 8-bit grey, then a seeded million RGB inputs anywhere in 0..1 (between levels too, as
    /// a fused pass may hand them over).
    fn inputs() -> impl Iterator<Item = [f32; 3]> {
        let greys = (0..=255u8).map(|v| [f32::from(v) / 255.0; 3]);
        let mut state = 0x5eed_0038_u64;
        let mut next = move || {
            // SplitMix64.
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
        };
        greys.chain((0..1_000_000).map(move |_| [next(), next(), next()]))
    }

    /// `pass` gives every input within one level (of 255) of `exact`.
    pub(crate) fn assert_within_a_level(name: &str, pass: &dyn Pass, exact: impl Fn([f32; 3]) -> [f32; 3]) {
        let input: Vec<[f32; 3]> = inputs().collect();
        let mut pixels = input.clone();
        let width = pixels.len() as u32;
        pass.run(&mut Band { pixels: &mut pixels, width, rows: 1, x0: 0, y0: 0, before: None });
        let mut worst = (0.0f32, [0.0f32; 3]);
        for (p, fast) in input.iter().zip(&pixels) {
            let e = exact(*p);
            let d = (0..3).map(|c| (fast[c] - e[c]).abs()).fold(0.0, f32::max) * 255.0;
            if d > worst.0 {
                worst = (d, *p);
            }
        }
        assert!(worst.0 <= 1.0, "{name}: {:.3} levels from the exact function at {:?}", worst.0, worst.1);
    }

    /// The sRGB curves exactly, a value brought into 0..1 before encoding.
    pub(crate) fn exact_encode(v: f32) -> f32 {
        fx::to_srgb(unit(v))
    }

    #[test]
    fn warmify_and_saturation_are_within_a_level_of_their_exact_maths() {
        let m = photo_filter_matrix(WARMING.map(|v| f32::from(v) / 255.0), WARMING_DENSITY, true);
        assert_within_a_level("warmify()", &warmify_pass(), |p| {
            let input = unit3(p);
            set_luma(mat3_mul(&m, input.map(fx::to_linear)).map(exact_encode), fx::luma(input))
        });
        for a in [0.16, 0.5, 1.0] {
            let pass = saturation_pass(a).expect("a pass above 0");
            assert_within_a_level(&format!("saturation({a})"), &pass, |p| vibrance_lin(unit3(p).map(fx::to_linear), a).map(exact_encode));
        }
    }
}
