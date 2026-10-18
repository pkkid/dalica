//! Soften's brush and eraser on the photo (`editing.md` §8.4): while Soften's panel is open, the
//! brush circle under the pointer, a drag adding a stroke ([`crate::state::EffectDraft::strokes`],
//! Paint or Erase as [`crate::state::EffectDraft::erase`] says, at most
//! [`crate::state::MAX_STROKES`]). Run after the canvas (`panels/edit.rs`), so it works with the edit
//! panel hidden too.
//!
//! **A stub until M12.18**, which owns this file: it draws nothing.

use egui::Ui;

use crate::app::App;

/// The photo area's part of an open Soften panel.
pub fn on_canvas(app: &mut App, ui: &mut Ui) {
    let _ = (app, ui);
}
