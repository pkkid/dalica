//! The packed disk cache of thumbnails (`architecture.md` §6.6).
//!
//! Under `<cache>/thumbs/`, each ladder [`Size`] has one append-only pack file of entry bytes and
//! one index of checksummed 40-byte records (`pack/format.rs`), never one file per thumbnail.
//! Entries are the JPEG q90 bytes ([`QUALITY`]) of one photo at one size, keyed by the
//! content-addressed [`Key`] of §6.4; the cache stores them as given.
//!
//! - **Reads** are positioned reads through `fs` (no memory mapping); an entry's CRC-32 is checked
//!   the first time this process reads it, and a mismatch is a miss.
//! - **Writes** append the bytes, then the index record. Nothing is synced per entry ([`Cache::sync`]
//!   does): a crash loses at most the tail, and a torn or damaged record costs only its own entry.
//! - **Dead entries** (replaced, removed) are counted; [`Cache::maintain`], run by a background
//!   job, compacts a size once they pass [`Options::compact_percent`] of its pack, into a new
//!   generation written beside the old one (the old one keeps serving meanwhile).
//! - **Pruning**: past [`Options::budget`] (2 GiB) the cache shrinks to
//!   [`Options::prune_to_percent`] (80 %) of it, the largest sizes first, the oldest entries of a
//!   size first.
//! - Only files of the cache's own name shape ([`is_own_name`]) are ever listed or deleted. The
//!   folder is held with an OS lock (`<dir>/lock`) so two processes never write one cache.

mod format;
mod one;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use dalica_fs::{LibraryLock, PackFile};

pub use format::{crc32, is_own_name};
use one::{EMPTY_BYTES, Pack, Rewrite};

/// JPEG quality of cache entries: the ladder encodes at this quality (fastest to decode, §6.6).
pub const QUALITY: u8 = 90;

/// The largest entry the cache takes (a screen-size preview of a very large display stays far below).
pub const MAX_ENTRY: u64 = 64 << 20;

/// The largest index file read whole at open (6.7 million records).
pub const MAX_INDEX: u64 = 256 << 20;

/// The default disk budget of the whole cache.
pub const DEFAULT_BUDGET: u64 = 2 << 30;

/// One rung of the thumbnail ladder (`architecture.md` §6.5), in device pixels on the long side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Size {
    S128,
    S256,
    S512,
    S1024,
    /// The edit view's preview: long side = the display's.
    Screen,
}

impl Size {
    /// Smallest first.
    pub const ALL: [Size; 5] = [Size::S128, Size::S256, Size::S512, Size::S1024, Size::Screen];
    /// The order pruning takes them in.
    pub const LARGEST_FIRST: [Size; 5] = [Size::Screen, Size::S1024, Size::S512, Size::S256, Size::S128];

    /// The name its files start with.
    pub fn name(self) -> &'static str {
        match self {
            Size::S128 => "128",
            Size::S256 => "256",
            Size::S512 => "512",
            Size::S1024 => "1024",
            Size::Screen => "screen",
        }
    }

    /// The long side in pixels; `None` for [`Size::Screen`], which depends on the display.
    pub fn pixels(self) -> Option<u32> {
        match self {
            Size::S128 => Some(128),
            Size::S256 => Some(256),
            Size::S512 => Some(512),
            Size::S1024 => Some(1024),
            Size::Screen => None,
        }
    }

    fn code(self) -> u32 {
        match self {
            Size::S128 => 128,
            Size::S256 => 256,
            Size::S512 => 512,
            Size::S1024 => 1024,
            Size::Screen => 0xffff_ffff,
        }
    }

    fn slot(self) -> usize {
        match self {
            Size::S128 => 0,
            Size::S256 => 1,
            Size::S512 => 2,
            Size::S1024 => 3,
            Size::Screen => 4,
        }
    }
}

/// The content-addressed key of one photo's rendering (`architecture.md` §6.4): a stable 128-bit
/// FNV-1a hash of the file path, its size and modification time, the recipe hash and the renderer
/// version. Any change to one of them is a new key, so an edited photo never reads a stale entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(pub u128);

impl Key {
    pub fn new(path: &Path, file_size: u64, mtime_ns: i128, recipe_hash: u64, renderer_version: u64) -> Key {
        let mut h = Fnv128::new();
        let path = path.as_os_str().as_encoded_bytes();
        h.write(&u64::try_from(path.len()).unwrap_or(u64::MAX).to_le_bytes());
        h.write(path);
        h.write(&file_size.to_le_bytes());
        h.write(&mtime_ns.to_le_bytes());
        h.write(&recipe_hash.to_le_bytes());
        h.write(&renderer_version.to_le_bytes());
        Key(h.0)
    }
}

/// FNV-1a, 128 bits: stable across runs, platforms and compiler versions (the std hasher is not).
struct Fnv128(u128);

