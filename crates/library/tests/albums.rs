//! `dalica-library` albums (`organizing.md` §4, `data-model.md` §3.2): album ops and their exact
//! inverses, the position keys of an add, a drop between two members, a reorder and Sort Album By,
//! key growth (10 000 drops at one spot stay under 10 bytes; repeated drops into the middle re-key
//! the album before a key passes 512 bytes, keeping the order), the album order (equal keys by
//! folder path, then file name), album dates, counts and covers (the first photo until one is set,
//! and again once the cover leaves), album ids, names and the caps.

use std::path::PathBuf;
use std::time::Instant;

use dalica_core::poskey::MAX_KEY_LEN;
use dalica_core::{AlbumId, Date, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::{
    AlbumInfo, Anchor, Error, Folder, Library, MAX_ALBUM_NAME, MAX_DESCRIPTION, Op, Photo, PhotoSort, SortBy, album_id_from, clean_album_name,
};

fn photo(id: PhotoId, folder: FolderId, name: &str, size: u64, day: u8) -> Photo {
    Photo {
        id,
        folder,
        name: name.into(),
        size,
        mtime: Timestamp::from_millis(1_700_000_000_000),
        width: 4,
        height: 3,
        date: format!("2026-06-{day:02}T12:00:00").parse().ok(),
        star: false,
    }
}

/// A library with folders `/p/a` and `/p/b` and `n` photos spread over them (`IMG_<i>.jpg`, day
/// and size counting down so the sorts differ).
fn library(n: usize) -> (Library, [FolderId; 2], Vec<PhotoId>) {
    let mut lib = Library::new();
    let folders = [lib.next_folder_id(), lib.next_folder_id()];
    for (id, path) in folders.iter().zip(["/p/a", "/p/b"]) {
        lib.apply(Op::AddFolder { folder: Folder { id: *id, path: PathBuf::from(path) } }).unwrap();
    }
    let ids: Vec<PhotoId> = (0..n).map(|_| lib.next_photo_id()).collect();
    let photos = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| photo(id, folders[i % 2], &format!("IMG_{i:05}.jpg"), 10_000 - i as u64, 28 - (i % 28) as u8))
        .collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    (lib, folders, ids)
}

fn aid(s: &str) -> AlbumId {
    AlbumId::parse(s).unwrap()
}

fn create(lib: &mut Library, id: &AlbumId, name: &str) {
    lib.apply(Op::CreateAlbums { albums: vec![(id.clone(), AlbumInfo::named(name))] }).unwrap();
}

/// Applies `op`, checks one revision and an exact inverse, applies it again; returns the inverse.
fn round_trip(lib: &mut Library, op: Op) -> Op {
    let before = lib.model().clone();
    let rev = lib.revision();
    let inverse = lib.apply(op.clone()).unwrap();
    assert_eq!(lib.revision(), rev + 1, "{op:?}");
    let after = lib.model().clone();
    lib.apply(inverse.clone()).unwrap();
    assert_eq!(lib.model(), &before, "{op:?} undone");
    lib.apply(op.clone()).unwrap();
    assert_eq!(lib.model(), &after, "{op:?} redone");
    inverse
}

fn keys(lib: &Library, album: &AlbumId) -> Vec<String> {
    lib.album_members(album).iter().map(|id| lib.member_key(album, *id).unwrap().to_string()).collect()
}

