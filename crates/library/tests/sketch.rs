//! Sketches in the model (`data-model.md` §4, `architecture.md` §6.5): opaque bytes capped at 32,
//! set in batches by an op with an exact inverse, dropped with their photo and brought back by the
//! inverse, and counted on the thumbnail revision rather than the library revision.

use std::path::PathBuf;

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, MAX_SKETCH, Op, Photo, Sketch};
use proptest::prelude::*;

fn photo(id: PhotoId, folder: FolderId, n: u8) -> Photo {
    let mtime = Timestamp::from_millis(1_700_000_000_000 + i64::from(n) * 1000);
    Photo { size: 100, mtime, width: 480, height: 320, ..crate::common::photo(id, folder, &format!("n{n}.jpg")) }
}

fn sketch(seed: u8, len: usize) -> Sketch {
    let bytes: Vec<u8> = (0..len).map(|i| seed.wrapping_mul(31).wrapping_add(i as u8)).collect();
    Sketch::new(&bytes).unwrap()
}

/// A recipe for an op against the current library (indices wrap around what exists).
#[derive(Clone, Debug)]
enum Seed {
    AddFolder(u8),
    RemoveFolder(u8),
    AddPhotos(Vec<(u8, u8)>),
    RemovePhotos(Vec<u8>),
    UpdatePhotos(Vec<(u8, u16)>),
    SetStars(Vec<(u8, bool)>),
    /// (photo index, `None` = clear, else (seed, length 1..=32)).
    SetSketches(Vec<(u8, Option<(u8, u8)>)>),
    Batch(Vec<Seed>),
    UnknownSketch,
}

fn leaf() -> impl Strategy<Value = Seed> {
    prop_oneof![
        1 => any::<u8>().prop_map(Seed::AddFolder),
        1 => any::<u8>().prop_map(Seed::RemoveFolder),
        2 => proptest::collection::vec((any::<u8>(), 0u8..12), 0..5).prop_map(Seed::AddPhotos),
        1 => proptest::collection::vec(any::<u8>(), 0..4).prop_map(Seed::RemovePhotos),
        1 => proptest::collection::vec((any::<u8>(), any::<u16>()), 0..4).prop_map(Seed::UpdatePhotos),
        1 => proptest::collection::vec((any::<u8>(), any::<bool>()), 0..4).prop_map(Seed::SetStars),
        4 => proptest::collection::vec((any::<u8>(), proptest::option::of((any::<u8>(), 1u8..=32))), 0..8).prop_map(Seed::SetSketches),
        1 => Just(Seed::UnknownSketch),
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
                    photo(id, pick(&folders, *f).unwrap_or(FolderId::new(999)), *n)
                })
                .collect(),
        },
        Seed::RemovePhotos(list) => Op::RemovePhotos { ids: list.iter().filter_map(|i| pick(&photos, *i)).collect() },
        Seed::UpdatePhotos(list) => Op::UpdatePhotos {
            photos: list
                .iter()
                .filter_map(|(i, size)| {
                    let mut p = lib.photo(pick(&photos, *i)?)?.clone();
                    p.size = u64::from(*size);
                    Some(p)
                })
                .collect(),
        },
        Seed::SetStars(list) => Op::SetStars { stars: list.iter().filter_map(|(i, on)| Some((pick(&photos, *i)?, *on))).collect() },
        Seed::SetSketches(list) => Op::SetSketches {
            sketches: list.iter().filter_map(|(i, s)| Some((pick(&photos, *i)?, s.map(|(seed, len)| sketch(seed, usize::from(len)))))).collect(),
        },
        Seed::Batch(seeds) => Op::Batch { ops: seeds.iter().map(|s| build(lib, s)).collect() },
        Seed::UnknownSketch => Op::SetSketches { sketches: vec![(PhotoId::new(u64::MAX), Some(sketch(1, 4)))] },
    }
}

