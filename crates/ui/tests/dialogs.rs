//! The dialog host (M5.14, `design-system.md` §6 `dialog`, `control-protocol.md` §6.6, §6.10),
//! driven headlessly at 1400 × 900: Tools ▸ Folder Manager… opens its dialog by command (every
//! widget of it, the backdrop included, has an id); Esc cancels and Enter presses the default
//! button; `ui.inspect` reports the dialog, and the window under it takes no clicks or keys;
//! `ui.dialog.confirm` / `ui.dialog.cancel` answer it; the tray's Clear confirmation asks through
//! the host. Also the M4.R loose ends of `ui.inspect` (scroll,
//! animations, the open popup and tooltip) and `ui.menu.tree {context: "tray"}`.
//!
//! `DALICA_SHOTS=<dir>` writes screenshots of the open dialogs at 1400 × 900 and 900 × 600 there.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver};
use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

/// A library in `root` with two folders of `per` scenes photos each, added and scanned.
fn session(root: &Path, per: usize) -> Session {
    let files = dalica_scenes::demo_library(2, 2 * per, 13).unwrap();
    let mut folders = BTreeSet::new();
    for f in &files {
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
    s
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
}

fn setup(per: usize) -> T {
    let dir = tempfile::tempdir().unwrap();
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(session(dir.path(), per)), control.picker()), 1400, 900, 1.0);
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

fn inspect(t: &mut T) -> Value {
    ok(t, "ui.inspect", json!({}))
}

fn has_widget(t: &T, id: &str) -> bool {
    t.d.widgets().iter().any(|w| w.id == id)
}

/// The photos of the light table's sections, in order.
fn sections(t: &T) -> Vec<Vec<PhotoId>> {
    t.d.app().panels().unwrap().sections.list.iter().map(|s| s.photos.clone()).collect()
}

/// Writes a screenshot to `$DALICA_SHOTS/<name>-<w>x<h>.png` at both window sizes, when asked.
fn shots(t: &mut T, name: &str) {
    let Some(dir) = std::env::var_os("DALICA_SHOTS") else { return };
    for (w, h) in [(1400, 900), (900, 600)] {
        ok(t, "ui.resize", json!({ "width": w, "height": h }));
        let path = Path::new(&dir).join(format!("{name}-{w}x{h}.png"));
        let _ = std::fs::remove_file(&path);
        t.control = Control::new(Config { write_roots: vec![Path::new(&dir).to_path_buf()], ..Config::default() });
        ok(t, "ui.screenshot", json!({ "path": path }));
    }
    ok(t, "ui.resize", json!({ "width": 1400, "height": 900 }));
}

