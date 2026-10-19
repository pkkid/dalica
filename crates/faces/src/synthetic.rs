//! Seeded synthetic networks and a small ONNX writer, for tests, fuzz seeds and benches: no real
//! model is ever needed to exercise the interpreter (`faces.md` §10).
//!
//! [`encode`] writes any [`Graph`] as an ONNX file (opset 13, weights as raw data), so a test can
//! build a network as data, write it and read it back through the same [`crate::graph::load`] a
//! downloaded model goes through. The protobuf helpers ([`varint`], [`len_field`],
//! [`varint_field`], [`value_info_bytes`]) let a test write a malformed file field by field.
//! [`yunet_shaped`], [`sface_shaped`] and [`auraface_shaped`] are seeded networks of the candidate
//! models' shapes, for the benches (`development.md` §8, M13.31).
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

// ---------------------------------------------------------------------------------------------
// The candidate models' shapes (`faces.md` §3.2, §8): seeded networks with each candidate's
// input, outputs, operators, layer pattern and about its number of weights, so the benches time
// what a real model costs without one (reserved decision 5 has not chosen a model, and no model is
// ever downloaded for a bench). They are not the models: their weights are seeded noise.
// ---------------------------------------------------------------------------------------------

/// A seeded network of a candidate model's shape, with what it costs.
#[derive(Clone, Debug)]
pub struct Shaped {
    pub graph: Graph,
    /// The candidate's weight elements (a file holds four bytes for each); a stand-in's own
    /// constants (the detector's planted faces) are not counted.
    pub weights: usize,
    /// Multiply-adds of one run (the convolutions and the fully connected layer).
    pub macs: u64,
}

/// A face [`yunet_shaped`] reports on every picture: its cell in the stride-16 grid and its box's
/// side in cells of that grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Planted {
    pub row: usize,
    pub col: usize,
    pub side: f32,
}

/// The side, in cells, of the stride-16 grid [`yunet_shaped`] plants faces in for a detector input
/// of `side` pixels (`side` rounded down to a multiple of 32, at least 32, as it builds the
/// network): a planted cell is reported only when its row and column are below it.
pub fn planted_grid(side: usize) -> usize {
    side.max(32) / 32 * 32 / 16
}

/// Builds a [`Shaped`] node by node, counting its weights and multiply-adds.
struct Builder {
    r: Seeded,
    nodes: Vec<Node>,
    weights: Vec<(String, Weight)>,
    names: usize,
    count: usize,
    macs: u64,
}

impl Builder {
    fn new(seed: u32) -> Builder {
        Builder { r: Seeded::new(seed), nodes: Vec::new(), weights: Vec::new(), names: 0, count: 0, macs: 0 }
    }

    fn name(&mut self, what: &str) -> String {
        self.names += 1;
        format!("{what}{}", self.names)
    }

    fn constant(&mut self, what: &str, dims: &[usize], values: Vec<f32>) -> String {
        let name = self.name(what);
        self.count += values.len();
        self.weights.push((name.clone(), Weight::f32(dims, values)));
        name
    }

    /// Seeded values around `base`, uniform within ± `spread` / 2.
    fn seeded(&mut self, what: &str, dims: &[usize], base: f32, spread: f32) -> String {
        let n = dims.iter().product();
        let values = self.r.values(n, spread).into_iter().map(|v| base + v).collect();
        self.constant(what, dims, values)
    }

    fn node(&mut self, op: &str, inputs: &[&str], attrs: &[(&str, Attr)]) -> String {
        let out = self.name(&op.to_ascii_lowercase());
        self.nodes.push(Node::new(op, inputs, &out, attrs));
        out
    }

