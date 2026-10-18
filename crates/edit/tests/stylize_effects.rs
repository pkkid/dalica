//! Vignette, Pixelate and Focal Zoom (`editing.md` §8.4, M12.12): a fingerprint per effect at its
//! defaults, each the same at two sizes after a downscale; Vignette leaves the centre, darkens the
//! corners and compounds; Pixelate in Normal mode makes each cell one colour, with the same cells
//! at two sizes and under a crop (anchored in the stored frame); Focal Zoom leaves its circle as it
//! was, wherever the photo is turned, straightened or cropped, and blurs along the radius more than
//! across it; Fade 100 gives the input; a zoomed region equals that window of the whole render;
//! hostile sizes render without a panic. `stylize-*-large.json` holds a whole 1200 × 800 render
//! of each at its defaults, `stylize-focal-zoom-coarse.json` a 2400 × 1600 Focal Zoom.

use crate::common::{assert_two_sizes_agree, check_fingerprint, exif, pixels, recipe, rgba, run, scene, stored, window};
use dalica_edit::fx::Space;
use dalica_edit::{Geometry, Point, Region, Source, Target, render, render_region};
use dalica_image::Rgba8;
use dalica_scenes::{Kind, Scene};

/// The three effects at their defaults, with their fingerprint cases.
const DEFAULTS: [(&str, &str); 3] = [
    ("stylize-vignette", "vignette(35,1.4,#000000,0)"),
    ("stylize-pixelate", "pixelate(20,normal,0)"),
    ("stylize-focal-zoom", "focal_zoom(0.5,0.5,0.3,50,50,0)"),
];

fn luma(p: [u8; 3]) -> f64 {
    0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2])
}

fn at(img: &Rgba8, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * img.width + x) * 4) as usize;
    [img.data[i], img.data[i + 1], img.data[i + 2]]
}

/// A picture `w × h` of `f(x, y)` grey levels.
fn grey(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Rgba8 {
    let data = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| {
        let v = f(x, y);
        [v, v, v, 255]
    });
    Rgba8 { width: w, height: h, data: data.collect() }
}

#[test]
fn each_effect_at_its_defaults_matches_its_fingerprint_at_two_sizes() {
    // Sky over ground with a sun: edges for the zoom's streaks and the cells.
    let img = rgba(&Scene::new(5, 300, 200).with_kind(Kind::Horizon).render().unwrap());
    for (case, op) in DEFAULTS {
        let small = assert_two_sizes_agree(Source::upright(&img), &recipe(&[op]), 96, 300, 2.0);
        assert_ne!(small, render(Source::upright(&img), &recipe(&[]), Target::Fit { width: 96, height: 96 }).unwrap(), "{op} changes the picture");
        check_fingerprint(case, &small, 0.5);
    }
}

/// A whole 1200 × 800 render of each effect at its defaults, large enough for Focal Zoom's long
/// paths and Pixelate's cells of many pixels, matches its `stylize-*-large` fingerprint, and a
/// 2400 × 1600 Focal Zoom (past `COARSE_PIXELS`, blurred on a grid of 2 × 2 cells) its
/// `stylize-focal-zoom-coarse` one, all blessed from the M12 code before M13.41 made them faster.
#[test]
fn each_effect_at_its_defaults_matches_its_large_fingerprint() {
    let img = rgba(&Scene::new(5, 1200, 800).with_kind(Kind::Horizon).render().unwrap());
    for (case, op) in DEFAULTS {
        check_fingerprint(&format!("{case}-large"), &run(&img, &[op]), 0.5);
    }
    let img = rgba(&Scene::new(5, 2400, 1600).with_kind(Kind::Horizon).render().unwrap());
    check_fingerprint("stylize-focal-zoom-coarse", &run(&img, &["focal_zoom(0.5,0.5,0.3,50,50,0)"]), 0.5);
}

