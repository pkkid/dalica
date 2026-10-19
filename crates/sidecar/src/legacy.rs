//! The legacy organizer's per-folder sidecar: a bounded, read-only reader (`data-model.md` §9,
//! `security.md` §4.2).
//!
//! **Provenance.** This reader is written from public descriptions of the file format (community
//! write-ups of the format, other open-source readers' prose documentation, never their code) and
//! from black-box observation of files the legacy organizer wrote, as recorded in the project's
//! private research notes (the data-and-files study). No code of the legacy organizer or of any
//! GPL reader was looked at or used.
//!
//! **The format.** A plain INI file: `[section]` lines and `key=value` lines split on the first
//! `=`, LF or CRLF line ends, no comments (`;` and `#` are data), no quoting or escapes. Sections
//! come in any order; a repeated section or key is merged here, the later value winning. Garbage
//! seen in real files (`[(null)]` sections, empty section names, byte-reversed lines) is skipped by
//! the typed readers while the rest is kept. Text is UTF-8 when the file says so (an `[encoding]`
//! section with `utf8=1`, or a UTF-8 byte-order mark) and Windows-1252 otherwise; bytes that do not
//! decode become U+FFFD and mark the text [`Text::lossy`] (shown, never written back).
//!
//! **Limits** ([`LegacyLimits`]): the file ≤ 64 MiB, a line ≤ 1 MiB, ≤ 1 000 000 sections, ≤ 1 000
//! keys in a section. A file past one is refused with a [`LegacyError`] naming the limit; nothing in
//! it is used. Nothing else fails the whole file: a value that cannot be read is left out.
//!
//! **What is read.** The folder's section ([`LegacySidecar::folder`]), one section per file named
//! exactly as on disk ([`LegacySidecar::file`]), album definitions ([`LegacySidecar::albums`]) and
//! named contacts ([`LegacySidecar::contacts`]). The originals folder ([`originals_dir_name`]) holds
//! a sidecar of the same format with the saved photos' recipes; it is read by the same
//! [`LegacySidecar::parse`]. The file names are built from a masked constant ([`sidecar_names`]).

mod names;
mod read;
mod values;

use std::collections::{BTreeMap, HashMap};

use dalica_core::DateTime;

use crate::fields::Geo;

pub use names::{folder_section, originals_dir_name, sidecar_names};
pub use values::{IGNORED_CONTACT, any_date, contact_id, day_count_date, decode_rect, decode_rect64, text_date};

/// The section that says the file is UTF-8.
const ENCODING_SECTION: &str = "encoding";
/// The section of named contacts (`<id>=<name>;<email>;<account>`).
const CONTACTS_SECTION: &str = "Contacts2";
/// Section names that are never a file.
const NOT_FILES: [&str; 4] = [ENCODING_SECTION, "Contacts", CONTACTS_SECTION, "photoid"];
/// A corruption artifact seen in real files.
const NULL_SECTION: &str = "(null)";
/// The prefix of an album definition's section name.
const ALBUM_PREFIX: &str = ".album:";
/// The folder category that hides it.
const HIDDEN_CATEGORY: &str = "Hidden Folders";

/// Size and shape limits for reading (`security.md` §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyLimits {
    pub max_bytes: usize,
    pub max_line: usize,
    pub max_sections: usize,
    pub max_keys: usize,
}

impl Default for LegacyLimits {
    fn default() -> LegacyLimits {
        LegacyLimits { max_bytes: 64 << 20, max_line: 1 << 20, max_sections: 1_000_000, max_keys: 1000 }
    }
}

/// Why a legacy sidecar is refused: one of the limits, by name. Nothing in the file is used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LegacyError {
    #[error("the legacy sidecar is {size} bytes, over the file limit of {limit}")]
    FileTooLarge { size: usize, limit: usize },
    #[error("line {line} of the legacy sidecar is over the line limit of {limit} bytes")]
    LineTooLong { line: usize, limit: usize },
    #[error("the legacy sidecar has more than {limit} sections")]
    TooManySections { limit: usize },
    #[error("section `{section}` of the legacy sidecar has more than {limit} keys")]
    TooManyKeys { section: String, limit: usize },
}

/// Decoded text, and whether some of its bytes did not decode (U+FFFD stands in for them): such
/// text is shown but never written into Dalica's files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    pub lossy: bool,
}

/// One section: its name and its keys in first-seen order, values as raw bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    name: Text,
    utf8: bool,
    keys: Vec<(String, Vec<u8>)>,
}

