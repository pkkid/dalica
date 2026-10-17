//! After an import with "Delete copied files from the card" (`organizing.md` §3): "Remove the imported files from the card? This cannot be undone." [Remove Imported Files] [Keep Files]; Remove runs `import.removeFromSource {confirm: true}` for the last import. Keep Files is its Cancel button (Esc).
//!
//! M11.2's stub: its title, its buttons and its draft (`dialogs[].fields`: `count`) are final; the
//! body shows the draft as text, and its buttons other than Cancel say what they wait for.
//! M11.20 draws it and makes its buttons act.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Remove from Card";
/// The question.
pub const TEXT: &str = "Remove the imported files from the card? This cannot be undone.";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;
/// Why its buttons do not act yet.
pub const NOT_YET: &str = "Arrives with importing.";

/// A dialog of this kind with an empty draft.
pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::RemoveFromCard)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: TITLE.into(),
        width: WIDTH,
        buttons: vec![Button::new("remove", "Remove Imported Files", ButtonKind::Other), Button::new("cancel", "Keep Files", ButtonKind::Cancel)],
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::text_lines(app, ui, &[TEXT.to_string()]);
    super::draft_lines(app, ui, d, &["count"]);
}

pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "remove" => Err(CmdError::Disabled(NOT_YET.into())),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
