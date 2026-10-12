//! The Tags pane (`organizing.md` §5.2): in the side pane, top to bottom, "Add a tag:" with its
//! field and + ([`add`]), "Tags in the selection:" ([`list`]) and the quick tags with their gear
//! ([`quick`]). Each part is a module of its own, so the tasks that fill them never share a file;
//! this one only stacks them.

pub mod add;
pub mod list;
pub mod quick;

use egui::{Align, Layout, Ui, UiBuilder};

use crate::app::App;

/// The gap between the parts (points).
const GAP: f32 = 16.0;

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let rect = ui.max_rect().shrink(8.0);
    let mut col = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    add::show(app, &mut col);
    col.add_space(GAP);
    list::show(app, &mut col);
    col.add_space(GAP);
    quick::show(app, &mut col);
}

/// A part's caption ("Add a tag:").
pub(crate) fn caption(app: &App, ui: &mut Ui, text: &str) {
    let t = app.tokens();
    ui.label(egui::RichText::new(text).font(dalica_ui_kit::fonts::regular(t.body)).color(t.text_label));
    ui.add_space(4.0);
}
