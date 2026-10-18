//! The photo viewer (M11.17, `viewing.md` §3), driven headlessly on a viewer session (the app as
//! `dalica --view FILE` opens it): a folder of five photos outside the library, the viewer opened
//! on the third.
//!
//! - it opens on the file's place showing the photo's pixels (decoded by path); → steps through the
//!   folder and the filmstrip follows; 1 toggles 100 %; Delete asks before anything goes, and
//!   Cancel leaves the library as it was; Ctrl+8 stars (the folder joins the library and the star
//!   reaches its sidecar); Space switches the background; E opens the edit view on the photo shown;
//! - the wheel zooms, and moves through the folder once Settings ▸ Interface says so;
//! - with "Hide controls automatically" on, the controls hide after 3 s without a mouse move (on
//!   the driver's virtual clock) and come back at the next move;
//! - while another Dalica holds the library, the changes are greyed with "Dalica is already open";
//! - fingerprints of the viewer, its Settings dialog and Options ▸ Photo Viewer at 1400 × 900,
//!   each fitting 900 × 600 (`tests/fingerprints/viewer-*.json`, `dialog-viewer-settings-*.json`,
//!   `dialog-options-viewer-*.json`; `DALICA_BLESS=1` stores new ones, renders go to
//!   `target/test-output/ui/`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, try_widget, widget};
use dalica_engine::Session;
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::viewer::{CANVAS_ID, Layout, bar, strip};
use dalica_ui::state::{Backdrop, View, Zoom};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, pos2, vec2};
use serde_json::json;

const NAMES: [&str; 5] = ["A.jpg", "B.jpg", "C.jpg", "D.jpg", "E.jpg"];

/// Five 1200 × 800 photos in `photos/Trip`, an empty library, and the viewer opened on C.jpg;
/// "Hide controls automatically" off so the controls stay for the fingerprints.
fn fixture(dir: &Path) -> (Session, PathBuf) {
    let folder = dir.join("photos").join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    for (i, name) in NAMES.iter().enumerate() {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40 + i as u64, 1200, 800), 85, None).unwrap();
        std::fs::write(folder.join(name), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("options.set", &json!({ "values": { "viewer.hideControls": false } })).unwrap();
    s.open_viewer(&folder.join("C.jpg")).unwrap();
    (s, folder)
}

/// How long the viewer's workers may take to decode the pictures and make the thumbnails (real
/// time: frames run as fast as the machine allows and say nothing about how long a worker had).
const PATIENCE: Duration = Duration::from_secs(120);

/// Settles with the pictures decoded by path and the filmstrip's thumbnails in (`ui.settle` does
/// not count them): frames run, each yielding a millisecond to the workers, until they are.
fn settle_with_thumbnails(d: &mut Driver) {
    d.settle(FRAMES).unwrap();
    let start = Instant::now();
    while !d.app().session().is_some_and(|s| s.thumbnails_pending() == 0 && s.viewer_images_pending() == 0) {
        assert!(start.elapsed() < PATIENCE, "the pictures and thumbnails are not in after {PATIENCE:?}");
        d.step();
        std::thread::sleep(Duration::from_millis(1));
    }
    d.settle(FRAMES).unwrap();
}

fn index(d: &Driver) -> usize {
    d.state().viewer.map(|v| v.index).unwrap()
}

/// The filmstrip cell shown as current.
fn current_cell(d: &Driver) -> String {
    let cells: Vec<String> = d.widgets().into_iter().filter(|w| w.id.starts_with("film:viewer/") && w.active).map(|w| w.id).collect();
    assert_eq!(cells.len(), 1, "{cells:?}");
    cells[0].clone()
}

/// Moves the pointer to the top middle of the window, over nothing but the overlay.
fn move_pointer(d: &mut Driver, x: f32) {
    d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: pos2(x, 20.0) }], PointerButton::Primary, Modifiers::NONE).unwrap();
}

/// A mouse move over the window, whatever the pointer did before: the pointer's first position
/// in a window is no move (it has nothing to move from), so it comes in and then moves.
fn mouse_move(d: &mut Driver) {
    move_pointer(d, 700.0);
    move_pointer(d, 640.0);
}

