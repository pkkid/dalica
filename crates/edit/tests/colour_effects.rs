//! The colour effects of tab 3 (`editing.md` §8.2, M12.6): Sepia, B&W, Warmify, Tint, Saturation
//! and Filtered B&W. Each renders (`VERSION` 1), matches its fingerprint at its defaults
//! (`fingerprints/colour-*.json`), renders the same at two sizes after a downscale and in a zoomed
//! region as in the whole render; then one invariant per effect: B&W and Filtered B&W give
//! R = G = B, B&W's grey is the Rec.601 luma within one level, a red filter lightens a red patch
//! over a blue one of the same luma; Sepia gives R ≥ G ≥ B and keeps the order of lumas; Warmify
//! raises R − B and keeps the luma within 3 %; Tint gives every coloured pixel the tint's hue and
//! Fade 100 gives the input; Saturation 0 is the identity and −1 gives R = G = B. Warmify and
//! Saturation also match fingerprints of a large photo rendered whole (`colour-*-large`, blessed
//! from the M12 code before M13.38 made their passes faster).

use crate::common::{assert_two_sizes_agree, check_fingerprint, pixels, recipe, rgba, run, scene, window};
use dalica_edit::ops::colour::REC601;
use dalica_edit::{Op, Region, Source, Target, render, render_region};
use dalica_image::Rgba8;
use dalica_scenes::{Kind, Scene};

/// Each effect's fingerprint case and its entry at its defaults.
const DEFAULTS: [(&str, &str); 6] = [
    ("colour-sepia", "sepia()"),
    ("colour-bw", "bw()"),
    ("colour-warmify", "warmify()"),
    ("colour-tint", "tint(#80cfff,0)"),
    ("colour-saturation", "saturation(0.16)"),
    ("colour-filtered-bw", "filtered_bw(#ff8000)"),
];

/// Coloured blobs over a gradient: every hue, light and dark.
fn blobs(seed: u64, width: u32, height: u32) -> Rgba8 {
    rgba(&Scene::new(seed, width, height).with_kind(Kind::Blobs).render().unwrap())
}

/// Every 8-bit colour on a coarse grid (step 17: 16³ colours), as one picture.
fn colour_cube() -> Rgba8 {
    let data: Vec<u8> = (0..16u8).flat_map(|r| (0..16u8).flat_map(move |g| (0..16u8).flat_map(move |b| [r * 17, g * 17, b * 17, 255]))).collect();
    Rgba8 { width: 64, height: 64, data }
}

fn luma(p: [u8; 3]) -> f64 {
    (0..3).map(|c| f64::from(REC601[c]) * f64::from(p[c])).sum()
}

/// The hue of `p` in turns (0..1); `p` is not grey.
fn hue(p: [f32; 3]) -> f32 {
    let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
    let d = mx - mn;
    let h = if mx == p[0] {
        ((p[1] - p[2]) / d).rem_euclid(6.0)
    } else if mx == p[1] {
        (p[2] - p[0]) / d + 2.0
    } else {
        (p[0] - p[1]) / d + 4.0
    };
    h / 6.0
}

fn is_grey(p: [u8; 3]) -> bool {
    p[0] == p[1] && p[1] == p[2]
}

#[test]
fn every_colour_effect_renders() {
    let img = blobs(1, 60, 40);
    for (_, entry) in DEFAULTS {
        let op = Op::parse(entry);
        assert_eq!(op.version(), Some(1), "{entry}");
        assert!(matches!(op.typed(), Some(Ok(_))), "{entry}: typed for rendering");
        assert_ne!(run(&img, &[entry]), img, "{entry} changes the photo");
    }
}

#[test]
fn each_effect_matches_its_fingerprint_at_two_sizes_and_in_a_region() {
    let up = blobs(7, 300, 200);
    for (case, entry) in DEFAULTS {
        let r = recipe(&[entry]);
        // Each effect maps each pixel on its own, but not along a straight line: on the scene's
        // two-pixel bright spot the mean of the mapped pixels (the large render downscaled) and
        // the mapped mean (the small render) part a little. Tint's line from the tint to white
        // bends at the tint's own luma, inside that spot (2.3 there).
        let tolerance = if entry.starts_with("tint") { 3.0 } else { 2.0 };
        let small = assert_two_sizes_agree(Source::upright(&up), &r, 64, 240, tolerance);
        check_fingerprint(case, &small, 0.5);
        // A zoomed region is that window of the whole render, under a crop and a turn too.
        for r in [r.clone(), recipe(&["crop(0.1,0.05,0.9,0.8)", entry, "rotate(90)"])] {
            let whole = render(Source::upright(&up), &r, Target::Scale(1.7)).unwrap();
            for (x, y, w, h) in [(0, 0, 31, 17), (whole.width / 3, whole.height / 2, 40, 25), (whole.width - 9, whole.height - 7, 9, 7)] {
                let part = render_region(Source::upright(&up), &r, Region { scale: 1.7, x, y, width: w, height: h }).unwrap();
                assert_eq!(part, window(&whole, x, y, w, h), "{entry}: region at {x},{y}");
            }
        }
    }
}

