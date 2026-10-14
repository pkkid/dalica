//! What a drag of photos looks like (`ui-layout.md` §4 and §5.5, `organizing.md` §4.3): the drag
//! pill that follows the pointer, the `drop_target` highlight of the row or tray under it, and the
//! insertion marker between two cells while an album is reordered. Painters only: the UI decides
//! what the drag does and calls them each frame.

use egui::{Context, CornerRadius, Id, LayerId, Order, Painter, Pos2, Rect, Stroke, StrokeKind, TextureId, Vec2, pos2, vec2};

use super::{Target, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// The pill's height, its mini thumbnail's box and the room round them.
pub const PILL_H: f32 = 32.0;
pub const PILL_THUMB: f32 = 24.0;
const PILL_PAD: f32 = 4.0;
const PILL_TEXT_GAP: f32 = 8.0;
const PILL_RIGHT: f32 = 12.0;
/// The widest the pill grows; a longer text is cut from its end.
pub const PILL_MAX_W: f32 = 360.0;
/// Where the pill sits from the pointer (right and below it, unless that leaves the window).
pub const PILL_OFFSET: Vec2 = vec2(16.0, 16.0);
/// The least room between the pill and the window's edges.
pub const PILL_MARGIN: f32 = 4.0;
/// The insertion marker's bar width and the length of its end caps.
pub const MARKER_W: f32 = 2.0;
pub const MARKER_CAP: f32 = 8.0;

/// What dropping here would do (the pill's text says it; `ui-layout.md` §5.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOp {
    /// Nothing under the pointer takes the photos.
    None,
    /// Onto a folder: the files move there.
    Move,
    /// Onto a folder with Ctrl: the files are copied there (the pill shows a +).
    Copy,
    /// Onto an album: the photos join it.
    AddToAlbum,
    /// Onto the tray: the photos are held there.
    Hold,
    /// Between two cells of an album: the photos move to that place in its order.
    Reorder,
}

/// The pill's text: the operation and the count, and the target's name where one is given
/// ("Move 3 photos to Garden", "Copy 1 photo", "Add 3 photos to album", "Hold 3 photos in the
/// tray", "Reorder 3 photos"; with nothing to drop on only "3 photos"). Counts are grouped by
/// thousands ("1,204 photos").
pub fn drag_pill_text(op: DragOp, count: usize, target: Option<&str>) -> String {
    let photos = format!("{} {}", grouped(count), if count == 1 { "photo" } else { "photos" });
    let to = |default: Option<&str>| match target.filter(|s| !s.is_empty()).or(default) {
        Some(name) => format!(" to {name}"),
        None => String::new(),
    };
    match op {
        DragOp::None => photos,
        DragOp::Move => format!("Move {photos}{}", to(None)),
        DragOp::Copy => format!("Copy {photos}{}", to(None)),
        DragOp::AddToAlbum => format!("Add {photos}{}", to(Some("album"))),
        DragOp::Hold => format!("Hold {photos} in the tray"),
        DragOp::Reorder => format!("Reorder {photos}"),
    }
}

/// `n` with its thousands grouped by commas.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Where a pill of `size` goes for a pointer at `pointer` in `window`: [`PILL_OFFSET`] right of and
/// below the pointer; on its left where the right edge would pass the window's, above it where the
/// bottom would; and always inside `window` less [`PILL_MARGIN`] (shrunk to fit a window smaller
/// than the pill). A pointer that is not a finite point counts as the window's centre; a window
/// that is empty or not finite gives [`Rect::NOTHING`].
pub fn drag_pill_rect(pointer: Pos2, size: Vec2, window: Rect) -> Rect {
    if !window.is_finite() || !window.is_positive() {
        return Rect::NOTHING;
    }
    let room = window.shrink(PILL_MARGIN);
    let room = if room.is_positive() { room } else { Rect::from_center_size(window.center(), Vec2::ZERO) };
    let fit = |v: f32, max: f32| if v.is_finite() { v.clamp(0.0, max) } else { 0.0 };
    let size = vec2(fit(size.x, room.width()), fit(size.y, room.height()));
    let pointer = if pointer.is_finite() { pointer } else { window.center() };
    let place = |p: f32, off: f32, len: f32, lo: f32, hi: f32| {
        let after = p + off;
        let at = if after + len > hi { p - off - len } else { after };
        at.clamp(lo, (hi - len).max(lo))
    };
    let x = place(pointer.x, PILL_OFFSET.x, size.x, room.min.x, room.max.x);
    let y = place(pointer.y, PILL_OFFSET.y, size.y, room.min.y, room.max.y);
    Rect::from_min_size(pos2(x, y), size)
}

/// The first photo's mini thumbnail: its texture and its pixel size (for the letterbox).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillThumb {
    pub texture: TextureId,
    pub size: Vec2,
}

/// One frame of a drag: what it would do, how many photos it carries, the target's name and the
/// first photo's thumbnail (the `placeholder` square while it has none).
#[derive(Debug, Clone, Copy)]
pub struct DragPill<'a> {
    pub op: DragOp,
    pub count: usize,
    pub target: Option<&'a str>,
    pub thumb: Option<PillThumb>,
}

