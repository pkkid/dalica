//! Rotations and flips in the library (M6.24, `ui-layout.md` §5.3 and §6.3), driven headlessly.
//!
//! A landscape photo stored with each of the eight EXIF orientations (each displays the same
//! upright picture) is rotated clockwise from the bottom bar's button: its cell turns portrait in
//! the very next frame, drawn from the cached thumbnail (the covering ladder size, `Drawn::Image`)
//! turned at paint time, and matches `rotated-cw.json`; a flip then mirrors it (`rotated-cw-flip`),
//! and undo brings back the rotated, then the original cell, pixel for pixel. No decode and no disk
//! load of a thumbnail ran for any of it (the session's thumbnail counters). The tray's mini turns
//! with the cell. Renders go to `target/test-output/ui/`; `DALICA_BLESS=1` stores new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_edit::Dihedral;
use dalica_engine::Session;
use dalica_scenes::exif::Exif;
use dalica_scenes::{Scene, fingerprint};
use dalica_ui::panels::thumbnails::{Drawn, Painted};
use dalica_ui::{App, Driver};
use dalica_ui_kit::raster::Image;
use serde_json::json;

const FRAMES: usize = 60 * 60;

/// One landscape scenes JPEG (600 × 400 as displayed) stored with EXIF orientation `o`.
fn library(dir: &Path, o: u8) -> Session {
    let folder = dir.join("photos").join("Turns");
    std::fs::create_dir_all(&folder).unwrap();
    let exif = Exif { orientation: o, ..Exif::default() };
    let bytes = dalica_scenes::jpeg(&Scene::new(24, 600, 400), 92, Some(&exif)).unwrap();
    std::fs::write(folder.join("landscape.jpg"), bytes).unwrap();
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s
}

fn the_photo(d: &Driver) -> PhotoId {
    d.app().session().unwrap().library().photos().next().unwrap().id
}

/// The decodes and disk loads of thumbnails so far.
fn work(d: &Driver) -> (u64, u64) {
    let s = d.app().session().unwrap().thumbnail_stats();
    (s.decodes, s.loads)
}

fn cell_rect(d: &Driver, id: PhotoId) -> [f32; 4] {
    let key = format!("thumb:{id}");
    d.widgets().into_iter().find(|w| w.id == key).unwrap_or_else(|| panic!("no {key}")).rect
}

/// What the last frame painted of `id` inside its light-table cell.
fn painted(d: &Driver, id: PhotoId) -> Painted {
    let cell = cell_rect(d, id);
    let inside = |p: &Painted| p.rect.min.x >= cell[0] - 0.5 && p.rect.max.x <= cell[0] + cell[2] + 0.5 && p.rect.min.y >= cell[1] - 0.5;
    d.app().panels().unwrap().thumbnails.painted().find(|p| p.photo == id && inside(p)).unwrap_or_else(|| panic!("{id} not painted in its cell"))
}

