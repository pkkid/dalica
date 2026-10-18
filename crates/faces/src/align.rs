//! Cutting a face out of a photo the way recognisers expect it (`faces.md` §2).
//!
//! With the five landmarks the detector finds (eyes, nose tip, mouth corners), the face is turned
//! and scaled by a similarity transform, found by least squares, so those points land on the
//! standard 112 × 112 template. Without landmarks (a face drawn by hand or read from a file's
//! metadata) a padded square around the box is used instead. Both warp with supersampling, so
//! shrinking a large face does not alias.
//!
//! Pure geometry on 8-bit RGB: no image types, no panics (every access is checked). Positions in
//! pixels have pixel centres at whole numbers; [`face_crop`] takes the normalised rectangle and
//! landmarks a face is stored with and converts them.
//!
//! Ported from LightCraft `crates/faces/src/align.rs` @ f34c7a8a (MIT OR Apache-2.0);
//! [`face_crop`] and [`BOX_PAD`] from its `crates/engine/src/faces_worker.rs`.

/// The five template points of a 112 × 112 aligned face, the eye on the image's left first: eye,
/// eye, nose tip, mouth corner, mouth corner (the usual recogniser template).
pub const TEMPLATE_112: [(f32, f32); 5] = [(38.2946, 51.6963), (73.5318, 51.5014), (56.0252, 71.7366), (41.5493, 92.3655), (70.7299, 92.2041)];

/// A box crop's side as a multiple of the box's larger side.
pub const BOX_PAD: f32 = 1.5;

/// The largest aligned crop side.
pub const MAX_CROP_SIDE: usize = 4096;

/// A borrowed 8-bit RGB picture, `width * height * 3` bytes, rows top to bottom.
#[derive(Clone, Copy, Debug)]
pub struct Rgb<'a> {
    pub data: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl Rgb<'_> {
    /// Whether the size is not zero and the data fills it exactly.
    pub fn valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.width.checked_mul(self.height).and_then(|n| n.checked_mul(3)) == Some(self.data.len())
    }

    /// The pixel at whole-number coordinates, clamped to the picture.
    fn at(&self, x: isize, y: isize) -> [f32; 3] {
        let x = x.clamp(0, self.width.saturating_sub(1) as isize) as usize;
        let y = y.clamp(0, self.height.saturating_sub(1) as isize) as usize;
        let i = (y * self.width + x) * 3;
        match self.data.get(i..i + 3) {
            Some(&[r, g, b]) => [f32::from(r), f32::from(g), f32::from(b)],
            _ => [0.0; 3],
        }
    }

    /// A bilinear sample at a fractional position.
    fn sample(&self, x: f32, y: f32) -> [f32; 3] {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let (a, b, c, d) = (self.at(x0, y0), self.at(x0 + 1, y0), self.at(x0, y0 + 1), self.at(x0 + 1, y0 + 1));
        let mut out = [0.0; 3];
        for (k, o) in out.iter_mut().enumerate() {
            let top = a[k] * (1.0 - fx) + b[k] * fx;
            let bottom = c[k] * (1.0 - fx) + d[k] * fx;
            *o = top * (1.0 - fy) + bottom * fy;
        }
        out
    }
}

/// `q = a · p + t` with complex `a` (scale and rotation) and `t`: source positions to output
/// positions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Similarity {
    pub a: (f32, f32),
    pub t: (f32, f32),
}

impl Similarity {
    /// The scale factor (output pixels per source pixel).
    pub fn scale(&self) -> f32 {
        self.a.0.hypot(self.a.1)
    }

    /// Where the source position `p` lands in the output.
    pub fn apply(&self, p: (f32, f32)) -> (f32, f32) {
        (self.a.0 * p.0 - self.a.1 * p.1 + self.t.0, self.a.0 * p.1 + self.a.1 * p.0 + self.t.1)
    }

    /// Where the output position `q` comes from in the source.
    fn invert(&self, q: (f32, f32)) -> Option<(f32, f32)> {
        let n = self.a.0 * self.a.0 + self.a.1 * self.a.1;
        if !(n.is_finite() && n > 1e-12) {
            return None;
        }
        let (dx, dy) = (q.0 - self.t.0, q.1 - self.t.1);
        Some(((dx * self.a.0 + dy * self.a.1) / n, (dy * self.a.0 - dx * self.a.1) / n))
    }
}

/// The least-squares similarity transform taking `from[i]` to `to[i]` (no reflection); `None` when
/// the points do not determine one (fewer than two, all the same point, or not finite).
pub fn fit_similarity(from: &[(f32, f32)], to: &[(f32, f32)]) -> Option<Similarity> {
    if from.len() != to.len() || from.len() < 2 {
        return None;
    }
    // in float64: landmarks far from the origin lose no precision
    let n = from.len() as f64;
    let mean = |pts: &[(f32, f32)]| (pts.iter().map(|p| f64::from(p.0)).sum::<f64>() / n, pts.iter().map(|p| f64::from(p.1)).sum::<f64>() / n);
    let (mp, mq) = (mean(from), mean(to));
    // a = Σ conj(p') q' / Σ |p'|²   (as complex numbers)
    let (mut re, mut im, mut norm) = (0.0f64, 0.0f64, 0.0f64);
    for (p, q) in from.iter().zip(to) {
        let (px, py) = (f64::from(p.0) - mp.0, f64::from(p.1) - mp.1);
        let (qx, qy) = (f64::from(q.0) - mq.0, f64::from(q.1) - mq.1);
        re += px * qx + py * qy;
        im += px * qy - py * qx;
        norm += px * px + py * py;
    }
    if !(norm.is_finite() && norm > 1e-9) {
        return None;
    }
    let a = (re / norm, im / norm);
    let t = (mq.0 - (a.0 * mp.0 - a.1 * mp.1), mq.1 - (a.0 * mp.1 + a.1 * mp.0));
    let m = Similarity { a: (a.0 as f32, a.1 as f32), t: (t.0 as f32, t.1 as f32) };
    [m.a.0, m.a.1, m.t.0, m.t.1].iter().all(|v| v.is_finite()).then_some(m)
}

