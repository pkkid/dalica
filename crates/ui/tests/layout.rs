//! The layout skeleton (M3.5): every region of `ui-layout.md` §2 is read back from the widget
//! registry (`region:<name>`, the names of `control-protocol.md` §6.6) at 1400 × 900 and 900 × 600
//! and matches the sizes the spec gives: menu bar 32, toolbar 40, tree 240 and an 8 pt splitter,
//! the light table and its 16 pt scroll bar, the side pane 280 when open (large: min(30 % W, 392)),
//! status line 20, bottom area 96 split at 0.365·W. The expected rects are written out here from the
//! spec, not taken from `panels::layout`, so a change to either shows.
//!
//! The responsiveness rules (M3.10, `ui-layout.md` §11, `design-system.md` §11.1): every control of
//! the title bar, toolbar, tree, side pane, status line and bottom area at 1400 × 900, 1100 × 700 and
//! 900 × 600, with the side pane closed, open and large, windowed and in full screen (F11); the
//! tree shrinks before the light table drops below 480, then the pane gives up its large width and
//! closes; narrow windows drop only the chrome the spec lets go, each before it would overlap.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, shot, widget};
use dalica_engine::Session;
use dalica_ui::headless::{Drag, Target};
use dalica_ui::state::Pane;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;

const SIZES: [(f32, f32); 2] = [(1400.0, 900.0), (900.0, 600.0)];
/// The sizes of the M3.10 done-when: the default, the width where the narrow rules start, the minimum.
const THREE_SIZES: [(f32, f32); 3] = [(1400.0, 900.0), (1100.0, 700.0), (900.0, 600.0)];

/// A library in `root` with two folders of scenes photos, added and scanned.
fn session(root: &Path) -> Session {
    session_named(root, None)
}

