//! The viewer's filmstrip (`viewing.md` §3.1): a dark band across the full width with square
//! thumbnails of the folder's photos, centred on the one shown (outlined in `photo_focus`), the
//! others dimmed; a click on one shows it. Slots before the first photo or after the last stay
//! empty, so the photo shown is always in the middle.
//!
//! **Pixels.** A photo the library holds draws what the session has in memory, never waiting
//! (`Session::thumbnail`, which queues what is missing): the ladder image turned by its recipe and
//! cut to its centre square, else the placeholder grey. A photo outside the library draws the
//! engine's small decode of its file by path (`Session::viewer_thumbnail`, its edits drawn, kept
//! in a capped cache), else the placeholder. The strip keeps its own textures, at most
//! [`MAX_TEXTURES`].
//!
//! Automation ids: `film:viewer/<n>`, the photo at place `n` of the folder (active when shown).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use dalica_edit::Dihedral;
use dalica_engine::Thumb;
use dalica_engine::viewer::Pixels;
use dalica_thumbs::Rgba8;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::epaint::Vertex;
use egui::{Color32, Id, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, Ui, pos2, vec2};

use super::listing::{Listing, Photo};
use crate::app::App;
use crate::panels::edit::filmstrip::centre_square;
use crate::panels::thumbnails::{recipe_orientation, turn_point};

/// The distance between two cells' centres, and the thumbnail square in each (points).
pub const CELL: f32 = 56.0;
pub const THUMB: f32 = 46.0;
/// The most thumbnails shown.
pub const MAX_CELLS: usize = 41;
/// The most textures the strip keeps.
pub const MAX_TEXTURES: usize = 96;
/// How much the photos other than the one shown are dimmed (black at this alpha over them).
pub const DIM_ALPHA: u8 = 120;

/// The band's automation id.
pub const STRIP_ID: &str = "region:viewerFilmstrip";

/// A filmstrip cell's automation id.
pub fn film_id(index: usize) -> String {
    format!("film:viewer/{index}")
}

/// How many cells a strip `width` wide shows: as many as fit, at most [`MAX_CELLS`], odd.
pub fn cells_for(width: f32) -> usize {
    if !width.is_finite() || width < CELL {
        return 0;
    }
    let n = ((width / CELL).floor() as usize).min(MAX_CELLS);
    if n.is_multiple_of(2) { n.saturating_sub(1) } else { n }
}

/// The places shown in `n` slots with place `index` of `len` in the middle one; slots before the
/// first or after the last are `None`.
pub fn window(len: usize, index: usize, n: usize) -> Vec<Option<usize>> {
    let half = n / 2;
    (0..n).map(|slot| index.checked_add(slot).and_then(|v| v.checked_sub(half)).filter(|k| *k < len)).collect()
}

#[derive(Clone)]
struct Texture {
    handle: TextureHandle,
    source: Weak<Rgba8>,
    width: u32,
    height: u32,
}

/// What the strip keeps between frames.
#[derive(Clone, Default)]
struct Strip {
    textures: HashMap<PathBuf, Texture>,
}

/// Draws the band in `band` and its cells at `opacity`; returns the place clicked.
pub fn show(app: &mut App, ui: &mut Ui, band: Rect, listing: &Listing, index: usize, opacity: f32) -> Option<usize> {
    if opacity <= 0.0 || !band.is_positive() {
        return None;
    }
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let id = Id::new("viewer.strip");
    let mut strip = ctx.data_mut(|d| d.remove_temp::<Strip>(id)).unwrap_or_default();
    // The band takes the clicks between the cells, so they never reach the photo behind it.
    let band_r = ui.interact(band, id, Sense::click());
    widgets::register(&ctx, STRIP_ID, Role::Other, "Filmstrip", Target::Widget(&band_r), false);
    let painter = ui.painter().with_clip_rect(band);
    painter.rect_filled(band, 0, t.overlay_bar.gamma_multiply(opacity));
    let n = cells_for(band.width());
    let left = (band.center().x - n as f32 * CELL / 2.0).round();
    let y = band.min.y + CELL / 2.0;
    let ppp = ctx.pixels_per_point();
    let mut clicked = None;
    let mut waiting = false;
    let slots = window(listing.photos.len(), index, n);
    for (slot, place) in slots.iter().enumerate() {
        let Some(place) = *place else { continue };
        let Some(photo) = listing.photos.get(place) else { continue };
        let square = Rect::from_center_size(pos2(left + CELL * (slot as f32 + 0.5), y), vec2(THUMB, THUMB));
        let r = ui.interact(square, Id::new(("viewer.film", place)), Sense::click());
        let current = place == index;
        waiting |= !thumbnail(&mut strip, app, &painter, &t, photo, square, ppp, opacity);
        if current {
            painter.rect_stroke(square, 0, Stroke::new(2.0, t.photo_focus.gamma_multiply(opacity)), StrokeKind::Outside);
        } else {
            painter.rect_filled(square, 0, Color32::from_black_alpha(DIM_ALPHA).gamma_multiply(opacity));
            if r.hovered() {
                painter.rect_stroke(square, 0, Stroke::new(1.0, t.overlay_icon.gamma_multiply(opacity)), StrokeKind::Outside);
            }
        }
        widgets::register(&ctx, &film_id(place), Role::Thumbnail, &photo.name, Target::Widget(&r), current);
        if r.on_hover_text(&photo.name).clicked() && !current {
            clicked = Some(place);
        }
    }
    if strip.textures.len() > MAX_TEXTURES {
        let shown: Vec<&PathBuf> = slots.iter().flatten().filter_map(|p| listing.photos.get(*p).map(|ph| &ph.path)).collect();
        strip.textures.retain(|path, _| shown.contains(&path));
    }
    // Thumbnails on their way: look again shortly (not at once, which would spin a CPU their
    // workers need).
    if waiting && app.session().is_some_and(|s| s.thumbnails_pending() > 0 || s.viewer_images_pending() > 0) {
        ctx.request_repaint_after(super::photo::PREVIEW_POLL);
    }
    ctx.data_mut(|d| d.insert_temp(id, strip));
    clicked
}

