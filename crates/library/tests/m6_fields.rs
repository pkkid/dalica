//! `dalica-library` M6 fields (`data-model.md` §2–§3, `xmp-interop.md` §5): what each photo and
//! folder gains, the op that changes each field with its exact inverse, the journal names of the
//! new ops, and synced hashes as derived data. Removal and restore, and refused ops changing
//! nothing, are properties of every op (`ops.rs`).

use std::path::PathBuf;

use dalica_core::{Date, DateTime, FolderId, PhotoId};
use dalica_library::{Error, Folder, FolderInfo, Library, Op, Photo, PhotoInfo, Recipe, Synced};
use serde_json::json;

fn photo(id: PhotoId, folder: FolderId, name: &str, date: Option<&str>) -> Photo {
    Photo { date: date.map(|d| d.parse().unwrap()), ..crate::common::photo(id, folder, name) }
}

/// Two folders, three photos in the first and one in the second.
fn library() -> (Library, [FolderId; 2], [PhotoId; 4]) {
    let mut lib = Library::new();
    let (a, b) = (lib.next_folder_id(), lib.next_folder_id());
    lib.apply(Op::AddFolder { folder: Folder { id: a, path: PathBuf::from("/photos/a") } }).unwrap();
    lib.apply(Op::AddFolder { folder: Folder { id: b, path: PathBuf::from("/photos/b") } }).unwrap();
    let ids = [lib.next_photo_id(), lib.next_photo_id(), lib.next_photo_id(), lib.next_photo_id()];
    lib.apply(Op::AddPhotos {
        photos: vec![
            photo(ids[0], a, "1.jpg", Some("2026-06-04T19:42:10")),
            photo(ids[1], a, "2.jpg", None),
            photo(ids[2], a, "3.jpg", Some("2026-06-05T08:00:00")),
            photo(ids[3], b, "4.jpg", None),
        ],
    })
    .unwrap();
    (lib, [a, b], ids)
}

fn dt(s: &str) -> DateTime {
    s.parse().unwrap()
}

/// Applies `op`, checks it bumped the revision once and touched exactly `folders`, then undoes it
/// and checks the model is back; returns the library as `op` left it (applied again).
fn round_trip(lib: &mut Library, op: Op, folders: &[FolderId]) -> Op {
    let before = lib.model().clone();
    let index = lib.tag_index().clone();
    let rev = lib.revision();
    let inverse = lib.apply(op.clone()).unwrap();
    assert_eq!(lib.revision(), rev + 1, "{op:?}");
    let all: Vec<FolderId> = lib.folders().map(|f| f.id).collect();
    for f in all {
        assert_eq!(lib.folder_revision(f) == lib.revision(), folders.contains(&f), "{op:?} touched {f}");
    }
    let after = lib.model().clone();
    let redo = lib.apply(inverse.clone()).unwrap();
    assert_eq!(lib.model(), &before, "{op:?} then {inverse:?}");
    assert_eq!(lib.tag_index(), &index);
    lib.apply(redo).unwrap();
    assert_eq!(lib.model(), &after, "redo of {op:?}");
    inverse
}

#[test]
fn a_new_photo_and_folder_have_nothing_said_about_them() {
    let (lib, [a, _], ids) = library();
    assert_eq!(lib.info(ids[0]), &PhotoInfo::EMPTY);
    assert!(lib.info(ids[0]).is_empty());
    assert_eq!((lib.caption(ids[0]), lib.tags_of(ids[0]), lib.is_hidden(ids[0])), ("", &[][..], false));
    assert_eq!(lib.folder_info(a), &FolderInfo::EMPTY);
    assert!(!lib.is_folder_hidden(a));
    assert_eq!(lib.synced(ids[0]), None);
    assert!(lib.infos().next().is_none() && lib.folder_infos().next().is_none() && lib.synced_all().next().is_none());
    // Unknown ids read as nothing said, never a panic.
    assert_eq!(lib.info(PhotoId::new(999)), &PhotoInfo::EMPTY);
    assert_eq!(lib.folder_info(FolderId::new(999)), &FolderInfo::EMPTY);
    assert_eq!(lib.effective_date(PhotoId::new(999)), None);
}

