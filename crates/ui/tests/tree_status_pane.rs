//! The tree pane, the splitter, the status line and the side-pane frame (M3.8), driven through the
//! control channel as an agent would: the splitter dragged by id keeps the tree within 200 … W − 480;
//! the Folders group header (30) and its rows (24); the status-line texts of `ui-layout.md` §6.1 for
//! no selection, one photo, a folder and several photos, which `ui.state` reports as `status`; the
//! side pane opened, widened and closed by id, and closing itself when the window narrows; the
//! regions in `ui.inspect` and `ui.widgets {region}`.
//!
//! Tree width and pane persistence across a restart is M3.6's (one owner for `settings.json`).

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::panels::status;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
}

/// A library of `photos` scenes photos spread evenly over `folders` folders, shown in a `w × h` window with a control
/// channel.
fn setup(folders: usize, photos: usize, (w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = dalica_scenes::demo_library(folders, photos, 5).unwrap();
    let mut roots = BTreeSet::new();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn code(t: &mut T, method: &str, params: Value) -> &'static str {
    match call(t, method, params.clone()) {
        Ok(v) => panic!("{method} {params} succeeded: {v}"),
        Err(e) => e.code.as_str(),
    }
}

fn rect(v: &Value) -> [f32; 4] {
    let a = v.as_array().unwrap_or_else(|| panic!("not a rect: {v}"));
    [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap() as f32)
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
}

fn widget(t: &mut T, id: &str) -> Value {
    let all = ok(t, "ui.widgets", json!({ "filter": id }));
    all.as_array().unwrap().iter().find(|w| w["id"] == id).cloned().unwrap_or_else(|| panic!("no `{id}`"))
}

fn status_text(t: &mut T) -> String {
    ok(t, "ui.state", json!({}))["status"].as_str().unwrap().to_string()
}

/// The status line's `text:status` label agrees with `ui.state`'s `status`.
fn assert_status(t: &mut T, want: &str, case: &str) {
    assert_eq!(status_text(t), want, "{case}: ui.state");
    assert_eq!(widget(t, "text:status")["label"], want, "{case}: the line drawn");
    assert_eq!(ok(t, "ui.inspect", json!({}))["status"], want, "{case}: ui.inspect");
}

/// The M3.8 done-when, part 1: the splitter dragged by id moves the tree within 200 … W − 480, at
/// both window sizes, and `ui.set` refuses a tree width outside those limits.
#[test]
fn the_splitter_dragged_by_id_moves_the_tree_within_its_limits() {
    for (w, h) in [(1400u32, 900u32), (900, 600)] {
        let mut t = setup(2, 6, (w, h));
        let wf = w as f32;
        let drag = |t: &mut T, dx: f32| {
            ok(t, "ui.dragWidget", json!({ "id": "splitter:tree", "dx": dx, "dy": 0 }));
            t.d.settle(FRAMES).unwrap();
            ok(t, "ui.state", json!({}))["treeWidth"].as_f64().unwrap() as f32
        };
        assert!((drag(&mut t, 50.0) - 290.0).abs() < 0.5, "{w}: the tree follows the pointer");
        let regions = ok(&mut t, "ui.inspect", json!({}))["regions"].clone();
        let tree = rect(&regions["tree"]);
        assert!((tree[2] - 290.0).abs() < 0.5, "{w}: the tree region follows: {tree:?}");
        assert!((rect(&regions["splitter"])[0] - tree[2]).abs() < 0.01);
        assert_eq!(drag(&mut t, -1000.0), 200.0, "{w}: the minimum");
        assert_eq!(drag(&mut t, 3000.0), wf - 480.0, "{w}: the light table keeps 480");
        let light = rect(&ok(&mut t, "ui.inspect", json!({}))["regions"]["lightTable"]);
        assert_eq!(light[2] + 8.0 + 16.0, 480.0, "{w}: splitter + light table + scroll bar");

        // `ui.set` takes a width inside the limits and refuses one outside them, changing nothing.
        assert_eq!(ok(&mut t, "ui.set", json!({ "treeWidth": 260 }))["treeWidth"], 260.0);
        for bad in [json!(199.5), json!(wf - 479.0), json!(-1), json!(1e39), json!("wide"), json!(null)] {
            assert_eq!(code(&mut t, "ui.set", json!({ "treeWidth": bad })), "bad_params", "{w}: treeWidth {bad}");
        }
        assert_eq!(ok(&mut t, "ui.set", json!({ "treeWidth": wf - 480.0 }))["treeWidth"], f64::from(wf - 480.0));
        assert_eq!(ok(&mut t, "ui.set", json!({ "treeWidth": 200 }))["treeWidth"], 200.0);
        assert_eq!(code(&mut t, "ui.set", json!({ "treeWidth": 100, "thumbSize": 160 })), "bad_params");
        assert_eq!(ok(&mut t, "ui.state", json!({}))["thumbSize"], 144.0, "a refused set changes nothing");
    }
}

/// The tree pane frame: the Folders group header (30, the count in its title) with 24-high rows
/// under it, all inside the tree region; clicking the header folds the rows away and back.
#[test]
fn the_tree_pane_has_the_folders_header_and_rows() {
    let mut t = setup(3, 12, (1400, 900));
    let tree = rect(&ok(&mut t, "ui.inspect", json!({}))["regions"]["tree"]);
    let header = widget(&mut t, "tree:group:folders");
    assert_eq!(header["label"], "Folders (3)");
    assert_eq!(header["checked"], true, "open");
    assert!(near(rect(&header["rect"]), [tree[0], tree[1], tree[2], 30.0]), "{header}");
    let rows: Vec<Value> = ok(&mut t, "ui.widgets", json!({ "filter": "tree:folder:" })).as_array().unwrap().clone();
    assert_eq!(rows.len(), 3);
    for (i, row) in rows.iter().enumerate() {
        let r = rect(&row["rect"]);
        assert!(near(r, [tree[0], tree[1] + 30.0 + 24.0 * i as f32, tree[2], 24.0]), "row {i}: {r:?}");
        assert_eq!(row["role"], "treeRow");
        assert!(row["label"].as_str().unwrap().ends_with("(4)"), "the count follows the name: {row}");
    }
    let rows_visited = |t: &mut T| ok(t, "ui.inspect", json!({}))["perf"]["treeRowsVisited"].as_u64();
    assert_eq!(rows_visited(&mut t), Some(3), "the header is not a row");

    ok(&mut t, "ui.clickWidget", json!({ "id": "tree:group:folders" }));
    assert_eq!(widget(&mut t, "tree:group:folders")["checked"], false);
    assert_eq!(ok(&mut t, "ui.widgets", json!({ "filter": "tree:folder:" })), json!([]), "folded");
    assert_eq!(rows_visited(&mut t), Some(0));
    ok(&mut t, "ui.clickWidget", json!({ "id": "tree:group:folders" }));
    assert_eq!(ok(&mut t, "ui.widgets", json!({ "filter": "tree:folder:" })).as_array().unwrap().len(), 3, "open again");
}

/// The M3.8 done-when, part 2: the status-line texts for no selection, one photo, a folder and
/// several photos, and `ui.state` reports the same text; a transient message replaces it for 4 s.
#[test]
fn the_status_line_says_what_is_selected_and_ui_state_reports_it() {
    let mut t = setup(2, 10, (1400, 900));
    assert_status(&mut t, "No selection", "nothing selected");

    let library = t.d.app().session().unwrap().library().clone();
    let folder = library.folders_in_display_order()[0].id;
    let photos = library.photos_in(folder);
    let ids: Vec<u64> = photos.iter().map(|p| p.id.get()).collect();

    // One photo: name, date and time, pixels, size on disk.
    ok(&mut t, "ui.clickWidget", json!({ "id": format!("thumb:{}", ids[0]) }));
    let p = library.photo(PhotoId::new(ids[0])).unwrap();
    assert!(p.width > 0 && p.sort_date().is_some(), "scenes photos have a size and a date");
    let one =
        [p.name.clone(), status::date_time(p.sort_date().unwrap()), format!("{} × {}", p.width, p.height), status::size(p.size)].join(status::SEP);
    assert_status(&mut t, &one, "one photo");
    assert!(one.starts_with(&p.name) && one.contains(" × ") && (one.ends_with(" KB") || one.ends_with(" MB")), "{one}");

    // Several photos: how many, their dates, their size on disk.
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [ids[0], ids[1], ids[2]] } }));
    t.d.settle(FRAMES).unwrap();
    let three: Vec<_> = ids[..3].iter().map(|i| library.photo(PhotoId::new(*i)).unwrap()).collect();
    let dates = status::date_range(three.iter().map(|p| p.sort_date().unwrap().date())).unwrap();
    let bytes: u64 = three.iter().map(|p| p.size).sum();
    assert_status(&mut t, &format!("3 pictures{0}{dates}{0}{1} on disk", status::SEP, status::size(bytes)), "three photos");

    // A folder from the tree, nothing selected on the light table.
    ok(&mut t, "engine.execute", json!({ "command": "select.none", "params": {} }));
    ok(&mut t, "ui.clickWidget", json!({ "id": format!("tree:folder:{}", folder.get()) }));
    let dates = status::date_range(photos.iter().map(|p| p.sort_date().unwrap().date())).unwrap();
    let bytes: u64 = photos.iter().map(|p| p.size).sum();
    let folder_text = format!("5 pictures{0}{dates}{0}{1} on disk", status::SEP, status::size(bytes));
    assert_status(&mut t, &folder_text, "a folder");

    // A refused action's reason replaces the text for 4 s, then the selection's text is back.
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "photo.star.toggle", "params": { "ids": [999_999] } })), "not_found");
    t.d.step();
    let message = status_text(&mut t);
    assert!(!message.is_empty() && message != folder_text, "the reason is shown: {message}");
    t.d.wait(3_500).unwrap();
    assert_eq!(status_text(&mut t), message, "still shown at 3.5 s");
    t.d.wait(600).unwrap();
    assert_status(&mut t, &folder_text, "the message is gone after 4 s");
    assert_eq!(code(&mut t, "ui.set", json!({ "status": "hello" })), "bad_params", "the line cannot be set");
}

