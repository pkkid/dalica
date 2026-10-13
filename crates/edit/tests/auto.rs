//! Auto Fix, Auto Contrast, Auto Color and the Tuning wands (`editing.md` §6.3, §7): a planted
//! colour cast is removed; a low-contrast scene stretches to nearly the full range with little
//! clipping; a full-range neutral photo barely changes; the result depends on the picture at the
//! operation's place (a cropped picture, an operation before it); a thumbnail and the full size
//! agree after downscale; the wands' values on known scenes; the Neutral Color Picker's colour;
//! fingerprints `fingerprints/auto-*.json`, one per operation; proptest random images never panic.

mod common;

use std::time::Instant;

use common::{assert_two_sizes_agree, check_fingerprint, recipe, scene};
use dalica_edit::analysis::{self, Lighting, PROXY_EDGE};
use dalica_edit::ops::Operation;
use dalica_edit::ops::auto::{AutoColor, AutoContrast, AutoFix};
use dalica_edit::ops::tone::luma;
use dalica_edit::{Op, Point, Source, Target, Typed, render};
use dalica_image::Rgba8;
use dalica_scenes::{Kind, Scene};
use proptest::prelude::*;

const OPS: [&str; 3] = ["autofix()", "autocontrast()", "autocolor()"];

fn run(img: &Rgba8, ops: &[&str]) -> Rgba8 {
    render(Source::upright(img), &recipe(ops), Target::Full).unwrap()
}

fn pixels(img: &Rgba8) -> impl Iterator<Item = [u8; 3]> + '_ {
    img.data.chunks(4).map(|p| [p[0], p[1], p[2]])
}

/// Each channel's mean, 0–255.
fn means(img: &Rgba8) -> [f64; 3] {
    let n = (img.data.len() / 4) as f64;
    let mut sum = [0.0; 3];
    for p in pixels(img) {
        for c in 0..3 {
            sum[c] += f64::from(p[c]);
        }
    }
    sum.map(|s| s / n)
}

fn spread(m: [f64; 3]) -> f64 {
    m.iter().copied().fold(f64::MIN, f64::max) - m.iter().copied().fold(f64::MAX, f64::min)
}

