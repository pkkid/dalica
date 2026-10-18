//! A small, single-threaded float32 CPU interpreter for the convolutional networks of the face
//! models (`faces.md` §2): the detector and the recogniser run on it.
//!
//! It runs checked convolution, affine and activation, broadcasting, pooling, matrix and shape
//! operators: Conv (with depthwise and other groups), BatchNormalization, PRelu, Relu, Sigmoid, Add,
//! Sub, Mul, Dropout (as identity), Flatten, Gemm, MaxPool, nearest whole-number Resize, Transpose,
//! constant-shape Reshape, GlobalAveragePool and Identity. Anything else, and any attribute it
//! does not follow, fails in [`Net::new`]; every shape and the working memory are checked for an
//! input before it runs ([`Net::check_input`]). Dense convolutions use a bounded im2col and
//! `matrixmultiply`'s GEMM through `ndarray`'s safe views; depthwise convolutions use row-wise
//! loops. It starts no thread of its own, and a [`Net`] is shared between threads by reference: a
//! run keeps all its state on its own stack.
//!
//! Ported from LightCraft `crates/faces/src/net.rs` @ f34c7a8a (MIT OR Apache-2.0), without its
//! GPU accelerator (`faces.md` §2: inference is on the CPU; `net/program.rs`, which only lowers a
//! net for that accelerator, is not ported).

use std::collections::HashMap;
use std::rc::Rc;

mod kernels;
mod shape;

use crate::graph::{Data, Graph, Node};
use crate::limits::{MAX_RANK, MAX_RESIZE_FACTOR, MAX_TENSOR_ELEMENTS};

/// Why a network cannot be built or run.
#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub enum NetError {
    /// An operator, attribute or form the interpreter refuses by name.
    #[error("this model uses something Dalica does not support: {0}")]
    Unsupported(String),
    /// Shapes that do not fit together, or a limit a shape exceeds.
    #[error("the model does not fit together: {0}")]
    Shape(String),
    /// A weight, an input or a computed value that is not a finite number (an overflow).
    #[error("the model gave a value that is not a finite number: {0}")]
    NotFinite(String),
}

type Result<T> = std::result::Result<T, NetError>;

fn shape_err<T>(why: impl Into<String>) -> Result<T> {
    Err(NetError::Shape(why.into()))
}

fn unsupported<T>(why: impl Into<String>) -> Result<T> {
    Err(NetError::Unsupported(why.into()))
}

/// A float32 tensor: its shape (at most rank 8, no zero dimension) and its values in row-major order.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl Tensor {
    /// A tensor whose values fill `shape` exactly, all finite, within the element limit.
    pub fn new(shape: Vec<usize>, data: Vec<f32>) -> Result<Tensor> {
        let n = shape.iter().try_fold(1usize, |a, d| a.checked_mul(*d));
        if shape.len() > MAX_RANK || shape.contains(&0) || n.is_none_or(|n| n > MAX_TENSOR_ELEMENTS) || n != Some(data.len()) {
            return shape_err(format!("{} values do not fill the shape {shape:?}", data.len()));
        }
        if data.iter().any(|v| !v.is_finite()) {
            return Err(NetError::NotFinite("the input".into()));
        }
        Ok(Tensor { shape, data })
    }

    fn zeros(shape: Vec<usize>) -> Result<Tensor> {
        match shape.iter().try_fold(1usize, |a, d| a.checked_mul(*d)) {
            Some(n) if n > 0 && n <= MAX_TENSOR_ELEMENTS && shape.len() <= MAX_RANK && !shape.contains(&0) => {
                let mut data = Vec::new();
                data.try_reserve_exact(n).map_err(|_| NetError::Shape("not enough memory for a model tensor".into()))?;
                data.resize(n, 0.0);
                Ok(Tensor { shape, data })
            }
            _ => shape_err("a tensor would be too large"),
        }
    }
}

/// A checked network, ready to run.
#[derive(Debug)]
pub struct Net {
    graph: Graph,
}

impl Net {
    /// Checks every operator, attribute, weight and name of `graph`; nothing unsupported gets past.
    pub fn new(graph: Graph) -> Result<Net> {
        shape::check_graph(&graph)?;
        for n in &graph.nodes {
            shape::check(n, &graph)?;
        }
        Ok(Net { graph })
    }

