//! Icons drawn in code on a 20 × 20 design grid (`design-system.md` §5): our own drawings, tinted
//! by the caller, stroke `max(1.5·k, 1)` where `k = side / 20`, round caps. No icon fonts, no
//! bitmaps, no SVG files: every glyph of the window shell is drawn here.
//!
//! Use: `icons::paint(painter, rect, Icon::Folder, tokens.icon)` paints the glyph centred in the
//! largest square that fits `rect`; [`paint_activity`] draws the spinner at an animation step.
//! Sizes (§5): 16 tree rows and menus, 18 toolbar toggles, 20 edit tools, 28 big action buttons,
//! 12 thumbnail badges; the caption glyphs read best at 10–12 inside their 46 × 24 buttons.
//!
//! The Folder Manager's folder states and root icons are a set of their own, [`FolderIcon`]
//! ([`paint_folder`]; `icons/folder_modes.rs`), and so are the edit view's, [`EditIcon`]
//! ([`paint_edit`]; `icons/edit.rs`), Save's and Export's, [`OutputIcon`] ([`paint_output`];
//! `icons/output.rs`), the automatic albums', [`AlbumIcon`] ([`paint_album`];
//! `icons/albums.rs`), the shortcut editor's, [`OptionsIcon`] ([`paint_options`];
//! `icons/options.rs`), and the rooms', slideshow's, viewer's, import tray's and compare's,
//! [`ViewingIcon`] ([`paint_viewing`]; `icons/viewing.rs`), and the effect tabs', [`EffectIcon`]
//! ([`paint_effect`]; `icons/effects.rs`). [`Glyph`] names a glyph of any set, for
//! widgets that take glyphs from more than one ([`paint_glyph`]).

mod albums;
mod edit;
mod effects;
mod folder_modes;
mod glyphs;
mod options;
mod output;
mod pen;
mod viewing;

pub use albums::{AlbumIcon, paint_album};
pub use edit::{EditIcon, paint_edit};
pub use effects::{EffectIcon, paint_effect};
pub use folder_modes::{FolderIcon, INHERITED_OPACITY, paint_folder, state_color};
pub use options::{OptionsIcon, paint_options};
pub use output::{OutputIcon, paint_output};
pub use viewing::{ViewingIcon, paint_viewing};

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2};

/// Every icon of the shell. Keep [`Icon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// A filled five-pointed star: the photo is starred; the Starred filter.
    Star,
    /// The same star as an outline: not starred.
    StarOutline,
    /// A plus sign: add something.
    Plus,
    /// A camera: Import.
    Import,
    /// A photo album (a book with a picture on its cover): albums in the tree.
    Album,
    /// An album with a plus: New Album.
    NewAlbum,
    /// An album with a caret: Add to Album ▾ in the tray.
    AddToAlbum,
    /// Three bulleted rows: the tree shown as a flat list.
    FlatList,
    /// An indented hierarchy: the tree shown as a tree.
    Tree,
    /// A chevron pointing down: expanded rows, dropdowns, view options.
    ChevronDown,
    /// A chevron pointing up.
    ChevronUp,
    /// A chevron pointing right: collapsed rows, submenus.
    ChevronRight,
    /// A chevron pointing left.
    ChevronLeft,
    /// Two chevrons pointing left («): the side-pane header.
    ChevronsLeft,
    /// A smiling face: the Faces filter.
    Faces,
    /// A video camera: the Videos filter.
    Videos,
    /// A map pin: the Geotagged filter.
    Geotagged,
    /// A magnifier: the search field.
    Search,
    /// A cross in a circle: clear the search field.
    ClearSearch,
    /// Eight spokes, graded: the activity indicator ([`paint_activity`] animates it).
    Activity,
    /// A push pin: Hold (keep photos in the tray).
    Hold,
    /// A circle with a slash: Clear the tray.
    ClearTray,
    /// An arrow turning counterclockwise: Rotate Counterclockwise.
    RotateCcw,
    /// An arrow turning clockwise: Rotate Clockwise.
    RotateCw,
    /// A magnifier with a plus: the loupe.
    Loupe,
    /// Two people: the People pane toggle.
    People,
    /// A folded map: the Places pane toggle.
    Places,
    /// A price tag: the Tags pane toggle.
    Tags,
    /// "i" in a circle: the Properties pane toggle.
    Properties,
    /// An arrow leaving a box: Export.
    Export,
    /// A frame split into three pictures: Collage.
    Collage,
    /// A screen with a play triangle: Slideshow.
    Slideshow,
    /// Three dots: More… (overflow).
    More,
    /// A folder: folders in the tree.
    Folder,
    /// A horizontal bar: the Minimize caption button.
    Minimize,
    /// A square: the Maximize caption button.
    Maximize,
    /// Two overlapping squares: the Restore caption button.
    Restore,
    /// A cross: the Close caption button, close buttons.
    Close,
}

