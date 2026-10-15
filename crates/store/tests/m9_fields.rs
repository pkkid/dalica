//! M9's fields on disk and in the index (`data-model.md` §2–§4, §10.1): a folder's photo sort,
//! albums with their memberships, keys and covers, and the index-only search facts, content
//! digests and first-indexed and last-changed times. Each surviving the journal and a snapshot, the
//! user's coming back from the folders and `albums.json` after the index is deleted and the
//! index-only ones never reaching a sidecar is `fields.rs`. Here: every field has a home; hostile
//! ids, keys and wrong types never panic and are kept where they cannot be used.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::common::NOW;
use dalica_core::{AlbumId, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::{AlbumInfo, ContentDigest, Facts, Folder, Library, Op, Photo, PhotoSort, SortBy};
use dalica_sidecar::albums::ENTRY_KEYS;
use dalica_sidecar::fields::{M9_FOLDER_KEYS, M9_PHOTO_KEYS, M9_TOP_KEYS};
use dalica_sidecar::{Limits, Sidecar};
use dalica_store::SidecarWriter;
use dalica_store::mapping::{
    self, ALBUM_INFO_FIELDS, CONTENT_DIGEST_FIELDS, FACTS_FIELDS, FOLDER_DATA_M9, FolderState, LIBRARY_DATA_M9, PHOTO_DATA_M9, PHOTO_SORT_FIELDS,
    PhotoState, Reading, Source,
};
use proptest::prelude::*;
use serde_json::{Value, json};

fn names() -> Vec<String> {
    vec!["IMG_0001.jpg".into(), "a]b ;=.jpg".into(), "東京 タワー.CR2".into(), format!("{}.jpg", "長".repeat(60)), "IMG_0005.png".into()]
}

fn scan(roots: &[&Path]) -> (Library, Vec<FolderId>, Vec<Vec<PhotoId>>) {
    let mut lib = Library::new();
    let (mut folders, mut ids) = (Vec::new(), Vec::new());
    for root in roots {
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.to_path_buf() } }).unwrap();
        let mine: Vec<PhotoId> = names().iter().map(|_| lib.next_photo_id()).collect();
        let photos = mine
            .iter()
            .zip(names())
            .map(|(&id, name)| Photo {
                id,
                folder: f,
                name,
                size: 10,
                mtime: Timestamp::from_millis(5),
                width: 4,
                height: 3,
                date: None,
                star: false,
            })
            .collect();
        lib.apply(Op::AddPhotos { photos }).unwrap();
        folders.push(f);
        ids.push(mine);
    }
    (lib, folders, ids)
}

fn id(s: &str) -> AlbumId {
    AlbumId::parse(s).unwrap()
}

fn sidecar(dir: &Path) -> Sidecar {
    Sidecar::parse(&std::fs::read(dir.join(".dalica.json")).unwrap(), &Limits::default()).unwrap()
}

