//! A streaming probe of an ONNX file: the graph's inputs and outputs (name, element type, shape),
//! without running or even fully loading the model, and the bounded protobuf walker
//! [`crate::graph`] reads whole models with.
//!
//! ONNX is protobuf. The probe reads only the few fields it needs (the producer, the opset, and the
//! graph's input, output and initializer names) and seeks past everything else, so a 260 MB model
//! costs a few kilobytes and a few milliseconds. The file is hostile input: every length is checked
//! against its parent message and the file size, the number of fields read is capped, strings are
//! bounded, and nothing indexes or unwraps.
//!
//! Ported from LightCraft `crates/faces/src/onnx.rs` @ f34c7a8a (MIT OR Apache-2.0): the error
//! type split into malformed and unsupported files, and the limits taken from [`crate::limits`].

use std::collections::HashSet;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use crate::limits::{MAX_FIELDS, MAX_MODEL_BYTES, MAX_STRING};

/// Dimensions of a declared input or output the probe reads (the loader allows [`crate::limits::MAX_RANK`]).
const MAX_DIMS: usize = 16;
/// Real graph inputs and outputs.
const MAX_TENSORS: usize = 64;
/// Older exporters list every weight as a graph input too; that many are read before the weights
/// are filtered out.
const MAX_LISTED_INPUTS: usize = 200_000;
const MAX_INITIALIZERS: usize = 1_000_000;

/// ONNX's element type for float32.
pub const FLOAT32: u32 = 1;
/// ONNX's element type for int64.
pub const INT64: u32 = 7;

/// The name of an ONNX element type (`TensorProto.DataType`), for an error that says which one.
pub fn element_type_name(t: u64) -> &'static str {
    match t {
        0 => "undefined",
        1 => "float32",
        2 => "uint8",
        3 => "int8",
        4 => "uint16",
        5 => "int16",
        6 => "int32",
        7 => "int64",
        8 => "string",
        9 => "bool",
        10 => "float16",
        11 => "float64",
        12 => "uint32",
        13 => "uint64",
        14 => "complex64",
        15 => "complex128",
        16 => "bfloat16",
        17..=20 => "float8",
        21 => "uint4",
        22 => "int4",
        _ => "an unknown element type",
    }
}

/// One dimension of a declared input or output.
#[derive(Clone, Debug, PartialEq)]
pub enum Dim {
    Fixed(u64),
    /// A named or unknown size (typically the batch).
    Dynamic(String),
}

/// A declared graph input or output.
#[derive(Clone, Debug, PartialEq)]
pub struct TensorInfo {
    pub name: String,
    /// ONNX element type ([`FLOAT32`] = 1).
    pub elem_type: u32,
    pub shape: Vec<Dim>,
}

/// What [`probe_reader`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnnxInfo {
    pub producer: String,
    /// The default-domain opset version.
    pub opset: Option<u64>,
    /// Real inputs only (weights that older exporters also list as inputs are left out).
    pub inputs: Vec<TensorInfo>,
    pub outputs: Vec<TensorInfo>,
}

/// Why a model file cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The bytes are not a well-formed model within the limits.
    #[error("not a usable ONNX model: {0}")]
    Malformed(String),
    /// A well-formed model that uses something Dalica refuses by name.
    #[error("this model uses something Dalica does not support: {0}")]
    Unsupported(String),
    #[error("could not read the model: {0}")]
    Io(#[from] io::Error),
}

pub(crate) type Result<T> = std::result::Result<T, LoadError>;

pub(crate) fn malformed<T>(why: impl Into<String>) -> Result<T> {
    Err(LoadError::Malformed(why.into()))
}

pub(crate) fn unsupported<T>(why: impl Into<String>) -> Result<T> {
    Err(LoadError::Unsupported(why.into()))
}

/// One protobuf field's value.
pub(crate) enum Wire {
    Varint(u64),
    /// A length-delimited field: its bytes are `start..end` and the reader sits at `start`.
    Len {
        start: u64,
        end: u64,
    },
    Fixed32(u32),
    Fixed64,
}

/// A bounded protobuf reader over a seekable source of `total` bytes.
pub(crate) struct Walker<'a, R> {
    pub(crate) r: &'a mut R,
    pub(crate) total: u64,
    pub(crate) fields: u32,
}

