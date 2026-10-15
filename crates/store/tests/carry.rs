//! Sidecar entries travel with their files (`data-model.md` §8, §10.3): a photo moved, renamed or
//! copied by Dalica takes its whole entry (unknown keys, album memberships, the album's name), its
//! originals backup and the backup's record; pending debounced changes are not lost on the way;
//! a clash renamed on the way keeps both entries; every fault point of the two sidecar writes
//! leaves the entry in at least one file, and carrying again finishes the job.

use std::path::{Path, PathBuf};

use dalica_core::{AlbumId, FolderId, PhotoId, PosKey, Timestamp};
use dalica_fs::faults::{arm, disarm_all, run_at};
use dalica_fs::transfer::{self, Outcome as Transfer};
use dalica_library::{AlbumInfo, Folder, Library, Op, Photo, PhotoInfo};
use dalica_sidecar::originals::{self, Entry, State};
use dalica_sidecar::{Field, Limits, Originals, Sidecar};
use dalica_store::carry::{self, Carry, CarryError, Mode};
use dalica_store::mapping;
use dalica_store::{Outcome, SidecarWriter};
use serde_json::{Value, json};

const T0: Timestamp = Timestamp::from_millis(1_791_000_000_000);

fn at(ms: i64) -> Timestamp {
    Timestamp::from_millis(T0.as_millis() + ms)
}

fn trams() -> AlbumId {
    AlbumId::parse("a77d02").unwrap()
}

/// Two folders, Lisbon (`a`, three photos) and Porto (`b`, one photo named like Lisbon's first),
/// their sidecars written: Lisbon's first photo starred, captioned and in the album "Trams", with
/// keys Dalica does not know beside them.
struct World {
    _dir: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
    lib: Library,
    fa: FolderId,
    fb: FolderId,
    /// Lisbon's photos, then Porto's.
    ids: Vec<PhotoId>,
    writer: SidecarWriter,
}

const LISBON: [&str; 3] = ["IMG_0001.jpg", "IMG_0002.jpg", "東京 ;=.jpg"];

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("Lisbon"), dir.path().join("Porto"));
    let mut lib = Library::new();
    let mut ids = Vec::new();
    let mut folders = Vec::new();
    for (path, names) in [(&a, &LISBON[..]), (&b, &["IMG_0001.jpg"][..])] {
        std::fs::create_dir(path).unwrap();
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: path.clone() } }).unwrap();
        let mut photos = Vec::new();
        for (i, name) in names.iter().enumerate() {
            std::fs::write(path.join(name), format!("{} {i}", path.display())).unwrap();
            let id = lib.next_photo_id();
            ids.push(id);
            photos.push(Photo {
                id,
                folder: f,
                name: name.to_string(),
                size: 10,
                mtime: Timestamp::from_millis(5),
                width: 4,
                height: 3,
                date: None,
                star: false,
            });
        }
        lib.apply(Op::AddPhotos { photos }).unwrap();
        folders.push(f);
    }
    let p = ids[0];
    for op in [
        Op::SetStars { stars: vec![(p, true)] },
        Op::SetPhotoInfo {
            photos: vec![
                (p, PhotoInfo { caption: "Tram 28".into(), ..PhotoInfo::default() }),
                (ids[3], PhotoInfo { caption: "Porto's own".into(), ..PhotoInfo::default() }),
            ],
        },
        Op::CreateAlbums { albums: vec![(trams(), AlbumInfo { name: "Trams".into(), ..AlbumInfo::default() })] },
        Op::AddMembers { members: vec![(trams(), p, PosKey::first())] },
    ] {
        lib.apply(op).unwrap();
    }
    let mut writer = SidecarWriter::new();
    for &f in &folders {
        writer.mark_dirty(f);
    }
    assert!(writer.flush(&lib, T0).values().all(|o| *o == Outcome::Written));
    // Keys only other programs (or later versions) know.
    edit(&a, |root| {
        root["photos"]["IMG_0001.jpg"]["faces"] = json!([{ "rect": [0.1, 0.2, 0.3, 0.4], "person": "p7f3a2c" }]);
        root["photos"]["IMG_0001.jpg"]["x-other"] = json!({ "rating": 5 });
        root["photos"]["IMG_0002.jpg"] = json!({ "geo": [38.7, -9.1] });
    });
    edit(&b, |root| root["photos"]["IMG_0001.jpg"]["x-porto"] = json!(true));
    World { _dir: dir, a, b, lib, fa: folders[0], fb: folders[1], ids, writer }
}

