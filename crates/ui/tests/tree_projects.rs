//! The tree's Projects group (M8.21, `ui-layout.md` §4, `creating.md` §2), driven headlessly: an
//! export made into a folder outside every watched root (the default location, `Pictures/Dalica/
//! Exports`) is listed in flat mode under Projects, above Folders, with the project icon; in tree
//! mode there is no Projects group and the folder is an ordinary row.
//!
//! The tree region is checked against fingerprints at 1400 × 900 and 900 × 600
//! (`region-tree-projects-<w>.json`); renders go to `target/test-output/ui/`, and `DALICA_BLESS=1`
//! stores new fingerprints.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::Session;
use dalica_fs::Expect;
use dalica_scenes::fingerprint;
use dalica_ui::panels::folders;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;
use serde_json::json;

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

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
    let mut d = Driver::new(App::with_picker(Some(session), Arc::new(ScriptedPicker::new())), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn widget(d: &Driver, id: &str) -> WidgetInfo {
    d.widgets()
        .into_iter()
        .find(|w| w.id == id)
        .unwrap_or_else(|| panic!("no `{id}` in {:?}", d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>()))
}

fn has(d: &Driver, id: &str) -> bool {
    d.widgets().iter().any(|w| w.id == id)
}

/// Whether the pane's model marks the row with widget id `id` a project.
fn is_project(d: &Driver, id: &str) -> bool {
    let model = folders::current_model(d.ctx());
    (0..model.items.len()).filter_map(|i| model.row(i)).find(|r| r.id == id).unwrap_or_else(|| panic!("no row {id}")).project
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the tree region (the pane and its splitter) of the last frame against `<name>.json`.
fn check_tree(d: &Driver, name: &str) {
    let shot = d.screenshot().unwrap();
    let (tree, splitter) = (widget(d, "region:tree").rect, widget(d, "region:splitter").rect);
    let x0 = tree[0].min(splitter[0]).round() as u32;
    let y0 = tree[1].min(splitter[1]).round() as u32;
    let x1 = ((tree[0] + tree[2]).max(splitter[0] + splitter[2]).round() as u32).min(shot.width);
    let y1 = ((tree[1] + tree[3]).max(splitter[1] + splitter[3]).round() as u32).min(shot.height);
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
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

/// In flat mode the export folder is listed under Projects, above Folders, with the project icon;
/// in tree mode there is no Projects group and its row is an ordinary folder.
#[test]
fn an_export_folder_outside_every_watched_root_is_listed_in_projects() {
    for size in SIZES {
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
}
