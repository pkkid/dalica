//! M8's fields on disk (`data-model.md` §2, §3, §6, §10.1): a photo's saved record in the
//! folder's originals record (`.dalica-originals/.dalica.json`), the copy a Save wrote beside a
//! photo (`savedAs`) and a folder's project mark (`"folder": {"project": "exported"}`) in the
//! folder sidecar. Each survives the journal and a snapshot, and comes back from the folders after
//! the index is deleted; hostile names and wrong types are kept and never used; a damaged record
//! is set aside and reported.

use std::path::Path;

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo, Project, Recipe, SaveState, Saved, SavedAs};
use dalica_sidecar::originals::{self as record, Originals};
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::mapping::{self, FOLDER_DATA_M8, PHOTO_DATA_M8, PhotoState, SAVED_AS_FIELDS, SAVED_FIELDS, Source};
use dalica_store::rebuild::{OriginalsFile, read_originals};
use dalica_store::{Opened, Outcome, SidecarWriter, Store, import_sidecars, write_folder};
use proptest::prelude::*;

const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

/// Names that are hard to write (`data-model.md` §10.7), all valid file names on every platform.
fn names() -> Vec<String> {
    vec!["IMG_0001.jpg".into(), "a]b ;=.jpg".into(), "東京 タワー.CR2".into(), format!("{}.jpg", "長".repeat(60)), "IMG_0005.png".into()]
}

/// A scan of `roots`: one folder per root holding [`names`], the same ids each time.
fn scan(roots: &[&Path]) -> (Library, Vec<FolderId>, Vec<Vec<PhotoId>>) {
    let mut lib = Library::new();
    let mut folders = Vec::new();
    let mut ids = Vec::new();
    for root in roots {
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.to_path_buf() } }).unwrap();
        let mine: Vec<PhotoId> = names().iter().map(|_| lib.next_photo_id()).collect();
        let photos = mine
            .iter()
            .zip(names())
            .map(|(&id, name)| Photo { id, folder: f, name, size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false })
            .collect();
        lib.apply(Op::AddPhotos { photos }).unwrap();
        folders.push(f);
        ids.push(mine);
    }
    (lib, folders, ids)
}

fn strings(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

fn saved(recipe: &[&str], state: SaveState, n: u64) -> Saved {
    Saved {
        recipe: strings(recipe),
        size: 1000 + n,
        mtime: Timestamp::from_millis(1_780_000_000_123 + n as i64),
        width: 4000,
        height: 3000,
        digest: format!("md5:{n:032x}"),
        state,
        time: (state == SaveState::Done).then_some(Timestamp::from_millis(1_790_000_000_000)),
    }
}

/// Edits, a Save, a pending Save, a Save beside, an Exported mark: every M8 field, as the ops
/// the engine would apply.
fn change_everything(lib: &Library, folders: &[FolderId], ids: &[Vec<PhotoId>]) -> Vec<Op> {
    let (a, b) = (&ids[0], &ids[1]);
    let recipes = Op::SetRecipes {
        recipes: vec![
            (a[0], Recipe { edits: strings(&["rotate(90)", "frobnicate(7)"]), redo: Vec::new() }),
            (a[2], Recipe { edits: strings(&["sepia()"]), redo: Vec::new() }),
            (b[3], Recipe { edits: strings(&["bw()"]), redo: Vec::new() }),
        ],
    };
    let mut after = lib.clone();
    after.apply(recipes.clone()).unwrap();
    vec![
        recipes,
        after.save_op(vec![(a[0], saved(&["rotate(90)", "frobnicate(7)"], SaveState::Done, 1)), (b[1], saved(&[], SaveState::Done, 2))]).unwrap(),
        Op::SetSaved { saved: vec![(a[4], Some(saved(&["autofix()"], SaveState::Pending, 3)))] },
        after.save_beside_op(a[2], "東京 タワー.jpg".into()).unwrap(),
        Op::SetSavedAs { saved_as: vec![(b[3], Some(SavedAs { name: "x-001.jpg".into(), recipe: strings(&["bw()"]) }))] },
        Op::SetProjects { projects: vec![(folders[1], Some(Project::Exported))] },
    ]
}

fn record(o: &mut Opened, lib: &mut Library, op: &Op) {
    lib.apply(op.clone()).unwrap();
    o.store.append(op).unwrap();
}

/// Writes each folder's originals record from the model, the way a Save does (M8.12 writes it
/// through `fs`'s atomic replace; any writer gives these bytes).
fn write_records(lib: &Library, folders: &[FolderId]) {
    for &f in folders {
        let mut r = Originals::new();
        for p in lib.photos_in(f) {
            if let Some(s) = lib.saved(p.id) {
                r.set(&p.name, &mapping::originals_entry(s)).unwrap();
            }
        }
        if r.is_empty() {
            continue;
        }
        let dir = lib.folder(f).unwrap().path.join(record::DIR_NAME);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(record::FILE_NAME), r.print()).unwrap();
    }
}

