//! The status line and the bottom bar (`ui-layout.md` §6). M1: the status line (counts, or the last
//! refusal) and the star button.

use dalica_ui_kit::{fonts, widgets};
use egui::{Align2, Ui, vec2};
use serde_json::json;

use super::Panels;
use crate::app::App;

pub const HEIGHT: f32 = 96.0;
pub const STATUS_HEIGHT: f32 = 20.0;

pub fn status(p: &Panels, app: &App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let text = if !app.state().status.is_empty() {
        app.state().status.clone()
    } else {
        let photos: usize = p.sections.list.iter().map(|s| s.photos.len()).sum();
        let selected = app.state().selection.ids.len();
        match selected {
            0 => format!("{photos} pictures"),
            n => format!("{n} of {photos} pictures selected"),
        }
    };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, fonts::regular(t.small), t.text_dim);
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ids = app.state().selection.ids.clone();
    let on = !ids.is_empty() && app.session().is_some_and(|s| ids.iter().all(|&id| s.library().photo(id).is_some_and(|p| p.star)));
    let area = ui.max_rect();
    let mut child = ui
        .new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(area.min + vec2(0.365 * area.width() + 12.0, 8.0), vec2(200.0, 24.0))));
    child.add_enabled_ui(!ids.is_empty(), |ui| {
        if widgets::star_toggle(ui, &t, "button:star", on).clicked() {
            let _ = app.run("photo.star.toggle", &json!({ "ids": ids }));
        }
    });
}
