//! A thumbnail cell (`ui-layout.md` §5.3, `design-system.md` §7): a square of the cell size with
//! the photo letterboxed in it (never cropped, never rounded), the selection's `cell_selected`
//! surround and 2 pt outline (white on the anchor), the hover surround, the star badge, and the
//! "!" of a photo that cannot be shown.
//!
//! **Boundary.** The parent calls [`show`] once per cell on screen, after the section's header,
//! with the frame, this part's state ([`Cells`]), the [`Thumbnails`] and a [`CellIn`]: the photo,
//! its section, the square on screen and (captions on) the caption line under it. The cell senses
//! clicks, registers `thumb:<PhotoId>` and its parts, asks [`Thumbnails::paint`] for the image and
//! returns a [`CellOut`] (its response) for [`super::input`] to act on.

use dalica_core::PhotoId;
use dalica_library::Photo;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Color32, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, vec2};

use super::Frame;
use crate::panels::thumbnails::{Drawn, PhotoRef, Thumbnails};
use crate::state;

/// What the cells keep between frames.
#[derive(Debug, Default)]
pub struct Cells {}

/// One cell to paint.
pub struct CellIn<'a> {
    pub photo: &'a Photo,
    pub section: state::Section,
    /// The square on screen.
    pub rect: Rect,
    /// The caption line under it, when captions are on.
    pub caption: Option<Rect>,
}

/// What a cell reports.
pub struct CellOut {
    pub photo: PhotoId,
    pub section: state::Section,
    /// The square on screen.
    pub rect: Rect,
    /// The letterboxed image inside it.
    pub image: Rect,
    pub response: Response,
    pub drawn: Drawn,
}

/// The image's rect inside a square `cell`: letterboxed to its aspect (`width × height`; unknown
/// sizes fill the square).
pub fn letterbox(cell: Rect, width: u32, height: u32) -> Rect {
    let aspect = if width > 0 && height > 0 { width as f32 / height as f32 } else { 1.0 };
    let size = if aspect >= 1.0 { vec2(cell.width(), cell.width() / aspect) } else { vec2(cell.height() * aspect, cell.height()) };
    Rect::from_center_size(cell.center(), size)
}

/// Paints one cell (see the module's boundary).
pub fn show(ui: &mut Ui, f: &Frame<'_>, _cells: &mut Cells, thumbs: &mut Thumbnails, c: &CellIn<'_>) -> CellOut {
    let t = f.t;
    let id = c.photo.id;
    let response = ui.interact(c.rect, Id::new(("thumb", id.get())), Sense::click());
    let selection = &f.state.selection;
    let selected = selection.contains(id);
    let image = letterbox(c.rect, c.photo.width, c.photo.height);
    let painter = ui.painter().clone();
    paint_surround(&painter, t, c.rect, image, selected, selection.anchor == Some(id), response.hovered());
    let device_px = image.width().max(image.height()) * f.pixels_per_point;
    let drawn = thumbs.paint(&painter, PhotoRef { session: f.session, photo: c.photo }, image, device_px);
    if drawn == Drawn::Failed {
        painter.text(image.center(), Align2::CENTER_CENTER, "!", fonts::semibold(18.0), t.caution);
    }
    if c.photo.star {
        widgets::star_badge(&painter, t, &format!("thumb:{}/star", id.get()), image);
    }
    widgets::register(ui.ctx(), &format!("thumb:{}", id.get()), Role::Thumbnail, &c.photo.name, Target::Widget(&response), selected);
    CellOut { photo: id, section: c.section, rect: c.rect, image, response, drawn }
}

fn paint_surround(painter: &egui::Painter, t: &Tokens, cell: Rect, image: Rect, selected: bool, anchor: bool, hovered: bool) {
    if selected {
        painter.rect_filled(cell, 0, t.cell_selected);
    } else if hovered {
        painter.rect_filled(cell, 0, t.hover.gamma_multiply(0.5));
    }
    if selected {
        let color = if anchor { Color32::WHITE } else { Color32::from_gray(170) };
        painter.rect_stroke(image, 0, Stroke::new(2.0, color), StrokeKind::Outside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn photos_are_letterboxed_never_cropped() {
        let cell = Rect::from_min_size(pos2(0.0, 0.0), vec2(144.0, 144.0));
        assert_eq!(letterbox(cell, 480, 320).size(), vec2(144.0, 96.0));
        assert_eq!(letterbox(cell, 320, 480).size(), vec2(96.0, 144.0));
        assert_eq!(letterbox(cell, 0, 0).size(), vec2(144.0, 144.0));
    }
}
