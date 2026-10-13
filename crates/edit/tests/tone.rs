//! The Tuning operation (`editing.md` §6.5, §7; `data-model.md` §3.1): the defaults change nothing;
//! Fill Light never darkens and lifts the dark most; Highlights and Shadows keep to their side of
//! the middle; Color Temperature moves the red/blue balance monotonically; the Neutral colour of a
//! cast photo becomes grey; `fill_light(a)` renders as Tuning with only `fill` and stays as
//! written; print and parse round-trip and hostile values clamp. Fingerprints:
//! `fingerprints/tone-*.json`, one per control at three values, each checked at two sizes.

mod common;

use std::time::Instant;

use common::{assert_two_sizes_agree, check_fingerprint, recipe, scene};
use dalica_edit::ops::Operation;
use dalica_edit::ops::tone::{FillLight, Tuning, luma};
use dalica_edit::{Op, Source, Target, Typed, render};
use dalica_image::Rgba8;
use dalica_scenes::{Kind, Scene};
use proptest::prelude::*;

fn run(img: &Rgba8, ops: &[&str]) -> Rgba8 {
    render(Source::upright(img), &recipe(ops), Target::Full).unwrap()
}

/// Every grey level, four rows of 256.
fn grey_ramp() -> Rgba8 {
    let data = (0..4).flat_map(|_| (0..=255u8).flat_map(|v| [v, v, v, 255])).collect();
    Rgba8 { width: 256, height: 4, data }
}

fn pixels(img: &Rgba8) -> impl Iterator<Item = [u8; 3]> + '_ {
    img.data.chunks(4).map(|p| [p[0], p[1], p[2]])
}

fn luma8(p: [u8; 3]) -> f32 {
    luma(p.map(|v| f32::from(v) / 255.0))
}

/// The grey level `v` of a rendered ramp.
fn ramp_at(img: &Rgba8, v: u8) -> u8 {
    img.data[usize::from(v) * 4]
}

#[test]
fn the_defaults_are_the_identity() {
    let img = scene(3, 120, 80);
    assert_eq!(Tuning::default().to_string(), "0,0,0,0,none");
    assert_eq!(FillLight::default().to_string(), "0");
    for ops in [&["tuning(0,0,0,0,none)"][..], &["fill_light(0)"], &["tuning(0,0,0,0,#808080)"], &["tuning(-0,0,0,-0,none)"]] {
        assert_eq!(run(&img, ops), img, "{ops:?}");
        assert_eq!(run(&grey_ramp(), ops), grey_ramp(), "{ops:?}");
    }
}

#[test]
fn fill_light_never_darkens_and_lifts_the_dark_most() {
    let img = scene(4, 160, 120);
    let ramp = grey_ramp();
    let mut last_mid = 0;
    for a in ["0.25", "0.5", "1"] {
        let tuning = format!("tuning({a},0,0,0,none)");
        let fill = format!("fill_light({a})");
        for src in [&img, &ramp] {
            let out = run(src, &[&tuning]);
            assert_eq!(run(src, &[&fill]), out, "{fill} renders as {tuning}");
            for (i, (before, after)) in pixels(src).zip(pixels(&out)).enumerate() {
                assert!((0..3).all(|c| after[c] >= before[c]), "{tuning}: pixel {i} darkened: {before:?} → {after:?}");
            }
        }
        let out = run(&ramp, &[&tuning]);
        assert_eq!((ramp_at(&out, 0), ramp_at(&out, 255)), (0, 255), "black and white stay");
        // The lower half is lifted more than the upper half, in absolute terms and relative to the
        // pixel; the relative lift falls as the pixel gets brighter.
        let lift = |range: std::ops::Range<u16>| range.map(|v| f64::from(ramp_at(&out, v as u8)) - f64::from(v)).sum::<f64>();
        assert!(lift(1..128) > 2.0 * lift(128..255), "{tuning}");
        let gain = |v: u8| f64::from(ramp_at(&out, v)) / f64::from(v);
        for (dark, bright) in [(16, 64), (64, 128), (128, 192), (192, 240)] {
            assert!(gain(dark) > gain(bright), "{tuning}: {dark} gains {} and {bright} {}", gain(dark), gain(bright));
        }
        // More fill, more light.
        assert!(ramp_at(&out, 64) > last_mid, "{tuning}");
        last_mid = ramp_at(&out, 64);
    }
}

