//! Drop Shadow and Museum Matte (M12.14, `editing.md` §8.4) on M12.13's frames. A fingerprint per
//! effect at its defaults (the same at two render sizes after a downscale); the canvas grows and the
//! photo's pixels are unchanged inside; the default shadow reads at a glance (M12.32); the shadow
//! falls on the side its angle names and the canvas's corners stay the background; two Museum Mattes are larger than one and frame it; a
//! region equals that window of the whole render. Fingerprints: `fingerprints/mats-*.json`.

use crate::common::{assert_two_sizes_agree, check_fingerprint, recipe, rgba, run, scene, window};
use dalica_edit::{Region, Source, Target, render, render_region};
use dalica_image::Rgba8;
use dalica_scenes::{Kind, Scene};

const SHADOW: &str = "drop_shadow(#000000,15,90,25,30,#ffffff)";
const MATTE: &str = "museum_matte(#1a0e03,25,#f0eae4,40)";

fn px(img: &Rgba8, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * img.width + x) * 4) as usize;
    [img.data[at], img.data[at + 1], img.data[at + 2], img.data[at + 3]]
}

/// The mean of the three channels over a rectangle.
fn mean(img: &Rgba8, x: u32, y: u32, w: u32, h: u32) -> f64 {
    let win = window(img, x, y, w, h);
    let sum: f64 = win.data.chunks(4).map(|p| f64::from(p[0]) + f64::from(p[1]) + f64::from(p[2])).sum();
    sum / (3.0 * f64::from(w * h))
}

#[test]
fn each_effect_at_its_defaults_matches_its_fingerprint_at_two_sizes() {
    let up = rgba(&Scene::new(5, 400, 300).with_kind(Kind::Blobs).render().unwrap());
    // As Border's: the photo lands on whole pixels at each size, so the frame's edges may sit half
    // a pixel apart between two sizes; the shadow's soft edge moves with them (a cell or two more).
    for (case, op, tolerance) in [("mats-drop-shadow", SHADOW, 7.0), ("mats-museum-matte", MATTE, 6.0)] {
        let small = assert_two_sizes_agree(Source::upright(&up), &recipe(&[op]), 160, 400, tolerance);
        check_fingerprint(case, &small, 0.5);
    }
}

#[test]
fn the_canvas_grows_and_the_photos_pixels_are_unchanged_inside() {
    let up = scene(7, 300, 200);
    let plain = run(&up, &["tuning(0.2,0,0,0.3,none)"]);
    // 200 px on the shorter side: Drop Shadow's margin is Distance + Blur + 25 thousandths, Museum
    // Matte's both mats, each rounded to whole pixels.
    for (op, margin) in [
        (SHADOW, 13u32),
        ("drop_shadow(#ff0000,30,200,100,0,#102030)", 31),
        ("drop_shadow(#000000,0,0,0,100,#ffffff)", 5),
        (MATTE, 13),
        ("museum_matte(#000000,100,#ffffff,0)", 20),
        ("museum_matte(#000000,0,#ffffff,0)", 0),
    ] {
        let framed = run(&up, &["tuning(0.2,0,0,0.3,none)", op]);
        assert_eq!((framed.width, framed.height), (300 + 2 * margin, 200 + 2 * margin), "{op}");
        assert_eq!(window(&framed, margin, margin, 300, 200), plain, "{op}: the photo unchanged");
    }
    // Museum Matte: the outer mat's colour at the corners, the inner mat's around the photo (darker
    // along the top and left, where the outer mat's shadow falls).
    let matte = run(&up, &[MATTE]);
    let (outer, inner) = ([0x1a, 0x0e, 0x03, 255], [0xf0, 0xea, 0xe4, 255]);
    for (x, y) in [(0, 0), (matte.width - 1, 0), (0, matte.height - 1), (matte.width - 1, matte.height - 1)] {
        assert_eq!(px(&matte, x, y), outer, "the outer mat at {x},{y}");
    }
    // The inner mat is 8 px (40/1000 of 200) inside the 13 px band.
    assert_eq!(px(&matte, 13 + 300, 13 + 200), inner, "the inner mat's lit corner");
    let (top, bottom) = (mean(&matte, 50, 5, 200, 1), mean(&matte, 50, 13 + 200 + 7, 200, 1));
    assert!(top + 20.0 < bottom, "the inner shadow along the top ({top:.1}) and not the bottom ({bottom:.1})");
}

