//! The photos of a slideshow frame (`viewing.md` §1.2–§1.3): the photo shown, and while a change is
//! under way the photo going out, each from its screen-size preview in the preview pane the show
//! keeps it in (the best thumbnail in memory stands in until that preview is there, so a frame
//! never waits), placed and clipped by M11.4's geometry for the transition and painted by
//! [`super::transition`]; then the caption on M11.3's plate while captions are on.
//!
//! A photo's size is its pixels as shown (its edits' turns and crop applied), so the fit never
//! shows it above twice those pixels whatever size the preview was made at. Textures are kept per
//! UI thread for the images the frame painted, so a frame uploads only an image it has not drawn.
//! While no change is under way the next photo's preview, once the show has made it in the other
//! pane, is uploaded too ([`preload`]): a change's first frame then finds both its photos' textures
//! and uploads nothing large (M11.29). Every other texture is let go at the end of the frame, so at
//! most the photo shown and the next one (or the two photos of a change) hold a texture. A
//! preview's pixels are converted to the texture's format on a worker thread, so the frame that
//! uploads it only hands them over (M12.24); until they are there the thumbnail stands in.
//!
//! Pan and Zoom moves each photo toward the focus the engine's [`Frame`] gives for it (its largest
//! named face, else its largest face, else its centre: `Frame::focus`, `Frame::outgoing_focus`,
//! M13.23); [`slides`] tells a test what the last frame drew.
//!
//! **A video** (M15.27): while the engine plays one in place of the photo shown ([`Frame::clip`]),
//! its frame due ([`clip_picture`]) is drawn in the photo's place (turned as the video's own edits
//! turn it), its poster in the pane standing in until the first frame is there; a video Dalica
//! cannot play is drawn as the video tile (its pane has no picture). The frame's texture is one
//! kept for the video, its pixels replaced when a new frame is due ([`clip_shown`] tells a test
//! which frame the last paint drew).

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use dalica_core::PhotoId;
use dalica_create::slideshow::{self as create, Size, Slide};
use dalica_edit::{Dihedral, Recipe};
use dalica_engine::cmd::slideshow::{ClipView, Frame, clip_picture};
use dalica_engine::preview::PANES;
use dalica_engine::{Session, Thumb};
use dalica_image::formats::video::frames::Frame as VideoFrame;
use dalica_image::{Orientation, Rgba8};
use dalica_ui_kit::widgets;
use egui::{Rect, TextureHandle, Ui};

use crate::app::App;

/// The caption's automation id.
pub const CAPTION_ID: &str = "text:slideshow.caption";
/// The most images whose textures are kept at once (the two photos of a change, or the photo shown
/// and the next one, and a stand-in): a cap over the per-frame letting go.
const KEPT: usize = 4;
/// The most previews converted on workers at once (the photo shown and the next one).
const PREPARING: usize = 2;

/// A texture of an image the session handed, and that image.
struct Tex {
    source: Weak<Rgba8>,
    handle: TextureHandle,
    /// The photo it shows, and whether the image is its preview (else a stand-in thumbnail).
    photo: PhotoId,
    preview: bool,
    /// Drawn or preloaded by the frame being painted.
    used: bool,
}

/// A preview being converted to the texture's pixel format on a worker thread (M12.24): the
/// image, and where the converted pixels arrive.
struct Pending {
    source: Weak<Rgba8>,
    rx: Receiver<Arc<egui::ColorImage>>,
    /// The converted pixels, arrived but held for a later frame (one preview upload per frame).
    ready: Option<Arc<egui::ColorImage>>,
}

/// How many previews' textures were prepared on a worker, and how many on the UI thread (when a
/// worker could not start). For tests and inspection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Prepared {
    pub on_worker: u64,
    pub on_ui_thread: u64,
}

