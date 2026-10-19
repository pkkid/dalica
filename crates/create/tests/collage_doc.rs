//! The collage document, its page formats, its generator and its `.dalica-collage` recipe
//! (M14.4, `creating.md` §3).

use dalica_core::PhotoId;
use dalica_create::collage::page::{self, CURRENT_DISPLAY, FIXED, LONG_SIDE, METRIC_SIZES, US_SIZES};
use dalica_create::collage::{
    Background, Border, Collage, DocError, Format, MAX_CLIPS, Move, Orientation, Page, PhotoSizes, Recipe, RecipeError, Rng, Settings, Style,
    formats, layout,
};
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::common::cases;

fn ids(n: u64) -> Vec<PhotoId> {
    (1..=n).map(PhotoId::new).collect()
}

fn paths(c: &Collage) -> Vec<String> {
    c.clips.iter().map(|c| format!("/photos/trip/IMG_{:04}.jpg", c.id.get())).collect()
}

/// A collage with every field away from its default.
fn full_collage() -> Collage {
    let mut c = Collage::new(ids(6), 0x0123_4567_89ab_cdef);
    c.style = Style::FrameMosaic;
    c.settings = Settings { border: Border::Instant, spacing: 0.0137, shadows: false, captions: true };
    c.page = Page { format: Format::Ratio { name: "My ratio".into(), width: 7.5, height: 3.25 }, orientation: Orientation::Portrait };
    c.shuffle = 42;
    if let Some(clip) = c.clips.get_mut(2) {
        clip.on_page = false;
    }
    c.background = Background { colour: [0x12, 0xab, 0xef], picture: Some(PhotoId::new(4)) };
    c.frame_centre = Some(PhotoId::new(5));
    c.moves = vec![
        Move { id: PhotoId::new(1), centre: Some([0.25, 0.75]), scale: Some(1.5), angle: Some(-12.25), layer: 2 },
        Move { id: PhotoId::new(3), centre: None, scale: None, angle: None, layer: -1 },
    ];
    c.swaps = vec![[PhotoId::new(1), PhotoId::new(6)]];
    c
}

fn recipe_json(c: &Collage) -> Value {
    let r = Recipe::new(c.clone(), paths(c), false, Some((3000, 1688))).expect("a valid recipe");
    serde_json::from_slice(&r.print()).expect("printed JSON")
}

#[test]
fn a_document_round_trips_through_its_recipe_byte_for_byte() {
    for c in [full_collage(), Collage::new(ids(3), 7), Collage::default()] {
        let r = Recipe::new(c.clone(), paths(&c), true, Some((1688, 3000))).expect("a valid recipe");
        let bytes = r.print();
        let back = Recipe::parse(&bytes).expect("its own recipe reads back");
        assert_eq!(back.collage, c);
        assert_eq!(back.paths, paths(&c));
        assert!(back.draft);
        assert_eq!(back.output, Some((1688, 3000)));
        assert_eq!(back.print(), bytes, "printed again, byte for byte");
        let text = String::from_utf8(bytes).expect("UTF-8");
        assert!(text.ends_with("}\n") && !text.contains('\r'), "LF, a final newline");
    }
    // Sorted keys, the seeds as 16 hex digits, the colour as #rrggbb.
    let text = String::from_utf8(Recipe::new(full_collage(), paths(&full_collage()), false, None).expect("valid").print()).expect("UTF-8");
    let keys: Vec<&str> = text.lines().filter(|l| l.starts_with("  \"")).map(|l| l.trim().split('"').nth(1).unwrap_or("")).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted);
    assert!(text.contains("\"seed\": \"0123456789abcdef\""), "{text}");
    assert!(text.contains("\"colour\": \"#12abef\""), "{text}");
}

#[test]
fn unknown_keys_are_kept_and_printed_back() {
    let mut v = recipe_json(&full_collage());
    v["fromTheFuture"] = json!({"nested": [1, 2, 3]});
    v["clips"][0]["rating"] = json!(5);
    v["settings"]["glow"] = json!(true);
    let r = Recipe::parse(&serde_json::to_vec(&v).expect("JSON")).expect("reads with unknown keys");
    let mut edited = r.clone();
    edited.collage.scramble();
    let back: Value = serde_json::from_slice(&edited.print()).expect("JSON");
    assert_eq!(back["fromTheFuture"], json!({"nested": [1, 2, 3]}));
    assert_eq!(back["clips"][0]["rating"], json!(5));
    assert_eq!(back["settings"]["glow"], json!(true));
    assert_ne!(back["seed"], v["seed"], "the known fields are the document's");
}

