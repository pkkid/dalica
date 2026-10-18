//! Options ▸ People (`organizing.md` §6.1, `faces.md` §3.1, `menus-and-shortcuts.md` §5 People):
//! Find faces in my photos, Suggest names, the Suggestion confidence and Grouping confidence
//! sliders (50 … 95 by 5), Store people names in photos, and the face model's status with
//! [Download…] / [Remove] / Use.
//!
//! M13.4's stub: the tab and its place in the dialog; the body says what arrives. M13.27 draws its
//! rows from the engine's options catalogue (M13.3) and puts changes into the draft.

use dalica_ui_kit::fonts;
use egui::{RichText, Ui};

use crate::app::App;

/// The stub's line.
pub const STUB: &str = "The people options arrive with face recognition.";

/// Draws the tab's body.
pub fn body(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    ui.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
}
