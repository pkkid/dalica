//! Adjust Date and Time (`organizing.md` §9): opened by `photo.adjustDate` (Tools ▸ Adjust Date and
//! Time…) for the selected photos.
//!
//! ```text
//! Adjust Picture Date — 3 pictures
//! ┌────────┐  Current date:  Thursday, June 4, 2026 10:00 AM
//! │ (photo)│  New date:      [2026-06-04] [11:30:00]  Thursday, June 4, 2026
//! └────────┘  (•) Shift all dates by the same amount
//!             ( ) Set all pictures to the same date and time
//!             Every picture moves 1 h 30 min later.
//!                                                          [Cancel] [OK]
//! ```
//!
//! The draft is `{ids, current, mode, shift, set}`: the photos; the first one's date as the
//! library sorts it (the date set by the user, else the file's capture date, else its time;
//! `null`: none), which "Current date:" shows; `"shift"` (every photo by the same amount, the
//! default) or `"set"` (all to one date and time); the shift in seconds; and the date to set
//! (`"YYYY-MM-DDTHH:MM:SS"`). "New date:" is the date the first photo gets: in shift mode the
//! current date moved by the shift, in set mode the date to set. Its date and time fields
//! (`ui-kit`'s [`date_field`] and [`time_field`]: typing, ↑ / ↓ on the part under the cursor, a day
//! that does not exist refused) keep both in step, so switching the mode keeps the new date. The
//! radios show only for several photos; one photo is shifted, which comes to the same. OK runs
//! `photo.dates.set` once (one undo step), `{ids, shift}` or `{ids, set}`; Cancel changes nothing.
//!
//! Widget ids (`control-protocol.md` §7, the dialog row): `field:dialog.date`,
//! `field:dialog.time`, `choice:dialog.mode/shift`, `choice:dialog.mode/set`,
//! `button:dialog.cancel`, `button:dialog.ok`.

use dalica_core::{Date, DateTime, PhotoId};
use dalica_edit::Dihedral;
use dalica_engine::{CmdError, Thumb};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, DATE_WIDTH, PickedDate, PickedTime, TIME_WIDTH, Target, date_field, time_field};
use dalica_ui_kit::{Tokens, fonts};
use egui::epaint::Vertex;
use egui::{Align, Color32, Id, Layout, Mesh, Rect, RichText, Sense, Shape, TextureHandle, Ui, pos2, vec2};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::panels::light_table::cell::letterbox;
use crate::panels::light_table::header::weekday;
use crate::panels::light_table::long_date;
use crate::panels::thumbnails::{recipe_orientation, turn_point};
use crate::status_text::date_time;

/// The title for one photo; several add " — N pictures".
pub const TITLE: &str = "Adjust Picture Date";
/// The dialog's width (points).
pub const WIDTH: f32 = 520.0;
/// The thumbnail's square (points).
const THUMB: f32 = 96.0;
/// The label column's width (points).
const LABEL_W: f32 = 96.0;
/// Between columns and rows.
const GAP: f32 = 8.0;
/// A radio row's height.
const CHOICE_H: f32 = 22.0;

pub const DATE_ID: &str = "field:dialog.date";
pub const TIME_ID: &str = "field:dialog.time";
pub const SHIFT_ID: &str = "choice:dialog.mode/shift";
pub const SET_ID: &str = "choice:dialog.mode/set";
/// The radios' labels.
pub const SHIFT_LABEL: &str = "Shift all dates by the same amount";
pub const SET_LABEL: &str = "Set all pictures to the same date and time";
/// "Current date:" for a photo without any date.
pub const NO_DATE: &str = "None";

/// The dialog for `ids`, its draft starting from the first photo's date.
pub fn dialog(app: &App, ids: &[PhotoId]) -> Dialog {
    let current = ids.first().and_then(|id| {
        let library = app.session()?.library();
        library.sort_date(library.photo(*id)?)
    });
    let mut d = Dialog::new(DialogKind::AdjustDate);
    let v = json!({ "ids": ids, "current": current, "mode": "shift", "shift": 0, "set": current });
    if let Value::Object(o) = v {
        d.fields = o;
    }
    d
}

