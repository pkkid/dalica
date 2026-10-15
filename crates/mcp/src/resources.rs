//! `resources/list`, `resources/templates/list` and `resources/read` (`mcp.md` §7). Empty until
//! M10.15 serves `dalica://library`, `dalica://commands`, `dalica://selection`,
//! `dalica://photo/{id}` and `dalica://edit-ops`: the lists are empty and every URI is not found
//! (`-32002`).

use serde_json::{Value, json};

use crate::backend::Backend;

/// The resources served.
pub(crate) fn list(_backend: &mut dyn Backend) -> Value {
    json!({ "resources": [] })
}

/// The resource templates served.
pub(crate) fn templates(_backend: &mut dyn Backend) -> Value {
    json!({ "resourceTemplates": [] })
}

/// The contents of `uri`, or why not (`None`: no such resource, `-32002`).
pub(crate) fn read(_backend: &mut dyn Backend, _uri: &str) -> Option<Result<Value, String>> {
    None
}
