//! Every field Dalica keeps, M6's, M8's, M9's and M13's together, through each way the store keeps
//! it (`data-model.md` §3, §4, §6, §7, §10.1): a rebuild from the folders after the index is deleted
//! gives back everything but the derived data (synced hashes, search facts, digests and times);
//! journal replay and snapshots keep everything; clearing a field takes its keys out; and an entry
//! holding only Dalica's fields is marked and pruned like a starred one when its file is gone.
//! The milestone files (`m6_fields.rs`, `m8_fields.rs`, `m9_fields.rs`) keep what is particular
//! to their fields.

use std::path::Path;

use dalica_core::geo::LatLon;
use dalica_core::{AlbumId, FolderId, PersonId, PhotoId, Rect01, Timestamp};
use dalica_library::{
    AlbumInfo, ContentDigest, Face, FaceKey, FacePerson, Facts, Folder, FolderInfo, Geo, Library, Op, Person, Photo, PhotoInfo, PhotoSort, Project,
    Recipe, SaveState, Saved, SavedAs, SortBy, Synced,
};
use dalica_sidecar::fields::{M13_PHOTO_KEYS, M13_TOP_KEYS};
use dalica_sidecar::originals::{self as record, Originals};
use dalica_sidecar::{Field, Limits, Sidecar};
use dalica_store::mapping::{FACE_FIELDS, FOLDER_DATA_M13, FolderState, LIBRARY_DATA_M13, PERSON_FIELDS, PHOTO_DATA_M13, PhotoState, Source};
use dalica_store::rebuild::import_library;
use dalica_store::{Opened, Outcome, SidecarWriter, Store, albums, mapping, people, write_folder};
use serde_json::{Value, json};

use crate::common::{NOW, strings};

/// Names that are hard to write (`data-model.md` §10.7). Windows cannot name a file with `"`, but
/// these only ever reach the model and the sidecar.
fn names() -> Vec<String> {
    vec!["IMG_0001.jpg".into(), "a]b \"c\";=.jpg".into(), "東京 タワー.CR2".into(), format!("{}.jpg", "長".repeat(80)), "IMG_0005.png".into()]
}

