//! Face tiles (`roadmap.md` M13 → *Face tiles*): the square face pictures of the Unnamed grid, the
//! tree, person sections and the People pane, cut on a worker from the thumbnail ladder at the
//! smallest size covering twice the face as shown, kept in memory only, never in the thumbnail
//! cache.
//!
//! - [`Session::request_face_tiles`] asks for faces at a side in device pixels;
//!   [`Session::take_face_tiles`] (each frame) hands over the tiles made since, each with the face
//!   and the rectangle it was cut for. A face asked for again at the same side while its tile is
//!   on its way is not asked twice.
//! - **The cut.** A tile shows the face's box grown by [`MARGIN`] of its larger side on every side,
//!   made square around its centre (moved inside the photo where it would leave it). The ladder
//!   size read is the smallest whose pixels give that square at least [`COVER`] times the tile's
//!   side (the photo's size as displayed after its EXIF orientation, which is the stored frame the
//!   rectangles are in); the entry is the photo unedited, read from the thumbnail cache (made and
//!   stored there by the ladder when missing, as a cell's would be) or, with thumbnails off,
//!   decoded to that size. The square is brought to the tile's side and turned by the recipe's
//!   rotations and flips, so the face stands as the photo shows it.
//! - **Stale results** (the face's rectangle changed or its photo left the library before the tile
//!   was taken) are dropped; [`Session::close_face_tiles`] cancels what is on its way.
//!
//! The owner thread only submits jobs and takes their results: it never reads, decodes or cuts.
//! No face rectangle or name ever reaches a log (`faces.md` §7).

use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, Rect01};
use dalica_edit::Dihedral;
use dalica_image::{Limits, Rgba8};
use dalica_library::FaceRef;
use dalica_thumbs::ladder::{self, Render, Source};
use dalica_thumbs::pack::Cache;
use dalica_thumbs::{Pool, Priority};

use crate::error::CmdError;
use crate::session::Session;

/// The tile workers.
pub const WORKERS: usize = 1;
/// The largest tile side, in device pixels.
pub const MAX_SIDE: u32 = 1024;
/// The most faces one request asks for.
pub const MAX_REQUEST: usize = 4096;
/// Tiles on their way, at most (asking for more cancels nothing: the rest wait for a later ask).
pub const MAX_OUTSTANDING: usize = 8192;
/// Tiles kept until taken.
pub const MAX_READY: usize = 8192;
/// How many times over the ladder entry covers the tile's side across the cut square.
pub const COVER: f64 = 2.0;
/// Around the face's box, on every side, as a share of its larger side.
pub const MARGIN: f64 = 0.25;
/// Between two looks at the results while waiting.
const WAIT_STEP: Duration = Duration::from_millis(2);

/// One tile made.
#[derive(Clone, Debug)]
pub struct FaceTile {
    pub face: FaceRef,
    /// The face's rectangle (stored frame) the tile was cut for.
    pub rect: Rect01,
    /// The side asked for; the image is `side × side`.
    pub side: u32,
    pub image: Arc<Rgba8>,
}

/// Counters of the tiles' work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaceTileStats {
    /// Tiles asked for (a face already on its way not counted again).
    pub requested: u64,
    /// Tiles made and handed over.
    pub made: u64,
    /// Tiles whose face changed or left before they were taken, or that were closed.
    pub dropped: u64,
    pub failures: u64,
}

/// What one job needs: everything owned, so the worker reads nothing of the session's.
struct Job {
    source: Source,
    cache: Option<Arc<Cache>>,
    render: Render,
    screen: u32,
    limits: Limits,
    /// The photo's size as displayed after its EXIF orientation (0 × 0 when unknown).
    size: (u32, u32),
    rect: Rect01,
    side: u32,
    turn: Dihedral,
}

/// A tile asked for and not yet handed over.
#[derive(Clone, Copy, Debug)]
struct Asked {
    face: FaceRef,
    rect: Rect01,
    side: u32,
}

impl Asked {
    /// What the tile is of: the face, its side and its rectangle (by the bits of its corners).
    fn key(&self) -> (FaceRef, u32, [u64; 4]) {
        (self.face, self.side, [self.rect.x0(), self.rect.y0(), self.rect.x1(), self.rect.y1()].map(f64::to_bits))
    }
}

