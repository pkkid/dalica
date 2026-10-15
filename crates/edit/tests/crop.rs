//! Crop (`editing.md` §6.1, `data-model.md` §3.1, §3.3): `crop(x0,y0,x1,y1)` parsed, clamped or
//! refused, rendered at two sizes alike and against its fingerprints at three values, the same
//! with a straighten in either recipe order, at least one pixel at any size; the tools' maths in
//! `crop_geom` (a ratio's rectangle, Rotate, dragging with the ratio locked or free, inside the
//! picture; LightCraft's tests ported) and the thirds of `guides`. Fingerprints:
//! `fingerprints/crop-*.json`.
//!
//! The `crop_geom` tests below are ported from LightCraft `crates/geom/src/crop.rs` and
//! `crates/geom/src/crop_drag.rs` at revision 76b53778 (MIT OR Apache-2.0; `NOTICE`), the thirds
//! test from LightCraft `crates/ui-egui/src/panels/crop_overlay.rs` at the same revision.

use crate::common::{self, assert_two_sizes_agree, cases, check_fingerprint, recipe, scene, window};
use dalica_edit::crop_geom::{self, CropBox, Handle, MIN_CROP_FRACTION, crop_fit_angle, drag, max_inscribed_scale, ratio_rect};
use dalica_edit::ops::Operation;
use dalica_edit::ops::crop::Crop;
use dalica_edit::{Geometry, Op, Point, Rect, Source, Target, Typed, guides, render};
use dalica_image::Orientation;
use proptest::prelude::*;

fn typed_crop(text: &str) -> Option<Crop> {
    match Op::parse(text).typed() {
        Some(Ok(Typed::Crop(c))) => Some(c),
        _ => None,
    }
}

// ---- The operation ----------------------------------------------------------------------------

