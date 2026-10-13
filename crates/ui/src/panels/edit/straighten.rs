//! The Straighten tool (`editing.md` §6.2): the dashed grid over the photo and the floating bar
//! with its slider, Apply and Cancel.
//!
//! A stub until M7.20: the tool opens at 0°, resets and cancels; Apply refuses until the
//! straighten command exists; nothing is drawn yet.

use dalica_engine::CmdError;
use egui::Ui;
use serde_json::Value;

use crate::app::App;
use crate::commands::tools::Tool;
use crate::state::{EditState, StraightenDraft, ToolKind};

pub struct Straighten;

pub static STRAIGHTEN: Straighten = Straighten;

impl Tool for Straighten {
    fn kind(&self) -> ToolKind {
        ToolKind::Straighten
    }

    fn label(&self) -> &'static str {
        "Straighten"
    }

    fn open(&self, _app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        edit.straighten = Some(StraightenDraft::default());
        Ok(())
    }

    fn apply(&self, _app: &mut App) -> Result<Value, CmdError> {
        Err(CmdError::Disabled("Not available yet.".into()))
    }
}

/// Draws the grid and the floating bar over the photo area.
pub fn show(_app: &mut App, _ui: &mut Ui) {}
