//! The toolbar's age slider (`organizing.md` §8.2, `ui-layout.md` §3): one handle, leftmost shows
//! photos of every age, moving right shows only newer ones, on a logarithmic scale from 20 years
//! down to one day. The map between the handle's place and the age, and the label that says it
//! ("Pictures up to 3 months old"), are pure functions the UI, the engine's callers and the tests
//! share.
//!
//! An age is a whole number of days back from today. Dragging snaps it to the unit its label
//! reads in (days below a week, whole weeks below 30 days, whole months below a year, whole years
//! after), so the filter is exactly what the label says.

use egui::{Color32, Id, Key, Modifiers, Pos2, Rect, Response, Sense, Ui, pos2, vec2};

use super::{Target, access_range, focus_ring, register};
use crate::Tokens;
use crate::registry::Role;

/// The oldest age the slider reaches: 20 years, in days.
pub const AGE_MAX_DAYS: u32 = 7305;

/// Days in an average month and year (the label's units).
const MONTH: f64 = 30.4375;
const YEAR: f64 = 365.25;

/// The unit an age of `days` reads in, and how many of it (at least 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Days(u32),
    Weeks(u32),
    Months(u32),
    Years(u32),
}

fn unit(days: u32) -> Unit {
    let d = days.clamp(1, AGE_MAX_DAYS);
    let n = |len: f64| ((f64::from(d) / len).round() as u32).max(1);
    if d < 7 {
        Unit::Days(d)
    } else if d < 30 {
        Unit::Weeks(n(7.0))
    } else if d < 365 && n(MONTH) < 12 {
        Unit::Months(n(MONTH))
    } else {
        Unit::Years(n(YEAR).min(20))
    }
}

/// `days` put on the unit its label reads in: 1 … 6 days, whole weeks below 30 days, whole months
/// below a year (11 at most: what would round to 12 months reads as a year), whole years up to [`AGE_MAX_DAYS`] (0 counts as 1 day, more than 20 years as 20).
/// Applying it twice changes nothing, and it never moves an age past a larger one.
pub fn age_snapped(days: u32) -> u32 {
    let len = |n: u32, unit: f64| (f64::from(n) * unit).round() as u32;
    match unit(days) {
        Unit::Days(n) => n,
        Unit::Weeks(n) => n * 7,
        Unit::Months(n) => len(n, MONTH),
        Unit::Years(n) => len(n, YEAR).min(AGE_MAX_DAYS),
    }
}

/// The age at handle place `position` (0 … 1, left to right): `None` (every photo) at 0 or for a
/// position that is not a number, else the days on a logarithmic scale from [`AGE_MAX_DAYS`] just
/// right of 0 down to 1 day at 1, snapped by [`age_snapped`]. Never larger for a larger position.
pub fn age_days(position: f32) -> Option<u32> {
    if position.is_nan() || position <= 0.0 {
        return None;
    }
    let f = f64::from(position.min(1.0));
    let days = (f64::from(AGE_MAX_DAYS).ln() * (1.0 - f)).exp().round();
    Some(age_snapped(days.clamp(1.0, f64::from(AGE_MAX_DAYS)) as u32))
}

/// The handle place (0 … 1) of an age: 0 for `None` (every photo), else the inverse of
/// [`age_days`]' scale (1 day at 1, 20 years and more just right of 0).
pub fn age_position(days: Option<u32>) -> f32 {
    match days {
        None => 0.0,
        Some(d) => {
            let d = f64::from(d.clamp(1, AGE_MAX_DAYS));
            // A hair right of 0 at the far end, so 20 years never reads as "every photo".
            (1.0 - d.ln() / f64::from(AGE_MAX_DAYS).ln()).max(0.001) as f32
        }
    }
}

/// What the slider says: "Pictures up to 3 months old", in days, weeks, months or years, one of
/// each singular ("Pictures up to 1 day old"); "All pictures" for `None`.
pub fn age_label(days: Option<u32>) -> String {
    let Some(days) = days else { return "All pictures".to_owned() };
    let (n, one, many) = match unit(days) {
        Unit::Days(n) => (n, "day", "days"),
        Unit::Weeks(n) => (n, "week", "weeks"),
        Unit::Months(n) => (n, "month", "months"),
        Unit::Years(n) => (n, "year", "years"),
    };
    format!("Pictures up to {n} {} old", if n == 1 { one } else { many })
}

