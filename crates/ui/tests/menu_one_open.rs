//! One menu open at a time (`menus.rs`): with a menu of the bar open (Picture, which holds View
//! and Edit), a right-click on a photo or on a tray item closes the bar's menu before its context
//! menu opens, in the same frame. No frame draws two menus, so no row of one is registered twice
//! (both menus have `menuitem:view.edit`), and the context menu is the one left open.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::common::{FRAMES, LARGE, new_driver, try_widget};
use dalica_engine::Session;
use dalica_ui::headless::Click;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::registry;
use egui::PointerButton;
use serde_json::json;

/// A library in `root` with one folder of 4 scenes photos, added and scanned, in a 1400 × 900
/// window, with the first photo selected (so the tray shows it).
fn setup(root: &Path) -> Driver {
    let mut folders = BTreeSet::new();
    for f in crate::common::demo_library(1, 4, 47).unwrap() {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for folder in &folders {
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(ScriptedPicker::new())), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

/// Records, every frame, how many menus (popup layers holding menu rows) the frame drew; the
/// largest count since the last [`take`] is what it returns.
fn count_menus(d: &Driver) -> Arc<Mutex<usize>> {
    let most = Arc::new(Mutex::new(0usize));
    let seen = Arc::clone(&most);
    d.ctx().on_end_pass(
        "test-count-menus",
        Arc::new(move |ui: &mut egui::Ui| {
            let ctx = ui.ctx();
            let layers: HashSet<egui::LayerId> = registry::widgets(ctx)
                .iter()
                .filter(|w| w.id.starts_with("menuitem:"))
                .filter_map(|w| w.egui_id.and_then(|id| ctx.read_response(id)))
                .map(|r| r.layer_id)
                .collect();
            let mut m = seen.lock().unwrap();
            *m = (*m).max(layers.len());
        }),
    );
    most
}

fn take(most: &Arc<Mutex<usize>>) -> usize {
    std::mem::take(&mut *most.lock().unwrap())
}

/// Opens the bar's Picture menu, then right-clicks `target`: exactly one menu is drawn in every
/// frame, no id is registered twice, and the context menu is the one left open.
fn right_click_with_picture_open(d: &mut Driver, most: &Arc<Mutex<usize>>, target: &str) {
    d.click("menu:Picture").unwrap();
    assert!(try_widget(d, "menu:Picture").is_some_and(|w| w.active), "the Picture menu is open");
    assert!(try_widget(d, "menuitem:view.edit").is_some(), "Picture holds View and Edit");
    take(most);
    d.click_with(target, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    assert_eq!(d.id_breach(), None, "a right-click on {target} with Picture open");
    assert_eq!(take(most), 1, "one menu drawn at a time while {target} is right-clicked with Picture open");
    d.settle(FRAMES).unwrap();
    assert!(try_widget(d, "menu:Picture").is_some_and(|w| !w.active), "the bar's menu closed");
    assert!(try_widget(d, "menuitem:view.edit").is_some(), "{target}'s context menu is open");
    d.key("Esc").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(d, "menuitem:view.edit").is_none(), "Esc closes the context menu");
}

#[test]
fn a_right_click_with_a_bar_menu_open_leaves_exactly_one_menu() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = setup(dir.path());
    // The last photo of the row: the first ones sit under the open Picture menu, where a
    // right-click lands on the menu.
    let photo = d.app().panels().unwrap().sections.list[0].photos[3];
    let thumb = format!("thumb:{}", photo.get());
    d.click(&thumb).unwrap();
    d.settle(FRAMES).unwrap();
    let most = count_menus(&d);
    right_click_with_picture_open(&mut d, &most, &thumb);
    right_click_with_picture_open(&mut d, &most, &format!("tray:item:{}", photo.get()));
}
