//! The Retouch tool (`editing.md` §6.4): the "Retouch" panel in place of the tab's content and the
//! patches made on the photo.
//!
//! ```text
//! [▣] Retouch
//! Click a blemish, then click a clean area to copy from.
//! Brush size                                   42 px
//! ───────○──────────────────────
//! [ Undo Patch ][ Redo Patch ]
//! [ Reset ]
//! [   Apply    ][   Cancel   ]
//! ```
//!
//! **On the photo.** A click on the photo picks the blemish (the target); a circle of the brush's
//! size stays there. While the pointer moves over the photo the target shows the texture under the
//! pointer (the session's draft preview of the patch as it would be), and a second click picks that
//! source: the patch is made. A right click drops a target still waiting. Before a target the
//! pointer carries a circle of the brush's size. The photo area's own clicks, hold and drag are off
//! while a tool is open (`canvas.rs`); the wheel still zooms. Points are read off the photo as it
//! shows (after its crop, straighten, rotations and flips) and kept in the stored frame
//! (`data-model.md` §3.3) through the photo's [`Geometry`].
//!
//! **The draft** (`edit.retouch`): the patches made, how many are in effect (Undo Patch lowers it,
//! Redo Patch raises it, a new patch drops those past it) and the brush, its radius a fraction of
//! the photo's longer side. Each patch is a step inside the tool; Apply records the patches in
//! effect as one `retouch(sx,sy,tx,ty,r;…)` entry, one "Retouch" step of the photo's history
//! (`photo.history.apply`), its radii converted to the operation's (a fraction of the shorter side).
//! The brush stays within what that operation stores (0.001 … 0.1 of the shorter side). Apply with
//! no patch in effect closes the tool and records nothing. Cancel and Esc drop the draft.
//!
//! **Keys:** + (or =) and − make the brush 1.2 times larger or smaller; the edit view's Undo and
//! Redo shortcuts (Ctrl+Z, Ctrl+Y) are Undo Patch and Redo Patch while the tool is open.
//!
//! **Actions** (`edit.tool.action {name, params}`), points `[x, y]` normalised to the photo as it
//! shows, as a click reads them: `target {at}` (a click picking the blemish), `source {at}` (the
//! second click), `patch {target, source}` (both), `drop` (a right click), `undo`, `redo` (Undo
//! Patch, Redo Patch), `brush {size}` (the radius, a fraction of the longer side) or
//! `brush {step}` (+ / − presses, ± 1 … 10).
//!
//! **The panel and the photo area.** The panel draws in the edit panel's content; the frame
//! (`panels/edit.rs`) calls only Straighten in the photo area, so the circles are painted on a layer
//! of their own just above the window's ([`overlay`]) and the clicks are read from the photo area's
//! response, which egui works out at the start of each frame from the previous frame's rects.
//! Ids: `slider:retouch.brush`, `button:retouch.undoPatch`, `button:retouch.redoPatch`,
//! `button:panel.reset`, `button:panel.apply`, `button:panel.cancel`.

use std::cell::RefCell;