/// A large photo rendered whole with Warmify and with Saturation matches its fingerprint
/// (`colour-warmify-large`, `colour-saturation-large`, blessed from the M12 per-pixel maths): the
/// 3-D tables the passes read since M13.38 give the same picture at full resolution.
#[test]
fn warmify_and_saturation_match_their_large_fingerprints() {
    let img = blobs(13, 1200, 800);
    for (case, entry) in [("colour-warmify-large", "warmify()"), ("colour-saturation-large", "saturation(0.16)")] {
        check_fingerprint(case, &render(Source::upright(&img), &recipe(&[entry]), Target::Full).unwrap(), 0.5);
    }
}

#[test]
fn bw_and_filtered_bw_are_grey_and_bw_is_the_rec601_luma() {
    let sources = [blobs(3, 120, 80), scene(4, 120, 80), colour_cube()];
    for src in &sources {
        let out = run(src, &["bw()"]);
        for (before, after) in pixels(src).zip(pixels(&out)) {
            assert!(is_grey(after), "bw(): {before:?} → {after:?}");
            assert!((f64::from(after[0]) - luma(before)).abs() <= 1.0, "bw(): {before:?} → {after:?}, luma {}", luma(before));
        }
        for entry in [
            "filtered_bw(#ff8000)",
            "filtered_bw(#ff0000)",
            "filtered_bw(#00ff00)",
            "filtered_bw(#0000ff)",
            "filtered_bw(#000000)",
            "filtered_bw(#ffffff)",
        ] {
            let out = run(src, &[entry]);
            assert!(pixels(&out).all(is_grey), "{entry}");
        }
        // A white filter is no filter, and a black one lets nothing choose: both are plain B&W.
        assert_eq!(run(src, &["filtered_bw(#ffffff)"]), run(src, &["bw()"]));
        assert_eq!(run(src, &["filtered_bw(#000000)"]), run(src, &["bw()"]));
    }
}

#[test]
fn a_red_filter_lightens_red_over_a_blue_of_the_same_luma() {
    // A red and a blue patch with the same Rec.601 luma (about 96), and a grey of it.
    let (red, blue, grey) = ([255u8, 33, 0], [0u8, 113, 255], [96u8, 96, 96]);
    assert!((luma(red) - luma(blue)).abs() < 1.0 && (luma(red) - luma(grey)).abs() < 1.0);
    let data = [red, blue, grey].iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    let img = Rgba8 { width: 3, height: 1, data };
    let bw: Vec<_> = pixels(&run(&img, &["bw()"])).collect();
    assert!(bw[0][0].abs_diff(bw[1][0]) <= 1, "plain B&W sees the same grey: {bw:?}");
    for filter in ["#ff0000", "#ff8000"] {
        let out: Vec<_> = pixels(&run(&img, &[&format!("filtered_bw({filter})")])).collect();
        assert!(out[0][0] > out[1][0] + 40, "{filter}: red {:?} over blue {:?}", out[0], out[1]);
        assert_eq!(out[2], grey, "{filter}: a grey stays the same grey");
    }
    let out: Vec<_> = pixels(&run(&img, &["filtered_bw(#0000ff)"])).collect();
    assert!(out[1][0] > out[0][0] + 40, "a blue filter the other way: {out:?}");
}

