//! The ONNX interpreter (`faces.md` §2, §10): every operator against a direct computation, PReLU
//! broadcasting, grouped convolutions and padding, a model shared between threads, and malformed,
//! truncated, oversized, unsupported and overflowing models refused with their reason, never a
//! panic. Every network is seeded, written with `synthetic::encode` and read back through the
//! loader a downloaded model goes through.
//!
//! Adapted from LightCraft `crates/faces/src/net.rs`, `net/recognition_tests.rs`,
//! `graph/tests.rs` and `onnx.rs` tests @ f34c7a8a (MIT OR Apache-2.0).

use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;

use dalica_faces::graph::{Attr, Graph, Node, Weight};
use dalica_faces::limits::MAX_MODEL_BYTES;
use dalica_faces::net::{Net, NetError, Tensor};
use dalica_faces::onnx::{LoadError, probe_reader};
use dalica_faces::runtime::{Model, ModelError};
use dalica_faces::synthetic::{self, Seeded, encode, len_field, value_info_bytes, varint, varint_field};
use proptest::prelude::*;

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
fn cases() -> u32 {
    if std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1") { 512 } else { 32 }
}

const TOLERANCE: f32 = 1e-5;

fn graph(nodes: Vec<Node>, weights: Vec<(&str, Weight)>, input: &[usize], outputs: &[&str]) -> Graph {
    Graph {
        nodes,
        weights: weights.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        inputs: vec![("x".into(), input.to_vec())],
        outputs: outputs.iter().map(|s| s.to_string()).collect(),
    }
}

/// One operator `op` reading `x` and `extra`, written as a file and loaded back.
fn one(op: &str, extra: &[&str], weights: Vec<(&str, Weight)>, attrs: &[(&str, Attr)], input: &[usize]) -> Result<Model, ModelError> {
    let inputs: Vec<&str> = std::iter::once("x").chain(extra.iter().copied()).collect();
    Model::from_bytes(&encode(&graph(vec![Node::new(op, &inputs, "y", attrs)], weights, input, &["y"])))
}

fn seeded(shape: &[usize], seed: u32, scale: f32) -> Tensor {
    Tensor::new(shape.to_vec(), Seeded::new(seed).values(shape.iter().product(), scale)).unwrap()
}

fn output(model: &Model, x: &Tensor) -> Tensor {
    model.run(x.clone()).unwrap().remove(0).1
}

fn assert_close(got: &Tensor, shape: &[usize], want: &[f32], what: &str) {
    assert_eq!(got.shape, shape, "{what}: shape");
    assert_eq!(got.data.len(), want.len(), "{what}: length");
    for (i, (a, b)) in got.data.iter().zip(want).enumerate() {
        assert!((a - b).abs() <= TOLERANCE * b.abs().max(1.0), "{what}: value {i} is {a}, a direct computation gives {b}");
    }
}

/// The flat index into `input` of element `at` of `out` under trailing-dimension broadcasting.
fn broadcast_index(mut at: usize, out: &[usize], input: &[usize]) -> usize {
    let (mut index, mut stride) = (0, 1);
    for (axis, &size) in out.iter().enumerate().rev() {
        let coordinate = at % size;
        at /= size;
        if let Some(&d) = (axis + input.len()).checked_sub(out.len()).and_then(|j| input.get(j)) {
            if d != 1 {
                index += coordinate * stride;
            }
            stride *= d;
        }
    }
    index
}

struct Conv {
    cin: usize,
    cout: usize,
    groups: usize,
    k: (usize, usize),
    stride: (usize, usize),
    /// top, left, bottom, right
    pads: [usize; 4],
    size: (usize, usize),
}

/// A convolution computed one output at a time, straight from its definition.
fn conv_reference(c: &Conv, x: &[f32], w: &[f32], bias: &[f32]) -> (Vec<usize>, Vec<f32>) {
    let (h, wi) = c.size;
    let (kh, kw) = c.k;
    let [pt, pl, pb, pr] = c.pads;
    let (oh, ow) = ((h + pt + pb - kh) / c.stride.0 + 1, (wi + pl + pr - kw) / c.stride.1 + 1);
    let cg = c.cin / c.groups;
    let mut out = vec![0f32; c.cout * oh * ow];
    for co in 0..c.cout {
        let group = co / (c.cout / c.groups);
        for oy in 0..oh {
            for ox in 0..ow {
                let mut acc = bias[co];
                for ci in 0..cg {
                    for ky in 0..kh {
                        for kx in 0..kw {
                            let iy = (oy * c.stride.0 + ky) as isize - pt as isize;
                            let ix = (ox * c.stride.1 + kx) as isize - pl as isize;
                            if iy >= 0 && ix >= 0 && (iy as usize) < h && (ix as usize) < wi {
                                acc += x[((group * cg + ci) * h + iy as usize) * wi + ix as usize] * w[((co * cg + ci) * kh + ky) * kw + kx];
                            }
                        }
                    }
                }
                out[(co * oh + oy) * ow + ox] = acc;
            }
        }
    }
    (vec![1, c.cout, oh, ow], out)
}

