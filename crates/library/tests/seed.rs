//! The op generator `ops.rs` and `search_props.rs` share: a [`Seed`] (a recipe for an op, drawn by
//! proptest) built into a real op against the current library, every op of the model in the mix:
//! M13's people, faces and geotags, M14's legacy ids and facts and the project kinds, and M15's
//! video facts included ([`op_name`] names each, and `ops.rs` checks the mix reaches them all).

use std::path::PathBuf;

use dalica_core::geo::LatLon;
use dalica_core::{AlbumId, DateTime, FolderId, PersonId, PhotoId, PosKey, Rect01, Timestamp};
use dalica_library::{
    AlbumInfo, Anchor, ContentDigest, Face, FaceKey, FacePerson, Facts, Folder, FolderInfo, FolderSidecar, Geo, LegacyOriginal, Library, Op, Person,
    Photo, PhotoInfo, PhotoSort, Project, Recipe, SaveState, Saved, SavedAs, Sketch, SortBy, Synced, VideoFacts, clean_tags,
};
use proptest::prelude::*;

/// A recipe for an op, turned into a real one against the current library (indices wrap around
/// the existing folders and photos; names are drawn from a small set so collisions happen).
#[derive(Clone, Debug)]
pub enum Seed {
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
    /// A project kind per folder (`None` clears the mark).
    SetProjects(Vec<(u8, Option<u8>)>),
    SetSketches(Vec<(u8, Option<u8>)>),
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
    // M13.
    /// Whole face lists: (photo, faces as (place, who)); who 0 is Unnamed, 1 Ignored, else a person.
    SetFaces(Vec<(u8, Vec<(u8, u8)>)>),
    SetGeo(Vec<(u8, Option<u8>)>),
    AddPerson(u8, u8),
    RenamePerson(u8, u8, u8),
    MergePeople(u8, u8),
    DeletePerson(u8),
    /// A person's thumbnail: one of their faces, or `None`.
    SetPersonThumbnail(u8, Option<u8>),
    // M14.
    SetLegacyIds(Vec<(u8, Option<u8>)>, Vec<(u8, Option<u8>)>),
    SetFolderSidecars(Vec<(u8, u8)>),
    SetLegacyOriginals(Vec<(u8, Option<u8>)>),
    // M15.
    SetVideos(Vec<(u8, Option<u8>)>),
    /// A seed's op stamped with a last-changed time.
    Stamped(Box<Seed>, u8),
    Batch(Vec<Seed>),
    UnknownPhoto,
    UnknownFolder,
    /// A tag or caption the caps refuse.
    BadText(u8),
}

/// Tags with case variants, so the case rule is exercised.
pub const TAGS: [&str; 7] = ["beach", "Beach", "BEACH", "sea", "Sea side", "dog", "Ünïcode"];
pub const CAPTIONS: [&str; 4] = ["", "Tram 28 at sunset", "a \"quoted\" caption\nover two lines", "Ünïcode ✓"];
pub const OPERATIONS: [&str; 5] = ["rotate(90)", "flip_h()", "crop(0.1,0.1,0.9,0.9)", "legacy:unknown(1)", "sepia()"];

pub fn tag(i: u8) -> String {
    TAGS[usize::from(i) % TAGS.len()].to_string()
}

pub fn tag_list(list: &[u8]) -> Vec<String> {
    let raw: Vec<String> = list.iter().map(|&i| tag(i)).collect();
    clean_tags(raw.iter().map(String::as_str))
}

pub fn recipe(n: u8) -> Recipe {
    let pick = |k: u8| OPERATIONS[usize::from(k) % OPERATIONS.len()].to_string();
    Recipe { edits: (0..n % 4).map(|k| pick(n.wrapping_add(k))).collect(), redo: (0..n % 3).map(pick).collect() }
}

pub fn date(d: u16) -> Option<DateTime> {
    DateTime::from_unix_secs(1_600_000_000 + i64::from(d) * 86_400)
}

