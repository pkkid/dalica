//! The colour sketch: a blurred preview of a photo in at most [`MAX_BYTES`] bytes, kept in the index
//! and in memory for every photo so a cell can be drawn on any frame without a disk read
//! (`architecture.md` §6.5).
//!
//! In the ThumbHash style, written from the public description of the idea: the picture is
//! averaged down to a small grid (alpha-weighted, in sRGB values), split into a lightness channel
//! L = (r + g + b) / 3 and two colour differences P = (r + g) / 2 − b and Q = r − g, and each
//! channel keeps the low-frequency corner of its cosine transform. Lightness keeps 6 frequencies
//! along the long side and at least 4 along the short one; the colour channels keep 4 × 4 at every
//! aspect, because sharp colour edges (a horizon) need them most. Each channel stores its mean (DC)
//! and its largest other coefficient (the scale), and every other coefficient as a 4-bit fraction
//! of that scale. The layout of the bytes is our own, bits least significant first:
//!
//! ```text
//! bits  0..4   format version (1)
//! bits  4..13  aspect: a signed 9-bit code c, width / height = 2^(c / 32)
//! bits 13..21  mean of L (8 bits, 0..1)
//! bits 21..35  means of P and Q (7 bits each, codes 0..=126 for −1..1, 63 is zero)
//! bits 35..53  scales of L, P and Q (6 bits each), stored as √(scale / cap), rounded up
//! bits 53..    the other coefficients, 4 bits each (codes 0..=14, 7 is zero), L then P then Q,
//!              each in row order over the triangle cx / nx + cy / ny < 1; zero padding to a byte
//! ```
//!
//! The coefficient counts follow from the aspect, so the length of a valid sketch is fixed by its
//! first two bytes: 26 bytes for a square photo, 24 for a long thin one, never more than
//! [`MAX_BYTES`]. Decoding checks the version, the exact length, the code ranges and the padding:
//! anything else is `Err`.
//!
//! Every step uses only `+ − × ÷ √` and rounding (our own cosine, no `powf`, `log` or `cos` from
//! the platform's maths library), so the same pixels give the same bytes on every platform.

use crate::Rgba8;

/// The most bytes a sketch takes.
pub const MAX_BYTES: usize = 32;

/// The format version written in the first four bits.
pub const VERSION: u8 = 1;

/// The longest side [`Sketch::to_rgba`] and [`Sketch::grid`] produce.
pub const MAX_DECODE_SIDE: u32 = 64;

/// The long side a sketch is usually drawn from: enough for a smooth blur when scaled up.
pub const DEFAULT_DECODE_SIDE: u32 = 32;

/// Frequencies kept along the long side, and the fewest along the short side: lightness, then
/// the two colour channels.
const L_LIMIT: usize = 6;
const L_LEAST: usize = 4;
const C_LIMIT: usize = 4;
const C_LEAST: usize = 4;
/// Room for the coefficient grid of any channel.
const GRID: usize = 6;
const _: () = assert!(L_LIMIT <= GRID && C_LIMIT <= GRID);
/// A square sketch, the longest (pinned by the tests), fits the cap.
const _: () = assert!((HEADER_BITS + AC_BITS * (21 - 1 + 2 * (10 - 1))).div_ceil(8) <= MAX_BYTES);
/// The photo is averaged down to at most this many cells on its long side before the transform.
const WORK_SIDE: u64 = 32;
/// Bits of the lightness mean, of each colour mean, and of each scale.
const L_DC_BITS: usize = 8;
const C_DC_BITS: usize = 7;
const SCALE_BITS: usize = 6;
/// Bits of each AC coefficient, and the code of zero (codes 0..=2 × AC_HALF).
const AC_BITS: usize = 4;
const AC_HALF: f64 = 7.0;
/// Header bits: version 4, aspect 9, the three means and the three scales.
const HEADER_BITS: usize = 4 + 9 + L_DC_BITS + 2 * C_DC_BITS + 3 * SCALE_BITS;
/// The aspect code's range (9 bits, signed): 2^(−8) to 2^(255/32), far past any panorama.
const ASPECT_MIN: i32 = -256;
const ASPECT_MAX: i32 = 255;
/// The largest scale each channel stores; larger coefficients are clipped (only near-checkerboard
/// pictures reach them).
const L_CAP: f64 = 1.0;
const C_CAP: f64 = 1.5;

