//! Edited photos in the light table (`roadmap.md` M7.9; `architecture.md` §6.5, `editing.md` §9),
//! driven headlessly: when a photo's recipe changes, its cell keeps drawing the texture it had
//! (never the placeholder, never its sketch) while the new thumbnail renders, held at a gate so the
//! frames in between are seen; the new texture then replaces it. After a crop the old texture
//! keeps its own shape, letterboxed inside the new one. The edited badge shows with the
//! edit, a rotation alone included, and goes with its undo or a Save. The cell fingerprints at
//! 1400 × 900 and 900 × 600 (`edited-cell-*.json`). Renders go to `target/test-output/ui/`;
//! `DALICA_BLESS=1` stores new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_engine::thumbs::ScanHook;
use dalica_scenes::{Scene, fingerprint};
use dalica_ui::panels::light_table::cell::letterbox;
use dalica_ui::panels::thumbnails::{Drawn, Painted};
use dalica_ui::{App, Driver};
use dalica_ui_kit::raster::Image;
use dalica_ui_kit::widgets::Badges;
use serde_json::json;

const FRAMES: usize = 60 * 60;

/// Three landscape scenes JPEGs (600 × 400) in one folder.
fn library(dir: &Path) -> Session {
    let folder = dir.join("photos").join("Edits");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..3u64 {
        let bytes = dalica_scenes::jpeg(&Scene::new(30 + i, 600, 400), 92, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{:04}.jpg", i + 1)), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s
}

fn first_photo(d: &Driver) -> PhotoId {
    let lib = d.app().session().unwrap().library();
    lib.photos().min_by_key(|p| p.name.clone()).unwrap().id
}

fn session(d: &mut Driver) -> &mut Session {
    d.app_mut().session_mut().unwrap()
}

fn cell_rect(d: &Driver, id: PhotoId) -> [f32; 4] {
    let key = format!("thumb:{id}");
    d.widgets().into_iter().find(|w| w.id == key).unwrap_or_else(|| panic!("no {key}")).rect
}

/// What the last frame painted of `id` in the light table.
fn painted(d: &Driver, id: PhotoId) -> Painted {
    d.app().panels().unwrap().thumbnails.painted().find(|p| p.photo == id).unwrap_or_else(|| panic!("{id} not painted"))
}

fn badges(d: &Driver, id: PhotoId) -> Badges {
    d.app().panels().unwrap().light_table.cells.get(id).unwrap().badges
}

fn region(shot: &Image, rect: [f32; 4], scale: f32) -> (u32, u32, Vec<u8>) {
    let (x0, y0) = ((rect[0] * scale).round() as u32, (rect[1] * scale).round() as u32);
    let (w, h) = ((rect[2] * scale).round() as u32, (rect[3] * scale).round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    (w, h, rgba)
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks `id`'s cell in the last frame against `<name>.json`; the render goes to `<name>.png`.
fn check_cell(d: &Driver, id: PhotoId, name: &str) {
    let shot = d.screenshot().unwrap();
    let (w, h, rgba) = region(&shot, cell_rect(d, id), 1.0);
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// Holds thumbnail scan jobs until opened.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn hook(&self) -> ScanHook {
        let gate = self.clone();
        Arc::new(move |_| {
            let (lock, cv) = &*gate.0;
            let g = lock.lock().unwrap();
            let _ = cv.wait_timeout_while(g, Duration::from_secs(60), |open| !*open).unwrap();
        })
    }

    fn open(&self) {
        let (lock, cv) = &*self.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}

/// Runs `frames` frames with the new thumbnail held at the gate; each one draws the cell's old
/// texture, flagged stale, with `badge` as the edited badge.
fn held_frames(d: &mut Driver, id: PhotoId, frames: usize, badge: bool, before: &Painted) {
    for n in 0..frames {
        d.step();
        let p = painted(d, id);
        assert!(matches!(p.drawn, Drawn::Image | Drawn::Upscaled), "frame {n}: the old texture, not {:?}", p.drawn);
        assert!(p.stale, "frame {n}: drawn from the earlier edits while the new thumbnail renders");
        assert_eq!((p.rect, p.turn), (before.rect, before.turn), "frame {n}");
        assert_eq!(badges(d, id).edited, badge, "frame {n}");
    }
    let totals = d.app().panels().unwrap().thumbnails.totals();
    assert_eq!(totals.placeholder_cells, 0, "no cell drew the placeholder");
}

#[test]
fn an_edited_cell_keeps_its_old_texture_until_the_new_one_lands_and_shows_the_badge_until_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Driver::new(App::new(Some(library(dir.path()))), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let id = first_photo(&d);
    let before = painted(&d, id);
    assert_eq!((before.drawn, before.stale), (Drawn::Image, false));
    assert!(!badges(&d, id).edited);
    let original = region(&d.screenshot().unwrap(), cell_rect(&d, id), 1.0);
    let sketches_before = d.app().panels().unwrap().thumbnails.totals().sketch_cells;

    // Auto Fix from the photo's own history, its new thumbnail held at the gate.
    let gate = Gate::default();
    session(&mut d).set_thumbnail_scan_hook(Some(gate.hook()));
    session(&mut d).execute("photo.history.apply", &json!({ "id": id, "op": "autofix()" })).unwrap();
    held_frames(&mut d, id, 30, true, &before);
    assert_eq!(d.app().panels().unwrap().thumbnails.totals().sketch_cells, sketches_before, "nor its sketch");
    assert_eq!(session(&mut d).thumbnail_stats().remade, 1, "its entries are being made again");
    gate.open();
    d.settle(FRAMES).unwrap();
    let after = painted(&d, id);
    assert_eq!((after.drawn, after.stale, after.rect), (Drawn::Image, false, before.rect), "the new texture");
    assert!(badges(&d, id).edited, "the edited badge");
    check_cell(&d, id, "edited-cell-1400x900");

    // Undo: the badge goes at once, the old texture stays until the unedited one is back.
    let gate = Gate::default();
    session(&mut d).set_thumbnail_scan_hook(Some(gate.hook()));
    session(&mut d).execute("photo.history.undo", &json!({ "id": id })).unwrap();
    held_frames(&mut d, id, 10, false, &before);
    gate.open();
    d.settle(FRAMES).unwrap();
    let back = painted(&d, id);
    assert_eq!((back.drawn, back.stale), (Drawn::Image, false));
    assert_eq!(region(&d.screenshot().unwrap(), cell_rect(&d, id), 1.0), original, "the cell as it was");
    assert_eq!(session(&mut d).thumbnail_stats().remade, 2);

    // A rotation alone is an unsaved edit too: the badge shows, as the section header counts it,
    // while nothing is made again (the cell turns the picture as it paints it).
    session(&mut d).set_thumbnail_scan_hook(None);
    session(&mut d).execute("photo.rotate.cw", &json!({ "ids": [id] })).unwrap();
    d.settle(FRAMES).unwrap();
    let turned = painted(&d, id);
    assert!(!turned.stale && badges(&d, id).edited, "a rotated photo carries the edited badge");
    assert!(session(&mut d).library().has_unsaved_edits(id));
    assert_eq!(session(&mut d).thumbnail_stats().remade, 2);

    // Saved into the file, the photo has nothing unsaved: the badge goes.
    session(&mut d).execute("photo.save", &json!({ "ids": [id], "confirm": true, "dontAsk": true })).unwrap();
    session(&mut d).wait_idle();
    d.settle(FRAMES).unwrap();
    assert!(!session(&mut d).library().has_unsaved_edits(id), "saved");
    assert!(!badges(&d, id).edited, "no badge after a Save");
}

/// A crop changes the cell's shape at once; while the cropped thumbnail renders, the old texture is
/// drawn at its own shape (3:2), letterboxed inside the new one (3:4) on the placeholder, never
/// stretched to it; the cropped texture then fills the new shape.
#[test]
fn a_newly_cropped_cell_draws_its_old_texture_at_its_own_shape() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Driver::new(App::new(Some(library(dir.path()))), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let id = first_photo(&d);
    let before = painted(&d, id);
    assert_eq!((before.drawn, before.stale), (Drawn::Image, false));
    let aspect = |r: egui::Rect| r.width() / r.height();
    assert!((aspect(before.rect) - 1.5).abs() < 0.01, "{before:?}");
    let [x, y, w, h] = cell_rect(&d, id);
    let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
    let cropped = letterbox(cell, 300, 400);

    // The middle half across: 300 × 400 of the 600 × 400 photo, its new thumbnail held at the gate.
    let gate = Gate::default();
    session(&mut d).set_thumbnail_scan_hook(Some(gate.hook()));
    session(&mut d).execute("photo.history.apply", &json!({ "id": id, "op": "crop(0.25,0,0.75,1)" })).unwrap();
    let placeholder = dalica_ui_kit::Tokens::default().placeholder.to_array();
    for n in 0..10 {
        d.step();
        let p = painted(&d, id);
        assert!(matches!(p.drawn, Drawn::Image | Drawn::Upscaled) && p.stale, "frame {n}: the old texture, {p:?}");
        assert!((aspect(p.rect) - 1.5).abs() < 0.01, "frame {n}: drawn at its own shape, not stretched: {p:?}");
        assert!((p.rect.width() - cropped.width()).abs() < 0.01, "frame {n}: as wide as the new shape {cropped:?}: {p:?}");
        assert!(cropped.contains_rect(p.rect) && (p.rect.center() - cropped.center()).length() < 0.01, "frame {n}: {p:?}");
    }
    // The bars above and below it inside the new shape are the placeholder.
    let shot = d.screenshot().unwrap();
    let (w, h, rgba) = region(&shot, [x, y, w, h], 1.0);
    std::fs::write(out_dir().join("cropped-cell-held-1400x900.png"), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let p = painted(&d, id);
    let bar = |y: f32| shot.pixel(cropped.center().x.round() as u32, y.round() as u32).unwrap();
    assert_eq!(bar((cropped.top() + p.rect.top()) / 2.0), placeholder, "the bar above");
    assert_eq!(bar((p.rect.bottom() + cropped.bottom()) / 2.0), placeholder, "the bar below");
    gate.open();
    d.settle(FRAMES).unwrap();
    let after = painted(&d, id);
    assert_eq!((after.drawn, after.stale), (Drawn::Image, false), "the cropped texture");
    assert!((after.rect.min - cropped.min).length() < 0.01 && (after.rect.max - cropped.max).length() < 0.01, "{after:?} fills {cropped:?}");
}

#[test]
fn the_edited_cell_at_900_by_600() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = library(dir.path());
    let id = s.library().photos().min_by_key(|p| p.name.clone()).unwrap().id;
    s.execute("photo.history.apply", &json!({ "id": id, "op": "autofix()" })).unwrap();
    s.update_thumbnail_recipes();
    s.wait_idle();
    let mut d = Driver::new(App::new(Some(s)), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    let p = painted(&d, id);
    assert_eq!((p.drawn, p.stale), (Drawn::Image, false));
    assert!(badges(&d, id).edited);
    check_cell(&d, id, "edited-cell-900x600");
}
