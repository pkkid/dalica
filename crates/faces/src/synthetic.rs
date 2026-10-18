//! Seeded synthetic networks and a small ONNX writer, for tests, fuzz seeds and benches: no real
//! model is ever needed to exercise the interpreter (`faces.md` §10).
//!
//! [`encode`] writes any [`Graph`] as an ONNX file (opset 13, weights as raw data), so a test can
//! build a network as data, write it and read it back through the same [`crate::graph::load`] a
//! downloaded model goes through. The protobuf helpers ([`varint`], [`len_field`],
//! [`varint_field`], [`value_info_bytes`]) let a test write a malformed file field by field.
//!
//! Ported from LightCraft `crates/faces/src/synthetic.rs` @ f34c7a8a (MIT OR Apache-2.0): the
//! protobuf helpers, the probe-only and tiny embedder models, the recognition model and the
//! every-operator graph; [`encode`], [`Seeded`] and the seeded weights are Dalica's.

use std::collections::HashMap;

use crate::graph::{Attr, Data, Graph, Node, Weight};

/// Appends `v` as a protobuf varint.
pub fn varint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// Appends field `n` as a length-delimited field holding `body`.
pub fn len_field(n: u64, body: &[u8], out: &mut Vec<u8>) {
    varint(n << 3 | 2, out);
    varint(body.len() as u64, out);
    out.extend_from_slice(body);
}

/// Appends field `n` as a varint field holding `v`.
pub fn varint_field(n: u64, v: u64, out: &mut Vec<u8>) {
    varint(n << 3, out);
    varint(v, out);
}

/// Appends field `n` as a 32-bit field holding `v`.
pub fn fixed32_field(n: u64, v: u32, out: &mut Vec<u8>) {
    varint(n << 3 | 5, out);
    out.extend_from_slice(&v.to_le_bytes());
}

/// A graph input or output (`ValueInfoProto`): `Ok(n)` is a fixed dimension, `Err(name)` a named
/// (dynamic) one.
pub fn value_info_bytes(name: &str, elem: u64, dims: &[Result<u64, &str>]) -> Vec<u8> {
    let mut shape = Vec::new();
    for d in dims {
        let mut dim = Vec::new();
        match d {
            Ok(v) => varint_field(1, *v, &mut dim),
            Err(p) => len_field(2, p.as_bytes(), &mut dim),
        }
        len_field(1, &dim, &mut shape);
    }
    let mut tensor = Vec::new();
    varint_field(1, elem, &mut tensor);
    len_field(2, &shape, &mut tensor);
    let mut ty = Vec::new();
    len_field(1, &tensor, &mut ty);
    let mut vi = Vec::new();
    len_field(1, name.as_bytes(), &mut vi);
    len_field(2, &ty, &mut vi);
    vi
}

/// An attribute (`AttributeProto`), or `None` for [`Attr::Other`], which has no encoding here.
fn attribute_bytes(name: &str, value: &Attr) -> Option<Vec<u8>> {
    let mut a = Vec::new();
    len_field(1, name.as_bytes(), &mut a);
    match value {
        Attr::Float(v) => {
            fixed32_field(2, v.to_bits(), &mut a);
            varint_field(20, 1, &mut a);
        }
        Attr::Int(v) => {
            varint_field(3, *v as u64, &mut a);
            varint_field(20, 2, &mut a);
        }
        Attr::Str(v) => {
            len_field(4, v.as_bytes(), &mut a);
            varint_field(20, 3, &mut a);
        }
        Attr::Floats(values) => {
            let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            len_field(7, &raw, &mut a);
            varint_field(20, 6, &mut a);
        }
        Attr::Ints(values) => {
            for v in values {
                varint_field(8, *v as u64, &mut a);
            }
            varint_field(20, 7, &mut a);
        }
        Attr::Other => return None,
    }
    Some(a)
}

/// A node (`NodeProto`); its attributes in name order, so the bytes do not depend on hashing.
pub fn node_bytes(node: &Node) -> Vec<u8> {
    let mut n = Vec::new();
    for i in &node.inputs {
        len_field(1, i.as_bytes(), &mut n);
    }
    for o in &node.outputs {
        len_field(2, o.as_bytes(), &mut n);
    }
    len_field(4, node.op.as_bytes(), &mut n);
    let mut attrs: Vec<_> = node.attrs.iter().collect();
    attrs.sort_by(|a, b| a.0.cmp(b.0));
    for (name, value) in attrs {
        if let Some(a) = attribute_bytes(name, value) {
            len_field(5, &a, &mut n);
        }
    }
    n
}