/// [`session`], with the first folder called `first` when given.
fn session_named(root: &Path, first: Option<&str>) -> Session {
    let files = dalica_scenes::demo_library(2, 24, 3).unwrap();
    let mut folders = BTreeSet::new();
    let first_folder = files.first().and_then(|f| f.path.components().next()).map(|c| c.as_os_str().to_owned());
    for f in &files {
        let mut rel = f.path.clone();
        if let (Some(name), Some(old)) = (first, &first_folder)
            && let Ok(rest) = f.path.strip_prefix(old)
        {
            rel = Path::new(name).join(rest);
        }
        let path = root.join("photos").join(&rel);
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
    let mut d = new_driver(App::with_picker(Some(session(root)), Arc::new(ScriptedPicker::new())), w as u32, h as u32, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

/// The regions the last frame registered, by name.
fn regions(d: &Driver) -> HashMap<String, [f32; 4]> {
    d.widgets().into_iter().filter_map(|w| w.id.strip_prefix("region:").map(|n| (n.to_string(), w.rect))).collect()
}

/// The tree width `ui-layout.md` §11 gives beside a side pane `pane` wide (0 when closed) in a
/// window `w` wide: the user's width until the light table (splitter and scroll bar included) would
/// drop below 480, then less, never below 200.
fn tree_beside(requested: f32, w: f32, pane: f32) -> f32 {
    requested.clamp(200.0, (w - pane - 480.0).max(200.0))
}

/// What `ui-layout.md` §2 and `design-system.md` §4 say, for a `w × h` window with the tree `tree`
/// wide and a side pane `pane` wide (0 when closed).
fn expected(w: f32, h: f32, tree: f32, pane: f32) -> Vec<(&'static str, [f32; 4])> {
    let middle = h - 32.0 - 40.0 - 20.0 - 96.0;
    let split = 0.365 * w;
    let light = w - tree - 8.0 - 16.0 - pane;
    let mut v = vec![
        ("menuBar", [0.0, 0.0, w, 32.0]),
        ("toolbar", [0.0, 32.0, w, 40.0]),
        ("tree", [0.0, 72.0, tree, middle]),
        ("splitter", [tree, 72.0, 8.0, middle]),
        ("lightTable", [tree + 8.0, 72.0, light, middle]),
        ("scrollbar", [tree + 8.0 + light, 72.0, 16.0, middle]),
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
        v.push(("sidePane", [w - pane, 72.0, pane, middle]));
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
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[0]);
    for (w, h) in SIZES {
        d.resize(w as u32, h as u32);
        d.settle(FRAMES).unwrap();
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

        // A side pane open pushes the light table; its large width is min(30 % W, 392).
        for (pane, large) in [(Pane::Tags, false), (Pane::Properties, true)] {
            d.app_mut().state_mut().pane = Some(pane);
            d.app_mut().state_mut().pane_large = large;
            d.settle(FRAMES).unwrap();
            let width = if large { (0.3 * w).min(392.0) } else { 280.0 };
            // M3.10 (§11): the tree makes room for the pane first (at 900 it is at its minimum).
            let tree = tree_beside(240.0, w, width);
            assert_regions(&d, &expected(w, h, tree, width), &format!("{w}×{h}, {pane:?} pane, large: {large}"));
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
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), SIZES[0]);
    for (w, h) in SIZES {
        d.app_mut().state_mut().tree_width = 240.0;
        d.resize(w as u32, h as u32);
        d.settle(FRAMES).unwrap();
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

// ---- M3.10: responsiveness rules and full screen ------------------------------------------------

const MENUS: [&str; 8] = ["menu:File", "menu:Edit", "menu:View", "menu:Folder", "menu:Picture", "menu:Create", "menu:Tools", "menu:Help"];
/// The toolbar row, left to right, with the sizes of `ui-layout.md` §3 (`None`: the search field,
/// whose width follows the window).
const TOOLBAR: [(&str, Option<[f32; 2]>); 12] = [
    ("button:toolbar.import", Some([110.0, 28.0])),
    ("button:toolbar.newAlbum", Some([30.0, 28.0])),
    ("button:tree.flat", Some([30.0, 28.0])),
    ("button:tree.tree", Some([30.0, 28.0])),
    ("button:toolbar.viewOptions", Some([22.0, 28.0])),
    ("button:filter.starred", Some([26.0, 26.0])),
    ("button:filter.faces", Some([26.0, 26.0])),
    ("button:filter.videos", Some([26.0, 26.0])),
    ("button:filter.geotagged", Some([26.0, 26.0])),
    ("slider:age", Some([100.0, 20.0])),
    ("field:search", None),
    ("indicator:activity", Some([28.0, 28.0])),
];
/// The bottom controls row, left to right, with the sizes of `ui-layout.md` §6.3–§6.4.
const CONTROLS_ROW: [(&str, [f32; 2]); 9] = [
    ("button:star", [36.0, 24.0]),
    ("button:rotate.ccw", [36.0, 24.0]),
    ("button:rotate.cw", [36.0, 24.0]),
    ("button:loupe", [24.0, 24.0]),
    ("slider:thumbSize", [128.0, 20.0]),
    ("toggle:pane.people", [32.0, 24.0]),
    ("toggle:pane.places", [32.0, 24.0]),
    ("toggle:pane.tags", [32.0, 24.0]),
    ("toggle:pane.properties", [32.0, 24.0]),
];
const TRAY_BUTTONS: [&str; 3] = ["button:tray.hold", "button:tray.clear", "button:tray.addToAlbum"];
const ACTIONS: [&str; 3] = ["action:export", "action:collage", "action:slideshow"];

/// egui rounds widget rects to 1/32 point (`emath::GUI_ROUNDING`), so a widget placed at a
/// fractional split (0.365·W) may stick out of its region by that much.
const ROUNDING: f32 = 1.0 / 32.0 + 0.001;

/// `r` inside `o`, give or take egui's rounding.
fn within(r: [f32; 4], o: [f32; 4]) -> bool {
    r[0] >= o[0] - ROUNDING && r[1] >= o[1] - ROUNDING && r[0] + r[2] <= o[0] + o[2] + ROUNDING && r[1] + r[3] <= o[1] + o[3] + ROUNDING
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

/// Whether two rects share some area (touching edges do not count).
fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] - ROUNDING && b[0] < a[0] + a[2] - ROUNDING && a[1] < b[1] + b[3] - ROUNDING && b[1] < a[1] + a[3] - ROUNDING
}

/// Every control of M3.6–M3.9 in a `w × h` window with the side pane `pane` (its protocol name)
/// open or not: registered, at its spec size, inside its region, in its order, and no two
/// overlapping. Every interactive widget has an id.
fn assert_controls(d: &Driver, w: f32, h: f32, pane: Option<&str>, case: &str) {
    let all: HashMap<String, WidgetInfo> = d.widgets().into_iter().map(|wi| (wi.id.clone(), wi)).collect();
    let r = regions(d);
    let rect = |id: &str| all.get(id).unwrap_or_else(|| panic!("{case}: `{id}` is not registered")).rect;
    let size = |id: &str| {
        let r = rect(id);
        [r[2], r[3]]
    };
    // Every control checked here, for the overlap check at the end.
    let mut placed: Vec<(String, [f32; 4])> = Vec::new();
    let mut place = |id: &str, region: &str| {
        let got = rect(id);
        assert!(within(got, r[region]), "{case}: `{id}` at {got:?} is outside `region:{region}` {:?}", r[region]);
        placed.push((id.to_string(), got));
        got
    };

    // The title bar (M3.6): the menus after the app icon, the drag gap, then Minimize, Maximize and
    // Close flush in the top-right corner, 46 × 31 each (the bar's divider below them).
    let mut x = 8.0 + 16.0;
    for id in MENUS {
        let m = place(id, "menuBar");
        assert!(m[0] >= x - 0.01, "{case}: `{id}` at {m:?} is left of what comes before it");
        x = m[0] + m[2];
    }
    for (i, id) in ["button:window.minimize", "button:window.maximize", "button:window.close"].into_iter().enumerate() {
        let b = place(id, "menuBar");
        assert_eq!(b, [w - 46.0 * (3 - i) as f32, 0.0, 46.0, 31.0], "{case}: `{id}`");
    }
    let drag = place("titlebar:drag", "menuBar");
    assert!(drag[0] >= x - 0.01 && near([drag[0] + drag[2], 0.0, 0.0, 0.0], [w - 138.0, 0.0, 0.0, 0.0]), "{case}: the gap {drag:?}");

    // The toolbar (M3.7): left to right, at the sizes of §3; the filters right of the splitter; the
    // search field to W − 64 and never under 160 (its text edit sits right of the magnifier); the
    // caption above the toggles from 1100 wide, the toggles centred below.
    let mut x = 0.0;
    for (id, want) in TOOLBAR {
        let c = place(id, "toolbar");
        if let Some(want) = want {
            assert_eq!(size(id), want, "{case}: `{id}`");
        }
        assert!(c[0] >= x - 0.01, "{case}: `{id}` at {c:?} overlaps the control before it");
        x = c[0] + c[2];
    }
    let splitter_right = r["splitter"][0] + r["splitter"][2];
    assert!(rect("button:filter.starred")[0] >= splitter_right + 8.0 - 0.01, "{case}: the filters start 8 right of the splitter");
    let search = rect("field:search");
    assert!(search[0] + search[2] <= w - 64.0 + 0.01 && search[2] >= 160.0 - 36.0, "{case}: the search field {search:?}");
    let filter_top = if w >= 1100.0 { 32.0 + 13.0 } else { 32.0 + 7.0 };
    assert_eq!(rect("button:filter.starred")[1], filter_top, "{case}: the filter caption shows from 1100 wide only");

    // The tree, the splitter, the light table's column (M3.5, M3.8).
    assert_eq!(place("splitter:tree", "splitter"), r["splitter"], "{case}");
    let column = [r["lightTable"][0], r["lightTable"][1], r["lightTable"][2] + r["scrollbar"][2], r["lightTable"][3]];
    for wi in all.values() {
        if wi.id.starts_with("tree:") {
            assert!(within(wi.rect, r["tree"]), "{case}: `{}` at {:?} is outside the tree", wi.id, wi.rect);
        }
        if wi.id.starts_with("thumb:") || wi.id.starts_with("scroll:") {
            assert!(within(wi.rect, column), "{case}: `{}` at {:?} is outside the light table", wi.id, wi.rect);
        }
        assert!(within(wi.rect, [0.0, 0.0, w, h]), "{case}: `{}` at {:?} is outside the window", wi.id, wi.rect);
    }

    // The side pane (M3.8): the 30 pt header with « and × (24 × 24, 3 in), the body under it.
    match pane {
        Some(name) => {
            let p = r["sidePane"];
            assert_eq!(place("button:pane.widen", "sidePane"), [p[0] + 3.0, p[1] + 3.0, 24.0, 24.0], "{case}");
            assert_eq!(place("button:pane.close", "sidePane"), [p[0] + p[2] - 27.0, p[1] + 3.0, 24.0, 24.0], "{case}");
            assert!(near(place(&format!("pane:{name}"), "sidePane"), [p[0], p[1] + 30.0, p[2], p[3] - 30.0]), "{case}: the pane body");
        }
        None => assert!(!all.keys().any(|id| id.starts_with("pane:") || id.starts_with("button:pane.")), "{case}: no pane, no pane controls"),
    }

    // The status line (M3.8) and the bottom area (M3.9).
    place("text:status", "statusLine");
    assert_eq!(place("tray", "tray"), r["tray"], "{case}");
    let mut y = r["trayButtons"][1];
    for id in TRAY_BUTTONS {
        let b = place(id, "trayButtons");
        assert_eq!(size(id), [34.0, 24.0], "{case}: `{id}`");
        assert!(b[1] >= y - 0.01, "{case}: `{id}` is under the one before it");
        y = b[1] + b[3];
    }
    let mut x = r["bottomControls"][0];
    for (id, want) in CONTROLS_ROW {
        let c = place(id, "bottomControls");
        assert_eq!(size(id), want, "{case}: `{id}`");
        assert!(c[0] >= x - 0.01, "{case}: `{id}` at {c:?} overlaps the control before it");
        x = c[0] + c[2];
    }
    let toggles: Vec<[f32; 4]> = CONTROLS_ROW[5..].iter().map(|(id, _)| rect(id)).collect();
    for pair in toggles.windows(2) {
        assert!((pair[0][0] + pair[0][2] - pair[1][0]).abs() < 0.01, "{case}: the pane toggles are joined");
    }
    let shown: Vec<&str> = ACTIONS.iter().chain(&["action:more"]).copied().filter(|id| all.contains_key(*id)).collect();
    let want: &[&str] = if w >= 1100.0 { &ACTIONS } else { &["action:more"] };
    assert_eq!(shown, want, "{case}: the action row (More… below 1100 wide)");
    for id in shown {
        place(id, "actionButtons");
        assert_eq!(size(id), [64.0, 56.0], "{case}: `{id}`");
    }

    for (i, (a, ra)) in placed.iter().enumerate() {
        for (b, rb) in placed.iter().skip(i + 1) {
            assert!(!overlap(*ra, *rb), "{case}: `{a}` {ra:?} overlaps `{b}` {rb:?}");
        }
    }
}

/// The M3.10 done-when: the geometry of every region and every control at 1400 × 900, 1100 × 700
/// and 900 × 600, with the side pane closed, open and large, windowed and in full screen. A pane
/// opened on purpose stays open at every size; the tree makes room for it first (`ui-layout.md`
/// §11). F11 toggles full screen, which lays out every region and control as the window does.
#[test]
fn every_control_is_in_place_at_three_sizes_with_the_pane_open_and_closed_and_in_full_screen() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), THREE_SIZES[0]);
    for (w, h) in THREE_SIZES {
        d.app_mut().state_mut().pane = None;
        d.resize(w as u32, h as u32);
        d.settle(FRAMES).unwrap();
        for (pane, large) in [(None, false), (Some(Pane::Tags), false), (Some(Pane::Properties), true)] {
            d.app_mut().state_mut().pane = pane;
            d.app_mut().state_mut().pane_large = large;
            d.settle(FRAMES).unwrap();
            let width = pane.map_or(0.0, |_| if large { (0.3 * w).min(392.0) } else { 280.0 });
            let name = pane.map(|p| match p {
                Pane::Tags => "tags",
                Pane::Properties => "properties",
                Pane::People => "people",
                Pane::Places => "places",
            });
            for full in [false, true] {
                if full {
                    d.key("F11").unwrap();
                    d.settle(FRAMES).unwrap();
                }
                assert_eq!(d.state().full_screen, full, "F11 toggles full screen");
                assert_eq!(d.state().pane, pane, "a pane opened on purpose stays open");
                let case = format!("{w}×{h}, pane {pane:?} (large: {large}), full screen: {full}");
                assert_regions(&d, &expected(w, h, tree_beside(240.0, w, width), width), &case);
                assert_controls(&d, w, h, name, &case);
                let pane_part = name.map_or("closed".to_string(), |n| if large { format!("{n}-large") } else { n.to_string() });
                shot(&d, &format!("layout-{w}x{h}-pane-{pane_part}{}", if full { "-fullscreen" } else { "" }));
            }
            d.key("F11").unwrap();
            d.settle(FRAMES).unwrap();
            assert!(!d.state().full_screen, "F11 again leaves full screen");
        }
    }
}

/// A screen bigger than the window: full screen at 1920 × 1080 lays out every region and control at
/// the spec's sizes, then leaving it (back to 1400 × 900) gives the window's layout again.
#[test]
fn full_screen_on_a_large_screen_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (1400.0, 900.0));
    d.key("F11").unwrap();
    d.resize(1920, 1080);
    d.settle(FRAMES).unwrap();
    assert!(d.state().full_screen);
    assert_regions(&d, &expected(1920.0, 1080.0, 240.0, 0.0), "full screen 1920×1080");
    assert_controls(&d, 1920.0, 1080.0, None, "full screen 1920×1080");
    d.key("F11").unwrap();
    d.resize(1400, 900);
    d.settle(FRAMES).unwrap();
    assert!(!d.state().full_screen);
    assert_regions(&d, &expected(1400.0, 900.0, 240.0, 0.0), "back to 1400×900");
    assert_controls(&d, 1400.0, 900.0, None, "back to 1400×900");
}

