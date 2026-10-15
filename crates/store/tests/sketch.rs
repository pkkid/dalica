//! Sketches in the index (`data-model.md` §4): they survive journal replay and a snapshot, a torn
//! journal tail loses only the torn batch, none reaches a folder's `.dalica.json`, and a rebuild
//! from the folders starts without them. Also measures what 100 k sketches add to the index.

use std::path::{Path, PathBuf};

use crate::common::NOW;
use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Model, Op, Photo, Sketch};
use dalica_store::{Opened, Recovery, SidecarWriter, Store, import_sidecars, journal, mapping, snapshot};

fn photo(id: PhotoId, folder: FolderId, name: String) -> Photo {
    Photo { id, folder, name, size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false }
}

/// A 25-byte sketch (the size the thumbnailer makes) that differs per photo.
fn sketch(id: PhotoId) -> Sketch {
    let seed = id.get();
    let bytes: Vec<u8> = (0..25u64).map(|i| (seed.wrapping_mul(2_654_435_761).wrapping_add(i * 97) >> 3) as u8).collect();
    Sketch::new(&bytes).unwrap()
}

/// A library with `folders` folders of `per` photos each, under `root`.
fn library(root: &Path, folders: u64, per: u64) -> (Library, Vec<Op>) {
    let mut lib = Library::new();
    let mut ops = Vec::new();
    for f in 0..folders {
        let id = lib.next_folder_id();
        let add = Op::AddFolder { folder: Folder { id, path: root.join(format!("f{f}")) } };
        let photos = (0..per).map(|n| photo(lib.next_photo_id(), id, format!("IMG_{n:04}.jpg"))).collect();
        for op in [add, Op::AddPhotos { photos }] {
            lib.apply(op.clone()).unwrap();
            ops.push(op);
        }
    }
    (lib, ops)
}

/// Sketch batches for every photo, `per_batch` photos per op.
fn sketch_batches(lib: &Library, per_batch: usize) -> Vec<Op> {
    let ids: Vec<PhotoId> = lib.photos().map(|p| p.id).collect();
    ids.chunks(per_batch).map(|c| Op::SetSketches { sketches: c.iter().map(|&id| (id, Some(sketch(id)))).collect() }).collect()
}

fn record(o: &mut Opened, op: &Op) {
    o.library.apply(op.clone()).unwrap();
    o.store.append(op).unwrap();
}

#[test]
fn sketches_survive_journal_replay_and_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (lib, ops) = library(Path::new("/photos"), 2, 5);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    for op in &ops {
        record(&mut o, op);
    }
    for op in sketch_batches(&lib, 4) {
        record(&mut o, &op);
    }
    let first = o.library.photos().next().unwrap().id;
    // A removed photo takes its sketch with it; a cleared sketch stays cleared.
    record(&mut o, &Op::RemovePhotos { ids: vec![first] });
    let second = o.library.photos().next().unwrap().id;
    record(&mut o, &Op::SetSketches { sketches: vec![(second, None)] });
    let want = o.library.model().clone();
    assert_eq!(o.library.sketch_count(), 8);
    drop(o);

    let mut o = Store::open(dir.path(), NOW).unwrap();
    assert_eq!(o.report, []);
    assert_eq!(o.library.model(), &want, "after replay");
    assert_eq!(o.library.sketch(second), None);

    // Snapshot, then more sketch ops on top: both halves come back.
    o.store.snapshot(&o.library).unwrap();
    let text = String::from_utf8(std::fs::read(dir.path().join(snapshot::FILE)).unwrap()).unwrap();
    assert!(text.contains(&format!("[{},\"{}\"]", PhotoId::new(3).get(), sketch(PhotoId::new(3)).to_base64())), "{text}");
    record(&mut o, &Op::SetSketches { sketches: vec![(second, Some(sketch(PhotoId::new(99))))] });
    let want = o.library.model().clone();
    drop(o);
    let o = Store::open(dir.path(), NOW).unwrap();
    assert_eq!(o.report, []);
    assert_eq!(o.library.model(), &want, "after snapshot + replay");
    assert_eq!(o.library.sketch(second), Some(&sketch(PhotoId::new(99))));
}

