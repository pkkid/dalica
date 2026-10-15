//! Every interactive widget drawn has an automation id, unique within its frame and in the grammar
//! of `control-protocol.md` §7 (M2.1). The window is driven headlessly at 1400 × 900 and 900 × 600
//! through the states M1 can reach: a library open with nothing selected, a photo selected and
//! starred, the light table scrolled to a later folder, the File menu open, the Folder Manager
//! opened from File ▸ Add Folder… and the Remove Folder question of Folder ▸ Remove from Dalica….
//! In each, the list of interactive egui widgets with no id must be empty.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_engine::Session;
use dalica_scan::Places;
use dalica_ui::dialogs::folder_manager::draft::Draft;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry::{self, Role};

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// A library in `root` with two folders of 30 scenes photos each, both added and scanned.
fn session(root: &Path) -> Session {
    let files = dalica_scenes::demo_library(2, 60, 5).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    assert_eq!(folders.len(), 2);
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    // The Folder Manager lists planted places, not this machine's.
    std::fs::create_dir_all(root.join("home").join("Pictures")).unwrap();
    s.set_places(Places::under(&root.join("home")), Vec::new());
    for folder in &folders {
        s.execute("folders.add", &serde_json::json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    s
}

/// Checks the last frame: no unregistered interactive widget, unique ids in the spec grammar, every
/// rect on screen. Returns the widgets.
fn check(d: &Driver, (w, h): (u32, u32), state: &str) -> Vec<WidgetInfo> {
    let missing = d.app().panels().expect("the panels are drawn").unregistered_widgets();
    assert_eq!(missing, [], "{w}×{h}, {state}: interactive widgets without an id");
    let widgets = d.widgets();
    let mut seen = HashSet::new();
    for wi in &widgets {
        assert!(registry::is_valid_id(&wi.id), "{w}×{h}, {state}: `{}` breaks the id grammar", wi.id);
        assert!(seen.insert(wi.id.as_str()), "{w}×{h}, {state}: `{}` is registered twice", wi.id);
        let [x, y, rw, rh] = wi.rect;
        assert!(
            rw > 0.0 && rh > 0.0 && x >= 0.0 && y >= 0.0 && x + rw <= w as f32 + 0.01 && y + rh <= h as f32 + 0.01,
            "{w}×{h}, {state}: `{}` at {:?}",
            wi.id,
            wi.rect
        );
    }
    widgets
}

fn ids_of(widgets: &[WidgetInfo], role: Role) -> Vec<String> {
    widgets.iter().filter(|w| w.role == role).map(|w| w.id.clone()).collect()
}

fn find<'a>(widgets: &'a [WidgetInfo], id: &str) -> &'a WidgetInfo {
    widgets.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no `{id}` in {:?}", widgets.iter().map(|w| &w.id).collect::<Vec<_>>()))
}

