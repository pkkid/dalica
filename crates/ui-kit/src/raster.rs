//! The CPU rasterizer: paints egui's tessellated output into an RGBA buffer, for headless
//! screenshots and fingerprint tests. It reproduces what `egui-wgpu` does on a gamma (`Rgba8Unorm`)
//! framebuffer, so a screenshot looks like the window:
//!
//! - vertex colours are premultiplied sRGBA in gamma space, interpolated across the triangle;
//! - fragment = vertex colour × texture sample, with egui's "predictable" bilinear filtering
//!   (texel centres, clamp to edge) or nearest when the texture asks for it;
//! - blending is premultiplied "over": `rgb = src + dst·(1 − src.a)`,
//!   `a = src.a·(1 − dst.a) + dst.a`; the result is stored as 8 bits per channel, no dithering;
//! - pixels are sampled at their centres with a top-left fill rule, so triangles that share an edge
//!   never paint a pixel twice; clip rectangles are rounded to whole pixels like a scissor.
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

/// Whether the edge from `a` to `b` is a top or left edge of a triangle with positive area.
fn top_left(a: [f32; 2], b: [f32; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    (dy == 0.0 && dx > 0.0) || dy < 0.0
}

fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

impl Target {
    fn triangle(&mut self, v: [Vertex; 3], ppp: f32, clip: [usize; 4], texture: &Texture) {
        let mut p = v.map(|v| [v.pos.x * ppp, v.pos.y * ppp]);
        let mut v = v;
        if p.iter().flatten().any(|c| !c.is_finite()) {
            return;
        }
        let mut area = edge(p[0], p[1], p[2]);
        if area == 0.0 {
            return;
        }
        // egui's winding is not consistent: make every triangle positive.
        if area < 0.0 {
            p.swap(1, 2);
            v.swap(1, 2);
            area = -area;
        }
        let min_x = p.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min).floor().max(clip[0] as f32) as usize;
        let max_x = p.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(clip[2] as f32) as usize;
        let min_y = p.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min).floor().max(clip[1] as f32) as usize;
        let max_y = p.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(clip[3] as f32) as usize;
        let colors = v.map(|v| unit(v.color));
        let uvs = v.map(|v| [v.uv.x, v.uv.y]);
        // Untextured shapes all point at one texel: sample it once.
        let flat_uv = uvs[0] == uvs[1] && uvs[1] == uvs[2];
        let flat_tex = flat_uv.then(|| texture.sample(uvs[0][0], uvs[0][1]));
        let flat_color = colors[0] == colors[1] && colors[1] == colors[2];
        let bias = [top_left(p[1], p[2]), top_left(p[2], p[0]), top_left(p[0], p[1])];
        for y in min_y..max_y {
            for x in min_x..max_x {
                let q = [x as f32 + 0.5, y as f32 + 0.5];
                let w = [edge(p[1], p[2], q), edge(p[2], p[0], q), edge(p[0], p[1], q)];
                if w.iter().zip(bias).any(|(&e, tl)| e < 0.0 || (e == 0.0 && !tl)) {
                    continue;
                }
                let w = w.map(|e| e / area);
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