use dalica_core::PhotoId;
use dalica_edit::ops::retouch::{MAX_RADIUS, MIN_RADIUS, Retouch as RetouchOp, Stroke};
use dalica_edit::{Geometry, Point, Recipe};
use dalica_engine::{CmdError, Context, Key};
use dalica_image::Orientation;
use dalica_ui_kit::icons::{EditIcon, paint_edit};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, SliderSpec, Target, ToastKind};
use dalica_ui_kit::{Tokens, fonts};
use egui::{
    Align2, CornerRadius, CursorIcon, Id, LayerId, Order, Pos2, Rect, Response, Sense, Stroke as Line, StrokeKind, Ui, UiBuilder, Vec2, WidgetType,
    pos2, vec2,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::canvas::{self, View};
use crate::app::App;
use crate::commands::parse;
use crate::commands::tools::Tool;
use crate::state::{BRUSH_DEFAULT, BRUSH_MAX, BRUSH_MIN, EditState, MAX_PATCHES, Patch, RetouchDraft, ToolKind};

pub const TITLE: &str = "Retouch";
pub const INSTRUCTION: &str = "Click a blemish, then click a clean area to copy from.";
pub const BRUSH_ID: &str = "slider:retouch.brush";
pub const BRUSH_LABEL: &str = "Brush size";
pub const UNDO_ID: &str = "button:retouch.undoPatch";
pub const REDO_ID: &str = "button:retouch.redoPatch";
pub const RESET_ID: &str = "button:panel.reset";
pub const APPLY_ID: &str = "button:panel.apply";
pub const CANCEL_ID: &str = "button:panel.cancel";
/// The tooltips of Undo Patch and Redo Patch.
pub const UNDO_TIP: &str = "Remove the last patch";
pub const REDO_TIP: &str = "Put back the patch removed last";
/// How much one + or − press scales the brush.
pub const BRUSH_STEP: f32 = 1.2;
/// The most + / − presses one `brush {step}` action takes.
pub const MAX_STEPS: i32 = 10;

/// The title row's height, the gaps between the panel's rows, and the gap between two buttons of
/// a row.
const TITLE_H: f32 = 26.0;
const GAP: f32 = 8.0;
const BUTTON_GAP: f32 = 6.0;
/// The hover preview follows the pointer once it has moved this far (normalised to the photo).
const HOVER_MOVE: f32 = 0.002;

pub struct Retouch;

pub static RETOUCH: Retouch = Retouch;

/// What the tool keeps outside the edit state, on the UI thread: the target waiting for its source,
/// the source under the pointer, and the candidate recipe last handed to the preview.
#[derive(Default)]
struct Local {
    /// The photo and the target clicked (stored frame) whose source is still to pick.
    pending: Option<(PhotoId, [f32; 2])>,
    /// The source under the pointer while a target waits (stored frame).
    hover: Option<[f32; 2]>,
    /// The photo and the candidate recipe the session's preview was last given.
    candidate: Option<(PhotoId, Vec<String>)>,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

fn with_local<R>(f: impl FnOnce(&mut Local) -> R) -> R {
    LOCAL.with(|l| f(&mut l.borrow_mut()))
}

/// The target of `photo` waiting for its source (stored frame), if one does.
pub fn pending(photo: PhotoId) -> Option<[f32; 2]> {
    with_local(|l| l.pending.filter(|(p, _)| *p == photo).map(|(_, at)| at))
}

fn forget_pending() {
    with_local(|l| {
        l.pending = None;
        l.hover = None;
    });
}

/// Drops the waiting target and the hover, and hands the preview back the photo's own recipe.
fn release(app: &mut App) {
    forget_pending();
    let had = with_local(|l| l.candidate.take().is_some());
    if had && let Some(s) = app.session_mut() {
        let _ = s.set_preview_candidate(None);
    }
}

/// The photo's stored size (after its EXIF orientation) and its geometry as the edit view shows
/// it: its own recipe's crop, straighten, rotations and flips.
fn geometry(app: &App, photo: PhotoId) -> Option<((f64, f64), Geometry)> {
    let s = app.session()?;
    let p = s.library().photo(photo)?;
    if p.width == 0 || p.height == 0 {
        return None;
    }
    let recipe = Recipe::parse(&s.library().info(photo).edits).unwrap_or_default();
    Some(((f64::from(p.width), f64::from(p.height)), Geometry::new(&recipe, Orientation::default(), p.width, p.height)))
}

/// The brush radii a patch may have on a photo of stored size `dims`, as fractions of its longer
/// side: the draft's range narrowed to what the operation stores (0.001 … 0.1 of the shorter side).
pub fn brush_range(dims: Option<(f64, f64)>) -> (f32, f32) {
    let ratio = dims.filter(|(w, h)| *w > 0.0 && *h > 0.0).map_or(1.0, |(w, h)| w.min(h) / w.max(h));
    let lo = BRUSH_MIN.max((MIN_RADIUS * ratio) as f32);
    let hi = BRUSH_MAX.min((MAX_RADIUS * ratio) as f32).max(lo);
    (lo, hi)
}

/// A brush radius (a fraction of the longer side) as the operation's (of the shorter side).
pub fn op_radius(radius: f32, dims: (f64, f64)) -> f64 {
    let (w, h) = dims;
    let ratio = if w > 0.0 && h > 0.0 { w.max(h) / w.min(h) } else { 1.0 };
    let r = f64::from(radius) * ratio;
    if r.is_finite() { r.clamp(MIN_RADIUS, MAX_RADIUS) } else { MIN_RADIUS }
}

/// `v` to six decimals (a millionth of the photo: finer than a pixel of any photo the library
/// reads), so the entry reads `0.03` rather than the single-precision draft's `0.029999999329…`.
fn stored_value(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r.is_finite() { r } else { v }
}

/// The `retouch(…)` entry of `patches` on a photo of stored size `dims`; `None` for no patch.
pub fn op_text(patches: &[Patch], dims: (f64, f64)) -> Option<String> {
    if patches.is_empty() {
        return None;
    }
    let point = |p: [f32; 2]| Point::new(stored_value(f64::from(p[0])), stored_value(f64::from(p[1])));
    let strokes = patches
        .iter()
        .take(MAX_PATCHES)
        .map(|p| Stroke { source: point(p.source), target: point(p.target), radius: stored_value(op_radius(p.radius, dims)) })
        .collect();
    Some(format!("retouch({})", RetouchOp { strokes }))
}

/// The stored-frame point under the displayed point `at` (normalised to the photo as it shows).
fn to_stored(g: &Geometry, at: [f32; 2]) -> Option<[f32; 2]> {
    let p = g.to_stored(Point::new(f64::from(at[0]), f64::from(at[1])))?;
    let c = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) as f32 } else { 0.5 };
    Some([c(p.x), c(p.y)])
}

