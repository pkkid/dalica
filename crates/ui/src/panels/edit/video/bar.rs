//! The video's control bar (`editing.md` §11, `viewing.md` §4), along the bottom of the video,
//! from left to right:
//!
//! ```text
//! [▶]  ●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━  [ [▸ ] [◂ ] ]  🔈 ━━━  [Full Screen]
//!      [━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━]
//!      00:00:09 / 00:00:41 (trimmed to 00:00:21)
//! ```
//!
//! - **Play / Pause** (`button:video.toggle`, `player.toggle`; active while playing): greyed with
//!   the reason on a clip Dalica cannot play.
//! - **The scrubber** (`slider:video.position`, M15.6's `scrubber`): the playhead clicked or
//!   dragged to a time (`player.seek`; it stays at the start of a clip Dalica cannot play), the trim
//!   ruler's handles under it (also `handle:video.trimStart` and `handle:video.trimEnd`), the
//!   readout under them (`slider:video.position/readout`, "00:00:09 / 00:00:41 (trimmed to
//!   00:00:21)"). A handle dragged shows where it goes and trims once it is let go (`player.setStart
//!   {ms}` or `player.setEnd {ms}`: one step of the video's history per drag; ← / → with its focus
//!   trim a frame at a time). The view state's `trim` names the handle being dragged.
//! - **Set Start** and **Set End** (`button:video.setStart`, `button:video.setEnd`): the trim at
//!   the playhead.
//! - **The volume** (`slider:video.volume` and its speaker `slider:video.volume/mute`, M15.6's
//!   `volume`; `player.volume`, 0 muted): greyed with the reason for a clip whose sound Dalica
//!   cannot decode.
//! - **Full Screen** (`button:video.fullScreen`, `player.fullScreen`; active while the video fills
//!   the window).
//!
//! Every control is a widget of its own, reached with Tab and worked with the keyboard (Space or
//! Enter presses a button; ← / → move the playhead, a handle or the volume).

use dalica_ui_kit::Tokens;
use dalica_ui_kit::icons::{Glyph, VideoIcon, paint_glyph};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, SCRUB_TRACK_ROW, Scrubber, TRIM_GRAB, Target, ToastKind};
use egui::{Align, CornerRadius, Id, Layout, Rect, Response, Sense, Ui, Vec2, WidgetType, pos2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::commands::video::Player;
use crate::state::{TrimHandle, VOLUME_MAX};

/// The bar's height (points): the scrubber's three rows and a margin above and below.
pub const HEIGHT: f32 = 60.0;
/// An icon button's edge (points).
pub const BUTTON: f32 = 28.0;
/// The space between the bar's controls (points).
pub const GAP: f32 = 6.0;

pub const TOGGLE_ID: &str = "button:video.toggle";
pub const POSITION_ID: &str = "slider:video.position";
pub const TRIM_START_ID: &str = "handle:video.trimStart";
pub const TRIM_END_ID: &str = "handle:video.trimEnd";
pub const SET_START_ID: &str = "button:video.setStart";
pub const SET_END_ID: &str = "button:video.setEnd";
pub const VOLUME_ID: &str = "slider:video.volume";
pub const FULL_SCREEN_ID: &str = "button:video.fullScreen";
/// The scrubber's readout.
pub const READOUT_ID: &str = "slider:video.position/readout";

/// A trim handle being dragged and where it is, kept between frames until it is let go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pending {
    start: Option<u64>,
    end: Option<u64>,
    /// The handle dragged last frame, so the view state's `trim` changes only when a drag starts
    /// or ends.
    dragged: Option<TrimHandle>,
}

fn pending_id() -> Id {
    Id::new("video.bar.pending")
}

fn unmuted_id() -> Id {
    Id::new("video.bar.unmuted")
}

/// The frame's length in milliseconds of the video `player` shows: from its frame rate, else its
/// length over its frames, else 0 (the scrubber's default).
fn frame_ms(app: &App, player: &Player) -> u64 {
    let rate = player.id.and_then(|id| app.session()?.library().video_facts(id).map(|f| f.frame_rate_milli)).unwrap_or(0);
    if rate > 0 {
        return (1_000_000 / u64::from(rate)).max(1);
    }
    player.duration_ms.checked_div(player.frame_count).map_or(0, |ms| ms.max(1))
}

