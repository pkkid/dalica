//! The standalone photo viewer (`viewing.md` §3): the photo over a dark translucent overlay
//! (`viewer_overlay`, black α210; opaque `viewer_bg` when chosen, Space toggles them), fitted above
//! the bottom band with "filename (W × H)" in white SemiBold over the band; the band holds the
//! filmstrip of the folder ([`strip`]) and the control bar ([`bar`]); "close ✕" at the top right.
//!
//! ```text
//!                                                                              close ✕
//!                  ┌──────────────────────────────────────────┐
//!                  │                  PHOTO                   │
//!                  └──────────────────────────────────────────┘
//!                           IMG_0151.jpg (3008 × 2000)
//! ═══════════════════════════════════════════════════════════════════════════════════
//!   [th][th][th][th][th][th][th][th][th][th][th][th][th][th][th]     ← filmstrip
//!          [+][−][1:1] [Open in Dalica]   ( ▶ )   [↺][↻] [★] [⋯]       ← control bar
//! ═══════════════════════════════════════════════════════════════════════════════════
//! ```
//!
//! - **Where it is.** The folder comes from the engine's listing ([`listing`]); the view state's
//!   `viewer.index` is the photo shown, put on the opened file's place when the listing arrives.
//! - **Pointer.** Dragging a zoomed photo pans it; a click on the overlay outside the photo closes
//!   the viewer (`viewer.close`). The wheel zooms about the pointer and Ctrl+wheel moves through the
//!   folder, or the other way round when Options ▸ Photo Viewer ▸ Mouse wheel says so
//!   (`viewer.wheel`, read every frame, so a change applies at once).
//! - **Controls.** The band, the bar and "close ✕" hide after [`HIDE_AFTER`] seconds without a
//!   mouse move when "Hide controls automatically" is on (fading over the overlay bar's fade), and
//!   show again at the next move; not while the More menu is open. The view state's
//!   `viewer.controls` says whether they are shown; set from outside (`ui.set`), it shows or hides
//!   them until the next move. The caption stays.
//! - **The background** follows "Translucent background" when the viewer opens and whenever the
//!   option changes; Space switches it for the session.
//!
//! Automation ids: the viewer `canvas:viewer` (labelled with the caption), the filmstrip
//! ([`strip`]), the bar and close ([`bar`]).

pub mod bar;
pub mod listing;
pub mod photo;
pub mod strip;

use dalica_core::PhotoId;
use dalica_engine::settings::WheelAction;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, OverlayFade, Target, overlay_bar_rect};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Id, MouseWheelUnit, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::state::Backdrop;

/// The viewer's automation id.
pub const CANVAS_ID: &str = "canvas:viewer";
/// The bottom band (filmstrip and bar), the caption line above it and the room kept at the top and
/// the sides of the photo (points).
pub const BAND_H: f32 = 140.0;
pub const CAPTION_H: f32 = 30.0;
pub const TOP: f32 = 56.0;
pub const SIDE: f32 = 16.0;
/// The controls hide after this long without a mouse move (seconds).
pub const HIDE_AFTER: f64 = 3.0;
/// The wheel's points per step on a trackpad (a notched wheel steps once per notch).
pub const WHEEL_STEP_POINTS: f32 = 40.0;
/// What the viewer says while the folder is read.
pub const READING: &str = "Reading the folder…";

/// Where the viewer's parts go in `window`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub photo: Rect,
    pub caption: Rect,
    pub band: Rect,
    pub bar: Rect,
}

impl Layout {
    pub fn of(window: Rect) -> Layout {
        let band = Rect::from_min_max(pos2(window.min.x, (window.max.y - BAND_H).max(window.min.y)), window.max);
        let caption = Rect::from_min_max(pos2(window.min.x, (band.min.y - CAPTION_H).max(window.min.y)), pos2(window.max.x, band.min.y));
        let photo = Rect::from_min_max(
            pos2(window.min.x + SIDE, (window.min.y + TOP).min(caption.min.y)),
            pos2((window.max.x - SIDE).max(window.min.x + SIDE), caption.min.y),
        );
        let bar = overlay_bar_rect(band, vec2(bar::BAR_W, bar::BAR_H));
        Layout { photo, caption, band, bar }
    }
}

