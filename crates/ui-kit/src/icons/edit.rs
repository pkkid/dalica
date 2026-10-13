//! The edit view's glyphs (`editing.md` §2, §5–§7; `design-system.md` §5), our own drawings on
//! the 20 × 20 grid with the shell icons' pen: the edit panel's five tabs, the Basic Fixes tools,
//! the Tuning tab's magic wand and eyedropper, and the top bar's and caption bar's buttons.
//!
//! They are a set of their own, [`EditIcon`] ([`paint_edit`]), like the Folder Manager's glyphs,
//! so the shell's icon sheet and its fingerprint stay as they are. Content stays inside 2 … 18 so
//! the stroke never leaves the square.

use egui::{Color32, Mesh, Painter, Rect, Shape, Stroke};

use super::pen::{Pen, arc_points};

/// The edit view's glyphs. Keep [`EditIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditIcon {
    /// A wrench: the Basic Fixes tab.
    BasicFixes,
    /// A sun half risen over the horizon: the Lighting and Color Tuning tab.
    Tuning,
    /// A paint brush: the Effects tab.
    Effects,
    /// A paint brush over two hills: the More Effects tab.
    MoreEffects,
    /// A paint brush over a cloud: the Even More Effects tab.
    EvenMoreEffects,
    /// Two interlocking corner brackets: Crop.
    Crop,
    /// A tilted picture over a level line: Straighten.
    Straighten,
    /// A sticking plaster across the square: Retouch.
    Retouch,
    /// A large and a small four-pointed sparkle: Auto Fix.
    AutoFix,
    /// A circle half filled: Auto Contrast.
    AutoContrast,
    /// Three overlapping circles: Auto Color.
    AutoColor,
    /// A full sun with eight rays: Fill Light.
    FillLight,
    /// A wand with a sparkle at its tip: Auto lighting and Auto color in the Tuning tab.
    MagicWand,
    /// A pipette with its bulb: the Neutral Color Picker.
    Eyedropper,
    /// An arrow pointing left: Back to Library.
    BackToLibrary,
    /// A play triangle in a circle: Play (a slideshow from this photo).
    Play,
    /// A solid triangle pointing left: the filmstrip's previous button.
    FilmstripPrevious,
    /// A solid triangle pointing right: the filmstrip's next button.
    FilmstripNext,
    /// A picture with a line of text under it: the caption bar's show-captions toggle.
    CaptionToggle,
    /// A waste bin with its lid: the caption bar's trash button.
    Trash,
}

impl EditIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [EditIcon; 20] = [
        EditIcon::BasicFixes,
        EditIcon::Tuning,
        EditIcon::Effects,
        EditIcon::MoreEffects,
        EditIcon::EvenMoreEffects,
        EditIcon::Crop,
        EditIcon::Straighten,
        EditIcon::Retouch,
        EditIcon::AutoFix,
        EditIcon::AutoContrast,
        EditIcon::AutoColor,
        EditIcon::FillLight,
        EditIcon::MagicWand,
        EditIcon::Eyedropper,
        EditIcon::BackToLibrary,
        EditIcon::Play,
        EditIcon::FilmstripPrevious,
        EditIcon::FilmstripNext,
        EditIcon::CaptionToggle,
        EditIcon::Trash,
    ];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            EditIcon::BasicFixes => "basic-fixes",
            EditIcon::Tuning => "tuning",
            EditIcon::Effects => "effects",
            EditIcon::MoreEffects => "more-effects",
            EditIcon::EvenMoreEffects => "even-more-effects",
            EditIcon::Crop => "crop",
            EditIcon::Straighten => "straighten",
            EditIcon::Retouch => "retouch",
            EditIcon::AutoFix => "auto-fix",
            EditIcon::AutoContrast => "auto-contrast",
            EditIcon::AutoColor => "auto-color",
            EditIcon::FillLight => "fill-light",
            EditIcon::MagicWand => "magic-wand",
            EditIcon::Eyedropper => "eyedropper",
            EditIcon::BackToLibrary => "back-to-library",
            EditIcon::Play => "play",
            EditIcon::FilmstripPrevious => "filmstrip-previous",
            EditIcon::FilmstripNext => "filmstrip-next",
            EditIcon::CaptionToggle => "caption-toggle",
            EditIcon::Trash => "trash",
        }
    }

    /// The glyph called `name` ([`EditIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<EditIcon> {
        EditIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_edit(painter: &Painter, rect: Rect, icon: EditIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(painter, &Pen::new(painter, rect, color), icon);
}

/// A point `s` along and `t` across the axis that leaves (`x0`, `y0`) towards the bottom-left at
/// 45° (`t` grows towards the bottom-right).
fn diag(x0: f32, y0: f32, s: f32, t: f32) -> (f32, f32) {
    let h = std::f32::consts::FRAC_1_SQRT_2;
    (x0 - s * h + t * h, y0 + s * h + t * h)
}

