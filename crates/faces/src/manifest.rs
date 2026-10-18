//! A face model's manifest: what the model is, what it needs and who may use it, as plain data
//! (`faces.md` §3.2).
//!
//! A manifest comes from the known-model table ([`crate::known`]), from the assumptions stated for a
//! file the user dropped in ([`crate::known::recognise`]), or from an entry of the user's own
//! `catalog.json` ([`crate::catalog`]). All of them are checked by [`validate`] before a manifest is
//! used, and [`parse`] reads JSON of bounded size. Everything the rest of Dalica relies on (sizes it
//! allocates, numbers it divides by, text it shows, addresses it fetches) is bounded here, and a
//! manifest without a licence or with an address that is not `https://` is refused.
//!
//! The JSON field names are LightCraft's bring-your-own format (`matchCosine`, `sizeBytes`, …), so
//! a catalog written for one works in the other.
//!
//! Ported from LightCraft `crates/faces/src/manifest.rs` @ f34c7a8a (MIT OR Apache-2.0); Dalica
//! requires the licence, adds the calibration table (`faces.md` §5) and leaves out the speed ratio.

use serde::{Deserialize, Serialize};

use crate::calibration::Calibration;
use crate::limits::MAX_MODEL_BYTES;

/// Largest manifest read.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// What a model does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Finds faces: boxes, scores and five landmarks.
    Detector,
    /// Turns an aligned face into a vector whose cosine to another says whether they are the same
    /// person (the recogniser).
    Embedder,
}

/// Whether the weights' licence allows commercial use. It says nothing about the training data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Commercial {
    Yes,
    /// Research or personal use only: never offered, hosted or linked by Dalica.
    No,
    /// The licence or its reach is unclear: the user is told so before installing.
    Unknown,
}

/// The weights' licence as the user reads it before a model is installed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Licence {
    /// An SPDX id or a short name ("MIT", "Apache-2.0", "Research use only"). Required.
    pub name: String,
    pub commercial: Commercial,
    /// Where the licence text is (`https://` only).
    pub url: Option<String>,
    /// The terms in plain words, shown before the model is installed. Required.
    pub notice: String,
}

impl Default for Licence {
    fn default() -> Self {
        Self { name: String::new(), commercial: Commercial::Unknown, url: None, notice: String::new() }
    }
}

/// The order of the colour planes the model reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Colour {
    Rgb,
    Bgr,
}

/// How a picture is fitted to the model's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Resize {
    /// Stretched to the input size (aligned face crops).
    Stretch,
    /// Aspect ratio kept, padded at the right and bottom (whole photos, for a detector).
    Letterbox,
}

/// How a picture becomes the model's input: NCHW float32, `(pixel − mean) / std` per channel with
/// pixels in 0 … 255.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct InputSpec {
    pub width: u32,
    pub height: u32,
    pub colour: Colour,
    pub mean: [f32; 3],
    pub std: [f32; 3],
    pub resize: Resize,
}

impl Default for InputSpec {
    /// The usual recogniser convention: 112 × 112 aligned RGB faces, `(x − 127.5) / 127.5`.
    fn default() -> Self {
        Self { width: 112, height: 112, colour: Colour::Rgb, mean: [127.5; 3], std: [127.5; 3], resize: Resize::Stretch }
    }
}

/// What the model gives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutputSpec {
    /// One vector of `dim` numbers per face.
    Embedding { dim: u32 },
    /// Detector outputs that a built-in decoder understands ([`crate::detect::DECODERS`]).
    Detector { decoder: String },
}

/// The model's recommended thresholds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Thresholds {
    /// Recognisers: the cosine at or above which two faces match; the Suggestion and Grouping
    /// confidence 85 (`faces.md` §5).
    pub match_cosine: Option<f32>,
    /// Detectors: the lowest detection score kept.
    pub score: Option<f32>,
    /// Detectors: the overlap (intersection over union) above which two boxes are one face.
    pub nms_iou: Option<f32>,
}

/// Everything Dalica knows about a face model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    /// The model's id and its folder name: see [`valid_id`].
    pub id: String,
    pub name: String,
    pub version: String,
    pub role: Role,
    #[serde(default)]
    pub licence: Licence,
    /// The model's own page (`https://` only), for the user's information.
    #[serde(default)]
    pub source: Option<String>,
    /// Lowercase hex SHA-256 of the `.onnx` file, when known.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// What the model was trained on, in plain words; says so when that is not known.
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub input: InputSpec,
    pub output: OutputSpec,
    #[serde(default)]
    pub thresholds: Thresholds,
    /// Recognisers: one cosine per confidence step 50, 55, …, 95, never falling (`faces.md` §5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration: Option<[f32; 10]>,
}

impl ModelManifest {
    /// The confidence → cosine map of a recogniser: its table, else its `matchCosine` with the
    /// linear rule; `None` for a detector or a recogniser with neither.
    pub fn calibration(&self) -> Option<Calibration> {
        if self.role != Role::Embedder {
            return None;
        }
        match (self.calibration, self.thresholds.match_cosine) {
            (Some(table), _) => Calibration::table(table).ok(),
            (None, Some(cosine)) => Calibration::linear(cosine).ok(),
            (None, None) => None,
        }
    }

    /// The embedding dimension of a recogniser.
    pub fn dimension(&self) -> Option<usize> {
        match self.output {
            OutputSpec::Embedding { dim } => usize::try_from(dim).ok(),
            OutputSpec::Detector { .. } => None,
        }
    }
}

