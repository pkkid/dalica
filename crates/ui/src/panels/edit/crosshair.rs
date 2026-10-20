//! The crosshair on the photo (`editing.md` §8.1): while Soft Focus, Focal B&W, Graduated Tint or
//! Focal Zoom's panel is open, ui-kit's crosshair (`crosshair:effect`) over the photo sets the
//! effect's centre (Graduated Tint's start) and, for Graduated Tint, the angle. Run after the
//! canvas (`panels/edit.rs`), so it works with the edit panel hidden too.
//!
//! - **Where it stands.** The effect's point `x,y` is normalised in the stored frame
//!   (`data-model.md` §3.3); it is shown through the photo's [`Geometry`] (the focused side's
//!   recipe: its straighten, crop, frames, rotations and flips), so a crosshair put on a turned,
//!   cropped, straightened or framed photo lands where it was put. The crosshair works on the
//!   photo as shown ([`Geometry::photo_area`] on the displayed picture): a Border's bands are not
//!   the photo, and the centre cannot be put on them.
//! - **The circle** (Soft Focus, Focal B&W, Focal Zoom) has the effect's Size as its radius, a
//!   share of the stored photo's shorter side as the renderers measure it.
//! - **Graduated Tint's handle** stands [`HANDLE_REACH`] of the shown photo's shorter side from the
//!   start, toward the angle (pulled in to stay on the photo); dragging it turns the angle (its
//!   distance does not count): degrees clockwise from the stored frame's up, measured where the
//!   renderer measures it (in units of the stored photo's shorter side).
//! - **Each drag is a change of the panel's session**: the values are set through
//!   [`effect_session::set_values`], so the preview follows and nothing reaches the recipe before
//!   Apply (Cancel records nothing). The drag under way is [`crate::state::EffectDraft::drag`]
//!   (`centre` or `extent`, from the stored point where it began).
//! - **Keys** ([`key`], through the effect tool): the arrows nudge the centre by [`NUDGE`] of the
//!   photo as shown (Shift: [`NUDGE_SHIFT`]) in the direction pressed, kept on the photo. An effect
//!   without a crosshair leaves the arrows to the keymap.
//!
//! Scripts set the same values with `edit.tool.action {name: "set", params: {x, y, angle}}`.

use dalica_core::PhotoId;
use dalica_edit::catalog::{self, Crosshair as Shape};
use dalica_edit::{Geometry, Point, Recipe, Rect as Area};
use dalica_engine::{CmdError, Key};
use dalica_image::Orientation;
use dalica_ui_kit::widgets::{self, Crosshair, ToastKind, crosshair_handle_id};
use egui::{Id, Pos2, Rect, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Map, Value, json};

use super::canvas;
use crate::app::App;
use crate::commands::{edit_view, effect_session, tools};
use crate::state::{CrosshairDrag, CrosshairPart, EffectDraft, EffectValue};

/// The crosshair's automation id.
pub const ID: &str = "crosshair:effect";
/// How far an arrow key moves the centre: a share of the photo as shown.
pub const NUDGE: f64 = 0.01;
/// How far Shift and an arrow move it.
pub const NUDGE_SHIFT: f64 = 0.1;
/// How far Graduated Tint's handle stands from the start: a share of the shown photo's shorter
/// side.
pub const HANDLE_REACH: f32 = 0.2;

/// The open effect's crosshair, or `None` when it has none.
fn shape(d: &EffectDraft) -> Option<Shape> {
    match catalog::lookup(&d.name)?.effect?.crosshair {
        Shape::None => None,
        s => Some(s),
    }
}

/// A number of the draft (its default when unset), `None` when it is not one.
fn number(d: &EffectDraft, name: &str) -> Option<f64> {
    match effect_session::value(d, name)? {
        EffectValue::Number(v) if v.is_finite() => Some(v),
        _ => None,
    }
}

/// The draft's point (`x`, `y`), in the stored frame.
fn centre_of(d: &EffectDraft) -> [f64; 2] {
    [number(d, "x").unwrap_or(0.5), number(d, "y").unwrap_or(0.5)]
}

fn unit(v: f64) -> f64 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 }
}

/// The photo as the open effect sees it: its geometry, and where the photo lies on the displayed
/// picture (normalised, ordered, not empty).
struct Shown {
    g: Geometry,
    area: Area,
}

