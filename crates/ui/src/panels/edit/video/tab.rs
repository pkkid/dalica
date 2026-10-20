//! The edit panel's video tab (`editing.md` §11), which `super::super::panel` shows in place of
//! tabs 1–5 while the edit view shows a video: a grid of tool buttons, two to a row,
//!
//! ```text
//! Save Frame as Picture   Export Clip
//! Rotate Left             Rotate Right
//! ```
//!
//! **Save Frame as Picture** (`button:video.saveFrame`, `player.saveFrame`: the frame shown, as a
//! JPEG in `Pictures/Dalica/Video Frames`) and **Export Clip** (`button:video.exportClip`,
//! `player.exportClip`: the trimmed part, where the save picker says) are greyed with what they
//! wait for until M15.26 makes the player act ([`LATER`]); **Rotate Left** and **Rotate Right**
//! (`button:video.rotateCcw`, `button:video.rotateCw`) turn the video now: the photo's own
//! `photo.rotate.ccw` and `photo.rotate.cw`, one step of its history (a rotation is one of a
//! video's own edits).

use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{Glyph, Icon, VideoIcon, paint_glyph};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, TOOL_BUTTON_H, Target, ToastKind, tool_button_width};
use egui::{Align2, CornerRadius, Id, Rect, Response, Sense, Ui, Vec2, WidgetType, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::commands::video::LATER;

pub const SAVE_FRAME_ID: &str = "button:video.saveFrame";
pub const EXPORT_CLIP_ID: &str = "button:video.exportClip";
pub const ROTATE_CCW_ID: &str = "button:video.rotateCcw";
pub const ROTATE_CW_ID: &str = "button:video.rotateCw";

/// The grid's columns and the gap between its buttons (both ways).
pub const COLUMNS: u8 = 2;
pub const GAP: f32 = 4.0;

/// The tab's buttons in reading order: id, glyph, the command it runs, and whether it acts now
/// (else it waits for the player, [`LATER`]); [`texts`] gives each its label and tooltip.
pub const BUTTONS: [(&str, Glyph, &str, bool); 4] = [
    (SAVE_FRAME_ID, Glyph::Video(VideoIcon::SaveFrame), "player.saveFrame", false),
    (EXPORT_CLIP_ID, Glyph::Video(VideoIcon::ExportClip), "player.exportClip", false),
    (ROTATE_CCW_ID, Glyph::Shell(Icon::RotateCcw), "photo.rotate.ccw", true),
    (ROTATE_CW_ID, Glyph::Shell(Icon::RotateCw), "photo.rotate.cw", true),
];

/// The label and the tooltip of the button `id` of [`BUTTONS`], in the language drawn.
pub fn texts(id: &str) -> (String, String) {
    match id {
        SAVE_FRAME_ID => (crate::tl!("Save Frame as Picture"), crate::tl!("Save the frame shown as a picture")),
        EXPORT_CLIP_ID => (crate::tl!("Export Clip"), crate::tl!("Export the trimmed clip")),
        ROTATE_CCW_ID => (crate::tl!("Rotate Left"), crate::tl!("Rotate the video counterclockwise")),
        _ => (crate::tl!("Rotate Right"), crate::tl!("Rotate the video clockwise")),
    }
}

/// The rects of the buttons inside `rect`, in [`BUTTONS`]' order.
pub fn layout(t: &Tokens, rect: Rect) -> [Rect; 4] {
    let w = tool_button_width(t, rect.width(), COLUMNS, GAP);
    std::array::from_fn(|i| {
        let (col, row) = ((i % 2) as f32, (i / 2) as f32);
        Rect::from_min_size(pos2(rect.min.x + col * (w + GAP), rect.min.y + row * (TOOL_BUTTON_H + GAP)), vec2(w, TOOL_BUTTON_H))
    })
}

/// The height the tab needs: its two rows.
pub fn needed_height() -> f32 {
    2.0 * TOOL_BUTTON_H + GAP
}

/// Draws the tab's page into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let Some(photo) = app.state().edit.photo else { return };
    let mut clicked = None;
    for ((id, glyph, command, live), rect) in BUTTONS.iter().zip(layout(&t, ui.max_rect())) {
        let (label, tip) = texts(id);
        // A button that waits says what for.
        let tip = if *live { tip } else { crate::i18n::tr(LATER) };
        if tool_button(ui, &t, id, *glyph, &label, rect, *live, &tip).clicked() {
            clicked = Some(*command);
        }
    }
    if let Some(command) = clicked
        && let Err(e) = app.run(command, &json!({ "ids": [photo] }))
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// A tool button over `rect`, drawn as the Basic Fixes tab draws its own (`design-system.md` §6)
/// with any glyph: the `inset` icon tile 3 pt from the top with the glyph in it, the label (12 pt,
/// elided) below; hovered `hover`, held `pressed`, disabled in `text_disabled`. Registers `id`
/// (role `tool`); the tooltip shows disabled too.
#[allow(clippy::too_many_arguments)] // the tool button's parts
fn tool_button(ui: &mut Ui, t: &Tokens, id: &str, glyph: Glyph, label: &str, rect: Rect, enabled: bool, tip: &str) -> Response {
    let m = &t.metrics;
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(rect, radius, t.pressed);
    } else if enabled && r.hovered() {
        p.rect_filled(rect, radius, t.hover);
    }
    let tile = Rect::from_min_size(pos2(rect.center().x - m.tool_tile.x / 2.0, rect.min.y + 3.0), m.tool_tile);
    p.rect_filled(tile, radius, t.inset);
    let (color, text) = if !enabled {
        (t.text_disabled, t.text_disabled)
    } else if r.hovered() {
        (t.text, t.text)
    } else {
        (t.icon, t.text_label)
    };
    paint_glyph(p, Rect::from_center_size(tile.center(), Vec2::splat(m.icon_tool)), glyph, color);
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), fonts::regular(t.count), text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 4.0).max(1.0));
    let galley = p.layout_job(job);
    let row = Rect::from_min_max(pos2(rect.min.x, tile.max.y + 1.0), pos2(rect.max.x, rect.max.y - 3.0));
    p.galley(Align2::CENTER_CENTER.align_size_within_rect(galley.size(), row).min.round(), galley, text);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::register(ui.ctx(), id, Role::Tool, label, Target::Widget(&r), false);
    widgets::tooltip(r, t, tip, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fits_the_panel_two_to_a_row() {
        let t = Tokens::DARK;
        let rect = Rect::from_min_size(pos2(8.0, 104.0), vec2(264.0, 300.0));
        let cells = layout(&t, rect);
        for c in cells {
            assert!(rect.contains_rect(c), "{c:?}");
        }
        assert_eq!(cells[0].min.y, cells[1].min.y);
        assert_eq!(cells[2].min.y, cells[0].max.y + GAP);
        assert_eq!(cells[3].max.y - rect.min.y, needed_height());
        let ids: std::collections::HashSet<_> = BUTTONS.iter().map(|b| b.0).collect();
        assert_eq!(ids.len(), BUTTONS.len());
        for (id, ..) in BUTTONS {
            assert!(id.starts_with("button:video."), "{id}: the protocol's names");
        }
    }
}
