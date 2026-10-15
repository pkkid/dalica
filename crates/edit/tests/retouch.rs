//! Retouch (`editing.md` §6.4, `data-model.md` §3.1, §3.3): a planted blemish is removed by one
//! stroke; strokes apply in order; the strokes are in the stored frame, so EXIF orientations and the
//! recipe's rotations never move them; two sizes agree; a stroke outside the photo or the crop is
//! clipped; a region is the same window of the whole render and reads only as far as the strokes
//! reach; parameters print back, clamp and refuse; hostile strokes never panic. Fingerprints:
//! `fingerprints/retouch-*.json`.
//!
//! Partly ported from LightCraft `crates/pipeline/src/spots.rs` tests (`remove_erases_a_dot`,
//! `a_window_grows_to_hold_the_spots_that_touch_it_and_their_sources`) at 76b53778 (MIT OR
//! Apache-2.0; `NOTICE`).

use crate::common::{self, assert_two_sizes_agree, cases, check_fingerprint, exif, recipe, run, scene, stored, window};
use dalica_edit::ops::retouch::{MAX_RADIUS, MAX_STROKES, MIN_RADIUS, Retouch, Stroke};
use dalica_edit::ops::{Adjust, Operation, Prepare, Reach};
use dalica_edit::{Geometry, Op, Point, Region, Source, Target, Typed, render, render_region};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::{Kind, Scene};
use proptest::prelude::*;

/// The mean grey level (mean of R, G, B) of the pixels whose centre lies between `r0` and `r1`
/// pixels from `(cx, cy)`.
fn ring_mean(img: &Rgba8, cx: f64, cy: f64, r0: f64, r1: f64) -> f64 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in 0..img.height {
        for x in 0..img.width {
            let d = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy);
            if d >= r0 && d < r1 {
                let at = ((y * img.width + x) * 4) as usize;
                sum += img.data[at..at + 3].iter().map(|v| f64::from(*v)).sum::<f64>() / 3.0;
                n += 1.0;
            }
        }
    }
    sum / n
}

/// `img` with a near-black disc of `radius` pixels centred on `(cx, cy)` pixels, its edge softened
/// from 0.8 to 1.2 radii (a hard edge resamples differently at every size).
fn plant(img: &Rgba8, cx: f64, cy: f64, radius: f64) -> Rgba8 {
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            let d = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy);
            let a = ((1.2 * radius - d) / (0.4 * radius)).clamp(0.0, 1.0);
            let at = ((y * img.width + x) * 4) as usize;
            for (c, dark) in [12.0, 10.0, 14.0].into_iter().enumerate() {
                let v = f64::from(out.data[at + c]);
                out.data[at + c] = (v + (dark - v) * a).round() as u8;
            }
        }
    }
    out
}

