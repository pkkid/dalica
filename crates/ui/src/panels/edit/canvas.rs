//! The photo area (`editing.md` §2–§3): the open photo from the session's preview, fitted with
//! [`PADDING`] all round on the canvas background with its drop shadow, zoom and pan, the zoom
//! read-out ([`super::zoom`]) and the navigator ([`super::navigator`]) while zoomed.
//!
//! **Pixels.** The canvas opens the photo in the session's preview (`Session::open_preview`) at the
//! photo area's box in device pixels, and prefetches its section neighbours. The preview is
//! rendered at the largest power-of-two scale of its source that covers the box, so it is drawn
//! fitted (scaled down to the box). Zoomed past it, the visible window is asked for at full
//! resolution (`Session::set_preview_window`) once the view stands still; until it arrives the
//! scaled preview is drawn, and no frame waits for it. While a preview is on its way and nothing
//! animates, the canvas draws again every [`PREVIEW_POLL`] rather than at once. Leaving the edit
//! view closes the preview ([`release`], from the bottom area, the one part drawn in both views).
//!
//! **Zoom** (`commands::zoom`): the edit state's `zoom` is `Fit` or a scale (1 is 100 %). Fit and
//! 1:1 set from outside the canvas (the bottom area's buttons, `ui.set`) animate [`ANIMATION`]
//! seconds (they jump when Options ▸ General ▸ Use animations is off, [`crate::scale::animations`],
//! and so do a click's Fit and a hold's 100 %); other outside changes jump, about the pointer when it is over the photo area (Ctrl+=,
//! Ctrl+−), else about the centre. The wheel zooms ×1.1 a notch about the pointer and a pinch
//! zooms (`dalica_ui_kit::input::wheel`), snapping to 100 % when crossing it; a trackpad scroll
//! pans.
//!
//! **Pointer** (no tool open; an open tool owns the clicks and drags): a press held [`HOLD`]
//! seconds on the fitted photo zooms to 100 % at that point and stays there; a click on a zoomed
//! photo returns to fit; a double-click returns to the library. With "Single-click to leave the
//! edit view" one click leaves and the press-and-hold zoom is off. Dragging a photo larger than the
//! area pans it (hand cursor), bounded so no edge comes inside the area, and glides on when let go
//! while moving (`dalica_ui_kit::input::flick`); with animations off the glide's whole distance is
//! moved in the frame it starts ([`glide_rest`]).
//!
//! **Compare** (`editing.md` §4, [`super::compare`]): while comparing, each side of the photo area
//! is drawn by the same code from its own preview pane (`Session::open_preview_in`), the focused
//! side always from pane 0 (the pane the tools hand their candidates to). The sides share the edit
//! state's zoom and one centre (synced pan); the wheel acts on the side under the pointer, a click
//! on the side without the focus gives it the focus, and only the focused side takes the hold, the
//! clicks and the double-click, and shows the zoom read-out and the navigator. The focused side is
//! `canvas:edit` (and [`view`]), the other `canvas:edit.other` ([`other_view`]).
//!
//! **What tests and agents see.** `canvas:edit` is the photo area; [`view`] is the last frame's
//! layout (the photo's rect, the scale, whether it is refining). The fitted scale is also kept per
//! thread for the zoom commands ([`fitted`]): it is derived from the layout, which commands do not
//! see.

use std::cell::Cell;
use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_edit::{Recipe, Region};
use dalica_engine::UpdateKind;
use dalica_image::{Orientation, Rgba8};
use dalica_ui_kit::input::{flick, wheel};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Color32, CursorIcon, Id, Pos2, Rect, Sense, TextureHandle, Ui, Vec2, pos2, vec2};
use serde_json::json;

use super::compare::{self, Pane};
use super::{navigator, zoom};
use crate::app::App;
use crate::commands::edit_view;
use crate::commands::zoom as maths;
use crate::perf::{self, Counter};
use crate::selection::Table;
use crate::state::Zoom;
use crate::tl;

