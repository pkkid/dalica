//! Optional build input: with `CRAFT_FONTS_DIR=<storytold/craft-fonts checkout>`, embeds the faces
//! of its `fonts/manifest.txt` that [`EMBEDDED_FACES`] names as `CRAFT_FONTS` (`src/fonts.rs`), the
//! CJK ones subset to the four national character sets ([`common_cjk`], [`subset`]); unset,
//! `CRAFT_FONTS` is empty and the UI draws with Inter alone (`docs/assets.md` §4). craft-fonts is
//! never a Cargo dependency, and this script only reads a local directory (no network) and writes
//! into `OUT_DIR`. `CRAFT_FONTS_REQUIRED=1` (release builds) turns a missing directory, manifest,
//! face or required UI face, or a face that does not subset, into a build error naming it.
//!
//! Adapted from LightCraft `crates/engine/build.rs` (revision d999e382); MIT OR Apache-2.0. Changed
//! for Dalica: no wasm32 subset (Dalica has no web build), no warning when `CRAFT_FONTS_DIR` is
//! unset (a developer build with Inter alone is the normal case), the licence fields carried for
//! the packages, the logic split into [`generate`] so `tests/fonts.rs` can test it, and (M16.27)
//! only one face per script embedded, the CJK faces subset by our own [`subset`].

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The UI faces a release must carry: family, style and the script it is required for. Thai is the
/// second language of 1.0; Japanese and Chinese file names are common in any language.
pub const REQUIRED_FACES: &[(&str, &str, &str)] =
    &[("BIZ UDPGothic", "Regular", "Jpan"), ("Noto Sans CJK SC", "Regular", "Hans"), ("Noto Sans Thai", "Regular", "Thai")];

/// The manifest's faces a build embeds (M16.27, the maintainer's answer on #1148): one UI face per
/// script. The others (BIZ UDPGothic Bold, the two Mincho faces) stay out of the packages, so CJK
/// text in bold is drawn with the Regular face.
pub const EMBEDDED_FACES: &[(&str, &str)] =
    &[("BIZ UDPGothic", "Regular"), ("Noto Sans CJK SC", "Regular"), ("Noto Sans Thai", "Regular"), ("Noto Sans Arabic", "Regular")];

/// At most this many manifest lines are read (the pinned manifest has 7).
const MAX_FACES: usize = 64;

/// A face file larger than this is refused (the largest pinned face, Noto Sans CJK SC, is 16.4 MB).
const MAX_FACE_BYTES: u64 = 64 << 20;

#[allow(dead_code)]
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let required = std::env::var_os("CRAFT_FONTS_REQUIRED").is_some_and(|v| !v.is_empty() && v != "0");
    let dir = std::env::var_os("CRAFT_FONTS_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).map(resolve);
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default());
    let out = generate(dir.as_deref(), required, &out_dir);
    for path in &out.watched {
        println!("cargo::rerun-if-changed={}", path.display());
    }
    for warning in &out.warnings {
        println!("cargo::warning={warning}");
    }
    for error in &out.errors {
        println!("cargo::error={error}");
    }
    let file = out_dir.join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&file, &out.source) {
        println!("cargo::error=writing {}: {e}", file.display());
    }
}

/// What [`generate`] made: the Rust source of `CRAFT_FONTS`, the files to watch, and the warnings
/// and errors to print.
#[derive(Debug, Default)]
pub struct Generated {
    pub source: String,
    pub embedded: usize,
    pub watched: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// One manifest line: `family | style | file | scripts | licence | licence file | sha256 | source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub family: String,
    pub style: String,
    pub file: String,
    pub scripts: Vec<String>,
    pub licence: String,
    pub licence_file: String,
}

impl Entry {
    /// Whether [`EMBEDDED_FACES`] names the face.
    pub fn is_embedded(&self) -> bool {
        EMBEDDED_FACES.iter().any(|(family, style)| self.family == *family && self.style == *style)
    }

    /// Whether the face is for a CJK script: it is subset to [`common_cjk`].
    pub fn is_cjk(&self) -> bool {
        self.scripts.iter().any(|s| matches!(s.as_str(), "Jpan" | "Hans" | "Hant" | "Kore" | "Hang" | "Hani"))
    }
}