fn sidecar(dir: &Path) -> Sidecar {
    Sidecar::parse(&std::fs::read(dir.join(".dalica.json")).unwrap(), &Limits::default()).unwrap()
}

/// Every M8 model field has a home; adding a field to `Saved` or `SavedAs` breaks a
/// destructuring here until it is placed in its table, and the sidecar crate reads exactly the
/// keys the tables name.
#[test]
fn every_m8_field_is_mapped() {
    let Saved { recipe: _, size: _, mtime: _, width: _, height: _, digest: _, state: _, time: _ } = Saved::default();
    let names: Vec<&str> = SAVED_FIELDS.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["recipe", "size", "mtime", "width", "height", "digest", "state", "time"]);
    assert!(SAVED_FIELDS.iter().all(|(n, s)| *s == Source::Originals(n)), "each under its own key in the originals record");
    let keys: Vec<&str> = SAVED_FIELDS
        .iter()
        .filter_map(|(_, s)| match s {
            Source::Originals(k) => Some(*k),
            _ => None,
        })
        .collect();
    assert_eq!(keys, record::ENTRY_KEYS);
    let SavedAs { name: _, recipe: _ } = SavedAs::default();
    assert_eq!(SAVED_AS_FIELDS, [("name", Source::Sidecar("savedAs")), ("recipe", Source::Sidecar("savedAs"))]);
    match Project::Exported {
        Project::Exported => assert_eq!(mapping::project_text(Project::Exported), "exported"),
    }
    assert_eq!(PHOTO_DATA_M8, [("saved", Source::Fields("SAVED_FIELDS")), ("savedAs", Source::Fields("SAVED_AS_FIELDS"))]);
    assert_eq!(FOLDER_DATA_M8, [("project", Source::FolderSidecar("project"))]);
    let photo_keys: Vec<&str> = SAVED_AS_FIELDS
        .iter()
        .filter_map(|(_, s)| match s {
            Source::Sidecar(k) => Some(*k),
            _ => None,
        })
        .take(1)
        .collect();
    assert_eq!(photo_keys, dalica_sidecar::fields::M8_PHOTO_KEYS);
    let folder_keys: Vec<&str> = FOLDER_DATA_M8
        .iter()
        .filter_map(|(_, s)| match s {
            Source::FolderSidecar(k) => Some(*k),
            _ => None,
        })
        .collect();
    assert_eq!(folder_keys, dalica_sidecar::fields::M8_FOLDER_KEYS);
    // The model's record and the file's entry convert both ways exactly.
    let s = saved(&["a()", "b()"], SaveState::Done, 9);
    assert_eq!(mapping::saved_of(mapping::originals_entry(&s)), s);
}

