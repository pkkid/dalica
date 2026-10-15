//! A 200-photo export of a mixed tray (36 in a task's gate: two of each kind) (`roadmap.md` M8.24;
//! `creating.md` §2) through
//! `export.run` and its job, on temporary folders: JPEGs in all eight orientations, CMYK, PNG with
//! and without alpha, 16-bit TIFF, WebP, GIF, BMP, JPEG XL, PSD and raw previews, edited and
//! unedited, some with GPS, exported at 1600 pixels, Normal quality, with a watermark and the
//! location removed. Every photo is written, and every file decodes at the size its source, its
//! edits and the long side give, a JPEG within the Normal quality's bounds, the watermark in its
//! box (against the same export without it) and no GPS left anywhere; a Cancel half-way leaves only
//! complete files.
//!
//! The tray itself is [`tray`], which `benches/save_export.rs` shares for the timed export (always
//! the 200 photos).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, JobEntry, JobState, Options, Session, ThumbOptions};
use serde_json::{Value, json};

use crate::common::pick;

/// The tray: 200 photos written from `dalica-scenes`, in name order, with the edits each gets.
pub mod tray {
    use std::path::Path;

    use dalica_fs::Expect;
    use dalica_image::encode::{self, Image, Metadata};
    use dalica_image::{Limits, Rgba8};
    use dalica_meta::{Changes, Place, WriteOptions};
    use dalica_scenes::formats::{self, Colour, Compression, Depth, Psd, Tiff, Webp};
    use dalica_scenes::{Exif, Scene};

    /// Photos in the tray.
    pub const PHOTOS: usize = 200;
    /// The long side of the export.
    pub const LONG_SIDE: u32 = 1600;
    /// The watermark's text.
    pub const WATERMARK: &str = "© Dalica 2026";

    /// The kinds of file, in the order they repeat through the tray.
    pub const KINDS: [&str; 18] = [
        "jpeg-o1",
        "jpeg-o2",
        "jpeg-o3",
        "jpeg-o4",
        "jpeg-o5",
        "jpeg-o6",
        "jpeg-o7",
        "jpeg-o8",
        "tiff-cmyk",
        "png",
        "png-alpha",
        "tiff-16",
        "webp",
        "gif",
        "bmp",
        "jxl",
        "psd",
        "raf",
    ];