fn read_json(folder: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap()
}

fn edit(folder: &Path, f: impl FnOnce(&mut Value)) {
    let mut root = read_json(folder);
    f(&mut root);
    std::fs::write(folder.join(".dalica.json"), Sidecar::parse(&serde_json::to_vec(&root).unwrap(), &Limits::default()).unwrap().print()).unwrap();
}

fn entry(folder: &Path, name: &str) -> Option<Value> {
    read_json(folder).get("photos").and_then(|p| p.get(name)).cloned()
}

/// Moves the file on disk (never replacing one), then the model, then the engine's part: both
/// folders' states sent to the writer, then the carry.
fn move_photo(w: &mut World, which: usize, name: &str, new_name: &str, now: Timestamp) -> dalica_store::CarryReport {
    let id = w.ids[which];
    let ident = dalica_fs::identity(&w.a.join(name)).unwrap().unwrap();
    assert!(matches!(transfer::move_file(&w.a, name, ident, &w.b, new_name).unwrap(), Transfer::Done(_)));
    w.lib.apply(Op::MovePhotos { moves: vec![(id, w.fb, new_name.into())] }).unwrap();
    w.writer.mark_dirty(w.fa);
    w.writer.mark_dirty(w.fb);
    let lib = &w.lib;
    w.writer.carry(&Carry::one(Mode::Move, &w.a, name, &w.b, new_name), w.fa, w.fb, |f| mapping::folder_state(lib, f), now)
}

#[test]
fn a_moved_photo_arrives_with_its_unknown_keys_and_memberships_and_leaves_the_source() {
    let mut w = world();
    let before = entry(&w.a, "IMG_0001.jpg").unwrap();
    let report = move_photo(&mut w, 0, "IMG_0001.jpg", "IMG_0001-001.jpg", at(5000));
    let carried = report.carried.unwrap();
    assert_eq!(carried.carried, [("IMG_0001.jpg".to_string(), "IMG_0001-001.jpg".to_string())]);
    // The whole entry, as it was: star, caption, membership, faces and the other program's key.
    assert_eq!(entry(&w.b, "IMG_0001-001.jpg").unwrap(), before);
    assert_eq!(before["albums"], json!({ "a77d02": "a0" }));
    assert_eq!(before["faces"][0]["person"], "p7f3a2c");
    // The album's name came along, so the album can be found from this folder alone.
    assert_eq!(read_json(&w.b)["albums"], json!({ "a77d02": { "name": "Trams" } }));
    // Gone from the source, whose other entries are untouched.
    assert_eq!(entry(&w.a, "IMG_0001.jpg"), None);
    assert_eq!(entry(&w.a, "IMG_0002.jpg").unwrap(), json!({ "geo": [38.7, -9.1] }));
    assert_eq!(read_json(&w.a).get("albums"), None, "no member left in Lisbon");
    // Porto's own photo is untouched.
    assert_eq!(entry(&w.b, "IMG_0001.jpg").unwrap(), json!({ "caption": "Porto's own", "x-porto": true }));
    // The model reads the same from the files.
    let sidecar = Sidecar::parse(&std::fs::read(w.b.join(".dalica.json")).unwrap(), &Limits::default()).unwrap();
    let reading = mapping::read_photo(&sidecar, "IMG_0001-001.jpg");
    assert_eq!(reading.star, mapping::Reading::Usable(true));
    // Carrying again finds nothing to carry and writes nothing.
    let bytes = (std::fs::read(w.a.join(".dalica.json")).unwrap(), std::fs::read(w.b.join(".dalica.json")).unwrap());
    let again = carry::entries(&Carry::one(Mode::Move, &w.a, "IMG_0001.jpg", &w.b, "IMG_0001-001.jpg"), at(6000)).unwrap();
    assert_eq!((again.carried.len(), again.absent.len()), (0, 1));
    assert_eq!(bytes, (std::fs::read(w.a.join(".dalica.json")).unwrap(), std::fs::read(w.b.join(".dalica.json")).unwrap()));
}