impl Fnv128 {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

    fn new() -> Fnv128 {
        Fnv128(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u128::from(b)).wrapping_mul(Self::PRIME);
        }
    }
}

/// The cache's limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Disk bytes of all packs and indexes together before [`Cache::prune`] removes anything.
    pub budget: u64,
    /// What pruning shrinks the cache to, in percent of the budget.
    pub prune_to_percent: u8,
    /// A pack is compacted once its dead bytes pass this share of it, in percent…
    pub compact_percent: u8,
    /// …and are at least this many bytes (no rewrites of nearly empty packs).
    pub compact_min_bytes: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options { budget: DEFAULT_BUDGET, prune_to_percent: 80, compact_percent: 25, compact_min_bytes: 1 << 20 }
    }
}

/// Why the cache could not do something.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Fs(#[from] dalica_fs::Error),
    #[error("a cache entry of {0} bytes is larger than the cache takes")]
    TooLarge(u64),
    #[error("thumbnail cache file {} is damaged", .0.display())]
    Damaged(PathBuf),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// One size's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SizeStats {
    pub entries: u64,
    /// Bytes of the live entries.
    pub live_bytes: u64,
    /// Pack bytes no live entry uses (what a compaction gives back).
    pub dead_bytes: u64,
    /// Index records that describe no live entry.
    pub dead_records: u64,
    /// The pack and its index on disk.
    pub disk_bytes: u64,
    /// The current generation (bumped by each compaction or clear).
    pub generation: u32,
}

/// What [`Cache::maintain`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Maintenance {
    /// Sizes compacted because of dead entries.
    pub compacted: Vec<Size>,
    /// Sizes pruning took entries from, in the order it took them.
    pub pruned: Vec<Size>,
    /// Disk bytes before and after.
    pub before: u64,
    pub after: u64,
}

/// The packed thumbnail cache in one folder. `Sync`: pool workers share one cache; each size has
/// its own lock, held only while its index is looked up or one entry is read or appended.
#[derive(Debug)]
pub struct Cache {
    dir: PathBuf,
    options: Options,
    packs: [Mutex<Pack>; 5],
    _lock: LibraryLock,
}

