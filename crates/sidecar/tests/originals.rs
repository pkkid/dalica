//! The originals record (`.dalica-originals/.dalica.json`, `data-model.md` §6) and M8's keys in
//! the folder sidecar (`savedAs`, `"folder": {"project"}`): round trips with hostile names and
//! values of every type never panic, keep what they cannot use, and a name that is not one plain
//! path component (`../x`, `CON`) is kept in the file but never offered as an entry
//! (`security.md` §5.4).

use dalica_sidecar::fields::{M8_FOLDER_KEYS, M8_PHOTO_KEYS};
use dalica_sidecar::originals::{ENTRY_KEYS, Entry, Refused, State};
use dalica_sidecar::{Field, Limits, Originals, SavedAs, Sidecar, plain_name};
use proptest::prelude::*;
use serde_json::{Value, json};

fn parse(text: &str) -> Originals {
    Originals::parse(text.as_bytes(), &Limits::default()).unwrap()
}

fn entry(size: u64) -> Entry {
    Entry {
        recipe: vec!["rotate(90)".into(), "frobnicate(7)".into(), "autofix()".into()],
        size,
        mtime: 1_780_000_000_123,
        width: 4000,
        height: 3000,
        digest: "md5:9e107d9d372bb6826bd81d3542a419d6".into(),
        state: State::Done,
        time: Some(1_791_000_000_000),
    }
}

#[test]
fn a_record_prints_in_the_documented_key_order_and_reads_back() {
    let mut r = Originals::new();
    r.set("IMG_0001.jpg", &entry(2_483_112)).unwrap();
    let text = String::from_utf8(r.print()).unwrap();
    let want = r#"{
  "dalica": {
    "version": 1
  },
  "originals": {
    "IMG_0001.jpg": {
      "digest": "md5:9e107d9d372bb6826bd81d3542a419d6",
      "height": 3000,
      "mtime": 1780000000123,
      "recipe": [
        "rotate(90)",
        "frobnicate(7)",
        "autofix()"
      ],
      "size": 2483112,
      "state": "done",
      "time": 1791000000000,
      "width": 4000
    }
  }
}
"#;
    assert_eq!(text, want);
    let back = parse(&text);
    assert_eq!(back, r);
    assert_eq!(back.entry("IMG_0001.jpg"), Field::Value(entry(2_483_112)));
    assert_eq!(back.entries(), vec![("IMG_0001.jpg".to_string(), entry(2_483_112))]);
    assert!(back.problems().is_empty());
    assert_eq!(back.version(), Some(1));
}

#[test]
fn optional_keys_have_defaults_and_unknown_keys_survive_a_rewrite() {
    let mut r = parse(
        r#"{"future": {"x": 1}, "dalica": {"version": 1, "note": "hi"},
            "originals": {"a.jpg": {"size": 5, "mtime": -3, "state": "pending", "colour": "teal"}}}"#,
    );
    let want = Entry { size: 5, mtime: -3, ..Entry::default() };
    assert_eq!(r.entry("a.jpg"), Field::Value(want.clone()));
    // Marking it done keeps the entry's unknown key and the document's.
    r.set("a.jpg", &Entry { state: State::Done, ..want }).unwrap();
    let back = parse(std::str::from_utf8(&r.print()).unwrap());
    let doc: Value = serde_json::from_slice(&back.print()).unwrap();
    assert_eq!(doc["originals"]["a.jpg"]["colour"], json!("teal"));
    assert_eq!(doc["future"], json!({"x": 1}));
    assert_eq!(doc["dalica"]["note"], json!("hi"));
    assert!(doc["originals"]["a.jpg"].get("time").is_none(), "no time is written as no key");
    // Removing the last entry takes the empty "originals" object with it.
    assert_eq!(r.remove("a.jpg"), Ok(true));
    assert_eq!(r.remove("a.jpg"), Ok(false));
    assert!(r.is_empty());
    let doc: Value = serde_json::from_slice(&r.print()).unwrap();
    assert!(doc.get("originals").is_none());
}