#[test]
fn create_add_reorder_remove_delete_and_undo() {
    let (mut lib, _, ids) = library(6);
    let a = aid("a91c0e");
    round_trip(&mut lib, Op::CreateAlbums { albums: vec![(a.clone(), AlbumInfo::named("Trams"))] });
    assert_eq!(lib.album(&a).unwrap().name, "Trams");
    assert!(lib.album_members(&a).is_empty() && lib.album_cover(&a).is_none() && lib.album_count(&a) == 0);

    // Added in the order given, after the last key.
    let add = lib.album_add_op(&a, &[ids[3], ids[1], ids[4]], None).unwrap();
    round_trip(&mut lib, add);
    assert_eq!(lib.album_members(&a), [ids[3], ids[1], ids[4]]);
    // Adding members again and repeats changes nothing; new ones go where they are dropped.
    let again = lib.album_add_op(&a, &[ids[1], ids[1]], None).unwrap();
    assert_eq!(again, Op::AddMembers { members: vec![] });
    let drop = lib.album_add_op(&a, &[ids[0], ids[0], ids[3], ids[5]], Some(Anchor::Before(ids[1]))).unwrap();
    round_trip(&mut lib, drop);
    assert_eq!(lib.album_members(&a), [ids[3], ids[0], ids[5], ids[1], ids[4]]);
    assert_eq!(lib.albums_of(ids[0]).len(), 1);

    // A reorder gives new keys to the moved photos only.
    let before: Vec<(PhotoId, PosKey)> = lib.album_members(&a).iter().map(|id| (*id, lib.member_key(&a, *id).unwrap().clone())).collect();
    let reorder = lib.album_reorder_op(&a, &[ids[4], ids[3]], Some(Anchor::After(ids[0]))).unwrap();
    let Op::SetMemberKeys { keys: moved } = &reorder else { panic!("{reorder:?}") };
    assert_eq!(moved.iter().map(|(_, id, _)| *id).collect::<Vec<_>>(), [ids[4], ids[3]]);
    round_trip(&mut lib, reorder);
    assert_eq!(lib.album_members(&a), [ids[0], ids[4], ids[3], ids[5], ids[1]]);
    for (id, key) in &before {
        if ![ids[4], ids[3]].contains(id) {
            assert_eq!(lib.member_key(&a, *id), Some(key), "an unmoved photo keeps its key");
        }
    }
    // To the start, and to the end.
    let op = lib.album_reorder_op(&a, &[ids[1]], Some(Anchor::Before(ids[0]))).unwrap();
    round_trip(&mut lib, op);
    let op = lib.album_reorder_op(&a, &[ids[0]], None).unwrap();
    round_trip(&mut lib, op);
    assert_eq!(lib.album_members(&a), [ids[1], ids[4], ids[3], ids[5], ids[0]]);

    // The cover: the first photo, then the one set, then the first again once it leaves.
    assert_eq!(lib.album_cover(&a), Some(ids[1]));
    round_trip(&mut lib, Op::SetCovers { covers: vec![(a.clone(), Some(ids[3]))] });
    assert_eq!(lib.album_cover(&a), Some(ids[3]));
    let removed = round_trip(&mut lib, Op::RemoveMembers { members: vec![(a.clone(), ids[3])] });
    assert_eq!(lib.album_cover(&a), Some(ids[1]), "the cover left: the first photo again");
    assert_eq!(lib.cover_set(&a), None);
    lib.apply(removed).unwrap();
    assert_eq!(lib.album_cover(&a), Some(ids[3]), "undo brings the cover back");

    // Deleting the album keeps the photos; undo brings back every membership and the cover.
    let model = lib.model().clone();
    let undo = lib.apply(Op::DeleteAlbums { ids: vec![a.clone()] }).unwrap();
    assert!(lib.album(&a).is_none() && lib.albums_of(ids[3]).is_empty());
    assert_eq!(lib.photos().count(), 6);
    lib.apply(undo).unwrap();
    assert_eq!(lib.model(), &model);
}

