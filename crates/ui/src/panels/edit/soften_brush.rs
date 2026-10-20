//! Soften's brush and eraser (`editing.md` §8.4): while Soften's panel is open, the Paint and
//! Erase buttons and the brush size in the panel ([`rows`], the panel's extra rows under its
//! sliders, scrolling with them in a short panel), and on the photo the brush circle under the
//! pointer, a drag adding a stroke ([`crate::state::EffectDraft::strokes`], Paint or Erase as
//! [`crate::state::EffectDraft::erase`] says, at most [`MAX_STROKES`]). Run after the canvas
//! (`panels/edit.rs`), so the photo's part works with the edit panel hidden too.
//!
//! ```text
//! Soften
//!               Softness
//! ○─────────────────────────────
//!                 Fade
//! ○─────────────────────────────
//! [ 🖌 Paint    ] [ ⌫ Erase     ]               (Paint │ Erase)
//! Brush size                         32 px
//! ───────○──────────────────────
//! [Reset]                [Apply] [Cancel]
//! ```
//!
//! - **Strokes.** A press on the photo starts a stroke at the brush's radius and mode; the drag
//!   adds its points; the release ends it. Each stroke is a change of the panel's session: the
//!   draft takes it as it is drawn and the preview follows ([`effect_session::sync_preview`],
//!   rendered off the UI thread), so the painted area previews live. Points are read off the
//!   photo as it shows (after its crop, straighten, frames, rotations and flips) and kept in the
//!   stored frame through the photo's [`Geometry`]; a press on a frame (a Border) starts nothing
//!   and a drag over one adds nothing there. Apply stores the strokes in the entry; Cancel records
//!   nothing.
//! - **Thin and round** ([`extend`]): a point closer to the last one kept than [`THIN`] of the
//!   brush's radius is dropped, and every point is rounded to 1 / [`PRECISION`] of the photo, so
//!   a long stroke stays a few kilobytes of the entry. A stroke full at [`MAX_STROKE_POINTS`]
//!   goes on as a new stroke; a stroke that would bring the entry past [`ENTRY_BUDGET`] (under the
//!   library's 64 KiB per operation) or past [`MAX_STROKES`] strokes ends there, and the status
//!   line says to apply first.
//! - **Erase** takes Soften away where it was painted. With no paint stroke there is nothing to
//!   erase (the whole photo is softened until the first stroke, and an erase stroke alone would
//!   leave nothing softened), so Erase is off until the first paint stroke.
//! - **The brush** is a fraction of the stored photo's shorter side, [`BRUSH_LO`] … [`BRUSH_HI`]
//!   (the draft's range narrowed to what the entry stores); + (or =) and − make it 1.2 times
//!   larger or smaller, as Retouch's.
//! - **On the photo:** the brush circle at the pointer (a bar across it while erasing) and, while
//!   the pointer is over the photo or a stroke is drawn, the painted area as a light wash: the
//!   entry's own mask ([`Soften::coverage`]) at a small size, made on a worker thread whenever the
//!   strokes change and drawn turned as the photo shows.
//!
//! Ids: `button:effect.paint`, `button:effect.erase`, `slider:soften.brush`.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use dalica_core::PhotoId;
use dalica_edit::ops::soften::{MAX_RADIUS, MIN_RADIUS, Mode, Soften, Stroke};
use dalica_edit::{Dihedral, Geometry, Point, Recipe};
use dalica_image::Orientation;
use dalica_library::tags::MAX_OPERATION;
use dalica_ui_kit::icons::{EffectIcon, paint_effect};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, SliderSpec, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::epaint::{Mesh, Vertex};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, Response, Sense, Shape, Stroke as Line, StrokeKind, TextureHandle, Ui, UiBuilder,
    WidgetType, pos2, vec2,
};

use super::canvas::{self, View};
use super::effect_panel::controls::ROW_GAP;
use crate::app::App;
use crate::commands::{edit_view, effect_session, tools};
use crate::state::{EffectDraft, MAX_STROKE_POINTS, MAX_STROKES, SOFTEN_BRUSH_DEFAULT, SOFTEN_BRUSH_MAX, SOFTEN_BRUSH_MIN, SoftenStroke, ToolKind};
use crate::tl;

