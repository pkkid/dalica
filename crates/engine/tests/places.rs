//! Geotags (`roadmap.md` M13.14; `organizing.md` §7, `data-model.md` §5, `xmp-interop.md` §2) on
//! temporary JPEG, PNG, WebP and TIFF files: `photo.geo.set` on several photos is one undo step,
//! reaches each file (EXIF GPS, or a TIFF's XMP) and reads back within 10⁻⁶°, undo and redo write
//! the old and new place; Clear Location removes the GPS position and its XMP mirror and undo
//! writes them back; with the option off nothing is written; a file's GPS is the photo's geotag
//! after the first scan, unless its sidecar says the user removed it, and a removed location never
//! comes back from the file; Copy and Paste Location; changing the location of 21 tagged photos
//! and removing it from 6 ask first; `places.list` and the effective geotag in `photo.inspect`
//! and `library.inspect`; each command's graceful failures; the owner thread makes no file system
//! call.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{entry, jpeg, lisbon, off_owner, open_with, read};
use dalica_core::geo::LatLon;
use dalica_core::{PhotoId, SystemClock};
use dalica_engine::cmd::places::{NO_LOCATION, NONE_LOCATED, NOTHING_COPIED};
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use dalica_library::Geo;
use dalica_meta::{Changes, Limits, Metadata, Place, WriteOptions};
use dalica_scenes::Scene;
use dalica_scenes::formats::{Tiff, Webp};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(30);

/// A place within 10⁻⁶° of `(lat, lon)` on each axis.
fn near(p: Option<Place>, lat: f64, lon: f64) -> bool {
    p.is_some_and(|p| (p.latitude() - lat).abs() <= 1e-6 && (p.longitude() - lon).abs() <= 1e-6)
}

fn point(lat: f64, lon: f64) -> LatLon {
    LatLon::new(lat, lon).unwrap()
}

/// What another application (a phone) writes: a caption and a place, so a JPEG, PNG or WebP holds
/// the place in EXIF GPS and in its new XMP packet, a TIFF in XMP.
fn phone(bytes: Vec<u8>, lat: f64, lon: f64) -> Vec<u8> {
    let changes = Changes { caption: Some(Some("From the phone".into())), place: Some(Place::new(lat, lon)), ..Changes::default() };
    dalica_meta::write_owned(&bytes, &changes, &WriteOptions::default()).unwrap()
}

/// A TIFF with an XMP packet (a caption another application wrote): Dalica writes a TIFF's place
/// to its XMP only.
fn captioned_tiff(seed: u64) -> Vec<u8> {
    let tiff = dalica_scenes::formats::tiff(&Scene::new(seed, 40, 30), &Tiff::default()).unwrap();
    let changes = Changes { caption: Some(Some("Scanned".into())), ..Changes::default() };
    dalica_meta::write_owned(&tiff, &changes, &WriteOptions::default()).unwrap()
}

fn png(seed: u64) -> Vec<u8> {
    dalica_scenes::png(&Scene::new(seed, 40, 30)).unwrap()
}

fn webp(seed: u64) -> Vec<u8> {
    dalica_scenes::formats::webp(&Scene::new(seed, 40, 30), &Webp::default()).unwrap()
}

const NAMES: [&str; 4] = ["a.jpg", "b.png", "c.webp", "d.tif"];

/// The four containers, plain (a TIFF holds an XMP packet: its place goes nowhere else).
fn plain() -> Vec<(&'static str, Vec<u8>)> {
    vec![(NAMES[0], jpeg(1)), (NAMES[1], png(2)), (NAMES[2], webp(3)), (NAMES[3], captioned_tiff(4))]
}

/// A library watching one folder `Lisbon` holding `files` (and `sidecar`, when given).
struct Lib {
    _library: tempfile::TempDir,
    _disk: tempfile::TempDir,
    folder: PathBuf,
    s: Session,
}

