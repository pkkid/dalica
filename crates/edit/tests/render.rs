//! The renderer (`architecture.md` §6.3–§6.4, `data-model.md` §3.1, §3.3): M6.3's rotations and
//! flips render exactly as before; operations Dalica cannot render are skipped and kept; geometry
//! applies first wherever it stands; a region is the same window of the whole render; the render
//! key follows the operations' versions; hostile recipes, sizes and parameters never panic and stay
//! within the allocation limit. Fingerprints: `fingerprints/render-*.json`.

mod common;

use std::time::Instant;

use common::{assert_two_sizes_agree, check_fingerprint, recipe, scene};
use dalica_edit::render::{MAX_PIXELS, MAX_SCALE};
use dalica_edit::version::{self, key, key_with};
use dalica_edit::{Error, Geometry, LISTED, Op, Point, Recipe, Region, Source, Target, Typed, render, render_region};
use dalica_image::{Orientation, Rgba8};
use dalica_scenes::{Kind, Scene, orient};
use proptest::prelude::*;

const GEOMETRY: [&str; 5] = ["rotate(90)", "rotate(180)", "rotate(270)", "flip_h()", "flip_v()"];

/// Every sequence of up to `max` operations from `GEOMETRY`, the empty one first.
fn recipes(max: usize) -> Vec<Vec<&'static str>> {
    let mut all = vec![vec![]];
    let mut last = vec![vec![]];
    for _ in 0..max {
        last = last.iter().flat_map(|r: &Vec<&str>| GEOMETRY.iter().map(move |op| [r.clone(), vec![*op]].concat())).collect();
        all.extend(last.iter().cloned());
    }
    all
}

fn stored(up: &Rgba8, o: u8) -> Rgba8 {
    let rgb = dalica_scenes::Rgb8 { width: up.width, height: up.height, data: up.data.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect() };
    common::rgba(&orient::store(&rgb, o).unwrap())
}

fn exif(o: u8) -> Orientation {
    Orientation::new(o).unwrap()
}

/// The window `(x, y, w, h)` of `img`.
fn window(img: &Rgba8, x: u32, y: u32, w: u32, h: u32) -> Rgba8 {
    let mut data = Vec::new();
    for row in y..y + h {
        let at = ((row * img.width + x) * 4) as usize;
        data.extend_from_slice(&img.data[at..at + (w * 4) as usize]);
    }
    Rgba8 { width: w, height: h, data }
}

#[test]
fn rotations_and_flips_render_exactly_as_before() {
    let up = scene(29, 48, 32);
    let all = recipes(4);
    assert_eq!(all.len(), 781);
    for o in 1..=8u8 {
        let file = stored(&up, o);
        let source = Source::new(&file, exif(o));
        for ops in &all {
            let r = recipe(ops);
            // M6.3: the EXIF orientation and the recipe's turns, as one pixel-exact transform.
            let before = r.display(exif(o)).apply(file.clone()).unwrap();
            assert_eq!(render(source, &r, Target::Full).unwrap(), before, "EXIF {o} + {ops:?}");
            // A box at least as large as the picture does not enlarge it.
            assert_eq!(render(source, &r, Target::Fit { width: 4000, height: 4000 }).unwrap(), before, "EXIF {o} + {ops:?}");
        }
    }
}

#[test]
fn operations_dalica_cannot_render_are_skipped_and_print_back_unchanged() {
    let img = scene(3, 40, 30);
    let source = Source::upright(&img);
    let plain = render(source, &recipe(&["rotate(90)", "flip_h()"]), Target::Full).unwrap();
    for between in [
        "hologram(3)",            // from a newer version
        "legacy:redeye(0.1,0.2)", // kept from the legacy organizer
        "rotate(45)",             // a typed operation with a parameter it does not take
        "autofix(1)",             // a module refusing its parameters
        "sepia()",                // known, no module yet (M12)
        "crop((",                 // damaged
    ] {
        let ops = ["rotate(90)", between, "flip_h()"];
        let r = recipe(&ops);
        assert_eq!(render(source, &r, Target::Full).unwrap(), plain, "{between}");
        assert_eq!(r.print(), ops, "{between}");
        // Printing and parsing again keeps it in place.
        assert_eq!(Recipe::parse(r.print()).unwrap(), r);
    }
    // Nothing before or after an unknown operation is dropped: the merge rules would have merged
    // the two rotations had the unknown operation not stood between them.
    let r = recipe(&["rotate(90)", "future()", "rotate(90)"]);
    assert_eq!(r.len(), 3);
    assert_eq!(render(source, &r, Target::Full).unwrap(), render(source, &recipe(&["rotate(180)"]), Target::Full).unwrap());
}

