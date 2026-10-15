//! The date and time fields (`design-system.md` §6, `organizing.md` §9), driven headlessly as a
//! user would: clicks, keys and typed text through egui's input.
//!
//! - the date rules: month lengths, reading a date or a time strictly, the limits of years 1 to
//!   9999, stepping a part (stopping at the limits, going round within a range, a day cut to its
//!   month); ported from LightCraft `crates/ui-egui/src/date_picker.rs` at revision 034ef489 (MIT
//!   OR Apache-2.0; `NOTICE`), with Dalica's limits and parts;
//! - entry: typing a date and a time sets the values; Esc gives back the ones from before;
//! - stepping: ↑ and ↓ step the part under the cursor and keep it selected;
//! - refusal: 31 February never becomes the value, the field says so in `caution`, and the text goes
//!   back when the edit ends;
//! - the look: a sheet of states (`fingerprints/datetime-states.json`) and a window
//!   with the fields at its bottom-right corner (`fingerprints/datetime-window-1400x900.json`). The
//!   renders go to `target/test-output/ui-kit/`; `DALICA_BLESS=1` stores new fingerprints (commit
//!   them only after looking at the PNGs).

use crate::common::{check, out_dir};
use dalica_ui_kit::widgets::{DatePart, PickedDate, PickedTime, TimePart, date_field, days_in_month, time_field};
use dalica_ui_kit::{Image, Rasterizer, Tokens, WidgetInfo, fonts, registry};
use egui::{Align2, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect, UiBuilder, pos2, vec2};

const DATE: &str = "field:test.date";
const TIME: &str = "field:test.time";

fn date(s: &str) -> PickedDate {
    PickedDate::parse(s).unwrap_or_else(|| panic!("{s} is a date"))
}

fn time(s: &str) -> PickedTime {
    PickedTime::parse(s).unwrap_or_else(|| panic!("{s} is a time"))
}

// ---- The rules (ported from LightCraft) ----

#[test]
fn month_lengths() {
    assert_eq!((days_in_month(2026, 2), days_in_month(2028, 2), days_in_month(1900, 2), days_in_month(2000, 2)), (28, 29, 28, 29));
    assert_eq!((days_in_month(2026, 4), days_in_month(2026, 12), days_in_month(2026, 13), days_in_month(2026, 0)), (30, 31, 0, 0));
}

#[test]
fn dates_read_and_write_iso_within_the_limits() {
    assert_eq!(PickedDate::parse("2026-08-14"), PickedDate::new(2026, 8, 14));
    assert_eq!(PickedDate::parse(" 2026-08-14 "), PickedDate::new(2026, 8, 14), "spaces around it are fine");
    for bad in [
        "",
        "2026",
        "2026-08",
        "26-08-14",
        "2026-8-14",
        "2026-13-01",
        "2026-00-10",
        "2026-02-30",
        "2026-02-31",
        "2027-02-29",
        "2026-04-31",
        "2026/08/14",
        "2026-08-14T10:00",
        "0000-01-01",
        "+026-08-14",
        "2026-08-1x",
        "banana",
        "２０２６-08-14",
    ] {
        assert_eq!(PickedDate::parse(bad), None, "{bad}");
    }
    for s in ["2026-08-14", "0001-01-01", "9999-12-31", "2000-02-29"] {
        assert_eq!(PickedDate::parse(s).map(|d| d.iso()).as_deref(), Some(s));
    }
    assert_eq!((PickedDate::new(0, 1, 1), PickedDate::new(10_000, 1, 1), PickedDate::new(2026, 2, 29)), (None, None, None));
}

#[test]
fn times_read_and_write() {
    assert_eq!(PickedTime::parse("09:05:07"), PickedTime::new(9, 5, 7));
    assert_eq!(PickedTime::parse("23:59"), PickedTime::new(23, 59, 0), "a whole minute");
    for bad in ["", "9:05", "24:00:00", "12:60:00", "12:00:60", "12:00:0", "12-00-00", "12:00:00Z", "noon"] {
        assert_eq!(PickedTime::parse(bad), None, "{bad}");
    }
    assert_eq!(time("07:30:00").iso(), "07:30:00");
    assert_eq!((time("01:30:05").seconds_of_day(), time("00:00:00").seconds_of_day(), time("23:59:59").seconds_of_day()), (5405, 0, 86_399));
}

