//! Picture ▸ Undo All Edits… (`editing.md` §9): "Remove all edits from %d pictures?" [Remove Edits] [Cancel], naming the saved photos (which keep their saved changes; Revert takes them back) when there are; Remove Edits runs `edit.undoAll` for the photos, one undo step.
//!
//! M11.2's stub: its title, its buttons and its draft (`dialogs[].fields`: `ids`) are final; the
//! body shows the draft as text, and its buttons other than Cancel say what they wait for.
//! M11.18 draws it and makes its buttons act.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Undo All Edits";
/// The question.
pub const TEXT: &str = "Remove all edits from the selected pictures?";
/// The dialog's width (points).
pub const WIDTH: f32 = 440.0;
/// Why its buttons do not act yet.
pub const NOT_YET: &str = "Arrives with Undo All Edits.";

/// A dialog of this kind with an empty draft.
pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::UndoAllEdits)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::new("removeEdits", "Remove Edits", ButtonKind::Default), Button::cancel()] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::text_lines(app, ui, &[TEXT.to_string()]);
    super::draft_lines(app, ui, d, &["ids"]);
}

pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match action {
        "removeEdits" => Err(CmdError::Disabled(NOT_YET.into())),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
