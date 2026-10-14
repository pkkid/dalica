//! The search facts a scan reads (`organizing.md` §8.1–§8.2, `roadmap.md` M9.7) on temp folders:
//! camera make and model from a JPEG's, a PNG's, a TIFF's and a TIFF-structured RAW's EXIF; the
//! GPS location from the EXIF GPS IFD and from XMP (a JPEG's packet, a RAW's `.xmp` sidecar); a
//! file without EXIF, and a video file (not listed before M15), give none; truncated and hostile
//! blocks give no facts and never a panic; the diff carries the facts into the model as derived
//! data, only for new and changed files.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dalica_image::{Format, RawKind};
use dalica_library::{Facts, Library, MAX_CAMERA, Op};
use dalica_meta::xmp_sidecar::{self, SidecarWrite};
use dalica_meta::{Changes, Place, WriteOptions};
use dalica_scan::meta::sync_parts;
use dalica_scan::{FoundPhoto, Options, ReadMetadata, Scan, diff, scan_with};

// --- A small TIFF / EXIF writer (TIFF 6.0: little-endian header, IFDs, out-of-line values) ---

const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;
const BYTE: u16 = 1;

const TAG_MAKE: u16 = 0x010f;
const TAG_MODEL: u16 = 0x0110;
const TAG_GPS_IFD: u16 = 0x8825;
const TAG_DNG_VERSION: u16 = 0xc612;

/// One IFD entry: tag, type, count and its value's bytes (little-endian).
#[derive(Clone)]
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: Vec<u8>,
}

fn ascii(tag: u16, bytes: &[u8]) -> Entry {
    let mut value = bytes.to_vec();
    value.push(0);
    Entry { tag, kind: ASCII, count: value.len() as u32, value }
}

fn short(tag: u16, values: &[u16]) -> Entry {
    Entry { tag, kind: SHORT, count: values.len() as u32, value: values.iter().flat_map(|v| v.to_le_bytes()).collect() }
}

fn long(tag: u16, v: u32) -> Entry {
    Entry { tag, kind: LONG, count: 1, value: v.to_le_bytes().to_vec() }
}

fn rationals(tag: u16, values: &[(u32, u32)]) -> Entry {
    Entry {
        tag,
        kind: RATIONAL,
        count: values.len() as u32,
        value: values.iter().flat_map(|(n, d)| [n.to_le_bytes(), d.to_le_bytes()].concat()).collect(),
    }
}

/// The GPS IFD of a location in degrees, minutes and seconds (`N`/`S`, `E`/`W` references).
fn gps_ifd(lat: (u32, u32, u32), north: bool, lon: (u32, u32, u32), east: bool) -> Vec<Entry> {
    vec![
        ascii(1, if north { b"N" } else { b"S" }),
        rationals(2, &[(lat.0, 1), (lat.1, 1), (lat.2, 1)]),
        ascii(3, if east { b"E" } else { b"W" }),
        rationals(4, &[(lon.0, 1), (lon.1, 1), (lon.2, 1)]),
    ]
}

fn ifd_len(entries: &[Entry]) -> usize {
    2 + 12 * entries.len() + 4 + entries.iter().filter(|e| e.value.len() > 4).map(|e| e.value.len().next_multiple_of(2)).sum::<usize>()
}

/// Appends an IFD at `out.len()` with its out-of-line values after it, pointing to `next`.
fn write_ifd(out: &mut Vec<u8>, mut entries: Vec<Entry>, next: u32) {
    entries.sort_by_key(|e| e.tag);
    let start = out.len();
    let mut data_at = start + 2 + 12 * entries.len() + 4;
    let mut data = Vec::new();
    out.extend((entries.len() as u16).to_le_bytes());
    for e in &entries {
        out.extend(e.tag.to_le_bytes());
        out.extend(e.kind.to_le_bytes());
        out.extend(e.count.to_le_bytes());
        if e.value.len() <= 4 {
            let mut inline = e.value.clone();
            inline.resize(4, 0);
            out.extend(inline);
        } else {
            out.extend((data_at as u32).to_le_bytes());
            data.extend(&e.value);
            if e.value.len() % 2 == 1 {
                data.push(0);
            }
            data_at += e.value.len().next_multiple_of(2);
        }
    }
    out.extend(next.to_le_bytes());
    out.extend(data);
}