/// What the viewer keeps between frames.
#[derive(Clone)]
struct Keep {
    fade: OverlayFade,
    last_move: Option<f64>,
    /// The wheel's steps not taken yet (a trackpad's small deltas add up).
    wheel: f32,
    /// "Translucent background" as last seen.
    translucent: Option<bool>,
    /// What the view state's `viewer.controls` was last set to here: a different value was set
    /// from outside (`ui.set`).
    written: Option<bool>,
    /// Hidden from outside: until the next mouse move.
    hidden: bool,
}

impl Default for Keep {
    fn default() -> Keep {
        Keep { fade: OverlayFade::new(true), last_move: None, wheel: 0.0, translucent: None, written: None, hidden: false }
    }
}

/// "filename (W × H)".
pub fn caption(name: &str, (w, h): (f32, f32)) -> String {
    format!("{name} ({} × {})", w.round() as u64, h.round() as u64)
}

/// Draws the viewer over `ui`'s whole rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let t = *app.tokens();
    let rect = ui.max_rect();
    let now = ctx.input(|i| i.time);
    let id = Id::new(CANVAS_ID);
    let mut keep = ctx.data_mut(|d| d.get_temp::<Keep>(id)).unwrap_or_default();
    frame(app, ui, &ctx, &t, rect, now, &mut keep);
    ctx.data_mut(|d| d.insert_temp(id, keep));
}

#[allow(clippy::too_many_arguments)]
fn frame(app: &mut App, ui: &mut Ui, ctx: &egui::Context, t: &Tokens, rect: Rect, now: f64, keep: &mut Keep) {
    let settings = app.session().map(|s| s.settings().viewer()).unwrap_or_default();
    if keep.translucent.is_some_and(|was| was != settings.translucent)
        && let Some(v) = app.state_mut().viewer.as_mut()
    {
        v.backdrop = if settings.translucent { Backdrop::Translucent } else { Backdrop::Opaque };
    }
    keep.translucent = Some(settings.translucent);
    let listed = listing::cached(app, ctx);
    let Some(v) = app.state().viewer else { return };
    let fill = match v.backdrop {
        Backdrop::Translucent => t.viewer_overlay,
        Backdrop::Opaque => t.viewer_bg,
    };
    ui.painter().rect_filled(rect, 0, fill);
    let layout = Layout::of(rect);

    // The controls: shown on a mouse move, hidden after a rest when the option says so; shown or
    // hidden from outside (`ui.set`) until the next move.
    if keep.written.is_none_or(|w| w != v.controls) {
        keep.hidden = !v.controls;
        keep.last_move = Some(now);
        keep.fade = OverlayFade::new(v.controls);
    }
    let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO || i.pointer.any_pressed());
    if moved || keep.last_move.is_none() {
        keep.last_move = Some(now);
        keep.hidden &= !moved;
    }
    let menu_open = egui::Popup::is_id_open(ctx, bar::more_popup());
    let rest = now - keep.last_move.unwrap_or(now);
    let show_controls = !keep.hidden && (!settings.hide_controls || menu_open || rest < HIDE_AFTER);
    keep.fade.set(show_controls, now);
    let opacity = keep.fade.opacity(now);
    if keep.fade.fading(now) {
        ctx.request_repaint();
    } else if show_controls && settings.hide_controls && !menu_open {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((HIDE_AFTER - rest).clamp(0.0, HIDE_AFTER)));
    }
    if let Some(v) = app.state_mut().viewer.as_mut() {
        v.controls = keep.fade.visible();
        keep.written = Some(v.controls);
    }

    let canvas = ui.interact(rect, Id::new(CANVAS_ID), Sense::click_and_drag());
    let shown_photo = match &listed {
        Ok(Some(l)) => l.at(v.index).map(|(i, p)| (i, p.clone())),
        _ => None,
    };
    let (Ok(Some(listing)), Some((index, photo))) = (listed.clone(), shown_photo) else {
        let text = match listed {
            Err(why) => why,
            Ok(Some(_)) => listing::EMPTY.to_owned(),
            Ok(None) => READING.to_owned(),
        };
        widgets::label_in(ui.painter(), layout.photo, Align2::CENTER_CENTER, &text, fonts::regular(t.body), t.overlay_icon);
        widgets::register(ctx, CANVAS_ID, Role::Other, &text, Target::Widget(&canvas), false);
        if bar::close_button(ui, t, rect, opacity) {
            let _ = app.run("viewer.close", &json!({}));
        }
        return;
    };

    // The wheel: zoom, or move through the folder.
    let hovered = canvas.hovered();
    let mut zoom_by = None;
    if hovered && let Some((n, ctrl, at)) = wheel(ctx, keep) {
        let navigate = (settings.wheel == WheelAction::Navigate) != ctrl;
        if navigate {
            let step = if n > 0 { "viewer.previous" } else { "viewer.next" };
            let _ = app.run(step, &json!({}));
        } else {
            zoom_by = Some((dalica_ui_kit::input::wheel::NOTCH.powi(n), at));
        }
    }
    // A step may have moved to another photo: draw the one now shown.
    let index = app.state().viewer.map_or(index, |v| v.index.min(listing.photos.len().saturating_sub(1)));
    let photo = listing.photos.get(index).cloned().unwrap_or(photo);

    let drag = if canvas.dragged_by(egui::PointerButton::Primary) { canvas.drag_delta() } else { Vec2::ZERO };
    let neighbours: Vec<PhotoId> =
        [index.checked_add(1), index.checked_sub(1)].into_iter().flatten().filter_map(|i| listing.photos.get(i).and_then(|p| p.id)).collect();
    let shown = photo::show(app, ui, layout.photo, &photo, &neighbours, &photo::Input { drag, wheel: zoom_by, hovered });
    let text = caption(&photo.name, shown.size);
    widgets::label_in(ui.painter(), layout.caption, Align2::CENTER_CENTER, &text, fonts::semibold(t.body), t.overlay_text);
    widgets::register(ctx, CANVAS_ID, Role::Other, &text, Target::Widget(&canvas), false);
    if canvas.clicked() && canvas.interact_pointer_pos().is_some_and(|p| !shown.image.contains(p)) {
        let _ = app.run("viewer.close", &json!({}));
        return;
    }

    if let Some(to) = strip::show(app, ui, layout.band, &listing, index, opacity) {
        crate::commands::viewer::go_to(app, to);
    }
    let held: Vec<PhotoId> = listing.photos.iter().filter_map(|p| p.id).collect();
    bar::show(app, ui, layout.bar, opacity, &photo, &held);
    if bar::close_button(ui, t, rect, opacity) {
        let _ = app.run("viewer.close", &json!({}));
    }
}

