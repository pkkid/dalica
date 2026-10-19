//! A line of text on the collage (a pile's caption, a contact sheet's file name), drawn with
//! [`crate::watermark`]'s Inter text: the watermark is drawn onto a small transparent strip sized so
//! its line comes out at the height asked for, and the strip is then sampled in the picture's frame
//! like the picture itself, so a turned picture's caption turns with it.

use dalica_image::Rgba8;

use super::paint::{Blend, Frame, Local, Sampler, paint};
use crate::watermark;

/// The smallest line height (pixels) drawn; smaller text is left out.
const MIN_PX: f64 = 4.0;
/// The widest strip, in pixels.
const MAX_STRIP: f64 = 16_384.0;

/// How the text shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The watermark's own look: white with a soft dark shadow (on a picture or a dark page).
    Light,
    /// Solid text of this colour, no shadow (on a white border or a light page).
    Dark([u8; 3]),
}

/// A line of text ready to paint: its strip and where the strip lies in the frame.
pub struct Label {
    strip: Rgba8,
    rect: Local,
    ink: Ink,
}

impl Label {
    /// `text` (cleaned as the watermark cleans it) at a line height of about `px`, no wider than
    /// `max_width`, centred on the local point `centre`. `None` when there is nothing to draw.
    pub fn new(text: &str, px: f64, max_width: f64, centre: (f64, f64), ink: Ink) -> Option<Label> {
        let text = watermark::clean(text);
        if text.is_empty() || !(px.is_finite() && max_width.is_finite()) || px < MIN_PX || max_width < MIN_PX {
            return None;
        }
        // Text wider than `max_width` at that height comes out shrunk to fit.
        let (w, h, p) = strip_for(&text, px, max_width)?;
        if f64::from(p.height) < MIN_PX / 2.0 {
            return None;
        }
        let mut strip = Rgba8 { width: w, height: h, data: Vec::new() };
        let n = usize::try_from(u64::from(w) * u64::from(h) * 4).ok()?;
        strip.data.try_reserve_exact(n).ok()?;
        strip.data.resize(n, 0);
        watermark::draw(&mut strip, &text);
        let (tx, ty) = (f64::from(p.x) + f64::from(p.width) / 2.0, f64::from(p.y) + f64::from(p.height) / 2.0);
        let (u0, v0) = (centre.0 - tx, centre.1 - ty);
        Some(Label { strip, rect: Local { u0, v0, u1: u0 + f64::from(w), v1: v0 + f64::from(h) }, ink })
    }

    /// Paints the text in `frame` onto `page`.
    pub fn paint(&self, page: &mut Rgba8, frame: Frame) {
        let Some(sampler) = Sampler::new(&self.strip, [0.0, 0.0, 1.0, 1.0], self.rect) else { return };
        paint(page, frame, self.rect, false, Blend::Over, |u, v| {
            let c = sampler.at(u, v);
            match self.ink {
                Ink::Light => c,
                Ink::Dark([r, g, b]) => {
                    // The watermark's white text over its black shadow: premultiplied red is the
                    // text's own coverage.
                    let t = (c[0] / 255.0).clamp(0.0, 1.0);
                    [f32::from(r) * t, f32::from(g) * t, f32::from(b) * t, 255.0 * t]
                }
            }
        });
    }
}

/// The strip for `text` at line height `px` (its width, height and the watermark's placement on
/// it). The watermark's line is the
/// lesser of 3 % of the strip's long side and half its height (its margins are a quarter of the
/// height once the strip is at least ten times as wide as tall), so a strip `2 px` high and at
/// least `34 px` wide draws the line at `px`; it is made wide enough for `max_width` of text, and the watermark shrinks wider text to that.
fn strip_for(text: &str, mut px: f64, max_width: f64) -> Option<(u32, u32, watermark::Placement)> {
    // A strip 34 px wide may hold more than `max_width` of text: shrink the line by the overflow
    // and lay it out again (the advance scales with the size, so once is nearly always enough).
    for _ in 0..4 {
        let h = (2.0 * px).ceil();
        let w = (34.0 * px).max(max_width / 0.95 + 2.0).ceil();
        if !(h.is_finite() && w.is_finite()) || h > MAX_STRIP || w > MAX_STRIP || px < MIN_PX / 2.0 {
            return None;
        }
        let (w, h) = (w as u32, h as u32);
        let p = watermark::layout(w, h, text)?;
        if f64::from(p.width) <= max_width + 0.5 {
            return Some((w, h, p));
        }
        px *= max_width / f64::from(p.width) * 0.99;
    }
    None
}
