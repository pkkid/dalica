//! People as data (`organizing.md` §6, `faces.md` §9; the face tiles are `crate::face_tiles`):
//! `people.list {sort}` (counts, thumbnails, pending suggestions), `people.create {name,
//! nickname?}`, `people.rename`, `people.merge`, `people.delete {confirm}` and
//! `people.setThumbnail`, each change one library undo step. Empty until M13.15.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
