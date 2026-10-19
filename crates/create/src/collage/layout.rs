//! The layouts: a pure function of the document and the pictures' aspect ratios to placements
//! normalised to the page, so the canvas, the render, a thumbnail of the room and the MCP reply
//! agree at any size and on every system.
//!
//! [`layout`] puts the pictures on the page in slot order ([`Collage::assignment`]), asks the
//! style's module (one per style, under `layout/`) for one [`Slot`] per picture, then applies the
//! user's placements: a pile's moves (centre, scale, angle) and everyone's layer (Bring to Top,
//! Send to Bottom). Until M14.8 and M14.9 every style answers [`simple_grid`].

mod exposure;
mod frame;
mod grid;
mod mosaic;
mod pile;
mod sheet;

use dalica_core::{PhotoId, Rect01};

use super::doc::{Collage, Settings, Style};

/// The narrowest and widest aspect ratio a picture is laid out with (anything else, or an
/// unknown one, is taken as square).
pub const MAX_ASPECT: f64 = 1_000.0;

/// Where one picture goes, normalised to the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The picture's position in the Clips.
    pub clip: usize,
    pub id: PhotoId,
    /// The centre of the picture's box (without its border) as a fraction of the page's width
    /// and height.
    pub centre: [f64; 2],
    /// The box's width and height as fractions of the page's width and height.
    pub size: [f64; 2],
    /// Degrees, clockwise, about the centre.
    pub angle: f64,
    /// Stacking order: 0 at the bottom, drawn first.
    pub z: usize,
    /// The part of the picture shown in the box (the whole picture, or the part cropped to fill
    /// it), normalised to the picture.
    pub shown: Rect01,
}

/// What a style answers for one picture: a [`Placement`] without who and where in the stack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub centre: [f64; 2],
    pub size: [f64; 2],
    pub angle: f64,
    pub shown: Rect01,
}

/// What a style lays out.
#[derive(Clone, Copy, Debug)]
pub struct Ctx<'a> {
    /// The page's width over its height.
    pub page_ratio: f64,
    /// The pictures' aspect ratios (width over height) in slot order.
    pub aspects: &'a [f64],
    /// The arrangement's seed.
    pub seed: u64,
    pub settings: &'a Settings,
    /// Frame Mosaic's centre picture, as a position in `aspects`.
    pub frame_centre: Option<usize>,
}

/// The placements of the pictures on the page, in stacking order (bottom first). `aspects` gives
/// each clip's aspect ratio by its position in the Clips (missing, non-finite or extreme ones are
/// taken as square).
pub fn layout(doc: &Collage, aspects: &[f64]) -> Vec<Placement> {
    let order = doc.assignment();
    let ratios: Vec<f64> = order.iter().map(|&i| sane_aspect(aspects.get(i).copied())).collect();
    let frame_centre = doc.frame_centre.and_then(|id| order.iter().position(|&i| doc.clips.get(i).is_some_and(|c| c.id == id)));
    let ctx = Ctx { page_ratio: sane_aspect(Some(doc.page.ratio())), aspects: &ratios, seed: doc.seed, settings: &doc.settings, frame_centre };
    let slots = match doc.style {
        Style::Pile => pile::layout(&ctx),
        Style::Mosaic => mosaic::layout(&ctx),
        Style::FrameMosaic => frame::layout(&ctx),
        Style::Grid => grid::layout(&ctx),
        Style::ContactSheet => sheet::layout(&ctx),
        Style::MultipleExposure => exposure::layout(&ctx),
    };
    let mut placed: Vec<(i64, Placement)> = Vec::with_capacity(order.len());
    for (z, (&clip, slot)) in order.iter().zip(slots).enumerate() {
        let Some(c) = doc.clips.get(clip) else { continue };
        let mut p = Placement { clip, id: c.id, centre: slot.centre, size: slot.size, angle: slot.angle, z, shown: slot.shown };
        let mut layer = 0;
        if let Some(m) = doc.move_of(c.id) {
            layer = m.layer;
            if doc.style.is_free() {
                p.centre = m.centre.unwrap_or(p.centre);
                p.size = m.scale.map_or(p.size, |s| p.size.map(|v| v * s));
                p.angle = m.angle.unwrap_or(p.angle);
            }
        }
        placed.push((layer, sane(p)));
    }
    // Stable: equal layers keep the slot order.
    placed.sort_by_key(|(layer, _)| *layer);
    placed.into_iter().enumerate().map(|(z, (_, p))| Placement { z, ..p }).collect()
}

/// An aspect ratio a layout can use: finite and within [`MAX_ASPECT`] either way, else 1.
pub fn sane_aspect(aspect: Option<f64>) -> f64 {
    match aspect {
        Some(a) if a.is_finite() && a > 0.0 => a.clamp(1.0 / MAX_ASPECT, MAX_ASPECT),
        _ => 1.0,
    }
}

/// Never a non-finite or negative number out of a layout: such a value becomes the page's centre,
/// a zero size or no turn.
fn sane(mut p: Placement) -> Placement {
    let fin = |v: f64, or: f64| if v.is_finite() { v } else { or };
    p.centre = p.centre.map(|v| fin(v, 0.5));
    p.size = p.size.map(|v| fin(v, 0.0).max(0.0));
    p.angle = fin(p.angle, 0.0);
    p
}

/// The part of a picture of `aspect` that fills a box of `cell` (both width over height): the
/// largest centred rectangle of the box's shape.
pub fn fill(aspect: f64, cell: f64) -> Rect01 {
    let (aspect, cell) = (sane_aspect(Some(aspect)), sane_aspect(Some(cell)));
    let (w, h) = if aspect > cell { (cell / aspect, 1.0) } else { (1.0, aspect / cell) };
    let (x0, y0) = ((1.0 - w) / 2.0, (1.0 - h) / 2.0);
    Rect01::new(x0, y0, x0 + w, y0 + h).unwrap_or(Rect01::FULL)
}

/// Rows and columns of equal cells, the pictures in reading order, each filling its cell, with
/// the grid spacing between the cells and round the edge: the stand-in every style answers until
/// its own layout lands.
pub fn simple_grid(ctx: &Ctx) -> Vec<Slot> {
    let n = ctx.aspects.len();
    if n == 0 {
        return Vec::new();
    }
    let r = ctx.page_ratio;
    // Page units: the height is 1 and the width `r`. Columns so the cells come out near square.
    let cols = ((n as f64 * r).sqrt().ceil() as usize).clamp(1, n);
    let rows = n.div_ceil(cols);
    let mut gap = ctx.settings.spacing.clamp(0.0, 1.0) * r.min(1.0);
    let cell = |gap: f64| ((r - gap * (cols as f64 + 1.0)) / cols as f64, (1.0 - gap * (rows as f64 + 1.0)) / rows as f64);
    let (mut cw, mut ch) = cell(gap);
    if cw <= 0.0 || ch <= 0.0 {
        gap = 0.0;
        (cw, ch) = cell(0.0);
    }
    ctx.aspects
        .iter()
        .enumerate()
        .map(|(i, &aspect)| {
            let (row, col) = ((i / cols) as f64, (i % cols) as f64);
            let x = gap + cw / 2.0 + col * (cw + gap);
            let y = gap + ch / 2.0 + row * (ch + gap);
            Slot { centre: [x / r, y], size: [cw / r, ch], angle: 0.0, shown: fill(aspect, cw / ch) }
        })
        .collect()
}
