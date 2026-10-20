//! Photos dragged out of the window to other applications (`ui-layout.md` §5.5, `FILE-DRAGOUT`):
//! a drag of photos whose pointer leaves the window, with the button still held, is handed to the
//! OS as a drag of the photos' files, behind the [`DragOut`] service the app implements (as it
//! implements `Picker`); headless sessions and tests plug in a recording one, or none.
//!
//! - **What leaves.** The photos' files on disk ([`Outgoing::paths`], the photos of offline
//!   folders left out), offered as a copy: Dalica never deletes the source (a target that moves the
//!   file is seen as any outside move). Unsaved edits stay in Dalica: when a photo carries them,
//!   the pill says so ([`UNSAVED_NOTE`], painted under it as `drag:note` while the drag is over
//!   nothing, and drawn into [`Outgoing::image`]).
//! - **Once.** The OS gets the drag the first frame the pointer is outside the window, once per
//!   drag. A drag that comes back in stays an inside drag: released over a target inside, it drops
//!   there as if it had never left (and the OS's drag is cancelled); released outside, nothing
//!   inside fires. Esc cancels both.
//! - **The image.** The drag pill as the window shows it (the first photo's thumbnail, the count),
//!   painted on the CPU by ui-kit's rasterizer into an RGBA image ([`pill_image`]); the OS shows it
//!   beside the pointer where it can.
//! - **Refusal.** An OS with no way to drag files out (`DragOut::start` returning why) leaves the
//!   drag inside, and the status line says why and how to copy the files instead.
//!
//! **Boundary.** `crate::drag::frame` calls [`leave`] and [`finish`]; the app installs its
//! service with [`install`] once its window's context exists.

use std::path::PathBuf;
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_image::Rgba8;
use dalica_ui_kit::widgets::{self, DragPill, PillThumb, Target};
use dalica_ui_kit::{Image, Rasterizer, Role, Tokens, fonts};
use egui::{Color32, ColorImage, Context, CornerRadius, Id, LayerId, Order, Pos2, RawInput, Rect, Stroke, StrokeKind, TextureOptions, pos2, vec2};

/// What the drag pill says when a photo dragged out has unsaved edits.
pub const UNSAVED_NOTE: &str = "Unsaved edits stay in Dalica";
/// The note's automation id (role `other`, painted, labelled with its text).
pub const NOTE_ID: &str = "drag:note";
/// The note's height and the gap above it.
const NOTE_H: f32 = 22.0;
const NOTE_GAP: f32 = 4.0;
/// The note's room either side of its text.
const NOTE_PAD: f32 = 10.0;
/// The most points either side of the offscreen pill image (the pill's widest, its note).
const IMAGE_W: f32 = 400.0;
const IMAGE_H: f32 = 72.0;
/// The scale the pill image is painted at, at most.
const MAX_IMAGE_SCALE: f32 = 3.0;

/// A drag handed to the OS.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    /// The photos' files, absolute, in the drag's order, each once.
    pub paths: Vec<PathBuf>,
    /// What the pill says ("3 photos").
    pub pill: String,
    /// [`UNSAVED_NOTE`] when a photo carries unsaved edits.
    pub note: Option<String>,
    /// The pill as an RGBA image (premultiplied, as the window paints it), at `scale` pixels per
    /// point; `None` when it could not be painted.
    pub image: Option<Image>,
    pub scale: f32,
}

/// The OS's drag of files, as the app implements it per OS. Called on the UI thread: an
/// implementation that waits for the OS does so on its own thread or in the app's window code.
pub trait DragOut: Send + Sync {
    /// Starts the OS's drag of `out` (the pointer has just left the window with the button held).
    /// `Err` says why this OS cannot, for the status line.
    fn start(&self, out: Outgoing) -> Result<(), String>;
    /// The drag ended inside the window (dropped there, or Esc): the OS's drag, if still running,
    /// ends with nothing dropped.
    fn cancel(&self);
}

fn service_id() -> Id {
    Id::new("dalica-drag/out-service")
}

fn state_id() -> Id {
    Id::new("dalica-drag/out-state")
}

/// Installs the app's drag-out service in `ctx` (the window does at start; tests their recording
/// one). Without one, a drag that leaves the window stays inside.
pub fn install(ctx: &Context, service: Arc<dyn DragOut>) {
    ctx.data_mut(|d| d.insert_temp(service_id(), Service(service)));
}

#[derive(Clone)]
struct Service(Arc<dyn DragOut>);

fn service(ctx: &Context) -> Option<Arc<dyn DragOut>> {
    ctx.data(|d| d.get_temp::<Service>(service_id())).map(|s| s.0)
}

/// Where this drag is with the OS: handed (once), or refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutState {
    pub handed: bool,
    pub refused: bool,
}

/// This drag's state with the OS.
pub fn state(ctx: &Context) -> OutState {
    ctx.data(|d| d.get_temp::<OutState>(state_id())).unwrap_or_default()
}

