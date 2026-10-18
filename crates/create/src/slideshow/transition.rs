//! The slideshow's transitions as geometry (`viewing.md` §1.3): for a progress 0…1 through a
//! change from one photo to the next, where the outgoing and the incoming photo are drawn, how
//! opaque and through which clip, so the UI only paints.
//!
//! How a [`Frame`] is painted: the screen black, then the outgoing [`Layer`], then the incoming one.
//! A layer paints black over its clip, then its photo stretched to its rectangle and cut to its
//! clip, both at the layer's opacity. At progress 0 a frame holds only the outgoing photo, at 1
//! only the incoming one; in between both, for every transition but Cut.

use std::time::Duration;

use super::geometry::{Point, Rect, Size, fit, side, unit};

/// How long a change between two photos lasts (`viewing.md` §1.3). Cut takes no time; Pan and
/// Zoom changes photos over this long too, and moves each photo over its whole display time.
pub const TRANSITION_TIME: Duration = Duration::from_millis(800);

/// The Pan and Zoom range: each photo grows by 5 to 10 % over its display time.
pub const PAN_ZOOM_MIN: f32 = 0.05;
pub const PAN_ZOOM_MAX: f32 = 0.10;

/// The transitions of the slideshow's drop-up list, in its order (`viewing.md` §1.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transition {
    Cut,
    PanAndZoom,
    #[default]
    Dissolve,
    Wipe,
    Circle,
    Push,
    Rectangle,
}

impl Transition {
    /// Every transition, in the list's order.
    pub const ALL: [Transition; 7] = [
        Transition::Cut,
        Transition::PanAndZoom,
        Transition::Dissolve,
        Transition::Wipe,
        Transition::Circle,
        Transition::Push,
        Transition::Rectangle,
    ];

    /// The name the list shows.
    pub fn label(self) -> &'static str {
        match self {
            Transition::Cut => "Cut",
            Transition::PanAndZoom => "Pan and Zoom",
            Transition::Dissolve => "Dissolve",
            Transition::Wipe => "Wipe",
            Transition::Circle => "Circle",
            Transition::Push => "Push",
            Transition::Rectangle => "Rectangle",
        }
    }

    /// The machine name (settings, command parameters).
    pub fn id(self) -> &'static str {
        match self {
            Transition::Cut => "cut",
            Transition::PanAndZoom => "panAndZoom",
            Transition::Dissolve => "dissolve",
            Transition::Wipe => "wipe",
            Transition::Circle => "circle",
            Transition::Push => "push",
            Transition::Rectangle => "rectangle",
        }
    }

    /// The transition named `id` (see [`Transition::id`]), if any.
    pub fn from_id(id: &str) -> Option<Transition> {
        Transition::ALL.into_iter().find(|t| t.id() == id)
    }

    /// How long a change between two photos takes with this transition.
    pub fn duration(self) -> Duration {
        match self {
            Transition::Cut => Duration::ZERO,
            _ => TRANSITION_TIME,
        }
    }
}

/// A circle, in the screen's units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Circle {
    pub centre: Point,
    pub radius: f32,
}

/// Where a layer may paint: inside `rect` (always inside the screen) and, when `circle` is set,
/// inside that circle too.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clip {
    pub rect: Rect,
    pub circle: Option<Circle>,
}

/// One photo of a frame: its rectangle (the photo stretched to it), its opacity (0..=1) and its clip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layer {
    pub rect: Rect,
    pub opacity: f32,
    pub clip: Clip,
}

/// What to paint at one moment of a change: the outgoing photo under the incoming one. A photo
/// that would not show is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub outgoing: Option<Layer>,
    pub incoming: Option<Layer>,
}

/// One photo as the transitions need it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    /// Its size in pixels, as shown (after its rotation).
    pub size: (u32, u32),
    /// The point Pan and Zoom moves toward, normalised in the photo (a detected face, else the
    /// centre); clamped to 0..1.
    pub focus: Point,
    /// How much Pan and Zoom grows it over its display time, clamped to 5–10 %
    /// ([`pan_zoom_amount`] picks one per photo).
    pub zoom: f32,
    /// How far through its display time it is (0..1); only Pan and Zoom moves with it.
    pub motion: f32,
}

impl Slide {
    /// A photo of `size` pixels, its focus the centre, its zoom the smallest, at its start.
    pub fn new(size: (u32, u32)) -> Slide {
        Slide { size, focus: Point::CENTRE, zoom: PAN_ZOOM_MIN, motion: 0.0 }
    }
}

/// The Pan and Zoom amount of the photo at `index` of a show: 5 % to 10 % in 1 % steps, so
/// neighbouring photos move differently, the same every time the show plays.
pub fn pan_zoom_amount(index: usize) -> f32 {
    PAN_ZOOM_MIN + 0.01 * ((index % 6) as f32)
}

/// The eased progress: slow at both ends (smoothstep), 0 at 0 and 1 at 1.
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// The least share of the screen either photo keeps while a change is under way: smoothstep
/// rounds to exactly 1 in `f32` within about 1e-4 of the end (and underflows to 0 just after the
/// start), which would drop a photo early. One part in a million is far above an `f32` step, so a
/// clip of that share of any screen side keeps a positive width, and far below a visible pixel.
const MIN_SHARE: f32 = 1e-6;

