//! Shortcut dispatch (`menus-and-shortcuts.md` §2.0): each key press of the frame becomes the
//! keymap's [`Key`], is resolved in the active context (a focused text field first), and runs its
//! command through [`App::run`] with the parameters the context implies (the selection, for photo
//! commands).

use dalica_engine::{Context, Key};
use serde_json::{Value, json};

use crate::app::App;

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

/// The parameters a shortcut passes to `id`: the selected photos for commands that take `ids`.
pub fn params_for(app: &App, id: &str) -> Value {
    let takes_ids = app.session().and_then(|s| s.spec(id)).is_some_and(|spec| spec.params.contains("\"ids\""));
    if takes_ids { json!({ "ids": app.state().selection.ids }) } else { json!({}) }
}

/// Runs the commands bound to this frame's key presses. Returns the ids that ran (or were refused;
/// a refusal's reason is on the status line).
pub fn dispatch(app: &mut App, ctx: &egui::Context) -> Vec<String> {
    let presses: Vec<Key> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => from_egui(*key, *modifiers),
                _ => None,
            })
            .collect()
    });
    let text_input = ctx.egui_wants_keyboard_input();
    let mut ran = Vec::new();
    for key in presses {
        let Some(id) = app.session().and_then(|s| s.keymap().resolve(&key, Context::Library, text_input)) else { continue };
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
}
