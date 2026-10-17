//! M11's *Photo viewer* in the live app (`m11.rs` part 4): `dalica --view FILE --headless` on a
//! folder of five `scenes` photos no library watches, `FILE` the third, driven over its control
//! channel:
//!
//! - it opens the photo viewer on that photo (`view: "viewer"`, `viewer.list` giving the folder in
//!   name order and the file's place), drawing the photo decoded from its path; the window matches
//!   `fingerprints/m11-1400x900-viewer.json`;
//! - → and ← (and K and J) step through the folder;
//! - Ctrl+8 stars the photo shown: its folder joins the library and the star lands in the
//!   folder's sidecar;
//! - E (Open in Dalica) turns the viewer into the main window's edit view on that photo.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::app::{self, idle, key, park, secs};
use crate::live::{self, Client, Launch};

const LARGE: (u32, u32) = (1400, 900);
const NAMES: [&str; 5] = ["A.jpg", "B.jpg", "C.jpg", "D.jpg", "E.jpg"];
const PATIENCE: Duration = Duration::from_secs(120);

/// `viewer.list`, asked again while it is pending.
fn viewer_list(c: &mut Client) -> Value {
    let start = Instant::now();
    loop {
        let r = c.exec("viewer.list", json!({}));
        if r["pending"] != true {
            return r;
        }
        assert!(start.elapsed() < PATIENCE, "viewer.list stayed pending: {r}");
        c.ok("ui.wait", json!({ "ms": 20 }));
    }
}

fn index(c: &mut Client) -> u64 {
    let st = c.state();
    st["viewer"]["index"].as_u64().unwrap_or_else(|| panic!("the viewer is open: {st}"))
}

/// Lets the viewer's pictures, decoded by path off the UI thread, and the filmstrip's thumbnails
/// come in: frames until two screenshots in a row are the same.
fn pictures_in(c: &mut Client, home: &Path) {
    let mut last: Option<Vec<u8>> = None;
    let start = Instant::now();
    loop {
        idle(c);
        c.ok("ui.wait", json!({ "ms": 100 }));
        let shot = live::screenshot(c, home, None, "m11-viewer-wait", LARGE.0, LARGE.1);
        if last.as_ref() == Some(&shot.rgba) {
            return;
        }
        assert!(start.elapsed() < PATIENCE, "the viewer never settled");
        last = Some(shot.rgba);
    }
}

pub fn viewer(exe: &Path, home: &Path) {
    let started = Instant::now();
    let folder: PathBuf = home.join("Photos").join("Trip");
    for (i, name) in NAMES.iter().enumerate() {
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(4000 + i as u64, 1200, 800), 85, None).unwrap();
        app::put(&folder.join(name), &jpeg, i as u64);
    }
    let file = folder.join("C.jpg");
    let (a, mut cl) = live::start_with(exe, home, Launch::default().arg("--view").arg(&file));
    let c = &mut cl;
    // The controls stay for the fingerprint (they hide after 3 s without a mouse move otherwise).
    c.exec("options.set", json!({ "values": { "viewer.hideControls": false } }));
    c.ok("ui.resize", json!({ "width": LARGE.0, "height": LARGE.1 }));

    // It opens on the file's place in its folder, listed in name order.
    let v = viewer_list(c);
    assert_eq!((v["file"].clone(), v["index"].clone(), v["readOnly"].clone()), (json!(file), json!(2), json!(false)), "{v}");
    let names: Vec<&str> = v["photos"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, NAMES, "{v}");
    let st = c.state();
    assert_eq!((st["view"].clone(), st["viewer"]["index"].clone()), (json!("viewer"), json!(2)), "{st}");
    let lib = c.exec("library.inspect", json!({}));
    assert_eq!(lib["counts"]["folders"], json!(0), "viewing adds nothing to the library: {lib}");
    park(c, f64::from(LARGE.1));
    pictures_in(c, home);
    let canvas = app::label(c, "canvas:viewer");
    assert_eq!(canvas, "C.jpg (1200 × 800)", "the photo shown, by name and size");
    live::screenshot(c, home, Some("m11-1400x900-viewer"), "m11-1400x900-viewer", LARGE.0, LARGE.1);
    println!("  dalica --view C.jpg: the viewer on the third of five, listed in name order (matches its fingerprint)");

    // Stepping through the folder.
    key(c, "Right");
    assert_eq!(index(c), 3, "→ steps to the next photo");
    key(c, "K");
    assert_eq!(index(c), 4, "K too");
    key(c, "Left");
    key(c, "J");
    assert_eq!(index(c), 2, "← and J step back");

    // Ctrl+8 stars the photo shown; its folder joins the library and the star reaches the sidecar.
    key(c, "Ctrl+8");
    idle(c);
    app::flush_until(c, || app::entry(&folder, "C.jpg").is_some_and(|e| e["star"] == true));
    let entry = app::entry(&folder, "C.jpg");
    assert_eq!(entry.as_ref().map(|e| e["star"].clone()), Some(json!(true)), "the star in the folder's sidecar: {entry:?}");
    let photos = app::photos(c, &folder);
    let starred: Vec<&String> = photos.iter().filter(|(_, p)| p.inspected["star"] == true).map(|(n, _)| n).collect();
    assert_eq!(starred, ["C.jpg"], "the folder is the library's now, C.jpg starred");
    println!("  Ctrl+8 starred C.jpg: its folder joined the library, the star is in the folder's sidecar");

    // E: Open in Dalica, the edit view on the photo.
    let id = photos["C.jpg"].id.clone();
    key(c, "E");
    idle(c);
    let st = c.state();
    assert_eq!((st["view"].clone(), st["edit"]["photo"].clone(), st["viewer"].clone()), (json!("edit"), id, Value::Null), "{st}");
    println!("  E opened C.jpg in the edit view");
    live::quit(a, cl);
    println!("  M11 viewer in {:.1} s", secs(started));
}