/// Tools ▸ Folder Manager… opens its dialog; `ui.inspect` reports it; Esc cancels, Enter presses
/// OK; the window under it takes no clicks or keys; one of each kind at a time.
#[test]
fn a_dialog_opens_by_command_and_esc_and_enter_close_it() {
    let mut t = setup(6);
    assert_eq!(inspect(&mut t)["dialog"], Value::Null);
    assert_eq!(ok(&mut t, "ui.menu.invoke", json!({ "path": "Tools/Folder Manager" })), json!({ "dialog": "folderManager" }));
    let v = inspect(&mut t);
    let d = &v["dialog"];
    assert_eq!(
        (d["kind"].as_str(), d["widget"].as_str(), d["title"].as_str()),
        (Some("folderManager"), Some("dialog:folderManager"), Some("Folder Manager"))
    );
    assert_eq!(d["buttons"], json!(["button:dialog.help", "button:dialog.cancel", "button:dialog.ok"]));
    // The draft: nothing selected (no library folder was), nothing changed.
    assert_eq!((d["default"].as_str(), &d["fields"]), (Some("button:dialog.ok"), &json!({ "ready": true })));
    assert_eq!(v["dialogs"], json!([{ "kind": "folderManager", "fields": { "ready": true } }]));
    for id in ["dialog:folderManager", "dialog:folderManager/backdrop", "button:dialog.ok", "button:dialog.cancel", "button:dialog.help"] {
        assert!(has_widget(&t, id), "{id}");
    }
    assert!(!t.d.widgets().iter().find(|w| w.id == "button:dialog.help").unwrap().enabled, "Help waits for help");
    shots(&mut t, "dialog-folder-manager");
    // Open: the menu item says why it cannot open again, and the command refuses.
    let tree = ok(&mut t, "ui.menu.tree", json!({}));
    let tools = tree.as_array().unwrap().iter().find(|m| m["label"] == "Tools").unwrap();
    let item = tools["children"].as_array().unwrap().iter().find(|i| i["id"] == "folders.manager").unwrap();
    assert_eq!((item["live"].as_bool(), item["enabled"].as_bool(), item["dialog"].as_str()), (Some(true), Some(false), Some("folderManager")));
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "folders.manager" })), "disabled");
    // Under the dialog: a click on a thumbnail lands on the backdrop, a shortcut does not run.
    let first = sections(&t)[0][0];
    ok(&mut t, "ui.clickWidget", json!({ "id": format!("thumb:{}", first.get()) }));
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+A" }));
    assert!(t.d.state().selection.ids.is_empty(), "the window under a dialog takes no input");
    // Esc cancels.
    ok(&mut t, "ui.key", json!({ "key": "Escape" }));
    assert!(t.d.state().dialogs.is_empty() && !has_widget(&t, "dialog:folderManager"));
    assert_eq!(inspect(&mut t)["dialog"], Value::Null);
    // Keys reach the window again.
    ok(&mut t, "ui.key", json!({ "combo": "Ctrl+A" }));
    assert!(!t.d.state().selection.ids.is_empty(), "Ctrl+A selects once the dialog is gone");
    ok(&mut t, "engine.execute", json!({ "command": "select.none" }));
    // Enter presses the default button (OK with nothing changed closes it).
    assert_eq!(ok(&mut t, "engine.execute", json!({ "command": "folders.manager" })), json!({ "dialog": "folderManager" }));
    ok(&mut t, "ui.settle", json!({}));
    ok(&mut t, "ui.key", json!({ "key": "Enter" }));
    assert!(t.d.state().dialogs.is_empty());
    // The buttons by click.
    ok(&mut t, "engine.execute", json!({ "command": "folders.manager" }));
    ok(&mut t, "ui.settle", json!({}));
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:dialog.cancel" }));
    assert!(t.d.state().dialogs.is_empty());
    // `ui.dialog.confirm` and `ui.dialog.cancel`.
    assert_eq!(code(&mut t, "ui.dialog.confirm", json!({})), "not_found");
    assert_eq!(code(&mut t, "ui.dialog.cancel", json!({})), "not_found");
    ok(&mut t, "ui.menu.invoke", json!({ "id": "folders.manager" }));
    assert_eq!(code(&mut t, "ui.dialog.confirm", json!({ "button": "dialog.help" })), "disabled");
    assert_eq!(code(&mut t, "ui.dialog.confirm", json!({ "button": "dialog.nope" })), "not_found");
    for bad in [json!({ "button": "ok" }), json!({ "button": 3 }), json!({ "button": "dialog." }), json!({ "zzz": 1 }), json!([1])] {
        assert_eq!(code(&mut t, "ui.dialog.confirm", bad.clone()), "bad_params", "{bad}");
    }
    assert_eq!(code(&mut t, "ui.dialog.cancel", json!({ "button": "dialog.cancel" })), "bad_params");
    assert_eq!(t.d.state().dialogs.len(), 1, "refused answers keep the dialog");
    assert_eq!(ok(&mut t, "ui.dialog.cancel", json!({})), json!({}));
    ok(&mut t, "ui.menu.invoke", json!({ "id": "folders.manager" }));
    assert_eq!(ok(&mut t, "ui.dialog.confirm", json!({ "button": "button:dialog.ok" })), json!({ "changed": 0 }), "nothing changed, nothing run");
    assert!(t.d.state().dialogs.is_empty());
    // `ui.set` does not open or change dialogs.
    assert_eq!(code(&mut t, "ui.set", json!({ "dialogs": [{ "kind": "folderManager" }] })), "bad_params");
    assert!(t.d.state().dialogs.is_empty());
}

/// Clear with held photos asks through the host: the Clear button opens `dialog:clearTray`, Esc
/// keeps the tray, `ui.dialog.confirm` clears it; over `engine.execute` the question is
/// `needs_confirm` with the dialog's prompt.
#[test]
fn the_tray_confirmation_works_through_the_host() {
    let mut t = setup(6);
    let s = sections(&t);
    let ids = |v: &[PhotoId]| v.iter().map(|p| p.get()).collect::<Vec<_>>();
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": ids(&s[0][0..2]) } }));
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:tray.hold" }));
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": ids(&s[1][0..1]) } }));
    ok(&mut t, "ui.settle", json!({}));
    // Over automation: the question, nothing cleared.
    let e = call(&mut t, "engine.execute", json!({ "command": "tray.clear" })).unwrap_err();
    assert_eq!(e.code.as_str(), "needs_confirm");
    let prompt = e.prompt.unwrap();
    assert_eq!((prompt["title"].as_str(), prompt["dialog"].as_str()), (Some("Clear Tray"), Some("clearTray")));
    assert_eq!(prompt["buttons"], json!(["Clear Tray", "Cancel"]));
    assert!(t.d.state().dialogs.is_empty(), "engine.execute never opens a dialog");
    // The button asks through the host.
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:tray.clear" }));
    let v = inspect(&mut t);
    assert_eq!(v["dialog"]["kind"], "clearTray");
    assert_eq!(
        (v["dialog"]["default"].as_str(), &v["dialog"]["command"]),
        (Some("button:dialog.clearTray"), &json!({ "id": "tray.clear", "params": {} }))
    );
    assert!(has_widget(&t, "dialog:clearTray"));
    shots(&mut t, "dialog-clear-tray");
    ok(&mut t, "ui.key", json!({ "key": "Escape" }));
    assert!(t.d.state().dialogs.is_empty());
    assert_eq!((t.d.state().tray.held.len(), t.d.state().selection.ids.len()), (2, 1), "Cancel keeps everything");
    // Confirmed over automation: everything goes.
    ok(&mut t, "ui.clickWidget", json!({ "id": "button:tray.clear" }));
    let r = ok(&mut t, "ui.dialog.confirm", json!({}));
    assert_eq!(r["count"], 0, "{r}");
    assert!(t.d.state().dialogs.is_empty() && t.d.state().tray.held.is_empty() && t.d.state().selection.ids.is_empty());
    // A confirmed command needs no dialog; one with nothing held asks nothing.
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": ids(&s[0][0..1]) } }));
    assert_eq!(ok(&mut t, "engine.execute", json!({ "command": "tray.clear" }))["count"], 0);
    assert!(t.d.state().dialogs.is_empty());
}

