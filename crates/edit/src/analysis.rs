//! Picture analysis shared by the operations that look at the picture before changing it (Auto
//! Fix, Auto Contrast, Auto Color and the Tuning wands, `editing.md` §6.3 and §7).
//!
//! They analyse a **fixed-size proxy**: the picture at the operation's place in the pipeline (after
//! the crop, the straighten and the operations applied before it, before the recipe's rotations
//! and flips), rendered so that its longer side is [`PROXY_EDGE`] pixels (smaller pictures as they
//! are). The renderer makes it ([`crate::ops::Prepare::proxy`]) the same way for a 128-pixel
//! thumbnail, the edit view and a full-size Save, so all three reach the same result. The wands
//! are given the photo's current render at whatever size it is and reduce it to the proxy size
//! first ([`proxy`]).
//!
//! Our own methods, on the sRGB-encoded values (0..1) of the pixels that are not fully
//! transparent:
//!
//! - [`levels`] (Auto Contrast): one black point, white point and midtone gamma for all three
//!   channels (linked, so the colours keep their balance). The black point is the darkest of the
//!   channels' [`CLIP`] percentiles and the white point the brightest of their `1 − CLIP`
//!   percentiles (each half an 8-bit level further out, so the pixels at the percentile keep
//!   their detail), so at most `CLIP` of the pixels clip at each end in each channel; a range
//!   narrower than [`MIN_RANGE`] is widened about its centre (a flat picture is not stretched into
//!   noise); the gamma moves the median luma half way towards the middle, within
//!   [`GAMMA_RANGE`].
//! - [`balance`] (Auto Color, and the Auto color wand): per-channel gains, the geometric mean of a
//!   grey-world estimate (the mid-tone, unclipped pixels average to grey) and a white-patch
//!   estimate (their bright percentile is white), normalised to keep the luma and each clamped to
//!   `1 / MAX_GAIN … MAX_GAIN`, so a photo that is mostly one colour (a sunset, a lawn) is
//!   corrected only so far.
//! - [`fill_amount`] (Auto Fix's little fill light) and [`auto_lighting`] (the lighting wand):
//!   from the luma's percentiles.
//! - [`color_at`] (the Neutral Color Picker): the mean colour of a small square under a point.

use dalica_image::Rgba8;

use crate::frame::Point;
use crate::ops::tone::{self, luma};

/// The longer side of the analysis proxy, in pixels.
pub const PROXY_EDGE: u32 = 256;

/// The share of pixels Auto Contrast lets clip at each end, per channel.
pub const CLIP: f32 = 0.005;
/// Half of one 8-bit level.
const HALF_LEVEL: f32 = 0.5 / 255.0;
/// The narrowest range Auto Contrast stretches to the full range (at most 4× contrast).
pub const MIN_RANGE: f32 = 0.25;
/// How far the midtone gamma moves the median luma towards the middle (0: not at all, 1: onto it).
const MID_PULL: f32 = 0.5;
/// The midtone gamma's bounds.
pub const GAMMA_RANGE: (f32, f32) = (0.8, 1.25);
/// The largest gain (and, inverted, the smallest) Auto Color gives a channel.
pub const MAX_GAIN: f32 = 1.35;
/// The percentile the white-patch estimate takes as white.
const WHITE_PATCH: f32 = 0.99;
/// The luma of the pixels the colour balance reads (outside: too dark or too bright to say).
const BALANCE_LUMA: (f32, f32) = (0.06, 0.94);
/// A channel at or above this is clipped and says nothing about the colour.
const CLIPPED: f32 = 0.98;
/// The fewest pixels the colour balance reads, as a share of the picture (and at least 16).
const BALANCE_MIN_SHARE: f32 = 0.01;
/// Auto Fix's fill light: `(FILL_FROM − median luma) · FILL_SLOPE`, at most `FILL_MAX`.
const FILL_FROM: f32 = 0.45;
const FILL_SLOPE: f32 = 0.8;
const FILL_MAX: f32 = 0.2;
/// The Neutral Color Picker's square reaches this share of the shorter side each way from the
/// point (at least one pixel).
pub const PICK_RADIUS: f64 = 0.01;
/// The brightest channel of the colour the Auto color wand picks.
const NEUTRAL_TOP: f32 = 230.0;

/// Whether `img` holds exactly `width × height` RGBA pixels.
fn well_formed(img: &Rgba8) -> bool {
    (u64::from(img.width) * u64::from(img.height)).checked_mul(4) == Some(img.data.len() as u64)
}

/// The pixels of `img` that are not fully transparent, as sRGB-encoded 0..1 values.
pub fn pixels(img: &Rgba8) -> Vec<[f32; 3]> {
    img.data.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).map(|p| [p[0], p[1], p[2]].map(|v| f32::from(v) / 255.0)).collect()
}