    /// One photo of the tray.
    #[derive(Clone, Debug)]
    pub struct Photo {
        pub name: String,
        pub kind: &'static str,
        /// Has transparency, so it exports as PNG.
        pub alpha: bool,
        /// Its file holds a place (EXIF GPS and XMP).
        pub gps: bool,
        /// Its size as shown (orientation applied).
        pub display: (u32, u32),
        /// The edits it gets (`photo.history.apply`), in order.
        pub edits: &'static [&'static str],
    }

    impl Photo {
        /// The name its export gets: its stem, `.png` with transparency, else `.jpg`.
        pub fn exported(&self) -> String {
            let stem = self.name.rsplit_once('.').map_or(self.name.as_str(), |(s, _)| s);
            format!("{stem}.{}", if self.alpha { "png" } else { "jpg" })
        }

        /// The size its export has at `long_side`: shown size, turned by its edits, fitted
        /// (never enlarged) as `image::resize::fit` does.
        pub fn expected(&self, long_side: u32) -> (u32, u32) {
            let quarters: u32 = self.edits.iter().map(|e| if *e == "rotate(90)" || *e == "rotate(270)" { 1 } else { 0 }).sum();
            let (w, h) = if quarters % 2 == 1 { (self.display.1, self.display.0) } else { self.display };
            let long = w.max(h);
            if long <= long_side {
                return (w, h);
            }
            let scale =
                |v: u32| ((u64::from(v) * u64::from(long_side) + u64::from(long) / 2) / u64::from(long)).clamp(1, u64::from(long_side)) as u32;
            (scale(w), scale(h))
        }
    }

    const TUNING: &str = "tuning(0.300,0.100,0.050,0.200,none)";

    /// The edits of photo `i`: five in eight are edited (125 of 200: turns, a Tuning, both).
    fn edits(i: usize) -> &'static [&'static str] {
        match i % 8 {
            1 | 5 => &["rotate(90)"],
            2 | 6 => &[TUNING],
            3 => &["rotate(270)", TUNING],
            _ => &[],
        }
    }

    /// The size of photo `i` at `scale` (1 for the test, larger for the bench); every 25th is a
    /// large one, so the export resizes it to [`LONG_SIDE`].
    fn size(i: usize, scale: u32) -> (u32, u32) {
        if i % 25 == 7 {
            return (2400, 1600);
        }
        let base = [(360, 240), (320, 400), (400, 300), (300, 300)][i % 4];
        (base.0 * scale, base.1 * scale)
    }

    fn rgba(img: &dalica_scenes::Rgb8, alpha: bool) -> Rgba8 {
        let w = img.width.max(2);
        let data = img
            .data
            .chunks(3)
            .enumerate()
            .flat_map(|(n, p)| {
                // A left-to-right alpha ramp from 40 (never fully clear) to 255.
                let x = (n as u32 % img.width) as u64;
                let a = if alpha { (40 + x * 215 / u64::from(w - 1)) as u8 } else { 255 };
                [p[0], p[1], p[2], a]
            })
            .collect();
        Rgba8 { width: img.width, height: img.height, data }
    }

    /// The camera EXIF of the JPEGs.
    fn exif(orientation: u8) -> Exif {
        Exif { orientation, make: Some("Dalicam".into()), model: Some("Mixed 200".into()), ..Exif::default() }
    }

    /// A RAF file around a camera JPEG (its preview), from the structure the RAF reader's
    /// provenance header describes: the magic, model, version, then the JPEG's offset and length.
    fn raf(jpeg: &[u8]) -> Vec<u8> {
        let mut b = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        let mut model = b"X-Dalica".to_vec();
        model.resize(32, 0);
        b.extend(model);
        b.extend(b"0100");
        b.extend([0u8; 20]);
        let at = 160u32;
        b.extend(at.to_be_bytes());
        b.extend((jpeg.len() as u32).to_be_bytes());
        b.extend([0u8; 16]);
        b.resize(at as usize, 0);
        b.extend(jpeg);
        b.extend([0x33u8; 200]);
        b
    }

    /// The bytes of photo `i` of `kind`, and its file extension.
    fn file(i: usize, kind: &str, scene: &Scene) -> (Vec<u8>, &'static str) {
        let jpeg = |o: u8| dalica_scenes::jpeg(scene, 90, Some(&exif(o))).unwrap();
        match kind {
            k if k.starts_with("jpeg-o") => (jpeg(k.as_bytes()[6] - b'0'), "jpg"),
            "tiff-cmyk" => (formats::tiff(scene, &Tiff { colour: Colour::Cmyk, compression: Compression::Lzw, ..Tiff::default() }).unwrap(), "tif"),
            "png" => (dalica_scenes::png(scene).unwrap(), "png"),
            "png-alpha" => (encode::png(&Image::rgba8(&rgba(&scene.render().unwrap(), true)), &Metadata::default()).unwrap(), "png"),
            "tiff-16" => {
                (formats::tiff(scene, &Tiff { depth: Depth::Sixteen, compression: Compression::Deflate, ..Tiff::default() }).unwrap(), "tif")
            }
            "webp" => (formats::webp(scene, &Webp::default()).unwrap(), "webp"),
            "gif" => (formats::gif(scene, 1).unwrap(), "gif"),
            "bmp" => (formats::bmp(scene, Colour::Rgb).unwrap(), "bmp"),
            "jxl" => (formats::jxl(scene, Colour::Rgb, Depth::Eight, false).unwrap(), "jxl"),
            "psd" => (formats::psd(scene, &Psd { rle: true, ..Psd::default() }).unwrap(), "psd"),
            _ => (raf(&jpeg(if i.is_multiple_of(2) { 1 } else { 6 })), "raf"),
        }
    }

    /// Whether photo `i` of `kind` gets a place: every third photo whose format Dalica writes
    /// metadata into.
    fn gps(i: usize, kind: &str) -> bool {
        i.is_multiple_of(3) && (kind.starts_with("jpeg") || kind.starts_with("png") || kind.starts_with("tiff") || kind == "webp")
    }

    /// Writes the first `n` photos of the tray ([`PHOTOS`] for all) into `dir` (which exists) at
    /// `scale` and answers them in name order.
    pub fn build(dir: &Path, scale: u32, n: usize) -> Vec<Photo> {
        let mut photos = Vec::with_capacity(n);
        for i in 0..n {
            let kind = KINDS[i % KINDS.len()];
            let (w, h) = size(i, scale);
            let scene = Scene::new(i as u64 + 1, w, h);
            let (mut bytes, ext) = file(i, kind, &scene);
            let gps = gps(i, kind);
            if gps {
                let place = Place::new(38.7 + i as f64 / 1000.0, -9.1 - i as f64 / 1000.0);
                bytes = dalica_meta::write_owned(&bytes, &Changes { place: Some(place), ..Changes::default() }, &WriteOptions::default()).unwrap();
            }
            let header = dalica_image::read_header(&bytes, &Limits::default()).unwrap();
            let name = format!("{:03}-{kind}.{ext}", i + 1);
            dalica_fs::replace(&dir.join(&name), &bytes, Expect::Absent).unwrap();
            photos.push(Photo { name, kind, alpha: kind == "png-alpha", gps, display: header.display_size(), edits: edits(i) });
        }
        photos
    }
}

