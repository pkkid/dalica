//! The first-run panel (M5.18, `organizing.md` §1), driven headlessly by widget id on a new
//! library whose standard places are planted (`Session::set_places`, the test override):
//!
//! - It is offered only for a new library, by the window (`App::persist_view`), never for one with
//!   folders, the demo library, or a headless session.
//! - Continue with the default choice adds Pictures, Documents and Desktop as Scan Always (one undo
//!   step), and the library's folders appear in the tree while the scan still runs (a gated header
//!   reader holds it in its last folder).
//! - "Choose folders…" opens the Folder Manager instead.
//! - Every widget has an id (the driver checks every frame); it fits 900 × 600, and the panel matches its fingerprints
//!   (`tests/fingerprints/dialog-first-run-1400x900.json`; `DALICA_BLESS=1` stores new ones, renders
//!   go to `target/test-output/ui/`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::common::{FRAMES, LARGE, check_sizes, new_driver, put_jpeg, try_widget};
use dalica_engine::Session;
use dalica_scan::{HeaderReader, Places};
use dalica_ui::dialogs::first_run;
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use serde_json::json;

/// A new library and a home with `Pictures/A Trip` (4 photos) and `Pictures/Z Last` (2), and
/// empty `Documents` and `Desktop`, planted as the session's places.
fn new_library(dir: &Path) -> (Session, PathBuf) {
    let home = dir.join("home");
    for i in 0..4 {
        put_jpeg(&home.join("Pictures/A Trip").join(format!("IMG_{i:04}.jpg")), i);
    }
    for i in 0..2 {
        put_jpeg(&home.join("Pictures/Z Last").join(format!("IMG_{i:04}.jpg")), 10 + i);
    }
    std::fs::create_dir_all(home.join("Documents")).unwrap();
    std::fs::create_dir_all(home.join("Desktop")).unwrap();
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_places(Places::under(&home), Vec::new());
    assert!(s.is_new_library());
    (s, home)
}

/// Holds the scan's header reads in folder `Z Last` until opened.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn reader(&self) -> HeaderReader {
        let gate = self.clone();
        let inner = dalica_scan::default_reader();
        Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
            if path.parent().and_then(Path::file_name).is_some_and(|n| n == "Z Last") {
                let (lock, cv) = &*gate.0;
                let open = lock.lock().unwrap();
                let _ = cv.wait_timeout_while(open, Duration::from_secs(8), |open| !*open).unwrap();
            }
            inner(path, limits)
        })
    }

    fn open(&self) {
        let (lock, cv) = &*self.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}

fn has(d: &Driver, id: &str) -> bool {
    try_widget(d, id).is_some()
}

fn tree_rows(d: &Driver) -> Vec<String> {
    d.widgets().into_iter().filter(|w| w.id.starts_with("tree:folder:")).map(|w| w.label).collect()
}

