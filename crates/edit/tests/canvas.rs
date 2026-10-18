//! The effects that change the picture's size (M12.13, `editing.md` §8.3–§8.4, `data-model.md`
//! §3.3): Widescreen Film and Border on the renderer's frames. A fingerprint per effect at its
//! defaults (the same at two render sizes); a Border is the photo grown by both bands with the
//! photo's pixels unchanged inside; Widescreen Film is 1.7:1 with its bars, none without Letterbox;
//! a later crop crops the photo and keeps the frame; a rotation turns the framed picture; a later
//! effect's stored-frame points stay on the photo; the photo's centre maps to the frame's photo
//! area and back; a region equals that window of the whole render; frames past the limits are
//! skipped without a panic. Fingerprints: `fingerprints/canvas-*.json`.

use crate::common::{assert_two_sizes_agree, check_fingerprint, recipe, rgba, run, scene, window};
use dalica_edit::render::{MAX_EDGE, MAX_PIXELS};
use dalica_edit::{Dihedral, Geometry, Point, Region, Source, Target, render, render_region};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::{Kind, Scene};

const BORDER: &str = "border(#000000,20,#ffffff,5,0,0)";
const WIDESCREEN: &str = "widescreen(true)";

fn px(img: &Rgba8, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * img.width + x) * 4) as usize;
    [img.data[at], img.data[at + 1], img.data[at + 2], img.data[at + 3]]
}

#[test]
fn each_effect_at_its_defaults_matches_its_fingerprint_at_two_sizes() {
    let up = rgba(&Scene::new(5, 400, 300).with_kind(Kind::Blobs).render().unwrap());
    // A frame's bands are whole pixels at every size (the photo lands on whole pixels), so its
    // lines may sit half a pixel apart between two sizes: the cells they cross differ a little more
    // than a pixel-only effect's.
    for (case, op, tolerance) in [
        ("canvas-widescreen", WIDESCREEN, 3.0),
        ("canvas-border", BORDER, 6.0),
        ("canvas-border-oval-caption", "border(#203040,40,#f0e0c0,15,100,120)", 6.0),
    ] {
        let small = assert_two_sizes_agree(Source::upright(&up), &recipe(&[op]), 160, 400, tolerance);
        check_fingerprint(case, &small, 0.5);
    }
}

#[test]
fn a_border_is_the_photo_grown_by_both_bands_with_the_photo_unchanged_inside() {
    let up = scene(7, 300, 200);
    let plain = run(&up, &["tuning(0.2,0,0,0.3,none)"]);
    for (op, outer, inner, caption) in [
        (BORDER, 4u32, 1u32, 0u32),
        ("border(#ff0000,100,#00ff00,30,0,0)", 20, 6, 0),
        ("border(#102030,10,#ffffff,0,0,200)", 2, 0, 40),
        ("border(#000000,0,#ffffff,0,0,0)", 0, 0, 0),
    ] {
        let framed = run(&up, &["tuning(0.2,0,0,0.3,none)", op]);
        let band = outer + inner;
        assert_eq!((framed.width, framed.height), (300 + 2 * band, 200 + 2 * band + caption), "{op}");
        // The photo's pixels, unchanged.
        assert_eq!(window(&framed, band, band, 300, 200), plain, "{op}");
        let entry = dalica_edit::Op::parse(op);
        let Some(Ok(dalica_edit::Typed::Border(b))) = entry.typed() else { panic!("{op}: a border") };
        let colour = |c: [u8; 3]| [c[0], c[1], c[2], 255];
        // The outer band in its colour at every corner and the caption; the inner band around the
        // photo.
        if outer > 0 {
            for (x, y) in [(0, 0), (framed.width - 1, 0), (0, framed.height - 1), (framed.width - 1, framed.height - 1)] {
                assert_eq!(px(&framed, x, y), colour(b.outer_color), "{op} at {x},{y}");
            }
        }
        if inner > 0 {
            for (x, y) in [(band - 1, band + 50), (band + 300, band + 50), (band + 100, band - 1), (band + 100, band + 200)] {
                assert_eq!(px(&framed, x, y), colour(b.inner_color), "{op} at {x},{y}");
            }
        }
        if caption > 0 {
            assert_eq!(px(&framed, 150, band * 2 + 200 + caption / 2), colour(b.outer_color), "{op}: the caption");
        }
    }
    // A corner radius of 100 opens an oval: the photo's corners are covered, its centre and the
    // middles of its sides show.
    let oval = run(&up, &["border(#000000,20,#ffffff,5,100,0)"]);
    assert_eq!(px(&oval, 5, 5), [0, 0, 0, 255], "the photo's corner under the outer band");
    assert_eq!(px(&oval, 4, 105), [255, 255, 255, 255], "the inner band along the oval");
    for (x, y) in [(155, 105), (155, 7), (7, 105), (303, 105)] {
        assert_eq!(px(&oval, x, y), px(&up, x - 5, y - 5), "the oval's opening at {x},{y}");
    }
}

