//! Shortcut dispatch (`menus-and-shortcuts.md` §2.0): each key press of the frame becomes the
//! keymap's [`Key`], is resolved in the active context and runs its command through [`App::run`]
//! with the parameters the context implies ([`params_for`]: the photos it acts on). While a text
//! field has the focus a key resolves Text input, then Global: the view's shortcuts are off while
//! typing. Plain arrows go to the library tree instead while it has the focus. [`join_keymap`]
//! adds the UI commands to the session's keymap, so their keys are resolved, conflict-checked and
//! rebindable like the engine's.

use dalica_engine::keymap::DefaultKeys;
use dalica_engine::{Context, Key, Session};
use serde_json::{Value, json};

use crate::app::{App, UI_COMMANDS};

/// Every UI command with its default keys, as the session's keymap takes them.
pub fn ui_keys() -> Vec<(&'static str, DefaultKeys)> {
    UI_COMMANDS.iter().map(|c| (c.id, c.keys)).collect()
}

/// Adds the UI commands to `session`'s keymap (`Session::add_ui_keys`). A default that does not
/// parse is a bug the tests catch; here it is logged and the engine's keys keep working.
pub fn join_keymap(session: &mut Session) {
    if let Err(e) = session.add_ui_keys(&ui_keys()) {
        log::error!("a UI command's default shortcut does not parse: {e}");
    }
}

/// The keymap's key for an egui key press, if it has one. `command` is the primary modifier (Ctrl,
/// or Cmd on macOS).
pub fn from_egui(key: egui::Key, modifiers: egui::Modifiers) -> Option<Key> {
    let code = match key {
        egui::Key::Escape => "Esc".to_string(),
        egui::Key::Plus => "Plus".to_string(),
        // Punctuation goes by its character (`Ctrl+,`); arrows have symbols too, but go by name.
        k if k.symbol_or_name().len() == 1 && k.symbol_or_name().bytes().all(|b| b.is_ascii_punctuation()) => k.symbol_or_name().to_string(),
        k => k.name().to_string(),
    };
    let mut text = String::new();
    for (on, name) in [(modifiers.command, "Ctrl+"), (modifiers.alt, "Alt+"), (modifiers.shift, "Shift+")] {
        if on {
            text.push_str(name);
        }
    }
    text.push_str(&code);
    Key::parse(&text).ok()
}

/// The photo commands that act on the Photo Tray's photos (the held ones and the selection,
/// [`crate::tray::photos`]) rather than only the selection: the star and rotate buttons under the
/// tray act on it (`ui-layout.md` §6.3), and their shortcuts and menu items act on the same set;
/// so do the quick tags' Ctrl+Alt+1 … Ctrl+Alt+0 (`organizing.md` §5.2).
pub const TRAY_COMMANDS: [&str; 14] = [
    "photo.star.toggle",
    "photo.star.set",
    "photo.rotate.cw",
    "photo.rotate.ccw",
    "tags.quick.1",
    "tags.quick.2",
    "tags.quick.3",
    "tags.quick.4",
    "tags.quick.5",
    "tags.quick.6",
    "tags.quick.7",
    "tags.quick.8",
    "tags.quick.9",
    "tags.quick.10",
];

/// Whether command `id` acts on the tray's photos ([`TRAY_COMMANDS`]).
pub fn acts_on_tray(id: &str) -> bool {
    TRAY_COMMANDS.contains(&id)
}

/// The photos command `id` acts on when run from a shortcut, a menu or a bottom button: the tray's
/// for [`TRAY_COMMANDS`], else the selection.
pub fn targets(app: &App, id: &str) -> Vec<dalica_core::PhotoId> {
    let state = app.state();
    if acts_on_tray(id) { crate::tray::photos(state) } else { state.selection.ids.clone() }
}

/// The parameters a shortcut passes to `id`: for commands that take `ids`, the photos it acts on
/// ([`targets`]).
pub fn params_for(app: &App, id: &str) -> Value {
    let takes_ids = app.session().and_then(|s| s.spec(id)).is_some_and(|spec| spec.params.contains("\"ids\""));
    if takes_ids { json!({ "ids": targets(app, id) }) } else { json!({}) }
}

