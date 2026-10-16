//! The edit view's keys that `editing.md` §12 documents, pressed headless in the window (the
//! edit view's own key dispatch): Space stars the photo shown, Ctrl+Shift+S opens the Export
//! dialog for it, F2 (Rename…, a Global key) opens Rename Files for it.

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, new_driver, put_jpeg, try_widget};
use dalica_engine::Session;
use dalica_ui::App;
use dalica_ui::state::View;
use serde_json::json;

/// The window in the edit view on the second of three photos: the driver, the photo shown.
fn editing(dir: &std::path::Path) -> (dalica_ui::Driver, dalica_core::PhotoId) {
    let folder = dir.join("Photos").join("Lisbon");
    for i in 0..3 {
        put_jpeg(&folder.join(format!("IMG_{i:04}.jpg")), i);
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, dalica_core::PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    let shown = ids[1].1;
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.app_mut().run("view.edit", &json!({ "id": shown })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!((d.state().view, d.state().edit.photo), (View::Edit, Some(shown)));
    (d, shown)
}

#[test]
fn space_stars_and_ctrl_shift_s_exports_the_photo_shown() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, shown) = editing(dir.path());
    let starred = |d: &dalica_ui::Driver| d.app().session().unwrap().library().photo(shown).unwrap().star;

    d.key("Space").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(starred(&d), "Space starred the photo shown");
    d.key("Space").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!starred(&d), "and unstarred it");

    d.key("Ctrl+Shift+S").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "dialog:export").is_some(), "the Export dialog is open");
    assert_eq!(d.state().export.as_ref().map(|e| e.ids.clone()), Some(vec![shown]), "for the photo shown");
    assert_eq!(d.state().view, View::Edit, "over the edit view");
}

#[test]
fn f2_opens_rename_files_for_the_photo_shown() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, shown) = editing(dir.path());
    d.key("F2").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, "dialog:renameFiles").is_some(), "the Rename Files dialog is open");
    assert_eq!(d.state().rename_files.as_ref().map(|r| r.ids.clone()), Some(vec![shown]), "for the photo shown");
    assert_eq!(d.state().view, View::Edit, "over the edit view");
}
