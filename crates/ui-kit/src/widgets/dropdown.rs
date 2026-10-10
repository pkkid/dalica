//! The borderless dropdown (`design-system.md` §6): text and a chevron that open a popup menu.

use egui::{CornerRadius, Id, Popup, Rect, Response, Ui, Vec2, WidgetType, pos2, vec2};

use super::marks::{self, Chevron};
use super::{Target, allocate, focus_ring, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// The id of the popup a [`dropdown`] with automation id `id` opens (to open or close it from code).
pub fn dropdown_popup_id(id: &str) -> Id {
    Id::new(id).with("popup")
}

/// `text` (13 pt, `text_label`) followed by a 12 pt chevron, 22 pt high, no border; `hover × 0.7`
/// fill and `text` hovered, `pressed` fill while held or open. A click toggles the popup menu that
/// `menu` fills, anchored below the dropdown; egui keeps it inside the window (flipping above when
/// there is no room below), and a click on an item or outside closes it. Registered as a button
/// whose active state is "open".
pub fn dropdown(ui: &mut Ui, t: &Tokens, id: &str, text: &str, menu: impl FnOnce(&mut Ui)) -> Response {
    let enabled = ui.is_enabled();
    let font = fonts::regular(t.body);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t.text_label);
    let size = vec2((galley.size().x + 28.0).round(), t.metrics.interact_h);
    let r = allocate(ui, id, size, enabled);
    let open = Popup::is_id_open(ui.ctx(), dropdown_popup_id(id));
    let p = ui.painter();
    if enabled && (open || r.is_pointer_button_down_on()) {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    let color = if !enabled {
        t.text_disabled
    } else if open || r.hovered() {
        t.text
    } else {
        t.text_label
    };
    p.galley(pos2(r.rect.min.x + 6.0, r.rect.center().y - galley.size().y / 2.0).round(), galley, color);
    let chevron = Rect::from_center_size(pos2(r.rect.max.x - 11.0, r.rect.center().y), Vec2::splat(t.metrics.icon_badge));
    marks::chevron(p, chevron, Chevron::Down, color);
    focus_ring(p, t, &r, r.rect, t.radius);
    register(ui.ctx(), id, Role::Button, text, Target::Widget(&r), open);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::ComboBox, enabled, text));
    if enabled {
        Popup::menu(&r).show(menu);
    }
    r
}
