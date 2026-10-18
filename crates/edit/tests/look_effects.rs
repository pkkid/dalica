//! The blur-based looks of tab 4 (`editing.md` §8.3, M12.10): HDR Look, Dream Glow and Plastic
//! Lens. Each renders (`VERSION` 1), matches its fingerprint at its defaults
//! (`fingerprints/looks-*.json`), renders the same at two sizes after a downscale and in a zoomed
//! region as in the whole render, leaves a flat patch flat and gives the input at Fade 100; then
//! one invariant per effect: HDR Look raises local contrast, Dream Glow lifts the midtones, Plastic
//! Lens leaves the centre sharper and the corners darker than the input.

use crate::common::{assert_two_sizes_agree, check_fingerprint, exif, recipe, run, scene, stored, window};
use dalica_edit::ops::looks::lens;
use dalica_edit::{Op, Region, Source, Target, render, render_region};
use dalica_image::Rgba8;

/// Each effect's fingerprint case and its entry at its defaults.
const DEFAULTS: [(&str, &str); 3] =
    [("looks-hdr", "hdr(20,3,0)"), ("looks-dream-glow", "dream_glow(25,50,0)"), ("looks-plastic-lens", "plastic_lens(70,30,0)")];

/// A photo with fine detail everywhere (so a blur shows wherever it applies).
fn detailed(w: u32, h: u32) -> Rgba8 {
    let mut img = scene(61, w, h);
    for y in 0..h {
        for x in 0..w {
            if (x / 3 + y / 3) % 2 == 0 {
                let at = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    img.data[at + c] /= 2;
                }
            }
        }
    }
    img
}

fn flat(w: u32, h: u32, v: [u8; 3]) -> Rgba8 {
    Rgba8 { width: w, height: h, data: (0..w * h).flat_map(|_| [v[0], v[1], v[2], 255]).collect() }
}

fn luma(img: &Rgba8, x: u32, y: u32) -> f64 {
    let at = ((y * img.width + x) * 4) as usize;
    0.299 * f64::from(img.data[at]) + 0.587 * f64::from(img.data[at + 1]) + 0.114 * f64::from(img.data[at + 2])
}

/// The mean luma over the window `(x, y, w, h)`.
fn mean(img: &Rgba8, (x0, y0, w, h): (u32, u32, u32, u32)) -> f64 {
    let mut sum = 0.0;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            sum += luma(img, x, y);
        }
    }
    sum / f64::from(w * h)
}

/// The mean absolute luma step between neighbours over the window: how sharp it is.
fn sharpness(img: &Rgba8, (x0, y0, w, h): (u32, u32, u32, u32)) -> f64 {
    let mut sum = 0.0;
    for y in y0..y0 + h - 1 {
        for x in x0..x0 + w - 1 {
            let l = luma(img, x, y);
            sum += (luma(img, x + 1, y) - l).abs() + (luma(img, x, y + 1) - l).abs();
        }
    }
    sum / f64::from((w - 1) * (h - 1))
}

