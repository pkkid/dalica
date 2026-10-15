//! Tab strips (`design-system.md` §6). The edit panel's ([`tabs`], `editing.md` §2, §5): icon-only
//! tabs 40 × 32 with tooltips; the active one filled `chrome` and joined to the panel below it, the
//! others on the strip's `canvas`. A dialog's ([`text_tabs`], the Options dialog, §11.3): text tabs
//! along the top of a page, the open one joined to the page ([`tab_page`]). When a strip is too
//! narrow the tabs shrink (a text tab's label cut short from its end with "…"), then the ones that
//! still do not fit move to an overflow menu, and the active tab always stays on the strip.
//!
//! [`fit`] (the widths) and [`Tab`] (one accessible tab at a rectangle, painted by the caller) are
//! ported from craft-ui `crates/ui/src/tabs/mod.rs` at revision 5c874d51 (github.com/storytold/
//! craft-libs; MIT OR Apache-2.0; `NOTICE`), unchanged but for Dalica's lints. [`tabs`] and
//! [`text_tabs`] are Dalica's strips built on them.

use egui::{CornerRadius, Id, Painter, Popup, PopupCloseBehavior, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use super::{Target, one_line, register, tooltip};
use crate::Tokens;
use crate::fonts;
use crate::icons::{self, Icon};
use crate::registry::Role;

/// Tabs visible on a strip and tabs that need an overflow menu, all in original order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabFit {
    /// `(input index, width)` for visible tabs. A nonempty input always retains the selected tab
    /// (or the final tab if `selected` was out of bounds), even when its width must become zero.
    pub shown: Vec<(usize, f32)>,
    pub overflow: Vec<usize>,
    /// Reserved width of the overflow button, zero when no tabs overflow. This can be smaller
    /// than the requested width in a tiny strip; use it when laying out the button.
    pub overflow_width: f32,
}

/// Largest meaningful extent for one UI component, in logical points.
const MAX_EXTENT: f32 = 1_000_000.0;

fn extent(value: f32) -> f32 {
    if value.is_finite() { value.clamp(0.0, MAX_EXTENT) } else { 0.0 }
}

/// Fit tab widths into `available` points, shrinking the widest tabs first to a common cap.
/// Each tab normally retains at least `min(natural_width, minimum_width)`. If that cannot fit,
/// retain the selected tab first, then as many others as fit, and reserve an overflow button.
/// A selected tab in a very narrow strip takes the space left after that button.
///
/// Negative and non-finite dimensions become zero; finite dimensions are capped at one million
/// points. Empty input and an out-of-range selection are valid. The widths of all shown tabs plus
/// `overflow_width` never exceed the sanitized available width. Work and storage are linear in
/// the input count (with at most 32 fitting passes); callers keep their own tab-count policy.
pub fn fit(natural: &[f32], selected: usize, available: f32, minimum_width: f32, overflow_width: f32) -> TabFit {
    let available = extent(available);
    let minimum_width = extent(minimum_width);
    let natural: Vec<f32> = natural.iter().map(|&w| extent(w)).collect();
    let all: Vec<usize> = (0..natural.len()).collect();
    if natural.iter().map(|&w| f64::from(w)).sum::<f64>() <= f64::from(available) {
        return TabFit { shown: all.into_iter().zip(natural).collect(), ..Default::default() };
    }
    // One tab has nowhere to overflow; all available space belongs to it.
    if natural.len() == 1 {
        return TabFit { shown: vec![(0, available)], ..Default::default() };
    }
    let floor = |w: f32| w.min(minimum_width);
    if natural.iter().map(|&w| f64::from(floor(w))).sum::<f64>() <= f64::from(available) {
        return TabFit { shown: squeeze(&natural, &all, available, minimum_width), ..Default::default() };
    }
    let overflow_width = extent(overflow_width).min(available);
    let budget = available - overflow_width;
    let selected = selected.min(natural.len().saturating_sub(1));
    let mut keep = vec![false; natural.len()];
    let mut used = 0.0f64;
    for i in std::iter::once(selected).chain(all.iter().copied().filter(|&i| i != selected)) {
        let Some(&width) = natural.get(i) else { continue };
        let width = f64::from(floor(width));
        if i == selected || used + width <= f64::from(budget) {
            if let Some(keep) = keep.get_mut(i) {
                *keep = true;
            }
            used += width;
        }
    }
    let (shown, overflow): (Vec<usize>, Vec<usize>) = all.into_iter().partition(|&i| keep.get(i).copied().unwrap_or(false));
    let shown = squeeze(&natural, &shown, budget, minimum_width);
    TabFit { shown, overflow, overflow_width }
}

