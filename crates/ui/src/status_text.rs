//! The status line's text (`ui-layout.md` §6.1), as pure functions of the library and the view
//! state, so the status line, the thumbnail tooltip and the tests share one formatter.
//!
//! - **What it says** ([`line`]): the Photo Tray's contents ([`crate::tray::photos`]: the held
//!   photos and the selection, each once). Nothing: "No selection". One photo: its name, date
//!   taken, pixel size and file size ([`one`]). Several photos or a section selected as a unit: how
//!   many, their date range and their size on disk, and their tags when they have some
//!   ([`several`]). The edit view (M7) prefixes the folder and adds the photo's place in it
//!   ([`edit`]).
//! - **How it is written**: sizes in decimal units with one decimal ([`size`]), dates in full
//!   ([`date_time`], [`date_range`]), fields separated by wide spaces ([`SEP`]).
//! - **When it is too long** ([`Line::fit`]): the folder prefix goes first, then the file name loses
//!   its middle ("IMG_…jpg"); a line still too long (a long tag list) loses its end.

use dalica_core::{Date, DateTime, PhotoId};
use dalica_library::{Library, Photo};

use crate::panels::light_table::long_date;
use crate::state::UiState;
use crate::tray;

/// The text shown when nothing is selected.
pub const NO_SELECTION: &str = "No selection";
/// How long a transient message replaces the text (seconds).
pub const MESSAGE_SECONDS: f64 = 4.0;
/// The space between two fields.
pub const SEP: &str = "     ";
/// Between the edit view's folder prefix and the file name.
pub const PREFIX_SEP: &str = " › ";
/// The mark that stands for what elision cut out.
pub const ELLIPSIS: &str = "…";

/// The status line's text, as fields: `prefix` the edit view's folder (the first thing dropped
/// when the line is too long), `name` the one-photo file name (middle-elided next), `rest` the
/// fields after it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    pub prefix: Option<String>,
    pub name: Option<String>,
    pub rest: Vec<String>,
}

impl Line {
    /// A line of one field (a transient message, "No selection").
    pub fn plain(text: impl Into<String>) -> Line {
        Line { prefix: None, name: None, rest: vec![text.into()] }
    }

    /// The whole text: "Folder › name", then the fields, joined by wide spaces.
    pub fn text(&self) -> String {
        self.render(true, None)
    }

    /// The text with or without the prefix, the name cut to `keep` characters (`None`: whole).
    fn render(&self, prefix: bool, keep: Option<usize>) -> String {
        let name = self.name.as_deref().map(|n| keep.map_or_else(|| n.to_owned(), |k| elide_middle(n, k)));
        let head = match (self.prefix.as_deref().filter(|_| prefix), name) {
            (Some(p), Some(n)) => Some(format!("{p}{PREFIX_SEP}{n}")),
            (Some(p), None) => Some(p.to_owned()),
            (None, n) => n,
        };
        head.iter().chain(self.rest.iter()).map(String::as_str).collect::<Vec<_>>().join(SEP)
    }

