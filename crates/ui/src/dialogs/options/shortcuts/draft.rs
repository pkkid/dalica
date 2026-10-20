//! The shortcut editor's draft (`menus-and-shortcuts.md` §2.0), pure: what the editor's changes
//! lead to, and what a key press, Replace, ↺ and Reset All put into the draft. Nothing here writes
//! the session's keymap: the draft is a list of `app.setShortcut`-shaped changes
//! ([`ShortcutChange`]: a command's key in one slot of one context, or none), applied in order on
//! a copy of the keymap to show what Apply would leave, and run through `app.setShortcut` and
//! `app.resetShortcuts` by the dialog's Apply ([`super::apply`]).
//!
//! - A key press while a cell records ([`press`]): Esc cancels; Backspace removes the cell's key; a
//!   key another command has where both can be active (the same context, or Global and any other)
//!   leaves the cell recording with a conflict for [Replace] or [Cancel]; any other key becomes the
//!   cell's. Every key the window reports is a key with its modifiers: egui reports no modifier
//!   key on its own, so the first key pressed is the one recorded (PhotoCraft's
//!   `is_modifier_key` filter is not needed).
//! - [`replace`] removes the key from every command that has it where the cell's context can be
//!   active, then gives it to the cell: the changes say so explicitly, so Apply needs no `replace`
//!   and a change it would refuse is never applied silently.
//! - [`restore`] (↺) puts a row back to its default keys, as if each were recorded in turn.
//! - After every change the draft is reduced to nothing when it leads back to the keymap in effect
//!   ([`settle`]), so a key changed and put back leaves Apply disabled (`design-system.md` §11.3).
//!
//! The conflict test and the key slots follow LightCraft `crates/ui-egui/src/panels/keymap.rs`
//! (`capture`) at b3d392b4 and PhotoCraft `crates/ui-egui/src/prefs_ui.rs` (`shortcuts_body`) at
//! 583425b4 (both MIT OR Apache-2.0), rewritten over Dalica's keymap and draft.

use dalica_engine::keymap::{SetError, overlapping};
use dalica_engine::{Context, Key, Keymap};

use crate::state::{KeySlot, MAX_SHORTCUT_CHANGES, ShortcutChange, ShortcutConflict, ShortcutEditor};
use crate::tl;

/// What the editor says when its draft is full (it holds at most [`MAX_SHORTCUT_CHANGES`]).
pub const FULL: &str = "Too many changed shortcuts: apply them first.";

/// The keymap the draft leads to: `base` (the session's) with Reset All, then each change in
/// order. A change the keymap refuses (a key another command has) is skipped here; Apply refuses
/// it ([`plan`]).
pub fn keymap(base: &Keymap, e: &ShortcutEditor) -> Keymap {
    let mut m = base.clone();
    if e.reset_all {
        m.reset(None);
    }
    for c in &e.changes {
        let key = c.key.as_deref().and_then(|k| Key::parse(k).ok());
        let _ = m.set(&c.command, c.context, Some(usize::from(c.slot)), key, false);
    }
    m
}

/// Checks that the draft applies to `base` as it stands: Reset All, then every change, each
/// accepted by the keymap without taking a key from another command. `Err` names the first one
/// that would not apply, with the editor's sentence for a conflict (`label` names a command).
pub fn plan(base: &Keymap, e: &ShortcutEditor, label: &dyn Fn(&str) -> String) -> Result<(), String> {
    let mut m = base.clone();
    if e.reset_all {
        m.reset(None);
    }
    for c in &e.changes {
        if !m.knows(&c.command) {
            return Err(format!("no command `{}`", c.command));
        }
        let key = match c.key.as_deref() {
            Some(k) => Some(Key::parse(k)?),
            None => None,
        };
        m.set(&c.command, c.context, Some(usize::from(c.slot)), key, false).map_err(|e| match e {
            SetError::Conflict(b) => sentence(&b.key.to_string(), &label(&b.command), b.context),
            other => other.to_string(),
        })?;
    }
    Ok(())
}

