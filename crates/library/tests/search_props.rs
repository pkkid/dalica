//! `dalica-library` search properties (`organizing.md` §8.1): an index kept in step through random
//! op sequences (every op of `tests/ops.rs` in the mix, their inverses too) always equals one built
//! from scratch, and its answers always equal the reference matcher's, photo by photo; any text at
//! all is bounded by the grammar's caps and never panics.
//!
//! The op generator is `tests/ops.rs`'s, copied so each test file stands alone.

use std::path::PathBuf;

use dalica_core::{AlbumId, DateTime, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::search::{Age, Extract, Filters, MAX_TERM, MAX_WORDS, Parsed, Query, Scope, SearchIndex, Update};
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

/// Queries over the generator's vocabulary: tags, captions, album and folder names, file names,
/// dates, cameras, operators and their combinations.
const QUERIES: [&str; 24] = [
    "",
    "beach",
    "BEA",
    "sea side",
    "-beach",
    "tag:sea",
    "-tag:beach dog",
    "tram",
    "sunset -tram",
    "n1",
    "n1.jpg",
    "jpg -n3",
    "f1",
    "sub",
    "lisbon",
    "best",
    "trams -x",
    "ünï",
    "2026",
    "2020-09",
    "september",
    "camera:canon",
    "camera:z -nikon",
    "person:ana -beach",
];

fn scopes() -> [Scope; 3] {
    [
        Scope::default(),
        Scope { hidden_pictures: true, ..Scope::default() },
        Scope { hidden_pictures: true, small_pictures: true, small_threshold: 500, ..Scope::default() },
    ]
}

/// A library to start from: four folders (one nested), twelve photos with captions, tags, dates,
/// hidden flags and facts, folder infos and two albums, so every op has something to act on.
fn start() -> Library {
    let mut lib = Library::new();
    let seeds = [
        Seed::AddFolder(0),
        Seed::AddFolder(1),
        Seed::AddFolder(6),
        Seed::AddFolder(3),
        Seed::AddPhotos((0..12).map(|n| (n % 4, n)).collect()),
        Seed::SetPhotoInfo((0..12).map(|n| (n, n, vec![n, n + 3])).collect()),
        Seed::SetFolderInfo(vec![(0, 1), (1, 3), (2, 2), (3, 0)]),
        Seed::SetFacts((0..12).map(|n| (n, n)).collect()),
        Seed::CreateAlbums(vec![(0, 0), (1, 1)]),
        Seed::AddMembers(vec![(0, 0, 0), (0, 5, 1), (0, 7, 2), (1, 2, 0), (1, 5, 1)]),
    ];
    for s in &seeds {
        let op = build(&mut lib, s);
        lib.apply(op).unwrap();
    }
    lib
}

/// Every query, filter and age agree between the index and the reference matcher.
fn check_answers(lib: &Library, index: &SearchIndex, filters: Filters, age: Option<u8>) -> Result<(), TestCaseError> {
    let extract = Extract::from_library(lib);
    for scope in scopes() {
        for q in QUERIES {
            let query = Query { text: q.into(), filters, age: age.map(|d| Age { days: u32::from(d) * 20, now: time(30) }) };
            let want = extract.matching(&query, &scope);
            prop_assert_eq!(index.matching(&query, &scope), want.clone(), "{:?}", query);
            let mut got = index.search(&query, &scope).ids;
            got.sort();
            prop_assert_eq!(got, want);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn an_updated_index_equals_a_fresh_one(
        seeds in proptest::collection::vec(seed(), 1..32),
        undo in proptest::collection::vec(any::<bool>(), 32),
        filters in (any::<bool>(), any::<bool>(), any::<bool>()),
        age in proptest::option::of(any::<u8>()),
    ) {
        let mut lib = start();
        let mut index = SearchIndex::from_library(&lib);
        prop_assert_eq!(index.len(), 12);
        let mut inverses: Vec<Op> = Vec::new();
        for (i, s) in seeds.iter().enumerate() {
            let op = build(&mut lib, s);
            if let Ok(inverse) = lib.apply(op.clone()) {
                index.apply(Update::for_op(&lib, &op));
                inverses.push(inverse);
            }
            if undo.get(i).copied().unwrap_or(false) && let Some(inverse) = inverses.pop() {
                lib.apply(inverse.clone()).unwrap();
                index.apply(Update::for_op(&lib, &inverse));
            }
            prop_assert!(index == SearchIndex::from_library(&lib), "after {:?}", s);
        }
        let filters = Filters { starred: filters.0, videos: filters.1, geotagged: filters.2, faces: false };
        check_answers(&lib, &index, filters, age)?;
        check_answers(&lib, &index, Filters::default(), None)?;
    }

    #[test]
    fn any_text_is_bounded_and_never_panics(text in ".{0,400}", words in proptest::collection::vec("[-a-z:._ \u{202e}\u{0}]{0,12}", 0..200)) {
        let mut lib = Library::new();
        for s in [Seed::AddFolder(1), Seed::AddPhotos(vec![(0, 1), (0, 2), (0, 3)]), Seed::SetPhotoInfo(vec![(0, 1, vec![0, 3]), (1, 2, vec![4])])] {
            let op = build(&mut lib, &s);
            lib.apply(op).unwrap();
        }
        let index = SearchIndex::from_library(&lib);
        let joined = words.join(" ");
        for t in [text.as_str(), joined.as_str()] {
            let p = Parsed::new(t);
            prop_assert!(p.atoms.len() <= MAX_WORDS);
            prop_assert!(p.atoms.iter().flat_map(|a| &a.tokens).all(|t| !t.is_empty() && t.len() <= MAX_TERM));
            let query = Query::text(t);
            prop_assert_eq!(index.matching(&query, &Scope::default()), Extract::from_library(&lib).matching(&query, &Scope::default()));
            let _ = index.search(&query, &Scope::default());
            prop_assert!(index.suggest(t, &Scope::default(), 1000).len() <= 100);
        }
    }
}
