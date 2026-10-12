//! Help ▸ Release Notes (`menus-and-shortcuts.md` §1.8): opened by `help.releaseNotes`, the
//! user-facing changelog (`whats-new.md`). It has no draft and one button, Close (Esc).
//!
//! The body is a placeholder line for now; the notes compiled into the binary replace [`body`]
//! later in this milestone.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use egui::{RichText, Ui};
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Release Notes";
/// The dialog's width (points).
pub const WIDTH: f32 = 560.0;

pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::ReleaseNotes)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::close()] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.label(RichText::new(format!("What is new in Dalica {}", env!("CARGO_PKG_VERSION"))).font(fonts::regular(t.body)).color(t.text));
}

/// The dialog has no button but Close.
pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}