/// "Ctrl+1 is already Small Thumbnails in Library.": `key` as the screen shows it.
pub fn sentence(key: &str, label: &str, context: Context) -> String {
    tl!("{key} is already {command} in {context}.", key = key, command = label.trim_end_matches('…'), context = super::context_text(context))
}

/// Whether two keymaps have the same bindings in effect.
pub fn same(a: &Keymap, b: &Keymap) -> bool {
    let set = |m: &Keymap| {
        let mut v: Vec<(String, Context, Key)> = m.bindings().into_iter().map(|b| (b.command, b.context, b.key)).collect();
        v.sort();
        v
    };
    set(a) == set(b)
}

/// Drops the draft when it leads to the keymap in effect, so Apply is disabled again.
pub fn settle(base: &Keymap, e: &mut ShortcutEditor) {
    if e.changed() && same(base, &keymap(base, e)) {
        e.changes.clear();
        e.reset_all = false;
    }
}

/// Adds a change; `false` when the draft is full ([`FULL`]).
fn push(e: &mut ShortcutEditor, cell: &KeySlot, key: Option<&Key>) -> bool {
    if e.changes.len() >= MAX_SHORTCUT_CHANGES {
        return false;
    }
    e.changes.push(ShortcutChange { command: cell.command.clone(), context: cell.context, slot: cell.slot, key: key.map(Key::to_string) });
    true
}

/// What a key press did to the draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pressed {
    /// Nothing was recording.
    Idle,
    /// Esc: recording ended, nothing changed.
    Cancelled,
    /// Backspace: the cell's key is removed.
    Removed,
    /// The key is the cell's now (or already was).
    Set,
    /// Another command has the key: the conflict waits for [Replace] or [Cancel].
    Conflict,
    /// The draft is full ([`FULL`]): recording ended.
    Full,
}

fn plain(key: &Key, code: &str) -> bool {
    !key.ctrl && !key.alt && !key.shift && key.code == code
}

/// A key pressed while a cell records (`e.recording`), against the session's keymap `base`.
pub fn press(base: &Keymap, e: &mut ShortcutEditor, key: &Key) -> Pressed {
    let Some(cell) = e.recording.clone() else { return Pressed::Idle };
    let done = |e: &mut ShortcutEditor| {
        e.recording = None;
        e.conflict = None;
    };
    if plain(key, "Esc") {
        done(e);
        return Pressed::Cancelled;
    }
    let draft = keymap(base, e);
    let keys = draft.keys_for(&cell.command, cell.context);
    if plain(key, "Backspace") {
        done(e);
        if usize::from(cell.slot) >= keys.len() {
            return Pressed::Removed;
        }
        let pressed = if push(e, &cell, None) { Pressed::Removed } else { Pressed::Full };
        settle(base, e);
        return pressed;
    }
    if keys.get(usize::from(cell.slot)) == Some(key) {
        done(e);
        return Pressed::Set;
    }
    if let Some(b) = draft.clash(&cell.command, cell.context, key) {
        e.conflict = Some(ShortcutConflict { cell, key: key.to_string(), command: b.command, context: b.context });
        return Pressed::Conflict;
    }
    done(e);
    let pressed = if push(e, &cell, Some(key)) { Pressed::Set } else { Pressed::Full };
    settle(base, e);
    pressed
}

/// [Replace]: the conflict's key leaves every command that has it where the cell's context can be
/// active and becomes the cell's. `false` when there was no conflict or the draft is full.
pub fn replace(base: &Keymap, e: &mut ShortcutEditor) -> bool {
    let Some(c) = e.conflict.take() else { return false };
    e.recording = None;
    let Ok(key) = Key::parse(&c.key) else { return false };
    let draft = keymap(base, e);
    let losers: Vec<KeySlot> = draft
        .bindings()
        .into_iter()
        .filter(|b| b.key == key && b.command != c.cell.command && overlapping(b.context, c.cell.context))
        .filter_map(|b| {
            let at = draft.keys_for(&b.command, b.context).iter().position(|k| *k == key)?;
            Some(KeySlot { command: b.command, context: b.context, slot: u8::try_from(at).ok()? })
        })
        .collect();
    if e.changes.len().saturating_add(losers.len()).saturating_add(1) > MAX_SHORTCUT_CHANGES {
        return false;
    }
    for l in &losers {
        push(e, l, None);
    }
    push(e, &c.cell, Some(&key));
    settle(base, e);
    true
}