/// A scan of `roots`: one folder per root holding [`names`], every photo with the same ids each
/// time (so a rebuilt model can be compared with the first).
fn scan(roots: &[&Path]) -> (Library, Vec<FolderId>, Vec<Vec<PhotoId>>) {
    let mut lib = Library::new();
    let (mut folders, mut ids) = (Vec::new(), Vec::new());
    for root in roots {
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: root.to_path_buf() } }).unwrap();
        let mine: Vec<PhotoId> = names().iter().map(|_| lib.next_photo_id()).collect();
        let photos = mine
            .iter()
            .zip(names())
            .map(|(&id, name)| Photo {
                id,
                folder: f,
                name,
                size: 10,
                mtime: Timestamp::from_millis(5),
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

fn dt(s: &str) -> dalica_core::DateTime {
    s.parse().unwrap()
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

fn best() -> AlbumId {
    AlbumId::parse("a91c0e0000aa").unwrap()
}

fn trams() -> AlbumId {
    AlbumId::parse("a77d020000bb").unwrap()
}

fn ana() -> PersonId {
    PersonId::parse("p7f3a2c00001").unwrap()
}

fn rui() -> PersonId {
    PersonId::parse("p0b1d0000002").unwrap()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
    Rect01::new(x0, y0, x1, y1).unwrap()
}

fn at(lat: f64, lon: f64) -> Option<Geo> {
    Some(Geo::At(LatLon::new(lat, lon).unwrap()))
}

/// M13's ops: two people (one with a nickname), named, ignored and unnamed faces over both
/// folders (stamped like the engine's), a thumbnail, two places and a location removed.
fn m13_ops(lib: &Library, ids: &[Vec<PhotoId>]) -> Vec<Op> {
    let (a, b) = (&ids[0], &ids[1]);
    let faces = Op::SetFaces {
        faces: vec![
            (
                a[0],
                vec![
                    Face::new(rect(0.41, 0.22, 0.52, 0.38), FacePerson::Person(ana())),
                    Face::new(rect(0.7, 0.3, 0.78, 0.44), FacePerson::Ignored),
                    Face::new(rect(0.1, 0.1, 0.2, 0.2), FacePerson::Unnamed),
                ],
            ),
            (
                b[2],
                vec![
                    Face::new(rect(0.123457, 0.25, 0.5, 1.0), FacePerson::Person(ana())),
                    Face::new(rect(0.6, 0.1, 0.9, 0.5), FacePerson::Person(rui())),
                ],
            ),
            (b[4], vec![Face::new(Rect01::FULL, FacePerson::Unnamed)]),
        ],
    };
    let mut people = lib.clone();
    let add = [
        Op::AddPerson { id: ana(), person: Person { name: "Ana Costa".into(), nickname: "Aninhas".into(), thumbnail: None } },
        Op::AddPerson { id: rui(), person: Person::named("Rui \"R\" ; Lopes 東京") },
    ];
    for op in &add {
        people.apply(op.clone()).unwrap();
    }
    let faces = people.stamped(faces, Timestamp::from_millis(1_790_000_000_000));
    let geo = Op::SetGeo { geo: vec![(a[0], at(38.7116, -9.1389)), (a[1], Some(Geo::Removed)), (b[3], at(-33.856784, 151.215297))] };
    let thumbnail = Op::SetPersonThumbnail { id: ana(), thumbnail: Some(FaceKey { photo: b[2], rect: rect(0.123457, 0.25, 0.5, 1.0) }) };
    let mut out = add.to_vec();
    out.extend([faces, geo, thumbnail]);
    out
}

/// M6's user fields: stars, captions, tags, hidden photos, recipes with an unknown operation and a
/// redo list, dates, a folder's description, date and place, a hidden folder; and synced hashes.
fn m6_ops(folders: &[FolderId], ids: &[Vec<PhotoId>]) -> Vec<Op> {
    let (a, b) = (&ids[0], &ids[1]);
    vec![
        Op::SetStars { stars: vec![(a[0], true), (b[2], true)] },
        Op::SetCaptions { captions: vec![(a[0], "Tram 28 at sunset".into()), (a[1], "Ein \"Bild\" ; = ] 東京".into()), (b[3], "x".into())] },
        Op::AddTags { ids: vec![a[0], a[1], b[0]], tags: strings(&["lisbon", "tram"]) },
        Op::AddTags { ids: vec![a[3]], tags: strings(&["a/b", "c,d", "Sunset"]) },
        Op::SetHidden { hidden: vec![(a[2], true), (b[4], true)] },
        Op::SetRecipes {
            recipes: vec![
                (a[1], Recipe { edits: strings(&["rotate(90)", "frobnicate(7)", "crop(0.1,0.1,0.9,0.9)"]), redo: strings(&["sepia()"]) }),
                (b[1], Recipe { edits: strings(&["flip_h()"]), redo: Vec::new() }),
            ],
        },
        Op::SetDates { dates: vec![(a[1], Some(dt("2026-06-04T19:42:10"))), (b[0], Some(dt("1999-12-31T23:59:59")))] },
        Op::SetFolderInfo {
            folders: vec![(
                folders[0],
                FolderInfo {
                    description: "Weekend in Lisbon with the family".into(),
                    date: Some("2026-06-04".parse().unwrap()),
                    place: "Lisbon, Portugal".into(),
                    hidden: false,
                },
            )],
        },
        Op::SetFoldersHidden { hidden: vec![(folders[1], true)] },
        Op::SetSynced { synced: vec![(a[0], Some(Synced { caption: Some(7), tags: Some(8), ..Synced::default() }))] },
    ]
}

/// Every field Dalica keeps, as the ops the engine would apply, each applied to `lib` as it is made
/// (M8's and M9's ops are built from the library as it is then): M6's ([`m6_ops`]); M8's edits, a
/// Save, a pending Save, a Save beside, a copy saved beside and an Exported mark; M9's sorts,
/// albums over both folders with a cover (the adds stamped with their last-changed time), search
/// facts, a digest and first-indexed times.
fn change_everything(lib: &mut Library, folders: &[FolderId], ids: &[Vec<PhotoId>]) -> Vec<Op> {
    let (a, b) = (&ids[0], &ids[1]);
    let mut out = Vec::new();
    let mut run = |lib: &mut Library, op: Op| {
        lib.apply(op.clone()).unwrap();
        out.push(op);
    };
    for op in m6_ops(folders, ids) {
        run(lib, op);
    }
    // M8.
    run(
        lib,
        Op::SetRecipes {
            recipes: vec![
                (a[0], Recipe { edits: strings(&["rotate(90)", "frobnicate(7)"]), redo: Vec::new() }),
                (a[2], Recipe { edits: strings(&["sepia()"]), redo: Vec::new() }),
                (b[3], Recipe { edits: strings(&["bw()"]), redo: Vec::new() }),
            ],
        },
    );
    let save =
        lib.save_op(vec![(a[0], saved(&["rotate(90)", "frobnicate(7)"], SaveState::Done, 1)), (b[1], saved(&[], SaveState::Done, 2))]).unwrap();
    run(lib, save);
    run(lib, Op::SetSaved { saved: vec![(a[4], Some(saved(&["autofix()"], SaveState::Pending, 3)))] });
    let beside = lib.save_beside_op(a[2], "東京 タワー.jpg".into()).unwrap();
    run(lib, beside);
    run(lib, Op::SetSavedAs { saved_as: vec![(b[3], Some(SavedAs { name: "x-001.jpg".into(), recipe: strings(&["bw()"]) }))] });
    run(lib, Op::SetProjects { projects: vec![(folders[1], Some(Project::Exported))] });
    // M9.
    run(lib, Op::SetFolderSorts { sorts: vec![(folders[0], PhotoSort::new(SortBy::Name, true)), (folders[1], PhotoSort::new(SortBy::Size, false))] });
    run(
        lib,
        Op::CreateAlbums {
            albums: vec![
                (best(), AlbumInfo { name: "Best of 2026".into(), description: "d".into(), date: None, use_newest: true, place: "Lisbon".into() }),
                (trams(), AlbumInfo { name: "Trams".into(), date: Some("2026-06-04".parse().unwrap()), ..AlbumInfo::default() }),
            ],
        },
    );
    for (album, photos) in [(best(), vec![b[2], a[0], a[3], b[0]]), (trams(), vec![a[2], b[2]])] {
        let add = lib.album_add_op(&album, &photos, None).unwrap();
        let add = lib.stamped(add, Timestamp::from_millis(1_790_000_000_000));
        run(lib, add);
    }
    run(lib, Op::SetCovers { covers: vec![(best(), Some(a[3]))] });
    run(lib, Op::SetFacts { facts: vec![(a[0], Facts { make: "Canon".into(), model: "EOS R5".into(), video: false, geotagged: true })] });
    run(
        lib,
        Op::SetDigests { digests: vec![(b[1], Some(ContentDigest { size: 10, mtime: Timestamp::from_millis(5), digest: "blake3:00ff".into() }))] },
    );
    let indexed = lib.first_indexed_op(&[a[0], a[1], b[4]], Timestamp::from_millis(1_780_000_000_000));
    run(lib, indexed);
    // M13.
    for op in m13_ops(lib, ids) {
        run(lib, op);
    }
    out
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

fn json_of(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(".dalica.json")).unwrap()).unwrap()
}

/// The library without its derived data (index-only: a rebuild starts without it): the synced
/// hashes, search facts, content digests and first-indexed times. The last-changed times are in
/// the sidecars (M10) and come back.
fn without_derived(lib: &Library) -> Library {
    let mut l = lib.clone();
    let synced = l.synced_all().map(|(id, _)| (id, None)).collect();
    let facts = l.facts_all().map(|(id, _)| (id, Facts::default())).collect();
    let digests = l.digests().map(|(id, _)| (id, None)).collect();
    let indexed = l.first_indexed_all().map(|(id, _)| (id, None)).collect();
    l.apply(Op::Batch { ops: vec![Op::SetSynced { synced }, Op::SetFacts { facts }, Op::SetDigests { digests }, Op::SetFirstIndexed { indexed }] })
        .unwrap();
    l
}

/// `data-model.md` §10.1 for every field: scan, change everything, write the sidecars, the
/// originals records and `albums.json`, delete the index, rebuild from the folders, compare.
#[test]
fn deleting_the_index_and_rebuilding_gives_every_field_back() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let paths: Vec<&Path> = roots.iter().map(|d| d.path()).collect();
    let lib_dir = tempfile::tempdir().unwrap();
    let (mut lib, folders, ids) = scan(&paths);
    let mut o = Store::open(lib_dir.path(), NOW).unwrap();
    o.store.snapshot(&lib).unwrap();
    for op in change_everything(&mut lib, &folders, &ids) {
        o.store.append(&op).unwrap();
    }
    assert!(lib.can_undo_save(ids[0][0]) && !lib.can_undo_save(ids[0][4]));
    assert_eq!(lib.unsaved_count(folders[0]), 1, "a Save and a Save beside leave only the photo never saved (M6's recipe)");
    assert!(lib.last_changed(ids[0][0]).is_some() && lib.first_indexed(ids[1][4]).is_some());

    // The index (journal and snapshot) holds it all.
    drop(o);
    let mut reopened = Store::open(lib_dir.path(), NOW).unwrap();
    assert_eq!((reopened.report.clone(), reopened.library.model()), (vec![], lib.model()));
    reopened.store.snapshot(&lib).unwrap();
    drop(reopened);
    let reopened = Store::open(lib_dir.path(), NOW).unwrap();
    assert_eq!((reopened.report.clone(), reopened.library.model()), (vec![], lib.model()));
    drop(reopened);

    // The sidecars, the originals records and albums.json.
    write_records(&lib, &folders);
    let mut w = SidecarWriter::new();
    for &f in &folders {
        w.mark_dirty(f);
    }
    let out = w.flush(&lib, NOW);
    assert!(out.values().all(|o| *o == Outcome::Written), "{out:?}");
    assert_eq!(albums::write(lib_dir.path(), &albums::state(&lib), NOW), albums::AlbumsWrite::Written);
    assert_eq!(people::write(lib_dir.path(), &people::state(&lib), NOW), people::PeopleWrite::Written);
    let (first, second) = (sidecar(paths[0]), sidecar(paths[1]));
    assert_eq!(second.folder().hidden, Field::Value(true), "a hidden folder says so in its own sidecar");
    assert_eq!(
        first.saved_as("東京 タワー.CR2"),
        Field::Value(dalica_sidecar::SavedAs { name: "東京 タワー.jpg".into(), recipe: strings(&["sepia()"]) })
    );
    assert_eq!((first.project(), second.project()), (Field::Absent, Field::Value("exported".into())));
    assert_eq!((first.folder_sort(), first.folder_reverse()), (Field::Value("name".into()), Field::Value(true)));
    assert_eq!((second.folder_sort(), second.folder_reverse()), (Field::Value("size".into()), Field::Absent));
    // Derived data and the saved records never reach a sidecar; the last-changed times do (M10).
    for d in &paths {
        let text = std::fs::read_to_string(d.join(".dalica.json")).unwrap();
        for derived in ["synced", "rating", "pending", "Canon", "geotagged", "blake3", "digest", "1780000000000", "1790000000000", "indexed"] {
            assert!(!text.contains(derived), "{derived} in {text}");
        }
    }
    let stamped = lib.photo(ids[0][0]).unwrap().name.clone();
    assert_eq!(first.photo(&stamped).changed, Field::Value(Timestamp::from_millis(1_790_000_000_000)));
    // M13: faces, places (a removed one as null) and each folder's people.
    let json = json_of(paths[0]);
    let entry = |n: usize| &json["photos"][lib.photo(ids[0][n]).unwrap().name.as_str()];
    assert_eq!(entry(0)["faces"][0], json!({"person": "p7f3a2c00001", "rect": [0.41, 0.22, 0.52, 0.38]}));
    assert_eq!(entry(0)["faces"][2], json!({"rect": [0.1, 0.1, 0.2, 0.2]}), "an unnamed face has no person");
    assert_eq!((&entry(0)["geo"], &entry(1)["geo"]), (&json!([38.7116, -9.1389]), &Value::Null));
    assert_eq!(json["people"], json!({"p7f3a2c00001": {"name": "Ana Costa", "nickname": "Aninhas"}}), "only the people in the folder");
    assert_eq!(json_of(paths[1])["people"].as_object().map(|p| p.len()), Some(2));

    // Delete the index; a new scan finds the same folders and photos with nothing said about them.
    for e in std::fs::read_dir(lib_dir.path()).unwrap() {
        let p = e.unwrap().path();
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("index.")) {
            std::fs::remove_file(p).unwrap();
        }
    }
    let (mut rebuilt, _, _) = scan(&paths);
    let import = import_library(&rebuilt, lib_dir.path(), NOW);
    assert!(import.problems.is_empty() && import.library_problems.is_empty() && import.read_only.is_empty(), "{import:?}");
    // One op per kind of data, the user's only (the derived data is not in any file).
    let kinds: Vec<String> = import.ops.iter().map(|op| serde_json::to_value(op).unwrap()["op"].as_str().unwrap_or("?").to_string()).collect();
    let want = [
        "setStars",
        "setPhotoInfo",
        "setFolderInfo",
        "setSavedAs",
        "setProjects",
        "setSaved",
        "setFolderSorts",
        "createAlbums",
        "addMembers",
        "setCovers",
        "batch",
        "setFaces",
        "setGeo",
        "batch",
        "setLastChanged",
    ];
    assert_eq!(kinds, want);
    for op in import.ops {
        let (derived, revision) = (op.is_derived(), rebuilt.revision());
        rebuilt.apply(op).unwrap();
        if derived {
            assert_eq!(rebuilt.revision(), revision, "the saved records move no revision");
        }
    }
    assert_eq!(rebuilt.model(), without_derived(&lib).model());
    assert_eq!(rebuilt.tag_index().counts(), lib.tag_index().counts());
    assert_eq!(rebuilt.unsaved_counts(), lib.unsaved_counts());
    assert!(rebuilt.is_folder_hidden(folders[1]) && rebuilt.is_hidden(ids[0][2]));
    assert_eq!((rebuilt.geo(ids[0][1]), rebuilt.is_geotagged(ids[0][1])), (Some(Geo::Removed), false), "a removed location stays removed");
    assert_eq!(rebuilt.person(&ana()).and_then(|p| p.thumbnail), Some(FaceKey { photo: ids[1][2], rect: rect(0.123457, 0.25, 0.5, 1.0) }));
    assert!(rebuilt.can_undo_save(ids[0][0]) && rebuilt.can_revert(ids[1][1]) && !rebuilt.can_undo_save(ids[0][4]));
    // Importing again finds nothing to change, and writing again changes no byte.
    assert!(import_library(&rebuilt, lib_dir.path(), NOW).ops.is_empty(), "importing again changes nothing");
    let before: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    for &f in &folders {
        w.mark_dirty(f);
    }
    assert!(w.flush(&rebuilt, NOW).values().all(|o| *o == Outcome::Unchanged));
    let after: Vec<Vec<u8>> = paths.iter().map(|d| std::fs::read(d.join(".dalica.json")).unwrap()).collect();
    assert_eq!(after, before, "a file that already says what the model says keeps its bytes");

    // Undo Save, then the record loses its entry: a rebuild clears the record too.
    let undo = rebuilt.undo_save_op(&[ids[0][0]]).unwrap();
    rebuilt.apply(undo).unwrap();
    write_records(&rebuilt, &folders[..1]);
    let (mut again, _, _) = scan(&paths);
    for op in import_library(&again, lib_dir.path(), NOW).ops {
        again.apply(op).unwrap();
    }
    assert_eq!(again.saved(ids[0][0]), None);
    assert_eq!(again.saved(ids[0][4]).map(|s| s.state), Some(SaveState::Pending));
}

