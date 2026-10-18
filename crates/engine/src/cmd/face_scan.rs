//! The face scan's commands (`faces.md` §4 and §9; the scan is `crate::faces_scan`, its index
//! `crate::faces_index`): `faces.enable {on}`, `faces.detect {ids, apply?}`, `faces.index`,
//! `faces.reset {ids}` (Picture ▸ Find Faces Again) and `faces.removeInFolders {folders, confirm}`
//! (the Folder Manager's Remove Faces). Empty until M13.18.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