#[test]
fn refusals_change_nothing() {
    let (mut lib, _, ids) = library(3);
    let a = aid("a0001");
    create(&mut lib, &a, "A");
    let op = lib.album_add_op(&a, &ids[..1], None).unwrap();
    lib.apply(op).unwrap();
    let unknown = aid("a0bad");
    let key = PosKey::first();
    let before = (lib.model().clone(), lib.revision());
    let cases: Vec<(Op, Error)> = vec![
        (Op::CreateAlbums { albums: vec![(a.clone(), AlbumInfo::named("again"))] }, Error::AlbumExists(a.clone())),
        (
            Op::CreateAlbums { albums: vec![(unknown.clone(), AlbumInfo::named("x")), (unknown.clone(), AlbumInfo::named("y"))] },
            Error::AlbumExists(unknown.clone()),
        ),
        (Op::CreateAlbums { albums: vec![(unknown.clone(), AlbumInfo::named(""))] }, Error::BadText { what: "album name", why: "" }),
        (Op::CreateAlbums { albums: vec![(unknown.clone(), AlbumInfo::named(" padded "))] }, Error::BadText { what: "album name", why: "" }),
        (Op::CreateAlbums { albums: vec![(unknown.clone(), AlbumInfo::named("two\nlines"))] }, Error::BadText { what: "album name", why: "" }),
        (
            Op::CreateAlbums { albums: vec![(unknown.clone(), AlbumInfo::named("x".repeat(MAX_ALBUM_NAME + 1)))] },
            Error::TooLong { what: "album name", max: MAX_ALBUM_NAME },
        ),
        (
            Op::SetAlbumInfo { albums: vec![(a.clone(), AlbumInfo { description: "x".repeat(MAX_DESCRIPTION + 1), ..AlbumInfo::named("A") })] },
            Error::TooLong { what: "album description", max: MAX_DESCRIPTION },
        ),
        (Op::SetAlbumInfo { albums: vec![(unknown.clone(), AlbumInfo::named("x"))] }, Error::UnknownAlbum(unknown.clone())),
        (Op::DeleteAlbums { ids: vec![a.clone(), a.clone()] }, Error::UnknownAlbum(a.clone())),
        (Op::AddMembers { members: vec![(a.clone(), ids[0], key.clone())] }, Error::AlreadyMember { album: a.clone(), photo: ids[0] }),
        (
            Op::AddMembers { members: vec![(a.clone(), ids[1], key.clone()), (a.clone(), ids[1], key.clone())] },
            Error::AlreadyMember { album: a.clone(), photo: ids[1] },
        ),
        (Op::AddMembers { members: vec![(unknown.clone(), ids[1], key.clone())] }, Error::UnknownAlbum(unknown.clone())),
        (Op::AddMembers { members: vec![(a.clone(), PhotoId::new(999), key.clone())] }, Error::UnknownPhoto(PhotoId::new(999))),
        (Op::RemoveMembers { members: vec![(a.clone(), ids[1])] }, Error::NotMember { album: a.clone(), photo: ids[1] }),
        (Op::SetMemberKeys { keys: vec![(a.clone(), ids[2], key.clone())] }, Error::NotMember { album: a.clone(), photo: ids[2] }),
        (Op::SetCovers { covers: vec![(a.clone(), Some(ids[2]))] }, Error::NotMember { album: a.clone(), photo: ids[2] }),
        (Op::SetCovers { covers: vec![(unknown.clone(), None)] }, Error::UnknownAlbum(unknown.clone())),
    ];
    for (op, want) in cases {
        let got = lib.apply(op.clone()).unwrap_err();
        // Text errors are compared by kind: the reason is the library's to word.
        match (&got, &want) {
            (Error::BadText { what: g, .. }, Error::BadText { what: w, .. }) => assert_eq!(g, w, "{op:?}"),
            _ => assert_eq!(got, want, "{op:?}"),
        }
        assert_eq!((lib.model().clone(), lib.revision()), before, "{op:?}");
    }
    // The op builders refuse the same.
    assert_eq!(lib.album_add_op(&unknown, &ids, None), Err(Error::UnknownAlbum(unknown.clone())));
    assert_eq!(lib.album_add_op(&a, &[PhotoId::new(999)], None), Err(Error::UnknownPhoto(PhotoId::new(999))));
    assert_eq!(lib.album_add_op(&a, &ids[1..2], Some(Anchor::After(ids[2]))), Err(Error::NotMember { album: a.clone(), photo: ids[2] }));
    assert_eq!(lib.album_reorder_op(&a, &ids[1..2], None), Err(Error::NotMember { album: a.clone(), photo: ids[1] }));
    assert_eq!(lib.album_reorder_op(&a, &ids[..1], Some(Anchor::Before(ids[0]))), Err(Error::NotMember { album: a.clone(), photo: ids[0] }));
    assert_eq!(lib.album_sort_op(&unknown, PhotoSort::default()), Err(Error::UnknownAlbum(unknown.clone())));
    assert_eq!(lib.album_rekey_op(&unknown), Err(Error::UnknownAlbum(unknown)));
}

