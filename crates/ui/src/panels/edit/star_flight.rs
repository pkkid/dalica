//! The star's flight in the edit view (`editing.md` §2.1, `design-system.md` §8; issue #755): when
//! the photo shown is starred by hand, a star rises a short way above the bottom area's star button
//! with one unhurried turn while a soft ring blooms round it, and both fade ([`STAR_SECONDS`]); when it is
//! unstarred, the filled star drops a little from the button, tilting, as it fades
//! ([`UNSTAR_SECONDS`]). Every pose is one pure function of the share of the flight played
//! ([`pose`]), on a sine ease-in-out: no snap at the start, no overshoot, no bounce.
//!
//! **Never in the way.** The star is written the moment it is asked for (the button, Ctrl+8, Space,
//! Picture ▸ Star): the flight only follows the photo's star flag, as the canvas sees it change from
//! one frame to the next. It is drawn on its own foreground layer, takes no input and holds nothing
//! back; a change mid-flight starts the new direction's flight from the button, and another photo
//! (or leaving the edit view, [`forget`]) ends it. Idle, it asks for no repaint.
//!
//! **Only by hand.** A flag change plays a flight only when the person's own input (a key or a
//! pointer button, with no control request driving the frame) came at most [`INPUT_WINDOW`] seconds
//! before; a star set over the control channel, the CLI or MCP never flies, so snapshots and
//! fingerprints stay as they were. With Options ▸ Use animations off nothing flies.

use dalica_core::PhotoId;
use dalica_ui_kit::Tokens;
use egui::epaint::{Mesh, PathShape};
use egui::{Color32, Context, Id, LayerId, Order, Pos2, Stroke, Vec2, pos2, vec2};

use crate::app::App;

/// How long a star's flight lasts, in seconds (the maintainer, 2026-11-06: "a full second", fun and
/// calm rather than quick).
pub const STAR_SECONDS: f64 = 1.0;
/// How long an unstar's drop lasts, in seconds (calm beside the burst).
pub const UNSTAR_SECONDS: f64 = 0.45;
/// How soon after the person's own key or click a star change must show to fly, in seconds.
pub const INPUT_WINDOW: f64 = 0.25;
/// The bottom area's star button, where every flight starts.
pub const BUTTON_ID: &str = "button:star";
/// How far the burst rises above the button, in points.
pub const RISE: f32 = 40.0;
/// The star button glyph's size, in points (`widgets::star_toggle`).
pub const GLYPH: f32 = 16.0;

/// Which way the flight goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The photo was starred: the burst.
    Star,
    /// The photo was unstarred: the drop.
    Unstar,
}

impl Kind {
    /// The flight's length, in seconds.
    pub fn seconds(self) -> f64 {
        match self {
            Kind::Star => STAR_SECONDS,
            Kind::Unstar => UNSTAR_SECONDS,
        }
    }
}

/// Where the flying star is at one moment, relative to the button glyph's centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// From the button glyph's centre, in points (y down).
    pub offset: Vec2,
    /// The star's size (its outer diameter), in points.
    pub size: f32,
    /// Its turn, in degrees clockwise.
    pub turn: f32,
    /// Its opacity, 0 … 1.
    pub alpha: f32,
    /// The burst's ring round it: radius in points and opacity.
    pub ring: Option<(f32, f32)>,
}

/// Sine ease-in-out: starts and ends at rest.
fn ease(x: f32) -> f32 {
    (1.0 - (std::f32::consts::PI * x.clamp(0.0, 1.0)).cos()) / 2.0
}

/// The share of `[a, b]` that `t` has passed, 0 … 1.
fn span(t: f32, a: f32, b: f32) -> f32 {
    ((t - a) / (b - a)).clamp(0.0, 1.0)
}