pub fn photo_info(n: u8, tags: &[u8]) -> PhotoInfo {
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
pub fn saved(n: u8) -> Saved {
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

pub fn saved_as(n: u8) -> SavedAs {
    SavedAs { name: format!("copy{}.jpg", n % 4), recipe: recipe(n).edits }
}

/// An op every library refuses (its photo is unknown).
pub fn refused() -> Op {
    Op::SetStars { stars: vec![(PhotoId::new(u64::MAX), true)] }
}

pub fn folder_info(n: u8) -> FolderInfo {
    FolderInfo {
        description: CAPTIONS[usize::from(n) % CAPTIONS.len()].to_string(),
        date: if n.is_multiple_of(2) { date(u16::from(n)).map(|d| d.date()) } else { None },
        place: if n.is_multiple_of(3) { "Lisbon, Portugal".into() } else { String::new() },
        hidden: n.is_multiple_of(5),
    }
}

/// Album ids from a small set, so clashes and unknown albums happen.
pub fn album(n: u8) -> AlbumId {
    AlbumId::parse(&format!("a{:04x}", n % 5)).unwrap()
}

/// Keys from a small set, so equal keys happen.
pub fn key(n: u8) -> PosKey {
    PosKey::parse(["a0", "a1", "a2", "b00", "Zz"][usize::from(n) % 5]).unwrap()
}

pub fn album_info(n: u8) -> AlbumInfo {
    AlbumInfo {
        name: ["Best of 2026", "Trams", "Ünïcode ✓", "x"][usize::from(n) % 4].into(),
        description: CAPTIONS[usize::from(n) % CAPTIONS.len()].to_string(),
        date: if n.is_multiple_of(2) { date(u16::from(n)).map(|d| d.date()) } else { None },
        use_newest: n.is_multiple_of(3),
        place: if n.is_multiple_of(5) { "Lisbon, Portugal".into() } else { String::new() },
    }
}

pub fn facts(n: u8) -> Facts {
    Facts {
        make: ["", "Canon", "NIKON CORPORATION"][usize::from(n) % 3].into(),
        model: ["", "EOS 5D", "Z 6"][usize::from(n / 3) % 3].into(),
        video: n.is_multiple_of(4),
        geotagged: n.is_multiple_of(5),
    }
}

/// People ids from a small set, so clashes and unknown people happen.
pub fn person(n: u8) -> PersonId {
    PersonId::parse(&format!("p{:04x}", n % 4)).unwrap()
}

pub fn pick_person(people: &[PersonId], i: u8) -> PersonId {
    if people.is_empty() || i.is_multiple_of(7) { person(i) } else { people[usize::from(i) % people.len()].clone() }
}

/// Names with a case variant, so the case rule refuses some.
pub const NAMES: [&str; 5] = ["Ana", "Rui", "ana", "Zé Ünïcode", "Maria Costa"];

/// A face at one of eight places, of nobody, an ignored face, or a person (known or not).
pub fn face(people: &[PersonId], place: u8, who: u8) -> Face {
    let x = f64::from(place % 8) / 10.0;
    let rect = Rect01::new(x, 0.1, x + 0.1, 0.3).unwrap();
    let who = match who % 4 {
        0 => FacePerson::Unnamed,
        1 => FacePerson::Ignored,
        _ => FacePerson::Person(pick_person(people, who)),
    };
    Face::new(rect, who)
}

pub fn geo(n: u8) -> Geo {
    if n.is_multiple_of(5) { Geo::Removed } else { Geo::At(LatLon::new(38.7 + f64::from(n % 10) / 10.0, -9.1 - f64::from(n / 10) / 10.0).unwrap()) }
}

/// Legacy ids from a small set, so two albums or people asking for one happen.
pub fn legacy_id(n: u8) -> String {
    format!("L{}", n % 4)
}

pub fn legacy_original(n: u8) -> LegacyOriginal {
    LegacyOriginal { size: u64::from(n) * 10, digest: if n.is_multiple_of(2) { format!("md5:{n}") } else { String::new() }, recipe: recipe(n).edits }
}

pub fn project(n: u8) -> Project {
    [Project::Exported, Project::Collages, Project::VideoFrames][usize::from(n) % 3]
}

/// A video's facts: an empty record (an unprobed video), or durations, sizes, turns, codecs and
/// rates drawn from `n`.
pub fn video(n: u8) -> VideoFacts {
    if n.is_multiple_of(6) {
        return VideoFacts::default();
    }
    VideoFacts {
        duration_ms: u64::from(n) * 1001,
        width: [1920, 3840, 640][usize::from(n) % 3],
        height: [1080, 2160, 480][usize::from(n) % 3],
        rotation: n % 4,
        video_codec: ["avc1", "hvc1", "V_VP9", ""][usize::from(n) % 4].into(),
        sound_codec: if n.is_multiple_of(2) { "mp4a".into() } else { String::new() },
        sound: n.is_multiple_of(2),
        frame_rate_milli: [29_970, 25_000, 60_000, 0][usize::from(n / 4) % 4],
    }
}

pub fn time(n: u8) -> Timestamp {
    Timestamp::from_millis(1_700_000_000_000 + i64::from(n) * 86_400_000)
}

pub fn sort(n: u8) -> PhotoSort {
    PhotoSort::new([SortBy::Date, SortBy::Name, SortBy::Size][usize::from(n) % 3], n.is_multiple_of(2))
}

pub fn pick_album(albums: &[AlbumId], i: u8) -> AlbumId {
    if albums.is_empty() || i.is_multiple_of(7) { album(i) } else { albums[usize::from(i) % albums.len()].clone() }
}

pub fn m9_leaf() -> impl Strategy<Value = Seed> {
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

/// The ops of M13 (people, faces, geotags), M14 (legacy ids and facts) and M15 (video facts).
pub fn later_leaf() -> impl Strategy<Value = Seed> {
    let opt = || proptest::collection::vec((any::<u8>(), proptest::option::of(any::<u8>())), 0..4);
    prop_oneof![
        3 => proptest::collection::vec((any::<u8>(), proptest::collection::vec((any::<u8>(), any::<u8>()), 0..4)), 0..3).prop_map(Seed::SetFaces),
        1 => opt().prop_map(Seed::SetGeo),
        2 => (any::<u8>(), any::<u8>()).prop_map(|(p, n)| Seed::AddPerson(p, n)),
        1 => (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(p, n, k)| Seed::RenamePerson(p, n, k)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Seed::MergePeople(a, b)),
        1 => any::<u8>().prop_map(Seed::DeletePerson),
        1 => (any::<u8>(), proptest::option::of(any::<u8>())).prop_map(|(p, f)| Seed::SetPersonThumbnail(p, f)),
        1 => (opt(), opt()).prop_map(|(a, p)| Seed::SetLegacyIds(a, p)),
        1 => proptest::collection::vec((any::<u8>(), any::<u8>()), 0..3).prop_map(Seed::SetFolderSidecars),
        1 => opt().prop_map(Seed::SetLegacyOriginals),
        2 => opt().prop_map(Seed::SetVideos),
    ]
}

pub fn leaf() -> impl Strategy<Value = Seed> {
    prop_oneof![
        3 => old_leaf(),
        2 => m9_leaf(),
        2 => later_leaf(),
        1 => (later_leaf(), any::<u8>()).prop_map(|(s, t)| Seed::Stamped(Box::new(s), t)),
        1 => (old_leaf(), any::<u8>()).prop_map(|(s, t)| Seed::Stamped(Box::new(s), t)),
        1 => (m9_leaf(), any::<u8>()).prop_map(|(s, t)| Seed::Stamped(Box::new(s), t)),
    ]
}

pub fn old_leaf() -> impl Strategy<Value = Seed> {
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
        proptest::collection::vec((any::<u8>(), proptest::option::of(any::<u8>())), 0..3).prop_map(Seed::SetProjects),
        proptest::collection::vec((any::<u8>(), proptest::option::of(any::<u8>())), 0..4).prop_map(Seed::SetSketches),
        proptest::collection::vec((any::<u8>(), any::<u8>()), 0..3).prop_map(Seed::Save),
        proptest::collection::vec(any::<u8>(), 0..3).prop_map(Seed::UndoSave),
        proptest::collection::vec(any::<u8>(), 0..3).prop_map(Seed::Revert),
        (any::<u8>(), any::<u8>()).prop_map(|(i, n)| Seed::SaveBeside(i, n)),
        Just(Seed::UnknownPhoto),
        Just(Seed::UnknownFolder),
        any::<u8>().prop_map(Seed::BadText),
    ]
}

pub fn seed() -> impl Strategy<Value = Seed> {
    prop_oneof![4 => leaf(), 1 => proptest::collection::vec(leaf(), 0..4).prop_map(Seed::Batch)]
}

pub fn pick<T: Copy>(items: &[T], i: u8) -> Option<T> {
    if items.is_empty() { None } else { items.get(usize::from(i) % items.len()).copied() }
}

/// Folder paths: six tops and three nested folders, so moves take subtrees along.
pub fn folder_path(n: u8) -> PathBuf {
    match n % 9 {
        k @ 0..6 => PathBuf::from(format!("/photos/f{k}")),
        k => PathBuf::from(format!("/photos/f{}/sub", k - 6)),
    }
}

/// A membership: the `i`th member of a picked album (or a pair that is none).
pub fn member(lib: &Library, albums: &[AlbumId], a: u8, i: u8) -> Option<(AlbumId, PhotoId)> {
    let a = pick_album(albums, a);
    let members = lib.album_members(&a).to_vec();
    let photo = pick(&members, i).or_else(|| lib.photos().next().map(|p| p.id))?;
    Some((a, photo))
}

pub fn build(lib: &mut Library, seed: &Seed) -> Op {
    let folders: Vec<FolderId> = lib.folders().map(|f| f.id).collect();
    let photos: Vec<PhotoId> = lib.photos().map(|p| p.id).collect();
    let albums: Vec<AlbumId> = lib.albums().map(|(id, _)| id.clone()).collect();
    let people: Vec<PersonId> = lib.people().map(|(id, _)| id.clone()).collect();
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
            Op::SetProjects { projects: list.iter().filter_map(|(i, n)| Some((pick(&folders, *i)?, n.map(project)))).collect() }
        }
        Seed::SetSketches(list) => Op::SetSketches {
            sketches: list
                .iter()
                .filter_map(|(i, n)| Some((pick(&photos, *i)?, n.and_then(|n| Sketch::new(&[n; 3][..usize::from(n % 3) + 1])))))
                .collect(),
        },
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
        Seed::SetFaces(list) => Op::SetFaces {
            faces: list
                .iter()
                .filter_map(|(i, faces)| Some((pick(&photos, *i)?, faces.iter().map(|(at, who)| face(&people, *at, *who)).collect())))
                .collect(),
        },
        Seed::SetGeo(list) => Op::SetGeo { geo: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, n.map(geo)))).collect() },
        Seed::AddPerson(p, n) => Op::AddPerson { id: person(*p), person: Person::named(NAMES[usize::from(*n) % NAMES.len()]) },
        Seed::RenamePerson(p, n, k) => Op::RenamePerson {
            id: pick_person(&people, *p),
            name: NAMES[usize::from(*n) % NAMES.len()].into(),
            nickname: if k.is_multiple_of(2) { String::new() } else { NAMES[usize::from(*k) % NAMES.len()].into() },
        },
        Seed::MergePeople(a, b) => Op::MergePeople { from: pick_person(&people, *a), into: pick_person(&people, *b) },
        Seed::DeletePerson(p) => Op::DeletePerson { id: pick_person(&people, *p) },
        Seed::SetPersonThumbnail(p, f) => {
            let id = pick_person(&people, *p);
            let faces = lib.person_faces(&id).to_vec();
            let thumbnail = f.and_then(|f| {
                let r = pick(&faces, f)?;
                Some(FaceKey { photo: r.photo, rect: lib.face(r)?.rect })
            });
            Op::SetPersonThumbnail { id, thumbnail }
        }
        Seed::SetLegacyIds(a, p) => Op::SetLegacyIds {
            albums: a.iter().map(|(i, n)| (pick_album(&albums, *i), n.map(legacy_id))).collect(),
            people: p.iter().map(|(i, n)| (pick_person(&people, *i), n.map(legacy_id))).collect(),
        },
        Seed::SetFolderSidecars(list) => Op::SetFolderSidecars {
            folders: list
                .iter()
                .filter_map(|(f, n)| {
                    Some((
                        pick(&folders, *f)?,
                        [FolderSidecar::None, FolderSidecar::Dalica, FolderSidecar::Legacy, FolderSidecar::Both][usize::from(*n) % 4],
                    ))
                })
                .collect(),
        },
        Seed::SetLegacyOriginals(list) => {
            Op::SetLegacyOriginals { originals: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, n.map(legacy_original)))).collect() }
        }
        Seed::SetVideos(list) => Op::SetVideos { videos: list.iter().filter_map(|(i, n)| Some((pick(&photos, *i)?, n.map(video)))).collect() },
        Seed::Stamped(seed, t) => {
            let op = build(lib, seed);
            lib.stamped(op, time(*t))
        }
        Seed::Batch(seeds) => Op::Batch { ops: seeds.iter().map(|s| build(lib, s)).collect() },
        Seed::UnknownPhoto => Op::SetStars { stars: vec![(PhotoId::new(u64::MAX), true)] },
        Seed::UnknownFolder => Op::SetFoldersHidden { hidden: vec![(FolderId::new(u64::MAX), true)] },
        Seed::BadText(n) => {
            let id = pick(&photos, *n).unwrap_or(PhotoId::new(1));
            match n % 9 {
                0 => Op::AddTags { ids: vec![id], tags: vec![" padded ".into()] },
                1 => Op::SetCaptions { captions: vec![(id, "x".repeat(dalica_library::MAX_CAPTION + 1))] },
                2 => Op::SetTags { tags: vec![(id, vec!["dog".into(), "DOG".into()])] },
                3 => Op::SetSaved { saved: vec![(id, Some(Saved { digest: "x".repeat(dalica_library::MAX_DIGEST + 1), ..saved(*n) }))] },
                4 => Op::SetSavedAs { saved_as: vec![(id, Some(SavedAs { name: String::new(), recipe: vec![] }))] },
                5 => Op::SetVideos { videos: vec![(id, Some(VideoFacts { rotation: 4 + n % 4, ..video(*n) }))] },
                6 => Op::SetVideos { videos: vec![(id, Some(VideoFacts { video_codec: "x".repeat(dalica_library::MAX_CODEC + 1), ..video(*n) }))] },
                7 => Op::SetLegacyIds {
                    albums: albums.first().map(|a| (a.clone(), Some(" padded ".to_string()))).into_iter().collect(),
                    people: vec![],
                },
                _ => Op::RenameTag { from: "dog".into(), to: "two\nlines".into() },
            }
        }
    }
}

