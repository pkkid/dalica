//! The layout skeleton (M3.5): every region of `ui-layout.md` §2 is read back from the widget
//! registry (`region:<name>`, the names of `control-protocol.md` §6.6) at 1400 × 900 and 900 × 600
//! and matches the sizes the spec gives: menu bar 24, toolbar 40, tree 240 and an 8 pt splitter,
//! the light table and its 16 pt scroll bar, the side pane 280 when open (large: min(30 % W, 392)),
//! status line 20, bottom area 96 split at 0.365·W. The expected rects are written out here from the
//! spec, not taken from `panels::layout`, so a change to either shows.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use dalica_engine::Session;
use dalica_ui::headless::{Drag, Target};
use dalica_ui::state::Pane;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;

const FRAMES: usize = 60 * 60;
const SIZES: [(f32, f32); 2] = [(1400.0, 900.0), (900.0, 600.0)];

/// A library in `root` with two folders of scenes photos, added and scanned.
fn session(root: &Path) -> Session {
    let files = dalica_scenes::demo_library(2, 24, 3).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for folder in &folders {
        s.execute("folders.add", &serde_json::json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    s
}

fn driver(root: &Path, (w, h): (f32, f32)) -> Driver {
    let mut d = Driver::new(App::with_picker(Some(session(root)), Arc::new(ScriptedPicker::new())), w as u32, h as u32, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

/// The regions the last frame registered, by name.
fn regions(d: &Driver) -> HashMap<String, [f32; 4]> {
    d.widgets().into_iter().filter_map(|w| w.id.strip_prefix("region:").map(|n| (n.to_string(), w.rect))).collect()
}

fn widget(d: &Driver, id: &str) -> WidgetInfo {
    d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no `{id}`"))
}

/// What `ui-layout.md` §2 and `design-system.md` §4 say, for a `w × h` window with the tree `tree`
/// wide and a side pane `pane` wide (0 when closed).
fn expected(w: f32, h: f32, tree: f32, pane: f32) -> Vec<(&'static str, [f32; 4])> {
    let middle = h - 24.0 - 40.0 - 20.0 - 96.0;
    let split = 0.365 * w;
    let light = w - tree - 8.0 - 16.0 - pane;
    let mut v = vec![
        ("menuBar", [0.0, 0.0, w, 24.0]),
        ("toolbar", [0.0, 24.0, w, 40.0]),
        ("tree", [0.0, 64.0, tree, middle]),
        ("splitter", [tree, 64.0, 8.0, middle]),
        ("lightTable", [tree + 8.0, 64.0, light, middle]),
        ("scrollbar", [tree + 8.0 + light, 64.0, 16.0, middle]),
        ("statusLine", [0.0, h - 116.0, w, 20.0]),
        // The tray box from x = 6 to 0.365·W − 56, 80 high, centred in the 96 pt area.
        ("tray", [6.0, h - 88.0, split - 56.0 - 6.0, 80.0]),
        // Hold, Clear, Add to Album: 34 wide, stacked over the box's height, right of it.
        ("trayButtons", [split - 48.0, h - 88.0, 34.0, 80.0]),
        // Right of the split: the controls row, then the action buttons under their separator.
        ("bottomControls", [split, h - 96.0, w - split, 36.0]),
        ("actionButtons", [split, h - 60.0, w - split, 60.0]),
    ];
    if pane > 0.0 {
        v.push(("sidePane", [w - pane, 64.0, pane, middle]));
    }
    v
}

fn assert_regions(d: &Driver, want: &[(&str, [f32; 4])], case: &str) {
    let got = regions(d);
    for (name, rect) in want {
        let r = got.get(*name).unwrap_or_else(|| panic!("{case}: `region:{name}` is not registered; have {:?}", got.keys()));
        assert!(r.iter().zip(rect).all(|(a, b)| (a - b).abs() < 0.01), "{case}: `{name}` is {r:?}, the spec says {rect:?}");
    }
    let names: BTreeSet<&str> = want.iter().map(|(n, _)| *n).collect();
    let extra: Vec<&String> = got.keys().filter(|n| !names.contains(n.as_str())).collect();
    assert!(extra.is_empty(), "{case}: regions not shown are registered: {extra:?}");
}

fn inside(r: [f32; 4], o: [f32; 4]) -> bool {
    r[0] >= o[0] - 0.01 && r[1] >= o[1] - 0.01 && r[0] + r[2] <= o[0] + o[2] + 0.01 && r[1] + r[3] <= o[1] + o[3] + 0.01
}

/// The M3.5 done-when.
#[test]
fn every_region_is_registered_at_its_spec_size_at_both_window_sizes() {
    for (w, h) in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let mut d = driver(dir.path(), (w, h));
        assert_regions(&d, &expected(w, h, 240.0, 0.0), &format!("{w}×{h}"));
        assert!(!regions(&d).contains_key("tabStrip") && !regions(&d).contains_key("sidePane"));

        // What M3.3–M3.9 draw lands in its region.
        let r = regions(&d);
        assert!(inside(widget(&d, "menu:File").rect, r["menuBar"]), "{w}×{h}: the File menu is in the menu bar");
        assert!(inside(widget(&d, "splitter:tree").rect, r["splitter"]));
        assert!(inside(widget(&d, "button:star").rect, r["bottomControls"]), "{w}×{h}: the star button is a bottom control");
        let column = [r["lightTable"][0], r["lightTable"][1], r["lightTable"][2] + r["scrollbar"][2], r["lightTable"][3]];
        for wi in d.widgets() {
            if wi.id.starts_with("tree:") {
                assert!(inside(wi.rect, r["tree"]), "{w}×{h}: `{}` at {:?} is outside the tree", wi.id, wi.rect);
            }
            if wi.id.starts_with("thumb:") || wi.id.starts_with("scroll:") {
                assert!(inside(wi.rect, column), "{w}×{h}: `{}` at {:?} is outside the light table", wi.id, wi.rect);
            }
        }
        let missing = d.app().panels().unwrap().unregistered_widgets();
        assert_eq!(missing, [], "{w}×{h}: interactive widgets without an id");

        // A side pane open pushes the light table; its large width is min(30 % W, 392).
        for (pane, large) in [(Pane::Tags, false), (Pane::Properties, true)] {
            d.app_mut().state_mut().pane = Some(pane);
            d.app_mut().state_mut().pane_large = large;
            d.settle(FRAMES).unwrap();
            let width = if large { (0.3 * w).min(392.0) } else { 280.0 };
            assert_regions(&d, &expected(w, h, 240.0, width), &format!("{w}×{h}, {pane:?} pane, large: {large}"));
        }
        d.app_mut().state_mut().pane = None;
        d.settle(FRAMES).unwrap();
        assert_regions(&d, &expected(w, h, 240.0, 0.0), &format!("{w}×{h}, pane closed again"));
    }
}

/// The regions follow the window when it is resized, without a restart.
#[test]
fn the_regions_follow_a_resize() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[0]);
    for (w, h) in [SIZES[1], (1100.0, 700.0), SIZES[0]] {
        d.resize(w as u32, h as u32);
        d.settle(FRAMES).unwrap();
        assert_regions(&d, &expected(w, h, 240.0, 0.0), &format!("resized to {w}×{h}"));
    }
}

/// Dragging `splitter:tree` changes the tree width (in `UiState`), within 200 … W − 480, and the
/// light table gives or takes the difference.
#[test]
fn the_splitter_resizes_the_tree_within_its_limits() {
    for (w, h) in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let mut d = driver(dir.path(), (w, h));
        let drag = |d: &mut Driver, dx: f32| {
            d.drag("splitter:tree", Target::Delta(egui::vec2(dx, 0.0)), &Drag::default()).unwrap();
            d.settle(FRAMES).unwrap();
        };
        drag(&mut d, 60.0);
        assert!((d.state().tree_width - 300.0).abs() < 0.5, "{w}×{h}: {}", d.state().tree_width);
        let tree = d.state().tree_width;
        assert_regions(&d, &expected(w, h, tree, 0.0), &format!("{w}×{h}, tree {tree}"));

        drag(&mut d, -500.0);
        assert_eq!(d.state().tree_width, 200.0, "{w}×{h}: the minimum");
        assert_regions(&d, &expected(w, h, 200.0, 0.0), &format!("{w}×{h}, narrowest tree"));

        drag(&mut d, 2000.0);
        assert_eq!(d.state().tree_width, w - 480.0, "{w}×{h}: the light table keeps 480");
        assert_regions(&d, &expected(w, h, w - 480.0, 0.0), &format!("{w}×{h}, widest tree"));
    }
}

/// A hostile tree width in the view state (from `ui.set` or a damaged settings file) is clamped by
/// the layout; the regions stay where the spec allows.
#[test]
fn a_hostile_tree_width_is_clamped_by_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[1]);
    for (bad, tree) in [(f32::NAN, 240.0), (f32::INFINITY, 240.0), (-5.0, 200.0), (1e9, 420.0)] {
        d.app_mut().state_mut().tree_width = bad;
        d.settle(FRAMES).unwrap();
        assert_regions(&d, &expected(900.0, 600.0, tree, 0.0), &format!("tree width {bad}"));
    }
}

/// The view state of `control-protocol.md` §6.5 that the layout reads is in `ui.state`.
#[test]
fn the_view_state_carries_tree_width_pane_thumb_size_and_full_screen() {
    let v = serde_json::to_value(dalica_ui::UiState::default()).unwrap();
    assert_eq!(v["treeWidth"], 240.0);
    assert_eq!(v["pane"], serde_json::Value::Null);
    assert_eq!(v["paneLarge"], false);
    assert_eq!(v["thumbSize"], 144.0);
    assert_eq!(v["fullScreen"], false);
}