impl Icon {
    /// Every icon, in declaration order.
    pub const ALL: [Icon; 38] = [
        Icon::Star,
        Icon::StarOutline,
        Icon::Plus,
        Icon::Import,
        Icon::Album,
        Icon::NewAlbum,
        Icon::AddToAlbum,
        Icon::FlatList,
        Icon::Tree,
        Icon::ChevronDown,
        Icon::ChevronUp,
        Icon::ChevronRight,
        Icon::ChevronLeft,
        Icon::ChevronsLeft,
        Icon::Faces,
        Icon::Videos,
        Icon::Geotagged,
        Icon::Search,
        Icon::ClearSearch,
        Icon::Activity,
        Icon::Hold,
        Icon::ClearTray,
        Icon::RotateCcw,
        Icon::RotateCw,
        Icon::Loupe,
        Icon::People,
        Icon::Places,
        Icon::Tags,
        Icon::Properties,
        Icon::Export,
        Icon::Collage,
        Icon::Slideshow,
        Icon::More,
        Icon::Folder,
        Icon::Minimize,
        Icon::Maximize,
        Icon::Restore,
        Icon::Close,
    ];

    /// The icon's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            Icon::Star => "star",
            Icon::StarOutline => "star-outline",
            Icon::Plus => "plus",
            Icon::Import => "import",
            Icon::Album => "album",
            Icon::NewAlbum => "new-album",
            Icon::AddToAlbum => "add-to-album",
            Icon::FlatList => "flat-list",
            Icon::Tree => "tree",
            Icon::ChevronDown => "chevron-down",
            Icon::ChevronUp => "chevron-up",
            Icon::ChevronRight => "chevron-right",
            Icon::ChevronLeft => "chevron-left",
            Icon::ChevronsLeft => "chevrons-left",
            Icon::Faces => "faces",
            Icon::Videos => "videos",
            Icon::Geotagged => "geotagged",
            Icon::Search => "search",
            Icon::ClearSearch => "clear-search",
            Icon::Activity => "activity",
            Icon::Hold => "hold",
            Icon::ClearTray => "clear-tray",
            Icon::RotateCcw => "rotate-ccw",
            Icon::RotateCw => "rotate-cw",
            Icon::Loupe => "loupe",
            Icon::People => "people",
            Icon::Places => "places",
            Icon::Tags => "tags",
            Icon::Properties => "properties",
            Icon::Export => "export",
            Icon::Collage => "collage",
            Icon::Slideshow => "slideshow",
            Icon::More => "more",
            Icon::Folder => "folder",
            Icon::Minimize => "minimize",
            Icon::Maximize => "maximize",
            Icon::Restore => "restore",
            Icon::Close => "close",
        }
    }

    /// The icon called `name` ([`Icon::name`]), if any.
    pub fn from_name(name: &str) -> Option<Icon> {
        Icon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// A glyph of any set: for widgets whose callers pick glyphs from more than one (a room tab's
/// Library and Import, the slideshow bar's Play and Pause).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Glyph {
    /// A shell icon.
    Shell(Icon),
    /// An edit-view glyph.
    Edit(EditIcon),
    /// A viewing glyph.
    Viewing(ViewingIcon),
    /// An effect-tab glyph.
    Effect(EffectIcon),
}

impl From<Icon> for Glyph {
    fn from(icon: Icon) -> Glyph {
        Glyph::Shell(icon)
    }
}

impl From<EditIcon> for Glyph {
    fn from(icon: EditIcon) -> Glyph {
        Glyph::Edit(icon)
    }
}

impl From<ViewingIcon> for Glyph {
    fn from(icon: ViewingIcon) -> Glyph {
        Glyph::Viewing(icon)
    }
}

impl From<EffectIcon> for Glyph {
    fn from(icon: EffectIcon) -> Glyph {
        Glyph::Effect(icon)
    }
}

/// Paints `glyph` in `color`, centred in the largest square that fits `rect`, with its set's
/// painter. An empty or non-finite rect paints nothing.
pub fn paint_glyph(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    match glyph {
        Glyph::Shell(icon) => paint(painter, rect, icon, color),
        Glyph::Edit(icon) => paint_edit(painter, rect, icon, color),
        Glyph::Viewing(icon) => paint_viewing(painter, rect, icon, color),
        Glyph::Effect(icon) => paint_effect(painter, rect, icon, color),
    }
}

