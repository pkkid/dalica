//! M6's settings and the metadata hook (`roadmap.md` M6.5): every new setting round-trips through
//! `settings.json` with unknown keys kept; a value of the wrong type is its default and is reported
//! when the library opens; the metadata hook sees the photos and fields of a star op, its undo and
//! its redo.

use std::path::{Path, PathBuf};

use crate::common::open;
use dalica_core::PhotoId;
use dalica_engine::settings::{FILE, MAX_RECENT_TAGS, MAX_TAG_BYTES, MetadataWrites, QUICK_TAGS, QuickTags, Settings};
use dalica_engine::{Field, Fields, MetadataChange, Origin, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

fn write_settings(library: &Path, v: &Value) {
    dalica_fs::create_dir_all(library).unwrap();
    dalica_fs::replace(&library.join(FILE), serde_json::to_vec_pretty(v).unwrap().as_slice(), Expect::Absent).unwrap();
}

fn stored(library: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(library.join(FILE)).unwrap()).unwrap()
}

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| t.to_string()).collect()
}

#[test]
fn nothing_stored_means_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let w = s.settings().metadata_writes();
    assert_eq!(
        (w.captions_tags_places, w.stars_as_ratings, w.update_modification_time, w.raw_xmp_sidecars, w.dates),
        (true, false, false, false, true),
        "menus-and-shortcuts.md §5 General → Files and xmp-interop.md §2"
    );
    assert_eq!(w, MetadataWrites::default());
    let q = s.settings().quick_tags();
    assert_eq!(q.slots.len(), 10);
    assert!(q.slots.iter().all(Option::is_none) && q.use_recent);
    assert!(s.settings().recent_tags().is_empty());
    assert!(s.notices().is_empty(), "{:?}", s.notices());
}

#[test]
fn every_new_setting_round_trips_through_settings_json_with_unknown_keys_kept() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path();
    write_settings(
        lib,
        &json!({
            "future": {"a": [1, 2]},
            "files": {"detectDuplicates": false, "futureFlag": "x"},
            "tags": {"futureList": [3]},
            "view": {"thumbSize": 150},
        }),
    );
    let mut s = open(lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    // Every option the other way round from its default.
    let writes =
        MetadataWrites { captions_tags_places: false, stars_as_ratings: true, update_modification_time: true, raw_xmp_sidecars: true, dates: false };
    let mut quick = QuickTags { use_recent: false, ..QuickTags::default() };
    quick.slots[0] = Some("beach".into());
    quick.slots[3] = Some("Grand-mère".into());
    quick.slots[9] = Some("  padded  ".into());
    quick.slots[5] = Some("   ".into());
    s.change_settings(|st| {
        st.set_metadata_writes(&writes);
        st.set_quick_tags(&quick);
        st.set_recent_tags(&tags(&["sunset", "family"]));
        st.note_recent_tags(&tags(&["lisbon", "family"]));
    });
    s.flush().unwrap();
    drop(s);

    let s = open(lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    assert_eq!(s.settings().metadata_writes(), writes);
    let back = s.settings().quick_tags();
    assert!(!back.use_recent);
    let expected: Vec<Option<&str>> = (0..QUICK_TAGS)
        .map(|i| match i {
            0 => Some("beach"),
            3 => Some("Grand-mère"),
            9 => Some("padded"),
            _ => None,
        })
        .collect();
    assert_eq!(back.slots.iter().map(Option::as_deref).collect::<Vec<_>>(), expected, "trimmed; blank slots unassigned");
    assert_eq!(s.settings().recent_tags(), tags(&["lisbon", "family", "sunset"]), "most recent first, each once");

    // Merge, never replace: unknown keys at every level are kept, the other settings untouched.
    let file = stored(lib);
    assert_eq!(file["future"], json!({"a": [1, 2]}));
    assert_eq!(file["files"]["detectDuplicates"], json!(false));
    assert_eq!(file["files"]["futureFlag"], json!("x"));
    assert_eq!(file["tags"]["futureList"], json!([3]));
    assert_eq!(s.settings().view_options().thumb_size, Some(150.0));
    assert_eq!(file["files"]["writeStarsAsRatings"], json!(true));
    assert_eq!(file["tags"]["quick"].as_array().map(Vec::len), Some(QUICK_TAGS));
    assert_eq!(file["tags"]["quick"][1], Value::Null);
}

#[test]
fn recent_tags_are_capped_and_overlong_tags_never_stored() {
    let mut st = Settings::default();
    let many: Vec<String> = (0..25).map(|i| format!("t{i}")).collect();
    st.set_recent_tags(&many);
    assert_eq!(st.recent_tags(), many[..MAX_RECENT_TAGS].to_vec());
    st.note_recent_tags(&tags(&["new"]));
    assert_eq!(st.recent_tags().first().map(String::as_str), Some("new"));
    assert_eq!(st.recent_tags().len(), MAX_RECENT_TAGS, "the oldest fell off");
    assert_eq!(st.recent_tags().last().map(String::as_str), Some("t8"));
    let long = "x".repeat(MAX_TAG_BYTES + 1);
    let mut q = QuickTags::default();
    q.slots[0] = Some(long.clone());
    st.set_quick_tags(&q);
    st.note_recent_tags(std::slice::from_ref(&long));
    assert_eq!(st.get("tags").unwrap()["quick"][0], Value::Null);
    assert!(!st.recent_tags().contains(&long));
    assert!(st.problems().is_empty(), "what is written reads back without a problem");
}

#[test]
fn a_wrong_typed_value_is_its_default_and_is_reported_when_the_library_opens() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path();
    let secret = "private-tag-text";
    write_settings(
        lib,
        &json!({
            "files": {
                "writeCaptionsTagsPlaces": "no",
                "writeStarsAsRatings": 1,
                "updateModificationTime": null,
                "writeRawXmpSidecars": [true],
                "writeDates": {"on": false},
            },
            "tags": {
                "quick": ["beach", 7, null, {"t": secret}, "x".repeat(MAX_TAG_BYTES + 1), "", "a", "b", "c", "d", "eleventh"],
                "useRecent": "off",
                "recent": ["sunset", false, secret, "x".repeat(MAX_TAG_BYTES + 1), "sunset"],
            },
        }),
    );
    let s = open(lib);
    // Each wrong-typed value is its default; what is usable is used.
    assert_eq!(s.settings().metadata_writes(), MetadataWrites::default());
    let q = s.settings().quick_tags();
    assert!(q.use_recent);
    assert_eq!(
        q.slots.iter().map(Option::as_deref).collect::<Vec<_>>(),
        [Some("beach"), None, None, None, None, None, Some("a"), Some("b"), Some("c"), Some("d")]
    );
    assert_eq!(s.settings().recent_tags(), tags(&["sunset", secret]));

    // Reported once each, by key, in the session's notices; tag text never appears.
    let notices = s.notices().join("\n");
    for key in [
        "files.writeCaptionsTagsPlaces",
        "files.writeStarsAsRatings",
        "files.updateModificationTime",
        "files.writeRawXmpSidecars",
        "files.writeDates",
        "tags.useRecent",
        "tags.quick: 3 slots",
        "more than 10 slots",
        "tags.recent: 2 entries",
    ] {
        assert_eq!(notices.matches(key).count(), 1, "{key} in:\n{notices}");
    }
    assert!(!notices.contains(secret) && !notices.contains("beach") && !notices.contains("sunset"), "{notices}");
    assert!(notices.lines().all(|l| l.starts_with("settings.json: ")), "{notices}");
    // Nothing was rewritten: the file stays as the user left it until a setting changes.
    assert_eq!(stored(lib)["files"]["writeStarsAsRatings"], json!(1));
}

