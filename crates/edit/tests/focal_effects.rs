//! The crosshair effects of tab 3 (`editing.md` §8.2, M12.8): Soft Focus, Focal B&W and Graduated
//! Tint. Inside the circle Soft Focus and Focal B&W leave the photo as it was; far outside Focal
//! B&W is grey and Soft Focus lowers the local variance; Graduated Tint leaves what lies behind its
//! start alone and a white patch white; the circle sits where its stored-frame centre says on a
//! photo stored turned, cropped and rotated; a region equals that window of the whole render; two
//! sizes agree after a downscale and match `fingerprints/focal-*.json`.

use crate::common::{self, assert_two_sizes_agree, check_fingerprint, exif, recipe, run, scene, stored, window};
use dalica_edit::ops::Operation;
use dalica_edit::ops::focal::{FocalBw, GraduatedTint, SoftFocus};
use dalica_edit::{Geometry, Point, Region, Source, Target, render, render_region};
use dalica_image::Rgba8;
use dalica_scenes::{Kind, Scene};

fn px(img: &Rgba8, x: u32, y: u32) -> [u8; 3] {
    let at = ((y * img.width + x) * 4) as usize;
    [img.data[at], img.data[at + 1], img.data[at + 2]]
}

/// Every pixel whose centre lies within `r` pixels of `(cx, cy)` (`inside`) or further than `r`
/// (`!inside`).
fn pixels_where(img: &Rgba8, cx: f64, cy: f64, r: f64, inside: bool) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for y in 0..img.height {
        for x in 0..img.width {
            let d = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy);
            if (d < r) == inside {
                out.push((x, y));
            }
        }
    }
    out
}

/// The mean over the picture of each 5 × 5 block's variance (grey levels): how much fine detail is
/// left, over the blocks wholly outside the radius `r` about `(cx, cy)`.
fn local_variance_outside(img: &Rgba8, cx: f64, cy: f64, r: f64) -> f64 {
    let (mut sum, mut n) = (0.0, 0);
    for by in (0..img.height - 5).step_by(5) {
        for bx in (0..img.width - 5).step_by(5) {
            if (f64::from(bx) + 2.5 - cx).hypot(f64::from(by) + 2.5 - cy) < r + 4.0 {
                continue;
            }
            let vals: Vec<f64> = (0..25).map(|i| px(img, bx + i % 5, by + i / 5).iter().map(|&v| f64::from(v)).sum::<f64>() / 3.0).collect();
            let mean = vals.iter().sum::<f64>() / 25.0;
            sum += vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 25.0;
            n += 1;
        }
    }
    sum / f64::from(n.max(1))
}

#[test]
fn each_effect_renders_at_version_one() {
    assert_eq!((SoftFocus::VERSION, FocalBw::VERSION, GraduatedTint::VERSION), (1, 1, 1));
    let img = scene(3, 120, 80);
    for e in ["soft_focus(0.5,0.5,0.3,0.5)", "focal_bw(0.5,0.5,0.3,0.5)", "graduated_tint(0.5,0.5,0,0.25,0.25,#9cc8ff)"] {
        assert_ne!(run(&img, &[e]), img, "{e}");
    }
    // Amount 0 and a white tint change nothing.
    assert_eq!(run(&img, &["soft_focus(0.5,0.5,0.3,0)"]), img);
    assert_eq!(run(&img, &["graduated_tint(0.5,0.5,0,0.25,1,#ffffff)"]), img);
}

#[test]
fn soft_focus_keeps_the_circle_and_blurs_outside_it() {
    let img = scene(11, 300, 200);
    let (cx, cy, size) = (0.4, 0.55, 0.25);
    let out = run(&img, &[&format!("soft_focus({cx},{cy},{size},0.8)")]);
    let (ox, oy, r) = (cx * 300.0, cy * 200.0, size * 200.0);
    for (x, y) in pixels_where(&img, ox, oy, r - 1.0, true) {
        assert_eq!(px(&out, x, y), px(&img, x, y), "inside at ({x}, {y})");
    }
    let (before, after) = (local_variance_outside(&img, ox, oy, r * 1.5), local_variance_outside(&out, ox, oy, r * 1.5));
    assert!(after < before * 0.5, "the local variance outside falls: {before:.2} → {after:.2}");
}