/// The photo area's automation id; while comparing, the focused side's.
pub const ID: &str = "canvas:edit";
/// While comparing, the side without the focus (a click gives it the focus).
pub const OTHER_ID: &str = "canvas:edit.other";
/// The room left around the fitted photo, in points.
pub const PADDING: f32 = 24.0;
/// How long Fit and 1:1 animate, in seconds (`design-system.md` §8).
pub const ANIMATION: f64 = 0.22;
/// How long a press on the fitted photo is held before it zooms to 100 %, in seconds.
pub const HOLD: f64 = 0.3;
/// How far the pointer may move during the hold, in points.
pub const HOLD_SLOP: f32 = 4.0;
/// How long the canvas rests between frames while a preview is on its way and nothing moves, so
/// the wait does not spin a CPU the preview's workers need. Two 60 Hz frames: egui takes one
/// predicted frame time off a delayed repaint, which leaves at least 16 ms between frames.
pub const PREVIEW_POLL: std::time::Duration = std::time::Duration::from_millis(33);
/// The message drawn when the photo cannot be shown (English; [`cannot_show`] in the language in
/// effect).
pub const CANNOT_SHOW: &str = "This photo cannot be shown.";

/// [`CANNOT_SHOW`] in the language in effect.
pub fn cannot_show() -> String {
    tl!("This photo cannot be shown.")
}

/// Why a photo or a clip cannot be shown (or plays without sound), in the language in effect: a
/// codec Dalica cannot play by the codec's name ("This video can't be played (ProRes)", "This clip
/// plays without sound (AC-3 sound can't be played)"), any other reason looked up whole by its
/// English.
pub fn reason(why: &str) -> String {
    let codec = |prefix: &str, suffix: &str| {
        why.strip_prefix(prefix).and_then(|rest| rest.strip_suffix(suffix)).filter(|c| !c.is_empty() && !c.contains(['(', ')']))
    };
    if let Some(c) = codec(&format!("{} (", dalica_engine::video::CANNOT_PLAY), ")") {
        tl!("This video can't be played ({codec})", codec = c)
    } else if let Some(c) = codec("This clip plays without sound (", " sound can't be played)") {
        tl!("This clip plays without sound ({codec} sound can't be played)", codec = c)
    } else {
        crate::i18n::tr(why)
    }
}

/// The image point (0 … 1 of the photo) shown at the area's centre when fitted.
const CENTRE: Pos2 = pos2(0.5, 0.5);

/// What the canvas laid out last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub photo: PhotoId,
    /// The photo area, in screen points.
    pub area: Rect,
    /// Where the photo is drawn, in screen points (not clipped to the area).
    pub image: Rect,
    /// The photo's size in pixels as it shows (after its crop and rotations).
    pub size: (f32, f32),
    /// The fitted scale and the scale shown (device pixels per photo pixel; 1 is 100 %).
    pub fit: f32,
    pub scale: f32,
    /// Shown larger than fitted.
    pub zoomed: bool,
    /// Larger than the area on some side: drag pans.
    pub pannable: bool,
    /// A refined window has been asked for and is not drawn yet.
    pub refining: bool,
    /// The refined window is drawn over the scaled preview.
    pub refined: bool,
    /// Fit or 1:1 is animating.
    pub animating: bool,
}

thread_local! {
    /// The fitted scale of the photo the canvas last drew on this thread (the UI thread).
    static FITTED: Cell<Option<(PhotoId, f32)>> = const { Cell::new(None) };
    /// The next zoom change is a step (Ctrl+=, Ctrl+−, [`note_step`]).
    static STEPPED: Cell<bool> = const { Cell::new(false) };
}

/// Marks the zoom change the canvas sees next as a step (Ctrl+=, Ctrl+−): it jumps about the
/// pointer, also when it lands on 100 % (`editing.md` §3), where 1:1 would animate about the centre.
pub fn note_step() {
    STEPPED.with(|s| s.set(true));
}

/// The fitted scale the canvas last laid out for `photo`, if it is the photo it last drew.
pub fn fitted(photo: PhotoId) -> Option<f32> {
    FITTED.with(Cell::get).filter(|(p, _)| *p == photo).map(|(_, f)| f)
}

fn view_id() -> Id {
    Id::new(ID).with("view")
}

/// The canvas's layout of the last frame that drew it (`None` outside the edit view); while
/// comparing, the focused side's.
pub fn view(ctx: &egui::Context) -> Option<View> {
    ctx.data(|d| d.get_temp::<Option<View>>(view_id())).flatten()
}

fn other_view_id() -> Id {
    Id::new(OTHER_ID).with("view")
}

