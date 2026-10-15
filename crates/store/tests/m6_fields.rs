//! M6's fields on disk (`data-model.md` §3, §10): every photo field (caption, tags, hidden, recipe,
//! redo list, date) and folder field (description, date, place, hidden) reaching `.dalica.json`,
//! surviving the journal and a snapshot, and coming back from the folders after the index is
//! deleted (§10.1) is `fields.rs`, with M8's and M9's. Here: an unknown edit operation keeps its
//! place and the ones after it (§10.4); odd file names round-trip with every field (§10.7); values
//! of the wrong type are kept and reported once.

use std::path::Path;

use crate::common::NOW;
use crate::common::strings;
use dalica_core::{Date, DateTime, FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, FolderInfo, Library, Op, Photo, PhotoInfo, Recipe};
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::mapping::{self, FolderState, PhotoState};
use dalica_store::{Outcome, SidecarWriter, import_sidecars, write_folder};
use proptest::prelude::*;

/// Names that are hard to write (`data-model.md` §10.7); Windows cannot name a file with `"`, but
/// these only ever reach the model and the sidecar.
fn names() -> Vec<String> {
    vec!["IMG_0001.jpg".into(), "a]b \"c\";=.jpg".into(), "東京 タワー.jpg".into(), format!("{}.jpg", "長".repeat(80)), "IMG_0005.jpg".into()]
}

/// A scan of `roots`: one folder per root holding [`names`], every photo with the same ids each
/// time (so a rebuilt model can be compared with the first).
fn scan(roots: &[&Path]) -> (Library, Vec<FolderId>, Vec<Vec<PhotoId>>) {
    let mut lib = Library::new();
    let mut folders = Vec::new();
    let mut ids = Vec::new();
    for root in roots {
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.to_path_buf() } }).unwrap();
        let mine: Vec<PhotoId> = names().iter().map(|_| lib.next_photo_id()).collect();
        let photos = mine
            .iter()
            .zip(names())
            .map(|(&id, name)| Photo { id, folder: f, name, size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false })
            .collect();
        lib.apply(Op::AddPhotos { photos }).unwrap();
        folders.push(f);
        ids.push(mine);
    }
    (lib, folders, ids)
}

fn dt(s: &str) -> DateTime {
    s.parse().unwrap()
}

fn sidecar(dir: &Path) -> Sidecar {
    Sidecar::parse(&std::fs::read(dir.join(".dalica.json")).unwrap(), &Limits::default()).unwrap()
}

/// `data-model.md` §10.4: an operation Dalica does not know keeps its place, and the ones after it,
/// through an import and any number of rewrites.
#[test]
fn an_unknown_operation_keeps_the_ones_after_it_through_a_rewrite() {
    let root = tempfile::tempdir().unwrap();
    let edits = ["rotate(90)", "warp_drive(9.5,\"x\")", "crop(0.08,0.10,0.92,0.86)", "sepia()"];
    std::fs::write(
        root.path().join(".dalica.json"),
        serde_json::to_vec(&serde_json::json!({"photos": {"IMG_0001.jpg": {"edits": edits, "redo": ["teleport()", "glow(0.4,3.0)"]}}})).unwrap(),
    )
    .unwrap();
    let (mut lib, folders, ids) = scan(&[root.path()]);
    for op in import_sidecars(&lib, NOW).ops {
        lib.apply(op).unwrap();
    }
    assert_eq!(lib.info(ids[0][0]).edits, strings(&edits));
    // Another field of the photo changes, then the recipe itself grows: the sidecar keeps every
    // operation in order each time.
    let mut w = SidecarWriter::new();
    lib.apply(Op::SetCaptions { captions: vec![(ids[0][0], "c".into())] }).unwrap();
    w.mark_dirty(folders[0]);
    w.flush(&lib, NOW);
    assert_eq!(sidecar(root.path()).photo("IMG_0001.jpg").edits, Field::Value(strings(&edits)));
    let mut longer = strings(&edits);
    longer.push("bw()".into());
    lib.apply(Op::SetRecipes { recipes: vec![(ids[0][0], Recipe { edits: longer.clone(), redo: Vec::new() })] }).unwrap();
    w.mark_dirty(folders[0]);
    w.flush(&lib, NOW);
    let p = sidecar(root.path()).photo("IMG_0001.jpg");
    assert_eq!((p.edits, p.redo, p.caption), (Field::Value(longer), Field::Absent, Field::Value("c".into())));
}

