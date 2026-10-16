//! `dalica-library` M9 fields (`data-model.md` §2–§4 and §8): search facts, content digests and
//! first-indexed times are index-only (loading them bumps no revision); the last-changed time is
//! stamped on the op of a change and undone with it; Recently Updated follows a virtual clock;
//! photos moved or renamed keep their id and everything said about them; folders move with their
//! subtrees; folder photo sorts; journal names, refusals and caps. A removed photo taking all of
//! it and coming back whole is a property of every op (`ops.rs`).

use std::path::PathBuf;
use std::time::Duration;

use dalica_core::{AlbumId, Clock, FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_library::{
    AlbumInfo, ContentDigest, Error, Facts, Folder, Library, MAX_CAMERA, MAX_DIGEST, MAX_FILE_NAME, Op, Photo, PhotoInfo, PhotoSort, SaveState,
    Saved, SortBy,
};

const DAY: Duration = Duration::from_secs(86_400);

fn photo(id: PhotoId, folder: FolderId, name: &str, size: u64, day: u8) -> Photo {
    let date = format!("2026-06-{day:02}T12:00:00").parse().ok();
    Photo { size, date, mtime: Timestamp::from_millis(1_600_000_000_000), ..crate::common::photo(id, folder, name) }
}

/// Folders `/p/a` and `/p/b`; photos `c.jpg` (100 B, June 3), `a.jpg` (300 B, June 1) and
/// `B.jpg` (200 B, June 2) in the first, `z.jpg` in the second.
fn library() -> (Library, [FolderId; 2], [PhotoId; 4]) {
    let mut lib = Library::new();
    let folders = [lib.next_folder_id(), lib.next_folder_id()];
    for (id, path) in folders.iter().zip(["/p/a", "/p/b"]) {
        lib.apply(Op::AddFolder { folder: Folder { id: *id, path: PathBuf::from(path) } }).unwrap();
    }
    let ids = [lib.next_photo_id(), lib.next_photo_id(), lib.next_photo_id(), lib.next_photo_id()];
    lib.apply(Op::AddPhotos {
        photos: vec![
            photo(ids[0], folders[0], "c.jpg", 100, 3),
            photo(ids[1], folders[0], "a.jpg", 300, 1),
            photo(ids[2], folders[0], "B.jpg", 200, 2),
            photo(ids[3], folders[1], "z.jpg", 50, 9),
        ],
    })
    .unwrap();
    (lib, folders, ids)
}

/// Applies a derived op: no revision, no folder revision, an exact derived inverse.
fn derived(lib: &mut Library, op: Op) {
    assert!(op.is_derived(), "{op:?}");
    let before = lib.model().clone();
    let (rev, thumbs) = (lib.revision(), lib.thumbnail_revision());
    let folder_revs: Vec<u64> = lib.folders().map(|f| lib.folder_revision(f.id)).collect();
    let inverse = lib.apply(op.clone()).unwrap();
    assert!(inverse.is_derived());
    assert_eq!((lib.revision(), lib.thumbnail_revision()), (rev, thumbs), "loading {op:?} bumps no revision");
    assert_eq!(lib.folders().map(|f| lib.folder_revision(f.id)).collect::<Vec<_>>(), folder_revs);
    let after = lib.model().clone();
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before);
    lib.apply(op).unwrap();
    assert_eq!(lib.model(), &after);
}

/// Applies an ordinary op, checks one revision and an exact inverse, applies it again; returns
/// the inverse.
fn round_trip(lib: &mut Library, op: Op) -> Op {
    assert!(!op.is_derived(), "{op:?}");
    let before = lib.model().clone();
    let rev = lib.revision();
    let inverse = lib.apply(op.clone()).unwrap();
    assert_eq!(lib.revision(), rev + 1);
    let after = lib.model().clone();
    lib.apply(inverse.clone()).unwrap();
    assert_eq!(lib.model(), &before, "{op:?} undone");
    lib.apply(op).unwrap();
    assert_eq!(lib.model(), &after);
    inverse
}

fn ms(days: i64) -> Timestamp {
    Timestamp::from_millis(1_790_000_000_000 + days * 86_400_000)
}