/// `ui-layout.md` §7 and §11 as the window narrows with a large pane open: the tree keeps its width
/// while it can, then shrinks toward 200 before the light table drops below 480; then the pane
/// takes its normal width; then it closes. Widening again never reopens it, and the tree gets its
/// width back. A pane opened on purpose in the narrow window stays, the tree at its minimum.
#[test]
fn a_narrowing_window_shrinks_the_tree_then_the_large_pane_then_closes_the_pane() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (1400.0, 900.0));
    d.app_mut().state_mut().pane = Some(Pane::Properties);
    d.app_mut().state_mut().pane_large = true;
    d.settle(FRAMES).unwrap();
    // (window width, tree, pane width or 0 when closed)
    for (w, tree, pane) in [
        (1400.0, 240.0, 392.0),
        (1100.0, 240.0, 330.0), // 1100 − 330 − 480 = 290: the tree keeps 240
        (1000.0, 220.0, 300.0), // 1000 − 300 − 480 = 220: the tree shrinks first
        (965.0, 205.0, 280.0),  // a large pane (289.5) no longer fits beside the 200 tree: normal width
        (960.0, 200.0, 280.0),  // the light table keeps exactly 480
        (959.0, 240.0, 0.0),    // nothing fits: the pane closes and the tree has its width back
        (1400.0, 240.0, 0.0),   // widening never reopens it
    ] {
        d.resize(w as u32, 700);
        d.settle(FRAMES).unwrap();
        let case = format!("narrowed to {w}");
        assert_eq!(d.state().pane.is_some(), pane > 0.0, "{case}");
        assert_eq!(d.state().pane_large, pane > 280.0, "{case}");
        assert_regions(&d, &expected(w, 700.0, tree, pane), &case);
        if pane > 0.0 && tree > 200.0 {
            let r = regions(&d);
            assert!(r["lightTable"][2] + 8.0 + 16.0 >= 480.0 - 0.01, "{case}: the light table keeps 480 while the tree can shrink");
        }
    }
    assert_eq!(d.state().tree_width, 240.0, "the user's tree width is kept; only the layout narrowed it");

    d.resize(900, 600);
    d.settle(FRAMES).unwrap();
    d.click("toggle:pane.people").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().pane, Some(Pane::People), "opened on purpose in a narrow window: it stays");
    assert_regions(&d, &expected(900.0, 600.0, 200.0, 280.0), "900×600, People opened on purpose");
    assert_controls(&d, 900.0, 600.0, Some("people"), "900×600, People opened on purpose");
}

