//! Date and time fields (`design-system.md` §6, `organizing.md` §9): [`date_field`] edits a
//! [`PickedDate`] written `YYYY-MM-DD`, [`time_field`] a [`PickedTime`] written `HH:MM:SS` (24
//! hours). Both are a [`TextField`], so they edit as every text field does (Esc, Return, a click
//! away, the right-click menu), and add:
//!
//! - **Typing**: the value follows the text whenever it reads as a day (or a time) that exists.
//!   While it does not (`2026-02-31`, half a date) the value keeps the last one that did, the
//!   field is outlined in `caution`, and when the edit ends (Return, a click away) the text goes
//!   back to that value: a day that does not exist is refused, never stored. Esc gives back the
//!   value from before the edit.
//! - **Arrows**: ↑ and ↓ step the part the cursor is in (the year, month or day; the hour, minute
//!   or second) by one and select it, so the next arrow steps it again and typing replaces it.
//!   Months, days, hours, minutes and seconds go round within their range without carrying into
//!   the next part, as native date controls do; years stop at the limits. A day past the end of
//!   its month after a step becomes the month's last day (31 January, one month on, is 28
//!   February).
//! - **Limits**: years [`MIN_YEAR`] to [`MAX_YEAR`], the range Dalica's dates have.
//!
//! No locale formats yet: the text is ISO, and callers show the long date beside it. A date
//! field speaks its long date as its AccessKit description, in the month and weekday names of
//! [`crate::text`] ("Monday, October 19, 2026" in English).
//!
//! The date rules (month lengths, reading a date strictly, the limits and stopping at them rather
//! than wrapping) and their tests are adapted from LightCraft `crates/ui-egui/src/date_picker.rs`
//! at revision 034ef489 (<https://github.com/storytold/lightcraft>, MIT OR Apache-2.0; `NOTICE`).
//! Changes for Dalica: keyboard fields on `ui-kit`'s text field instead of a calendar popup;
//! whole days only, with years 1–9999 instead of 0–9999; a time of day beside the date; the arrows
//! step a part instead of paging a calendar; never a panic.

use std::ops::Range;

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{CornerRadius, Key, Modifiers, Stroke, StrokeKind, Ui};

use super::access;
use super::field::{FieldOut, TextField, egui_id};
use crate::Tokens;
use crate::text::{self, Texts};

/// The earliest year a date can have.
pub const MIN_YEAR: u16 = 1;
/// The latest year a date can have.
pub const MAX_YEAR: u16 = 9999;
/// What an empty date field shows.
pub const DATE_HINT: &str = "YYYY-MM-DD";
/// What an empty time field shows.
pub const TIME_HINT: &str = "HH:MM:SS";
/// The width that fits a date (points).
pub const DATE_WIDTH: f32 = 96.0;
/// The width that fits a time (points).
pub const TIME_WIDTH: f32 = 80.0;

/// Days in `month` (1–12) of `year` in the Gregorian calendar; 0 for a month that does not exist.
pub fn days_in_month(year: u16, month: u8) -> u8 {
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

/// The parts of a date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DatePart {
    Year,
    Month,
    Day,
}

/// The parts of a time of day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimePart {
    Hour,
    Minute,
    Second,
}

/// A day that exists, in years [`MIN_YEAR`] to [`MAX_YEAR`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PickedDate {
    year: u16,
    month: u8,
    day: u8,
}

impl PickedDate {
    /// The day, or `None` when it does not exist or lies outside the limits.
    pub fn new(year: u16, month: u8, day: u8) -> Option<PickedDate> {
        ((MIN_YEAR..=MAX_YEAR).contains(&year) && (1..=days_in_month(year, month)).contains(&day)).then_some(PickedDate { year, month, day })
    }

    pub fn year(&self) -> u16 {
        self.year
    }

    pub fn month(&self) -> u8 {
        self.month
    }

    pub fn day(&self) -> u8 {
        self.day
    }

    /// Reads `YYYY-MM-DD` exactly (spaces around it are fine): four, two and two digits; `None`
    /// for anything else or a day that does not exist.
    pub fn parse(s: &str) -> Option<PickedDate> {
        let s = s.trim();
        if s.len() != 10 || s.get(4..5) != Some("-") || s.get(7..8) != Some("-") {
            return None;
        }
        PickedDate::new(u16::try_from(digits(s, 0..4)?).ok()?, u8::try_from(digits(s, 5..7)?).ok()?, u8::try_from(digits(s, 8..10)?).ok()?)
    }

