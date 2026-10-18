//! M12's frames in the live app (`m12.rs` part 4): `dalica --headless` on a 1200 × 800 `scenes`
//! JPEG, with automation writing into the test's home, driven over its control channel:
//!
//! - Border applied from its panel at its defaults (outer band 20, inner band 5, in thousandths of
//!   the shorter side: 20 pixels in all): `photo.inspect`'s displayed size and `edit`'s
//!   `shown_size` are the photo grown by 20 pixels on every side;
//! - exported at the original size: the file is the grown picture, black outside, white inside, the
//!   photo's pixels unchanged within (to the JPEG's rounding);
//! - saved: the original is kept byte for byte in `.dalica-originals/`, the file holds the grown
//!   picture, the photo inside unchanged; an export after the Save is grown once, not twice.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, click, resize, run, secs};
use crate::common::{LARGE, decode, edits, mean_difference, panel, px, undo_label};
use crate::live::{self, Client, Launch};

const PHOTO: (u32, u32) = (1200, 800);
/// The Border's default bands: 20 + 5 thousandths of the shorter side.
const BAND: u32 = (20 + 5) * PHOTO.1 / 1000;
/// The photo's pixels inside a frame, against the original: a JPEG round trip's error.
const INSIDE: f64 = 3.0;

/// Exports the photo at its own size into `home/Exports/<name>`; the file written.
fn export(c: &mut Client, home: &Path, id: &Value, name: &str) -> PathBuf {
    let done = run(
        c,
        "export.run",
        json!({ "ids": [id], "dir": home.join("Exports"), "folderName": name, "numbers": false, "size": "original",
                "quality": "maximum", "watermark": false, "removeLocation": false }),
    );
    let written = done["result"]["written"].as_array().unwrap_or_else(|| panic!("export.run: {done}"));
    assert_eq!(written.len(), 1, "{done}");
    PathBuf::from(written[0].as_str().unwrap())
}

/// Checks a framed picture: grown by [`BAND`] on every side, the outer band black and the inner
/// white at the middle of the left edge, the photo inside as `plain` shows it.
fn framed(what: &str, bytes: &[u8], plain: &dalica_image::Rgba8) -> f64 {
    let img = decode(bytes);
    assert_eq!((img.width, img.height), (PHOTO.0 + 2 * BAND, PHOTO.1 + 2 * BAND), "{what}: the photo grown by {BAND} px on every side");
    let mid = img.height / 2;
    let outer = px(&img.data, img.width, 4, mid);
    let inner = px(&img.data, img.width, BAND - 2, mid);
    assert!(outer.iter().all(|v| *v < 24), "{what}: the outer band is black: {outer:?}");
    assert!(inner.iter().all(|v| *v > 231), "{what}: the inner band is white: {inner:?}");
    let d = mean_difference(&img, (BAND, BAND), plain, (0, 0), PHOTO);
    assert!(d < INSIDE, "{what}: the photo's pixels inside the frame are unchanged (mean difference {d:.2})");
    d
}

pub fn frame(exe: &Path, home: &Path) {
    let started = Instant::now();
    let pictures = home.join("Pictures");
    let folder = pictures.join("Framed");
    let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(5150, PHOTO.0, PHOTO.1), 95, None).unwrap();
    app::put(&folder.join("Framed.jpg"), &jpeg, 1);
    let plain = decode(&jpeg);
    let (a, mut cl) = live::start_with(exe, home, Launch::default().arg("--automation-write-root").arg(home));
    let c = &mut cl;
    resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    let id = c.photos()[0]["id"].clone();
    run(c, "view.edit", json!({ "id": id }));
    run(c, "edit.tab", json!({ "tab": "effects3" }));
    click(c, "effect:border");
    assert_eq!(panel(c)["name"], "border");
    click(c, "button:panel.apply");
    let recipe = edits(c, &id);
    assert_eq!(recipe, ["border(#000000,20,#ffffff,5,0,0)"], "Border at its defaults");
    assert_eq!(undo_label(c, &id), "Undo Border");
    let grown = (PHOTO.0 + 2 * BAND, PHOTO.1 + 2 * BAND);
    let shown = dalica_edit::recipe::shown_size(&recipe, PHOTO.0, PHOTO.1, dalica_edit::recipe::Cropping::Counted);
    assert_eq!(shown, grown, "edit's shown size");
    let displayed = c.inspect(&id)["displayed"].clone();
    assert_eq!(displayed, json!({ "width": grown.0, "height": grown.1 }), "photo.inspect's displayed size");

    let first = export(c, home, &id, "Before Save");
    let d_export = framed("the export", &std::fs::read(&first).unwrap(), &plain);

    let done = run(c, "photo.save", json!({ "ids": [id], "confirm": true }));
    assert_eq!(done["state"], "done", "{done}");
    let kept = std::fs::read(folder.join(".dalica-originals").join("Framed.jpg")).expect("the original kept in .dalica-originals/");
    assert!(kept == jpeg, "the original kept byte for byte");
    let d_save = framed("the saved file", &std::fs::read(folder.join("Framed.jpg")).unwrap(), &plain);
    let again = export(c, home, &id, "After Save");
    framed("an export after the Save", &std::fs::read(&again).unwrap(), &plain);
    assert_eq!(c.inspect(&id)["displayed"], displayed, "the saved photo shows at the grown size, once");
    println!(
        "  Border: shown, exported and saved at {} × {} (the photo grown by {BAND} px on every side; inside off by {d_export:.2} exported, {d_save:.2} saved); the original kept; an export after the Save framed once",
        grown.0, grown.1
    );
    live::quit(a, cl);
    println!("  M12 a frame's grown size in {:.1} s", secs(started));
}
