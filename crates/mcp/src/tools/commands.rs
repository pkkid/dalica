//! One tool per command (`mcp.md` §4.4): `cmd_` + the command id with `.` replaced by `_`, its
//! annotations derived from the command's effect. Empty until M10.15; until then `command_run`
//! reaches every command. `--compact` leaves this group out.

use serde_json::Value;

use super::{Tool, ToolResult};
use crate::backend::{Backend, Progress};

/// This group's tools.
pub(crate) fn tools(_b: &mut dyn Backend) -> Vec<Tool> {
    Vec::new()
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(_b: &mut dyn Backend, _name: &str, _args: &Value, _progress: &mut dyn Progress) -> Option<ToolResult> {
    None
}