const PATIENCE: Duration = Duration::from_secs(600);

/// The photos of the tray the tests export: all 200 at the review gate, two of each kind in a
/// task's gate.
fn photos() -> usize {
    pick(2 * tray::KINDS.len(), tray::PHOTOS)
}

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
    photos: Vec<tray::Photo>,
    ids: Vec<PhotoId>,
}

impl Lib {
    /// A library watching `Photos/Mixed`, the tray in it and each photo's edits applied.
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let mixed = dir.path().join("Photos").join("Mixed");
        dalica_fs::create_dir_all(&mixed).unwrap();
        let photos = tray::build(&mixed, 1, self::photos());
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let ids: Vec<PhotoId> = photos
            .iter()
            .map(|p| s.library().photos().find(|q| q.name == p.name).unwrap_or_else(|| panic!("{} is in the library", p.name)).id)
            .collect();
        for (p, id) in photos.iter().zip(&ids) {
            for op in p.edits {
                s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
            }
            assert_eq!(s.library().info(*id).edits.len(), p.edits.len(), "{}", p.name);
        }
        Lib { dir, s, photos, ids }
    }

    fn out(&self) -> PathBuf {
        self.dir.path().join("Out")
    }

    fn params(&self, folder: &str, watermark: bool) -> Value {
        json!({
            "ids": self.ids, "dir": self.out(), "folderName": folder, "size": tray::LONG_SIDE, "quality": "normal",
            "watermark": if watermark { json!(tray::WATERMARK) } else { json!(false) }, "removeLocation": true,
        })
    }

    /// Runs `export.run` to its end and waits for the scan that follows.
    fn export(&mut self, folder: &str, watermark: bool) -> (JobEntry, f64) {
        let started = Instant::now();
        let reply = self.s.execute("export.run", &self.params(folder, watermark)).unwrap();
        let entry = self.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap();
        let secs = started.elapsed().as_secs_f64();
        assert!(entry.state.is_finished(), "{entry:?}");
        self.s.wait_idle();
        (entry, secs)
    }
}

/// The files in `dir`, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

fn decode(path: &Path) -> dalica_image::Rgba8 {
    let bytes = std::fs::read(path).unwrap();
    dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap_or_else(|e| panic!("{} decodes: {e}", path.display()))
}