/// Runs the commands bound to this frame's key presses. Returns the ids that ran (or were refused;
/// a refusal's reason is on the status line).
pub fn dispatch(app: &mut App, ctx: &egui::Context) -> Vec<String> {
    // With the library tree focused, its pane moves its own selection with the plain arrows.
    let tree_focused = crate::panels::folders::has_focus(ctx);
    let presses: Vec<Key> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } if tree_focused && crate::panels::folders::is_tree_key(*key, *modifiers) => {
                    None
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => from_egui(*key, *modifiers),
                _ => None,
            })
            .collect()
    });
    let text_input = ctx.egui_wants_keyboard_input();
    // While a text field has the focus, the view's shortcuts are off (Ctrl+8, Ctrl+A, Ctrl+Z are
    // the field's or nothing): Text input, then Global.
    let view = if text_input { Context::Global } else { Context::Library };
    let mut ran = Vec::new();
    for key in presses {
        let Some(id) = app.session().and_then(|s| s.keymap().resolve(&key, view, text_input)) else { continue };
        let params = params_for(app, &id);
        let _ = app.run(&id, &params);
        ran.push(id);
    }
    ran
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    #[test]
    fn egui_presses_become_keymap_keys() {
        let ctrl = Modifiers { command: true, ctrl: true, ..Modifiers::NONE };
        for (key, mods, want) in [
            (egui::Key::Num8, ctrl, "Ctrl+8"),
            (egui::Key::F4, Modifiers::NONE, "F4"),
            (egui::Key::Escape, Modifiers::NONE, "Esc"),
            (egui::Key::Z, Modifiers { shift: true, ..ctrl }, "Ctrl+Shift+Z"),
            (egui::Key::Comma, ctrl, "Ctrl+,"),
            (egui::Key::Plus, ctrl, "Ctrl+Plus"),
            (egui::Key::ArrowUp, Modifiers::SHIFT, "Shift+Up"),
            (egui::Key::A, Modifiers::ALT, "Alt+A"),
        ] {
            assert_eq!(from_egui(key, mods).map(|k| k.to_string()).as_deref(), Some(want), "{key:?}");
        }
        // Keys the keymap has no name for are ignored, never a panic.
        assert_eq!(from_egui(egui::Key::Copy, Modifiers::NONE), None);
    }

    /// While a text field has the focus the view's shortcuts are off (`menus-and-shortcuts.md`
    /// §2.0): Ctrl+8, Ctrl+A and Ctrl+Z typed into the caption editor star, select and undo
    /// nothing in the library, while Global's F11 still works.
    #[test]
    fn the_views_shortcuts_are_off_while_a_field_has_the_focus() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(3);
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        d.run("photo.star.toggle", &json!({ "ids": [ids[2].get()] })).unwrap();
        d.run("select.set", &json!({ "ids": [ids[0].get()] })).unwrap();
        d.run("photo.caption.edit", &json!({})).unwrap();
        d.settle(3600).unwrap();
        assert!(d.ctx().egui_wants_keyboard_input(), "the editor has the focus");
        let starred = |d: &crate::headless::Driver| -> Vec<bool> {
            let lib = d.app().session().unwrap().library();
            ids.iter().map(|id| lib.photo(*id).unwrap().star).collect()
        };
        for key in ["Ctrl+8", "Ctrl+A", "Ctrl+Z"] {
            d.key(key).unwrap();
        }
        assert_eq!(starred(&d), [false, false, true], "no star set, none undone");
        assert_eq!(d.state().selection.ids, vec![ids[0]], "nothing selected");
        assert_eq!(d.app().session().unwrap().undo_stack().len(), 1);
        d.key("F11").unwrap();
        assert!(d.app().state().full_screen, "Global keys still work");
        assert!(d.ctx().egui_wants_keyboard_input(), "still typing");
    }

    /// UI commands' keys run them like the engine's: F11, Ctrl+1 / Ctrl+2, Ctrl+0, and a rebinding
    /// moves the command to its new key.
    #[test]
    fn ui_command_keys_run_their_commands() {
        let (_dir, s, _) = crate::app::tests::session_with_photos(2);
        let mut d = crate::headless::Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.key("F11").unwrap();
        assert!(d.app().state().full_screen);
        d.key("F11").unwrap();
        assert!(!d.app().state().full_screen);
        d.key("Ctrl+1").unwrap();
        assert_eq!(d.app().state().thumb_size, crate::menus::THUMB_SMALL);
        d.key("Ctrl+2").unwrap();
        assert_eq!(d.app().state().thumb_size, crate::state::THUMB_DEFAULT);
        d.key("Ctrl+0").unwrap();
        assert_eq!(d.app().state().pane, Some(crate::state::Pane::Properties));
        d.key("Ctrl+0").unwrap();
        assert_eq!(d.app().state().pane, None);
        d.app_mut().run("app.setShortcut", &json!({ "command": "window.fullscreen", "context": "global", "key": "F12" })).unwrap();
        d.key("F11").unwrap();
        assert!(!d.app().state().full_screen, "F11 is no longer bound");
        d.key("F12").unwrap();
        assert!(d.app().state().full_screen);
    }
}
