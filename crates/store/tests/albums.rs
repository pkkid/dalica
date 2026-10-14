//! Albums on disk at the store level (`data-model.md` §3.2, §4, §10.12): memberships with their
//! position keys in each folder sidecar, the album names mirrored in each member folder, and the
//! definitions with a copy of the order in `albums.json`. An album over three folders keeps its
//! exact order after deleting `albums.json` and the index and rebuilding, its name recovered from
//! the folders; a damaged `albums.json` is set aside and reported.

use std::path::{Path, PathBuf};

use dalica_core::{AlbumId, Date, FolderId, PhotoId, PosKey, Timestamp};
use dalica_library::{AlbumInfo, Anchor, Folder, Library, Op, Photo};
use dalica_sidecar::albums::AlbumsFile;
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::albums::{self, AlbumsRead, AlbumsWrite};
use dalica_store::rebuild::import_library;
use dalica_store::{Outcome, SidecarWriter, Store, import_sidecars};
use serde_json::{Value, json};

const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);
const NAMES: [&str; 4] = ["IMG_0001.jpg", "IMG_0002.jpg", "a]b ;=.jpg", "東京.jpg"];

/// A scan of `roots`: one folder per root holding [`NAMES`], the same ids each time.
fn scan(roots: &[&Path]) -> (Library, Vec<FolderId>, Vec<Vec<PhotoId>>) {
    let mut lib = Library::new();
    let (mut folders, mut ids) = (Vec::new(), Vec::new());
    for root in roots {
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.to_path_buf() } }).unwrap();
        let mine: Vec<PhotoId> = NAMES.iter().map(|_| lib.next_photo_id()).collect();
        let photos = mine
            .iter()
            .zip(NAMES)
            .map(|(&id, name)| Photo {
                id,
                folder: f,
                name: name.into(),
                size: 1,
                mtime: Timestamp::EPOCH,
                width: 4,
                height: 3,
                date: None,
                star: false,
            })
            .collect();
        lib.apply(Op::AddPhotos { photos }).unwrap();
        folders.push(f);
        ids.push(mine);
    }
    (lib, folders, ids)
}

fn id(s: &str) -> AlbumId {
    AlbumId::parse(s).unwrap()
}

fn best() -> AlbumId {
    id("a91c0e000001")
}

fn trams() -> AlbumId {
    id("a77d02000002")
}

fn sidecar(dir: &Path) -> Sidecar {
    Sidecar::parse(&std::fs::read(dir.join(".dalica.json")).unwrap(), &Limits::default()).unwrap()
}

/// Two albums, one over all three folders in an order of the user's (interleaved, then a drop
/// between two members), with info and a cover; the other in one folder.
fn make_albums(lib: &mut Library, ids: &[Vec<PhotoId>]) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut run = |lib: &mut Library, op: Op| {
        lib.apply(op.clone()).unwrap();
        ops.push(op);
    };
    let info = AlbumInfo {
        name: "Best of 2026".into(),
        description: "The year in pictures".into(),
        date: Some("2026-12-31".parse::<Date>().unwrap()),
        use_newest: false,
        place: "Portugal".into(),
    };
    run(lib, Op::CreateAlbums { albums: vec![(best(), info), (trams(), AlbumInfo::named("Trams"))] });
    let order = [ids[2][1], ids[0][0], ids[1][3], ids[0][2], ids[2][0]];
    let add = lib.album_add_op(&best(), &order, None).unwrap();
    run(lib, add);
    // Dropped between the first two members.
    let drop = lib.album_add_op(&best(), &[ids[1][0]], Some(Anchor::After(ids[2][1]))).unwrap();
    run(lib, drop);
    run(lib, Op::SetCovers { covers: vec![(best(), Some(ids[1][3]))] });
    let add = lib.album_add_op(&trams(), &[ids[0][3], ids[0][1]], None).unwrap();
    run(lib, add);
    ops
}