#[test]
fn widescreen_film_is_one_point_seven_to_one_with_its_bars_and_without_letterbox_none() {
    for (w, h) in [(400u32, 300u32), (200, 300), (900, 300), (171, 100)] {
        let up = scene(3, w, h);
        let film = run(&up, &[WIDESCREEN]);
        let ratio = f64::from(film.width) / f64::from(film.height);
        assert!((f64::from(film.width) - 1.7 * f64::from(film.height)).abs() <= 1.0, "{w}×{h}: {}×{} ({ratio})", film.width, film.height);
        assert!(film.width <= w && film.height <= h, "{w}×{h}: a crop");
        // The bars cover 15 % of the height each; a row they cover in part is darkened, not black.
        let bar = f64::from(film.height) * 0.15;
        let row_black = |img: &Rgba8, y: u32| (0..img.width).all(|x| px(img, x, y)[..3] == [0, 0, 0]);
        for y in 0..film.height {
            let barred = f64::from(y) + 1.0 <= bar || f64::from(film.height - y) <= bar;
            assert_eq!(row_black(&film, y), barred, "{w}×{h}: row {y} of {} (bar {bar})", film.height);
        }
        let open = run(&up, &["widescreen(false)"]);
        assert_eq!((open.width, open.height), (film.width, film.height));
        assert!((0..open.height).all(|y| !row_black(&open, y)), "{w}×{h}: no bars without Letterbox");
        // Between the bars it is the same picture.
        let edge = bar.ceil() as u32;
        assert_eq!(window(&open, 0, edge, open.width, open.height - 2 * edge), window(&film, 0, edge, film.width, film.height - 2 * edge));
    }
}

#[test]
fn a_crop_after_a_border_crops_the_photo_and_keeps_the_frame() {
    let up = scene(19, 240, 160);
    let crop = "crop(0.25,0.1,0.75,0.9)";
    let after = run(&up, &[BORDER, crop]);
    assert_eq!(after, run(&up, &[crop, BORDER]));
    let photo = run(&up, &[crop]);
    let band = 3; // 20/1000 and 5/1000 of 120 px, rounded
    assert_eq!((after.width, after.height), (photo.width + 2 * band, photo.height + 2 * band));
    assert_eq!(window(&after, band, band, photo.width, photo.height), photo);
    assert_eq!(px(&after, 0, 0), [0, 0, 0, 255]);
}

#[test]
fn a_rotation_after_a_frame_turns_the_framed_picture() {
    let up = scene(23, 120, 90);
    for frame in [BORDER, "border(#ff8000,40,#ffffff,10,30,150)", WIDESCREEN] {
        let framed = run(&up, &[frame]);
        for (turn, d) in [("rotate(90)", Dihedral::ROTATE_90), ("flip_h()", Dihedral::FLIP_H), ("rotate(270)", Dihedral::ROTATE_270)] {
            assert_eq!(run(&up, &[frame, turn]), d.apply(framed.clone()).unwrap(), "{frame} {turn}");
            assert_eq!(run(&up, &[turn, frame]), d.apply(framed.clone()).unwrap(), "{turn} {frame}");
        }
    }
}

