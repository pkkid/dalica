//! Helpers the `create` tests share: `dalica-scenes` photos as files (JPEG, PNG at 8 or 16 bits,
//! with or without alpha), decoding what the pipeline wrote, comparing pictures, and the
//! fingerprint check (`tests/fingerprints/<case>.json`; the image always goes to
//! `target/test-output/create/<case>.png`, and `DALICA_BLESS=1` stores a new fingerprint: look at
//! the PNG first).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use dalica_core::Timestamp;
use dalica_create::save::{Options, Seen};
use dalica_edit::{Recipe, Source, Target};
use dalica_fs::TempDir;
use dalica_image::encode::{self, Channels, Image, Metadata, Pixels};
use dalica_image::{Decoded, Limits, Rgba8, formats, read_header};
use dalica_scenes::{Exif, Rgb8, Scene, fingerprint};

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A property test's case count: `n` in full mode, at most 32 in quick mode.
pub fn cases(n: u32) -> u32 {
    if full() { n } else { n.min(32) }
}

/// The photo's name in the Save tests' folders.
pub const NAME: &str = "IMG_0001.jpg";
/// The time of every Save here, so two runs write the same bytes.
pub const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

/// A fresh temporary folder.
pub fn folder() -> TempDir {
    TempDir::new("dalica-create-test").unwrap()
}

/// Save options at `NOW` on two threads.
pub fn options() -> Options {
    Options { threads: 2, ..Options::new(NOW) }
}

pub fn read(path: impl AsRef<Path>) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

/// The file at `path` as the library sees it.
pub fn seen(path: &Path) -> Seen {
    let meta = std::fs::metadata(path).unwrap();
    Seen { size: meta.len(), mtime: Timestamp::from_system_time(meta.modified().unwrap()) }
}

pub fn meta(bytes: &[u8]) -> dalica_meta::Metadata {
    dalica_meta::read(bytes, &dalica_meta::Limits::default()).unwrap()
}

/// An RGB scene as opaque RGBA.
pub fn rgba(img: &Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() }
}

/// The `scenes` photo of `seed` at `width × height`.
pub fn scene(seed: u64, width: u32, height: u32) -> Scene {
    Scene::new(seed, width, height)
}

/// The scene's pixels, upright, as RGBA.
pub fn pixels(seed: u64, width: u32, height: u32) -> Rgba8 {
    rgba(&scene(seed, width, height).render().unwrap())
}

/// A JPEG of the scene at `quality`, with EXIF orientation `orientation` (the stored pixels are
/// the scene as rendered; the file says how to turn them).
pub fn jpeg(seed: u64, width: u32, height: u32, quality: u8, orientation: u8) -> Vec<u8> {
    let exif = Exif { orientation, make: Some("Dalicam".into()), model: Some("Model 7".into()), ..Exif::default() };
    dalica_scenes::jpeg(&scene(seed, width, height), quality, Some(&exif)).unwrap()
}

/// An 8-bit RGB PNG of the scene.
pub fn png(seed: u64, width: u32, height: u32) -> Vec<u8> {
    dalica_scenes::png(&scene(seed, width, height)).unwrap()
}

/// The scene with a left-to-right alpha ramp (0 at the left edge, 255 at the right).
pub fn with_alpha(seed: u64, width: u32, height: u32) -> Rgba8 {
    let mut img = pixels(seed, width, height);
    for (i, p) in img.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (i as u32 % width) as u64;
        p[3] = (x * 255 / u64::from(width.max(2) - 1)) as u8;
    }
    img
}

/// An 8-bit RGBA PNG of `img`.
pub fn png_rgba(img: &Rgba8) -> Vec<u8> {
    encode::png(&Image::rgba8(img), &Metadata::default()).unwrap()
}

/// 16-bit samples of `img` (`v × 257` plus a position pattern in the low byte, so dropping it
/// shows), with or without its alpha.
pub fn deep_samples(img: &Rgba8, alpha: bool) -> Vec<u16> {
    let keep = if alpha { 4 } else { 3 };
    img.data
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p[..keep].iter().enumerate().map(move |(c, &v)| (u16::from(v) << 8) | ((i * 7 + c * 3) % 256) as u16).collect::<Vec<_>>())
        .collect()
}

