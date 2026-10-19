//! Buttons beyond `text_button` and `icon_button` (`design-system.md` §6): the primary button, the
//! segmented pair, the big action button of the bottom area and the filter toggle.

use egui::{Align2, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetType, pos2, vec2};

use super::{Target, allocate, focus_ring, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::icons::{self, Icon};
use crate::registry::Role;

/// The default action of a dialog or an empty state: as `text_button`, but filled `accent` (lighter
/// hovered, darker pressed) with `on_accent` text and no border. Disabled, it falls back to the
/// plain button look with `text_disabled` text, so a disabled default never looks clickable.
pub fn primary_button(ui: &mut Ui, t: &Tokens, id: &str, label: &str) -> Response {
    let enabled = ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), fonts::semibold(t.button_label), if enabled { t.on_accent } else { t.text_disabled });
    let size = vec2((galley.size().x + 18.0).max(37.0).round(), t.metrics.button_h);
    let r = allocate(ui, id, size, enabled);
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    if enabled {
        let fill = if r.is_pointer_button_down_on() {
            t.accent_pressed
        } else if r.hovered() {
            t.accent_hover
        } else {
            t.accent
        };
        p.rect_filled(r.rect, radius, fill);
    } else {
        p.rect(r.rect, radius, t.button, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    }
    focus_ring(p, t, &r, r.rect, t.radius);
    let pos = r.rect.center() - galley.size() / 2.0;
    p.galley(pos.round(), galley, t.on_accent);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), false);
    r
}

/// What a segment shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegmentLabel<'a> {
    /// A text label, SemiBold like `text_button`.
    Text(&'a str),
    /// An icon glyph, `0.6 ×` the segment's height.
    Icon(Icon),
}

/// One segment of a [`segmented`] control: its automation id (`button:tree.flat`), what it shows and
/// its tooltip (also its accessible name for an icon; empty for none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment<'a> {
    pub id: &'a str,
    pub label: SegmentLabel<'a>,
    pub tooltip: &'a str,
}

