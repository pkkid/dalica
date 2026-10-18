//! Soften with its paint mask (`editing.md` §8.4, M12.11): without strokes the whole photo
//! softens; with strokes it applies only where they painted, an eraser gives the photo back, Fade
//! 100 gives the input; strokes stay in the stored frame under every EXIF orientation and turn; the
//! mask and the render are the same at two sizes and in a zoomed region; more than 1 000 strokes are
//! refused; hostile and maximal stroke sets render in bounded time. Fingerprints:
//! `fingerprints/soften-*.json`.

use crate::common::{self, assert_two_sizes_agree, box_downscale, cases, check_fingerprint, exif, full, recipe, run, scene, stored, window};
use dalica_edit::fx::{self, Dabs, Space};
use dalica_edit::ops::soften::{FEATHER, MAX_POINTS, MAX_STROKES, Mode, Soften};
use dalica_edit::ops::{Adjust, Operation, Prepare};
use dalica_edit::{Geometry, Op, Point, Region, Source, Target, Typed, render, render_region};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::{Kind, Scene};
use proptest::prelude::*;

/// A painted stroke across the middle of the photo, drawn with a few points.
const PAINT: &str = "paint,0.06,0.2,0.4,0.45,0.55,0.7,0.45,0.8,0.6";
const PATH: [(f64, f64); 4] = [(0.2, 0.4), (0.45, 0.55), (0.7, 0.45), (0.8, 0.6)];

