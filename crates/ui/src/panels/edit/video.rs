//! A video in the edit view (`editing.md` §11, `viewing.md` §4): while the edit view's photo is a
//! video (`crate::commands::video::shown`), the photo area is the video (`region:video`) with its
//! control bar along its bottom ([`bar`]: play and pause, the scrubber with the trim ruler's
//! handles, Set Start, Set End, the volume, Full Screen), and the edit panel shows the video tab
//! ([`tab`], drawn by `super::panel`) in place of tabs 1–5.
//!
//! **The picture.** Until M15.20 makes poster frames, the video is the video tile
//! (`light_table::cell::video_tile`, `picture:video`) with its sprocket edge, fitted to the area
//! above the bar with the photo's [`PADDING`] all round, at the size the video shows at (the
//! container's frame size and rotation, then the recipe's turns; 16:9 while the size is unknown).
//! **A video Dalica cannot play** (`dalica_engine::video::playable`: every video until M15.15
//! decodes frames) says so on its tile, "This video can't be played" (`text:video.cannotPlay`),
//! with **Open With ▸** under it (`button:video.openWith`): the photo menu's Open With rows in a
//! popup (`photo.openWith` on the video, filled by M15.22), so another application can play it.
//!
//! Drawn over the edit view ([`show`], after `super::show`); nothing here reads the disk or
//! decodes: the facts are the library's.

pub mod bar;
pub mod tab;

use dalica_core::PhotoId;
use dalica_library::Library;
use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Badges, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::containers::menu::menu_style;
use egui::{
    Align, CornerRadius, Id, Layout, Popup, PopupKind, Rect, RectAlign, Sense, SetOpenCommand, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};
use serde_json::Value;

use crate::app::App;
use crate::menus::{self, Node, Platform};
use crate::panels::layout;
use crate::panels::light_table::cell;
use crate::panels::thumbnails::shape_of;

/// The video's area, for `ui.screenshot {region: "video"}` and `ui.widgets {region}`.
pub const REGION_ID: &str = "region:video";
/// The video's picture (the video tile until its poster frame).
pub const PICTURE_ID: &str = "picture:video";
/// "This video can't be played" on a video Dalica cannot play.
pub const CANNOT_PLAY_ID: &str = "text:video.cannotPlay";
/// Open With ▸ under it.
pub const OPEN_WITH_ID: &str = "button:video.openWith";
/// The space around the picture (the photo's, `canvas::PADDING`).
pub const PADDING: f32 = super::canvas::PADDING;
/// The shape a video of unknown frame size is drawn at.
pub const UNKNOWN_SIZE: (u32, u32) = (16, 9);

/// `ms` as the player's readout writes a time: `hh:mm:ss`.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

/// What the edit view shows of video `id`: the size it shows at (the photo's size as the scan
/// stored it, its container's rotation applied, then turned by the recipe; [`UNKNOWN_SIZE`] when
/// the scan did not learn it) and why Dalica cannot play it (`None`: it can).
pub fn look(library: &Library, id: PhotoId) -> ((u32, u32), Option<&'static str>) {
    let size = library.photo(id).map_or((0, 0), |p| {
        let shape = shape_of(&library.info(id).edits, p.width, p.height);
        shape.turn.display_size(p.width, p.height)
    });
    let size = if size.0 == 0 || size.1 == 0 { UNKNOWN_SIZE } else { size };
    let why = match library.video_facts(id) {
        Some(facts) => dalica_engine::video::playable(facts).err(),
        None => Some(dalica_engine::video::CANNOT_PLAY),
    };
    (size, why)
}

/// Where the picture goes in `area` (the photo area above the bar): fitted inside it less
/// [`PADDING`] all round, centred, keeping the shape of `size`.
pub fn picture_rect(area: Rect, size: (u32, u32)) -> Rect {
    let boxed = area.shrink(PADDING);
    if !(boxed.is_positive() && boxed.is_finite()) {
        return Rect::from_center_size(area.center(), Vec2::ZERO);
    }
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let scale = (boxed.width() / w).min(boxed.height() / h);
    Rect::from_center_size(boxed.center(), vec2(w * scale, h * scale))
}