/// The `CRAFT_FONTS` source for the checkout at `dir` (none: empty), with the subset faces written
/// into `out_dir`. A broken input is a warning, or with `required` an error naming the directory
/// and what is missing; either way the source is valid Rust.
pub fn generate(dir: Option<&Path>, required: bool, out_dir: &Path) -> Generated {
    let mut out = Generated::default();
    let mut entries = String::new();
    match dir {
        None if required => {
            out.errors.push("CRAFT_FONTS_REQUIRED is set but CRAFT_FONTS_DIR is missing: set it to a storytold/craft-fonts checkout".into())
        }
        None => {}
        Some(dir) => match embed(dir, required, out_dir, &mut out.watched) {
            Ok((text, count)) => {
                entries = text;
                out.embedded = count;
            }
            Err(e) if required => out.errors.push(format!("CRAFT_FONTS_DIR={}: {e}", dir.display())),
            Err(e) => out.warnings.push(format!(
                "building without craft-fonts: CRAFT_FONTS_DIR={}: {e}; Chinese, Japanese and Thai text will show boxes",
                dir.display()
            )),
        },
    }
    out.source = format!("pub static CRAFT_FONTS: &[CraftFont] = &[\n{entries}];\n");
    out
}

/// Parses a manifest's text: comments and blank lines skipped, a short line an error.
pub fn parse_manifest(text: &str) -> Result<Vec<Entry>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    if lines.len() > MAX_FACES {
        return Err(format!("{} faces in fonts/manifest.txt, more than {MAX_FACES}", lines.len()));
    }
    lines
        .into_iter()
        .map(|line| {
            let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
            let [family, style, file, scripts, licence, licence_file, ..] = f.as_slice() else {
                return Err(format!("malformed manifest line: {line}"));
            };
            let scripts: Vec<String> = scripts.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
            if family.is_empty() || style.is_empty() || file.is_empty() || scripts.is_empty() {
                return Err(format!("malformed manifest line: {line}"));
            }
            Ok(Entry {
                family: (*family).into(),
                style: (*style).into(),
                file: (*file).into(),
                scripts,
                licence: (*licence).into(),
                licence_file: (*licence_file).into(),
            })
        })
        .collect()
}

/// The first required UI face `entries` lack, as a message naming it.
pub fn missing_required(entries: &[Entry]) -> Option<String> {
    REQUIRED_FACES.iter().find_map(|(family, style, script)| {
        let present = entries.iter().any(|e| e.family == *family && e.style == *style && e.scripts.iter().any(|s| s == script));
        (!present).then(|| format!("the required UI face {family} {style} ({script}) is missing from fonts/manifest.txt"))
    })
}

/// One `CraftFont { .. }` initialiser per embedded manifest line, and how many.
fn embed(dir: &Path, required: bool, out_dir: &Path, watched: &mut Vec<PathBuf>) -> Result<(String, usize), String> {
    if !dir.is_dir() {
        return Err("not a directory".into());
    }
    let manifest = dir.join("fonts").join("manifest.txt");
    watched.push(manifest.clone());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let entries = parse_manifest(&text)?;
    if required && let Some(missing) = missing_required(&entries) {
        return Err(missing);
    }
    let mut out = String::new();
    let mut common: Option<BTreeSet<u32>> = None;
    let mut count = 0;
    for e in entries.iter().filter(|e| e.is_embedded()) {
        let path = dir.join(&e.file).canonicalize().map_err(|err| format!("{}: {err}", e.file))?;
        watched.push(path.clone());
        let path = if e.is_cjk() {
            let common = common.get_or_insert_with(common_cjk);
            subset_file(&path, common, out_dir).map_err(|err| format!("{}: {err}", e.file))?
        } else {
            path
        };
        let scripts: Vec<String> = e.scripts.iter().map(|s| format!("{s:?}")).collect();
        let _ = writeln!(
            out,
            "    CraftFont {{ family: {:?}, style: {:?}, scripts: &[{}], licence: {:?}, licence_file: {:?}, bytes: include_bytes!({:?}) }},",
            e.family,
            e.style,
            scripts.join(", "),
            e.licence,
            e.licence_file,
            path.display().to_string(),
        );
        count += 1;
    }
    Ok((out, count))
}

/// Subsets the face at `path` to the characters [`kept`] keeps, into `out_dir`, and returns the
/// written file's path.
fn subset_file(path: &Path, common: &BTreeSet<u32>, out_dir: &Path) -> Result<PathBuf, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_FACE_BYTES {
        return Err(format!("{size} bytes, more than {MAX_FACE_BYTES}"));
    }
    let font = std::fs::read(path).map_err(|e| e.to_string())?;
    let small = subset(&font, &|c| kept(common, c))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let out = out_dir.join(format!("subset-{name}"));
    std::fs::write(&out, &small).map_err(|e| format!("writing {}: {e}", out.display()))?;
    Ok(out)
}

