//! Geotags (`organizing.md` §7, `data-model.md` §5; the state is `crate::places`):
//! `photo.geo.set {ids, geo | null, confirm?}`, `geo.copy {id}`, `geo.paste {ids}`, `geo.clear
//! {ids, confirm?}` (Tools ▸ Geotag) and `places.list {ids? | section?}`. Empty until M13.14.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
