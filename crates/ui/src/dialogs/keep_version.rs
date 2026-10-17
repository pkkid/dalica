//! "Keep which version?" (`editing.md` §4): asked when leaving A│A with sides that differ, [Left] [Right] (or [Top] [Bottom] stacked) [Cancel] with "Don't ask again"; the side kept becomes one step of the photo's history (`edit.recipe.set`).
//!
//! M11.2's stub: its title, its buttons and its draft (`dialogs[].fields`: `dontAsk`) are final; the
//! body shows the draft as text, and its buttons other than Cancel say what they wait for.
//! M11.16 draws it and makes its buttons act.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Keep Which Version?";
/// The question.
pub const TEXT: &str = "Keep which version of this photo?";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;
/// Why its buttons do not act yet.
pub const NOT_YET: &str = "Arrives with compare.";

/// A dialog of this kind with an empty draft.
pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::KeepVersion)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![Button::new("left", "Left", ButtonKind::Other), Button::new("right", "Right", ButtonKind::Other), Button::cancel()],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::text_lines(app, ui, &[TEXT.to_string()]);
    super::draft_lines(app, ui, d, &["dontAsk"]);
}

pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "left" | "right" => Err(CmdError::Disabled(NOT_YET.into())),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
