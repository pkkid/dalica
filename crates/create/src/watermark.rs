//! The export watermark (`creating.md` §2): white text with a soft shadow in the bottom-right
//! corner, 3 % of the long side high, with margins relative to the image so every export size
//! looks the same.
//!
//! Ported from LightCraft `crates/engine/src/export.rs` (`Watermark`, `draw_watermark`,
//! `blend_watermark`, `watermark_coverage`) @ 25ef65c8, MIT OR Apache-2.0 (`NOTICE`): text only,
//! horizontal, one face (Inter SemiBold from `assets/fonts/`, through `ab_glyph`), always
//! bottom-right. Changed from the original: the size and margins follow the long side, not the
//! short one; the text is cleaned and shrunk to fit inside the image; the shadow is blurred (the
//! original draws a sharp offset copy); glyphs are placed at their sub-pixel positions in the image;
//! the blend is "over" in straight alpha, so text on a transparent pixel shows (on an opaque pixel
//! it is the original's blend).
//!
//! The hook the output pipeline calls after resizing and before encoding ([`crate::output`]).

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use dalica_image::Rgba8;

/// Inter SemiBold (OFL 1.1, `ATTRIBUTION.md`), the face `ui-kit` also uses.
static FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");

/// The text's line height (the face's ascent to descent) as a fraction of the image's long side.
pub const SIZE: f32 = 0.03;
/// The margin from the right and bottom edges as a fraction of the long side (at most a quarter of
/// that side, so a very narrow image keeps room for the text).
pub const MARGIN: f32 = 0.025;
/// The most characters drawn; the rest of a longer text is left out.
pub const MAX_CHARS: usize = 200;
/// Text that would have to shrink below this line height (pixels) to fit is not drawn.
pub const MIN_PX: f32 = 2.0;

/// The text's opacity over the photo: it reads as white and lets a little of the photo through
/// (LightCraft's default, 0.7, reads as light grey on dark photos).
const TEXT_OPACITY: f32 = 0.85;
/// The shadow's opacity at its densest.
const SHADOW_OPACITY: f32 = 0.55;
/// How far the shadow falls down and to the right, as a fraction of the line height (at least one
/// pixel).
const SHADOW_OFFSET: f32 = 0.05;
/// The shadow's blur radius (each of two box passes), as a fraction of the line height.
const SHADOW_BLUR: f32 = 0.04;

/// Where the watermark goes on an image: its text box (pen line from ascent to descent) in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The box's left edge.
    pub x: f32,
    /// The box's top edge (the face's ascent line).
    pub y: f32,
    /// The text's advance width.
    pub width: f32,
    /// The line height (ascent to descent): [`SIZE`] × the long side unless shrunk to fit.
    pub height: f32,
    /// The right margin: the box ends this far from the right edge.
    pub margin_x: f32,
    /// The bottom margin: the box ends this far from the bottom edge.
    pub margin_y: f32,
}

/// `text` as it is drawn: control characters, bidirectional controls and other invisible format
/// characters removed (tabs, line and paragraph breaks become spaces), trimmed, and cut to
/// [`MAX_CHARS`] characters.
pub fn clean(text: &str) -> String {
    let mapped: String = text
        .chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}' => Some(' '),
            c if c.is_control() || invisible(c) => None,
            c => Some(c),
        })
        .collect();
    mapped.trim().chars().take(MAX_CHARS).collect::<String>().trim_end().to_owned()
}

/// Bidirectional controls, zero-width characters, variation selectors and tags: characters that
/// change how the text around them reads or draw nothing (a missing-glyph box in Inter).
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

fn font() -> Option<FontRef<'static>> {
    FontRef::try_from_slice(FONT).ok()
}

/// The advance width of `text` (already cleaned) at line height `px`, kerning included.
fn advance(font: &FontRef<'_>, text: &str, px: f32) -> f32 {
    let sf = font.as_scaled(PxScale::from(px));
    let mut width = 0.0f32;
    let mut prev: Option<GlyphId> = None;
    for c in text.chars() {
        let id = sf.glyph_id(c);
        if let Some(p) = prev {
            width += sf.kern(p, id);
        }
        width += sf.h_advance(id);
        prev = Some(id);
    }
    width
}

