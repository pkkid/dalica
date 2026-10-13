//! Tab 2, Lighting and color tuning (`editing.md` §7): Fill Light, Highlights, Shadows, Color
//! Temperature, the Neutral Color Picker and the two wands. Moving its sliders is a Tuning session,
//! one history step when it ends.
//!
//! A stub until M7.21: the session opens at the defaults (the photo's own Tuning values join with
//! the engine's Tuning commands), resets and cancels; Apply refuses until then; the tab draws
//! nothing yet. A session leaves the rest of the panel enabled.

use dalica_engine::CmdError;
use egui::Ui;
use serde_json::Value;

use crate::app::App;
use crate::commands::tools::Tool;
use crate::state::{EditState, ToolKind, TuningDraft};

pub struct Tuning;

pub static TUNING: Tuning = Tuning;

impl Tool for Tuning {
    fn kind(&self) -> ToolKind {
        ToolKind::Tuning
    }

    fn label(&self) -> &'static str {
        "Tuning"
    }

    fn modal(&self) -> bool {
        false
    }

    fn open(&self, _app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        edit.tuning = Some(TuningDraft::default());
        Ok(())
    }

    fn apply(&self, _app: &mut App) -> Result<Value, CmdError> {
        Err(CmdError::Disabled("Not available yet.".into()))
    }
}

/// Draws the Tuning tab's content.
pub fn show(_app: &mut App, _ui: &mut Ui) {}