fn squeeze(natural: &[f32], indices: &[usize], total: f32, minimum: f32) -> Vec<(usize, f32)> {
    let width = |i: usize| natural.get(i).copied().unwrap_or(0.0);
    let sum_at = |cap: f32| indices.iter().map(|&i| f64::from(width(i).min(cap.max(width(i).min(minimum))))).sum::<f64>();
    let (mut lo, mut hi) = (minimum, indices.iter().map(|&i| width(i)).fold(minimum, f32::max));
    if sum_at(hi) > f64::from(total) {
        for _ in 0..32 {
            let mid = lo + (hi - lo) * 0.5;
            if sum_at(mid) <= f64::from(total) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    } else {
        lo = hi;
    }
    let mut remaining = total;
    indices
        .iter()
        .map(|&i| {
            let natural = width(i);
            // Also handles a selected tab whose minimum alone exceeds the budget, and prevents
            // accumulated floating-point rounding from placing the overflow button past the end.
            let width = natural.min(lo.max(natural.min(minimum))).min(remaining);
            remaining = (remaining - width).max(0.0);
            (i, width)
        })
        .collect()
}

/// An accessible tab at a caller-supplied rectangle, with caller-supplied painting.
///
/// IDs must be unique in their UI scope and independent of translated labels. Selection remains
/// caller-owned. `Sense::click()` is the default; use [`Self::sense`] for draggable dock tabs.
pub struct Tab<'a> {
    id: Id,
    label: &'a str,
    selected: bool,
    sense: Sense,
    focus_stroke: Option<Stroke>,
}

impl<'a> Tab<'a> {
    pub fn new(id: Id, label: &'a str, selected: bool) -> Self {
        Self { id, label, selected, sense: Sense::click(), focus_stroke: None }
    }

    pub fn sense(mut self, sense: Sense) -> Self {
        self.sense = sense;
        self
    }

    /// Set the inset keyboard-focus outline using the application's theme. The default uses
    /// egui's selection stroke. An explicit `Stroke::NONE` disables the outline.
    pub fn focus_stroke(mut self, stroke: Stroke) -> Self {
        self.focus_stroke = Some(stroke);
        self
    }

    /// Register a tab, call its painter and return the unmodified click/drag response.
    ///
    /// This does not allocate space or alter the UI cursor. The painter receives the response
    /// for hover/press styling. Non-finite, empty, inverted or excessive rectangles yield an inert
    /// zero-size response and do not invoke the painter. Parent disabled state applies normally.
    pub fn show_at(self, ui: &mut Ui, rect: Rect, paint: impl FnOnce(&Ui, &Response)) -> Response {
        let size = rect.size();
        let valid = rect.is_finite() && size.x > 0.0 && size.y > 0.0 && size.x <= MAX_EXTENT && size.y <= MAX_EXTENT;
        let safe_rect = if valid { rect } else { Rect::from_min_size(egui::Pos2::ZERO, Vec2::ZERO) };
        let response = ui.interact(safe_rect, self.id, if valid { self.sense } else { Sense::hover() });
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, valid && ui.is_enabled(), self.selected, self.label));
        ui.ctx().accesskit_node_builder(self.id, |node| {
            node.set_role(egui::accesskit::Role::Tab);
            node.clear_toggled();
            node.set_selected(self.selected);
        });
        if !valid {
            return response;
        }
        paint(ui, &response);
        if response.has_focus() && rect.width() > 4.0 && rect.height() > 4.0 {
            let focus = rect.shrink(2.0);
            let stroke = self.focus_stroke.unwrap_or(ui.visuals().selection.stroke);
            if stroke.width.is_finite() && stroke.width > 0.0 {
                let stroke = Stroke::new(stroke.width.min(focus.width().min(focus.height()) * 0.5), stroke.color);
                ui.painter().rect_stroke(focus, 0.0, stroke, StrokeKind::Inside);
            }
        }
        response
    }
}

