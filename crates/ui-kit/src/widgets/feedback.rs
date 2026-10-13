//! Feedback widgets (`design-system.md` §6, §8): the progress row of background jobs, the empty
//! state of a view with nothing to show, and the toast that confirms an action with no visible
//! result in place.

use std::sync::Arc;

use egui::{Align, Area, Color32, CornerRadius, Galley, Id, Order, Painter, Rect, Response, Sense, Ui, Vec2, WidgetType, pos2, vec2};

use super::marks;
use super::{Target, focus_ring, one_line, primary_button, register, text_button};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// What a [`progress_row`] did this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgressOut {
    /// The cancel button was clicked.
    pub cancel: bool,
}

/// One background job (`design-system.md` §6) in a `width × 20` row: its `label` (11 pt `text_dim`,
/// elided), a 4 pt bar (`track`, filled `accent` to `fraction`; a sliding `accent` segment when the
/// fraction is unknown), and, given `cancel_id`, a 16 pt × button (`text` hovered). Registers `id`
/// on the row and `cancel_id` on the button.
#[allow(clippy::too_many_arguments)] // the signature of `design-system.md` §6, plus `ui`, the tokens, ids and width
pub fn progress_row(ui: &mut Ui, t: &Tokens, id: &str, label: &str, fraction: Option<f32>, cancel_id: Option<&str>, width: f32) -> ProgressOut {
    let enabled = ui.is_enabled();
    let width = if width.is_finite() { width.max(80.0) } else { 240.0 };
    let (_, row) = ui.allocate_space(vec2(width, t.metrics.status_h));
    let mut out = ProgressOut::default();
    let cancel_w = if cancel_id.is_some() { 20.0 } else { 0.0 };
    let bar_w = (row.width() * 0.4).min(120.0);
    let bar = Rect::from_min_size(pos2(row.max.x - cancel_w - bar_w - 4.0, row.center().y - 2.0), vec2(bar_w, 4.0));
    let p = ui.painter();
    let galley = one_line(p, label, fonts::regular(t.small), t.text_dim, bar.min.x - row.min.x - 8.0);
    p.galley(pos2(row.min.x, row.center().y - galley.size().y / 2.0).round(), galley, t.text_dim);
    p.rect_filled(bar, CornerRadius::same(2), t.track);
    let fill = if enabled { t.accent } else { t.text_disabled };
    match fraction.filter(|f| f.is_finite()) {
        Some(f) => {
            let w = bar.width() * f.clamp(0.0, 1.0);
            if w > 0.0 {
                p.rect_filled(Rect::from_min_size(bar.min, vec2(w, bar.height())), CornerRadius::same(2), fill);
            }
        }
        None => {
            // A third of the bar sliding across once every 1.2 s, wrapping round.
            let time = ui.input(|i| i.time);
            let phase = if time.is_finite() { (time / 1.2).fract() as f32 } else { 0.0 };
            let seg = bar.width() / 3.0;
            let x = bar.min.x + phase * (bar.width() + seg) - seg;
            let part = Rect::from_min_max(pos2(x.max(bar.min.x), bar.min.y), pos2((x + seg).min(bar.max.x), bar.max.y));
            if part.width() > 0.0 {
                p.rect_filled(part, CornerRadius::same(2), fill);
            }
            ui.ctx().request_repaint();
        }
    }
    let reg_clip = ui.clip_rect();
    register(ui.ctx(), id, Role::Other, label, Target::Painted { rect: row, clip: reg_clip }, false);
    if let Some(cancel_id) = cancel_id {
        let rect = Rect::from_center_size(pos2(row.max.x - 10.0, row.center().y), Vec2::splat(16.0));
        let c = ui.interact(rect, Id::new(cancel_id), if enabled { Sense::click() } else { Sense::hover() });
        let p = ui.painter();
        if enabled && c.hovered() {
            p.rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
        }
        let glyph = if !enabled {
            t.text_disabled
        } else if c.hovered() {
            t.text
        } else {
            t.icon
        };
        marks::cross(p, rect, glyph);
        focus_ring(p, t, &c, rect, t.radius);
        register(ui.ctx(), cancel_id, Role::Button, "Cancel", Target::Widget(&c), false);
        c.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, "Cancel"));
        out.cancel = c.on_hover_text("Cancel").clicked();
    }
    out
}

