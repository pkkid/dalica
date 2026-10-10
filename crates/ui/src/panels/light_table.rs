//! The light table (`ui-layout.md` §5): one section per folder, newest first — a header block, then
//! a justified grid of square cells with each photo letterboxed, never cropped. Only the rows on
//! screen are laid out and painted; the section list is rebuilt only when the library changes.

use dalica_core::{FolderId, PhotoId};
use dalica_library::Library;
use dalica_thumbs::Priority;
use dalica_ui_kit::registry::{self, Role, WidgetInfo};
use dalica_ui_kit::{Tokens, fonts, widgets};
use egui::{Align2, Color32, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

use super::Panels;
use crate::app::App;

/// Section header block height, the gap below it, the left/right margins, the minimum horizontal
/// gap and the vertical gap between rows (points, `design-system.md` §4).
const HEADER: f32 = 56.0;
const HEADER_GAP: f32 = 12.0;
const MARGIN: f32 = 24.0;
const MIN_GAP: f32 = 12.0;
const ROW_GAP: f32 = 22.0;

/// One folder's section.
pub struct Section {
    pub folder: FolderId,
    pub title: String,
    pub subtitle: String,
    pub photos: Vec<PhotoId>,
}

/// The sections, rebuilt when the library's revision changes.
#[derive(Default)]
pub struct Sections {
    revision: Option<u64>,
    pub list: Vec<Section>,
}

impl Sections {
    pub fn update(&mut self, library: &Library) {
        if self.revision == Some(library.revision()) {
            return;
        }
        self.revision = Some(library.revision());
        self.list = library
            .folders_in_display_order()
            .into_iter()
            .map(|f| {
                let photos: Vec<PhotoId> = library.photos_in(f.id).iter().map(|p| p.id).collect();
                let count = if photos.len() == 1 { "1 photo".to_string() } else { format!("{} photos", photos.len()) };
                let subtitle = match library.folder_date(f.id) {
                    Some(d) => format!("{} · {count}", long_date(d.date())),
                    None => count,
                };
                Section { folder: f.id, title: folder_name(&f.path), subtitle, photos }
            })
            .collect();
    }
}

/// `June 4, 2026`.
pub fn long_date(d: dalica_core::Date) -> String {
    const MONTHS: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let month = MONTHS.get(usize::from(d.month()).saturating_sub(1)).copied().unwrap_or("?");
    format!("{month} {}, {}", d.day(), d.year())
}

/// A folder's display name: its last path component.
pub fn folder_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Columns, cell edge and horizontal gap for a grid `width` points wide.
pub fn grid(width: f32, cell: f32) -> (usize, f32) {
    let inner = (width - 2.0 * MARGIN).max(cell);
    let cols = (((inner + MIN_GAP) / (cell + MIN_GAP)).floor() as usize).max(1);
    let gap = if cols > 1 { (inner - cols as f32 * cell) / (cols - 1) as f32 } else { 0.0 };
    (cols, gap)
}

fn section_height(n: usize, cols: usize, cell: f32) -> f32 {
    let rows = n.div_ceil(cols.max(1));
    HEADER + HEADER_GAP + rows as f32 * cell + rows.saturating_sub(1) as f32 * ROW_GAP + ROW_GAP
}

/// The offset of `folder`'s section from the top of the light table, for scrolling to it.
pub fn offset_of(sections: &Sections, folder: FolderId, width: f32, cell: f32) -> Option<f32> {
    let (cols, _) = grid(width, cell);
    let mut y = 0.0;
    for s in &sections.list {
        if s.folder == folder {
            return Some(y);
        }
        y += section_height(s.photos.len(), cols, cell);
    }
    None
}

/// The image's rect inside a square `cell`: letterboxed to its aspect.
fn letterbox(cell: Rect, width: u32, height: u32) -> Rect {
    let aspect = if width > 0 && height > 0 { width as f32 / height as f32 } else { 1.0 };
    let size = if aspect >= 1.0 { vec2(cell.width(), cell.width() / aspect) } else { vec2(cell.height() * aspect, cell.height()) };
    Rect::from_center_size(cell.center(), size)
}

pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0, t.grid_bg);
    let Some(session) = app.session() else { return };
    p.sections.update(session.library());
    let cell = app.state().thumb_size;
    let width = rect.width();
    let (cols, gap) = grid(width, cell);
    let total: f32 = p.sections.list.iter().map(|s| section_height(s.photos.len(), cols, cell)).sum();
    let mut area = egui::ScrollArea::vertical().id_salt("light_table").auto_shrink([false, false]);
    if let Some(y) = p.scroll_to.take() {
        area = area.vertical_scroll_offset(y);
    }
    let mut clicked: Option<(PhotoId, bool)> = None;
    area.show_viewport(ui, |ui, viewport| {
        ui.set_min_height(total);
        let origin = ui.max_rect().min;
        let painter = ui.painter().clone();
        let library = session.library();
        let selection = &app.state().selection;
        let mut y = 0.0;
        for s in &p.sections.list {
            let h = section_height(s.photos.len(), cols, cell);
            if y + h < viewport.min.y || y > viewport.max.y {
                y += h;
                continue;
            }
            // The header block.
            let header = Rect::from_min_size(origin + vec2(0.0, y), vec2(width, HEADER));
            painter.rect_filled(header, 0, t.canvas);
            if app.state().folder == Some(s.folder) {
                painter.rect_filled(Rect::from_min_size(header.min, vec2(2.0, HEADER)), 0, t.accent);
            }
            painter.text(header.min + vec2(12.0, 10.0), Align2::LEFT_TOP, &s.title, fonts::semibold(t.title), t.text);
            painter.text(header.min + vec2(12.0, 32.0), Align2::LEFT_TOP, &s.subtitle, fonts::regular(12.5), t.text_dim);
            // The rows on screen.
            let top = y + HEADER + HEADER_GAP;
            for (i, &id) in s.photos.iter().enumerate() {
                let (row, col) = (i / cols, i % cols);
                let cy = top + row as f32 * (cell + ROW_GAP);
                if cy + cell < viewport.min.y || cy > viewport.max.y {
                    continue;
                }
                let Some(photo) = library.photo(id) else { continue };
                let cell_rect = Rect::from_min_size(origin + vec2(MARGIN + col as f32 * (cell + gap), cy), vec2(cell, cell));
                let response = ui.interact(cell_rect, Id::new(("thumb", id.get())), Sense::click());
                let selected = selection.contains(id);
                let image = letterbox(cell_rect, photo.width, photo.height);
                paint_cell(&painter, &t, cell_rect, image, selected, selection.anchor == Some(id), response.hovered());
                match p.thumbnails.get(id) {
                    Some(tex) => {
                        painter.image(tex, image, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        painter.rect_filled(image, 0, t.placeholder);
                        if p.thumbnails.failure(id).is_some() {
                            painter.text(image.center(), Align2::CENTER_CENTER, "!", fonts::semibold(18.0), t.caution);
                        }
                    }
                }
                let folder = library.folder(photo.folder);
                p.thumbnails.request(id, Priority::VISIBLE, || folder.map(|f| f.path.join(&photo.name)));
                if photo.star {
                    widgets::star_badge(&painter, &t, &format!("thumb:{}.star", id.get()), image);
                }
                registry::register(ui.ctx(), WidgetInfo::new(&format!("thumb:{}", id.get()), Role::Other, &photo.name, cell_rect, true, selected));
                if response.clicked() {
                    clicked = Some((id, ui.input(|i| i.modifiers.command)));
                }
            }
            y += h;
        }
    });
    if let Some((id, toggle)) = clicked {
        let mode = if toggle { "toggle" } else { "replace" };
        if app.run("select.set", &json!({ "ids": [id.get()], "mode": mode })).is_ok() {
            let folder = app.session().and_then(|s| s.library().photo(id)).map(|ph| ph.folder);
            app.state_mut().folder = folder;
        }
    }
}

