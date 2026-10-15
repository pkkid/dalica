//! The control method `library.inspect` (`control-protocol.md` §6.4): the engine's inspection over
//! the window's tree, plus the window's fields (`current`, `visibleSections`, `selection`, `tray`,
//! `scroll`), agreeing with `ui.inspect`; graceful failures. What the engine's part holds is
//! `crates/engine/tests/library_inspect.rs`'s.

use std::sync::Arc;

use crate::common::new_driver;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

struct T {
    _dir: Option<dalica_fs::TempDir>,
    control: Control,
    conn: Connection,
    d: Driver,
}

fn setup(library: bool) -> T {
    let (dir, session) = if library {
        let dir = dalica_engine::demo::fresh_dir().unwrap();
        let session = crate::common::demo_open(dir.path(), Arc::new(dalica_core::SystemClock));
        (Some(dir), Some(session))
    } else {
        (None, None)
    };
    let control = Control::new(Config { demo: true, ..Config::default() });
    let mut d = new_driver(App::with_picker(session, control.picker()), 1400, 900, 1.0);
    if library {
        d.settle(600).unwrap();
    }
    T { _dir: dir, control, conn: Connection::new(), d }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn run(t: &mut T, command: &str, params: Value) -> Value {
    ok(t, "engine.execute", json!({ "command": command, "params": params }))
}

fn code(t: &mut T, params: Value) -> &'static str {
    match call(t, "library.inspect", params.clone()) {
        Ok(v) => panic!("library.inspect {params} answered {v}"),
        Err(e) => e.code.as_str(),
    }
}

#[test]
fn the_method_adds_the_window_fields_and_agrees_with_ui_inspect() {
    let mut t = setup(true);
    run(&mut t, "select.set", json!({ "ids": [3, 4] }));
    run(&mut t, "tray.hold", json!({}));
    run(&mut t, "select.set", json!({ "ids": [5, 6] }));
    ok(&mut t, "ui.settle", json!({}));
    let v = ok(&mut t, "library.inspect", json!({ "photos": true, "section": "folder:1" }));
    let ui = ok(&mut t, "ui.inspect", json!({}));

    for key in ["revision", "tree", "sections", "search", "undo", "current", "visibleSections", "selection", "tray", "scroll"] {
        assert!(v.get(key).is_some(), "{key}");
    }
    let sel = &v["selection"];
    assert_eq!((&sel["ids"], &sel["section"], &sel["anchor"]), (&ui["selection"]["ids"], &ui["selection"]["section"], &ui["selection"]["anchor"]));
    assert_eq!((&sel["ids"], &sel["wholeSection"]), (&json!([5, 6]), &json!(false)));
    assert_eq!(v["tray"], json!({ "token": null, "items": [3, 4, 5, 6], "held": [3, 4], "traySelected": [] }));
    assert_eq!(v["scroll"], ui["scroll"]);
    assert_eq!(v["current"], "folder:1", "the top of the light table");
    assert_eq!(v["visibleSections"][0], "folder:1");
    assert_eq!(v["view"]["thumbSize"], ui["thumbSize"]);
    let photo = |id: u64| v["photos"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
    assert_eq!((&photo(3)["held"], &photo(3)["selected"]), (&json!(true), &json!(false)));
    assert_eq!((&photo(5)["held"], &photo(5)["selected"]), (&json!(false), &json!(true)));

    // The tree is the window's: a collapsed group shows collapsed, and its sections leave.
    run(&mut t, "view.groupCollapse", json!({ "group": "folders", "collapsed": true }));
    let v = ok(&mut t, "library.inspect", json!({}));
    let folders = v["tree"]["groups"].as_array().unwrap().iter().find(|g| g["group"] == "folders").unwrap().clone();
    assert_eq!((&folders["collapsed"], &folders["rows"]), (&json!(true), &json!([])));
    assert!(v["sections"].as_array().unwrap().iter().all(|s| !s["section"].as_str().unwrap().starts_with("folder:")));
}

#[test]
fn bad_requests_fail_gracefully() {
    let mut t = setup(true);
    for p in [json!({ "photos": true, "limit": 5000 }), json!({ "offset": 1 }), json!({ "nope": 1 }), json!("photos"), json!([1])] {
        assert_eq!(code(&mut t, p.clone()), "bad_params", "{p}");
    }
    assert_eq!(code(&mut t, json!({ "section": "folder:99" })), "not_found");
    let mut empty = setup(false);
    assert_eq!(code(&mut empty, json!({})), "disabled", "the library is still opening");
}
