//! The operation syntax and the recipe rules (`data-model.md` §3.1): every operation prints back
//! the text it was parsed from, unknown and `legacy:` operations keep their text and place, the
//! rules for appending rotations and flips, the pipeline order, and the recipe limit; the 27
//! effects' syntax (M12.1, `editing.md` §8.5): defaults round-trip, numbers clamp, and an entry
//! whose parameters are refused is kept and skipped.

use dalica_edit::catalog::{Kind, OpInfo, catalog};
use dalica_edit::{Dihedral, Error, LISTED, MAX_OPERATIONS, Op, Recipe, Stage, Turn, Typed, ops};
use dalica_image::Orientation;
use proptest::prelude::*;

use crate::common::{cases, run, scene};

/// One example of every operation of `data-model.md` §3.1, with typical parameters.
const EVERY_OPERATION: &[&str] = &[
    "rotate(90)",
    "rotate(180)",
    "rotate(270)",
    "flip_h()",
    "flip_v()",
    "crop(0.08,0.10,0.92,0.86)",
    "straighten(1.5)",
    "straighten(-10)",
    "retouch(0.1,0.2,0.3,0.4,0.02;0.5,0.5,0.6,0.6,0.01)",
    "autofix()",
    "autocontrast()",
    "autocolor()",
    "fill_light(0.25)",
    "tuning(0.2,0.1,0.05,-0.3,#808080)",
    "tuning(0,0,0,0,none)",
    "trim(1.5,12.25)",
    "sharpen(0.6)",
    "sepia()",
    "bw()",
    "warmify()",
    "grain()",
    "grain(10,true)",
    "tint(#80cfff,0)",
    "saturation(0.16)",
    "soft_focus(0.5,0.5,0.3,0.7)",
    "glow(0.4,3.0)",
    "filtered_bw(#ff8000)",
    "focal_bw(0.4,0.6,0.25,0.5)",
    "graduated_tint(0.3,0.25,0.25,#9cc8ff)",
    "graduated_tint(0.5,0.4,0,0.25,0.25,#9cc8ff)",
    "infrared(0)",
    "plastic_lens(70,30,0)",
    "hdr(20,3,0)",
    "widescreen(true)",
    "dream_glow(25,50,0)",
    "invert()",
    "duotone(#004488,#ffff00,0,20,0)",
    "boost(50)",
    "soften(50,50)",
    "soften(50,50;paint,0.05,0.2,0.3,0.25,0.3;erase,0.02,0.22,0.3)",
    "vignette(35,1.4,#000000,0)",
    "pixelate(20,normal,0)",
    "focal_zoom(0.5,0.5,0.3,50,50,0)",
    "border(#000000,20,#ffffff,5,0,0)",
    "drop_shadow(#000000,4,90,10,30,#ffffff)",
    "museum_matte(#1a0e03,25,#f0eae4,40)",
];

#[test]
fn every_operation_prints_back_as_parsed() {
    for &text in EVERY_OPERATION {
        let op = Op::parse(text);
        assert!(op.is_known(), "{text} should be known, got {op:?}");
        assert_eq!(op.to_string(), text);
    }
    // Every listed name has an example above.
    for name in LISTED {
        assert!(EVERY_OPERATION.iter().any(|t| t.starts_with(&format!("{name}("))), "no example of {name}");
    }
    // The sidecar example of data-model.md §3 round-trips as a whole.
    let edits = ["rotate(90)", "crop(0.08,0.10,0.92,0.86)", "straighten(1.5)", "autofix()", "fill_light(0.25)", "sepia()"];
    assert_eq!(Recipe::parse(edits).unwrap().print(), edits);
}

#[test]
fn rotations_and_flips_are_typed() {
    assert_eq!(Op::parse("rotate(90)"), Op::Rotate(Turn::Quarter));
    assert_eq!(Op::parse("rotate(180)"), Op::Rotate(Turn::Half));
    assert_eq!(Op::parse("rotate(270)"), Op::Rotate(Turn::ThreeQuarters));
    assert_eq!(Op::parse("flip_h()"), Op::FlipH);
    assert_eq!(Op::parse("flip_v()"), Op::FlipV);
    assert_eq!(Op::parse("crop(0,0,1,1)"), Op::Listed { name: "crop", params: "0,0,1,1".into() });
    assert_eq!(Op::Rotate(Turn::ThreeQuarters).to_string(), "rotate(270)");
}

