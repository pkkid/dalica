use proptest::prelude::*;
use serde_json::{Map, Value, json};

use super::*;

fn parse(text: &str) -> Result<Sidecar, ParseError> {
    Sidecar::parse(text.as_bytes(), &Limits::default())
}

fn text(s: &Sidecar) -> String {
    String::from_utf8(s.print()).unwrap()
}

#[test]
fn a_new_sidecar_prints_its_version() {
    let s = Sidecar::new();
    assert_eq!(text(&s), "{\n  \"dalica\": {\n    \"version\": 1\n  }\n}\n");
    assert_eq!(s.version(), Some(1));
    assert_eq!(parse(&text(&s)).unwrap(), s);
}

/// The example of `data-model.md` §3, with keys Dalica does not know yet.
const EXAMPLE: &str = r#"{
  "dalica": { "version": 1, "writer": "a future Dalica" },
  "folder": { "description": "Weekend in Lisbon", "date": "2026-06-04", "order": ["IMG_0003.jpg", "IMG_0001.jpg"] },
  "photos": {
    "IMG_0001.jpg": {
      "star": true,
      "caption": "Tram 28 at sunset",
      "faces": [{ "rect": [0.41, 0.22, 0.52, 0.38], "person": "p7f3a2c" }],
      "albums": { "a91c0e": "V" },
      "geo": [38.7116, -9.1389],
      "x-unknown": { "nested": [1, 2.5, null, "s"] }
    },
    "IMG_0002.jpg": { "star": "yes", "caption": "wrong-type star" }
  },
  "planted": { "by": "another app", "list": [true, false] },
  "people": { "p7f3a2c": { "name": "Ana Costa" } }
}"#;

#[test]
fn editing_one_photo_keeps_unknown_keys_and_wrong_types_elsewhere() {
    let mut s = parse(EXAMPLE).unwrap();
    let before: Value = serde_json::from_str(EXAMPLE).unwrap();
    assert_eq!(s.star("IMG_0001.jpg"), Field::Value(true));
    assert_eq!(s.star("IMG_0002.jpg"), Field::WrongType(json!("yes")));
    assert_eq!(s.problems(), [Problem { pointer: "/photos/IMG_0002.jpg/star".into(), expected: "true or false" }]);

    s.set_star("IMG_0003.jpg", true).unwrap();
    let after: Value = serde_json::from_slice(&s.print()).unwrap();
    // Everything that was there is still there, unchanged; only the new star was added.
    let mut expected = before.clone();
    expected["photos"]["IMG_0003.jpg"] = json!({ "star": true });
    assert_eq!(after, expected);
    assert_eq!(after["planted"], before["planted"]);
    assert_eq!(after["photos"]["IMG_0002.jpg"]["star"], json!("yes"));
    assert_eq!(after["photos"]["IMG_0001.jpg"]["x-unknown"], before["photos"]["IMG_0001.jpg"]["x-unknown"]);
    assert_eq!(after["dalica"]["writer"], json!("a future Dalica"));

    // Clearing a star keeps the rest of the entry; clearing the last key removes the entry.
    s.set_star("IMG_0001.jpg", false).unwrap();
    assert_eq!(s.star("IMG_0001.jpg"), Field::Absent);
    assert!(s.photo_names().any(|n| n == "IMG_0001.jpg"));
    s.set_star("IMG_0003.jpg", false).unwrap();
    assert!(!s.photo_names().any(|n| n == "IMG_0003.jpg"));
    s.set_star("never-there.jpg", false).unwrap();
    // Starring the photo whose star had the wrong type replaces it: that is the user's edit.
    s.set_star("IMG_0002.jpg", true).unwrap();
    assert_eq!(s.star("IMG_0002.jpg"), Field::Value(true));
    assert!(s.problems().is_empty());
}

#[test]
fn printing_is_sorted_two_spaced_lf_and_stable() {
    let s = parse(EXAMPLE).unwrap();
    let t = text(&s);
    assert!(!t.contains('\r') && t.ends_with("}\n") && t.contains("\n  \"dalica\": {\n    \"version\": 1,\n    \"writer\""));
    let top: Vec<&str> = t.lines().filter(|l| l.starts_with("  \"")).collect();
    assert_eq!(top, ["  \"dalica\": {", "  \"folder\": {", "  \"people\": {", "  \"photos\": {", "  \"planted\": {"]);
    assert_eq!(text(&parse(&t).unwrap()), t);
    assert!(t.contains("\"geo\": [\n        38.7116,\n        -9.1389\n      ]"));
}

#[test]
fn bom_crlf_and_duplicate_keys() {
    let crlf = "\u{feff}{\r\n  \"photos\": {\r\n    \"a.jpg\": { \"star\": false, \"star\": true }\r\n  }\r\n}\r\n";
    let s = parse(crlf).unwrap();
    assert_eq!(s.star("a.jpg"), Field::Value(true));
    assert!(!text(&s).contains('\r'));
}

