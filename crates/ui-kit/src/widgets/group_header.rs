//! The import tray's group header (`organizing.md` §3, `design-system.md` §6): one per gap of more
//! than two hours in the capture times, its clock button selecting the whole group.

use egui::{CornerRadius, Id, Rect, Sense, Ui, Vec2, WidgetType, pos2, vec2};

use super::{Target, focus_ring, one_line, register, tooltip};
use crate::Tokens;
use crate::fonts;
use crate::icons::{self, ViewingIcon};
use crate::registry::Role;
use crate::text::{self, Text};

/// The edge of the header's clock button (points).
pub const GROUP_CLOCK: f32 = 24.0;

/// What a [`group_header`] did this pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupHeaderOut {
    /// The clock was clicked: select the group's pictures.
    pub select: bool,
    /// The header's rect.
    pub rect: Rect,
}

/// The automation id of the clock button of header `id`.
pub fn group_select_id(id: &str) -> String {
    format!("{id}/select")
}

/// A group header in the import tray: the full width of `ui` × `metrics.group_header_h` (30), no
/// fill; the clock button ([`GROUP_CLOCK`], radius 4, the 16 pt [`ViewingIcon::Clock`]) 4 pt in,
/// its glyph `icon` at rest, `text` on a `hover × 0.7` plate hovered, `pressed` held, `pick` on a
/// `tool_active` plate while the whole group is selected (`all_selected`); then the `title`
/// (SemiBold 13 `text_label`, the times, "June 4, 2026 · 10:02 – 12:40") 6 pt after it and the
/// `detail` (12 `text_dim`, "24 pictures") 10 pt after that, on one line, cut short with "…".
/// Disabled, the glyph and the texts are `text_disabled`. Registers `<id>` (role `other`, painted,
/// labelled with the title) and `<id>/select` (role `button`, active while the group is selected,
/// "Select this group").
pub fn group_header(ui: &mut Ui, t: &Tokens, id: &str, title: &str, detail: &str, all_selected: bool) -> GroupHeaderOut {
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
    let (_, rect) = ui.allocate_space(vec2(width, t.metrics.group_header_h));
    let clock = Rect::from_min_size(pos2(rect.min.x + 4.0, rect.center().y - GROUP_CLOCK / 2.0), Vec2::splat(GROUP_CLOCK));
    let select_id = group_select_id(id);
    let r = ui.interact(clock, Id::new(&select_id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(clock, CornerRadius::same(t.radius), t.pressed);
    } else if all_selected {
        p.rect_filled(clock, CornerRadius::same(t.radius), t.tool_active);
    } else if enabled && r.hovered() {
        p.rect_filled(clock, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    focus_ring(p, t, &r, clock, t.radius);
    let glyph = if !enabled {
        t.text_disabled
    } else if all_selected {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    icons::paint_viewing(p, Rect::from_center_size(clock.center(), Vec2::splat(t.metrics.icon_menu)), ViewingIcon::Clock, glyph);
    let (title_color, detail_color) = if enabled { (t.text_label, t.text_dim) } else { (t.text_disabled, t.text_disabled) };
    let mut x = clock.max.x + 6.0;
    let end = rect.max.x - 8.0;
    if end - x >= 1.0 {
        let g = one_line(p, title, fonts::semibold(t.body), title_color, end - x);
        let w = g.size().x;
        p.galley(pos2(x, rect.center().y - g.size().y / 2.0).round(), g, title_color);
        x += w + 10.0;
    }
    if !detail.is_empty() && end - x >= 1.0 {
        let g = one_line(p, detail, fonts::regular(t.count), detail_color, end - x);
        p.galley(pos2(x, rect.center().y - g.size().y / 2.0).round(), g, detail_color);
    }
    register(ui.ctx(), id, Role::Other, title, Target::Painted { rect, clip: ui.clip_rect() }, false);
    let texts = text::texts(ui.ctx());
    let name = texts.get(Text::SelectGroup);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, all_selected, name));
    register(ui.ctx(), &select_id, Role::Button, name, Target::Widget(&r), all_selected);
    let r = tooltip(r, t, name, Some(texts.get(Text::SelectGroupDetail)));
    GroupHeaderOut { select: r.clicked(), rect }
}