/// The panel is the window's: offered for a new library only, once.
#[test]
fn first_run_is_offered_only_for_a_new_library_and_never_for_the_demo() {
    let dir = tempfile::tempdir().unwrap();
    // The window (it persists the view) offers it when it gets the library.
    let (s, _home) = new_library(dir.path());
    let mut app = App::new(None);
    app.persist_view();
    app.set_session(s);
    assert_eq!(app.state().dialogs.iter().map(|d| d.kind).collect::<Vec<_>>(), [DialogKind::FirstRun]);
    assert!(!first_run::offer(&mut app), "once");
    // A headless or snapshot session never shows it.
    let s = app.take_session().unwrap();
    let mut d = new_driver(App::new(Some(s)), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    assert!(d.state().dialogs.is_empty() && !has(&d, "dialog:firstRun"));
    // A library with folders, and the demo library, are not new.
    let s = d.into_app().take_session().unwrap();
    drop(s);
    let demo = tempfile::tempdir().unwrap();
    let mut app = App::new(Some(crate::common::demo_open(demo.path(), Arc::new(dalica_core::SystemClock))));
    assert!(!first_run::offer(&mut app), "never for the demo library");
    app.persist_view();
    assert!(app.state().dialogs.is_empty());
    let other = tempfile::tempdir().unwrap();
    let (mut s, home) = new_library(other.path());
    s.execute("folders.add", &json!({ "path": home.join("Pictures") })).unwrap();
    let mut app = App::new(Some(s));
    assert!(!first_run::offer(&mut app), "a library with a folder is not new");
}

/// Continue with the default choice adds the standard places as Scan Always in one undo step, and
/// the folders show up in the tree while the scan is still running.
#[test]
fn first_run_with_the_default_choice_adds_the_standard_places_and_folders_appear_while_the_scan_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, home) = new_library(dir.path());
    let gate = Gate::default();
    s.set_scan_reader(Some(gate.reader()));
    let mut app = App::new(Some(s));
    assert!(first_run::offer(&mut app));
    let mut d = new_driver(app, 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    assert!(has(&d, "dialog:firstRun"));
    assert!(try_widget(&d, "choice:dialog.start/places").unwrap().active, "the default choice");
    assert!(!try_widget(&d, "choice:dialog.start/choose").unwrap().active);
    assert!(tree_rows(&d).is_empty());
    d.click("button:dialog.continue").unwrap();
    // Continue closes the panel; the default-viewer question follows (M15.24).
    assert_eq!(d.state().dialogs.iter().map(|d| d.kind).collect::<Vec<_>>(), [DialogKind::DefaultViewer], "{:?}", d.state().status);
    let s = d.app().session().unwrap();
    let watched: Vec<PathBuf> =
        s.folder_rules().iter().filter(|(_, r)| r.mode.map(|m| m.id()) == Some("always")).map(|(p, _)| p.to_path_buf()).collect();
    assert_eq!(watched, [home.join("Desktop"), home.join("Documents"), home.join("Pictures")]);
    assert_eq!(s.undo_stack().len(), 1, "one undo step");
    assert!(!s.is_new_library());
    // The first folder appears while the scan waits in the last one.
    let mut seen = false;
    for _ in 0..4000 {
        d.step();
        if tree_rows(&d).iter().any(|r| r.starts_with("A Trip")) {
            seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(seen, "the first folder never appeared: {:?}", tree_rows(&d));
    assert!(d.app().session().unwrap().busy(), "the scan is still running");
    assert!(!tree_rows(&d).iter().any(|r| r.starts_with("Z Last")));
    gate.open();
    d.settle(FRAMES).unwrap();
    let mut rows = tree_rows(&d);
    rows.sort();
    assert_eq!(rows, ["A Trip (4)", "Z Last (2)"]);
    assert_eq!(d.app().session().unwrap().library().photos().count(), 6);
}

/// "Choose folders…" opens the Folder Manager instead, and adds nothing.
#[test]
fn choose_folders_opens_the_folder_manager() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _home) = new_library(dir.path());
    let mut app = App::new(Some(s));
    assert!(first_run::offer(&mut app));
    let mut d = new_driver(app, 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    d.click("choice:dialog.start/choose").unwrap();
    assert!(try_widget(&d, "choice:dialog.start/choose").unwrap().active);
    assert_eq!(d.state().dialogs[0].fields["choice"], "choose");
    d.click("button:dialog.continue").unwrap();
    assert_eq!(d.state().dialogs.iter().map(|d| d.kind).collect::<Vec<_>>(), [DialogKind::DefaultViewer, DialogKind::FolderManager]);
    assert!(d.app().session().unwrap().folder_rules().is_empty());
    d.settle(FRAMES).unwrap();
    assert!(has(&d, "dialog:folderManager"));
}

/// The panel has its parts at about 640 × 460, matches its fingerprint, and fits the small window.
#[test]
fn the_panel_matches_its_fingerprint_and_fits_the_small_window() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _home) = new_library(dir.path());
    let mut app = App::new(Some(s));
    assert!(first_run::offer(&mut app));
    let mut d = new_driver(app, LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    for id in ["dialog:firstRun", "dialog:firstRun/backdrop", "choice:dialog.start/places", "choice:dialog.start/choose", "button:dialog.continue"] {
        assert!(has(&d, id), "{id}");
    }
    let r = try_widget(&d, "dialog:firstRun").unwrap().rect;
    assert!((r[2] - first_run::WIDTH).abs() <= 4.0 && (r[3] - first_run::HEIGHT).abs() <= 16.0, "≈ 640 × 460, not {r:?}");
    check_sizes(&mut d, "dialog:firstRun", "dialog-first-run");
}