/// Draws the video over the edit view's photo area while the edit view shows a video; nothing
/// otherwise.
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(photo) = crate::commands::video::shown(app) else { return };
    let t = *app.tokens();
    let l = layout::compute_edit(ui.max_rect(), app.state());
    let area = l.canvas;
    if !area.is_positive() {
        return;
    }
    ui.painter().rect_filled(area, 0, t.canvas);
    widgets::register(ui.ctx(), REGION_ID, Role::Other, "Video", Target::Painted { rect: area, clip: area }, false);
    let bar_h = bar::HEIGHT.min(area.height());
    let video = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - bar_h));
    let Some((size, why)) = app.session().map(|s| look(s.library(), photo)) else { return };
    let picture = picture_rect(video, size);
    if picture.is_positive() {
        let painter = ui.painter().with_clip_rect(video.intersect(ui.clip_rect()));
        widgets::photo_shadow(&painter, picture);
        cell::video_tile(&painter, &t, picture);
        widgets::badges(&painter, &t, picture, &Badges { video: Some(widgets::Video { seconds: None }), ..Badges::default() });
        widgets::register(ui.ctx(), PICTURE_ID, Role::Other, "Video", Target::Painted { rect: picture, clip: video }, false);
        if why.is_some() {
            cannot_play(app, ui, &t, picture, video);
        }
    }
    let strip = Rect::from_min_max(pos2(area.min.x, area.max.y - bar_h), area.max);
    let mut child = ui.new_child(UiBuilder::new().id_salt("videoBar").max_rect(strip));
    child.set_clip_rect(strip.intersect(ui.clip_rect()));
    bar::show(app, &mut child);
}