/// A TIFF structure: `pixels` right after the header (at offset 8), then IFD0 with `ifd0`, then
/// the GPS IFD (`gps`) with IFD0's pointer to it.
fn tiff(pixels: &[u8], ifd0: Vec<Entry>, gps: Option<Vec<Entry>>) -> Vec<u8> {
    tiff_after(&[], pixels, ifd0, gps)
}

/// [`tiff`] with `extra` between the 8-byte header and the pixels (a CR2's `CR\x02\0` and raw IFD
/// offset).
fn tiff_after(extra: &[u8], pixels: &[u8], mut ifd0: Vec<Entry>, gps: Option<Vec<Entry>>) -> Vec<u8> {
    let mut out = b"II*\0".to_vec();
    let ifd0_at = (8 + extra.len() + pixels.len()).next_multiple_of(2);
    out.extend((ifd0_at as u32).to_le_bytes());
    out.extend(extra);
    out.extend(pixels);
    out.resize(ifd0_at, 0);
    if gps.is_some() {
        ifd0.push(long(TAG_GPS_IFD, 0));
        let gps_at = ifd0_at + ifd_len(&ifd0);
        if let Some(e) = ifd0.last_mut() {
            e.value = (gps_at as u32).to_le_bytes().to_vec();
        }
    }
    write_ifd(&mut out, ifd0, 0);
    if let Some(gps) = gps {
        write_ifd(&mut out, gps, 0);
    }
    out
}

/// IFD0's camera entries.
fn camera(make: &[u8], model: &[u8]) -> Vec<Entry> {
    vec![ascii(TAG_MAKE, make), ascii(TAG_MODEL, model)]
}

/// An 8 × 8 RGB baseline TIFF (or, with `raw`, a TIFF-structured RAW recognised by its magic
/// number: a Canon CR2 header, DNGVersion in IFD0).
fn tiff_image(mut ifd0: Vec<Entry>, gps: Option<Vec<Entry>>, raw: bool) -> Vec<u8> {
    let pixels: Vec<u8> = (0..8 * 8 * 3).map(|i| (i * 7 % 251) as u8).collect();
    let extra: &[u8] = if raw { b"CR\x02\0\0\0\0\0" } else { b"" };
    ifd0.extend([
        long(256, 8),
        long(257, 8),
        short(258, &[8, 8, 8]),
        short(259, &[1]),
        short(262, &[2]),
        long(273, 8 + extra.len() as u32),
        short(277, &[3]),
        long(278, 8),
        long(279, pixels.len() as u32),
    ]);
    if raw {
        ifd0.push(Entry { tag: TAG_DNG_VERSION, kind: BYTE, count: 4, value: vec![1, 4, 0, 0] });
    }
    tiff_after(extra, &pixels, ifd0, gps)
}

fn plain_jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 64, 48), 85, None).unwrap()
}

/// `jpeg` with an APP1 segment (`prefix` then `body`) right after its SOI.
fn with_app1(jpeg: &[u8], prefix: &[u8], body: &[u8]) -> Vec<u8> {
    let len = 2 + prefix.len() + body.len();
    assert!(len <= 0xffff, "an APP1 segment holds at most 65 533 bytes");
    let mut out = jpeg[..2].to_vec();
    out.extend([0xff, 0xe1]);
    out.extend((len as u16).to_be_bytes());
    out.extend(prefix);
    out.extend(body);
    out.extend(&jpeg[2..]);
    out
}

/// A plain JPEG with this EXIF block (a TIFF structure).
fn exif_jpeg(seed: u64, exif: &[u8]) -> Vec<u8> {
    with_app1(&plain_jpeg(seed), b"Exif\0\0", exif)
}