#[test]
fn focal_bw_keeps_colour_inside_and_is_grey_far_outside() {
    let img = scene(13, 300, 200);
    let (cx, cy, size) = (0.6, 0.4, 0.2);
    for sharpness in [0.0, 0.5, 1.0] {
        let out = run(&img, &[&format!("focal_bw({cx},{cy},{size},{sharpness})")]);
        let (ox, oy, r) = (cx * 300.0, cy * 200.0, size * 200.0);
        for (x, y) in pixels_where(&img, ox, oy, r - 1.0, true) {
            assert_eq!(px(&out, x, y), px(&img, x, y), "inside at ({x}, {y}), sharpness {sharpness}");
        }
        // Past the softest edge (2 × Size) every pixel is grey.
        for (x, y) in pixels_where(&img, ox, oy, 2.0 * r + 1.0, false) {
            let [r_, g, b] = px(&out, x, y);
            assert!(r_ == g && g == b, "grey at ({x}, {y}), sharpness {sharpness}: {:?}", [r_, g, b]);
        }
    }
}

#[test]
fn graduated_tint_leaves_what_lies_behind_its_start_and_keeps_white_white() {
    // A blobs photo with a white patch in its top half.
    let mut img = scene(17, 200, 160);
    for y in 10..40 {
        for x in 120..170 {
            let at = ((y * 200 + x) * 4) as usize;
            img.data[at..at + 3].copy_from_slice(&[255, 255, 255]);
        }
    }
    for (angle, behind) in [(0.0, "below"), (90.0, "left"), (180.0, "above")] {
        let out = run(&img, &[&format!("graduated_tint(0.5,0.5,{angle},0.25,0.5,#9cc8ff)")]);
        for y in 0..160 {
            for x in 0..200 {
                let is_behind = match behind {
                    "below" => y >= 80,
                    "left" => x < 100,
                    _ => y < 80,
                };
                if is_behind {
                    assert_eq!(px(&out, x, y), px(&img, x, y), "behind the start at ({x}, {y}), angle {angle}");
                }
            }
        }
    }
    let out = run(&img, &["graduated_tint(0.5,0.5,0,0.1,1,#9cc8ff)"]);
    for y in 10..40 {
        for x in 120..170 {
            assert_eq!(px(&out, x, y), [255; 3], "white stays white at ({x}, {y})");
        }
    }
    // Ahead of the feather the photo takes the colour: less red, as much blue.
    let (mut red, mut was) = (0u64, 0u64);
    for y in 0..20 {
        for x in 0..100 {
            red += u64::from(px(&out, x, y)[0]);
            was += u64::from(px(&img, x, y)[0]);
        }
    }
    assert!(red * 10 < was * 9, "{red} vs {was}");
}

/// A crosshair placed on a photo stored turned (EXIF 6), cropped and rotated lands where its
/// stored-frame point shows on the displayed picture: the coloured disc of Focal B&W is centred
/// there and has the radius Size says.
#[test]
fn the_circle_sits_where_its_stored_centre_says_on_a_turned_cropped_photo() {
    // A saturated red photo, upright 240 × 160, stored turned for EXIF 6.
    let up = Rgba8 { width: 240, height: 160, data: [200u8, 40, 30, 255].repeat(240 * 160) };
    let file = stored(&up, 6);
    let (cx, cy, size) = (0.35, 0.6, 0.12);
    for ops in [
        vec!["crop(0.1,0.2,0.9,0.95)".to_string(), format!("focal_bw({cx},{cy},{size},1)"), "rotate(90)".into()],
        vec![
            "straighten(5)".into(),
            "crop(0.2,0.1,0.8,0.9)".into(),
            format!("focal_bw({cx},{cy},{size},1)"),
            "rotate(270)".into(),
            "flip_h()".into(),
        ],
    ] {
        let ops: Vec<&str> = ops.iter().map(String::as_str).collect();
        let r = recipe(&ops);
        let out = render(Source::new(&file, exif(6)), &r, Target::Full).unwrap();
        let g = Geometry::new(&r, exif(6), file.width, file.height);
        let p = g.to_display(Point::new(cx, cy)).unwrap();
        let (ex, ey) = (p.x * f64::from(out.width), p.y * f64::from(out.height));
        // The coloured pixels' centroid and area.
        let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
        for y in 0..out.height {
            for x in 0..out.width {
                let [r_, g_, _] = px(&out, x, y);
                if r_ > g_ + 40 {
                    sx += f64::from(x) + 0.5;
                    sy += f64::from(y) + 0.5;
                    n += 1.0;
                }
            }
        }
        assert!(n > 0.0, "{ops:?}: no colour left");
        let (mx, my) = (sx / n, sy / n);
        assert!((mx - ex).abs() < 1.0 && (my - ey).abs() < 1.0, "{ops:?}: the disc at ({mx:.1}, {my:.1}), the point at ({ex:.1}, {ey:.1})");
        // Size is relative to the stored frame's shorter side (its y axis), as long as the
        // displayed picture shows it (a straighten enlarges it).
        let (o, s) = (g.to_display(Point::new(0.0, 0.0)).unwrap(), g.to_display(Point::new(0.0, 1.0)).unwrap());
        let short = ((s.x - o.x) * f64::from(out.width)).hypot((s.y - o.y) * f64::from(out.height));
        let radius = (n / std::f64::consts::PI).sqrt();
        assert!((radius - size * short).abs() < 1.0, "{ops:?}: radius {radius:.1}, expected {:.1}", size * short);
    }
}