/// The face tiles' state.
#[derive(Default)]
pub struct FaceTiles {
    pool: Option<Pool<u64, Result<Rgba8, String>>>,
    next: u64,
    outstanding: HashMap<u64, Asked>,
    /// The tiles on their way, by what they are of ([`Asked::key`]).
    asked: HashSet<(FaceRef, u32, [u64; 4])>,
    ready: Vec<FaceTile>,
    stats: FaceTileStats,
}

impl std::fmt::Debug for FaceTiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceTiles").field("pending", &self.outstanding.len()).field("ready", &self.ready.len()).finish_non_exhaustive()
    }
}

impl FaceTiles {
    /// The tiles asked for and not yet handed over.
    pub fn pending(&self) -> usize {
        self.outstanding.len()
    }

    pub fn stats(&self) -> FaceTileStats {
        self.stats
    }
}

/// The long side the ladder entry needs for a `side`-pixel tile of the square around `rect` on a
/// photo of `size` (displayed after EXIF orientation; unknown: square).
fn needed_long_side(rect: &Rect01, size: (u32, u32), side: u32) -> u32 {
    let (w, h) = if size.0 == 0 || size.1 == 0 { (1.0, 1.0) } else { (f64::from(size.0), f64::from(size.1)) };
    let long = w.max(h);
    // The square's side as a share of the photo's long side.
    let square = ((rect.x1() - rect.x0()) * w).max((rect.y1() - rect.y0()) * h) * (1.0 + 2.0 * MARGIN) / long;
    let want = COVER * f64::from(side) / square.max(1e-6);
    if want.is_finite() { want.ceil().clamp(1.0, f64::from(ladder::MAX_SCREEN)) as u32 } else { ladder::MAX_SCREEN }
}

/// The square around `rect` (grown by [`MARGIN`]) in the pixels of `img`, moved inside it and at
/// most its shorter side: `(x, y, side)`.
fn square_in(img: &Rgba8, rect: &Rect01) -> Option<(u32, u32, u32)> {
    let (w, h) = (f64::from(img.width), f64::from(img.height));
    if img.width == 0 || img.height == 0 {
        return None;
    }
    let (fw, fh) = ((rect.x1() - rect.x0()) * w, (rect.y1() - rect.y0()) * h);
    let side = (fw.max(fh) * (1.0 + 2.0 * MARGIN)).round().clamp(1.0, w.min(h));
    let (cx, cy) = ((rect.x0() + rect.x1()) / 2.0 * w, (rect.y0() + rect.y1()) / 2.0 * h);
    let x = (cx - side / 2.0).clamp(0.0, w - side).round();
    let y = (cy - side / 2.0).clamp(0.0, h - side).round();
    let side = side as u32;
    Some((x as u32, y as u32, side.min(img.width.saturating_sub(x as u32)).min(img.height.saturating_sub(y as u32)).max(1)))
}

/// The pixels of `img` in the square `(x, y, side)`.
fn crop(img: &Rgba8, (x, y, side): (u32, u32, u32)) -> Option<Rgba8> {
    let (w, s) = (img.width as usize, side as usize);
    let mut data = Vec::with_capacity(s.checked_mul(s)?.checked_mul(4)?);
    for row in y as usize..y as usize + s {
        let start = (row.checked_mul(w)?.checked_add(x as usize)?).checked_mul(4)?;
        data.extend_from_slice(img.data.get(start..start.checked_add(s.checked_mul(4)?)?)?);
    }
    Some(Rgba8 { width: side, height: side, data })
}

