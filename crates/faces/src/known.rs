//! The models Dalica knows by their SHA-256, the built-in download list, and what to make of a model
//! file the user drops in (`faces.md` §3).
//!
//! **Known models.** The candidates `faces.md` §3.2 lists (a detector and two recognisers), each
//! with what is honestly known about it: the weights' licence, whether it allows commercial use,
//! what the model was trained on (or that this is not known), its size and its hash. A file the
//! user already has is recognised by its hash ([`lookup`], [`recognise`]), so its terms can be shown
//! before it is installed. Weights are never part of Dalica.
//!
//! **The built-in list.** Which of them Dalica offers to download is reserved decision 5, recorded
//! by the maintainer on 2026-10-18 (issue #818): the YuNet detector with the SFace (the default)
//! and AuraFace recognisers, LightCraft's set. [`OFFERED`] names them; each one's address is pinned
//! to a commit of the model's own repository ([`pinned`]) with its size and SHA-256, and its
//! licence is shown before anything is fetched. The same files are pinned for the corpus test in
//! `xtask/corpus/face-models.sha256`. A model whose weights are for non-commercial use is never
//! offered, whatever [`OFFERED`] says.
//!
//! **Calibration.** Each recogniser carries its table of the Suggestion and Grouping confidence
//! (`faces.md` §5), measured on the public-domain portraits of `corpus/public-faces/` by
//! `tests/corpus.rs` (M13.17), which fails when the table here is not the one it measures.
//!
//! **A dropped file.** Recognised by its hash (the known models, then the user's catalog), or else
//! read by its declared shapes: a float image input and one vector output make a recogniser whose
//! manifest is drafted with its assumptions stated in plain words (112 × 112 aligned RGB faces,
//! `(x − 127.5) / 127.5`, one vector per face); anything else is refused with the reason.
//!
//! Ported from LightCraft `crates/faces/src/known.rs` and `crates/faces/src/suggest.rs` @ f34c7a8a
//! (MIT OR Apache-2.0): the known models' facts, hashes and pinned addresses; the draft and its
//! assumptions. The [`OFFERED`] gate is Dalica's.

use crate::manifest::{Colour, Commercial, InputSpec, Licence, ModelManifest, OutputSpec, Resize, Role, Thresholds};
use crate::onnx::{Dim, FLOAT32, OnnxInfo, TensorInfo};

/// The candidate detector's id (YuNet 2023mar).
pub const YUNET_ID: &str = "yunet-2023mar";
/// The candidate recognisers' ids.
pub const SFACE_ID: &str = "sface-2021dec";
pub const AURAFACE_ID: &str = "auraface-v1";

pub const YUNET_SHA256: &str = "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";
pub const SFACE_SHA256: &str = "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79";
pub const AURAFACE_SHA256: &str = "a7933ea5330113b01c9b60351d8f4c33003f145d8470ac5f0e52ee2effe25c60";

/// The ids on the built-in download list: reserved decision 5 (`roadmap.md`, *Reserved
/// decisions*; recorded 2026-10-18): the detector, then the recognisers, the default first.
pub const OFFERED: &[&str] = &[YUNET_ID, SFACE_ID, AURAFACE_ID];

/// The recogniser offered first (reserved decision 5): smaller and faster than AuraFace.
pub const DEFAULT_RECOGNISER: &str = SFACE_ID;

fn licence(name: &str, commercial: Commercial, url: &str, notice: &str) -> Licence {
    Licence { name: name.into(), commercial, url: Some(url.into()), notice: notice.into() }
}

