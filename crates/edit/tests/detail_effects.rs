//! The detail effects of tab 3 (`editing.md` §8.2, M12.7): Sharpen, Film Grain and Glow. Each has a
//! fingerprint at its defaults (`fingerprints/detail-*.json`, a 1:1 window of a large photo, where
//! sharpening and grain show) and renders the same at two sizes after a downscale and in a region
//! as in the whole render. Invariants: Sharpen 0 and Grain 0 are the identity; Sharpen leaves a
//! flat patch alone and raises an edge's contrast; Grain keeps the mean luma within one level,
//! adds nothing to R − G and, with Lighten, never darkens; Glow never darkens a pixel and leaves a
//! dark flat patch alone.

use crate::common::{assert_two_sizes_agree, check_fingerprint, pixels, recipe, run, scene, window};
use dalica_edit::ops::tone::luma;
use dalica_edit::{Region, Source, Target, render, render_region};
use dalica_image::Rgba8;

/// The three effects at their defaults, with their fingerprints' names.
const DEFAULTS: [(&str, &str); 3] = [("detail-sharpen", "sharpen(0.6)"), ("detail-grain", "grain(10,false)"), ("detail-glow", "glow(0.65,3)")];

/// A large photo (shorter side 2000 px: Sharpen's sigma 0.8 px, the grain's cells 1 px at 1:1).
fn large() -> Rgba8 {
    scene(12, 3000, 2000)
}

/// `w × h` pixels of `f(x, y)` (grey levels).
fn grey(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Rgba8 {
    let data = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| [f(x, y); 3].into_iter().chain([255])).collect();
    Rgba8 { width: w, height: h, data }
}

fn at(img: &Rgba8, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * img.width + x) * 4) as usize;
    [img.data[i], img.data[i + 1], img.data[i + 2]]
}

fn mean_luma(img: &Rgba8) -> f64 {
    let n = f64::from(img.width * img.height);
    pixels(img).map(|p| f64::from(luma(p.map(|v| f32::from(v) / 255.0)))).sum::<f64>() / n * 255.0
}

/// At 1:1 a window of each effect equals that window of the whole render, exactly (Sharpen's taps,
/// the grain placed in the stored frame, Glow's blur); the window is the fingerprint.
#[test]
fn each_effect_at_its_defaults_matches_its_fingerprint_and_a_region_equals_the_whole() {
    let img = large();
    // The window holds the photo's white square (sharp edges) and the gradient around it.
    let (x, y, w, h) = (40, 40, 320, 240);
    for (case, op) in DEFAULTS {
        let r = recipe(&[op]);
        let part = render_region(Source::upright(&img), &r, Region { scale: 1.0, x, y, width: w, height: h }).unwrap();
        let whole = render(Source::upright(&img), &r, Target::Full).unwrap();
        assert!(part == window(&whole, x, y, w, h), "{op}: the region differs from the whole render");
        assert!(part != window(&img, x, y, w, h), "{op} changes the picture");
        check_fingerprint(case, &part, 0.5);
    }
}

/// A large render shrunk to a small one's size matches the small render: the sizes are relative to
/// the picture (the grain fades where a pixel spans several of its cells, and Sharpen's least
/// sigma keeps a small render's sharpening within the tolerance at its default amount).
#[test]
fn each_effect_renders_the_same_at_two_sizes() {
    let img = scene(12, 1600, 1067);
    for (_, op) in DEFAULTS {
        assert_two_sizes_agree(Source::upright(&img), &recipe(&[op]), 240, 960, 2.0);
    }
    // Sharpen under 1:1 works at the render's pixel scale (`MIN_SIGMA_PX`), so only its default
    // amount is held to the large render shrunk; Grain and Glow at their extremes too.
    for op in ["grain(50,false)", "grain(50,true)", "glow(1,30)", "glow(1,0.3)"] {
        assert_two_sizes_agree(Source::upright(&img), &recipe(&[op]), 240, 960, 2.0);
    }
}

