//! The control bar of the slideshow and the photo viewer (`viewing.md` §1.3 and §3.1,
//! `design-system.md` §6): a rounded translucent dark panel over the photo, its buttons light on
//! dark, fading in and out over [`OVERLAY_FADE_SECS`].
//!
//! The bar keeps no clock of its own: [`OverlayFade`] turns "shown" and "hidden" into an opacity
//! on the clock the caller gives it (the slideshow's `Clock`, a test's virtual one), so a show and
//! its tests agree to the frame. When the bar should show is the caller's decision (`viewing.md`
//! §1.2: 2 s at the start, 2.5 s after a mouse move).

use egui::{CornerRadius, Id, InnerResponse, Rect, Response, Sense, Ui, UiBuilder, Vec2, WidgetType, pos2, vec2};

use super::{Target, focus_ring, register};
use crate::Tokens;
use crate::fonts;
use crate::icons::{self, Glyph};
use crate::registry::Role;

/// How long the bar takes to fade in or out (seconds).
pub const OVERLAY_FADE_SECS: f64 = 0.2;
/// The space inside the bar round its controls (points).
pub const OVERLAY_PAD: Vec2 = vec2(12.0, 8.0);
/// The height of an overlay button, and the width of one without a label (points).
pub const OVERLAY_BUTTON: f32 = 32.0;
/// The glyph of an overlay button (points).
pub const OVERLAY_GLYPH: f32 = 18.0;
/// The gap between the bar's bottom and the bottom of its area (points).
pub const OVERLAY_BOTTOM: f32 = 24.0;

/// The bar's opacity over time: shown or hidden, and since when. A change part-way through a
/// fade starts from the opacity reached, so the bar never jumps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayFade {
    visible: bool,
    changed_at: f64,
    from: f32,
}

impl OverlayFade {
    /// A bar already fully shown (`true`) or hidden (`false`), with no fade running.
    pub fn new(visible: bool) -> OverlayFade {
        OverlayFade { visible, changed_at: f64::NEG_INFINITY, from: if visible { 1.0 } else { 0.0 } }
    }

    /// Whether the bar is shown (or fading in).
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Shows or hides the bar at `now` (seconds on the caller's clock). Setting the state it is
    /// already in changes nothing, so the fade is not restarted.
    pub fn set(&mut self, visible: bool, now: f64) {
        if visible == self.visible {
            return;
        }
        let reached = self.opacity(now);
        *self = OverlayFade { visible, changed_at: now, from: reached };
    }

    /// The opacity at `now`, 0 … 1: from where the last change began, towards its end at a steady
    /// rate (a whole fade takes [`OVERLAY_FADE_SECS`]; one reversed half-way takes half of it). A
    /// clock that is not a number, or that runs backwards, gives the state's end value.
    pub fn opacity(&self, now: f64) -> f32 {
        let target = if self.visible { 1.0 } else { 0.0 };
        let dt = now - self.changed_at;
        if !dt.is_finite() || dt < 0.0 {
            return target;
        }
        let step = (dt / OVERLAY_FADE_SECS).clamp(0.0, 1.0) as f32;
        let from = if self.from.is_finite() { self.from.clamp(0.0, 1.0) } else { target };
        if self.visible { (from + step).min(1.0) } else { (from - step).max(0.0) }
    }

    /// Whether a fade is still running at `now` (the caller asks for another frame).
    pub fn fading(&self, now: f64) -> bool {
        let o = self.opacity(now);
        o > 0.0 && o < 1.0
    }
}

/// The bar's rect: `size`, centred along the bottom of `area` [`OVERLAY_BOTTOM`] above it, never
/// wider or taller than `area`.
pub fn overlay_bar_rect(area: Rect, size: Vec2) -> Rect {
    if !(area.is_finite() && area.is_positive()) {
        return Rect::NOTHING;
    }
    let clean = |v: f32, max: f32| if v.is_finite() { v.clamp(0.0, max) } else { 0.0 };
    let size = vec2(clean(size.x, area.width()), clean(size.y, area.height()));
    let bottom = (area.max.y - OVERLAY_BOTTOM).max(area.min.y + size.y);
    Rect::from_min_size(pos2(area.center().x - size.x / 2.0, bottom - size.y), size)
}

/// The control bar at `rect` (`viewing.md` §1.3): an `overlay_bar` (`black α170`) panel with radius
/// `overlay_radius` (10), its controls laid out left to right in it, [`OVERLAY_PAD`] in, centred
/// vertically. Everything (panel and controls) is drawn at `opacity`; at 0 nothing is drawn,
/// registered or clickable, and `None` comes back. The panel itself takes the clicks that land on
/// it between its controls, so a click on the bar never reaches the photo. Registered as `id`
/// (role `other`, "Controls").
pub fn overlay_bar<R>(ui: &mut Ui, t: &Tokens, id: &str, rect: Rect, opacity: f32, add: impl FnOnce(&mut Ui) -> R) -> Option<InnerResponse<R>> {
    let opacity = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 0.0 };
    if opacity <= 0.0 || !(rect.is_finite() && rect.is_positive()) {
        return None;
    }
    let panel = ui.interact(rect, Id::new(id), Sense::click());
    ui.painter().rect_filled(rect, CornerRadius::same(t.overlay_radius), t.overlay_bar.gamma_multiply(opacity));
    register(ui.ctx(), id, Role::Other, "Controls", Target::Widget(&panel), false);
    let inner =
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink2(OVERLAY_PAD)).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.multiply_opacity(opacity);
            ui.spacing_mut().item_spacing.x = 6.0;
            add(ui)
        });
    Some(InnerResponse::new(inner.inner, panel))
}

