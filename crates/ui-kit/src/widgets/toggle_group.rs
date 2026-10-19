//! A row of image toggle buttons, one chosen (`design-system.md` §6, `creating.md` §3: the
//! collage's picture borders and the page's orientation): each toggle a square-ish button with a
//! glyph and a tooltip; a click chooses it, ←/→ (and ↑/↓) with the focus on the row move the choice.

use egui::{CornerRadius, Id, Key, Modifiers, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetType, pos2, vec2};

use super::{Target, focus_ring, register};
use crate::Tokens;
use crate::icons::{Glyph, paint_glyph};
use crate::registry::Role;

/// The gap between two toggles of a group, in points.
pub const TOGGLE_GROUP_GAP: f32 = 4.0;
/// The share of a toggle's shorter side its glyph takes.
pub const TOGGLE_GLYPH_SHARE: f32 = 0.7;

/// One toggle of a [`toggle_group`]: its automation id (`toggle:collage.border.white`), its glyph
/// and its tooltip (also its accessible name).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageToggle<'a> {
    pub id: &'a str,
    pub glyph: Glyph,
    pub tooltip: &'a str,
}

/// What a [`toggle_group`] did this pass.
#[derive(Debug, Clone)]
pub struct ToggleGroupOut {
    /// The toggle chosen this pass (a click or an arrow key), when it changed.
    pub chosen: Option<usize>,
    /// Each toggle's response, in order.
    pub responses: Vec<Response>,
}

/// The choice an arrow key moves `chosen` to among `n` toggles: one step, kept at the ends.
/// `None` (nothing chosen) steps onto the first toggle.
pub fn toggle_step(chosen: Option<usize>, n: usize, forward: bool) -> Option<usize> {
    let last = n.checked_sub(1)?;
    Some(match chosen.map(|c| c.min(last)) {
        None => 0,
        Some(c) if forward => (c + 1).min(last),
        Some(c) => c.saturating_sub(1),
    })
}

/// A row of image toggles, each `size`, [`TOGGLE_GROUP_GAP`] apart, of which `chosen` is the one
/// chosen (out of range: none). Each toggle: radius 4; the chosen one `tool_active` with a 1 pt
/// `accent` border and a `pick` glyph, the others `button` with a 1 pt `button_border`, `hover`
/// hovered and `pressed` held, an `icon` glyph (`text` hovered); disabled `text_disabled`. The
/// glyph is [`TOGGLE_GLYPH_SHARE`] of the shorter side. A click chooses a toggle (a click on the
/// chosen one keeps it: one is always chosen); with the focus on any toggle, ←/↑ and →/↓ move the
/// choice one step (kept at the ends) and the focus with it. Each toggle registers its id (role
/// `toggle`, active when chosen) and shows its tooltip.
pub fn toggle_group(ui: &mut Ui, t: &Tokens, toggles: &[ImageToggle<'_>], chosen: usize, size: Vec2) -> ToggleGroupOut {
    let enabled = ui.is_enabled();
    let n = toggles.len();
    let size = vec2(if size.x.is_finite() { size.x.max(8.0) } else { 32.0 }, if size.y.is_finite() { size.y.max(8.0) } else { 28.0 });
    let total = vec2(n as f32 * size.x + n.saturating_sub(1) as f32 * TOGGLE_GROUP_GAP, size.y);
    let (_, outer) = ui.allocate_space(total);
    let current = (chosen < n).then_some(chosen);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let responses: Vec<Response> = toggles
        .iter()
        .enumerate()
        .map(|(i, tg)| {
            let rect = Rect::from_min_size(pos2(outer.min.x + i as f32 * (size.x + TOGGLE_GROUP_GAP), outer.min.y), size);
            ui.interact(rect, Id::new(tg.id), sense)
        })
        .collect();

    let mut picked = None;
    if enabled {
        if let Some(i) = responses.iter().position(Response::clicked) {
            picked = Some(i);
        }
        if responses.iter().any(Response::has_focus) {
            let (back, forth) = ui.input_mut(|i| {
                let back = i.consume_key(Modifiers::NONE, Key::ArrowLeft) | i.consume_key(Modifiers::NONE, Key::ArrowUp);
                let forth = i.consume_key(Modifiers::NONE, Key::ArrowRight) | i.consume_key(Modifiers::NONE, Key::ArrowDown);
                (back, forth)
            });
            let from = picked.or(current);
            if forth != back
                && let Some(to) = toggle_step(from, n, forth)
            {
                picked = Some(to);
                if let Some(r) = responses.get(to) {
                    r.request_focus();
                }
            }
        }
    }
    let shown = picked.or(current);

    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    for (i, (tg, r)) in toggles.iter().zip(&responses).enumerate() {
        let on = shown == Some(i);
        let (fill, border) = if on {
            (t.tool_active, if enabled { t.accent } else { t.text_disabled })
        } else if enabled && r.is_pointer_button_down_on() {
            (t.pressed, t.button_border)
        } else if enabled && r.hovered() {
            (t.hover, t.button_border)
        } else {
            (t.button, t.button_border)
        };
        p.rect(r.rect, radius, fill, Stroke::new(1.0, border), StrokeKind::Inside);
        let glyph = if !enabled {
            t.text_disabled
        } else if on {
            t.pick
        } else if r.hovered() {
            t.text
        } else {
            t.icon
        };
        let side = (size.x.min(size.y) * TOGGLE_GLYPH_SHARE).round();
        paint_glyph(p, Rect::from_center_size(r.rect.center(), Vec2::splat(side)), tg.glyph, glyph);
        focus_ring(p, t, r, r.rect, t.radius);
        register(ui.ctx(), tg.id, Role::Toggle, tg.tooltip, Target::Widget(r), on);
        r.widget_info(|| egui::WidgetInfo::selected(WidgetType::RadioButton, enabled, on, tg.tooltip));
    }
    let responses = responses.into_iter().zip(toggles).map(|(r, tg)| if tg.tooltip.is_empty() { r } else { r.on_hover_text(tg.tooltip) }).collect();
    ToggleGroupOut { chosen: picked.filter(|&i| Some(i) != current), responses }
}