fn flush(w: &mut SidecarWriter, lib: &Library, folders: &[FolderId]) -> Vec<Outcome> {
    for &f in folders {
        w.mark_dirty(f);
    }
    w.flush(lib, NOW).into_values().collect()
}

fn apply_all(lib: &mut Library, ops: Vec<Op>) {
    for op in ops {
        lib.apply(op).unwrap();
    }
}

/// `data-model.md` §10.12 at the store level.
#[test]
fn an_album_over_three_folders_keeps_its_order_after_losing_albums_json_and_the_index() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    let mut o = Store::open(lib_dir.path(), NOW).unwrap();
    for op in make_albums(&mut lib, &ids) {
        o.store.append(&op).unwrap();
    }
    let order = lib.album_members(&best()).to_vec();
    assert_eq!(order, [ids[2][1], ids[1][0], ids[0][0], ids[1][3], ids[0][2], ids[2][0]]);
    let mut w = SidecarWriter::new();
    assert!(flush(&mut w, &lib, &folders).iter().all(|o| *o == Outcome::Written));
    assert_eq!(albums::write(lib_dir.path(), &albums::state(&lib), NOW), AlbumsWrite::Written);

    // Each member folder carries its photos' keys and the names of their albums; only those.
    let first = sidecar(paths[0]);
    let Field::Value(names) = first.album_names() else { panic!() };
    assert_eq!(names.names.into_iter().collect::<Vec<_>>(), [(trams(), "Trams".into()), (best(), "Best of 2026".into())]);
    let Field::Value(m) = first.photo_albums("IMG_0001.jpg") else { panic!() };
    assert_eq!(m.members.get(&best()), lib.member_key(&best(), ids[0][0]));
    let Field::Value(names) = sidecar(paths[1]).album_names() else { panic!() };
    assert_eq!(names.names.keys().collect::<Vec<_>>(), [&best()]);
    // albums.json holds the definitions and a copy of the order, by path.
    let text: Value = serde_json::from_slice(&std::fs::read(lib_dir.path().join("albums.json")).unwrap()).unwrap();
    let entry = &text["albums"][best().as_str()];
    assert_eq!(entry["name"], json!("Best of 2026"));
    assert_eq!(entry["cover"], json!(paths[1].join("東京.jpg").to_str().unwrap()));
    let copied: Vec<&str> = entry["order"].as_array().unwrap().iter().map(|p| p[0].as_str().unwrap()).collect();
    let expected: Vec<String> = order.iter().map(|&p| path_of(&lib, p)).collect();
    assert_eq!(copied, expected);
    drop(o);

    // Delete the index only: the definitions, cover and order come back whole.
    for name in ["index.log", "index.snap"] {
        let _ = std::fs::remove_file(lib_dir.path().join(name));
    }
    let (mut rebuilt, _, _) = scan(&paths);
    let import = import_library(&rebuilt, lib_dir.path(), NOW);
    assert!(import.problems.is_empty() && import.library_problems.is_empty(), "{import:?}");
    apply_all(&mut rebuilt, import.ops);
    assert_eq!(rebuilt.model(), lib.model());
    assert!(import_library(&rebuilt, lib_dir.path(), NOW).ops.is_empty(), "importing again changes nothing");
    let before: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    assert!(flush(&mut w, &rebuilt, &folders).iter().all(|o| *o == Outcome::Unchanged));
    let after: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    assert_eq!(after, before);
    assert_eq!(albums::write(lib_dir.path(), &albums::state(&rebuilt), NOW), AlbumsWrite::Unchanged);

    // Delete albums.json and the index: the order and the names come back from the folders.
    std::fs::remove_file(lib_dir.path().join("albums.json")).unwrap();
    let (mut again, _, _) = scan(&paths);
    let import = import_library(&again, lib_dir.path(), NOW);
    assert!(import.problems.is_empty() && import.library_problems.is_empty(), "{import:?}");
    apply_all(&mut again, import.ops);
    assert_eq!(again.album_members(&best()), order.as_slice());
    assert_eq!(again.album_members(&trams()), lib.album_members(&trams()));
    assert_eq!(again.album(&best()), Some(&AlbumInfo::named("Best of 2026")), "description, date, place and cover start empty");
    assert_eq!(again.cover_set(&best()), None);
    for (album, members) in lib.memberships() {
        for (photo, key) in members {
            assert_eq!(again.member_key(album, *photo), Some(key));
        }
    }
    // The same through `import_sidecars`, which never reads albums.json.
    let (mut third, _, _) = scan(&paths);
    let ops = import_sidecars(&third, NOW).ops;
    apply_all(&mut third, ops);
    assert_eq!(third.model(), again.model());
}

