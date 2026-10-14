//! A baseline JPEG encoder (ITU-T T.81 / JFIF): grey, YCbCr 4:4:4 and 4:2:0.
//!
//! Adapted from LightCraft (MIT OR Apache-2.0), `crates/codecs/src/jpeg_par.rs` at 25ef65c8.
//! Changes: scoped threads instead of `rayon` (no new dependency, and the caller's thread does
//! the work unless asked for more); colour conversion per MCU from the caller's pixels instead of
//! whole float planes (no allocation proportional to the image beyond the output); 8- or 16-bit
//! input; coefficients clamped to the baseline range; no indexing that can fail.
//!
//! The image is cut into bands of whole MCU rows separated by restart markers (`DRI` + `RSTn`).
//! DC prediction restarts at every marker, so bands are entropy-coded independently and joined.
//! The band layout depends only on the image size, so every thread count writes the same bytes.
//! Quantisation uses the T.81 Annex K example tables scaled by the usual 1–100 quality
//! convention; Huffman coding uses the Annex K example tables (interleaved baseline, readable by
//! every decoder).

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{EncodeError, Image, Pixels, Subsampling};

/// T.81 Annex K.1 / K.2 example quantization tables (natural order).
pub(crate) const Q_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56,
    68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
pub(crate) const Q_CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

/// Zig-zag position k → natural index (T.81 Figure A.6).
pub(crate) const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43,
    36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// T.81 Annex K.3 example Huffman tables: (code counts per length 1..16, symbols).
const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUMA_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08,
    0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59,
    0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6,
    0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91,
    0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58,
    0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4,
    0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa,
];

/// The largest quantised coefficient a baseline Huffman table codes (10-bit AC magnitudes; DC
/// differences then stay within 11 bits).
const COEFF_MAX: f32 = 1023.0;

/// Huffman code table: symbol → (code, length).
struct Huff {
    code: [(u16, u8); 256],
}

impl Huff {
    /// Canonical codes from counts (T.81 Annex C).
    fn new(bits: &[u8; 16], vals: &[u8]) -> Huff {
        let mut code = [(0u16, 0u8); 256];
        let (mut c, mut k) = (0u16, 0usize);
        for (len, &n) in (1u8..).zip(bits) {
            for _ in 0..n {
                if let Some(slot) = vals.get(k).and_then(|&v| code.get_mut(usize::from(v))) {
                    *slot = (c, len);
                }
                c = c.wrapping_add(1);
                k += 1;
            }
            c = c.wrapping_shl(1);
        }
        Huff { code }
    }

    #[inline]
    fn get(&self, symbol: u32) -> (u32, u32) {
        self.code.get(symbol as usize).map_or((0, 0), |&(c, l)| (u32::from(c), u32::from(l)))
    }
}

/// The quantisation table for `quality` (1–100): the Independent JPEG Group's scaling of `base`,
/// kept within 1..=255 for 8-bit tables.
pub(crate) fn scaled_quant(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    base.map(|b| ((u32::from(b) * scale + 50) / 100).clamp(1, 255) as u16)
}

/// Entropy-coded segment writer with 0xFF byte stuffing.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits { out: Vec::new(), acc: 0, n: 0 }
    }

    /// Appends the low `len` bits of `code` (`len` ≤ 16).
    #[inline]
    fn put(&mut self, code: u32, len: u32) {
        if len == 0 || len > 16 {
            return;
        }
        self.acc = (self.acc << len) | (code & ((1 << len) - 1));
        self.n += len;
        while self.n >= 8 {
            let b = (self.acc >> (self.n - 8)) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0);
            }
            self.n -= 8;
        }
        self.acc &= (1 << self.n) - 1;
    }

    /// Pads with 1-bits to a byte boundary.
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad);
        }
        self.out
    }
}

/// AAN output scale factors: `s[0] = 1`, `s[k] = cos(kπ/16)·√2`.
fn aan_scale() -> [f32; 8] {
    std::array::from_fn(|k| if k == 0 { 1.0 } else { ((k as f64 * std::f64::consts::PI / 16.0).cos() * std::f64::consts::SQRT_2) as f32 })
}

