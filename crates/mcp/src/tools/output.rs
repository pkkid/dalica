//! The output helpers of `mcp.md` §4.2 (`render_photo`, `export`), with progress and cancellation
//! per photo. Empty until M10.14.

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
