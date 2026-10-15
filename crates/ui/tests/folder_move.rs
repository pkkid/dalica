//! Folder ▸ Move… (M9.20, `organizing.md` §9), driven headlessly with a scripted folder picker on
//! temporary folders under a watched folder (`photos`: `Trip`, `Home`, `Archive`):
//!
//! - the menu item opens the folder picker for the current folder, starting in its parent; the
//!   folder picked runs `folder.move`: the folder moves on disk with its photos, keeps its id, and
//!   the tree follows (in tree mode its row moves under its new parent); one undo step, which
//!   Edit ▸ Undo takes back;
//! - the next Move Folder starts where the last one went (`lastMoveFolder`, saved in the
//!   settings);
//! - a folder outside the library is refused with the reason and nothing moves; a cancel moves
//!   nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::FolderId;
use dalica_engine::Session;
use dalica_ui::panels::folders::{self, model::Kind};
use dalica_ui::{App, Driver, ScriptedPicker};
use serde_json::json;

const FRAMES: usize = 60 * 60;

struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: FolderId,
    trip: FolderId,
    picker: ScriptedPicker,
    d: Driver,
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap()
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("photos");
    for (folder, n) in [("Trip", 3), ("Home", 2), ("Archive", 1)] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        for i in 0..n {
            std::fs::write(root.join(folder).join(format!("{folder}_{i}.jpg")), jpeg(i + folder.len() as u64 * 10)).unwrap();
        }
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    let (home, trip) = (s.library().folder_by_path(&root.join("Home")).unwrap(), s.library().folder_by_path(&root.join("Trip")).unwrap());
    let picker = ScriptedPicker::new();
    let mut d = Driver::new(App::with_picker(Some(s), Arc::new(picker.clone())), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, root, home, trip, picker, d }
}

fn path_of(d: &Driver, f: FolderId) -> PathBuf {
    d.app().session().unwrap().library().folder(f).unwrap().path.clone()
}

fn jpegs(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |r| r.filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "jpg")).count())
}

/// The tree's rows top to bottom as (row id, depth), headers left out.
fn rows(d: &Driver) -> Vec<(String, usize)> {
    folders::current_model(d.ctx())
        .items
        .iter()
        .filter_map(|it| match &it.kind {
            Kind::Row(r) => Some((r.id.clone(), r.depth)),
            _ => None,
        })
        .collect()
}

fn row(f: FolderId) -> String {
    format!("tree:folder:{}", f.get())
}

fn depth(d: &Driver, f: FolderId) -> Option<usize> {
    rows(d).into_iter().find(|(id, _)| *id == row(f)).map(|(_, depth)| depth)
}

fn undo_label(d: &Driver) -> Option<&'static str> {
    d.app().session().unwrap().undo_stack().undo_label()
}

/// Runs frames until the picker's answer has run and the file jobs are done.
fn settle_jobs(d: &mut Driver) {
    let s = d.settle_for(FRAMES, true);
    assert!(s.settled, "still busy: {:?}", s.busy);
}

/// Folder ▸ Move… on the current folder, clicked.
fn move_by_menu(d: &mut Driver) {
    d.click("menu:Folder").unwrap();
    d.settle(FRAMES).unwrap();
    d.click("menuitem:folder.move").unwrap();
    settle_jobs(d);
}

fn select_folder(d: &mut Driver, f: FolderId) {
    d.run("select.section", &json!({ "section": format!("folder:{f}") })).unwrap();
    d.settle(FRAMES).unwrap();
}

