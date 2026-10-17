//! The Import room (`organizing.md` §3): "Import from:", the import tray, the preview and the
//! destination strip, in the room's region under the tab strip.
//!
//! M11.2's stub: the room's region with a line saying what arrives. M11.15 draws the sources, the
//! tray and the preview (in modules under `import/`), M11.20 the destination strip and the
//! progress.

use dalica_ui_kit::{fonts, widgets};
use egui::{Align2, Ui};

use crate::app::App;

/// The stub's line.
pub const STUB: &str = "Import: choosing a card, a drive or a folder arrives with the Import room.";

/// Draws the room into `ui`'s rect (the `room` region).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    widgets::label_in(ui.painter(), ui.max_rect(), Align2::CENTER_CENTER, STUB, fonts::regular(t.body), t.text_dim);
}
