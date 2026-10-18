//! The face detector (`faces.md` §2): a photo in, faces out, each a box, a score and five
//! landmarks normalised on the upright photo.
//!
//! The photo is letterboxed (aspect ratio kept, padded at the right and bottom) into the model's
//! square input (640 px for the candidate detector, `faces.md` §3.2; the side comes from the
//! manifest), the model gives a score, a box and five landmarks for every cell of three grids, and
//! the cells above the score threshold are decoded, de-duplicated by non-maximum suppression and
//! mapped back to the photo as fractions of its width and height (0 … 1, y down). Boxes are
//! clipped to the photo; landmarks may lie slightly outside it. Which decoder a model needs is
//! named by its manifest ([`DECODERS`]); the model itself is whatever the manifest describes, so
//! the choice of model stays data (reserved decision 5).
//!
//! The pixels come from the caller as plain 8-bit RGB ([`Rgb`]); this module has no image types.
//!
//! Ported from LightCraft `crates/faces/src/yunet.rs` @ f34c7a8a (MIT OR Apache-2.0).

mod decode;
mod letterbox;

use crate::align::Rgb;
use crate::manifest::{self, InputSpec, ModelManifest, OutputSpec, Role};
use crate::net::Tensor;
use crate::runtime::Model;

pub use decode::iou;

/// The detector output decoders built into Dalica, as a manifest names them.
pub const DECODERS: &[&str] = &["yunet-v2"];

/// The largest photo side searched.
pub const MAX_SIDE: usize = 65_536;

/// The most faces kept on one photo (`data-model.md` §3).
pub const MAX_FACES: usize = 256;

/// Why detection failed.
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    /// The model or its manifest is not a usable detector.
    #[error("the detector is not usable: {0}")]
    Model(String),
    /// The picture or the options.
    #[error("{0}")]
    Input(String),
}

/// How strict detection is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The lowest detection score kept (0 … 1).
    pub score: f32,
    /// Boxes overlapping a better one by more than this (intersection over union) are dropped.
    pub nms_iou: f32,
    /// The most faces returned.
    pub max_faces: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { score: 0.6, nms_iou: 0.3, max_faces: MAX_FACES }
    }
}

impl Options {
    /// The defaults with the thresholds the manifest recommends.
    pub fn from_manifest(m: &ModelManifest) -> Options {
        let d = Options::default();
        Options { score: m.thresholds.score.unwrap_or(d.score), nms_iou: m.thresholds.nms_iou.unwrap_or(d.nms_iou), ..d }
    }

    /// Non-finite or out-of-range values replaced by the defaults or clamped.
    fn sane(&self) -> Options {
        let d = Options::default();
        Options {
            score: if self.score.is_finite() { self.score.clamp(0.0, 1.0) } else { d.score },
            nms_iou: if self.nms_iou.is_finite() { self.nms_iou.clamp(0.0, 1.0) } else { d.nms_iou },
            max_faces: self.max_faces.min(MAX_FACES),
        }
    }
}

/// A detected face, normalised on the photo (0 … 1, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    /// `[x0, y0, x1, y1]`, inside the photo.
    pub rect: [f32; 4],
    /// The detection score (0 … 1).
    pub score: f32,
    /// The eye on the image's left, the other eye, the nose tip, the mouth corner on the image's
    /// left, the other mouth corner; the order of [`crate::align::TEMPLATE_112`].
    pub landmarks: [(f32, f32); 5],
}

/// A loaded detector.
#[derive(Debug)]
pub struct Detector {
    model: Model,
    input: InputSpec,
    side: usize,
    options: Options,
}

impl Detector {
    /// A detector from a loaded model and the manifest describing it: the manifest must be a
    /// detector's with a known decoder, the model's input must match its square input, and every
    /// output the decoder reads must be there with the size it expects.
    pub fn new(model: Model, manifest: &ModelManifest) -> Result<Detector, DetectError> {
        manifest::validate(manifest).map_err(|e| DetectError::Model(e.to_string()))?;
        if manifest.role != Role::Detector || !matches!(&manifest.output, OutputSpec::Detector { decoder } if decoder == "yunet-v2") {
            return Err(DetectError::Model("the manifest does not describe a detector".into()));
        }
        let side = usize::try_from(manifest.input.width).map_err(|_| DetectError::Model("the input size".into()))?;
        let want = [1, 3, side, side];
        let declared = model.input_shape();
        if declared.len() != 4 || declared.iter().zip(want).any(|(&d, w)| d != 0 && d != w) {
            return Err(DetectError::Model(format!("the model's input {declared:?} is not the manifest's [1, 3, {side}, {side}]")));
        }
        let shapes = model.check_input(&want).map_err(|e| DetectError::Model(e.to_string()))?;
        let names = &model.net().graph().outputs;
        for stride in decode::STRIDES {
            let cells = (side / stride) * (side / stride);
            for (kind, components) in decode::PARTS {
                let name = format!("{kind}_{stride}");
                let at = names.iter().position(|n| *n == name).ok_or_else(|| DetectError::Model(format!("the model has no output `{name}`")))?;
                let count = shapes.get(at).and_then(|s| s.iter().try_fold(1usize, |n, d| n.checked_mul(*d)));
                if count != Some(cells * components) {
                    return Err(DetectError::Model(format!("the output `{name}` has the wrong shape")));
                }
            }
        }
        Ok(Detector { model, input: manifest.input, side, options: Options::from_manifest(manifest) })
    }

    /// The side of the model's square input.
    pub fn input_side(&self) -> usize {
        self.side
    }

    /// The options the manifest recommends.
    pub fn options(&self) -> Options {
        self.options
    }

    /// Finds faces in an upright photo, best first.
    pub fn detect(&self, img: &Rgb, opts: &Options) -> Result<Vec<Face>, DetectError> {
        let (width, height) = (img.width, img.height);
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(DetectError::Input(format!("a {width} × {height} picture cannot be searched")));
        }
        if !img.valid() {
            return Err(DetectError::Input("the pixel data does not match the picture's size".into()));
        }
        let opts = opts.sane();
        let side = self.side as f32;
        let scale = (side / width as f32).min(side / height as f32);
        let planes = letterbox::letterbox(img, self.side, scale, &self.input)
            .ok_or_else(|| DetectError::Input("the picture could not be fitted to the detector".into()))?;
        let input = Tensor::new(vec![1, 3, self.side, self.side], planes).map_err(|e| DetectError::Input(e.to_string()))?;
        let outputs = self.model.run(input).map_err(|e| DetectError::Model(e.to_string()))?;
        let frame = decode::Frame { side: self.side, scale, width: width as f32, height: height as f32 };
        let found = decode::candidates(&outputs, &frame, &opts)?;
        Ok(decode::suppress(found, &opts))
    }
}

/// A fixed photo-like test picture (smooth gradients plus texture), `width * height * 3` bytes;
/// each side at most 4096.
pub fn probe_picture(width: usize, height: usize) -> Vec<u8> {
    let n = width.min(4096) * height.min(4096) * 3;
    (0..n).map(|i| ((i % 640) as f32 * 0.3 + (i.wrapping_mul(7919) % 97) as f32 + (i / 640 % 480) as f32 * 0.2) % 255.0).map(|v| v as u8).collect()
}