/// Journal replay and snapshots keep every op and every field, the derived ones included; a
/// snapshot taken half way holds the model as it was then; a torn record loses only itself.
#[test]
fn journal_replay_and_snapshots_keep_every_field() {
    let dir = tempfile::tempdir().unwrap();
    let (lib0, folders, ids) = scan(&[Path::new("/photos/lisbon"), Path::new("/photos/porto")]);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    // The scan's own ops first, so the journal holds the whole history.
    let mut lib = Library::new();
    for f in lib0.folders() {
        record(&mut o, &mut lib, &Op::AddFolder { folder: f.clone() });
        record(&mut o, &mut lib, &Op::AddPhotos { photos: lib0.photos_in(f.id).into_iter().cloned().collect() });
    }
    let changes = change_everything(&mut lib.clone(), &folders, &ids);
    let (first, rest) = changes.split_at(changes.len() / 2);
    for op in first {
        record(&mut o, &mut lib, op);
    }
    let mid = lib.clone();
    o.store.snapshot(&lib).unwrap();
    for op in rest {
        record(&mut o, &mut lib, op);
    }
    // A rename merges tags; a whole info and the synced hashes are set the way rebuilds set them; a
    // Revert; a project mark moves.
    let revert = lib.revert_op(&[ids[1][1]]).unwrap();
    for op in [
        Op::RenameTag { from: "tram".into(), to: "Lisbon".into() },
        Op::SetPhotoInfo { photos: vec![(ids[1][4], PhotoInfo { caption: "whole".into(), hidden: true, ..PhotoInfo::EMPTY })] },
        Op::SetSynced { synced: vec![(ids[1][1], Some(Synced { rating: Some(1), place: Some(2), date: Some(3), ..Synced::default() }))] },
        revert,
        Op::SetProjects { projects: vec![(folders[0], Some(Project::Exported)), (folders[1], None)] },
    ] {
        record(&mut o, &mut lib, &op);
    }
    drop(o);
    let replayed = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((replayed.report.clone(), replayed.library.model()), (vec![], lib.model()));
    assert_eq!(replayed.library.synced(ids[0][0]), Some(&Synced { caption: Some(7), tags: Some(8), ..Synced::default() }));
    assert_eq!(replayed.library.tag_index().counts(), lib.tag_index().counts());
    assert_eq!(replayed.library.saved(ids[0][4]).map(|s| s.state), Some(SaveState::Pending));
    assert_eq!(replayed.library.project(folders[0]), Some(Project::Exported));
    let mut o = replayed;
    o.store.snapshot(&o.library).unwrap();
    drop(o);
    let snapped = Store::open(dir.path(), NOW).unwrap();
    assert_eq!((snapped.report.clone(), snapped.library.model()), (vec![], lib.model()));
    assert_eq!(snapped.store.seq(), snapped.store.snapshot_seq());
    let snap = std::fs::read_to_string(dir.path().join(dalica_store::snapshot::FILE)).unwrap();
    for key in ["\"saved\"", "\"savedAs\"", "\"projects\"", "\"facts\"", "\"digests\"", "\"people\"", "\"faces\"", "\"geo\""] {
        assert!(snap.contains(key), "{key} in {snap}");
    }
    // The snapshot taken half way holds the model as it was then.
    let p = dir.path().join("mid.snap");
    dalica_store::snapshot::write(&p, &mid, 9).unwrap();
    let (back, _) = dalica_store::snapshot::read(&std::fs::read(&p).unwrap()).unwrap();
    assert_eq!(back.model(), mid.model());
    // A torn last record loses only that op.
    drop(snapped);
    let mut o = Store::open(dir.path(), NOW).unwrap();
    let before = o.library.clone();
    o.store.append(&Op::SetCaptions { captions: vec![(ids[0][4], "torn".into())] }).unwrap();
    drop(o);
    let log = dir.path().join(dalica_store::journal::FILE);
    let bytes = std::fs::read(&log).unwrap();
    std::fs::write(&log, &bytes[..bytes.len() - 3]).unwrap();
    let torn = Store::open(dir.path(), NOW).unwrap();
    assert_eq!(torn.library.model(), before.model());
}

