//! The effect tabs' tile grids (`editing.md` §8.1–§8.4): tabs 3–5 of the edit panel (`effects`,
//! `effects2`, `effects3`), each a fixed grid of three columns and at most four rows of tiles
//! (`effect:<name>`), a click applying a one-click effect (`edit.effect.apply`) or opening a
//! parameter effect's panel (`edit.tool.open {tool: "effect", name}`, [`super::effect_panel`]).
//!
//! **A stub until M12.15**, which owns this file: each tab shows the empty state "No effects yet"
//! (`empty:edit.effects`).

use dalica_ui_kit::widgets;
use egui::Ui;

use crate::app::App;
use crate::state::EditTab;

/// The effect tabs' empty state until their grids.
pub const EMPTY_ID: &str = "empty:edit.effects";
pub const EMPTY_TITLE: &str = "No effects yet";
pub const EMPTY_BODY: &str = "Effects come in a later version of Dalica.";

/// Draws effect tab `tab`'s grid in `ui`'s rect (disabled while a tool is open: the caller
/// disables `ui`).
pub fn show(app: &mut App, ui: &mut Ui, tab: EditTab) {
    let _ = tab;
    let t = *app.tokens();
    widgets::empty_state(ui, &t, EMPTY_ID, EMPTY_TITLE, EMPTY_BODY, None);
}
