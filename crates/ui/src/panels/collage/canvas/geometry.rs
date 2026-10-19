//! Where each picture is on the screen: the engine's placements (normalised to the page) mapped
//! onto the page's rect, with the border round each picture as the render draws it
//! (`create::collage::render`: white 3 % of the picture's short side all round, instant 5 % with a
//! 20 % bottom; Pile and Contact Sheet only), and the point tests the gestures use. Pure.

use dalica_create::collage::{Border, Collage, Placement};
use egui::{Pos2, Rect, Vec2, pos2, vec2};

/// One picture on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Card {
    /// The picture's number among the placements (`item:<n>`).
    pub item: u32,
    /// The picture's box (without its border), unturned, centred on the placement's centre.
    pub picture: Rect,
    /// The box with its border, unturned, in the same frame (its centre is lower for an instant
    /// border).
    pub card: Rect,
    /// Degrees, clockwise on the screen, round the picture's centre.
    pub angle: f32,
}

impl Card {
    /// The picture's centre on the screen.
    pub fn centre(&self) -> Pos2 {
        self.picture.center()
    }

    /// The corners of `rect` (one of this card's rects) turned with the card round the picture's
    /// centre: top-left, top-right, bottom-right, bottom-left.
    pub fn corners(&self, rect: Rect) -> [Pos2; 4] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let o = self.centre();
        [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].map(|p| {
            let d = p - o;
            o + vec2(d.x * c - d.y * s, d.x * s + d.y * c)
        })
    }

    /// Whether the screen point `p` is on the card (its border counts).
    pub fn contains(&self, p: Pos2) -> bool {
        let (s, c) = self.angle.to_radians().sin_cos();
        let d = p - self.centre();
        let local = self.centre() + vec2(d.x * c + d.y * s, -d.x * s + d.y * c);
        self.card.contains(local)
    }

    /// The axis-aligned box round the turned picture (its border left out): its centre is the
    /// placement's centre.
    pub fn picture_bounds(&self) -> Rect {
        Rect::from_points(&self.corners(self.picture))
    }

    /// The axis-aligned box round the turned card.
    pub fn bounds(&self) -> Rect {
        Rect::from_points(&self.corners(self.card))
    }
}

/// The border's sides and bottom for a picture box of `size` in `doc`, as the render draws them.
pub fn border(doc: &Collage, size: Vec2) -> (f32, f32) {
    let border = if doc.style.has_borders() { doc.settings.border } else { Border::None };
    let short = size.x.min(size.y);
    (border.sides() as f32 * short, border.bottom() as f32 * short)
}

/// The point at `(x, y)` normalised to `page`.
pub fn to_screen(page: Rect, [x, y]: [f64; 2]) -> Pos2 {
    pos2(page.min.x + x as f32 * page.width(), page.min.y + y as f32 * page.height())
}

/// The screen point `p` normalised to `page` (0 … 1 across it).
pub fn to_page(page: Rect, p: Pos2) -> [f32; 2] {
    let w = page.width().max(f32::EPSILON);
    let h = page.height().max(f32::EPSILON);
    [(p.x - page.min.x) / w, (p.y - page.min.y) / h]
}

/// Placement `p` (picture `item`) on `page` with its centre moved to `centre`, its box scaled by
/// `grow` and turned to `angle`; `None` for numbers that are not finite.
pub fn card_at(doc: &Collage, page: Rect, item: u32, p: &Placement, centre: Pos2, grow: f32, angle: f32) -> Option<Card> {
    let size = vec2(p.size[0] as f32 * page.width(), p.size[1] as f32 * page.height()) * grow;
    if !(size.x.is_finite() && size.y.is_finite() && centre.x.is_finite() && centre.y.is_finite() && angle.is_finite()) {
        return None;
    }
    let picture = Rect::from_center_size(centre, size.max(Vec2::ZERO));
    let (side, bottom) = border(doc, picture.size());
    let card = Rect::from_min_max(picture.min - vec2(side, side), picture.max + vec2(side, bottom));
    Some(Card { item, picture, card, angle })
}

/// Placement `p` (picture `item`) where the engine puts it on `page`.
pub fn card(doc: &Collage, page: Rect, item: u32, p: &Placement) -> Option<Card> {
    card_at(doc, page, item, p, to_screen(page, p.centre), 1.0, p.angle as f32)
}

/// The topmost card under `p` (the last drawn: `cards` are bottom first).
pub fn hit(cards: &[Card], p: Pos2) -> Option<&Card> {
    cards.iter().rev().find(|c| c.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_core::{PhotoId, Rect01};
    use dalica_create::collage::Style;

    fn placement(centre: [f64; 2], size: [f64; 2], angle: f64) -> Placement {
        Placement { clip: 0, id: PhotoId::new(1), centre, size, angle, z: 0, shown: Rect01::FULL }
    }

    #[test]
    fn a_card_sits_on_its_placement_with_its_border() {
        let page = Rect::from_min_size(pos2(100.0, 50.0), vec2(400.0, 200.0));
        let mut doc = Collage { style: Style::Pile, ..Collage::default() };
        doc.settings.border = Border::Instant;
        let c = card(&doc, page, 3, &placement([0.25, 0.5], [0.25, 0.5], 90.0)).unwrap();
        assert_eq!(c.centre(), pos2(200.0, 150.0));
        assert_eq!(c.picture.size(), vec2(100.0, 100.0));
        // Instant: 5 % round, 20 % below.
        assert_eq!((c.card.min, c.card.max), (pos2(145.0, 95.0), pos2(255.0, 220.0)));
        // Turned a quarter clockwise, the border's thick bottom is on the left.
        assert!(c.contains(pos2(135.0, 150.0)) && !c.contains(pos2(260.0, 150.0)));
        assert_eq!(c.picture_bounds().center(), c.centre());
        assert_eq!(hit(&[c], pos2(200.0, 150.0)).map(|c| c.item), Some(3));
        // A grid has no border.
        doc.style = Style::Grid;
        let g = card(&doc, page, 0, &placement([0.5, 0.5], [0.5, 0.5], 0.0)).unwrap();
        assert_eq!(g.card, g.picture);
        assert_eq!(to_page(page, to_screen(page, [0.75, 0.25])), [0.75, 0.25]);
        assert_eq!(card(&doc, page, 0, &placement([f64::NAN, 0.5], [0.5, 0.5], 0.0)), None);
    }
}
