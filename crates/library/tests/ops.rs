//! `dalica-library` ops (`architecture.md` §11): for random op sequences, applying an op and then
//! its inverse restores the model (and the tag index), each apply bumps the revision once (derived
//! ops: never), and a refused op changes nothing. Every op is in the mix, M6's and M8's included
//! (saved records, copies saved beside, the Exported mark, and the batches of a Save, an Undo Save,
//! a Revert and a Save beside), and the cached unsaved counts always equal fresh ones. M9's ops join
//! the mix: albums (created, deleted, info, members added, removed and re-keyed, covers, and the
//! ops of an add, a drop, a reorder and Sort Album By), photos moved and renamed, folders moved
//! with their subtrees, folder photo sorts, search facts, digests, first-indexed and last-changed
//! times, and stamped ops; the cached album order, Starred Photos and Recently Updated always equal
//! fresh ones.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use dalica_core::{FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::{Anchor, Folder, Library, Op, Photo, Synced};
use proptest::prelude::*;

use crate::seed::*;

/// The tag index as the photos' tags say it should be: (tag, count) in folded order, the
/// spelling most photos use (ties: the first in byte order).
fn expected_counts(lib: &Library) -> Vec<(String, usize)> {
    let mut by_fold: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (_, info) in lib.infos() {
        for t in &info.tags {
            *by_fold.entry(t.to_lowercase()).or_default().entry(t.clone()).or_default() += 1;
        }
    }
    by_fold
        .into_values()
        .map(|spellings| {
            let total = spellings.values().sum();
            let max = spellings.values().copied().max().unwrap_or(0);
            let tag = spellings.into_iter().find(|(_, n)| *n == max).map(|(t, _)| t).unwrap_or_default();
            (tag, total)
        })
        .collect()
}

