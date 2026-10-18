//! Shared widgets (`design-system.md` §6). Panels compose these and never re-implement them. Each
//! one takes an automation id, registers itself through [`register`] and gives AccessKit a role and
//! a label.
//!
//! [`register`] is the one way anything gets an automation id (`control-protocol.md` §7): these
//! widgets call it, and so do the panels for the controls they build themselves (tree rows,
//! thumbnails, menu items).
//!
//! Every widget paints its states from [`Tokens`]: rest, hover, pressed, active (toggled, selected
//! or open), disabled (`ui.is_enabled()` false, or its own `enabled` flag) and focused (the 1.5 pt
//! `accent` ring of [`focus_ring`], `design-system.md` §9). The `tests/widgets.rs` gallery renders
//! each one in each state.

use egui::{Align2, Color32, Context, CornerRadius, Id, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetType, vec2};

use crate::Tokens;
use crate::fonts;
use crate::icons::{self, Icon};
use crate::registry::{self, Role, WidgetInfo};

mod age_slider;
mod autocomplete;
mod badges;
mod banner;
mod buttons;
mod caption_plate;
mod checkbox;
mod crosshair;
mod datetime;
mod dialog;
mod drag_pill;
mod dropdown;
mod effect_tile;
mod feedback;
mod field;
mod form;
mod group_header;
mod header;
mod histogram;
mod key_cell;
mod list_row;
mod marks;
mod overlay_bar;
mod slider;
mod splitter;
mod stepper;
mod swatch;
mod tabs;
mod tool_button;
mod tooltip;