/// Paints `photo` into `square`: its thumbnail cut to the centre square (the library's from the
/// thumbnail ladder, turned by its recipe; another from the viewer's decode by path, its edits
/// drawn), else the placeholder. Returns whether that is all it will show (`false` while a
/// thumbnail is on its way).
#[allow(clippy::too_many_arguments)]
fn thumbnail(strip: &mut Strip, app: &mut App, painter: &egui::Painter, t: &Tokens, photo: &Photo, square: Rect, ppp: f32, opacity: f32) -> bool {
    let placeholder = t.placeholder.gamma_multiply(opacity);
    let Some(session) = app.session_mut() else {
        painter.rect_filled(square, 0, placeholder);
        return true;
    };
    let (image, turn, settled) = match photo.id {
        Some(id) => {
            let turn = recipe_orientation(&session.library().info(id).edits);
            let need = (THUMB * ppp * 1.5).ceil().clamp(1.0, 4096.0) as u32;
            match session.thumbnail(id, need) {
                Thumb::Image { image, .. } => (Some(image), turn, true),
                Thumb::Sketch(_) | Thumb::NotScanned => (None, turn, session.thumbnail_failure(id).is_some()),
            }
        }
        None => match session.viewer_thumbnail(&photo.path) {
            Pixels::Ready(image) => (Some(image), Dihedral::IDENTITY, true),
            Pixels::Pending => (None, Dihedral::IDENTITY, false),
            Pixels::Failed(_) => (None, Dihedral::IDENTITY, true),
        },
    };
    let Some(image) = image else {
        painter.rect_filled(square, 0, placeholder);
        return settled;
    };
    let fresh = strip.textures.get(&photo.path).is_some_and(|tex| std::ptr::eq(tex.source.as_ptr(), Arc::as_ptr(&image)));
    if !fresh && let Some(tex) = upload(painter.ctx(), &photo.name, &image) {
        strip.textures.insert(photo.path.clone(), tex);
    }
    match strip.textures.get(&photo.path) {
        Some(tex) => {
            painter.add(cropped(square, tex, turn, opacity));
        }
        None => {
            painter.rect_filled(square, 0, placeholder);
        }
    }
    true
}

fn upload(ctx: &egui::Context, name: &str, image: &Arc<Rgba8>) -> Option<Texture> {
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return None;
    }
    let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
    let handle = ctx.load_texture(format!("viewer.film:{name}"), pixels, egui::TextureOptions::LINEAR);
    Some(Texture { handle, source: Arc::downgrade(image), width: image.width, height: image.height })
}

/// `tex` (the upright picture) turned by `turn` and cut to its centre square, over `square`.
/// The square is cut from the texture's own size: the thumbnail as rendered, which a frame grows
/// and a crop cuts (`edit::render`), so a thumbnail still showing earlier edits is never stretched
/// to the recipe's new shape.
fn cropped(square: Rect, tex: &Texture, turn: Dihedral, opacity: f32) -> Shape {
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
    let color = Color32::WHITE.gamma_multiply(opacity);
    for (pos, shown) in corners {
        let (u, v) = turn_point(back, shown.x, shown.y);
        mesh.vertices.push(Vertex { pos, uv: pos2(u, v), color });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_is_centred_on_the_photo_shown() {
        assert_eq!((cells_for(1400.0), cells_for(900.0), cells_for(f32::NAN), cells_for(10.0)), (25, 15, 0, 0));
        assert_eq!(window(3, 0, 5), [None, None, Some(0), Some(1), Some(2)]);
        assert_eq!(window(9, 8, 3), [Some(7), Some(8), None]);
        assert_eq!(window(0, 0, 3), [None, None, None]);
    }
}