thread_local! {
    static TEXTURES: RefCell<Vec<Tex>> = const { RefCell::new(Vec::new()) };
    static PENDING: RefCell<Vec<Pending>> = const { RefCell::new(Vec::new()) };
    /// Whether the frame being painted has uploaded a preview already.
    static UPLOADED: Cell<bool> = const { Cell::new(false) };
    static PREPARED: Cell<Prepared> = const { Cell::new(Prepared { on_worker: 0, on_ui_thread: 0 }) };
    /// The photos the last frame placed, and their slides (at most two).
    static SLIDES: RefCell<Vec<(PhotoId, Slide)>> = const { RefCell::new(Vec::new()) };
    /// The video's texture: the frame it holds, as turned, and its handle.
    static CLIP_TEX: RefCell<Option<(Weak<VideoFrame>, Dihedral, TextureHandle)>> = const { RefCell::new(None) };
    /// The video and the number of its frame the last paint drew.
    static CLIP_SHOWN: Cell<Option<(PhotoId, usize)>> = const { Cell::new(None) };
}

/// An image to paint: photo `photo`'s preview (`preview`) or a stand-in thumbnail.
struct Image {
    photo: PhotoId,
    preview: bool,
    pixels: Arc<Rgba8>,
}

/// `image`'s size when its pixels are whole (four bytes each), else `None`.
fn dims(px: &Rgba8) -> Option<[usize; 2]> {
    let dims = [px.width as usize, px.height as usize];
    let whole = dims[0] != 0 && dims[1] != 0 && dims[0].saturating_mul(dims[1]).saturating_mul(4) == px.data.len();
    whole.then_some(dims)
}

/// The kept texture of `image`, marked as used by this frame and made the most recently used.
fn kept_texture(image: &Image) -> Option<egui::TextureId> {
    TEXTURES.with(|kept| {
        let mut kept = kept.borrow_mut();
        kept.retain(|t| t.source.strong_count() > 0);
        let at = kept.iter().position(|t| t.source.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &image.pixels)))?;
        // The most recently used goes last.
        let mut t = kept.remove(at);
        t.used = true;
        let id = t.handle.id();
        kept.push(t);
        Some(id)
    })
}

/// Uploads `pixels` as `image`'s texture and keeps it (the least recently used kept one let go
/// past [`KEPT`]). The pixels are only handed over: no copy or conversion here.
fn upload(ctx: &egui::Context, image: &Image, pixels: Arc<egui::ColorImage>) -> egui::TextureId {
    let handle = ctx.load_texture("slideshow", pixels, egui::TextureOptions::LINEAR);
    let id = handle.id();
    TEXTURES.with(|kept| {
        let mut kept = kept.borrow_mut();
        if kept.len() >= KEPT {
            kept.remove(0);
        }
        kept.push(Tex { source: Arc::downgrade(&image.pixels), handle, photo: image.photo, preview: image.preview, used: true });
    });
    id
}

/// The texture of `image`, marked as used by this frame: the one kept for it, else a new one.
/// A thumbnail (small) is converted and uploaded here. A preview's pixels are converted to the
/// texture's format on a worker ([`prepare`]): `None` until they are there, and the frame they
/// arrive in only uploads them, one preview per frame. `None` too when the image is malformed.
fn texture(ctx: &egui::Context, image: &Image) -> Option<egui::TextureId> {
    if let Some(id) = kept_texture(image) {
        return Some(id);
    }
    let dims = dims(&image.pixels)?;
    if !image.preview {
        let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.pixels.data);
        return Some(upload(ctx, image, Arc::new(pixels)));
    }
    prepare(ctx, image, dims)
}