/// `data-model.md` §10.1 for every M8 field: scan, edit, save, save beside, mark, delete the
/// index, rebuild from the folders, compare.
#[test]
fn deleting_the_index_and_rebuilding_gives_every_m8_field_back() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    let mut o = Store::open(lib_dir.path(), NOW).unwrap();
    let mut w = SidecarWriter::new();
    for op in change_everything(&lib, &folders, &ids) {
        record(&mut o, &mut lib, &op);
    }
    write_records(&lib, &folders);
    for &f in &folders {
        w.mark_dirty(f);
    }
    let out = w.flush(&lib, NOW);
    assert!(out.values().all(|o| *o == Outcome::Written), "{out:?}");
    // savedAs in the photo's entry, the mark in "folder"; the saved records never in a sidecar.
    let first = sidecar(paths[0]);
    assert_eq!(
        first.saved_as("東京 タワー.CR2"),
        Field::Value(dalica_sidecar::SavedAs { name: "東京 タワー.jpg".into(), recipe: strings(&["sepia()"]) })
    );
    assert_eq!(first.project(), Field::Absent);
    assert_eq!(sidecar(paths[1]).project(), Field::Value("exported".into()));
    for d in &paths {
        let text = std::fs::read_to_string(d.join(".dalica.json")).unwrap();
        assert!(!text.contains("digest") && !text.contains("pending"), "{text}");
    }
    assert!(lib.can_undo_save(ids[0][0]) && !lib.can_undo_save(ids[0][4]));
    assert_eq!(lib.unsaved_count(folders[0]), 0, "a Save and a Save beside leave nothing unsaved");

    // The index (journal and snapshot) holds it all.
    o.store.snapshot(&lib).unwrap();
    drop(o);
    let reopened = Store::open(lib_dir.path(), NOW).unwrap();
    assert_eq!((reopened.report.clone(), reopened.library.model()), (vec![], lib.model()));
    drop(reopened);

    // Delete the index; a new scan finds the folders and photos with nothing said about them.
    for e in std::fs::read_dir(lib_dir.path()).unwrap() {
        std::fs::remove_file(e.unwrap().path()).unwrap();
    }
    let (mut rebuilt, _, _) = scan(&paths);
    let import = import_sidecars(&rebuilt, NOW);
    assert!(import.problems.is_empty() && import.read_only.is_empty(), "{import:?}");
    let kinds: Vec<&str> = import
        .ops
        .iter()
        .map(|op| match op {
            Op::SetPhotoInfo { .. } => "info",
            Op::SetSavedAs { .. } => "savedAs",
            Op::SetProjects { .. } => "projects",
            Op::SetSaved { .. } => "saved",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["info", "savedAs", "projects", "saved"]);
    for op in import.ops {
        let (derived, revision) = (op.is_derived(), rebuilt.revision());
        rebuilt.apply(op).unwrap();
        if derived {
            assert_eq!(rebuilt.revision(), revision, "the saved records move no revision");
        }
    }
    assert_eq!(rebuilt.model(), lib.model());
    assert_eq!(rebuilt.unsaved_counts(), lib.unsaved_counts());
    assert!(rebuilt.can_undo_save(ids[0][0]) && rebuilt.can_revert(ids[1][1]) && !rebuilt.can_undo_save(ids[0][4]));
    // Importing again finds nothing to change, and writing again changes no byte.
    assert!(import_sidecars(&rebuilt, NOW).ops.is_empty());
    let before: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    for &f in &folders {
        w.mark_dirty(f);
    }
    assert!(w.flush(&rebuilt, NOW).values().all(|o| *o == Outcome::Unchanged));
    let after: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    assert_eq!(after, before);

    // Undo Save, then the record loses its entry: a rebuild clears the record too.
    let undo = rebuilt.undo_save_op(&[ids[0][0]]).unwrap();
    rebuilt.apply(undo).unwrap();
    write_records(&rebuilt, &folders[..1]);
    let (mut again, _, _) = scan(&paths);
    for op in import_sidecars(&again, NOW).ops {
        again.apply(op).unwrap();
    }
    assert_eq!(again.saved(ids[0][0]), None);
    assert_eq!(again.saved(ids[0][4]).map(|s| s.state), Some(SaveState::Pending));
}