/// The pose `t` of the way through a flight of `kind` (clamped to 0 … 1; NaN is the start).
///
/// - **Star** (1 s): up 40 pt, one full turn, 16 to 24 pt, all on the ease; opaque to half way,
///   then eased out. The ring blooms from 10 to 40 pt on the ease, its opacity rising to 0.5 and
///   falling back on a sine arc, so it never appears or vanishes at once.
/// - **Unstar** (450 ms): down 18 pt, an eighth of a turn back (−45°), 16 to 12 pt, eased out of
///   sight over the whole drop.
pub fn pose(kind: Kind, t: f32) -> Pose {
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let u = ease(t);
    match kind {
        Kind::Star => Pose {
            offset: vec2(0.0, -RISE * u),
            size: GLYPH + 8.0 * u,
            turn: 360.0 * u,
            alpha: 1.0 - ease(span(t, 0.5, 1.0)),
            ring: Some((10.0 + 30.0 * u, 0.5 * (std::f32::consts::PI * t).sin().max(0.0))),
        },
        Kind::Unstar => Pose { offset: vec2(0.0, 18.0 * u), size: GLYPH - 4.0 * u, turn: -45.0 * u, alpha: 1.0 - u, ring: None },
    }
}

/// A flight in the air.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub kind: Kind,
    /// The photo it is for.
    pub photo: PhotoId,
    /// When it started, on the frame clock (`egui::InputState::time`).
    pub start: f64,
}

/// What the canvas remembers between frames.
#[derive(Debug, Clone, Copy)]
struct Watch {
    /// The photo shown and its star flag, last frame.
    seen: Option<(PhotoId, bool)>,
    /// When the person last pressed a key or a pointer button (no control request driving).
    input: f64,
    flight: Option<Flight>,
}

impl Default for Watch {
    fn default() -> Self {
        Watch { seen: None, input: f64::NEG_INFINITY, flight: None }
    }
}

fn watch_id() -> Id {
    Id::new("dalica-ui.starFlight")
}

/// The flight in the air, if one is (tests, agents).
pub fn flight(ctx: &Context) -> Option<Flight> {
    ctx.data(|d| d.get_temp::<Watch>(watch_id())).and_then(|w| w.flight)
}

/// Forgets the photo and any flight: the edit view was left.
pub fn forget(ctx: &Context) {
    ctx.data_mut(|d| d.remove_temp::<Watch>(watch_id()));
}

/// Whether this frame holds the person's own key or pointer-button event.
fn by_hand(ctx: &Context) -> bool {
    crate::control::automated().is_none()
        && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::PointerButton { .. })))
}

/// Follows `photo`'s star flag and draws the flight in the air. Call it every frame the edit view
/// shows `photo` (the canvas's focused side).
pub fn show(app: &App, ctx: &Context, photo: PhotoId) {
    let now = ctx.input(|i| i.time);
    let mut w = ctx.data(|d| d.get_temp::<Watch>(watch_id())).unwrap_or_default();
    if by_hand(ctx) {
        w.input = now;
    }
    let starred = app.session().and_then(|s| s.library().photo(photo)).is_some_and(|p| p.star);
    let animate = crate::scale::animations(ctx);
    match w.seen {
        Some((seen, was)) if seen == photo && was != starred => {
            let recent = crate::control::automated().is_none() && now - w.input <= INPUT_WINDOW;
            let kind = if starred { Kind::Star } else { Kind::Unstar };
            w.flight = (animate && recent).then_some(Flight { kind, photo, start: now });
        }
        Some((seen, _)) if seen != photo => w.flight = None,
        _ => {}
    }
    w.seen = Some((photo, starred));
    if !animate {
        w.flight = None;
    }
    if let Some(f) = w.flight {
        let t = (now - f.start) / f.kind.seconds();
        if t < 1.0 {
            if let Some(button) = ctx.read_response(Id::new(BUTTON_ID)) {
                paint(ctx, app.tokens(), button.rect.center(), &pose(f.kind, t as f32));
            }
            ctx.request_repaint();
        } else {
            w.flight = None;
        }
    }
    ctx.data_mut(|d| d.insert_temp(watch_id(), w));
}