#[test]
fn scramble_draws_a_new_seed_and_drops_the_users_placements() {
    let mut c = full_collage();
    let (seed, order) = (c.seed, c.assignment());
    c.scramble();
    assert_ne!(c.seed, seed);
    assert!(c.moves.is_empty() && c.swaps.is_empty());
    let unswapped = Collage { seed, moves: Vec::new(), swaps: Vec::new(), ..full_collage() }.assignment();
    assert_eq!(c.assignment(), unswapped, "the assignment of pictures is not Scramble's");
    assert_ne!(order, Vec::<usize>::new());
    // The same document scrambles the same way every time.
    let mut again = full_collage();
    again.scramble();
    assert_eq!(again.seed, c.seed);
}

#[test]
fn shuffle_keeps_the_slots_and_changes_the_assignment() {
    for n in [2, 3, 7, 30] {
        let mut c = Collage::new(ids(n), 99);
        c.style = Style::Grid;
        c.moves = vec![Move { id: PhotoId::new(1), centre: Some([0.5, 0.5]), scale: None, angle: None, layer: 0 }];
        c.swaps = vec![[PhotoId::new(1), PhotoId::new(2)]];
        let aspects = vec![1.5; n as usize];
        let before = layout(&c, &aspects);
        c.shuffle_pictures();
        let after = layout(&c, &aspects);
        let slots = |ps: &[dalica_create::collage::Placement]| {
            let mut s: Vec<String> = ps.iter().map(|p| format!("{:?} {:?} {}", p.centre, p.size, p.angle)).collect();
            s.sort();
            s
        };
        assert_eq!(slots(&before), slots(&after), "{n} pictures: the same slots");
        let who = |ps: &[dalica_create::collage::Placement]| ps.iter().map(|p| (format!("{:?}", p.centre), p.id)).collect::<Vec<_>>();
        assert_ne!(who(&before), who(&after), "{n} pictures: another assignment");
        assert!(c.swaps.is_empty(), "the swaps named the old slots");
        assert_eq!(c.moves.len(), 1, "a pile's moves stay with their pictures");
    }
    // One picture: nothing to change, nothing breaks.
    let mut one = Collage::new(ids(1), 1);
    one.shuffle_pictures();
    assert_eq!(one.assignment(), vec![0]);
}

#[test]
fn each_page_format_gives_its_ratio_in_both_orientations() {
    let custom = vec![("Panorama".to_string(), 3.0, 1.0), ("Broken".to_string(), f64::NAN, 1.0), ("Zero".to_string(), 0.0, 1.0)];
    for (sizes, table) in [(PhotoSizes::Us, US_SIZES), (PhotoSizes::Metric, METRIC_SIZES)] {
        let list = formats(sizes, &custom, Some((2560, 1440)));
        let names: Vec<&str> = list.iter().map(Format::name).collect();
        let mut expected: Vec<&str> = table.iter().chain(FIXED.iter()).map(|t| t.0).collect();
        expected.extend(["Panorama", CURRENT_DISPLAY]);
        assert_eq!(names, expected, "unusable custom ratios are left out");
        for format in list {
            let (w, h) = match format {
                Format::Ratio { width, height, .. } => (width, height),
                Format::Display { width, height } => (f64::from(width), f64::from(height)),
            };
            let long = w.max(h) / w.min(h);
            let landscape = Page { format: format.clone(), orientation: Orientation::Landscape };
            let portrait = Page { format: format.clone(), orientation: Orientation::Portrait };
            assert!((landscape.ratio() - long).abs() < 1e-12, "{}", format.name());
            assert!((portrait.ratio() - 1.0 / long).abs() < 1e-12, "{}", format.name());
            let (lw, lh) = landscape.pixel_size(LONG_SIDE);
            assert_eq!(lw, LONG_SIDE);
            assert!((f64::from(lw) / f64::from(lh) - long).abs() < long / f64::from(lh), "{}: {lw} × {lh}", format.name());
            assert_eq!(portrait.pixel_size(LONG_SIDE), (lh, lw));
        }
    }
    // Current Display writes the display's own size; any other format 3000 on the long side.
    let display = Page { format: Format::display(2560, 1440).expect("a display"), orientation: Orientation::Landscape };
    assert_eq!(display.output_size(), (2560, 1440));
    assert_eq!(Page { orientation: Orientation::Portrait, ..display }.output_size(), (1440, 2560));
    assert_eq!(Page::default().output_size(), (3000, 1688));
    assert_eq!(Page::default().format.name(), page::DEFAULT_FORMAT);
    assert!(Format::display(0, 1080).is_none() && Format::ratio("x", 1.0, f64::INFINITY).is_none());
}

