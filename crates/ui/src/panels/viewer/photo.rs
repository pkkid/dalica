//! The photo of the viewer (`viewing.md` §3.1): fitted in the photo area, or zoomed (the view
//! state's `viewer.zoom`, 1 is 100 % of the photo's pixels) and panned.
//!
//! **Pixels.** A photo the library holds is drawn from the session's preview (the edit view's
//! pane 0, `Session::open_preview`, at the photo area's box in device pixels), with its edits; its
//! neighbours in the folder are prefetched, so stepping does not wait for a decode. Zoomed past the
//! preview, the visible window is asked for at full resolution once the view stands still
//! (`Session::set_preview_window`), as the edit view does. A photo of a folder the library does
//! not hold is drawn as a frame of its shape with [`OUTSIDE`]: the engine renders only the
//! library's photos so far.
//!
//! **Zoom and pan.** Zoom changes made outside the photo (+, −, 1, the bar) jump, keeping the point
//! at the centre of the area; the wheel zooms about the pointer. Dragging a photo larger than the
//! area pans it, its edges never pulled inside. A new photo starts centred. The fitted scale is
//! kept per thread for the zoom commands ([`fitted`]).

use std::cell::Cell;
use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_edit::{Recipe, Region};
use dalica_image::{Orientation, Rgba8};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Color32, CursorIcon, Id, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2, vec2};

use super::listing::Photo;
use crate::app::App;
use crate::commands::zoom as maths;
use crate::state::Zoom;

/// What a photo outside the library shows instead of its pixels.
pub const OUTSIDE: &str = "Dalica shows the pixels of photos in its library; this folder is not in it.";
/// How long the photo rests between frames while its preview is on its way.
pub const PREVIEW_POLL: std::time::Duration = std::time::Duration::from_millis(33);

thread_local! {
    /// The fitted scale of the photo the viewer last drew on this thread (the UI thread).
    static FITTED: Cell<Option<f32>> = const { Cell::new(None) };
}

/// The fitted scale the viewer last laid out (device pixels per photo pixel).
pub fn fitted() -> Option<f32> {
    FITTED.with(Cell::get)
}

/// A texture of an image the session handed, and that image.
#[derive(Clone)]
struct Tex {
    source: Weak<Rgba8>,
    handle: TextureHandle,
}

impl Tex {
    fn of(&self, image: &Arc<Rgba8>) -> bool {
        self.source.upgrade().is_some_and(|s| Arc::ptr_eq(&s, image))
    }
}

fn upload(ctx: &egui::Context, name: &str, image: &Arc<Rgba8>) -> Option<Tex> {
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return None;
    }
    let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
    Some(Tex { source: Arc::downgrade(image), handle: ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR) })
}

/// What the photo keeps between frames (in the context's data).
#[derive(Clone, Default)]
struct State {
    /// The photo shown (its path), and the image point at the area's centre.
    shown: Option<std::path::PathBuf>,
    centre: Option<Pos2>,
    preview: Option<Tex>,
    refined: Option<Tex>,
    asked: Option<Region>,
    prefetched: Option<PhotoId>,
}

/// What the photo laid out this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    /// Where the photo is drawn, in screen points (not clipped).
    pub image: Rect,
    /// Its size in pixels as it shows (after its rotations and crop).
    pub size: (f32, f32),
    pub fit: f32,
    pub scale: f32,
    /// Larger than the area on some side: dragging pans.
    pub pannable: bool,
    /// The preview is on its way.
    pub waiting: bool,
}

/// The input the photo acts on this frame.
pub struct Input {
    /// Dragged by this much (screen points) with the primary button.
    pub drag: Vec2,
    /// The wheel zooms by this factor about this point.
    pub wheel: Option<(f32, Pos2)>,
    pub hovered: bool,
}

/// The photo's size in pixels as it shows: the library's size turned and cut by its recipe; the
/// listed size outside the library.
fn photo_size(app: &App, photo: &Photo) -> (f32, f32) {
    if let Some((info, p)) = photo.id.and_then(|id| app.session().and_then(|s| Some((s.library().info(id), s.library().photo(id)?)))) {
        let recipe = Recipe::parse(&info.edits).unwrap_or_default();
        let g = dalica_edit::Geometry::new(&recipe, Orientation::default(), p.width, p.height);
        if let Ok((w, h)) = g.display_size(1.0) {
            return (w as f32, h as f32);
        }
    }
    (photo.width as f32, photo.height as f32)
}