/// The M3.8 done-when, part 3: the pane opens (Ctrl+0), widens and narrows with «, and closes with ×,
/// all by id; its frame is a 30-high header over an empty body; it pushes the light table.
#[test]
fn the_side_pane_opens_widens_and_closes_by_id() {
    let mut t = setup(1, 3, (1400, 900));
    let regions = |t: &mut T| ok(t, "ui.inspect", json!({}))["regions"].clone();
    assert_eq!(regions(&mut t)["sidePane"], Value::Null);
    assert_eq!(ok(&mut t, "ui.widgets", json!({ "region": "sidePane" })), json!([]), "a region not shown has no widgets");

    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+0" }));
    t.d.settle(FRAMES).unwrap();
    assert_eq!(ok(&mut t, "ui.state", json!({}))["pane"], "properties");
    let r = regions(&mut t);
    let pane = rect(&r["sidePane"]);
    assert!(near(pane, [1120.0, 72.0, 280.0, 712.0]), "{pane:?}");
    assert_eq!(rect(&r["scrollbar"])[0] + 16.0, 1120.0, "the light table is pushed, not covered");
    let ids: Vec<String> =
        ok(&mut t, "ui.widgets", json!({ "region": "sidePane" })).as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids, ["region:sidePane", "button:pane.widen", "button:pane.close", "pane:properties"]);
    let widen = rect(&widget(&mut t, "button:pane.widen")["rect"]);
    let close = rect(&widget(&mut t, "button:pane.close")["rect"]);
    assert!(widen[0] < pane[0] + 8.0 && widen[1] >= pane[1] && widen[1] + widen[3] <= pane[1] + 30.0, "« at the left of the header: {widen:?}");
    assert!(close[0] + close[2] > pane[0] + pane[2] - 8.0 && close[1] + close[3] <= pane[1] + 30.0, "× at the right: {close:?}");
    assert!(near(rect(&widget(&mut t, "pane:properties")["rect"]), [pane[0], pane[1] + 30.0, pane[2], pane[3] - 30.0]));
    assert_eq!(widget(&mut t, "pane:properties")["label"], "Properties");

    ok(&mut t, "ui.clickWidget", json!({ "id": "button:pane.widen" }));
    assert_eq!(ok(&mut t, "ui.state", json!({}))["paneLarge"], true);
    assert_eq!(rect(&regions(&mut t)["sidePane"])[2], 392.0, "large: min(30 % of 1400, 392)");
    assert_eq!(widget(&mut t, "button:pane.widen")["checked"], json!(null), "an icon button has no checked state");
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:pane.widen" }));
    assert_eq!(rect(&regions(&mut t)["sidePane"])[2], 280.0, "« again: the normal width");

    ok(&mut t, "ui.clickWidget", json!({ "id": "button:pane.close" }));
    let s = ok(&mut t, "ui.state", json!({}));
    assert_eq!((s["pane"].clone(), s["paneLarge"].clone()), (Value::Null, json!(false)));
    assert_eq!(regions(&mut t)["sidePane"], Value::Null);
    for gone in ["pane:", "button:pane."] {
        assert!(ok(&mut t, "ui.widgets", json!({ "filter": gone })).as_array().unwrap().is_empty(), "the frame is gone: {gone}");
    }
}