    /// The text that fits in `max` (as measured by `width`): the whole text; else without the
    /// prefix; else with the name middle-elided to the most characters that fit; else (nothing of
    /// the name fits) cut at its end with "…". A `max` that is not a number keeps the whole text.
    /// Measures O(log n) candidates.
    pub fn fit(&self, max: f32, width: impl Fn(&str) -> f32) -> String {
        let whole = self.text();
        let too_wide = width(&whole) > max;
        if !too_wide {
            return whole;
        }
        if self.prefix.is_some() {
            let bare = self.render(false, None);
            if width(&bare) <= max {
                return bare;
            }
        }
        if let Some(name) = &self.name {
            let n = name.chars().count();
            if n > 0 && width(&self.render(false, Some(0))) <= max {
                // The most characters of the name kept that still fit (width grows with them).
                let (mut lo, mut hi) = (0usize, n - 1);
                while lo < hi {
                    let mid = lo + (hi - lo).div_ceil(2);
                    if width(&self.render(false, Some(mid))) <= max {
                        lo = mid;
                    } else {
                        hi = mid - 1;
                    }
                }
                return self.render(false, Some(lo));
            }
        }
        let base = if self.name.is_some() { self.render(false, Some(0)) } else { self.render(false, None) };
        elide_end(&base, max, width)
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
    format!("{start}{ELLIPSIS}{end}")
}

/// `s` cut at its end with "…" to the most characters that fit `max`; unchanged when it fits;
/// empty when not even "…" fits.
pub fn elide_end(s: &str, max: f32, width: impl Fn(&str) -> f32) -> String {
    if width(s) <= max {
        return s.to_owned();
    }
    if !max.is_finite() || width(ELLIPSIS) > max {
        return String::new();
    }
    let cut = |k: usize| format!("{}{ELLIPSIS}", s.chars().take(k).collect::<String>().trim_end());
    let (mut lo, mut hi) = (0usize, s.chars().count().saturating_sub(1));
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if width(&cut(mid)) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
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

/// `4000 × 3000`, when the size is known.
pub fn pixels(width: u32, height: u32) -> Option<String> {
    (width > 0 && height > 0).then(|| format!("{width} × {height}"))
}

/// `48 pictures`, `1 picture`.
pub fn pictures(n: usize) -> String {
    if n == 1 { "1 picture".to_string() } else { format!("{n} pictures") }
}

/// `(12 of 48)`: the edit view's photo `nth` (from 1) of `of`.
pub fn position(nth: usize, of: usize) -> String {
    format!("({nth} of {of})")
}

/// `Tags: lisbon (40), tram (6)`; `None` without tags.
pub fn tags(counts: &[(String, usize)]) -> Option<String> {
    if counts.is_empty() {
        return None;
    }
    let list: Vec<String> = counts.iter().map(|(tag, n)| format!("{tag} ({n})")).collect();
    Some(format!("Tags: {}", list.join(", ")))
}

/// How many photos carry each tag, given every photo's tags in a row: the most used first, ties in
/// alphabetical order.
pub fn tag_counts<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<(String, usize)> {
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for tag in tags {
        let n = counts.entry(tag).or_default();
        *n = n.saturating_add(1);
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().map(|(t, n)| (t.to_owned(), n)).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// A photo's details: date taken (else the file's date), pixel size, file size (unknown fields
/// left out). The status line and the thumbnail tooltip both show them.
pub fn details(p: &Photo) -> Vec<String> {
    let mut out = Vec::with_capacity(3);
    out.extend(p.sort_date().map(date_time));
    out.extend(pixels(p.width, p.height));
    out.push(size(p.size));
    out
}

/// The line for one photo: name, date, pixel size, size on disk.
pub fn one(p: &Photo) -> Line {
    Line { prefix: None, name: Some(p.name.clone()), rest: details(p) }
}

/// The line for several photos: how many, their date range, their size on disk, their tags.
pub fn several(photos: &[&Photo], tag_counts: &[(String, usize)]) -> Line {
    let mut rest = vec![pictures(photos.len())];
    rest.extend(date_range(photos.iter().filter_map(|p| p.sort_date().map(|d| d.date()))));
    let bytes = photos.iter().fold(0u64, |sum, p| sum.saturating_add(p.size));
    rest.push(format!("{} on disk", size(bytes)));
    rest.extend(tags(tag_counts));
    Line { prefix: None, name: None, rest }
}

/// The edit view's line (M7): "Folder › name", the photo's details, then its place in the folder
/// "(12 of 48)".
pub fn edit(folder: &str, p: &Photo, nth: usize, of: usize) -> Line {
    let mut line = one(p);
    line.prefix = Some(folder.to_owned());
    line.rest.push(position(nth, of));
    line
}

/// What the line says about the Photo Tray's contents in `state`: one photo; several; else "No
/// selection". Photos the library no longer has are left out.
pub fn line(library: &Library, state: &UiState) -> Line {
    let ids: Vec<PhotoId> = tray::photos(state);
    let picked: Vec<&Photo> = ids.iter().filter_map(|id| library.photo(*id)).collect();
    match picked.as_slice() {
        [] => Line::plain(NO_SELECTION),
        [p] => one(p),
        // Photos carry no tags until M6: the field stays out until then.
        many => several(many, &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Selection;
    use dalica_core::FolderId;

    /// One unit per character: a font where every character is as wide.
    fn chars(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn sizes_read_as_the_spec_shows_them() {
        assert_eq!(size(3_100_000), "3.1 MB");
        assert_eq!(size(212_400_000), "212.4 MB");
        assert_eq!(size(12_700), "12.7 KB");
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
    fn small_fields_read_as_the_spec_shows_them() {
        assert_eq!(pixels(4000, 3000).as_deref(), Some("4000 × 3000"));
        assert_eq!(pixels(0, 3000), None);
        assert_eq!(pictures(1), "1 picture");
        assert_eq!(pictures(0), "0 pictures");
        assert_eq!(position(12, 48), "(12 of 48)");
        assert_eq!(tags(&[]), None);
        let counts = tag_counts(["tram", "lisbon", "lisbon", "beach", "tram", "lisbon", "alfama"]);
        assert_eq!(counts[2..], [("alfama".to_string(), 1), ("beach".to_string(), 1)], "ties in alphabetical order");
        let counts = tag_counts(["tram", "lisbon", "lisbon", "beach", "tram", "lisbon"]);
        assert_eq!(counts, [("lisbon".to_string(), 3), ("tram".to_string(), 2), ("beach".to_string(), 1)]);
        assert_eq!(tags(&counts).as_deref(), Some("Tags: lisbon (3), tram (2), beach (1)"));
    }

    #[test]
    fn long_names_lose_their_middle() {
        assert_eq!(elide_middle("IMG_0361.jpg", 20), "IMG_0361.jpg");
        assert_eq!(elide_middle("IMG_0361.jpg", 7), "IMG_…jpg");
        assert_eq!(elide_middle("abc", 1), "a…");
        assert_eq!(elide_middle("abc", 0), "…");
        assert_eq!(elide_middle("ñandú.jpg", 4), "ña…pg", "characters, never bytes");
    }

    #[test]
    fn a_long_line_drops_the_prefix_then_elides_the_name_then_its_end() {
        let line = Line { prefix: Some("Lisbon".into()), name: Some("IMG_0361.jpg".into()), rest: vec!["3.1 MB".into(), "(12 of 48)".into()] };
        let whole = "Lisbon › IMG_0361.jpg     3.1 MB     (12 of 48)";
        assert_eq!(line.text(), whole);
        assert_eq!(line.fit(chars(whole), chars), whole, "fits");
        assert_eq!(line.fit(f32::NAN, chars), whole, "no width: the whole text");
        let bare = "IMG_0361.jpg     3.1 MB     (12 of 48)";
        assert_eq!(line.fit(chars(whole) - 1.0, chars), bare, "the prefix goes first");
        assert_eq!(line.fit(chars(bare), chars), bare);
        assert_eq!(line.fit(chars(bare) - 1.0, chars), "IMG_0…1.jpg     3.1 MB     (12 of 48)", "then the name's middle");
        assert_eq!(line.fit(chars("IMG_…jpg     3.1 MB     (12 of 48)"), chars), "IMG_…jpg     3.1 MB     (12 of 48)");
        assert_eq!(line.fit(chars("…     3.1 MB     (12 of 48)"), chars), "…     3.1 MB     (12 of 48)");
        assert_eq!(line.fit(10.0, chars), "…     3.1…", "then the end");
        assert_eq!(line.fit(0.5, chars), "", "not even the ellipsis");
        assert_eq!(line.fit(-3.0, chars), "");

        // A line without a name (several photos) loses its end.
        let many = Line::plain("48 pictures     Tags: lisbon (40), tram (6)");
        assert_eq!(many.fit(20.0, chars), "48 pictures     Tag…");
        assert_eq!(elide_end("abc", 2.0, chars), "a…");
        assert_eq!(elide_end("abc", 1.0, chars), "…");
        assert_eq!(elide_end("abc", f32::INFINITY, chars), "abc");
        // A prefix without a name (no photo yet) is still dropped first.
        let folder = Line { prefix: Some("Lisbon".into()), name: None, rest: vec!["0 pictures".into()] };
        assert_eq!(folder.text(), "Lisbon     0 pictures");
        assert_eq!(folder.fit(12.0, chars), "0 pictures");
    }

    fn photo(id: u64, name: &str, bytes: u64, date: Option<(u8, u8, u8)>) -> Photo {
        let date = date.map(|(d, h, m)| DateTime::new(Date::new(2026, 6, d).unwrap(), h, m, 0).unwrap());
        Photo {
            id: PhotoId::new(id),
            folder: FolderId::new(1),
            name: name.into(),
            size: bytes,
            mtime: Default::default(),
            width: 4000,
            height: 3000,
            date,
            star: false,
        }
    }

    #[test]
    fn the_lines_read_as_the_spec_shows_them() {
        let a = photo(1, "IMG_0361.jpg", 3_100_000, Some((4, 19, 42)));
        let b = photo(2, "IMG_0362.jpg", 200_000_000, Some((6, 8, 0)));
        let c = photo(3, "IMG_0363.jpg", 9_300_000, Some((5, 8, 0)));
        assert_eq!(one(&a).text(), "IMG_0361.jpg     June 4, 2026 7:42 PM     4000 × 3000     3.1 MB");
        assert_eq!(several(&[&a, &b, &c], &[]).text(), "3 pictures     June 4, 2026 to June 6, 2026     212.4 MB on disk");
        let counts = [("lisbon".to_string(), 40), ("tram".to_string(), 6)];
        assert_eq!(
            several(&[&a, &b, &c], &counts).text(),
            "3 pictures     June 4, 2026 to June 6, 2026     212.4 MB on disk     Tags: lisbon (40), tram (6)"
        );
        assert_eq!(several(&[&a, &a], &[]).text(), "2 pictures     June 4, 2026     6.2 MB on disk", "one day: that day alone");
        assert_eq!(
            edit("Lisbon Weekend", &a, 12, 48).text(),
            "Lisbon Weekend › IMG_0361.jpg     June 4, 2026 7:42 PM     4000 × 3000     3.1 MB     (12 of 48)"
        );
    }

    #[test]
    fn an_empty_tray_has_no_selection() {
        let lib = Library::new();
        assert_eq!(line(&lib, &UiState::default()).text(), NO_SELECTION);
        let mut s = UiState { selection: Selection::new(vec![PhotoId::new(9)], None, None, false), ..UiState::default() };
        s.tray.set_held(vec![PhotoId::new(4)]);
        assert_eq!(line(&lib, &s).text(), NO_SELECTION, "unknown ids say nothing");
    }
}