pub use age_slider::{AGE_MAX_DAYS, age_days, age_label, age_position, age_slider, age_snapped};
pub use autocomplete::{AutocompleteOut, MAX_SUGGESTIONS, VISIBLE_SUGGESTIONS, autocomplete, autocomplete_matches};
pub use badges::{
    BADGE_GAP, BADGE_GLYPH, BADGE_INSET, BADGE_PLATE, Badge, BadgeRects, Badges, PHOTO_SHADOW, Video, badge_row, badges, duration_text,
    excluded_mark, failed_mark, photo_shadow, star_badge,
};
pub use banner::{BANNER_H, BANNER_PAD, BANNER_TEXT_GAP, BannerOut, filter_banner};
pub use buttons::{Segment, SegmentLabel, action_button, primary_button, segmented, toggle_filter};
pub use caption_plate::{
    CAPTION_PLATE_LINES, CAPTION_PLATE_PAD, CAPTION_PLATE_RADIUS, CAPTION_PLATE_SIZE, CAPTION_PLATE_WIDTH, CaptionPlateOut, caption_galley,
    caption_plate,
};
pub use checkbox::{CheckboxRowOut, checkbox_row};
pub use crosshair::{CROSSHAIR_ARM, CROSSHAIR_GRAB, CROSSHAIR_HANDLE, Crosshair, CrosshairOut, crosshair, crosshair_handle_id, handle_angle};
pub use datetime::{
    DATE_HINT, DATE_WIDTH, DatePart, MAX_YEAR, MIN_YEAR, PickedDate, PickedTime, PickerOut, TIME_HINT, TIME_WIDTH, TimePart, date_field,
    days_in_month, time_field,
};
pub use dialog::{ButtonKind, DialogButton, DialogResponse, dialog};
pub use drag_pill::{
    DragOp, DragPill, MARKER_CAP, MARKER_W, PILL_H, PILL_MARGIN, PILL_MAX_W, PILL_OFFSET, PILL_THUMB, PillThumb, drag_pill, drag_pill_rect,
    drag_pill_text, drop_target, insertion_marker,
};
pub use dropdown::{DROPDOWN_ITEM_MAX_W, DROPDOWN_ITEM_MIN_W, dropdown, dropdown_item, dropdown_popup_id};
pub use effect_tile::{
    EFFECT_BADGE, EFFECT_GRID_COLUMNS, EFFECT_GRID_ROWS, EFFECT_LABEL_MIN, EFFECT_TILE_GAP, EffectGridOut, EffectTile, effect_grid, effect_grid_size,
    effect_tile, effect_tile_id, effect_tile_label,
};
pub use feedback::{
    ProgressOut, TOAST_ERROR_SECS, TOAST_LINES, TOAST_SECS, ToastKind, ToastText, elide_middle, empty_state, progress_row, toast, toast_galley,
    toast_opacity, toast_text_width,
};
pub use field::{Ending, FieldOut, SearchOut, TextField, focus_field, search_field, text_field};
pub use form::{CHECK_BOX, CHOICE_LABEL_X, FORM_GAP, FORM_LABEL_MAX, FORM_ROW_GAP, Form, RADIO_R, checkbox, form_group, hint, radio, radio_row};
pub use group_header::{GROUP_CLOCK, GroupHeaderOut, group_header, group_select_id};
pub use header::section_header;
pub use histogram::{HISTOGRAM_HEADROOM, MAX_BINS, histogram, histogram_curves, histogram_mesh, paint_histogram};
pub use key_cell::{
    KEY_CAP_GAP, KEY_CAP_PAD, KEY_CELL_MAX_KEYS, KEY_CELL_W, KeyCell, NO_KEY_TEXT, RECORDING_TEXT, key_cell, key_cell_height, key_cell_label,
    key_text,
};
pub use list_row::{LIST_COLUMN_GAP, LIST_ROW_GUTTER, LIST_ROW_H, ListCell, MODIFIED_DOT, list_columns, list_row, list_row_height, list_text};
pub use overlay_bar::{
    OVERLAY_BOTTOM, OVERLAY_BUTTON, OVERLAY_FADE_SECS, OVERLAY_GLYPH, OVERLAY_PAD, OverlayFade, overlay_bar, overlay_bar_rect, overlay_button,
};
pub use slider::{
    STOP_SNAP, STOPS_FIELD_W, STOPS_TRACK_MIN, SliderOut, SliderScale, SliderSpec, StopsSlider, TUNING_NUDGE, edit_slider_value, effect_slider,
    next_stop, nudged, scale_fraction, scale_nudged, scale_value, slider, snap_to_stop, stops_slider, tuning_nudged, tuning_slider, typed_value,
    zoom_slider,
};
pub use splitter::{arrow_delta, split_position, splitter};
pub use stepper::{STEPPER_BUTTON, STEPPER_GAP, StepperOut, stepped, stepper, stepper_ids};
pub use swatch::{
    HUE_STRIP_W, PICKER_FIELD, PICKER_PAD, SWATCH_SIZE, SwatchOut, close_swatch, field_color, field_point, hue_at, hue_color, hue_point, open_swatch,
    swatch, swatch_open, swatch_popup_rect, swatch_popup_size,
};
pub use tabs::{
    ROOM_TAB_CLOSE, ROOM_TAB_MIN_WIDTH, ROOM_TAB_PAD, RoomTab, RoomTabsOut, ShownRoomTab, TAB_MIN_WIDTH, TAB_OVERFLOW_WIDTH, TEXT_TAB_MIN_WIDTH,
    TEXT_TAB_PAD, Tab, TabFit, TabItem, TabsOut, TextTab, TextTabsOut, fit as fit_tabs, room_tab_close_id, room_tabs, tab_page, tabs,
    tabs_overflow_id, text_tabs, text_tabs_height,
};
pub use tool_button::{TOOL_BUTTON_H, tool_button, tool_button_width};
pub use tooltip::{TOOLTIP_WIDTH, tooltip};

/// What an automation id is attached to.
#[derive(Clone, Copy)]
pub enum Target<'a> {
    /// An egui widget: its rect, egui id and enabled state come from the response (enabled = it
    /// takes input: an enabled widget that senses clicks, drags or focus).
    Widget(&'a Response),
    /// A painted part with no input of its own (a badge, a scroll handle), visible within `clip`.
    Painted { rect: Rect, clip: Rect },
}