#[test]
fn objects_of_the_wrong_type_are_reported_once_and_replaced_on_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path();
    write_settings(lib, &json!({ "files": true, "tags": ["beach"] }));
    let mut s = open(lib);
    assert_eq!(s.settings().metadata_writes(), MetadataWrites::default());
    assert_eq!(s.settings().quick_tags(), QuickTags::default());
    assert!(s.settings().recent_tags().is_empty());
    assert_eq!(s.notices().iter().filter(|n| n.contains("\"files\" is not an object")).count(), 1, "{:?}", s.notices());
    assert_eq!(s.notices().iter().filter(|n| n.contains("\"tags\" is not an object")).count(), 1, "{:?}", s.notices());
    s.change_settings(|st| {
        st.set_metadata_writes(&MetadataWrites { dates: false, ..MetadataWrites::default() });
        st.note_recent_tags(&tags(&["beach"]));
    });
    s.flush().unwrap();
    drop(s);
    let s = open(lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    assert!(!s.settings().metadata_writes().dates);
    assert_eq!(s.settings().recent_tags(), tags(&["beach"]));
}

/// A session over one folder of `n` photos, its ids in name order, the folder.
fn library(n: usize) -> (tempfile::TempDir, Session, Vec<PhotoId>, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let mut s = open(&dir.path().join("library"));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    photos.sort();
    (dir, s, photos.into_iter().map(|(_, id)| id).collect(), folder)
}

#[test]
fn the_metadata_hook_sees_the_photos_and_fields_of_a_star_op_its_undo_and_its_redo() {
    let (_dir, mut s, ids, _) = library(4);
    let (_, dropped) = s.take_metadata_changes();
    assert_eq!(dropped, 0);
    let star = Fields::of(&[Field::Star]);
    let expect = |photos: &[PhotoId]| vec![MetadataChange { origin: Origin::Command, photos: photos.iter().map(|id| (*id, star)).collect() }];

    // ids[1] is starred first, so the toggle below changes only ids[0] and ids[2].
    s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[1]]), 0));
    s.execute("photo.star.set", &json!({ "ids": [ids[0], ids[1], ids[2]], "on": true })).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[0], ids[2]]), 0), "only the photos whose star changed");

    // Undo and redo reach the hook too, with the same photos and fields.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[0], ids[2]]), 0));
    assert!(!s.library().photo(ids[0]).unwrap().star);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[0], ids[2]]), 0));

    // What changes nothing tells the hook nothing; queries and polling neither.
    s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    s.execute("library.query", &json!({ "starred": true })).unwrap();
    s.poll();
    assert_eq!(s.take_metadata_changes(), (Vec::new(), 0));

    // Changes queue up until taken, oldest first; `poll` leaves them for the writer (M6.33).
    s.execute("photo.star.toggle", &json!({ "ids": [ids[3]] })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.poll();
    let (changes, _) = s.take_metadata_changes();
    assert_eq!(changes, [expect(&[ids[3]]), expect(&[ids[3]])].concat());
}

#[test]
fn the_new_command_modules_are_joined_into_the_registry() {
    // Whatever M6.14–M6.16 put in the M6 areas is registered, every id once.
    let (_dir, s, _, _) = library(1);
    let ids: Vec<&str> = s.commands().iter().map(|c| c.id).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
    for specs in [
        dalica_engine::cmd::rotate::specs(),
        dalica_engine::cmd::caption::specs(),
        dalica_engine::cmd::hide::specs(),
        dalica_engine::cmd::folder_info::specs(),
        dalica_engine::cmd::tags::specs(),
        dalica_engine::cmd::dates::specs(),
        dalica_engine::cmd::info::specs(),
    ] {
        assert!(specs.iter().all(|spec| ids.contains(&spec.id)));
    }
}
