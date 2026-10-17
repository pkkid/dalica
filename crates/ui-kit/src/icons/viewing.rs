//! The glyphs of the rooms, the slideshow, the photo viewer, the import tray and compare
//! (`viewing.md` §1.3 and §3.1, `organizing.md` §3, `ui-layout.md` §8, `design-system.md` §5): our
//! own drawings on the 20 × 20 grid with the shell icons' pen.
//!
//! They are a set of their own, [`ViewingIcon`] ([`paint_viewing`]), like the edit view's glyphs, so
//! the shell's icon sheet and its fingerprint stay as they are. Content stays inside 2 … 18 so the
//! stroke never leaves the square. The meanings the shell already draws are not drawn again: Import
//! is the shell's camera ([`super::Icon::Import`]), More its three dots and Close its cross; Play is
//! the edit view's ([`super::EditIcon::Play`]).
//!
//! Each glyph is drawn apart from the shell glyph it could be taken for: Previous and Next carry a
//! bar beside their triangle (the filmstrip's arrows have none), Exclude puts its "none" sign on a
//! picture (the tray's Clear is the bare sign), Exit's arrow leaves through a door frame (Back to
//! Library's arrow is bare) and Captions are two lines low in a screen (the slideshow's screen
//! holds a triangle).

use egui::{Color32, Painter, Rect};

use super::pen::Pen;

/// The viewing glyphs. Keep [`ViewingIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewingIcon {
    /// Four thumbnails in a square: the Library room tab.
    Library,
    /// An arrow leaving through a door frame on its right: Exit (the slideshow).
    Exit,
    /// Two upright bars: Pause.
    Pause,
    /// A triangle pointing left against a bar: Previous.
    Previous,
    /// A triangle pointing right against a bar: Next.
    Next,
    /// A frame wiped half across by a diagonal, the wiped half solid: Transition.
    Transition,
    /// A screen with two lines of text low in it: Captions.
    Captions,
    /// A clock face with its hands at three: the import tray's select-this-group button.
    Clock,
    /// A picture with a "none" sign at its corner: Exclude (the import tray).
    Exclude,
    /// "1:1": Actual size.
    ActualSize,
    /// An arrow coming into a frame from its top-right: Open in Dalica.
    OpenInDalica,
    /// Two arrows, the upper pointing right, the lower left (⇄): Switch Focus (compare).
    SwitchFocus,
    /// Two arrows, the left pointing up, the right down (⇅): Switch Layout (compare).
    SwitchLayout,
    /// A frame whose left half is solid: Show Original (the before/after hold).
    ShowOriginal,
}

