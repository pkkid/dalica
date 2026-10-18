//! The filmstrip in the edit top bar (`editing.md` §2): ◀ (`button:film.previous`), up to nine
//! square-cropped thumbnails of the open photo's section in light-table order (`film:<PhotoId>`)
//! with the photo shown centred and outlined in `photo_focus`, ▶ (`button:film.next`). A click on
//! a thumbnail opens that photo (`view.edit`), which then sits in the centre; the arrows step
//! (`edit.previous`, `edit.next`) and are greyed at the section's ends. Slots before the first
//! photo or after the last stay empty, so the photo shown is always in the middle.
//!
//! **Comparing** (`editing.md` §4): the strip is the focused side's. In A│B a click shows that
//! photo on the focused side (`edit.comparePhoto`) and the arrows step that side
//! (`commands::compare::route`); in A│A a click leaves for that photo, asking "Keep which
//! version?" first when the two sides differ.
//!
//! **Sizes.** The strip's cells are [`CELL`] apart (the spec's 56 pt), so seven fit between the
//! arrows at a 900 pt window and nine at 1400; the thumbnail in each is a [`THUMB`] square, the
//! most the 40 pt bar holds with 4 pt above and below.
//!
//! **Pixels.** Each thumbnail draws what the session has in memory, the same frame and never
//! waiting (`Session::thumbnail`, which queues what is missing): the ladder image, turned by the
//! photo's recipe and cropped to its centre square; else the mean colour of its sketch; else the
//! placeholder grey. The strip keeps its own textures, at most [`MAX_TEXTURES`], uploaded again when
//! the session hands a new image (a covering size, or an edit's new thumbnail).
//!
//! **Cost.** The section's photos are looked up once per library revision, tree options and photo
//! shown, not per frame; a frame touches only the strip's few cells.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use dalica_core::PhotoId;
use dalica_edit::Dihedral;
use dalica_engine::Thumb;
use dalica_library::TreeOptions;
use dalica_thumbs::Rgba8;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::icons::EditIcon;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::epaint::Vertex;
use egui::{Color32, Id, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, Ui, pos2, vec2};
use serde_json::json;

use super::top_bar;
use crate::app::App;
use crate::commands::edit_view::{self, Place};
use crate::panels::light_table::tree_options;
use crate::panels::thumbnails::{recipe_orientation, turn_point};
use crate::selection::Table;

/// The distance between two cells' centres, and the thumbnail square in each.
pub const CELL: f32 = 56.0;
pub const THUMB: f32 = 32.0;
/// The arrows' width at each end.
pub const ARROW_W: f32 = 24.0;
/// The most thumbnails shown.
pub const MAX_CELLS: usize = 9;
/// The most textures the strip keeps.
pub const MAX_TEXTURES: usize = 32;

pub const PREVIOUS_ID: &str = "button:film.previous";
pub const NEXT_ID: &str = "button:film.next";
pub const PREVIOUS: &str = "Previous photo";
pub const NEXT: &str = "Next photo";

/// A filmstrip thumbnail's automation id.
pub fn film_id(id: PhotoId) -> String {
    format!("film:{id}")
}

/// How many thumbnails a strip `width` wide shows: as many [`CELL`]s as fit between the arrows, at
/// most [`MAX_CELLS`], odd (so the photo shown has as many on each side); 0 when not even one fits.
pub fn cells_for(width: f32) -> usize {
    if !width.is_finite() {
        return 0;
    }
    let fit = ((width - 2.0 * ARROW_W) / CELL).floor();
    let n = if fit >= 1.0 { (fit as usize).min(MAX_CELLS) } else { 0 };
    if n.is_multiple_of(2) { n.saturating_sub(1) } else { n }
}

/// The photos in `n` slots with `photos[index]` in the middle one; slots before the first photo or
/// after the last are `None`.
pub fn window(photos: &[PhotoId], index: usize, n: usize) -> Vec<Option<PhotoId>> {
    let half = n / 2;
    (0..n).map(|slot| index.checked_add(slot).and_then(|v| v.checked_sub(half)).and_then(|k| photos.get(k).copied())).collect()
}

/// A texture of one photo's thumbnail and the image it was made from.
#[derive(Clone)]
struct Texture {
    handle: TextureHandle,
    source: Weak<Rgba8>,
    width: u32,
    height: u32,
}

/// What the strip keeps between frames (in the context's data).
#[derive(Clone, Default)]
struct Strip {
    /// What `place` was looked up for: the library revision, the tree options, the photo shown.
    key: Option<(u64, TreeOptions, PhotoId)>,
    place: Option<Place>,
    textures: HashMap<PhotoId, Texture>,
    /// The mean colour of each sketch drawn, with the sketch it was decoded from.
    means: HashMap<PhotoId, (dalica_library::Sketch, Color32)>,
}

