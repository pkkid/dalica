//! The coordinate frame of stored rectangles and points (`data-model.md` §3.3): stored values are
//! on the photo after its EXIF orientation and before the recipe's rotations and flips, and
//! `edit::frame` converts them to and from the displayed photo for each of the eight transforms.
//!
//! - Displayed-then-stored (and stored-then-displayed) is the identity for every transform: exactly
//!   on dyadic values, to within the rounding of `1 − v` on any value.
//! - Four quarter turns, two left–right flips and two top–bottom flips bring every point back.
//! - On real pixels: a rectangle on a `scenes` photo, stored with any EXIF orientation and turned
//!   by any short recipe, covers exactly the same pixels in the stored and the displayed frame.

use dalica_edit::{Dihedral, Op, Point, Recipe, Rect};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::{Rgb8, Scene, orient};
use proptest::prelude::*;

/// The largest rounding error one `1 − v` may add for `v` in 0..1 (half a unit in the last place of
/// 1), with room for the two such steps a round trip takes.
const TOLERANCE: f64 = 2.0 * f64::EPSILON;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOLERANCE * a.abs().max(b.abs()).max(1.0)
}

fn close_point(a: Point, b: Point) -> bool {
    close(a.x, b.x) && close(a.y, b.y)
}

fn close_rect(a: Rect, b: Rect) -> bool {
    close(a.x0, b.x0) && close(a.y0, b.y0) && close(a.x1, b.x1) && close(a.y1, b.y1)
}

fn transform() -> impl Strategy<Value = Dihedral> {
    (0..8usize).prop_map(|i| Dihedral::ALL[i])
}

/// Any normalised value, and some outside 0..1 (a crop dragged past the edge before clamping).
fn value() -> impl Strategy<Value = f64> {
    prop_oneof![4 => 0.0..=1.0f64, 1 => -2.0..3.0f64]
}

/// A dyadic value `k / 2²⁰` in 0..1, for which `1 − v` is exact.
fn dyadic() -> impl Strategy<Value = f64> {
    (0u32..=1 << 20).prop_map(|k| f64::from(k) / f64::from(1u32 << 20))
}

fn ordered_rect<S: Strategy<Value = f64>>(v: fn() -> S) -> impl Strategy<Value = Rect> {
    (v(), v(), v(), v()).prop_map(|(a, b, c, d)| Rect::new(a, b, c, d).ordered())
}

proptest! {
    #[test]
    fn displayed_then_stored_is_the_identity_for_points(t in transform(), x in value(), y in value()) {
        let p = Point::new(x, y);
        let back = p.to_display(t).to_stored(t);
        prop_assert!(close_point(back, p), "{}: {p:?} came back as {back:?}", t.name());
        let back = p.to_stored(t).to_display(t);
        prop_assert!(close_point(back, p), "{}: {p:?} came back as {back:?}", t.name());
    }

    #[test]
    fn displayed_then_stored_is_exactly_the_identity_on_dyadic_points(t in transform(), x in dyadic(), y in dyadic()) {
        let p = Point::new(x, y);
        prop_assert_eq!(p.to_display(t).to_stored(t), p);
        prop_assert_eq!(p.to_stored(t).to_display(t), p);
        // Mapping by two transforms one after the other is mapping by their composition.
        for u in Dihedral::ALL {
            prop_assert_eq!(p.to_display(t).to_display(u), p.to_display(t.then(u)));
        }
    }

    #[test]
    fn displayed_then_stored_is_the_identity_for_rectangles(t in transform(), r in ordered_rect(value), d in ordered_rect(dyadic)) {
        let back = r.to_display(t).to_stored(t);
        prop_assert!(close_rect(back, r), "{}: {r:?} came back as {back:?}", t.name());
        let back = r.to_stored(t).to_display(t);
        prop_assert!(close_rect(back, r), "{}: {r:?} came back as {back:?}", t.name());
        prop_assert_eq!(d.to_display(t).to_stored(t), d);
        prop_assert_eq!(d.to_stored(t).to_display(t), d);
        // The displayed rectangle keeps its area and its corners in order.
        let shown = r.to_display(t);
        prop_assert!(shown.x0 <= shown.x1 && shown.y0 <= shown.y1);
        prop_assert!(close((shown.x1 - shown.x0) * (shown.y1 - shown.y0), (r.x1 - r.x0) * (r.y1 - r.y0)));
    }

    #[test]
    fn four_turns_and_two_flips_bring_every_point_back(x in value(), y in value(), dx in dyadic(), dy in dyadic()) {
        for (op, times) in [("rotate(90)", 4), ("rotate(270)", 4), ("rotate(180)", 2), ("flip_h()", 2), ("flip_v()", 2)] {
            let step = Op::parse(op).orientation().unwrap();
            let (mut p, mut d) = (Point::new(x, y), Point::new(dx, dy));
            for _ in 0..times {
                p = p.to_display(step);
                d = d.to_display(step);
            }
            prop_assert!(close_point(p, Point::new(x, y)), "{op} × {times}: {p:?}");
            prop_assert_eq!(d, Point::new(dx, dy), "{} × {}", op, times);
            // The recipe agrees: those operations add up to no turn at all.
            let recipe = Recipe::parse(vec![op; times]).unwrap();
            prop_assert_eq!(recipe.orientation(), Dihedral::IDENTITY);
        }
    }
}