    /// The graph it runs.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// The shape the model declares for its input (0 for a dynamic size).
    pub fn input_shape(&self) -> Option<&[usize]> {
        self.graph.inputs.first().map(|(_, s)| s.as_slice())
    }

    /// Infers and checks every intermediate shape for an input of `shape` without allocating any
    /// values: each tensor within the element limit, and live tensors, constant copies and
    /// convolution scratch within the working limit at every step. Returns the outputs' shapes.
    pub fn check_input(&self, shape: &[usize]) -> Result<Vec<Vec<usize>>> {
        shape::infer(&self.graph, shape)
    }

    /// Runs the network on `input` (its single input); returns each output by name. A value that
    /// is not finite, in the input or computed by any operator, is an error.
    pub fn run(&self, input: Tensor) -> Result<Vec<(String, Tensor)>> {
        shape::tensor(&input.shape, input.data.len(), &input.data)?;
        self.check_input(&input.shape)?;
        let mut scratch = kernels::Scratch::default();
        let g = &self.graph;
        let Some((input_name, _)) = g.inputs.first() else { return shape_err("the model has no input") };
        // when each value is last read, so big intermediates are dropped as soon as nothing needs them
        let mut last_use: HashMap<&str, usize> = HashMap::new();
        for (i, n) in g.nodes.iter().enumerate() {
            for name in &n.inputs {
                last_use.insert(name.as_str(), i);
            }
        }
        for o in &g.outputs {
            last_use.insert(o.as_str(), usize::MAX);
        }
        let mut values: HashMap<String, Rc<Tensor>> = HashMap::new();
        values.insert(input_name.clone(), Rc::new(input));
        for (i, node) in g.nodes.iter().enumerate() {
            let get = |idx: usize| -> Result<Rc<Tensor>> {
                let name = node.inputs.get(idx).ok_or_else(|| NetError::Shape(format!("{} is missing an input", node.op)))?;
                if let Some(value) = values.get(name) {
                    return Ok(value.clone());
                }
                match g.weights.get(name) {
                    Some(crate::graph::Weight { dims, data: Data::F32(data) }) => Ok(Rc::new(Tensor::new(dims.clone(), data.clone())?)),
                    _ => shape_err(format!("`{name}` is read before it is computed")),
                }
            };
            let out = match node.op.as_str() {
                "Conv" => kernels::conv(node, &*get(0)?, g, &mut scratch)?,
                "Relu" => map(&*get(0)?, |v| v.max(0.0)),
                "Sigmoid" => map(&*get(0)?, |v| 1.0 / (1.0 + (-v).exp())),
                "Add" | "Sub" | "Mul" => kernels::binary(node.op.as_str(), &*get(0)?, &*get(1)?)?,
                "PRelu" => kernels::prelu(node, &*get(0)?, g)?,
                "BatchNormalization" => kernels::batch_norm(node, &*get(0)?, g)?,
                "Gemm" => kernels::gemm(node, &*get(0)?, g)?,
                "Flatten" => kernels::flatten(node, &*get(0)?)?,
                "Dropout" | "Identity" => (*get(0)?).clone(),
                "GlobalAveragePool" => kernels::global_average(&*get(0)?)?,
                "MaxPool" => max_pool(node, &*get(0)?)?,
                "Resize" => resize(node, &*get(0)?, g)?,
                "Transpose" => transpose(node, &*get(0)?)?,
                "Reshape" => reshape(node, &*get(0)?, g)?,
                other => return unsupported(format!("the operator `{other}`")),
            };
            if out.data.iter().any(|v| !v.is_finite()) {
                return Err(NetError::NotFinite(format!("{} (node {i})", node.op)));
            }
            let Some(name) = node.outputs.first() else { return shape_err("a node has no output") };
            values.insert(name.clone(), Rc::new(out));
            for name in &node.inputs {
                if last_use.get(name.as_str()) == Some(&i) {
                    values.remove(name);
                }
            }
        }
        g.outputs
            .iter()
            .map(|name| {
                let t = values.remove(name).ok_or_else(|| NetError::Shape(format!("the output `{name}` was never computed")))?;
                // an output listed twice was refused by `check_graph`, so this is the only reference
                let t = Rc::try_unwrap(t).unwrap_or_else(|shared| (*shared).clone());
                Ok((name.clone(), t))
            })
            .collect()
    }
}

