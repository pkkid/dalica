//! The glyphs of the Options dialog's shortcut editor (`menus-and-shortcuts.md` §2.0,
//! `design-system.md` §5): our own drawings on the 20 × 20 grid with the shell icons' pen. **Restore**
//! (↺) puts a shortcut back to its default; **Clear** (×) removes a key from a key cell.
//!
//! They are a set of their own, [`OptionsIcon`] ([`paint_options`]), like the edit view's glyphs, so
//! the shell's icon sheet and its fingerprint stay as they are. Content stays inside 2 … 18 so the
//! stroke never leaves the square. Restore is drawn apart from the shell's Rotate Counterclockwise
//! (its arrow at the left end pointing down, a dot at its centre: the default it goes back to), and
//! Clear apart from the shell's Close (the × on a key cap pointing left, the Backspace key that also
//! removes a key while recording).

use egui::{Color32, Painter, Rect};

use super::pen::Pen;

/// The shortcut editor's glyphs. Keep [`OptionsIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionsIcon {
    /// An open circle turning counterclockwise round a dot (↺): back to the default.
    Restore,
    /// A × on a key cap pointing left: remove the key.
    Clear,
}

impl OptionsIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [OptionsIcon; 2] = [OptionsIcon::Restore, OptionsIcon::Clear];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            OptionsIcon::Restore => "restore-default",
            OptionsIcon::Clear => "clear-key",
        }
    }

    /// The glyph called `name` ([`OptionsIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<OptionsIcon> {
        OptionsIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_options(painter: &Painter, rect: Rect, icon: OptionsIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

fn draw(p: &Pen, icon: OptionsIcon) {
    match icon {
        OptionsIcon::Restore => {
            // Three quarters of a circle from its left over the top round to its bottom, the arrow
            // at the left end pointing down (counterclockwise), and the default at its centre.
            p.arc(10.5, 10.5, 6.5, 180.0, 450.0);
            p.fill(&[(1.75, 9.25), (6.25, 9.25), (4.0, 13.5)]);
            p.dot(10.5, 10.5, 1.75);
        }
        OptionsIcon::Clear => {
            // A key cap pointing left with a × on it.
            p.closed(&[(7.0, 4.5), (17.5, 4.5), (17.5, 15.5), (7.0, 15.5), (2.5, 10.0)]);
            p.seg((9.75, 7.75), (14.25, 12.25));
            p.seg((14.25, 7.75), (9.75, 12.25));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in OptionsIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `Clear` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(OptionsIcon::Clear as usize + 1, OptionsIcon::ALL.len());
        let mut names: Vec<_> = OptionsIcon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OptionsIcon::ALL.len(), "names are unique");
        for icon in OptionsIcon::ALL {
            assert_eq!(OptionsIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(OptionsIcon::from_name("restore"), None);
    }
}
