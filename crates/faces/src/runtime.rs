//! A face model ready to run: read within the limits, its operators checked, its working memory
//! checked for its declared input, and every run guarded (`faces.md` §2).
//!
//! A [`Model`] is read from bytes, a seekable source or a path; a source longer than
//! [`MAX_MODEL_BYTES`] is refused before a byte of it is read. Loading reads the graph
//! ([`crate::graph::load`]), checks every operator and weight ([`Net::new`]) and, when the declared
//! input has no dynamic size, every shape and the working memory for it ([`Model::check_input`]), so
//! a model that cannot run is refused at load, never half-way through a scan. A model is shared
//! between threads by reference (`&Model` or an `Arc`): each run keeps its values on its own stack,
//! so two threads running one model give the same outputs. A panic inside a run (which would be a
//! bug: the kernels check every slice first) is caught and returned as an error.
//!
//! Ported from LightCraft `crates/faces/src/runtime.rs` @ f34c7a8a (MIT OR Apache-2.0): the file
//! read, the load checks and the guard. Its recogniser (manifests, preprocessing, unit embeddings,
//! the self-test) is `embed.rs` and `selftest.rs` (M13.10).

use std::io::{Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use crate::graph;
use crate::limits::MAX_MODEL_BYTES;
use crate::net::{Net, NetError, Tensor};
use crate::onnx::LoadError;

/// Why a model cannot be loaded or run.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// The file is not a model within the limits, or uses something refused by name.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// The graph's operators, shapes or values.
    #[error(transparent)]
    Net(#[from] NetError),
    /// The interpreter gave up (a panic, caught).
    #[error("the face model runtime gave up while {0}")]
    Panicked(&'static str),
}

/// A loaded, checked model.
#[derive(Debug)]
pub struct Model {
    net: Net,
}

fn guarded<T>(what: &'static str, f: impl FnOnce() -> Result<T, ModelError>) -> Result<T, ModelError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(ModelError::Panicked(what)))
}

impl Model {
    /// A model from the whole file in memory.
    pub fn from_bytes(bytes: &[u8]) -> Result<Model, ModelError> {
        guarded("loading the model", || {
            let net = Net::new(graph::load(bytes)?)?;
            let model = Model { net };
            if let Some(shape) = model.fixed_input() {
                model.net.check_input(&shape)?;
            }
            Ok(model)
        })
    }

    /// A model from a seekable source, refused unread when it is longer than [`MAX_MODEL_BYTES`].
    pub fn read<R: Read + Seek>(source: &mut R) -> Result<Model, ModelError> {
        let length = source.seek(SeekFrom::End(0)).map_err(LoadError::from)?;
        if length > MAX_MODEL_BYTES {
            return Err(LoadError::Malformed("the model exceeds 512 MiB".into()).into());
        }
        if length == 0 {
            return Err(LoadError::Malformed("the file is empty".into()).into());
        }
        source.seek(SeekFrom::Start(0)).map_err(LoadError::from)?;
        let size = usize::try_from(length).map_err(|_| LoadError::Malformed("the model is too large for this system".into()))?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(|_| LoadError::Malformed("not enough memory for the model".into()))?;
        source.take(MAX_MODEL_BYTES + 1).read_to_end(&mut bytes).map_err(LoadError::from)?;
        Model::from_bytes(&bytes)
    }

    /// A model from a file.
    pub fn open(path: &Path) -> Result<Model, ModelError> {
        let mut file = std::fs::File::open(path).map_err(LoadError::from)?;
        Model::read(&mut file)
    }

    /// The checked network.
    pub fn net(&self) -> &Net {
        &self.net
    }

    /// The input shape the model declares (0 for a dynamic size).
    pub fn input_shape(&self) -> &[usize] {
        self.net.input_shape().unwrap_or(&[])
    }

    /// The declared input shape when no size in it is dynamic.
    fn fixed_input(&self) -> Option<Vec<usize>> {
        let shape = self.input_shape();
        (!shape.is_empty() && !shape.contains(&0)).then(|| shape.to_vec())
    }

    /// Checks every shape and the working memory for an input of `shape` without running anything;
    /// returns the outputs' shapes. [`Model::run`] checks the same first.
    pub fn check_input(&self, shape: &[usize]) -> Result<Vec<Vec<usize>>, ModelError> {
        guarded("checking an input", || Ok(self.net.check_input(shape)?))
    }

    /// Runs the model on `input`; returns each output by name.
    pub fn run(&self, input: Tensor) -> Result<Vec<(String, Tensor)>, ModelError> {
        guarded("running the model", || Ok(self.net.run(input)?))
    }
}