/// Quantization reciprocals with the AAN scaling folded in: coefficient = out / (8·s[u]·s[v]·q).
fn recip_table(q: &[u16; 64]) -> [f32; 64] {
    let s = aan_scale();
    std::array::from_fn(|i| 1.0 / (f32::from(q[i]) * s[i / 8] * s[i % 8] * 8.0))
}

/// One 8-point forward DCT (Arai–Agui–Nakajima flowgraph, 5 multiplies) on `d[o + k·stride]`.
#[inline(always)]
fn aan_1d(d: &mut [f32; 64], o: usize, stride: usize) {
    let g = |k: usize| (o + k * stride) & 63;
    let (t0, t7) = (d[g(0)] + d[g(7)], d[g(0)] - d[g(7)]);
    let (t1, t6) = (d[g(1)] + d[g(6)], d[g(1)] - d[g(6)]);
    let (t2, t5) = (d[g(2)] + d[g(5)], d[g(2)] - d[g(5)]);
    let (t3, t4) = (d[g(3)] + d[g(4)], d[g(3)] - d[g(4)]);
    // even part
    let (t10, t13, t11, t12) = (t0 + t3, t0 - t3, t1 + t2, t1 - t2);
    d[g(0)] = t10 + t11;
    d[g(4)] = t10 - t11;
    let z1 = (t12 + t13) * 0.707_106_77;
    d[g(2)] = t13 + z1;
    d[g(6)] = t13 - z1;
    // odd part
    let (t10, t11, t12) = (t4 + t5, t5 + t6, t6 + t7);
    let z5 = (t10 - t12) * 0.382_683_43;
    let z2 = 0.541_196_1 * t10 + z5;
    let z4 = 1.306_563 * t12 + z5;
    let z3 = t11 * 0.707_106_77;
    let (z11, z13) = (t7 + z3, t7 - z3);
    d[g(5)] = z13 + z2;
    d[g(3)] = z13 - z2;
    d[g(1)] = z11 + z4;
    d[g(7)] = z11 - z4;
}

/// Forward DCT of a level-shifted 8×8 block and quantization.
#[inline]
pub(crate) fn fdct_quant(block: &[f32; 64], recip: &[f32; 64], out: &mut [i16; 64]) {
    let mut d = *block;
    for r in 0..8 {
        aan_1d(&mut d, r * 8, 1);
    }
    for c in 0..8 {
        aan_1d(&mut d, c, 8);
    }
    for ((o, v), r) in out.iter_mut().zip(d).zip(recip) {
        *o = (v * r).round().clamp(-COEFF_MAX, COEFF_MAX) as i16;
    }
}

/// The magnitude category of `v` and its additional bits (T.81 F.1.2.1).
#[inline]
fn magnitude(v: i32) -> (u32, u32) {
    let a = v.unsigned_abs();
    let size = 32 - a.leading_zeros();
    let bits = if v < 0 { (v - 1) as u32 } else { v as u32 };
    (size, bits & ((1u32 << size) - 1))
}

fn encode_block(w: &mut Bits, q: &[i16; 64], pred: &mut i32, dc: &Huff, ac: &Huff) {
    let d = i32::from(q[0]) - *pred;
    *pred = i32::from(q[0]);
    let (s, b) = magnitude(d);
    let (c, l) = dc.get(s);
    w.put(c, l);
    w.put(b, s);
    let mut run = 0u32;
    for &zi in &ZIGZAG[1..] {
        let v = i32::from(q[zi & 63]);
        if v == 0 {
            run += 1;
            continue;
        }
        while run >= 16 {
            let (c, l) = ac.get(0xF0);
            w.put(c, l);
            run -= 16;
        }
        let (s, b) = magnitude(v);
        let (c, l) = ac.get((run << 4) | s);
        w.put(c, l);
        w.put(b, s);
        run = 0;
    }
    if run > 0 {
        let (c, l) = ac.get(0x00);
        w.put(c, l);
    }
}

