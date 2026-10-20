//! A decoded picture's Y′CbCr planes to 8-bit sRGB: the matrix and range its stream signals
//! (BT.601, BT.709, BT.2020, or none for GBR), any chroma subsampling, 8 to 16 bits a sample;
//! HDR pictures (PQ or HLG, or BT.2020 primaries) linearised, tone-mapped to SDR, moved to the
//! sRGB primaries and encoded as sRGB. Scaling happens here too: each output pixel averages the
//! luma of the block of source pixels it covers, so a small poster costs a small conversion.
//!
//! Provenance: written from the public text of ITU-T H.273 (the colour code points), ITU-R
//! BT.601, BT.709 and BT.2020 (the matrices and the BT.2020 → BT.709 primaries conversion), SMPTE
//! ST 2084 (PQ), ITU-R BT.2100 (HLG and its OOTF, the 203 cd/m² reference white of BT.2408) and
//! IEC 61966-2-1 (sRGB); the tone curve is the extended Reinhard operator on the largest
//! component. No code was consulted.

use crate::limits::Budget;
use crate::{Error, Result, Rgba8};

/// One plane's samples.
#[derive(Clone, Copy, Debug)]
pub enum Samples<'a> {
    U8(&'a [u8]),
    U16(&'a [u16]),
}

/// One plane of a picture: `width × height` samples from (`x0`, `y0`), rows `stride` apart.
#[derive(Clone, Copy, Debug)]
pub struct Plane<'a> {
    pub samples: Samples<'a>,
    pub stride: usize,
    pub x0: usize,
    pub y0: usize,
    pub width: usize,
    pub height: usize,
}

impl Plane<'_> {
    /// The sample at (`x`, `y`) of the visible area, 0 outside the data (never a panic).
    #[inline]
    fn at(&self, x: usize, y: usize) -> u32 {
        let i = (self.y0 + y).wrapping_mul(self.stride).wrapping_add(self.x0 + x);
        match self.samples {
            Samples::U8(d) => d.get(i).map_or(0, |&v| u32::from(v)),
            Samples::U16(d) => d.get(i).map_or(0, |&v| u32::from(v)),
        }
    }

    /// Whether every visible sample lies inside the data.
    fn fits(&self) -> bool {
        let len = match self.samples {
            Samples::U8(d) => d.len(),
            Samples::U16(d) => d.len(),
        };
        if self.width == 0 || self.height == 0 {
            return false;
        }
        let last = (self.y0 + self.height - 1).checked_mul(self.stride).and_then(|r| r.checked_add(self.x0 + self.width));
        last.is_some_and(|l| l <= len)
    }
}

/// What the stream says about its colours (ITU-T H.273 code points).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    pub full_range: bool,
}

impl Colour {
    /// Unspecified everywhere, limited range.
    pub const UNSPECIFIED: Colour = Colour { primaries: 2, transfer: 2, matrix: 2, full_range: false };

    /// Whether the picture is HDR (PQ or HLG) or wide-gamut (BT.2020 primaries).
    pub fn is_hdr(&self) -> bool {
        matches!(self.transfer, 16 | 18) || self.primaries == 9
    }
}

/// A decoded picture: its planes (chroma absent for monochrome), bit depth and colours.
#[derive(Clone, Copy, Debug)]
pub struct Picture<'a> {
    pub luma: Plane<'a>,
    pub chroma: Option<(Plane<'a>, Plane<'a>)>,
    pub bit_depth: u8,
    pub colour: Colour,
}

impl Picture<'_> {
    pub fn width(&self) -> u32 {
        u32::try_from(self.luma.width).unwrap_or(u32::MAX)
    }

    pub fn height(&self) -> u32 {
        u32::try_from(self.luma.height).unwrap_or(u32::MAX)
    }

    /// The mean luma, 0 (black) to 1 (white) on the signal's range, from a grid of at most
    /// 64 × 64 samples.
    pub fn mean_luma(&self) -> f32 {
        let (w, h) = (self.luma.width, self.luma.height);
        if w == 0 || h == 0 {
            return 0.0;
        }
        let (sx, sy) = ((w / 64).max(1), (h / 64).max(1));
        let (mut sum, mut n) = (0u64, 0u64);
        for y in (sy / 2..h).step_by(sy) {
            for x in (sx / 2..w).step_by(sx) {
                sum += u64::from(self.luma.at(x, y));
                n += 1;
            }
        }
        let mean = sum as f32 / n.max(1) as f32;
        let (black, span) = self.luma_range();
        ((mean - black) / span).clamp(0.0, 1.0)
    }

    /// The luma code of black and the span to white.
    fn luma_range(&self) -> (f32, f32) {
        let depth = i32::from(self.bit_depth.clamp(8, 16));
        if self.colour.full_range {
            (0.0, ((1u32 << depth) - 1) as f32)
        } else {
            let unit = (1u32 << (depth - 8)) as f32;
            (16.0 * unit, 219.0 * unit)
        }
    }
}

