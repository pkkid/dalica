//! The sliders of `design-system.md` §6: the labelled [`slider`] of the edit panels (a control
//! spec, drag, Shift = fine, double-click resets, ↑/↓ nudge, click the value to type one), its
//! Tuning variant [`tuning_slider`] (no value shown: it is a tooltip; + / − nudge 2 %), and the
//! thin [`zoom_slider`] for the thumbnail size and edit zoom; and the [`stops_slider`] of the
//! Export dialog (snaps near given stops, a linked number field beside it).
//!
//! Every number that reaches them may be hostile (a spec from a plug-in, a value from a sidecar):
//! ranges are ordered, NaN never propagates, and nothing panics.

use std::ops::RangeInclusive;

use egui::{Align2, Color32, CornerRadius, Id, Key, Modifiers, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use super::{Target, focus_ring, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// What a [`slider`] edits: the control spec shared by the UI and the API (`architecture.md` §3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderSpec<'a> {
    /// The automation id, `slider:<recipe op>.<param>` (`control-protocol.md` §7).
    pub id: &'a str,
    /// The label shown left of the value (and the accessible name).
    pub label: &'a str,
    pub min: f64,
    pub max: f64,
    /// What a double-click resets to.
    pub default: f64,
    /// Values are whole multiples of `step` (none if it is not a positive finite number).
    pub step: f64,
    /// Decimals shown.
    pub decimals: u8,
    /// Shown after the value ("%", " K"); empty for none.
    pub unit: &'a str,
}

impl SliderSpec<'_> {
    /// `(low, high)`: the range in order; `(0, 1)` if either end is not finite.
    pub fn bounds(&self) -> (f64, f64) {
        if self.min.is_finite() && self.max.is_finite() { (self.min.min(self.max), self.min.max(self.max)) } else { (0.0, 1.0) }
    }

    /// `v` inside the range; NaN becomes the default (itself kept inside the range).
    pub fn clamp(&self, v: f64) -> f64 {
        let (lo, hi) = self.bounds();
        let v = if v.is_nan() { if self.default.is_nan() { lo } else { self.default } } else { v };
        v.max(lo).min(hi)
    }

    /// `v` on the spec's steps and inside its range.
    pub fn snap(&self, v: f64) -> f64 {
        let v = self.clamp(v);
        if !(self.step.is_finite() && self.step > 0.0) {
            return v;
        }
        let snapped = (v / self.step).round() * self.step;
        if snapped.is_finite() { self.clamp(snapped) } else { v }
    }

    /// Where `v` sits in the range, 0 to 1.
    pub fn fraction(&self, v: f64) -> f32 {
        let (lo, hi) = self.bounds();
        let span = hi - lo;
        if span <= 0.0 { 0.0 } else { ((self.clamp(v) - lo) / span) as f32 }
    }

    /// The value at `fraction` of the range (clamped to 0 … 1), not yet snapped.
    pub fn at_fraction(&self, fraction: f32) -> f64 {
        let (lo, hi) = self.bounds();
        let f = if fraction.is_finite() { f64::from(fraction).clamp(0.0, 1.0) } else { 0.0 };
        lo + (hi - lo) * f
    }

    /// `v` as shown: `decimals` places (at most 6), the unit after it, never "-0".
    pub fn format(&self, v: f64) -> String {
        let v = self.clamp(v);
        let d = usize::from(self.decimals.min(6));
        let mut s = format!("{v:.d$}");
        if s.starts_with('-') && s.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') {
            s.remove(0);
        }
        format!("{s}{}", self.unit)
    }
}

/// What a typed value asks for: a number such as `0.5`, `+12`, `-1,5` (comma or point), optionally
/// followed by `%` or `K`; on the spec's steps and clamped to its range. `None` when it is not a
/// finite number, so the slider keeps its value.
pub fn typed_value(spec: &SliderSpec<'_>, text: &str) -> Option<f64> {
    let t = text.trim().replace(',', ".");
    let t = t.trim_end_matches(|c: char| c == '%' || c == 'K' || c == 'k' || c.is_whitespace());
    let t = t.strip_prefix('+').unwrap_or(t);
    let v: f64 = t.parse().ok().filter(|v: &f64| v.is_finite())?;
    Some(spec.snap(v))
}