/// A sample type the encoder reads, as a level on 0–255.
trait Level: Copy + Send + Sync {
    fn level(self) -> f32;
}

impl Level for u8 {
    #[inline]
    fn level(self) -> f32 {
        f32::from(self)
    }
}

impl Level for u16 {
    #[inline]
    fn level(self) -> f32 {
        f32::from(self) * (255.0 / 65535.0)
    }
}

/// The caller's interleaved pixels, read with edge replication past the right and bottom edges.
struct Source<'a, T> {
    data: &'a [T],
    w: usize,
    h: usize,
    stride: usize,
}

impl<T: Level> Source<'_, T> {
    #[inline]
    fn at(&self, x: usize, y: usize) -> usize {
        (y.min(self.h.saturating_sub(1)) * self.w + x.min(self.w.saturating_sub(1))) * self.stride
    }

    #[inline]
    fn grey(&self, x: usize, y: usize) -> f32 {
        self.data.get(self.at(x, y)).map_or(0.0, |v| v.level())
    }

    #[inline]
    fn rgb(&self, x: usize, y: usize) -> [f32; 3] {
        let i = self.at(x, y);
        match self.data.get(i..i + 3) {
            Some(&[r, g, b]) => [r.level(), g.level(), b.level()],
            _ => [0.0; 3],
        }
    }
}

/// JFIF YCbCr, level-shifted: Y − 128, Cb − 128, Cr − 128.
#[inline]
fn ycbcr([r, g, b]: [f32; 3]) -> [f32; 3] {
    [0.299 * r + 0.587 * g + 0.114 * b - 128.0, -0.168_736 * r - 0.331_264 * g + 0.5 * b, 0.5 * r - 0.418_688 * g - 0.081_312 * b]
}

/// Everything a band needs, shared by every thread.
struct Plan<'a, T> {
    src: Source<'a, T>,
    grey: bool,
    /// Luma blocks per MCU in each direction (1 or 2).
    hs: usize,
    vs: usize,
    /// MCUs across and down.
    mx: usize,
    my: usize,
    /// The chroma plane's size (4:2:0: half the image, rounded up).
    cw: usize,
    ch: usize,
    rows_per_band: usize,
    luma: [f32; 64],
    chroma: [f32; 64],
    dc_luma: Huff,
    ac_luma: Huff,
    dc_chroma: Huff,
    ac_chroma: Huff,
}