#[test]
fn parameters_print_back_clamp_and_refuse() {
    assert_eq!(Retouch::default().to_string(), "");
    assert_eq!(Retouch::parse("").unwrap(), Retouch::default());
    let text = "0.1,0.2,0.3,0.4,0.02;0.5,0.5,0.6,0.6,0.01";
    let r = Retouch::parse(text).unwrap();
    assert_eq!(r.strokes.len(), 2);
    assert_eq!(r.strokes[0], Stroke { source: Point::new(0.1, 0.2), target: Point::new(0.3, 0.4), radius: 0.02 });
    assert_eq!(r.to_string(), text);
    assert_eq!(Retouch::parse(&r.to_string()).unwrap(), r);
    // Through the recipe: typed, labelled, the version that renders, printed back verbatim.
    let op = Op::parse(&format!("retouch({text})"));
    assert!(matches!(op.typed(), Some(Ok(Typed::Retouch(ref t))) if *t == r));
    assert_eq!(op.label(), Some("Retouch"));
    assert_eq!(op.version(), Some(1));
    assert_eq!(op.to_string(), format!("retouch({text})"));
    // Out of range: clamped.
    let c = Retouch::parse("-7,9,0.5,0.5,1e9;0.5,0.5,0.5,0.5,0;0.5,0.5,1e308,-1e308,-3").unwrap();
    assert_eq!(c.strokes[0].source, Point::new(-1.0, 2.0));
    assert_eq!(c.strokes[0].radius, MAX_RADIUS);
    assert_eq!(c.strokes[1].radius, MIN_RADIUS);
    assert_eq!(c.strokes[2].target, Point::new(2.0, -1.0));
    assert_eq!(c.strokes[2].radius, MIN_RADIUS);
    // Not numbers, the wrong count, empty strokes, too many strokes: refused, the entry kept.
    for bad in ["NaN,0,0,0,0.1", "0,0,0,0,inf", "0,0,0,0", "0,0,0,0,0.1,0", ";", "0,0,0,0,0.1;", ";;;", "a,b,c,d,e", "0, 0,0,0,0.1"] {
        assert!(Retouch::parse(bad).is_err(), "{bad}");
        let op = Op::parse(&format!("retouch({bad})"));
        assert!(matches!(op.typed(), Some(Err(_))), "{bad}");
        assert_eq!(op.to_string(), format!("retouch({bad})"));
    }
    let stroke = "0.1,0.1,0.2,0.2,0.01";
    let at_cap = vec![stroke; MAX_STROKES].join(";");
    assert_eq!(Retouch::parse(&at_cap).unwrap().strokes.len(), MAX_STROKES);
    let over = vec![stroke; MAX_STROKES + 1].join(";");
    assert!(Retouch::parse(&over).is_err());
}

#[test]
fn no_strokes_change_nothing() {
    let img = scene(5, 90, 60);
    assert_eq!(run(&img, &["retouch()"]), img);
    // A stroke wholly outside the photo draws nothing.
    assert_eq!(run(&img, &["retouch(0.5,0.5,1.8,0.5,0.05;0.5,0.5,0.5,-0.9,0.1)"]), img);
}

#[test]
fn one_stroke_removes_a_planted_blemish() {
    let (w, h) = (240u32, 160u32);
    let clean = scene(21, w, h);
    let (cx, cy) = (0.5 * f64::from(w), 0.5 * f64::from(h));
    let blemish = 0.03 * f64::from(h);
    let spotted = plant(&clean, cx, cy, blemish);
    // The stroke: 0.06 of the shorter side (full strength out to 0.6 of it, past the blemish),
    // copying from a clean area to the right.
    let r = 0.06 * f64::from(h);
    let healed = run(&spotted, &["retouch(0.62,0.5,0.5,0.5,0.06)"]);
    let disc = |img: &Rgba8| ring_mean(img, cx, cy, 0.0, blemish);
    let around = |img: &Rgba8| ring_mean(img, cx, cy, r, 2.0 * r);
    let (before, after, want) = (disc(&spotted), disc(&healed), disc(&clean));
    // Tolerance: the patch's mean within 10 grey levels of its surroundings (and of the clean
    // photo there); the blemish was over 40 levels off.
    assert!((before - around(&spotted)).abs() > 40.0, "the blemish shows: {before} vs {}", around(&spotted));
    assert!((after - around(&healed)).abs() <= 10.0, "healed {after} vs surroundings {}", around(&healed));
    assert!((after - want).abs() <= 10.0, "healed {after} vs clean {want}");
    println!("blemish {before:.1}, healed {after:.1}, surroundings {:.1}, clean {want:.1}", around(&healed));
    // Outside the stroke nothing changes.
    for y in 0..h {
        for x in 0..w {
            if (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) >= r + 1.0 {
                let at = ((y * w + x) * 4) as usize;
                assert_eq!(healed.data[at..at + 4], spotted.data[at..at + 4], "({x}, {y})");
            }
        }
    }
}

