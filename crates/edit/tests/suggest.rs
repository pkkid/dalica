//! Suggested crops (`editing.md` §6.1): every face inside with their eyes on the upper third line,
//! ahead of the subject; the subject on a thirds point, the horizon on a third line, three distinct
//! rectangles inside the photo in the asked ratio, centred crops for a flat image, the same answer
//! every time (with no faces, or none usable, the same as without faces), and no panic on random
//! images, sizes, ratios and faces.

use crate::common::{cases, scene};
use dalica_edit::suggest::{COUNT, EYES, MAX_OVERLAP, Reason, Suggestion, analyse, suggest};
use dalica_edit::{Error, Rect};
use dalica_image::Rgba8;
use proptest::prelude::*;

const THIRDS: [(f64, f64); 4] = [(1.0 / 3.0, 1.0 / 3.0), (2.0 / 3.0, 1.0 / 3.0), (1.0 / 3.0, 2.0 / 3.0), (2.0 / 3.0, 2.0 / 3.0)];

/// A `w × h` picture painted by `f(u, v)` (normalised pixel centres) as RGB 0–1.
fn paint(w: u32, h: u32, f: impl Fn(f64, f64) -> [f64; 3]) -> Rgba8 {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = f((f64::from(x) + 0.5) / f64::from(w), (f64::from(y) + 0.5) / f64::from(h));
            data.extend(c.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8));
            data.push(255);
        }
    }
    Rgba8 { width: w, height: h, data }
}

/// A soft grey gradient with a red disc of radius `r` (in units of the width) at `(bx, by)`.
fn blob(w: u32, h: u32, bx: f64, by: f64, r: f64) -> Rgba8 {
    let aspect = f64::from(w) / f64::from(h);
    paint(w, h, |u, v| {
        let d = ((u - bx) * aspect).hypot(v - by) / aspect;
        let grey = 0.42 + 0.08 * u;
        if d < r { [0.85, 0.15, 0.1] } else { [grey, grey, grey + 0.02] }
    })
}

/// Sky over ground: a straight horizon at `hy`.
fn horizon(w: u32, h: u32, hy: f64) -> Rgba8 {
    paint(w, h, |u, v| if v < hy { [0.55 + 0.1 * v, 0.7 + 0.05 * v, 0.92] } else { [0.25 + 0.05 * u, 0.32, 0.14] })
}

/// `(x, y)` in the rectangle's own coordinates.
fn within(r: Rect, x: f64, y: f64) -> (f64, f64) {
    ((x - r.x0) / (r.x1 - r.x0), (y - r.y0) / (r.y1 - r.y0))
}

fn nearest_third(r: Rect, x: f64, y: f64) -> f64 {
    let (u, v) = within(r, x, y);
    THIRDS.iter().map(|(tx, ty)| (u - tx).hypot(v - ty)).fold(f64::INFINITY, f64::min)
}

fn iou(a: Rect, b: Rect) -> f64 {
    let area = |r: Rect| (r.x1 - r.x0) * (r.y1 - r.y0);
    let ix = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let iy = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    ix * iy / (area(a) + area(b) - ix * iy)
}

/// Inside the photo, of the ratio on a `w × h` photo (within `tolerance`), pairwise distinct.
fn assert_valid(s: &[Suggestion; COUNT], w: u32, h: u32, aspect: f64, tolerance: f64) {
    for (i, x) in s.iter().enumerate() {
        let r = x.rect;
        assert!([r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)), "{i}: {r:?} outside the photo");
        assert!(r.x1 > r.x0 && r.y1 > r.y0, "{i}: {r:?} is empty");
        let got = (r.x1 - r.x0) * f64::from(w) / ((r.y1 - r.y0) * f64::from(h));
        assert!((got / aspect - 1.0).abs() <= tolerance, "{i}: ratio {got} for {aspect} ({r:?} on {w} × {h})");
        for (j, y) in s.iter().enumerate().skip(i + 1) {
            assert!(iou(r, y.rect) <= MAX_OVERLAP + 1e-9, "{i} and {j} overlap: {r:?} {:?}", y.rect);
        }
    }
}