    /// `2026-08-14`.
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Days since 1970-01-01 (negative before it): Howard Hinnant's `days_from_civil`.
    pub fn days_since_epoch(&self) -> i64 {
        let (m, d) = (i64::from(self.month), i64::from(self.day));
        let y = i64::from(self.year) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The day `delta` steps of `part` away: years stop at the limits, months and days go round
    /// within the year and the month, and a day past the end of its new month becomes its last.
    pub fn step(self, part: DatePart, delta: i32) -> PickedDate {
        let delta = i64::from(delta);
        let (mut year, mut month, mut day) = (self.year, self.month, self.day);
        match part {
            DatePart::Year => {
                let y = i64::from(year).saturating_add(delta).clamp(i64::from(MIN_YEAR), i64::from(MAX_YEAR));
                year = u16::try_from(y).unwrap_or(year);
            }
            DatePart::Month => month = wrap(month, 1, 12, delta),
            DatePart::Day => day = wrap(day, 1, days_in_month(year, month), delta),
        }
        day = day.min(days_in_month(year, month)).max(1);
        PickedDate::new(year, month, day).unwrap_or(self)
    }
}

/// A time of day to the second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PickedTime {
    hour: u8,
    minute: u8,
    second: u8,
}

impl PickedTime {
    /// The time, or `None` past 23:59:59.
    pub fn new(hour: u8, minute: u8, second: u8) -> Option<PickedTime> {
        (hour < 24 && minute < 60 && second < 60).then_some(PickedTime { hour, minute, second })
    }

    pub fn hour(&self) -> u8 {
        self.hour
    }

    pub fn minute(&self) -> u8 {
        self.minute
    }

    pub fn second(&self) -> u8 {
        self.second
    }

    /// Reads `HH:MM:SS`, or `HH:MM` for a whole minute (spaces around it are fine); `None` for
    /// anything else or a time past 23:59:59.
    pub fn parse(s: &str) -> Option<PickedTime> {
        let s = s.trim();
        let second = match s.len() {
            5 => 0,
            8 if s.get(5..6) == Some(":") => digits(s, 6..8)?,
            _ => return None,
        };
        if s.get(2..3) != Some(":") {
            return None;
        }
        PickedTime::new(u8::try_from(digits(s, 0..2)?).ok()?, u8::try_from(digits(s, 3..5)?).ok()?, u8::try_from(second).ok()?)
    }