/// [Cancel] on a conflict: the cell stops recording, nothing changes.
pub fn cancel(e: &mut ShortcutEditor) {
    e.conflict = None;
    e.recording = None;
}

/// ↺: `command`'s keys in `context` go back to their defaults, each as if recorded in its slot; a
/// default another command has now stops there with that conflict (the cell recording).
pub fn restore(base: &Keymap, e: &mut ShortcutEditor, command: &str, context: Context) -> Pressed {
    cancel(e);
    let draft = keymap(base, e);
    let (keys, defaults) = (draft.keys_for(command, context), draft.default_keys_for(command, context));
    if keys == defaults {
        return Pressed::Set;
    }
    if e.changes.len().saturating_add(keys.len()).saturating_add(defaults.len()) > MAX_SHORTCUT_CHANGES {
        return Pressed::Full;
    }
    let cell = |slot: usize| KeySlot { command: command.to_string(), context, slot: u8::try_from(slot).unwrap_or(u8::MAX) };
    for slot in (0..keys.len()).rev() {
        push(e, &cell(slot), None);
    }
    let mut pressed = Pressed::Set;
    for (slot, key) in defaults.iter().enumerate() {
        e.recording = Some(cell(slot));
        pressed = press(base, e, key);
        if pressed != Pressed::Set {
            break;
        }
    }
    settle(base, e);
    pressed
}

