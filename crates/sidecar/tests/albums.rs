//! M9's keys on disk (`data-model.md` §3, §3.2, §4): a photo's `albums` (album id → position key),
//! the folder's `sort` and `reverse` and the top-level album names in `.dalica.json`, and the
//! library's `albums.json`. Ids, keys and names that cannot be used are kept verbatim and never
//! used; keys Dalica does not know are kept; the printed key order is the documented one.

use std::collections::BTreeMap;

use dalica_core::{AlbumId, PosKey};
use dalica_sidecar::albums::{Album, AlbumsFile, ENTRY_KEYS};
use dalica_sidecar::fields::{ALBUMS, M9_FOLDER_KEYS, M9_PHOTO_KEYS, M9_TOP_KEYS, REVERSE, SORT};
use dalica_sidecar::{Field, Limits, ParseError, Problem, Sidecar, WrongType};
use proptest::prelude::*;
use serde_json::{Value, json};

fn parse(text: &str) -> Sidecar {
    Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap()
}

fn json_of(s: &Sidecar) -> Value {
    serde_json::from_slice(&s.print()).unwrap()
}

fn id(s: &str) -> AlbumId {
    AlbumId::parse(s).unwrap()
}

fn key(s: &str) -> PosKey {
    PosKey::parse(s).unwrap()
}

fn members(list: &[(&str, &str)]) -> BTreeMap<AlbumId, PosKey> {
    list.iter().map(|(a, k)| (id(a), key(k))).collect()
}

#[test]
fn the_m9_keys_are_the_documented_ones() {
    assert_eq!(M9_PHOTO_KEYS, ["albums"]);
    assert_eq!(M9_FOLDER_KEYS, ["sort", "reverse"]);
    assert_eq!(M9_TOP_KEYS, ["albums"]);
    assert_eq!((ALBUMS, SORT, REVERSE), ("albums", "sort", "reverse"));
    assert_eq!(ENTRY_KEYS, ["name", "description", "date", "useNewest", "place", "cover", "order"]);
}