/// Kr and Kb of a matrix code point (`None`: GBR, no matrix), unspecified by the picture's height.
fn matrix(code: u8, height: u32) -> Option<(f32, f32)> {
    match code {
        0 => None,
        1 => Some((0.2126, 0.0722)),
        4 => Some((0.30, 0.11)),
        5 | 6 => Some((0.299, 0.114)),
        7 => Some((0.212, 0.087)),
        9 | 10 => Some((0.2627, 0.0593)),
        _ if height >= 720 => Some((0.2126, 0.0722)),
        _ => Some((0.299, 0.114)),
    }
}

/// Fixed-point fraction bits of the conversion tables.
const FRAC: i32 = 8;

/// Codes of the intermediate R′G′B′ signal (12 bits).
const TOP: i32 = (LUT - 1) as i32;

/// The tables that turn one picture format's codes into 8-bit sRGB, made once per format and
/// kept by the frame source: per code value, its contribution to R′, G′ and B′ in 12-bit fixed
/// point; then the signal to sRGB (SDR: a straight scale; HDR: `Hdr`).
#[derive(Default)]
pub struct Converter {
    key: Option<(Colour, u8, bool)>,
    y: Vec<i32>,
    rv: Vec<i32>,
    gu: Vec<i32>,
    gv: Vec<i32>,
    bu: Vec<i32>,
    gbr: bool,
    sdr: Vec<u8>,
    hdr: Option<Hdr>,
}

impl Converter {
    fn prepare(&mut self, pic: &Picture) {
        let depth = pic.bit_depth.clamp(8, 16);
        let tall = pic.height() >= 720;
        let key = (pic.colour, depth, tall);
        if self.key == Some(key) {
            return;
        }
        let codes = 1usize << depth;
        let max = (codes - 1) as f32;
        let unit = (1u32 << (depth - 8)) as f32;
        let (y_black, y_span, c_mid, c_span) = if pic.colour.full_range {
            (0.0, max, (1u32 << (depth - 1)) as f32, max)
        } else {
            (16.0 * unit, 219.0 * unit, 128.0 * unit, 224.0 * unit)
        };
        let fix = |v: f32| (v * TOP as f32 * (1 << FRAC) as f32).round() as i32;
        let luma = |c: usize| (c as f32 - y_black) / y_span;
        let chroma = |c: usize| (c as f32 - c_mid) / c_span;
        self.y = (0..codes).map(|c| fix(luma(c))).collect();
        match matrix(pic.colour.matrix, pic.height()) {
            None => {
                // GBR: luma carries G, Cb carries B and Cr carries R, all on the luma range.
                self.gbr = true;
                self.rv = (0..codes).map(|c| fix(luma(c))).collect();
                self.bu = self.rv.clone();
                self.gu = vec![0; codes];
                self.gv = vec![0; codes];
            }
            Some((kr, kb)) => {
                self.gbr = false;
                let kg = 1.0 - kr - kb;
                self.rv = (0..codes).map(|c| fix(2.0 * (1.0 - kr) * chroma(c))).collect();
                self.bu = (0..codes).map(|c| fix(2.0 * (1.0 - kb) * chroma(c))).collect();
                self.gu = (0..codes).map(|c| fix(2.0 * kb * (1.0 - kb) / kg * chroma(c))).collect();
                self.gv = (0..codes).map(|c| fix(2.0 * kr * (1.0 - kr) / kg * chroma(c))).collect();
            }
        }
        self.sdr = (0..LUT).map(|i| ((i as f32 / TOP as f32) * 255.0 + 0.5) as u8).collect();
        self.hdr = pic.colour.is_hdr().then(|| Hdr::new(pic.colour));
        self.key = Some(key);
    }

