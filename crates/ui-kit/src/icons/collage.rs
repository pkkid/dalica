//! The glyphs of the Collage room (`creating.md` §3, `design-system.md` §5): our own drawings on
//! the 20 × 20 grid with the shell icons' pen.
//!
//! They are a set of their own, [`CollageIcon`] ([`paint_collage`]), so the other sets' sheets and
//! fingerprints stay as they are. Content stays inside 2 … 18 so the stroke never leaves the
//! square. The six styles are each a page (a framed square) drawn the way the style lays pictures
//! out, apart from the shell's Collage (a frame split in three); the border toggles are one
//! picture (a solid block) with no frame, a thin even frame and an instant print's deep bottom; the
//! orientations are a page on its side and upright. Bring to Top and Send to Bottom are the same
//! two overlapping squares, the solid one in front or behind.

use egui::{Color32, Painter, Rect};

use super::pen::Pen;

/// The collage glyphs. Keep [`CollageIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollageIcon {
    /// Two pictures scattered at angles over each other: the Picture Pile style.
    PicturePile,
    /// A page cut into blocks of different sizes: the Mosaic style.
    Mosaic,
    /// A page with a solid block in its middle and blocks round it: the Frame Mosaic style.
    FrameMosaic,
    /// A page in three columns and two rows of equal cells: the Grid style.
    Grid,
    /// Four small solid pictures, each over a line for its name: the Contact Sheet style.
    ContactSheet,
    /// Two framed pictures overlapping, their overlap solid: the Multiple Exposure style.
    MultipleExposure,
    /// A picture alone, a solid block with no frame: Picture borders none.
    NoBorder,
    /// The picture in a thin even frame: Picture borders white.
    WhiteBorder,
    /// The picture in an instant print's frame, deep at the bottom: Picture borders instant.
    InstantBorder,
    /// A page on its side with a hill on it: landscape.
    Landscape,
    /// The page upright: portrait.
    Portrait,
    /// A die showing five: Scramble Collage (a new random arrangement).
    Scramble,
    /// Two arrows crossing to the right: Shuffle Pictures (a new assignment to the slots).
    Shuffle,
    /// A framed picture (a hill and the sun) over a bar under the page: Set as Background.
    SetAsBackground,
    /// A solid square in the four corners of a viewfinder: Set as Frame Center.
    FrameCenter,
    /// Two overlapping squares, the solid one in front: Bring to Top.
    BringToTop,
    /// The same squares, the solid one behind: Send to Bottom.
    SendToBottom,
    /// A framed picture with a plus at its top right: Get More.
    GetMore,
}