/// Everything the model promises beyond equality: indexes agree with it, no info for anything
/// gone, no empty info, no photo with one tag twice.
fn check_invariants(lib: &Library) -> Result<(), TestCaseError> {
    for p in lib.photos() {
        prop_assert_eq!(lib.photo_by_name(p.folder, &p.name).map(|q| q.id), Some(p.id));
    }
    for f in lib.folders() {
        prop_assert_eq!(lib.folder_by_path(&f.path), Some(f.id));
    }
    for (id, info) in lib.infos() {
        prop_assert!(lib.photo(id).is_some() && !info.is_empty());
        let mut folded: Vec<String> = info.tags.iter().map(|t| t.to_lowercase()).collect();
        folded.sort();
        folded.dedup();
        prop_assert_eq!(folded.len(), info.tags.len());
    }
    for (id, info) in lib.folder_infos() {
        prop_assert!(lib.folder(id).is_some() && !info.is_empty());
    }
    for (id, s) in lib.synced_all() {
        prop_assert!(lib.photo(id).is_some() && *s != Synced::default());
    }
    for (id, _) in lib.saved_all() {
        prop_assert!(lib.photo(id).is_some());
    }
    for (id, _) in lib.saved_as_all() {
        prop_assert!(lib.photo(id).is_some());
    }
    for (id, _) in lib.projects() {
        prop_assert!(lib.folder(id).is_some());
    }
    // M9: memberships only of albums and photos there are, never an empty album map, covers in
    // their albums, index-only data only for photos there are, no default sort stored.
    for (album, members) in lib.memberships() {
        prop_assert!(lib.album(album).is_some() && !members.is_empty());
        let mut fresh: Vec<(PosKey, PathBuf, String, PhotoId)> = Vec::new();
        for (id, key) in members {
            let p = lib.photo(*id);
            prop_assert!(p.is_some());
            let p = p.unwrap();
            fresh.push((key.clone(), lib.folder(p.folder).unwrap().path.clone(), p.name.clone(), *id));
        }
        fresh.sort();
        prop_assert_eq!(lib.album_members(album), &fresh.into_iter().map(|(.., id)| id).collect::<Vec<_>>()[..]);
        prop_assert_eq!(lib.album_count(album), members.len());
    }
    // Drops find their neighbours through an index kept beside the memberships: an add at the
    // end lands last and a move before the first lands first, whatever ops came before.
    for (album, _) in lib.albums() {
        let members = lib.album_members(album).to_vec();
        if let Some(outsider) = lib.photos().map(|p| p.id).find(|id| !members.contains(id)) {
            let mut copy = lib.clone();
            copy.apply(copy.album_add_op(album, &[outsider], None).unwrap()).unwrap();
            prop_assert_eq!(copy.album_members(album).last(), Some(&outsider));
        }
        if let [first, .., last] = members[..] {
            let mut copy = lib.clone();
            copy.apply(copy.album_reorder_op(album, &[last], Some(Anchor::Before(first))).unwrap()).unwrap();
            prop_assert_eq!(copy.album_members(album).first(), Some(&last));
        }
    }
    for (album, _) in lib.albums() {
        if let Some(cover) = lib.cover_set(album) {
            prop_assert!(lib.member_key(album, cover).is_some());
        }
        prop_assert_eq!(lib.album_cover(album), lib.cover_set(album).or_else(|| lib.album_members(album).first().copied()));
    }
    for (id, f) in lib.facts_all() {
        prop_assert!(lib.photo(id).is_some() && !f.is_empty());
    }
    for (id, _) in lib.digests() {
        prop_assert!(lib.photo(id).is_some());
    }
    for (id, _) in lib.first_indexed_all().chain(lib.last_changed_all()) {
        prop_assert!(lib.photo(id).is_some());
    }
    for (id, sort) in lib.folder_sorts() {
        prop_assert!(lib.folder(id).is_some() && !sort.is_default());
    }
    // The cached automatic albums are the ones worked out afresh.
    let mut starred: Vec<&Photo> = lib.photos().filter(|p| p.star).collect();
    starred.sort_by_key(|p| (lib.sort_date(p), p.name.clone(), p.id));
    prop_assert_eq!(lib.starred_photos(), &starred.iter().map(|p| p.id).collect::<Vec<_>>()[..]);
    let now = time(40);
    let since = Timestamp::from_millis(now.as_millis() - 30 * 86_400_000);
    let mut recent: Vec<&Photo> = lib.photos().filter(|p| lib.updated_time(p) >= since).collect();
    recent.sort_by_key(|p| (std::cmp::Reverse(lib.updated_time(p)), p.name.clone(), p.id));
    prop_assert_eq!(lib.recently_updated(now, 30), recent.iter().map(|p| p.id).collect::<Vec<_>>());
    // The cached unsaved counts are the ones counted afresh.
    let mut fresh: HashMap<FolderId, usize> = HashMap::new();
    for p in lib.photos().filter(|p| lib.has_unsaved_edits(p.id)) {
        *fresh.entry(p.folder).or_default() += 1;
    }
    prop_assert_eq!(lib.unsaved_counts(), &fresh);
    let counts: Vec<(String, usize)> = lib.tag_index().counts().into_iter().map(|c| (c.tag, c.count)).collect();
    prop_assert_eq!(counts, expected_counts(lib));
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases(512, 32))]

    #[test]
    fn apply_then_inverse_restores_the_model(seeds in proptest::collection::vec(seed(), 1..40)) {
        let mut lib = Library::new();
        for s in &seeds {
            let op = build(&mut lib, s);
            let before = lib.model().clone();
            let index = lib.tag_index().clone();
            let rev = lib.revision();
            let step = u64::from(!op.is_derived());
            match lib.apply(op.clone()) {
                Ok(inverse) => {
                    prop_assert_eq!(lib.revision(), rev + step);
                    prop_assert_eq!(inverse.is_derived(), op.is_derived());
                    // The op and its inverse survive the journal's JSON.
                    for o in [&op, &inverse] {
                        let json = serde_json::to_string(o).unwrap();
                        prop_assert_eq!(&serde_json::from_str::<Op>(&json).unwrap(), o);
                    }
                    check_invariants(&lib)?;
                    lib.apply(inverse).unwrap();
                    prop_assert_eq!(lib.revision(), rev + 2 * step);
                    prop_assert_eq!(lib.model(), &before);
                    prop_assert_eq!(lib.tag_index(), &index);
                    // Go forward again so the library keeps growing.
                    lib.apply(op).unwrap();
                }
                Err(_) => {
                    prop_assert_eq!(lib.revision(), rev);
                    prop_assert_eq!(lib.model(), &before);
                    prop_assert_eq!(lib.tag_index(), &index);
                }
            }
            check_invariants(&lib)?;
        }
    }
}

fn library_with(n: usize) -> (Library, FolderId, Vec<PhotoId>) {
    let mut lib = Library::new();
    let folder = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: folder, path: PathBuf::from("/photos/a") } }).unwrap();
    let ids: Vec<PhotoId> = (0..n).map(|_| lib.next_photo_id()).collect();
    let photos = ids.iter().zip(0u8..).map(|(&id, i)| photo(id, folder, i, 100)).collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    (lib, folder, ids)
}

#[test]
fn star_three_undo_one() {
    let (mut lib, _, ids) = library_with(5);
    lib.apply(Op::SetStars { stars: ids.iter().map(|&id| (id, false)).collect() }).unwrap();
    let mut undo = Vec::new();
    for &id in &ids[..3] {
        undo.push(lib.apply(Op::SetStars { stars: vec![(id, true)] }).unwrap());
    }
    lib.apply(undo.pop().unwrap()).unwrap();
    assert_eq!(lib.starred().iter().map(|p| p.id).collect::<Vec<_>>(), &ids[..2]);
}