#[test]
fn facts_digests_and_times_are_index_only() {
    let (mut lib, _, ids) = library();
    let canon = Facts { make: "Canon".into(), model: "EOS 5D Mark IV".into(), video: false, geotagged: true };
    derived(&mut lib, Op::SetFacts { facts: vec![(ids[0], canon.clone()), (ids[1], Facts { video: true, ..Facts::default() })] });
    assert_eq!(lib.facts(ids[0]), &canon);
    assert!(lib.facts(ids[1]).video && lib.facts(ids[2]).is_empty());
    // The default clears, so equal models compare equal.
    derived(&mut lib, Op::SetFacts { facts: vec![(ids[1], Facts::default())] });
    assert_eq!(lib.facts_all().count(), 1);

    let digest = ContentDigest { size: 100, mtime: Timestamp::from_millis(1_600_000_000_000), digest: "md5:0123abcd".into() };
    derived(&mut lib, Op::SetDigests { digests: vec![(ids[0], Some(digest.clone()))] });
    assert_eq!(lib.content_digest(ids[0]), Some("md5:0123abcd"));
    // A rewritten file (another size) has no digest until one is made again.
    let mut grown = lib.photo(ids[0]).unwrap().clone();
    grown.size = 101;
    lib.apply(Op::UpdatePhotos { photos: vec![grown] }).unwrap();
    assert_eq!(lib.content_digest(ids[0]), None);
    assert_eq!(lib.digest_record(ids[0]), Some(&digest), "the record stays for the next look");

    derived(&mut lib, Op::SetFirstIndexed { indexed: vec![(ids[0], Some(ms(0))), (ids[1], Some(ms(1)))] });
    derived(&mut lib, Op::SetLastChanged { changed: vec![(ids[2], Some(ms(2)))] });
    assert_eq!((lib.first_indexed(ids[0]), lib.last_changed(ids[2]), lib.last_changed(ids[0])), (Some(ms(0)), Some(ms(2)), None));
    // A scan records the first-indexed time of the photos it adds, once.
    let op = lib.first_indexed_op(&[ids[0], ids[2], ids[2], PhotoId::new(999)], ms(5));
    assert_eq!(op, Op::SetFirstIndexed { indexed: vec![(ids[2], Some(ms(5)))] });
    derived(&mut lib, op);

    // Refusals and caps change nothing.
    let before = (lib.model().clone(), lib.revision());
    for op in [
        Op::SetFacts { facts: vec![(ids[0], Facts { make: "x".repeat(MAX_CAMERA + 1), ..Facts::default() })] },
        Op::SetFacts { facts: vec![(PhotoId::new(999), Facts::default())] },
        Op::SetDigests { digests: vec![(ids[0], Some(ContentDigest { digest: "x".repeat(MAX_DIGEST + 1), ..digest.clone() }))] },
        Op::SetFirstIndexed { indexed: vec![(PhotoId::new(999), None)] },
        Op::SetLastChanged { changed: vec![(ids[0], None), (PhotoId::new(999), None)] },
    ] {
        assert!(lib.apply(op.clone()).is_err(), "{op:?}");
        assert_eq!((lib.model().clone(), lib.revision()), before);
    }
}

