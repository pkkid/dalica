//! The crosshair on the photo (`editing.md` §8.1): while Soft Focus, Focal B&W, Graduated Tint or
//! Focal Zoom's panel is open, a draggable crosshair (`crosshair:effect`) over the photo sets the
//! effect's centre (and Graduated Tint's start and angle); the drag under way is
//! [`crate::state::EffectDraft::drag`]. Run after the canvas (`panels/edit.rs`), so it works with
//! the edit panel hidden too.
//!
//! **A stub until M12.17**, which owns this file: it draws nothing.

use egui::Ui;

use crate::app::App;

/// The crosshair's automation id.
pub const ID: &str = "crosshair:effect";

/// The photo area's part of an open crosshair effect.
pub fn on_canvas(app: &mut App, ui: &mut Ui) {
    let _ = (app, ui);
}