impl Lib {
    fn new(files: &[(&str, Vec<u8>)], sidecar: Option<Value>) -> Lib {
        let (library, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let folder = disk.path().join("Lisbon");
        dalica_fs::create_dir_all(&folder).unwrap();
        for (name, bytes) in files {
            dalica_fs::replace(&folder.join(name), bytes, Expect::Absent).unwrap();
        }
        if let Some(v) = sidecar {
            dalica_fs::replace(&folder.join(".dalica.json"), serde_json::to_vec(&v).unwrap().as_slice(), Expect::Absent).unwrap();
        }
        let mut s = open_with(library.path(), Arc::new(SystemClock));
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        assert_eq!(s.library().photos().count(), files.len(), "every file is a photo: {:?}", s.scan_warnings());
        Lib { _library: library, _disk: disk, folder, s }
    }

    fn id(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap().id
    }

    fn ids(&self) -> Vec<PhotoId> {
        NAMES.iter().map(|n| self.id(n)).collect()
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e:?}"))
    }

    fn flush(&mut self) {
        assert!(self.s.flush_metadata(WAIT), "the writes landed in time");
        assert_eq!(self.s.pending_metadata_writes(), 0);
        assert_eq!(self.s.metadata_events().count(), 0, "{:?}", self.s.metadata_events().collect::<Vec<_>>());
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    /// The file's metadata, read off the owner thread.
    fn meta(&self, name: &str) -> Metadata {
        dalica_meta::read(&read(&self.path(name)), &Limits::default()).unwrap()
    }

    fn geo(&self, name: &str) -> Option<Geo> {
        self.s.library().geo(self.id(name))
    }

    fn sidecar_geo(&mut self, name: &str) -> Option<Value> {
        self.s.flush().unwrap();
        entry(&self.folder, name).and_then(|e| e.get("geo").cloned())
    }
}

/// Another application writes `changes` into `path` (off the owner thread).
fn other_app(path: &Path, changes: Changes) {
    let path = path.to_path_buf();
    off_owner(move || {
        let bytes = std::fs::read(&path).unwrap();
        let new = dalica_meta::write_owned(&bytes, &changes, &WriteOptions::default()).unwrap();
        let id = dalica_fs::identity(&path).unwrap().unwrap();
        dalica_fs::replace(&path, &new, Expect::Unchanged(id)).unwrap();
    });
}

/// A geotag set on every container is one undo step, written into each file (read back within
/// 10⁻⁶° by another reader) and into the sidecar; undo writes the old value back (no GPS: the
/// files had none) and redo the new one. The owner thread makes no file system call meanwhile.
#[test]
fn a_geotag_set_is_one_undo_step_that_reaches_every_file_and_undo_writes_the_old_place_back() {
    let mut l = Lib::new(&plain(), None);
    let ids = l.ids();
    assert!(NAMES.iter().all(|n| l.geo(n).is_none() && l.meta(n).owned.place.is_none()));
    let before = l.s.undo_stack().len();
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let v = l.run("photo.geo.set", json!({ "ids": ids, "geo": [38.711_634_9, -9.138_912_3] }));
    assert_eq!((v["changed"].clone(), v["geo"].clone()), (json!(4), json!([38.711635, -9.138912])), "kept at six decimals");
    assert_eq!(l.s.undo_stack().len(), before + 1, "one undo step");
    assert_eq!(l.s.undo_stack().undo_label(), Some("Set Location"));
    assert_eq!(l.s.pending_metadata_writes(), 4, "one write per photo");
    assert!(l.s.flush_metadata(WAIT));
    // Setting the same place again changes nothing and adds no step.
    assert_eq!(l.run("photo.geo.set", json!({ "ids": ids, "geo": [38.711635, -9.138912] }))["changed"], json!(0));
    assert_eq!(l.s.undo_stack().len(), before + 1);
    l.s.poll();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "no fs call was refused on the owner thread");
    l.flush();
    for name in NAMES {
        let m = l.meta(name);
        assert!(near(m.owned.place, 38.711635, -9.138912), "{name}: {:?}", m.owned.place);
        if name.ends_with(".tif") {
            assert!(m.sources.exif.place.is_none() && near(m.sources.xmp.place, 38.711635, -9.138912), "a TIFF's place goes to XMP");
        } else {
            assert!(near(m.sources.exif.place, 38.711635, -9.138912), "{name}: EXIF GPS");
        }
        assert_eq!(l.geo(name), Some(Geo::At(point(38.711635, -9.138912))));
        assert_eq!(l.sidecar_geo(name), Some(json!([38.711635, -9.138912])), "{name}");
    }
    assert!(l.s.undo_stack().len() == before + 1, "nothing came back from the files as a step");

