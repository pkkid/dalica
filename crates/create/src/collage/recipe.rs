//! The `.dalica-collage` recipe: a collage written beside its JPEG, so Edit Collage reopens it
//! (`creating.md` §3). JSON: the version, every field of the [`Collage`], `draft`, each clip's id
//! and path, and the output's size.
//!
//! Printed like a sidecar (keys sorted by their bytes at every level, two-space indentation, LF,
//! a final newline), so a document round-trips through its recipe byte for byte. Read under
//! limits ([`MAX_BYTES`], [`MAX_DEPTH`], [`MAX_CLIPS`], every number finite and in range): a
//! hostile recipe is refused with a [`RecipeError`], never a panic. Keys Dalica does not know are
//! kept and printed back (objects merge key by key, lists of the same length item by item).
//!
//! A path from a recipe is only ever matched against the photos already indexed, never opened
//! (`security.md` §5.5).

use dalica_core::PhotoId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::doc::{Background, Clip, Collage, DocError, MAX_CLIPS, Move, Settings, Style, hex_u64};
use super::page::{MAX_PIXELS, Page};

/// The recipe's file extension (without the dot).
pub const EXTENSION: &str = "dalica-collage";
/// The format version this crate writes and reads.
pub const VERSION: u64 = 1;
/// The largest recipe read, in bytes.
pub const MAX_BYTES: usize = 1 << 20;
/// The deepest nesting read.
pub const MAX_DEPTH: usize = 32;
/// The longest path a clip may carry, in bytes.
pub const MAX_PATH: usize = 32 * 1024;

/// Why a recipe is refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecipeError {
    #[error("the recipe is {0} bytes, over the limit of {MAX_BYTES}")]
    TooLarge(usize),
    #[error("the recipe is not valid JSON: {0}")]
    Json(String),
    #[error("over the recipe's limits: {0}")]
    Limit(String),
    #[error("not a collage recipe: {0}")]
    Shape(String),
    #[error("made by a newer version of Dalica (recipe version {0})")]
    Newer(u64),
    #[error(transparent)]
    Doc(#[from] DocError),
}

/// A recipe: the collage, each clip's path, whether it is a draft, the output's size, and every
/// key of the file it was read from.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub collage: Collage,
    /// Each clip's path, by its position in the Clips.
    pub paths: Vec<String>,
    /// Save Draft wrote it (the room was closed without Create Collage).
    pub draft: bool,
    /// The written picture's size in pixels.
    pub output: Option<(u32, u32)>,
    /// The file as read (empty for a new recipe): its unknown keys are printed back.
    raw: Map<String, Value>,
}

/// The file's shape.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    dalica: Header,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    output: Option<Size>,
    style: Style,
    settings: Settings,
    page: Page,
    #[serde(with = "hex_u64")]
    seed: u64,
    #[serde(with = "hex_u64")]
    shuffle: u64,
    clips: Vec<FileClip>,
    background: Background,
    #[serde(default)]
    frame_centre: Option<PhotoId>,
    #[serde(default)]
    moves: Vec<Move>,
    #[serde(default)]
    swaps: Vec<[PhotoId; 2]>,
}

#[derive(Serialize, Deserialize)]
struct Header {
    version: u64,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileClip {
    id: PhotoId,
    path: String,
    on_page: bool,
}

impl Recipe {
    /// A new recipe of `collage`, with `paths[i]` the path of clip `i`. Refused when the collage
    /// fails [`Collage::check`], a path is missing or too long, or the output's size is out of
    /// range.
    pub fn new(collage: Collage, paths: Vec<String>, draft: bool, output: Option<(u32, u32)>) -> Result<Recipe, RecipeError> {
        let r = Recipe { collage, paths, draft, output, raw: Map::new() };
        r.check()?;
        Ok(r)
    }

    /// Reads a recipe under the limits.
    pub fn parse(bytes: &[u8]) -> Result<Recipe, RecipeError> {
        if bytes.len() > MAX_BYTES {
            return Err(RecipeError::TooLarge(bytes.len()));
        }
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let value: Value = serde_json::from_slice(bytes).map_err(|e| RecipeError::Json(e.to_string()))?;
        check_depth(&value, 0)?;
        let Value::Object(raw) = value else {
            return Err(RecipeError::Shape("the top level is not an object".into()));
        };
        let version = raw.get("dalica").and_then(|d| d.get("version")).and_then(Value::as_u64);
        match version {
            None | Some(0) => return Err(RecipeError::Shape("no dalica.version".into())),
            Some(v) if v > VERSION => return Err(RecipeError::Newer(v)),
            Some(_) => {}
        }
        if let Some(n) = raw.get("clips").and_then(Value::as_array).map(Vec::len)
            && n > MAX_CLIPS
        {
            return Err(RecipeError::Limit(format!("{n} clips, over {MAX_CLIPS}")));
        }
        let file: File = serde_json::from_value(Value::Object(raw.clone())).map_err(|e| RecipeError::Shape(e.to_string()))?;
        let (clips, paths) = file.clips.into_iter().map(|c| (Clip { id: c.id, on_page: c.on_page }, c.path)).unzip();
        let collage = Collage {
            style: file.style,
            settings: file.settings,
            page: file.page,
            seed: file.seed,
            shuffle: file.shuffle,
            clips,
            background: file.background,
            frame_centre: file.frame_centre,
            moves: file.moves,
            swaps: file.swaps,
        };
        let r = Recipe { collage, paths, draft: file.draft, output: file.output.map(|s| (s.width, s.height)), raw };
        r.check()?;
        Ok(r)
    }