fn path_of(lib: &Library, p: PhotoId) -> String {
    let photo = lib.photo(p).unwrap();
    lib.folder(photo.folder).unwrap().path.join(&photo.name).to_str().unwrap().to_string()
}

#[test]
fn albums_json_keeps_what_it_does_not_know_and_drops_deleted_albums() {
    let roots = [tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, _, _) = scan(&paths);
    lib.apply(Op::CreateAlbums { albums: vec![(best(), AlbumInfo::named("Best")), (trams(), AlbumInfo::named("Trams"))] }).unwrap();
    let planted = json!({
        "dalica": {"version": 1, "writer": "a future Dalica"},
        "albums": {
            best().as_str(): {"name": "Old name", "x-colour": "red"},
            "a00000000003": {"name": "Deleted elsewhere"},
            "B4D": {"name": "kept"},
            "a00000000004": {"description": "no name: kept"}
        },
        "planted": [1, 2]
    });
    std::fs::write(lib_dir.path().join("albums.json"), planted.to_string()).unwrap();
    assert_eq!(albums::write(lib_dir.path(), &albums::state(&lib), NOW), AlbumsWrite::Written);
    let v: Value = serde_json::from_slice(&std::fs::read(lib_dir.path().join("albums.json")).unwrap()).unwrap();
    assert_eq!(v["albums"][best().as_str()], json!({"name": "Best", "useNewest": true, "x-colour": "red"}));
    assert_eq!(v["albums"][trams().as_str()], json!({"name": "Trams", "useNewest": true}));
    assert!(v["albums"].get("a00000000003").is_none(), "an album the model no longer has goes");
    assert_eq!(v["albums"]["B4D"], planted["albums"]["B4D"]);
    assert_eq!(v["albums"]["a00000000004"], planted["albums"]["a00000000004"]);
    assert_eq!((&v["planted"], &v["dalica"]["writer"]), (&json!([1, 2]), &json!("a future Dalica")));
    assert_eq!(albums::write(lib_dir.path(), &albums::state(&lib), NOW), AlbumsWrite::Unchanged);

    // Reading gives the model's albums back, and reports what it does not use.
    let AlbumsRead::Read(r) = albums::read(lib_dir.path(), NOW) else { panic!() };
    assert_eq!(r.albums.keys().collect::<Vec<_>>(), [&trams(), &best()]);
    assert_eq!(r.albums[&best()].info, AlbumInfo::named("Best"));
    assert_eq!(r.notes.len(), 2, "{:?}", r.notes);
    assert_eq!(albums::read(tempfile::tempdir().unwrap().path(), NOW), AlbumsRead::Absent);
}

#[test]
fn albums_json_names_are_cleaned_and_long_text_is_cut_but_kept_in_the_file() {
    let lib_dir = tempfile::tempdir().unwrap();
    let long = "x".repeat(dalica_library::MAX_DESCRIPTION + 10);
    let planted = json!({"albums": {
        best().as_str(): {"name": "  Best\tof 2026 ", "description": long},
        trams().as_str(): {"name": " \u{7} "}
    }});
    std::fs::write(lib_dir.path().join("albums.json"), planted.to_string()).unwrap();
    let AlbumsRead::Read(r) = albums::read(lib_dir.path(), NOW) else { panic!() };
    assert_eq!(r.albums[&best()].info.name, "Best of 2026");
    assert_eq!(r.albums[&best()].info.description.len(), dalica_library::MAX_DESCRIPTION);
    assert!(!r.albums.contains_key(&trams()), "a name that is nothing once cleaned is not used");
    assert_eq!(r.notes.len(), 2, "{:?}", r.notes);
}

