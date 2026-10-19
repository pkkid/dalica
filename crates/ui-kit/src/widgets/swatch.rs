//! The colour swatch of the effect panels (`design-system.md` §6, `editing.md` §8.1): a button
//! filled with the colour that opens a picker popup kept inside the window: a saturation / value
//! field, a hue strip, a hex field and the eyedropper button (armed while the next click on the
//! photo picks the colour).
//!
//! The popup's placement is ported from PhotoCraft `crates/ui-egui/src/widgets.rs`
//! (`swatch_popup`, `color_swatch`) at e4b2868a, MIT OR Apache-2.0: at the swatch's left edge, or
//! further left where the window's edge needs it, below the swatch or above it as room allows.
//! Ours has a fixed size, so it is placed every pass from the swatch alone ([`swatch_popup_rect`]).
//! The field and the strip are in [`picker`].

mod picker;

use dalica_core::color::{Hsv, hex_u8, hsv_to_rgb, parse_hex_u8, rgb_from_u8, rgb_to_hsv, rgb_to_u8};
use egui::{
    Context, CornerRadius, Id, Key, Order, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetType, pos2, vec2,
};

pub use picker::{field_color, field_point, hue_at, hue_color, hue_point};

use super::{Target, TextField, access, access_range, allocate, focus_ring, register, tooltip};
use crate::Tokens;
use crate::icons::{EditIcon, paint_edit};
use crate::registry::Role;
use crate::text::{self, Text};

/// The swatch button's size.
pub const SWATCH_SIZE: Vec2 = vec2(36.0, 22.0);
/// The side of the picker's saturation / value field.
pub const PICKER_FIELD: f32 = 150.0;
/// The width of the picker's hue strip.
pub const HUE_STRIP_W: f32 = 16.0;
/// The popup's inner margin, and the space between its parts.
pub const PICKER_PAD: f32 = 10.0;

/// The gap between the field and the strip, and between them and the row under them.
const GAP: f32 = 8.0;
/// The hex field's width.
const HEX_W: f32 = 84.0;
/// The eyedropper button's side.
const DROPPER: f32 = 24.0;
/// The popup's distance from the swatch and from the window's edges.
const EDGE: f32 = 4.0;
/// Cells per side of the field's mesh (and the strip's length).
const CELLS: u32 = 32;

/// What a [`swatch`] did this pass.
#[derive(Debug, Clone)]
pub struct SwatchOut {
    /// The colour picked, when it changed (in the field, the strip or the hex field).
    pub color: Option<[u8; 3]>,
    /// The eyedropper button was clicked: arm it (or disarm it, if armed). The caller samples the
    /// photo on its next click and disarms it.
    pub eyedropper: bool,
    /// The popup is open.
    pub open: bool,
    /// The swatch button's response.
    pub response: Response,
}

/// What the picker of a swatch keeps between passes: the colour as HSV (so a hue survives black,
/// white and the greys) and the hex text while it is typed.
#[derive(Debug, Clone, Default)]
struct PickerState {
    hsv: Hsv,
    hex: Option<String>,
}

fn open_id(id: &str) -> Id {
    Id::new(id).with("open")
}

fn state_id(id: &str) -> Id {
    Id::new(id).with("picker")
}

/// Whether the popup of swatch `id` is open.
pub fn swatch_open(ctx: &Context, id: &str) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id(id))).unwrap_or(false)
}

/// Opens the popup of swatch `id`, as a click on it does (scripts and tests).
pub fn open_swatch(ctx: &Context, id: &str) {
    ctx.data_mut(|d| d.insert_temp(open_id(id), true));
}

/// Closes the popup of swatch `id`.
pub fn close_swatch(ctx: &Context, id: &str) {
    ctx.data_mut(|d| {
        d.remove::<bool>(open_id(id));
        d.remove::<PickerState>(state_id(id));
    });
}

/// The popup's size: the margin, the field and the strip beside it, the row of the hex field and
/// the eyedropper under them.
pub fn swatch_popup_size() -> Vec2 {
    vec2(PICKER_PAD + PICKER_FIELD + GAP + HUE_STRIP_W + PICKER_PAD, PICKER_PAD + PICKER_FIELD + GAP + DROPPER + PICKER_PAD)
}

