//! Faces as data (`organizing.md` §6, `faces.md` §9): `faces.list {person | unnamed | ids,
//! ignored?}`, `faces.name {faces, person | name, create?}`, `faces.ignore {faces}`, `faces.move
//! {faces, person}`, `faces.remove {faces}` (back to Unnamed) and `faces.add {id, rect, person?}`
//! (a face drawn by hand), each change one library undo step. Empty until M13.15.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
