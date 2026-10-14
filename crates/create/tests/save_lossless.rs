//! Lossless rotate-only Saves of JPEGs on temp folders (`data-model.md` §6, `formats.md` §1): a
//! rotated MCU-sized JPEG keeps its own quantisation tables (transposed with the blocks for a
//! quarter turn) and decodes to the rotated original within 1 level, with orientation 1 and a
//! turned EXIF thumbnail; a partial-MCU size, a file with data after its end and a recipe that
//! cancels the EXIF orientation keep their pixel data bytes and take the composed orientation (an
//! EXIF block added when there is none); a flip after a rotation; the original is kept and Undo
//! Save brings its exact bytes back; a later Save starts again from it; the owned fields are
//! merged; a damaged file is refused with nothing changed.

mod common;

use std::path::Path;

use common::*;
use dalica_core::Timestamp;
use dalica_create::save::lossless::{self, Method};
use dalica_create::save::{self, Error, Options, Saved, Saving};
use dalica_edit::Recipe;
use dalica_fs::TempDir;
use dalica_image::{Limits, Orientation, Rgba8, jpeg_quality, jpeg_transform, orient, read_header};
use dalica_meta::Changes;
use dalica_scenes::Exif;

const NAME: &str = "IMG_0001.jpg";
const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

/// The zig-zag order of an 8 × 8 block (ITU-T T.81 Figure A.6): position `k` of a DQT table
/// holds the quantiser of natural index `ZIGZAG[k]`.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43,
    36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

fn folder() -> TempDir {
    TempDir::new("dalica-lossless-test").unwrap()
}

fn options() -> Options {
    Options { threads: 2, ..Options::new(NOW) }
}

/// A camera-like JPEG of the scene: quality 80, stored for EXIF orientation `o`, with a 160-pixel
/// EXIF thumbnail.
fn camera(seed: u64, width: u32, height: u32, o: u8) -> Vec<u8> {
    let exif = Exif { orientation: o, thumbnail: Some(160), make: Some("Dalicam".into()), model: Some("Model 7".into()), ..Exif::default() };
    dalica_scenes::jpeg(&scene(seed, width, height), 80, Some(&exif)).unwrap()
}

fn read(path: impl AsRef<Path>) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

/// Saves `bytes` as `NAME` in `dir` with `ops` through the dispatch; the in-place result.
fn save_with(dir: &Path, bytes: &[u8], ops: &[&str]) -> Saved {
    std::fs::write(dir.join(NAME), bytes).unwrap();
    match save::save(dir, NAME, &recipe(ops), &Changes::default(), &options()).unwrap() {
        Saving::InPlace(saved) => saved,
        other => panic!("not saved in place: {other:?}"),
    }
}

/// The marker segments of a JPEG up to its first scan: (marker, the whole segment).
fn segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 2;
    while at + 4 <= bytes.len() {
        assert_eq!(bytes[at], 0xff, "a marker at {at}");
        let marker = bytes[at + 1];
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        out.push((marker, bytes[at..at + 2 + len].to_vec()));
        if marker == 0xda {
            break;
        }
        at += 2 + len;
    }
    out
}

/// The entropy-coded data and everything after it: from the end of the first scan header on.
fn scan_data(bytes: &[u8]) -> &[u8] {
    let end: usize = segments(bytes).iter().map(|(_, s)| s.len()).sum::<usize>() + 2;
    &bytes[end..]
}

/// The 8-bit quantisation tables by slot, in natural order.
fn tables(bytes: &[u8]) -> Vec<(u8, [u16; 64])> {
    let mut out = Vec::new();
    for (_, seg) in segments(bytes).into_iter().filter(|(m, _)| *m == 0xdb) {
        let mut body = &seg[4..];
        while !body.is_empty() {
            assert_eq!(body[0] >> 4, 0, "8-bit tables");
            let mut table = [0u16; 64];
            for k in 0..64 {
                table[ZIGZAG[k]] = u16::from(body[1 + k]);
            }
            out.push((body[0] & 15, table));
            body = &body[65..];
        }
    }
    out.sort_by_key(|(slot, _)| *slot);
    out
}