/// With a side pane open, the splitter drags the tree only as far as leaves the light table 480.
#[test]
fn the_splitter_leaves_the_light_table_480_beside_the_side_pane() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (1400.0, 900.0));
    d.app_mut().state_mut().pane = Some(Pane::Tags);
    d.settle(FRAMES).unwrap();
    d.drag("splitter:tree", Target::Delta(egui::vec2(2000.0, 0.0)), &Drag::default()).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().tree_width, 1400.0 - 280.0 - 480.0);
    assert_regions(&d, &expected(1400.0, 900.0, 640.0, 280.0), "widest tree beside the pane");
    assert_controls(&d, 1400.0, 900.0, Some("tags"), "widest tree beside the pane");
}

/// The columns of `shot`'s title row (rows 4 … 27) inside `x0..x1` that are not the bar's fill.
fn title_ink(shot: &dalica_ui_kit::Image, x0: f32, x1: f32) -> Vec<u32> {
    let fill = dalica_ui_kit::Tokens::default().chrome.to_array();
    let px = |x: u32, y: u32| {
        let i = ((y * shot.width + x) * 4) as usize;
        [shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]]
    };
    (x0.max(0.0) as u32..x1.max(0.0) as u32).filter(|&x| (4..28).any(|y| px(x, y).iter().zip(&fill).any(|(a, b)| a.abs_diff(*b) > 12))).collect()
}

