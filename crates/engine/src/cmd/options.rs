//! The Options dialog's commands (`menus-and-shortcuts.md` §5, `design-system.md` §11.3):
//! `options.list {tab?}` (query: the catalogue's rows with their values) and `options.set {values}`
//! (checked against the catalogue as a whole, one atomic write, no undo step), over the catalogue
//! in `crate::options`. Empty until M10.5.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