/// A weight (`TensorProto`) with its values as raw data.
pub fn weight_bytes(name: &str, weight: &Weight) -> Vec<u8> {
    let mut t = Vec::new();
    for d in &weight.dims {
        varint_field(1, *d as u64, &mut t);
    }
    let raw: Vec<u8> = match &weight.data {
        Data::F32(v) => {
            varint_field(2, 1, &mut t);
            v.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
        Data::I64(v) => {
            varint_field(2, 7, &mut t);
            v.iter().flat_map(|v| v.to_le_bytes()).collect()
        }
    };
    len_field(8, name.as_bytes(), &mut t);
    len_field(9, &raw, &mut t);
    t
}

/// Writes `graph` as an ONNX model (opset 13): its nodes, its weights in name order, its input
/// with its declared shape (0 written as a dynamic `N`) and its outputs as float32 of no declared
/// shape.
pub fn encode(graph: &Graph) -> Vec<u8> {
    let mut g = Vec::new();
    for n in &graph.nodes {
        len_field(1, &node_bytes(n), &mut g);
    }
    let mut weights: Vec<_> = graph.weights.iter().collect();
    weights.sort_by(|a, b| a.0.cmp(b.0));
    for (name, w) in weights {
        len_field(5, &weight_bytes(name, w), &mut g);
    }
    for (name, shape) in &graph.inputs {
        let dims: Vec<Result<u64, &str>> = shape.iter().map(|&d| if d == 0 { Err("N") } else { Ok(d as u64) }).collect();
        len_field(11, &value_info_bytes(name, 1, &dims), &mut g);
    }
    for name in &graph.outputs {
        len_field(12, &value_info_bytes(name, 1, &[]), &mut g);
    }
    let mut ops = Vec::new();
    len_field(1, b"", &mut ops);
    varint_field(2, 13, &mut ops);
    let mut model = Vec::new();
    varint_field(1, 8, &mut model);
    len_field(2, b"dalica-synthetic", &mut model);
    len_field(7, &g, &mut model);
    len_field(8, &ops, &mut model);
    model
}

/// A small deterministic generator of weights (a linear congruential sequence).
#[derive(Clone, Debug)]
pub struct Seeded(u32);

impl Seeded {
    pub fn new(seed: u32) -> Seeded {
        Seeded(seed ^ 0x9e37_79b9)
    }

    /// The next value, uniform in `[-0.5, 0.5)` times `scale`.
    pub fn next(&mut self, scale: f32) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.0 >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * scale
    }

    /// `n` values, as [`Seeded::next`].
    pub fn values(&mut self, n: usize, scale: f32) -> Vec<f32> {
        (0..n).map(|_| self.next(scale)).collect()
    }
}

/// A model shaped like a face embedder for the probe only: float input `data` `[N, 3, 112, 112]`,
/// an old-style weight also listed as an input, float output `emb` `[N, output_dim]`, opset 17, and
/// no nodes (so it does not load as a network).
pub fn embedder_model(output_dim: u64) -> Vec<u8> {
    let mut graph = Vec::new();
    len_field(11, &value_info_bytes("data", 1, &[Err("N"), Ok(3), Ok(112), Ok(112)]), &mut graph);
    len_field(11, &value_info_bytes("w", 1, &[Ok(4)]), &mut graph);
    let mut tensor = Vec::new();
    varint_field(1, 4, &mut tensor);
    len_field(8, b"w", &mut tensor);
    len_field(9, &[7u8; 5000], &mut tensor);
    len_field(5, &tensor, &mut graph);
    len_field(12, &value_info_bytes("emb", 1, &[Err("N"), Ok(output_dim)]), &mut graph);
    let mut ops = Vec::new();
    len_field(1, b"", &mut ops);
    varint_field(2, 17, &mut ops);
    let mut model = Vec::new();
    varint_field(1, 8, &mut model);
    len_field(2, b"pytorch", &mut model);
    len_field(7, &graph, &mut model);
    len_field(8, &ops, &mut model);
    model
}

fn graph_of(nodes: Vec<Node>, weights: Vec<(&str, Weight)>, input: (&str, Vec<usize>), outputs: &[&str]) -> Graph {
    Graph {
        nodes,
        weights: weights.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<HashMap<_, _>>(),
        inputs: vec![(input.0.to_string(), input.1)],
        outputs: outputs.iter().map(|s| s.to_string()).collect(),
    }
}