#[test]
fn geometry_applies_first_wherever_it_stands() {
    let img = scene(11, 36, 24);
    let source = Source::new(&img, exif(6));
    let adjust = ["autofix()", "tuning(0.2,0.1,0.05,-0.3,#808080)", "retouch(0.1,0.2,0.3,0.4,0.02)", "autocolor()", "fill_light(0.25)"];
    for turn in ["rotate(90)", "flip_v()", "straighten(1.5)", "crop(0.1,0.1,0.9,0.9)"] {
        let first: Vec<&str> = [turn].into_iter().chain(adjust).collect();
        let last: Vec<&str> = adjust.into_iter().chain([turn]).collect();
        let middle: Vec<&str> = adjust[..2].iter().copied().chain([turn]).chain(adjust[2..].iter().copied()).collect();
        let expected = render(source, &recipe(&first), Target::Full).unwrap();
        for ops in [&last, &middle] {
            let r = recipe(ops);
            assert_eq!(r.pipeline().next().map(Op::to_string).as_deref(), Some(turn), "{ops:?}");
            assert_eq!(render(source, &r, Target::Full).unwrap(), expected, "{ops:?}");
            assert_eq!(
                render(source, &r, Target::Fit { width: 17, height: 17 }).unwrap(),
                render(source, &recipe(&first), Target::Fit { width: 17, height: 17 }).unwrap(),
                "{ops:?}"
            );
        }
    }
}

#[test]
fn a_region_is_that_window_of_the_whole_render() {
    let up = scene(7, 61, 43);
    let cases: [(u8, &[&str]); 5] = [
        (1, &[]),
        (6, &["rotate(90)"]),
        (3, &["flip_h()", "autofix()", "rotate(270)"]),
        (8, &["crop(0.1,0.2,0.8,0.9)", "tuning(0.4,0,0,0,none)", "flip_v()"]),
        (5, &["autocontrast()", "straighten(-3)", "autocolor()", "retouch(0.5,0.5,0.2,0.2,0.05)"]),
    ];
    for (o, ops) in cases {
        let file = stored(&up, o);
        let source = Source::new(&file, exif(o));
        let r = recipe(ops);
        for scale in [1.0, 0.5, 0.37, 1.0 / 3.0, 2.5] {
            let whole = render(source, &r, Target::Scale(scale)).unwrap();
            let (w, h) = (whole.width, whole.height);
            assert_eq!(Geometry::new(&r, exif(o), file.width, file.height).display_size(scale).unwrap(), (w, h));
            for (x, y, ww, wh) in [(0, 0, w, h), (0, 0, 1, 1), (w / 3, h / 4, w / 2, h / 3), (w - 5, h - 4, 5, 4), (w / 2, 1, 1, h - 1)] {
                let region = Region { scale, x, y, width: ww.max(1), height: wh.max(1) };
                let part = render_region(source, &r, region).unwrap();
                assert_eq!(part, window(&whole, x, y, ww.max(1), wh.max(1)), "EXIF {o} {ops:?} at {scale}: {region:?}");
            }
            // A window reaching past the picture is cut to it.
            let past = render_region(source, &r, Region { scale, x: w - 3, y: h - 2, width: 50, height: 50 }).unwrap();
            assert_eq!(past, window(&whole, w - 3, h - 2, 3, 2));
            // A window wholly outside is refused.
            assert!(matches!(render_region(source, &r, Region { scale, x: w, y: 0, width: 5, height: 5 }), Err(Error::BadTarget(_))));
        }
    }
}