/// A window narrowing until the light table would drop below 480 first takes the pane's large width
/// away, then closes it (`ui-layout.md` §11).
#[test]
fn the_side_pane_closes_itself_when_the_window_narrows() {
    let mut t = setup(1, 3, (1400, 900));
    ok(&mut t, "ui.set", json!({ "pane": "tags", "paneLarge": true }));
    t.d.settle(FRAMES).unwrap();
    let state = |t: &mut T| {
        let s = ok(t, "ui.state", json!({}));
        (s["pane"].clone(), s["paneLarge"].clone())
    };
    t.d.resize(1100, 700);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(state(&mut t), (json!("tags"), json!(true)), "1100: 200 + 480 + 330 fits");
    t.d.resize(965, 700);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(state(&mut t), (json!("tags"), json!(false)), "965: the large pane no longer fits, the normal one does");
    assert_eq!(rect(&ok(&mut t, "ui.inspect", json!({}))["regions"]["sidePane"])[2], 280.0);
    t.d.resize(900, 600);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(state(&mut t), (Value::Null, json!(false)), "900: closed");
    assert_eq!(ok(&mut t, "ui.inspect", json!({}))["regions"]["sidePane"], Value::Null);
}

/// `ui.inspect` → `regions` lists every region of §6.6 (`null` when not shown) and `ui.widgets
/// {region}` keeps the widgets inside one.
#[test]
fn regions_are_reported_and_filter_the_widgets() {
    let mut t = setup(2, 3, (900, 600));
    let r = ok(&mut t, "ui.inspect", json!({}))["regions"].clone();
    let names: Vec<&str> = r.as_object().unwrap().keys().map(String::as_str).collect();
    for name in [
        "menuBar",
        "toolbar",
        "tabStrip",
        "tree",
        "splitter",
        "lightTable",
        "scrollbar",
        "sidePane",
        "statusLine",
        "tray",
        "trayButtons",
        "bottomControls",
        "actionButtons",
        "editPanel",
        "canvas",
        "filmstrip",
        "captionBar",
    ] {
        assert!(names.contains(&name), "`{name}` in {names:?}");
    }
    for hidden in ["tabStrip", "sidePane", "editPanel", "canvas", "filmstrip", "captionBar"] {
        assert_eq!(r[hidden], Value::Null, "{hidden}");
    }
    assert!(near(rect(&r["statusLine"]), [0.0, 484.0, 900.0, 20.0]));
    assert!(near(rect(&r["tree"]), [0.0, 72.0, 240.0, 412.0]));

    let tree: Vec<String> =
        ok(&mut t, "ui.widgets", json!({ "region": "tree" })).as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(tree.first().map(String::as_str), Some("region:tree"));
    assert!(tree.iter().skip(1).all(|id| id.starts_with("tree:")), "{tree:?}");
    assert_eq!(tree.iter().filter(|id| id.starts_with("tree:folder:")).count(), 2);
    let status_ids = ok(&mut t, "ui.widgets", json!({ "region": "statusLine", "filter": "text:" }));
    assert_eq!(status_ids.as_array().unwrap().len(), 1, "{status_ids}");
    for bad in [json!({ "region": "attic" }), json!({ "region": 3 }), json!({ "region": "tree", "x": 1 })] {
        assert_eq!(code(&mut t, "ui.widgets", bad.clone()), "bad_params", "{bad}");
    }
}

