//! The mouse wheel, the trackpad and the pinch over a photo (`editing.md` §3, §6.4): what one
//! frame's wheel input does to the view.
//!
//! Ported from PhotoCraft `crates/ui-egui/src/wheel_nav.rs` at revision 10fb9233 (MIT OR
//! Apache-2.0; `NOTICE`): [`classify`], the zoom applied the frame a notch arrives with its smoothed
//! tail ignored, [`configure`] and the Windows pinch fold ([`legacy_pinch_to_zoom`],
//! [`fold_legacy_pinch`]). Dalica's changes: no preferences (PhotoCraft's "Zoom with Scroll Wheel"
//! and "Zoom with Trackpad Pinch"); instead the kind of wheel decides, because the edit view zooms
//! with the wheel (`editing.md` §3) and a trackpad must still be able to pan a zoomed photo:
//!
//! - a notched mouse wheel (line or page units) zooms around the pointer, exactly ×1.1 per notch
//!   ([`NOTCH`]), applied the frame it arrives;
//! - a trackpad's two-finger scroll (point units) pans;
//! - ⇧ or ⌘/Ctrl + wheel scrolls sideways and ⌥/Alt + wheel pans (after [`configure`]);
//! - a pinch zooms around the pointer: egui's `Event::Zoom` on macOS and in browsers, and on
//!   Windows the Ctrl + wheel fractions [`fold_legacy_pinch`] turns back into a pinch.
//!
//! The kind and the Alt state of a gesture come from its wheel events and last until the next one,
//! so the smoothed tail egui feeds after a notch never turns into a pan.

use egui::{Context, Event, Id, Modifiers, MouseWheelUnit, RawInput, Vec2};

/// Zoom factor of one wheel notch: every step is exactly ×1.1.
pub const NOTCH: f32 = 1.1;

/// A Ctrl + wheel event this soon (seconds) after a pinch step still belongs to the pinch, even
/// when it happens to be a whole notch.
const PINCH_GAP: f64 = 0.25;

/// What the wheel does to the view this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wheel {
    /// Multiply the zoom by this factor around the pointer.
    Zoom(f32),
    /// Move the image by this many screen points (egui's content direction).
    Pan(Vec2),
}

/// Inputs of one frame's wheel gesture.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    /// Smoothed scroll delta (points, content direction); what panning uses.
    pub scroll: Vec2,
    /// This frame's wheel events for the gesture in progress, unsmoothed (points, content
    /// direction); what zooming uses.
    pub raw: Vec2,
    /// egui's pinch zoom factor (1 = none).
    pub zoom_delta: f32,
    /// ⌥/Alt was held for the wheel gesture in progress.
    pub alt: bool,
    /// The gesture in progress comes from a notched wheel (line or page units), not a trackpad.
    pub notched: bool,
    /// Points per wheel notch (egui's `line_scroll_speed`).
    pub notch: f32,
}

/// The view change for one frame of wheel input, if any.
pub fn classify(i: Input) -> Option<Wheel> {
    if i.zoom_delta.is_finite() && i.zoom_delta > 0.0 && i.zoom_delta != 1.0 {
        return Some(Wheel::Zoom(i.zoom_delta));
    }
    // One notch (`notch` points) is one ×1.1 step.
    let step = |dy: f32| {
        let notch = if i.notch.is_finite() && i.notch > 0.0 { i.notch } else { 40.0 };
        let f = NOTCH.powf(dy / notch);
        (f.is_finite() && f > 0.0 && f != 1.0).then_some(Wheel::Zoom(f))
    };
    match (i.alt, i.notched) {
        // ⇧ and ⌘/Ctrl fold the wheel into x: those still scroll sideways. The smoothed tail of a
        // notch is ignored.
        (false, true) if i.raw.y != 0.0 || i.scroll.y != 0.0 => step(i.raw.y),
        _ if !(i.scroll.x.is_finite() && i.scroll.y.is_finite()) || i.scroll == Vec2::ZERO => None,
        _ => Some(Wheel::Pan(i.scroll)),
    }
}

/// Set egui's wheel modifiers (once, at setup): ⌘/Ctrl + wheel scrolls sideways like ⇧ + wheel
/// instead of zooming, everywhere in the app, so only a real pinch reaches `zoom_delta`.
pub fn configure(ctx: &Context) {
    ctx.options_mut(|o| {
        o.input_options.zoom_modifier = Modifiers::NONE;
        o.input_options.horizontal_scroll_modifier = Modifiers::SHIFT | Modifiers::COMMAND;
    });
}

