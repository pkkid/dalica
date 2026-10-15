//! The shortcut editor's query (`menus-and-shortcuts.md` §2.0): `keymap.list {context?, search?,
//! withoutKeys?}`, one row per command and context with its keys in effect and its defaults.
//! Rebinding stays `app.setShortcut` / `app.resetShortcuts` in `cmd/app.rs`. Empty until M10.6.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
