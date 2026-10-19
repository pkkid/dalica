//! Legacy folders at scale (`development.md` §8, `data-model.md` §9, `roadmap.md` M14.22): a first
//! scan of 10 000 photos in 100 folders whose data is only in the previous organizer's sidecar,
//! against the same folders with `.dalica.json`, and a legacy sidecar of 64 MiB (the reader's
//! limit) read whole. Run it with
//!
//! ```sh
//! cargo xtask bench legacy_scan                                 # release, history
//! cargo bench -p dalica-scan --bench legacy_scan -- --json
//! ```
//!
//! **The folders.** Two trees of `--folders` (100) folders of `--per-folder` (100) photos each:
//! the same `scenes` JPEGs (640 × 427, the scan reads their headers only). In the first tree each
//! folder has a legacy sidecar (written here from the documented format, its name from
//! `dalica-sidecar`'s masked constant): a section per photo with a star on every third, a caption,
//! three keywords, an edit, a face naming a contact and an album, and a geotag; the folder's
//! section (description, date, place), its contacts and one album definition. In the second each
//! folder has a `.dalica.json` saying the same of each photo (star, caption, tags, edit, an unnamed
//! face, geotag) and of the folder; Dalica keeps album definitions and people's names outside the
//! folder's sidecar. `scan_with` (the scan's worker: the walk, every header read, each folder's
//! sidecar read and converted) over each tree with the default options, `--samples` (5) times,
//! interleaved: rows `legacy scan <n>` and `dalica scan <n>`.
//!
//! **The large file.** One folder of 100 photos with a legacy sidecar of exactly 64 MiB, the
//! reader's limit (`LegacyLimits::max_bytes`): the photos' sections, then sections for files that
//! are not there, each with a caption and keywords, padded to the last byte. Scanned
//! `--samples` times: row `legacy file 64 MiB`.
//!
//! Rows carry `p50_ms`, `p95_ms`, `max_ms` and their budget (`budget_ms`, on the median);
//! `cargo xtask budgets` judges them, `--check` makes the bench judge them itself (in release, at
//! full size), `--plant-ms N` adds N ms to each judged time. Without `--bench` (under
//! `cargo test --benches`) it runs a smoke pass: 4 folders of 25 photos, a 1 MiB file, one sample.
//! The files (about 0.2 GB) are written under the target directory and removed after.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_scan::legacy::FolderSidecar;
use dalica_scan::{Options, Scan, scan_with};
use dalica_scenes::{Scene, jpeg};
use dalica_sidecar::fields::GeoTag;
use dalica_sidecar::legacy::{LegacyLimits, folder_section, sidecar_names};
use dalica_sidecar::{Face, FacePerson, FaceRect, FolderValue, Geo, PhotoValue, Sidecar};
use serde_json::{Value, json};

/// The budgets (`development.md` §8): 1.5 × the first measurement (the median of three runs,
/// release on two hardware threads, 2026-10-19: the legacy-only scan 128.7 ms, the `.dalica.json`
/// scan 124.2 ms, the 64 MiB file 661.1 ms), rounded up to 5 ms.
const LEGACY_SCAN_BUDGET_MS: f64 = 195.0;
const DALICA_SCAN_BUDGET_MS: f64 = 190.0;
const LARGE_FILE_BUDGET_MS: f64 = 995.0;
/// The scan the budgets hold for: 100 folders of 100 photos.
const FOLDERS: usize = 100;
const PER_FOLDER: usize = 100;
/// The photos' size: the scan reads only their headers.
const PHOTO: (u32, u32) = (640, 427);
const ALBUM: &str = "5d2c0e0f1a2b3c4d5e6f708192a3b4c5";
const CONTACT: &str = "0632e71e2ffd6c6d";