#[test]
fn highlights_and_shadows_keep_to_their_side_of_the_middle() {
    let img = scene(5, 160, 120);
    let ramp = grey_ramp();
    for v in ["0.1", "0.25", "0.5"] {
        for (op, brighter) in [(format!("tuning(0,{v},0,0,none)"), true), (format!("tuning(0,0,{v},0,none)"), false)] {
            for src in [&img, &ramp] {
                let out = run(src, &[&op]);
                for (before, after) in pixels(src).zip(pixels(&out)) {
                    let l = luma8(before);
                    let untouched = if brighter { l < 0.498 } else { l > 0.502 };
                    if untouched {
                        assert_eq!(before, after, "{op}: luma {l}");
                    } else if brighter {
                        assert!((0..3).all(|c| after[c] >= before[c]), "{op}: {before:?} → {after:?}");
                    } else {
                        assert!((0..3).all(|c| after[c] <= before[c]), "{op}: {before:?} → {after:?}");
                    }
                }
            }
            let out = run(&ramp, &[&op]);
            assert_eq!((ramp_at(&out, 0), ramp_at(&out, 255)), (0, 255), "{op}: black and white stay");
            if brighter {
                assert!(ramp_at(&out, 192) > 192 + 2, "{op}");
            } else {
                assert!(ramp_at(&out, 64) < 64 - 2, "{op}");
            }
        }
    }
}

#[test]
fn temperature_moves_the_red_blue_balance_monotonically() {
    let img = scene(6, 160, 120);
    let balance = |out: &Rgba8| {
        let (r, b) = pixels(out).fold((0u64, 0u64), |(r, b), p| (r + u64::from(p[0]), b + u64::from(p[2])));
        r as f64 / b as f64
    };
    let mut last = 0.0;
    for t in ["-1", "-0.5", "-0.1", "0", "0.1", "0.5", "1"] {
        let out = run(&img, &[&format!("tuning(0,0,0,{t},none)")]);
        let now = balance(&out);
        assert!(now > last, "temp {t}: red/blue {now} after {last}");
        last = now;
        // A mid grey keeps its brightness.
        let grey = run(&grey_ramp(), &[&format!("tuning(0,0,0,{t},none)")]);
        let p = pixels(&grey).nth(128).unwrap();
        assert!((luma8(p) - luma8([128; 3])).abs() <= 1.5 / 255.0, "temp {t}: {p:?}");
        if t.starts_with('-') {
            assert!(p[2] > p[0], "cooler is bluer: {p:?}");
        } else if t != "0" {
            assert!(p[0] > p[2], "warmer is redder: {p:?}");
        }
    }
}

#[test]
fn the_neutral_colour_of_a_cast_photo_becomes_grey() {
    // A grey card on a `scenes` photo, then a warm cast over the whole.
    let mut clean = scene(9, 160, 120);
    for y in 50..70 {
        for x in 70..90 {
            let i = ((y * 160 + x) * 4) as usize;
            clean.data[i..i + 3].copy_from_slice(&[140; 3]);
        }
    }
    let at = 60 * 160 + 80;
    let cast = [1.0, 0.85, 0.65];
    let mut warm = clean.clone();
    for p in warm.data.chunks_mut(4) {
        for c in 0..3 {
            p[c] = (f64::from(p[c]) * cast[c]).round() as u8;
        }
    }
    let picked = pixels(&warm).nth(at).unwrap();
    let op = format!("tuning(0,0,0,0,#{:02x}{:02x}{:02x})", picked[0], picked[1], picked[2]);
    assert_eq!(Tuning::parse(&op[7..op.len() - 1]).unwrap().neutral, Some(picked));
    let out = run(&warm, &[&op]);
    let p = pixels(&out).nth(at).unwrap();
    assert!(p.iter().max().unwrap() - p.iter().min().unwrap() <= 2, "{op}: {picked:?} → {p:?}");
    // The whole photo's balance comes back close to the clean one's.
    let rb = |img: &Rgba8| {
        let (r, b) = pixels(img).fold((0u64, 0u64), |(r, b), p| (r + u64::from(p[0]), b + u64::from(p[2])));
        r as f64 / b as f64
    };
    let (clean_rb, warm_rb, fixed_rb) = (rb(&clean), rb(&warm), rb(&out));
    assert!((fixed_rb - clean_rb).abs() < 0.1 * (warm_rb - clean_rb).abs(), "red/blue: clean {clean_rb}, cast {warm_rb}, fixed {fixed_rb}");
}

