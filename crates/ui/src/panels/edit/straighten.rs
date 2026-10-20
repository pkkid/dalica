//! The Straighten tool (`editing.md` §6.2): the dashed white grid over the photo and the floating
//! bar with its wide slider, Apply and Cancel.
//!
//! - **The draft** is `edit.straighten.angle`, in degrees on screen (positive turns the photo
//!   clockwise as it shows), −[`STRAIGHTEN_MAX`] … [`STRAIGHTEN_MAX`]. The tool opens at the angle
//!   the photo already has (its last `straighten(…)`), so re-opening it adjusts that angle.
//! - **Live.** While the draft differs from the photo's angle, the photo's recipe with the draft's
//!   `straighten(…)` appended is the preview's candidate (`Session::set_preview_candidate`): the
//!   canvas draws it turned and scaled so no corner is empty, a quick draft first and then the
//!   final. The candidate follows the draft whatever changed it (the slider, a key, an action,
//!   `ui.set`), and is dropped again on Apply and Cancel.
//! - **Stored frame.** `straighten(deg)` is measured in the stored frame, which turns the other
//!   way on screen when the recipe mirrors the photo (`data-model.md` §3.3); the draft is the
//!   on-screen angle, turned into the stored one for the candidate and for Apply.
//! - **Input.** The slider (`slider:straighten.angle`, its middle 0°) drags in 0.1° steps, a
//!   click jumps, a double-click goes back to 0°; ←/→ (and ↓/↑) nudge [`NUDGE`]°; Esc cancels;
//!   Apply (`button:panel.apply`) is one "Straighten" step of the photo's history through
//!   `photo.straighten.apply`; Cancel (`button:panel.cancel`) records nothing.
//! - **A│A's right side** (`editing.md` §4): the tool works on the variant's recipe
//!   (`tools::recipe`), and Apply appends the same `straighten(…)` the candidate holds as one step
//!   of the variant's history (`tools::variant_step`); the photo's own recipe is not touched.
//! - **Actions** (`edit.tool.action`): `set {angle}` sets the draft, `nudge {steps}` moves it by
//!   that many nudges, as the slider and the arrows do.

use std::cell::RefCell;

use dalica_core::PhotoId;
use dalica_edit::ops::straighten::Straighten as StraightenOp;
use dalica_edit::{Op, Recipe, Typed, guides};
use dalica_engine::{CmdError, Key};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, ToastKind};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Color32, CornerRadius, Id, Layout, Rect, Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use super::canvas;
use crate::app::App;
use crate::commands::edit_view;
use crate::commands::tools::{self, Tool};
use crate::state::{EditState, STRAIGHTEN_MAX, StraightenDraft, ToolKind};

/// The slider's automation id (`control-protocol.md` §7).
pub const SLIDER_ID: &str = "slider:straighten.angle";
/// Apply and Cancel on the floating bar (`control-protocol.md` §7).
pub const APPLY_ID: &str = "button:panel.apply";
pub const CANCEL_ID: &str = "button:panel.cancel";
/// The floating bar itself, which takes the clicks around its parts (`control-protocol.md` §7).
pub const BAR_ID: &str = "box:straighten";
/// One arrow-key nudge, in degrees.
pub const NUDGE: f32 = 0.1;
/// The slider's step, in degrees.
pub const STEP: f32 = 0.1;
/// The floating bar's fill (`editing.md` §6.2: black α180) and corner radius.
pub const BAR_FILL: Color32 = Color32::from_black_alpha(180);
pub const BAR_RADIUS: u8 = 6;
/// How far above the photo's bottom edge the bar floats, in points.
pub const BAR_LIFT: f32 = 10.0;
/// The bar's inner padding and the gap between its parts, in points.
const BAR_PAD: f32 = 10.0;
const BAR_GAP: f32 = 8.0;
/// The gap between Apply and Cancel, in points.
const BUTTON_GAP: f32 = 6.0;
/// The slider's width: half the photo's, within these bounds (points).
const SLIDER_MIN: f32 = 120.0;
const SLIDER_MAX: f32 = 420.0;
/// The slider's height (its hit area), in points.
const SLIDER_H: f32 = 20.0;
/// The grid: white at α170 (premultiplied), 1 pt, dashed.
const GRID_COLOR: Color32 = Color32::from_rgba_premultiplied(170, 170, 170, 170);
const GRID_DASH: f32 = 4.0;
const GRID_GAP: f32 = 4.0;
/// Angles are kept to this many decimals of a degree, as `photo.straighten.apply` stores them.
const DECIMALS: i32 = 3;