#[test]
fn a_star_still_in_the_debounce_lands_in_the_destination() {
    let mut w = world();
    let (p2, p3) = (w.ids[1], w.ids[2]);
    // Starred 100 ms after the last write: the write waits for the debounce.
    w.lib.apply(Op::SetStars { stars: vec![(p2, true), (p3, true)] }).unwrap();
    w.writer.mark_dirty(w.fa);
    assert!(w.writer.write_due(&w.lib, at(100)).is_empty(), "still in the debounce");
    assert_eq!(entry(&w.a, "IMG_0002.jpg").unwrap().get("star"), None);
    // The engine's order: the model moves the photo, its states go to the writer, then the carry.
    let report = move_photo(&mut w, 1, "IMG_0002.jpg", "IMG_0002.jpg", at(200));
    assert_eq!(report.carried.unwrap().carried.len(), 1);
    assert_eq!(entry(&w.b, "IMG_0002.jpg").unwrap(), json!({ "geo": [38.7, -9.1], "star": true }));
    assert_eq!(entry(&w.a, "IMG_0002.jpg"), None);
    // The other star of that debounce reached Lisbon on the way.
    assert_eq!(entry(&w.a, LISBON[2]).unwrap()["star"], true);

    // The other order (carried before the model moves the photo) loses nothing either.
    w.lib.apply(Op::SetStars { stars: vec![(p3, false)] }).unwrap();
    w.lib.apply(Op::SetPhotoInfo { photos: vec![(p3, PhotoInfo { caption: "late".into(), ..PhotoInfo::default() })] }).unwrap();
    w.writer.mark_dirty(w.fa);
    let ident = dalica_fs::identity(&w.a.join(LISBON[2])).unwrap().unwrap();
    assert!(matches!(transfer::move_file(&w.a, LISBON[2], ident, &w.b, LISBON[2]).unwrap(), Transfer::Done(_)));
    let lib = &w.lib;
    let report = w.writer.carry(&Carry::one(Mode::Move, &w.a, LISBON[2], &w.b, LISBON[2]), w.fa, w.fb, |f| mapping::folder_state(lib, f), at(300));
    assert_eq!(report.writes.get(&w.fa), Some(&Outcome::Written), "the pending write first");
    assert_eq!(entry(&w.b, LISBON[2]).unwrap(), json!({ "caption": "late" }));
    w.lib.apply(Op::MovePhotos { moves: vec![(p3, w.fb, LISBON[2].into())] }).unwrap();
    w.writer.mark_dirty(w.fa);
    w.writer.mark_dirty(w.fb);
    w.writer.flush(&w.lib, at(400));
    assert_eq!(entry(&w.b, LISBON[2]).unwrap(), json!({ "caption": "late" }));
    assert_eq!(entry(&w.a, LISBON[2]), None);
}