#[test]
fn unknown_legacy_and_out_of_range_operations_are_kept_verbatim() {
    let cases = [
        // From a newer version.
        ("frame(3,#000000)", false),
        ("hologram()", false),
        // Kept from the legacy organizer's sidecar.
        ("legacy:redeye(0.1,0.2,0.3,0.4)", true),
        ("legacy:whatever", true),
        // A typed operation with a parameter it does not take.
        ("rotate(45)", false),
        ("rotate(0)", false),
        ("rotate(360)", false),
        ("rotate(-90)", false),
        ("rotate(090)", false),
        ("rotate( 90)", false),
        ("rotate()", false),
        ("flip_h(1)", false),
        ("flip_v(x)", false),
        // Damaged text.
        ("", false),
        ("crop", false),
        ("crop(0,0,1,1", false),
        ("Crop(0,0,1,1)", false),
        ("crop((0))", false),
        ("sepia() ", false),
        ("sépia()", false),
        ("crop(0\n,1)", false),
    ];
    for (text, legacy) in cases {
        let op = Op::parse(text);
        assert_eq!(op.to_string(), text);
        assert!(!op.is_known(), "{text:?} should not be known");
        assert_eq!(op.stage(), Stage::Skipped);
        assert_eq!(op.orientation(), None);
        match op {
            Op::Legacy(_) => assert!(legacy, "{text:?}"),
            Op::Unknown(_) => assert!(!legacy, "{text:?}"),
            other => panic!("{text:?} parsed as {other:?}"),
        }
    }
}

fn push_all(texts: &[&str]) -> Recipe {
    let mut r = Recipe::new();
    for t in texts {
        r.push(Op::parse(t)).unwrap();
    }
    r
}

#[test]
fn a_rotation_after_a_rotation_merges_and_a_net_zero_removes_it() {
    assert_eq!(push_all(&["rotate(90)", "rotate(90)"]).print(), ["rotate(180)"]);
    assert_eq!(push_all(&["rotate(90)", "rotate(90)", "rotate(90)"]).print(), ["rotate(270)"]);
    assert_eq!(push_all(&["rotate(270)", "rotate(180)"]).print(), ["rotate(90)"]);
    assert!(push_all(&["rotate(90)", "rotate(270)"]).is_empty());
    assert!(push_all(&["rotate(180)", "rotate(180)"]).is_empty());
    assert!(push_all(&["rotate(90)"; 4]).is_empty());
    // Only the last operation merges: a rotation after other edits is a new entry.
    assert_eq!(push_all(&["rotate(90)", "sepia()", "rotate(90)"]).print(), ["rotate(90)", "sepia()", "rotate(90)"]);
    // A net zero leaves what came before alone.
    assert_eq!(push_all(&["sepia()", "rotate(90)", "rotate(270)"]).print(), ["sepia()"]);
    // A merge that removes the last rotation does not reach further back.
    assert_eq!(push_all(&["rotate(90)", "flip_h()", "rotate(180)", "rotate(180)"]).print(), ["rotate(90)", "flip_h()"]);
}

#[test]
fn an_unknown_operation_between_two_rotations_keeps_both() {
    for between in ["hologram(2)", "legacy:redeye(0.1,0.2,0.3,0.4)", "rotate(45)", "crop(0,0,1,1"] {
        let mut r = Recipe::parse(["rotate(90)", between]).unwrap();
        r.push(Op::Rotate(Turn::Quarter)).unwrap();
        assert_eq!(r.print(), ["rotate(90)", between, "rotate(90)"]);
        // The unknown operation does not stop the rotation after it from counting.
        assert_eq!(r.orientation(), Dihedral::ROTATE_180);
    }
}

#[test]
fn two_identical_flips_in_a_row_cancel() {
    assert!(push_all(&["flip_h()", "flip_h()"]).is_empty());
    assert!(push_all(&["flip_v()", "flip_v()"]).is_empty());
    assert_eq!(push_all(&["flip_h()", "flip_v()"]).print(), ["flip_h()", "flip_v()"]);
    assert_eq!(push_all(&["flip_h()", "rotate(90)", "flip_h()"]).print(), ["flip_h()", "rotate(90)", "flip_h()"]);
    assert_eq!(push_all(&["flip_h()", "flip_v()", "flip_v()"]).print(), ["flip_h()"]);
    assert_eq!(push_all(&["flip_v()", "bw()", "flip_v()"]).print(), ["flip_v()", "bw()", "flip_v()"]);
    // Other operations append as they are, even twice in a row.
    assert_eq!(push_all(&["sepia()", "sepia()"]).print(), ["sepia()", "sepia()"]);
}