/// "This video can't be played" under the tile's Play glyph, and Open With ▸ under it, kept inside
/// `video`.
fn cannot_play(app: &mut App, ui: &mut Ui, t: &Tokens, picture: Rect, video: Rect) {
    let glyph = (picture.width().min(picture.height()) / 3.0).clamp(10.0, 64.0);
    let text = crate::tl!("This video can't be played");
    let galley = ui.painter().layout_no_wrap(text.clone(), fonts::semibold(t.body), t.text);
    let top = (picture.center().y + glyph / 2.0 + 12.0).min(video.max.y - galley.size().y - t.metrics.button_h - 16.0).max(video.min.y);
    let line = Rect::from_min_size(pos2(picture.center().x - galley.size().x / 2.0, top), galley.size());
    ui.painter().with_clip_rect(video.intersect(ui.clip_rect())).galley(line.min.round(), galley, t.text);
    widgets::register(ui.ctx(), CANNOT_PLAY_ID, Role::Other, &text, Target::Painted { rect: line, clip: video }, false);
    let label = crate::tl!("Open With");
    let galley = ui.painter().layout_no_wrap(label.clone(), fonts::semibold(t.button_label), t.text);
    let chevron = 12.0;
    let size = vec2((galley.size().x + 18.0 + chevron + 4.0).round(), t.metrics.button_h);
    let button = Rect::from_min_size(pos2((picture.center().x - size.x / 2.0).round(), (line.max.y + 10.0).round()), size);
    let r = ui.interact(button, Id::new(OPEN_WITH_ID), Sense::click());
    let p = ui.painter();
    let fill = if r.is_pointer_button_down_on() || Popup::is_id_open(ui.ctx(), open_with_popup()) {
        t.pressed
    } else if r.hovered() {
        t.hover
    } else {
        t.button
    };
    p.rect(button, CornerRadius::same(t.radius), fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    p.galley(pos2(button.min.x + 9.0, button.center().y - galley.size().y / 2.0).round(), galley, t.text);
    let mark = Rect::from_center_size(pos2(button.max.x - 9.0 - chevron / 2.0, button.center().y), Vec2::splat(chevron));
    icons::paint(p, mark, Icon::ChevronRight, t.text);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    widgets::register(ui.ctx(), OPEN_WITH_ID, Role::Button, &label, Target::Widget(&r), false);
    open_with(app, ui, button, r.clicked());
}

fn open_with_popup() -> Id {
    Id::new(OPEN_WITH_ID).with("popup")
}

/// The Open With rows of the photo menu (`crate::menus::photo_menu`): what the photo's own Open
/// With ▸ submenu lists, so the two never differ.
pub fn open_with_rows(app: &App) -> Vec<Node> {
    menus::photo_menu(app, Platform::current())
        .into_iter()
        .find_map(|n| match n {
            Node::Submenu { label: "Open With", children, .. } => Some(children),
            _ => None,
        })
        .unwrap_or_default()
}

/// Open With ▸'s popup to the right of `button`: a click on the button (`clicked`) opens it, or
/// closes it when open; a row clicked runs through `menus::activate` on the video (the edit view's
/// photo); a refusal is on the status line.
fn open_with(app: &mut App, ui: &Ui, button: Rect, clicked: bool) {
    let id = open_with_popup();
    let ctx = ui.ctx().clone();
    if !clicked && !Popup::is_id_open(&ctx, id) {
        // Closed: its applications are forgotten (`crate::commands::os::sync_open_with`).
        if app.state().open_with.as_ref().is_some_and(|m| Some(m.photo) == app.state().edit.photo) {
            crate::commands::os::sync_open_with(app, &ctx, app.state().edit.photo, false);
        }
        return;
    }
    let nodes = open_with_rows(app);
    let top = ctx.content_rect().top();
    let mut picked = None;
    Popup::new(id, ctx.clone(), button, ui.layer_id())
        .kind(PopupKind::Menu)
        .layout(Layout::top_down_justified(Align::Min))
        .style(menu_style)
        .gap(2.0)
        .align(RectAlign::RIGHT_START)
        .open_memory(clicked.then_some(SetOpenCommand::Toggle))
        .show(|ui| picked = menus::popup_rows(ui, &nodes, top));
    // Open, it asks the system for the video's applications once ("Loading…" until then).
    let open = Popup::is_id_open(&ctx, id) && picked.is_none();
    let photo = app.state().edit.photo;
    crate::commands::os::sync_open_with(app, &ctx, photo, open);
    let Some(item) = picked.filter(|i| i.enabled) else { return };
    if let Err(e) = menus::activate(app, &item, &Value::Null) {
        app.state_mut().status = e.to_string();
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

    #[test]
    fn the_picture_is_fitted_inside_the_padding_and_keeps_its_shape() {
        let area = Rect::from_min_size(pos2(280.0, 104.0), vec2(1120.0, 640.0));
        let r = picture_rect(area, (1920, 1080));
        assert!(area.shrink(PADDING).contains_rect(r), "{r:?}");
        assert!((r.width() / r.height() - 16.0 / 9.0).abs() < 0.01, "{r:?}");
        assert_eq!(r.center(), area.center());
        let tall = picture_rect(area, (1080, 1920));
        assert!((tall.height() - area.shrink(PADDING).height()).abs() < 0.01, "{tall:?}");
        // Upscaled: a small clip fills the area as a still never does in Fit; it has no pixels yet.
        assert!(picture_rect(area, (160, 90)).width() > 160.0);
        for bad in [Rect::NOTHING, Rect::from_min_size(pos2(0.0, 0.0), vec2(30.0, 30.0)), Rect::from_min_size(pos2(0.0, 0.0), vec2(f32::NAN, 9.0))] {
            assert!(!picture_rect(bad, (16, 9)).is_positive(), "{bad:?}");
        }
    }
}