/// Why a sketch could not be made or read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{width} × {height} with {bytes} bytes is not an RGBA image")]
    Image { width: u32, height: u32, bytes: usize },
    #[error("a sketch is at least 7 bytes, this has {0}")]
    Short(usize),
    #[error("unknown sketch version {0}")]
    Version(u8),
    #[error("a sketch of this shape is {expected} bytes, this has {got}")]
    Length { expected: usize, got: usize },
    #[error("the sketch is malformed: {0}")]
    Malformed(&'static str),
}

/// One decoded sketch: cheap to keep, copy and draw (no allocation until it is drawn).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sketch {
    aspect_code: i32,
    l: Channel,
    p: Channel,
    q: Channel,
}

/// The coefficients of one channel, row-major `cy * GRID + cx`; the DC in `c[0]`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Channel {
    nx: usize,
    ny: usize,
    c: [f32; GRID * GRID],
}

/// Makes the sketch of an upright RGBA picture (any size; usually a 128 or 256 thumbnail). The
/// sketch is opaque: colours are weighted by alpha, and fully transparent areas take the
/// picture's mean colour.
pub fn encode(image: &Rgba8) -> Result<Vec<u8>, Error> {
    let bad = || Error::Image { width: image.width, height: image.height, bytes: image.data.len() };
    let (w, h) = (u64::from(image.width), u64::from(image.height));
    let expected = w.checked_mul(h).and_then(|n| n.checked_mul(4)).ok_or_else(bad)?;
    if w == 0 || h == 0 || u64::try_from(image.data.len()).ok() != Some(expected) {
        return Err(bad());
    }
    let code = aspect_code(w as f64 / h as f64);
    let (gw, gh) = work_size(w, h);
    let cells = average(image, gw, gh).ok_or_else(bad)?;

    let (lx, ly) = counts(code, L_LIMIT, L_LEAST);
    let (cx, cy) = counts(code, C_LIMIT, C_LEAST);
    let l = transform(&cells, gw, gh, lx, ly, |[r, g, b]| (r + g + b) / 3.0);
    let p = transform(&cells, gw, gh, cx, cy, |[r, g, b]| (r + g) / 2.0 - b);
    let q = transform(&cells, gw, gh, cx, cy, |[r, g, _]| r - g);

    let mut out = Bits::default();
    out.put(u32::from(VERSION), 4);
    out.put((code & 0x1ff) as u32, 9);
    out.put(unit_code(l.dc(), L_DC_BITS), L_DC_BITS);
    out.put(signed_code(p.dc(), C_DC_BITS), C_DC_BITS);
    out.put(signed_code(q.dc(), C_DC_BITS), C_DC_BITS);
    let (ls, ps, qs) = (scale_code(l.scale(), L_CAP), scale_code(p.scale(), C_CAP), scale_code(q.scale(), C_CAP));
    for s in [ls, ps, qs] {
        out.put(s, SCALE_BITS);
    }
    for (chan, s) in [(&l, scale_of(ls, L_CAP)), (&p, scale_of(ps, C_CAP)), (&q, scale_of(qs, C_CAP))] {
        for (x, y) in ac_cells(chan.nx, chan.ny) {
            let v = chan.get(x, y);
            let n = if s > 0.0 { (v / s * AC_HALF).round().clamp(-AC_HALF, AC_HALF) + AC_HALF } else { AC_HALF };
            out.put(n as u32, AC_BITS);
        }
    }
    Ok(out.bytes)
}