/// Registers the automation id `id` for this pass (`control-protocol.md` §7): the one helper every
/// widget and panel uses, never [`registry::register`] by hand. The rect is clipped to what is
/// visible; a widget scrolled entirely out of sight is not listed. `active` is the toggled, selected
/// or open state.
pub fn register(ctx: &Context, id: &str, role: Role, label: &str, target: Target<'_>, active: bool) {
    let info = match target {
        Target::Widget(r) => {
            if !r.interact_rect.is_positive() {
                return;
            }
            let enabled = r.enabled() && (r.sense.interactive() || r.sense.is_focusable());
            WidgetInfo::new(id, role, label, r.interact_rect, enabled, active).with_egui_id(r.id)
        }
        Target::Painted { rect, clip } => {
            let rect = rect.intersect(clip);
            if !rect.is_positive() {
                return;
            }
            WidgetInfo::new(id, role, label, rect, true, active)
        }
    };
    registry::register(ctx, info);
}

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

/// The keyboard focus ring (`design-system.md` §9): a `focus_width` `accent` outline 1 pt outside
/// `rect`, so it reads on any fill (the primary button's `accent` included). Painted only while
/// `r` has the keyboard focus.
pub fn focus_ring(painter: &Painter, t: &Tokens, r: &Response, rect: Rect, radius: u8) {
    if r.has_focus() {
        painter.rect_stroke(
            rect.expand(1.0),
            CornerRadius::same(radius.saturating_add(1)),
            Stroke::new(t.focus_width, t.accent),
            StrokeKind::Outside,
        );
    }
}

/// `text` on one line in `font`, cut short with "…" when wider than `max` points.
fn one_line(painter: &Painter, text: &str, font: egui::FontId, color: Color32, max: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(if max.is_finite() { max.max(1.0) } else { 1.0 });
    painter.layout_job(job)
}

/// A button with a text label: SemiBold 11.5 `text`, `(text_w + 18).max(37) × 24`, radius 4, fill
/// `button` / `hover` / `pressed`, 1 pt `button_border`.
pub fn text_button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, active: bool) -> Response {
    let enabled = ui.is_enabled();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), fonts::semibold(t.button_label), if enabled { t.text } else { t.text_disabled });
    let size = vec2((galley.size().x + 18.0).max(37.0).round(), t.metrics.button_h);
    let r = allocate(ui, id, size, enabled);
    let p = ui.painter();
    p.rect(r.rect, CornerRadius::same(t.radius), fill(t, &r, active), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    focus_ring(p, t, &r, r.rect, t.radius);
    let pos = r.rect.center() - galley.size() / 2.0;
    p.galley(pos.round(), galley, t.text);
    register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), active);
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
    focus_ring(p, t, &r, highlight, t.radius);
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
    register(ui.ctx(), id, Role::IconButton, tooltip, Target::Widget(&r), active);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, tooltip));
    r.on_hover_text(tooltip)
}

/// The star toggle (36 × 24, `design-system.md` §4): one star glyph, filled `star` when on,
/// outlined `icon` when off, on a button plate.
pub fn star_toggle(ui: &mut Ui, t: &Tokens, id: &str, on: bool) -> Response {
    let enabled = ui.is_enabled();
    let r = allocate(ui, id, t.metrics.star_button, enabled);
    let p = ui.painter();
    p.rect(r.rect, CornerRadius::same(t.radius), fill(t, &r, false), Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    focus_ring(p, t, &r, r.rect, t.radius);
    let glyph = Rect::from_center_size(r.rect.center(), Vec2::splat(16.0));
    let (icon, color) = match (enabled, on) {
        (false, _) => (if on { Icon::Star } else { Icon::StarOutline }, t.text_disabled),
        (true, true) => (Icon::Star, t.star),
        (true, false) => (Icon::StarOutline, t.icon),
    };
    icons::paint(p, glyph, icon, color);
    register(ui.ctx(), id, Role::Toggle, "Star", Target::Widget(&r), on);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Checkbox, enabled, on, "Star"));
    r.on_hover_text("Star / Unstar")
}

/// A label in `font`, `color`, aligned in `rect` (a convenience for panels).
pub fn label_in(painter: &Painter, rect: Rect, align: Align2, text: &str, font: egui::FontId, color: Color32) -> Rect {
    painter.text(align.pos_in_rect(&rect), align, text, font, color)
}

