//! M7's view state and the edit view's frame (M7.4), driven through the control channel as an
//! agent would: every field of `edit` round-trips through `ui.state` / `ui.set` (bad values are
//! `bad_params`, a photo that does not exist `not_found`, and nothing changes); Ctrl+3 and Enter on
//! a selected photo show every edit-view region at the rects of `editing.md` §2 at 1400 × 900 and
//! 900 × 600, and Esc and Ctrl+1 return; Ctrl+9 hides and shows the edit panel; Ctrl+3 with nothing
//! selected gives the toast; an open tool takes Esc first; every interactive widget of the frame
//! has a valid, unique id; the frame matches its fingerprints at both sizes
//! (`tests/fingerprints/edit-frame-<w>x<h>.json`).
//!
//! `DALICA_SHOTS=<dir>` writes the frame's screenshots there too.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry;
use serde_json::{Value, json};

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// A session on a library under `dir` holding one folder of 4 scenes photos; their ids.
fn session(dir: &Path) -> (Session, Vec<u64>) {
    let files = dalica_scenes::demo_library(1, 4, 11).unwrap();
    let mut roots = std::collections::BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 4);
    (s, ids)
}

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    ids: Vec<u64>,
}

fn setup((w, h): (u32, u32)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (s, ids) = session(dir.path());
    let control = Control::new(Config::default());
    let mut d = Driver::new(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d, ids }
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

/// The view state without `status` (the status line's text, which a frame may rewrite).
fn state(t: &mut T) -> Value {
    let mut v = ok(t, "ui.state", json!({}));
    v.as_object_mut().unwrap().remove("status");
    v
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    ok(t, "engine.execute", json!({ "command": id, "params": params }))
}

fn key(t: &mut T, combo: &str) {
    ok(t, "ui.key", json!({ "combo": combo }));
    t.d.settle(FRAMES).unwrap();
}

fn regions(t: &mut T) -> Value {
    ok(t, "ui.inspect", json!({}))["regions"].clone()
}

fn select(t: &mut T, id: u64) {
    run(t, "select.set", json!({ "ids": [id] }));
    t.d.settle(FRAMES).unwrap();
}

/// Each field of `edit` round-trips through `ui.set` and `ui.state`, alone and together; `ui.set`
/// merges `edit` field by field; resetting it gives back the defaults and moves nothing else.
#[test]
fn every_edit_field_round_trips_through_ui_set_and_ui_state() {
    let mut t = setup((1400, 900));
    let photo = t.ids[2];
    let defaults = state(&mut t);
    assert_eq!(
        defaults["edit"],
        json!({ "photo": null, "tab": "basic", "tool": null, "compare": "one", "zoom": "fit", "panelVisible": true,
                "crop": null, "straighten": null, "retouch": null, "tuning": null })
    );
    assert_eq!(defaults["view"], "library");

    let full = json!({
        "photo": photo, "tab": "tuning", "tool": "crop", "compare": "same", "zoom": 2.0, "panelVisible": false,
        "crop": { "ratio": "4:3", "rect": [0.125, 0.25, 0.5, 0.5] },
        "straighten": { "angle": -3.5 },
        "retouch": { "patches": [{ "target": [0.25, 0.5], "source": [0.75, 0.5], "radius": 0.0625 }], "position": 1, "brush": 0.0625 },
        "tuning": { "fill": 0.5, "highlights": 0.25, "shadows": 0.125, "temperature": -0.5, "neutral": [0.5, 0.5, 0.25] }
    });
    let v = ok(&mut t, "ui.set", json!({ "view": "edit", "edit": full }));
    assert_eq!((&v["view"], &v["edit"]), (&json!("edit"), &full));
    assert_eq!(state(&mut t)["edit"], full);
    // One field at a time keeps the others.
    for (field, value) in [
        ("tab", json!("effects3")),
        ("compare", json!("two")),
        ("zoom", json!("fit")),
        ("zoom", json!(0.25)),
        ("panelVisible", json!(true)),
        ("tool", json!("tuning")),
        ("crop", json!({ "ratio": "manual", "rect": null })),
        ("crop", json!({ "ratio": "8.5:11", "rect": [0.0, 0.0, 1.0, 1.0] })),
        ("straighten", json!({ "angle": 10.0 })),
        ("retouch", json!(null)),
        ("tuning", json!({ "fill": 1.0, "highlights": 0.5, "shadows": 0.5, "temperature": 1.0, "neutral": null })),
    ] {
        let before = state(&mut t);
        let v = ok(&mut t, "ui.set", json!({ "edit": { field: value } }));
        let mut want = before["edit"].clone();
        want[field] = value.clone();
        assert_eq!(v["edit"], want, "{field} = {value}");
    }
    // Back to the library and the defaults: nothing else moved.
    ok(&mut t, "ui.set", json!({ "view": "library", "edit": defaults["edit"] }));
    assert_eq!(state(&mut t), defaults);
}

/// Bad values are `bad_params`, a photo that does not exist `not_found`, and nothing changes.
#[test]
fn bad_edit_values_are_refused_and_change_nothing() {
    let mut t = setup((1400, 900));
    let photo = t.ids[0];
    ok(&mut t, "ui.set", json!({ "view": "edit", "edit": { "photo": photo } }));
    let before = state(&mut t);
    for bad in [
        json!({ "view": "edit", "edit": { "photo": null } }),
        json!({ "edit": { "photo": null, "tool": "crop" } }),
        json!({ "edit": { "photo": "one" } }),
        json!({ "edit": { "tab": "effects4" } }),
        json!({ "edit": { "tool": "lasso" } }),
        json!({ "edit": { "compare": "three" } }),
        json!({ "edit": { "zoom": 5 } }),
        json!({ "edit": { "zoom": 0 } }),
        json!({ "edit": { "zoom": -1 } }),
        json!({ "edit": { "zoom": "big" } }),
        json!({ "edit": { "panelVisible": "yes" } }),
        json!({ "edit": { "zzz": 1 } }),
        json!({ "edit": 5 }),
        json!({ "edit": { "crop": { "ratio": "4:0", "rect": null } } }),
        json!({ "edit": { "crop": { "ratio": "wide", "rect": null } } }),
        json!({ "edit": { "crop": { "ratio": "manual", "rect": [0.5, 0.5, 0.75, 0.25] } } }),
        json!({ "edit": { "crop": { "ratio": "manual", "rect": [0.0, 0.0, 0.0, 0.5] } } }),
        json!({ "edit": { "straighten": { "angle": 10.5 } } }),
        json!({ "edit": { "straighten": { "angle": "level" } } }),
        json!({ "edit": { "retouch": { "patches": [], "position": 1, "brush": 0.05 } } }),
        json!({ "edit": { "retouch": { "patches": [], "position": 0, "brush": 0.0 } } }),
        json!({ "edit": { "retouch": { "patches": [{ "target": [1.5, 0.5], "source": [0.5, 0.5], "radius": 0.05 }], "position": 0, "brush": 0.05 } } }),
        json!({ "edit": { "tuning": { "fill": 2.0, "highlights": 0.0, "shadows": 0.0, "temperature": 0.0, "neutral": null } } }),
        json!({ "edit": { "tuning": { "fill": 0.0, "highlights": 0.75, "shadows": 0.0, "temperature": 0.0, "neutral": null } } }),
        json!({ "edit": { "tuning": { "fill": 0.0, "highlights": 0.0, "shadows": 0.0, "temperature": 0.0, "neutral": [2.0, 0.0, 0.0] } } }),
    ] {
        assert_eq!(code(&mut t, "ui.set", bad.clone()), "bad_params", "{bad}");
        assert_eq!(state(&mut t), before, "{bad}: nothing changed");
    }
    assert_eq!(code(&mut t, "ui.set", json!({ "edit": { "photo": 999_999 } })), "not_found");
    assert_eq!(state(&mut t), before);
}

/// Checks the last frame: no unregistered interactive widget, unique ids in the grammar, every
/// rect on screen.
fn check_ids(t: &T, what: &str) {
    let (w, h, _) = t.d.size();
    assert_eq!(t.d.app().panels().unwrap().unregistered_widgets(), [], "{w}×{h} {what}: interactive widgets without an id");
    let mut seen = HashSet::new();
    for wi in t.d.widgets() {
        assert!(registry::is_valid_id(&wi.id), "{w}×{h} {what}: `{}` breaks the id grammar", wi.id);
        assert!(seen.insert(wi.id.clone()), "{w}×{h} {what}: `{}` is registered twice", wi.id);
        let [x, y, rw, rh] = wi.rect;
        assert!(
            rw > 0.0 && rh > 0.0 && x >= 0.0 && y >= 0.0 && x + rw <= w as f32 + 0.01 && y + rh <= h as f32 + 0.01,
            "{w}×{h} {what}: `{}` at {:?}",
            wi.id,
            wi.rect
        );
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the whole window against `tests/fingerprints/<name>.json` (`DALICA_BLESS=1` writes it),
/// and writes it to the test output (and to `$DALICA_SHOTS`).
fn check_window(t: &T, name: &str) {
    let s = t.d.screenshot().unwrap();
    let png = fingerprint::png_rgba(s.width, s.height, &s.rgba);
    std::fs::write(out_dir().join(format!("{name}.png")), &png).unwrap();
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
    }
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(s.width, s.height, &s.rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(s.width, s.height, &s.rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// The edit view's regions at a window `w` × `h` with the panel shown and no side pane
/// (`editing.md` §2): menu bar 24, top bar 40 with the filmstrip between its groups (240 from the
/// left, 220 from the right), the edit panel 280, the photo area with the caption bar (36) under
/// it, the library's status line (20) and bottom area (96).
fn edit_regions(w: f32, h: f32) -> Value {
    let mid = h - 72.0 - 116.0;
    json!({
        "menuBar": [0.0, 0.0, w, 32.0],
        "editTopBar": [0.0, 32.0, w, 40.0],
        "filmstrip": [240.0, 32.0, w - 460.0, 40.0],
        "editPanel": [0.0, 72.0, 280.0, mid],
        "canvas": [280.0, 72.0, w - 280.0, mid - 36.0],
        "captionBar": [280.0, 72.0 + mid - 36.0, w - 280.0, 36.0],
        "statusLine": [0.0, h - 116.0, w, 20.0],
        "toolbar": null, "tree": null, "lightTable": null, "scrollbar": null, "splitter": null, "sidePane": null,
    })
}

/// Ctrl+3 on a selected photo shows every edit-view region at the rects of `editing.md` §2 at
/// both window sizes, with valid ids and the frame's fingerprint; Esc returns to the library with
/// its selection; Enter opens the edit view too and Ctrl+1 leaves it.
#[test]
fn ctrl3_shows_the_edit_view_at_both_sizes_and_esc_returns() {
    for (w, h) in SIZES {
        let mut t = setup((w, h));
        let photo = t.ids[1];
        select(&mut t, photo);
        key(&mut t, "Ctrl+3");
        let s = state(&mut t);
        assert_eq!((&s["view"], &s["edit"]["photo"]), (&json!("edit"), &json!(photo)), "{w}×{h}");
        let r = regions(&mut t);
        for (name, want) in edit_regions(w as f32, h as f32).as_object().unwrap() {
            assert_eq!(&r[name], want, "{w}×{h}: region {name}");
        }
        for name in ["tray", "trayButtons", "bottomControls", "actionButtons"] {
            assert!(r[name].is_array(), "{w}×{h}: the bottom area keeps {name}");
        }
        for id in
            ["button:edit.back", "canvas:edit", "region:editTopBar", "region:editPanel", "region:canvas", "region:filmstrip", "region:captionBar"]
        {
            assert!(t.d.widgets().iter().any(|wi| wi.id == id), "{w}×{h}: {id}");
        }
        check_ids(&t, "edit view");
        check_window(&t, &format!("edit-frame-{w}x{h}"));

        key(&mut t, "Esc");
        let s = state(&mut t);
        assert_eq!((&s["view"], &s["edit"]["photo"], &s["selection"]["ids"]), (&json!("library"), &Value::Null, &json!([photo])), "{w}×{h}");
        let r = regions(&mut t);
        assert!(r["lightTable"].is_array() && r["editPanel"].is_null() && r["canvas"].is_null(), "{w}×{h}: {r}");

        key(&mut t, "Enter");
        assert_eq!(state(&mut t)["view"], "edit", "{w}×{h}: Enter opens the edit view");
        key(&mut t, "Ctrl+1");
        let s = state(&mut t);
        assert_eq!(s["view"], "library", "{w}×{h}: Ctrl+1 in the edit view is Library View");
        assert_eq!(s["thumbSize"], 144.0, "{w}×{h}: not Small Thumbnails");
        // Back to Library by its button.
        key(&mut t, "Ctrl+3");
        t.d.click("button:edit.back").unwrap();
        t.d.settle(FRAMES).unwrap();
        assert_eq!(state(&mut t)["view"], "library", "{w}×{h}");
    }
}

/// Ctrl+9 hides and shows the edit panel; the photo area takes its width; View ▸ Show Edit
/// Controls is checked while it shows.
#[test]
fn ctrl9_hides_and_shows_the_edit_panel() {
    let mut t = setup((1400, 900));
    let photo = t.ids[0];
    run(&mut t, "view.edit", json!({ "id": photo }));
    t.d.settle(FRAMES).unwrap();
    let checked = |t: &mut T| {
        let tree = ok(t, "ui.menu.tree", json!({}));
        let view = tree.as_array().unwrap().iter().find(|m| m["widget"] == "menu:View").cloned().unwrap();
        let item = view["children"].as_array().unwrap().iter().find(|i| i["id"] == "edit.panelToggle").cloned().unwrap();
        (item["enabled"].clone(), item["checked"].clone(), item["shortcut"].clone())
    };
    // The menus write the primary modifier as macOS names it.
    let ctrl9 = if cfg!(target_os = "macos") { "Cmd+9" } else { "Ctrl+9" };
    assert_eq!(checked(&mut t), (json!(true), json!(true), json!(ctrl9)));
    key(&mut t, "Ctrl+9");
    let r = regions(&mut t);
    assert_eq!((&r["editPanel"], &r["canvas"]), (&Value::Null, &json!([0.0, 72.0, 1400.0, 676.0])));
    assert_eq!(state(&mut t)["edit"]["panelVisible"], false);
    assert_eq!(checked(&mut t).1, json!(false));
    check_ids(&t, "panel hidden");
    key(&mut t, "Ctrl+9");
    assert_eq!(regions(&mut t)["editPanel"], json!([0.0, 72.0, 280.0, 712.0]));
    assert_eq!(state(&mut t)["edit"]["panelVisible"], true);
}

/// Ctrl+3 (and Enter) with nothing selected stays in the library and shows the toast.
#[test]
fn ctrl3_with_nothing_selected_gives_the_toast() {
    let mut t = setup((900, 600));
    assert_eq!(state(&mut t)["selection"]["ids"], json!([]));
    key(&mut t, "Ctrl+3");
    assert_eq!(state(&mut t)["view"], "library");
    let toasts = ok(&mut t, "ui.inspect", json!({}))["toasts"].clone();
    assert!(toasts.as_array().unwrap().iter().any(|x| x["text"] == "Select a photo to edit."), "{toasts}");
    assert_eq!(code(&mut t, "engine.execute", json!({ "command": "view.edit", "params": {} })), "disabled");
}

/// An open tool takes Esc first (it is cancelled, its draft dropped); the next Esc leaves.
/// Leaving with a tool open cancels it too.
#[test]
fn an_open_tool_takes_esc_first() {
    let mut t = setup((1400, 900));
    let photo = t.ids[3];
    run(&mut t, "view.edit", json!({ "id": photo }));
    run(&mut t, "edit.tool.open", json!({ "tool": "crop" }));
    t.d.settle(FRAMES).unwrap();
    assert_eq!(state(&mut t)["edit"]["crop"], json!({ "ratio": "manual", "rect": null }));
    check_ids(&t, "crop open");
    key(&mut t, "Esc");
    let s = state(&mut t);
    assert_eq!((&s["view"], &s["edit"]["tool"], &s["edit"]["crop"]), (&json!("edit"), &Value::Null, &Value::Null));
    key(&mut t, "Esc");
    assert_eq!(state(&mut t)["view"], "library");

    run(&mut t, "view.edit", json!({ "id": photo }));
    run(&mut t, "edit.tool.open", json!({ "tool": "retouch" }));
    run(&mut t, "view.library", json!({}));
    let s = state(&mut t);
    assert_eq!((&s["edit"]["tool"], &s["edit"]["retouch"]), (&Value::Null, &Value::Null));
}

/// The new commands refuse bad parameters through the protocol and change nothing.
#[test]
fn the_new_commands_fail_gracefully() {
    let mut t = setup((1400, 900));
    let before = state(&mut t);
    for (id, params, want) in [
        ("view.edit", json!({ "id": 999_999 }), "not_found"),
        ("view.edit", json!({ "id": [] }), "bad_params"),
        ("view.library", json!({}), "disabled"),
        ("view.library", json!({ "now": true }), "bad_params"),
        ("edit.panelToggle", json!({ "on": 1 }), "bad_params"),
        ("edit.tab", json!({ "tab": null }), "bad_params"),
        ("edit.tool.open", json!({ "tool": "crop" }), "disabled"),
        ("edit.tool.open", json!({ "tool": "wand" }), "bad_params"),
        ("edit.tool.apply", json!({}), "disabled"),
        ("edit.tool.cancel", json!({}), "disabled"),
        ("edit.tool.reset", json!({}), "disabled"),
        ("edit.tool.action", json!({ "name": "drag" }), "disabled"),
        ("edit.tool.action", json!({ "params": {} }), "bad_params"),
    ] {
        assert_eq!(code(&mut t, "engine.execute", json!({ "command": id, "params": params })), want, "{id} {params}");
        assert_eq!(state(&mut t), before, "{id} {params}");
    }
}
