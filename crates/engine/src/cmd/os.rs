//! The operating system's ways with files (`organizing.md` §9, `viewing.md` §3.3, `security.md`
//! §5.11; the service is `crate::os::Os`): `photo.locate {id}`, `folder.locate {id}`,
//! `photo.openWithList {id}`, `photo.openWith {id, handler}`, `photo.openExternal {ids}`,
//! `photo.copyToSystem {ids, picture?}`, `photo.copyPath {ids}` (M15.17), `viewer.fileTypes` and
//! `viewer.setDefault {types}` (M15.24). The launching ones are refused over automation. Empty
//! until M15.17.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
