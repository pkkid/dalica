//! The borderless dropdown (`design-system.md` §6): text and a chevron that open a popup menu, and
//! the menu's items with a description line and a check mark ([`dropdown_item`]); and its rich
//! variant (M14.7, `creating.md` §3: the collage style), whose closed face and menu rows are rich
//! rows: an icon, a bold name and a one-line description ([`rich_dropdown`], [`rich_item`]).

use egui::{Color32, CornerRadius, Id, Painter, Popup, Rect, Response, Sense, Ui, Vec2, WidgetType, pos2, vec2};

use super::form::check_mark;
use super::marks::{self, Chevron};
use super::{Target, allocate, focus_ring, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::icons::{Glyph, paint_glyph};
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

/// The height of a rich row ([`rich_dropdown`]'s face and each [`rich_item`]).
pub const RICH_ROW_H: f32 = 44.0;
/// The side of a rich row's icon.
pub const RICH_ROW_ICON: f32 = 28.0;
/// The padding at a rich row's ends, and between its icon and its text.
pub const RICH_ROW_PAD: f32 = 8.0;
/// The narrowest a [`rich_item`] gets (the menu's width).
pub const RICH_ITEM_MIN_W: f32 = 260.0;
/// The room the closed face's chevron takes at its right end.
const RICH_CHEVRON_W: f32 = 20.0;

/// One rich row: an icon, a bold name and a one-line description ("Picture Pile" over "Looks
/// like a pile of scattered pictures").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RichRow<'a> {
    pub glyph: Glyph,
    pub name: &'a str,
    pub description: &'a str,
}

/// A rich row's text as painted: the name and the description, and whether each was cut short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichRowText {
    pub name: String,
    pub description: String,
    pub name_elided: bool,
    pub description_elided: bool,
}

/// Where a rich row's text starts, from its left edge.
fn rich_text_x() -> f32 {
    RICH_ROW_PAD + RICH_ROW_ICON + RICH_ROW_PAD
}

/// The narrowest a rich row is with its name whole: the icon, the name (SemiBold 13) and, for a
/// dropdown's face, the chevron. A rich row is never narrower, so its name is never cut.
pub fn rich_row_min_width(painter: &Painter, t: &Tokens, row: &RichRow<'_>, chevron: bool) -> f32 {
    let name = painter.layout_no_wrap(row.name.to_owned(), fonts::semibold(t.body), t.text).size().x;
    let end = if chevron { RICH_CHEVRON_W } else { 0.0 } + RICH_ROW_PAD;
    (rich_text_x() + name + end).ceil()
}

/// The galleys of a rich row `width` wide: the name whole (the row is widened to
/// [`rich_row_min_width`] first), the description cut short with "…" to the room left.
fn rich_galleys(
    painter: &Painter,
    t: &Tokens,
    row: &RichRow<'_>,
    width: f32,
    chevron: bool,
    colors: (Color32, Color32),
) -> (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>) {
    let width = if width.is_finite() { width } else { 0.0 }.max(rich_row_min_width(painter, t, row, chevron));
    let room = width - rich_text_x() - RICH_ROW_PAD;
    let name = painter.layout_no_wrap(row.name.to_owned(), fonts::semibold(t.body), colors.0);
    let detail = one_line(painter, row.description, fonts::regular(t.small), colors.1, room);
    (name, detail)
}

/// The text a rich row `width` wide shows ([`rich_dropdown`]'s face when `chevron`, else a
/// [`rich_item`]): the name always whole, the description cut short with "…" when it does not fit.
pub fn rich_row_text(painter: &Painter, t: &Tokens, row: &RichRow<'_>, width: f32, chevron: bool) -> RichRowText {
    let (name, detail) = rich_galleys(painter, t, row, width, chevron, (t.text, t.text_dim));
    RichRowText { name: shown_text(&name), description: shown_text(&detail), name_elided: name.elided, description_elided: detail.elided }
}

/// The text a galley shows, "…" included where it was cut short.
fn shown_text(galley: &egui::Galley) -> String {
    galley.rows.iter().flat_map(|row| row.glyphs.iter().map(|g| g.chr)).collect()
}

/// Paints `row` in `rect`: the icon centred in its column, the name over the description.
fn paint_rich_row(p: &Painter, t: &Tokens, rect: Rect, row: &RichRow<'_>, chevron: bool, colors: (Color32, Color32, Color32)) {
    let (glyph, name_color, detail_color) = colors;
    let icon = Rect::from_center_size(pos2(rect.min.x + RICH_ROW_PAD + RICH_ROW_ICON / 2.0, rect.center().y), Vec2::splat(RICH_ROW_ICON));
    paint_glyph(p, icon, row.glyph, glyph);
    let (name, detail) = rich_galleys(p, t, row, rect.width(), chevron, (name_color, detail_color));
    let x = rect.min.x + rich_text_x();
    let gap = 2.0;
    let top = rect.center().y - (name.size().y + gap + detail.size().y) / 2.0;
    let name_w = name.size().x;
    let name_h = name.size().y;
    p.galley(pos2(x, top).round(), name, name_color);
    p.galley(pos2(x, top + name_h + gap).round(), detail, detail_color);
    if chevron {
        let at = pos2((x + name_w + RICH_CHEVRON_W / 2.0).min(rect.max.x - RICH_CHEVRON_W / 2.0), top + name_h / 2.0);
        marks::chevron(p, Rect::from_center_size(at, Vec2::splat(t.metrics.icon_badge)), Chevron::Down, name_color);
    }
}

