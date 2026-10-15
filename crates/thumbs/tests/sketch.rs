//! The colour sketch (`architecture.md` §6.5) on `scenes` photos: small, deterministic, the right
//! colours and aspect; hostile bytes never panic.

use std::time::Instant;

use dalica_scenes::{Kind, Scene};
use dalica_thumbs::Rgba8;
use dalica_thumbs::sketch::{self, DEFAULT_DECODE_SIDE, Error, MAX_BYTES, MAX_DECODE_SIDE};
use proptest::prelude::*;

/// The stated tolerance, in 8-bit levels, between the photo's 4 × 4 cell means and the decoded
/// sketch's: on any channel of any cell; averaged over one photo's cells and channels; and the
/// median of those averages over all the photos. Measured at M4.1: 33.0, 9.1 and 3.6 (the worst
/// are sharp horizons on 3:1 panoramas, which a blur cannot keep).
const CELL_MAX: f64 = 40.0;
const CELL_MEAN: f64 = 12.0;
const MEDIAN_MEAN: f64 = 5.0;

/// Photo shapes: square, 4:3, 3:2, 16:9 and portrait ones, a 3:1 panorama and a 1:4 strip.
const SHAPES: [(u32, u32); 8] = [(256, 256), (256, 192), (192, 256), (256, 171), (171, 256), (256, 144), (256, 85), (64, 256)];

fn rgba(scene: &Scene) -> Rgba8 {
    let rgb = scene.render().unwrap();
    let data = rgb.data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect();
    Rgba8 { width: rgb.width, height: rgb.height, data }
}

fn photos() -> Vec<(Scene, Rgba8)> {
    let mut out = Vec::new();
    for (i, &(w, h)) in SHAPES.iter().enumerate() {
        for (k, kind) in Kind::ALL.into_iter().enumerate() {
            for seed in 0..4u64 {
                let scene = Scene::new(seed * 31 + i as u64 * 7 + k as u64, w, h).with_kind(kind);
                out.push((scene, rgba(&scene)));
            }
        }
    }
    out
}

/// The mean colour of each cell of a `n` × `n` grid over the picture (RGB, 0..255).
fn cell_means(img: &Rgba8, n: u32) -> Vec<[f64; 3]> {
    let mut sums = vec![[0f64; 4]; (n * n) as usize];
    for y in 0..img.height {
        for x in 0..img.width {
            let i = (y * img.width + x) as usize * 4;
            let cell = ((y * n / img.height) * n + x * n / img.width) as usize;
            for (sum, v) in sums[cell].iter_mut().zip(&img.data[i..i + 3]) {
                *sum += f64::from(*v);
            }
            sums[cell][3] += 1.0;
        }
    }
    sums.iter().map(|s| [s[0] / s[3], s[1] / s[3], s[2] / s[3]]).collect()
}

/// (worst, mean) absolute difference between the photo's and the sketch's 4 × 4 cell means.
fn colour_error(photo: &Rgba8, bytes: &[u8]) -> (f64, f64) {
    let decoded = sketch::decode(bytes).unwrap().to_rgba(DEFAULT_DECODE_SIDE);
    let (a, b) = (cell_means(photo, 4), cell_means(&decoded, 4));
    let diffs: Vec<f64> = a.iter().zip(&b).flat_map(|(p, s)| (0..3).map(move |c| (p[c] - s[c]).abs())).collect();
    (diffs.iter().copied().fold(0.0, f64::max), diffs.iter().sum::<f64>() / diffs.len() as f64)
}