/// An empty view's message (`design-system.md` §6), centred in the space `ui` has left: `title`
/// SemiBold 18 `text_label`, `body` 13 `text_dim` centred and wrapped at 360 pt, and an optional
/// primary button `(id, label)` 16 pt below. Registers `id` on the block; returns the button's
/// response.
pub fn empty_state(ui: &mut Ui, t: &Tokens, id: &str, title: &str, body: &str, action: Option<(&str, &str)>) -> Option<Response> {
    let area = ui.available_rect_before_wrap();
    let wrap = (area.width() - 32.0).clamp(40.0, 360.0);
    let p = ui.painter();
    let mut job = egui::text::LayoutJob::simple(title.to_owned(), fonts::semibold(t.empty_title), t.text_label, wrap);
    job.halign = Align::Center;
    let title_g = p.layout_job(job);
    let mut job = egui::text::LayoutJob::simple(body.to_owned(), fonts::regular(t.body), t.text_dim, wrap);
    job.halign = Align::Center;
    let body_g = p.layout_job(job);
    let button_h = if action.is_some() { 16.0 + t.metrics.button_h } else { 0.0 };
    let height = title_g.size().y + 8.0 + body_g.size().y + button_h;
    let top = (area.center().y - height / 2.0).max(area.min.y).round();
    let cx = area.center().x.round();
    let block = Rect::from_min_size(pos2(cx - wrap / 2.0, top), vec2(wrap, height));
    let title_h = title_g.size().y;
    let body_y = top + title_h + 8.0;
    p.galley(pos2(cx, top), title_g, t.text_label);
    let body_h = body_g.size().y;
    p.galley(pos2(cx, body_y), body_g, t.text_dim);
    register(ui.ctx(), id, Role::Other, title, Target::Painted { rect: block, clip: ui.clip_rect() }, false);
    let response = action.map(|(button_id, label)| {
        let slot = Rect::from_min_size(pos2(block.min.x, body_y + body_h + 16.0), vec2(wrap, t.metrics.button_h));
        ui.scope_builder(egui::UiBuilder::new().max_rect(slot).layout(egui::Layout::top_down(Align::Center)), |ui| {
            primary_button(ui, t, button_id, label)
        })
        .inner
    });
    ui.allocate_rect(Rect::from_min_max(area.min, pos2(area.max.x, block.max.y.max(area.min.y))), Sense::hover());
    response
}

/// A toast's kind: how long it stays and how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastKind {
    /// A confirmation ("Starred 12 photos"): [`TOAST_SECS`].
    #[default]
    Info,
    /// A warning the user should read, nothing lost ("Could not write the caption into
    /// IMG_0041.jpg: …; the folder sidecar keeps the value"): [`TOAST_ERROR_SECS`], with a
    /// `caution` bar at its left edge.
    Caution,
    /// A failure: [`TOAST_ERROR_SECS`], with a `danger` bar at its left edge.
    Error,
}

impl ToastKind {
    /// How long a toast of this kind shows, in seconds.
    pub fn life(self) -> f64 {
        match self {
            ToastKind::Info => TOAST_SECS,
            ToastKind::Caution | ToastKind::Error => TOAST_ERROR_SECS,
        }
    }

    /// The colour of the bar at its left edge, if it has one.
    fn bar(self, t: &Tokens) -> Option<Color32> {
        match self {
            ToastKind::Info => None,
            ToastKind::Caution => Some(t.caution),
            ToastKind::Error => Some(t.danger),
        }
    }
}

/// How long an info toast stays, in seconds (`design-system.md` §6).
pub const TOAST_SECS: f64 = 1.4;
/// How long a caution or error toast stays, in seconds.
pub const TOAST_ERROR_SECS: f64 = 6.0;
/// The fade-out at the end of a toast's life, in seconds.
const TOAST_FADE: f64 = 0.2;
/// The most lines a toast's text wraps to; past that it is cut short in the middle.
pub const TOAST_LINES: usize = 3;
/// The longest text a toast lays out, in characters (callers keep theirs far shorter).
const TOAST_MAX_CHARS: usize = 2000;

