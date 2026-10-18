//! The effect tabs' tiles (`design-system.md` §6, `editing.md` §8.1): an [`effect_tile`] is a live
//! preview of the photo with one effect at its defaults over the effect's name, and
//! [`effect_grid`] lays a tab's tiles out three to a row, at most four rows, in the 280-wide edit
//! panel.

use egui::{Align2, Color32, CornerRadius, Rect, Response, TextureId, Ui, Vec2, WidgetType, pos2, vec2};

use super::{Target, allocate, focus_ring, one_line, register, tooltip};
use crate::Tokens;
use crate::fonts;
use crate::icons::{EffectIcon, paint_effect};
use crate::registry::Role;

/// The space between two tiles of a grid, across and down.
pub const EFFECT_TILE_GAP: f32 = 4.0;
/// Tiles to a row.
pub const EFFECT_GRID_COLUMNS: usize = 3;
/// The most rows a tab's grid has (it never scrolls).
pub const EFFECT_GRID_ROWS: usize = 4;
/// The side of the "1-click" badge's plate, at the preview's top-right corner.
pub const EFFECT_BADGE: f32 = 16.0;

/// The preview's distance from the tile's top edge.
const PREVIEW_TOP: f32 = 3.0;
/// The badge's distance from the preview's edges.
const BADGE_INSET: f32 = 2.0;

/// One tile: what it shows and says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectTile<'a> {
    /// The effect's name in the catalogue (`sepia`, `soft_focus`): the tile registers
    /// `effect:<name>`.
    pub name: &'a str,
    /// The label under the preview ("Soft Focus").
    pub label: &'a str,
    /// What the effect does, shown on hover (a size-changing effect says so).
    pub tooltip: &'a str,
    /// The rendered preview, filling the 78 × 48 preview area; `None` until it has arrived.
    pub preview: Option<TextureId>,
    /// A one-click effect (applied at once): shows the "1-click" badge.
    pub one_click: bool,
    /// Takes clicks (no photo, or a video, disables the grid).
    pub enabled: bool,
}

/// The automation id of the tile of effect `name`: `effect:<name>` (`control-protocol.md` §7).
pub fn effect_tile_id(name: &str) -> String {
    format!("effect:{name}")
}

/// An effect tile (`metrics.effect_tile`, 86 × 69): the preview (`metrics.effect_preview`, 78 × 48,
/// square corners) 3 pt from the top, or the `placeholder` (`gray 38`) until it arrives; the label
/// below in SemiBold 11, centred and cut with "…"; for a one-click effect a [`EFFECT_BADGE`]
/// `badge_plate` square 2 pt inside the preview's top-right corner holding the 12 pt one-click
/// glyph. Rest: no fill, label `text_label`; hovered: `hover` fill (radius 4), label `text`; held:
/// `pressed`; disabled: the preview dimmed by `excluded_dim`, label and badge glyph
/// `text_disabled`. The tooltip shows on hover, disabled too. Registers `effect:<name>` (role
/// `tool`).
pub fn effect_tile(ui: &mut Ui, t: &Tokens, tile: &EffectTile<'_>) -> Response {
    let m = &t.metrics;
    let id = effect_tile_id(tile.name);
    let enabled = tile.enabled && ui.is_enabled();
    let r = allocate(ui, &id, m.effect_tile, enabled);
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(r.rect, radius, t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(r.rect, radius, t.hover);
    }
    let preview = Rect::from_min_size(pos2(r.rect.center().x - m.effect_preview.x / 2.0, r.rect.min.y + PREVIEW_TOP), m.effect_preview);
    match tile.preview {
        Some(texture) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            p.image(texture, preview, uv, Color32::WHITE);
        }
        None => {
            p.rect_filled(preview, 0, t.placeholder);
        }
    }
    if !enabled {
        p.rect_filled(preview, 0, t.excluded_dim);
    }
    let text = if !enabled {
        t.text_disabled
    } else if r.hovered() {
        t.text
    } else {
        t.text_label
    };
    if tile.one_click {
        let plate = Rect::from_min_size(pos2(preview.max.x - BADGE_INSET - EFFECT_BADGE, preview.min.y + BADGE_INSET), Vec2::splat(EFFECT_BADGE));
        p.rect_filled(plate, CornerRadius::same(3), t.badge_plate);
        let glyph = if enabled { t.text } else { t.text_disabled };
        paint_effect(p, Rect::from_center_size(plate.center(), Vec2::splat(m.icon_badge)), EffectIcon::OneClick, glyph);
    }
    let galley = one_line(p, tile.label, fonts::semibold(t.small), text, r.rect.width() - 4.0);
    let row = Rect::from_min_max(pos2(r.rect.min.x, preview.max.y), r.rect.max);
    let at = Align2::CENTER_CENTER.align_size_within_rect(galley.size(), row).min;
    p.galley(at.round(), galley, text);
    focus_ring(p, t, &r, r.rect, t.radius);
    register(ui.ctx(), &id, Role::Tool, tile.label, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, tile.label));
    tooltip(r, t, tile.label, Some(tile.tooltip))
}

