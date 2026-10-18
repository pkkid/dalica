//! The embeddings cache (`faces.md` §3.3, §4, §6): embeddings, landmarks and searched photos
//! round-trip; a torn tail drops only the last record; a second model's cache is never mixed in;
//! removing a model deletes only its file; a crash before or after an append's commit keeps the
//! old or the new state; hostile files and records never panic.

use std::path::Path;

use dalica_core::{PhotoId, Rect01, Timestamp};
use dalica_fs::faults::{Fault, arm, disarm_all, points};
use dalica_store::faces_cache::{self, CacheError, CachedFace, FacesCache, Model, Opened, PhotoStamp, Record};
use proptest::prelude::*;

use crate::common::cases;

const SHA_A: &str = "0f1e2d3c4b5a69780f1e2d3c4b5a69780f1e2d3c4b5a69780f1e2d3c4b5a6978";
const SHA_B: &str = "aaaabbbbccccddddeeeeffff00001111aaaabbbbccccddddeeeeffff00001111";
const DIM: u32 = 8;

fn model(id: &str) -> Model<'_> {
    Model { id, sha256: SHA_A }
}

fn stamp(id: u64) -> PhotoStamp {
    PhotoStamp { id: PhotoId::new(id), size: 1000 + id, mtime: Timestamp::from_millis(1_780_000_000_000 + id as i64) }
}

fn rect(x: f64) -> Rect01 {
    Rect01::new(x, 0.1, x + 0.2, 0.4).unwrap()
}

/// A unit-length embedding seeded by `seed`.
fn embedding(seed: u32) -> Vec<f32> {
    let v: Vec<f32> = (0..DIM).map(|i| ((seed * 31 + i * 7) % 17) as f32 - 8.0).collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

fn face(x: f64, seed: u32) -> CachedFace {
    let landmarks = seed.is_multiple_of(2).then_some([[0.1, 0.2], [0.3, 0.2], [0.2, 0.3], [0.15, 0.35], [0.25, 0.35]]);
    CachedFace { rect: rect(x), landmarks, embedding: embedding(seed) }
}

fn open(dir: &Path, m: &Model) -> (FacesCache, Opened) {
    FacesCache::open(dir, m, DIM).unwrap()
}

/// The cache's contents in a comparable order.
fn contents(c: &FacesCache) -> Vec<(u64, bool, usize)> {
    let mut v: Vec<(u64, bool, usize)> = c.photos().map(|p| (p.stamp.id.get(), p.searched, p.faces.len())).collect();
    v.sort_unstable();
    v
}

#[test]
fn embeddings_landmarks_and_searched_photos_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let m = model("sface-2021dec");
    let (mut c, opened) = open(dir.path(), &m);
    assert_eq!(opened, Opened::New);
    c.append(&[
        Record::Face { photo: stamp(1), face: face(0.1, 1) },
        Record::Face { photo: stamp(1), face: face(0.5, 2) },
        Record::Searched { photo: stamp(1) },
        Record::Searched { photo: stamp(2) },
    ])
    .unwrap();
    c.append(&[Record::Face { photo: stamp(3), face: face(0.3, 3) }]).unwrap();
    drop(c);
    let (c, opened) = open(dir.path(), &m);
    assert_eq!(opened, Opened::Loaded { records: 5, skipped: 0 });
    assert_eq!(c.faces(), 3);
    assert_eq!(c.photo(&stamp(1)).map(|p| (p.searched, p.faces.clone())), Some((true, vec![face(0.1, 1), face(0.5, 2)])));
    assert!(c.searched(&stamp(2)) && !c.searched(&stamp(3)));
    assert_eq!(c.face(&stamp(3), &rect(0.3)), Some(&face(0.3, 3)));
    // The file changed on disk (another size or time), or an index rebuilt with another photo
    // under the id: nothing is found.
    let changed = PhotoStamp { size: 7, ..stamp(1) };
    assert!(c.photo(&changed).is_none() && !c.searched(&changed) && c.face(&changed, &rect(0.1)).is_none());
    drop(c);

    // A record for the photo's new file starts it afresh; a face at the same rectangle replaces;
    // Forget drops a photo; a rewrite keeps exactly what is held and makes the file smaller.
    let (mut c, _) = open(dir.path(), &m);
    c.append(&[Record::Searched { photo: changed }, Record::Face { photo: stamp(3), face: face(0.3, 9) }, Record::Forget { photo: PhotoId::new(2) }])
        .unwrap();
    assert_eq!(contents(&c), [(1, true, 0), (3, false, 1)]);
    assert_eq!(c.face(&stamp(3), &rect(0.3)).map(|f| f.embedding.clone()), Some(embedding(9)));
    let before = std::fs::metadata(c.path()).unwrap().len();
    c.rewrite(&m).unwrap();
    assert!(std::fs::metadata(c.path()).unwrap().len() < before);
    c.append(&[Record::Searched { photo: stamp(4) }]).unwrap();
    let want = contents(&c);
    drop(c);
    let (c, opened) = open(dir.path(), &m);
    assert_eq!((contents(&c), opened), (want, Opened::Loaded { records: 3, skipped: 0 }));
}

