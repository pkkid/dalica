//! The video player's volume (`editing.md` §11, `viewing.md` §4): a speaker toggle that mutes and
//! restores the sound, and a short slider beside it.
//!
//! The volume is a level from 0 to 1 and a mute: muting keeps the level, so unmuting gives back
//! the sound as it was. The slider shows what is heard (the level, or nothing while muted); moving
//! it sets the level and unmutes.

use egui::{Color32, CornerRadius, Id, Key, Modifiers, Pos2, Rect, Response, Sense, Ui, Vec2, pos2, vec2};

use super::{Target, access_range, focus_ring, register};
use crate::Tokens;
use crate::icons::{VideoIcon, paint_video};
use crate::registry::Role;
use crate::text::{self, Text};

/// The speaker toggle's edge, in points.
pub const VOLUME_BUTTON: f32 = 24.0;
/// The speaker glyph's size, in points.
pub const VOLUME_GLYPH: f32 = 16.0;
/// The slider's width, in points.
pub const VOLUME_SLIDER_W: f32 = 64.0;
/// The gap between the toggle and the slider, in points.
pub const VOLUME_GAP: f32 = 4.0;
/// What one arrow key moves the level by.
pub const VOLUME_STEP: f32 = 0.1;
/// The level unmuting gives when the level was 0 (silence would not sound unmuted).
pub const VOLUME_UNMUTED: f32 = 0.5;

/// A level put in 0 … 1; one that is not a number is 0.
pub fn volume_level(level: f32) -> f32 {
    if level.is_nan() { 0.0 } else { level.clamp(0.0, 1.0) }
}

/// The volume after a click on the speaker: muted it unmutes with the level it had (or
/// [`VOLUME_UNMUTED`] when that was 0); unmuted it mutes and keeps the level.
pub fn volume_toggled(level: f32, muted: bool) -> (f32, bool) {
    let level = volume_level(level);
    if muted || level == 0.0 { (if level == 0.0 { VOLUME_UNMUTED } else { level }, false) } else { (level, true) }
}

/// What a [`volume`] did this pass.
#[derive(Debug, Clone)]
pub struct VolumeOut {
    /// The new level (0 … 1), when it changed.
    pub level: Option<f32>,
    /// The new mute, when it changed.
    pub muted: Option<bool>,
    /// The speaker toggle's response.
    pub toggle: Response,
    /// The slider's response.
    pub slider: Response,
}

