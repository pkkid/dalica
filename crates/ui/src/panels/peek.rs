//! The quick full-screen preview (`viewing.md` §2): while Ctrl+Alt (Cmd+Option) is held over a
//! thumbnail, or in the edit view, the photo fills the window on black over whatever is shown;
//! nothing else is drawn.
//!
//! - **The gesture** ([`watch`], at the start of every frame, before the panels take input): the
//!   modifiers going down over a light-table cell (found by its `thumb:` id in the last frame's
//!   widget registry) start the preview of that photo in that cell's section; in the edit view,
//!   anywhere off a cell, of the open photo. Over empty space nothing happens. While it shows,
//!   the wheel's events are taken out of the frame's input (the light table does not scroll and
//!   Ctrl+wheel does not resize its cells) and step through the section ([`cmd::step`]): one step
//!   per notch, or per [`WHEEL_STEP`] points of a trackpad. Releasing the keys ends it
//!   ([`cmd::end`]: the last photo focused and scrolled into view). A preview an agent started
//!   with `peek {id}` ends with `peek {id: null}`, not with the keys.
//! - **The pixels** ([`show`], over everything, on its own layer): the best thumbnail the light
//!   table has uploaded, scaled up on the first frame (its sketch before any; the placeholder only
//!   when there is nothing at all), replaced in place by the screen-size preview once it arrives.
//!   A preview pane already on the photo is used as it is (the edit view's); otherwise the preview
//!   opens the photo in pane [`PANE`], which it closes again when it ends, and never touches while
//!   compare holds that pane (`edit.compare` other than `one`). The section's neighbours are
//!   prefetched.
//! - [`stats`]: what the preview drew, frame by frame (tests).

use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_image::Rgba8;
use dalica_ui_kit::registry::{self, Role};
use egui::{Color32, Context, Event, Id, MouseWheelUnit, Rect, TextureHandle, Ui, pos2, vec2};

use super::Panels;
use super::thumbnails::{Drawn, PhotoRef, fit_within};
use crate::app::App;
use crate::commands::peek as cmd;
use crate::state::{Compare, Room, Section, View};

/// The preview pane the quick preview opens its photo in when no pane shows it (pane 0 is the
/// edit view's).
pub const PANE: usize = 1;
/// Points of trackpad scrolling that make one step.
pub const WHEEL_STEP: f32 = 50.0;
/// How often the preview looks again while the screen-size preview is on its way.
const POLL: std::time::Duration = std::time::Duration::from_millis(33);

/// What the preview drew in one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// The screen-size preview.
    Preview,
    /// A thumbnail (what [`super::thumbnails::Thumbnails::paint`] drew), while the preview loads.
    Thumbnail(Drawn),
}

/// What the preview drew since the app started (tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Frames the preview was shown.
    pub frames: u64,
    /// Of those, frames that drew no picture of the photo (the placeholder, or nothing).
    pub grey_frames: u64,
    /// What the latest photo's first frame drew.
    pub first: Option<Shown>,
    /// What the last frame drew.
    pub last: Option<Shown>,
}

/// The gesture between frames (in the context's data).
#[derive(Clone, Default)]
struct Gesture {
    /// Started by the keys, so it ends when they are released.
    held: bool,
    /// The section it steps through.
    section: Option<Section>,
    /// Trackpad points not yet a step.
    wheel: f32,
    /// The photo it opened in [`PANE`], to close when it ends.
    opened: Option<PhotoId>,
    /// The photo whose neighbours were prefetched.
    prefetched: Option<PhotoId>,
    /// The photo shown in the last frame.
    shown: Option<PhotoId>,
    /// The preview's texture and the image it was made from.
    texture: Option<(Weak<Rgba8>, TextureHandle)>,
    stats: Stats,
}

fn key() -> Id {
    Id::new("peek")
}

fn load(ctx: &Context) -> Gesture {
    ctx.data(|d| d.get_temp::<Gesture>(key())).unwrap_or_default()
}

fn store(ctx: &Context, g: Gesture) {
    ctx.data_mut(|d| d.insert_temp(key(), g));
}

/// What the preview drew so far.
pub fn stats(ctx: &Context) -> Stats {
    load(ctx).stats
}

/// Whether Ctrl+Alt (Cmd+Option on macOS) is held.
fn keys_held(ctx: &Context) -> bool {
    ctx.input(|i| i.modifiers.command && i.modifiers.alt)
}

