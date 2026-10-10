//! The status line (`ui-layout.md` §6.1): 20 high, full width, between the light table and the
//! bottom area. For now it shows the photo count (or how many are selected), or the result of the
//! last refused action; the texts of §6.1 come with M3.8.

use dalica_ui_kit::fonts;
use egui::{Align2, Ui};

use super::Panels;
use crate::app::App;

/// The text the status line shows.
pub fn text(p: &Panels, app: &App) -> String {
    if !app.state().status.is_empty() {
        return app.state().status.clone();
    }
    let photos: usize = p.sections.list.iter().map(|s| s.photos.len()).sum();
    match app.state().selection.ids.len() {
        0 => format!("{photos} pictures"),
        n => format!("{n} of {photos} pictures selected"),
    }
}

/// Paints the status line into `ui`'s rect (the status-line region of [`super::layout`]).
pub fn show(p: &Panels, app: &App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.chrome);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text(p, app), fonts::regular(t.small), t.text_dim);
}
