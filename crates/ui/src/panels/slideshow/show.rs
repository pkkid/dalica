//! The photos of a slideshow frame (`viewing.md` §1.2–§1.3): the photo shown, and while a change is
//! under way the photo going out, each from its screen-size preview in the preview pane the show
//! keeps it in (the best thumbnail in memory stands in until that preview is there, so a frame
//! never waits), placed and clipped by M11.4's geometry for the transition and painted by
//! [`super::transition`]; then the caption on M11.3's plate while captions are on.
//!
//! A photo's size is its pixels as shown (its edits' turns and crop applied), so the fit never
//! shows it above twice those pixels whatever size the preview was made at. Textures are kept per
//! UI thread for the last few images painted, so a frame uploads only an image it has not drawn.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_create::slideshow::{self as create, Size, Slide};
use dalica_edit::Recipe;
use dalica_engine::cmd::slideshow::Frame;
use dalica_engine::{Session, Thumb};
use dalica_image::{Orientation, Rgba8};
use dalica_ui_kit::widgets;
use egui::{Rect, TextureHandle, Ui};

use crate::app::App;

/// The caption's automation id.
pub const CAPTION_ID: &str = "text:slideshow.caption";
/// The most images whose textures are kept (the two photos of a change, and their stand-ins).
const KEPT: usize = 4;

/// A texture of an image the session handed, and that image.
struct Tex {
    source: Weak<Rgba8>,
    handle: TextureHandle,
}

thread_local! {
    static TEXTURES: RefCell<Vec<Tex>> = const { RefCell::new(Vec::new()) };
}

/// The texture of `image`: the one kept for it, else a new upload (the oldest kept one let go).
/// `None` when the image is malformed.
fn texture(ctx: &egui::Context, image: &Arc<Rgba8>) -> Option<egui::TextureId> {
    TEXTURES.with(|kept| {
        let mut kept = kept.borrow_mut();
        kept.retain(|t| t.source.strong_count() > 0);
        if let Some(t) = kept.iter().find(|t| t.source.upgrade().is_some_and(|s| Arc::ptr_eq(&s, image))) {
            return Some(t.handle.id());
        }
        let dims = [image.width as usize, image.height as usize];
        if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
            return None;
        }
        let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
        let handle = ctx.load_texture("slideshow", pixels, egui::TextureOptions::LINEAR);
        let id = handle.id();
        if kept.len() >= KEPT {
            kept.remove(0);
        }
        kept.push(Tex { source: Arc::downgrade(image), handle });
        Some(id)
    })
}

/// Lets go of every texture kept (the show ended).
pub fn release() {
    TEXTURES.with(|kept| kept.borrow_mut().clear());
}

/// What photo `id` paints now: its preview in `pane` when it is there, else the best thumbnail in
/// memory for a long side of `need` device pixels; with its recipe (what the preview shows).
fn image_of(session: &mut Session, id: PhotoId, pane: usize, need: u32) -> Option<(Arc<Rgba8>, Vec<String>)> {
    if let Some(p) = session.preview_in(pane).filter(|p| p.photo == id) {
        return Some((Arc::clone(&p.image), p.recipe.clone()));
    }
    let recipe = session.library().info(id).edits.clone();
    match session.thumbnail(id, need) {
        Thumb::Image { image, .. } => Some((image, recipe)),
        Thumb::Sketch(_) | Thumb::NotScanned => None,
    }
}

/// Photo `id`'s size in pixels as shown: the library's size turned and cut by `recipe`; the
/// image's own size when the library does not know it.
fn shown_size(session: &Session, id: PhotoId, recipe: &[String], image: &Rgba8) -> (u32, u32) {
    let dims = session.library().photo(id).map_or((0, 0), |p| (p.width, p.height));
    if dims.0 == 0 || dims.1 == 0 {
        return (image.width, image.height);
    }
    let parsed = Recipe::parse(recipe).unwrap_or_default();
    dalica_edit::Geometry::new(&parsed, Orientation::default(), dims.0, dims.1).display_size(1.0).unwrap_or(dims)
}

/// One photo of the frame, ready to place.
struct Photo {
    texture: egui::TextureId,
    slide: Slide,
}

fn photo(ui: &Ui, session: &mut Session, (id, pane): (PhotoId, usize), index: usize, motion: f32, need: u32) -> Option<Photo> {
    let (image, recipe) = image_of(session, id, pane, need)?;
    let texture = texture(ui.ctx(), &image)?;
    let size = shown_size(session, id, &recipe, &image);
    let slide = Slide { size, zoom: create::pan_zoom_amount(index), motion, ..Slide::new(size) };
    Some(Photo { texture, slide })
}

/// Paints `frame` over `rect` (already black): the photos as the transition places them (the photo
/// shown zoomed by `zoom` about the point `pan`, M11.21's [`super::zoom`]), then the caption of the
/// photo shown while `captions` is on. Answers where the photo shown sits fitted, in the screen's
/// units (what a drag pans).
pub fn paint(app: &mut App, ui: &Ui, rect: Rect, frame: &Frame, captions: bool, (zoom, pan): (f32, [f32; 2])) -> Option<create::Rect> {
    let t = *app.tokens();
    let ppp = ui.ctx().pixels_per_point();
    let need = ((rect.width().max(rect.height()) * ppp).ceil().clamp(1.0, 16_384.0)) as u32;
    let v = frame.view;
    let session = app.session_mut()?;
    let incoming = frame.photo.zip(v.index).and_then(|(p, i)| photo(ui, session, p, i, v.motion, need));
    let change = v.change.filter(|_| frame.outgoing.is_some());
    let outgoing = frame.outgoing.zip(change).and_then(|(p, (i, _))| photo(ui, session, p, i, v.outgoing_motion, need));
    let screen = Size::new(rect.width(), rect.height());
    let painter = ui.painter_at(rect);
    match (&incoming, &outgoing) {
        (Some(inc), out) => {
            let progress = change.map_or(1.0, |(_, p)| p);
            let f = create::frame(v.transition, screen, ppp, out.as_ref().map(|o| &o.slide), &inc.slide, progress);
            if let (Some(layer), Some(o)) = (f.outgoing, out) {
                super::transition::paint(&painter, rect.min, &layer, o.texture);
            }
            if let Some(layer) = f.incoming {
                let layer = super::zoom::zoom_layer(layer, screen, zoom, pan);
                super::transition::paint(&painter, rect.min, &layer, inc.texture);
            }
        }
        // The incoming photo has nothing to show yet: the outgoing one stays where it was.
        (None, Some(o)) => {
            let f = create::frame(v.transition, screen, ppp, None, &o.slide, 1.0);
            if let Some(layer) = f.incoming {
                super::transition::paint(&painter, rect.min, &layer, o.texture);
            }
        }
        (None, None) => {}
    }
    let fit = incoming.as_ref().map(|inc| create::fit(inc.slide.size, screen, ppp));
    if !captions {
        return fit;
    }
    let Some((id, _)) = frame.photo else { return fit };
    let caption = session.library().caption(id).to_string();
    // Above the control bar's place, so the bar never covers it (and it never moves with the bar).
    let bottom = (create::geometry::CAPTION_BOTTOM * rect.height()).max(super::bar::CAPTION_CLEAR);
    let _ = widgets::caption_plate(&painter, &t, CAPTION_ID, rect, bottom, &caption);
    fit
}