/// The job: the ladder entry (or a decode), the square cut, sized and turned.
fn make(job: &Job, cancel: &std::sync::atomic::AtomicBool) -> Result<Rgba8, String> {
    let size = ladder::covering(needed_long_side(&job.rect, job.size, job.side), job.screen);
    let image = match &job.cache {
        Some(cache) => ladder::fetch_with(cache, &job.source, &job.render, size, job.screen, &job.limits, cancel).map_err(|e| e.to_string())?.image,
        None => dalica_thumbs::thumbnail_with(&job.source.path, ladder::pixels(size, job.screen), &job.render.decode_options(), &job.limits)
            .map_err(|e| e.to_string())?,
    };
    let square = square_in(&image, &job.rect).ok_or("the photo has no pixels")?;
    let cut = crop(&image, square).ok_or("the face lies outside the picture")?;
    let tile = dalica_image::resize::resize(&cut, job.side, job.side).map_err(|e| e.to_string())?;
    job.turn.apply(tile).map_err(|e| e.to_string())
}

impl Session {
    /// Asks for the tiles of `faces` at `side` device pixels (1 to [`MAX_SIDE`]), made on a worker;
    /// [`Session::take_face_tiles`] hands them over. Faces the library does not have are passed
    /// over; at most [`MAX_REQUEST`] per call. Returns how many tiles were asked for. Never touches
    /// the disk.
    pub fn request_face_tiles(&mut self, faces: &[FaceRef], side: u32) -> Result<usize, CmdError> {
        if !(1..=MAX_SIDE).contains(&side) {
            return Err(CmdError::BadParams(format!("a face tile's side is 1 to {MAX_SIDE} pixels")));
        }
        if faces.len() > MAX_REQUEST {
            return Err(CmdError::Limit(format!("at most {MAX_REQUEST} face tiles at once")));
        }
        let (cache, render, screen, limits) = self.thumbs.screen_source();
        let mut asked = 0;
        for r in faces {
            if self.face_tiles.outstanding.len() >= MAX_OUTSTANDING {
                break;
            }
            let Some(face) = self.library.face(*r) else { continue };
            let rect = face.rect;
            let wanted = Asked { face: *r, rect, side };
            if self.face_tiles.asked.contains(&wanted.key()) {
                continue;
            }
            let Some(job) = self.face_job(r.photo, rect, side, cache.clone(), render, screen, limits) else { continue };
            let tiles = &mut self.face_tiles;
            tiles.next = tiles.next.wrapping_add(1);
            let key = tiles.next;
            tiles.asked.insert(wanted.key());
            tiles.outstanding.insert(key, wanted);
            tiles.stats.requested = tiles.stats.requested.saturating_add(1);
            let pool = tiles.pool.get_or_insert_with(|| Pool::new(WORKERS));
            pool.submit(key, Priority::VISIBLE, move |cancel| {
                catch_unwind(AssertUnwindSafe(|| make(&job, cancel))).unwrap_or_else(|_| Err("cutting the tile panicked".into()))
            });
            asked += 1;
        }
        Ok(asked)
    }

    /// What a tile of photo `id` needs, owned (`None` when the library lacks the photo).
    #[allow(clippy::too_many_arguments, reason = "the ladder's inputs, each its own concern")]
    fn face_job(&self, id: PhotoId, rect: Rect01, side: u32, cache: Option<Arc<Cache>>, render: Render, screen: u32, limits: Limits) -> Option<Job> {
        let photo = self.library.photo(id)?;
        let folder = self.library.folder(photo.folder)?;
        let source = Source { path: folder.path.join(&photo.name), file_size: photo.size, mtime_ms: photo.mtime.as_millis() };
        let turn = crate::cmd::faces::turn_of(&self.library, id);
        Some(Job { source, cache, render, screen, limits, size: (photo.width, photo.height), rect, side, turn })
    }

    /// Takes in what the workers finished.
    fn poll_face_tiles(&mut self) {
        let tiles = &mut self.face_tiles;
        while let Some((key, result)) = tiles.pool.as_ref().and_then(Pool::try_recv) {
            let Some(asked) = tiles.outstanding.remove(&key) else { continue };
            tiles.asked.remove(&asked.key());
            match result {
                Ok(image) if self.library.face(asked.face).is_some_and(|f| f.rect == asked.rect) => {
                    if tiles.ready.len() >= MAX_READY {
                        tiles.ready.remove(0);
                        tiles.stats.dropped = tiles.stats.dropped.saturating_add(1);
                    }
                    tiles.ready.push(FaceTile { face: asked.face, rect: asked.rect, side: asked.side, image: Arc::new(image) });
                    tiles.stats.made = tiles.stats.made.saturating_add(1);
                }
                Ok(_) => tiles.stats.dropped = tiles.stats.dropped.saturating_add(1),
                Err(why) => {
                    tiles.stats.failures = tiles.stats.failures.saturating_add(1);
                    // The reason only: never the face's rectangle or person.
                    log::debug!("face tile of photo {}: {why}", asked.face.photo);
                }
            }
        }
    }