/// A toast's text and how many characters at its start stay whole when it is cut short (the file
/// name of "Could not write the caption into IMG_0041.jpg: …"). A plain `&str` keeps none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastText<'a> {
    pub text: &'a str,
    pub keep: usize,
}

impl<'a> ToastText<'a> {
    /// `text`, with its first `keep` characters kept whole.
    pub fn new(text: &'a str, keep: usize) -> ToastText<'a> {
        ToastText { text, keep }
    }
}

impl<'a> From<&'a str> for ToastText<'a> {
    fn from(text: &'a str) -> ToastText<'a> {
        ToastText { text, keep: 0 }
    }
}

impl<'a> From<&'a String> for ToastText<'a> {
    fn from(text: &'a String) -> ToastText<'a> {
        ToastText { text, keep: 0 }
    }
}

/// A toast's opacity `age` seconds after it appeared: 1 until the last 0.2 s of its life, then
/// fading to 0, and 0 once it is over (or for an age that is negative or not a number).
pub fn toast_opacity(kind: ToastKind, age: f64) -> f32 {
    let life = kind.life();
    if !(age.is_finite() && age >= 0.0) || age >= life {
        return 0.0;
    }
    ((life - age) / TOAST_FADE).min(1.0) as f32
}

/// `text` cut to `n` of its characters with a "…" in place of the rest: the cut falls in the
/// middle, but never inside the first `keep` characters while they fit in `n` (past that, the plain
/// middle). The whole text when it has at most `n` characters.
pub fn elide_middle(text: &str, keep: usize, n: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if n >= len {
        return text.to_owned();
    }
    let half = n.div_ceil(2);
    let head = if keep <= n { keep.max(half) } else { half };
    let tail = n.saturating_sub(head);
    let start: String = chars.get(..head).unwrap_or_default().iter().collect();
    let end: String = chars.get(len.saturating_sub(tail)..).unwrap_or_default().iter().collect();
    format!("{}…{}", start.trim_end(), end.trim_start())
}

/// The widest a toast's text runs in an `area_width`-wide area: the area less the plate's padding
/// and a 24 pt margin each side, between 40 and 520 pt.
pub fn toast_text_width(t: &Tokens, area_width: f32) -> f32 {
    let w = area_width - 2.0 * t.metrics.toast_padding.x - 48.0;
    if w.is_finite() { w.clamp(40.0, 520.0) } else { 40.0 }
}

/// A toast's text laid out for an `area_width`-wide area (`design-system.md` §6): 14 pt `text`
/// wrapped at [`toast_text_width`] to at most [`TOAST_LINES`] lines; a text longer than that is
/// cut short in the middle ([`elide_middle`], its `keep` characters whole) to the most characters
/// that fit.
pub fn toast_galley(painter: &Painter, t: &Tokens, text: ToastText<'_>, area_width: f32) -> Arc<Galley> {
    let width = toast_text_width(t, area_width);
    let layout = |s: String| painter.layout(s, fonts::regular(t.large), t.text, width);
    let capped: String = text.text.chars().take(TOAST_MAX_CHARS).collect();
    let whole = layout(capped.clone());
    if whole.rows.len() <= TOAST_LINES {
        return whole;
    }
    // The most characters that still fit: 0 always does (a lone "…").
    let (mut lo, mut hi) = (0, capped.chars().count());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if layout(elide_middle(&capped, text.keep, mid)).rows.len() <= TOAST_LINES {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    layout(elide_middle(&capped, text.keep, lo))
}

/// A toast (`design-system.md` §6): `text` in 14 pt `text` ([`toast_galley`]: up to three lines,
/// cut short in the middle past that) on a `toast_bg` plate with radius 6 and 16 × 10 padding,
/// centred near the bottom of `area` (24 pt above it), on top of everything, faded by
/// [`toast_opacity`]; nothing once its time is over. A caution or error toast has a 4 pt `caution`
/// or `danger` bar at its left edge. An `action` `(id, label)` adds a text button
/// (`button:toast.showFolder`). Registers `id` (`toast:<n>`) on the plate, labelled with the whole
/// text; returns the action's response.
#[allow(clippy::too_many_arguments)] // what to show, where, how long it has shown, and its action
pub fn toast<'a>(
    ui: &Ui,
    t: &Tokens,
    id: &str,
    area: Rect,
    text: impl Into<ToastText<'a>>,
    kind: ToastKind,
    age: f64,
    action: Option<(&str, &str)>,
) -> Option<Response> {
    let opacity = toast_opacity(kind, age);
    if opacity <= 0.0 || !area.is_positive() {
        return None;
    }
    let text = text.into();
    let pad = t.metrics.toast_padding;
    let galley = toast_galley(ui.painter(), t, text, area.width());
    let button_w = action.map_or(0.0, |(_, label)| {
        let w = ui.painter().layout_no_wrap(label.to_owned(), fonts::semibold(t.button_label), t.text).size().x;
        12.0 + (w + 18.0).max(37.0).round()
    });
    let bar_color = kind.bar(t);
    let bar = if bar_color.is_some() { 4.0 } else { 0.0 };
    let size = vec2(
        bar + 2.0 * pad.x + galley.size().x + button_w,
        (galley.size().y.max(if action.is_some() { t.metrics.button_h } else { 0.0 }) + 2.0 * pad.y).round(),
    );
    let pos = pos2((area.center().x - size.x / 2.0).round(), area.max.y - t.metrics.toast_bottom - size.y);
    let mut clicked = None;
    Area::new(Id::new(id)).order(Order::Foreground).fixed_pos(pos).constrain(false).interactable(action.is_some()).show(ui.ctx(), |ui| {
        ui.set_opacity(opacity);
        let (_, plate) = ui.allocate_space(size);
        let p = ui.painter();
        p.rect_filled(plate, CornerRadius::same(t.toast_radius), t.toast_bg);
        if let Some(color) = bar_color {
            let edge = Rect::from_min_size(plate.min, vec2(bar, plate.height()));
            let r = t.toast_radius;
            p.rect_filled(edge, CornerRadius { nw: r, sw: r, ne: 0, se: 0 }, color);
        }
        let text_pos = pos2(plate.min.x + bar + pad.x, plate.center().y - galley.size().y / 2.0);
        p.galley(text_pos.round(), galley, t.text);
        register(ui.ctx(), id, Role::Other, text.text, Target::Painted { rect: plate, clip: ui.clip_rect() }, false);
        if let Some((button_id, label)) = action {
            let slot = Rect::from_min_max(pos2(plate.max.x - pad.x - button_w + 12.0, plate.min.y), pos2(plate.max.x - pad.x, plate.max.y));
            clicked = Some(
                ui.scope_builder(egui::UiBuilder::new().max_rect(slot).layout(egui::Layout::left_to_right(Align::Center)), |ui| {
                    text_button(ui, t, button_id, label, false)
                })
                .inner,
            );
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_stay_their_time_then_fade() {
        assert_eq!(toast_opacity(ToastKind::Info, 0.0), 1.0);
        assert_eq!(toast_opacity(ToastKind::Info, 1.1), 1.0);
        assert!((toast_opacity(ToastKind::Info, 1.3) - 0.5).abs() < 1e-4);
        assert_eq!(toast_opacity(ToastKind::Info, 1.4), 0.0);
        assert_eq!(toast_opacity(ToastKind::Error, 1.4), 1.0, "errors stay 6 s");
        assert_eq!(toast_opacity(ToastKind::Error, 6.0), 0.0);
        assert_eq!(toast_opacity(ToastKind::Caution, 5.0), 1.0, "cautions stay 6 s");
        assert_eq!(toast_opacity(ToastKind::Caution, 6.0), 0.0);
        for bad in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(toast_opacity(ToastKind::Info, bad), 0.0, "{bad}");
        }
    }
}
