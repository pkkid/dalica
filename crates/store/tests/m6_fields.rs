//! M6's fields on disk (`data-model.md` §3, §10): every photo field (caption, tags, hidden, recipe,
//! redo list, date) and folder field (description, date, place, hidden) reaches `.dalica.json`,
//! survives the journal and a snapshot, and comes back from the folders after the index is
//! deleted (§10.1); an unknown edit operation keeps its place and the ones after it (§10.4); odd
//! file names round-trip with every field (§10.7); the synced hashes stay in the index; values of
//! the wrong type are kept and reported once.

use std::path::Path;

use dalica_core::{Date, DateTime, FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, FolderInfo, Library, Op, Photo, PhotoInfo, Recipe, Synced};
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::mapping::{self, FolderState, PhotoState};
use dalica_store::{Opened, Outcome, SidecarWriter, Store, import_sidecars, write_folder};
use proptest::prelude::*;

const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

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

fn strings(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

/// Changes everything a user can change in M6, as the ops the engine would apply.
fn change_everything(folders: &[FolderId], ids: &[Vec<PhotoId>]) -> Vec<Op> {
    let (a, b) = (&ids[0], &ids[1]);
    vec![
        Op::SetStars { stars: vec![(a[0], true), (b[2], true)] },
        Op::SetCaptions { captions: vec![(a[0], "Tram 28 at sunset".into()), (a[1], "Ein \"Bild\" ; = ] 東京".into()), (b[3], "x".into())] },
        Op::AddTags { ids: vec![a[0], a[1], b[0]], tags: strings(&["lisbon", "tram"]) },
        Op::AddTags { ids: vec![a[3]], tags: strings(&["a/b", "c,d", "Sunset"]) },
        Op::SetHidden { hidden: vec![(a[2], true), (b[4], true)] },
        Op::SetRecipes {
            recipes: vec![
                (a[0], Recipe { edits: strings(&["rotate(90)", "frobnicate(7)", "crop(0.1,0.1,0.9,0.9)"]), redo: strings(&["sepia()"]) }),
                (b[1], Recipe { edits: strings(&["flip_h()"]), redo: Vec::new() }),
            ],
        },
        Op::SetDates { dates: vec![(a[1], Some(dt("2026-06-04T19:42:10"))), (b[0], Some(dt("1999-12-31T23:59:59")))] },
        Op::SetFolderInfo {
            folders: vec![(
                folders[0],
                FolderInfo {
                    description: "Weekend in Lisbon with the family".into(),
                    date: Some("2026-06-04".parse().unwrap()),
                    place: "Lisbon, Portugal".into(),
                    hidden: false,
                },
            )],
        },
        Op::SetFoldersHidden { hidden: vec![(folders[1], true)] },
        Op::SetSynced { synced: vec![(a[0], Some(Synced { caption: Some(7), tags: Some(8), ..Synced::default() }))] },
    ]
}

fn record(o: &mut Opened, lib: &mut Library, op: &Op) {
    lib.apply(op.clone()).unwrap();
    o.store.append(op).unwrap();
}

fn sidecar(dir: &Path) -> Sidecar {
    Sidecar::parse(&std::fs::read(dir.join(".dalica.json")).unwrap(), &Limits::default()).unwrap()
}

/// The library without its synced hashes (index-only: a rebuild starts without them).
fn without_synced(lib: &Library) -> Library {
    let mut out = lib.clone();
    let clear: Vec<(PhotoId, Option<Synced>)> = lib.synced_all().map(|(id, _)| (id, None)).collect();
    out.apply(Op::SetSynced { synced: clear }).unwrap();
    out
}

/// `data-model.md` §10.1 for every M6 field: scan, change everything, delete the index, rebuild
/// from the folders, compare.
#[test]
fn deleting_the_index_and_rebuilding_gives_every_m6_field_back() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    let mut o = Store::open(lib_dir.path(), NOW).unwrap();
    let mut w = SidecarWriter::new();
    for op in change_everything(&folders, &ids) {
        record(&mut o, &mut lib, &op);
    }
    for &f in &folders {
        w.mark_dirty(f);
    }
    let out = w.flush(&lib, NOW);
    assert!(out.values().all(|o| *o == Outcome::Written), "{out:?}");
    // The synced hashes never reach a sidecar.
    for d in &paths {
        let text = std::fs::read_to_string(d.join(".dalica.json")).unwrap();
        assert!(!text.contains("synced") && !text.contains("rating"), "{text}");
    }
    let hidden = sidecar(paths[1]);
    assert_eq!(hidden.folder().hidden, Field::Value(true), "a hidden folder says so in its own sidecar");

    // The index (journal and snapshot) holds it all.
    o.store.snapshot(&lib).unwrap();
    drop(o);
    let reopened = Store::open(lib_dir.path(), NOW).unwrap();
    assert_eq!((reopened.report.clone(), reopened.library.model()), (vec![], lib.model()));
    drop(reopened);

    // Delete the index; a new scan finds the same folders and photos with nothing said about them.
    for e in std::fs::read_dir(lib_dir.path()).unwrap() {
        std::fs::remove_file(e.unwrap().path()).unwrap();
    }
    let (mut rebuilt, _, _) = scan(&paths);
    let import = import_sidecars(&rebuilt, NOW);
    assert!(import.problems.is_empty() && import.read_only.is_empty(), "{import:?}");
    assert_eq!(import.ops.len(), 3, "stars, photo info, folder info: {:?}", import.ops);
    for op in import.ops {
        rebuilt.apply(op).unwrap();
    }
    assert_eq!(rebuilt.model(), without_synced(&lib).model());
    assert_eq!(rebuilt.tag_index().counts(), lib.tag_index().counts());
    assert!(rebuilt.is_folder_hidden(folders[1]) && rebuilt.is_hidden(ids[0][2]));
    // Importing again finds nothing to change, and writing again changes no byte.
    assert!(import_sidecars(&rebuilt, NOW).ops.is_empty());
    let before: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    for &f in &folders {
        w.mark_dirty(f);
    }
    assert!(w.flush(&rebuilt, NOW).values().all(|o| *o == Outcome::Unchanged));
    let after: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    assert_eq!(after, before);
}

