//! `doc_inspect` (`mcp.md` §4.1): the library as `library.inspect` gives it
//! (`control-protocol.md` §6.4), one implementation behind both modes: headless it is the engine
//! query `library.inspect` (run as `engine.execute`), and with a window attached (connect mode) the
//! control method of that name, which adds the window's fields (`current`, `visibleSections`,
//! `selection`, `tray`, `scroll`). The arguments pass through unchanged; the engine checks them.

use serde_json::{Map, Value, json};

use super::{Tool, ToolResult, object};
use crate::backend::{Backend, Progress};

/// This group's tools.
pub(crate) fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "doc_inspect",
        title: "Inspect library",
        description: "The library as the user sees it: revision, watched folders, counts, the library tree, the light table's sections, search mode, view settings, undo, unsaved edits, pending writes, problems and scan progress (and with the app attached its current section, selection, Photo Tray and scroll). `photos: true` (or a section id such as \"folder:3\") adds the photos in light-table order, paged by `offset` and `limit` (200 by default, at most 1000), with `page: {offset, limit, total}`; `section` restricts them to one section. Changes nothing.".into(),
        input: object(
            json!({
                "photos": {"type": ["boolean", "string"], "description": "true, or a section id (folder:<id>, album:<id>, album:starred, album:search): list the photos"},
                "section": {"type": "string", "description": "Only this section's photos (folder:<id>, album:<id>, album:starred, album:recent, album:search)"},
                "offset": {"type": "integer", "description": "Photos to skip (default 0)"},
                "limit": {"type": "integer", "description": "Photos to list (default 200, at most 1000)"},
            }),
            &[],
        ),
        output: Some(json!({"type": "object", "properties": {
            "revision": {"type": "integer"}, "counts": {"type": "object"}, "tree": {"type": "object"}, "sections": {"type": "array"},
        }, "required": ["revision", "counts", "tree", "sections"]})),
        read_only: true,
        destructive: false,
        idempotent: true,
    }]
}

/// Runs `name` if it is one of this group's tools.
pub(crate) fn call(b: &mut dyn Backend, name: &str, args: &Value, progress: &mut dyn Progress) -> Option<ToolResult> {
    if name != "doc_inspect" {
        return None;
    }
    let mut params = Map::new();
    for key in ["photos", "section", "offset", "limit"] {
        if let Some(v) = args.get(key).filter(|v| !v.is_null()) {
            params.insert(key.into(), v.clone());
        }
    }
    let params = Value::Object(params);
    Some(ToolResult::from(if b.has_ui() {
        b.call("library.inspect", params, progress)
    } else {
        b.call("engine.execute", json!({ "command": "library.inspect", "params": params }), progress)
    }))
}
