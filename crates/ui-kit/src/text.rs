//! The few texts `ui-kit` draws or speaks itself (`localization.md`): the month and weekday names
//! of a long date, the colour picker's labels, the text menu, the names of the small buttons inside
//! widgets (Clear search, Zoom in, Mute, …). They live in one table, [`Texts`], that the app sets on
//! its [`egui::Context`] with [`set`]; until it does, every widget speaks English. `ui` builds the
//! table from its catalogs: [`Texts::translated`] asks for each [`Text`] by its English and its
//! context, so a catalog keyed by English text finds them as it finds its own.
//!
//! The table belongs to the context, not the process, so two windows (or two tests running side
//! by side) can speak two languages. Reading it is a clone of an `Arc`, never work per widget.
//!
//! Templates hold named placeholders in braces (`{name}`), filled by [`fill`]; a translation may
//! move them but must keep their names.

use std::sync::Arc;

use egui::{Context, Id};

macro_rules! texts {
    ($( $(#[$doc:meta])* $name:ident => ($context:literal, $english:literal), )*) => {
        /// One text of the table.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Text {
            $( $(#[$doc])* $name, )*
        }

        impl Text {
            /// Every text, in table order.
            pub const ALL: &'static [Text] = &[$(Text::$name),*];

            /// What the text is, for a translator and for a catalog that keys by English text
            /// with a disambiguating context (`May` the month, not the verb).
            pub fn context(self) -> &'static str {
                match self {
                    $(Text::$name => $context,)*
                }
            }

            /// The English text, what every widget says until the app sets a table.
            pub fn english(self) -> &'static str {
                match self {
                    $(Text::$name => $english,)*
                }
            }
        }
    };
}

texts! {
    January => ("month", "January"),
    February => ("month", "February"),
    March => ("month", "March"),
    April => ("month", "April"),
    May => ("month", "May"),
    June => ("month", "June"),
    July => ("month", "July"),
    August => ("month", "August"),
    September => ("month", "September"),
    October => ("month", "October"),
    November => ("month", "November"),
    December => ("month", "December"),
    Sunday => ("weekday", "Sunday"),
    Monday => ("weekday", "Monday"),
    Tuesday => ("weekday", "Tuesday"),
    Wednesday => ("weekday", "Wednesday"),
    Thursday => ("weekday", "Thursday"),
    Friday => ("weekday", "Friday"),
    Saturday => ("weekday", "Saturday"),
    /// A long date, the date field's spoken value: `{weekday}`, `{month}`, `{day}`, `{year}`.
    LongDate => ("long date", "{weekday}, {month} {day}, {year}"),
    /// The colour picker's saturation and brightness field.
    SaturationBrightness => ("colour picker", "Saturation and brightness"),
    /// The colour picker's hue strip.
    Hue => ("colour picker", "Hue"),
    /// The colour picker's eyedropper.
    PickColor => ("colour picker", "Pick a color from the photo"),
    /// The colour picker's hex field.
    HexColor => ("colour picker", "Hex color"),
    Cut => ("text menu", "Cut"),
    Copy => ("text menu", "Copy"),
    Paste => ("text menu", "Paste"),
    SelectAll => ("text menu", "Select All"),
    ClearSearch => ("search field", "Clear search"),
    Cancel => ("progress row", "Cancel"),
    MoreTabs => ("tab strip", "More tabs"),
    Close => ("room tab", "Close"),
    /// A room tab's close button: `{name}` is the room's.
    CloseNamed => ("room tab", "Close {name}"),
    ZoomIn => ("map", "Zoom in"),
    ZoomOut => ("map", "Zoom out"),
    Pin => ("map", "Pin"),
    DropPin => ("map", "Drop a pin"),
    Map => ("map", "Map"),
    /// The map's spoken zoom: `{zoom}` is the level.
    MapZoom => ("map", "Zoom level {zoom}"),
    Mute => ("video", "Mute"),
    Unmute => ("video", "Unmute"),
    Volume => ("video", "Volume"),
    Position => ("video", "Position"),
    TrimStart => ("video", "Trim start"),
    TrimEnd => ("video", "Trim end"),
    /// The player's readout of a trimmed clip: `{time}` ("00:00:09 / 00:00:41"), `{length}` the
    /// length kept.
    Trimmed => ("video", "{time} (trimmed to {length})"),
    Decrease => ("stepper", "Decrease"),
    Increase => ("stepper", "Increase"),
    TopLeftCorner => ("face box handle", "Top-left corner"),
    TopEdge => ("face box handle", "Top edge"),
    TopRightCorner => ("face box handle", "Top-right corner"),
    RightEdge => ("face box handle", "Right edge"),
    BottomRightCorner => ("face box handle", "Bottom-right corner"),
    BottomEdge => ("face box handle", "Bottom edge"),
    BottomLeftCorner => ("face box handle", "Bottom-left corner"),
    LeftEdge => ("face box handle", "Left edge"),
    Ignore => ("face", "Ignore"),
    IgnoreFace => ("face", "Ignore this face"),
    UnnamedFace => ("face", "Unnamed face"),
    Face => ("face", "Face"),
    Confirm => ("face", "Confirm"),
    ConfirmMatch => ("face", "Confirm this match"),
    Reject => ("face", "Reject"),
    NotThisPerson => ("face", "Not this person"),
    SelectGroup => ("group header", "Select this group"),
    SelectGroupDetail => ("group header", "Selects every picture taken in this period"),
    ShowAll => ("filter banner", "Show All"),
    LeaveSearch => ("filter banner", "Leave search"),
    Splitter => ("splitter", "Splitter"),
    Crosshair => ("effect", "Crosshair"),
    Angle => ("effect", "Angle"),
    TurnAndScale => ("collage", "Turn and scale"),
    /// The turn-and-scale handle's spoken value: `{angle}` in degrees, `{scale}` in per cent.
    TurnedScaled => ("collage", "Turned {angle}°, scaled {scale} %"),
    Star => ("star toggle", "Star"),
    StarUnstar => ("star toggle", "Star / Unstar"),
    RecordingKey => ("shortcut editor", "Press the new shortcut…"),
    NoShortcut => ("shortcut editor", "No shortcut"),
    Histogram => ("edit panel", "Histogram"),
    Controls => ("overlay", "Controls"),
}

/// The table of [`Text`]s, English unless the app gave translations.
#[derive(Debug, Clone, PartialEq)]
pub struct Texts {
    /// One per [`Text::ALL`], in its order.
    strings: Vec<String>,
}

impl Default for Texts {
    fn default() -> Self {
        Texts::english()
    }
}

impl Texts {
    /// Every text in English.
    pub fn english() -> Texts {
        Texts { strings: Text::ALL.iter().map(|t| t.english().to_owned()).collect() }
    }

    /// The table `lookup` translates: each [`Text`] it answers with a non-empty string takes it,
    /// the rest stay English.
    pub fn translated(mut lookup: impl FnMut(Text) -> Option<String>) -> Texts {
        let strings = Text::ALL.iter().map(|&t| lookup(t).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| t.english().to_owned())).collect();
        Texts { strings }
    }

    /// The same table with `text` saying `value` (English again for an empty value).
    pub fn with(mut self, text: Text, value: impl Into<String>) -> Texts {
        let value = value.into();
        if let Some(slot) = self.strings.get_mut(text as usize) {
            *slot = if value.trim().is_empty() { text.english().to_owned() } else { value };
        }
        self
    }

    /// What `text` says.
    pub fn get(&self, text: Text) -> &str {
        self.strings.get(text as usize).map_or(text.english(), String::as_str)
    }

    /// The name of `month` (1 = January … 12); empty for a month that does not exist.
    pub fn month(&self, month: u8) -> &str {
        const MONTHS: [Text; 12] = [
            Text::January,
            Text::February,
            Text::March,
            Text::April,
            Text::May,
            Text::June,
            Text::July,
            Text::August,
            Text::September,
            Text::October,
            Text::November,
            Text::December,
        ];
        MONTHS.get(usize::from(month).wrapping_sub(1)).map_or("", |&t| self.get(t))
    }

    /// The name of `weekday` (0 = Sunday … 6 = Saturday); empty for one that does not exist.
    pub fn weekday(&self, weekday: u8) -> &str {
        const DAYS: [Text; 7] = [Text::Sunday, Text::Monday, Text::Tuesday, Text::Wednesday, Text::Thursday, Text::Friday, Text::Saturday];
        DAYS.get(usize::from(weekday)).map_or("", |&t| self.get(t))
    }

    /// `year-month-day` as a long date ("Monday, October 19, 2026" in English); empty for a day
    /// that does not exist.
    pub fn long_date(&self, year: u16, month: u8, day: u8) -> String {
        let Some(weekday) = weekday_of(year, month, day) else { return String::new() };
        fill(
            self.get(Text::LongDate),
            &[("weekday", self.weekday(weekday)), ("month", self.month(month)), ("day", &day.to_string()), ("year", &year.to_string())],
        )
    }
}