#[test]
fn the_model_stands_over_carried_keys_it_owns() {
    let mut w = world();
    // Unstarred and recaptioned in the debounce: the carried entry still says otherwise.
    w.lib.apply(Op::SetStars { stars: vec![(w.ids[0], false)] }).unwrap();
    w.lib.apply(Op::SetPhotoInfo { photos: vec![(w.ids[0], PhotoInfo::default())] }).unwrap();
    w.writer.mark_dirty(w.fa);
    move_photo(&mut w, 0, "IMG_0001.jpg", "IMG_0009.jpg", at(100)).carried.unwrap();
    let e = entry(&w.b, "IMG_0009.jpg").unwrap();
    assert_eq!((e.get("star"), e.get("caption")), (None, None));
    assert_eq!(e["x-other"], json!({ "rating": 5 }));
    assert_eq!(e["albums"], json!({ "a77d02": "a0" }));
}

#[test]
fn a_rename_keeps_the_entry() {
    let mut w = world();
    let before = entry(&w.a, "IMG_0001.jpg").unwrap();
    let ident = dalica_fs::identity(&w.a.join("IMG_0001.jpg")).unwrap().unwrap();
    assert!(matches!(transfer::move_file(&w.a, "IMG_0001.jpg", ident, &w.a, "Tram 28.jpg").unwrap(), Transfer::Done(_)));
    w.lib.apply(Op::MovePhotos { moves: vec![(w.ids[0], w.fa, "Tram 28.jpg".into())] }).unwrap();
    w.writer.mark_dirty(w.fa);
    let lib = &w.lib;
    let c = Carry::rename(&w.a, vec![("IMG_0001.jpg".into(), "Tram 28.jpg".into())]);
    let report = w.writer.carry(&c, w.fa, w.fa, |f| mapping::folder_state(lib, f), at(100));
    assert_eq!(report.carried.unwrap().carried.len(), 1);
    assert_eq!(entry(&w.a, "Tram 28.jpg").unwrap(), before);
    assert_eq!(entry(&w.a, "IMG_0001.jpg"), None);
    assert_eq!(read_json(&w.a)["albums"], json!({ "a77d02": { "name": "Trams" } }));
}

#[test]
fn a_move_that_renamed_a_duplicate_keeps_both_entries() {
    let mut w = world();
    let mine = entry(&w.a, "IMG_0001.jpg").unwrap();
    let theirs = entry(&w.b, "IMG_0001.jpg").unwrap();
    // The name is taken in Porto: nothing moves, and the operation picks the next free name.
    let ident = dalica_fs::identity(&w.a.join("IMG_0001.jpg")).unwrap().unwrap();
    assert_eq!(transfer::move_file(&w.a, "IMG_0001.jpg", ident, &w.b, "IMG_0001.jpg").unwrap(), Transfer::Exists);
    move_photo(&mut w, 0, "IMG_0001.jpg", "IMG_0001-001.jpg", at(100)).carried.unwrap();
    assert_eq!(entry(&w.b, "IMG_0001.jpg").unwrap(), theirs);
    assert_eq!(entry(&w.b, "IMG_0001-001.jpg").unwrap(), mine);
}

#[test]
fn a_copy_duplicates_the_entry() {
    let w = world();
    let before = (entry(&w.a, "IMG_0001.jpg").unwrap(), read_json(&w.a));
    let ident = dalica_fs::identity(&w.a.join("IMG_0001.jpg")).unwrap().unwrap();
    assert!(matches!(transfer::copy_file(&w.a, "IMG_0001.jpg", ident, &w.b, "IMG_0001 copy.jpg").unwrap(), Transfer::Done(_)));
    let out = carry::entries(&Carry::one(Mode::Copy, &w.a, "IMG_0001.jpg", &w.b, "IMG_0001 copy.jpg"), at(100)).unwrap();
    assert_eq!(out.carried.len(), 1);
    assert_eq!(entry(&w.b, "IMG_0001 copy.jpg").unwrap(), before.0);
    assert_eq!(read_json(&w.a), before.1, "the source is not written");
}