    /// The picture converted to 8-bit sRGB at `out_w × out_h` (at most its own size), each output
    /// pixel averaging the luma of the source block it covers. The output buffer is taken from
    /// `budget`.
    pub fn convert(&mut self, pic: &Picture, out_w: u32, out_h: u32, budget: &mut Budget) -> Result<Rgba8> {
        self.convert_on(pic, out_w, out_h, budget, 0)
    }

    /// As [`Converter::convert`], with bands of rows on `threads` scoped threads (0 or 1: the
    /// caller's); the same bytes for any count.
    pub fn convert_on(&mut self, pic: &Picture, out_w: u32, out_h: u32, budget: &mut Budget, threads: usize) -> Result<Rgba8> {
        if !pic.luma.fits() || pic.chroma.as_ref().is_some_and(|(cb, cr)| !cb.fits() || !cr.fits()) {
            return Err(Error::Malformed("a decoded picture smaller than its size".into()));
        }
        self.prepare(pic);
        let (ow, oh) = (out_w.clamp(1, pic.width()) as usize, out_h.clamp(1, pic.height()) as usize);
        let len = ow.checked_mul(oh).and_then(|n| n.checked_mul(4)).ok_or(Error::TooLarge { width: out_w, height: out_h })?;
        let mut data = budget.vec(len, 255u8)?;
        let chroma = pic.chroma.as_ref();
        let this = &*self;
        let run = |first: usize, band: &mut [u8]| match (pic.luma.samples, chroma.map(|(b, r)| (b.samples, r.samples))) {
            (Samples::U8(l), None) => this.rows(pic, l, None, ow, oh, first, band),
            (Samples::U8(l), Some((Samples::U8(b), Samples::U8(r)))) => this.rows(pic, l, Some((b, r)), ow, oh, first, band),
            (Samples::U16(l), None) => this.rows(pic, l, None, ow, oh, first, band),
            (Samples::U16(l), Some((Samples::U16(b), Samples::U16(r)))) => this.rows(pic, l, Some((b, r)), ow, oh, first, band),
            _ => {}
        };
        let mixed = !matches!(
            (pic.luma.samples, chroma.map(|(b, r)| (b.samples, r.samples))),
            (_, None) | (Samples::U8(_), Some((Samples::U8(_), Samples::U8(_)))) | (Samples::U16(_), Some((Samples::U16(_), Samples::U16(_))))
        );
        if mixed {
            return Err(Error::Malformed("a decoded picture mixing sample widths".into()));
        }
        let bands = threads.clamp(1, 16).min(oh);
        if bands <= 1 {
            run(0, &mut data);
        } else {
            let rows_per = oh.div_ceil(bands);
            // Bands in a queue drained by the caller and by the helpers the system gives (a thread
            // it refuses leaves its share to the others).
            let queue = std::sync::Mutex::new(data.chunks_mut(rows_per * ow * 4).enumerate().collect::<Vec<_>>());
            let drain = || {
                loop {
                    let next = match queue.lock() {
                        Ok(mut q) => q.pop(),
                        Err(poisoned) => poisoned.into_inner().pop(),
                    };
                    let Some((i, band)) = next else { break };
                    run(i * rows_per, band);
                }
            };
            std::thread::scope(|scope| {
                for _ in 1..bands {
                    let _ = std::thread::Builder::new().spawn_scoped(scope, drain);
                }
                drain();
            });
        }
        Ok(Rgba8 { width: ow as u32, height: oh as u32, data })
    }

    /// One pixel's sRGB from its luma and chroma codes.
    #[inline(always)]
    fn pixel(&self, yc: usize, u: usize, v: usize) -> [u8; 3] {
        let at = |t: &[i32], i: usize| t.get(i).copied().unwrap_or(0);
        let yv = at(&self.y, yc);
        let (r, g, b) = if self.gbr {
            (at(&self.rv, v), yv, at(&self.bu, u))
        } else {
            (yv + at(&self.rv, v), yv - at(&self.gu, u) - at(&self.gv, v), yv + at(&self.bu, u))
        };
        let q = |x: i32| ((x + (1 << (FRAC - 1))) >> FRAC).clamp(0, TOP) as usize;
        let rgb = [q(r), q(g), q(b)];
        match &self.hdr {
            Some(hdr) => hdr.map(rgb),
            None => rgb.map(|c| self.sdr.get(c).copied().unwrap_or(255)),
        }
    }

