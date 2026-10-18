//! Face models (`faces.md` §3 and §9; the state is `crate::face_models`): `faces.models.list`,
//! `faces.models.inspect {path}`, `faces.models.install {path, acknowledged}`,
//! `faces.models.download {id, acknowledged}`, `faces.models.cancel {id}`, `faces.models.remove
//! {id}` and `faces.models.select {id}`; nothing is installed or fetched unless `acknowledged` is
//! `true`. Empty until M13.16.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