#[test]
fn captions() {
    let (mut lib, [a, b], ids) = library();
    let op = Op::SetCaptions { captions: vec![(ids[0], "Tram 28 at sunset".into()), (ids[3], "Ünïcode \"quoted\"\nline".into())] };
    round_trip(&mut lib, op, &[a, b]);
    assert_eq!(lib.caption(ids[0]), "Tram 28 at sunset");
    assert_eq!(lib.caption(ids[3]), "Ünïcode \"quoted\"\nline");
    // An empty caption clears it, and the photo leaves the info map.
    round_trip(&mut lib, Op::SetCaptions { captions: vec![(ids[0], String::new())] }, &[a]);
    assert_eq!(lib.infos().map(|(id, _)| id).collect::<Vec<_>>(), [ids[3]]);
    // Setting one photo twice in one op: the last wins, the undo restores the first.
    round_trip(&mut lib, Op::SetCaptions { captions: vec![(ids[1], "one".into()), (ids[1], "two".into())] }, &[a]);
    assert_eq!(lib.caption(ids[1]), "two");
}

#[test]
fn tags_add_remove_rename_set() {
    let (mut lib, [a, b], ids) = library();
    let undo = round_trip(&mut lib, Op::AddTags { ids: ids.to_vec(), tags: vec!["lisbon".into(), "tram".into()] }, &[a, b]);
    assert!(matches!(undo, Op::SetTags { .. }));
    round_trip(&mut lib, Op::RemoveTags { ids: vec![ids[0]], tags: vec!["TRAM".into()] }, &[a]);
    assert_eq!(lib.tags_of(ids[0]), ["lisbon"]);
    round_trip(&mut lib, Op::RenameTag { from: "tram".into(), to: "streetcar".into() }, &[a, b]);
    assert_eq!(lib.tags_of(ids[3]), ["lisbon", "streetcar"]);
    round_trip(&mut lib, Op::SetTags { tags: vec![(ids[2], vec![])] }, &[a]);
    assert!(lib.tags_of(ids[2]).is_empty());
    // Tagging a thousand photos is one op and one revision.
    let mut big = Library::new();
    let f = big.next_folder_id();
    big.apply(Op::AddFolder { folder: Folder { id: f, path: PathBuf::from("/x") } }).unwrap();
    let many: Vec<PhotoId> = (0..1000).map(|_| big.next_photo_id()).collect();
    big.apply(Op::AddPhotos { photos: many.iter().map(|&id| photo(id, f, &format!("{id}.jpg"), None)).collect() }).unwrap();
    round_trip(&mut big, Op::AddTags { ids: many.clone(), tags: vec!["beach".into()] }, &[f]);
    assert_eq!(big.tag_index().count("beach"), 1000);
}

#[test]
fn hidden_photos_and_folders() {
    let (mut lib, [a, b], ids) = library();
    round_trip(&mut lib, Op::SetHidden { hidden: vec![(ids[1], true), (ids[3], true)] }, &[a, b]);
    assert!(lib.is_hidden(ids[1]) && lib.is_hidden(ids[3]) && !lib.is_hidden(ids[0]));
    round_trip(&mut lib, Op::SetHidden { hidden: vec![(ids[1], false)] }, &[a]);
    assert!(!lib.is_hidden(ids[1]));
    round_trip(&mut lib, Op::SetFoldersHidden { hidden: vec![(b, true)] }, &[b]);
    assert!(lib.is_folder_hidden(b) && !lib.is_folder_hidden(a));
    // Folder Properties keeps the hidden flag it is given.
    let info = FolderInfo { description: "Weekend in Lisbon".into(), ..lib.folder_info(b).clone() };
    round_trip(&mut lib, Op::SetFolderInfo { folders: vec![(b, info)] }, &[b]);
    assert!(lib.is_folder_hidden(b));
    round_trip(&mut lib, Op::SetFoldersHidden { hidden: vec![(b, false)] }, &[b]);
    assert_eq!(lib.folder_info(b).description, "Weekend in Lisbon");
}

