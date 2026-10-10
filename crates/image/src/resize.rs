//! Downscaling for thumbnails: an exact area average (each output pixel is the mean of the source
//! area it covers, with fractional edge coverage), separable, in integer arithmetic. Integer maths
//! gives the same pixels on every CPU, which keeps thumbnail fingerprints portable; SIMD resizers
//! do not.

use crate::{Result, Rgba8};

/// Scales `img` down to fit in `long_side` pixels on its longer side, keeping the aspect ratio
/// (each side at least 1). Never enlarges: a smaller image comes back unchanged.
pub fn fit(img: &Rgba8, long_side: u32) -> Result<Rgba8> {
    let long = img.width.max(img.height);
    if long <= long_side || long_side == 0 {
        return Ok(img.clone());
    }
    let scale = |v: u32| ((u64::from(v) * u64::from(long_side) + u64::from(long) / 2) / u64::from(long)).clamp(1, u64::from(long_side)) as u32;
    resize(img, scale(img.width), scale(img.height))
}

/// Area-average resize to exactly `width × height` (for downscaling; upscaling repeats pixels).
pub fn resize(img: &Rgba8, width: u32, height: u32) -> Result<Rgba8> {
    let (sw, sh) = (img.width as usize, img.height as usize);
    let (dw, dh) = (width.max(1) as usize, height.max(1) as usize);
    let xs = taps(sw, dw);
    let ys = taps(sh, dh);
    // Horizontal pass into 16.8 fixed point: value × 256.
    let mut mid = vec![0u32; dw * sh * 4];
    for y in 0..sh {
        let row = img.data.get(y * sw * 4..(y + 1) * sw * 4).unwrap_or_default();
        for (x, tap) in xs.iter().enumerate() {
            let mut acc = [0u64; 4];
            for &(src, w) in tap {
                for (c, a) in acc.iter_mut().enumerate() {
                    *a += u64::from(row.get(src * 4 + c).copied().unwrap_or(0)) * w;
                }
            }
            for (c, a) in acc.iter().enumerate() {
                if let Some(slot) = mid.get_mut((y * dw + x) * 4 + c) {
                    *slot = ((a * 256 + sw as u64 / 2) / sw as u64) as u32;
                }
            }
        }
    }
    // Vertical pass, rounding once at the end.
    let mut out = vec![0u8; dw * dh * 4];
    for (y, tap) in ys.iter().enumerate() {
        for x in 0..dw {
            let mut acc = [0u64; 4];
            for &(src, w) in tap {
                for (c, a) in acc.iter_mut().enumerate() {
                    *a += u64::from(mid.get((src * dw + x) * 4 + c).copied().unwrap_or(0)) * w;
                }
            }
            let den = sh as u64 * 256;
            for (c, a) in acc.iter().enumerate() {
                if let Some(slot) = out.get_mut((y * dw + x) * 4 + c) {
                    *slot = ((a + den / 2) / den).min(255) as u8;
                }
            }
        }
    }
    Ok(Rgba8 { width: dw as u32, height: dh as u32, data: out })
}

/// For each of `dst` outputs, the source pixels it covers and their weights. Source pixel `j`
/// spans `[j·dst, (j+1)·dst)` and output `i` spans `[i·src, (i+1)·src)` on a common axis, so the
/// overlaps are integers and each output's weights sum to `src`.
fn taps(src: usize, dst: usize) -> Vec<Vec<(usize, u64)>> {
    (0..dst)
        .map(|i| {
            let (lo, hi) = (i * src, (i + 1) * src);
            let first = lo / dst;
            let last = (hi - 1) / dst;
            (first..=last.min(src.saturating_sub(1)))
                .filter_map(|j| {
                    let overlap = hi.min((j + 1) * dst).saturating_sub(lo.max(j * dst));
                    (overlap > 0).then_some((j, overlap as u64))
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Rgba8 {
        Rgba8 { width: w, height: h, data: px.repeat((w * h) as usize) }
    }

    #[test]
    fn weights_cover_each_output_exactly() {
        for (src, dst) in [(7, 3), (3000, 256), (5, 5), (1, 1), (4, 1)] {
            for tap in taps(src, dst) {
                assert_eq!(tap.iter().map(|t| t.1).sum::<u64>(), src as u64);
            }
        }
    }

    #[test]
    fn a_solid_colour_stays_exact() {
        let out = resize(&solid(97, 61, [10, 200, 37, 255]), 13, 8).unwrap();
        assert!(out.data.chunks(4).all(|p| p == [10, 200, 37, 255]));
    }

    #[test]
    fn averages_and_fits() {
        let img = Rgba8 { width: 2, height: 1, data: vec![0, 0, 0, 255, 255, 255, 255, 255] };
        assert_eq!(resize(&img, 1, 1).unwrap().data, [128, 128, 128, 255]);
        let big = solid(6000, 4000, [1, 2, 3, 4]);
        let t = fit(&big, 256).unwrap();
        assert_eq!((t.width, t.height), (256, 171));
        let small = solid(100, 50, [1, 2, 3, 4]);
        assert_eq!(fit(&small, 256).unwrap(), small);
        assert_eq!(fit(&solid(1, 3000, [9, 9, 9, 9]), 256).unwrap().width, 1);
    }
}