/// One tab of a [`tabs`] strip: its name in the automation id (`<strip id>/<name>`), its glyph and
/// its tooltip (also its accessible name).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabItem<'a> {
    pub name: &'a str,
    pub icon: Icon,
    pub tooltip: &'a str,
}

/// What a [`tabs`] strip did this pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabsOut {
    /// The tab clicked (on the strip or in the overflow menu), as an index into the items.
    pub clicked: Option<usize>,
    /// How the tabs were laid out.
    pub fit: TabFit,
}

/// The narrowest a tab shrinks before tabs move to the overflow menu (points).
pub const TAB_MIN_WIDTH: f32 = 24.0;
/// The width of the overflow (more tabs) button.
pub const TAB_OVERFLOW_WIDTH: f32 = 18.0;

/// The automation id of the overflow button of strip `id`.
pub fn tabs_overflow_id(id: &str) -> String {
    format!("{id}/more")
}

/// The edit panel's tab strip: the full width of `ui` and the edit tab's height (32), `canvas`
/// behind the tabs. Each tab is `metrics.edit_tab` (40 × 32) with its glyph (18 pt) centred: the
/// active one filled `chrome` with its top corners rounded and no bottom edge, so it joins the
/// panel below; the others `hover × 0.7` hovered and `pressed` held. Glyphs are `icon` at rest,
/// `text` hovered, `pick` active and `text_disabled` disabled. Every tab shows its tooltip.
///
/// Narrower than the tabs, they shrink (down to [`TAB_MIN_WIDTH`]) and then the ones that do not
/// fit move to a menu behind a chevron button at the end (`<id>/more`); the active tab always stays
/// on the strip. Tabs register as `<id>/<name>` (role `tab`, active when shown), in the menu too.
pub fn tabs(ui: &mut Ui, t: &Tokens, id: &str, items: &[TabItem<'_>], active: usize) -> TabsOut {
    let m = &t.metrics;
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
    let (_, strip) = ui.allocate_space(vec2(width, m.edit_tab.y));
    ui.painter().rect_filled(strip, 0, t.canvas);
    let natural = vec![m.edit_tab.x; items.len()];
    let fit = fit(&natural, active, strip.width(), TAB_MIN_WIDTH, TAB_OVERFLOW_WIDTH);
    let mut out = TabsOut::default();
    let mut x = strip.min.x;
    let radius = t.radius;
    for &(i, w) in &fit.shown {
        let Some(item) = items.get(i) else { continue };
        let rect = Rect::from_min_size(pos2(x, strip.min.y), vec2(w, strip.height()));
        x += w;
        let on = i == active;
        let tab_id = format!("{id}/{}", item.name);
        let r = Tab::new(Id::new(&tab_id), item.tooltip, on).focus_stroke(Stroke::new(t.focus_width, t.accent)).show_at(ui, rect, |ui, r| {
            let p = ui.painter();
            let top = CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 };
            if on {
                p.rect_filled(rect, top, t.chrome);
            } else if enabled && r.is_pointer_button_down_on() {
                p.rect_filled(rect.shrink2(vec2(2.0, 3.0)), CornerRadius::same(radius), t.pressed);
            } else if enabled && r.hovered() {
                p.rect_filled(rect.shrink2(vec2(2.0, 3.0)), CornerRadius::same(radius), t.hover.gamma_multiply(0.7));
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
            let side = m.icon_toolbar.min(rect.width() - 4.0).max(0.0);
            if side > 0.0 {
                icons::paint(p, Rect::from_center_size(rect.center(), Vec2::splat(side)), item.icon, glyph);
            }
        });
        register(ui.ctx(), &tab_id, Role::Tab, item.tooltip, Target::Widget(&r), on);
        let r = tooltip(r, t, item.tooltip, None);
        if r.clicked() {
            out.clicked = Some(i);
        }
    }
    if !fit.overflow.is_empty() && fit.overflow_width > 0.0 {
        let rect = Rect::from_min_size(pos2(strip.max.x - fit.overflow_width, strip.min.y), vec2(fit.overflow_width, strip.height()));
        overflow_button(ui, t, id, rect, |ui| {
            for &i in &fit.overflow {
                let Some(item) = items.get(i) else { continue };
                if overflow_row(ui, t, &format!("{id}/{}", item.name), Some(item.icon), item.tooltip) {
                    out.clicked = Some(i);
                }
            }
        });
    }
    out.fit = fit;
    out
}

/// The overflow (more tabs) button of strip `id` at `rect` (`<id>/more`): a chevron, `hover × 0.7`
/// hovered, `pressed` held or open; a click opens a menu of `rows` under it.
fn overflow_button(ui: &mut Ui, t: &Tokens, id: &str, rect: Rect, rows: impl FnOnce(&mut Ui)) {
    let enabled = ui.is_enabled();
    let radius = t.radius;
    let more_id = tabs_overflow_id(id);
    let more = ui.interact(rect, Id::new(&more_id), if enabled { Sense::click() } else { Sense::hover() });
    let popup_id = Id::new(&more_id).with("popup");
    let open = Popup::is_id_open(ui.ctx(), popup_id);
    let p = ui.painter();
    if enabled && (open || more.is_pointer_button_down_on()) {
        p.rect_filled(rect.shrink2(vec2(1.0, 3.0)), CornerRadius::same(radius), t.pressed);
    } else if enabled && more.hovered() {
        p.rect_filled(rect.shrink2(vec2(1.0, 3.0)), CornerRadius::same(radius), t.hover.gamma_multiply(0.7));
    }
    let glyph = if !enabled {
        t.text_disabled
    } else if open || more.hovered() {
        t.text
    } else {
        t.icon
    };
    icons::paint(p, Rect::from_center_size(rect.center(), Vec2::splat(t.metrics.icon_badge.min(rect.width()))), Icon::ChevronDown, glyph);
    super::focus_ring(p, t, &more, rect, radius);
    register(ui.ctx(), &more_id, Role::Button, "More tabs", Target::Widget(&more), open);
    more.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, "More tabs"));
    let more = tooltip(more, t, "More tabs", None);
    if enabled {
        Popup::menu(&more).id(popup_id).close_behavior(PopupCloseBehavior::CloseOnClick).show(rows);
    }
}