#[test]
fn sketches_match_the_photos_colours_and_aspect() {
    let photos = photos();
    let (mut worst_cell, mut means, mut longest) = (0f64, Vec::new(), 0);
    for (scene, photo) in &photos {
        let bytes = sketch::encode(photo).unwrap();
        assert!(bytes.len() <= MAX_BYTES, "{scene:?}: {} bytes", bytes.len());
        longest = longest.max(bytes.len());
        let s = sketch::decode(&bytes).unwrap();
        let aspect = f64::from(scene.width) / f64::from(scene.height);
        assert!((s.aspect() / aspect - 1.0).abs() < 0.05, "{scene:?}: aspect {} for {aspect}", s.aspect());
        let (w, h) = s.size(DEFAULT_DECODE_SIDE);
        assert!((f64::from(w) / f64::from(h) / aspect - 1.0).abs() < 0.05, "{scene:?}: decoded as {w} × {h}");
        let (max, mean) = colour_error(photo, &bytes);
        assert!(max <= CELL_MAX && mean <= CELL_MEAN, "{scene:?}: cell error max {max:.1}, mean {mean:.1}");
        worst_cell = worst_cell.max(max);
        means.push(mean);
    }
    means.sort_by(f64::total_cmp);
    let median = means[means.len() / 2];
    assert!(median <= MEDIAN_MEAN, "median of the per-photo mean errors {median:.1}");
    println!(
        "4 × 4 cell error over {} photos: worst cell {worst_cell:.1}, worst photo mean {:.1}, median photo mean {median:.1} levels; longest sketch {longest} bytes",
        photos.len(),
        means.last().unwrap()
    );
}

#[test]
fn flat_colours_come_back_almost_exactly() {
    for colour in [[0u8, 0, 0], [255, 255, 255], [128, 128, 128], [200, 40, 30], [20, 160, 60], [30, 60, 220], [250, 220, 0]] {
        let data = (0..48 * 32).flat_map(|_| [colour[0], colour[1], colour[2], 255]).collect();
        let bytes = sketch::encode(&Rgba8 { width: 48, height: 32, data }).unwrap();
        let s = sketch::decode(&bytes).unwrap();
        for px in s.grid(4, 4).into_iter().chain([s.mean()]) {
            for c in 0..3 {
                assert!(px[c].abs_diff(colour[c]) <= 6, "{colour:?} came back as {px:?}");
            }
            assert_eq!(px[3], 255);
        }
    }
}

#[test]
fn the_same_photo_gives_the_same_bytes() {
    for (_, photo) in photos().into_iter().step_by(7) {
        let a = sketch::encode(&photo).unwrap();
        let b = std::thread::spawn(move || sketch::encode(&photo).unwrap()).join().unwrap();
        assert_eq!(a, b);
    }
    // Pinned: the encoder uses only + − × ÷ √ and rounding, and scenes render the same pixels
    // everywhere, so these bytes are the same on every platform. A change here is a format change.
    let bytes = sketch::encode(&rgba(&Scene::new(7, 256, 171).with_kind(Kind::Horizon))).unwrap();
    assert_eq!(hex(&bytes), GOLDEN);
}

const GOLDEN: &str = "3101ce280415eaeeee12ef0eeed0ec6eefe2d0ee1c8fb000";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_decoded_jpeg_thumbnail_sketches_like_its_pixels() {
    let scene = Scene::new(11, 640, 427).with_kind(Kind::Blobs);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg");
    std::fs::write(&path, dalica_scenes::jpeg(&scene, 92, None).unwrap()).unwrap();
    let thumb = dalica_thumbs::thumbnail(&path, 128, &dalica_image::Limits::default()).unwrap();
    let from_jpeg = sketch::encode(&thumb).unwrap();
    let (max, mean) = colour_error(&rgba(&scene), &from_jpeg);
    assert!(max <= CELL_MAX && mean <= CELL_MEAN, "cell error max {max:.1}, mean {mean:.1}");
    let aspect = sketch::decode(&from_jpeg).unwrap().aspect();
    assert!((aspect / (640.0 / 427.0) - 1.0).abs() < 0.05);
}

