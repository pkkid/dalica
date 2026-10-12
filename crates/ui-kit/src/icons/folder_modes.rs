//! The Folder Manager's glyphs (`organizing.md` §2, `design-system.md` §5), our own drawings on
//! the 20 × 20 grid with the shell icons' pen: the three folder states (Scan Always, Scan Once,
//! Remove from Dalica) and the icons of the tree's roots (the standard places and the volumes).
//!
//! They are a set of their own, [`FolderIcon`], so the shell's icon sheet and its fingerprint stay
//! as they are. A state the folder inherits from a parent is drawn at [`INHERITED_OPACITY`]
//! ([`state_color`]).

use egui::{Color32, Painter, Rect};

use super::pen::{Pen, arc_points};

/// The Folder Manager's glyphs. Keep [`FolderIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderIcon {
    /// Two arrows chasing each other round a circle: Scan Always (indexed and watched).
    ScanAlways,
    /// A check mark: Scan Once (indexed once, then left as it is).
    ScanOnce,
    /// A cross: Remove from Dalica (not indexed).
    Remove,
    /// A framed picture: the Pictures folder.
    Pictures,
    /// A page with a folded corner and lines of text: the Documents folder.
    Documents,
    /// A monitor on its stand: the Desktop folder.
    Desktop,
    /// A house: the home folder.
    Home,
    /// A disk drive with its light: a local volume.
    Drive,
    /// A drive on a network line: a network volume.
    NetworkDrive,
}

/// The opacity of a state a folder inherits rather than sets itself (`organizing.md` §2: 50 %).
pub const INHERITED_OPACITY: f32 = 0.5;

impl FolderIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [FolderIcon; 9] = [
        FolderIcon::ScanAlways,
        FolderIcon::ScanOnce,
        FolderIcon::Remove,
        FolderIcon::Pictures,
        FolderIcon::Documents,
        FolderIcon::Desktop,
        FolderIcon::Home,
        FolderIcon::Drive,
        FolderIcon::NetworkDrive,
    ];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            FolderIcon::ScanAlways => "scan-always",
            FolderIcon::ScanOnce => "scan-once",
            FolderIcon::Remove => "remove",
            FolderIcon::Pictures => "pictures",
            FolderIcon::Documents => "documents",
            FolderIcon::Desktop => "desktop",
            FolderIcon::Home => "home",
            FolderIcon::Drive => "drive",
            FolderIcon::NetworkDrive => "network-drive",
        }
    }

    /// The glyph called `name` ([`FolderIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<FolderIcon> {
        FolderIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// The colour a state glyph is painted in: `color` itself when the folder sets the state, at
/// [`INHERITED_OPACITY`] when it inherits it.
pub fn state_color(color: Color32, inherited: bool) -> Color32 {
    if inherited { color.gamma_multiply(INHERITED_OPACITY) } else { color }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_folder(painter: &Painter, rect: Rect, icon: FolderIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

/// A filled arrowhead at the end of an arc round (`cx`, `cy`) of radius `r` at angle `a` (degrees),
/// pointing the way the angle grows (clockwise on screen).
fn arrowhead(p: &Pen, cx: f32, cy: f32, r: f32, a: f32) {
    let (s, c) = a.to_radians().sin_cos();
    let (x, y) = (cx + r * c, cy + r * s);
    // Along the arc (t) and across it (n).
    let (tx, ty) = (-s, c);
    let (nx, ny) = (c, s);
    let (len, half) = (2.6, 2.4);
    p.fill(&[(x + tx * len, y + ty * len), (x + nx * half, y + ny * half), (x - nx * half, y - ny * half)]);
}

fn draw(p: &Pen, icon: FolderIcon) {
    match icon {
        FolderIcon::ScanAlways => {
            // Two arcs of one circle, each ending in an arrowhead, chasing each other clockwise.
            let (cx, cy, r) = (10.0, 10.0, 6.0);
            for start in [200.0, 20.0] {
                let end = start + 125.0;
                p.line(&arc_points(cx, cy, r, start, end));
                arrowhead(p, cx, cy, r, end);
            }
        }
        FolderIcon::ScanOnce => p.line(&[(4.5, 10.5), (8.3, 14.3), (15.5, 5.7)]),
        FolderIcon::Remove => {
            p.seg((5.0, 5.0), (15.0, 15.0));
            p.seg((15.0, 5.0), (5.0, 15.0));
        }
        FolderIcon::Pictures => {
            // A landscape picture in its frame: a sun and one hill.
            p.rect(2.5, 4.0, 17.5, 16.0, 1.5);
            p.dot(6.6, 7.8, 1.4);
            p.fill(&[(4.6, 14.0), (10.6, 8.6), (15.4, 14.0)]);
        }
        FolderIcon::Documents => {
            p.closed(&[(4.5, 2.5), (11.5, 2.5), (15.5, 6.5), (15.5, 17.5), (4.5, 17.5)]);
            p.line(&[(11.5, 2.5), (11.5, 6.5), (15.5, 6.5)]);
            for y in [10.0, 12.5, 15.0] {
                p.seg((7.0, y), (13.0, y));
            }
        }
        FolderIcon::Desktop => {
            p.rect(2.5, 3.5, 17.5, 13.5, 1.5);
            p.seg((10.0, 13.5), (10.0, 16.5));
            p.seg((6.5, 16.5), (13.5, 16.5));
        }
        FolderIcon::Home => {
            p.line(&[(2.5, 10.0), (10.0, 3.0), (17.5, 10.0)]);
            p.elbow(&[(4.5, 8.5), (4.5, 17.0), (15.5, 17.0), (15.5, 8.5)]);
            p.elbow(&[(8.5, 17.0), (8.5, 12.0), (11.5, 12.0), (11.5, 17.0)]);
        }
        FolderIcon::Drive => {
            p.rect(2.5, 5.5, 17.5, 14.5, 2.0);
            p.seg((5.5, 10.0), (10.0, 10.0));
            p.dot(14.3, 10.0, 1.1);
        }
        FolderIcon::NetworkDrive => {
            p.rect(3.5, 2.5, 16.5, 9.5, 1.5);
            p.dot(13.3, 6.0, 0.9);
            p.seg((10.0, 9.5), (10.0, 15.0));
            p.seg((3.0, 15.0), (17.0, 15.0));
            p.dot(10.0, 15.0, 1.8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in FolderIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        assert_eq!(FolderIcon::NetworkDrive as usize + 1, FolderIcon::ALL.len());
        for icon in FolderIcon::ALL {
            assert_eq!(FolderIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(FolderIcon::from_name("folder"), None);
    }

    #[test]
    fn inherited_states_are_half_as_opaque() {
        let c = Color32::from_rgb(1, 101, 221);
        assert_eq!(state_color(c, false), c);
        let half = state_color(c, true);
        assert_eq!(half.a(), 128);
        assert_eq!(half, c.gamma_multiply(0.5));
    }
}