/// A row of joined segments, one of which is selected (`active`), like the toolbar's Flat | Tree pair
/// (`ui-layout.md` §3). Each segment is `size` (a text segment widens to fit its label); the row has
/// one `button_border` outline with 1 pt dividers, rounded at its ends only. The selected segment is
/// filled `tool_active` with a `pick` glyph; the others `button`, `hover` hovered, `pressed` held.
/// Returns the index of the segment clicked this pass.
pub fn segmented(ui: &mut Ui, t: &Tokens, segments: &[Segment<'_>], active: Option<usize>, size: Vec2) -> Option<usize> {
    let enabled = ui.is_enabled();
    let font = fonts::semibold(t.button_label);
    let widths: Vec<f32> = segments
        .iter()
        .map(|s| match s.label {
            SegmentLabel::Text(text) => (ui.painter().layout_no_wrap(text.to_owned(), font.clone(), t.text).size().x + 18.0).max(size.x).round(),
            SegmentLabel::Icon(_) => size.x,
        })
        .collect();
    let total = vec2(widths.iter().sum(), size.y);
    let (_, outer) = ui.allocate_space(total);
    let n = segments.len();
    let radius = t.radius;
    let mut clicked = None;
    let mut x = outer.min.x;
    for (i, (seg, w)) in segments.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(pos2(x, outer.min.y), vec2(*w, size.y));
        x += w;
        let r = ui.interact(rect, egui::Id::new(seg.id), if enabled { Sense::click() } else { Sense::hover() });
        let on = active == Some(i);
        let fill = if on {
            t.tool_active
        } else if enabled && r.is_pointer_button_down_on() {
            t.pressed
        } else if enabled && r.hovered() {
            t.hover
        } else {
            t.button
        };
        let corners = CornerRadius {
            nw: if i == 0 { radius } else { 0 },
            sw: if i == 0 { radius } else { 0 },
            ne: if i + 1 == n { radius } else { 0 },
            se: if i + 1 == n { radius } else { 0 },
        };
        let p = ui.painter();
        p.rect_filled(rect, corners, fill);
        let glyph = if !enabled {
            t.text_disabled
        } else if on {
            t.pick
        } else if r.hovered() {
            t.text
        } else {
            t.icon
        };
        match seg.label {
            SegmentLabel::Text(text) => {
                let color = if !enabled { t.text_disabled } else { t.text };
                p.text(rect.center(), Align2::CENTER_CENTER, text, font.clone(), color);
            }
            SegmentLabel::Icon(icon) => {
                icons::paint(p, Rect::from_center_size(rect.center(), Vec2::splat((size.y * 0.6).round())), icon, glyph);
            }
        }
        if i > 0 {
            p.vline(rect.min.x, rect.y_range().shrink(1.0), Stroke::new(1.0, t.button_border));
        }
        let name = match seg.label {
            SegmentLabel::Text(text) if seg.tooltip.is_empty() => text,
            _ => seg.tooltip,
        };
        r.widget_info(|| egui::WidgetInfo::selected(WidgetType::RadioButton, enabled, on, name));
        register(ui.ctx(), seg.id, Role::Toggle, name, Target::Widget(&r), on);
        let r = if seg.tooltip.is_empty() { r } else { r.on_hover_text(seg.tooltip) };
        if r.clicked() {
            clicked = Some(i);
        }
        // Painted last, so a neighbour's fill never covers it.
        focus_ring(ui.painter(), t, &r, rect, radius);
    }
    ui.painter().rect_stroke(outer, CornerRadius::same(radius), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    clicked
}

/// The big bottom-bar button (`design-system.md` §6, `ui-layout.md` §6.5): 64 × 56, the icon (28)
/// centred in the top 34 pt, the label (11.5) below, elided to fit. Rest: no fill, `icon` glyph,
/// `text_label` label; hovered: `hover` fill, `text` glyph and label; pressed or `active` (its menu
/// open): `pressed` fill and a `pick` glyph; disabled: both in `text_disabled`.
#[allow(clippy::too_many_arguments)] // the signature of `design-system.md` §6, plus `ui`, the tokens and the states
pub fn action_button(ui: &mut Ui, t: &Tokens, id: &str, icon: Icon, label: &str, active: bool, enabled: bool) -> Response {
    let m = &t.metrics;
    let enabled = enabled && ui.is_enabled();
    let r = allocate(ui, id, m.action_button, enabled);
    let p = ui.painter();
    let down = enabled && (active || r.is_pointer_button_down_on());
    if down {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(r.rect, CornerRadius::same(t.radius), t.hover);
    }
    let (glyph, text) = if !enabled {
        (t.text_disabled, t.text_disabled)
    } else if down {
        (t.pick, t.text)
    } else if r.hovered() {
        (t.text, t.text)
    } else {
        (t.icon, t.text_label)
    };
    let band = Rect::from_min_size(r.rect.min, vec2(r.rect.width(), m.action_icon_band));
    icons::paint(p, Rect::from_center_size(band.center(), Vec2::splat(m.action_icon)), icon, glyph);
    let galley = one_line(p, label, fonts::regular(t.button_label), text, r.rect.width() - 4.0);
    let below = Rect::from_min_max(pos2(r.rect.min.x, band.max.y), r.rect.max);
    p.galley((below.center() - galley.size() / 2.0).round(), galley, text);
    focus_ring(p, t, &r, r.rect, t.radius);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), active);
    r
}

/// A filter toggle of the toolbar's joined filter row (`ui-layout.md` §3): 26 × 26, a 16 pt glyph;
/// on: `tool_active` fill with radius 5 and a 1 pt `accent` stroke, `pick` glyph; off: no fill,
/// `icon` glyph, `hover × 0.7` and a `text` glyph hovered, `pressed` held.
pub fn toggle_filter(ui: &mut Ui, t: &Tokens, id: &str, icon: Icon, on: bool, tooltip: &str) -> Response {
    let enabled = ui.is_enabled();
    let edge = t.metrics.filter_toggle;
    let r = allocate(ui, id, Vec2::splat(edge), enabled);
    let p = ui.painter();
    let radius = CornerRadius::same(t.filter_radius);
    if on {
        let stroke = Stroke::new(1.0, if enabled { t.accent } else { t.text_disabled });
        p.rect(r.rect, radius, t.tool_active, stroke, StrokeKind::Inside);
    } else if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(r.rect, radius, t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(r.rect, radius, t.hover.gamma_multiply(0.7));
    }
    let glyph = if !enabled {
        t.text_disabled
    } else if on {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    icons::paint(p, Rect::from_center_size(r.rect.center(), Vec2::splat(t.metrics.icon_menu)), icon, glyph);
    focus_ring(p, t, &r, r.rect, t.filter_radius);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Checkbox, enabled, on, tooltip));
    register(ui.ctx(), id, Role::Toggle, tooltip, Target::Widget(&r), on);
    if tooltip.is_empty() { r } else { r.on_hover_text(tooltip) }
}