#[test]
fn the_default_shadow_reads_at_a_glance() {
    // 320 px on the shorter side: Distance 15 is 4.8 px, Blur 25 is 8 px, the margin 65 → 21 px.
    let up = scene(19, 480, 320);
    let img = run(&up, &[SHADOW]);
    let m = 21u32;
    assert_eq!((img.width, img.height), (480 + 2 * m, 320 + 2 * m));
    // Two columns centred 8 px beyond each side of the photo, away from the corners. On the right
    // the shadow's edge is 3.2 px nearer: the tent's closed form gives 0.7 × 4.8² / 128 of the way
    // to black, about 32 levels. On the left the shadow has moved away: the background.
    let rows = (m + 40, 240);
    let right = mean(&img, m + 480 + 7, rows.0, 2, rows.1);
    let left = mean(&img, m - 9, rows.0, 2, rows.1);
    assert!(right <= 255.0 - 20.0, "the shadow 8 px beyond the right edge ({right:.1})");
    assert!(left > right + 20.0, "offset, not a halo: the left ({left:.1}) lighter than the right ({right:.1})");
    for (x, y) in [(0, 0), (img.width - 1, 0), (0, img.height - 1), (img.width - 1, img.height - 1)] {
        assert_eq!(px(&img, x, y), [255, 255, 255, 255], "the background at {x},{y}");
    }
}

#[test]
fn the_shadow_falls_on_the_side_its_angle_names_and_the_far_corner_is_the_background() {
    let up = scene(11, 240, 160);
    // 160 px: Distance 30 is 4.8 px, Blur 20 is 3.2 px; the margin 30 + 20 + 25 → 12 px.
    let m = 12u32;
    for (angle, side) in [(0, "top"), (90, "right"), (180, "bottom"), (270, "left")] {
        let op = format!("drop_shadow(#000000,30,{angle},20,0,#ffffff)");
        let img = run(&up, &[op.as_str()]);
        assert_eq!((img.width, img.height), (240 + 2 * m, 160 + 2 * m), "{op}");
        // A strip 4 px wide just past each side of the photo, away from the corners.
        let strips = [
            ("top", mean(&img, m + 20, m - 4, 200, 4)),
            ("right", mean(&img, m + 240, m + 20, 4, 120)),
            ("bottom", mean(&img, m + 20, m + 160, 200, 4)),
            ("left", mean(&img, m - 4, m + 20, 4, 120)),
        ];
        for (name, v) in strips {
            if name == side {
                assert!(v < 160.0, "{op}: the shadow on the {name} ({v:.1})");
            } else {
                assert!(v > 200.0, "{op}: only the blur's edge on the {name} ({v:.1})");
            }
        }
        for (x, y) in [(0, 0), (img.width - 1, 0), (0, img.height - 1), (img.width - 1, img.height - 1)] {
            assert_eq!(px(&img, x, y), [255, 255, 255, 255], "{op}: the background at {x},{y}");
        }
    }
    // Fade 100 leaves the background only; the shadow's colour is its own.
    let faded = run(&up, &["drop_shadow(#000000,30,90,20,100,#336699)"]);
    assert_eq!(px(&faded, faded.width - m / 2, faded.height / 2), [0x33, 0x66, 0x99, 255]);
    // Without Blur the margin is 30 + 25 thousandths: 9 px.
    // Without Blur the margin is 30 + 25 thousandths of 160 px: 9 px.
    let red = run(&up, &["drop_shadow(#ff0000,30,90,0,0,#ffffff)"]);
    assert_eq!(px(&red, 9 + 241, red.height / 2), [255, 0, 0, 255], "a hard red shadow");
}

#[test]
fn two_museum_mattes_are_larger_than_one_and_frame_it() {
    let up = scene(17, 200, 150);
    let one = run(&up, &[MATTE]);
    let two = run(&up, &[MATTE, MATTE]);
    assert!(two.width > one.width && two.height > one.height);
    // The second matte wraps the first: 65/1000 of its shorter side, rounded.
    let band = (f64::from(one.width.min(one.height)) * 0.065).round() as u32;
    assert_eq!((two.width, two.height), (one.width + 2 * band, one.height + 2 * band));
    assert_eq!(window(&two, band, band, one.width, one.height), one);
}

#[test]
fn a_region_equals_that_window_of_the_whole_render() {
    let up = scene(13, 160, 120);
    let source = Source::upright(&up);
    for ops in [
        vec![SHADOW],
        vec![MATTE],
        vec!["autocontrast()", "drop_shadow(#203040,30,135,100,10,#e0e0e0)", "retouch(0.3,0.4,0.6,0.5,0.04)", MATTE, "rotate(90)"],
        vec!["crop(0.1,0.1,0.9,0.8)", MATTE, MATTE, "drop_shadow(#000000,12,300,40,30,#ffffff)", "flip_v()"],
    ] {
        let r = recipe(&ops);
        for scale in [1.0, 0.5] {
            let whole = render(source, &r, Target::Scale(scale)).unwrap();
            for (x, y, w, h) in [
                (0, 0, 9, 7),
                (3, whole.height / 3, whole.width / 2, whole.height / 3),
                (whole.width - 20, whole.height - 11, 20, 11),
                (0, 0, whole.width, whole.height),
            ] {
                let part = render_region(source, &r, Region { scale, x, y, width: w, height: h }).unwrap();
                assert_eq!(part, window(&whole, x, y, w, h), "{ops:?} at {scale}: {x},{y} {w}×{h}");
            }
        }
    }
}
