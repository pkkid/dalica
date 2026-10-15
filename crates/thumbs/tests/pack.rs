//! The packed disk cache (`architecture.md` §6.6) in temporary folders: round trips at every size,
//! torn and damaged files, compaction, pruning, foreign files, `fs` fault injection on the pack
//! writes, hostile bytes, the folder lock and the UI-thread guard.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use dalica_fs::faults::{Fault, arm, disarm_all};
use dalica_fs::pack::{PACK_POINTS, points};
use dalica_thumbs::pack::{Cache, Error, Key, MAX_ENTRY, Options, Size, is_own_name};
use proptest::prelude::*;

fn key(i: u64) -> Key {
    Key::new(Path::new(&format!("/photos/IMG_{i:05}.JPG")), 1000 + i, 1_700_000_000_000_000_000 + i128::from(i), 0, 1)
}

/// Deterministic bytes that differ per key and version.
fn bytes(i: u64, version: u64, len: usize) -> Vec<u8> {
    let mut x = i.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ version.wrapping_mul(0xd1b5_4a32_d192_ed03) ^ 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn opts() -> Options {
    Options { budget: u64::MAX, compact_min_bytes: 0, ..Options::default() }
}

fn file(dir: &Path, size: Size, generation: u32, ext: &str) -> PathBuf {
    dir.join(format!("{}-{generation:08x}.{ext}", size.name()))
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn put_and_get_round_trip_at_every_size() {
    let dir = tempfile::tempdir().unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    for (s, size) in Size::ALL.into_iter().enumerate() {
        for i in 0..20u64 {
            c.put(size, key(i), &bytes(i, s as u64, 100 + i as usize * 37)).unwrap();
        }
    }
    c.put(Size::S128, key(99), b"").unwrap();
    let check = |c: &Cache| {
        for (s, size) in Size::ALL.into_iter().enumerate() {
            for i in 0..20u64 {
                assert!(c.contains(size, key(i)));
                assert_eq!(c.get(size, key(i)).unwrap().unwrap(), bytes(i, s as u64, 100 + i as usize * 37), "{size:?} {i}");
            }
            assert_eq!(c.get(size, key(1000)).unwrap(), None);
            assert_eq!(c.stats(size).entries, 20 + u64::from(size == Size::S128));
        }
        assert_eq!(c.get(Size::S128, key(99)).unwrap().unwrap(), b"");
    };
    check(&c);
    c.sync().unwrap();
    drop(c);
    let c = Cache::open(dir.path(), opts()).unwrap();
    check(&c);
    // One pack and one index per size, nothing else but the lock.
    let mut expected: Vec<String> =
        Size::ALL.into_iter().flat_map(|s| [format!("{}-00000001.idx", s.name()), format!("{}-00000001.pack", s.name())]).collect();
    expected.push("lock".into());
    expected.sort();
    assert_eq!(names(dir.path()), expected);

    // Replacing an entry makes the old bytes dead; removing one makes it dead too.
    let before = c.stats(Size::S256);
    c.put(Size::S256, key(3), b"replaced").unwrap();
    assert_eq!(c.get(Size::S256, key(3)).unwrap().unwrap(), b"replaced");
    // Putting the same bytes again writes nothing.
    let disk = c.stats(Size::S256).disk_bytes;
    c.put(Size::S256, key(3), b"replaced").unwrap();
    assert_eq!(c.stats(Size::S256).disk_bytes, disk);
    assert!(c.remove(Size::S256, key(4)).unwrap());
    assert!(!c.remove(Size::S256, key(4)).unwrap());
    c.remove_all(key(5)).unwrap();
    let after = c.stats(Size::S256);
    assert_eq!(after.entries, before.entries - 2);
    assert!(after.dead_bytes > before.dead_bytes && after.dead_records == 5, "{after:?}");
    drop(c);
    let c = Cache::open(dir.path(), opts()).unwrap();
    assert_eq!(c.get(Size::S256, key(3)).unwrap().unwrap(), b"replaced");
    assert_eq!(c.get(Size::S256, key(4)).unwrap(), None);
    assert!(Size::ALL.into_iter().all(|s| c.get(s, key(5)).unwrap().is_none()));
    assert_eq!(c.stats(Size::S256).dead_records, 5);
    // An entry over the cap is refused before anything is written.
    let disk = c.disk_bytes();
    assert!(matches!(c.put(Size::Screen, key(1), &vec![0; MAX_ENTRY as usize + 1]), Err(Error::TooLarge(_))));
    assert_eq!(c.disk_bytes(), disk);
}

#[test]
fn keys_are_stable_and_change_with_every_part() {
    let p = Path::new("/a/b.jpg");
    let k = Key::new(p, 1, 2, 3, 4);
    assert_eq!(k, Key::new(p, 1, 2, 3, 4));
    // Pinned: the key must not change between runs, platforms or compiler versions.
    assert_eq!(format!("{:032x}", Key::new(Path::new(""), 0, 0, 0, 0).0), "6fdd964bc6f0c7cdce1ebf786e1dffcd");
    for other in [
        Key::new(Path::new("/a/c.jpg"), 1, 2, 3, 4),
        Key::new(p, 9, 2, 3, 4),
        Key::new(p, 1, 9, 3, 4),
        Key::new(p, 1, 2, 9, 4),
        Key::new(p, 1, 2, 3, 9),
    ] {
        assert_ne!(k, other);
    }
}

#[test]
fn reopening_after_a_torn_index_tail_loses_only_the_torn_entry() {
    let dir = tempfile::tempdir().unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    for i in 0..10 {
        c.put(Size::S256, key(i), &bytes(i, 0, 500)).unwrap();
    }
    c.sync().unwrap();
    drop(c);
    let idx = file(dir.path(), Size::S256, 1, "idx");
    let len = fs::metadata(&idx).unwrap().len();
    assert_eq!(len, 16 + 10 * 40);
    // The last record was being written when the power went.
    fs::OpenOptions::new().write(true).open(&idx).unwrap().set_len(len - 13).unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    for i in 0..9 {
        assert_eq!(c.get(Size::S256, key(i)).unwrap().unwrap(), bytes(i, 0, 500), "{i}");
    }
    assert_eq!(c.get(Size::S256, key(9)).unwrap(), None);
    // The torn tail was cut, so new records line up again.
    assert_eq!(fs::metadata(&idx).unwrap().len(), 16 + 9 * 40);
    c.put(Size::S256, key(9), &bytes(9, 1, 500)).unwrap();
    c.put(Size::S256, key(10), &bytes(10, 1, 500)).unwrap();
    drop(c);
    let c = Cache::open(dir.path(), opts()).unwrap();
    assert_eq!(c.get(Size::S256, key(9)).unwrap().unwrap(), bytes(9, 1, 500));
    assert_eq!(c.get(Size::S256, key(10)).unwrap().unwrap(), bytes(10, 1, 500));
    assert_eq!(c.stats(Size::S256).entries, 11);
    drop(c);

    // A damaged record in the middle costs only its own entry.
    let mut raw = fs::read(&idx).unwrap();
    raw[16 + 4 * 40 + 20] ^= 0x40;
    fs::write(&idx, &raw).unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    for i in 0..11 {
        assert_eq!(c.get(Size::S256, key(i)).unwrap().is_some(), i != 4, "{i}");
    }
    drop(c);

    // Damaged entry bytes are a miss, not wrong pixels; bytes cut off the pack end lose only theirs.
    let pack = file(dir.path(), Size::S256, 1, "pack");
    let mut raw = fs::read(&pack).unwrap();
    raw[16 + 2 * 500 + 7] ^= 1;
    let n = raw.len();
    raw.truncate(n - 200);
    fs::write(&pack, &raw).unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    assert_eq!(c.get(Size::S256, key(2)).unwrap(), None);
    assert_eq!(c.get(Size::S256, key(10)).unwrap(), None);
    for i in [0, 1, 3, 5, 6, 7, 8, 9] {
        assert!(c.get(Size::S256, key(i)).unwrap().is_some(), "{i}");
    }
}

#[test]
fn compaction_keeps_every_live_entry_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    let mut model = HashMap::new();
    for i in 0..200u64 {
        let b = bytes(i, 0, 300 + (i as usize % 7) * 50);
        c.put(Size::S512, key(i), &b).unwrap();
        model.insert(i, b);
    }
    c.put(Size::S128, key(1), b"other size").unwrap();
    assert!(!c.compaction_due(Size::S512));
    for i in (0..200u64).step_by(3) {
        let b = bytes(i, 1, 400);
        c.put(Size::S512, key(i), &b).unwrap();
        model.insert(i, b);
    }
    for i in (1..200u64).step_by(5) {
        c.remove(Size::S512, key(i)).unwrap();
        model.remove(&i);
    }
    let before = c.stats(Size::S512);
    assert!(before.dead_bytes * 4 > before.disk_bytes, "{before:?}");
    assert!(c.compaction_due(Size::S512));
    assert!(!c.compaction_due(Size::S128));
    let m = c.maintain().unwrap();
    assert_eq!(m.compacted, [Size::S512]);
    assert!(m.pruned.is_empty() && m.after < m.before, "{m:?}");
    let after = c.stats(Size::S512);
    assert_eq!((after.entries, after.live_bytes, after.dead_bytes, after.dead_records), (before.entries, before.live_bytes, 0, 0));
    assert_eq!(after.generation, 2);
    assert_eq!(after.disk_bytes, 32 + after.live_bytes + after.entries * 40);
    let check = |c: &Cache| {
        for i in 0..200u64 {
            assert_eq!(c.get(Size::S512, key(i)).unwrap(), model.get(&i).cloned(), "{i}");
        }
        assert_eq!(c.get(Size::S128, key(1)).unwrap().unwrap(), b"other size");
    };
    check(&c);
    // The old generation is gone; the new one is all there is.
    assert!(!file(dir.path(), Size::S512, 1, "pack").exists() && !file(dir.path(), Size::S512, 1, "idx").exists());
    assert!(file(dir.path(), Size::S512, 2, "pack").exists() && file(dir.path(), Size::S512, 2, "idx").exists());
    drop(c);
    let c = Cache::open(dir.path(), opts()).unwrap();
    check(&c);
    assert_eq!(c.stats(Size::S512).generation, 2);
    assert!(!c.compaction_due(Size::S512));
    // Below the minimum size nothing is compacted, however dead.
    drop(c);
    let c = Cache::open(dir.path(), Options { compact_min_bytes: 1 << 30, ..opts() }).unwrap();
    for i in 0..200u64 {
        c.remove(Size::S512, key(i)).unwrap();
    }
    assert!(!c.compaction_due(Size::S512));
}

#[test]
fn a_generation_counts_only_once_its_index_exists() {
    let dir = tempfile::tempdir().unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    c.put(Size::S128, key(1), b"gen one").unwrap();
    drop(c);
    // A compaction committed its new pack and was killed before committing the index.
    fs::write(file(dir.path(), Size::S128, 2, "pack"), b"half a generation").unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    assert_eq!(c.get(Size::S128, key(1)).unwrap().unwrap(), b"gen one");
    assert_eq!(c.stats(Size::S128).generation, 1);
    assert!(!file(dir.path(), Size::S128, 2, "pack").exists());
    // Killed after both commits, before removing the old generation: the newer one wins.
    c.put(Size::S128, key(1), b"replaced").unwrap();
    assert!(c.compact(Size::S128).unwrap());
    drop(c);
    fs::write(file(dir.path(), Size::S128, 1, "pack"), b"stale").unwrap();
    fs::write(file(dir.path(), Size::S128, 1, "idx"), b"stale").unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    assert_eq!(c.stats(Size::S128).generation, 2);
    assert_eq!(c.get(Size::S128, key(1)).unwrap().unwrap(), b"replaced");
    assert!(!file(dir.path(), Size::S128, 1, "pack").exists() && !file(dir.path(), Size::S128, 1, "idx").exists());
    drop(c);
    // A header from another size (a renamed file) is not trusted: the size starts over, empty.
    fs::copy(file(dir.path(), Size::S256, 1, "idx"), file(dir.path(), Size::S128, 2, "idx")).unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    assert_eq!(c.get(Size::S128, key(1)).unwrap(), None);
    assert_eq!(c.stats(Size::S128).generation, 3);
    c.put(Size::S128, key(1), b"fresh").unwrap();
    assert_eq!(c.get(Size::S128, key(1)).unwrap().unwrap(), b"fresh");
}

#[test]
fn compaction_runs_while_other_threads_put_and_get() {
    let dir = tempfile::tempdir().unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    for i in 0..300u64 {
        c.put(Size::S256, key(i), &bytes(i, 0, 600)).unwrap();
    }
    std::thread::scope(|s| {
        let c = &c;
        s.spawn(move || {
            for round in 0..10u64 {
                for i in (round * 30)..(round * 30 + 30) {
                    c.put(Size::S256, key(i), &bytes(i, 1, 700)).unwrap();
                }
                c.put(Size::S256, key(1000 + round), &bytes(1000 + round, 0, 50)).unwrap();
            }
        });
        s.spawn(move || {
            for n in 0..3000u64 {
                let i = n * 7 % 300;
                let got = c.get(Size::S256, key(i)).unwrap().unwrap();
                assert!(got == bytes(i, 0, 600) || got == bytes(i, 1, 700), "{i}");
            }
        });
        s.spawn(move || {
            for _ in 0..8 {
                c.compact(Size::S256).unwrap();
            }
        });
    });
    c.compact(Size::S256).unwrap();
    let check = |c: &Cache| {
        for i in 0..300u64 {
            assert_eq!(c.get(Size::S256, key(i)).unwrap().unwrap(), bytes(i, 1, 700), "{i}");
        }
        for r in 0..10u64 {
            assert_eq!(c.get(Size::S256, key(1000 + r)).unwrap().unwrap(), bytes(1000 + r, 0, 50));
        }
    };
    check(&c);
    assert_eq!(c.stats(Size::S256).dead_bytes, 0);
    drop(c);
    check(&Cache::open(dir.path(), opts()).unwrap());
}

/// Files that are not the cache's: they must come through everything untouched.
fn plant_foreign(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let names = ["notes.txt", "128-00000001.pack.bak", "256.pack", "screen-zzzzzzzz.idx", "512-1.idx", ".hidden", "1024-00000001.jpg"];
    let mut planted = Vec::new();
    for n in names {
        let p = dir.join(n);
        fs::write(&p, n.as_bytes()).unwrap();
        planted.push((p, n.as_bytes().to_vec()));
    }
    // A folder with an own name and a file inside it.
    fs::create_dir_all(dir.join("128-0000ffff.pack")).unwrap();
    let inner = dir.join("128-0000ffff.pack").join("256-00000001.pack");
    fs::write(&inner, b"inside").unwrap();
    planted.push((inner, b"inside".to_vec()));
    #[cfg(unix)]
    {
        // A link with an own name, pointing at a foreign file.
        std::os::unix::fs::symlink(dir.join("notes.txt"), dir.join("1024-0000fffe.idx")).unwrap();
    }
    assert!(!names.iter().any(|n| is_own_name(n)));
    planted
}

fn assert_untouched(planted: &[(PathBuf, Vec<u8>)]) {
    for (p, b) in planted {
        assert_eq!(&fs::read(p).unwrap(), b, "{}", p.display());
    }
    #[cfg(unix)]
    {
        let link = planted[0].0.with_file_name("1024-0000fffe.idx");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }
}

#[test]
fn pruning_stops_at_80_percent_takes_the_largest_sizes_first_and_never_touches_a_foreign_file() {
    let dir = tempfile::tempdir().unwrap();
    let planted = plant_foreign(dir.path());
    let budget = 400_000;
    let c = Cache::open(dir.path(), Options { budget, ..opts() }).unwrap();
    let fill = [(Size::Screen, 20u64, 5000usize), (Size::S1024, 30, 4000), (Size::S512, 40, 3000), (Size::S256, 50, 1000), (Size::S128, 60, 500)];
    for (size, n, len) in fill {
        for i in 0..n {
            c.put(size, key(i), &bytes(i, len as u64, len)).unwrap();
        }
    }
    let before: Vec<_> = Size::ALL.into_iter().map(|s| c.stats(s)).collect();
    let total = c.disk_bytes();
    assert!(total > budget, "{total}");
    let target = budget * 80 / 100;
    let m = c.maintain().unwrap();
    assert_eq!(m.pruned, [Size::Screen, Size::S1024], "{m:?}");
    let after = c.disk_bytes();
    // Stops at 80 %: at most one entry (and its record) below it.
    assert!(after <= target && after > target - 4040, "{after} vs {target}");
    assert_eq!(c.stats(Size::Screen).entries, 0);
    // The oldest 1024 entries went, the newest stayed.
    let kept: Vec<u64> = (0..30).filter(|&i| c.contains(Size::S1024, key(i))).collect();
    assert!(!kept.is_empty() && kept.len() < 30);
    assert_eq!(kept, ((30 - kept.len() as u64)..30).collect::<Vec<_>>());
    for &i in &kept {
        assert_eq!(c.get(Size::S1024, key(i)).unwrap().unwrap(), bytes(i, 4000, 4000));
    }
    // The smaller sizes were not touched.
    for (k, size) in Size::ALL.into_iter().enumerate().take(3) {
        assert_eq!(c.stats(size), before[k], "{size:?}");
    }
    assert_untouched(&planted);
    // Under the budget, pruning does nothing.
    assert!(c.prune().unwrap().is_empty());
    drop(c);
    let c = Cache::open(dir.path(), Options { budget, ..opts() }).unwrap();
    assert_eq!(c.disk_bytes(), after);
    // Clearing everything (a budget of 0) still leaves foreign files alone.
    drop(c);
    let c = Cache::open(dir.path(), Options { budget: 0, ..opts() }).unwrap();
    let m = c.maintain().unwrap();
    assert_eq!(m.pruned, [Size::S1024, Size::S512, Size::S256, Size::S128]);
    assert!(Size::ALL.into_iter().all(|s| c.stats(s).entries == 0));
    assert_untouched(&planted);
    // Only own-shape files (and the lock) besides the planted ones.
    for n in names(dir.path()) {
        assert!(
            is_own_name(&n)
                || n == "lock"
                || planted.iter().any(|(p, _)| p.file_name().unwrap() == n.as_str())
                || n == "128-0000ffff.pack"
                || n == "1024-0000fffe.idx",
            "{n}"
        );
    }
}

#[derive(Default)]
struct Model {
    /// Entries that must be there with these bytes.
    sure: HashMap<(Size, Key), Vec<u8>>,
    /// Entries that may be there (an operation on them failed part-way); if there, with these bytes.
    maybe: HashMap<(Size, Key), Vec<Vec<u8>>>,
}

impl Model {
    fn check(&self, c: &Cache, sure_present: bool) {
        for ((size, key), want) in &self.sure {
            match c.get(*size, *key).unwrap() {
                Some(got) => assert_eq!(&got, want, "{size:?} {key:?}"),
                None => assert!(!sure_present, "{size:?} {key:?} lost"),
            }
        }
        for ((size, key), allowed) in &self.maybe {
            if let Some(got) = c.get(*size, *key).unwrap() {
                assert!(allowed.contains(&got), "{size:?} {key:?}: wrong bytes");
            }
        }
    }

    fn unsure(&mut self, size: Size, key: Key, new: Option<Vec<u8>>) {
        let mut allowed: Vec<Vec<u8>> = self.sure.remove(&(size, key)).into_iter().collect();
        allowed.extend(new);
        self.maybe.entry((size, key)).or_default().extend(allowed);
    }
}

/// The operations after a restart, stopping at the first failure as a killed process would.
fn run(dir: &Path, model: &mut Model, live: &mut Option<Cache>) -> Result<(), Error> {
    let c = live.insert(Cache::open(dir, opts())?);
    for (size, key) in model.sure.keys().copied().collect::<Vec<_>>() {
        c.get(size, key)?;
    }
    let fresh = bytes(500, 0, 800);
    model.unsure(Size::S128, key(500), Some(fresh.clone()));
    c.put(Size::S128, key(500), &fresh)?;
    model.unsure(Size::S512, key(0), None);
    c.remove(Size::S512, key(0))?;
    c.sync()?;
    c.maintain()?;
    Ok(())
}

/// One fault of each kind the cache handles differently (`fs`'s own tests fail and crash every pack
/// point): an error while the cache runs (it keeps serving), a crash while compaction commits its
/// new generation, and an error in the best-effort cleanup at open.
#[test]
fn fs_fault_injection_on_every_pack_write_leaves_a_readable_cache() {
    assert!([points::APPEND, points::TEMP_RENAME, points::TRUNCATE].iter().all(|p| PACK_POINTS.contains(p)));
    for (point, fault) in [(points::APPEND, Fault::Error), (points::TEMP_RENAME, Fault::Crash), (points::TRUNCATE, Fault::Error)] {
        {
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path();
            let mut model = Model::default();
            {
                let c = Cache::open(d, opts()).unwrap();
                for i in 0..30u64 {
                    c.put(Size::S128, key(i), &bytes(i, 0, 300)).unwrap();
                }
                for i in 0..10u64 {
                    c.put(Size::S512, key(i), &bytes(i, 9, 900)).unwrap();
                    model.sure.insert((Size::S512, key(i)), bytes(i, 9, 900));
                }
                // Dead entries, so maintenance compacts the 128 pack.
                for i in 0..30u64 {
                    let b = if i < 15 { bytes(i, 1, 300) } else { bytes(i, 0, 300) };
                    c.put(Size::S128, key(i), &b).unwrap();
                    model.sure.insert((Size::S128, key(i)), b);
                }
                assert!(c.compaction_due(Size::S128));
                c.sync().unwrap();
            }
            // A torn index tail and a stray temporary file, so opening cuts one and removes the other.
            let idx = file(d, Size::S128, 1, "idx");
            let mut f = fs::OpenOptions::new().append(true).open(&idx).unwrap();
            std::io::Write::write_all(&mut f, b"torn!!!").unwrap();
            drop(f);
            let stray = d.join(".128-00000001.pack.dalica-tmp-0123456789abcdef");
            fs::write(&stray, b"left by a crash").unwrap();

            arm(point, fault);
            let mut live = None;
            let result = run(d, &mut model, &mut live);
            disarm_all();
            // The fault fired: the run failed at it, or (cleanup at open is best effort) its step was skipped.
            match point {
                points::REMOVE => assert!(result.is_ok() && stray.exists(), "{fault:?}: {result:?}"),
                points::TRUNCATE => assert!(result.is_ok(), "{fault:?}: {result:?}"),
                _ if fault == Fault::Crash => {
                    assert!(matches!(result, Err(Error::Fs(dalica_fs::Error::Crashed(p))) if p == point), "{point}: {result:?}")
                }
                _ => assert!(matches!(result, Err(Error::Fs(dalica_fs::Error::Io { .. }))), "{point}: {result:?}"),
            }
            // After an error (not a crash) the running cache keeps serving.
            if let (Fault::Error, Some(c)) = (fault, &live) {
                model.check(c, false);
                c.put(Size::S256, key(1), b"still writable").unwrap();
                assert_eq!(c.get(Size::S256, key(1)).unwrap().unwrap(), b"still writable");
                model.unsure(Size::S256, key(1), Some(b"still writable".to_vec()));
            }
            drop(live);
            // After a restart: nothing that was there is lost, nothing reads back wrong.
            let c = Cache::open(d, opts()).unwrap();
            model.check(&c, true);
            c.put(Size::S1024, key(7), b"after").unwrap();
            assert_eq!(c.get(Size::S1024, key(7)).unwrap().unwrap(), b"after", "{point} {fault:?}");
            // Leftovers of the failed step are gone; only whole generations remain.
            for n in names(d) {
                assert!(n == "lock" || (is_own_name(&n) && !n.starts_with('.')), "{point} {fault:?}: {n}");
            }
            assert_eq!(names(d).len(), 11, "{point} {fault:?}: {:?}", names(d));
        }
    }
}

proptest! {
    #![proptest_config(crate::common::cases(64, 16))]

    /// Any bytes in a pack or index open as a working cache: never a panic, never an error.
    #[test]
    fn hostile_files_open_as_a_working_cache(idx in proptest::collection::vec(any::<u8>(), 0..400), pack in proptest::collection::vec(any::<u8>(), 0..400), keep_headers in any::<bool>()) {
        let dir = tempfile::tempdir().unwrap();
        drop(Cache::open(dir.path(), opts()).unwrap());
        let (ip, pp) = (file(dir.path(), Size::S128, 1, "idx"), file(dir.path(), Size::S128, 1, "pack"));
        let mut i = if keep_headers { fs::read(&ip).unwrap() } else { Vec::new() };
        let mut p = if keep_headers { fs::read(&pp).unwrap() } else { Vec::new() };
        i.extend_from_slice(&idx);
        p.extend_from_slice(&pack);
        fs::write(&ip, &i).unwrap();
        fs::write(&pp, &p).unwrap();
        let c = Cache::open(dir.path(), opts()).unwrap();
        for k in 0..20 {
            let _ = c.get(Size::S128, key(k)).unwrap();
        }
        c.put(Size::S128, key(3), b"fine").unwrap();
        prop_assert_eq!(c.get(Size::S128, key(3)).unwrap().unwrap(), b"fine".to_vec());
        c.maintain().unwrap();
        drop(c);
        let c = Cache::open(dir.path(), opts()).unwrap();
        prop_assert_eq!(c.get(Size::S128, key(3)).unwrap().unwrap(), b"fine".to_vec());
    }
}

#[test]
fn one_process_at_a_time_and_never_on_the_ui_thread() {
    let dir = tempfile::tempdir().unwrap();
    let c = Cache::open(dir.path(), opts()).unwrap();
    assert!(matches!(Cache::open(dir.path(), opts()), Err(Error::Fs(dalica_fs::Error::Locked(_)))));
    c.put(Size::S128, key(1), b"x").unwrap();
    let ui = dalica_fs::mark_ui_thread();
    assert!(matches!(c.put(Size::S128, key(2), b"y"), Err(Error::Fs(dalica_fs::Error::OnUiThread))));
    assert!(matches!(c.get(Size::S128, key(1)), Err(Error::Fs(dalica_fs::Error::OnUiThread))));
    assert!(matches!(c.sync(), Err(Error::Fs(dalica_fs::Error::OnUiThread))));
    assert!(matches!(Cache::open(&dir.path().join("other"), opts()), Err(Error::Fs(dalica_fs::Error::OnUiThread))));
    // What the cache knows in memory needs no disk.
    assert!(c.contains(Size::S128, key(1)));
    drop(ui);
    assert_eq!(c.get(Size::S128, key(1)).unwrap().unwrap(), b"x");
    assert_eq!(c.get(Size::S128, key(2)).unwrap(), None);
}