/// A tiny working network shaped like a face embedder: input `data` `[1, 3, 112, 112]`, a 1 × 1
/// convolution to `dim` channels, global average pooling and flattening to `[1, dim]`.
pub fn tiny_embedder_model(dim: usize) -> Vec<u8> {
    let w: Vec<f32> = (0..dim * 3).map(|i| ((i * 37 % 19) as f32 - 9.0) / 20.0).collect();
    let b: Vec<f32> = (0..dim).map(|i| (i % 5) as f32 / 10.0 - 0.2).collect();
    encode(&graph_of(
        vec![
            Node::new("Conv", &["data", "w", "b"], "c", &[("kernel_shape", Attr::Ints(vec![1, 1]))]),
            Node::new("GlobalAveragePool", &["c"], "p", &[]),
            Node::new("Flatten", &["p"], "emb", &[("axis", Attr::Int(1))]),
        ],
        vec![("w", Weight::f32(&[dim, 3, 1, 1], w)), ("b", Weight::f32(&[dim], b))],
        ("data", vec![1, 3, 112, 112]),
        &["emb"],
    ))
}

/// A seeded recognition network using the recognisers' operators (Sub, Mul, a strided padded Conv,
/// BatchNormalization, PRelu, Add, Dropout, GlobalAveragePool, Flatten, Gemm): input `data`
/// `[1, 3, 112, 112]` of pixel values, output `emb` `[1, 8]`.
pub fn recognition_model(seed: u32) -> Vec<u8> {
    let mut r = Seeded::new(seed);
    let nodes = vec![
        Node::new("Sub", &["data", "center"], "centered", &[]),
        Node::new("Mul", &["centered", "scale"], "scaled", &[]),
        Node::new("Conv", &["scaled", "conv.w", "conv.b"], "conv", &[("pads", Attr::Ints(vec![1; 4])), ("strides", Attr::Ints(vec![2, 2]))]),
        Node::new("BatchNormalization", &["conv", "bn.s", "bn.b", "bn.m", "bn.v"], "bn", &[("epsilon", Attr::Float(1e-5))]),
        Node::new("PRelu", &["bn", "slope"], "act", &[]),
        Node::new("Add", &["act", "bn"], "skip", &[]),
        Node::new("Dropout", &["skip"], "drop", &[("ratio", Attr::Float(0.4))]),
        Node::new("GlobalAveragePool", &["drop"], "pool", &[]),
        Node::new("Flatten", &["pool"], "flat", &[]),
        Node::new("Gemm", &["flat", "fc.w", "fc.b"], "emb", &[("transB", Attr::Int(1))]),
    ];
    let weights = vec![
        ("center", Weight::f32(&[1], vec![127.5])),
        ("scale", Weight::f32(&[1], vec![1.0 / 127.5])),
        ("conv.w", Weight::f32(&[5, 3, 3, 3], r.values(135, 0.2))),
        ("conv.b", Weight::f32(&[5], r.values(5, 0.2))),
        ("bn.s", Weight::f32(&[5], r.values(5, 0.2).into_iter().map(|v| 1.0 + v).collect())),
        ("bn.b", Weight::f32(&[5], r.values(5, 0.2))),
        ("bn.m", Weight::f32(&[5], r.values(5, 0.2))),
        ("bn.v", Weight::f32(&[5], r.values(5, 0.2).into_iter().map(|v| 1.0 + v).collect())),
        ("slope", Weight::f32(&[5, 1, 1], r.values(5, 0.2).into_iter().map(|v| 0.2 + v).collect())),
        ("fc.w", Weight::f32(&[8, 5], r.values(40, 0.2))),
        ("fc.b", Weight::f32(&[8], r.values(8, 0.2))),
    ];
    encode(&graph_of(nodes, weights, ("data", vec![1, 3, 112, 112]), &["emb"]))
}

/// A detector-shaped stand-in that reports nothing: input `input` `[1, 3, size, size]` and, for
/// strides 8, 16 and 32, the outputs `cls_*`, `obj_*`, `bbox_*` and `kps_*` (`[1, cells, 1|1|4|10]`)
/// as constant zeros through `Identity`. `size` must be a multiple of 32.
pub fn tiny_detector_model(size: usize) -> Vec<u8> {
    let (mut nodes, mut weights, mut outputs) = (Vec::new(), Vec::new(), Vec::new());
    for stride in [8usize, 16, 32] {
        let cells = (size / stride).max(1).pow(2);
        for (kind, components) in [("cls", 1usize), ("obj", 1), ("bbox", 4), ("kps", 10)] {
            let name = format!("{kind}_{stride}");
            let weight = format!("weight_{name}");
            nodes.push(Node::new("Identity", &[&weight], &name, &[]));
            weights.push((weight, Weight::f32(&[1, cells, components], vec![0.0; cells * components])));
            outputs.push(name);
        }
    }
    let graph = Graph { nodes, weights: weights.into_iter().collect(), inputs: vec![("input".into(), vec![1, 3, size, size])], outputs };
    encode(&graph)
}

