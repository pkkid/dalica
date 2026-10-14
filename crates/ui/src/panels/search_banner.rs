//! The filter banner (`ui-layout.md` §9, `organizing.md` §8.1): in search mode, 32 high across the
//! top of the light table and its scroll bar ([`super::layout::Layout::banner`]), in the
//! `filter_banner` colour, with [Show All] at its left and "Showing 33 pictures in 4 folders
//! (0.01 s)" after it (or, in duplicates mode, "12 duplicate groups (31 files)").
//!
//! A stub until M9.22: it paints the banner's colour and what is searched for; Show All and the
//! counts arrive with the engine's search (M9.11) and M9.5's `filter_banner` widget.

use dalica_ui_kit::fonts;
use egui::{Align2, Ui, pos2};

use crate::app::App;
use crate::state::SearchState;

/// The text the stub shows for `search`: what is searched for.
pub fn text(search: &SearchState) -> String {
    let mut parts: Vec<String> = Vec::new();
    let typed = search.text.trim();
    if !typed.is_empty() {
        parts.push(format!("\"{}\"", typed.chars().take(80).collect::<String>()));
    }
    let f = &search.filters;
    for (on, name) in [(f.starred, "starred"), (f.faces, "with faces"), (f.videos, "videos"), (f.geotagged, "geotagged")] {
        if on {
            parts.push(name.into());
        }
    }
    if search.age > 0.0 {
        parts.push("by age".into());
    }
    format!("Searching: {}", parts.join(", "))
}

/// Paints the banner into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.filter_banner);
    let line = text(&app.state().search);
    ui.painter().text(pos2(rect.min.x + 12.0, rect.center().y), Align2::LEFT_CENTER, line, fonts::regular(t.body), t.text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Filters;

    #[test]
    fn the_stub_says_what_is_searched_for() {
        let s = SearchState {
            text: " lisbon ".into(),
            filters: Filters { starred: true, videos: true, ..Filters::default() },
            age: 0.3,
            ..SearchState::default()
        };
        assert_eq!(text(&s), "Searching: \"lisbon\", starred, videos, by age");
        assert_eq!(text(&SearchState::default()), "Searching: ");
    }
}