#[test]
fn an_off_centre_blob_lands_near_a_thirds_point_in_the_first_suggestion() {
    let ratios = [3.0 / 2.0, 1.0, 4.0 / 3.0, 16.0 / 9.0, 2.0 / 3.0];
    for (bx, by) in [(0.2, 0.28), (0.78, 0.7), (0.25, 0.75), (0.7, 0.22)] {
        let img = blob(300, 200, bx, by, 0.05);
        let found = analyse(&img).unwrap().subject.expect("a subject");
        assert!((found.x - bx).abs() < 0.02 && (found.y - by).abs() < 0.03, "found {found:?} for ({bx}, {by})");
        for aspect in ratios {
            let s = suggest(&img, 3000, 2000, aspect, &[]).unwrap();
            assert_valid(&s, 3000, 2000, aspect, 1e-9);
            assert_eq!(s[0].reason, Reason::Subject, "({bx}, {by}) at {aspect}: {s:?}");
            let off = nearest_third(s[0].rect, bx, by);
            assert!(off < 0.03, "({bx}, {by}) at {aspect}: {:?} puts the blob {off:.3} from a thirds point", s[0].rect);
            let (u, v) = within(s[0].rect, bx, by);
            assert!((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v), "the blob is cut off");
        }
    }
}

/// Two faces (skin-coloured ovals) at `faces`, and a red blob at the far right that saliency
/// would take for the subject.
fn two_faces(w: u32, h: u32, faces: [Rect; 2]) -> Rgba8 {
    let aspect = f64::from(w) / f64::from(h);
    paint(w, h, |u, v| {
        let in_face = faces.iter().any(|f| {
            let (cx, cy, rx, ry) = ((f.x0 + f.x1) / 2.0, (f.y0 + f.y1) / 2.0, (f.x1 - f.x0) / 2.0, (f.y1 - f.y0) / 2.0);
            ((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2) <= 1.0
        });
        if in_face {
            [0.87, 0.68, 0.55]
        } else if ((u - 0.9) * aspect).hypot(v - 0.8) < 0.06 {
            [0.85, 0.12, 0.1]
        } else {
            [0.35, 0.4, 0.45]
        }
    })
}

#[test]
fn two_planted_faces_are_held_by_the_first_suggestion_their_eyes_near_the_upper_third() {
    let faces = [Rect::new(0.25, 0.35, 0.35, 0.55), Rect::new(0.55, 0.4, 0.65, 0.6)];
    let img = two_faces(300, 200, faces);
    assert!(analyse(&img).unwrap().subject.is_some(), "the picture has a salient subject the faces go before");
    // The faces' eyes, weighted by their areas (equal here).
    let eyes = faces.iter().map(|f| f.y0 + EYES * (f.y1 - f.y0)).sum::<f64>() / 2.0;
    for aspect in [3.0 / 2.0, 1.0, 4.0 / 3.0, 16.0 / 9.0, 2.0 / 3.0, 4.0 / 5.0] {
        let s = suggest(&img, 3000, 2000, aspect, &faces).unwrap();
        assert_valid(&s, 3000, 2000, aspect, 1e-9);
        assert_eq!(s[0].reason, Reason::Faces, "at {aspect}: {s:?}");
        let r = s[0].rect;
        for f in &faces {
            let inside = f.x0 >= r.x0 - 1e-9 && f.y0 >= r.y0 - 1e-9 && f.x1 <= r.x1 + 1e-9 && f.y1 <= r.y1 + 1e-9;
            assert!(inside, "at {aspect}: {r:?} cuts the face {f:?}");
        }
        let (_, v) = within(r, 0.45, eyes);
        assert!((v - 1.0 / 3.0).abs() < 0.07, "at {aspect}: {r:?} puts the eyes at {v:.3} of its height");
        // The other two are still the picture's own: the faces' rectangle is added, not repeated.
        assert!(s[1..].iter().all(|x| x.reason != Reason::Faces), "{s:?}");
    }
}

#[test]
fn without_usable_faces_the_suggestions_are_those_without_faces() {
    let unusable = [
        Rect::new(f64::NAN, 0.1, 0.2, 0.3),
        Rect::new(0.1, 0.1, 0.1, 0.4),
        Rect::new(1.2, 0.1, 1.5, 0.4),
        Rect::new(-0.5, -0.5, 0.0, 0.0),
        Rect::new(0.2, f64::INFINITY, 0.3, 0.4),
    ];
    for seed in 0..6u64 {
        let img = scene(seed, 240, 160);
        for aspect in [1.0, 3.0 / 2.0, 9.0 / 16.0] {
            let plain = suggest(&img, 6000, 4000, aspect, &[]).unwrap();
            assert!(plain.iter().all(|x| x.reason != Reason::Faces), "{plain:?}");
            assert_eq!(suggest(&img, 6000, 4000, aspect, &unusable).unwrap(), plain, "seed {seed} at {aspect}");
        }
    }
}

#[test]
fn a_horizon_scene_puts_the_horizon_near_a_third_line() {
    for hy in [0.4, 0.45, 0.55, 0.62] {
        let img = horizon(300, 200, hy);
        let found = analyse(&img).unwrap();
        assert_eq!(found.subject, None, "a horizon is not a subject");
        let line = found.horizon.expect("a horizon");
        assert!((line.y - hy).abs() < 0.01 && line.slope.abs() < 1e-9, "found {line:?} for {hy}");
        for aspect in [3.0 / 2.0, 16.0 / 9.0, 1.0, 4.0 / 5.0] {
            let s = suggest(&img, 1200, 800, aspect, &[]).unwrap();
            assert_valid(&s, 1200, 800, aspect, 1e-9);
            assert_eq!(s[0].reason, Reason::Horizon, "{hy} at {aspect}");
            let (_, v) = within(s[0].rect, 0.5, hy);
            let off = (v - 1.0 / 3.0).abs().min((v - 2.0 / 3.0).abs());
            assert!(off < 0.015, "{hy} at {aspect}: {:?} puts the horizon at {v:.3} of its height", s[0].rect);
        }
    }
}

#[test]
fn a_sloping_horizon_is_found_with_its_slope() {
    // 4° down to the right.
    let k = 4f64.to_radians().tan() * 300.0 / 200.0;
    let img = paint(300, 200, |u, v| if v < 0.5 + k * (u - 0.5) { [0.6, 0.75, 0.95] } else { [0.3, 0.35, 0.15] });
    let line = analyse(&img).unwrap().horizon.expect("a horizon");
    assert!((line.y - 0.5).abs() < 0.02 && (line.slope - k).abs() < 0.04, "found {line:?}, slope {k}");
}

#[test]
fn three_distinct_rectangles_inside_the_photo_in_the_asked_ratio() {
    let ratios = [1.0, 3.0 / 2.0, 2.0 / 3.0, 4.0 / 3.0, 16.0 / 9.0, 5.0 / 3.0, 8.5 / 11.0, 1.0 / 7.0, 9.0];
    for seed in 0..12u64 {
        for (w, h) in [(240, 160), (160, 240), (200, 200)] {
            let img = scene(seed, w, h);
            for aspect in ratios {
                let s = suggest(&img, w * 25, h * 25, aspect, &[]).unwrap();
                assert_valid(&s, w * 25, h * 25, aspect, 1e-9);
            }
        }
    }
}

#[test]
fn a_flat_image_gives_centred_crops() {
    for c in [[0.0, 0.0, 0.0], [0.5, 0.5, 0.5], [1.0, 1.0, 1.0], [0.2, 0.6, 0.9]] {
        let img = paint(120, 80, |_, _| c);
        let found = analyse(&img).unwrap();
        assert_eq!((found.subject, found.horizon), (None, None));
        for aspect in [1.0, 3.0 / 2.0, 9.0 / 16.0] {
            let s = suggest(&img, 6000, 4000, aspect, &[]).unwrap();
            assert_valid(&s, 6000, 4000, aspect, 1e-9);
            for x in &s {
                assert_eq!(x.reason, Reason::Centred);
                let r = x.rect;
                assert!(((r.x0 + r.x1) / 2.0 - 0.5).abs() < 1e-9 && ((r.y0 + r.y1) / 2.0 - 0.5).abs() < 1e-9, "{r:?}");
            }
            // The first is the ratio's largest fit: it touches two opposite sides.
            let r = s[0].rect;
            assert!((r.x0.abs() < 1e-9 && (r.x1 - 1.0).abs() < 1e-9) || (r.y0.abs() < 1e-9 && (r.y1 - 1.0).abs() < 1e-9), "{r:?}");
        }
    }
}

#[test]
fn the_same_input_gives_the_same_output() {
    for seed in [1u64, 7, 42] {
        let img = scene(seed, 256, 171);
        let t = std::time::Instant::now();
        let a = suggest(&img, 6000, 4000, 4.0 / 3.0, &[]).unwrap();
        println!("suggest on a 256 × 171 proxy: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
        let b = suggest(&img.clone(), 6000, 4000, 4.0 / 3.0, &[]).unwrap();
        assert_eq!(a, b);
    }
}

#[test]
fn a_small_proxy_of_a_large_photo_suggests_what_a_large_proxy_does() {
    // The proxy size only changes the analysis grid's input, not where things are.
    let small = blob(256, 171, 0.22, 0.3, 0.06);
    let large = blob(768, 512, 0.22, 0.3, 0.06);
    let (a, b) = (suggest(&small, 6000, 4000, 1.0, &[]).unwrap(), suggest(&large, 6000, 4000, 1.0, &[]).unwrap());
    assert_eq!(a[0].reason, b[0].reason);
    assert!(iou(a[0].rect, b[0].rect) > 0.9, "{:?} vs {:?}", a[0].rect, b[0].rect);
}

#[test]
fn bad_input_is_refused() {
    let img = paint(10, 10, |_, _| [0.5, 0.5, 0.5]);
    for aspect in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(suggest(&img, 100, 100, aspect, &[]), Err(Error::BadTarget(_))), "{aspect}");
    }
    assert!(matches!(suggest(&img, 0, 100, 1.0, &[]), Err(Error::BadTarget(_))));
    assert!(matches!(suggest(&img, 100, 0, 1.0, &[]), Err(Error::BadTarget(_))));
    let short = Rgba8 { width: 10, height: 10, data: vec![0; 399] };
    assert!(matches!(suggest(&short, 100, 100, 1.0, &[]), Err(Error::BadImage { .. })));
    let empty = Rgba8 { width: 0, height: 0, data: Vec::new() };
    assert!(matches!(suggest(&empty, 100, 100, 1.0, &[]), Err(Error::BadImage { .. })));
    let huge = Rgba8 { width: u32::MAX, height: u32::MAX, data: vec![0; 16] };
    assert!(matches!(suggest(&huge, 100, 100, 1.0, &[]), Err(Error::BadImage { .. })));
}

