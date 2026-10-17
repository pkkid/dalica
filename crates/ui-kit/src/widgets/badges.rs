//! The marks a thumbnail carries (`design-system.md` §7, `ui-layout.md` §5.3): the photo's drop
//! shadow, the badge row inside the image's bottom-right corner (edited ✎ · geotagged · held ·
//! starred, the star always rightmost), a video's sprocket strip along the image's left edge and
//! its duration pill at the bottom-left, the `caution` "!" of a photo that cannot be shown, and the
//! `danger` ⊘ of a picture left out of an import (`organizing.md` §3).
//!
//! One painter, [`badges`], is fed what the model knows ([`Badges`]); it never decides what a
//! photo is. Plates are 18 × 18 with a 12 pt glyph on `badge_plate`, 4 pt in from the image's
//! corner and 2 pt apart. When the image is too narrow for the whole row, the badges that do not
//! fit are left out from the left, so the star (then held, then geotagged) stays; the duration pill
//! shows only where it fits beside the row.

use egui::epaint::Shadow;
use egui::{Align2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use super::{Target, register};
use crate::Tokens;
use crate::fonts;
use crate::icons::{self, Icon};
use crate::registry::Role;

/// Edge of a badge plate (points).
pub const BADGE_PLATE: f32 = 18.0;
/// Edge of the glyph on a plate (points).
pub const BADGE_GLYPH: f32 = 12.0;
/// Inset of the plates from the image's corner (points).
pub const BADGE_INSET: f32 = 4.0;
/// Space between two plates (points).
pub const BADGE_GAP: f32 = 2.0;
/// The photo's drop shadow (`ui-layout.md` §5.3): `black α90`, 2 pt down, 4 pt blur. A photo
/// colour, not chrome, so it lives here beside the painter that draws it.
pub const PHOTO_SHADOW: Shadow = Shadow { offset: [0, 2], blur: 4, spread: 0, color: Color32::from_black_alpha(90) };

/// One badge of the bottom-right row, in row order (left to right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Badge {
    /// The photo has an edit recipe not yet saved to its file (✎).
    Edited,
    /// The photo has a location.
    Geotagged,
    /// The photo is held in the tray.
    Held,
    /// The photo is starred.
    Starred,
}

impl Badge {
    /// The row order: edited, geotagged, held, starred.
    pub const ALL: [Badge; 4] = [Badge::Edited, Badge::Geotagged, Badge::Held, Badge::Starred];

    /// The name an automation id uses for the badge (`thumb:42/star`).
    pub fn name(self) -> &'static str {
        match self {
            Badge::Edited => "edited",
            Badge::Geotagged => "geotag",
            Badge::Held => "held",
            Badge::Starred => "star",
        }
    }

    /// What the badge says (its accessibility label).
    pub fn label(self) -> &'static str {
        match self {
            Badge::Edited => "Edited",
            Badge::Geotagged => "Geotagged",
            Badge::Held => "Held in the tray",
            Badge::Starred => "Starred",
        }
    }
}

/// What a thumbnail's marks show: the facts the model knows about the photo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Badges {
    pub edited: bool,
    pub geotagged: bool,
    pub held: bool,
    pub starred: bool,
    /// A video: its sprocket strip, and the duration pill when the length is known.
    pub video: Option<Video>,
}

/// A video's marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Video {
    /// The length in whole seconds, if known.
    pub seconds: Option<u32>,
}

impl Badges {
    /// Whether `badge` is on.
    pub fn has(&self, badge: Badge) -> bool {
        match badge {
            Badge::Edited => self.edited,
            Badge::Geotagged => self.geotagged,
            Badge::Held => self.held,
            Badge::Starred => self.starred,
        }
    }

    /// The row's badges that are on, left to right.
    pub fn row(&self) -> impl Iterator<Item = Badge> + '_ {
        Badge::ALL.into_iter().filter(|&b| self.has(b))
    }

    /// Every combination of the four row badges, each as a photo and as a video of `seconds`
    /// (32 in all): the badge sheet's tiles.
    pub fn combinations(seconds: u32) -> Vec<Badges> {
        let mut out = Vec::with_capacity(32);
        for video in [None, Some(Video { seconds: Some(seconds) })] {
            for bits in 0u8..16 {
                out.push(Badges { edited: bits & 1 != 0, geotagged: bits & 2 != 0, held: bits & 4 != 0, starred: bits & 8 != 0, video });
            }
        }
        out
    }
}

