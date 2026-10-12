//! The first-run panel (`organizing.md` §1): shown once for a new library with no watched folders
//! (`Session::is_new_library`), never in `--demo`.
//!
//! **A stub on the dialog host until M5.18**, which fills this module and opens the panel when a
//! new library starts (from its own code; the host needs no change): the two choices (the standard
//! places as Scan Always, or the Folder Manager) and Continue, which adds the places or opens the
//! Folder Manager. The seams are the host's three functions: [`look`] (title, width ≈ 640, the
//! default button Continue), [`body`] (draws the draft in [`Dialog::fields`], never blocking a
//! frame) and [`act`] (Continue). Until then Continue only closes it.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, Look};
use crate::app::App;

/// The panel's width (points, `organizing.md` §1: ≈ 640 × 460).
pub const WIDTH: f32 = 640.0;

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: "Welcome to Dalica".into(), width: WIDTH, buttons: vec![Button::new("continue", "Continue", ButtonKind::Default)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    ui.label(RichText::new("Dalica shows the photos in the folders it watches.").font(fonts::regular(t.body)).color(t.text));
}

/// Continue: nothing to do yet (M5.18 adds the standard places or opens the Folder Manager).
pub fn act(_app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    Ok(json!({}))
}