impl<T: Level> Plan<'_, T> {
    fn bands(&self) -> usize {
        self.my.div_ceil(self.rows_per_band)
    }

    /// The entropy-coded data of band `band` (its MCU rows), DC prediction starting at 0.
    fn encode_band(&self, band: usize) -> Vec<u8> {
        let mut w = Bits::new();
        let mut pred = [0i32; 3];
        let mut q = [0i16; 64];
        let first = band * self.rows_per_band;
        for mcu_y in first..(first + self.rows_per_band).min(self.my) {
            for mcu_x in 0..self.mx {
                let (x0, y0) = (mcu_x * 8 * self.hs, mcu_y * 8 * self.vs);
                if self.grey {
                    let blk: [f32; 64] = std::array::from_fn(|i| self.src.grey(x0 + i % 8, y0 + i / 8) - 128.0);
                    fdct_quant(&blk, &self.luma, &mut q);
                    encode_block(&mut w, &q, &mut pred[0], &self.dc_luma, &self.ac_luma);
                    continue;
                }
                if self.hs == 1 {
                    let (mut y, mut cb, mut cr) = ([0f32; 64], [0f32; 64], [0f32; 64]);
                    for i in 0..64 {
                        let [l, b, r] = ycbcr(self.src.rgb(x0 + i % 8, y0 + i / 8));
                        (y[i], cb[i], cr[i]) = (l, b, r);
                    }
                    fdct_quant(&y, &self.luma, &mut q);
                    encode_block(&mut w, &q, &mut pred[0], &self.dc_luma, &self.ac_luma);
                    for (blk, p) in [(cb, 1), (cr, 2)] {
                        fdct_quant(&blk, &self.chroma, &mut q);
                        encode_block(&mut w, &q, &mut pred[p], &self.dc_chroma, &self.ac_chroma);
                    }
                    continue;
                }
                // 4:2:0: four luma blocks, then one Cb and one Cr block from 2 × 2 RGB means (the
                // conversion is linear, so averaging first and converting once is the same).
                for by in 0..2 {
                    for bx in 0..2 {
                        let (bx0, by0) = (x0 + bx * 8, y0 + by * 8);
                        let blk: [f32; 64] = std::array::from_fn(|i| ycbcr(self.src.rgb(bx0 + i % 8, by0 + i / 8))[0]);
                        fdct_quant(&blk, &self.luma, &mut q);
                        encode_block(&mut w, &q, &mut pred[0], &self.dc_luma, &self.ac_luma);
                    }
                }
                let (mut cb, mut cr) = ([0f32; 64], [0f32; 64]);
                for i in 0..64 {
                    let cx = (mcu_x * 8 + i % 8).min(self.cw.saturating_sub(1));
                    let cy = (mcu_y * 8 + i / 8).min(self.ch.saturating_sub(1));
                    let (px, py) = (2 * cx, 2 * cy);
                    let mut s = [0f32; 3];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let p = self.src.rgb(px + dx, py + dy);
                        s = [s[0] + p[0], s[1] + p[1], s[2] + p[2]];
                    }
                    let [_, b, r] = ycbcr(s.map(|v| v * 0.25));
                    (cb[i], cr[i]) = (b, r);
                }
                for (blk, p) in [(cb, 1), (cr, 2)] {
                    fdct_quant(&blk, &self.chroma, &mut q);
                    encode_block(&mut w, &q, &mut pred[p], &self.dc_chroma, &self.ac_chroma);
                }
            }
        }
        w.finish()
    }

    /// Every band's data, in order, encoded on up to `threads` threads (this one included).
    fn encode_bands(&self, threads: usize) -> Result<Vec<Vec<u8>>, EncodeError> {
        let n = self.bands();
        let workers = threads.min(n).max(1);
        if workers == 1 {
            return Ok((0..n).map(|b| self.encode_band(b)).collect());
        }
        let next = AtomicUsize::new(0);
        let slots: Mutex<Vec<Option<Vec<u8>>>> = Mutex::new(vec![None; n]);
        let work = || {
            loop {
                let band = next.fetch_add(1, Ordering::Relaxed);
                if band >= n {
                    break;
                }
                let coded = self.encode_band(band);
                if let Ok(mut s) = slots.lock()
                    && let Some(slot) = s.get_mut(band)
                {
                    *slot = Some(coded);
                }
            }
        };
        let failed = std::thread::scope(|scope| {
            // A thread that cannot be started is no loss: the bands it would have taken are
            // taken by the others, this thread included.
            let handles: Vec<_> =
                (1..workers).filter_map(|i| std::thread::Builder::new().name(format!("jpeg-{i}")).spawn_scoped(scope, work).ok()).collect();
            work();
            handles.into_iter().map(|h| h.join().is_err()).fold(false, |a, b| a | b)
        });
        let slots = slots.into_inner().map_err(|_| EncodeError::Thread)?;
        if failed {
            return Err(EncodeError::Thread);
        }
        slots.into_iter().map(|s| s.ok_or(EncodeError::Thread)).collect()
    }
}

fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xFF, marker]);
    // Payloads are at most 65,533 bytes (checked by the APP segment writer; the rest are fixed).
    out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap_or(u16::MAX).to_be_bytes());
    out.extend_from_slice(payload);
}

fn dht(out: &mut Vec<u8>, class_id: u8, bits: &[u8; 16], vals: &[u8]) {
    let mut p = vec![class_id];
    p.extend_from_slice(bits);
    p.extend_from_slice(vals);
    segment(out, 0xC4, &p);
}