#[test]
fn the_render_key_follows_the_versions_of_the_operations_a_recipe_uses() {
    let recipes = [
        recipe(&[]),
        recipe(&["rotate(90)"]),
        recipe(&["crop(0.1,0.1,0.9,0.9)"]),
        recipe(&["crop(0.1,0.1,0.9,0.9)", "autofix()"]),
        recipe(&["autofix()", "future_op()"]),
        recipe(&["legacy:x()", "tuning(0,0,0,0,none)"]),
    ];
    let current = |name: &str| dalica_edit::ops::version(name);
    for r in &recipes {
        assert_eq!(key(r), key_with(r, current));
        assert_eq!(key(r), key(&Recipe::parse(r.print()).unwrap()), "stable for the same recipe");
    }
    // A new version of `crop` changes the renderer half of the key of the recipes with a crop, and
    // nothing else.
    let bumped = |name: &str| if name == "crop" { current(name).map(|v| v + 1) } else { current(name) };
    for r in &recipes {
        let uses_crop = r.ops().iter().any(|op| op.name() == Some("crop"));
        let (before, after) = (key(r), key_with(r, bumped));
        assert_eq!(before.recipe_hash, after.recipe_hash);
        assert_eq!(before.renderer_version != after.renderer_version, uses_crop, "{:?}", r.print());
    }
    // The recipe half follows the printed recipe, unknown entries included.
    assert_ne!(key(&recipes[4]).recipe_hash, key(&recipe(&["autofix()"])).recipe_hash);
    assert_ne!(key(&recipe(&["rotate(90)", "flip_h()"])).recipe_hash, key(&recipe(&["flip_h()", "rotate(90)"])).recipe_hash);
    // An unknown operation has no version; the renderer half ignores it.
    assert_eq!(key(&recipes[4]).renderer_version, key(&recipe(&["autofix()"])).renderer_version);
    assert_eq!(
        version::versions(&recipes[3]).into_iter().collect::<Vec<_>>(),
        vec![("autofix", current("autofix").unwrap()), ("crop", current("crop").unwrap())]
    );
}

#[test]
fn every_operation_has_a_label_and_typed_ones_print_back() {
    let labels = [
        ("rotate(90)", "Rotate"),
        ("flip_h()", "Flip Horizontal"),
        ("flip_v()", "Flip Vertical"),
        ("crop(0,0,1,1)", "Crop"),
        ("straighten(2)", "Straighten"),
        ("retouch(0.1,0.2,0.3,0.4,0.02)", "Retouch"),
        ("autofix()", "Auto Fix"),
        ("autocontrast()", "Auto Contrast"),
        ("autocolor()", "Auto Color"),
        ("fill_light(0.25)", "Fill Light"),
        ("tuning(0.2,0.1,0.05,-0.3,#808080)", "Tuning"),
        ("sepia()", "Sepia"),
        ("bw()", "B&W"),
        ("grain(10,true)", "Film Grain"),
    ];
    for (text, label) in labels {
        let op = Op::parse(text);
        assert_eq!(op.label(), Some(label), "{text}");
        assert!(op.version().is_some(), "{text}");
        // An operation with a module comes back from its typed parameters as the same text.
        if let Some(typed) = op.typed() {
            let typed: Typed = typed.unwrap();
            assert_eq!(Op::from(typed).to_string(), text);
        }
    }
    for name in LISTED {
        assert!(dalica_edit::ops::label(name).is_some(), "{name} has no label");
    }
    for skipped in ["hologram()", "legacy:redeye(1)", "rotate(45)"] {
        let op = Op::parse(skipped);
        assert_eq!((op.label(), op.version(), op.typed()), (None, None, None), "{skipped}");
    }
    assert!(matches!(Op::parse("autofix(1)").typed(), Some(Err(_))));
    assert_eq!(Op::parse("rotate(270)").typed(), Some(Ok(Typed::Rotate(dalica_edit::ops::orient::Rotate(dalica_edit::Turn::ThreeQuarters)))));
}

#[test]
fn the_geometry_maps_points_between_the_stored_and_the_displayed_picture() {
    for ops in recipes(2) {
        let r = recipe(&ops);
        let g = Geometry::new(&r, exif(7), 30, 20);
        assert_eq!(g.orientation(), r.orientation());
        for p in [Point::new(0.0, 0.0), Point::new(0.25, 0.75), Point::new(1.0, 0.5)] {
            let shown = g.to_display(p).unwrap();
            assert_eq!(shown, p.to_display(r.orientation()), "{ops:?}");
            let back = g.to_stored(shown).unwrap();
            assert!((back.x - p.x).abs() < 1e-12 && (back.y - p.y).abs() < 1e-12, "{ops:?}: {p:?} → {shown:?} → {back:?}");
        }
    }
}