/// YuNet 2023mar (OpenCV Zoo): a small face detector.
pub fn yunet() -> ModelManifest {
    ModelManifest {
        id: YUNET_ID.into(),
        name: "YuNet (face detector)".into(),
        version: "2023mar".into(),
        role: Role::Detector,
        licence: licence(
            "MIT",
            Commercial::Yes,
            "https://github.com/opencv/opencv_zoo/blob/main/models/face_detection_yunet/LICENSE",
            "MIT licence, copyright Shiqi Yu. A separate download you choose to make.",
        ),
        source: Some("https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet".into()),
        sha256: Some(YUNET_SHA256.into()),
        size_bytes: Some(232_589),
        provenance: "Trained on WIDER FACE, whose terms are a separate question from the weights' MIT licence. A detector: it finds faces, it does not identify anyone.".into(),
        input: InputSpec { width: 640, height: 640, colour: Colour::Bgr, mean: [0.0; 3], std: [1.0; 3], resize: Resize::Letterbox },
        output: OutputSpec::Detector { decoder: "yunet-v2".into() },
        thresholds: Thresholds { match_cosine: None, score: Some(0.6), nms_iou: Some(0.3) },
        calibration: None,
    }
}

/// SFace 2021dec (OpenCV Zoo). Its `matchCosine` was measured by LightCraft (0.55: 97.5 % right
/// suggestions for 88 % of 755 named faces) and is the table's value at 85; the rest of the table
/// was measured on `corpus/public-faces/` (M13.17: 45 portraits of 10 people; the highest cosine
/// of two different people 0.330, the median of one person's pairs 0.678).
pub fn sface() -> ModelManifest {
    ModelManifest {
        id: SFACE_ID.into(),
        name: "SFace (face recogniser)".into(),
        version: "2021dec".into(),
        role: Role::Embedder,
        licence: licence(
            "Apache-2.0 (as labelled by OpenCV Zoo)",
            Commercial::Unknown,
            "https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface",
            "Labelled Apache-2.0, but what it was trained on is not documented, and questions about commercial use (opencv_zoo issues 313 and 318) are unanswered. Fine to try for yourself; do not redistribute it.",
        ),
        source: Some("https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface".into()),
        sha256: Some(SFACE_SHA256.into()),
        size_bytes: Some(38_696_353),
        provenance: "Undocumented. The original SFace repository mentions CASIA-WebFace, VGGFace2 and MS1MV2.".into(),
        input: InputSpec { width: 112, height: 112, colour: Colour::Rgb, mean: [0.0; 3], std: [1.0; 3], resize: Resize::Stretch },
        output: OutputSpec::Embedding { dim: 128 },
        thresholds: Thresholds { match_cosine: Some(0.55), ..Thresholds::default() },
        calibration: Some([0.34, 0.37, 0.40, 0.43, 0.46, 0.49, 0.52, 0.55, 0.615, 0.68]),
    }
}

/// AuraFace v1 `glintr100` (fal.ai). Its `matchCosine` was measured by LightCraft: 0.40 (96.6 %
/// right suggestions for 56 % of the same faces), the table's value at 85; the rest of the table
/// was measured on `corpus/public-faces/` (M13.17; the highest cosine of two different people
/// 0.303, the median of one person's pairs 0.630).
pub fn auraface() -> ModelManifest {
    ModelManifest {
        id: AURAFACE_ID.into(),
        name: "AuraFace v1 (face recogniser)".into(),
        version: "1".into(),
        role: Role::Embedder,
        licence: licence(
            "Apache-2.0",
            Commercial::Yes,
            "https://huggingface.co/fal/AuraFace-v1",
            "Apache-2.0. Its training data is described only as a commercial dataset, with no dataset named and no consent statement, and it covers some ethnicities less well. About 261 MB, and slow on a CPU.",
        ),
        source: Some("https://huggingface.co/fal/AuraFace-v1".into()),
        sha256: Some(AURAFACE_SHA256.into()),
        size_bytes: Some(260_694_151),
        provenance: "Undisclosed: \"a commercial dataset comprising face images from various sources\".".into(),
        input: InputSpec::default(),
        output: OutputSpec::Embedding { dim: 512 },
        thresholds: Thresholds { match_cosine: Some(0.40), ..Thresholds::default() },
        calibration: Some([0.31, 0.323, 0.336, 0.349, 0.361, 0.374, 0.387, 0.40, 0.515, 0.63]),
    }
}