/// Clearing `savedAs` and the mark takes their keys out; other programs' keys stay.
#[test]
fn clearing_the_m8_fields_removes_their_keys() {
    let root = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&[root.path()]);
    lib.apply(Op::SetSavedAs { saved_as: vec![(ids[0][1], Some(SavedAs { name: "b.jpg".into(), recipe: Vec::new() }))] }).unwrap();
    lib.apply(Op::SetProjects { projects: vec![(folders[0], Some(Project::Exported))] }).unwrap();
    let state = mapping::folder_state(&lib, folders[0]).unwrap();
    assert_eq!(write_folder(&state, NOW), Outcome::Written);
    let text = std::fs::read_to_string(root.path().join(".dalica.json")).unwrap();
    let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["photos"]["a]b ;=.jpg"]["savedAs"], serde_json::json!({"name": "b.jpg"}));
    assert_eq!(doc["folder"], serde_json::json!({"project": "exported"}));
    doc["folder"]["theirs"] = serde_json::json!(1);
    std::fs::write(root.path().join(".dalica.json"), doc.to_string()).unwrap();
    lib.apply(Op::SetSavedAs { saved_as: vec![(ids[0][1], None)] }).unwrap();
    lib.apply(Op::SetProjects { projects: vec![(folders[0], None)] }).unwrap();
    let state = mapping::folder_state(&lib, folders[0]).unwrap();
    assert_eq!(write_folder(&state, NOW), Outcome::Written);
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(root.path().join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(doc["folder"], serde_json::json!({"theirs": 1}));
    assert!(doc["photos"].as_object().unwrap().is_empty(), "{doc}");
}

/// Values Dalica cannot use are kept in the files, reported, and never reach the model or a file:
/// a `savedAs` naming `../x` or `CON`, an unknown project kind, originals entries under such names
/// or of the wrong type.
#[test]
fn hostile_names_and_wrong_types_are_kept_and_never_used() {
    let root = tempfile::tempdir().unwrap();
    let d = root.path();
    let sidecar_text = r#"{"folder": {"project": "album"}, "photos": {
        "IMG_0001.jpg": {"savedAs": {"name": "../x"}, "star": true},
        "IMG_0005.png": {"savedAs": {"name": "CON"}},
        "a]b ;=.jpg": {"savedAs": ["b.jpg"]}
    }}"#;
    std::fs::write(d.join(".dalica.json"), sidecar_text).unwrap();
    let record_text = r#"{"originals": {
        "../IMG_0001.jpg": {"size": 1, "mtime": 0, "state": "done"},
        "CON": {"size": 1, "mtime": 0, "state": "done"},
        "IMG_0001.jpg": {"size": "x", "mtime": 0, "state": "done"},
        "IMG_0005.png": {"size": 1, "mtime": 0, "state": "done", "digest": 5}
    }}"#;
    std::fs::create_dir_all(d.join(".dalica-originals")).unwrap();
    std::fs::write(d.join(".dalica-originals/.dalica.json"), record_text).unwrap();
    let (mut lib, folders, ids) = scan(&[d]);
    let import = import_sidecars(&lib, NOW);
    for op in import.ops {
        lib.apply(op).unwrap();
    }
    assert!(lib.photo(ids[0][0]).unwrap().star, "the rest of the entry is read");
    assert_eq!((lib.saved_as_all().count(), lib.saved_all().count(), lib.project(folders[0])), (0, 0, None));
    let problems: Vec<&str> = import.problems.iter().map(|(_, p)| p.as_str()).collect();
    for want in
        ["plain file name", "not named by a plain file name", "IMG_0001.jpg/size", "IMG_0005.png/digest", "/photos/a]b ;=.jpg/savedAs", "\"album\""]
    {
        assert!(problems.iter().any(|p| p.contains(want)), "{want} in {problems:?}");
    }
    // A write leaves every one of them as it was, and touches neither `../x` nor the record.
    lib.apply(Op::SetStars { stars: vec![(ids[0][4], true)] }).unwrap();
    let state = mapping::folder_state(&lib, folders[0]).unwrap();
    assert_eq!(write_folder(&state, NOW), Outcome::Written);
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join(".dalica.json")).unwrap()).unwrap();
    let was: serde_json::Value = serde_json::from_str(sidecar_text).unwrap();
    assert_eq!(doc["folder"], was["folder"]);
    for name in ["IMG_0001.jpg", "a]b ;=.jpg"] {
        assert_eq!(doc["photos"][name], was["photos"][name], "{name}");
    }
    assert_eq!(doc["photos"]["IMG_0005.png"]["savedAs"], was["photos"]["IMG_0005.png"]["savedAs"]);
    assert_eq!(std::fs::read_to_string(d.join(".dalica-originals/.dalica.json")).unwrap(), record_text);
    // A user's own Save beside writes over the unusable value.
    lib.apply(Op::SetSavedAs { saved_as: vec![(ids[0][0], Some(SavedAs { name: "IMG_0001-001.jpg".into(), recipe: Vec::new() }))] }).unwrap();
    assert_eq!(write_folder(&mapping::folder_state(&lib, folders[0]).unwrap(), NOW), Outcome::Written);
    assert_eq!(sidecar(d).saved_as("IMG_0001.jpg"), Field::Value(dalica_sidecar::SavedAs { name: "IMG_0001-001.jpg".into(), recipe: Vec::new() }));
}