/// The CRC-32 of PNG chunks (ISO 3309, the reflected 0xEDB88320 polynomial).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A PNG with an `eXIf` chunk holding `exif` right after its `IHDR`.
fn exif_png(seed: u64, exif: &[u8]) -> Vec<u8> {
    let png = dalica_scenes::png(&dalica_scenes::Scene::new(seed, 64, 48)).unwrap();
    // Signature (8) and IHDR (4 + 4 + 13 + 4).
    let at = 8 + 25;
    let mut chunk = (exif.len() as u32).to_be_bytes().to_vec();
    let mut body = b"eXIf".to_vec();
    body.extend(exif);
    chunk.extend(&body);
    chunk.extend(crc32(&body).to_be_bytes());
    [&png[..at], &chunk, &png[at..]].concat()
}

/// An XMP packet with only `exif:GPSLatitude` and `exif:GPSLongitude`.
fn gps_xmp(lat: &str, lon: &str) -> Vec<u8> {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\" exif:GPSLatitude=\"{lat}\" exif:GPSLongitude=\"{lon}\"/>\
         </rdf:RDF></x:xmpmeta>"
    )
    .into_bytes()
}

fn options() -> WriteOptions {
    WriteOptions { stars_as_ratings: true, ..WriteOptions::default() }
}

/// A minimal MP4: an `ftyp` box (brand `isom`) and an empty `mdat`.
fn mp4() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(24u32.to_be_bytes());
    out.extend(b"ftypisom");
    out.extend(0x200u32.to_be_bytes());
    out.extend(b"isommp41");
    out.extend(8u32.to_be_bytes());
    out.extend(b"mdat");
    out
}

const CR3: &[u8] = b"\0\0\0\x18ftypcrx \0\0\0\x01 and the rest of a Canon RAW file";

fn time(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(981_173_106 + n)
}

/// Writes `bytes` at `path` with modification time `time(n)`.
fn put(path: &Path, bytes: &[u8], n: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    std::fs::File::options().write(true).open(path).unwrap().set_modified(time(n)).unwrap();
}

fn reading() -> Options {
    Options { metadata: ReadMetadata::all(), ..Options::default() }
}

fn found<'a>(scan: &'a Scan, name: &str) -> &'a FoundPhoto {
    scan.folders.iter().flat_map(|f| &f.photos).find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed"))
}

/// The facts the scan read for `name` (none when its metadata could not be read).
fn facts_of(scan: &Scan, name: &str) -> Facts {
    found(scan, name).file.as_ref().map(|f| f.1.clone()).unwrap_or_default()
}

fn cam(make: &str, model: &str, geotagged: bool) -> Facts {
    Facts { make: make.into(), model: model.into(), video: false, geotagged }
}

fn lisbon() -> Vec<Entry> {
    gps_ifd((38, 42, 36), true, (9, 8, 24), false)
}