/// Reads a sketch. Hostile bytes give `Err`, never a panic.
pub fn decode(bytes: &[u8]) -> Result<Sketch, Error> {
    if bytes.len() < HEADER_BITS.div_ceil(8) {
        return Err(Error::Short(bytes.len()));
    }
    let mut r = Reader { bytes, at: 0 };
    let version = r.take(4)? as u8;
    if version != VERSION {
        return Err(Error::Version(version));
    }
    let raw = r.take(9)? as i32;
    let code = if raw >= 256 { raw - 512 } else { raw };
    let (lx, ly) = counts(code, L_LIMIT, L_LEAST);
    let (cx, cy) = counts(code, C_LIMIT, C_LEAST);
    let expected = length(lx, ly, cx, cy);
    if bytes.len() != expected {
        return Err(Error::Length { expected, got: bytes.len() });
    }
    let mut l = Channel::new(lx, ly);
    let mut p = Channel::new(cx, cy);
    let mut q = Channel::new(cx, cy);
    l.c[0] = unit_of(r.take(L_DC_BITS)?, L_DC_BITS) as f32;
    p.c[0] = signed_of(r.take(C_DC_BITS)?, C_DC_BITS)? as f32;
    q.c[0] = signed_of(r.take(C_DC_BITS)?, C_DC_BITS)? as f32;
    let scales = [scale_of(r.take(SCALE_BITS)?, L_CAP), scale_of(r.take(SCALE_BITS)?, C_CAP), scale_of(r.take(SCALE_BITS)?, C_CAP)];
    for (chan, s) in [&mut l, &mut p, &mut q].into_iter().zip(scales) {
        for (x, y) in ac_cells(chan.nx, chan.ny) {
            let n = r.take(AC_BITS)?;
            if f64::from(n) > 2.0 * AC_HALF {
                return Err(Error::Malformed("a coefficient is out of range"));
            }
            if let Some(slot) = chan.c.get_mut(y * GRID + x) {
                *slot = ((f64::from(n) - AC_HALF) / AC_HALF * s) as f32;
            }
        }
    }
    if r.rest() != 0 {
        return Err(Error::Malformed("the padding is not zero"));
    }
    Ok(Sketch { aspect_code: code, l, p, q })
}

impl Sketch {
    /// Width ÷ height of the sketched photo (within 1.1 %).
    pub fn aspect(&self) -> f64 {
        aspect_of(self.aspect_code)
    }

    /// The size [`Sketch::to_rgba`] makes for `long_side` (clamped to 1..=[`MAX_DECODE_SIDE`]).
    pub fn size(&self, long_side: u32) -> (u32, u32) {
        let long = long_side.clamp(1, MAX_DECODE_SIDE);
        let a = self.aspect();
        let short = |v: f64| (v.round() as u32).clamp(1, long);
        if a >= 1.0 { (long, short(f64::from(long) / a)) } else { (short(f64::from(long) * a), long) }
    }

    /// The mean colour, opaque.
    pub fn mean(&self) -> [u8; 4] {
        rgb(self.l.dc(), self.p.dc(), self.q.dc())
    }

    /// The sketch as a small opaque picture with the photo's aspect, `long_side` pixels on its
    /// long side (clamped to 1..=[`MAX_DECODE_SIDE`]); scale it up with filtering to draw it.
    pub fn to_rgba(&self, long_side: u32) -> Rgba8 {
        let (w, h) = self.size(long_side);
        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
        self.render(w as usize, h as usize, |px| data.extend_from_slice(&px));
        Rgba8 { width: w, height: h, data }
    }

    /// The colour at the centre of each cell of a `cols` × `rows` grid laid over the photo
    /// (each clamped to 1..=[`MAX_DECODE_SIDE`]), row by row: for drawing a few flat rectangles.
    pub fn grid(&self, cols: u32, rows: u32) -> Vec<[u8; 4]> {
        let (w, h) = (cols.clamp(1, MAX_DECODE_SIDE) as usize, rows.clamp(1, MAX_DECODE_SIDE) as usize);
        let mut out = Vec::with_capacity(w * h);
        self.render(w, h, |px| out.push(px));
        out
    }

    /// Evaluates the sketch at the centres of a `w` × `h` grid, row by row (both at most
    /// [`MAX_DECODE_SIDE`]): per row, each channel's coefficients collapse to one value per
    /// x frequency, then each pixel is a short dot product.
    fn render(&self, w: usize, h: usize, mut emit: impl FnMut([u8; 4])) {
        let cos_x = cosines(w);
        let cos_y = cosines(h);
        for cos_row in cos_y.iter().take(h) {
            let [l, p, q] = [&self.l, &self.p, &self.q].map(|chan| chan.row(cos_row));
            for cos_col in cos_x.iter().take(w) {
                let dot = |row: &[f32; GRID]| row.iter().zip(cos_col).map(|(a, b)| a * b).sum::<f32>();
                emit(rgb(dot(&l), dot(&p), dot(&q)));
            }
        }
    }
}

impl Channel {
    fn new(nx: usize, ny: usize) -> Channel {
        Channel { nx, ny, c: [0.0; GRID * GRID] }
    }

    fn get(&self, x: usize, y: usize) -> f64 {
        f64::from(self.c.get(y * GRID + x).copied().unwrap_or(0.0))
    }

    fn dc(&self) -> f32 {
        self.c.first().copied().unwrap_or(0.0)
    }