proptest! {
    #![proptest_config(crate::common::cases(512, 32))]

    /// The library property test with sketch ops in the mix: an op then its inverse restores the
    /// model (sketches included), derived ops move only the thumbnail revision, every inverse
    /// survives the journal's JSON, and no sketch outlives its photo.
    #[test]
    fn apply_then_inverse_restores_the_model_with_sketches(seeds in proptest::collection::vec(seed(), 1..40)) {
        let mut lib = Library::new();
        for s in &seeds {
            let op = build(&mut lib, s);
            let before = lib.model().clone();
            let (rev, thumbs) = (lib.revision(), lib.thumbnail_revision());
            let derived = op.is_derived();
            match lib.apply(op.clone()) {
                Ok(inverse) => {
                    prop_assert_eq!(inverse.is_derived(), derived);
                    if derived {
                        prop_assert_eq!((lib.revision(), lib.thumbnail_revision()), (rev, thumbs + 1));
                    } else {
                        prop_assert_eq!(lib.revision(), rev + 1);
                        prop_assert!(lib.thumbnail_revision() - thumbs <= 1);
                    }
                    let json = serde_json::to_string(&inverse).unwrap();
                    prop_assert_eq!(&serde_json::from_str::<Op>(&json).unwrap(), &inverse);
                    let json = serde_json::to_string(&op).unwrap();
                    prop_assert_eq!(&serde_json::from_str::<Op>(&json).unwrap(), &op);
                    lib.apply(inverse).unwrap();
                    prop_assert_eq!(lib.model(), &before);
                    lib.apply(op).unwrap();
                }
                Err(_) => {
                    prop_assert_eq!((lib.revision(), lib.thumbnail_revision()), (rev, thumbs));
                    prop_assert_eq!(lib.model(), &before);
                }
            }
            for (id, _) in lib.sketches() {
                prop_assert!(lib.photo(id).is_some(), "a sketch outlived photo {}", id);
            }
        }
    }

    #[test]
    fn any_sketch_round_trips_through_base64_and_json(bytes in proptest::collection::vec(any::<u8>(), 1..=MAX_SKETCH)) {
        let s = Sketch::new(&bytes).unwrap();
        prop_assert_eq!(s.as_bytes(), &bytes[..]);
        let text = s.to_base64();
        prop_assert_eq!(text.len(), (bytes.len() * 4).div_ceil(3));
        prop_assert_eq!(Sketch::from_base64(&text), Ok(s));
        let json = serde_json::to_string(&s).unwrap();
        prop_assert_eq!(json, format!("\"{text}\""));
    }

    /// Hostile text never panics: it gives a sketch of at most 32 bytes or an error.
    #[test]
    fn any_text_decodes_to_a_capped_sketch_or_an_error(text in "[A-Za-z0-9+/=_ -]{0,60}") {
        if let Ok(s) = Sketch::from_base64(&text) {
            prop_assert!((1..=MAX_SKETCH).contains(&s.as_bytes().len()));
            prop_assert_eq!(s.to_base64(), text);
        }
    }
}

fn library_with(n: u8) -> (Library, FolderId, Vec<PhotoId>) {
    let mut lib = Library::new();
    let folder = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: folder, path: PathBuf::from("/photos/a") } }).unwrap();
    let ids: Vec<PhotoId> = (0..n).map(|_| lib.next_photo_id()).collect();
    let photos = ids.iter().zip(0u8..).map(|(&id, i)| photo(id, folder, i)).collect();
    lib.apply(Op::AddPhotos { photos }).unwrap();
    (lib, folder, ids)
}