/// `value` moved by `steps` keyboard nudges: one nudge is about 1/200 of the range, a whole number
/// of the spec's steps (at least one).
pub fn nudged(spec: &SliderSpec<'_>, value: f64, steps: f64) -> f64 {
    let (lo, hi) = spec.bounds();
    let step = if spec.step.is_finite() && spec.step > 0.0 { spec.step } else { (hi - lo) / 200.0 };
    if !(step.is_finite() && step > 0.0) || !steps.is_finite() {
        return spec.clamp(value);
    }
    let unit = ((hi - lo) / 200.0 / step).round().max(1.0) * step;
    spec.snap(spec.clamp(value) + unit * steps)
}

/// `value` moved by `steps` Tuning nudges (+ / −, `editing.md` §7): one nudge is 2 % of the range,
/// then put on the spec's steps and clamped.
pub fn tuning_nudged(spec: &SliderSpec<'_>, value: f64, steps: f64) -> f64 {
    let (lo, hi) = spec.bounds();
    if !steps.is_finite() {
        return spec.clamp(value);
    }
    spec.snap(spec.clamp(value) + (hi - lo) * TUNING_NUDGE * steps)
}

/// The share of the range one + / − press moves a [`tuning_slider`].
pub const TUNING_NUDGE: f64 = 0.02;

/// Which slider row to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// The value shown right of the label, click to type.
    Labelled,
    /// No value field: the value is a tooltip, + / − nudge 2 %.
    Tuning,
}

/// What a [`slider`] did this pass.
#[derive(Debug, Clone)]
pub struct SliderOut {
    /// The new value, when it changed.
    pub value: Option<f64>,
    /// A drag (or a one-shot change: a click, a typed value, a nudge) began: open an undo step.
    pub drag_started: bool,
    /// It ended: close the undo step (`design-system.md` §6: one undo step per drag).
    pub drag_stopped: bool,
    /// The change was a double-click reset to the default.
    pub reset: bool,
    /// The track's response.
    pub response: Response,
}

/// The temporary state of a value being typed: the text and how many passes the field has shown.
#[derive(Debug, Clone, Default)]
struct Typing {
    text: String,
    passes: u8,
}

fn typing_id(spec_id: &str) -> Id {
    Id::new(spec_id).with("typing")
}

/// Opens the value of slider `spec` for typing, as a click on it does (for scripts and tests); the
/// field shows the current value, selected, on the next pass.
pub fn edit_slider_value(ui: &Ui, spec: &SliderSpec<'_>, value: f64) {
    let text = spec.format(value).trim_end_matches(spec.unit).to_owned();
    ui.data_mut(|d| d.insert_temp(typing_id(spec.id), Typing { text, passes: 0 }));
}

/// The labelled slider row (45 pt, `design-system.md` §6): the label left and the value right on an
/// 18 pt row (12.5 pt, `text_label`), then a 2 pt `track` line with a hollow ring thumb (radius 7,
/// stroked 2 pt `thumb`, `thumb_hover` hovered or dragged). Drag sets the value (Shift: ten times
/// finer), a click jumps to it, a double-click resets to the default, ↑/↓ nudge while the pointer
/// is on the row or the slider has the focus (Shift: five nudges), and a click on the value opens it
/// for typing (Return or clicking away applies, Esc keeps the old value).
///
/// Registers `spec.id` on the track and `<spec.id>/value` on the value (or the field typing it).
pub fn slider(ui: &mut Ui, t: &Tokens, spec: &SliderSpec<'_>, value: f64) -> SliderOut {
    slider_row(ui, t, spec, value, Variant::Labelled)
}

/// The Tuning tab's slider (`editing.md` §7): the [`slider`] row without a value field. The value
/// shows as a tooltip while the pointer is on the track or it is dragged; + and − nudge it by
/// [`TUNING_NUDGE`] of the range while the pointer is on the row or it has the focus (↑ / ↓ still
/// nudge finely); drag, click and double-click (reset to the default) work as in [`slider`].
/// Registers `spec.id` on the track only.
pub fn tuning_slider(ui: &mut Ui, t: &Tokens, spec: &SliderSpec<'_>, value: f64) -> SliderOut {
    slider_row(ui, t, spec, value, Variant::Tuning)
}

