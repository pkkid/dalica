//! `import.run {ids?, destination, folderName, after}` and `import.removeFromSource {confirm}`
//! (`organizing.md` §3 items 4–6): copying, verifying and entering the photos as one undo step,
//! and removing a card's copied files. Empty until M11.8.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