    /// The largest AC coefficient's magnitude.
    fn scale(&self) -> f64 {
        ac_cells(self.nx, self.ny).map(|(x, y)| self.get(x, y).abs()).fold(0.0, f64::max)
    }

    /// For one output row: per x frequency, the sum over y frequencies of coefficient × cos.
    fn row(&self, cos_y: &[f32; GRID]) -> [f32; GRID] {
        let mut out = [0f32; GRID];
        // Coefficients outside the kept triangle are zero, so every channel sums the whole grid.
        for (coefficients, k) in self.c.as_chunks::<GRID>().0.iter().zip(cos_y) {
            for (o, c) in out.iter_mut().zip(coefficients) {
                *o += c * k;
            }
        }
        out
    }
}

/// The coefficient grid of one channel from the averaged cells, scaled so that decoding is a plain
/// sum of coefficient × cos × cos.
fn transform(cells: &[[f64; 3]], gw: usize, gh: usize, nx: usize, ny: usize, f: impl Fn([f64; 3]) -> f64) -> Channel {
    let values: Vec<f64> = cells.iter().map(|c| f(*c)).collect();
    let mut chan = Channel::new(nx, ny);
    // cos(π k (i + ½) / n) for each axis, as `[k][i]`.
    let table = |n: usize| -> Vec<Vec<f64>> { (0..GRID).map(|k| (0..n).map(|i| cos_pi(k as f64 * (i as f64 + 0.5) / n as f64)).collect()).collect() };
    let (cos_x, cos_y) = (table(gw), table(gh));
    let cos = |t: &Vec<Vec<f64>>, k: usize, i: usize| t.get(k).and_then(|r| r.get(i)).copied().unwrap_or(0.0);
    // Separable: first along x for every row, then along y.
    let mut rows = vec![[0f64; GRID]; gh];
    for (y, row) in rows.iter_mut().enumerate() {
        for (cx, slot) in row.iter_mut().enumerate().take(nx.min(gw)) {
            *slot = (0..gw).map(|x| values.get(y * gw + x).copied().unwrap_or(0.0) * cos(&cos_x, cx, x)).sum();
        }
    }
    let n = (gw * gh) as f64;
    for cy in 0..ny.min(gh) {
        for cx in 0..nx.min(gw) {
            let keep = (cx == 0 && cy == 0) || in_triangle(cx, cy, nx, ny);
            if !keep {
                continue;
            }
            let sum: f64 = rows.iter().enumerate().map(|(y, row)| row.get(cx).copied().unwrap_or(0.0) * cos(&cos_y, cy, y)).sum();
            let weight = if cx > 0 { 2.0 } else { 1.0 } * if cy > 0 { 2.0 } else { 1.0 };
            if let Some(slot) = chan.c.get_mut(cy * GRID + cx) {
                *slot = (sum / n * weight) as f32;
            }
        }
    }
    chan
}

/// The photo averaged into `gw` × `gh` cells as 0..1 RGB, weighted by alpha.
fn average(image: &Rgba8, gw: usize, gh: usize) -> Option<Vec<[f64; 3]>> {
    let (w, h) = (image.width as usize, image.height as usize);
    let col: Vec<usize> = (0..w).map(|x| x * gw / w).collect();
    let mut sums = vec![[0u64; 4]; gw * gh];
    for (y, row) in image.data.chunks_exact(w.checked_mul(4)?).enumerate().take(h) {
        let base = (y * gh / h) * gw;
        for (&[r, g, b, a], cell) in row.as_chunks::<4>().0.iter().zip(&col) {
            let s = sums.get_mut(base + cell)?;
            let a = u64::from(a);
            *s = [s[0] + u64::from(r) * a, s[1] + u64::from(g) * a, s[2] + u64::from(b) * a, s[3] + a];
        }
    }
    let total = sums.iter().fold([0u64; 4], |t, s| [t[0] + s[0], t[1] + s[1], t[2] + s[2], t[3] + s[3]]);
    let colour = |s: &[u64; 4]| [s[0], s[1], s[2]].map(|v| v as f64 / (s[3] as f64 * 255.0));
    let mean = if total[3] > 0 { colour(&total) } else { [0.5; 3] };
    Some(sums.iter().map(|s| if s[3] > 0 { colour(s) } else { mean }).collect())
}