#[test]
fn a_torn_tail_drops_only_the_last_record() {
    let dir = tempfile::tempdir().unwrap();
    let m = model("sface");
    let (mut c, _) = open(dir.path(), &m);
    c.append(&[Record::Face { photo: stamp(1), face: face(0.1, 1) }, Record::Searched { photo: stamp(1) }]).unwrap();
    c.append(&[Record::Face { photo: stamp(2), face: face(0.1, 2) }]).unwrap();
    let path = c.path().to_path_buf();
    drop(c);
    let bytes = std::fs::read(&path).unwrap();
    for cut in [1, 3, 20, 40] {
        std::fs::write(&path, &bytes[..bytes.len() - cut]).unwrap();
        let (mut c, opened) = open(dir.path(), &m);
        assert!(matches!(opened, Opened::TornTail { records: 2, dropped, .. } if dropped > 0), "{cut}: {opened:?}");
        assert_eq!(contents(&c), [(1, true, 1)], "{cut}");
        // The torn tail was cut, so the next record is read back after the others.
        c.append(&[Record::Searched { photo: stamp(5) }]).unwrap();
        drop(c);
        let (c, opened) = open(dir.path(), &m);
        assert_eq!((opened, contents(&c)), (Opened::Loaded { records: 3, skipped: 0 }, vec![(1, true, 1), (5, true, 0)]), "{cut}");
    }
    // A flipped byte in a record's middle fails its checksum: that record and what follows go.
    std::fs::write(&path, &bytes).unwrap();
    let mut flipped = bytes.clone();
    let at = flipped.len() - 30;
    flipped[at] ^= 0x40;
    std::fs::write(&path, &flipped).unwrap();
    let (c, opened) = open(dir.path(), &m);
    assert!(matches!(opened, Opened::TornTail { records: 2, .. }), "{opened:?}");
    assert_eq!(contents(&c), [(1, true, 1)]);
}

/// Each model keeps its own file; another model's (or another version's under the same id, or
/// another dimension) is never read; removing a model deletes only its file.
#[test]
fn a_second_model_is_never_mixed_in_and_removing_one_deletes_only_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (model("sface"), Model { id: "auraface-v1", sha256: SHA_B });
    let (mut ca, _) = open(dir.path(), &a);
    ca.append(&[Record::Face { photo: stamp(1), face: face(0.1, 1) }, Record::Searched { photo: stamp(1) }]).unwrap();
    let (mut cb, opened) = open(dir.path(), &b);
    assert_eq!((opened, cb.faces()), (Opened::New, 0), "another model starts empty");
    cb.append(&[Record::Searched { photo: stamp(2) }]).unwrap();
    drop((ca, cb));
    let (ca, _) = open(dir.path(), &a);
    let (cb, _) = open(dir.path(), &b);
    assert_eq!((contents(&ca), contents(&cb)), (vec![(1, true, 1)], vec![(2, true, 0)]));
    drop((ca, cb));
    assert_eq!(faces_cache::list(dir.path()).unwrap(), ["auraface-v1", "sface"]);

    // The same id with another file (a new version) or another dimension: started again, empty.
    let newer = Model { id: "sface", sha256: SHA_B };
    let (c, opened) = open(dir.path(), &newer);
    assert_eq!((opened, c.faces()), (Opened::Replaced { why: "the face cache of another model".into() }, 0));
    drop(c);
    let (c, opened) = FacesCache::open(dir.path(), &newer, DIM * 2).unwrap();
    assert_eq!((opened, c.dim()), (Opened::Replaced { why: "a face cache of another embedding dimension".into() }, DIM * 2));
    drop(c);

    // Removing one model deletes its file and nothing else: not the other's, not a file of
    // another shape.
    let foreign = ["faces-x.txt", "faces-Upper.cache", "notes.cache", "faces-.cache"];
    for name in foreign {
        std::fs::write(dir.path().join(name), b"theirs").unwrap();
    }
    assert!(faces_cache::remove(dir.path(), "sface").unwrap());
    assert!(!faces_cache::remove(dir.path(), "sface").unwrap(), "already gone");
    assert!(faces_cache::path(dir.path(), "auraface-v1").unwrap().exists());
    for name in foreign {
        assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), b"theirs", "{name}");
    }
    assert_eq!(faces_cache::list(dir.path()).unwrap(), ["auraface-v1"]);
    let (cb, _) = open(dir.path(), &b);
    assert_eq!(contents(&cb), [(2, true, 0)]);
}