/// `ui.inspect` reports the scroll offsets and a running scroll animation, the open popup and the
/// tooltip shown; `ui.menu.tree {context: "tray"}` lists the tray menu's rows.
#[test]
fn inspect_reports_scroll_animations_popups_tooltips_and_the_tray_menu() {
    let mut t = setup(30);
    let v = inspect(&mut t);
    assert_eq!(v["scroll"], json!({ "section": null, "offset": 0.0 }), "the view state's anchor");
    let views = &v["scrollViews"];
    assert_eq!(
        (views["lightTable"].as_f64(), views["tree"].as_f64(), &views["topSection"], &views["sticky"]),
        (Some(0.0), Some(0.0), &Value::Null, &Value::Null)
    );
    assert_eq!(v["animations"], json!([]));
    assert_eq!((v["popup"].clone(), v["tooltip"].clone()), (Value::Null, Value::Null));
    // A tooltip shown on hover: its title, and its second line when it has one.
    ok(&mut t, "ui.hoverWidget", json!({ "id": "indicator:activity", "ms": 900 }));
    assert_eq!(inspect(&mut t)["tooltip"], "No background work");
    ok(&mut t, "ui.hoverWidget", json!({ "id": "button:filter.faces", "ms": 900 }));
    assert_eq!(inspect(&mut t)["tooltip"], "Show only photos with faces\nAvailable once Dalica finds faces");
    ok(&mut t, "ui.pointer", json!({ "events": [{ "kind": "move", "x": 700, "y": 500 }] }));
    ok(&mut t, "ui.wait", json!({ "ms": 100 }));
    // Scrolling to the second section eases there: the animation runs, then the offsets say where.
    let second = t.d.app().panels().unwrap().sections.list[1].id.to_string();
    ok(&mut t, "engine.execute", json!({ "command": "section.scrollTo", "params": { "section": second } }));
    let running = inspect(&mut t)["animations"].clone();
    assert_eq!(running, json!([{ "kind": "scrollToSection", "region": "lightTable" }]));
    ok(&mut t, "ui.settle", json!({}));
    let v = inspect(&mut t);
    assert_eq!(v["animations"], json!([]));
    let views = &v["scrollViews"];
    assert!(views["lightTable"].as_f64().unwrap() > 100.0, "{views}");
    assert_eq!((v["scroll"]["section"].as_str(), views["topSection"].as_str()), (Some(second.as_str()), Some(second.as_str())));
    // The File menu open: its rows, then closed again.
    ok(&mut t, "ui.clickWidget", json!({ "id": "menu:File" }));
    let popup = inspect(&mut t)["popup"].clone();
    assert_eq!((popup["kind"].as_str(), popup["menu"].as_str()), (Some("menu"), Some("File")));
    let add = popup["items"].as_array().unwrap().iter().find(|i| i["widget"] == "menuitem:folders.manager").cloned().unwrap();
    assert_eq!((add["id"].as_str(), add["label"].as_str(), add["enabled"].as_bool()), (Some("folders.manager"), Some("Add Folder…"), Some(true)));
    ok(&mut t, "ui.key", json!({ "key": "Escape" }));
    assert_eq!(inspect(&mut t)["popup"], Value::Null);
    // The tray menu: its rows for the tray's photos.
    let s = sections(&t);
    ok(&mut t, "engine.execute", json!({ "command": "select.set", "params": { "ids": [s[0][0].get(), s[0][1].get()] } }));
    let rows = ok(&mut t, "ui.menu.tree", json!({ "context": "tray" }));
    let labels: Vec<&str> = rows.as_array().unwrap().iter().filter_map(|r| r["label"].as_str()).collect();
    assert_eq!(labels, ["View and Edit", "Hold", "Remove from Tray", "Rotate Clockwise", "Rotate Counterclockwise", "Locate on Disk", "Properties"]);
    let remove = rows.as_array().unwrap().iter().find(|r| r["id"] == "tray.clear").unwrap();
    assert_eq!((remove["widget"].as_str(), remove["enabled"].as_bool()), (Some("menuitem:tray.clear"), Some(true)));
    assert!(rows.as_array().unwrap().iter().any(|r| r["separator"] == true));
}
