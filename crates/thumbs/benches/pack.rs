//! Reading thumbnails from the packed cache against the same bytes as loose files, one per
//! thumbnail (`architecture.md` §6.6). Run it with
//!
//! ```sh
//! cargo bench -p dalica-thumbs --bench pack                 # 100 000 entries of ~6 KB
//! cargo bench -p dalica-thumbs --bench pack -- --count 20000 --bytes 15000 --json
//! ```
//!
//! Loose files are written without an fsync each (the flattering case for them); the pack is
//! synced once at the end. Both stores live under the target directory's temporary folder (on disk, not tmpfs) and are
//! removed afterwards. Reads are warm (the OS page cache holds the files after writing them):
//! dropping the page cache needs root, so cold reads are not measured here. Under `cargo test`
//! (no `--bench` argument) it runs a 300-entry smoke pass.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_thumbs::pack::{Cache, Key, Options, Size};

struct Args {
    count: usize,
    bytes: usize,
    json: bool,
}

fn args() -> Args {
    let all: Vec<String> = std::env::args().collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args { count: if full { 100_000 } else { 300 }, bytes: 6_000, json: false };
    let mut it = all.iter().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--count" => a.count = it.next().and_then(|v| v.parse().ok()).unwrap_or(a.count),
            "--bytes" => a.bytes = it.next().and_then(|v| v.parse().ok()).unwrap_or(a.bytes),
            "--json" => a.json = true,
            _ => {}
        }
    }
    a
}

/// xorshift64*: deterministic payloads without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

fn payloads(n: usize, avg: usize) -> Vec<(Key, Vec<u8>)> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    (0..n)
        .map(|i| {
            let key = Key::new(
                Path::new(&format!("/photos/{:03}/IMG_{i:06}.JPG", i / 500)),
                3_000_000 + i as u64,
                1_700_000_000_000_000_000 + i as i128,
                0,
                1,
            );
            let len = avg / 2 + (rng.next() as usize % avg.max(1));
            let mut bytes = Vec::with_capacity(len);
            while bytes.len() < len {
                bytes.extend_from_slice(&rng.next().to_le_bytes());
            }
            bytes.truncate(len);
            (key, bytes)
        })
        .collect()
}

fn loose_path(root: &Path, key: Key) -> PathBuf {
    let hex = format!("{:032x}", key.0);
    root.join(&hex[..2]).join(format!("{hex}.jpg"))
}

#[cfg(unix)]
fn allocated(m: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks() * 512
}

#[cfg(not(unix))]
fn allocated(m: &std::fs::Metadata) -> u64 {
    m.len()
}

fn disk_usage(dir: &Path) -> (u64, u64) {
    let (mut files, mut bytes) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            } else {
                files += 1;
                bytes += allocated(&m);
            }
        }
    }
    (files, bytes)
}

fn per(d: Duration, n: usize) -> f64 {
    d.as_secs_f64() * 1e6 / n.max(1) as f64
}

fn main() {
    let a = args();
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("pack-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (pack_dir, loose_dir) = (root.join("thumbs"), root.join("loose"));
    std::fs::create_dir_all(&loose_dir).unwrap();
    let data = payloads(a.count, a.bytes);
    let total: usize = data.iter().map(|(_, b)| b.len()).sum();
    let options = Options { budget: u64::MAX, ..Options::default() };

    // Write both stores.
    let t = Instant::now();
    let cache = Cache::open(&pack_dir, options).unwrap();
    for (key, bytes) in &data {
        cache.put(Size::S128, *key, bytes).unwrap();
    }
    cache.sync().unwrap();
    let pack_write = t.elapsed();
    drop(cache);
    let t = Instant::now();
    for (key, bytes) in &data {
        let p = loose_path(&loose_dir, *key);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, bytes).unwrap();
    }
    let loose_write = t.elapsed();

    // Open: the packed cache loads its index.
    let t = Instant::now();
    let cache = Cache::open(&pack_dir, options).unwrap();
    let pack_open = t.elapsed();

    // A shuffled order (a scroll that jumps), and the stored order (a scroll through one folder).
    let mut shuffled: Vec<usize> = (0..data.len()).collect();
    let mut rng = Rng(42);
    for i in (1..shuffled.len()).rev() {
        shuffled.swap(i, rng.next() as usize % (i + 1));
    }
    let in_order: Vec<usize> = (0..data.len()).collect();
    let mut rows = Vec::new();
    // The first pass also checks each entry's CRC (once per process); then the steady state.
    for (name, order) in [("read in order, first (CRC)", &in_order), ("read in order", &in_order), ("read shuffled", &shuffled)] {
        let t = Instant::now();
        let mut read = 0usize;
        for &i in order.iter() {
            let (key, bytes) = &data[i];
            let got = cache.get(Size::S128, *key).unwrap().unwrap();
            assert_eq!(got.len(), bytes.len());
            read += got.len();
        }
        let pack = t.elapsed();
        assert_eq!(read, total);
        let t = Instant::now();
        for &i in order.iter() {
            let (key, bytes) = &data[i];
            let got = std::fs::read(loose_path(&loose_dir, *key)).unwrap();
            assert_eq!(got.len(), bytes.len());
        }
        let loose = t.elapsed();
        rows.push((name.to_string(), pack, loose));
    }
    let (pack_files, pack_disk) = disk_usage(&pack_dir);
    let (loose_files, loose_disk) = disk_usage(&loose_dir);
    drop(cache);
    std::fs::remove_dir_all(&root).unwrap();

    let n = data.len();
    if a.json {
        let rows_json: Vec<String> = rows
            .iter()
            .map(|(name, p, l)| format!("{{\"name\":\"{name}\",\"pack_us_per_read\":{:.3},\"loose_us_per_read\":{:.3}}}", per(*p, n), per(*l, n)))
            .collect();
        println!(
            "{{\"entries\":{n},\"payload_bytes\":{total},\"pack_write_s\":{:.3},\"loose_write_s\":{:.3},\"pack_open_ms\":{:.3},\"pack_files\":{pack_files},\"pack_disk_bytes\":{pack_disk},\"loose_files\":{loose_files},\"loose_disk_bytes\":{loose_disk},\"rows\":[{}]}}",
            pack_write.as_secs_f64(),
            loose_write.as_secs_f64(),
            pack_open.as_secs_f64() * 1e3,
            rows_json.join(",")
        );
        return;
    }
    println!("{n} entries, {:.1} MB of payload (average {} bytes)", total as f64 / 1e6, total / n.max(1));
    println!("write + sync     pack {:>8.2} s     loose {:>8.2} s", pack_write.as_secs_f64(), loose_write.as_secs_f64());
    println!("open (index)     pack {:>8.2} ms", pack_open.as_secs_f64() * 1e3);
    for (name, p, l) in &rows {
        println!(
            "{name:<28} pack {:>7.2} µs/read  loose {:>7.2} µs/read  ({:.1}× faster)",
            per(*p, n),
            per(*l, n),
            l.as_secs_f64() / p.as_secs_f64().max(1e-9)
        );
    }
    println!(
        "on disk          pack {pack_files} files, {:.1} MB     loose {loose_files} files, {:.1} MB",
        pack_disk as f64 / 1e6,
        loose_disk as f64 / 1e6
    );
}