fn place(font: &FontRef<'_>, width: u32, height: u32, text: &str) -> Option<Placement> {
    if text.is_empty() || width == 0 || height == 0 {
        return None;
    }
    let (w, h) = (width as f32, height as f32);
    let long = w.max(h);
    let margin_x = (MARGIN * long).min(w / 4.0);
    let margin_y = (MARGIN * long).min(h / 4.0);
    let (room_w, room_h) = (w - 2.0 * margin_x, h - 2.0 * margin_y);
    let mut px = (SIZE * long).min(room_h);
    let mut tw = advance(font, text, px);
    if !(tw.is_finite() && tw > 0.0) {
        return None;
    }
    if tw > room_w {
        // Advances and kerning scale with the size, so this fits exactly.
        px *= room_w / tw;
        tw = advance(font, text, px).min(room_w);
    }
    if !(px.is_finite() && px >= MIN_PX) {
        return None;
    }
    Some(Placement { x: w - margin_x - tw, y: h - margin_y - px, width: tw, height: px, margin_x, margin_y })
}

/// Where the watermark of `text` goes on a `width × height` image, or `None` when nothing is
/// drawn (blank text after [`clean`], an image too small for it).
pub fn layout(width: u32, height: u32, text: &str) -> Option<Placement> {
    let font = font()?;
    place(&font, width, height, &clean(text))
}

/// Draws `text` onto `image` (8-bit sRGB, straight alpha). Empty or blank text draws nothing; text
/// that cannot be drawn (no glyphs, an image too small) is left out rather than failing the export.
pub fn draw(image: &mut Rgba8, text: &str) {
    let expected = u64::from(image.width).checked_mul(u64::from(image.height)).and_then(|n| n.checked_mul(4));
    if expected != Some(image.data.len() as u64) {
        return;
    }
    let Some(font) = font() else { return };
    let text = clean(text);
    let Some(p) = place(&font, image.width, image.height, &text) else { return };
    let Some(mask) = Mask::text(&font, &text, &p) else { return };
    let shadow = mask.shadow(p.height);
    composite(image, &mask, &shadow);
}