/// Where [`badges`] painted each mark (screen rects), for automation ids and tests.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BadgeRects {
    /// The row's plates, left to right.
    pub row: Vec<(Badge, Rect)>,
    /// The video's sprocket strip.
    pub sprockets: Option<Rect>,
    /// The video's duration pill.
    pub duration: Option<Rect>,
}

impl BadgeRects {
    /// The plate of `badge`, if it was painted.
    pub fn get(&self, badge: Badge) -> Option<Rect> {
        self.row.iter().find(|(b, _)| *b == badge).map(|&(_, r)| r)
    }
}

/// `1:23`, `12:05`, `1:02:03`: a video's length.
pub fn duration_text(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Paints the photo's drop shadow under `image` (call before the image).
pub fn photo_shadow(painter: &Painter, image: Rect) {
    if image.is_positive() && image.is_finite() {
        painter.add(PHOTO_SHADOW.as_shape(image, CornerRadius::ZERO));
    }
}

/// How many plates fit across an image `width` points wide.
fn plates_that_fit(width: f32) -> usize {
    let room = width - 2.0 * BADGE_INSET + BADGE_GAP;
    if !(room.is_finite() && room > 0.0) {
        return 0;
    }
    (room / (BADGE_PLATE + BADGE_GAP)).floor() as usize
}

/// The glyph a plate carries.
#[derive(Clone, Copy)]
enum Glyph {
    Icon(Icon),
    Pencil,
}

fn glyph(t: &Tokens, badge: Badge) -> (Glyph, Color32) {
    match badge {
        Badge::Edited => (Glyph::Pencil, t.text),
        Badge::Geotagged => (Glyph::Icon(Icon::Geotagged), t.text),
        Badge::Held => (Glyph::Icon(Icon::Hold), t.text),
        Badge::Starred => (Glyph::Icon(Icon::Star), t.star),
    }
}

/// Paints the plates of `glyphs` right-aligned in the bottom-right corner of `image`, at most as
/// many as fit (the leftmost are dropped). Returns their rects, left to right, with their index in
/// `glyphs`.
fn plates(painter: &Painter, t: &Tokens, image: Rect, glyphs: &[(Glyph, Color32)]) -> Vec<(usize, Rect)> {
    if image.height() < BADGE_PLATE + 2.0 * BADGE_INSET || !image.is_finite() {
        return Vec::new();
    }
    let n = glyphs.len().min(plates_that_fit(image.width()));
    let skip = glyphs.len() - n;
    let nf = n as f32;
    let mut x = image.max.x - BADGE_INSET - nf * BADGE_PLATE - (nf - 1.0).max(0.0) * BADGE_GAP;
    let y = image.max.y - BADGE_INSET - BADGE_PLATE;
    let mut rects = Vec::with_capacity(n);
    for (i, &(g, color)) in glyphs.iter().enumerate().skip(skip) {
        let plate = Rect::from_min_size(pos2(x, y), Vec2::splat(BADGE_PLATE));
        painter.rect_filled(plate, CornerRadius::same(3), t.badge_plate);
        let square = Rect::from_center_size(plate.center(), Vec2::splat(BADGE_GLYPH));
        match g {
            Glyph::Icon(icon) => icons::paint(painter, square, icon, color),
            Glyph::Pencil => pencil(painter, square, color),
        }
        rects.push((i, plate));
        x += BADGE_PLATE + BADGE_GAP;
    }
    rects
}

/// Paints a row of badges inside the bottom-right corner of `image`, right-aligned in the order
/// given (so the last one is rightmost), each a 12 pt glyph on a `badge_plate`. Returns the rects of
/// the plates, left to right. Plates that do not fit are left out from the left; nothing is painted
/// if the image is too small to hold one plate.
pub fn badge_row(painter: &Painter, t: &Tokens, image: Rect, glyphs: &[(Icon, Color32)]) -> Vec<Rect> {
    let glyphs: Vec<(Glyph, Color32)> = glyphs.iter().map(|&(i, c)| (Glyph::Icon(i), c)).collect();
    plates(painter, t, image, &glyphs).into_iter().map(|(_, r)| r).collect()
}

/// The star badge of a starred photo, registered as `id` (a part of its cell: `"thumb:42/star"`).
pub fn star_badge(painter: &Painter, t: &Tokens, id: &str, image: Rect) -> Option<Rect> {
    let plate = badge_row(painter, t, image, &[(Icon::Star, t.star)]).pop()?;
    register(painter.ctx(), id, Role::Badge, "Starred", Target::Painted { rect: plate, clip: painter.clip_rect() }, true);
    Some(plate)
}

/// Paints every mark of `b` over `image` (the photo's letterboxed rect; call after the image): the
/// badge row, and for a video the sprocket strip and the duration pill. Registers nothing (the cell
/// gives the parts it exposes their ids). Returns where each mark went.
pub fn badges(painter: &Painter, t: &Tokens, image: Rect, b: &Badges) -> BadgeRects {
    let mut out = BadgeRects::default();
    if !(image.is_finite() && image.is_positive()) {
        return out;
    }
    let row: Vec<Badge> = b.row().collect();
    let glyphs: Vec<(Glyph, Color32)> = row.iter().map(|&badge| glyph(t, badge)).collect();
    out.row = plates(painter, t, image, &glyphs).into_iter().filter_map(|(i, r)| row.get(i).map(|&badge| (badge, r))).collect();
    if let Some(video) = b.video {
        let strip = sprockets(painter, t, image);
        out.sprockets = strip;
        if let Some(seconds) = video.seconds {
            let left = strip.map_or(image.min.x, |s| s.max.x) + BADGE_INSET;
            let right = out.row.first().map_or(image.max.x - BADGE_INSET, |(_, r)| r.min.x - BADGE_GAP);
            out.duration = duration_pill(painter, t, image, left, right, seconds);
        }
    }
    out
}

/// The film-sprocket strip along the image's left edge: a dark band with light holes. Returns it,
/// or `None` when the image is too small to carry one.
fn sprockets(painter: &Painter, t: &Tokens, image: Rect) -> Option<Rect> {
    let w = (image.width() * 0.07).clamp(5.0, 10.0).round();
    if image.width() < 4.0 * w || image.height() < 2.0 * w {
        return None;
    }
    let strip = Rect::from_min_size(image.min, vec2(w, image.height()));
    painter.rect_filled(strip, 0, t.badge_plate);
    let hole = (w * 0.5).round().max(2.0);
    let pitch = hole * 2.0;
    // Holes evenly spaced, centred along the strip, as many as fit.
    let count = ((image.height() - hole) / pitch).floor().max(0.0) as usize + 1;
    let span = (count as f32 - 1.0) * pitch + hole;
    let top = strip.min.y + (strip.height() - span) / 2.0;
    let x = strip.center().x - hole / 2.0;
    for k in 0..count {
        let r = Rect::from_min_size(pos2(x, top + k as f32 * pitch), Vec2::splat(hole));
        painter.rect_filled(r, CornerRadius::same(1), t.text_label);
    }
    Some(strip)
}

/// The duration pill, bottom-left between `left` and `right` (screen x); `None` when it does not
/// fit there.
fn duration_pill(painter: &Painter, t: &Tokens, image: Rect, left: f32, right: f32, seconds: u32) -> Option<Rect> {
    if image.height() < BADGE_PLATE + 2.0 * BADGE_INSET {
        return None;
    }
    let galley = painter.layout_no_wrap(duration_text(seconds), fonts::regular(t.caption), t.text);
    let size = vec2((galley.size().x + 12.0).round(), BADGE_PLATE);
    if left + size.x > right {
        return None;
    }
    let pill = Rect::from_min_size(pos2(left, image.max.y - BADGE_INSET - BADGE_PLATE), size);
    painter.rect_filled(pill, CornerRadius::same((BADGE_PLATE / 2.0) as u8), t.badge_plate);
    let pos = pill.center() - galley.size() / 2.0;
    painter.galley(pos.round(), galley, t.text);
    Some(pill)
}

/// The `caution` "!" of a photo that cannot be shown (a missing or unreadable file), centred in
/// `image`, sized to it (12–28 pt).
pub fn failed_mark(painter: &Painter, t: &Tokens, image: Rect) {
    if !(image.is_finite() && image.is_positive()) {
        return;
    }
    let size = (image.width().min(image.height()) * 0.3).clamp(12.0, 28.0);
    painter.text(image.center(), Align2::CENTER_CENTER, "!", fonts::semibold(size), t.caution);
}

/// The excluded mark of the import tray (`organizing.md` §3): a duplicate or excluded picture's
/// `cell` dimmed with `excluded_dim` (`black α150`, over the photo too: call after it) and a
/// `danger` ⊘ centred on `image` (the photo's letterboxed rect): a circle 40 % of the image's
/// shorter side across (16–48 pt) with a stroke a tenth of that (at least 2 pt) and a slash from its
/// bottom-left to its top-right. Returns the mark's square, `None` for an empty or non-finite rect.
/// Registers nothing (the cell gives its parts their ids).
pub fn excluded_mark(painter: &Painter, t: &Tokens, cell: Rect, image: Rect) -> Option<Rect> {
    if !(cell.is_finite() && cell.is_positive() && image.is_finite() && image.is_positive()) {
        return None;
    }
    painter.rect_filled(cell, 0, t.excluded_dim);
    let d = (image.width().min(image.height()) * 0.4).clamp(16.0, 48.0);
    let stroke = Stroke::new((d * 0.1).max(2.0), t.danger);
    let r = (d - stroke.width) / 2.0;
    let c = image.center();
    painter.circle_stroke(c, r, stroke);
    let k = r * std::f32::consts::FRAC_1_SQRT_2;
    painter.line_segment([pos2(c.x - k, c.y + k), pos2(c.x + k, c.y - k)], stroke);
    Some(Rect::from_center_size(c, Vec2::splat(d)))
}

/// The square `rect` is centred in and the map from the 20 × 20 grid to points.
fn grid(rect: Rect) -> Option<(f32, impl Fn(f32, f32) -> Pos2)> {
    let side = rect.width().min(rect.height());
    if !(side.is_finite() && side > 0.0 && rect.center().is_finite()) {
        return None;
    }
    let k = side / 20.0;
    let origin = rect.center() - Vec2::splat(side) / 2.0;
    Some((side, move |x: f32, y: f32| pos2(origin.x + x * k, origin.y + y * k)))
}

/// The edited mark: a pencil, point down-left, on the icons' grid and stroke. A part of the badge,
/// not a shell icon.
fn pencil(painter: &Painter, rect: Rect, color: Color32) {
    let Some((side, at)) = grid(rect) else { return };
    let stroke = Stroke::new(icons::stroke_width(side), color);
    let body = vec![at(13.5, 3.5), at(16.5, 6.5), at(7.5, 15.5), at(3.5, 16.5), at(4.5, 12.5)];
    painter.add(Shape::closed_line(body, stroke));
    painter.line_segment([at(11.5, 5.5), at(14.5, 8.5)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_clock_times() {
        assert_eq!(duration_text(0), "0:00");
        assert_eq!(duration_text(83), "1:23");
        assert_eq!(duration_text(725), "12:05");
        assert_eq!(duration_text(3723), "1:02:03");
        assert_eq!(duration_text(u32::MAX), "1193046:28:15");
    }

    #[test]
    fn plates_fit_from_the_right() {
        assert_eq!(plates_that_fit(0.0), 0);
        assert_eq!(plates_that_fit(f32::NAN), 0);
        assert_eq!(plates_that_fit(25.9), 0);
        assert_eq!(plates_that_fit(26.0), 1, "one plate and both insets");
        assert_eq!(plates_that_fit(46.0), 2);
        assert_eq!(plates_that_fit(86.0), 4);
    }

    #[test]
    fn the_row_keeps_its_order_and_combinations_cover_everything() {
        let b = Badges { edited: true, held: true, starred: true, ..Badges::default() };
        assert_eq!(b.row().collect::<Vec<_>>(), [Badge::Edited, Badge::Held, Badge::Starred]);
        let all = Badges::combinations(83);
        assert_eq!(all.len(), 32);
        let unique: std::collections::HashSet<_> = all.iter().map(|b| format!("{b:?}")).collect();
        assert_eq!(unique.len(), 32);
    }
}