/// One hidden tab in the overflow menu: its glyph (an icon tab's) and its label, 24 high.
fn overflow_row(ui: &mut Ui, t: &Tokens, id: &str, icon: Option<Icon>, label: &str) -> bool {
    let font = fonts::regular(t.body);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, t.text_label);
    let text_x = if icon.is_some() { 30.0 } else { 12.0 };
    let size = vec2((galley.size().x + text_x + 10.0).max(120.0), t.metrics.tree_row_h.max(galley.size().y + 6.0));
    let (_, rect) = ui.allocate_space(size);
    let r = ui.interact(rect, Id::new(id), Sense::click());
    let p = ui.painter();
    let hot = r.hovered();
    if hot {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover);
    }
    let color = if hot { t.text } else { t.text_label };
    if let Some(icon) = icon {
        let glyph = Rect::from_center_size(pos2(rect.min.x + 14.0, rect.center().y), Vec2::splat(t.metrics.icon_menu));
        icons::paint(p, glyph, icon, if hot { t.text } else { t.icon });
    }
    p.galley(pos2(rect.min.x + text_x, rect.center().y - galley.size().y / 2.0).round(), galley, color);
    register(ui.ctx(), id, Role::Tab, label, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    r.clicked()
}

/// One tab of a [`text_tabs`] strip: its name in the automation id (`<strip id>/<name>`) and its
/// label (also its accessible name).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextTab<'a> {
    pub name: &'a str,
    pub label: &'a str,
}

/// What a [`text_tabs`] strip did this pass.
#[derive(Debug, Clone, PartialEq)]
pub struct TextTabsOut {
    /// The tab clicked (on the strip or in the overflow menu), as an index into the items.
    pub clicked: Option<usize>,
    /// How the tabs were laid out.
    pub fit: TabFit,
    /// The strip's rect: the page goes right under it ([`tab_page`]).
    pub rect: Rect,
    /// Each tab on the strip, in order: its index, its rect, its label as painted (the whole label,
    /// or its start with "…" when the tab is narrower than it) and whether it was cut.
    pub shown: Vec<(usize, Rect, String, bool)>,
}