/// Where the popup of `size` goes for a swatch at `swatch` in a window `screen`: its left edge at
/// the swatch's, moved left as far as the window's right edge needs (and never past its left
/// edge); 4 pt below the swatch, or above it when there is no room below, or, with room on
/// neither side, on the roomier side pulled inside the window. Always inside `screen` (less 4 pt)
/// when it fits.
pub fn swatch_popup_rect(swatch: Rect, size: Vec2, screen: Rect) -> Rect {
    let inner = screen.shrink(EDGE);
    let left = swatch.left().min(inner.right() - size.x).max(inner.left());
    let below = swatch.bottom() + EDGE;
    let above = swatch.top() - EDGE - size.y;
    let top = if below + size.y <= inner.bottom() {
        below
    } else if above >= inner.top() {
        above
    } else if inner.bottom() - swatch.bottom() >= swatch.top() - inner.top() {
        below.min(inner.bottom() - size.y).max(inner.top())
    } else {
        above.max(inner.top()).min(inner.bottom() - size.y)
    };
    let (left, top) = (if left.is_finite() { left } else { 0.0 }, if top.is_finite() { top } else { 0.0 });
    Rect::from_min_size(pos2(left, top), size)
}

/// The picker's colour as HSV: the one kept from earlier passes while it still names `color` (so
/// the hue and saturation of a grey survive), else `color`'s own, keeping the old hue for a grey.
fn hsv_of(kept: Option<&PickerState>, color: [u8; 3]) -> Hsv {
    match kept {
        Some(s) if rgb_to_u8(hsv_to_rgb(s.hsv[0], s.hsv[1], s.hsv[2])) == color => s.hsv,
        _ => {
            let hsv = rgb_to_hsv(rgb_from_u8(color));
            match kept {
                Some(s) if hsv[1] <= 0.0 || hsv[2] <= 0.0 => [s.hsv[0], if hsv[2] <= 0.0 { s.hsv[1] } else { hsv[1] }, hsv[2]],
                _ => hsv,
            }
        }
    }
}

/// A colour swatch ([`SWATCH_SIZE`], radius 3): `color` inside a 1 pt `field_border` (`text_label`
/// hovered; a 1.5 pt `accent` outline while its popup is open or the eyedropper is `armed`). A click
/// opens or closes its popup ([`swatch_popup_rect`]: the popup frame, `chrome`, 1 pt
/// `button_border`, radius 6, popup shadow), which holds the saturation / value field
/// ([`PICKER_FIELD`] square, its marker a light ring on a dark one) and the hue strip
/// ([`HUE_STRIP_W`] wide, 360° at the top, its marker two `text` triangles) beside it, and under them
/// the hex field (`#rrggbb`; Return or clicking away takes a valid colour, anything else gives back
/// the old) and the eyedropper button (24 pt; `tool_active` with a `pick` glyph while armed). A
/// click or drag in the field or the strip sets the colour. A click outside the popup and the
/// swatch closes it, unless the eyedropper is armed (that click is the pick); so does Esc.
///
/// Registers `id` (`color:effect.<param>`, role `button`, active while open) and, while open,
/// `<id>/popup`, `<id>/field`, `<id>/hue`, `<id>/hex` and `<id>/eyedropper` (role `toggle`, active
/// while armed).
pub fn swatch(ui: &mut Ui, t: &Tokens, id: &str, label: &str, color: [u8; 3], armed: bool) -> SwatchOut {
    let enabled = ui.is_enabled();
    let r = allocate(ui, id, SWATCH_SIZE, enabled);
    let mut open = enabled && swatch_open(ui.ctx(), id);
    if r.clicked() {
        open = !open;
        if open {
            open_swatch(ui.ctx(), id);
        } else {
            close_swatch(ui.ctx(), id);
        }
    }
    if !enabled && open {
        close_swatch(ui.ctx(), id);
        open = false;
    }
    let p = ui.painter();
    let radius = CornerRadius::same(3);
    let shown = if enabled { picker::c32(rgb_from_u8(color)) } else { picker::c32(rgb_from_u8(color)).gamma_multiply(0.4) };
    p.rect_filled(r.rect, radius, shown);
    let border = if !enabled {
        Stroke::new(1.0, t.text_disabled)
    } else if open || armed {
        Stroke::new(t.focus_width, t.accent)
    } else if r.hovered() {
        Stroke::new(1.0, t.text_label)
    } else {
        Stroke::new(1.0, t.field_border)
    };
    p.rect_stroke(r.rect, radius, border, StrokeKind::Inside);
    focus_ring(p, t, &r, r.rect, 3);
    let hex = hex_u8(color);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::ColorButton, enabled, label));
    access(&r, |node| {
        node.set_value(hex.as_str());
        node.set_expanded(open);
    });
    register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), open);
    let r = tooltip(r, t, label, Some(&hex));
    let mut out = SwatchOut { color: None, eyedropper: false, open, response: r.clone() };
    if open {
        popup(ui, t, id, label, color, armed, &r, &mut out);
    }
    out
}

