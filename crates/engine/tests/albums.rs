//! Albums on temporary folders (`roadmap.md` M9.10; `organizing.md` §4, `data-model.md` §3.2, §4,
//! §10.12): the album commands through `Session::execute`, what they write to the folder sidecars
//! and `albums.json`, and what scans and rebuilds read back.
//!
//! - `data-model.md` §10.12 through the commands: an album over three folders in a custom order
//!   keeps its exact order after `albums.json` and the index are deleted and the library rebuilt,
//!   and a write after the rebuild keeps every membership on disk;
//! - undo and redo of every command restore the model and the sidecars on disk;
//! - a drop between two members rewrites only the moved photo's sidecar;
//! - deleting an album keeps the files, removes every membership, and undo brings them back;
//! - Sort Album By and Reverse Order; Starred Photos and Recently Updated on a virtual clock;
//! - scans: a folder added with a sidecar brings its albums, memberships and photo sort; a folder
//!   renamed on disk keeps its memberships; a rebuild with `albums.json` brings back definitions,
//!   covers and empty albums; `albums.json` is written through the I/O worker and read at open.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{open, open_with};
use dalica_core::{AlbumId, Clock, PhotoId, SystemClock, Timestamp, VirtualClock};
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use serde_json::{Value, json};

const FILES: [&str; 7] = [
    "Lisbon/IMG_0001.jpg",
    "Lisbon/IMG_0002.jpg",
    "Lisbon/IMG_0003.jpg",
    "Porto/IMG_0101.jpg",
    "Porto/IMG_0102.jpg",
    "Sintra/IMG_0201.jpg",
    "Sintra/IMG_0202.jpg",
];
const FOLDERS: [&str; 3] = ["Lisbon", "Porto", "Sintra"];

/// The photo files under `root` (sizes differ: the scene number sets the picture).
fn write_photos(root: &Path) {
    for (i, name) in FILES.iter().enumerate() {
        let path = root.join(name);
        dalica_fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96 + 8 * i as u32, 64), 80, None).unwrap();
        dalica_fs::replace(&path, &bytes, Expect::Absent).unwrap();
    }
}

/// A library watching `Trips/` with `Lisbon` (3 photos), `Porto` (2) and `Sintra` (2).
struct Fx {
    lib: tempfile::TempDir,
    _disk: tempfile::TempDir,
    root: PathBuf,
}

impl Fx {
    fn new() -> (Fx, Session) {
        Fx::with_clock(Arc::new(SystemClock))
    }

    fn with_clock(clock: Arc<dyn Clock>) -> (Fx, Session) {
        let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let root = disk.path().join("Trips");
        write_photos(&root);
        let mut s = open_with(lib.path(), clock);
        s.execute("folders.add", &json!({ "path": root })).unwrap();
        s.wait_idle();
        (Fx { lib, _disk: disk, root }, s)
    }

    /// Every sidecar after all pending writes landed, without its `"dalica"` header, by folder.
    fn sidecars(&self, s: &mut Session) -> BTreeMap<&'static str, Value> {
        s.flush().unwrap();
        FOLDERS.iter().map(|f| (*f, sidecar(&self.root.join(f)))).collect()
    }

    fn albums_json(&self, s: &mut Session) -> Value {
        s.flush().unwrap();
        std::fs::read(self.lib.path().join("albums.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
    }

    /// Closes the session and deletes the index (and `albums.json` with `albums`).
    fn lose_index(&self, s: Session, albums: bool) {
        drop(s);
        for name in ["index.log", "index.snap"] {
            let _ = std::fs::remove_file(self.lib.path().join(name));
        }
        if albums {
            std::fs::remove_file(self.lib.path().join("albums.json")).unwrap();
        }
    }

    fn rebuild(&self, s: &mut Session) {
        s.execute("library.rebuild", &json!({})).unwrap();
        s.wait_idle();
    }
}

/// A folder's sidecar without its `"dalica"` header and without empty `"photos"` or `"albums"`
/// objects (a file left with nothing else says the same as none).
fn sidecar(folder: &Path) -> Value {
    let mut v: Value = std::fs::read(folder.join(".dalica.json")).map_or(json!({}), |b| serde_json::from_slice(&b).unwrap());
    if let Some(o) = v.as_object_mut() {
        o.remove("dalica");
        o.retain(|k, v| !((k == "photos" || k == "albums") && v.as_object().is_some_and(|m| m.is_empty())));
    }
    v
}

/// `"Lisbon/IMG_0001.jpg"`'s photo id.
fn id(s: &Session, path: &str) -> PhotoId {
    let (folder, name) = path.split_once('/').unwrap();
    s.library().photos().find(|p| p.name == name && s.library().folder(p.folder).unwrap().path.ends_with(folder)).unwrap().id
}

/// `"Lisbon/IMG_0001.jpg"` for photo `id`.
fn path_of(s: &Session, id: PhotoId) -> String {
    let p = s.library().photo(id).unwrap();
    let folder = s.library().folder(p.folder).unwrap();
    format!("{}/{}", folder.path.file_name().unwrap().to_string_lossy(), p.name)
}

/// Album `a`'s members in album order, by path.
fn members(s: &Session, a: &AlbumId) -> Vec<String> {
    s.library().album_members(a).iter().map(|m| path_of(s, *m)).collect()
}

/// The only album named `name`.
fn album(s: &Session, name: &str) -> AlbumId {
    let found: Vec<&AlbumId> = s.library().albums().filter(|(_, i)| i.name == name).map(|(id, _)| id).collect();
    assert_eq!(found.len(), 1, "albums named {name}: {found:?}");
    found[0].clone()
}

fn create(s: &mut Session, name: &str, paths: &[&str]) -> AlbumId {
    let ids: Vec<PhotoId> = paths.iter().map(|p| id(s, p)).collect();
    let v = s.execute("album.create", &json!({ "name": name, "ids": ids })).unwrap();
    AlbumId::parse(v["album"].as_str().unwrap()).unwrap()
}

/// The memberships the sidecars hold, as (path, album id, key), sorted.
fn memberships_on_disk(sidecars: &BTreeMap<&'static str, Value>) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (folder, doc) in sidecars {
        for (name, entry) in doc["photos"].as_object().into_iter().flatten() {
            for (a, k) in entry["albums"].as_object().into_iter().flatten() {
                out.push((format!("{folder}/{name}"), a.clone(), k.as_str().unwrap_or_default().to_string()));
            }
        }
    }
    out.sort();
    out
}