/// Why a manifest is refused.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum ManifestError {
    #[error("`{0}` is not valid: {1}")]
    Field(&'static str, String),
    #[error("not a face-model manifest: {0}")]
    Parse(String),
    #[error("a manifest may be at most {MAX_MANIFEST_BYTES} bytes")]
    TooLarge,
}

fn bad<T>(field: &'static str, why: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError::Field(field, why.into()))
}

/// A model id is a folder name: 1 to 64 characters, a lowercase letter or digit first, then
/// lowercase letters, digits, `.`, `_` or `-`. So it is never `.`, `..`, hidden or a path.
pub fn valid_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    let first_ok = bytes.next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    first_ok && id.len() <= 64 && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

/// Whether `u` is an address Dalica shows or fetches: `https://`, a host, at most 500 characters,
/// no spaces or control characters.
pub fn valid_https(u: &str) -> bool {
    u.len() <= 500
        && u.strip_prefix("https://").is_some_and(|rest| rest.split('/').next().is_some_and(|host| !host.is_empty()))
        && !u.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn text(field: &'static str, s: &str, min: usize, max: usize) -> Result<(), ManifestError> {
    let n = s.chars().count();
    if n < min || n > max {
        return bad(field, format!("must be {min} to {max} characters"));
    }
    if s.chars().any(|c| c.is_control() && c != '\n') {
        return bad(field, "must not contain control characters");
    }
    Ok(())
}

fn url(field: &'static str, u: &Option<String>) -> Result<(), ManifestError> {
    match u {
        Some(u) if !valid_https(u) => bad(field, "must be an https:// address of at most 500 characters"),
        _ => Ok(()),
    }
}

fn ratio(field: &'static str, v: Option<f32>, lo: f32, hi: f32) -> Result<(), ManifestError> {
    match v {
        Some(v) if !(v.is_finite() && (lo..=hi).contains(&v)) => bad(field, format!("must be a number from {lo} to {hi}")),
        _ => Ok(()),
    }
}

/// Checks a manifest from any source.
pub fn validate(m: &ModelManifest) -> Result<(), ManifestError> {
    if !valid_id(&m.id) {
        return bad("id", "must be 1 to 64 characters: lowercase letters and digits, then also '.', '_' or '-'");
    }
    text("name", &m.name, 1, 120)?;
    text("version", &m.version, 1, 40)?;
    text("licence.name", &m.licence.name, 1, 80)?;
    text("licence.notice", &m.licence.notice, 1, 800)?;
    text("provenance", &m.provenance, 0, 800)?;
    url("licence.url", &m.licence.url)?;
    url("source", &m.source)?;
    if m.sha256.as_deref().is_some_and(|h| !crate::hash::is_sha256(h)) {
        return bad("sha256", "must be 64 lowercase hexadecimal digits");
    }
    if m.size_bytes.is_some_and(|s| s == 0 || s > MAX_MODEL_BYTES) {
        return bad("sizeBytes", format!("must be 1 to {MAX_MODEL_BYTES}"));
    }
    let i = &m.input;
    if !(16..=4096).contains(&i.width) || !(16..=4096).contains(&i.height) {
        return bad("input.width/height", "must be 16 to 4096");
    }
    if i.mean.iter().any(|v| !v.is_finite() || v.abs() > 1000.0) {
        return bad("input.mean", "must be finite numbers within ±1000");
    }
    if i.std.iter().any(|v| !v.is_finite() || v.abs() < 1e-3 || v.abs() > 1000.0) {
        return bad("input.std", "must be finite, not zero, within ±1000");
    }
    match &m.output {
        OutputSpec::Embedding { dim } => {
            if m.role != Role::Embedder {
                return bad("output", "an embedding output needs the embedder role");
            }
            if !(1..=4096).contains(dim) {
                return bad("output.dim", "must be 1 to 4096");
            }
        }
        OutputSpec::Detector { decoder } => {
            if m.role != Role::Detector {
                return bad("output", "a detector output needs the detector role");
            }
            if !crate::detect::DECODERS.contains(&decoder.as_str()) {
                return bad("output.decoder", format!("unknown decoder `{}`", decoder.chars().take(40).collect::<String>()));
            }
            if i.width != i.height || !i.width.is_multiple_of(32) || i.width < 64 {
                return bad("input.width/height", "a detector's input is a square whose side is a multiple of 32, at least 64");
            }
        }
    }
    ratio("thresholds.matchCosine", m.thresholds.match_cosine, -1.0, 1.0)?;
    ratio("thresholds.score", m.thresholds.score, 0.0, 1.0)?;
    ratio("thresholds.nmsIou", m.thresholds.nms_iou, 0.0, 1.0)?;
    if let Some(table) = m.calibration {
        if m.role != Role::Embedder {
            return bad("calibration", "only a recogniser has a calibration table");
        }
        Calibration::table(table).map_err(|e| ManifestError::Field("calibration", e.to_string()))?;
    }
    Ok(())
}

/// Reads a manifest (at most [`MAX_MANIFEST_BYTES`] of JSON) and validates it.
pub fn parse(bytes: &[u8]) -> Result<ModelManifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let m: ModelManifest = serde_json::from_slice(bytes).map_err(|e| ManifestError::Parse(e.to_string()))?;
    validate(&m)?;
    Ok(m)
}