pub fn photo(id: PhotoId, folder: FolderId, n: u8, size: u64) -> Photo {
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

/// The journal name of every op, so `ops.rs` can check the mix reaches each one. The match has no
/// wildcard: a new op does not compile until it is named here (and given a seed above).
pub fn op_name(op: &Op) -> &'static str {
    match op {
        Op::AddFolder { .. } => "addFolder",
        Op::RemoveFolder { .. } => "removeFolder",
        Op::AddPhotos { .. } => "addPhotos",
        Op::RemovePhotos { .. } => "removePhotos",
        Op::UpdatePhotos { .. } => "updatePhotos",
        Op::SetStars { .. } => "setStars",
        Op::SetSketches { .. } => "setSketches",
        Op::SetCaptions { .. } => "setCaptions",
        Op::AddTags { .. } => "addTags",
        Op::RemoveTags { .. } => "removeTags",
        Op::RenameTag { .. } => "renameTag",
        Op::SetTags { .. } => "setTags",
        Op::SetHidden { .. } => "setHidden",
        Op::SetRecipes { .. } => "setRecipes",
        Op::SetDates { .. } => "setDates",
        Op::SetPhotoInfo { .. } => "setPhotoInfo",
        Op::SetFolderInfo { .. } => "setFolderInfo",
        Op::SetFoldersHidden { .. } => "setFoldersHidden",
        Op::SetSynced { .. } => "setSynced",
        Op::SetSaved { .. } => "setSaved",
        Op::SetSavedAs { .. } => "setSavedAs",
        Op::SetProjects { .. } => "setProjects",
        Op::MovePhotos { .. } => "movePhotos",
        Op::MoveFolder { .. } => "moveFolder",
        Op::SetFolderSorts { .. } => "setFolderSorts",
        Op::CreateAlbums { .. } => "createAlbums",
        Op::DeleteAlbums { .. } => "deleteAlbums",
        Op::SetAlbumInfo { .. } => "setAlbumInfo",
        Op::AddMembers { .. } => "addMembers",
        Op::RemoveMembers { .. } => "removeMembers",
        Op::SetMemberKeys { .. } => "setMemberKeys",
        Op::SetCovers { .. } => "setCovers",
        Op::SetFacts { .. } => "setFacts",
        Op::SetDigests { .. } => "setDigests",
        Op::SetFirstIndexed { .. } => "setFirstIndexed",
        Op::SetLastChanged { .. } => "setLastChanged",
        Op::SetFaces { .. } => "setFaces",
        Op::SetGeo { .. } => "setGeo",
        Op::AddPerson { .. } => "addPerson",
        Op::RenamePerson { .. } => "renamePerson",
        Op::MergePeople { .. } => "mergePeople",
        Op::DeletePerson { .. } => "deletePerson",
        Op::SetPersonThumbnail { .. } => "setPersonThumbnail",
        Op::SetLegacyIds { .. } => "setLegacyIds",
        Op::SetFolderSidecars { .. } => "setFolderSidecars",
        Op::SetLegacyOriginals { .. } => "setLegacyOriginals",
        Op::SetVideos { .. } => "setVideos",
        Op::Batch { .. } => "batch",
    }
}

/// Every op's journal name ([`op_name`]).
pub const OP_NAMES: [&str; 48] = [
    "addFolder",
    "removeFolder",
    "addPhotos",
    "removePhotos",
    "updatePhotos",
    "setStars",
    "setSketches",
    "setCaptions",
    "addTags",
    "removeTags",
    "renameTag",
    "setTags",
    "setHidden",
    "setRecipes",
    "setDates",
    "setPhotoInfo",
    "setFolderInfo",
    "setFoldersHidden",
    "setSynced",
    "setSaved",
    "setSavedAs",
    "setProjects",
    "movePhotos",
    "moveFolder",
    "setFolderSorts",
    "createAlbums",
    "deleteAlbums",
    "setAlbumInfo",
    "addMembers",
    "removeMembers",
    "setMemberKeys",
    "setCovers",
    "setFacts",
    "setDigests",
    "setFirstIndexed",
    "setLastChanged",
    "setFaces",
    "setGeo",
    "addPerson",
    "renamePerson",
    "mergePeople",
    "deletePerson",
    "setPersonThumbnail",
    "setLegacyIds",
    "setFolderSidecars",
    "setLegacyOriginals",
    "setVideos",
    "batch",
];