pub const PAINT_ID: &str = "button:effect.paint";
pub const ERASE_ID: &str = "button:effect.erase";
pub const BRUSH_ID: &str = "slider:soften.brush";
/// The slider's label and the buttons' tooltips (English; drawn through the catalogs).
pub const BRUSH_LABEL: &str = "Brush size";
pub const PAINT_TIP: &str = "Paint where Soften applies";
pub const ERASE_TIP: &str = "Erase Soften where it was painted";
/// The effect this brush serves, as the recipe names it.
pub const EFFECT: &str = "soften";
/// How much one + or − press scales the brush (as Retouch's).
pub const BRUSH_STEP: f32 = 1.2;
/// The brush range, a fraction of the stored photo's shorter side: the draft's range narrowed to
/// what the entry stores.
pub const BRUSH_LO: f32 = if SOFTEN_BRUSH_MIN > MIN_RADIUS as f32 { SOFTEN_BRUSH_MIN } else { MIN_RADIUS as f32 };
pub const BRUSH_HI: f32 = if SOFTEN_BRUSH_MAX < MAX_RADIUS as f32 { SOFTEN_BRUSH_MAX } else { MAX_RADIUS as f32 };
/// A point closer to the last one kept than this fraction of the brush's radius is dropped.
pub const THIN: f64 = 0.25;
/// Points are rounded to 1 / `PRECISION` of the photo (a pixel of a 10 000-pixel photo).
pub const PRECISION: f32 = 10_000.0;
/// The most bytes the entry may reach by drawing: well under the library's per-operation limit.
pub const ENTRY_BUDGET: usize = MAX_OPERATION - MAX_OPERATION / 16;
/// The most bytes one point adds to the entry (`,0.1234,0.5678`).
const POINT_BYTES: usize = 14;
/// The most bytes a stroke adds before its points (`;erase,0.1234`).
const STROKE_BYTES: usize = 16;
/// The longer side of the painted-area wash's mask, in its pixels.
pub const MASK_EDGE: f64 = 256.0;
/// The wash's opacity where the mask is full.
const WASH: f32 = 0.35;
/// The gap between the mode buttons.
const MODE_GAP: f32 = 6.0;

/// A stroke being drawn: the photo, the stroke's index in the draft.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Live {
    photo: PhotoId,
    index: usize,
}

/// What the wash's mask is made from: the photo, its size, its recipe and the strokes.
#[derive(Debug, Clone, PartialEq)]
struct MaskKey {
    photo: PhotoId,
    dims: (u32, u32),
    recipe: Vec<String>,
    strokes: Vec<SoftenStroke>,
}