/// Move Folder through the scripted picker moves the folder on disk with its id, the tree follows
/// it, Edit ▸ Undo moves it back; the next Move Folder starts where the last one went.
#[test]
fn move_folder_through_the_picker_moves_it_and_the_tree_follows() {
    let mut fx = fixture();
    let (home, trip) = (fx.home, fx.trip);
    let archive = fx.root.join("Archive");
    // Tree mode: the folders under the watched folder, one level down.
    fx.d.click("button:tree.tree").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(depth(&fx.d, home), Some(1));

    // The picker opens in the folder's parent; the folder picked runs folder.move.
    select_folder(&mut fx.d, home);
    let opener = dalica_ui::commands::opener("folder.move").unwrap();
    assert_eq!((opener.open)(fx.d.app_mut(), &json!({})).unwrap(), json!({ "dialog": "filePicker" }));
    let request = fx.d.app().folder_request().cloned().unwrap();
    assert_eq!((request.command, request.folder.as_deref(), request.params), ("folder.move", Some(fx.root.as_path()), json!({ "folder": home })));
    settle_jobs(&mut fx.d);
    assert_eq!(path_of(&fx.d, home), fx.root.join("Home"), "an empty picker is a cancel: nothing moved");
    assert_eq!(undo_label(&fx.d), None);

    fx.picker.push_folder(&archive);
    move_by_menu(&mut fx.d);
    let moved = archive.join("Home");
    assert!(!fx.root.join("Home").exists() && jpegs(&moved) == 2, "moved on disk with its photos");
    assert_eq!(path_of(&fx.d, home), moved, "the folder keeps its id");
    assert_eq!(undo_label(&fx.d), Some("Move Folder"));
    fx.d.settle(FRAMES).unwrap();
    // The tree follows: Home is now a level deeper, right under Archive.
    let archive_id = fx.d.app().session().unwrap().library().folder_by_path(&archive).unwrap();
    let r = rows(&fx.d);
    let at = |f: FolderId| r.iter().position(|(id, _)| *id == row(f)).unwrap();
    assert_eq!((depth(&fx.d, archive_id), depth(&fx.d, home)), (Some(1), Some(2)));
    assert_eq!(at(home), at(archive_id) + 1, "under Archive: {r:?}");
    assert!(fx.d.widgets().iter().any(|w| w.id == row(home)), "its row is drawn");

    // The next Move Folder starts in Archive, remembered in the settings.
    select_folder(&mut fx.d, trip);
    (opener.open)(fx.d.app_mut(), &json!({})).unwrap();
    assert_eq!(fx.d.app().folder_request().and_then(|r| r.folder.clone()), Some(archive.clone()));
    assert_eq!(fx.d.app().session().unwrap().settings().organize().last_move_folder, Some(archive.clone()));
    settle_jobs(&mut fx.d);
    assert_eq!(path_of(&fx.d, trip), fx.root.join("Trip"));

    // Edit ▸ Undo moves it back, and the tree with it.
    fx.d.click("menu:Edit").unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click("menuitem:edit.undo").unwrap();
    settle_jobs(&mut fx.d);
    assert!(jpegs(&fx.root.join("Home")) == 2 && !moved.exists());
    assert_eq!(path_of(&fx.d, home), fx.root.join("Home"));
    assert_eq!(depth(&fx.d, home), Some(1));
}

/// A folder outside the library is refused with the reason; nothing moves and no step is made.
#[test]
fn a_folder_outside_the_library_is_refused() {
    let mut fx = fixture();
    let home = fx.home;
    let outside = tempfile::tempdir().unwrap();
    select_folder(&mut fx.d, home);
    fx.picker.push_folder(outside.path());
    move_by_menu(&mut fx.d);
    assert_eq!(fx.d.state().status, dalica_engine::cmd::folder_ops::NOT_IN_LIBRARY);
    assert_eq!(path_of(&fx.d, home), fx.root.join("Home"));
    assert!(jpegs(&fx.root.join("Home")) == 2 && std::fs::read_dir(outside.path()).unwrap().next().is_none());
    assert_eq!(undo_label(&fx.d), None);
    // Into itself: refused too.
    fx.picker.push_folder(fx.root.join("Home"));
    move_by_menu(&mut fx.d);
    assert_eq!(fx.d.state().status, dalica_engine::cmd::folder_ops::INTO_ITSELF);
    assert_eq!(undo_label(&fx.d), None);
}