#[test]
fn two_sizes_agree_after_downscale_and_renders_match_their_fingerprints() {
    let up = scene(21, 300, 200);
    for (case, o, ops) in
        [("render-fit-upright", 1u8, vec![]), ("render-fit-exif6-rotate90", 6, vec!["rotate(90)"]), ("render-fit-exif3-flip_h", 3, vec!["flip_h()"])]
    {
        let file = stored(&up, o);
        let small = assert_two_sizes_agree(Source::new(&file, exif(o)), &recipe(&ops), 64, 240, 2.0);
        check_fingerprint(case, &small, 0.5);
    }
    // An analysing operation (Auto Fix, from the fixed-size proxy) on soft blobs with the corner
    // marker painted over: the fix stretches the picture, and at a hard edge such as the marker's
    // it would magnify the two sizes' resampling differences past the tolerance.
    let mut blobs = common::rgba(&Scene::new(5, 300, 200).with_kind(Kind::Blobs).render().unwrap());
    let m = blobs.width.min(blobs.height) / 16;
    for y in m..2 * m {
        for x in m..2 * m {
            let (from, to) = (((2 * m * blobs.width + x) * 4) as usize, ((y * blobs.width + x) * 4) as usize);
            blobs.data.copy_within(from..from + 4, to);
        }
    }
    let small = assert_two_sizes_agree(Source::upright(&blobs), &recipe(&["autofix()"]), 64, 240, 2.0);
    check_fingerprint("render-fit-blobs-autofix", &small, 0.5);
    // A zoomed-in window, enlarged with bilinear taps.
    let file = stored(&up, 1);
    let zoom = render_region(Source::upright(&file), &recipe(&[]), Region { scale: 3.0, x: 300, y: 200, width: 96, height: 64 }).unwrap();
    check_fingerprint("render-region-3x", &zoom, 0.5);
}

#[test]
fn renders_over_the_limits_are_refused_before_allocating() {
    let img = scene(1, 64, 48);
    let source = Source::upright(&img);
    let r = recipe(&["rotate(90)"]);
    for target in [
        Target::Scale(0.0),
        Target::Scale(-1.0),
        Target::Scale(f64::NAN),
        Target::Scale(f64::INFINITY),
        Target::Scale(MAX_SCALE * 2.0),
        Target::Fit { width: 0, height: 10 },
    ] {
        assert!(matches!(render(source, &r, target), Err(Error::BadTarget(_))), "{target:?}");
    }
    // 64 × 48 at the largest scale is within the limit; a large source at it is not.
    assert!(render_region(source, &r, Region { scale: MAX_SCALE, x: 0, y: 0, width: 10, height: 10 }).is_ok());
    let big = Rgba8 { width: 4096, height: 4096, data: vec![0; 4096 * 4096 * 4] };
    let t = Instant::now();
    assert!(matches!(render(Source::upright(&big), &r, Target::Scale(MAX_SCALE)), Err(Error::TooLarge { .. })));
    assert!(t.elapsed().as_millis() < 500, "refused without rendering");
    // A small window of it is fine: only the window is allocated.
    let part = render_region(Source::upright(&big), &r, Region { scale: MAX_SCALE, x: 100_000, y: 100_000, width: 32, height: 32 }).unwrap();
    assert_eq!((part.width, part.height), (32, 32));
    // A window as large as the whole picture is not.
    let whole = Region { scale: MAX_SCALE, x: 0, y: 0, width: u32::MAX, height: u32::MAX };
    assert!(matches!(render_region(Source::upright(&big), &r, whole), Err(Error::TooLarge { .. })));
    // A buffer of the wrong length, and an empty picture.
    let bad = Rgba8 { width: 10, height: 10, data: vec![0; 399] };
    assert!(matches!(render(Source::upright(&bad), &r, Target::Full), Err(Error::BadImage { .. })));
    let empty = Rgba8 { width: 0, height: 7, data: vec![] };
    assert_eq!(render(Source::upright(&empty), &r, Target::Full).unwrap(), Rgba8 { width: 7, height: 0, data: vec![] });
    assert!(render_region(Source::upright(&empty), &r, Region { scale: 1.0, x: 0, y: 0, width: 1, height: 1 }).is_err());
    assert!(u64::from(part.width) * u64::from(part.height) <= MAX_PIXELS);
}

