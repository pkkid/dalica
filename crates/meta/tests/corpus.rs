//! The metadata corpus test (`docs/development.md` §11, `docs/xmp-interop.md` §8), compiled only
//! with the `corpus` feature and run by `cargo xtask test-corpus -p dalica-meta`: the files of
//! `corpus/metadata/` (our own `dalica-scenes` photos after exiv2, ImageMagick and Pillow wrote
//! captions, keywords, ratings, GPS, dates, face regions and foreign metadata into them, plus RAW
//! `.xmp` sidecars) and the JPEGs of `corpus/photos/` (real camera and phone files: maker notes,
//! MPF secondary images, data after the end of the image), fetched by `cargo xtask corpus --all`.
//!
//! The set is checked against its manifest `xtask/corpus/metadata.sha256` first: a missing or stale
//! file is a failure ("run `cargo xtask corpus --all`"), never a skip. Then:
//!
//! - every file reads without a problem (`dalica_meta::read`, or `xmp_sidecar::read` for `.xmp`);
//! - every owned field is written (set and cleared, star and unstar) with `write_owned`, and each
//!   write that changes the file must pass `verify`, keep every byte outside the rewritable spans
//!   (the payload digest equal; for TIFF, written append-only, every such byte still at its
//!   offset), keep every foreign EXIF, IPTC, XMP and text row of the listing, leave the other
//!   owned fields as they were, and give the same bytes when written again;
//! - a JPEG write also keeps the image data (from the first SOS to the primary image's EOI) and
//!   everything after that EOI byte for byte, and every image its MPF index lists other than the
//!   primary still starts with FFD8 at the offset the index gives and is the image it was
//!   ([`jpeg_kept`]); each MPF file also proves the check: the same file with a segment slipped in
//!   after the MPF index (the secondary images moved, the offsets not) must fail it;
//! - RAW sidecars go through `xmp_sidecar::merge` under the same rules (foreign XMP rows kept, the
//!   change read back, writing again changes nothing), and the DNG-like file pairs with its sidecar.
//!
//! The expected refusals (`xmp-interop.md` §4) are counted as refusals, not failures: a RAW (the
//! DNG-like file) is never written, a TIFF's place or date cannot be written where its EXIF
//! holds the value or no XMP mirror exists, and a JPEG's place or date cannot be written where
//! its EXIF block has no room left in its APP1 segment (`xmp-interop.md` §4, *Size limits*: some
//! cameras leave only a few bytes of the 65,533 free). Any other error, or a write that breaks a rule above, is
//! a failure. The counts must reach the floors in `tests/corpus_floors.json`, which only ever rise.
#![cfg(feature = "corpus")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dalica_core::{Date, DateTime};
use dalica_meta::exif::IfdKind;
use dalica_meta::xmp_sidecar::{self, SidecarWrite, XmpFrom};
use dalica_meta::{Changes, Container, DateTaken, Error, Limits, OwnedFields, Place, WriteOptions};

/// The workspace root (this crate is `crates/meta`).
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

type Floors = BTreeMap<String, BTreeMap<String, u64>>;

fn floors() -> Floors {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("corpus_floors.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The sets this crate tests.
const SETS: &[&str] = &["metadata", "photos"];

/// Whether `set` holds `rel` for this test: every file of `metadata`, the JPEGs of `photos` (its
/// TIFF, PNG and WebP files are pixel-format samples for `dalica-image`).
fn measured(set: &str, rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    set != "photos" || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
}

/// Counts that are printed but have no floor: the expected refusals and the writes that change
/// nothing are not quality measures (a refusal that turns into a checked write raises `written`,
/// whose floor catches the reverse).
const UNFLOORED: &[&str] = &["refused_raw", "refused_tiff_place_date", "refused_exif_full", "unchanged", "sidecar_unchanged"];

// ---------------------------------------------------------------------------------------------
// Manifests and verification

/// `(sha256, relative path)` entries, or `None` when the set has no manifest (not published yet).
fn manifest(set: &str) -> Option<Vec<(String, String)>> {
    let path = root().join("xtask").join("corpus").join(format!("{set}.sha256"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => panic!("{}: {e}", path.display()),
    };
    let entries = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (h, p) = l.split_once("  ").unwrap_or_else(|| panic!("{}: bad line {l:?}", path.display()));
            assert!(h.len() == 64 && !p.split('/').any(|c| c.is_empty() || c == ".."), "{}: bad line {l:?}", path.display());
            (h.to_string(), p.to_string())
        })
        .collect();
    Some(entries)
}

/// SHA-256 (FIPS 180-4) of `data`, lower-case hex: the manifests' check, without a dependency.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut tail = data.as_chunks::<64>().1.to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in data.as_chunks::<64>().0.iter().chain(tail.as_chunks::<64>().0) {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let t1 = hh
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) ^ (a & c) ^ (b & c));
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[test]
fn sha256_known_answers() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

