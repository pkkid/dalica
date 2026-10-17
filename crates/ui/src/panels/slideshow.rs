//! The slideshow (`viewing.md` §1): the photo shown on pure black, filling the window, its
//! transitions, its caption and its control bar.
//!
//! M11.2's stub: the window on `viewer_bg` (pure black) with a line naming the photo shown and its
//! place in the show. M11.19 paints the photos and their transitions (`slideshow/show.rs`,
//! `slideshow/transition.rs`), M11.21 the bar and the zoom (`slideshow/bar.rs`,
//! `slideshow/zoom.rs`).

use dalica_ui_kit::{fonts, widgets};
use egui::{Align2, Ui};

use crate::app::App;

/// Draws the slideshow over `ui`'s whole rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.viewer_bg);
    let Some(s) = app.state().slideshow.as_ref() else { return };
    let line = match s.photo() {
        Some(p) => format!("Slideshow: photo {p}, {} of {}", s.index.saturating_add(1), s.photos.len()),
        None => "Slideshow".to_string(),
    };
    widgets::label_in(ui.painter(), rect, Align2::CENTER_CENTER, &line, fonts::regular(t.body), t.text_dim);
}