/// Values of the wrong type, and a recipe past the caps, are kept in the file as they are and
/// reported once; the model keeps its own value, and only the user's own value replaces them.
#[test]
fn wrong_types_are_kept_reported_once_and_replaced_only_by_the_user() {
    let root = tempfile::tempdir().unwrap();
    let too_long = vec!["x()"; dalica_library::MAX_OPERATIONS + 1];
    let long_caption = "é".repeat(dalica_library::MAX_CAPTION);
    let doc = serde_json::json!({
        "folder": {"description": 7, "date": "someday", "place": "Porto", "hidden": "no"},
        "photos": {
            "IMG_0001.jpg": {"caption": ["not", "text"], "tags": "beach", "date": "2026-13-01T00:00:00", "star": "yes"},
            "IMG_0005.jpg": {"edits": too_long, "caption": long_caption, "tags": ["  Beach ", "beach", "sea\nside", ""]}
        }
    });
    let original = serde_json::to_vec_pretty(&doc).unwrap();
    std::fs::write(root.path().join(".dalica.json"), &original).unwrap();
    let (mut lib, folders, ids) = scan(&[root.path()]);
    let import = import_sidecars(&lib, NOW);
    let problems: Vec<&str> = import.problems.iter().map(|(_, p)| p.as_str()).collect();
    assert_eq!(problems.len(), 9, "{problems:#?}");
    for p in [
        "/folder/description",
        "/folder/date",
        "/folder/hidden",
        "/photos/IMG_0001.jpg/caption",
        "/photos/IMG_0001.jpg/tags",
        "/photos/IMG_0001.jpg/date",
        "/photos/IMG_0001.jpg/star",
    ] {
        assert_eq!(problems.iter().filter(|q| q.starts_with(p)).count(), 1, "{p}: {problems:#?}");
    }
    assert_eq!(problems.iter().filter(|q| q.starts_with("IMG_0005.jpg: the recipe")).count(), 1, "{problems:#?}");
    assert_eq!(problems.iter().filter(|q| q.starts_with("IMG_0005.jpg: the caption")).count(), 1, "{problems:#?}");
    for op in import.ops {
        lib.apply(op).unwrap();
    }
    let (a, e) = (ids[0][0], ids[0][4]);
    assert_eq!(lib.info(a), &PhotoInfo::EMPTY);
    assert!(!lib.photo(a).unwrap().star);
    assert_eq!(lib.folder_info(folders[0]), &FolderInfo { place: "Porto".into(), ..FolderInfo::EMPTY });
    // Text past its cap is shown cut; tags are cleaned; the recipe past the caps is not used.
    assert_eq!(lib.info(e).caption.len(), dalica_library::MAX_CAPTION);
    assert_eq!(lib.info(e).tags, strings(&["Beach", "sea side"]));
    assert!(lib.info(e).edits.is_empty());

    // A change elsewhere in the folder rewrites it: every kept value is still there, as it was.
    let mut w = SidecarWriter::new();
    lib.apply(Op::SetStars { stars: vec![(ids[0][2], true)] }).unwrap();
    w.mark_dirty(folders[0]);
    assert_eq!(w.flush(&lib, NOW)[&folders[0]], Outcome::Written);
    let mut expected = doc.clone();
    expected["photos"]["東京 タワー.jpg"] = serde_json::json!({"star": true});
    let now: serde_json::Value = serde_json::from_slice(&std::fs::read(root.path().join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(now, expected);

    // The user's own caption and hide replace the wrong-type values; nothing else moves.
    lib.apply(Op::SetCaptions { captions: vec![(a, "mine".into())] }).unwrap();
    lib.apply(Op::SetFoldersHidden { hidden: vec![(folders[0], true)] }).unwrap();
    w.mark_dirty(folders[0]);
    w.flush(&lib, NOW);
    let now: serde_json::Value = serde_json::from_slice(&std::fs::read(root.path().join(".dalica.json")).unwrap()).unwrap();
    expected["photos"]["IMG_0001.jpg"]["caption"] = serde_json::json!("mine");
    expected["folder"]["hidden"] = serde_json::json!(true);
    assert_eq!(now, expected);
}

/// A sidecar that already says what the model says is never rewritten, even where the model would
/// spell it differently (an EXIF-style date, a tag list the library would clean).
#[test]
fn equal_content_in_another_spelling_is_left_byte_for_byte() {
    let root = tempfile::tempdir().unwrap();
    let raw = r#"{"photos":{"IMG_0001.jpg":{"date":"2026:06:04 19:42:10","tags":["a","A"],"hidden":false}},"folder":{"date":"2026:06:04"}}"#;
    // In Dalica's own layout, so that only the content can make a difference.
    let text = String::from_utf8(Sidecar::parse(raw.as_bytes(), &Limits::default()).unwrap().print()).unwrap();
    std::fs::write(root.path().join(".dalica.json"), &text).unwrap();
    let (mut lib, folders, ids) = scan(&[root.path()]);
    for op in import_sidecars(&lib, NOW).ops {
        lib.apply(op).unwrap();
    }
    assert_eq!(lib.info(ids[0][0]).date, Some(dt("2026-06-04T19:42:10")));
    assert_eq!(lib.info(ids[0][0]).tags, strings(&["a"]));
    assert_eq!(lib.folder_info(folders[0]).date, Some(Date::new(2026, 6, 4).unwrap()));
    let state = mapping::folder_state(&lib, folders[0]).unwrap();
    assert_eq!(write_folder(&state, NOW), Outcome::Unchanged);
    assert_eq!(std::fs::read_to_string(root.path().join(".dalica.json")).unwrap(), text);
}

/// The mapping's own round trip on any values: what [`mapping::apply`] writes, the readers give
/// back, and printing then parsing changes nothing.
fn info() -> impl Strategy<Value = PhotoInfo> {
    let list = proptest::collection::vec("\\PC{0,10}", 0..4);
    (
        "\\PC{0,30}",
        proptest::collection::vec("\\PC{0,12}", 0..5),
        any::<bool>(),
        list.clone(),
        list,
        proptest::option::of((1u16..=9999, 1u8..=12, 1u8..=28, 0u8..24, 0u8..60, 0u8..60)),
    )
        .prop_map(|(caption, tags, hidden, edits, redo, date)| PhotoInfo {
            caption: dalica_library::clean_caption(&caption),
            tags: dalica_library::clean_tags(tags.iter().map(String::as_str)),
            hidden,
            edits,
            redo,
            date: date.map(|(y, m, d, h, mi, s)| DateTime::new(Date::new(y, m, d).unwrap(), h, mi, s).unwrap()),
        })
}

fn folder_info() -> impl Strategy<Value = FolderInfo> {
    ("\\PC{0,30}", proptest::option::of((1u16..=9999, 1u8..=12, 1u8..=28)), "\\PC{0,20}", any::<bool>()).prop_map(|(d, date, p, hidden)| FolderInfo {
        description: dalica_library::clean_description(&d),
        date: date.map(|(y, m, d)| Date::new(y, m, d).unwrap()),
        place: dalica_library::clean_place(&p),
        hidden,
    })
}

proptest! {
    #![proptest_config(crate::common::cases(256, 32))]

    #[test]
    fn sidecars_with_every_field_round_trip(
        photos in proptest::collection::vec(("\\PC{1,16}|[\\]\";=.]{1,6}", any::<bool>(), info()), 0..6),
        folder in folder_info(),
        planted in proptest::option::of("\\PC{0,8}"),
    ) {
        let mut sidecar = Sidecar::new();
        if let Some(p) = &planted {
            let text = format!("{{\"planted\": {}}}", serde_json::Value::String(p.clone()));
            sidecar = Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap();
        }
        let mut seen = std::collections::HashSet::new();
        let photos: Vec<(String, PhotoState)> = photos
            .into_iter()
            .filter(|(n, ..)| seen.insert(n.clone()))
            .map(|(n, star, info)| (n, PhotoState { star, info, ..PhotoState::default() }))
            .collect();
        let state = FolderState { id: FolderId::new(1), path: "/x".into(), folder: folder.clone(), photos: photos.clone(), ..FolderState::default() };
        prop_assert!(mapping::apply(&state, &mut sidecar).is_empty());
        let back = Sidecar::parse(&sidecar.print(), &Limits::default()).unwrap();
        prop_assert_eq!(&back, &sidecar);
        prop_assert!(back.problems().is_empty(), "{:?}", back.problems());
        prop_assert_eq!(mapping::read_folder(&back).merged(&FolderInfo::EMPTY), folder);
        for (name, want) in &photos {
            let got = mapping::read_photo(&back, name).merged(&PhotoState::default());
            prop_assert_eq!(&got, want);
        }
        // Writing the same state again changes nothing.
        let mut again = back.clone();
        prop_assert!(mapping::apply(&state, &mut again).is_empty());
        prop_assert_eq!(again.print(), back.print());
    }
}
