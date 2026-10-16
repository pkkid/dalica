//! Every checkable menu item (M10.25, `menus-and-shortcuts.md` §1, parity row `SYS-COMMANDS`),
//! driven through the control channel as an agent would: the bar's check and radio rows are listed
//! (a new one fails here until it is wired), and each mark follows the state it reads both ways:
//! the item's command, run as a click (`ui.menu.invoke`), and `ui.set` on the view state it shows.
//! The two marks the engine keeps (Use Color Management, a session setting; Sort By, the folder's
//! photo sort in the library) follow `engine.execute` instead of `ui.set`.

use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_engine::Session;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::menus::THUMB_SMALL;
use dalica_ui::state::THUMB_DEFAULT;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    /// The first photo and its folder.
    photo: u64,
    folder: u64,
}

/// A library of two folders of three photos each, the first folder's section selected.
fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(2, 6, 5).unwrap();
    let mut roots = std::collections::BTreeSet::new();
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
    let first = s.library().photos().min_by_key(|p| p.id).unwrap();
    let (photo, folder) = (first.id.get(), first.folder.get());
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    d.click(&format!("tree:folder:{folder}")).unwrap();
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d, photo, folder }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

/// Every row of `nodes` with a mark, as `(id, params, radio, live)`, in menu order.
fn marked(nodes: &[Value], out: &mut Vec<(String, Value, bool, bool)>) {
    for n in nodes {
        if let Some(children) = n["children"].as_array() {
            marked(children, out);
        } else if n.get("checked").is_some() {
            let id = n["id"].as_str().unwrap().to_string();
            out.push((id, n.get("params").cloned().unwrap_or(Value::Null), n["radio"] == true, n["live"] == true));
        }
    }
}

fn rows(t: &mut T) -> Vec<(String, Value, bool, bool)> {
    let tree = ok(t, "ui.menu.tree", json!({}));
    let mut out = Vec::new();
    marked(tree.as_array().unwrap(), &mut out);
    out
}

/// Whether the row running `id` with `params` is checked now.
fn mark(t: &mut T, id: &str, params: &Value) -> bool {
    let tree = ok(t, "ui.menu.tree", json!({}));
    let row = tree.as_array().unwrap().iter().find_map(|m| find(m, id, params)).unwrap_or_else(|| panic!("no row {id} {params}"));
    row["checked"] == true
}

fn find(node: &Value, id: &str, params: &Value) -> Option<Value> {
    if let Some(children) = node["children"].as_array() {
        return children.iter().find_map(|c| find(c, id, params));
    }
    (node["id"] == id && node.get("params").cloned().unwrap_or(Value::Null) == *params).then(|| node.clone())
}

/// Runs the row as a click does.
fn invoke(t: &mut T, id: &str, params: &Value) {
    ok(t, "ui.menu.invoke", json!({ "id": id, "params": params }));
    t.d.settle(FRAMES).unwrap();
}

fn set(t: &mut T, state: Value) {
    ok(t, "ui.set", state);
    t.d.settle(FRAMES).unwrap();
}

/// The marks of the rows running `id` (each with its params), in menu order.
fn marks_of(t: &mut T, id: &str) -> Vec<bool> {
    let params: Vec<Value> = rows(t).into_iter().filter(|r| r.0 == id).map(|r| r.1).collect();
    params.iter().map(|p| mark(t, id, p)).collect()
}