/// The texture of preview `image` (of `dims`) once a worker has converted its pixels: polls the
/// conversion under way, or starts one. A worker that cannot start leaves the conversion to this
/// thread, as before M12.24.
fn prepare(ctx: &egui::Context, image: &Image, dims: [usize; 2]) -> Option<egui::TextureId> {
    let busy = UPLOADED.with(Cell::get);
    let arrived = PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        let at = pending.iter().position(|p| p.source.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &image.pixels)))?;
        let p = pending.get_mut(at)?;
        if p.ready.is_none() {
            match p.rx.try_recv() {
                Ok(pixels) => p.ready = Some(pixels),
                Err(TryRecvError::Empty) => return Some(None),
                // The worker ended without an answer: try again.
                Err(TryRecvError::Disconnected) => {
                    pending.remove(at);
                    return None;
                }
            }
        }
        if busy {
            return Some(None);
        }
        Some(pending.remove(at).ready)
    });
    match arrived {
        Some(Some(pixels)) => {
            count(|p| p.on_worker += 1);
            UPLOADED.with(|u| u.set(true));
            return Some(upload(ctx, image, pixels));
        }
        Some(None) => return None,
        None => {}
    }
    let (tx, rx) = mpsc::channel();
    let source = Arc::clone(&image.pixels);
    let wake = ctx.clone();
    let started = std::thread::Builder::new().name("dalica-slideshow-texture".into()).spawn(move || {
        let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &source.data);
        if tx.send(Arc::new(pixels)).is_ok() {
            wake.request_repaint();
        }
    });
    if started.is_err() {
        count(|p| p.on_ui_thread += 1);
        let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.pixels.data);
        return Some(upload(ctx, image, Arc::new(pixels)));
    }
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        if pending.len() >= PREPARING {
            pending.remove(0);
        }
        pending.push(Pending { source: Arc::downgrade(&image.pixels), rx, ready: None });
    });
    None
}

fn count(f: impl FnOnce(&mut Prepared)) {
    PREPARED.with(|c| {
        let mut p = c.get();
        f(&mut p);
        c.set(p);
    });
}

/// Starts a frame's painting: no kept texture is used yet; conversions of previews the session
/// let go are dropped.
fn begin_frame() {
    TEXTURES.with(|kept| kept.borrow_mut().iter_mut().for_each(|t| t.used = false));
    SLIDES.with(|slides| slides.borrow_mut().clear());
    PENDING.with(|pending| pending.borrow_mut().retain(|p| p.source.strong_count() > 0));
    UPLOADED.with(|u| u.set(false));
}

/// Ends a frame's painting: the textures it neither drew nor preloaded are let go.
fn end_frame() {
    TEXTURES.with(|kept| kept.borrow_mut().retain(|t| t.used));
}

/// Lets go of every texture kept and every conversion under way (the show ended).
pub fn release() {
    TEXTURES.with(|kept| kept.borrow_mut().clear());
    PENDING.with(|pending| pending.borrow_mut().clear());
    CLIP_TEX.with(|c| c.borrow_mut().take());
    CLIP_SHOWN.with(|c| c.set(None));
}

/// The video and the number of its frame the last paint on this UI thread drew, while it drew one.
pub fn clip_shown() -> Option<(PhotoId, usize)> {
    CLIP_SHOWN.with(Cell::get)
}

/// `image` (a video frame: every alpha 255) turned by `turn`, as a colour image; `None` when its
/// pixels are not whole.
fn turned(image: &Rgba8, turn: Dihedral) -> Option<egui::ColorImage> {
    let [w, h] = dims(image)?;
    if turn == Dihedral::IDENTITY {
        return Some(egui::ColorImage::from_rgba_premultiplied([w, h], &image.data));
    }
    let (ow, oh) = turn.display_size(image.width, image.height);
    let (ow, oh) = (ow as usize, oh as usize);
    let back = turn.inverse();
    let mut out = Vec::with_capacity(ow.saturating_mul(oh).saturating_mul(4));
    for y in 0..oh {
        for x in 0..ow {
            let (u, v) = crate::panels::thumbnails::turn_point(back, (x as f32 + 0.5) / ow as f32, (y as f32 + 0.5) / oh as f32);
            let sx = ((u * w as f32) as usize).min(w - 1);
            let sy = ((v * h as f32) as usize).min(h - 1);
            let at = (sy * w + sx) * 4;
            out.extend_from_slice(image.data.get(at..at + 4).unwrap_or(&[0, 0, 0, 255]));
        }
    }
    Some(egui::ColorImage::from_rgba_premultiplied([ow, oh], &out))
}