#[test]
fn sepia_is_warm_brown_and_keeps_the_order_of_lumas() {
    for src in [blobs(5, 120, 80), colour_cube()] {
        let out = run(&src, &["sepia()"]);
        let mut pairs: Vec<(f64, [u8; 3])> = pixels(&src).map(luma).zip(pixels(&out)).collect();
        for (_, p) in &pairs {
            assert!(p[0] >= p[1] && p[1] >= p[2], "sepia(): {p:?}");
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        for w in pairs.windows(2) {
            // Equal lumas may round apart by a level; a darker input never comes out lighter.
            if w[1].0 - w[0].0 > 0.5 {
                assert!(luma(w[1].1) >= luma(w[0].1), "luma {} → {:?} but {} → {:?}", w[0].0, w[0].1, w[1].0, w[1].1);
            }
        }
        // Brown: the mid-tones are clearly warm.
        let mid = pairs.iter().find(|(l, _)| (110.0..140.0).contains(l)).unwrap().1;
        assert!(mid[0] > mid[2] + 30, "{mid:?}");
    }
    // Black and white stay.
    let ends = Rgba8 { width: 2, height: 1, data: vec![0, 0, 0, 255, 255, 255, 255, 255] };
    assert_eq!(run(&ends, &["sepia()"]), ends);
}

#[test]
fn warmify_raises_red_over_blue_and_keeps_the_luma() {
    // On photos no pixel gets cooler; on every colour of the cube, saturated ones at the gamut's
    // edge too (where a channel at 255 cannot grow), the luma holds and the whole gets warmer.
    for (src, per_pixel) in [(blobs(6, 120, 80), true), (scene(8, 120, 80), true), (scene(12, 120, 80), true), (colour_cube(), false)] {
        let out = run(&src, &["warmify()"]);
        let (mut before_rb, mut after_rb) = (0i64, 0i64);
        for (a, b) in pixels(&src).zip(pixels(&out)) {
            let (ra, rb) = (i64::from(a[0]) - i64::from(a[2]), i64::from(b[0]) - i64::from(b[2]));
            assert!(!per_pixel || rb >= ra, "warmify(): {a:?} → {b:?}");
            assert!((luma(b) - luma(a)).abs() <= 0.03 * 255.0, "warmify(): {a:?} → {b:?}: luma {} → {}", luma(a), luma(b));
            before_rb += ra;
            after_rb += rb;
        }
        let n = i64::from(src.width * src.height);
        assert!(after_rb > before_rb + 8 * n, "R − B rises by {} levels on average", (after_rb - before_rb) / n);
    }
}

#[test]
fn tint_gives_its_hue_to_every_coloured_pixel_and_fade_100_is_the_input() {
    let src = blobs(9, 120, 80);
    for colour in [[0x80u8, 0xcf, 0xff], [0xc0, 0x40, 0x20], [0x30, 0xa0, 0x30]] {
        let hex = format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2]);
        let tint_hue = hue(colour.map(|v| f32::from(v) / 255.0));
        let out = run(&src, &[&format!("tint({hex},0)")]);
        let mut coloured = 0;
        for p in pixels(&out) {
            let spread = p.iter().max().unwrap() - p.iter().min().unwrap();
            if spread < 24 {
                continue; // too close to grey for 8 bits to carry a hue precisely
            }
            coloured += 1;
            let h = hue(p.map(|v| f32::from(v) / 255.0));
            let off = ((h - tint_hue).rem_euclid(1.0)).min((tint_hue - h).rem_euclid(1.0)) * 360.0;
            assert!(off <= 3.0, "{hex}: {p:?} is {off}° off the tint's hue");
        }
        assert!(coloured > out.data.len() / 4 / 2, "{hex}: most pixels are coloured ({coloured})");
        assert_eq!(run(&src, &[&format!("tint({hex},100)")]), src, "{hex}: Fade 100 is the input");
        // Fade 50 lies between the input and the tint.
        let half = run(&src, &[&format!("tint({hex},50)")]);
        for ((a, b), m) in pixels(&src).zip(pixels(&out)).zip(pixels(&half)) {
            for c in 0..3 {
                assert!(m[c] >= a[c].min(b[c]).saturating_sub(1) && m[c] <= a[c].max(b[c]).saturating_add(1), "{a:?} {b:?} {m:?}");
            }
        }
    }
}

#[test]
fn saturation_0_is_the_identity_and_minus_1_is_grey() {
    for src in [blobs(10, 120, 80), colour_cube()] {
        assert_eq!(run(&src, &["saturation(0)"]), src);
        assert!(pixels(&run(&src, &["saturation(-1)"])).all(is_grey));
        assert_eq!(run(&src, &["saturation(-1)"]), run(&src, &["bw()"]));
        // In between, colours are duller below 0 and livelier above.
        let spread = |img: &Rgba8| pixels(img).map(|p| u64::from(p.iter().max().unwrap() - p.iter().min().unwrap())).sum::<u64>();
        let base = spread(&src);
        assert!(spread(&run(&src, &["saturation(-0.5)"])) < base);
        let (low, high) = (spread(&run(&src, &["saturation(0.16)"])), spread(&run(&src, &["saturation(1)"])));
        assert!(base < low && low < high, "{base} < {low} < {high}");
    }
}

#[test]
fn hostile_values_render_as_their_clamped_entries() {
    let img = blobs(11, 40, 30);
    assert_eq!(run(&img, &["saturation(1e308)"]), run(&img, &["saturation(1)"]));
    assert_eq!(run(&img, &["saturation(-inf)"]), run(&img, &["saturation(-1)"]));
    assert_eq!(run(&img, &["tint(#80CFFF,-inf)"]), run(&img, &["tint(#80cfff,0)"]));
    assert_eq!(run(&img, &["tint(#80cfff,1e9)"]), img);
    for bad in ["saturation(NaN)", "tint(#80cfff)", "filtered_bw(orange)", "sepia(1)", "bw(,)"] {
        assert_eq!(recipe(&[bad]).print(), [bad], "kept as written");
        assert_eq!(run(&img, &[bad]), img, "{bad}: skipped");
    }
}