#[test]
fn memberships_read_their_usable_entries_and_keep_the_rest() {
    let s = parse(r#"{"photos": {"a.jpg": {"star": true, "albums": {"a91c0e": "a0", "a77d02": "a1", "B4D": "a0", "a1234": "!", "a5678": 7}}}}"#);
    let Field::Value(read) = s.photo_albums("a.jpg") else { panic!("{:?}", s.photo_albums("a.jpg")) };
    assert_eq!(read.members, members(&[("a91c0e", "a0"), ("a77d02", "a1")]));
    assert_eq!(read.bad, ["B4D", "a1234", "a5678"]);
    assert!(s.problems().is_empty(), "entries left out are the reader's to report: {:?}", s.problems());
    assert_eq!(s.photo_albums("b.jpg"), Field::Absent);

    // Writing: usable entries become the model's, the bad ones stay verbatim.
    let mut w = s.clone();
    w.set_photo_albums("a.jpg", &members(&[("a77d02", "a2"), ("a00000", "Zz")]), |_| true).unwrap();
    assert_eq!(json_of(&w)["photos"]["a.jpg"]["albums"], json!({"a77d02": "a2", "a00000": "Zz", "B4D": "a0", "a1234": "!", "a5678": 7}));
    // A membership of the id of a bad entry replaces it.
    w.set_photo_albums("a.jpg", &members(&[("a1234", "a0")]), |_| true).unwrap();
    assert_eq!(json_of(&w)["photos"]["a.jpg"]["albums"], json!({"a1234": "a0", "B4D": "a0", "a5678": 7}));
    // Usable entries of albums the writer does not manage stay as they are.
    let mut m = w.clone();
    m.set_photo_albums("a.jpg", &members(&[("a77d02", "a5")]), |a| *a == id("a77d02")).unwrap();
    assert_eq!(json_of(&m)["photos"]["a.jpg"]["albums"], json!({"a1234": "a0", "B4D": "a0", "a5678": 7, "a77d02": "a5"}));
    let mut m = w.clone();
    m.set_photo_albums("a.jpg", &BTreeMap::new(), |a| *a != id("a1234")).unwrap();
    assert_eq!(json_of(&m)["photos"]["a.jpg"]["albums"], json!({"a1234": "a0", "B4D": "a0", "a5678": 7}));
    // Clearing keeps the bad entries, and the star.
    w.set_photo_albums("a.jpg", &BTreeMap::new(), |_| true).unwrap();
    assert_eq!(json_of(&w)["photos"]["a.jpg"], json!({"star": true, "albums": {"B4D": "a0", "a5678": 7}}));
}

#[test]
fn clearing_the_last_membership_removes_the_key_and_the_empty_entry_and_creates_nothing() {
    let mut s = Sidecar::new();
    let before = s.print();
    s.set_photo_albums("x.jpg", &BTreeMap::new(), |_| true).unwrap();
    assert_eq!(s.print(), before, "clearing creates nothing");
    s.set_photo_albums("x.jpg", &members(&[("a91c0e", "a0")]), |_| true).unwrap();
    assert_eq!(json_of(&s)["photos"]["x.jpg"], json!({"albums": {"a91c0e": "a0"}}));
    s.set_photo_albums("x.jpg", &BTreeMap::new(), |_| true).unwrap();
    assert_eq!(json_of(&s)["photos"], json!({}), "the entry it emptied goes");
    // An entry that was there with other keys and no albums is left as it is.
    let mut t = parse(r#"{"photos": {"y.jpg": {}}}"#);
    let before = t.print();
    t.set_photo_albums("y.jpg", &BTreeMap::new(), |_| true).unwrap();
    assert_eq!(t.print(), before);
}

#[test]
fn albums_of_the_wrong_type_are_kept_reported_and_written_over_only_by_memberships() {
    let mut s = parse(r#"{"photos": {"a.jpg": {"albums": ["a91c0e"]}}, "albums": "Trams"}"#);
    assert_eq!(s.photo_albums("a.jpg"), Field::WrongType(json!(["a91c0e"])));
    assert_eq!(s.album_names(), Field::WrongType(json!("Trams")));
    assert_eq!(
        s.problems(),
        [
            Problem { pointer: "/photos/a.jpg/albums".into(), expected: "an object from album id to position key" },
            Problem { pointer: "/albums".into(), expected: "an object from album id to {\"name\": …}" },
        ]
    );
    let before = s.print();
    s.set_photo_albums("a.jpg", &BTreeMap::new(), |_| true).unwrap();
    s.set_album_names(&BTreeMap::new());
    assert_eq!(s.print(), before, "clearing never writes over a value of the wrong type");
    s.set_photo_albums("a.jpg", &members(&[("a91c0e", "a0")]), |_| true).unwrap();
    s.set_album_names(&BTreeMap::from([(id("a91c0e"), "Trams".to_string())]));
    assert_eq!(json_of(&s)["photos"]["a.jpg"]["albums"], json!({"a91c0e": "a0"}));
    assert_eq!(json_of(&s)["albums"], json!({"a91c0e": {"name": "Trams"}}));
    // `photos` or the entry of the wrong type refuses, keeping it.
    let mut t = parse(r#"{"photos": {"a.jpg": 5}}"#);
    assert_eq!(t.set_photo_albums("a.jpg", &members(&[("a91c0e", "a0")]), |_| true), Err(WrongType { pointer: "/photos/a.jpg".into() }));
    let mut u = parse(r#"{"photos": []}"#);
    assert_eq!(u.set_photo_albums("a.jpg", &BTreeMap::new(), |_| true), Err(WrongType { pointer: "/photos".into() }));
    assert_eq!(json_of(&u)["photos"], json!([]));
}

#[test]
fn album_names_keep_their_objects_unknown_keys_and_the_entries_they_cannot_use() {
    let mut s = parse(
        r#"{"albums": {"a91c0e": {"name": "Best of 2026", "colour": "red"}, "a77d02": {"name": "Old"}, "nope": {"name": "x"}, "a1234": {"name": ""}, "a5678": "Trams"}}"#,
    );
    let Field::Value(read) = s.album_names() else { panic!() };
    assert_eq!(read.names, BTreeMap::from([(id("a91c0e"), "Best of 2026".to_string()), (id("a77d02"), "Old".to_string())]));
    assert_eq!(read.bad, ["a1234", "a5678", "nope"]);
    s.set_album_names(&BTreeMap::from([(id("a91c0e"), "Best".to_string()), (id("a00001"), "New".to_string())]));
    assert_eq!(
        json_of(&s)["albums"],
        json!({"a91c0e": {"name": "Best", "colour": "red"}, "a00001": {"name": "New"}, "nope": {"name": "x"}, "a1234": {"name": ""}, "a5678": "Trams"})
    );
    let mut t = parse(r#"{"albums": {"a91c0e": {"name": "Trams"}}}"#);
    t.set_album_names(&BTreeMap::new());
    assert_eq!(json_of(&t), json!({}), "the emptied key goes");
}

#[test]
fn the_folder_sort_and_reverse_live_in_folder_and_clear_like_its_other_keys() {
    let mut s = Sidecar::new();
    assert_eq!((s.folder_sort(), s.folder_reverse()), (Field::Absent, Field::Absent));
    s.set_folder_sort(Some("name")).unwrap();
    s.set_folder_reverse(true).unwrap();
    assert_eq!(json_of(&s)["folder"], json!({"sort": "name", "reverse": true}));
    assert_eq!((s.folder_sort(), s.folder_reverse()), (Field::Value("name".into()), Field::Value(true)));
    s.set_folder_sort(None).unwrap();
    s.set_folder_reverse(false).unwrap();
    assert_eq!(json_of(&s), json!({"dalica": {"version": 1}}), "an emptied \"folder\" goes");
    let t = parse(r#"{"folder": {"sort": 3, "reverse": "yes", "order": ["b.jpg"]}}"#);
    assert_eq!(t.folder_sort(), Field::WrongType(json!(3)));
    assert_eq!(t.folder_reverse(), Field::WrongType(json!("yes")));
    assert_eq!(
        t.problems(),
        [Problem { pointer: "/folder/sort".into(), expected: "a string" }, Problem { pointer: "/folder/reverse".into(), expected: "true or false" },]
    );
    let mut u = parse(r#"{"folder": "Lisbon"}"#);
    assert_eq!(u.set_folder_sort(Some("size")), Err(WrongType { pointer: "/folder".into() }));
}

#[test]
fn a_sidecar_with_every_m9_key_prints_in_the_documented_key_order() {
    let mut s = Sidecar::new();
    s.set_folder_sort(Some("size")).unwrap();
    s.set_folder_reverse(true).unwrap();
    s.set_photo_albums("b.jpg", &members(&[("a91c0e", "a1"), ("a77d02", "a0")]), |_| true).unwrap();
    s.set_album_names(&BTreeMap::from([(id("a91c0e"), "Best of 2026".to_string()), (id("a77d02"), "Trams".to_string())]));
    let text = String::from_utf8(s.print()).unwrap();
    assert_eq!(
        text,
        "{\n  \"albums\": {\n    \"a77d02\": {\n      \"name\": \"Trams\"\n    },\n    \"a91c0e\": {\n      \"name\": \"Best of 2026\"\n    }\n  },\n  \"dalica\": {\n    \"version\": 1\n  },\n  \"folder\": {\n    \"reverse\": true,\n    \"sort\": \"size\"\n  },\n  \"photos\": {\n    \"b.jpg\": {\n      \"albums\": {\n        \"a77d02\": \"a0\",\n        \"a91c0e\": \"a1\"\n      }\n    }\n  }\n}\n"
    );
    assert_eq!(parse(&text), s);
}

fn album(name: &str) -> Album {
    Album { name: name.into(), ..Album::default() }
}

#[test]
fn albums_json_round_trips_with_unknown_keys_and_unusable_entries_kept() {
    let text = r#"{
      "dalica": {"version": 1, "writer": "a future Dalica"},
      "albums": {
        "a91c0e": {"name": "Best of 2026", "description": "d", "date": "2026-06-04", "useNewest": true, "place": "Lisbon",
                   "cover": "/p/IMG_0002.jpg", "order": [["/p/IMG_0002.jpg", "a0"], ["/q/x.jpg", "a1"], ["/q/bad.jpg", "!"], 5],
                   "x-colour": "red"},
        "a77d02": {"name": "Trams", "date": "June", "useNewest": "yes", "place": 7},
        "B4D": {"name": "upper case id"},
        "a1234": {"description": "no name"},
        "a5678": "Trams"
      },
      "planted": [1, 2]
    }"#;
    let f = AlbumsFile::parse(text.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(f.version(), Some(1));
    let read = f.albums();
    assert_eq!(read.bad, ["B4D", "a1234", "a5678"]);
    let best = &read.albums[&id("a91c0e")];
    assert_eq!(
        *best,
        Album {
            name: "Best of 2026".into(),
            description: "d".into(),
            date: Some("2026-06-04".parse().unwrap()),
            use_newest: true,
            place: "Lisbon".into(),
            cover: Some("/p/IMG_0002.jpg".into()),
            order: vec![("/p/IMG_0002.jpg".into(), key("a0")), ("/q/x.jpg".into(), key("a1"))],
        }
    );
    assert_eq!(read.albums[&id("a77d02")], album("Trams"), "values of the wrong type read as empty");
    let pointers: Vec<String> = f.problems().into_iter().map(|p| p.pointer).collect();
    assert_eq!(
        pointers,
        [
            "/albums/B4D",
            "/albums/a1234/name",
            "/albums/a5678",
            "/albums/a77d02/place",
            "/albums/a77d02/date",
            "/albums/a77d02/useNewest",
            "/albums/a91c0e/order/2",
            "/albums/a91c0e/order/3",
        ]
    );
    // Printing then parsing gives the same document; setting an album keeps its unknown keys.
    let again = AlbumsFile::parse(&f.print(), &Limits::default()).unwrap();
    assert_eq!(again, f);
    let mut g = f.clone();
    g.set(&id("a91c0e"), &album("Best"));
    g.remove("a77d02");
    let v: Value = serde_json::from_slice(&g.print()).unwrap();
    assert_eq!(v["albums"]["a91c0e"], json!({"name": "Best", "x-colour": "red"}));
    assert!(v["albums"].get("a77d02").is_none());
    assert_eq!(v["albums"]["B4D"], json!({"name": "upper case id"}));
    assert_eq!(v["albums"]["a5678"], json!("Trams"));
    assert_eq!(v["planted"], json!([1, 2]));
    assert_eq!(v["dalica"]["writer"], json!("a future Dalica"));
    // An entry of the wrong type under the id written is replaced.
    g.set(&id("a5678"), &album("Trams"));
    assert_eq!(g.albums().albums[&id("a5678")], album("Trams"));
}

#[test]
fn a_damaged_albums_json_is_a_parse_error() {
    let l = Limits::default();
    assert!(matches!(AlbumsFile::parse(b"{\"albums\": ", &l), Err(ParseError::Json { .. })));
    assert_eq!(AlbumsFile::parse(b"[1]", &l), Err(ParseError::NotAnObject));
    let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
    assert!(matches!(AlbumsFile::parse(format!("{{\"albums\": {deep}}}").as_bytes(), &l), Err(ParseError::Limit(_))));
    let small = Limits { max_photos: 2, ..Limits::default() };
    let three = r#"{"albums": {"a0001": {"name": "a"}, "a0002": {"name": "b"}, "a0003": {"name": "c"}}}"#;
    assert!(matches!(AlbumsFile::parse(three.as_bytes(), &small), Err(ParseError::Limit(_))));
    // A BOM and CRLF are read.
    let f = AlbumsFile::parse(b"\xef\xbb\xbf{\r\n\"albums\": {\"a0001\": {\"name\": \"a\"}}\r\n}", &l).unwrap();
    assert_eq!(f.albums().albums.len(), 1);
}

fn hostile_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        "[a-zA-Z0-9!~/ ]{0,12}".prop_map(Value::String),
        Just(json!("a0")),
        Just(json!("A00000000000000000000000000")),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map("(a[0-9a-f]{0,18}|[A-Z!]{1,4}|name)", inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

fn hostile_id() -> impl Strategy<Value = String> {
    prop_oneof!["a[0-9a-f]{4,16}", "a[0-9a-fA-Z]{0,20}", ".{0,8}"]
}

proptest! {
    #![proptest_config(crate::common::cases(256, 32))]

    /// Hostile memberships, names and sorts never panic; what cannot be used is kept verbatim
    /// through any write of the usable ones.
    #[test]
    fn hostile_m9_keys_never_panic_and_keep_what_cannot_be_used(
        entries in prop::collection::btree_map(hostile_id(), hostile_value(), 0..6),
        names in prop::collection::btree_map(hostile_id(), hostile_value(), 0..6),
        sort in hostile_value(),
        reverse in hostile_value(),
        write in prop::collection::btree_map("a[0-9a-f]{4,8}", "a[0-9a-z]", 0..4),
    ) {
        let doc = json!({
            "photos": {"p.jpg": {"albums": Value::Object(entries.clone().into_iter().collect())}},
            "albums": Value::Object(names.clone().into_iter().collect()),
            "folder": {"sort": sort, "reverse": reverse},
        });
        let mut s = Sidecar::parse(doc.to_string().as_bytes(), &Limits::default()).unwrap();
        let _ = (s.problems(), s.folder_sort(), s.folder_reverse(), s.album_names());
        let Field::Value(read) = s.photo_albums("p.jpg") else { panic!("an object") };
        let Field::Value(read_names) = s.album_names() else { panic!("an object") };
        let members: BTreeMap<AlbumId, PosKey> = write.iter().map(|(a, k)| (id(a), key(k))).collect();
        s.set_photo_albums("p.jpg", &members, |_| true).unwrap();
        let named: BTreeMap<AlbumId, String> = members.keys().map(|a| (a.clone(), "N".to_string())).collect();
        s.set_album_names(&named);
        let out = json_of(&s);
        for bad in &read.bad {
            if AlbumId::parse(bad).is_ok_and(|a| members.contains_key(&a)) {
                continue;
            }
            prop_assert_eq!(&out["photos"]["p.jpg"]["albums"][bad], &entries[bad]);
        }
        for bad in &read_names.bad {
            if AlbumId::parse(bad).is_ok_and(|a| named.contains_key(&a)) {
                continue;
            }
            prop_assert_eq!(&out["albums"][bad], &names[bad]);
        }
        let again = match s.photo_albums("p.jpg") {
            Field::Value(v) => v.members,
            Field::Absent => BTreeMap::new(),
            Field::WrongType(v) => panic!("{v}"),
        };
        prop_assert_eq!(again, members);
        // The albums.json reader takes the same hostile shapes.
        let file = json!({"albums": Value::Object(names.into_iter().collect())});
        let f = AlbumsFile::parse(file.to_string().as_bytes(), &Limits::default()).unwrap();
        let _ = (f.albums(), f.problems());
        prop_assert_eq!(AlbumsFile::parse(&f.print(), &Limits::default()).unwrap(), f);
    }
}
