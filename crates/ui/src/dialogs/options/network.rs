//! Options ▸ Network (`menus-and-shortcuts.md` §5 Network, `organizing.md` §7): Allow map tiles, the
//! map tile server (a URL template, with its attribution), the geocoding service and the proxy
//! (system or manual: host and port), as reserved decision 4 leaves them.
//!
//! M13.4's stub: the tab and its place in the dialog; the body says what arrives. M13.28 draws its
//! rows from the engine's options catalogue (M13.3) and puts changes into the draft.

use dalica_ui_kit::fonts;
use egui::{RichText, Ui};

use crate::app::App;

/// The stub's line.
pub const STUB: &str = "The network options arrive with the Places pane's map.";

/// Draws the tab's body.
pub fn body(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    ui.label(RichText::new(STUB).font(fonts::regular(t.body)).color(t.text_dim));
}
