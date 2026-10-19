//! The pictures on the canvas, drawn from the thumbnail ladder as the render draws them
//! (`create::collage::render`): the background (the colour, or a picture covering the page; black
//! for Multiple Exposure), then per picture bottom first its shadow (Draw shadows; the render's
//! Drop Shadow down and to the right, as a soft-edged quad), its border and the part of the picture
//! its placement shows, turned by its angle; Multiple Exposure blends the pictures with equal
//! weights instead.
//!
//! **Never a decode, never the disk.** Each picture asks the session for the ladder size that
//! covers its place on the screen ([`Session::thumbnail`]: from memory, what is missing queued at
//! the visible priority), and draws the best it has: that size, a smaller one scaled up while it
//! loads, else the grey `placeholder`. At most [`UPLOADS_PER_FRAME`] new textures go up a frame;
//! a texture is replaced when the session's image of it changes (a larger size, new edits) and
//! dropped once its picture is no longer drawn. The ladder images are the photo upright as its
//! file shows it with its other edits applied; its recipe's turns and flips are applied here, as the
//! light table does ([`recipe_orientation`]), read once per photo
//! and library revision.

use std::collections::HashMap;
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_create::collage::render::{INSTANT_BORDER, SHADOW, WHITE_BORDER};
use dalica_create::collage::{Border, Collage, Placement, Style};
use dalica_edit::Dihedral;
use dalica_engine::thumbs::Size;
use dalica_engine::{Session, Thumb};
use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, TextureHandle, TextureId, pos2, vec2};

use super::geometry::Card;
use crate::panels::thumbnails::{recipe_orientation, turn_point};

/// The most textures uploaded in one frame; the rest go up in the next frames.
pub const UPLOADS_PER_FRAME: usize = 8;

/// One picture's texture: the ladder size it shows, the session's image it was made from (by
/// address: a new image is a new `Arc`), and the frame it was last drawn in.
#[derive(Clone)]
struct Texture {
    size: Size,
    image: usize,
    handle: TextureHandle,
    drawn: u64,
}

/// The canvas's textures and the photos' turns, kept between frames (in egui's temporary data).
#[derive(Clone, Default)]
pub struct Pictures {
    textures: HashMap<PhotoId, Texture>,
    turns: HashMap<PhotoId, Dihedral>,
    revision: u64,
    frame: u64,
    uploads: usize,
    /// Pictures drawn this frame without the ladder size that covers them (a smaller one, or the
    /// placeholder) while it is on its way.
    pub waiting: usize,
}

/// What a picture is drawn from.
#[derive(Clone, Copy)]
pub struct Source {
    pub texture: TextureId,
    pub turn: Dihedral,
    /// The texture's size as the photo shows it (turned), in pixels.
    pub size: [f32; 2],
}

impl Pictures {
    /// Starts a frame: the turns are read again when the library changed.
    pub fn begin(&mut self, session: &Session) {
        self.frame = self.frame.wrapping_add(1);
        self.uploads = 0;
        self.waiting = 0;
        let revision = session.library().revision();
        if revision != self.revision {
            self.turns.clear();
            self.revision = revision;
        }
    }

    /// Ends a frame: the textures of pictures not drawn in it go.
    pub fn end(&mut self) {
        let frame = self.frame;
        self.textures.retain(|_, t| t.drawn == frame);
    }

    /// How many textures are kept.
    pub fn textures(&self) -> usize {
        self.textures.len()
    }

    fn turn(&mut self, session: &Session, id: PhotoId) -> Dihedral {
        *self.turns.entry(id).or_insert_with(|| recipe_orientation(&session.library().info(id).edits))
    }

    /// What photo `id` is drawn from at `need` device pixels on its long side: the session's best
    /// image now, uploaded if this frame still may; `None` while it has none (the placeholder).
    pub fn source(&mut self, ctx: &egui::Context, session: &mut Session, id: PhotoId, need: f32) -> Option<Source> {
        let need = if need.is_finite() { need.clamp(1.0, 16_384.0) as u32 } else { 1 };
        let frame = self.frame;
        let turn = self.turn(session, id);
        let failed = session.thumbnail_failure(id).is_some();
        let mut covered = false;
        if let Thumb::Image { size, image, covers } = session.thumbnail(id, need) {
            let address = Arc::as_ptr(&image) as usize;
            let fresh = self.textures.get(&id).is_some_and(|t| t.size == size && t.image == address);
            if fresh {
                covered = covers;
            } else if self.uploads < UPLOADS_PER_FRAME
                && let Some(handle) = upload(ctx, id, &image)
            {
                self.uploads += 1;
                self.textures.insert(id, Texture { size, image: address, handle, drawn: frame });
                covered = covers;
            }
        }
        if !covered && !failed {
            self.waiting += 1;
        }
        let t = self.textures.get_mut(&id)?;
        t.drawn = frame;
        let [w, h] = t.handle.size().map(|v| v as f32);
        let size = if turn.quarters() % 2 == 1 { [h, w] } else { [w, h] };
        Some(Source { texture: t.handle.id(), turn, size })
    }
}

/// `image` as a texture; `None` when its pixels do not match its size.
fn upload(ctx: &egui::Context, id: PhotoId, image: &dalica_thumbs::Rgba8) -> Option<TextureHandle> {
    let dims = [image.width as usize, image.height as usize];
    if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
        return None;
    }
    let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
    Some(ctx.load_texture(format!("collage:{id}"), pixels, egui::TextureOptions::LINEAR))
}

/// A colour of the document (`[r, g, b]`).
pub fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The page's ground: the background colour, or black under Multiple Exposure.
pub fn ground(doc: &Collage) -> Color32 {
    if doc.style == Style::MultipleExposure { Color32::BLACK } else { rgb(doc.background.colour) }
}