/// Days since 1970 match the calendar, both ways of the epoch and across leap days.
#[test]
fn days_since_the_epoch() {
    assert_eq!(date("1970-01-01").days_since_epoch(), 0);
    assert_eq!(date("1969-12-31").days_since_epoch(), -1);
    assert_eq!(date("2000-03-01").days_since_epoch() - date("2000-02-28").days_since_epoch(), 2, "2000 was a leap year");
    assert_eq!(date("1900-03-01").days_since_epoch() - date("1900-02-28").days_since_epoch(), 1, "1900 was not");
    assert_eq!(date("2026-10-12").days_since_epoch(), 20_738);
    assert_eq!(date("9999-12-31").days_since_epoch() - date("0001-01-01").days_since_epoch(), 3_652_058);
}

/// Stepping stops at years 1 and 9999 instead of wrapping, goes round within a month and a year,
/// and cuts a day to the end of its new month.
#[test]
fn stepping_stops_at_the_limits_and_keeps_days_real() {
    let step = |s: &str, part, delta| date(s).step(part, delta).iso();
    assert_eq!(step("2026-08-14", DatePart::Year, 1), "2027-08-14");
    assert_eq!(step("9999-12-31", DatePart::Year, 1), "9999-12-31", "the last year");
    assert_eq!(step("0001-01-01", DatePart::Year, -1), "0001-01-01", "the first year");
    assert_eq!(step("2026-08-14", DatePart::Year, i32::MAX), "9999-08-14");
    assert_eq!(step("2026-08-14", DatePart::Year, i32::MIN), "0001-08-14");
    assert_eq!(step("2026-12-14", DatePart::Month, 1), "2026-01-14", "months go round within the year");
    assert_eq!(step("2026-01-14", DatePart::Month, -1), "2026-12-14");
    assert_eq!(step("2026-01-31", DatePart::Month, 1), "2026-02-28", "31 January, a month on");
    assert_eq!(step("2028-02-29", DatePart::Year, 1), "2029-02-28", "a leap day, a year on");
    assert_eq!(step("2026-02-28", DatePart::Day, 1), "2026-02-01", "days go round within the month");
    assert_eq!(step("2026-03-01", DatePart::Day, -1), "2026-03-31");
    assert_eq!(step("2026-03-15", DatePart::Day, i32::MIN), date("2026-03-15").step(DatePart::Day, i32::MIN % 31).iso());
    let tstep = |s: &str, part, delta| time(s).step(part, delta).iso();
    assert_eq!(tstep("23:59:59", TimePart::Hour, 1), "00:59:59");
    assert_eq!(tstep("23:59:59", TimePart::Minute, 1), "23:00:59", "no carry into the hour");
    assert_eq!(tstep("00:00:00", TimePart::Second, -1), "00:00:59");
    assert_eq!(tstep("10:00:00", TimePart::Minute, 90), "10:30:00");
}

// ---- The fields, driven ----

/// A date field and a time field side by side in a window, with room around them to click away.
struct Rig {
    ctx: Context,
    t: Tokens,
    size: (u32, u32),
    at: Pos2,
    date: PickedDate,
    time: PickedTime,
    enabled: bool,
    caption: Option<&'static str>,
    /// What the fields said last pass: (changed, valid) for the date and the time.
    date_out: (bool, bool),
    time_out: (bool, bool),
    /// Passes in which a field said its value changed.
    changes: usize,
    clock: f64,
    widgets: Vec<WidgetInfo>,
    raster: Rasterizer,
}

impl Rig {
    fn new() -> Rig {
        Rig::with((400, 160), pos2(10.0, 30.0))
    }

