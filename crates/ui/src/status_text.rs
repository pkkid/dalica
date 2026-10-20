//! The status line's text (`ui-layout.md` §6.1), as pure functions of the library and the view
//! state, so the status line, the thumbnail tooltip and the tests share one formatter.
//!
//! - **What it says** ([`line`]): the Photo Tray's contents ([`crate::tray::photos`]: the held
//!   photos and the selection, each once). Nothing: "No selection". One photo: its name, date
//!   (the effective one, [`Library::sort_date`]: the date set by the user, else the date taken,
//!   else the file's), pixel size and file size ([`one`]). Several photos or a section selected as
//!   a unit: how many, their date range and their size on disk, and their tags when they have
//!   some, most used first ([`several`], [`selection_tags`]). The edit view prefixes the folder
//!   and adds the photo's place in its section ([`edit_photo`]): "Lisbon › IMG_0361.jpg … (12 of 48)".
//!   The Collage room in front speaks of its own selection instead ([`collage_photos`]: the
//!   pictures selected on its canvas and the clips selected in its Clips tab, [`photos_line`]).
//! - **How it is written**: sizes in decimal units with one decimal ([`size`]), dates in full
//!   ([`date_time`], [`date_range`]), fields separated by wide spaces ([`SEP`]).
//! - **Background work** ([`exporting`], [`scanning`], [`with_activity`]): while an export runs,
//!   "Exporting 12 of 48" (`creating.md` §2), and while a scan runs, "Scanning: 1,204 pictures
//!   found" (`organizing.md` §1), stand in place of "No selection", and are the last fields when
//!   something is selected; with both, the export's comes first.
//! - **When it is too long** ([`Line::fit`]): the scan field goes first, then the export's, then the
//!   folder prefix, then the file name loses its middle ("IMG_…jpg"); a line still too long (a long
//!   tag list) loses its end.

use dalica_core::{Date, DateTime, PhotoId};
use dalica_library::{Library, Photo};

use crate::panels::light_table::long_date;
use crate::state::UiState;
use crate::{tl, tln, tray};

/// The text shown when nothing is selected, in English ([`no_selection`] in the language in
/// effect).
pub const NO_SELECTION: &str = "No selection";

/// The text shown when nothing is selected.
pub fn no_selection() -> String {
    tl!("No selection")
}
/// How long a transient message replaces the text (seconds).
pub const MESSAGE_SECONDS: f64 = 4.0;
/// The space between two fields.
pub const SEP: &str = "     ";
/// Between the edit view's folder prefix and the file name.
pub const PREFIX_SEP: &str = " › ";
/// The mark that stands for what elision cut out.
pub const ELLIPSIS: &str = "…";

/// The status line's text, as fields: `prefix` the edit view's folder, `name` the one-photo file
/// name, `rest` the fields after it, `activity` the background work's fields at the end
/// ("Exporting 12 of 48", "Scanning: 1,204 pictures found"). When the line is too long, `activity`
/// is dropped from its end one field at a time first, then `prefix`, then `name` is middle-elided.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    pub prefix: Option<String>,
    pub name: Option<String>,
    pub rest: Vec<String>,
    pub activity: Vec<String>,
}

impl Line {
    /// A line of one field (a transient message, "No selection").
    pub fn plain(text: impl Into<String>) -> Line {
        Line { rest: vec![text.into()], ..Line::default() }
    }

    /// The whole text: "Folder › name", then the fields, then the activity, joined by wide spaces.
    pub fn text(&self) -> String {
        self.with_fields(self.render(true, None), self.activity.len())
    }

    /// `text` followed by the first `n` activity fields.
    fn with_fields(&self, text: String, n: usize) -> String {
        let fields = self.activity.iter().take(n).map(String::as_str);
        if text.is_empty() {
            fields.collect::<Vec<_>>().join(SEP)
        } else {
            std::iter::once(text.as_str()).chain(fields).collect::<Vec<_>>().join(SEP)
        }
    }

    /// The text without the activity, with or without the prefix, the name cut to `keep`
    /// characters (`None`: whole).
    fn render(&self, prefix: bool, keep: Option<usize>) -> String {
        let name = self.name.as_deref().map(|n| keep.map_or_else(|| n.to_owned(), |k| elide_middle(n, k)));
        let head = match (self.prefix.as_deref().filter(|_| prefix), name) {
            (Some(p), Some(n)) => Some(format!("{p}{PREFIX_SEP}{n}")),
            (Some(p), None) => Some(p.to_owned()),
            (None, n) => n,
        };
        head.iter().chain(self.rest.iter()).map(String::as_str).collect::<Vec<_>>().join(SEP)
    }