#[test]
fn stamped_ops_record_the_change_time() {
    let (mut lib, [a, _], ids) = library();
    lib.apply(Op::AddTags { ids: vec![ids[1]], tags: vec!["tram".into()] }).unwrap();
    // A star: one step; the time comes with the op and goes with its undo.
    let op = lib.stamped(Op::SetStars { stars: vec![(ids[0], true)] }, ms(0));
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op, "the journal replays the same time");
    let undo = round_trip(&mut lib, op);
    assert_eq!(lib.last_changed(ids[0]), Some(ms(0)));
    lib.apply(undo).unwrap();
    assert_eq!(lib.last_changed(ids[0]), None);

    // A tag rename stamps the photos that hold the tag.
    let op = lib.stamped(Op::RenameTag { from: "Tram".into(), to: "trams".into() }, ms(1));
    lib.apply(op).unwrap();
    assert_eq!(lib.last_changed_all().collect::<Vec<_>>(), [(ids[1], ms(1))]);

    // Album memberships and moves are changes too.
    let album = AlbumId::parse("a0001").unwrap();
    lib.apply(Op::CreateAlbums { albums: vec![(album.clone(), AlbumInfo::named("A"))] }).unwrap();
    let add = lib.album_add_op(&album, &[ids[2]], None).unwrap();
    lib.apply(lib.stamped(add, ms(2))).unwrap();
    lib.apply(lib.stamped(Op::MovePhotos { moves: vec![(ids[3], a, "moved.jpg".into())] }, ms(3))).unwrap();
    assert_eq!((lib.last_changed(ids[2]), lib.last_changed(ids[3])), (Some(ms(2)), Some(ms(3))));

    // What the files say (a scan) and derived data are not Dalica's changes: the op comes back as
    // it was.
    let scan = Op::UpdatePhotos { photos: vec![lib.photo(ids[0]).unwrap().clone()] };
    assert_eq!(lib.stamped(scan.clone(), ms(4)), scan);
    let facts = Op::SetFacts { facts: vec![(ids[0], Facts::default())] };
    assert_eq!(lib.stamped(facts.clone(), ms(4)), facts);
    // A photo the op removes is left out, so the stamped op still applies.
    let op = lib
        .stamped(Op::Batch { ops: vec![Op::SetCaptions { captions: vec![(ids[0], "gone".into())] }, Op::RemovePhotos { ids: vec![ids[0]] }] }, ms(5));
    lib.apply(op).unwrap();
    assert!(lib.photo(ids[0]).is_none());
    // Photos the op brings back (an undone delete) are stamped.
    let op = Op::Batch {
        ops: vec![
            Op::AddPhotos { photos: vec![photo(PhotoId::new(50), a, "new.jpg", 1, 1)] },
            Op::SetCaptions { captions: vec![(PhotoId::new(50), "back".into())] },
        ],
    };
    lib.apply(lib.stamped(op, ms(6))).unwrap();
    assert_eq!(lib.last_changed(PhotoId::new(50)), Some(ms(6)));
}

/// Recently Updated (`organizing.md` §4.1): the later of the last change and the file's time, the
/// file's time only once it is after the first-indexed time; being indexed is no change. Within the
/// days asked for before now, newest first.
#[test]
fn recently_updated_follows_a_virtual_clock() {
    let (mut lib, _, ids) = library();
    let clock = VirtualClock::new(ms(100));
    let recent = |lib: &Library, clock: &VirtualClock| lib.recently_updated(clock.now(), 30);
    // Indexed two months ago; the files are older still.
    let op = lib.first_indexed_op(&ids, clock.now().saturating_sub(DAY * 60));
    lib.apply(op).unwrap();
    assert!(recent(&lib, &clock).is_empty());
    assert_eq!(lib.updated_time(lib.photo(ids[0]).unwrap()), Library::NEVER_UPDATED);

    let op = lib.stamped(Op::SetStars { stars: vec![(ids[1], true)] }, clock.now());
    lib.apply(op).unwrap();
    assert_eq!(recent(&lib, &clock), [ids[1]]);
    clock.advance(DAY * 10);
    let op = lib.stamped(Op::SetCaptions { captions: vec![(ids[2], "Tram 28".into())] }, clock.now());
    lib.apply(op).unwrap();
    assert_eq!(recent(&lib, &clock), [ids[2], ids[1]], "newest first");
    clock.advance(DAY * 25);
    assert_eq!(recent(&lib, &clock), [ids[2]], "35 days since the star");
    assert_eq!(lib.recently_updated(clock.now(), 40), [ids[2], ids[1]], "the setting's days");
    clock.advance(DAY * 6);
    assert!(recent(&lib, &clock).is_empty());

    // A file rewritten outside Dalica after it was indexed counts by its own time; a photo indexed
    // today does not (being imported is no change), until its file changes after that.
    let mut touched = lib.photo(ids[3]).unwrap().clone();
    touched.mtime = clock.now().saturating_sub(DAY);
    lib.apply(Op::UpdatePhotos { photos: vec![touched] }).unwrap();
    let new = lib.next_photo_id();
    let folder = lib.photo(ids[0]).unwrap().folder;
    lib.apply(Op::AddPhotos { photos: vec![photo(new, folder, "new.jpg", 1, 1)] }).unwrap();
    let mut file = lib.photo(new).unwrap().clone();
    file.mtime = clock.now().saturating_sub(DAY * 2);
    lib.apply(Op::UpdatePhotos { photos: vec![file.clone()] }).unwrap();
    let op = lib.first_indexed_op(&[new], clock.now());
    lib.apply(op).unwrap();
    assert_eq!(recent(&lib, &clock), [ids[3]]);
    file.mtime = clock.now();
    lib.apply(Op::UpdatePhotos { photos: vec![file] }).unwrap();
    assert_eq!(recent(&lib, &clock), [ids[3]], "a file time equal to the first-indexed time is no change");
    clock.advance(DAY);
    let mut file = lib.photo(new).unwrap().clone();
    file.mtime = clock.now();
    lib.apply(Op::UpdatePhotos { photos: vec![file] }).unwrap();
    assert_eq!(recent(&lib, &clock), [new, ids[3]]);

    // With no first-indexed time (a rebuild records none) only Dalica's changes count.
    let mut rebuilt = Library::new();
    for f in lib.folders() {
        rebuilt.apply(Op::AddFolder { folder: f.clone() }).unwrap();
    }
    rebuilt.apply(Op::AddPhotos { photos: lib.photos().cloned().collect() }).unwrap();
    assert!(rebuilt.recently_updated(clock.now(), 50_000).is_empty(), "no change in 50 000 days");
    let op = rebuilt.stamped(Op::SetStars { stars: vec![(ids[0], true)] }, clock.now());
    rebuilt.apply(op).unwrap();
    assert_eq!(rebuilt.recently_updated(clock.now(), 30), [ids[0]]);
}

