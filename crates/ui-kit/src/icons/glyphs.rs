//! The shell glyphs, each our own drawing on the 20 × 20 grid (`design-system.md` §5). Content
//! stays inside 2 … 18 so the stroke never leaves the square.

use super::Icon;
use super::pen::{Pen, arc_points};

/// The tangent angle, in degrees from the circle's centre, of a teardrop whose circle has radius
/// `r` and whose tip is `d` below the centre.
fn teardrop_tangent(r: f32, d: f32) -> f32 {
    (r / d).clamp(-1.0, 1.0).asin().to_degrees()
}

/// A photo album: a book with a spine and a small mountain on its cover, in the box
/// (`x0`, `y0`) – (`x1`, `y1`).
fn album(p: &Pen, x0: f32, y0: f32, x1: f32, y1: f32) {
    p.rect(x0, y0, x1, y1, 1.5);
    let spine = x0 + 3.0;
    p.seg((spine, y0), (spine, y1));
    let (l, r) = (spine + 1.8, x1 - 1.8);
    let base = y1 - 2.4;
    let peak = base - (r - l) * 0.75;
    p.fill(&[(l, base), ((l + r) / 2.0, peak), (r, base)]);
}

/// A rotation arrow turning counterclockwise: three quarters of a circle with an arrowhead at the
/// top pointing left. The clockwise twin is the same drawing mirrored.
fn rotate_ccw(p: &Pen) {
    p.arc(10.0, 11.0, 6.0, 270.0, 520.0);
    p.fill(&[(6.6, 5.0), (10.6, 1.9), (10.6, 8.1)]);
}

/// One magnifier: a lens of radius `r` round (`c`, `c`) and a handle to the bottom-right corner.
fn magnifier(p: &Pen, c: f32, r: f32) {
    p.circle(c, c, r);
    let h = c + r * std::f32::consts::FRAC_1_SQRT_2;
    p.seg((h, h), (17.0, 17.0));
}

/// The activity spinner at `step` (0 … 7): eight spokes, the brightest leading clockwise.
pub(super) fn spokes(p: &Pen, step: u32) {
    let color = p.color();
    for i in 0..8u32 {
        let a = ((i + step % 8) as f32 * 45.0 - 90.0).to_radians();
        let (c, s) = (a.cos(), a.sin());
        let pen = p.tinted(color.gamma_multiply((i + 1) as f32 / 8.0));
        pen.seg((10.0 + 3.8 * c, 10.0 + 3.8 * s), (10.0 + 7.5 * c, 10.0 + 7.5 * s));
    }
}

