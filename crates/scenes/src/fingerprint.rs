//! Pixel fingerprints instead of golden images (`architecture.md` §11): per-channel mean and standard
//! deviation of R, G and B over an 8 × 8 grid, plus the image size. Tests compare an image with a
//! stored fingerprint (JSON, committed next to the test) within a tolerance; no media is committed.
//!
//! This crate never writes to disk (layer rule 4): on a mismatch, [`Mismatch`] carries the PNGs of
//! what was rendered, and the test writes them under `target/test-output/` for a human to look at.
//! `DALICA_BLESS=1` makes a test store a new fingerprint instead of comparing; a blessed change is
//! committed only after looking at the images (`AGENTS.md` §3).

use serde::{Deserialize, Serialize};

/// Grid cells per side.
pub const GRID: usize = 8;

/// The fingerprint of an RGBA image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub width: u32,
    pub height: u32,
    /// Row-major cells; each holds `[mean, deviation]` for R, G and B, rounded to 0.01.
    pub cells: Vec<[[f64; 2]; 3]>,
}

/// Why an image does not match its fingerprint.
#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    pub message: String,
    /// The rendered image, as PNG, to write next to the failure.
    pub actual_png: Vec<u8>,
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The fingerprint of `rgba` (`width × height`, 4 bytes per pixel, rows top to bottom).
pub fn fingerprint(width: u32, height: u32, rgba: &[u8]) -> Fingerprint {
    let mut cells = Vec::with_capacity(GRID * GRID);
    let (w, h) = (width as usize, height as usize);
    for gy in 0..GRID {
        for gx in 0..GRID {
            let (x0, x1) = (gx * w / GRID, ((gx + 1) * w / GRID).max(gx * w / GRID + 1).min(w.max(1)));
            let (y0, y1) = (gy * h / GRID, ((gy + 1) * h / GRID).max(gy * h / GRID + 1).min(h.max(1)));
            let mut cell = [[0.0; 2]; 3];
            for (c, stat) in cell.iter_mut().enumerate() {
                let (mut sum, mut sq, mut n) = (0.0, 0.0, 0.0);
                for y in y0..y1 {
                    for x in x0..x1 {
                        if let Some(&v) = rgba.get((y * w + x) * 4 + c) {
                            let v = f64::from(v);
                            sum += v;
                            sq += v * v;
                            n += 1.0;
                        }
                    }
                }
                let mean = if n > 0.0 { sum / n } else { 0.0 };
                let var = if n > 0.0 { (sq / n - mean * mean).max(0.0) } else { 0.0 };
                *stat = [round2(mean), round2(var.sqrt())];
            }
            cells.push(cell);
        }
    }
    Fingerprint { width, height, cells }
}

/// The largest difference between two fingerprints' cell values (or infinity if the sizes differ).
pub fn distance(a: &Fingerprint, b: &Fingerprint) -> f64 {
    if (a.width, a.height) != (b.width, b.height) || a.cells.len() != b.cells.len() {
        return f64::INFINITY;
    }
    a.cells.iter().zip(&b.cells).flat_map(|(x, y)| x.iter().zip(y).flat_map(|(p, q)| [(p[0] - q[0]).abs(), (p[1] - q[1]).abs()])).fold(0.0, f64::max)
}

/// Compares `rgba` with `expected` within `tolerance` (in 0–255 units per cell value).
pub fn check(width: u32, height: u32, rgba: &[u8], expected: &Fingerprint, tolerance: f64) -> Result<(), Mismatch> {
    let actual = fingerprint(width, height, rgba);
    let d = distance(&actual, expected);
    if d <= tolerance {
        return Ok(());
    }
    Err(Mismatch {
        message: format!(
            "{width} × {height} image is {d:.2} away from its fingerprint ({} × {}), tolerance {tolerance}",
            expected.width, expected.height
        ),
        actual_png: png_rgba(width, height, rgba),
    })
}

/// An RGBA image as PNG bytes (empty if it cannot be encoded).
pub fn png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let ok = enc.write_header().and_then(|mut w| w.write_image_data(rgba).and_then(|()| w.finish()));
    if ok.is_err() {
        return Vec::new();
    }
    out
}

/// The fingerprint as pretty JSON, for storing.
pub fn to_json(f: &Fingerprint) -> String {
    serde_json::to_string_pretty(f).unwrap_or_default()
}

/// A stored fingerprint.
pub fn from_json(text: &str) -> Result<Fingerprint, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        px.repeat((w * h) as usize)
    }

    #[test]
    fn a_solid_image_has_no_deviation_and_matches_itself() {
        let img = solid(40, 30, [10, 20, 30, 255]);
        let f = fingerprint(40, 30, &img);
        assert_eq!(f.cells.len(), 64);
        assert!(f.cells.iter().all(|c| c == &[[10.0, 0.0], [20.0, 0.0], [30.0, 0.0]]));
        assert!(check(40, 30, &img, &f, 0.0).is_ok());
        assert_eq!(from_json(&to_json(&f)).unwrap(), f);
    }

    #[test]
    fn a_change_or_another_size_is_a_mismatch_with_the_image() {
        let f = fingerprint(40, 30, &solid(40, 30, [10, 20, 30, 255]));
        let other = solid(40, 30, [14, 20, 30, 255]);
        let m = check(40, 30, &other, &f, 3.0).unwrap_err();
        assert!(m.message.contains("4.00 away"), "{}", m.message);
        assert!(m.actual_png.starts_with(b"\x89PNG"));
        assert!(check(40, 30, &other, &f, 4.0).is_ok());
        assert_eq!(distance(&f, &fingerprint(41, 30, &solid(41, 30, [10, 20, 30, 255]))), f64::INFINITY);
        // Tiny images still have 64 cells.
        assert_eq!(fingerprint(3, 2, &solid(3, 2, [1, 1, 1, 1])).cells.len(), 64);
    }
}