    /// A convolution with a bias (a batch normalisation after it folded in, as exporters do) on an
    /// `hw` input: `k` × `k`, `stride`, padded to keep the size at stride 1, `group` groups.
    /// Weights uniform with variance `gain` × 2 / fan-in. Returns the output and its size.
    #[allow(clippy::too_many_arguments)]
    fn conv(&mut self, x: &str, cin: usize, cout: usize, k: usize, stride: usize, group: usize, hw: usize, gain: f32) -> (String, usize) {
        let fan_in = cin / group * k * k;
        let spread = (24.0 * gain / fan_in as f32).sqrt();
        let w = self.seeded("w", &[cout, cin / group, k, k], 0.0, spread);
        let b = self.seeded("b", &[cout], 0.0, 0.02);
        let pad = (k / 2) as i64;
        let attrs = [
            ("kernel_shape", Attr::Ints(vec![k as i64; 2])),
            ("strides", Attr::Ints(vec![stride as i64; 2])),
            ("pads", Attr::Ints(vec![pad; 4])),
            ("group", Attr::Int(group as i64)),
        ];
        let out = self.node("Conv", &[x, &w, &b], &attrs);
        let side = hw.div_ceil(stride);
        self.macs += (cout * fan_in * side * side) as u64;
        (out, side)
    }

    fn batch_norm(&mut self, x: &str, c: usize) -> String {
        let s = self.seeded("bn", &[c], 1.0, 0.1);
        let b = self.seeded("bn", &[c], 0.0, 0.1);
        let m = self.seeded("bn", &[c], 0.0, 0.1);
        let v = self.seeded("bn", &[c], 1.0, 0.1);
        self.node("BatchNormalization", &[x, &s, &b, &m, &v], &[("epsilon", Attr::Float(1e-5))])
    }

    fn prelu(&mut self, x: &str, c: usize) -> String {
        let slope = self.seeded("slope", &[c, 1, 1], 0.25, 0.1);
        self.node("PRelu", &[x, &slope], &[])
    }

    fn graph(self, input: (&str, Vec<usize>), outputs: Vec<String>) -> Shaped {
        let (weights, macs) = (self.count, self.macs);
        let graph = Graph { nodes: self.nodes, weights: self.weights.into_iter().collect(), inputs: vec![(input.0.to_string(), input.1)], outputs };
        Shaped { graph, weights, macs }
    }
}

/// YuNet's convolution unit: a 1 × 1 convolution, a depthwise 3 × 3 one, and a ReLU unless it ends
/// a head.
fn dp_unit(b: &mut Builder, x: &str, cin: usize, cout: usize, hw: usize, relu: bool, gain: f32) -> String {
    let (y, _) = b.conv(x, cin, cout, 1, 1, 1, hw, gain);
    let (y, _) = b.conv(&y, cout, cout, 3, 1, cout, hw, gain);
    if relu { b.node("Relu", &[&y], &[]) } else { y }
}

fn max_pool(b: &mut Builder, x: &str) -> String {
    b.node("MaxPool", &[x], &[("kernel_shape", Attr::Ints(vec![2, 2])), ("strides", Attr::Ints(vec![2, 2]))])
}