#[test]
fn convolutions_equal_a_direct_computation() {
    let cases = [
        // dense, padded
        Conv { cin: 3, cout: 4, groups: 1, k: (3, 3), stride: (1, 1), pads: [1; 4], size: (7, 9) },
        // strided
        Conv { cin: 3, cout: 4, groups: 1, k: (3, 3), stride: (2, 2), pads: [1; 4], size: (8, 8) },
        // different strides and asymmetric pads
        Conv { cin: 2, cout: 3, groups: 1, k: (3, 2), stride: (2, 1), pads: [2, 1, 0, 1], size: (9, 7) },
        // depthwise
        Conv { cin: 4, cout: 4, groups: 4, k: (3, 3), stride: (1, 1), pads: [1; 4], size: (6, 5) },
        // depthwise, strided, with a channel multiplier
        Conv { cin: 3, cout: 6, groups: 3, k: (3, 3), stride: (2, 2), pads: [1; 4], size: (9, 8) },
        // grouped
        Conv { cin: 4, cout: 8, groups: 2, k: (3, 3), stride: (1, 1), pads: [1; 4], size: (5, 6) },
        // pointwise (a plain matrix product)
        Conv { cin: 2, cout: 6, groups: 1, k: (1, 1), stride: (1, 1), pads: [0; 4], size: (5, 5) },
        // a single input channel, no padding
        Conv { cin: 1, cout: 2, groups: 1, k: (3, 3), stride: (1, 1), pads: [0; 4], size: (3, 4) },
        // odd maps that cross the 256-pixel lowering panels, grouped, asymmetric pads
        Conv { cin: 6, cout: 10, groups: 2, k: (3, 3), stride: (1, 1), pads: [2, 1, 0, 1], size: (23, 29) },
    ];
    for (i, c) in cases.iter().enumerate() {
        let seed = i as u32;
        let wdims = [c.cout, c.cin / c.groups, c.k.0, c.k.1];
        let w = Seeded::new(seed + 100).values(wdims.iter().product(), 1.0);
        let b = Seeded::new(seed + 200).values(c.cout, 1.0);
        let x = seeded(&[1, c.cin, c.size.0, c.size.1], seed, 2.0);
        let attrs = [
            ("group", Attr::Int(c.groups as i64)),
            ("kernel_shape", Attr::Ints(vec![c.k.0 as i64, c.k.1 as i64])),
            ("strides", Attr::Ints(vec![c.stride.0 as i64, c.stride.1 as i64])),
            ("pads", Attr::Ints(c.pads.iter().map(|&p| p as i64).collect())),
        ];
        let model = one("Conv", &["w", "b"], vec![("w", Weight::f32(&wdims, w.clone())), ("b", Weight::f32(&[c.cout], b.clone()))], &attrs, &x.shape)
            .unwrap();
        let (shape, want) = conv_reference(c, &x.data, &w, &b);
        assert_close(&output(&model, &x), &shape, &want, &format!("conv case {i}"));
    }
}

#[test]
fn padding_stays_zero_when_one_run_reuses_the_lowering_scratch_for_another_layer() {
    // two grouped convolutions in one run share the im2col scratch; the second's padding must not
    // see the first's values, and a run on zeros must not leave any behind for the next run
    let c1 = Conv { cin: 3, cout: 6, groups: 1, k: (3, 3), stride: (1, 1), pads: [1; 4], size: (19, 17) };
    let c2 = Conv { cin: 6, cout: 4, groups: 2, k: (3, 3), stride: (1, 1), pads: [2, 0, 1, 2], size: (19, 17) };
    let (w1, w2) = (Seeded::new(1).values(6 * 3 * 9, 1.0), Seeded::new(2).values(4 * 3 * 9, 1.0));
    let (b1, b2) = (vec![0.0; 6], vec![0.0; 4]);
    let pads = |p: [usize; 4]| ("pads", Attr::Ints(p.iter().map(|&v| v as i64).collect()));
    let g = graph(
        vec![Node::new("Conv", &["x", "w1"], "h", &[pads(c1.pads)]), Node::new("Conv", &["h", "w2"], "y", &[("group", Attr::Int(2)), pads(c2.pads)])],
        vec![("w1", Weight::f32(&[6, 3, 3, 3], w1.clone())), ("w2", Weight::f32(&[4, 3, 3, 3], w2.clone()))],
        &[1, 3, 19, 17],
        &["y"],
    );
    let model = Model::from_bytes(&encode(&g)).unwrap();
    for zero in [false, true, false] {
        let x = if zero { Tensor::new(vec![1, 3, 19, 17], vec![0.0; 3 * 19 * 17]).unwrap() } else { seeded(&[1, 3, 19, 17], 9, 2.0) };
        let (_, h) = conv_reference(&c1, &x.data, &w1, &b1);
        let (shape, want) = conv_reference(&c2, &h, &w2, &b2);
        assert_close(&output(&model, &x), &shape, &want, &format!("two layers, zero input {zero}"));
    }
}

#[test]
fn batch_normalisation_equals_the_inference_formula_for_images_and_matrices() {
    for shape in [vec![1, 3, 2, 5], vec![2, 3]] {
        let x = seeded(&shape, 3, 4.0);
        let params = [vec![0.7, -1.1, 2.3], vec![-0.4, 0.6, 0.2], vec![0.5, -0.3, 0.9], vec![0.2, 0.0, 1.4]];
        let weights = ["scale", "bias", "mean", "var"].iter().zip(&params).map(|(n, v)| (*n, Weight::f32(&[3], v.clone()))).collect();
        let model = one("BatchNormalization", &["scale", "bias", "mean", "var"], weights, &[("epsilon", Attr::Float(1e-3))], &shape).unwrap();
        let plane: usize = shape.get(2..).unwrap_or(&[]).iter().product();
        let want: Vec<f32> = x
            .data
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let c = (i / plane) % 3;
                (v - params[2][c]) * params[0][c] / (params[3][c] + 1e-3).sqrt() + params[1][c]
            })
            .collect();
        assert_close(&output(&model, &x), &shape, &want, &format!("batch norm {shape:?}"));
    }
}

