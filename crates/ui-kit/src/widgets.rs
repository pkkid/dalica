//! Shared widgets (`design-system.md` §6). Panels compose these and never re-implement them. Each
//! one takes an automation id, registers itself in the [`registry`](crate::registry) and gives
//! AccessKit a role and a label.

use egui::{Align2, Color32, CornerRadius, Id, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetType, vec2};

use crate::Tokens;
use crate::fonts;
use crate::icons::{self, Icon};
use crate::registry::{self, Role, WidgetInfo};

/// Allocates `size`, gives it the stable id `id` and clicks if `enabled`.
fn allocate(ui: &mut Ui, id: &str, size: Vec2, enabled: bool) -> Response {
    let (_, rect) = ui.allocate_space(size);
    let enabled = enabled && ui.is_enabled();
    ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() })
}

fn fill(t: &Tokens, r: &Response, active: bool) -> Color32 {
    if active || r.is_pointer_button_down_on() {
        t.pressed
    } else if r.hovered() {
        t.hover
    } else {
        t.button
    }
}

/// A button with a text label: SemiBold 11.5 `text`, `(text_w + 18).max(37) × 24`, radius 4, fill
/// `button` / `hover` / `pressed`, 1 pt `button_border`.
pub fn text_button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, active: bool) -> Response {
    let enabled = ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), fonts::semibold(11.5), if enabled { t.text } else { t.text_disabled });
    let size = vec2((galley.size().x + 18.0).max(37.0).round(), 24.0);
    let r = allocate(ui, id, size, enabled);
    let p = ui.painter();
    p.rect(r.rect, CornerRadius::same(t.radius), fill(t, &r, active), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let pos = r.rect.center() - galley.size() / 2.0;
    p.galley(pos.round(), galley, t.text);
    registry::register(ui.ctx(), WidgetInfo::new(id, Role::Button, label, r.rect, enabled, active));
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    r
}

/// A square icon button of edge `size`: a highlight square `min(32, size)` with radius 4,
/// `tool_active` when active, `hover × 0.7` when hovered; the glyph is `icon` at rest, `text` on
/// hover, `pick` when active and `text_disabled` when disabled.
#[allow(clippy::too_many_arguments)] // the signature of `design-system.md` §6, plus `ui` and the tokens
pub fn icon_button(ui: &mut Ui, t: &Tokens, id: &str, icon: Icon, size: f32, active: bool, enabled: bool, tooltip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = allocate(ui, id, vec2(size, size), enabled);
    let p = ui.painter();
    let highlight = Rect::from_center_size(r.rect.center(), Vec2::splat(size.min(32.0)));
    if active {
        p.rect_filled(highlight, CornerRadius::same(t.radius), t.tool_active);
    } else if enabled && r.hovered() {
        p.rect_filled(highlight, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    let glyph = if !enabled {
        t.text_disabled
    } else if active {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    icons::paint(p, Rect::from_center_size(r.rect.center(), Vec2::splat((size * 0.6).round())), icon, glyph);
    registry::register(ui.ctx(), WidgetInfo::new(id, Role::IconButton, tooltip, r.rect, enabled, active));
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, tooltip));
    r.on_hover_text(tooltip)
}

/// The star toggle (36 × 24, `design-system.md` §4): one star glyph, filled `star` when on,
/// outlined `icon` when off, on a button plate.
pub fn star_toggle(ui: &mut Ui, t: &Tokens, id: &str, on: bool) -> Response {
    let enabled = ui.is_enabled();
    let r = allocate(ui, id, vec2(36.0, 24.0), enabled);
    let p = ui.painter();
    p.rect(r.rect, CornerRadius::same(t.radius), fill(t, &r, false), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let glyph = Rect::from_center_size(r.rect.center(), Vec2::splat(16.0));
    let (icon, color) = match (enabled, on) {
        (false, _) => (if on { Icon::Star } else { Icon::StarOutline }, t.text_disabled),
        (true, true) => (Icon::Star, t.star),
        (true, false) => (Icon::StarOutline, t.icon),
    };
    icons::paint(p, glyph, icon, color);
    registry::register(ui.ctx(), WidgetInfo::new(id, Role::Toggle, "Star", r.rect, enabled, on));
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Checkbox, enabled, on, "Star"));
    r.on_hover_text("Star / Unstar")
}

/// Edge of a badge plate and its glyph, and the inset from the image corner (points).
const BADGE_PLATE: f32 = 18.0;
const BADGE_GLYPH: f32 = 12.0;
const BADGE_INSET: f32 = 4.0;

/// Paints a row of badges inside the bottom-right corner of `image`, right-aligned in the order
/// given (so the last one is rightmost), each a 12 pt glyph on a `badge_plate`. Returns the rects of
/// the plates, left to right. Nothing is painted if the image is too small to hold one plate.
pub fn badge_row(painter: &Painter, t: &Tokens, image: Rect, glyphs: &[(Icon, Color32)]) -> Vec<Rect> {
    if image.width() < BADGE_PLATE + 2.0 * BADGE_INSET || image.height() < BADGE_PLATE + 2.0 * BADGE_INSET {
        return Vec::new();
    }
    let n = glyphs.len() as f32;
    let mut x = image.max.x - BADGE_INSET - n * BADGE_PLATE - (n - 1.0).max(0.0) * 2.0;
    let y = image.max.y - BADGE_INSET - BADGE_PLATE;
    let mut rects = Vec::with_capacity(glyphs.len());
    for &(icon, color) in glyphs {
        let plate = Rect::from_min_size(egui::pos2(x, y), Vec2::splat(BADGE_PLATE));
        painter.rect_filled(plate, CornerRadius::same(3), t.badge_plate);
        icons::paint(painter, Rect::from_center_size(plate.center(), Vec2::splat(BADGE_GLYPH)), icon, color);
        rects.push(plate);
        x += BADGE_PLATE + 2.0;
    }
    rects
}

/// The star badge of a starred photo, registered as `id` (e.g. `"grid.cell.42.star"`).
pub fn star_badge(painter: &Painter, t: &Tokens, id: &str, image: Rect) -> Option<Rect> {
    let plate = badge_row(painter, t, image, &[(Icon::Star, t.star)]).pop()?;
    registry::register(painter.ctx(), WidgetInfo::new(id, Role::Badge, "Starred", plate, true, true));
    Some(plate)
}

/// A label in `font`, `color`, aligned in `rect` (a convenience for panels).
pub fn label_in(painter: &Painter, rect: Rect, align: Align2, text: &str, font: egui::FontId, color: Color32) -> Rect {
    painter.text(align.pos_in_rect(&rect), align, text, font, color)
}
