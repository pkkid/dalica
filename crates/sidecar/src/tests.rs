use proptest::prelude::*;
use serde_json::{Map, Value, json};

use dalica_core::{Date, DateTime};

use super::fields::PHOTO_KEYS;
use super::*;

/// A property test's cases: `full` at full strength (`DALICA_FULL_TESTS=1`, the review gate),
/// `quick` in a task's gate.
fn cases(full: u32, quick: u32) -> ProptestConfig {
    ProptestConfig::with_cases(if std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1") { full } else { quick })
}

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

/// The full example of `data-model.md` §3: every key Dalica reads, per photo and per folder.
const FULL: &str = r#"{
  "dalica": { "version": 1 },
  "folder": {
    "description": "Weekend in Lisbon with the family",
    "date": "2026-06-04",
    "place": "Lisbon, Portugal",
    "hidden": true,
    "sort": "manual",
    "order": ["IMG_0003.jpg", "IMG_0001.jpg", "IMG_0002.jpg"]
  },
  "photos": {
    "IMG_0001.jpg": {
      "star": true,
      "caption": "Tram 28 at sunset",
      "tags": ["lisbon", "tram", "sunset"],
      "hidden": false,
      "edits": ["rotate(90)", "crop(0.08,0.10,0.92,0.86)", "frobnicate(7)", "sepia()"],
      "redo": ["glow(0.4,3.0)"],
      "geo": [38.7116, -9.1389],
      "date": "2026-06-04T19:42:10"
    },
    "IMG_0002.jpg": { "date": "2026:06:04 19:42:10", "hidden": true }
  }
}"#;