/// Encodes a checked `image` (size within 1..=65535 per side, exact sample count, quality 1..=100).
pub(super) fn encode(image: &Image<'_>, quality: u8, sub: Subsampling, app: &[(u8, Vec<u8>)], threads: usize) -> Result<Vec<u8>, EncodeError> {
    match image.pixels {
        Pixels::U8(data) => encode_from(image, data, quality, sub, app, threads),
        Pixels::U16(data) => encode_from(image, data, quality, sub, app, threads),
    }
}

fn encode_from<T: Level>(
    image: &Image<'_>,
    data: &[T],
    quality: u8,
    sub: Subsampling,
    app: &[(u8, Vec<u8>)],
    threads: usize,
) -> Result<Vec<u8>, EncodeError> {
    let (width, height) = (image.width as usize, image.height as usize);
    let grey = image.channels.is_grey();
    let (hs, vs) = if !grey && sub == Subsampling::S420 { (2, 2) } else { (1, 1) };
    let (mcu_w, mcu_h) = (8 * hs, 8 * vs);
    let (mx, my) = (width.div_ceil(mcu_w), height.div_ceil(mcu_h));
    // Restart interval: whole MCU rows per band, ≤ 65,535 MCUs, about 64 bands for large images.
    let rows_per_band = (my / 64).clamp(1, (65_535 / mx.max(1)).max(1));
    let ql = scaled_quant(&Q_LUMA, quality);
    let qc = scaled_quant(&Q_CHROMA, quality);
    let plan = Plan {
        src: Source { data, w: width, h: height, stride: image.channels.count() },
        grey,
        hs,
        vs,
        mx,
        my,
        cw: width.div_ceil(hs),
        ch: height.div_ceil(vs),
        rows_per_band,
        luma: recip_table(&ql),
        chroma: recip_table(&qc),
        dc_luma: Huff::new(&DC_LUMA_BITS, &DC_VALS),
        ac_luma: Huff::new(&AC_LUMA_BITS, &AC_LUMA_VALS),
        dc_chroma: Huff::new(&DC_CHROMA_BITS, &DC_VALS),
        ac_chroma: Huff::new(&AC_CHROMA_BITS, &AC_CHROMA_VALS),
    };
    let coded = plan.encode_bands(threads)?;

    let mut out = Vec::with_capacity(coded.iter().map(Vec::len).sum::<usize>() + app.iter().map(|(_, p)| p.len() + 4).sum::<usize>() + 1024);
    out.extend_from_slice(&[0xFF, 0xD8]);
    for (marker, payload) in app {
        segment(&mut out, *marker, payload);
    }
    let dqt = |id: u8, q: &[u16; 64]| -> Vec<u8> {
        let mut p = vec![id];
        p.extend(ZIGZAG.iter().map(|&i| q.get(i).map_or(1, |&v| v as u8)));
        p
    };
    segment(&mut out, 0xDB, &dqt(0, &ql));
    if !grey {
        segment(&mut out, 0xDB, &dqt(1, &qc));
    }
    let mut sof = vec![8];
    sof.extend_from_slice(&(image.height as u16).to_be_bytes());
    sof.extend_from_slice(&(image.width as u16).to_be_bytes());
    if grey {
        sof.extend_from_slice(&[1, 1, 0x11, 0]);
    } else {
        sof.extend_from_slice(&[3, 1, ((hs as u8) << 4) | vs as u8, 0, 2, 0x11, 1, 3, 0x11, 1]);
    }
    segment(&mut out, 0xC0, &sof);
    dht(&mut out, 0x00, &DC_LUMA_BITS, &DC_VALS);
    dht(&mut out, 0x10, &AC_LUMA_BITS, &AC_LUMA_VALS);
    if !grey {
        dht(&mut out, 0x01, &DC_CHROMA_BITS, &DC_VALS);
        dht(&mut out, 0x11, &AC_CHROMA_BITS, &AC_CHROMA_VALS);
    }
    if coded.len() > 1 {
        let interval = u16::try_from(rows_per_band * mx).map_err(|_| EncodeError::Unsupported("a restart interval past 65,535 MCUs"))?;
        segment(&mut out, 0xDD, &interval.to_be_bytes());
    }
    if grey {
        segment(&mut out, 0xDA, &[1, 1, 0x00, 0, 63, 0]);
    } else {
        segment(&mut out, 0xDA, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
    }
    let last = coded.len().saturating_sub(1);
    for (i, c) in coded.iter().enumerate() {
        out.extend_from_slice(c);
        if i < last {
            out.extend_from_slice(&[0xFF, 0xD0 + (i % 8) as u8]);
        }
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The AAN path matches the direct DCT-II definition.
    #[test]
    fn aan_matches_reference_dct() {
        let block: [f32; 64] = std::array::from_fn(|i| ((i * 37 % 255) as f32) - 128.0);
        let q = [1u16; 64];
        let mut out = [0i16; 64];
        fdct_quant(&block, &recip_table(&q), &mut out);
        for v in 0..8 {
            for u in 0..8 {
                let c = |k: usize| if k == 0 { std::f64::consts::FRAC_1_SQRT_2 } else { 1.0 };
                let mut s = 0.0;
                for y in 0..8 {
                    for x in 0..8 {
                        s += block[y * 8 + x] as f64
                            * (((2 * x + 1) * u) as f64 * std::f64::consts::PI / 16.0).cos()
                            * (((2 * y + 1) * v) as f64 * std::f64::consts::PI / 16.0).cos();
                    }
                }
                let f = 0.25 * c(u) * c(v) * s;
                assert!((out[v * 8 + u] as f64 - f).abs() <= 0.51, "({u},{v}): {} vs {f}", out[v * 8 + u]);
            }
        }
    }

    #[test]
    fn huffman_tables_are_consistent() {
        for (bits, n) in [(&AC_LUMA_BITS, AC_LUMA_VALS.len()), (&AC_CHROMA_BITS, AC_CHROMA_VALS.len()), (&DC_LUMA_BITS, 12), (&DC_CHROMA_BITS, 12)] {
            assert_eq!(bits.iter().map(|&b| b as usize).sum::<usize>(), n);
        }
        // Every symbol a baseline block can need has a code: DC sizes 0..=11, AC run/size pairs
        // with sizes 1..=10, ZRL and EOB.
        for (dc, ac) in [
            (Huff::new(&DC_LUMA_BITS, &DC_VALS), Huff::new(&AC_LUMA_BITS, &AC_LUMA_VALS)),
            (Huff::new(&DC_CHROMA_BITS, &DC_VALS), Huff::new(&AC_CHROMA_BITS, &AC_CHROMA_VALS)),
        ] {
            assert!((0..=11).all(|s| dc.get(s).1 > 0));
            assert!((0..16).all(|run| (1..=10).all(|s| ac.get((run << 4) | s).1 > 0)));
            assert!(ac.get(0xF0).1 > 0 && ac.get(0x00).1 > 0);
        }
    }

    #[test]
    fn extreme_blocks_stay_in_the_baseline_range() {
        let mut out = [0i16; 64];
        for block in [[127f32; 64], [-128f32; 64], std::array::from_fn(|i| if (i / 8 + i % 8) % 2 == 0 { 127.0 } else { -128.0 })] {
            fdct_quant(&block, &recip_table(&[1; 64]), &mut out);
            assert!(out.iter().all(|&c| c.unsigned_abs() <= 1023), "{out:?}");
        }
    }

    #[test]
    fn quality_scaling_follows_the_usual_curve() {
        assert_eq!(scaled_quant(&Q_LUMA, 50), Q_LUMA);
        assert!(scaled_quant(&Q_LUMA, 100).iter().all(|&v| v == 1));
        assert_eq!(scaled_quant(&Q_LUMA, 75)[0], 8);
        assert!(scaled_quant(&Q_CHROMA, 1).iter().all(|&v| v == 255));
    }
}