/// Every M9 model field has a home; adding a field to one of these structs breaks a
/// destructuring here until it is placed in its table, and the sidecar crate reads exactly the
/// keys the tables name.
#[test]
fn every_m9_field_is_mapped() {
    let AlbumInfo { name: _, description: _, date: _, use_newest: _, place: _ } = AlbumInfo::default();
    let names: Vec<&str> = ALBUM_INFO_FIELDS.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["name", "description", "date", "useNewest", "place"]);
    assert!(ALBUM_INFO_FIELDS.iter().all(|(n, s)| *s == Source::Albums(n) && ENTRY_KEYS.contains(n)));
    let Facts { make: _, model: _, video: _, geotagged: _ } = Facts::default();
    assert_eq!(FACTS_FIELDS.map(|(n, _)| n), ["make", "model", "video", "geotagged"]);
    let ContentDigest { size: _, mtime: _, digest: _ } = ContentDigest::default();
    assert_eq!(CONTENT_DIGEST_FIELDS.map(|(n, _)| n), ["size", "mtime", "digest"]);
    assert!(FACTS_FIELDS.iter().chain(&CONTENT_DIGEST_FIELDS).all(|(_, s)| *s == Source::Index), "index-only");
    let PhotoSort { by: _, reverse: _ } = PhotoSort::default();
    assert_eq!(PHOTO_SORT_FIELDS, [("by", Source::FolderSidecar("sort")), ("reverse", Source::FolderSidecar("reverse"))]);
    for by in [SortBy::Date, SortBy::Name, SortBy::Size] {
        match by {
            SortBy::Date => assert_eq!(mapping::sort_text(by), None),
            SortBy::Name => assert_eq!(mapping::sort_text(by), Some("name")),
            SortBy::Size => assert_eq!(mapping::sort_text(by), Some("size")),
        }
    }
    assert_eq!(
        PHOTO_DATA_M9,
        [
            ("albums", Source::Sidecar("albums")),
            ("facts", Source::Fields("FACTS_FIELDS")),
            ("digest", Source::Fields("CONTENT_DIGEST_FIELDS")),
            ("firstIndexed", Source::Index),
            ("lastChanged", Source::Index),
        ]
    );
    assert_eq!(FOLDER_DATA_M9, [("sort", Source::Fields("PHOTO_SORT_FIELDS")), ("albumNames", Source::TopSidecar("albums"))]);
    assert_eq!(
        LIBRARY_DATA_M9,
        [("albums", Source::Fields("ALBUM_INFO_FIELDS")), ("members", Source::Sidecar("albums")), ("covers", Source::Albums("cover"))]
    );
    // The sidecar keys the tables name are the ones the sidecar crate reads.
    let photo_keys: Vec<&str> = PHOTO_DATA_M9.iter().filter_map(|(_, s)| if let Source::Sidecar(k) = s { Some(*k) } else { None }).collect();
    assert_eq!(photo_keys, M9_PHOTO_KEYS);
    let folder_keys: Vec<&str> =
        PHOTO_SORT_FIELDS.iter().filter_map(|(_, s)| if let Source::FolderSidecar(k) = s { Some(*k) } else { None }).collect();
    assert_eq!(folder_keys, M9_FOLDER_KEYS);
    let top: Vec<&str> = FOLDER_DATA_M9.iter().filter_map(|(_, s)| if let Source::TopSidecar(k) = s { Some(*k) } else { None }).collect();
    assert_eq!(top, M9_TOP_KEYS);
    assert!(ENTRY_KEYS.contains(&"cover") && ENTRY_KEYS.contains(&"order"));
    // The sidecar state names each M9 field.
    let PhotoState { star: _, info: _, saved_as: _, albums: _ } = PhotoState::default();
    let FolderState { id: _, path: _, folder: _, photos: _, project: _, sort: _, album_names: _, known_albums: _ } = FolderState::default();
}

