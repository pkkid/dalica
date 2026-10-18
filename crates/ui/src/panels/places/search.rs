//! "Search for a place:" (`organizing.md` §7): the field `field:places.search` at the bottom of the
//! Places pane; Enter runs `map.search` (typed coordinates found without the network, a name sent
//! to the geocoding service), the results listed under it, a click moving the pin there.
//!
//! M13.4's stub: its place in the pane; it draws nothing until M13.28.

use egui::Ui;

use crate::app::App;

/// Draws the search part of the Places pane.
pub fn show(_app: &mut App, _ui: &mut Ui) {}
