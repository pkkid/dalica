//! The slideshow's zoom (`viewing.md` §1.3–§1.4): + and −, the bar's magnifier and slider zoom the
//! photo shown (1 fitted … [`SLIDE_ZOOM_MAX`]); zoomed, the show is paused (M11.4's Zoomed) and a
//! drag on the photo pans it.
//!
//! The view state keeps the zoom and the pan (`slideshow.zoom`, `slideshow.pan`: the point of the
//! photo at the centre of the screen, normalised). Here they become where the photo is painted
//! ([`zoomed_rect`]: its fitted rectangle grown by the zoom about that point, the pan kept so the
//! photo covers the screen wherever it is larger than it) and a drag becomes a new pan
//! ([`dragged_pan`]). The zoom grows the fitted photo, so it may go past the fit's 2× cap: that is
//! what zooming is for.

use dalica_create::slideshow::{Layer, Rect as SlideRect, Size};
use egui::{Response, Vec2};

use crate::app::App;
use crate::commands::slideshow as cmd;
use crate::state::SLIDE_ZOOM_MAX;

/// The magnifier's zoom from fitted (it goes back to fitted from any zoom).
pub const MAGNIFIER_ZOOM: f32 = 2.0;

/// A zoom that is a number within 1 … [`SLIDE_ZOOM_MAX`] (anything else is fitted).
fn sane_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() { zoom.clamp(1.0, SLIDE_ZOOM_MAX) } else { 1.0 }
}

/// The pan of one axis kept so a photo `extent` long on a screen `screen` long covers it: centred
/// when the photo is shorter than the screen.
fn clamp_axis(pan: f32, extent: f32, screen: f32) -> f32 {
    let pan = if pan.is_finite() { pan.clamp(0.0, 1.0) } else { 0.5 };
    if !(extent.is_finite() && screen.is_finite()) || extent <= screen || extent <= 0.0 {
        return 0.5;
    }
    let half = screen / 2.0 / extent;
    pan.clamp(half, 1.0 - half)
}

/// `pan` kept within what the photo fitted at `fit` and zoomed by `zoom` can show on `screen`.
pub fn clamp_pan(fit: SlideRect, screen: Size, zoom: f32, pan: [f32; 2]) -> [f32; 2] {
    let z = sane_zoom(zoom);
    [clamp_axis(pan[0], fit.width * z, screen.width), clamp_axis(pan[1], fit.height * z, screen.height)]
}

/// Where the photo fitted at `fit` is painted at `zoom` with `pan` on `screen`: its rectangle grown
/// by the zoom, placed so the pan's point of the photo is at the screen's centre (the pan clamped,
/// [`clamp_pan`]). At 1 it is `fit`.
pub fn zoomed_rect(fit: SlideRect, screen: Size, zoom: f32, pan: [f32; 2]) -> SlideRect {
    let z = sane_zoom(zoom);
    if z <= 1.0 {
        return fit;
    }
    let s = screen.sane();
    let [px, py] = clamp_pan(fit, s, z, pan);
    let (w, h) = (fit.width * z, fit.height * z);
    SlideRect::new(s.width / 2.0 - px * w, s.height / 2.0 - py * h, w, h)
}

/// The pan after the photo fitted at `fit` and zoomed by `zoom` is dragged by `delta` points: the
/// photo follows the pointer (clamped, [`clamp_pan`]). Fitted, the pan stays at the centre.
pub fn dragged_pan(fit: SlideRect, screen: Size, zoom: f32, pan: [f32; 2], delta: Vec2) -> [f32; 2] {
    let z = sane_zoom(zoom);
    if z <= 1.0 {
        return [0.5, 0.5];
    }
    let (w, h) = (fit.width * z, fit.height * z);
    let step = |d: f32, extent: f32| if d.is_finite() && extent > 0.0 { d / extent } else { 0.0 };
    clamp_pan(fit, screen, z, [pan[0] - step(delta.x, w), pan[1] - step(delta.y, h)])
}

/// The incoming photo's `layer` as the zoom shows it: its rectangle zoomed ([`zoomed_rect`]), its
/// clip the whole screen. Fitted, `layer` is unchanged.
pub fn zoom_layer(layer: Layer, screen: Size, zoom: f32, pan: [f32; 2]) -> Layer {
    if sane_zoom(zoom) <= 1.0 {
        return layer;
    }
    let mut out = layer;
    out.rect = zoomed_rect(layer.rect, screen, zoom, pan);
    out.clip.rect = SlideRect::screen(screen);
    out.clip.circle = None;
    out
}

/// The photo area's drag (`canvas`, sensing drags): while the show is zoomed a drag pans the photo
/// shown, `fit` being where it sits fitted on `screen`.
pub fn input(app: &mut App, canvas: &Response, fit: Option<SlideRect>, screen: Size) {
    let delta = canvas.drag_delta();
    if !canvas.dragged() || delta == Vec2::ZERO {
        return;
    }
    let Some(fit) = fit else { return };
    if cmd::show(app).is_none() {
        return;
    }
    if let Some(s) = app.state_mut().slideshow.as_mut()
        && s.zoom > 1.0
    {
        s.pan = dragged_pan(fit, screen, s.zoom, s.pan, delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Size = Size::new(1400.0, 900.0);

    fn fit() -> SlideRect {
        SlideRect::new(100.0, 0.0, 1200.0, 900.0)
    }

    #[test]
    fn fitted_is_the_fit_and_a_zoom_grows_it_about_the_pan() {
        assert_eq!(zoomed_rect(fit(), SCREEN, 1.0, [0.2, 0.9]), fit());
        let r = zoomed_rect(fit(), SCREEN, 2.0, [0.5, 0.5]);
        assert_eq!((r.width, r.height), (2400.0, 1800.0));
        assert_eq!((r.x + r.width / 2.0, r.y + r.height / 2.0), (700.0, 450.0), "the centre stays at the centre");
        // Panned to a corner: the photo still covers the screen.
        let r = zoomed_rect(fit(), SCREEN, 2.0, [0.0, 0.0]);
        assert_eq!((r.x, r.y), (0.0, 0.0));
        let r = zoomed_rect(fit(), SCREEN, 2.0, [1.0, 1.0]);
        assert!((r.right() - 1400.0).abs() < 1e-3 && (r.bottom() - 900.0).abs() < 1e-3, "{r:?}");
        // Hostile zooms and pans answer a sane rectangle.
        for (z, p) in [(f32::NAN, [0.5, 0.5]), (f32::INFINITY, [f32::NAN, -3.0]), (-2.0, [9.0, 0.5])] {
            let r = zoomed_rect(fit(), SCREEN, z, p);
            assert!(r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite(), "{z} {p:?}: {r:?}");
        }
    }

    #[test]
    fn a_drag_moves_the_photo_with_the_pointer_and_stops_at_its_edges() {
        let p = dragged_pan(fit(), SCREEN, 2.0, [0.5, 0.5], Vec2::new(-240.0, 0.0));
        assert!((p[0] - 0.6).abs() < 1e-5 && p[1] == 0.5, "dragged left, the photo shows more of its right: {p:?}");
        let p = dragged_pan(fit(), SCREEN, 2.0, [0.5, 0.5], Vec2::new(-1e6, 1e6));
        assert_eq!(p, clamp_pan(fit(), SCREEN, 2.0, [1.0, 0.0]));
        assert_eq!(dragged_pan(fit(), SCREEN, 1.0, [0.2, 0.2], Vec2::new(10.0, 10.0)), [0.5, 0.5], "fitted, nothing pans");
    }
}