/// The five outer and five inner points of the star on the 20 × 20 grid, mapped into `rect`.
fn star_points(rect: Rect) -> Vec<Pos2> {
    let k = rect.width().min(rect.height()) / 20.0;
    let c = rect.center() + egui::vec2(0.0, 0.6 * k);
    let (outer, inner) = (8.6 * k, 3.7 * k);
    (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { outer } else { inner };
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// The stroke width of an icon drawn in a square of `side` points.
pub fn stroke_width(side: f32) -> f32 {
    (1.5 * side / 20.0).max(1.0)
}

/// The largest square that fits `rect`, centred in it; `None` for an empty or non-finite rect.
fn square(rect: Rect) -> Option<Rect> {
    let side = rect.width().min(rect.height());
    (side.is_finite() && side > 0.0 && rect.center().is_finite()).then(|| Rect::from_center_size(rect.center(), egui::vec2(side, side)))
}

/// Paints the activity spinner in `color` at animation `phase` (in turns; the bright spoke steps
/// clockwise eight times per turn), centred in the largest square that fits `rect`. Phase 0 is
/// [`Icon::Activity`]; a non-finite phase draws phase 0.
pub fn paint_activity(painter: &Painter, rect: Rect, color: Color32, phase: f32) {
    let Some(rect) = square(rect) else { return };
    let step = if phase.is_finite() { (phase.rem_euclid(1.0) * 8.0).floor() as u32 % 8 } else { 0 };
    glyphs::spokes(&pen::Pen::new(painter, rect, color), step);
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let Some(rect) = square(rect) else { return };
    let side = rect.width();
    let k = side / 20.0;
    match icon {
        Icon::Star => {
            let points = star_points(rect);
            // The star is concave: fill it as a fan around its centre, then stroke the outline thinly
            // so its edges are anti-aliased like egui's own shapes.
            let mut mesh = Mesh::default();
            mesh.colored_vertex(rect.center() + egui::vec2(0.0, 0.6 * k), color);
            for &p in &points {
                mesh.colored_vertex(p, color);
            }
            for i in 0..10u32 {
                mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
            }
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::closed_line(points, Stroke::new((0.6 * k).max(0.5), color)));
        }
        Icon::StarOutline => {
            painter.add(Shape::closed_line(star_points(rect.shrink(0.5 * stroke_width(side))), Stroke::new(stroke_width(side), color)));
        }
        Icon::Plus => {
            let (c, r) = (rect.center(), 6.0 * k);
            let stroke = Stroke::new(stroke_width(side), color);
            painter.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], stroke);
            painter.line_segment([c - egui::vec2(0.0, r), c + egui::vec2(0.0, r)], stroke);
        }
        other => glyphs::draw(&pen::Pen::new(painter, rect, color), other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_star_fits_its_square_and_scales() {
        for side in [12.0, 20.0, 28.0] {
            let rect = Rect::from_min_size(pos2(5.0, 7.0), egui::vec2(side, side));
            let points = star_points(rect);
            assert_eq!(points.len(), 10);
            assert!(points.iter().all(|p| rect.expand(0.01).contains(*p)), "{side}: {points:?}");
        }
        assert_eq!(stroke_width(12.0), 1.0);
        assert_eq!(stroke_width(20.0), 1.5);
    }

    #[test]
    fn all_lists_every_icon_once_in_declaration_order() {
        for (i, icon) in Icon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `Close` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(Icon::Close as usize + 1, Icon::ALL.len());
        let mut names: Vec<_> = Icon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Icon::ALL.len(), "names are unique");
        for icon in Icon::ALL {
            assert_eq!(Icon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(Icon::from_name("camera"), None);
    }

    #[test]
    fn arcs_have_at_least_two_points_and_end_where_asked() {
        let pts = pen::arc_points(10.0, 10.0, 5.0, 0.0, 90.0);
        assert!(pts.len() >= 10);
        let last = pts.last().copied().unwrap();
        assert!((last.0 - 10.0).abs() < 1e-4 && (last.1 - 15.0).abs() < 1e-4, "{last:?}");
        assert_eq!(pen::arc_points(0.0, 0.0, 1.0, 5.0, 5.0).len(), 2);
        assert_eq!(pen::arc_points(0.0, 0.0, 1.0, 0.0, f32::NAN).len(), 2);
    }
}