    l.run("edit.undo", json!({}));
    l.flush();
    for name in NAMES {
        assert_eq!(l.geo(name), None, "{name}: the model's word is gone again");
        assert!(l.meta(name).owned.place.is_none(), "{name}: the place the file had (none) is written back");
    }
    l.run("edit.redo", json!({}));
    l.flush();
    assert!(NAMES.iter().all(|n| near(l.meta(n).owned.place, 38.711635, -9.138912)));
}

/// A phone photo's GPS is its geotag after the first scan (no undo step, nothing written back),
/// unless its sidecar says the user removed the location. Clear Location removes the GPS position
/// and its XMP mirror from every file; undo writes both back. A location removed never comes back
/// from the file: not when another application writes GPS into it again.
#[test]
fn clear_location_removes_the_gps_and_its_xmp_mirror_and_undo_writes_them_back() {
    let files = vec![
        (NAMES[0], phone(jpeg(1), 38.7139, -9.1394)),
        (NAMES[1], phone(png(2), 41.1496, -8.6109)),
        (NAMES[2], phone(webp(3), -33.8688, 151.2093)),
        (NAMES[3], phone(captioned_tiff(4), 64.1466, -21.9426)),
        ("e.jpg", phone(jpeg(5), 1.0, 2.0)),
    ];
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { "e.jpg": { "geo": null } } });
    let mut l = Lib::new(&files, Some(sidecar));
    let spots = [(38.7139, -9.1394), (41.1496, -8.6109), (-33.8688, 151.2093), (64.1466, -21.9426)];
    for (name, (lat, lon)) in NAMES.iter().zip(spots) {
        assert_eq!(l.geo(name), Some(Geo::At(point(lat, lon))), "{name}: the file's GPS is the geotag after the first scan");
        assert_eq!(l.sidecar_geo(name), Some(json!([lat, lon])), "{name}: and the sidecar holds it");
        let m = l.meta(name);
        assert!(near(m.sources.xmp.place, lat, lon), "{name}: the file holds an XMP mirror");
        assert_eq!(m.sources.exif.place.is_some(), !name.ends_with(".tif"), "{name}");
    }
    assert_eq!(l.geo("e.jpg"), Some(Geo::Removed), "the sidecar's word beats the file's GPS");
    assert!(l.s.undo_stack().is_empty(), "a scan's import is no undo step");
    assert_eq!(l.s.pending_metadata_writes(), 0, "nothing goes back into the files");

    let ids = l.ids();
    let v = l.run("geo.clear", json!({ "ids": ids }));
    assert_eq!((v["changed"].clone(), v["geo"].clone()), (json!(4), Value::Null));
    assert_eq!(l.s.undo_stack().undo_label(), Some("Clear Location"));
    l.flush();
    for name in NAMES {
        let m = l.meta(name);
        assert!(m.sources.exif.place.is_none() && m.sources.xmp.place.is_none(), "{name}: the position and its mirror are gone");
        assert_eq!(m.owned.caption.as_deref(), Some("From the phone"), "{name}: the rest is kept");
        assert_eq!(l.geo(name), Some(Geo::Removed));
        assert_eq!(l.sidecar_geo(name), Some(Value::Null), "{name}");
    }
    assert_eq!(l.run("places.list", json!({ "ids": ids }))["removed"], json!(4));

    l.run("edit.undo", json!({}));
    l.flush();
    for (name, (lat, lon)) in NAMES.iter().zip(spots) {
        let m = l.meta(name);
        assert!(near(m.sources.xmp.place, lat, lon), "{name}: the XMP mirror is written back");
        assert_eq!(near(m.sources.exif.place, lat, lon), !name.ends_with(".tif"), "{name}: the GPS position too");
        assert_eq!(l.geo(name), Some(Geo::At(point(lat, lon))));
    }

    // Removed again; another application then writes GPS into the file: a rescan keeps the
    // user's word.
    l.run("edit.redo", json!({}));
    l.flush();
    other_app(&l.path("a.jpg"), Changes { place: Place::new(10.0, 20.0).map(Some), ..Changes::default() });
    l.s.start_verification();
    l.s.wait_idle();
    assert!(l.meta("a.jpg").owned.place.is_some());
    assert_eq!(l.geo("a.jpg"), Some(Geo::Removed), "never re-imported over the user's word");
    assert_eq!(l.geo("e.jpg"), Some(Geo::Removed));
}