/// `src` stretched over the turned quad `corners` (top-left, top-right, bottom-right, bottom-left
/// of the picture as shown), showing the part `shown` (`[x0, y0, x1, y1]` of the photo as it shows,
/// turned), tinted `tint`.
pub fn quad(painter: &Painter, src: &Source, corners: [Pos2; 4], shown: [f32; 4], tint: Color32) {
    let [x0, y0, x1, y1] = shown;
    let back = src.turn.inverse();
    let uv = |x: f32, y: f32| {
        let (u, v) = turn_point(back, x, y);
        pos2(u, v)
    };
    let uvs = [uv(x0, y0), uv(x1, y0), uv(x1, y1), uv(x0, y1)];
    let mut mesh = Mesh::with_texture(src.texture);
    for (pos, uv) in corners.into_iter().zip(uvs) {
        mesh.vertices.push(Vertex { pos, uv, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// The part of a picture of `size` that covers a page of `ratio` (width over height), centred.
pub fn cover(size: [f32; 2], ratio: f32) -> [f32; 4] {
    let pic = size[0] / size[1];
    if !(pic.is_finite() && ratio.is_finite() && pic > 0.0 && ratio > 0.0) {
        return [0.0, 0.0, 1.0, 1.0];
    }
    if pic > ratio {
        let f = ratio / pic;
        [(1.0 - f) / 2.0, 0.0, (1.0 + f) / 2.0, 1.0]
    } else {
        let f = pic / ratio;
        [0.0, (1.0 - f) / 2.0, 1.0, (1.0 + f) / 2.0]
    }
}

/// The background picture over the whole `page`.
pub fn background(painter: &Painter, page: Rect, src: &Source) {
    let corners = [page.left_top(), page.right_top(), page.right_bottom(), page.left_bottom()];
    quad(painter, src, corners, cover(src.size, page.aspect_ratio()), Color32::WHITE);
}

/// The render's shadow of `card`: the card moved down and to the right by its Drop Shadow's
/// distance and blurred by its blur (both per mille of the card's short side), at its density.
pub fn shadow(painter: &Painter, card: &Card) {
    let (distance, blur, fade, angle) = SHADOW;
    let short = card.card.width().min(card.card.height()) / 1000.0;
    let (s, c) = (angle as f32).to_radians().sin_cos();
    let offset = vec2(s * distance as f32 * short, -c * distance as f32 * short);
    let blur = blur as f32 * short;
    let opacity = (1.0 - fade as f32 / 100.0).clamp(0.0, 1.0);
    let dark = Color32::from_black_alpha((255.0 * opacity) as u8);
    let moved = |r: Rect| card.corners(r).map(|p| p + offset);
    let inner = moved(card.card.shrink(blur.min(card.card.width().min(card.card.height()) / 2.0)));
    let outer = moved(card.card.expand(blur));
    // The inner quad solid, a ring fading to nothing at the outer one.
    let mut mesh = Mesh::default();
    for p in inner {
        mesh.colored_vertex(p, dark);
    }
    for p in outer {
        mesh.colored_vertex(p, Color32::TRANSPARENT);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    for k in 0..4u32 {
        let n = (k + 1) % 4;
        mesh.add_triangle(k, n, 4 + n);
        mesh.add_triangle(k, 4 + n, 4 + k);
    }
    painter.add(Shape::mesh(mesh));
}

/// The border colour of `doc`'s pictures, if they have one.
pub fn border_colour(doc: &Collage) -> Option<Color32> {
    if !doc.style.has_borders() {
        return None;
    }
    match doc.settings.border {
        Border::None => None,
        Border::White => Some(rgb(WHITE_BORDER)),
        Border::Instant => Some(rgb(INSTANT_BORDER)),
    }
}

/// The whole picture's long side in device pixels that shows its part `p.shown` at `card`'s size
/// (what the ladder size must cover).
pub fn need(card: &Card, p: &Placement, pixels_per_point: f32) -> f32 {
    let s = &p.shown;
    let w = card.picture.width() / (s.x1() - s.x0()).max(1e-6) as f32;
    let h = card.picture.height() / (s.y1() - s.y0()).max(1e-6) as f32;
    w.max(h) * pixels_per_point
}

/// One picture: its shadow, border and pixels (or the placeholder), tinted `tint`.
pub fn picture(painter: &Painter, doc: &Collage, card: &Card, p: &Placement, src: Option<&Source>, placeholder: Color32, tint: Color32) {
    let exposure = doc.style == Style::MultipleExposure;
    if !exposure && doc.settings.shadows && tint == Color32::WHITE {
        shadow(painter, card);
    }
    if let Some(colour) = border_colour(doc) {
        painter.add(Shape::convex_polygon(card.corners(card.card).to_vec(), colour.gamma_multiply(f32::from(tint.a()) / 255.0), Stroke::NONE));
    }
    let corners = card.corners(card.picture);
    match src {
        Some(src) => {
            let s = &p.shown;
            quad(painter, src, corners, [s.x0() as f32, s.y0() as f32, s.x1() as f32, s.y1() as f32], tint);
        }
        None => {
            painter.add(Shape::convex_polygon(corners.to_vec(), placeholder.gamma_multiply(f32::from(tint.a()) / 255.0), Stroke::NONE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_background_picture_covers_the_page_centred() {
        assert_eq!(cover([200.0, 100.0], 1.0), [0.25, 0.0, 0.75, 1.0]);
        assert_eq!(cover([100.0, 200.0], 1.0), [0.0, 0.25, 1.0, 0.75]);
        assert_eq!(cover([100.0, 0.0], 1.0), [0.0, 0.0, 1.0, 1.0]);
    }
}
