//! The Unnamed grid (`organizing.md` §6.2): while Unnamed is selected in the People group
//! (`unnamed.open`) it fills the light table's region in its place: the header "Unnamed people"
//! with [Expand Groups] (`button:faces.expandGroups`) and [Show Ignored Faces]
//! (`button:faces.showIgnored`), then the groups of similar faces, largest first (`group:<n>`), each
//! with its face tiles (`face:<photo>.<n>`), its "Add a name" field (`field:faces.name/<n>`) and
//! its × (`group:<n>/ignore`); faces are selected by click, Ctrl / Shift-click and a rubber band,
//! dragged onto a person, and open the face menu (`crate::menus::face_menu`).
//!
//! M13.4's stub: the header and its two toggles (live: `faces.expandGroups`,
//! `faces.showIgnored`) and a line saying what arrives; [`suggestions`] is the "Name?" marks'
//! place (M13.30). M13.21 draws the groups.

pub mod suggestions;

use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets;
use egui::{Align, Layout, RichText, Ui, UiBuilder};

use crate::app::App;

/// The header's title.
pub const TITLE: &str = "Unnamed people";
/// The stub's line.
pub const STUB: &str = "Faces appear here once face recognition finds them.";
/// The header's toggles: their widget ids, labels and commands.
pub const TOGGLES: [(&str, &str, &str); 2] =
    [("button:faces.expandGroups", "Expand Groups", "faces.expandGroups"), ("button:faces.showIgnored", "Show Ignored Faces", "faces.showIgnored")];

/// Draws the grid into `ui`'s rect (the light table's region, its scroll bar's included).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect().shrink(16.0);
    let mut col = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    let unnamed = app.state().unnamed.clone();
    let mut clicked = None;
    col.horizontal(|ui| {
        ui.label(RichText::new(TITLE).font(fonts::semibold(t.title)).color(t.text));
        ui.add_space(16.0);
        for ((id, label, command), on) in TOGGLES.iter().zip([unnamed.expanded, unnamed.show_ignored]) {
            if widgets::text_button(ui, &t, id, label, on).clicked() {
                clicked = Some(*command);
            }
        }
    });
    col.add_space(12.0);
    col.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
    suggestions::show(app, &mut col);
    if let Some(command) = clicked {
        let _ = app.run(command, &serde_json::json!({}));
    }
}
