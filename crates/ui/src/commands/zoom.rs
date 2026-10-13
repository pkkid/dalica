//! The edit view's zoom (`editing.md` §3): the slider maths, and the commands Fit, 1:1, the zoom
//! slider, Ctrl+= and Ctrl+−.
//!
//! **Scales.** A scale is device pixels per pixel of the photo as it shows (after its crop and
//! rotations): 1 is 100 %, [`ZOOM_MAX`] the slider's 400 % end. The *fitted* scale `fit` is the
//! largest that shows the whole photo inside the photo area less its padding, never above 1 (a
//! photo smaller than the area shows at 100 %). The edit state's `zoom` is `Fit` or a scale
//! ([`Zoom`]); a scale at or below the fitted one shows fitted ([`effective`]).
//!
//! **The slider.** v ∈ [0, 1]: 0 is fit, ½ is always 100 %, 1 is 400 %. With r = 1 / fit (the
//! actual size over the fitted size), the scale relative to fit is 1 + (2^(2v) − 1)(r − 1) on the
//! lower half and r · 2^(4(v − ½)) on the upper ([`relative_at`]); [`slider_at`] is its inverse.
//!
//! **Steps.** The wheel (×1.1 a notch, `dalica_ui_kit::input::wheel`) and Ctrl+= / Ctrl+− (×[`STEP`])
//! multiply the scale, land exactly on 100 % when they cross it, never go below fit (they become
//! `Fit`) nor above 400 % ([`step`]). The canvas keeps the image point under the pointer where it
//! was ([`anchor`]) and the photo over the whole area where it is larger than it ([`clamp`]).
//!
//! **Commands** (UI-only: they change `edit.zoom`; each refuses outside the edit view):
//! `edit.zoom.fit` (the bottom area's Fit), `edit.zoom.actual` (1:1), `edit.zoom.in` (Ctrl+=,
//! Ctrl++), `edit.zoom.out` (Ctrl+−), `edit.zoom.set {slider}` (the zoom slider). Fit and 1:1
//! animate on the canvas (0.22 s); the others jump. The fitted scale they need is the one the
//! canvas laid out last ([`crate::panels::edit::canvas::fitted`]); before the photo has been drawn
//! it counts as 1.

use dalica_engine::{CmdError, Context};
use egui::{Pos2, Vec2, pos2};
use serde::Deserialize;
use serde_json::{Value, json};

use super::edit_view::{NOT_EDITING, open_photo};
use super::{UiCommand, no_params, parse};
use crate::app::App;
use crate::panels::edit::canvas;
use crate::state::{ZOOM_MAX, ZOOM_MIN, Zoom};

/// One Ctrl+= / Ctrl+− step.
pub const STEP: f32 = 1.25;
/// Scales this close (relatively) count as equal: the snap to 100 %, fit.
const EPS: f32 = 1e-4;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand { id: "edit.zoom.fit", label: "Fit", menu: "", keys: &[], params: "{}: fit the photo to the photo area (animated)", run: fit },
    UiCommand {
        id: "edit.zoom.actual",
        label: "Actual Size",
        menu: "",
        keys: &[],
        params: "{}: show the photo at 100 % (1:1, animated)",
        run: actual,
    },
    UiCommand {
        id: "edit.zoom.in",
        label: "Zoom In",
        menu: "",
        keys: &[(Context::EditView, "Ctrl+="), (Context::EditView, "Ctrl+Plus")],
        params: "{}: zoom in one step (×1.25) about the pointer; lands on 100 % when crossing it; at most 400 %",
        run: zoom_in,
    },
    UiCommand {
        id: "edit.zoom.out",
        label: "Zoom Out",
        menu: "",
        keys: &[(Context::EditView, "Ctrl+-")],
        params: "{}: zoom out one step (÷1.25) about the pointer; lands on 100 % when crossing it; fit at the end",
        run: zoom_out,
    },
    UiCommand {
        id: "edit.zoom.set",
        label: "Zoom",
        menu: "",
        keys: &[],
        params: r#"{"slider": 0..1}: the zoom slider: 0 fit, 0.5 100 %, 1 400 %"#,
        run: set,
    },
];

