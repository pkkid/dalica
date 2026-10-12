//! Cache upkeep (`architecture.md` §6.6) in temporary folders: pruning stops at its share of the
//! budget, takes the largest sizes and the oldest entries first, steps over the entries the caller
//! keeps, leaves every file that is not the cache's own byte for byte, and handles another
//! library's entries in a shared cache like any other entry; forgetting removes only what it is
//! told.

use std::fs;
use std::path::Path;

use dalica_thumbs::pack::{Cache, Key, Options, Size};
use dalica_thumbs::upkeep::{self, Budget};

fn key(library: &str, i: u64) -> Key {
    Key::new(Path::new(&format!("/{library}/IMG_{i:05}.JPG")), 1000 + i, 1_700_000_000_000_000_000 + i128::from(i), 0, 1)
}

fn bytes(i: u64, len: usize) -> Vec<u8> {
    let mut x = i.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// The pack's own options never prune: upkeep alone decides here.
fn open(dir: &Path) -> Cache {
    Cache::open(dir, Options { budget: u64::MAX, compact_min_bytes: u64::MAX, ..Options::default() }).unwrap()
}

/// Entry lengths per size: larger sizes, larger entries.
fn len(size: Size) -> usize {
    match size {
        Size::S128 => 1_000,
        Size::S256 => 3_000,
        Size::S512 => 8_000,
        Size::S1024 => 20_000,
        Size::Screen => 50_000,
    }
}

/// `n` photos of `library` with an entry at every size, oldest first.
fn fill(c: &Cache, library: &str, from: u64, n: u64) {
    for i in from..from + n {
        for size in Size::ALL {
            c.put(size, key(library, i), &bytes(i, len(size))).unwrap();
        }
    }
    c.sync().unwrap();
}

fn keep_nothing(_: Size, _: Key) -> bool {
    false
}

#[test]
fn within_its_budget_the_cache_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(dir.path());
    fill(&c, "a", 0, 10);
    let before = c.disk_bytes();
    let p = upkeep::prune(&c, &Budget { bytes: before, prune_to_percent: 80 }, &keep_nothing).unwrap();
    assert_eq!((p.removed, p.after), (0, before), "{p:?}");
    assert!(!upkeep::over(&c, &Budget { bytes: before, prune_to_percent: 80 }));
    assert!(upkeep::over(&c, &Budget { bytes: before - 1, prune_to_percent: 80 }));
}

#[test]
fn pruning_stops_at_80_percent_largest_sizes_and_oldest_entries_first() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(dir.path());
    fill(&c, "a", 0, 40);
    let full = c.disk_bytes();
    // The target lies halfway into the 1024 entries: the screen size goes whole, then the oldest
    // 1024 entries, and nothing smaller.
    let (screen, big) = (c.stats(Size::Screen).disk_bytes, c.stats(Size::S1024).disk_bytes);
    let budget = Budget { bytes: (full - screen - big / 2) * 100 / 80, prune_to_percent: 80 };
    assert!(full > budget.bytes);
    let p = upkeep::prune(&c, &budget, &keep_nothing).unwrap();
    assert_eq!(p.sizes, vec![Size::Screen, Size::S1024], "{p:?}");
    assert!(p.after <= budget.target(), "pruned to {} > target {}", p.after, budget.target());
    assert!(p.after + len(Size::S1024) as u64 + 40 > budget.target(), "stops at the target: {} ≪ {}", p.after, budget.target());
    assert_eq!(p.after, c.disk_bytes());
    assert_eq!(c.stats(Size::Screen).entries, 0);
    let left = c.entries(Size::S1024);
    assert!(!left.is_empty() && left.len() < 40);
    // The oldest went: what is left is the newest, in order.
    let first_kept = 40 - left.len() as u64;
    assert_eq!(left.iter().map(|(k, _)| *k).collect::<Vec<_>>(), (first_kept..40).map(|i| key("a", i)).collect::<Vec<_>>());
    for size in [Size::S512, Size::S256, Size::S128] {
        assert_eq!(c.stats(size).entries, 40, "{size:?} untouched");
    }
    // The dead bytes are really gone: every size is compacted.
    for size in Size::ALL {
        assert_eq!(c.stats(size).dead_bytes, 0, "{size:?}");
    }
    // Entries left still read back.
    assert_eq!(c.get(Size::S1024, key("a", 39)).unwrap().unwrap(), bytes(39, len(Size::S1024)));
}