/// The open popup of swatch `id` (see [`swatch`]).
#[allow(clippy::too_many_arguments)] // the swatch's arguments, its response and what it reports
fn popup(ui: &Ui, t: &Tokens, id: &str, label: &str, color: [u8; 3], armed: bool, swatch: &Response, out: &mut SwatchOut) {
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let rect = swatch_popup_rect(swatch.rect, swatch_popup_size(), screen);
    let kept: Option<PickerState> = ctx.data(|d| d.get_temp(state_id(id)));
    let mut state = PickerState { hsv: hsv_of(kept.as_ref(), color), hex: kept.and_then(|k| k.hex) };
    let mut typing = false;
    let texts = text::texts(&ctx);
    egui::Area::new(Id::new(id).with("popup")).order(Order::Foreground).fixed_pos(rect.min).constrain(false).show(&ctx, |ui| {
        let (_, frame) = ui.allocate_space(rect.size());
        let p = ui.painter().clone();
        let round = CornerRadius::same(t.window_radius);
        p.add(t.popup_shadow.as_shape(frame, round));
        p.rect(frame, round, t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
        // The body takes the clicks between the parts, so none falls through to the photo.
        let body = ui.interact(frame, Id::new(format!("{id}/popup")), Sense::CLICK);
        register(&ctx, &format!("{id}/popup"), Role::Other, label, Target::Widget(&body), true);

        // The saturation / value field.
        let field = Rect::from_min_size(frame.min + Vec2::splat(PICKER_PAD), Vec2::splat(PICKER_FIELD));
        let fr = ui.interact(field, Id::new(format!("{id}/field")), Sense::click_and_drag());
        if let Some(at) = fr.interact_pointer_pos().filter(|_| fr.dragged() || fr.clicked() || fr.drag_started()) {
            let (x, y) = (((at.x - field.left()) / field.width()).clamp(0.0, 1.0), ((at.y - field.top()) / field.height()).clamp(0.0, 1.0));
            state.hsv = [state.hsv[0], x, 1.0 - y];
            state.hex = None;
        }
        let hue = state.hsv[0];
        p.add(Shape::mesh(picker::grid_mesh(field, CELLS, |x, y| field_color(hue, x, y))));
        p.rect_stroke(field, 0, Stroke::new(1.0, t.field_border), StrokeKind::Outside);
        let (fx, fy) = field_point(state.hsv);
        let marker = pos2(field.left() + fx * field.width(), field.top() + fy * field.height());
        p.circle_stroke(marker, 5.0, Stroke::new(3.0, t.crosshair_outer));
        p.circle_stroke(marker, 5.0, Stroke::new(1.5, t.crosshair));
        focus_ring(&p, t, &fr, field, 0);
        let (sat, val) = (state.hsv[1] * 100.0, state.hsv[2] * 100.0);
        access(&fr, |node| node.set_value(format!("{sat:.0} %, {val:.0} %")));
        register(&ctx, &format!("{id}/field"), Role::Other, texts.get(Text::SaturationBrightness), Target::Widget(&fr), fr.dragged());

        // The hue strip.
        let strip = Rect::from_min_size(pos2(field.right() + GAP, field.top()), vec2(HUE_STRIP_W, PICKER_FIELD));
        let sr = ui.interact(strip, Id::new(format!("{id}/hue")), Sense::click_and_drag());
        if let Some(at) = sr.interact_pointer_pos().filter(|_| sr.dragged() || sr.clicked() || sr.drag_started()) {
            state.hsv[0] = hue_at((at.y - strip.top()) / strip.height()).min(359.999);
            state.hex = None;
        }
        p.add(Shape::mesh(picker::grid_mesh(strip, CELLS, |_, y| hue_color(y))));
        p.rect_stroke(strip, 0, Stroke::new(1.0, t.field_border), StrokeKind::Outside);
        let sy = strip.top() + hue_point(state.hsv[0]) * strip.height();
        for (x, dir) in [(strip.left() - 1.0, 1.0f32), (strip.right() + 1.0, -1.0)] {
            let tri = vec![pos2(x, sy), pos2(x - 5.0 * dir, sy - 4.0), pos2(x - 5.0 * dir, sy + 4.0)];
            p.add(Shape::convex_polygon(tri, t.text, Stroke::NONE));
        }
        focus_ring(&p, t, &sr, strip, 0);
        let degrees = f64::from(state.hsv[0]);
        sr.widget_info(|| egui::WidgetInfo::slider(true, degrees, texts.get(Text::Hue)));
        access_range(&sr, degrees, 0.0, 360.0, 1.0, &format!("{degrees:.0}°"));
        register(&ctx, &format!("{id}/hue"), Role::Other, texts.get(Text::Hue), Target::Widget(&sr), sr.dragged());

        // The hex field and the eyedropper.
        let row = Rect::from_min_size(pos2(field.left(), field.bottom() + GAP), vec2(field.width() + GAP + HUE_STRIP_W, DROPPER));
        let picked = rgb_to_u8(hsv_to_rgb(state.hsv[0], state.hsv[1], state.hsv[2]));
        let hex_id = format!("{id}/hex");
        let mut text = state.hex.clone().unwrap_or_else(|| hex_u8(picked));
        let field_rect = Rect::from_min_size(pos2(row.left(), row.center().y - t.metrics.text_field_h / 2.0), vec2(HEX_W, t.metrics.text_field_h));
        let f = ui
            .scope_builder(UiBuilder::new().max_rect(field_rect), |ui| {
                TextField::new(&hex_id, "#rrggbb", HEX_W).select_on_focus(true).show(ui, t, &mut text)
            })
            .inner;
        access(&f.response, |node| node.set_label(texts.get(Text::HexColor)));
        typing = f.editing || f.ending.is_some();
        if f.committed() {
            if let Some(c) = parse_hex_u8(&text) {
                state.hsv = hsv_of(Some(&state), c);
            }
            state.hex = None;
        } else if f.cancelled() {
            state.hex = None;
        } else if f.editing {
            state.hex = Some(text);
        }
        let dropper = Rect::from_min_size(pos2(row.right() - DROPPER, row.top()), Vec2::splat(DROPPER));
        let dr = ui.interact(dropper, Id::new(format!("{id}/eyedropper")), Sense::click());
        let fill = if armed {
            Some(t.tool_active)
        } else if dr.is_pointer_button_down_on() {
            Some(t.pressed)
        } else if dr.hovered() {
            Some(t.hover)
        } else {
            None
        };
        if let Some(fill) = fill {
            p.rect_filled(dropper, CornerRadius::same(t.radius), fill);
        }
        let glyph = if armed {
            t.pick
        } else if dr.hovered() {
            t.text
        } else {
            t.icon
        };
        paint_edit(&p, Rect::from_center_size(dropper.center(), Vec2::splat(t.metrics.icon_menu)), EditIcon::Eyedropper, glyph);
        focus_ring(&p, t, &dr, dropper, t.radius);
        let pick = texts.get(Text::PickColor);
        dr.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, true, armed, pick));
        register(&ctx, &format!("{id}/eyedropper"), Role::Toggle, pick, Target::Widget(&dr), armed);
        if dr.clicked() {
            out.eyedropper = true;
        }
        let _ = tooltip(dr, t, pick, None);
    });

    let now = rgb_to_u8(hsv_to_rgb(state.hsv[0], state.hsv[1], state.hsv[2]));
    if now != color {
        out.color = Some(now);
    }
    // A click outside the popup and the swatch, or Esc (not while typing a hex), closes it; while
    // the eyedropper is armed the outside click is the pick and the popup stays.
    let (outside, escape) = ctx.input(|i| {
        let outside = i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|at: Pos2| !rect.contains(at) && !swatch.rect.contains(at));
        (outside, i.key_pressed(Key::Escape))
    });
    if (outside && !armed && !out.eyedropper) || (escape && !typing) {
        close_swatch(&ctx, id);
        out.open = false;
    } else {
        ctx.data_mut(|d| d.insert_temp(state_id(id), state));
    }
}