    /// Output rows from `first` on, as many as `out` holds.
    #[allow(clippy::too_many_arguments)]
    fn rows<T: Copy + Into<u32>>(&self, pic: &Picture, luma: &[T], chroma: Option<(&[T], &[T])>, ow: usize, oh: usize, first: usize, out: &mut [u8]) {
        let (w, h) = (pic.luma.width, pic.luma.height);
        let lp = &pic.luma;
        let top = (self.y.len() - 1) as u32;
        let code = |v: T| v.into().min(top) as usize;
        let span = |o: usize, n: usize, outn: usize| {
            let a = o * n / outn;
            (a, ((o + 1) * n / outn).max(a + 1).min(n))
        };
        let cols: Vec<(usize, usize)> = (0..ow).map(|x| span(x, w, ow)).collect();
        let cw = pic.chroma.as_ref().map_or(1, |(p, _)| p.width);
        // The chroma column of each output column (the centre of the block it covers).
        let cxs: Vec<usize> = cols.iter().map(|&(x0, x1)| ((x0 + x1 - 1) / 2) * cw / w.max(1)).collect();
        let mid = self.y.len() / 2;
        let full = ow == w && oh == h;
        for (oy, out_row) in out.chunks_exact_mut(ow * 4).enumerate().map(|(i, r)| (first + i, r)).take_while(|(oy, _)| *oy < oh) {
            let (y0, y1) = span(oy, h, oh);
            let (cb_row, cr_row) = match (chroma, &pic.chroma) {
                (Some((b, r)), Some((bp, rp))) => {
                    let cy = ((y0 + y1 - 1) / 2) * bp.height / h.max(1);
                    (row(bp, b, cy), row(rp, r, cy))
                }
                _ => (&[][..], &[][..]),
            };
            let chroma_at = |cx: usize| (cb_row.get(cx).map_or(mid, |&c| code(c)), cr_row.get(cx).map_or(mid, |&c| code(c)));
            let pixels = out_row.as_chunks_mut::<4>().0.iter_mut();
            if full {
                let lrow = row(lp, luma, y0);
                for ((px, &l), &cx) in pixels.zip(lrow.iter()).zip(cxs.iter()) {
                    let (u, v) = chroma_at(cx);
                    let [r, g, b] = self.pixel(code(l), u, v);
                    *px = [r, g, b, 255];
                }
                continue;
            }
            let rows: Vec<&[T]> = (y0..y1).map(|y| row(lp, luma, y)).collect();
            for ((px, &(x0, x1)), &cx) in pixels.zip(cols.iter()).zip(cxs.iter()) {
                let mut sum = 0u32;
                for r in &rows {
                    for &v in r.get(x0..x1).unwrap_or_default() {
                        sum += v.into();
                    }
                }
                let n = ((x1 - x0) * rows.len()).max(1) as u32;
                let (u, v) = chroma_at(cx);
                let [r, g, b] = self.pixel((sum / n).min(top) as usize, u, v);
                *px = [r, g, b, 255];
            }
        }
    }
}

/// Row `y` of plane `p`'s visible area in `data` (empty outside it).
fn row<'d, T>(p: &Plane, data: &'d [T], y: usize) -> &'d [T] {
    let at = (p.y0 + y) * p.stride + p.x0;
    data.get(at..at + p.width).unwrap_or_default()
}

/// The picture converted with a converter of its own (see [`Converter::convert`]).
pub fn to_rgba(pic: &Picture, out_w: u32, out_h: u32, budget: &mut Budget) -> Result<Rgba8> {
    Converter::default().convert(pic, out_w, out_h, budget)
}

/// Entries of the linearising and encoding tables.
const LUT: usize = 4096;

/// SDR reference white in cd/m² (ITU-R BT.2408).
const REFERENCE_WHITE: f32 = 203.0;

