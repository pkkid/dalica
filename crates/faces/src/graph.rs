//! A full, bounded read of a small ONNX inference model: its nodes, attributes and weights.
//!
//! The detector and the recogniser share this reader and [`crate::net`]. The limits are those of
//! [`crate::limits`]: a file of at most 512 MiB, at most 64 Mi weight elements, rank at most 8, a
//! capped number of nodes, weights and list values; every length is checked against its parent, and
//! a weight blob whose size disagrees with its shape is an error. What the interpreter does not
//! follow is refused by name ([`LoadError::Unsupported`]): external or sparse weights, custom
//! operator domains, training graphs, opsets outside 10–17, and element types other than float32
//! (int64 only for constant shapes).
//!
//! Ported from LightCraft `crates/faces/src/graph.rs` @ f34c7a8a (MIT OR Apache-2.0); training
//! graphs, declared ranks and the names of refused element types are Dalica's.

use std::collections::HashMap;
use std::io::Cursor;

use crate::limits::{MAX_LIST, MAX_MODEL_BYTES, MAX_NODE_ARITY, MAX_NODES, MAX_OUTPUTS, MAX_RANK, MAX_WEIGHT_ELEMENTS, MAX_WEIGHT_TENSORS, OPSETS};
use crate::onnx::{Dim, FLOAT32, INT64, LoadError, Result, Walker, Wire, element_type_name, malformed, unsupported};

/// A node attribute.
#[derive(Clone, Debug, PartialEq)]
pub enum Attr {
    Int(i64),
    Float(f32),
    Str(String),
    Ints(Vec<i64>),
    Floats(Vec<f32>),
    /// A kind the reader does not keep (a tensor, a sub-graph…): any operator given one is refused.
    Other,
}

/// One operator of the graph.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub op: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub attrs: HashMap<String, Attr>,
}

impl Node {
    /// A node with one output and the given attributes.
    pub fn new(op: &str, inputs: &[&str], output: &str, attrs: &[(&str, Attr)]) -> Node {
        Node {
            op: op.into(),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            outputs: vec![output.into()],
            attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        }
    }
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.attrs.get(name) {
            Some(Attr::Int(v)) => Some(*v),
            _ => None,
        }
    }
    pub fn ints(&self, name: &str) -> Option<&[i64]> {
        match self.attrs.get(name) {
            Some(Attr::Ints(v)) => Some(v),
            _ => None,
        }
    }
    pub fn string(&self, name: &str) -> Option<&str> {
        match self.attrs.get(name) {
            Some(Attr::Str(v)) => Some(v),
            _ => None,
        }
    }
}

/// A weight's values.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    F32(Vec<f32>),
    /// Only constant shapes (`Reshape`'s second input).
    I64(Vec<i64>),
}

/// A weight tensor (an initializer).
#[derive(Clone, Debug, PartialEq)]
pub struct Weight {
    pub dims: Vec<usize>,
    pub data: Data,
}

impl Weight {
    /// A float32 weight.
    pub fn f32(dims: &[usize], values: Vec<f32>) -> Weight {
        Weight { dims: dims.to_vec(), data: Data::F32(values) }
    }
    /// An int64 constant (a shape).
    pub fn i64(dims: &[usize], values: Vec<i64>) -> Weight {
        Weight { dims: dims.to_vec(), data: Data::I64(values) }
    }
}

/// A model's graph, as read.
#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub weights: HashMap<String, Weight>,
    /// The inputs the caller feeds (weights some exporters also list as inputs are left out), and
    /// their declared shapes (0 for a dynamic size).
    pub inputs: Vec<(String, Vec<usize>)>,
    pub outputs: Vec<String>,
}