/// `design-system.md` §11.1 "Narrow windows", from 1400 down to the 900 minimum: each piece of
/// chrome that may go drops before it would overlap anything, and stays gone as the window narrows
/// further: the window title when it no longer fits its gap (a long folder name drops it early; the
/// short "Dalica" fits at every width), the filter caption below 1100, the search field shrinking
/// to its 160 minimum. The menus, the filter toggles and the caption buttons never go.
#[test]
fn narrow_windows_drop_only_the_chrome_the_spec_lets_go_before_anything_overlaps() {
    const LONG: &str = "Summer holiday at the lake house with the whole family";
    let dir = tempfile::tempdir().unwrap();
    let app = App::with_picker(Some(session_named(dir.path(), Some(LONG))), Arc::new(ScriptedPicker::new()));
    let mut d = new_driver(app, 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let row = d.widgets().into_iter().find(|wi| wi.id.starts_with("tree:folder:") && wi.label.starts_with(LONG)).expect("the long folder's row");
    d.click(&row.id).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().folder.is_some());

    for (selected, title) in [(true, format!("{LONG} — Dalica")), (false, "Dalica".to_string())] {
        if !selected {
            d.app_mut().state_mut().folder = None;
        }
        let mut dropped_at = None;
        for w in (900..=1400).rev().step_by(25) {
            let w = w as f32;
            d.resize(w as u32, 700);
            d.settle(FRAMES).unwrap();
            let case = format!("{w}×700, title “{title}”");
            assert_eq!(widget(&d, "titlebar:drag").label, title, "{case}");
            assert_controls(&d, w, 700.0, None, &case);
            // The title: centred on the window, slid into the gap with 16 to spare on each side, or
            // not drawn at all.
            let drag = widget(&d, "titlebar:drag").rect;
            let ink = title_ink(&d.screenshot().unwrap(), drag[0], drag[0] + drag[2]);
            if let (Some(first), Some(last)) = (ink.first(), ink.last()) {
                assert!(dropped_at.is_none(), "{case}: the title came back after it was dropped at {dropped_at:?}");
                assert!(
                    *first as f32 >= drag[0] + 16.0 - 1.0 && *last as f32 <= drag[0] + drag[2] - 16.0 + 1.0,
                    "{case}: {first}…{last} in {drag:?}"
                );
                let (centre, half) = ((*first + *last) as f32 / 2.0, (*last - *first) as f32 / 2.0);
                let want = (w / 2.0).clamp(drag[0] + 16.0 + half, drag[0] + drag[2] - 16.0 - half);
                assert!((centre - want).abs() <= 2.0, "{case}: the title is centred on the window, slid into the gap ({centre} vs {want})");
            } else if dropped_at.is_none() {
                dropped_at = Some(w);
            }
        }
        if selected {
            let at = dropped_at.expect("the long title drops before 900");
            assert!(at < 1100.0 && at > 900.0, "the long title fits at 1100 and drops on the way to 900 (at {at})");
        } else {
            assert_eq!(dropped_at, None, "“Dalica” fits at every width down to 900: it never drops");
        }
    }
}

