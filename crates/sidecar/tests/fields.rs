//! M10's key on disk (`data-model.md` §3): a photo's `changed`, the UTC time Dalica last changed
//! what is said about it, with milliseconds. It round-trips exactly; a value that is not such a
//! time is kept and reported, never used; writing it keeps the entry's other keys, Dalica's and
//! anyone else's; clearing it removes only it.

use dalica_core::{DateTime, Timestamp};
use dalica_sidecar::fields::{CHANGED, M10_PHOTO_KEYS};
use dalica_sidecar::{Field, Limits, PhotoValue, Problem, Sidecar, parse_utc, utc_text};
use proptest::prelude::*;
use serde_json::{Value, json};

use crate::common::cases;

fn parse(text: &str) -> Sidecar {
    Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap()
}

fn json_of(s: &Sidecar) -> Value {
    serde_json::from_slice(&s.print()).unwrap()
}

#[test]
fn the_m10_key_is_the_documented_one_and_reads_as_utc_with_milliseconds() {
    assert_eq!((M10_PHOTO_KEYS, CHANGED), (["changed"], "changed"));
    let t = Timestamp::from_millis(1_792_057_267_250);
    assert_eq!(utc_text(t).as_deref(), Some("2026-10-15T09:41:07.250Z"));
    assert_eq!(parse_utc("2026-10-15T09:41:07.250Z"), Some(t));
    assert_eq!(parse_utc("2026-10-15T09:41:07Z"), Some(Timestamp::from_millis(1_792_057_267_000)), "milliseconds may be left out");
    assert_eq!(utc_text(Timestamp::from_millis(-1)).as_deref(), Some("1969-12-31T23:59:59.999Z"));
    for bad in [
        "2026-10-15T09:41:07.250",
        "2026-10-15T09:41:07.25Z",
        "2026-10-15T09:41:07.2500Z",
        "2026-10-15T09:41:07.25xZ",
        "2026-13-01T00:00:00Z",
        "",
        "Z",
    ] {
        assert_eq!(parse_utc(bad), None, "{bad:?}");
    }
    assert_eq!(utc_text(Timestamp::from_millis(i64::MAX)), None, "past the year 9999");
}