struct Args {
    folders: usize,
    per_folder: usize,
    large_bytes: usize,
    samples: usize,
    json: bool,
    check: bool,
    plant_ms: f64,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let limit = LegacyLimits::default().max_bytes;
    let mut a = Args {
        folders: if full { FOLDERS } else { 4 },
        per_folder: if full { PER_FOLDER } else { 25 },
        large_bytes: if full { limit } else { 1 << 20 },
        samples: if full { 5 } else { 1 },
        json: false,
        check: false,
        plant_ms: 0.0,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--folders" => a.folders = value()?.parse().map_err(|e| format!("--folders: {e}"))?,
            "--per-folder" => a.per_folder = value()?.parse().map_err(|e| format!("--per-folder: {e}"))?,
            "--large-bytes" => a.large_bytes = value()?.parse().map_err(|e| format!("--large-bytes: {e}"))?,
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--plant-ms" => a.plant_ms = value()?.parse().map_err(|e| format!("--plant-ms: {e}"))?,
            "--json" => a.json = true,
            "--check" => a.check = true,
            "--bench" => {}
            other => {
                return Err(format!(
                    "unknown argument `{other}` (use --folders N, --per-folder N, --large-bytes N, --samples N, --check, --plant-ms N, --json)"
                ));
            }
        }
    }
    if !(1..=1000).contains(&a.folders) || !(1..=1000).contains(&a.per_folder) {
        return Err("--folders and --per-folder must be 1..=1000".into());
    }
    if !(1 << 16..=limit).contains(&a.large_bytes) {
        return Err(format!("--large-bytes must be 65536..={limit}"));
    }
    if !(1..=100).contains(&a.samples) || !(0.0..=100_000.0).contains(&a.plant_ms) {
        return Err("--samples must be 1..=100 and --plant-ms 0..=100000".into());
    }
    Ok(a)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn row(name: &str, times: &[Duration], extra: Value) -> Value {
    let mut t: Vec<f64> = times.iter().map(|d| ms(*d)).collect();
    t.sort_by(f64::total_cmp);
    let mut r =
        json!({ "name": name, "samples": t.len(), "p50_ms": rank(&t, 50), "p95_ms": rank(&t, 95), "max_ms": t.last().copied().unwrap_or(0.0) });
    if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    r
}

fn photo_name(i: usize) -> String {
    format!("IMG_{:04}.jpg", i + 1)
}

/// What every folder says of photo `i`.
struct Said {
    star: bool,
    caption: String,
    tags: [String; 3],
    lat: f64,
    lon: f64,
}

fn said(folder: usize, i: usize) -> Said {
    Said {
        star: i.is_multiple_of(3),
        caption: format!("Day {} by the harbour, photo {}", folder + 1, i + 1),
        tags: [format!("trip {}", folder % 7), "harbour".into(), format!("roll {}", i % 5)],
        lat: 40.0 + folder as f64 * 0.01,
        lon: -3.0 + i as f64 * 0.001,
    }
}

/// A legacy sidecar's lines for `folder`'s `n` photos (CRLF, UTF-8, as the legacy organizer wrote).
fn legacy_lines(folder: usize, n: usize) -> Vec<String> {
    let mut lines = vec!["[encoding]".to_string(), "utf8=1".into()];
    for i in 0..n {
        let s = said(folder, i);
        lines.push(format!("[{}]", photo_name(i)));
        if s.star {
            lines.push("star=yes".into());
        }
        lines.push(format!("caption={}", s.caption));
        lines.push(format!("keywords={}", s.tags.join(",")));
        lines.push("filters=bw=1;".into());
        lines.push(format!("faces=rect64(4000400080008000),{};", CONTACT.trim_start_matches('0')));
        lines.push(format!("albums={ALBUM}"));
        lines.push(format!("geotag={:.6},{:.6}", s.lat, s.lon));
    }
    lines.push(format!("[{}]", folder_section()));
    lines.push(format!("description=The harbour, day {}", folder + 1));
    lines.push("date=42078.5".into());
    lines.push("location=Lisbon".into());
    lines.push("[Contacts2]".into());
    lines.push(format!("{}=Zoë Ann;;", CONTACT.trim_start_matches('0')));
    lines.push(format!("[.album:{ALBUM}]"));
    lines.push("name=Harbour".into());
    lines.push(format!("token={ALBUM}"));
    lines
}