/// The drag pill (`design-system.md` §6), painted above everything near `pointer` and never
/// outside the window ([`drag_pill_rect`]): the popup frame (`chrome`, 1 pt `button_border`, radius
/// 6, the popup shadow), [`PILL_H`] high; the first photo's thumbnail letterboxed in a
/// [`PILL_THUMB`] square at its left (corners square, as every photo's), and on it, for a copy, an
/// `on_accent` plus on an `accent` disc; then [`drag_pill_text`] in 13 pt `text`, cut from its end with "…" past
/// [`PILL_MAX_W`]. Registers `id` (role `other`, painted, labelled with the text). Returns the
/// pill's rect ([`Rect::NOTHING`] when nothing was painted).
pub fn drag_pill(ctx: &Context, t: &Tokens, id: &str, pointer: Pos2, pill: &DragPill<'_>) -> Rect {
    let window = ctx.content_rect();
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new(id)));
    let text = drag_pill_text(pill.op, pill.count, pill.target);
    let lead = PILL_PAD + PILL_THUMB + PILL_TEXT_GAP;
    let natural = painter.layout_no_wrap(text.clone(), fonts::regular(t.body), t.text).size().x;
    let width = (lead + natural.ceil() + PILL_RIGHT).min(PILL_MAX_W);
    let rect = drag_pill_rect(pointer, vec2(width, PILL_H), window);
    if !rect.is_positive() {
        return Rect::NOTHING;
    }
    let radius = CornerRadius::same(t.toast_radius);
    painter.add(t.popup_shadow.as_shape(rect, radius));
    painter.rect(rect, radius, t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let thumb_box = Rect::from_min_size(pos2(rect.min.x + PILL_PAD, rect.center().y - PILL_THUMB / 2.0), Vec2::splat(PILL_THUMB));
    paint_thumb(&painter, t, thumb_box.intersect(rect), pill.thumb);
    if pill.op == DragOp::Copy {
        // On the thumbnail's bottom-right corner, kept clear of the pill's edge.
        plus_disc(&painter, t, thumb_box.max - vec2(1.0, 5.0));
    }
    let max = rect.max.x - PILL_RIGHT - (rect.min.x + lead);
    if max >= 1.0 {
        let galley = one_line(&painter, &text, fonts::regular(t.body), t.text, max);
        let pos = pos2(rect.min.x + lead, rect.center().y - galley.size().y / 2.0).round();
        painter.galley(pos, galley, t.text);
    }
    register(ctx, id, Role::Other, &text, Target::Painted { rect, clip: window }, true);
    rect
}

/// The photo letterboxed in `cell` (never cropped, never rounded), or the `placeholder` square.
fn paint_thumb(painter: &Painter, t: &Tokens, cell: Rect, thumb: Option<PillThumb>) {
    if !cell.is_positive() {
        return;
    }
    let fitted = thumb.and_then(|th| {
        let (w, h) = (th.size.x, th.size.y);
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return None;
        }
        let scale = (cell.width() / w).min(cell.height() / h);
        Some((th.texture, Rect::from_center_size(cell.center(), vec2(w * scale, h * scale))))
    });
    match fitted {
        Some((texture, image)) => {
            painter.image(texture, image, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            painter.rect_filled(cell, 0, t.placeholder);
        }
    }
}

/// The copy mark: an `accent` disc of radius 6 round `center` with an `on_accent` +.
fn plus_disc(painter: &Painter, t: &Tokens, center: Pos2) {
    painter.circle_filled(center, 6.0, t.accent);
    let stroke = Stroke::new(1.5, t.on_accent);
    painter.line_segment([center - vec2(3.0, 0.0), center + vec2(3.0, 0.0)], stroke);
    painter.line_segment([center - vec2(0.0, 3.0), center + vec2(0.0, 3.0)], stroke);
}

/// The `drop_target` highlight (`design-system.md` §2): a `drop_target_fill` plate with a 1.5 pt
/// `drop_target` outline inside `rect`, radius 4: one painter for the tree's folder and album rows
/// and the tray while a drag hovers them. An empty or non-finite rect paints nothing.
pub fn drop_target(painter: &Painter, t: &Tokens, rect: Rect) {
    if !rect.is_finite() || !rect.is_positive() {
        return;
    }
    let radius = CornerRadius::same(t.radius);
    painter.rect(rect, radius, t.drop_target_fill, Stroke::new(1.5, t.drop_target), StrokeKind::Inside);
}

/// The insertion marker while photos are dragged between two cells of an album: a [`MARKER_W`]
/// `accent` bar down from `top` for `height`, with a [`MARKER_CAP`]-long cap across each end (an
/// I-beam), centred on `top.x`. A non-finite place or a height that is not positive paints nothing.
pub fn insertion_marker(painter: &Painter, t: &Tokens, top: Pos2, height: f32) {
    if !top.is_finite() || !height.is_finite() || height <= 0.0 {
        return;
    }
    let half = MARKER_W / 2.0;
    let bar = Rect::from_min_max(pos2(top.x - half, top.y), pos2(top.x + half, top.y + height));
    painter.rect_filled(bar, 0, t.accent);
    for y in [top.y, top.y + height - MARKER_W] {
        painter.rect_filled(Rect::from_min_size(pos2(top.x - MARKER_CAP / 2.0, y), vec2(MARKER_CAP, MARKER_W)), 1, t.accent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1204), "1,204");
        assert_eq!(grouped(12_345_678), "12,345,678");
    }
}
