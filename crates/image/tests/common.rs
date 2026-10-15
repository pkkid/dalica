//! Helpers the `dalica-image` tests share: the quick/full switch, scenes pictures as RGBA, and the
//! fingerprint checks (`architecture.md` §11: an 8 × 8 grid of per-channel mean and deviation;
//! on a mismatch the actual and expected PNGs go to `target/test-output/image/`, and
//! `DALICA_BLESS=1` stores a new fingerprint: look at the PNGs first).

#![allow(dead_code)]

use std::path::PathBuf;

use dalica_image::Rgba8;
use dalica_scenes::{Rgb8, fingerprint};

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A property test's case count: `n` in full mode, at most 32 in quick mode.
pub fn cases(n: u32) -> u32 {
    if full() { n } else { n.min(32) }
}

/// `target/test-output/image/`, created.
pub fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/image");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Opaque RGBA of an RGB image.
pub fn rgba(img: &Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect() }
}

/// A 16-bit sample rounded to 8 bits.
pub fn to8_16(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32_767) / 65_535) as u8
}

/// The largest difference between the two images' fingerprints.
pub fn distance(a: &Rgba8, b: &Rgba8) -> f64 {
    fingerprint::distance(&fingerprint::fingerprint(a.width, a.height, &a.data), &fingerprint::fingerprint(b.width, b.height, &b.data))
}

/// Fails (writing actual/expected PNGs) unless the two have the same size and their fingerprints
/// agree within `tol` per value.
pub fn assert_close(case: &str, actual: &Rgba8, expected: &Rgba8, tol: f64) {
    let d = distance(actual, expected);
    if d > tol || (actual.width, actual.height) != (expected.width, expected.height) {
        let dir = out_dir();
        std::fs::write(dir.join(format!("{case}-actual.png")), fingerprint::png_rgba(actual.width, actual.height, &actual.data)).unwrap();
        std::fs::write(dir.join(format!("{case}-expected.png")), fingerprint::png_rgba(expected.width, expected.height, &expected.data)).unwrap();
        panic!("{case}: {d:.2} away from its reference, tolerance {tol} (PNGs in {})", dir.display());
    }
}

/// Fails (writing actual/expected PNGs) unless `actual` passes `fingerprint::check` against the
/// fingerprint of `expected` within `tol` per value.
pub fn assert_cells_close(case: &str, actual: &Rgba8, expected: &Rgba8, tol: f64) {
    let e = fingerprint::fingerprint(expected.width, expected.height, &expected.data);
    if let Err(m) = fingerprint::check(actual.width, actual.height, &actual.data, &e, tol) {
        let dir = out_dir();
        std::fs::write(dir.join(format!("{case}-actual.png")), &m.actual_png).unwrap();
        std::fs::write(dir.join(format!("{case}-expected.png")), fingerprint::png_rgba(expected.width, expected.height, &expected.data)).unwrap();
        panic!("{case}: {} (PNGs in {})", m.message, dir.display());
    }
}

/// Compares `img` with `fingerprints/<name>.json` within `tol` (`DALICA_BLESS=1` stores it
/// instead), writing the image as `target/test-output/image/<name>.png` to look at.
pub fn assert_fingerprint(name: &str, img: &Rgba8, tol: f64) {
    let dir = out_dir();
    std::fs::write(dir.join(format!("{name}.png")), fingerprint::png_rgba(img.width, img.height, &img.data)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(img.width, img.height, &img.data)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(img.width, img.height, &img.data, &expected, tol) {
        std::fs::write(dir.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {} (images in {})", m.message, dir.display());
    }
}