/// A mask made: its size (before the recipe's rotations and flips) and coverage, row-major.
struct Mask {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

/// The wash's mask on its way.
struct Job {
    key: MaskKey,
    rx: Receiver<Option<Mask>>,
}

/// What the brush keeps on the UI thread outside the edit state.
#[derive(Default)]
struct Local {
    live: Option<Live>,
    /// The entry's bytes when the live stroke began, plus what its points added (an upper bound).
    bytes: usize,
    job: Option<Job>,
    /// The wash shown: what it was made from, its texture and the turn it is drawn with.
    wash: Option<(MaskKey, TextureHandle, Dihedral)>,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

fn local<R>(f: impl FnOnce(&mut Local) -> R) -> R {
    LOCAL.with(|l| f(&mut l.borrow_mut()))
}

/// `p` rounded to 1 / [`PRECISION`] and kept inside the photo (`NaN` to the centre).
pub fn round_point(p: [f32; 2]) -> [f32; 2] {
    p.map(|v| if v.is_finite() { ((v * PRECISION).round() / PRECISION).clamp(0.0, 1.0) } else { 0.5 })
}

/// Adds `p` (stored frame, normalised) to `stroke` when it is at least [`THIN`] of the brush's
/// radius from the last point kept (`aspect`: the stored photo's width and height over its shorter
/// side, so the distance is in the radius's unit) and the stroke has room; rounded
/// ([`round_point`]). Whether it was added.
pub fn extend(stroke: &mut SoftenStroke, p: [f32; 2], aspect: (f64, f64)) -> bool {
    if stroke.points.len() >= MAX_STROKE_POINTS {
        return false;
    }
    let p = round_point(p);
    if let Some(last) = stroke.points.last() {
        let dx = f64::from(p[0] - last[0]) * aspect.0;
        let dy = f64::from(p[1] - last[1]) * aspect.1;
        let d = (dx * dx + dy * dy).sqrt();
        if d.is_nan() || d < THIN * f64::from(stroke.radius) {
            return false;
        }
    }
    stroke.points.push(p);
    true
}

/// The stored photo's width and height over its shorter side.
fn aspect(dims: (u32, u32)) -> (f64, f64) {
    let (w, h) = (f64::from(dims.0.max(1)), f64::from(dims.1.max(1)));
    let s = w.min(h);
    (w / s, h / s)
}

/// The open photo and its Soften draft, while Soften's panel is open.
fn soften_draft(app: &App) -> Option<(PhotoId, EffectDraft)> {
    let e = &app.state().edit;
    match (e.tool, &e.effect) {
        (Some(ToolKind::Effect), Some(d)) if d.name == EFFECT => Some((edit_view::open_photo(app)?, d.clone())),
        _ => None,
    }
}

/// The photo's stored size (after its EXIF orientation).
fn dims(app: &App, photo: PhotoId) -> Option<(u32, u32)> {
    let p = app.session()?.library().photo(photo)?;
    (p.width > 0 && p.height > 0).then_some((p.width, p.height))
}

/// The photo's geometry as the edit view shows it with the panel open: its recipe (or the
/// variant's) before the new entry, which changes no geometry.
fn geometry(app: &App, photo: PhotoId, dims: (u32, u32)) -> Geometry {
    let recipe = Recipe::parse(tools::recipe(app, photo)).unwrap_or_default();
    Geometry::new(&recipe, Orientation::default(), dims.0, dims.1)
}

/// The stored-frame point under the displayed point `at` (normalised to the picture as it shows);
/// `None` off the photo (on a frame).
fn to_stored(g: &Geometry, at: [f32; 2]) -> Option<[f32; 2]> {
    let q = g.to_stored(Point::new(f64::from(at[0]), f64::from(at[1])))?;
    let inside = |v: f64| v.is_finite() && (-1e-6..=1.0 + 1e-6).contains(&v);
    (inside(q.x) && inside(q.y)).then(|| [q.x.clamp(0.0, 1.0) as f32, q.y.clamp(0.0, 1.0) as f32])
}

/// Where the stored-frame point `at` shows on the screen, with the photo drawn at `view`.
fn to_screen(g: &Geometry, view: &View, at: [f32; 2]) -> Option<Pos2> {
    let d = g.to_display(Point::new(f64::from(at[0]), f64::from(at[1])))?;
    let s = pos2(view.image.min.x + d.x as f32 * view.image.width(), view.image.min.y + d.y as f32 * view.image.height());
    (s.x.is_finite() && s.y.is_finite()).then_some(s)
}

/// The displayed point (normalised) under screen point `p`, when it is on the visible picture.
fn displayed_at(view: &View, p: Pos2) -> Option<[f32; 2]> {
    let r = view.image;
    let visible = r.intersect(view.area);
    (visible.contains(p) && r.width() > 0.0 && r.height() > 0.0).then(|| [(p.x - r.min.x) / r.width(), (p.y - r.min.y) / r.height()])
}

/// The radius on the screen of a brush of `radius` (of the shorter side) at the stored point `at`.
fn screen_radius(g: &Geometry, view: &View, dims: (u32, u32), at: [f32; 2], radius: f32) -> Option<f32> {
    let (ax, _) = aspect(dims);
    let a = to_screen(g, view, at)?;
    let b = to_screen(g, view, [at[0] + (f64::from(radius) / ax) as f32, at[1]])?;
    let r = (b - a).length();
    r.is_finite().then_some(r)
}

/// Sets the draft and hands the preview its candidate.
fn set_draft(app: &mut App, d: EffectDraft) {
    app.state_mut().edit.effect = Some(d);
    effect_session::sync_preview(app);
}

/// The brush set to `radius`, kept within [`BRUSH_LO`] … [`BRUSH_HI`].
fn set_brush(app: &mut App, radius: f32) {
    let Some((_, mut d)) = soften_draft(app) else { return };
    d.brush = if radius.is_finite() { radius.clamp(BRUSH_LO, BRUSH_HI) } else { d.brush.clamp(BRUSH_LO, BRUSH_HI) };
    app.state_mut().edit.effect = Some(d);
}

/// Paint (`erase` false) or Erase; Erase only once a paint stroke is there.
fn set_mode(app: &mut App, erase: bool) {
    let Some((_, mut d)) = soften_draft(app) else { return };
    d.erase = erase && painted(&d);
    app.state_mut().edit.effect = Some(d);
}

/// Whether `d` holds a paint stroke (something to erase).
fn painted(d: &EffectDraft) -> bool {
    d.strokes.iter().any(|s| !s.erase && !s.points.is_empty())
}

/// The photo area's part of an open Soften panel.
pub fn on_canvas(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let Some((photo, d)) = soften_draft(app) else {
        local(|l| *l = Local::default());
        return;
    };
    // A panel reset (or `ui.set`) since: Erase needs a paint stroke, the brush its range.
    if (d.erase && !painted(&d)) || !(BRUSH_LO..=BRUSH_HI).contains(&d.brush) {
        let mut fixed = d.clone();
        fixed.erase = d.erase && painted(&d);
        fixed.brush = if d.brush.is_finite() { d.brush.clamp(BRUSH_LO, BRUSH_HI) } else { SOFTEN_BRUSH_DEFAULT };
        app.state_mut().edit.effect = Some(fixed);
    }
    keys(app, &ctx);
    strokes(app, &ctx, photo);
    overlay(app, ui, photo);
}

/// The keys + (or =) and − scale the brush 1.2 times, as Retouch's; never while a field takes the
/// keys or a dialog is open.
fn keys(app: &mut App, ctx: &egui::Context) {
    let steps: i32 = ctx.input(|i| {
        i.events
            .iter()
            .map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.command && !modifiers.ctrl && !modifiers.alt => match key {
                    egui::Key::Plus | egui::Key::Equals => 1,
                    egui::Key::Minus => -1,
                    _ => 0,
                },
                _ => 0,
            })
            .sum()
    });
    if steps == 0 || ctx.text_edit_focused() || !app.state().dialogs.is_empty() {
        return;
    }
    let Some((_, d)) = soften_draft(app) else { return };
    set_brush(app, d.brush * BRUSH_STEP.powi(steps.clamp(-10, 10)));
}