/// Every format the scan reads metadata of gives its facts; files without EXIF give none.
#[test]
fn jpeg_tiff_png_and_raw_files_give_their_camera_and_location() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Facts");
    // Camera and location in the EXIF block.
    put(&dir.join("camera.jpg"), &exif_jpeg(1, &tiff(&[], camera(b"Dalica", b"Scenes 1"), Some(lisbon()))), 0);
    // The camera only, as written by `scenes`.
    let exif = dalica_scenes::Exif { make: Some("NIKON CORPORATION".into()), model: Some("NIKON Z 6".into()), ..dalica_scenes::Exif::default() };
    put(&dir.join("scenes.jpg"), &dalica_scenes::jpeg(&dalica_scenes::Scene::new(2, 64, 48), 85, Some(&exif)).unwrap(), 0);
    put(&dir.join("plain.jpg"), &plain_jpeg(3), 0);
    put(&dir.join("plain.png"), &dalica_scenes::png(&dalica_scenes::Scene::new(4, 64, 48)).unwrap(), 0);
    put(&dir.join("camera.png"), &exif_png(5, &tiff(&[], camera(b"FUJIFILM", b"X100V"), None)), 0);
    put(&dir.join("camera.tif"), &tiff_image(camera(b"Phase One", b"IQ4"), Some(lisbon()), false), 0);
    let plain_tiff =
        dalica_scenes::formats::tiff::tiff(&dalica_scenes::Scene::new(6, 32, 24), &dalica_scenes::formats::tiff::Tiff::default()).unwrap();
    put(&dir.join("plain.tif"), &plain_tiff, 0);
    put(&dir.join("IMG_0001.CR2"), &tiff_image(camera(b"Canon", b"Canon EOS 5D Mark IV"), Some(lisbon()), true), 0);
    // A RAW whose container is not read for metadata: the location from its `.xmp` sidecar.
    put(&dir.join("IMG_0002.CR3"), CR3, 0);
    let place = Changes { place: Some(Place::new(38.71, -9.14)), ..Changes::default() };
    let Ok(SidecarWrite::Write(xmp)) = xmp_sidecar::merge(None, None, &place, &options()) else { panic!("a new sidecar") };
    put(&dir.join("IMG_0002.xmp"), &xmp, 0);
    put(&dir.join("IMG_0003.CR3"), CR3, 0);
    // A video: not a photo before M15.
    put(&dir.join("clip.mp4"), &mp4(), 0);

    let scan = scan_with(&[tmp.path().to_path_buf()], &reading());
    assert_eq!(facts_of(&scan, "camera.jpg"), cam("Dalica", "Scenes 1", true));
    assert_eq!(facts_of(&scan, "scenes.jpg"), cam("NIKON CORPORATION", "NIKON Z 6", false), "kept as read");
    assert_eq!(facts_of(&scan, "camera.png"), cam("FUJIFILM", "X100V", false));
    assert_eq!(facts_of(&scan, "camera.tif"), cam("Phase One", "IQ4", true));
    assert_eq!(found(&scan, "IMG_0001.CR2").format, Format::Raw(RawKind::TiffBased));
    assert_eq!(facts_of(&scan, "IMG_0001.CR2"), cam("Canon", "Canon EOS 5D Mark IV", true), "kept as read");
    assert_eq!(facts_of(&scan, "IMG_0002.CR3"), cam("", "", true), "the sidecar's XMP location");
    assert_eq!(found(&scan, "IMG_0002.CR3").format, Format::Raw(RawKind::Cr3));
    for name in ["plain.jpg", "plain.png", "plain.tif", "IMG_0003.CR3"] {
        assert!(found(&scan, name).file.is_some(), "{name} was read");
        assert_eq!(facts_of(&scan, name), Facts::default(), "{name}: no EXIF, no facts");
    }
    assert!(scan.folders.iter().flat_map(|f| &f.photos).all(|p| p.name != "clip.mp4"), "a video is not listed before M15");
    assert!(scan.folders.iter().flat_map(|f| &f.photos).all(|p| p.file.as_ref().is_none_or(|f| !f.1.video)));

    // Without metadata reading, nothing is read (facts included).
    let off = scan_with(&[tmp.path().to_path_buf()], &Options::default());
    assert!(off.folders.iter().flat_map(|f| &f.photos).all(|p| p.file.is_none()));
}