// ---------------------------------------------------------------------------------------------
// The edits, and what a write may change

fn dt(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> DateTime {
    DateTime::new(Date::new(y, mo, d).unwrap(), h, mi, s).unwrap()
}

/// What each edit asks for (`xmp-interop.md` §8 test 1: Unicode, `/` and `,` in tags, a southern
/// and eastern place, a date with an offset and a fraction).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    Caption,
    Tags,
    Star,
    Unstar,
    Place,
    Date,
    ClearCaption,
    ClearTags,
    ClearPlace,
}

const EDITS: [Edit; 9] =
    [Edit::Caption, Edit::Tags, Edit::Star, Edit::Unstar, Edit::Place, Edit::Date, Edit::ClearCaption, Edit::ClearTags, Edit::ClearPlace];

fn caption() -> String {
    "Dalica wrote this caption — ✓ Ünïcode, 東京".to_string()
}

fn tags() -> Vec<String> {
    vec!["dalica".into(), "Zürich/Altstadt".into(), "a, b".into()]
}

fn place() -> Place {
    Place::new(-33.856_8, 151.215_3).unwrap()
}

fn date() -> DateTaken {
    DateTaken::new(dt(2020, 2, 29, 12, 34, 56), Some(-180), Some("125")).unwrap()
}

impl Edit {
    fn changes(self) -> Changes {
        match self {
            Edit::Caption => Changes { caption: Some(Some(caption())), ..Changes::default() },
            Edit::Tags => Changes { tags: Some(tags()), ..Changes::default() },
            Edit::Star => Changes { star: Some(true), ..Changes::default() },
            Edit::Unstar => Changes { star: Some(false), ..Changes::default() },
            Edit::Place => Changes { place: Some(Some(place())), ..Changes::default() },
            Edit::Date => Changes { date_taken: Some(date()), ..Changes::default() },
            Edit::ClearCaption => Changes { caption: Some(None), ..Changes::default() },
            Edit::ClearTags => Changes { tags: Some(vec![]), ..Changes::default() },
            Edit::ClearPlace => Changes { place: Some(None), ..Changes::default() },
        }
    }

    /// The owned fields other than the edited one, which the write must leave as they were.
    fn others(self, o: &OwnedFields) -> OwnedFields {
        let mut o = o.clone();
        match self {
            Edit::Caption | Edit::ClearCaption => o.caption = None,
            Edit::Tags | Edit::ClearTags => o.tags.clear(),
            Edit::Star | Edit::Unstar => o.rating = None,
            Edit::Place | Edit::ClearPlace => o.place = None,
            Edit::Date => (o.date_taken, o.date_source) = (None, None),
        }
        o
    }

    /// Whether the edit's result reads back from `o` (for sidecars; `verify` does this for files).
    fn reads_back(self, before: &OwnedFields, o: &OwnedFields) -> bool {
        match self {
            Edit::Caption => o.caption == Some(caption()),
            Edit::Tags => o.tags == tags(),
            Edit::Star => o.rating == Some(5),
            // Unstarring clears only a rating Dalica could have set (`xmp-interop.md` §2).
            Edit::Unstar => o.rating == if before.rating == Some(5) { Some(0) } else { before.rating },
            Edit::Place => {
                o.place.is_some_and(|p| (p.latitude() - place().latitude()).abs() < 1e-6 && (p.longitude() - place().longitude()).abs() < 1e-6)
            }
            Edit::Date => o.date_taken == Some(date()),
            Edit::ClearCaption => o.caption.is_none(),
            Edit::ClearTags => o.tags.is_empty(),
            Edit::ClearPlace => o.place.is_none(),
        }
    }

    fn is_place_or_date(self) -> bool {
        matches!(self, Edit::Place | Edit::ClearPlace | Edit::Date)
    }
}

fn options() -> WriteOptions {
    // The time of the write is fixed, so every run writes the same bytes.
    let metadata_date = DateTaken::new(dt(2026, 10, 12, 9, 0, 0), Some(0), None);
    WriteOptions { stars_as_ratings: true, metadata_date, ..WriteOptions::default() }
}

/// Large enough that no row of this set is cut or capped, so the comparison sees every value.
fn listing_limits() -> Limits {
    Limits { max_rows: 1_000_000, max_row_value: 1 << 20, ..Limits::default() }
}