/// The photos of the draft.
fn ids(fields: &Map<String, Value>) -> Vec<Value> {
    fields.get("ids").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn date_of(fields: &Map<String, Value>, key: &str) -> Option<DateTime> {
    fields.get(key).and_then(Value::as_str).and_then(|s| s.parse().ok())
}

/// The draft's mode: `true` to set one date for all, `false` to shift them (also when the draft
/// says neither).
fn sets(fields: &Map<String, Value>) -> bool {
    fields.get("mode").and_then(Value::as_str) == Some("set")
}

/// Seconds since 1970-01-01T00:00:00 of `d`, read as UTC (the inverse of
/// [`DateTime::from_unix_secs`]).
pub fn unix_secs(d: DateTime) -> i64 {
    let day = PickedDate::new(d.date().year(), d.date().month(), d.date().day()).map_or(0, |p| p.days_since_epoch());
    day * 86_400 + i64::from(d.hour()) * 3600 + i64::from(d.minute()) * 60 + i64::from(d.second())
}

/// The date the first photo gets: in shift mode its current date moved by the shift, in set mode
/// the date to set. `None` when the draft gives none (no current date to shift, no date to set,
/// a shift past the years 1 to 9999).
pub fn new_date(fields: &Map<String, Value>) -> Option<DateTime> {
    if sets(fields) {
        return date_of(fields, "set");
    }
    let shift = fields.get("shift").and_then(Value::as_i64)?;
    DateTime::from_unix_secs(unix_secs(date_of(fields, "current")?).checked_add(shift)?)
}

/// Writes `new` as the date to set and as the shift from the current date, so both modes give it.
fn put_new(fields: &mut Map<String, Value>, new: DateTime) {
    fields.insert("set".into(), json!(new));
    if let Some(current) = date_of(fields, "current") {
        fields.insert("shift".into(), json!(unix_secs(new) - unix_secs(current)));
    }
}

/// "1 day 2 h 30 min", the size of a shift of `secs` seconds ("0 min" for none).
pub fn amount(secs: i64) -> String {
    let s = secs.unsigned_abs();
    let (days, hours, minutes, seconds) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60, s % 60);
    let mut parts = Vec::with_capacity(4);
    if days > 0 {
        parts.push(if days == 1 { "1 day".to_string() } else { format!("{days} days") });
    }
    if hours > 0 {
        parts.push(format!("{hours} h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes} min"));
    }
    if seconds > 0 {
        parts.push(format!("{seconds} s"));
    }
    if parts.is_empty() { "0 min".to_string() } else { parts.join(" ") }
}

/// What a shift of `secs` seconds does to `n` photos, in words.
pub fn shift_note(secs: i64, n: usize) -> String {
    let every = if n == 1 { "The picture" } else { "Every picture" };
    match secs {
        0 => format!("{every} keeps its date."),
        s if s > 0 => format!("{every} moves {} later.", amount(s)),
        s => format!("{every} moves {} earlier.", amount(s)),
    }
}

/// "Thursday, June 4, 2026 10:00 AM".
fn long(d: DateTime) -> String {
    format!("{}, {}", weekday(d.date()), date_time(d))
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let n = ids(&d.fields).len();
    let title = if n > 1 { format!("{TITLE} — {n} pictures") } else { TITLE.to_string() };
    let ok = Button::new("ok", "OK", ButtonKind::Default).enabled(new_date(&d.fields).is_some());
    Look { title, width: WIDTH, buttons: vec![Button::cancel(), ok] }
}

/// One row: `label` in its column, then what `add` draws.
fn row<R>(ui: &mut Ui, t: &Tokens, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.allocate_ui_with_layout(vec2(LABEL_W, t.metrics.text_field_h), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(LABEL_W);
            ui.label(RichText::new(label).font(fonts::regular(t.body)).color(t.text_label));
        });
        add(ui)
    })
    .inner
}

/// A radio row: the circle and the label, the whole row clickable. Returns whether it was clicked.
fn radio(ui: &mut Ui, t: &Tokens, id: &str, label: &str, checked: bool, enabled: bool) -> bool {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, CHOICE_H), Sense::hover());
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let c = pos2(rect.min.x + 8.0, rect.center().y);
    let ring = if !enabled {
        t.text_disabled
    } else if r.hovered() {
        t.text_label
    } else {
        t.button_border
    };
    p.circle(c, 7.0, t.field, egui::Stroke::new(1.0, ring));
    if checked {
        p.circle_filled(c, 3.5, if enabled { t.accent } else { t.text_disabled });
    }
    if r.has_focus() {
        p.circle_stroke(c, 9.0, egui::Stroke::new(t.focus_width, t.accent));
    }
    let text = if enabled { t.text_label } else { t.text_disabled };
    p.text(pos2(rect.min.x + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, label, fonts::regular(t.body), text);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, checked, label));
    widgets::register(ui.ctx(), id, Role::Toggle, label, Target::Widget(&r), checked);
    r.clicked()
}