/// A seeded network of every operator the interpreter runs (both kinds of convolution, batch
/// normalisation, PRelu, Sigmoid, broadcasting Add, Sub and Mul with values and constants, max
/// pooling, nearest upscaling, transposes, Reshape, Identity, Dropout, global pooling, Flatten and
/// Gemm), with two outputs (`y` `[1, 5]` and `nhwc` `[1, 4, 4, 8]`). Its input `x` is `[1, 4, 8, 8]`.
pub fn every_operator_graph(seed: u32) -> Graph {
    let mut r = Seeded::new(seed);
    let positive = |r: &mut Seeded, n: usize, base: f32| -> Vec<f32> { r.values(n, 0.4).into_iter().map(|v| base + v).collect() };
    let weights = vec![
        ("w1", Weight::f32(&[8, 4, 3, 3], r.values(288, 0.9))),
        ("b1", Weight::f32(&[8], r.values(8, 0.9))),
        ("bn_s", Weight::f32(&[8], positive(&mut r, 8, 0.8))),
        ("bn_b", Weight::f32(&[8], r.values(8, 0.9))),
        ("bn_m", Weight::f32(&[8], r.values(8, 0.9))),
        ("bn_v", Weight::f32(&[8], positive(&mut r, 8, 0.5))),
        ("slope", Weight::f32(&[8, 1, 1], positive(&mut r, 8, 0.2))),
        ("w2", Weight::f32(&[8, 1, 3, 3], r.values(72, 0.9))),
        ("scales", Weight::f32(&[4], vec![1.0, 1.0, 2.0, 2.0])),
        ("half", Weight::f32(&[1], vec![0.5])),
        ("per_channel", Weight::f32(&[1, 8, 1, 1], r.values(8, 0.9))),
        ("wg", Weight::f32(&[5, 8], r.values(40, 0.9))),
        ("bg", Weight::f32(&[5], r.values(5, 0.9))),
        ("flat", Weight::i64(&[2], vec![1, -1])),
    ];
    let pads = |p: i64| ("pads", Attr::Ints(vec![p; 4]));
    let nodes = vec![
        Node::new("Conv", &["x", "w1", "b1"], "c1", &[pads(1)]),
        Node::new("BatchNormalization", &["c1", "bn_s", "bn_b", "bn_m", "bn_v"], "bn", &[]),
        Node::new("PRelu", &["bn", "slope"], "pr", &[]),
        Node::new("Conv", &["pr", "w2"], "c2", &[("group", Attr::Int(8)), ("strides", Attr::Ints(vec![2, 2])), pads(1)]),
        Node::new("Sigmoid", &["c2"], "sg", &[]),
        Node::new("Mul", &["c2", "sg"], "swish", &[]),
        Node::new("Resize", &["swish", "", "scales"], "up", &[]),
        Node::new("Add", &["up", "pr"], "sum", &[]),
        Node::new("Sub", &["sum", "half"], "less", &[]),
        Node::new("MaxPool", &["less"], "pool", &[("kernel_shape", Attr::Ints(vec![2, 2])), ("strides", Attr::Ints(vec![2, 2]))]),
        Node::new("Relu", &["pool"], "relu", &[]),
        Node::new("Transpose", &["relu"], "nhwc", &[("perm", Attr::Ints(vec![0, 2, 3, 1]))]),
        Node::new("Transpose", &["nhwc"], "nchw", &[("perm", Attr::Ints(vec![0, 3, 1, 2]))]),
        Node::new("Mul", &["nchw", "per_channel"], "scaled", &[]),
        Node::new("Identity", &["scaled"], "same", &[]),
        Node::new("Dropout", &["same"], "kept", &[]),
        Node::new("GlobalAveragePool", &["kept"], "avg", &[]),
        Node::new("Flatten", &["avg"], "row", &[]),
        Node::new("Reshape", &["row", "flat"], "row2", &[]),
        Node::new("Gemm", &["row2", "wg", "bg"], "y", &[("transB", Attr::Int(1)), ("alpha", Attr::Float(0.5))]),
    ];
    graph_of(nodes, weights, ("x", vec![1, 4, 8, 8]), &["y", "nhwc"])
}