/// Draws the strip into `ui`'s rect (the `filmstrip` region).
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(photo) = edit_view::open_photo(app) else { return };
    let t = *app.tokens();
    let rect = ui.max_rect();
    let id = Id::new("filmstrip");
    let mut strip = ui.ctx().data_mut(|d| d.remove_temp::<Strip>(id)).unwrap_or_default();
    refresh(&mut strip, app, photo);
    if let Some(place) = strip.place.clone() {
        paint(&mut strip, app, ui, &t, rect, &place);
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, strip));
}

/// Looks the open photo's section up again when the library, the tree options or the photo changed.
fn refresh(strip: &mut Strip, app: &App, photo: PhotoId) {
    let Some(session) = app.session() else { return };
    let key = (session.library().revision(), tree_options(app), photo);
    if strip.key.as_ref() == Some(&key) {
        return;
    }
    strip.place = Table::of(app).and_then(|table| edit_view::place_in(&table, photo));
    strip.key = Some(key);
}

fn paint(strip: &mut Strip, app: &mut App, ui: &mut Ui, t: &Tokens, rect: Rect, place: &Place) {
    let n = cells_for(rect.width());
    let y = rect.center().y;
    let row = n as f32 * CELL;
    let left = (rect.center().x - row / 2.0).round();
    let len = place.photos.len();
    let arrow = |x: f32| Rect::from_center_size(pos2(x, y), vec2(ARROW_W, THUMB));
    let at_first = place.index == 0;
    let at_last = place.index.saturating_add(1) >= len;
    if top_bar::glyph_button(ui, t, PREVIOUS_ID, EditIcon::FilmstripPrevious, arrow(left - ARROW_W / 2.0), !at_first, PREVIOUS).clicked() {
        let _ = app.run("edit.previous", &json!({}));
    }
    if top_bar::glyph_button(ui, t, NEXT_ID, EditIcon::FilmstripNext, arrow(left + row + ARROW_W / 2.0), !at_last, NEXT).clicked() {
        let _ = app.run("edit.next", &json!({}));
    }
    let slots = window(&place.photos, place.index, n);
    let ppp = ui.ctx().pixels_per_point();
    let mut open = None;
    for (slot, id) in slots.iter().enumerate() {
        let Some(id) = *id else { continue };
        let square = Rect::from_center_size(pos2(left + CELL * (slot as f32 + 0.5), y), vec2(THUMB, THUMB));
        let r = ui.interact(square, Id::new(("film", id.get())), Sense::click());
        thumbnail(strip, app, ui, t, id, square, ppp);
        let current = slot == n / 2;
        if current {
            ui.painter().rect_stroke(square, 0, Stroke::new(2.0, t.photo_focus), StrokeKind::Outside);
        } else if r.hovered() {
            ui.painter().rect_stroke(square, 0, Stroke::new(1.0, t.text_dim), StrokeKind::Outside);
        }
        let name = app.session().and_then(|s| s.library().photo(id)).map(|p| p.name.clone()).unwrap_or_default();
        widgets::register(ui.ctx(), &film_id(id), Role::Thumbnail, &name, Target::Widget(&r), current);
        let r = r.on_hover_text(&name);
        if r.clicked() && !current {
            open = Some(id);
        }
    }
    if strip.textures.len() > MAX_TEXTURES {
        strip.textures.retain(|id, _| slots.contains(&Some(*id)));
    }
    if strip.means.len() > MAX_TEXTURES {
        strip.means.retain(|id, _| slots.contains(&Some(*id)));
    }
    if let Some(id) = open {
        // In A│B a click shows the photo on the focused side; otherwise it opens it.
        let command = if crate::commands::compare::mode(app) == crate::state::Compare::Two { "edit.comparePhoto" } else { "view.edit" };
        let _ = app.run(command, &json!({ "id": id.get() }));
    }
}

/// Paints photo `id` into `square`: its thumbnail cropped to the centre square, else its sketch's
/// mean colour, else the placeholder.
fn thumbnail(strip: &mut Strip, app: &mut App, ui: &Ui, t: &Tokens, id: PhotoId, square: Rect, ppp: f32) {
    let painter = ui.painter();
    let Some(session) = app.session_mut() else { return };
    let turn = recipe_orientation(&session.library().info(id).edits);
    // A square cut from a 3:2 photo: ask for the long side that covers the short one.
    let need = (THUMB * ppp * 1.5).ceil().clamp(1.0, 4096.0) as u32;
    match session.thumbnail(id, need) {
        Thumb::Image { image, .. } => {
            let fresh = strip.textures.get(&id).is_some_and(|tex| std::ptr::eq(tex.source.as_ptr(), Arc::as_ptr(&image)));
            if !fresh && let Some(tex) = upload(ui.ctx(), id, &image) {
                strip.textures.insert(id, tex);
            }
            match strip.textures.get(&id) {
                Some(tex) => {
                    painter.add(cropped(square, tex, turn));
                }
                None => {
                    painter.rect_filled(square, 0, t.placeholder);
                }
            }
        }
        Thumb::Sketch(sketch) => {
            let mean = match strip.means.get(&id) {
                Some((s, c)) if *s == sketch => Some(*c),
                _ => dalica_thumbs::sketch::decode(sketch.as_bytes()).ok().map(|d| {
                    let [r, g, b, _] = d.mean();
                    let c = Color32::from_rgb(r, g, b);
                    strip.means.insert(id, (sketch, c));
                    c
                }),
            };
            painter.rect_filled(square, 0, mean.unwrap_or(t.placeholder));
        }
        Thumb::NotScanned => {
            painter.rect_filled(square, 0, t.placeholder);
        }
    }
}

