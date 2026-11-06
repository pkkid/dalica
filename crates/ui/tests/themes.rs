//! The window in every theme (M16.8; `design-system.md` §2 Themes), headless, by id: the library,
//! the edit view, the Options dialog and the Collage room are drawn in each theme of
//! [`ThemeKind::ALL`], and what each region paints most is its theme's token (the toolbar
//! `chrome`, the light table `grid_bg`, the edit canvas `canvas`, …), so a colour a panel takes
//! from Dark or from a literal shows up here; the main window is fingerprinted in every theme
//! (`theme-<id>-library-1400x900`) and the edit view in Pro (Light) and Classic
//! (`theme-<id>-edit-1400x900`), and switching back to Dark after the sweep gives Dark's
//! fingerprint unchanged; the viewer's filmstrip outlines its photo in white on its dark band in a
//! light theme; at 900 × 600 the library in Pro (Light) fits as in Dark. Renders go to
//! `target/test-output/ui/`; `DALICA_BLESS=1` stores new fingerprints.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_fits, check_pixels, new_driver, rect_pixels, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::panels::collage::PANEL_W;
use dalica_ui::panels::viewer::strip;
use dalica_ui::state::View;
use dalica_ui::{App, Driver};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::theme::ThemeKind;
use egui::Color32;
use serde_json::json;

/// A library in `root` holding one folder of eight scenes photos, added and scanned; its pictures
/// folder (where collages go) inside `root`.
fn session(root: &Path) -> (Session, Vec<PhotoId>) {
    let files = crate::common::demo_library(1, 8, 21).unwrap();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = root.join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(root.join("Pictures")));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 100 })).unwrap();
    let ids = found["photos"].as_array().unwrap().iter().map(|p| PhotoId::new(p["id"].as_u64().unwrap())).collect();
    (s, ids)
}

fn run(d: &mut Driver, id: &str, params: serde_json::Value) {
    d.run(id, &params).unwrap_or_else(|e| panic!("{id}: {e}"));
    d.settle(FRAMES).unwrap();
}

/// The colour `rect` (`[x, y, w, h]`) of `shot` paints most.
fn most(shot: &dalica_ui_kit::Image, rect: [f32; 4]) -> Color32 {
    let (_, _, rgba) = rect_pixels(shot, rect);
    let mut counts: HashMap<[u8; 4], usize> = HashMap::new();
    for p in rgba.as_chunks::<4>().0 {
        *counts.entry(*p).or_default() += 1;
    }
    let ([r, g, b, a], _) = counts.into_iter().max_by_key(|(c, n)| (*n, *c)).unwrap();
    Color32::from_rgba_premultiplied(r, g, b, a)
}

/// A region (by widget id), the token it paints most, and that token.
type Expect = (&'static str, &'static str, fn(&Tokens) -> Color32);

const LIBRARY: [Expect; 9] = [
    ("region:menuBar", "chrome", |t| t.chrome),
    ("region:toolbar", "chrome", |t| t.chrome),
    ("region:tree", "chrome", |t| t.chrome),
    ("region:lightTable", "grid_bg", |t| t.grid_bg),
    ("region:scrollbar", "inset", |t| t.inset),
    ("region:statusLine", "chrome", |t| t.chrome),
    ("region:tray", "tray_bg", |t| t.tray_bg),
    ("region:bottomControls", "chrome", |t| t.chrome),
    ("region:actionButtons", "chrome", |t| t.chrome),
];

const EDIT: [Expect; 8] = [
    ("region:menuBar", "chrome", |t| t.chrome),
    ("region:editTopBar", "chrome", |t| t.chrome),
    ("region:filmstrip", "chrome", |t| t.chrome),
    ("region:editPanel", "chrome", |t| t.chrome),
    ("region:canvas", "canvas", |t| t.canvas),
    ("region:captionBar", "canvas", |t| t.canvas),
    ("region:statusLine", "chrome", |t| t.chrome),
    ("region:tray", "tray_bg", |t| t.tray_bg),
];

const OPTIONS: [Expect; 1] = [("dialog:options", "chrome", |t| t.chrome)];

const COLLAGE: [Expect; 3] =
    [("region:menuBar", "chrome", |t| t.chrome), ("region:tabStrip", "canvas", |t| t.canvas), ("region:statusLine", "chrome", |t| t.chrome)];

/// A region given by its rect: a name, `[x, y, w, h]`, its token's name and the token.
type Extra<'a> = (&'a str, [f32; 4], &'a str, Color32);

/// Whether each region of `expect` (and of `extra`) paints its token most in `kind`: the ones that
/// do not.
fn check(d: &Driver, kind: ThemeKind, view: &str, expect: &[Expect], extra: &[Extra]) -> Vec<String> {
    let t = Tokens::get(d.ctx());
    assert_eq!(t.kind, kind, "the theme worn in {view}");
    let shot = d.screenshot().unwrap();
    let regions = expect.iter().map(|(id, token, of)| (*id, widget(d, id).rect, *token, of(&t)));
    let mut wrong = Vec::new();
    for (id, rect, token, want) in regions.chain(extra.iter().copied()) {
        let got = most(&shot, rect);
        if got != want {
            wrong.push(format!("{}: {view}: {id} paints {got:?}, not `{token}` {want:?}", kind.id()));
        }
    }
    wrong
}