#[test]
fn a_damaged_albums_json_is_set_aside_and_reported_and_the_albums_come_back_from_the_folders() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    lib.apply(Op::CreateAlbums { albums: vec![(trams(), AlbumInfo::named("Trams"))] }).unwrap();
    let add = lib.album_add_op(&trams(), &[ids[1][2], ids[0][1]], None).unwrap();
    lib.apply(add).unwrap();
    let mut w = SidecarWriter::new();
    flush(&mut w, &lib, &folders);
    let file = lib_dir.path().join("albums.json");
    std::fs::write(&file, b"{\"albums\": {").unwrap();

    let (mut rebuilt, _, _) = scan(&paths);
    let import = import_library(&rebuilt, lib_dir.path(), NOW);
    assert_eq!(import.library_problems.len(), 1, "{import:?}");
    assert!(import.library_problems[0].contains("kept as"), "{:?}", import.library_problems);
    assert!(!file.exists());
    let kept: Vec<PathBuf> = std::fs::read_dir(lib_dir.path()).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].file_name().unwrap().to_str().unwrap().starts_with("albums.json.corrupt-"), "{kept:?}");
    assert_eq!(std::fs::read(&kept[0]).unwrap(), b"{\"albums\": {");
    apply_all(&mut rebuilt, import.ops);
    assert_eq!(rebuilt.model(), lib.model());

    // Writing over a damaged file keeps it aside first.
    std::fs::write(&file, b"not json").unwrap();
    let AlbumsWrite::SetAsideAndWritten { kept: Some(kept), .. } = albums::write(lib_dir.path(), &albums::state(&lib), NOW) else { panic!() };
    assert_eq!(std::fs::read(kept).unwrap(), b"not json");
    let f = AlbumsFile::parse(&std::fs::read(&file).unwrap(), &Limits::default()).unwrap();
    assert_eq!(f.albums().albums.keys().collect::<Vec<_>>(), [&trams()]);
}

#[test]
fn a_folder_whose_sidecar_is_damaged_takes_its_memberships_from_the_copy_of_the_order() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    lib.apply(Op::CreateAlbums { albums: vec![(trams(), AlbumInfo::named("Trams"))] }).unwrap();
    let add = lib.album_add_op(&trams(), &[ids[1][2], ids[0][1], ids[1][0]], None).unwrap();
    lib.apply(add).unwrap();
    let mut w = SidecarWriter::new();
    flush(&mut w, &lib, &folders);
    albums::write(lib_dir.path(), &albums::state(&lib), NOW);
    std::fs::write(paths[1].join(".dalica.json"), b"{ damaged").unwrap();

    let (mut rebuilt, _, _) = scan(&paths);
    let import = import_library(&rebuilt, lib_dir.path(), NOW);
    assert_eq!(import.read_only, [folders[1]]);
    apply_all(&mut rebuilt, import.ops);
    assert_eq!(rebuilt.album_members(&trams()), lib.album_members(&trams()));
}