/// Where the stored-frame point `at` shows on the screen, with the photo drawn at `view`.
fn to_screen(g: &Geometry, view: &View, at: [f32; 2]) -> Option<Pos2> {
    let d = g.to_display(Point::new(f64::from(at[0]), f64::from(at[1])))?;
    let s = pos2(view.image.min.x + d.x as f32 * view.image.width(), view.image.min.y + d.y as f32 * view.image.height());
    (s.x.is_finite() && s.y.is_finite()).then_some(s)
}

/// The displayed point (normalised) under screen point `p`, when it is on the photo.
fn displayed_at(view: &View, p: Pos2) -> Option<[f32; 2]> {
    let r = view.image;
    (r.contains(p) && r.width() > 0.0 && r.height() > 0.0).then(|| [(p.x - r.min.x) / r.width(), (p.y - r.min.y) / r.height()])
}

/// The radius on the screen of a brush of `radius` (a fraction of the longer side) at the stored
/// point `at`.
fn screen_radius(g: &Geometry, dims: (f64, f64), view: &View, at: [f32; 2], radius: f32) -> Option<f32> {
    let dx = (f64::from(radius) * dims.0.max(dims.1) / dims.0) as f32;
    let a = to_screen(g, view, at)?;
    let b = to_screen(g, view, [at[0] + dx, at[1]])?;
    let r = (b - a).length();
    r.is_finite().then_some(r)
}

/// The open photo and its Retouch draft, or `disabled`.
fn draft(app: &App) -> Result<(PhotoId, RetouchDraft), CmdError> {
    let e = &app.state().edit;
    match (e.photo, &e.retouch, e.tool) {
        (Some(p), Some(d), Some(ToolKind::Retouch)) => Ok((p, d.clone())),
        _ => Err(CmdError::Disabled("Retouch is not open.".into())),
    }
}

fn set_draft(app: &mut App, d: RetouchDraft) {
    app.state_mut().edit.retouch = Some(d);
}

/// What the draft holds, as the actions answer.
fn summary(app: &App) -> Value {
    let e = &app.state().edit;
    let pending = e.photo.and_then(pending);
    match &e.retouch {
        Some(d) => json!({ "patches": d.position, "made": d.patches.len(), "brush": d.brush, "pending": pending }),
        None => json!({ "patches": 0, "made": 0, "pending": pending }),
    }
}

/// A normalised point of an action's parameters: two finite numbers in 0 … 1.
fn unit_point(v: [f64; 2], what: &str) -> Result<[f32; 2], CmdError> {
    if v.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)) {
        Ok([v[0] as f32, v[1] as f32])
    } else {
        Err(CmdError::BadParams(format!("`{what}` is [x, y] on the photo, each 0 … 1")))
    }
}

fn unknown_size() -> CmdError {
    CmdError::Disabled("The photo's size is not known yet.".into())
}

/// Picks the target at the displayed point `at`.
fn pick_target(app: &mut App, at: [f32; 2]) -> Result<Value, CmdError> {
    let (photo, _) = draft(app)?;
    let (_, g) = geometry(app, photo).ok_or_else(unknown_size)?;
    let target = to_stored(&g, at).ok_or_else(unknown_size)?;
    with_local(|l| {
        l.pending = Some((photo, target));
        l.hover = None;
    });
    Ok(summary(app))
}

