//! The photo area (`editing.md` §2–§3): the photo fitted with 24 pt padding on the canvas
//! background, zoom and pan, the read-out and the navigator while zoomed.
//!
//! A stub until M7.15: it registers the photo area as `canvas:edit` and leaves the canvas
//! background the region painted.

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::Ui;

use super::{navigator, zoom};
use crate::app::App;

/// The photo area's automation id.
pub const ID: &str = "canvas:edit";

pub fn show(app: &mut App, ui: &mut Ui) {
    let rect = ui.max_rect();
    widgets::register(ui.ctx(), ID, Role::Other, "Photo", Target::Painted { rect, clip: ui.clip_rect() }, false);
    zoom::show(app, ui);
    navigator::show(app, ui);
}