/// `img` reduced so its longer side is at most [`PROXY_EDGE`], by averaging whole blocks of
/// pixels (alpha included); a picture that fits is returned as it is. A buffer that does not hold
/// `width × height` pixels gives an empty picture.
pub fn proxy(img: &Rgba8) -> Rgba8 {
    if !well_formed(img) {
        return Rgba8 { width: 0, height: 0, data: Vec::new() };
    }
    let long = img.width.max(img.height);
    if long <= PROXY_EDGE {
        return img.clone();
    }
    let f = long.div_ceil(PROXY_EDGE);
    let (w, h) = (img.width.div_ceil(f), img.height.div_ceil(f));
    let stride = img.width as usize;
    let mut data = Vec::with_capacity(w as usize * h as usize * 4);
    for by in 0..h {
        for bx in 0..w {
            let mut sum = [0u64; 4];
            let mut n = 0u64;
            for y in by * f..by.saturating_add(1).saturating_mul(f).min(img.height) {
                let row = y as usize * stride;
                for x in bx * f..bx.saturating_add(1).saturating_mul(f).min(img.width) {
                    let at = (row + x as usize) * 4;
                    if let Some(p) = img.data.get(at..at + 4) {
                        for (s, v) in sum.iter_mut().zip(p) {
                            *s += u64::from(*v);
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            data.extend(sum.map(|s| ((s + n / 2) / n).min(255) as u8));
        }
    }
    Rgba8 { width: w, height: h, data }
}

/// The value below which a share `q` of the sorted `values` lie (nearest rank).
fn percentile(sorted: &[f32], q: f32) -> Option<f32> {
    let last = sorted.len().checked_sub(1)?;
    let i = ((q.clamp(0.0, 1.0) * last as f32).round() as usize).min(last);
    sorted.get(i).copied()
}

fn sorted(values: impl Iterator<Item = f32>) -> Vec<f32> {
    let mut v: Vec<f32> = values.filter(|v| v.is_finite()).collect();
    v.sort_unstable_by(f32::total_cmp);
    v
}

/// One set of levels for all three channels: `((v − black) / (white − black))^gamma`, clamped to
/// 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    pub black: f32,
    pub white: f32,
    pub gamma: f32,
}

impl Default for Levels {
    fn default() -> Levels {
        Levels::IDENTITY
    }
}

impl Levels {
    pub const IDENTITY: Levels = Levels { black: 0.0, white: 1.0, gamma: 1.0 };

    /// The levels at `v` (0..1).
    pub fn apply(&self, v: f32) -> f32 {
        let range = self.white - self.black;
        if !(range > 0.0 && range.is_finite()) {
            return v.clamp(0.0, 1.0);
        }
        let t = ((v - self.black) / range).clamp(0.0, 1.0);
        if self.gamma == 1.0 { t } else { t.powf(self.gamma) }
    }

    /// Whether the levels change nothing.
    pub fn is_identity(&self) -> bool {
        *self == Levels::IDENTITY
    }
}

/// Auto Contrast's levels for `px` (see the module's notes). No pixels: the identity.
pub fn levels(px: &[[f32; 3]]) -> Levels {
    let mut ends = Vec::with_capacity(3);
    for c in 0..3 {
        let s = sorted(px.iter().map(|p| p[c]));
        match (percentile(&s, CLIP), percentile(&s, 1.0 - CLIP)) {
            (Some(lo), Some(hi)) => ends.push((lo, hi)),
            _ => return Levels::IDENTITY,
        }
    }
    // Half an 8-bit level outside the percentiles, so the pixels at them keep a trace of detail
    // and only those beyond them clip.
    let mut black = (ends.iter().map(|e| e.0).fold(1.0f32, f32::min) - HALF_LEVEL).max(0.0);
    let mut white = (ends.iter().map(|e| e.1).fold(0.0f32, f32::max) + HALF_LEVEL).min(1.0);
    if white - black < MIN_RANGE {
        let centre = (black + white) / 2.0;
        black = (centre - MIN_RANGE / 2.0).clamp(0.0, 1.0 - MIN_RANGE);
        white = black + MIN_RANGE;
    }
    let linear = Levels { black, white, gamma: 1.0 };
    let lumas = sorted(px.iter().map(|p| luma(p.map(|v| linear.apply(v)))));
    let gamma = match percentile(&lumas, 0.5) {
        Some(m) if m > 0.02 && m < 0.98 => {
            let target = m + (0.5 - m) * MID_PULL;
            (target.ln() / m.ln()).clamp(GAMMA_RANGE.0, GAMMA_RANGE.1)
        }
        _ => 1.0,
    };
    Levels { black, white, gamma: if gamma.is_finite() { gamma } else { 1.0 } }
}

/// Whether a pixel's luma lies where the colour balance reads it.
fn mid_tone(p: &[f32; 3]) -> bool {
    let l = luma(*p);
    (BALANCE_LUMA.0..=BALANCE_LUMA.1).contains(&l)
}

/// The fewest pixels the colour balance reads from a picture of `n` pixels.
fn balance_needs(n: usize) -> usize {
    ((n as f32 * BALANCE_MIN_SHARE).ceil() as usize).max(16)
}

/// Auto Color's per-channel gains for `px` (see the module's notes); `[1, 1, 1]` when too few
/// pixels say anything about the colour.
pub fn balance(px: &[[f32; 3]]) -> [f32; 3] {
    let usable: Vec<[f32; 3]> = px.iter().copied().filter(|p| mid_tone(p) && p.iter().all(|v| *v < CLIPPED)).collect();
    if usable.len() < balance_needs(px.len()) {
        return [1.0; 3];
    }
    let n = usable.len() as f64;
    let mean: [f32; 3] = std::array::from_fn(|c| (usable.iter().map(|p| f64::from(p[c])).sum::<f64>() / n) as f32);
    let white: [f32; 3] = std::array::from_fn(|c| percentile(&sorted(usable.iter().map(|p| p[c])), WHITE_PATCH).unwrap_or(1.0));
    let to_grey = |c: [f32; 3]| -> Option<[f32; 3]> {
        let y = luma(c);
        (c.iter().all(|v| *v > 1e-3) && y > 1e-3).then(|| c.map(|v| y / v))
    };
    let (Some(gw), Some(wp)) = (to_grey(mean), to_grey(white)) else { return [1.0; 3] };
    let g: [f32; 3] = std::array::from_fn(|c| (gw[c] * wp[c]).sqrt());
    let y = luma(g);
    if !(y > 0.0 && y.is_finite()) {
        return [1.0; 3];
    }
    g.map(|v| (v / y).clamp(1.0 / MAX_GAIN, MAX_GAIN))
}

/// `px` with `gains`, clamped to 0..1.
pub fn apply_gains(px: &mut [[f32; 3]], gains: [f32; 3]) {
    for p in px {
        for (v, g) in p.iter_mut().zip(gains) {
            *v = (*v * g).clamp(0.0, 1.0);
        }
    }
}

/// `px` through `levels`.
pub fn apply_levels(px: &mut [[f32; 3]], levels: &Levels) {
    for p in px {
        *p = p.map(|v| levels.apply(v));
    }
}

/// Auto Fix's fill light for `px` (after its colour and contrast): a little, and only for a
/// picture whose median luma lies below 0.45.
pub fn fill_amount(px: &[[f32; 3]]) -> f64 {
    let lumas = sorted(px.iter().map(|p| luma(*p)));
    percentile(&lumas, 0.5).map_or(0.0, |m| f64::from(((FILL_FROM - m) * FILL_SLOPE).clamp(0.0, FILL_MAX)))
}

/// What the Tuning tab's Auto lighting wand sets (`editing.md` §7), each within its control's
/// range.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lighting {
    pub fill: f64,
    pub highlights: f64,
    pub shadows: f64,
}

/// `v` rounded to hundredths (so it prints short in the recipe) and kept within `range`.
fn hundredths(v: f32, range: (f64, f64)) -> f64 {
    let v = (f64::from(v) * 100.0).round() / 100.0;
    if v.is_finite() { v.clamp(range.0, range.1) + 0.0 } else { range.0 }
}

/// The most Fill Light the lighting wand sets.
const WAND_FILL_MAX: f64 = 0.6;

/// The Auto lighting wand for the picture `img` (the photo's current render, any size): Fill
/// Light for a dark picture (median luma under 0.45), Highlights for dull highlights (the 99th
/// percentile of the luma under 0.9) and Shadows for lifted blacks (the 1st percentile over
/// 0.06). A picture already spread over the whole range with its middle at the middle gets all
/// three at 0.
pub fn auto_lighting(img: &Rgba8) -> Lighting {
    let px = pixels(&proxy(img));
    let lumas = sorted(px.iter().map(|p| luma(*p)));
    let (Some(low), Some(mid), Some(high)) = (percentile(&lumas, 0.01), percentile(&lumas, 0.5), percentile(&lumas, 0.99)) else {
        return Lighting::default();
    };
    Lighting {
        fill: hundredths((0.45 - mid) * 1.5, (tone::FILL.0, WAND_FILL_MAX.min(tone::FILL.1))),
        highlights: hundredths(0.9 - high, tone::HIGHLIGHTS),
        shadows: hundredths((low - 0.06) * 1.5, tone::SHADOWS),
    }
}

/// The Auto color wand for the picture `img` (the photo's current render, any size): the colour
/// that, picked as Tuning's neutral colour, gives Auto Color's balance ([`balance`]), at a
/// brightness that keeps its proportions in 8 bits (its brightest channel 230). `None` when too
/// few pixels say anything about the colour.
pub fn auto_neutral(img: &Rgba8) -> Option<[u8; 3]> {
    let px = pixels(&proxy(img));
    let usable = px.iter().filter(|p| mid_tone(p) && p.iter().all(|v| *v < CLIPPED)).count();
    if usable < balance_needs(px.len()) {
        return None;
    }
    // Tuning makes the picked colour `c` grey with the gains `luma(c) / c`, so `c ∝ 1 / gains`.
    let inverse = balance(&px).map(|g| 1.0 / g);
    let top = inverse.iter().copied().fold(0.0f32, f32::max);
    if !(top > 0.0 && top.is_finite()) {
        return None;
    }
    Some(inverse.map(|v| (v / top * NEUTRAL_TOP).round().clamp(0.0, 255.0) as u8))
}

/// The mean colour of `img` (any size) in the square reaching [`PICK_RADIUS`] of the shorter side
/// around the normalised point `at` (`(0, 0)` top-left, `(1, 1)` bottom-right of `img`), for the
/// Neutral Color Picker. Fully transparent pixels are left out. `None` for a point outside the
/// picture or not finite, a buffer that does not hold `width × height` pixels, or when nothing
/// under the point is visible.
pub fn color_at(img: &Rgba8, at: Point) -> Option<[u8; 3]> {
    let inside = |v: f64| (0.0..=1.0).contains(&v);
    if !(inside(at.x) && inside(at.y)) || img.width == 0 || img.height == 0 || !well_formed(img) {
        return None;
    }
    let (w, h) = (f64::from(img.width), f64::from(img.height));
    let cx = ((at.x * w).floor() as u32).min(img.width - 1);
    let cy = ((at.y * h).floor() as u32).min(img.height - 1);
    let r = ((w.min(h) * PICK_RADIUS).round() as u32).max(1);
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    for y in cy.saturating_sub(r)..=cy.saturating_add(r).min(img.height - 1) {
        for x in cx.saturating_sub(r)..=cx.saturating_add(r).min(img.width - 1) {
            let i = (y as usize * img.width as usize + x as usize) * 4;
            if let Some(&[red, green, blue, alpha]) = img.data.get(i..i + 4)
                && alpha > 0
            {
                sum[0] += u64::from(red);
                sum[1] += u64::from(green);
                sum[2] += u64::from(blue);
                n += 1;
            }
        }
    }
    (n > 0).then(|| sum.map(|s| ((s + n / 2) / n).min(255) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_and_levels_of_edge_cases() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[0.25], 0.0), Some(0.25));
        assert_eq!(levels(&[]), Levels::IDENTITY);
        // A flat picture is not stretched beyond MIN_RANGE.
        let flat = levels(&[[0.5; 3]; 100]);
        assert!((flat.white - flat.black - MIN_RANGE).abs() < 1e-6);
        assert!((flat.apply(0.5) - 0.5).abs() < 1e-3);
        let black = levels(&[[0.0; 3]; 100]);
        assert_eq!((black.black, black.white), (0.0, MIN_RANGE));
        assert_eq!(balance(&[]), [1.0; 3]);
        assert_eq!(balance(&[[0.0; 3]; 100]), [1.0; 3]);
        for v in [0.0, 0.3, 1.0] {
            assert_eq!(Levels::IDENTITY.apply(v), v);
        }
        assert_eq!(Levels { black: 0.5, white: 0.5, gamma: 1.0 }.apply(0.7), 0.7);
    }

    #[test]
    fn the_proxy_averages_whole_blocks() {
        let img = Rgba8 { width: 512, height: 2, data: (0..1024).flat_map(|i| [(i % 2) as u8 * 200, 0, 0, 255]).collect() };
        let p = proxy(&img);
        assert_eq!((p.width, p.height), (256, 1));
        assert!(p.data.chunks(4).all(|q| q == [100, 0, 0, 255]), "{:?}", &p.data[..8]);
        let short = Rgba8 { width: 1000, height: 1000, data: vec![255; 16] };
        assert_eq!(proxy(&short).data.len(), 0);
        let huge = Rgba8 { width: u32::MAX, height: u32::MAX, data: Vec::new() };
        assert_eq!(proxy(&huge).width, 0);
        assert_eq!(color_at(&huge, Point { x: 0.5, y: 0.5 }), None);
    }
}
