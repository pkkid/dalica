//! The Places pane (`organizing.md` §7), in the side pane: the header's status ("No places in the
//! selection" / "9 places in the selection"), the map of the photos of the current folder, album
//! or selection (`map:places`: the offline layer, tiles once allowed, markers `marker:<n>` with
//! their counts and bubbles `popup:places.bubble`, zoom + / −, the pin tool `button:places.pin`),
//! photos dropped from the light table to geotag them, and "Search for a place:"
//! (`field:places.search`). Its body is `pane:places`; M13.22 registers it as the region
//! `region:places` too (`ui.screenshot {region: "places"}`). While it is open the session is told so (`Session::set_places_open`), the only time tiles may be fetched.
//!
//! M13.4's stub: the status line and a line saying what arrives; [`tiles`] (M13.28) and [`search`]
//! (M13.28) are the tiles' and the search field's places. M13.22 draws the pane and its map.

pub mod search;
pub mod tiles;

use dalica_ui_kit::fonts;
use egui::{Align, Layout, RichText, Ui, UiBuilder};

use crate::app::App;

/// The status line with nothing geotagged.
pub const NONE: &str = "No places in the selection";
/// The stub's line.
pub const STUB: &str = "The map arrives with geotags.";

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect().shrink(8.0);
    let mut col = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    col.label(RichText::new(NONE).font(fonts::regular(t.body)).color(t.text_label));
    col.add_space(12.0);
    col.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
    tiles::show(app, &mut col);
    search::show(app, &mut col);
}