/// Resolves a relative `CRAFT_FONTS_DIR` against both roots a caller might mean: a build script runs
/// in the package root (`crates/ui-kit`), while the `../craft-fonts` the docs use is relative to the
/// workspace root.
fn resolve(dir: PathBuf) -> PathBuf {
    if dir.is_absolute() {
        return dir;
    }
    let package = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let workspace = package.parent().and_then(Path::parent).unwrap_or(package.as_path());
    let (first, second) = (workspace.join(&dir), package.join(&dir));
    if first.is_dir() || !second.is_dir() { first } else { second }
}

// --- The CJK character sets ---------------------------------------------------------------------

/// Whether `c` is a Han ideograph (the URO, extension A, the compatibility block and the
/// supplementary planes' extensions) or a precomposed Hangul syllable: the characters a CJK face is
/// subset by. Everything else (kana, bopomofo, Hangul jamo, punctuation, Latin) is always kept.
pub fn is_han_or_hangul(c: u32) -> bool {
    matches!(c, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3FFFF | 0xAC00..=0xD7A3)
}

/// Whether a CJK face keeps `c`: any character but a Han or Hangul one, and those in `common`.
pub fn kept(common: &BTreeSet<u32>, c: u32) -> bool {
    !is_han_or_hangul(c) || common.contains(&c)
}

/// The blocks [`COMMON_CJK`] covers, in its order: the URO, Hangul syllables, the compatibility
/// ideographs (none of the four sets has a character in extension A or beyond).
pub const COMMON_CJK_BLOCKS: &[(u32, u32)] = &[(0x4E00, 0x9FFF), (0xAC00, 0xD7A3), (0xF900, 0xFAFF)];

/// The Han characters and Hangul syllables of the four national sets everyday Chinese, Japanese
/// and Korean text is written in: GB 2312 (rows 16 to 87), Big5 level 1 (A440 to C67E), JIS X 0208
/// (rows 1 to 8 and 16 to 84) and KS X 1001, as the WHATWG encodings map them (the GBK, Big5,
/// Shift_JIS and EUC-KR indexes): 10,831 Han characters and 2,350 Hangul syllables. One bit per
/// code point of [`COMMON_CJK_BLOCKS`], most significant bit first, in base64. Generated with
/// `encoding_rs` 0.8.44; `tests/fonts.rs` decodes the four sets again with it and compares, so a
/// wrong bit fails a test.
pub const COMMON_CJK: &str = "\
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\
CAAAgAAAAAyIDVDAgAgIAACAiIDAAAAAQMiAxAgAAMAAAAAIAAAAgAAACAAJQAAAAAyIDGHKgN4MiA3AioCEAAAAAMiA3AyIDUCI\
AAgIgABAyIDUCIgAQAAAAAiIDUCAAIAMiA1AgAAICIAAAIiA1AiICEDIgNQAAAAAycDUDIgNQMiA3AwAAEAAAAAMmA3AyIDUCIgN\
wIAAAAyIDUDIgIQIAABAgIAACAAAAMiA1AiIAECAAAQMiA1AiICADIgMQAAAAAyIDUDIwNwMiA3AgAAEAAAAAMig3AyIDUCIAAgI\
gAAAyIDUKIAAAIAAAAiAAUCAAAAMiA1AgAAICAAAAMiAxAiICEDJoNAIiAwAyIDUDooNxMiA3AwAAAAAAAAMiA3AyIDUCIgMwICA\
UAyIDUCAAAQAAAAAiAAACIgFAMmg1AgAAEAAAAAIiAkAiICUCIgNAAAAAAyIDUDIhNQMiA3AgAAEAAAAAMig1AyIDUDIgNwIiAQA\
yITUTIgBQMgAFAyIBUCIgFAMiElAiICEDIgAQMiA1AyICUDLwNRIiAxAyIDUD///////////////////////////////////////\
/////wAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// The code points [`COMMON_CJK`] marks.
pub fn common_cjk() -> BTreeSet<u32> {
    let mut bits = Vec::with_capacity(COMMON_CJK.len() * 6);
    for c in COMMON_CJK.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        bits.extend((0..6).rev().map(|i| v >> i & 1 == 1));
    }
    let code_points = COMMON_CJK_BLOCKS.iter().flat_map(|(first, last)| *first..=*last);
    code_points.zip(bits).filter(|(_, on)| *on).map(|(c, _)| c).collect()
}

// --- The subsetter ------------------------------------------------------------------------------

