//! The recogniser (`faces.md` §2): an aligned face in, one unit-length vector out.
//!
//! An [`Embedder`] is a loaded model and the manifest describing it. Loading checks that the model's
//! input is the manifest's `[1, 3, height, width]` and that it gives one vector of the manifest's
//! dimension; embedding turns the crop into the model's input (colour order, `(x − mean) / std`),
//! runs it, checks every number is finite and scales the vector to unit length with a float64 norm,
//! so large but finite outputs cannot overflow. Which model is used is the manifest's business
//! (reserved decision 5).
//!
//! Ported from LightCraft `crates/faces/src/runtime.rs` @ f34c7a8a (MIT OR Apache-2.0): its
//! `Embedder` (load checks, preprocessing, the unit vector), without the accelerator and batching.

use std::path::Path;

use crate::align::{Rgb, face_crop};
use crate::manifest::{self, Colour, ModelManifest, OutputSpec, Role};
use crate::net::Tensor;
use crate::runtime::Model;

/// Why a face could not be embedded.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The model or its manifest is not a usable recogniser.
    #[error("the face recogniser is not usable: {0}")]
    Model(String),
    /// The model failed on this face.
    #[error("the face recogniser failed: {0}")]
    Run(String),
    /// The picture.
    #[error("{0}")]
    Input(String),
}

/// A loaded recogniser.
#[derive(Debug)]
pub struct Embedder {
    model: Model,
    manifest: ModelManifest,
    width: usize,
    height: usize,
    dim: usize,
}

impl Embedder {
    /// A recogniser from a loaded model and the manifest describing it.
    pub fn new(model: Model, manifest: &ModelManifest) -> Result<Embedder, EmbedError> {
        manifest::validate(manifest).map_err(|e| EmbedError::Model(e.to_string()))?;
        let (Role::Embedder, OutputSpec::Embedding { dim }) = (manifest.role, &manifest.output) else {
            return Err(EmbedError::Model("the manifest does not describe a recogniser".into()));
        };
        let size = |v: u32| usize::try_from(v).map_err(|_| EmbedError::Model("the input size".into()));
        let (width, height, dim) = (size(manifest.input.width)?, size(manifest.input.height)?, size(*dim)?);
        let want = [1, 3, height, width];
        let declared = model.input_shape();
        if declared.len() != 4 || declared.iter().zip(want).any(|(&d, w)| d != 0 && d != w) {
            return Err(EmbedError::Model(format!("the model's input {declared:?} is not the manifest's [1, 3, {height}, {width}]")));
        }
        let shapes = model.check_input(&want).map_err(|e| EmbedError::Model(e.to_string()))?;
        match shapes.as_slice() {
            [shape] if shape.first() == Some(&1) && shape.iter().try_fold(1usize, |n, d| n.checked_mul(*d)) == Some(dim) => {}
            [shape] => {
                return Err(EmbedError::Model(format!("the model gives {shape:?} per face, not the manifest's {dim} numbers")));
            }
            _ => return Err(EmbedError::Model(format!("the model has {} outputs; a recogniser has one", shapes.len()))),
        }
        Ok(Embedder { model, manifest: manifest.clone(), width, height, dim })
    }

    /// A recogniser from a model file.
    pub fn open(path: &Path, manifest: &ModelManifest) -> Result<Embedder, EmbedError> {
        let model = Model::open(path).map_err(|e| EmbedError::Model(e.to_string()))?;
        Embedder::new(model, manifest)
    }

    /// The size of the aligned face the model reads (width, height).
    pub fn input_size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// The embedding dimension.
    pub fn dimension(&self) -> usize {
        self.dim
    }

    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    /// The face as a unit-length vector. `crop` is an aligned face of exactly
    /// [`Embedder::input_size`], 8-bit RGB.
    pub fn embed(&self, crop: &[u8]) -> Result<Vec<f32>, EmbedError> {
        let input = self.input(crop)?;
        let mut outputs = self.model.run(input).map_err(|e| EmbedError::Run(e.to_string()))?;
        let out = match (outputs.len(), outputs.pop()) {
            (1, Some((_, t))) => t.data,
            _ => return Err(EmbedError::Run("the model must give one vector".into())),
        };
        unit(out, self.dim)
    }

    /// A stored face of a photo (`rect` and `landmarks` normalised, as [`crate::align::face_crop`]
    /// takes them) aligned and embedded.
    pub fn embed_face(&self, img: &Rgb, rect: [f32; 4], landmarks: Option<&[(f32, f32); 5]>) -> Result<Vec<f32>, EmbedError> {
        let crop = face_crop(img, rect, landmarks, self.width, self.height)
            .ok_or_else(|| EmbedError::Input("the face could not be cut out of the picture".into()))?;
        self.embed(&crop)
    }

    /// The model's input for an aligned face.
    fn input(&self, crop: &[u8]) -> Result<Tensor, EmbedError> {
        let plane = self.width * self.height;
        if crop.len() != plane * 3 {
            return Err(EmbedError::Input("the face picture is not the size the model reads".into()));
        }
        let spec = &self.manifest.input;
        let order: [usize; 3] = match spec.colour {
            Colour::Rgb => [0, 1, 2],
            Colour::Bgr => [2, 1, 0],
        };
        let mut data = vec![0.0f32; 3 * plane];
        for (c, &src) in order.iter().enumerate() {
            let (m, s) = (spec.mean.get(c).copied().unwrap_or(0.0), spec.std.get(c).copied().unwrap_or(1.0));
            let out = data.get_mut(c * plane..(c + 1) * plane).unwrap_or(&mut []);
            for (o, px) in out.iter_mut().zip(crop.as_chunks::<3>().0) {
                *o = (f32::from(px.get(src).copied().unwrap_or(0)) - m) / s;
            }
        }
        Tensor::new(vec![1, 3, self.height, self.width], data).map_err(|e| EmbedError::Input(e.to_string()))
    }
}

/// `out` checked (`dim` finite numbers, not all zero) and scaled to unit length by its float64 norm.
pub fn unit(out: Vec<f32>, dim: usize) -> Result<Vec<f32>, EmbedError> {
    if out.len() != dim {
        return Err(EmbedError::Run(format!("the model gave {} numbers per face, not {dim}", out.len())));
    }
    if out.iter().any(|v| !v.is_finite()) {
        return Err(EmbedError::Run("the model gave numbers that are not finite".into()));
    }
    let norm = out.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>().sqrt();
    if !norm.is_finite() || norm < 1e-12 {
        return Err(EmbedError::Run("the model gave an all-zero face".into()));
    }
    Ok(out.into_iter().map(|v| (f64::from(v) / norm) as f32).collect())
}
