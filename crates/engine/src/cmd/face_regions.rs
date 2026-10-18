//! People names in photo files (`xmp-interop.md` §6): `faces.writeXmp {ids, confirm}` (Tools ▸
//! Experimental ▸ Write People Names to Photos…), and the MWG regions written with Store people
//! names in photos (`people.storeNamesInPhotos`). Empty until M13.19.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