/// `data-model.md` §10.12 through the commands: an album over three folders in a custom order,
/// `albums.json` and the index deleted, the library rebuilt: the same albums in the same order; a
/// sidecar write afterwards keeps every membership on disk.
#[test]
fn an_album_over_three_folders_keeps_its_order_after_albums_json_and_the_index_are_lost() {
    let (fx, mut s) = Fx::new();
    let a = create(&mut s, "Best of the trip", &["Sintra/IMG_0202.jpg", "Lisbon/IMG_0002.jpg", "Porto/IMG_0101.jpg"]);
    // A drop between two members, and photos added before one.
    let (sintra, lisbon3, porto2) = (id(&s, "Sintra/IMG_0201.jpg"), id(&s, "Lisbon/IMG_0003.jpg"), id(&s, "Porto/IMG_0102.jpg"));
    s.execute("album.add", &json!({ "album": a, "ids": [sintra, lisbon3], "before": id(&s, "Lisbon/IMG_0002.jpg") })).unwrap();
    s.execute("album.reorder", &json!({ "album": a, "ids": [id(&s, "Porto/IMG_0101.jpg")], "after": id(&s, "Sintra/IMG_0202.jpg") })).unwrap();
    s.execute("album.add", &json!({ "album": a, "ids": [porto2] })).unwrap();
    let trams = create(&mut s, "Trams", &["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0003.jpg"]);
    let order = members(&s, &a);
    assert_eq!(
        order,
        ["Sintra/IMG_0202.jpg", "Porto/IMG_0101.jpg", "Sintra/IMG_0201.jpg", "Lisbon/IMG_0003.jpg", "Lisbon/IMG_0002.jpg", "Porto/IMG_0102.jpg"]
    );
    let trams_order = members(&s, &trams);
    let on_disk = memberships_on_disk(&fx.sidecars(&mut s));
    assert_eq!(on_disk.len(), 8);
    let names = fx.albums_json(&mut s);
    assert_eq!(names["albums"][a.as_str()]["name"], "Best of the trip", "albums.json was written");
    assert_eq!(names["albums"][a.as_str()]["order"].as_array().unwrap().len(), 6);

    fx.lose_index(s, true);
    let mut s = open(fx.lib.path());
    assert_eq!(s.library().photos().count(), 0, "the index is gone");
    fx.rebuild(&mut s);
    assert_eq!(s.library().albums().count(), 2);
    assert_eq!(album(&s, "Best of the trip"), a, "the same id");
    assert_eq!(members(&s, &a), order, "the same order");
    assert_eq!(members(&s, &trams), trams_order);
    assert!(s.library().album(&a).unwrap().use_newest, "dated by its newest photo");
    assert_eq!(fx.albums_json(&mut s)["albums"][a.as_str()]["name"], "Best of the trip", "albums.json written again after the rebuild");

    // A write in every folder after the rebuild (a star on a photo of each): every membership stays.
    for p in ["Lisbon/IMG_0001.jpg", "Porto/IMG_0102.jpg", "Sintra/IMG_0201.jpg"] {
        let photo = id(&s, p);
        s.execute("photo.star.set", &json!({ "ids": [photo], "on": true })).unwrap();
    }
    let after = fx.sidecars(&mut s);
    assert_eq!(after["Lisbon"]["photos"]["IMG_0001.jpg"]["star"], json!(true));
    assert_eq!(memberships_on_disk(&after), on_disk, "a write after the rebuild keeps every membership");

    // The rebuild that builds a model aside (`library.rebuild` on a full library) gives the same.
    fx.rebuild(&mut s);
    assert_eq!(members(&s, &album(&s, "Best of the trip")), order);
    assert_eq!(memberships_on_disk(&fx.sidecars(&mut s)), on_disk);
}

