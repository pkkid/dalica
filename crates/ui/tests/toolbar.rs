//! The toolbar (M3.7, `ui-layout.md` §3): every control's id (`control-protocol.md` §7) is
//! registered at 1400 × 900 and 900 × 600 with the size the spec gives, inside the toolbar, left to
//! right; the search field runs from 0.4·W (or right of the filters) to W − 64; the filter caption
//! shows from 1100 wide, hides below, and the hovered filter's tooltip replaces it; controls whose
//! command does not exist yet are present and disabled; the ▾ popup opens with its rows. The sizes
//! are written out here from the spec, not taken from `panels::toolbar`, so a change to either
//! shows.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use dalica_engine::Session;
use dalica_ui::panels::toolbar;
use dalica_ui::{App, Driver, ScriptedPicker};
use dalica_ui_kit::WidgetInfo;

const FRAMES: usize = 60 * 60;
const SIZES: [(f32, f32); 2] = [(1400.0, 900.0), (900.0, 600.0)];
/// The toolbar region: under the 24 pt menu bar, 40 high.
const TOP: f32 = 32.0;
const HEIGHT: f32 = 40.0;

/// A library in `root` with two folders of scenes photos, added and scanned.
fn session(root: &Path) -> Session {
    let files = dalica_scenes::demo_library(2, 12, 3).unwrap();
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

fn widget(d: &Driver, id: &str) -> WidgetInfo {
    d.widgets().into_iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no `{id}`"))
}

fn size(w: &WidgetInfo) -> (f32, f32) {
    (w.rect[2], w.rect[3])
}

/// Every id of `control-protocol.md` §7's toolbar row, with the size `ui-layout.md` §3 gives
/// (`None` where the size depends on the window: the search field).
const CONTROLS: [(&str, Option<(f32, f32)>); 12] = [
    ("button:toolbar.import", Some((110.0, 28.0))),
    ("button:toolbar.newAlbum", Some((30.0, 28.0))),
    ("button:tree.flat", Some((30.0, 28.0))),
    ("button:tree.tree", Some((30.0, 28.0))),
    ("button:toolbar.viewOptions", Some((22.0, 28.0))),
    ("button:filter.starred", Some((26.0, 26.0))),
    ("button:filter.faces", Some((26.0, 26.0))),
    ("button:filter.videos", Some((26.0, 26.0))),
    ("button:filter.geotagged", Some((26.0, 26.0))),
    ("slider:age", Some((100.0, 20.0))),
    ("field:search", None),
    ("indicator:activity", Some((28.0, 28.0))),
];

/// Pixels in rows `y0..y1`, columns `x0..x1` of a screenshot that differ from the toolbar's fill
/// (sampled at `(4, TOP + 2)`), counted per column: how many columns have ink.
fn ink_columns(shot: &dalica_ui_kit::Image, x0: f32, x1: f32, y0: f32, y1: f32) -> usize {
    let px = |x: u32, y: u32| {
        let i = ((y * shot.width + x) * 4) as usize;
        [shot.rgba[i], shot.rgba[i + 1], shot.rgba[i + 2]]
    };
    let fill = px(4, TOP as u32 + 2);
    (x0 as u32..x1 as u32).filter(|&x| (y0 as u32..y1 as u32).any(|y| px(x, y).iter().zip(fill).any(|(a, b)| a.abs_diff(b) > 12))).count()
}

/// The M3.7 done-when: every toolbar id at both sizes with the sizes of §3; the filter caption
/// hides below 1100 wide.
#[test]
fn every_toolbar_control_is_registered_with_its_spec_size_at_both_window_sizes() {
    for (w, h) in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let mut d = driver(dir.path(), (w, h));
        let case = format!("{w}×{h}");
        let mut previous_right = 0.0;
        for (id, want) in CONTROLS {
            let wi = widget(&d, id);
            if let Some(want) = want {
                assert_eq!(size(&wi), want, "{case}: `{id}` is {:?}", wi.rect);
            }
            let [x, y, rw, rh] = wi.rect;
            assert!(x >= 0.0 && y >= TOP && x + rw <= w && y + rh <= TOP + HEIGHT, "{case}: `{id}` at {:?} is outside the toolbar", wi.rect);
            assert!(x >= previous_right - 0.01, "{case}: `{id}` at {:?} overlaps the control before it", wi.rect);
            previous_right = x + rw;
        }
        // The search field: 28 high, from 0.4·W (or further right) to W − 64, never under 160.
        let search = widget(&d, "field:search").rect;
        let frame_left = widget(&d, "slider:age").rect[0] + 100.0;
        assert!(search[0] >= 0.4 * w - 28.0 && search[0] >= frame_left, "{case}: the search field starts at {}", search[0]);
        // The field's text edit sits inside the 28 pt frame, right of the magnifier.
        assert!(search[0] + search[2] <= w - 64.0 + 0.01 && search[2] >= 160.0 - 36.0, "{case}: {search:?}");
        // The filters and the search field are in the light table's column (right of tree + splitter).
        assert!(widget(&d, "button:filter.starred").rect[0] >= 248.0, "{case}");
        // The pair and the filters are joined.
        let (flat, tree) = (widget(&d, "button:tree.flat").rect, widget(&d, "button:tree.tree").rect);
        assert_eq!(flat[0] + flat[2], tree[0], "{case}: Flat | Tree are joined");
        let filters: Vec<[f32; 4]> = toolbar::FILTER_IDS.iter().map(|id| widget(&d, id).rect).collect();
        for pair in filters.windows(2) {
            assert_eq!(pair[0][0] + pair[0][2], pair[1][0], "{case}: the filters are joined");
        }

        // The caption: above the toggles from 1100 wide (ink there), gone below (the toggles centred,
        // nothing above them).
        let (fx0, fy) = (filters[0][0], filters[0][1]);
        let fx1 = widget(&d, "slider:age").rect[0] + 100.0;
        let shot = d.screenshot().unwrap();
        if w >= 1100.0 {
            assert_eq!(fy, TOP + 13.0, "{case}: the toggles sit under the caption");
            assert!(ink_columns(&shot, fx0, fx1, TOP, fy) > 10, "{case}: the caption is drawn");
        } else {
            assert_eq!(fy, TOP + 7.0, "{case}: without the caption the toggles are centred");
            assert_eq!(ink_columns(&shot, fx0, fx1, TOP, fy - 1.0), 0, "{case}: no caption below 1100 wide");
        }

        // The search field's clear button is there while it holds text.
        assert!(d.widgets().iter().all(|w| w.id != "button:search.clear"), "{case}: no clear button while empty");
        toolbar::set_search_text(d.ctx(), "lisbon");
        d.settle(FRAMES).unwrap();
        let clear = widget(&d, "button:search.clear");
        assert_eq!(size(&clear), (18.0, 18.0), "{case}");
        assert!(clear.rect[0] + 18.0 <= w - 64.0 && clear.rect[1] > TOP && clear.rect[1] + 18.0 < TOP + HEIGHT, "{case}: {:?}", clear.rect);
        assert_eq!(toolbar::search_text(d.ctx()), "lisbon");
        toolbar::set_search_text(d.ctx(), "");
        d.settle(FRAMES).unwrap();

        let missing = d.app().panels().unwrap().unregistered_widgets();
        assert_eq!(missing, [], "{case}: interactive widgets without an id");
    }
}

