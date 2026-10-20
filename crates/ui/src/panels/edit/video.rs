//! A video in the edit view (`editing.md` §11, `viewing.md` §4): while the edit view's photo is a
//! video (`crate::commands::video::shown`), the photo area is the video (`region:video`) with its
//! control bar along its bottom ([`bar`]: play and pause, the scrubber with its readout and the
//! trim ruler's handles, Set Start, Set End, the volume, Full Screen), and the edit panel shows the
//! video tab ([`tab`], drawn by `super::panel`) in place of tabs 1–5. Full Screen spreads the video
//! and its bar over the whole window until it is pressed again.
//!
//! **The picture** (`picture:video`), fitted to the area above the bar with the photo's
//! [`PADDING`] all round at the shape it shows at (the recipe's turns applied): the frame the
//! engine's player shows (`Session::video_frame`: decoded ahead on the player's worker, never here),
//! else the poster frame (the video's thumbnail, M15.20), else the video tile
//! (`light_table::cell::video_tile`) with its sprocket edge at the size the scan learnt (16:9 while
//! it is unknown). A new frame replaces the one texture's pixels; nothing here reads the disk or
//! decodes.
//!
//! **A video Dalica cannot play** (`dalica_engine::video::playback`: its codec, or a stream its
//! decoder refused when the player opened it) says so on its tile, "This video can't be played"
//! (`text:video.cannotPlay`), with **Open With ▸** under it (`button:video.openWith`): the photo
//! menu's Open With rows in a popup (`photo.openWith` on the video), so another application can
//! play it. Its bar keeps what works on the container (the trim ruler, Set Start, Set End) and
//! greys the rest with the reason.
//!
//! Drawn over the edit view ([`show`], after `super::show`); while it plays the window repaints
//! every frame.

pub mod bar;
pub mod tab;

use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_edit::Dihedral;
use dalica_engine::Thumb;
use dalica_image::Rgba8;
use dalica_library::Library;
use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Badges, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::containers::menu::menu_style;
use egui::epaint::Vertex;
use egui::{
    Align, Color32, CornerRadius, Id, Layout, Mesh, Popup, PopupKind, Rect, RectAlign, Sense, SetOpenCommand, Shape, Stroke, StrokeKind,
    TextureHandle, Ui, UiBuilder, Vec2, pos2, vec2,
};
use serde_json::Value;

use crate::app::App;
use crate::commands::video::Player;
use crate::menus::{self, Node, Platform};
use crate::panels::layout;
use crate::panels::light_table::cell;
use crate::panels::thumbnails::{recipe_orientation, shape_of, turn_point};

/// The video's area, for `ui.screenshot {region: "video"}` and `ui.widgets {region}`.
pub const REGION_ID: &str = "region:video";
/// The video's picture (its frame, poster frame or tile).
pub const PICTURE_ID: &str = "picture:video";
/// "This video can't be played" on a video Dalica cannot play.
pub const CANNOT_PLAY_ID: &str = "text:video.cannotPlay";
/// Open With ▸ under it.
pub const OPEN_WITH_ID: &str = "button:video.openWith";
/// The space around the picture (the photo's, `canvas::PADDING`).
pub const PADDING: f32 = super::canvas::PADDING;
/// The shape a video of unknown frame size is drawn at.
pub const UNKNOWN_SIZE: (u32, u32) = (16, 9);
/// The long side of the poster frame asked for (the area's, at most).
const POSTER_SIDE: f32 = 1920.0;

/// `ms` as the player's readout writes a time: `hh:mm:ss`.
pub fn clock(ms: u64) -> String {
    dalica_ui_kit::widgets::clock_text(ms)
}

/// What the edit view shows of video `id`: the size it shows at (the photo's size as the scan
/// stored it, its container's rotation applied, then turned by the recipe; [`UNKNOWN_SIZE`] when
/// the scan did not learn it) and why Dalica cannot play it from its facts (`None`: it can).
pub fn look(library: &Library, id: PhotoId) -> ((u32, u32), Option<String>) {
    let size = library.photo(id).map_or((0, 0), |p| {
        let shape = shape_of(&library.info(id).edits, p.width, p.height);
        shape.turn.display_size(p.width, p.height)
    });
    let size = if size.0 == 0 || size.1 == 0 { UNKNOWN_SIZE } else { size };
    let empty = dalica_library::VideoFacts::default();
    let why = dalica_engine::video::playback(library.video_facts(id).unwrap_or(&empty)).video.err();
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

/// What the picture's texture was made from.
#[derive(Clone)]
enum Source {
    Frame(Weak<dalica_image::formats::video::frames::Frame>),
    Poster(Weak<Rgba8>),
}

/// The picture's one texture, kept between frames in the context's data.
#[derive(Clone)]
struct Picture {
    photo: PhotoId,
    source: Source,
    handle: TextureHandle,
    /// The upright image's size.
    size: (u32, u32),
}

fn picture_id() -> Id {
    Id::new("video.picture")
}

/// The image `image` (upright) as a colour image; `opaque` takes its bytes as they are (a video
/// frame: every alpha 255, so premultiplied and not are the same), else they are premultiplied.
fn color_image(image: &Rgba8, opaque: bool) -> Option<egui::ColorImage> {
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return None;
    }
    Some(if opaque {
        egui::ColorImage::from_rgba_premultiplied(dims, &image.data)
    } else {
        egui::ColorImage::from_rgba_unmultiplied(dims, &image.data)
    })
}

