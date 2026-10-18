//! The person menu's dialogs (`organizing.md` §6.3), opened by the person menu's rows
//! (`crate::commands::people`) on a person's tree row or section header:
//!
//! - **Rename…**: Name and Nickname, [OK] [Cancel]; OK runs `people.rename {person, name,
//!   nickname}`. A name another person has (ignoring case) asks to merge instead.
//! - **Merge Into ▸ a person**: "Merge %s into %s? Their faces become %s's." [Merge] [Cancel];
//!   Merge runs `people.merge {person, into}` (the target's name is kept).
//! - **Delete Person**: "Delete %s? Their faces go back to Unnamed." [Delete] [Cancel]; Delete runs
//!   `people.delete {person, confirm: true}`.
//!
//! Each is one library undo step. M13.4's stubs: their titles, buttons and drafts
//! (`dialogs[].fields`: `person`, and Rename's `name` and `nickname`, Merge's `into`) are final; the
//! bodies show the draft as text, and their default buttons say what they wait for. M13.25 draws
//! them and makes them act.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const RENAME_TITLE: &str = "Rename Person";
pub const MERGE_TITLE: &str = "Merge People";
pub const DELETE_TITLE: &str = "Delete Person";
/// The questions, until the people's names fill them in.
pub const MERGE_TEXT: &str = "Merge this person into the other? Their faces become the other person's.";
pub const DELETE_TEXT: &str = "Delete this person? Their faces go back to Unnamed.";
/// The dialogs' width (points).
pub const WIDTH: f32 = 420.0;
/// Why their buttons do not act yet.
pub const NOT_YET: &str = "Arrives with the People group.";

/// A dialog of `kind` (one of this module's) with `draft`.
pub fn dialog(kind: DialogKind, draft: Map<String, Value>) -> Dialog {
    let mut d = Dialog::new(kind);
    d.fields = draft;
    d
}

/// The default button of each kind: its action and label.
fn default_button(kind: DialogKind) -> (&'static str, &'static str) {
    match kind {
        DialogKind::MergePeople => ("merge", "Merge"),
        DialogKind::DeletePerson => ("delete", "Delete"),
        _ => ("ok", "OK"),
    }
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let title = match d.kind {
        DialogKind::MergePeople => MERGE_TITLE,
        DialogKind::DeletePerson => DELETE_TITLE,
        _ => RENAME_TITLE,
    };
    let (action, label) = default_button(d.kind);
    Look { title: title.into(), width: WIDTH, buttons: vec![Button::new(action, label, ButtonKind::Default), Button::cancel()] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    match d.kind {
        DialogKind::MergePeople => {
            super::text_lines(app, ui, &[MERGE_TEXT.to_string()]);
            super::draft_lines(app, ui, d, &["person", "into"]);
        }
        DialogKind::DeletePerson => {
            super::text_lines(app, ui, &[DELETE_TEXT.to_string()]);
            super::draft_lines(app, ui, d, &["person"]);
        }
        _ => super::draft_lines(app, ui, d, &["person", "name", "nickname"]),
    }
}

pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    if action == default_button(d.kind).0 {
        return Err(CmdError::Disabled(NOT_YET.into()));
    }
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}