/// While comparing, the layout of the side without the focus in the last frame (`None` otherwise).
pub fn other_view(ctx: &egui::Context) -> Option<View> {
    ctx.data(|d| d.get_temp::<Option<View>>(other_view_id())).flatten()
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

/// Fit or 1:1 on its way: where the view was when it started.
#[derive(Clone, Copy, Debug)]
struct Anim {
    from_scale: f32,
    from_centre: Pos2,
    start: f64,
}

/// A press on the fitted photo that may become a hold.
#[derive(Clone, Copy, Debug)]
struct Press {
    at: Pos2,
    time: f64,
}

/// A photo's size as it shows, with the recipe and the library's size it was worked out from.
type Sized = (Vec<String>, (u32, u32), (f32, f32));

/// What the canvas keeps between frames (in the context's data).
#[derive(Clone, Default)]
struct Canvas {
    photo: Option<PhotoId>,
    /// The image point shown at the area's centre (the target while animating).
    centre: Option<Pos2>,
    /// The edit state's zoom as the canvas last applied it.
    seen: Option<Zoom>,
    anim: Option<Anim>,
    preview: Option<Tex>,
    refined: Option<Tex>,
    /// The window asked for.
    asked: Option<Region>,
    press: Option<Press>,
    /// A hold zoomed: the click its release makes is not a click.
    swallow: bool,
    /// The neighbours were prefetched for this photo (once: the section is looked up then).
    prefetched: Option<PhotoId>,
    /// The photo's size as it shows, for the recipe it was worked out from.
    size: Option<Sized>,
    /// The preview pane the side shows (the focused side's is 0).
    pane: usize,
}

/// The most steps of a glide [`glide_rest`] adds up (a glide at the fastest speed comes to rest in
/// fewer).
const GLIDE_STEPS: usize = 64;

/// The ease-out of the zoom animation at `p` (0 … 1).
fn ease(p: f32) -> f32 {
    let q = 1.0 - p.clamp(0.0, 1.0);
    1.0 - q * q * q
}

/// The scale and centre shown now: the animation's (unless animations are off), else the target's.
fn shown(c: &Canvas, now: f64, target_scale: f32, animate: bool) -> (f32, Pos2, bool) {
    let target_centre = c.centre.unwrap_or(CENTRE);
    match c.anim {
        Some(a) if animate && now - a.start < ANIMATION && a.from_scale > 0.0 && target_scale > 0.0 => {
            let e = ease(((now - a.start) / ANIMATION) as f32);
            let scale = (a.from_scale.ln() + (target_scale.ln() - a.from_scale.ln()) * e).exp();
            let centre = a.from_centre.lerp(target_centre, e);
            if scale.is_finite() { (scale, centre, true) } else { (target_scale, target_centre, false) }
        }
        _ => (target_scale, target_centre, false),
    }
}

/// The photo's size in pixels as it shows: its displayed size turned and cut by `recipe`; the
/// preview's own size when the library does not know the photo's.
fn photo_size(c: &mut Canvas, app: &App, photo: PhotoId, recipe: &[String], preview: Option<&Rgba8>) -> (f32, f32) {
    let dims = app.session().and_then(|s| s.library().photo(photo)).map_or((0, 0), |p| (p.width, p.height));
    if let Some((r, d, size)) = &c.size
        && r.as_slice() == recipe
        && *d == dims
    {
        return *size;
    }
    let size = if dims.0 > 0 && dims.1 > 0 {
        let recipe_parsed = Recipe::parse(recipe).unwrap_or_default();
        let g = dalica_edit::Geometry::new(&recipe_parsed, Orientation::default(), dims.0, dims.1);
        g.display_size(1.0).map(|(w, h)| (w as f32, h as f32)).unwrap_or((dims.0 as f32, dims.1 as f32))
    } else if let Some(p) = preview {
        (p.width as f32, p.height as f32)
    } else {
        (0.0, 0.0)
    };
    c.size = Some((recipe.to_vec(), dims, size));
    size
}

/// Uploads `image` as a canvas texture; `None` when it is malformed.
fn upload(ctx: &egui::Context, name: &str, image: &Arc<Rgba8>) -> Option<Tex> {
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return None;
    }
    let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
    let handle = ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR);
    Some(Tex { source: Arc::downgrade(image), handle })
}