fn map(x: &Tensor, f: impl Fn(f32) -> f32) -> Tensor {
    Tensor { shape: x.shape.clone(), data: x.data.iter().map(|v| f(*v)).collect() }
}

/// `[1, c, h, w]` → `(c, h, w)`.
fn chw(x: &Tensor) -> Result<(usize, usize, usize)> {
    match x.shape.as_slice() {
        [1, c, h, w] => Ok((*c, *h, *w)),
        s => unsupported(format!("a tensor of shape {s:?} (only [1, channels, height, width])")),
    }
}

/// `pads` of an ONNX node as (top, left, bottom, right).
fn pads(node: &Node) -> Result<(usize, usize, usize, usize)> {
    match node.ints("pads") {
        None => Ok((0, 0, 0, 0)),
        Some([t, l, b, r]) => {
            let u = |v: &i64| usize::try_from(*v).map_err(|_| NetError::Shape("a negative pad".into()));
            Ok((u(t)?, u(l)?, u(b)?, u(r)?))
        }
        Some(_) => unsupported("pads that are not [top, left, bottom, right]"),
    }
}

fn pair(node: &Node, name: &str, default: usize) -> Result<(usize, usize)> {
    match node.ints(name) {
        None => Ok((default, default)),
        Some([a, b]) => match (usize::try_from(*a), usize::try_from(*b)) {
            (Ok(a), Ok(b)) if a >= 1 && b >= 1 => Ok((a, b)),
            _ => shape_err(format!("`{name}` must be positive")),
        },
        Some(_) => unsupported(format!("`{name}` that is not two numbers")),
    }
}

