//! Form rows (`design-system.md` §6, `creating.md` §2): the two-column layout of a settings dialog
//! such as Export to Folder, with the labels right-aligned in one column and the controls
//! left-aligned in the next ([`Form`]); a hint line under a control ([`hint`]); rows disabled as a
//! group ([`form_group`]); and the controls painted for it: [`checkbox`], [`radio`] and
//! [`radio_row`] (a radio button followed by the controls it enables, such as a field and a
//! slider).
//!
//! Every control line is one `text_field_h` (24 pt) high, so a row's label sits on the middle of
//! its first line whatever the control. Labels are text, not controls: they register no id.

use egui::text::LayoutJob;
use egui::{
    Align, Color32, CornerRadius, Id, Layout, Painter, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, WidgetType, pos2, vec2,
};

use super::{Target, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// The space between the label column and the controls.
pub const FORM_GAP: f32 = 10.0;
/// The space between two rows (lines within a row use the ui's `item_spacing.y`).
pub const FORM_ROW_GAP: f32 = 8.0;
/// The widest a label column grows; longer labels are cut short with "…".
pub const FORM_LABEL_MAX: f32 = 240.0;
/// The side of a [`checkbox`]'s box.
pub const CHECK_BOX: f32 = 14.0;
/// The radius of a [`radio`]'s circle.
pub const RADIO_R: f32 = 7.0;
/// Where a [`checkbox`]'s or [`radio`]'s label starts, from the control's left edge.
pub const CHOICE_LABEL_X: f32 = 22.0;

/// The column layout of a form: how wide its label column is. Make one per form from all its
/// labels ([`Form::new`]), so every row's controls start at the same x.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Form {
    /// The width of the label column, in points.
    pub label_w: f32,
}

impl Form {
    /// A form whose label column fits the widest of `labels` in the body font (at most
    /// [`FORM_LABEL_MAX`]).
    pub fn new(ui: &Ui, t: &Tokens, labels: &[&str]) -> Form {
        let font = fonts::regular(t.body);
        let widest = labels.iter().map(|l| ui.painter().layout_no_wrap((*l).to_owned(), font.clone(), t.text_label).size().x).fold(0.0, f32::max);
        Form { label_w: widest.ceil().clamp(0.0, FORM_LABEL_MAX) }
    }

    /// One row: `label` right-aligned in the label column, on the middle of the first control line,
    /// and `add`'s controls top-down in the column after it, left-aligned. The label is
    /// `text_label`, `text_disabled` when the ui is disabled ([`form_group`]). An empty label
    /// leaves the column blank (a row continuing the one above). Rows are [`FORM_ROW_GAP`] apart.
    pub fn row<R>(&self, ui: &mut Ui, t: &Tokens, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        let line_h = t.metrics.text_field_h;
        let top = ui.cursor().min;
        let right = ui.max_rect().max.x;
        let label_w = if self.label_w.is_finite() { self.label_w.clamp(0.0, FORM_LABEL_MAX) } else { 0.0 };
        let x = top.x + label_w + FORM_GAP;
        let controls = Rect::from_min_max(pos2(x, top.y), pos2(right.max(x + 1.0), ui.max_rect().max.y.max(top.y + line_h)));
        let inner = ui.scope_builder(UiBuilder::new().max_rect(controls).layout(Layout::top_down(Align::Min)), add).inner;
        if !label.is_empty() {
            let color = if ui.is_enabled() { t.text_label } else { t.text_disabled };
            let p = ui.painter();
            let galley = one_line(p, label, fonts::regular(t.body), color, label_w);
            let pos = pos2(top.x + label_w - galley.size().x, top.y + (line_h - galley.size().y) / 2.0);
            p.galley(pos.round(), galley, color);
        }
        ui.add_space((FORM_ROW_GAP - ui.spacing().item_spacing.y).max(0.0));
        inner
    }
}

/// Rows (or any controls) enabled or disabled together: disabled, every control in `add` ignores
/// the pointer and the keys, paints its disabled look, and is registered as disabled.
pub fn form_group<R>(ui: &mut Ui, enabled: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_enabled_ui(enabled, add).inner
}

/// A hint line under a control: 11 pt `text_dim` (`text_disabled` disabled), wrapped at the width
/// left in the ui. Text, not a control.
pub fn hint(ui: &mut Ui, t: &Tokens, text: &str) -> Rect {
    let color = if ui.is_enabled() { t.text_dim } else { t.text_disabled };
    let width = ui.available_width();
    let job = LayoutJob::simple(text.to_owned(), fonts::regular(t.small), color, if width.is_finite() { width.max(24.0) } else { 360.0 });
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, color);
    rect
}

/// The colour of a choice's outline: `button_border` at rest, `text_label` hovered,
/// `text_disabled` disabled.
fn outline(t: &Tokens, r: &Response, enabled: bool) -> Color32 {
    if !enabled {
        t.text_disabled
    } else if r.hovered() || r.is_pointer_button_down_on() {
        t.text_label
    } else {
        t.button_border
    }
}