#[test]
fn strokes_apply_in_order() {
    let (w, h) = (240u32, 160u32);
    let clean = scene(23, w, h);
    let (t1, t2) = ((0.3 * f64::from(w), 0.5 * f64::from(h)), (0.7 * f64::from(w), 0.5 * f64::from(h)));
    let spotted = plant(&clean, t1.0, t1.1, 0.03 * f64::from(h));
    // First heal the blemish at T1 from a clean area, then copy T1 onto T2: T2 receives clean
    // detail. The other way round, T2 receives the blemish.
    let heal_t1 = "0.3,0.3,0.3,0.5,0.06";
    let copy_t1_to_t2 = "0.3,0.5,0.7,0.5,0.06";
    let in_order = run(&spotted, &[&format!("retouch({heal_t1};{copy_t1_to_t2})")]);
    let reversed = run(&spotted, &[&format!("retouch({copy_t1_to_t2};{heal_t1})")]);
    let at_t2 = |img: &Rgba8| ring_mean(img, t2.0, t2.1, 0.0, 0.03 * f64::from(h));
    let want = at_t2(&clean);
    assert!((at_t2(&in_order) - want).abs() <= 10.0, "in order: {} vs {want}", at_t2(&in_order));
    assert!(want - at_t2(&reversed) > 30.0, "reversed: the blemish is copied: {} vs {want}", at_t2(&reversed));
    // Two entries apply in order too, as one entry with both strokes (the picture is rounded to 8
    // bits between them, so within a level or two).
    let two = run(&spotted, &[&format!("retouch({heal_t1})"), &format!("retouch({copy_t1_to_t2})")]);
    let most = two.data.iter().zip(&in_order.data).map(|(a, b)| a.abs_diff(*b)).max();
    assert!(most <= Some(2), "{most:?}");
}

#[test]
fn strokes_are_in_the_stored_frame() {
    let up = scene(31, 60, 40);
    let ops = ["retouch(0.2,0.3,0.6,0.4,0.08;0.8,0.8,0.3,0.7,0.05)"];
    let upright = run(&up, &ops);
    assert_ne!(upright, up);
    for o in 1..=8u8 {
        let file = stored(&up, o);
        for turn in [&[][..], &["rotate(90)"], &["flip_h()", "rotate(270)"]] {
            let r = recipe(&[&ops[..], turn].concat());
            let out = render(Source::new(&file, exif(o)), &r, Target::Full).unwrap();
            // The photo after its EXIF orientation is retouched as stored; the rotations and flips
            // turn the result and never move a stroke.
            let expected = r.orientation().apply(upright.clone()).unwrap();
            assert_eq!(out, expected, "EXIF {o} {turn:?}");
        }
    }
}

#[test]
fn a_stroke_outside_the_photo_or_the_crop_is_clipped() {
    let img = scene(37, 120, 80);
    // A stroke reaching over the edge, from a source past the other edge: changes only near its
    // target, no panic.
    let edge = run(&img, &["retouch(-0.5,0.5,0.99,0.5,0.1)"]);
    assert_ne!(edge, img);
    for y in 0..img.height {
        for x in 0..img.width - 20 {
            let at = ((y * img.width + x) * 4) as usize;
            assert_eq!(edge.data[at..at + 4], img.data[at..at + 4], "({x}, {y})");
        }
    }
    // After a crop: a stroke outside the kept part draws nothing; one inside draws only there.
    let crop = "crop(0.5,0,1,1)";
    assert_eq!(run(&img, &[crop, "retouch(0.75,0.5,0.25,0.5,0.05)"]), run(&img, &[crop]));
    assert_eq!(run(&img, &["retouch(0.75,0.5,0.25,0.5,0.05)", crop]), run(&img, &[crop]));
    let inside = run(&img, &[crop, "retouch(0.1,0.5,0.75,0.5,0.05)"]);
    assert_ne!(inside, run(&img, &[crop]));
    // The stroke at the same place after a straighten lands where the stored point shows.
    let straight = recipe(&["straighten(5)", "retouch(0.3,0.3,0.6,0.6,0.05)"]);
    let g = Geometry::new(&straight, Orientation::default(), img.width, img.height);
    let p = g.stored_to_output(Point::new(0.6, 0.6)).unwrap();
    let out = render(Source::upright(&img), &straight, Target::Full).unwrap();
    let base = run(&img, &["straighten(5)"]);
    let (px, py) = ((p.x * f64::from(out.width)) as u32, (p.y * f64::from(out.height)) as u32);
    let at = ((py * out.width + px) * 4) as usize;
    assert_ne!(out.data[at..at + 3], base.data[at..at + 3], "the patch is at the mapped point");
}