/// Inserting at one spot 10 000 times (appending, prepending, dropping right after the last drop)
/// keeps every key under 10 bytes (`data-model.md` §3.2).
#[test]
fn ten_thousand_drops_at_one_spot_keep_keys_short() {
    const N: usize = 10_000;
    let (mut lib, _, ids) = library(N);
    let start = Instant::now();
    for (album, pattern) in [(aid("a0001"), "append"), (aid("a0002"), "prepend"), (aid("a0003"), "after the last drop")] {
        create(&mut lib, &album, pattern);
        // Two members to drop between or beside.
        let op = lib.album_add_op(&album, &ids[..2], None).unwrap();
        lib.apply(op).unwrap();
        let mut last = ids[0];
        for &id in &ids[2..N] {
            let at = match pattern {
                "append" => None,
                // The first member is the last one dropped (`ids[0]` at the start).
                "prepend" => Some(Anchor::Before(last)),
                _ => Some(Anchor::After(last)),
            };
            let op = lib.album_add_op(&album, &[id], at).unwrap();
            assert!(matches!(op, Op::AddMembers { .. }), "{pattern}: no re-key needed");
            lib.apply(op).unwrap();
            last = id;
        }
        let longest = keys(&lib, &album).iter().map(String::len).max().unwrap();
        println!("{pattern}: {N} inserts, longest key {longest} bytes");
        assert!(longest < 10, "{pattern}: {longest} bytes");
        assert_eq!(lib.album_count(&album), N);
        let members = lib.album_members(&album);
        match pattern {
            "append" => assert_eq!(members, &ids[..N]),
            "prepend" => {
                assert_eq!(members[0], ids[N - 1]);
                assert_eq!(&members[N - 2..], &ids[..2]);
            }
            _ => {
                assert_eq!(members[0], ids[0]);
                assert_eq!(&members[1..N - 1], &ids[2..N]);
                assert_eq!(members[N - 1], ids[1]);
            }
        }
    }
    println!("30 000 inserts in {:?}", start.elapsed());
}

/// Dropping into the middle of the newest pair over and over grows keys steadily; before one would
/// pass 512 bytes the album is re-keyed in the same op, and the order is the one the drops made.
#[test]
fn repeated_middle_drops_rekey_before_512_bytes() {
    const N: usize = 4_000;
    let (mut lib, _, ids) = library(N);
    let album = aid("a00aa");
    create(&mut lib, &album, "Middle");
    let op = lib.album_add_op(&album, &ids[..2], None).unwrap();
    lib.apply(op).unwrap();
    // The expected order, kept by hand: each photo goes between the last two dropped.
    let mut order: Vec<PhotoId> = ids[..2].to_vec();
    // Each drop goes right after `lo`, so between `lo` and the photo dropped there last.
    let mut lo = ids[0];
    let mut rekeyed = 0;
    let mut longest = 0;
    for (i, &id) in ids[2..N].iter().enumerate() {
        let op = lib.album_add_op(&album, &[id], Some(Anchor::After(lo))).unwrap();
        let rekey = matches!(op, Op::Batch { .. });
        let before = rekey.then(|| lib.model().clone());
        let at = order.iter().position(|&x| x == lo).unwrap() + 1;
        order.insert(at, id);
        let undo = lib.apply(op.clone()).unwrap();
        // Only the dropped photo's key is new (or, after a re-key, every key).
        let new = match &op {
            Op::AddMembers { members } => members.iter().map(|(_, _, k)| k.as_str().len()).max().unwrap(),
            _ => keys(&lib, &album).iter().map(String::len).max().unwrap(),
        };
        longest = longest.max(new);
        assert!(longest <= MAX_KEY_LEN);
        if let Some(before) = before {
            rekeyed += 1;
            assert_eq!(lib.album_members(&album), &order[..], "a re-key keeps the order");
            // The re-key is one step with the drop: undone, every key is as it was.
            let after = lib.model().clone();
            lib.apply(undo).unwrap();
            assert_eq!(lib.model(), &before);
            lib.apply(op).unwrap();
            assert_eq!(lib.model(), &after);
        }
        // Every other drop moves `lo` up, so the newest pair shrinks from both ends.
        if i % 2 == 1 {
            lo = id;
        }
    }
    println!("{N} middle drops: re-keyed {rekeyed} times, longest key {longest} bytes");
    assert!(rekeyed >= 1, "the keys never needed a re-key");
    assert_eq!(lib.album_members(&album), &order[..], "the order the drops made");
    assert!(keys(&lib, &album).iter().all(|k| k.len() <= MAX_KEY_LEN));
}