    /// `09:05:00`.
    pub fn iso(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// Seconds since midnight.
    pub fn seconds_of_day(&self) -> u32 {
        u32::from(self.hour) * 3600 + u32::from(self.minute) * 60 + u32::from(self.second)
    }

    /// The time `delta` steps of `part` away, each part going round within its range.
    pub fn step(self, part: TimePart, delta: i32) -> PickedTime {
        let delta = i64::from(delta);
        let mut t = self;
        match part {
            TimePart::Hour => t.hour = wrap(t.hour, 0, 23, delta),
            TimePart::Minute => t.minute = wrap(t.minute, 0, 59, delta),
            TimePart::Second => t.second = wrap(t.second, 0, 59, delta),
        }
        t
    }
}

/// `value` moved `delta` steps round the range `low..=high`.
fn wrap(value: u8, low: u8, high: u8, delta: i64) -> u8 {
    if high < low {
        return value;
    }
    let span = i64::from(high) - i64::from(low) + 1;
    let at = (i64::from(value) - i64::from(low)).rem_euclid(span);
    let moved = at.saturating_add(delta.rem_euclid(span)).rem_euclid(span) + i64::from(low);
    u8::try_from(moved).unwrap_or(value)
}

/// The ASCII digits of `s` in `range`, as a number.
fn digits(s: &str, range: Range<usize>) -> Option<u32> {
    let part = s.get(range)?;
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// A value a field edits part by part.
trait Parts: Copy + PartialEq {
    type Part: Copy;
    fn read(text: &str) -> Option<Self>;
    fn write(&self) -> String;
    /// The part a cursor at character `at` is in, and that part's characters.
    fn part_at(at: usize) -> (Self::Part, Range<usize>);
    fn stepped(self, part: Self::Part, delta: i32) -> Self;
    /// The value as a screen reader says it, when that is more than its text.
    fn spoken(&self, _texts: &Texts) -> Option<String> {
        None
    }
}

impl Parts for PickedDate {
    type Part = DatePart;

    fn read(text: &str) -> Option<Self> {
        PickedDate::parse(text)
    }

    fn write(&self) -> String {
        self.iso()
    }

    fn part_at(at: usize) -> (DatePart, Range<usize>) {
        match at {
            0..=4 => (DatePart::Year, 0..4),
            5..=7 => (DatePart::Month, 5..7),
            _ => (DatePart::Day, 8..10),
        }
    }

    fn stepped(self, part: DatePart, delta: i32) -> Self {
        self.step(part, delta)
    }

    fn spoken(&self, texts: &Texts) -> Option<String> {
        Some(texts.long_date(self.year, self.month, self.day)).filter(|s| !s.is_empty())
    }
}

impl Parts for PickedTime {
    type Part = TimePart;

    fn read(text: &str) -> Option<Self> {
        PickedTime::parse(text)
    }

    fn write(&self) -> String {
        self.iso()
    }

    fn part_at(at: usize) -> (TimePart, Range<usize>) {
        match at {
            0..=2 => (TimePart::Hour, 0..2),
            3..=5 => (TimePart::Minute, 3..5),
            _ => (TimePart::Second, 6..8),
        }
    }

    fn stepped(self, part: TimePart, delta: i32) -> Self {
        self.step(part, delta)
    }
}

/// What a date or time field did this pass.
#[derive(Debug, Clone)]
pub struct PickerOut {
    /// The text field's output (its response, frame and how an edit ended).
    pub field: FieldOut,
    /// The value changed this pass: typed, stepped, or given back by Esc.
    pub changed: bool,
    /// The text reads as a value now; while it does not, the field is outlined in `caution`.
    pub valid: bool,
}

/// A date field `width` wide, registered as `id`, editing `value`; see the module docs.
pub fn date_field(ui: &mut Ui, t: &Tokens, id: &str, value: &mut PickedDate, width: f32) -> PickerOut {
    picker(ui, t, id, DATE_HINT, width, value)
}

/// A time field `width` wide, registered as `id`, editing `value`; see the module docs.
pub fn time_field(ui: &mut Ui, t: &Tokens, id: &str, value: &mut PickedTime, width: f32) -> PickerOut {
    picker(ui, t, id, TIME_HINT, width, value)
}

fn picker<V: Parts>(ui: &mut Ui, t: &Tokens, id: &str, hint: &str, width: f32, value: &mut V) -> PickerOut {
    let focus = egui_id(id);
    let memo = focus.with("picker");
    let focused = ui.memory(|m| m.has_focus(focus));
    // The text being typed lives here while the field has the focus; otherwise it is the value.
    let mut text = if focused { ui.data(|d| d.get_temp::<String>(memo)).unwrap_or_else(|| value.write()) } else { value.write() };
    let mut changed = false;
    if focused && ui.is_enabled() {
        let (up, down) =
            ui.input_mut(|i| (i.count_and_consume_key(Modifiers::NONE, Key::ArrowUp), i.count_and_consume_key(Modifiers::NONE, Key::ArrowDown)));
        if up + down > 0 {
            let delta = i32::try_from(up).unwrap_or(i32::MAX).saturating_sub(i32::try_from(down).unwrap_or(i32::MAX));
            let at = TextEditState::load(ui.ctx(), focus).and_then(|s| s.cursor.char_range()).map_or(0, |r| usize::from(r.primary.index));
            let (part, span) = V::part_at(at);
            // From what is typed when it reads, else from the value.
            let next = V::read(&text).unwrap_or(*value).stepped(part, delta);
            if next != *value {
                *value = next;
                changed = true;
            }
            text = next.write();
            let mut state = TextEditState::load(ui.ctx(), focus).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(span.start), CCursor::new(span.end))));
            state.store(ui.ctx(), focus);
        }
    }
    // Wide enough for a date or time in the font in use (the widths are the Small font's).
    let digits: String = hint.chars().map(|c| if c.is_alphabetic() { '0' } else { c }).collect();
    let fits = ui.painter().layout_no_wrap(digits, crate::fonts::regular(t.body), t.text).size().x + 18.0;
    let field = TextField::new(id, hint, width.max(fits)).show(ui, t, &mut text);
    let read = V::read(&text);
    if field.response.changed()
        && let Some(v) = read
        && v != *value
    {
        *value = v;
        changed = true;
    }
    if field.ending.is_some() || !field.editing {
        // The edit is over: what did not read is refused, and the field shows the value again.
        ui.data_mut(|d| d.remove::<String>(memo));
    } else {
        ui.data_mut(|d| d.insert_temp(memo, text));
    }
    let valid = read.is_some();
    // The long date ("Monday, October 19, 2026"), in the names the app's text table gives.
    if let Some(spoken) = value.spoken(&text::texts(ui.ctx())) {
        access(&field.response, |node| node.set_description(spoken));
    }
    if field.editing && !valid {
        ui.painter().rect_stroke(field.rect, CornerRadius::same(t.radius), Stroke::new(t.focus_width, t.caution), StrokeKind::Inside);
    }
    PickerOut { field, changed, valid }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_stays_in_range() {
        assert_eq!((wrap(12, 1, 12, 1), wrap(1, 1, 12, -1), wrap(5, 1, 12, 24)), (1, 12, 5));
        assert_eq!((wrap(0, 0, 59, -1), wrap(59, 0, 59, 1), wrap(30, 0, 59, i64::MAX)), (59, 0, 37));
        assert_eq!(wrap(30, 0, 59, i64::MIN), 22);
        assert_eq!(wrap(3, 5, 1, 1), 3, "an empty range leaves the value alone");
    }

    #[test]
    fn parts_follow_the_cursor() {
        assert_eq!(<PickedDate as Parts>::part_at(0).0, DatePart::Year);
        assert_eq!(<PickedDate as Parts>::part_at(4).0, DatePart::Year);
        assert_eq!(<PickedDate as Parts>::part_at(5).0, DatePart::Month);
        assert_eq!(<PickedDate as Parts>::part_at(8).0, DatePart::Day);
        assert_eq!(<PickedDate as Parts>::part_at(usize::MAX).0, DatePart::Day);
        assert_eq!(<PickedTime as Parts>::part_at(2).0, TimePart::Hour);
        assert_eq!(<PickedTime as Parts>::part_at(3).0, TimePart::Minute);
        assert_eq!(<PickedTime as Parts>::part_at(8).0, TimePart::Second);
    }
}