/// Draws the bar into `ui`'s rect for `player` (`None` while the engine has not opened the video);
/// `why` is why its pictures cannot be played.
pub fn show(app: &mut App, ui: &mut Ui, player: Option<&Player>, why: Option<&str>) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.chrome);
    let ctx = ui.ctx().clone();
    let empty = Player::default();
    let p = player.unwrap_or(&empty);
    // Why play, the steps and seeking are greyed: the clip's reason, or that it is still opening.
    let pictures_why = why.map(str::to_string).or_else(|| p.pictures_refused());
    let pictures = pictures_why.is_none() && player.is_some();
    let video = app.state().video;
    let mut run: Vec<(&'static str, Value)> = Vec::new();
    let mut pending: Pending = ctx.data(|d| d.get_temp(pending_id())).unwrap_or_default();
    let inner = rect.shrink2(vec2(8.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        let (glyph, label) = if video.playing { (VideoIcon::Pause, crate::tl!("Pause")) } else { (VideoIcon::Play, crate::tl!("Play")) };
        let tip = pictures_why.clone().unwrap_or_else(|| label.clone());
        if glyph_button(ui, &t, TOGGLE_ID, Glyph::Video(glyph), &label, video.playing, pictures, &tip).clicked() {
            run.push(("player.toggle", json!({})));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            if widgets::text_button(ui, &t, FULL_SCREEN_ID, &crate::tl!("Full Screen"), video.full_screen).clicked() {
                run.push(("player.fullScreen", json!({})));
            }
            volume(app, ui, &t, p, player.is_some(), &mut run);
            let open = player.is_some();
            for (id, glyph, label, command) in [
                (SET_END_ID, VideoIcon::SetEnd, crate::tl!("Set End"), "player.setEnd"),
                (SET_START_ID, VideoIcon::SetStart, crate::tl!("Set Start"), "player.setStart"),
            ] {
                if glyph_button(ui, &t, id, Glyph::Video(glyph), &label, false, open, &label).clicked() {
                    run.push((command, json!({})));
                }
            }
            let width = (ui.available_width() - GAP).max(0.0);
            let (start, end) = p.bounds();
            let clip = Scrubber {
                duration_ms: p.duration_ms,
                position_ms: video.position,
                start_ms: pending.start.unwrap_or(start),
                end_ms: pending.end.unwrap_or(end),
                step_ms: frame_ms(app, p),
            };
            let out = ui.add_enabled_ui(open, |ui| widgets::scrubber(ui, &t, POSITION_ID, &clip, width)).inner;
            if let Some(ms) = out.position.filter(|_| pictures) {
                run.push(("player.seek", json!({ "ms": ms })));
            }
            if let Some(ms) = out.start {
                pending.start = Some(ms);
            }
            if let Some(ms) = out.end {
                pending.end = Some(ms);
            }
            // The handles under their protocol names: the scrubber's grabs, where it draws them.
            let track = out.response.rect;
            let (x0, x1) = (track.min.x + widgets::PLAYHEAD_R + 1.0, track.max.x - widgets::PLAYHEAD_R - 1.0);
            let x_of = |ms: u64| if clip.duration_ms == 0 { x0 } else { x0 + (ms as f64 / clip.duration_ms as f64) as f32 * (x1 - x0) };
            let ruler_y = track.min.y + SCRUB_TRACK_ROW;
            let ruler = |a: f32, b: f32| Rect::from_min_max(pos2(a, ruler_y), pos2(b, ruler_y + widgets::SCRUB_RULER_ROW));
            let normal = clip.normalized();
            let (xs, xe) = (x_of(normal.start_ms), x_of(normal.end_ms));
            let dragged = ctx.dragged_id();
            let start_dragged = dragged == Some(Id::new(format!("{POSITION_ID}/start")));
            let end_dragged = dragged == Some(Id::new(format!("{POSITION_ID}/end")));
            for (id, grab, label, active) in [
                (TRIM_START_ID, ruler(xs - 4.0, xs - 4.0 + TRIM_GRAB), crate::tl!("Trim start"), start_dragged),
                (TRIM_END_ID, ruler(xe + 4.0 - TRIM_GRAB, xe + 4.0), crate::tl!("Trim end"), end_dragged),
            ] {
                if open {
                    widgets::register(&ctx, id, Role::Other, &label, Target::Painted { rect: grab, clip: rect }, active);
                }
            }
            // A handle let go (or stepped with the keys) trims once.
            if let Some(ms) = pending.start.filter(|_| !start_dragged) {
                run.push(("player.setStart", json!({ "ms": ms })));
                pending.start = None;
            }
            if let Some(ms) = pending.end.filter(|_| !end_dragged) {
                run.push(("player.setEnd", json!({ "ms": ms })));
                pending.end = None;
            }
            let handle = if start_dragged {
                Some(TrimHandle::Start)
            } else if end_dragged {
                Some(TrimHandle::End)
            } else {
                None
            };
            if handle != pending.dragged {
                pending.dragged = handle;
                app.state_mut().video.trim = handle;
            }
        });
    });
    ctx.data_mut(|d| d.insert_temp(pending_id(), pending));
    for (id, params) in run {
        if let Err(e) = app.run(id, &params) {
            app.toast(&e.to_string(), ToastKind::Caution);
        }
    }
}