#[test]
fn the_photos_centre_maps_to_the_frames_photo_area_and_back() {
    let r = recipe(&["crop(0.1,0.2,0.7,0.8)", BORDER, "border(#ffffff,50,#000000,0,0,100)", "rotate(90)"]);
    let g = Geometry::new(&r, Orientation::default(), 400, 300);
    assert_eq!(g.frames(), 2);
    assert_eq!(g.photo_size(), (240.0, 180.0));
    // 240 × 180: the first border is 4 + 1 px, then 250 × 190 gets 10 px and a 19 px caption.
    assert_eq!(g.output_size(1.0).unwrap(), (270, 229));
    assert_eq!(g.display_size(1.0).unwrap(), (229, 270));
    let area = g.photo_area().unwrap();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(close(area.x0, 15.0 / 270.0) && close(area.y0, 15.0 / 229.0), "{area:?}");
    assert!(close(area.x1, 255.0 / 270.0) && close(area.y1, 195.0 / 229.0), "{area:?}");
    // The crop's centre is the stored point (0.4, 0.5): the middle of the photo area.
    let c = g.stored_to_output(Point::new(0.4, 0.5)).unwrap();
    assert!(close(c.x, (area.x0 + area.x1) / 2.0) && close(c.y, (area.y0 + area.y1) / 2.0), "{c:?}");
    let back = g.output_to_stored(c).unwrap();
    assert!(close(back.x, 0.4) && close(back.y, 0.5), "{back:?}");
    let shown = g.to_display(Point::new(0.4, 0.5)).unwrap();
    let again = g.to_stored(shown).unwrap();
    assert!(close(again.x, 0.4) && close(again.y, 0.5), "{again:?}");
    // A point on the frame is off the photo.
    let corner = g.output_to_stored(Point::new(0.0, 0.0)).unwrap();
    assert!(corner.x < 0.1 && corner.y < 0.2, "{corner:?}");
    // Widescreen Film crops: the photo reaches past the output.
    let film = Geometry::new(&recipe(&[WIDESCREEN]), Orientation::default(), 400, 300);
    let area = film.photo_area().unwrap();
    assert!(close(area.x0, 0.0) && close(area.x1, 1.0) && area.y0 < 0.0 && area.y1 > 1.0, "{area:?}");
}

#[test]
fn a_later_effect_stays_on_the_photo_inside_a_frame() {
    // A retouch stroke placed after a Border heals the same spot of the photo as one placed before.
    let up = scene(31, 200, 150);
    let heal = "retouch(0.3,0.4,0.6,0.5,0.04)";
    let before = run(&up, &[heal, BORDER]);
    let after = run(&up, &[BORDER, heal]);
    assert_eq!((before.width, before.height), (after.width, after.height));
    let worst = before.data.iter().zip(&after.data).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(worst <= 1, "the stroke lands on the same pixels ({worst})");
    assert_ne!(after, run(&up, &[BORDER]), "the stroke is applied");
}

#[test]
fn a_region_equals_that_window_of_the_whole_render() {
    let up = scene(13, 160, 120);
    let source = Source::upright(&up);
    for ops in [
        vec![BORDER],
        vec![WIDESCREEN],
        vec!["autocontrast()", WIDESCREEN, "retouch(0.3,0.4,0.6,0.5,0.04)", "border(#336699,60,#ffffff,20,40,100)", "autocolor()", "rotate(90)"],
        vec!["crop(0.1,0.1,0.9,0.8)", "border(#000000,30,#ffffff,10,100,0)", "border(#ffffff,30,#000000,10,0,50)", "flip_v()"],
    ] {
        let r = recipe(&ops);
        for scale in [1.0, 0.5, 2.0] {
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

#[test]
fn a_thousand_borders_stay_within_the_limits_without_a_panic() {
    let up = scene(2, 64, 48);
    let source = Source::upright(&up);
    for entry in ["border(#000000,100,#ffffff,100,100,200)", BORDER, "widescreen(true)"] {
        let many: Vec<&str> = std::iter::repeat_n(entry, 1000).collect();
        let r = recipe(&many);
        let g = Geometry::new(&r, Orientation::default(), 64, 48);
        let (w, h) = g.output_size(1.0).unwrap();
        assert!(w <= MAX_EDGE && h <= MAX_EDGE && u64::from(w) * u64::from(h) <= MAX_PIXELS, "{entry}: {w}×{h}");
        let fit = render(source, &r, Target::Fit { width: 96, height: 96 }).unwrap();
        assert!(fit.width <= 96 && fit.height <= 96 && fit.width >= 1, "{entry}: {}×{}", fit.width, fit.height);
        let part = render_region(source, &r, Region { scale: 1.0, x: w / 2, y: h / 2, width: 16, height: 16 }).unwrap();
        assert!(part.width <= 16 && part.height <= 16);
        // The frames past the limits are kept in the recipe.
        assert_eq!(r.len(), 1000);
    }
    // Hostile sizes: a frame on a one-pixel picture, a box of one pixel.
    let dot = scene(1, 1, 1);
    let r = recipe(&[BORDER, WIDESCREEN, "border(#000000,100,#ffffff,100,100,200)"]);
    assert!(render(Source::upright(&dot), &r, Target::Full).is_ok());
    let fit = render(Source::upright(&up), &r, Target::Fit { width: 1, height: 1 }).unwrap();
    assert!(fit.width >= 1 && fit.height >= 1);
}