#[test]
fn alpha_weights_the_colours_and_the_sketch_is_opaque() {
    // Left half transparent red, right half opaque blue: the sketch is blue all over.
    let (w, h) = (32u32, 16u32);
    let data = (0..w * h).flat_map(|i| if i % w < w / 2 { [255, 0, 0, 0] } else { [0, 0, 255, 255] }).collect();
    let s = sketch::decode(&sketch::encode(&Rgba8 { width: w, height: h, data }).unwrap()).unwrap();
    for px in s.grid(4, 2) {
        assert!(px[2] > 200 && px[0] < 50 && px[3] == 255, "{px:?}");
    }
    // Nothing visible at all: mid grey.
    let clear = Rgba8 { width: 8, height: 8, data: vec![0; 8 * 8 * 4] };
    let mean = sketch::decode(&sketch::encode(&clear).unwrap()).unwrap().mean();
    assert!(mean[..3].iter().all(|&c| c.abs_diff(128) <= 4), "{mean:?}");
}

#[test]
fn tiny_and_extreme_shapes_sketch() {
    for (w, h) in [(1, 1), (2, 1), (1, 3), (3, 3), (1000, 1), (1, 1000), (5000, 20), (7, 7)] {
        let data = (0..w * h).flat_map(|i| [(i * 37 % 256) as u8, (i * 11 % 256) as u8, 90, 255]).collect();
        let bytes = sketch::encode(&Rgba8 { width: w, height: h, data }).unwrap();
        assert!(bytes.len() <= MAX_BYTES);
        let s = sketch::decode(&bytes).unwrap();
        let ratio = f64::from(w) / f64::from(h);
        assert!((s.aspect() / ratio - 1.0).abs() < 0.05 || !(1.0 / 256.0..=200.0).contains(&ratio), "{w} × {h}: {}", s.aspect());
        let img = s.to_rgba(DEFAULT_DECODE_SIDE);
        assert_eq!(img.data.len() as u32, img.width * img.height * 4);
    }
}

#[test]
fn bad_images_are_errors() {
    let bad = [
        Rgba8 { width: 0, height: 0, data: vec![] },
        Rgba8 { width: 0, height: 4, data: vec![] },
        Rgba8 { width: 4, height: 4, data: vec![0; 63] },
        Rgba8 { width: 4, height: 4, data: vec![0; 65] },
        Rgba8 { width: u32::MAX, height: u32::MAX, data: vec![0; 16] },
    ];
    for img in bad {
        assert!(matches!(sketch::encode(&img), Err(Error::Image { .. })), "{}x{}", img.width, img.height);
    }
}

#[test]
fn decoded_sizes_are_clamped() {
    let photo = rgba(&Scene::new(3, 300, 200));
    let s = sketch::decode(&sketch::encode(&photo).unwrap()).unwrap();
    assert_eq!(s.size(0), (1, 1));
    assert_eq!(s.size(32), (32, 21));
    assert_eq!(s.size(u32::MAX).0, MAX_DECODE_SIDE);
    assert_eq!(s.to_rgba(u32::MAX).width, MAX_DECODE_SIDE);
    assert_eq!(s.grid(0, 0).len(), 1);
    assert_eq!(s.grid(1000, 3).len(), MAX_DECODE_SIDE as usize * 3);
}

#[test]
fn malformed_sketches_are_errors() {
    let good = sketch::encode(&rgba(&Scene::new(5, 200, 200))).unwrap();
    let whole = sketch::decode(&good).unwrap();
    assert!((whole.aspect() - 1.0).abs() < 0.05, "{}", whole.aspect());
    for n in 0..good.len() {
        assert!(sketch::decode(&good[..n]).is_err(), "a sketch cut to {n} bytes decoded");
    }
    let mut long = good.clone();
    long.push(0);
    assert!(matches!(sketch::decode(&long), Err(Error::Length { .. })));
    let mut version = good.clone();
    version[0] = (version[0] & 0xf0) | 2;
    assert_eq!(sketch::decode(&version), Err(Error::Version(2)));
    // Bits 21..28 hold P's mean: code 127 is out of range.
    let mut mean = good.clone();
    mean[2] |= 0xe0;
    mean[3] |= 0x0f;
    assert_eq!(sketch::decode(&mean), Err(Error::Malformed("a mean colour is out of range")));
    // Bits 53..57 hold the first coefficient: code 15 is out of range.
    let mut coefficient = good.clone();
    coefficient[6] |= 0xe0;
    coefficient[7] |= 0x01;
    assert_eq!(sketch::decode(&coefficient), Err(Error::Malformed("a coefficient is out of range")));
    // A square sketch is 205 bits: the top three bits of the last byte are padding.
    let mut padding = good.clone();
    *padding.last_mut().unwrap() |= 0x80;
    assert_eq!(sketch::decode(&padding), Err(Error::Malformed("the padding is not zero")));
    assert_eq!(sketch::decode(&[]), Err(Error::Short(0)));
}