fn set_state(ctx: &Context, s: OutState) {
    ctx.data_mut(|d| d.insert_temp(state_id(), s));
}

/// Whether `pos` (the pointer, `None` once it has gone) is outside the window.
pub fn outside(ctx: &Context, pos: Option<Pos2>) -> bool {
    pos.is_none_or(|p| !ctx.content_rect().contains(p))
}

/// The files of `ids` on disk, each once, leaving out photos the library no longer has and those of
/// offline folders.
pub fn paths(session: &Session, ids: &[PhotoId]) -> Vec<PathBuf> {
    let lib = session.library();
    let mut out: Vec<PathBuf> = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(photo) = lib.photo(*id) else { continue };
        if session.is_folder_offline(photo.folder) {
            continue;
        }
        let Some(folder) = lib.folder(photo.folder) else { continue };
        let path = folder.path.join(&photo.name);
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// [`UNSAVED_NOTE`] when one of `ids` has unsaved edits.
pub fn note(session: &Session, ids: &[PhotoId]) -> Option<&'static str> {
    let lib = session.library();
    ids.iter().any(|id| lib.photo(*id).is_some() && lib.has_unsaved_edits(*id)).then_some(UNSAVED_NOTE)
}

/// The pointer left the window during a drag of `ids`: hands the drag to the OS, once per drag.
/// Returns the status line's message when the OS refused (once per drag).
pub fn leave(ctx: &Context, session: &mut Session, tokens: &Tokens, ids: &[PhotoId], pill: &str) -> Option<String> {
    let s = state(ctx);
    if s.handed || s.refused {
        return None;
    }
    let service = service(ctx)?;
    let paths = paths(session, ids);
    if paths.is_empty() {
        set_state(ctx, OutState { handed: false, refused: true });
        return Some("These photos' folders are offline: there are no files to drag out.".into());
    }
    let note = note(session, ids).map(str::to_string);
    let scale = ctx.pixels_per_point().clamp(1.0, MAX_IMAGE_SCALE);
    let thumb = ids.first().and_then(|id| match session.thumbnail(*id, (widgets::PILL_THUMB * scale).ceil() as u32) {
        dalica_engine::Thumb::Image { image, .. } => Some(image),
        _ => None,
    });
    let image = pill_image(tokens, ids.len(), thumb.as_deref(), note.as_deref(), scale);
    let out = Outgoing { paths, pill: pill.to_string(), note, image, scale };
    match service.start(out) {
        Ok(()) => {
            set_state(ctx, OutState { handed: true, refused: false });
            None
        }
        Err(why) => {
            set_state(ctx, OutState { handed: false, refused: true });
            Some(why)
        }
    }
}

/// The drag ended: `inside` (dropped in the window, or Esc) cancels the OS's drag if it was handed
/// one. Forgets the drag's state either way.
pub fn finish(ctx: &Context, inside: bool) {
    let s = state(ctx);
    if s.handed
        && inside
        && let Some(service) = service(ctx)
    {
        service.cancel();
    }
    ctx.data_mut(|d| d.remove::<OutState>(state_id()));
}

/// Paints [`UNSAVED_NOTE`] (or `note`) under the pill `pill` on `ctx`'s tooltip layer and
/// registers it as [`NOTE_ID`]; returns its rect.
pub fn paint_note(ctx: &Context, t: &Tokens, pill: Rect, note: &str) -> Rect {
    if !pill.is_positive() {
        return Rect::NOTHING;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new(NOTE_ID)));
    let galley = painter.layout_no_wrap(note.to_string(), fonts::regular(t.small), t.text_dim);
    let width = (galley.size().x + 2.0 * NOTE_PAD).ceil();
    let rect = Rect::from_min_size(pos2(pill.min.x, pill.max.y + NOTE_GAP), vec2(width, NOTE_H));
    let radius = CornerRadius::same(t.toast_radius);
    painter.rect(rect, radius, t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let at = pos2(rect.min.x + NOTE_PAD, rect.center().y - galley.size().y / 2.0).round();
    painter.galley(at, galley, t.text_dim);
    widgets::register(ctx, NOTE_ID, Role::Other, note, Target::Painted { rect, clip: ctx.content_rect() }, true);
    rect
}