/// The volume: M15.6's speaker and slider over the player's 0 … 100 (0 muted; unmuting gives back
/// the level it had), greyed with the reason for a clip whose sound Dalica cannot decode.
fn volume(app: &App, ui: &mut Ui, t: &Tokens, p: &Player, open: bool, run: &mut Vec<(&'static str, Value)>) {
    let ctx = ui.ctx().clone();
    let now = app.state().video.volume;
    if now > 0 {
        ctx.data_mut(|d| d.insert_temp(unmuted_id(), now));
    }
    let unmuted: u8 = ctx.data(|d| d.get_temp(unmuted_id())).unwrap_or(now);
    let max = f32::from(VOLUME_MAX);
    let level = f32::from(if now > 0 { now } else { unmuted }) / max;
    let enabled = open && p.muted.is_none();
    let out = ui.add_enabled_ui(enabled, |ui| widgets::volume(ui, t, VOLUME_ID, level, now == 0)).inner;
    if let Some(why) = p.muted.as_deref().filter(|_| open) {
        widgets::tooltip(out.toggle.clone(), t, why, None);
        widgets::tooltip(out.slider.clone(), t, why, None);
    }
    if out.level.is_some() || out.muted.is_some() {
        let muted = out.muted.unwrap_or(now == 0);
        let level = out.level.unwrap_or(level);
        let volume = if muted { 0 } else { ((level * max).round() as u8).min(VOLUME_MAX) };
        if volume != now {
            run.push(("player.volume", json!({ "volume": volume })));
        }
    }
}

/// A square button of edge [`BUTTON`] with `glyph`: the `icon_button` look (a highlight square,
/// `tool_active` when `active`, `hover × 0.7` hovered; the glyph `icon`, `text` hovered, `pick`
/// active, `text_disabled` disabled) for any glyph, named `label`, with `tip` its tooltip (shown
/// disabled too: the reason). Registers `id` (role `iconButton`).
#[allow(clippy::too_many_arguments)] // the button's parts
fn glyph_button(ui: &mut Ui, t: &Tokens, id: &str, glyph: Glyph, label: &str, active: bool, enabled: bool, tip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let (_, rect) = ui.allocate_space(Vec2::splat(BUTTON));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if active {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.tool_active);
    } else if enabled && r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let color = if !enabled {
        t.text_disabled
    } else if active {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    paint_glyph(p, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), glyph, color);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    widgets::register(ui.ctx(), id, Role::IconButton, label, Target::Widget(&r), active);
    widgets::tooltip(r, t, tip, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_lasts_its_rate_or_the_clip_over_its_frames() {
        let app = App::new(None);
        let p = Player { duration_ms: 2000, frame_count: 50, ..Player::default() };
        assert_eq!(frame_ms(&app, &p), 40);
        assert_eq!(frame_ms(&app, &Player::default()), 0, "nothing known: the scrubber's default");
        assert_eq!(frame_ms(&app, &Player { duration_ms: 3, frame_count: 50, ..Player::default() }), 1);
    }
}