/// The height of Soften's rows in its panel ([`rows`]).
pub fn rows_height(t: &Tokens) -> f32 {
    t.metrics.button_h + ROW_GAP + t.metrics.slider_row_h
}

/// Soften's rows in its parameter panel, under its sliders (the panel's extra rows,
/// [`super::effect_panel::extra_rows`]): Paint │ Erase, then Brush size in pixels of the stored
/// photo across the brush.
pub fn rows(app: &mut App, ui: &mut Ui) {
    let Some((photo, d)) = soften_draft(app) else { return };
    let t: Tokens = *app.tokens();
    let w = ui.available_width().max(1.0);

    let (_, row) = ui.allocate_space(vec2(w, t.metrics.button_h));
    let half = ((row.width() - MODE_GAP) / 2.0).max(0.0).floor();
    let paint_r = Rect::from_min_size(row.min, vec2(half, row.height()));
    let erase_r = Rect::from_min_size(pos2(row.max.x - half, row.min.y), vec2(half, row.height()));
    let paint = mode_button(ui, &t, PAINT_ID, &tl!("Paint"), EffectIcon::Brush, paint_r, !d.erase, true, &tl!("Paint where Soften applies"));
    let erase =
        mode_button(ui, &t, ERASE_ID, &tl!("Erase"), EffectIcon::Eraser, erase_r, d.erase, painted(&d), &tl!("Erase Soften where it was painted"));
    if paint.clicked() {
        set_mode(app, false);
    } else if erase.clicked() {
        set_mode(app, true);
    }

    ui.add_space(ROW_GAP);
    let (_, slider) = ui.allocate_space(vec2(w, t.metrics.slider_row_h));
    let short = dims(app, photo).map_or(1000.0, |(w, h)| f64::from(w.min(h)));
    let px = |r: f32| (2.0 * f64::from(r) * short).round().max(1.0);
    let (label, unit) = (tl!("Brush size"), super::retouch::px_unit());
    let spec = SliderSpec {
        id: BRUSH_ID,
        label: &label,
        min: px(BRUSH_LO),
        max: px(BRUSH_HI).max(px(BRUSH_LO) + 1.0),
        default: px(SOFTEN_BRUSH_DEFAULT),
        step: 1.0,
        decimals: 0,
        unit: &unit,
    };
    let mut child = ui.new_child(UiBuilder::new().id_salt("soften.brush.size").max_rect(slider));
    let out = widgets::slider(&mut child, &t, &spec, px(d.brush));
    if let Some(v) = out.value {
        set_brush(app, (v / (2.0 * short)) as f32);
    }
}

