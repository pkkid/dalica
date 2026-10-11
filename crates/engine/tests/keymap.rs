//! The keymap and its settings (`menus-and-shortcuts.md` §2.0): key parsing, contexts, conflicts,
//! resolution, rebinding through `app.setShortcut`, and a rebinding surviving a restart.

use std::sync::Arc;

use dalica_core::SystemClock;
use dalica_engine::keymap::{DefaultKeys, Platform, first_default, parse_default};
use dalica_engine::{CmdError, Context, Key, Keymap, Session};
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
    map.set("photo.star.toggle", Context::Library, Some(key("F9")), false).unwrap();
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
    assert!(map.set("select.set", Context::Library, Some(key("F11")), false).is_err());
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
    assert_eq!(map.set("star", Context::Library, Some(key("Ctrl+1")), false), Err("Ctrl+1 is already small in Library.".into()));
    map.set("star", Context::Library, Some(key("Ctrl+1")), true).unwrap();
    assert_eq!(map.keys_for("star", Context::Library), [key("Ctrl+1")]);
    assert!(map.keys_for("small", Context::Library).is_empty());
    assert!(map.conflicts().is_empty());
    // Removing a binding, then resetting one command, then all.
    map.set("star", Context::Library, None, false).unwrap();
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
    s.execute("app.setShortcut", &json!({ "command": "edit.undo", "context": "library", "key": null })).unwrap();
    s.flush().unwrap();
    drop(s);
    let s = session(dir.path());
    assert_eq!(s.keymap().resolve(&key("Ctrl+Shift+8"), Context::Library, false).as_deref(), Some("photo.star.toggle"));
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
        json!([1]),
    ];
    for p in cases {
        let r = s.execute("app.setShortcut", &p);
        assert!(matches!(r, Err(CmdError::BadParams(_) | CmdError::NotFound(_))), "{p}: {r:?}");
    }
    // A clash with another command in the same context.
    let r = s.execute("app.setShortcut", &json!({ "command": "edit.redo", "context": "library", "key": "Ctrl+Z" }));
    assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("already edit.undo")), "{r:?}");
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
    assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("already photo.star.toggle")), "{r:?}");
    let r = s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "editView", "key": "F11" }));
    assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("already window.fullscreen")), "{r:?}");
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
