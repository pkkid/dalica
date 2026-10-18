//! The filter banner (`ui-layout.md` §9, `organizing.md` §8.1): in search mode, 32 high across the
//! top of the light table and its scroll bar ([`super::layout::Layout::banner`]), `ui-kit`'s
//! [`widgets::filter_banner`]: [Show All] at its left (`button:search.showAll`, which leaves search
//! mode and brings the scroll position back), and what is shown after it ([`text`]): "Showing 33
//! pictures in 4 folders (0.01 s) · 439.8 MB", "Searching…" until the first answer, or in Find
//! Duplicates' mode "12 duplicate groups (31 files)"; while the light table shows the photos of the
//! people selected in the People pane, "Showing photos with Ana and Rui" (`organizing.md` §6.4,
//! [`crate::commands::people_pane::banner`]), whose Show All also clears that selection. The
//! banner itself is `banner:search`.

use dalica_engine::Session;
use dalica_engine::search::SearchMode;
use dalica_ui_kit::widgets;
use egui::Ui;

use crate::app::App;
use crate::status_text;

/// Automation ids (`control-protocol.md` §7).
pub const BANNER_ID: &str = "banner:search";
pub const SHOW_ALL_ID: &str = "button:search.showAll";

/// What the banner says until the first answer of a search arrives.
pub const SEARCHING: &str = "Searching…";

/// `n` and the word for one or many: `1 picture`, `33 pictures`.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The banner's line for a search's numbers: "Showing 33 pictures in 4 folders (0.01 s) ·
/// 439.8 MB".
pub fn line(matches: usize, folders: usize, seconds: f64, bytes: u64) -> String {
    let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    format!(
        "Showing {} in {} ({seconds:.2} s) · {}",
        count(matches, "picture", "pictures"),
        count(folders, "folder", "folders"),
        status_text::size(bytes)
    )
}

/// What the banner says for `session`'s search: the numbers of its last answer, [`SEARCHING`]
/// before the first one, or Find Duplicates' groups and files.
pub fn text(session: &Session) -> String {
    let s = session.search_state();
    match (s.mode, s.result.as_ref()) {
        (Some(SearchMode::Duplicates), Some(r)) => dalica_engine::duplicates::banner(r.groups.len(), r.count()),
        (Some(SearchMode::Search), Some(r)) => line(r.count(), r.folders, r.seconds, r.bytes),
        _ => SEARCHING.to_string(),
    }
}

/// Paints the banner into `ui`'s rect; Show All leaves search mode (and drops the people the
/// People pane filters by).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let people = crate::commands::people_pane::banner(app);
    let line = people.unwrap_or_else(|| app.session().map_or_else(|| SEARCHING.to_string(), text));
    let out = widgets::filter_banner(ui, &t, BANNER_ID, SHOW_ALL_ID, &line);
    if out.show_all.clicked() {
        crate::commands::search::clear(app);
        app.state_mut().people_pane.selected.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_counts_in_words() {
        assert_eq!(line(33, 4, 0.0123, 439_800_000), "Showing 33 pictures in 4 folders (0.01 s) · 439.8 MB");
        assert_eq!(line(1, 1, 0.5, 900), "Showing 1 picture in 1 folder (0.50 s) · 900 bytes");
        assert_eq!(line(0, 0, f64::NAN, 0), "Showing 0 pictures in 0 folders (0.00 s) · 0 bytes");
    }
}