#[test]
fn the_viewer_steps_zooms_stars_and_opens_the_photo_in_dalica() {
    let dir = tempfile::tempdir().unwrap();
    let (s, folder) = fixture(dir.path());
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    settle_with_thumbnails(&mut d);
    assert_eq!(d.state().view, View::Viewer);
    assert_eq!((index(&d), current_cell(&d)), (2, strip::film_id(2)), "it opens on the file's place");
    assert_eq!(widget(&d, CANVAS_ID).label, "C.jpg (1200 × 800)");
    // The photo's own pixels, decoded by path, fill the middle of the photo area.
    let middle = Layout::of(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(LARGE.0 as f32, LARGE.1 as f32))).photo.center();
    let px = d.screenshot().unwrap().pixel(middle.x as u32, middle.y as u32).unwrap();
    assert!(u32::from(px[0]) + u32::from(px[1]) + u32::from(px[2]) > 120, "the photo is drawn, not a frame on the overlay: {px:?}");
    check_sizes(&mut d, CANVAS_ID, "viewer");

    // → steps, and the filmstrip follows; at the end it stops.
    d.key("Right").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((index(&d), current_cell(&d)), (3, strip::film_id(3)));
    d.key("K").unwrap();
    d.key("K").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(index(&d), 4, "the last photo stays");
    d.click(&strip::film_id(1)).unwrap();
    assert_eq!(index(&d), 1, "a click on the filmstrip jumps");

    // 1 toggles 100 %.
    d.key("1").unwrap();
    assert_eq!(d.state().viewer.unwrap().zoom, Zoom::Scale(1.0));
    d.key("1").unwrap();
    assert_eq!(d.state().viewer.unwrap().zoom, Zoom::Fit);

    // Delete asks first about this one picture; Cancel keeps it and leaves the library as it was.
    d.key("Delete").unwrap();
    let asked = dalica_ui::dialogs::inspect(d.app());
    assert_eq!((asked["kind"].as_str(), asked["command"]["id"].as_str()), (Some("deletePhotos"), Some("viewer.delete")), "{asked}");
    assert_eq!(
        dalica_ui::dialogs::delete::photos_text(d.app(), &asked["command"]["params"]).as_deref(),
        Some("Move the selected picture to the trash?")
    );
    d.click("button:dialog.cancel").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(folder.join("B.jpg").exists() && index(&d) == 1);
    let lib = d.app().session().unwrap().library();
    assert_eq!((lib.folders().count(), lib.photos().count()), (0, 0), "no folder added");

    // Ctrl+8 stars the photo shown (its folder joins the library); the button shows it.
    d.key("Ctrl+8").unwrap();
    d.settle(FRAMES).unwrap();
    let starred: Vec<String> = d.app().session().unwrap().library().photos().filter(|p| p.star).map(|p| p.name.clone()).collect();
    assert_eq!(starred, ["B.jpg"]);
    assert!(widget(&d, "button:viewer.star").active);

    // Space switches the background.
    d.key("Space").unwrap();
    assert_eq!(d.state().viewer.unwrap().backdrop, Backdrop::Opaque);
    d.key("Space").unwrap();
    assert_eq!(d.state().viewer.unwrap().backdrop, Backdrop::Translucent);

    // The wheel zooms; Settings ▸ Interface makes it move through the folder.
    d.scroll(Some(CANVAS_ID), vec2(0.0, 40.0)).unwrap();
    assert!(matches!(d.state().viewer.unwrap().zoom, Zoom::Scale(s) if s < 1.0), "{:?}", d.state().viewer);
    d.run("viewer.settings", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    check_sizes(&mut d, "dialog:viewerSettings", "dialog-viewer-settings");
    d.click("choice:option.viewer.wheel/navigate").unwrap();
    d.click("button:dialog.ok").unwrap();
    assert!(d.state().dialogs.is_empty());
    d.scroll(Some(CANVAS_ID), vec2(0.0, -40.0)).unwrap();
    assert_eq!((index(&d), d.state().viewer.unwrap().zoom), (2, Zoom::Fit), "the wheel moved to the next photo");

    // Options ▸ Photo Viewer.
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("tab:options.viewer").unwrap();
    d.settle(FRAMES).unwrap();
    check_sizes(&mut d, "dialog:options", "dialog-options-viewer");
    d.click("button:dialog.cancel").unwrap();

    // E: the edit view on the photo shown.
    d.key("E").unwrap();
    d.settle(FRAMES).unwrap();
    let id = d.app().session().unwrap().library().photos().find(|p| p.name == "C.jpg").map(|p| p.id);
    assert_eq!((d.state().view, d.state().edit.photo), (View::Edit, id));

    // The star reached the folder's sidecar.
    let mut app = d.into_app();
    app.session_mut().unwrap().flush().unwrap();
    let sidecar: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap();
    assert_eq!(sidecar["photos"]["B.jpg"]["star"], true, "{sidecar}");
}

#[test]
fn the_controls_hide_after_three_seconds_without_a_mouse_move() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = fixture(dir.path());
    s.execute("options.set", &json!({ "values": { "viewer.hideControls": true } })).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    // The settle's frames of virtual time may pass 3 s (a slow decode): the times count from the
    // move.
    d.settle(FRAMES).unwrap();
    mouse_move(&mut d);
    d.wait(2500).unwrap();
    assert!(d.state().viewer.unwrap().controls && try_widget(&d, bar::BAR_ID).is_some(), "shown 2.5 s after the move");
    d.wait(800).unwrap();
    assert!(!d.state().viewer.unwrap().controls, "hidden after 3 s");
    assert!(try_widget(&d, bar::BAR_ID).is_none() && try_widget(&d, bar::CLOSE_ID).is_none(), "nothing left to click");
    move_pointer(&mut d, 760.0);
    d.wait(300).unwrap();
    assert!(d.state().viewer.unwrap().controls && try_widget(&d, bar::BAR_ID).is_some(), "back at the next move");
}

#[test]
fn while_another_dalica_holds_the_library_the_changes_are_greyed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Loose");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("x.jpg"), dalica_scenes::jpeg(&dalica_scenes::Scene::new(3, 300, 200), 80, None).unwrap()).unwrap();
    let scratch = dir.path().join("scratch");
    let s = Session::open_locked_viewer(&scratch, &folder.join("x.jpg"), Arc::new(dalica_core::SystemClock), &Default::default()).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    // "Hide controls automatically" is on (a locked session cannot change options), and the
    // settle's frames of virtual time pass its 3 s when the photo is slow to decode: a move shows
    // the controls, faded in 0.3 s later and well inside the 3 s.
    mouse_move(&mut d);
    d.wait(300).unwrap();
    for id in ["button:viewer.star", "button:viewer.rotateLeft", "button:viewer.openInDalica"] {
        let w = widget(&d, id);
        assert!(!w.enabled && w.label == dalica_engine::viewer::ALREADY_OPEN, "{id}: {w:?}");
    }
    assert!(matches!(d.run("viewer.toggleStar", &json!({})), Err(e) if e.to_string().contains(dalica_engine::viewer::ALREADY_OPEN)));
    assert!(widget(&d, "button:viewer.zoomIn").enabled, "viewing still works");
}