/// `font` (TrueType or CFF outlines) with only the characters `keep` keeps: a new cmap (format 12)
/// maps them, every other glyph's outline is emptied, and glyph ids stay as they were, so GSUB,
/// GPOS and the metrics tables are kept unchanged. Composite glyphs keep their components. A CFF
/// table is rewritten compact (offsets past its CharStrings moved); a glyf table with long offsets.
/// A font this cannot read is an error, never a panic.
pub fn subset(font: &[u8], keep: &dyn Fn(u32) -> bool) -> Result<Vec<u8>, String> {
    let face = ttf_parser::Face::parse(font, 0).map_err(|e| format!("not a font: {e}"))?;
    let mut map: BTreeMap<u32, u16> = BTreeMap::new();
    if let Some(cmap) = face.tables().cmap {
        for sub in cmap.subtables {
            if !sub.is_unicode() {
                continue;
            }
            sub.codepoints(|cp| {
                if keep(cp)
                    && let Some(g) = sub.glyph_index(cp)
                {
                    map.entry(cp).or_insert(g.0);
                }
            });
        }
    }
    if map.is_empty() {
        return Err("no character kept".into());
    }
    let mut glyphs: BTreeSet<u16> = map.values().copied().collect();
    glyphs.insert(0);
    gsub_closure(&face, &mut glyphs);
    let tables = tables(font)?;
    let has_glyf = tables.iter().any(|t| &t.tag == b"glyf");
    let (mut glyf, mut loca) = (None, None);
    if has_glyf {
        let (g, l) = subset_glyf(&tables, &mut glyphs, face.number_of_glyphs())?;
        glyf = Some(g);
        loca = Some(l);
    }
    let mut out = Vec::new();
    for t in &tables {
        let data = match &t.tag {
            b"DSIG" => continue,
            b"cmap" => cmap12(&map)?,
            b"glyf" => glyf.take().unwrap_or_default(),
            b"loca" => loca.take().unwrap_or_default(),
            b"head" if has_glyf => {
                // indexToLocFormat: long offsets.
                let mut head = t.data.to_vec();
                match head.get_mut(50..52) {
                    Some(format) => format.copy_from_slice(&[0, 1]),
                    None => return Err("a short head table".into()),
                }
                head
            }
            b"CFF " => subset_cff(t.data, &glyphs)?,
            _ => t.data.to_vec(),
        };
        out.push((t.tag, data));
    }
    let small = write_sfnt(version(font)?, out)?;
    ttf_parser::Face::parse(&small, 0).map_err(|e| format!("the subset does not parse: {e}"))?;
    Ok(small)
}

/// The GSUB features a shaper applies to horizontal text on its own (harfrust, under egui): their
/// output glyphs must survive the subset, or a kept character could shape into an emptied glyph.
const SHAPED_FEATURES: &[&[u8; 4]] = &[b"ccmp", b"locl", b"rlig", b"calt", b"liga", b"clig", b"rclt", b"rvrn", b"ljmo", b"vjmo", b"tjmo"];

