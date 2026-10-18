//! The face pictures of the People pane and the Unnamed grid (`crate::panels::faces`): the
//! engine's face tiles (`Session::request_face_tiles`, cut on a worker, in memory only) as
//! textures, kept per face and side. The session hands each tile to one taker only, so every view
//! showing faces reads this one store; each calls [`begin`] and [`finish`] around its faces.
//!
//! Each frame the pane calls [`begin`] (the tiles made since are taken and uploaded), [`picture`]
//! for each face it draws (a face without its texture yet is noted), then [`finish`] (the faces
//! noted are asked for, once per face, rectangle and side, and the window looks again while
//! tiles are on their way). A face whose rectangle changed is asked for again; at most [`MAX_KEPT`]
//! textures are kept (more empties the store, and what is shown is asked for again).

use std::collections::HashMap;

use dalica_core::Rect01;
use dalica_engine::face_tiles::MAX_SIDE;
use dalica_ui_kit::widgets::FacePicture;
use egui::{Id, Rect, TextureHandle, pos2};

use crate::app::App;
use crate::state::FaceRef;

/// The most face textures kept.
pub const MAX_KEPT: usize = 1024;
/// How often the window looks for tiles while some are on their way.
const POLL: std::time::Duration = std::time::Duration::from_millis(33);

#[derive(Clone, Default)]
struct Store {
    /// The textures by face and side (device pixels), with the rectangle each was cut for.
    kept: HashMap<(FaceRef, u32), (Rect01, TextureHandle)>,
    /// The faces asked for, with the rectangle asked for.
    asked: HashMap<(FaceRef, u32), Rect01>,
    /// The faces drawn this frame without their texture.
    wanted: Vec<(FaceRef, u32)>,
}

fn key() -> Id {
    Id::new("peoplePane.pictures")
}

/// The side in device pixels of a picture `points` wide.
pub fn side_px(ctx: &egui::Context, points: f32) -> u32 {
    let px = (points * ctx.pixels_per_point()).round();
    if px.is_finite() { (px as u32).clamp(1, MAX_SIDE) } else { 1 }
}

/// Takes the tiles the engine made since the last frame and uploads them.
pub fn begin(app: &mut App, ctx: &egui::Context) {
    let Some(session) = app.session_mut() else { return };
    let tiles = session.take_face_tiles();
    let mut made = Vec::with_capacity(tiles.len());
    for tile in tiles {
        let Ok(index) = u16::try_from(tile.face.index) else { continue };
        let face = FaceRef { photo: tile.face.photo, index };
        let image = &tile.image;
        let dims = [image.width as usize, image.height as usize];
        if dims[0] == 0 || dims[1] == 0 || dims[0].saturating_mul(dims[1]).saturating_mul(4) != image.data.len() {
            continue;
        }
        // Uploaded outside the store's lock: the texture manager takes the context's own.
        let pixels = egui::ColorImage::from_rgba_unmultiplied(dims, &image.data);
        let handle = ctx.load_texture(format!("face:{face}:{}", tile.side), pixels, egui::TextureOptions::LINEAR);
        made.push(((face, tile.side), (tile.rect, handle)));
    }
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Store>(key());
        s.wanted.clear();
        for (k, v) in made {
            if s.kept.len() >= MAX_KEPT {
                s.kept.clear();
                s.asked.clear();
            }
            s.kept.insert(k, v);
        }
    });
}

/// The picture of `face` (cut for `rect`) at `side` device pixels, if it has arrived; otherwise it
/// is noted for [`finish`] to ask for.
pub fn picture(ctx: &egui::Context, face: FaceRef, rect: Rect01, side: u32) -> Option<FacePicture> {
    ctx.data_mut(|d| {
        let s = d.get_temp_mut_or_default::<Store>(key());
        match s.kept.get(&(face, side)) {
            Some((cut, handle)) if *cut == rect => Some(FacePicture { texture: handle.id(), uv: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)) }),
            _ => {
                if s.asked.get(&(face, side)) != Some(&rect) {
                    s.wanted.push((face, side));
                }
                None
            }
        }
    })
}

/// Asks for the faces drawn without their picture this frame, and looks again soon while tiles are
/// on their way.
pub fn finish(app: &mut App, ctx: &egui::Context) {
    let Some(session) = app.session_mut() else { return };
    let wanted = ctx.data_mut(|d| std::mem::take(&mut d.get_temp_mut_or_default::<Store>(key()).wanted));
    let mut by_side: HashMap<u32, Vec<dalica_library::FaceRef>> = HashMap::new();
    let mut asked = Vec::new();
    for (face, side) in wanted {
        let r = dalica_library::FaceRef { photo: face.photo, index: usize::from(face.index) };
        let Some(rect) = session.library().face(r).map(|f| f.rect) else { continue };
        by_side.entry(side).or_default().push(r);
        asked.push(((face, side), rect));
    }
    for (side, faces) in by_side {
        for chunk in faces.chunks(dalica_engine::face_tiles::MAX_REQUEST) {
            // A refusal (a side out of range) leaves the placeholder.
            let _ = session.request_face_tiles(chunk, side);
        }
    }
    if !asked.is_empty() {
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Store>(key()).asked.extend(asked));
    }
    if session.face_tiles().pending() > 0 {
        ctx.request_repaint_after(POLL);
    }
}