/// A convolution with one input channel per group (depthwise, or a single-channel input) by
/// row-wise loops: no lowering is worth it.
fn conv_direct(node: &Node, x: &Tensor, g: &Graph) -> Result<Tensor> {
    let (cin, h, w) = chw(x)?;
    let weight = node.inputs.get(1).and_then(|n| g.weights.get(n)).ok_or_else(|| NetError::Shape("a Conv lost its weights".into()))?;
    let Data::F32(wd) = &weight.data else { return unsupported("non-float Conv weights") };
    let [cout, cin_g, kh, kw] = weight.dims.as_slice() else { return unsupported("a Conv that is not 2-D") };
    let (cout, cin_g, kh, kw) = (*cout, *cin_g, *kh, *kw);
    let groups =
        usize::try_from(node.int("group").unwrap_or(1)).ok().filter(|g| *g >= 1).ok_or_else(|| NetError::Shape("a bad Conv group".into()))?;
    if groups.checked_mul(cin_g) != Some(cin) || !cout.is_multiple_of(groups) || kh == 0 || kw == 0 {
        return shape_err("the Conv weights do not match its input");
    }
    let bias: Option<&[f32]> = match node.inputs.get(2).filter(|b| !b.is_empty()).and_then(|b| g.weights.get(b)) {
        Some(b) => match &b.data {
            Data::F32(v) if v.len() == cout => Some(v),
            _ => return shape_err("the Conv bias does not match"),
        },
        None => None,
    };
    let (sh, sw) = pair(node, "strides", 1)?;
    let (pt, pl, pb, pr) = pads(node)?;
    let ph = h.checked_add(pt).and_then(|v| v.checked_add(pb)).ok_or_else(|| NetError::Shape("Conv padding overflow".into()))?;
    let pw = w.checked_add(pl).and_then(|v| v.checked_add(pr)).ok_or_else(|| NetError::Shape("Conv padding overflow".into()))?;
    if ph < kh || pw < kw {
        return shape_err("the Conv kernel is larger than its input");
    }
    let (oh, ow) = ((ph - kh) / sh + 1, (pw - kw) / sw + 1);
    let mut out = Tensor::zeros(vec![1, cout, oh, ow])?;
    let plane = oh * ow;
    let in_planes: Vec<&[f32]> = x.data.chunks_exact((h * w).max(1)).collect();
    let cout_g = cout / groups;
    for (co, out_plane) in out.data.chunks_exact_mut(plane.max(1)).enumerate() {
        if let Some(b) = bias.and_then(|b| b.get(co)) {
            out_plane.iter_mut().for_each(|o| *o = *b);
        }
        let group = co / cout_g;
        for ci_l in 0..cin_g {
            let Some(in_plane) = in_planes.get(group * cin_g + ci_l) else { return shape_err("a Conv input channel is missing") };
            let base = (co * cin_g + ci_l) * kh * kw;
            let Some(kernel) = wd.get(base..base + kh * kw) else { return shape_err("the Conv weights are too short") };
            for (k, wv) in kernel.iter().enumerate() {
                if *wv == 0.0 {
                    continue;
                }
                let (ky, kx) = (k / kw, k % kw);
                for oy in 0..oh {
                    // the input row for this output row and kernel row (padded coordinates minus the pad)
                    let Some(iy) = (oy * sh + ky).checked_sub(pt).filter(|iy| *iy < h) else { continue };
                    let in_row = in_plane.get(iy * w..(iy + 1) * w).unwrap_or(&[]);
                    let out_row = out_plane.get_mut(oy * ow..(oy + 1) * ow).unwrap_or(&mut []);
                    if sw == 1 {
                        // ox runs over outputs whose input column ox + kx - pl lies inside the row
                        let lo = pl.saturating_sub(kx);
                        let hi = ow.min((w + pl).saturating_sub(kx));
                        if lo < hi {
                            let src = in_row.get(lo + kx - pl..hi + kx - pl).unwrap_or(&[]);
                            let dst = out_row.get_mut(lo..hi).unwrap_or(&mut []);
                            dst.iter_mut().zip(src).for_each(|(o, i)| *o += wv * i);
                        }
                    } else {
                        for (ox, o) in out_row.iter_mut().enumerate() {
                            if let Some(ix) = (ox * sw + kx).checked_sub(pl).filter(|ix| *ix < w)
                                && let Some(i) = in_row.get(ix)
                            {
                                *o += wv * i;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

fn max_pool(node: &Node, x: &Tensor) -> Result<Tensor> {
    let (c, h, w) = chw(x)?;
    let (kh, kw) = pair(node, "kernel_shape", 1)?;
    let (sh, sw) = pair(node, "strides", 1)?;
    if pads(node)? != (0, 0, 0, 0) {
        return unsupported("a padded MaxPool");
    }
    if h < kh || w < kw {
        return shape_err("the MaxPool window is larger than its input");
    }
    let (oh, ow) = ((h - kh) / sh + 1, (w - kw) / sw + 1);
    let mut out = Tensor::zeros(vec![1, c, oh, ow])?;
    for (plane_in, plane_out) in x.data.chunks_exact((h * w).max(1)).zip(out.data.chunks_exact_mut((oh * ow).max(1))) {
        for oy in 0..oh {
            for ox in 0..ow {
                let mut m = f32::NEG_INFINITY;
                for ky in 0..kh {
                    let row = plane_in.get((oy * sh + ky) * w..(oy * sh + ky + 1) * w).unwrap_or(&[]);
                    for kx in 0..kw {
                        if let Some(v) = row.get(ox * sw + kx) {
                            m = m.max(*v);
                        }
                    }
                }
                if let Some(o) = plane_out.get_mut(oy * ow + ox) {
                    *o = m;
                }
            }
        }
    }
    Ok(out)
}

/// A whole-number factor of a nearest `Resize`, from 1 to [`MAX_RESIZE_FACTOR`].
fn whole(v: f32) -> Option<usize> {
    ((1.0..=MAX_RESIZE_FACTOR as f32).contains(&v) && v.fract() == 0.0).then_some(v as usize)
}

/// Nearest-neighbour upscale by whole numbers (the detector's feature pyramid doubles each map).
fn resize(node: &Node, x: &Tensor, g: &Graph) -> Result<Tensor> {
    let (c, h, w) = chw(x)?;
    let scales = match node.inputs.get(2).and_then(|n| g.weights.get(n)).map(|w| &w.data) {
        Some(Data::F32(s)) if s.len() == 4 => s,
        _ => return unsupported("a Resize whose scales are not four floats"),
    };
    let (Some(sh), Some(sw)) = (whole(scales.get(2).copied().unwrap_or(0.0)), whole(scales.get(3).copied().unwrap_or(0.0))) else {
        return unsupported("a Resize by a scale that is not a whole number from 1 to 16");
    };
    if scales.first() != Some(&1.0) || scales.get(1) != Some(&1.0) {
        return unsupported("a Resize that scales channels");
    }
    let (oh, ow) = (h * sh, w * sw);
    let mut out = Tensor::zeros(vec![1, c, oh, ow])?;
    for (plane_in, plane_out) in x.data.chunks_exact((h * w).max(1)).zip(out.data.chunks_exact_mut((oh * ow).max(1))) {
        for (oy, row_out) in plane_out.chunks_exact_mut(ow.max(1)).enumerate() {
            let row_in = plane_in.get((oy / sh) * w..(oy / sh + 1) * w).unwrap_or(&[]);
            for (ox, o) in row_out.iter_mut().enumerate() {
                *o = row_in.get(ox / sw).copied().unwrap_or(0.0);
            }
        }
    }
    Ok(out)
}

fn transpose(node: &Node, x: &Tensor) -> Result<Tensor> {
    let default_perm: Vec<i64> = (0..x.shape.len() as i64).rev().collect();
    let perm: Vec<usize> = node
        .ints("perm")
        .unwrap_or(&default_perm)
        .iter()
        .map(|p| usize::try_from(*p).map_err(|_| NetError::Shape("a negative permutation".into())))
        .collect::<Result<_>>()?;
    let rank = x.shape.len();
    let mut seen = vec![false; rank];
    if perm.len() != rank || perm.iter().any(|p| *p >= rank || std::mem::replace(seen.get_mut(*p).unwrap_or(&mut true), true)) {
        return shape_err("the Transpose permutation does not match the tensor");
    }
    let out_shape: Vec<usize> = perm.iter().filter_map(|p| x.shape.get(*p).copied()).collect();
    // strides of the input, in elements
    let mut in_strides = vec![1usize; rank];
    for i in (0..rank.saturating_sub(1)).rev() {
        let v = in_strides.get(i + 1).copied().unwrap_or(1) * x.shape.get(i + 1).copied().unwrap_or(1);
        *in_strides.get_mut(i).ok_or_else(|| NetError::Shape("a missing transpose stride".into()))? = v;
    }
    let strides_for_out: Vec<usize> = perm.iter().filter_map(|p| in_strides.get(*p).copied()).collect();
    let mut out = Tensor::zeros(out_shape.clone())?;
    let mut counter = vec![0usize; rank];
    let mut src = 0usize;
    for o in out.data.iter_mut() {
        *o = x.data.get(src).copied().unwrap_or(0.0);
        // advance the multi-index of the output and the matching input offset
        for d in (0..rank).rev() {
            let (Some(c), Some(len), Some(stride)) = (counter.get_mut(d), out_shape.get(d), strides_for_out.get(d)) else { break };
            *c += 1;
            src += stride;
            if *c < *len {
                break;
            }
            src -= stride * *len;
            *c = 0;
        }
    }
    Ok(out)
}

fn reshape(node: &Node, x: &Tensor, g: &Graph) -> Result<Tensor> {
    let Some(Data::I64(dims)) = node.inputs.get(1).and_then(|n| g.weights.get(n)).map(|w| &w.data) else {
        return unsupported("a Reshape with a computed shape");
    };
    let total = x.data.len();
    let mut shape: Vec<usize> = Vec::with_capacity(dims.len());
    let mut infer = None;
    for (i, d) in dims.iter().enumerate() {
        match *d {
            -1 if infer.is_none() => {
                infer = Some(i);
                shape.push(1);
            }
            0 => shape.push(x.shape.get(i).copied().ok_or_else(|| NetError::Shape("a Reshape copies a missing dimension".into()))?),
            d if d > 0 => shape.push(usize::try_from(d).map_err(|_| NetError::Shape("a Reshape dimension is too large".into()))?),
            _ => return shape_err("a bad Reshape dimension"),
        }
    }
    let known = shape.iter().try_fold(1usize, |a, d| a.checked_mul(*d)).filter(|k| *k > 0);
    if let (Some(i), Some(known)) = (infer, known) {
        if !total.is_multiple_of(known) {
            return shape_err("the Reshape does not divide evenly");
        }
        if let Some(s) = shape.get_mut(i) {
            *s = total / known;
        }
    }
    Tensor::new(shape, x.data.clone())
}