/// Equal keys (after a merge) sort by folder path, then file name; a drop between two equal keys
/// re-keys the album.
#[test]
fn equal_keys_sort_by_path_then_name() {
    let mut lib = Library::new();
    let (b, a) = (lib.next_folder_id(), lib.next_folder_id());
    lib.apply(Op::AddFolder { folder: Folder { id: b, path: PathBuf::from("/p/b") } }).unwrap();
    lib.apply(Op::AddFolder { folder: Folder { id: a, path: PathBuf::from("/p/a") } }).unwrap();
    let ids: Vec<PhotoId> = (0..4).map(|_| lib.next_photo_id()).collect();
    lib.apply(Op::AddPhotos {
        photos: vec![
            photo(ids[0], b, "1.jpg", 1, 1),
            photo(ids[1], a, "2.jpg", 1, 1),
            photo(ids[2], a, "1.jpg", 1, 1),
            photo(ids[3], b, "0.jpg", 1, 1),
        ],
    })
    .unwrap();
    let album = aid("a1234");
    create(&mut lib, &album, "Merged");
    let same = PosKey::parse("a5").unwrap();
    let low = PosKey::parse("a1").unwrap();
    lib.apply(Op::AddMembers {
        members: vec![
            (album.clone(), ids[0], same.clone()),
            (album.clone(), ids[1], same.clone()),
            (album.clone(), ids[2], same.clone()),
            (album.clone(), ids[3], low),
        ],
    })
    .unwrap();
    // a1 first; then a5 in /p/a (1.jpg, 2.jpg), then /p/b.
    assert_eq!(lib.album_members(&album), [ids[3], ids[2], ids[1], ids[0]]);
    // Moving the first between two equal keys: no key fits, so the album is re-keyed in order.
    let op = lib.album_reorder_op(&album, &[ids[3]], Some(Anchor::After(ids[2]))).unwrap();
    round_trip(&mut lib, op);
    assert_eq!(lib.album_members(&album), [ids[2], ids[3], ids[1], ids[0]]);
    let k = keys(&lib, &album);
    assert!(k.windows(2).all(|w| w[0] < w[1]), "fresh keys are distinct: {k:?}");
}

#[test]
fn sort_album_by_rewrites_every_key() {
    let (mut lib, _, ids) = library(5);
    let album = aid("a5050");
    create(&mut lib, &album, "Sorted");
    let op = lib.album_add_op(&album, &[ids[2], ids[0], ids[4], ids[1], ids[3]], None).unwrap();
    lib.apply(op).unwrap();
    // Day and size count down with the index; names count up.
    let cases = [
        (PhotoSort::new(SortBy::Name, false), vec![ids[0], ids[1], ids[2], ids[3], ids[4]]),
        (PhotoSort::new(SortBy::Name, true), vec![ids[4], ids[3], ids[2], ids[1], ids[0]]),
        (PhotoSort::new(SortBy::Date, false), vec![ids[4], ids[3], ids[2], ids[1], ids[0]]),
        (PhotoSort::new(SortBy::Size, false), vec![ids[4], ids[3], ids[2], ids[1], ids[0]]),
        (PhotoSort::new(SortBy::Size, true), vec![ids[0], ids[1], ids[2], ids[3], ids[4]]),
    ];
    for (sort, want) in cases {
        let op = lib.album_sort_op(&album, sort).unwrap();
        let Op::SetMemberKeys { keys: new } = &op else { panic!("{op:?}") };
        assert_eq!(new.len(), 5, "every key rewritten");
        round_trip(&mut lib, op);
        assert_eq!(lib.album_members(&album), &want[..], "{sort:?}");
    }
    // Re-keying keeps the order.
    let order = lib.album_members(&album).to_vec();
    let op = lib.album_rekey_op(&album).unwrap();
    round_trip(&mut lib, op);
    assert_eq!(lib.album_members(&album), &order[..]);
    assert_eq!(keys(&lib, &album)[0], PosKey::first().to_string());
}