/// With Write captions, tags and places off, a geotag change writes nothing into the files; the
/// sidecar keeps it.
#[test]
fn with_the_option_off_no_place_is_written() {
    let mut l = Lib::new(&plain(), None);
    let ids = l.ids();
    let bytes: Vec<Vec<u8>> = NAMES.iter().map(|n| read(&l.path(n))).collect();
    let mut w = l.s.settings().metadata_writes();
    w.captions_tags_places = false;
    l.s.change_settings(|st| st.set_metadata_writes(&w));
    l.run("photo.geo.set", json!({ "ids": ids, "geo": [10.5, 20.25] }));
    assert_eq!(l.s.pending_metadata_writes(), 0, "nothing is queued");
    l.run("photo.geo.set", json!({ "ids": [ids[0]], "geo": null }));
    l.flush();
    for (i, name) in NAMES.iter().enumerate() {
        assert_eq!(read(&l.path(name)), bytes[i], "{name} unchanged");
    }
    assert_eq!(l.sidecar_geo("b.png"), Some(json!([10.5, 20.25])));
    assert_eq!(l.sidecar_geo("a.jpg"), Some(Value::Null));
}

/// A place another application changed in a file whose geotag Dalica did not change follows the
/// file at the next scan, with no undo step; one Dalica changed and did not write (the option
/// off) stays Dalica's.
#[test]
fn a_place_changed_in_another_application_follows_the_file() {
    let mut l = Lib::new(&[("a.jpg", phone(jpeg(1), 38.7, -9.1)), ("b.jpg", phone(jpeg(2), 38.7, -9.1))], None);
    let (a, b) = (l.id("a.jpg"), l.id("b.jpg"));
    let mut w = l.s.settings().metadata_writes();
    w.captions_tags_places = false;
    l.s.change_settings(|st| st.set_metadata_writes(&w));
    l.run("photo.geo.set", json!({ "ids": [b], "geo": [1.0, 1.0] }));
    let steps = l.s.undo_stack().len();
    other_app(&l.path("a.jpg"), Changes { place: Place::new(41.15, -8.61).map(Some), ..Changes::default() });
    other_app(&l.path("b.jpg"), Changes { place: Place::new(41.15, -8.61).map(Some), ..Changes::default() });
    l.s.start_verification();
    l.s.wait_idle();
    assert_eq!(l.s.library().geo(a), Some(Geo::At(point(41.15, -8.61))));
    assert_eq!(l.s.library().geo(b), Some(Geo::At(point(1.0, 1.0))), "changed on both sides: Dalica's stays");
    assert_eq!(l.s.undo_stack().len(), steps, "the file's place is no undo step");
    // The file loses its GPS: the model's word goes, and the photo is not geotagged.
    other_app(&l.path("a.jpg"), Changes { place: Some(None), ..Changes::default() });
    l.s.start_verification();
    l.s.wait_idle();
    assert_eq!(l.s.library().geo(a), None);
    assert!(!l.s.library().is_geotagged(a));
}

/// Copy Location keeps a photo's place; Paste Location gives it to photos as one undo step, and
/// it reaches their files.
#[test]
fn paste_applies_the_copied_place() {
    let mut l = Lib::new(&[("a.jpg", phone(jpeg(1), 38.7139, -9.1394)), ("b.jpg", jpeg(2)), ("c.png", png(3))], None);
    let (a, b, c) = (l.id("a.jpg"), l.id("b.jpg"), l.id("c.png"));
    assert_eq!(l.s.execute("geo.paste", &json!({ "ids": [b] })), Err(CmdError::Disabled(NOTHING_COPIED.into())));
    assert_eq!(l.s.execute("geo.copy", &json!({ "id": b })), Err(CmdError::Disabled(NO_LOCATION.into())));
    assert_eq!(l.run("geo.copy", json!({ "id": a })), json!({ "id": a, "geo": [38.7139, -9.1394] }));
    assert_eq!(l.s.places().copied(), Some([38.7139, -9.1394]));
    assert!(matches!(l.s.execute("geo.paste", &json!({ "id": b })), Err(CmdError::BadParams(_))));
    assert!(matches!(l.s.execute("geo.paste", &json!({ "ids": [] })), Err(CmdError::Disabled(_))));
    let before = l.s.undo_stack().len();
    assert_eq!(l.run("geo.paste", json!({ "ids": [b, c, a] }))["changed"], json!(2), "a has the place already");
    assert_eq!((l.s.undo_stack().len(), l.s.undo_stack().undo_label()), (before + 1, Some("Paste Location")));
    l.flush();
    for name in ["b.jpg", "c.png"] {
        assert_eq!(l.geo(name), Some(Geo::At(point(38.7139, -9.1394))));
        assert!(near(l.meta(name).owned.place, 38.7139, -9.1394), "{name}");
    }
    l.run("edit.undo", json!({}));
    assert!(l.geo("b.jpg").is_none() && l.geo("c.png").is_none());
}