    /// The tiles made since the last call, in the order they were finished. Each frame of a view
    /// showing faces calls it.
    pub fn take_face_tiles(&mut self) -> Vec<FaceTile> {
        self.poll_face_tiles();
        std::mem::take(&mut self.face_tiles.ready)
    }

    /// Cancels the tiles on their way (the view showing faces closed); tiles made and not yet
    /// taken are dropped too.
    pub fn close_face_tiles(&mut self) {
        let tiles = &mut self.face_tiles;
        if let Some(pool) = &tiles.pool {
            for key in tiles.outstanding.keys() {
                pool.cancel(key);
            }
        }
        let gone = tiles.outstanding.len().saturating_add(tiles.ready.len());
        tiles.stats.dropped = tiles.stats.dropped.saturating_add(gone as u64);
        tiles.outstanding.clear();
        tiles.asked.clear();
        tiles.ready.clear();
    }

    /// Waits until every tile asked for has been made (or could not be) or `timeout` has passed,
    /// keeping the tiles for [`Session::take_face_tiles`]; returns whether it got there. Blocks
    /// the calling thread (CLI, tests, benches).
    pub fn wait_face_tiles(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.poll_face_tiles();
            if self.face_tiles.outstanding.is_empty() {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(WAIT_STEP);
        }
    }

    /// The face tiles' state.
    pub fn face_tiles(&self) -> &FaceTiles {
        &self.face_tiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
        Rect01::new(x0, y0, x1, y1).unwrap()
    }

    /// A small face needs a larger entry than a large one; an unknown photo size reads as square;
    /// the need is bounded.
    #[test]
    fn the_entry_covers_the_square_twice_over() {
        let small = needed_long_side(&rect(0.45, 0.45, 0.5, 0.5), (4000, 3000), 64);
        let large = needed_long_side(&rect(0.2, 0.2, 0.7, 0.7), (4000, 3000), 64);
        assert!(small > large, "{small} {large}");
        // The square is 0.05 × 1.5 of the long side: 2 × 64 / 0.075 ≈ 1707.
        assert_eq!(small, 1707);
        assert_eq!(needed_long_side(&rect(0.0, 0.0, 1.0, 1.0), (0, 0), 64), 86);
        assert!(needed_long_side(&rect(0.5, 0.5, 0.500001, 0.500001), (4000, 3000), 1024) <= ladder::MAX_SCREEN);
    }

    /// The square is cut around the face, moved inside the picture at the edges, and never larger
    /// than the picture's shorter side.
    #[test]
    fn the_square_stays_inside_the_picture() {
        let img = Rgba8 { width: 200, height: 100, data: vec![0; 200 * 100 * 4] };
        assert_eq!(square_in(&img, &rect(0.4, 0.4, 0.5, 0.6)), Some((75, 35, 30)));
        let (x, y, s) = square_in(&img, &rect(0.0, 0.0, 0.1, 0.1)).unwrap();
        assert_eq!((x, y), (0, 0));
        assert!(s >= 1);
        assert_eq!(square_in(&img, &rect(0.0, 0.0, 1.0, 1.0)), Some((50, 0, 100)));
        let cut = crop(&img, (150, 50, 50)).unwrap();
        assert_eq!((cut.width, cut.height, cut.data.len()), (50, 50, 50 * 50 * 4));
        assert!(crop(&img, (180, 50, 50)).is_none(), "outside the picture");
        assert_eq!(square_in(&Rgba8 { width: 0, height: 0, data: Vec::new() }, &rect(0.1, 0.1, 0.2, 0.2)), None);
    }
}