impl Shown {
    /// The geometry of the recipe the panel works on (the focused side's), without the effect.
    fn of(app: &App, photo: PhotoId) -> Option<Shown> {
        let s = app.session()?;
        let p = s.library().photo(photo)?;
        if p.width == 0 || p.height == 0 {
            return None;
        }
        let recipe = Recipe::parse(tools::recipe(app, photo)).unwrap_or_default();
        let g = Geometry::new(&recipe, Orientation::default(), p.width, p.height);
        let area = g.photo_area()?.to_display(g.orientation());
        let ok = [area.x0, area.y0, area.x1, area.y1].iter().all(|v| v.is_finite()) && area.x1 - area.x0 > 1e-9 && area.y1 - area.y0 > 1e-9;
        ok.then_some(Shown { g, area })
    }

    /// Where the stored point `p` shows, normalised to the photo as shown ((0, 0) its top-left);
    /// outside 0..1 when a crop leaves it out.
    fn shown(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        let d = self.g.to_display(Point::new(p[0], p[1]))?;
        let a = &self.area;
        let s = [(d.x - a.x0) / (a.x1 - a.x0), (d.y - a.y0) / (a.y1 - a.y0)];
        s.iter().all(|v| v.is_finite()).then_some(s)
    }

    /// The stored point under `s` (normalised to the photo as shown, brought onto it).
    fn stored(&self, s: [f64; 2]) -> Option<[f64; 2]> {
        let a = &self.area;
        let d = Point::new(a.x0 + unit(s[0]) * (a.x1 - a.x0), a.y0 + unit(s[1]) * (a.y1 - a.y0));
        let p = self.g.to_stored(d)?;
        Some([unit(p.x), unit(p.y)])
    }

    /// The stored frame's axes (from its point (0, 0) to (1, 0) and to (0, 1)) as shown,
    /// normalised to the photo as shown.
    fn axes(&self) -> Option<([f64; 2], [f64; 2])> {
        let o = self.shown([0.0, 0.0])?;
        let x = self.shown([1.0, 0.0])?;
        let y = self.shown([0.0, 1.0])?;
        Some(([x[0] - o[0], x[1] - o[1]], [y[0] - o[0], y[1] - o[1]]))
    }

    /// The photo as shown on the screen, the picture drawn at `image`.
    fn on_screen(&self, image: Rect) -> Rect {
        let at = |x: f64, y: f64| pos2(image.min.x + x as f32 * image.width(), image.min.y + y as f32 * image.height());
        Rect::from_min_max(at(self.area.x0, self.area.y0), at(self.area.x1, self.area.y1))
    }
}

/// The stored frame's axes on the screen, the photo shown at `rect`.
fn screen_axes(axes: ([f64; 2], [f64; 2]), rect: Rect) -> (Vec2, Vec2) {
    let v = |a: [f64; 2]| vec2(a[0] as f32 * rect.width(), a[1] as f32 * rect.height());
    (v(axes.0), v(axes.1))
}

/// The direction of `angle` (degrees clockwise from the stored frame's up) on the screen, the
/// stored frame's axes there `ex` and `ey` (perpendicular: the picture is drawn at one scale).
/// `None` when an axis has no length.
pub fn direction(ex: Vec2, ey: Vec2, angle: f64) -> Option<Vec2> {
    let (lx, ly) = (ex.length(), ey.length());
    if !(lx > 0.0 && ly > 0.0 && lx.is_finite() && ly.is_finite() && angle.is_finite()) {
        return None;
    }
    let a = angle.to_radians();
    let d = ex / lx * a.sin() as f32 - ey / ly * a.cos() as f32;
    (d.length() > 0.0).then(|| d.normalized())
}

/// The angle (degrees clockwise from the stored frame's up, 0 … 360) of the screen vector `d`, the
/// stored frame's axes on the screen `ex` and `ey`; `None` for no direction.
pub fn angle_of(ex: Vec2, ey: Vec2, d: Vec2) -> Option<f64> {
    let (lx, ly) = (ex.length(), ey.length());
    if !(lx > 0.0 && ly > 0.0 && d.length() > 0.0 && d.x.is_finite() && d.y.is_finite()) {
        return None;
    }
    let (along_x, along_y) = (f64::from(d.dot(ex / lx)), f64::from(d.dot(ey / ly)));
    let a = along_x.atan2(-along_y).to_degrees().rem_euclid(360.0);
    // `rem_euclid` can round up to exactly 360.
    Some(if a >= 360.0 { 0.0 } else { a })
}