/// A damaged originals record is set aside as `.dalica.json.corrupt-<time>`, reported, and the
/// model keeps its records; a link in place of the folder is never followed.
#[test]
fn a_damaged_record_is_set_aside_and_reported() {
    let root = tempfile::tempdir().unwrap();
    let d = root.path();
    let (mut lib, folders, ids) = scan(&[d]);
    lib.apply(Op::SetSaved { saved: vec![(ids[0][0], Some(saved(&[], SaveState::Done, 1)))] }).unwrap();
    write_records(&lib, &folders);
    let path = d.join(".dalica-originals/.dalica.json");
    std::fs::write(&path, b"{\"originals\": [").unwrap();
    let import = import_sidecars(&lib, NOW);
    assert!(import.ops.is_empty(), "the model keeps its records: {:?}", import.ops);
    assert_eq!(import.problems.len(), 1, "{:?}", import.problems);
    assert!(import.problems[0].1.contains("kept as") && import.problems[0].1.contains(".dalica.json.corrupt-"), "{:?}", import.problems);
    assert!(import.read_only.is_empty(), "the folder's own sidecar is fine");
    assert!(!path.exists());
    let kept: Vec<String> =
        std::fs::read_dir(d.join(".dalica-originals")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].starts_with(".dalica.json.corrupt-"), "{kept:?}");
    assert_eq!(std::fs::read(d.join(".dalica-originals").join(&kept[0])).unwrap(), b"{\"originals\": [");
    // With the record gone, the next rebuild finds no record.
    assert_eq!(read_originals(d, NOW), OriginalsFile::Absent);
    #[cfg(unix)]
    {
        let other = tempfile::tempdir().unwrap();
        std::fs::remove_dir_all(d.join(".dalica-originals")).unwrap();
        write_records(&lib, &folders);
        std::fs::rename(d.join(".dalica-originals"), other.path().join("kept")).unwrap();
        std::os::unix::fs::symlink(other.path().join("kept"), d.join(".dalica-originals")).unwrap();
        assert!(matches!(read_originals(d, NOW), OriginalsFile::Unreadable(_)));
        let import = import_sidecars(&lib, NOW);
        assert!(import.ops.is_empty() && import.problems.len() == 1, "{import:?}");
    }
}