    fn with(size: (u32, u32), at: Pos2) -> Rig {
        let t = Tokens::default();
        let ctx = Context::default();
        t.apply(&ctx);
        let mut rig = Rig {
            ctx,
            t,
            size,
            at,
            date: date("2026-06-04"),
            time: time("10:00:00"),
            enabled: true,
            caption: None,
            date_out: (false, true),
            time_out: (false, true),
            changes: 0,
            clock: 0.0,
            widgets: vec![],
            raster: Rasterizer::new(),
        };
        rig.settle();
        rig
    }

    fn pass(&mut self, events: Vec<Event>) -> egui::FullOutput {
        self.clock += 1.0 / 60.0;
        let mut raw = dalica_ui_kit::raw_input(self.size.0, self.size.1, 1.0);
        raw.time = Some(self.clock);
        raw.events = events;
        let Rig { ctx, t, at, date, time, enabled, caption, date_out, time_out, changes, .. } = self;
        let out = ctx.run_ui(raw, |ui| {
            if let Some(c) = caption {
                ui.painter().text(pos2(6.0, 3.0), Align2::LEFT_TOP, *c, fonts::regular(t.small), t.text_dim);
            }
            let rect = Rect::from_min_size(*at, vec2(200.0, 30.0));
            ui.scope_builder(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                ui.add_enabled_ui(*enabled, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let d = date_field(ui, t, DATE, date, 96.0);
                    let tm = time_field(ui, t, TIME, time, 80.0);
                    *date_out = (d.changed, d.valid);
                    *time_out = (tm.changed, tm.valid);
                    *changes += usize::from(d.changed) + usize::from(tm.changed);
                });
            });
        });
        self.widgets = registry::widgets(&self.ctx);
        out
    }

    fn frame(&mut self, events: Vec<Event>) {
        let mut delta = self.pass(events).textures_delta;
        self.raster.set_textures(&mut delta);
        self.raster.free_textures(&mut delta);
    }

    fn render(&mut self) -> Image {
        let out = self.pass(vec![]);
        dalica_ui_kit::paint(&self.ctx, &mut self.raster, out, self.size.0, self.size.1, self.t.chrome).unwrap()
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }

    fn rect(&self, id: &str) -> Rect {
        let w = self.widgets.iter().rev().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id} on screen"));
        Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3]))
    }

    fn click_at(&mut self, at: Pos2) {
        self.clock += 1.0;
        let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(at), press(true)]);
        self.frame(vec![press(false)]);
        self.settle();
    }

    fn click(&mut self, id: &str) {
        let r = self.rect(id);
        self.click_at(r.center());
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        self.frame(vec![ev(true), ev(false)]);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_string())]);
        self.settle();
    }

    /// Click into `id`, select its text and type `text` over it.
    fn retype(&mut self, id: &str, text: &str) {
        self.click(id);
        self.key(Key::A, Modifiers::COMMAND);
        self.type_text(text);
    }

    /// Click into `id` and put the cursor `n` characters from its start.
    fn cursor_at(&mut self, id: &str, n: usize) {
        self.click(id);
        self.key(Key::Home, Modifiers::NONE);
        for _ in 0..n {
            self.key(Key::ArrowRight, Modifiers::NONE);
        }
    }

    fn focused(&self) -> bool {
        self.ctx.memory(|m| m.focused()).is_some()
    }

    /// The selected characters of field `id`, as a range.
    fn selection(&self, id: &str) -> Option<(usize, usize)> {
        let state = egui::text_edit::TextEditState::load(&self.ctx, egui::Id::new(id))?;
        let r = state.cursor.char_range()?;
        let (a, b) = (usize::from(r.primary.index), usize::from(r.secondary.index));
        Some((a.min(b), a.max(b)))
    }
}

