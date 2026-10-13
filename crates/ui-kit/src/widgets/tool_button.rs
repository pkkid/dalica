//! The edit panel's tool button (`design-system.md` §6, `editing.md` §6): the Basic Fixes grid
//! draws Crop, Straighten, Retouch, Auto Fix, Auto Contrast and Auto Color with it, three to a row,
//! each an icon tile above a label.

use egui::{Align2, CornerRadius, Rect, Response, Ui, Vec2, WidgetType, pos2, vec2};

use super::{Target, allocate, focus_ring, one_line, register, tooltip};
use crate::Tokens;
use crate::fonts;
use crate::icons::{self, Icon};
use crate::registry::Role;

/// The height of a tool button: 3 pt above the 30 pt icon tile, the 16 pt label row, 3 pt below.
pub const TOOL_BUTTON_H: f32 = 52.0;

/// The width of one of `columns` tool buttons across `width` points with `gap` between them;
/// at least the icon tile's width.
pub fn tool_button_width(t: &Tokens, width: f32, columns: u8, gap: f32) -> f32 {
    let n = f32::from(columns.max(1));
    let gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
    let w = if width.is_finite() { (width - gap * (n - 1.0)) / n } else { 0.0 };
    w.floor().max(t.metrics.tool_tile.x)
}

/// A tool button `width` wide and [`TOOL_BUTTON_H`] high: the `inset` icon tile
/// (`metrics.tool_tile`, 44 × 30, radius 4) centred at the top with the 20 pt glyph in it, the label
/// (12 pt, elided) below. Rest: no fill, `icon` glyph, `text_label` label; hovered: `hover` fill,
/// `text` glyph and label; held: `pressed` fill; `active` (its tool is open): `tool_active` fill and
/// a `pick` glyph; disabled: glyph and label in `text_disabled`. `tooltip` (empty for none) shows on
/// hover, disabled too. Registers `id` (role `tool`, active while open).
#[allow(clippy::too_many_arguments)] // the signature of `design-system.md` §6, plus `ui`, the tokens, the states and the width
pub fn tool_button(ui: &mut Ui, t: &Tokens, id: &str, icon: Icon, label: &str, active: bool, enabled: bool, width: f32, tip: &str) -> Response {
    let m = &t.metrics;
    let enabled = enabled && ui.is_enabled();
    let width = if width.is_finite() { width.max(m.tool_tile.x) } else { m.tool_tile.x };
    let r = allocate(ui, id, vec2(width, TOOL_BUTTON_H), enabled);
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    if active {
        p.rect_filled(r.rect, radius, t.tool_active);
    } else if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(r.rect, radius, t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(r.rect, radius, t.hover);
    }
    let tile = Rect::from_min_size(pos2(r.rect.center().x - m.tool_tile.x / 2.0, r.rect.min.y + 3.0), m.tool_tile);
    p.rect_filled(tile, radius, t.inset);
    let (glyph, text) = if !enabled {
        (t.text_disabled, t.text_disabled)
    } else if active {
        (t.pick, t.text)
    } else if r.hovered() {
        (t.text, t.text)
    } else {
        (t.icon, t.text_label)
    };
    icons::paint(p, Rect::from_center_size(tile.center(), Vec2::splat(m.icon_tool)), icon, glyph);
    let galley = one_line(p, label, fonts::regular(t.count), text, r.rect.width() - 4.0);
    let row = Rect::from_min_max(pos2(r.rect.min.x, tile.max.y + 1.0), pos2(r.rect.max.x, r.rect.max.y - 3.0));
    let at = Align2::CENTER_CENTER.align_size_within_rect(galley.size(), row).min;
    p.galley(at.round(), galley, text);
    focus_ring(p, t, &r, r.rect, t.radius);
    register(ui.ctx(), id, Role::Tool, label, Target::Widget(&r), active);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    tooltip(r, t, tip, None)
}