/// With `albums.json` kept, a rebuild of a lost index brings back what only it holds: the
/// description, place, date and cover, and an album with no members.
#[test]
fn a_rebuild_brings_back_definitions_covers_and_empty_albums_from_albums_json() {
    let (fx, mut s) = Fx::new();
    let a = create(&mut s, "Trip", &["Lisbon/IMG_0001.jpg", "Porto/IMG_0101.jpg", "Sintra/IMG_0201.jpg"]);
    s.execute(
        "album.info.set",
        &json!({ "album": a, "description": "The best of it", "place": "Portugal", "date": "2026-06-04", "useNewest": false }),
    )
    .unwrap();
    s.execute("album.setCover", &json!({ "album": a, "id": id(&s, "Porto/IMG_0101.jpg") })).unwrap();
    let empty = AlbumId::parse(s.execute("album.create", &json!({ "name": "Later" })).unwrap()["album"].as_str().unwrap()).unwrap();
    let want = (s.library().album(&a).cloned(), s.library().album(&empty).cloned(), members(&s, &a));
    s.flush().unwrap();
    fx.lose_index(s, false);
    let mut s = open(fx.lib.path());
    fx.rebuild(&mut s);
    assert_eq!((s.library().album(&a).cloned(), s.library().album(&empty).cloned(), members(&s, &a)), want);
    assert_eq!(s.library().cover_set(&a).map(|c| path_of(&s, c)).as_deref(), Some("Porto/IMG_0101.jpg"));
    // A rebuild on a full library (built aside) too.
    fx.rebuild(&mut s);
    assert_eq!((s.library().album(&a).cloned(), s.library().album(&empty).cloned(), members(&s, &a)), want);
    assert_eq!(s.library().cover_set(&a).map(|c| path_of(&s, c)).as_deref(), Some("Porto/IMG_0101.jpg"));
}