/// A mode button filling `rect`: its glyph and label, `tool_active` while its mode is on.
#[allow(clippy::too_many_arguments)] // the button's parts, as the ui-kit buttons take them
fn mode_button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, icon: EffectIcon, rect: Rect, active: bool, enabled: bool, tip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    let fill = if active {
        t.tool_active
    } else if enabled && r.is_pointer_button_down_on() {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.button
    };
    p.rect(rect, radius, fill, Line::new(1.0, t.button_border), StrokeKind::Inside);
    let (glyph, text) = if !enabled {
        (t.text_disabled, t.text_disabled)
    } else if active {
        (t.pick, t.text)
    } else {
        (t.icon, t.text)
    };
    let icon_r = Rect::from_center_size(pos2(rect.min.x + 14.0, rect.center().y), vec2(16.0, 16.0));
    paint_effect(p, icon_r, icon, glyph);
    p.text(pos2(icon_r.max.x + 5.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::semibold(t.button_label), text);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    widgets::register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), active);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, active, label));
    widgets::tooltip(r, t, tip, None)
}

/// The stroke under way: a press on the photo starts one, the drag adds its points, the release
/// ends it.
fn strokes(app: &mut App, ctx: &egui::Context, photo: PhotoId) {
    let (pressed, down, pos) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.latest_pos()));
    let live = local(|l| l.live).filter(|l| l.photo == photo);
    if live.is_none() && !pressed {
        return;
    }
    let Some(view) = canvas::view(ctx).filter(|v| v.photo == photo) else {
        local(|l| l.live = None);
        return;
    };
    let Some(dims) = dims(app, photo) else { return };
    let g = geometry(app, photo, dims);
    let at = pos.and_then(|p| displayed_at(&view, p)).and_then(|a| to_stored(&g, a));
    let Some((_, mut d)) = soften_draft(app) else { return };

    let Some(l) = live else {
        let hovered = ctx.read_response(Id::new(canvas::ID)).is_some_and(|r| r.hovered());
        let Some(at) = at.filter(|_| hovered) else { return };
        let bytes = effect_session::entry(&d).map_or(0, |e| e.len());
        if d.strokes.len() >= MAX_STROKES || bytes + STROKE_BYTES + POINT_BYTES > ENTRY_BUDGET {
            app.state_mut().status = format!("Soften holds at most {MAX_STROKES} strokes of this length: apply these first.");
            return;
        }
        let erase = d.erase && painted(&d);
        d.strokes.push(SoftenStroke { points: vec![round_point(at)], radius: d.brush.clamp(BRUSH_LO, BRUSH_HI), erase });
        let index = d.strokes.len() - 1;
        local(|l| {
            l.live = Some(Live { photo, index });
            l.bytes = bytes + STROKE_BYTES + POINT_BYTES;
        });
        set_draft(app, d);
        return;
    };
    if !down {
        local(|l| l.live = None);
    }
    let Some(at) = at else { return };
    let bytes = local(|l| l.bytes);
    if bytes + POINT_BYTES > ENTRY_BUDGET {
        local(|l| l.live = None);
        app.state_mut().status = "Soften's strokes are as long as one effect holds: apply these first.".into();
        return;
    }
    let Some(stroke) = d.strokes.get_mut(l.index) else {
        local(|l| l.live = None);
        return;
    };
    if stroke.points.len() >= MAX_STROKE_POINTS {
        // Full: go on as a new stroke from its last point, while there is room for one.
        let last = stroke.points.last().copied().unwrap_or_else(|| round_point(at));
        let next = SoftenStroke { points: vec![last], radius: stroke.radius, erase: stroke.erase };
        if d.strokes.len() >= MAX_STROKES || bytes + STROKE_BYTES + 2 * POINT_BYTES > ENTRY_BUDGET {
            local(|l| l.live = None);
            return;
        }
        d.strokes.push(next);
        let index = d.strokes.len() - 1;
        if let Some(s) = d.strokes.get_mut(index) {
            extend(s, at, aspect(dims));
        }
        local(|l| {
            l.live = down.then_some(Live { photo, index });
            l.bytes += STROKE_BYTES + 2 * POINT_BYTES;
        });
        set_draft(app, d);
        return;
    }
    let added = extend(stroke, at, aspect(dims));
    // The release keeps the last point, however close.
    let end = round_point(at);
    let last = !down && !added && stroke.points.last() != Some(&end) && stroke.points.len() < MAX_STROKE_POINTS;
    if last {
        stroke.points.push(end);
    }
    if added || last {
        local(|l| l.bytes += POINT_BYTES);
        set_draft(app, d);
    }
}