/// The day of the week of `year-month-day` in the proleptic Gregorian calendar (0 = Sunday), or
/// `None` for a day that does not exist.
pub fn weekday_of(year: u16, month: u8, day: u8) -> Option<u8> {
    if year == 0 || day == 0 || day > crate::widgets::days_in_month(year, month) {
        return None;
    }
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let offset = *OFFSETS.get(usize::from(month).checked_sub(1)?)?;
    let y = i32::from(year) - i32::from(month < 3);
    u8::try_from((y + y / 4 - y / 100 + y / 400 + offset + i32::from(day)).rem_euclid(7)).ok()
}

/// `template` with each `{name}` of `values` replaced by its value; other braces stay as they are.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

fn key() -> Id {
    Id::new("dalica-ui-kit.text")
}

/// Sets the table every widget drawn in `ctx` speaks from now on.
pub fn set(ctx: &Context, texts: Texts) {
    ctx.data_mut(|d| d.insert_temp(key(), Arc::new(texts)));
}

/// The table of `ctx`: the one [`set`] gave, else English.
pub fn texts(ctx: &Context) -> Arc<Texts> {
    ctx.data(|d| d.get_temp::<Arc<Texts>>(key())).unwrap_or_else(|| Arc::new(Texts::english()))
}

/// What `text` says in `ctx`.
pub fn get(ctx: &Context, text: Text) -> String {
    texts(ctx).get(text).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_english_until_set_and_keeps_english_where_a_translation_is_missing() {
        let ctx = Context::default();
        assert_eq!(get(&ctx, Text::Hue), "Hue");
        set(&ctx, Texts::translated(|t| (t == Text::Hue).then(|| "Teinte".to_owned())));
        assert_eq!(get(&ctx, Text::Hue), "Teinte");
        assert_eq!(get(&ctx, Text::Mute), "Mute");
        // An empty answer is no answer.
        assert_eq!(Texts::english().with(Text::Mute, " ").get(Text::Mute), "Mute");
        // Another context is untouched.
        assert_eq!(get(&Context::default(), Text::Hue), "Hue");
    }

    #[test]
    fn long_dates_name_the_weekday_and_month_and_refuse_days_that_do_not_exist() {
        let t = Texts::english();
        assert_eq!(t.long_date(2026, 10, 19), "Monday, October 19, 2026");
        assert_eq!(t.long_date(2000, 2, 29), "Tuesday, February 29, 2000");
        assert_eq!(t.long_date(1, 1, 1), "Monday, January 1, 1");
        for (y, m, d) in [(2026, 2, 29), (2026, 13, 1), (2026, 0, 1), (0, 1, 1), (2026, 1, 0), (u16::MAX, u8::MAX, u8::MAX)] {
            assert_eq!(t.long_date(y, m, d), "", "{y}-{m}-{d}");
        }
        assert_eq!(t.month(0), "");
        assert_eq!(t.weekday(7), "");
        let fr = t.with(Text::LongDate, "{weekday} {day} {month} {year}").with(Text::October, "octobre").with(Text::Monday, "lundi");
        assert_eq!(fr.long_date(2026, 10, 19), "lundi 19 octobre 2026");
    }

    #[test]
    fn every_text_has_an_english_text_and_a_context_and_templates_keep_their_names() {
        assert_eq!(Texts::english().strings.len(), Text::ALL.len());
        for (i, &t) in Text::ALL.iter().enumerate() {
            assert_eq!(t as usize, i);
            assert!(!t.english().trim().is_empty() && !t.context().trim().is_empty(), "{t:?}");
        }
        assert_eq!(fill("Close {name}", &[("name", "Collage")]), "Close Collage");
        assert_eq!(fill("{a} {b} {a}", &[("a", "x")]), "x {b} x");
    }
}