/// The rich dropdown (`creating.md` §3, the collage style): `row` as its closed face, `width` wide
/// (never narrower than [`rich_row_min_width`], so the name stays whole and the description is cut
/// short instead), [`RICH_ROW_H`] high: the icon ([`RICH_ROW_ICON`]), the name in SemiBold 13 with
/// a chevron after it, the description under it in 11 pt `text_dim`. No border, as [`dropdown`]:
/// `hover × 0.7` fill hovered, `pressed` while held or open; the name and chevron `text`, the icon
/// `pick` while open; disabled all `text_disabled`. A click toggles the popup menu `menu` fills
/// (with [`rich_item`]s), anchored below. Registered as a button, active while open, labelled
/// "name: description".
pub fn rich_dropdown(ui: &mut Ui, t: &Tokens, id: &str, row: &RichRow<'_>, width: f32, menu: impl FnOnce(&mut Ui)) -> Response {
    let enabled = ui.is_enabled();
    let width = if width.is_finite() { width } else { 0.0 }.max(rich_row_min_width(ui.painter(), t, row, true));
    let r = allocate(ui, id, vec2(width, RICH_ROW_H), enabled);
    let open = Popup::is_id_open(ui.ctx(), dropdown_popup_id(id));
    let p = ui.painter();
    if enabled && (open || r.is_pointer_button_down_on()) {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    let colors = if !enabled {
        (t.text_disabled, t.text_disabled, t.text_disabled)
    } else if open {
        (t.pick, t.text, t.text_dim)
    } else if r.hovered() {
        (t.text, t.text, t.text_dim)
    } else {
        (t.icon, t.text_label, t.text_dim)
    };
    paint_rich_row(p, t, r.rect, row, true, colors);
    focus_ring(p, t, &r, r.rect, t.radius);
    let label = format!("{}: {}", row.name, row.description);
    register(ui.ctx(), id, Role::Button, &label, Target::Widget(&r), open);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::ComboBox, enabled, &label));
    if enabled {
        Popup::menu(&r).show(menu);
    }
    r
}

/// One row of a [`rich_dropdown`]'s menu: `row` as a rich row, as wide as the menu (at least
/// [`RICH_ITEM_MIN_W`], at most [`DROPDOWN_ITEM_MAX_W`]; the description cut short past that),
/// [`RICH_ROW_H`] high. Checked (the style in use): `tool_active` fill and a `pick` icon; hovered
/// `hover` fill (`pressed` held) with a `text` name; disabled all `text_disabled`. Registered as
/// `id` (role `menuItem`, active when checked, labelled "name: description"). A click on it closes
/// the menu.
pub fn rich_item(ui: &mut Ui, t: &Tokens, id: &str, row: &RichRow<'_>, checked: bool) -> Response {
    let enabled = ui.is_enabled();
    let p = ui.painter();
    let natural = rich_text_x()
        + p.layout_no_wrap(row.name.to_owned(), fonts::semibold(t.body), t.text)
            .size()
            .x
            .max(p.layout_no_wrap(row.description.to_owned(), fonts::regular(t.small), t.text_dim).size().x)
        + RICH_ROW_PAD;
    let room = ui.available_width();
    let width = natural
        .max(if room.is_finite() && room <= DROPDOWN_ITEM_MAX_W { room } else { 0.0 })
        .clamp(RICH_ITEM_MIN_W, DROPDOWN_ITEM_MAX_W)
        .max(rich_row_min_width(p, t, row, false));
    let (_, rect) = ui.allocate_space(vec2(width, RICH_ROW_H));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let hot = enabled && (r.hovered() || r.has_focus());
    let p = ui.painter();
    if hot {
        p.rect_filled(rect, CornerRadius::same(t.radius), if r.is_pointer_button_down_on() { t.pressed } else { t.hover });
    } else if checked {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.tool_active);
    }
    let colors = if !enabled {
        (t.text_disabled, t.text_disabled, t.text_disabled)
    } else if checked {
        (t.pick, t.text, t.text_dim)
    } else if hot {
        (t.text, t.text, t.text_dim)
    } else {
        (t.icon, t.text_label, t.text_dim)
    };
    paint_rich_row(p, t, rect, row, false, colors);
    let label = format!("{}: {}", row.name, row.description);
    register(ui.ctx(), id, Role::MenuItem, &label, Target::Widget(&r), checked);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::SelectableLabel, enabled, checked, &label));
    r
}