/// Shows a tooltip on `response` this pass, whatever egui's hover delay: for a widget that times
/// its own (a thumbnail's 600 ms, `ui-layout.md` §5.5). The look of [`tooltip`]: `title` in 13 pt
/// `text`, `detail` below in 11 pt `text_dim`, wrapped at [`TOOLTIP_WIDTH`].
pub fn tooltip_now(response: &Response, t: &Tokens, title: &str, detail: Option<&str>) {
    if title.is_empty() && detail.is_none_or(str::is_empty) {
        return;
    }
    response.show_tooltip_ui(|ui| {
        ui.set_max_width(TOOLTIP_WIDTH);
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(egui::RichText::new(title).font(fonts::regular(t.body)).color(t.text));
        if let Some(detail) = detail.filter(|d| !d.is_empty()) {
            ui.label(egui::RichText::new(detail).font(fonts::regular(t.small)).color(t.text_dim));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(ctx: &Context, ui: impl FnMut(&mut Ui)) {
        ctx.run_ui(crate::raw_input(400, 300, 1.0), ui).textures_delta.clear();
    }

    fn context(t: &Tokens) -> Context {
        let ctx = Context::default();
        t.apply(&ctx);
        ctx
    }

    #[test]
    fn an_interactive_widget_without_an_id_is_listed_until_it_registers() {
        let t = Tokens::default();
        let ctx = context(&t);
        // A plain egui button nobody registered is caught; our own widgets register themselves.
        pass(&ctx, |ui| {
            text_button(ui, &t, "button:ours", "Ours", false);
            let _ = ui.button("Theirs");
        });
        let missing = registry::unregistered(&ctx);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert_eq!(registry::find(&ctx, "button:ours").map(|w| w.enabled), Some(true));
        // Registered through the helper, it is covered.
        pass(&ctx, |ui| {
            text_button(ui, &t, "button:ours", "Ours", false);
            let r = ui.button("Theirs");
            register(ui.ctx(), "button:theirs", Role::Button, "Theirs", Target::Widget(&r), false);
        });
        assert_eq!(registry::unregistered(&ctx), []);
        assert_eq!(registry::widgets(&ctx).len(), 2);
    }

    #[test]
    fn rects_are_clipped_to_what_is_visible_and_hidden_widgets_are_not_listed() {
        let t = Tokens::default();
        let ctx = context(&t);
        pass(&ctx, |ui| {
            // A disabled icon button is listed, as disabled.
            icon_button(ui, &t, "button:off", Icon::Star, 24.0, false, false, "Off");
            ui.scope(|ui| {
                ui.set_clip_rect(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(400.0, 100.0)));
                let half = Rect::from_min_size(egui::pos2(10.0, 90.0), vec2(20.0, 20.0));
                let r = ui.interact(half, Id::new("half"), Sense::click());
                register(ui.ctx(), "thumb:1", Role::Thumbnail, "half", Target::Widget(&r), false);
                let gone = Rect::from_min_size(egui::pos2(10.0, 200.0), vec2(20.0, 20.0));
                let r = ui.interact(gone, Id::new("gone"), Sense::click());
                register(ui.ctx(), "thumb:2", Role::Thumbnail, "gone", Target::Widget(&r), false);
                register(ui.ctx(), "thumb:1/star", Role::Badge, "", Target::Painted { rect: half, clip: ui.clip_rect() }, true);
                register(ui.ctx(), "thumb:2/star", Role::Badge, "", Target::Painted { rect: gone, clip: ui.clip_rect() }, true);
            });
        });
        let ids: Vec<String> = registry::widgets(&ctx).into_iter().map(|w| w.id).collect();
        assert_eq!(ids, ["button:off", "thumb:1", "thumb:1/star"]);
        assert_eq!(registry::find(&ctx, "button:off").map(|w| w.enabled), Some(false));
        assert_eq!(registry::find(&ctx, "thumb:1").map(|w| w.rect), Some([10.0, 90.0, 20.0, 10.0]));
        assert_eq!(registry::find(&ctx, "thumb:1/star").map(|w| (w.rect, w.egui_id)), Some(([10.0, 90.0, 20.0, 10.0], None)));
        assert_eq!(registry::unregistered(&ctx), [], "a widget clipped out of sight is not counted");
    }
}
