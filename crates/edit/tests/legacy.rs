//! Legacy edit operations mapped to Dalica's (`data-model.md` §9, `editing.md` §13): every mapped
//! operation parses back typed and renders; legacy ends land on Dalica's ends; unknown names, wrong
//! counts and markers are kept verbatim in their place; only the last crop is carried; a crop on
//! the stored pixels covers the same pixels under each of the eight EXIF orientations; the library
//! rotation, flip, crop key and text overlay; the redo list; the limit; no token list panics.

use crate::common::{cases, exif, scene, stored};
use dalica_edit::legacy::{self, Converted, Legacy, kept, map, rect64, tokens};
use dalica_edit::ops::crop::Crop;
use dalica_edit::{Dihedral, Error, MAX_OPERATIONS, Op, Recipe, Rect, Source, Target, Typed, render};
use dalica_image::{Orientation, Rgba8};
use proptest::prelude::*;

/// Every legacy operation the module maps, at the legacy defaults.
const MAPPED: [&str; 18] = [
    "crop64=1,1c000934f322d3f8",
    "enhance=1",
    "autolight=1",
    "autocolor=1",
    "fill=1,0.186916",
    "finetune2=1,0.200000,0.100000,0.050000,00b0a090,0.300000",
    "unsharp2=1,0.600000",
    "sepia=1",
    "bw=1",
    "warm=1",
    "grain2=1",
    "sat=1,0.161800",
    "glow2=1,0.650000,3.000000",
    "ansel=1,ffffff00",
    "Boost=1,50.000000",
    "HDR=1,20.000000,3.000000,0.000000",
    "Holga=1,70.000000,30.000000,0.000000",
    "TwoTone=1,0,20,0,00004488,00ffff00",
];

/// More mapped operations: the two whose sample is not at its defaults alone.
const MAPPED_MORE: [&str; 2] = ["Vignette=1,35.000000,1.400000,0.000000,00000000", "Border=1,20,5,0,00000000,00ffffff,0"];

fn up() -> Orientation {
    Orientation::default()
}

fn converted(filters: &[&str]) -> Converted {
    let legacy = Legacy { filters: filters.iter().map(|s| s.to_string()).collect(), ..Legacy::default() };
    legacy::convert(&legacy).unwrap()
}

fn texts(ops: &[Op]) -> Vec<String> {
    ops.iter().map(Op::to_string).collect()
}

#[test]
fn every_mapped_operation_parses_back_typed_and_renders() {
    let photo = scene(7, 48, 32);
    for token in MAPPED.iter().chain(&MAPPED_MORE) {
        let op = map(token, up());
        let text = op.to_string();
        assert!(!text.starts_with("legacy:"), "{token} → {text}");
        let back = Op::parse(&text);
        assert_eq!(back, op, "{token}");
        assert!(matches!(back.typed(), Some(Ok(_))), "{token} → {text} is not typed");
        let recipe = Recipe::parse([text.as_str()]).unwrap();
        let out = render(Source::upright(&photo), &recipe, Target::Full).unwrap();
        assert!(out.width > 0 && out.height > 0, "{token}");
    }
}