/// The photo area while Soften is open: the painted area's wash and the brush circle.
fn overlay(app: &mut App, ui: &Ui, photo: PhotoId) {
    let ctx = ui.ctx();
    let Some(view) = canvas::view(ctx).filter(|v| v.photo == photo) else { return };
    let Some(dims) = dims(app, photo) else { return };
    let Some((_, d)) = soften_draft(app) else { return };
    let response = ctx.read_response(Id::new(canvas::ID));
    let hover = response.as_ref().filter(|r| r.hovered()).and_then(Response::hover_pos).and_then(|p| displayed_at(&view, p).map(|at| (p, at)));
    let drawing = local(|l| l.live.is_some());
    let painter = ui.painter_at(view.area);
    let t = *app.tokens();

    // The wash, its mask made off the UI thread whenever the strokes change.
    let key = (!d.strokes.is_empty()).then(|| MaskKey { photo, dims, recipe: tools::recipe(app, photo), strokes: d.strokes.clone() });
    wash(ctx, key.as_ref());
    if hover.is_some() || drawing {
        local(|l| {
            if let Some((_, tex, turn)) = &l.wash {
                painter.add(textured(view.image, tex.id(), *turn));
            }
        });
    }

    // The brush circle at the pointer.
    let Some((p, at)) = hover else { return };
    ctx.set_cursor_icon(CursorIcon::Crosshair);
    let g = geometry(app, photo, dims);
    let Some(stored) = to_stored(&g, at) else { return };
    let Some(r) = screen_radius(&g, &view, dims, stored, d.brush) else { return };
    let r = r.max(2.0);
    painter.circle_stroke(p, r + 1.25, Line::new(1.0, t.face_box_outer));
    painter.circle_stroke(p, r, Line::new(1.5, t.face_box));
    if d.erase {
        let half = (r * 0.5).clamp(2.0, 8.0);
        let bar = [p - vec2(half, 0.0), p + vec2(half, 0.0)];
        painter.line_segment(bar, Line::new(3.0, t.face_box_outer));
        painter.line_segment(bar, Line::new(1.5, t.face_box));
    }
}