#[test]
fn album_dates_counts_and_names() {
    let (mut lib, _, ids) = library(4);
    let album = aid("a7777");
    create(&mut lib, &album, "Dated");
    assert_eq!(lib.album_date(&album), None, "empty and no date");
    let op = lib.album_add_op(&album, &ids[1..3], None).unwrap();
    lib.apply(op).unwrap();
    // The newest photo's date: index 1 is day 27.
    assert_eq!(lib.album_date(&album), "2026-06-27T12:00:00".parse().ok());
    assert_eq!(lib.album_count(&album), 2);
    let own = AlbumInfo { date: Some(Date::new(2020, 1, 2).unwrap()), use_newest: false, ..AlbumInfo::named("Dated") };
    round_trip(&mut lib, Op::SetAlbumInfo { albums: vec![(album.clone(), own.clone())] });
    assert_eq!(lib.album_date(&album), "2020-01-02T00:00:00".parse().ok(), "its own date");
    round_trip(&mut lib, Op::SetAlbumInfo { albums: vec![(album.clone(), AlbumInfo { use_newest: true, ..own })] });
    assert_eq!(lib.album_date(&album), "2026-06-27T12:00:00".parse().ok(), "use the newest photo's");
    // A user date on a member moves the album date.
    lib.apply(Op::SetDates { dates: vec![(ids[2], "2027-01-01T00:00:00".parse().ok())] }).unwrap();
    assert_eq!(lib.album_date(&album), "2027-01-01T00:00:00".parse().ok());

    assert_eq!(clean_album_name("  Best\tof\n2026  ").as_deref(), Some("Best of 2026"));
    assert_eq!(clean_album_name(" \n "), None);
    assert_eq!(clean_album_name(&"é".repeat(MAX_ALBUM_NAME)).map(|n| n.len()), Some(MAX_ALBUM_NAME));
}

#[test]
fn album_ids_are_drawn_again_on_a_clash() {
    let (mut lib, ..) = library(1);
    assert_eq!(album_id_from(0x1234_5678_9abc_def0).unwrap().as_str(), "a56789abcdef0");
    assert_eq!(album_id_from(0).unwrap().as_str(), "a000000000000");
    let first = lib.new_album_id(|| 7).unwrap();
    assert_eq!(first.as_str(), "a000000000007");
    create(&mut lib, &first, "Seven");
    let mut draws = [7u64, 7, 9].into_iter();
    assert_eq!(lib.new_album_id(|| draws.next().unwrap_or(0)).unwrap().as_str(), "a000000000009");
    assert!(lib.new_album_id(|| 7).is_err(), "a broken random source gives up");
}

