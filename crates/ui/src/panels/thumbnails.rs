//! Thumbnails for the light table: decoded on the job pool's workers (never on the UI thread),
//! uploaded as egui textures when they arrive, kept in a byte-budgeted LRU.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use dalica_core::PhotoId;
use dalica_thumbs::{Lru, Pool, Priority, Rgba8, Submitted};

/// The long side thumbnails are decoded at (pixels): enough for the default 144 pt cell at 1.75×.
pub const LONG_SIDE: u32 = 256;
/// Texture memory kept for thumbnails.
pub const BUDGET: usize = 256 << 20;

pub struct Thumbnails {
    pool: Pool<PhotoId, Result<Rgba8, String>>,
    textures: Lru<PhotoId, egui::TextureHandle>,
    requested: HashSet<PhotoId>,
    failed: HashMap<PhotoId, String>,
}

impl Thumbnails {
    pub fn new() -> Thumbnails {
        Thumbnails {
            pool: Pool::new(dalica_thumbs::default_workers()),
            textures: Lru::new(BUDGET),
            requested: HashSet::new(),
            failed: HashMap::new(),
        }
    }

    /// The texture of `id`, if it has arrived.
    pub fn get(&mut self, id: PhotoId) -> Option<egui::TextureId> {
        self.textures.get(&id).map(egui::TextureHandle::id)
    }

    /// Why `id` has no thumbnail, if it failed.
    pub fn failure(&self, id: PhotoId) -> Option<&str> {
        self.failed.get(&id).map(String::as_str)
    }

    /// Asks for `id`'s thumbnail (the file at `path`) at `priority`, unless it is loaded, on its way
    /// or failed. `path` is only built when needed.
    pub fn request(&mut self, id: PhotoId, priority: Priority, path: impl FnOnce() -> Option<PathBuf>) {
        if self.textures.contains(&id) || self.requested.contains(&id) || self.failed.contains_key(&id) {
            return;
        }
        let Some(path) = path() else { return };
        let limits = dalica_image::Limits::default();
        let submitted = self.pool.submit(id, priority, move |_cancel| dalica_thumbs::thumbnail(&path, LONG_SIDE, &limits).map_err(|e| e.to_string()));
        if matches!(submitted, Submitted::Queued | Submitted::Merged | Submitted::Running) {
            self.requested.insert(id);
        }
    }

    /// Uploads the thumbnails that arrived. Returns how many.
    pub fn poll(&mut self, ctx: &egui::Context) -> usize {
        let mut n = 0;
        while let Some((id, result)) = self.pool.try_recv() {
            self.requested.remove(&id);
            n += 1;
            match result {
                Ok(img) => {
                    let size = [img.width as usize, img.height as usize];
                    if size[0].saturating_mul(size[1]).saturating_mul(4) != img.data.len() {
                        self.failed.insert(id, "the decoder returned a wrong-sized image".into());
                        continue;
                    }
                    let bytes = img.data.len();
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, &img.data);
                    let handle = ctx.load_texture(format!("thumb:{id}"), image, egui::TextureOptions::LINEAR);
                    self.textures.insert(id, handle, bytes);
                }
                Err(why) => {
                    self.failed.insert(id, why);
                }
            }
        }
        n
    }

    /// Thumbnails requested and not yet arrived.
    pub fn pending(&self) -> usize {
        self.requested.len()
    }
}

impl Default for Thumbnails {
    fn default() -> Self {
        Thumbnails::new()
    }
}