pub struct Straighten;

pub static STRAIGHTEN: Straighten = Straighten;

impl Tool for Straighten {
    fn kind(&self) -> ToolKind {
        ToolKind::Straighten
    }

    fn label(&self) -> &'static str {
        "Straighten"
    }

    fn open(&self, app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        let angle = edit.photo.map_or(0.0, |p| current(&tools::recipe(app, p)));
        edit.straighten = Some(StraightenDraft { angle });
        Ok(())
    }

    fn apply(&self, app: &mut App) -> Result<Value, CmdError> {
        let photo = edit_view::open_photo(app).ok_or_else(|| CmdError::Disabled("Open a photo in the edit view first.".into()))?;
        let angle = draft_angle(app);
        let edits = tools::recipe(app, photo);
        let result = if tools::on_variant(app, photo) {
            // The candidate's `straighten(…)`, rounded as the engine rounds it; none: the same angle.
            let op = candidate(&edits, angle).and_then(|mut r| r.pop());
            tools::variant_step(app, photo, op)?
        } else {
            let degrees = stored(angle, mirrored(&edits));
            app.run("photo.straighten.apply", &json!({ "id": photo, "degrees": degrees }))?
        };
        clear_candidate(app);
        Ok(result)
    }

    fn action(&self, app: &mut App, name: &str, params: &Value) -> Result<Value, CmdError> {
        let number = |key: &str| -> Result<f64, CmdError> {
            let only = params.as_object().is_some_and(|o| o.len() == 1 && o.contains_key(key));
            let v = params.get(key).and_then(Value::as_f64).filter(|v| only && v.is_finite());
            v.ok_or_else(|| CmdError::BadParams(format!("{name} takes {{\"{key}\": number}}")))
        };
        let angle = match name {
            "set" => {
                let a = number("angle")?;
                if a.abs() > f64::from(STRAIGHTEN_MAX) {
                    return Err(CmdError::BadParams(format!("`angle` is −{STRAIGHTEN_MAX} … {STRAIGHTEN_MAX} degrees")));
                }
                a as f32
            }
            "nudge" => {
                let steps = number("steps")?;
                if steps.abs() > 1000.0 {
                    return Err(CmdError::BadParams("`steps` is −1000 … 1000".into()));
                }
                nudged(draft_angle(app), steps as f32)
            }
            _ => return Err(CmdError::BadParams(format!("Straighten has no action `{}` (set, nudge)", name.chars().take(40).collect::<String>()))),
        };
        set_angle(app, angle);
        Ok(json!({ "angle": draft_angle(app) }))
    }

    /// The arrows nudge the angle; Esc drops the live preview and cancels.
    fn key(&self, app: &mut App, key: &Key) -> Option<Result<Value, CmdError>> {
        let steps = match key.to_string().as_str() {
            "Right" | "Up" => 1.0,
            "Left" | "Down" => -1.0,
            "Esc" => return Some(cancel(app)),
            _ => return None,
        };
        set_angle(app, nudged(draft_angle(app), steps));
        Some(Ok(json!({ "angle": draft_angle(app) })))
    }
}

// --- the angle -------------------------------------------------------------------------------

/// `v` to [`DECIMALS`] places, never `-0`.
fn round(v: f64) -> f64 {
    let k = 10f64.powi(DECIMALS);
    let r = (v * k).round() / k + 0.0;
    if r.is_finite() { r } else { 0.0 }
}

/// `angle` within the range (0 when it is not finite), on the slider's 0.1° steps when `snap`.
fn clamp_angle(angle: f32, snap: bool) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }
    let a = angle.clamp(-STRAIGHTEN_MAX, STRAIGHTEN_MAX);
    let a = if snap { (a / STEP).round() * STEP } else { a };
    (round(f64::from(a)) as f32).clamp(-STRAIGHTEN_MAX, STRAIGHTEN_MAX) + 0.0
}