/// The space either side of a text tab's label.
pub const TEXT_TAB_PAD: f32 = 14.0;
/// The narrowest a text tab shrinks before tabs move to the overflow menu.
pub const TEXT_TAB_MIN_WIDTH: f32 = 56.0;

/// The height of a text tab strip: the room tab strip's 28, taller when the font needs it.
pub fn text_tabs_height(t: &Tokens) -> f32 {
    t.metrics.room_tabs_h.max((t.body * 1.25 + 10.0).ceil())
}

/// A dialog's tab strip (`design-system.md` §6, §11.3): the full width of `ui` ×
/// [`text_tabs_height`] on the dialog's `chrome`, its bottom edge the top of the page below (a 1 pt
/// `button_border` line, open under the active tab). Each tab is its label (13 pt) with
/// [`TEXT_TAB_PAD`] either side: the active one filled `inset` (the page's fill) with a 1 pt
/// `button_border` outline round its top and sides, top corners rounded, joined to the page, its
/// label `text`; the others `text_label` with no fill, `hover × 0.7` hovered and `pressed` held
/// (inset 2 × 3, radius 4), their label `text`; disabled labels `text_disabled`.
///
/// Narrower than the tabs, the widest shrink first to a common width ([`fit`]), their labels cut
/// short from their end with "…"; below [`TEXT_TAB_MIN_WIDTH`] each, the tabs that do not fit
/// move to a menu behind a chevron button at the end of the strip (`<id>/more`) and the active tab
/// always stays on the strip. Tabs register as `<id>/<name>` (role `tab`, active when shown,
/// labelled with the whole label), in the menu too.
pub fn text_tabs(ui: &mut Ui, t: &Tokens, id: &str, items: &[TextTab<'_>], active: usize) -> TextTabsOut {
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
    let (_, strip) = ui.allocate_space(vec2(width, text_tabs_height(t)));
    let font = fonts::regular(t.body);
    let natural: Vec<f32> =
        items.iter().map(|i| (ui.painter().layout_no_wrap(i.label.to_owned(), font.clone(), t.text).size().x + 2.0 * TEXT_TAB_PAD).ceil()).collect();
    let fit = fit(&natural, active, strip.width(), TEXT_TAB_MIN_WIDTH, TAB_OVERFLOW_WIDTH);
    let mut out = TextTabsOut { clicked: None, fit: TabFit::default(), rect: strip, shown: Vec::new() };
    let radius = t.radius;
    let top = CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 };
    let mut x = strip.min.x;
    let mut active_rect = None;
    for &(i, w) in &fit.shown {
        let Some(item) = items.get(i) else { continue };
        let rect = Rect::from_min_size(pos2(x, strip.min.y), vec2(w, strip.height()));
        x += w;
        let on = i == active;
        let tab_id = format!("{id}/{}", item.name);
        let mut painted = (String::new(), false);
        let r = Tab::new(Id::new(&tab_id), item.label, on).focus_stroke(Stroke::new(t.focus_width, t.accent)).show_at(ui, rect, |ui, r| {
            let p = ui.painter();
            if on {
                p.rect_filled(rect, top, t.inset);
                tab_outline(p, t, rect);
            } else if enabled && r.is_pointer_button_down_on() {
                p.rect_filled(rect.shrink2(vec2(2.0, 3.0)), CornerRadius::same(radius), t.pressed);
            } else if enabled && r.hovered() {
                p.rect_filled(rect.shrink2(vec2(2.0, 3.0)), CornerRadius::same(radius), t.hover.gamma_multiply(0.7));
            }
            let color = if !enabled {
                t.text_disabled
            } else if on || r.hovered() {
                t.text
            } else {
                t.text_label
            };
            let room = rect.width() - 2.0 * TEXT_TAB_PAD;
            if room >= 1.0 {
                let galley = one_line(p, item.label, font.clone(), color, room);
                painted = (galley.rows.first().map(|r| r.text()).unwrap_or_default(), galley.elided);
                let pos = pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0);
                p.galley(pos.round(), galley, color);
            } else {
                painted = (String::new(), !item.label.is_empty());
            }
        });
        register(ui.ctx(), &tab_id, Role::Tab, item.label, Target::Widget(&r), on);
        if on {
            active_rect = Some(rect);
        }
        out.shown.push((i, rect, painted.0, painted.1));
        if r.clicked() {
            out.clicked = Some(i);
        }
    }
    // The page's top edge along the strip's bottom, open under the active tab.
    let y = strip.max.y - 0.5;
    let p = ui.painter();
    let line = Stroke::new(1.0, t.button_border);
    match active_rect {
        Some(a) => {
            p.line_segment([pos2(strip.min.x, y), pos2(a.min.x + 0.5, y)], line);
            p.line_segment([pos2(a.max.x - 0.5, y), pos2(strip.max.x, y)], line);
        }
        None => {
            p.line_segment([pos2(strip.min.x, y), pos2(strip.max.x, y)], line);
        }
    }
    if !fit.overflow.is_empty() && fit.overflow_width > 0.0 {
        let rect = Rect::from_min_size(pos2(strip.max.x - fit.overflow_width, strip.min.y), vec2(fit.overflow_width, strip.height() - 1.0));
        overflow_button(ui, t, id, rect, |ui| {
            for &i in &fit.overflow {
                let Some(item) = items.get(i) else { continue };
                if overflow_row(ui, t, &format!("{id}/{}", item.name), None, item.label) {
                    out.clicked = Some(i);
                }
            }
        });
    }
    out.fit = fit;
    out
}