fn transposed(table: &[u16; 64]) -> [u16; 64] {
    std::array::from_fn(|i| table[(i % 8) * 8 + i / 8])
}

/// The EXIF orientation of the net transform of `ops` on a file with orientation `exif`.
fn net(exif: u8, ops: &[&str]) -> Orientation {
    recipe(ops).display(Orientation::new(exif).unwrap()).to_exif()
}

/// The decoder's samples before colour conversion (Y, Cb, Cr upsampled), packed as RGBA so
/// `orient::apply` turns them.
fn ycc(bytes: &[u8]) -> Rgba8 {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::YCbCr);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let samples = d.decode().unwrap();
    let info = d.info().unwrap();
    let data = samples.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    Rgba8 { width: u32::from(info.width), height: u32::from(info.height), data }
}

/// The APP segments other than APP1 (JFIF, ICC, IPTC, …) and comments.
fn metadata_segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    segments(bytes).into_iter().filter(|(m, _)| matches!(m, 0xe0 | 0xe2..=0xef | 0xfe)).collect()
}

/// The positions in the EXIF segment where `new` differs from `original`, outside the original's
/// thumbnail span; the two segments have the same length and the thumbnail changed.
fn exif_changes(new: &[u8], original: &[u8]) -> Vec<usize> {
    let exif = |b: &[u8]| segments(b).into_iter().find(|(m, s)| *m == 0xe1 && s[4..].starts_with(b"Exif\0\0")).unwrap().1;
    let (a, b) = (exif(new), exif(original));
    assert_eq!(a.len(), b.len(), "the EXIF segment keeps its size");
    let at = original.windows(b.len()).position(|w| w == b).unwrap();
    let thumb = read_header(original, &Limits::default()).unwrap().thumbnail.unwrap();
    let thumb = thumb.start - at..thumb.end - at;
    assert_ne!(a[thumb.clone()], b[thumb.clone()], "the thumbnail changed");
    (0..a.len()).filter(|&i| a[i] != b[i] && !thumb.contains(&i)).collect()
}

/// The EXIF thumbnail of `bytes`, decoded.
fn thumbnail(bytes: &[u8]) -> Rgba8 {
    let header = read_header(bytes, &Limits::default()).unwrap();
    decode(header.thumbnail(bytes).expect("an EXIF thumbnail"))
}

#[test]
fn a_rotated_mcu_sized_jpeg_keeps_its_tables_and_decodes_to_the_rotated_original() {
    let tmp = folder();
    let dir = tmp.path();
    let original = camera(21, 320, 240, 1);
    let saved = save_with(dir, &original, &["rotate(90)"]);
    let bytes = read(dir.join(NAME));
    assert!(saved.first);
    assert_eq!(read(dir.join(".dalica-originals").join(NAME)), original, "the original is kept byte for byte");
    assert_eq!(saved.entry.recipe, ["rotate(90)"]);
    assert_eq!((saved.report.width, saved.report.height), (240, 320));

    let header = read_header(&bytes, &Limits::default()).unwrap();
    assert_eq!(header.orientation, Orientation::UPRIGHT);
    assert_eq!((header.width, header.height), (240, 320), "the stored pixels are turned");

    // The coefficients are the original's, moved: nothing was requantised.
    let o = net(1, &["rotate(90)"]);
    let limits = Limits::default();
    let expected = jpeg_transform::read(&original, &limits).unwrap().transformed(o).unwrap();
    assert_eq!(jpeg_transform::read(&bytes, &limits).unwrap(), expected);
    // A quarter turn transposes each table along with the blocks.
    let before = tables(&original);
    let after = tables(&bytes);
    assert_eq!(after.len(), before.len());
    for ((slot_a, a), (slot_b, b)) in after.iter().zip(&before) {
        assert_eq!(slot_a, slot_b);
        assert_eq!(*a, transposed(b), "table {slot_a} transposed");
    }
    assert_eq!(jpeg_quality::quality(&bytes), jpeg_quality::quality(&original), "the same quality");
    assert_eq!(saved.report.source_quality, jpeg_quality::quality(&original));

    // The decoder's samples within 1 level of the rotated original's (its inverse DCT rounds its
    // row and column passes differently once they swap); after the colour conversion within 3.
    let (max, mean) = difference(&ycc(&bytes), &orient::apply(ycc(&original), o).unwrap());
    assert!(max <= 1, "within 1 level of the rotated original: {max} (mean {mean:.4})");
    let (max, mean) = difference(&decode(&bytes), &orient::apply(decode(&original), o).unwrap());
    assert!(max <= 3 && mean < 0.05, "the pixels: {max} (mean {mean:.4})");

    // The EXIF thumbnail is turned with the picture, in the span the old one had.
    let old = thumbnail(&original);
    let new = thumbnail(&bytes);
    assert_eq!((new.width, new.height), (old.height, old.width));
    let p = psnr(&new, &orient::apply(old, o).unwrap());
    assert!(p > 30.0, "the thumbnail shows the turned picture: {p:.1} dB");
    let span = |b: &[u8]| read_header(b, &limits).unwrap().thumbnail.unwrap();
    assert_eq!(span(&bytes).len(), span(&original).len());
    assert!(saved.report.thumbnail);

    // Every other metadata segment is the original's; EXIF differs only in the thumbnail's bytes
    // (the orientation was 1 and stays 1; the flip test below changes it).
    assert_eq!(metadata_segments(&bytes), metadata_segments(&original));
    assert_eq!(exif_changes(&bytes, &original), Vec::<usize>::new(), "only the thumbnail changes");
}