/// `angle` moved by `steps` nudges of [`NUDGE`]°, on the tenths, within the range.
pub fn nudged(angle: f32, steps: f32) -> f32 {
    if !steps.is_finite() {
        return clamp_angle(angle, false);
    }
    let tenths = (clamp_angle(angle, false) * 10.0).round() + (steps * NUDGE * 10.0).round();
    clamp_angle(tenths / 10.0, false)
}

/// Whether the recipe mirrors the photo (then a stored angle turns the other way on screen).
fn mirrored(edits: &[String]) -> bool {
    Recipe::parse(edits).is_ok_and(|r| r.orientation().mirrored())
}

/// The straighten that counts in `edits` (the last one), in stored degrees; 0 without one.
fn stored_angle(edits: &[String]) -> f64 {
    edits.iter().rev().find_map(|t| if let Some(Ok(Typed::Straighten(s))) = Op::parse(t).typed() { Some(s.degrees) } else { None }).unwrap_or(0.0)
}

/// The photo's angle on screen, as the tool opens at it.
pub fn current(edits: &[String]) -> f32 {
    let a = stored_angle(edits);
    clamp_angle((if mirrored(edits) { -a } else { a }) as f32, false)
}

/// The stored degrees of the on-screen `angle`.
fn stored(angle: f32, mirrored: bool) -> f64 {
    let a = round(f64::from(clamp_angle(angle, false)));
    if mirrored { -a + 0.0 } else { a }
}

/// The recipe the preview shows for `angle`: `None` (the photo's own) when it is the angle the
/// photo has, else its recipe with that `straighten(…)` appended (only the last counts).
pub fn candidate(edits: &[String], angle: f32) -> Option<Vec<String>> {
    let degrees = stored(angle, mirrored(edits));
    if (degrees - round(stored_angle(edits))).abs() < 1e-9 {
        return None;
    }
    let op = StraightenOp::new(degrees)?;
    let mut recipe = edits.to_vec();
    recipe.push(format!("straighten({op})"));
    Some(recipe)
}

fn draft_angle(app: &App) -> f32 {
    app.state().edit.straighten.map_or(0.0, |d| d.angle)
}

// --- the live preview ------------------------------------------------------------------------

thread_local! {
    /// The candidate last handed to the session's preview on this thread (the UI thread), so a
    /// frame hands it again only when the draft changed.
    static SENT: RefCell<Option<(PhotoId, Option<Vec<String>>)>> = const { RefCell::new(None) };
}

/// Sets the draft to `angle` (clamped) and the preview to follow it.
fn set_angle(app: &mut App, angle: f32) {
    if let Some(d) = &mut app.state_mut().edit.straighten {
        d.angle = clamp_angle(angle, false);
    }
    sync_candidate(app);
}

/// Hands the preview the candidate of the draft, when it changed since it was last handed.
fn sync_candidate(app: &mut App) {
    let Some(photo) = edit_view::open_photo(app) else { return };
    let Some(draft) = app.state().edit.straighten else { return };
    let edits = tools::recipe(app, photo);
    let Some(session) = app.session_mut() else { return };
    if session.preview_photo() != Some(photo) {
        // The canvas opens the photo's preview; the next frame hands the candidate.
        return;
    }
    let want = candidate(&edits, draft.angle);
    let key = Some((photo, want));
    if SENT.with(|s| *s.borrow() == key) {
        return;
    }
    let recipe = key.as_ref().and_then(|(_, w)| w.as_deref());
    if session.set_preview_candidate(recipe).is_ok() {
        SENT.with(|s| *s.borrow_mut() = key);
    }
}

/// Back to the photo's own recipe in the preview.
fn clear_candidate(app: &mut App) {
    if let Some(s) = app.session_mut()
        && s.preview_photo().is_some()
    {
        let _ = s.set_preview_candidate(None);
    }
    SENT.with(|s| *s.borrow_mut() = None);
}

