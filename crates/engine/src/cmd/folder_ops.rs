//! Moving folders (`organizing.md` §9, `data-model.md` §8): `folder.move` into a library folder,
//! merging photo by photo into a folder of the same name, its watched-folder entry rebased
//! (`Session::rebase_folders`); a file operation (`fileops.rs`). Empty until M9.15.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