/// A RAW container `dalica-meta` does not read (CR3 here) takes its camera from its header's EXIF,
/// and its location from its `.xmp` sidecar; a JPEG's camera comes from its own metadata, never
/// from the header. (A planted header reader stands in for a real CR3's `CMT1` block.)
#[test]
fn a_raw_container_takes_its_camera_from_its_header() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Raws");
    put(&dir.join("IMG_0001.CR3"), CR3, 0);
    let place = Changes { place: Some(Place::new(38.71, -9.14)), ..Changes::default() };
    let Ok(SidecarWrite::Write(xmp)) = xmp_sidecar::merge(None, None, &place, &options()) else { panic!("a new sidecar") };
    put(&dir.join("IMG_0001.xmp"), &xmp, 0);
    put(&dir.join("IMG_0002.CR3"), CR3, 0);
    put(&dir.join("plain.jpg"), &plain_jpeg(1), 0);
    let reader: dalica_scan::HeaderReader = std::sync::Arc::new(|path: &Path, limits: &dalica_image::Limits| {
        let jpeg = path.extension().is_some_and(|e| e == "jpg");
        if jpeg {
            return dalica_image::read_header_file(path, limits).map(|(h, _)| h);
        }
        let exif =
            dalica_image::exif::Exif { make: Some(" Canon\n".into()), model: Some("Canon EOS R5".into()), ..dalica_image::exif::Exif::default() };
        let named = path.file_name().is_some_and(|n| n == "IMG_0001.CR3");
        Ok(dalica_image::Header {
            format: Format::Raw(RawKind::Cr3),
            width: 64,
            height: 48,
            orientation: dalica_image::Orientation::default(),
            exif: named.then_some(exif),
            exif_error: None,
            thumbnail: None,
        })
    });
    let options = Options { metadata: ReadMetadata::all(), reader, bytes_reader: None, ..Options::default() };
    let scan = scan_with(&[tmp.path().to_path_buf()], &options);
    assert_eq!(facts_of(&scan, "IMG_0001.CR3"), cam("Canon", "Canon EOS R5", true));
    assert_eq!(facts_of(&scan, "IMG_0002.CR3"), Facts::default(), "no EXIF in the header: none");
    assert!(found(&scan, "IMG_0002.CR3").file.is_some(), "read all the same");
    assert_eq!(facts_of(&scan, "plain.jpg"), Facts::default());
    // The diff carries them into the model.
    let mut lib = Library::new();
    for op in diff(&mut lib, &scan) {
        lib.apply(op).unwrap();
    }
    let id = lib.photo_by_name(lib.folder_by_path(&dir).unwrap(), "IMG_0001.CR3").unwrap().id;
    assert_eq!(lib.facts(id), &cam("Canon", "Canon EOS R5", true));
}

/// A location from the EXIF GPS IFD and one from XMP `exif:GPSLatitude` / `exif:GPSLongitude`
/// alone; a GPS IFD without a usable latitude and longitude is no location.
#[test]
fn a_location_comes_from_the_exif_gps_ifd_or_from_xmp() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Places");
    put(&dir.join("exif.jpg"), &exif_jpeg(1, &tiff(&[], vec![], Some(gps_ifd((48, 51, 29), true, (2, 17, 40), true)))), 0);
    let xmp = with_app1(&plain_jpeg(2), b"http://ns.adobe.com/xap/1.0/\0", &gps_xmp("38,42.6N", "9,8.4W"));
    put(&dir.join("xmp.jpg"), &xmp, 0);
    // Zero denominators, a latitude alone, an empty GPS IFD.
    let broken = vec![ascii(1, b"N"), rationals(2, &[(1, 0), (2, 0), (3, 0)]), ascii(3, b"E"), rationals(4, &[(1, 0), (0, 0), (0, 0)])];
    put(&dir.join("zero.jpg"), &exif_jpeg(3, &tiff(&[], vec![], Some(broken))), 0);
    put(&dir.join("half.jpg"), &exif_jpeg(4, &tiff(&[], vec![], Some(vec![ascii(1, b"N"), rationals(2, &[(10, 1), (0, 1), (0, 1)])]))), 0);
    put(&dir.join("empty.jpg"), &exif_jpeg(5, &tiff(&[], camera(b"Dalica", b""), Some(vec![]))), 0);
    let scan = scan_with(&[tmp.path().to_path_buf()], &reading());
    assert!(facts_of(&scan, "exif.jpg").geotagged);
    assert!(facts_of(&scan, "xmp.jpg").geotagged);
    for name in ["zero.jpg", "half.jpg"] {
        assert_eq!(facts_of(&scan, name), Facts::default(), "{name}");
    }
    assert_eq!(facts_of(&scan, "empty.jpg"), cam("Dalica", "", false));
}

