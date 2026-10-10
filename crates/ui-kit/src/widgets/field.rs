//! Text fields (`design-system.md` §6): the plain [`text_field`] and the toolbar's [`search_field`].
//! Both paint their own frame (`field` fill, `field_border`, `accent` and `canvas` when focused)
//! around a frameless egui `TextEdit`, so they follow the tokens rather than egui's visuals.

use egui::{CornerRadius, Id, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2, pos2, vec2};

use super::marks;
use super::{Target, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// The frame of a field in `rect`: `field` (or `canvas` focused) with a 1 pt border, `field_border`
/// at rest, `button_border` hovered and `accent` focused (`focus_width`, the focus ring).
fn frame(t: &Tokens, rect: Rect, hovered: bool, focused: bool, enabled: bool) -> Vec<Shape> {
    let fill = if focused { t.canvas } else { t.field };
    let (width, color) = if focused {
        (t.focus_width, t.accent)
    } else if enabled && hovered {
        (1.0, t.button_border)
    } else {
        (1.0, t.field_border)
    };
    vec![
        Shape::rect_filled(rect, CornerRadius::same(t.radius), fill),
        Shape::rect_stroke(rect, CornerRadius::same(t.radius), Stroke::new(width, color), StrokeKind::Inside),
    ]
}

/// A single-line text field `width × 24` showing `hint` in `text_dim` while empty. Text is 13 pt
/// `text` (`text_disabled` disabled). Registered as `id` on the text edit, labelled with `hint`.
pub fn text_field(ui: &mut Ui, t: &Tokens, id: &str, text: &mut String, hint: &str, width: f32) -> Response {
    let enabled = ui.is_enabled();
    let width = if width.is_finite() { width.max(24.0) } else { 120.0 };
    let (_, rect) = ui.allocate_space(vec2(width, t.metrics.text_field_h));
    let background = ui.painter().add(Shape::Noop);
    let inner = rect.shrink2(vec2(8.0, 0.0));
    let r = edit(ui, t, id, text, hint, inner, enabled);
    let shapes = frame(t, rect, r.hovered(), r.has_focus(), enabled);
    ui.painter().set(background, Shape::Vec(shapes));
    register(ui.ctx(), id, Role::Other, hint, Target::Widget(&r), r.has_focus());
    r
}

fn edit(ui: &mut Ui, t: &Tokens, id: &str, text: &mut String, hint: &str, rect: Rect, enabled: bool) -> Response {
    let hint = egui::RichText::new(hint).color(t.text_dim).font(fonts::regular(t.body));
    let edit = TextEdit::singleline(text)
        .id(Id::new(id))
        .frame(egui::Frame::NONE)
        .hint_text(hint)
        .font(fonts::regular(t.body))
        .text_color(if enabled { t.text } else { t.text_disabled })
        .margin(egui::Margin::ZERO)
        .desired_width(rect.width())
        .vertical_align(egui::Align::Center);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.add_enabled(enabled, edit)
}

/// What a [`search_field`] did this pass.
#[derive(Debug, Clone)]
pub struct SearchOut {
    /// The text edit's response.
    pub response: Response,
    /// The text changed (typed or cleared): filter again.
    pub changed: bool,
    /// The clear button was clicked.
    pub cleared: bool,
}

/// The search field (`design-system.md` §6): `width × 28`, radius 4, a 14 pt magnifier and the
/// `hint` placeholder in `text_dim`, and a clear (×) button registered as `clear_id` while the text is
/// not empty; clearing keeps the focus in the field. Filters run on every keystroke: `changed` says
/// when.
pub fn search_field(ui: &mut Ui, t: &Tokens, id: &str, clear_id: &str, text: &mut String, hint: &str, width: f32) -> SearchOut {
    let enabled = ui.is_enabled();
    let width = if width.is_finite() { width.max(64.0) } else { 160.0 };
    let (_, rect) = ui.allocate_space(vec2(width, t.metrics.search_h));
    let background = ui.painter().add(Shape::Noop);
    let has_text = !text.is_empty();
    let clear_rect = Rect::from_center_size(pos2(rect.max.x - 14.0, rect.center().y), Vec2::splat(18.0));
    let right = if has_text { clear_rect.min.x - 2.0 } else { rect.max.x - 8.0 };
    let inner = Rect::from_min_max(pos2(rect.min.x + 28.0, rect.min.y), pos2(right.max(rect.min.x + 32.0), rect.max.y));
    let before = text.clone();
    let mut r = edit(ui, t, id, text, hint, inner, enabled);
    let mut cleared = false;
    let mut clear_shapes = Vec::new();
    if has_text {
        let c = ui.interact(clear_rect, Id::new(clear_id), if enabled { Sense::click() } else { Sense::hover() });
        if enabled && c.hovered() {
            clear_shapes.push(Shape::circle_filled(clear_rect.center(), 8.0, t.hover));
        }
        if c.has_focus() {
            clear_shapes.push(Shape::circle_stroke(clear_rect.center(), 9.0, Stroke::new(t.focus_width, t.accent)));
        }
        let glyph = if !enabled {
            t.text_disabled
        } else if c.hovered() {
            t.text
        } else {
            t.text_dim
        };
        marks::cross(ui.painter(), clear_rect.shrink(2.0), glyph);
        register(ui.ctx(), clear_id, Role::Button, "Clear search", Target::Widget(&c), false);
        c.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "Clear search"));
        if c.clicked() {
            text.clear();
            cleared = true;
            r.request_focus();
        }
    }
    let changed = *text != before;
    if changed {
        r.mark_changed();
    }
    let focused = r.has_focus();
    let mut shapes = frame(t, rect, r.hovered(), focused, enabled);
    shapes.extend(clear_shapes);
    ui.painter().set(background, Shape::Vec(shapes));
    let glass = Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), Vec2::splat(14.0));
    marks::magnifier(ui.painter(), glass, if enabled { t.text_dim } else { t.text_disabled });
    register(ui.ctx(), id, Role::Other, hint, Target::Widget(&r), focused);
    SearchOut { response: r, changed, cleared }
}