/// Changing the location of more than 20 photos that already have one, or removing it from more
/// than 5, asks first (`needs_confirm` over automation: the question and "confirm": true); at the
/// thresholds it does not.
#[test]
fn moving_21_tagged_photos_or_clearing_6_asks_first() {
    let photos: Vec<Vec<u8>> = (0..22).map(jpeg).collect();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos, None);
    let mut w = s.settings().metadata_writes();
    w.captions_tags_places = false;
    s.change_settings(|st| st.set_metadata_writes(&w));
    // Untagged photos are no move: 22 at once do not ask.
    s.execute("photo.geo.set", &json!({ "ids": ids, "geo": [1.0, 1.0] })).unwrap();
    let steps = s.undo_stack().len();
    let asked = s.execute("photo.geo.set", &json!({ "ids": &ids[..21], "geo": [2.0, 2.0] }));
    assert_eq!(asked, Err(CmdError::Disabled("Change the location of 21 photos? Send \"confirm\": true to go ahead.".into())));
    assert_eq!((s.undo_stack().len(), s.library().geotag(ids[0])), (steps, Some(point(1.0, 1.0))), "nothing changed");
    assert_eq!(s.execute("photo.geo.set", &json!({ "ids": &ids[..20], "geo": [2.0, 2.0] })).unwrap()["changed"], json!(20));
    s.execute("photo.geo.set", &json!({ "ids": &ids[..21], "geo": [3.0, 3.0], "confirm": true })).unwrap();
    assert_eq!(s.library().geotag(ids[20]), Some(point(3.0, 3.0)));

    let clear = |s: &mut Session, n: usize, confirm: bool| s.execute("geo.clear", &json!({ "ids": &ids[..n], "confirm": confirm }));
    assert_eq!(clear(&mut s, 6, false), Err(CmdError::Disabled("Remove the location from 6 photos? Send \"confirm\": true to go ahead.".into())));
    let null = s.execute("photo.geo.set", &json!({ "ids": &ids[..6], "geo": null }));
    assert_eq!(null, Err(CmdError::Disabled("Remove the location from 6 photos? Send \"confirm\": true to go ahead.".into())));
    assert!(s.library().is_geotagged(ids[0]));
    assert_eq!(clear(&mut s, 5, false).unwrap()["changed"], json!(5));
    // The five without a location do not count: one more asks no question.
    assert_eq!(clear(&mut s, 6, false).unwrap()["changed"], json!(1));
    assert_eq!(clear(&mut s, 12, true).unwrap()["changed"], json!(6));
    assert_eq!(clear(&mut s, 3, false), Err(CmdError::Disabled(NONE_LOCATED.into())));
}