#[test]
fn fill_light_is_read_as_tuning_with_only_fill_and_kept_as_written() {
    let op = Op::parse("fill_light(0.25)");
    assert_eq!(op.typed(), Some(Ok(Typed::FillLight(FillLight { amount: 0.25 }))));
    assert_eq!(FillLight { amount: 0.25 }.as_tuning(), Tuning { fill: 0.25, ..Tuning::default() });
    let r = recipe(&["fill_light(0.25)", "fill_light(.5)"]);
    assert_eq!(r.print(), ["fill_light(0.25)", "fill_light(.5)"], "kept as written");
    let img = scene(10, 90, 60);
    assert_eq!(run(&img, &["fill_light(.5)"]), run(&img, &["tuning(0.5,0,0,0,none)"]));
    // What Dalica writes is `tuning`.
    let t = Typed::Tuning(Tuning { fill: 0.25, ..Tuning::default() });
    assert_eq!(Op::from(t).to_string(), "tuning(0.25,0,0,0,none)");
    assert_eq!(Op::parse("tuning(1,0,0,0,none)").label(), Some("Tuning"));
    assert_eq!(Op::parse("fill_light(1)").version(), Some(1));
    assert_eq!(Op::parse("tuning(1,0,0,0,none)").version(), Some(1));
}

#[test]
fn print_and_parse_round_trip_and_hostile_values_clamp() {
    for text in ["0,0,0,0,none", "0.2,0.1,0.05,-0.3,#808080", "1,0.5,0.5,1,#ffffff", "0.333,0.01,0.49,-1,#0a0b0c"] {
        assert_eq!(Tuning::parse(text).unwrap().to_string(), text);
    }
    let t = |fill, highlights, shadows, temp, neutral| Tuning { fill, highlights, shadows, temp, neutral };
    for (text, expected) in [
        ("2,1,-1,5,#FFFFFF", t(1.0, 0.5, 0.0, 1.0, Some([255; 3]))),
        ("inf,-inf,1e308,-1e308,none", t(1.0, 0.0, 0.5, -1.0, None)),
        (" 0.5 , 0.1,0.2 ,-0.25, #A0b0C0 ", t(0.5, 0.1, 0.2, -0.25, Some([0xa0, 0xb0, 0xc0]))),
        ("-0,0,0,-1e-400,none", t(0.0, 0.0, 0.0, 0.0, None)),
    ] {
        let parsed = Tuning::parse(text).unwrap();
        assert_eq!(parsed, expected, "{text}");
        assert_eq!(Tuning::parse(&parsed.to_string()).unwrap(), parsed, "{text}");
        assert!(!parsed.to_string().contains("-0,"), "{text}: {parsed}");
    }
    for text in [
        "",
        "0,0,0,0",
        "0,0,0,0,none,0",
        "NaN,0,0,0,none",
        "0,0,0,nan,none",
        "a,0,0,0,none",
        "0,0,0,0,#12345",
        "0,0,0,0,#1234567",
        "0,0,0,0,#gggggg",
        "0,0,0,0,#+1+2+3",
        "0,0,0,0,#éééé",
        "0,0,0,0,808080",
        "0,0,0,0,None",
    ] {
        assert!(Tuning::parse(text).is_err(), "{text:?}");
    }
    assert_eq!(FillLight::parse("-1").unwrap(), FillLight { amount: 0.0 });
    assert_eq!(FillLight::parse("7").unwrap(), FillLight { amount: 1.0 });
    for text in ["", "NaN", "0.5,0", "x"] {
        assert!(FillLight::parse(text).is_err(), "{text:?}");
    }
    // Typed values out of range are clamped too (`clamped`, and when rendered).
    let wild = t(f64::NAN, 9.0, -9.0, f64::INFINITY, None);
    assert_eq!(wild.clamped(), t(0.0, 0.5, 0.0, 1.0, None));
    assert_eq!(FillLight { amount: f64::NEG_INFINITY }.clamped(), FillLight { amount: 0.0 });
    // A refused entry is kept and skipped.
    let img = scene(12, 40, 30);
    for bad in ["tuning(NaN,0,0,0,none)", "tuning(1,0,0,0,#xyz)", "fill_light(nan)"] {
        assert_eq!(recipe(&[bad]).print(), [bad]);
        assert_eq!(run(&img, &[bad]), img, "{bad}");
    }
    // Clamped on reading: a hostile but readable entry renders as its clamped value.
    assert_eq!(run(&img, &["tuning(1e308,9,-9,-inf,none)"]), run(&img, &["tuning(1,0.5,0,-1,none)"]));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn typed_values_print_and_read_back(
        fill in 0.0..=1.0f64,
        highlights in 0.0..=0.5f64,
        shadows in 0.0..=0.5f64,
        temp in -1.0..=1.0f64,
        neutral in proptest::option::of(any::<[u8; 3]>()),
    ) {
        let t = Tuning { fill, highlights, shadows, temp, neutral };
        prop_assert_eq!(Tuning::parse(&t.to_string()).unwrap(), t);
        let f = FillLight { amount: fill };
        prop_assert_eq!(FillLight::parse(&f.to_string()).unwrap(), f);
    }

    #[test]
    fn any_text_reads_in_range_or_is_refused(text in "[-0-9a-fA-FnoeiNI.,# ]{0,40}") {
        if let Ok(t) = Tuning::parse(&text) {
            prop_assert!((0.0..=1.0).contains(&t.fill) && (0.0..=0.5).contains(&t.highlights));
            prop_assert!((0.0..=0.5).contains(&t.shadows) && (-1.0..=1.0).contains(&t.temp));
        }
        if let Ok(f) = FillLight::parse(&text) {
            prop_assert!((0.0..=1.0).contains(&f.amount));
        }
    }
}