/// The candidate detector's shape (YuNet 2023mar, `known::yunet`): input `input` `[1, 3, side,
/// side]` (`side` a multiple of 32; 640 for the candidate), a backbone of 1 × 1 and depthwise 3 × 3
/// units with max pooling down to strides 8, 16 and 32, a top-down neck (nearest upscaling and
/// additions), and per stride a head giving `cls_*`, `obj_*` (through a sigmoid), `bbox_*` and
/// `kps_*` as `[1, cells, 1 | 1 | 4 | 10]`: the outputs the `yunet-v2` decoder reads. About the
/// candidate's 76 k weights.
///
/// The heads' convolutions are seeded small and a constant map is added after them, so the
/// network reports exactly the `planted` faces (score about 0.9997, the template's five landmarks
/// in each box) and no other, whatever the picture: a scan of it finds the same faces on every
/// photo. Planted cells outside the stride-16 grid are left out.
pub fn yunet_shaped(seed: u32, side: usize, planted: &[Planted]) -> Shaped {
    let side = side.max(32) / 32 * 32;
    let mut b = Builder::new(seed);
    let (x, hw) = b.conv("input", 3, 16, 3, 2, 1, side, 1.0);
    let x = b.node("Relu", &[&x], &[]);
    let x = dp_unit(&mut b, &x, 16, 16, hw, true, 1.0);
    let x = max_pool(&mut b, &x);
    let hw = hw / 2;
    let x = dp_unit(&mut b, &x, 16, 16, hw, true, 1.0);
    let x = dp_unit(&mut b, &x, 16, 64, hw, true, 1.0);
    let x = dp_unit(&mut b, &x, 64, 64, hw, true, 1.0);
    let x = dp_unit(&mut b, &x, 64, 64, hw, true, 1.0);
    let mut stages = Vec::new();
    let (mut x, mut hw) = (x, hw);
    for _ in 0..3 {
        x = max_pool(&mut b, &x);
        hw /= 2;
        x = dp_unit(&mut b, &x, 64, 64, hw, true, 1.0);
        x = dp_unit(&mut b, &x, 64, 64, hw, true, 1.0);
        stages.push((x.clone(), hw));
    }
    // The neck, top down: stride 32 as it is, then each finer stride with the coarser one added.
    let scales = b.constant("scales", &[4], vec![1.0, 1.0, 2.0, 2.0]);
    let mut necks: Vec<(String, usize)> = Vec::new();
    let mut above: Option<String> = None;
    for (feature, hw) in stages.iter().rev() {
        let merged = match &above {
            Some(up) => {
                let up = b.node("Resize", &[up, "", &scales], &[]);
                let sum = b.node("Add", &[feature, &up], &[]);
                dp_unit(&mut b, &sum, 64, 64, *hw, true, 1.0)
            }
            None => feature.clone(),
        };
        above = Some(merged.clone());
        necks.push((merged, *hw));
    }
    necks.reverse();
    let mut outputs = Vec::new();
    for ((feature, hw), stride) in necks.iter().zip([8usize, 16, 32]) {
        let shared = dp_unit(&mut b, feature, 64, 64, *hw, true, 1.0);
        for (kind, n) in [("cls", 1usize), ("obj", 1), ("bbox", 4), ("kps", 10)] {
            let head = dp_unit(&mut b, &shared, 64, n, *hw, false, 1e-6);
            // The planted map is the stand-in's, not the candidate's: it is not counted.
            let map = b.name("plant");
            b.weights.push((map.clone(), Weight::f32(&[1, n, *hw, *hw], planted_map(kind, n, *hw, stride, planted))));
            let mut y = b.node("Add", &[&head, &map], &[]);
            if n == 1 {
                y = b.node("Sigmoid", &[&y], &[]);
            }
            let y = b.node("Transpose", &[&y], &[("perm", Attr::Ints(vec![0, 2, 3, 1]))]);
            let shape = b.name("shape");
            b.weights.push((shape.clone(), Weight::i64(&[3], vec![1, -1, n as i64])));
            let name = format!("{kind}_{stride}");
            b.nodes.push(Node::new("Reshape", &[&y, &shape], &name, &[]));
            outputs.push(name);
        }
    }
    b.graph(("input", vec![1, 3, side, side]), outputs)
}

/// The constant added to a head's `n` channels of an `hw` × `hw` grid at `stride`: scores far below
/// zero but at the planted cells (stride 16 only), and there each face's box and landmarks.
fn planted_map(kind: &str, n: usize, hw: usize, stride: usize, planted: &[Planted]) -> Vec<f32> {
    let plane = hw * hw;
    let mut v = vec![if n == 1 { -8.0 } else { 0.0 }; n * plane];
    if stride != 16 {
        return v;
    }
    for p in planted.iter().filter(|p| p.row < hw && p.col < hw) {
        let side = p.side.max(0.5);
        let values: Vec<f32> = match kind {
            "cls" | "obj" => vec![8.0],
            "bbox" => vec![0.5, 0.5, side.ln(), side.ln()],
            // The template's landmarks in a box of `side` cells centred on the cell's middle, as
            // offsets from the cell's corner.
            _ => crate::align::TEMPLATE_112.iter().flat_map(|(x, y)| [0.5 + side * (x / 112.0 - 0.5), 0.5 + side * (y / 112.0 - 0.5)]).collect(),
        };
        for (c, value) in values.into_iter().enumerate().take(n) {
            if let Some(slot) = v.get_mut(c * plane + p.row * hw + p.col) {
                *slot = value;
            }
        }
    }
    v
}

