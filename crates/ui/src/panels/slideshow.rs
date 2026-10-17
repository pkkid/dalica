//! The slideshow (`viewing.md` §1): the photo shown on pure black, filling the window, its
//! transitions, its caption and (M11.21) its control bar.
//!
//! Each frame the photo area (`canvas:slideshow`, the whole window) takes the mouse of §1.4 (a move
//! shows the bar and the show keeps playing, the wheel steps back and on, a click goes on only
//! while the bar is hidden), the show catches up with the session's clock
//! ([`crate::commands::slideshow::sync`]), and [`show`] paints the photos as the transition places
//! them, with the caption while captions are on. The previews cover the window in device pixels.
//! Over them the control bar ([`bar`]); zoomed, a drag pans the photo ([`zoom`]).

pub mod bar;
pub mod show;
pub mod transition;
pub mod zoom;

use std::time::Duration;

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Id, Sense, Ui};

use crate::app::App;
use crate::commands::slideshow as cmd;

/// The photo area's automation id.
pub const ID: &str = "canvas:slideshow";
/// The shortest time between two steps of the wheel (a trackpad sends many small deltas).
const WHEEL_STEP: f64 = 0.25;
/// How often a show asks for a frame while a change runs (and Pan and Zoom moves), and otherwise.
const MOVING: Duration = Duration::from_millis(16);
const WAITING: Duration = Duration::from_millis(50);

/// When the wheel last stepped (egui's time), per UI thread.
fn last_wheel(ctx: &egui::Context) -> f64 {
    ctx.data(|d| d.get_temp::<f64>(Id::new(ID).with("wheel"))).unwrap_or(f64::NEG_INFINITY)
}

/// Draws the slideshow over `ui`'s whole rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.viewer_bg);
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let px = |v: f32| if v.is_finite() { (v * ppp).round().clamp(1.0, 16_384.0) as u32 } else { 1 };
    cmd::set_fit((px(rect.width()), px(rect.height())));

    // The mouse (§1.4), before the show moves on.
    let response = ui.interact(rect, Id::new(ID), Sense::click_and_drag());
    let label = cmd::show(app).and_then(|s| s.photo()).map_or_else(|| "Slideshow".to_string(), |p| format!("Slideshow: photo {p}"));
    widgets::register(&ctx, ID, Role::Other, &label, Target::Widget(&response), false);
    let (moved, wheel, now) = ctx.input(|i| {
        let wheel: f32 = i.events.iter().filter_map(|e| if let egui::Event::MouseWheel { delta, .. } = e { Some(delta.y) } else { None }).sum();
        (i.pointer.delta() != egui::Vec2::ZERO, wheel, i.time)
    });
    if moved && response.hovered() {
        cmd::with_show(|s| s.mouse_moved());
    }
    if wheel != 0.0 && response.hovered() && now - last_wheel(&ctx) >= WHEEL_STEP {
        cmd::with_show(|s| s.wheel(wheel));
        ctx.data_mut(|d| d.insert_temp(Id::new(ID).with("wheel"), now));
    }
    if response.clicked() {
        cmd::with_show(|s| s.click());
    }

    let Some(frame) = cmd::sync(app) else {
        show::release();
        return;
    };
    let captions = app.session().is_some_and(|s| s.settings().slideshow().captions);
    let (zoom, pan) = cmd::show(app).map_or((1.0, [0.5, 0.5]), |s| (s.zoom, s.pan));
    let fit = show::paint(app, ui, rect, &frame, captions, (zoom, pan));
    zoom::input(app, &response, fit, dalica_create::slideshow::Size::new(rect.width(), rect.height()));
    let bar = bar::show(app, ui, rect, &frame);
    let v = frame.view;
    let moving = bar.fading
        || v.change.is_some()
        || (v.transition == dalica_create::slideshow::Transition::PanAndZoom && v.state == dalica_create::slideshow::State::Playing);
    ctx.request_repaint_after(if moving { MOVING } else { WAITING });
}
