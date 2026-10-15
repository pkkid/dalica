//! A day in the life, end to end (`roadmap.md` M10.35; `data-model.md` §3 and §8,
//! `architecture.md` §4): a folder an earlier session left data in is scanned; photos are starred,
//! captioned and tagged, moved to another folder and renamed; the whole chain is undone, one step
//! at a time; the session quits, a new one opens the library and rebuilds it from the watched
//! folders. Every photo is then back at its path with the bytes it started with, and the rebuilt
//! library says about it what the scan said at the start (star, caption, tags, hidden), its sidecar
//! entry keeping a key only another program knows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_fs::Expect;
use serde_json::{Value, json};

use crate::common::{entry, files, jpeg, open, read, wait_files};

const PATIENCE: Duration = Duration::from_secs(120);
const OTHER: &str = "x-other";

/// What the library and the disk say about one photo.
#[derive(Clone, PartialEq, Eq)]
struct Seen {
    bytes: Vec<u8>,
    star: bool,
    caption: String,
    tags: Vec<String>,
    hidden: bool,
    /// The sidecar entry's key only another program knows.
    other: Option<Value>,
}

impl std::fmt::Debug for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The bytes by length and a checksum, so a difference reads at a glance.
        let sum = self.bytes.iter().fold(0u32, |a, b| a.rotate_left(5) ^ u32::from(*b));
        f.debug_struct("Seen")
            .field("bytes", &format_args!("{} bytes, sum {sum:08x}", self.bytes.len()))
            .field("star", &self.star)
            .field("caption", &self.caption)
            .field("tags", &self.tags)
            .field("hidden", &self.hidden)
            .field("other", &self.other)
            .finish()
    }
}

/// Every photo the library lists, by its path relative to `pictures`.
fn seen(s: &Session, pictures: &Path) -> BTreeMap<String, Seen> {
    let lib = s.library();
    let mut out = BTreeMap::new();
    for p in lib.photos() {
        let folder = &lib.folder(p.folder).unwrap().path;
        let path = folder.join(&p.name);
        let rel =
            path.strip_prefix(pictures).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        let info = lib.info(p.id);
        let other = entry(folder, &p.name).and_then(|e| e.get(OTHER).cloned());
        out.insert(
            rel,
            Seen { bytes: read(&path), star: p.star, caption: info.caption.clone(), tags: info.tags.clone(), hidden: info.hidden, other },
        );
    }
    out
}

fn id(s: &Session, folder: &Path, name: &str) -> PhotoId {
    let f = s.library().folder_by_path(folder).unwrap_or_else(|| panic!("no folder {}", folder.display()));
    s.library().photo_by_name(f, name).unwrap_or_else(|| panic!("no {name} in {}", folder.display())).id
}

/// Writes everything pending and checks nothing is left unsaved.
fn settle(s: &mut Session) {
    assert!(s.flush_metadata(PATIENCE), "the metadata writes finished");
    s.flush().unwrap();
}

/// The photos whose files the day wrote a caption or tags into (Options: "write captions, tags
/// and places into photo files", on by default), and wrote back when it was undone.
const WRITTEN: [&str; 2] = ["Lisbon/IMG_0002.jpg", "Lisbon/IMG_0003.jpg"];

/// A JPEG's image data: its segments other than the application segments (where EXIF, XMP and
/// IPTC live), and the scan to the end.
fn image_data(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes[..2].to_vec();
    let mut i = 2;
    while i + 4 <= bytes.len() {
        let marker = bytes[i + 1];
        if marker == 0xDA {
            out.extend_from_slice(&bytes[i..]);
            break;
        }
        let end = i + 2 + usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        if !(0xE0..=0xEF).contains(&marker) {
            out.extend_from_slice(&bytes[i..end]);
        }
        i = end;
    }
    out
}

/// What a file says about itself (caption, tags, rating, place, date).
fn owned(bytes: &[u8]) -> dalica_meta::OwnedFields {
    dalica_meta::read(bytes, &dalica_meta::Limits::default()).unwrap().owned
}

/// `now` lists the photos `start` did, each saying the same; its bytes the same, or for a photo the
/// day wrote metadata into, the same image data and the same fields as the file says them: undoing
/// a caption or a tag writes the old value back, which leaves the file's metadata block in place.
fn same(start: &BTreeMap<String, Seen>, now: &BTreeMap<String, Seen>, what: &str) {
    assert_eq!(now.keys().collect::<Vec<_>>(), start.keys().collect::<Vec<_>>(), "{what} lists the photos it started with");
    for (rel, before) in start {
        let after = &now[rel];
        if WRITTEN.contains(&rel.as_str()) {
            assert!(image_data(&after.bytes) == image_data(&before.bytes), "{what}: {rel}'s image data changed");
            assert_eq!(owned(&after.bytes), owned(&before.bytes), "{what}: {rel}'s file says otherwise");
        } else {
            assert!(after.bytes == before.bytes, "{what}: {rel}'s bytes changed: {after:?}, was {before:?}");
        }
        let data = |x: &Seen| (x.star, x.caption.clone(), x.tags.clone(), x.hidden, x.other.clone());
        assert_eq!(data(after), data(before), "{what}: {rel} is not as it started");
    }
}