/// Draws every glyph except the star and plus, which `paint` draws itself.
pub(super) fn draw(p: &Pen, icon: Icon) {
    match icon {
        // Drawn by `icons::paint` (kept there so their pixels never move).
        Icon::Star | Icon::StarOutline | Icon::Plus => {}
        Icon::AppMark => {
            // A framed picture: a sun over two hills.
            p.rect(2.5, 2.5, 17.5, 17.5, 3.5);
            p.dot(13.2, 6.8, 1.7);
            p.fill(&[(4.6, 15.4), (8.8, 8.6), (13.0, 15.4)]);
            p.fill(&[(10.4, 15.4), (12.9, 11.6), (15.4, 15.4)]);
        }
        Icon::Import => {
            // A camera: body, viewfinder hump, lens and flash.
            p.rect(2.5, 6.0, 17.5, 16.0, 2.0);
            p.line(&[(6.8, 6.0), (8.0, 3.8), (12.0, 3.8), (13.2, 6.0)]);
            p.circle(10.0, 11.0, 3.0);
            p.dot(15.0, 8.4, 0.75);
        }
        Icon::Album => album(p, 3.5, 2.5, 16.5, 17.5),
        Icon::NewAlbum => {
            album(p, 2.5, 6.5, 13.5, 17.5);
            p.seg((16.0, 1.75), (16.0, 6.25));
            p.seg((13.75, 4.0), (18.25, 4.0));
        }
        Icon::AddToAlbum => {
            album(p, 2.5, 2.5, 13.0, 17.5);
            p.fill(&[(14.6, 8.6), (18.4, 8.6), (16.5, 11.4)]);
        }
        Icon::FlatList => {
            for y in [5.0, 10.0, 15.0] {
                p.dot(4.0, y, 1.3);
                p.seg((7.5, y), (17.0, y));
            }
        }
        Icon::Tree => {
            // A root row and two indented child rows joined to it.
            p.seg((3.0, 4.5), (13.0, 4.5));
            p.elbow(&[(5.0, 4.5), (5.0, 15.5), (8.5, 15.5)]);
            p.seg((5.0, 10.0), (8.5, 10.0));
            for y in [10.0, 15.5] {
                p.seg((10.5, y), (17.0, y));
            }
        }
        Icon::ChevronDown => p.line(&[(5.5, 7.75), (10.0, 12.25), (14.5, 7.75)]),
        Icon::ChevronUp => p.line(&[(5.5, 12.25), (10.0, 7.75), (14.5, 12.25)]),
        Icon::ChevronRight => p.line(&[(7.75, 5.5), (12.25, 10.0), (7.75, 14.5)]),
        Icon::ChevronLeft => p.line(&[(12.25, 5.5), (7.75, 10.0), (12.25, 14.5)]),
        Icon::ChevronsLeft => {
            p.line(&[(9.5, 5.5), (5.0, 10.0), (9.5, 14.5)]);
            p.line(&[(15.0, 5.5), (10.5, 10.0), (15.0, 14.5)]);
        }
        Icon::Faces => {
            // A face: round head, two eyes, a smile.
            p.circle(10.0, 10.0, 7.25);
            p.dot(7.4, 8.3, 1.1);
            p.dot(12.6, 8.3, 1.1);
            p.arc(10.0, 10.0, 4.0, 35.0, 145.0);
        }
        Icon::Videos => {
            // A video camera: body and lens cone.
            p.rect(2.5, 5.5, 12.5, 14.5, 2.0);
            p.closed(&[(12.5, 8.6), (17.5, 5.8), (17.5, 14.2), (12.5, 11.4)]);
        }
        Icon::Geotagged => {
            // A map pin: a teardrop with a hole.
            let (cx, cy, r, tip) = (10.0, 7.8, 5.6, 18.0);
            let t = teardrop_tangent(r, tip - cy);
            let mut pts = arc_points(cx, cy, r, 90.0 + (90.0 - t), 360.0 + 90.0 - (90.0 - t));
            pts.push((cx, tip));
            p.closed(&pts);
            p.circle(cx, cy, 2.0);
        }
        Icon::Search => magnifier(p, 8.5, 5.25),
        Icon::ClearSearch => {
            p.circle(10.0, 10.0, 7.25);
            p.seg((7.3, 7.3), (12.7, 12.7));
            p.seg((12.7, 7.3), (7.3, 12.7));
        }
        Icon::Activity => spokes(p, 0),
        Icon::Hold => {
            // A push pin, point down.
            p.closed(&[(6.8, 2.5), (13.2, 2.5), (12.0, 4.6), (12.0, 8.4), (15.0, 12.0), (5.0, 12.0), (8.0, 8.4), (8.0, 4.6)]);
            p.seg((10.0, 12.0), (10.0, 17.5));
        }
        Icon::ClearTray => {
            // A circle with a slash: "none".
            p.circle(10.0, 10.0, 7.25);
            let d = 7.25 * std::f32::consts::FRAC_1_SQRT_2;
            p.seg((10.0 - d, 10.0 - d), (10.0 + d, 10.0 + d));
        }
        Icon::RotateCcw => rotate_ccw(p),
        Icon::RotateCw => rotate_ccw(&p.mirrored()),
        Icon::Loupe => {
            // A magnifier with a plus: magnify.
            magnifier(p, 8.5, 5.75);
            p.seg((6.0, 8.5), (11.0, 8.5));
            p.seg((8.5, 6.0), (8.5, 11.0));
        }
        Icon::People => {
            // Two people, one behind the other.
            p.circle(14.2, 6.2, 2.2);
            p.arc(14.4, 15.6, 4.4, 255.0, 340.0);
            p.circle(7.2, 7.4, 2.7);
            p.arc(7.2, 17.6, 5.4, 200.0, 340.0);
        }
        Icon::Places => {
            // A folded map.
            p.closed(&[(2.5, 5.0), (7.5, 3.0), (12.5, 5.0), (17.5, 3.0), (17.5, 15.0), (12.5, 17.0), (7.5, 15.0), (2.5, 17.0)]);
            p.seg((7.5, 3.0), (7.5, 15.0));
            p.seg((12.5, 5.0), (12.5, 17.0));
        }
        Icon::Tags => {
            // A price tag with its hole.
            p.closed(&[(3.0, 3.0), (10.5, 3.0), (17.0, 9.5), (9.5, 17.0), (3.0, 10.5)]);
            p.dot(7.0, 7.0, 1.3);
        }
        Icon::Properties => {
            // "i" in a circle.
            p.circle(10.0, 10.0, 7.25);
            p.dot(10.0, 6.4, 1.1);
            p.seg((10.0, 9.0), (10.0, 14.5));
        }
        Icon::Export => {
            // An arrow leaving an open box.
            p.line(&[(4.0, 11.0), (4.0, 16.5), (16.0, 16.5), (16.0, 11.0)]);
            p.seg((10.0, 13.0), (10.0, 3.0));
            p.line(&[(6.5, 6.5), (10.0, 3.0), (13.5, 6.5)]);
        }
        Icon::Collage => {
            // A frame split into three pictures.
            p.rect(2.5, 3.5, 17.5, 16.5, 1.5);
            p.seg((10.0, 3.5), (10.0, 16.5));
            p.seg((10.0, 10.0), (17.5, 10.0));
        }
        Icon::Slideshow => {
            // A screen with a play triangle, on a foot.
            p.rect(2.5, 3.5, 17.5, 14.0, 1.5);
            p.fill(&[(8.4, 6.2), (8.4, 11.3), (12.8, 8.75)]);
            p.seg((7.0, 17.0), (13.0, 17.0));
        }
        Icon::More => {
            for x in [4.5, 10.0, 15.5] {
                p.dot(x, 10.0, 1.6);
            }
        }
        Icon::Folder => {
            // Straight edges as one elbow (crisp), the tab's slope on its own.
            p.elbow(&[(9.6, 6.5), (17.5, 6.5), (17.5, 15.5), (2.5, 15.5), (2.5, 4.5), (7.8, 4.5)]);
            p.seg((7.8, 4.5), (9.6, 6.5));
            p.seg((2.5, 8.5), (17.5, 8.5));
        }
        Icon::Minimize => p.seg((5.0, 10.0), (15.0, 10.0)),
        Icon::Maximize => p.rect(5.0, 5.0, 15.0, 15.0, 0.0),
        Icon::Restore => {
            p.rect(4.5, 7.5, 12.5, 15.5, 0.0);
            p.elbow(&[(7.5, 7.5), (7.5, 4.5), (15.5, 4.5), (15.5, 12.5), (12.5, 12.5)]);
        }
        Icon::Close => {
            p.seg((5.0, 5.0), (15.0, 15.0));
            p.seg((15.0, 5.0), (5.0, 15.0));
        }
    }
}