fn slider_row(ui: &mut Ui, t: &Tokens, spec: &SliderSpec<'_>, value: f64, variant: Variant) -> SliderOut {
    let tuning = variant == Variant::Tuning;
    let m = &t.metrics;
    let enabled = ui.is_enabled();
    let value = spec.clamp(value);
    let width = ui.available_width().max(4.0 * m.slider_ring);
    let (_, row) = ui.allocate_space(vec2(width, m.slider_row_h));
    let label_rect = Rect::from_min_size(row.min, vec2(row.width(), m.slider_label_h));
    let band = Rect::from_min_max(pos2(row.min.x, label_rect.max.y), row.max);
    let ring = m.slider_ring;
    let line = Rect::from_min_max(pos2(band.min.x + ring + 1.0, band.center().y), pos2(band.max.x - ring - 1.0, band.center().y));
    let id = Id::new(spec.id);
    let resp = ui.interact(band, id, if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let value_w = (row.width() * 0.4).clamp(24.0, 72.0);
    let value_rect = Rect::from_min_max(pos2(label_rect.max.x - value_w, label_rect.min.y), label_rect.max);

    let to_x = |v: f64| line.min.x + spec.fraction(v) * line.width();
    let from_x = |x: f32| spec.at_fraction(if line.width() > 0.0 { (x - line.min.x) / line.width() } else { 0.0 });
    let mut out = SliderOut { value: None, drag_started: false, drag_stopped: false, reset: false, response: resp.clone() };
    let set = |out: &mut SliderOut, v: f64, one_shot: bool| {
        if (v - value).abs() > 1e-12 {
            out.value = Some(v);
        }
        if one_shot {
            out.drag_started = true;
            out.drag_stopped = true;
        }
    };

    // The value, or the field typing one.
    let mut typing: Option<Typing> = ui.data(|d| d.get_temp(typing_id(spec.id)));
    if !enabled || tuning {
        typing = None;
        ui.data_mut(|d| d.remove::<Typing>(typing_id(spec.id)));
    }
    let typing_now = typing.is_some();
    if let Some(mut state) = typing {
        let field_id = id.with("field");
        let edit = egui::TextEdit::singleline(&mut state.text)
            .id(field_id)
            .font(fonts::regular(t.detail))
            .text_color(t.text)
            .horizontal_align(egui::Align::RIGHT)
            .desired_width(value_rect.width() - 8.0)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO);
        // The field's frame, as the text fields have it focused (`field.rs`).
        let p = ui.painter();
        p.rect_filled(value_rect, CornerRadius::same(t.radius), t.canvas);
        p.rect_stroke(value_rect, CornerRadius::same(t.radius), Stroke::new(t.focus_width, t.accent), egui::StrokeKind::Inside);
        let value_rect = value_rect.shrink2(vec2(4.0, 0.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(value_rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let field = child.add(edit);
        register(ui.ctx(), &format!("{}/value", spec.id), Role::Other, spec.label, Target::Widget(&field), true);
        if state.passes < 2 {
            // Focus and select the old value on the first passes, so typing replaces it.
            field.request_focus();
            let mut te = egui::text_edit::TextEditState::load(ui.ctx(), field_id).unwrap_or_default();
            let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(state.text.chars().count()));
            te.cursor.set_char_range(Some(all));
            te.store(ui.ctx(), field_id);
            state.passes += 1;
            ui.data_mut(|d| d.insert_temp(typing_id(spec.id), state));
        } else if !field.has_focus() {
            let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
            if !cancelled && let Some(v) = typed_value(spec, &state.text) {
                set(&mut out, v, true);
            }
            ui.data_mut(|d| d.remove::<Typing>(typing_id(spec.id)));
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id(spec.id), state));
        }
    } else if !tuning {
        let value_resp = ui.interact(value_rect, id.with("value"), if enabled { Sense::click() } else { Sense::hover() });
        register(ui.ctx(), &format!("{}/value", spec.id), Role::Other, spec.label, Target::Widget(&value_resp), false);
        if value_resp.clicked() {
            edit_slider_value(ui, spec, value);
        }
        if enabled && value_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
    }

    // The track: double-click resets, drag and click set, the arrows nudge.
    if enabled && !typing_now {
        if resp.double_clicked() {
            out.reset = true;
            set(&mut out, spec.snap(spec.default), true);
        } else {
            if resp.drag_started() {
                out.drag_started = true;
            }
            if (resp.dragged() || resp.drag_started())
                && let Some(p) = resp.interact_pointer_pos()
            {
                let fine = ui.input(|i| i.modifiers.shift);
                let v = if fine {
                    let dx = f64::from(ui.input(|i| i.pointer.delta().x));
                    let (lo, hi) = spec.bounds();
                    value + dx * (hi - lo) / f64::from(line.width().max(1.0)) * 0.1
                } else {
                    from_x(p.x)
                };
                set(&mut out, spec.snap(v), false);
            } else if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
            {
                set(&mut out, spec.snap(from_x(p.x)), true);
            }
            if resp.drag_stopped() {
                out.drag_stopped = true;
            }
            if out.value.is_none() && (resp.has_focus() || ui.rect_contains_pointer(row)) {
                let (up, down, shift) = ui.input_mut(|i| {
                    let shift = i.modifiers.shift;
                    let m = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
                    (i.consume_key(m, Key::ArrowUp), i.consume_key(m, Key::ArrowDown), shift)
                });
                if up || down {
                    let steps = if up { 1.0 } else { -1.0 } * if shift { 5.0 } else { 1.0 };
                    set(&mut out, nudged(spec, value, steps), true);
                } else if tuning {
                    let (plus, minus) = ui.input_mut(|i| {
                        let plus = i.consume_key(Modifiers::NONE, Key::Plus) || i.consume_key(Modifiers::NONE, Key::Equals);
                        (plus, i.consume_key(Modifiers::NONE, Key::Minus))
                    });
                    if plus || minus {
                        set(&mut out, tuning_nudged(spec, value, if plus { 1.0 } else { -1.0 }), true);
                    }
                }
            }
        }
    }
    let shown = out.value.unwrap_or(value);

    // Paint.
    let p = ui.painter();
    let text_color = if enabled { t.text_label } else { t.text_disabled };
    let font = fonts::regular(t.detail);
    p.text(label_rect.left_center(), Align2::LEFT_CENTER, spec.label, font.clone(), text_color);
    if !typing_now && !tuning {
        p.text(label_rect.right_center(), Align2::RIGHT_CENTER, spec.format(shown), font, text_color);
    }
    let y = line.center().y;
    let half = m.slider_track / 2.0;
    p.rect_filled(
        Rect::from_min_max(pos2(line.min.x, y - half), pos2(line.max.x, y + half)),
        1.0,
        if enabled { t.track } else { t.text_disabled.gamma_multiply(0.6) },
    );
    let hot = enabled && (resp.hovered() || resp.dragged() || resp.is_pointer_button_down_on());
    let ring_color = if !enabled {
        t.text_disabled
    } else if hot {
        t.thumb_hover
    } else {
        t.thumb
    };
    let c = pos2(to_x(shown), y);
    p.circle_filled(c, ring, ui.visuals().panel_fill);
    p.circle_stroke(c, ring - 1.0, Stroke::new(2.0, ring_color));
    if resp.has_focus() {
        p.circle_stroke(c, ring + 2.0, Stroke::new(t.focus_width, t.accent));
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    register(ui.ctx(), spec.id, Role::Other, spec.label, Target::Widget(&resp), resp.dragged());
    resp.widget_info(|| egui::WidgetInfo::slider(enabled, shown, spec.label));
    if tuning && enabled && (resp.hovered() || resp.dragged()) {
        super::tooltip_now(&resp, t, &spec.format(shown), None);
    }
    out
}

/// How near, in points, a drag or click must come to a stop for a [`stops_slider`] to snap to it.
pub const STOP_SNAP: f32 = 6.0;
/// The width of a [`stops_slider`]'s number field.
pub const STOPS_FIELD_W: f32 = 56.0;
/// The narrowest a [`stops_slider`]'s track gets.
pub const STOPS_TRACK_MIN: f32 = 60.0;
/// How far below the track's middle a stop's tick starts, and how long it is.
const TICK: (f32, f32) = (4.0, 4.0);

/// What a [`stops_slider`] edits: the control spec, the values it snaps to, and its layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopsSlider<'a> {
    /// The control spec (id, range, default, step); `unit` is shown after the number field.
    pub spec: SliderSpec<'a>,
    /// The values a drag snaps to when it comes within [`STOP_SNAP`] of them, drawn as ticks under
    /// the track (export sizes: 480, 640, … 3840). Values outside the range are pulled into it.
    pub stops: &'a [f64],
    /// Show the linked number field (and the unit) before the track.
    pub field: bool,
    /// The track's width; not finite: all the width left on the line (at least
    /// [`STOPS_TRACK_MIN`]).
    pub track_w: f32,
}

