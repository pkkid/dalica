//! The Photo Tray in the edit view follows the open photo's edits (`roadmap.md` M7.23; `editing.md`
//! §9: edits show on every thumbnail). The edit view draws no light table, so the thumbnails it
//! paints (the tray's minis) ask for their better images at the start of the next frame
//! (`Thumbnails::begin_frame`): after an Auto Fix through the photo's own history, the tray's mini
//! of the open photo draws a texture of the new recipe, not the unedited one, and comes back to the
//! unedited picture with the undo.

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_scenes::Scene;
use dalica_ui::panels::thumbnails::{Drawn, Painted};
use dalica_ui::{App, Driver};
use serde_json::json;

/// Two landscape scenes JPEGs (600 × 400) in one folder.
fn library(dir: &Path) -> Session {
    let folder = dir.join("photos").join("Tray");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..2u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(40 + i, 600, 400), 92, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s
}

/// What the last frame painted of `id` (in the edit view: the tray's mini).
fn painted(d: &Driver, id: PhotoId) -> Painted {
    d.app().panels().unwrap().thumbnails.painted().find(|p| p.photo == id).unwrap_or_else(|| panic!("{id} not painted"))
}

/// The texture the frame drew for `id` is of the photo's current recipe.
fn shows_current(d: &Driver, id: PhotoId) -> bool {
    let p = painted(d, id);
    matches!(p.drawn, Drawn::Image | Drawn::Upscaled) && !p.stale
}

#[test]
fn the_tray_mini_of_the_open_photo_follows_its_edits_in_the_edit_view() {
    let dir = tempfile::tempdir().unwrap();
    let session = library(dir.path());
    let mut d = new_driver(App::new(Some(session)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let id = d.app().session().unwrap().library().photos().min_by_key(|p| p.name.clone()).unwrap().id;
    d.run("view.edit", &json!({ "id": id })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(shows_current(&d, id), "the tray's mini of the open photo before any edit: {:?}", painted(&d, id));

    d.run("photo.history.apply", &json!({ "id": id, "op": "autofix()" })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.app().state().view, dalica_ui::state::View::Edit, "still in the edit view");
    assert!(shows_current(&d, id), "after Auto Fix the tray's mini draws the edited picture, not the old one: {:?}", painted(&d, id));

    d.run("photo.history.undo", &json!({ "id": id })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(shows_current(&d, id), "after the undo the tray's mini draws the unedited picture again: {:?}", painted(&d, id));
}