/// Reset All: every binding back to its default; the changes before it no longer count.
pub fn reset_all(base: &Keymap, e: &mut ShortcutEditor) {
    cancel(e);
    e.changes.clear();
    e.reset_all = true;
    settle(base, e);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Keymap {
        Keymap::new_on(
            [
                ("photo.star.toggle", Context::Library, "Ctrl+8"),
                ("photo.star.toggle", Context::Library, "F4"),
                ("photo.star.toggle", Context::EditView, "Ctrl+8"),
                ("view.thumbsSmall", Context::Library, "Ctrl+1"),
                ("view.library", Context::EditView, "Ctrl+1"),
                ("app.quit", Context::Global, "Ctrl+Q"),
            ],
            dalica_engine::keymap::Platform::Linux,
        )
        .unwrap()
    }

    fn key(k: &str) -> Key {
        Key::parse(k).unwrap()
    }

    fn recording(command: &str, context: Context, slot: u8) -> ShortcutEditor {
        ShortcutEditor { recording: Some(KeySlot { command: command.into(), context, slot }), ..ShortcutEditor::default() }
    }

    fn keys(m: &Keymap, command: &str, context: Context) -> Vec<String> {
        m.keys_for(command, context).iter().map(Key::to_string).collect()
    }

    #[test]
    fn a_key_press_records_cancels_removes_or_asks() {
        let b = base();
        let mut e = recording("photo.star.toggle", Context::Library, 0);
        assert_eq!(press(&b, &mut e, &key("F8")), Pressed::Set);
        assert_eq!((e.recording.is_none(), keys(&keymap(&b, &e), "photo.star.toggle", Context::Library)), (true, vec!["F8".into(), "F4".into()]));
        e.recording = Some(KeySlot { command: "photo.star.toggle".into(), context: Context::Library, slot: 1 });
        assert_eq!(press(&b, &mut e, &key("Esc")), Pressed::Cancelled);
        assert_eq!((e.recording.is_none(), e.changes.len()), (true, 1));
        e.recording = Some(KeySlot { command: "photo.star.toggle".into(), context: Context::Library, slot: 1 });
        assert_eq!(press(&b, &mut e, &key("Backspace")), Pressed::Removed);
        assert_eq!(keys(&keymap(&b, &e), "photo.star.toggle", Context::Library), ["F8"]);
        // Plan accepts it; the edit view keeps its own Ctrl+8.
        assert_eq!(plan(&b, &e, &|c| c.to_string()), Ok(()));
        assert_eq!(keys(&keymap(&b, &e), "photo.star.toggle", Context::EditView), ["Ctrl+8"]);
        // A key another command has where both are active asks; one in another view does not.
        let mut e = recording("photo.star.toggle", Context::Library, 0);
        assert_eq!(press(&b, &mut e, &key("Ctrl+Q")), Pressed::Conflict);
        let c = e.conflict.clone().unwrap();
        assert_eq!((c.command.as_str(), c.context, e.recording.is_some()), ("app.quit", Context::Global, true));
        assert!(e.changes.is_empty());
        cancel(&mut e);
        assert_eq!((e.conflict.is_none(), e.recording.is_none(), e.changed()), (true, true, false));
    }

    #[test]
    fn replace_takes_the_key_and_a_change_put_back_leaves_nothing_to_apply() {
        let b = base();
        let mut e = recording("photo.star.toggle", Context::Library, 0);
        assert_eq!(press(&b, &mut e, &key("Ctrl+1")), Pressed::Conflict);
        assert_eq!(sentence("Ctrl+1", "Small Thumbnails", Context::Library), "Ctrl+1 is already Small Thumbnails in Library.");
        assert!(replace(&b, &mut e));
        let m = keymap(&b, &e);
        assert_eq!(keys(&m, "photo.star.toggle", Context::Library), ["Ctrl+1", "F4"]);
        assert!(keys(&m, "view.thumbsSmall", Context::Library).is_empty());
        assert_eq!(keys(&m, "view.library", Context::EditView), ["Ctrl+1"], "another view keeps it");
        assert_eq!(plan(&b, &e, &|c| c.to_string()), Ok(()));
        // Without the removal the plan refuses, naming the binding.
        let mut bare = e.clone();
        bare.changes.retain(|c| c.command == "photo.star.toggle");
        assert_eq!(plan(&b, &bare, &|_| "Small Thumbnails".into()), Err("Ctrl+1 is already Small Thumbnails in Library.".into()));
        // ↺ on both rows puts everything back: nothing left to apply.
        restore(&b, &mut e, "photo.star.toggle", Context::Library);
        assert_eq!(restore(&b, &mut e, "view.thumbsSmall", Context::Library), Pressed::Set);
        assert!(!e.changed(), "{e:?}");
    }

    #[test]
    fn restore_stops_at_a_default_another_command_took_and_reset_all_undoes_everything() {
        let mut b = base();
        b.set("view.thumbsSmall", Context::Library, None, Some(key("F4")), true).unwrap();
        assert_eq!(keys(&b, "photo.star.toggle", Context::Library), ["Ctrl+8"]);
        let mut e = ShortcutEditor::default();
        assert_eq!(restore(&b, &mut e, "photo.star.toggle", Context::Library), Pressed::Conflict);
        assert_eq!(e.conflict.as_ref().map(|c| (c.key.as_str(), c.command.as_str())), Some(("F4", "view.thumbsSmall")));
        reset_all(&b, &mut e);
        assert!(e.reset_all && e.changes.is_empty() && e.conflict.is_none());
        let m = keymap(&b, &e);
        assert_eq!(keys(&m, "photo.star.toggle", Context::Library), ["Ctrl+8", "F4"]);
        assert_eq!(keys(&m, "view.thumbsSmall", Context::Library), ["Ctrl+1"]);
        // Nothing overridden: Reset All changes nothing, so there is nothing to apply.
        let mut e = ShortcutEditor::default();
        reset_all(&base(), &mut e);
        assert!(!e.changed());
    }
}