    /// The text that fits in `max` (as measured by `width`): the whole text; else with fewer
    /// activity fields, the last dropped first, down to none; else also without the prefix; else with the name middle-elided to the most
    /// characters that fit; else (nothing of the name fits) cut at its end with "…". A `max` that
    /// is not a number keeps the whole text. Measures O(log n) candidates.
    pub fn fit(&self, max: f32, width: impl Fn(&str) -> f32) -> String {
        let whole = self.text();
        let too_wide = width(&whole) > max;
        if !too_wide {
            return whole;
        }
        let base = self.render(true, None);
        for n in (0..self.activity.len()).rev() {
            let fewer = self.with_fields(base.clone(), n);
            if width(&fewer) <= max {
                return fewer;
            }
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

/// `3.1 MB`: bytes in decimal units with one decimal (below 1 KB, whole bytes), in the language in
/// effect ([`crate::i18n::format::file_size`]).
pub fn size(bytes: u64) -> String {
    crate::i18n::format::file_size(bytes)
}

/// `7:42 PM`, or `7:42:05 PM` with `seconds`: a time of day on the twelve-hour clock.
pub fn clock(d: DateTime, seconds: bool) -> String {
    let (hour, pm) = match d.hour() {
        0 => (12, false),
        h @ 1..=11 => (h, false),
        12 => (12, true),
        h => (h.saturating_sub(12), true),
    };
    let (minute, second) = (format!("{:02}", d.minute()), format!("{:02}", d.second()));
    match (seconds, pm) {
        (false, false) => tl!(ctx = "time", "{hour}:{minute} AM", hour = hour, minute = minute),
        (false, true) => tl!(ctx = "time", "{hour}:{minute} PM", hour = hour, minute = minute),
        (true, false) => tl!(ctx = "time", "{hour}:{minute}:{second} AM", hour = hour, minute = minute, second = second),
        (true, true) => tl!(ctx = "time", "{hour}:{minute}:{second} PM", hour = hour, minute = minute, second = second),
    }
}

/// `June 4, 2026 7:42 PM`.
pub fn date_time(d: DateTime) -> String {
    tl!(ctx = "date", "{date} {time}", date = long_date(d.date()), time = clock(d, false))
}

/// `June 4, 2026 to June 6, 2026`, or one date when they all fall on the same day; `None` when no
/// photo has a date.
pub fn date_range(dates: impl IntoIterator<Item = Date>) -> Option<String> {
    let (first, last) = dates
        .into_iter()
        .fold((None, None), |(lo, hi): (Option<Date>, Option<Date>), d| (Some(lo.map_or(d, |l| l.min(d))), Some(hi.map_or(d, |h| h.max(d)))));
    match (first, last) {
        (Some(a), Some(b)) if a != b => Some(tl!(ctx = "date", "{first} to {last}", first = long_date(a), last = long_date(b))),
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
    tln!(n, "{n} picture", "{n} pictures")
}

/// `1,204`: a count with its thousands grouped as the language groups them
/// ([`crate::i18n::format::number`]).
pub fn grouped(n: usize) -> String {
    crate::i18n::format::number(u64::try_from(n).unwrap_or(u64::MAX))
}

/// `Scanning: 1,204 pictures found` (`organizing.md` §1): the status line's field while a scan
/// runs, `found` the photos its walks have listed so far.
pub fn scanning(found: usize) -> String {
    tln!(found, "Scanning: {n} picture found", "Scanning: {n} pictures found")
}

/// `Exporting 12 of 48` (`creating.md` §2): the status line's field while an export runs, `done`
/// of its `total` photos (never more than the total).
pub fn exporting(done: u64, total: u64) -> String {
    let as_count = |n: u64| grouped(usize::try_from(n).unwrap_or(usize::MAX));
    tl!("Exporting {done} of {total}", done = as_count(done.min(total)), total = as_count(total))
}

/// `line` with the background work's fields `activity` (most kept first: the export's, then the
/// scan's): in place of "No selection" when nothing is selected, else as the last fields; when space
/// is short the last is dropped first. No fields leave the line as it is.
pub fn with_activity(mut line: Line, activity: Vec<String>) -> Line {
    if activity.is_empty() {
        return line;
    }
    if line == Line::plain(no_selection()) {
        // The first field stands as the line's own, so a line too narrow even for it loses its end.
        let mut fields = activity.into_iter();
        return Line { rest: fields.next().into_iter().collect(), activity: fields.collect(), ..Line::default() };
    }
    line.activity = activity;
    line
}

/// `(12 of 48)`: the edit view's photo `nth` (from 1) of `of`.
pub fn position(nth: usize, of: usize) -> String {
    tl!("({nth} of {of})", nth = grouped(nth), of = grouped(of))
}

/// `Tags: lisbon (40), tram (6)`; `None` without tags.
pub fn tags(counts: &[(String, usize)]) -> Option<String> {
    if counts.is_empty() {
        return None;
    }
    let list: Vec<String> = counts.iter().map(|(tag, n)| format!("{tag} ({n})")).collect();
    Some(tl!("Tags: {tags}", tags = list.join(&tl!(ctx = "list", ", "))))
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

/// A photo's details: `date` (the effective date, else the file's), pixel size as `shown` (the
/// size it shows at, [`displayed`]), file size (unknown fields left out). The status line and the
/// thumbnail tooltip both show them.
pub fn details(p: &Photo, date: Option<DateTime>, shown: (u32, u32)) -> Vec<String> {
    let mut out = Vec::with_capacity(3);
    out.extend(date.map(date_time));
    out.extend(pixels(shown.0, shown.1));
    out.push(size(p.size));
    out
}

/// The size photo `p` shows at: its file's displayed size grown by its recipe's frames (Border,
/// Drop Shadow, …) and turned by its rotations and flips (a landscape turned a quarter is a
/// portrait); a crop is not counted (`edit::shown_size`).
pub fn displayed(library: &Library, p: &Photo) -> (u32, u32) {
    dalica_edit::shown_size(&library.info(p.id).edits, p.width, p.height, dalica_edit::Cropping::Ignored)
}

/// The line for one photo: name, effective date, pixel size as it shows ([`displayed`]), size on
/// disk.
pub fn one(library: &Library, p: &Photo) -> Line {
    Line { name: Some(p.name.clone()), rest: details(p, library.sort_date(p), displayed(library, p)), ..Line::default() }
}

/// The line for several photos: how many, their date range, their size on disk, their tags.
pub fn several(library: &Library, photos: &[&Photo], tag_counts: &[(String, usize)]) -> Line {
    let mut rest = vec![pictures(photos.len())];
    rest.extend(date_range(photos.iter().filter_map(|p| library.sort_date(p).map(|d| d.date()))));
    let bytes = photos.iter().fold(0u64, |sum, p| sum.saturating_add(p.size));
    rest.push(tl!("{size} on disk", size = size(bytes)));
    rest.extend(tags(tag_counts));
    Line { rest, ..Line::default() }
}

/// The edit view's line format: "Folder › name", the photo's details from its file (date taken,
/// stored pixel size), then its place "(12 of 48)". The edit view itself shows [`edit_photo`],
/// which reads the library.
pub fn edit(folder: &str, p: &Photo, nth: usize, of: usize) -> Line {
    let mut line = Line { name: Some(p.name.clone()), rest: details(p, p.sort_date(), (p.width, p.height)), ..Line::default() };
    line.prefix = Some(folder.to_owned());
    line.rest.push(position(nth, of));
    line
}

/// The edit view's line (`ui-layout.md` §6.1): "Folder › name", the photo's details as [`one`]
/// gives them (its effective date, the size it shows at), then its place among its section's photos
/// "(12 of 48)" (`nth` from 1; left out when the photo is on no section, `of` 0).
pub fn edit_photo(library: &Library, folder: &str, p: &Photo, nth: usize, of: usize) -> Line {
    let mut line = one(library, p);
    line.prefix = Some(folder.to_owned());
    if of > 0 {
        line.rest.push(position(nth, of));
    }
    line
}

/// What the line says about the Photo Tray's contents in `state`: one photo; several; else "No
/// selection". Photos the library no longer has are left out.
pub fn line(library: &Library, state: &UiState) -> Line {
    photos_line(library, &tray::photos(state))
}

/// What the line says about photos `ids`: one photo; several; else "No selection". Photos the
/// library no longer has are left out.
pub fn photos_line(library: &Library, ids: &[PhotoId]) -> Line {
    let picked: Vec<&Photo> = ids.iter().filter_map(|id| library.photo(*id)).collect();
    match picked.as_slice() {
        [] => Line::plain(no_selection()),
        [p] => one(library, p),
        many => several(library, many, &selection_tags(library, ids)),
    }
}

/// The photos the Collage room's selection holds in `state`: the pictures selected on its canvas
/// (by number into `placed`, the photo of each picture on the page in the engine's order), then
/// the clips selected in its Clips tab, each photo once. A number past the page is left out.
pub fn collage_photos(state: &UiState, placed: &[PhotoId]) -> Vec<PhotoId> {
    let canvas = state.collage.selection.iter().filter_map(|n| placed.get(*n as usize).copied());
    let mut out: Vec<PhotoId> = Vec::new();
    for id in canvas.chain(state.collage.clips.iter().copied()) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// The tags of the photos `ids` with how many of them hold each, most used first, ties in
/// alphabetical order ignoring case: the status line's "Tags: lisbon (40), tram (6)". Unknown and
/// repeated ids count once at most.
pub fn selection_tags(library: &Library, ids: &[PhotoId]) -> Vec<(String, usize)> {
    // `tag_counts_in` lists them in alphabetical order ignoring case; a stable sort keeps it for
    // ties.
    let mut counts: Vec<(String, usize)> = library.tag_counts_in(ids).into_iter().map(|c| (c.tag, c.count)).collect();
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts
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
        assert_eq!(size(u64::MAX), "18,446,744.1 TB");
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
        let line = Line {
            prefix: Some("Lisbon".into()),
            name: Some("IMG_0361.jpg".into()),
            rest: vec!["3.1 MB".into(), "(12 of 48)".into()],
            activity: Vec::new(),
        };
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
        let folder = Line { prefix: Some("Lisbon".into()), rest: vec!["0 pictures".into()], ..Line::default() };
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
        let lib = Library::new();
        assert_eq!(one(&lib, &a).text(), "IMG_0361.jpg     June 4, 2026 7:42 PM     4000 × 3000     3.1 MB");
        assert_eq!(several(&lib, &[&a, &b, &c], &[]).text(), "3 pictures     June 4, 2026 to June 6, 2026     212.4 MB on disk");
        let counts = [("lisbon".to_string(), 40), ("tram".to_string(), 6)];
        assert_eq!(
            several(&lib, &[&a, &b, &c], &counts).text(),
            "3 pictures     June 4, 2026 to June 6, 2026     212.4 MB on disk     Tags: lisbon (40), tram (6)"
        );
        assert_eq!(several(&lib, &[&a, &a], &[]).text(), "2 pictures     June 4, 2026     6.2 MB on disk", "one day: that day alone");
        assert_eq!(
            edit("Lisbon Weekend", &a, 12, 48).text(),
            "Lisbon Weekend › IMG_0361.jpg     June 4, 2026 7:42 PM     4000 × 3000     3.1 MB     (12 of 48)"
        );
        assert_eq!(edit_photo(&lib, "Lisbon Weekend", &a, 12, 48).text(), edit("Lisbon Weekend", &a, 12, 48).text());
        assert_eq!(
            edit_photo(&lib, "Lisbon", &a, 0, 0).text(),
            "Lisbon › IMG_0361.jpg     June 4, 2026 7:42 PM     4000 × 3000     3.1 MB",
            "on no section"
        );
    }

    #[test]
    fn counts_are_grouped_by_thousands_and_the_scan_reads_as_the_spec_shows_it() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_204), "1,204");
        assert_eq!(grouped(12_345_678), "12,345,678");
        assert_eq!(grouped(usize::MAX).len(), usize::MAX.to_string().len() + (usize::MAX.to_string().len() - 1) / 3);
        assert_eq!(scanning(1_204), "Scanning: 1,204 pictures found");
        assert_eq!(scanning(1), "Scanning: 1 picture found");
        assert_eq!(scanning(0), "Scanning: 0 pictures found");
    }

    #[test]
    fn the_scan_replaces_no_selection_and_is_dropped_first_when_space_is_short() {
        let scan = vec![scanning(1_204)];
        assert_eq!(with_activity(Line::plain(NO_SELECTION), scan.clone()).text(), "Scanning: 1,204 pictures found");
        assert_eq!(with_activity(Line::plain(NO_SELECTION), Vec::new()).text(), NO_SELECTION);
        let line = Line { name: Some("IMG_0361.jpg".into()), rest: vec!["3.1 MB".into()], ..Line::default() };
        let with = with_activity(line.clone(), scan);
        let whole = "IMG_0361.jpg     3.1 MB     Scanning: 1,204 pictures found";
        assert_eq!(with.text(), whole, "the last field");
        assert_eq!(with.fit(chars(whole), chars), whole);
        assert_eq!(with.fit(chars(whole) - 1.0, chars), "IMG_0361.jpg     3.1 MB", "dropped first");
        assert_eq!(
            with.fit(chars("IMG_0361.jpg     3.1 MB") - 1.0, chars),
            line.fit(chars("IMG_0361.jpg     3.1 MB") - 1.0, chars),
            "then as before"
        );
        // A transient message is a plain line: the scan is not added to it by the status line.
        assert_eq!(Line { activity: vec!["x".into()], ..Line::default() }.text(), "x");
    }

    #[test]
    fn an_export_reads_as_the_spec_shows_it_and_outlasts_the_scan_when_space_is_short() {
        assert_eq!(exporting(12, 48), "Exporting 12 of 48");
        assert_eq!(exporting(0, 48), "Exporting 0 of 48");
        assert_eq!(exporting(60, 48), "Exporting 48 of 48", "never more than the total");
        assert_eq!(exporting(1_204, 2_000), "Exporting 1,204 of 2,000");
        let both = vec![exporting(12, 48), scanning(1_204)];
        // Nothing selected: both in place of "No selection", the scan dropped first.
        let alone = with_activity(Line::plain(NO_SELECTION), both.clone());
        let whole = "Exporting 12 of 48     Scanning: 1,204 pictures found";
        assert_eq!(alone.text(), whole);
        assert_eq!(alone.fit(chars(whole), chars), whole);
        assert_eq!(alone.fit(chars(whole) - 1.0, chars), "Exporting 12 of 48", "the scan first");
        assert_eq!(alone.fit(10.0, chars), "Exporting…", "then the export's end");
        // A photo selected: the last fields, the scan's dropped first, then the export's, then the
        // prefix.
        let line = Line { prefix: Some("Lisbon".into()), name: Some("IMG_0361.jpg".into()), rest: vec!["3.1 MB".into()], ..Line::default() };
        let with = with_activity(line, both);
        let whole = "Lisbon › IMG_0361.jpg     3.1 MB     Exporting 12 of 48     Scanning: 1,204 pictures found";
        assert_eq!(with.text(), whole);
        let export = "Lisbon › IMG_0361.jpg     3.1 MB     Exporting 12 of 48";
        assert_eq!(with.fit(chars(whole) - 1.0, chars), export, "the scan first");
        assert_eq!(with.fit(chars(export) - 1.0, chars), "Lisbon › IMG_0361.jpg     3.1 MB", "then the export's");
        assert_eq!(with.fit(chars("Lisbon › IMG_0361.jpg     3.1 MB") - 1.0, chars), "IMG_0361.jpg     3.1 MB", "then the prefix");
    }

    #[test]
    fn an_empty_tray_has_no_selection() {
        let lib = Library::new();
        assert_eq!(line(&lib, &UiState::default()).text(), NO_SELECTION);
        let mut s = UiState { selection: Selection::new(vec![PhotoId::new(9)], None, None, false), ..UiState::default() };
        s.tray.set_held(vec![PhotoId::new(4)]);
        assert_eq!(line(&lib, &s).text(), NO_SELECTION, "unknown ids say nothing");
    }

    /// The Collage room's selection: canvas pictures by number into the placed photos, then the
    /// clips selected, each once; a number past the page is left out.
    #[test]
    fn the_collage_rooms_selection_is_its_canvas_pictures_then_its_clips() {
        let placed = [PhotoId::new(7), PhotoId::new(3), PhotoId::new(5)];
        let mut s = UiState::default();
        assert!(collage_photos(&s, &placed).is_empty());
        s.collage.selection = vec![2, 0, 9];
        s.collage.clips = vec![PhotoId::new(7), PhotoId::new(11)];
        assert_eq!(collage_photos(&s, &placed), [PhotoId::new(5), PhotoId::new(7), PhotoId::new(11)]);
        assert_eq!(photos_line(&Library::new(), &collage_photos(&s, &placed)).text(), NO_SELECTION, "unknown ids say nothing");
    }

    /// A photo turned a quarter by its recipe reads at the size it shows at, not its file's.
    #[test]
    fn a_turned_photo_reads_at_its_displayed_size() {
        let (_dir, mut s, ids) = crate::app::tests::session_with_photos(1);
        let p = s.library().photo(ids[0]).unwrap().clone();
        assert!(p.width > p.height, "a landscape");
        let state = UiState { selection: Selection::new(vec![p.id], None, None, false), ..UiState::default() };
        let (wide, tall) = (format!("{} × {}", p.width, p.height), format!("{} × {}", p.height, p.width));
        assert!(line(s.library(), &state).text().contains(&wide));
        s.execute("photo.rotate.cw", &serde_json::json!({ "ids": [p.id] })).unwrap();
        assert_eq!(displayed(s.library(), &p), (p.height, p.width));
        let text = line(s.library(), &state).text();
        assert!(text.contains(&tall) && !text.contains(&wide), "{text}");
        s.execute("photo.rotate.cw", &serde_json::json!({ "ids": [p.id] })).unwrap();
        assert!(line(s.library(), &state).text().contains(&wide), "half a turn: as stored");
    }
}