/// Hostile values are made safe (control characters, overlong text); hostile and truncated blocks
/// give no facts; nothing panics, and the diff of all of it applies.
#[test]
fn truncated_and_hostile_blocks_give_no_facts_and_never_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Hostile");
    // Overlong and control characters: kept safe and capped.
    let overlong = vec![b'A'; 60_000];
    put(&dir.join("long.jpg"), &exif_jpeg(1, &tiff(&[], camera(&overlong, b"\x07Model\x1b\nX\t"), None)), 0);
    // Latin-1 bytes that are no UTF-8.
    put(&dir.join("latin1.jpg"), &exif_jpeg(2, &tiff(&[], camera(b"Citro\xebn", b"Caf\xe9"), None)), 0);
    // A looping IFD chain, a block cut inside IFD0, a GPS pointer past the end, a Make whose value
    // lies past the end, a count that overflows, no TIFF at all.
    let mut looping = tiff(&[], camera(b"Loop", b"Loop"), None);
    // IFD0 at 8: its count, two entries, then the next-IFD offset, pointed back at itself.
    let next = 8 + 2 + 2 * 12;
    looping[next..next + 4].copy_from_slice(&8u32.to_le_bytes());
    put(&dir.join("loop.jpg"), &exif_jpeg(3, &looping), 0);
    let short = tiff(&[], camera(b"Cut", b"Short"), None);
    put(&dir.join("short.jpg"), &exif_jpeg(8, &short[..20]), 0);
    let gps_far = {
        let mut ifd0 = camera(b"Far", b"Away");
        ifd0.push(long(TAG_GPS_IFD, 0x00ff_ffff));
        tiff(&[], ifd0, None)
    };
    put(&dir.join("gps_far.jpg"), &exif_jpeg(4, &gps_far), 0);
    let mut make_far = camera(b"", b"");
    make_far[0] = Entry { tag: TAG_MAKE, kind: ASCII, count: 64, value: vec![] };
    let mut bytes = tiff(&[], make_far, None);
    // Point the Make's 64-byte value far past the block.
    let entry = 8 + 2;
    bytes[entry + 8..entry + 12].copy_from_slice(&0x7fff_fff0u32.to_le_bytes());
    put(&dir.join("make_far.jpg"), &exif_jpeg(5, &bytes), 0);
    let mut overflow = tiff(&[], camera(b"Over", b"Flow"), None);
    overflow[entry + 4..entry + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    put(&dir.join("overflow.jpg"), &exif_jpeg(6, &overflow), 0);
    put(&dir.join("garbage.jpg"), &exif_jpeg(7, &[0xab; 300]), 0);
    // Truncated: a JPEG and a TIFF with camera and location cut every few bytes.
    let full_jpeg = exif_jpeg(8, &tiff(&[], camera(b"Dalica", b"Scenes 1"), Some(lisbon())));
    let full_tiff = tiff_image(camera(b"Dalica", b"Scenes 1"), Some(lisbon()), false);
    let cut = tmp.path().join("Cut");
    let mut cuts = Vec::new();
    for (ext, bytes) in [("jpg", &full_jpeg), ("tif", &full_tiff)] {
        for at in (0..bytes.len()).step_by((bytes.len() / 60).max(1)) {
            let name = format!("{at:05}.{ext}");
            put(&cut.join(&name), &bytes[..at], 0);
            cuts.push(name);
        }
    }

    let scan = scan_with(&[tmp.path().to_path_buf()], &reading());
    let long_facts = facts_of(&scan, "long.jpg");
    assert!(long_facts.make.len() <= MAX_CAMERA && long_facts.make.bytes().all(|b| b == b'A'), "capped: {}", long_facts.make.len());
    assert!(!long_facts.make.is_empty());
    assert_eq!(long_facts.model, "Model X");
    assert_eq!(facts_of(&scan, "latin1.jpg"), cam("Citroën", "Café", false));
    for name in ["loop.jpg", "short.jpg", "gps_far.jpg", "make_far.jpg", "overflow.jpg", "garbage.jpg"] {
        assert_eq!(facts_of(&scan, name), Facts::default(), "{name}: a hostile block gives no facts");
    }
    let whole = cam("Dalica", "Scenes 1", true);
    let (mut all, mut none) = (0, 0);
    for name in &cuts {
        let listed = scan.folders.iter().flat_map(|f| &f.photos).find(|p| &p.name == name);
        if let Some(p) = listed {
            let f = p.file.as_ref().map(|f| f.1.clone()).unwrap_or_default();
            assert!(f == whole || f == Facts::default(), "{name}: all or nothing, got {f:?}");
            if f == whole { all += 1 } else { none += 1 }
        }
    }
    assert!(all > 0 && none > 0, "cut in the image data: all; in the metadata: nothing ({all}, {none})");
    // And the diff of all of it applies (every camera within the library's cap).
    let mut lib = Library::new();
    for op in diff(&mut lib, &scan) {
        lib.apply(op).unwrap();
    }
    let id = lib.photo_by_name(lib.folder_by_path(&dir).unwrap(), "long.jpg").unwrap().id;
    assert_eq!(lib.facts(id), &long_facts);
}