/// Typing a date and a time sets them; Return keeps them; Esc gives back the values from before
/// the edit, whatever was typed or stepped meanwhile.
#[test]
fn entry_sets_the_values_and_esc_gives_them_back() {
    let mut rig = Rig::new();
    rig.retype(DATE, "2026-08-14");
    assert_eq!(rig.date, date("2026-08-14"), "the value follows the text as soon as it reads");
    rig.key(Key::Enter, Modifiers::NONE);
    assert!(!rig.focused());
    assert_eq!(rig.date, date("2026-08-14"));
    rig.retype(TIME, "07:45:30");
    assert_eq!(rig.time, time("07:45:30"));
    rig.key(Key::Tab, Modifiers::NONE);
    rig.retype(TIME, "19:05");
    assert_eq!(rig.time, time("19:05:00"), "a whole minute");
    rig.key(Key::Enter, Modifiers::NONE);

    // Esc: the values from before the edit.
    rig.retype(DATE, "1999-12-31");
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(rig.date, date("1999-12-01"), "the day stepped (the cursor is at the end)");
    rig.key(Key::Escape, Modifiers::NONE);
    assert_eq!(rig.date, date("2026-08-14"));
    assert!(!rig.focused());
}

/// ↑ and ↓ step the part the cursor is in and select it, so they keep stepping it; the time field
/// steps the same way.
#[test]
fn arrows_step_the_part_under_the_cursor() {
    let mut rig = Rig::new();
    rig.cursor_at(DATE, 6);
    let before = rig.changes;
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(rig.date, date("2026-07-04"), "the month");
    assert_eq!(rig.selection(DATE), Some((5, 7)), "the month is selected");
    assert!(rig.changes > before, "the field says it changed");
    rig.key(Key::ArrowUp, Modifiers::NONE);
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(rig.date, date("2026-09-04"));
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(rig.date, date("2026-08-04"));
    rig.key(Key::Home, Modifiers::NONE);
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(rig.date, date("2025-08-04"), "the year");
    assert_eq!(rig.selection(DATE), Some((0, 4)));
    rig.key(Key::End, Modifiers::NONE);
    rig.key(Key::ArrowDown, Modifiers::NONE);
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(rig.date, date("2025-08-02"), "the day");
    rig.key(Key::Enter, Modifiers::NONE);

    rig.cursor_at(TIME, 0);
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(rig.time, time("09:00:00"), "the hour");
    rig.key(Key::End, Modifiers::NONE);
    rig.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(rig.time, time("09:00:59"), "seconds go round, no carry");
    rig.cursor_at(TIME, 4);
    for _ in 0..30 {
        rig.key(Key::ArrowUp, Modifiers::NONE);
    }
    assert_eq!(rig.time, time("09:30:59"));

    // Not focused, the arrows do nothing to the fields.
    rig.key(Key::Enter, Modifiers::NONE);
    rig.click_at(pos2(350.0, 140.0));
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!((rig.date, rig.time), (date("2025-08-02"), time("09:30:59")));

    // Disabled, they take no focus and step nothing.
    rig.enabled = false;
    rig.settle();
    rig.click(DATE);
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(rig.date, date("2025-08-02"));
    assert!(!rig.focused());
}

/// 31 February never becomes the value: the field reads as not valid while it shows, and the text
/// goes back to the value when the edit ends; the arrows step on from the value.
#[test]
fn the_31st_of_february_is_refused() {
    let mut rig = Rig::new();
    rig.retype(DATE, "2026-02-28");
    assert_eq!(rig.date, date("2026-02-28"));
    rig.retype(DATE, "2026-02-31");
    assert_eq!(rig.date, date("2026-02-28"), "refused");
    assert_eq!(rig.date_out, (false, false), "the field says the text does not read");
    rig.key(Key::Enter, Modifiers::NONE);
    assert_eq!(rig.date, date("2026-02-28"));
    assert!(rig.date_out.1, "the text is the value again");
    // Half a date, then an arrow: it steps from the value, and the text reads again.
    rig.retype(DATE, "2026-0");
    assert!(!rig.date_out.1);
    rig.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(rig.date, date("2026-03-28"), "the month (the cursor is in it), stepped from the value");
    assert!(rig.date_out.1);
    // A time that does not exist either.
    rig.retype(TIME, "25:00:00");
    assert_eq!((rig.time, rig.time_out.1), (time("10:00:00"), false));
    rig.key(Key::Tab, Modifiers::NONE);
    assert_eq!((rig.time, rig.time_out.1), (time("10:00:00"), true));
}