/// The texture the dialog shows its photo with: the photo, the ladder size, and the texture.
type ThumbSlot = (PhotoId, dalica_engine::thumbs::Size, TextureHandle);

/// Paints photo `id`'s thumbnail letterboxed into `rect`, turned by its recipe: the ladder image
/// the session has in memory (asked for at the visible priority when missing, never read from the
/// disk here), else a plain tile. One texture is kept for the dialog, replaced when the photo or
/// the size changes.
fn thumbnail(app: &mut App, ui: &mut Ui, id: PhotoId, rect: Rect) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let need = (rect.width() * ctx.pixels_per_point()).ceil().max(1.0) as u32;
    let Some(session) = app.session_mut() else { return };
    let thumb = session.thumbnail(id, need);
    let library = session.library();
    let Some(photo) = library.photo(id) else { return };
    let turn = recipe_orientation(&library.info(id).edits);
    let (w, h) = turn.display_size(photo.width, photo.height);
    let image = letterbox(rect, w, h);
    let painter = ui.painter_at(rect);
    let key = Id::new("adjustDate.thumbnail");
    let texture = match thumb {
        Thumb::Image { size, image, .. } => {
            let kept = ctx.data(|d| d.get_temp::<ThumbSlot>(key)).filter(|(p, s, _)| *p == id && *s == size).map(|(_, _, h)| h);
            kept.or_else(|| {
                let dims = [image.width as usize, image.height as usize];
                if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
                    return None;
                }
                let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
                let handle = ctx.load_texture(format!("adjustDate:{id}"), pixels, egui::TextureOptions::LINEAR);
                ctx.data_mut(|d| d.insert_temp(key, (id, size, handle.clone())));
                Some(handle)
            })
        }
        Thumb::Sketch(_) | Thumb::NotScanned => None,
    };
    match texture {
        Some(handle) => {
            painter.add(textured(image, handle.id(), turn));
        }
        None => {
            painter.rect_filled(image, 0, t.field);
        }
    }
}

/// `rect` covered by `texture`, its picture turned by `turn`.
fn textured(rect: Rect, texture: egui::TextureId, turn: Dihedral) -> Shape {
    let back = turn.inverse();
    let mut mesh = Mesh::with_texture(texture);
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let (u, v) = turn_point(back, x, y);
        mesh.vertices.push(Vertex { pos: rect.lerp_inside(vec2(x, y)), uv: pos2(u, v), color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let n = ids(&d.fields).len();
    let first = d.fields.get("ids").and_then(Value::as_array).and_then(|a| a.first()).and_then(|v| serde_json::from_value::<PhotoId>(v.clone()).ok());
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP * 2.0, GAP);
        let (rect, _) = ui.allocate_exact_size(vec2(THUMB, THUMB), Sense::hover());
        if let Some(id) = first {
            thumbnail(app, ui, id, rect);
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = GAP;
            let fields = &mut d.fields;
            let current = date_of(fields, "current");
            let shown = current.map_or_else(|| NO_DATE.to_string(), long);
            row(ui, &t, "Current date:", |ui| ui.label(RichText::new(shown).font(fonts::regular(t.body)).color(t.text)));

            // The fields start from the new date (else the current one, else a fixed day).
            let new = new_date(fields).or(current);
            let mut day =
                new.and_then(|n| PickedDate::new(n.date().year(), n.date().month(), n.date().day())).or_else(|| PickedDate::new(2000, 1, 1));
            let mut time = new.and_then(|n| PickedTime::new(n.hour(), n.minute(), n.second())).unwrap_or_default();
            let mut changed = false;
            row(ui, &t, "New date:", |ui| {
                if let Some(day) = &mut day {
                    changed |= date_field(ui, &t, DATE_ID, day, DATE_WIDTH).changed;
                }
                changed |= time_field(ui, &t, TIME_ID, &mut time, TIME_WIDTH).changed;
                if let Some(day) = day.and_then(|p| Date::new(p.year(), p.month(), p.day()).ok()) {
                    ui.label(RichText::new(format!("{}, {}", weekday(day), long_date(day))).font(fonts::regular(t.detail)).color(t.text_dim));
                }
            });
            if changed
                && let Some(new) = day
                    .and_then(|p| Date::new(p.year(), p.month(), p.day()).ok())
                    .and_then(|date| DateTime::new(date, time.hour(), time.minute(), time.second()).ok())
            {
                put_new(fields, new);
            }
            if n > 1 {
                ui.add_space(2.0);
                let set = sets(fields);
                // Shifting needs a date to shift from.
                if radio(ui, &t, SHIFT_ID, SHIFT_LABEL, !set, current.is_some()) {
                    fields.insert("mode".into(), json!("shift"));
                }
                if radio(ui, &t, SET_ID, SET_LABEL, set, true) {
                    fields.insert("mode".into(), json!("set"));
                }
            }
            if !sets(fields)
                && let Some(shift) = fields.get("shift").and_then(Value::as_i64)
            {
                ui.label(RichText::new(shift_note(shift, n)).font(fonts::regular(t.detail)).color(t.text_dim));
            }
        });
    });
}

