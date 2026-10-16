//! M10's *Preferences and shortcuts* in the live app (`m10.rs` part 1): `dalica --headless` on a
//! fresh library, driven over its control channel, across three sessions:
//!
//! - every option the build can change set to another value with one `options.set`, read back by
//!   `options.list`, then again after a restart, while the keys `settings.json` held that Dalica
//!   does not know stay in the file verbatim, and back to every default at the end;
//! - Star rebound to F8 through `app.setShortcut`: F8 pressed headless stars the selected photo and
//!   Ctrl+8 no longer does; Ctrl+1 for Star in the Library refused with the editor's sentence
//!   naming Small Thumbnails, then taken with `replace`, and Ctrl+1 stars; both keys kept across a
//!   restart;
//! - Reset All in the shortcut editor (Help ▸ Keyboard Shortcuts…, Ctrl+/): its question, OK, and
//!   every row of `keymap.list` back to its default keys, Ctrl+8 starring again.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Map, Value, json};

use crate::app::{self, click, dialog, key, run, secs};
use crate::live::{self, Client};

const STAR: &str = "photo.star.toggle";
const SMALL: &str = "view.thumbsSmall";
/// The keys `settings.json` holds that no Dalica knows, as another version might write them.
const UNKNOWN: [(&str, &str); 2] = [("/future", r#"{"x":1,"list":[true,null]}"#), ("/interface/later", "2")];

/// `home/Pictures/Harbour` with three photos large enough for any small-picture threshold.
fn harbour(home: &Path) -> PathBuf {
    let folder = home.join("Pictures").join("Harbour");
    for (i, name) in ["a.jpg", "b.jpg", "c.jpg"].iter().enumerate() {
        let jpeg = app::jpeg(300 + i as u64, (1200, 900), "Dalica", "Scenes 1", &format!("2024-05-0{}T09:00:00", i + 1));
        app::put(&folder.join(name), &jpeg, i as u64);
    }
    folder
}

fn settings_file(home: &Path) -> PathBuf {
    home.join("library").join("settings.json")
}

fn settings(home: &Path) -> Value {
    let bytes = std::fs::read(settings_file(home)).unwrap_or_else(|e| panic!("settings.json: {e}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("settings.json is JSON: {e}"))
}

/// The unknown keys are in `settings.json` exactly as written.
fn unknown_kept(home: &Path, when: &str) {
    let s = settings(home);
    for (pointer, text) in UNKNOWN {
        let want: Value = serde_json::from_str(text).unwrap();
        assert_eq!(s.pointer(pointer), Some(&want), "{when}: `{pointer}` kept in settings.json: {s}");
    }
}

/// `options.list`'s rows, by key, in the dialog's order.
fn rows(c: &mut Client) -> Vec<Value> {
    let r = run(c, "options.list", json!({}));
    r["options"].as_array().cloned().unwrap_or_else(|| panic!("options.list: {r}"))
}

fn values(c: &mut Client) -> Map<String, Value> {
    rows(c).into_iter().map(|r| (r["key"].as_str().unwrap().to_string(), r["value"].clone())).collect()
}

/// A value of row `r` other than its default (a choice of one is left out), as the dialog would
/// set it.
fn other(r: &Value, folder: &Path) -> Option<Value> {
    let default = &r["default"];
    match r["type"].as_str().unwrap_or_default() {
        "bool" => Some(json!(default != &json!(true))),
        "int" => {
            let (min, max) = (r["min"].as_u64().unwrap(), r["max"].as_u64().unwrap());
            Some(json!(if default == &json!(min) { max } else { min }))
        }
        "scale" => Some(json!(150)),
        "choice" => r["choices"].as_array().unwrap().iter().map(|c| c["id"].clone()).rfind(|id| id != default),
        "text" => Some(json!("© Ana Lima")),
        "folder" => Some(json!(folder)),
        other => panic!("options.list: an unknown type {other}: {r}"),
    }
}

/// `keymap.list`'s rows for `command` in `context`, their keys in effect.
fn keys(c: &mut Client, command: &str, context: &str) -> Vec<String> {
    let r = run(c, "keymap.list", json!({ "withoutKeys": true }));
    let row = r["rows"].as_array().unwrap().iter().find(|w| w["command"] == command && w["context"] == context).cloned();
    row.map(|w| w["keys"].as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

fn starred(c: &mut Client, id: &Value) -> bool {
    c.inspect(id)["star"] == true
}

/// Selects photo `id` alone and presses `combo` in the window; whether the photo is starred after.
fn press_on(c: &mut Client, id: &Value, combo: &str) -> bool {
    run(c, "select.set", json!({ "ids": [id] }));
    key(c, combo);
    starred(c, id)
}

fn start(exe: &Path, home: &Path) -> (live::App, Client) {
    let (a, mut c) = live::start(exe, home);
    app::idle(&mut c);
    (a, c)
}

pub fn preferences(exe: &Path, home: &Path) {
    let started = Instant::now();
    let folder = harbour(home);
    let out = home.join("Exports");
    std::fs::create_dir_all(&out).unwrap();

    // Session 1: the library and its folder; then keys of a later version in settings.json.
    let (a, mut cl) = start(exe, home);
    run(&mut cl, "folders.add", json!({ "path": folder }));
    live::quit(a, cl);
    let mut doc: Value = std::fs::read(settings_file(home)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    doc["future"] = serde_json::from_str(UNKNOWN[0].1).unwrap();
    if !doc["interface"].is_object() {
        doc["interface"] = json!({});
    }
    doc["interface"]["later"] = json!(2);
    std::fs::write(settings_file(home), serde_json::to_vec_pretty(&doc).unwrap()).unwrap();

    // Session 2.
    let (a, mut cl) = start(exe, home);
    let c = &mut cl;
    let photos = app::photos(c, &home.join("Pictures"));
    let id = photos["Harbour/a.jpg"].id.clone();
    assert!(!starred(c, &id));

    // Star rebound to F8: F8 stars the selected photo, Ctrl+8 no longer does anything.
    let star_defaults = keys(c, STAR, "library");
    assert!(star_defaults.contains(&"Ctrl+8".to_string()), "Star's defaults in the Library: {star_defaults:?}");
    run(c, "app.setShortcut", json!({ "command": STAR, "context": "library", "key": "F8" }));
    assert_eq!(keys(c, STAR, "library"), ["F8"]);
    assert!(press_on(c, &id, "F8"), "F8 stars the selected photo");
    assert!(press_on(c, &id, "Ctrl+8"), "Ctrl+8 no longer toggles Star");
    assert!(!press_on(c, &id, "F8"), "F8 again takes the star off");
    println!("  Star rebound to F8 through app.setShortcut (it had {star_defaults:?}): F8 stars, Ctrl+8 does not");

    // A conflict: Ctrl+1 is Small Thumbnails in the Library; refused, then taken with replace.
    let refused = c.refused("app.setShortcut", json!({ "command": STAR, "context": "library", "key": "Ctrl+1", "slot": 1 }));
    let why = refused["error"].as_str().unwrap();
    // The engine names a window command (`UI_COMMANDS`) by its id, its own commands by their label
    // (`crates/engine/tests/keymap.rs`); the editor's sentence uses the label.
    let named = why.contains("Small Thumbnails") || why.contains(SMALL);
    assert!(why.contains("Ctrl+1 is already") && named && why.contains("in Library."), "the conflict names the other binding: {refused}");
    assert_eq!(
        (keys(c, STAR, "library"), keys(c, SMALL, "library")),
        (vec!["F8".to_string()], vec!["Ctrl+1".to_string()]),
        "a refusal changes nothing"
    );
    let took = run(c, "app.setShortcut", json!({ "command": STAR, "context": "library", "key": "Ctrl+1", "slot": 1, "replace": true }));
    assert!(took.to_string().contains(SMALL), "replace says whom it took the key from: {took}");
    assert_eq!(keys(c, STAR, "library"), ["F8", "Ctrl+1"]);
    assert!(keys(c, SMALL, "library").is_empty(), "Small Thumbnails lost Ctrl+1");
    assert!(keys(c, "view.library", "editView").contains(&"Ctrl+1".to_string()), "the edit view's Ctrl+1 is no conflict and stays");
    assert!(press_on(c, &id, "Ctrl+1"), "Ctrl+1 now stars");
    assert!(!press_on(c, &id, "F8"), "and F8 still does");
    println!("  Ctrl+1 for Star: refused naming Small Thumbnails in Library ({why}), then taken with replace");

    // Every option set to another value at once, read back.
    let defaults: Map<String, Value> = rows(c).into_iter().map(|r| (r["key"].as_str().unwrap().to_string(), r["default"].clone())).collect();
    let mut set = Map::new();
    let mut unavailable = Vec::new();
    for r in rows(c) {
        let k = r["key"].as_str().unwrap().to_string();
        if r["available"] != true {
            unavailable.push(k);
            continue;
        }
        if let Some(v) = other(&r, &out) {
            set.insert(k, v);
        }
    }
    let changed = run(c, "options.set", json!({ "values": set }));
    let mut listed: Vec<&str> = changed["changed"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
    listed.sort_unstable();
    let mut want: Vec<&str> = set.keys().map(String::as_str).collect();
    want.sort_unstable();
    assert_eq!(listed, want, "every option changed: {changed}");
    assert_eq!(changed["rescan"], true, "File Types changed: one rescan");
    app::idle(c);
    let now = values(c);
    for (k, v) in &set {
        assert_eq!(&now[k], v, "options.list reads back {k}");
    }
    unknown_kept(home, "after options.set");
    println!("  {} options set at once and read back ({} unavailable in this build: {unavailable:?})", set.len(), unavailable.len());
    live::quit(a, cl);
    unknown_kept(home, "after the quit");

    // Session 3: the options and both keys survived the restart.
    let (a, mut cl) = start(exe, home);
    let c = &mut cl;
    let now = values(c);
    for (k, v) in &set {
        assert_eq!(&now[k], v, "{k} after a restart");
    }
    assert_eq!(keys(c, STAR, "library"), ["F8", "Ctrl+1"], "the rebinding survived the restart");
    assert!(press_on(c, &id, "F8"), "F8 stars after the restart");
    println!("  the options and the keys survived a restart, with the unknown keys kept");

    // Reset All in the shortcut editor restores every default.
    key(c, "Ctrl+/");
    assert_eq!(dialog(c).as_deref(), Some("options"), "Ctrl+/ opens Options");
    assert_eq!(c.ok("ui.state", json!({}))["options"]["tab"], "shortcuts", "on its Keyboard Shortcuts tab");
    click(c, "button:shortcuts.resetAll");
    assert_eq!(app::label(c, "text:shortcuts.resetAll"), "Restore all default shortcuts?");
    click(c, "button:shortcuts.resetAll/confirm");
    click(c, "button:dialog.ok");
    assert_eq!(dialog(c), None, "OK applies and closes");
    let r = run(c, "keymap.list", json!({ "withoutKeys": true }));
    let all = r["rows"].as_array().unwrap();
    let modified: Vec<&Value> = all.iter().filter(|w| w["modified"] != false || w["keys"] != w["defaults"]).collect();
    assert!(modified.is_empty(), "Reset All restores every default: {modified:?}");
    assert_eq!((keys(c, STAR, "library"), keys(c, SMALL, "library")), (star_defaults, vec!["Ctrl+1".to_string()]));
    assert!(!press_on(c, &id, "Ctrl+8"), "Ctrl+8 is Star again (it takes the star off)");
    assert!(!press_on(c, &id, "F8"), "F8 does nothing");
    println!("  Reset All: {} keymap rows, every one at its default", all.len());

    // Every option back to its default.
    let back: Map<String, Value> = set.keys().map(|k| (k.clone(), defaults[k].clone())).collect();
    run(c, "options.set", json!({ "values": back }));
    app::idle(c);
    let now = values(c);
    for k in set.keys() {
        assert_eq!(now[k], defaults[k], "{k} back to its default");
    }
    live::quit(a, cl);
    unknown_kept(home, "at the end");
    let keymap = settings(home)["keymap"].clone();
    assert!(keymap.is_null() || keymap == json!([]), "no shortcut override is left: {keymap}");
    println!("  every option back to its default; M10 preferences in {:.1} s", secs(started));
}