/// Makes the patch from the waiting target to the source at the displayed point `at`.
fn pick_source(app: &mut App, at: [f32; 2]) -> Result<Value, CmdError> {
    let (photo, mut d) = draft(app)?;
    let target = pending(photo).ok_or_else(|| CmdError::Disabled("Click a blemish first.".into()))?;
    let (_, g) = geometry(app, photo).ok_or_else(unknown_size)?;
    let source = to_stored(&g, at).ok_or_else(unknown_size)?;
    let position = d.position.min(d.patches.len());
    if position >= MAX_PATCHES {
        return Err(CmdError::Disabled(format!("One retouch holds at most {MAX_PATCHES} patches: apply these first.")));
    }
    d.patches.truncate(position);
    d.patches.push(Patch { target, source, radius: d.brush });
    d.position = d.patches.len();
    forget_pending();
    set_draft(app, d);
    Ok(summary(app))
}

/// Undo Patch (`back`) or Redo Patch: one patch fewer or one more in effect.
fn step_patches(app: &mut App, back: bool) -> Result<Value, CmdError> {
    let (_, mut d) = draft(app)?;
    let position = d.position.min(d.patches.len());
    d.position = if back {
        position.checked_sub(1).ok_or_else(|| CmdError::Disabled("No patch to undo.".into()))?
    } else if position < d.patches.len() {
        position + 1
    } else {
        return Err(CmdError::Disabled("No patch to redo.".into()));
    };
    forget_pending();
    set_draft(app, d);
    Ok(summary(app))
}

/// The brush set to `radius`, kept within what the photo's patches may be.
fn set_brush(app: &mut App, radius: f32) -> Result<Value, CmdError> {
    let (photo, mut d) = draft(app)?;
    let (lo, hi) = brush_range(geometry(app, photo).map(|(dims, _)| dims));
    d.brush = if radius.is_finite() { radius.clamp(lo, hi) } else { d.brush.clamp(lo, hi) };
    set_draft(app, d);
    Ok(summary(app))
}