/// `sidecars` without the memberships of albums `lib` does not have and no folder names: what an
/// album undone out of existence leaves (`data-model.md` §3: a membership of an album the library
/// does not have is kept as written; with no name anywhere it is never used).
fn without_inert(sidecars: &BTreeMap<&'static str, Value>, lib: &dalica_library::Library) -> BTreeMap<&'static str, Value> {
    let mut out = sidecars.clone();
    for doc in out.values_mut() {
        let named: Vec<String> = doc["albums"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let Some(photos) = doc.get_mut("photos").and_then(Value::as_object_mut) else { continue };
        for entry in photos.values_mut() {
            let Some(o) = entry.as_object_mut() else { continue };
            if let Some(albums) = o.get_mut("albums").and_then(Value::as_object_mut) {
                albums.retain(|a, _| named.contains(a) || AlbumId::parse(a).is_ok_and(|id| lib.album(&id).is_some()));
                if albums.is_empty() {
                    o.remove("albums");
                }
            }
        }
        photos.retain(|_, e| e.as_object().is_none_or(|o| !o.is_empty()));
        if photos.is_empty()
            && let Some(o) = doc.as_object_mut()
        {
            o.remove("photos");
        }
    }
    out
}

/// Every album command, then undo and redo of each: the model and every sidecar on disk come back
/// exactly, step by step. Undoing the album's creation leaves its membership entries in the
/// sidecars, unnamed and never used (an album the library does not have is not the model's to
/// write); everything else on disk is as it was.
#[test]
fn undo_and_redo_of_every_command_restore_the_model_and_the_sidecars() {
    let (fx, mut s) = Fx::new();
    let p = |s: &Session, path: &str| id(s, path);
    let start = (s.library().model().clone(), fx.sidecars(&mut s));
    let mut states = vec![start];
    let a = create(&mut s, "Trip", &["Lisbon/IMG_0001.jpg", "Porto/IMG_0101.jpg"]);
    states.push((s.library().model().clone(), fx.sidecars(&mut s)));
    let commands: Vec<(&str, Value)> = vec![
        (
            "album.add",
            json!({ "album": a, "ids": [p(&s, "Sintra/IMG_0201.jpg"), p(&s, "Lisbon/IMG_0002.jpg")], "after": p(&s, "Lisbon/IMG_0001.jpg") }),
        ),
        ("album.reorder", json!({ "album": a, "ids": [p(&s, "Porto/IMG_0101.jpg")], "before": p(&s, "Lisbon/IMG_0001.jpg") })),
        ("album.info.set", json!({ "album": a, "name": "Portugal", "place": "Lisbon" })),
        ("album.setCover", json!({ "album": a, "id": p(&s, "Sintra/IMG_0201.jpg") })),
        ("section.sort", json!({ "section": format!("album:{a}"), "by": "name" })),
        ("section.sort", json!({ "section": format!("album:{a}"), "reverse": true })),
        (
            "section.sort",
            json!({ "section": format!("folder:{}", s.library().folder_by_path(&fx.root.join("Porto")).unwrap()), "by": "size", "reverse": true }),
        ),
        ("album.remove", json!({ "album": a, "ids": [p(&s, "Sintra/IMG_0201.jpg")], "confirm": true })),
        ("album.delete", json!({ "album": a, "confirm": true })),
    ];
    for (command, params) in &commands {
        s.execute(command, params).unwrap_or_else(|e| panic!("{command}: {e}"));
        states.push((s.library().model().clone(), fx.sidecars(&mut s)));
    }
    assert_eq!(s.undo_stack().len(), commands.len() + 1);
    for i in (0..states.len() - 1).rev() {
        s.execute("edit.undo", &json!({})).unwrap();
        let now = (s.library().model().clone(), fx.sidecars(&mut s));
        assert_eq!(now.0, states[i].0, "the model after undoing step {i}");
        assert_eq!(without_inert(&now.1, s.library()), without_inert(&states[i].1, s.library()), "the sidecars after undoing step {i}");
        if i > 0 {
            assert_eq!(now.1, states[i].1, "the sidecars after undoing step {i}, byte for byte in meaning");
        }
    }
    // Only the undone album's keys are left, with no name for it anywhere.
    let left = memberships_on_disk(&fx.sidecars(&mut s));
    assert_eq!(
        left.iter().map(|(p, a, _)| (p.as_str(), a.as_str())).collect::<Vec<_>>(),
        [("Lisbon/IMG_0001.jpg", a.as_str()), ("Porto/IMG_0101.jpg", a.as_str())]
    );
    assert!(fx.sidecars(&mut s).values().all(|d| d.get("albums").is_none()));
    for (i, want) in states.iter().enumerate().skip(1) {
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(&s.library().model().clone(), &want.0, "the model after redoing step {i}");
        assert_eq!(&fx.sidecars(&mut s), &want.1, "the sidecars after redoing step {i}");
    }
}

/// Dropping a photo between two members of other folders rewrites only its own folder's sidecar,
/// and only its key there.
#[test]
fn a_drop_between_two_members_rewrites_only_the_moved_photos_sidecar() {
    let (fx, mut s) = Fx::new();
    let a = create(&mut s, "Trip", &["Lisbon/IMG_0001.jpg", "Porto/IMG_0101.jpg", "Sintra/IMG_0201.jpg", "Sintra/IMG_0202.jpg"]);
    let before = fx.sidecars(&mut s);
    let identity = |f: &str| dalica_fs::identity(&fx.root.join(f).join(".dalica.json")).unwrap().unwrap();
    let files: Vec<_> = FOLDERS.iter().map(|f| identity(f)).collect();
    let moved = id(&s, "Sintra/IMG_0202.jpg");
    s.execute("album.reorder", &json!({ "album": a, "ids": [moved], "before": id(&s, "Porto/IMG_0101.jpg") })).unwrap();
    assert_eq!(members(&s, &a), ["Lisbon/IMG_0001.jpg", "Sintra/IMG_0202.jpg", "Porto/IMG_0101.jpg", "Sintra/IMG_0201.jpg"]);
    let after = fx.sidecars(&mut s);
    assert_eq!((identity("Lisbon"), identity("Porto")), (files[0], files[1]), "untouched files");
    assert_eq!((&after["Lisbon"], &after["Porto"]), (&before["Lisbon"], &before["Porto"]));
    assert_ne!(identity("Sintra"), files[2]);
    let mut changed = after["Sintra"].clone();
    changed["photos"]["IMG_0202.jpg"]["albums"] = before["Sintra"]["photos"]["IMG_0202.jpg"]["albums"].clone();
    assert_eq!(changed, before["Sintra"], "only the moved photo's key changed");
    let key = after["Sintra"]["photos"]["IMG_0202.jpg"]["albums"][a.as_str()].as_str().unwrap().to_string();
    let lisbon = before["Lisbon"]["photos"]["IMG_0001.jpg"]["albums"][a.as_str()].as_str().unwrap().to_string();
    let porto = before["Porto"]["photos"]["IMG_0101.jpg"]["albums"][a.as_str()].as_str().unwrap().to_string();
    assert!(lisbon < key && key < porto, "{lisbon} < {key} < {porto}");
}

/// Deleting an album keeps every file and takes every membership out of the model (and its name
/// out of the folders); undo brings all of it back.
#[test]
fn deleting_an_album_keeps_the_files_and_undo_brings_every_membership_back() {
    let (fx, mut s) = Fx::new();
    let files: BTreeMap<PathBuf, Vec<u8>> = FILES.iter().map(|f| (fx.root.join(f), std::fs::read(fx.root.join(f)).unwrap())).collect();
    let a = create(&mut s, "Trip", &["Lisbon/IMG_0001.jpg", "Porto/IMG_0101.jpg", "Sintra/IMG_0201.jpg"]);
    let (model, disk) = (s.library().model().clone(), fx.sidecars(&mut s));
    assert_eq!(disk["Porto"]["albums"][a.as_str()]["name"], "Trip");
    s.execute("album.delete", &json!({ "album": a, "confirm": true })).unwrap();
    assert!(s.library().album(&a).is_none());
    assert_eq!(s.library().memberships().count(), 0, "every membership left the model");
    let gone = fx.sidecars(&mut s);
    for f in FOLDERS {
        assert!(gone[f].get("albums").is_none(), "{f}'s album names went: {}", gone[f]);
    }
    assert!(fx.albums_json(&mut s)["albums"].get(a.as_str()).is_none(), "albums.json no longer has it");
    let kept: BTreeMap<PathBuf, Vec<u8>> = files.keys().map(|p| (p.clone(), std::fs::read(p).unwrap())).collect();
    assert_eq!(kept, files, "the photos' files never change");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &model);
    assert_eq!(fx.sidecars(&mut s), disk);
    assert_eq!(fx.albums_json(&mut s)["albums"][a.as_str()]["name"], "Trip");
    // Deleted, it never comes back from a rebuild.
    s.execute("edit.redo", &json!({})).unwrap();
    fx.rebuild(&mut s);
    assert_eq!(s.library().albums().count(), 0);
}

/// Sort Album By ▸ Name / Size / Date and Reverse Order rewrite the album's keys, one undo step
/// each; Starred Photos and Recently Updated keep their own order.
#[test]
fn sort_album_by_and_reverse_order() {
    let (_fx, mut s) = Fx::new();
    let a = create(&mut s, "Trip", &["Sintra/IMG_0202.jpg", "Lisbon/IMG_0002.jpg", "Porto/IMG_0101.jpg", "Lisbon/IMG_0001.jpg"]);
    let section = format!("album:{a}");
    let steps = s.undo_stack().len();
    s.execute("section.sort", &json!({ "section": section, "by": "name" })).unwrap();
    assert_eq!(members(&s, &a), ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Porto/IMG_0101.jpg", "Sintra/IMG_0202.jpg"]);
    s.execute("section.sort", &json!({ "section": section, "reverse": true })).unwrap();
    assert_eq!(members(&s, &a), ["Sintra/IMG_0202.jpg", "Porto/IMG_0101.jpg", "Lisbon/IMG_0002.jpg", "Lisbon/IMG_0001.jpg"]);
    // The pictures grow with the file order: by size is the same as by name here, reversed.
    s.execute("section.sort", &json!({ "section": section, "by": "size", "reverse": true })).unwrap();
    let by_size: Vec<String> = {
        let mut list: Vec<(u64, String)> =
            s.library().album_members(&a).iter().map(|m| (s.library().photo(*m).unwrap().size, path_of(&s, *m))).collect();
        list.sort();
        list.reverse();
        list.into_iter().map(|(_, p)| p).collect()
    };
    assert_eq!(members(&s, &a), by_size);
    s.execute("section.sort", &json!({ "section": section, "by": "date" })).unwrap();
    assert_eq!(s.undo_stack().len(), steps + 4);
    assert_eq!(s.execute("section.sort", &json!({ "section": "album:starred", "by": "name" })).unwrap_err().code(), "disabled");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(members(&s, &a), by_size);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(members(&s, &a), ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Porto/IMG_0101.jpg", "Sintra/IMG_0202.jpg"]);
}

/// Starred Photos follow the stars; Recently Updated follows what Dalica changed (stars, album
/// memberships) within its days on a virtual clock, newest first.
#[test]
fn starred_photos_and_recently_updated_follow_the_clock() {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_system_time(std::time::SystemTime::now())));
    let (_fx, mut s) = Fx::with_clock(clock.clone());
    let list = |s: &mut Session, kind: &str| -> Vec<String> {
        let v = s.execute("albums.list", &json!({})).unwrap();
        let row = v.as_array().unwrap().iter().find(|r| r["kind"] == kind).unwrap().clone();
        let ids: Vec<PhotoId> = match kind {
            "starred" => s.library().starred_photos().to_vec(),
            _ => s.library().recently_updated(s.clock().now(), 30),
        };
        assert_eq!(row["count"], json!(ids.len()));
        ids.iter().map(|id| path_of(s, *id)).collect()
    };
    assert_eq!(list(&mut s, "recent").len(), FILES.len(), "just indexed: every photo is recent");
    clock.advance(Duration::from_secs(31 * 86_400));
    assert!(list(&mut s, "recent").is_empty(), "31 days later none is");
    assert!(list(&mut s, "starred").is_empty());
    let star = id(&s, "Porto/IMG_0102.jpg");
    s.execute("photo.star.set", &json!({ "ids": [star], "on": true })).unwrap();
    assert_eq!(list(&mut s, "starred"), ["Porto/IMG_0102.jpg"]);
    assert_eq!(list(&mut s, "recent"), ["Porto/IMG_0102.jpg"]);
    clock.advance(Duration::from_secs(86_400));
    create(&mut s, "Trip", &["Lisbon/IMG_0003.jpg"]);
    assert_eq!(list(&mut s, "recent"), ["Lisbon/IMG_0003.jpg", "Porto/IMG_0102.jpg"], "newest first");
    // The section lists the same photos.
    let refreshed = s.execute("section.refreshThumbs", &json!({ "section": "album:recent" })).unwrap();
    assert_eq!(refreshed["photos"], json!(2));
    // Unstarring is a change too; the starred list empties.
    clock.advance(Duration::from_secs(86_400));
    s.execute("photo.star.set", &json!({ "ids": [star], "on": false })).unwrap();
    assert!(list(&mut s, "starred").is_empty());
    assert_eq!(list(&mut s, "recent"), ["Porto/IMG_0102.jpg", "Lisbon/IMG_0003.jpg"]);
    // Thirty days on both are still there; a day later the album change left, then the unstar.
    clock.advance(Duration::from_secs(29 * 86_400));
    assert_eq!(list(&mut s, "recent"), ["Porto/IMG_0102.jpg", "Lisbon/IMG_0003.jpg"]);
    clock.advance(Duration::from_secs(86_400));
    assert_eq!(list(&mut s, "recent"), ["Porto/IMG_0102.jpg"]);
    clock.advance(Duration::from_secs(86_400));
    assert!(list(&mut s, "recent").is_empty());
    // Undo puts the times back with the change.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(list(&mut s, "starred"), ["Porto/IMG_0102.jpg"]);
    s.wait_idle();
}

/// `photo.inspect` lists a photo's albums; `albums.list` the albums with their counts and covers.
#[test]
fn inspect_and_list_show_the_albums() {
    let (_fx, mut s) = Fx::new();
    let a = create(&mut s, "Trip", &["Lisbon/IMG_0001.jpg", "Porto/IMG_0101.jpg"]);
    let b = create(&mut s, "Bridges", &["Porto/IMG_0101.jpg"]);
    let porto = id(&s, "Porto/IMG_0101.jpg");
    let v = s.execute("photo.inspect", &json!({ "ids": [porto] })).unwrap();
    let mut listed: Vec<(String, String)> =
        v[0]["albums"].as_array().unwrap().iter().map(|e| (e["id"].as_str().unwrap().to_string(), e["name"].as_str().unwrap().to_string())).collect();
    listed.sort();
    let mut want = vec![(a.to_string(), "Trip".to_string()), (b.to_string(), "Bridges".to_string())];
    want.sort();
    assert_eq!(listed, want);
    let rows = s.execute("albums.list", &json!({})).unwrap();
    let users: Vec<(String, u64)> =
        rows.as_array().unwrap()[2..].iter().map(|r| (r["name"].as_str().unwrap().to_string(), r["count"].as_u64().unwrap())).collect();
    assert_eq!(users, [("Bridges".to_string(), 1), ("Trip".to_string(), 2)]);
    assert_eq!(rows[3]["cover"], json!(id(&s, "Lisbon/IMG_0001.jpg")));
    assert_eq!(rows[3]["section"], json!(format!("album:{a}")));
}

/// A folder added with a sidecar from elsewhere (another library, or a folder that was removed and
/// comes back) brings its albums, its photos' memberships in their order and its photo sort; an
/// album it names that this library deleted this session stays deleted.
#[test]
fn a_folder_added_with_a_sidecar_brings_its_albums_memberships_and_sort() {
    let (fx, mut s) = Fx::new();
    let extra = fx.root.parent().unwrap().join("Faro");
    for (i, name) in ["IMG_0301.jpg", "IMG_0302.jpg", "IMG_0303.jpg"].iter().enumerate() {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(30 + i as u64, 64, 48), 80, None).unwrap();
        dalica_fs::create_dir_all(&extra).unwrap();
        dalica_fs::replace(&extra.join(name), &bytes, Expect::Absent).unwrap();
    }
    let gone = create(&mut s, "Gone", &["Lisbon/IMG_0001.jpg"]);
    s.execute("album.delete", &json!({ "album": gone, "confirm": true })).unwrap();
    let doc = json!({
        "dalica": { "version": 1 },
        "folder": { "sort": "name", "reverse": true, "description": "Algarve" },
        "photos": {
            "IMG_0301.jpg": { "albums": { "a0beac40": "a2", "a5ea51de": "a1" } },
            "IMG_0302.jpg": { "albums": { "a0beac40": "a1", "a7777777": "a0", gone.as_str(): "a0" } },
            "IMG_0303.jpg": { "albums": { "bogus": "a0", "a0beac40": "not a key!" }, "otherApp": 1 }
        },
        "albums": { "a0beac40": { "name": "Beach" }, "a5ea51de": { "name": "  Sea\tside " }, gone.as_str(): { "name": "Gone" } },
        "elsewhere": true
    });
    dalica_fs::replace(&extra.join(".dalica.json"), &serde_json::to_vec_pretty(&doc).unwrap(), Expect::Absent).unwrap();
    s.execute("folders.add", &json!({ "path": extra })).unwrap();
    s.wait_idle();
    let beach = AlbumId::parse("a0beac40").unwrap();
    assert_eq!(s.library().album(&beach).unwrap().name, "Beach");
    assert!(s.library().album(&beach).unwrap().use_newest);
    assert_eq!(members(&s, &beach), ["Faro/IMG_0302.jpg", "Faro/IMG_0301.jpg"], "by key");
    assert_eq!(s.library().album(&AlbumId::parse("a5ea51de").unwrap()).unwrap().name, "Sea side", "the name cleaned");
    assert!(s.library().album(&AlbumId::parse("a7777777").unwrap()).is_none(), "named nowhere: not used");
    assert!(s.library().album(&gone).is_none(), "deleted here: stays deleted");
    let folder = s.library().folder_by_path(&extra).unwrap();
    assert_eq!(s.library().folder_sort(folder), dalica_library::PhotoSort::new(dalica_library::SortBy::Name, true));
    assert_eq!(s.undo_stack().len(), 2, "a scan records no step");
    // A write in the folder (a star) keeps what Dalica could not use.
    let star = id(&s, "Faro/IMG_0303.jpg");
    s.execute("photo.star.set", &json!({ "ids": [star], "on": true })).unwrap();
    s.flush().unwrap();
    let after = sidecar(&extra);
    assert_eq!(after["photos"]["IMG_0303.jpg"]["albums"], doc["photos"]["IMG_0303.jpg"]["albums"]);
    assert_eq!(after["photos"]["IMG_0302.jpg"]["albums"]["a7777777"], json!("a0"));
    assert_eq!(after["photos"]["IMG_0302.jpg"]["albums"][gone.as_str()], json!("a0"));
    assert_eq!(after["elsewhere"], json!(true));
    assert_eq!((after["folder"]["sort"].clone(), after["folder"]["reverse"].clone()), (json!("name"), json!(true)));
    assert_eq!(after["albums"]["a0beac40"]["name"], "Beach");
}

