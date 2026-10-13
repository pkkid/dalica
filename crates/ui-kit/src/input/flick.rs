//! The release glide of a pan (`editing.md` §3): letting go of a drag that pans a zoomed photo
//! while the pointer is still moving keeps the view gliding that way, slowing smoothly to a stop.
//! Any click, new drag or key press stops it.
//!
//! Ported from PhotoCraft `crates/ui-egui/src/flick_pan.rs` at revision 10fb9233 (MIT OR
//! Apache-2.0; `NOTICE`). Dalica's change: no "Enable Flick Panning" preference; the glide is
//! always on. [`Flick`] is the pure state (velocity estimate and decay); [`frame`] drives it from
//! one canvas frame's input and returns the screen-space pan to apply.

use egui::{Context, Id, Vec2};

/// Pointer movement this recent (seconds) sets the release velocity.
const WINDOW: f64 = 0.08;
/// Time constant of the exponential friction (seconds): a fast flick stops in under a second.
pub const FLICK_TAU: f32 = 0.18;
/// A release slower than this (screen points per second) does not glide.
pub const FLICK_MIN_START: f32 = 150.0;
/// The glide stops below this speed.
const MIN_SPEED: f32 = 15.0;
/// Fastest glide (screen points per second); a hostile or glitchy delta can't fling the view away.
pub const FLICK_MAX_SPEED: f32 = 6000.0;
/// Longest frame step the glide integrates; a stalled frame doesn't jump the view.
pub const FLICK_MAX_DT: f32 = 0.1;
/// At most this many drag steps are kept (a frame storm cannot grow the list without end).
const MAX_SAMPLES: usize = 64;

/// One canvas's flick state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flick {
    /// Recent drag steps: (time, frame interval, screen delta).
    samples: Vec<(f64, f32, Vec2)>,
    /// A pan drag was in progress last frame.
    dragging: bool,
    /// The glide velocity in screen points per second, while gliding.
    velocity: Option<Vec2>,
    /// Time of the previous frame.
    last: Option<f64>,
}

impl Flick {
    /// Frame interval since the previous call, clamped to `0..=FLICK_MAX_DT`; records `now`.
    fn tick(&mut self, now: f64) -> f32 {
        let dt = self.last.map_or(0.0, |l| (now - l) as f32);
        self.last = Some(now);
        if dt.is_finite() { dt.clamp(0.0, FLICK_MAX_DT) } else { 0.0 }
    }

    /// A pan drag moved the pointer by `delta` this frame (`dt` after the previous one).
    pub fn track(&mut self, now: f64, dt: f32, delta: Vec2) {
        self.velocity = None;
        self.dragging = true;
        if delta.is_finite() && dt.is_finite() {
            self.samples.push((now, dt, delta));
        }
        self.samples.retain(|s| now - s.0 <= WINDOW);
        if self.samples.len() > MAX_SAMPLES {
            let extra = self.samples.len() - MAX_SAMPLES;
            self.samples.drain(..extra);
        }
    }

    /// Pointer velocity over the last moments before `now`, in screen points per second.
    pub fn release_velocity(&self, now: f64) -> Vec2 {
        let (mut d, mut t) = (Vec2::ZERO, 0.0f32);
        for &(at, dt, delta) in &self.samples {
            if now - at <= WINDOW {
                d += delta;
                t += dt;
            }
        }
        if t <= 1e-4 {
            return Vec2::ZERO;
        }
        let v = d / t;
        if !v.is_finite() {
            return Vec2::ZERO;
        }
        let speed = v.length();
        if speed > FLICK_MAX_SPEED { v * (FLICK_MAX_SPEED / speed) } else { v }
    }

    /// The drag ended at `now`: start gliding if the pointer was still moving fast enough.
    /// Returns whether a glide started.
    pub fn release(&mut self, now: f64) -> bool {
        self.dragging = false;
        let v = self.release_velocity(now);
        self.samples.clear();
        self.velocity = (v.length() >= FLICK_MIN_START).then_some(v);
        self.velocity.is_some()
    }

    /// Advance the glide by `dt` seconds: the screen-space distance moved, or `None` when not
    /// gliding (or it just came to rest).
    pub fn step(&mut self, dt: f32) -> Option<Vec2> {
        let v = self.velocity?;
        let dt = if dt.is_finite() { dt.clamp(0.0, FLICK_MAX_DT) } else { 0.0 };
        // Exact integral of v·e^(−t/τ) over the step, then the decayed velocity.
        let decay = (-dt / FLICK_TAU).exp();
        let moved = v * (FLICK_TAU * (1.0 - decay));
        let next = v * decay;
        self.velocity = (next.is_finite() && next.length() >= MIN_SPEED).then_some(next);
        moved.is_finite().then_some(moved)
    }

    /// Stop any glide (a click, new drag or key press).
    pub fn stop(&mut self) {
        self.velocity = None;
        self.samples.clear();
    }

    pub fn gliding(&self) -> bool {
        self.velocity.is_some()
    }
}

/// Drive canvas `id`'s flick for this frame. `pan_drag` is whether a drag panned the view by
/// `delta` (screen points) this frame. Returns the screen-space distance the glide moves the view
/// this frame (the same sense as a drag delta).
pub fn frame(ctx: &Context, id: Id, pan_drag: bool, delta: Vec2) -> Option<Vec2> {
    let (now, interrupt) = ctx.input(|i| {
        let key = i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
        (i.time, i.pointer.any_pressed() || key)
    });
    let mut flick = ctx.data(|d| d.get_temp::<Flick>(id)).unwrap_or_default();
    let dt = flick.tick(now);
    let moved = if pan_drag {
        flick.track(now, dt, delta);
        None
    } else {
        if flick.dragging {
            flick.release(now);
        }
        if interrupt {
            flick.stop();
        }
        flick.step(dt)
    };
    // A moving drag asks for one more frame, so its release is seen even if nothing else repaints.
    if flick.gliding() || (pan_drag && delta != Vec2::ZERO) {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, flick));
    moved
}
