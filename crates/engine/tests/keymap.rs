//! The keymap and its settings (`menus-and-shortcuts.md` §2.0): key parsing, contexts, conflicts,
//! resolution, rebinding through `app.setShortcut`, and a rebinding surviving a restart.

use std::sync::Arc;

use dalica_core::SystemClock;
use dalica_engine::keymap::{DefaultKeys, Platform, SetError, first_default, parse_default};
use dalica_engine::{Binding, CmdError, Context, Key, Keymap, Session};
use serde_json::json;

fn key(k: &str) -> Key {
    Key::parse(k).unwrap()
}

#[test]
fn keys_parse_to_one_canonical_form() {
    for (text, canonical) in [
        ("Ctrl+8", "Ctrl+8"),
        ("cmd+shift+z", "Ctrl+Shift+Z"),
        ("Shift+Ctrl+Z", "Ctrl+Shift+Z"),
        ("Option+Enter", "Alt+Enter"),
        ("alt+return", "Alt+Enter"),
        ("f4", "F4"),
        ("F24", "F24"),
        ("Ctrl+,", "Ctrl+,"),
        ("Ctrl+/", "Ctrl+/"),
        ("Ctrl++", "Ctrl+Plus"),
        ("escape", "Esc"),
        ("PgDn", "PageDown"),
    ] {
        assert_eq!(key(text).to_string(), canonical, "{text}");
    }
    for bad in ["", "Ctrl+", "Hyper+A", "Ctrl+Ctrl+A", "F25", "F0", "AB", "Ctrl+ä", "+", &"Ctrl+".repeat(20)] {
        assert!(Key::parse(bad).is_err(), "{bad:?}");
    }
}

/// Ctrl+1 is Small Thumbnails in Library and Back to Library in Edit view: no conflict. The same
/// key in Global and Library is one.
#[test]
fn conflicts_depend_on_contexts_that_can_be_active_together() {
    let map = Keymap::new([("view.thumbsSmall", Context::Library, "Ctrl+1"), ("view.library", Context::EditView, "Ctrl+1")]).unwrap();
    assert!(map.conflicts().is_empty());
    let map = Keymap::new([("view.thumbsSmall", Context::Library, "Ctrl+1"), ("app.something", Context::Global, "Ctrl+1")]).unwrap();
    assert_eq!(map.conflicts().len(), 1);
    let map = Keymap::new([("a", Context::Library, "F5"), ("b", Context::Library, "F5")]).unwrap();
    assert_eq!(map.conflicts().len(), 1);
}

#[test]
fn no_two_default_bindings_conflict_and_every_default_parses() {
    let map = Keymap::from_specs(&dalica_engine::cmd::specs()).unwrap();
    assert_eq!(map.conflicts(), []);
    assert_eq!(map.keys_for("photo.star.toggle", Context::Library), [key("Ctrl+8"), key("F4")]);
}

/// The defaults are checked for every platform, not only the one the tests run on.
#[test]
fn no_default_conflicts_on_any_platform_and_redo_differs_on_macos() {
    let specs = dalica_engine::cmd::specs();
    for p in Platform::ALL {
        let map = Keymap::from_specs_on(&specs, p).unwrap();
        assert_eq!(map.conflicts(), [], "{p:?}");
        // Every command is known, with or without keys; every spec's defaults parse here.
        assert!(specs.iter().all(|s| map.knows(s.id)), "{p:?}");
        let redo = if p == Platform::Mac { "Ctrl+Shift+Z" } else { "Ctrl+Y" };
        assert_eq!(map.keys_for("edit.redo", Context::Library), [key(redo)], "{p:?}");
        assert_eq!(map.keys_for("edit.undo", Context::Library), [key("Ctrl+Z")], "{p:?}");
        assert_eq!(map.resolve(&key(redo), Context::Library, false).as_deref(), Some("edit.redo"));
    }
    assert_eq!(
        Keymap::from_specs(&specs).unwrap().keys_for("edit.redo", Context::Library),
        Keymap::from_specs_on(&specs, Platform::current()).unwrap().keys_for("edit.redo", Context::Library)
    );
}