/// Cancel: the live preview dropped, the tool closed, nothing recorded.
fn cancel(app: &mut App) -> Result<Value, CmdError> {
    clear_candidate(app);
    app.run("edit.tool.cancel", &json!({}))
}

// --- drawing ---------------------------------------------------------------------------------

/// Where the floating bar's parts go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarLayout {
    pub bar: Rect,
    pub slider: Rect,
    pub buttons: Rect,
}

/// The floating bar over `photo` (the visible part of the photo) inside `area`: centred on the
/// photo, its bottom [`BAR_LIFT`] above the photo's bottom edge, the slider half the photo's width
/// (within its bounds, and narrower when the area is), then `buttons` wide for Apply and Cancel.
pub fn bar_layout(photo: Rect, area: Rect, buttons: f32, button_h: f32) -> BarLayout {
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    let photo = if photo.is_positive() && photo.is_finite() { photo } else { area };
    let buttons = finite(buttons, 0.0).clamp(0.0, 1000.0);
    let fixed = 2.0 * BAR_PAD + BAR_GAP + buttons;
    let room = finite(area.width() - 16.0 - fixed, 0.0).max(24.0);
    let slider_w = finite(photo.width() * 0.5, SLIDER_MIN).clamp(SLIDER_MIN, SLIDER_MAX).min(room);
    let h = finite(button_h, SLIDER_H).clamp(SLIDER_H, 100.0) + 2.0 * 6.0;
    let w = fixed + slider_w;
    let bottom = finite((photo.max.y - BAR_LIFT).min(area.max.y - BAR_LIFT), 0.0);
    let (lo, hi) = (finite(area.min.x, 0.0), finite(area.max.x - w, 0.0));
    let left = finite(photo.center().x - w / 2.0, lo).min(hi).max(lo);
    let top = (bottom - h).max(finite(area.min.y, 0.0));
    let bar = Rect::from_min_size(pos2(left.round(), top.round()), vec2(w.round(), h));
    let slider = Rect::from_min_size(pos2(bar.min.x + BAR_PAD, bar.center().y - SLIDER_H / 2.0), vec2(slider_w, SLIDER_H));
    let buttons = Rect::from_min_max(pos2(slider.max.x + BAR_GAP, bar.min.y), pos2(bar.max.x - BAR_PAD, bar.max.y));
    BarLayout { bar, slider, buttons }
}

/// The angle at `x` on a slider track from `x0` to `x1` (its middle 0°), on the 0.1° steps.
pub fn angle_at(x: f32, x0: f32, x1: f32) -> f32 {
    let w = x1 - x0;
    if !(w.is_finite() && w > 0.0 && x.is_finite()) {
        return 0.0;
    }
    let f = ((x - x0) / w).clamp(0.0, 1.0);
    clamp_angle((f * 2.0 - 1.0) * STRAIGHTEN_MAX, true)
}

/// The angle as the slider's tooltip shows it ("+3.5°", "-2°", "0°").
pub fn angle_text(angle: f32) -> String {
    let a = clamp_angle(angle, false);
    if a == 0.0 {
        return "0°".into();
    }
    let s = format!("{a:+.1}");
    format!("{}°", s.strip_suffix(".0").unwrap_or(&s))
}

/// The width of a text button with `label` (`widgets::text_button`'s rule).
fn button_width(ui: &Ui, t: &Tokens, label: &str) -> f32 {
    let w = ui.painter().layout_no_wrap(label.to_owned(), fonts::semibold(t.button_label), t.text).size().x;
    (w + 18.0).max(37.0).round()
}