/// Adds to `glyphs` every glyph the [`SHAPED_FEATURES`]' lookups (and the lookups their contextual
/// rules call) can substitute for them, until nothing more is added. A conservative closure: a
/// contextual rule's lookups count whatever the context.
pub fn gsub_closure(face: &ttf_parser::Face, glyphs: &mut BTreeSet<u16>) {
    use ttf_parser::GlyphId;
    use ttf_parser::gsub::SubstitutionSubtable as Sub;
    let Some(gsub) = face.tables().gsub else { return };
    // The features of each script's default language system: egui gives the shaper no language,
    // so a face's language-specific forms (Noto Sans CJK's Japanese, Korean and Traditional
    // `locl` glyphs) are never drawn.
    let mut lookups: BTreeSet<u16> = BTreeSet::new();
    for script in gsub.scripts {
        let Some(system) = script.default_language else { continue };
        for index in system.required_feature.into_iter().chain(system.feature_indices) {
            let Some(feature) = gsub.features.get(index) else { continue };
            if system.required_feature == Some(index) || SHAPED_FEATURES.iter().any(|t| ttf_parser::Tag::from_bytes(t) == feature.tag) {
                lookups.extend(feature.lookup_indices);
            }
        }
    }
    // The lookups contextual rules call.
    let mut todo: Vec<u16> = lookups.iter().copied().collect();
    while let Some(index) = todo.pop() {
        let Some(lookup) = gsub.lookups.get(index) else { continue };
        let mut called = Vec::new();
        for sub in lookup.subtables.into_iter::<Sub>() {
            match sub {
                Sub::Context(
                    ttf_parser::opentype_layout::ContextLookup::Format1 { sets, .. }
                    | ttf_parser::opentype_layout::ContextLookup::Format2 { sets, .. },
                ) => {
                    for set in sets {
                        for rule in set {
                            called.extend(rule.lookups.into_iter().map(|r| r.lookup_list_index));
                        }
                    }
                }
                Sub::Context(ttf_parser::opentype_layout::ContextLookup::Format3 { lookups, .. }) => {
                    called.extend(lookups.into_iter().map(|r| r.lookup_list_index))
                }
                Sub::ChainContext(
                    ttf_parser::opentype_layout::ChainedContextLookup::Format1 { sets, .. }
                    | ttf_parser::opentype_layout::ChainedContextLookup::Format2 { sets, .. },
                ) => {
                    for set in sets {
                        for rule in set {
                            called.extend(rule.lookups.into_iter().map(|r| r.lookup_list_index));
                        }
                    }
                }
                Sub::ChainContext(ttf_parser::opentype_layout::ChainedContextLookup::Format3 { lookups, .. }) => {
                    called.extend(lookups.into_iter().map(|r| r.lookup_list_index))
                }
                _ => {}
            }
        }
        for c in called {
            if lookups.insert(c) {
                todo.push(c);
            }
        }
    }
    // Each pass adds what the kept glyphs substitute to; at most one pass per lookup can add
    // anything new in a well-formed font, so the passes are capped.
    for _ in 0..=lookups.len() {
        let mut added: Vec<u16> = Vec::new();
        for lookup in lookups.iter().filter_map(|i| gsub.lookups.get(*i)) {
            for sub in lookup.subtables.into_iter::<Sub>() {
                for &g in glyphs.iter() {
                    let id = GlyphId(g);
                    match &sub {
                        Sub::Single(ttf_parser::gsub::SingleSubstitution::Format1 { coverage, delta }) if coverage.contains(id) => {
                            added.push(g.wrapping_add_signed(*delta))
                        }
                        Sub::Single(ttf_parser::gsub::SingleSubstitution::Format2 { coverage, substitutes }) => {
                            added.extend(coverage.get(id).and_then(|i| substitutes.get(i)).map(|s| s.0))
                        }
                        Sub::Multiple(m) => {
                            added.extend(m.coverage.get(id).and_then(|i| m.sequences.get(i)).into_iter().flat_map(|s| s.substitutes).map(|s| s.0))
                        }
                        Sub::Alternate(a) => {
                            added.extend(a.coverage.get(id).and_then(|i| a.alternate_sets.get(i)).into_iter().flat_map(|s| s.alternates).map(|s| s.0))
                        }
                        Sub::Ligature(l) => {
                            for lig in l.coverage.get(id).and_then(|i| l.ligature_sets.get(i)).into_iter().flatten() {
                                if lig.components.into_iter().all(|c| glyphs.contains(&c.0)) {
                                    added.push(lig.glyph.0);
                                }
                            }
                        }
                        Sub::ReverseChainSingle(r) => added.extend(r.coverage.get(id).and_then(|i| r.substitutes.get(i)).map(|s| s.0)),
                        _ => {}
                    }
                }
            }
        }
        let before = glyphs.len();
        glyphs.extend(added);
        if glyphs.len() == before {
            break;
        }
    }
}

fn bytes(d: &[u8], at: usize, len: usize) -> Result<&[u8], String> {
    at.checked_add(len).and_then(|end| d.get(at..end)).ok_or_else(|| format!("truncated at byte {at}"))
}

fn be16(d: &[u8], at: usize) -> Result<u16, String> {
    let b = bytes(d, at, 2)?;
    Ok(u16::from_be_bytes([b.first().copied().unwrap_or(0), b.get(1).copied().unwrap_or(0)]))
}

fn be32(d: &[u8], at: usize) -> Result<u32, String> {
    let b = bytes(d, at, 4)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(b);
    Ok(u32::from_be_bytes(a))
}

fn byte(d: &[u8], at: usize) -> Result<u8, String> {
    d.get(at).copied().ok_or_else(|| format!("truncated at byte {at}"))
}

fn version(font: &[u8]) -> Result<u32, String> {
    be32(font, 0)
}

struct Table<'a> {
    tag: [u8; 4],
    data: &'a [u8],
}

/// The table directory of a single font (not a collection).
fn tables(font: &[u8]) -> Result<Vec<Table<'_>>, String> {
    let count = usize::from(be16(font, 4)?);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let rec = 12 + 16 * i;
        let tag = bytes(font, rec, 4)?;
        let off = be32(font, rec + 8)? as usize;
        let len = be32(font, rec + 12)? as usize;
        let mut t = [0u8; 4];
        t.copy_from_slice(tag);
        out.push(Table { tag: t, data: bytes(font, off, len).map_err(|_| "a table past the end of the file".to_string())? });
    }
    Ok(out)
}