#[test]
fn a_day_of_organizing_undone_step_by_step_comes_back_from_the_sidecars_as_it_began() {
    let _faults = crate::common::shared();
    let (disk, earlier, lib) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let pictures = disk.path().join("Pictures");
    let (lisbon, porto) = (pictures.join("Lisbon"), pictures.join("Porto"));
    for (folder, name, seed) in
        [(&lisbon, "IMG_0001.jpg", 1), (&lisbon, "IMG_0002.jpg", 2), (&lisbon, "IMG_0003.jpg", 3), (&porto, "PORTO_0001.jpg", 4)]
    {
        dalica_fs::create_dir_all(folder).unwrap();
        dalica_fs::replace(&folder.join(name), &jpeg(seed), Expect::Absent).unwrap();
    }

    // An earlier session (another library) left data in the sidecars; another program added a key.
    {
        let mut s = open(earlier.path());
        s.execute("folders.add", &json!({ "path": pictures })).unwrap();
        s.wait_idle();
        let first = id(&s, &lisbon, "IMG_0001.jpg");
        s.execute("photo.star.set", &json!({ "ids": [first], "on": true })).unwrap();
        s.execute("photo.caption.set", &json!({ "ids": [first], "caption": "Tram 28" })).unwrap();
        s.execute("photo.tags.add", &json!({ "ids": [first, id(&s, &porto, "PORTO_0001.jpg")], "tags": "tram" })).unwrap();
        settle(&mut s);
    }
    let sidecar = lisbon.join(".dalica.json");
    let mut doc: Value = serde_json::from_slice(&read(&sidecar)).unwrap();
    doc["photos"]["IMG_0001.jpg"][OTHER] = json!({ "rating": 4, "by": "another program" });
    std::fs::write(&sidecar, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();

    // The day: the folder is scanned into a new library.
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": pictures })).unwrap();
    s.wait_idle();
    let start = seen(&s, &pictures);
    assert_eq!(start.keys().collect::<Vec<_>>(), ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg", "Lisbon/IMG_0003.jpg", "Porto/PORTO_0001.jpg"]);
    let first = &start["Lisbon/IMG_0001.jpg"];
    assert!(first.star && first.caption == "Tram 28" && first.tags == ["tram"] && first.other.is_some(), "the scan read the sidecar: {first:?}");
    let steps = s.undo_stack().len();

    // Star, caption and tag; move two photos to Porto; rename one there.
    let (one, two, three) = (id(&s, &lisbon, "IMG_0001.jpg"), id(&s, &lisbon, "IMG_0002.jpg"), id(&s, &lisbon, "IMG_0003.jpg"));
    s.execute("photo.star.set", &json!({ "ids": [two, three], "on": true })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [two], "caption": "Alfama at dusk" })).unwrap();
    s.execute("photo.tags.add", &json!({ "ids": [two, three], "tags": "lisbon" })).unwrap();
    let porto_id = s.library().folder_by_path(&porto).unwrap();
    s.execute("photo.moveTo", &json!({ "ids": [one, two], "folder": porto_id, "confirm": true })).unwrap();
    wait_files(&mut s);
    s.execute("photo.rename", &json!({ "ids": [two], "name": "Alfama" })).unwrap();
    wait_files(&mut s);
    settle(&mut s);
    assert_eq!(s.undo_stack().len(), steps + 5, "five steps");
    let renamed = s.library().photo(two).unwrap().name.clone();
    assert_eq!(files(&lisbon), ["IMG_0003.jpg"]);
    assert_eq!(files(&porto), {
        let mut v = vec!["IMG_0001.jpg".to_string(), "PORTO_0001.jpg".to_string(), renamed.clone()];
        v.sort();
        v
    });
    // The entries went with the files: the moved one keeps the foreign key, the renamed one its day.
    assert_eq!(entry(&porto, "IMG_0001.jpg").unwrap()[OTHER], first.other.clone().unwrap());
    let day = entry(&porto, &renamed).unwrap();
    assert_eq!((day["caption"].as_str(), day["star"].as_bool()), (Some("Alfama at dusk"), Some(true)), "{day}");

    // Undo the whole chain, one step at a time.
    for _ in 0..5 {
        s.execute("edit.undo", &json!({})).unwrap();
        wait_files(&mut s);
    }
    assert_eq!(s.undo_stack().len(), steps, "the chain is undone");
    assert_eq!(s.undo_stack().redo_len(), 5);
    settle(&mut s);
    same(&start, &seen(&s, &pictures), "undone, the session");
    drop(s);

    // Quit; open the library again and rebuild it from the watched folders.
    let mut s = open(lib.path());
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    same(&start, &seen(&s, &pictures), "the rebuilt library");
    let on_disk: Vec<PathBuf> = [&lisbon, &porto].iter().flat_map(|f| files(f).into_iter().map(move |n| f.join(n))).collect();
    assert_eq!(on_disk.len(), start.len(), "no file left behind: {on_disk:?}");
}