impl CollageIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [CollageIcon; 18] = [
        CollageIcon::PicturePile,
        CollageIcon::Mosaic,
        CollageIcon::FrameMosaic,
        CollageIcon::Grid,
        CollageIcon::ContactSheet,
        CollageIcon::MultipleExposure,
        CollageIcon::NoBorder,
        CollageIcon::WhiteBorder,
        CollageIcon::InstantBorder,
        CollageIcon::Landscape,
        CollageIcon::Portrait,
        CollageIcon::Scramble,
        CollageIcon::Shuffle,
        CollageIcon::SetAsBackground,
        CollageIcon::FrameCenter,
        CollageIcon::BringToTop,
        CollageIcon::SendToBottom,
        CollageIcon::GetMore,
    ];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            CollageIcon::PicturePile => "picture-pile",
            CollageIcon::Mosaic => "mosaic",
            CollageIcon::FrameMosaic => "frame-mosaic",
            CollageIcon::Grid => "grid",
            CollageIcon::ContactSheet => "contact-sheet",
            CollageIcon::MultipleExposure => "multiple-exposure",
            CollageIcon::NoBorder => "no-border",
            CollageIcon::WhiteBorder => "white-border",
            CollageIcon::InstantBorder => "instant-border",
            CollageIcon::Landscape => "landscape",
            CollageIcon::Portrait => "portrait",
            CollageIcon::Scramble => "scramble",
            CollageIcon::Shuffle => "shuffle",
            CollageIcon::SetAsBackground => "set-as-background",
            CollageIcon::FrameCenter => "frame-center",
            CollageIcon::BringToTop => "bring-to-top",
            CollageIcon::SendToBottom => "send-to-bottom",
            CollageIcon::GetMore => "get-more",
        }
    }

    /// The glyph called `name` ([`CollageIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<CollageIcon> {
        CollageIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_collage(painter: &Painter, rect: Rect, icon: CollageIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

/// The corners of a `w` × `h` box round (`cx`, `cy`) turned `deg` degrees clockwise.
fn turned(cx: f32, cy: f32, w: f32, h: f32, deg: f32) -> [(f32, f32); 4] {
    let (s, c) = deg.to_radians().sin_cos();
    [(-w, -h), (w, -h), (w, h), (-w, h)].map(|(x, y)| (cx + (x * c - y * s) / 2.0, cy + (x * s + y * c) / 2.0))
}

/// A small hill on the ground line `base` from `x0` to `x1`.
fn hill(p: &Pen, x0: f32, x1: f32, base: f32) {
    p.fill(&[(x0, base), ((x0 + x1) / 2.0, base - (x1 - x0) * 0.55), (x1, base)]);
}

/// A solid block from (`x0`, `y0`) to (`x1`, `y1`).
fn block(p: &Pen, x0: f32, y0: f32, x1: f32, y1: f32) {
    p.fill(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
}

fn draw(p: &Pen, icon: CollageIcon) {
    match icon {
        CollageIcon::PicturePile => {
            p.closed(&turned(8.2, 7.8, 10.0, 7.4, -14.0));
            p.closed(&turned(11.8, 12.4, 10.0, 7.4, 9.0));
        }
        CollageIcon::Mosaic => {
            p.rect(2.5, 2.5, 17.5, 17.5, 1.0);
            p.seg((8.0, 2.5), (8.0, 17.5));
            p.seg((2.5, 11.5), (8.0, 11.5));
            p.seg((8.0, 8.5), (17.5, 8.5));
            p.seg((13.0, 8.5), (13.0, 17.5));
        }
        CollageIcon::FrameMosaic => {
            p.rect(2.5, 2.5, 17.5, 17.5, 1.0);
            block(p, 6.5, 6.5, 13.5, 13.5);
            p.seg((10.0, 2.5), (10.0, 6.5));
            p.seg((10.0, 13.5), (10.0, 17.5));
            p.seg((2.5, 10.0), (6.5, 10.0));
            p.seg((13.5, 10.0), (17.5, 10.0));
        }
        CollageIcon::Grid => {
            p.rect(2.5, 4.0, 17.5, 16.0, 1.0);
            p.seg((7.5, 4.0), (7.5, 16.0));
            p.seg((12.5, 4.0), (12.5, 16.0));
            p.seg((2.5, 10.0), (17.5, 10.0));
        }
        CollageIcon::ContactSheet => {
            for (x, y) in [(3.0, 3.0), (11.0, 3.0), (3.0, 11.0), (11.0, 11.0)] {
                block(p, x, y, x + 6.0, y + 4.2);
                p.seg((x + 0.5, y + 6.2), (x + 5.5, y + 6.2));
            }
        }
        CollageIcon::MultipleExposure => {
            p.rect(2.5, 2.5, 13.0, 13.0, 1.0);
            p.rect(7.0, 7.0, 17.5, 17.5, 1.0);
            block(p, 7.0, 7.0, 13.0, 13.0);
        }
        CollageIcon::NoBorder => {
            block(p, 3.0, 5.0, 17.0, 15.0);
        }
        CollageIcon::WhiteBorder => {
            p.rect(2.5, 4.0, 17.5, 16.0, 0.5);
            block(p, 5.0, 6.5, 15.0, 13.5);
        }
        CollageIcon::InstantBorder => {
            p.rect(4.0, 2.5, 16.0, 17.5, 0.5);
            block(p, 6.3, 4.8, 13.7, 12.6);
        }
        CollageIcon::Landscape => {
            p.rect(2.5, 5.0, 17.5, 15.0, 1.0);
            hill(p, 5.0, 12.0, 13.0);
        }
        CollageIcon::Portrait => {
            p.rect(5.0, 2.5, 15.0, 17.5, 1.0);
            hill(p, 7.0, 13.0, 15.5);
        }
        CollageIcon::Scramble => {
            p.rect(3.0, 3.0, 17.0, 17.0, 3.0);
            for (x, y) in [(6.8, 6.8), (13.2, 6.8), (10.0, 10.0), (6.8, 13.2), (13.2, 13.2)] {
                p.dot(x, y, 1.3);
            }
        }
        CollageIcon::Shuffle => {
            // Two arrows from the left crossing on their way right, each with its head at the end.
            p.line(&[(2.5, 6.0), (6.0, 6.0), (11.5, 14.0), (14.5, 14.0)]);
            p.line(&[(2.5, 14.0), (6.0, 14.0), (11.5, 6.0), (14.5, 6.0)]);
            p.fill(&[(14.0, 3.2), (18.0, 6.0), (14.0, 8.8)]);
            p.fill(&[(14.0, 11.2), (18.0, 14.0), (14.0, 16.8)]);
        }
        CollageIcon::SetAsBackground => {
            p.rect(2.5, 2.5, 17.5, 13.5, 1.0);
            hill(p, 4.5, 12.5, 13.5);
            p.dot(14.0, 6.2, 1.5);
            p.seg((2.5, 17.0), (17.5, 17.0));
        }
        CollageIcon::FrameCenter => {
            for (x, y, dx, dy) in [(2.5, 2.5, 1.0, 1.0), (17.5, 2.5, -1.0, 1.0), (2.5, 17.5, 1.0, -1.0), (17.5, 17.5, -1.0, -1.0)] {
                p.elbow(&[(x + 4.0 * dx, y), (x, y), (x, y + 4.0 * dy)]);
            }
            block(p, 6.5, 6.5, 13.5, 13.5);
        }
        CollageIcon::BringToTop => {
            block(p, 2.5, 2.5, 12.0, 12.0);
            p.elbow(&[(13.8, 8.0), (17.5, 8.0), (17.5, 17.5), (8.0, 17.5), (8.0, 13.8)]);
        }
        CollageIcon::SendToBottom => {
            p.rect(2.5, 2.5, 12.0, 12.0, 0.5);
            block(p, 13.8, 8.0, 17.5, 17.5);
            block(p, 8.0, 13.8, 13.8, 17.5);
        }
        CollageIcon::GetMore => {
            p.rect(2.5, 6.0, 14.0, 17.0, 1.0);
            hill(p, 4.5, 11.0, 15.0);
            p.seg((15.5, 2.0), (15.5, 8.0));
            p.seg((12.5, 5.0), (18.0, 5.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in CollageIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `GetMore` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(CollageIcon::GetMore as usize + 1, CollageIcon::ALL.len());
        for icon in CollageIcon::ALL {
            assert_eq!(CollageIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(CollageIcon::from_name("collage"), None);
    }

    #[test]
    fn a_turned_box_keeps_its_centre_and_its_size() {
        let q = turned(10.0, 10.0, 8.0, 4.0, 30.0);
        let (cx, cy) = q.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0 / 4.0, y + p.1 / 4.0));
        assert!((cx - 10.0).abs() < 1e-4 && (cy - 10.0).abs() < 1e-4);
        let side = ((q[1].0 - q[0].0).powi(2) + (q[1].1 - q[0].1).powi(2)).sqrt();
        assert!((side - 8.0).abs() < 1e-4, "{side}");
    }
}
