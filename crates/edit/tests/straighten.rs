//! Straighten (`editing.md` §6.2, `data-model.md` §3.1, §3.3): `straighten(deg)` parsed, clamped to
//! ±10° or refused; 0° is the identity; any angle up to ±10° leaves no empty corner and is the
//! least enlargement that does; two sizes agree and three angles match their fingerprints; the
//! turn is clockwise for positive angles; the straighten grid of `guides` (PhotoCraft's test
//! ported). Fingerprints: `fingerprints/straighten-*.json`.
//!
//! The grid test is ported from PhotoCraft `crates/ui-egui/src/crop_overlay.rs`
//! (`grid_spacing_is_bounded`) at revision 32c918aa (MIT OR Apache-2.0; `NOTICE`).

mod common;

use common::{assert_two_sizes_agree, check_fingerprint, recipe, scene};
use dalica_edit::guides::{self, GRID_MAX};
use dalica_edit::ops::Operation;
use dalica_edit::ops::straighten::{MAX_DEGREES, Straighten};
use dalica_edit::{Geometry, Op, Point, Source, Target, Typed, render};
use dalica_image::{Orientation, Rgba8};
use proptest::prelude::*;

fn upright() -> Orientation {
    Orientation::default()
}

#[test]
fn straighten_parameters_print_back_and_hostile_ones_are_clamped_or_refused() {
    for (text, degrees) in [("0", 0.0), ("1.5", 1.5), ("-10", -10.0), ("10", 10.0), ("-0.1", -0.1), ("2.25", 2.25)] {
        let s = Straighten::parse(text).unwrap();
        assert_eq!(s.degrees, degrees);
        assert_eq!(s.to_string(), text);
        assert_eq!(Op::from(Typed::Straighten(s)).to_string(), format!("straighten({text})"));
    }
    // Outside the range: clamped to it.
    assert_eq!(Straighten::parse("45").unwrap().degrees, MAX_DEGREES);
    assert_eq!(Straighten::parse("-1e300").unwrap().degrees, -MAX_DEGREES);
    assert_eq!(Straighten::parse("-0").unwrap().to_string(), "0");
    // Not finite, not a number, the wrong count: refused (kept and skipped).
    for bad in ["NaN", "inf", "-inf", "", "1,2", " 1", "1 ", "ten", "1e999"] {
        assert!(Straighten::parse(bad).is_err(), "{bad}");
        assert!(matches!(Op::parse(&format!("straighten({bad})")).typed(), Some(Err(_)) | None), "{bad}");
    }
    assert_eq!(Straighten::new(f64::NAN), None);
    assert_eq!(Straighten::default().degrees, 0.0);
    assert_eq!(Straighten::VERSION, 1);
}

#[test]
fn zero_degrees_is_the_identity() {
    let img = scene(6, 90, 60);
    let source = Source::new(&img, Orientation::new(5).unwrap());
    for target in [Target::Full, Target::Fit { width: 40, height: 40 }, Target::Scale(1.3)] {
        for ops in [vec!["straighten(0)"], vec!["straighten(0)", "rotate(90)"], vec!["crop(0.1,0.1,0.6,0.9)", "straighten(-0)"]] {
            let without: Vec<&str> = ops.iter().copied().filter(|o| !o.starts_with("straighten")).collect();
            assert_eq!(render(source, &recipe(&ops), target).unwrap(), render(source, &recipe(&without), target).unwrap(), "{ops:?} {target:?}");
        }
    }
    // A refused straighten renders nothing either.
    let plain = render(source, &recipe(&[]), Target::Full).unwrap();
    for bad in ["straighten(NaN)", "straighten()", "straighten(-inf)"] {
        assert_eq!(render(source, &recipe(&[bad]), Target::Full).unwrap(), plain, "{bad}");
    }
}

