//! The glyphs of saving and exporting (`creating.md` §2, `data-model.md` §6, `design-system.md`
//! §5), our own drawings on the 20 × 20 grid with the shell icons' pen: the disk of Save (the
//! folder header's Save button) and the project of the library tree's Projects group (an export
//! folder, a collage folder).
//!
//! They are a set of their own, [`OutputIcon`] ([`paint_output`]), like the edit view's glyphs, so
//! the shell's icon sheet and its fingerprint stay as they are. Content stays inside 2 … 18 so the
//! stroke never leaves the square.

use egui::{Color32, Painter, Rect};

use super::pen::Pen;

/// The glyphs of saving and exporting. Keep [`OutputIcon::ALL`] in declaration order (a test
/// checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputIcon {
    /// A disk with its shutter and label: Save (edits written into the photo).
    Save,
    /// A briefcase: a project in the tree's Projects group (an export folder, a collage folder).
    Project,
}

impl OutputIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [OutputIcon; 2] = [OutputIcon::Save, OutputIcon::Project];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            OutputIcon::Save => "save",
            OutputIcon::Project => "project",
        }
    }

    /// The glyph called `name` ([`OutputIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<OutputIcon> {
        OutputIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_output(painter: &Painter, rect: Rect, icon: OutputIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

fn draw(p: &Pen, icon: OutputIcon) {
    match icon {
        OutputIcon::Save => {
            // The body with its top-right corner cut, the shutter at the top and the label below.
            p.closed(&[(2.5, 2.5), (14.0, 2.5), (17.5, 6.0), (17.5, 17.5), (2.5, 17.5)]);
            p.elbow(&[(6.0, 2.5), (6.0, 7.0), (13.0, 7.0), (13.0, 2.5)]);
            p.elbow(&[(5.5, 17.5), (5.5, 11.0), (14.5, 11.0), (14.5, 17.5)]);
        }
        OutputIcon::Project => {
            // A briefcase: the case, its handle, the band round it and the clasp on the band.
            p.rect(2.5, 6.0, 17.5, 16.5, 1.5);
            p.elbow(&[(7.0, 6.0), (7.0, 3.0), (13.0, 3.0), (13.0, 6.0)]);
            p.seg((2.5, 10.5), (17.5, 10.5));
            p.fill(&[(8.5, 9.0), (11.5, 9.0), (11.5, 12.0), (8.5, 12.0)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in OutputIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `Project` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(OutputIcon::Project as usize + 1, OutputIcon::ALL.len());
        let mut names: Vec<_> = OutputIcon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OutputIcon::ALL.len(), "names are unique");
        for icon in OutputIcon::ALL {
            assert_eq!(OutputIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(OutputIcon::from_name("folder"), None);
    }
}