/// Local contrast: the mean over 16 × 16 blocks of each block's luma deviation.
fn local_contrast(img: &Rgba8) -> f64 {
    let (mut sum, mut n) = (0.0, 0.0);
    for by in 0..img.height / 16 {
        for bx in 0..img.width / 16 {
            let ls: Vec<f64> = (0..256).map(|i| luma(img, bx * 16 + i % 16, by * 16 + i / 16)).collect();
            let m = ls.iter().sum::<f64>() / 256.0;
            sum += (ls.iter().map(|l| (l - m) * (l - m)).sum::<f64>() / 256.0).sqrt();
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn every_look_renders_as_version_one() {
    let img = detailed(64, 48);
    for (_, entry) in DEFAULTS {
        let op = Op::parse(entry);
        assert!(matches!(op.typed(), Some(Ok(_))), "{entry}");
        assert_eq!(op.version(), Some(1), "{entry}");
        assert_ne!(run(&img, &[entry]), img, "{entry} changes the photo");
    }
}

#[test]
fn two_sizes_agree_and_match_the_fingerprints() {
    let up = scene(12, 1600, 1067);
    for (case, entry) in DEFAULTS {
        let small = assert_two_sizes_agree(Source::upright(&up), &recipe(&[entry]), 240, 960, 2.0);
        check_fingerprint(case, &small, 0.5);
    }
}

#[test]
fn fade_100_gives_the_input() {
    let img = detailed(90, 60);
    for entry in ["hdr(20,3,100)", "dream_glow(25,50,100)", "plastic_lens(70,30,100)", "hdr(80,7,100)"] {
        assert_eq!(run(&img, &[entry]), img, "{entry}");
    }
}

/// A flat patch stays flat: HDR Look leaves it as it is, Dream Glow gives one colour everywhere;
/// Plastic Lens without grain adds no texture: the middle is untouched and only the vignette
/// darkens toward the corners, by the distance from the centre alone.
#[test]
fn each_leaves_a_flat_patch_flat() {
    let (w, h) = (120u32, 80u32);
    for v in [[0, 0, 0], [128, 90, 200], [255, 255, 255], [3, 250, 77]] {
        let img = flat(w, h, v);
        for entry in ["hdr(20,3,0)", "hdr(80,7,0)", "hdr(1.3,1,0)"] {
            assert_eq!(run(&img, &[entry]), img, "{entry} on {v:?}");
        }
        for entry in ["dream_glow(25,50,0)", "dream_glow(0,100,0)", "dream_glow(50,0,30)"] {
            let out = run(&img, &[entry]);
            assert!(out.data.chunks(4).all(|p| p == &out.data[..4]), "{entry} on {v:?}");
        }
        let out = run(&img, &["plastic_lens(100,0,0)"]);
        for y in 0..h {
            for x in 0..w {
                let at = ((y * w + x) * 4) as usize;
                let (soft, shade) = lens((w, h), x, y);
                let px = &out.data[at..at + 3];
                for c in 0..3 {
                    let want = (f32::from(v[c]) * shade).round();
                    assert!((f32::from(px[c]) - want).abs() <= 1.0, "plastic lens on {v:?} at {x},{y}: {px:?}");
                }
                if soft == 0.0 && shade == 1.0 {
                    assert_eq!(px, &v[..], "the middle is untouched");
                }
            }
        }
    }
}

#[test]
fn hdr_look_raises_local_contrast() {
    let img = scene(17, 400, 300);
    let before = local_contrast(&img);
    let mut last = before;
    for entry in ["hdr(20,1,0)", "hdr(20,3,0)", "hdr(20,7,0)"] {
        let c = local_contrast(&run(&img, &[entry]));
        assert!(c > last, "{entry}: {c} after {last} (input {before})");
        last = c;
    }
    // Fade halfway keeps it between.
    let half = local_contrast(&run(&img, &["hdr(20,3,50)"]));
    assert!(half > before && half < local_contrast(&run(&img, &["hdr(20,3,0)"])));
}

#[test]
fn dream_glow_lifts_the_midtones_and_softens() {
    let img = detailed(200, 150);
    let all = (0, 0, 200, 150);
    let none = run(&img, &["dream_glow(25,0,0)"]);
    let lifted = run(&img, &["dream_glow(25,100,0)"]);
    assert!(mean(&lifted, all) > mean(&none, all) + 5.0, "{} vs {}", mean(&lifted, all), mean(&none, all));
    // The bloom softens the fine detail the overlay of the photo on itself would keep.
    assert!(sharpness(&run(&img, &["dream_glow(50,50,0)"]), all) < sharpness(&run(&img, &["dream_glow(0,50,0)"]), all));
}

#[test]
fn plastic_lens_leaves_the_centre_sharper_and_the_corners_darker() {
    let (w, h) = (300u32, 200u32);
    let img = detailed(w, h);
    let centre = (w / 2 - 30, h / 2 - 20, 60, 40);
    let corners = [(0, 0, 40, 30), (w - 40, 0, 40, 30), (0, h - 30, 40, 30), (w - 40, h - 30, 40, 30)];
    // Without grain the middle is the input itself.
    let clean = run(&img, &["plastic_lens(70,0,0)"]);
    assert_eq!(window(&clean, centre.0, centre.1, centre.2, centre.3), window(&img, centre.0, centre.1, centre.2, centre.3));
    for (grain, out) in [(false, clean), (true, run(&img, &["plastic_lens(70,30,0)"]))] {
        let keep = sharpness(&out, centre) / sharpness(&img, centre);
        assert!(keep > 0.95 && (grain || keep < 1.05), "the centre keeps its detail: {keep}");
        for c in corners {
            let s = sharpness(&out, c) / sharpness(&img, c);
            assert!(s < 0.6 * keep, "corner {c:?} softer: {s} vs {keep}");
            assert!(mean(&out, c) < mean(&img, c) - 5.0, "corner {c:?} darker: {} vs {}", mean(&out, c), mean(&img, c));
        }
    }
}

#[test]
fn a_region_is_that_window_of_the_whole_render() {
    let up = detailed(120, 90);
    let cases: [(u8, &[&str]); 5] = [
        (1, &["hdr(20,3,0)"]),
        // A wide blur on its grid of cells.
        (6, &["hdr(80,7,10)", "rotate(90)"]),
        (3, &["straighten(-4)", "crop(0.1,0.2,0.8,0.9)", "dream_glow(50,70,0)", "tuning(0.3,0,0,0,none)"]),
        (1, &["dream_glow(25,50,0)"]),
        (8, &["plastic_lens(100,60,20)", "flip_h()"]),
    ];
    for (o, ops) in cases {
        let file = stored(&up, o);
        let source = Source::new(&file, exif(o));
        let r = recipe(ops);
        for scale in [1.0, 0.5, 2.5] {
            let whole = render(source, &r, Target::Scale(scale)).unwrap();
            let (w, h) = (whole.width, whole.height);
            for (x, y, ww, wh) in [(0, 0, w, h), (0, 0, 1, 1), (w / 3, h / 4, w / 2, h / 3), (w - 5, h - 4, 5, 4), (w / 5, h / 5, 7, 9)] {
                let region = Region { scale, x, y, width: ww.max(1), height: wh.max(1) };
                let part = render_region(source, &r, region).unwrap();
                assert_eq!(part, window(&whole, x, y, ww.max(1), wh.max(1)), "EXIF {o} {ops:?} at {scale}: {region:?}");
            }
        }
    }
}

/// Each look at its widest settings renders the same at two sizes too (the grid of cells at the
/// large size, the pixels themselves at the small one), cropped past the photo's white square and
/// white blob: a boost toward white of a hard edge that one size resolves and the other averages
/// is not the same after the downscale (HDR Look at Radius 80 and Strength 3 there: 2.2 at the
/// blob, the rest of the picture within 1; at the defaults the whole photo agrees, above). The
/// widest radius is held at the default strength: at Strength 7 it differs by 2.1 even here.
/// Plastic Lens's grain is held at 50: at 100 the large render's grain clips in the shadows and
/// highlights before it is averaged (3.7).
#[test]
fn the_widest_settings_agree_at_two_sizes() {
    let up = scene(12, 1600, 1067);
    for entry in ["hdr(80,3,0)", "hdr(1.3,7,0)", "dream_glow(50,100,0)", "dream_glow(0,100,0)", "plastic_lens(100,50,0)"] {
        assert_two_sizes_agree(Source::upright(&up), &recipe(&["crop(0.2,0.25,0.65,1)", entry]), 240, 960, 2.0);
    }
}