/// A button on an overlay (`viewing.md` §1.3, §3.1): [`OVERLAY_BUTTON`] high, square without a
/// label, else its glyph ([`OVERLAY_GLYPH`]) 10 pt in and its label (13 pt `overlay_text`) 6 pt
/// after it; radius 4; no fill at rest, `overlay_hover` hovered, `overlay_pressed` held or
/// `active`; the glyph `overlay_icon` at rest, `overlay_text` hovered or active,
/// `overlay_disabled` (and the label too) disabled. Registered as `id` (role `iconButton`,
/// labelled `tooltip`, active when `active`); shows `tooltip`.
#[allow(clippy::too_many_arguments)] // `icon_button`'s signature with a label
pub fn overlay_button(ui: &mut Ui, t: &Tokens, id: &str, glyph: Glyph, label: Option<&str>, active: bool, enabled: bool, tooltip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let text_color = if enabled { t.overlay_text } else { t.overlay_disabled };
    let galley = label.filter(|l| !l.is_empty()).map(|l| ui.painter().layout_no_wrap(l.to_owned(), fonts::regular(t.body), text_color));
    let width = galley.as_ref().map_or(OVERLAY_BUTTON, |g| (10.0 + OVERLAY_GLYPH + 6.0 + g.size().x + 12.0).ceil());
    let (_, rect) = ui.allocate_space(vec2(width, OVERLAY_BUTTON));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && (active || r.is_pointer_button_down_on()) {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_hover);
    }
    focus_ring(p, t, &r, rect, t.radius);
    let glyph_color = if !enabled {
        t.overlay_disabled
    } else if active || r.hovered() {
        t.overlay_text
    } else {
        t.overlay_icon
    };
    let glyph_center = match galley {
        Some(_) => pos2(rect.min.x + 10.0 + OVERLAY_GLYPH / 2.0, rect.center().y),
        None => rect.center(),
    };
    icons::paint_glyph(p, Rect::from_center_size(glyph_center, Vec2::splat(OVERLAY_GLYPH)), glyph, glyph_color);
    if let Some(g) = galley {
        let pos = pos2(rect.min.x + 10.0 + OVERLAY_GLYPH + 6.0, rect.center().y - g.size().y / 2.0);
        p.galley(pos.round(), g, text_color);
    }
    register(ui.ctx(), id, Role::IconButton, tooltip, Target::Widget(&r), active);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, tooltip));
    super::tooltip(r, t, tooltip, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_fade_is_at_its_end_value_and_a_change_restarts_from_where_it_was() {
        let shown = OverlayFade::new(true);
        assert_eq!((shown.opacity(0.0), shown.opacity(-5.0), shown.opacity(f64::NAN)), (1.0, 1.0, 1.0));
        let mut f = OverlayFade::new(false);
        assert_eq!(f.opacity(10.0), 0.0);
        f.set(true, 10.0);
        assert!((f.opacity(10.1) - 0.5).abs() < 1e-4);
        // Hidden half-way in: it fades out from 0.5, reaching 0 in half the time.
        f.set(false, 10.1);
        assert!((f.opacity(10.1) - 0.5).abs() < 1e-4);
        assert!((f.opacity(10.15) - 0.25).abs() < 1e-4);
        assert_eq!(f.opacity(10.4), 0.0);
        // A clock that runs backwards gives the end value, never a value outside 0 … 1.
        assert_eq!(f.opacity(9.0), 0.0);
    }

    #[test]
    fn the_bar_sits_centred_above_the_bottom_and_inside_its_area() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0));
        let r = overlay_bar_rect(area, vec2(800.0, 56.0));
        assert_eq!(r, Rect::from_min_size(pos2(300.0, 900.0 - 24.0 - 56.0), vec2(800.0, 56.0)));
        let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 40.0));
        let r = overlay_bar_rect(small, vec2(f32::INFINITY, 56.0));
        assert!(small.contains_rect(r), "{r:?}");
        assert_eq!(overlay_bar_rect(Rect::NOTHING, vec2(10.0, 10.0)), Rect::NOTHING);
    }
}