/// Plants a recorded backup of `name` in `folder` (a photo saved before), with a key the record's
/// reader does not know.
fn plant_backup(folder: &Path, name: &str, bytes: &[u8]) {
    let dir = dalica_fs::originals::ensure_dir(folder).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
    let path = dir.join(originals::FILE_NAME);
    let mut record = match std::fs::read(&path) {
        Ok(b) => Originals::parse(&b, &Limits::default()).unwrap(),
        Err(_) => Originals::new(),
    };
    let e = Entry {
        recipe: vec!["sepia()".into()],
        size: bytes.len() as u64,
        mtime: 1_700_000_000_000,
        width: 4,
        height: 3,
        digest: "blake3:00ff".into(),
        state: State::Done,
        time: Some(1_790_000_000_000),
    };
    record.set(name, &e).unwrap();
    let mut root: Value = serde_json::from_slice(&record.print()).unwrap();
    root[originals::KEY][name]["x-future"] = json!(1);
    std::fs::write(&path, serde_json::to_vec(&root).unwrap()).unwrap();
}

fn record(folder: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(folder.join(originals::DIR_NAME).join(originals::FILE_NAME)).unwrap()).unwrap()
}

#[test]
fn the_backup_and_its_record_travel_and_a_copy_duplicates_them() {
    let w = world();
    plant_backup(&w.a, "IMG_0001.jpg", b"the original");
    plant_backup(&w.a, "IMG_0002.jpg", b"another original");
    let kept = record(&w.a)[originals::KEY]["IMG_0001.jpg"].clone();
    assert_eq!(kept["x-future"], 1);
    // A move.
    let out = carry::backups(&Carry::one(Mode::Move, &w.a, "IMG_0001.jpg", &w.b, "IMG_0001-001.jpg"), at(100)).unwrap();
    assert_eq!(out.carried, [("IMG_0001.jpg".to_string(), "IMG_0001-001.jpg".to_string())]);
    let (src, dst) = (w.a.join(originals::DIR_NAME), w.b.join(originals::DIR_NAME));
    assert_eq!(std::fs::read(dst.join("IMG_0001-001.jpg")).unwrap(), b"the original");
    assert!(!src.join("IMG_0001.jpg").exists());
    assert_eq!(record(&w.b)[originals::KEY]["IMG_0001-001.jpg"], kept);
    assert_eq!(record(&w.a)[originals::KEY].get("IMG_0001.jpg"), None);
    assert!(record(&w.a)[originals::KEY].get("IMG_0002.jpg").is_some(), "the other record stays");
    // The record reads as the model's.
    let reading = Originals::parse(&std::fs::read(dst.join(originals::FILE_NAME)).unwrap(), &Limits::default()).unwrap();
    assert!(matches!(reading.entry("IMG_0001-001.jpg"), Field::Value(e) if e.size == 12));
    // Again: nothing left to carry.
    let again = carry::backups(&Carry::one(Mode::Move, &w.a, "IMG_0001.jpg", &w.b, "IMG_0001-001.jpg"), at(200)).unwrap();
    assert_eq!((again.carried.len(), again.absent.len()), (0, 1));

    // A copy duplicates the backup and its record.
    let other = record(&w.a)[originals::KEY]["IMG_0002.jpg"].clone();
    let out = carry::backups(&Carry::one(Mode::Copy, &w.a, "IMG_0002.jpg", &w.b, "IMG_0002.jpg"), at(300)).unwrap();
    assert_eq!(out.carried.len(), 1);
    assert_eq!(std::fs::read(src.join("IMG_0002.jpg")).unwrap(), b"another original");
    assert_eq!(std::fs::read(dst.join("IMG_0002.jpg")).unwrap(), b"another original");
    assert_eq!(record(&w.a)[originals::KEY]["IMG_0002.jpg"], other);
    assert_eq!(record(&w.b)[originals::KEY]["IMG_0002.jpg"], other);
    // A photo never saved has nothing to carry.
    let none = carry::backups(&Carry::one(Mode::Move, &w.a, LISBON[2], &w.b, LISBON[2]), at(400)).unwrap();
    assert_eq!(none.absent, [LISBON[2]]);
}

