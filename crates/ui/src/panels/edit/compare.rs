//! Compare in the edit view (`editing.md` §4): A│B's and A│A's second side beside the first (or
//! under it), the focused side's "Selected" pill, and before/after.
//!
//! M11.2's stub: nothing is drawn (A│B and A│A are not offered yet, `crate::commands::compare`).
//! M11.16 splits the photo area into the two sides and draws them.

use egui::Ui;

use crate::app::App;

/// Draws the compare modes' parts over the photo area (`ui`'s rect, the `canvas` region).
pub fn show(_app: &mut App, _ui: &mut Ui) {}