#[test]
fn sharpen_0_and_grain_0_are_the_identity() {
    let img = scene(3, 900, 600);
    let plain = run(&img, &[]);
    for op in ["sharpen(0)", "grain(0,false)", "grain(0,true)", "glow(0,3)"] {
        assert_eq!(run(&img, &[op]), plain, "{op}");
    }
}

#[test]
fn sharpen_leaves_a_flat_patch_alone_and_raises_an_edges_contrast() {
    // Two flat halves, 77 and 179, on a 1200 × 800 picture (sigma 0.5 px, five taps).
    let img = grey(1200, 800, |x, _| if x < 600 { 77 } else { 179 });
    let out = run(&img, &["sharpen(1)"]);
    for y in [0, 400, 799] {
        for x in (0..1200u32).filter(|x| x.abs_diff(600) > 4) {
            assert_eq!(at(&out, x, y), at(&img, x, y), "flat at ({x}, {y})");
        }
        let (dark, light) = (at(&out, 599, y)[0], at(&out, 600, y)[0]);
        assert!(dark < 77 && light > 179, "edge at row {y}: {dark}, {light}");
    }
    // Stronger with the amount.
    let more = run(&img, &["sharpen(3)"]);
    assert!(at(&more, 599, 400)[0] < at(&out, 599, 400)[0] && at(&more, 600, 400)[0] > at(&out, 600, 400)[0]);
}

#[test]
fn grain_keeps_the_mean_luma_adds_no_colour_and_lighten_never_darkens() {
    let img = large();
    let plain = run(&img, &[]);
    let out = run(&img, &["grain(10,false)"]);
    assert!((mean_luma(&out) - mean_luma(&plain)).abs() <= 1.0, "{} vs {}", mean_luma(&out), mean_luma(&plain));
    // The same noise in every channel: R − G and G − B as they were wherever nothing clipped.
    let (mut same, mut total, mut moved) = (0usize, 0usize, 0usize);
    for (a, b) in pixels(&plain).zip(pixels(&out)) {
        if b.iter().any(|v| *v == 0 || *v == 255) {
            continue;
        }
        total += 1;
        moved += usize::from(a != b);
        let d = |p: [u8; 3]| (i16::from(p[0]) - i16::from(p[1]), i16::from(p[1]) - i16::from(p[2]));
        let (da, db) = (d(a), d(b));
        assert!((da.0 - db.0).abs() <= 1 && (da.1 - db.1).abs() <= 1, "{a:?} → {b:?}");
        same += usize::from(da == db);
    }
    assert!(same as f64 >= total as f64 * 0.999, "{same} of {total}");
    assert!(moved as f64 >= total as f64 * 0.5, "grain on {moved} of {total}");
    // Lighten adds only light grain.
    let light = run(&img, &["grain(10,true)"]);
    assert!(pixels(&plain).zip(pixels(&light)).all(|(a, b)| (0..3).all(|c| b[c] >= a[c])));
    assert!(mean_luma(&light) > mean_luma(&plain) + 1.0);
}

#[test]
fn glow_never_darkens_and_leaves_a_dark_flat_patch_alone() {
    let img = scene(4, 900, 600);
    let plain = run(&img, &[]);
    for op in ["glow(0.65,3)", "glow(1,30)", "glow(1,0.3)"] {
        let out = run(&img, &[op]);
        assert!(pixels(&plain).zip(pixels(&out)).all(|(a, b)| (0..3).all(|c| b[c] >= a[c])), "{op}");
        assert!(mean_luma(&out) > mean_luma(&plain), "{op} glows");
    }
    // A dark flat field (level 110) with a white square in its right part: the field away from the
    // square's glow is unchanged, and the glow brightens the field next to the square.
    let img = grey(900, 600, |x, y| if (700..800).contains(&x) && (250..350).contains(&y) { 255 } else { 110 });
    let out = run(&img, &["glow(0.65,3)"]);
    // Sigma 18 px: the blur reaches about 3 sigma.
    for y in (0..600).step_by(7) {
        for x in (0..600).step_by(5) {
            assert_eq!(at(&out, x, y), [110; 3], "({x}, {y})");
        }
    }
    assert!(at(&out, 690, 300)[0] > 110, "{:?}", at(&out, 690, 300));
}