impl<R: Read + Seek> Walker<'_, R> {
    pub(crate) fn pos(&mut self) -> Result<u64> {
        Ok(self.r.stream_position()?)
    }

    pub(crate) fn seek_to(&mut self, at: u64) -> Result<()> {
        self.r.seek(SeekFrom::Start(at))?;
        Ok(())
    }

    fn exact<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut b = [0u8; N];
        match self.r.read_exact(&mut b) {
            Ok(()) => Ok(b),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => malformed("the file ends inside a field"),
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn varint(&mut self) -> Result<u64> {
        let mut v = 0u64;
        for shift in (0..70u32).step_by(7) {
            let [b] = self.exact::<1>()?;
            if shift == 63 && b > 1 {
                return malformed("a number overflows 64 bits");
            }
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        malformed("a number is too long")
    }

    /// Checks that `n` more bytes from here stay inside the message ending at `end` and the file.
    fn room(&mut self, n: u64, end: u64) -> Result<()> {
        let at = self.pos()?.checked_add(n).ok_or_else(|| LoadError::Malformed("an offset overflows".into()))?;
        if at > end || at > self.total {
            return malformed("a field runs past its message");
        }
        Ok(())
    }

    /// The next field of the message that ends at `end`, or `None` at its end.
    pub(crate) fn next(&mut self, end: u64) -> Result<Option<(u32, Wire)>> {
        if self.pos()? >= end {
            return Ok(None);
        }
        self.fields = self.fields.saturating_add(1);
        if self.fields > MAX_FIELDS {
            return malformed("the file has too many fields");
        }
        let tag = self.varint()?;
        let number = u32::try_from(tag >> 3).map_err(|_| LoadError::Malformed("a field number is too large".into()))?;
        if number == 0 || number > 0x1fff_ffff {
            return malformed("an invalid field number");
        }
        let wire = match tag & 7 {
            0 => Wire::Varint(self.varint()?),
            1 => {
                self.room(8, end)?;
                self.exact::<8>()?;
                Wire::Fixed64
            }
            5 => {
                self.room(4, end)?;
                Wire::Fixed32(u32::from_le_bytes(self.exact::<4>()?))
            }
            2 => {
                let len = self.varint()?;
                let start = self.pos()?;
                let stop = start.checked_add(len).ok_or_else(|| LoadError::Malformed("an offset overflows".into()))?;
                if stop > end || stop > self.total {
                    return malformed("a field runs past its message");
                }
                Wire::Len { start, end: stop }
            }
            _ => return malformed("an unsupported protobuf field type"),
        };
        if self.pos()? > end {
            return malformed("a field runs past its message");
        }
        Ok(Some((number, wire)))
    }

    /// A bounded UTF-8 string occupying `start..end`; graph names are never silently truncated.
    pub(crate) fn string(&mut self, start: u64, end: u64) -> Result<String> {
        if end.saturating_sub(start) > MAX_STRING as u64 {
            return malformed("a string exceeds 4096 bytes");
        }
        let buf = self.bytes(start, end, MAX_STRING as u64)?;
        String::from_utf8(buf).map_err(|_| LoadError::Malformed("a string is not UTF-8".into()))
    }

    /// The bytes of `start..end`, which must be at most `max` long; the reader must sit at `start`.
    pub(crate) fn bytes(&mut self, start: u64, end: u64, max: u64) -> Result<Vec<u8>> {
        let len = end.saturating_sub(start);
        if len > max {
            return malformed("a field is larger than allowed");
        }
        let mut buf = Vec::new();
        self.r.by_ref().take(len).read_to_end(&mut buf)?;
        if buf.len() as u64 != len {
            return malformed("the file ends inside a field");
        }
        Ok(buf)
    }
}

fn dim<R: Read + Seek>(w: &mut Walker<R>, end: u64) -> Result<Option<Dim>> {
    let mut out = None;
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (1, Wire::Varint(v)) => out = Some(Dim::Fixed(v)),
            (2, Wire::Len { start, end: e }) => {
                out = Some(Dim::Dynamic(w.string(start, e)?));
                w.seek_to(e)?;
            }
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(out)
}

fn shape<R: Read + Seek>(w: &mut Walker<R>, end: u64) -> Result<Vec<Dim>> {
    let mut dims = Vec::new();
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (1, Wire::Len { end: e, .. }) => {
                if dims.len() >= MAX_DIMS {
                    return malformed("a declared tensor has too many dimensions");
                }
                dims.push(dim(w, e)?.unwrap_or(Dim::Dynamic(String::new())));
                w.seek_to(e)?;
            }
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(dims)
}

fn tensor_type<R: Read + Seek>(w: &mut Walker<R>, end: u64, info: &mut TensorInfo) -> Result<()> {
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (1, Wire::Varint(v)) => info.elem_type = u32::try_from(v).unwrap_or(u32::MAX),
            (2, Wire::Len { end: e, .. }) => {
                info.shape = shape(w, e)?;
                w.seek_to(e)?;
            }
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(())
}

/// A `TypeProto`: its tensor type's element type and shape, into `info`.
pub(crate) fn type_proto<R: Read + Seek>(w: &mut Walker<R>, end: u64, info: &mut TensorInfo) -> Result<()> {
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (1, Wire::Len { end: e, .. }) => {
                tensor_type(w, e, info)?;
                w.seek_to(e)?;
            }
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(())
}

/// A `ValueInfoProto`: a name and a type.
pub(crate) fn value_info<R: Read + Seek>(w: &mut Walker<R>, end: u64) -> Result<TensorInfo> {
    let mut info = TensorInfo { name: String::new(), elem_type: 0, shape: Vec::new() };
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (1, Wire::Len { start, end: e }) => {
                info.name = w.string(start, e)?;
                w.seek_to(e)?;
            }
            (2, Wire::Len { end: e, .. }) => {
                type_proto(w, e, &mut info)?;
                w.seek_to(e)?;
            }
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(info)
}

/// The `name` (field 8) of a weight tensor; its bulk data is skipped, never read.
fn tensor_name<R: Read + Seek>(w: &mut Walker<R>, end: u64) -> Result<Option<String>> {
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (8, Wire::Len { start, end: e }) => return Ok(Some(w.string(start, e)?)),
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(None)
}

fn graph<R: Read + Seek>(w: &mut Walker<R>, end: u64, info: &mut OnnxInfo, weights: &mut HashSet<String>) -> Result<()> {
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (5, Wire::Len { end: e, .. }) => {
                if weights.len() < MAX_INITIALIZERS
                    && let Some(name) = tensor_name(w, e)?
                {
                    weights.insert(name);
                }
                w.seek_to(e)?;
            }
            (11, Wire::Len { end: e, .. }) => {
                if info.inputs.len() >= MAX_LISTED_INPUTS {
                    return malformed("the graph lists too many inputs");
                }
                info.inputs.push(value_info(w, e)?);
                w.seek_to(e)?;
            }
            (12, Wire::Len { end: e, .. }) => {
                if info.outputs.len() >= MAX_TENSORS {
                    return malformed("the graph has too many outputs");
                }
                info.outputs.push(value_info(w, e)?);
                w.seek_to(e)?;
            }
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(())
}

/// An `OperatorSetIdProto`: its version when it is the default domain.
fn opset<R: Read + Seek>(w: &mut Walker<R>, end: u64) -> Result<Option<u64>> {
    let (mut domain, mut version) = (String::new(), None);
    while let Some((n, f)) = w.next(end)? {
        match (n, f) {
            (1, Wire::Len { start, end: e }) => {
                domain = w.string(start, e)?;
                w.seek_to(e)?;
            }
            (2, Wire::Varint(v)) => version = Some(v),
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(if domain.is_empty() || domain == "ai.onnx" { version } else { None })
}

/// Reads an ONNX model's graph inputs and outputs from any seekable source. A source longer than
/// [`MAX_MODEL_BYTES`] is refused before a byte of it is read.
pub fn probe_reader<R: Read + Seek>(r: &mut R) -> Result<OnnxInfo> {
    let total = r.seek(SeekFrom::End(0))?;
    if total > MAX_MODEL_BYTES {
        return malformed("the model exceeds 512 MiB");
    }
    if total == 0 {
        return malformed("the file is empty");
    }
    r.seek(SeekFrom::Start(0))?;
    let mut w = Walker { r, total, fields: 0 };
    let (mut info, mut weights, mut saw_graph) = (OnnxInfo::default(), HashSet::new(), false);
    while let Some((n, f)) = w.next(total)? {
        match (n, f) {
            (2, Wire::Len { start, end }) => {
                info.producer = w.string(start, end)?;
                w.seek_to(end)?;
            }
            (7, Wire::Len { end, .. }) => {
                saw_graph = true;
                graph(&mut w, end, &mut info, &mut weights)?;
                w.seek_to(end)?;
            }
            (8, Wire::Len { end, .. }) => {
                if let Some(v) = opset(&mut w, end)? {
                    info.opset = Some(v);
                }
                w.seek_to(end)?;
            }
            (_, Wire::Len { end, .. }) => w.seek_to(end)?,
            _ => {}
        }
    }
    if !saw_graph {
        return malformed("there is no graph");
    }
    info.inputs.retain(|t| !weights.contains(&t.name));
    if info.inputs.len() > MAX_TENSORS {
        return malformed("the graph has too many inputs");
    }
    if info.inputs.is_empty() || info.outputs.is_empty() {
        return malformed("the graph has no inputs or no outputs");
    }
    Ok(info)
}

/// Reads an ONNX file's graph inputs and outputs.
pub fn probe_path(path: &Path) -> Result<OnnxInfo> {
    probe_reader(&mut io::BufReader::new(std::fs::File::open(path)?))
}
