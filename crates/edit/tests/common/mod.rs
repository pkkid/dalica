//! Helpers every operation's tests share (`architecture.md` §11): `dalica-scenes` photos as RGBA,
//! the two-size agreement check (render at two sizes, compare after a box downscale) and the
//! fingerprint check (`tests/fingerprints/<case>.json`; on a mismatch the actual PNG goes to
//! `target/test-output/edit/`, and `DALICA_BLESS=1` stores the new fingerprint: look at the PNGs
//! first).

#![allow(dead_code)]

use std::path::PathBuf;

use dalica_edit::{Recipe, Source, Target, render};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::{Rgb8, Scene, fingerprint, orient};

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A property test's case count: `n` in full mode, at most 32 in quick mode.
pub fn cases(n: u32) -> u32 {
    if full() { n } else { n.min(32) }
}

/// The geometric operations.
pub const GEOMETRY: [&str; 5] = ["rotate(90)", "rotate(180)", "rotate(270)", "flip_h()", "flip_v()"];

/// Every sequence of up to `max` operations from `GEOMETRY`, the empty one first.
pub fn recipes(max: usize) -> Vec<Vec<&'static str>> {
    let mut all = vec![vec![]];
    let mut last = vec![vec![]];
    for _ in 0..max {
        last = last.iter().flat_map(|r: &Vec<&str>| GEOMETRY.iter().map(move |op| [r.clone(), vec![*op]].concat())).collect();
        all.extend(last.iter().cloned());
    }
    all
}

/// `recipe` rendered at full size on the upright `img`.
pub fn run(img: &Rgba8, ops: &[&str]) -> Rgba8 {
    render(Source::upright(img), &recipe(ops), Target::Full).unwrap()
}

/// The RGB of each pixel.
pub fn pixels(img: &Rgba8) -> impl Iterator<Item = [u8; 3]> + '_ {
    img.data.chunks(4).map(|p| [p[0], p[1], p[2]])
}

/// The upright `up` as a file stores it under EXIF orientation `o`.
pub fn stored(up: &Rgba8, o: u8) -> Rgba8 {
    let rgb = Rgb8 { width: up.width, height: up.height, data: up.data.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect() };
    rgba(&orient::store(&rgb, o).unwrap())
}

pub fn exif(o: u8) -> Orientation {
    Orientation::new(o).unwrap()
}

/// The window `(x, y, w, h)` of `img`.
pub fn window(img: &Rgba8, x: u32, y: u32, w: u32, h: u32) -> Rgba8 {
    let mut data = Vec::new();
    for row in y..y + h {
        let at = ((row * img.width + x) * 4) as usize;
        data.extend_from_slice(&img.data[at..at + (w * 4) as usize]);
    }
    Rgba8 { width: w, height: h, data }
}

/// An RGB scene as opaque RGBA.
pub fn rgba(img: &Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() }
}

/// The `scenes` photo of `seed` at `width × height`, upright, as RGBA.
pub fn scene(seed: u64, width: u32, height: u32) -> Rgba8 {
    rgba(&Scene::new(seed, width, height).render().expect("a valid scene size"))
}

/// A recipe from operation strings.
pub fn recipe(ops: &[&str]) -> Recipe {
    Recipe::parse(ops).expect("a recipe under the limit")
}

/// `img` reduced to `width × height` by averaging the source area under each output pixel (an
/// independent reference for comparing sizes; not the renderer's resampler).
pub fn box_downscale(img: &Rgba8, width: u32, height: u32) -> Rgba8 {
    let (sw, sh) = (f64::from(img.width), f64::from(img.height));
    let (fx, fy) = (sw / f64::from(width), sh / f64::from(height));
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (x0, x1) = (f64::from(x) * fx, f64::from(x + 1) * fx);
            let (y0, y1) = (f64::from(y) * fy, f64::from(y + 1) * fy);
            let mut sum = [0.0f64; 4];
            let mut total = 0.0;
            for sy in y0.floor() as u32..(y1.ceil() as u32).min(img.height) {
                let wy = (y1.min(f64::from(sy + 1)) - y0.max(f64::from(sy))).max(0.0);
                for sx in x0.floor() as u32..(x1.ceil() as u32).min(img.width) {
                    let wx = (x1.min(f64::from(sx + 1)) - x0.max(f64::from(sx))).max(0.0);
                    let at = ((sy * img.width + sx) * 4) as usize;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += f64::from(img.data[at + c]) * wx * wy;
                    }
                    total += wx * wy;
                }
            }
            data.extend(sum.map(|v| (v / total.max(1e-12)).round().clamp(0.0, 255.0) as u8));
        }
    }
    Rgba8 { width, height, data }
}

/// Renders `recipe` on `source` fitted into `small` and into `large` pixels, and checks that the
/// large render, box-downscaled to the small one's size, matches it within `tolerance` (the
/// largest difference of a fingerprint cell's mean or deviation, 0–255). Returns the small render.
pub fn assert_two_sizes_agree(source: Source<'_>, recipe: &Recipe, small: u32, large: u32, tolerance: f64) -> Rgba8 {
    let a = render(source, recipe, Target::Fit { width: small, height: small }).expect("the small render");
    let b = render(source, recipe, Target::Fit { width: large, height: large }).expect("the large render");
    let b_small = box_downscale(&b, a.width, a.height);
    let (fa, fb) = (fingerprint::fingerprint(a.width, a.height, &a.data), fingerprint::fingerprint(a.width, a.height, &b_small.data));
    let d = fingerprint::distance(&fa, &fb);
    if d > tolerance {
        let dir = out_dir();
        std::fs::write(dir.join("two-sizes-small.png"), fingerprint::png_rgba(a.width, a.height, &a.data)).unwrap();
        std::fs::write(dir.join("two-sizes-large.png"), fingerprint::png_rgba(b_small.width, b_small.height, &b_small.data)).unwrap();
        panic!(
            "{:?}: {small} px and {large} px renders differ by {d:.2} after downscale (tolerance {tolerance}; PNGs in {})",
            recipe.print(),
            dir.display()
        );
    }
    a
}

/// `target/test-output/edit/`, created.
pub fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/edit");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks `img` against `tests/fingerprints/<case>.json` within `tolerance`, or stores it under
/// `DALICA_BLESS=1`. The image is always written to `target/test-output/edit/<case>.png`.
pub fn check_fingerprint(case: &str, img: &Rgba8, tolerance: f64) {
    std::fs::write(out_dir().join(format!("{case}.png")), fingerprint::png_rgba(img.width, img.height, &img.data)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{case}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
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
