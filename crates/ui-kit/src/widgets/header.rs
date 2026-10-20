//! The collapsible section header of the library tree's groups and of dialogs (`design-system.md` §6).

use egui::{CornerRadius, Rect, Response, Ui, Vec2, WidgetType, pos2, vec2};

use super::marks::{self, Chevron};
use super::{Target, allocate, focus_ring, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// A 30 pt header across the available width: a 12 pt chevron (right when closed, down when `open`)
/// and the SemiBold 13 `title` in `text_label` (`text` hovered), elided to fit. No fill at rest,
/// `hover × 0.5` hovered, `pressed × 0.6` held. A click toggles: the caller flips `open` when the
/// response is clicked. Registered as a toggle whose active state is `open`.
pub fn section_header(ui: &mut Ui, t: &Tokens, id: &str, title: &str, open: bool) -> Response {
    let enabled = ui.is_enabled();
    let width = ui.available_width().max(t.metrics.section_header_h);
    let r = allocate(ui, id, vec2(width, t.metrics.section_header_h), enabled);
    let p = ui.painter();
    if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.pressed.gamma_multiply(0.6));
    } else if enabled && r.hovered() {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.5));
    }
    let (glyph, text) = if !enabled {
        (t.text_disabled, t.text_disabled)
    } else if r.hovered() {
        (t.text, t.text)
    } else {
        (t.icon, t.text_label)
    };
    let chevron = Rect::from_center_size(pos2(r.rect.min.x + 12.0, r.rect.center().y), Vec2::splat(t.metrics.icon_badge));
    marks::chevron(p, chevron, if open { Chevron::Down } else { Chevron::Right }, glyph);
    let x = r.rect.min.x + 24.0;
    let galley = one_line(p, title, fonts::semibold(t.body), text, r.rect.max.x - x - 6.0);
    p.galley(pos2(x, r.rect.center().y - galley.size().y / 2.0).round(), galley, text);
    focus_ring(p, t, &r, r.rect, t.radius);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::CollapsingHeader, enabled, open, title));
    super::access(&r, |node| {
        node.clear_toggled();
        node.set_expanded(open);
    });
    register(ui.ctx(), id, Role::Toggle, title, Target::Widget(&r), open);
    r
}
