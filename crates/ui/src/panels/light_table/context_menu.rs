//! The photo context menu of a thumbnail (`organizing.md` §9.1, `menus-and-shortcuts.md` §4): a
//! right-click on a cell selects the photo first when it is not selected
//! ([`super::input`]), then opens this menu for the selection, bounded by the window like every
//! menu (`crate::menus::context_menu`). Its rows are [`menus::photo_menu`]'s, the album variant in
//! an album section; rows whose commands do not exist yet are greyed with their reason, and
//! `ui.menu.tree {context: "photo"}` returns the same model without opening it.
//!
//! A frame: [`super::input::finish`] notes the cell whose menu was asked for or is open, and once
//! the library is no longer borrowed [`show`] draws the menu on that cell's response and runs the
//! row clicked. M9.18 fills Add to Album ▸ with the albums and draws View and Edit in bold.

use egui::Response;

use crate::app::App;
use crate::menus;

/// Draws the photo menu on `response` (a thumbnail cell's) while it is open, and runs the row
/// clicked. Returns the command id run, if any.
pub fn show(response: &Response, app: &mut App) -> Option<&'static str> {
    let nodes = menus::photo_menu(app, menus::Platform::current());
    menus::context_menu(response, app, &nodes)
}