/// Closes the session's preview once the edit view is left: its jobs are cancelled and the whole
/// decode of a zoomed view let go. Called every frame the library shows (by the bottom area).
pub fn release(app: &mut App) {
    if edit_view::open_photo(app).is_none()
        && let Some(s) = app.session_mut()
        && s.preview_photo().is_some()
    {
        s.close_preview();
    }
}

/// Opens `photo` in preview pane `pane` at `fit` (the box in device pixels), or keeps the box up
/// to date, showing it as `shows` says; the focused side prefetches its neighbours once per photo.
fn sync_preview(app: &mut App, c: &mut Canvas, p: &Pane, fit: (u32, u32)) {
    let Some(session) = app.session_mut() else { return };
    let (pane, photo) = (p.pane, p.photo);
    if session.preview_photo_in(pane) == Some(photo) {
        let _ = session.set_preview_fit_in(pane, fit);
    } else {
        let _ = session.open_preview_in(pane, photo, fit);
        c.asked = None;
    }
    if session.preview_shows(pane).is_some_and(|s| s != p.shows) {
        let _ = session.set_preview_shows(pane, p.shows);
    }
    if p.focused && c.prefetched != Some(photo) {
        c.prefetched = Some(photo);
        let neighbours: Vec<PhotoId> = Table::of(app)
            .and_then(|t| edit_view::place_in(&t, photo))
            .map(|p| {
                let before = p.index.checked_sub(1).and_then(|i| p.photos.get(i));
                let after = p.index.checked_add(1).and_then(|i| p.photos.get(i));
                after.into_iter().chain(before).copied().collect()
            })
            .unwrap_or_default();
        if let Some(s) = app.session_mut() {
            s.prefetch_previews(&neighbours);
        }
    }
}

/// Reports the previews that arrived since the last frame to the perf counters.
fn report_updates(app: &mut App, ctx: &egui::Context) {
    let Some(session) = app.session_mut() else { return };
    for u in session.take_preview_updates() {
        let counter = match u.kind {
            UpdateKind::Draft => Counter::PreviewDraftMicros,
            UpdateKind::Final => Counter::PreviewFinalMicros,
            UpdateKind::Refined => Counter::PreviewRefinedMicros,
        };
        let micros = if u.ms.is_finite() && u.ms > 0.0 { (u.ms * 1000.0).min(u64::MAX as f64) as u64 } else { 0 };
        perf::count(ctx, counter, micros);
    }
}

/// The window of the photo shown in `area` at `scale`, in the displayed picture's pixels at that
/// scale (what `Session::set_preview_window` takes); `None` when nothing of it is visible.
pub fn window(image: Rect, area: Rect, scale: f32, ppp: f32, size: (f32, f32)) -> Option<Region> {
    let visible = image.intersect(area);
    if !visible.is_positive() || !(scale.is_finite() && scale > 0.0 && ppp.is_finite() && ppp > 0.0) {
        return None;
    }
    let full = (size.0 * scale, size.1 * scale);
    let px = |v: f32, max: f32| if v.is_finite() { v.clamp(0.0, max.max(0.0)) } else { 0.0 };
    let x0 = px(((visible.min.x - image.min.x) * ppp).floor(), full.0);
    let y0 = px(((visible.min.y - image.min.y) * ppp).floor(), full.1);
    let x1 = px(((visible.max.x - image.min.x) * ppp).ceil(), full.0.ceil());
    let y1 = px(((visible.max.y - image.min.y) * ppp).ceil(), full.1.ceil());
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w >= 1.0 && h >= 1.0) {
        return None;
    }
    Some(Region { scale: f64::from(scale), x: x0 as u32, y: y0 as u32, width: w as u32, height: h as u32 })
}