/// The eased progress of a change at `t` (0..=1): exactly 0 at 0 and 1 at 1, and strictly between
/// [`MIN_SHARE`] and 1 − [`MIN_SHARE`] in between, so both photos show until the change ends.
fn eased(t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 { t.clamp(0.0, 1.0) } else { ease(t).clamp(MIN_SHARE, 1.0 - MIN_SHARE) }
}

/// Where a photo fitted at `fit` is drawn `motion` (0..1) through its display time under Pan and
/// Zoom: grown by up to `zoom` (5–10 %) about its focus point, so the view closes in on that point,
/// and clipped to its fitted rectangle so it never spills into the black around it. The rectangle
/// at motion 0 is the fit itself.
pub fn pan_zoom(fit: Rect, focus: Point, zoom: f32, motion: f32) -> (Rect, Rect) {
    let zoom = if zoom.is_finite() { zoom.clamp(PAN_ZOOM_MIN, PAN_ZOOM_MAX) } else { PAN_ZOOM_MIN };
    let focus = Point::new(if focus.x.is_finite() { unit(focus.x) } else { 0.5 }, if focus.y.is_finite() { unit(focus.y) } else { 0.5 });
    let scale = 1.0 + zoom * ease(unit(motion));
    let (fx, fy) = (fit.x + focus.x * fit.width, fit.y + focus.y * fit.height);
    let rect = Rect::new(fx + (fit.x - fx) * scale, fy + (fit.y - fy) * scale, fit.width * scale, fit.height * scale);
    (rect, fit)
}

/// The frame `progress` (0..1; anything else clamped, not finite → 0) through a change from
/// `outgoing` (none at the start of a show) to `incoming` with `transition`, on a black `screen`
/// at `pixels_per_point` (see [`fit`]).
///
/// - **Cut**: the outgoing photo until progress 1, then the incoming one.
/// - **Dissolve**: the incoming photo fades in over the outgoing one.
/// - **Pan and Zoom**: a dissolve between the photos as they move (see [`pan_zoom`]).
/// - **Wipe**: the incoming photo is uncovered from the left edge to the right.
/// - **Circle**: the incoming photo shows through a circle growing from the screen's centre.
/// - **Push**: the incoming photo comes in from the right and pushes the outgoing one out to the
///   left; the two never overlap.
/// - **Rectangle**: the incoming photo shows through a rectangle of the screen's shape growing from
///   its centre.
pub fn frame(transition: Transition, screen: Size, pixels_per_point: f32, outgoing: Option<&Slide>, incoming: &Slide, progress: f32) -> Frame {
    let s = screen.sane();
    let full = Rect::screen(s);
    let t = unit(progress);
    let e = eased(t);
    let (w, h) = (side(s.width), side(s.height));
    let placed = |slide: &Slide| -> (Rect, Rect) {
        let fitted = fit(slide.size, s, pixels_per_point);
        if transition == Transition::PanAndZoom {
            let (rect, clip) = pan_zoom(fitted, slide.focus, slide.zoom, slide.motion);
            (rect, clip.intersect(&full))
        } else {
            (fitted, full)
        }
    };
    let layer = |rect: Rect, opacity: f32, clip: Rect, circle: Option<Circle>| Layer {
        rect,
        opacity: unit(opacity),
        clip: Clip { rect: clip.intersect(&full), circle },
    };

    let out = outgoing.map(placed);
    let (in_rect, in_clip) = placed(incoming);
    let (out_layer, in_layer) = match transition {
        Transition::Cut => (out.map(|(r, c)| layer(r, 1.0, c, None)), (t >= 1.0).then(|| layer(in_rect, 1.0, in_clip, None))),
        Transition::Dissolve | Transition::PanAndZoom => (out.map(|(r, c)| layer(r, 1.0, c, None)), Some(layer(in_rect, e, in_clip, None))),
        Transition::Wipe => {
            let edge = w * e;
            (out.map(|(r, _)| layer(r, 1.0, Rect::new(edge, 0.0, w - edge, h), None)), Some(layer(in_rect, 1.0, Rect::new(0.0, 0.0, edge, h), None)))
        }
        Transition::Circle => {
            let radius = e * (w.hypot(h) / 2.0);
            let circle = Circle { centre: full.centre(), radius };
            (out.map(|(r, c)| layer(r, 1.0, c, None)), (radius > 0.0).then(|| layer(in_rect, 1.0, full, Some(circle))))
        }
        Transition::Push => {
            let shift = w * e;
            (
                out.map(|(r, _)| layer(r.translate(-shift, 0.0), 1.0, Rect::new(0.0, 0.0, w - shift, h), None)),
                Some(layer(in_rect.translate(w - shift, 0.0), 1.0, Rect::new(w - shift, 0.0, shift, h), None)),
            )
        }
        Transition::Rectangle => {
            let (rw, rh) = (w * e, h * e);
            (out.map(|(r, c)| layer(r, 1.0, c, None)), Some(layer(in_rect, 1.0, Rect::new((w - rw) / 2.0, (h - rh) / 2.0, rw, rh), None)))
        }
    };
    let shows = |l: &Layer| l.opacity > 0.0 && !l.clip.rect.is_empty() && l.clip.circle.is_none_or(|c| c.radius > 0.0);
    Frame { outgoing: out_layer.filter(|l| t < 1.0 && shows(l)), incoming: in_layer.filter(|l| t > 0.0 && shows(l)) }
}
