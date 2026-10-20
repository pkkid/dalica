//! Every format saves as `formats.md` §1's "Save edits in place" column says (`roadmap.md` M8.23;
//! `data-model.md` §6), through `photo.save` and `photo.undoSave` on temporary folders of
//! procedural `scenes` files, checking the files on disk after each step:
//!
//! - **JPEG** (baseline with EXIF and a place, baseline without EXIF, progressive, CMYK and YCCK)
//!   is saved in place: the original kept byte for byte in `.dalica-originals/`, the file the
//!   recipe rendered from it at the original's quality with its chroma subsampling, its camera
//!   EXIF and place kept and the library's caption and tags merged; Undo Save puts the exact bytes
//!   back.
//! - **PNG without transparency** (8-bit, 16-bit, and RGBA whose alpha is all opaque) is saved in
//!   place as PNG; a 16-bit PNG stays 16-bit (exact for a rotate-only Save; other edits render at 8
//!   bits and are widened back).
//! - **PNG with transparency, TIFF, WebP, GIF, BMP, JPEG XL, PSD, a raw** and, with the `heif`
//!   feature, **HEIC** get `<stem>.jpg` beside them (then `<stem>-001.jpg`) as a new unedited
//!   photo, the source byte-identical and recording the copy; Undo Save does not apply.
//! - **AVIF** cannot be saved, nor HEIC in a build without the `heif` feature: `photo.save` is
//!   disabled with the reason, or leaves such a photo out with the reason beside savable ones, and
//!   nothing is written. A **video** is not listed before M15, so Save never reaches one.
//! - **Rotate-only** Saves of JPEGs: an MCU-sized baseline file is turned in its coefficients
//!   (orientation 1, its own quantisation tables); a partial-MCU size and a progressive file keep
//!   their entropy-coded data byte for byte and take the orientation flag.
//!
//! The HEIC case runs with `cargo test -p dalica-engine --features heif --test all save_formats::`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_create::output::{self, Purpose, Spec};
use dalica_edit::Recipe;
use dalica_engine::cmd::save::NOTHING_SAVED;
use dalica_engine::{CacheDir, CmdError, JobEntry, JobState, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_image::encode::{self, Channels, Image, Metadata, Pixels};
use dalica_image::{Decoded, Format, Limits, Rgba8, Samples, formats, jpeg_quality, read_header};
use dalica_meta::{Changes, Place, WriteOptions};
use dalica_scenes::formats::{Colour, Compression, Depth, Psd, Tiff, Webp};
use dalica_scenes::{Exif, Scene};
use serde_json::{Value, json};

const PATIENCE: Duration = Duration::from_secs(300);
const TUNING: &str = "tuning(0.25,0,0,0,none)";
const ROTATE: &str = "rotate(90)";

// --- the library --------------------------------------------------------------------------------

struct Lib {
    dir: tempfile::TempDir,
    s: Session,
}

impl Lib {
    /// A library watching `Pictures/Formats` holding `files`, scanned.
    fn new(files: &[(&str, Vec<u8>)]) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Pictures").join("Formats");
        dalica_fs::create_dir_all(&folder).unwrap();
        for (name, bytes) in files {
            dalica_fs::replace(&folder.join(name), bytes, Expect::Absent).unwrap();
        }
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        Lib { dir, s }
    }

    fn folder(&self) -> PathBuf {
        self.dir.path().join("Pictures").join("Formats")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder().join(name)
    }

    fn kept(&self, name: &str) -> PathBuf {
        self.folder().join(".dalica-originals").join(name)
    }

    fn id(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} is in the library")).id
    }

    fn has(&self, name: &str) -> bool {
        self.s.library().photos().any(|p| p.name == name)
    }

    fn edit(&mut self, name: &str, ops: &[&str]) -> PhotoId {
        let id = self.id(name);
        for op in ops {
            self.s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
        }
        id
    }

    /// Lands every pending metadata write-back (the caption, tags and star the test set are
    /// written into the files after a debounce) and waits for the rescan, so the bytes read next
    /// are the ones the Save starts from.
    fn settle(&mut self) {
        assert!(self.s.flush_metadata(PATIENCE), "the metadata writes landed");
        self.s.wait_idle();
        assert_eq!(self.s.pending_metadata_writes(), 0);
        self.s.flush().unwrap();
    }

    fn edits(&self, id: PhotoId) -> Vec<String> {
        self.s.library().info(id).edits.clone()
    }

    /// Runs `cmd` with `params` and waits for its job.
    fn job(&mut self, cmd: &str, params: Value) -> JobEntry {
        let reply = self.s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd} {params}: {e:?}"));
        assert_eq!(reply["pending"], json!(true), "{reply}");
        let entry = self.s.wait_job(serde_json::from_value(reply["job"].clone()).unwrap(), PATIENCE).unwrap();
        assert!(entry.state.is_finished(), "{cmd}: {entry:?}");
        self.s.wait_idle();
        entry
    }

    /// Saves `ids` and answers the job's result, checking that nothing failed.
    fn save(&mut self, ids: &[PhotoId]) -> Value {
        let done = self.job("photo.save", json!({ "ids": ids, "confirm": true }));
        assert_eq!(done.state, JobState::Done);
        let result = done.result.unwrap();
        assert_eq!(result["failed"], json!([]), "{result}");
        result
    }

    /// The names in the folder, Dalica's own files left out, once every pending write has landed:
    /// the folder's sidecar is written on a debounce, and a listing taken while it is being
    /// replaced would see its temporary file (`..dalica.json.dalica-tmp-…`).
    fn names(&mut self) -> Vec<String> {
        self.settle();
        let mut names: Vec<String> = std::fs::read_dir(self.folder())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with(".dalica"))
            .collect();
        names.sort();
        names
    }
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn decode(bytes: &[u8]) -> Rgba8 {
    dalica_image::decode(bytes, &Limits::default()).unwrap()
}

/// `bytes` decoded to the samples the file stores, before the colour stage and orientation.
fn stored(bytes: &[u8]) -> Decoded {
    let limits = Limits::default();
    let header = read_header(bytes, &limits).unwrap();
    let mut budget = dalica_image::Budget::new(&limits, header.width, header.height);
    formats::decoder(header.format)(bytes, &header, &mut budget, &limits).unwrap()
}

/// Mean absolute difference per colour channel of two images of one size.
fn difference(case: &str, a: &Rgba8, b: &Rgba8) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height), "{case}");
    let sum: u64 = a.data.chunks(4).zip(b.data.chunks(4)).map(|(p, q)| (0..3).map(|c| u64::from(p[c].abs_diff(q[c]))).sum::<u64>()).sum();
    sum as f64 / (f64::from(a.width) * f64::from(a.height) * 3.0)
}