#[test]
fn the_generator_gives_fixed_numbers_for_fixed_seeds() {
    // SplitMix64's published first outputs for seed 0.
    let mut rng = Rng::new(0);
    assert_eq!([rng.next_u64(), rng.next_u64(), rng.next_u64()], [0xe220_a839_7b1d_cdaf, 0x6e78_9e6a_a1b9_65f4, 0x06c4_5d18_8009_454f]);
    // Pinned once: what the layouts draw from must never change between systems or releases.
    let mut rng = Rng::new(1234);
    let below: Vec<u64> = (0..6).map(|_| rng.below(10)).collect();
    let mut order: Vec<u32> = (0..8).collect();
    Rng::new(1234).shuffle(&mut order);
    let unit = Rng::stream(1234, 1).unit();
    assert_eq!(below, PINNED_BELOW);
    assert_eq!(order, PINNED_ORDER);
    assert_eq!(unit.to_bits(), PINNED_UNIT);
    assert!((0.0..1.0).contains(&unit));
    assert_eq!(Rng::new(5).below(0), 0);
}

const PINNED_BELOW: [u64; 6] = [7, 5, 2, 3, 7, 6];
const PINNED_ORDER: [u32; 8] = [6, 0, 3, 2, 7, 1, 4, 5];
const PINNED_UNIT: u64 = 4_596_593_500_016_158_148;

#[test]
fn the_layout_applies_a_piles_moves_and_the_layers() {
    let mut c = Collage::new(ids(4), 3);
    let aspects = [1.5, 0.75, 1.0, 2.0];
    let plain = layout(&c, &aspects);
    assert_eq!(plain.len(), 4);
    assert_eq!(plain.iter().map(|p| p.z).collect::<Vec<_>>(), [0, 1, 2, 3]);
    c.moves = vec![Move { id: PhotoId::new(1), centre: Some([0.1, 0.9]), scale: Some(2.0), angle: Some(30.0), layer: 1 }];
    let moved = layout(&c, &aspects);
    let top = moved.last().expect("four placements");
    assert_eq!(top.id, PhotoId::new(1), "Bring to Top puts it last");
    assert_eq!(top.centre, [0.1, 0.9]);
    assert_eq!(top.angle, 30.0);
    let first = plain.first().expect("four placements");
    assert_eq!(top.size, first.size.map(|v| v * 2.0));
    // Off the page: left out of the layout, kept in the Clips.
    if let Some(clip) = c.clips.get_mut(1) {
        clip.on_page = false;
    }
    assert!(layout(&c, &aspects).iter().all(|p| p.id != PhotoId::new(2)));
    assert_eq!(c.clips.len(), 4);
    // Hostile aspect ratios lay out finite.
    for p in layout(&c, &[f64::NAN, -1.0, 0.0, f64::INFINITY]) {
        assert!(p.centre.iter().chain(&p.size).all(|v| v.is_finite()));
    }
}