/// The M2.1 done-when.
#[test]
fn every_interactive_widget_has_a_unique_id_at_both_window_sizes() {
    for size in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let session = session(dir.path());
        let mut d = new_driver(App::with_picker(Some(session), Arc::new(ScriptedPicker::new())), size.0, size.1, 1.0);
        d.settle(FRAMES).unwrap();

        // Opened, nothing selected: the menu title, the tree rows, the cells, a disabled star button
        // and the scroll bar (two folders of 30 do not fit).
        let widgets = check(&d, size, "opened");
        assert_eq!(find(&widgets, "menu:File").role, Role::Menu);
        let rows = ids_of(&widgets, Role::TreeRow);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.iter().all(|r| r.strip_prefix("tree:folder:").is_some_and(|n| n.parse::<u64>().is_ok())), "{rows:?}");
        let thumbs = ids_of(&widgets, Role::Thumbnail);
        assert!(thumbs.len() >= 6 && thumbs.iter().all(|t| t.strip_prefix("thumb:").is_some_and(|n| n.parse::<u64>().is_ok())), "{thumbs:?}");
        assert!(!find(&widgets, "button:star").enabled);
        let track = find(&widgets, "scroll:track").clone();
        let handle_before = find(&widgets, "scroll:thumb").rect;
        assert!(track.enabled && track.egui_id.is_some());
        let inside =
            |r: [f32; 4], o: [f32; 4]| r[0] >= o[0] && r[1] >= o[1] && r[0] + r[2] <= o[0] + o[2] + 0.01 && r[1] + r[3] <= o[1] + o[3] + 0.01;
        assert!(inside(handle_before, track.rect), "handle {handle_before:?} on track {:?}", track.rect);

        // A photo selected and starred: its badge is a part of its cell.
        let first = thumbs[0].clone();
        d.click(&first).unwrap();
        d.key("Ctrl+8").unwrap();
        d.settle(FRAMES).unwrap();
        let widgets = check(&d, size, "starred");
        let star = find(&widgets, "button:star");
        assert!(star.enabled && star.active && star.role == Role::Toggle);
        // Starred Photos shows it first, as `<thumb>/in/starred` (its folder's cell moved down).
        let copy = format!("{first}/in/starred");
        let badge = find(&widgets, &format!("{copy}/star"));
        assert!(badge.role == Role::Badge && badge.egui_id.is_none());
        assert!(inside(badge.rect, find(&widgets, &copy).rect));
        assert!(find(&widgets, &first).active, "the selected cell is active");
        assert!(!find(&widgets, &copy).active, "the selection is in the folder, not in Starred Photos");

        // Scrolled to the second folder by its tree row: other cells; the speed thumb stays at the
        // centre of its track whatever the position (`ui-layout.md` §5.6).
        let second = rows[1].clone();
        d.click(&second).unwrap();
        d.settle(FRAMES).unwrap();
        let widgets = check(&d, size, "scrolled");
        assert!(find(&widgets, &second).active);
        let handle_after = find(&widgets, "scroll:thumb").rect;
        assert_eq!(handle_after, handle_before, "the speed thumb rests at the centre");
        assert_ne!(ids_of(&widgets, Role::Thumbnail).first(), Some(&first), "other cells are on screen");

        // The File menu open: its item is listed under the command it runs, the Folder Manager's.
        d.click("menu:File").unwrap();
        let widgets = check(&d, size, "File menu open");
        let item = find(&widgets, "menuitem:folders.manager");
        assert!(item.enabled && item.role == Role::MenuItem && item.label == "Add Folder…");
        assert!(find(&widgets, "menu:File").active, "the open menu's title is active");

        // File ▸ Add Folder… opens the Folder Manager with the folder selected in the tree
        // pre-selected.
        let folder = dalica_core::FolderId::new(second.trim_start_matches("tree:folder:").parse().unwrap());
        assert_eq!(d.state().folder, Some(folder));
        let path = d.app().session().unwrap().library().folder(folder).unwrap().path.clone();
        d.click("menuitem:folders.manager").unwrap();
        d.settle(FRAMES).unwrap();
        let widgets = check(&d, size, "Folder Manager");
        assert!(find(&widgets, "dialog:folderManager").rect[2] > 0.0);
        let fm = d.state().dialogs.iter().find(|d| d.kind.name() == "folderManager").unwrap();
        assert_eq!(Draft::read(&fm.fields).selected.as_deref(), path.to_str());
        d.click("button:dialog.cancel").unwrap();
        assert!(d.state().dialogs.is_empty());

        // Folder ▸ Remove from Dalica… asks first: the question names the folder; Cancel keeps it.
        d.click("menu:Folder").unwrap();
        let widgets = check(&d, size, "Folder menu open");
        let item = find(&widgets, "menuitem:folder.remove");
        assert!(item.enabled && item.label == "Remove from Dalica…");
        d.click("menuitem:folder.remove").unwrap();
        d.settle(FRAMES).unwrap();
        let widgets = check(&d, size, "Remove Folder");
        assert!(find(&widgets, "button:dialog.removeFolder").enabled);
        d.click("button:dialog.cancel").unwrap();
        assert!(d.state().dialogs.is_empty());
        assert!(d.app().session().unwrap().library().folder(folder).is_some(), "Cancel keeps the folder");
    }
}

/// The old M1 ids are gone: scripts that used them fail cleanly instead of clicking something else.
#[test]
fn the_m1_ids_no_longer_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = new_driver(App::with_picker(Some(session(dir.path())), Arc::new(ScriptedPicker::new())), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    for old in ["menu.file", "menu.file.addFolder", "bottom.star"] {
        assert!(matches!(d.click(old), Err(dalica_ui::DriverError::NoWidget(_))), "{old}");
    }
    assert!(!d.widgets().iter().any(|w| w.id.ends_with(".star")));
}