/// How far from `from` along the unit vector `dir` the point stays inside `rect`, up to `most`.
fn reach_inside(rect: Rect, from: Pos2, dir: Vec2, most: f32) -> f32 {
    let mut t = most.max(0.0);
    for (p, d, lo, hi) in [(from.x, dir.x, rect.min.x, rect.max.x), (from.y, dir.y, rect.min.y, rect.max.y)] {
        if d > 0.0 {
            t = t.min((hi - p) / d);
        } else if d < 0.0 {
            t = t.min((lo - p) / d);
        }
    }
    if t.is_finite() { t.max(0.0) } else { 0.0 }
}

/// Sets `values` on the open panel; a refusal says why.
fn set(app: &mut App, values: Map<String, Value>) {
    if let Err(e) = effect_session::set_values(app, &values) {
        app.toast(&crate::i18n::tr(&e.to_string()), ToastKind::Caution);
    }
}

fn set_drag(app: &mut App, drag: Option<CrosshairDrag>) {
    if let Some(d) = app.state_mut().edit.effect.as_mut()
        && d.drag != drag
    {
        d.drag = drag;
    }
}

/// The photo area's part of an open crosshair effect: the crosshair drawn over the photo, its
/// drags setting the centre and Graduated Tint's angle.
pub fn on_canvas(app: &mut App, ui: &mut Ui) {
    let Some(photo) = edit_view::open_photo(app) else { return };
    let Some(d) = app.state().edit.effect.clone() else { return };
    let Some(shape) = shape(&d) else { return };
    let Some(view) = canvas::view(ui.ctx()).filter(|v| v.photo == photo) else { return };
    let Some(shown) = Shown::of(app, photo) else { return };
    let Some(axes) = shown.axes() else { return };
    let rect = shown.on_screen(view.image);
    if !(rect.width() > 0.0 && rect.height() > 0.0 && rect.is_finite()) {
        return;
    }
    let centre_stored = centre_of(&d);
    let Some(centre) = shown.shown(centre_stored) else { return };
    let centre = centre.map(|v| unit(v) as f32);
    let (ex, ey) = screen_axes(axes, rect);
    let short = rect.width().min(rect.height());
    let to_screen = |p: [f32; 2]| pos2(rect.min.x + p[0] * rect.width(), rect.min.y + p[1] * rect.height());

    let radius = match shape {
        Shape::Circle => number(&d, "size").map(|s| s as f32 * ex.length().min(ey.length()) / short),
        _ => None,
    };
    let handle = match shape {
        Shape::Gradient => {
            let at = to_screen(centre);
            direction(ex, ey, number(&d, "angle").unwrap_or(0.0)).map(|dir| {
                let p = at + dir * reach_inside(rect, at, dir, HANDLE_REACH * short);
                [(p.x - rect.min.x) / rect.width(), (p.y - rect.min.y) / rect.height()]
            })
        }
        _ => None,
    };

    let t = *app.tokens();
    let mut child = ui.new_child(UiBuilder::new().id_salt("effect.crosshair").max_rect(view.area));
    child.set_clip_rect(view.area.intersect(ui.clip_rect()));
    let out = widgets::crosshair(&mut child, &t, ID, rect, &Crosshair { centre, radius, handle });

    // The drag under way.
    let ctx = ui.ctx();
    let on_centre = ctx.is_being_dragged(Id::new(ID));
    let on_handle = ctx.is_being_dragged(Id::new(crosshair_handle_id(ID)));
    if out.drag_started {
        let (part, from) = match (on_handle && !on_centre, handle) {
            (true, Some(h)) => (CrosshairPart::Extent, shown.stored(h.map(f64::from)).unwrap_or(centre_stored)),
            _ => (CrosshairPart::Centre, centre_stored),
        };
        set_drag(app, Some(CrosshairDrag { part, from: from.map(|v| unit(v) as f32) }));
    }
    if out.drag_stopped || !(on_centre || on_handle || out.drag_started) {
        set_drag(app, None);
    }

    // The values the drag set.
    let mut values = Map::new();
    let mut centre_now = centre;
    if let Some(c) = out.centre
        && let Some([x, y]) = shown.stored(c.map(f64::from))
    {
        values.insert("x".into(), json!(x));
        values.insert("y".into(), json!(y));
        centre_now = c;
    }
    if let Some(h) = out.handle
        && let Some(angle) = angle_of(ex, ey, to_screen(h) - to_screen(centre_now))
    {
        // To a hundredth of a degree: finer than a pointer sets it, and the entry reads `180`.
        values.insert("angle".into(), json!((angle * 100.0).round() / 100.0 % 360.0));
    }
    if !values.is_empty() {
        set(app, values);
    }
}

