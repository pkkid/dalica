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

use dalica_core::{AlbumId, DateTime, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::{
    AlbumInfo, Anchor, ContentDigest, Facts, Folder, FolderInfo, Library, Op, Photo, PhotoInfo, PhotoSort, Project, Recipe, SaveState, Saved,
    SavedAs, SortBy, Synced, clean_tags,
};
use proptest::prelude::*;

/// A recipe for an op, turned into a real one against the current library (indices wrap around
/// the existing folders and photos; names are drawn from a small set so collisions happen).
#[derive(Clone, Debug)]
enum Seed {
    AddFolder(u8),
    RemoveFolder(u8),
    AddPhotos(Vec<(u8, u8)>),
    RemovePhotos(Vec<u8>),
    UpdatePhotos(Vec<(u8, u8, u16)>),
    SetStars(Vec<(u8, bool)>),
    SetCaptions(Vec<(u8, u8)>),
    AddTags(Vec<u8>, Vec<u8>),
    RemoveTags(Vec<u8>, Vec<u8>),
    RenameTag(u8, u8),
    SetTags(Vec<(u8, Vec<u8>)>),
    SetHidden(Vec<(u8, bool)>),
    SetRecipes(Vec<(u8, u8)>),
    SetDates(Vec<(u8, Option<u16>)>),
    SetPhotoInfo(Vec<(u8, u8, Vec<u8>)>),
    SetFolderInfo(Vec<(u8, u8)>),
    SetFoldersHidden(Vec<(u8, bool)>),
    SetSynced(Vec<(u8, Option<u8>)>),
    SetSaved(Vec<(u8, Option<u8>)>),
    SetSavedAs(Vec<(u8, Option<u8>)>),
    SetProjects(Vec<(u8, bool)>),
    /// The batches M8's file changes apply (when the library refuses to build one, an op on an
    /// unknown photo).
    Save(Vec<(u8, u8)>),
    UndoSave(Vec<u8>),
    Revert(Vec<u8>),
    SaveBeside(u8, u8),
    // M9.
    CreateAlbums(Vec<(u8, u8)>),
    DeleteAlbums(Vec<u8>),
    SetAlbumInfo(Vec<(u8, u8)>),
    AddMembers(Vec<(u8, u8, u8)>),
    RemoveMembers(Vec<(u8, u8)>),
    SetMemberKeys(Vec<(u8, u8, u8)>),
    SetCovers(Vec<(u8, Option<u8>)>),
    /// An album's add, reorder and sort ops (album, photos, anchor (before?, member), kind).
    AlbumOp(u8, Vec<u8>, Option<(bool, u8)>, u8),
    MovePhotos(Vec<(u8, u8, u8)>),
    MoveFolder(u8, u8),
    SetFolderSorts(Vec<(u8, u8)>),
    SetFacts(Vec<(u8, u8)>),
    SetDigests(Vec<(u8, Option<u8>)>),
    SetFirstIndexed(Vec<(u8, Option<u8>)>),
    SetLastChanged(Vec<(u8, Option<u8>)>),
    /// A seed's op stamped with a last-changed time.
    Stamped(Box<Seed>, u8),
    Batch(Vec<Seed>),
    UnknownPhoto,
    UnknownFolder,
    /// A tag or caption the caps refuse.
    BadText(u8),
}

/// Tags with case variants, so the case rule is exercised.
const TAGS: [&str; 7] = ["beach", "Beach", "BEACH", "sea", "Sea side", "dog", "Ünïcode"];
const CAPTIONS: [&str; 4] = ["", "Tram 28 at sunset", "a \"quoted\" caption\nover two lines", "Ünïcode ✓"];
const OPERATIONS: [&str; 5] = ["rotate(90)", "flip_h()", "crop(0.1,0.1,0.9,0.9)", "legacy:unknown(1)", "sepia()"];

fn tag(i: u8) -> String {
    TAGS[usize::from(i) % TAGS.len()].to_string()
}

fn tag_list(list: &[u8]) -> Vec<String> {
    let raw: Vec<String> = list.iter().map(|&i| tag(i)).collect();
    clean_tags(raw.iter().map(String::as_str))
}

fn recipe(n: u8) -> Recipe {
    let pick = |k: u8| OPERATIONS[usize::from(k) % OPERATIONS.len()].to_string();
    Recipe { edits: (0..n % 4).map(|k| pick(n.wrapping_add(k))).collect(), redo: (0..n % 3).map(pick).collect() }
}

fn date(d: u16) -> Option<DateTime> {
    DateTime::from_unix_secs(1_600_000_000 + i64::from(d) * 86_400)
}

fn photo_info(n: u8, tags: &[u8]) -> PhotoInfo {
    let r = recipe(n);
    PhotoInfo {
        caption: CAPTIONS[usize::from(n) % CAPTIONS.len()].to_string(),
        tags: tag_list(tags),
        hidden: n.is_multiple_of(3),
        edits: r.edits,
        redo: r.redo,
        date: if n.is_multiple_of(2) { date(u16::from(n)) } else { None },
    }
}

/// A saved record, done or pending.
fn saved(n: u8) -> Saved {
    Saved {
        recipe: recipe(n).edits,
        size: u64::from(n) * 1000,
        mtime: Timestamp::from_millis(1_700_000_000_000 + i64::from(n)),
        width: u32::from(n),
        height: 7,
        digest: format!("d{n}"),
        state: if n.is_multiple_of(3) { SaveState::Pending } else { SaveState::Done },
        time: if n.is_multiple_of(2) { Some(Timestamp::from_millis(i64::from(n))) } else { None },
    }
}

fn saved_as(n: u8) -> SavedAs {
    SavedAs { name: format!("copy{}.jpg", n % 4), recipe: recipe(n).edits }
}

/// An op every library refuses (its photo is unknown).
fn refused() -> Op {
    Op::SetStars { stars: vec![(PhotoId::new(u64::MAX), true)] }
}

fn folder_info(n: u8) -> FolderInfo {
    FolderInfo {
        description: CAPTIONS[usize::from(n) % CAPTIONS.len()].to_string(),
        date: if n.is_multiple_of(2) { date(u16::from(n)).map(|d| d.date()) } else { None },
        place: if n.is_multiple_of(3) { "Lisbon, Portugal".into() } else { String::new() },
        hidden: n.is_multiple_of(5),
    }
}

/// Album ids from a small set, so clashes and unknown albums happen.
fn album(n: u8) -> AlbumId {
    AlbumId::parse(&format!("a{:04x}", n % 5)).unwrap()
}

/// Keys from a small set, so equal keys happen.
fn key(n: u8) -> PosKey {
    PosKey::parse(["a0", "a1", "a2", "b00", "Zz"][usize::from(n) % 5]).unwrap()
}

fn album_info(n: u8) -> AlbumInfo {
    AlbumInfo {
        name: ["Best of 2026", "Trams", "Ünïcode ✓", "x"][usize::from(n) % 4].into(),
        description: CAPTIONS[usize::from(n) % CAPTIONS.len()].to_string(),
        date: if n.is_multiple_of(2) { date(u16::from(n)).map(|d| d.date()) } else { None },
        use_newest: n.is_multiple_of(3),
        place: if n.is_multiple_of(5) { "Lisbon, Portugal".into() } else { String::new() },
    }
}

fn facts(n: u8) -> Facts {
    Facts {
        make: ["", "Canon", "NIKON CORPORATION"][usize::from(n) % 3].into(),
        model: ["", "EOS 5D", "Z 6"][usize::from(n / 3) % 3].into(),
        video: n.is_multiple_of(4),
        geotagged: n.is_multiple_of(5),
    }
}

fn time(n: u8) -> Timestamp {
    Timestamp::from_millis(1_700_000_000_000 + i64::from(n) * 86_400_000)
}

fn sort(n: u8) -> PhotoSort {
    PhotoSort::new([SortBy::Date, SortBy::Name, SortBy::Size][usize::from(n) % 3], n.is_multiple_of(2))
}

fn pick_album(albums: &[AlbumId], i: u8) -> AlbumId {
    if albums.is_empty() || i.is_multiple_of(7) { album(i) } else { albums[usize::from(i) % albums.len()].clone() }
}

fn m9_leaf() -> impl Strategy<Value = Seed> {
    let list2 = || proptest::collection::vec((any::<u8>(), any::<u8>()), 0..4);
    let list3 = || proptest::collection::vec((any::<u8>(), any::<u8>(), any::<u8>()), 0..4);
    let opt = || proptest::collection::vec((any::<u8>(), proptest::option::of(any::<u8>())), 0..4);
    prop_oneof![
        list2().prop_map(Seed::CreateAlbums),
        proptest::collection::vec(any::<u8>(), 0..3).prop_map(Seed::DeleteAlbums),
        list2().prop_map(Seed::SetAlbumInfo),
        list3().prop_map(Seed::AddMembers),
        list2().prop_map(Seed::RemoveMembers),
        list3().prop_map(Seed::SetMemberKeys),
        opt().prop_map(Seed::SetCovers),
        (any::<u8>(), proptest::collection::vec(any::<u8>(), 0..4), proptest::option::of((any::<bool>(), any::<u8>())), any::<u8>())
            .prop_map(|(a, ids, at, kind)| Seed::AlbumOp(a, ids, at, kind)),
        list3().prop_map(Seed::MovePhotos),
        (any::<u8>(), any::<u8>()).prop_map(|(f, p)| Seed::MoveFolder(f, p)),
        list2().prop_map(Seed::SetFolderSorts),
        list2().prop_map(Seed::SetFacts),
        opt().prop_map(Seed::SetDigests),
        opt().prop_map(Seed::SetFirstIndexed),
        opt().prop_map(Seed::SetLastChanged),
    ]
}

fn leaf() -> impl Strategy<Value = Seed> {
    prop_oneof![
        3 => old_leaf(),
        2 => m9_leaf(),
        1 => (old_leaf(), any::<u8>()).prop_map(|(s, t)| Seed::Stamped(Box::new(s), t)),
        1 => (m9_leaf(), any::<u8>()).prop_map(|(s, t)| Seed::Stamped(Box::new(s), t)),
    ]
}

fn old_leaf() -> impl Strategy<Value = Seed> {
    prop_oneof![
        any::<u8>().prop_map(Seed::AddFolder),
        any::<u8>().prop_map(Seed::RemoveFolder),
        proptest::collection::vec((any::<u8>(), 0u8..12), 0..5).prop_map(Seed::AddPhotos),
        proptest::collection::vec(any::<u8>(), 0..4).prop_map(Seed::RemovePhotos),
        proptest::collection::vec((any::<u8>(), 0u8..12, any::<u16>()), 0..4).prop_map(Seed::UpdatePhotos),
        proptest::collection::vec((any::<u8>(), any::<bool>()), 0..6).prop_map(Seed::SetStars),
        proptest::collection::vec((any::<u8>(), any::<u8>()), 0..5).prop_map(Seed::SetCaptions),
        (proptest::collection::vec(any::<u8>(), 0..6), proptest::collection::vec(any::<u8>(), 0..4)).prop_map(|(i, t)| Seed::AddTags(i, t)),
        (proptest::collection::vec(any::<u8>(), 0..6), proptest::collection::vec(any::<u8>(), 0..3)).prop_map(|(i, t)| Seed::RemoveTags(i, t)),
        (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Seed::RenameTag(a, b)),
        proptest::collection::vec((any::<u8>(), proptest::collection::vec(any::<u8>(), 0..5)), 0..4).prop_map(Seed::SetTags),
        proptest::collection::vec((any::<u8>(), any::<bool>()), 0..6).prop_map(Seed::SetHidden),
        proptest::collection::vec((any::<u8>(), any::<u8>()), 0..4).prop_map(Seed::SetRecipes),
        proptest::collection::vec((any::<u8>(), proptest::option::of(0u16..3000)), 0..4).prop_map(Seed::SetDates),
        proptest::collection::vec((any::<u8>(), any::<u8>(), proptest::collection::vec(any::<u8>(), 0..4)), 0..4).prop_map(Seed::SetPhotoInfo),
        proptest::collection::vec((any::<u8>(), any::<u8>()), 0..3).prop_map(Seed::SetFolderInfo),
        proptest::collection::vec((any::<u8>(), any::<bool>()), 0..3).prop_map(Seed::SetFoldersHidden),
        proptest::collection::vec((any::<u8>(), proptest::option::of(any::<u8>())), 0..4).prop_map(Seed::SetSynced),
        proptest::collection::vec((any::<u8>(), proptest::option::of(any::<u8>())), 0..4).prop_map(Seed::SetSaved),
        proptest::collection::vec((any::<u8>(), proptest::option::of(any::<u8>())), 0..4).prop_map(Seed::SetSavedAs),
        proptest::collection::vec((any::<u8>(), any::<bool>()), 0..3).prop_map(Seed::SetProjects),
        proptest::collection::vec((any::<u8>(), any::<u8>()), 0..3).prop_map(Seed::Save),
        proptest::collection::vec(any::<u8>(), 0..3).prop_map(Seed::UndoSave),
        proptest::collection::vec(any::<u8>(), 0..3).prop_map(Seed::Revert),
        (any::<u8>(), any::<u8>()).prop_map(|(i, n)| Seed::SaveBeside(i, n)),
        Just(Seed::UnknownPhoto),
        Just(Seed::UnknownFolder),
        any::<u8>().prop_map(Seed::BadText),
    ]
}

fn seed() -> impl Strategy<Value = Seed> {
    prop_oneof![4 => leaf(), 1 => proptest::collection::vec(leaf(), 0..4).prop_map(Seed::Batch)]
}

fn pick<T: Copy>(items: &[T], i: u8) -> Option<T> {
    if items.is_empty() { None } else { items.get(usize::from(i) % items.len()).copied() }
}

/// Folder paths: six tops and three nested folders, so moves take subtrees along.
fn folder_path(n: u8) -> PathBuf {
    match n % 9 {
        k @ 0..6 => PathBuf::from(format!("/photos/f{k}")),
        k => PathBuf::from(format!("/photos/f{}/sub", k - 6)),
    }
}

/// A membership: the `i`th member of a picked album (or a pair that is none).
fn member(lib: &Library, albums: &[AlbumId], a: u8, i: u8) -> Option<(AlbumId, PhotoId)> {
    let a = pick_album(albums, a);
    let members = lib.album_members(&a).to_vec();
    let photo = pick(&members, i).or_else(|| lib.photos().next().map(|p| p.id))?;
    Some((a, photo))
}

fn build(lib: &mut Library, seed: &Seed) -> Op {
    let folders: Vec<FolderId> = lib.folders().map(|f| f.id).collect();
    let photos: Vec<PhotoId> = lib.photos().map(|p| p.id).collect();
    let albums: Vec<AlbumId> = lib.albums().map(|(id, _)| id.clone()).collect();
    match seed {
        Seed::AddFolder(n) => {
            let id = lib.next_folder_id();
            Op::AddFolder { folder: Folder { id, path: folder_path(*n) } }
        }
        Seed::RemoveFolder(i) => Op::RemoveFolder { id: pick(&folders, *i).unwrap_or(FolderId::new(999)) },
        Seed::AddPhotos(list) => Op::AddPhotos {
            photos: list
                .iter()
                .map(|(f, n)| {
                    let id = lib.next_photo_id();
                    photo(id, pick(&folders, *f).unwrap_or(FolderId::new(999)), *n, 1000)
                })
                .collect(),
        },
        Seed::RemovePhotos(list) => Op::RemovePhotos { ids: list.iter().filter_map(|i| pick(&photos, *i)).collect() },
        Seed::UpdatePhotos(list) => Op::UpdatePhotos {
            photos: list
                .iter()
                .filter_map(|(i, n, size)| {
                    let old = lib.photo(pick(&photos, *i)?)?;
                    let mut p = old.clone();
                    p.name = format!("n{n}.jpg");
                    p.size = u64::from(*size);
                    p.date = if size.is_multiple_of(3) { None } else { DateTime::from_unix_secs(1_780_000_000 + i64::from(*size)) };
                    Some(p)
                })
                .collect(),
        },
        Seed::SetStars(list) => Op::SetStars { stars: list.iter().filter_map(|(i, on)| Some((pick(&photos, *i)?, *on))).collect() },
        Seed::SetCaptions(list) => Op::SetCaptions {
            captions: list.iter().filter_map(|(i, c)| Some((pick(&photos, *i)?, CAPTIONS[usize::from(*c) % CAPTIONS.len()].to_string()))).collect(),
        },
        Seed::AddTags(ids, tags) => {
            Op::AddTags { ids: ids.iter().filter_map(|i| pick(&photos, *i)).collect(), tags: tags.iter().map(|&t| tag(t)).collect() }
        }
        Seed::RemoveTags(ids, tags) => {
            Op::RemoveTags { ids: ids.iter().filter_map(|i| pick(&photos, *i)).collect(), tags: tags.iter().map(|&t| tag(t)).collect() }
        }
        Seed::RenameTag(a, b) => Op::RenameTag { from: tag(*a), to: tag(*b) },
        Seed::SetTags(list) => Op::SetTags { tags: list.iter().filter_map(|(i, t)| Some((pick(&photos, *i)?, tag_list(t)))).collect() },
        Seed::SetHidden(list) => Op::SetHidden { hidden: list.iter().filter_map(|(i, on)| Some((pick(&photos, *i)?, *on))).collect() },
        Seed::SetRecipes(list) => Op::SetRecipes { recipes: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, recipe(*n)))).collect() },
        Seed::SetDates(list) => Op::SetDates { dates: list.iter().filter_map(|(i, d)| Some((pick(&photos, *i)?, d.and_then(date)))).collect() },
        Seed::SetPhotoInfo(list) => {
            Op::SetPhotoInfo { photos: list.iter().filter_map(|(i, n, t)| Some((pick(&photos, *i)?, photo_info(*n, t)))).collect() }
        }
        Seed::SetFolderInfo(list) => {
            Op::SetFolderInfo { folders: list.iter().filter_map(|(i, n)| Some((pick(&folders, *i)?, folder_info(*n)))).collect() }
        }
        Seed::SetFoldersHidden(list) => Op::SetFoldersHidden { hidden: list.iter().filter_map(|(i, on)| Some((pick(&folders, *i)?, *on))).collect() },
        Seed::SetSynced(list) => Op::SetSynced {
            synced: list
                .iter()
                .filter_map(|(i, h)| {
                    Some((
                        pick(&photos, *i)?,
                        h.map(|h| Synced { caption: Some(u64::from(h)), tags: h.checked_sub(100).map(u64::from), ..Synced::default() }),
                    ))
                })
                .collect(),
        },
        Seed::SetSaved(list) => Op::SetSaved { saved: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, n.map(saved)))).collect() },
        Seed::SetSavedAs(list) => Op::SetSavedAs { saved_as: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, n.map(saved_as)))).collect() },
        Seed::SetProjects(list) => {
            Op::SetProjects { projects: list.iter().filter_map(|(i, on)| Some((pick(&folders, *i)?, on.then_some(Project::Exported)))).collect() }
        }
        Seed::Save(list) => {
            lib.save_op(list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, saved(*n)))).collect()).unwrap_or_else(|_| refused())
        }
        Seed::UndoSave(list) => lib.undo_save_op(&list.iter().filter_map(|i| pick(&photos, *i)).collect::<Vec<_>>()).unwrap_or_else(|_| refused()),
        Seed::Revert(list) => lib.revert_op(&list.iter().filter_map(|i| pick(&photos, *i)).collect::<Vec<_>>()).unwrap_or_else(|_| refused()),
        Seed::SaveBeside(i, n) => match pick(&photos, *i) {
            Some(id) => lib.save_beside_op(id, saved_as(*n).name).unwrap_or_else(|_| refused()),
            None => refused(),
        },
        Seed::CreateAlbums(list) => Op::CreateAlbums { albums: list.iter().map(|(a, n)| (album(*a), album_info(*n))).collect() },
        Seed::DeleteAlbums(list) => Op::DeleteAlbums { ids: list.iter().map(|a| pick_album(&albums, *a)).collect() },
        Seed::SetAlbumInfo(list) => Op::SetAlbumInfo { albums: list.iter().map(|(a, n)| (pick_album(&albums, *a), album_info(*n))).collect() },
        Seed::AddMembers(list) => {
            Op::AddMembers { members: list.iter().filter_map(|(a, i, k)| Some((pick_album(&albums, *a), pick(&photos, *i)?, key(*k)))).collect() }
        }
        Seed::RemoveMembers(list) => Op::RemoveMembers { members: list.iter().filter_map(|(a, i)| member(lib, &albums, *a, *i)).collect() },
        Seed::SetMemberKeys(list) => {
            Op::SetMemberKeys { keys: list.iter().filter_map(|(a, i, k)| member(lib, &albums, *a, *i).map(|(a, p)| (a, p, key(*k)))).collect() }
        }
        Seed::SetCovers(list) => Op::SetCovers {
            covers: list.iter().map(|(a, i)| (pick_album(&albums, *a), i.and_then(|i| member(lib, &albums, *a, i).map(|(_, p)| p)))).collect(),
        },
        Seed::AlbumOp(a, ids, at, kind) => {
            let a = pick_album(&albums, *a);
            let members = lib.album_members(&a).to_vec();
            let anchor = at.and_then(|(before, i)| {
                let m = pick(&members, i)?;
                Some(if before { Anchor::Before(m) } else { Anchor::After(m) })
            });
            let result = match kind % 4 {
                0 => lib.album_add_op(&a, &ids.iter().filter_map(|i| pick(&photos, *i)).collect::<Vec<_>>(), anchor),
                1 => lib.album_reorder_op(&a, &ids.iter().filter_map(|i| pick(&members, *i)).collect::<Vec<_>>(), anchor),
                2 => lib.album_sort_op(&a, sort(*kind)),
                _ => lib.album_rekey_op(&a),
            };
            result.unwrap_or_else(|_| refused())
        }
        Seed::MovePhotos(list) => Op::MovePhotos {
            moves: list.iter().filter_map(|(i, f, n)| Some((pick(&photos, *i)?, pick(&folders, *f)?, format!("n{}.jpg", n % 12)))).collect(),
        },
        Seed::MoveFolder(f, n) => Op::MoveFolder { id: pick(&folders, *f).unwrap_or(FolderId::new(999)), path: folder_path(*n) },
        Seed::SetFolderSorts(list) => Op::SetFolderSorts { sorts: list.iter().filter_map(|(f, n)| Some((pick(&folders, *f)?, sort(*n)))).collect() },
        Seed::SetFacts(list) => Op::SetFacts { facts: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, facts(*n)))).collect() },
        Seed::SetDigests(list) => Op::SetDigests {
            digests: list
                .iter()
                .filter_map(|(i, n)| {
                    Some((pick(&photos, *i)?, n.map(|n| ContentDigest { size: u64::from(n), mtime: time(n), digest: format!("md5:{n}") })))
                })
                .collect(),
        },
        Seed::SetFirstIndexed(list) => {
            Op::SetFirstIndexed { indexed: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, n.map(time)))).collect() }
        }
        Seed::SetLastChanged(list) => {
            Op::SetLastChanged { changed: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, n.map(time)))).collect() }
        }
        Seed::Stamped(seed, t) => {
            let op = build(lib, seed);
            lib.stamped(op, time(*t))
        }
        Seed::Batch(seeds) => Op::Batch { ops: seeds.iter().map(|s| build(lib, s)).collect() },
        Seed::UnknownPhoto => Op::SetStars { stars: vec![(PhotoId::new(u64::MAX), true)] },
        Seed::UnknownFolder => Op::SetFoldersHidden { hidden: vec![(FolderId::new(u64::MAX), true)] },
        Seed::BadText(n) => {
            let id = pick(&photos, *n).unwrap_or(PhotoId::new(1));
            match n % 6 {
                0 => Op::AddTags { ids: vec![id], tags: vec![" padded ".into()] },
                1 => Op::SetCaptions { captions: vec![(id, "x".repeat(dalica_library::MAX_CAPTION + 1))] },
                2 => Op::SetTags { tags: vec![(id, vec!["dog".into(), "DOG".into()])] },
                3 => Op::SetSaved { saved: vec![(id, Some(Saved { digest: "x".repeat(dalica_library::MAX_DIGEST + 1), ..saved(*n) }))] },
                4 => Op::SetSavedAs { saved_as: vec![(id, Some(SavedAs { name: String::new(), recipe: vec![] }))] },
                _ => Op::RenameTag { from: "dog".into(), to: "two\nlines".into() },
            }
        }
    }
}

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

fn photo(id: PhotoId, folder: FolderId, n: u8, size: u64) -> Photo {
    Photo {
        id,
        folder,
        name: format!("n{n}.jpg"),
        size,
        mtime: Timestamp::from_millis(1_700_000_000_000 + i64::from(n) * 1000),
        width: 480,
        height: 320,
        date: DateTime::from_unix_secs(1_780_000_000 + i64::from(n) * 60),
        star: n.is_multiple_of(2),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

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