/// Paints and runs the photo area in `ui`'s rect (the `canvas` region): one photo, or the two
/// sides of a compare mode ([`compare::panes`]), each from its preview pane, zoom and pan synced.
pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if edit_view::open_photo(app).is_none() {
        ctx.data_mut(|d| {
            d.insert_temp(view_id(), None::<View>);
            d.insert_temp(other_view_id(), None::<View>);
        });
        return;
    }
    // Ctrl+= and Ctrl+− are the photo's zoom here, never egui's zoom of the whole window.
    if ctx.options(|o| o.zoom_with_keyboard) {
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
    }
    let panes = compare::panes(app, ui.max_rect());
    if let Some(s) = app.session_mut() {
        // The focused side is always pane 0 (the pane the tools hand their candidates to).
        if s.preview_focus() != 0 {
            let _ = s.set_preview_focus(0);
        }
        if panes.len() < 2 && s.preview_photo_in(1).is_some() {
            s.close_preview_in(1);
        }
    }
    report_updates(app, &ctx);
    let frame = Frame { stepped: STEPPED.with(|s| s.replace(false)), wheel: wheel::read(&ctx) };
    if panes.len() < 2 {
        ctx.data_mut(|d| {
            d.insert_temp(other_view_id(), None::<View>);
            d.remove_temp::<Pos2>(shared_centre_id());
        });
    }
    for p in &panes {
        if !side(app, ui, &ctx, p, &frame) {
            // The edit view was left.
            ctx.data_mut(|d| {
                d.insert_temp(view_id(), None::<View>);
                d.insert_temp(other_view_id(), None::<View>);
            });
            return;
        }
    }
}

/// What every side of one frame shares: whether the zoom change seen this frame is a step, and the
/// wheel's gesture.
struct Frame {
    stepped: bool,
    wheel: Option<wheel::Wheel>,
}

fn shared_centre_id() -> Id {
    Id::new(ID).with("centre")
}