#[test]
fn prelu_broadcasts_per_channel_scalar_and_spatial_slopes() {
    // input [2, 3, 2, 2]: two images, three channels, a 2 × 2 plane
    let x = seeded(&[2, 3, 2, 2], 4, 4.0);
    for (dims, slopes) in [
        (vec![3, 1, 1], vec![0.1, 0.2, 0.3]),
        (vec![1, 3, 1, 1], vec![0.1, 0.2, 0.3]),
        (vec![1], vec![0.3]),
        (vec![2, 2], vec![0.1, 0.2, 0.3, 0.4]),
        (vec![3, 2, 2], (0..12).map(|i| i as f32 * 0.05).collect()),
    ] {
        let model = one("PRelu", &["s"], vec![("s", Weight::f32(&dims, slopes.clone()))], &[], &x.shape).unwrap();
        let want: Vec<f32> =
            x.data.iter().enumerate().map(|(i, &v)| if v < 0.0 { v * slopes[broadcast_index(i, &x.shape, &dims)] } else { v }).collect();
        assert_close(&output(&model, &x), &x.shape, &want, &format!("PRelu slope {dims:?}"));
    }
    // a slope that would enlarge its input is refused
    let err = one("PRelu", &["s"], vec![("s", Weight::f32(&[4], vec![0.1; 4]))], &[], &[1, 3, 2, 2]).unwrap_err();
    assert!(matches!(err, ModelError::Net(NetError::Shape(_))), "{err}");
}

#[test]
fn elementwise_operators_equal_a_direct_computation() {
    let x = seeded(&[1, 3, 4, 5], 5, 8.0);
    type Unary = fn(f32) -> f32;
    let unary: [(&str, Unary); 4] = [("Relu", |v| v.max(0.0)), ("Sigmoid", |v| 1.0 / (1.0 + (-v).exp())), ("Identity", |v| v), ("Dropout", |v| v)];
    for (op, f) in unary {
        let model = one(op, &[], vec![], &[], &x.shape).unwrap();
        let want: Vec<f32> = x.data.iter().map(|&v| f(v)).collect();
        assert_close(&output(&model, &x), &x.shape, &want, op);
    }
    // broadcasting against constants of several shapes, operand order kept
    for (op, f) in [("Add", (|a, b| a + b) as fn(f32, f32) -> f32), ("Sub", |a, b| a - b), ("Mul", |a, b| a * b)] {
        for dims in [vec![1], vec![5], vec![3, 1, 1], vec![1, 3, 4, 5], vec![4, 1]] {
            let c = Seeded::new(6).values(dims.iter().product(), 4.0);
            let model = one(op, &["c"], vec![("c", Weight::f32(&dims, c.clone()))], &[], &x.shape).unwrap();
            let want: Vec<f32> = x.data.iter().enumerate().map(|(i, &v)| f(v, c[broadcast_index(i, &x.shape, &dims)])).collect();
            assert_close(&output(&model, &x), &x.shape, &want, &format!("{op} {dims:?}"));
        }
        // the constant first: the output takes the broadcast shape
        let model = Model::from_bytes(&encode(&graph(
            vec![Node::new(op, &["c", "x"], "y", &[])],
            vec![("c", Weight::f32(&[3, 1], vec![0.5, 1.5, 2.5]))],
            &[1, 2],
            &["y"],
        )))
        .unwrap();
        let x2 = Tensor::new(vec![1, 2], vec![1.0, 2.0]).unwrap();
        let want: Vec<f32> = (0..6).map(|i| f([0.5, 1.5, 2.5][i / 2], [1.0, 2.0][i % 2])).collect();
        assert_close(&output(&model, &x2), &[3, 2], &want, &format!("{op} constant first"));
    }
}

#[test]
fn gemm_equals_a_direct_computation_with_transpositions_and_every_bias_shape() {
    for ta in [0, 1] {
        for tb in [0, 1] {
            for bias_dims in [vec![], vec![2], vec![1, 2], vec![2, 1], vec![2, 2]] {
                let (ashape, bshape) = (if ta == 0 { vec![2, 3] } else { vec![3, 2] }, if tb == 0 { vec![3, 2] } else { vec![2, 3] });
                let a = seeded(&ashape, 7, 2.0);
                let b = Seeded::new(8).values(6, 2.0);
                let bias = Seeded::new(9).values(bias_dims.iter().product(), 2.0);
                let mut weights = vec![("w", Weight::f32(&bshape, b.clone()))];
                let mut extra = vec!["w"];
                if !bias_dims.is_empty() {
                    weights.push(("b", Weight::f32(&bias_dims, bias.clone())));
                    extra.push("b");
                }
                let attrs = [("transA", Attr::Int(ta)), ("transB", Attr::Int(tb)), ("alpha", Attr::Float(0.7)), ("beta", Attr::Float(-0.4))];
                let model = one("Gemm", &extra, weights, &attrs, &ashape).unwrap();
                let mut want = vec![0f32; 4];
                for i in 0..2 {
                    for j in 0..2 {
                        let sum: f32 = (0..3)
                            .map(|k| a.data[if ta == 0 { i * 3 + k } else { k * 2 + i }] * b[if tb == 0 { k * 2 + j } else { j * 3 + k }])
                            .sum();
                        let c = if bias_dims.is_empty() { 0.0 } else { bias[broadcast_index(i * 2 + j, &[2, 2], &bias_dims)] };
                        want[i * 2 + j] = 0.7 * sum - 0.4 * c;
                    }
                }
                assert_close(&output(&model, &a), &[2, 2], &want, &format!("Gemm transA {ta} transB {tb} bias {bias_dims:?}"));
            }
        }
    }
}