/// The kept glyphs of a glyf table (and their components, transitively), every other glyph empty:
/// the new glyf and its long loca.
fn subset_glyf(tables: &[Table], glyphs: &mut BTreeSet<u16>, count: u16) -> Result<(Vec<u8>, Vec<u8>), String> {
    let get = |tag: &[u8; 4]| tables.iter().find(|t| &t.tag == tag).map(|t| t.data).ok_or_else(|| "a glyf font without head or loca".to_string());
    let long = be16(get(b"head")?, 50)? == 1;
    let (loca, glyf) = (get(b"loca")?, get(b"glyf")?);
    let n = usize::from(count);
    let at = |i: usize| -> Result<usize, String> { if long { Ok(be32(loca, i * 4)? as usize) } else { Ok(usize::from(be16(loca, i * 2)?) * 2) } };
    let glyph = |i: usize| -> Result<&[u8], String> {
        let (s, e) = (at(i)?, at(i + 1)?);
        glyf.get(s..e).ok_or_else(|| format!("glyph {i} out of range"))
    };
    let mut todo: Vec<u16> = glyphs.iter().copied().collect();
    while let Some(g) = todo.pop() {
        let i = usize::from(g);
        if i >= n {
            continue;
        }
        let data = glyph(i)?;
        if data.len() < 10 || (be16(data, 0)? as i16) >= 0 {
            continue;
        }
        // A composite: its components, each once (a loop is cut by the set).
        let mut p = 10;
        loop {
            let flags = be16(data, p)?;
            let component = be16(data, p + 2)?;
            if glyphs.insert(component) {
                todo.push(component);
            }
            p += 4 + if flags & 1 != 0 { 4 } else { 2 };
            p += if flags & 8 != 0 {
                2
            } else if flags & 0x40 != 0 {
                4
            } else if flags & 0x80 != 0 {
                8
            } else {
                0
            };
            if flags & 0x20 == 0 {
                break;
            }
        }
    }
    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity((n + 1) * 4);
    for i in 0..n {
        new_loca.extend_from_slice(&offset32(new_glyf.len())?);
        if u16::try_from(i).is_ok_and(|g| glyphs.contains(&g)) {
            new_glyf.extend_from_slice(glyph(i)?);
            while new_glyf.len() % 4 != 0 {
                new_glyf.push(0);
            }
        }
    }
    new_loca.extend_from_slice(&offset32(new_glyf.len())?);
    Ok((new_glyf, new_loca))
}

fn offset32(n: usize) -> Result<[u8; 4], String> {
    u32::try_from(n).map(u32::to_be_bytes).map_err(|_| "a table over 4 GB".to_string())
}

/// A CFF INDEX: its offsets (1-based, from `base + 1`) and where it ends.
struct Index {
    offsets: Vec<usize>,
    base: usize,
    end: usize,
}

impl Index {
    fn count(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    /// Item `i`'s byte range in the CFF table.
    fn item(&self, i: usize) -> Result<(usize, usize), String> {
        match (self.offsets.get(i), self.offsets.get(i + 1)) {
            (Some(s), Some(e)) if s <= e => Ok((self.base + s, self.base + e)),
            _ => Err(format!("a bad INDEX item {i}")),
        }
    }
}

fn read_index(d: &[u8], at: usize) -> Result<Index, String> {
    let count = usize::from(be16(d, at)?);
    if count == 0 {
        return Ok(Index { offsets: Vec::new(), base: at + 2, end: at + 2 });
    }
    let off_size = usize::from(byte(d, at + 2)?);
    if !(1..=4).contains(&off_size) {
        return Err(format!("a CFF INDEX with offSize {off_size}"));
    }
    let mut offsets = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let b = bytes(d, at + 3 + i * off_size, off_size)?;
        let o = b.iter().fold(0usize, |a, x| (a << 8) | usize::from(*x));
        if o == 0 || offsets.last().is_some_and(|last| o < *last) {
            return Err("a CFF INDEX with offsets out of order".into());
        }
        offsets.push(o);
    }
    let base = at + 3 + (count + 1) * off_size - 1;
    let end = base + offsets.last().copied().unwrap_or(1);
    if end > d.len() {
        return Err("a CFF INDEX past the end of the table".into());
    }
    Ok(Index { offsets, base, end })
}

/// A CFF INDEX of `items`, with the smallest offset size that holds them.
fn write_index(items: &[&[u8]]) -> Result<Vec<u8>, String> {
    let total: usize = items.iter().map(|i| i.len()).sum::<usize>() + 1;
    let off_size: usize = match total {
        0..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFF_FFFF => 3,
        _ => 4,
    };
    let mut out = Vec::with_capacity(3 + (items.len() + 1) * off_size + total);
    out.extend_from_slice(&u16::try_from(items.len()).map_err(|_| "too many glyphs".to_string())?.to_be_bytes());
    out.push(off_size as u8);
    let mut at = 1usize;
    let mut offsets = vec![at];
    for item in items {
        at += item.len();
        offsets.push(at);
    }
    for o in offsets {
        let b = offset32(o)?;
        out.extend_from_slice(b.get(4 - off_size..).unwrap_or_default());
    }
    for item in items {
        out.extend_from_slice(item);
    }
    Ok(out)
}

