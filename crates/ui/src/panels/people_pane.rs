//! The People pane (`organizing.md` §6.4), in the side pane: "People in these photos" (the people
//! of the selection, or of the photo in the edit view, with counts; Ctrl-click selects several,
//! which filter the light table, `people.filter`) and "Unnamed faces" (each with Add a name and ×),
//! suggestions shown as "Name?" with ✓ / ×; hovering a face highlights its box on the photo in the
//! edit view and back (`peoplePane.hovered`); in the edit view Add a Person Manually
//! (`faces.addManually`). Its body is `pane:people`; M13.24 registers it as the region
//! `region:people` too (`ui.screenshot {region: "people"}`).
//!
//! M13.4's stub: the two captions and a line saying what arrives; [`suggestions`] is the "Name?"
//! marks' place (M13.30). M13.24 draws the pane.

pub mod suggestions;

use dalica_ui_kit::fonts;
use egui::{Align, Layout, RichText, Ui, UiBuilder};

use crate::app::App;

/// The pane's two parts.
pub const IN_PHOTOS: &str = "People in these photos";
pub const UNNAMED: &str = "Unnamed faces";
/// The stub's line.
pub const STUB: &str = "People appear here once face recognition finds them.";

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect().shrink(8.0);
    let mut col = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    for caption in [IN_PHOTOS, UNNAMED] {
        col.label(RichText::new(caption).font(fonts::regular(t.body)).color(t.text_label));
        col.add_space(4.0);
    }
    col.add_space(12.0);
    col.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
    suggestions::show(app, &mut col);
}