/// The display peak assumed for HDR pictures, in cd/m² (HLG's nominal peak; PQ pictures are
/// mastered for about this on phones).
const PEAK: f32 = 1000.0;

/// The HDR (or wide-gamut) path: signal → linear light → tone curve → BT.709 primaries → sRGB.
struct Hdr {
    /// Signal codes (12 bits) to linear light (relative to reference white; HLG: scene light).
    linear: Vec<f32>,
    /// HLG's OOTF gain for a scene luminance 0..1 (in 4,096 steps).
    ootf: Vec<f32>,
    /// Linear 0..1 to sRGB codes.
    encode: Vec<u8>,
    wide: bool,
    tone: bool,
    hlg: bool,
}

impl Hdr {
    fn new(c: Colour) -> Hdr {
        let hlg = c.transfer == 18;
        let pq = c.transfer == 16;
        let linear = (0..LUT)
            .map(|i| {
                let e = i as f32 / (LUT - 1) as f32;
                if pq {
                    pq_eotf(e) / REFERENCE_WHITE
                } else if hlg {
                    hlg_inverse_oetf(e)
                } else {
                    // An SDR transfer: BT.1886's gamma 2.4.
                    e.powf(2.4)
                }
            })
            .collect();
        let encode = (0..LUT).map(|i| (srgb_oetf(i as f32 / (LUT - 1) as f32) * 255.0 + 0.5) as u8).collect();
        // BT.2100's OOTF for a 1000 cd/m² display (γ = 1.2), relative to reference white.
        let ootf = (0..LUT).map(|i| PEAK * (i as f32 / (LUT - 1) as f32).max(1e-6).powf(0.2) / REFERENCE_WHITE).collect();
        Hdr { linear, ootf, encode, wide: c.primaries == 9, tone: pq || hlg, hlg }
    }

    #[inline(always)]
    fn map(&self, [r, g, b]: [usize; 3]) -> [u8; 3] {
        let top = LUT - 1;
        let scale = top as f32;
        let lin = |c: usize| self.linear.get(c).copied().unwrap_or(0.0);
        let (mut r, mut g, mut b) = (lin(r), lin(g), lin(b));
        if self.hlg {
            let ys = 0.2627 * r + 0.6780 * g + 0.0593 * b;
            // `as` saturates: below 0 (and NaN) is 0.
            let gain = self.ootf.get(((ys * scale + 0.5) as usize).min(top)).copied().unwrap_or(0.0);
            (r, g, b) = (r * gain, g * gain, b * gain);
        }
        if self.wide {
            (r, g, b) = (1.6605 * r - 0.5876 * g - 0.0728 * b, -0.1246 * r + 1.1329 * g - 0.0083 * b, -0.0182 * r - 0.1006 * g + 1.1187 * b);
        }
        let (r, g, b) = (r.max(0.0), g.max(0.0), b.max(0.0));
        let k = if self.tone {
            // Extended Reinhard on the largest component, so hues keep their ratios.
            let m = r.max(g).max(b);
            let white = PEAK / REFERENCE_WHITE;
            if m > 1e-6 { (1.0 + m / (white * white)) / (1.0 + m) } else { 0.0 }
        } else {
            1.0
        };
        let enc = |v: f32| self.encode.get(((v * k * scale + 0.5) as usize).min(top)).copied().unwrap_or(255);
        [enc(r), enc(g), enc(b)]
    }
}

/// SMPTE ST 2084: signal 0..1 to cd/m².
fn pq_eotf(e: f32) -> f32 {
    let (m1, m2, c1, c2, c3) = (0.159_301_76_f32, 78.843_75_f32, 0.835_937_5_f32, 18.851_563_f32, 18.6875_f32);
    let p = e.max(0.0).powf(1.0 / m2);
    let num = (p - c1).max(0.0);
    let den = (c2 - c3 * p).max(1e-6);
    10_000.0 * (num / den).powf(1.0 / m1)
}

/// ITU-R BT.2100 HLG: signal 0..1 to scene light 0..1.
fn hlg_inverse_oetf(e: f32) -> f32 {
    let (a, b, c) = (0.178_832_77_f32, 0.284_668_92_f32, 0.559_910_7_f32);
    if e <= 0.5 { e * e / 3.0 } else { (((e - c) / a).exp() + b) / 12.0 }
}