/// The caption's threshold is 1100: shown at 1100 wide, hidden at 1099.
#[test]
fn the_filter_caption_hides_below_1100_wide() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (1100.0, 700.0));
    assert_eq!(widget(&d, "button:filter.starred").rect[1], TOP + 13.0);
    d.resize(1099, 700);
    d.settle(FRAMES).unwrap();
    assert_eq!(widget(&d, "button:filter.starred").rect[1], TOP + 7.0);
}

/// Hovering a filter replaces the caption with its tooltip (wider than "Filters").
#[test]
fn a_hovered_filter_shows_its_tooltip_in_the_caption() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (1400.0, 900.0));
    let starred = widget(&d, "button:filter.starred").rect;
    let right = widget(&d, "slider:age").rect[0] + 100.0;
    let (x0, x1) = (starred[0] - 60.0, right + 60.0);
    let caption = ink_columns(&d.screenshot().unwrap(), x0, x1, TOP, starred[1]);
    d.hover("button:filter.faces", 100).unwrap();
    let hovered = ink_columns(&d.screenshot().unwrap(), x0, x1, TOP, starred[1]);
    assert!(hovered > caption + 40, "the tooltip replaces the caption: {caption} → {hovered} columns of ink");
}

/// Controls without a command are present and disabled, and a click on them changes nothing; ▾
/// opens its popup, whose rows are listed: the folder sorts live (`view.folderSort`), the rest
/// disabled until their commands exist.
#[test]
fn controls_without_a_command_are_disabled_and_the_view_options_popup_opens() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (1400.0, 900.0));
    for (id, _) in CONTROLS {
        let wi = widget(&d, id);
        // ▾ and Flat | Tree (`view.folderView`) have their commands.
        let want = matches!(id, "button:toolbar.viewOptions" | "button:tree.flat" | "button:tree.tree");
        assert_eq!(wi.enabled, want, "`{id}` enabled: {}", wi.enabled);
    }
    let before = d.state().clone();
    assert!(d.click("button:toolbar.import").is_err(), "a disabled control does not take clicks");
    assert_eq!(d.state(), &before);

    d.click("button:toolbar.viewOptions").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(widget(&d, "button:toolbar.viewOptions").active, "▾ shows its popup open");
    let rows: Vec<String> = toolbar::VIEW_OPTIONS_ROWS.iter().filter_map(toolbar::ViewOption::widget_id).collect();
    for id in &rows {
        let row = widget(&d, id);
        let live = id.starts_with("menuitem:view.folderSort/");
        assert_eq!(row.enabled, live, "`{id}` is live only once its command exists");
        assert!(row.rect[1] >= TOP + HEIGHT - 1.0, "`{id}` opens below the toolbar: {:?}", row.rect);
    }
    assert!(widget(&d, "menuitem:view.folderSort/date").active, "Sort by Date is checked");
    d.key("Escape").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!widget(&d, "button:toolbar.viewOptions").active);
    assert!(d.widgets().iter().all(|w| !rows.contains(&w.id)), "the rows are gone with the popup");
}

/// The toolbar's controls follow the tree: widening it moves the filters and the search field right.
#[test]
fn the_filters_and_search_move_right_as_the_tree_widens() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = driver(dir.path(), (900.0, 600.0));
    let (filter, search) = (widget(&d, "button:filter.starred").rect[0], widget(&d, "field:search").rect[0]);
    d.app_mut().state_mut().tree_width = 380.0;
    d.settle(FRAMES).unwrap();
    let moved = widget(&d, "button:filter.starred").rect[0];
    assert!(moved > filter && moved >= 380.0 + 8.0, "the filters stay right of the splitter: {filter} → {moved}");
    assert!(widget(&d, "field:search").rect[0] > search);
    assert_eq!(widget(&d, "button:toolbar.import").rect[0], 8.0, "the left-hand buttons stay");
}