/// Allocates a choice line (the mark, then `label`) and paints the label; returns the response,
/// the line and whether it takes input.
fn choice(ui: &mut Ui, t: &Tokens, id: &str, label: &str) -> (Response, Rect, bool) {
    let enabled = ui.is_enabled();
    let color = if enabled { t.text_label } else { t.text_disabled };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), fonts::regular(t.body), color);
    let w = CHOICE_LABEL_X + if label.is_empty() { -6.0 } else { galley.size().x + 2.0 };
    let (_, rect) = ui.allocate_space(vec2(w.ceil(), t.metrics.text_field_h));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let pos = pos2(rect.min.x + CHOICE_LABEL_X, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos.round(), galley, color);
    (r, rect, enabled)
}

/// A checkbox and its label, the whole line clickable: a 14 pt box (radius 3, `field` with a 1 pt
/// outline: `button_border`, `text_label` hovered) that turns `accent` with an `on_accent` check
/// mark when on; disabled, the outline, the mark and the label are `text_disabled`. A click flips
/// `on` and marks the response changed. Registered as `id` (role `toggle`, active when on).
pub fn checkbox(ui: &mut Ui, t: &Tokens, id: &str, label: &str, on: &mut bool) -> Response {
    let (mut r, rect, enabled) = choice(ui, t, id, label);
    if enabled && r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let p = ui.painter();
    let b = Rect::from_center_size(pos2(rect.min.x + 1.0 + CHECK_BOX / 2.0, rect.center().y), vec2(CHECK_BOX, CHECK_BOX));
    let radius = CornerRadius::same(3);
    if *on && enabled {
        p.rect_filled(b, radius, t.accent);
        check_mark(p, b.shrink(2.0), t.on_accent);
    } else {
        p.rect(b, radius, t.field, Stroke::new(1.0, outline(t, &r, enabled)), StrokeKind::Inside);
        if *on {
            check_mark(p, b.shrink(2.0), t.text_disabled);
        }
    }
    if r.has_focus() {
        p.rect_stroke(b.expand(1.0), CornerRadius::same(4), super::focus_stroke(t), StrokeKind::Outside);
    }
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Checkbox, enabled, *on, label));
    register(ui.ctx(), id, Role::Toggle, label, Target::Widget(&r), *on);
    r
}

/// A radio button and its label, the whole line clickable: a circle of radius 7 (`field` with a
/// 1 pt outline: `button_border`, `text_label` hovered) holding an `accent` dot when `checked`
/// (`text_disabled` disabled). The caller sets its value on `clicked()`. Registered as `id` (role
/// `toggle`, active when checked).
pub fn radio(ui: &mut Ui, t: &Tokens, id: &str, label: &str, checked: bool) -> Response {
    let (r, rect, enabled) = choice(ui, t, id, label);
    let p = ui.painter();
    let c = pos2(rect.min.x + 1.0 + RADIO_R, rect.center().y);
    p.circle(c, RADIO_R, t.field, Stroke::new(1.0, outline(t, &r, enabled)));
    if checked {
        p.circle_filled(c, RADIO_R / 2.0, if enabled { t.accent } else { t.text_disabled });
    }
    if r.has_focus() {
        p.circle_stroke(c, RADIO_R + 2.0, super::focus_stroke(t));
    }
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::RadioButton, enabled, checked, label));
    register(ui.ctx(), id, Role::Toggle, label, Target::Widget(&r), checked);
    r
}

/// A [`radio`] followed on its line by the controls it governs (`(•) Resize to: [1600] pixels
/// ───○───`): `add`'s controls are enabled only while the radio is `checked`. Returns the radio's
/// response and what `add` returned.
pub fn radio_row<R>(ui: &mut Ui, t: &Tokens, id: &str, label: &str, checked: bool, add: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
    ui.horizontal(|ui| {
        let r = radio(ui, t, id, label, checked);
        let inner = ui.add_enabled_ui(checked, add).inner;
        (r, inner)
    })
    .inner
}

/// A check mark filling `rect` (a short stroke down, a long one up), in `color`. Nothing for an
/// empty or non-finite rect.
pub(crate) fn check_mark(painter: &Painter, rect: Rect, color: Color32) {
    let side = rect.width().min(rect.height());
    if !(side.is_finite() && side > 0.0 && rect.center().is_finite()) {
        return;
    }
    let o = rect.center() - vec2(side, side) / 2.0;
    let at = |x: f32, y: f32| pos2(o.x + x * side / 20.0, o.y + y * side / 20.0);
    let width = (2.2 * side / 20.0).max(1.2);
    painter.add(Shape::line(vec![at(3.5, 10.5), at(8.0, 15.0), at(16.5, 5.5)], Stroke::new(width, color)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_mark_ignores_empty_rects() {
        let ctx = egui::Context::default();
        let count = |rect: Rect| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| check_mark(ui.painter(), rect, Color32::WHITE));
            out.textures_delta.clear();
            out.shapes.len()
        };
        let none = count(Rect::NOTHING);
        assert_eq!(count(Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(10.0, 10.0))), none);
        assert_eq!(count(Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0))), none + 1);
    }
}
