//! `library.inspect` (`control-protocol.md` §6.4): the engine's half of the library's state for the
//! control channel and MCP's `doc_inspect`, one implementation for every surface. Empty until
//! M10.7.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