#[test]
fn geometry_comes_first_in_the_pipeline_and_unknown_operations_are_skipped() {
    let r = Recipe::parse([
        "sepia()",
        "hologram()",
        "rotate(90)",
        "fill_light(0.2)",
        "legacy:x()",
        "crop(0,0,0.5,0.5)",
        "flip_h()",
        "bw()",
        "straighten(2)",
    ])
    .unwrap();
    let order: Vec<String> = r.pipeline().map(Op::to_string).collect();
    assert_eq!(order, ["rotate(90)", "crop(0,0,0.5,0.5)", "flip_h()", "straighten(2)", "sepia()", "fill_light(0.2)", "bw()"]);
    // The recipe itself keeps every operation where it stood.
    assert_eq!(r.len(), 9);
    assert_eq!(r.print()[1], "hologram()");
    assert_eq!(r.print()[4], "legacy:x()");
}

#[test]
fn a_recipe_holds_at_most_the_limit() {
    let full = vec!["sepia()"; MAX_OPERATIONS];
    let mut r = Recipe::parse(&full).unwrap();
    assert_eq!(r.len(), MAX_OPERATIONS);
    assert_eq!(Recipe::parse(vec!["sepia()"; MAX_OPERATIONS + 1]), Err(Error::TooManyOperations));
    assert_eq!(r.push(Op::parse("bw()")), Err(Error::TooManyOperations));
    assert_eq!(r.len(), MAX_OPERATIONS);
    // A full recipe ending in a rotation still merges and cancels.
    let mut texts = full;
    *texts.last_mut().unwrap() = "rotate(90)";
    let mut r = Recipe::parse(&texts).unwrap();
    r.push(Op::Rotate(Turn::Quarter)).unwrap();
    assert_eq!(r.ops().last(), Some(&Op::Rotate(Turn::Half)));
    r.push(Op::Rotate(Turn::Half)).unwrap();
    assert_eq!(r.len(), MAX_OPERATIONS - 1);
}

#[test]
fn the_displayed_size_follows_the_exif_orientation_and_the_recipe() {
    let o = |v| Orientation::new(v).unwrap();
    assert_eq!(Recipe::new().display_size(o(1), 400, 300), (400, 300));
    assert_eq!(Recipe::new().display_size(o(6), 400, 300), (300, 400));
    let quarter = Recipe::parse(["rotate(90)"]).unwrap();
    assert_eq!(quarter.display_size(o(1), 400, 300), (300, 400));
    assert_eq!(quarter.display_size(o(6), 400, 300), (400, 300));
    assert_eq!(quarter.display(o(6)), Dihedral::ROTATE_180);
    assert_eq!(quarter.display(o(8)), Dihedral::IDENTITY);
    assert_eq!(Recipe::parse(["flip_h()", "flip_v()"]).unwrap().orientation(), Dihedral::ROTATE_180);
    assert_eq!(Recipe::parse(["flip_h()", "rotate(90)"]).unwrap().orientation(), Dihedral::TRANSVERSE);
    assert_eq!(Recipe::parse(["rotate(90)"; 4]).unwrap().orientation(), Dihedral::IDENTITY);
}