/// The toolbar follows the tree as the layout narrows it: beside a side pane the filters start 8
/// right of the splitter (`ui-layout.md` §3; never over the ▾ button), not where the user's wider
/// tree would end.
#[test]
fn the_filters_start_8_right_of_the_splitter_beside_a_side_pane() {
    for (w, h, tree, want) in [(1100.0, 700.0, 400.0, 340.0), (1100.0, 700.0, 600.0, 340.0), (900.0, 600.0, 400.0, 200.0)] {
        let dir = tempfile::tempdir().unwrap();
        let mut d = driver(dir.path(), (w, h));
        d.app_mut().state_mut().tree_width = tree;
        d.app_mut().state_mut().pane = Some(Pane::Tags);
        d.settle(FRAMES).unwrap();
        let case = format!("{w}×{h}, tree {tree} beside the Tags pane");
        assert_regions(&d, &expected(w, h, want, 280.0), &case);
        // 8 right of the splitter, unless that is left of the ▾ button's end (246) plus the 12 gap.
        let view_options = widget(&d, "button:toolbar.viewOptions").rect;
        let start = (want + 8.0 + 8.0).max(view_options[0] + view_options[2] + 12.0);
        assert_eq!(widget(&d, "button:filter.starred").rect[0], start, "{case}: 8 right of the splitter");
        assert_controls(&d, w, h, Some("tags"), &case);
    }
}