#[test]
fn an_unordered_rectangle_comes_back_ordered() {
    let r = Rect::new(0.75, 0.5, 0.25, 0.125);
    for t in Dihedral::ALL {
        assert_eq!(r.to_display(t).to_stored(t), r.ordered(), "{}", t.name());
    }
}

fn rgba(img: &Rgb8) -> Rgba8 {
    Rgba8 { width: img.width, height: img.height, data: img.data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() }
}

/// The pixels of `img` whose cells the normalised `r` covers, edges rounded to the nearest pixel
/// boundary (the rule the crop tool uses to snap a rectangle to whole pixels).
fn crop(img: &Rgba8, r: Rect) -> Rgba8 {
    let snap = |v: f64, n: u32| (v * f64::from(n)).round().clamp(0.0, f64::from(n)) as u32;
    let (x0, x1) = (snap(r.x0, img.width), snap(r.x1, img.width));
    let (y0, y1) = (snap(r.y0, img.height), snap(r.y1, img.height));
    let mut data = Vec::new();
    for y in y0..y1 {
        let at = ((y * img.width + x0) * 4) as usize;
        data.extend_from_slice(&img.data[at..at + ((x1 - x0) * 4) as usize]);
    }
    Rgba8 { width: x1 - x0, height: y1 - y0, data }
}

/// A rectangle whose edges lie within a third of a pixel of the pixel boundaries `k`, so rounding
/// to whole pixels is never a tie in either frame.
fn rect_near_boundaries(w: u32, h: u32, k: [u32; 4], jitter: [f64; 4]) -> Rect {
    let edge = |k: u32, n: u32, j: f64| (f64::from(k % (n + 1)) + j) / f64::from(n);
    let (a, b) = (edge(k[0], w, jitter[0]), edge(k[1], w, jitter[1]));
    let (c, d) = (edge(k[2], h, jitter[2]), edge(k[3], h, jitter[3]));
    Rect::new(a, c, b, d).ordered()
}

const RECIPES: [&[&str]; 7] =
    [&[], &["rotate(90)"], &["rotate(180)"], &["rotate(270)"], &["flip_h()"], &["rotate(90)", "flip_v()"], &["flip_h()", "rotate(270)", "flip_v()"]];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn a_rectangle_covers_the_same_pixels_in_both_frames(
        size in 0..3usize,
        k in prop::array::uniform4(0u32..200),
        jitter in prop::array::uniform4(-0.3..0.3f64),
    ) {
        let (w, h) = [(29, 48), (37, 20), (16, 16)][size];
        let upright = Scene::new(7, w, h).render().unwrap();
        let r = rect_near_boundaries(w, h, k, jitter);
        // The stored frame is the upright photo, whichever EXIF orientation the file carries.
        let selected = rgba(&upright);
        let in_stored = crop(&selected, r);
        for o in 1..=8u8 {
            let exif = Orientation::new(o).unwrap();
            let file = rgba(&orient::store(&upright, o).unwrap());
            prop_assert_eq!(&Dihedral::from_exif(exif).apply(file.clone()).unwrap(), &selected);
            for ops in RECIPES {
                let recipe = Recipe::parse(ops.iter().copied()).unwrap();
                let shown = recipe.display(exif).apply(file.clone()).unwrap();
                let turn = recipe.orientation();
                let in_display = crop(&shown, r.to_display(turn));
                // What the displayed rectangle covers is the stored rectangle's pixels, turned.
                prop_assert_eq!(&in_display, &turn.apply(in_stored.clone()).unwrap(), "EXIF {} + {:?}, {:?}", o, ops, r);
                // And the displayed rectangle maps back to the stored one.
                prop_assert_eq!(crop(&selected, r.to_display(turn).to_stored(turn)), in_stored.clone());
            }
        }
    }
}