#[test]
fn pooling_resizing_and_shape_operators_equal_a_direct_computation() {
    let x = seeded(&[1, 2, 6, 7], 10, 8.0);
    let at = |c: usize, y: usize, xx: usize| x.data[(c * 6 + y) * 7 + xx];
    // MaxPool 3 × 2, strides 2 × 3
    let model = one("MaxPool", &[], vec![], &[("kernel_shape", Attr::Ints(vec![3, 2])), ("strides", Attr::Ints(vec![2, 3]))], &x.shape).unwrap();
    let mut want = Vec::new();
    for c in 0..2 {
        for oy in 0..2 {
            for ox in 0..2 {
                want.push(
                    (0..3).flat_map(|ky| (0..2).map(move |kx| (ky, kx))).map(|(ky, kx)| at(c, oy * 2 + ky, ox * 3 + kx)).fold(f32::MIN, f32::max),
                );
            }
        }
    }
    assert_close(&output(&model, &x), &[1, 2, 2, 2], &want, "MaxPool");
    // GlobalAveragePool
    let model = one("GlobalAveragePool", &[], vec![], &[], &x.shape).unwrap();
    let want: Vec<f32> = x.data.chunks(42).map(|p| p.iter().sum::<f32>() / 42.0).collect();
    assert_close(&output(&model, &x), &[1, 2, 1, 1], &want, "GlobalAveragePool");
    // nearest Resize by 2 × 3, both coordinate modes it follows
    for (coord, nearest) in [("asymmetric", "floor"), ("half_pixel", "round_prefer_floor")] {
        let attrs = [
            ("mode", Attr::Str("nearest".into())),
            ("coordinate_transformation_mode", Attr::Str(coord.into())),
            ("nearest_mode", Attr::Str(nearest.into())),
        ];
        let model = one("Resize", &["", "s"], vec![("s", Weight::f32(&[4], vec![1.0, 1.0, 2.0, 3.0]))], &attrs, &x.shape).unwrap();
        let mut want = Vec::new();
        for c in 0..2 {
            for y in 0..12 {
                for xx in 0..21 {
                    want.push(at(c, y / 2, xx / 3));
                }
            }
        }
        assert_close(&output(&model, &x), &[1, 2, 12, 21], &want, &format!("Resize {coord}"));
    }
    // Transpose to NHWC, and the default (reversed) permutation
    let model = one("Transpose", &[], vec![], &[("perm", Attr::Ints(vec![0, 2, 3, 1]))], &x.shape).unwrap();
    let want: Vec<f32> = (0..6).flat_map(|y| (0..7).flat_map(move |xx| (0..2).map(move |c| (c, y, xx)))).map(|(c, y, xx)| at(c, y, xx)).collect();
    assert_close(&output(&model, &x), &[1, 6, 7, 2], &want, "Transpose NHWC");
    let m = seeded(&[2, 3], 11, 2.0);
    let model = one("Transpose", &[], vec![], &[], &m.shape).unwrap();
    let want: Vec<f32> = (0..3).flat_map(|j| (0..2).map(move |i| (i, j))).map(|(i, j)| m.data[i * 3 + j]).collect();
    assert_close(&output(&model, &m), &[3, 2], &want, "Transpose default");
    // Flatten (and a negative axis), Reshape (with -1 and 0)
    let model = one("Flatten", &[], vec![], &[("axis", Attr::Int(-1))], &x.shape).unwrap();
    assert_close(&output(&model, &x), &[12, 7], &x.data, "Flatten axis -1");
    let model = one("Flatten", &[], vec![], &[], &x.shape).unwrap();
    assert_close(&output(&model, &x), &[1, 84], &x.data, "Flatten");
    let model = one("Reshape", &["s"], vec![("s", Weight::i64(&[3], vec![0, -1, 3]))], &[], &x.shape).unwrap();
    assert_close(&output(&model, &x), &[1, 28, 3], &x.data, "Reshape");
}

#[test]
fn a_seeded_network_of_every_operator_reads_back_and_runs_the_same_every_time() {
    for seed in 0..4 {
        let g = synthetic::every_operator_graph(seed);
        let in_memory = Net::new(g.clone()).unwrap();
        let read_back = Model::from_bytes(&encode(&g)).unwrap();
        assert_eq!(read_back.net().graph(), &g, "the file holds the graph exactly");
        let x = seeded(&[1, 4, 8, 8], seed, 2.0);
        let (a, b) = (in_memory.run(x.clone()).unwrap(), read_back.run(x.clone()).unwrap());
        assert_eq!(a, b);
        assert_eq!(a, read_back.run(x).unwrap(), "deterministic");
        assert_eq!(a.iter().map(|(n, t)| (n.as_str(), t.shape.clone())).collect::<Vec<_>>(), [("y", vec![1, 5]), ("nhwc", vec![1, 4, 4, 8])]);
        assert_eq!(read_back.check_input(&[1, 4, 8, 8]).unwrap(), [vec![1, 5], vec![1, 4, 4, 8]]);
    }
}