#[test]
fn parameters_at_their_legacy_ends_land_at_the_dalica_ends() {
    let cases = [
        ("fill=1,0.000000", "tuning(0,0,0,0,none)"),
        ("fill=1,1.000000", "tuning(1,0,0,0,none)"),
        ("finetune2=1,0,0,0,00000000,-1", "tuning(0,0,0,-1,none)"),
        ("finetune2=1,1,0.48,0.48,00ff8000,1", "tuning(1,0.5,0.5,1,#ff8000)"),
        ("unsharp2=1,0", "sharpen(0)"),
        ("unsharp2=1,3", "sharpen(3)"),
        ("sat=1,-1", "saturation(-1)"),
        ("sat=1,1", "saturation(1)"),
        ("glow2=1,0,0.3", "glow(0,0.3)"),
        ("glow2=1,1,30", "glow(1,30)"),
        ("ansel=1,ff0080ff", "filtered_bw(#0080ff)"),
        ("Boost=1,0", "boost(0)"),
        ("Boost=1,100", "boost(100)"),
        ("HDR=1,1.3,1,0", "hdr(1.3,1,0)"),
        ("HDR=1,80,7,100", "hdr(80,7,100)"),
        ("Holga=1,0,0,0", "plastic_lens(0,0,0)"),
        ("Holga=1,100,100,100", "plastic_lens(100,100,100)"),
        ("TwoTone=1,-95,0,0,0,ffffff", "duotone(#000000,#ffffff,-95,0,0)"),
        ("TwoTone=1,95,100,100,00004488,00ffff00", "duotone(#004488,#ffff00,95,100,100)"),
        ("Vignette=1,0,1,0,00ffffff", "vignette(0,1,#ffffff,0)"),
        ("Vignette=1,50,2,100,00000000", "vignette(50,2,#000000,100)"),
        ("Border=1,0,0,0,0,0,0", "border(#000000,0,#000000,0,0,0)"),
        ("Border=1,100,100,0,00ff0000,0000ff00,0", "border(#ff0000,100,#00ff00,100,0,0)"),
        // Past an end: clamped onto it.
        ("finetune2=1,5,9,-3,00000000,-7", "tuning(1,0.5,0,-1,none)"),
        ("Boost=1,1000", "boost(100)"),
    ];
    for (token, want) in cases {
        assert_eq!(map(token, up()).to_string(), want, "{token}");
    }
}

#[test]
fn unknown_names_wrong_counts_and_markers_keep_their_text_and_place() {
    let filters = [
        "redeye=1",
        "autolight=1",
        "Sepia=1",
        "grain2=1,0.5",
        "retouch=1",
        "bw=1",
        "tilt=1,0.565155,0.000000",
        "radblur=1,0.5,0.5,0.239766,0.146199",
        "Lomo=1,50.000000,0.000000",
        "sat=1,nan",
        "sat=0,0.5",
        "Border=1,20,5,10,00000000,00ffffff,0",
        "garbage",
        "warm=1",
    ];
    let c = converted(&filters);
    assert_eq!(
        texts(&c.edits),
        [
            "legacy:redeye(1)",
            "autocontrast()",
            "legacy:Sepia(1)",
            "legacy:grain2(1,0.5)",
            "legacy:retouch(1)",
            "bw()",
            "legacy:tilt(1,0.565155,0.000000)",
            "legacy:radblur(1,0.5,0.5,0.239766,0.146199)",
            "legacy:Lomo(1,50.000000,0.000000)",
            "legacy:sat(1,nan)",
            "legacy:sat(0,0.5)",
            "legacy:Border(1,20,5,10,00000000,00ffffff,0)",
            "legacy:garbage",
            "warmify()",
        ]
    );
    // Each kept entry is a legacy entry Dalica skips, and the original token comes back from it.
    for (op, token) in c.edits.iter().zip(filters) {
        if let Op::Legacy(text) = op {
            let inner = text.strip_prefix("legacy:").unwrap();
            let back = inner.strip_suffix(')').map_or(inner.to_owned(), |s| s.replacen('(', "=", 1));
            assert_eq!(back, token);
        }
    }
    // A trailing empty field is tolerated.
    assert_eq!(map("unsharp2=1,0.6,", up()).to_string(), "sharpen(0.6)");
    assert_eq!(kept("x=1,(a;b)").to_string(), "legacy:x(1,(a;b))");
}