/// Fills an `out_w` × `out_h` picture with the source under each output pixel, averaging enough
/// sub-samples that shrinking does not alias.
fn warp(img: &Rgb, m: &Similarity, out_w: usize, out_h: usize) -> Option<Vec<u8>> {
    if !img.valid() || out_w == 0 || out_h == 0 || out_w > MAX_CROP_SIDE || out_h > MAX_CROP_SIDE {
        return None;
    }
    if ![m.a.0, m.a.1, m.t.0, m.t.1].iter().all(|v| v.is_finite()) {
        return None;
    }
    // how many source pixels one output pixel spans, per side
    let span = 1.0 / m.scale();
    let n = if span.is_finite() { (span.ceil() as usize).clamp(1, 4) } else { 1 };
    let mut out = vec![0u8; out_w * out_h * 3];
    for (i, px) in out.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        let (ox, oy) = ((i % out_w) as f32, (i / out_w) as f32);
        let mut acc = [0.0f32; 3];
        for sy in 0..n {
            for sx in 0..n {
                let q = (ox + (sx as f32 + 0.5) / n as f32 - 0.5, oy + (sy as f32 + 0.5) / n as f32 - 0.5);
                let (x, y) = m.invert(q)?;
                let s = img.sample(x, y);
                acc.iter_mut().zip(s).for_each(|(a, v)| *a += v);
            }
        }
        let k = (n * n) as f32;
        for (o, a) in px.iter_mut().zip(acc) {
            *o = (a / k).round().clamp(0.0, 255.0) as u8;
        }
    }
    Some(out)
}

/// The face aligned to the template, `out_w` × `out_h` (the template is scaled from 112 × 112).
/// `landmarks` are in pixels of `img`. `None` when they cannot define a transform or the face would
/// be absurdly small or large.
pub fn align_to_template(img: &Rgb, landmarks: &[(f32, f32); 5], out_w: usize, out_h: usize) -> Option<Vec<u8>> {
    let m = template_transform(landmarks, out_w, out_h)?;
    warp(img, &m, out_w, out_h)
}

/// The transform [`align_to_template`] warps by.
pub fn template_transform(landmarks: &[(f32, f32); 5], out_w: usize, out_h: usize) -> Option<Similarity> {
    let (sx, sy) = (out_w as f32 / 112.0, out_h as f32 / 112.0);
    let template: Vec<(f32, f32)> = TEMPLATE_112.iter().map(|(x, y)| (x * sx, y * sy)).collect();
    let m = fit_similarity(landmarks, &template)?;
    let scale = m.scale();
    (scale.is_finite() && (0.01..=100.0).contains(&scale)).then_some(m)
}

/// A square around a face box (pixels of `img`), `pad` times its larger side, as `out_w` × `out_h`.
#[allow(clippy::too_many_arguments)]
pub fn crop_box(img: &Rgb, x0: f32, y0: f32, x1: f32, y1: f32, pad: f32, out_w: usize, out_h: usize) -> Option<Vec<u8>> {
    if ![x0, y0, x1, y1, pad].iter().all(|v| v.is_finite()) {
        return None;
    }
    let side = (x1 - x0).abs().max((y1 - y0).abs()) * pad;
    if !(side.is_finite() && side >= 2.0) {
        return None;
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let s = out_w.min(out_h) as f32 / side;
    let m = Similarity { a: (s, 0.0), t: ((out_w as f32 - 1.0) / 2.0 - s * cx, (out_h as f32 - 1.0) / 2.0 - s * cy) };
    warp(img, &m, out_w, out_h)
}

/// The crop a recogniser reads for a stored face: `rect` `[x0, y0, x1, y1]` and `landmarks`
/// normalised to the picture (0 … 1, y down, as faces are stored and detected). Aligned to the
/// template by the landmarks when there are any, else a box crop ([`BOX_PAD`]).
pub fn face_crop(img: &Rgb, rect: [f32; 4], landmarks: Option<&[(f32, f32); 5]>, out_w: usize, out_h: usize) -> Option<Vec<u8>> {
    let (w, h) = (img.width as f32, img.height as f32);
    // a normalised position x covers pixel x·w − ½ in centre coordinates
    let px = |(x, y): (f32, f32)| (x * w - 0.5, y * h - 0.5);
    if let Some(l) = landmarks {
        return align_to_template(img, &l.map(px), out_w, out_h);
    }
    let [x0, y0, x1, y1] = rect;
    let (a, b) = (px((x0, y0)), px((x1, y1)));
    crop_box(img, a.0, a.1, b.0, b.1, BOX_PAD, out_w, out_h)
}