#[test]
fn refused_ops_change_nothing() {
    let (mut lib, folder, ids) = library_with(3);
    let before = (lib.model().clone(), lib.revision());
    let dup_name = photo(PhotoId::new(100), folder, 0, 1);
    let cases = [
        Op::AddFolder { folder: Folder { id: folder, path: PathBuf::from("/other") } },
        Op::AddFolder { folder: Folder { id: FolderId::new(50), path: PathBuf::from("/photos/a") } },
        Op::RemoveFolder { id: FolderId::new(77) },
        Op::AddPhotos { photos: vec![dup_name] },
        Op::AddPhotos { photos: vec![photo(ids[0], folder, 9, 1)] },
        Op::AddPhotos { photos: vec![photo(PhotoId::new(101), FolderId::new(77), 9, 1)] },
        Op::RemovePhotos { ids: vec![ids[0], ids[0]] },
        Op::UpdatePhotos { photos: vec![Photo { folder: FolderId::new(77), ..lib.photo(ids[0]).unwrap().clone() }] },
        Op::UpdatePhotos { photos: vec![Photo { name: lib.photo(ids[1]).unwrap().name.clone(), ..lib.photo(ids[0]).unwrap().clone() }] },
        Op::SetStars { stars: vec![(ids[0], true), (PhotoId::new(9999), true)] },
        // A batch whose second op fails undoes its first.
        Op::Batch { ops: vec![Op::SetStars { stars: vec![(ids[0], true)] }, Op::RemoveFolder { id: FolderId::new(77) }] },
    ];
    for op in cases {
        assert!(lib.apply(op.clone()).is_err(), "{op:?}");
        assert_eq!((lib.model().clone(), lib.revision()), before, "{op:?}");
    }
    // Swapping two names in one update is fine.
    let (a, b) = (lib.photo(ids[0]).unwrap().clone(), lib.photo(ids[1]).unwrap().clone());
    lib.apply(Op::UpdatePhotos { photos: vec![Photo { name: b.name.clone(), ..a.clone() }, Photo { name: a.name.clone(), ..b.clone() }] }).unwrap();
}

#[test]
fn removing_a_folder_removes_its_photos_and_comes_back_whole() {
    let (mut lib, folder, ids) = library_with(4);
    let before = lib.model().clone();
    let inverse = lib.apply(Op::RemoveFolder { id: folder }).unwrap();
    assert!(lib.photos().next().is_none() && lib.folder(folder).is_none());
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before);
    assert_eq!(lib.photos_in(folder).len(), ids.len());
}

#[test]
fn display_order_and_revisions() {
    let mut lib = Library::new();
    let (f1, f2, f3) = (lib.next_folder_id(), lib.next_folder_id(), lib.next_folder_id());
    for (id, path) in [(f1, "/p/old"), (f2, "/p/new"), (f3, "/p/empty")] {
        lib.apply(Op::AddFolder { folder: Folder { id, path: PathBuf::from(path) } }).unwrap();
    }
    let mut add = |folder, name: &str, date: Option<&str>, secs: i64| {
        let id = lib.next_photo_id();
        let p = Photo {
            id,
            folder,
            name: name.into(),
            size: 1,
            mtime: Timestamp::from_millis(secs * 1000),
            width: 1,
            height: 1,
            date: date.map(|d| d.parse().unwrap()),
            star: false,
        };
        lib.apply(Op::AddPhotos { photos: vec![p] }).unwrap();
        id
    };
    let b = add(f1, "b.jpg", Some("2020-01-02T00:00:00"), 0);
    let a = add(f1, "a.jpg", Some("2020-01-02T00:00:00"), 0);
    let c = add(f1, "c.jpg", None, 1_577_836_800); // mtime 2020-01-01, before both
    let z = add(f2, "z.jpg", Some("2024-05-05T00:00:00"), 0);
    assert_eq!(lib.photos_in(f1).iter().map(|p| p.id).collect::<Vec<_>>(), [c, a, b]);
    let order: Vec<FolderId> = lib.folders_in_display_order().iter().map(|f| f.id).collect();
    assert_eq!(order, [f2, f1, f3]);
    // Folder revisions move only for the folders an op touches.
    let (r1, r2) = (lib.folder_revision(f1), lib.folder_revision(f2));
    lib.apply(Op::SetStars { stars: vec![(z, true)] }).unwrap();
    assert_eq!(lib.folder_revision(f1), r1);
    assert!(lib.folder_revision(f2) > r2);
    assert_eq!(lib.folder_revision(f2), lib.revision());
    // Ids are never reused, even after removal.
    lib.apply(Op::RemovePhotos { ids: vec![z] }).unwrap();
    assert!(lib.next_photo_id() > z);
}
