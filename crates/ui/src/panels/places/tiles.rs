//! The map tiles over the offline layer (`organizing.md` §7): the pane's one-time question "Show map
//! tiles from %provider%? Dalica fetches map pictures from the internet while this pane is open."
//! [Allow] [Not Now] (`map.allowTiles`), then the tiles the session fetched for the visible box with
//! the provider's attribution.
//!
//! M13.4's stub: its place in the pane; it draws nothing and fetches nothing until M13.28.

use egui::Ui;

use crate::app::App;

/// Draws the tiles part of the Places pane.
pub fn show(_app: &mut App, _ui: &mut Ui) {}