/// Paints and runs one side `p`; `false` when the pointer left the edit view.
fn side(app: &mut App, ui: &mut Ui, ctx: &egui::Context, p: &Pane, frame: &Frame) -> bool {
    let photo = p.photo;
    let t = *app.tokens();
    let area = p.area;
    let ppp = ctx.pixels_per_point();
    let now = ctx.input(|i| i.time);
    let id = Id::new(ID).with(p.key);
    let mut c = ctx.data_mut(|d| d.remove_temp::<Canvas>(id)).unwrap_or_default();
    if c.photo != Some(photo) {
        c = Canvas { photo: Some(photo), ..Canvas::default() };
    }
    if c.pane != p.pane {
        c.pane = p.pane;
        c.asked = None;
    }
    let animate = crate::scale::animations(ctx);
    if !animate || c.anim.is_some_and(|a| (now - a.start).partial_cmp(&ANIMATION) != Some(std::cmp::Ordering::Less)) {
        c.anim = None;
    }
    // Comparing: both sides show the same place of their photos (synced pan).
    if p.comparing
        && c.anim.is_none()
        && let Some(shared) = ctx.data(|d| d.get_temp::<Pos2>(shared_centre_id()))
    {
        c.centre = Some(shared);
    }

    let boxed = area.shrink(PADDING);
    let px = |v: f32| if v.is_finite() { (v * ppp).round().clamp(1.0, 16_384.0) as u32 } else { 1 };
    sync_preview(app, &mut c, p, (px(boxed.width()), px(boxed.height())));

    let preview = app.session().and_then(|s| s.preview_in(p.pane)).filter(|v| v.photo == photo).cloned();
    let recipe: Vec<String> = match &preview {
        Some(v) => v.recipe.clone(),
        None => app.session().map(|s| s.library().info(photo).edits.clone()).unwrap_or_default(),
    };
    let size = photo_size(&mut c, app, photo, &recipe, preview.as_ref().map(|v| v.image.as_ref()));
    let fit = maths::fit_scale(size, (boxed.width() * ppp, boxed.height() * ppp));
    if p.focused {
        FITTED.with(|f| f.set(Some((photo, fit))));
    }

    // The focused side answers to the photo area's own id (the tools read its response).
    let response = ui.interact(area, Id::new(if p.focused { ID } else { OTHER_ID }), Sense::click_and_drag());
    let pointer = response.hover_pos();
    let tool_open = p.focused && app.state().edit.tool.is_some();
    let drawn = |s: f32| maths::drawn_size(size, s, ppp);
    let held = |centre: Pos2, s: f32| maths::clamp(centre, drawn(s), area.size());

    // The zoom as last applied, and what is shown of it now.
    let seen = *c.seen.get_or_insert(app.state().edit.zoom);
    let (mut scale, mut centre, _) = shown(&c, now, maths::effective(seen, fit), animate);

    // A change made outside the canvas: Fit and 1:1 animate, the others jump about the pointer.
    let zoom = app.state().edit.zoom;
    if zoom != seen {
        let target = maths::effective(zoom, fit);
        match zoom {
            Zoom::Fit => {
                c.anim = Some(Anim { from_scale: scale, from_centre: centre, start: now });
                c.centre = Some(CENTRE);
            }
            Zoom::Scale(1.0) if !frame.stepped => {
                c.anim = Some(Anim { from_scale: scale, from_centre: centre, start: now });
                c.centre = Some(held(centre, target));
            }
            Zoom::Scale(_) => {
                c.anim = None;
                let about = pointer.unwrap_or(area.center());
                c.centre = Some(held(maths::anchor(centre, area.center(), about, size, ppp, scale, target), target));
            }
        }
        c.seen = Some(zoom);
        (scale, centre, _) = shown(&c, now, target, animate);
    }

    // The wheel and the pinch, over this side.
    if let (Some(w), Some(at)) = (frame.wheel, pointer) {
        match w {
            wheel::Wheel::Zoom(f) => {
                let to = maths::step(scale, f, fit);
                let target = maths::effective(to, fit);
                c.centre = Some(held(maths::anchor(centre, area.center(), at, size, ppp, scale, target), target));
                c.anim = None;
                c.seen = Some(to);
                app.state_mut().edit.zoom = to;
                (scale, centre) = (target, c.centre.unwrap_or(CENTRE));
            }
            wheel::Wheel::Pan(v) => {
                let d = drawn(scale);
                if d.x > 0.0 && d.y > 0.0 {
                    centre = held(centre - vec2(v.x / d.x, v.y / d.y), scale);
                    c.centre = Some(centre);
                }
            }
        }
    }

    let zoomed = maths::is_zoomed(app.state().edit.zoom, fit);
    let pannable = {
        let d = drawn(scale);
        d.x > area.width() + 0.5 || d.y > area.height() + 0.5
    };

    // Drag pans, and glides on when let go while moving.
    let panning = !tool_open && pannable && response.dragged_by(egui::PointerButton::Primary);
    let delta = if panning { response.drag_delta() } else { Vec2::ZERO };
    let glide = match flick::frame(ctx, id.with("flick"), panning, delta) {
        Some(first) if !animate => Some(first + glide_rest(ctx, id.with("flick"))),
        glide => glide,
    };
    let moved = delta + glide.unwrap_or(Vec2::ZERO);
    if moved != Vec2::ZERO && pannable {
        let d = drawn(scale);
        if d.x > 0.0 && d.y > 0.0 {
            centre = held(centre - vec2(moved.x / d.x, moved.y / d.y), scale);
            c.centre = Some(centre);
            c.anim = None;
        }
    }

    let image = Rect::from_center_size(area.center() + (pos2(0.5, 0.5) - centre) * drawn(scale), drawn(scale));

    // Clicks, the hold and the double-click (an open tool owns them); a click on the side without
    // the focus gives it the focus.
    let action = if !p.focused {
        if response.clicked() {
            let _ = app.run("edit.switchFocus", &json!({}));
        }
        Action::None
    } else if tool_open {
        Action::None
    } else {
        pointer_input(app, ctx, &response, &mut c, image, now, zoomed)
    };
    match action {
        Action::None => {}
        Action::Left => return false,
        Action::Fit => {
            app.state_mut().edit.zoom = Zoom::Fit;
            c.seen = Some(Zoom::Fit);
            c.anim = Some(Anim { from_scale: scale, from_centre: centre, start: now });
            c.centre = Some(CENTRE);
        }
        Action::Hold(at) => {
            let to = Zoom::Scale(1.0);
            let target = maths::effective(to, fit);
            app.state_mut().edit.zoom = to;
            c.seen = Some(to);
            c.anim = Some(Anim { from_scale: scale, from_centre: centre, start: now });
            c.centre = Some(held(maths::anchor(centre, area.center(), at, size, ppp, scale, target), target));
        }
    }
    if !tool_open && response.hovered() && pannable {
        ctx.set_cursor_icon(if response.dragged() { CursorIcon::Grabbing } else { CursorIcon::Grab });
    }

    // What is shown this frame (a click or a hold starts animating from here).
    let (scale, centre, animating) = shown(&c, now, maths::effective(c.seen.unwrap_or(Zoom::Fit), fit), animate);
    let image = Rect::from_center_size(area.center() + (pos2(0.5, 0.5) - centre) * drawn(scale), drawn(scale));

    // The pixels: the preview fitted, the refined window over it.
    let painter = ui.painter_at(area);
    if let Some(v) = &preview {
        if c.preview.as_ref().is_none_or(|tex| !tex.of(&v.image)) {
            c.preview = upload(ctx, &format!("edit:preview{}", p.key), &v.image);
        }
        if image.intersects(area) {
            if image.width() <= area.width() && image.height() <= area.height() {
                widgets::photo_shadow(&painter, image);
            }
            if let Some(tex) = &c.preview {
                painter.image(tex.handle.id(), image, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
    } else {
        c.preview = None;
    }
    let failure = if preview.is_none() { app.session().and_then(|s| s.preview_failure_in(p.pane)).map(str::to_owned) } else { None };
    if let Some(why) = &failure {
        failed(&painter, &t, area, why);
    }

    // Zoomed past the preview: the visible window at full resolution, asked for once the view
    // stands still.
    let need = preview.as_ref().is_some_and(|v| size.0 * scale > v.image.width as f32 * 1.01 || size.1 * scale > v.image.height as f32 * 1.01);
    let still = !animating && !panning && glide.is_none();
    if need {
        if still {
            let w = window(image, area, scale, ppp, size);
            if w != c.asked
                && let Some(s) = app.session_mut()
                && s.set_preview_window_in(p.pane, w).is_ok()
            {
                c.asked = w;
            }
        }
    } else if c.asked.is_some() {
        if let Some(s) = app.session_mut() {
            let _ = s.set_preview_window_in(p.pane, None);
        }
        c.asked = None;
    }
    let refined = app.session().and_then(|s| s.preview_refined_in(p.pane)).filter(|r| r.photo == photo && Some(r.region) == c.asked).cloned();
    let mut drew_refined = false;
    if let Some(r) = &refined
        && (r.region.scale - f64::from(scale)).abs() <= 1e-6 * f64::from(scale)
    {
        if c.refined.as_ref().is_none_or(|tex| !tex.of(&r.image)) {
            c.refined = upload(ctx, &format!("edit:refined{}", p.key), &r.image);
        }
        if let Some(tex) = &c.refined {
            let min = image.min + vec2(r.region.x as f32, r.region.y as f32) / ppp;
            let rect = Rect::from_min_size(min, vec2(r.image.width as f32, r.image.height as f32) / ppp);
            painter.image(tex.handle.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            drew_refined = true;
        }
    } else if refined.is_none() {
        c.refined = None;
    }
    // On its way while the session still works on it (a window that failed is not waited for).
    let pending = app.session().is_some_and(|s| s.preview_pending() > 0);
    let refining = c.asked.is_some() && !drew_refined && pending;
    if refining && preview.is_some() {
        perf::count(ctx, Counter::PreviewWaiting, 1);
    }

    // A photo that cannot be shown: what the area says is its label (screen readers, agents).
    let label = failure.as_deref().map_or_else(|| tl!("Photo"), message);
    widgets::register(ctx, if p.focused { ID } else { OTHER_ID }, Role::Other, &label, Target::Widget(&response), p.focused && p.comparing);
    let v = View { photo, area, image, size, fit, scale, zoomed, pannable, refining, refined: drew_refined, animating };
    if p.focused {
        zoom::show(ui, &t, &v);
        if zoomed && pannable {
            let tex = c.preview.as_ref().map(|tex| tex.handle.id());
            if let Some(to) = navigator::show(ui, &t, &v, tex) {
                c.centre = Some(held(to, scale));
                c.anim = None;
            }
        }
    }

    // Animating: draw again at once. Waiting for a preview: look again after [`PREVIEW_POLL`],
    // so the wait leaves the CPU to the workers rendering it (`ui.settle` counts the open photo's
    // preview as busy on its own: `Session::preview_busy`).
    if animating {
        ctx.request_repaint();
    } else if pending {
        ctx.request_repaint_after(PREVIEW_POLL);
    }
    if let Some(press) = c.press {
        let left = HOLD - (now - press.time);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.clamp(0.0, HOLD)));
    }
    let shared = c.centre;
    ctx.data_mut(|d| {
        d.insert_temp(id, c);
        d.insert_temp(if p.focused { view_id() } else { other_view_id() }, Some(v));
        if p.comparing
            && let Some(centre) = shared
        {
            d.insert_temp(shared_centre_id(), centre);
        }
    });
    true
}

/// What the pointer did to the photo this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    None,
    /// Back to the library (a double-click, or one click with the setting).
    Left,
    /// A click on the zoomed photo: back to fit.
    Fit,
    /// A press held on the fitted photo at this point: 100 % there.
    Hold(Pos2),
}

/// The clicks, the hold and the double-click on the photo `image`.
fn pointer_input(app: &mut App, ctx: &egui::Context, r: &egui::Response, c: &mut Canvas, image: Rect, now: f64, zoomed: bool) -> Action {
    let (pressed, down, released, at) =
        ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.interact_pos()));
    // Read only when the pointer acts (the settings are parsed for it).
    let single = |app: &App| app.session().is_some_and(|s| s.settings().edit().single_click_leaves);
    let mut action = Action::None;
    // A press on the fitted photo may become a hold.
    if pressed
        && r.hovered()
        && !zoomed
        && let Some(p) = at.filter(|p| image.contains(*p))
    {
        c.press = (!single(app)).then_some(Press { at: p, time: now });
    }
    if let Some(p) = c.press {
        let moved = at.is_some_and(|a| (a - p.at).length() > HOLD_SLOP);
        if !down || moved {
            c.press = None;
        } else if now - p.time >= HOLD {
            c.press = None;
            c.swallow = true;
            action = Action::Hold(p.at);
        }
    }
    if r.double_clicked() && !c.swallow {
        if !single(app) && app.run("view.library", &json!({})).is_ok() {
            action = Action::Left;
        }
    } else if r.clicked() && !c.swallow {
        if single(app) {
            if app.run("view.library", &json!({})).is_ok() {
                action = Action::Left;
            }
        } else if zoomed {
            action = Action::Fit;
        }
    }
    if released {
        c.swallow = false;
    }
    if action == Action::Left
        && let Some(s) = app.session_mut()
    {
        s.close_preview();
    }
    action
}

