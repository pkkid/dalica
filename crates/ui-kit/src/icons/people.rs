//! The glyphs of people and places (`organizing.md` §6–§7, `design-system.md` §5): the People
//! group, the Unnamed row, ignored faces, Add a Person Manually, and the Places pane's map, pins
//! and zoom buttons; our own drawings on the 20 × 20 grid with the shell icons' pen.
//!
//! They are a set of their own, [`PeopleIcon`] ([`paint_people`]), so the other sets' sheets and
//! fingerprints stay as they are. Content stays inside 2 … 18 so the stroke never leaves the square.
//! Meanings the kit already draws are not drawn twice: **People** is the shell's two people (the
//! People pane toggle), **Places** its folded map, **Map pin** its Geotagged pin and **Map zoom in**
//! its plus; the set names them so a panel takes every glyph of people and places from one place.
//! The new drawings share one bust (a head over a curve of shoulders) so they read as one family:
//! **Person** (the bust alone, centred), **Unnamed** (a smaller bust in the four corners of a
//! viewfinder: a face found, not yet named), **Ignored face** (the bust with a "none" sign at its
//! top right, apart from the tray's bare Clear sign and the import tray's Exclude), **Add person**
//! (the bust with a plus at its top right), **Marker** (a solid pin: photos on the map), **Pin
//! tool** (a smaller outlined pin planted in a ring on the ground) and **Map zoom out** (the plus's
//! bar alone; the title bar's Minimize is the same shape in another place).

use egui::{Color32, Painter, Rect};

use super::Icon;
use super::pen::{Pen, arc_points};

/// The glyphs of people and places. Keep [`PeopleIcon::ALL`] in declaration order (a test checks
/// it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeopleIcon {
    /// One person, head and shoulders: a person's row and section.
    Person,
    /// Two people: the People group (the shell's [`Icon::People`]).
    People,
    /// A small person in the corners of a viewfinder: the Unnamed row (faces found, not named).
    Unnamed,
    /// A person with a "none" sign: ignored faces.
    IgnoredFace,
    /// A person with a plus: Add a Person Manually.
    AddPerson,
    /// A folded map: the Places pane (the shell's [`Icon::Places`]).
    Places,
    /// An outlined pin with a hole: a place on the map (the shell's [`Icon::Geotagged`]).
    MapPin,
    /// A solid pin: photos on the map.
    Marker,
    /// A small outlined pin planted in a ring: the pane's pin tool.
    PinTool,
    /// A plus: the map's zoom in (the shell's [`Icon::Plus`]).
    MapZoomIn,
    /// A bar: the map's zoom out.
    MapZoomOut,
}