/// Clearing every field takes its keys out again, and a folder left with nothing to say keeps
/// only what other programs put there.
#[test]
fn clearing_every_field_removes_its_keys() {
    let root = tempfile::tempdir().unwrap();
    let planted = json!({"planted": 1, "folder": {"theirs": 1}, "photos": {"IMG_0001.jpg": {"x-other": true}}});
    std::fs::write(root.path().join(".dalica.json"), planted.to_string()).unwrap();
    let (mut lib, folders, ids) = scan(&[root.path()]);
    let (f, a) = (folders[0], &ids[0]);
    let mut ops = m6_ops(&[f, f], &[a.clone(), a.clone()]);
    ops.extend([
        Op::SetSavedAs { saved_as: vec![(a[1], Some(SavedAs { name: "b.jpg".into(), recipe: Vec::new() }))] },
        Op::SetProjects { projects: vec![(f, Some(Project::Exported))] },
        Op::SetFolderSorts { sorts: vec![(f, PhotoSort::new(SortBy::Name, true))] },
        Op::AddPerson { id: ana(), person: Person::named("Ana") },
        Op::SetFaces { faces: vec![(a[0], vec![Face::new(Rect01::FULL, FacePerson::Person(ana()))])] },
        Op::SetGeo { geo: vec![(a[0], Some(Geo::Removed)), (a[2], at(1.0, 2.0))] },
    ]);
    let mut undo = Vec::new();
    for op in ops {
        undo.push(lib.apply(op).unwrap());
    }
    assert_eq!(write_folder(&mapping::folder_state(&lib, f).unwrap(), NOW), Outcome::Written);
    let doc = json_of(root.path());
    assert_eq!(doc["photos"]["a]b \"c\";=.jpg"]["savedAs"], json!({"name": "b.jpg"}));
    assert_eq!(doc["folder"]["project"], "exported");
    assert_eq!((&doc["folder"]["sort"], &doc["folder"]["reverse"]), (&json!("name"), &json!(true)));
    assert_eq!(doc["photos"]["IMG_0001.jpg"]["faces"], json!([{"person": ana().as_str(), "rect": [0.0, 0.0, 1.0, 1.0]}]));
    assert_eq!((&doc["photos"]["IMG_0001.jpg"]["geo"], &doc["people"][ana().as_str()]["name"]), (&Value::Null, &json!("Ana")));
    for op in undo.into_iter().rev() {
        lib.apply(op).unwrap();
    }
    assert_eq!(write_folder(&mapping::folder_state(&lib, f).unwrap(), NOW), Outcome::Written);
    assert_eq!(json_of(root.path()), planted);
}