#[test]
fn vignette_leaves_the_centre_darkens_the_corners_and_two_are_darker_than_one() {
    let img = scene(13, 160, 120);
    let one = run(&img, &["vignette(35,1.4,#000000,0)"]);
    let two = run(&img, &["vignette(35,1.4,#000000,0)", "vignette(35,1.4,#000000,0)"]);
    // The middle of the picture, out to well inside the ramp, is untouched.
    for y in 40..80 {
        for x in 50..110 {
            assert_eq!(at(&one, x, y), at(&img, x, y), "({x}, {y})");
        }
    }
    let corners = [(0, 0), (159, 0), (0, 119), (159, 119), (3, 2), (155, 116)];
    for (x, y) in corners {
        let (a, b, c) = (luma(at(&img, x, y)), luma(at(&one, x, y)), luma(at(&two, x, y)));
        assert!(b < a * 0.8 + 1.0, "({x}, {y}): {a} → {b}");
        assert!(c < b || c == 0.0, "({x}, {y}): two {c} not darker than one {b}");
    }
    // Never lighter toward black, anywhere.
    assert!(pixels(&img).zip(pixels(&one)).all(|(a, b)| (0..3).all(|k| b[k] <= a[k])));
    // Toward a colour: white corners on a black picture, the middle still black.
    let black = grey(160, 120, |_, _| 0);
    let white = run(&black, &["vignette(50,2,#ffffff,0)"]);
    assert_eq!(at(&white, 0, 0), [255, 255, 255]);
    assert_eq!(at(&white, 80, 60), [0, 0, 0]);
    // A larger Size reaches further in; Size 0 changes nothing.
    let small = run(&img, &["vignette(10,1.4,#000000,0)"]);
    assert_eq!(at(&small, 20, 15), at(&img, 20, 15));
    assert_ne!(at(&one, 20, 15), at(&img, 20, 15));
    assert_eq!(run(&img, &["vignette(0,2,#000000,0)"]), img);
}

/// Each `side × side` block (anchored at `(ox, oy)`, clipped at the edges) of `img` is one colour;
/// returns the blocks' colours, row-major.
fn blocks(img: &Rgba8, side: u32, ox: u32, oy: u32) -> Vec<[u8; 3]> {
    let starts = |o: u32, n: u32| -> Vec<u32> {
        // The first boundary inside: where `x + o` is a multiple of the side.
        let mut s = vec![0];
        s.extend((0..).map(|k| (side - o % side) % side + k * side).skip_while(|v| *v == 0).take_while(|v| *v < n));
        s
    };
    let (xs, ys) = (starts(ox, img.width), starts(oy, img.height));
    let mut out = Vec::new();
    for (j, &y0) in ys.iter().enumerate() {
        let y1 = ys.get(j + 1).copied().unwrap_or(img.height);
        for (i, &x0) in xs.iter().enumerate() {
            let x1 = xs.get(i + 1).copied().unwrap_or(img.width);
            let c = at(img, x0, y0);
            for y in y0..y1 {
                for x in x0..x1 {
                    assert_eq!(at(img, x, y), c, "({x}, {y}) in the cell at ({x0}, {y0})");
                }
            }
            out.push(c);
        }
    }
    out
}

