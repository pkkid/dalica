//! Tab 1, Basic fixes (`editing.md` §6): the grid of tool buttons, three to a row, each an icon tile
//! above its label, then the Fill Light row ([`super::fill_light`], filled by M7.21).
//!
//! ```text
//! Crop          Straighten     Retouch
//! Auto Fix      Auto Contrast  Auto Color
//! [☀] Fill Light ─────○─────────
//! ```
//!
//! Crop, Straighten and Retouch open their tool (`edit.tool.open`), and their button shows active
//! while it is open; Auto Fix (`edit.autoFix`, Ctrl+Shift+E), Auto Contrast and Auto Color apply at
//! once as one step of the photo's history (`photo.history.apply`). Ids `tool:<name>`.

use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{EditIcon, paint_edit};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, TOOL_BUTTON_H, Target, ToastKind, tool_button_width};
use egui::{Align2, CornerRadius, Id, Rect, Response, Sense, Ui, UiBuilder, Vec2, WidgetType, pos2, vec2};
use serde_json::json;

use super::fill_light;
use crate::app::App;
use crate::state::ToolKind;
use crate::tl;

/// What a tool button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Opens the tool (`edit.tool.open`).
    Open(ToolKind),
    /// Auto Fix (`edit.autoFix`).
    AutoFix,
    /// Appends this operation to the photo's recipe (`photo.history.apply`).
    Apply(&'static str),
}

/// The grid in reading order: id, label, glyph, tooltip, action.
pub const TOOLS: [(&str, &str, EditIcon, &str, Action); 6] = [
    ("tool:crop", "Crop", EditIcon::Crop, "Crop the photo", Action::Open(ToolKind::Crop)),
    ("tool:straighten", "Straighten", EditIcon::Straighten, "Straighten a tilted photo", Action::Open(ToolKind::Straighten)),
    ("tool:retouch", "Retouch", EditIcon::Retouch, "Remove blemishes and spots", Action::Open(ToolKind::Retouch)),
    ("tool:autofix", "Auto Fix", EditIcon::AutoFix, "Fix the contrast and color in one click", Action::AutoFix),
    ("tool:autocontrast", "Auto Contrast", EditIcon::AutoContrast, "Set the black and white points", Action::Apply("autocontrast()")),
    ("tool:autocolor", "Auto Color", EditIcon::AutoColor, "Remove a color cast", Action::Apply("autocolor()")),
];

/// A label or tooltip of [`TOOLS`] in the language in effect (the table keeps the English, which
/// the catalogs key on).
pub fn text(english: &str) -> String {
    match english {
        "Crop" => tl!("Crop"),
        "Crop the photo" => tl!("Crop the photo"),
        "Straighten" => tl!("Straighten"),
        "Straighten a tilted photo" => tl!("Straighten a tilted photo"),
        "Retouch" => tl!("Retouch"),
        "Remove blemishes and spots" => tl!("Remove blemishes and spots"),
        "Auto Fix" => tl!("Auto Fix"),
        "Fix the contrast and color in one click" => tl!("Fix the contrast and color in one click"),
        "Auto Contrast" => tl!("Auto Contrast"),
        "Set the black and white points" => tl!("Set the black and white points"),
        "Auto Color" => tl!("Auto Color"),
        "Remove a color cast" => tl!("Remove a color cast"),
        other => crate::i18n::tr(other),
    }
}

/// The columns of the grid, the gap between buttons (both ways), and the gap above the Fill Light
/// row and its height.
pub const COLUMNS: u8 = 3;
pub const GAP: f32 = 4.0;
pub const FILL_GAP: f32 = 8.0;
pub const FILL_H: f32 = 45.0;

/// The rects of the six buttons and of the Fill Light row inside `rect`.
pub fn layout(t: &Tokens, rect: Rect) -> ([Rect; 6], Rect) {
    let w = tool_button_width(t, rect.width(), COLUMNS, GAP);
    let cells = std::array::from_fn(|i| {
        let (col, row) = ((i % 3) as f32, (i / 3) as f32);
        Rect::from_min_size(pos2(rect.min.x + col * (w + GAP), rect.min.y + row * (TOOL_BUTTON_H + GAP)), vec2(w, TOOL_BUTTON_H))
    });
    let top = rect.min.y + 2.0 * TOOL_BUTTON_H + GAP + FILL_GAP;
    (cells, Rect::from_min_size(pos2(rect.min.x, top), vec2(rect.width(), FILL_H)))
}