/// A recogniser of the candidates' shape: an IResNet (the ArcFace family's residual network) on a
/// `[1, 3, 112, 112]` input `data` (`normalise`: the pixels' `(x − 127.5) / 127.5` done inside, by
/// Sub and Mul), a 3 × 3 stem of 64 channels, four stages of `blocks` residual blocks (64, 128, 256
/// and 512 channels, each stage halving the size: batch normalisation, a 3 × 3 convolution, PReLU, a
/// 3 × 3 convolution, a 1 × 1 shortcut where the size changes, an addition), then batch
/// normalisation, Dropout, Flatten, a fully connected layer to `dim` and a last batch
/// normalisation: output `emb` `[1, dim]`.
pub fn iresnet_shaped(seed: u32, blocks: [usize; 4], dim: usize, normalise: bool) -> Shaped {
    let mut b = Builder::new(seed);
    let mut x = "data".to_string();
    if normalise {
        let centre = b.constant("centre", &[1], vec![127.5]);
        let scale = b.constant("scale", &[1], vec![1.0 / 127.5]);
        x = b.node("Sub", &[&x, &centre], &[]);
        x = b.node("Mul", &[&x, &scale], &[]);
    }
    let (y, mut hw) = b.conv(&x, 3, 64, 3, 1, 1, 112, 1.0);
    let mut x = b.prelu(&y, 64);
    let mut cin = 64;
    for (stage, n) in blocks.into_iter().enumerate() {
        let cout = 64 << stage;
        for i in 0..n.max(1) {
            let stride = if i == 0 { 2 } else { 1 };
            let normed = b.batch_norm(&x, cin);
            let (y, _) = b.conv(&normed, cin, cout, 3, 1, 1, hw, 1.0);
            let y = b.prelu(&y, cout);
            // The residual branch starts small, so a deep stack of them stays finite.
            let (y, out_hw) = b.conv(&y, cout, cout, 3, stride, 1, hw, 0.01);
            let shortcut = if stride != 1 || cin != cout { b.conv(&x, cin, cout, 1, stride, 1, hw, 1.0).0 } else { x.clone() };
            x = b.node("Add", &[&y, &shortcut], &[]);
            (cin, hw) = (cout, out_hw);
        }
    }
    let y = b.batch_norm(&x, cin);
    let y = b.node("Dropout", &[&y], &[]);
    let y = b.node("Flatten", &[&y], &[]);
    let features = cin * hw * hw;
    let w = b.seeded("fc", &[dim, features], 0.0, (12.0 / features as f32).sqrt());
    let bias = b.seeded("fc", &[dim], 0.0, 0.02);
    let y = b.node("Gemm", &[&y, &w, &bias], &[("transB", Attr::Int(1))]);
    b.macs += (dim * features) as u64;
    let s = b.seeded("bn", &[dim], 1.0, 0.1);
    let sb = b.seeded("bn", &[dim], 0.0, 0.1);
    let m = b.seeded("bn", &[dim], 0.0, 0.1);
    let v = b.seeded("bn", &[dim], 1.0, 0.1);
    b.nodes.push(Node::new("BatchNormalization", &[&y, &s, &sb, &m, &v], "emb", &[("epsilon", Attr::Float(1e-5))]));
    b.graph(("data", vec![1, 3, 112, 112]), vec!["emb".into()])
}