#[test]
fn tiny_and_thin_proxies_still_answer() {
    for (w, h) in [(1, 1), (1, 50), (50, 1), (2, 3), (7, 7)] {
        let img = paint(w, h, |u, v| [u, v, 0.5]);
        for aspect in [1.0, 3.0, 1.0 / 3.0] {
            let s = suggest(&img, w, h, aspect, &[]).unwrap();
            assert_valid(&s, w, h, aspect, 1e-9);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn random_images_ratios_and_faces_never_panic(
        w in 1u32..48,
        h in 1u32..48,
        seed in any::<u64>(),
        pw in 1u32..100_000,
        ph in 1u32..100_000,
        aspect in prop_oneof![0.001f64..1000.0, Just(f64::MIN_POSITIVE), Just(f64::MAX), Just(1e-300)],
        faces in proptest::collection::vec(
            (prop_oneof![-2.0f64..3.0, Just(f64::NAN), Just(f64::INFINITY)], -2.0f64..3.0, -2.0f64..3.0, -2.0f64..3.0),
            0..4,
        ),
    ) {
        let faces: Vec<Rect> = faces.into_iter().map(|(a, b, c, d)| Rect::new(a, b, c, d)).collect();
        let mut state = seed | 1;
        let data = (0..w * h * 4).map(|_| { state ^= state << 13; state ^= state >> 7; state ^= state << 17; state as u8 }).collect();
        let img = Rgba8 { width: w, height: h, data };
        let s = suggest(&img, pw, ph, aspect, &faces).unwrap();
        for x in &s {
            let r = x.rect;
            prop_assert!([r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)), "{:?}", r);
            prop_assert!(r.x0 <= r.x1 && r.y0 <= r.y1, "{:?}", r);
        }
        prop_assert_eq!(s, suggest(&img, pw, ph, aspect, &faces).unwrap());
    }
}