#[test]
fn every_control_at_three_values_agrees_at_two_sizes_and_matches_its_fingerprint() {
    // Blobs over a gradient with about a third of the picture dark and a third bright, so every
    // control has something to move.
    let up = common::rgba(&Scene::new(5, 300, 200).with_kind(Kind::Blobs).render().unwrap());
    let cases = [
        ("tone-fill-0.25", "tuning(0.25,0,0,0,none)"),
        ("tone-fill-0.5", "tuning(0.5,0,0,0,none)"),
        ("tone-fill-1", "tuning(1,0,0,0,none)"),
        ("tone-highlights-0.1", "tuning(0,0.1,0,0,none)"),
        ("tone-highlights-0.25", "tuning(0,0.25,0,0,none)"),
        ("tone-highlights-0.5", "tuning(0,0.5,0,0,none)"),
        ("tone-shadows-0.1", "tuning(0,0,0.1,0,none)"),
        ("tone-shadows-0.25", "tuning(0,0,0.25,0,none)"),
        ("tone-shadows-0.5", "tuning(0,0,0.5,0,none)"),
        ("tone-temp-cool-1", "tuning(0,0,0,-1,none)"),
        ("tone-temp-warm-0.5", "tuning(0,0,0,0.5,none)"),
        ("tone-temp-warm-1", "tuning(0,0,0,1,none)"),
        ("tone-neutral-c8a080", "tuning(0,0,0,0,#c8a080)"),
        ("tone-neutral-8090b0", "tuning(0,0,0,0,#8090b0)"),
        ("tone-neutral-a0b090", "tuning(0,0,0,0,#a0b090)"),
    ];
    for (case, op) in cases {
        let small = assert_two_sizes_agree(Source::upright(&up), &recipe(&[op]), 64, 240, 2.0);
        check_fingerprint(case, &small, 0.5);
    }
}

#[test]
fn one_tuning_render_of_a_2560_pixel_preview_is_timed() {
    // The tone stage alone on a 2560 × 1707 picture (no resampling: `Target::Full`), every
    // control on; printed for the PR (budgets come with M7.11).
    let img = scene(5, 2560, 1707);
    let all = recipe(&["tuning(0.4,0.2,0.2,0.3,#c8b4a0)"]);
    let none = recipe(&[]);
    // The best of five runs of each, after a warm-up.
    let time = |r| {
        let mut best = f64::INFINITY;
        let mut out = render(Source::upright(&img), r, Target::Full).unwrap();
        for _ in 0..5 {
            let t = Instant::now();
            out = render(Source::upright(&img), r, Target::Full).unwrap();
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        (best, out)
    };
    let (base, _) = time(&none);
    let (tuned, out) = time(&all);
    assert_eq!((out.width, out.height), (2560, 1707));
    assert_ne!(out, img);
    println!("2560 × 1707: copy {base:.1} ms, with Tuning {tuned:.1} ms (Tuning ≈ {:.1} ms)", tuned - base);
}