/// Draws `photo` in `area` and runs its zoom and pan; `neighbours` are prefetched.
pub fn show(app: &mut App, ui: &mut Ui, area: Rect, photo: &Photo, neighbours: &[PhotoId], input: &Input) -> Shown {
    let ctx = ui.ctx().clone();
    let t = *app.tokens();
    let ppp = ctx.pixels_per_point();
    let id = Id::new("viewer.photo");
    let mut c = ctx.data_mut(|d| d.remove_temp::<State>(id)).unwrap_or_default();
    if c.shown.as_ref() != Some(&photo.path) {
        c = State { shown: Some(photo.path.clone()), prefetched: c.prefetched, ..State::default() };
    }
    let px = |v: f32| if v.is_finite() { (v * ppp).round().clamp(1.0, 16_384.0) as u32 } else { 1 };
    if let (Some(pid), Some(s)) = (photo.id, app.session_mut()) {
        let fit_px = (px(area.width()), px(area.height()));
        if s.preview_photo() == Some(pid) {
            let _ = s.set_preview_fit(fit_px);
        } else if s.open_preview(pid, fit_px).is_ok() {
            c.asked = None;
        }
        if c.prefetched != Some(pid) {
            c.prefetched = Some(pid);
            s.prefetch_previews(neighbours);
        }
    }
    let size = photo_size(app, photo);
    let fit = maths::fit_scale(size, (area.width() * ppp, area.height() * ppp));
    FITTED.with(|f| f.set(Some(fit)));
    let zoom = app.state().viewer.map_or(Zoom::Fit, |v| v.zoom);
    let drawn = |s: f32| maths::drawn_size(size, s, ppp);
    let mut scale = maths::effective(zoom, fit);
    let mut centre = maths::clamp(c.centre.unwrap_or(pos2(0.5, 0.5)), drawn(scale), area.size());
    if let Some((factor, at)) = input.wheel {
        let to = maths::step(scale, factor, fit);
        let target = maths::effective(to, fit);
        centre = maths::clamp(maths::anchor(centre, area.center(), at, size, ppp, scale, target), drawn(target), area.size());
        scale = target;
        if let Some(v) = app.state_mut().viewer.as_mut() {
            v.zoom = to;
        }
    }
    let d = drawn(scale);
    let pannable = d.x > area.width() + 0.5 || d.y > area.height() + 0.5;
    if pannable && input.drag != Vec2::ZERO && d.x > 0.0 && d.y > 0.0 {
        centre = maths::clamp(centre - vec2(input.drag.x / d.x, input.drag.y / d.y), d, area.size());
    }
    if pannable && input.hovered {
        ctx.set_cursor_icon(if input.drag != Vec2::ZERO { CursorIcon::Grabbing } else { CursorIcon::Grab });
    }
    c.centre = Some(centre);
    let image = Rect::from_center_size(area.center() + (pos2(0.5, 0.5) - centre) * d, d);
    let painter = ui.painter_at(area);

    let preview = photo.id.and_then(|pid| app.session().and_then(|s| s.preview()).filter(|p| p.photo == pid).cloned());
    let mut waiting = false;
    match (&preview, photo.id) {
        (Some(p), _) => {
            if c.preview.as_ref().is_none_or(|tex| !tex.of(&p.image)) {
                c.preview = upload(&ctx, "viewer:preview", &p.image);
            }
            if let Some(tex) = &c.preview {
                painter.image(tex.handle.id(), image, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        (None, Some(_)) => {
            c.preview = None;
            match app.session().and_then(|s| s.preview_failure()) {
                Some(why) => stand_in(&painter, &t, image, &crate::panels::edit::canvas::message(why)),
                None => waiting = true,
            }
        }
        (None, None) => stand_in(&painter, &t, image, OUTSIDE),
    }

    // Zoomed past the preview: the visible window at full resolution, once the view stands still.
    let need = preview.as_ref().is_some_and(|p| size.0 * scale > p.image.width as f32 * 1.01 || size.1 * scale > p.image.height as f32 * 1.01);
    if need {
        if input.drag == Vec2::ZERO {
            let w = crate::panels::edit::canvas::window(image, area, scale, ppp, size);
            if w != c.asked
                && let Some(s) = app.session_mut()
                && s.set_preview_window(w).is_ok()
            {
                c.asked = w;
            }
        }
    } else if c.asked.take().is_some()
        && let Some(s) = app.session_mut()
    {
        let _ = s.set_preview_window(None);
    }
    let refined =
        photo.id.and_then(|pid| app.session().and_then(|s| s.preview_refined()).filter(|r| r.photo == pid && Some(r.region) == c.asked).cloned());
    match &refined {
        Some(r) if (r.region.scale - f64::from(scale)).abs() <= 1e-6 * f64::from(scale) => {
            if c.refined.as_ref().is_none_or(|tex| !tex.of(&r.image)) {
                c.refined = upload(&ctx, "viewer:refined", &r.image);
            }
            if let Some(tex) = &c.refined {
                let min = image.min + vec2(r.region.x as f32, r.region.y as f32) / ppp;
                let rect = Rect::from_min_size(min, vec2(r.image.width as f32, r.image.height as f32) / ppp);
                painter.image(tex.handle.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        Some(_) => {}
        None => c.refined = None,
    }
    if app.session().is_some_and(|s| s.preview_pending() > 0) {
        waiting = true;
        ctx.request_repaint_after(PREVIEW_POLL);
    }
    ctx.data_mut(|d| d.insert_temp(id, c));
    Shown { image, size, fit, scale, pannable, waiting }
}

/// A photo whose pixels cannot be drawn: a frame of its shape with `why` in it.
fn stand_in(painter: &egui::Painter, t: &Tokens, image: Rect, why: &str) {
    painter.rect_stroke(image, 0, Stroke::new(1.0, t.overlay_disabled), StrokeKind::Inside);
    let wrap = (image.width() - 24.0).max(120.0);
    let galley = painter.layout(why.to_owned(), fonts::regular(t.body), t.overlay_icon, wrap);
    let at = image.center() - galley.size() / 2.0;
    painter.galley(at.round(), galley, t.overlay_icon);
}