#[test]
fn platform_qualifiers_pick_a_default_and_refuse_unknown_platforms() {
    for (text, platform, applies) in [
        ("Ctrl+8", Platform::Mac, true),
        ("mac:Ctrl+Shift+Z", Platform::Mac, true),
        ("mac:Ctrl+Shift+Z", Platform::Linux, false),
        ("win,linux:Ctrl+Y", Platform::Windows, true),
        ("win, linux:Ctrl+Y", Platform::Linux, true),
        ("win,linux:Ctrl+Y", Platform::Mac, false),
        ("linux:Ctrl+,", Platform::Linux, true),
    ] {
        let (k, a) = parse_default(text, platform).unwrap();
        assert_eq!(a, applies, "{text} {platform:?}");
        assert_eq!(k.to_string(), text.rsplit(':').next().unwrap(), "{text}");
    }
    for bad in ["bsd:F1", "mac:", ":F1", "mac;win:F1", "mac:Hyper+A", "Mac:F1"] {
        assert!(parse_default(bad, Platform::Mac).is_err(), "{bad:?}");
    }
    // A user's key is never qualified.
    assert!(Key::parse("mac:Ctrl+Y").is_err());
    // A keymap whose default does not parse is refused.
    assert!(Keymap::new_on([("a", Context::Library, "plan9:F1")], Platform::Linux).is_err());
    let keys: DefaultKeys = &[(Context::Library, "mac:Ctrl+Shift+Z"), (Context::Library, "win,linux:Ctrl+Y")];
    assert_eq!(first_default(keys, Platform::Mac), Some("Ctrl+Shift+Z"));
    assert_eq!(first_default(keys, Platform::Windows), Some("Ctrl+Y"));
    assert_eq!(first_default(&[(Context::Library, "mac:F1")], Platform::Linux), None);
    assert_eq!(first_default(&[], Platform::Linux), None);
}

/// The UI's commands join the keymap: known with or without keys, conflict-checked, added twice
/// without change; a default that does not parse changes nothing.
#[test]
fn added_commands_join_the_keymap_and_its_conflict_check() {
    let mut map = Keymap::from_specs_on(&dalica_engine::cmd::specs(), Platform::Linux).unwrap();
    let ui: [(&str, DefaultKeys); 3] =
        [("view.thumbsSmall", &[(Context::Library, "Ctrl+1")]), ("view.library", &[(Context::EditView, "Ctrl+1")]), ("select.set", &[])];
    map.set("photo.star.toggle", Context::Library, None, Some(key("F9")), false).unwrap();
    map.add_commands(ui, Platform::Linux).unwrap();
    map.add_commands(ui, Platform::Linux).unwrap();
    assert!(map.knows("select.set") && map.knows("view.library") && !map.knows("no.such"));
    assert_eq!(map.keys_for("view.thumbsSmall", Context::Library), [key("Ctrl+1")], "added once");
    assert_eq!(map.keys_for("photo.star.toggle", Context::Library), [key("F9")], "overrides kept");
    assert_eq!(map.conflicts(), [], "Ctrl+1 in two view contexts is no conflict");
    assert_eq!(map.resolve(&key("Ctrl+1"), Context::EditView, false).as_deref(), Some("view.library"));
    // A real conflict is found and a rebinding onto it refused.
    map.add_commands([("window.fullscreen", &[(Context::Global, "Ctrl+1")] as DefaultKeys)], Platform::Linux).unwrap();
    assert_eq!(map.conflicts().len(), 2);
    map.add_commands([("window.fullscreen", &[(Context::Global, "F11")] as DefaultKeys)], Platform::Linux).unwrap();
    assert_eq!(map.conflicts(), []);
    assert!(map.set("select.set", Context::Library, None, Some(key("F11")), false).is_err());
    // Nothing changes when a default does not parse.
    let before = map.bindings();
    assert!(map.add_commands([("x.y", &[(Context::Library, "Hyper+X")] as DefaultKeys)], Platform::Linux).is_err());
    assert_eq!(map.bindings(), before);
    assert!(!map.knows("x.y"));
}

#[test]
fn a_key_resolves_text_input_then_view_then_global() {
    let map = Keymap::new([
        ("global.thing", Context::Global, "Ctrl+A"),
        ("library.thing", Context::Library, "Ctrl+A"),
        ("text.selectAll", Context::TextInput, "Ctrl+A"),
        ("global.only", Context::Global, "F11"),
    ])
    .unwrap();
    assert_eq!(map.resolve(&key("Ctrl+A"), Context::Library, true).as_deref(), Some("text.selectAll"));
    assert_eq!(map.resolve(&key("Ctrl+A"), Context::Library, false).as_deref(), Some("library.thing"));
    assert_eq!(map.resolve(&key("Ctrl+A"), Context::EditView, false).as_deref(), Some("global.thing"));
    assert_eq!(map.resolve(&key("F11"), Context::EditView, true).as_deref(), Some("global.only"));
    assert_eq!(map.resolve(&key("F12"), Context::Library, false), None);
}

