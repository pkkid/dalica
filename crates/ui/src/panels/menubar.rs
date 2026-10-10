//! The menu bar (`menus-and-shortcuts.md`). M1 has File ▸ Add Folder…; the full menu tree comes from
//! the menu catalog later.

use dalica_ui_kit::registry::{self, Role, WidgetInfo};
use egui::Ui;

use super::Panels;
use crate::app::App;

pub const HEIGHT: f32 = 24.0;

pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let can_add = app.session().is_some() && p.picker.is_some() && p.dialog.is_none();
    egui::MenuBar::new().ui(ui, |ui| {
        let file = ui.menu_button("File", |ui| {
            let item = ui.add_enabled(can_add, egui::Button::new("Add Folder…"));
            registry::register(ui.ctx(), WidgetInfo::new("menu.file.addFolder", Role::Button, "Add Folder…", item.rect, can_add, false));
            if item.clicked() {
                p.add_folder();
                ui.close();
            }
        });
        registry::register(ui.ctx(), WidgetInfo::new("menu.file", Role::Button, "File", file.response.rect, true, false));
    });
}