/// Pixels of `a` and `b` (the same size) in `x0..x1 × y0..y1` whose colour differs by more than a
/// JPEG's noise.
fn changed(a: &dalica_image::Rgba8, b: &dalica_image::Rgba8, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> usize {
    let mut n = 0;
    for y in y0..y1.min(a.height) {
        for x in x0..x1.min(a.width) {
            let i = ((y * a.width + x) * 4) as usize;
            n += usize::from((0..3).any(|c| a.data[i + c].abs_diff(b.data[i + c]) > 24));
        }
    }
    n
}

#[test]
fn a_mixed_tray_of_200_exports_whole_with_a_watermark_and_no_location() {
    let n = photos();
    let started = Instant::now();
    let mut lib = Lib::new();
    let built = started.elapsed().as_secs_f64();
    assert_eq!(lib.photos.len(), n);
    for kind in tray::KINDS {
        assert!(lib.photos.iter().filter(|p| p.kind == kind).count() >= n / tray::KINDS.len(), "{kind} is in the tray");
    }
    let edited = lib.photos.iter().filter(|p| !p.edits.is_empty()).count();
    let located = lib.photos.iter().filter(|p| p.gps).count();
    assert!((3 * n / 10..7 * n / 10).contains(&edited) && located >= 3 * n / 20, "{edited} edited, {located} with GPS");
    assert!(lib.photos.iter().any(|p| p.display.0.max(p.display.1) > tray::LONG_SIDE), "a photo the export makes smaller");
    // The places are really there, in the files the export reads.
    for p in lib.photos.iter().filter(|p| p.gps) {
        let bytes = std::fs::read(lib.dir.path().join("Photos").join("Mixed").join(&p.name)).unwrap();
        let meta = dalica_meta::read(&bytes, &dalica_meta::Limits::default()).unwrap();
        assert!(meta.owned.place.is_some(), "{} has a place", p.name);
    }

    let (done, marked_secs) = lib.export("Mixed", true);
    assert_eq!(done.state, JobState::Done);
    assert_eq!(done.label, format!("Exported {n} pictures"));
    let result = done.result.unwrap();
    assert_eq!((result["total"].clone(), result["done"].clone()), (json!(n), json!(n)));
    assert_eq!((result["skipped"].clone(), result["failed"].clone(), result["cancelled"].clone()), (json!([]), json!([]), json!(false)));
    assert_eq!(result["written"].as_array().unwrap().len(), n);
    let marked = lib.out().join("Mixed");
    let mut want: Vec<String> = lib.photos.iter().map(tray::Photo::exported).collect();
    want.push(".dalica.json".into());
    want.sort();
    lib.s.flush().unwrap(); // the folder's sidecar is written on a debounce: list it once it has landed
    assert_eq!(files(&marked), want, "one file per photo, under its own name");

    // The same export without the watermark, to find it by difference.
    let (plain_done, plain_secs) = lib.export("Plain", false);
    assert_eq!(plain_done.state, JobState::Done);
    let plain = lib.out().join("Plain");

    let mut least_marked = usize::MAX;
    for p in &lib.photos {
        let name = p.exported();
        let path = marked.join(&name);
        let bytes = std::fs::read(&path).unwrap();
        let header = dalica_image::read_header(&bytes, &dalica_image::Limits::default()).unwrap();
        let want_format = if p.alpha { dalica_image::Format::Png } else { dalica_image::Format::Jpeg };
        assert_eq!(header.format, want_format, "{name}");
        assert_eq!(header.orientation.get(), 1, "{name}");
        let img = decode(&path);
        let (w, h) = p.expected(tray::LONG_SIDE);
        assert_eq!((img.width, img.height), (w, h), "{name} ({:?} shown, edits {:?})", p.display, p.edits);
        if !p.alpha {
            let q = dalica_image::jpeg_quality::quality(&bytes).unwrap_or(0);
            assert!(q.abs_diff(85) <= 2, "{name}: quality {q}, Normal is 85 ± 2");
        }
        let meta = dalica_meta::read(&bytes, &dalica_meta::Limits::default()).unwrap();
        assert_eq!(meta.owned.place, None, "{name}: no place");
        assert_eq!((meta.sources.exif.place, meta.sources.xmp.place), (None, None), "{name}: no GPS in EXIF or XMP");

        // The watermark: in its box, bottom-right, and nowhere near the top-left.
        let unmarked = decode(&plain.join(&name));
        let b = dalica_create::watermark::layout(w, h, tray::WATERMARK).unwrap();
        let text = (b.x.floor() as u32, b.y.floor() as u32, (b.x + b.width).ceil() as u32, (b.y + b.height).ceil() as u32);
        let inside = changed(&img, &unmarked, text);
        least_marked = least_marked.min(inside);
        assert!(inside > 10, "{name}: the watermark is in its box ({inside} pixels changed)");
        assert!(text.0 > w / 2 && text.1 > h / 2, "{name}: the box is bottom-right: {text:?} in {w} × {h}");
        assert_eq!(changed(&img, &unmarked, (0, 0, w / 2, h / 2)), 0, "{name}: the rest is unchanged");
    }
    eprintln!(
        "mixed export: tray built and scanned in {built:.1} s; {n} photos exported in {marked_secs:.1} s with the watermark, {plain_secs:.1} s without; \
         at least {least_marked} pixels of watermark"
    );
}

#[test]
fn a_cancel_half_way_through_the_mixed_tray_leaves_only_complete_files() {
    let n = photos();
    let mut lib = Lib::new();
    let reply = lib.s.execute("export.run", &lib.params("Mixed", true)).unwrap();
    let job = reply["job"].as_u64().unwrap();
    let start = Instant::now();
    while lib.s.job(job).unwrap().progress.done < (n / 2) as u64 {
        assert!(start.elapsed() < PATIENCE, "timed out");
        lib.s.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(lib.s.cancel_jobs(Some(job)).unwrap(), [job]);
    let done = lib.s.wait_job(job, PATIENCE).unwrap();
    lib.s.wait_idle();
    assert_eq!(done.state, JobState::Cancelled);
    let result = done.result.unwrap();
    assert_eq!(result["cancelled"], true);
    let written = result["written"].as_array().unwrap().len();
    assert!((n / 2..n).contains(&written), "{written} written");

    // Only complete files: each decodes whole at its size; nothing else is in the folder.
    let out = lib.out().join("Mixed");
    lib.s.flush().unwrap(); // the folder's sidecar has landed: its temporary file is not mistaken for the export's
    let names = files(&out);
    assert_eq!(names.len(), written + 1, "{names:?}");
    for name in names.iter().filter(|n| *n != ".dalica.json") {
        let p = lib.photos.iter().find(|p| p.exported() == *name).unwrap_or_else(|| panic!("{name} is a photo's export"));
        let img = decode(&out.join(name));
        assert_eq!((img.width, img.height), p.expected(tray::LONG_SIDE), "{name}");
    }
    assert_eq!(lib.s.library().photos_in(lib.s.library().folder_by_path(&out).unwrap()).len(), written, "what was written is in the library");
}
