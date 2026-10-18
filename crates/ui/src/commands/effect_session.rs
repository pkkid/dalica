//! The effect panels' session (`editing.md` §8.1, M12): the [`Tool`] behind
//! `edit.tool.open {tool: "effect", name}`, so `edit.tool.action`, `.apply`, `.cancel`, `.reset`
//! and Esc reach an open parameter panel as they reach Crop or Retouch
//! ([`super::tools`]). Its draft is [`crate::state::EditState::effect`].
//!
//! **A stub until M12.16**, which owns this file: [`Tool::open_named`] opens the panel of any
//! parameter effect the catalogue lists (a one-click effect has no panel: it is
//! `edit.effect.apply`, [`super::effects`]) with every value at its catalogue default, Reset goes
//! back to them, Cancel and Esc close it and record nothing, and Apply refuses with "Not available
//! yet.". M12.16 adds the live preview (the edit view's candidate recipe), Apply as one step of the
//! photo's history and the actions `set`, `pick` and `reset`; the crosshair ([`crate::panels::edit::crosshair`],
//! M12.17) and Soften's brush ([`crate::panels::edit::soften_brush`], M12.18) change the draft
//! through it.

use dalica_edit::catalog::{Kind, OpInfo};
use dalica_engine::CmdError;
use serde_json::Value;

use super::effects;
use super::tools::Tool;
use crate::app::App;
use crate::state::{EditState, EffectDraft, EffectValue, ToolKind, effect_row};

/// What a stub's Apply answers.
pub const NOT_YET: &str = "Not available yet.";

/// The effect tool.
pub struct Effect;

pub static EFFECT: Effect = Effect;

/// The parameter values of `row` at their catalogue defaults; a parameter whose default is still
/// open (M12.1 settles it) is left out and reads as its default when it is settled.
pub fn defaults(row: &OpInfo) -> std::collections::BTreeMap<String, EffectValue> {
    row.params
        .iter()
        .filter_map(|p| {
            let value = if let Kind::Flag = p.kind {
                EffectValue::Flag(matches!(p.default, "1" | "true" | "on"))
            } else if let Kind::Number { .. } | Kind::Open = p.kind {
                EffectValue::Number(p.default.parse::<f64>().ok().filter(|v| v.is_finite())?)
            } else if let Kind::Colour { .. } | Kind::Choice(_) = p.kind {
                (!p.default.is_empty()).then(|| EffectValue::Text(p.default.into()))?
            } else {
                return None;
            };
            Some((p.name.to_string(), value))
        })
        .collect()
}

/// A fresh panel on effect `name`: refused (`bad_params`) when `name` is no effect or a one-click
/// one.
pub fn draft(name: &str) -> Result<EffectDraft, CmdError> {
    let shown: String = name.chars().take(40).collect();
    let row = effect_row(name).ok_or_else(|| CmdError::BadParams(format!("`name` is an effect of tabs 3–5 (\"sharpen\"), not `{shown}`")))?;
    if effects::one_click(row) {
        return Err(CmdError::BadParams(format!("{} has no panel: it applies with one click (edit.effect.apply)", row.label)));
    }
    let mut d = EffectDraft::new(name);
    d.params = defaults(row);
    Ok(d)
}

impl Tool for Effect {
    fn kind(&self) -> ToolKind {
        ToolKind::Effect
    }

    fn label(&self) -> &'static str {
        "the effect"
    }

    fn open(&self, _app: &App, _edit: &mut EditState) -> Result<(), CmdError> {
        Err(CmdError::BadParams("`name` names the effect to open (\"sharpen\")".into()))
    }

    fn open_named(&self, _app: &App, edit: &mut EditState, name: Option<&str>) -> Result<(), CmdError> {
        let Some(name) = name else { return self.open(_app, edit) };
        edit.effect = Some(draft(name)?);
        Ok(())
    }

    fn apply(&self, _app: &mut App) -> Result<Value, CmdError> {
        Err(CmdError::Disabled(NOT_YET.into()))
    }

    fn reset(&self, _app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        let name = edit.effect.as_ref().map(|e| e.name.clone()).ok_or_else(|| CmdError::Disabled("No effect is open.".into()))?;
        edit.effect = Some(draft(&name)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_starts_at_the_catalogue_defaults_and_one_click_effects_have_none() {
        let d = draft("sharpen").unwrap();
        assert_eq!(d.params.get("a"), Some(&EffectValue::Number(0.6)));
        assert_eq!(d.check(), Ok(()));
        let grain = draft("grain").unwrap();
        assert_eq!(grain.params.get("lighten"), Some(&EffectValue::Flag(false)));
        assert!(matches!(draft("sepia"), Err(CmdError::BadParams(m)) if m.contains("one click")));
        for bad in ["", "crop", "autofix", "nope", &"x".repeat(5000)] {
            assert!(matches!(draft(bad), Err(CmdError::BadParams(_))), "{bad:.20}");
        }
    }
}