#[test]
fn pixelate_makes_each_cell_one_colour_with_the_same_cells_at_two_sizes() {
    // 400 × 300: Pixel Size 20 is 6 px at full size and 3 px at half size.
    let img = scene(14, 400, 300);
    let r = recipe(&["pixelate(20,normal,0)"]);
    let full = render(Source::upright(&img), &r, Target::Full).unwrap();
    let half = render(Source::upright(&img), &r, Target::Scale(0.5)).unwrap();
    let (a, b) = (blocks(&full, 6, 0, 0), blocks(&half, 3, 0, 0));
    assert_eq!(a.len(), b.len(), "as many cells at both sizes");
    let worst = a.iter().zip(&b).map(|(p, q)| (0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap()).max().unwrap();
    assert!(worst <= 6, "a cell's colour differs by {worst} between the sizes");
    // The cells stay on the stored frame under a crop: inside the crop, the cells that lie wholly
    // inside it are the uncropped render's (the crop starts 100 px and 60 px in).
    let cropped = render(Source::upright(&img), &recipe(&["crop(0.25,0.2,0.75,0.8)", "pixelate(20,normal,0)"]), Target::Full).unwrap();
    blocks(&cropped, 6, 100, 60);
    for y in 0..cropped.height {
        for x in 0..cropped.width {
            let (sx, sy) = (x + 100, y + 60);
            let inside = sx / 6 * 6 >= 100 && sy / 6 * 6 >= 60 && (sx / 6 + 1) * 6 <= 300 && (sy / 6 + 1) * 6 <= 240;
            if inside {
                assert_eq!(at(&cropped, x, y), at(&full, sx, sy), "({x}, {y})");
            }
        }
    }
    // The blend modes keep the cells' colour where it matters: Multiply never lightens, Screen
    // never darkens.
    let multiply = run(&img, &["pixelate(20,multiply,0)"]);
    let screen = run(&img, &["pixelate(20,screen,0)"]);
    for ((p, m), s) in pixels(&img).zip(pixels(&multiply)).zip(pixels(&screen)) {
        assert!((0..3).all(|k| m[k] <= p[k] && s[k] >= p[k]), "{p:?}: multiply {m:?}, screen {s:?}");
    }
}

/// The renders of `ops` with and without `effect` on `up` stored under EXIF `o`, at `scale`, and
/// the stored-frame centre's place in the displayed render (pixels).
fn with_and_without(up: &Rgba8, o: u8, ops: &[&str], effect: &str, centre: Point, scale: f64) -> (Rgba8, Rgba8, (f64, f64), f64) {
    let file = stored(up, o);
    let source = Source::new(&file, exif(o));
    let mut with: Vec<&str> = ops.to_vec();
    with.push(effect);
    let a = render(source, &recipe(&with), Target::Scale(scale)).unwrap();
    let b = render(source, &recipe(ops), Target::Scale(scale)).unwrap();
    let g = Geometry::new(&recipe(ops), exif(o), file.width, file.height);
    let p = g.to_display(centre).unwrap();
    // The stored photo's shorter side in output pixels.
    let (ow, oh) = g.output_size(scale).unwrap();
    let short = Space::from_geometry(&g, ow, oh).unwrap().short();
    let at = (p.x * f64::from(a.width), p.y * f64::from(a.height));
    (a, b, at, short)
}

#[test]
fn focal_zoom_leaves_its_circle_wherever_the_photo_is_turned_and_blurs_outside() {
    let up = scene(15, 240, 180);
    let centre = Point::new(0.35, 0.6);
    let effect = "focal_zoom(0.35,0.6,0.2,60,100,0)";
    for (o, ops) in [
        (1u8, &[][..]),
        (6, &["rotate(90)"][..]),
        (3, &["straighten(-4)", "crop(0.1,0.15,0.85,0.9)"][..]),
        (8, &["crop(0.05,0.1,0.9,0.95)", "flip_h()", "rotate(270)"][..]),
    ] {
        let (a, b, (cx, cy), short) = with_and_without(&up, o, ops, effect, centre, 1.0);
        assert_eq!((a.width, a.height), (b.width, b.height));
        let (mut kept, mut changed, mut outside) = (0, 0, 0);
        for y in 0..a.height {
            for x in 0..a.width {
                let d = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) / short;
                if d < 0.2 - 1.5 / short {
                    assert_eq!(at(&a, x, y), at(&b, x, y), "EXIF {o} {ops:?}: ({x}, {y}) inside the circle");
                    kept += 1;
                } else if d > 0.2 + 2.0 / short {
                    outside += 1;
                    changed += usize::from(at(&a, x, y) != at(&b, x, y));
                }
            }
        }
        assert!(kept > 500, "EXIF {o} {ops:?}: {kept} pixels in the circle");
        assert!(changed * 10 > outside * 7, "EXIF {o} {ops:?}: {changed} of {outside} outside changed");
    }
}

#[test]
fn focal_zoom_blurs_along_the_radius_more_than_across_it() {
    let (w, h) = (200u32, 200u32);
    let polar = |x: u32, y: u32| {
        let (dx, dy) = (f64::from(x) + 0.5 - 100.0, f64::from(y) + 0.5 - 100.0);
        (dx.hypot(dy), dy.atan2(dx))
    };
    // Rings change along the radius; spokes across it. Both have about the same period where
    // measured (radius 80: a ring every 8 px, a spoke every 8 px of arc).
    let rings = grey(w, h, |x, y| if ((polar(x, y).0 / 4.0) as u32).is_multiple_of(2) { 40 } else { 215 });
    let spokes = grey(w, h, |x, y| if (((polar(x, y).1 + 4.0) * 80.0 / 8.0) as u32).is_multiple_of(2) { 40 } else { 215 });
    let op = "focal_zoom(0.5,0.5,0.1,30,100,0)";
    let change = |img: &Rgba8| {
        let out = run(img, &[op]);
        let mut sum = 0.0;
        let mut n = 0.0;
        for y in 0..h {
            for x in 0..w {
                let r = polar(x, y).0;
                if (70.0..90.0).contains(&r) {
                    sum += (luma(at(img, x, y)) - luma(at(&out, x, y))).abs();
                    n += 1.0;
                }
            }
        }
        sum / n
    };
    let (along, across) = (change(&rings), change(&spokes));
    assert!(along > 40.0, "rings change by {along}");
    assert!(along > 4.0 * across, "rings {along}, spokes {across}");
}