/// The texture to draw for video `photo` this frame: the player's frame, else its poster frame
/// (uploaded when it changed: a new frame sets the same texture's pixels), else `None` (the tile).
fn texture(app: &mut App, ctx: &egui::Context, photo: PhotoId, player: Option<&Player>, need: u32) -> Option<Picture> {
    let kept: Option<Picture> = ctx.data(|d| d.get_temp(picture_id())).filter(|p: &Picture| p.photo == photo);
    let session = app.session_mut()?;
    let frame = if player.is_some_and(|p| p.id == Some(photo)) { session.video_frame() } else { None };
    let (image, source, opaque): (Option<&Rgba8>, Source, bool) = match &frame {
        Some(f) => {
            if let Some(k) = &kept
                && matches!(&k.source, Source::Frame(w) if std::ptr::eq(w.as_ptr(), Arc::as_ptr(f)))
            {
                return kept;
            }
            (Some(&f.image), Source::Frame(Arc::downgrade(f)), true)
        }
        None => match session.thumbnail(photo, need) {
            Thumb::Image { image, .. } => {
                if let Some(k) = &kept
                    && matches!(&k.source, Source::Poster(w) if std::ptr::eq(w.as_ptr(), Arc::as_ptr(&image)))
                {
                    return kept;
                }
                let picture = upload(ctx, kept, photo, &image, Source::Poster(Arc::downgrade(&image)), false);
                return picture;
            }
            // While a frame or poster was shown, it stays until the next arrives.
            _ => return kept.filter(|k| matches!(k.source, Source::Frame(_))),
        },
    };
    image.and_then(|image| upload(ctx, kept, photo, image, source, opaque))
}

/// Puts `image` in the kept texture (its pixels replaced when it is the same size) or a new one.
fn upload(ctx: &egui::Context, kept: Option<Picture>, photo: PhotoId, image: &Rgba8, source: Source, opaque: bool) -> Option<Picture> {
    let pixels = color_image(image, opaque)?;
    let size = (image.width, image.height);
    let handle = match kept {
        Some(mut k) => {
            k.handle.set(pixels, egui::TextureOptions::LINEAR);
            k.handle
        }
        None => ctx.load_texture("video:picture", pixels, egui::TextureOptions::LINEAR),
    };
    let picture = Picture { photo, source, handle, size };
    ctx.data_mut(|d| d.insert_temp(picture_id(), picture.clone()));
    Some(picture)
}

/// The upright texture over `rect`, turned by `turn`.
fn textured(rect: Rect, texture: egui::TextureId, turn: Dihedral) -> Shape {
    let back = turn.inverse();
    let mut mesh = Mesh::with_texture(texture);
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let (u, v) = turn_point(back, x, y);
        let pos = pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
        mesh.vertices.push(Vertex { pos, uv: pos2(u, v), color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

/// The video's area: the photo area, or the whole window while it fills the screen.
pub fn area(app: &App, window: Rect) -> Rect {
    if app.state().video.full_screen { window } else { layout::compute_edit(window, app.state()).canvas }
}

/// Draws the video over the edit view's photo area while the edit view shows a video; nothing
/// otherwise.
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(photo) = crate::commands::video::shown(app) else { return };
    let ctx = ui.ctx().clone();
    let player = crate::commands::video::sync(app, &ctx);
    let t = *app.tokens();
    let area = area(app, ui.max_rect());
    if !area.is_positive() {
        return;
    }
    if app.state().video.full_screen {
        // Over the whole window: what is under it is not hovered.
        ui.interact(area, Id::new("video.fullScreen.cover"), Sense::hover());
    }
    ui.painter().rect_filled(area, 0, t.canvas);
    widgets::register(&ctx, REGION_ID, Role::Other, "Video", Target::Painted { rect: area, clip: area }, false);
    let bar_h = bar::HEIGHT.min(area.height());
    let video = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - bar_h));
    let Some((size, facts_why)) = app.session().map(|s| look(s.library(), photo)) else { return };
    let why = player.as_ref().and_then(|p| p.reason.clone()).or(facts_why);
    let turn = app.session().map_or(Dihedral::IDENTITY, |s| recipe_orientation(&s.library().info(photo).edits));
    let need = (video.width().max(video.height()) * ctx.pixels_per_point()).clamp(1.0, POSTER_SIDE) as u32;
    let texture = if why.is_none() { texture(app, &ctx, photo, player.as_ref(), need) } else { None };
    let size = texture.as_ref().map_or(size, |p| turn.display_size(p.size.0, p.size.1));
    let picture = picture_rect(video, size);
    if picture.is_positive() {
        let painter = ui.painter().with_clip_rect(video.intersect(ui.clip_rect()));
        widgets::photo_shadow(&painter, picture);
        match &texture {
            Some(p) => {
                painter.add(textured(picture, p.handle.id(), turn));
            }
            None => {
                cell::video_tile(&painter, &t, picture);
                widgets::badges(&painter, &t, picture, &Badges { video: Some(widgets::Video { seconds: None }), ..Badges::default() });
            }
        }
        widgets::register(&ctx, PICTURE_ID, Role::Other, "Video", Target::Painted { rect: picture, clip: video }, false);
        if why.is_some() {
            cannot_play(app, ui, &t, picture, video);
        }
    }
    let strip = Rect::from_min_max(pos2(area.min.x, area.max.y - bar_h), area.max);
    let mut child = ui.new_child(UiBuilder::new().id_salt("videoBar").max_rect(strip));
    child.set_clip_rect(strip.intersect(ui.clip_rect()));
    bar::show(app, &mut child, player.as_ref(), why.as_deref());
    if player.as_ref().is_some_and(|p| p.playing || p.pending) {
        ctx.request_repaint();
    }
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