#[test]
fn crop_parameters_print_back_and_hostile_ones_are_clamped_or_refused() {
    for text in ["0,0,1,1", "0.08,0.1,0.92,0.86", "0.25,0.5,0.75,1", "0.1,0.2,0.30000000000000004,0.9"] {
        let c = Crop::parse(text).unwrap();
        assert_eq!(c.to_string(), text);
        assert_eq!(Op::from(Typed::Crop(c)).to_string(), format!("crop({text})"));
    }
    // The stored text is kept verbatim; the typed value is what renders.
    assert_eq!(Op::parse("crop(0.10,0.20,0.90,0.80)").to_string(), "crop(0.10,0.20,0.90,0.80)");
    assert_eq!(typed_crop("crop(0.10,0.20,0.90,0.80)").unwrap().rect, Rect::new(0.1, 0.2, 0.9, 0.8));
    // Outside the picture: clamped into it.
    assert_eq!(Crop::parse("-0.5,0.2,1.5,0.8").unwrap().rect, Rect::new(0.0, 0.2, 1.0, 0.8));
    assert_eq!(Crop::parse("-0,-0,1,1").unwrap().to_string(), "0,0,1,1");
    // Reversed, empty, not finite, the wrong count, not numbers: refused, kept and skipped.
    for bad in [
        "0.9,0.9,0.1,0.1",
        "0.2,0.1,0.8,0.1",
        "0.5,0.5,0.5,0.5",
        "1.2,0,1.5,1",
        "NaN,0,1,1",
        "0,0,inf,1",
        "1e308,-1e308,0,0",
        "0,0,1",
        "0,0,1,1,1",
        "",
        " 0,0,1,1",
        "0,0,1,1 ",
        "a,b,c,d",
        "0x1,0,1,1",
    ] {
        assert!(Crop::parse(bad).is_err(), "{bad}");
        assert!(matches!(Op::parse(&format!("crop({bad})")).typed(), Some(Err(_)) | None), "{bad}");
    }
    assert_eq!(Crop::new(Rect::new(f64::NAN, 0.0, 1.0, 1.0)), None);
    assert_eq!(Crop::default().rect, Rect::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(Crop::VERSION, 1);
}

#[test]
fn a_refused_crop_is_skipped_and_kept() {
    let img = scene(4, 40, 30);
    let plain = render(Source::upright(&img), &recipe(&[]), Target::Full).unwrap();
    for bad in ["crop(0.9,0.9,0.1,0.1)", "crop(NaN,NaN,NaN,NaN)", "crop(1e308,-1e308,0,0)", "crop()"] {
        let r = recipe(&[bad]);
        assert_eq!(render(Source::upright(&img), &r, Target::Full).unwrap(), plain, "{bad}");
        assert_eq!(r.print(), vec![bad.to_owned()]);
    }
}

#[test]
fn a_crop_on_whole_pixels_at_full_size_copies_them() {
    let img = scene(9, 40, 30);
    let r = recipe(&["crop(0.25,0.1,0.75,0.9)"]);
    let out = render(Source::upright(&img), &r, Target::Full).unwrap();
    assert_eq!(out, window(&img, 10, 3, 20, 24));
    // Only the last crop counts.
    let twice = recipe(&["crop(0,0,0.5,0.5)", "autofix()", "crop(0.25,0.1,0.75,0.9)"]);
    let once = recipe(&["autofix()", "crop(0.25,0.1,0.75,0.9)"]);
    assert_eq!(render(Source::upright(&img), &twice, Target::Full).unwrap(), render(Source::upright(&img), &once, Target::Full).unwrap());
}

#[test]
fn a_crop_is_in_the_stored_frame_whatever_the_rotations_and_the_exif_orientation() {
    // EXIF 6 makes the stored frame 30 × 40; the crop is of that, then the recipe turns it.
    let up = scene(12, 30, 40);
    let rgb = dalica_scenes::Rgb8 { width: up.width, height: up.height, data: up.data.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect() };
    let file = common::rgba(&dalica_scenes::orient::store(&rgb, 6).unwrap());
    let source = Source::new(&file, Orientation::new(6).unwrap());
    let cropped_upright = window(&up, 6, 8, 12, 20);
    let expected = recipe(&["rotate(90)"]).display(Orientation::default()).apply(cropped_upright).unwrap();
    for ops in [["crop(0.2,0.2,0.6,0.7)", "rotate(90)"], ["rotate(90)", "crop(0.2,0.2,0.6,0.7)"]] {
        assert_eq!(render(source, &recipe(&ops), Target::Full).unwrap(), expected, "{ops:?}");
    }
    // The geometry maps the crop's corners to the output's.
    let g = Geometry::new(&recipe(&["crop(0.2,0.2,0.6,0.7)"]), Orientation::new(6).unwrap(), file.width, file.height);
    let tl = g.stored_to_output(Point::new(0.2, 0.2)).unwrap();
    let br = g.stored_to_output(Point::new(0.6, 0.7)).unwrap();
    assert!(tl.x.abs() < 1e-12 && tl.y.abs() < 1e-12 && (br.x - 1.0).abs() < 1e-12 && (br.y - 1.0).abs() < 1e-12, "{tl:?} {br:?}");
    assert_eq!(g.size(), (12.0, 20.0));
}

#[test]
fn a_crop_is_at_least_one_pixel_at_any_size() {
    let img = scene(2, 40, 30);
    for text in ["crop(0.5,0.5,0.5000001,0.5000001)", "crop(0,0,0.0001,1)", "crop(0.99999,0.99999,1,1)", "crop(0,0.5,1,0.5000000001)"] {
        let r = recipe(&[text]);
        for target in
            [Target::Full, Target::Fit { width: 1, height: 1 }, Target::Fit { width: 300, height: 300 }, Target::Scale(0.01), Target::Scale(7.0)]
        {
            let out = render(Source::upright(&img), &r, target).unwrap();
            assert!(out.width >= 1 && out.height >= 1, "{text} {target:?}");
            assert_eq!(out.data.len(), (out.width * out.height * 4) as usize);
        }
        let c = typed_crop(text).unwrap();
        let (x0, y0, x1, y1) = c.pixels(40.0, 30.0);
        assert!(x1 - x0 >= 1.0 - 1e-9 && y1 - y0 >= 1.0 - 1e-9 && x0 >= 0.0 && y0 >= 0.0 && x1 <= 40.0 && y1 <= 30.0, "{text}");
    }
    // A picture smaller than a pixel along a side keeps all of it.
    assert_eq!(Crop::default().pixels(0.5, 3.0), (0.0, 0.0, 0.5, 3.0));
}

#[test]
fn a_crop_and_a_straighten_render_the_same_in_either_order() {
    let img = scene(14, 90, 60);
    let source = Source::new(&img, Orientation::new(3).unwrap());
    for (crop, straighten) in [("crop(0.1,0.2,0.8,0.9)", "straighten(4)"), ("crop(0,0,0.5,0.5)", "straighten(-10)")] {
        let a = recipe(&[crop, straighten]);
        let b = recipe(&[straighten, "autofix()", crop]);
        let c = recipe(&["autofix()", straighten, crop]);
        for target in [Target::Full, Target::Fit { width: 40, height: 40 }, Target::Scale(1.7)] {
            let expected = render(source, &recipe(&["autofix()", crop, straighten]), target).unwrap();
            for r in [&b, &c] {
                assert_eq!(render(source, r, target).unwrap(), expected, "{:?} {target:?}", r.print());
            }
            // Without the adjust operation, still order-free.
            assert_eq!(render(source, &a, target).unwrap(), render(source, &recipe(&[straighten, crop]), target).unwrap());
        }
    }
}

#[test]
fn crops_agree_at_two_sizes_and_match_their_fingerprints() {
    let img = scene(21, 300, 200);
    for (case, op) in [("crop-centre", "crop(0.25,0.25,0.75,0.75)"), ("crop-corner", "crop(0,0,0.6,0.45)"), ("crop-strip", "crop(0.05,0.4,0.95,0.6)")]
    {
        let small = assert_two_sizes_agree(Source::upright(&img), &recipe(&[op]), 64, 240, 2.0);
        check_fingerprint(case, &small, 0.5);
    }
}

// ---- crop_geom: the tools' maths (ported from LightCraft, then Dalica's own) -----------------

const W: f64 = 600.0;
const H: f64 = 400.0;
const EPS: f64 = 1e-6;

fn geo(x0: f64, y0: f64, x1: f64, y1: f64) -> CropBox {
    CropBox::new(Rect::new(x0, y0, x1, y1), 0.0)
}

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn w_of(r: Rect) -> f64 {
    r.x1 - r.x0
}

fn h_of(r: Rect) -> f64 {
    r.y1 - r.y0
}

fn centre(r: Rect) -> Point {
    Point::new((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)
}

fn from_centre(c: Point, w: f64, h: f64) -> Rect {
    Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0)
}

fn drag_on(g: CropBox, hd: Handle, from: Point, to: Point, aspect: Option<f64>) -> CropBox {
    drag(g, hd, from, to, W, H, aspect)
}

fn px_aspect(g: &CropBox) -> f64 {
    w_of(g.rect) * W / (h_of(g.rect) * H)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn a_turned_fit_is_inside_and_maximal() {
    for ang in [-45.0, -10.0, 0.0, 3.5, 30.0, 45.0] {
        let c = crop_fit_angle(W, H, ang, None);
        assert!(c.is_within_image(W, H), "{ang}");
        // 1 % bigger does not fit.
        let big = CropBox::new(from_centre(centre(c.rect), w_of(c.rect) * 1.01, h_of(c.rect) * 1.01), ang);
        assert!(!big.is_within_image(W, H), "{ang}");
    }
}

#[test]
fn constrained_pulls_a_crop_inside() {
    let c = CropBox::new(Rect::new(0.5, 0.5, 1.4, 1.2), 12.0);
    assert!(c.constrained(300.0, 200.0).is_within_image(300.0, 200.0));
}

#[test]
fn with_angle_keeps_a_crop_that_still_fits() {
    let c = geo(0.3, 0.3, 0.7, 0.7);
    for ang in [-3.0, 0.5, 2.0, 5.0] {
        let t = c.with_angle(ang, W, H);
        assert_eq!((t.rect, t.angle), (c.rect, ang), "{ang}°");
    }
}

#[test]
fn with_angle_shrinks_a_crop_that_no_longer_fits_keeping_its_ratio() {
    for (rect, ang) in [(Rect::new(0.0, 0.0, 1.0, 1.0), 15.0), (Rect::new(0.0, 0.1, 0.8, 0.9), -20.0), (Rect::new(0.2, 0.0, 0.8, 1.0), 45.0)] {
        let c = CropBox::new(rect, 0.0);
        let t = c.with_angle(ang, W, H);
        assert!(t.is_within_image(W, H), "{rect:?} at {ang}°");
        assert!((px_aspect(&c) - px_aspect(&t)).abs() < 1e-9, "ratio kept: {} vs {}", px_aspect(&c), px_aspect(&t));
        assert_eq!(t.angle, ang);
    }
}

#[test]
fn with_angle_survives_a_degenerate_crop() {
    let t = geo(0.0, 0.5, 1.0, 0.5).with_angle(30.0, W, H);
    assert!(t.is_within_image(W, H));
    assert!([t.rect.x0, t.rect.y0, t.rect.x1, t.rect.y1].iter().all(|v| v.is_finite()));
}

#[test]
fn the_fill_scale_is_the_inverse_of_the_inscribed_scale() {
    assert_eq!(crop_geom::fill_scale(W, H, 0.0), 1.0);
    for ang in [-10.0, -2.5, 1.0, 10.0] {
        let k = crop_geom::fill_scale(W, H, ang);
        assert!((k * max_inscribed_scale(W, H, ang, W, H) - 1.0).abs() < 1e-12 && k > 1.0, "{ang}");
        assert_eq!(k, crop_geom::fill_scale(W, H, -ang), "symmetric");
    }
    for (w, h) in [(0.0, 10.0), (f64::NAN, 1.0), (10.0, f64::INFINITY)] {
        assert_eq!(crop_geom::fill_scale(w, h, 5.0), 1.0);
    }
}

// Feature: dragging a crop handle.
#[test]
fn a_free_corner_dragged_inside_follows_and_the_opposite_corner_stays() {
    let g = drag_on(geo(0.2, 0.2, 0.8, 0.8), Handle::BottomRight, p(0.8, 0.8), p(0.9, 0.7), None);
    assert!(near(g.rect.x0, 0.2) && near(g.rect.y0, 0.2));
    assert!(near(g.rect.x1, 0.9) && near(g.rect.y1, 0.7));
}

#[test]
fn a_corner_dragged_past_the_edge_stops_there_and_the_anchor_stays() {
    let g = drag_on(geo(0.2, 0.2, 0.8, 0.8), Handle::BottomRight, p(0.8, 0.8), p(1.3, 0.7), None);
    assert!(near(g.rect.x0, 0.2) && near(g.rect.y0, 0.2), "anchor moved: {:?}", g.rect);
    assert!(near(g.rect.x1, 1.0), "stops at the edge: {:?}", g.rect);
    assert!(near(g.rect.y1, 0.7), "the other axis still follows the pointer: {:?}", g.rect);
}

#[test]
fn a_locked_corner_dragged_past_the_edge_keeps_its_ratio_and_anchor() {
    let a = 1.5;
    let g = drag_on(geo(0.2, 0.2, 0.8, 0.2 + 0.6 * W / H / a), Handle::BottomRight, p(0.8, 0.6), p(1.4, 0.9), Some(a));
    assert!(near(g.rect.x0, 0.2) && near(g.rect.y0, 0.2), "{:?}", g.rect);
    assert!(near(px_aspect(&g), a), "{}", px_aspect(&g));
    assert!(g.rect.x1 <= 1.0 + EPS && g.rect.y1 <= 1.0 + EPS);
    assert!(g.rect.x1 > 0.95 || g.rect.y1 > 0.95, "grows until it touches an edge: {:?}", g.rect);
}

#[test]
fn a_locked_edge_drag_keeps_the_ratio_and_grows_about_its_midline() {
    let a = 2.0;
    let start = geo(0.3, 0.3, 0.7, 0.3 + 0.4 * W / H / a);
    let g = drag_on(start, Handle::Bottom, p(0.5, start.rect.y1), p(0.5, start.rect.y1 + 0.1), Some(a));
    assert!(near(g.rect.y0, start.rect.y0), "the opposite edge stays");
    assert!(near(centre(g.rect).x, centre(start.rect).x), "grows symmetrically sideways");
    assert!(near(px_aspect(&g), a));
    assert!(h_of(g.rect) > h_of(start.rect));
}

#[test]
fn a_free_edge_drag_moves_only_that_edge() {
    let g = drag_on(geo(0.2, 0.2, 0.8, 0.8), Handle::Left, p(0.2, 0.5), p(0.1, 0.9), None);
    assert!(near(g.rect.x0, 0.1) && near(g.rect.y0, 0.2) && near(g.rect.x1, 0.8) && near(g.rect.y1, 0.8));
}

#[test]
fn a_move_past_the_edge_keeps_the_size_and_slides_along_it() {
    let start = geo(0.5, 0.25, 0.9, 0.65);
    let g = drag_on(start, Handle::Move, p(0.7, 0.45), p(1.3, 0.55), None);
    assert!(near(w_of(g.rect), 0.4) && near(h_of(g.rect), 0.4));
    assert!(near(g.rect.x1, 1.0), "flush with the right edge: {:?}", g.rect);
    assert!(near(g.rect.y0, 0.35), "still follows the pointer vertically: {:?}", g.rect);
}

#[test]
fn an_edge_dragged_across_the_opposite_one_keeps_a_minimum_size() {
    let g = drag_on(geo(0.2, 0.2, 0.8, 0.8), Handle::Right, p(0.8, 0.5), p(-0.5, 0.5), None);
    assert!(w_of(g.rect) * W >= MIN_CROP_FRACTION * H - EPS && g.rect.x1 > g.rect.x0);
    assert!(near(g.rect.x0, 0.2));
}

#[test]
fn on_a_turned_picture_a_drag_outside_stays_inside_it() {
    let start = crop_fit_angle(W, H, 12.0, Some(1.5));
    let c = centre(start.rect);
    for hd in (0..9).filter_map(Handle::from_index) {
        for aspect in [None, Some(1.5)] {
            let g = drag_on(start, hd, c, p(3.0, -3.0), aspect);
            assert!(g.is_within_image(W, H), "{hd:?} {aspect:?}: {:?}", g.rect);
            assert_eq!(g.angle, 12.0);
        }
    }
}

#[test]
fn hostile_numbers_give_the_start_crop_back() {
    let s = geo(0.2, 0.2, 0.8, 0.8);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(drag_on(s, Handle::BottomRight, p(0.8, 0.8), p(bad, 0.5), None), s);
    }
    assert_eq!(drag(s, Handle::BottomRight, p(0.8, 0.8), p(0.9, 0.9), 0.0, 400.0, None), s);
    assert_eq!(drag_on(s, Handle::BottomRight, p(0.8, 0.8), p(0.9, 0.9), Some(f64::NAN)), s);
    assert_eq!(drag_on(s, Handle::BottomRight, p(0.8, 0.8), p(0.9, 0.9), Some(-1.0)), s);
    let reversed = geo(0.8, 0.8, 0.2, 0.2);
    assert_eq!(drag_on(reversed, Handle::Move, p(0.5, 0.5), p(0.6, 0.6), None), reversed);
}

#[test]
fn a_locked_frame_flush_with_the_top_grows_sliding_down() {
    let a = 1.5;
    let start = geo(0.2, 0.0, 0.5, 0.3 * W / (a * H));
    let g = drag_on(start, Handle::Right, p(0.5, 0.2), p(0.9, 0.2), Some(a));
    assert!(near(g.rect.x0, 0.2), "the anchor edge stays: {:?}", g.rect);
    assert!(w_of(g.rect) > w_of(start.rect) + 0.05, "it must grow: {:?}", g.rect);
    assert!(near(px_aspect(&g), a));
    assert!(g.is_within_image(W, H));
}

#[test]
fn a_full_turned_fit_still_respects_the_picture_on_a_locked_edge_drag() {
    let start = crop_fit_angle(W, H, 20.0, Some(1.5));
    let g = drag_on(start, Handle::Right, p(0.9, 0.5), p(1.5, 0.5), Some(1.5));
    assert!(g.is_within_image(W, H) && near(px_aspect(&g), 1.5));
}

#[test]
fn an_extreme_locked_ratio_can_still_be_moved() {
    let start = geo(0.1, 0.4, 0.9, 0.4 + 0.8 * W / (60.0 * H));
    let g = drag_on(start, Handle::Move, p(0.5, 0.4), p(0.5, 0.7), Some(60.0));
    assert!(g.rect.y0 > 0.5, "moved: {:?}", g.rect);
}

#[test]
fn a_huge_finite_pointer_still_grows_a_corner_to_the_edge() {
    let a = 1.5;
    let start = geo(0.2, 0.2, 0.5, 0.2 + 0.3 * W / (a * H));
    let g = drag_on(start, Handle::BottomRight, p(0.5, 0.4), p(1e300, 1e300), Some(a));
    assert!(g.rect.x1 > 0.95 || g.rect.y1 > 0.95, "{:?}", g.rect);
    let g = drag_on(start, Handle::BottomRight, p(0.5, 0.4), p(1e300, 1e300), None);
    assert!(near(g.rect.x1, 1.0) && near(g.rect.y1, 1.0), "{:?}", g.rect);
}

#[test]
fn on_an_unturned_picture_results_never_leave_the_unit_square() {
    let g = drag_on(geo(0.2, 0.2, 0.8, 0.8), Handle::BottomRight, p(0.8, 0.8), p(5.0, 5.0), None);
    assert!(g.rect.x1 <= 1.0 && g.rect.y1 <= 1.0 && g.rect.x0 >= 0.0 && g.rect.y0 >= 0.0, "{:?}", g.rect);
}

#[test]
fn handle_numbering_and_names_match_the_overlay() {
    assert_eq!(Handle::from_index(0), Some(Handle::TopLeft));
    assert_eq!(Handle::from_index(6), Some(Handle::Bottom));
    assert_eq!(Handle::from_index(8), Some(Handle::Move));
    assert_eq!(Handle::from_index(9), None);
    assert_eq!(Handle::from_name("bottomLeft"), Some(Handle::BottomLeft));
    assert_eq!(Handle::from_name("nope"), None);
    for hd in (0..9).filter_map(Handle::from_index) {
        assert_eq!(Handle::from_name(hd.name()), Some(hd));
    }
}

// Dalica's own: the size dropdown's rectangle and the Rotate button.

#[test]
fn a_ratio_gives_its_largest_centred_rectangle() {
    for (w, h, a) in [
        (600.0, 400.0, 1.0),
        (600.0, 400.0, 1.5),
        (600.0, 400.0, 16.0 / 9.0),
        (600.0, 400.0, 4.0 / 6.0),
        (400.0, 600.0, 5.0 / 7.0),
        (400.0, 600.0, 3.0),
    ] {
        let r = ratio_rect(w, h, a);
        let (pw, ph) = (w_of(r) * w, h_of(r) * h);
        assert!((pw / ph - a).abs() < 1e-9, "{w}×{h} at {a}: {r:?}");
        assert!(near(pw, w) || near(ph, h), "largest: touches two sides: {r:?}");
        assert!(near(centre(r).x, 0.5) && near(centre(r).y, 0.5));
        assert!(r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= 1.0 && r.y1 <= 1.0);
    }
    // The picture's own ratio is the whole picture; unusable input gives the whole picture.
    assert_eq!(ratio_rect(600.0, 400.0, 1.5), Rect::new(0.0, 0.0, 1.0, 1.0));
    for (w, h, a) in [(600.0, 400.0, 0.0), (600.0, 400.0, f64::NAN), (0.0, 400.0, 1.0), (f64::INFINITY, 1.0, 1.0)] {
        assert_eq!(ratio_rect(w, h, a), Rect::new(0.0, 0.0, 1.0, 1.0));
    }
}

#[test]
fn rotate_swaps_portrait_and_landscape_about_the_centre_and_stays_inside() {
    // A small landscape crop: turned in place, the same centre and pixel size swapped.
    let start = CropBox::new(ratio_rect(W, H, 1.5), 0.0);
    let small = CropBox::new(from_centre(Point::new(0.4, 0.5), w_of(start.rect) * 0.4, h_of(start.rect) * 0.4), 0.0);
    let t = crop_geom::rotate(small, W, H);
    assert!(near(px_aspect(&t), 1.0 / 1.5), "{}", px_aspect(&t));
    assert!(near(centre(t.rect).x, 0.4) && near(centre(t.rect).y, 0.5));
    assert!(near(w_of(t.rect) * W, h_of(small.rect) * H));
    // Twice is the start again.
    let back = crop_geom::rotate(t, W, H);
    assert!(near(back.rect.x0, small.rect.x0) && near(back.rect.y1, small.rect.y1), "{:?}", back.rect);
    // The largest 3:2 on a 3:2 picture turned to 2:3: shrunk to fit, centred, still inside.
    let t = crop_geom::rotate(start, W, H);
    assert!(near(px_aspect(&t), 1.0 / 1.5) && near(h_of(t.rect), 1.0) && t.rect.y0 >= 0.0 && t.rect.y1 <= 1.0);
    assert!(near(centre(t.rect).x, 0.5));
    // Near an edge: moved in rather than cut.
    let edge = CropBox::new(Rect::new(0.8, 0.0, 1.0, 0.2), 0.0);
    let t = crop_geom::rotate(edge, W, H);
    assert!(t.rect.x0 >= 0.0 && t.rect.x1 <= 1.0 && t.rect.y0 >= 0.0 && t.rect.y1 <= 1.0, "{:?}", t.rect);
    assert!(near(px_aspect(&t), 1.0 / px_aspect(&edge)));
    // Hostile input: unchanged.
    let bad = CropBox::new(Rect::new(f64::NAN, 0.0, 1.0, 1.0), 0.0);
    assert!(crop_geom::rotate(bad, W, H).rect.x0.is_nan());
    assert_eq!(crop_geom::rotate(small, 0.0, H), small);
}

// ---- guides: the thirds ------------------------------------------------------------------------

#[test]
fn the_thirds_are_two_lines_each_way_inside_the_crop() {
    let t = guides::thirds();
    assert_eq!(t.len(), 4);
    assert!(t.iter().flatten().all(|&[u, v]| (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)));
    let vertical: Vec<f32> = t.iter().filter(|l| l[0][0] == l[1][0]).map(|l| l[0][0]).collect();
    let horizontal: Vec<f32> = t.iter().filter(|l| l[0][1] == l[1][1]).map(|l| l[0][1]).collect();
    assert_eq!(vertical, vec![1.0 / 3.0, 2.0 / 3.0]);
    assert_eq!(horizontal, vec![1.0 / 3.0, 2.0 / 3.0]);
}

// ---- Properties --------------------------------------------------------------------------------

fn any_handle() -> impl Strategy<Value = Handle> {
    (0u8..9).prop_map(|i| Handle::from_index(i).unwrap_or(Handle::Move))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    /// Whatever the pointer does, the crop stays inside the picture and keeps a minimum size; the
    /// anchor and the ratio hold (ported).
    #[test]
    fn every_drag_stays_inside_anchored_and_shaped(
        angle in -45.0f64..45.0,
        ratio in prop::option::of(0.3f64..4.0),
        hd in any_handle(),
        tx in -4.0f64..5.0, ty in -4.0f64..5.0,
        shrink in 0.2f64..1.0,
    ) {
        let base = crop_fit_angle(W, H, angle, ratio);
        let c = centre(base.rect);
        let start = CropBox::new(from_centre(c, w_of(base.rect) * shrink, h_of(base.rect) * shrink), angle);
        prop_assume!(start.is_within_image(W, H));
        let g = drag(start, hd, c, p(tx, ty), W, H, ratio);
        prop_assert!(g.is_within_image(W, H), "{:?} -> {:?}", start.rect, g.rect);
        prop_assert!(w_of(g.rect) > 0.0 && h_of(g.rect) > 0.0);
        let min = MIN_CROP_FRACTION * W.min(H) - 1e-6;
        prop_assert!(w_of(g.rect) * W >= min.min(w_of(start.rect) * W) && h_of(g.rect) * H >= min.min(h_of(start.rect) * H), "min size {:?}", g.rect);
        if let Some(a) = ratio {
            prop_assert!((px_aspect(&g) - a).abs() < 1e-6 * a.max(1.0), "ratio {}: {}", a, px_aspect(&g));
        }
        let (s, r) = (start.rect, g.rect);
        use Handle::*;
        if matches!(hd, TopRight | BottomRight | Right) { prop_assert!((r.x0 - s.x0).abs() < 1e-6, "x0"); }
        if matches!(hd, TopLeft | BottomLeft | Left) { prop_assert!((r.x1 - s.x1).abs() < 1e-6, "x1"); }
        if matches!(hd, BottomLeft | BottomRight | Bottom) { prop_assert!((r.y0 - s.y0).abs() < 1e-6, "y0"); }
        if matches!(hd, TopLeft | TopRight | Top) { prop_assert!((r.y1 - s.y1).abs() < 1e-6, "y1"); }
        if hd == Move {
            prop_assert!((w_of(r) - w_of(s)).abs() < 1e-9 && (h_of(r) - h_of(s)).abs() < 1e-9);
        }
    }

    /// Dalica's stored crop (angle 0, any picture shape and ratio): every drag keeps the ratio and
    /// stays in the unit square, and the result is a crop the operation accepts.
    #[test]
    fn every_stored_crop_drag_keeps_its_ratio_and_stays_in_the_picture(
        (w, h) in (16.0f64..6000.0, 16.0f64..6000.0),
        ratio in prop::option::of(0.25f64..4.0),
        hd in any_handle(),
        (gx, gy) in (0.0f64..1.0, 0.0f64..1.0),
        (tx, ty) in (-3.0f64..4.0, -3.0f64..4.0),
        shrink in 0.1f64..1.0,
        (ox, oy) in (-0.5f64..0.5, -0.5f64..0.5),
    ) {
        let base = match ratio { Some(a) => ratio_rect(w, h, a), None => Rect::new(0.0, 0.0, 1.0, 1.0) };
        let (bw, bh) = (w_of(base) * shrink, h_of(base) * shrink);
        let c = Point::new(0.5 + ox * (1.0 - bw), 0.5 + oy * (1.0 - bh));
        let start = CropBox::new(from_centre(c, bw, bh), 0.0);
        let g = drag(start, hd, p(gx, gy), p(tx, ty), w, h, ratio);
        let r = g.rect;
        prop_assert!(r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= 1.0 && r.y1 <= 1.0 && r.x0 < r.x1 && r.y0 < r.y1, "{:?}", r);
        if let Some(a) = ratio {
            let got = w_of(r) * w / (h_of(r) * h);
            prop_assert!((got - a).abs() < 1e-6 * a.max(1.0), "ratio {}: {}", a, got);
        }
        prop_assert!(Crop::new(r).is_some());
        let rt = crop_geom::rotate(g, w, h);
        prop_assert!(rt.rect.x0 >= 0.0 && rt.rect.y0 >= 0.0 && rt.rect.x1 <= 1.0 && rt.rect.y1 <= 1.0, "{:?}", rt.rect);
    }

    #[test]
    fn a_drag_never_panics_on_arbitrary_numbers(
        x0 in -2.0f64..3.0, y0 in -2.0f64..3.0, x1 in -2.0f64..3.0, y1 in -2.0f64..3.0,
        angle in -90.0f64..90.0, tx in -1e9f64..1e9, ty in -1e9f64..1e9,
        ratio in prop::option::of(-1.0f64..10.0), hd in any_handle(),
    ) {
        let g = drag(CropBox::new(Rect::new(x0, y0, x1, y1), angle), hd, p(0.5, 0.5), p(tx, ty), W, H, ratio);
        prop_assert!(g.rect.x0.is_finite() && g.rect.y1.is_finite());
        let _ = crop_geom::rotate(g, W, H);
    }

    /// Any crop text renders or is refused without panicking, within its bounds.
    #[test]
    fn hostile_crop_parameters_never_panic(vals in prop::collection::vec(prop_oneof![any::<f64>(), -2.0f64..3.0], 0..6)) {
        let text = vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
        if let Ok(c) = Crop::parse(&text) {
            let r = c.rect;
            prop_assert!(0.0 <= r.x0 && r.x0 < r.x1 && r.x1 <= 1.0 && 0.0 <= r.y0 && r.y0 < r.y1 && r.y1 <= 1.0);
            prop_assert_eq!(Crop::parse(&c.to_string()).unwrap(), c);
        }
        let img = scene(1, 12, 9);
        let out = render(Source::upright(&img), &recipe(&[&format!("crop({text})")]), Target::Fit { width: 7, height: 7 }).unwrap();
        prop_assert!(out.width >= 1 && out.height >= 1);
    }
}