fn paint_cell(painter: &egui::Painter, t: &Tokens, cell: Rect, image: Rect, selected: bool, anchor: bool, hovered: bool) {
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

    #[test]
    fn the_grid_is_justified_and_never_empty() {
        let (cols, gap) = grid(1100.0, 144.0);
        assert_eq!(cols, 6);
        assert!((MARGIN * 2.0 + 6.0 * 144.0 + 5.0 * gap - 1100.0).abs() < 0.01 && gap >= MIN_GAP);
        assert_eq!(grid(10.0, 144.0), (1, 0.0));
        assert_eq!(grid(f32::NAN, 144.0).0, 1);
        assert_eq!(section_height(0, 3, 100.0), HEADER + HEADER_GAP + ROW_GAP);
        assert_eq!(section_height(4, 3, 100.0), HEADER + HEADER_GAP + 200.0 + ROW_GAP + ROW_GAP);
    }

    #[test]
    fn photos_are_letterboxed_never_cropped() {
        let cell = Rect::from_min_size(pos2(0.0, 0.0), vec2(144.0, 144.0));
        assert_eq!(letterbox(cell, 480, 320).size(), vec2(144.0, 96.0));
        assert_eq!(letterbox(cell, 320, 480).size(), vec2(96.0, 144.0));
        assert_eq!(letterbox(cell, 0, 0).size(), vec2(144.0, 144.0));
    }

    #[test]
    fn dates_read_as_words() {
        assert_eq!(long_date(dalica_core::Date::new(2026, 6, 4).unwrap()), "June 4, 2026");
    }
}
