//! The video's control bar (`editing.md` §11, `viewing.md` §4), a stub until M15.26 draws it from
//! M15.6's scrubber and volume widgets: along the bottom of the video, Play / Pause
//! (`button:video.toggle`, `player.toggle`), the scrubber (`slider:video.position`) with the trim
//! ruler's handles (`handle:video.trimStart`, `handle:video.trimEnd`), Set Start and Set End
//! (`button:video.setStart`, `button:video.setEnd`, greyed until the player trims), the volume
//! (`slider:video.volume`) and Full Screen (`button:video.fullScreen`, `player.fullScreen`). Until
//! then the scrubber and the volume are their tracks, painted.

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, ToastKind};
use egui::{Align, Layout, Rect, Ui, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::commands::video::LATER;

/// The bar's height (points).
pub const HEIGHT: f32 = 40.0;
/// The volume slider's width (points).
pub const VOLUME_W: f32 = 80.0;
/// A trim handle's width (points).
const HANDLE_W: f32 = 6.0;

pub const TOGGLE_ID: &str = "button:video.toggle";
pub const POSITION_ID: &str = "slider:video.position";
pub const TRIM_START_ID: &str = "handle:video.trimStart";
pub const TRIM_END_ID: &str = "handle:video.trimEnd";
pub const SET_START_ID: &str = "button:video.setStart";
pub const SET_END_ID: &str = "button:video.setEnd";
pub const VOLUME_ID: &str = "slider:video.volume";
pub const FULL_SCREEN_ID: &str = "button:video.fullScreen";

/// Draws the bar into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.chrome);
    let video = app.state().video;
    let mut run: Option<&'static str> = None;
    let inner = rect.shrink2(vec2(8.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
        let label = if video.playing { "Pause" } else { "Play" };
        if widgets::text_button(ui, &t, TOGGLE_ID, label, video.playing).clicked() {
            run = Some("player.toggle");
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::text_button(ui, &t, FULL_SCREEN_ID, "Full Screen", video.full_screen).clicked() {
                run = Some("player.fullScreen");
            }
            let (_, volume) = ui.allocate_space(vec2(VOLUME_W, 4.0));
            track(ui, &t, volume, f32::from(video.volume) / f32::from(crate::state::VOLUME_MAX));
            widgets::register(
                ui.ctx(),
                VOLUME_ID,
                Role::Other,
                "Volume",
                Target::Painted { rect: volume.expand2(vec2(0.0, 6.0)), clip: rect },
                false,
            );
            for (id, label) in [(SET_END_ID, "Set End"), (SET_START_ID, "Set Start")] {
                let r = ui.add_enabled_ui(false, |ui| widgets::text_button(ui, &t, id, label, false)).inner;
                widgets::tooltip(r, &t, LATER, None);
            }
            let left = ui.available_rect_before_wrap();
            let scrubber = Rect::from_min_max(pos2(left.min.x + 8.0, rect.center().y - 2.0), pos2(left.max.x - 8.0, rect.center().y + 2.0));
            if scrubber.is_positive() {
                track(ui, &t, scrubber, 0.0);
                widgets::register(
                    ui.ctx(),
                    POSITION_ID,
                    Role::Other,
                    "Position",
                    Target::Painted { rect: scrubber.expand2(vec2(0.0, 6.0)), clip: rect },
                    false,
                );
                for (id, x, label) in [(TRIM_START_ID, scrubber.min.x, "Trim start"), (TRIM_END_ID, scrubber.max.x, "Trim end")] {
                    let handle = Rect::from_center_size(pos2(x, scrubber.center().y), vec2(HANDLE_W, 16.0));
                    ui.painter().rect_filled(handle, 1, t.thumb);
                    widgets::register(ui.ctx(), id, Role::Other, label, Target::Painted { rect: handle, clip: rect }, false);
                }
            }
        });
    });
    if let Some(id) = run
        && let Err(e) = app.run(id, &json!({}))
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// A slider's track in `rect`, filled up to `fraction` (0 … 1).
fn track(ui: &Ui, t: &dalica_ui_kit::Tokens, rect: Rect, fraction: f32) {
    ui.painter().rect_filled(rect, 2, t.track);
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    if f > 0.0 {
        let filled = Rect::from_min_max(rect.min, pos2(rect.min.x + rect.width() * f, rect.max.y));
        ui.painter().rect_filled(filled, 2, t.accent);
    }
}
