//! A face tile (`organizing.md` §6.2–§6.4): one face cropped from its photo, as the Unnamed grid,
//! a person's Zoom to Faces cells and the People pane show it.
//!
//! The square picture (or the `placeholder` with a person until the crop arrives) on a surround
//! [`FACE_TILE_PAD`] wide, then, when asked, a label row (the person's name, or a suggested name as
//! "Name?") and a row of ✓ / × marks ([`suggest_marks`](super::suggest_marks)); an optional ×
//! at the picture's top-right ignores the face. The tile reports clicks and drags; selecting,
//! ignoring, confirming and the faces dragged are the caller's.

use egui::{Align2, Color32, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, TextureId, Ui, Vec2, WidgetType, pos2, vec2};

use super::marks::cross;
use super::suggest_marks::{SUGGEST_MARK, marks_at};
use super::{Target, focus_ring, one_line, register, tooltip};
use crate::Tokens;
use crate::fonts;
use crate::icons::{PeopleIcon, paint_people};
use crate::registry::Role;

/// The surround round the picture, each side (points).
pub const FACE_TILE_PAD: f32 = 4.0;
/// The label row under the picture (points).
pub const FACE_LABEL_H: f32 = 18.0;
/// The marks row under the label (points): the 20 pt buttons with 2 pt above and below.
pub const FACE_MARKS_H: f32 = SUGGEST_MARK + 4.0;
/// The side of the ignore × at the picture's top-right (points).
pub const FACE_IGNORE: f32 = 16.0;
/// The ignore ×'s distance from the picture's edges (points).
pub const FACE_IGNORE_INSET: f32 = 2.0;
/// The smallest and largest picture a tile shows, and the one it shows for a side that is no
/// number (points).
pub const FACE_TILE_MIN: f32 = 20.0;
pub const FACE_TILE_MAX: f32 = 320.0;
pub const FACE_TILE_DEFAULT: f32 = 96.0;
/// The picture's opacity while its face is dragged away.
pub const FACE_DRAG_OPACITY: f32 = 0.5;

/// The face picture: a texture and the part of it the face fills (`uv`, 0 … 1), so a caller can
/// hand over a crop or a whole thumbnail with the face's rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FacePicture {
    pub texture: TextureId,
    pub uv: Rect,
}

/// What the label row says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceLabel<'a> {
    /// No label row.
    #[default]
    None,
    /// The person's name.
    Name(&'a str),
    /// A suggested person, shown as "Name?".
    Suggested(&'a str),
}

/// One tile: what it shows and its state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceTile<'a> {
    /// The face's id in the model: the tile registers `face:<id>`.
    pub id: &'a str,
    /// The picture's side (points), kept within [`FACE_TILE_MIN`] … [`FACE_TILE_MAX`].
    pub side: f32,
    /// The face picture; `None` until it has arrived (the placeholder shows).
    pub picture: Option<FacePicture>,
    pub label: FaceLabel<'a>,
    /// In the selection: its surround `cell_selected` and a 2 pt outline round the picture.
    pub selected: bool,
    /// The selection's anchor: its outline `photo_focus` (white) instead of `photo_selected`.
    pub focused: bool,
    /// Its face is being dragged: the picture shows at [`FACE_DRAG_OPACITY`].
    pub dragged: bool,
    /// Show the ignore × at the picture's top-right.
    pub ignore: bool,
    /// Show the ✓ / × marks of a suggestion under the label.
    pub marks: bool,
}