/// The texture of the video's frame `picture` turned by `turn`: the kept one when it holds that
/// frame, else the kept one with its pixels replaced (or a new one).
fn clip_texture(ctx: &egui::Context, picture: &Arc<VideoFrame>, turn: Dihedral) -> Option<egui::TextureId> {
    CLIP_TEX.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((source, t, handle)) = c.as_ref()
            && *t == turn
            && std::ptr::eq(source.as_ptr(), Arc::as_ptr(picture))
        {
            return Some(handle.id());
        }
        let pixels = turned(&picture.image, turn)?;
        let handle = match c.take() {
            Some((_, _, mut handle)) => {
                handle.set(pixels, egui::TextureOptions::LINEAR);
                handle
            }
            None => ctx.load_texture("slideshow:video", pixels, egui::TextureOptions::LINEAR),
        };
        let id = handle.id();
        *c = Some((Arc::downgrade(picture), turn, handle));
        Some(id)
    })
}

/// The video `clip` placed as the `index`th photo of the show, from its frame due; `None` before
/// its first frame (the poster in its pane stands in).
fn clip_photo(ui: &Ui, session: &Session, clip: &ClipView, index: usize) -> Option<Photo> {
    let picture = clip_picture(clip.key)?;
    let turn = crate::panels::thumbnails::recipe_orientation(&session.library().info(clip.photo).edits);
    let texture = clip_texture(ui.ctx(), &picture, turn)?;
    let size = turn.display_size(picture.image.width, picture.image.height);
    let slide = Slide { size, focus: create::Point::CENTRE, zoom: create::pan_zoom_amount(index), motion: 0.0 };
    SLIDES.with(|slides| slides.borrow_mut().push((clip.photo, slide)));
    CLIP_SHOWN.with(|c| c.set(Some((clip.photo, picture.index))));
    Some(Photo { texture, slide })
}

/// The images holding a texture on this UI thread now, least recently used first: each photo and
/// whether the image is its preview (else a stand-in thumbnail). For tests and inspection: the
/// photo shown and the next one, or a change's two photos.
pub fn kept() -> Vec<(PhotoId, bool)> {
    TEXTURES.with(|kept| kept.borrow().iter().filter(|t| t.source.strong_count() > 0).map(|t| (t.photo, t.preview)).collect())
}

/// The photos the last frame on this UI thread placed and their slides: the photo shown first,
/// then the outgoing one while a change is under way (M13.23).
pub fn slides() -> Vec<(PhotoId, Slide)> {
    SLIDES.with(|slides| slides.borrow().clone())
}

/// Where this UI thread's preview textures were prepared so far (M12.24).
pub fn prepared() -> Prepared {
    PREPARED.with(Cell::get)
}

/// Waits up to `timeout` for the conversions under way on this UI thread to land; the frames after
/// upload them as before. Answers whether every one did. For tests and benches whose virtual clock
/// runs ahead of the workers (as `Session::wait_preview` for the previews): the show itself never
/// waits for a conversion.
pub fn wait_prepared(timeout: Duration) -> bool {
    let until = Instant::now() + timeout;
    PENDING.with(|pending| {
        let mut all = true;
        for p in pending.borrow_mut().iter_mut().filter(|p| p.ready.is_none()) {
            match p.rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(pixels) => p.ready = Some(pixels),
                Err(RecvTimeoutError::Timeout) => all = false,
                // The worker ended without an answer: the next frame starts it again.
                Err(RecvTimeoutError::Disconnected) => {}
            }
        }
        all
    })
}

/// Photo `id`'s preview in `pane` when it is there, with its recipe (what the preview shows).
fn preview_of(session: &Session, id: PhotoId, pane: usize) -> Option<(Image, Vec<String>)> {
    let p = session.preview_in(pane).filter(|p| p.photo == id)?;
    Some((Image { photo: id, preview: true, pixels: Arc::clone(&p.image) }, p.recipe.clone()))
}