/// What the one output pipeline makes of `source` with `ops` for `purpose`, decoded: the oracle a
/// saved file must match.
fn oracle(source: &[u8], ops: &[&str], purpose: Purpose) -> Rgba8 {
    let recipe = Recipe::parse(ops).unwrap();
    decode(&output::render(source, &recipe, &Spec::new(purpose)).unwrap().bytes)
}

// --- the files ----------------------------------------------------------------------------------

/// A camera JPEG of scene `seed` at `quality` with EXIF (make, model, a 160-pixel thumbnail).
fn camera(seed: u64, width: u32, height: u32, quality: u8) -> Vec<u8> {
    let exif = Exif { orientation: 1, thumbnail: Some(160), make: Some("Dalicam".into()), model: Some("Formats 1".into()), ..Exif::default() };
    dalica_scenes::jpeg(&Scene::new(seed, width, height), quality, Some(&exif)).unwrap()
}

/// `bytes` with a place written into them (EXIF GPS and XMP).
fn with_place(bytes: &[u8], place: Place) -> Vec<u8> {
    dalica_meta::write_owned(bytes, &Changes { place: Some(Some(place)), ..Changes::default() }, &WriteOptions::default()).unwrap()
}

/// A JPEG of flat 8 × 8 blocks (DC coefficients only, quantiser 1) with `components` (1, 3 or 4)
/// components at full resolution: baseline (`frame` 0xC0) or progressive (0xC2, one DC scan and
/// no AC scans, so every AC coefficient is 0). `adobe` adds an APP14 segment with that colour
/// transform (0 CMYK, 1 YCbCr, 2 YCCK). Each block decodes to the stored value `block(bx, by)`
/// per component. No encoder we use writes progressive or four-component JPEGs.
fn flat_jpeg(width: u32, height: u32, components: u8, frame: u8, adobe: Option<u8>, block: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let n = usize::from(components);
    let progressive = frame == 0xc2;
    let mut out = vec![0xff, 0xd8];
    let mut segment = |marker: u8, body: &[u8]| {
        out.extend([0xff, marker]);
        out.extend(((body.len() + 2) as u16).to_be_bytes());
        out.extend(body);
    };
    if let Some(t) = adobe {
        segment(0xee, &[b"Adobe".as_slice(), &100u16.to_be_bytes(), &[0, 0, 0, 0, t]].concat());
    }
    segment(0xdb, &[[0u8].as_slice(), &[1u8; 64]].concat());
    let mut sof = vec![8];
    sof.extend((height as u16).to_be_bytes());
    sof.extend((width as u16).to_be_bytes());
    sof.push(components);
    for id in 1..=components {
        sof.extend([id, 0x11, 0]);
    }
    segment(frame, &sof);
    // DC table 0: the typical luminance DC table of T.81 Annex K.3; AC table 0: only EOB, code "0".
    let dc_bits = [0u8, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
    segment(0xc4, &[[0x00u8].as_slice(), &dc_bits, &(0..12).collect::<Vec<u8>>()].concat());
    if !progressive {
        let ac_bits = [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        segment(0xc4, &[[0x10u8].as_slice(), &ac_bits, &[0x00]].concat());
    }
    let mut sos = vec![components];
    for id in 1..=components {
        sos.extend([id, 0]);
    }
    sos.extend(if progressive { [0, 0, 0] } else { [0, 63, 0] });
    segment(0xda, &sos);
    // Canonical DC codes (T.81 Annex C).
    let mut codes = Vec::new();
    let mut code = 0u32;
    for (len, &count) in dc_bits.iter().enumerate() {
        for _ in 0..count {
            codes.push((code, len as u32 + 1));
            code += 1;
        }
        code <<= 1;
    }
    let mut bits = BitWriter::default();
    let mut pred = [0i32; 4];
    for by in 0..height.div_ceil(8) {
        for bx in 0..width.div_ceil(8) {
            let values = block(bx, by);
            for c in 0..n {
                let dc = (i32::from(values[c]) - 128) * 8;
                let diff = dc - pred[c];
                pred[c] = dc;
                let cat = 32 - diff.unsigned_abs().leading_zeros();
                let (code, len) = codes[cat as usize];
                bits.put(code, len);
                if cat > 0 {
                    let v = if diff < 0 { diff + (1 << cat) - 1 } else { diff };
                    bits.put(v as u32, cat);
                }
                if !progressive {
                    bits.put(0, 1); // EOB
                }
            }
        }
    }
    out.extend(bits.finish());
    out.extend([0xff, 0xd9]);
    out
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, len: u32) {
        for i in (0..len).rev() {
            self.acc = (self.acc << 1) | ((value >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.byte();
            }
        }
    }

    fn byte(&mut self) {
        let b = self.acc as u8;
        self.out.push(b);
        if b == 0xff {
            self.out.push(0); // byte stuffing
        }
        self.acc = 0;
        self.n = 0;
    }

    fn finish(mut self) -> Vec<u8> {
        while self.n != 0 {
            self.put(1, 1); // pad with ones (T.81 F.1.2.3)
        }
        self.out
    }
}

/// Block values of a gentle colour pattern, so an edit has something to change.
fn pattern(bx: u32, by: u32) -> [u8; 4] {
    [(60 + bx * 17 % 120) as u8, (90 + by * 23 % 100) as u8, (40 + (bx + by) * 11 % 150) as u8, (20 + bx * by % 60) as u8]
}

/// YCbCr blocks of the same pattern (luma varied, chroma near neutral).
fn ycc_pattern(bx: u32, by: u32) -> [u8; 4] {
    [(50 + (bx * 13 + by * 7) % 160) as u8, (118 + bx % 20) as u8, (138 - by % 20) as u8, 0]
}

/// The scene's pixels with a left-to-right alpha ramp from 40 to 255 (`opaque`: all 255).
fn rgba(seed: u64, width: u32, height: u32, opaque: bool) -> Rgba8 {
    let img = Scene::new(seed, width, height).render().unwrap();
    let data = img
        .data
        .chunks(3)
        .enumerate()
        .flat_map(|(i, p)| {
            let x = u64::from(i as u32 % width);
            let a = if opaque { 255 } else { (40 + x * 215 / u64::from(width - 1)) as u8 };
            [p[0], p[1], p[2], a]
        })
        .collect();
    Rgba8 { width, height, data }
}

/// A PNG's bit depth and colour type (2 RGB, 6 RGBA), from its IHDR chunk.
fn png_layout(bytes: &[u8]) -> (u8, u8) {
    assert_eq!(&bytes[12..16], b"IHDR");
    (bytes[24], bytes[25])
}

fn png_rgba(img: &Rgba8) -> Vec<u8> {
    encode::png(&Image::rgba8(img), &Metadata::default()).unwrap()
}

/// A 16-bit RGB PNG of the scene: each 8-bit value spread over 16 bits with a low-order ramp, so
/// the file holds samples an 8-bit round trip cannot keep.
fn png16(seed: u64, width: u32, height: u32) -> Vec<u8> {
    let img = Scene::new(seed, width, height).render().unwrap();
    let samples: Vec<u16> = img.data.iter().enumerate().map(|(i, &v)| (i32::from(v) * 257 + (i % 97) as i32 - 48).clamp(0, 65_535) as u16).collect();
    encode::png(&Image { width, height, channels: Channels::Rgb, pixels: Pixels::U16(&samples) }, &Metadata::default()).unwrap()
}

/// A RAF file around a camera JPEG (its preview), from the structure the RAF reader's provenance
/// header describes: the magic, model, version, then the JPEG's offset and length.
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

/// An ISO BMFF box.
fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

/// An `ftyp` box of the brands given.
fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
    let mut p = major.to_vec();
    p.extend([0, 0, 0, 0]);
    for c in compatible {
        p.extend(*c);
    }
    bx(b"ftyp", &p)
}

/// An AVIF file as far as Dalica reads one: its brands and a meta box.
fn avif() -> Vec<u8> {
    [ftyp(b"avif", &[b"mif1", b"avif"]), bx(b"meta", &[0; 16])].concat()
}

/// An MP4 file as far as Dalica reads one: its brands and an empty movie.
fn mp4() -> Vec<u8> {
    [ftyp(b"mp42", &[b"mp42", b"isom"]), bx(b"moov", &[]), bx(b"mdat", &[0; 64])].concat()
}

// --- JPEG ---------------------------------------------------------------------------------------

/// The marker segments of a JPEG up to and including its first scan header.
fn segments(bytes: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut at = 2;
    while at + 4 <= bytes.len() {
        assert_eq!(bytes[at], 0xff, "a marker at {at}");
        let marker = bytes[at + 1];
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        out.push((marker, &bytes[at..at + 2 + len]));
        if marker == 0xda {
            break;
        }
        at += 2 + len;
    }
    out
}

/// The entropy-coded data and everything after it.
fn scan_data(bytes: &[u8]) -> &[u8] {
    let end: usize = segments(bytes).iter().map(|(_, s)| s.len()).sum::<usize>() + 2;
    &bytes[end..]
}

/// The frame marker (0xC0 baseline, 0xC2 progressive, …).
fn frame(bytes: &[u8]) -> u8 {
    segments(bytes).iter().map(|(m, _)| *m).find(|m| matches!(m, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf)).unwrap()
}

/// JPEG in place: baseline with EXIF and a place, baseline without EXIF, progressive, CMYK and
/// YCCK. Each saves at its own quality and subsampling, keeps its metadata and takes the
/// library's; Undo Save puts every original back byte for byte.
#[test]
fn jpegs_save_in_place_with_their_quality_matched_and_metadata_kept() {
    let place = Place::new(38.7139, -9.1394).unwrap();
    let files = vec![
        ("exif.jpg", with_place(&camera(1, 320, 240, 85), place)),
        ("plain.jpg", dalica_scenes::jpeg(&Scene::new(2, 240, 176), 70, None).unwrap()),
        ("progressive.jpg", flat_jpeg(96, 64, 3, 0xc2, None, ycc_pattern)),
        ("cmyk.jpg", flat_jpeg(96, 64, 4, 0xc0, Some(0), pattern)),
        ("ycck.jpg", flat_jpeg(96, 64, 4, 0xc0, Some(2), pattern)),
    ];
    let mut lib = Lib::new(&files);
    assert_eq!(frame(&files[2].1), 0xc2, "a progressive file");
    let ids: Vec<PhotoId> = files.iter().map(|(name, _)| lib.edit(name, &[TUNING])).collect();
    lib.s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Alfama" })).unwrap();
    lib.s.execute("photo.tags.add", &json!({ "ids": [ids[0]], "tags": ["tram"] })).unwrap();
    lib.settle();
    let before: Vec<Vec<u8>> = files.iter().map(|(name, _)| read(&lib.path(name))).collect();

    let result = lib.save(&ids);
    assert_eq!(result["done"], json!(files.len()));
    assert_eq!(lib.names(), ["cmyk.jpg", "exif.jpg", "plain.jpg", "progressive.jpg", "ycck.jpg"], "nothing written beside");
    for (((name, _), id), original) in files.iter().zip(&ids).zip(&before) {
        let saved = read(&lib.path(name));
        assert_eq!(&read(&lib.kept(name)), original, "{name}: the original is kept byte for byte");
        let header = read_header(&saved, &Limits::default()).unwrap();
        assert_eq!((header.format, header.orientation.get()), (Format::Jpeg, 1), "{name}");
        assert_eq!(frame(&saved), 0xc0, "{name}: written as a baseline JPEG");
        let (want, got) = (jpeg_quality::read(original).unwrap(), jpeg_quality::read(&saved).unwrap());
        assert_eq!(got.quality, want.quality, "{name}: the quality is matched");
        if want.sampling != jpeg_quality::Sampling::Grey {
            assert_eq!(got.sampling.for_jpeg(), want.sampling.for_jpeg(), "{name}: the subsampling is kept");
        }
        let d = difference(name, &decode(&saved), &oracle(original, &[TUNING], Purpose::InPlace));
        assert!(d < 0.5, "{name}: the file is the recipe rendered from the original (mean difference {d:.3})");
        assert!(lib.edits(*id).is_empty() && lib.s.library().can_undo_save(*id), "{name}");
    }
    // The camera's EXIF and the place stay; the library's caption and tags are merged.
    let saved = read(&lib.path("exif.jpg"));
    let exif = read_header(&saved, &Limits::default()).unwrap().exif.unwrap();
    assert_eq!((exif.make.as_deref(), exif.model.as_deref()), (Some("Dalicam"), Some("Formats 1")));
    let meta = dalica_meta::read(&saved, &dalica_meta::Limits::default()).unwrap();
    assert_eq!(meta.owned.place, Some(place), "the place stays");
    assert_eq!((meta.owned.caption.as_deref(), meta.owned.tags.clone()), (Some("Alfama"), vec!["tram".to_string()]));

    // Undo Save: every original back, byte for byte, with its edits as unsaved edits.
    let done = lib.job("photo.undoSave", json!({ "ids": ids }));
    assert_eq!((done.state, done.label.as_str()), (JobState::Done, "Undid the save of 5 photos"), "{:?}", done.result);
    for (((name, _), id), original) in files.iter().zip(&ids).zip(&before) {
        assert_eq!(&read(&lib.path(name)), original, "{name}: the exact original");
        assert!(!lib.kept(name).exists(), "{name}");
        assert_eq!(lib.edits(*id), [TUNING], "{name}");
    }
}

// --- PNG ----------------------------------------------------------------------------------------

/// A PNG without transparency saves in place as PNG: 8-bit, 16-bit, and RGBA whose alpha is all
/// opaque (written without it). 16 bits stay 16 bits: a rotate-only Save keeps every sample
/// exactly; a Tuning renders at 8 bits and is widened back. Undo Save puts each original back.
#[test]
fn pngs_without_transparency_save_in_place_as_png() {
    let files = vec![
        ("rgb8.png", dalica_scenes::png(&Scene::new(11, 120, 80)).unwrap()),
        ("rgb16.png", png16(12, 120, 80)),
        ("turn16.png", png16(13, 120, 80)),
        ("opaque.png", png_rgba(&rgba(14, 120, 80, true))),
    ];
    let edits: [&[&str]; 4] = [&[TUNING], &[TUNING], &[ROTATE], &[TUNING]];
    let mut lib = Lib::new(&files);
    let ids: Vec<PhotoId> = files.iter().zip(edits).map(|((name, _), ops)| lib.edit(name, ops)).collect();
    let before: Vec<Vec<u8>> = files.iter().map(|(name, _)| read(&lib.path(name))).collect();

    lib.save(&ids);
    assert_eq!(lib.names(), ["opaque.png", "rgb16.png", "rgb8.png", "turn16.png"], "nothing written beside");
    for (((name, _), ops), original) in files.iter().zip(edits).zip(&before) {
        let saved = read(&lib.path(name));
        assert_eq!(&read(&lib.kept(name)), original, "{name}: the original is kept");
        assert_eq!(read_header(&saved, &Limits::default()).unwrap().format, Format::Png, "{name}");
        let d = difference(name, &decode(&saved), &oracle(original, ops, Purpose::InPlace));
        assert!(d < 0.5, "{name}: the file is the recipe rendered from the original (mean difference {d:.3})");
    }
    let ihdr = |name: &str| png_layout(&read(&lib.path(name)));
    assert_eq!(png_layout(&before[3]), (8, 6), "the source is RGBA");
    assert_eq!(ihdr("opaque.png"), (8, 2), "an alpha channel that is all opaque is left out");
    assert_eq!((ihdr("rgb8.png"), ihdr("rgb16.png"), ihdr("turn16.png")), ((8, 2), (16, 2), (16, 2)), "each keeps its depth");
    // The samples as decoded, with the channel count the decoder gives them.
    let deep = |name: &str| {
        let d = stored(&read(&lib.path(name)));
        let n = d.channels();
        match d.samples {
            Samples::U16(s) => (s, n),
            _ => panic!("{name}: not 16-bit samples"),
        }
    };
    // A rotate-only Save turns the 16-bit samples exactly.
    let source = stored(&before[2]);
    let n = source.channels();
    let Samples::U16(src) = &source.samples else { panic!("the source is 16-bit") };
    let (turned, m) = deep("turn16.png");
    assert_eq!(m, n);
    let (w, h) = (source.width as usize, source.height as usize);
    let turned_header = read_header(&read(&lib.path("turn16.png")), &Limits::default()).unwrap();
    assert_eq!((turned_header.width, turned_header.height), (h as u32, w as u32));
    for y in 0..h {
        for x in 0..w {
            // A quarter turn clockwise: the source's (x, y) lands at (h - 1 - y, x).
            let (tx, ty) = (h - 1 - y, x);
            let (s, t) = ((y * w + x) * n, (ty * h + tx) * n);
            assert_eq!(&turned[t..t + 3], &src[s..s + 3], "the sample at ({x}, {y}) moved exactly");
        }
    }
    // A Tuning renders at 8 bits: the 16-bit file holds 8-bit values widened (v × 257).
    assert!(deep("rgb16.png").0.iter().all(|v| v % 257 == 0), "other edits render at 8 bits and are widened back");

    let done = lib.job("photo.undoSave", json!({ "ids": ids }));
    assert_eq!(done.state, JobState::Done);
    for ((name, _), original) in files.iter().zip(&before) {
        assert_eq!(&read(&lib.path(name)), original, "{name}: the exact original");
        assert!(!lib.kept(name).exists(), "{name}");
    }
}

// --- formats saved beside -----------------------------------------------------------------------

/// The files Dalica cannot write in place, each with the edits it gets: (name, bytes, edits).
fn beside_sources() -> Vec<(&'static str, Vec<u8>, &'static [&'static str])> {
    let scene = |seed: u64| Scene::new(seed, 120, 80);
    #[allow(unused_mut)]
    let mut files: Vec<(&'static str, Vec<u8>, &'static [&'static str])> = vec![
        ("alpha.png", png_rgba(&rgba(21, 120, 80, false)), &[TUNING]),
        ("tiff8.tif", dalica_scenes::formats::tiff(&scene(22), &Tiff { compression: Compression::Lzw, ..Tiff::default() }).unwrap(), &[TUNING]),
        (
            "tiff16.tiff",
            dalica_scenes::formats::tiff(&scene(23), &Tiff { depth: Depth::Sixteen, compression: Compression::Deflate, ..Tiff::default() }).unwrap(),
            &[ROTATE],
        ),
        ("cmyk.tif", dalica_scenes::formats::tiff(&scene(24), &Tiff { colour: Colour::Cmyk, ..Tiff::default() }).unwrap(), &[TUNING]),
        ("lossless.webp", dalica_scenes::formats::webp(&scene(25), &Webp::default()).unwrap(), &[TUNING]),
        ("first.gif", dalica_scenes::formats::gif(&scene(26), 1).unwrap(), &[TUNING]),
        ("bitmap.bmp", dalica_scenes::formats::bmp(&scene(27), Colour::Rgb).unwrap(), &[TUNING]),
        ("xl.jxl", dalica_scenes::formats::jxl(&scene(28), Colour::Rgb, Depth::Eight, false).unwrap(), &[TUNING]),
        ("layers.psd", dalica_scenes::formats::psd(&scene(29), &Psd { rle: true, ..Psd::default() }).unwrap(), &[TUNING]),
        ("camera.raf", raf(&camera(30, 160, 120, 90)), &[TUNING, ROTATE]),
    ];
    #[cfg(feature = "heif")]
    files.push(("iphone.heic", heic::build(64, 48), &[TUNING]));
    files
}

/// Every format Dalica cannot write in place gets `<stem>.jpg` beside it: the recipe rendered as
/// a JPEG (transparency over white), a new unedited photo with the source's owned fields; the
/// source stays byte-identical, keeps its recipe and records the copy, and Undo Save and Revert do
/// not apply. A second Save after another edit takes the next free name.
#[test]
fn formats_dalica_cannot_write_get_a_jpeg_beside_and_stay_byte_identical() {
    let sources = beside_sources();
    let files: Vec<(&str, Vec<u8>)> = sources.iter().map(|(name, bytes, _)| (*name, bytes.clone())).collect();
    let mut lib = Lib::new(&files);
    let ids: Vec<PhotoId> = sources.iter().map(|(name, _, ops)| lib.edit(name, ops)).collect();
    lib.s.execute("photo.caption.set", &json!({ "ids": ids, "caption": "Beside" })).unwrap();
    lib.s.execute("photo.star.set", &json!({ "ids": ids, "on": true })).unwrap();
    lib.settle();
    let before: Vec<Vec<u8>> = sources.iter().map(|(name, ..)| read(&lib.path(name))).collect();
    assert_ne!(before[0], files[0].1, "the caption was written back into alpha.png before the Save, not during it");

    let result = lib.save(&ids);
    assert_eq!(result["done"], json!(sources.len()));
    assert!(!lib.folder().join(".dalica-originals").exists(), "no original is kept: nothing was replaced");
    for (((name, _, ops), id), original) in sources.iter().zip(&ids).zip(&before) {
        assert_eq!(&read(&lib.path(name)), original, "{name}: the source is byte-identical");
        let jpg = format!("{}.jpg", name.rsplit_once('.').unwrap().0);
        let bytes = read(&lib.path(&jpg));
        let header = read_header(&bytes, &Limits::default()).unwrap();
        assert_eq!((header.format, header.orientation.get()), (Format::Jpeg, 1), "{name}");
        let d = difference(name, &decode(&bytes), &oracle(original, ops, Purpose::Beside));
        assert!(d < 0.5, "{name}: the copy is the recipe rendered (mean difference {d:.3})");
        // The copy is a new, unedited photo with the source's owned fields.
        let copy = lib.s.library().photo(lib.id(&jpg)).unwrap().clone();
        assert_eq!((copy.width, copy.height), header.display_size(), "{name}");
        assert!(copy.star, "{name}: the star is carried");
        let info = lib.s.library().info(copy.id);
        assert!(info.edits.is_empty() && info.caption == "Beside", "{name}: {info:?}");
        // The source records the copy and keeps its recipe.
        let recorded = lib.s.library().saved_as(*id).map(|c| (c.name.clone(), c.recipe.clone()));
        let want: Vec<String> = ops.iter().map(|s| s.to_string()).collect();
        assert_eq!(recorded, Some((jpg.clone(), want.clone())), "{name}");
        assert_eq!(lib.edits(*id), want, "{name}");
        assert!(!lib.s.library().has_unsaved_edits(*id) && lib.s.library().saved(*id).is_none(), "{name}");
    }
    // A TIFF turned by its recipe: the copy holds the turned pixels.
    let turned = lib.s.library().photo(lib.id("tiff16.jpg")).unwrap().clone();
    assert_eq!((turned.width, turned.height), (80, 120));
    for cmd in ["photo.undoSave", "photo.revert"] {
        let params = json!({ "ids": ids, "confirm": true });
        let params = if cmd == "photo.undoSave" { json!({ "ids": ids }) } else { params };
        assert_eq!(lib.s.execute(cmd, &params), Err(CmdError::Disabled(NOTHING_SAVED.into())), "{cmd}: saves in place only");
    }

    // Another edit and Save: the next free name, the first copy untouched.
    lib.settle(); // the copies' owned fields, written back after a debounce
    let first = read(&lib.path("tiff8.jpg"));
    let tiff = lib.edit("tiff8.tif", &["autofix()"]);
    assert!(lib.s.library().has_unsaved_edits(tiff));
    lib.save(&[tiff]);
    assert_eq!(read(&lib.path("tiff8.jpg")), first);
    assert!(lib.has("tiff8-001.jpg"));
    assert_eq!(lib.s.library().saved_as(tiff).map(|c| c.name.as_str()), Some("tiff8-001.jpg"));
    assert_eq!(read(&lib.path("tiff8.tif")), before[1], "the source is still byte-identical");
}

// --- formats that cannot be saved ---------------------------------------------------------------

/// AVIF (never decoded) cannot be saved, nor HEIC in a build without the `heif` feature:
/// `photo.save` is disabled up front with the reason when every photo named is such a file, and
/// beside a photo that can be saved such a file is left out with the reason while the other is
/// saved. Nothing of it is written or kept, and it keeps its edits. A video (listed since M15.11)
/// is refused the same way: Save never writes to a video file.
#[test]
fn avif_and_video_cannot_be_saved_and_nothing_is_written() {
    #[allow(unused_mut)]
    let mut refused = vec![("still.avif", avif(), "AVIF files can't be displayed"), ("clip.mp4", mp4(), "This video can't be played")];
    #[cfg(not(feature = "heif"))]
    refused.push(("iphone.heic", heic::build(64, 48), "HEIC support isn't included in this build"));
    let mut files: Vec<(&str, Vec<u8>)> = refused.iter().map(|(name, bytes, _)| (*name, bytes.clone())).collect();
    files.push(("fine.jpg", camera(51, 160, 120, 85)));
    let mut lib = Lib::new(&files);
    let fine = lib.edit("fine.jpg", &[TUNING]);
    let mut ids = Vec::new();
    // A video takes only a rotation (M15.16): its edit is one.
    let op = |name: &str| if name.ends_with(".mp4") { ROTATE } else { TUNING };
    for (name, _, why) in &refused {
        let id = lib.edit(name, &[op(name)]);
        let reason = format!("{why}, so Dalica cannot save its edits");
        // Alone: disabled up front with the reason, no job started.
        assert_eq!(lib.s.execute("photo.save", &json!({ "ids": [id], "confirm": true })), Err(CmdError::Disabled(reason)), "{name}");
        assert!(!lib.s.file_jobs_running(), "{name}");
        ids.push(id);
    }
    assert!(matches!(lib.s.execute("photo.save", &json!({ "ids": ids, "confirm": true })), Err(CmdError::Disabled(_))), "all of them: disabled");

    // Beside a photo that can be saved: left out with the reason, the other saved.
    let mut all = ids.clone();
    all.push(fine);
    let done = lib.job("photo.save", json!({ "ids": all, "confirm": true }));
    let n = refused.len();
    assert_eq!(done.label, format!("Saved 1 of {} photos; {n} failed", n + 1));
    let result = done.result.unwrap();
    assert_eq!(result["photos"], json!([fine]));
    let failed: Vec<Value> =
        refused.iter().map(|(name, _, why)| json!({ "file": lib.path(name), "reason": format!("{why}, so Dalica cannot save its edits") })).collect();
    assert_eq!(result["failed"], json!(failed));
    assert!(lib.s.library().can_undo_save(fine));
    for ((name, bytes, _), id) in refused.iter().zip(&ids) {
        assert_eq!(&read(&lib.path(name)), bytes, "{name}: untouched");
        assert_eq!(lib.edits(*id), [op(name)], "{name}: keeps its edits");
        assert!(lib.s.library().has_unsaved_edits(*id), "{name}: still unsaved");
        assert!(lib.s.library().saved(*id).is_none() && lib.s.library().saved_as(*id).is_none(), "{name}");
        assert!(!lib.kept(name).exists(), "{name}: nothing kept");
    }
    let mut want: Vec<String> = files.iter().map(|(name, _)| name.to_string()).collect();
    want.sort();
    assert_eq!(lib.names(), want, "nothing written beside");
}

// --- rotate-only Saves --------------------------------------------------------------------------

/// A rotate-only Save of an MCU-sized baseline JPEG turns its coefficients (orientation 1, the
/// same quality, the pixels the rotated original's); a partial-MCU size and a progressive file
/// keep their entropy-coded data byte for byte and take the orientation flag. Undo Save puts each
/// original back.
#[test]
fn rotate_only_saves_are_lossless_or_take_the_orientation_flag() {
    let files = vec![
        ("whole.jpg", camera(41, 320, 240, 85)),
        ("partial.jpg", camera(42, 330, 250, 85)),
        ("progressive.jpg", flat_jpeg(96, 64, 3, 0xc2, None, ycc_pattern)),
    ];
    let mut lib = Lib::new(&files);
    let ids: Vec<PhotoId> = files.iter().map(|(name, _)| lib.edit(name, &[ROTATE])).collect();
    lib.save(&ids);
    for ((name, original), id) in files.iter().zip(&ids) {
        assert_eq!(&read(&lib.kept(name)), original, "{name}: the original is kept");
        let photo = lib.s.library().photo(*id).unwrap().clone();
        let (w, h) = read_header(original, &Limits::default()).unwrap().display_size();
        assert_eq!((photo.width, photo.height), (h, w), "{name}: the photo is turned");
        let saved = read(&lib.path(name));
        let d = difference(name, &decode(&saved), &oracle(original, &[ROTATE], Purpose::InPlace));
        assert!(d < 1.5, "{name}: shows the rotated original (mean difference {d:.3})");
        assert_eq!(jpeg_quality::quality(&saved), jpeg_quality::quality(original), "{name}: its own tables");
    }
    let whole = read(&lib.path("whole.jpg"));
    assert_eq!(read_header(&whole, &Limits::default()).unwrap().orientation.get(), 1, "turned in its coefficients");
    assert_ne!(scan_data(&whole), scan_data(&files[0].1));
    for (name, original) in &files[1..] {
        let saved = read(&lib.path(name));
        assert_eq!(read_header(&saved, &Limits::default()).unwrap().orientation.get(), 6, "{name}: the orientation flag");
        assert_eq!(scan_data(&saved), scan_data(original), "{name}: the entropy-coded data byte for byte");
        assert_eq!(frame(&saved), frame(original), "{name}");
    }

    let done = lib.job("photo.undoSave", json!({ "ids": ids }));
    assert_eq!(done.state, JobState::Done);
    for (name, original) in &files {
        assert_eq!(&read(&lib.path(name)), original, "{name}: the exact original");
    }
}

// --- HEIC -----------------------------------------------------------------------------------------

mod heic {
    //! Synthetic HEIC files: 8-bit 4:2:0 grids of 32 × 32 coded pictures, each stored as PCM
    //! samples (H.265 7.3.8.7), so the HEVC stage reproduces them exactly. Trimmed from the writer
    //! in `crates/image/tests/heif.rs`, itself adapted from LightCraft
    //! `crates/heif/src/testdata.rs` at revision 76b53778 (MIT OR Apache-2.0, Copyright (c) 2026
    //! ArtCraft Team and the LightCraft contributors; `NOTICE`).

    use super::bx;

    /// Side of every coded picture.
    const TILE: u32 = 32;
    /// Main profile.
    const PROFILE: u32 = 1;

    #[derive(Default)]
    struct Bits {
        bytes: Vec<u8>,
        n: usize,
    }

    impl Bits {
        fn put(&mut self, v: u32, len: u32) {
            for i in (0..len).rev() {
                if self.n.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if (v >> i) & 1 == 1
                    && let Some(b) = self.bytes.last_mut()
                {
                    *b |= 0x80 >> (self.n % 8);
                }
                self.n += 1;
            }
        }

        fn ue(&mut self, v: u32) {
            let x = u64::from(v) + 1;
            let len = 64 - x.leading_zeros();
            self.put(0, len - 1);
            self.put((x & 0xffff_ffff) as u32, len);
        }

        fn align_zero(&mut self) {
            while !self.n.is_multiple_of(8) {
                self.put(0, 1);
            }
        }

        fn trailing(&mut self) {
            self.put(1, 1);
            self.align_zero();
        }
    }

    /// The CABAC arithmetic encoder of H.265 9.3.4.4, for the bins these pictures need.
    struct Cabac {
        low: u32,
        range: u32,
        first: bool,
        outstanding: u32,
    }

    impl Cabac {
        fn new() -> Cabac {
            Cabac { low: 0, range: 510, first: true, outstanding: 0 }
        }

        fn put_bit(&mut self, out: &mut Bits, b: u32) {
            if self.first {
                self.first = false;
            } else {
                out.put(b, 1);
            }
            while self.outstanding > 0 {
                out.put(1 - b, 1);
                self.outstanding -= 1;
            }
        }

        fn renorm(&mut self, out: &mut Bits) {
            while self.range < 256 {
                if self.low < 256 {
                    self.put_bit(out, 0);
                } else if self.low >= 512 {
                    self.low -= 512;
                    self.put_bit(out, 1);
                } else {
                    self.low -= 256;
                    self.outstanding += 1;
                }
                self.range <<= 1;
                self.low <<= 1;
            }
        }

        /// `part_mode` = PART_2Nx2N: the most probable symbol of a fresh context.
        fn part_mode_2nx2n(&mut self, out: &mut Bits) {
            let lps = [128, 176, 208, 240][((self.range >> 6) & 3) as usize];
            self.range -= lps;
            self.renorm(out);
        }

        /// A terminating bin equal to 1 (`pcm_flag`, `end_of_slice_segment_flag`), with the flush.
        fn terminate_one(&mut self, out: &mut Bits) {
            self.range -= 2;
            self.low += self.range;
            self.range = 2;
            self.renorm(out);
            self.put_bit(out, (self.low >> 9) & 1);
            out.put(((self.low >> 7) & 3) | 1, 2);
        }
    }

    fn nal(kind: u8, rbsp: &[u8]) -> Vec<u8> {
        let mut out = vec![kind << 1, 1];
        let mut zeros = 0;
        for &b in rbsp {
            if zeros >= 2 && b <= 3 {
                out.push(3);
                zeros = 0;
            }
            zeros = if b == 0 { zeros + 1 } else { 0 };
            out.push(b);
        }
        out
    }

    fn ptl(w: &mut Bits) {
        w.put(0, 2);
        w.put(0, 1);
        w.put(PROFILE, 5);
        w.put(1 << (31 - PROFILE), 32); // compatible with itself
        w.put(0b1001, 4); // progressive, frame only
        w.put(0, 22);
        w.put(0, 21);
        w.put(0, 1);
        w.put(93, 8); // level 3.1
    }

    fn parameter_sets() -> [Vec<u8>; 3] {
        let mut v = Bits::default();
        v.put(0, 4);
        v.put(3, 2);
        v.put(0, 6);
        v.put(0, 3);
        v.put(1, 1);
        v.put(0xffff, 16);
        ptl(&mut v);
        v.put(0, 1); // vps_sub_layer_ordering_info_present_flag
        v.ue(0);
        v.ue(0);
        v.ue(0);
        v.put(0, 6); // vps_max_layer_id
        v.ue(0); // vps_num_layer_sets_minus1
        v.put(0, 1); // vps_timing_info_present_flag
        v.put(0, 1); // vps_extension_flag
        v.trailing();

        let mut s = Bits::default();
        s.put(0, 4);
        s.put(0, 3);
        s.put(1, 1);
        ptl(&mut s);
        s.ue(0);
        s.ue(1); // 4:2:0
        s.ue(TILE);
        s.ue(TILE);
        s.put(0, 1); // conformance_window_flag
        s.ue(0); // 8-bit luma
        s.ue(0); // 8-bit chroma
        s.ue(4);
        s.put(1, 1);
        s.ue(0);
        s.ue(0);
        s.ue(0);
        s.ue(2); // log2_min_luma_coding_block_size_minus3: 32
        s.ue(0); // log2_diff_max_min_luma_coding_block_size: CTB 32
        s.ue(0); // log2_min_luma_transform_block_size_minus2: 4
        s.ue(3); // log2_diff_max_min_luma_transform_block_size: 32
        s.ue(0);
        s.ue(0);
        s.put(0, 1); // scaling_list_enabled_flag
        s.put(0, 1); // amp_enabled_flag
        s.put(0, 1); // sample_adaptive_offset_enabled_flag
        s.put(1, 1); // pcm_enabled_flag
        s.put(7, 4); // pcm_sample_bit_depth_luma_minus1
        s.put(7, 4); // pcm_sample_bit_depth_chroma_minus1
        s.ue(2); // log2_min_pcm_luma_coding_block_size_minus3: 32
        s.ue(0); // log2_diff_max_min_pcm_luma_coding_block_size
        s.put(1, 1); // pcm_loop_filter_disabled_flag
        s.ue(0); // num_short_term_ref_pic_sets
        s.put(0, 1); // long_term_ref_pics_present_flag
        s.put(0, 1); // sps_temporal_mvp_enabled_flag
        s.put(0, 1); // strong_intra_smoothing_enabled_flag
        s.put(0, 1); // vui_parameters_present_flag
        s.put(0, 1); // sps_extension_present_flag
        s.trailing();

        let mut p = Bits::default();
        p.ue(0);
        p.ue(0);
        p.put(0, 6); // dependent slices, output flag, 3 extra header bits, sign hiding
        p.put(0, 1); // cabac_init_present_flag
        p.ue(0);
        p.ue(0);
        p.ue(0); // init_qp_minus26 (se(0) is the same bits)
        p.put(0, 3); // constrained intra, transform skip, cu_qp_delta
        p.ue(0);
        p.ue(0);
        p.put(0, 1);
        p.put(0, 2);
        p.put(0, 1); // transquant_bypass_enabled_flag
        p.put(0, 1); // tiles_enabled_flag
        p.put(0, 1); // entropy_coding_sync_enabled_flag
        p.put(0, 1); // pps_loop_filter_across_slices_enabled_flag
        p.put(1, 1); // deblocking_filter_control_present_flag
        p.put(0, 1); // deblocking_filter_override_enabled_flag
        p.put(1, 1); // pps_deblocking_filter_disabled_flag
        p.put(0, 1);
        p.put(0, 1);
        p.ue(0);
        p.put(0, 1);
        p.put(0, 1);
        p.trailing();
        [nal(32, &v.bytes), nal(33, &s.bytes), nal(34, &p.bytes)]
    }

    /// One coded 32 × 32 picture of YCbCr samples `sample(x, y)`: the `hvcC` payload and the item
    /// data (a length-prefixed NAL unit).
    fn picture(sample: &dyn Fn(u32, u32) -> [u8; 3]) -> (Vec<u8>, Vec<u8>) {
        let mut w = Bits::default();
        w.put(1, 1); // first_slice_segment_in_pic_flag
        w.put(0, 1); // no_output_of_prior_pics_flag
        w.ue(0);
        w.ue(2); // I slice
        w.ue(0); // slice_qp_delta, se(0)
        w.trailing(); // byte_alignment()
        let mut c = Cabac::new();
        c.part_mode_2nx2n(&mut w);
        c.terminate_one(&mut w); // pcm_flag
        w.align_zero(); // pcm_alignment_zero_bit
        for y in 0..TILE {
            for x in 0..TILE {
                w.put(u32::from(sample(x, y)[0]), 8);
            }
        }
        for k in 1..3 {
            for y in 0..TILE / 2 {
                for x in 0..TILE / 2 {
                    w.put(u32::from(sample(2 * x, 2 * y)[k]), 8);
                }
            }
        }
        let mut c = Cabac::new();
        c.terminate_one(&mut w); // end_of_slice_segment_flag
        w.align_zero();
        let slice = nal(19, &w.bytes);

        let mut h = vec![1u8, PROFILE as u8];
        h.extend_from_slice(&(1u32 << (31 - PROFILE)).to_be_bytes());
        h.extend_from_slice(&[0x90, 0, 0, 0, 0, 0, 93, 0xf0, 0, 0xfc, 0xfd, 0xf8, 0xf8, 0, 0, 0x0f, 3]);
        for (kind, set) in [32u8, 33, 34].into_iter().zip(&parameter_sets()) {
            h.push(0x80 | kind);
            h.extend_from_slice(&1u16.to_be_bytes());
            h.extend_from_slice(&(set.len() as u16).to_be_bytes());
            h.extend_from_slice(set);
        }
        let mut data = (slice.len() as u32).to_be_bytes().to_vec();
        data.extend_from_slice(&slice);
        (h, data)
    }

    fn full_box(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![version, 0, 0, 0];
        p.extend_from_slice(payload);
        bx(kind, &p)
    }

    fn ispe(w: u32, h: u32) -> Vec<u8> {
        let mut p = w.to_be_bytes().to_vec();
        p.extend_from_slice(&h.to_be_bytes());
        full_box(b"ispe", 0, &p)
    }

    /// Samples with a position pattern: luma varied, chroma a gentle tint.
    fn sample(x: u32, y: u32) -> [u8; 3] {
        [((x * 7 + y * 3) % 200 + 20) as u8, (110 + x % 30) as u8, (150 - y % 30) as u8]
    }

    /// A `width` × `height` HEIC (a grid of coded pictures, full-range BT.601).
    pub fn build(width: u32, height: u32) -> Vec<u8> {
        let (cols, rows) = (width.div_ceil(TILE), height.div_ceil(TILE));
        // Properties: 1 hvcC, 2 the tiles' ispe, 3 the grid's ispe, 4 colr.
        let mut tiles = Vec::new();
        let mut hvcc = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                let (h, data) = picture(&|x, y| sample((c * TILE + x).min(width - 1), (r * TILE + y).min(height - 1)));
                hvcc = h;
                tiles.push(data);
            }
        }
        let mut colr = b"nclx".to_vec();
        for v in [1u16, 13, 6] {
            colr.extend_from_slice(&v.to_be_bytes());
        }
        colr.push(0x80);
        let props = [bx(b"hvcC", &hvcc), ispe(TILE, TILE), ispe(width, height), bx(b"colr", &colr)].concat();
        let grid_id = tiles.len() as u16 + 1;
        let mut grid = vec![0u8, 0, (rows - 1) as u8, (cols - 1) as u8];
        grid.extend_from_slice(&(width as u16).to_be_bytes());
        grid.extend_from_slice(&(height as u16).to_be_bytes());
        // Items 1..=n are the tiles (hidden), item n + 1 the grid.
        let items: Vec<(u16, &[u8; 4], Vec<u8>, bool)> = tiles
            .into_iter()
            .enumerate()
            .map(|(i, data)| (i as u16 + 1, b"hvc1", data, true))
            .chain(std::iter::once((grid_id, b"grid", grid, false)))
            .collect();

        let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
        let mut hdlr = vec![0u8; 4];
        hdlr.extend_from_slice(b"pict");
        hdlr.extend_from_slice(&[0; 13]);
        let hdlr = full_box(b"hdlr", 0, &hdlr);
        let pitm = full_box(b"pitm", 0, &grid_id.to_be_bytes());
        let mut iinf = (items.len() as u16).to_be_bytes().to_vec();
        for (id, kind, _, hidden) in &items {
            let mut e = vec![2, 0, 0, u8::from(*hidden)]; // version 2, flags
            e.extend_from_slice(&id.to_be_bytes());
            e.extend_from_slice(&[0, 0]);
            e.extend_from_slice(*kind);
            e.push(0);
            iinf.extend_from_slice(&bx(b"infe", &e));
        }
        let iinf = full_box(b"iinf", 0, &iinf);
        let mut dimg = grid_id.to_be_bytes().to_vec();
        dimg.extend_from_slice(&(grid_id - 1).to_be_bytes());
        for id in 1..grid_id {
            dimg.extend_from_slice(&id.to_be_bytes());
        }
        let iref = full_box(b"iref", 0, &bx(b"dimg", &dimg));
        let mut ipma = (items.len() as u32).to_be_bytes().to_vec();
        for (id, _, _, hidden) in &items {
            ipma.extend_from_slice(&id.to_be_bytes());
            if *hidden {
                ipma.extend_from_slice(&[2, 0x81, 2]);
            } else {
                ipma.extend_from_slice(&[2, 3, 0x84]);
            }
        }
        let iprp = bx(b"iprp", &[bx(b"ipco", &props), full_box(b"ipma", 0, &ipma)].concat());
        let iloc = |base: u32| {
            let mut l = vec![0x44, 0x00];
            l.extend_from_slice(&(items.len() as u16).to_be_bytes());
            let mut offset = base;
            for (id, _, data, _) in &items {
                l.extend_from_slice(&id.to_be_bytes());
                l.extend_from_slice(&[0, 0, 0, 0, 0, 1]); // construction method 0, data ref 0, one extent
                l.extend_from_slice(&offset.to_be_bytes());
                l.extend_from_slice(&(data.len() as u32).to_be_bytes());
                offset += data.len() as u32;
            }
            full_box(b"iloc", 1, &l)
        };
        let meta = |base| full_box(b"meta", 0, &[hdlr.clone(), pitm.clone(), iloc(base), iinf.clone(), iref.clone(), iprp.clone()].concat());
        let start = (ftyp.len() + meta(0).len() + 8) as u32;
        let mdat = bx(b"mdat", &items.iter().flat_map(|(_, _, data, _)| data.iter().copied()).collect::<Vec<u8>>());
        [ftyp, meta(start), mdat].concat()
    }

    #[test]
    fn the_writer_makes_a_heic_of_the_size_asked() {
        let file = build(64, 48);
        assert_eq!(dalica_image::sniff(&file), Some(dalica_image::Format::Heif));
        #[cfg(feature = "heif")]
        {
            let img = super::decode(&file);
            assert_eq!((img.width, img.height), (64, 48));
        }
    }
}