/// A crash before an append's commit (half the bytes written) loses only that batch, whole; a
/// crash after it (written, not synced) keeps it; an error cuts the file back and the next append
/// lands.
#[test]
fn a_crash_before_or_after_the_commit_keeps_the_old_or_the_new_state() {
    let batch = [Record::Face { photo: stamp(2), face: face(0.2, 2) }, Record::Searched { photo: stamp(2) }];
    for (point, fault, kept) in
        [(points::APPEND_WRITE, Fault::Crash, false), (points::APPEND_FSYNC, Fault::Crash, true), (points::APPEND_WRITE, Fault::Error, false)]
    {
        let dir = tempfile::tempdir().unwrap();
        let m = model("sface");
        let (mut c, _) = open(dir.path(), &m);
        c.append(&[Record::Face { photo: stamp(1), face: face(0.1, 1) }]).unwrap();
        arm(point, fault);
        let out = c.append(&batch);
        disarm_all();
        assert!(matches!(out, Err(CacheError::Fs(_))), "{point} {fault:?}: {out:?}");
        assert_eq!(contents(&c), [(1, false, 1)], "{point} {fault:?}: memory holds only what was written");
        if fault == Fault::Error {
            c.append(&[Record::Searched { photo: stamp(3) }]).unwrap();
        }
        drop(c);
        let (c, _) = open(dir.path(), &m);
        let mut want = vec![(1, false, 1)];
        if kept {
            want.push((2, true, 1));
        }
        if fault == Fault::Error {
            want.push((3, true, 0));
        }
        assert_eq!(contents(&c), want, "{point} {fault:?}");
    }
}

#[test]
fn hostile_models_and_records_are_refused_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    for id in ["", "../x", "Sface", "-x", "a/b", "a\\b", "x y", &"a".repeat(65)] {
        assert!(matches!(FacesCache::open(dir.path(), &Model { id, sha256: SHA_A }, DIM), Err(CacheError::BadModel(_))), "{id:?}");
        assert!(faces_cache::remove(dir.path(), id).is_err(), "{id:?}");
    }
    for sha in ["", "xyz", &SHA_A.to_uppercase(), &SHA_A[1..]] {
        assert!(matches!(FacesCache::open(dir.path(), &Model { id: "m", sha256: sha }, DIM), Err(CacheError::BadModel(_))), "{sha:?}");
    }
    for dim in [0, faces_cache::MAX_DIM + 1] {
        assert!(matches!(FacesCache::open(dir.path(), &model("m"), dim), Err(CacheError::BadModel(_))), "{dim}");
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing written");
    let (mut c, _) = open(dir.path(), &model("m"));
    let size = std::fs::metadata(c.path()).unwrap().len();
    let short = CachedFace { embedding: vec![0.5; DIM as usize - 1], ..face(0.1, 1) };
    let nan = CachedFace { embedding: vec![f32::NAN; DIM as usize], ..face(0.1, 1) };
    let inf = CachedFace { landmarks: Some([[f32::INFINITY, 0.0]; 5]), ..face(0.1, 1) };
    for bad in [short, nan, inf] {
        let batch = [Record::Searched { photo: stamp(1) }, Record::Face { photo: stamp(1), face: bad }];
        assert!(matches!(c.append(&batch), Err(CacheError::BadRecord(_))));
    }
    assert_eq!((std::fs::metadata(c.path()).unwrap().len(), c.photos().count()), (size, 0), "a refused batch writes and applies nothing");
}

proptest! {
    #![proptest_config(cases(256, 32))]

    /// Any bytes after a valid header, or as the whole file, never panic: what reads is kept, the
    /// rest cut or started again, and the cache still takes new records.
    #[test]
    fn hostile_files_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512), keep_header in any::<bool>()) {
        let dir = tempfile::tempdir().unwrap();
        let m = model("m");
        let path = faces_cache::path(dir.path(), "m").unwrap();
        let mut file = Vec::new();
        if keep_header {
            drop(open(dir.path(), &m));
            file = std::fs::read(&path).unwrap();
        }
        file.extend_from_slice(&bytes);
        std::fs::write(&path, &file).unwrap();
        let (mut c, _) = open(dir.path(), &m);
        c.append(&[Record::Searched { photo: stamp(9) }]).unwrap();
        drop(c);
        let (c, _) = open(dir.path(), &m);
        prop_assert!(c.searched(&stamp(9)));
    }
}