/// A 16-bit PNG of `img` (RGB, or RGBA when `alpha`), optionally with an ICC profile.
pub fn png16(img: &Rgba8, alpha: bool, icc: Option<&[u8]>) -> Vec<u8> {
    let samples = deep_samples(img, alpha);
    let channels = if alpha { Channels::Rgba } else { Channels::Rgb };
    let image = Image { width: img.width, height: img.height, channels, pixels: Pixels::U16(&samples) };
    encode::png(&image, &Metadata { icc, ..Metadata::default() }).unwrap()
}

/// A recipe from operation strings.
pub fn recipe(ops: &[&str]) -> Recipe {
    Recipe::parse(ops).unwrap()
}

/// `bytes` decoded upright to 8-bit sRGB RGBA, as the library shows it.
pub fn decode(bytes: &[u8]) -> Rgba8 {
    dalica_image::decode(bytes, &Limits::default()).unwrap()
}

/// `bytes` decoded to the stored samples, before the colour stage and orientation.
pub fn decode_stored(bytes: &[u8]) -> Decoded {
    let limits = Limits::default();
    let header = read_header(bytes, &limits).unwrap();
    let mut budget = dalica_image::Budget::new(&limits, header.width, header.height);
    formats::decoder(header.format)(bytes, &header, &mut budget, &limits).unwrap()
}

/// What `edit::render` makes of the file `bytes` with `recipe` at full size (the oracle the
/// pipeline must match).
pub fn oracle(bytes: &[u8], recipe: &Recipe) -> Rgba8 {
    let limits = Limits::default();
    let header = read_header(bytes, &limits).unwrap();
    let stored = {
        // The stored (unturned) pixels: decode upright, then undo the EXIF orientation.
        let upright = decode(bytes);
        dalica_edit::Dihedral::from_exif(header.orientation).inverse().apply(upright).unwrap()
    };
    dalica_edit::render(Source::new(&stored, header.orientation), recipe, Target::Full).unwrap()
}

/// The largest difference of any sample between two pictures of the same size, and the mean
/// absolute difference.
pub fn difference(a: &Rgba8, b: &Rgba8) -> (u8, f64) {
    assert_eq!((a.width, a.height), (b.width, b.height), "the pictures differ in size");
    let mut max = 0u8;
    let mut sum = 0u64;
    for (x, y) in a.data.iter().zip(&b.data) {
        let d = x.abs_diff(*y);
        max = max.max(d);
        sum += u64::from(d);
    }
    (max, sum as f64 / a.data.len().max(1) as f64)
}

/// Peak signal-to-noise ratio of the colour channels, in dB (infinite when equal).
pub fn psnr(a: &Rgba8, b: &Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height), "the pictures differ in size");
    let mut sum = 0f64;
    let mut n = 0f64;
    for (p, q) in a.data.as_chunks::<4>().0.iter().zip(b.data.as_chunks::<4>().0) {
        for c in 0..3 {
            let d = f64::from(p[c]) - f64::from(q[c]);
            sum += d * d;
            n += 1.0;
        }
    }
    let mse = sum / n.max(1.0);
    if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

/// `target/test-output/create/`, created.
pub fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/create");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks `img` against `tests/fingerprints/<case>.json` within `tolerance`, or stores it under
/// `DALICA_BLESS=1`. The image is always written to `target/test-output/create/<case>.png`.
pub fn check_fingerprint(case: &str, img: &Rgba8, tolerance: f64) {
    std::fs::write(out_dir().join(format!("{case}.png")), fingerprint::png_rgba(img.width, img.height, &img.data)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{case}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::create_dir_all(stored.parent().unwrap()).unwrap();
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(img.width, img.height, &img.data)) + "\n").unwrap();
        println!("blessed {}", stored.display());
    }
    let text = std::fs::read_to_string(&stored)
        .unwrap_or_else(|e| panic!("{case}: no fingerprint at {} ({e}); bless it with DALICA_BLESS=1", stored.display()));
    let expected = fingerprint::from_json(&text).unwrap();
    if let Err(m) = fingerprint::check(img.width, img.height, &img.data, &expected, tolerance) {
        std::fs::write(out_dir().join(format!("{case}-actual.png")), &m.actual_png).unwrap();
        panic!("{case}: {}", m.message);
    }
}