/// The fitted scale of a photo `image` pixels in size (as it shows) in a box `area` device pixels
/// in size: the largest that shows it whole, never above 1 nor below [`ZOOM_MIN`]. 1 for sizes
/// that are not positive and finite.
pub fn fit_scale(image: (f32, f32), area: (f32, f32)) -> f32 {
    let ok = |v: f32| v.is_finite() && v > 0.0;
    if !(ok(image.0) && ok(image.1) && ok(area.0) && ok(area.1)) {
        return 1.0;
    }
    (area.0 / image.0).min(area.1 / image.1).clamp(ZOOM_MIN, 1.0)
}

/// `fit` made usable: finite, in `ZOOM_MIN ..= 1` (1 otherwise).
fn sane_fit(fit: f32) -> f32 {
    if fit.is_finite() && fit > 0.0 { fit.clamp(ZOOM_MIN, 1.0) } else { 1.0 }
}

/// The scale relative to fit at slider position `v` for r = actual size ÷ fitted size (r ≥ 1):
/// 1 + (2^(2v) − 1)(r − 1) for v ≤ ½, r · 2^(4(v − ½)) above. `v` is held to 0 … 1.
pub fn relative_at(v: f32, r: f32) -> f32 {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let r = if r.is_finite() { r.max(1.0) } else { 1.0 };
    if v <= 0.5 { 1.0 + ((2.0 * v).exp2() - 1.0) * (r - 1.0) } else { r * (4.0 * (v - 0.5)).exp2() }
}

/// The scale (1 is 100 %) at slider position `v` for a photo whose fitted scale is `fit`: never
/// below fit, at most [`ZOOM_MAX`].
pub fn scale_at(v: f32, fit: f32) -> f32 {
    let fit = sane_fit(fit);
    (fit * relative_at(v, 1.0 / fit)).clamp(fit, ZOOM_MAX)
}

/// The slider position of `scale` for a photo whose fitted scale is `fit`: [`scale_at`]'s inverse
/// (0 at or below fit, 1 at 400 % and above).
pub fn slider_at(scale: f32, fit: f32) -> f32 {
    let fit = sane_fit(fit);
    if !scale.is_finite() || scale <= fit * (1.0 + EPS) {
        return 0.0;
    }
    let s = scale.min(ZOOM_MAX);
    if s >= 1.0 {
        return (0.5 + s.log2() / 4.0).clamp(0.5, 1.0);
    }
    // fit < s < 1, so r > 1.
    let r = 1.0 / fit;
    let v = (1.0 + (s / fit - 1.0) / (r - 1.0)).log2() / 2.0;
    if v.is_finite() { v.clamp(0.0, 0.5) } else { 0.0 }
}

/// The scale `zoom` shows at for a photo whose fitted scale is `fit`.
pub fn effective(zoom: Zoom, fit: f32) -> f32 {
    let fit = sane_fit(fit);
    match zoom {
        Zoom::Scale(s) if s.is_finite() => s.clamp(fit, ZOOM_MAX),
        Zoom::Fit | Zoom::Scale(_) => fit,
    }
}

/// Whether `zoom` shows the photo larger than fitted.
pub fn is_zoomed(zoom: Zoom, fit: f32) -> bool {
    effective(zoom, fit) > sane_fit(fit) * (1.0 + EPS)
}

/// The zoom for `scale`: `Fit` at or below the fitted scale, else the scale (at most 400 %).
pub fn zoom_for(scale: f32, fit: f32) -> Zoom {
    let fit = sane_fit(fit);
    if !scale.is_finite() || scale <= fit * (1.0 + EPS) { Zoom::Fit } else { Zoom::Scale(scale.min(ZOOM_MAX)) }
}

/// One step from `from` by `factor` (above 1 zooms in): exactly 100 % when the step crosses it,
/// `Fit` at or below the fitted scale, at most 400 %. A factor that is not positive and finite
/// changes nothing.
pub fn step(from: f32, factor: f32, fit: f32) -> Zoom {
    let fit = sane_fit(fit);
    let from = if from.is_finite() { from.clamp(fit, ZOOM_MAX) } else { fit };
    if !(factor.is_finite() && factor > 0.0) {
        return zoom_for(from, fit);
    }
    let mut to = from * factor;
    if (from < 1.0 - EPS && to > 1.0) || (from > 1.0 + EPS && to < 1.0) {
        to = 1.0;
    }
    zoom_for(to, fit)
}