#[test]
fn a_region_is_that_window_of_the_whole_render() {
    let up = scene(41, 120, 90);
    let cases: [(u8, &[&str]); 4] = [
        (1, &["retouch(0.7,0.2,0.2,0.3,0.06;0.2,0.3,0.6,0.7,0.04)"]),
        (6, &["retouch(0.1,0.1,0.9,0.9,0.1)", "rotate(90)"]),
        (3, &["straighten(-4)", "crop(0.1,0.2,0.8,0.9)", "retouch(0.5,0.5,0.3,0.4,0.05;0.3,0.4,0.6,0.6,0.08)", "tuning(0.3,0,0,0,none)"]),
        (8, &["retouch(0.2,0.2,0.25,0.25,0.02;0.25,0.25,0.3,0.3,0.02;0.3,0.3,0.35,0.35,0.02)", "flip_h()"]),
    ];
    for (o, ops) in cases {
        let file = stored(&up, o);
        let source = Source::new(&file, exif(o));
        let r = recipe(ops);
        for scale in [1.0, 0.5, 0.37, 2.5] {
            let whole = render(source, &r, Target::Scale(scale)).unwrap();
            let (w, h) = (whole.width, whole.height);
            for (x, y, ww, wh) in
                [(0, 0, w, h), (0, 0, 1, 1), (w / 3, h / 4, w / 2, h / 3), (w - 5, h - 4, 5, 4), (w / 2, 1, 1, h - 1), (w / 5, h / 5, 7, 9)]
            {
                let region = Region { scale, x, y, width: ww.max(1), height: wh.max(1) };
                let part = render_region(source, &r, region).unwrap();
                assert_eq!(part, window(&whole, x, y, ww.max(1), wh.max(1)), "EXIF {o} {ops:?} at {scale}: {region:?}");
            }
        }
    }
}

#[test]
fn small_windows_of_a_larger_photo_see_every_stroke_they_need() {
    // Small strokes on a larger photo, so a region's reach is a fraction of it: a chain (each stroke
    // copying from where the one before drew), strokes far apart, one reading from further off and
    // one drawn over an earlier one; every window of a grid equals that window of the whole.
    let img = scene(45, 600, 400);
    let r = recipe(&[
        "retouch(0.1,0.1,0.15,0.12,0.015;0.15,0.12,0.2,0.14,0.015;0.2,0.14,0.25,0.16,0.02;0.66,0.56,0.6,0.5,0.01;0.62,0.5,0.6,0.52,0.02;0.8,0.2,0.75,0.25,0.01)",
    ]);
    let g = Geometry::new(&r, Orientation::default(), img.width, img.height);
    let ctx = Prepare { width: img.width, height: img.height, scale: 1.0, proxy: None, geometry: &g };
    let typed = Retouch::parse("0.1,0.1,0.15,0.12,0.015;0.15,0.12,0.2,0.14,0.015;0.2,0.14,0.25,0.16,0.02;0.66,0.56,0.6,0.5,0.01;0.62,0.5,0.6,0.52,0.02;0.8,0.2,0.75,0.25,0.01").unwrap();
    let Some(Reach::Pixels(reach)) = typed.prepare(&ctx).map(|p| p.reach()) else { panic!("a bounded reach") };
    assert!(reach < 200, "{reach}");
    for scale in [1.0, 0.5, 1.5] {
        let whole = render(Source::upright(&img), &r, Target::Scale(scale)).unwrap();
        for y in (0..whole.height).step_by(71) {
            for x in (0..whole.width).step_by(97) {
                let region = Region { scale, x, y, width: 24, height: 20 };
                let (w, h) = (24.min(whole.width - x), 20.min(whole.height - y));
                let part = render_region(Source::upright(&img), &r, region).unwrap();
                assert_eq!(part, window(&whole, x, y, w, h), "{region:?}");
            }
        }
    }
}