/// One DICT operand: its integer value (0 for a real) and where its bytes are in the DICT.
#[derive(Clone, Copy)]
struct Operand {
    value: i64,
    at: usize,
    len: usize,
}

/// A DICT's entries: operator (two-byte ones as 1200 + the second byte) and operands.
fn read_dict(d: &[u8]) -> Result<Vec<(u16, Vec<Operand>)>, String> {
    let mut out = Vec::new();
    let mut operands = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let b0 = byte(d, i)?;
        match b0 {
            0..=21 => {
                let op = if b0 == 12 {
                    i += 1;
                    1200 + u16::from(byte(d, i)?)
                } else {
                    u16::from(b0)
                };
                out.push((op, std::mem::take(&mut operands)));
                i += 1;
            }
            28 => {
                operands.push(Operand { value: i64::from(be16(d, i + 1)? as i16), at: i, len: 3 });
                i += 3;
            }
            29 => {
                operands.push(Operand { value: i64::from(be32(d, i + 1)? as i32), at: i, len: 5 });
                i += 5;
            }
            30 => {
                let start = i;
                i += 1;
                loop {
                    let b = byte(d, i)?;
                    i += 1;
                    if b & 0x0f == 0x0f || b >> 4 == 0x0f {
                        break;
                    }
                }
                operands.push(Operand { value: 0, at: start, len: i - start });
            }
            32..=246 => {
                operands.push(Operand { value: i64::from(b0) - 139, at: i, len: 1 });
                i += 1;
            }
            247..=250 => {
                operands.push(Operand { value: (i64::from(b0) - 247) * 256 + i64::from(byte(d, i + 1)?) + 108, at: i, len: 2 });
                i += 2;
            }
            251..=254 => {
                operands.push(Operand { value: -(i64::from(b0) - 251) * 256 - i64::from(byte(d, i + 1)?) - 108, at: i, len: 2 });
                i += 2;
            }
            _ => return Err(format!("a bad CFF DICT byte {b0}")),
        }
    }
    Ok(out)
}

/// `value` as a DICT integer of exactly `len` bytes, if it fits.
fn encode_operand(value: i64, len: usize) -> Option<Vec<u8>> {
    match len {
        5 => i32::try_from(value).ok().map(|v| [&[29u8][..], &v.to_be_bytes()].concat()),
        3 => i16::try_from(value).ok().map(|v| [&[28u8][..], &v.to_be_bytes()].concat()),
        2 if (108..=1131).contains(&value) => Some(vec![((value - 108) / 256 + 247) as u8, ((value - 108) % 256) as u8]),
        2 if (-1131..=-108).contains(&value) => Some(vec![((-value - 108) / 256 + 251) as u8, ((-value - 108) % 256) as u8]),
        1 if (-107..=107).contains(&value) => Some(vec![(value + 139) as u8]),
        _ => None,
    }
}

/// Moves the offset operands of the DICT at `range` in `cff` that point at or past `from` back by
/// `by` bytes: `ops` names each offset operator and its operand's position.
fn shift_offsets(cff: &mut [u8], range: (usize, usize), ops: &[(u16, usize)], from: usize, by: usize) -> Result<(), String> {
    let dict = read_dict(cff.get(range.0..range.1).ok_or("a DICT past the end")?)?;
    for (op, operands) in dict {
        let Some((_, n)) = ops.iter().find(|(o, _)| *o == op) else { continue };
        let Some(operand) = operands.get(*n) else { continue };
        let Ok(old) = usize::try_from(operand.value) else { continue };
        if old < from {
            continue;
        }
        let moved = i64::try_from(old - by).map_err(|_| "an offset out of range".to_string())?;
        let encoded = encode_operand(moved, operand.len).ok_or_else(|| format!("CFF operator {op}'s offset does not fit its old width"))?;
        let at = range.0 + operand.at;
        cff.get_mut(at..at + operand.len).ok_or("a DICT operand past the end")?.copy_from_slice(&encoded);
    }
    Ok(())
}