/// Uploads `image` as photo `id`'s strip texture; `None` when it is malformed.
fn upload(ctx: &egui::Context, id: PhotoId, image: &Arc<Rgba8>) -> Option<Texture> {
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return None;
    }
    let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
    let handle = ctx.load_texture(format!("film:{id}"), pixels, egui::TextureOptions::LINEAR);
    Some(Texture { handle, source: Arc::downgrade(image), width: image.width, height: image.height })
}

/// The centre square of the picture `width` × `height` as shown (each side 0–1 across it).
pub fn centre_square(width: u32, height: u32) -> Rect {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    if w >= h {
        let m = (1.0 - h / w) / 2.0;
        Rect::from_min_max(pos2(m, 0.0), pos2(1.0 - m, 1.0))
    } else {
        let m = (1.0 - w / h) / 2.0;
        Rect::from_min_max(pos2(0.0, m), pos2(1.0, 1.0 - m))
    }
}

/// `tex` (the upright picture) turned by `turn` and cropped to its centre square, over `square`.
/// The square is cut from the texture's own size: the thumbnail as rendered, which a frame grows
/// and a crop cuts (`edit::render`), so a thumbnail still showing earlier edits is never stretched
/// to the recipe's new shape.
fn cropped(square: Rect, tex: &Texture, turn: Dihedral) -> Shape {
    let (w, h) = turn.display_size(tex.width, tex.height);
    let crop = centre_square(w, h);
    let back = turn.inverse();
    let mut mesh = Mesh::with_texture(tex.handle.id());
    let corners: [(Pos2, Pos2); 4] = [
        (square.left_top(), crop.left_top()),
        (square.right_top(), crop.right_top()),
        (square.right_bottom(), crop.right_bottom()),
        (square.left_bottom(), crop.left_bottom()),
    ];
    for (pos, shown) in corners {
        let (u, v) = turn_point(back, shown.x, shown.y);
        mesh.vertices.push(Vertex { pos, uv: pos2(u, v), color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u64) -> Vec<PhotoId> {
        (1..=n).map(PhotoId::new).collect()
    }

    #[test]
    fn seven_cells_fit_at_900_and_nine_at_1400() {
        // The filmstrip region is the window less 460 (`panels::layout`).
        assert_eq!(cells_for(900.0 - 460.0), 7);
        assert_eq!(cells_for(1400.0 - 460.0), 9);
        assert_eq!(cells_for(2.0 * ARROW_W + 2.0 * CELL), 1, "odd");
        for bad in [0.0, -5.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(cells_for(bad) <= MAX_CELLS, "{bad}");
        }
        assert_eq!(cells_for(10.0), 0);
    }

    #[test]
    fn the_window_keeps_the_photo_shown_in_the_middle() {
        let p = ids(12);
        let s = |v: &[Option<PhotoId>]| v.iter().map(|o| o.map_or(0, PhotoId::get)).collect::<Vec<_>>();
        assert_eq!(s(&window(&p, 5, 7)), [3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(s(&window(&p, 0, 7)), [0, 0, 0, 1, 2, 3, 4], "empty slots before the first");
        assert_eq!(s(&window(&p, 11, 5)), [10, 11, 12, 0, 0], "and after the last");
        assert_eq!(s(&window(&p[..1], 0, 9)), [0, 0, 0, 0, 1, 0, 0, 0, 0]);
        assert!(window(&p, 3, 0).is_empty());
        assert_eq!(s(&window(&[], 0, 3)), [0, 0, 0]);
        assert_eq!(s(&window(&p, usize::MAX, 3)), [0, 0, 0], "an index past the end shows nothing");
    }

    #[test]
    fn the_crop_is_the_centre_square() {
        assert_eq!(centre_square(400, 300), Rect::from_min_max(pos2(0.125, 0.0), pos2(0.875, 1.0)));
        assert_eq!(centre_square(300, 400), Rect::from_min_max(pos2(0.0, 0.125), pos2(1.0, 0.875)));
        assert_eq!(centre_square(5, 5), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        assert_eq!(centre_square(0, 0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
    }
}
