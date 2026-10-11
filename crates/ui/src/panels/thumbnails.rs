//! Thumbnails for the light table: decoded on the job pool's workers (never on the UI thread),
//! uploaded as egui textures when they arrive, kept in a byte-budgeted LRU.
//!
//! **The painting API** (fixed by the light-table skeleton; the never-blank work changes what is
//! behind it, not its shape). Each frame the light table calls, in order:
//!
//! 1. [`Thumbnails::begin_frame`] once, before anything is painted: finished work is applied
//!    (textures uploaded) and the theme noted;
//! 2. [`Thumbnails::paint`] once per cell on screen, with the photo ([`PhotoRef`]), the image's
//!    letterboxed rect in points and its long side in device pixels (which ladder size covers it);
//!    it paints the best it has into the rect and says what it drew ([`Drawn`]);
//! 3. [`Thumbnails::end_frame`] once, after the last cell, with how the view moved ([`Motion`]),
//!    for prefetch in the scroll direction.
//!
//! The cell paints everything around the image (selection, badges, the "!" of a failure); the
//! light table never looks inside this module. [`Thumbnails::pending`] says what is on its way
//! (`ui.settle` waits for it).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_library::Photo;
use dalica_thumbs::{Lru, Pool, Priority, Rgba8, Submitted};
use dalica_ui_kit::Tokens;
use egui::{Color32, Painter, Rect, pos2};

/// The long side thumbnails are decoded at (pixels): enough for the default 144 pt cell at 1.75×.
pub const LONG_SIDE: u32 = 256;
/// Texture memory kept for thumbnails.
pub const BUDGET: usize = 256 << 20;

/// The photo a cell shows, with what the thumbnails may need to find its pixels.
#[derive(Clone, Copy)]
pub struct PhotoRef<'a> {
    pub session: &'a Session,
    pub photo: &'a Photo,
}

impl PhotoRef<'_> {
    /// The photo's file, if its folder is known.
    pub fn path(&self) -> Option<PathBuf> {
        self.session.library().folder(self.photo.folder).map(|f| f.path.join(&self.photo.name))
    }
}

/// What [`Thumbnails::paint`] drew into a cell's image rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drawn {
    /// The photo's pixels.
    Image,
    /// The placeholder: nothing to show yet.
    Placeholder,
    /// The placeholder, for good: the photo cannot be shown ([`Thumbnails::failure`] says why).
    Failed,
}

/// How the light table's view moved this frame, for prefetch.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    /// The view's top in content points.
    pub offset: f64,
    /// The view's height in points.
    pub view_h: f32,
    /// Points per second, positive scrolling down.
    pub velocity: f64,
}

pub struct Thumbnails {
    pool: Pool<PhotoId, Result<Rgba8, String>>,
    textures: Lru<PhotoId, egui::TextureHandle>,
    requested: HashSet<PhotoId>,
    failed: HashMap<PhotoId, String>,
    /// `gray 38` from the theme of the current frame.
    placeholder: Color32,
}

impl Thumbnails {
    pub fn new() -> Thumbnails {
        Thumbnails {
            pool: Pool::new(dalica_thumbs::default_workers()),
            textures: Lru::new(BUDGET),
            requested: HashSet::new(),
            failed: HashMap::new(),
            placeholder: Tokens::default().placeholder,
        }
    }

    /// Starts a frame: uploads the thumbnails that arrived and notes the theme. Returns how many
    /// arrived.
    pub fn begin_frame(&mut self, ctx: &egui::Context, _session: &mut Session, t: &Tokens) -> usize {
        self.placeholder = t.placeholder;
        self.poll(ctx)
    }

    /// Paints `photo` into `rect` (its letterboxed image, in points) for an image `device_px` device
    /// pixels on its long side: its thumbnail, or the placeholder while there is none (asking for it
    /// at visible priority). Never touches the disk.
    pub fn paint(&mut self, painter: &Painter, photo: PhotoRef<'_>, rect: Rect, device_px: f32) -> Drawn {
        // One decode size for now; the ladder picks its size by `device_px`.
        let _ = device_px;
        let id = photo.photo.id;
        if let Some(tex) = self.textures.get(&id).map(egui::TextureHandle::id) {
            painter.image(tex, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            return Drawn::Image;
        }
        painter.rect_filled(rect, 0, self.placeholder);
        if self.failed.contains_key(&id) {
            return Drawn::Failed;
        }
        self.request(id, Priority::VISIBLE, || photo.path());
        Drawn::Placeholder
    }

    /// Ends a frame: how the view moved, for prefetch (none yet).
    pub fn end_frame(&mut self, _session: &mut Session, _motion: &Motion) {}

    /// Why `id` has no thumbnail, if it failed.
    pub fn failure(&self, id: PhotoId) -> Option<&str> {
        self.failed.get(&id).map(String::as_str)
    }

    /// Thumbnails requested and not yet arrived.
    pub fn pending(&self) -> usize {
        self.requested.len()
    }

    /// Asks for `id`'s thumbnail (the file at `path`) at `priority`, unless it is loaded, on its way
    /// or failed. `path` is only built when needed.
    fn request(&mut self, id: PhotoId, priority: Priority, path: impl FnOnce() -> Option<PathBuf>) {
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
    fn poll(&mut self, ctx: &egui::Context) -> usize {
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
}

impl Default for Thumbnails {
    fn default() -> Self {
        Thumbnails::new()
    }
}