/// A coverage map (0–1) over a rectangle of the image whose top-left pixel is `(x0, y0)` (it may
/// reach outside the image).
struct Mask {
    x0: i64,
    y0: i64,
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Mask {
    /// The text's coverage, glyphs at their sub-pixel pen positions, with room around it for the
    /// shadow.
    fn text(font: &FontRef<'_>, text: &str, p: &Placement) -> Option<Mask> {
        let px = p.height;
        let pad = i64::try_from(shadow_offset(px) + 2 * shadow_radius(px) + 2).ok()?;
        let x0 = (p.x.floor() as i64).checked_sub(pad)?;
        let y0 = (p.y.floor() as i64).checked_sub(pad)?;
        let width = usize::try_from((p.width.ceil() as i64).checked_add(2 * pad + 2)?).ok()?;
        let height = usize::try_from((px.ceil() as i64).checked_add(2 * pad + 2)?).ok()?;
        let mut mask = Mask { x0, y0, width, height, data: vec![0.0; width.checked_mul(height)?] };

        let sf = font.as_scaled(PxScale::from(px));
        let baseline = p.y + sf.ascent();
        let mut pen = p.x;
        let mut prev: Option<GlyphId> = None;
        for c in text.chars() {
            let id = sf.glyph_id(c);
            if let Some(q) = prev {
                pen += sf.kern(q, id);
            }
            let glyph = id.with_scale_and_position(px, point(pen, baseline));
            pen += sf.h_advance(id);
            prev = Some(id);
            // A character the face lacks draws its missing-glyph box (glyph 0).
            let Some(outline) = font.outline_glyph(glyph) else { continue };
            let bounds = outline.px_bounds();
            let (bx, by) = (bounds.min.x as i64 - mask.x0, bounds.min.y as i64 - mask.y0);
            outline.draw(|gx, gy, cov| mask.add(bx + i64::from(gx), by + i64::from(gy), cov));
        }
        Some(mask)
    }

    fn add(&mut self, x: i64, y: i64, cov: f32) {
        if let Some(v) = self.index(x, y).and_then(|i| self.data.get_mut(i)) {
            *v = (*v + cov).clamp(0.0, 1.0);
        }
    }

    fn index(&self, x: i64, y: i64) -> Option<usize> {
        let (x, y) = (usize::try_from(x).ok()?, usize::try_from(y).ok()?);
        (x < self.width && y < self.height).then(|| y * self.width + x)
    }

    fn at(&self, x: i64, y: i64) -> f32 {
        self.index(x, y).and_then(|i| self.data.get(i)).copied().unwrap_or(0.0)
    }

    /// The shadow: this coverage blurred (two box passes each way, close to a Gaussian) and moved
    /// down and to the right.
    fn shadow(&self, px: f32) -> Mask {
        let radius = shadow_radius(px);
        let offset = i64::try_from(shadow_offset(px)).unwrap_or(1);
        let mut data = self.data.clone();
        for _ in 0..2 {
            blur_rows(&mut data, self.width, radius);
            blur_columns(&mut data, self.width, self.height, radius);
        }
        Mask { x0: self.x0 + offset, y0: self.y0 + offset, width: self.width, height: self.height, data }
    }
}

fn shadow_offset(px: f32) -> usize {
    (px * SHADOW_OFFSET).round().max(1.0) as usize
}

fn shadow_radius(px: f32) -> usize {
    (px * SHADOW_BLUR).round().max(1.0) as usize
}

/// A box blur of radius `r` along each row (outside the mask counts as empty).
fn blur_rows(data: &mut [f32], width: usize, r: usize) {
    let mut line = Vec::with_capacity(width);
    for row in data.chunks_mut(width.max(1)) {
        line.clear();
        line.extend_from_slice(row);
        box_line(&line, r, |i, v| {
            if let Some(slot) = row.get_mut(i) {
                *slot = v;
            }
        });
    }
}

/// A box blur of radius `r` down each column (outside the mask counts as empty).
fn blur_columns(data: &mut [f32], width: usize, height: usize, r: usize) {
    let mut line = Vec::with_capacity(height);
    for x in 0..width {
        line.clear();
        line.extend((0..height).map(|y| data.get(y * width + x).copied().unwrap_or(0.0)));
        box_line(&line, r, |y, v| {
            if let Some(slot) = data.get_mut(y * width + x) {
                *slot = v;
            }
        });
    }
}

/// The running mean of `src` over `2r + 1` samples, written through `out(index, value)`.
fn box_line(src: &[f32], r: usize, mut out: impl FnMut(usize, f32)) {
    let n = (2 * r + 1) as f32;
    let get = |i: isize| usize::try_from(i).ok().and_then(|i| src.get(i)).copied().unwrap_or(0.0);
    let ri = isize::try_from(r).unwrap_or(1);
    let mut sum: f32 = (-ri..=ri).map(get).sum();
    for (i, _) in src.iter().enumerate() {
        out(i, (sum / n).clamp(0.0, 1.0));
        let i = isize::try_from(i).unwrap_or(isize::MAX - ri - 1);
        sum += get(i + ri + 1) - get(i - ri);
    }
}

/// The shadow (black) and then the text (white) over `image`, inside the image only.
fn composite(image: &mut Rgba8, text: &Mask, shadow: &Mask) {
    let (w, h) = (i64::from(image.width), i64::from(image.height));
    let x_start = text.x0.min(shadow.x0).max(0);
    let y_start = text.y0.min(shadow.y0).max(0);
    let x_end = (text.x0 + text.width as i64).max(shadow.x0 + shadow.width as i64).min(w);
    let y_end = (text.y0 + text.height as i64).max(shadow.y0 + shadow.height as i64).min(h);
    for y in y_start..y_end {
        for x in x_start..x_end {
            let s = shadow.at(x - shadow.x0, y - shadow.y0) * SHADOW_OPACITY;
            let t = text.at(x - text.x0, y - text.y0) * TEXT_OPACITY;
            if s <= 0.0 && t <= 0.0 {
                continue;
            }
            let Ok(i) = usize::try_from((y * w + x) * 4) else { continue };
            if let Some(px) = image.data.get_mut(i..i + 4) {
                over(px, 0.0, s);
                over(px, 255.0, t);
            }
        }
    }
}

/// Grey level `value` at coverage `k` over the straight-alpha pixel `px` (RGBA).
fn over(px: &mut [u8], value: f32, k: f32) {
    let k = k.clamp(0.0, 1.0);
    let Some(&alpha) = px.get(3) else { return };
    let a = f32::from(alpha) / 255.0;
    let out_a = k + a * (1.0 - k);
    if k <= 0.0 || out_a <= 0.0 {
        return;
    }
    for c in px.iter_mut().take(3) {
        let v = (value * k + f32::from(*c) * a * (1.0 - k)) / out_a;
        *c = v.round().clamp(0.0, 255.0) as u8;
    }
    if let Some(slot) = px.get_mut(3) {
        *slot = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}