#[test]
fn names_that_are_not_plain_are_kept_but_never_reach_a_file() {
    let text = r#"{"originals": {
        "../x": {"size": 1, "mtime": 0, "state": "done"},
        "CON": {"size": 1, "mtime": 0, "state": "done"},
        "con.jpg": {"size": 1, "mtime": 0, "state": "done"},
        "a/b.jpg": {"size": 1, "mtime": 0, "state": "done"},
        "C:x.jpg": {"size": 1, "mtime": 0, "state": "done"},
        "": {"size": 1, "mtime": 0, "state": "done"},
        "ok.jpg": {"size": 1, "mtime": 0, "state": "done"}
    }}"#;
    let mut r = parse(text);
    let names: Vec<String> = r.entries().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["ok.jpg"]);
    let bad: Vec<String> = r.problems().into_iter().filter(|p| p.expected == "a plain file name").map(|p| p.pointer).collect();
    assert_eq!(bad.len(), 6, "{bad:?}");
    assert!(bad.contains(&"/originals/..~1x".to_string()) && bad.contains(&"/originals/CON".to_string()));
    // Writing under such a name is refused; the entries stay in the file through a rewrite.
    assert_eq!(r.set("../x", &entry(9)), Err(Refused::Name("../x".into())));
    assert_eq!(r.remove("CON"), Err(Refused::Name("CON".into())));
    r.set("ok.jpg", &entry(9)).unwrap();
    let back = parse(std::str::from_utf8(&r.print()).unwrap());
    assert_eq!(back.names().count(), 7);
    assert_eq!(back.entry("../x"), Field::Value(Entry { size: 1, state: State::Done, ..Entry::default() }));
}

#[test]
fn wrong_types_are_kept_and_reported_and_the_rest_is_read() {
    let text = r#"{"dalica": "v1", "originals": {
        "a.jpg": {"size": "big", "mtime": 0, "state": "done"},
        "b.jpg": {"size": 1, "mtime": 0, "state": "finished"},
        "c.jpg": {"size": 1, "state": "done"},
        "d.jpg": [1, 2],
        "e.jpg": {"size": 1, "mtime": 0, "state": "done", "width": 5000000000, "recipe": ["x()", 3]},
        "f.jpg": {"size": 7, "mtime": 0, "state": "pending"}
    }}"#;
    let r = parse(text);
    for name in ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"] {
        assert!(matches!(r.entry(name), Field::WrongType(_)), "{name}");
    }
    assert_eq!(r.entries(), vec![("f.jpg".to_string(), Entry { size: 7, ..Entry::default() })]);
    let pointers: Vec<String> = r.problems().into_iter().map(|p| p.pointer).collect();
    for p in [
        "/dalica",
        "/originals/a.jpg/size",
        "/originals/b.jpg/state",
        "/originals/c.jpg/mtime",
        "/originals/d.jpg",
        "/originals/e.jpg/recipe",
        "/originals/e.jpg/width",
    ] {
        assert!(pointers.contains(&p.to_string()), "{p} in {pointers:?}");
    }
    assert_eq!(pointers.len(), 7, "{pointers:?}");
    // A wrong-type entry is never written over by a set elsewhere, and refuses a set of its own.
    let mut r = r;
    r.set("g.jpg", &entry(1)).unwrap();
    assert!(matches!(r.set("d.jpg", &entry(1)), Err(Refused::WrongType(_))));
    let back = parse(std::str::from_utf8(&r.print()).unwrap());
    assert_eq!(back.entry("d.jpg"), Field::WrongType(json!([1, 2])));
    // `"originals"` itself of the wrong type: kept, nothing usable, every write refused.
    let mut odd = parse(r#"{"originals": [1]}"#);
    assert!(odd.entries().is_empty() && odd.problems().len() == 1);
    assert!(matches!(odd.set("a.jpg", &entry(1)), Err(Refused::WrongType(_))));
    assert!(matches!(odd.remove("a.jpg"), Err(Refused::WrongType(_))));
}

