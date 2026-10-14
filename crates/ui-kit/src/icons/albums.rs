//! The automatic albums' glyphs (`organizing.md` §4.1, `ui-layout.md` §4, `design-system.md` §5):
//! our own drawings on the 20 × 20 grid with the shell icons' pen, each the user album's book (the
//! outline and spine of [`Icon::Album`](super::Icon::Album)) with an emblem on its cover in place
//! of the picture: a star for **Starred Photos**, a clock for **Recently Updated** and a
//! magnifier for the temporary **Search results** album.
//!
//! They are a set of their own, [`AlbumIcon`] ([`paint_album`]), like Save's and Export's glyphs,
//! so the shell's icon sheet and its fingerprint stay as they are. Content stays inside 2 … 18 so
//! the stroke never leaves the square.

use egui::{Color32, Mesh, Painter, Rect, Shape, Stroke};

use super::pen::Pen;

/// The automatic albums' glyphs. Keep [`AlbumIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlbumIcon {
    /// A book with a star on its cover: the Starred Photos album.
    Starred,
    /// A book with a clock on its cover: the Recently Updated album.
    Recent,
    /// A book with a magnifier on its cover: the temporary Search results album.
    Search,
}

impl AlbumIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [AlbumIcon; 3] = [AlbumIcon::Starred, AlbumIcon::Recent, AlbumIcon::Search];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            AlbumIcon::Starred => "starred-album",
            AlbumIcon::Recent => "recent-album",
            AlbumIcon::Search => "search-album",
        }
    }

    /// The glyph called `name` ([`AlbumIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<AlbumIcon> {
        AlbumIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_album(painter: &Painter, rect: Rect, icon: AlbumIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(painter, &Pen::new(painter, rect, color), icon);
}

/// The middle of the book's cover, right of the spine.
const COVER: (f32, f32) = (11.5, 10.0);

fn draw(painter: &Painter, p: &Pen, icon: AlbumIcon) {
    // The book of the user album: its outline and the spine.
    p.rect(3.5, 2.5, 16.5, 17.5, 1.5);
    p.seg((6.5, 2.5), (6.5, 17.5));
    let (cx, cy) = COVER;
    match icon {
        AlbumIcon::Starred => star(painter, p, cx, cy + 0.3, 3.8, 1.6),
        AlbumIcon::Recent => {
            // A clock face with its hands at three o'clock.
            p.circle(cx, cy, 3.1);
            p.elbow(&[(cx, cy - 1.9), (cx, cy), (cx + 1.5, cy)]);
        }
        AlbumIcon::Search => {
            // A lens up and left of the middle and its handle towards the bottom-right corner.
            let (lx, ly, r) = (cx - 0.6, cy - 0.9, 2.3);
            p.circle(lx, ly, r);
            let h = r * std::f32::consts::FRAC_1_SQRT_2;
            p.seg((lx + h, ly + h), (cx + 2.9, cy + 3.1));
        }
    }
}

/// A filled five-pointed star round (`cx`, `cy`) with outer and inner radii `outer` and `inner`
/// in grid units: the star is concave, so it is filled as a fan round its centre and outlined
/// thinly so its edges are anti-aliased like the pen's strokes.
fn star(painter: &Painter, p: &Pen, cx: f32, cy: f32, outer: f32, inner: f32) {
    let points: Vec<_> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { outer } else { inner };
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            p.pt(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    let color = p.color();
    let mut mesh = Mesh::default();
    mesh.colored_vertex(p.pt(cx, cy), color);
    for &pt in &points {
        mesh.colored_vertex(pt, color);
    }
    for i in 0..10u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
    }
    painter.add(Shape::mesh(mesh));
    let k = (p.pt(1.0, 0.0).x - p.pt(0.0, 0.0).x).abs();
    painter.add(Shape::closed_line(points, Stroke::new((0.5 * k).max(0.5), color)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in AlbumIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `Search` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(AlbumIcon::Search as usize + 1, AlbumIcon::ALL.len());
        let mut names: Vec<_> = AlbumIcon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), AlbumIcon::ALL.len(), "names are unique");
        for icon in AlbumIcon::ALL {
            assert_eq!(AlbumIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(AlbumIcon::from_name("album"), None);
    }
}
