//! The borderless dropdown (`design-system.md` §6): text and a chevron that open a popup menu, and
//! the menu's items with a description line and a check mark ([`dropdown_item`]).

use egui::{CornerRadius, Id, Popup, Rect, Response, Sense, Ui, Vec2, WidgetType, pos2, vec2};

use super::form::check_mark;
use super::marks::{self, Chevron};
use super::{Target, allocate, focus_ring, one_line, register};
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

/// The narrowest a [`dropdown_item`] gets.
pub const DROPDOWN_ITEM_MIN_W: f32 = 200.0;
/// The widest a [`dropdown_item`] gets; longer titles and descriptions are cut short with "…".
pub const DROPDOWN_ITEM_MAX_W: f32 = 420.0;
/// The column the check mark sits in, left of the title.
const CHECK_COLUMN: f32 = 24.0;

/// One item of a [`dropdown`]'s menu with a description line (`creating.md` §2: "Automatic" over
/// "Keeps the original's quality"): the `title` 13 pt `text_label` and, when not empty, the
/// `description` under it in 11 pt `text_dim`, after a column holding a check mark when `checked`;
/// `hover` fill (radius 4) with `text` title and mark hovered; disabled all `text_disabled`. As
/// wide as the menu (at least [`DROPDOWN_ITEM_MIN_W`], at most [`DROPDOWN_ITEM_MAX_W`]); 24 pt
/// high, 38 with a description. Registered as `id` (role `menuItem`, active when checked). A click
/// on it closes the menu.
pub fn dropdown_item(ui: &mut Ui, t: &Tokens, id: &str, title: &str, description: &str, checked: bool) -> Response {
    let enabled = ui.is_enabled();
    let (title_font, detail_font) = (fonts::regular(t.body), fonts::regular(t.small));
    let p = ui.painter();
    let natural = CHECK_COLUMN
        + 10.0
        + p.layout_no_wrap(title.to_owned(), title_font.clone(), t.text_label)
            .size()
            .x
            .max(p.layout_no_wrap(description.to_owned(), detail_font.clone(), t.text_dim).size().x);
    let room = ui.available_width();
    let width = natural.max(if room.is_finite() && room <= DROPDOWN_ITEM_MAX_W { room } else { 0.0 }).clamp(DROPDOWN_ITEM_MIN_W, DROPDOWN_ITEM_MAX_W);
    let height = if description.is_empty() { t.metrics.text_field_h } else { 38.0 };
    let (_, rect) = ui.allocate_space(vec2(width, height));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let hot = enabled && (r.hovered() || r.has_focus());
    let p = ui.painter();
    if hot {
        p.rect_filled(rect, CornerRadius::same(t.radius), if r.is_pointer_button_down_on() { t.pressed } else { t.hover });
    }
    let (title_color, detail_color) = if !enabled {
        (t.text_disabled, t.text_disabled)
    } else if hot {
        (t.text, t.text_dim)
    } else {
        (t.text_label, t.text_dim)
    };
    let text_x = rect.min.x + CHECK_COLUMN;
    let max = rect.max.x - text_x - 8.0;
    let title_g = one_line(p, title, title_font, title_color, max);
    let first = if description.is_empty() { rect.center().y } else { rect.min.y + 4.0 + title_g.size().y / 2.0 };
    if checked {
        check_mark(p, Rect::from_center_size(pos2(rect.min.x + CHECK_COLUMN / 2.0, first), Vec2::splat(12.0)), title_color);
    }
    p.galley(pos2(text_x, first - title_g.size().y / 2.0).round(), title_g.clone(), title_color);
    if !description.is_empty() {
        let detail = one_line(p, description, detail_font, detail_color, max);
        p.galley(pos2(text_x, first + title_g.size().y / 2.0 + 1.0).round(), detail, detail_color);
    }
    let label = if description.is_empty() { title.to_owned() } else { format!("{title}: {description}") };
    register(ui.ctx(), id, Role::MenuItem, &label, Target::Widget(&r), checked);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::SelectableLabel, enabled, checked, &label));
    r
}