/// `img` with every pixel through `f`.
fn map(img: &Rgba8, f: impl Fn([u8; 3]) -> [f64; 3]) -> Rgba8 {
    let mut out = img.clone();
    for p in out.data.chunks_mut(4) {
        let v = f([p[0], p[1], p[2]]);
        for c in 0..3 {
            p[c] = v[c].round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// A `scenes` photo turned grey (its luma), so grey-world holds.
fn grey_scene(seed: u64, w: u32, h: u32) -> Rgba8 {
    map(&scene(seed, w, h), |p| [f64::from(luma(p.map(|v| f32::from(v) / 255.0))) * 255.0; 3])
}

/// A grey `scenes` photo with its levels spread evenly over the whole range (each pixel replaced
/// by its rank): a full-range neutral photo whose median is the middle.
fn full_range_neutral(seed: u64, w: u32, h: u32) -> Rgba8 {
    let grey = grey_scene(seed, w, h);
    let mut order: Vec<(u8, usize)> = pixels(&grey).enumerate().map(|(i, p)| (p[0], i)).collect();
    order.sort();
    let n = order.len() as f64;
    let mut out = grey.clone();
    for (rank, (_, i)) in order.into_iter().enumerate() {
        let v = (rank as f64 / (n - 1.0) * 255.0).round() as u8;
        out.data[i * 4..i * 4 + 3].copy_from_slice(&[v; 3]);
    }
    out
}

fn with_cast(img: &Rgba8) -> Rgba8 {
    map(img, |p| [0, 1, 2].map(|c| f64::from(p[c]) * CAST[c]))
}

fn mean_abs_diff(a: &Rgba8, b: &Rgba8) -> f64 {
    a.data.iter().zip(&b.data).map(|(x, y)| (f64::from(*x) - f64::from(*y)).abs()).sum::<f64>() / a.data.len() as f64 * 4.0 / 3.0
}

fn max_abs_diff(a: &Rgba8, b: &Rgba8) -> u8 {
    a.data.iter().zip(&b.data).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// The cast every cast test plants: a warm, yellow-green tint.
const CAST: [f64; 3] = [1.0, 0.88, 0.7];

#[test]
fn a_planted_colour_cast_is_removed() {
    for seed in [1, 7, 21] {
        let clean = grey_scene(seed, 300, 200);
        let cast = with_cast(&clean);
        assert!(spread(means(&cast)) > 25.0, "the cast shows: {:?}", means(&cast));
        for op in ["autocolor()", "autofix()"] {
            let fixed = run(&cast, &[op]);
            let m = means(&fixed);
            // Stated tolerance: the channel means within 2 of 255 of each other.
            assert!(spread(m) <= 2.0, "{op} on seed {seed}: means {m:?} (cast {:?})", means(&cast));
        }
        // Auto Contrast keeps the balance of the channels (one set of levels for all three).
        let levelled = means(&run(&cast, &["autocontrast()"]));
        assert!(levelled[0] > levelled[1] && levelled[1] > levelled[2], "{levelled:?}");
        // The Auto color wand's colour, as Tuning's neutral, removes it too.
        let picked = analysis::auto_neutral(&cast).expect("a colour");
        let op = format!("tuning(0,0,0,0,#{:02x}{:02x}{:02x})", picked[0], picked[1], picked[2]);
        let m = means(&run(&cast, &[&op]));
        assert!(spread(m) <= 2.0, "{op} on seed {seed}: means {m:?}");
        // A clean grey photo needs no correction: the wand picks a grey.
        let grey = analysis::auto_neutral(&clean).expect("a colour");
        assert!(grey.iter().max().unwrap() - grey.iter().min().unwrap() <= 1, "{grey:?}");
    }
}

#[test]
fn a_strong_single_colour_is_corrected_only_so_far() {
    // A photo that is all one green: the gains stay within 1/1.35 … 1.35, so it is not made grey.
    let green = Rgba8 { width: 64, height: 64, data: (0..64 * 64).flat_map(|_| [60, 170, 50, 255]).collect() };
    let p = pixels(&run(&green, &["autocolor()"])).next().unwrap();
    assert!(p[1] > p[0] + 40 && p[1] > p[2] + 40, "still green: {p:?}");
}

#[test]
fn a_low_contrast_scene_stretches_to_nearly_the_full_range() {
    for seed in [2, 8, 13] {
        // A full-range neutral photo squeezed into 0.35 … 0.65.
        let flat = map(&full_range_neutral(seed, 300, 200), |p| p.map(|v| 0.35 * 255.0 + f64::from(v) * 0.3));
        for op in ["autocontrast()", "autofix()"] {
            let out = run(&flat, &[op]);
            let mut l: Vec<u8> = pixels(&out).map(|p| p[1]).collect();
            l.sort();
            let at = |q: f64| l[((l.len() - 1) as f64 * q).round() as usize];
            // Stated: the 1st and 99th percentiles reach within 8 of black and white, and at most
            // 1.5 % of the pixels clip at either end.
            assert!(at(0.01) <= 8 && at(0.99) >= 247, "{op} on seed {seed}: {} … {}", at(0.01), at(0.99));
            let clipped = pixels(&out).filter(|p| p.iter().any(|v| *v == 0 || *v == 255)).count() as f64 / l.len() as f64;
            assert!(clipped <= 0.015, "{op} on seed {seed}: {:.2} % clipped", clipped * 100.0);
        }
    }
}

#[test]
fn a_full_range_neutral_photo_barely_changes() {
    for seed in [3, 9, 17] {
        let img = full_range_neutral(seed, 300, 200);
        for op in OPS {
            let out = run(&img, &[op]);
            let (mean, max) = (mean_abs_diff(&img, &out), max_abs_diff(&img, &out));
            // Stated: on average under 1.5 of 255, and no pixel by more than 4.
            assert!(mean < 1.5 && max <= 4, "{op} on seed {seed}: mean {mean:.2}, max {max}");
        }
    }
}

#[test]
fn the_result_depends_on_the_picture_at_the_operations_place() {
    // A crop changes the picture the fix sees: fixing the left third alone differs from the same
    // third of the fixed whole. (The `crop` entry renders with M7.7; its output is exactly such a
    // window, and the proxy is made after it.)
    let whole = scene(5, 300, 200);
    let cut = |img: &Rgba8, x0: u32, w: u32| {
        let data =
            (0..img.height).flat_map(|y| img.data[((y * img.width + x0) * 4) as usize..((y * img.width + x0 + w) * 4) as usize].to_vec()).collect();
        Rgba8 { width: w, height: img.height, data }
    };
    for op in OPS {
        let of_whole = cut(&run(&whole, &[op]), 0, 100);
        let of_third = run(&cut(&whole, 0, 100), &[op]);
        let d = mean_abs_diff(&of_whole, &of_third);
        assert!(d > 2.0, "{op}: cropping first changed the result by only {d:.2}");
    }
    // An operation applied before changes what the fix sees: warming then Auto Color takes the
    // warmth back out; Auto Color then warming keeps it.
    let grey = grey_scene(4, 300, 200);
    let rb = |img: &Rgba8| {
        let m = means(img);
        m[0] / m[2]
    };
    let warm_then_fix = rb(&run(&grey, &["tuning(0,0,0,1,none)", "autocolor()"]));
    let fix_then_warm = rb(&run(&grey, &["autocolor()", "tuning(0,0,0,1,none)"]));
    assert!((warm_then_fix - 1.0).abs() < 0.03, "red/blue {warm_then_fix}");
    assert!(fix_then_warm > 1.2, "red/blue {fix_then_warm}");
}

#[test]
fn a_thumbnail_and_the_full_size_agree_after_downscale() {
    // The analysis is the same proxy at every size, so the parameters are the same; what differs
    // is resampling before the fix, which the fix's stretch magnifies. Stated: on average under
    // 1.25 of 255 per pixel, and within 10 per fingerprint cell (the cells holding the scenes'
    // hard-edged white marker differ most: up to 4× contrast and 1.35× a channel turn the
    // renderer's and the reference downscale's 1-level edge differences into several).
    let up = common::rgba(&Scene::new(5, 300, 200).with_kind(Kind::Blobs).render().unwrap());
    let cast = with_cast(&grey_scene(6, 300, 200));
    for img in [&up, &cast] {
        for op in OPS {
            let small = assert_two_sizes_agree(Source::upright(img), &recipe(&[op]), 64, 240, 10.0);
            let large = render(Source::upright(img), &recipe(&[op]), Target::Fit { width: 240, height: 240 }).unwrap();
            let d = mean_abs_diff(&small, &common::box_downscale(&large, small.width, small.height));
            assert!(d < 1.25, "{op}: 64 px and 240 px differ by {d:.2} on average");
            // A 128 px thumbnail and the full size reduced to it.
            let thumb = render(Source::upright(img), &recipe(&[op]), Target::Fit { width: 128, height: 128 }).unwrap();
            let full = common::box_downscale(&run(img, &[op]), thumb.width, thumb.height);
            let d = mean_abs_diff(&thumb, &full);
            assert!(d < 1.25, "{op}: thumbnail and full size differ by {d:.2} on average");
        }
    }
}

#[test]
fn the_lighting_wand_on_known_scenes() {
    let base = full_range_neutral(11, 300, 200);
    // Spread over the whole range with its middle at the middle: nothing to do.
    assert_eq!(analysis::auto_lighting(&base), Lighting::default());
    // Dark and dull: Fill Light and Highlights, no Shadows.
    let dark = map(&base, |p| p.map(|v| f64::from(v) * 0.5));
    let l = analysis::auto_lighting(&dark);
    assert!(l.fill >= 0.25 && l.fill <= 0.6, "{l:?}");
    assert!(l.highlights >= 0.3, "{l:?}");
    assert_eq!(l.shadows, 0.0, "{l:?}");
    // Washed-out blacks over a bright middle: Shadows, no Fill Light, no Highlights.
    let washed = map(&base, |p| p.map(|v| 0.25 * 255.0 + f64::from(v) * 0.75));
    let l = analysis::auto_lighting(&washed);
    assert!(l.shadows >= 0.25, "{l:?}");
    assert_eq!((l.fill, l.highlights), (0.0, 0.0), "{l:?}");
    // The values print short and lie in their controls' ranges; applying them brightens the dark
    // photo.
    let l = analysis::auto_lighting(&dark);
    for v in [l.fill, l.highlights, l.shadows] {
        assert_eq!((v * 100.0).round() / 100.0, v);
    }
    let op = format!("tuning({},{},{},0,none)", l.fill, l.highlights, l.shadows);
    assert!(Op::parse(&op).typed().is_some_and(|t| t.is_ok()), "{op}");
    let lifted = run(&dark, &[&op]);
    assert!(means(&lifted)[1] > means(&dark)[1] + 10.0, "{op}");
    // The wand reads any size the same way: the photo at full size and at its proxy size agree.
    let dark_big = map(&full_range_neutral(11, 1200, 800), |p| p.map(|v| f64::from(v) * 0.5));
    let small = common::box_downscale(&dark_big, PROXY_EDGE, PROXY_EDGE * 2 / 3);
    let (a, b) = (analysis::auto_lighting(&dark_big), analysis::auto_lighting(&small));
    assert!((a.fill - b.fill).abs() <= 0.02 && (a.highlights - b.highlights).abs() <= 0.02, "{a:?} {b:?}");
    // Nothing visible: nothing to do, and no colour to pick.
    let clear = Rgba8 { width: 10, height: 10, data: vec![0; 400] };
    assert_eq!(analysis::auto_lighting(&clear), Lighting::default());
    assert_eq!(analysis::auto_neutral(&clear), None);
    let black = Rgba8 { width: 10, height: 10, data: (0..100).flat_map(|_| [0, 0, 0, 255]).collect() };
    assert_eq!(analysis::auto_neutral(&black), None);
}

#[test]
fn the_colour_wand_picks_the_cast() {
    let picked = analysis::auto_neutral(&with_cast(&grey_scene(12, 300, 200))).unwrap();
    // The picked colour has the cast's proportions, its brightest channel at 230.
    let ratio = |a: u8, b: u8| f64::from(a) / f64::from(b);
    assert_eq!(picked[0], 230, "{picked:?}");
    assert!((ratio(picked[1], picked[0]) - CAST[1]).abs() < 0.03, "{picked:?}");
    assert!((ratio(picked[2], picked[0]) - CAST[2]).abs() < 0.03, "{picked:?}");
}

#[test]
fn the_picker_averages_the_colour_under_the_point() {
    // A 400 × 300 photo with a 40 × 40 patch of #c08060 from (80, 80).
    let mut img = scene(14, 400, 300);
    for y in 80..120u32 {
        for x in 80..120u32 {
            let i = ((y * 400 + x) * 4) as usize;
            img.data[i..i + 3].copy_from_slice(&[0xc0, 0x80, 0x60]);
        }
    }
    assert_eq!(analysis::color_at(&img, Point { x: 100.5 / 400.0, y: 100.5 / 300.0 }), Some([0xc0, 0x80, 0x60]));
    // At the patch's corner the square mixes in the surroundings.
    assert_ne!(analysis::color_at(&img, Point { x: 80.0 / 400.0, y: 80.0 / 300.0 }), Some([0xc0, 0x80, 0x60]));
    // The corners are inside; anything else is not.
    assert!(analysis::color_at(&img, Point { x: 0.0, y: 0.0 }).is_some());
    assert!(analysis::color_at(&img, Point { x: 1.0, y: 1.0 }).is_some());
    for p in [(-0.01, 0.5), (0.5, 1.01), (f64::NAN, 0.5), (0.5, f64::INFINITY)] {
        assert_eq!(analysis::color_at(&img, Point { x: p.0, y: p.1 }), None, "{p:?}");
    }
    // Transparent pixels carry no colour.
    let clear = Rgba8 { width: 4, height: 4, data: (0..64).map(|i| if i % 4 == 3 { 0 } else { 200 }).collect() };
    assert_eq!(analysis::color_at(&clear, Point { x: 0.5, y: 0.5 }), None);
    assert_eq!(analysis::color_at(&Rgba8 { width: 0, height: 0, data: vec![] }, Point { x: 0.5, y: 0.5 }), None);
}

#[test]
fn the_fixes_take_no_parameters_and_have_labels_and_versions() {
    for (text, label, typed) in [
        ("autofix()", "Auto Fix", Typed::AutoFix(AutoFix)),
        ("autocontrast()", "Auto Contrast", Typed::AutoContrast(AutoContrast)),
        ("autocolor()", "Auto Color", Typed::AutoColor(AutoColor)),
    ] {
        let op = Op::parse(text);
        assert_eq!(op.typed(), Some(Ok(typed.clone())));
        assert_eq!(op.label(), Some(label));
        assert_eq!(op.version(), Some(1));
        assert_eq!(Op::from(typed).to_string(), text);
    }
    assert_eq!((AutoFix::VERSION, AutoContrast::VERSION, AutoColor::VERSION), (1, 1, 1));
    // Parameters are refused; the entry is kept and skipped.
    let img = scene(15, 60, 40);
    for bad in ["autofix(1)", "autocontrast(0.5)", "autocolor(x)"] {
        assert!(matches!(Op::parse(bad).typed(), Some(Err(_))), "{bad}");
        assert_eq!(recipe(&[bad]).print(), [bad]);
        assert_eq!(run(&img, &[bad]), img, "{bad}");
    }
    // A flat picture, a black one and a fully transparent one render without failing, alpha kept.
    for data in [vec![128u8; 64 * 4], (0..64).flat_map(|_| [0, 0, 0, 255]).collect(), vec![0; 64 * 4]] {
        let img = Rgba8 { width: 8, height: 8, data };
        for op in OPS {
            let out = run(&img, &[op]);
            assert_eq!((out.width, out.height), (8, 8));
            assert!(out.data.chunks(4).zip(img.data.chunks(4)).all(|(a, b)| a[3] == b[3]), "{op}: alpha kept");
        }
    }
}

#[test]
fn every_fix_matches_its_fingerprint() {
    // Blobs over a gradient (dull and tinted), and a grey photo with a planted cast.
    let up = common::rgba(&Scene::new(5, 300, 200).with_kind(Kind::Blobs).render().unwrap());
    let cast = with_cast(&grey_scene(6, 300, 200));
    for (case, img, op) in [
        ("auto-autofix", &up, "autofix()"),
        ("auto-autocontrast", &up, "autocontrast()"),
        ("auto-autocolor", &up, "autocolor()"),
        ("auto-autofix-cast", &cast, "autofix()"),
    ] {
        let small = render(Source::upright(img), &recipe(&[op]), Target::Fit { width: 64, height: 64 }).unwrap();
        check_fingerprint(case, &small, 0.5);
    }
}

#[test]
fn one_auto_fix_render_of_a_2560_pixel_preview_is_timed() {
    // Auto Fix on a 2560 × 1707 picture (`Target::Full`): the proxy render, the analysis and the
    // pass; printed for the PR (budgets come with M7.11).
    let img = scene(5, 2560, 1707);
    let time = |ops: &[&str]| {
        let r = recipe(ops);
        let mut best = f64::INFINITY;
        let mut out = render(Source::upright(&img), &r, Target::Full).unwrap();
        for _ in 0..5 {
            let t = Instant::now();
            out = render(Source::upright(&img), &r, Target::Full).unwrap();
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        (best, out)
    };
    let (base, _) = time(&[]);
    let (fixed, out) = time(&["autofix()"]);
    assert_eq!((out.width, out.height), (2560, 1707));
    assert_ne!(out, img);
    let t = Instant::now();
    let _ = analysis::auto_lighting(&img);
    let _ = analysis::auto_neutral(&img);
    let wands = t.elapsed().as_secs_f64() * 1000.0;
    println!("2560 × 1707: copy {base:.1} ms, with Auto Fix {fixed:.1} ms (Auto Fix ≈ {:.1} ms); both wands {wands:.1} ms", fixed - base);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn random_images_never_panic(
        w in 1u32..40,
        h in 1u32..40,
        seed in any::<u64>(),
        ops in proptest::sample::subsequence(vec!["autofix()", "autocontrast()", "autocolor()", "tuning(0.3,0,0,0.5,none)"], 1..4),
        at in any::<(f64, f64)>(),
    ) {
        // Bytes from a small generator, alpha included.
        let mut s = seed | 1;
        let data: Vec<u8> = (0..w * h * 4).map(|_| { s ^= s << 13; s ^= s >> 7; s ^= s << 17; (s >> 24) as u8 }).collect();
        let img = Rgba8 { width: w, height: h, data };
        let out = render(Source::upright(&img), &recipe(&ops), Target::Full).unwrap();
        prop_assert_eq!((out.width, out.height), (w, h));
        let thumb = render(Source::upright(&img), &recipe(&ops), Target::Fit { width: 7, height: 7 }).unwrap();
        prop_assert!(thumb.width <= 7 && thumb.height <= 7);
        let l = analysis::auto_lighting(&img);
        prop_assert!((0.0..=1.0).contains(&l.fill) && (0.0..=0.5).contains(&l.highlights) && (0.0..=0.5).contains(&l.shadows));
        let _ = analysis::auto_neutral(&img);
        let _ = analysis::color_at(&img, Point { x: at.0, y: at.1 });
        // A buffer that does not match its size is read as nothing, never indexed past its end.
        let short = Rgba8 { width: w + 3, height: h, data: img.data.clone() };
        prop_assert_eq!(analysis::auto_lighting(&short), Lighting::default());
        prop_assert_eq!(analysis::color_at(&short, Point { x: 0.5, y: 0.5 }), None);
    }
}
