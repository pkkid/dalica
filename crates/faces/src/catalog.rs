//! The user's own list of models to download: `catalog.json` in the models folder (`faces.md`
//! §3.2).
//!
//! Dalica's built-in list holds only models whose terms let the project point at them
//! ([`crate::known`]). Anything else (a stronger recogniser whose weights are for research use, a
//! model the user trained, a mirror they trust) can be added by the user: each entry is a model
//! manifest ([`crate::manifest`]) plus the address to fetch it from, and it then gets the same
//! download as a built-in one, its licence notice shown first (in the caution colour). Dalica ships
//! and links to none of them.
//!
//! ```json
//! { "models": [ {
//!     "id": "my-recogniser", "name": "My recogniser", "version": "1", "role": "embedder",
//!     "url": "https://example.org/weights/recogniser.onnx",
//!     "sha256": "…64 lowercase hex digits…", "sizeBytes": 166000000,
//!     "licence": { "name": "Research use only", "commercial": "no", "notice": "Not for commercial use." },
//!     "provenance": "Trained on …",
//!     "output": { "kind": "embedding", "dim": 512 },
//!     "thresholds": { "matchCosine": 0.4 },
//!     "calibration": [0.30, 0.31, 0.32, 0.33, 0.35, 0.37, 0.38, 0.40, 0.43, 0.47]
//! } ] }
//! ```
//!
//! `input` may be left out (112 × 112 RGB, `(x − 127.5) / 127.5`); `calibration` is optional. The
//! file is untrusted: it is read with a size limit, every entry is validated, and a bad entry is
//! reported and skipped, never a reason to refuse the rest. `sha256` and `sizeBytes` are required,
//! since a download is checked against them, and so is a licence.
//!
//! Ported from LightCraft `crates/faces/src/catalog.rs` @ f34c7a8a (MIT OR Apache-2.0).

use serde::Deserialize;
use serde_json::Value;

use crate::known::{self, Download};
use crate::manifest::{self, ModelManifest, Role};

/// Largest `catalog.json` read.
pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
/// Most entries taken from it.
pub const MAX_ENTRIES: usize = 50;

/// One model of the catalog: its manifest and where to fetch it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub manifest: ModelManifest,
    pub url: String,
    /// The file's name in the models folder, from the address.
    pub file_name: String,
}

impl Entry {
    /// What the download needs (`sha256` and `size_bytes` are present: [`parse`] requires them).
    pub fn download(&self) -> Option<Download> {
        Some(Download {
            id: self.manifest.id.clone(),
            url: self.url.clone(),
            file_name: self.file_name.clone(),
            size_bytes: self.manifest.size_bytes?,
            sha256: self.manifest.sha256.clone()?,
        })
    }
}

/// What a catalog file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    /// One plain sentence for each entry (or the file) that could not be used.
    pub errors: Vec<String>,
}

impl Catalog {
    /// The entries' manifests, to recognise a dropped file by its hash ([`known::recognise`]).
    pub fn manifests(&self) -> Vec<ModelManifest> {
        self.entries.iter().map(|e| e.manifest.clone()).collect()
    }
}

#[derive(Deserialize)]
struct Raw {
    url: String,
    #[serde(flatten)]
    manifest: ModelManifest,
}

/// A file name for the download, from the address: letters, digits, `.`, `_`, `-`, ending in
/// `.onnx`, never hidden.
fn file_name_of(url: &str) -> String {
    let last = url.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("");
    let clean: String = last.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).take(80).collect();
    if clean.ends_with(".onnx") && clean.len() > 5 && !clean.starts_with('.') { clean } else { "model.onnx".to_string() }
}

fn entry(v: &Value) -> Result<Entry, String> {
    let raw: Raw = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    manifest::validate(&raw.manifest).map_err(|e| e.to_string())?;
    let m = &raw.manifest;
    if m.role != Role::Embedder {
        return Err("only face recognisers (role \"embedder\") can be listed".into());
    }
    if m.sha256.is_none() || m.size_bytes.is_none() {
        return Err("needs `sha256` and `sizeBytes`: a download is checked against them".into());
    }
    if !manifest::valid_https(&raw.url) {
        return Err("`url` must be an https:// address of at most 500 characters, without spaces".into());
    }
    if known::all().iter().any(|k| k.id == m.id) {
        return Err("that id belongs to a model Dalica already knows".into());
    }
    if m.sha256.as_deref().is_some_and(|h| known::lookup(h).is_some()) {
        return Err("Dalica already knows a model with that file".into());
    }
    Ok(Entry { file_name: file_name_of(&raw.url), url: raw.url, manifest: raw.manifest })
}

/// Reads a catalog file's bytes: the usable entries, and why the others were left out.
pub fn parse(bytes: &[u8]) -> Catalog {
    let mut out = Catalog::default();
    if bytes.len() > MAX_CATALOG_BYTES {
        out.errors.push(format!("catalog.json is larger than {} KB: not read", MAX_CATALOG_BYTES / 1024));
        return out;
    }
    // some editors begin a UTF-8 file with a byte-order mark
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => {
            out.errors.push(format!("catalog.json is not valid JSON: {e}"));
            return out;
        }
    };
    let Some(models) = value.get("models").and_then(Value::as_array) else {
        out.errors.push("catalog.json needs a \"models\" list".into());
        return out;
    };
    if models.len() > MAX_ENTRIES {
        out.errors.push(format!("only the first {MAX_ENTRIES} models of the catalog are read"));
    }
    for (i, v) in models.iter().take(MAX_ENTRIES).enumerate() {
        let label = v
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| format!("model {}", i + 1), |id| format!("`{}`", id.chars().take(40).collect::<String>()));
        match entry(v) {
            Ok(e) if out.entries.iter().any(|x| x.manifest.id == e.manifest.id) => {
                out.errors.push(format!("{label}: listed twice, the first is used"))
            }
            Ok(e) => out.entries.push(e),
            Err(why) => out.errors.push(format!("{label}: {why}")),
        }
    }
    out
}
