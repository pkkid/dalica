//! The saved picture shows at once after a Save (`data-model.md` §6, `editing.md` §9), driven
//! headlessly: the light table's cell of a photo is uploaded, the photo is opened in the edit view,
//! edited and saved there. The tray's mini then draws the saved file (the edited picture), not the
//! texture uploaded before the edit, and back in the light table so does the cell: its texture,
//! uploaded from the original file with no edits, is not current for the saved file with no edits.
//! Judged by the mean colour of what was drawn against the pictures before and after.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scenes::Scene;
use dalica_ui::panels::thumbnails::{Drawn, Painted};
use dalica_ui::state::View;
use dalica_ui::{App, Driver};
use serde_json::json;

const FRAMES: usize = 60 * 60;
/// A strong brightening: the edited picture's mean colour is far from the original's.
const EDIT: &str = "tuning(0.6,0,0,0.4,none)";

/// Two landscape scenes JPEGs (600 × 400) in one folder.
fn library(dir: &Path) -> (Session, PathBuf) {
    let folder = dir.join("photos").join("Saved");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..2u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(50 + i, 600, 400), 92, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    (s, folder)
}

/// What the last frame painted of `id` (the light table's cell, or in the edit view the tray's mini).
fn painted(d: &Driver, id: PhotoId) -> Painted {
    d.app().panels().unwrap().thumbnails.painted().find(|p| p.photo == id).unwrap_or_else(|| panic!("{id} not painted"))
}

/// The mean colour of the last frame inside `id`'s painted image rect (a pixel in from its edges),
/// and what it drew there.
fn drawn_mean(d: &Driver, id: PhotoId) -> ([f64; 3], Painted) {
    let p = painted(d, id);
    let shot = d.screenshot().unwrap();
    let x0 = (p.rect.min.x.ceil() as u32).saturating_add(1);
    let y0 = (p.rect.min.y.ceil() as u32).saturating_add(1);
    let x1 = (p.rect.max.x.floor() as u32).saturating_sub(1).min(shot.width);
    let y1 = (p.rect.max.y.floor() as u32).saturating_sub(1).min(shot.height);
    assert!(x1 > x0 + 8 && y1 > y0 + 8, "{id}: a drawn rect too small: {:?}", p.rect);
    let mut rgba = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    (mean(&rgba), p)
}

fn mean(rgba: &[u8]) -> [f64; 3] {
    let n = (rgba.len() / 4).max(1) as f64;
    let mut m = [0.0; 3];
    for px in rgba.as_chunks::<4>().0 {
        for (c, v) in m.iter_mut().zip(px) {
            *c += f64::from(*v) / n;
        }
    }
    m
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
}

/// The cell drew a texture of the photo as it is now, at its covering size.
fn current(p: &Painted) -> bool {
    p.drawn == Drawn::Image && !p.stale
}

#[test]
fn after_a_save_the_tray_and_the_cell_show_the_saved_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (session, folder) = library(dir.path());
    let mut d = crate::common::new_driver(App::new(Some(session)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let (id, name) = {
        let lib = d.app().session().unwrap().library();
        let p = lib.photos().min_by_key(|p| p.name.clone()).unwrap();
        (p.id, p.name.clone())
    };
    let (cell_original, p) = drawn_mean(&d, id);
    assert!(current(&p), "the light table's cell is uploaded: {p:?}");

    // The edit view: the tray's mini before and after the edit.
    d.run("view.edit", &json!({ "id": id })).unwrap();
    d.settle(FRAMES).unwrap();
    let (tray_original, p) = drawn_mean(&d, id);
    assert!(current(&p), "the tray's mini before the edit: {p:?}");
    d.run("photo.history.apply", &json!({ "id": id, "op": EDIT })).unwrap();
    d.settle(FRAMES).unwrap();
    let (tray_edited, p) = drawn_mean(&d, id);
    assert!(current(&p), "the tray's mini after the edit: {p:?}");
    assert!(distance(tray_edited, tray_original) > 10.0, "the edit shows in the tray: {tray_original:?} → {tray_edited:?}");

    // Save, in the edit view.
    d.run("photo.save", &json!({ "ids": [id], "confirm": true, "dontAsk": true })).unwrap();
    assert!(d.settle_for(FRAMES, true).settled, "the save and its thumbnails settle");
    assert_eq!(d.app().state().view, View::Edit, "still in the edit view");
    let session = d.app().session().unwrap();
    assert!(session.library().info(id).edits.is_empty() && !session.library().has_unsaved_edits(id), "saved");
    let (tray_saved, p) = drawn_mean(&d, id);
    assert!(current(&p), "the tray's mini after the save: {p:?}");
    assert!(
        distance(tray_saved, tray_edited) < 4.0 && distance(tray_saved, tray_original) > 10.0,
        "the tray's mini shows the saved picture, not the original: original {tray_original:?}, edited {tray_edited:?}, after the save {tray_saved:?}"
    );

    // Back in the light table, the cell draws the saved file too.
    d.run("view.library", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let (cell_saved, p) = drawn_mean(&d, id);
    assert!(current(&p), "the cell after the save: {p:?}");
    let bytes = std::fs::read(folder.join(&name)).unwrap();
    let on_disk = mean(&dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap().data);
    assert!(
        distance(cell_saved, on_disk) < 4.0 && distance(cell_saved, cell_original) > 10.0,
        "the cell shows the saved picture, not the original: original {cell_original:?}, file {on_disk:?}, after the save {cell_saved:?}"
    );
}