#[test]
fn fade_100_gives_the_input() {
    let img = scene(16, 150, 100);
    let base = ["crop(0.05,0.05,0.95,0.9)", "tuning(0.2,0,0,0,none)"];
    for op in ["vignette(50,2,#ff0000,100)", "pixelate(80,overlay,100)", "focal_zoom(0.2,0.3,0.1,100,0,100)"] {
        let with: Vec<&str> = base.iter().copied().chain([op]).collect();
        assert_eq!(run(&img, &with), run(&img, &base), "{op}");
    }
    // And a half Fade lies between.
    let half = run(&img, &["vignette(50,2,#000000,50)"]);
    let full = run(&img, &["vignette(50,2,#000000,0)"]);
    let (a, b, c) = (luma(at(&img, 0, 0)), luma(at(&half, 0, 0)), luma(at(&full, 0, 0)));
    assert!(a > b && b > c, "{a} {b} {c}");
}

#[test]
fn a_region_is_that_window_of_the_whole_render() {
    let up = scene(17, 120, 90);
    let effects = [
        "vignette(35,1.4,#000000,0)",
        "vignette(50,1.8,#204060,20)",
        "pixelate(20,normal,0)",
        "pixelate(60,soft_light,30)",
        "focal_zoom(0.5,0.5,0.3,50,50,0)",
        "focal_zoom(0.1,0.8,0.05,100,0,10)",
    ];
    let around: [(u8, &[&str]); 3] = [(1, &[]), (6, &["crop(0.1,0.1,0.9,0.85)", "rotate(90)"]), (3, &["straighten(-4)", "flip_v()"])];
    for effect in effects {
        for (o, ops) in around {
            let file = stored(&up, o);
            let source = Source::new(&file, exif(o));
            let mut with: Vec<&str> = ops.to_vec();
            with.extend([effect, "tuning(0.2,0,0,0,none)"]);
            let r = recipe(&with);
            for scale in [1.0, 0.45, 2.5] {
                let whole = render(source, &r, Target::Scale(scale)).unwrap();
                let (w, h) = (whole.width, whole.height);
                for (x, y, ww, wh) in [(0, 0, w, h), (0, 0, 1, 1), (w / 3, h / 4, w / 2, h / 3), (w - 5, h - 4, 5, 4), (w / 5, h / 2, 7, 9)] {
                    let region = Region { scale, x, y, width: ww, height: wh };
                    let part = render_region(source, &r, region).unwrap();
                    assert_eq!(part, window(&whole, x, y, ww, wh), "{with:?} EXIF {o} at {scale}: {region:?}");
                }
            }
        }
    }
}

#[test]
fn hostile_sizes_render_without_a_panic() {
    for (w, h) in [(1, 1), (2, 1), (1, 7), (3, 2)] {
        let img = scene(18, w.max(8), h.max(8));
        let tiny = window(&img, 0, 0, w, h);
        for op in [
            "vignette(50,2,#ffffff,0)",
            "pixelate(150,overlay,0)",
            "pixelate(2,multiply,0)",
            "focal_zoom(1,0,0,100,0,0)",
            "focal_zoom(0,1,1,100,100,0)",
        ] {
            let out = run(&tiny, &[op]);
            assert_eq!((out.width, out.height), (w, h), "{op}");
            for scale in [0.01, 7.0] {
                // A size the renderer refuses is an error, never a panic.
                let _ = render(Source::upright(&tiny), &recipe(&[op]), Target::Scale(scale));
            }
        }
    }
}