/// Journal replay and snapshots keep every M8 op: the saved records (derived), copies saved
/// beside and project marks, and their clearing.
#[test]
fn journal_replay_and_snapshots_keep_every_m8_field() {
    let dir = tempfile::tempdir().unwrap();
    let (lib0, folders, ids) = scan(&[Path::new("/photos/lisbon"), Path::new("/photos/porto")]);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    let mut lib = Library::new();
    for f in lib0.folders() {
        record(&mut o, &mut lib, &Op::AddFolder { folder: f.clone() });
        record(&mut o, &mut lib, &Op::AddPhotos { photos: lib0.photos_in(f.id).into_iter().cloned().collect() });
    }
    let changes = change_everything(&lib, &folders, &ids);
    let (first, rest) = changes.split_at(3);
    for op in first {
        record(&mut o, &mut lib, op);
    }
    let mid = lib.clone();
    o.store.snapshot(&lib).unwrap();
    for op in rest {
        record(&mut o, &mut lib, op);
    }
    let revert = lib.revert_op(&[ids[1][1]]).unwrap();
    record(&mut o, &mut lib, &revert);
    record(&mut o, &mut lib, &Op::SetProjects { projects: vec![(folders[0], Some(Project::Exported)), (folders[1], None)] });
    drop(o);
    let replayed = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((replayed.report.clone(), replayed.library.model()), (vec![], lib.model()));
    assert_eq!(replayed.library.saved(ids[0][4]).map(|s| s.state), Some(SaveState::Pending));
    assert_eq!(replayed.library.project(folders[0]), Some(Project::Exported));
    let mut o = replayed;
    o.store.snapshot(&o.library).unwrap();
    drop(o);
    let snapped = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((snapped.report.clone(), snapped.library.model()), (vec![], lib.model()));
    assert_eq!(snapped.store.seq(), snapped.store.snapshot_seq());
    let snap = std::fs::read_to_string(dir.path().join(dalica_store::snapshot::FILE)).unwrap();
    assert!(snap.contains("\"saved\"") && snap.contains("\"savedAs\"") && snap.contains("\"projects\""), "{snap}");
    // The snapshot taken half way holds the model as it was then.
    let p = dir.path().join("mid.snap");
    dalica_store::snapshot::write(&p, &mid, 9).unwrap();
    let (back, _) = dalica_store::snapshot::read(&std::fs::read(&p).unwrap()).unwrap();
    assert_eq!(back.model(), mid.model());
}

/// A photo deleted outside Dalica whose entry holds only `savedAs` is Dalica's entry like a starred
/// one (`data-model.md` §7): marked at the folder's next write, kept for 30 days, pruned by the
/// first write after them; the copy itself is another photo and keeps its own entry.
#[test]
fn an_entry_with_only_saved_as_is_marked_and_pruned_when_its_file_is_gone() {
    use dalica_store::stale::MISSING_KEY;
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::new();
    let f = lib.next_folder_id();
    lib.apply(Op::AddFolder { folder: Folder { id: f, path: dir.path().to_path_buf() } }).unwrap();
    let mut ids = Vec::new();
    for name in ["a.CR2", "a.jpg"] {
        std::fs::write(dir.path().join(name), name).unwrap();
        let id = lib.next_photo_id();
        let photo = Photo { id, folder: f, name: name.into(), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false };
        lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();
        ids.push(id);
    }
    lib.apply(Op::SetSavedAs { saved_as: vec![(ids[0], Some(SavedAs { name: "a.jpg".into(), recipe: Vec::new() }))] }).unwrap();
    lib.apply(Op::SetStars { stars: vec![(ids[1], true)] }).unwrap();
    let mut w = SidecarWriter::new();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, NOW)[&f], Outcome::Written);

    std::fs::remove_file(dir.path().join("a.CR2")).unwrap();
    lib.apply(Op::RemovePhotos { ids: vec![ids[0]] }).unwrap();
    w.mark_dirty(f);
    assert_eq!(w.flush(&lib, NOW)[&f], Outcome::Written, "the mark is written");
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.path().join(".dalica.json")).unwrap()).unwrap();
    assert!(doc["dalica"][MISSING_KEY]["a.CR2"].is_string(), "{doc}");
    assert_eq!(doc["photos"]["a.CR2"]["savedAs"]["name"], "a.jpg", "the entry is kept while it waits");

    let day = std::time::Duration::from_secs(24 * 60 * 60);
    for (d, on) in [(29, false), (31, true)] {
        lib.apply(Op::SetStars { stars: vec![(ids[1], on)] }).unwrap();
        w.mark_dirty(f);
        assert_eq!(w.flush(&lib, NOW.saturating_add(day * d))[&f], Outcome::Written);
    }
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.path().join(".dalica.json")).unwrap()).unwrap();
    assert!(doc["photos"].get("a.CR2").is_none(), "{doc}");
    assert!(doc["dalica"].get(MISSING_KEY).is_none(), "{doc}");
    assert_eq!(doc["photos"]["a.jpg"]["star"], true);
}

