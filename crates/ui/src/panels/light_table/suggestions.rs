//! A person section's suggestion strip (`organizing.md` §6.3): "Suggestions:" with [Confirm All],
//! [Confirm], [Reject], [Find More], the toggle "Show only suggestions" and the hint "Click ✓ to
//! confirm a match or × to reject it.", under the section's header while the person has pending
//! suggestions; ✓ and × on each suggested face.
//!
//! M13.4's stub: no strip ([`height`] is 0) until M13.30 draws it.

use egui::Ui;

use crate::app::App;
use crate::state::Section;

/// The strip's height under `section`'s header: none until suggestions arrive.
pub fn height(_app: &App, _section: Section) -> f32 {
    0.0
}

/// Draws the strip of `section` into `ui`'s rect.
pub fn show(_app: &mut App, _ui: &mut Ui, _section: Section) {}