impl Section {
    pub fn name(&self) -> &Text {
        &self.name
    }

    /// The key names in first-seen order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.keys.iter().map(|(k, _)| k.as_str())
    }

    /// A value's raw bytes.
    pub fn raw(&self, key: &str) -> Option<&[u8]> {
        self.keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_slice())
    }

    /// A value, decoded.
    pub fn get(&self, key: &str) -> Option<Text> {
        self.raw(key).map(|v| read::decode(v, self.utf8))
    }

    /// A value, decoded, `None` when absent or blank.
    fn text(&self, key: &str) -> Option<Text> {
        self.get(key).filter(|t| !t.text.trim().is_empty())
    }

    fn yes(&self, key: &str) -> bool {
        self.raw(key).is_some_and(|v| v.trim_ascii() == b"yes")
    }
}

/// The folder's own data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegacyFolder {
    pub description: Option<Text>,
    /// From the day-count form.
    pub date: Option<DateTime>,
    pub place: Option<Text>,
    /// Whether the folder's category is the hidden one.
    pub hidden: bool,
}

/// Whose face it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyPerson {
    /// A contact id, 16 lowercase hexadecimal digits; its name is in [`LegacySidecar::contacts`]
    /// of this folder or an ancestor's, or nowhere (an unnamed face).
    Contact(String),
    /// The ignore marker ([`IGNORED_CONTACT`]).
    Ignored,
}

/// A face: its rectangle `[x0, y0, x1, y1]` in the stored pixels (before any EXIF orientation),
/// normalised, and whose it is.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyFace {
    pub rect: [f64; 4],
    pub person: LegacyPerson,
}

/// The text overlay, kept whole as written, and the text of each of its blocks (read through its
/// length prefixes, so `;` and line breaks inside the text are kept).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextOverlay {
    pub raw: Text,
    pub texts: Vec<Text>,
}

/// One file's data. Absent or unreadable values are empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegacyFile {
    pub star: bool,
    pub hidden: bool,
    pub caption: Option<Text>,
    pub keywords: Vec<Text>,
    /// Quarter turns clockwise, 0–3.
    pub rotation: Option<u8>,
    /// The flip value as written (`flipped(n)`).
    pub flip: Option<u8>,
    /// The effective crop, `[x0, y0, x1, y1]` in the stored pixels.
    pub crop: Option<[f64; 4]>,
    /// The edit list, one raw token per operation (`name=1,params…`), in order.
    pub edits: Vec<String>,
    /// The undone operations available to redo, as raw tokens.
    pub redo: Vec<String>,
    pub text: Option<TextOverlay>,
    pub text_active: Option<bool>,
    pub faces: Vec<LegacyFace>,
    /// Face items that could not be read.
    pub bad_faces: usize,
    /// Album tokens (32 lowercase hexadecimal digits), in order, each once.
    pub albums: Vec<String>,
    pub geotag: Option<Geo>,
}

/// An album definition.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyAlbum {
    /// 32 lowercase hexadecimal digits.
    pub token: String,
    pub name: Text,
    pub date: Option<DateTime>,
    pub description: Option<Text>,
    pub place: Option<Text>,
}

/// A parsed legacy sidecar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacySidecar {
    utf8: bool,
    sections: Vec<Section>,
    exact: HashMap<String, usize>,
    folded: HashMap<String, usize>,
}

impl LegacySidecar {
    /// Reads a legacy sidecar (or the originals folder's) under `limits`.
    pub fn parse(bytes: &[u8], limits: &LegacyLimits) -> Result<LegacySidecar, LegacyError> {
        let read::Read { utf8, sections } = read::read(bytes, limits)?;
        let mut exact = HashMap::new();
        let mut folded = HashMap::new();
        for (i, s) in sections.iter().enumerate() {
            exact.entry(s.name.text.clone()).or_insert(i);
            folded.entry(s.name.text.to_lowercase()).or_insert(i);
        }
        Ok(LegacySidecar { utf8, sections, exact, folded })
    }

    /// Whether the text is read as UTF-8 (otherwise Windows-1252).
    pub fn is_utf8(&self) -> bool {
        self.utf8
    }