#[test]
fn a_backup_never_replaces_one_in_the_destination() {
    let w = world();
    plant_backup(&w.a, "IMG_0001.jpg", b"lisbon's original");
    plant_backup(&w.b, "IMG_0001.jpg", b"porto's original!");
    let porto = record(&w.b);
    let out = carry::backups(&Carry::one(Mode::Move, &w.a, "IMG_0001.jpg", &w.b, "IMG_0001.jpg"), at(100)).unwrap();
    assert_eq!((out.carried.len(), out.refused.len()), (0, 1));
    assert_eq!(std::fs::read(w.b.join(originals::DIR_NAME).join("IMG_0001.jpg")).unwrap(), b"porto's original!");
    assert_eq!(std::fs::read(w.a.join(originals::DIR_NAME).join("IMG_0001.jpg")).unwrap(), b"lisbon's original");
    assert_eq!(record(&w.b), porto);
    assert!(record(&w.a)[originals::KEY].get("IMG_0001.jpg").is_some());
}

/// An error or a crash at a step of either sidecar write (one of each kind; `fs` tries every point)
/// leaves the entry in at least one of the two files, and carrying again puts it in the
/// destination and out of the source.
#[test]
fn every_fault_point_keeps_the_entry_and_a_second_carry_finishes() {
    for write in 0..2 {
        for (point, fault) in crate::common::FAULTS {
            {
                let w = world();
                let want = entry(&w.a, "IMG_0001.jpg").unwrap();
                let c = Carry::one(Mode::Move, &w.a, "IMG_0001.jpg", &w.b, "IMG_0001-001.jpg");
                disarm_all();
                if write == 0 {
                    arm(point, fault);
                } else {
                    // The second write is the source's: armed once the first reaches the point.
                    run_at(point, move || run_at(point, move || arm(point, fault)));
                }
                let first = carry::entries(&c, at(100));
                disarm_all();
                let (src, dst) = (entry(&w.a, "IMG_0001.jpg"), entry(&w.b, "IMG_0001-001.jpg"));
                assert!(src.as_ref() == Some(&want) || dst.as_ref() == Some(&want), "{write} {point} {fault:?}: lost ({first:?})");
                // The fault fired, and a failed source write says the entry is in both files.
                match (write, &first) {
                    (0, Err(CarryError::Fs(_))) | (1, Err(CarryError::SourceKept { .. })) => {}
                    _ => panic!("{write} {point} {fault:?}: {first:?}"),
                }
                carry::entries(&c, at(200)).unwrap();
                assert_eq!(entry(&w.b, "IMG_0001-001.jpg").as_ref(), Some(&want), "{write} {point} {fault:?}");
                assert_eq!(entry(&w.a, "IMG_0001.jpg"), None, "{write} {point} {fault:?}");
            }
        }
    }
}

#[test]
fn a_damaged_destination_is_kept_aside_and_nothing_is_carried() {
    let mut w = world();
    std::fs::write(w.b.join(".dalica.json"), b"{ not json").unwrap();
    let before = entry(&w.a, "IMG_0001.jpg").unwrap();
    let report = move_photo(&mut w, 0, "IMG_0001.jpg", "IMG_0001-001.jpg", at(100));
    // Porto had a write pending: that write found the damage and kept the file aside.
    match (&report.carried, report.writes.get(&w.fb)) {
        (Err(CarryError::ReadOnly(p)), Some(Outcome::SetAside(kept))) if *p == w.b => assert_eq!(std::fs::read(kept).unwrap(), b"{ not json"),
        other => panic!("{other:?}"),
    }
    assert_eq!(entry(&w.a, "IMG_0001.jpg").unwrap(), before, "the entry stays in the source");
    assert!(!w.b.join(".dalica.json").exists());
    assert_eq!(w.writer.read_only().collect::<Vec<_>>(), [w.fb], "nothing is written there until the user resets it");
    // A read-only folder refuses the next carry at once.
    let c = Carry::one(Mode::Move, &w.a, "IMG_0002.jpg", &w.b, "IMG_0002.jpg");
    let lib = &w.lib;
    assert!(matches!(w.writer.carry(&c, w.fa, w.fb, |f| mapping::folder_state(lib, f), at(200)).carried, Err(CarryError::ReadOnly(_))));
}