/// The averaging grid: the photo's own size, or `WORK_SIDE` on the long side.
fn work_size(w: u64, h: u64) -> (usize, usize) {
    let long = w.max(h);
    let side = long.min(WORK_SIDE);
    let fit = |v: u64| ((v * side + long / 2) / long).clamp(1, side) as usize;
    (fit(w), fit(h))
}

/// How many frequencies a channel keeps along x and y for an aspect code.
fn counts(code: i32, limit: usize, least: usize) -> (usize, usize) {
    let a = aspect_of(code);
    let short = |v: f64| (v.round() as usize).clamp(least.min(limit), limit);
    if a >= 1.0 { (limit, short(limit as f64 / a)) } else { (short(limit as f64 * a), limit) }
}

/// The AC cells kept for an `nx` × `ny` grid, in row order.
fn ac_cells(nx: usize, ny: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..ny).flat_map(move |y| (0..nx).map(move |x| (x, y))).filter(move |&(x, y)| (x, y) != (0, 0) && in_triangle(x, y, nx, ny))
}

fn in_triangle(x: usize, y: usize, nx: usize, ny: usize) -> bool {
    x * ny < nx * (ny - y.min(ny))
}

/// The byte length of a sketch with these coefficient counts.
fn length(lx: usize, ly: usize, cx: usize, cy: usize) -> usize {
    let bits = HEADER_BITS + AC_BITS * (ac_cells(lx, ly).count() + 2 * ac_cells(cx, cy).count());
    bits.div_ceil(8)
}

/// The largest code of a `bits`-bit field.
fn top(bits: usize) -> f64 {
    f64::from((1u32 << bits) - 1)
}

/// 0..1 as a `bits`-bit code.
fn unit_code(v: f32, bits: usize) -> u32 {
    (f64::from(v).clamp(0.0, 1.0) * top(bits)).round() as u32
}

fn unit_of(n: u32, bits: usize) -> f64 {
    f64::from(n) / top(bits)
}

/// −1..1 as a `bits`-bit code with an exact zero in the middle; the top code is unused.
fn signed_code(v: f32, bits: usize) -> u32 {
    let half = (top(bits) - 1.0) / 2.0;
    ((f64::from(v).clamp(-1.0, 1.0) + 1.0) * half).round() as u32
}

fn signed_of(n: u32, bits: usize) -> Result<f64, Error> {
    let half = (top(bits) - 1.0) / 2.0;
    if f64::from(n) > 2.0 * half {
        return Err(Error::Malformed("a mean colour is out of range"));
    }
    Ok(f64::from(n) / half - 1.0)
}

/// A scale is stored as √(scale / cap), rounded up so every coefficient stays within it.
fn scale_code(scale: f64, cap: f64) -> u32 {
    ((scale / cap).clamp(0.0, 1.0).sqrt() * top(SCALE_BITS)).ceil().clamp(0.0, top(SCALE_BITS)) as u32
}

fn scale_of(n: u32, cap: f64) -> f64 {
    let f = unit_of(n, SCALE_BITS);
    f * f * cap
}

/// LPQ to opaque 8-bit sRGB.
fn rgb(l: f32, p: f32, q: f32) -> [u8; 4] {
    let b = l - p * 2.0 / 3.0;
    let r = p + b + q / 2.0;
    let g = r - q;
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    [to8(r), to8(g), to8(b), 255]
}

/// cos(π k (i + ½) / n) for i < n (at most [`MAX_DECODE_SIDE`]) and k < GRID, as `[i][k]`.
fn cosines(n: usize) -> [[f32; GRID]; MAX_DECODE_SIDE as usize] {
    let mut t = [[0f32; GRID]; MAX_DECODE_SIDE as usize];
    for (i, ks) in t.iter_mut().enumerate().take(n) {
        // Chebyshev recurrence: cos((k+1)θ) = 2 cos θ cos kθ − cos((k−1)θ).
        let c1 = cos_pi((i as f64 + 0.5) / n as f64);
        let (mut prev, mut cur) = (1.0, c1);
        for (k, slot) in ks.iter_mut().enumerate() {
            *slot = if k == 0 { 1.0 } else { cur as f32 };
            if k > 0 {
                (prev, cur) = (cur, 2.0 * c1 * cur - prev);
            }
        }
    }
    t
}