/// The brush after `steps` + presses (− presses when negative).
fn step_brush(app: &mut App, steps: i32) -> Result<Value, CmdError> {
    let (_, d) = draft(app)?;
    set_brush(app, d.brush * BRUSH_STEP.powi(steps.clamp(-MAX_STEPS, MAX_STEPS)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct At {
    at: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchParams {
    target: [f64; 2],
    source: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrushParams {
    #[serde(default)]
    size: Option<f64>,
    #[serde(default)]
    step: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

impl Tool for Retouch {
    fn kind(&self) -> ToolKind {
        ToolKind::Retouch
    }

    fn label(&self) -> &'static str {
        "Retouch"
    }

    /// No patches and the default brush, within what the photo's patches may be.
    fn open(&self, app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        let (lo, hi) = brush_range(edit.photo.and_then(|p| geometry(app, p)).map(|(dims, _)| dims));
        edit.retouch = Some(RetouchDraft { brush: BRUSH_DEFAULT.clamp(lo, hi), ..RetouchDraft::default() });
        forget_pending();
        Ok(())
    }

    /// Every patch dropped; the brush stays as it is.
    fn reset(&self, app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        let brush = edit.retouch.as_ref().map(|d| d.brush);
        self.open(app, edit)?;
        if let (Some(b), Some(d)) = (brush, edit.retouch.as_mut()) {
            d.brush = b;
        }
        Ok(())
    }

    /// The patches in effect as one `retouch(…)` step of the photo's history; none: nothing.
    fn apply(&self, app: &mut App) -> Result<Value, CmdError> {
        let (photo, d) = draft(app)?;
        let in_effect = d.patches.get(..d.position.min(d.patches.len())).unwrap_or_default();
        let Some(op) = op_text(in_effect, geometry(app, photo).map_or((1.0, 1.0), |(dims, _)| dims)) else {
            release(app);
            return Ok(json!({ "applied": false, "patches": 0 }));
        };
        let result = app.run("photo.history.apply", &json!({ "id": photo, "op": op }))?;
        release(app);
        Ok(json!({ "applied": true, "patches": in_effect.len(), "op": op, "history": result }))
    }

    fn action(&self, app: &mut App, name: &str, params: &Value) -> Result<Value, CmdError> {
        match name {
            "target" => {
                let p: At = parse(params)?;
                pick_target(app, unit_point(p.at, "at")?)
            }
            "source" => {
                let p: At = parse(params)?;
                pick_source(app, unit_point(p.at, "at")?)
            }
            "patch" => {
                let p: PatchParams = parse(params)?;
                let (target, source) = (unit_point(p.target, "target")?, unit_point(p.source, "source")?);
                pick_target(app, target)?;
                pick_source(app, source)
            }
            "drop" => {
                let _: Nothing = parse(params)?;
                draft(app)?;
                forget_pending();
                Ok(summary(app))
            }
            "undo" | "redo" => {
                let _: Nothing = parse(params)?;
                step_patches(app, name == "undo")
            }
            "brush" => {
                let p: BrushParams = parse(params)?;
                match (p.size, p.step) {
                    (Some(s), None) if s.is_finite() && (f64::from(BRUSH_MIN)..=f64::from(BRUSH_MAX)).contains(&s) => set_brush(app, s as f32),
                    (None, Some(n)) if n != 0 && (-MAX_STEPS..=MAX_STEPS).contains(&n) => step_brush(app, n),
                    _ => Err(CmdError::BadParams(format!(
                        "`brush` takes `size` ({BRUSH_MIN} … {BRUSH_MAX} of the longer side) or `step` (±1 … {MAX_STEPS}), not both"
                    ))),
                }
            }
            _ => Err(CmdError::BadParams(format!("Retouch has no action `{}`", name.chars().take(40).collect::<String>()))),
        }
    }

    /// The brush keys (+ or = larger, − smaller), the Undo and Redo shortcuts walking the patches,
    /// and Esc, which hands the preview back before the tool is cancelled.
    fn key(&self, app: &mut App, key: &Key) -> Option<Result<Value, CmdError>> {
        if key.code == "Esc" && !key.ctrl && !key.alt {
            release(app);
            return None;
        }
        if !key.ctrl && !key.alt {
            match key.code.as_str() {
                "Plus" | "=" => return Some(step_brush(app, 1)),
                "-" => return Some(step_brush(app, -1)),
                _ => {}
            }
        }
        match app.session().and_then(|s| s.keymap().resolve(key, Context::EditView, false)).as_deref() {
            Some("edit.history.undo" | "edit.undo") => Some(step_patches(app, true)),
            Some("edit.history.redo" | "edit.redo") => Some(step_patches(app, false)),
            _ => None,
        }
    }
}

/// Draws the Retouch panel in place of the tab's content, and the patches on the photo.
pub fn show(app: &mut App, ui: &mut Ui) {
    let Ok((photo, d)) = draft(app) else { return };
    let ctx = ui.ctx().clone();
    minus_key(app, &ctx);
    panel(app, ui, photo, &d);
    overlay(app, &ctx, ui.layer_id());
}

/// What a click in the panel asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Click {
    Undo,
    Redo,
    Reset,
    Apply,
    Cancel,
}

/// Where the panel's parts go in `rect`, the instruction `instruction_h` tall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub title: Rect,
    /// The instruction's top-left corner.
    pub instruction: Pos2,
    pub slider: Rect,
    pub undo: Rect,
    pub redo: Rect,
    pub reset: Rect,
    pub apply: Rect,
    pub cancel: Rect,
}

/// The panel's layout: the title, the instruction, the Brush size row, Undo Patch │ Redo Patch,
/// Reset on a row of its own, then Apply │ Cancel. When the content is too short for that (the edit
/// panel of a 900 × 600 window) Reset joins the patch row as its third button; Apply │ Cancel never
/// leave the content, overlapping what is above them before that.
pub fn layout(t: &Tokens, rect: Rect, instruction_h: f32) -> Layout {
    let bh = t.metrics.button_h;
    let w = rect.width().max(0.0);
    let title = Rect::from_min_size(rect.min, vec2(w, TITLE_H));
    let instruction = pos2(rect.min.x, title.max.y);
    let instruction_h = if instruction_h.is_finite() { instruction_h.max(0.0) } else { 0.0 };
    let slider = Rect::from_min_size(pos2(rect.min.x, instruction.y + instruction_h + GAP), vec2(w, t.metrics.slider_row_h));
    let y = slider.max.y + GAP;
    let roomy = y + 3.0 * bh + BUTTON_GAP + 2.0 * GAP <= rect.max.y;
    let cell = |y: f32, i: f32, n: f32| {
        let cw = ((w - (n - 1.0) * BUTTON_GAP) / n).max(0.0).floor();
        let x = if i + 1.0 >= n { rect.min.x + w - cw } else { rect.min.x + i * (cw + BUTTON_GAP) };
        Rect::from_min_size(pos2(x, y), vec2(cw, bh))
    };
    let (undo, redo, reset, after) = if roomy {
        (cell(y, 0.0, 2.0), cell(y, 1.0, 2.0), cell(y + bh + BUTTON_GAP, 0.0, 2.0), y + 2.0 * bh + BUTTON_GAP + GAP)
    } else {
        (cell(y, 0.0, 3.0), cell(y, 1.0, 3.0), cell(y, 2.0, 3.0), y + bh + GAP)
    };
    let ay = after.min(rect.max.y - bh).max(rect.min.y);
    Layout { title, instruction, slider, undo, redo, reset, apply: cell(ay, 0.0, 2.0), cancel: cell(ay, 1.0, 2.0) }
}

fn panel(app: &mut App, ui: &mut Ui, photo: PhotoId, d: &RetouchDraft) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let p = ui.painter().clone();
    let job = egui::text::LayoutJob::simple(INSTRUCTION.to_owned(), fonts::regular(t.detail), t.text_label, rect.width().max(1.0));
    let galley = p.layout_job(job);
    let l = layout(&t, rect, galley.size().y);
    // The title: the Retouch glyph and the name; the instruction under it, wrapped.
    let glyph = Rect::from_min_size(pos2(l.title.min.x, l.title.center().y - t.metrics.icon_tool / 2.0), Vec2::splat(t.metrics.icon_tool));
    paint_edit(&p, glyph, EditIcon::Retouch, t.icon);
    p.text(pos2(glyph.max.x + 8.0, l.title.center().y), Align2::LEFT_CENTER, TITLE, fonts::semibold(t.title), t.text);
    p.galley(l.instruction, galley, t.text_label);

    // Brush size, in pixels of the photo across the brush.
    let dims = geometry(app, photo).map(|(dims, _)| dims);
    let (lo, hi) = brush_range(dims);
    let long = dims.map_or(1000.0, |(w, h)| w.max(h));
    let px = |r: f32| (2.0 * f64::from(r) * long).round();
    let spec = SliderSpec {
        id: BRUSH_ID,
        label: BRUSH_LABEL,
        min: px(lo).max(1.0),
        max: px(hi).max(1.0),
        default: px(BRUSH_DEFAULT.clamp(lo, hi)).max(1.0),
        step: 1.0,
        decimals: 0,
        unit: " px",
    };
    let mut child = ui.new_child(UiBuilder::new().id_salt("retouch.brush").max_rect(l.slider));
    let out = widgets::slider(&mut child, &t, &spec, px(d.brush));

    let mut clicked = None;
    let position = d.position.min(d.patches.len());
    if button(ui, &t, UNDO_ID, "Undo Patch", l.undo, position > 0, false, UNDO_TIP).clicked() {
        clicked = Some(Click::Undo);
    }
    if button(ui, &t, REDO_ID, "Redo Patch", l.redo, position < d.patches.len(), false, REDO_TIP).clicked() {
        clicked = Some(Click::Redo);
    }
    if button(ui, &t, RESET_ID, "Reset", l.reset, !d.patches.is_empty(), false, "Remove every patch").clicked() {
        clicked = Some(Click::Reset);
    }
    if button(ui, &t, APPLY_ID, "Apply", l.apply, true, true, "Keep the patches").clicked() {
        clicked = Some(Click::Apply);
    }
    if button(ui, &t, CANCEL_ID, "Cancel", l.cancel, true, false, "Close without the patches").clicked() {
        clicked = Some(Click::Cancel);
    }

    if let Some(v) = out.value {
        let _ = set_brush(app, (v / (2.0 * long)) as f32);
    }
    let done = match clicked {
        None => return,
        Some(Click::Undo) => step_patches(app, true),
        Some(Click::Redo) => step_patches(app, false),
        Some(Click::Reset) => app.run("edit.tool.reset", &json!({})),
        Some(Click::Apply) => app.run("edit.tool.apply", &json!({})),
        Some(Click::Cancel) => {
            release(app);
            app.run("edit.tool.cancel", &json!({}))
        }
    };
    if let Err(e) = done {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// A button filling `rect` with a centred label: as `widgets::text_button` draws it, or filled
/// `accent` as `widgets::primary_button` when `primary`; drawn disabled when `enabled` is false.
/// The tooltip shows disabled too.
#[allow(clippy::too_many_arguments)] // the button's parts, as the ui-kit buttons take them
fn button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, rect: Rect, enabled: bool, primary: bool, tip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    let down = enabled && r.is_pointer_button_down_on();
    let hovered = enabled && r.hovered();
    let text = if primary && enabled {
        let fill = if down {
            t.accent_pressed
        } else if hovered {
            t.accent_hover
        } else {
            t.accent
        };
        p.rect_filled(rect, radius, fill);
        t.on_accent
    } else {
        let fill = if down {
            t.pressed
        } else if hovered {
            t.hover
        } else {
            t.button
        };
        p.rect(rect, radius, fill, Line::new(1.0, t.button_border), StrokeKind::Inside);
        if enabled { t.text } else { t.text_disabled }
    };
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), fonts::semibold(t.button_label), text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 12.0).max(1.0));
    let galley = p.layout_job(job);
    p.galley(Align2::CENTER_CENTER.align_size_within_rect(galley.size(), rect).min.round(), galley, text);
    widgets::register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::tooltip(r, t, tip, None)
}