/// The distance from `(x, y)` to the polyline through `path` (output pixels).
fn distance(path: &[(f64, f64)], x: f64, y: f64) -> f64 {
    if let [(px, py)] = path {
        return (x - px).hypot(y - py);
    }
    path.windows(2)
        .map(|w| {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            (x - ax - t * dx).hypot(y - ay - t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

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

#[test]
fn renders_as_version_one_and_refuses_more_than_a_thousand_strokes() {
    let op = Op::parse(&format!("soften(50,50;{PAINT})"));
    assert!(matches!(op.typed(), Some(Ok(Typed::Soften(_)))));
    assert_eq!(op.version(), Some(1));
    let stroke = "paint,0.01,0.5,0.5";
    let at_cap = vec![stroke; MAX_STROKES].join(";");
    assert_eq!(Soften::parse(&format!("50,50;{at_cap}")).unwrap().strokes.len(), MAX_STROKES);
    let over = format!("soften(50,50;{})", vec![stroke; MAX_STROKES + 1].join(";"));
    let op = Op::parse(&over);
    assert!(matches!(op.typed(), Some(Err(_))));
    assert_eq!(op.to_string(), over, "kept as written");
    let img = scene(3, 40, 30);
    assert_eq!(run(&img, &["rotate(90)", &over]), run(&img, &["rotate(90)"]), "and skipped");
    // A stroke at the most points is read.
    let long: Vec<String> = (0..MAX_POINTS).map(|i| format!("{},0.5", i as f64 / MAX_POINTS as f64)).collect();
    assert_eq!(Soften::parse(&format!("50,50;erase,0.1,{}", long.join(","))).unwrap().strokes[0].points.len(), MAX_POINTS);
}

/// The mask is the strokes' dabs combined in order from 0 (`fx::paint`), with Soften's feather;
/// without strokes it is 1 everywhere.
#[test]
fn the_mask_is_the_strokes_dabs_in_order() {
    let (w, h) = (240u32, 160u32);
    let g = Geometry::new(&recipe(&["straighten(3)", "crop(0.1,0,0.9,1)"]), Orientation::default(), w, h);
    let soften = Soften::parse(&format!("50,50;{PAINT};erase,0.03,0.45,0.55,0.5,0.5;paint,0.02,0.3,0.7")).unwrap();
    let mask = soften.coverage(&g, w, h, (0, 0, w, h)).unwrap();
    let space = Space::from_geometry(&g, w, h).unwrap();
    let dabs: Vec<Dabs> = soften.strokes.iter().map(|s| Dabs::new(&space, &s.points, s.radius, FEATHER, s.mode == Mode::Erase)).collect();
    let want = fx::paint(&dabs, 0.0, 0, 0, w, h).unwrap();
    // (Within rounding: the mask measures a dab's distance as a square root, `fx` with `hypot`.)
    assert!(mask.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-5));
    assert!(mask.contains(&1.0) && mask.contains(&0.0));
    // A window of it is the same window of the whole.
    let part = soften.coverage(&g, w, h, (50, 70, 33, 21)).unwrap();
    for y in 0..21usize {
        assert_eq!(part[y * 33..(y + 1) * 33], mask[(70 + y) * 240 + 50..(70 + y) * 240 + 83]);
    }
    assert!(Soften::default().coverage(&g, w, h, (0, 0, 7, 5)).unwrap().iter().all(|v| *v == 1.0));
    assert_eq!(soften.coverage(&g, w, h, (0, 0, 0, 5)), None);
}

#[test]
fn without_strokes_the_whole_photo_softens_and_a_flat_patch_stays_flat() {
    let img = detailed(120, 80);
    let out = run(&img, &["soften(100,0)"]);
    // Detail (the difference between neighbours) drops everywhere, in each quarter.
    let detail = |img: &Rgba8, x0: u32, y0: u32| {
        let mut sum = 0.0;
        for y in y0 + 5..y0 + 35 {
            for x in x0 + 5..x0 + 55 {
                let (a, b) = (((y * 120 + x) * 4) as usize, ((y * 120 + x + 1) * 4) as usize);
                sum += f64::from(img.data[a].abs_diff(img.data[b]));
            }
        }
        sum
    };
    for (x0, y0) in [(0, 0), (60, 0), (0, 40), (60, 40)] {
        assert!(detail(&out, x0, y0) < 0.5 * detail(&img, x0, y0), "({x0}, {y0}): {} vs {}", detail(&out, x0, y0), detail(&img, x0, y0));
    }
    // A flat grey stays flat and glows a little brighter.
    let grey = Rgba8 { width: 50, height: 40, data: [128, 128, 128, 255].repeat(50 * 40) };
    let soft = run(&grey, &["soften(100,0)"]);
    let first = soft.data[0];
    assert!(soft.data.chunks(4).all(|p| p == [first, first, first, 255]));
    assert!(first > 140, "{first}");
}

#[test]
fn outside_a_painted_stroke_the_photo_is_unchanged() {
    let (w, h) = (240u32, 160u32);
    let img = detailed(w, h);
    let out = run(&img, &[&format!("soften(80,0;{PAINT})")]);
    let path: Vec<(f64, f64)> = PATH.iter().map(|(x, y)| (x * f64::from(w), y * f64::from(h))).collect();
    let r = 0.06 * f64::from(h);
    let (mut inside, mut changed) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let d = distance(&path, f64::from(x) + 0.5, f64::from(y) + 0.5);
            let at = ((y * w + x) * 4) as usize;
            if d > r {
                assert_eq!(out.data[at..at + 4], img.data[at..at + 4], "({x}, {y}) is {d:.1} px from the stroke");
            } else if d < 0.5 * r - 1.0 {
                inside += 1;
                changed += usize::from(out.data[at..at + 3] != img.data[at..at + 3]);
            }
        }
    }
    assert!(inside > 500 && changed * 10 > inside * 8, "{changed} of {inside} changed under the stroke");
}

#[test]
fn an_eraser_stroke_over_it_gives_the_photo_back() {
    let img = detailed(240, 160);
    // An eraser whose hard core covers the whole painted stroke: nothing is left.
    let erase = "erase,0.13,0.2,0.4,0.45,0.55,0.7,0.45,0.8,0.6";
    assert_eq!(run(&img, &[&format!("soften(80,0;{PAINT};{erase})")]), img);
    // Erasing only the first half gives the photo back there and leaves the rest softened.
    let half = run(&img, &[&format!("soften(80,0;{PAINT};erase,0.13,0.2,0.4,0.4,0.5)")]);
    let at = |img: &Rgba8, x: f64, y: f64| {
        let i = (((y * 160.0) as u32 * 240 + (x * 240.0) as u32) * 4) as usize;
        img.data[i..i + 3].to_vec()
    };
    assert_eq!(at(&half, 0.25, 0.43), at(&img, 0.25, 0.43));
    let painted = run(&img, &[&format!("soften(80,0;{PAINT})")]);
    assert_ne!(at(&painted, 0.25, 0.43), at(&img, 0.25, 0.43));
    let far: Vec<u8> = (0..40).flat_map(|i| at(&half, 0.66 + f64::from(i) * 0.001, 0.46)).collect();
    let far_img: Vec<u8> = (0..40).flat_map(|i| at(&img, 0.66 + f64::from(i) * 0.001, 0.46)).collect();
    assert_ne!(far, far_img);
    // A stroke painted after an eraser paints again.
    assert_ne!(run(&img, &[&format!("soften(80,0;{PAINT};{erase};{PAINT})")]), img);
}

#[test]
fn fade_100_or_softness_0_gives_the_input() {
    let img = detailed(90, 60);
    for entry in ["soften(50,100)", "soften(100,100)", "soften(0,0)", &format!("soften(80,100;{PAINT})"), &format!("soften(0,0;{PAINT})")] {
        assert_eq!(run(&img, &[entry]), img, "{entry}");
    }
    // An erase stroke alone leaves nothing to soften.
    assert_eq!(run(&img, &["soften(80,0;erase,0.2,0.5,0.5)"]), img);
}

#[test]
fn a_stroke_on_a_turned_photo_lands_where_its_stored_points_say() {
    let up = detailed(90, 60);
    let ops = ["soften(90,0;paint,0.08,0.2,0.3,0.35,0.3)".to_owned()];
    let upright = run(&up, &[&ops[0]]);
    assert_ne!(upright, up);
    for o in 1..=8u8 {
        let file = stored(&up, o);
        for turn in [&[][..], &["rotate(90)"], &["flip_h()", "rotate(270)"]] {
            let all: Vec<&str> = std::iter::once(ops[0].as_str()).chain(turn.iter().copied()).collect();
            let r = recipe(&all);
            let out = render(Source::new(&file, exif(o)), &r, Target::Full).unwrap();
            // The photo after its EXIF orientation is softened where the stored points say; the
            // turns then move the softened picture.
            assert_eq!(out, r.orientation().apply(upright.clone()).unwrap(), "EXIF {o} {turn:?}");
        }
    }
    // Turned 90° the stroke at the stored (0.25, 0.3) shows at the displayed point it maps to.
    let r = recipe(&[&ops[0], "rotate(90)"]);
    let shown = render(Source::upright(&up), &r, Target::Full).unwrap();
    let plain = run(&up, &["rotate(90)"]);
    let p = Point::new(0.25, 0.3).to_display(r.orientation());
    let at = ((p.y * f64::from(shown.height)) as u32 * shown.width + (p.x * f64::from(shown.width)) as u32) as usize * 4;
    assert_ne!(shown.data[at..at + 3], plain.data[at..at + 3]);
    // Far from it (the stored (0.8, 0.8)) nothing changed.
    let q = Point::new(0.8, 0.8).to_display(r.orientation());
    let at = ((q.y * f64::from(shown.height)) as u32 * shown.width + (q.x * f64::from(shown.width)) as u32) as usize * 4;
    assert_eq!(shown.data[at..at + 3], plain.data[at..at + 3]);
}

#[test]
fn a_region_is_that_window_of_the_whole_render() {
    let up = detailed(120, 90);
    let cases: [(u8, &[&str]); 4] = [
        (1, &["soften(50,50)"]),
        (6, &["soften(70,20;paint,0.05,0.2,0.3,0.6,0.7;erase,0.03,0.4,0.5)", "rotate(90)"]),
        (3, &["straighten(-4)", "crop(0.1,0.2,0.8,0.9)", "soften(100,0;paint,0.1,0.5,0.5,0.3,0.4)", "tuning(0.3,0,0,0,none)"]),
        (8, &["soften(40,30;paint,0.02,0.1,0.1,0.9,0.9;paint,0.02,0.9,0.1,0.1,0.9)", "flip_h()"]),
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

/// The mask at two sizes, the larger box-downscaled, agrees within a fraction of a pixel's edge.
#[test]
fn the_mask_is_the_same_at_two_sizes() {
    let soften = Soften::parse(&format!("50,50;{PAINT};erase,0.03,0.45,0.55,0.5,0.5;paint,0.02,0.3,0.7")).unwrap();
    let as_image = |w: u32, h: u32| {
        let g = Geometry::new(&recipe(&["straighten(2)"]), Orientation::default(), w, h);
        let m = soften.coverage(&g, w, h, (0, 0, w, h)).unwrap();
        Rgba8 { width: w, height: h, data: m.iter().flat_map(|v| [(v * 255.0).round() as u8; 3].into_iter().chain([255])).collect() }
    };
    let (small, large) = (as_image(150, 100), as_image(600, 400));
    let down = box_downscale(&large, 150, 100);
    let diff: Vec<u8> = small.data.iter().zip(&down.data).map(|(a, b)| a.abs_diff(*b)).collect();
    let mean = diff.iter().map(|d| f64::from(*d)).sum::<f64>() / diff.len() as f64;
    assert!(mean < 1.0 && diff.iter().all(|d| *d < 40), "mean {mean}, most {:?}", diff.iter().max());
}

#[test]
fn two_sizes_agree_and_match_the_fingerprints() {
    let up = common::rgba(&Scene::new(5, 300, 200).with_kind(Kind::Blobs).render().unwrap());
    for (case, ops) in [
        // The defaults over the whole photo.
        ("soften-whole", &["soften(50,50)"][..]),
        // A painted stroke at full strength (no Fade) across the middle, the rest untouched.
        ("soften-stroke", &["soften(100,0;paint,0.1,0.15,0.5,0.4,0.35,0.6,0.6,0.85,0.45)"]),
    ] {
        let small = assert_two_sizes_agree(Source::upright(&up), &recipe(ops), 160, 480, 2.0);
        check_fingerprint(case, &small, 0.5);
    }
}

/// The cap: 1 000 strokes of many points with a large brush render in bounded time (a coarser
/// grid for the mask past its budget); the full gate draws every stroke with the most points.
#[test]
fn the_most_strokes_render_in_bounded_time() {
    let (img, points) = if full() { (scene(53, 800, 600), MAX_POINTS) } else { (scene(53, 160, 120), 32) };
    let strokes: Vec<String> = (0..MAX_STROKES)
        .map(|i| {
            let mode = if i % 5 == 4 { "erase" } else { "paint" };
            let pts: Vec<String> = (0..points)
                .map(|k| {
                    let t = k as f64 / points as f64;
                    format!("{:.4},{:.4}", (t * 7.0 + i as f64 * 0.37).sin() * 1.4 + 0.5, (t * 5.0 + i as f64 * 0.11).cos() * 1.4 + 0.5)
                })
                .collect();
            format!("{mode},{},{}", [0.25, 0.05, 0.001][i % 3], pts.join(","))
        })
        .collect();
    let op = format!("soften(60,10;{})", strokes.join(";"));
    assert!(matches!(Op::parse(&op).typed(), Some(Ok(_))));
    let start = std::time::Instant::now();
    let out = run(&img, &[&op]);
    assert_eq!((out.width, out.height), (img.width, img.height));
    assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
}

fn hostile() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("0".to_owned()),
        Just("-0".to_owned()),
        Just("NaN".to_owned()),
        Just("inf".to_owned()),
        Just("-inf".to_owned()),
        Just("1e308".to_owned()),
        Just("1e-320".to_owned()),
        Just(String::new()),
        (-3.0f64..4.0).prop_map(|v| v.to_string()),
        (0.0f64..1.0).prop_map(|v| v.to_string()),
    ]
}

fn stroke_text() -> impl Strategy<Value = String> {
    (prop::sample::select(vec!["paint", "erase", "", "brush"]), prop::collection::vec(hostile(), 1..=7))
        .prop_map(|(mode, v)| format!("{mode},{}", v.join(",")))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(120)))]

    #[test]
    fn hostile_strokes_never_panic(
        head in prop::collection::vec(hostile(), 2..=2),
        strokes in prop::collection::vec(stroke_text(), 0..5),
        w in 1u32..40,
        h in 1u32..40,
        o in 1u8..=8,
        extra in prop::sample::select(vec!["", "crop(0.2,0.1,0.7,0.9)", "straighten(-7)", "rotate(90)"]),
        scale in prop::sample::select(vec![0.3, 1.0, 3.0]),
    ) {
        let params = std::iter::once(head.join(",")).chain(strokes).collect::<Vec<_>>().join(";");
        if let Ok(s) = Soften::parse(&params) {
            prop_assert_eq!(Soften::parse(&s.to_string()).unwrap(), s.clone());
            for st in &s.strokes {
                prop_assert!((0.001..=0.25).contains(&st.radius));
                prop_assert!(st.points.iter().all(|p| (-1.0..=2.0).contains(&p.x) && (-1.0..=2.0).contains(&p.y)));
            }
            let g = Geometry::new(&recipe(&[]), Orientation::default(), w, h);
            let ctx = Prepare { width: w, height: h, scale: 1.0, proxy: None, geometry: &g };
            let _ = s.prepare(&ctx);
            if let Some(m) = s.coverage(&g, w, h, (0, 0, w, h)) {
                prop_assert!(m.iter().all(|v| (0.0..=1.0).contains(v)));
            }
        }
        let file = stored(&scene(59, w.max(2), h.max(2)), o);
        let source = Source::new(&file, exif(o));
        let op = format!("soften({params})");
        let ops: Vec<&str> = [extra, op.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        let r = recipe(&ops);
        let whole = render(source, &r, Target::Scale(scale));
        prop_assert!(whole.is_ok());
        let whole = whole.unwrap();
        let region = Region { scale, x: whole.width / 3, y: whole.height / 3, width: 5, height: 5 };
        let part = render_region(source, &r, region);
        prop_assert!(part.is_ok());
        prop_assert_eq!(part.unwrap(), window(&whole, region.x, region.y, 5.min(whole.width - region.x), 5.min(whole.height - region.y)));
    }
}
