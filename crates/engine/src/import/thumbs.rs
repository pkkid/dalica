//! The candidates' thumbnails (`organizing.md` §3 item 2): kept in memory only, never in the
//! thumbnail cache on disk (a card's pictures are not the library's). The cache holds at most
//! [`CAP_BYTES`] of pixels; past that the least recently used go. A thumbnail that went is made
//! again from its file when asked for ([`remake`], on a thread of its own, at most
//! [`MAX_REMAKING`] at a time).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dalica_image::Rgba8;

use super::worker::lock;

/// Most bytes of pixels the cache holds.
pub const CAP_BYTES: usize = 128 * 1024 * 1024;

/// Most thumbnails made again at a time.
pub const MAX_REMAKING: usize = 2;

/// The listing's thumbnails by candidate id.
#[derive(Debug)]
pub(crate) struct ThumbCache {
    map: HashMap<u64, (Arc<Rgba8>, u64)>,
    tick: u64,
    bytes: usize,
    cap: usize,
    /// Ids whose thumbnail was made and then dropped.
    dropped: HashSet<u64>,
    /// Ids being made again.
    remaking: HashSet<u64>,
}

impl Default for ThumbCache {
    fn default() -> Self {
        ThumbCache::with_cap(CAP_BYTES)
    }
}

impl ThumbCache {
    pub fn with_cap(cap: usize) -> ThumbCache {
        ThumbCache { map: HashMap::new(), tick: 0, bytes: 0, cap, dropped: HashSet::new(), remaking: HashSet::new() }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    /// Keeps `img` as `id`'s thumbnail, dropping the least recently used past the cap (never the
    /// one just kept).
    pub fn insert(&mut self, id: u64, img: Arc<Rgba8>) {
        let tick = self.next_tick();
        self.remaking.remove(&id);
        self.dropped.remove(&id);
        let size = img.data.len();
        if let Some((old, _)) = self.map.insert(id, (img, tick)) {
            self.bytes = self.bytes.saturating_sub(old.data.len());
        }
        self.bytes = self.bytes.saturating_add(size);
        while self.bytes > self.cap {
            let Some(oldest) = self.map.iter().filter(|(k, _)| **k != id).min_by_key(|(_, (_, t))| *t).map(|(k, _)| *k) else { break };
            if let Some((gone, _)) = self.map.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(gone.data.len());
                self.dropped.insert(oldest);
            }
        }
    }

    /// `id`'s thumbnail, marked as just used.
    pub fn get(&mut self, id: u64) -> Option<Arc<Rgba8>> {
        let tick = self.next_tick();
        let (img, used) = self.map.get_mut(&id)?;
        *used = tick;
        Some(Arc::clone(img))
    }

    /// Whether `id`'s thumbnail is in memory.
    pub fn has(&self, id: u64) -> bool {
        self.map.contains_key(&id)
    }

    /// Whether `id`'s thumbnail was dropped and is not being made again yet; if so (and fewer
    /// than [`MAX_REMAKING`] are being made) it is marked as being made.
    fn claim_remake(&mut self, id: u64) -> bool {
        if self.dropped.contains(&id) && !self.remaking.contains(&id) && self.remaking.len() < MAX_REMAKING {
            self.remaking.insert(id);
            true
        } else {
            false
        }
    }

    /// The bytes of pixels held.
    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

/// Makes `id`'s dropped thumbnail again from `path` on a thread of its own, when it was dropped
/// and not already being made.
pub(crate) fn remake(cache: &Arc<Mutex<ThumbCache>>, id: u64, path: PathBuf, long_side: u32) {
    if !lock(cache).claim_remake(id) {
        return;
    }
    let cache2 = Arc::clone(cache);
    let spawned = std::thread::Builder::new().name("dalica-import-thumb".into()).spawn(move || {
        let made = std::panic::catch_unwind(|| make(&path, long_side)).ok().flatten();
        let mut c = lock(&cache2);
        match made {
            Some(img) => c.insert(id, Arc::new(img)),
            // It stays dropped; not asked for again until the next listing.
            None => {
                c.remaking.remove(&id);
                c.dropped.remove(&id);
            }
        }
    });
    if spawned.is_err() {
        lock(cache).remaking.remove(&id);
    }
}

/// A thumbnail of the picture at `path`, at most `long_side` pixels, upright: from its embedded
/// one when that decodes, else a scaled decode (as the listing makes them).
fn make(path: &Path, long_side: u32) -> Option<Rgba8> {
    let read = dalica_fs::read(path, dalica_scan::import::MAX_KEPT).ok()??;
    let limits = dalica_image::Limits::default();
    let header = dalica_image::read_header(&read.bytes, &limits).ok()?;
    if let Some(embedded) = header.thumbnail(&read.bytes)
        && let Ok(small) = dalica_image::decode_thumbnail(embedded, long_side, &limits)
        && let Ok(upright) = dalica_image::orient::apply(small, header.orientation)
    {
        return Some(upright);
    }
    dalica_image::decode_thumbnail(&read.bytes, long_side, &limits).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(n: usize) -> Arc<Rgba8> {
        Arc::new(Rgba8 { width: 1, height: 1, data: vec![0; n] })
    }

    #[test]
    fn the_least_recently_used_thumbnails_go_past_the_cap_and_may_be_made_again() {
        let mut c = ThumbCache::with_cap(300);
        c.insert(1, img(100));
        c.insert(2, img(100));
        c.insert(3, img(100));
        assert!(c.get(1).is_some(), "1 is used again: 2 is now the oldest");
        c.insert(4, img(100));
        assert!(c.has(1) && !c.has(2) && c.has(3) && c.has(4));
        assert_eq!(c.bytes(), 300);
        assert!(c.claim_remake(2), "a dropped thumbnail may be made again");
        assert!(!c.claim_remake(2), "once");
        assert!(!c.claim_remake(9), "one never made is not");
        c.insert(5, img(1000));
        assert!(c.has(5) && c.bytes() == 1000, "the one just kept stays even past the cap");
    }
}