#[test]
fn a_pass_reads_only_as_far_as_its_strokes_reach() {
    let (w, h) = (4000u32, 3000u32);
    let g = Geometry::new(&recipe(&[]), Orientation::default(), w, h);
    let ctx = Prepare { width: w, height: h, scale: 1.0, proxy: None, geometry: &g };
    let reach = |text: &str| Retouch::parse(text).unwrap().prepare(&ctx).map(|p| p.reach());
    assert_eq!(reach(""), None, "no strokes, no pass");
    // r = 0.01 × 3000 = 30 px; the source 0.02 × 4000 = 80 px away: the box (4 r), the offset and
    // the bilinear taps.
    let Some(Reach::Pixels(near)) = reach("0.52,0.5,0.5,0.5,0.01") else { panic!("a bounded reach") };
    assert!((200..=210).contains(&near), "{near}");
    // A source further away reads further.
    let Some(Reach::Pixels(far)) = reach("0.9,0.5,0.5,0.5,0.01") else { panic!("a bounded reach") };
    assert!(far > near + 1500 && far < 1800, "{far}");
    // Two strokes apart read as far as the further one; a stroke reading where an earlier one drew
    // reads as far as both together.
    let Some(Reach::Pixels(apart)) = reach("0.12,0.1,0.1,0.1,0.01;0.9,0.5,0.5,0.5,0.01") else { panic!("a bounded reach") };
    assert_eq!(apart, far);
    let Some(Reach::Pixels(chained)) = reach("0.52,0.5,0.5,0.5,0.01;0.5,0.5,0.1,0.5,0.01") else { panic!("a bounded reach") };
    assert!(chained >= near + far - 1 && chained <= near + far + 1, "{chained} vs {near} + {far}");
    // The other order: the first stroke draws where the second one reads only if they meet.
    let Some(Reach::Pixels(unchained)) = reach("0.5,0.5,0.1,0.5,0.01;0.52,0.5,0.5,0.5,0.01") else { panic!("a bounded reach") };
    assert!(unchained >= far - 1 && unchained <= far + 1, "{unchained} vs {far}");
    // A zoomed region of a large photo renders only its window and the strokes' reach.
    let img = scene(43, 1600, 1200);
    let r = recipe(&["retouch(0.55,0.5,0.5,0.5,0.01)"]);
    let region = Region { scale: 1.0, x: 780, y: 580, width: 40, height: 40 };
    let part = render_region(Source::upright(&img), &r, region).unwrap();
    let whole = render(Source::upright(&img), &r, Target::Full).unwrap();
    assert_eq!(part, window(&whole, 780, 580, 40, 40));
}

#[test]
fn two_sizes_agree_and_match_the_fingerprints() {
    // A sky over ground with a sun, and two planted blemishes: one in the ground, one in the sky.
    let horizon = common::rgba(&Scene::new(5, 300, 200).with_kind(Kind::Horizon).render().unwrap());
    let up = plant(&plant(&horizon, 150.0, 150.0, 9.0), 210.0, 40.0, 9.0);
    for (case, ops) in [
        // The ground's blemish removed.
        ("retouch-one", &["retouch(0.64,0.75,0.5,0.75,0.1)"][..]),
        // Both blemishes removed, then the sun copied into the sky (in the sky's tone).
        ("retouch-three", &["retouch(0.64,0.75,0.5,0.75,0.1;0.86,0.2,0.7,0.2,0.1;0.46,0.24,0.88,0.14,0.08)"]),
        // The ground's blemish removed after a straighten and a crop (the sky's left alone).
        ("retouch-straighten-crop", &["straighten(4)", "crop(0.1,0.1,0.9,0.9)", "retouch(0.64,0.75,0.5,0.75,0.1)"]),
    ] {
        let small = assert_two_sizes_agree(Source::upright(&up), &recipe(ops), 64, 240, 2.0);
        check_fingerprint(case, &small, 0.5);
    }
}