    /// The recipe as bytes: the known fields over the file it was read from.
    pub fn print(&self) -> Vec<u8> {
        let c = &self.collage;
        let clips = c
            .clips
            .iter()
            .enumerate()
            .map(|(i, clip)| FileClip { id: clip.id, path: self.paths.get(i).cloned().unwrap_or_default(), on_page: clip.on_page })
            .collect();
        let file = File {
            dalica: Header { version: VERSION },
            draft: self.draft,
            output: self.output.map(|(width, height)| Size { width, height }),
            style: c.style,
            settings: c.settings,
            page: c.page.clone(),
            seed: c.seed,
            shuffle: c.shuffle,
            clips,
            background: c.background,
            frame_centre: c.frame_centre,
            moves: c.moves.clone(),
            swaps: c.swaps.clone(),
        };
        // Serialising plain data with string keys cannot fail; an empty object would only ever
        // stand for a bug, and the recipe is parsed back before it is written.
        let known = serde_json::to_value(file).unwrap_or(Value::Null);
        let merged = merge(Value::Object(self.raw.clone()), known);
        let mut out = String::new();
        print_value(&mut out, &merged, 0);
        out.push('\n');
        out.into_bytes()
    }

    fn check(&self) -> Result<(), RecipeError> {
        self.collage.check()?;
        if self.paths.len() != self.collage.clips.len() {
            return Err(RecipeError::Shape(format!("{} paths for {} clips", self.paths.len(), self.collage.clips.len())));
        }
        if let Some(p) = self.paths.iter().find(|p| p.len() > MAX_PATH) {
            return Err(RecipeError::Limit(format!("a path of {} bytes", p.len())));
        }
        if let Some((w, h)) = self.output
            && !((1..=MAX_PIXELS).contains(&w) && (1..=MAX_PIXELS).contains(&h))
        {
            return Err(RecipeError::Limit(format!("an output of {w} × {h} pixels")));
        }
        Ok(())
    }
}

fn check_depth(v: &Value, depth: usize) -> Result<(), RecipeError> {
    if depth > MAX_DEPTH {
        return Err(RecipeError::Limit(format!("nesting deeper than {MAX_DEPTH}")));
    }
    match v {
        Value::Array(a) => a.iter().try_for_each(|x| check_depth(x, depth + 1)),
        Value::Object(m) => m.values().try_for_each(|x| check_depth(x, depth + 1)),
        _ => Ok(()),
    }
}

/// `known` over `raw`: objects key by key, lists of the same length item by item, anything else
/// `known`.
fn merge(raw: Value, known: Value) -> Value {
    match (raw, known) {
        (Value::Object(mut r), Value::Object(k)) => {
            for (key, v) in k {
                let old = r.remove(&key).unwrap_or(Value::Null);
                r.insert(key, merge(old, v));
            }
            Value::Object(r)
        }
        (Value::Array(r), Value::Array(k)) if r.len() == k.len() => Value::Array(r.into_iter().zip(k).map(|(r, k)| merge(r, k)).collect()),
        (_, known) => known,
    }
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

/// The sidecar's printing: keys sorted by their bytes, two spaces, LF.
fn print_value(out: &mut String, v: &Value, level: usize) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                indent(out, level + 1);
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push_str(": ");
                if let Some(v) = m.get(*k) {
                    print_value(out, v, level + 1);
                }
                out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" });
            }
            indent(out, level);
            out.push('}');
        }
        Value::Array(a) if !a.is_empty() => {
            out.push_str("[\n");
            for (i, v) in a.iter().enumerate() {
                indent(out, level + 1);
                print_value(out, v, level + 1);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            indent(out, level);
            out.push(']');
        }
        Value::Object(_) => out.push_str("{}"),
        Value::Array(_) => out.push_str("[]"),
        scalar => out.push_str(&scalar.to_string()),
    }
}
