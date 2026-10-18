//! The CPU rasterizer: paints egui's tessellated output into an RGBA buffer, for headless
//! screenshots and fingerprint tests. It reproduces what `egui-wgpu` does on a gamma (`Rgba8Unorm`)
//! framebuffer, so a screenshot looks like the window:
//!
//! - vertex colours are premultiplied sRGBA in gamma space, interpolated across the triangle;
//! - fragment = vertex colour × texture sample, with egui's "predictable" bilinear filtering
//!   (texel centres, clamp to edge) or nearest when the texture asks for it;
//! - blending is premultiplied "over": `rgb = src + dst·(1 − src.a)`,
//!   `a = src.a·(1 − dst.a) + dst.a`; the result is stored as 8 bits per channel, no dithering;
//! - vertices snap to a 1/256-pixel grid and edges are tested in exact integers; pixels are sampled
//!   at their centres with a top-left fill rule, so triangles that share an edge paint each pixel
//!   on it exactly once (no gap, no double blend); clip rectangles are rounded to whole pixels like
//!   a scissor.
//!
//! Paint callbacks (custom GPU painting) have no CPU equivalent and are skipped.

use std::collections::HashMap;

use egui::epaint::{ClippedPrimitive, ImageData, Primitive, Vertex};
use egui::{Color32, TextureFilter, TextureId, TexturesDelta};

/// The largest edge, in pixels, of a texture or an image this rasterizer will allocate.
pub const MAX_SIDE: u32 = 8_192;
/// The most pixels of a texture or an image (64 MB at 4 bytes each).
pub const MAX_PIXELS: u64 = 1 << 24;

fn size_ok(width: u64, height: u64) -> bool {
    (1..=u64::from(MAX_SIDE)).contains(&width) && (1..=u64::from(MAX_SIDE)).contains(&height) && width * height <= MAX_PIXELS
}

/// An RGBA image, 4 bytes per pixel (premultiplied, as egui's framebuffer), rows top to bottom.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({} × {})", self.width, self.height)
    }
}

impl Image {
    /// The pixel at (x, y), if inside.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.rgba.get(i..i + 4).and_then(|p| <[u8; 4]>::try_from(p).ok())
    }
}

struct Texture {
    width: usize,
    height: usize,
    /// Premultiplied gamma RGBA, as egui uploads them.
    texels: Vec<Color32>,
    nearest: bool,
}

impl Texture {
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.clamp(0, self.width as i64 - 1) as usize;
        let y = y.clamp(0, self.height as i64 - 1) as usize;
        self.texels.get(y * self.width + x).map_or([0.0; 4], |&c| unit(c))
    }

    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (fx, fy) = (u * self.width as f32, v * self.height as f32);
        if self.nearest {
            return self.texel(fx.floor() as i64, fy.floor() as i64);
        }
        let (px, py) = (fx - 0.5, fy - 0.5);
        let (x0, y0) = (px.floor(), py.floor());
        let (tx, ty) = (px - x0, py - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (tl, tr, bl, br) = (self.texel(x0, y0), self.texel(x0 + 1, y0), self.texel(x0, y0 + 1), self.texel(x0 + 1, y0 + 1));
        let mut out = [0.0; 4];
        for (c, o) in out.iter_mut().enumerate() {
            let top = tl[c] + (tr[c] - tl[c]) * tx;
            let bottom = bl[c] + (br[c] - bl[c]) * tx;
            *o = top + (bottom - top) * ty;
        }
        out
    }
}

/// Error from [`Rasterizer::render`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RasterError {
    #[error("an image of {0} × {1} pixels is outside 1 … {MAX_SIDE} per side or {MAX_PIXELS} in all")]
    Size(u32, u32),
}

/// Keeps egui's textures and paints frames with them.
#[derive(Default)]
pub struct Rasterizer {
    textures: HashMap<TextureId, Texture>,
}

fn unit(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_array();
    [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, f32::from(a) / 255.0]
}