impl<'a> FaceTile<'a> {
    /// A tile of face `id` at the default side, with nothing else shown.
    pub fn new(id: &'a str) -> FaceTile<'a> {
        FaceTile {
            id,
            side: FACE_TILE_DEFAULT,
            picture: None,
            label: FaceLabel::None,
            selected: false,
            focused: false,
            dragged: false,
            ignore: false,
            marks: false,
        }
    }
}

/// What a [`face_tile`] did this pass.
#[derive(Debug, Clone)]
pub struct FaceTileOut {
    /// The tile's response: clicks (with their modifiers), drags, hover.
    pub response: Response,
    /// The ignore × was clicked.
    pub ignore: bool,
    /// ✓ was clicked.
    pub confirm: bool,
    /// × under the label was clicked.
    pub reject: bool,
    /// Where the picture is.
    pub picture: Rect,
}

/// The automation id of the tile of face `id`: `face:<id>`.
pub fn face_tile_id(id: &str) -> String {
    format!("face:{id}")
}

/// The picture's side for `side`: within [`FACE_TILE_MIN`] … [`FACE_TILE_MAX`], [`FACE_TILE_DEFAULT`]
/// for a side that is no number.
pub fn face_side(side: f32) -> f32 {
    if side.is_finite() { side.clamp(FACE_TILE_MIN, FACE_TILE_MAX) } else { FACE_TILE_DEFAULT }
}

/// The size of a tile of picture `side` with a label row and a marks row when asked.
pub fn face_tile_size(side: f32, label: bool, marks: bool) -> Vec2 {
    let side = face_side(side);
    let rows = if label { FACE_LABEL_H } else { 0.0 } + if marks { FACE_MARKS_H } else { 0.0 };
    vec2(side + 2.0 * FACE_TILE_PAD, side + 2.0 * FACE_TILE_PAD + rows)
}

/// The ignore ×'s button.
fn ignore_button(ui: &mut Ui, t: &Tokens, id: &str, picture: Rect, enabled: bool) -> Response {
    let rect =
        Rect::from_min_size(pos2(picture.max.x - FACE_IGNORE_INSET - FACE_IGNORE, picture.min.y + FACE_IGNORE_INSET), Vec2::splat(FACE_IGNORE));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    // A light-on-dark control over the photo, as the overlays' buttons are.
    let (lift, glyph) = if !enabled {
        (None, t.overlay_disabled)
    } else if r.is_pointer_button_down_on() {
        (Some(t.overlay_pressed), t.overlay_text)
    } else if r.hovered() {
        (Some(t.overlay_hover), t.overlay_text)
    } else {
        (None, t.overlay_icon)
    };
    p.rect_filled(rect, CornerRadius::same(3), t.overlay_bar);
    if let Some(lift) = lift {
        p.rect_filled(rect, CornerRadius::same(3), lift);
    }
    focus_ring(p, t, &r, rect, 3);
    // The search field's cross, 8 pt across.
    cross(p, rect.expand(2.0), glyph);
    register(ui.ctx(), id, Role::Button, "Ignore", Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Button, enabled, "Ignore"));
    tooltip(r, t, "Ignore this face", None)
}