#[test]
fn two_threads_running_one_model_agree() {
    let model = Arc::new(Model::from_bytes(&synthetic::recognition_model(3)).unwrap());
    let x = Tensor::new(vec![1, 3, 112, 112], (0..3 * 112 * 112).map(|i| (i * 37 % 256) as f32).collect()).unwrap();
    let alone = model.run(x.clone()).unwrap();
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (model, x) = (Arc::clone(&model), x.clone());
            std::thread::spawn(move || (0..3).map(|_| model.run(x.clone()).unwrap()).collect::<Vec<_>>())
        })
        .collect();
    for t in threads {
        for out in t.join().unwrap() {
            assert_eq!(out, alone);
        }
    }
    assert_eq!(alone[0].1.shape, vec![1, 8]);
}

/// A minimal valid model with one `Identity` node and the weight tensor `t`, for file-level cases.
fn identity_model(weights: &[Vec<u8>], node_extra: &[u8], graph_extra: &[u8], model_extra: &[u8], opset: u64) -> Vec<u8> {
    let mut n = Vec::new();
    for (id, v) in [(1, "x"), (2, "y"), (4, "Identity")] {
        len_field(id, v.as_bytes(), &mut n);
    }
    n.extend_from_slice(node_extra);
    let mut g = Vec::new();
    len_field(1, &n, &mut g);
    for t in weights {
        len_field(5, t, &mut g);
    }
    len_field(11, &value_info_bytes("x", 1, &[Ok(1), Ok(3), Ok(4), Ok(4)]), &mut g);
    len_field(12, &value_info_bytes("y", 1, &[Ok(1), Ok(3), Ok(4), Ok(4)]), &mut g);
    g.extend_from_slice(graph_extra);
    let mut ops = Vec::new();
    varint_field(2, opset, &mut ops);
    let mut out = Vec::new();
    len_field(7, &g, &mut out);
    len_field(8, &ops, &mut out);
    out.extend_from_slice(model_extra);
    out
}

fn raw_weight(dtype: u64, dims: &[u64], raw: &[u8]) -> Vec<u8> {
    let mut t = Vec::new();
    for d in dims {
        varint_field(1, *d, &mut t);
    }
    varint_field(2, dtype, &mut t);
    len_field(8, b"w", &mut t);
    len_field(9, raw, &mut t);
    t
}

fn load(bytes: &[u8]) -> Result<Model, ModelError> {
    Model::from_bytes(bytes)
}

