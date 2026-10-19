//! The glyphs of the operating system's ways to reach a file (`ui-layout.md`, `viewing.md` §4,
//! `organizing.md`, `design-system.md` §5): Open With, Locate on Disk, the external editor and a
//! camera on the import's device list; our own drawings on the 20 × 20 grid with the shell icons'
//! pen.
//!
//! They are a set of their own, [`OsIcon`] ([`paint_os`]), so the other sets' sheets and
//! fingerprints stay as they are. Content stays inside 2 … 18 so the stroke never leaves the
//! square. Meanings the kit already draws are not drawn twice: **Camera** is the shell's camera
//! ([`Icon::Import`]); the set names it so a panel takes every glyph of the system from one place.
//!
//! The new drawings: **Open With** (a window with its title bar and an arrow leaving it at its
//! top right, apart from the viewer's Open in Dalica whose arrow comes in), **Locate on Disk** (a
//! folder with a magnifier over its bottom-right corner) and **External editor** (a picture frame
//! open at its top right with a pencil across the gap).

use egui::{Color32, Painter, Rect};

use super::pen::Pen;
use super::{Glyph, Icon};

/// The glyphs of the system's file hand-offs. Keep [`OsIcon::ALL`] in declaration order (a test
/// checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsIcon {
    /// A window with an arrow leaving it at its top right: Open With.
    OpenWith,
    /// A folder under a magnifier: Locate on Disk.
    LocateOnDisk,
    /// A picture frame with a pencil across its open corner: Edit in the external editor.
    ExternalEditor,
    /// A camera: a camera or phone on the import's device list (the shell's [`Icon::Import`]).
    Camera,
}

impl OsIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [OsIcon; 4] = [OsIcon::OpenWith, OsIcon::LocateOnDisk, OsIcon::ExternalEditor, OsIcon::Camera];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            OsIcon::OpenWith => "open-with",
            OsIcon::LocateOnDisk => "locate-on-disk",
            OsIcon::ExternalEditor => "external-editor",
            OsIcon::Camera => "camera",
        }
    }

    /// The glyph called `name` ([`OsIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<OsIcon> {
        OsIcon::ALL.into_iter().find(|i| i.name() == name)
    }

    /// The glyph of another set this one is, for the meanings the kit already draws.
    pub fn shared(self) -> Option<Glyph> {
        match self {
            OsIcon::Camera => Some(Glyph::Shell(Icon::Import)),
            _ => None,
        }
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_os(painter: &Painter, rect: Rect, icon: OsIcon, color: Color32) {
    if let Some(glyph) = icon.shared() {
        super::paint_glyph(painter, rect, glyph, color);
        return;
    }
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

fn draw(p: &Pen, icon: OsIcon) {
    match icon {
        OsIcon::OpenWith => {
            // A window open at its top right, its title bar, an arrow leaving it there.
            p.elbow(&[(9.5, 5.5), (2.5, 5.5), (2.5, 17.5), (14.5, 17.5), (14.5, 10.5)]);
            p.seg((2.5, 9.0), (6.5, 9.0));
            p.seg((7.5, 13.0), (17.0, 3.5));
            p.line(&[(11.5, 3.5), (17.0, 3.5), (17.0, 9.0)]);
        }
        OsIcon::LocateOnDisk => {
            // A folder whose right and bottom edges stop short of the magnifier over its corner.
            p.elbow(&[(9.5, 15.0), (2.5, 15.0), (2.5, 3.5), (7.0, 3.5)]);
            p.seg((7.0, 3.5), (8.5, 5.0));
            p.elbow(&[(8.5, 5.0), (15.5, 5.0), (15.5, 9.0)]);
            p.circle(13.0, 13.0, 3.25);
            let h = 13.0 + 3.25 * std::f32::consts::FRAC_1_SQRT_2;
            p.seg((h, h), (17.5, 17.5));
        }
        OsIcon::ExternalEditor => {
            // A frame open at its top right; a pencil from there down into it, tip at the bottom left.
            p.elbow(&[(9.5, 4.0), (2.5, 4.0), (2.5, 17.0), (15.5, 17.0), (15.5, 11.0)]);
            p.closed(&[(15.4, 2.4), (17.6, 4.6), (10.6, 11.6), (7.5, 12.5), (8.4, 9.4)]);
        }
        // Drawn by the set it is shared with ([`OsIcon::shared`]).
        OsIcon::Camera => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in OsIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `Camera` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(OsIcon::Camera as usize + 1, OsIcon::ALL.len());
        for icon in OsIcon::ALL {
            assert_eq!(OsIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(OsIcon::from_name("import"), None);
    }
}