fn saved_as_value() -> impl Strategy<Value = Option<SavedAs>> {
    proptest::option::of(
        ("IMG_[0-9]{1,6}( [a-z東]{1,4})?\\.jpg", proptest::collection::vec("\\PC{0,8}", 0..3)).prop_map(|(name, recipe)| SavedAs { name, recipe }),
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// The model's `savedAs` and project mark written into any sidecar read back exactly, and
    /// every other key keeps its value.
    #[test]
    fn m8_sidecar_fields_round_trip(
        photos in proptest::collection::vec(("\\PC{1,16}|[\\]\";=.]{1,6}", any::<bool>(), saved_as_value()), 0..6),
        project in proptest::option::of(Just(Project::Exported)),
        planted in proptest::option::of("\\PC{0,8}"),
    ) {
        let mut sidecar = Sidecar::new();
        if let Some(p) = &planted {
            let text = format!("{{\"planted\": {}}}", serde_json::Value::String(p.clone()));
            sidecar = Sidecar::parse(text.as_bytes(), &Limits::default()).unwrap();
        }
        let mut seen = std::collections::HashSet::new();
        let photos: Vec<(String, PhotoState)> = photos
            .into_iter()
            .filter(|(n, ..)| seen.insert(n.clone()))
            .map(|(n, star, saved_as)| (n, PhotoState { star, saved_as, ..PhotoState::default() }))
            .collect();
        let state = dalica_store::FolderState { id: FolderId::new(1), path: "/x".into(), folder: Default::default(), photos: photos.clone(), project };
        prop_assert!(mapping::apply(&state, &mut sidecar).is_empty());
        let back = Sidecar::parse(&sidecar.print(), &Limits::default()).unwrap();
        prop_assert_eq!(&back, &sidecar);
        prop_assert!(back.problems().is_empty(), "{:?}", back.problems());
        prop_assert_eq!(mapping::read_folder(&back).project, mapping::Reading::Usable(project));
        for (name, want) in &photos {
            let got = mapping::read_photo(&back, name).merged(&PhotoState::default());
            prop_assert_eq!(&got, want);
        }
        if let Some(p) = &planted {
            let doc: serde_json::Value = serde_json::from_slice(&back.print()).unwrap();
            prop_assert_eq!(&doc["planted"], &serde_json::Value::String(p.clone()));
        }
    }

    /// A saved record written into a record file and read back is the same record.
    #[test]
    fn saved_records_round_trip_through_the_record(
        recipe in proptest::collection::vec("\\PC{0,12}", 0..4),
        size in any::<u64>(),
        mtime in any::<i64>(),
        width in any::<u32>(),
        height in any::<u32>(),
        digest in "[a-z0-9]{0,8}:[0-9a-f]{0,64}",
        done in any::<bool>(),
        time in proptest::option::of(any::<i64>()),
    ) {
        let s = Saved {
            recipe,
            size,
            mtime: Timestamp::from_millis(mtime),
            width,
            height,
            digest,
            state: if done { SaveState::Done } else { SaveState::Pending },
            time: time.map(Timestamp::from_millis),
        };
        let mut r = Originals::new();
        r.set("IMG_0001.jpg", &mapping::originals_entry(&s)).unwrap();
        let back = Originals::parse(&r.print(), &Limits::default()).unwrap();
        let reading = mapping::read_originals(&back);
        prop_assert!(reading.notes.is_empty(), "{:?}", reading.notes);
        prop_assert_eq!(reading.saved, vec![("IMG_0001.jpg".to_string(), s)]);
    }
}
