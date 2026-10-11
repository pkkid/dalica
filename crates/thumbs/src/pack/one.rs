//! One ladder size's pack: the open files of its current generation and the in-memory index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dalica_fs::{PackFile, TempFile, list_shaped, remove_shaped};

use super::format::{self, HEADER_LEN, INDEX_MAGIC, PACK_MAGIC, Part, RECORD_LEN, Record, crc32};
use super::{Error, Key, MAX_ENTRY, MAX_INDEX, Result, Size};

/// Where an entry's bytes are in the pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Loc {
    pub offset: u64,
    pub len: u32,
    pub crc: u32,
    /// The bytes were checked against `crc` in this process (or written by it).
    pub verified: bool,
}

/// The open pack of one size.
#[derive(Debug)]
pub(crate) struct Pack {
    dir: PathBuf,
    size: Size,
    pub gen_: u32,
    pub data: PackFile,
    index: PackFile,
    pub map: HashMap<Key, Loc>,
    /// Sum of the live entries' lengths.
    pub live: u64,
    /// Index records that no longer describe a live entry (replaced, removed, damaged).
    pub dead_records: u64,
    /// A write failed part-way: reload from disk before the next write.
    broken: bool,
    /// A compaction of this size is under way (on some thread).
    pub compacting: bool,
}

/// The bytes an empty pack takes on disk (its two headers).
pub(crate) const EMPTY_BYTES: u64 = 2 * HEADER_LEN;

fn own(name: &str) -> bool {
    format::is_own_name(name)
}

impl Pack {
    /// Opens the newest whole generation of `size` in `dir`, or starts an empty one. Other
    /// generations and stray temporary files of this size are removed (best effort). A torn index
    /// tail is cut; damaged records are skipped one by one.
    pub fn load(dir: &Path, size: Size) -> Result<Pack> {
        let files = list_shaped(dir, |n| matches!(format::parse_name(n).or_else(|| format::parse_temp_name(n)), Some((s, _, _)) if s == size))?;
        let mut whole: Option<u32> = None;
        let mut newest = 0u32;
        for f in &files {
            if let Some((_, g, Part::Pack)) = format::parse_name(&f.name)
                && files.iter().any(|o| o.name == format::file_name(size, g, Part::Index))
            {
                whole = whole.max(Some(g));
            }
            if let Some((_, g, _)) = format::parse_name(&f.name).or_else(|| format::parse_temp_name(&f.name)) {
                newest = newest.max(g);
            }
        }
        let keep = whole.map(|g| [format::file_name(size, g, Part::Pack), format::file_name(size, g, Part::Index)]);
        for f in &files {
            if !keep.as_ref().is_some_and(|k| k.contains(&f.name)) {
                let _ = remove_shaped(&dir.join(&f.name), own);
            }
        }
        if let Some(g) = whole {
            if let Some(pack) = Pack::open_gen(dir, size, g)? {
                return Ok(pack);
            }
            // Unusable headers: start over in the next generation.
            for part in [Part::Pack, Part::Index] {
                let _ = remove_shaped(&dir.join(format::file_name(size, g, part)), own);
            }
        }
        let next = newest.checked_add(1).ok_or_else(|| gen_exhausted(dir))?;
        Pack::create(dir, size, next)
    }

    /// Opens generation `g`; `Ok(None)` when its headers are not this size's.
    fn open_gen(dir: &Path, size: Size, g: u32) -> Result<Option<Pack>> {
        let data = PackFile::open(&dir.join(format::file_name(size, g, Part::Pack)))?;
        let mut index = PackFile::open(&dir.join(format::file_name(size, g, Part::Index)))?;
        if data.len() < HEADER_LEN || index.len() < HEADER_LEN || index.len() > MAX_INDEX {
            return Ok(None);
        }
        let head = data.read_vec(0, HEADER_LEN, HEADER_LEN)?;
        let body = index.read_vec(0, index.len(), MAX_INDEX)?;
        if !format::header_ok(&head, PACK_MAGIC, size) || !body.get(..16).is_some_and(|h| format::header_ok(h, INDEX_MAGIC, size)) {
            return Ok(None);
        }
        let mut map: HashMap<Key, Loc> = HashMap::new();
        let mut records = 0u64;
        let records_bytes = body.get(16..).unwrap_or_default();
        let (chunks, torn) = records_bytes.as_chunks::<{ format::RECORD }>();
        for chunk in chunks {
            records += 1;
            match Record::decode(chunk) {
                Some(Record::Put { key, offset, len, crc }) => {
                    let fits = offset >= HEADER_LEN
                        && u64::from(len) <= MAX_ENTRY
                        && offset.checked_add(u64::from(len)).is_some_and(|end| end <= data.len());
                    if fits {
                        map.insert(key, Loc { offset, len, crc, verified: false });
                    }
                }
                Some(Record::Remove { key }) => {
                    map.remove(&key);
                }
                None => {}
            }
        }
        let torn = u64::try_from(torn.len()).unwrap_or(0);
        let mut broken = false;
        if torn > 0 && index.truncate(index.len().saturating_sub(torn)).is_err() {
            // Reads still work; the next write reloads and tries the cut again.
            broken = true;
        }
        let live = map.values().map(|l| u64::from(l.len)).sum();
        let dead_records = records.saturating_sub(u64::try_from(map.len()).unwrap_or(u64::MAX));
        Ok(Some(Pack { dir: dir.to_path_buf(), size, gen_: g, data, index, map, live, dead_records, broken, compacting: false }))
    }