/// This frame's wheel over the viewer: whole steps (positive: up, away from the user), whether
/// Ctrl (Cmd on macOS) was held, and where the pointer is. A trackpad's small deltas add up to
/// steps of [`WHEEL_STEP_POINTS`].
fn wheel(ctx: &egui::Context, keep: &mut Keep) -> Option<(i32, bool, Pos2)> {
    let (delta, ctrl) = ctx.input(|i| {
        let mut sum = 0.0f32;
        let mut ctrl = false;
        for e in &i.events {
            if let egui::Event::MouseWheel { unit, delta, modifiers, .. } = e {
                let per_step = match unit {
                    MouseWheelUnit::Point => WHEEL_STEP_POINTS,
                    MouseWheelUnit::Line | MouseWheelUnit::Page => 1.0,
                };
                if delta.y.is_finite() && *delta != Vec2::ZERO {
                    sum += delta.y / per_step;
                    ctrl |= modifiers.command || modifiers.ctrl;
                }
            }
        }
        (sum, ctrl)
    });
    if delta == 0.0 {
        return None;
    }
    keep.wheel = (keep.wheel + delta).clamp(-100.0, 100.0);
    let n = keep.wheel.trunc();
    if n == 0.0 {
        return None;
    }
    keep.wheel -= n;
    let at = ctx.input(|i| i.pointer.hover_pos())?;
    Some((n as i32, ctrl, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_photo_sits_above_the_caption_and_the_band_at_every_size() {
        for (w, h) in [(1400.0, 900.0), (900.0, 600.0), (200.0, 100.0)] {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
            let l = Layout::of(window);
            assert!(l.photo.max.y <= l.caption.min.y && l.caption.max.y <= l.band.min.y, "{w}×{h}: {l:?}");
            assert!(window.contains_rect(l.band) && window.contains_rect(l.bar), "{w}×{h}: {l:?}");
        }
        assert_eq!(caption("IMG_0151.jpg", (3008.0, 2000.0)), "IMG_0151.jpg (3008 × 2000)");
    }
}