/// cos(π t) from `+ − × ÷` only, so it is the same on every platform (error below 1e-8; `t` is a
/// small frequency × position here, far below where the reduction loses precision).
fn cos_pi(t: f64) -> f64 {
    let t = t.abs();
    let t = t - ((t / 2.0) as u64 * 2) as f64;
    let t = if t > 1.0 { 2.0 - t } else { t };
    let (t, sign) = if t > 0.5 { (1.0 - t, -1.0) } else { (t, 1.0) };
    let x = t * std::f64::consts::PI;
    let x2 = x * x;
    // Taylor series to x^14 on [0, π/2].
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..=7u32 {
        let d = f64::from((2 * k - 1) * (2 * k));
        term = -term * x2 / d;
        sum += term;
    }
    sign * sum
}

/// width ÷ height for an aspect code: 2^(code / 32), from square roots only.
fn aspect_of(code: i32) -> f64 {
    let (whole, frac) = (code.div_euclid(32), code.rem_euclid(32));
    let mut v = 1.0;
    for _ in 0..whole.unsigned_abs() {
        v = if whole > 0 { v * 2.0 } else { v / 2.0 };
    }
    // frac/32 in binary: bit 4 is ½, bit 3 is ¼, … bit 0 is 1/32.
    let mut root = std::f64::consts::SQRT_2;
    for bit in (0..5).rev() {
        if frac & (1 << bit) != 0 {
            v *= root;
        }
        root = root.sqrt();
    }
    v
}

/// The aspect code nearest to `ratio` in log terms (ties and extremes deterministic).
fn aspect_code(ratio: f64) -> i32 {
    // 2^(1/64): half a step, so the boundary between codes c and c + 1 is aspect_of(c) × HALF.
    let half = (0..6).fold(2.0f64, |v, _| v.sqrt());
    let (mut lo, mut hi) = (ASPECT_MIN, ASPECT_MAX);
    // The smallest code whose upper boundary lies above the ratio.
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if ratio < aspect_of(mid) * half { hi = mid } else { lo = mid + 1 }
    }
    lo
}

#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    at: usize,
}

impl Bits {
    /// Appends the low `n` bits of `v`, least significant first.
    fn put(&mut self, v: u32, n: usize) {
        for i in 0..n {
            if self.at.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (v >> i) & 1 == 1
                && let Some(b) = self.bytes.last_mut()
            {
                *b |= 1 << (self.at % 8);
            }
            self.at += 1;
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<u32, Error> {
        let mut v = 0;
        for i in 0..n {
            let byte = self.bytes.get(self.at / 8).ok_or(Error::Malformed("the sketch ends early"))?;
            v |= u32::from((byte >> (self.at % 8)) & 1) << i;
            self.at += 1;
        }
        Ok(v)
    }

    /// The bits left after the last coefficient, as a number (zero when the padding is clean).
    fn rest(&mut self) -> u32 {
        let left = (self.bytes.len() * 8).saturating_sub(self.at);
        self.take(left.min(31)).unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cos_matches_the_library() {
        for i in -400..=400 {
            let t = f64::from(i) / 97.0;
            assert!((cos_pi(t) - (std::f64::consts::PI * t).cos()).abs() < 1e-8, "{t}");
        }
    }

    #[test]
    fn aspect_codes_round_trip_and_are_nearest() {
        assert_eq!(aspect_code(1.0), 0);
        for code in ASPECT_MIN..=ASPECT_MAX {
            assert_eq!(aspect_code(aspect_of(code)), code);
        }
        for i in 1..2000 {
            let r = f64::from(i) / 100.0;
            let a = aspect_of(aspect_code(r));
            assert!((a / r).log2().abs() <= 1.0 / 64.0 + 1e-9, "{r} → {a}");
        }
    }

    #[test]
    fn longest_sketch_fits() {
        let longest = (ASPECT_MIN..=ASPECT_MAX)
            .map(|c| {
                let (lx, ly) = counts(c, L_LIMIT, L_LEAST);
                let (cx, cy) = counts(c, C_LIMIT, C_LEAST);
                length(lx, ly, cx, cy)
            })
            .max();
        let shortest = (ASPECT_MIN..=ASPECT_MAX)
            .map(|c| {
                let (lx, ly) = counts(c, L_LIMIT, L_LEAST);
                let (cx, cy) = counts(c, C_LIMIT, C_LEAST);
                length(lx, ly, cx, cy)
            })
            .min();
        assert_eq!((shortest, longest), (Some(24), Some(26)));
        assert_eq!(length(L_LIMIT, L_LIMIT, C_LIMIT, C_LIMIT), 26);
    }
}
