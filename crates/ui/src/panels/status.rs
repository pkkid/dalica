//! The status line (`ui-layout.md` §6.1): 20 high, full width, between the light table and the
//! bottom area, the text centred in Small `text_dim` on `chrome`. It says what is selected: "No
//! selection"; one photo's name, date, size in pixels and on disk; or how many pictures a selection
//! or the selected folder holds, their date range and their size on disk. A transient message (the
//! result of the last action, or why it was refused) replaces the text for 4 s.
//!
//! The text is rebuilt only when the library, the folder or the selection changes, and written to
//! [`UiState::status_line`](crate::state::UiState::status_line), which `ui.state` reports as
//! `status`.

use dalica_core::{Date, DateTime, FolderId, PhotoId};
use dalica_library::{Library, Photo};
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Id, Ui};

use super::Panels;
use super::light_table::long_date;
use crate::app::App;

/// The text shown when nothing is selected.
pub const NO_SELECTION: &str = "No selection";
/// How long a transient message replaces the text (seconds).
pub const MESSAGE_SECONDS: f64 = 4.0;
/// The space between two fields.
pub const SEP: &str = "     ";
/// Padding kept free at each end of the line when the text is fitted (points).
const PAD: f32 = 8.0;

/// The status line's text, as fields: `name` is the one-photo file name, the field that is
/// middle-elided first when the line is too long; `rest` the fields after it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    pub name: Option<String>,
    pub rest: Vec<String>,
}

impl Line {
    fn plain(text: impl Into<String>) -> Line {
        Line { name: None, rest: vec![text.into()] }
    }

    /// The fields joined by wide spaces.
    pub fn text(&self) -> String {
        self.name.iter().chain(self.rest.iter()).map(String::as_str).collect::<Vec<_>>().join(SEP)
    }

    /// The text with the file name cut to `keep` characters (`IMG_…jpg`).
    fn with_name_cut_to(&self, keep: usize) -> String {
        Line { name: self.name.as_deref().map(|n| elide_middle(n, keep)), rest: self.rest.clone() }.text()
    }
}

/// `s` cut to `keep` characters plus "…" in the middle (the start keeps the odd character);
/// unchanged when it is not longer than `keep`.
pub fn elide_middle(s: &str, keep: usize) -> String {
    let n = s.chars().count();
    if n <= keep {
        return s.to_string();
    }
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let start: String = s.chars().take(head).collect();
    let end: String = s.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

/// `3.1 MB`: bytes in decimal units with one decimal (below 1 KB, whole bytes).
pub fn size(bytes: u64) -> String {
    const UNITS: [(f64, &str); 4] = [(1e12, "TB"), (1e9, "GB"), (1e6, "MB"), (1e3, "KB")];
    let b = bytes as f64;
    match UNITS.iter().find(|(scale, _)| b >= *scale) {
        Some((scale, unit)) => format!("{:.1} {unit}", b / scale),
        None if bytes == 1 => "1 byte".to_string(),
        None => format!("{bytes} bytes"),
    }
}

/// `June 4, 2026 7:42 PM`.
pub fn date_time(d: DateTime) -> String {
    let (hour, half) = match d.hour() {
        0 => (12, "AM"),
        h @ 1..=11 => (h, "AM"),
        12 => (12, "PM"),
        h => (h.saturating_sub(12), "PM"),
    };
    format!("{} {hour}:{:02} {half}", long_date(d.date()), d.minute())
}

/// `48 pictures`, `1 picture`.
pub fn pictures(n: usize) -> String {
    if n == 1 { "1 picture".to_string() } else { format!("{n} pictures") }
}

/// The line for one photo: name, date, dimensions, size on disk (unknown fields left out).
fn one(p: &Photo) -> Line {
    let mut rest = Vec::new();
    rest.extend(p.sort_date().map(date_time));
    if p.width > 0 && p.height > 0 {
        rest.push(format!("{} × {}", p.width, p.height));
    }
    rest.push(size(p.size));
    Line { name: Some(p.name.clone()), rest }
}

/// `June 4, 2026 to June 6, 2026`, or one date when they all fall on the same day; `None` when no
/// photo has a date.
pub fn date_range(dates: impl IntoIterator<Item = Date>) -> Option<String> {
    let (first, last) = dates
        .into_iter()
        .fold((None, None), |(lo, hi): (Option<Date>, Option<Date>), d| (Some(lo.map_or(d, |l| l.min(d))), Some(hi.map_or(d, |h| h.max(d)))));
    match (first, last) {
        (Some(a), Some(b)) if a != b => Some(format!("{} to {}", long_date(a), long_date(b))),
        (Some(a), _) => Some(long_date(a)),
        _ => None,
    }
}

/// The line for several photos: how many, their date range, their size on disk.
fn several(photos: &[&Photo]) -> Line {
    let mut rest = vec![pictures(photos.len())];
    rest.extend(date_range(photos.iter().filter_map(|p| p.sort_date().map(|d| d.date()))));
    let bytes = photos.iter().fold(0u64, |sum, p| sum.saturating_add(p.size));
    rest.push(format!("{} on disk", size(bytes)));
    Line { name: None, rest }
}

/// What the line says about the selection: one photo; several; else the selected folder's
/// pictures; else "No selection". Selected ids the library no longer has are left out.
pub fn line(library: &Library, folder: Option<FolderId>, selection: &[PhotoId]) -> Line {
    let picked: Vec<&Photo> = selection.iter().filter_map(|id| library.photo(*id)).collect();
    match picked.as_slice() {
        [p] => one(p),
        [] => match folder.filter(|f| library.folder(*f).is_some()) {
            Some(f) => several(&library.photos_in(f)),
            None => Line::plain(NO_SELECTION),
        },
        many => several(many),
    }
}

/// What the cached line was built from: the library revision, the folder, the selection.
type Key = (u64, Option<FolderId>, Vec<PhotoId>);

/// The line for `app`'s selection, rebuilt only when the library, folder or selection changed
/// (cached in `ctx`); `None` before the library is open.
fn cached_line(app: &App, ctx: &egui::Context) -> Option<Line> {
    let session = app.session()?;
    let state = app.state();
    let key: Key = (session.library().revision(), state.folder, state.selection.ids.clone());
    let id = Id::new("status:line");
    if let Some((k, line)) = ctx.data(|d| d.get_temp::<(Key, Line)>(id))
        && k == key
    {
        return Some(line);
    }
    let line = line(session.library(), state.folder, &state.selection.ids);
    ctx.data_mut(|d| d.insert_temp(id, (key, line.clone())));
    Some(line)
}

/// Clears the transient message once it has shown for [`MESSAGE_SECONDS`], and asks for the frame
/// that will clear it.
fn expire_message(app: &mut App, ctx: &egui::Context) {
    let id = Id::new("status:message");
    if app.state().status.is_empty() {
        ctx.data_mut(|d| d.remove::<(String, f64)>(id));
        return;
    }
    let now = ctx.input(|i| i.time);
    let since = match ctx.data(|d| d.get_temp::<(String, f64)>(id)) {
        Some((m, since)) if m == app.state().status => since,
        _ => {
            let message = app.state().status.clone();
            ctx.data_mut(|d| d.insert_temp(id, (message, now)));
            now
        }
    };
    let left = MESSAGE_SECONDS - (now - since);
    if left.is_finite() && left > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
    } else {
        app.state_mut().status.clear();
        ctx.data_mut(|d| d.remove::<(String, f64)>(id));
    }
}