/// A paint brush from its handle's end at (`x0`, `y0`) down-left to its bristles' tip, `len` grid
/// units long: a handle, a solid ferrule and a tapering tuft.
fn brush(p: &Pen, x0: f32, y0: f32, len: f32) {
    let u = len / 17.5;
    let at = |s: f32, t: f32| diag(x0, y0, s * u, t * u);
    p.line(&[at(0.0, 0.0), at(7.0, 0.0)]);
    p.fill(&[at(7.0, -1.7), at(7.0, 1.7), at(8.8, 1.7), at(8.8, -1.7)]);
    p.fill(&[at(8.8, -1.7), at(8.8, 1.7), at(12.0, 2.2), at(15.0, 1.4), at(17.5, 0.0), at(15.0, -1.4), at(12.0, -2.2)]);
}

/// A filled four-pointed sparkle round (`cx`, `cy`): points `outer` from the centre, waist `inner`.
/// The shape is concave, so it is filled as a fan round its centre and outlined thinly so its
/// edges are anti-aliased like egui's own shapes.
fn sparkle(painter: &Painter, p: &Pen, cx: f32, cy: f32, outer: f32, inner: f32) {
    let points: Vec<_> = (0..8)
        .map(|i| {
            let r = if i % 2 == 0 { outer } else { inner };
            let a = (i as f32 * 45.0 - 90.0).to_radians();
            p.pt(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    let mut mesh = Mesh::default();
    mesh.colored_vertex(p.pt(cx, cy), p.color());
    for &pt in &points {
        mesh.colored_vertex(pt, p.color());
    }
    for i in 0..8u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 8);
    }
    painter.add(Shape::mesh(mesh));
    let k = (p.pt(1.0, 0.0).x - p.pt(0.0, 0.0).x).abs();
    painter.add(Shape::closed_line(points, Stroke::new((0.5 * k).max(0.5), p.color())));
}

/// A filmstrip arrow: a solid triangle pointing right (mirror the pen for left).
fn filmstrip_arrow(p: &Pen) {
    p.fill(&[(6.5, 3.5), (15.0, 10.0), (6.5, 16.5)]);
}

fn draw(painter: &Painter, p: &Pen, icon: EditIcon) {
    match icon {
        EditIcon::BasicFixes => {
            // An open-ended wrench: a ring with its jaw facing up-right, the handle down-left.
            // The handle is solid and tapers, so the glyph never reads as a magnifier.
            let (cx, cy, r) = (12.6, 7.4, 4.0);
            p.line(&arc_points(cx, cy, r, 5.0, 265.0));
            let at = |s: f32, t: f32| diag(cx, cy, s, t);
            p.fill(&[at(r - 0.6, -1.8), at(r - 0.6, 1.8), at(r + 8.6, 1.3), at(r + 8.6, -1.3)]);
            let end = at(r + 8.6, 0.0);
            p.dot(end.0, end.1, 1.3);
        }
        EditIcon::Tuning => {
            // A sun half risen: a semicircle on the horizon, five rays, a shorter line below.
            let (cx, cy) = (10.0, 12.5);
            p.line(&arc_points(cx, cy, 4.0, 180.0, 360.0));
            for a in [195.0_f32, 235.0, 270.0, 305.0, 345.0] {
                let (s, c) = a.to_radians().sin_cos();
                p.seg((cx + 5.8 * c, cy + 5.8 * s), (cx + 7.8 * c, cy + 7.8 * s));
            }
            p.seg((2.5, 12.5), (17.5, 12.5));
            p.seg((6.0, 16.0), (14.0, 16.0));
        }
        EditIcon::Effects => brush(p, 16.8, 3.2, 18.0),
        EditIcon::MoreEffects => {
            // Two hills on the ground and a smaller brush above them.
            p.fill(&[(2.5, 17.0), (7.0, 10.5), (11.5, 17.0)]);
            p.fill(&[(8.5, 17.0), (12.5, 12.5), (16.5, 17.0)]);
            brush(p, 17.5, 2.5, 10.5);
        }
        EditIcon::EvenMoreEffects => {
            // A cloud and a smaller brush above it.
            let mut cloud = arc_points(5.3, 14.3, 2.2, 90.0, 270.0);
            cloud.extend(arc_points(8.6, 13.0, 3.3, 200.0, 335.0));
            cloud.extend(arc_points(12.6, 14.6, 1.9, 270.0, 450.0));
            p.closed(&cloud);
            brush(p, 17.5, 2.5, 10.5);
        }
        EditIcon::Crop => {
            p.elbow(&[(6.0, 2.5), (6.0, 14.0), (17.5, 14.0)]);
            p.elbow(&[(2.5, 6.0), (14.0, 6.0), (14.0, 17.5)]);
        }
        EditIcon::Straighten => {
            // A picture turned 10° over a level horizontal line.
            let (s, c) = (-10.0_f32).to_radians().sin_cos();
            let corner = |x: f32, y: f32| (10.0 + x * c - y * s, 10.0 + x * s + y * c);
            p.closed(&[corner(-6.3, -4.3), corner(6.3, -4.3), corner(6.3, 4.3), corner(-6.3, 4.3)]);
            p.seg((2.5, 10.0), (17.5, 10.0));
        }
        EditIcon::Retouch => {
            // A plaster at 45°: a stadium outline with its square pad in the middle.
            let (s, c) = (-45.0_f32).to_radians().sin_cos();
            let at = |(x, y): (f32, f32)| (10.0 + x * c - y * s, 10.0 + x * s + y * c);
            let mut outline = arc_points(4.5, 0.0, 3.0, -90.0, 90.0);
            outline.extend(arc_points(-4.5, 0.0, 3.0, 90.0, 270.0));
            p.closed(&outline.into_iter().map(at).collect::<Vec<_>>());
            p.closed(&[at((-2.2, -3.0)), at((2.2, -3.0)), at((2.2, 3.0)), at((-2.2, 3.0))]);
        }
        EditIcon::AutoFix => {
            sparkle(painter, p, 8.8, 11.2, 6.8, 1.9);
            sparkle(painter, p, 15.0, 5.0, 3.0, 0.9);
        }
        EditIcon::AutoContrast => {
            p.fill(&arc_points(10.0, 10.0, 7.0, 90.0, 270.0));
            p.circle(10.0, 10.0, 7.0);
        }
        EditIcon::AutoColor => {
            p.circle(10.0, 7.0, 4.2);
            p.circle(7.4, 11.6, 4.2);
            p.circle(12.6, 11.6, 4.2);
        }
        EditIcon::FillLight => {
            p.circle(10.0, 10.0, 3.4);
            for i in 0..8 {
                let (s, c) = (i as f32 * 45.0).to_radians().sin_cos();
                p.seg((10.0 + 5.6 * c, 10.0 + 5.6 * s), (10.0 + 7.6 * c, 10.0 + 7.6 * s));
            }
        }
        EditIcon::MagicWand => {
            p.line(&[(3.5, 16.5), (11.5, 8.5)]);
            sparkle(painter, p, 14.0, 6.0, 3.8, 1.1);
            p.dot(6.0, 5.5, 1.0);
            p.dot(15.5, 13.0, 0.8);
        }
        EditIcon::Eyedropper => {
            // A pipette from its bulb at the top right to its tip at the bottom left.
            let (x0, y0) = (16.6, 3.4);
            let at = |s: f32, t: f32| diag(x0, y0, s, t);
            let bulb = at(2.4, 0.0);
            p.dot(bulb.0, bulb.1, 2.3);
            p.line(&[at(5.0, -2.8), at(5.0, 2.8)]);
            p.closed(&[at(5.8, -1.3), at(13.0, -1.3), at(16.4, 0.0), at(13.0, 1.3), at(5.8, 1.3)]);
        }
        EditIcon::BackToLibrary => {
            p.seg((3.5, 10.0), (16.5, 10.0));
            p.line(&[(8.5, 5.0), (3.5, 10.0), (8.5, 15.0)]);
        }
        EditIcon::Play => {
            p.circle(10.0, 10.0, 7.5);
            p.fill(&[(8.0, 6.3), (14.2, 10.0), (8.0, 13.7)]);
        }
        EditIcon::FilmstripPrevious => filmstrip_arrow(&p.mirrored()),
        EditIcon::FilmstripNext => filmstrip_arrow(p),
        EditIcon::CaptionToggle => {
            // A landscape picture and a caption line under it.
            p.rect(2.5, 3.0, 17.5, 12.5, 1.5);
            p.fill(&[(4.5, 11.0), (8.5, 7.0), (12.5, 11.0)]);
            p.dot(13.5, 6.3, 1.2);
            p.seg((5.0, 16.0), (15.0, 16.0));
        }
        EditIcon::Trash => {
            p.seg((3.0, 5.0), (17.0, 5.0));
            p.elbow(&[(8.0, 5.0), (8.0, 2.8), (12.0, 2.8), (12.0, 5.0)]);
            p.line(&[(5.0, 5.0), (6.0, 17.0), (14.0, 17.0), (15.0, 5.0)]);
            p.seg((8.5, 8.0), (8.5, 14.0));
            p.seg((11.5, 8.0), (11.5, 14.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in EditIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `Trash` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(EditIcon::Trash as usize + 1, EditIcon::ALL.len());
        let mut names: Vec<_> = EditIcon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EditIcon::ALL.len(), "names are unique");
        for icon in EditIcon::ALL {
            assert_eq!(EditIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(EditIcon::from_name("folder"), None);
    }

    #[test]
    fn the_diagonal_axis_runs_down_left_at_45_degrees() {
        let (x, y) = diag(10.0, 10.0, std::f32::consts::SQRT_2, 0.0);
        assert!((x - 9.0).abs() < 1e-5 && (y - 11.0).abs() < 1e-5, "{x} {y}");
        let (x, y) = diag(10.0, 10.0, 0.0, std::f32::consts::SQRT_2);
        assert!((x - 11.0).abs() < 1e-5 && (y - 11.0).abs() < 1e-5, "{x} {y}");
    }
}