    /// Every section, in first-seen order.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// A section by its exact name.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.exact.get(name).and_then(|&i| self.sections.get(i))
    }

    /// The folder's own data.
    pub fn folder(&self) -> LegacyFolder {
        let Some(s) = self.section(&folder_section()) else { return LegacyFolder::default() };
        let category = s.get("P2category").or_else(|| s.get("category"));
        LegacyFolder {
            description: s.text("description"),
            date: s.get("date").and_then(|d| day_count_date(&d.text)),
            place: s.text("location"),
            hidden: category.is_some_and(|c| c.text.trim() == HIDDEN_CATEGORY),
        }
    }

    /// The names of the sections that describe a file, in first-seen order: every section but the
    /// folder's, the encoding, contacts, album definitions and garbage (`(null)`, names that are not
    /// one plain path component).
    pub fn file_names(&self) -> Vec<&str> {
        let folder = folder_section();
        self.sections.iter().map(|s| s.name.text.as_str()).filter(|n| is_file_section(n, &folder)).collect()
    }

    /// One file's data, found by its exact name, else ignoring case. `None` when no file section
    /// has that name.
    pub fn file(&self, name: &str) -> Option<LegacyFile> {
        let i = self.exact.get(name).or_else(|| self.folded.get(&name.to_lowercase()))?;
        let s = self.sections.get(*i)?;
        if !is_file_section(&s.name.text, &folder_section()) {
            return None;
        }
        let (faces, bad_faces) = s.get("faces").map(|f| values::faces(&f.text)).unwrap_or_default();
        let mut albums: Vec<String> = Vec::new();
        for token in s.get("albums").map(|a| a.text).unwrap_or_default().split(',').filter_map(values::album_token) {
            if !albums.contains(&token) {
                albums.push(token);
            }
        }
        let tokens = |key: &str| {
            s.get(key).map(|t| t.text.split(';').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()).unwrap_or_default()
        };
        Some(LegacyFile {
            star: s.yes("star"),
            hidden: s.yes("hidden"),
            caption: s.text("caption"),
            keywords: s.get("keywords").map(|k| values::list(&k, ',')).unwrap_or_default(),
            rotation: s.get("rotate").and_then(|r| values::call_number(&r.text, "rotate")).filter(|n| *n <= 3),
            flip: s.get("flipped").and_then(|f| values::call_number(&f.text, "flipped")),
            crop: s.get("crop").and_then(|c| decode_rect64(&c.text)),
            edits: tokens("filters"),
            redo: tokens("redo"),
            text: s.raw("text").filter(|t| !t.is_empty()).map(|t| values::overlay(t, self.utf8)),
            text_active: s.raw("textactive").and_then(|v| match v.trim_ascii() {
                b"1" => Some(true),
                b"0" => Some(false),
                _ => None,
            }),
            faces,
            bad_faces,
            albums,
            geotag: s.get("geotag").and_then(|g| values::geotag(&g.text)),
        })
    }

    /// The album definitions, in first-seen order, each token once (a later definition replaces an
    /// earlier one). A definition without a name (a deleted album's tombstone) or without a usable
    /// token is skipped.
    pub fn albums(&self) -> Vec<LegacyAlbum> {
        let mut out: Vec<LegacyAlbum> = Vec::new();
        for s in self.sections.iter().filter(|s| s.name.text.starts_with(ALBUM_PREFIX)) {
            let token = s
                .get("token")
                .and_then(|t| values::album_token(&t.text))
                .or_else(|| s.name.text.strip_prefix(ALBUM_PREFIX).and_then(values::album_token));
            let (Some(token), Some(name)) = (token, s.text("name")) else { continue };
            let album = LegacyAlbum {
                token,
                name,
                date: s.get("date").and_then(|d| any_date(&d.text)),
                description: s.text("description"),
                place: s.text("location"),
            };
            match out.iter_mut().find(|a| a.token == album.token) {
                Some(slot) => *slot = album,
                None => out.push(album),
            }
        }
        out
    }

    /// The named contacts of this folder: contact id (16 lowercase hexadecimal digits) → name.
    /// Only the three-field form `<name>;<email>;<account>` with a name is read.
    pub fn contacts(&self) -> BTreeMap<String, Text> {
        let mut out = BTreeMap::new();
        let Some(s) = self.section(CONTACTS_SECTION) else { return out };
        for key in s.keys() {
            let (Some(id), Some(value)) = (contact_id(key), s.get(key)) else { continue };
            let fields: Vec<&str> = value.text.split(';').collect();
            if let [name, _, _] = fields.as_slice()
                && !name.trim().is_empty()
            {
                out.insert(id, values::part(&value, name.trim()));
            }
        }
        out
    }
}

fn is_file_section(name: &str, folder: &str) -> bool {
    name != folder && name != NULL_SECTION && !NOT_FILES.contains(&name) && !name.starts_with(ALBUM_PREFIX) && crate::plain_name(name)
}