/// Paints the status line into `ui`'s rect (the status-line region of [`super::layout`]) and
/// records the text drawn in the view state.
pub fn show(_p: &Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let ctx = ui.ctx().clone();
    expire_message(app, &ctx);
    let line = match app.state().status.as_str() {
        "" => cached_line(app, &ctx).unwrap_or_default(),
        message => Line::plain(message),
    };
    let font = fonts::regular(t.small);
    let painter = ui.painter();
    let width = |s: &str| painter.layout_no_wrap(s.to_owned(), font.clone(), t.text_dim).size().x;
    let max = (rect.width() - 2.0 * PAD).max(0.0);
    let mut text = line.text();
    // Too long: middle-elide the file name, a character at a time (bounded by the name's length).
    if let Some(name) = &line.name
        && width(&text) > max
    {
        for keep in (0..name.chars().count()).rev() {
            text = line.with_name_cut_to(keep);
            if width(&text) <= max {
                break;
            }
        }
    }
    let drawn = widgets::label_in(painter, rect, Align2::CENTER_CENTER, &text, font, t.text_dim);
    widgets::register(&ctx, "text:status", Role::Other, &text, Target::Painted { rect: drawn, clip: rect }, false);
    app.state_mut().status_line = text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_the_spec_shows_them() {
        assert_eq!(size(3_100_000), "3.1 MB");
        assert_eq!(size(212_400_000), "212.4 MB");
        assert_eq!(size(999), "999 bytes");
        assert_eq!(size(1), "1 byte");
        assert_eq!(size(0), "0 bytes");
        assert_eq!(size(1_500), "1.5 KB");
        assert_eq!(size(2_000_000_000), "2.0 GB");
        assert_eq!(size(u64::MAX), "18446744.1 TB");
    }

    #[test]
    fn dates_and_times_read_as_the_spec_shows_them() {
        let day = |d| Date::new(2026, 6, d).unwrap();
        let at = |h, m| DateTime::new(day(4), h, m, 0).unwrap();
        assert_eq!(date_time(at(19, 42)), "June 4, 2026 7:42 PM");
        assert_eq!(date_time(at(0, 5)), "June 4, 2026 12:05 AM");
        assert_eq!(date_time(at(12, 0)), "June 4, 2026 12:00 PM");
        assert_eq!(date_time(at(11, 59)), "June 4, 2026 11:59 AM");
        assert_eq!(date_range([day(6), day(4), day(5)]).as_deref(), Some("June 4, 2026 to June 6, 2026"));
        assert_eq!(date_range([day(4), day(4)]).as_deref(), Some("June 4, 2026"));
        assert_eq!(date_range([]), None);
    }

    #[test]
    fn long_names_lose_their_middle() {
        assert_eq!(elide_middle("IMG_0361.jpg", 20), "IMG_0361.jpg");
        assert_eq!(elide_middle("IMG_0361.jpg", 7), "IMG_…jpg");
        assert_eq!(elide_middle("abc", 1), "a…");
        assert_eq!(elide_middle("abc", 0), "…");
        assert_eq!(elide_middle("ñandú.jpg", 4), "ña…pg", "characters, never bytes");
        let line = Line { name: Some("a-very-long-name.jpg".into()), rest: vec!["3.1 MB".into()] };
        assert_eq!(line.text(), "a-very-long-name.jpg     3.1 MB");
        assert_eq!(line.with_name_cut_to(6), "a-v…jpg     3.1 MB");
    }

    #[test]
    fn an_empty_library_has_no_selection() {
        let lib = Library::new();
        assert_eq!(line(&lib, None, &[]).text(), NO_SELECTION);
        assert_eq!(line(&lib, Some(FolderId::new(4)), &[PhotoId::new(9)]).text(), NO_SELECTION, "unknown ids say nothing");
        assert_eq!(pictures(1), "1 picture");
        assert_eq!(pictures(0), "0 pictures");
    }
}
