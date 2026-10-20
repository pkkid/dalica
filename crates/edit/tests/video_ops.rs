//! A video's trim (`roadmap.md` M15.25; `editing.md` §11, `data-model.md` §3.1): `trim(start,end)`
//! in whole milliseconds parses and prints back, refuses a start at or after its end and anything
//! but whole milliseconds, is a video-only operation of the catalogue that renders nothing, and a
//! recipe's last readable trim is the one that counts.

use dalica_edit::catalog::{self, Kind};
use dalica_edit::ops::video::{self, MAX_MS, Trim};
use dalica_edit::ops::{self, Operation};
use dalica_edit::{Op, Recipe};

fn edits(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn trim_parses_prints_and_refuses_what_is_not_a_start_before_an_end() {
    for text in ["0,1", "500,1500", "1,1000000000"] {
        let t = Trim::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(t.to_string(), text);
        assert_eq!(Trim::parse(&t.to_string()), Ok(t));
    }
    let t = Trim::parse("500,1500").unwrap();
    assert_eq!((t.start_ms, t.end_ms, t.length_ms(), t.entry()), (500, 1500, 1000, "trim(500,1500)".to_string()));
    assert_eq!(Trim::new(500, 1500), Ok(t));
    for bad in [
        "1500,500",
        "5,5",
        "-1,5",
        "0,-5",
        "-500,-100",
        "0.5,1.5",
        "+5,9",
        " 5,9",
        "5, 9",
        "05,9",
        "a,b",
        "5",
        "",
        "5,9,12",
        "0,1000000001",
        "0,99999999999999999999",
    ] {
        assert!(Trim::parse(bad).is_err(), "{bad:?} is refused");
    }
    let why = Trim::parse("1500,500").unwrap_err();
    assert_eq!((why.op, why.reason.as_str()), ("trim", "the start must come before the end"));
    assert!(Trim::new(0, MAX_MS + 1).is_err() && Trim::new(7, 7).is_err());
    assert_eq!(Trim::default(), Trim { start_ms: 0, end_ms: MAX_MS }, "the whole clip");
}

#[test]
fn trim_is_a_video_only_entry_of_the_catalogue_that_renders_nothing() {
    let row = catalog::lookup("trim").expect("trim is in the catalogue");
    assert_eq!((row.syntax, row.renders(), row.changes_size), ("trim(start,end)", false, false));
    assert!(row.note.contains("Videos only") && row.note.contains("milliseconds"), "{}", row.note);
    assert!(row.params.iter().all(|p| p.kind == Kind::Seconds && p.default.is_empty()));
    assert_eq!(row.params.iter().map(|p| p.name).collect::<Vec<_>>(), ["start", "end"]);
    assert_eq!((ops::label("trim"), ops::version("trim")), (Some("Trim"), Some(0)));
    assert_eq!((Trim::NAME, Trim::LABEL, Trim::VERSION), ("trim", "Trim", 0));
    // Known, kept as written, and untyped: it never reaches the renderer.
    let op = Op::parse("trim(500,1500)");
    assert!(op.is_known() && op.typed().is_none());
    assert_eq!(op.to_string(), "trim(500,1500)");
    let recipe = Recipe::parse(edits(&["rotate(90)", "trim(500,1500)"])).unwrap();
    assert_eq!(recipe.print(), ["rotate(90)", "trim(500,1500)"]);
}

#[test]
fn the_last_readable_trim_of_a_recipe_counts() {
    assert_eq!(video::of(&edits(&[])), None);
    assert_eq!(video::of(&edits(&["rotate(90)"])), None);
    assert_eq!(video::of(&edits(&["rotate(90)", "trim(500,1500)"])), Some(Trim { start_ms: 500, end_ms: 1500 }));
    assert_eq!(video::find(&edits(&["trim(1,2)", "rotate(90)", "trim(0,900)"])), Some((2, Some(Trim { start_ms: 0, end_ms: 900 }))));
    // An unreadable last trim: found (so Set Start replaces it) but the clip plays whole.
    assert_eq!(video::find(&edits(&["trim(0,10)", "trim(1.5,12.25)"])), Some((1, None)));
    assert_eq!(video::of(&edits(&["trim(0,10)", "trim(9,3)"])), None);
}