#[test]
fn rebinding_unbinding_and_replacing() {
    let mut map =
        Keymap::new([("star", Context::Library, "Ctrl+8"), ("star", Context::Library, "F4"), ("small", Context::Library, "Ctrl+1")]).unwrap();
    // A clash is refused with the editor's message, unless replace.
    let clash = map.set("star", Context::Library, None, Some(key("Ctrl+1")), false).unwrap_err();
    assert_eq!(clash, SetError::Conflict(Binding { command: "small".into(), context: Context::Library, key: key("Ctrl+1") }));
    assert_eq!(clash.to_string(), "Ctrl+1 is already small in Library.");
    map.set("star", Context::Library, None, Some(key("Ctrl+1")), true).unwrap();
    assert_eq!(map.keys_for("star", Context::Library), [key("Ctrl+1")]);
    assert!(map.keys_for("small", Context::Library).is_empty());
    assert!(map.conflicts().is_empty());
    // Removing a binding, then resetting one command, then all.
    map.set("star", Context::Library, None, None, false).unwrap();
    assert!(map.keys_for("star", Context::Library).is_empty());
    map.reset(Some("star"));
    assert_eq!(map.keys_for("star", Context::Library), [key("Ctrl+8"), key("F4")]);
    map.reset(None);
    assert_eq!(map.keys_for("small", Context::Library), [key("Ctrl+1")]);
}

fn session(dir: &std::path::Path) -> Session {
    Session::open(dir, Arc::new(SystemClock)).unwrap()
}

#[test]
fn a_rebinding_survives_a_restart_and_unknown_settings_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.json"),
        br#"{"futureOption": {"x": 1}, "keymap": [{"command": "future.command", "context": "library", "key": "F9"}, {"command": "photo.star.toggle", "context": "nowhere", "key": "F9"}]}"#,
    )
    .unwrap();
    let mut s = session(dir.path());
    let r = s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "library", "key": "ctrl+shift+8" })).unwrap();
    assert_eq!(r["keys"], json!(["Ctrl+Shift+8"]));
    let r = s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "library", "key": "F8", "slot": 1 })).unwrap();
    assert_eq!(r["keys"], json!(["Ctrl+Shift+8", "F8"]));
    s.execute("app.setShortcut", &json!({ "command": "edit.undo", "context": "library", "key": null })).unwrap();
    s.flush().unwrap();
    drop(s);
    let s = session(dir.path());
    assert_eq!(s.keymap().keys_for("photo.star.toggle", Context::Library), [key("Ctrl+Shift+8"), key("F8")], "both slots kept");
    assert_eq!(s.keymap().resolve(&key("Ctrl+Shift+8"), Context::Library, false).as_deref(), Some("photo.star.toggle"));
    assert_eq!(s.keymap().resolve(&key("F8"), Context::Library, false).as_deref(), Some("photo.star.toggle"));
    assert_eq!(s.keymap().resolve(&key("Ctrl+8"), Context::Library, false), None);
    assert_eq!(s.keymap().resolve(&key("Ctrl+Z"), Context::Library, false), None);
    // Keys and entries this version does not know were kept as they were.
    let text = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
    assert!(text.contains("\"futureOption\"") && text.contains("future.command") && text.contains("nowhere"), "{text}");
    drop(s);
    // Reset restores the defaults, also after a restart.
    let mut s = session(dir.path());
    s.execute("app.resetShortcuts", &json!({})).unwrap();
    s.flush().unwrap();
    drop(s);
    let s = session(dir.path());
    assert_eq!(s.keymap().resolve(&key("Ctrl+8"), Context::Library, false).as_deref(), Some("photo.star.toggle"));
    assert!(std::fs::read_to_string(dir.path().join("settings.json")).unwrap().contains("future.command"));
}