/// The age slider (`organizing.md` §8.2): `zoom_slider`'s thin look, `width` × 20 (the toolbar's
/// is 100 wide, `ui-layout.md` §3), a 2 pt `track` line (`thumb` hovered) and a 3 × 14 thumb in
/// `thumb_hover` (`pick` dragged) at [`age_position`]. Dragging or clicking sets `days` through
/// [`age_days`] and marks the response changed; ←/→ with the focus move the handle a twentieth of
/// the way. The hover text and the accessible label are [`age_label`] while an age is set, else
/// `tooltip`. Registers `id` (role `other`, active while an age is set).
pub fn age_slider(ui: &mut Ui, t: &Tokens, id: &str, days: &mut Option<u32>, width: f32, tooltip: &str) -> Response {
    let m = &t.metrics;
    let enabled = ui.is_enabled();
    let width = if width.is_finite() { width.max(m.zoom_thumb.x + 4.0) } else { m.zoom_slider.x };
    let (_, rect) = ui.allocate_space(vec2(width, m.zoom_slider.y));
    let mut r = ui.interact(rect, Id::new(id), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let inset = m.zoom_thumb.x / 2.0 + 1.0;
    let (x0, x1) = (rect.min.x + inset, rect.max.x - inset);
    let mut pos = age_position(*days);
    if enabled {
        if (r.dragged() || r.clicked())
            && let Some(Pos2 { x, .. }) = r.interact_pointer_pos()
        {
            pos = if x1 > x0 { ((x - x0) / (x1 - x0)).clamp(0.0, 1.0) } else { 0.0 };
        }
        if r.has_focus() {
            let (left, right) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight)));
            if left {
                pos = (pos - 0.05).max(0.0);
            }
            if right {
                pos = (pos + 0.05).min(1.0);
            }
        }
    }
    let new = if pos == age_position(*days) { *days } else { age_days(pos) };
    if new != *days {
        *days = new;
        r.mark_changed();
    }
    let p = ui.painter();
    let hot = enabled && (r.hovered() || r.dragged());
    let y = rect.center().y;
    let track: Color32 = if !enabled {
        t.text_disabled.gamma_multiply(0.6)
    } else if hot {
        t.thumb
    } else {
        t.track
    };
    let half = m.slider_track / 2.0;
    p.rect_filled(Rect::from_min_max(pos2(x0, y - half), pos2(x1, y + half)), 1.0, track);
    let thumb_color = if !enabled {
        t.text_disabled
    } else if r.dragged() || r.is_pointer_button_down_on() {
        t.pick
    } else {
        t.thumb_hover
    };
    let at = x0 + age_position(*days) * (x1 - x0).max(0.0);
    let thumb = Rect::from_center_size(pos2(at.round(), y), m.zoom_thumb);
    p.rect_filled(thumb, 1.0, thumb_color);
    focus_ring(p, t, &r, thumb.expand(1.0), 1);
    let label = if days.is_some() { age_label(*days) } else { tooltip.to_owned() };
    r.widget_info(|| egui::WidgetInfo::slider(enabled, f64::from(age_position(*days)), &label));
    access_range(&r, f64::from(age_position(*days)), 0.0, 1.0, 0.05, &age_label(*days));
    register(ui.ctx(), id, Role::Other, &label, Target::Widget(&r), days.is_some());
    if label.is_empty() { r } else { r.on_hover_text(label) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_switch_where_the_label_would_round_badly() {
        assert_eq!(unit(6), Unit::Days(6));
        assert_eq!(unit(7), Unit::Weeks(1));
        assert_eq!(unit(29), Unit::Weeks(4));
        assert_eq!(unit(30), Unit::Months(1));
        assert_eq!(unit(334), Unit::Months(11));
        assert_eq!(unit(350), Unit::Months(11));
        assert_eq!(unit(351), Unit::Years(1));
        assert_eq!(unit(364), Unit::Years(1));
        assert_eq!(unit(365), Unit::Years(1));
        assert_eq!(unit(u32::MAX), Unit::Years(20));
        assert_eq!(unit(0), Unit::Days(1));
    }
}