#[test]
fn moved_and_renamed_photos_keep_their_id_and_data() {
    let (mut lib, [a, b], ids) = library();
    let id = ids[1];
    let album = AlbumId::parse("a00ff").unwrap();
    lib.apply(Op::CreateAlbums { albums: vec![(album.clone(), AlbumInfo::named("Kept"))] }).unwrap();
    let add = lib.album_add_op(&album, &[ids[0], id, ids[2]], None).unwrap();
    lib.apply(add).unwrap();
    let info = PhotoInfo {
        caption: "Tram 28".into(),
        tags: vec!["lisbon".into(), "tram".into()],
        edits: vec!["rotate(90)".into(), "sepia()".into()],
        redo: vec!["bw()".into()],
        ..PhotoInfo::default()
    };
    lib.apply(Op::SetPhotoInfo { photos: vec![(id, info.clone())] }).unwrap();
    lib.apply(Op::SetStars { stars: vec![(id, true)] }).unwrap();
    lib.apply(Op::SetFacts { facts: vec![(id, Facts { make: "Canon".into(), ..Facts::default() })] }).unwrap();
    let saved = Saved { recipe: vec!["crop(0,0,1,1)".into()], state: SaveState::Done, ..Saved::default() };
    lib.apply(Op::SetSaved { saved: vec![(id, Some(saved.clone()))] }).unwrap();
    let order = lib.album_members(&album).to_vec();
    let key = lib.member_key(&album, id).cloned();

    // Moved to the other folder under a new name.
    let rev_b = lib.folder_revision(b);
    round_trip(&mut lib, Op::MovePhotos { moves: vec![(id, b, "Lisbon-001.jpg".into())] });
    let p = lib.photo(id).unwrap();
    assert_eq!((p.folder, p.name.as_str(), p.star), (b, "Lisbon-001.jpg", true));
    assert_eq!(lib.info(id), &info);
    assert_eq!(lib.member_key(&album, id).cloned(), key, "its album key travels with it");
    assert_eq!(lib.facts(id).make, "Canon");
    assert_eq!(lib.saved(id), Some(&saved));
    assert_eq!(lib.photo_by_name(b, "Lisbon-001.jpg").map(|p| p.id), Some(id));
    assert!(lib.photo_by_name(a, "a.jpg").is_none(), "the old name is free");
    assert!(lib.folder_revision(a) == lib.revision() && lib.folder_revision(b) > rev_b, "both folders are written");
    // Equal keys aside, the album order is by key, so a move does not reorder it.
    assert_eq!(lib.album_members(&album), &order[..]);

    // Renamed in place, and two photos swapping names in one op.
    round_trip(&mut lib, Op::MovePhotos { moves: vec![(id, b, "renamed.jpg".into())] });
    round_trip(&mut lib, Op::MovePhotos { moves: vec![(ids[0], a, "B.jpg".into()), (ids[2], a, "c.jpg".into())] });
    assert_eq!(lib.photo_by_name(a, "B.jpg").map(|p| p.id), Some(ids[0]));

    // Refusals change nothing.
    let before = (lib.model().clone(), lib.revision());
    let cases = [
        (Op::MovePhotos { moves: vec![(ids[0], a, "c.jpg".into())] }, Error::NameTaken { folder: a, name: "c.jpg".into() }),
        (
            Op::MovePhotos { moves: vec![(ids[0], b, "x.jpg".into()), (ids[2], b, "x.jpg".into())] },
            Error::NameTaken { folder: b, name: "x.jpg".into() },
        ),
        (Op::MovePhotos { moves: vec![(ids[0], b, "x".into()), (ids[0], b, "y".into())] }, Error::PhotoExists(ids[0])),
        (Op::MovePhotos { moves: vec![(PhotoId::new(999), b, "x".into())] }, Error::UnknownPhoto(PhotoId::new(999))),
        (Op::MovePhotos { moves: vec![(ids[0], FolderId::new(999), "x".into())] }, Error::UnknownFolder(FolderId::new(999))),
        (Op::MovePhotos { moves: vec![(ids[0], b, "x".repeat(MAX_FILE_NAME + 1))] }, Error::TooLong { what: "file name", max: MAX_FILE_NAME }),
    ];
    for (op, want) in cases {
        assert_eq!(lib.apply(op.clone()).unwrap_err(), want, "{op:?}");
        assert_eq!((lib.model().clone(), lib.revision()), before);
    }
    for bad in ["", ".", "..", "sub/x.jpg", "nul\0.jpg"] {
        let op = Op::MovePhotos { moves: vec![(ids[0], b, bad.into())] };
        assert!(matches!(lib.apply(op), Err(Error::BadText { what: "file name", .. })), "{bad:?}");
        assert_eq!((lib.model().clone(), lib.revision()), before);
    }
    // Names with odd characters are fine (`data-model.md` §10.7).
    round_trip(&mut lib, Op::MovePhotos { moves: vec![(ids[0], b, "a ] \" ; = ü.jpg".into())] });
}

