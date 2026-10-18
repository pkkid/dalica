//! The glyphs of the effect tabs (`editing.md` §8, `design-system.md` §5): our own drawings on the
//! 20 × 20 grid with the shell icons' pen.
//!
//! They are a set of their own, [`EffectIcon`] ([`paint_effect`]), so the other sets' sheets and
//! fingerprints stay as they are. Content stays inside 2 … 18 so the stroke never leaves the
//! square. Each is drawn apart from the glyph it could be taken for: the Brush has bristles where
//! the edit view's Eyedropper has a bulb and Retouch's tool, the Crosshair's circle is crossed by
//! ticks where the Clock has hands, and the Eraser is a tilted block, not a bin.

use egui::{Color32, Painter, Rect};

use super::pen::Pen;

/// The effect glyphs. Keep [`EffectIcon::ALL`] in declaration order (a test checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectIcon {
    /// A paint brush, its bristles at the bottom left: Soften's brush.
    Brush,
    /// A tilted block, its lower end solid, on a line: Soften's eraser.
    Eraser,
    /// A ring crossed by four ticks round a dot: the crosshair of the focal effects.
    Crosshair,
    /// A pointer clicking (an arrow with three short rays at its tip): the "1-click" badge.
    OneClick,
}

impl EffectIcon {
    /// Every glyph, in declaration order.
    pub const ALL: [EffectIcon; 4] = [EffectIcon::Brush, EffectIcon::Eraser, EffectIcon::Crosshair, EffectIcon::OneClick];

    /// The glyph's stable kebab-case name (icon sheets, inspection output).
    pub fn name(self) -> &'static str {
        match self {
            EffectIcon::Brush => "brush",
            EffectIcon::Eraser => "eraser",
            EffectIcon::Crosshair => "crosshair",
            EffectIcon::OneClick => "one-click",
        }
    }

    /// The glyph called `name` ([`EffectIcon::name`]), if any.
    pub fn from_name(name: &str) -> Option<EffectIcon> {
        EffectIcon::ALL.into_iter().find(|i| i.name() == name)
    }
}

/// Paints `icon` in `color`, centred in the largest square that fits `rect`. An empty or
/// non-finite rect paints nothing.
pub fn paint_effect(painter: &Painter, rect: Rect, icon: EffectIcon, color: Color32) {
    let Some(rect) = super::square(rect) else { return };
    draw(&Pen::new(painter, rect, color), icon);
}

/// The grid point `s` along and `t` across a diagonal that starts at (`x0`, `y0`) and runs down to
/// the left (`t` positive towards the bottom right).
fn down_left(x0: f32, y0: f32, s: f32, t: f32) -> (f32, f32) {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    (x0 - s * k + t * k, y0 + s * k + t * k)
}

/// The grid point `s` along and `t` across a diagonal that starts at (`x0`, `y0`) and runs up to
/// the right (`t` positive towards the bottom right).
fn up_right(x0: f32, y0: f32, s: f32, t: f32) -> (f32, f32) {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    (x0 + s * k + t * k, y0 - s * k + t * k)
}

fn draw(p: &Pen, icon: EffectIcon) {
    match icon {
        EffectIcon::Brush => {
            // The handle from the top right, the ferrule, then the bristles tapering to a tip.
            let at = |s: f32, t: f32| down_left(16.2, 3.8, s, t);
            p.line(&[at(0.0, 0.0), at(6.6, 0.0)]);
            p.fill(&[at(6.6, -1.5), at(8.6, -1.5), at(8.6, 1.5), at(6.6, 1.5)]);
            p.fill(&[at(9.4, -1.9), at(12.0, -2.2), at(14.8, -1.4), at(16.8, 0.2), at(14.8, 1.6), at(12.0, 2.2), at(9.4, 1.9)]);
        }
        EffectIcon::Eraser => {
            // A block tilted up to the right, its lower third solid (the rubber), on a line.
            let at = |s: f32, t: f32| up_right(3.4, 12.6, s, t);
            let (len, wide) = (12.0, 5.8);
            p.closed(&[at(0.0, 0.0), at(len, 0.0), at(len, wide), at(0.0, wide)]);
            p.fill(&[at(0.0, 0.0), at(4.4, 0.0), at(4.4, wide), at(0.0, wide)]);
            p.seg((10.0, 17.2), (17.0, 17.2));
        }
        EffectIcon::Crosshair => {
            p.circle(10.0, 10.0, 5.0);
            p.seg((10.0, 2.5), (10.0, 6.5));
            p.seg((10.0, 13.5), (10.0, 17.5));
            p.seg((2.5, 10.0), (6.5, 10.0));
            p.seg((13.5, 10.0), (17.5, 10.0));
            p.dot(10.0, 10.0, 1.2);
        }
        EffectIcon::OneClick => {
            // A pointer's head and tail, its tip at (8, 8), and three rays out from the tip.
            p.fill(&[(8.0, 8.0), (14.8, 14.2), (8.0, 16.6)]);
            p.seg((11.0, 14.6), (13.0, 17.2));
            p.seg((8.0, 5.8), (8.0, 3.0));
            p.seg((5.8, 8.0), (3.0, 8.0));
            p.seg((6.2, 6.2), (4.3, 4.3));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_glyph_once_in_declaration_order() {
        for (i, icon) in EffectIcon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
        // `OneClick` is the last variant: a variant added after it without an `ALL` entry fails here.
        assert_eq!(EffectIcon::OneClick as usize + 1, EffectIcon::ALL.len());
        for icon in EffectIcon::ALL {
            assert_eq!(EffectIcon::from_name(icon.name()), Some(icon));
        }
        assert_eq!(EffectIcon::from_name("pencil"), None);
    }
}