#[test]
fn a_large_render_is_timed() {
    // A 24 MP photo to a 2560-pixel preview and to a 256-pixel thumbnail, through the whole
    // pipeline (two operations analysing their proxy). Printed for the PR; budgets come with M7.11.
    let img = scene(5, 6000, 4000);
    let r = recipe(&["autofix()", "rotate(90)", "tuning(0.3,0,0,0,none)"]);
    for edge in [2560, 256] {
        let t = Instant::now();
        let out = render(Source::upright(&img), &r, Target::Fit { width: edge, height: edge }).unwrap();
        println!("24 MP → {} × {}: {:.1} ms", out.width, out.height, t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(out.height, edge);
    }
}

/// Operation texts for random recipes: every kind, with hostile parameters.
fn op_text() -> impl Strategy<Value = String> {
    let fixed = prop::sample::select(vec![
        "rotate(90)",
        "rotate(180)",
        "rotate(270)",
        "flip_h()",
        "flip_v()",
        "autofix()",
        "autocontrast()",
        "autocolor()",
        "sepia()",
        "crop(0.1,0.2,0.9,0.8)",
        "crop(NaN,NaN,NaN,NaN)",
        "crop(1e308,-1e308,0,0)",
        "crop(0.9,0.9,0.1,0.1)",
        "crop()",
        "straighten(1e300)",
        "straighten(NaN)",
        "straighten(-inf)",
        "retouch(0.5,0.5,0.5,0.5,1e9)",
        "retouch(;;;)",
        "tuning(NaN,inf,-inf,1e308,#zzzzzz)",
        "fill_light(-1)",
        "legacy:redeye()",
        "hologram()",
        "rotate(45)",
    ])
    .prop_map(String::from);
    prop_oneof![4 => fixed, 1 => "[a-z_]{1,8}\\([^()]{0,12}\\)", 1 => any::<String>()]
}

fn hostile_f64() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(0.0),
        Just(-0.0),
        Just(f64::NAN),
        Just(f64::INFINITY),
        Just(f64::MIN_POSITIVE),
        Just(1e-300),
        Just(MAX_SCALE),
        Just(1e300),
        0.001f64..4.0,
        any::<f64>(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn random_recipes_and_hostile_targets_never_panic(
        ops in prop::collection::vec(op_text(), 0..8),
        o in 1u8..=8,
        (w, h) in (0u32..24, 0u32..24),
        scale in hostile_f64(),
        (bw, bh) in (0u32..64, 0u32..64),
        (x, y, rw, rh) in (any::<u32>(), any::<u32>(), any::<u32>(), any::<u32>()),
    ) {
        let r = Recipe::parse(&ops).unwrap();
        prop_assert_eq!(r.print(), ops.clone());
        let img = Rgba8 { width: w, height: h, data: (0..w * h * 4).map(|i| (i * 37 % 251) as u8).collect() };
        let source = Source::new(&img, exif(o));
        let mut results = vec![
            render(source, &r, Target::Full),
            render(source, &r, Target::Fit { width: bw, height: bh }),
            render(source, &r, Target::Scale(scale)),
            render_region(source, &r, Region { scale, x, y, width: rw, height: rh }),
            render_region(source, &r, Region { scale, x: x % 64, y: y % 64, width: rw % 64, height: rh % 64 }),
        ];
        for out in results.drain(..).flatten() {
            prop_assert!(u64::from(out.width) * u64::from(out.height) <= MAX_PIXELS);
            prop_assert_eq!(out.data.len() as u64, u64::from(out.width) * u64::from(out.height) * 4);
        }
        // The key never panics either, and the geometry maps or refuses points.
        let _ = key(&r);
        let g = Geometry::new(&r, exif(o), w, h);
        let _ = g.to_display(Point::new(scale, -scale));
        let _ = g.to_stored(Point::new(f64::NAN, 0.5));
    }
}