#[test]
fn a_damaged_settings_file_is_kept_aside_and_defaults_are_used() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("settings.json"), b"{ not json").unwrap();
    let s = session(dir.path());
    assert_eq!(s.notices().len(), 1, "{:?}", s.notices());
    assert!(s.notices()[0].contains("defaults used"));
    assert_eq!(s.keymap().resolve(&key("Ctrl+8"), Context::Library, false).as_deref(), Some("photo.star.toggle"));
    let names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.starts_with("settings.json.corrupt-")), "{names:?}");
}

#[test]
fn shortcut_commands_refuse_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    let cases = [
        json!({ "command": "no.such", "context": "library", "key": "F9" }),
        json!({ "command": "photo.star.toggle", "context": "kitchen", "key": "F9" }),
        json!({ "command": "photo.star.toggle", "context": "library", "key": "Hyper+F9" }),
        json!({ "command": "photo.star.toggle", "context": "library" }),
        json!({ "command": "photo.star.toggle", "context": "library", "key": 9 }),
        json!({ "command": "photo.star.toggle", "context": "library", "key": "F9", "extra": true }),
        json!({ "command": "photo.star.toggle", "context": "library", "key": "F9", "slot": 2 }),
        json!({ "command": "photo.star.toggle", "context": "library", "key": "F9", "slot": -1 }),
        json!({ "command": "photo.star.toggle", "context": "library", "key": "F9", "slot": "1" }),
        json!([1]),
    ];
    for p in cases {
        let r = s.execute("app.setShortcut", &p);
        assert!(matches!(r, Err(CmdError::BadParams(_) | CmdError::NotFound(_))), "{p}: {r:?}");
    }
    // A clash with another command in the same context.
    let r = s.execute("app.setShortcut", &json!({ "command": "edit.redo", "context": "library", "key": "Ctrl+Z" }));
    assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("Ctrl+Z is already Undo in Library.")), "{r:?}");
    assert!(matches!(s.execute("app.resetShortcuts", &json!({ "command": "nope" })), Err(CmdError::NotFound(_))));
    assert!(matches!(s.execute("app.resetShortcuts", &json!({ "all": true })), Err(CmdError::BadParams(_))));
    assert_eq!(s.keymap().overrides(), []);
}

/// The UI's commands, as the app adds them (`Session::add_ui_keys`).
const UI: [(&str, DefaultKeys); 3] =
    [("view.thumbsSmall", &[(Context::Library, "Ctrl+1")]), ("window.fullscreen", &[(Context::Global, "F11")]), ("select.set", &[])];

/// A UI command is rebound through `app.setShortcut`, stored once in `settings.json`, and the
/// binding applies again once the next session has added the UI commands.
#[test]
fn ui_commands_are_rebound_through_set_shortcut_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    // Before the app adds them, the engine does not know UI commands.
    let r = s.execute("app.setShortcut", &json!({ "command": "view.thumbsSmall", "context": "library", "key": "F9" }));
    assert!(matches!(r, Err(CmdError::NotFound(_))), "{r:?}");
    s.add_ui_keys(&UI).unwrap();
    s.add_ui_keys(&UI).unwrap();
    assert_eq!(s.keymap().resolve(&key("F11"), Context::EditView, false).as_deref(), Some("window.fullscreen"));
    let r = s.execute("app.setShortcut", &json!({ "command": "view.thumbsSmall", "context": "library", "key": "Ctrl+Shift+1" })).unwrap();
    assert_eq!(r["keys"], json!(["Ctrl+Shift+1"]));
    s.execute("app.setShortcut", &json!({ "command": "select.set", "context": "library", "key": "F9" })).unwrap();
    s.flush().unwrap();
    drop(s);
    let text = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
    assert_eq!(text.matches("view.thumbsSmall").count(), 1, "{text}");
    let mut s = session(dir.path());
    assert_eq!(s.keymap().resolve(&key("Ctrl+Shift+1"), Context::Library, false), None, "not until the UI commands are added");
    s.add_ui_keys(&UI).unwrap();
    assert_eq!(s.keymap().resolve(&key("Ctrl+Shift+1"), Context::Library, false).as_deref(), Some("view.thumbsSmall"));
    assert_eq!(s.keymap().resolve(&key("Ctrl+1"), Context::Library, false), None);
    assert_eq!(s.keymap().resolve(&key("F9"), Context::Library, false).as_deref(), Some("select.set"));
    s.execute("app.resetShortcuts", &json!({ "command": "view.thumbsSmall" })).unwrap();
    assert_eq!(s.keymap().resolve(&key("Ctrl+1"), Context::Library, false).as_deref(), Some("view.thumbsSmall"));
    s.flush().unwrap();
    drop(s);
    let text = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
    assert!(!text.contains("view.thumbsSmall") && text.contains("select.set"), "{text}");
}

