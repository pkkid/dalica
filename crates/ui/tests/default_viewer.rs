//! Dalica as the default viewer in the window (M15.24, `viewing.md` §3.3), headless and by widget
//! id with the recording `Os` standing for a window's system:
//!
//! - the viewer's Settings ▸ File Types shows each type with the fake's handler ("JPEG (Image
//!   Viewer)"); Select All then OK hands the system every type; "Don't use Dalica as a viewer"
//!   then OK hands it none;
//! - Options ▸ Photo Viewer ▸ File Types: a checkbox then Apply hands the system that type, and
//!   [Current] checks the types Dalica opens now; a system that cannot greys the part;
//! - the question after the first run appears once, nothing chosen, and Use Dalica hands the
//!   system the types checked.
//!
//! The PR's screenshots are written to `target/test-output/ui/default-viewer-*.png`.

use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, new_driver, put_jpeg, shot, try_widget, widget};
use dalica_engine::Session;
use dalica_engine::os::{Handler, OsCall, Recording, Service, viewer_file_types};
use dalica_scan::{FileType, Places};
use dalica_ui::dialogs::first_run;
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use serde_json::json;

/// A library with one photo, a windowed recording `Os` whose JPEG and PNG open in an image viewer
/// and whose GIF opens in Dalica.
fn setup(dir: &Path) -> (Driver, Arc<Recording>) {
    let folder = dir.join("Photos");
    put_jpeg(&folder.join("a.jpg"), 1);
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let os = Arc::new(Recording::windowed());
    let viewer = Handler { id: "org.example.Viewer.desktop".into(), name: "Image Viewer".into(), icon: Some("viewer".into()) };
    os.set_default(FileType::Jpeg, viewer.clone());
    os.set_default(FileType::Png, viewer);
    os.set_default(FileType::Gif, Handler::new("org.dalica.Dalica.desktop", "Dalica"));
    s.set_os(os.clone());
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    (d, os)
}

fn set_calls(os: &Recording) -> Vec<Vec<FileType>> {
    os.calls()
        .into_iter()
        .filter_map(|c| match c {
            OsCall::SetDefaultViewer(t) => Some(t),
            _ => None,
        })
        .collect()
}

#[test]
fn the_file_types_tab_shows_the_handlers_and_select_all_then_ok_sets_every_type() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, os) = setup(dir.path());
    d.run("viewer.settings", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("tab:viewerSettings.fileTypes").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(widget(&d, "check:option.viewer.fileType/jpeg").label, "JPEG (Image Viewer)");
    assert_eq!(widget(&d, "check:option.viewer.fileType/png").label, "PNG (Image Viewer)");
    assert_eq!(widget(&d, "check:option.viewer.fileType/gif").label, "GIF (Dalica)");
    assert_eq!(widget(&d, "check:option.viewer.fileType/bmp").label, "BMP", "the system does not say");
    assert!(widget(&d, "choice:option.viewer.defaultViewer/off").active, "none chosen yet");
    d.click("button:option.viewer.defaultViewer/selectAll").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(widget(&d, "choice:option.viewer.defaultViewer/on").active);
    assert!(widget(&d, "check:option.viewer.fileType/bmp").active);
    shot(&d, "default-viewer-settings-file-types");
    os.clear();
    d.click("button:dialog.ok").unwrap();
    assert!(d.state().dialogs.is_empty(), "{:?}", d.state().status);
    let every = viewer_file_types();
    assert_eq!(set_calls(&os), std::slice::from_ref(&every));
    assert_eq!(d.app().session().unwrap().settings().viewer().default_viewer, every);

    // Opened again it shows them chosen; "Don't use Dalica as a viewer" then OK hands it none.
    d.run("viewer.settings", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("tab:viewerSettings.fileTypes").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(widget(&d, "check:option.viewer.fileType/png").active);
    d.click("choice:option.viewer.defaultViewer/off").unwrap();
    d.settle(FRAMES).unwrap();
    os.clear();
    d.click("button:dialog.ok").unwrap();
    assert_eq!(set_calls(&os), [Vec::<FileType>::new()]);
}

#[test]
fn options_photo_viewer_sets_the_types_checked_and_current_checks_dalicas() {
    let dir = tempfile::tempdir().unwrap();
    let (mut d, os) = setup(dir.path());
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("tab:options.viewer").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(widget(&d, "check:option.viewer.fileType/jpeg").label, "JPEG (Image Viewer)");
    d.click("check:option.viewer.fileType/jpeg").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(widget(&d, "check:option.viewer.fileType/jpeg").active && widget(&d, "choice:option.viewer.defaultViewer/on").active);
    shot(&d, "default-viewer-options");
    os.clear();
    d.click("button:dialog.apply").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(set_calls(&os), [vec![FileType::Jpeg]]);
    // [Current]: the types Dalica opens now (GIF, as the system says).
    d.click("button:option.viewer.defaultViewer/current").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(widget(&d, "check:option.viewer.fileType/gif").active && !widget(&d, "check:option.viewer.fileType/jpeg").active);
    d.click("button:dialog.cancel").unwrap();

    // A system that cannot greys the part with its reason.
    os.set_unavailable(Service::DefaultViewer, Some("No way here"));
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("tab:options.viewer").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!widget(&d, "choice:option.viewer.defaultViewer/on").enabled);
    assert!(try_widget(&d, "check:option.viewer.fileType/jpeg").is_none(), "no types listed");
}

#[test]
fn the_question_after_the_first_run_appears_once_and_use_dalica_sets_the_types_checked() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    put_jpeg(&home.join("Pictures/Trip/IMG_0001.jpg"), 1);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_places(Places::under(&home), Vec::new());
    let os = Arc::new(Recording::windowed());
    os.set_default(FileType::Jpeg, Handler::new("org.example.Viewer.desktop", "Image Viewer"));
    s.set_os(os.clone());
    let mut app = App::new(Some(s));
    assert!(first_run::offer(&mut app));
    let mut d = new_driver(app, LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d.click("button:dialog.continue").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().dialogs.iter().map(|d| d.kind).collect::<Vec<_>>(), [DialogKind::DefaultViewer]);
    assert_eq!(widget(&d, "check:defaultViewer.type/jpeg").label, "JPEG (Image Viewer)");
    assert!(!widget(&d, "check:defaultViewer.type/jpeg").active, "nothing chosen");
    assert!(!widget(&d, "button:dialog.use").enabled);
    d.click("check:defaultViewer.type/jpeg").unwrap();
    d.click("check:defaultViewer.type/png").unwrap();
    d.settle(FRAMES).unwrap();
    shot(&d, "default-viewer-first-run");
    os.clear();
    d.click("button:dialog.use").unwrap();
    assert!(d.state().dialogs.is_empty());
    assert_eq!(set_calls(&os), [vec![FileType::Jpeg, FileType::Png]]);
    // Once: the first run is over, so neither it nor the question comes back.
    d.settle(FRAMES).unwrap();
    assert!(d.state().dialogs.is_empty());
    let mut app = d.into_app();
    assert!(!first_run::offer(&mut app));
    assert!(app.state().dialogs.is_empty());
}