/// A photo deleted outside Dalica whose entry holds only Dalica's fields other than the star (M6's
/// info; M8's copy saved beside; M13's faces, or only a removed location, `"geo": null`) is
/// Dalica's entry like a starred one (`data-model.md` §7): marked at the folder's next write, kept
/// for 30 days, pruned by the first write after them; the copy saved beside is another photo and
/// keeps its own entry.
#[test]
fn an_entry_with_only_dalica_fields_is_marked_and_pruned_when_its_file_is_gone() {
    use dalica_store::stale::{MISSING_KEY, PRUNE_AFTER};
    let info = PhotoInfo {
        caption: "gone soon".into(),
        tags: strings(&["beach"]),
        hidden: true,
        edits: strings(&["rotate(90)"]),
        redo: strings(&["sepia()"]),
        date: Some(dt("2026-06-04T19:42:10")),
    };
    let cases: [(&str, [&str; 2], Op, &str, Value); 4] = [
        ("M6 info", ["b.jpg", "a.jpg"], Op::SetPhotoInfo { photos: vec![(PhotoId::new(1), info)] }, "/photos/b.jpg/caption", json!("gone soon")),
        (
            "savedAs",
            ["a.CR2", "a.jpg"],
            Op::SetSavedAs { saved_as: vec![(PhotoId::new(1), Some(SavedAs { name: "a.jpg".into(), recipe: Vec::new() }))] },
            "/photos/a.CR2/savedAs/name",
            json!("a.jpg"),
        ),
        (
            "faces",
            ["b.jpg", "a.jpg"],
            Op::SetFaces { faces: vec![(PhotoId::new(1), vec![Face::new(rect(0.1, 0.2, 0.3, 0.4), FacePerson::Ignored)])] },
            "/photos/b.jpg/faces/0/person",
            json!("ignored"),
        ),
        ("geo null", ["b.jpg", "a.jpg"], Op::SetGeo { geo: vec![(PhotoId::new(1), Some(Geo::Removed))] }, "/photos/b.jpg/geo", Value::Null),
    ];
    for (case, names, set, kept_at, kept) in cases {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::new();
        let f = lib.next_folder_id();
        lib.apply(Op::AddFolder { folder: Folder { id: f, path: dir.path().to_path_buf() } }).unwrap();
        let mut ids = Vec::new();
        for name in names {
            std::fs::write(dir.path().join(name), name).unwrap();
            let id = lib.next_photo_id();
            let photo = Photo { id, folder: f, name: name.into(), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false };
            lib.apply(Op::AddPhotos { photos: vec![photo] }).unwrap();
            ids.push(id);
        }
        assert_eq!(ids[0], PhotoId::new(1), "{case}: the op names the first photo");
        lib.apply(set).unwrap();
        lib.apply(Op::SetStars { stars: vec![(ids[1], true)] }).unwrap();
        let mut w = SidecarWriter::new();
        w.mark_dirty(f);
        assert_eq!(w.flush(&lib, NOW)[&f], Outcome::Written, "{case}");

        std::fs::remove_file(dir.path().join(names[0])).unwrap();
        lib.apply(Op::RemovePhotos { ids: vec![ids[0]] }).unwrap();
        w.mark_dirty(f);
        assert_eq!(w.flush(&lib, NOW)[&f], Outcome::Written, "{case}: the mark is written");
        let doc = json_of(dir.path());
        assert!(doc["dalica"][MISSING_KEY][names[0]].is_string(), "{case}: {doc}");
        assert_eq!(doc.pointer(kept_at), Some(&kept), "{case}: the entry is kept while it waits");

        // A write 29 days on keeps it; the first write after 30 days prunes it and its mark.
        let day = std::time::Duration::from_secs(24 * 60 * 60);
        for (d, on) in [(29, false), (31, true)] {
            lib.apply(Op::SetStars { stars: vec![(ids[1], on)] }).unwrap();
            w.mark_dirty(f);
            assert_eq!(w.flush(&lib, NOW.saturating_add(day * d))[&f], Outcome::Written, "{case}");
        }
        let doc = json_of(dir.path());
        assert!(doc["photos"].get(names[0]).is_none(), "{case}: {doc}");
        assert!(doc["dalica"].get(MISSING_KEY).is_none(), "{case}: {doc}");
        assert_eq!(doc["photos"][names[1]]["star"], true, "{case}");
        assert_eq!(PRUNE_AFTER, day * 30);
    }
}