#[test]
fn changed_round_trips_and_keeps_the_entrys_other_keys() {
    let mut s = parse(r#"{"photos": {"a.jpg": {"star": true, "x-other": [1, 2]}}}"#);
    assert_eq!(s.photo("a.jpg").changed, Field::Absent, "no Dalica change recorded");
    let t = Timestamp::from_millis(1_792_057_267_250);
    s.set_photo("a.jpg", &PhotoValue::Changed(Some(t))).unwrap();
    assert_eq!(json_of(&s)["photos"]["a.jpg"], json!({ "star": true, "x-other": [1, 2], "changed": "2026-10-15T09:41:07.250Z" }));
    let again = Sidecar::parse(&s.print(), &Limits::default()).unwrap();
    assert_eq!(again.photo("a.jpg").changed, Field::Value(t));
    // Cleared: only the key goes; an entry left with nothing goes too.
    s.set_photo("a.jpg", &PhotoValue::Changed(None)).unwrap();
    assert_eq!(json_of(&s)["photos"]["a.jpg"], json!({ "star": true, "x-other": [1, 2] }));
    let mut only = parse(r#"{"photos": {"b.jpg": {"changed": "2026-10-15T09:41:07.250Z"}}}"#);
    only.set_photo("b.jpg", &PhotoValue::Changed(None)).unwrap();
    assert!(json_of(&only).get("photos").is_none_or(|p| p.get("b.jpg").is_none()));
    // A time no date can spell is written as no key.
    s.set_photo("a.jpg", &PhotoValue::Changed(Some(Timestamp::from_millis(i64::MAX)))).unwrap();
    assert!(json_of(&s)["photos"]["a.jpg"].get("changed").is_none());
}

#[test]
fn a_changed_that_is_not_a_utc_time_is_kept_and_reported() {
    for bad in [json!(1_792_057_267_250_i64), json!("yesterday"), json!("2026-10-15T09:41:07"), json!({ "t": 1 })] {
        let text = json!({ "photos": { "a.jpg": { "changed": bad.clone(), "star": true } } }).to_string();
        let s = parse(&text);
        assert_eq!(s.photo("a.jpg").changed, Field::WrongType(bad.clone()), "{bad}");
        assert_eq!(s.photo("a.jpg").star, Field::Value(true), "{bad}: the rest is read");
        let problems = s.problems();
        assert!(
            problems.contains(&Problem { pointer: "/photos/a.jpg/changed".into(), expected: "a UTC time (2026-10-15T09:41:07.250Z)" }),
            "{bad}: {problems:?}"
        );
        assert_eq!(json_of(&s)["photos"]["a.jpg"]["changed"], bad, "kept as it is");
    }
}

proptest! {
    #![proptest_config(cases(256, 32))]

    /// Every time from the year 1 to 9999 round-trips through its text exactly, and the text sorts
    /// as the times do within a year of four digits.
    #[test]
    fn every_time_in_range_round_trips(ms in -62_135_596_800_000_i64..253_402_300_799_999_i64) {
        let t = Timestamp::from_millis(ms);
        let text = utc_text(t).unwrap();
        prop_assert_eq!(parse_utc(&text), Some(t));
        let d: DateTime = text.trim_end_matches('Z').split('.').next().unwrap().parse().unwrap();
        prop_assert_eq!(DateTime::from_unix_secs(t.as_secs()), Some(d));
    }
}

// M13's keys (`data-model.md` §3): a photo's `faces` and `geo`, the folder's `people`.

mod m13 {
    use std::collections::BTreeMap;

    use dalica_core::PersonId;
    use dalica_sidecar::fields::{FACES, GEO, IGNORED, M13_PHOTO_KEYS, M13_TOP_KEYS, MAX_FACES, PEOPLE};
    use dalica_sidecar::{Face, FacePerson, FaceRect, Field, FolderPeople, Geo, Limits, PersonName, PhotoFaces, Problem, Sidecar};
    use proptest::prelude::*;
    use serde_json::{Value, json};

    use super::{json_of, parse};
    use crate::common::cases;

    fn pid(s: &str) -> PersonId {
        PersonId::parse(s).unwrap()
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> FaceRect {
        FaceRect::new(x0, y0, x1, y1).unwrap()
    }

    fn reparse(s: &Sidecar) -> Sidecar {
        Sidecar::parse(&s.print(), &Limits::default()).unwrap()
    }

    fn ana() -> BTreeMap<PersonId, PersonName> {
        BTreeMap::from([
            (pid("p7f3a2c"), PersonName { name: "Ana Costa".into(), nickname: None }),
            (pid("p0b1d"), PersonName { name: "Rui Lopes".into(), nickname: Some("Ruizinho".into()) }),
        ])
    }

    fn lisbon_faces() -> Vec<Face> {
        vec![
            Face { rect: rect(0.41, 0.22, 0.52, 0.38), person: FacePerson::Named(pid("p7f3a2c")) },
            Face { rect: rect(0.70, 0.30, 0.78, 0.44), person: FacePerson::Ignored },
            Face { rect: rect(0.0, 0.0, 1.0, 1.0), person: FacePerson::Unnamed },
        ]
    }

    #[test]
    fn the_m13_keys_are_the_documented_ones() {
        assert_eq!((M13_PHOTO_KEYS, M13_TOP_KEYS, IGNORED, MAX_FACES), (["faces", "geo"], ["people"], "ignored", 256));
        assert_eq!((FACES, GEO, PEOPLE), ("faces", "geo", "people"));
    }

    #[test]
    fn faces_people_and_a_geotag_print_and_parse_back_equal() {
        let mut s = Sidecar::new();
        s.set_photo_faces("IMG_0001.jpg", &lisbon_faces()).unwrap();
        s.set_geo("IMG_0001.jpg", Geo::new(38.7116, -9.1389)).unwrap();
        s.set_people(&ana());
        let again = reparse(&s);
        assert_eq!(again, s);
        assert_eq!(again.photo_faces("IMG_0001.jpg"), Field::Value(PhotoFaces { faces: lisbon_faces(), bad: vec![], over: 0 }));
        let geo = again.geo("IMG_0001.jpg");
        assert_eq!(geo, Field::Value(Geo::new(38.7116, -9.1389).unwrap()));
        let Field::Value(g) = geo else { unreachable!() };
        assert_eq!((g.lat(), g.lon()), (38.7116, -9.1389));
        assert_eq!(again.people(), Field::Value(FolderPeople { people: ana(), bad: vec![] }));
        assert!(again.problems().is_empty(), "{:?}", again.problems());
        // An unnamed face has no `person`; the others name a person id or `ignored`.
        assert_eq!(
            json_of(&s)["photos"]["IMG_0001.jpg"]["faces"],
            json!([
                { "rect": [0.41, 0.22, 0.52, 0.38], "person": "p7f3a2c" },
                { "rect": [0.7, 0.3, 0.78, 0.44], "person": "ignored" },
                { "rect": [0.0, 0.0, 1.0, 1.0] }
            ])
        );
        // Clearing every key removes the photo's entry and the folder's people.
        s.set_photo_faces("IMG_0001.jpg", &[]).unwrap();
        s.set_geo("IMG_0001.jpg", None).unwrap();
        s.set_people(&BTreeMap::new());
        let emptied = json_of(&s);
        assert_eq!(emptied, json!({ "dalica": { "version": 1 }, "photos": {} }));
        // Clearing what is not there creates nothing.
        s.set_photo_faces("b.jpg", &[]).unwrap();
        s.set_geo("b.jpg", None).unwrap();
        assert_eq!(json_of(&s), emptied);
        let mut fresh = Sidecar::new();
        fresh.set_photo_faces("b.jpg", &[]).unwrap();
        fresh.set_geo("b.jpg", None).unwrap();
        assert_eq!(fresh, Sidecar::new());
    }

    #[test]
    fn the_sorted_pretty_output_is_byte_stable_with_six_decimals() {
        let mut s = Sidecar::new();
        let faces = [Face { rect: rect(0.123_456_7, 0.25, 0.5, 0.999_999_9), person: FacePerson::Named(pid("p7f3a2c")) }];
        s.set_photo_faces("a.jpg", &faces).unwrap();
        s.set_geo("a.jpg", Geo::new(-0.000_000_1, 179.999_999_96)).unwrap();
        s.set_people(&BTreeMap::from([(pid("p7f3a2c"), PersonName { name: "Ana".into(), nickname: Some("Aninhas".into()) })]));
        let expected = r#"{
  "dalica": {
    "version": 1
  },
  "people": {
    "p7f3a2c": {
      "name": "Ana",
      "nickname": "Aninhas"
    }
  },
  "photos": {
    "a.jpg": {
      "faces": [
        {
          "person": "p7f3a2c",
          "rect": [
            0.123457,
            0.25,
            0.5,
            1.0
          ]
        }
      ],
      "geo": [
        0.0,
        180.0
      ]
    }
  }
}
"#;
        assert_eq!(String::from_utf8(s.print()).unwrap(), expected);
        let again = reparse(&s);
        assert_eq!(again.print(), s.print(), "printing what was read gives the same bytes");
        let mut rewritten = again.clone();
        let Field::Value(read) = again.photo_faces("a.jpg") else { panic!("faces") };
        rewritten.set_photo_faces("a.jpg", &read.faces).unwrap();
        let Field::Value(g) = again.geo("a.jpg") else { panic!("geo") };
        rewritten.set_geo("a.jpg", Some(g)).unwrap();
        assert_eq!(rewritten.print(), s.print(), "writing what was read changes no byte");
    }

    #[test]
    fn each_hostile_face_is_skipped_with_the_rest_of_the_photo_kept() {
        let hostile = [
            json!({ "rect": [0.5, 0.2, 0.4, 0.3] }),                         // inverted
            json!({ "rect": [0.1, 0.1, 1.2, 0.3] }),                         // outside the photo
            json!({ "rect": [-0.1, 0.1, 0.2, 0.3] }),                        // outside the photo
            json!({ "rect": [0.1, 0.1, 1e300, 0.3] }),                       // huge (JSON has no NaN or infinity: FaceRect::new refuses those)
            json!({ "rect": [0.1, 0.1, 0.2] }),                              // three corners
            json!({ "rect": [0.1, "0.1", 0.2, 0.3] }),                       // not a number
            json!({ "rect": [0.1, 0.1, 0.2, 0.3], "person": "../p7f3a2c" }), // not a person id
            json!({ "rect": [0.1, 0.1, 0.2, 0.3], "person": "a91c0e" }),     // an album id
            json!({ "rect": [0.1, 0.1, 0.2, 0.3], "person": "Ignored" }),
            json!({ "rect": [0.1, 0.1, 0.2, 0.3], "person": 7 }),
            json!({ "person": "p7f3a2c" }), // no rectangle
            json!([0.1, 0.1, 0.2, 0.3]),
            json!(null),
        ];
        let good = json!({ "rect": [0.1, 0.1, 0.2, 0.3], "person": "p7f3a2c" });
        for bad in hostile {
            let text = json!({ "photos": { "a.jpg": { "faces": [bad.clone(), good.clone()], "star": true, "geo": [1.5, 2.5] } } }).to_string();
            let mut s = parse(&text);
            let want =
                PhotoFaces { faces: vec![Face { rect: rect(0.1, 0.1, 0.2, 0.3), person: FacePerson::Named(pid("p7f3a2c")) }], bad: vec![0], over: 0 };
            assert_eq!(s.photo_faces("a.jpg"), Field::Value(want.clone()), "{bad}");
            assert_eq!(s.photo("a.jpg").star, Field::Value(true), "{bad}: the rest is read");
            assert_eq!(s.geo("a.jpg"), Field::Value(Geo::new(1.5, 2.5).unwrap()), "{bad}");
            assert!(s.problems().is_empty(), "{bad}: a bad face is left out, not a wrong type: {:?}", s.problems());
            // Rewriting the faces keeps the unusable entry verbatim, after Dalica's.
            s.set_photo_faces("a.jpg", &want.faces).unwrap();
            assert_eq!(json_of(&s)["photos"]["a.jpg"]["faces"], json!([good.clone(), bad.clone()]), "{bad}");
            // And clearing Dalica's faces still keeps it.
            s.set_photo_faces("a.jpg", &[]).unwrap();
            assert_eq!(json_of(&s)["photos"]["a.jpg"]["faces"], json!([bad.clone()]), "{bad}");
        }
    }

    #[test]
    fn hostile_geotags_and_wrong_types_are_kept_and_reported() {
        let cases = [
            (GEO, json!([90.5, 0.0])),
            (GEO, json!([-91, 0.0])),
            (GEO, json!([0.0, 180.5])),
            (GEO, json!([1e300, 0.0])),
            (GEO, json!([1.0])),
            (GEO, json!([1.0, 2.0, 3.0])),
            (GEO, json!(["38.7", "-9.1"])),
            (GEO, json!({ "lat": 1.0, "lon": 2.0 })),
            (FACES, json!({ "rect": [0.1, 0.1, 0.2, 0.3] })),
            (FACES, json!("p7f3a2c")),
        ];
        for (key, bad) in cases {
            let text = json!({ "photos": { "a.jpg": { key: bad.clone(), "caption": "kept" } } }).to_string();
            let mut s = parse(&text);
            if key == GEO {
                assert_eq!(s.geo("a.jpg"), Field::WrongType(bad.clone()), "{bad}");
            } else {
                assert_eq!(s.photo_faces("a.jpg"), Field::WrongType(bad.clone()), "{bad}");
            }
            assert_eq!(s.photo("a.jpg").caption, Field::Value("kept".into()), "{bad}: the rest is read");
            let pointer = format!("/photos/a.jpg/{key}");
            assert_eq!(s.problems().iter().filter(|p| p.pointer == pointer).count(), 1, "{bad}: reported once: {:?}", s.problems());
            assert_eq!(json_of(&s)["photos"]["a.jpg"][key], bad, "{bad}: kept as it is");
            // Clearing the faces never removes a list it does not understand; a place is one value,
            // set or cleared over whatever is there, as `set_photo` does.
            if key == GEO {
                s.set_geo("a.jpg", None).unwrap();
                assert!(json_of(&s)["photos"]["a.jpg"].get(GEO).is_none(), "{bad}");
            } else {
                s.set_photo_faces("a.jpg", &[]).unwrap();
                assert_eq!(json_of(&s)["photos"]["a.jpg"][key], bad, "{bad}: kept by clearing");
            }
        }
        // A latitude exactly on a pole and a longitude on the antimeridian are places.
        for (lat, lon) in [(90.0, 180.0), (-90.0, -180.0)] {
            assert!(Geo::new(lat, lon).is_some());
        }
        for (lat, lon) in [(f64::NAN, 0.0), (0.0, f64::INFINITY), (90.000_001, 0.0)] {
            assert_eq!(Geo::new(lat, lon), None, "{lat}, {lon}");
        }
        for r in [[f64::NAN, 0.0, 1.0, 1.0], [0.5, 0.0, 0.4, 1.0], [0.0, 0.0, 1.000_001, 1.0]] {
            assert_eq!(FaceRect::new(r[0], r[1], r[2], r[3]), None, "{r:?}");
        }
    }

    #[test]
    fn hostile_people_entries_are_kept_and_left_out() {
        let text = json!({
            "people": {
                "p7f3a2c": { "name": "Ana Costa", "x-color": "red" },
                "p0b1d": { "name": "Rui", "nickname": "" },
                "../p1234": { "name": "Path" },
                "a91c0e": { "name": "Album" },
                "p9999": { "name": "" },
                "p8888": { "name": 7 },
                "p7777": { "name": "Eva", "nickname": 3 },
                "p6666": "Zé"
            },
            "photos": { "a.jpg": { "star": true } }
        })
        .to_string();
        let mut s = parse(&text);
        let Field::Value(read) = s.people() else { panic!("people") };
        let want = BTreeMap::from([
            (pid("p7f3a2c"), PersonName { name: "Ana Costa".into(), nickname: None }),
            (pid("p0b1d"), PersonName { name: "Rui".into(), nickname: None }),
        ]);
        assert_eq!(read.people, want);
        let mut bad = read.bad.clone();
        bad.sort();
        assert_eq!(bad, ["../p1234", "a91c0e", "p6666", "p7777", "p8888", "p9999"]);
        assert!(s.problems().is_empty(), "{:?}", s.problems());
        // Renaming Ana and dropping Rui keeps Ana's other key and every entry left out.
        s.set_people(&BTreeMap::from([(pid("p7f3a2c"), PersonName { name: "Ana C.".into(), nickname: Some("Aninhas".into()) })]));
        let people = &json_of(&s)["people"];
        assert_eq!(people["p7f3a2c"], json!({ "name": "Ana C.", "nickname": "Aninhas", "x-color": "red" }));
        assert!(people.get("p0b1d").is_none());
        for kept in ["../p1234", "a91c0e", "p6666", "p7777", "p8888", "p9999"] {
            assert_eq!(people[kept], json!(text.parse::<Value>().unwrap()["people"][kept]), "{kept}");
        }
        // A `people` that is not an object is a wrong type, kept until people are written.
        let mut wrong = parse(r#"{"people": ["Ana"]}"#);
        assert_eq!(wrong.people(), Field::WrongType(json!(["Ana"])));
        assert!(wrong.problems().contains(&Problem { pointer: "/people".into(), expected: "an object from person id to {\"name\": …}" }));
        wrong.set_people(&BTreeMap::new());
        assert_eq!(json_of(&wrong)["people"], json!(["Ana"]));
        wrong.set_people(&want);
        assert_eq!(wrong.people(), Field::Value(FolderPeople { people: want, bad: vec![] }));
    }

    #[test]
    fn unknown_keys_in_a_face_survive_a_rewrite() {
        let text = json!({ "photos": { "a.jpg": { "faces": [
            { "rect": [0.1, 0.1, 0.2, 0.2], "person": "p7f3a2c", "x-source": "xmp" },
            { "rect": [0.5, 0.5, 0.6, 0.6], "future": { "landmarks": [1, 2] } },
            { "rect": [0.7, 0.7, 0.8, 0.8], "x-note": "gone with its face" }
        ] } } })
        .to_string();
        let mut s = parse(&text);
        let Field::Value(read) = s.photo_faces("a.jpg") else { panic!("faces") };
        // Name the second face, remove the third, keep the first: each keeps its own keys.
        let mut faces = read.faces.clone();
        faces[1].person = FacePerson::Named(pid("p0b1d"));
        faces.truncate(2);
        s.set_photo_faces("a.jpg", &faces).unwrap();
        assert_eq!(
            json_of(&s)["photos"]["a.jpg"]["faces"],
            json!([
                { "rect": [0.1, 0.1, 0.2, 0.2], "person": "p7f3a2c", "x-source": "xmp" },
                { "rect": [0.5, 0.5, 0.6, 0.6], "person": "p0b1d", "future": { "landmarks": [1, 2] } }
            ])
        );
        // Every rectangle moved (a Save turning the photo): the keys follow the faces in order;
        // naming a face unnamed again removes its `person` only.
        let moved = [
            Face { rect: rect(0.8, 0.1, 0.9, 0.2), person: FacePerson::Unnamed },
            Face { rect: rect(0.4, 0.5, 0.5, 0.6), person: FacePerson::Named(pid("p0b1d")) },
        ];
        s.set_photo_faces("a.jpg", &moved).unwrap();
        assert_eq!(
            json_of(&s)["photos"]["a.jpg"]["faces"],
            json!([
                { "rect": [0.8, 0.1, 0.9, 0.2], "x-source": "xmp" },
                { "rect": [0.4, 0.5, 0.5, 0.6], "person": "p0b1d", "future": { "landmarks": [1, 2] } }
            ])
        );
        // Reordered faces with the same rectangles keep their own keys.
        s.set_photo_faces("a.jpg", &[moved[1].clone(), moved[0].clone()]).unwrap();
        let list = &json_of(&s)["photos"]["a.jpg"]["faces"];
        assert_eq!((list[0]["future"].clone(), list[1]["x-source"].clone()), (json!({ "landmarks": [1, 2] }), json!("xmp")));
    }

    #[test]
    fn faces_past_the_cap_are_kept_and_left_out() {
        let face = |i: usize| json!({ "rect": [0.0, 0.0, (i + 1) as f64 / 1000.0, 0.5] });
        let list: Vec<Value> = (0..MAX_FACES + 3).map(face).collect();
        let mut s = parse(&json!({ "photos": { "a.jpg": { "faces": list } } }).to_string());
        let Field::Value(read) = s.photo_faces("a.jpg") else { panic!("faces") };
        assert_eq!((read.faces.len(), read.over, read.bad.len()), (MAX_FACES, 3, 0));
        // Removing the first face keeps the three left out after Dalica's faces.
        s.set_photo_faces("a.jpg", &read.faces[1..]).unwrap();
        let written = json_of(&s)["photos"]["a.jpg"]["faces"].as_array().unwrap().clone();
        assert_eq!(written.len(), MAX_FACES + 2);
        assert_eq!(written[MAX_FACES - 1..], [face(MAX_FACES), face(MAX_FACES + 1), face(MAX_FACES + 2)]);
    }

    fn unit() -> impl Strategy<Value = f64> {
        prop_oneof![Just(0.0), Just(1.0), 0.0..=1.0f64]
    }

    fn face() -> impl Strategy<Value = Face> {
        let person = prop_oneof![Just(FacePerson::Unnamed), Just(FacePerson::Ignored), "p[0-9a-f]{4,16}".prop_map(|p| FacePerson::Named(pid(&p)))];
        (unit(), unit(), unit(), unit(), person).prop_map(|(a, b, c, d, person)| Face { rect: rect(a.min(c), b.min(d), a.max(c), b.max(d)), person })
    }

    proptest! {
        #![proptest_config(cases(256, 32))]

        /// Any faces and any place print and read back exactly, and a second write changes no byte.
        #[test]
        fn any_faces_and_place_round_trip(
            faces in proptest::collection::vec(face(), 0..8),
            lat in -90.0..=90.0f64,
            lon in -180.0..=180.0f64,
        ) {
            let mut s = Sidecar::new();
            s.set_photo_faces("a.jpg", &faces).unwrap();
            s.set_geo("a.jpg", Geo::new(lat, lon)).unwrap();
            let again = reparse(&s);
            let want = if faces.is_empty() { Field::Absent } else { Field::Value(PhotoFaces { faces: faces.clone(), bad: vec![], over: 0 }) };
            prop_assert_eq!(again.photo_faces("a.jpg"), want);
            prop_assert_eq!(again.geo("a.jpg"), Field::Value(Geo::new(lat, lon).unwrap()));
            let mut twice = again.clone();
            twice.set_photo_faces("a.jpg", &faces).unwrap();
            prop_assert_eq!(twice.print(), s.print());
        }
    }
}