/// The photo the keys would show now, with the section to step through: the light-table cell under
/// the pointer, else in the edit view the open photo; `None` in a dialog, a room, the slideshow,
/// the viewer, or while typing (AltGr is Ctrl+Alt on some keyboards).
fn target(app: &App, ctx: &Context) -> Option<(PhotoId, Option<Section>)> {
    let state = app.state();
    if !state.dialogs.is_empty() || state.room != Room::Library || ctx.egui_wants_keyboard_input() {
        return None;
    }
    let library = app.session()?.library();
    let pos = ctx.input(|i| i.pointer.hover_pos());
    let cell = pos.and_then(|pos| {
        registry::widgets(ctx).into_iter().rev().filter(|w| w.role == Role::Thumbnail).find_map(|w| {
            let [x, y, width, height] = w.rect;
            if !Rect::from_min_size(pos2(x, y), vec2(width, height)).contains(pos) {
                return None;
            }
            let (photo, section) = parse_thumb(&w.id)?;
            let photo = library.photo(photo)?;
            Some((photo.id, Some(section.unwrap_or(Section::Folder(photo.folder)))))
        })
    });
    match state.view {
        View::Library => cell,
        View::Edit => cell.or_else(|| state.edit.photo.map(|p| (p, None))),
        View::Slideshow | View::Viewer => None,
    }
}

/// The photo and, for an album's copy, the section of a cell's id (`thumb:<PhotoId>` or
/// `thumb:<PhotoId>/in/<album>`); `None` for any other id (a badge: `thumb:42/star`).
fn parse_thumb(id: &str) -> Option<(PhotoId, Option<Section>)> {
    let rest = id.strip_prefix("thumb:")?;
    let (photo, album) = match rest.split_once('/') {
        Some((photo, tail)) => (photo, Some(tail.strip_prefix("in/")?)),
        None => (rest, None),
    };
    let photo = PhotoId::new(photo.parse::<u64>().ok()?);
    let section = match album {
        Some(a) => Some(format!("album:{a}").parse::<Section>().ok()?),
        None => None,
    };
    Some((photo, section))
}

/// Takes the wheel's events out of this frame's input; returns the steps they make (positive: on
/// to the next photos, the wheel turned towards the user).
fn take_wheel(ctx: &Context, carry: &mut f32) -> i32 {
    let mut steps = 0i32;
    ctx.input_mut(|i| {
        i.events.retain(|e| {
            let Event::MouseWheel { unit, delta, .. } = e else { return true };
            let d = if delta.y != 0.0 { delta.y } else { delta.x };
            if !d.is_finite() {
                return false;
            }
            match unit {
                MouseWheelUnit::Point => {
                    *carry -= d;
                    let n = (*carry / WHEEL_STEP).trunc();
                    *carry -= n * WHEEL_STEP;
                    steps = steps.saturating_add(n.clamp(-1000.0, 1000.0) as i32);
                }
                MouseWheelUnit::Line | MouseWheelUnit::Page => {
                    if d != 0.0 {
                        let n = (d.abs().round().clamp(1.0, 1000.0)) as i32;
                        steps = steps.saturating_add(if d < 0.0 { n } else { -n });
                    }
                }
            }
            false
        });
        i.smooth_scroll_delta = egui::Vec2::ZERO;
    });
    steps
}

/// Starts, steps and ends the preview from the keys and the wheel. Called at the start of every
/// frame, before the panels take input.
pub fn watch(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let mut g = load(&ctx);
    let held = keys_held(&ctx);
    if app.state().peek.is_none() {
        g.held = false;
        if held && let Some((photo, section)) = target(app, &ctx) {
            app.state_mut().peek = Some(photo);
            g.held = true;
            g.section = section;
            g.wheel = 0.0;
            take_wheel(&ctx, &mut g.wheel);
        }
    } else if g.held && !held {
        g.held = false;
        cmd::end(app, g.section);
    } else {
        let steps = take_wheel(&ctx, &mut g.wheel);
        if steps != 0 {
            cmd::step(app, g.section, steps);
        }
    }
    store(&ctx, g);
}