#[test]
fn folders_move_with_their_subtree() {
    let mut lib = Library::new();
    let add = |lib: &mut Library, path: &str| {
        let id = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id, path: PathBuf::from(path) } }).unwrap();
        id
    };
    let a = add(&mut lib, "/p/a");
    let x = add(&mut lib, "/p/a/x");
    let y = add(&mut lib, "/p/a/x/y");
    let ab = add(&mut lib, "/p/ab");
    let b = add(&mut lib, "/p/b");
    let pid = lib.next_photo_id();
    lib.apply(Op::AddPhotos { photos: vec![photo(pid, y, "1.jpg", 1, 1)] }).unwrap();

    round_trip(&mut lib, Op::MoveFolder { id: a, path: PathBuf::from("/q/renamed") });
    let path = |lib: &Library, id| lib.folder(id).unwrap().path.clone();
    assert_eq!(path(&lib, a), PathBuf::from("/q/renamed"));
    assert_eq!(path(&lib, x), PathBuf::from("/q/renamed").join("x"));
    assert_eq!(path(&lib, y), PathBuf::from("/q/renamed").join("x").join("y"));
    assert_eq!(path(&lib, ab), PathBuf::from("/p/ab"), "a sibling with the same prefix stays");
    assert_eq!(lib.folder_by_path(&PathBuf::from("/q/renamed").join("x").join("y")), Some(y));
    assert_eq!(lib.folder_by_path(&PathBuf::from("/p/a/x")), None);
    assert_eq!(lib.photo(pid).unwrap().folder, y, "ids are kept");
    // Only the moved folders are written.
    assert_eq!(lib.folder_revision(b) < lib.revision(), lib.folder_revision(y) == lib.revision());

    // A move onto itself changes nothing but still applies; a subfolder alone moves too.
    round_trip(&mut lib, Op::MoveFolder { id: x, path: PathBuf::from("/q/renamed/x") });
    round_trip(&mut lib, Op::MoveFolder { id: y, path: PathBuf::from("/p/b/y") });

    let before = (lib.model().clone(), lib.revision());
    let cases = [
        (Op::MoveFolder { id: a, path: PathBuf::from("/q/renamed/x/inside") }, Error::FolderIntoItself(a)),
        (Op::MoveFolder { id: x, path: PathBuf::from("/q") }, Error::FolderIntoItself(x)),
        // `/p/b` holds a library folder, and `/p/b/y` another.
        (Op::MoveFolder { id: x, path: PathBuf::from("/p/b") }, Error::FolderPathExists(y)),
        (Op::MoveFolder { id: ab, path: PathBuf::from("/p/b/y") }, Error::FolderPathExists(y)),
        (Op::MoveFolder { id: ab, path: PathBuf::from("/p") }, Error::FolderIntoItself(ab)),
        (Op::MoveFolder { id: FolderId::new(999), path: PathBuf::from("/z") }, Error::UnknownFolder(FolderId::new(999))),
    ];
    for (op, want) in cases {
        assert_eq!(lib.apply(op.clone()).unwrap_err(), want, "{op:?}");
        assert_eq!((lib.model().clone(), lib.revision()), before);
    }
    assert!(lib.apply(Op::MoveFolder { id: a, path: PathBuf::new() }).is_err());
}