/// The active text tab's outline: its top (between the rounded corners) and its sides, down to
/// the page's top edge, 1 pt `button_border`.
fn tab_outline(p: &Painter, t: &Tokens, rect: Rect) {
    let line = Stroke::new(1.0, t.button_border);
    let (l, r) = (rect.min.x + 0.5, rect.max.x - 0.5);
    let (top, bottom) = (rect.min.y + 0.5, rect.max.y);
    let k = f32::from(t.radius);
    p.line_segment([pos2(l, bottom), pos2(l, top + k)], line);
    p.line_segment([pos2(r, bottom), pos2(r, top + k)], line);
    p.line_segment([pos2(l + k, top), pos2(r - k, top)], line);
    corner(p, pos2(l + k, top + k), k, 180.0, line);
    corner(p, pos2(r - k, top + k), k, 270.0, line);
}

/// A rounded corner's quarter arc round `c`, radius `k`, from `a0` degrees (0° = right, 90° =
/// down) on for 90°.
fn corner(p: &Painter, c: egui::Pos2, k: f32, a0: f32, line: Stroke) {
    let pts: Vec<_> = (0..=6)
        .map(|i| {
            let a = (a0 + 90.0 * i as f32 / 6.0).to_radians();
            pos2(c.x + k * a.cos(), c.y + k * a.sin())
        })
        .collect();
    p.add(egui::Shape::line(pts, line));
}

/// The page under a [`text_tabs`] strip, at `rect` (from the strip's bottom down): filled `inset`
/// with a 1 pt `button_border` outline round its sides and bottom, bottom corners rounded; its top
/// edge is the strip's.
pub fn tab_page(painter: &Painter, t: &Tokens, rect: Rect) {
    if !(rect.is_finite() && rect.is_positive()) {
        return;
    }
    let radius = t.radius;
    let bottom = CornerRadius { nw: 0, ne: 0, sw: radius, se: radius };
    painter.rect_filled(rect, bottom, t.inset);
    let line = Stroke::new(1.0, t.button_border);
    let (l, r, b) = (rect.min.x + 0.5, rect.max.x - 0.5, rect.max.y - 0.5);
    let k = f32::from(radius);
    painter.line_segment([pos2(l, rect.min.y), pos2(l, b - k)], line);
    painter.line_segment([pos2(r, rect.min.y), pos2(r, b - k)], line);
    painter.line_segment([pos2(l + k, b), pos2(r - k, b)], line);
    corner(painter, pos2(r - k, b - k), k, 0.0, line);
    corner(painter, pos2(l + k, b - k), k, 90.0, line);
}