impl Rasterizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies (and takes) the texture uploads of a frame; call before [`render`](Self::render).
    /// Patches to a texture that does not exist, or that fall outside it, are ignored.
    pub fn set_textures(&mut self, delta: &mut TexturesDelta) {
        for (id, deltas) in delta.set.drain() {
            for d in deltas {
                let ImageData::Color(img) = &d.image;
                let [w, h] = img.size;
                if !size_ok(w as u64, h as u64) || img.pixels.len() != w * h {
                    continue;
                }
                let nearest = d.options.magnification == TextureFilter::Nearest;
                let texels = img.pixels.clone();
                match d.pos {
                    None => {
                        self.textures.insert(id, Texture { width: w, height: h, texels, nearest });
                    }
                    Some([x0, y0]) => {
                        let Some(t) = self.textures.get_mut(&id) else { continue };
                        for y in 0..h {
                            for x in 0..w {
                                let (tx, ty) = (x0.saturating_add(x), y0.saturating_add(y));
                                if tx >= t.width || ty >= t.height {
                                    continue;
                                }
                                if let (Some(dst), Some(src)) = (t.texels.get_mut(ty * t.width + tx), texels.get(y * w + x)) {
                                    *dst = *src;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Frees (and takes) the textures a frame released; call after [`render`](Self::render).
    pub fn free_textures(&mut self, delta: &mut TexturesDelta) {
        for id in delta.free.drain() {
            self.textures.remove(&id);
        }
    }

    /// Paints `primitives` (in points, scaled by `pixels_per_point`) over a `width × height` image
    /// cleared to `clear`.
    pub fn render(
        &self,
        primitives: &[ClippedPrimitive],
        pixels_per_point: f32,
        width: u32,
        height: u32,
        clear: Color32,
    ) -> Result<Image, RasterError> {
        if !size_ok(u64::from(width), u64::from(height)) {
            return Err(RasterError::Size(width, height));
        }
        let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { pixels_per_point } else { 1.0 };
        let mut target = Target { width: width as usize, height: height as usize, px: vec![clear.to_array(); width as usize * height as usize] };
        for prim in primitives {
            let Primitive::Mesh(mesh) = &prim.primitive else { continue };
            let Some(texture) = self.textures.get(&mesh.texture_id) else { continue };
            // The scissor rectangle, rounded to pixels and clamped to the image, as egui-wgpu does.
            let c = prim.clip_rect;
            let clamp_x = |v: f32| (v * ppp).round().clamp(0.0, width as f32) as usize;
            let clamp_y = |v: f32| (v * ppp).round().clamp(0.0, height as f32) as usize;
            let clip = [clamp_x(c.min.x), clamp_y(c.min.y), clamp_x(c.max.x), clamp_y(c.max.y)];
            if clip[0] >= clip[2] || clip[1] >= clip[3] {
                continue;
            }
            for &[i, j, k] in mesh.indices.as_chunks::<3>().0 {
                let v = |i: u32| mesh.vertices.get(i as usize).copied();
                if let (Some(a), Some(b), Some(c)) = (v(i), v(j), v(k)) {
                    target.triangle([a, b, c], ppp, clip, texture);
                }
            }
        }
        Ok(target.into_image())
    }
}

struct Target {
    width: usize,
    height: usize,
    /// 8 bits per channel, like the framebuffer: every blend is quantised.
    px: Vec<[u8; 4]>,
}

/// Sub-pixel steps per pixel: vertices snap to a 1/256-pixel grid (8 bits, as GPUs do), so every
/// edge test below is exact integer arithmetic.
const SUBPIXEL: i64 = 256;
/// The farthest a snapped coordinate may lie from the origin (±2^40 steps, 4 G pixels): products
/// of two differences stay far inside `i128`.
const FIXED_LIMIT: i64 = 1 << 40;

fn fixed(v: f32) -> i64 {
    // `as` saturates (and NaN is rejected before), so this never overflows.
    ((f64::from(v) * SUBPIXEL as f64).round() as i64).clamp(-FIXED_LIMIT, FIXED_LIMIT)
}

/// Whether the edge from `a` to `b` is a top or left edge of a triangle with positive area.
fn top_left(a: [i64; 2], b: [i64; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    (dy == 0 && dx > 0) || dy < 0
}

/// The edge function of `p` against the edge `a → b`, exact: `edge(a, b, p) == -edge(b, a, p)`,
/// so a pixel centre on an edge two triangles share is inside exactly one of them.
fn edge(a: [i64; 2], b: [i64; 2], p: [i64; 2]) -> i128 {
    i128::from(b[0] - a[0]) * i128::from(p[1] - a[1]) - i128::from(b[1] - a[1]) * i128::from(p[0] - a[0])
}

impl Target {
    fn triangle(&mut self, v: [Vertex; 3], ppp: f32, clip: [usize; 4], texture: &Texture) {
        let pf = v.map(|v| [v.pos.x * ppp, v.pos.y * ppp]);
        if pf.iter().flatten().any(|c| !c.is_finite()) {
            return;
        }
        let mut p = pf.map(|q| q.map(fixed));
        let mut v = v;
        let mut area = edge(p[0], p[1], p[2]);
        if area == 0 {
            return;
        }
        // egui's winding is not consistent: make every triangle positive.
        if area < 0 {
            p.swap(1, 2);
            v.swap(1, 2);
            area = -area;
        }
        // The pixels whose centres may be inside, within the scissor (which lies inside the image).
        let lo = |i: usize, c: usize| p.iter().map(|q| q[i]).min().map_or(c as i64, |m| m.div_euclid(SUBPIXEL).max(c as i64));
        let hi = |i: usize, c: usize| p.iter().map(|q| q[i]).max().map_or(c as i64, |m| (m + SUBPIXEL - 1).div_euclid(SUBPIXEL).min(c as i64));
        let (min_x, max_x, min_y, max_y) = (lo(0, clip[0]), hi(0, clip[2]), lo(1, clip[1]), hi(1, clip[3]));
        let colors = v.map(|v| unit(v.color));
        let uvs = v.map(|v| [v.uv.x, v.uv.y]);
        // Untextured shapes all point at one texel: sample it once.
        let flat_uv = uvs[0] == uvs[1] && uvs[1] == uvs[2];
        let flat_tex = flat_uv.then(|| texture.sample(uvs[0][0], uvs[0][1]));
        let flat_color = colors[0] == colors[1] && colors[1] == colors[2];
        let bias = [top_left(p[1], p[2]), top_left(p[2], p[0]), top_left(p[0], p[1])];
        let area = area as f64;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let q = [x * SUBPIXEL + SUBPIXEL / 2, y * SUBPIXEL + SUBPIXEL / 2];
                let w = [edge(p[1], p[2], q), edge(p[2], p[0], q), edge(p[0], p[1], q)];
                if w.iter().zip(bias).any(|(&e, tl)| e < 0 || (e == 0 && !tl)) {
                    continue;
                }
                let w = w.map(|e| (e as f64 / area) as f32);
                let (x, y) = (x as usize, y as usize);
                let color =
                    if flat_color { colors[0] } else { std::array::from_fn(|c| w[0] * colors[0][c] + w[1] * colors[1][c] + w[2] * colors[2][c]) };
                let tex = match flat_tex {
                    Some(t) => t,
                    None => {
                        texture.sample(w[0] * uvs[0][0] + w[1] * uvs[1][0] + w[2] * uvs[2][0], w[0] * uvs[0][1] + w[1] * uvs[1][1] + w[2] * uvs[2][1])
                    }
                };
                let src: [f32; 4] = std::array::from_fn(|c| color[c] * tex[c]);
                if let Some(dst) = self.px.get_mut(y * self.width + x) {
                    let d = dst.map(|v| f32::from(v) / 255.0);
                    let inv = 1.0 - src[3];
                    let q8 = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
                    *dst = [q8(src[0] + d[0] * inv), q8(src[1] + d[1] * inv), q8(src[2] + d[2] * inv), q8(src[3] * (1.0 - d[3]) + d[3])];
                }
            }
        }
    }

    fn into_image(self) -> Image {
        let rgba = self.px.into_iter().flatten().collect();
        Image { width: self.width as u32, height: self.height as u32, rgba }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::{ImageDelta, Mesh};
    use egui::{ColorImage, Rect, TextureOptions, pos2};

    fn white_texture() -> Rasterizer {
        let mut r = Rasterizer::new();
        let mut delta = TexturesDelta::default();
        delta.set.insert(TextureId::default(), [ImageDelta::full(ColorImage::new([1, 1], vec![Color32::WHITE]), TextureOptions::LINEAR)].into());
        r.set_textures(&mut delta);
        r
    }

    fn rect_mesh(rect: Rect, color: Color32) -> ClippedPrimitive {
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(rect, color);
        ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(mesh) }
    }

    #[test]
    fn a_rect_fills_exactly_its_pixels_and_blends_over() {
        let r = white_texture();
        let prims = [rect_mesh(Rect::from_min_max(pos2(2.0, 2.0), pos2(6.0, 5.0)), Color32::from_rgb(200, 0, 0))];
        let img = r.render(&prims, 1.0, 8, 8, Color32::BLACK).unwrap();
        let red = (0..8).flat_map(|y| (0..8).map(move |x| (x, y))).filter(|&(x, y)| img.pixel(x, y) == Some([200, 0, 0, 255])).count();
        assert_eq!(red, 12, "4 × 3 pixels, the shared diagonal painted once");
        assert_eq!(img.pixel(1, 1), Some([0, 0, 0, 255]));
        // Half-transparent white (premultiplied) over black is mid grey.
        let prims = [rect_mesh(Rect::from_min_max(pos2(0.0, 0.0), pos2(8.0, 8.0)), Color32::from_white_alpha(128))];
        let img = r.render(&prims, 1.0, 8, 8, Color32::BLACK).unwrap();
        assert_eq!(img.pixel(4, 4), Some([128, 128, 128, 255]));
        // Twice the scale paints twice the pixels per side.
        let prims = [rect_mesh(Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0)), Color32::WHITE)];
        let img = r.render(&prims, 2.0, 8, 8, Color32::BLACK).unwrap();
        assert_eq!(img.pixel(3, 3), Some([255; 4]));
        assert_eq!(img.pixel(4, 4), Some([0, 0, 0, 255]));
    }

    #[test]
    fn clipping_and_bad_input_never_panic() {
        let r = white_texture();
        let mut prim = rect_mesh(Rect::from_min_max(pos2(-1e9, -1e9), pos2(1e9, 1e9)), Color32::WHITE);
        prim.clip_rect = Rect::from_min_max(pos2(1.0, 1.0), pos2(3.0, 3.0));
        let img = r.render(&[prim], 1.0, 4, 4, Color32::BLACK).unwrap();
        assert_eq!(img.pixel(0, 0), Some([0, 0, 0, 255]));
        assert_eq!(img.pixel(2, 2), Some([255; 4]));
        // Out-of-range indices, NaN positions, unknown textures: skipped.
        let mut mesh = Mesh { indices: vec![0, 1, 99, 0, 1], ..Default::default() };
        mesh.colored_vertex(pos2(f32::NAN, 0.0), Color32::WHITE);
        mesh.colored_vertex(pos2(1.0, 1.0), Color32::WHITE);
        let mut other = Mesh::default();
        other.add_colored_rect(Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 4.0)), Color32::WHITE);
        other.texture_id = TextureId::User(7);
        let prims = [Primitive::Mesh(mesh), Primitive::Mesh(other)].map(|primitive| ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive });
        assert!(r.render(&prims, f32::NAN, 4, 4, Color32::BLACK).is_ok());
        assert_eq!(r.render(&[], 1.0, 0, 4, Color32::BLACK), Err(RasterError::Size(0, 4)));
        assert_eq!(r.render(&[], 1.0, 4, MAX_SIDE + 1, Color32::BLACK), Err(RasterError::Size(4, MAX_SIDE + 1)));
        assert_eq!(r.render(&[], 1.0, MAX_SIDE, MAX_SIDE, Color32::BLACK), Err(RasterError::Size(MAX_SIDE, MAX_SIDE)), "64 M pixels is over the cap");
    }

    /// A 2 × 2 texture of one colour, sampled as egui samples a photo.
    fn image_texture(color: Color32) -> Rasterizer {
        let mut r = white_texture();
        let mut delta = TexturesDelta::default();
        delta.set.insert(TextureId::User(1), [ImageDelta::full(ColorImage::new([2, 2], vec![color; 4]), TextureOptions::LINEAR)].into());
        r.set_textures(&mut delta);
        r
    }

    /// An image drawn as egui tessellates one: a textured quad of two triangles sharing a
    /// diagonal, from the top right to the bottom left (`add_rect_with_uv`) or from the top left to
    /// the bottom right (`main`, as a filled rectangle path is fanned).
    fn image_mesh(rect: Rect, tint: Color32, main: bool) -> ClippedPrimitive {
        let mut mesh = Mesh::with_texture(TextureId::User(1));
        mesh.add_rect_with_uv(rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        if main {
            // Vertices: 0 top left, 1 top right, 2 bottom left, 3 bottom right.
            mesh.indices = vec![0, 1, 3, 0, 3, 2];
        }
        ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(mesh) }
    }

    /// The pixels whose centres lie inside `rect` by more than the fixed-point step.
    fn centres_inside(rect: Rect, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
        let m = 0.01;
        (0..height).flat_map(move |y| (0..width).map(move |x| (x, y))).filter(move |&(x, y)| {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            cx > rect.min.x + m && cx < rect.max.x - m && cy > rect.min.y + m && cy < rect.max.y - m
        })
    }

    #[test]
    fn a_photo_quad_has_no_gap_on_its_diagonal() {
        // The album cell of the review screenshot: a letterboxed photo with fractional edges, drawn
        // over a dark shadow. Before the fix one pixel centre on the diagonal (328, 241) was
        // painted by neither triangle and the shadow showed through.
        let r = image_texture(Color32::from_rgb(230, 230, 230));
        // (272, 203.23)–(416, 300.77), to the bit: the diagonal runs through (328.5, 241.5).
        let rect = Rect::from_min_max(pos2(272.0, f32::from_bits(0x434b_3b04)), pos2(416.0, f32::from_bits(0x4396_6229)));
        let shadow = rect_mesh(rect.expand(2.0), Color32::from_rgb(10, 10, 10));
        let img = r.render(&[shadow, image_mesh(rect, Color32::WHITE, true)], 1.0, 440, 320, Color32::BLACK).unwrap();
        let gaps: Vec<_> = centres_inside(rect, 440, 320).filter(|&(x, y)| img.pixel(x, y) != Some([230, 230, 230, 255])).collect();
        assert!(gaps.is_empty(), "the shadow shows through at {gaps:?}");
    }

    #[test]
    fn quads_at_fractional_offsets_paint_every_centre_once() {
        // Half-transparent white over black: painted once is 128, twice 192, never 0.
        let r = image_texture(Color32::WHITE);
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32
        };
        for _ in 0..200 {
            let (x0, y0) = (next() * 40.0, next() * 40.0);
            let rect = Rect::from_min_max(pos2(x0, y0), pos2(x0 + 1.0 + next() * 60.0, y0 + 1.0 + next() * 60.0));
            for (ppp, main) in [(1.0, false), (1.0, true), (1.5, false), (1.5, true)] {
                let img = r.render(&[image_mesh(rect, Color32::from_white_alpha(128), main)], ppp, 160, 160, Color32::BLACK).unwrap();
                for (x, y) in centres_inside(rect * ppp, 160, 160) {
                    assert_eq!(img.pixel(x, y), Some([128, 128, 128, 255]), "({x}, {y}) in {rect:?} at ×{ppp}");
                }
                let twice =
                    (0..160).flat_map(|y| (0..160).map(move |x| (x, y))).filter(|&(x, y)| img.pixel(x, y).is_some_and(|p| p[0] > 128)).count();
                assert_eq!(twice, 0, "no pixel blended twice in {rect:?} at ×{ppp}");
            }
        }
    }

    #[test]
    fn bilinear_sampling_matches_texel_centres() {
        let t = Texture { width: 2, height: 1, texels: vec![Color32::TRANSPARENT, Color32::WHITE], nearest: false };
        assert_eq!(t.sample(0.25, 0.5), [0.0; 4]);
        assert_eq!(t.sample(0.75, 0.5), [1.0; 4]);
        assert_eq!(t.sample(0.5, 0.5), [0.5; 4]);
        assert_eq!(t.sample(-5.0, 0.5), [0.0; 4], "clamped to the edge");
        let n = Texture { nearest: true, ..t };
        assert_eq!(n.sample(0.49, 0.5), [0.0; 4]);
    }
}