/// A folder renamed outside Dalica keeps what is said about it and its photos (`data-model.md`
/// §10.2): memberships, captions, the folder's sort, and a write at its new path keeps them on
/// disk. Through the (polling) watcher, which sees the rename, the ids, the cover and the indexed
/// times stay too (nothing shows as new); the verification pass sees a new folder and one gone, and
/// brings everything back from the sidecar.
#[test]
fn a_folder_renamed_on_disk_keeps_its_memberships_and_everything_else() {
    for watcher in [false, true] {
        let clock = Arc::new(VirtualClock::new(Timestamp::from_system_time(std::time::SystemTime::now())));
        let (fx, mut s) = Fx::with_clock(clock.clone());
        let settle = |s: &mut Session| {
            for _ in 0..4 {
                clock.advance(Duration::from_secs(1));
                assert!(s.wait_watching(Duration::from_secs(60)), "the watcher answered");
                s.wait_idle();
            }
        };
        if watcher {
            s.set_watching(dalica_engine::watching::WatchMode::Polling);
            settle(&mut s);
        }
        let a = create(&mut s, "Trip", &["Porto/IMG_0101.jpg", "Lisbon/IMG_0001.jpg", "Porto/IMG_0102.jpg"]);
        let (first, second) = (id(&s, "Porto/IMG_0101.jpg"), id(&s, "Porto/IMG_0102.jpg"));
        let _ = first;
        s.execute("album.setCover", &json!({ "album": a, "id": second })).unwrap();
        s.execute("photo.caption.set", &json!({ "ids": [first], "caption": "Ponte" })).unwrap();
        let porto = s.library().folder_by_path(&fx.root.join("Porto")).unwrap();
        s.execute("section.sort", &json!({ "section": format!("folder:{porto}"), "by": "name", "reverse": true })).unwrap();
        let order = members(&s, &a);
        s.flush().unwrap();
        // Long after they were indexed (by the clock), so a photo taken as new would be recent.
        clock.advance(Duration::from_secs(60 * 86_400));
        std::fs::rename(fx.root.join("Porto"), fx.root.join("Oporto")).unwrap();
        if watcher {
            settle(&mut s);
        } else {
            s.start_verification();
            s.wait_idle();
        }
        let oporto = s.library().folder_by_path(&fx.root.join("Oporto")).expect("the rename is found");
        assert_eq!(members(&s, &a), order.iter().map(|p| p.replace("Porto/", "Oporto/")).collect::<Vec<_>>(), "watcher: {watcher}");
        assert_eq!(s.library().info(id(&s, "Oporto/IMG_0101.jpg")).caption, "Ponte");
        assert_eq!(s.library().folder_sort(oporto), dalica_library::PhotoSort::new(dalica_library::SortBy::Name, true));
        if watcher {
            // The watcher sees the rename: ids, the cover and the indexed times stay.
            assert_eq!((oporto, id(&s, "Oporto/IMG_0101.jpg")), (porto, first), "the folder and its photos keep their ids");
            assert_eq!(s.library().cover_set(&a), Some(second));
            assert!(s.library().recently_updated(s.clock().now(), 30).is_empty(), "nothing shows as new");
        }
        let second = id(&s, "Oporto/IMG_0102.jpg");
        s.execute("photo.star.set", &json!({ "ids": [second], "on": true })).unwrap();
        s.flush().unwrap();
        let doc = sidecar(&fx.root.join("Oporto"));
        assert!(doc["photos"]["IMG_0101.jpg"]["albums"].get(a.as_str()).is_some(), "{doc}");
        assert!(doc["photos"]["IMG_0102.jpg"]["albums"].get(a.as_str()).is_some(), "{doc}");
        assert_eq!(doc["photos"]["IMG_0101.jpg"]["caption"], "Ponte");
        assert_eq!(doc["albums"][a.as_str()]["name"], "Trip");
        assert_eq!((doc["folder"]["sort"].clone(), doc["folder"]["reverse"].clone()), (json!("name"), json!(true)));
        if watcher {
            assert_eq!(
                fx.albums_json(&mut s)["albums"][a.as_str()]["cover"],
                json!(fx.root.join("Oporto").join("IMG_0102.jpg")),
                "albums.json follows the path"
            );
        }
    }
}

