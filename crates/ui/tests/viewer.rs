//! The photo viewer (M11.17, `viewing.md` §3), driven headlessly on a viewer session (the app as
//! `dalica --view FILE` opens it): a folder of five photos the library holds, the viewer opened on
//! the third.
//!
//! - it opens on the file's place; → steps through the folder and the filmstrip follows; 1
//!   toggles 100 %; Ctrl+8 stars (the star reaches the folder's sidecar); Space switches the
//!   background; Delete asks before anything goes; E opens the edit view on the photo shown;
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

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, try_widget, widget};
use dalica_engine::Session;
use dalica_ui::headless::{PointerEvent, PointerKind};
use dalica_ui::panels::viewer::{CANVAS_ID, bar, strip};
use dalica_ui::state::{Backdrop, DialogKind, View, Zoom};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, pos2, vec2};
use serde_json::json;

const NAMES: [&str; 5] = ["A.jpg", "B.jpg", "C.jpg", "D.jpg", "E.jpg"];

/// Five 1200 × 800 photos in `photos/Trip`, a library holding the folder, and the viewer opened
/// on C.jpg; "Hide controls automatically" off so the controls stay for the fingerprints.
fn fixture(dir: &Path) -> (Session, PathBuf) {
    let folder = dir.join("photos").join("Trip");
    std::fs::create_dir_all(&folder).unwrap();
    for (i, name) in NAMES.iter().enumerate() {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(40 + i as u64, 1200, 800), 85, None).unwrap();
        std::fs::write(folder.join(name), bytes).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s.execute("options.set", &json!({ "values": { "viewer.hideControls": false } })).unwrap();
    s.open_viewer(&folder.join("C.jpg")).unwrap();
    (s, folder)
}

/// Settles with the filmstrip's thumbnails in (`ui.settle` does not count them).
fn settle_with_thumbnails(d: &mut Driver) {
    d.settle(FRAMES).unwrap();
    d.step_until(|app| app.session().is_some_and(|s| s.thumbnails_pending() == 0), FRAMES).unwrap();
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

#[test]
fn the_viewer_steps_zooms_stars_and_opens_the_photo_in_dalica() {
    let dir = tempfile::tempdir().unwrap();
    let (s, folder) = fixture(dir.path());
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    settle_with_thumbnails(&mut d);
    assert_eq!(d.state().view, View::Viewer);
    assert_eq!((index(&d), current_cell(&d)), (2, strip::film_id(2)), "it opens on the file's place");
    assert_eq!(widget(&d, CANVAS_ID).label, "C.jpg (1200 × 800)");
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

    // Ctrl+8 stars the photo shown; the button shows it.
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

    // Delete asks first; Cancel keeps the photo.
    d.key("Delete").unwrap();
    assert_eq!(d.state().dialogs.last().map(|x| x.kind), Some(DialogKind::DeletePhotos));
    d.click("button:dialog.cancel").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(folder.join("B.jpg").exists() && index(&d) == 1);

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
    d.settle(FRAMES).unwrap();
    move_pointer(&mut d, 700.0);
    d.wait(2500).unwrap();
    assert!(d.state().viewer.unwrap().controls && try_widget(&d, bar::BAR_ID).is_some(), "shown 2.5 s after the move");
    d.wait(800).unwrap();
    assert!(!d.state().viewer.unwrap().controls, "hidden after 3 s");
    assert!(try_widget(&d, bar::BAR_ID).is_none() && try_widget(&d, bar::CLOSE_ID).is_none(), "nothing left to click");
    move_pointer(&mut d, 640.0);
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
    for id in ["button:viewer.star", "button:viewer.rotateLeft", "button:viewer.openInDalica"] {
        let w = widget(&d, id);
        assert!(!w.enabled && w.label == dalica_engine::viewer::ALREADY_OPEN, "{id}: {w:?}");
    }
    assert!(matches!(d.run("viewer.toggleStar", &json!({})), Err(e) if e.to_string().contains(dalica_engine::viewer::ALREADY_OPEN)));
    assert!(widget(&d, "button:viewer.zoomIn").enabled, "viewing still works");
}