/// The five points of a star of outer diameter `size` at `centre`, turned `turn` degrees: ten
/// points, outer and inner (0.42 of the outer radius) by turns, the first straight up.
pub fn star_points(centre: Pos2, size: f32, turn: f32) -> [Pos2; 10] {
    let outer = size / 2.0;
    let inner = outer * 0.42;
    std::array::from_fn(|i| {
        let a = (turn - 90.0 + 36.0 * i as f32).to_radians();
        let r = if i % 2 == 0 { outer } else { inner };
        pos2(centre.x + r * a.cos(), centre.y + r * a.sin())
    })
}

/// Paints one pose over the window: the ring, then the star (`star`, filled) on a 1 pt
/// `crosshair_outer` edge so it reads over a bright photo.
fn paint(ctx: &Context, t: &Tokens, button: Pos2, p: &Pose) {
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, watch_id()));
    let centre = button + p.offset;
    if let Some((radius, alpha)) = p.ring
        && alpha > 0.0
    {
        painter.circle_stroke(centre, radius, Stroke::new(2.0, t.star.gamma_multiply(alpha)));
    }
    if p.alpha <= 0.0 {
        return;
    }
    let points = star_points(centre, p.size, p.turn);
    let fill: Color32 = t.star.gamma_multiply(p.alpha);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, fill);
    for q in points {
        mesh.colored_vertex(q, fill);
    }
    for i in 0..10u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
    }
    painter.add(mesh);
    painter.add(PathShape::closed_line(points.to_vec(), Stroke::new(1.0, t.crosshair_outer.gamma_multiply(p.alpha))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_start_on_the_button_and_end_out_of_sight() {
        for kind in [Kind::Star, Kind::Unstar] {
            let start = pose(kind, 0.0);
            assert_eq!((start.offset, start.size, start.turn, start.alpha), (Vec2::ZERO, GLYPH, 0.0, 1.0), "{kind:?}");
            let end = pose(kind, 1.0);
            assert!(end.alpha.abs() < 1e-6 && end.ring.is_none_or(|(_, a)| a.abs() < 1e-6), "{kind:?}");
            assert_eq!(pose(kind, f32::NAN), start, "{kind:?}");
            assert_eq!(pose(kind, 7.0), end, "{kind:?}");
        }
        let star = pose(Kind::Star, 1.0);
        assert_eq!((star.offset, star.size, star.turn), (vec2(0.0, -RISE), 24.0, 360.0));
        let drop = pose(Kind::Unstar, 1.0);
        assert_eq!((drop.offset, drop.size, drop.turn), (vec2(0.0, 18.0), 12.0, -45.0));
    }

    #[test]
    fn the_ease_starts_softly_and_never_overshoots() {
        // The first frame of 60 Hz moves the star less than a tenth of a point: no snap.
        let first = pose(Kind::Star, (1.0 / 60.0 / STAR_SECONDS) as f32);
        assert!(first.offset.y.abs() < 0.1, "{first:?}");
        let mut last = 0.0;
        for i in 0..=100 {
            let y = -pose(Kind::Star, i as f32 / 100.0).offset.y;
            assert!(y >= last && y <= RISE, "monotonic, no overshoot at {i}");
            last = y;
        }
    }

    #[test]
    fn a_star_has_ten_points_round_its_centre() {
        let pts = star_points(pos2(10.0, 10.0), 16.0, 0.0);
        assert!((pts[0] - pos2(10.0, 2.0)).length() < 1e-4, "the first point straight up");
        assert!(pts.iter().all(|p| ((*p - pos2(10.0, 10.0)).length() - 8.0).abs() < 1e-4 || ((*p - pos2(10.0, 10.0)).length() - 3.36).abs() < 1e-4));
    }
}