// ---- The look ----

const STATES: [&str; 6] = ["rest", "focused", "stepped", "invalid", "disabled", "time"];

/// One state: the fields in a tile, driven into it.
fn tile(name: &'static str) -> (Rig, Image) {
    let mut rig = Rig::with((240, 70), pos2(10.0, 24.0));
    rig.caption = Some(name);
    match name {
        "focused" => rig.cursor_at(DATE, 10),
        "stepped" => {
            rig.cursor_at(DATE, 6);
            rig.key(Key::ArrowUp, Modifiers::NONE);
        }
        "invalid" => rig.retype(DATE, "2026-02-31"),
        "disabled" => {
            rig.enabled = false;
            rig.settle();
        }
        "time" => {
            rig.cursor_at(TIME, 0);
            rig.key(Key::ArrowUp, Modifiers::NONE);
        }
        _ => {}
    }
    let image = rig.render();
    (rig, image)
}

/// The sheet of states: 3 × 2 tiles, 1 px `divider` lines between them.
fn sheet() -> (Image, Vec<(&'static str, Rig)>) {
    let tiles: Vec<(&str, Rig, Image)> = STATES
        .iter()
        .map(|&n| {
            let (rig, img) = tile(n);
            (n, rig, img)
        })
        .collect();
    let (tw, th) = (tiles[0].2.width, tiles[0].2.height);
    let (w, h) = (3 * tw + 2, 2 * th + 1);
    let mut rgba: Vec<u8> = Tokens::default().divider.to_array().repeat((w * h) as usize);
    for (i, (_, _, img)) in tiles.iter().enumerate() {
        let (cx, cy) = ((i as u32 % 3) * (tw + 1), (i as u32 / 3) * (th + 1));
        for y in 0..th {
            let src = (y * tw * 4) as usize..((y + 1) * tw * 4) as usize;
            let dst = (((cy + y) * w + cx) * 4) as usize;
            rgba[dst..dst + src.len()].copy_from_slice(&img.rgba[src]);
        }
    }
    (Image { width: w, height: h, rgba }, tiles.into_iter().map(|(n, r, _)| (n, r)).collect())
}

/// The window at `size` with the fields at its bottom-right corner, the date being stepped.
fn window(size: (u32, u32)) -> Image {
    let at = pos2(size.0 as f32 - 200.0, size.1 as f32 - 40.0);
    let mut rig = Rig::with(size, at);
    rig.cursor_at(DATE, 2);
    rig.key(Key::ArrowUp, Modifiers::NONE);
    rig.render()
}

/// Every state renders identically twice, registers its fields with valid ids, and matches its
/// fingerprint; so does the window.
#[test]
fn every_state_renders_identically_twice_and_matches_its_fingerprint() {
    let (a, rigs) = sheet();
    let (b, _) = sheet();
    assert!(a.rgba == b.rgba, "two renders of the sheet differ");
    for (name, rig) in &rigs {
        for id in [DATE, TIME] {
            assert!(rig.widgets.iter().any(|w| w.id == id), "{name}: {id} registers");
        }
        assert_eq!(registry::unregistered(&rig.ctx), [], "{name}: interactive widgets without an id");
        for w in &rig.widgets {
            assert!(registry::is_valid_id(&w.id), "{name}: {:?} breaks the id grammar", w.id);
        }
        match *name {
            "invalid" => assert!(!rig.date_out.1),
            "stepped" => assert_eq!(rig.date, date("2026-07-04")),
            "time" => assert_eq!(rig.time, time("11:00:00")),
            _ => {}
        }
    }
    let mut failures = Vec::new();
    check("datetime-states", &a, &mut failures);
    // The window at one size: one fingerprint per state (the app's tests check the layout of
    // the small window).
    let (one, two) = (window((1400, 900)), window((1400, 900)));
    assert!(one.rgba == two.rgba, "two renders of the window differ");
    check("datetime-window-1400x900", &one, &mut failures);
    assert!(failures.is_empty(), "{} (renders in {})", failures.join("\n"), out_dir().display());
}
