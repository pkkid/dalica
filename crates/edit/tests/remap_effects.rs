//! The colour remaps of tabs 4 and 5 (`editing.md` §8.3–§8.4, M12.9): Invert Colors, Infrared Film,
//! Duo-Tone and Boost. Each renders (`VERSION` 1), matches its fingerprint at its defaults
//! (`fingerprints/remap-*.json`), renders the same at two sizes after a downscale and in a zoomed
//! region as in the whole render; then one invariant per effect: Invert twice is the identity
//! within one level; Infrared gives R = G = B with a green patch lighter and a blue one darker than
//! their luma; Duo-Tone maps black to the first colour and white to the second at Brightness 0 and
//! every pixel onto that segment; Boost 0 is the identity and Boost raises chroma and contrast; Fade
//! 100 gives the input.

use crate::common::{assert_two_sizes_agree, check_fingerprint, pixels, recipe, rgba, run, scene, window};
use dalica_edit::ops::colour::REC601;
use dalica_edit::{Op, Region, Source, Target, render, render_region};
use dalica_image::Rgba8;
use dalica_scenes::{Kind, Scene};

/// Each effect's fingerprint case and its entry at its defaults.
const DEFAULTS: [(&str, &str); 4] = [
    ("remap-invert", "invert()"),
    ("remap-infrared", "infrared(0)"),
    ("remap-duotone", "duotone(#004488,#ffff00,0,20,0)"),
    ("remap-boost", "boost(50)"),
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

/// One row of these colours.
fn patches(colours: &[[u8; 3]]) -> Rgba8 {
    let data = colours.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    Rgba8 { width: colours.len() as u32, height: 1, data }
}

fn luma(p: [u8; 3]) -> f64 {
    (0..3).map(|c| f64::from(REC601[c]) * f64::from(p[c])).sum()
}

fn is_grey(p: [u8; 3]) -> bool {
    p[0] == p[1] && p[1] == p[2]
}

/// The summed chroma (max − min) of every pixel.
fn chroma(img: &Rgba8) -> u64 {
    pixels(img).map(|p| u64::from(p.iter().max().unwrap() - p.iter().min().unwrap())).sum()
}

/// The standard deviation of the lumas.
fn luma_spread(img: &Rgba8) -> f64 {
    let l: Vec<f64> = pixels(img).map(luma).collect();
    let mean = l.iter().sum::<f64>() / l.len() as f64;
    (l.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / l.len() as f64).sqrt()
}

#[test]
fn every_remap_effect_renders() {
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
        let small = assert_two_sizes_agree(Source::upright(&up), &r, 64, 240, 2.0);
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

#[test]
fn invert_is_the_negative_and_twice_is_the_identity() {
    for src in [blobs(2, 120, 80), scene(3, 120, 80), colour_cube()] {
        let once = run(&src, &["invert()"]);
        for (a, b) in pixels(&src).zip(pixels(&once)) {
            for c in 0..3 {
                assert!((255 - i32::from(a[c]) - i32::from(b[c])).abs() <= 1, "{a:?} → {b:?}");
            }
        }
        let twice = run(&src, &["invert()", "invert()"]);
        for (a, b) in pixels(&src).zip(pixels(&twice)) {
            assert!((0..3).all(|c| a[c].abs_diff(b[c]) <= 1), "{a:?} → {b:?}");
        }
    }
}

#[test]
fn infrared_is_grey_with_foliage_light_and_sky_dark() {
    for src in [blobs(4, 120, 80), colour_cube()] {
        assert!(pixels(&run(&src, &["infrared(0)"])).all(is_grey));
        assert_eq!(run(&src, &["infrared(100)"]), src, "Fade 100 is the input");
    }
    // A foliage green, a pure green, a sky blue, a pure blue and a grey.
    let colours = [[80u8, 120, 40], [0, 200, 0], [100, 150, 220], [0, 0, 200], [128, 128, 128]];
    let out: Vec<_> = pixels(&run(&patches(&colours), &["infrared(0)"])).collect();
    for (i, p) in colours.iter().enumerate() {
        let (l, g) = (luma(*p), f64::from(out[i][0]));
        match i {
            0 | 1 => assert!(g > l + 20.0, "green {p:?} → {g}, luma {l}"),
            2 | 3 => assert!(g < l - 20.0, "blue {p:?} → {g}, luma {l}"),
            _ => assert_eq!(out[i], *p, "a grey stays the same grey"),
        }
    }
    // Fade 50 lies between the input and the effect.
    let src = blobs(5, 60, 40);
    let (full, half) = (run(&src, &["infrared(0)"]), run(&src, &["infrared(50)"]));
    for ((a, b), m) in pixels(&src).zip(pixels(&full)).zip(pixels(&half)) {
        for c in 0..3 {
            assert!(m[c] >= a[c].min(b[c]).saturating_sub(1) && m[c] <= a[c].max(b[c]).saturating_add(1), "{a:?} {b:?} {m:?}");
        }
    }
}

#[test]
fn duotone_maps_black_and_white_to_its_colours_and_every_pixel_onto_the_segment() {
    let ends = patches(&[[0, 0, 0], [255, 255, 255]]);
    for (first, second) in [([0x00u8, 0x44, 0x88], [0xffu8, 0xff, 0x00]), ([0x30, 0x10, 0x00], [0xf0, 0xe0, 0xc0]), ([0xff, 0xff, 0xff], [0, 0, 0])] {
        let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        for contrast in [0, 20, 100] {
            let entry = format!("duotone({},{},0,{contrast},0)", hex(first), hex(second));
            let out: Vec<_> = pixels(&run(&ends, &[&entry])).collect();
            assert_eq!(out, [first, second], "{entry}");
            // Every pixel lies on the line from the first colour to the second (within rounding).
            for src in [blobs(6, 80, 60), colour_cube()] {
                for p in pixels(&run(&src, &[&entry])) {
                    let d: [f64; 3] = std::array::from_fn(|c| f64::from(second[c]) - f64::from(first[c]));
                    let v: [f64; 3] = std::array::from_fn(|c| f64::from(p[c]) - f64::from(first[c]));
                    let len2: f64 = d.iter().map(|x| x * x).sum();
                    let t = (v.iter().zip(&d).map(|(a, b)| a * b).sum::<f64>() / len2).clamp(0.0, 1.0);
                    let off = (0..3).map(|c| (v[c] - t * d[c]).powi(2)).sum::<f64>().sqrt();
                    assert!(off <= 1.0, "{entry}: {p:?} is {off} off the segment");
                }
            }
        }
    }
    // Brightness lifts or sinks the mid-tones; Fade 100 gives the input.
    let src = blobs(8, 80, 60);
    let mean = |img: &Rgba8| pixels(img).map(luma).sum::<f64>();
    let (dark, base, light) = (
        mean(&run(&src, &["duotone(#000000,#ffffff,-50,20,0)"])),
        mean(&run(&src, &["duotone(#000000,#ffffff,0,20,0)"])),
        mean(&run(&src, &["duotone(#000000,#ffffff,50,20,0)"])),
    );
    assert!(dark < base && base < light, "{dark} < {base} < {light}");
    assert_eq!(run(&src, &["duotone(#004488,#ffff00,0,20,100)"]), src);
}

#[test]
fn boost_0_is_the_identity_and_boost_raises_chroma_and_contrast() {
    for src in [blobs(9, 120, 80), scene(10, 120, 80), colour_cube()] {
        assert_eq!(run(&src, &["boost(0)"]), src);
        let (c0, s0) = (chroma(&src), luma_spread(&src));
        let mut last = (c0, s0);
        for strength in [25, 50, 100] {
            let out = run(&src, &[&format!("boost({strength})")]);
            let now = (chroma(&out), luma_spread(&out));
            assert!(now.0 > last.0 && now.1 > last.1, "boost({strength}): chroma {} → {}, contrast {:.2} → {:.2}", last.0, now.0, last.1, now.1);
            last = now;
        }
    }
}

#[test]
fn hostile_values_render_as_their_clamped_entries() {
    let img = blobs(11, 40, 30);
    assert_eq!(run(&img, &["boost(1e308)"]), run(&img, &["boost(100)"]));
    assert_eq!(run(&img, &["boost(-inf)"]), img);
    assert_eq!(run(&img, &["infrared(-inf)"]), run(&img, &["infrared(0)"]));
    assert_eq!(run(&img, &["duotone(#004488,#ffff00,1e9,-1e9,1e9)"]), img);
    assert_eq!(run(&img, &["duotone(#004488,#FFFF00,-1e9,1e9,0)"]), run(&img, &["duotone(#004488,#ffff00,-95,100,0)"]));
    for bad in ["boost(NaN)", "infrared()", "duotone(#004488,yellow,0,20,0)", "invert(1)", "boost(50,1)"] {
        assert_eq!(recipe(&[bad]).print(), [bad], "kept as written");
        assert_eq!(run(&img, &[bad]), img, "{bad}: skipped");
    }
}