fn tag(kind: IfdKind, t: u16) -> String {
    dalica_meta::exif::tag_name(kind, t).map_or_else(|| format!("Tag 0x{t:04x}"), str::to_owned)
}

/// Listing rows that are owned properties (or the pointers and block sizes that move with them):
/// a write may change them. Everything else is foreign and must survive.
struct Owned {
    exif: BTreeSet<(String, String)>,
    /// Rows a write may add (a new IFD's version, IPTC 1:90 and 2:00), but never change or remove.
    added: BTreeSet<(String, String)>,
}

impl Owned {
    fn new() -> Owned {
        let g = |k: IfdKind| k.group().to_string();
        let mut exif = BTreeSet::new();
        for t in [0x010e, 0x8769, 0x8825, 0x02bc, 0x83bb, 0x8649] {
            exif.insert((g(IfdKind::Image), tag(IfdKind::Image, t)));
        }
        for t in [0x9003, 0x9004, 0x9011, 0x9012, 0x9291, 0x9292] {
            exif.insert((g(IfdKind::Exif), tag(IfdKind::Exif, t)));
        }
        for t in [0x0001, 0x0002, 0x0003, 0x0004] {
            exif.insert((g(IfdKind::Gps), tag(IfdKind::Gps, t)));
        }
        for name in ["Caption", "Keywords"] {
            exif.insert(("IPTC".into(), name.into()));
        }
        let mut added = BTreeSet::new();
        added.insert((g(IfdKind::Exif), tag(IfdKind::Exif, 0x9000)));
        added.insert((g(IfdKind::Gps), tag(IfdKind::Gps, 0x0000)));
        added.insert(("IPTC".into(), "Coded character set".into()));
        added.insert(("IPTC".into(), "Record version".into()));
        Owned { exif, added }
    }

    fn owned(&self, group: &str, label: &str) -> bool {
        if group.starts_with("XMP") {
            // `dc:description` is owned in its x-default item only: other languages are foreign.
            const XMP: [&str; 9] = [
                "dc:subject",
                "xmp:Rating",
                "exif:GPSLatitude",
                "exif:GPSLongitude",
                "exif:DateTimeOriginal",
                "photoshop:DateCreated",
                "xmp:CreateDate",
                "xmp:MetadataDate",
                "dc:description",
            ];
            // Language alternatives list as `name [lang]`, an item without a language as `name`.
            return XMP.iter().any(|p| label == *p || label.strip_prefix(p) == Some(" [x-default]"));
        }
        self.exif.contains(&(group.to_string(), label.to_string()))
    }
}

type Rows = BTreeMap<(String, String), Vec<String>>;

fn rows_of(rows: impl Iterator<Item = (String, String, String)>, owned: &Owned) -> Rows {
    let mut out = Rows::new();
    for (group, label, value) in rows.filter(|(g, l, _)| !owned.owned(g, l)) {
        out.entry((group, label)).or_default().push(value);
    }
    out
}

fn file_rows(bytes: &[u8], owned: &Owned) -> Rows {
    let listing = dalica_meta::listing(bytes, &listing_limits()).unwrap();
    assert!(!listing.truncated, "the listing was cut");
    rows_of(listing.rows.into_iter().map(|r| (r.group, r.label, r.value)), owned)
}

fn sidecar_rows(bytes: &[u8], owned: &Owned) -> Rows {
    let rows = dalica_meta::xmp::rows(bytes, None, &listing_limits()).unwrap();
    rows_of(rows.into_iter().map(|r| (r.group, r.label, r.value)), owned)
}