/// The arrows on an open crosshair effect: the centre nudged by [`NUDGE`] of the photo as shown
/// (Shift: [`NUDGE_SHIFT`]) the way the arrow points, kept on the photo. `None` for any other key,
/// or when the open effect has no crosshair (the keymap gets the key).
pub fn key(app: &mut App, key: &Key) -> Option<Result<Value, CmdError>> {
    if key.ctrl || key.alt {
        return None;
    }
    let (dx, dy) = match key.code.as_str() {
        "Left" => (-1.0, 0.0),
        "Right" => (1.0, 0.0),
        "Up" => (0.0, -1.0),
        "Down" => (0.0, 1.0),
        _ => return None,
    };
    let photo = edit_view::open_photo(app)?;
    let d = app.state().edit.effect.clone()?;
    shape(&d)?;
    let step = if key.shift { NUDGE_SHIFT } else { NUDGE };
    let moved = Shown::of(app, photo).and_then(|s| {
        let c = s.shown(centre_of(&d))?;
        s.stored([unit(c[0]) + dx * step, unit(c[1]) + dy * step])
    });
    let Some([x, y]) = moved else {
        return Some(Err(CmdError::Disabled("The photo's size is not known yet.".into())));
    };
    let mut values = Map::new();
    values.insert("x".into(), json!(x));
    values.insert("y".into(), json!(y));
    Some(effect_session::set_values(app, &values).map(|_| json!({ "x": x, "y": y })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn angles_turn_with_the_stored_frame() {
        // Upright, then turned a quarter clockwise (the stored x axis points down the screen, its
        // y axis left), at a scale.
        let upright = (vec2(300.0, 0.0), vec2(0.0, 200.0));
        let turned = (vec2(0.0, 300.0), vec2(-200.0, 0.0));
        for (angle, up, tu) in
            [(0.0, vec2(0.0, -1.0), vec2(1.0, 0.0)), (90.0, vec2(1.0, 0.0), vec2(0.0, 1.0)), (225.0, vec2(-1.0, 1.0), vec2(-1.0, -1.0))]
        {
            let d = direction(upright.0, upright.1, angle).unwrap();
            assert!((d - up.normalized()).length() < 1e-5, "{angle}: {d:?}");
            let d = direction(turned.0, turned.1, angle).unwrap();
            assert!((d - tu.normalized()).length() < 1e-5, "{angle} turned: {d:?}");
            assert!(near(angle_of(upright.0, upright.1, up * 7.0).unwrap(), angle), "{angle}");
            assert!(near(angle_of(turned.0, turned.1, tu * 7.0).unwrap(), angle), "{angle} turned");
        }
        for bad in [Vec2::ZERO, vec2(f32::NAN, 1.0)] {
            assert_eq!(angle_of(upright.0, upright.1, bad), None);
        }
        assert_eq!(direction(Vec2::ZERO, upright.1, 0.0), None);
        assert_eq!(direction(upright.0, upright.1, f64::NAN), None);
    }

    #[test]
    fn the_handle_stays_on_the_photo() {
        let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(100.0, 50.0));
        assert_eq!(reach_inside(r, pos2(60.0, 35.0), vec2(1.0, 0.0), 20.0), 20.0);
        assert_eq!(reach_inside(r, pos2(60.0, 35.0), vec2(0.0, -1.0), 40.0), 25.0);
        assert_eq!(reach_inside(r, pos2(110.0, 35.0), vec2(1.0, 0.0), 40.0), 0.0);
        assert_eq!(reach_inside(r, pos2(60.0, 35.0), vec2(0.0, 1.0), f32::NAN), 0.0);
    }
}