/// Clearing every field takes its keys out again, and a folder left with nothing to say keeps
/// only what other programs put there.
#[test]
fn clearing_every_field_removes_its_keys() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".dalica.json"), br#"{"planted": 1, "photos": {"IMG_0001.jpg": {"x-other": true}}}"#).unwrap();
    let (mut lib, folders, ids) = scan(&[root.path()]);
    let ops = change_everything(&[folders[0], folders[0]], &[ids[0].clone(), ids[0].clone()]);
    let mut undo = Vec::new();
    for op in ops {
        undo.push(lib.apply(op).unwrap());
    }
    let state = mapping::folder_state(&lib, folders[0]).unwrap();
    assert_eq!(write_folder(&state, NOW), Outcome::Written);
    for op in undo.into_iter().rev() {
        lib.apply(op).unwrap();
    }
    let state = mapping::folder_state(&lib, folders[0]).unwrap();
    assert_eq!(write_folder(&state, NOW), Outcome::Written);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(root.path().join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(json, serde_json::json!({"planted": 1, "photos": {"IMG_0001.jpg": {"x-other": true}}}));
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

/// Journal replay and snapshots keep every M6 op, the synced hashes included, and a torn record
/// loses only itself.
#[test]
fn journal_replay_and_snapshots_keep_every_m6_field() {
    let dir = tempfile::tempdir().unwrap();
    let (lib0, folders, ids) = scan(&[Path::new("/photos/lisbon"), Path::new("/photos/porto")]);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    // The scan's own ops first, so the journal holds the whole history.
    let mut lib = Library::new();
    for f in lib0.folders() {
        record(&mut o, &mut lib, &Op::AddFolder { folder: f.clone() });
        record(&mut o, &mut lib, &Op::AddPhotos { photos: lib0.photos_in(f.id).into_iter().cloned().collect() });
    }
    let changes = change_everything(&folders, &ids);
    let (first, rest) = changes.split_at(5);
    for op in first {
        record(&mut o, &mut lib, op);
    }
    let mid = lib.clone();
    o.store.snapshot(&lib).unwrap();
    for op in rest {
        record(&mut o, &mut lib, op);
    }
    // A rename merges tags; a whole info and the synced hashes are set the way rebuilds set them.
    for op in [
        Op::RenameTag { from: "tram".into(), to: "Lisbon".into() },
        Op::SetPhotoInfo { photos: vec![(ids[1][4], PhotoInfo { caption: "whole".into(), hidden: true, ..PhotoInfo::EMPTY })] },
        Op::SetSynced { synced: vec![(ids[1][1], Some(Synced { rating: Some(1), place: Some(2), date: Some(3), ..Synced::default() }))] },
    ] {
        record(&mut o, &mut lib, &op);
    }
    drop(o);
    let replayed = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((replayed.report.clone(), replayed.library.model()), (vec![], lib.model()));
    assert_eq!(replayed.library.synced(ids[0][0]), Some(&Synced { caption: Some(7), tags: Some(8), ..Synced::default() }));
    assert_eq!(replayed.library.tag_index().counts(), lib.tag_index().counts());
    let mut o = replayed;
    o.store.snapshot(&o.library).unwrap();
    drop(o);
    let snapped = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((snapped.report.clone(), snapped.library.model()), (vec![], lib.model()));
    assert_eq!(snapped.store.seq(), snapped.store.snapshot_seq());
    // The snapshot taken half way holds the model as it was then.
    let (back, _) = dalica_store::snapshot::read(&{
        let p = dir.path().join("mid.snap");
        dalica_store::snapshot::write(&p, &mid, 9).unwrap();
        std::fs::read(&p).unwrap()
    })
    .unwrap();
    assert_eq!(back.model(), mid.model());
    // A torn last record loses only that op.
    drop(snapped);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    let before = o.library.clone();
    o.store.append(&Op::SetCaptions { captions: vec![(ids[0][4], "torn".into())] }).unwrap();
    drop(o);
    let log = dir.path().join(dalica_store::journal::FILE);
    let bytes = std::fs::read(&log).unwrap();
    std::fs::write(&log, &bytes[..bytes.len() - 3]).unwrap();
    let torn = Store::open(dir.path(), NOW).unwrap();
    assert_eq!(torn.library.model(), before.model());
}

/// A photo deleted outside Dalica whose entry holds only M6 fields (no star) is Dalica's entry like
/// a starred one (`data-model.md` §7): marked at the folder's next write, kept for 30 days, pruned
/// by the first write after them.
#[test]
fn an_entry_with_only_m6_fields_is_marked_and_pruned_when_its_file_is_gone() {
    use dalica_store::stale::{MISSING_KEY, PRUNE_AFTER};
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: dir.path().to_path_buf() } }).unwrap();
    let mut ids = Vec::new();
    for name in ["a.jpg", "b.jpg"] {
        std::fs::write(dir.path().join(name), name).unwrap();
        let id = lib.next_photo_id();
        let photo = Photo { id, folder: f, name: name.into(), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false };
        lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();
        ids.push(id);
    }
    let info = PhotoInfo {
        caption: "gone soon".into(),
        tags: strings(&["beach"]),
        hidden: true,
        edits: strings(&["rotate(90)"]),
        redo: strings(&["sepia()"]),
        date: Some(dt("2026-06-04T19:42:10")),
    };
    lib.apply(Op::SetPhotoInfo { photos: vec![(ids[1], info)] }).unwrap();
    let t0 = NOW;
    let mut w = SidecarWriter::new();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, t0)[&f], Outcome::Written);

    std::fs::remove_file(dir.path().join("b.jpg")).unwrap();
    lib.apply(Op::RemovePhotos { ids: vec![ids[1]] }).unwrap();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, t0)[&f], Outcome::Written, "the mark is written");
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.path().join(".dalica.json")).unwrap()).unwrap();
    assert!(doc["dalica"][MISSING_KEY]["b.jpg"].is_string(), "{doc}");
    assert_eq!(doc["photos"]["b.jpg"]["caption"], "gone soon", "the entry is kept while it waits");

    // A write 29 days on keeps it; the first write after 30 days prunes it and its mark.
    let day = std::time::Duration::from_secs(24 * 60 * 60);
    for (d, caption) in [(29, "x"), (31, "y")] {
        lib.apply(Op::SetCaptions { captions: vec![(ids[0], caption.into())] }).unwrap();
        w.mark_dirty(f);
        assert_eq!(w.flush(&lib, t0.saturating_add(day * d))[&f], Outcome::Written);
    }
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.path().join(".dalica.json")).unwrap()).unwrap();
    assert!(doc["photos"].get("b.jpg").is_none(), "{doc}");
    assert!(doc["dalica"].get(MISSING_KEY).is_none(), "{doc}");
    assert_eq!(doc["photos"]["a.jpg"]["caption"], "y");
    assert_eq!(PRUNE_AFTER, day * 30);
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
    #![proptest_config(ProptestConfig::with_cases(256))]

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
            .map(|(n, star, info)| (n, PhotoState { star, info }))
            .collect();
        let state = FolderState { id: FolderId::new(1), path: "/x".into(), folder: folder.clone(), photos: photos.clone() };
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