#[test]
fn two_crops_give_the_last_one_in_its_place() {
    let c = converted(&["crop64=1,0000000080008000", "bw=1", "crop64=1,c0008000ffffffff", "sepia=1"]);
    let edits = texts(&c.edits);
    assert_eq!(edits.len(), 3);
    assert_eq!(edits[0], "bw()");
    assert_eq!(edits[2], "sepia()");
    let Rect { x0, y0, x1, y1 } = rect64("c0008000ffffffff").unwrap();
    assert_eq!(edits[1], Op::from(Typed::Crop(Crop::new(Rect::new(x0, y0, x1, y1)).unwrap())).to_string());
    // The crop key is used only when the edit list carries no crop.
    let key = Rect::new(0.25, 0.25, 0.75, 0.75);
    let legacy = Legacy { filters: vec!["crop64=1,0000000080008000".into()], crop: Some(key), ..Legacy::default() };
    assert_eq!(legacy::convert(&legacy).unwrap().edits, [map("crop64=1,0000000080008000", up())]);
    let legacy = Legacy { filters: vec!["bw=1".into()], crop: Some(key), ..Legacy::default() };
    assert_eq!(texts(&legacy::convert(&legacy).unwrap().edits), ["crop(0.25,0.25,0.75,0.75)", "bw()"]);
    let legacy = Legacy { crop: Some(Rect::new(0.5, 0.5, 0.5, 0.9)), ..Legacy::default() };
    assert_eq!(texts(&legacy::convert(&legacy).unwrap().edits), ["legacy:crop(0.5,0.5,0.5,0.9)"]);
}

#[test]
fn short_rectangles_are_padded_and_a_full_one_is_the_whole_picture() {
    assert_eq!(rect64("ffffffff"), Some(Rect::new(0.0, 0.0, 1.0, 1.0)));
    assert_eq!(rect64("rect64(ffffffff)"), Some(Rect::new(0.0, 0.0, 1.0, 1.0)));
    assert_eq!(rect64("0000ffffffffffff"), Some(Rect::new(0.0, 1.0, 1.0, 1.0)));
    for bad in ["", "rect64()", "1ffffffffffffffff", "xyz", "rect64(ff"] {
        assert_eq!(rect64(bad), None, "{bad}");
    }
    assert_eq!(tokens("autolight=1;;enhance=1;"), ["autolight=1", "enhance=1"]);
}

/// The window `(x0, y0)–(x1, y1)` of `img`, in pixels.
fn cut(img: &Rgba8, x0: u32, y0: u32, x1: u32, y1: u32) -> Rgba8 {
    let mut data = Vec::new();
    for y in y0..y1 {
        let at = ((y * img.width + x0) * 4) as usize;
        data.extend_from_slice(&img.data[at..at + ((x1 - x0) * 4) as usize]);
    }
    Rgba8 { width: x1 - x0, height: y1 - y0, data }
}

#[test]
fn a_crop_covers_the_legacy_rectangle_under_every_orientation() {
    // 85 and 51 divide 65535, so the rectangle's fields are whole pixels on either side.
    let upright = scene(11, 85, 51);
    for o in 1..=8u8 {
        let file = stored(&upright, o);
        let (w, h) = (file.width, file.height);
        let (px0, py0, px1, py1) = (w / 5, h / 4, w * 3 / 5, h * 2 / 3 + 1);
        let field = |p: u32, size: u32| u64::from(p) * 65535 / u64::from(size);
        let hex = format!("{:04x}{:04x}{:04x}{:04x}", field(px0, w), field(py0, h), field(px1, w), field(py1, h));
        let c = converted_with(&[&format!("crop64=1,{hex}")], o);
        let recipe = Recipe::parse(c.edits_text()).unwrap();
        let got = render(Source::new(&file, exif(o)), &recipe, Target::Full).unwrap();
        let want = Dihedral::from_exif(exif(o)).apply(cut(&file, px0, py0, px1, py1)).unwrap();
        assert_eq!((got.width, got.height), (want.width, want.height), "orientation {o}");
        let worst = got.data.iter().zip(&want.data).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        assert!(worst <= 1, "orientation {o}: off by {worst}");
    }
}