/// The bar's checkable rows: every one is listed here, each is live (Simplified Tree View waits
/// for the simplified tree, `LIB-TREE-TREE`, and stays off), and each mark follows its command and
/// `ui.set` (or `engine.execute` for the engine's two).
#[test]
fn every_checkable_item_follows_its_command_and_ui_set() {
    let mut t = setup();
    let listed: Vec<(String, Value, bool)> = rows(&mut t).into_iter().map(|(id, p, radio, _)| (id, p, radio)).collect();
    let caption = |c: &str| ("view.caption".to_string(), json!({ "caption": c }), true);
    let sort = |p: Value, radio: bool| ("section.sort".to_string(), p, radio);
    let plain = |id: &str, radio: bool| (id.to_string(), Value::Null, radio);
    let expected = vec![
        plain("view.thumbsSmall", true),
        plain("view.thumbsNormal", true),
        plain("pane.properties", false),
        plain("pane.tags", false),
        plain("pane.people", false),
        plain("pane.places", false),
        plain("edit.panelToggle", false),
        plain("view.smallPictures", false),
        plain("view.hiddenPictures", false),
        plain("view.colorManagement", false),
        caption("none"),
        caption("filename"),
        caption("caption"),
        caption("tags"),
        caption("resolution"),
        caption("date"),
        ("view.folderView".into(), json!({ "mode": "flat" }), true),
        ("view.folderView".into(), json!({ "mode": "tree" }), true),
        plain("view.simplifiedTree", false),
        sort(json!({ "by": "date" }), true),
        sort(json!({ "by": "name" }), true),
        sort(json!({ "by": "size" }), true),
        sort(json!({ "reverse": true }), false),
    ];
    assert_eq!(listed, expected, "every checkable row of the bar, in the order the tree lists them (a submenu's rows after its menu's own)");
    for (id, params, _, live) in rows(&mut t) {
        assert_eq!(live, id != "view.simplifiedTree", "{id} {params}");
    }
    assert!(!mark(&mut t, "view.simplifiedTree", &Value::Null), "not built yet: off");

    // Captions: one radio on, by the command and by ui.set.
    let captions = ["none", "filename", "caption", "tags", "resolution", "date"];
    for (i, c) in captions.iter().enumerate() {
        invoke(&mut t, "view.caption", &json!({ "caption": c }));
        assert_eq!(marks_of(&mut t, "view.caption"), (0..6).map(|k| k == i).collect::<Vec<_>>(), "invoked {c}");
    }
    for (i, c) in captions.iter().enumerate().rev() {
        set(&mut t, json!({ "caption": c }));
        assert_eq!(marks_of(&mut t, "view.caption"), (0..6).map(|k| k == i).collect::<Vec<_>>(), "ui.set {c}");
    }

    // Folder view: flat or tree.
    invoke(&mut t, "view.folderView", &json!({ "mode": "tree" }));
    assert_eq!(marks_of(&mut t, "view.folderView"), [false, true]);
    set(&mut t, json!({ "treeMode": "flat" }));
    assert_eq!(marks_of(&mut t, "view.folderView"), [true, false]);
    invoke(&mut t, "view.folderView", &json!({ "mode": "flat" }));
    assert_eq!(marks_of(&mut t, "view.folderView"), [true, false], "choosing the checked radio keeps it");

    // Thumbnail size: Small or Normal.
    let sizes = |t: &mut T| (mark(t, "view.thumbsSmall", &Value::Null), mark(t, "view.thumbsNormal", &Value::Null));
    assert_eq!(sizes(&mut t), (false, true));
    invoke(&mut t, "view.thumbsSmall", &Value::Null);
    assert_eq!(sizes(&mut t), (true, false));
    set(&mut t, json!({ "thumbSize": THUMB_DEFAULT }));
    assert_eq!(sizes(&mut t), (false, true));
    set(&mut t, json!({ "thumbSize": THUMB_SMALL }));
    assert_eq!(sizes(&mut t), (true, false));
    invoke(&mut t, "view.thumbsNormal", &Value::Null);
    assert_eq!(sizes(&mut t), (false, true));
    set(&mut t, json!({ "thumbSize": 200.0 }));
    assert_eq!(sizes(&mut t), (false, false), "a size of the slider's own: neither");

    // The side panes: one at a time, a second click closes it.
    let panes = ["properties", "tags", "people", "places"];
    let pane_marks = |t: &mut T| panes.map(|p| mark(t, &format!("pane.{p}"), &Value::Null));
    for (i, p) in panes.iter().enumerate() {
        invoke(&mut t, &format!("pane.{p}"), &Value::Null);
        assert_eq!(pane_marks(&mut t), panes.map(|q| q == *p), "invoked {p}");
        invoke(&mut t, &format!("pane.{p}"), &Value::Null);
        assert_eq!(pane_marks(&mut t), [false; 4], "invoked {p} again");
        set(&mut t, json!({ "pane": p }));
        assert_eq!(pane_marks(&mut t), panes.map(|q| q == *p), "ui.set {p}");
        set(&mut t, json!({ "pane": null }));
        assert_eq!(pane_marks(&mut t), [false; 4], "{i}: ui.set no pane");
    }

    // Small Pictures and Hidden Pictures: on and off each way.
    for (id, field) in [("view.smallPictures", "smallPictures"), ("view.hiddenPictures", "hiddenPictures")] {
        assert!(!mark(&mut t, id, &Value::Null));
        invoke(&mut t, id, &Value::Null);
        assert!(mark(&mut t, id, &Value::Null), "{id} invoked");
        set(&mut t, json!({ field: false }));
        assert!(!mark(&mut t, id, &Value::Null), "{id} ui.set off");
        set(&mut t, json!({ field: true }));
        assert!(mark(&mut t, id, &Value::Null), "{id} ui.set on");
        invoke(&mut t, id, &Value::Null);
        assert!(!mark(&mut t, id, &Value::Null), "{id} invoked again");
    }

    // Use Color Management: the session's setting, by the command and by engine.execute.
    let cm = "view.colorManagement";
    assert!(mark(&mut t, cm, &Value::Null), "on until the library says otherwise");
    invoke(&mut t, cm, &Value::Null);
    assert!(!mark(&mut t, cm, &Value::Null));
    ok(&mut t, "engine.execute", json!({ "command": cm, "params": {} }));
    assert!(mark(&mut t, cm, &Value::Null), "engine.execute turns it back on");

    // Sort By: the selected folder's photo sort, by the command and by engine.execute.
    let sorts = |t: &mut T| marks_of(t, "section.sort");
    assert_eq!(sorts(&mut t), [true, false, false, false], "by date, oldest first: the default");
    invoke(&mut t, "section.sort", &json!({ "by": "name" }));
    assert_eq!(sorts(&mut t), [false, true, false, false]);
    invoke(&mut t, "section.sort", &json!({ "reverse": true }));
    assert_eq!(sorts(&mut t), [false, true, false, true], "Reverse Order turns the sort around and keeps its key");
    let section = format!("folder:{}", t.folder);
    ok(&mut t, "engine.execute", json!({ "command": "section.sort", "params": { "section": section, "by": "size", "reverse": false } }));
    assert_eq!(sorts(&mut t), [false, false, true, false], "engine.execute");
    ok(&mut t, "engine.execute", json!({ "command": "edit.undo", "params": {} }));
    assert_eq!(sorts(&mut t), [false, true, false, true], "undo puts the sort back, and its marks");
    // An album is a manual order: no mark.
    let photo = t.photo;
    ok(&mut t, "engine.execute", json!({ "command": "photo.star.set", "params": { "ids": [photo], "on": true } }));
    t.d.settle(FRAMES).unwrap();
    t.d.click("tree:album:starred").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(sorts(&mut t), [false; 4], "Starred Photos: no folder sort");

    // Show Edit Controls: the edit view's panel.
    invoke(&mut t, "view.edit", &json!({ "id": photo }));
    let panel = "edit.panelToggle";
    assert!(mark(&mut t, panel, &Value::Null), "the panel shows");
    invoke(&mut t, panel, &Value::Null);
    assert!(!mark(&mut t, panel, &Value::Null));
    set(&mut t, json!({ "edit": { "panelVisible": true } }));
    assert!(mark(&mut t, panel, &Value::Null), "ui.set shows it");
    set(&mut t, json!({ "edit": { "panelVisible": false } }));
    assert!(!mark(&mut t, panel, &Value::Null), "ui.set hides it");
}