/// The diff turns the facts read into the model's, in its sync op, as derived data: a first scan
/// sets them, a scan of unchanged files reads nothing and changes nothing, a file rewritten with
/// another camera updates them, and one whose EXIF is gone clears them.
#[test]
fn the_diff_brings_the_facts_into_the_model_for_new_and_changed_files_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Lisbon");
    put(&dir.join("a.jpg"), &exif_jpeg(1, &tiff(&[], camera(b"Dalica", b"Scenes 1"), Some(lisbon()))), 0);
    put(&dir.join("b.jpg"), &exif_jpeg(2, &tiff(&[], camera(b"Dalica", b"Scenes 2"), None)), 0);
    put(&dir.join("plain.jpg"), &plain_jpeg(3), 0);
    let roots = [tmp.path().to_path_buf()];
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan_with(&roots, &reading()));
    let sync = ops.last().unwrap();
    assert!(sync_parts(sync).is_some(), "the facts ride in the sync op");
    let revision = |lib: &Library| lib.revision();
    for op in ops {
        lib.apply(op).unwrap();
    }
    let id = |lib: &Library, name: &str| lib.photo_by_name(lib.folder_by_path(&dir).unwrap(), name).unwrap().id;
    assert_eq!(lib.facts(id(&lib, "a.jpg")), &cam("Dalica", "Scenes 1", true));
    assert_eq!(lib.facts(id(&lib, "b.jpg")), &cam("Dalica", "Scenes 2", false));
    assert_eq!(lib.facts(id(&lib, "plain.jpg")), &Facts::default());
    assert_eq!(lib.facts_all().count(), 2, "no facts, nothing stored");

    // Unchanged files: the model knows them; nothing read, nothing to change.
    let known =
        |lib: &Library| Options { metadata: ReadMetadata::NewOrChanged(std::sync::Arc::new(dalica_scan::Known::of(lib))), ..Options::default() };
    let again = scan_with(&roots, &known(&lib));
    assert!(again.folders.iter().flat_map(|f| &f.photos).all(|p| p.file.is_none()));
    assert!(diff(&mut lib.clone(), &again).is_empty());

    // Rewritten with another camera; another one without EXIF any more.
    put(&dir.join("a.jpg"), &exif_jpeg(1, &tiff(&[], camera(b"Dalica", b"Scenes 9"), Some(lisbon()))), 10);
    put(&dir.join("b.jpg"), &plain_jpeg(2), 10);
    let changed = scan_with(&roots, &known(&lib));
    let before = revision(&lib);
    let ops = diff(&mut lib, &changed);
    let Some(Op::Batch { ops: parts }) = ops.last().filter(|op| sync_parts(op).is_some()) else { panic!("a sync op last: {ops:?}") };
    assert!(matches!(parts.last(), Some(Op::SetFacts { facts }) if facts.len() == 2), "{parts:?}");
    for op in ops {
        lib.apply(op).unwrap();
    }
    assert!(revision(&lib) > before, "the files' sizes changed");
    assert_eq!(lib.facts(id(&lib, "a.jpg")), &cam("Dalica", "Scenes 9", true));
    assert_eq!(lib.facts(id(&lib, "b.jpg")), &Facts::default(), "cleared");
    assert_eq!(lib.facts_all().count(), 1);
    assert!(diff(&mut lib.clone(), &scan_with(&roots, &known(&lib))).is_empty(), "and then nothing more");
}
