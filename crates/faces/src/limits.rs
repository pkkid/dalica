//! The limits a face model is held to (`faces.md` §2, `security.md` §4.2). Every one is checked
//! before a model runs: the file and its weights when it is read ([`crate::graph::load`]), the shape
//! of every value and the working memory when it is checked for an input
//! ([`crate::runtime::Model::check_input`]), and finiteness of every weight at load and of every
//! activation as it is made.

/// The largest model file read (512 MiB).
pub const MAX_MODEL_BYTES: u64 = 512 * 1024 * 1024;

/// Elements across all weight tensors of a model (64 Mi; 256 MiB as float32).
pub const MAX_WEIGHT_ELEMENTS: usize = 64 * 1024 * 1024;

/// Elements of any one tensor the interpreter allocates (64 Mi).
pub const MAX_TENSOR_ELEMENTS: usize = 64 * 1024 * 1024;

/// The largest rank of a tensor, a weight or a declared input or output.
pub const MAX_RANK: usize = 8;

/// Live tensors, constant copies and convolution scratch at any one step, in bytes (512 MiB).
pub const MAX_WORKING_BYTES: usize = 512 * 1024 * 1024;

/// [`MAX_WORKING_BYTES`] as float32 elements.
pub const MAX_WORKING_ELEMENTS: usize = MAX_WORKING_BYTES / 4;

/// Nodes in a graph.
pub const MAX_NODES: usize = 20_000;

/// Weight tensors in a graph.
pub const MAX_WEIGHT_TENSORS: usize = 20_000;

/// Graph outputs.
pub const MAX_OUTPUTS: usize = 64;

/// Inputs, outputs or attributes of one node.
pub const MAX_NODE_ARITY: usize = 64;

/// Bytes of a name or any other string in the file.
pub const MAX_STRING: usize = 4096;

/// Values of one attribute list or packed number list.
pub const MAX_LIST: usize = 1_000_000;

/// Protobuf fields read from one file, counted across every message.
pub const MAX_FIELDS: u32 = 5_000_000;

/// The default-domain opsets the interpreter follows.
pub const OPSETS: std::ops::RangeInclusive<u64> = 10..=17;

/// A convolution's stride or padding.
pub const MAX_STRIDE_OR_PAD: usize = 65_536;

/// The largest whole-number factor of a nearest-neighbour `Resize`.
pub const MAX_RESIZE_FACTOR: usize = 16;