/// `v` snapped to the stop nearest it when that stop is within `reach` (in value units), else put on
/// the spec's steps; always inside the range.
pub fn snap_to_stop(spec: &SliderSpec<'_>, stops: &[f64], v: f64, reach: f64) -> f64 {
    let v = spec.clamp(v);
    let reach = if reach.is_finite() { reach.abs() } else { 0.0 };
    let nearest = stops.iter().copied().filter(|s| s.is_finite()).map(|s| spec.clamp(s)).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()));
    match nearest {
        Some(stop) if (stop - v).abs() <= reach => stop,
        _ => spec.snap(v),
    }
}

/// The value one key press moves a [`stops_slider`] to: the next stop above `value` (`up`) or below
/// it, and past the last stop the end of the range; with no stops a [`nudged`] step.
pub fn next_stop(spec: &SliderSpec<'_>, stops: &[f64], value: f64, up: bool) -> f64 {
    let value = spec.clamp(value);
    if !stops.iter().any(|s| s.is_finite()) {
        return nudged(spec, value, if up { 1.0 } else { -1.0 });
    }
    let (lo, hi) = spec.bounds();
    let marks = stops.iter().copied().filter(|s| s.is_finite()).map(|s| spec.clamp(s)).chain([lo, hi]);
    let next =
        if up { marks.filter(|&s| s > value + 1e-9).min_by(f64::total_cmp) } else { marks.filter(|&s| s < value - 1e-9).max_by(f64::total_cmp) };
    next.unwrap_or(value)
}