#[test]
fn unreadable_files_are_parse_errors() {
    assert!(matches!(parse("{\n  \"photos\": {,\n}"), Err(ParseError::Json { line: 2, .. })));
    assert!(matches!(parse(""), Err(ParseError::Json { .. })));
    assert_eq!(parse("[1, 2]"), Err(ParseError::NotAnObject));
    assert_eq!(parse("\"text\""), Err(ParseError::NotAnObject));
    let deep = format!("{}1{}", "{\"a\":".repeat(20), "}".repeat(20));
    assert!(matches!(parse(&deep), Err(ParseError::Limit(_))));
    let very_deep = "[".repeat(100_000);
    assert!(parse(&very_deep).is_err());
    let small = Limits { max_string: 8, max_list: 3, max_photos: 1, max_keys_per_entry: 2, max_bytes: 200, ..Limits::default() };
    for bad in [
        r#"{"x": "123456789"}"#,
        r#"{"x": [1, 2, 3, 4]}"#,
        r#"{"photos": {"a": {}, "b": {}}}"#,
        r#"{"photos": {"a": {"k1": 1, "k2": 2, "k3": 3}}}"#,
        r#"{"123456789": 1}"#,
    ] {
        assert!(matches!(Sidecar::parse(bad.as_bytes(), &small), Err(ParseError::Limit(_))), "{bad}");
    }
    assert!(matches!(Sidecar::parse(&[b' '; 201], &small), Err(ParseError::Limit(_))));
}

#[test]
fn wrong_types_are_reported_and_never_written_over() {
    let mut s = parse(r#"{"dalica": {"version": "one"}, "photos": {"a.jpg": [1], "b.jpg": {"star": 1}}}"#).unwrap();
    assert_eq!(s.version(), None);
    let pointers: Vec<String> = s.problems().into_iter().map(|p| p.pointer).collect();
    assert_eq!(pointers, ["/dalica/version", "/photos/a.jpg", "/photos/b.jpg/star"]);
    assert_eq!(s.set_star("a.jpg", true), Err(WrongType { pointer: "/photos/a.jpg".into() }));
    assert_eq!(s.set_star("a.jpg", false), Err(WrongType { pointer: "/photos/a.jpg".into() }));
    let mut t = parse(r#"{"photos": "oops"}"#).unwrap();
    assert_eq!(t.set_star("x.jpg", true), Err(WrongType { pointer: "/photos".into() }));
    assert_eq!(t.problems()[0].pointer, "/photos");
    assert_eq!(text(&t), "{\n  \"photos\": \"oops\"\n}\n");
    // JSON-pointer escaping of `~` and `/` in names.
    let u = parse(r#"{"photos": {"a/b~c": 3}}"#).unwrap();
    assert_eq!(u.problems()[0].pointer, "/photos/a~1b~0c");
}

#[test]
fn plain_names() {
    for good in ["IMG_0001.jpg", "a]b.jpg", "say \"hi\".jpg", "a;b=c.jpg", "東京.png", ".hidden.jpg", "CONSOLE.jpg", "con2.jpg"] {
        assert!(plain_name(good), "{good}");
    }
    for bad in ["", ".", "..", "a/b.jpg", "a\\b.jpg", "a\0.jpg", "C:", "c:x.jpg", "x.jpg:stream", "CON", "con.jpg", "NUL.txt", "lpt9", "COM1 .jpg"] {
        assert!(!plain_name(bad), "{bad:?}");
    }
}

/// Any JSON value with finite numbers, up to a small depth.
fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>().prop_filter("finite", |f| f.is_finite()).prop_map(Value::from),
        "\\PC{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
            proptest::collection::btree_map("\\PC{0,10}", inner, 0..5).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn parse_of_print_is_identity(doc in proptest::collection::btree_map("\\PC{0,10}", json_value(), 0..6)) {
        let s = Sidecar { root: doc.into_iter().collect::<Map<String, Value>>() };
        let printed = s.print();
        let back = Sidecar::parse(&printed, &Limits::default()).unwrap();
        prop_assert_eq!(&back, &s);
        prop_assert_eq!(back.print(), printed);
    }

    #[test]
    fn any_file_name_round_trips_as_a_key(names in proptest::collection::vec("\\PC{1,40}|[\\]\";=\\\\/~.\\u{0}-\\u{1f}]{1,8}", 1..6)) {
        let mut s = Sidecar::new();
        for n in &names {
            s.set_star(n, true).unwrap();
        }
        let back = Sidecar::parse(&s.print(), &Limits::default()).unwrap();
        for n in &names {
            prop_assert_eq!(back.star(n), Field::Value(true));
        }
        let mut listed: Vec<&str> = back.photo_names().collect();
        let mut want: Vec<&str> = names.iter().map(String::as_str).collect();
        listed.sort_unstable();
        want.sort_unstable();
        want.dedup();
        prop_assert_eq!(listed, want);
    }

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        if let Ok(s) = Sidecar::parse(&bytes, &Limits::default()) {
            let _ = s.problems();
            let _ = s.print();
        }
    }
}

/// Found by `parse_of_print_is_identity`: without `serde_json`'s `float_roundtrip` these came back
/// one unit in the last place off, so coordinates would drift on every rewrite.
#[test]
fn floats_survive_any_number_of_rewrites() {
    for f in [1.373_398_601_339_381_5e-238, 38.7116, -9.1389, 0.1 + 0.2, f64::MIN_POSITIVE, 5e-324, f64::MAX] {
        let mut s = Sidecar::new();
        s.root.insert("geo".into(), json!([f]));
        let mut text = s.print();
        for _ in 0..3 {
            text = Sidecar::parse(&text, &Limits::default()).unwrap().print();
        }
        let back = Sidecar::parse(&text, &Limits::default()).unwrap();
        assert_eq!(back.root["geo"][0].as_f64(), Some(f), "{f:e}");
    }
}