/// The photo's size in points at scale `s`: `size` pixels as it shows, `ppp` egui's pixels per
/// point.
pub fn drawn_size(size: (f32, f32), s: f32, ppp: f32) -> Vec2 {
    let ppp = if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 };
    let v = Vec2::new(size.0 * s / ppp, size.1 * s / ppp);
    if v.x.is_finite() && v.y.is_finite() && v.x >= 0.0 && v.y >= 0.0 { v } else { Vec2::ZERO }
}

/// Where the view's centre goes when the scale changes from `s0` to `s1` so that the image point
/// under `pointer` stays under it. `centre` is the image point (0 … 1 of the photo) shown at
/// `area_centre`; `size` is the photo's size in pixels as it shows and `ppp` egui's pixels per
/// point. Not yet held inside the photo ([`clamp`]).
pub fn anchor(centre: Pos2, area_centre: Pos2, pointer: Pos2, size: (f32, f32), ppp: f32, s0: f32, s1: f32) -> Pos2 {
    let (d0, d1) = (drawn_size(size, s0, ppp), drawn_size(size, s1, ppp));
    if !(d0.x > 0.0 && d0.y > 0.0 && d1.x > 0.0 && d1.y > 0.0) {
        return centre;
    }
    let off = pointer - area_centre;
    let q = pos2(centre.x + off.x / d0.x, centre.y + off.y / d0.y);
    let c = pos2(q.x - off.x / d1.x, q.y - off.y / d1.y);
    if c.x.is_finite() && c.y.is_finite() { c } else { centre }
}

/// `centre` held so the photo, `drawn` points in size, covers an area `area` points in size where
/// it can: centred on an axis where it is no larger than the area, else with no edge pulled inside.
pub fn clamp(centre: Pos2, drawn: Vec2, area: Vec2) -> Pos2 {
    let axis = |c: f32, d: f32, a: f32| {
        if !(d.is_finite() && a.is_finite()) || d <= a || d <= 0.0 {
            return 0.5;
        }
        let half = a / 2.0 / d;
        if c.is_finite() { c.clamp(half, 1.0 - half) } else { 0.5 }
    };
    pos2(axis(centre.x, drawn.x, area.x), axis(centre.y, drawn.y, area.y))
}

/// The fitted scale the canvas last laid out for the open photo (1 before it has drawn it).
fn current_fit(app: &App) -> f32 {
    open_photo(app).and_then(canvas::fitted).unwrap_or(1.0)
}

fn editing(app: &App) -> Result<(), CmdError> {
    open_photo(app).map(|_| ()).ok_or_else(|| CmdError::Disabled(NOT_EDITING.into()))
}

/// Sets the zoom; replies with it.
fn set_zoom(app: &mut App, zoom: Zoom) -> Result<Value, CmdError> {
    app.state_mut().edit.zoom = zoom;
    Ok(json!({ "zoom": zoom }))
}

fn fit(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    editing(app)?;
    set_zoom(app, Zoom::Fit)
}

fn actual(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    editing(app)?;
    set_zoom(app, Zoom::Scale(1.0))
}

fn by(app: &mut App, params: &Value, factor: f32) -> Result<Value, CmdError> {
    no_params(params)?;
    editing(app)?;
    let fit = current_fit(app);
    let from = effective(app.state().edit.zoom, fit);
    set_zoom(app, step(from, factor, fit))
}

fn zoom_in(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    by(app, params, STEP)
}