    /// Writes the headers of an empty generation `g` (each file whole, by rename) and opens it.
    fn create(dir: &Path, size: Size, g: u32) -> Result<Pack> {
        for (part, magic) in [(Part::Pack, PACK_MAGIC), (Part::Index, INDEX_MAGIC)] {
            let mut t = TempFile::create(&dir.join(format::file_name(size, g, part)))?;
            t.write(&format::header(magic, size))?;
            t.commit()?;
        }
        Pack::open_gen(dir, size, g)?.ok_or_else(|| Error::Damaged(dir.join(format::file_name(size, g, Part::Index))))
    }

    /// Reloads from disk if a write failed part-way.
    pub fn ready(&mut self) -> Result<()> {
        if self.broken {
            let fresh = Pack::load(&self.dir, self.size)?;
            *self = fresh;
        }
        Ok(())
    }

    /// The entry's bytes, checked against its CRC the first time this process reads them. A
    /// mismatch or missing bytes drop the entry (`Ok(None)`).
    pub fn get(&mut self, key: Key) -> Result<Option<Vec<u8>>> {
        let Some(loc) = self.map.get(&key).copied() else { return Ok(None) };
        let bytes = match self.data.read_vec(loc.offset, u64::from(loc.len), MAX_ENTRY) {
            Ok(b) => b,
            Err(dalica_fs::Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.forget(key);
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        if !loc.verified {
            if crc32(&bytes) != loc.crc {
                self.forget(key);
                return Ok(None);
            }
            if let Some(l) = self.map.get_mut(&key) {
                l.verified = true;
            }
        }
        Ok(Some(bytes))
    }

    fn forget(&mut self, key: Key) {
        if let Some(old) = self.map.remove(&key) {
            self.live = self.live.saturating_sub(u64::from(old.len));
            self.dead_records = self.dead_records.saturating_add(1);
        }
    }

    pub fn put(&mut self, key: Key, bytes: &[u8]) -> Result<()> {
        let len = u32::try_from(bytes.len())
            .ok()
            .filter(|&n| u64::from(n) <= MAX_ENTRY)
            .ok_or(Error::TooLarge(u64::try_from(bytes.len()).unwrap_or(u64::MAX)))?;
        self.ready()?;
        let crc = crc32(bytes);
        if self.map.get(&key).is_some_and(|l| l.len == len && l.crc == crc) {
            return Ok(());
        }
        let offset = self.data.append(bytes).inspect_err(|_| self.broken = true)?;
        let rec = Record::Put { key, offset, len, crc }.encode();
        self.index.append(&rec).inspect_err(|_| self.broken = true)?;
        self.forget(key);
        self.map.insert(key, Loc { offset, len, crc, verified: true });
        self.live = self.live.saturating_add(u64::from(len));
        Ok(())
    }

    /// Marks the entry dead; `Ok(false)` when there was none.
    pub fn remove(&mut self, key: Key) -> Result<bool> {
        self.ready()?;
        if !self.map.contains_key(&key) {
            return Ok(false);
        }
        // Gone from memory either way: a lost removal record only brings back an entry nobody asks
        // for (keys are content-addressed).
        self.forget(key);
        self.index.append(&Record::Remove { key }.encode()).inspect_err(|_| self.broken = true)?;
        // The removal record itself describes no live entry either.
        self.dead_records = self.dead_records.saturating_add(1);
        Ok(true)
    }

    pub fn sync(&mut self) -> Result<()> {
        self.data.sync()?;
        self.index.sync()?;
        Ok(())
    }

    /// Pack bytes no live entry uses.
    pub fn dead_bytes(&self) -> u64 {
        self.data.len().saturating_sub(HEADER_LEN).saturating_sub(self.live)
    }

    pub fn disk_bytes(&self) -> u64 {
        self.data.len().saturating_add(self.index.len())
    }

    /// What this pack would take on disk after a compaction.
    pub fn compacted_bytes(&self) -> u64 {
        let n = u64::try_from(self.map.len()).unwrap_or(u64::MAX);
        EMPTY_BYTES.saturating_add(self.live).saturating_add(n.saturating_mul(RECORD_LEN))
    }

    /// Drops every entry: the files of this generation go, an empty next generation replaces them.
    pub fn clear(&mut self) -> Result<()> {
        let next = self.gen_.checked_add(1).ok_or_else(|| gen_exhausted(&self.dir))?;
        let fresh = Pack::create(&self.dir, self.size, next)?;
        let old = std::mem::replace(self, fresh);
        old.remove_files();
        Ok(())
    }

    /// Removes this generation's files (best effort: what is left is removed by the next open).
    pub fn remove_files(self) {
        let (dir, size, g) = (self.dir.clone(), self.size, self.gen_);
        drop(self);
        for part in [Part::Pack, Part::Index] {
            let _ = remove_shaped(&dir.join(format::file_name(size, g, part)), own);
        }
    }

    /// The live entries, oldest first.
    pub fn entries_by_offset(&self) -> Vec<(Key, Loc)> {
        let mut v: Vec<(Key, Loc)> = self.map.iter().map(|(k, l)| (*k, *l)).collect();
        v.sort_by_key(|(_, l)| l.offset);
        v
    }
}

fn gen_exhausted(dir: &Path) -> Error {
    Error::Damaged(dir.to_path_buf())
}

/// A compaction's new generation, written beside the current one.
pub(crate) struct Rewrite {
    pub gen_: u32,
    pack: TempFile,
    /// Where each copied entry now lives, by its old offset.
    moved: HashMap<Key, (u64, Loc)>,
}

impl Rewrite {
    /// Starts generation `g` and copies `entries` into it from `reader` (a second handle on the
    /// current pack, so puts and gets go on meanwhile). Entries whose bytes fail their CRC are left out.
    pub fn start(dir: &Path, size: Size, g: u32, reader: &PackFile, entries: &[(Key, Loc)]) -> Result<Rewrite> {
        let mut pack = TempFile::create(&dir.join(format::file_name(size, g, Part::Pack)))?;
        pack.write(&format::header(PACK_MAGIC, size))?;
        let mut rw = Rewrite { gen_: g, pack, moved: HashMap::with_capacity(entries.len()) };
        for (key, loc) in entries {
            if let Some(new) = rw.copy(reader, *loc)? {
                rw.moved.insert(*key, (loc.offset, new));
            }
        }
        Ok(rw)
    }

    fn copy(&mut self, from: &PackFile, loc: Loc) -> Result<Option<Loc>> {
        let bytes = from.read_vec(loc.offset, u64::from(loc.len), MAX_ENTRY)?;
        if crc32(&bytes) != loc.crc {
            return Ok(None);
        }
        let offset = self.pack.write(&bytes)?;
        Ok(Some(Loc { offset, len: loc.len, crc: loc.crc, verified: true }))
    }

    /// Brings the rewrite up to date with `pack` (entries put since the copy began are copied now),
    /// commits the new pack and then its index, and swaps them in. The old generation's files go.
    pub fn finish(mut self, pack: &mut Pack) -> Result<()> {
        let mut map = HashMap::with_capacity(pack.map.len());
        for (key, loc) in pack.entries_by_offset() {
            let new = match self.moved.get(&key) {
                Some((old_offset, new)) if *old_offset == loc.offset => Some(*new),
                _ => self.copy(&pack.data, loc)?,
            };
            if let Some(new) = new {
                map.insert(key, new);
            }
        }
        let (dir, size, g) = (pack.dir.clone(), pack.size, self.gen_);
        let mut index = TempFile::create(&dir.join(format::file_name(size, g, Part::Index)))?;
        let mut body = Vec::with_capacity(map.len().saturating_mul(RECORD_LEN as usize).saturating_add(16));
        body.extend_from_slice(&format::header(INDEX_MAGIC, size));
        let mut ordered: Vec<(&Key, &Loc)> = map.iter().collect();
        ordered.sort_by_key(|(_, l)| l.offset);
        for (key, loc) in ordered {
            body.extend_from_slice(&Record::Put { key: *key, offset: loc.offset, len: loc.len, crc: loc.crc }.encode());
        }
        index.write(&body)?;
        // The pack first: a generation counts only once its index exists.
        self.pack.commit()?;
        index.commit()?;
        match Pack::open_gen(&dir, size, g) {
            Ok(Some(mut fresh)) => {
                // Already verified while copying.
                fresh.map = map;
                let old = std::mem::replace(pack, fresh);
                old.remove_files();
                Ok(())
            }
            Ok(None) => {
                pack.broken = true;
                Err(Error::Damaged(dir.join(format::file_name(size, g, Part::Index))))
            }
            Err(e) => {
                pack.broken = true;
                Err(e)
            }
        }
    }
}