#[test]
fn recipes_are_opaque_strings_kept_verbatim() {
    let (mut lib, [a, _], ids) = library();
    let recipe = Recipe {
        edits: vec!["rotate(90)".into(), "legacy:unknown(1;2)".into(), "future_op(0.5,\"x\")".into(), "flip_h()".into()],
        redo: vec!["glow(0.4,3.0)".into()],
    };
    round_trip(&mut lib, Op::SetRecipes { recipes: vec![(ids[0], recipe.clone())] }, &[a]);
    assert_eq!(lib.info(ids[0]).recipe(), recipe);
    assert_eq!((&lib.info(ids[0]).edits, &lib.info(ids[0]).redo), (&recipe.edits, &recipe.redo));
    round_trip(&mut lib, Op::SetRecipes { recipes: vec![(ids[0], Recipe::default())] }, &[a]);
    assert!(lib.info(ids[0]).is_empty());
    let long = Recipe { edits: vec!["x".repeat(dalica_library::MAX_OPERATION + 1)], redo: vec![] };
    assert!(matches!(lib.apply(Op::SetRecipes { recipes: vec![(ids[0], long)] }), Err(Error::TooLong { .. })));
    let many = Recipe { edits: vec!["sepia()".into(); dalica_library::MAX_OPERATIONS + 1], redo: vec![] };
    assert!(matches!(lib.apply(Op::SetRecipes { recipes: vec![(ids[0], many)] }), Err(Error::TooMany { .. })));
}

#[test]
fn a_date_set_by_the_user_keeps_the_files_date_beside_it() {
    let (mut lib, [a, _], ids) = library();
    let user = dt("2026-06-04T21:00:00");
    round_trip(&mut lib, Op::SetDates { dates: vec![(ids[0], Some(user)), (ids[1], Some(user))] }, &[a]);
    assert_eq!(lib.effective_date(ids[0]), Some(user));
    assert_eq!(lib.photo(ids[0]).unwrap().date, Some(dt("2026-06-04T19:42:10")), "the file's date is kept");
    assert_eq!(lib.effective_date(ids[1]), Some(user), "a photo without a date of its own gets one");
    assert_eq!(lib.sort_date(lib.photo(ids[1]).unwrap()), Some(user));
    // Clearing goes back to the file's date, then to the file time.
    round_trip(&mut lib, Op::SetDates { dates: vec![(ids[0], None), (ids[1], None)] }, &[a]);
    assert_eq!(lib.effective_date(ids[0]), Some(dt("2026-06-04T19:42:10")));
    assert_eq!(lib.effective_date(ids[1]), None);
    assert_eq!(lib.sort_date(lib.photo(ids[1]).unwrap()), DateTime::from_unix_secs(1_700_000_000));
    // A rescan that updates the photo from its file keeps the user's date.
    round_trip(&mut lib, Op::SetDates { dates: vec![(ids[2], Some(user))] }, &[a]);
    let rescanned = Photo { size: 99, date: Some(dt("2001-01-01T00:00:00")), ..lib.photo(ids[2]).unwrap().clone() };
    lib.apply(Op::UpdatePhotos { photos: vec![rescanned] }).unwrap();
    assert_eq!(lib.effective_date(ids[2]), Some(user));
}

#[test]
fn folder_info() {
    let (mut lib, [a, _], _) = library();
    let info = FolderInfo {
        description: "Weekend in Lisbon with the family".into(),
        date: Some(Date::new(2026, 6, 4).unwrap()),
        place: "Lisbon, Portugal".into(),
        hidden: false,
    };
    round_trip(&mut lib, Op::SetFolderInfo { folders: vec![(a, info.clone())] }, &[a]);
    assert_eq!(lib.folder_info(a), &info);
    round_trip(&mut lib, Op::SetFolderInfo { folders: vec![(a, FolderInfo::default())] }, &[a]);
    assert!(lib.folder_infos().next().is_none());
    let long = FolderInfo { place: "p".repeat(dalica_library::MAX_PLACE + 1), ..FolderInfo::default() };
    assert!(matches!(lib.apply(Op::SetFolderInfo { folders: vec![(a, long)] }), Err(Error::TooLong { .. })));
    let long = FolderInfo { description: "d".repeat(dalica_library::MAX_DESCRIPTION + 1), ..FolderInfo::default() };
    assert!(lib.apply(Op::SetFolderInfo { folders: vec![(a, long)] }).is_err());
}