/// The `.dalica.json` saying of `folder`'s `n` photos what its legacy sidecar says.
fn dalica_file(folder: usize, n: usize) -> Vec<u8> {
    let mut s = Sidecar::new();
    for i in 0..n {
        let said = said(folder, i);
        let name = photo_name(i);
        let values = [
            PhotoValue::Star(said.star),
            PhotoValue::Caption(said.caption),
            PhotoValue::Tags(said.tags.to_vec()),
            PhotoValue::Edits(vec!["bw()".into()]),
        ];
        for v in &values {
            s.set_photo(&name, v).expect("a photo value");
        }
        let face = Face { rect: FaceRect::new(0.25, 0.25, 0.5, 0.5).expect("a face rectangle"), person: FacePerson::Unnamed };
        s.set_photo_faces(&name, &[face]).expect("the faces");
        s.set_geo(&name, Geo::new(said.lat, said.lon).map(GeoTag::At)).expect("the geotag");
    }
    for v in [FolderValue::Description(format!("The harbour, day {}", folder + 1)), FolderValue::Place("Lisbon".into())] {
        s.set_folder(&v).expect("a folder value");
    }
    s.print()
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("a folder");
    }
    std::fs::write(path, bytes).expect("a file");
}

/// The two trees under `root`: the photos with legacy sidecars, and with `.dalica.json`.
fn trees(root: &Path, a: &Args, photo: &[u8]) -> (PathBuf, PathBuf) {
    let (legacy, dalica) = (root.join("legacy"), root.join("dalica"));
    for f in 0..a.folders {
        let name = format!("{:03} Harbour", f + 1);
        for i in 0..a.per_folder {
            write(&legacy.join(&name).join(photo_name(i)), photo);
            write(&dalica.join(&name).join(photo_name(i)), photo);
        }
        write(&legacy.join(&name).join(&sidecar_names()[0]), (legacy_lines(f, a.per_folder).join("\r\n") + "\r\n").as_bytes());
        write(&dalica.join(&name).join(dalica_sidecar::FILE_NAME), &dalica_file(f, a.per_folder));
    }
    (legacy, dalica)
}

/// A folder of `per_folder` photos with a legacy sidecar of exactly `bytes` bytes. Returns the
/// folder and the sections the file has.
fn large(root: &Path, per_folder: usize, bytes: usize, photo: &[u8]) -> (PathBuf, usize) {
    let dir = root.join("large").join("Everything");
    for i in 0..per_folder {
        write(&dir.join(photo_name(i)), photo);
    }
    let mut text = String::with_capacity(bytes);
    for line in legacy_lines(0, per_folder) {
        text.push_str(&line);
        text.push_str("\r\n");
    }
    // Sections for files that are not there until the file is nearly full; the last line pads it.
    let mut k = 0usize;
    loop {
        let section = format!(
            "[GONE_{k:07}.jpg]\r\nstar=yes\r\ncaption=A photo that moved away long ago, number {k}\r\nkeywords=old,moved,roll {}\r\n",
            k % 97
        );
        if text.len() + section.len() + 64 > bytes {
            break;
        }
        text.push_str(&section);
        k += 1;
    }
    // The encoding's, the photos', the folder's, the contacts' and the album's sections.
    let sections = k + per_folder + 4;
    let pad = bytes.saturating_sub(text.len() + "pad=\r\n".len());
    text.push_str("pad=");
    text.push_str(&"x".repeat(pad));
    text.push_str("\r\n");
    assert_eq!(text.len(), bytes, "the legacy sidecar is exactly its size");
    write(&dir.join(&sidecar_names()[0]), text.as_bytes());
    (dir, sections)
}

fn scan(root: &Path) -> (Duration, Scan) {
    let t = Instant::now();
    let s = scan_with(&[root.to_path_buf()], &Options::default());
    (t.elapsed(), s)
}