/// Draws the grid and the Fill Light row into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let (cells, fill) = layout(&t, ui.max_rect());
    let open = app.state().edit.tool;
    let photo = app.state().edit.photo.is_some();
    let mut clicked = None;
    for ((id, label, icon, tip, action), rect) in TOOLS.iter().zip(cells) {
        let active = matches!(action, Action::Open(kind) if open == Some(*kind));
        if tool_button(ui, &t, id, *icon, &text(label), rect, active, photo, &text(tip)).clicked() {
            clicked = Some(*action);
        }
    }
    if let Some(action) = clicked {
        run(app, action);
    }
    let mut row = ui.new_child(UiBuilder::new().id_salt("basic.fillLight").max_rect(fill));
    fill_light::show(app, &mut row);
}

/// Runs `action` on the photo shown; a refusal is a toast.
fn run(app: &mut App, action: Action) {
    let Some(photo) = app.state().edit.photo else { return };
    let done = match action {
        Action::Open(kind) => app.run("edit.tool.open", &json!({ "tool": kind.name() })),
        Action::AutoFix => app.run("edit.autoFix", &json!({})),
        Action::Apply(op) => app.run("photo.history.apply", &json!({ "id": photo, "op": op })),
    };
    if let Err(e) = done {
        app.toast(&crate::i18n::tr(&e.to_string()), ToastKind::Caution);
    }
}

/// A Basic Fixes tool button over `rect`, drawn as `widgets::tool_button` draws it (`design-system.md`
/// §6) with an edit glyph: the `inset` icon tile (`metrics.tool_tile`) 3 pt from the top, the
/// 20 pt glyph in it, the label (12 pt, elided) below. Rest: no fill, `icon` glyph, `text_label`
/// label; hovered `hover` fill, `text` glyph and label; held `pressed`; `active` (its tool open)
/// `tool_active` fill and a `pick` glyph; disabled both `text_disabled`. Registers `id` (role
/// `tool`, active while open); the tooltip shows disabled too.
#[allow(clippy::too_many_arguments)] // the tool button's parts, as `widgets::tool_button` takes them
fn tool_button(ui: &mut Ui, t: &Tokens, id: &str, icon: EditIcon, label: &str, rect: Rect, active: bool, enabled: bool, tip: &str) -> Response {
    let m = &t.metrics;
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    if active {
        p.rect_filled(rect, radius, t.tool_active);
    } else if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(rect, radius, t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(rect, radius, t.hover);
    }
    let tile = Rect::from_min_size(pos2(rect.center().x - m.tool_tile.x / 2.0, rect.min.y + 3.0), m.tool_tile);
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
    paint_edit(p, Rect::from_center_size(tile.center(), Vec2::splat(m.icon_tool)), icon, glyph);
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), fonts::regular(t.count), text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 4.0).max(1.0));
    let galley = p.layout_job(job);
    let row = Rect::from_min_max(pos2(rect.min.x, tile.max.y + 1.0), pos2(rect.max.x, rect.max.y - 3.0));
    p.galley(Align2::CENTER_CENTER.align_size_within_rect(galley.size(), row).min.round(), galley, text);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    widgets::register(ui.ctx(), id, Role::Tool, label, Target::Widget(&r), active);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    widgets::tooltip(r, t, tip, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_three_by_two_then_the_fill_light_row() {
        let t = Tokens::DARK;
        let (cells, fill) = layout(&t, Rect::from_min_size(pos2(8.0, 104.0), vec2(264.0, 400.0)));
        assert_eq!(cells[0], Rect::from_min_size(pos2(8.0, 104.0), vec2(85.0, TOOL_BUTTON_H)));
        assert_eq!(cells[2].min.x, 8.0 + 2.0 * 89.0);
        assert_eq!(cells[3].min, pos2(8.0, 104.0 + TOOL_BUTTON_H + GAP));
        assert!(cells.iter().all(|c| c.max.x <= 272.0 && c.width() >= t.metrics.tool_tile.x));
        assert_eq!(fill.min.y, cells[5].max.y + FILL_GAP);
        assert_eq!(fill.width(), 264.0);
        // Too narrow for three, each button keeps the icon tile's width.
        let (cells, _) = layout(&t, Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::NAN, 10.0)));
        assert!(cells.iter().all(|c| c.width() == t.metrics.tool_tile.x));
    }

    #[test]
    fn the_tools_have_unique_ids_and_open_the_three_panels() {
        let ids: std::collections::HashSet<_> = TOOLS.iter().map(|t| t.0).collect();
        assert_eq!(ids.len(), 6);
        assert!(TOOLS.iter().all(|t| dalica_ui_kit::registry::is_valid_id(t.0)));
        let opened: Vec<_> = TOOLS.iter().filter_map(|t| if let Action::Open(k) = t.4 { Some(k) } else { None }).collect();
        assert_eq!(opened, [ToolKind::Crop, ToolKind::Straighten, ToolKind::Retouch]);
    }
}
