//! The tree's Projects group (M8.21, `ui-layout.md` §4, `creating.md` §2), driven headlessly: an
//! export made into a folder outside every watched root (the default location, `Pictures/Dalica/
//! Exports`) is listed in flat mode under Projects, above Folders, with the project icon; in tree
//! mode there is no Projects group and the folder is an ordinary row.
//!
//! The tree region is checked against fingerprints at 1400 × 900 (at 900 × 600 they only have to fit)
//! (`region-tree-projects-1400.json`); renders go to `target/test-output/ui/`, and `DALICA_BLESS=1`
//! stores new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, check_tree, new_driver, widget};
use dalica_core::{PhotoId, SystemClock};
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_ui::panels::folders;
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::json;

/// A library watching `Photos` (its `Lisbon` folder holds 3 photos), with `Pictures` as the
/// Pictures folder, after an export of them into the default location as "Lisbon Weekend"; the
/// export folder.
fn exported(dir: &Path) -> (Session, PathBuf) {
    let lisbon = dir.join("Photos").join("Lisbon");
    dalica_fs::create_dir_all(&lisbon).unwrap();
    for i in 0..3u64 {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 240, 160), 85, None).unwrap();
        dalica_fs::replace(&lisbon.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(SystemClock)).unwrap();
    s.set_pictures_dir(Some(dir.join("Pictures")));
    s.execute("folders.add", &json!({ "path": dir.join("Photos") })).unwrap();
    s.wait_idle();
    let ids: Vec<PhotoId> = s.library().photos().map(|p| p.id).collect();
    let location = PathBuf::from(s.execute("export.defaults", &json!({ "ids": ids })).unwrap()["location"].as_str().unwrap());
    assert_eq!(location, dir.join("Pictures").join("Dalica").join("Exports"));
    assert!(!s.roots().iter().any(|r| location.starts_with(r)), "the location lies outside every watched root");
    let job = s.execute("export.run", &json!({ "ids": ids, "dir": location, "folderName": "Lisbon Weekend", "size": 120 })).unwrap()["job"]
        .as_u64()
        .unwrap();
    assert!(s.wait_job(job, Duration::from_secs(300)).unwrap().state.is_finished());
    s.wait_idle();
    (s, location.join("Lisbon Weekend"))
}

fn driver(session: Session, (w, h): (u32, u32)) -> Driver {
    let mut d = new_driver(App::with_picker(Some(session), Arc::new(ScriptedPicker::new())), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn has(d: &Driver, id: &str) -> bool {
    d.widgets().iter().any(|w| w.id == id)
}

/// Whether the pane's model marks the row with widget id `id` a project.
fn is_project(d: &Driver, id: &str) -> bool {
    let model = folders::current_model(d.ctx());
    (0..model.items.len()).filter_map(|i| model.row(i)).find(|r| r.id == id).unwrap_or_else(|| panic!("no row {id}")).project
}

/// In flat mode the export folder is listed under Projects, above Folders, with the project icon;
/// in tree mode there is no Projects group and its row is an ordinary folder.
#[test]
fn an_export_folder_outside_every_watched_root_is_listed_in_projects() {
    let size = LARGE;
    let dir = tempfile::tempdir().unwrap();
    let (session, out) = exported(dir.path());
    let folder = session.library().folder_by_path(&out).expect("the export folder is in the library");
    let source = session.library().folder_by_path(&dir.path().join("Photos").join("Lisbon")).unwrap();
    let mut d = driver(session, size);
    assert_eq!(d.state().tree_mode, dalica_ui::state::TreeMode::Flat);

    let (projects, folders_header) = (widget(&d, "tree:group:projects"), widget(&d, "tree:group:folders"));
    assert_eq!(projects.label, "Projects (1)", "one project folder");
    let (row, source_row) = (format!("tree:folder:{folder}"), format!("tree:folder:{source}"));
    let r = widget(&d, &row);
    assert_eq!(r.label, "Lisbon Weekend (3)");
    assert!(projects.rect[1] < r.rect[1] && r.rect[1] < folders_header.rect[1], "under Projects, above Folders");
    assert!(widget(&d, &source_row).rect[1] > folders_header.rect[1], "the source stays under Folders");
    assert!(is_project(&d, &row) && !is_project(&d, &source_row), "only the export folder has the project icon");
    check_tree(&d, &format!("region-tree-projects-{}", size.0));

    // Tree mode has no Projects group: the export folder is a root of its own there.
    d.run("view.folderView", &json!({ "mode": "tree" })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!has(&d, "tree:group:projects"));
    assert!(has(&d, &row), "listed with the folders");
    assert!(!is_project(&d, &row));
    d.run("view.folderView", &json!({ "mode": "flat" })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(has(&d, "tree:group:projects") && is_project(&d, &row));
}

/// A folder under the projects directory (`Pictures/Dalica`) is listed in Projects without the
/// Exported mark, beside a folder outside it that stays under Folders. The projects directory stays
/// `Pictures/Dalica` whatever export location is chosen (M9.28): a location inside the watched
/// `Pictures` neither makes every folder of it a project nor takes Prints out of Projects.
#[test]
fn a_folder_under_the_projects_directory_is_listed_in_projects() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    let (prints, holiday) = (pictures.join("Dalica").join("Prints"), pictures.join("Holiday"));
    for (i, folder) in [&prints, &holiday].into_iter().enumerate() {
        dalica_fs::create_dir_all(folder).unwrap();
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 240, 160), 85, None).unwrap();
        dalica_fs::replace(&folder.join("IMG_0001.jpg"), &bytes, Expect::Absent).unwrap();
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(SystemClock)).unwrap();
    s.set_pictures_dir(Some(pictures.clone()));
    s.execute("folders.add", &json!({ "path": pictures })).unwrap();
    s.wait_idle();
    assert_eq!(s.projects_dir(), Some(pictures.join("Dalica")));
    let (p, h) = (s.library().folder_by_path(&prints).unwrap(), s.library().folder_by_path(&holiday).unwrap());
    assert!(s.library().project(p).is_none(), "no Exported mark");
    let mut d = driver(s, (1400, 900));
    let (row, other) = (format!("tree:folder:{p}"), format!("tree:folder:{h}"));
    assert_eq!(widget(&d, "tree:group:projects").label, "Projects (1)");
    let folders_header = widget(&d, "tree:group:folders");
    assert!(widget(&d, &row).rect[1] < folders_header.rect[1] && is_project(&d, &row), "Prints is a project");
    assert!(widget(&d, &other).rect[1] > folders_header.rect[1] && !is_project(&d, &other), "Holiday is a folder");

    // The export location moved into Pictures: the projects directory does not follow it.
    let session = d.app_mut().session_mut().unwrap();
    let export = session.settings().export();
    session.change_settings(|st| st.set_export(&dalica_engine::settings::ExportSettings { location: Some(pictures.join("Exports")), ..export }));
    assert_eq!(d.app().session().unwrap().projects_dir(), Some(pictures.join("Dalica")));
    d.settle(FRAMES).unwrap();
    assert_eq!(widget(&d, "tree:group:projects").label, "Projects (1)", "only Prints is a project");
    assert!(is_project(&d, &row) && !is_project(&d, &other));
}