/// Encode and decode times per photo, printed for the PR (`cargo test --release -p dalica-thumbs
/// --test all -- --nocapture sketch::timings`). Not asserted: debug builds and busy machines vary.
#[test]
fn timings() {
    let thumbs: Vec<Rgba8> = photos().into_iter().map(|(_, p)| p).filter(|p| p.width.max(p.height) == 256).collect();
    // One round in debug builds keeps the suite fast; the PR numbers come from a release build.
    let rounds = if cfg!(debug_assertions) { 1 } else { 20u32 };
    let start = Instant::now();
    let mut sketches = Vec::new();
    for _ in 0..rounds {
        sketches = thumbs.iter().map(|p| sketch::encode(p).unwrap()).collect();
    }
    let encode = start.elapsed() / (rounds * thumbs.len() as u32);
    let per_sketch = |f: &dyn Fn(&[u8]) -> usize| {
        let start = Instant::now();
        let mut n = 0;
        for _ in 0..rounds * 20 {
            for b in &sketches {
                n += f(std::hint::black_box(b));
            }
        }
        assert!(n > 0);
        start.elapsed() / (rounds * 20 * sketches.len() as u32)
    };
    let decode = per_sketch(&|b| sketch::decode(b).map(|s| s.aspect() as usize + 1).unwrap());
    let grid = per_sketch(&|b| sketch::decode(b).unwrap().grid(4, 4).len());
    let small = per_sketch(&|b| sketch::decode(b).unwrap().to_rgba(16).data.len());
    let image = per_sketch(&|b| sketch::decode(b).unwrap().to_rgba(DEFAULT_DECODE_SIDE).data.len());
    println!(
        "sketch per 256 px photo ({} photos): encode {encode:?}; decode {decode:?}; + 4 × 4 grid {grid:?}; + 16 px image {small:?}; + 32 px image {image:?}",
        thumbs.len()
    );
}

proptest! {
    #![proptest_config(crate::common::cases(4096, 256))]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        if let Ok(s) = sketch::decode(&bytes) {
            prop_assert!(bytes.len() <= MAX_BYTES);
            let img = s.to_rgba(DEFAULT_DECODE_SIDE);
            prop_assert_eq!(img.data.len() as u32, img.width * img.height * 4);
            let _ = (s.mean(), s.grid(4, 4), s.aspect());
        }
    }

    #[test]
    fn plausible_headers_never_panic(head in any::<u16>(), tail in proptest::collection::vec(any::<u8>(), 0..40)) {
        // Version 1 with a random aspect, so most cases reach the coefficient checks.
        let mut bytes = vec![((head as u8) & 0xf0) | 1, (head >> 8) as u8];
        bytes.extend(tail);
        let _ = sketch::decode(&bytes).map(|s| s.to_rgba(MAX_DECODE_SIDE));
    }
}

proptest! {
    // Each case renders and encodes a photo first, so fewer cases.
    #![proptest_config(crate::common::cases(512, 32))]

    #[test]
    fn truncated_and_flipped_sketches_never_panic(seed in any::<u64>(), cut in 0usize..32, flip in any::<(usize, u8)>()) {
        let good = sketch::encode(&rgba(&Scene::new(seed, 48, 36))).unwrap();
        prop_assert!(sketch::decode(&good[..cut.min(good.len().saturating_sub(1))]).is_err());
        let mut bad = good.clone();
        let i = flip.0 % bad.len();
        bad[i] ^= flip.1;
        let _ = sketch::decode(&bad).map(|s| s.grid(8, 8));
    }
}
