//! The Crop tool (`editing.md` §6.1): the "Crop Photo" panel, the size dropdown, the suggestions,
//! and the rectangle drawn on the photo.
//!
//! A stub until M7.19: the tool opens with a Manual draft and no rectangle, resets and cancels it;
//! Enter applies (and Apply refuses until the crop command exists); its panel draws nothing yet.

use dalica_engine::{CmdError, Key};
use egui::Ui;
use serde_json::{Value, json};

use crate::app::App;
use crate::commands::tools::Tool;
use crate::state::{CropDraft, EditState, ToolKind};

pub struct Crop;

pub static CROP: Crop = Crop;

impl Tool for Crop {
    fn kind(&self) -> ToolKind {
        ToolKind::Crop
    }

    fn label(&self) -> &'static str {
        "Crop"
    }

    fn open(&self, _app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        edit.crop = Some(CropDraft::default());
        Ok(())
    }

    fn apply(&self, _app: &mut App) -> Result<Value, CmdError> {
        Err(CmdError::Disabled("Not available yet.".into()))
    }

    /// Enter applies the crop (`editing.md` §6.1).
    fn key(&self, app: &mut App, key: &Key) -> Option<Result<Value, CmdError>> {
        (key.to_string() == "Enter").then(|| app.run("edit.tool.apply", &json!({})))
    }
}

/// Draws the Crop panel in place of the tab's content.
pub fn show(_app: &mut App, _ui: &mut Ui) {}
