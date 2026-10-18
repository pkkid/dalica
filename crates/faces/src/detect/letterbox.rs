//! A photo fitted into the detector's square input: scaled with its aspect ratio kept into the top
//! left corner, the rest black, as NCHW float32 in the model's colour order and normalisation.
//!
//! Ported from LightCraft `crates/faces/src/yunet.rs` @ f34c7a8a (MIT OR Apache-2.0): the two-pass
//! resampling (exact area averaging when shrinking, linear interpolation when enlarging); the
//! colour order and the normalisation come from the manifest here.

use crate::align::Rgb;
use crate::manifest::{Colour, InputSpec};

struct Tap {
    start: usize,
    weights: Vec<f32>,
}

/// For each of `dst` output positions, which `src` positions to average and with what weights.
/// `src` and `dst` are at least 1.
fn taps(src: usize, dst: usize) -> Vec<Tap> {
    let last = src.saturating_sub(1);
    let r = src as f32 / dst.max(1) as f32;
    (0..dst)
        .map(|i| {
            if r >= 1.0 {
                let (a, b) = (i as f32 * r, (i as f32 + 1.0) * r);
                let first = (a.floor() as usize).min(last);
                let end = ((b.ceil() as usize).min(src)).max(first + 1);
                let weights: Vec<f32> = (first..end).map(|j| ((j as f32 + 1.0).min(b) - (j as f32).max(a)).max(0.0) / r).collect();
                Tap { start: first, weights }
            } else {
                let c = ((i as f32 + 0.5) * r - 0.5).max(0.0);
                let lo = (c.floor() as usize).min(last);
                let hi = (lo + 1).min(last);
                let f = c - lo as f32;
                if hi == lo { Tap { start: lo, weights: vec![1.0] } } else { Tap { start: lo, weights: vec![1.0 - f, f] } }
            }
        })
        .collect()
}

/// `img` scaled by `scale` into the top left of a `side` × `side` input; `None` when the picture is
/// not valid. Returns the planes, ready for a `[1, 3, side, side]` tensor.
pub(crate) fn letterbox(img: &Rgb, side: usize, scale: f32, spec: &InputSpec) -> Option<Vec<f32>> {
    if !img.valid() || side == 0 || !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let (width, height) = (img.width, img.height);
    let nw = ((width as f32 * scale).round() as usize).clamp(1, side);
    let nh = ((height as f32 * scale).round() as usize).clamp(1, side);
    let (xt, yt) = (taps(width, nw), taps(height, nh));
    // horizontal pass: every source row, nw columns, three channels
    let mut mid = Vec::new();
    mid.try_reserve_exact(height.checked_mul(nw)?.checked_mul(3)?).ok()?;
    mid.resize(height * nw * 3, 0.0f32);
    for (src_row, mid_row) in img.data.chunks_exact(width * 3).zip(mid.chunks_exact_mut(nw * 3)) {
        for (tap, out) in xt.iter().zip(mid_row.as_chunks_mut::<3>().0) {
            let mut acc = [0.0f32; 3];
            for (j, wt) in tap.weights.iter().enumerate() {
                if let Some(&[r, g, b]) = src_row.get((tap.start + j) * 3..(tap.start + j) * 3 + 3) {
                    acc[0] += wt * f32::from(r);
                    acc[1] += wt * f32::from(g);
                    acc[2] += wt * f32::from(b);
                }
            }
            out.copy_from_slice(&acc);
        }
    }
    // vertical pass into the model's planes, normalised; the padding is black
    let plane = side * side;
    let order: [usize; 3] = match spec.colour {
        Colour::Rgb => [0, 1, 2],
        Colour::Bgr => [2, 1, 0],
    };
    let norm = |c: usize, v: f32| {
        let (m, s) = (spec.mean.get(c).copied().unwrap_or(0.0), spec.std.get(c).copied().unwrap_or(1.0));
        (v - m) / s
    };
    let mut data = Vec::new();
    data.try_reserve_exact(plane.checked_mul(3)?).ok()?;
    for c in 0..3 {
        let black = norm(c, 0.0);
        data.extend(std::iter::repeat_n(black, plane));
    }
    for (y, tap) in yt.iter().enumerate() {
        for x in 0..nw {
            let mut acc = [0.0f32; 3];
            for (j, wt) in tap.weights.iter().enumerate() {
                let at = ((tap.start + j) * nw + x) * 3;
                if let Some(&[r, g, b]) = mid.get(at..at + 3) {
                    acc[0] += wt * r;
                    acc[1] += wt * g;
                    acc[2] += wt * b;
                }
            }
            for (c, &src) in order.iter().enumerate() {
                if let (Some(o), Some(v)) = (data.get_mut(c * plane + y * side + x), acc.get(src)) {
                    *o = norm(c, *v);
                }
            }
        }
    }
    Some(data)
}