/// A CFF table whose CharStrings keep only `glyphs` (every other one `endchar`), compacted: what
/// follows the CharStrings (in a CID font, the Font DICTs and their Private DICTs and subroutines)
/// moves up, and the Top DICT's and Font DICTs' offsets with it.
fn subset_cff(cff: &[u8], glyphs: &BTreeSet<u16>) -> Result<Vec<u8>, String> {
    if byte(cff, 0)? != 1 {
        return Err("not a CFF version 1 table".into());
    }
    let header = usize::from(byte(cff, 2)?);
    let names = read_index(cff, header)?;
    let top = read_index(cff, names.end)?;
    if top.count() != 1 {
        return Err("a CFF table with more than one font".into());
    }
    let top_range = top.item(0)?;
    let top_dict = read_dict(cff.get(top_range.0..top_range.1).ok_or("a Top DICT past the end")?)?;
    let offset = |op: u16| -> Option<usize> {
        top_dict.iter().find(|(o, _)| *o == op).and_then(|(_, a)| a.first()).and_then(|a| usize::try_from(a.value).ok())
    };
    let cs_at = offset(17).ok_or("a CFF table without CharStrings")?;
    let strings = read_index(cff, cs_at)?;
    let empty: &[u8] = &[14]; // endchar
    let mut items: Vec<&[u8]> = Vec::with_capacity(strings.count());
    for g in 0..strings.count() {
        let kept = u16::try_from(g).is_ok_and(|g| glyphs.contains(&g));
        items.push(if kept {
            let (s, e) = strings.item(g)?;
            cff.get(s..e).ok_or("a charstring past the end")?
        } else {
            empty
        });
    }
    let index = write_index(&items)?;
    let old_end = strings.end;
    let by = (old_end - cs_at).checked_sub(index.len()).ok_or("the CharStrings grew")?;
    let mut out = Vec::with_capacity(cff.len() - by);
    out.extend_from_slice(cff.get(..cs_at).ok_or("CharStrings past the end")?);
    out.extend_from_slice(&index);
    out.extend_from_slice(cff.get(old_end..).unwrap_or_default());
    // charset, Encoding, Private (size, offset), FDArray, FDSelect.
    shift_offsets(&mut out, top_range, &[(15, 0), (16, 0), (18, 1), (1236, 0), (1237, 0)], old_end, by)?;
    if let Some(fd_array) = offset(1236) {
        let fd_array = if fd_array >= old_end { fd_array - by } else { fd_array };
        let fonts = read_index(&out, fd_array)?;
        for i in 0..fonts.count() {
            shift_offsets(&mut out, fonts.item(i)?, &[(18, 1)], old_end, by)?;
        }
    }
    Ok(out)
}

/// A cmap with one format 12 subtable for `map`, under Unicode full repertoire (0, 4) and Windows
/// UCS-4 (3, 10).
fn cmap12(map: &BTreeMap<u32, u16>) -> Result<Vec<u8>, String> {
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for (&cp, &g) in map {
        if let Some(last) = groups.last_mut()
            && last.1 + 1 == cp
            && last.2 + (cp - last.0) == u32::from(g)
        {
            last.1 = cp;
            continue;
        }
        groups.push((cp, cp, u32::from(g)));
    }
    let n = u32::try_from(groups.len()).map_err(|_| "too many cmap groups".to_string())?;
    let mut out = Vec::with_capacity(36 + 12 * groups.len());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes());
    for (platform, encoding) in [(0u16, 4u16), (3, 10)] {
        out.extend_from_slice(&platform.to_be_bytes());
        out.extend_from_slice(&encoding.to_be_bytes());
        out.extend_from_slice(&20u32.to_be_bytes());
    }
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(16 + 12 * n).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    for (start, end, glyph) in groups {
        out.extend_from_slice(&start.to_be_bytes());
        out.extend_from_slice(&end.to_be_bytes());
        out.extend_from_slice(&glyph.to_be_bytes());
    }
    Ok(out)
}

/// A font file of `tables`, sorted by tag, each 4-byte aligned with its checksum.
fn write_sfnt(version: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Result<Vec<u8>, String> {
    tables.sort_by_key(|t| t.0);
    let n = u16::try_from(tables.len()).map_err(|_| "too many tables".to_string())?;
    let (mut pow, mut log) = (1u16, 0u16);
    while pow * 2 <= n {
        pow *= 2;
        log += 1;
    }
    let mut out = Vec::new();
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&(pow * 16).to_be_bytes());
    out.extend_from_slice(&log.to_be_bytes());
    out.extend_from_slice(&(n * 16 - pow * 16).to_be_bytes());
    let directory = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&offset32(directory + body.len())?);
        out.extend_from_slice(&offset32(data.len())?);
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    Ok(out)
}

fn checksum(d: &[u8]) -> u32 {
    d.chunks(4).fold(0u32, |a, c| {
        let mut b = [0u8; 4];
        b.iter_mut().zip(c).for_each(|(x, y)| *x = *y);
        a.wrapping_add(u32::from_be_bytes(b))
    })
}