/// Draws the preview over `ui`'s whole rect, on its own layer above the panels; with no preview,
/// lets go of the pane it opened.
pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let mut g = load(&ctx);
    let comparing = app.state().view == View::Edit && app.state().edit.compare != Compare::One;
    let Some(photo) = app.state().peek else {
        if g.opened.is_none() && g.shown.is_none() {
            return;
        }
        if let Some(opened) = g.opened.take()
            && !comparing
            && let Some(s) = app.session_mut()
            && s.preview_photo_in(PANE) == Some(opened)
        {
            s.close_preview_in(PANE);
        }
        g.texture = None;
        g.shown = None;
        g.prefetched = None;
        store(&ctx, g);
        return;
    };
    let t = *app.tokens();
    let rect = ui.max_rect();
    let ppp = ctx.pixels_per_point();
    let fit = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, key()));
    painter.rect_filled(rect, 0, t.viewer_bg);

    // The screen-size preview: a pane already on the photo, else ours.
    let first = g.shown != Some(photo);
    g.shown = Some(photo);
    if let Some(s) = app.session_mut() {
        let on = (0..=PANE).find(|&pane| s.preview_photo_in(pane) == Some(photo));
        let ours = s.preview_photo_in(PANE).is_none() || s.preview_photo_in(PANE) == g.opened;
        match on {
            Some(PANE) if g.opened == Some(photo) => {
                let _ = s.set_preview_fit_in(PANE, fit);
            }
            Some(_) => {}
            None if !comparing && ours && s.open_preview_in(PANE, photo, fit).is_ok() => g.opened = Some(photo),
            None => {}
        }
    }
    if g.prefetched != Some(photo) {
        g.prefetched = Some(photo);
        let neighbours = cmd::neighbours(app, g.section);
        if let Some(s) = app.session_mut() {
            s.prefetch_previews(&neighbours);
        }
    }
    let preview =
        app.session().and_then(|s| (0..=PANE).find_map(|pane| s.preview_in(pane).filter(|f| f.photo == photo)).map(|f| Arc::clone(&f.image)));

    let shown = match preview {
        Some(image) => {
            let fresh = g.texture.as_ref().is_some_and(|(w, _)| w.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &image)));
            if !fresh {
                g.texture = upload(&ctx, &image).map(|h| (Arc::downgrade(&image), h));
            }
            match &g.texture {
                Some((_, tex)) => {
                    let at = fit_within(rect, image.width, image.height);
                    painter.image(tex.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    Some(Shown::Preview)
                }
                None => thumbnail(p, app, &painter, rect, ppp, photo),
            }
        }
        None => {
            g.texture = None;
            thumbnail(p, app, &painter, rect, ppp, photo)
        }
    };
    let st = &mut g.stats;
    st.frames = st.frames.saturating_add(1);
    if matches!(shown, None | Some(Shown::Thumbnail(Drawn::Placeholder))) {
        st.grey_frames = st.grey_frames.saturating_add(1);
    }
    if first {
        st.first = shown;
    }
    st.last = shown;
    if shown != Some(Shown::Preview) && app.session().is_some_and(|s| s.preview_pending() > 0) {
        ctx.request_repaint_after(POLL);
    }
    store(&ctx, g);
}

/// Paints the best thumbnail of `photo` the light table has, fitted to `rect`.
fn thumbnail(p: &mut Panels, app: &App, painter: &egui::Painter, rect: Rect, ppp: f32, photo: PhotoId) -> Option<Shown> {
    let session = app.session()?;
    let photo = session.library().photo(photo)?;
    let r = PhotoRef { session, photo };
    let (w, h) = p.thumbnails.displayed(r);
    let at = fit_within(rect, w, h);
    let device_px = at.width().max(at.height()) * ppp;
    Some(Shown::Thumbnail(p.thumbnails.paint(painter, r, at, device_px)))
}

/// Uploads `image` as the preview's texture; `None` when it is malformed.
fn upload(ctx: &Context, image: &Rgba8) -> Option<TextureHandle> {
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return None;
    }
    let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
    Some(ctx.load_texture("peek:preview", pixels, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_ids_name_the_photo_and_an_album_section() {
        assert_eq!(parse_thumb("thumb:42"), Some((PhotoId::new(42), None)));
        assert_eq!(parse_thumb("thumb:42/in/starred"), Some((PhotoId::new(42), Some("album:starred".parse().unwrap()))));
        assert_eq!(parse_thumb("thumb:42/star"), None, "a badge is not a cell");
        assert_eq!(parse_thumb("tree:folder:7"), None);
        assert_eq!(parse_thumb("thumb:x"), None);
    }
}