/// `albums.json` is written through the I/O worker (the owner thread touches no file), read at
/// open, and a damaged one is set aside, reported, and written anew.
#[test]
fn albums_json_is_written_off_the_owner_thread_and_a_damaged_one_is_set_aside() {
    let (fx, mut s) = Fx::new();
    let ui = dalica_fs::mark_ui_thread();
    let a = create(&mut s, "Trip", &["Lisbon/IMG_0001.jpg"]);
    s.execute("album.info.set", &json!({ "album": a, "description": "Summer" })).unwrap();
    s.poll();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), 0, "the owner thread made no file call");
    let file = fx.albums_json(&mut s);
    assert_eq!(file["albums"][a.as_str()]["description"], "Summer");
    drop(s);
    std::fs::write(fx.lib.path().join("albums.json"), b"{ not json").unwrap();
    let mut s = open(fx.lib.path());
    assert!(s.scan_warnings().iter().any(|w| w.contains("albums.json") && w.contains("kept as")), "{:?}", s.scan_warnings());
    let kept: Vec<String> = std::fs::read_dir(fx.lib.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("albums.json.corrupt-"))
        .collect();
    assert_eq!(kept.len(), 1, "the damaged file is kept aside");
    assert_eq!(fx.albums_json(&mut s)["albums"][a.as_str()]["description"], "Summer", "a new one written from the model");
    assert!(s.execute("album.info.set", &json!({ "album": a, "description": 3 })).is_err());
    let e = s.execute("album.delete", &json!({ "album": a })).unwrap_err();
    assert!(matches!(e, CmdError::Disabled(_)));
}