/// Turn a Windows touchpad pinch, which arrives as Ctrl + wheel in fractions of a notch, into
/// pinch events (`Event::Zoom`) at egui's Ctrl-scroll zoom speed (`notch` points per line,
/// `speed` per point). Ctrl + wheel in whole notches (a mouse wheel) is left to scroll sideways,
/// unless it comes within [`PINCH_GAP`] of a pinch step. `last` is the time of the previous pinch
/// step; returns the time of the latest one.
pub fn legacy_pinch_to_zoom(events: &mut [Event], now: f64, last: Option<f64>, notch: f32, speed: f32) -> Option<f64> {
    let ctrl_wheel = |e: &Event| match e {
        Event::MouseWheel { unit: MouseWheelUnit::Line, delta, modifiers, .. } if modifiers.ctrl => Some(*delta),
        _ => None,
    };
    let fraction = |d: Vec2| d.x.fract() != 0.0 || d.y.fract() != 0.0;
    let pinching = last.is_some_and(|t| (0.0..PINCH_GAP).contains(&(now - t))) || events.iter().filter_map(ctrl_wheel).any(fraction);
    if !pinching {
        return last;
    }
    let mut latest = last;
    for e in events.iter_mut() {
        if let Some(d) = ctrl_wheel(e) {
            let f = (speed * notch * (d.x + d.y)).exp();
            *e = Event::Zoom(if f.is_finite() && f > 0.0 { f } else { 1.0 });
            latest = Some(now);
        }
    }
    latest
}

fn pinch_id() -> Id {
    Id::new("dalica-wheel-legacy-pinch")
}

/// [`legacy_pinch_to_zoom`] for one frame's raw input (the app's `raw_input_hook`, on Windows).
pub fn fold_legacy_pinch(ctx: &Context, raw: &mut RawInput) {
    let (notch, speed) = ctx.options(|o| (o.input_options.line_scroll_speed, o.input_options.scroll_zoom_speed));
    let now = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
    let last = ctx.data(|d| d.get_temp::<f64>(pinch_id()));
    let latest = legacy_pinch_to_zoom(&mut raw.events, now, last, notch, speed);
    if latest != last
        && let Some(t) = latest
    {
        ctx.data_mut(|d| d.insert_temp(pinch_id(), t));
    }
}

/// The Alt state and the kind of the wheel gesture in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Gesture {
    alt: bool,
    notched: bool,
}

fn gesture_id() -> Id {
    Id::new("dalica-wheel-gesture")
}

/// Read this frame's wheel input. Call it every frame (hovered or not) so the gesture's Alt state
/// and kind follow the latest wheel event.
pub fn read(ctx: &Context) -> Option<Wheel> {
    let notch = ctx.options(|o| o.input_options.line_scroll_speed);
    let (scroll, zoom_delta, latest, wheel, page) = ctx.input(|i| {
        let latest = i.events.iter().rev().find_map(|e| match e {
            Event::MouseWheel { unit, modifiers, .. } => {
                Some(Gesture { alt: modifiers.alt && !modifiers.command && !modifiers.ctrl, notched: *unit != MouseWheelUnit::Point })
            }
            _ => None,
        });
        let wheel: Vec<(MouseWheelUnit, Vec2, Modifiers)> = i
            .events
            .iter()
            .filter_map(|e| match e {
                Event::MouseWheel { unit, delta, modifiers, .. } => Some((*unit, *delta, *modifiers)),
                _ => None,
            })
            .collect();
        (i.smooth_scroll_delta, i.zoom_delta(), latest, wheel, i.content_rect().height())
    });
    let gesture = match latest {
        Some(g) => {
            ctx.data_mut(|d| d.insert_temp(gesture_id(), g));
            g
        }
        None => ctx.data(|d| d.get_temp::<Gesture>(gesture_id())).unwrap_or_default(),
    };
    // The plain wheel events of this frame (⇧, ⌘/Ctrl and ⌥ wheels never zoom), unsmoothed.
    let raw = wheel.iter().filter(|(_, _, m)| !m.alt && !m.shift && !m.command && !m.ctrl).fold(Vec2::ZERO, |sum, (unit, delta, _)| {
        sum + *delta
            * match unit {
                MouseWheelUnit::Point => 1.0,
                MouseWheelUnit::Line => notch,
                MouseWheelUnit::Page => page,
            }
    });
    classify(Input { scroll, raw, zoom_delta, alt: gesture.alt, notched: gesture.notched, notch })
}