/// The pixels of `rect` (points = pixels at scale 1) of a screenshot.
fn region(shot: &Image, rect: [f32; 4]) -> (u32, u32, Vec<u8>) {
    let (x0, y0) = (rect[0].round() as u32, rect[1].round() as u32);
    let (w, h) = (rect[2].round() as u32, rect[3].round() as u32);
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

/// Checks `id`'s cell in the last frame against `<name>.json`; the render goes to `<label>.png`.
fn check_cell(d: &Driver, id: PhotoId, name: &str, label: &str) {
    let shot = d.screenshot().unwrap();
    let (w, h, rgba) = region(&shot, cell_rect(d, id));
    std::fs::write(out_dir().join(format!("{label}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    std::fs::write(out_dir().join(format!("{label}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() && label == name {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{label}-actual.png")), &m.actual_png).unwrap();
        panic!("{label}: {}", m.message);
    }
}

/// The pixels of the image `p` painted, from the last frame.
fn image_pixels(d: &Driver, p: &Painted) -> (u32, u32, Vec<u8>) {
    let r = p.rect;
    region(&d.screenshot().unwrap(), [r.min.x, r.min.y, r.width(), r.height()])
}

/// Mean absolute difference per channel of two equally sized RGBA buffers, `b` mirrored
/// left-right.
fn mirrored_difference(w: u32, a: &[u8], b: &[u8]) -> f64 {
    // The bottom strip holds the badge row (the edited mark of a rotated photo), drawn the same way
    // round on both: compare the photo above it.
    let badges = (dalica_ui_kit::widgets::BADGE_PLATE + dalica_ui_kit::widgets::BADGE_INSET).ceil() as usize;
    let (w, h) = (w as usize, (a.len() / 4 / w as usize).saturating_sub(badges));
    let mut sum = 0u64;
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let pa = a[(y * w + x) * 4 + c];
                let pb = b[(y * w + (w - 1 - x)) * 4 + c];
                sum += u64::from(pa.abs_diff(pb));
            }
        }
    }
    sum as f64 / (w * h * 3) as f64
}

#[test]
fn a_rotation_from_the_bottom_bar_turns_the_cell_from_the_cached_thumbnail_for_every_exif_orientation() {
    for o in 1..=8u8 {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Driver::new(App::new(Some(library(dir.path(), o))), 1400, 900, 1.0);
        d.settle(FRAMES).unwrap();
        let id = the_photo(&d);
        let p = d.app().session().unwrap().library().photo(id).unwrap().clone();
        assert_eq!((p.width, p.height), (600, 400), "o{o}: displayed upright");
        let before = painted(&d, id);
        assert_eq!((before.drawn, before.turn), (Drawn::Image, Dihedral::IDENTITY), "o{o}");
        assert!(before.rect.width() > before.rect.height(), "o{o}: a landscape cell");
        let original = image_pixels(&d, &before);
        let original_cell = region(&d.screenshot().unwrap(), cell_rect(&d, id));

        // Select it (the tray shows it), then the bottom bar's Rotate clockwise.
        d.click(&format!("thumb:{id}")).unwrap();
        d.settle(FRAMES).unwrap();
        let start = work(&d);
        d.click("button:rotate.cw").unwrap();
        d.step();
        let info = d.app().session().unwrap().library().info(id).clone();
        assert_eq!(info.edits, vec!["rotate(90)".to_string()], "o{o}");
        // The very next frame: portrait, the same cached image, turned.
        let turned = painted(&d, id);
        assert_eq!((turned.drawn, turned.turn), (Drawn::Image, Dihedral::ROTATE_90), "o{o}");
        assert!(turned.rect.height() > turned.rect.width(), "o{o}: a portrait cell: {:?}", turned.rect);
        assert!((turned.rect.width() - before.rect.height()).abs() < 0.5 && (turned.rect.height() - before.rect.width()).abs() < 0.5, "o{o}");
        // The tray's mini turns too.
        let minis: Vec<Painted> = d.app().panels().unwrap().thumbnails.painted().filter(|p| p.photo == id && p.rect != turned.rect).collect();
        assert!(!minis.is_empty() && minis.iter().all(|m| m.turn == Dihedral::ROTATE_90 && m.rect.height() > m.rect.width()), "o{o}: {minis:?}");
        d.settle(FRAMES).unwrap();
        d.key("Ctrl+D").unwrap();
        d.settle(FRAMES).unwrap();
        check_cell(&d, id, "rotated-cw", &if o == 1 { "rotated-cw".to_string() } else { format!("rotated-cw-o{o}") });
        let rotated = image_pixels(&d, &painted(&d, id));

        // A flip mirrors it (the flip acts on the selection).
        d.click(&format!("thumb:{id}")).unwrap();
        d.key("Ctrl+Shift+H").unwrap();
        d.key("Ctrl+D").unwrap();
        d.settle(FRAMES).unwrap();
        let flipped = painted(&d, id);
        assert_eq!(flipped.turn, Dihedral::ROTATE_90.then(Dihedral::FLIP_H), "o{o}");
        assert_eq!(flipped.rect, turned.rect, "o{o}: still portrait");
        check_cell(&d, id, "rotated-cw-flip", &if o == 1 { "rotated-cw-flip".to_string() } else { format!("rotated-cw-flip-o{o}") });
        let (w, _, mirror) = image_pixels(&d, &flipped);
        let diff = mirrored_difference(w, &rotated.2, &mirror);
        assert!(diff < 2.0, "o{o}: the flip is the rotated cell mirrored (mean difference {diff:.2})");

        // Undo: the rotated cell, then the original one, as they were.
        d.key("Ctrl+Z").unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(image_pixels(&d, &painted(&d, id)), rotated, "o{o}: one undo: the rotated cell");
        d.key("Ctrl+Z").unwrap();
        d.settle(FRAMES).unwrap();
        let back = painted(&d, id);
        assert_eq!((back.turn, back.rect), (Dihedral::IDENTITY, before.rect), "o{o}");
        assert_eq!(image_pixels(&d, &back), original, "o{o}: two undos: the original cell");
        assert_eq!(region(&d.screenshot().unwrap(), cell_rect(&d, id)), original_cell, "o{o}: the whole cell as before");
        assert!(d.app().session().unwrap().library().info(id).edits.is_empty(), "o{o}");

        assert_eq!(work(&d), start, "o{o}: no decode and no thumbnail load ran for the rotation, the flip or the undos");
    }
}

#[test]
fn the_tray_menus_rotations_act_on_the_tray() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Driver::new(App::new(Some(library(dir.path(), 1))), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    let id = the_photo(&d);
    d.click(&format!("thumb:{id}")).unwrap();
    d.settle(FRAMES).unwrap();
    let start = work(&d);
    // The tray's menu rows run the same commands on the tray's photos.
    let nodes = dalica_ui::panels::bottom::tray::context_nodes(d.app(), &[id]);
    let row = |cmd: &str| {
        nodes
            .iter()
            .find_map(|n| match n {
                dalica_ui::menus::Node::Item(i) if i.id == cmd => Some(i.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {cmd} row"))
    };
    for cmd in ["photo.rotate.cw", "photo.rotate.ccw"] {
        let r = row(cmd);
        assert!(r.live && r.enabled, "{cmd}: {:?}", r.reason);
        assert_eq!(r.params, json!({ "ids": [id] }));
    }
    let ccw = row("photo.rotate.ccw");
    d.run(ccw.id, &ccw.params).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.app().session().unwrap().library().info(id).edits, vec!["rotate(270)".to_string()]);
    let p = painted(&d, id);
    assert_eq!(p.turn, Dihedral::ROTATE_270);
    assert!(p.rect.height() > p.rect.width());
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join("rotated-ccw-tray-900x600.png"), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    // The bottom bar's counterclockwise button turns it back: a net whole turn leaves no edit.
    d.click("button:rotate.cw").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.app().session().unwrap().library().info(id).edits.is_empty());
    assert_eq!(painted(&d, id).turn, Dihedral::IDENTITY);
    assert_eq!(work(&d), start, "no decode ran");
}