fn converted_with(filters: &[&str], o: u8) -> Converted {
    let legacy = Legacy { filters: filters.iter().map(|s| s.to_string()).collect(), orientation: exif(o), ..Legacy::default() };
    legacy::convert(&legacy).unwrap()
}

#[test]
fn rotation_flip_text_and_redo() {
    let legacy = Legacy {
        filters: vec!["bw=1".into()],
        redo: vec!["crop64=1,0000000080008000".into(), "Lomo=1,50".into(), "crop64=1,00000000ffffffff".into()],
        quarters: 3,
        flipped: true,
        text: Some("1;132;11;a (b);c;Times;0.3,0.6,0.03,0;v1,1,2;;".into()),
        text_active: Some("1".into()),
        orientation: exif(6),
        ..Legacy::default()
    };
    let c = legacy::convert(&legacy).unwrap();
    assert_eq!(
        c.edits_text(),
        ["flip_h()", "rotate(270)", "bw()", "legacy:text(1;132;11;a (b);c;Times;0.3,0.6,0.03,0;v1,1,2;;)", "legacy:textactive(1)"]
    );
    let redo = c.redo_text();
    assert_eq!(redo.len(), 3);
    assert!(redo[0].starts_with("crop(") && redo[2].starts_with("crop("));
    assert_eq!(redo[1], "legacy:Lomo(1,50)");
    // No turn for a whole turn.
    let legacy = Legacy { quarters: 4, ..Legacy::default() };
    assert!(legacy::convert(&legacy).unwrap().edits.is_empty());
}

#[test]
fn more_entries_than_a_recipe_holds_are_refused() {
    let legacy = Legacy { filters: vec!["bw=1".into(); MAX_OPERATIONS], quarters: 1, ..Legacy::default() };
    assert_eq!(legacy::convert(&legacy), Err(Error::TooManyOperations));
    let legacy = Legacy { redo: vec!["bw=1".into(); MAX_OPERATIONS + 1], ..Legacy::default() };
    assert_eq!(legacy::convert(&legacy), Err(Error::TooManyOperations));
}

fn token() -> impl Strategy<Value = String> {
    let names = prop::sample::select(vec![
        "crop64",
        "enhance",
        "fill",
        "finetune2",
        "unsharp2",
        "sat",
        "glow2",
        "ansel",
        "Boost",
        "HDR",
        "Holga",
        "TwoTone",
        "Vignette",
        "Border",
        "bw",
        "tilt",
        "x",
    ]);
    let field = prop_oneof![
        Just("1".to_owned()),
        Just("nan".to_owned()),
        Just("inf".to_owned()),
        Just("".to_owned()),
        any::<f64>().prop_map(|v| v.to_string()),
        "[0-9a-fA-F]{0,20}",
        any::<String>(),
    ];
    prop_oneof![(names, prop::collection::vec(field, 0..8)).prop_map(|(n, f)| format!("{n}={}", f.join(","))), any::<String>(),]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn no_token_list_panics(
        filters in prop::collection::vec(token(), 0..12),
        redo in prop::collection::vec(token(), 0..4),
        quarters in any::<u8>(),
        flipped in any::<bool>(),
        crop in prop::option::of((any::<f64>(), any::<f64>(), any::<f64>(), any::<f64>())),
        o in 1..=8u8,
    ) {
        let legacy = Legacy {
            filters,
            redo,
            quarters,
            flipped,
            crop: crop.map(|(a, b, c, d)| Rect::new(a, b, c, d)),
            orientation: exif(o),
            ..Legacy::default()
        };
        let c = legacy::convert(&legacy).unwrap();
        // Every entry prints back as it parses.
        for op in c.edits.iter().chain(&c.redo) {
            prop_assert_eq!(&Op::parse(&op.to_string()), op);
        }
        prop_assert!(Recipe::parse(c.edits_text()).is_ok());
    }
}