#[test]
fn many_strokes_render() {
    // The cap's worth of strokes on a small photo: no panic, in time.
    let img = scene(53, 64, 48);
    let strokes: Vec<String> = (0..MAX_STROKES)
        .map(|i| format!("{},{},{},{},0.05", (i % 7) as f64 / 7.0, (i % 5) as f64 / 5.0, (i % 11) as f64 / 11.0, (i % 3) as f64 / 3.0))
        .collect();
    let op = format!("retouch({})", strokes.join(";"));
    let start = std::time::Instant::now();
    let out = run(&img, &[&op]);
    assert_eq!((out.width, out.height), (64, 48));
    assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
}

fn hostile() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("0".to_owned()),
        Just("-0".to_owned()),
        Just("NaN".to_owned()),
        Just("inf".to_owned()),
        Just("-inf".to_owned()),
        Just("1e308".to_owned()),
        Just("-1e308".to_owned()),
        Just("1e-320".to_owned()),
        Just(String::new()),
        (-3.0f64..4.0).prop_map(|v| v.to_string()),
        (0.0f64..1.0).prop_map(|v| v.to_string()),
    ]
}

fn stroke_text() -> impl Strategy<Value = String> {
    prop::collection::vec(hostile(), 4..=6).prop_map(|v| v.join(","))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(120)))]

    #[test]
    fn hostile_strokes_never_panic(
        strokes in prop::collection::vec(stroke_text(), 0..6),
        w in 1u32..40,
        h in 1u32..40,
        o in 1u8..=8,
        extra in prop::sample::select(vec!["", "crop(0.2,0.1,0.7,0.9)", "straighten(-7)", "rotate(90)"]),
        scale in prop::sample::select(vec![0.3, 1.0, 3.0]),
    ) {
        let params = strokes.join(";");
        if let Ok(r) = Retouch::parse(&params) {
            // What parses prints back to the same strokes, every number in range.
            prop_assert_eq!(Retouch::parse(&r.to_string()).unwrap(), r.clone());
            for s in &r.strokes {
                prop_assert!(s.radius >= MIN_RADIUS && s.radius <= MAX_RADIUS);
                prop_assert!([s.source.x, s.source.y, s.target.x, s.target.y].iter().all(|v| (-1.0..=2.0).contains(v)));
            }
        }
        let file = stored(&scene(59, w.max(2), h.max(2)), o);
        let source = Source::new(&file, exif(o));
        let op = format!("retouch({params})");
        let ops: Vec<&str> = [extra, op.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        let r = recipe(&ops);
        let whole = render(source, &r, Target::Scale(scale));
        prop_assert!(whole.is_ok());
        let whole = whole.unwrap();
        prop_assert_eq!(whole.data.len(), (whole.width * whole.height * 4) as usize);
        let fit = render(source, &r, Target::Fit { width: 7, height: 7 });
        prop_assert!(fit.is_ok());
        let region = Region { scale, x: whole.width / 3, y: whole.height / 3, width: 5, height: 5 };
        let part = render_region(source, &r, region);
        prop_assert!(part.is_ok());
        let part = part.unwrap();
        prop_assert_eq!(part, window(&whole, region.x, region.y, 5.min(whole.width - region.x), 5.min(whole.height - region.y)));
    }
}
