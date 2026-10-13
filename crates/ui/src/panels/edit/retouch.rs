//! The Retouch tool (`editing.md` §6.4): the "Retouch" panel (brush size, Undo Patch, Redo Patch,
//! Reset, Apply, Cancel) and the patches made on the photo.
//!
//! A stub until M7.22: the tool opens with no patches and the default brush, resets and cancels;
//! Apply refuses until the retouch operation exists; its panel draws nothing yet.

use dalica_engine::CmdError;
use egui::Ui;
use serde_json::Value;

use crate::app::App;
use crate::commands::tools::Tool;
use crate::state::{EditState, RetouchDraft, ToolKind};

pub struct Retouch;

pub static RETOUCH: Retouch = Retouch;

impl Tool for Retouch {
    fn kind(&self) -> ToolKind {
        ToolKind::Retouch
    }

    fn label(&self) -> &'static str {
        "Retouch"
    }

    fn open(&self, _app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        edit.retouch = Some(RetouchDraft::default());
        Ok(())
    }

    fn apply(&self, _app: &mut App) -> Result<Value, CmdError> {
        Err(CmdError::Disabled("Not available yet.".into()))
    }
}

/// Draws the Retouch panel in place of the tab's content.
pub fn show(_app: &mut App, _ui: &mut Ui) {}
