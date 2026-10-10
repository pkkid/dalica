//! The menu bar (`menus-and-shortcuts.md` §1): the in-window menus of the menu model
//! ([`crate::menus::show_bar`]), every menu bounded by the window. M3.6 builds the title bar
//! (`design-system.md` §11.1) around it.

use egui::Ui;

use super::Panels;
use crate::app::App;

pub const HEIGHT: f32 = 24.0;

pub fn show(_p: &mut Panels, app: &mut App, ui: &mut Ui) {
    crate::menus::show_bar(ui, app);
}