#[test]
fn hostile_recipes_are_refused() {
    let base = recipe_json(&full_collage());
    let parse = |v: &Value| Recipe::parse(&serde_json::to_vec(v).expect("JSON"));

    // Not a number: JSON has none, and a number past f64 is refused by the reader.
    let text = String::from_utf8(serde_json::to_vec(&base).expect("JSON")).expect("UTF-8");
    for bad in ["NaN", "Infinity", "1e999"] {
        let hostile =
            text.replacen("\"angle\": -12.25", &format!("\"angle\": {bad}"), 1).replacen("\"angle\":-12.25", &format!("\"angle\":{bad}"), 1);
        assert_ne!(hostile, text);
        assert!(Recipe::parse(hostile.as_bytes()).is_err(), "{bad}");
    }
    // A number out of range.
    let mut v = base.clone();
    v["settings"]["spacing"] = json!(0.5);
    assert!(matches!(parse(&v), Err(RecipeError::Doc(DocError::Range(_)))));
    v = base.clone();
    v["moves"][0]["scale"] = json!(1e300);
    assert!(matches!(parse(&v), Err(RecipeError::Doc(DocError::Range(_)))));
    // A huge clip list.
    v = base.clone();
    v["clips"] = Value::Array((0..=MAX_CLIPS as u64).map(|i| json!({"id": i, "path": "", "onPage": true})).collect());
    assert!(matches!(parse(&v), Err(RecipeError::Limit(_))));
    // Depth 100.
    v = base.clone();
    let mut deep = json!(0);
    for _ in 0..100 {
        deep = json!([deep]);
    }
    v["deep"] = deep;
    assert!(matches!(parse(&v), Err(RecipeError::Limit(_))));
    // A clip id twice.
    v = base.clone();
    v["clips"][1]["id"] = v["clips"][0]["id"].clone();
    assert!(matches!(parse(&v), Err(RecipeError::Doc(DocError::Duplicate(_)))));
    // A frame centre, a background or a moved picture not in the clips.
    for key in [&["frameCentre"][..], &["background", "picture"], &["moves", "0", "id"]] {
        v = base.clone();
        let mut at = &mut v;
        for k in key {
            at = match k.parse::<usize>() {
                Ok(i) => &mut at[i],
                Err(_) => &mut at[*k],
            };
        }
        *at = json!(999);
        assert!(matches!(parse(&v), Err(RecipeError::Doc(DocError::NotAClip { .. }))), "{key:?}");
    }
    // Over 1 MiB, not JSON, another shape, a newer version, a bad seed or colour.
    assert!(matches!(Recipe::parse(&vec![b' '; (1 << 20) + 1]), Err(RecipeError::TooLarge(_))));
    assert!(matches!(Recipe::parse(b"{"), Err(RecipeError::Json(_))));
    assert!(matches!(Recipe::parse(b"[]"), Err(RecipeError::Shape(_))));
    v = base.clone();
    v["dalica"]["version"] = json!(2);
    assert!(matches!(parse(&v), Err(RecipeError::Newer(2))));
    for (key, bad) in [("seed", json!("xyz")), ("seed", json!(12)), ("style", json!("collage"))] {
        v = base.clone();
        v[key] = bad;
        assert!(matches!(parse(&v), Err(RecipeError::Shape(_))), "{key}");
    }
    v = base.clone();
    v["background"]["colour"] = json!("#12345");
    assert!(matches!(parse(&v), Err(RecipeError::Shape(_))));
    // A document made in code is checked too.
    let mut c = Collage::new(ids(2), 0);
    c.frame_centre = Some(PhotoId::new(9));
    assert!(Recipe::new(c, vec![String::new(); 2], false, None).is_err());
    assert!(Recipe::new(Collage::new(ids(2), 0), vec![String::new()], false, None).is_err(), "a path per clip");
}

/// A recipe text with one of its bytes or values changed at random.
fn mutated() -> impl Strategy<Value = Vec<u8>> {
    let base = Recipe::new(full_collage(), paths(&full_collage()), false, Some((3000, 1688))).expect("valid").print();
    (any::<usize>(), any::<u8>(), 0usize..4).prop_map(move |(at, byte, how)| {
        let mut b = base.clone();
        let i = at % b.len().max(1);
        match how {
            0 => b[i] = byte,
            1 => b.truncate(i),
            2 => b.insert(i, byte),
            _ => b = b.repeat(1 + byte as usize % 3),
        }
        b
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(512)))]

    #[test]
    fn no_recipe_panics(bytes in prop_oneof![proptest::collection::vec(any::<u8>(), 0..256), mutated()]) {
        if let Ok(r) = Recipe::parse(&bytes) {
            // Anything accepted is a valid document that prints and reads back the same.
            prop_assert!(r.collage.check().is_ok());
            let again = Recipe::parse(&r.print()).expect("an accepted recipe reads back");
            prop_assert_eq!(&again.collage, &r.collage);
            let n = layout(&again.collage, &[]).len();
            prop_assert!(n <= MAX_CLIPS);
        }
    }

    #[test]
    fn scramble_and_shuffle_are_pure(seed in any::<u64>(), shuffle in any::<u64>(), n in 0u64..40) {
        let mut a = Collage::new(ids(n), seed);
        a.shuffle = shuffle;
        let mut b = a.clone();
        a.scramble();
        a.shuffle_pictures();
        b.scramble();
        b.shuffle_pictures();
        prop_assert_eq!(&a, &b);
        prop_assert!(a.check().is_ok());
        let mut order = a.assignment();
        order.sort_unstable();
        prop_assert_eq!(order, (0..n as usize).collect::<Vec<_>>());
    }
}
