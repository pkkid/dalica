//! Options ▸ Photo Viewer (`viewing.md` §3.3, `menus-and-shortcuts.md` §5 Photo Viewer): the File Types (drawn disabled, "Arrives with OS integration") and the Interface options (the mouse wheel, hiding the controls, the translucent background).
//!
//! M11.2's stub: the tab and its place in the dialog; the body says what arrives. M11.17 draws
//! its rows from the engine's options catalogue (M11.1) and puts changes into the draft.

use dalica_ui_kit::fonts;
use egui::{RichText, Ui};

use crate::app::App;

/// The stub's line.
pub const STUB: &str = "The photo viewer's options arrive with the photo viewer.";

/// Draws the tab's body.
pub fn body(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    ui.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
}