/// IEC 61966-2-1: linear 0..1 to the sRGB signal.
fn srgb_oetf(v: f32) -> f32 {
    if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;

    fn plane(data: &[u8], w: usize, h: usize) -> Plane<'_> {
        Plane { samples: Samples::U8(data), stride: w, x0: 0, y0: 0, width: w, height: h }
    }

    #[test]
    fn limited_range_greys_and_primaries() {
        let mut budget = Budget::new(&Limits::default(), 2, 2);
        let colour = Colour { primaries: 1, transfer: 1, matrix: 1, full_range: false };
        let px = |y: u8, cb: u8, cr: u8, budget: &mut Budget| {
            let (l, c1, c2) = ([y; 4], [cb], [cr]);
            let pic = Picture { luma: plane(&l, 2, 2), chroma: Some((plane(&c1, 1, 1), plane(&c2, 1, 1))), bit_depth: 8, colour };
            let rgba = to_rgba(&pic, 2, 2, budget).unwrap();
            [rgba.data[0], rgba.data[1], rgba.data[2]]
        };
        assert_eq!(px(16, 128, 128, &mut budget), [0, 0, 0]);
        assert_eq!(px(235, 128, 128, &mut budget), [255, 255, 255]);
        // BT.709 red: Y 63, Cb 102, Cr 240.
        let red = px(63, 102, 240, &mut budget);
        assert!(red[0] > 250 && red[1] < 5 && red[2] < 5, "{red:?}");
    }

    #[test]
    fn scaling_averages_the_covered_luma_and_hdr_stays_in_range() {
        let mut budget = Budget::new(&Limits::default(), 4, 4);
        let l = [16u8, 235, 16, 235, 16, 235, 16, 235, 16, 235, 16, 235, 16, 235, 16, 235];
        let pic = Picture { luma: plane(&l, 4, 4), chroma: None, bit_depth: 8, colour: Colour::UNSPECIFIED };
        let small = to_rgba(&pic, 2, 2, &mut budget).unwrap();
        assert_eq!((small.width, small.height), (2, 2));
        assert!((120..=135).contains(&small.data[0]), "{}", small.data[0]);
        assert!((pic.mean_luma() - 0.5).abs() < 0.01);
        // HLG white (10-bit full scale) maps to white, black to black.
        let hlg = Colour { primaries: 9, transfer: 18, matrix: 9, full_range: false };
        let w = [940u16; 4];
        let c = [512u16];
        fn p(d: &[u16]) -> Plane<'_> {
            Plane { samples: Samples::U16(d), stride: 2, x0: 0, y0: 0, width: 2, height: 2 }
        }
        fn cp(d: &[u16]) -> Plane<'_> {
            Plane { samples: Samples::U16(d), stride: 1, x0: 0, y0: 0, width: 1, height: 1 }
        }
        let pic = Picture { luma: p(&w), chroma: Some((cp(&c), cp(&c))), bit_depth: 10, colour: hlg };
        let out = to_rgba(&pic, 2, 2, &mut budget).unwrap();
        assert!(out.data[0] > 240, "{:?}", &out.data[..4]);
        // Bands on threads: the same bytes.
        let big: Vec<u8> = (0..64 * 48).map(|i| (i * 7 % 220 + 16) as u8).collect();
        let c: Vec<u8> = (0..32 * 24).map(|i| (i * 3 % 200 + 20) as u8).collect();
        let pic =
            Picture { luma: plane(&big, 64, 48), chroma: Some((plane(&c, 32, 24), plane(&c, 32, 24))), bit_depth: 8, colour: Colour::UNSPECIFIED };
        let mut conv = Converter::default();
        let one = conv.convert(&pic, 50, 37, &mut budget).unwrap();
        for threads in [2, 3, 8, 40] {
            assert_eq!(conv.convert_on(&pic, 50, 37, &mut budget, threads).unwrap(), one, "{threads}");
        }
        // A plane that claims more than its data holds is refused.
        let short = [16u8; 3];
        let bad = Picture { luma: plane(&short, 2, 2), chroma: None, bit_depth: 8, colour: Colour::UNSPECIFIED };
        assert!(matches!(to_rgba(&bad, 2, 2, &mut budget), Err(Error::Malformed(_))));
    }
}