#[test]
fn a_sort_dalica_cannot_hold_is_kept_reported_and_written_over_only_when_the_user_sorts() {
    let dir = tempfile::tempdir().unwrap();
    let planted = json!({"folder": {"sort": "manual", "reverse": true, "order": ["b.jpg", "a.jpg"]}, "photos": {"IMG_0001.jpg": {"star": true}}});
    std::fs::write(dir.path().join(".dalica.json"), planted.to_string()).unwrap();
    let (mut lib, folders, ids) = scan(&[dir.path()]);
    let r = mapping::read_folder(&sidecar(dir.path()));
    assert_eq!((r.sort.clone(), r.reverse.clone()), (Reading::Unusable, Reading::Usable(true)));
    assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
    assert_eq!(r.photo_sort(PhotoSort::new(SortBy::Size, false)), PhotoSort::new(SortBy::Size, true), "the model keeps its own by");
    let import = dalica_store::import_sidecars(&lib, NOW);
    assert_eq!(import.problems.len(), 1);
    for op in import.ops {
        lib.apply(op).unwrap();
    }
    assert_eq!(lib.folder_sort(folders[0]), PhotoSort::new(SortBy::Date, true));
    // Another change keeps "manual" as written.
    lib.apply(Op::SetStars { stars: vec![(ids[0][1], true)] }).unwrap();
    let mut w = SidecarWriter::new();
    w.mark_dirty(folders[0]);
    w.flush(&lib, NOW);
    let v: Value = serde_json::from_slice(&std::fs::read(dir.path().join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(v["folder"], planted["folder"]);
    // Sorting the folder writes over it; the order Dalica does not use yet stays.
    lib.apply(Op::SetFolderSorts { sorts: vec![(folders[0], PhotoSort::new(SortBy::Name, true))] }).unwrap();
    w.mark_dirty(folders[0]);
    w.flush(&lib, NOW);
    let v: Value = serde_json::from_slice(&std::fs::read(dir.path().join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(v["folder"], json!({"sort": "name", "reverse": true, "order": ["b.jpg", "a.jpg"]}));
}

fn hostile_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        ".{0,10}".prop_map(Value::String),
        Just(json!("a0")),
        Just(json!("name")),
        Just(json!("size")),
        Just(json!("modified")),
    ];
    leaf.prop_recursive(2, 12, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(Value::Array),
            prop::collection::btree_map("(a[0-9a-f]{4,6}|name|[A-Z]{1,3})", inner, 0..3).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(crate::common::cases(256, 32))]

    /// Hostile ids, keys, names and wrong types never panic. What the reader cannot use stays in
    /// the file through a write, and the model's memberships, names and sort read back as written.
    #[test]
    fn hostile_m9_values_never_panic_and_keep_what_cannot_be_used(
        photo_albums in prop_oneof![hostile_value(), prop::collection::btree_map("(a[0-9a-f]{4,6}|[A-Z!]{1,3})", hostile_value(), 0..5).prop_map(|m| Value::Object(m.into_iter().collect()))],
        top_albums in hostile_value(),
        sort in hostile_value(),
        reverse in hostile_value(),
        members in prop::collection::btree_map("a[0-9a-f]{4,6}", "a[0-9a-z]", 0..3),
        known_extra in prop::collection::btree_set("a[0-9a-f]{4,6}", 0..3),
        model_sort in (0..3u8, any::<bool>()),
    ) {
        let doc = json!({
            "photos": {"p.jpg": {"albums": photo_albums.clone(), "x-other": 1}},
            "albums": top_albums.clone(),
            "folder": {"sort": sort.clone(), "reverse": reverse.clone()},
        });
        let mut s = Sidecar::parse(doc.to_string().as_bytes(), &Limits::default()).unwrap();
        let _ = mapping::read_photo(&s, "p.jpg");
        let folder = mapping::read_folder(&s);
        let (names, _) = mapping::read_album_names(&s);
        let _ = (folder.photo_sort(PhotoSort::default()), names);

        let members: BTreeMap<AlbumId, PosKey> = members.iter().map(|(a, k)| (id(a), PosKey::parse(k).unwrap())).collect();
        let mut known: BTreeSet<AlbumId> = known_extra.iter().map(|a| id(a)).collect();
        known.extend(members.keys().cloned());
        let by = [SortBy::Date, SortBy::Name, SortBy::Size][usize::from(model_sort.0)];
        let state = FolderState {
            photos: vec![("p.jpg".into(), PhotoState { albums: members.clone(), ..PhotoState::default() })],
            sort: PhotoSort::new(by, model_sort.1),
            album_names: members.keys().map(|a| (a.clone(), format!("Album {a}"))).collect(),
            known_albums: known.clone(),
            ..FolderState::default()
        };
        let refused = mapping::apply(&state, &mut s);
        prop_assert!(refused.is_empty(), "{refused:?}");
        let out: Value = serde_json::from_slice(&s.print()).unwrap();
        // Entries the reader could not use, and memberships of albums the model does not have,
        // stay as written; the others are the model's.
        if let Value::Object(m) = &photo_albums {
            for (k, v) in m {
                let album = AlbumId::parse(k).ok();
                let usable = album.is_some() && v.as_str().is_some_and(|t| PosKey::parse(t).is_ok());
                let ours = album.as_ref().is_some_and(|a| known.contains(a));
                let replaced = album.as_ref().is_some_and(|a| members.contains_key(a));
                if !replaced && !(usable && ours) {
                    prop_assert_eq!(&out["photos"]["p.jpg"]["albums"][k], v);
                }
            }
        }
        prop_assert_eq!(&out["photos"]["p.jpg"]["x-other"], &json!(1));
        // The model's values read back; a value of the wrong type stays only while the model has
        // nothing to say there.
        match mapping::read_photo(&s, "p.jpg").albums {
            Reading::Usable(back) => {
                let back: BTreeMap<AlbumId, PosKey> = back.into_iter().filter(|(a, _)| known.contains(a)).collect();
                prop_assert_eq!(back, members.clone());
            }
            Reading::Unusable => prop_assert!(members.is_empty() && !photo_albums.is_object()),
        }
        let f = mapping::read_folder(&s);
        match f.sort {
            Reading::Usable(b) => prop_assert_eq!(b, by),
            Reading::Unusable => prop_assert_eq!(by, SortBy::Date),
        }
        match f.reverse {
            Reading::Usable(r) => prop_assert_eq!(r, model_sort.1),
            Reading::Unusable => prop_assert!(!model_sort.1),
        }
        match mapping::read_album_names(&s).0 {
            Reading::Usable(names) => prop_assert_eq!(names, state.album_names.clone()),
            Reading::Unusable => prop_assert!(state.album_names.is_empty()),
        }
    }
}
