//! The connect-mode UI tools of `mcp.md` §4.3 (`screenshot`, `inspect_ui`, `list_widgets`,
//! `click`, `drag`, `press_key`, `type_text`, `scroll`, `menu_invoke`, `settle`,
//! `control_call`). Empty until M10.17.

use serde_json::Value;

use super::{Tool, ToolResult};
use crate::backend::{Backend, Progress};

/// This group's tools (`has_ui`: a window is attached).
pub(crate) fn tools(_has_ui: bool) -> Vec<Tool> {
    Vec::new()
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(_b: &mut dyn Backend, _name: &str, _args: &Value, _progress: &mut dyn Progress) -> Option<ToolResult> {
    None
}
