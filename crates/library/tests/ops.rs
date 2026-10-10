//! `dalica-library` ops (`architecture.md` §11): for random op sequences, applying an op and then
//! its inverse restores the model, each apply bumps the revision once, and a refused op changes
//! nothing.

use std::path::PathBuf;

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo};
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
    Batch(Vec<Seed>),
    UnknownPhoto,
}

fn leaf() -> impl Strategy<Value = Seed> {
    prop_oneof![
        any::<u8>().prop_map(Seed::AddFolder),
        any::<u8>().prop_map(Seed::RemoveFolder),
        proptest::collection::vec((any::<u8>(), 0u8..12), 0..5).prop_map(Seed::AddPhotos),
        proptest::collection::vec(any::<u8>(), 0..4).prop_map(Seed::RemovePhotos),
        proptest::collection::vec((any::<u8>(), 0u8..12, any::<u16>()), 0..4).prop_map(Seed::UpdatePhotos),
        proptest::collection::vec((any::<u8>(), any::<bool>()), 0..6).prop_map(Seed::SetStars),
        Just(Seed::UnknownPhoto),
    ]
}

fn seed() -> impl Strategy<Value = Seed> {
    prop_oneof![4 => leaf(), 1 => proptest::collection::vec(leaf(), 0..4).prop_map(Seed::Batch)]
}

fn pick<T: Copy>(items: &[T], i: u8) -> Option<T> {
    if items.is_empty() { None } else { items.get(usize::from(i) % items.len()).copied() }
}

fn build(lib: &mut Library, seed: &Seed) -> Op {
    let folders: Vec<FolderId> = lib.folders().map(|f| f.id).collect();
    let photos: Vec<PhotoId> = lib.photos().map(|p| p.id).collect();
    match seed {
        Seed::AddFolder(n) => {
            let id = lib.next_folder_id();
            Op::AddFolder { folder: Folder { id, path: PathBuf::from(format!("/photos/f{}", n % 6)) } }
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
        Seed::Batch(seeds) => Op::Batch { ops: seeds.iter().map(|s| build(lib, s)).collect() },
        Seed::UnknownPhoto => Op::SetStars { stars: vec![(PhotoId::new(u64::MAX), true)] },
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn apply_then_inverse_restores_the_model(seeds in proptest::collection::vec(seed(), 1..40)) {
        let mut lib = Library::new();
        for s in &seeds {
            let op = build(&mut lib, s);
            let before = lib.model().clone();
            let rev = lib.revision();
            match lib.apply(op.clone()) {
                Ok(inverse) => {
                    prop_assert_eq!(lib.revision(), rev + 1);
                    // The op and its inverse survive the journal's JSON.
                    let json = serde_json::to_string(&inverse).unwrap();
                    prop_assert_eq!(&serde_json::from_str::<Op>(&json).unwrap(), &inverse);
                    lib.apply(inverse).unwrap();
                    prop_assert_eq!(lib.revision(), rev + 2);
                    prop_assert_eq!(lib.model(), &before);
                    // Go forward again so the library keeps growing.
                    lib.apply(op).unwrap();
                }
                Err(_) => {
                    prop_assert_eq!(lib.revision(), rev);
                    prop_assert_eq!(lib.model(), &before);
                }
            }
            // Indexes agree with the model.
            for p in lib.photos() {
                prop_assert_eq!(lib.photo_by_name(p.folder, &p.name).map(|q| q.id), Some(p.id));
            }
            for f in lib.folders() {
                prop_assert_eq!(lib.folder_by_path(&f.path), Some(f.id));
            }
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