/// The foreign rows that a write lost, changed or added (other than the rows it may add).
fn foreign_diff(before: &Rows, after: &Rows, owned: &Owned) -> Vec<String> {
    let mut out = Vec::new();
    for (k, v) in before {
        if after.get(k) != Some(v) {
            out.push(format!("{}/{} changed or lost", k.0, k.1));
        }
    }
    for k in after.keys().filter(|k| !before.contains_key(*k) && !owned.added.contains(*k)) {
        out.push(format!("{}/{} added", k.0, k.1));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Measuring the set

#[derive(Default)]
struct Tally {
    counts: BTreeMap<String, u64>,
    failures: Vec<String>,
    lines: Vec<String>,
}

impl Tally {
    fn count(&mut self, key: &str) {
        *self.counts.entry(key.to_string()).or_default() += 1;
    }

    fn fail(&mut self, rel: &str, what: String) {
        self.failures.push(format!("{rel}: {what}"));
    }
}

/// Every byte outside `before`'s rewritable spans kept: the payload digest for JPEG, PNG and WebP;
/// for TIFF (append-only) each such byte still at its offset.
fn payload_kept(before: &[u8], after: &[u8], limits: &Limits) -> bool {
    let (Ok(a), Ok(b)) = (dalica_meta::scan(before, limits), dalica_meta::scan(after, limits)) else { return false };
    if a.container == Container::Tiff {
        return after.len() >= before.len()
            && dalica_meta::container::outside(before.len(), &a.spans).into_iter().all(|r| before.get(r.clone()) == after.get(r));
    }
    dalica_meta::container::payload_digest(before, &a.spans) == dalica_meta::container::payload_digest(after, &b.spans)
}

/// A refusal `xmp-interop.md` §4 promises: a RAW is never written; a TIFF's place or date goes only
/// to XMP mirrors, so it fails where the EXIF holds the value (EXIF wins on reading) or no mirror
/// exists to take it; a JPEG's EXIF must fit one APP1, so a place or date fails, naming its
/// field, where the block has no room left for it.
fn expected_refusal(rel: &str, bytes: &[u8], edit: Edit, error: &Error, limits: &Limits) -> Option<&'static str> {
    let is_raw = rel.to_ascii_lowercase().ends_with(".dng");
    if is_raw && matches!(error, Error::Unsupported(Container::Tiff)) {
        return Some("refused_raw");
    }
    let m = dalica_meta::read(bytes, limits).ok()?;
    // The writer appends a changed value and never repacks, so it fails only for want of room.
    let full = |field: &str| matches!(error, Error::TooMany(what) if *what == format!("EXIF bytes for the {field}"));
    if m.container == Some(Container::Jpeg) && ((edit == Edit::Place && full("place")) || (edit == Edit::Date && full("date taken"))) {
        return Some("refused_exif_full");
    }
    if m.container != Some(Container::Tiff) || !edit.is_place_or_date() || !matches!(error, Error::Refused { field: "place" | "date taken", .. }) {
        return None;
    }
    let x = &m.sources.xmp;
    let exif_holds = match edit {
        Edit::Date => m.sources.exif.date_taken.is_some(),
        _ => m.sources.exif.place.is_some(),
    };
    let no_mirror = match edit {
        Edit::Date => x.date_time_original.is_none() && x.date_created.is_none() && x.create_date.is_none(),
        _ => false,
    };
    (exif_holds || no_mirror).then_some("refused_tiff_place_date")
}

fn measure_file(rel: &str, bytes: &[u8], owned: &Owned, t: &mut Tally) {
    let limits = Limits::default();
    let opts = options();
    t.count("files");
    let before = match dalica_meta::read(bytes, &limits) {
        Ok(m) if m.problems.is_empty() => m,
        Ok(m) => return t.fail(rel, format!("reads with problems: {:?}", m.problems)),
        Err(e) => return t.fail(rel, format!("does not read: {e}")),
    };
    t.count("read");
    t.count("images_read");
    let foreign_before = file_rows(bytes, owned);
    let mut results = Vec::new();
    for edit in EDITS {
        t.count("writes");
        let changes = edit.changes();
        let after = match dalica_meta::write_owned(bytes, &changes, &opts) {
            Ok(after) => after,
            Err(e) => {
                match expected_refusal(rel, bytes, edit, &e, &limits) {
                    Some(key) => t.count(key),
                    None => t.fail(rel, format!("{edit:?}: {e}")),
                }
                results.push(format!("{edit:?}: refused"));
                continue;
            }
        };
        if after == bytes {
            t.count("unchanged");
            results.push(format!("{edit:?}: unchanged"));
            continue;
        }
        let mut problems = Vec::new();
        if let Err(e) = dalica_meta::verify(bytes, &after, &changes, &limits) {
            problems.push(format!("verify: {e}"));
        }
        if !payload_kept(bytes, &after, &limits) {
            problems.push("bytes outside the metadata changed".into());
        }
        if before.container == Some(Container::Jpeg) {
            problems.extend(jpeg_kept(bytes, &after));
        }
        problems.extend(foreign_diff(&foreign_before, &file_rows(&after, owned), owned));
        match dalica_meta::read(&after, &limits) {
            Ok(m) if !m.problems.is_empty() => problems.push(format!("reads back with problems: {:?}", m.problems)),
            Ok(m) => {
                if edit.others(&m.owned) != edit.others(&before.owned) {
                    problems.push(format!("other owned fields changed: {:?} → {:?}", edit.others(&before.owned), edit.others(&m.owned)));
                }
                if !edit.reads_back(&before.owned, &m.owned) {
                    problems.push("the edit does not read back".into());
                }
            }
            Err(e) => problems.push(format!("does not read back: {e}")),
        }
        match dalica_meta::write_owned(&after, &changes, &opts) {
            Ok(again) if again == after => {}
            Ok(_) => problems.push("writing the same values again changed the file".into()),
            Err(e) => problems.push(format!("writing again failed: {e}")),
        }
        if problems.is_empty() {
            t.count("written");
            results.push(format!("{edit:?}: ok"));
        } else {
            results.push(format!("{edit:?}: FAILED"));
            t.fail(rel, format!("{edit:?}: {}", problems.join("; ")));
        }
    }
    let mut extra = String::new();
    if before.container == Some(Container::Jpeg) {
        extra = jpeg_structure(rel, bytes, t);
    }
    t.lines.push(format!("  {rel:<46} {:<7} · {}{extra}", before.container.map_or("?", Container::name), summary(&results)));
}

// ---------------------------------------------------------------------------------------------
// A JPEG's image data, its MPF index and what follows its EOI

/// The MPF index of a JPEG (CIPA DC-007 §5.2): where its MP header is, where its APP2 segment
/// ends, and each MP entry's image size and offset (from the MP header; 0 for the primary).
struct Mpf {
    header: usize,
    segment_end: usize,
    entries: Vec<(usize, usize)>,
}

/// What a JPEG write must keep: the image data from the first SOS to the primary's EOI, and the
/// MPF index if there is one.
struct JpegLayout {
    image: std::ops::Range<usize>,
    mpf: Option<Mpf>,
}

fn be_or_le(b: &[u8], at: usize, n: usize, big: bool) -> Option<usize> {
    let bytes = b.get(at..at.checked_add(n)?)?;
    let fold = |acc: usize, &x: &u8| (acc << 8) | usize::from(x);
    Some(if big { bytes.iter().fold(0, fold) } else { bytes.iter().rev().fold(0, fold) })
}

/// The MP Index IFD at the MP header `header` (bounded: at most 64 tags and 64 entries).
fn mpf_at(b: &[u8], header: usize, segment_end: usize) -> Option<Mpf> {
    let big = match b.get(header..header + 2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let ifd = header.checked_add(be_or_le(b, header + 4, 4, big)?)?;
    let count = be_or_le(b, ifd, 2, big)?.min(64);
    for k in 0..count {
        let e = ifd + 2 + 12 * k;
        if be_or_le(b, e, 2, big)? != 0xb002 {
            continue;
        }
        let n = (be_or_le(b, e + 4, 4, big)? / 16).min(64);
        let data = header.checked_add(be_or_le(b, e + 8, 4, big)?)?;
        let entries =
            (0..n).map(|i| Some((be_or_le(b, data + 16 * i + 4, 4, big)?, be_or_le(b, data + 16 * i + 8, 4, big)?))).collect::<Option<Vec<_>>>()?;
        return Some(Mpf { header, segment_end, entries });
    }
    None
}

/// Walks a JPEG (ITU-T T.81 B.1) to the end of its primary image: the segments before the first
/// scan, then scans and the segments between them, entropy-coded data skipped by its markers
/// (FF00 stuffing and RSTn are data), to EOI. `None` for a file with no EOI.
fn jpeg_layout(b: &[u8]) -> Option<JpegLayout> {
    let marker_at = |at: usize| -> Option<(usize, u8)> {
        let mut at = at;
        (b.get(at)? == &0xff).then_some(())?;
        while b.get(at + 1)? == &0xff {
            at += 1;
        }
        Some((at, *b.get(at + 1)?))
    };
    let seg_len = |at: usize| be_or_le(b, at + 2, 2, true).filter(|&n| n >= 2);
    let (mut at, mut mpf) = (2, None);
    let sos = loop {
        let (m_at, marker) = marker_at(at)?;
        match marker {
            0xda => break m_at,
            0xd9 => return None,
            0x01 | 0xd0..=0xd8 => at = m_at + 2,
            _ => {
                let end = m_at + 2 + seg_len(m_at)?;
                if marker == 0xe2 && mpf.is_none() && b.get(m_at + 4..m_at + 8) == Some(b"MPF\0") {
                    mpf = mpf_at(b, m_at + 8, end);
                }
                at = end;
            }
        }
    };
    let mut at = sos;
    loop {
        let (m_at, marker) = marker_at(at)?;
        match marker {
            0xd9 => return Some(JpegLayout { image: sos..m_at + 2, mpf }),
            0x01 | 0xd0..=0xd8 => at = m_at + 2,
            _ => {
                at = m_at + 2 + seg_len(m_at)?;
                if marker == 0xda {
                    // Entropy-coded data: up to the next marker that is not stuffing or a restart.
                    loop {
                        at += b.get(at..)?.iter().position(|&x| x == 0xff)?;
                        match *b.get(at + 1)? {
                            0x00 | 0xd0..=0xd7 => at += 2,
                            0xff => at += 1,
                            _ => break,
                        }
                    }
                }
            }
        }
    }
}

/// The images the MPF index lists other than the primary, as `(start in the file, size)`, where
/// the original's start holds an SOI (an index that was already wrong is not ours to judge).
fn mpf_images(b: &[u8], mpf: &Mpf) -> Vec<(usize, usize)> {
    mpf.entries
        .iter()
        .filter(|(_, offset)| *offset != 0)
        .filter_map(|&(size, offset)| Some((mpf.header.checked_add(offset)?, size)))
        .filter(|&(start, _)| b.get(start..start + 2) == Some(&[0xff, 0xd8][..]))
        .collect()
}

/// What a JPEG write broke: the image data, the data after EOI, or an image of the MPF index.
fn jpeg_kept(before: &[u8], after: &[u8]) -> Vec<String> {
    let Some(old) = jpeg_layout(before) else { return Vec::new() };
    let Some(new) = jpeg_layout(after) else { return vec!["the written JPEG has no EOI where the original had one".into()] };
    let mut problems = Vec::new();
    if before.get(old.image.clone()) != after.get(new.image.clone()) {
        problems.push("the image data changed".into());
    }
    if before.get(old.image.end..) != after.get(new.image.end..) {
        problems.push("the data after EOI changed".into());
    }
    let Some(old_mpf) = &old.mpf else { return problems };
    let listed = mpf_images(before, old_mpf);
    if listed.is_empty() {
        return problems;
    }
    let Some(new_mpf) = &new.mpf else {
        problems.push("the MPF index is gone".into());
        return problems;
    };
    if new_mpf.entries.len() != old_mpf.entries.len() {
        problems.push(format!("the MPF index lists {} images, not {}", new_mpf.entries.len(), old_mpf.entries.len()));
        return problems;
    }
    for (k, (&(_, old_offset), &(size, offset))) in old_mpf.entries.iter().zip(&new_mpf.entries).enumerate().filter(|(_, (o, _))| o.1 != 0) {
        let Some(&(old_start, old_size)) = listed.iter().find(|(s, _)| *s == old_mpf.header + old_offset) else { continue };
        let start = new_mpf.header + offset;
        if after.get(start..start + 2) != Some(&[0xff, 0xd8][..]) {
            problems.push(format!("MPF image {k} does not start with FFD8 at its offset ({offset} from the MP header)"));
        } else if size != old_size || after.get(start..start + size) != before.get(old_start..old_start + old_size) {
            problems.push(format!("MPF image {k} is not the image it was"));
        }
    }
    problems
}

/// Counts what `bytes` holds beyond its primary image (MPF images, data after EOI) and proves the
/// MPF check on it: a segment slipped in after the MPF index, the offsets left as they were, must
/// fail [`jpeg_kept`]. The note for the file's line.
fn jpeg_structure(rel: &str, bytes: &[u8], t: &mut Tally) -> String {
    let Some(layout) = jpeg_layout(bytes) else { return " · no EOI".into() };
    let mut note = String::new();
    let trailer = bytes.len() - layout.image.end;
    if trailer > 0 {
        t.count("trailers");
        note += &format!(" · {trailer} bytes after EOI");
    }
    let Some(mpf) = &layout.mpf else { return note };
    let listed = mpf_images(bytes, mpf);
    if listed.is_empty() {
        return note + " · MPF without other images";
    }
    t.count("mpf_files");
    note += &format!(" · MPF: {} more images", listed.len());
    let shifted = [&bytes[..mpf.segment_end], b"\xff\xfe\x00\x07shift", &bytes[mpf.segment_end..]].concat();
    let caught = jpeg_kept(bytes, &shifted);
    if caught.iter().any(|p| p.starts_with("MPF image")) {
        t.count("mpf_shift_caught");
    } else {
        t.fail(rel, format!("moving the MPF images without fixing their offsets was not caught: {caught:?}"));
    }
    note
}

fn measure_sidecar(rel: &str, bytes: &[u8], owned: &Owned, t: &mut Tally) {
    let limits = Limits::default();
    let opts = options();
    t.count("files");
    let r = xmp_sidecar::read(None, Some(bytes), &limits);
    if r.xmp_from != XmpFrom::Sidecar || r.damaged.is_some() || !r.metadata.problems.is_empty() {
        return t.fail(rel, format!("sidecar does not read: from {:?}, damaged {:?}, problems {:?}", r.xmp_from, r.damaged, r.metadata.problems));
    }
    t.count("read");
    t.count("sidecars_read");
    let foreign_before = sidecar_rows(bytes, owned);
    let mut results = Vec::new();
    for edit in EDITS {
        t.count("sidecar_writes");
        let changes = edit.changes();
        let after = match xmp_sidecar::merge(None, Some(bytes), &changes, &opts) {
            Ok(SidecarWrite::Unchanged) => {
                t.count("sidecar_unchanged");
                results.push(format!("{edit:?}: unchanged"));
                continue;
            }
            Ok(SidecarWrite::Write(after)) => after,
            Ok(SidecarWrite::ReplaceDamaged { damaged, .. }) => {
                t.fail(rel, format!("{edit:?}: a well-formed sidecar was treated as damaged ({damaged})"));
                continue;
            }
            Err(e) => {
                t.fail(rel, format!("{edit:?}: {e}"));
                continue;
            }
        };
        let mut problems = Vec::new();
        if let Err(e) = xmp_sidecar::well_formed(&after, &limits) {
            problems.push(format!("not well-formed: {e}"));
        }
        problems.extend(foreign_diff(&foreign_before, &sidecar_rows(&after, owned), owned));
        let m = xmp_sidecar::read(None, Some(&after), &limits).metadata;
        if edit.others(&m.owned) != edit.others(&r.metadata.owned) {
            problems.push("other owned fields changed".into());
        }
        if !edit.reads_back(&r.metadata.owned, &m.owned) {
            problems.push("the edit does not read back".into());
        }
        if !matches!(xmp_sidecar::merge(None, Some(&after), &changes, &opts), Ok(SidecarWrite::Unchanged)) {
            problems.push("writing the same values again changed the sidecar".into());
        }
        if problems.is_empty() {
            t.count("sidecar_written");
            results.push(format!("{edit:?}: ok"));
        } else {
            results.push(format!("{edit:?}: FAILED"));
            t.fail(rel, format!("{edit:?}: {}", problems.join("; ")));
        }
    }
    t.lines.push(format!("  {rel:<46} sidecar · {}", summary(&results)));
}

/// A RAW with its sidecar (`xmp-interop.md` §7): the sidecar replaces the embedded packet, the RAW's
/// own EXIF date still comes first, and a new sidecar starts from the embedded packet.
fn measure_pair(rel: &str, raw: &[u8], sidecar: &[u8], owned: &Owned, t: &mut Tally) {
    let limits = Limits::default();
    let alone = xmp_sidecar::read(Some(raw), None, &limits);
    let paired = xmp_sidecar::read(Some(raw), Some(sidecar), &limits);
    let side = xmp_sidecar::read(None, Some(sidecar), &limits);
    let mut problems = Vec::new();
    if alone.xmp_from != XmpFrom::Embedded || paired.xmp_from != XmpFrom::Sidecar {
        problems.push(format!("XMP from {:?} alone and {:?} with its sidecar", alone.xmp_from, paired.xmp_from));
    }
    if paired.metadata.owned.caption != side.metadata.owned.caption || paired.metadata.owned.rating != side.metadata.owned.rating {
        problems.push("the sidecar's caption and rating do not replace the embedded ones".into());
    }
    if paired.metadata.owned.date_taken != alone.metadata.owned.date_taken || alone.metadata.owned.date_taken.is_none() {
        problems.push("the RAW's EXIF date does not come first".into());
    }
    // A new sidecar for the RAW starts as a copy of its embedded packet: its foreign rows survive.
    let embedded = dalica_meta::scan(raw, &limits).ok().and_then(|s| s.xmp.map(|x| x.into_owned()));
    match (embedded, xmp_sidecar::merge(Some(raw), None, &Edit::Caption.changes(), &options())) {
        (Some(packet), Ok(SidecarWrite::Write(new))) => {
            problems.extend(foreign_diff(&sidecar_rows(&packet, owned), &sidecar_rows(&new, owned), owned));
            let m = xmp_sidecar::read(Some(raw), Some(&new), &limits).metadata;
            if m.owned.caption != Some(caption()) {
                problems.push("the new sidecar's caption does not read back".into());
            }
        }
        (packet, other) => problems.push(format!("a new sidecar: embedded packet {}, merge {other:?}", packet.is_some())),
    }
    if problems.is_empty() {
        t.count("raw_sidecar_pairs");
    } else {
        t.fail(rel, format!("with its sidecar: {}", problems.join("; ")));
    }
}

fn summary(results: &[String]) -> String {
    let n = |s: &str| results.iter().filter(|r| r.ends_with(s)).count();
    format!("{} written, {} unchanged, {} refused, {} failed", n(": ok"), n(": unchanged"), n(": refused"), n(": FAILED"))
}

/// Verifies the set against its manifest and measures it; `Err` is why it cannot be measured.
/// A set's counts and its failures.
type Measured = (BTreeMap<String, u64>, Vec<String>);

fn measure(set: &str) -> Option<Result<Measured, String>> {
    let entries = manifest(set)?;
    let dir = root().join("corpus").join(set);
    let (mut missing, mut stale) = (Vec::new(), Vec::new());
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (hash, rel) in &entries {
        match std::fs::read(dir.join(rel)) {
            Err(_) => missing.push(rel.clone()),
            Ok(bytes) if sha256_hex(&bytes) != *hash => stale.push(rel.clone()),
            Ok(bytes) => {
                files.insert(rel.clone(), bytes);
            }
        }
    }
    if !missing.is_empty() || !stale.is_empty() {
        return Some(Err(format!(
            "corpus/{set}: {} of {} files missing ({}), {} stale ({}) against xtask/corpus/{set}.sha256: run `cargo xtask corpus --all`",
            missing.len(),
            entries.len(),
            some(&missing),
            stale.len(),
            some(&stale)
        )));
    }
    let owned = Owned::new();
    let mut t = Tally::default();
    for (rel, bytes) in files.iter().filter(|(rel, _)| measured(set, rel)) {
        let lower = rel.to_ascii_lowercase();
        if lower.ends_with(".xmp") {
            measure_sidecar(rel, bytes, &owned, &mut t);
        } else {
            measure_file(rel, bytes, &owned, &mut t);
        }
        // A RAW whose `<stem>.xmp` is in the set.
        if lower.ends_with(".dng")
            && let Some((stem, _)) = rel.rsplit_once('.')
            && let Some(sidecar) = files.get(&format!("{stem}.xmp"))
        {
            measure_pair(rel, bytes, sidecar, &owned, &mut t);
        }
    }
    println!("\ncorpus/{set}: {} files", files.keys().filter(|rel| measured(set, rel)).count());
    for l in &t.lines {
        println!("{l}");
    }
    Some(Ok((t.counts, t.failures)))
}

/// The first few names of a list, for messages.
fn some(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(8).map(String::as_str).collect();
    if names.len() > shown.len() { format!("{}, …", shown.join(", ")) } else { shown.join(", ") }
}

/// Measures `set` and holds it to its floors; `Ok(None)` when it is not published yet.
fn check(set: &str) -> Result<Option<BTreeMap<String, u64>>, String> {
    let floors = floors();
    let Some(measured) = measure(set) else {
        if floors.contains_key(set) {
            return Err(format!("{set} has floors in corpus_floors.json but no manifest xtask/corpus/{set}.sha256"));
        }
        println!("corpus/{set}: not published yet (no xtask/corpus/{set}.sha256); SKIPPED, not passed, and it has no floors");
        return Ok(None);
    };
    let (measured, failures) = measured?;
    println!("corpus/{set} measured: {measured:?}");
    let mut problems = failures;
    let floor = floors.get(set).ok_or_else(|| format!("{set} is measured but has no floors in corpus_floors.json: add {measured:?}"))?;
    for (key, min) in floor {
        if UNFLOORED.contains(&key.as_str()) {
            problems.push(format!("{set}.{key} is not a quality measure and takes no floor"));
            continue;
        }
        match measured.get(key) {
            None => problems.push(format!("{set}.{key}: 0 is below its floor {min}")),
            Some(got) if got < min => problems.push(format!("{set}.{key}: {got} is below its floor {min}")),
            Some(got) if got > min => println!("corpus/{set}.{key}: {got} is above its floor {min}; raise the floor in this PR"),
            Some(_) => {}
        }
    }
    for key in measured.keys().filter(|k| !floor.contains_key(*k) && !UNFLOORED.contains(&k.as_str())) {
        problems.push(format!("{set}.{key} has no floor (add it at the measured count)"));
    }
    if problems.is_empty() { Ok(Some(measured)) } else { Err(problems.join("\n")) }
}

#[test]
fn floors_name_known_sets_only() {
    for set in floors().keys() {
        assert!(SETS.contains(&set.as_str()), "corpus_floors.json names an unknown set {set:?}");
    }
}

#[test]
fn metadata() {
    if let Err(e) = check("metadata") {
        panic!("{e}");
    }
}

#[test]
fn photos() {
    if let Err(e) = check("photos") {
        panic!("{e}");
    }
}