/// Every model known by its hash.
pub fn all() -> Vec<ModelManifest> {
    vec![yunet(), sface(), auraface()]
}

/// The known model whose file has this SHA-256 (lowercase hex).
pub fn lookup(sha256: &str) -> Option<ModelManifest> {
    all().into_iter().find(|m| m.sha256.as_deref() == Some(sha256))
}

/// Where a model is downloaded from: an `https://` address pinned to a commit of the model's own
/// repository, and the size and SHA-256 the file must have before anything uses it.
#[derive(Clone, Debug, PartialEq)]
pub struct Download {
    pub id: String,
    pub url: String,
    /// The file's name in the models folder (independent of the address).
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

impl Download {
    /// The site the file comes from, as the user is told ("github.com").
    pub fn host(&self) -> &str {
        self.url.strip_prefix("https://").and_then(|r| r.split('/').next()).unwrap_or("")
    }
}

/// The candidates' pinned addresses: (id, address, file name).
const PINNED: &[(&str, &str, &str)] = &[
    (
        YUNET_ID,
        "https://github.com/opencv/opencv_zoo/raw/25f423d0e04c31a17254620e58febd7386da523b/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
        "face_detection_yunet_2023mar.onnx",
    ),
    (
        SFACE_ID,
        "https://github.com/opencv/opencv_zoo/raw/25f423d0e04c31a17254620e58febd7386da523b/models/face_recognition_sface/face_recognition_sface_2021dec.onnx",
        "face_recognition_sface_2021dec.onnx",
    ),
    (AURAFACE_ID, "https://huggingface.co/fal/AuraFace-v1/resolve/af6d057c9b0ec4071d4c49c80e3539258798b609/glintr100.onnx", "glintr100.onnx"),
];

/// A known model's pinned address, whether or not it is offered.
pub fn pinned(id: &str) -> Option<Download> {
    let (_, url, file_name) = PINNED.iter().find(|(i, _, _)| *i == id)?;
    let m = all().into_iter().find(|m| m.id == id)?;
    Some(Download { id: m.id, url: (*url).into(), file_name: (*file_name).into(), size_bytes: m.size_bytes?, sha256: m.sha256? })
}

/// How to download the model with this id, if it is on the built-in list.
pub fn download(id: &str) -> Option<Download> {
    if !OFFERED.contains(&id) {
        return None;
    }
    let m = all().into_iter().find(|m| m.id == id)?;
    if m.licence.commercial == Commercial::No {
        return None;
    }
    pinned(id)
}

/// The built-in download list, in [`OFFERED`]'s order, each with its manifest.
pub fn builtin() -> Vec<(ModelManifest, Download)> {
    OFFERED.iter().filter_map(|id| Some((all().into_iter().find(|m| m.id == *id)?, download(id)?))).collect()
}

/// What to make of a model file the user dropped in.
#[derive(Clone, Debug, PartialEq)]
pub enum Recognition {
    /// A model known by its SHA-256 (built in or from the user's catalog): its manifest as it is.
    Known(ModelManifest),
    /// An unknown model shaped like a recogniser: a drafted manifest and the assumptions it
    /// rests on, in plain words, for the user to read before installing.
    Assumed { manifest: ModelManifest, assumptions: Vec<String> },
    /// Not usable, and why.
    Unsupported(String),
}

fn fixed(d: &Dim) -> Option<u64> {
    match d {
        Dim::Fixed(v) => Some(*v),
        Dim::Dynamic(_) => None,
    }
}

/// An id from the file name and the start of the hash: lowercase letters, digits and `-`.
fn make_id(file_name: &str, sha256: &str) -> String {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let clean: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let clean: String = clean.trim_matches('-').chars().take(40).collect();
    let short: String = sha256.chars().filter(char::is_ascii_hexdigit).take(8).collect::<String>().to_ascii_lowercase();
    match (clean.is_empty(), short.is_empty()) {
        (true, true) => "custom".into(),
        (true, false) => format!("custom-{short}"),
        (false, true) => format!("custom-{clean}"),
        (false, false) => format!("custom-{clean}-{short}"),
    }
}

fn display_name(file_name: &str) -> String {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let name: String = stem.chars().filter(|c| !c.is_control()).take(60).collect();
    if name.trim().is_empty() { "Your face model".into() } else { format!("{} (your model)", name.trim()) }
}

/// The input of a recogniser: float32 `[batch, 3, height, width]`; a dynamic size is taken as 112.
fn face_input(t: &TensorInfo) -> Option<(u32, u32)> {
    if t.elem_type != FLOAT32 || t.shape.len() != 4 || t.shape.get(1).and_then(fixed) != Some(3) {
        return None;
    }
    let size = |d: Option<&Dim>| match d.and_then(fixed) {
        Some(v) if (16..=1024).contains(&v) => u32::try_from(v).ok(),
        Some(_) => None,
        None => Some(112),
    };
    Some((size(t.shape.get(3))?, size(t.shape.get(2))?))
}

/// What to do with a probed model file: `sha256` and `size` describe it, `file_name` names it, and
/// `extra` holds the manifests of the user's catalog, also recognised by hash.
pub fn recognise(info: &OnnxInfo, sha256: &str, size: u64, file_name: &str, extra: &[ModelManifest]) -> Recognition {
    if let Some(m) = lookup(sha256).or_else(|| extra.iter().find(|m| m.sha256.as_deref() == Some(sha256)).cloned()) {
        return Recognition::Known(m);
    }
    let [input] = info.inputs.as_slice() else {
        return Recognition::Unsupported(format!("the model has {} inputs; a face model takes one picture", info.inputs.len()));
    };
    let Some((width, height)) = face_input(input) else {
        return Recognition::Unsupported("its input is not a float picture of shape [batch, 3, height, width]".into());
    };
    let [output] = info.outputs.as_slice() else {
        return Recognition::Unsupported(
            "it has several outputs, like a face detector; only a known detector or a model giving one vector per face can be added".into(),
        );
    };
    if output.elem_type != FLOAT32 || output.shape.len() != 2 {
        return Recognition::Unsupported("its output is not one vector per face (shape [batch, dimension])".into());
    }
    let dim = match output.shape.get(1).and_then(fixed).map(u32::try_from) {
        Some(Ok(d)) if (8..=4096).contains(&d) => d,
        _ => return Recognition::Unsupported("its output size is not a fixed number from 8 to 4096".into()),
    };
    let manifest = ModelManifest {
        id: make_id(file_name, sha256),
        name: display_name(file_name),
        version: "1".into(),
        role: Role::Embedder,
        licence: Licence {
            name: "Unknown".into(),
            commercial: Commercial::Unknown,
            url: None,
            notice: "Dalica does not know where this model came from or what its licence allows. Use it only if its licence allows what you are doing, and do not redistribute it.".into(),
        },
        source: None,
        sha256: Some(sha256.to_string()).filter(|h| crate::hash::is_sha256(h)),
        size_bytes: Some(size).filter(|s| *s > 0 && *s <= crate::limits::MAX_MODEL_BYTES),
        provenance: "Unknown.".into(),
        input: InputSpec { width, height, ..InputSpec::default() },
        output: OutputSpec::Embedding { dim },
        thresholds: Thresholds::default(),
        calibration: None,
    };
    let assumptions = vec![
        format!("Faces are aligned crops of {width} × {height} pixels."),
        "Pixels are RGB, normalised as (x − 127.5) / 127.5, the usual convention of face recognisers.".into(),
        format!("Each face becomes one vector of {dim} numbers; two faces match when their cosine similarity is high."),
    ];
    Recognition::Assumed { manifest, assumptions }
}
