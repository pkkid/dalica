//! The quick full-screen preview (`viewing.md` §2): while Ctrl+Alt is held, the photo fills the
//! window on black, over whatever is shown; nothing else is drawn.
//!
//! M11.2's stub: the window covered with `viewer_bg` and a line naming the photo. M11.14 paints the
//! photo from the best thumbnail in the cache, refined in place.

use dalica_ui_kit::{fonts, widgets};
use egui::{Align2, Ui};

use crate::app::App;

/// Draws the preview over `ui`'s whole rect, on its own layer above the panels.
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(photo) = app.state().peek else { return };
    let t = *app.tokens();
    let rect = ui.max_rect();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("peek")));
    painter.rect_filled(rect, 0, t.viewer_bg);
    widgets::label_in(&painter, rect, Align2::CENTER_CENTER, &format!("Photo {photo}"), fonts::regular(t.body), t.text_dim);
}