/// `places.list`: the geotags of the photos named or of a section, with the count of places.
#[test]
fn places_lists_the_geotags_of_the_photos_shown_or_selected() {
    let photos: Vec<Vec<u8>> = (0..4).map(jpeg).collect();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos, None);
    s.execute("photo.geo.set", &json!({ "ids": &ids[..2], "geo": [38.7, -9.1] })).unwrap();
    s.execute("photo.geo.set", &json!({ "ids": [ids[2]], "geo": [41.1, -8.6] })).unwrap();
    let folder = s.library().photo(ids[0]).unwrap().folder;
    let v = s.execute("places.list", &json!({ "section": format!("folder:{folder}") })).unwrap();
    assert_eq!((v["total"].clone(), v["places"].clone(), v["removed"].clone()), (json!(4), json!(2), json!(0)));
    assert_eq!(v["photos"].as_array().unwrap().len(), 3);
    let v = s.execute("places.list", &json!({ "ids": [ids[3], ids[1], ids[1]] })).unwrap();
    assert_eq!(v, json!({ "total": 2, "photos": [{ "id": ids[1], "geo": [38.7, -9.1] }], "places": 1, "removed": 0 }));
    s.execute("photo.star.set", &json!({ "ids": [ids[2]], "on": true })).unwrap();
    let starred = s.execute("places.list", &json!({ "section": "album:starred" })).unwrap();
    assert_eq!((starred["total"].clone(), starred["places"].clone()), (json!(1), json!(1)));

    // The effective geotag in `photo.inspect` and `library.inspect`.
    s.execute("geo.clear", &json!({ "ids": [ids[1]] })).unwrap();
    let v = s.execute("photo.inspect", &json!({ "ids": [ids[0], ids[1], ids[3]] })).unwrap();
    let got: Vec<(Value, Value)> = v.as_array().unwrap().iter().map(|p| (p["geotagged"].clone(), p["place"].clone())).collect();
    assert_eq!(got, [(json!(true), json!([38.7, -9.1])), (json!(false), Value::Null), (json!(false), Value::Null)]);
    let lib = s.execute("library.inspect", &json!({ "photos": format!("folder:{folder}") })).unwrap();
    let row = |id: PhotoId| lib["photos"].as_array().unwrap().iter().find(|p| p["id"] == json!(id)).cloned().unwrap();
    assert_eq!((row(ids[2])["geo"].clone(), row(ids[2])["place"].clone()), (json!(true), json!([41.1, -8.6])));
    assert_eq!((row(ids[1])["geo"].clone(), row(ids[1])["place"].clone()), (json!(false), Value::Null), "removed by the user");
}

/// Each command's graceful failures: nothing selected, unknown photos and sections, bad points and
/// bad shapes; none changes the library.
#[test]
fn each_command_fails_gracefully() {
    let photos: Vec<Vec<u8>> = (0..2).map(jpeg).collect();
    let (_lib, _disk, _folder, mut s, ids) = lisbon(&photos, None);
    let model = s.library().model().clone();
    let disabled = |r: Result<Value, CmdError>| matches!(r, Err(CmdError::Disabled(_)));
    let bad = |r: Result<Value, CmdError>| matches!(r, Err(CmdError::BadParams(_)));
    let missing = |r: Result<Value, CmdError>| matches!(r, Err(CmdError::NotFound(_)));
    assert!(disabled(s.execute("photo.geo.set", &json!({ "ids": [], "geo": [1.0, 1.0] }))));
    assert!(disabled(s.execute("geo.clear", &json!({ "ids": [] }))));
    assert!(disabled(s.execute("geo.clear", &json!({ "ids": ids }))), "nothing to remove");
    assert!(disabled(s.execute("places.list", &json!({}))));
    assert!(missing(s.execute("photo.geo.set", &json!({ "ids": [9999], "geo": [1.0, 1.0] }))));
    assert!(missing(s.execute("geo.copy", &json!({ "id": 9999 }))));
    assert!(missing(s.execute("places.list", &json!({ "ids": [9999] }))));
    assert!(missing(s.execute("places.list", &json!({ "section": "folder:9999" }))));
    for geo in [json!([91.0, 0.0]), json!([0.0, -180.5]), json!([1.0]), json!("38.7, -9.1"), json!([1.0, 2.0, 3.0]), json!({})] {
        assert!(bad(s.execute("photo.geo.set", &json!({ "ids": ids, "geo": geo }))), "{geo}");
    }
    assert!(bad(s.execute("photo.geo.set", &json!({ "ids": ids }))), "geo is required");
    assert!(bad(s.execute("photo.geo.set", &json!({ "ids": ids, "geo": null, "confirm": "yes" }))));
    assert!(disabled(s.execute("geo.paste", &json!({ "ids": ids }))), "nothing copied");
    assert!(bad(s.execute("places.list", &json!({ "ids": ids, "section": "album:starred" }))));
    assert!(bad(s.execute("places.list", &json!({ "section": "nowhere" }))));
    assert!(disabled(s.execute("places.list", &json!({ "section": "album:search" }))));
    assert_eq!(s.library().model(), &model, "nothing changed");
    assert!(s.undo_stack().is_empty());
}