/// Entries of `albums.json` the session never had in its model are kept by its writes: with the
/// index lost, a new album made before any rebuild does not drop the others' definitions, and the
/// rebuild then brings them all back.
#[test]
fn a_write_keeps_the_albums_the_session_never_had() {
    let (fx, mut s) = Fx::new();
    let a = create(&mut s, "Trip", &["Lisbon/IMG_0001.jpg", "Sintra/IMG_0202.jpg"]);
    s.execute("album.info.set", &json!({ "album": a, "description": "Summer" })).unwrap();
    s.flush().unwrap();
    fx.lose_index(s, false);
    let mut s = open(fx.lib.path());
    assert_eq!(s.library().albums().count(), 0);
    let later = AlbumId::parse(s.execute("album.create", &json!({ "name": "Later" })).unwrap()["album"].as_str().unwrap()).unwrap();
    let file = fx.albums_json(&mut s);
    assert_eq!(file["albums"][a.as_str()]["description"], "Summer", "kept: {file}");
    assert_eq!(file["albums"][later.as_str()]["name"], "Later");
    fx.rebuild(&mut s);
    assert_eq!(s.library().album(&a).unwrap().description, "Summer");
    assert_eq!(members(&s, &a), ["Lisbon/IMG_0001.jpg", "Sintra/IMG_0202.jpg"]);
    assert!(s.library().album(&later).is_some());
}