/// Keeps the wash's mask in step with `key` (`None`: no strokes, no wash): a finished mask
/// becomes the texture shown, and a new one is asked for when the strokes changed since.
fn wash(ctx: &egui::Context, key: Option<&MaskKey>) {
    let Some(key) = key else {
        local(|l| {
            l.wash = None;
            l.job = None;
        });
        return;
    };
    // A finished mask, even one the strokes have passed since: it shows until the next is made.
    let done = local(|l| {
        let job = l.job.as_ref()?;
        let out = match job.rx.try_recv() {
            Ok(mask) => Some((job.key.clone(), mask)),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
        l.job = None;
        out
    });
    if let Some((made, Some(mask))) = done {
        let pixels: Vec<Color32> =
            mask.data.iter().map(|m| Color32::from_rgba_unmultiplied(255, 255, 255, (m.clamp(0.0, 1.0) * WASH * 255.0).round() as u8)).collect();
        if pixels.len() == mask.width.saturating_mul(mask.height) {
            let image = egui::ColorImage::new([mask.width, mask.height], pixels);
            let turn = Recipe::parse(&made.recipe).map(|r| r.orientation()).unwrap_or_default();
            let tex = ctx.load_texture("soften:wash", image, egui::TextureOptions::LINEAR);
            local(|l| l.wash = Some((made, tex, turn)));
        }
    }
    let (shown, running) = local(|l| (l.wash.as_ref().is_some_and(|(k, ..)| k == key), l.job.is_some()));
    if !shown && !running {
        let (tx, rx) = mpsc::channel();
        let work = key.clone();
        let started = std::thread::Builder::new().name("dalica-soften-wash".into()).spawn(move || {
            let _ = tx.send(mask_of(&work));
        });
        if started.is_ok() {
            local(|l| l.job = Some(Job { key: key.clone(), rx }));
        }
    }
    if local(|l| l.job.is_some()) {
        ctx.request_repaint_after(Duration::from_millis(8));
    }
}

/// The wash's mask of `key`: the entry's coverage over the whole picture at a small size, before
/// the recipe's rotations and flips. On a worker thread.
fn mask_of(key: &MaskKey) -> Option<Mask> {
    let recipe = Recipe::parse(&key.recipe).ok()?;
    let g = Geometry::new(&recipe, Orientation::default(), key.dims.0, key.dims.1);
    let (w, h) = g.size();
    let long = w.max(h);
    if !(long.is_finite() && long > 0.0) {
        return None;
    }
    let (width, height) = g.output_size((MASK_EDGE / long).min(1.0)).ok()?;
    let strokes = key
        .strokes
        .iter()
        .filter(|s| !s.points.is_empty())
        .map(|s| Stroke {
            mode: if s.erase { Mode::Erase } else { Mode::Paint },
            radius: f64::from(s.radius),
            points: s.points.iter().map(|p| Point::new(f64::from(p[0]), f64::from(p[1]))).collect(),
        })
        .collect();
    let soften = Soften { strokes, ..Soften::default() };
    let data = soften.coverage(&g, width, height, (0, 0, width, height))?;
    Some(Mask { width: width as usize, height: height as usize, data })
}

/// `rect` covered by `texture`, its picture turned by `turn` (the recipe's rotations and flips).
fn textured(rect: Rect, texture: egui::TextureId, turn: Dihedral) -> Shape {
    let back = turn.inverse();
    let mut mesh = Mesh::with_texture(texture);
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let uv = Point::new(x, y).transform(back);
        mesh.vertices.push(Vertex { pos: rect.lerp_inside(vec2(x as f32, y as f32)), uv: pos2(uv.x as f32, uv.y as f32), color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke() -> SoftenStroke {
        SoftenStroke { points: Vec::new(), radius: 0.05, erase: false }
    }

    #[test]
    fn points_closer_than_a_quarter_of_the_radius_are_dropped_and_the_rest_rounded() {
        let mut s = stroke();
        assert!(extend(&mut s, [0.123_456, 0.5], (1.5, 1.0)));
        assert_eq!(s.points, [[0.1235, 0.5]]);
        // 0.005 across on a 3:2 photo is 0.0075 of the shorter side: under 0.0125.
        assert!(!extend(&mut s, [0.1285, 0.5], (1.5, 1.0)));
        assert!(extend(&mut s, [0.1335, 0.5], (1.5, 1.0)));
        assert_eq!(s.points.len(), 2);
        // Hostile points stay inside the photo; NaN goes to the centre.
        assert_eq!(round_point([f32::NAN, 7.0]), [0.5, 1.0]);
        assert_eq!(round_point([-0.2, f32::INFINITY]), [0.0, 0.5]);
    }

    #[test]
    fn a_long_scribble_stays_well_under_the_entrys_limit() {
        // 100 000 pointer moves back and forth across the photo at the smallest brush.
        let mut s = SoftenStroke { radius: BRUSH_LO, ..stroke() };
        for i in 0..100_000 {
            let x = ((i as f32 / 1000.0).sin() + 1.0) / 2.0;
            let y = i as f32 / 100_000.0;
            extend(&mut s, [x, y], (1.5, 1.0));
        }
        assert_eq!(s.points.len(), MAX_STROKE_POINTS, "the stroke fills up and stops");
        let mut d = EffectDraft::new(EFFECT);
        d.strokes.push(s);
        let entry = effect_session::entry(&d).unwrap();
        assert!(entry.len() <= "soften(50,50)".len() + STROKE_BYTES + MAX_STROKE_POINTS * POINT_BYTES, "{}", entry.len());
        assert!(entry.len() < MAX_OPERATION / 4, "{}", entry.len());
    }

    #[test]
    fn the_brush_range_is_what_the_entry_stores() {
        assert_eq!((BRUSH_LO, BRUSH_HI), (0.005, 0.25));
        const { assert!(ENTRY_BUDGET < MAX_OPERATION) };
        assert_eq!(aspect((300, 200)), (1.5, 1.0));
        assert_eq!(aspect((0, 0)), (1.0, 1.0));
    }
}