/// The shown size (`Recipe::shown_size`, `shown_size`): a Border grows the picture by both bands,
/// a later rotation turns the grown picture, a crop counts only where the reader counts it (the
/// frame then grows the photo as that reader sizes it), and without a frame it is the file's size
/// turned; an unreadable recipe or an unknown size changes nothing.
#[test]
fn the_shown_size_is_the_frames_grown_size_turned_and_the_files_without_a_frame() {
    use dalica_edit::{Cropping, shown_size};
    let s = |e: &[&str], c| shown_size(&e.iter().map(|t| t.to_string()).collect::<Vec<_>>(), 600, 400, c);
    // 5 % of the short side (400) on every side: 600 × 400 shows at 640 × 440.
    let border = "border(#000000,50,#ffffff,0,0,0)";
    for c in [Cropping::Counted, Cropping::Ignored] {
        assert_eq!(s(&[border], c), (640, 440), "{c:?}");
        assert_eq!(s(&[border, "rotate(90)"], c), (440, 640), "{c:?}: turned after the frame");
        assert_eq!(s(&["rotate(90)", border], c), (440, 640), "{c:?}: a turn turns the framed picture wherever it stands");
        assert_eq!(s(&[], c), (600, 400), "{c:?}");
        assert_eq!(s(&["rotate(90)", "flip_h()", "sepia()"], c), (400, 600), "{c:?}: the file's size turned");
        assert_eq!(s(&["rotate(45)"], c), (600, 400), "{c:?}: an operation edit does not know is no turn");
    }
    // A crop to the middle half across (300 × 400): the frame's bands follow the photo it frames.
    let cropped = ["crop(0.25,0,0.75,1)", border];
    assert_eq!(s(&cropped, Cropping::Counted), (330, 430));
    assert_eq!(s(&cropped, Cropping::Ignored), (640, 440));
    assert_eq!(s(&cropped[..1], Cropping::Counted), (300, 400));
    assert_eq!(s(&cropped[..1], Cropping::Ignored), (600, 400));
    // The same through a parsed recipe and an EXIF orientation that turns the file.
    let r = Recipe::parse([border]).unwrap();
    assert_eq!(r.shown_size(Orientation::new(6).unwrap(), 600, 400, Cropping::Counted), (440, 640));
    // A recipe edit cannot read counts as none; an unknown size stays unknown.
    let too_many = vec![border.to_string(); MAX_OPERATIONS + 1];
    assert_eq!(shown_size(&too_many, 600, 400, Cropping::Counted), (600, 400));
    assert_eq!(shown_size(&[border.to_string()], 0, 0, Cropping::Counted), (0, 0));
}

/// The 27 effects' rows.
fn effects() -> impl Iterator<Item = &'static OpInfo> {
    catalog().iter().filter(|op| op.effect.is_some())
}

/// The text between an entry's parentheses.
fn inner(entry: &str) -> &str {
    entry.split_once('(').and_then(|(_, rest)| rest.strip_suffix(')')).unwrap()
}

/// An effect's default entry with its `i`-th parameter (strokes aside) set to `value`, or left out
/// when `value` is `None`.
fn changed(op: &OpInfo, i: usize, value: Option<&str>) -> String {
    let mut fields: Vec<String> = op.params.iter().filter(|p| !matches!(p.kind, Kind::Strokes { .. })).map(|p| p.default.to_string()).collect();
    match value {
        Some(v) => fields[i] = v.to_string(),
        None => {
            fields.remove(i);
        }
    }
    format!("{}({})", op.name, fields.join(","))
}

#[test]
fn every_effect_at_its_defaults_prints_and_parses_back_equal() {
    for op in effects() {
        let defaults = Typed::default_of(op.name).unwrap_or_else(|| panic!("{}: a module", op.name));
        let entry = Op::from(defaults.clone());
        assert_eq!(Some(entry.to_string()), op.default_entry(), "{}", op.name);
        // Through the sidecar's strings and back.
        let recipe = Recipe::parse([entry.to_string()]).unwrap();
        assert_eq!(recipe.print(), [entry.to_string()]);
        assert_eq!(Typed::parse(op.name, inner(&recipe.print()[0])), Some(Ok(defaults.clone())), "{}", op.name);
        assert_eq!(defaults.clone().clamped(), defaults, "{}: the defaults are in range", op.name);
        // A stub module (`VERSION` 0) reads its parameters but is not typed for rendering yet; a
        // module that renders (1 or more) is typed with these defaults.
        assert_eq!(Some(defaults.version()), ops::version(op.name), "{}", op.name);
        if defaults.version() == 0 {
            assert_eq!(recipe.ops()[0].typed(), None, "{}", op.name);
            assert!(!op.renders(), "{}", op.name);
        } else {
            assert_eq!(recipe.ops()[0].typed(), Some(Ok(defaults.clone())), "{}", op.name);
            assert!(op.renders(), "{}", op.name);
        }
        assert_eq!(recipe.ops()[0].label(), Some(op.label));
        assert_eq!(recipe.ops()[0].stage(), Stage::Adjust);
    }
}