/// The drag pill of `count` photos over nothing (the first photo's `thumb`, `note` under it) as an
/// RGBA image at `scale` pixels per point, painted as the window paints it: ui-kit's `drag_pill` on
/// a context of its own, rasterized on the CPU and cut to the pill. `None` when it cannot be
/// painted.
pub fn pill_image(t: &Tokens, count: usize, thumb: Option<&Rgba8>, note: Option<&str>, scale: f32) -> Option<Image> {
    let scale = if scale.is_finite() { scale.clamp(1.0, MAX_IMAGE_SCALE) } else { 1.0 };
    let ctx = Context::default();
    fonts::install(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(IMAGE_W, IMAGE_H));
    let input = || {
        let mut raw = RawInput { screen_rect: Some(screen), ..RawInput::default() };
        if let Some(v) = raw.viewports.get_mut(&raw.viewport_id) {
            v.native_pixels_per_point = Some(scale);
        }
        raw
    };
    let mut raster = Rasterizer::new();
    // The fonts take effect from the second pass on.
    let mut first = ctx.run_ui(input(), |_| {});
    raster.set_textures(&mut first.textures_delta);
    let texture = thumb.and_then(|img| {
        let size = [usize::try_from(img.width).ok()?, usize::try_from(img.height).ok()?];
        let want = size[0].checked_mul(size[1])?.checked_mul(4)?;
        if want == 0 || img.data.len() != want {
            return None;
        }
        let image = ColorImage::from_rgba_unmultiplied(size, &img.data);
        Some((ctx.load_texture("dalica-drag/pill-thumb", image, TextureOptions::LINEAR), vec2(img.width as f32, img.height as f32)))
    });
    let mut rect = Rect::NOTHING;
    let mut out = ctx.run_ui(input(), |ui| {
        let ctx = ui.ctx();
        // The pill's place for a pointer at the image's corner, less its offset.
        let pointer = screen.min + vec2(widgets::PILL_MARGIN, widgets::PILL_MARGIN) - widgets::PILL_OFFSET;
        let pill = DragPill {
            op: widgets::DragOp::None,
            count,
            target: None,
            thumb: texture.as_ref().map(|(h, size)| PillThumb { texture: h.id(), size: *size }),
        };
        rect = widgets::drag_pill(ctx, t, "drag:pill", pointer, &pill);
        if let Some(note) = note {
            rect = rect.union(paint_note(ctx, t, rect, note));
        }
    });
    raster.set_textures(&mut out.textures_delta);
    if !rect.is_positive() {
        return None;
    }
    let ppp = out.pixels_per_point;
    let (w, h) = ((IMAGE_W * ppp).round() as u32, (IMAGE_H * ppp).round() as u32);
    let primitives = ctx.tessellate(out.shapes, ppp);
    let full = raster.render(&primitives, ppp, w, h, Color32::TRANSPARENT).ok()?;
    crop(&full, rect, ppp)
}

/// The part of `image` (painted at `ppp` pixels per point) under `rect`, in whole pixels.
fn crop(image: &Image, rect: Rect, ppp: f32) -> Option<Image> {
    let px = |v: f32, max: u32| ((v * ppp).round().max(0.0) as u32).min(max);
    let (x0, y0) = (px(rect.min.x, image.width), px(rect.min.y, image.height));
    let (x1, y1) = (px(rect.max.x, image.width), px(rect.max.y, image.height));
    let (w, h) = (x1.checked_sub(x0)?, y1.checked_sub(y0)?);
    if w == 0 || h == 0 {
        return None;
    }
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in y0..y1 {
        let start = (y as usize * image.width as usize + x0 as usize) * 4;
        rgba.extend_from_slice(image.rgba.get(start..start + w as usize * 4)?);
    }
    Some(Image { width: w, height: h, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_image_is_the_pill_and_grows_a_line_for_the_note() {
        let t = Tokens::default();
        let plain = pill_image(&t, 3, None, None, 1.0).expect("a pill image");
        assert_eq!(plain.height, widgets::PILL_H as u32);
        assert!(plain.width > 40 && plain.width <= widgets::PILL_MAX_W as u32, "{plain:?}");
        assert_eq!(plain.rgba.len(), (plain.width * plain.height * 4) as usize);
        // The pill's fill is opaque chrome in its middle.
        let [.., a] = plain.pixel(plain.width / 2, plain.height / 2).unwrap();
        assert_eq!(a, 255);
        let noted = pill_image(&t, 3, None, Some(UNSAVED_NOTE), 1.0).unwrap();
        assert_eq!(noted.height, (widgets::PILL_H + NOTE_GAP + NOTE_H) as u32);
        // Twice the pixels at twice the scale; a hostile scale is clamped.
        let double = pill_image(&t, 3, None, None, 2.0).unwrap();
        assert_eq!(double.height, plain.height * 2);
        assert!(pill_image(&t, 3, None, None, f32::NAN).is_some());
        // A thumbnail whose bytes do not match its size is left out, not trusted.
        let bad = Rgba8 { width: 4, height: 4, data: vec![0; 7] };
        assert!(pill_image(&t, 1, Some(&bad), None, 1.0).is_some());
    }

    #[test]
    fn a_crop_never_reads_past_the_image() {
        let img = Image { width: 4, height: 2, rgba: (0..32).collect() };
        let c = crop(&img, Rect::from_min_max(pos2(1.0, 1.0), pos2(9.0, 9.0)), 1.0).unwrap();
        assert_eq!((c.width, c.height), (3, 1));
        assert_eq!(c.rgba, (20..32).collect::<Vec<u8>>());
        assert!(crop(&img, Rect::from_min_max(pos2(5.0, 0.0), pos2(9.0, 9.0)), 1.0).is_none());
    }
}
