//! Places and geotagging in the live app (`m13.rs` part 3): `dalica --headless` on six photos
//! without a location, every connection it tries counted by the loopback proxy:
//!
//! - the Places pane opens on the offline map with tiles off (its one-time question answered Not
//!   Now) and nothing connects;
//! - three photos dragged from the light table onto the map and the bubble's OK geotag them where
//!   they were let go, as one undo step (one Undo takes all three back, Redo puts them there
//!   again); the pane with its marker matches its fingerprint;
//! - the place reaches each of the three files (EXIF GPS and its XMP mirror: each file holds a
//!   caption's XMP packet another application wrote) within 10⁻⁶° of the library's, no other
//!   file, and a new library reading copies of the files finds it there;
//! - the geotags survive a restart and an index rebuild;
//! - Tools ▸ Geotag ▸ Clear Location… removes the GPS position and its XMP mirror from the files.

use std::path::{Path, PathBuf};
use std::time::Instant;

use dalica_meta::Limits;
use serde_json::{Value, json};

use crate::app::{self, click, dialog, flush_until, run, secs};
use crate::common::{LARGE, Proxy, steady_shot, undo_state};
use crate::live::{self, Client, Launch};

/// Where the map looks: Portugal at zoom 6.
const PORTUGAL: [f64; 2] = [39.6, -8.0];
/// The library's geotag and the file's agree this closely.
const CLOSE: f64 = 1e-6;

/// The file's places: (after precedence, EXIF's, XMP's), each `[lat, lon]`.
type Gps = (Option<[f64; 2]>, Option<[f64; 2]>, Option<[f64; 2]>);

fn gps(path: &Path) -> Gps {
    let Ok(read) = dalica_meta::read(&std::fs::read(path).unwrap_or_default(), &Limits::default()) else { return (None, None, None) };
    let point = |p: &Option<dalica_meta::Place>| p.as_ref().map(|p| [p.latitude(), p.longitude()]);
    (point(&read.owned.place), point(&read.sources.exif.place), point(&read.sources.xmp.place))
}

fn near(a: Option<[f64; 2]>, b: [f64; 2]) -> bool {
    a.is_some_and(|a| (a[0] - b[0]).abs() <= CLOSE && (a[1] - b[1]).abs() <= CLOSE)
}

/// The library's geotag of a photo (`photo.inspect` → `place`), `[lat, lon]`.
fn place(c: &mut Client, id: &Value) -> Option<[f64; 2]> {
    let p = c.inspect(id);
    let v = &p["place"];
    match (v[0].as_f64(), v[1].as_f64(), v["lat"].as_f64(), v["lon"].as_f64()) {
        (Some(lat), Some(lon), _, _) | (_, _, Some(lat), Some(lon)) => Some([lat, lon]),
        _ => {
            assert!(v.is_null(), "photo.inspect place: {p}");
            None
        }
    }
}

fn markers(c: &mut Client) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = c
        .widgets("marker:")
        .into_iter()
        .filter_map(|w| Some((w["id"].as_str()?.to_string(), w["label"].as_str().unwrap_or_default().to_string())))
        .filter(|(id, _)| id.starts_with("marker:") && !id.contains('/'))
        .collect();
    v.sort();
    v
}

fn launch(proxy: &Proxy, home: &Path) -> Launch {
    proxy.launch(Launch::default().env("DALICA_FACE_MODELS", home.join("models")))
}

