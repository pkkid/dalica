//! `doc_inspect` (`mcp.md` §4.1): the library as `library.inspect` gives it
//! (`control-protocol.md` §6.4). Empty until M10.7 makes `library.inspect` an engine query.

use serde_json::Value;

use super::{Tool, ToolResult};
use crate::backend::{Backend, Progress};

/// This group's tools.
pub(crate) fn tools() -> Vec<Tool> {
    Vec::new()
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(_b: &mut dyn Backend, _name: &str, _args: &Value, _progress: &mut dyn Progress) -> Option<ToolResult> {
    None
}