#[test]
fn damaged_records_are_parse_errors() {
    for bad in ["", "{", "[]", "\"x\"", "{\"originals\": {\"a\": 1,}}", "\u{feff}"] {
        assert!(Originals::parse(bad.as_bytes(), &Limits::default()).is_err(), "{bad:?}");
    }
    let tiny = Limits { max_photos: 1, ..Limits::default() };
    assert!(Originals::parse(br#"{"originals": {"a": {}, "b": {}}}"#, &tiny).is_err());
    // A BOM and CRLF are fine.
    assert!(Originals::parse(b"\xef\xbb\xbf{\r\n\"originals\": {}\r\n}", &Limits::default()).is_ok());
}

#[test]
fn saved_as_and_the_project_mark_round_trip_in_the_folder_sidecar() {
    assert_eq!(M8_PHOTO_KEYS, ["savedAs"]);
    assert_eq!(M8_FOLDER_KEYS, ["project"]);
    let mut s = Sidecar::new();
    let copy = SavedAs { name: "IMG_0001.jpg".into(), recipe: vec!["sepia()".into()] };
    s.set_saved_as("IMG_0001.CR2", Some(&copy)).unwrap();
    s.set_project(Some("exported")).unwrap();
    let text = String::from_utf8(s.print()).unwrap();
    assert!(text.contains("\"folder\": {\n    \"project\": \"exported\"\n  }"), "{text}");
    assert!(
        text.contains("\"savedAs\": {\n        \"name\": \"IMG_0001.jpg\",\n        \"recipe\": [\n          \"sepia()\"\n        ]\n      }"),
        "{text}"
    );
    let mut back = Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(back.saved_as("IMG_0001.CR2"), Field::Value(copy.clone()));
    assert_eq!(back.project(), Field::Value("exported".into()));
    assert!(back.problems().is_empty());
    // Unknown keys inside savedAs survive a new copy; clearing removes the key and the empty entry.
    let mut doc: Value = serde_json::from_slice(&back.print()).unwrap();
    doc["photos"]["IMG_0001.CR2"]["savedAs"]["quality"] = json!(88);
    back = Sidecar::parse(doc.to_string().as_bytes(), &Limits::default()).unwrap();
    let again = SavedAs { name: "IMG_0001-001.jpg".into(), recipe: Vec::new() };
    back.set_saved_as("IMG_0001.CR2", Some(&again)).unwrap();
    let doc: Value = serde_json::from_slice(&back.print()).unwrap();
    assert_eq!(doc["photos"]["IMG_0001.CR2"]["savedAs"], json!({"name": "IMG_0001-001.jpg", "quality": 88}));
    assert_eq!(back.saved_as("IMG_0001.CR2"), Field::Value(again));
    back.set_saved_as("IMG_0001.CR2", None).unwrap();
    back.set_project(None).unwrap();
    // As with every photo key, the emptied entry goes and `photos` stays.
    let doc: Value = serde_json::from_slice(&back.print()).unwrap();
    assert_eq!(doc, json!({"dalica": {"version": 1}, "photos": {}}));
}

#[test]
fn saved_as_and_project_of_the_wrong_type_are_kept_and_reported() {
    let text = r#"{"folder": {"project": 3}, "photos": {
        "a.jpg": {"savedAs": "b.jpg"},
        "b.jpg": {"savedAs": {"name": ""}},
        "c.jpg": {"savedAs": {"name": "c.jpg", "recipe": "x()"}},
        "d.jpg": {"savedAs": {"name": "../d.jpg"}}
    }}"#;
    let mut s = Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap();
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        assert!(matches!(s.saved_as(name), Field::WrongType(_)), "{name}");
    }
    // The sidecar reads a name as written; whether it reaches a file is the reader's check.
    assert_eq!(s.saved_as("d.jpg"), Field::Value(SavedAs { name: "../d.jpg".into(), recipe: Vec::new() }));
    assert!(!plain_name("../d.jpg"));
    assert!(matches!(s.project(), Field::WrongType(_)));
    let pointers: Vec<String> = s.problems().into_iter().map(|p| p.pointer).collect();
    assert_eq!(pointers, ["/folder/project", "/photos/a.jpg/savedAs", "/photos/b.jpg/savedAs", "/photos/c.jpg/savedAs"]);
    // Setting over a value that is not an object is refused; it is kept.
    assert!(s.set_saved_as("a.jpg", Some(&SavedAs { name: "x.jpg".into(), recipe: Vec::new() })).is_err());
    assert_eq!(Sidecar::parse(&s.print(), &Limits::default()).unwrap(), s);
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        (-1e9f64..1e9).prop_map(Value::from),
        "\\PC{0,8}".prop_map(Value::String),
        prop::sample::select(vec!["pending", "done", "Done", ""]).prop_map(|s| Value::String(s.into())),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            proptest::collection::btree_map("\\PC{0,6}", inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// A hostile name: anything at all, or one of the shapes `security.md` §5.4 names.
fn name() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\PC{0,12}",
        prop::sample::select(vec!["../x", "..", ".", "CON", "nul.txt", "a/b", "a\\b", "C:x", "x:y", "", "a\u{0}b", "IMG_0001.jpg", "東京.jpg"])
            .prop_map(str::to_string),
    ]
}

fn an_entry() -> impl Strategy<Value = Entry> {
    (
        proptest::collection::vec("\\PC{0,10}", 0..4),
        any::<u64>(),
        any::<i64>(),
        any::<u32>(),
        any::<u32>(),
        "\\PC{0,20}",
        any::<bool>(),
        proptest::option::of(any::<i64>()),
    )
        .prop_map(|(recipe, size, mtime, width, height, digest, done, time)| Entry {
            recipe,
            size,
            mtime,
            width,
            height,
            digest,
            state: if done { State::Done } else { State::Pending },
            time,
        })
}