impl Cache {
    /// Opens (creating it if needed) the cache in `dir`. Returns `Err(Fs(Locked))` at once when
    /// another process holds it.
    pub fn open(dir: &Path, options: Options) -> Result<Cache> {
        dalica_fs::create_private_dir_all(dir)?;
        let lock = LibraryLock::acquire(dir)?;
        let [a, b, c, d, e] = Size::ALL;
        let packs = [
            Mutex::new(Pack::load(dir, a)?),
            Mutex::new(Pack::load(dir, b)?),
            Mutex::new(Pack::load(dir, c)?),
            Mutex::new(Pack::load(dir, d)?),
            Mutex::new(Pack::load(dir, e)?),
        ];
        Ok(Cache { dir: dir.to_path_buf(), options, packs, _lock: lock })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn options(&self) -> Options {
        self.options
    }

    fn pack(&self, size: Size) -> MutexGuard<'_, Pack> {
        let slot = self.packs.get(size.slot()).unwrap_or(&self.packs[0]);
        slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The entry of `key` at `size`, or `None`.
    pub fn get(&self, size: Size, key: Key) -> Result<Option<Vec<u8>>> {
        self.pack(size).get(key)
    }

    /// Whether `key` has an entry at `size` (from memory; no disk access).
    pub fn contains(&self, size: Size, key: Key) -> bool {
        self.pack(size).map.contains_key(&key)
    }

    /// Stores `bytes` as the entry of `key` at `size`, replacing (and making dead) any earlier one.
    pub fn put(&self, size: Size, key: Key, bytes: &[u8]) -> Result<()> {
        self.pack(size).put(key, bytes)
    }

    /// Makes the entry of `key` at `size` dead; `Ok(false)` when there was none.
    pub fn remove(&self, size: Size, key: Key) -> Result<bool> {
        self.pack(size).remove(key)
    }

    /// Makes every size's entry of `key` dead (the photo was edited, deleted or moved).
    pub fn remove_all(&self, key: Key) -> Result<()> {
        for size in Size::ALL {
            self.remove(size, key)?;
        }
        Ok(())
    }

    /// Makes every write so far durable.
    pub fn sync(&self) -> Result<()> {
        for size in Size::ALL {
            self.pack(size).sync()?;
        }
        Ok(())
    }

    /// The live entries of `size`, oldest first, with their lengths in bytes: what a pruning
    /// policy ([`crate::upkeep`]) chooses from. From memory; no disk access.
    pub fn entries(&self, size: Size) -> Vec<(Key, u32)> {
        self.pack(size).entries_by_offset().into_iter().map(|(key, loc)| (key, loc.len)).collect()
    }

    pub fn stats(&self, size: Size) -> SizeStats {
        let p = self.pack(size);
        SizeStats {
            entries: u64::try_from(p.map.len()).unwrap_or(u64::MAX),
            live_bytes: p.live,
            dead_bytes: p.dead_bytes(),
            dead_records: p.dead_records,
            disk_bytes: p.disk_bytes(),
            generation: p.gen_,
        }
    }

    /// Disk bytes of every pack and index.
    pub fn disk_bytes(&self) -> u64 {
        Size::ALL.into_iter().map(|s| self.pack(s).disk_bytes()).fold(0, u64::saturating_add)
    }

    /// Whether `size`'s dead bytes have passed the compaction threshold.
    pub fn compaction_due(&self, size: Size) -> bool {
        let p = self.pack(size);
        let dead = p.dead_bytes();
        dead >= self.options.compact_min_bytes.max(1) && u128::from(dead) * 100 > u128::from(p.data.len()) * u128::from(self.options.compact_percent)
    }

    /// Rewrites `size` with only its live entries, byte for byte, into a new generation. Gets and
    /// puts of that size go on while the bulk is copied; entries put meanwhile are carried over at
    /// the end. `Ok(false)` when another compaction of that size is already running.
    pub fn compact(&self, size: Size) -> Result<bool> {
        let (snapshot, gen_, next, path) = {
            let mut p = self.pack(size);
            p.ready()?;
            if p.compacting {
                return Ok(false);
            }
            let next = p.gen_.checked_add(1).ok_or_else(|| Error::Damaged(p.data.path().to_path_buf()))?;
            p.compacting = true;
            (p.entries_by_offset(), p.gen_, next, p.data.path().to_path_buf())
        };
        let copied = PackFile::open(&path).map_err(Error::from).and_then(|reader| Rewrite::start(&self.dir, size, next, &reader, &snapshot));
        let mut p = self.pack(size);
        p.compacting = false;
        let rewrite = copied?;
        if p.gen_ != gen_ {
            // Cleared or reloaded meanwhile; the rewrite's temporary file is dropped.
            return Ok(false);
        }
        rewrite.finish(&mut p)?;
        Ok(true)
    }

    /// Shrinks the cache to [`Options::prune_to_percent`] of the budget once it is over the budget:
    /// the largest size first; within a size, its oldest entries first; a size that has to go
    /// whole is cleared. Returns the sizes it took entries from.
    pub fn prune(&self) -> Result<Vec<Size>> {
        let mut pruned = Vec::new();
        if self.disk_bytes() <= self.options.budget {
            return Ok(pruned);
        }
        let target = u64::try_from(u128::from(self.options.budget) * u128::from(self.options.prune_to_percent.min(100)) / 100).unwrap_or(u64::MAX);
        for size in Size::LARGEST_FIRST {
            let total = self.disk_bytes();
            if total <= target {
                break;
            }
            let need = total - target;
            let shrink = {
                let mut p = self.pack(size);
                p.ready()?;
                let disk = p.disk_bytes();
                if disk <= EMPTY_BYTES {
                    continue;
                }
                pruned.push(size);
                if disk.saturating_sub(EMPTY_BYTES) <= need {
                    p.clear()?;
                    false
                } else {
                    // Keep the newest entries that fit in what this size may still take.
                    let allowed = disk - need;
                    let mut after = p.compacted_bytes();
                    for (key, loc) in p.entries_by_offset() {
                        if after <= allowed {
                            break;
                        }
                        after = after.saturating_sub(u64::from(loc.len).saturating_add(format::RECORD_LEN));
                        p.map.remove(&key);
                        p.live = p.live.saturating_sub(u64::from(loc.len));
                        p.dead_records = p.dead_records.saturating_add(1);
                    }
                    true
                }
            };
            if shrink && !self.compact(size)? {
                // Another compaction of this size is running and will drop what was taken out.
                continue;
            }
        }
        Ok(pruned)
    }

    /// The background job's work: compacts every size whose dead entries passed the threshold,
    /// then prunes. Run it from a worker after a batch of writes, never on the UI thread.
    pub fn maintain(&self) -> Result<Maintenance> {
        let before = self.disk_bytes();
        let mut compacted = Vec::new();
        for size in Size::ALL {
            if self.compaction_due(size) && self.compact(size)? {
                compacted.push(size);
            }
        }
        let pruned = self.prune()?;
        Ok(Maintenance { compacted, pruned, before, after: self.disk_bytes() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv128_matches_the_published_test_vectors() {
        let hash = |b: &[u8]| {
            let mut h = Fnv128::new();
            h.write(b);
            h.0
        };
        assert_eq!(hash(b""), 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d);
        assert_eq!(hash(b"a"), 0xd228_cb69_6f1a_8caf_7891_2b70_4e4a_8964);
    }
}
