//! The organizing helpers of `mcp.md` §4.2 (`scan_folder`, `query_photos`, `photo_info`,
//! `select_photos`, `set_metadata`, `create_album`, `jobs_list`, `jobs_cancel`), acting on the
//! headless tray when `ids` is omitted. Empty until M10.13.

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