#[test]
fn a_half_turn_copies_the_tables_byte_for_byte() {
    let tmp = folder();
    let original = camera(22, 320, 240, 1);
    save_with(tmp.path(), &original, &["rotate(180)"]);
    let bytes = read(tmp.path().join(NAME));
    let dqt = |b: &[u8]| segments(b).into_iter().filter(|(m, _)| *m == 0xdb).collect::<Vec<_>>();
    assert_eq!(dqt(&bytes), dqt(&original));
    let (max, _) = difference(&decode(&bytes), &orient::apply(decode(&original), net(1, &["rotate(180)"])).unwrap());
    assert!(max <= 1, "{max}");
}

#[test]
fn a_partial_mcu_size_keeps_its_pixel_data_and_gets_the_composed_orientation() {
    let tmp = folder();
    let dir = tmp.path();
    // 330 × 250 is not a whole number of 16 × 16 MCUs: a quarter turn would move the partial
    // bottom row to the left edge.
    let original = camera(23, 330, 250, 1);
    let turned = lossless::turn(&original, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap().unwrap();
    assert_eq!(turned.method, Method::Orientation);

    let saved = save_with(dir, &original, &["rotate(90)"]);
    let bytes = read(dir.join(NAME));
    assert_eq!(scan_data(&bytes), scan_data(&original), "the pixel data bytes are kept");
    let header = read_header(&bytes, &Limits::default()).unwrap();
    assert_eq!(header.orientation, Orientation::new(6).unwrap(), "a quarter turn clockwise");
    assert_eq!(header.display_size(), (250, 330));
    assert_eq!((saved.report.width, saved.report.height), (250, 330));
    assert_eq!(read(dir.join(".dalica-originals").join(NAME)), original);
    assert_eq!(decode(&bytes), oracle(&original, &recipe(&["rotate(90)"])), "shown as the edit showed it");
    // The thumbnail is untouched: readers turn it by the same tag.
    assert_eq!(header.thumbnail(&bytes), read_header(&original, &Limits::default()).unwrap().thumbnail(&original));
}

#[test]
fn a_jpeg_without_exif_gets_a_block_holding_the_orientation() {
    let tmp = folder();
    let original = dalica_scenes::jpeg(&scene(24, 330, 250), 85, None).unwrap();
    assert!(read_header(&original, &Limits::default()).unwrap().exif.is_none());
    save_with(tmp.path(), &original, &["rotate(270)"]);
    let bytes = read(tmp.path().join(NAME));
    assert_eq!(scan_data(&bytes), scan_data(&original));
    assert_eq!(read_header(&bytes, &Limits::default()).unwrap().orientation, Orientation::new(8).unwrap());
    assert_eq!(decode(&bytes), oracle(&original, &recipe(&["rotate(270)"])));

    // Turned in the DCT domain, it needs no tag and gets no EXIF block.
    let whole = dalica_scenes::jpeg(&scene(24, 320, 240), 85, None).unwrap();
    let turned = lossless::turn(&whole, &recipe(&["rotate(270)"]), &Changes::default(), &options()).unwrap().unwrap();
    assert_eq!(turned.method, Method::Transformed);
    let header = read_header(&turned.bytes, &Limits::default()).unwrap();
    assert!(header.exif.is_none());
    assert_eq!((header.width, header.height, header.orientation), (240, 320, Orientation::UPRIGHT));
}

#[test]
fn a_flip_after_a_rotation_is_lossless_too() {
    let tmp = folder();
    let ops = ["rotate(90)", "flip_h()"];
    // Stored for orientation 6, so the net transform composes the file's and the recipe's.
    let original = camera(25, 320, 240, 6);
    let o = net(6, &ops);
    let turned = lossless::turn(&original, &recipe(&ops), &Changes::default(), &options()).unwrap().unwrap();
    assert_eq!(turned.method, Method::Transformed);
    save_with(tmp.path(), &original, &ops);
    let bytes = read(tmp.path().join(NAME));
    assert_eq!(read_header(&bytes, &Limits::default()).unwrap().orientation, Orientation::UPRIGHT);
    let limits = Limits::default();
    assert_eq!(jpeg_transform::read(&bytes, &limits).unwrap(), jpeg_transform::read(&original, &limits).unwrap().transformed(o).unwrap());
    let (max, mean) = difference(&ycc(&bytes), &orient::apply(ycc(&original), o).unwrap());
    assert!(max <= 1, "{max} (mean {mean:.4})");
    let (max, mean) = difference(&decode(&bytes), &oracle(&original, &recipe(&ops)));
    assert!(max <= 3 && mean < 0.05, "as the edit showed it: {max} (mean {mean:.4})");
    let new = thumbnail(&bytes);
    let p = psnr(&new, &orient::apply(thumbnail(&original), o).unwrap());
    assert!(p > 30.0, "{p:.1} dB");
    // Orientation 6 became 1: one byte of IFD0's entry, and nothing else outside the thumbnail.
    assert_eq!(metadata_segments(&bytes), metadata_segments(&original));
    assert_eq!(exif_changes(&bytes, &original).len(), 1);
}

#[test]
fn a_recipe_that_undoes_the_exif_orientation_sets_it_to_1() {
    let tmp = folder();
    // Orientation 6 shows the stored pixels a quarter turn clockwise; three more make a full turn.
    let original = camera(26, 320, 240, 6);
    assert_eq!(net(6, &["rotate(270)"]), Orientation::UPRIGHT);
    let saved = save_with(tmp.path(), &original, &["rotate(270)"]);
    let bytes = read(tmp.path().join(NAME));
    assert_eq!(scan_data(&bytes), scan_data(&original), "nothing to move");
    let header = read_header(&bytes, &Limits::default()).unwrap();
    assert_eq!(header.orientation, Orientation::UPRIGHT);
    assert_eq!((saved.report.width, saved.report.height), (header.width, header.height));
}

#[test]
fn data_after_the_end_of_the_image_takes_the_orientation_fallback() {
    let tmp = folder();
    let mut original = camera(27, 320, 240, 1);
    original.extend_from_slice(b"\0\0a secondary image would be here");
    save_with(tmp.path(), &original, &["rotate(90)"]);
    let bytes = read(tmp.path().join(NAME));
    assert_eq!(scan_data(&bytes), scan_data(&original), "the image data and the trailer are kept");
    assert_eq!(read_header(&bytes, &Limits::default()).unwrap().orientation, Orientation::new(6).unwrap());
}

#[test]
fn undo_save_brings_the_original_back_and_a_later_save_starts_from_it() {
    let tmp = folder();
    let dir = tmp.path();
    let original = camera(28, 320, 240, 1);
    save_with(dir, &original, &["rotate(90)"]);

    let later = Options { now: Timestamp::from_millis(NOW.as_millis() + 1000), ..options() };
    let again = match save::save(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &later).unwrap() {
        Saving::InPlace(saved) => saved,
        other => panic!("{other:?}"),
    };
    assert!(!again.first);
    assert_eq!(again.entry.recipe, ["rotate(90)", "rotate(90)"], "the whole recipe is recorded");
    let bytes = read(dir.join(NAME));
    let limits = Limits::default();
    let half = jpeg_transform::read(&original, &limits).unwrap().transformed(net(1, &["rotate(180)"])).unwrap();
    assert_eq!(jpeg_transform::read(&bytes, &limits).unwrap(), half, "turned from the kept original");
    assert_eq!(read(dir.join(".dalica-originals").join(NAME)), original);

    let undone = save::undo(dir, NAME, &later).unwrap();
    assert_eq!(undone.recipe.as_deref(), Some(&["rotate(90)".to_string(), "rotate(90)".to_string()][..]));
    assert_eq!(read(dir.join(NAME)), original, "the exact original bytes");
    assert!(!dir.join(".dalica-originals").join(NAME).exists());
}

#[test]
fn the_owned_fields_are_merged() {
    let tmp = folder();
    let dir = tmp.path();
    let original = camera(29, 320, 240, 1);
    std::fs::write(dir.join(NAME), &original).unwrap();
    let changes = Changes { caption: Some(Some("Harbour at dawn".into())), tags: Some(vec!["boats".into()]), ..Changes::default() };
    save::save(dir, NAME, &recipe(&["rotate(90)"]), &changes, &options()).unwrap();
    let bytes = read(dir.join(NAME));
    let meta = dalica_meta::read(&bytes, &dalica_meta::Limits::default()).unwrap();
    assert_eq!(meta.owned.caption.as_deref(), Some("Harbour at dawn"));
    assert_eq!(meta.owned.tags, ["boats"]);
    let limits = Limits::default();
    let expected = jpeg_transform::read(&original, &limits).unwrap().transformed(net(1, &["rotate(90)"])).unwrap();
    assert_eq!(jpeg_transform::read(&bytes, &limits).unwrap(), expected, "still lossless");
}

#[test]
fn other_recipes_and_formats_are_not_lossless() {
    let jpeg = camera(30, 64, 48, 1);
    let opts = options();
    let none = |bytes: &[u8], ops: &[&str]| lossless::turn(bytes, &recipe(ops), &Changes::default(), &opts).unwrap().is_none();
    assert!(none(&jpeg, &["rotate(90)", "autofix()"]));
    assert!(none(&jpeg, &[]));
    assert!(none(&png(30, 64, 48), &["rotate(90)"]));
    assert!(!lossless::applies(&Recipe::default()));

    // The dispatch renders such a recipe: the file is re-encoded, not turned.
    let tmp = folder();
    let saved = save_with(tmp.path(), &camera(30, 320, 240, 1), &["rotate(90)", "autofix()"]);
    assert_eq!(saved.entry.recipe, ["rotate(90)", "autofix()"]);
}

#[test]
fn a_damaged_jpeg_is_refused_with_nothing_changed() {
    let tmp = folder();
    let dir = tmp.path();
    let full = camera(31, 320, 240, 1);
    // Cut inside the entropy-coded data.
    let cut = full.len() - scan_data(&full).len() / 2;
    let damaged = full[..cut].to_vec();
    std::fs::write(dir.join(NAME), &damaged).unwrap();
    let err = save::save(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    assert!(matches!(err, Error::Output(_)), "{err:?}");
    assert_eq!(read(dir.join(NAME)), damaged);
    assert!(!dir.join(".dalica-originals").join(NAME).exists());
}

#[test]
fn a_24_megapixel_turn_is_timed() {
    let original = camera(32, 6000, 4000, 1);
    let start = std::time::Instant::now();
    let turned = lossless::turn(&original, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap().unwrap();
    let elapsed = start.elapsed();
    assert_eq!(turned.method, Method::Transformed);
    assert_eq!((turned.width, turned.height), (4000, 6000));
    println!("lossless quarter turn of a 6000 × 4000 JPEG ({} bytes): {elapsed:.2?}", original.len());
}