/// Draws the grid and the floating bar over the photo area, and runs the bar.
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(photo) = edit_view::open_photo(app) else { return };
    if app.state().edit.straighten.is_none() {
        return;
    }
    // The draft may have changed outside the tool (`ui.set`): the preview follows it.
    sync_candidate(app);
    let ctx = ui.ctx().clone();
    let Some(v) = canvas::view(&ctx).filter(|v| v.photo == photo) else { return };
    let t = *app.tokens();
    let area = ui.max_rect();
    let painter = ui.painter_at(area);

    // The grid over the photo.
    let image = v.image;
    let mut shapes = Vec::new();
    for line in guides::grid(image.width(), image.height()) {
        let points: Vec<egui::Pos2> = line.iter().map(|[x, y]| image.lerp_inside(vec2(*x, *y))).collect();
        Shape::dashed_line_many(&points, Stroke::new(1.0, GRID_COLOR), GRID_DASH, GRID_GAP, &mut shapes);
    }
    painter.extend(shapes);

    // The floating bar.
    let buttons_w = button_width(ui, &t, "Apply") + BUTTON_GAP + button_width(ui, &t, "Cancel");
    let l = bar_layout(image.intersect(area), area, buttons_w, t.metrics.button_h);
    painter.rect_filled(l.bar, CornerRadius::same(BAR_RADIUS), BAR_FILL);
    // The bar takes the clicks around its parts (an open tool owns the canvas's anyway).
    let bar = ui.interact(l.bar, Id::new(BAR_ID), Sense::click_and_drag());
    widgets::register(ui.ctx(), BAR_ID, Role::Other, "Straighten", Target::Widget(&bar), false);

    if let Some(a) = slider(ui, &t, l.slider, draft_angle(app)) {
        set_angle(app, a);
    }

    let mut child = ui.new_child(UiBuilder::new().max_rect(l.buttons).layout(Layout::right_to_left(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = BUTTON_GAP;
    let cancel_clicked = widgets::text_button(&mut child, &t, CANCEL_ID, "Cancel", false).clicked();
    let apply_clicked = widgets::primary_button(&mut child, &t, APPLY_ID, "Apply").clicked();
    let result = if apply_clicked {
        Some(app.run("edit.tool.apply", &json!({})))
    } else if cancel_clicked {
        Some(cancel(app))
    } else {
        None
    };
    if let Some(Err(e)) = result {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// The wide slider on the bar: a track with a tick at its middle (0°) and the ring thumb. Drag
/// sets the angle on 0.1° steps, a click jumps there, a double-click goes back to 0°. Returns the
/// new angle when it changed.
fn slider(ui: &mut Ui, t: &Tokens, rect: Rect, angle: f32) -> Option<f32> {
    let m = &t.metrics;
    let ring = m.slider_ring;
    let resp = ui.interact(rect, Id::new(SLIDER_ID), Sense::click_and_drag());
    let (x0, x1) = (rect.min.x + ring + 1.0, rect.max.x - ring - 1.0);
    let wanted = if resp.double_clicked() {
        Some(0.0)
    } else if resp.dragged() || resp.drag_started() || resp.clicked() {
        resp.interact_pointer_pos().map(|p| angle_at(p.x, x0, x1))
    } else {
        None
    };
    let out = wanted.filter(|a| (a - angle).abs() > 1e-6);
    let shown = out.unwrap_or(angle);

    let p = ui.painter();
    let y = rect.center().y;
    let half = m.slider_track / 2.0;
    p.rect_filled(Rect::from_min_max(pos2(x0, y - half), pos2(x1, y + half)), 1.0, t.track);
    let mid = (x0 + x1) / 2.0;
    p.line_segment([pos2(mid, y - 5.0), pos2(mid, y + 5.0)], Stroke::new(1.0, t.track));
    let f = (shown + STRAIGHTEN_MAX) / (2.0 * STRAIGHTEN_MAX);
    let c = pos2(x0 + f.clamp(0.0, 1.0) * (x1 - x0), y);
    let hot = resp.hovered() || resp.dragged() || resp.is_pointer_button_down_on();
    p.circle_filled(c, ring, t.chrome);
    p.circle_stroke(c, ring - 1.0, Stroke::new(2.0, if hot { t.thumb_hover } else { t.thumb }));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        widgets::tooltip_now(&resp, t, &angle_text(shown), None);
    }
    resp.widget_info(|| egui::WidgetInfo::slider(true, f64::from(shown), "Straighten angle"));
    let max = f64::from(STRAIGHTEN_MAX);
    widgets::access_range(&resp, f64::from(shown), -max, max, f64::from(STEP), &angle_text(shown));
    widgets::register(ui.ctx(), SLIDER_ID, Role::Other, "Straighten angle", Target::Widget(&resp), resp.dragged());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn the_angle_is_read_from_the_last_straighten_and_turned_when_mirrored() {
        assert_eq!(current(&[]), 0.0);
        assert_eq!(current(&s(&["straighten(2)", "autofix()", "straighten(-3.5)"])), -3.5);
        assert_eq!(current(&s(&["straighten(2)", "flip_h()"])), -2.0, "a mirrored recipe turns the other way on screen");
        assert_eq!(current(&s(&["straighten(nope)"])), 0.0);
    }

    #[test]
    fn the_candidate_appends_the_drafts_straighten_unless_it_is_the_photos() {
        assert_eq!(candidate(&[], 0.0), None);
        assert_eq!(candidate(&[], 3.5), Some(s(&["straighten(3.5)"])));
        assert_eq!(candidate(&s(&["straighten(3.5)"]), 3.5), None);
        assert_eq!(candidate(&s(&["straighten(3.5)"]), 0.0), Some(s(&["straighten(3.5)", "straighten(0)"])));
        assert_eq!(candidate(&s(&["flip_h()"]), 2.0), Some(s(&["flip_h()", "straighten(-2)"])));
        assert_eq!(candidate(&[], 0.3), Some(s(&["straighten(0.3)"])), "no float noise");
    }

    #[test]
    fn nudges_move_by_tenths_within_the_range() {
        assert_eq!(nudged(0.0, 1.0), 0.1);
        assert_eq!(nudged(0.1, 1.0), 0.2);
        assert_eq!(nudged(0.0, -3.0), -0.3);
        assert_eq!(nudged(9.95, 1.0), 10.0);
        assert_eq!(nudged(10.0, 1.0), 10.0);
        assert_eq!(nudged(-10.0, -1.0), -10.0);
        assert_eq!(nudged(f32::NAN, 1.0), 0.1);
        assert_eq!(nudged(1.0, f32::INFINITY), 1.0);
        let mut a = 0.0;
        for _ in 0..35 {
            a = nudged(a, 1.0);
        }
        assert_eq!(a, 3.5, "no drift");
    }

    #[test]
    fn the_slider_maps_its_middle_to_zero_and_its_ends_to_the_range() {
        assert_eq!(angle_at(150.0, 100.0, 200.0), 0.0);
        assert_eq!(angle_at(100.0, 100.0, 200.0), -10.0);
        assert_eq!(angle_at(500.0, 100.0, 200.0), 10.0);
        assert_eq!(angle_at(167.5, 100.0, 200.0), 3.5);
        assert_eq!(angle_at(150.0, 200.0, 100.0), 0.0, "an empty track");
        assert_eq!(angle_at(f32::NAN, 100.0, 200.0), 0.0);
    }

    #[test]
    fn the_angle_text() {
        assert_eq!(angle_text(0.0), "0°");
        assert_eq!(angle_text(3.5), "+3.5°");
        assert_eq!(angle_text(-2.0), "-2°");
        assert_eq!(angle_text(f32::NAN), "0°");
    }

    #[test]
    fn the_bar_floats_centred_above_the_photos_bottom_edge() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
        let photo = Rect::from_min_max(pos2(100.0, 50.0), pos2(900.0, 650.0));
        let l = bar_layout(photo, area, 120.0, 24.0);
        assert!((l.bar.max.y - (photo.max.y - BAR_LIFT)).abs() <= 0.5, "{l:?}");
        assert!((l.bar.center().x - photo.center().x).abs() <= 0.5, "{l:?}");
        assert!(l.bar.contains_rect(l.slider) && l.bar.contains_rect(l.buttons));
        assert!(l.slider.max.x <= l.buttons.min.x);
        assert_eq!(l.slider.width(), 400.0, "half the photo's width");
        // A small area: the bar stays inside it.
        let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));
        let l = bar_layout(small, small, 120.0, 24.0);
        assert!(small.contains_rect(l.bar), "{l:?}");
        // Hostile sizes do not panic.
        let _ = bar_layout(Rect::NOTHING, Rect::NOTHING, f32::NAN, f32::INFINITY);
        let _ = bar_layout(Rect::EVERYTHING, Rect::EVERYTHING, -5.0, f32::NAN);
    }
}