/// Nothing M13 adds is silently dropped (`data-model.md` §2–§4): every field of a face and a
/// person and both kinds of geotag is placed in a table, and the sidecar crate reads exactly the
/// keys the tables name. A new field breaks a destructuring here until it is placed.
#[test]
fn every_m13_field_has_a_place() {
    let Face { rect: _, person: _ } = Face::new(Rect01::FULL, FacePerson::Unnamed);
    assert_eq!(FACE_FIELDS.map(|(n, _)| n), ["rect", "person"]);
    assert!(FACE_FIELDS.iter().all(|(_, s)| *s == Source::Sidecar("faces")), "each in an object of the photo's faces list");
    let Person { name: _, nickname: _, thumbnail: _ } = Person::default();
    let FaceKey { photo: _, rect: _ } = FaceKey { photo: PhotoId::new(1), rect: Rect01::FULL };
    assert_eq!(PERSON_FIELDS.map(|(n, _)| n), ["name", "nickname", "thumbnail"]);
    assert!(PERSON_FIELDS.iter().all(|(n, s)| *s == Source::People(n)), "each under its own key in people.json");
    match Geo::Removed {
        Geo::At(_) | Geo::Removed => {}
    }
    assert_eq!(PHOTO_DATA_M13, [("faces", Source::Fields("FACE_FIELDS")), ("geo", Source::Sidecar("geo"))]);
    assert_eq!(FOLDER_DATA_M13, [("people", Source::TopSidecar("people"))]);
    assert_eq!(LIBRARY_DATA_M13, [("people", Source::Fields("PERSON_FIELDS"))]);
    let photo_keys: Vec<&str> =
        FACE_FIELDS.iter().chain(&PHOTO_DATA_M13).filter_map(|(_, s)| if let Source::Sidecar(k) = s { Some(*k) } else { None }).collect();
    assert_eq!(photo_keys, ["faces", "faces", "geo"]);
    assert_eq!(["faces", "geo"], M13_PHOTO_KEYS);
    assert_eq!(FOLDER_DATA_M13.map(|(_, s)| s), M13_TOP_KEYS.map(Source::TopSidecar));
    // `PhotoState` and `FolderState` name their M13 fields in `m9_fields.rs`'s destructuring.
    assert_eq!((PhotoState::default().faces, FolderState::default().people), (Vec::new(), Default::default()));
}