/// A face tile ([`face_tile_size`]): the picture `tile.side` square [`FACE_TILE_PAD`] in from the
/// tile's top and sides, filling the square from its `uv` (square corners), or the `placeholder`
/// (`gray 38`) with a person in `text_disabled` until it arrives. Rest: no surround; hovered:
/// `hover × 0.5`; selected: `cell_selected` with a 2 pt outline round the picture, `photo_focus`
/// for the anchor and `photo_selected` for the rest; dragged: the picture at
/// [`FACE_DRAG_OPACITY`]; disabled: the picture dimmed by `excluded_dim`. The label (12 pt, one
/// line, cut with "…") is centred under it: a name in `text_label` (`text` hovered or selected), a
/// suggestion as "Name?" in `text_dim`. With `ignore`, a [`FACE_IGNORE`] `overlay_bar` square
/// (radius 3) [`FACE_IGNORE_INSET`] inside the picture's top-right holds an 8 pt × (`overlay_icon`;
/// `overlay_text` on `overlay_hover` hovered, `overlay_pressed` held, `overlay_disabled` disabled),
/// so it reads on any face; with `marks`, the ✓ / × pair is centred under the
/// label. The tile takes clicks and drags. Registers `face:<id>` (role `thumbnail`, active when
/// selected, labelled with what its label says or "Unnamed face"), `face:<id>/ignore` and
/// `face:<id>/confirm`, `face:<id>/reject`.
pub fn face_tile(ui: &mut Ui, t: &Tokens, tile: &FaceTile<'_>) -> FaceTileOut {
    let id = face_tile_id(tile.id);
    let enabled = ui.is_enabled();
    let side = face_side(tile.side);
    let has_label = tile.label != FaceLabel::None;
    let size = face_tile_size(side, has_label, tile.marks);
    let (_, rect) = ui.allocate_space(size);
    let r = ui.interact(rect, Id::new(&id), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let hovered = enabled && r.contains_pointer();
    let picture = Rect::from_min_size(rect.min + Vec2::splat(FACE_TILE_PAD), Vec2::splat(side));

    let p = ui.painter();
    if tile.selected {
        p.rect_filled(rect, 0, t.cell_selected);
    } else if hovered {
        p.rect_filled(rect, 0, t.hover.gamma_multiply(0.5));
    }
    let opacity = if tile.dragged { FACE_DRAG_OPACITY } else { 1.0 };
    match tile.picture {
        Some(pic) => {
            p.image(pic.texture, picture, pic.uv, Color32::WHITE.gamma_multiply(opacity));
        }
        None => {
            p.rect_filled(picture, 0, t.placeholder.gamma_multiply(opacity));
            let glyph = Rect::from_center_size(picture.center(), Vec2::splat((side * 0.5).round()));
            paint_people(p, glyph, PeopleIcon::Person, t.text_disabled.gamma_multiply(opacity));
        }
    }
    if !enabled {
        p.rect_filled(picture, 0, t.excluded_dim);
    }
    if tile.selected {
        let outline = if tile.focused { t.photo_focus } else { t.photo_selected };
        p.rect_stroke(picture, 0, Stroke::new(2.0, outline), StrokeKind::Outside);
    }
    focus_ring(p, t, &r, rect, 0);

    let suggested;
    let (text, color) = match tile.label {
        FaceLabel::None => ("", t.text_label),
        FaceLabel::Name(name) => (
            name,
            if !enabled {
                t.text_disabled
            } else if hovered || tile.selected {
                t.text
            } else {
                t.text_label
            },
        ),
        FaceLabel::Suggested(name) => {
            suggested = format!("{name}?");
            (suggested.as_str(), if enabled { t.text_dim } else { t.text_disabled })
        }
    };
    let mut row_top = picture.max.y;
    if has_label {
        let row = Rect::from_min_size(pos2(rect.min.x, row_top), vec2(rect.width(), FACE_LABEL_H));
        let galley = one_line(p, text, fonts::regular(t.count), color, rect.width() - 4.0);
        let at = Align2::CENTER_CENTER.align_size_within_rect(galley.size(), row).min;
        p.galley(at.round(), galley, color);
        row_top = row.max.y;
    }
    let accessible = match tile.label {
        FaceLabel::None => "Unnamed face",
        _ => text,
    };
    register(ui.ctx(), &id, Role::Thumbnail, accessible, Target::Widget(&r), tile.selected);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, tile.selected, accessible));

    // The parts on top take their own clicks.
    let ignore = tile.ignore && ignore_button(ui, t, &format!("{id}/ignore"), picture, enabled).clicked();
    let (confirm, reject) = if tile.marks {
        let row = Rect::from_min_size(pos2(rect.min.x, row_top), vec2(rect.width(), FACE_MARKS_H));
        let out = marks_at(ui, t, &id, row, enabled);
        (out.confirm, out.reject)
    } else {
        (false, false)
    };
    FaceTileOut { response: r, ignore, confirm, reject, picture }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_stay_in_range_and_sizes_add_their_rows() {
        assert_eq!(face_side(96.0), 96.0);
        assert_eq!(face_side(4.0), FACE_TILE_MIN);
        assert_eq!(face_side(9000.0), FACE_TILE_MAX);
        assert_eq!(face_side(f32::NAN), FACE_TILE_DEFAULT);
        assert_eq!(face_side(f32::NEG_INFINITY), FACE_TILE_DEFAULT);
        assert_eq!(face_tile_size(96.0, false, false), vec2(104.0, 104.0));
        assert_eq!(face_tile_size(96.0, true, false), vec2(104.0, 122.0));
        assert_eq!(face_tile_size(96.0, true, true), vec2(104.0, 146.0));
        assert_eq!(face_tile_id("12"), "face:12");
    }
}