impl PeopleIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [PeopleIcon; 11] = [
        PeopleIcon::Person,
        PeopleIcon::People,
        PeopleIcon::Unnamed,
        PeopleIcon::IgnoredFace,
        PeopleIcon::AddPerson,
        PeopleIcon::Places,
        PeopleIcon::MapPin,
        PeopleIcon::Marker,
        PeopleIcon::PinTool,
        PeopleIcon::MapZoomIn,
        PeopleIcon::MapZoomOut,
    ];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            PeopleIcon::Person => "person",
            PeopleIcon::People => "people",
            PeopleIcon::Unnamed => "unnamed",
            PeopleIcon::IgnoredFace => "ignored-face",
            PeopleIcon::AddPerson => "add-person",
            PeopleIcon::Places => "places",
            PeopleIcon::MapPin => "map-pin",
            PeopleIcon::Marker => "marker",
            PeopleIcon::PinTool => "pin-tool",
            PeopleIcon::MapZoomIn => "map-zoom-in",
            PeopleIcon::MapZoomOut => "map-zoom-out",
        }
    }

    /// The glyph called `name` ([`PeopleIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<PeopleIcon> {
        PeopleIcon::ALL.into_iter().find(|i| i.name() == name)
    }

    /// The shell icon this glyph is, for the meanings the shell already draws.
    pub fn shell(self) -> Option<Icon> {
        match self {
            PeopleIcon::People => Some(Icon::People),
            PeopleIcon::Places => Some(Icon::Places),
            PeopleIcon::MapPin => Some(Icon::Geotagged),
            PeopleIcon::MapZoomIn => Some(Icon::Plus),
            _ => None,
        }
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_people(painter: &Painter, rect: Rect, icon: PeopleIcon, color: Color32) {
    if let Some(shell) = icon.shell() {
        super::paint(painter, rect, shell, color);
        return;
    }
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

/// A bust: a head of radius `r` centred at (`x`, `y`) and the curve of the shoulders under it, `s`
/// wide each side of the middle.
fn bust(p: &Pen, x: f32, y: f32, r: f32, s: f32) {
    p.circle(x, y, r);
    // The shoulders: an arc of a circle of radius `s` whose top sits 1.4 under the head.
    let top = y + r + 1.4;
    p.arc(x, top + s, s, 205.0, 335.0);
}

/// The outline of a pin round (`cx`, `cy`) of radius `r` with its tip at (`cx`, `tip`): an arc over
/// the top from tangent to tangent, then the tip.
fn pin_outline(cx: f32, cy: f32, r: f32, tip: f32) -> Vec<(f32, f32)> {
    let t = (r / (tip - cy)).clamp(-1.0, 1.0).asin().to_degrees();
    let mut pts = arc_points(cx, cy, r, 180.0 - t, 360.0 + t);
    pts.push((cx, tip));
    pts
}

/// The "none" sign: a circle of radius `r` round (`x`, `y`) crossed from top left to bottom right.
fn none_sign(p: &Pen, x: f32, y: f32, r: f32) {
    p.circle(x, y, r);
    let d = r * std::f32::consts::FRAC_1_SQRT_2;
    p.seg((x - d, y - d), (x + d, y + d));
}

fn draw(p: &Pen, icon: PeopleIcon) {
    match icon {
        // Drawn by the shell (`paint_people` hands them over).
        PeopleIcon::People | PeopleIcon::Places | PeopleIcon::MapPin | PeopleIcon::MapZoomIn => {}
        PeopleIcon::Person => bust(p, 10.0, 7.2, 3.4, 6.8),
        PeopleIcon::Unnamed => {
            // A viewfinder's four corners round a smaller bust.
            p.elbow(&[(2.5, 6.5), (2.5, 2.5), (6.5, 2.5)]);
            p.elbow(&[(13.5, 2.5), (17.5, 2.5), (17.5, 6.5)]);
            p.elbow(&[(17.5, 13.5), (17.5, 17.5), (13.5, 17.5)]);
            p.elbow(&[(6.5, 17.5), (2.5, 17.5), (2.5, 13.5)]);
            bust(p, 10.0, 7.8, 2.5, 4.8);
        }
        PeopleIcon::IgnoredFace => {
            bust(p, 7.6, 8.0, 3.0, 6.0);
            none_sign(p, 14.8, 5.2, 3.2);
        }
        PeopleIcon::AddPerson => {
            bust(p, 7.6, 8.0, 3.0, 6.0);
            p.seg((15.5, 2.0), (15.5, 8.0));
            p.seg((12.5, 5.0), (18.0, 5.0));
        }
        PeopleIcon::Marker => p.fill(&pin_outline(10.0, 7.6, 5.4, 17.8)),
        PeopleIcon::PinTool => {
            // A ring on the ground (an ellipse), the pin's tip planted in its middle.
            let ring: Vec<(f32, f32)> = (0..36)
                .map(|i| {
                    let a = (i as f32 * 10.0).to_radians();
                    (10.0 + 6.4 * a.cos(), 15.4 + 2.2 * a.sin())
                })
                .collect();
            p.closed(&ring);
            p.closed(&pin_outline(10.0, 6.8, 4.4, 15.4));
            p.circle(10.0, 6.8, 1.5);
        }
        PeopleIcon::MapZoomOut => p.seg((4.0, 10.0), (16.0, 10.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in PeopleIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `MapZoomOut` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(PeopleIcon::MapZoomOut as usize + 1, PeopleIcon::ALL.len());
        for icon in PeopleIcon::ALL {
            assert_eq!(PeopleIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(PeopleIcon::from_name("face"), None);
    }

    #[test]
    fn pins_are_closed_round_the_top_and_end_at_their_tip() {
        let pts = pin_outline(10.0, 7.6, 5.4, 17.8);
        assert_eq!(pts.last().copied(), Some((10.0, 17.8)));
        assert!(pts.iter().all(|&(x, y)| (2.0..=18.0).contains(&x) && (2.0..=18.0).contains(&y)), "{pts:?}");
        // A degenerate pin (tip at the centre) still gives finite points.
        assert!(pin_outline(10.0, 10.0, 4.0, 10.0).iter().all(|&(x, y)| x.is_finite() && y.is_finite()));
    }
}