#[test]
fn an_effect_value_out_of_range_is_brought_to_the_nearest_end() {
    for op in effects() {
        for (i, p) in op.params.iter().enumerate() {
            let Kind::Number { min, max } = p.kind else { continue };
            for (value, bound) in [("1e9", max), ("inf", max), ("-1e9", min), ("-inf", min)] {
                let entry = changed(op, i, Some(value));
                let read = Typed::parse(op.name, inner(&entry)).unwrap().unwrap_or_else(|e| panic!("{entry}: {e}"));
                assert_eq!(read, Typed::parse(op.name, inner(&changed(op, i, Some(&bound.to_string())))).unwrap().unwrap(), "{entry}");
                assert_eq!(read.clone().clamped(), read, "{entry}");
            }
        }
    }
}

#[test]
fn an_effect_with_a_nan_a_missing_field_or_a_bad_colour_is_kept_and_skipped() {
    let img = scene(7, 24, 16);
    let plain = run(&img, &["rotate(90)"]);
    let mut refused = Vec::new();
    for op in effects() {
        for (i, p) in op.params.iter().enumerate() {
            match p.kind {
                Kind::Number { .. } => refused.extend([changed(op, i, Some("NaN")), changed(op, i, Some("")), changed(op, i, None)]),
                Kind::Colour { .. } => {
                    refused.extend([changed(op, i, Some("#12345g")), changed(op, i, Some("80cfff")), changed(op, i, Some("#fff"))])
                }
                Kind::Flag | Kind::Choice(_) => refused.push(changed(op, i, Some("maybe"))),
                _ => {}
            }
        }
        // An extra field, and parameters on a one-click effect.
        refused.push(format!(
            "{}({}1)",
            op.name,
            op.default_entry().map(|e| inner(&e).to_string() + if op.params.is_empty() { "" } else { "," }).unwrap()
        ));
    }
    refused.push("soften(50,50;paint,0.05,0.5)".into());
    refused.push("soften(50,50;smudge,0.05,0.5,0.5)".into());
    refused.push("soften(50,50;paint,0.05)".into());
    for entry in &refused {
        let op = Op::parse(entry);
        assert!(op.is_known(), "{entry}");
        assert!(matches!(op.typed(), Some(Err(_))), "{entry}: refused, {:?}", op.typed());
        let ops = [entry.as_str(), "rotate(90)"];
        let recipe = Recipe::parse(ops).unwrap();
        assert_eq!(recipe.print(), ops, "{entry}: kept as written");
        assert_eq!(run(&img, &ops), plain, "{entry}: skipped");
    }
}

/// Strings that look like operations more often than random text does.
fn op_text() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<String>(),
        "[a-z_]{0,14}\\([ -~]{0,20}\\)",
        "legacy:[ -~]{0,20}",
        prop::sample::select(EVERY_OPERATION.to_vec()).prop_map(str::to_owned),
        "rotate\\(-?[0-9]{1,4}\\)",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases(256)))]

    #[test]
    fn any_string_parses_and_prints_back_unchanged(text in op_text()) {
        let op = Op::parse(&text);
        prop_assert_eq!(op.to_string(), text);
        // Typed operations re-parse to themselves.
        prop_assert_eq!(Op::parse(&op.to_string()), op);
    }

    #[test]
    fn pushing_never_panics_and_never_changes_the_net_orientation(texts in prop::collection::vec(op_text(), 0..24)) {
        let parsed = Recipe::parse(&texts).unwrap();
        prop_assert_eq!(parsed.print(), texts.clone());
        let mut pushed = Recipe::new();
        for t in &texts {
            pushed.push(Op::parse(t)).unwrap();
        }
        prop_assert!(pushed.len() <= parsed.len());
        prop_assert_eq!(pushed.orientation(), parsed.orientation());
        // Merging only removes geometry: every other operation survives, in order.
        let others = |r: &Recipe| r.ops().iter().filter(|op| op.orientation().is_none()).cloned().collect::<Vec<_>>();
        prop_assert_eq!(others(&pushed), others(&parsed));
        prop_assert_eq!(pushed.pipeline().count() + pushed.ops().iter().filter(|op| op.stage() == Stage::Skipped).count(), pushed.len());
    }
}
