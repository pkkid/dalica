//! The keymap and its settings (`menus-and-shortcuts.md` §2.0): key parsing, contexts, conflicts,
//! resolution, rebinding through `app.setShortcut`, and a rebinding surviving a restart.

use std::sync::Arc;

use dalica_core::SystemClock;
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