/// The rest of the glide of flick `id`, all at once (animations are off): its steps added up, and
/// the glide stopped.
fn glide_rest(ctx: &egui::Context, id: Id) -> Vec2 {
    let mut f = ctx.data(|d| d.get_temp::<flick::Flick>(id)).unwrap_or_default();
    let mut total = Vec2::ZERO;
    for _ in 0..GLIDE_STEPS {
        match f.step(flick::FLICK_MAX_DT) {
            Some(m) => total += m,
            None => break,
        }
    }
    f.stop();
    ctx.data_mut(|d| d.insert_temp(id, f));
    total
}

/// The widest the hint under the reason is drawn before it wraps, in points.
const HINT_WIDTH: f32 = 420.0;

/// The whole "cannot be shown" message as one text: [`CANNOT_SHOW`], the reason and, when its
/// format has one, the hint (`dalica_image::formats::hint`), one per line; each line in the
/// language in effect (the reason and the hint looked up whole by their English).
pub fn message(why: &str) -> String {
    let (title, reason) = (cannot_show(), reason(why));
    match dalica_image::formats::hint_for(why) {
        Some(hint) => format!("{title}\n{reason}\n{}", crate::i18n::tr(hint)),
        None => format!("{title}\n{reason}"),
    }
}

/// The "cannot be shown" message, with the reason under it and, when the format has one, the
/// hint under that, wrapped and centred.
fn failed(painter: &egui::Painter, t: &Tokens, area: Rect, why: &str) {
    let c = area.center();
    painter.text(c, Align2::CENTER_BOTTOM, cannot_show(), fonts::semibold(15.0), t.text_label);
    let hint = dalica_image::formats::hint_for(why).map(crate::i18n::tr);
    let why: String = reason(why).chars().take(200).collect();
    let reason = painter.text(c + vec2(0.0, 6.0), Align2::CENTER_TOP, why, fonts::regular(12.5), t.text_dim);
    if let Some(hint) = hint {
        let width = HINT_WIDTH.min(area.width() - 2.0 * PADDING).max(1.0);
        let mut job = egui::text::LayoutJob::simple(hint, fonts::regular(12.5), t.text_dim, width);
        job.halign = egui::Align::Center;
        painter.galley(pos2(c.x, reason.bottom() + 10.0), painter.layout_job(job), t.text_dim);
    }
}