#[test]
fn sketches_hold_one_to_32_bytes() {
    assert_eq!(Sketch::new(&[]), None);
    assert_eq!(Sketch::new(&[7; MAX_SKETCH + 1]), None);
    assert_eq!(Sketch::new(&[7; MAX_SKETCH]).unwrap().as_bytes(), &[7; MAX_SKETCH]);
    assert_eq!(Sketch::new(&[1, 2, 3]).unwrap().to_base64(), "AQID");
    assert_eq!(Sketch::new(&[0xff]).unwrap().to_base64(), "/w");
    assert_eq!(format!("{:?}", Sketch::new(&[1, 2]).unwrap()), "Sketch(AQI)");
    // Base64 of 33 bytes, empty text, a stray bit, a bad character, padding, a lone character.
    for bad in [&"A".repeat(44)[..], "", "/x", "AQ=D", "AQI=", "AQIDB", "A?"] {
        assert!(Sketch::from_base64(bad).is_err(), "{bad:?}");
    }
    // An op naming a 33-byte sketch does not parse, so a journal record holding one is refused.
    let json = format!(r#"{{"op":"setSketches","sketches":[[1,"{}"]]}}"#, "A".repeat(44));
    assert!(serde_json::from_str::<Op>(&json).is_err());
    let json = r#"{"op":"setSketches","sketches":[[1,"AQID"],[2,null]]}"#;
    let op: Op = serde_json::from_str(json).unwrap();
    assert_eq!(op, Op::SetSketches { sketches: vec![(PhotoId::new(1), Sketch::new(&[1, 2, 3])), (PhotoId::new(2), None)] });
    assert_eq!(serde_json::to_string(&op).unwrap(), json);
}

#[test]
fn a_sketch_batch_moves_only_the_thumbnail_revision() {
    let (mut lib, folder, ids) = library_with(4);
    let (rev, folder_rev, thumbs) = (lib.revision(), lib.folder_revision(folder), lib.thumbnail_revision());
    let batch: Vec<(PhotoId, Option<Sketch>)> = ids.iter().zip(1u8..).map(|(&id, i)| (id, Some(sketch(i, 25)))).collect();
    let inverse = lib.apply(Op::SetSketches { sketches: batch }).unwrap();
    assert_eq!((lib.revision(), lib.folder_revision(folder), lib.thumbnail_revision()), (rev, folder_rev, thumbs + 1));
    assert_eq!(lib.sketch_count(), 4);
    assert_eq!(lib.sketch(ids[2]), Some(&sketch(3, 25)));
    // The inverse clears them again, still without touching the library revision.
    assert!(inverse.is_derived());
    lib.apply(inverse).unwrap();
    assert_eq!((lib.revision(), lib.sketch_count(), lib.thumbnail_revision()), (rev, 0, thumbs + 2));
    // A batch of sketch ops is derived; a batch with anything else, or an empty one, is not.
    let set = || Op::SetSketches { sketches: vec![(ids[0], Some(sketch(9, 3)))] };
    assert!(Op::Batch { ops: vec![set(), set()] }.is_derived());
    assert!(!Op::Batch { ops: vec![set(), Op::SetStars { stars: vec![] }] }.is_derived());
    assert!(!Op::Batch { ops: vec![] }.is_derived());
    // A star still bumps the library revision and leaves the thumbnail revision alone.
    let thumbs = lib.thumbnail_revision();
    lib.apply(Op::SetStars { stars: vec![(ids[0], true)] }).unwrap();
    assert_eq!((lib.revision(), lib.thumbnail_revision()), (rev + 1, thumbs));
}

#[test]
fn removing_a_photo_drops_its_sketch_and_the_inverse_brings_it_back() {
    let (mut lib, folder, ids) = library_with(3);
    lib.apply(Op::SetSketches { sketches: vec![(ids[0], Some(sketch(1, 20))), (ids[1], Some(sketch(2, 32)))] }).unwrap();
    let before = lib.model().clone();
    let (rev, thumbs) = (lib.revision(), lib.thumbnail_revision());
    let inverse = lib.apply(Op::RemovePhotos { ids: vec![ids[0], ids[2]] }).unwrap();
    assert_eq!((lib.sketch(ids[0]), lib.sketch_count()), (None, 1));
    assert_eq!((lib.revision(), lib.thumbnail_revision()), (rev + 1, thumbs + 1));
    assert!(!inverse.is_derived(), "{inverse:?}");
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before);
    // A photo without a sketch comes back with a plain AddPhotos and the thumbnails stay put.
    let thumbs = lib.thumbnail_revision();
    let inverse = lib.apply(Op::RemovePhotos { ids: vec![ids[2]] }).unwrap();
    assert!(matches!(inverse, Op::AddPhotos { .. }), "{inverse:?}");
    assert_eq!(lib.thumbnail_revision(), thumbs);
    lib.apply(inverse).unwrap();
    // The whole folder goes and comes back with its sketches.
    let inverse = lib.apply(Op::RemoveFolder { id: folder }).unwrap();
    assert_eq!(lib.sketch_count(), 0);
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before);
}

#[test]
fn an_update_keeps_the_sketch_and_a_refused_sketch_op_changes_nothing() {
    let (mut lib, _, ids) = library_with(2);
    lib.apply(Op::SetSketches { sketches: vec![(ids[0], Some(sketch(1, 8)))] }).unwrap();
    // A rescan that sees a new size keeps the old sketch until the thumbnailer replaces it.
    let p = Photo { size: 5, ..lib.photo(ids[0]).unwrap().clone() };
    lib.apply(Op::UpdatePhotos { photos: vec![p] }).unwrap();
    assert_eq!(lib.sketch(ids[0]), Some(&sketch(1, 8)));
    let before = (lib.model().clone(), lib.revision(), lib.thumbnail_revision());
    let cases = [
        Op::SetSketches { sketches: vec![(ids[1], Some(sketch(2, 8))), (PhotoId::new(77), Some(sketch(3, 8)))] },
        Op::Batch { ops: vec![Op::SetSketches { sketches: vec![(ids[1], Some(sketch(2, 8)))] }, Op::RemovePhotos { ids: vec![PhotoId::new(77)] }] },
        Op::Batch { ops: vec![Op::RemovePhotos { ids: vec![ids[0]] }, Op::SetSketches { sketches: vec![(ids[0], None)] }] },
    ];
    for op in cases {
        assert!(lib.apply(op.clone()).is_err(), "{op:?}");
        assert_eq!((lib.model().clone(), lib.revision(), lib.thumbnail_revision()), before, "{op:?}");
    }
    // Later entries win, and the inverse restores the first value.
    let inverse = lib.apply(Op::SetSketches { sketches: vec![(ids[1], Some(sketch(4, 8))), (ids[1], None), (ids[1], Some(sketch(5, 2)))] }).unwrap();
    assert_eq!(lib.sketch(ids[1]), Some(&sketch(5, 2)));
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before.0);
}
