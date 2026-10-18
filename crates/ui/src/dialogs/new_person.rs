//! The Unnamed grid's two dialogs (`organizing.md` §6.2):
//!
//! - **New Person**: Name (prefilled with what was typed), Nickname (optional), [OK] [Cancel]; asked
//!   when a name typed under a group or for a face is nobody's yet, and by the face menu's Move to
//!   New Person…. OK creates the person and names the faces (`faces.name {faces, name, create}`)
//!   as one undo step.
//! - **"Ignore these faces?"** [Ignore] [Cancel] with "Don't ask again": a group's × and Delete on
//!   selected faces. Ignore runs `faces.ignore {faces}`.
//!
//! M13.4's stubs: their titles, buttons and drafts (`dialogs[].fields`: New Person's `name`,
//! `nickname` and `faces`; the question's `faces` and `dontAsk`) are final; the bodies show the
//! draft as text, and their default buttons say what they wait for. M13.21 draws them and makes
//! them act.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::state::FaceRef;

pub const TITLE: &str = "New Person";
pub const IGNORE_TITLE: &str = "Ignore Faces";
/// The question of the ignore dialog.
pub const IGNORE_TEXT: &str = "Ignore these faces?";
/// The dialogs' width (points).
pub const WIDTH: f32 = 400.0;
/// Why their buttons do not act yet.
pub const NOT_YET: &str = "Arrives with the Unnamed faces.";

/// New Person for `faces`, its Name prefilled with `name`.
pub fn dialog(name: &str, faces: &[FaceRef]) -> Dialog {
    let mut d = Dialog::new(DialogKind::NewPerson);
    d.fields = Map::from_iter([("name".into(), json!(name)), ("nickname".into(), json!("")), ("faces".into(), json!(faces))]);
    d
}

/// "Ignore these faces?" for `faces`.
pub fn ignore_dialog(faces: &[FaceRef]) -> Dialog {
    let mut d = Dialog::new(DialogKind::IgnoreFaces);
    d.fields = Map::from_iter([("faces".into(), json!(faces)), ("dontAsk".into(), json!(false))]);
    d
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    match d.kind {
        DialogKind::IgnoreFaces => {
            Look { title: IGNORE_TITLE.into(), width: WIDTH, buttons: vec![Button::new("ignore", "Ignore", ButtonKind::Default), Button::cancel()] }
        }
        _ => Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::new("ok", "OK", ButtonKind::Default), Button::cancel()] },
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    match d.kind {
        DialogKind::IgnoreFaces => {
            super::text_lines(app, ui, &[IGNORE_TEXT.to_string()]);
            super::draft_lines(app, ui, d, &["faces", "dontAsk"]);
        }
        _ => super::draft_lines(app, ui, d, &["name", "nickname", "faces"]),
    }
}

pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    match (d.kind, action) {
        (DialogKind::NewPerson, "ok") | (DialogKind::IgnoreFaces, "ignore") => Err(CmdError::Disabled(NOT_YET.into())),
        _ => Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name()))),
    }
}
