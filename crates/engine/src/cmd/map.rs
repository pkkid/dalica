//! The map's network half (`organizing.md` §7; the state is `crate::map_tiles`): `map.allow {on}`
//! (Allow map tiles) and `places.search {text}` (typed coordinates found locally, else the
//! geocoding service, only when asked). Empty until M13.20.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
