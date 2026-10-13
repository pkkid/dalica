//! The menu bar (`menus-and-shortcuts.md` §1): on Windows and Linux the title bar
//! ([`crate::titlebar`], `design-system.md` §11.1) around the in-window menus of the menu model
//! ([`crate::menus::show_bar`]), every menu bounded by the window.

use egui::Ui;

use super::Panels;
use crate::app::App;

pub const HEIGHT: f32 = crate::titlebar::HEIGHT;

pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    // The region's whole row (the ui given here is inset for the menus): the caption buttons sit
    // flush in its corner.
    let bar = ui.clip_rect();
    let section = app.state().folder.and_then(|f| p.sections.list.iter().find(|s| s.folder == f)).map(|s| s.title.clone());
    crate::titlebar::show(ui, app, bar, section.as_deref());
}