/// Never crash on bad input: a thumbnail size no check stopped (written straight into the view
/// state, as a damaged settings file or another writer could) is clamped where the light table
/// uses it: a frame renders and the cells have a sane size. (A non-finite size ends up in range,
/// at the default or wherever the bottom bar's size slider puts it.)
#[test]
fn a_hostile_thumbnail_size_renders_a_sane_light_table() {
    let mut t = setup(1, 4, (1400, 900));
    for (bad, want) in [(f32::NAN, None), (f32::INFINITY, None), (f32::NEG_INFINITY, None), (-1.0, Some(64.0)), (0.0, Some(64.0)), (1e9, Some(320.0))]
    {
        t.d.app_mut().state_mut().thumb_size = bad;
        t.d.settle(FRAMES).unwrap();
        let cells = ok(&mut t, "ui.widgets", json!({ "filter": "thumb:" }));
        let cells: Vec<[f32; 4]> = cells.as_array().unwrap().iter().filter(|w| w["role"] == "thumbnail").map(|w| rect(&w["rect"])).collect();
        assert!(!cells.is_empty(), "thumb size {bad}: cells are laid out");
        let cell = want.unwrap_or(cells[0][2]);
        assert!((64.0..=320.0).contains(&cell), "thumb size {bad}: cell {cell}");
        // Rects are clipped to the view: a cell of a lower row may show only its top.
        for r in &cells {
            assert!(r[2] == cell && r[3] > 0.0 && r[3] <= cell, "thumb size {bad}: {r:?}");
        }
        assert!(cells.iter().any(|r| r[3] == cell), "thumb size {bad}: a whole cell is {cell} square");
    }
    assert_eq!(dalica_ui::UiState { thumb_size: f32::NAN, ..dalica_ui::UiState::default() }.sanitized().thumb_size, 144.0);
}
