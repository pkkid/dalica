//! A video in the edit view (`editing.md` §11, `viewing.md` §4), a stub until M15.21 shows its
//! poster and M15.26 plays it: while the edit view's photo is a video
//! (`crate::commands::video::shown`), the photo area is the video (`region:video`) with its
//! control bar along its bottom ([`bar`]: play and pause, the scrubber with the trim ruler's
//! handles, Set Start, Set End, the volume, Full Screen), and the edit panel shows the video tab
//! ([`tab`]: Save Frame as Picture, Export Clip) instead of tabs 1–5. Until then the area says
//! what arrives and how the player's view state stands, so the keys and the bar can be seen to
//! act. Drawn over the edit view ([`show`], after `super::show`); nothing here reads the disk or
//! decodes.

pub mod bar;
pub mod tab;

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Rect, Ui, UiBuilder, pos2};

use crate::app::App;
use crate::panels::layout;

/// The video's area, for `ui.screenshot {region: "video"}` and `ui.widgets {region}`.
pub const REGION_ID: &str = "region:video";
/// What the stub area says.
pub const STUB: &str = "Video playback arrives with the video player.";

/// `ms` as the player's readout writes a time: `hh:mm:ss`.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// Draws the video over the edit view's photo area, and its tab over the edit panel, while the
/// edit view shows a video; nothing otherwise.
pub fn show(app: &mut App, ui: &mut Ui) {
    if crate::commands::video::shown(app).is_none() {
        return;
    }
    let t = *app.tokens();
    let l = layout::compute_edit(ui.max_rect(), app.state());
    let area = l.canvas;
    if area.is_positive() {
        ui.painter().rect_filled(area, 0, t.canvas);
        widgets::register(ui.ctx(), REGION_ID, Role::Other, "Video", Target::Painted { rect: area, clip: area }, false);
        let bar_h = bar::HEIGHT.min(area.height());
        let video = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - bar_h));
        let v = app.state().video;
        let line = format!("{} · {}", if v.playing { "Playing" } else { "Paused" }, clock(v.position));
        let font = dalica_ui_kit::fonts::regular(t.body);
        widgets::label_in(ui.painter(), video, Align2::CENTER_CENTER, STUB, font.clone(), t.text_label);
        let below = Rect::from_min_max(pos2(video.min.x, video.center().y + 12.0), video.max);
        widgets::label_in(ui.painter(), below, Align2::CENTER_TOP, &line, font, t.text_dim);
        let strip = Rect::from_min_max(pos2(area.min.x, area.max.y - bar_h), area.max);
        let mut child = ui.new_child(UiBuilder::new().id_salt("videoBar").max_rect(strip));
        child.set_clip_rect(strip.intersect(ui.clip_rect()));
        bar::show(app, &mut child);
    }
    if let Some(panel) = l.edit_panel.filter(|r| r.is_positive()) {
        ui.painter().rect_filled(panel, 0, t.chrome);
        let mut child = ui.new_child(UiBuilder::new().id_salt("videoTab").max_rect(panel.shrink(12.0)));
        child.set_clip_rect(panel.intersect(ui.clip_rect()));
        tab::show(app, &mut child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readout_writes_hours_minutes_and_seconds() {
        assert_eq!(clock(0), "00:00:00");
        assert_eq!(clock(59_999), "00:00:59");
        assert_eq!(clock((3 * 3600 + 2 * 60) * 1000), "03:02:00");
        assert!(clock(u64::MAX).ends_with(":51"), "any number, no panic");
    }
}