#[cfg(test)]
mod tests {
    //! The fitting tests of craft-ui `crates/ui/src/tabs/tests.rs` at 5c874d51.
    use super::*;

    fn total(fit: &TabFit) -> f32 {
        fit.shown.iter().map(|&(_, width)| width).sum::<f32>() + fit.overflow_width
    }

    #[test]
    fn natural_widths_and_common_cap_preserve_tab_order() {
        let f = fit(&[60.0, 70.0, 80.0], 0, 400.0, 40.0, 18.0);
        assert_eq!(f.shown, [(0, 60.0), (1, 70.0), (2, 80.0)]);
        assert!(f.overflow.is_empty());
        assert_eq!(f.overflow_width, 0.0);
        let f = fit(&[50.0, 70.0, 120.0], 0, 200.0, 40.0, 18.0);
        assert!(f.overflow.is_empty());
        assert_eq!(f.shown[0], (0, 50.0));
        assert_eq!(f.shown[1], (1, 70.0));
        assert!((f.shown[2].1 - 80.0).abs() < 0.01);
        assert!((total(&f) - 200.0).abs() < 0.01);
    }

    #[test]
    fn overflow_retains_selection_and_partitions_all_tabs() {
        let natural = [80.0, 90.0, 85.0, 95.0];
        for selected in [0, 1, 2, 3, usize::MAX] {
            for available in [0.0, 1.0, 5.0, 18.0, 19.0, 55.0, 120.0, 200.0, 500.0] {
                let f = fit(&natural, selected, available, 40.0, 18.0);
                let selected = selected.min(natural.len() - 1);
                assert!(f.shown.iter().any(|&(i, _)| i == selected), "{selected} at {available}: {f:?}");
                assert!(!f.overflow.contains(&selected));
                assert!(f.shown.windows(2).all(|w| w[0].0 < w[1].0));
                assert!(f.overflow.windows(2).all(|w| w[0] < w[1]));
                let mut all: Vec<usize> = f.shown.iter().map(|&(i, _)| i).chain(f.overflow.iter().copied()).collect();
                all.sort_unstable();
                assert_eq!(all, [0, 1, 2, 3]);
                assert!(total(&f) <= available + 0.001, "{available}: {f:?}");
            }
        }
    }

    #[test]
    fn one_tab_never_reserves_an_unused_overflow_button() {
        let f = fit(&[80.0], 99, 10.0, 40.0, 18.0);
        assert_eq!(f.shown, [(0, 10.0)]);
        assert!(f.overflow.is_empty());
        assert_eq!(f.overflow_width, 0.0);
    }

    #[test]
    fn malformed_dimensions_remain_finite_and_within_the_strip() {
        for available in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 0.01, 10.0, 20.0, 1e20, f32::MAX] {
            for minimum in [f32::NAN, f32::INFINITY, -5.0, 0.0, 40.0, f32::MAX] {
                for overflow in [f32::NAN, f32::INFINITY, -5.0, 0.0, 18.0, f32::MAX] {
                    for natural in [&[][..], &[f32::NAN, 50.0][..], &[f32::MAX, -5.0, 30.0, f32::INFINITY][..]] {
                        let f = fit(natural, usize::MAX, available, minimum, overflow);
                        assert!(f.shown.iter().all(|&(_, w)| w.is_finite() && w >= 0.0), "{f:?}");
                        assert!(f.overflow_width.is_finite() && f.overflow_width >= 0.0);
                        assert!(total(&f) <= extent(available) + 0.125, "{available} {minimum} {overflow}: {f:?}");
                        assert_eq!(f.shown.len() + f.overflow.len(), natural.len());
                    }
                }
            }
        }
    }

    #[test]
    fn many_tabs_keep_the_last_selected_without_quadratic_membership_scans() {
        let widths = vec![60.0; 10_000];
        let f = fit(&widths, widths.len() - 1, 240.0, 40.0, 18.0);
        assert!(f.shown.iter().any(|&(i, w)| i == widths.len() - 1 && w > 0.0));
        assert_eq!(f.shown.len() + f.overflow.len(), widths.len());
        assert!(total(&f) <= 240.001);
    }
}