/// The reason a load failed, as the user reads it.
fn reason(bytes: &[u8]) -> String {
    match load(bytes) {
        Ok(_) => panic!("a model that should be refused loaded"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn malformed_files_fail_to_load_with_their_reason() {
    let one_float = raw_weight(1, &[1], &1f32.to_le_bytes());
    assert!(load(&identity_model(std::slice::from_ref(&one_float), &[], &[], &[], 13)).is_ok(), "the base case loads");
    let mut lying = Vec::new();
    varint(7 << 3 | 2, &mut lying);
    varint(1 << 40, &mut lying); // a graph 1 TB long in a 10-byte file
    let mut overflow = Vec::new();
    varint(7 << 3 | 2, &mut overflow);
    varint(u64::MAX, &mut overflow);
    let mut endless = vec![0x08];
    endless.extend_from_slice(&[0xff; 11]);
    let mut long_name = Vec::new();
    len_field(1, &[b'x'; 4097], &mut long_name);
    let mut not_utf8 = Vec::new();
    len_field(1, &[0xff], &mut not_utf8);
    let mut repeated_attribute = Vec::new();
    for _ in 0..2 {
        let mut a = Vec::new();
        len_field(1, b"ratio", &mut a);
        varint_field(20, 1, &mut a);
        len_field(5, &a, &mut repeated_attribute);
    }
    let mut no_nodes = Vec::new();
    len_field(7, &[], &mut no_nodes);
    let node_free = {
        let mut g = Vec::new();
        len_field(11, &value_info_bytes("x", 1, &[Ok(1)]), &mut g);
        len_field(12, &value_info_bytes("y", 1, &[Ok(1)]), &mut g);
        let mut ops = Vec::new();
        varint_field(2, 13, &mut ops);
        let mut m = Vec::new();
        len_field(7, &g, &mut m);
        len_field(8, &ops, &mut m);
        m
    };
    let mut two_floats = raw_weight(1, &[1], &1f32.to_le_bytes());
    varint(4 << 3 | 5, &mut two_floats);
    two_floats.extend_from_slice(&1f32.to_le_bytes());
    let mut two_raw = raw_weight(1, &[1], &1f32.to_le_bytes());
    len_field(9, &1f32.to_le_bytes(), &mut two_raw);
    for (what, bytes, says) in [
        ("empty", Vec::new(), "empty"),
        ("zeros", vec![0u8; 64], "field number"),
        ("text", b"this is not an onnx model at all".to_vec(), ""),
        ("a lying length", lying, "runs past"),
        ("an overflowing length", overflow, "overflow"),
        ("an endless number", endless, "number"),
        ("no graph", synthetic::embedder_model(8)[..4].to_vec(), ""),
        ("a graph of no nodes", node_free, "no nodes"),
        ("a graph without inputs or outputs", no_nodes, "no inputs or no outputs"),
        ("a name over 4096 bytes", identity_model(&[], &long_name, &[], &[], 13), "4096"),
        ("a name that is not UTF-8", identity_model(&[], &not_utf8, &[], &[], 13), "UTF-8"),
        ("a repeated attribute", identity_model(&[], &repeated_attribute, &[], &[], 13), "repeated"),
        ("a weight shorter than its shape", identity_model(&[raw_weight(1, &[2], &[0; 4])], &[], &[], &[], 13), "disagree"),
        ("a weight longer than its shape", identity_model(&[raw_weight(1, &[1], &[0; 8])], &[], &[], &[], 13), "disagree"),
        ("a weight of three bytes", identity_model(&[raw_weight(1, &[1], &[0; 3])], &[], &[], &[], 13), "disagree"),
        ("raw and typed values", identity_model(&[two_floats], &[], &[], &[], 13), "both raw and typed"),
        ("two raw values", identity_model(&[two_raw], &[], &[], &[], 13), "repeated or excessive"),
        ("a weight defined twice", identity_model(&[one_float.clone(), one_float.clone()], &[], &[], &[], 13), "twice"),
    ] {
        let got = reason(&bytes);
        assert!(got.contains(says), "{what}: {got}");
        assert!(got.starts_with("not a usable ONNX model"), "{what} is malformed: {got}");
    }
}

#[test]
fn unsupported_models_are_refused_by_name() {
    let w = raw_weight(1, &[1], &1f32.to_le_bytes());
    let mut external = w.clone();
    len_field(13, b"location", &mut external);
    let mut located = w.clone();
    varint_field(14, 1, &mut located);
    let mut sparse = Vec::new();
    len_field(15, b"sparse", &mut sparse);
    let mut custom = Vec::new();
    len_field(7, b"vendor.example", &mut custom);
    let mut training_domain = Vec::new();
    len_field(7, b"ai.onnx.preview.training", &mut training_domain);
    let mut training_info = Vec::new();
    len_field(20, b"training", &mut training_info);
    let mut half_input = Vec::new();
    len_field(11, &value_info_bytes("h", 10, &[Ok(1)]), &mut half_input);
    let file_cases = [
        ("external weights", identity_model(&[external], &[], &[], &[], 13), "outside the model file"),
        ("weights at an external location", identity_model(&[located], &[], &[], &[], 13), "outside the model file"),
        ("sparse weights", identity_model(&[], &[], &sparse, &[], 13), "sparse weights"),
        ("a custom domain", identity_model(&[], &custom, &[], &[], 13), "vendor.example"),
        ("a training operator", identity_model(&[], &training_domain, &[], &[], 13), "training graphs"),
        ("a training graph", identity_model(&[], &[], &[], &training_info, 13), "training graphs"),
        ("a float64 weight", identity_model(&[raw_weight(11, &[1], &[0; 8])], &[], &[], &[], 13), "float64"),
        ("a float16 input", identity_model(&[], &[], &half_input, &[], 13), "float16"),
        ("opset 18", identity_model(&[], &[], &[], &[], 18), "opset 18"),
        ("opset 9", identity_model(&[], &[], &[], &[], 9), "opset 9"),
    ];
    for (what, bytes, says) in file_cases {
        let err = load(&bytes).unwrap_err();
        assert!(matches!(err, ModelError::Load(LoadError::Unsupported(_))), "{what}: {err}");
        assert!(err.to_string().contains(says), "{what}: {err}");
    }
    let one_by_one = || ("w", Weight::f32(&[3, 1, 1, 1], vec![1.0; 3]));
    let net_cases = [
        ("Softmax", one("Softmax", &[], vec![], &[], &[1, 3, 4, 4]), "`Softmax`"),
        ("a loop", one("Loop", &[], vec![], &[], &[1, 3, 4, 4]), "`Loop`"),
        ("a dilated convolution", one("Conv", &["w"], vec![one_by_one()], &[("dilations", Attr::Ints(vec![2, 2]))], &[1, 3, 4, 4]), "dilation"),
        ("automatic padding", one("Conv", &["w"], vec![one_by_one()], &[("auto_pad", Attr::Str("SAME_UPPER".into()))], &[1, 3, 4, 4]), "padding"),
        ("an unknown attribute", one("Relu", &[], vec![], &[("alpha", Attr::Float(0.1))], &[1, 3, 4, 4]), "`alpha`"),
        (
            "a training batch norm",
            {
                let ws = ["s", "b", "m", "v"].map(|n| (n, Weight::f32(&[3], vec![1.0; 3])));
                one("BatchNormalization", &["s", "b", "m", "v"], ws.into_iter().collect(), &[("training_mode", Attr::Int(1))], &[1, 3, 4, 4])
            },
            "training",
        ),
        (
            "a linear resize",
            one(
                "Resize",
                &["", "s"],
                vec![("s", Weight::f32(&[4], vec![1.0, 1.0, 2.0, 2.0]))],
                &[("mode", Attr::Str("linear".into()))],
                &[1, 3, 4, 4],
            ),
            "nearest",
        ),
        ("a fractional resize", one("Resize", &["", "s"], vec![("s", Weight::f32(&[4], vec![1.0, 1.0, 1.5, 1.5]))], &[], &[1, 3, 4, 4]), "whole"),
        (
            "a padded max pool",
            one("MaxPool", &[], vec![], &[("kernel_shape", Attr::Ints(vec![2, 2])), ("pads", Attr::Ints(vec![1; 4]))], &[1, 3, 4, 4]),
            "MaxPool",
        ),
        (
            "a computed reshape",
            Model::from_bytes(&encode(&graph(vec![Node::new("Reshape", &["x", "x"], "y", &[])], vec![], &[1, 3, 4, 4], &["y"]))),
            "constant",
        ),
        ("an integer activation", one("Add", &["w"], vec![("w", Weight::i64(&[1], vec![1]))], &[], &[1, 3, 4, 4]), "float32"),
    ];
    for (what, result, says) in net_cases {
        let err = result.unwrap_err();
        assert!(matches!(err, ModelError::Net(NetError::Unsupported(_))), "{what}: {err}");
        assert!(err.to_string().contains(says), "{what}: {err}");
    }
}

#[test]
fn inconsistent_graphs_and_hostile_attributes_fail_to_load() {
    let w = |dims: &[usize]| ("w", Weight::f32(dims, vec![0.1; dims.iter().product()]));
    for (what, result) in [
        ("a convolution without weights", one("Conv", &["w"], vec![], &[], &[1, 3, 4, 4])),
        ("channels that do not match", one("Conv", &["w"], vec![w(&[1, 5, 1, 1])], &[], &[1, 3, 4, 4])),
        ("a kernel larger than the input", one("Conv", &["w"], vec![w(&[1, 3, 7, 7])], &[], &[1, 3, 4, 4])),
        (
            "a kernel_shape unlike its weights",
            one("Conv", &["w"], vec![w(&[3, 3, 1, 1])], &[("kernel_shape", Attr::Ints(vec![3, 3]))], &[1, 3, 4, 4]),
        ),
        ("a zero group", one("Conv", &["w"], vec![w(&[3, 3, 1, 1])], &[("group", Attr::Int(0))], &[1, 3, 4, 4])),
        ("a zero stride", one("Conv", &["w"], vec![w(&[3, 3, 1, 1])], &[("strides", Attr::Ints(vec![0, 1]))], &[1, 3, 4, 4])),
        ("a negative pad", one("Conv", &["w"], vec![w(&[3, 3, 1, 1])], &[("pads", Attr::Ints(vec![-1, 0, 0, 0]))], &[1, 3, 4, 4])),
        ("a huge pad", one("Conv", &["w"], vec![w(&[3, 3, 1, 1])], &[("pads", Attr::Ints(vec![i64::MAX; 4]))], &[1, 3, 4, 4])),
        ("a mistyped attribute", one("Conv", &["w"], vec![w(&[3, 3, 1, 1])], &[("strides", Attr::Float(1.0))], &[1, 3, 4, 4])),
        ("a repeated permutation axis", one("Transpose", &[], vec![], &[("perm", Attr::Ints(vec![0, 0, 1, 2]))], &[1, 3, 4, 4])),
        ("a reshape that changes the count", one("Reshape", &["s"], vec![("s", Weight::i64(&[2], vec![4, 4]))], &[], &[1, 3, 4, 4])),
        ("a huge reshape", one("Reshape", &["s"], vec![("s", Weight::i64(&[2], vec![i64::MAX, 1]))], &[], &[1, 3, 4, 4])),
        ("a flatten axis past the rank", one("Flatten", &[], vec![], &[("axis", Attr::Int(i64::MIN))], &[1, 3, 4, 4])),
        ("a dropout ratio of one", one("Dropout", &[], vec![], &[("ratio", Attr::Float(1.0))], &[1, 3, 4, 4])),
        ("a negative variance", {
            let ws = [("s", 1.0), ("b", 0.0), ("m", 0.0), ("v", -1.0)].map(|(n, v)| (n, Weight::f32(&[3], vec![v; 3])));
            one("BatchNormalization", &["s", "b", "m", "v"], ws.into_iter().collect(), &[], &[1, 3, 4, 4])
        }),
        ("an add of an unknown value", one("Add", &["c"], vec![], &[], &[1, 3, 4, 4])),
        ("an empty broadcast operand", one("Mul", &["w"], vec![("w", Weight::f32(&[0, 3, 1, 1], vec![]))], &[], &[1, 3, 4, 4])),
        ("a gemm whose sizes differ", one("Gemm", &["w"], vec![w(&[4, 2])], &[], &[2, 3])),
    ] {
        assert!(result.is_err(), "{what}");
    }
    // a value read before it is made, a value made twice, and a constant output
    let relu = |input: &str, output: &str| Node::new("Relu", &[input], output, &[]);
    for (what, g) in [
        ("a forward read", graph(vec![relu("later", "y"), relu("x", "later")], vec![], &[1, 3], &["y"])),
        ("a value made twice", graph(vec![relu("x", "y"), relu("x", "y")], vec![], &[1, 3], &["y"])),
        ("a constant output", graph(vec![relu("x", "y")], vec![w(&[1])], &[1, 3], &["w"])),
        ("an output never made", graph(vec![relu("x", "y")], vec![], &[1, 3], &["z"])),
    ] {
        assert!(Net::new(g).is_err(), "{what}");
    }
}

#[test]
fn truncated_and_corrupted_files_never_panic() {
    let good = synthetic::recognition_model(1);
    assert!(load(&good).is_ok());
    for n in 0..good.len() {
        assert!(load(&good[..n]).is_err(), "a file cut at {n} of {} bytes loads", good.len());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn any_bytes_are_refused_or_loaded_and_run_without_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        if let Ok(model) = load(&bytes) {
            let shape: Vec<usize> = model.input_shape().iter().map(|&d| d.max(1)).collect();
            if let Some(n) = Some(shape.iter().product::<usize>()).filter(|&n| n > 0 && n <= 1 << 16) {
                let _ = Tensor::new(shape, vec![0.5; n]).map(|x| model.run(x));
            }
        }
    }

    #[test]
    fn a_model_with_changed_bytes_is_refused_or_runs_without_a_panic(flips in proptest::collection::vec((0usize..4096, any::<u8>()), 1..6)) {
        let mut bytes = synthetic::recognition_model(2);
        let len = bytes.len();
        for (at, v) in flips {
            if let Some(b) = bytes.get_mut(at % len) {
                *b = v;
            }
        }
        if let Ok(model) = load(&bytes) {
            let input = Tensor::new(vec![1, 3, 112, 112], vec![100.0; 3 * 112 * 112]).unwrap();
            let _ = model.run(input);
        }
    }
}

/// A source that claims to be one byte over the limit and must never be read.
struct Oversized;

impl Read for Oversized {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        panic!("an oversized model must not be read")
    }
}

impl Seek for Oversized {
    fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
        Ok(MAX_MODEL_BYTES + 1)
    }
}

#[test]
fn oversized_models_fail_to_load_before_anything_is_read_or_run() {
    // a file over 512 MiB, unread
    assert!(Model::read(&mut Oversized).unwrap_err().to_string().contains("512 MiB"));
    assert!(probe_reader(&mut Oversized).is_err());
    // a file read through `read` loads like its bytes
    assert!(Model::read(&mut Cursor::new(synthetic::recognition_model(4))).is_ok());
    // weights over 64 Mi elements, declared in a tiny file
    let too_many = identity_model(&[raw_weight(1, &[8192, 8193], &[])], &[], &[], &[], 13);
    assert!(reason(&too_many).contains("64 Mi"));
    // a weight or a declared input of rank 9
    assert!(reason(&identity_model(&[raw_weight(1, &[1; 9], &[0; 4])], &[], &[], &[], 13)).contains("8 dimensions"));
    let mut rank9 = Vec::new();
    len_field(11, &value_info_bytes("big", 1, &[Ok(1); 9]), &mut rank9);
    assert!(reason(&identity_model(&[], &[], &rank9, &[], 13)).contains("8 dimensions"));
    // live tensors and scratch over 512 MiB for the declared input: refused at load, nothing allocated
    let wide = one("Conv", &["w"], vec![("w", Weight::f32(&[4096, 3, 1, 1], vec![0.1; 4096 * 3]))], &[], &[1, 3, 4096, 4096]).unwrap_err();
    assert!(wide.to_string().contains("limit"), "{wide}");
    // the same for an input given at run time to a model with a dynamic size
    let dynamic = one("Conv", &["w"], vec![("w", Weight::f32(&[4096, 3, 1, 1], vec![0.1; 4096 * 3]))], &[], &[1, 3, 0, 0]).unwrap();
    assert!(dynamic.check_input(&[1, 3, 4096, 4096]).is_err());
    assert!(dynamic.check_input(&[1, 3, 8, 8]).is_ok());
    // scratch kept across layers counts: each layer fits alone, both peaks together do not
    let network = |output_channels: usize| {
        let pads = [("pads", Attr::Ints(vec![1, 0, 0, 0]))];
        Net::new(graph(
            vec![Node::new("Conv", &["x", "w"], "y", &pads), Node::new("Conv", &["y", "v"], "z", &pads)],
            vec![
                ("w", Weight::f32(&[2, 130_000, 2, 1], vec![0.0; 2 * 130_000 * 2])),
                ("v", Weight::f32(&[output_channels, 2, 2, 1], vec![0.0; output_channels * 4])),
            ],
            &[1, 130_000, 2, 256],
            &["z"],
        ))
        .unwrap()
    };
    assert!(network(131_072).check_input(&[1, 130_000, 2, 256]).is_err());
    assert!(network(65_536).check_input(&[1, 130_000, 2, 256]).is_ok());
}

#[test]
fn a_weight_or_value_that_is_not_finite_is_an_error() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let err = one("PRelu", &["w"], vec![("w", Weight::f32(&[1], vec![value]))], &[], &[1, 3]).unwrap_err();
        assert!(matches!(err, ModelError::Net(NetError::NotFinite(_))), "{err}");
    }
    // an activation that overflows, and the next run of the same model is fine
    let model = one("Mul", &["w"], vec![("w", Weight::f32(&[1], vec![f32::MAX]))], &[], &[1, 2]).unwrap();
    let err = model.run(Tensor::new(vec![1, 2], vec![2.0, 0.0]).unwrap()).unwrap_err();
    assert!(matches!(err, ModelError::Net(NetError::NotFinite(_))), "{err}");
    assert!(model.run(Tensor::new(vec![1, 2], vec![0.5, 0.0]).unwrap()).is_ok());
    // batch normalisation coefficients that overflow are refused before running
    let ws = [("s", f32::MAX), ("b", 0.0), ("m", f32::MAX), ("v", 0.0)].map(|(n, v)| (n, Weight::f32(&[3], vec![v; 3])));
    assert!(one("BatchNormalization", &["s", "b", "m", "v"], ws.into_iter().collect(), &[], &[1, 3, 2, 2]).is_err());
    // an input that is not finite, or does not fill its shape, is refused
    let relu = one("Relu", &[], vec![], &[], &[0, 0]).unwrap();
    assert!(Tensor::new(vec![1], vec![f32::NAN]).is_err());
    for input in [
        Tensor { shape: vec![1], data: vec![f32::NAN] },
        Tensor { shape: vec![usize::MAX, 2], data: vec![] },
        Tensor { shape: vec![0, 3], data: vec![] },
        Tensor { shape: vec![1; 9], data: vec![1.0] },
        Tensor { shape: vec![2], data: vec![1.0] },
    ] {
        assert!(relu.run(input).is_err());
    }
}