/// Whether the output's corners all land inside the stored picture, and whether they would not
/// if the picture were enlarged 1 % less.
fn corners(g: &Geometry) -> Vec<Point> {
    [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().map(|&(x, y)| g.output_to_stored(Point::new(x, y)).unwrap()).collect()
}

#[test]
fn up_to_ten_degrees_leaves_no_empty_corner_with_the_least_enlargement() {
    for (w, h) in [(300u32, 200u32), (200, 300), (100, 100), (4000, 30)] {
        for deg in [-10.0, -7.5, -0.1, 0.1, 3.0, 10.0, 25.0] {
            let r = recipe(&[&format!("straighten({deg})")]);
            let g = Geometry::new(&r, upright(), w, h);
            assert_eq!(g.size(), (f64::from(w), f64::from(h)), "the output keeps the picture's size");
            let cs = corners(&g);
            for c in &cs {
                assert!((-1e-9..=1.0 + 1e-9).contains(&c.x) && (-1e-9..=1.0 + 1e-9).contains(&c.y), "{w}×{h} at {deg}°: corner {c:?} outside");
            }
            // Least: some corner touches the picture's edge.
            let touches = cs.iter().any(|c| c.x.abs() < 1e-9 || c.y.abs() < 1e-9 || (c.x - 1.0).abs() < 1e-9 || (c.y - 1.0).abs() < 1e-9);
            assert!(touches, "{w}×{h} at {deg}°: {cs:?}");
        }
    }
    // On pixels: a picture of one colour straightened stays that colour everywhere, opaque.
    let flat = Rgba8 { width: 60, height: 40, data: [200u8, 90, 30, 255].repeat(60 * 40) };
    for deg in ["-10", "10"] {
        let out = render(Source::upright(&flat), &recipe(&[&format!("straighten({deg})")]), Target::Full).unwrap();
        assert_eq!((out.width, out.height), (60, 40));
        assert!(out.data.chunks(4).all(|p| p == [200, 90, 30, 255]), "{deg}");
    }
}

#[test]
fn a_positive_angle_turns_the_picture_clockwise() {
    // Clockwise on screen (`y` down) moves what is right of the centre downwards, so the output's
    // point right of the centre shows what was above and to the right of it in the picture.
    let g = Geometry::new(&recipe(&["straighten(10)"]), upright(), 200, 200);
    let p = g.output_to_stored(Point::new(0.9, 0.5)).unwrap();
    assert!(p.x > 0.5 && p.y < 0.5, "{p:?}");
    let g = Geometry::new(&recipe(&["straighten(-10)"]), upright(), 200, 200);
    let p = g.output_to_stored(Point::new(0.9, 0.5)).unwrap();
    assert!(p.x > 0.5 && p.y > 0.5, "{p:?}");
    // The centre stays the centre, and points map back and forth.
    let g = Geometry::new(&recipe(&["straighten(4)", "crop(0.1,0.2,0.7,0.9)", "flip_h()"]), Orientation::new(6).unwrap(), 50, 80);
    for q in [Point::new(0.5, 0.5), Point::new(0.2, 0.7), Point::new(0.95, 0.05)] {
        let back = g.to_stored(g.to_display(q).unwrap()).unwrap();
        assert!((back.x - q.x).abs() < 1e-12 && (back.y - q.y).abs() < 1e-12, "{q:?} → {back:?}");
    }
    let c = Geometry::new(&recipe(&["straighten(7)"]), upright(), 64, 48).output_to_stored(Point::new(0.5, 0.5)).unwrap();
    assert!((c.x - 0.5).abs() < 1e-12 && (c.y - 0.5).abs() < 1e-12);
}

#[test]
fn straightens_agree_at_two_sizes_and_match_their_fingerprints() {
    let img = scene(21, 300, 200);
    for (case, op) in [("straighten-minus10", "straighten(-10)"), ("straighten-2_5", "straighten(2.5)"), ("straighten-10", "straighten(10)")] {
        // 2.5 rather than the 2.0 of upright renders: at a corner of a turned picture an output
        // pixel's footprint reaches just past the source's edge and takes the edge pixels, a share
        // that differs with the size (2.13 at 2.5°, where this scene has a bright spot there).
        let small = assert_two_sizes_agree(Source::upright(&img), &recipe(&[op]), 64, 240, 2.5);
        check_fingerprint(case, &small, 0.5);
    }
}

#[test]
fn only_the_last_straighten_counts() {
    let img = scene(8, 60, 40);
    let a = render(Source::upright(&img), &recipe(&["straighten(5)", "autofix()", "straighten(-3)"]), Target::Full).unwrap();
    let b = render(Source::upright(&img), &recipe(&["autofix()", "straighten(-3)"]), Target::Full).unwrap();
    assert_eq!(a, b);
}

#[test]
fn the_grid_spacing_is_bounded() {
    for (w, h) in [(10.0, 10.0), (64.0, 64.0), (640.0, 320.0), (1e6, 1e6), (f32::MAX, 3.0)] {
        let g = guides::grid(w, h);
        let v = g.iter().filter(|l| l[0][0] == l[1][0]).count();
        let hz = g.len() - v;
        assert!((1..GRID_MAX).contains(&v) && (1..GRID_MAX).contains(&hz), "{w}x{h}: {v} {hz}");
        assert!(g.iter().flatten().all(|&[u, v]| (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)));
    }
    // About one line per GRID_PX on screen.
    let g = guides::grid(640.0, 320.0);
    assert_eq!(g.iter().filter(|l| l[0][0] == l[1][0]).count(), 19);
    assert_eq!(g.iter().filter(|l| l[0][1] == l[1][1]).count(), 9);
    // Nothing to draw on a degenerate size.
    for (w, h) in [(0.0, 10.0), (0.5, 100.0), (f32::NAN, 10.0), (10.0, f32::INFINITY)] {
        assert!(guides::grid(w, h).is_empty(), "{w}x{h}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Any straighten text renders or is refused without panicking; an accepted one is in range
    /// and prints back; every output corner lies inside the picture.
    #[test]
    fn hostile_straighten_parameters_never_panic(v in prop_oneof![any::<f64>(), -20.0f64..20.0], (w, h) in (1u32..40, 1u32..40)) {
        let text = v.to_string();
        if let Ok(s) = Straighten::parse(&text) {
            prop_assert!((-MAX_DEGREES..=MAX_DEGREES).contains(&s.degrees));
            prop_assert_eq!(Straighten::parse(&s.to_string()).unwrap(), s);
        }
        let r = recipe(&[&format!("straighten({text})")]);
        let g = Geometry::new(&r, upright(), w, h);
        for c in corners(&g) {
            prop_assert!((-1e-9..=1.0 + 1e-9).contains(&c.x) && (-1e-9..=1.0 + 1e-9).contains(&c.y), "{:?}", c);
        }
        let img = scene(1, w.max(4), h.max(4));
        let out = render(Source::upright(&img), &r, Target::Fit { width: 9, height: 9 }).unwrap();
        prop_assert!(out.width >= 1 && out.height >= 1);
    }
}
