//! Album commands (`organizing.md` §4, `data-model.md` §3.2): `album.create`, `album.info.set`,
//! `album.add`, `album.remove`, `album.reorder`, `album.setCover`, `album.delete` and the
//! `albums.list` query, each one library undo step. Empty until M9.10.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