/// The volume: the speaker toggle ([`VOLUME_BUTTON`] square, the speaker glyph for the volume
/// heard, [`VideoIcon::for_volume`], at [`VOLUME_GLYPH`]: no fill at rest, `hover × 0.7` hovered,
/// `pressed` held; glyph `icon`, `text` hovered, `text_disabled` disabled), then [`VOLUME_GAP`]
/// later the slider ([`VOLUME_SLIDER_W`] wide, the zoom slider's height): a 2 pt `track` line
/// (`thumb` hovered), the part heard `text_label`, a 3 × 14 thumb `thumb_hover` (`pick` dragged).
///
/// A click on the speaker mutes or unmutes ([`volume_toggled`]); a click or drag on the slider sets
/// the level and unmutes; ← / → with the slider's focus move it by [`VOLUME_STEP`].
///
/// Registers `<id>/mute` (role `toggle`, active while muted, labelled "Mute") and `id` (role
/// `other`, active while dragged, labelled "Volume").
pub fn volume(ui: &mut Ui, t: &Tokens, id: &str, level: f32, muted: bool) -> VolumeOut {
    let m = &t.metrics;
    let enabled = ui.is_enabled();
    let level = volume_level(level);
    let height = VOLUME_BUTTON.max(m.zoom_slider.y);
    let (_, rect) = ui.allocate_space(vec2(VOLUME_BUTTON + VOLUME_GAP + VOLUME_SLIDER_W, height));
    let button = Rect::from_center_size(pos2(rect.min.x + VOLUME_BUTTON / 2.0, rect.center().y), Vec2::splat(VOLUME_BUTTON));
    let bar = Rect::from_min_max(pos2(button.max.x + VOLUME_GAP, rect.min.y), rect.max);
    let mute_id = format!("{id}/mute");
    let sense = |s: Sense| if enabled { s } else { Sense::hover() };
    let mut rt = ui.interact(button, Id::new(&mute_id), sense(Sense::click()));
    let mut rs = ui.interact(bar, Id::new(id), sense(Sense::click_and_drag()));

    let (mut now_level, mut now_muted) = (level, muted);
    if enabled {
        if rt.clicked() {
            (now_level, now_muted) = volume_toggled(level, muted);
        }
        let inset = m.zoom_thumb.x / 2.0 + 1.0;
        let (x0, x1) = (bar.min.x + inset, bar.max.x - inset);
        if (rs.dragged() || rs.clicked())
            && let Some(Pos2 { x, .. }) = rs.interact_pointer_pos()
            && x1 > x0
        {
            now_level = volume_level((x - x0) / (x1 - x0));
            now_muted = false;
        }
        if rs.has_focus() {
            let (left, right) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight)));
            let heard = if now_muted { 0.0 } else { now_level };
            let by = VOLUME_STEP * (f32::from(u8::from(right)) - f32::from(u8::from(left)));
            if by != 0.0 {
                now_level = volume_level(heard + by);
                now_muted = false;
            }
        }
    }
    let out_level = (now_level != level).then_some(now_level);
    let out_muted = (now_muted != muted).then_some(now_muted);
    if out_muted.is_some() {
        rt.mark_changed();
    }
    if out_level.is_some() || out_muted.is_some() {
        rs.mark_changed();
    }

    let p = ui.painter();
    // The speaker toggle.
    if enabled && (rt.is_pointer_button_down_on() || rt.hovered()) {
        let fill = if rt.is_pointer_button_down_on() { t.pressed } else { t.hover.gamma_multiply(0.7) };
        p.rect_filled(button, CornerRadius::same(t.radius), fill);
    }
    focus_ring(p, t, &rt, button, t.radius);
    let glyph: Color32 = if !enabled {
        t.text_disabled
    } else if rt.hovered() {
        t.text
    } else {
        t.icon
    };
    paint_video(p, Rect::from_center_size(button.center(), Vec2::splat(VOLUME_GLYPH)), VideoIcon::for_volume(now_level, now_muted), glyph);

    // The slider, showing what is heard.
    let inset = m.zoom_thumb.x / 2.0 + 1.0;
    let (x0, x1) = (bar.min.x + inset, bar.max.x - inset);
    let heard = if now_muted { 0.0 } else { now_level };
    let xh = x0 + heard * (x1 - x0).max(0.0);
    let y = bar.center().y;
    let half = m.slider_track / 2.0;
    let hot = enabled && (rs.hovered() || rs.dragged());
    let track = if !enabled {
        t.text_disabled.gamma_multiply(0.6)
    } else if hot {
        t.thumb
    } else {
        t.track
    };
    p.rect_filled(Rect::from_min_max(pos2(x0, y - half), pos2(x1, y + half)), 1.0, track);
    let part = if enabled { t.text_label } else { t.text_disabled };
    if xh > x0 {
        p.rect_filled(Rect::from_min_max(pos2(x0, y - half), pos2(xh, y + half)), 1.0, part);
    }
    let thumb_color = if !enabled {
        t.text_disabled
    } else if rs.dragged() || rs.is_pointer_button_down_on() {
        t.pick
    } else {
        t.thumb_hover
    };
    let thumb = Rect::from_center_size(pos2(xh.round(), y), m.zoom_thumb);
    p.rect_filled(thumb, 1.0, thumb_color);
    focus_ring(p, t, &rs, thumb.expand(1.0), 1);

    let ctx = ui.ctx();
    let texts = text::texts(ctx);
    let (mute, volume) = (texts.get(Text::Mute), texts.get(Text::Volume));
    rt.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, now_muted, mute));
    register(ctx, &mute_id, Role::Toggle, mute, Target::Widget(&rt), now_muted);
    rs.widget_info(|| egui::WidgetInfo::slider(enabled, f64::from(heard), volume));
    access_range(&rs, f64::from(heard), 0.0, 1.0, f64::from(VOLUME_STEP), &format!("{:.0} %", heard * 100.0));
    register(ctx, id, Role::Other, volume, Target::Widget(&rs), rs.dragged());
    let rt = rt.on_hover_text(texts.get(if now_muted { Text::Unmute } else { Text::Mute }));
    VolumeOut { level: out_level, muted: out_muted, toggle: rt, slider: rs }
}