fn f32s(raw: &[u8]) -> Vec<f32> {
    raw.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

fn i64s(raw: &[u8]) -> Vec<i64> {
    raw.as_chunks::<8>().0.iter().map(|b| i64::from_le_bytes(*b)).collect()
}

type Bytes<'a, 'b> = Walker<'a, Cursor<&'b [u8]>>;

/// A list of varints in a packed field (the reader up to `end`), appended to `out`.
fn packed_varints(w: &mut Bytes, end: u64, out: &mut Vec<u64>) -> Result<()> {
    while w.pos()? < end {
        if out.len() >= MAX_LIST {
            return malformed("a list is too long");
        }
        out.push(w.varint()?);
        if w.pos()? > end {
            return malformed("a packed number runs past its field");
        }
    }
    Ok(())
}

fn attribute(w: &mut Bytes, end: u64) -> Result<(String, Attr)> {
    let (mut name, mut kind) = (String::new(), 0u64);
    let (mut int, mut float, mut text) = (0i64, 0f32, String::new());
    let (mut ints, mut floats): (Vec<u64>, Vec<f32>) = (Vec::new(), Vec::new());
    while let Some((n, field)) = w.next(end)? {
        match (n, field) {
            (1, Wire::Len { start, end: e }) => {
                name = w.string(start, e)?;
                w.seek_to(e)?;
            }
            (2, Wire::Fixed32(v)) => float = f32::from_bits(v),
            // protobuf int64 is two's complement in a varint
            (3, Wire::Varint(v)) => int = v as i64,
            (4, Wire::Len { start, end: e }) => {
                text = w.string(start, e)?;
                w.seek_to(e)?;
            }
            (7, Wire::Len { start, end: e }) => {
                let raw = w.bytes(start, e, (MAX_LIST * 4) as u64)?;
                if !raw.len().is_multiple_of(4) || floats.len().saturating_add(raw.len() / 4) > MAX_LIST {
                    return malformed("an attribute's float list is invalid or too long");
                }
                floats.extend(f32s(&raw));
            }
            (7, Wire::Fixed32(v)) => {
                if floats.len() >= MAX_LIST {
                    return malformed("an attribute's float list is too long");
                }
                floats.push(f32::from_bits(v));
            }
            (8, Wire::Len { end: e, .. }) => {
                packed_varints(w, e, &mut ints)?;
                w.seek_to(e)?;
            }
            (8, Wire::Varint(v)) => {
                if ints.len() >= MAX_LIST {
                    return malformed("a list is too long");
                }
                ints.push(v);
            }
            (20, Wire::Varint(v)) => kind = v,
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    let value = match kind {
        1 => Attr::Float(float),
        2 => Attr::Int(int),
        3 => Attr::Str(text),
        6 => Attr::Floats(floats),
        7 => Attr::Ints(ints.into_iter().map(|v| v as i64).collect()),
        _ => Attr::Other,
    };
    Ok((name, value))
}

fn node(w: &mut Bytes, end: u64) -> Result<Node> {
    let mut n = Node { op: String::new(), inputs: Vec::new(), outputs: Vec::new(), attrs: HashMap::new() };
    while let Some((num, field)) = w.next(end)? {
        match (num, field) {
            (1, Wire::Len { start, end: e }) => {
                n.inputs.push(w.string(start, e)?);
                w.seek_to(e)?;
            }
            (2, Wire::Len { start, end: e }) => {
                n.outputs.push(w.string(start, e)?);
                w.seek_to(e)?;
            }
            (4, Wire::Len { start, end: e }) => {
                n.op = w.string(start, e)?;
                w.seek_to(e)?;
            }
            (5, Wire::Len { end: e, .. }) => {
                let (k, v) = attribute(w, e)?;
                if k.is_empty() || n.attrs.insert(k, v).is_some() {
                    return malformed("an attribute name is empty or repeated");
                }
                w.seek_to(e)?;
            }
            (7, Wire::Len { start, end: e }) => {
                let domain = w.string(start, e)?;
                if domain == "ai.onnx.preview.training" {
                    return unsupported("training graphs");
                }
                if !domain.is_empty() && domain != "ai.onnx" {
                    return unsupported(format!("operators of the custom domain `{domain}`"));
                }
                w.seek_to(e)?;
            }
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
        if n.inputs.len() > MAX_NODE_ARITY || n.outputs.len() > MAX_NODE_ARITY || n.attrs.len() > MAX_NODE_ARITY {
            return malformed("a node is too large");
        }
    }
    Ok(n)
}

/// A weight tensor (`TensorProto`): its name, shape and float32 or int64 values. `budget` is the
/// number of weight elements still allowed.
fn tensor(w: &mut Bytes, end: u64, budget: &mut usize) -> Result<(String, Weight)> {
    let (mut name, mut dtype) = (String::new(), 0u64);
    let mut dims: Vec<u64> = Vec::new();
    let mut raw: Option<&[u8]> = None;
    let (mut float_data, mut int_data): (Vec<f32>, Vec<u64>) = (Vec::new(), Vec::new());
    let max_bytes = (*budget as u64).saturating_mul(8);
    while let Some((n, field)) = w.next(end)? {
        match (n, field) {
            (1, Wire::Len { end: e, .. }) => {
                packed_varints(w, e, &mut dims)?;
                w.seek_to(e)?;
            }
            (1, Wire::Varint(v)) => dims.push(v),
            (2, Wire::Varint(v)) => dtype = v,
            (4, Wire::Len { start, end: e }) => {
                let bytes = w.bytes(start, e, max_bytes)?;
                if !bytes.len().is_multiple_of(4) || float_data.len().saturating_add(bytes.len() / 4) > *budget {
                    return malformed("invalid or excessive packed float weights");
                }
                float_data.extend(f32s(&bytes));
            }
            (4, Wire::Fixed32(v)) => {
                if float_data.len() >= *budget {
                    return malformed("too many float weights");
                }
                float_data.push(f32::from_bits(v));
            }
            (7, Wire::Varint(v)) => {
                if int_data.len() >= *budget {
                    return malformed("too many integer weights");
                }
                int_data.push(v);
            }
            (7, Wire::Len { end: e, .. }) => {
                packed_varints(w, e, &mut int_data)?;
                w.seek_to(e)?;
            }
            (8, Wire::Len { start, end: e }) => {
                name = w.string(start, e)?;
                w.seek_to(e)?;
            }
            (9, Wire::Len { start, end: e }) => {
                if raw.is_some() || e.saturating_sub(start) > max_bytes {
                    return malformed("repeated or excessive raw weights");
                }
                let from = usize::try_from(start).map_err(|_| LoadError::Malformed("a weight offset overflows".into()))?;
                let to = usize::try_from(e).map_err(|_| LoadError::Malformed("a weight offset overflows".into()))?;
                raw = Some((*w.r.get_ref()).get(from..to).ok_or_else(|| LoadError::Malformed("short raw weights".into()))?);
                w.seek_to(e)?;
            }
            (13, Wire::Len { .. }) | (14, Wire::Varint(1..)) => return unsupported("weights stored outside the model file"),
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
        if dims.len() > MAX_RANK {
            return malformed("a weight has more than 8 dimensions");
        }
    }
    if dtype != u64::from(FLOAT32) && dtype != u64::from(INT64) {
        return unsupported(format!("a weight of type {} (only float32, and int64 for shapes)", element_type_name(dtype)));
    }
    let mut elements: usize = 1;
    let mut shape = Vec::new();
    for d in &dims {
        let d = usize::try_from(*d).map_err(|_| LoadError::Malformed("a weight dimension is too large".into()))?;
        elements = elements.checked_mul(d).ok_or_else(|| LoadError::Malformed("a weight is too large".into()))?;
        shape.push(d);
    }
    if elements > *budget {
        return malformed("the weights exceed 64 Mi elements");
    }
    *budget -= elements;
    if raw.is_some() && (!float_data.is_empty() || !int_data.is_empty()) {
        return malformed("a weight has both raw and typed values");
    }
    let data = if dtype == u64::from(FLOAT32) {
        let v = match raw {
            Some(raw) if elements.checked_mul(4) != Some(raw.len()) => return malformed("a float weight's bytes disagree with its shape"),
            Some(raw) => f32s(raw),
            None => float_data,
        };
        if v.len() != elements {
            return malformed("a weight's size disagrees with its shape");
        }
        Data::F32(v)
    } else {
        let v = match raw {
            Some(raw) if elements.checked_mul(8) != Some(raw.len()) => return malformed("an integer weight's bytes disagree with its shape"),
            Some(raw) => i64s(raw),
            None => int_data.into_iter().map(|v| v as i64).collect(),
        };
        if v.len() != elements {
            return malformed("a weight's size disagrees with its shape");
        }
        Data::I64(v)
    };
    if name.is_empty() {
        return malformed("a weight has no name");
    }
    Ok((name, Weight { dims: shape, data }))
}

/// A declared graph input or output: its name and shape (0 for a dynamic size).
fn declared(w: &mut Bytes, end: u64) -> Result<(String, Vec<usize>)> {
    let info = crate::onnx::value_info(w, end)?;
    if info.shape.len() > MAX_RANK {
        return malformed("a declared input or output has more than 8 dimensions");
    }
    let shape = info
        .shape
        .iter()
        .map(|d| match d {
            Dim::Fixed(v) => usize::try_from(*v).map_err(|_| LoadError::Malformed("a declared dimension is too large".into())),
            Dim::Dynamic(_) => Ok(0),
        })
        .collect::<Result<_>>()?;
    Ok((info.name, shape))
}

/// The graph (`GraphProto`) ending at `end`, into `g`.
fn graph(w: &mut Bytes, end: u64, g: &mut Graph) -> Result<()> {
    let mut budget = MAX_WEIGHT_ELEMENTS;
    while let Some((n, field)) = w.next(end)? {
        match (n, field) {
            (1, Wire::Len { end: e, .. }) => {
                if g.nodes.len() >= MAX_NODES {
                    return malformed("the graph has too many nodes");
                }
                g.nodes.push(node(w, e)?);
                w.seek_to(e)?;
            }
            (5, Wire::Len { end: e, .. }) => {
                if g.weights.len() >= MAX_WEIGHT_TENSORS {
                    return malformed("the graph has too many weights");
                }
                let (name, weight) = tensor(w, e, &mut budget)?;
                if g.weights.insert(name, weight).is_some() {
                    return malformed("a weight is defined twice");
                }
                w.seek_to(e)?;
            }
            (11, Wire::Len { end: e, .. }) => {
                if g.inputs.len() >= MAX_WEIGHT_TENSORS {
                    return malformed("the graph lists too many inputs");
                }
                g.inputs.push(declared(w, e)?);
                w.seek_to(e)?;
            }
            (12, Wire::Len { end: e, .. }) => {
                if g.outputs.len() >= MAX_OUTPUTS {
                    return malformed("the graph has too many outputs");
                }
                g.outputs.push(declared(w, e)?.0);
                w.seek_to(e)?;
            }
            (15, Wire::Len { .. }) => return unsupported("sparse weights"),
            (_, Wire::Len { end: e, .. }) => w.seek_to(e)?,
            _ => {}
        }
    }
    Ok(())
}

/// Reads a bounded float32 ONNX inference model. Nothing is run: [`crate::net::Net::new`] checks the
/// operators and [`crate::runtime::Model::check_input`] the shapes and the working memory.
pub fn load(bytes: &[u8]) -> Result<Graph> {
    let total = bytes.len() as u64;
    if total > MAX_MODEL_BYTES {
        return malformed("the model exceeds 512 MiB");
    }
    let info = crate::onnx::probe_reader(&mut Cursor::new(bytes))?;
    match info.opset {
        Some(v) if OPSETS.contains(&v) => {}
        Some(v) => return unsupported(format!("opset {v} (only default-domain opsets 10 through 17)")),
        None => return unsupported("a model without a default-domain opset"),
    }
    if let Some(t) = info.inputs.iter().chain(&info.outputs).find(|t| t.elem_type != FLOAT32) {
        return unsupported(format!("the input or output `{}` of type {} (only float32)", t.name, element_type_name(u64::from(t.elem_type))));
    }
    if info.inputs.iter().chain(&info.outputs).any(|v| v.shape.iter().any(|d| matches!(d, Dim::Fixed(0)))) {
        return malformed("a declared input or output has a zero dimension");
    }
    let mut cursor = Cursor::new(bytes);
    let mut w = Walker { r: &mut cursor, total, fields: 0 };
    let mut g = Graph { nodes: Vec::new(), weights: HashMap::new(), inputs: Vec::new(), outputs: Vec::new() };
    let mut saw_graph = false;
    while let Some((n, field)) = w.next(total)? {
        match (n, field) {
            (7, Wire::Len { end, .. }) => {
                if saw_graph {
                    return malformed("the model has more than one graph");
                }
                saw_graph = true;
                graph(&mut w, end, &mut g)?;
                w.seek_to(end)?;
            }
            (20, Wire::Len { .. }) => return unsupported("training graphs"),
            (_, Wire::Len { end, .. }) => w.seek_to(end)?,
            _ => {}
        }
    }
    if !saw_graph || g.nodes.is_empty() {
        return malformed("the graph has no nodes");
    }
    let weights = &g.weights;
    g.inputs.retain(|(name, _)| !weights.contains_key(name));
    if g.inputs.is_empty() || g.outputs.is_empty() {
        return malformed("the graph has no inputs or no outputs");
    }
    Ok(g)
}