/// The value as the number field shows it: the spec's decimals, without the unit.
fn number(spec: &SliderSpec<'_>, v: f64) -> String {
    let shown = spec.format(v);
    shown.strip_suffix(spec.unit).unwrap_or(&shown).to_owned()
}

/// The slider with stops (`design-system.md` §6, `creating.md` §2: `[1600] pixels ───○───`), on one
/// 24 pt line: a linked number field ([`STOPS_FIELD_W`] wide, registered `<spec.id>/value`; typing
/// a value and pressing Return or clicking away sets it exactly, on the spec's steps and clamped,
/// Esc keeps the old one) and the spec's unit after it, when `field` is set; then the 2 pt track
/// with a tick under it for each stop and the hollow ring thumb of [`slider`]. A drag or click sets
/// the value, snapping to a stop within [`STOP_SNAP`] points; a double-click resets to the default;
/// ↑ / ↓ (while the pointer is on the track or it has the focus) and ← / → (with the focus) go to
/// the next stop. Registers `spec.id` on the track.
pub fn stops_slider(ui: &mut Ui, t: &Tokens, s: &StopsSlider<'_>, value: f64) -> SliderOut {
    ui.horizontal(|ui| stops_line(ui, t, s, value)).inner
}

fn stops_line(ui: &mut Ui, t: &Tokens, s: &StopsSlider<'_>, value: f64) -> SliderOut {
    let spec = &s.spec;
    let m = &t.metrics;
    let enabled = ui.is_enabled();
    let value = spec.clamp(value);
    let line_h = m.text_field_h;
    let mut changed: Option<f64> = None;
    let mut one_shot = false;

    // The number field and the unit.
    if s.field {
        let field_id = format!("{}/value", spec.id);
        let buffer = Id::new(spec.id).with("stops-typing");
        let mut text = ui.data(|d| d.get_temp::<String>(buffer)).unwrap_or_else(|| number(spec, value));
        let f = super::TextField::new(&field_id, "", STOPS_FIELD_W).select_on_focus(true).show(ui, t, &mut text);
        if f.committed()
            && let Some(v) = typed_value(spec, &text)
        {
            changed = Some(v);
            one_shot = true;
        }
        if f.editing && f.ending.is_none() {
            ui.data_mut(|d| d.insert_temp(buffer, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(buffer));
        }
        let unit = spec.unit.trim();
        if !unit.is_empty() {
            let color = if enabled { t.text_label } else { t.text_disabled };
            let galley = ui.painter().layout_no_wrap(unit.to_owned(), fonts::regular(t.body), color);
            let (_, rect) = ui.allocate_space(vec2(galley.size().x.ceil(), line_h));
            ui.painter().galley(pos2(rect.min.x, rect.center().y - galley.size().y / 2.0).round(), galley, color);
        }
    }

    // The track.
    let width = if s.track_w.is_finite() { s.track_w } else { ui.available_width() };
    let width = if width.is_finite() { width.max(STOPS_TRACK_MIN) } else { STOPS_TRACK_MIN };
    let (_, rect) = ui.allocate_space(vec2(width, line_h));
    let resp = ui.interact(rect, Id::new(spec.id), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let ring = m.slider_ring;
    let (x0, x1) = (rect.min.x + ring + 1.0, rect.max.x - ring - 1.0);
    let span = (x1 - x0).max(1.0);
    let to_x = |v: f64| x0 + spec.fraction(v) * (x1 - x0);
    let from_x = |x: f32| spec.at_fraction((x - x0) / span);
    let (lo, hi) = spec.bounds();
    let reach = f64::from(STOP_SNAP) * (hi - lo) / f64::from(span);
    let mut out = SliderOut { value: None, drag_started: false, drag_stopped: false, reset: false, response: resp.clone() };
    if enabled && changed.is_none() {
        if resp.double_clicked() {
            out.reset = true;
            changed = Some(spec.snap(spec.default));
            one_shot = true;
        } else {
            out.drag_started = resp.drag_started();
            if (resp.dragged() || resp.drag_started() || resp.clicked())
                && let Some(p) = resp.interact_pointer_pos()
            {
                changed = Some(snap_to_stop(spec, s.stops, from_x(p.x), reach));
                one_shot = resp.clicked();
            }
            out.drag_stopped = resp.drag_stopped();
            let hovered = ui.rect_contains_pointer(rect);
            if changed.is_none() && (resp.has_focus() || hovered) {
                let focus = resp.has_focus();
                let (up, down) = ui.input_mut(|i| {
                    let up = i.consume_key(Modifiers::NONE, Key::ArrowUp) || (focus && i.consume_key(Modifiers::NONE, Key::ArrowRight));
                    let down = i.consume_key(Modifiers::NONE, Key::ArrowDown) || (focus && i.consume_key(Modifiers::NONE, Key::ArrowLeft));
                    (up, down)
                });
                if up || down {
                    changed = Some(next_stop(spec, s.stops, value, up));
                    one_shot = true;
                }
            }
        }
    }
    if let Some(v) = changed {
        if (v - value).abs() > 1e-12 {
            out.value = Some(v);
        }
        if one_shot {
            out.drag_started = true;
            out.drag_stopped = true;
        }
    }
    let shown = out.value.unwrap_or(value);

    // Paint: the track, the ticks, the ring.
    let p = ui.painter();
    let y = rect.center().y;
    let guide = if enabled { t.track } else { t.text_disabled.gamma_multiply(0.6) };
    let half = m.slider_track / 2.0;
    p.rect_filled(Rect::from_min_max(pos2(x0, y - half), pos2(x1, y + half)), 1.0, guide);
    for &stop in s.stops.iter().filter(|s| s.is_finite()) {
        let x = to_x(stop);
        p.line_segment([pos2(x, y + TICK.0), pos2(x, y + TICK.0 + TICK.1)], Stroke::new(1.0, guide));
    }
    let hot = enabled && (resp.hovered() || resp.dragged() || resp.is_pointer_button_down_on());
    let ring_color = if !enabled {
        t.text_disabled
    } else if hot {
        t.thumb_hover
    } else {
        t.thumb
    };
    let c = pos2(to_x(shown), y);
    p.circle_filled(c, ring, ui.visuals().panel_fill);
    p.circle_stroke(c, ring - 1.0, Stroke::new(2.0, ring_color));
    if resp.has_focus() {
        p.circle_stroke(c, ring + 2.0, Stroke::new(t.focus_width, t.accent));
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    register(ui.ctx(), spec.id, Role::Other, spec.label, Target::Widget(&resp), resp.dragged());
    resp.widget_info(|| egui::WidgetInfo::slider(enabled, shown, spec.label));
    out
}

/// `(low, high)` of a zoom slider's range: ordered, `(0, 1)` if an end is not finite.
fn zoom_bounds(range: &RangeInclusive<f32>) -> (f32, f32) {
    let (a, b) = (*range.start(), *range.end());
    if a.is_finite() && b.is_finite() { (a.min(b), a.max(b)) } else { (0.0, 1.0) }
}

/// The thin slider for the thumbnail size and the edit zoom (`design-system.md` §6): 110 × 20, a
/// 2 pt `track` line (`thumb` hovered) and a 3 × 14 thumb in `thumb_hover` (`pick` dragged).
/// Dragging or clicking sets `value` and marks the response changed; ←/→ with the focus move it a
/// twentieth of the range. A `value` outside the range (or NaN) is pulled into it.
pub fn zoom_slider(ui: &mut Ui, t: &Tokens, id: &str, value: &mut f32, range: RangeInclusive<f32>, tooltip: &str) -> Response {
    let m = &t.metrics;
    let enabled = ui.is_enabled();
    let (lo, hi) = zoom_bounds(&range);
    let v0 = if value.is_nan() { lo } else { value.max(lo).min(hi) };
    let (_, rect) = ui.allocate_space(m.zoom_slider);
    let mut r = ui.interact(rect, Id::new(id), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let inset = m.zoom_thumb.x / 2.0 + 1.0;
    let (x0, x1) = (rect.min.x + inset, rect.max.x - inset);
    let span = hi - lo;
    let to_x = |v: f32| if span > 0.0 { x0 + (v - lo) / span * (x1 - x0) } else { x0 };
    let mut v = v0;
    if enabled {
        if (r.dragged() || r.clicked())
            && let Some(Pos2 { x, .. }) = r.interact_pointer_pos()
        {
            let f = if x1 > x0 { ((x - x0) / (x1 - x0)).clamp(0.0, 1.0) } else { 0.0 };
            v = lo + f * span;
        }
        if r.has_focus() {
            let (left, right) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight)));
            let nudge = span / 20.0;
            if left {
                v = (v - nudge).max(lo);
            }
            if right {
                v = (v + nudge).min(hi);
            }
        }
    }
    if v != *value {
        *value = v;
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
    let thumb = Rect::from_center_size(pos2(to_x(v).round(), y), m.zoom_thumb);
    p.rect_filled(thumb, 1.0, thumb_color);
    focus_ring(p, t, &r, thumb.expand(1.0), 1);
    register(ui.ctx(), id, Role::Other, tooltip, Target::Widget(&r), r.dragged());
    r.widget_info(|| egui::WidgetInfo::slider(enabled, f64::from(v), tooltip));
    if tooltip.is_empty() { r } else { r.on_hover_text(tooltip) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPOSURE: SliderSpec<'static> =
        SliderSpec { id: "slider:tuning.fill_light", label: "Fill light", min: 0.0, max: 100.0, default: 0.0, step: 1.0, decimals: 0, unit: "" };

    #[test]
    fn values_are_stepped_clamped_and_formatted() {
        let s = EXPOSURE;
        assert_eq!(s.snap(33.4), 33.0);
        assert_eq!(s.snap(-5.0), 0.0);
        assert_eq!(s.snap(f64::NAN), 0.0, "NaN becomes the default");
        assert_eq!(s.snap(f64::INFINITY), 100.0);
        assert_eq!(s.fraction(25.0), 0.25);
        assert_eq!(s.at_fraction(f32::NAN), 0.0);
        assert_eq!(s.at_fraction(2.0), 100.0);
        let temp = SliderSpec { min: -1.0, max: 1.0, step: 0.01, decimals: 2, unit: " K", ..s };
        assert_eq!(temp.format(-0.001), "0.00 K", "never -0");
        assert_eq!(temp.format(-0.5), "-0.50 K");
        // Hostile specs: reversed or infinite ranges, a zero or NaN step, a NaN default.
        let reversed = SliderSpec { min: 10.0, max: -10.0, ..s };
        assert_eq!(reversed.bounds(), (-10.0, 10.0));
        assert_eq!(reversed.snap(42.0), 10.0);
        let broken = SliderSpec { min: f64::NAN, max: f64::INFINITY, step: 0.0, default: f64::NAN, decimals: 255, ..s };
        assert_eq!(broken.bounds(), (0.0, 1.0));
        assert_eq!(broken.snap(f64::NAN), 0.0);
        assert_eq!(broken.snap(0.37), 0.37, "no step: not snapped");
        assert_eq!(broken.format(0.5), "0.500000", "at most 6 decimals");
        let flat = SliderSpec { min: 3.0, max: 3.0, ..s };
        assert_eq!((flat.fraction(3.0), flat.snap(99.0)), (0.0, 3.0));
        let tiny = SliderSpec { step: f64::MIN_POSITIVE, ..s };
        assert_eq!(tiny.snap(50.0), 50.0);
    }

    #[test]
    fn typed_values_are_stepped_and_clamped() {
        let s = EXPOSURE;
        assert_eq!(typed_value(&s, "12"), Some(12.0));
        assert_eq!(typed_value(&s, " +33,4 "), Some(33.0));
        assert_eq!(typed_value(&s, "250 %"), Some(100.0));
        assert_eq!(typed_value(&s, "-3"), Some(0.0));
        let temp = SliderSpec { min: 2000.0, max: 50000.0, step: 50.0, ..s };
        assert_eq!(typed_value(&temp, "5600 K"), Some(5600.0));
        for bad in ["", "abc", "1.2.3", "NaN", "inf", "--1", "+"] {
            assert_eq!(typed_value(&s, bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nudges_are_a_sensible_step() {
        let s = EXPOSURE;
        assert_eq!(nudged(&s, 10.0, 1.0), 11.0, "1/200 of 100 rounds to one step");
        assert_eq!(nudged(&s, 10.0, -5.0), 5.0);
        assert_eq!(nudged(&s, 99.0, 5.0), 100.0, "clamped");
        let wide = SliderSpec { min: -100.0, max: 100.0, ..s };
        assert_eq!(nudged(&wide, 0.0, 1.0), 1.0);
        let fine = SliderSpec { min: -5.0, max: 5.0, step: 0.01, decimals: 2, ..s };
        assert!((nudged(&fine, 0.0, 1.0) - 0.05).abs() < 1e-9);
        assert_eq!(nudged(&s, 10.0, f64::NAN), 10.0);
        let stepless = SliderSpec { step: f64::NAN, ..s };
        assert!((nudged(&stepless, 10.0, 1.0) - 10.5).abs() < 1e-9);
        let flat = SliderSpec { min: 1.0, max: 1.0, step: f64::NAN, ..s };
        assert_eq!(nudged(&flat, 7.0, 1.0), 1.0);
    }

    #[test]
    fn tuning_nudges_are_two_percent_of_the_range() {
        let fill = SliderSpec { min: 0.0, max: 1.0, step: 0.001, decimals: 3, ..EXPOSURE };
        assert!((tuning_nudged(&fill, 0.5, 1.0) - 0.52).abs() < 1e-9);
        assert!((tuning_nudged(&fill, 0.5, -1.0) - 0.48).abs() < 1e-9);
        assert_eq!(tuning_nudged(&fill, 0.99, 1.0), 1.0, "clamped");
        let temp = SliderSpec { min: -1.0, max: 1.0, step: 0.0, ..fill };
        assert!((tuning_nudged(&temp, 0.0, 1.0) - 0.04).abs() < 1e-9);
        assert_eq!(tuning_nudged(&temp, 0.3, f64::NAN), 0.3);
        assert_eq!(tuning_nudged(&temp, f64::NAN, 1.0), temp.snap(temp.default + 0.04));
    }

    const SIZE: SliderSpec<'static> =
        SliderSpec { id: "slider:export.size", label: "Resize to", min: 100.0, max: 4000.0, default: 1600.0, step: 1.0, decimals: 0, unit: "pixels" };
    const STOPS: [f64; 8] = [480.0, 640.0, 800.0, 1024.0, 1280.0, 1600.0, 2048.0, 3840.0];

    #[test]
    fn values_snap_to_a_stop_within_reach_only() {
        assert_eq!(snap_to_stop(&SIZE, &STOPS, 1640.0, 50.0), 1600.0);
        assert_eq!(snap_to_stop(&SIZE, &STOPS, 1560.2, 50.0), 1600.0);
        assert_eq!(snap_to_stop(&SIZE, &STOPS, 1700.4, 50.0), 1700.0, "out of reach: on the steps");
        assert_eq!(snap_to_stop(&SIZE, &STOPS, 1150.0, 200.0), 1024.0, "the nearest stop wins");
        assert_eq!(snap_to_stop(&SIZE, &[], 1640.0, 50.0), 1640.0);
        // Hostile input: NaN stops and reach, stops outside the range, a NaN value.
        assert_eq!(snap_to_stop(&SIZE, &[f64::NAN, 9000.0], 3990.0, 50.0), 4000.0, "a stop past the end is the end");
        assert_eq!(snap_to_stop(&SIZE, &STOPS, 1610.0, f64::NAN), 1610.0);
        assert_eq!(snap_to_stop(&SIZE, &STOPS, f64::NAN, 1.0), 1600.0, "NaN is the default, a stop");
    }

    #[test]
    fn keys_step_from_stop_to_stop_then_to_the_ends() {
        assert_eq!(next_stop(&SIZE, &STOPS, 1600.0, true), 2048.0);
        assert_eq!(next_stop(&SIZE, &STOPS, 1600.0, false), 1280.0);
        assert_eq!(next_stop(&SIZE, &STOPS, 1700.0, false), 1600.0);
        assert_eq!(next_stop(&SIZE, &STOPS, 3840.0, true), 4000.0);
        assert_eq!(next_stop(&SIZE, &STOPS, 4000.0, true), 4000.0);
        assert_eq!(next_stop(&SIZE, &STOPS, 480.0, false), 100.0);
        assert_eq!(next_stop(&SIZE, &[], 1600.0, true), nudged(&SIZE, 1600.0, 1.0), "no stops: a nudge");
        assert_eq!(number(&SIZE, 1600.0), "1600");
    }

    #[test]
    fn zoom_bounds_are_ordered_and_finite() {
        assert_eq!(zoom_bounds(&(64.0..=320.0)), (64.0, 320.0));
        assert_eq!(zoom_bounds(&(320.0..=64.0)), (64.0, 320.0));
        assert_eq!(zoom_bounds(&(f32::NAN..=1.0)), (0.0, 1.0));
    }
}