/// The first candidate recogniser's shape (SFace 2021dec, `known::sface`): an IResNet of 1, 2, 2
/// and 1 blocks to a 128-number embedding, normalising inside as its manifest expects (about its
/// 9.7 M weights).
pub fn sface_shaped(seed: u32) -> Shaped {
    iresnet_shaped(seed, [1, 2, 2, 1], 128, true)
}

/// The second candidate recogniser's shape (AuraFace v1 `glintr100`, `known::auraface`): IResNet-100
/// (3, 13, 30 and 3 blocks) to a 512-number embedding, normalised by the caller as its manifest
/// says (about its 65 M weights).
pub fn auraface_shaped(seed: u32) -> Shaped {
    iresnet_shaped(seed, [3, 13, 30, 3], 512, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::Rgb;
    use crate::detect::{Detector, probe_picture};
    use crate::embed::Embedder;
    use crate::known;
    use crate::runtime::Model;

    /// Within 5 % of the candidate file's weights (four bytes each).
    fn near_file(weights: usize, file_bytes: u64) {
        let want = file_bytes as f64 / 4.0;
        assert!((weights as f64 / want - 1.0).abs() < 0.05, "{weights} weights against the file's {want:.0}");
    }

    #[test]
    fn the_detector_shape_reports_exactly_the_planted_faces_on_any_picture() {
        let planted = [Planted { row: 1, col: 1, side: 1.5 }, Planted { row: 2, col: 2, side: 1.0 }, Planted { row: 9, col: 9, side: 1.0 }];
        let shaped = yunet_shaped(7, 64, &planted);
        let mut m = known::yunet();
        m.input.width = 64;
        m.input.height = 64;
        let det = Detector::new(Model::from_bytes(&encode(&shaped.graph)).unwrap(), &m).unwrap();
        let pixels = probe_picture(64, 64);
        let faces = det.detect(&Rgb { data: &pixels, width: 64, height: 64 }, &det.options()).unwrap();
        // The cell outside the 4 × 4 stride-16 grid is left out; the two inside, one face each.
        assert_eq!(planted_grid(64), 4);
        assert_eq!(faces.len(), 2, "{faces:?}");
        let centre = |f: &crate::detect::Face| ((f.rect[0] + f.rect[2]) / 2.0, (f.rect[1] + f.rect[3]) / 2.0);
        let mut centres: Vec<_> = faces.iter().map(centre).collect();
        centres.sort_by(|a, b| a.0.total_cmp(&b.0));
        for ((x, y), cell) in centres.iter().zip([1.5f32, 2.5]) {
            assert!((x - cell * 16.0 / 64.0).abs() < 0.01 && (y - cell * 16.0 / 64.0).abs() < 0.01, "{x}, {y}");
        }
        assert!(faces.iter().all(|f| f.score > 0.99));
        let full = yunet_shaped(7, 640, &[]);
        assert!((60_000..90_000).contains(&full.weights), "about YuNet's 76 k weights: {}", full.weights);
    }

    #[test]
    fn the_recogniser_shapes_load_as_their_candidates_with_about_their_weights() {
        let sface = sface_shaped(3);
        near_file(sface.weights, known::sface().size_bytes.unwrap());
        let e = Embedder::new(Model::from_bytes(&encode(&sface.graph)).unwrap(), &known::sface()).unwrap();
        assert_eq!((e.input_size(), e.dimension()), ((112, 112), 128));
        // AuraFace's whole network is 260 MB: its count is checked here, its load by the bench.
        let aura = auraface_shaped(3);
        near_file(aura.weights, known::auraface().size_bytes.unwrap());
        assert!(
            aura.weights <= crate::limits::MAX_WEIGHT_ELEMENTS && aura.macs > 5 * sface.macs,
            "{} weights, {} and {} multiply-adds",
            aura.weights,
            aura.macs,
            sface.macs
        );
    }
}
