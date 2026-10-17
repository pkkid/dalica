//! Options ▸ Slideshow (`viewing.md` §1.5, `menus-and-shortcuts.md` §5 Slideshow): Loop slideshow, Display time, Transition, Show captions, and Play music and Play video sound (drawn disabled with their reasons).
//!
//! M11.2's stub: the tab and its place in the dialog; the body says what arrives. M11.21 draws
//! its rows from the engine's options catalogue (M11.1) and puts changes into the draft.

use dalica_ui_kit::fonts;
use egui::{RichText, Ui};

use crate::app::App;

/// The stub's line.
pub const STUB: &str = "The slideshow's options arrive with the slideshow.";

/// Draws the tab's body.
pub fn body(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    ui.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
}