#[test]
fn synced_hashes_are_derived_data() {
    let (mut lib, [a, _], ids) = library();
    let (rev, thumbs, folder_rev) = (lib.revision(), lib.thumbnail_revision(), lib.folder_revision(a));
    let synced = Synced { caption: Some(7), tags: Some(u64::MAX), ..Synced::default() };
    let op = Op::SetSynced { synced: vec![(ids[0], Some(synced))] };
    assert!(op.is_derived());
    let inverse = lib.apply(op).unwrap();
    assert!(inverse.is_derived());
    assert_eq!(lib.synced(ids[0]), Some(&synced));
    assert_eq!((lib.revision(), lib.thumbnail_revision(), lib.folder_revision(a)), (rev, thumbs, folder_rev), "no revision moves");
    // Its JSON keeps a full 64-bit hash.
    let json = serde_json::to_value(&inverse).unwrap();
    assert_eq!(serde_json::from_value::<Op>(json).unwrap(), inverse);
    let back = serde_json::to_string(&Op::SetSynced { synced: vec![(ids[0], Some(synced))] }).unwrap();
    assert!(back.contains("18446744073709551615"), "{back}");
    // An all-None value clears the entry; the inverse puts it back.
    let undo = lib.apply(Op::SetSynced { synced: vec![(ids[0], Some(Synced::default()))] }).unwrap();
    assert_eq!(lib.synced(ids[0]), None);
    lib.apply(undo).unwrap();
    assert_eq!(lib.synced(ids[0]), Some(&synced));
    // A batch of synced hashes is derived; one mixed with a user op is not.
    assert!(Op::Batch { ops: vec![Op::SetSynced { synced: vec![] }] }.is_derived());
    assert!(!Op::Batch { ops: vec![Op::SetSynced { synced: vec![] }, Op::SetHidden { hidden: vec![] }] }.is_derived());
    lib.apply(inverse).unwrap();
    assert_eq!(lib.synced(ids[0]), None);
}

#[test]
fn journal_records_of_the_new_ops() {
    let p = PhotoId::new(3);
    let f = FolderId::new(2);
    let cases = [
        (Op::SetCaptions { captions: vec![(p, "c".into())] }, json!({"op": "setCaptions", "captions": [[3, "c"]]})),
        (Op::AddTags { ids: vec![p], tags: vec!["t".into()] }, json!({"op": "addTags", "ids": [3], "tags": ["t"]})),
        (Op::RemoveTags { ids: vec![p], tags: vec!["t".into()] }, json!({"op": "removeTags", "ids": [3], "tags": ["t"]})),
        (Op::RenameTag { from: "a".into(), to: "b".into() }, json!({"op": "renameTag", "from": "a", "to": "b"})),
        (Op::SetTags { tags: vec![(p, vec!["t".into()])] }, json!({"op": "setTags", "tags": [[3, ["t"]]]})),
        (Op::SetHidden { hidden: vec![(p, true)] }, json!({"op": "setHidden", "hidden": [[3, true]]})),
        (
            Op::SetRecipes { recipes: vec![(p, Recipe { edits: vec!["rotate(90)".into()], redo: vec![] })] },
            json!({"op": "setRecipes", "recipes": [[3, {"edits": ["rotate(90)"], "redo": []}]]}),
        ),
        (
            Op::SetDates { dates: vec![(p, Some(dt("2026-06-04T19:42:10"))), (p, None)] },
            json!({"op": "setDates", "dates": [[3, "2026-06-04T19:42:10"], [3, null]]}),
        ),
        (
            Op::SetPhotoInfo {
                photos: vec![(p, PhotoInfo { caption: "c".into(), hidden: true, ..PhotoInfo::default() }), (p, PhotoInfo::default())],
            },
            json!({"op": "setPhotoInfo", "photos": [[3, {"caption": "c", "hidden": true}], [3, {}]]}),
        ),
        (
            Op::SetFolderInfo { folders: vec![(f, FolderInfo { date: Some(Date::new(2026, 6, 4).unwrap()), ..FolderInfo::default() })] },
            json!({"op": "setFolderInfo", "folders": [[2, {"date": "2026-06-04"}]]}),
        ),
        (Op::SetFoldersHidden { hidden: vec![(f, true)] }, json!({"op": "setFoldersHidden", "hidden": [[2, true]]})),
        (
            Op::SetSynced { synced: vec![(p, Some(Synced { tags: Some(5), ..Synced::default() })), (p, None)] },
            json!({"op": "setSynced", "synced": [[3, {"tags": 5}], [3, null]]}),
        ),
    ];
    for (op, want) in cases {
        assert_eq!(serde_json::to_value(&op).unwrap(), want);
        assert_eq!(serde_json::from_value::<Op>(want).unwrap(), op);
    }
    // Keys left out of an info read as their defaults.
    let lenient: PhotoInfo = serde_json::from_value(json!({"tags": ["a"]})).unwrap();
    assert_eq!(lenient, PhotoInfo { tags: vec!["a".into()], ..PhotoInfo::default() });
}