#[test]
fn shortcut_commands_refuse_bad_input_for_ui_commands() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    s.add_ui_keys(&UI).unwrap();
    for p in [
        json!({ "command": "view.thumbsHuge", "context": "library", "key": "F9" }),
        json!({ "command": "view.thumbsSmall", "context": "library", "key": "Ctrl+Ctrl+1" }),
        json!({ "command": "view.thumbsSmall", "context": "library", "key": "mac:Ctrl+1" }),
        json!({ "command": "view.thumbsSmall", "context": "attic", "key": "F9" }),
        json!({ "command": "view.thumbsSmall", "context": "library", "key": ["F9"] }),
    ] {
        let r = s.execute("app.setShortcut", &p);
        assert!(matches!(r, Err(CmdError::BadParams(_) | CmdError::NotFound(_))), "{p}: {r:?}");
    }
    // Conflicts: Ctrl+8 is Star in Library; F11 (Global) clashes in every context.
    let r = s.execute("app.setShortcut", &json!({ "command": "view.thumbsSmall", "context": "library", "key": "Ctrl+8" }));
    assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("Ctrl+8 is already Star / Unstar in Library.")), "{r:?}");
    let r = s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "editView", "key": "F11" }));
    assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("F11 is already window.fullscreen in Global.")), "{r:?}");
    // The same key in another view context is allowed.
    s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "editView", "key": "Ctrl+1" })).unwrap();
    assert!(matches!(s.execute("app.resetShortcuts", &json!({ "command": "view.thumbsHuge" })), Err(CmdError::NotFound(_))));
    s.execute("app.resetShortcuts", &json!({ "command": "window.fullscreen" })).unwrap();
    // A UI default that does not parse is refused and changes nothing.
    let before = s.keymap().bindings();
    assert!(s.add_ui_keys(&[("bad.command", &[(Context::Library, "plan9:F1")])]).is_err());
    assert_eq!(s.keymap().bindings(), before);
    assert!(s.keymap().conflicts().is_empty());
}

/// What `keymap.list` returns, as (command, context) pairs.
fn listed(s: &mut Session, params: serde_json::Value) -> Vec<(String, serde_json::Value)> {
    let r = s.execute("keymap.list", &params).unwrap();
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(r["count"], json!(rows.len()));
    rows.iter().map(|row| (row["command"].as_str().unwrap().to_string(), row["context"].clone())).collect()
}

/// The UI commands of the listing tests: Ctrl+1 in two view contexts, a global key, one without.
const UI_LIST: [(&str, DefaultKeys); 4] = [
    ("view.thumbsSmall", &[(Context::Library, "Ctrl+1")]),
    ("view.library", &[(Context::EditView, "Ctrl+1"), (Context::EditView, "Esc")]),
    ("window.fullscreen", &[(Context::Global, "F11")]),
    ("select.set", &[]),
];