/// Every folder of `scan` read from `sidecar`, with `photos` photos in all.
fn check(scan: &Scan, sidecar: FolderSidecar, photos: usize, stars: usize) {
    assert!(scan.folders.iter().all(|f| f.sidecar == sidecar), "every folder's data from {sidecar:?}: {:?}", scan.warnings);
    assert_eq!(scan.folders.iter().map(|f| f.photos.len()).sum::<usize>(), photos);
    assert_eq!(scan.folders.iter().flat_map(|f| &f.photos).filter(|p| p.star).count(), stars, "the stars read");
}

fn bench(a: &Args) -> Vec<Value> {
    let tmp = tempfile::Builder::new().prefix("legacy-scan-").tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a temporary folder");
    let photo = jpeg(&Scene::new(7, PHOTO.0, PHOTO.1), 85, None).expect("a scenes JPEG");
    let (legacy, dalica) = trees(tmp.path(), a, &photo);
    let (large_dir, sections) = large(tmp.path(), a.per_folder, a.large_bytes, &photo);
    let n = a.folders * a.per_folder;
    let stars = a.folders * a.per_folder.div_ceil(3);

    let (mut legacy_times, mut dalica_times, mut large_times) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..a.samples {
        let (t, s) = scan(&legacy);
        check(&s, FolderSidecar::Legacy, n, stars);
        legacy_times.push(t);
        let (t, s) = scan(&dalica);
        check(&s, FolderSidecar::Dalica, n, stars);
        dalica_times.push(t);
        let (t, s) = scan(&large_dir);
        check(&s, FolderSidecar::Legacy, a.per_folder, a.per_folder.div_ceil(3));
        large_times.push(t);
    }
    let mib = a.large_bytes as f64 / (1024.0 * 1024.0);
    vec![
        row(&format!("legacy scan {n}"), &legacy_times, json!({ "budget_ms": LEGACY_SCAN_BUDGET_MS, "photos": n, "folders": a.folders })),
        row(&format!("dalica scan {n}"), &dalica_times, json!({ "budget_ms": DALICA_SCAN_BUDGET_MS, "photos": n, "folders": a.folders })),
        row(
            &format!("legacy file {mib:.0} MiB"),
            &large_times,
            json!({ "budget_ms": LARGE_FILE_BUDGET_MS, "bytes": a.large_bytes, "sections": sections, "photos": a.per_folder }),
        ),
    ]
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("legacy_scan: {e}");
            std::process::exit(2);
        }
    };
    let rows = bench(&a);
    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let report = json!({
        "bench": "legacy_scan",
        "mode": mode,
        "threads": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "folders": a.folders,
        "photos": a.folders * a.per_folder,
        "large_bytes": a.large_bytes,
        "rows": rows,
    });
    if a.json {
        println!("{report}");
    } else {
        println!("{:<24} {:>8} {:>10} {:>10} {:>10}", "row", "samples", "p50 ms", "p95 ms", "max ms");
        for r in &rows {
            println!(
                "{:<24} {:>8} {:>10.2} {:>10.2} {:>10.2}",
                r["name"].as_str().unwrap_or(""),
                r["samples"].as_u64().unwrap_or(0),
                r["p50_ms"].as_f64().unwrap_or(0.0),
                r["p95_ms"].as_f64().unwrap_or(0.0),
                r["max_ms"].as_f64().unwrap_or(0.0),
            );
        }
    }
    let full = a.folders * a.per_folder >= FOLDERS * PER_FOLDER && a.large_bytes >= LegacyLimits::default().max_bytes;
    if a.check && mode == "release" && full {
        let mut missed = Vec::new();
        for r in &rows {
            let name = r["name"].as_str().unwrap_or("?");
            let Some(budget) = r["budget_ms"].as_f64() else { continue };
            let judged = r["p50_ms"].as_f64().unwrap_or(f64::NAN) + a.plant_ms;
            if judged.is_nan() || judged > budget {
                missed.push(format!("{name}: median {judged:.1} ms, over the {budget} ms budget"));
            }
        }
        for m in &missed {
            eprintln!("legacy_scan: over budget: {m}");
        }
        if !missed.is_empty() {
            std::process::exit(1);
        }
    }
}