#[test]
fn a_region_is_that_window_of_the_whole_render() {
    let up = scene(19, 160, 120);
    let cases: [(u8, &[&str]); 3] = [
        (1, &["soft_focus(0.3,0.4,0.2,1)"]),
        (6, &["straighten(-3)", "crop(0.1,0.1,0.9,0.85)", "soft_focus(0.6,0.5,0.15,0.6)", "focal_bw(0.4,0.6,0.25,0.3)", "rotate(90)"]),
        (3, &["graduated_tint(0.4,0.3,135,0.3,0.6,#ffa040)", "soft_focus(0.5,0.5,0.3,0.5)", "flip_h()"]),
    ];
    for (o, ops) in cases {
        let file = stored(&up, o);
        let source = Source::new(&file, exif(o));
        let r = recipe(ops);
        for scale in [1.0, 0.45, 2.0] {
            let whole = render(source, &r, Target::Scale(scale)).unwrap();
            let (w, h) = (whole.width, whole.height);
            for (x, y, ww, wh) in [(0, 0, w, h), (0, 0, 1, 1), (w / 3, h / 4, w / 2, h / 3), (w - 5, h - 4, 5, 4), (w / 2, 1, 3, h - 1)] {
                let region = Region { scale, x, y, width: ww.max(1), height: wh.max(1) };
                let part = render_region(source, &r, region).unwrap();
                assert_eq!(part, window(&whole, x, y, ww.max(1), wh.max(1)), "EXIF {o} {ops:?} at {scale}: {region:?}");
            }
        }
    }
}

/// A wide Soft Focus blurs on a grid of cells (here 2 to 8 pixels a side): a region still equals
/// that window of the whole render, whichever way the window lies across the cells.
#[test]
fn a_region_of_a_wide_soft_focus_is_that_window_of_the_whole_render() {
    let up = scene(31, 640, 480);
    let r = recipe(&["crop(0.05,0,1,0.95)", "soft_focus(0.45,0.55,0.2,1)", "rotate(90)"]);
    let file = stored(&up, 8);
    let source = Source::new(&file, exif(8));
    for scale in [1.0, 2.5] {
        let whole = render(source, &r, Target::Scale(scale)).unwrap();
        let (w, h) = (whole.width, whole.height);
        for (x, y, ww, wh) in [(0, 0, 7, 5), (w / 3 + 1, h / 4 + 3, 37, 29), (w - 9, h - 11, 9, 11), (w / 2 + 5, 3, 2, h - 3)] {
            let region = Region { scale, x, y, width: ww, height: wh };
            let part = render_region(source, &r, region).unwrap();
            assert_eq!(part, window(&whole, x, y, ww, wh), "at {scale}: {region:?}");
        }
    }
}

#[test]
fn two_sizes_agree_and_match_the_fingerprints() {
    let photo = common::rgba(&Scene::new(23, 300, 200).with_kind(Kind::Blobs).render().unwrap());
    let sky = common::rgba(&Scene::new(5, 300, 200).with_kind(Kind::Horizon).render().unwrap());
    for (case, img, entry) in [
        ("focal-soft-focus", &photo, format!("{}({})", SoftFocus::NAME, SoftFocus::default())),
        ("focal-bw", &photo, format!("{}({})", FocalBw::NAME, FocalBw::default())),
        ("focal-graduated-tint", &sky, format!("{}({})", GraduatedTint::NAME, GraduatedTint::default())),
    ] {
        let small = assert_two_sizes_agree(Source::upright(img), &recipe(&[&entry]), 64, 240, 2.0);
        check_fingerprint(case, &small, 0.5);
    }
}

/// Soft Focus at its defaults on a 1200 × 800 photo, large enough that its blur runs on a grid of
/// cells and every fast path is taken, matches `fingerprints/focal-soft-focus-large.json` (blessed
/// from the M12 code before its speed work, M13.37).
#[test]
fn a_large_soft_focus_matches_its_fingerprint() {
    let photo = common::rgba(&Scene::new(23, 1200, 800).with_kind(Kind::Blobs).render().unwrap());
    let entry = format!("{}({})", SoftFocus::NAME, SoftFocus::default());
    let out = render(Source::upright(&photo), &recipe(&[&entry]), Target::Full).unwrap();
    check_fingerprint("focal-soft-focus-large", &out, 0.5);
}