/// Every command of the registry and of the UI's list, once per context it has default keys in;
/// with `withoutKeys` the commands without keys too, once each with no context.
#[test]
fn the_listing_covers_every_command_once_per_context() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    s.add_ui_keys(&UI_LIST).unwrap();
    let platform = Platform::current();
    let mut expected: Vec<(String, serde_json::Value)> = Vec::new();
    let mut keyless: Vec<String> = Vec::new();
    let declared = s.specs().iter().map(|spec| (spec.id, spec.keys)).chain(UI_LIST).collect::<Vec<_>>();
    for (id, keys) in &declared {
        let mut contexts: Vec<Context> = keys.iter().filter(|(_, k)| parse_default(k, platform).unwrap().1).map(|(c, _)| *c).collect();
        contexts.sort();
        contexts.dedup();
        if contexts.is_empty() {
            keyless.push(id.to_string());
        }
        expected.extend(contexts.into_iter().map(|c| (id.to_string(), json!(c))));
    }
    let mut with_keys = listed(&mut s, json!({}));
    with_keys.sort_by_key(|(c, x)| (c.clone(), x.to_string()));
    expected.sort_by_key(|(c, x)| (c.clone(), x.to_string()));
    assert_eq!(with_keys, expected);
    assert!(keyless.contains(&"select.set".to_string()) && keyless.contains(&"keymap.list".to_string()));
    let all = listed(&mut s, json!({ "withoutKeys": true }));
    assert_eq!(all.len(), expected.len() + keyless.len());
    for id in &keyless {
        assert_eq!(all.iter().filter(|(c, _)| c == id).collect::<Vec<_>>(), [&(id.clone(), json!(null))], "{id}");
    }
    // A row says what it binds: label and menu path, keys in effect and defaults, unmodified.
    let r = s.execute("keymap.list", &json!({ "context": "library", "search": "Star / Unstar" })).unwrap();
    assert_eq!(
        r["rows"],
        json!([{ "command": "photo.star.toggle", "label": "Star / Unstar", "menu": "Picture/Star / Unstar", "context": "library",
                 "keys": ["Ctrl+8", "F4"], "defaults": ["Ctrl+8", "F4"], "modified": false, "editable": true }])
    );
    // Pressed commands are editable; the hold gestures (the quick preview, Show original), once the
    // app adds them, are listed but not editable, and their key cannot be changed.
    let r = s.execute("keymap.list", &json!({ "withoutKeys": true })).unwrap();
    assert!(r["rows"].as_array().unwrap().iter().all(|row| row["editable"] == json!(true)));
    let holds: [(&str, DefaultKeys); 2] = [("peek", &[]), ("edit.showOriginal", &[(Context::EditView, "\\")])];
    s.add_ui_keys(&holds).unwrap();
    let r = s.execute("keymap.list", &json!({ "withoutKeys": true })).unwrap();
    for row in r["rows"].as_array().unwrap() {
        let hold = matches!(row["command"].as_str(), Some("peek" | "edit.showOriginal"));
        assert_eq!(row["editable"], json!(!hold), "{row}");
    }
    let refused = s.execute("app.setShortcut", &json!({ "command": "edit.showOriginal", "context": "editView", "key": "F9" }));
    assert!(matches!(refused, Err(CmdError::Disabled(_))), "{refused:?}");
}

/// Search by name, menu path and key, ignoring case and spelling; the Context filter.
#[test]
fn the_listing_searches_names_menu_paths_and_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    s.add_ui_keys(&UI_LIST).unwrap();
    let ctrl1 = [("view.thumbsSmall".to_string(), json!("library")), ("view.library".to_string(), json!("editView"))];
    for spelling in ["ctrl+1", "Ctrl 1", "CTRL-1", "cmd+1", "⌘1"] {
        let mut got = listed(&mut s, json!({ "search": spelling }));
        got.sort_by_key(|(c, _)| std::cmp::Reverse(c.clone()));
        assert_eq!(got, ctrl1, "{spelling}");
    }
    // By label and by menu path, ignoring case.
    let star = listed(&mut s, json!({ "search": "star / UNSTAR" }));
    assert_eq!(star, [("photo.star.toggle".into(), json!("library")), ("photo.star.toggle".into(), json!("editView"))]);
    assert!(listed(&mut s, json!({ "search": "edit/redo" })).iter().any(|(c, _)| c == "edit.redo"));
    // A key without modifiers finds it under any: F4 is Star's second key; F1 is not F11.
    assert!(listed(&mut s, json!({ "search": "f4" })).iter().all(|(c, _)| c == "photo.star.toggle"));
    assert!(listed(&mut s, json!({ "search": "F1" })).iter().all(|(c, _)| c != "window.fullscreen"));
    assert!(listed(&mut s, json!({ "search": "8" })).iter().any(|(c, _)| c == "photo.star.toggle"));
    // Text that is not a key matches the keys as written.
    let ctrl = listed(&mut s, json!({ "search": "ctrl+" }));
    assert!(ctrl.iter().any(|(c, _)| c == "photo.star.toggle") && ctrl.iter().all(|(c, _)| c != "window.fullscreen"));
    assert!(listed(&mut s, json!({ "search": "no such thing" })).is_empty());
    // The Context filter: Library hides Edit view rows; keyless commands belong to no context.
    let library = listed(&mut s, json!({ "context": "library", "search": "ctrl+1" }));
    assert_eq!(library, [("view.thumbsSmall".into(), json!("library"))]);
    let keyless = listed(&mut s, json!({ "context": "editView", "withoutKeys": true }));
    assert!(keyless.iter().all(|(_, c)| *c == json!("editView") || c.is_null()));
    assert!(keyless.contains(&("select.set".into(), json!(null))));
}