/// The whole window against `tests/fingerprints/<name>.json`.
fn fingerprint(d: &Driver, name: &str) {
    let s = d.screenshot().unwrap();
    check_pixels(name, s.width, s.height, &s.rgba);
}

#[test]
fn every_region_paints_its_themes_token_and_dark_comes_back_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = session(dir.path());
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    // The library as every theme shows it: the first photo selected and its folder chosen, as a
    // return from the edit view leaves them.
    run(&mut d, "view.edit", json!({ "id": ids[0].get() }));
    run(&mut d, "view.library", json!({}));
    let mut wrong = Vec::new();
    for kind in ThemeKind::ALL {
        let id = kind.id();
        run(&mut d, &format!("theme.{id}"), json!({}));
        // The library.
        wrong.extend(check(&d, kind, "the library", &LIBRARY, &[]));
        fingerprint(&d, &format!("theme-{id}-library-1400x900"));
        // The edit view.
        run(&mut d, "view.edit", json!({ "id": ids[0].get() }));
        wrong.extend(check(&d, kind, "the edit view", &EDIT, &[]));
        if matches!(kind, ThemeKind::ProLight | ThemeKind::Classic) {
            fingerprint(&d, &format!("theme-{id}-edit-1400x900"));
        }
        run(&mut d, "view.library", json!({}));
        // The Options dialog, over the library.
        run(&mut d, "app.preferences", json!({}));
        wrong.extend(check(&d, kind, "the Options dialog", &OPTIONS, &[]));
        d.key("Esc").unwrap();
        d.settle(FRAMES).unwrap();
        assert!(d.state().options.is_none());
        // The Collage room: its left panel `inset`, the room's right edge (beside the page)
        // `grid_bg`.
        run(&mut d, "create.collage", json!({ "ids": &ids[..6], "seed": "5eed" }));
        let room = widget(&d, "region:collage").rect;
        let t = Tokens::get(d.ctx());
        let panel = [room[0], room[1] + 40.0, PANEL_W.min(room[2]), room[3] - 40.0];
        let edge = [room[0] + room[2] - 16.0, room[1], 16.0, room[3]];
        let extra = [("the left panel", panel, "inset", t.inset), ("the room's edge", edge, "grid_bg", t.grid_bg)];
        wrong.extend(check(&d, kind, "the Collage room", &COLLAGE, &extra));
        run(&mut d, "collage.closeRoom", json!({}));
    }
    assert!(wrong.is_empty(), "regions off their theme's token:\n{}", wrong.join("\n"));
    // Back to Dark: Dark's own fingerprint, unchanged by the themes before it.
    run(&mut d, "theme.dark", json!({}));
    assert_eq!(*d.app().tokens(), Tokens::DARK);
    fingerprint(&d, "theme-dark-library-1400x900");
}

#[test]
fn at_900_by_600_the_library_in_pro_light_fits_as_in_dark() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _) = session(dir.path());
    let mut d = new_driver(App::new(Some(s)), SMALL.0, SMALL.1, 1.0);
    d.settle(FRAMES).unwrap();
    check_fits(&d, "the library in Dark");
    let rects = |d: &Driver| d.widgets().into_iter().map(|w| (w.id, w.rect)).collect::<Vec<_>>();
    let dark = rects(&d);
    run(&mut d, "theme.proLight", json!({}));
    assert_eq!(Tokens::get(d.ctx()).kind, ThemeKind::ProLight);
    check_fits(&d, "the library in Pro (Light)");
    assert_eq!(rects(&d), dark, "the same widgets in the same places");
    crate::common::shot(&d, "theme-proLight-library-900x600");
}

/// How long the viewer's workers may take to decode the pictures and make the thumbnails.
const PATIENCE: Duration = Duration::from_secs(120);

#[test]
fn the_viewers_filmstrip_outlines_its_photo_in_white_in_a_light_theme() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Trip");
    for (i, name) in ["A.jpg", "B.jpg", "C.jpg"].iter().enumerate() {
        crate::common::put_jpeg(&folder.join(name), 40 + i as u64);
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("options.set", &json!({ "values": { "viewer.hideControls": false } })).unwrap();
    s.execute("theme.proLight", &json!({})).unwrap();
    s.open_viewer(&folder.join("B.jpg")).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    let start = Instant::now();
    while !d.app().session().is_some_and(|s| s.thumbnails_pending() == 0 && s.viewer_images_pending() == 0) {
        assert!(start.elapsed() < PATIENCE, "the thumbnails are not in after {PATIENCE:?}");
        d.step();
        std::thread::sleep(Duration::from_millis(1));
    }
    d.settle(FRAMES).unwrap();
    let t = Tokens::get(d.ctx());
    assert_eq!((t.kind, d.state().view), (ThemeKind::ProLight, View::Viewer));
    assert_eq!(t.photo_focus, Color32::BLACK, "a light theme's focus outline is black …");
    let cell = widget(&d, &strip::film_id(1)).rect;
    let shot = d.screenshot().unwrap();
    // The 2 pt outline outside the cell's square, on the band.
    let outline = shot.pixel((cell[0] - 1.0) as u32, (cell[1] + cell[3] / 2.0) as u32).unwrap();
    assert_eq!(outline, t.overlay_text.to_array(), "… but the viewer's band is dark in every theme, so its outline is `overlay_text`");
    crate::common::shot(&d, "theme-proLight-viewer-1400x900");
}