/// An entry whose known keys hold anything at all.
fn a_raw_entry() -> impl Strategy<Value = Value> {
    let key = prop_oneof![prop::sample::select(ENTRY_KEYS.to_vec()).prop_map(str::to_string), "\\PC{0,6}"];
    prop_oneof![proptest::collection::btree_map(key, json_value(), 0..10).prop_map(|m| Value::Object(m.into_iter().collect())), json_value(),]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Entries set under any name read back exactly when the name is plain, are refused when it is
    /// not, and the document survives print → parse.
    #[test]
    fn set_entries_round_trip(items in proptest::collection::vec((name(), an_entry()), 0..8)) {
        let mut r = Originals::new();
        let mut want = std::collections::BTreeMap::new();
        for (n, e) in &items {
            match r.set(n, e) {
                Ok(()) => {
                    prop_assert!(plain_name(n));
                    want.insert(n.clone(), e.clone());
                }
                Err(Refused::Name(bad)) => prop_assert!(!plain_name(&bad)),
                Err(other) => prop_assert!(false, "{other:?}"),
            }
        }
        let back = Originals::parse(&r.print(), &Limits::default()).unwrap();
        prop_assert_eq!(&back, &r);
        prop_assert!(back.problems().is_empty(), "{:?}", back.problems());
        // Both in name order (a `String`'s order is its bytes').
        let want: Vec<(String, Entry)> = want.into_iter().collect();
        prop_assert_eq!(back.entries(), want);
    }

    /// A record holding anything under any name: reading never panics, an entry is used only when
    /// its name is plain and its known keys are right, problems point at the rest, and a rewrite
    /// of other entries keeps every one of them as it was.
    #[test]
    fn hostile_records_never_panic_and_keep_what_they_cannot_use(
        raw in proptest::collection::btree_map(name(), a_raw_entry(), 0..8),
        extra in an_entry(),
    ) {
        let doc = json!({ "dalica": { "version": 1 }, "originals": Value::Object(raw.clone().into_iter().collect()) });
        let Ok(mut r) = Originals::parse(doc.to_string().as_bytes(), &Limits::default()) else {
            // Only the limits can refuse it (deep nesting); nothing else is lost.
            return Ok(());
        };
        let usable: Vec<String> = r.entries().into_iter().map(|(n, _)| n).collect();
        for n in raw.keys() {
            let used = usable.contains(n);
            let read = matches!(r.entry(n), Field::Value(_));
            prop_assert_eq!(used, read && plain_name(n), "{}", n);
            let at = format!("/originals/{}", n.replace('~', "~0").replace('/', "~1"));
            let reported = r.problems().iter().any(|p| p.pointer == at || p.pointer.starts_with(&format!("{at}/")));
            prop_assert_eq!(reported, !used, "{}", n);
        }
        // Writing one more entry under a fresh plain name keeps every other one byte for byte.
        let fresh = "zz-fresh.jpg";
        prop_assume!(!raw.contains_key(fresh));
        r.set(fresh, &extra).unwrap();
        let back: Value = serde_json::from_slice(&r.print()).unwrap();
        for (n, v) in &raw {
            prop_assert_eq!(&back["originals"][n.as_str()], v);
        }
        prop_assert_eq!(Originals::parse(&r.print(), &Limits::default()).unwrap().entry(fresh), Field::Value(extra));
    }

    /// Arbitrary bytes never panic the parser.
    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        if let Ok(r) = Originals::parse(&bytes, &Limits::default()) {
            let _ = (r.entries(), r.problems(), r.print());
        }
    }

    /// `savedAs` and `project` holding anything: reading never panics, a set refused or done never
    /// touches another photo's keys, and the document survives print → parse.
    #[test]
    fn hostile_saved_as_and_project_never_panic(
        saved_as in json_value(),
        project in json_value(),
        name in name(),
        copy in ("\\PC{1,12}", proptest::collection::vec("\\PC{0,8}", 0..3)),
    ) {
        let doc = json!({ "folder": { "project": project }, "photos": { "other.jpg": { "savedAs": saved_as.clone(), "star": true } } });
        let mut s = Sidecar::parse(doc.to_string().as_bytes(), &Limits::default()).unwrap();
        let _ = (s.saved_as("other.jpg"), s.project(), s.problems());
        let copy = SavedAs { name: copy.0, recipe: copy.1 };
        if name != "other.jpg" && s.set_saved_as(&name, Some(&copy)).is_ok() {
            prop_assert_eq!(s.saved_as(&name), Field::Value(copy));
        }
        let _ = s.set_project(Some("exported"));
        let back = Sidecar::parse(&s.print(), &Limits::default()).unwrap();
        prop_assert_eq!(&back, &s);
        let doc: Value = serde_json::from_slice(&back.print()).unwrap();
        prop_assert_eq!(&doc["photos"]["other.jpg"]["savedAs"], &saved_as);
    }
}