/// A face the model cannot hold (an empty rectangle, or a person the library does not have) stays
/// in the file after the model's faces and is never used; a face the model holds at the same
/// rectangle takes its place; other keys of a face are kept.
#[test]
fn faces_the_model_cannot_hold_are_kept_in_the_file() {
    let root = tempfile::tempdir().unwrap();
    let theirs = json!({"rect": [0.5, 0.5, 0.5, 0.6], "x-tool": 1});
    let stranger = json!({"rect": [0.1, 0.1, 0.3, 0.3], "person": "pabcdef"});
    let planted = json!({"photos": {"IMG_0001.jpg": {"faces": [theirs.clone(), stranger.clone(), {"rect": [0.6, 0.6, 0.7, 0.7], "x-kept": true}]}}});
    std::fs::write(root.path().join(".dalica.json"), planted.to_string()).unwrap();
    let (mut lib, folders, ids) = scan(&[root.path()]);
    let (f, a) = (folders[0], ids[0][0]);
    let import = import_library(&lib, root.path(), NOW);
    for op in import.ops {
        lib.apply(op).unwrap();
    }
    assert_eq!(lib.faces(a), [Face::new(rect(0.6, 0.6, 0.7, 0.7), FacePerson::Unnamed)], "only the face the model can hold");
    assert!(import.problems.iter().any(|(_, p)| p.contains("1 faces name a person named nowhere")), "{:?}", import.problems);
    assert!(import.problems.iter().any(|(_, p)| p.contains("1 faces have an empty rectangle")), "{:?}", import.problems);
    assert!(import.problems.iter().all(|(_, p)| !p.contains("0.1")), "rectangles never reach a note");

    // Naming the face the model holds: the others are kept after it, as written.
    lib.apply(Op::AddPerson { id: ana(), person: Person::named("Ana") }).unwrap();
    lib.apply(Op::SetFaces { faces: vec![(a, vec![Face::new(rect(0.6, 0.6, 0.7, 0.7), FacePerson::Person(ana()))])] }).unwrap();
    assert_eq!(write_folder(&mapping::folder_state(&lib, f).unwrap(), NOW), Outcome::Written);
    let faces = &json_of(root.path())["photos"]["IMG_0001.jpg"]["faces"];
    assert_eq!(faces, &json!([{"rect": [0.6, 0.6, 0.7, 0.7], "x-kept": true, "person": ana().as_str()}, theirs, stranger]));
    assert_eq!(write_folder(&mapping::folder_state(&lib, f).unwrap(), NOW), Outcome::Unchanged);

    // A face drawn by the model over the stranger's rectangle takes its place.
    let over = vec![Face::new(rect(0.6, 0.6, 0.7, 0.7), FacePerson::Person(ana())), Face::new(rect(0.1, 0.1, 0.3, 0.3), FacePerson::Ignored)];
    lib.apply(Op::SetFaces { faces: vec![(a, over)] }).unwrap();
    assert_eq!(write_folder(&mapping::folder_state(&lib, f).unwrap(), NOW), Outcome::Written);
    let faces = &json_of(root.path())["photos"]["IMG_0001.jpg"]["faces"];
    assert_eq!(faces[1], json!({"rect": [0.1, 0.1, 0.3, 0.3], "person": "ignored"}));
    assert_eq!(faces.as_array().map(Vec::len), Some(3), "{faces}");
    assert_eq!(faces[2], theirs);
    let geo = Sidecar::parse(&std::fs::read(root.path().join(".dalica.json")).unwrap(), &Limits::default()).unwrap().geo("IMG_0001.jpg");
    assert_eq!(geo, Field::Absent);
}