#[test]
fn memberships_of_an_album_named_nowhere_are_kept_and_reported_and_bad_ones_never_used() {
    let roots = [tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let planted = json!({
        "photos": {
            "IMG_0001.jpg": {"albums": {"a00000000009": "a0", "a77d02000002": "a1", "NOT-AN-ID": "a0", "a00000000008": "!!"}},
            "IMG_0002.jpg": {"albums": {"a77d02000002": "a0"}}
        },
        "albums": {"a77d02000002": {"name": "Trams"}, "a00000000008": {"name": "Bad keys"}}
    });
    std::fs::write(paths[0].join(".dalica.json"), planted.to_string()).unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    let import = import_sidecars(&lib, NOW);
    assert_eq!(import.library_problems.len(), 1, "{import:?}");
    assert!(import.library_problems[0].contains("a00000000009"));
    assert_eq!(import.problems.len(), 2, "the two bad entries: {:?}", import.problems);
    apply_all(&mut lib, import.ops);
    assert_eq!(lib.album_members(&trams()), [ids[0][1], ids[0][0]]);
    assert_eq!(lib.album(&id("a00000000008")), Some(&AlbumInfo::named("Bad keys")), "named, so created; its bad key is not used");
    assert_eq!(lib.album_count(&id("a00000000008")), 0);
    assert!(lib.album(&id("a00000000009")).is_none());

    // Writing back keeps every entry the model could not use, byte for byte where nothing changed.
    // The name of an album without members in the folder goes; the memberships stay as written.
    let mut w = SidecarWriter::new();
    assert_eq!(flush(&mut w, &lib, &folders), [Outcome::Written]);
    let v: Value = serde_json::from_slice(&std::fs::read(paths[0].join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(v["photos"], planted["photos"]);
    assert_eq!(v["albums"], json!({"a77d02000002": {"name": "Trams"}}));
    assert_eq!(flush(&mut w, &lib, &folders), [Outcome::Unchanged]);
    let key = lib.album_add_op(&trams(), &[ids[0][2]], None).unwrap();
    lib.apply(key).unwrap();
    assert_eq!(flush(&mut w, &lib, &folders), [Outcome::Written]);
    let v: Value = serde_json::from_slice(&std::fs::read(paths[0].join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(v["photos"]["IMG_0001.jpg"], planted["photos"]["IMG_0001.jpg"]);
    assert_eq!(v["photos"]["a]b ;=.jpg"]["albums"], json!({"a77d02000002": lib.member_key(&trams(), ids[0][2]).unwrap().as_str()}));
    assert_eq!(v["albums"], json!({"a77d02000002": {"name": "Trams"}}), "names follow the members in the folder");
    let _ = PosKey::first();
}

#[test]
fn a_deleted_album_loses_its_names_in_the_folders_and_never_comes_back_from_them() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    lib.apply(Op::CreateAlbums { albums: vec![(trams(), AlbumInfo::named("Trams")), (best(), AlbumInfo::named("Best"))] }).unwrap();
    for (album, photos) in [(trams(), vec![ids[0][0], ids[1][1]]), (best(), vec![ids[1][1]])] {
        let add = lib.album_add_op(&album, &photos, None).unwrap();
        lib.apply(add).unwrap();
    }
    let mut w = SidecarWriter::new();
    flush(&mut w, &lib, &folders);
    let undo = lib.apply(Op::DeleteAlbums { ids: vec![trams()] }).unwrap();
    flush(&mut w, &lib, &folders);
    albums::write(lib_dir.path(), &albums::state(&lib), NOW);
    assert_eq!(sidecar(paths[0]).album_names(), Field::Absent);
    let Field::Value(names) = sidecar(paths[1]).album_names() else { panic!() };
    assert_eq!(names.names.keys().collect::<Vec<_>>(), [&best()]);
    for with_albums_json in [true, false] {
        if !with_albums_json {
            std::fs::remove_file(lib_dir.path().join("albums.json")).unwrap();
        }
        let (mut rebuilt, _, _) = scan(&paths);
        let import = import_library(&rebuilt, lib_dir.path(), NOW);
        apply_all(&mut rebuilt, import.ops);
        assert_eq!(rebuilt.album(&trams()), None);
        assert_eq!(rebuilt.album_members(&best()), [ids[1][1]]);
    }
    // Undo puts the album back, and writing puts its names back beside the memberships.
    lib.apply(undo).unwrap();
    flush(&mut w, &lib, &folders);
    let (mut rebuilt, _, _) = scan(&paths);
    let ops = import_sidecars(&rebuilt, NOW).ops;
    apply_all(&mut rebuilt, ops);
    assert_eq!(rebuilt.album_members(&trams()), lib.album_members(&trams()));
}