impl ViewingIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [ViewingIcon; 14] = [
        ViewingIcon::Library,
        ViewingIcon::Exit,
        ViewingIcon::Pause,
        ViewingIcon::Previous,
        ViewingIcon::Next,
        ViewingIcon::Transition,
        ViewingIcon::Captions,
        ViewingIcon::Clock,
        ViewingIcon::Exclude,
        ViewingIcon::ActualSize,
        ViewingIcon::OpenInDalica,
        ViewingIcon::SwitchFocus,
        ViewingIcon::SwitchLayout,
        ViewingIcon::ShowOriginal,
    ];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            ViewingIcon::Library => "library",
            ViewingIcon::Exit => "exit",
            ViewingIcon::Pause => "pause",
            ViewingIcon::Previous => "previous",
            ViewingIcon::Next => "next",
            ViewingIcon::Transition => "transition",
            ViewingIcon::Captions => "captions",
            ViewingIcon::Clock => "clock",
            ViewingIcon::Exclude => "exclude",
            ViewingIcon::ActualSize => "actual-size",
            ViewingIcon::OpenInDalica => "open-in-dalica",
            ViewingIcon::SwitchFocus => "switch-focus",
            ViewingIcon::SwitchLayout => "switch-layout",
            ViewingIcon::ShowOriginal => "show-original",
        }
    }

    /// The glyph called `name` ([`ViewingIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<ViewingIcon> {
        ViewingIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_viewing(painter: &Painter, rect: Rect, icon: ViewingIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

/// A skip glyph: a solid triangle pointing right with a bar at its tip (mirror the pen for left).
fn skip(p: &Pen) {
    p.fill(&[(4.5, 4.0), (13.0, 10.0), (4.5, 16.0)]);
    p.fill(&[(13.5, 4.0), (16.0, 4.0), (16.0, 16.0), (13.5, 16.0)]);
}

/// An arrow along a horizontal line at `y` from `x0` to the head at `x1` (either way round).
fn h_arrow(p: &Pen, x0: f32, x1: f32, y: f32) {
    let back = if x1 > x0 { -3.0 } else { 3.0 };
    p.seg((x0, y), (x1, y));
    p.line(&[(x1 + back, y - 3.0), (x1, y), (x1 + back, y + 3.0)]);
}

/// An arrow along a vertical line at `x` from `y0` to the head at `y1` (either way round).
fn v_arrow(p: &Pen, x: f32, y0: f32, y1: f32) {
    let back = if y1 > y0 { -3.0 } else { 3.0 };
    p.seg((x, y0), (x, y1));
    p.line(&[(x - 3.0, y1 + back), (x, y1), (x + 3.0, y1 + back)]);
}

fn draw(p: &Pen, icon: ViewingIcon) {
    match icon {
        ViewingIcon::Library => {
            // Four thumbnails, two by two: the light table.
            for (x, y) in [(3.0, 3.0), (11.0, 3.0), (3.0, 11.0), (11.0, 11.0)] {
                p.rect(x, y, x + 6.0, y + 6.0, 1.0);
            }
        }
        ViewingIcon::Exit => {
            // A door frame open to the left on the right, an arrow leaving it to the left.
            p.elbow(&[(11.0, 3.0), (17.0, 3.0), (17.0, 17.0), (11.0, 17.0)]);
            h_arrow(p, 13.0, 3.0, 10.0);
        }
        ViewingIcon::Pause => {
            p.fill(&[(5.0, 4.0), (8.5, 4.0), (8.5, 16.0), (5.0, 16.0)]);
            p.fill(&[(11.5, 4.0), (15.0, 4.0), (15.0, 16.0), (11.5, 16.0)]);
        }
        ViewingIcon::Previous => skip(&p.mirrored()),
        ViewingIcon::Next => skip(p),
        ViewingIcon::Transition => {
            // A frame, a diagonal from its bottom-left to its top-right, the upper-left half solid.
            p.rect(3.0, 4.0, 17.0, 16.0, 0.0);
            p.fill(&[(3.0, 4.0), (17.0, 4.0), (3.0, 16.0)]);
        }
        ViewingIcon::Captions => {
            // A screen with two lines of text in its lower part.
            p.rect(2.5, 3.5, 17.5, 16.5, 1.5);
            p.seg((5.5, 10.5), (14.5, 10.5));
            p.seg((7.0, 13.5), (13.0, 13.5));
        }
        ViewingIcon::Clock => {
            p.circle(10.0, 10.0, 7.25);
            p.elbow(&[(10.0, 5.5), (10.0, 10.0), (13.5, 10.0)]);
        }
        ViewingIcon::Exclude => {
            // A picture (its frame and a hill) with a "none" sign over its bottom-right corner.
            p.elbow(&[(10.0, 13.0), (2.5, 13.0), (2.5, 3.0), (14.0, 3.0), (14.0, 9.0)]);
            p.fill(&[(4.5, 11.0), (7.5, 6.5), (10.0, 11.0)]);
            p.circle(14.0, 13.75, 3.5);
            let d = 3.5 * std::f32::consts::FRAC_1_SQRT_2;
            p.seg((14.0 - d, 13.75 - d), (14.0 + d, 13.75 + d));
        }
        ViewingIcon::ActualSize => {
            // "1:1": two ones with a flag each and a colon between them.
            for x in [5.5, 14.5] {
                p.seg((x, 4.5), (x, 15.5));
                p.seg((x - 2.0, 6.5), (x, 4.5));
            }
            p.dot(10.0, 8.0, 1.1);
            p.dot(10.0, 13.0, 1.1);
        }
        ViewingIcon::OpenInDalica => {
            // A frame open at its top-right corner, an arrow coming into it from there.
            p.elbow(&[(9.0, 6.0), (3.0, 6.0), (3.0, 17.0), (14.0, 17.0), (14.0, 11.0)]);
            p.seg((17.0, 3.0), (8.5, 11.5));
            p.line(&[(8.5, 6.5), (8.5, 11.5), (13.5, 11.5)]);
        }
        ViewingIcon::SwitchFocus => {
            h_arrow(p, 3.0, 17.0, 6.5);
            h_arrow(p, 17.0, 3.0, 13.5);
        }
        ViewingIcon::SwitchLayout => {
            v_arrow(p, 6.5, 17.0, 3.0);
            v_arrow(p, 13.5, 3.0, 17.0);
        }
        ViewingIcon::ShowOriginal => {
            p.rect(3.0, 4.0, 17.0, 16.0, 0.0);
            p.fill(&[(3.0, 4.0), (10.0, 4.0), (10.0, 16.0), (3.0, 16.0)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in ViewingIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `ShowOriginal` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(ViewingIcon::ShowOriginal as usize + 1, ViewingIcon::ALL.len());
        let mut names: Vec<_> = ViewingIcon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ViewingIcon::ALL.len(), "names are unique");
        for icon in ViewingIcon::ALL {
            assert_eq!(ViewingIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(ViewingIcon::from_name("play"), None);
    }
}