fn zoom_out(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    by(app, params, 1.0 / STEP)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Slider {
    slider: f32,
}

fn set(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Slider = parse(params)?;
    if !(p.slider.is_finite() && (0.0..=1.0).contains(&p.slider)) {
        return Err(CmdError::BadParams(format!("`slider` is 0 … 1, not {}", p.slider)));
    }
    editing(app)?;
    let fit = current_fit(app);
    let zoom = if p.slider <= 0.0 { Zoom::Fit } else { zoom_for(scale_at(p.slider, fit), fit) };
    set_zoom(app, zoom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_commands_refuse_outside_the_edit_view_and_bad_params() {
        let (_dir, s, ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        for id in ["edit.zoom.fit", "edit.zoom.actual", "edit.zoom.in", "edit.zoom.out"] {
            assert!(matches!(app.run(id, &json!({})), Err(CmdError::Disabled(m)) if m == NOT_EDITING), "{id}");
        }
        assert!(matches!(app.run("edit.zoom.set", &json!({ "slider": 0.5 })), Err(CmdError::Disabled(_))));
        app.run("view.edit", &json!({ "id": ids[0].get() })).unwrap();
        for (id, bad) in [
            ("edit.zoom.fit", json!({ "x": 1 })),
            ("edit.zoom.actual", json!([1])),
            ("edit.zoom.in", json!("in")),
            ("edit.zoom.out", json!({ "by": 2 })),
            ("edit.zoom.set", json!({})),
            ("edit.zoom.set", json!({ "slider": "half" })),
            ("edit.zoom.set", json!({ "slider": 1.5 })),
            ("edit.zoom.set", json!({ "slider": -0.1 })),
            ("edit.zoom.set", json!({ "slider": 0.5, "x": 1 })),
            ("edit.zoom.set", json!(0.5)),
        ] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
            assert_eq!(app.state().edit.zoom, Zoom::Fit, "{id} {bad}: nothing changed");
        }
        // Before the canvas has drawn, the fitted scale counts as 1.
        assert_eq!(app.run("edit.zoom.actual", &json!({})).unwrap(), json!({ "zoom": 1.0 }));
        assert_eq!(app.run("edit.zoom.in", &json!({})).unwrap(), json!({ "zoom": 1.25 }));
        assert_eq!(app.run("edit.zoom.out", &json!({})).unwrap(), json!({ "zoom": "fit" }), "100 % is the fitted scale here");
        assert_eq!(app.run("edit.zoom.set", &json!({ "slider": 1.0 })).unwrap(), json!({ "zoom": 4.0 }));
        assert_eq!(app.run("edit.zoom.set", &json!({ "slider": 0.0 })).unwrap(), json!({ "zoom": "fit" }));
        assert_eq!(app.run("edit.zoom.fit", &json!({})).unwrap(), json!({ "zoom": "fit" }));
    }

    #[test]
    fn anchor_keeps_the_point_under_the_pointer() {
        let (size, ppp, ac, p) = ((4000.0, 3000.0), 2.0, pos2(500.0, 400.0), pos2(620.0, 330.0));
        let c0 = pos2(0.4, 0.6);
        let c1 = anchor(c0, ac, p, size, ppp, 0.5, 0.75);
        let under = |c: Pos2, s: f32| {
            let d = drawn_size(size, s, ppp);
            pos2(c.x + (p.x - ac.x) / d.x, c.y + (p.y - ac.y) / d.y)
        };
        let (a, b) = (under(c0, 0.5), under(c1, 0.75));
        assert!((a - b).length() < 1e-5, "{a:?} {b:?}");
        assert_eq!(anchor(c0, ac, p, (0.0, 3000.0), ppp, 0.5, 0.75), c0, "no size, no move");
        assert_eq!(anchor(c0, ac, p, size, ppp, f32::NAN, 0.75), c0);
    }

    #[test]
    fn clamp_centres_small_axes_and_keeps_edges_out() {
        let area = Vec2::new(100.0, 100.0);
        assert_eq!(clamp(pos2(0.1, 0.9), Vec2::new(80.0, 400.0), area), pos2(0.5, 0.875));
        assert_eq!(clamp(pos2(0.0, 0.3), Vec2::new(200.0, 400.0), area), pos2(0.25, 0.3));
        assert_eq!(clamp(pos2(f32::NAN, 0.3), Vec2::new(200.0, f32::INFINITY), area), pos2(0.5, 0.5));
    }
}