/// A folder moved out of the watched root and back again, through the (polling) watcher: it comes
/// back as new photos, which take their memberships (and the folder its album names and sort) from
/// the sidecar, so the next write keeps them on disk. A second folder brought in by the same
/// rescan, naming an album the library does not have, creates it once.
#[test]
fn a_folder_moved_out_and_back_through_the_watcher_keeps_its_memberships() {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_system_time(std::time::SystemTime::now())));
    let (fx, mut s) = Fx::with_clock(clock.clone());
    s.set_watching(dalica_engine::watching::WatchMode::Polling);
    let settle = |s: &mut Session| {
        for _ in 0..4 {
            clock.advance(Duration::from_secs(1));
            assert!(s.wait_watching(Duration::from_secs(60)), "the watcher answered");
            s.wait_idle();
        }
    };
    settle(&mut s);
    let a = create(&mut s, "Trip", &["Porto/IMG_0101.jpg", "Lisbon/IMG_0001.jpg", "Porto/IMG_0102.jpg"]);
    let porto = s.library().folder_by_path(&fx.root.join("Porto")).unwrap();
    s.execute("section.sort", &json!({ "section": format!("folder:{porto}"), "by": "name" })).unwrap();
    let order = members(&s, &a);
    let before = fx.sidecars(&mut s)["Porto"].clone();
    settle(&mut s);
    let away = fx.root.parent().unwrap().join("Away");
    std::fs::rename(fx.root.join("Porto"), &away).unwrap();
    settle(&mut s);
    assert!(s.library().folder_by_path(&fx.root.join("Porto")).is_none(), "the folder left");
    assert_eq!(members(&s, &a), ["Lisbon/IMG_0001.jpg"]);
    // Back, with a second folder whose sidecar names an album the library does not have.
    let faro = fx.root.join("Faro");
    dalica_fs::create_dir_all(&faro).unwrap();
    for (i, name) in ["IMG_0301.jpg", "IMG_0302.jpg"].iter().enumerate() {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40 + i as u64, 64, 48), 80, None).unwrap();
        dalica_fs::replace(&faro.join(name), &bytes, Expect::Absent).unwrap();
    }
    let doc = json!({ "photos": { "IMG_0301.jpg": { "albums": { "a0beac40": "a1" } } }, "albums": { "a0beac40": { "name": "Beach" } } });
    dalica_fs::replace(&faro.join(".dalica.json"), &serde_json::to_vec(&doc).unwrap(), Expect::Absent).unwrap();
    std::fs::rename(&away, fx.root.join("Porto")).unwrap();
    settle(&mut s);
    assert_eq!(members(&s, &a), order, "the memberships came back with the folder");
    let back = s.library().folder_by_path(&fx.root.join("Porto")).unwrap();
    assert_eq!(s.library().folder_sort(back), dalica_library::PhotoSort::new(dalica_library::SortBy::Name, false));
    assert_eq!(members(&s, &album(&s, "Beach")), ["Faro/IMG_0301.jpg"]);
    let star = id(&s, "Porto/IMG_0102.jpg");
    s.execute("photo.star.set", &json!({ "ids": [star], "on": true })).unwrap();
    let mut after = fx.sidecars(&mut s)["Porto"].clone();
    assert_eq!(after["photos"]["IMG_0102.jpg"]["star"], json!(true));
    after["photos"]["IMG_0102.jpg"].as_object_mut().unwrap().remove("star");
    assert_eq!(after, before, "a write after the return keeps every membership, the names and the sort");
}