#[test]
fn journal_names_of_the_album_ops() {
    let a = aid("a91c0e");
    let p = PhotoId::new(3);
    let k = PosKey::first();
    let cases = [
        (Op::CreateAlbums { albums: vec![(a.clone(), AlbumInfo::named("x"))] }, "createAlbums"),
        (Op::DeleteAlbums { ids: vec![a.clone()] }, "deleteAlbums"),
        (Op::SetAlbumInfo { albums: vec![] }, "setAlbumInfo"),
        (Op::AddMembers { members: vec![(a.clone(), p, k.clone())] }, "addMembers"),
        (Op::RemoveMembers { members: vec![(a.clone(), p)] }, "removeMembers"),
        (Op::SetMemberKeys { keys: vec![(a.clone(), p, k)] }, "setMemberKeys"),
        (Op::SetCovers { covers: vec![(a, Some(p))] }, "setCovers"),
    ];
    for (op, name) in cases {
        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(json["op"], name);
        assert_eq!(serde_json::from_value::<Op>(json).unwrap(), op);
    }
    // A hostile key or album id in a journal record is refused, never used.
    for bad in [
        r#"{"op":"addMembers","members":[["a91c0e",3,"not a key!"]]}"#,
        r#"{"op":"addMembers","members":[["../x",3,"a0"]]}"#,
        r#"{"op":"setCovers","covers":[["A91C0E",3]]}"#,
    ] {
        assert!(serde_json::from_str::<Op>(bad).is_err(), "{bad}");
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

    /// Adds and reorders with random anchors, over an album that starts with equal keys (a merge),
    /// put the photos exactly where they were dropped and keep every other photo in its place;
    /// undo restores the album.
    #[test]
    fn drops_land_where_they_are_dropped(
        equal in proptest::collection::vec((0u8..12, 0u8..3), 0..8),
        steps in proptest::collection::vec(
            (proptest::prelude::any::<bool>(), proptest::collection::vec(0u8..12, 1..4), proptest::option::of((proptest::prelude::any::<bool>(), proptest::prelude::any::<u8>()))),
            1..40,
        ),
    ) {
        let (mut lib, _, ids) = library(12);
        let album = aid("a0e0e");
        create(&mut lib, &album, "Drops");
        // Members with only three distinct keys, so equal keys sort by folder and name.
        let mut seen = std::collections::HashSet::new();
        let members = equal
            .iter()
            .filter(|(i, _)| seen.insert(*i))
            .map(|(i, k)| (album.clone(), ids[usize::from(*i)], PosKey::parse(["a1", "a2", "a3"][usize::from(*k)]).unwrap()))
            .collect();
        lib.apply(Op::AddMembers { members }).unwrap();
        for (add, picks, at) in steps {
            let before = lib.album_members(&album).to_vec();
            let mut moving: Vec<PhotoId> = Vec::new();
            for p in picks {
                let id = if add { ids[usize::from(p)] } else if before.is_empty() { break } else { before[usize::from(p) % before.len()] };
                if !moving.contains(&id) && (!add || !before.contains(&id)) {
                    moving.push(id);
                }
            }
            let anchor = at.filter(|_| !before.is_empty()).map(|(b, i)| {
                let x = before[usize::from(i) % before.len()];
                if b { Anchor::Before(x) } else { Anchor::After(x) }
            });
            let anchored = match anchor {
                Some(Anchor::Before(x) | Anchor::After(x)) => Some(x),
                None => None,
            };
            let result = if add { lib.album_add_op(&album, &moving, anchor) } else { lib.album_reorder_op(&album, &moving, anchor) };
            if anchored.is_some_and(|x| moving.contains(&x)) {
                proptest::prop_assert!(result.is_err() || moving.is_empty());
                continue;
            }
            let op = result.unwrap();
            let mut expected: Vec<PhotoId> = before.iter().copied().filter(|id| !moving.contains(id)).collect();
            let at = match anchor {
                None => expected.len(),
                Some(Anchor::Before(x)) => expected.iter().position(|&id| id == x).unwrap(),
                Some(Anchor::After(x)) => expected.iter().position(|&id| id == x).unwrap() + 1,
            };
            let tail = expected.split_off(at);
            expected.extend(&moving);
            expected.extend(tail);
            let model = lib.model().clone();
            let undo = lib.apply(op).unwrap();
            proptest::prop_assert_eq!(lib.album_members(&album), &expected[..]);
            lib.apply(undo).unwrap();
            proptest::prop_assert_eq!(lib.model(), &model);
            proptest::prop_assert_eq!(lib.album_members(&album), &before[..]);
            lib.apply(lib.album_rekey_op(&album).unwrap()).unwrap();
            let rekeyed = lib.album_members(&album).to_vec();
            proptest::prop_assert_eq!(&rekeyed, &before);
            // The re-keyed album takes the same drop the same way; on to the next step from there.
            let op = if add { lib.album_add_op(&album, &moving, anchor) } else { lib.album_reorder_op(&album, &moving, anchor) }.unwrap();
            lib.apply(op).unwrap();
            proptest::prop_assert_eq!(lib.album_members(&album), &expected[..]);
        }
    }
}