/// − as a brush step while the keymap cannot see it: `keys::from_egui` names egui's Minus key by
/// its symbol, the Unicode minus, which the keymap does not parse (`canvas.rs` does the same for
/// Ctrl+−). Once it does, [`Tool::key`] gets the key and this does nothing.
fn minus_key(app: &mut App, ctx: &egui::Context) {
    let lost = |m: &egui::Modifiers| !m.command && !m.ctrl && !m.alt && crate::keys::from_egui(egui::Key::Minus, *m).is_none();
    let presses = ctx.input(|i| {
        i.events.iter().filter(|e| matches!(e, egui::Event::Key { key: egui::Key::Minus, pressed: true, modifiers, .. } if lost(modifiers))).count()
    });
    if presses == 0 || ctx.egui_wants_keyboard_input() || !app.state().dialogs.is_empty() {
        return;
    }
    let steps = i32::try_from(presses).unwrap_or(MAX_STEPS).min(MAX_STEPS);
    let _ = step_brush(app, -steps);
}

/// The layer the circles are painted on, just above the window's own.
fn overlay_layer() -> LayerId {
    LayerId::new(Order::Background, Id::new("retouch:overlay"))
}

/// The photo area while Retouch is open: its clicks pick the target and the source, the preview
/// shows the patches in effect (and the one under the pointer), and the circles are drawn.
fn overlay(app: &mut App, ctx: &egui::Context, above: LayerId) {
    let Ok((photo, _)) = draft(app) else { return };
    let Some(view) = canvas::view(ctx).filter(|v| v.photo == photo) else { return };
    let Some((dims, g)) = geometry(app, photo) else { return };
    let response = ctx.read_response(Id::new(canvas::ID));
    let pointer = response.as_ref().and_then(Response::hover_pos).and_then(|p| displayed_at(&view, p).map(|at| (p, at)));
    if pointer.is_some() {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
    }

    // The clicks.
    if let Some(r) = &response {
        if r.secondary_clicked() {
            forget_pending();
        } else if r.clicked()
            && let Some(at) = r.interact_pointer_pos().and_then(|p| displayed_at(&view, p))
        {
            let done = if pending(photo).is_some() { pick_source(app, at) } else { pick_target(app, at) };
            if let Err(e) = done {
                app.toast(&e.to_string(), ToastKind::Caution);
            }
        }
    }
    let Ok((_, d)) = draft(app) else { return };
    let waiting = pending(photo);

    // The source under the pointer while a target waits.
    let hover = waiting.and(pointer).and_then(|(_, at)| to_stored(&g, at));
    let hover = with_local(|l| {
        match (hover, l.hover) {
            (Some(h), Some(old)) if (h[0] - old[0]).abs() < HOVER_MOVE && (h[1] - old[1]).abs() < HOVER_MOVE => {}
            (h, _) => l.hover = h,
        }
        l.hover
    });

    // The preview: the photo's recipe and the patches in effect, and the one under the pointer.
    let mut patches: Vec<Patch> = d.patches.iter().take(d.position).copied().collect();
    if let (Some(target), Some(source)) = (waiting, hover) {
        patches.push(Patch { target, source, radius: d.brush });
    }
    let want = op_text(&patches, dims).and_then(|op| {
        let mut texts = app.session()?.library().info(photo).edits.clone();
        texts.push(op);
        Some(texts)
    });
    let have = with_local(|l| l.candidate.clone()).filter(|(p, _)| *p == photo).map(|(_, c)| c);
    if want != have
        && let Some(s) = app.session_mut()
        && s.preview_photo() == Some(photo)
        && s.set_preview_candidate(want.as_deref()).is_ok()
    {
        with_local(|l| l.candidate = want.map(|w| (photo, w)));
    }

    // The circles: the waiting target, and the brush at the pointer.
    ctx.set_sublayer(above, overlay_layer());
    let painter = ctx.layer_painter(overlay_layer()).with_clip_rect(view.area);
    let t = *app.tokens();
    let ring = |centre: Pos2, radius: f32| {
        painter.circle_stroke(centre, radius + 1.25, Line::new(1.0, t.face_box_outer));
        painter.circle_stroke(centre, radius, Line::new(1.5, t.face_box));
    };
    let target_at = waiting.and_then(|at| Some((to_screen(&g, &view, at)?, screen_radius(&g, dims, &view, at, d.brush)?)));
    if let Some((c, r)) = target_at {
        ring(c, r.max(2.0));
    }
    if let Some((p, at)) = pointer
        && let Some(stored) = to_stored(&g, at)
        && let Some(r) = screen_radius(&g, dims, &view, stored, d.brush)
    {
        if let Some((c, _)) = target_at {
            painter.line_segment([c, p], Line::new(1.0, t.face_box_outer));
        }
        ring(p, r.max(2.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brush_stays_within_what_the_operation_stores() {
        // 3:2: the operation's 0.1 of the shorter side is 0.0667 of the longer.
        let (lo, hi) = brush_range(Some((3000.0, 2000.0)));
        assert_eq!(lo, BRUSH_MIN);
        assert!((hi - 0.1 * 2.0 / 3.0).abs() < 1e-6, "{hi}");
        assert!((op_radius(hi, (3000.0, 2000.0)) - MAX_RADIUS).abs() < 1e-6);
        assert!((op_radius(BRUSH_DEFAULT, (2000.0, 3000.0)) - 0.045).abs() < 1e-6);
        // A long panorama: never an empty range.
        let (lo, hi) = brush_range(Some((100_000.0, 100.0)));
        assert!(lo <= hi && lo >= BRUSH_MIN, "{lo} {hi}");
        // An unknown or hostile size: the square's range.
        for dims in [None, Some((0.0, 10.0)), Some((f64::NAN, 1.0))] {
            let (lo, hi) = brush_range(dims);
            assert!(lo.is_finite() && hi.is_finite() && lo <= hi, "{dims:?}");
        }
        assert_eq!(op_radius(f32::NAN, (10.0, 10.0)), MIN_RADIUS);
        assert_eq!(op_radius(1.0, (10.0, 0.0)), MAX_RADIUS);
    }

    #[test]
    fn the_buttons_stay_in_the_content_and_reset_joins_the_patch_row_when_short() {
        let t = Tokens::DARK;
        let tall = layout(&t, Rect::from_min_size(pos2(8.0, 104.0), vec2(264.0, 480.0)), 32.0);
        assert_eq!(tall.undo.min.y, tall.redo.min.y);
        assert!(tall.reset.min.y > tall.undo.max.y && tall.apply.min.y > tall.reset.max.y);
        assert_eq!((tall.undo.width(), tall.reset.min.x, tall.cancel.max.x), (129.0, 8.0, 272.0));
        // The edit panel of a 900 × 600 window: the content is 178 tall.
        let short = layout(&t, Rect::from_min_size(pos2(8.0, 104.0), vec2(264.0, 178.0)), 32.0);
        assert_eq!((short.undo.min.y, short.redo.min.y), (short.reset.min.y, short.reset.min.y));
        assert!(short.undo.max.x < short.redo.min.x && short.redo.max.x < short.reset.min.x && short.reset.max.x <= 272.0);
        assert!(short.apply.min.y > short.reset.max.y && short.apply.max.y <= 104.0 + 178.0);
        // Tiny or hostile: Apply and Cancel inside, nothing NaN.
        for (h, ih) in [(30.0, 32.0), (0.0, f32::NAN), (100.0, f32::INFINITY)] {
            let l = layout(&t, Rect::from_min_size(pos2(0.0, 0.0), vec2(264.0, h)), ih);
            assert!(l.apply.min.y >= 0.0 && l.apply.min.y.is_finite() && l.cancel.max.x <= 264.0, "{h} {ih}: {l:?}");
        }
    }

    #[test]
    fn the_entry_lists_the_patches_in_order() {
        assert_eq!(op_text(&[], (300.0, 200.0)), None);
        let patches =
            [Patch { target: [0.25, 0.5], source: [0.75, 0.5], radius: 0.02 }, Patch { target: [0.5, 0.25], source: [0.5, 0.75], radius: 0.04 }];
        let text = op_text(&patches, (300.0, 200.0)).unwrap();
        assert_eq!(text, "retouch(0.75,0.5,0.25,0.5,0.03;0.5,0.75,0.5,0.25,0.06)");
        let recipe = Recipe::parse(&[text]).unwrap();
        assert_eq!(recipe.pipeline().count(), 1);
    }
}