/// The size of a grid of `count` tiles: three columns wide whatever the count (so a tab's grid
/// keeps its place), as many rows as the tiles fill, at most [`EFFECT_GRID_ROWS`].
pub fn effect_grid_size(t: &Tokens, count: usize) -> Vec2 {
    let tile = t.metrics.effect_tile;
    let rows = count.div_ceil(EFFECT_GRID_COLUMNS).min(EFFECT_GRID_ROWS);
    let n = |k: usize| k as f32;
    let width = n(EFFECT_GRID_COLUMNS) * tile.x + n(EFFECT_GRID_COLUMNS - 1) * EFFECT_TILE_GAP;
    let height = if rows == 0 { 0.0 } else { n(rows) * tile.y + n(rows - 1) * EFFECT_TILE_GAP };
    vec2(width, height)
}

/// What an [`effect_grid`] did this pass.
#[derive(Debug, Clone)]
pub struct EffectGridOut {
    /// The index of the tile clicked, if one was.
    pub clicked: Option<usize>,
    /// The grid's rect.
    pub rect: Rect,
    /// Each tile's rect, in the tiles' order.
    pub tiles: Vec<Rect>,
}

/// A tab's tiles in reading order, [`EFFECT_GRID_COLUMNS`] to a row and [`EFFECT_TILE_GAP`]
/// apart, the grid centred across the width available ([`effect_grid_size`]). Tiles past the
/// [`EFFECT_GRID_ROWS`]th row are not shown (a tab has at most twelve).
pub fn effect_grid(ui: &mut Ui, t: &Tokens, tiles: &[EffectTile<'_>]) -> EffectGridOut {
    let shown = tiles.len().min(EFFECT_GRID_COLUMNS * EFFECT_GRID_ROWS);
    let size = effect_grid_size(t, shown);
    let room = ui.available_width();
    let width = if room.is_finite() { room.max(size.x) } else { size.x };
    let (_, area) = ui.allocate_space(vec2(width, size.y));
    let rect = Rect::from_min_size(pos2((area.center().x - size.x / 2.0).round(), area.min.y), size);
    let tile_size = t.metrics.effect_tile;
    let mut out = EffectGridOut { clicked: None, rect, tiles: Vec::with_capacity(shown) };
    for (i, tile) in tiles.iter().take(shown).enumerate() {
        let (col, row) = ((i % EFFECT_GRID_COLUMNS) as f32, (i / EFFECT_GRID_COLUMNS) as f32);
        let at = rect.min + vec2(col * (tile_size.x + EFFECT_TILE_GAP), row * (tile_size.y + EFFECT_TILE_GAP));
        let cell = Rect::from_min_size(at, tile_size);
        let r = ui.scope_builder(egui::UiBuilder::new().max_rect(cell), |ui| effect_tile(ui, t, tile)).inner;
        if r.clicked() {
            out.clicked = Some(i);
        }
        out.tiles.push(r.rect);
    }
    out
}