fn strings(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

#[test]
fn every_m6_field_reads_with_its_type() {
    let s = parse(FULL).unwrap();
    assert!(s.problems().is_empty(), "{:?}", s.problems());
    let p = s.photo("IMG_0001.jpg");
    assert_eq!(p.star, Field::Value(true));
    assert_eq!(p.caption, Field::Value("Tram 28 at sunset".into()));
    assert_eq!(p.tags, Field::Value(strings(&["lisbon", "tram", "sunset"])));
    assert_eq!(p.hidden, Field::Value(false));
    // An operation Dalica does not know is one string like the others, in its place.
    assert_eq!(p.edits, Field::Value(strings(&["rotate(90)", "crop(0.08,0.10,0.92,0.86)", "frobnicate(7)", "sepia()"])));
    assert_eq!(p.redo, Field::Value(strings(&["glow(0.4,3.0)"])));
    assert_eq!(p.date, Field::Value("2026-06-04T19:42:10".parse().unwrap()));
    // The EXIF spelling of a date reads as the same date.
    assert_eq!(s.photo("IMG_0002.jpg").date, Field::Value("2026-06-04T19:42:10".parse().unwrap()));
    assert_eq!(s.photo("IMG_0002.jpg").caption, Field::Absent);
    assert_eq!(s.photo("nobody.jpg").tags, Field::Absent);
    let f = s.folder();
    assert_eq!(f.description, Field::Value("Weekend in Lisbon with the family".into()));
    assert_eq!(f.date, Field::Value("2026-06-04".parse().unwrap()));
    assert_eq!(f.place, Field::Value("Lisbon, Portugal".into()));
    assert_eq!(f.hidden, Field::Value(true));
    assert_eq!(Sidecar::new().folder().hidden, Field::Absent);
}

#[test]
fn every_m6_field_is_set_and_cleared_without_touching_the_rest() {
    let mut s = parse(FULL).unwrap();
    let before: Value = serde_json::from_str(FULL).unwrap();
    let date = "2001-02-03T04:05:06".parse().unwrap();
    for v in [
        PhotoValue::Caption("Ein \"Bild\" ; = ]".into()),
        PhotoValue::Tags(strings(&["a/b", "c,d", "東京"])),
        PhotoValue::Hidden(true),
        PhotoValue::Edits(strings(&["rotate(270)"])),
        PhotoValue::Redo(strings(&["x()", "y()"])),
        PhotoValue::Date(Some(date)),
    ] {
        s.set_photo("IMG_0001.jpg", &v).unwrap();
    }
    s.set_folder(&FolderValue::Place("Porto".into())).unwrap();
    s.set_folder(&FolderValue::Date(Some("1999-12-31".parse().unwrap()))).unwrap();
    let back = parse(&text(&s)).unwrap();
    let p = back.photo("IMG_0001.jpg");
    assert_eq!(p.caption, Field::Value("Ein \"Bild\" ; = ]".into()));
    assert_eq!(p.tags, Field::Value(strings(&["a/b", "c,d", "東京"])));
    assert_eq!((p.hidden, p.date), (Field::Value(true), Field::Value(date)));
    assert_eq!(p.edits, Field::Value(strings(&["rotate(270)"])));
    assert_eq!(p.redo, Field::Value(strings(&["x()", "y()"])));
    assert_eq!(back.folder().place, Field::Value("Porto".into()));
    let json: Value = serde_json::from_slice(&back.print()).unwrap();
    assert_eq!(json["photos"]["IMG_0001.jpg"]["date"], json!("2001-02-03T04:05:06"));
    assert_eq!(json["folder"]["date"], json!("1999-12-31"));
    assert_eq!(json["photos"]["IMG_0001.jpg"]["geo"], before["photos"]["IMG_0001.jpg"]["geo"]);
    assert_eq!(json["folder"]["order"], before["folder"]["order"]);
    assert_eq!(json["folder"]["sort"], json!("manual"));

    // Clearing: the empty value removes the key; the last key removes the entry or the object.
    for v in [
        PhotoValue::Star(false),
        PhotoValue::Caption(String::new()),
        PhotoValue::Tags(Vec::new()),
        PhotoValue::Hidden(false),
        PhotoValue::Edits(Vec::new()),
        PhotoValue::Redo(Vec::new()),
        PhotoValue::Date(None),
    ] {
        s.set_photo("IMG_0002.jpg", &v).unwrap();
        s.set_photo("never.jpg", &v).unwrap();
    }
    assert!(!s.photo_names().any(|n| n == "IMG_0002.jpg" || n == "never.jpg"));
    let mut bare = Sidecar::new();
    bare.set_photo("a.jpg", &PhotoValue::Tags(Vec::new())).unwrap();
    bare.set_folder(&FolderValue::Hidden(false)).unwrap();
    assert_eq!(bare, Sidecar::new(), "clearing never creates anything");
    bare.set_folder(&FolderValue::Hidden(true)).unwrap();
    assert_eq!(text(&bare), "{\n  \"dalica\": {\n    \"version\": 1\n  },\n  \"folder\": {\n    \"hidden\": true\n  }\n}\n");
    bare.set_folder(&FolderValue::Hidden(false)).unwrap();
    assert_eq!(bare, Sidecar::new());
    // Keys of other programs keep an object alive.
    for v in [FolderValue::Description(String::new()), FolderValue::Date(None), FolderValue::Place(String::new()), FolderValue::Hidden(false)] {
        s.set_folder(&v).unwrap();
    }
    let json: Value = serde_json::from_slice(&s.print()).unwrap();
    assert_eq!(json["folder"], json!({ "sort": "manual", "order": ["IMG_0003.jpg", "IMG_0001.jpg", "IMG_0002.jpg"] }));
}

#[test]
fn wrong_types_of_m6_fields_are_kept_and_reported() {
    let doc = r#"{
      "folder": { "description": 3, "date": "June", "place": ["x"], "hidden": "yes" },
      "photos": { "a.jpg": { "caption": false, "tags": ["ok", 1], "hidden": 0, "edits": "rotate(90)", "redo": {}, "date": "2026-02-30T00:00:00" } }
    }"#;
    let mut s = parse(doc).unwrap();
    let p = s.photo("a.jpg");
    assert_eq!(p.caption, Field::WrongType(json!(false)));
    assert_eq!(p.tags, Field::WrongType(json!(["ok", 1])));
    assert_eq!(p.date, Field::WrongType(json!("2026-02-30T00:00:00")));
    assert_eq!(s.folder().date, Field::WrongType(json!("June")));
    let pointers: Vec<String> = s.problems().into_iter().map(|p| p.pointer).collect();
    assert_eq!(
        pointers,
        [
            "/folder/description",
            "/folder/date",
            "/folder/place",
            "/folder/hidden",
            "/photos/a.jpg/caption",
            "/photos/a.jpg/tags",
            "/photos/a.jpg/hidden",
            "/photos/a.jpg/edits",
            "/photos/a.jpg/redo",
            "/photos/a.jpg/date"
        ]
    );
    // Printing keeps every one of them as it was.
    let again: Value = serde_json::from_slice(&s.print()).unwrap();
    assert_eq!(again, serde_json::from_str::<Value>(doc).unwrap());
    // A folder that is not an object is never written over.
    let mut t = parse(r#"{"folder": "Lisbon"}"#).unwrap();
    assert_eq!(t.set_folder(&FolderValue::Hidden(true)), Err(WrongType { pointer: "/folder".into() }));
    assert_eq!(t.set_folder(&FolderValue::Hidden(false)), Err(WrongType { pointer: "/folder".into() }));
    assert_eq!(t.problems()[0], Problem { pointer: "/folder".into(), expected: "an object" });
    assert_eq!(t.folder().hidden, Field::Absent);
    assert_eq!(s.set_photo("a.jpg", &PhotoValue::Caption("now right".into())), Ok(()));
    assert_eq!(s.photo("a.jpg").caption, Field::Value("now right".into()));
}

/// A value for one of the keys Dalica reads: often of the right type, sometimes not.
fn photo_entry() -> impl Strategy<Value = Map<String, Value>> {
    let strings = proptest::collection::vec("\\PC{0,8}", 0..4);
    let key_value = prop_oneof![
        any::<bool>().prop_map(|b| ("star".to_string(), Value::Bool(b))),
        "\\PC{0,20}".prop_map(|s| ("caption".to_string(), Value::String(s))),
        strings.clone().prop_map(|l| ("tags".to_string(), json!(l))),
        any::<bool>().prop_map(|b| ("hidden".to_string(), Value::Bool(b))),
        strings.clone().prop_map(|l| ("edits".to_string(), json!(l))),
        strings.prop_map(|l| ("redo".to_string(), json!(l))),
        (1u16..=9999, 1u8..=12, 1u8..=28, 0u8..24).prop_map(|(y, m, d, h)| ("date".to_string(), json!(format!("{y:04}-{m:02}-{d:02}T{h:02}:00:00")))),
        (prop::sample::select(PHOTO_KEYS.to_vec()), json_value()).prop_map(|(k, v)| (k.to_string(), v)),
        ("\\PC{0,6}", json_value()),
    ];
    proptest::collection::vec(key_value, 0..10).prop_map(|kv| kv.into_iter().collect())
}

fn photo_value() -> impl Strategy<Value = PhotoValue> {
    let strings = proptest::collection::vec("\\PC{0,8}", 0..4);
    prop_oneof![
        any::<bool>().prop_map(PhotoValue::Star),
        "\\PC{0,20}".prop_map(PhotoValue::Caption),
        strings.clone().prop_map(PhotoValue::Tags),
        any::<bool>().prop_map(PhotoValue::Hidden),
        strings.clone().prop_map(PhotoValue::Edits),
        strings.prop_map(PhotoValue::Redo),
        proptest::option::of((1u16..=9999, 1u8..=12, 1u8..=28, 0u8..24, 0u8..60))
            .prop_map(|d| PhotoValue::Date(d.map(|(y, m, day, h, mi)| DateTime::new(Date::new(y, m, day).unwrap(), h, mi, 0).unwrap()))),
        proptest::option::of(0i64..253_402_300_799_999).prop_map(|t| PhotoValue::Changed(t.map(dalica_core::Timestamp::from_millis))),
    ]
}

/// What `photo(name)` should say about the key `v` sets.
fn reads_back(s: &Sidecar, name: &str, v: &PhotoValue) -> bool {
    let p = s.photo(name);
    let empty = |f: &Field<bool>| matches!(f, Field::Absent);
    match v {
        PhotoValue::Star(b) => {
            if *b {
                p.star == Field::Value(true)
            } else {
                empty(&p.star)
            }
        }
        PhotoValue::Hidden(b) => {
            if *b {
                p.hidden == Field::Value(true)
            } else {
                empty(&p.hidden)
            }
        }
        PhotoValue::Caption(c) => {
            if c.is_empty() {
                p.caption == Field::Absent
            } else {
                p.caption == Field::Value(c.clone())
            }
        }
        PhotoValue::Tags(l) => {
            if l.is_empty() {
                p.tags == Field::Absent
            } else {
                p.tags == Field::Value(l.clone())
            }
        }
        PhotoValue::Edits(l) => {
            if l.is_empty() {
                p.edits == Field::Absent
            } else {
                p.edits == Field::Value(l.clone())
            }
        }
        PhotoValue::Redo(l) => {
            if l.is_empty() {
                p.redo == Field::Absent
            } else {
                p.redo == Field::Value(l.clone())
            }
        }
        PhotoValue::Date(d) => match d {
            Some(d) => p.date == Field::Value(*d),
            None => p.date == Field::Absent,
        },
        PhotoValue::Changed(t) => match t {
            Some(t) => p.changed == Field::Value(*t),
            None => p.changed == Field::Absent,
        },
    }
}

proptest! {
    #![proptest_config(cases(512, 32))]

    /// Sidecars with every M6 field, of any type: printing round-trips, reading never panics, and
    /// a value set reads back exactly while every other key keeps its value.
    #[test]
    fn m6_fields_round_trip_and_never_panic(
        entries in proptest::collection::btree_map("\\PC{1,12}|[\\]\";=.]{1,4}", photo_entry(), 0..4),
        folder in proptest::option::of(json_value()),
        edits in proptest::collection::vec(("\\PC{1,12}", photo_value()), 0..6),
    ) {
        let mut root = Map::new();
        root.insert("photos".into(), Value::Object(entries.iter().map(|(k, v)| (k.clone(), Value::Object(v.clone()))).collect()));
        if let Some(f) = folder {
            root.insert("folder".into(), f);
        }
        let mut s = Sidecar { root };
        let printed = s.print();
        let back = Sidecar::parse(&printed, &Limits::default()).unwrap();
        prop_assert_eq!(&back, &s);
        for name in entries.keys() {
            let _ = back.photo(name);
        }
        let _ = (back.folder(), back.problems());
        for (name, v) in &edits {
            let before: Value = serde_json::from_slice(&s.print()).unwrap();
            if s.set_photo(name, v).is_ok() {
                let again = Sidecar::parse(&s.print(), &Limits::default()).unwrap();
                prop_assert!(reads_back(&again, name, v), "{:?} on {:?}", v, name);
                // Every other entry and key is as it was.
                let after: Value = serde_json::from_slice(&s.print()).unwrap();
                for (k, e) in before["photos"].as_object().into_iter().flatten() {
                    if k != name {
                        prop_assert_eq!(&after["photos"][k], e);
                    } else if let (Some(e), Some(a)) = (e.as_object(), after["photos"][k].as_object()) {
                        let key = v.key();
                        for (kk, vv) in e.iter().filter(|(kk, _)| kk.as_str() != key) {
                            prop_assert_eq!(a.get(kk), Some(vv));
                        }
                    }
                }
                prop_assert_eq!(&after["folder"], &before["folder"]);
            }
        }
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
    #![proptest_config(cases(1024, 256))]

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