#[test]
fn damage_found_by_the_carry_itself_is_kept_aside_too() {
    let mut w = world();
    std::fs::write(w.b.join(".dalica.json"), b"[1, 2").unwrap();
    let ident = dalica_fs::identity(&w.a.join("IMG_0002.jpg")).unwrap().unwrap();
    assert!(matches!(transfer::move_file(&w.a, "IMG_0002.jpg", ident, &w.b, "IMG_0002.jpg").unwrap(), Transfer::Done(_)));
    let lib = &w.lib;
    let c = Carry::one(Mode::Move, &w.a, "IMG_0002.jpg", &w.b, "IMG_0002.jpg");
    let report = w.writer.carry(&c, w.fa, w.fb, |f| mapping::folder_state(lib, f), at(100));
    assert!(matches!(&report.carried, Err(CarryError::SetAside { kept: Some(k), .. }) if std::fs::read(k).unwrap() == b"[1, 2"), "{report:?}");
    assert!(report.writes.is_empty());
    assert_eq!(entry(&w.a, "IMG_0002.jpg").unwrap(), json!({ "geo": [38.7, -9.1] }));
    assert_eq!(w.writer.read_only().collect::<Vec<_>>(), [w.fb]);
}

#[test]
fn a_folder_that_is_not_there_is_never_created() {
    let w = world();
    let gone = w.b.join("gone");
    let r = carry::entries(&Carry::one(Mode::Move, &w.a, "IMG_0001.jpg", &gone, "IMG_0001.jpg"), at(100));
    assert!(matches!(r, Err(CarryError::Missing(p)) if p == gone));
    assert!(!gone.exists());
    assert!(entry(&w.a, "IMG_0001.jpg").is_some());
    let r = carry::backups(&Carry::one(Mode::Move, &w.a, "IMG_0001.jpg", &gone, "IMG_0001.jpg"), at(100));
    assert!(matches!(r, Err(CarryError::Missing(_))));
}

/// A folder merge (Lisbon moved onto Porto) carries every entry in one pass, renaming the one
/// whose name Porto already has, and never replaces Porto's sidecar.
#[test]
fn a_folder_merge_carries_every_entry() {
    let w = world();
    let mine: Vec<Value> = LISBON.iter().map(|n| entry(&w.a, n).unwrap_or(Value::Null)).collect();
    let porto = entry(&w.b, "IMG_0001.jpg").unwrap();
    let names: Vec<(String, String)> =
        LISBON.iter().map(|n| (n.to_string(), if *n == "IMG_0001.jpg" { "IMG_0001-001.jpg".to_string() } else { n.to_string() })).collect();
    for (old, new) in &names {
        let ident = dalica_fs::identity(&w.a.join(old)).unwrap().unwrap();
        assert!(matches!(transfer::move_file(&w.a, old, ident, &w.b, new).unwrap(), Transfer::Done(_)));
    }
    let out = carry::entries(&Carry { mode: Mode::Move, from: w.a.clone(), to: w.b.clone(), names: names.clone() }, at(100)).unwrap();
    // The third photo had no entry of its own.
    assert_eq!(out.carried.len() + out.absent.len(), 3);
    for ((_, new), want) in names.iter().zip(&mine) {
        assert_eq!(entry(&w.b, new).unwrap_or(Value::Null), *want, "{new}");
    }
    assert_eq!(entry(&w.b, "IMG_0001.jpg").unwrap(), porto);
    assert_eq!(read_json(&w.a).get("photos"), None);
    // What Lisbon said about itself stays in its own sidecar (which goes with the folder).
    assert!(w.a.join(".dalica.json").exists());
}