/// The best thumbnail of photo `id` in memory for a long side of `need` device pixels, with the
/// photo's recipe: what stands in while its preview is not there or not prepared.
fn stand_in(session: &mut Session, id: PhotoId, need: u32) -> Option<(Image, Vec<String>)> {
    let recipe = session.library().info(id).edits.clone();
    match session.thumbnail(id, need) {
        Thumb::Image { image, .. } => Some((Image { photo: id, preview: false, pixels: image }, recipe)),
        Thumb::Sketch(_) | Thumb::NotScanned => None,
    }
}

/// While no change is under way, prepares and uploads the next photo's preview once the show has
/// made it in a pane other than the shown photo's, so the change to it uploads nothing large. A
/// show stepped to a photo whose preview is not there (or not prepared) yet paints its thumbnail
/// meanwhile, as before.
fn preload(ctx: &egui::Context, session: &Session, frame: &Frame) {
    if frame.view.change.is_some() || frame.outgoing.is_some() {
        return;
    }
    let Some((shown, pane)) = frame.photo else { return };
    for other in (0..PANES).filter(|p| *p != pane) {
        // Only the photo the pane is open on now (not a frame it held before).
        let Some(p) = session.preview_in(other).filter(|p| p.photo != shown && session.preview_photo_in(other) == Some(p.photo)) else {
            continue;
        };
        let _ = texture(ctx, &Image { photo: p.photo, preview: true, pixels: Arc::clone(&p.image) });
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

/// Photo `id` from `pane` (or its stand-in), placed as the `index`th photo of the show, `motion`
/// through its display time, Pan and Zoom moving it toward `focus`.
fn photo(
    ui: &Ui,
    session: &mut Session,
    (id, pane): (PhotoId, usize),
    index: usize,
    (motion, focus): (f32, create::Point),
    need: u32,
) -> Option<Photo> {
    let preview = preview_of(session, id, pane).and_then(|(image, recipe)| texture(ui.ctx(), &image).map(|t| (image, recipe, t)));
    let (image, recipe, texture) = match preview {
        Some(p) => p,
        None => {
            let (image, recipe) = stand_in(session, id, need)?;
            let texture = texture(ui.ctx(), &image)?;
            (image, recipe, texture)
        }
    };
    let size = shown_size(session, id, &recipe, &image.pixels);
    let slide = Slide { size, focus, zoom: create::pan_zoom_amount(index), motion };
    SLIDES.with(|slides| slides.borrow_mut().push((id, slide)));
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
    begin_frame();
    CLIP_SHOWN.with(|c| c.set(None));
    let clip = frame.clip.filter(|c| frame.photo.is_some_and(|(p, _)| p == c.photo));
    if clip.is_none() {
        CLIP_TEX.with(|c| c.borrow_mut().take());
    }
    let incoming = clip
        .zip(v.index)
        .and_then(|(c, i)| clip_photo(ui, session, &c, i))
        .or_else(|| frame.photo.zip(v.index).and_then(|(p, i)| photo(ui, session, p, i, (v.motion, frame.focus), need)));
    let change = v.change.filter(|_| frame.outgoing.is_some());
    let outgoing = frame.outgoing.zip(change).and_then(|(p, (i, _))| photo(ui, session, p, i, (v.outgoing_motion, frame.outgoing_focus), need));
    preload(ui.ctx(), session, frame);
    end_frame();
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
    // A video with nothing to show (one Dalica cannot play): its tile, fitted as the video.
    if incoming.is_none()
        && let Some((id, _)) = frame.photo
        && session.is_video(id)
    {
        let size = session.library().video_facts(id).map(|f| f.display_size()).filter(|(w, h)| *w > 0 && *h > 0).unwrap_or((1920, 1080));
        let r = create::fit(size, screen, ppp);
        let tile = Rect::from_min_size(rect.min + egui::vec2(r.x, r.y), egui::vec2(r.width, r.height));
        crate::panels::light_table::cell::video_tile(&painter, &t, tile);
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
