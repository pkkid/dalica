//! The effect tabs' one-click effects (`editing.md` §8.1, M12): `edit.effect.apply {name}`, a
//! one-click tile's click (Sepia, B&W, Warmify, Invert Colors), and `edit.bw`, B&W on the photo
//! shown (Ctrl+Shift+B in the Edit context only; in the library Black and White is Picture ▸
//! Batch Edit ▸ Black and White). A parameter effect opens its panel instead:
//! `edit.tool.open {tool: "effect", name}` ([`super::effect_session`]).
//!
//! **Stubs until M12.15**, which owns this file: both commands check their parameters, the edit
//! view and that no tool is open, then refuse with "Not available yet."; M12.15 makes the apply one
//! step of the photo's history ("Undo Sepia") and the tile grid ([`crate::panels::edit::effects`])
//! calls it.

use dalica_edit::catalog::OpInfo;
use dalica_engine::{CmdError, Context};
use serde::Deserialize;
use serde_json::{Value, json};

use super::edit_view::{NOT_EDITING, open_photo};
use super::{Check, UiCommand, no_params, parse};
use crate::app::App;
use crate::state::effect_row;

/// What a stub answers.
pub const NOT_YET: &str = "Not available yet.";

/// Whether effect `row` applies with one click (a tile without a panel): it has no parameters.
/// The seam the catalogue's own one-click column (M12.1) replaces.
pub fn one_click(row: &OpInfo) -> bool {
    row.params.is_empty()
}

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "edit.effect.apply",
        label: "Apply Effect",
        menu: "",
        keys: &[],
        params: r#"{"name": string}: apply one-click effect `name` ("sepia", "bw", "warmify", …) to the photo shown in the edit view, as one step of its history"#,
        run: apply,
    },
    UiCommand {
        id: "edit.bw",
        label: "B&W",
        menu: "",
        keys: &[(Context::EditView, "Ctrl+Shift+B")],
        params: "{}: apply B&W to the photo shown in the edit view, as one step of its history",
        run: bw,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("edit.effect.apply", can_apply), ("edit.bw", can_apply)];

/// Whether an effect can be applied now: the edit view shows a photo and no tool is open.
fn can_apply(app: &App, _params: &Value) -> Result<(), String> {
    if open_photo(app).is_none() {
        return Err(NOT_EDITING.into());
    }
    match super::tools::open_tool(app) {
        Some(t) => Err(format!("Apply or cancel {} first.", t.label())),
        None => Ok(()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Apply {
    name: String,
}

fn apply(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Apply = parse(params)?;
    let shown: String = p.name.chars().take(40).collect();
    let row = effect_row(&p.name).ok_or_else(|| CmdError::BadParams(format!("`name` is an effect of tabs 3–5 (\"sepia\"), not `{shown}`")))?;
    if !one_click(row) {
        return Err(CmdError::BadParams(format!("{} has parameters: open its panel (edit.tool.open {{tool: \"effect\", name}})", row.label)));
    }
    can_apply(app, params).map_err(CmdError::Disabled)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

fn bw(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    apply(app, &json!({ "name": "bw" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn one_click_effects_are_checked_then_wait_for_their_task() {
        let (_dir, s, ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("edit.bw", &json!({})), Err(CmdError::Disabled(m)) if m == NOT_EDITING));
        app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
        assert!(matches!(app.run("edit.effect.apply", &json!({ "name": "sharpen" })), Err(CmdError::BadParams(m)) if m.contains("panel")));
        assert!(matches!(app.run("edit.effect.apply", &json!({ "name": "crop" })), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("edit.effect.apply", &json!({ "name": "sepia" })), Err(CmdError::Disabled(m)) if m == NOT_YET));
        app.run("edit.tool.open", &json!({ "tool": "crop" })).unwrap();
        assert!(matches!(app.run("edit.bw", &json!({})), Err(CmdError::Disabled(m)) if m.contains("Crop")));
    }
}
