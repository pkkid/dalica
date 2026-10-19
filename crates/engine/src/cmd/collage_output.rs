//! Create Collage, Save Draft and Edit Collage (`creating.md` §1 and §3; the state is
//! `crate::collage_output`): `collage.create {path?, display?}` and `collage.saveDraft` as a
//! `collage` job, `collage.edit {id}` and `collage.canEdit {id}`. Empty until M14.17.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