#[test]
fn kept_entries_survive_pruning_even_when_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(dir.path());
    fill(&c, "a", 0, 30);
    // Photos 0..5 are offline: their entries are the oldest of every size.
    let offline: Vec<Key> = (0..5).map(|i| key("a", i)).collect();
    let keep = |_: Size, k: Key| offline.contains(&k);
    let budget = Budget { bytes: c.disk_bytes() / 2, prune_to_percent: 80 };
    let p = upkeep::prune(&c, &budget, &keep).unwrap();
    assert!(p.after <= budget.target(), "{p:?}");
    assert!(p.kept >= 5, "{p:?}");
    for size in Size::ALL {
        for k in &offline {
            assert!(c.contains(size, *k), "{size:?}: an offline photo's entry was pruned");
        }
    }
    assert!(!c.contains(Size::Screen, key("a", 5)), "the oldest online entry went");
}

#[test]
fn when_only_kept_entries_are_left_the_cache_stays_above_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(dir.path());
    fill(&c, "a", 0, 10);
    let budget = Budget { bytes: 10_000, prune_to_percent: 80 };
    let p = upkeep::prune(&c, &budget, &|_, _| true).unwrap();
    assert_eq!(p.removed, 0);
    assert_eq!(p.kept, 50, "every entry was looked at and kept");
    assert!(p.after > budget.target());
    for size in Size::ALL {
        assert_eq!(c.stats(size).entries, 10);
    }
}

#[test]
fn pruning_only_ever_deletes_cache_entries() {
    let dir = tempfile::tempdir().unwrap();
    let c = open(dir.path());
    fill(&c, "a", 0, 20);
    // Things that are not the cache's own: a photo, a sidecar, a stray file, names close to the
    // pack's.
    let foreign = [
        ("IMG_0001.jpg", b"jpeg".to_vec()),
        (".dalica.json", b"{}".to_vec()),
        ("notes.txt", b"mine".to_vec()),
        ("128-00000001.pack.bak", b"not ours".to_vec()),
        ("screen-zz.idx", b"not ours".to_vec()),
    ];
    for (name, body) in &foreign {
        fs::write(dir.path().join(name), body).unwrap();
    }
    let p = upkeep::prune(&c, &Budget { bytes: 1, prune_to_percent: 80 }, &keep_nothing).unwrap();
    assert_eq!(p.removed, 100, "{p:?}");
    for (name, body) in &foreign {
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), *body, "{name} was touched");
    }
    for size in Size::ALL {
        assert_eq!(c.stats(size).entries, 0);
    }
}

/// A cache folder is shared by the libraries the user opens one after another. A library forgets
/// only the entries of its own photos; another library's entries are pruned like any other entry,
/// oldest first, and kept when they are newer.
#[test]
fn another_librarys_entries_are_never_forgotten_and_pruned_by_age_alone() {
    let dir = tempfile::tempdir().unwrap();
    {
        let c = open(dir.path());
        fill(&c, "other", 0, 10);
    }
    // Library "a" opens the same cache later.
    let c = open(dir.path());
    fill(&c, "a", 0, 10);
    // "a" removes all its photos: only their entries go.
    let mine: Vec<(Size, Key)> = (0..10).flat_map(|i| Size::ALL.map(|s| (s, key("a", i)))).collect();
    assert_eq!(upkeep::forget(&c, &mine).unwrap(), 50);
    assert_eq!(upkeep::forget(&c, &mine).unwrap(), 0, "already gone");
    for size in Size::ALL {
        assert_eq!(c.entries(size).iter().map(|(k, _)| *k).collect::<Vec<_>>(), (0..10).map(|i| key("other", i)).collect::<Vec<_>>());
    }
    // New entries of "a"; then pruning takes the other library's older entries first.
    fill(&c, "a", 100, 10);
    let budget = Budget { bytes: c.disk_bytes() / 2, prune_to_percent: 80 };
    upkeep::prune(&c, &budget, &keep_nothing).unwrap();
    let screen: Vec<Key> = c.entries(Size::Screen).into_iter().map(|(k, _)| k).collect();
    assert!(screen.iter().all(|k| (100..110).any(|i| key("a", i) == *k)), "the older library's entries went first");
}

#[test]
fn a_cache_reopened_after_pruning_holds_what_pruning_left() {
    let dir = tempfile::tempdir().unwrap();
    let left: Vec<Vec<(Key, u32)>> = {
        let c = open(dir.path());
        fill(&c, "a", 0, 30);
        let budget = Budget { bytes: c.disk_bytes() / 3, prune_to_percent: 80 };
        upkeep::prune(&c, &budget, &keep_nothing).unwrap();
        c.sync().unwrap();
        Size::ALL.map(|s| c.entries(s)).to_vec()
    };
    let c = open(dir.path());
    assert_eq!(Size::ALL.map(|s| c.entries(s)).to_vec(), left);
}