/// A snapshot without sketches has no `sketches` key (so it reads the same in a version that does
/// not know them); one naming a photo it does not hold, or a sketch over 32 bytes, is refused.
#[test]
fn snapshots_without_sketches_look_as_before_and_bad_ones_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(snapshot::FILE);
    let (lib, _) = library(Path::new("/photos"), 1, 2);
    snapshot::write(&path, &lib, 2).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("sketches"));
    assert_eq!(snapshot::read(&bytes).unwrap().0.model(), lib.model());

    let line = |json: String| {
        let mut out = format!("{:08x} ", dalica_store::crc32(json.as_bytes())).into_bytes();
        out.extend_from_slice(json.as_bytes());
        out.push(b'\n');
        out
    };
    let base = r#"{"snapshot":1,"seq":2,"folders":[{"id":1,"path":"/p"}],"photos":[{"id":1,"folder":1,"name":"a.jpg","size":1,"mtime":0,"width":1,"height":1,"date":null,"star":false}]"#;
    let good = line(format!(r#"{base},"sketches":[[1,"AQID"]]}}"#));
    assert_eq!(snapshot::read(&good).unwrap().0.sketch(PhotoId::new(1)), Sketch::new(&[1, 2, 3]).as_ref());
    let dangling = line(format!(r#"{base},"sketches":[[2,"AQID"]]}}"#));
    assert!(snapshot::read(&dangling).unwrap_err().contains("no photo 2"));
    let oversized = line(format!(r#"{base},"sketches":[[1,"{}"]]}}"#, "A".repeat(44)));
    assert!(snapshot::read(&oversized).unwrap_err().contains("sketch"));
}

/// A cut inside the last sketch batch's record replays to the batches before it: a torn tail loses
/// that batch and nothing else.
#[test]
fn a_torn_tail_loses_only_the_torn_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (lib, ops) = library(Path::new("/photos"), 1, 9);
    let batches = sketch_batches(&lib, 3);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    for op in ops.iter().chain(&batches[..2]) {
        record(&mut o, op);
    }
    let want: Model = o.library.model().clone();
    record(&mut o, &batches[2]);
    drop(o);
    let bytes = std::fs::read(dir.path().join(journal::FILE)).unwrap();
    let ends = journal::read(&bytes).ends;
    let (start, end) = (ends[ends.len() - 2], ends[ends.len() - 1]);
    assert_eq!(end, bytes.len());
    // Cut inside the batch's record: just after the previous one, in the middle, one byte short
    // (every byte of a torn tail is `lib.rs`'s `truncation_at_every_byte…`).
    for len in [start + 1, (start + end) / 2, end - 1] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(journal::FILE), &bytes[..len]).unwrap();
        let o = Store::open(d.path(), NOW).unwrap();
        assert_eq!(o.library.model(), &want, "prefix {len}");
        assert_eq!(o.library.sketch_count(), 6, "prefix {len}");
        let kept = (ops.len() + 2) as u64;
        assert!(matches!(&o.report[..], [Recovery::JournalTail { kept: k, .. }] if *k == kept), "prefix {len}: {:?}", o.report);
    }
}

/// No sketch reaches `.dalica.json`: the sidecar of a folder whose photos all have sketches holds
/// only the user's stars, byte for byte what it holds without the sketches.
#[test]
fn no_sketch_reaches_the_sidecar() {
    let root = tempfile::tempdir().unwrap();
    let (mut lib, _) = library(root.path(), 1, 4);
    let folder = lib.folders().next().unwrap().id;
    std::fs::create_dir_all(&lib.folder(folder).unwrap().path).unwrap();
    let ids: Vec<PhotoId> = lib.photos().map(|p| p.id).collect();
    lib.apply(Op::SetStars { stars: vec![(ids[1], true)] }).unwrap();
    let plain = mapping::folder_state(&lib, folder).unwrap();
    for op in sketch_batches(&lib, 2) {
        lib.apply(op).unwrap();
    }
    assert_eq!(lib.sketch_count(), 4);
    assert_eq!(mapping::folder_state(&lib, folder).unwrap(), plain);

    let mut w = SidecarWriter::new();
    w.mark_dirty(folder);
    w.flush(&lib, NOW);
    let path: PathBuf = lib.folder(folder).unwrap().path.join(".dalica.json");
    let text = String::from_utf8(std::fs::read(&path).unwrap()).unwrap();
    assert!(!text.contains("sketch"), "{text}");
    for id in &ids {
        assert!(!text.contains(&sketch(*id).to_base64()), "{text}");
    }
    // A sketch-only change leaves the folder clean: nothing to write.
    let rev = lib.folder_revision(folder);
    lib.apply(Op::SetSketches { sketches: vec![(ids[0], None)] }).unwrap();
    assert_eq!(lib.folder_revision(folder), rev);
}

/// After the index is lost, the rebuilt library has the folders' stars back from the sidecars but
/// no sketches: they are derived data, made again by the thumbnailer.
#[test]
fn a_rebuild_from_the_folders_starts_without_sketches() {
    let root = tempfile::tempdir().unwrap();
    let lib_dir = tempfile::tempdir().unwrap();
    let (lib, ops) = library(root.path(), 1, 3);
    let folder = lib.folders().next().unwrap().id;
    std::fs::create_dir_all(&lib.folder(folder).unwrap().path).unwrap();
    let mut o = Store::open(lib_dir.path(), NOW).unwrap();
    for op in ops.iter().chain(&sketch_batches(&lib, 3)) {
        record(&mut o, op);
    }
    let star = o.library.photos().next().unwrap().id;
    record(&mut o, &Op::SetStars { stars: vec![(star, true)] });
    let mut w = SidecarWriter::new();
    w.mark_dirty(folder);
    w.flush(&o.library, NOW);
    o.store.snapshot(&o.library).unwrap();
    drop(o);
    // A damaged index opens empty.
    std::fs::write(lib_dir.path().join(snapshot::FILE), b"garbage\n").unwrap();
    let o = Store::open(lib_dir.path(), NOW).unwrap();
    assert!(matches!(o.report.last(), Some(Recovery::RebuildNeeded { .. })), "{:?}", o.report);
    assert_eq!(o.library.sketch_count(), 0);
    // The scan finds the same photos again; the sidecars give back stars and nothing else.
    let (mut rebuilt, _) = library(root.path(), 1, 3);
    let import = import_sidecars(&rebuilt, NOW);
    assert!(import.ops.iter().all(|op| !matches!(op, Op::SetSketches { .. })), "{:?}", import.ops);
    for op in import.ops {
        rebuilt.apply(op).unwrap();
    }
    assert_eq!(rebuilt.starred().len(), 1);
    assert_eq!(rebuilt.sketch_count(), 0);
}

/// What 100 k sketches add to the index: journal bytes when they arrive in batches of 1 000, and
/// snapshot bytes. Printed for the PR (`cargo test -p dalica-store --test all sketch:: -- --nocapture`)
/// and held under a ceiling: 25-byte sketches as base64 in JSON cost under 50 bytes each.
/// At full strength 100 k photos; in a task's gate 2 000 (the per-sketch ceilings are the same).
#[test]
fn index_growth_for_100k_sketches() {
    let photos: u64 = if crate::common::full() { 100_000 } else { 2_000 };
    let dir = tempfile::tempdir().unwrap();
    let (lib, ops) = library(Path::new("/photos"), 100, photos / 100);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    o.library = lib;
    for op in &ops {
        o.store.append(op).unwrap();
    }
    o.store.snapshot(&o.library).unwrap();
    let size = |name: &str| std::fs::metadata(dir.path().join(name)).unwrap().len();
    let (snap_before, log_before) = (size(snapshot::FILE), size(journal::FILE));
    for op in sketch_batches(&o.library.clone(), 1000) {
        record(&mut o, &op);
    }
    let log_growth = size(journal::FILE) - log_before;
    o.store.snapshot(&o.library).unwrap();
    let snap_growth = size(snapshot::FILE) - snap_before;
    let in_memory = photos * std::mem::size_of::<(PhotoId, Sketch)>() as u64;
    eprintln!(
        "{photos} sketches (25 bytes): journal +{log_growth} B ({:.1} B/sketch, {} batches), snapshot {snap_before} -> {} B (+{snap_growth} B, {:.1} B/sketch), in memory >= {in_memory} B",
        log_growth as f64 / photos as f64,
        photos / 1000,
        snap_before + snap_growth,
        snap_growth as f64 / photos as f64,
    );
    assert!(log_growth < 50 * photos, "journal +{log_growth}");
    assert!(snap_growth < 50 * photos, "snapshot +{snap_growth}");
    // And it all reads back.
    let want = o.library.model().clone();
    drop(o);
    let o = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((o.report.clone(), o.library.sketch_count()), (vec![], photos as usize));
    assert_eq!(o.library.model(), &want);
}