/// `outer` holds the app's home (`outer/app`) and, for the read-back, a second home (`outer`
/// itself, whose automation may read the first's folders).
pub fn places(exe: &Path, outer: &Path) {
    let started = Instant::now();
    let home = &outer.join("app");
    std::fs::create_dir_all(home).unwrap();
    let proxy = Proxy::start();
    let folder = home.join("Pictures").join("Lisbon");
    let files: Vec<PathBuf> = (1..=6).map(|n| folder.join(format!("IMG_{n:04}.jpg"))).collect();
    for (i, f) in files.iter().enumerate() {
        let plain = dalica_scenes::jpeg(&dalica_scenes::Scene::new(1300 + i as u64, 640, 480), 88, None).unwrap();
        // A caption another application wrote: the file holds an XMP packet, so the place gets its
        // mirror there (`xmp-interop.md` §2).
        let changes = dalica_meta::Changes { caption: Some(Some("Lisbon".into())), ..Default::default() };
        let bytes = dalica_meta::write_owned(&plain, &changes, &dalica_meta::WriteOptions::default()).unwrap();
        app::put(f, &bytes, i as u64);
        assert_eq!(gps(f), (None, None, None), "the photos start without a place");
    }
    let (a, mut cl) = live::start_with(exe, home, launch(&proxy, home));
    let c = &mut cl;
    app::resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    let by_path = app::photos(c, &home.join("Pictures"));
    let ids: Vec<Value> = (1..=6).map(|n| by_path[&format!("Lisbon/IMG_{n:04}.jpg")].id.clone()).collect();

    // The pane on the offline map; its question answered Not Now.
    run(c, "pane.places", json!({}));
    run(c, "map.pan", json!({ "centre": PORTUGAL, "zoom": 6 }));
    if app::widget(c, "button:map.notNow").is_some() {
        click(c, "button:map.notNow");
    }
    assert_eq!(proxy.connections(), Vec::<String>::new(), "start-up, the scan and the pane with tiles off made no connection");

    // Three photos dropped on the map: one undo step.
    let three = &ids[..3];
    run(c, "select.set", json!({ "ids": three }));
    let centre = c.state()["places"]["centre"].clone();
    let centre = [centre[0].as_f64().unwrap(), centre[1].as_f64().unwrap()];
    let (depth, _) = undo_state(c);
    c.ok("ui.dragWidget", json!({ "id": format!("thumb:{}", three[1]), "toWidget": "map:places" }));
    app::idle(c);
    assert_eq!(app::label(c, "map:places/bubble"), "Put 3 photos here?");
    click(c, "button:places.put");
    let at = place(c, &three[0]).expect("geotagged");
    assert!((at[0] - centre[0]).abs() < 0.05 && (at[1] - centre[1]).abs() < 0.05, "where it was let go: {at:?} near {centre:?}");
    for id in three {
        assert_eq!(place(c, id), Some(at), "the three at one point");
    }
    for id in &ids[3..] {
        assert_eq!(place(c, id), None, "photo {id} keeps no place");
    }
    let (after, label) = undo_state(c);
    assert_eq!(after, depth + 1, "geotagging by drag is one undo step ({label})");
    assert_eq!(markers(c), [("marker:0".to_string(), "3".to_string())], "one marker of 3");
    steady_shot(c, home, "m13-1400x900-places");
    run(c, "edit.undo", json!({}));
    assert!(three.iter().all(|id| place(c, id).is_none()), "one Undo takes all three back");
    run(c, "edit.redo", json!({}));
    assert!(three.iter().all(|id| place(c, id) == Some(at)), "Redo puts them there again");
    println!("  three photos dropped on the map at {at:?}: one undo step, one marker of 3");

    // The GPS round-trip.
    flush_until(c, || files[..3].iter().all(|f| near(gps(f).0, at)));
    for f in &files[..3] {
        let (owned, exif, xmp) = gps(f);
        assert!(
            near(owned, at) && near(exif, at) && near(xmp, at),
            "{}: the place in EXIF and its XMP mirror within 1e-6°: {:?}",
            f.display(),
            gps(f)
        );
    }
    for f in &files[3..] {
        assert_eq!(gps(f), (None, None, None), "{}: no place", f.display());
    }
    live::quit(a, cl);
    read_back(exe, outer, &files[..3], at);

    // A restart, then an index rebuild.
    let (a, mut cl) = live::start_with(exe, home, launch(&proxy, home));
    let c = &mut cl;
    app::resize(c, LARGE);
    assert!(three.iter().all(|id| place(c, id).is_some_and(|p| near(Some(p), at))), "after a restart the three keep their place");
    run(c, "library.rebuild", json!({ "confirm": true }));
    let by_path = app::photos(c, &home.join("Pictures"));
    let rebuilt: Vec<Value> = (1..=6).map(|n| by_path[&format!("Lisbon/IMG_{n:04}.jpg")].id.clone()).collect();
    for (i, id) in rebuilt.iter().enumerate() {
        let p = place(c, id);
        assert_eq!(p.is_some_and(|p| near(Some(p), at)), i < 3, "after an index rebuild photo {} has {p:?}", i + 1);
    }
    println!("  the place reached the three files (EXIF GPS and XMP) within 1e-6°; kept over a restart and an index rebuild");

    // Clear Location from the Tools menu: the position and its mirror leave the files.
    run(c, "select.set", json!({ "ids": &rebuilt[..3] }));
    c.ok("ui.menu.invoke", json!({ "path": "Tools/Geotag/Clear Location…", "wait": true }));
    app::idle(c);
    if dialog(c).is_some() {
        c.ok("ui.dialog.confirm", json!({}));
        app::idle(c);
    }
    assert!(rebuilt.iter().all(|id| place(c, id).is_none()), "no photo has a place");
    flush_until(c, || files[..3].iter().all(|f| gps(f) == (None, None, None)));
    for f in &files[..3] {
        assert_eq!(gps(f), (None, None, None), "{}: Clear Location removes the GPS position and its XMP mirror", f.display());
    }
    live::quit(a, cl);
    assert_eq!(proxy.connections(), Vec::<String>::new(), "no connection at any point");
    println!("  Clear Location removed the GPS position and its XMP mirror from the three files");
    println!("  M13 places and geotagging in {:.1} s", secs(started));
}

/// The GPS a new library reads from the files: copies of `files` (without the folder's sidecar)
/// in a folder of `outer`, watched by a second app whose home is `outer`, are found at `at` within
/// 1e-6° (its first scan imports the file's GPS).
fn read_back(exe: &Path, outer: &Path, files: &[PathBuf], at: [f64; 2]) {
    let folder = outer.join("Copied");
    std::fs::create_dir_all(&folder).unwrap();
    for f in files {
        std::fs::copy(f, folder.join(f.file_name().unwrap())).unwrap();
    }
    let proxy = Proxy::start();
    let (a, mut cl) = live::start_with(exe, outer, launch(&proxy, outer));
    let c = &mut cl;
    run(c, "folders.add", json!({ "path": folder }));
    let ids: Vec<Value> = c.photos().into_iter().map(|p| p["id"].clone()).collect();
    assert_eq!(ids.len(), files.len());
    for id in &ids {
        let p = place(c, id);
        assert!(p.is_some_and(|p| near(Some(p), at)), "a new library reads the place from the file: {p:?}, set {at:?}");
    }
    live::quit(a, cl);
    assert_eq!(proxy.connections(), Vec::<String>::new());
    println!("  a new library reading copies of the three files finds their place within 1e-6°");
}
