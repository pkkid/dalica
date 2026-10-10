//! Every interactive widget drawn has an automation id, unique within its frame and in the grammar
//! of `control-protocol.md` §7 (M2.1). The window is driven headlessly at 1400 × 900 and 900 × 600
//! through the states M1 can reach: a library open with nothing selected, a photo selected and
//! starred, the light table scrolled to a later folder, and the File menu open. In each, the list
//! of interactive egui widgets with no id must be empty.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use dalica_engine::Session;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry::{self, Role};

const FRAMES: usize = 60 * 60;
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
        let mut d = Driver::new(App::with_picker(Some(session), Arc::new(ScriptedPicker::new())), size.0, size.1, 1.0);
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
        let badge = find(&widgets, &format!("{first}/star"));
        assert!(badge.role == Role::Badge && badge.egui_id.is_none());
        assert!(inside(badge.rect, find(&widgets, &first).rect));
        assert!(find(&widgets, &first).active, "the selected cell is active");

        // Scrolled to the second folder by its tree row: other cells, the handle further down.
        d.click(&rows[1]).unwrap();
        d.settle(FRAMES).unwrap();
        let widgets = check(&d, size, "scrolled");
        assert!(find(&widgets, &rows[1]).active);
        let handle_after = find(&widgets, "scroll:thumb").rect;
        assert!(handle_after[1] > handle_before[1], "the handle moved down: {handle_before:?} → {handle_after:?}");
        assert_ne!(ids_of(&widgets, Role::Thumbnail).first(), Some(&first), "other cells are on screen");

        // The File menu open: its item is listed under the command it runs.
        d.click("menu:File").unwrap();
        let widgets = check(&d, size, "File menu open");
        let item = find(&widgets, "menuitem:folders.add");
        assert!(item.enabled && item.role == Role::MenuItem && item.label == "Add Folder…");
        assert!(find(&widgets, "menu:File").active, "the open menu's title is active");
    }
}

/// The old M1 ids are gone: scripts that used them fail cleanly instead of clicking something else.
#[test]
fn the_m1_ids_no_longer_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Driver::new(App::with_picker(Some(session(dir.path())), Arc::new(ScriptedPicker::new())), 900, 600, 1.0);
    d.settle(FRAMES).unwrap();
    for old in ["menu.file", "menu.file.addFolder", "bottom.star"] {
        assert!(matches!(d.click(old), Err(dalica_ui::DriverError::NoWidget(_))), "{old}");
    }
    assert!(!d.widgets().iter().any(|w| w.id.ends_with(".star")));
}
