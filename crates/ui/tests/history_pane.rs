//! The History pane (M16.33, issue #726; `ui-layout.md` §7, `editing.md` §5.1), driven headless:
//! opened from its bottom-bar toggle and View ▸ History, it lists the open photo's steps with the
//! current one marked, and a click on an earlier row jumps there in one command, the later steps
//! kept redoable; in the library it follows the selection, asking for one photo when several are
//! selected. Screenshots at 1400 × 900 and 900 × 600 go to the test output (`target/test-output/ui/`,
//! `history-pane-*`).

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_fits, new_driver, put_jpeg, shot, try_widget, widget};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::state::{Pane, View};
use dalica_ui::{App, Driver};
use serde_json::json;

/// Three photos, the second open in the edit view with three edits, in a `w × h` window.
fn editing(dir: &std::path::Path, (w, h): (u32, u32)) -> (Driver, PhotoId, Vec<PhotoId>) {
    let folder = dir.join("Photos").join("Lisbon");
    for i in 0..3 {
        put_jpeg(&folder.join(format!("IMG_{i:04}.jpg")), i);
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    let ids: Vec<PhotoId> = ids.into_iter().map(|(_, id)| id).collect();
    let shown = ids[1];
    for op in ["crop(0,0,0.5,1)", "straighten(1.4)", "tuning(0.3,0,0,0,none)"] {
        s.execute("photo.history.apply", &json!({ "id": shown, "op": op })).unwrap();
    }
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d.app_mut().run("view.edit", &json!({ "id": shown })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().view, d.state().edit.photo), (View::Edit, Some(shown)));
    (d, shown, ids)
}

fn edits(d: &Driver, id: PhotoId) -> (usize, usize) {
    let info = d.app().session().unwrap().library().info(id);
    (info.edits.len(), info.redo.len())
}

/// The ids of the rows shown, and which one is marked current.
fn rows(d: &Driver) -> (Vec<String>, Vec<String>) {
    let mut all: Vec<_> = d.widgets().into_iter().filter(|w| w.id.starts_with("history:")).collect();
    all.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    let active = all.iter().filter(|w| w.active).map(|w| w.id.clone()).collect();
    (all.into_iter().map(|w| w.id).collect(), active)
}

#[test]
fn the_toggle_opens_the_pane_and_a_click_jumps_back_a_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, shown, _) = editing(dir.path(), LARGE);
    assert!(try_widget(&d, "pane:history").is_none());

    d.click("toggle:pane.history").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().pane, Some(Pane::History));
    assert!(widget(&d, "toggle:pane.history").active, "its toggle is on");
    let (ids, active) = rows(&d);
    assert_eq!(ids, ["history:0", "history:1", "history:2", "history:3"], "Original and the three edits, oldest at the top");
    assert_eq!(active, ["history:3"], "the last edit is current");
    assert_eq!(widget(&d, "history:1").label, "Crop 3:4", "a 48 × 64 crop of the 96 × 64 photo");
    shot(&d, "history-pane-edit-1400x900");

    // Back a step, then to the original: each one command, the later steps kept.
    d.click("history:2").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(edits(&d, shown), (2, 1), "Tuning undone and redoable");
    assert_eq!(rows(&d), (vec!["history:0".into(), "history:1".into(), "history:2".into(), "history:3".into()], vec!["history:2".into()]));
    d.click("history:0").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(edits(&d, shown), (0, 3), "Original: no edits, all three redoable");
    shot(&d, "history-pane-original-1400x900");
    d.click("history:3").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(edits(&d, shown), (3, 0), "and forward again to the last");
    assert_eq!(d.app().session().unwrap().undo_stack().len(), 0, "never a library step");

    // View ▸ History closes it, checked while open.
    assert!(dalica_ui::menus::checked(d.app(), "pane.history", &json!({})));
    d.app_mut().run("pane.history", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().pane, None);
}

#[test]
fn in_the_library_it_follows_the_selection_and_fits_the_small_window() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, shown, ids) = editing(dir.path(), SMALL);
    d.key("Escape").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().view, View::Library);
    d.app_mut().run("pane.history", &json!({})).unwrap();
    d.app_mut().run("select.set", &json!({ "ids": [shown] })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(rows(&d).0.len(), 4, "the selected photo's steps");
    check_fits(&d, "the History pane at 900 × 600");
    shot(&d, "history-pane-library-900x600");

    d.app_mut().run("select.set", &json!({ "ids": [ids[0], shown] })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(rows(&d).0.is_empty(), "two photos: no rows");
    assert!(try_widget(&d, "pane:history").is_some(), "the pane stays, asking for one photo");
    shot(&d, "history-pane-two-photos-900x600");

    d.app_mut().run("view.edit", &json!({ "id": shown })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(rows(&d).0.len(), 4, "the edit view shows the open photo");
    check_fits(&d, "the History pane in the edit view at 900 × 600");
    shot(&d, "history-pane-edit-900x600");
}
