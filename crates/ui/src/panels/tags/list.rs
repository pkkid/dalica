//! The Tags pane's "Tags in the selection:" (`organizing.md` §5.2): every tag of the selected
//! photos, with partial counts ("beach (3 of 8)"), × on hover and the tag menu (`tag:<tag text>`).
//!
//! For now it shows its caption and "None"; the rows (from `tags.list`) arrive later in this
//! milestone.

use dalica_ui_kit::fonts;
use egui::{RichText, Ui};

use crate::app::App;

pub fn show(app: &mut App, ui: &mut Ui) {
    super::caption(app, ui, "Tags in the selection:");
    let t = app.tokens();
    ui.label(RichText::new("None").font(fonts::regular(t.body)).color(t.text_dim));
}