/// Two keys per context: a second key in slot 1 resolves beside the first; Star rebound to F8
/// in slot 0 keeps F4, and Ctrl+8 no longer stars. Removing a slot keeps the other.
#[test]
fn a_command_has_two_key_slots_per_context() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    let undo = |s: &Session, k: &str| s.keymap().resolve(&key(k), Context::Library, false);
    let r = s.execute("app.setShortcut", &json!({ "command": "edit.undo", "context": "library", "key": "Alt+Backspace", "slot": 1 })).unwrap();
    assert_eq!(r["keys"], json!(["Ctrl+Z", "Alt+Backspace"]));
    assert_eq!(undo(&s, "Ctrl+Z").as_deref(), Some("edit.undo"));
    assert_eq!(undo(&s, "Alt+Backspace").as_deref(), Some("edit.undo"));
    // Star rebound to F8 in slot 0, written as on screen.
    let r = s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "library", "key": "f8", "slot": 0 })).unwrap();
    assert_eq!(r["keys"], json!(["F8", "F4"]));
    assert_eq!(undo(&s, "F8").as_deref(), Some("photo.star.toggle"));
    assert_eq!(undo(&s, "Ctrl+8"), None);
    assert_eq!(s.keymap().resolve(&key("Ctrl+8"), Context::EditView, false).as_deref(), Some("photo.star.toggle"), "Edit view keeps its keys");
    // The same key in its other slot moves; removing slot 0 keeps slot 1.
    let r = s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "library", "key": "F4", "slot": 0 })).unwrap();
    assert_eq!(r["keys"], json!(["F4"]));
    let r = s.execute("app.setShortcut", &json!({ "command": "edit.undo", "context": "library", "key": null, "slot": 0 })).unwrap();
    assert_eq!(r["keys"], json!(["Alt+Backspace"]));
    let rows = s.execute("keymap.list", &json!({ "context": "library", "search": "edit/undo" })).unwrap();
    assert_eq!(rows["rows"][0]["keys"], json!(["Alt+Backspace"]));
    assert_eq!(rows["rows"][0]["defaults"], json!(["Ctrl+Z"]));
    assert_eq!(rows["rows"][0]["modified"], json!(true));
    // Back to the defaults by hand is no override at all.
    s.execute("app.setShortcut", &json!({ "command": "edit.undo", "context": "library", "key": "Ctrl+Z" })).unwrap();
    assert!(s.keymap().overrides().iter().all(|o| o.command != "edit.undo"));
}

/// A conflict names the other command's label, its context and the key; `replace` takes the key
/// and says from whom. Ctrl+1 in Library and in Edit view is no conflict.
#[test]
fn a_conflict_names_the_other_binding_and_replace_moves_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    s.add_ui_keys(&UI_LIST).unwrap();
    let set = json!({ "command": "edit.redo", "context": "library", "key": "Ctrl+8", "slot": 1 });
    let r = s.execute("app.setShortcut", &set);
    assert_eq!(r, Err(CmdError::BadParams("Ctrl+8 is already Star / Unstar in Library.".into())));
    assert_eq!(s.keymap().overrides(), [], "a refusal changes nothing");
    let mut replace = set.clone();
    replace["replace"] = json!(true);
    let r = s.execute("app.setShortcut", &replace).unwrap();
    assert_eq!(r["took"], json!([{ "command": "photo.star.toggle", "context": "library" }]));
    assert_eq!(s.keymap().resolve(&key("Ctrl+8"), Context::Library, false).as_deref(), Some("edit.redo"));
    assert_eq!(s.keymap().keys_for("photo.star.toggle", Context::Library), [key("F4")]);
    // Ctrl+1 is Small Thumbnails in Library and Back to Library in Edit view.
    let r = s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "editView", "key": "Ctrl+1" }));
    assert_eq!(r, Err(CmdError::BadParams("Ctrl+1 is already view.library in Edit view.".into())));
    s.execute("app.setShortcut", &json!({ "command": "view.thumbsSmall", "context": "library", "key": "Ctrl+1" })).unwrap();
    s.execute("app.setShortcut", &json!({ "command": "edit.undo", "context": "slideshow", "key": "Ctrl+1" })).unwrap();
    assert_eq!(s.keymap().conflicts(), []);
}