#[test]
fn folder_photo_sorts() {
    let (mut lib, [a, b], ids) = library();
    let [c, big_a, mid_b, _] = ids;
    assert_eq!(lib.folder_sort(a), PhotoSort::default());
    assert_eq!(lib.photos_sorted(a), [big_a, mid_b, c], "by date by default");
    let cases = [
        (PhotoSort::new(SortBy::Date, true), vec![c, mid_b, big_a]),
        (PhotoSort::new(SortBy::Name, false), vec![big_a, mid_b, c]),
        (PhotoSort::new(SortBy::Name, true), vec![c, mid_b, big_a]),
        (PhotoSort::new(SortBy::Size, false), vec![c, mid_b, big_a]),
        (PhotoSort::new(SortBy::Size, true), vec![big_a, mid_b, c]),
    ];
    for (sort, want) in cases {
        round_trip(&mut lib, Op::SetFolderSorts { sorts: vec![(a, sort)] });
        assert_eq!(lib.folder_sort(a), sort);
        assert_eq!(lib.photos_sorted(a), want, "{sort:?}");
        assert_eq!(lib.folder_revision(a), lib.revision(), "the sort lives in the folder's sidecar");
    }
    // The default is not stored.
    round_trip(&mut lib, Op::SetFolderSorts { sorts: vec![(a, PhotoSort::default())] });
    assert_eq!(lib.folder_sorts().count(), 0);
    assert!(lib.apply(Op::SetFolderSorts { sorts: vec![(b, PhotoSort::default()), (FolderId::new(999), PhotoSort::default())] }).is_err());
    let json = serde_json::to_value(PhotoSort::new(SortBy::Size, true)).unwrap();
    assert_eq!(json, serde_json::json!({ "by": "size", "reverse": true }));
}

#[test]
fn journal_names_of_the_m9_ops() {
    let p = PhotoId::new(1);
    let f = FolderId::new(2);
    let cases = [
        (Op::MovePhotos { moves: vec![(p, f, "x.jpg".into())] }, "movePhotos"),
        (Op::MoveFolder { id: f, path: PathBuf::from("/x") }, "moveFolder"),
        (Op::SetFolderSorts { sorts: vec![(f, PhotoSort::new(SortBy::Name, false))] }, "setFolderSorts"),
        (Op::SetFacts { facts: vec![(p, Facts { make: "Canon".into(), ..Facts::default() })] }, "setFacts"),
        (Op::SetDigests { digests: vec![(p, Some(ContentDigest::default()))] }, "setDigests"),
        (Op::SetFirstIndexed { indexed: vec![(p, Some(ms(0)))] }, "setFirstIndexed"),
        (Op::SetLastChanged { changed: vec![(p, None)] }, "setLastChanged"),
    ];
    for (op, name) in cases {
        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], name);
        assert_eq!(serde_json::from_value::<Op>(json).unwrap(), op);
    }
    // Ops on search facts, digests and times are derived; the others are not.
    assert!(Op::SetFacts { facts: vec![] }.is_derived() && Op::SetLastChanged { changed: vec![] }.is_derived());
    assert!(!Op::MovePhotos { moves: vec![] }.is_derived() && !Op::SetFolderSorts { sorts: vec![] }.is_derived());
}