/// OK: `photo.dates.set {ids, shift}` or `{ids, set}`.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let f = &d.fields;
    let ids = ids(f);
    if sets(f) {
        let set = date_of(f, "set").ok_or_else(|| CmdError::Disabled("Choose a date first.".into()))?;
        app.run("photo.dates.set", &json!({ "ids": ids, "set": set }))
    } else {
        let shift = f.get("shift").and_then(Value::as_i64).ok_or_else(|| CmdError::BadParams("the shift is a whole number of seconds".into()))?;
        app.run("photo.dates.set", &json!({ "ids": ids, "shift": shift }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> DateTime {
        s.parse().unwrap()
    }

    fn draft(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(o) => o,
            _ => Map::new(),
        }
    }

    #[test]
    fn the_new_date_is_the_shifted_or_the_set_one() {
        let mut f = draft(json!({ "ids": [1, 2], "current": "2026-06-04T10:00:00", "mode": "shift", "shift": 5400, "set": "2026-06-04T10:00:00" }));
        assert_eq!(new_date(&f), Some(dt("2026-06-04T11:30:00")));
        put_new(&mut f, dt("2026-06-05T09:00:00"));
        assert_eq!((f["shift"].clone(), f["set"].clone()), (json!(82_800), json!("2026-06-05T09:00:00")), "both modes give it");
        f.insert("mode".into(), json!("set"));
        assert_eq!(new_date(&f), Some(dt("2026-06-05T09:00:00")));
        f.insert("set".into(), json!("2026-02-31T00:00:00"));
        assert_eq!(new_date(&f), None, "no such day");
        // Shifting needs a current date and a shift that stays in the years 1 to 9999.
        let none = draft(json!({ "ids": [1], "current": null, "mode": "shift", "shift": 0, "set": null }));
        assert_eq!(new_date(&none), None);
        let far = draft(json!({ "ids": [1], "current": "9999-12-31T23:59:59", "mode": "shift", "shift": 1 }));
        assert_eq!(new_date(&far), None);
        let huge = draft(json!({ "ids": [1], "current": "2026-01-01T00:00:00", "mode": "shift", "shift": i64::MAX }));
        assert_eq!(new_date(&huge), None);
        let bad = draft(json!({ "ids": [1], "current": "2026-01-01T00:00:00", "mode": "shift", "shift": "1" }));
        assert_eq!(new_date(&bad), None);
    }

    #[test]
    fn unix_seconds_match_the_core_dates() {
        for s in ["0001-01-01T00:00:00", "1969-12-31T23:59:59", "1970-01-01T00:00:00", "2000-02-29T12:00:00", "9999-12-31T23:59:59"] {
            assert_eq!(DateTime::from_unix_secs(unix_secs(dt(s))), Some(dt(s)), "{s}");
        }
    }

    #[test]
    fn shift_notes_say_how_far_and_which_way() {
        assert_eq!(amount(5400), "1 h 30 min");
        assert_eq!(amount(-90_061), "1 day 1 h 1 min 1 s");
        assert_eq!(amount(2 * 86_400), "2 days");
        assert!(amount(i64::MIN).ends_with(" s"), "no overflow");
        assert_eq!(shift_note(5400, 3), "Every picture moves 1 h 30 min later.");
        assert_eq!(shift_note(-60, 1), "The picture moves 1 min earlier.");
        assert_eq!(shift_note(0, 2), "Every picture keeps its date.");
    }
}