/// `app.resetShortcuts {}` restores every default binding exactly, as the listing shows them.
#[test]
fn reset_restores_every_default_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    s.add_ui_keys(&UI_LIST).unwrap();
    let before = s.execute("keymap.list", &json!({ "withoutKeys": true })).unwrap();
    for p in [
        json!({ "command": "photo.star.toggle", "context": "library", "key": "F8", "slot": 0 }),
        json!({ "command": "edit.undo", "context": "library", "key": null }),
        json!({ "command": "select.set", "context": "library", "key": "F9" }),
        json!({ "command": "edit.redo", "context": "editView", "key": "Esc", "replace": true }),
    ] {
        s.execute("app.setShortcut", &p).unwrap();
    }
    assert_ne!(s.execute("keymap.list", &json!({ "withoutKeys": true })).unwrap(), before);
    s.execute("app.resetShortcuts", &json!({})).unwrap();
    assert_eq!(s.execute("keymap.list", &json!({ "withoutKeys": true })).unwrap(), before);
    assert_eq!(s.keymap().overrides(), []);
}

#[test]
fn the_listing_refuses_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    for p in [
        json!({ "context": "kitchen" }),
        json!({ "context": 1 }),
        json!({ "search": 8 }),
        json!({ "withoutKeys": "yes" }),
        json!({ "without_keys": true }),
        json!([]),
    ] {
        let r = s.execute("keymap.list", &p);
        assert!(matches!(r, Err(CmdError::BadParams(_))), "{p}: {r:?}");
    }
    let r = s.execute("keymap.list", &json!({ "search": "x".repeat(257) }));
    assert!(matches!(r, Err(CmdError::Limit(_))), "{r:?}");
    assert!(s.execute("keymap.list", &json!({ "search": "é".repeat(128) })).unwrap()["rows"].as_array().unwrap().is_empty());
}

/// Labels on screen: `Ctrl+Shift+S` on Windows and Linux, `⇧⌘S` on macOS; every default key
/// reads back from its label on every platform, and the spellings people type are taken.
#[test]
fn keys_have_a_label_per_platform_and_lenient_spellings() {
    assert_eq!(key("Ctrl+Shift+S").pretty(Platform::Mac), "⇧⌘S");
    assert_eq!(key("Ctrl+Shift+S").pretty(Platform::Windows), "Ctrl+Shift+S");
    assert_eq!(key("Ctrl+Alt+Shift+Enter").pretty(Platform::Mac), "⌥⇧⌘↩");
    assert_eq!(key("Esc").pretty(Platform::Mac), "⎋");
    assert_eq!(key("Ctrl++").pretty(Platform::Mac), "⌘+");
    assert_eq!(key("Ctrl+,").pretty(Platform::Mac), "⌘,");
    for p in Platform::ALL {
        let map = Keymap::from_specs_on(&dalica_engine::cmd::specs(), p).unwrap();
        for b in map.bindings() {
            assert_eq!(Key::parse_lenient(&b.key.pretty(p)), Ok(b.key.clone()), "{} on {p:?}", b.key);
        }
    }
    for (text, canonical) in [
        ("ctrl-1", "Ctrl+1"),
        ("Ctrl 1", "Ctrl+1"),
        (" shift  ctrl s ", "Ctrl+Shift+S"),
        ("ctrl--", "Ctrl+-"),
        ("ctrl -", "Ctrl+-"),
        ("ctrl +", "Ctrl+Plus"),
        ("⌘⇧Z", "Ctrl+Shift+Z"),
        ("⌥←", "Alt+Left"),
        ("⇟", "PageDown"),
        ("-", "-"),
        ("+", "Plus"),
    ] {
        assert_eq!(Key::parse_lenient(text).map(|k| k.to_string()), Ok(canonical.to_string()), "{text}");
    }
    for bad in ["", "ctrl", "⌘", "⌃⌘S", "Ctrl+", "ctrl-shift", "Hyper-1", "mac:Ctrl+Y", "ctrl ctrl 1", &"ctrl ".repeat(20)] {
        assert!(Key::parse_lenient(bad).is_err(), "{bad:?}");
    }
}
