//! The detector, alignment and embeddings (`faces.md` §2) on seeded synthetic networks: a detector
//! of the real one's shape whose outputs are planted constants decodes to the expected boxes and
//! landmarks, suppression keeps one box per face, boxes stay inside the photo; alignment puts the
//! five landmarks on the template within half a pixel; every embedding has unit length.
//!
//! Adapted from LightCraft `crates/faces/src/yunet.rs`, `align.rs` and `runtime.rs` tests
//! @ f34c7a8a (MIT OR Apache-2.0).

use dalica_faces::align::{Rgb, Similarity, TEMPLATE_112, align_to_template, crop_box, face_crop, template_transform};
use dalica_faces::detect::{Detector, Face, MAX_SIDE, Options};
use dalica_faces::embed::Embedder;
use dalica_faces::graph::{Attr, Graph, Node, Weight};
use dalica_faces::known;
use dalica_faces::manifest::{InputSpec, ModelManifest, OutputSpec, Role};
use dalica_faces::runtime::Model;
use dalica_faces::synthetic::{self, Seeded, encode};
use proptest::prelude::*;

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
fn cases() -> u32 {
    if std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1") { 256 } else { 16 }
}

/// The detector's input side in these tests (the real one's is 640; any multiple of 32 decodes the
/// same way, and a small one keeps the tests quick).
const SIDE: usize = 64;

/// One planted cell: stride, row, column, class score, objectness, box, landmarks.
struct Plant {
    stride: usize,
    row: usize,
    col: usize,
    cls: f32,
    obj: f32,
    bbox: [f32; 4],
    kps: [f32; 10],
}

const KPS: [f32; 10] = [0.25, 0.25, 0.75, 0.25, 0.5, 0.5, 0.3, 0.75, 0.7, 0.75];

fn plant(stride: usize, row: usize, col: usize, score: f32, bbox: [f32; 4]) -> Plant {
    Plant { stride, row, col, cls: score * score, obj: 1.0, bbox, kps: KPS }
}

/// A network of the detector's shape (input `[1, 3, side, side]`, the twelve outputs
/// `cls|obj|bbox|kps_<stride>` of `[1, cells, 1|1|4|10]`) whose outputs are the planted constants.
fn detector_bytes(side: usize, plants: &[Plant]) -> Vec<u8> {
    let (mut nodes, mut weights, mut outputs) = (Vec::new(), Vec::new(), Vec::new());
    for stride in [8usize, 16, 32] {
        let cols = side / stride;
        let cells = cols * cols;
        for (kind, n) in [("cls", 1usize), ("obj", 1), ("bbox", 4), ("kps", 10)] {
            let mut values = vec![0.0f32; cells * n];
            for p in plants.iter().filter(|p| p.stride == stride) {
                let at = (p.row * cols + p.col) * n;
                let v: &[f32] = match kind {
                    "cls" => std::slice::from_ref(&p.cls),
                    "obj" => std::slice::from_ref(&p.obj),
                    "bbox" => &p.bbox,
                    _ => &p.kps,
                };
                values[at..at + n].copy_from_slice(v);
            }
            let name = format!("{kind}_{stride}");
            let weight = format!("w_{name}");
            nodes.push(Node::new("Identity", &[&weight], &name, &[]));
            weights.push((weight, Weight::f32(&[1, cells, n], values)));
            outputs.push(name);
        }
    }
    encode(&Graph { nodes, weights: weights.into_iter().collect(), inputs: vec![("input".into(), vec![1, 3, side, side])], outputs })
}

/// The candidate detector's manifest at another input side, with no file hash.
fn detector_manifest(side: usize) -> ModelManifest {
    let mut m = known::yunet();
    m.input.width = side as u32;
    m.input.height = side as u32;
    m.sha256 = None;
    m.size_bytes = None;
    m
}

fn detector(plants: &[Plant]) -> Detector {
    Detector::new(Model::from_bytes(&detector_bytes(SIDE, plants)).unwrap(), &detector_manifest(SIDE)).unwrap()
}

fn grey(width: usize, height: usize) -> Vec<u8> {
    vec![128u8; width * height * 3]
}

fn find(d: &Detector, width: usize, height: usize) -> Vec<Face> {
    let pixels = grey(width, height);
    d.detect(&Rgb { data: &pixels, width, height }, &Options::default()).unwrap()
}

fn assert_near(got: f32, want: f32, what: &str) {
    assert!((got - want).abs() < 1e-5, "{what}: {got}, expected {want}");
}

#[test]
fn planted_outputs_decode_to_the_expected_box_and_landmarks() {
    // a 128 × 64 photo fills the top 64 × 32 of the 64-pixel input (scale 0.5)
    let d = detector(&[plant(8, 2, 3, 0.9, [0.5, 0.5, 2f32.ln(), 1.5f32.ln()])]);
    assert_eq!(d.input_side(), SIDE);
    let faces = find(&d, 128, 64);
    assert_eq!(faces.len(), 1, "{faces:?}");
    let f = faces[0];
    assert_near(f.score, 0.9, "score");
    // centre (3.5, 2.5) cells of 8 px = (28, 20), size 16 × 12 input pixels; the input maps to the
    // photo's width over 64 px and its height over 32 px
    for (got, want) in f.rect.iter().zip([20.0 / 64.0, 14.0 / 32.0, 36.0 / 64.0, 26.0 / 32.0]) {
        assert_near(*got, want, "box");
    }
    for (i, (x, y)) in f.landmarks.iter().enumerate() {
        assert_near(*x, (KPS[i * 2] + 3.0) * 8.0 / 64.0, "landmark x");
        assert_near(*y, (KPS[i * 2 + 1] + 2.0) * 8.0 / 32.0, "landmark y");
    }
    // below the score threshold: nothing
    let faint = detector(&[plant(8, 2, 3, 0.5, [0.5, 0.5, 0.0, 0.0])]);
    assert!(find(&faint, 128, 64).is_empty());
    assert_eq!(faint.detect(&Rgb { data: &grey(128, 64), width: 128, height: 64 }, &Options { score: 0.4, ..Options::default() }).unwrap().len(), 1);
}

#[test]
fn suppression_keeps_one_box_per_face() {
    let d = detector(&[
        // one face found by two neighbouring cells and by the coarser grid
        plant(8, 2, 3, 0.9, [0.5, 0.5, 2f32.ln(), 1.5f32.ln()]),
        plant(8, 2, 4, 0.8, [-0.5, 0.5, 2f32.ln(), 1.5f32.ln()]),
        plant(16, 1, 1, 0.7, [0.75, 0.25, 0.0, 0.75f32.ln()]),
        // another face elsewhere
        plant(8, 1, 6, 0.75, [0.5, 0.5, 0.0, 0.0]),
    ]);
    let faces = find(&d, 128, 64);
    assert_eq!(faces.iter().map(|f| (f.score * 100.0).round()).collect::<Vec<_>>(), [90.0, 75.0], "{faces:?}");
    assert_near(faces[1].rect[0], 48.0 / 64.0, "the second face's box");
    // the most faces asked for, best first
    assert_eq!(d.detect(&Rgb { data: &grey(128, 64), width: 128, height: 64 }, &Options { max_faces: 1, ..Options::default() }).unwrap().len(), 1);
    // every box of the three overlapping cells is the same face: with no suppression all three
    // come back
    let all = d.detect(&Rgb { data: &grey(128, 64), width: 128, height: 64 }, &Options { nms_iou: 1.0, score: 0.6, max_faces: 10 }).unwrap();
    assert_eq!(all.len(), 4);
}

#[test]
fn a_box_past_the_photos_edge_is_clipped_to_it() {
    // centred on the input's corner, 64 × 64 input pixels: half of it lies outside
    let d = detector(&[plant(32, 0, 0, 0.95, [0.0, 0.0, 2f32.ln(), 2f32.ln()])]);
    let faces = find(&d, 64, 64);
    assert_eq!(faces.len(), 1);
    for (got, want) in faces[0].rect.iter().zip([0.0, 0.0, 0.5, 0.5]) {
        assert_near(*got, want, "clipped box");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// Any planted outputs on any photo size: every box inside the photo, a real box, no NaN; and
    /// the same faces on a second run.
    #[test]
    fn boxes_stay_inside_the_photo(seed in any::<u32>(), width in 1usize..300, height in 1usize..300) {
        let mut r = Seeded::new(seed);
        let plants: Vec<Plant> = (0..12)
            .map(|i| {
                let stride = [8, 16, 32][i % 3];
                let cols = SIDE / stride;
                let cell = (r.next(1.0) + 0.5) * (cols * cols) as f32;
                let cell = (cell as usize).min(cols * cols - 1);
                let mut kps = [0.0; 10];
                kps.iter_mut().for_each(|k| *k = r.next(6.0));
                Plant {
                    stride, row: cell / cols, col: cell % cols,
                    cls: r.next(1.0) + 0.5, obj: r.next(3.0) + 0.5,
                    bbox: [r.next(8.0), r.next(8.0), r.next(30.0), r.next(30.0)], kps,
                }
            })
            .collect();
        let d = detector(&plants);
        let pixels = synthetic_picture(width, height);
        let img = Rgb { data: &pixels, width, height };
        let opts = Options { score: 0.1, ..Options::default() };
        let faces = d.detect(&img, &opts).unwrap();
        for f in &faces {
            let [x0, y0, x1, y1] = f.rect;
            prop_assert!((0.0..=1.0).contains(&x0) && (0.0..=1.0).contains(&y0) && (0.0..=1.0).contains(&x1) && (0.0..=1.0).contains(&y1), "{f:?}");
            prop_assert!(x0 < x1 && y0 < y1, "{f:?}");
            prop_assert!(f.score.is_finite() && f.landmarks.iter().all(|(x, y)| x.is_finite() && y.is_finite()), "{f:?}");
        }
        prop_assert_eq!(d.detect(&img, &opts).unwrap(), faces);
    }
}

fn synthetic_picture(width: usize, height: usize) -> Vec<u8> {
    (0..width * height * 3).map(|i| ((i * 31) % 251) as u8).collect()
}

#[test]
fn hostile_pictures_and_options_are_errors_or_defaults_never_panics() {
    let d = detector(&[]);
    let o = Options::default();
    assert!(d.detect(&Rgb { data: &[], width: 0, height: 0 }, &o).is_err());
    assert!(d.detect(&Rgb { data: &[0; 12], width: 2, height: 3 }, &o).is_err(), "wrong byte count");
    assert!(d.detect(&Rgb { data: &[0; 3], width: usize::MAX, height: 2 }, &o).is_err(), "overflowing size");
    assert!(d.detect(&Rgb { data: &[0; 3], width: MAX_SIDE + 1, height: 1 }, &o).is_err());
    for (w, h) in [(1, 1), (1, 700), (700, 1), (37, 41)] {
        assert!(find(&d, w, h).is_empty(), "{w} × {h}");
    }
    let nan = Options { score: f32::NAN, nms_iou: f32::INFINITY, max_faces: usize::MAX };
    assert!(d.detect(&Rgb { data: &grey(12, 12), width: 12, height: 12 }, &nan).is_ok(), "non-finite options fall back");
}

#[test]
fn a_model_that_is_not_shaped_like_the_detector_is_refused_at_load() {
    let manifest = detector_manifest(SIDE);
    // an output missing
    let mut bytes = detector_bytes(SIDE, &[]);
    while let Some(at) = bytes.windows(5).position(|w| w == b"cls_8") {
        bytes[at..at + 5].copy_from_slice(b"bad_8");
    }
    let error = Detector::new(Model::from_bytes(&bytes).unwrap(), &manifest).unwrap_err().to_string();
    assert!(error.contains("cls_8"), "{error}");
    // an input of another size than the manifest's
    assert!(Detector::new(Model::from_bytes(&detector_bytes(96, &[])).unwrap(), &manifest).is_err());
    // a recogniser, or a recogniser's manifest
    assert!(Detector::new(Model::from_bytes(&synthetic::tiny_embedder_model(16)).unwrap(), &manifest).is_err());
    assert!(Detector::new(Model::from_bytes(&detector_bytes(SIDE, &[])).unwrap(), &known::sface()).is_err());
}

fn gaussian_dots(width: usize, height: usize, at: &[(f32, f32)], sigma: f32) -> Vec<u8> {
    let mut data = vec![0u8; width * height * 3];
    for y in 0..height {
        for x in 0..width {
            let v: f32 = at.iter().map(|(cx, cy)| (-((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)) / (2.0 * sigma * sigma)).exp()).sum();
            data[(y * width + x) * 3] = (v.min(1.0) * 255.0).round() as u8;
        }
    }
    data
}

/// A picture with a dot centred on each of five landmarks, the landmarks being the template moved
/// by a known similarity (turned, enlarged, shifted by fractions of a pixel): aligning must put the
/// dots back on the template.
#[test]
fn alignment_maps_the_five_landmarks_to_their_template_within_half_a_pixel() {
    let (w, h) = (400usize, 300usize);
    let (s, th) = (1.7f32, -15f32.to_radians());
    let place = Similarity { a: (s * th.cos(), s * th.sin()), t: (150.37, 60.81) };
    let landmarks = TEMPLATE_112.map(|p| place.apply(p));
    // the transform itself
    let m = template_transform(&landmarks, 112, 112).unwrap();
    for (l, t) in landmarks.iter().zip(TEMPLATE_112) {
        let q = m.apply(*l);
        assert!((q.0 - t.0).abs() < 0.01 && (q.1 - t.1).abs() < 0.01, "{q:?} vs {t:?}");
    }
    // the pixels: the centre of mass of each dot lands on its template point
    let data = gaussian_dots(w, h, &landmarks, 3.0);
    let img = Rgb { data: &data, width: w, height: h };
    let out = align_to_template(&img, &landmarks, 112, 112).unwrap();
    assert_eq!(out.len(), 112 * 112 * 3);
    for t in TEMPLATE_112 {
        let (mut sx, mut sy, mut sw) = (0.0f32, 0.0f32, 0.0f32);
        for y in (t.1 as usize - 8)..=(t.1 as usize + 8) {
            for x in (t.0 as usize - 8)..=(t.0 as usize + 8) {
                let v = f32::from(out[(y * 112 + x) * 3]);
                sx += v * x as f32;
                sy += v * y as f32;
                sw += v;
            }
        }
        assert!(sw > 100.0, "no dot near {t:?}");
        let (cx, cy) = (sx / sw, sy / sw);
        assert!((cx - t.0).abs() <= 0.5 && (cy - t.1).abs() <= 0.5, "dot at ({cx}, {cy}), template {t:?}");
    }
    // the same face from the normalised landmarks a face is stored with
    let normalised = landmarks.map(|(x, y)| ((x + 0.5) / w as f32, (y + 0.5) / h as f32));
    let again = face_crop(&img, [0.0, 0.0, 1.0, 1.0], Some(&normalised), 112, 112).unwrap();
    assert!(again.iter().zip(&out).all(|(a, b)| a.abs_diff(*b) <= 1), "the normalised landmarks give the same crop");
}

#[test]
fn without_landmarks_a_padded_box_is_cut_and_degenerate_input_gives_nothing() {
    let (w, h) = (200usize, 100usize);
    let mut data = vec![0u8; w * h * 3];
    for y in 40..60 {
        for x in 90..110 {
            data[(y * w + x) * 3 + 1] = 255;
        }
    }
    let img = Rgb { data: &data, width: w, height: h };
    // the 20 px block, padded 1.5 times, fills the middle two thirds of a 60 px crop
    let out = face_crop(&img, [90.0 / 200.0, 40.0 / 100.0, 110.0 / 200.0, 60.0 / 100.0], None, 60, 60).unwrap();
    let at = |x: usize, y: usize| out[(y * 60 + x) * 3 + 1];
    assert!(at(30, 30) > 250 && at(12, 12) > 250 && at(47, 47) > 250, "the block");
    assert!(at(5, 5) < 5 && at(55, 30) < 5 && at(30, 55) < 5, "the padding");
    assert!(crop_box(&img, 5.0, 5.0, 5.0, 5.0, 1.5, 64, 64).is_none(), "an empty box");
    assert!(crop_box(&img, f32::NAN, 0.0, 10.0, 10.0, 1.5, 64, 64).is_none());
    assert!(crop_box(&img, 0.0, 0.0, 10.0, 10.0, 1.5, 0, 64).is_none());
    assert!(crop_box(&img, 0.0, 0.0, 10.0, 10.0, 1.5, 100_000, 64).is_none());
    assert!(crop_box(&Rgb { data: &data[..10], width: w, height: h }, 0.0, 0.0, 10.0, 10.0, 1.5, 64, 64).is_none(), "wrong data length");
    // landmarks all on one point, a hundredth of a pixel apart, or not finite
    assert!(align_to_template(&img, &[(5.0, 5.0); 5], 112, 112).is_none());
    let tiny = [(1.0, 1.0), (1.01, 1.0), (1.0, 1.01), (1.01, 1.01), (1.005, 1.005)];
    assert!(align_to_template(&img, &tiny, 112, 112).is_none());
    assert!(align_to_template(&img, &[(f32::NAN, 0.0); 5], 112, 112).is_none());
    // a box mostly outside the picture is padded by clamping, not an error
    assert!(crop_box(&img, -30.0, -30.0, 20.0, 20.0, 1.5, 32, 32).is_some());
}

fn recogniser_manifest(dim: u32, input: InputSpec) -> ModelManifest {
    let mut m = known::sface();
    m.id = "test-recogniser".into();
    m.sha256 = None;
    m.size_bytes = None;
    m.input = input;
    m.output = OutputSpec::Embedding { dim };
    m
}

/// A recogniser whose numbers are near the top of float32: their sum of squares overflows in
/// float32, not in float64.
fn huge_output_model() -> Vec<u8> {
    let nodes = vec![
        Node::new("GlobalAveragePool", &["data"], "p", &[]),
        Node::new("Flatten", &["p"], "f", &[("axis", Attr::Int(1))]),
        Node::new("Gemm", &["f", "w", "b"], "emb", &[("transB", Attr::Int(1))]),
    ];
    let weights = [
        ("w".to_string(), Weight::f32(&[3, 3], vec![1e36, 0.0, 0.0, 0.0, 1e36, 0.0, 0.0, 0.0, 1e36])),
        ("b".to_string(), Weight::f32(&[3], vec![0.0; 3])),
    ];
    encode(&Graph { nodes, weights: weights.into_iter().collect(), inputs: vec![("data".into(), vec![1, 3, 112, 112])], outputs: vec!["emb".into()] })
}

#[test]
fn embeddings_have_unit_length() {
    let unit = |v: &[f32]| (v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() - 1.0).abs() < 1e-6;
    let face: Vec<u8> = (0..112 * 112 * 3).map(|i| (i * 7 % 256) as u8).collect();
    let tiny =
        Embedder::new(Model::from_bytes(&synthetic::tiny_embedder_model(16)).unwrap(), &recogniser_manifest(16, InputSpec::default())).unwrap();
    assert_eq!((tiny.input_size(), tiny.dimension()), ((112, 112), 16));
    let v = tiny.embed(&face).unwrap();
    assert!(v.len() == 16 && unit(&v), "{v:?}");
    let raw = InputSpec { mean: [0.0; 3], std: [1.0; 3], ..InputSpec::default() };
    for seed in 0..3 {
        let e = Embedder::new(Model::from_bytes(&synthetic::recognition_model(seed)).unwrap(), &recogniser_manifest(8, raw)).unwrap();
        let v = e.embed(&face).unwrap();
        assert!(v.len() == 8 && unit(&v), "seed {seed}: {v:?}");
    }
    // large but finite numbers: a float64 norm
    let e = Embedder::new(Model::from_bytes(&huge_output_model()).unwrap(), &recogniser_manifest(3, raw)).unwrap();
    let v = e.embed(&vec![255; 112 * 112 * 3]).unwrap();
    assert!(v.iter().all(|x| x.is_finite()) && unit(&v), "{v:?}");
    // a stored face of a photo, aligned and embedded
    let photo = synthetic_picture(300, 200);
    let img = Rgb { data: &photo, width: 300, height: 200 };
    let v = tiny.embed_face(&img, [0.3, 0.3, 0.6, 0.7], None).unwrap();
    assert!(unit(&v));
    // the wrong crop size, and a face that cannot be cut out
    assert!(tiny.embed(&[0u8; 10]).is_err());
    assert!(tiny.embed_face(&img, [0.5, 0.5, 0.5, 0.5], None).is_err());
}

#[test]
fn the_colour_order_reaches_the_model() {
    let rgb = Embedder::new(Model::from_bytes(&synthetic::tiny_embedder_model(16)).unwrap(), &recogniser_manifest(16, InputSpec::default())).unwrap();
    let bgr_spec = InputSpec { colour: dalica_faces::manifest::Colour::Bgr, ..InputSpec::default() };
    let bgr = Embedder::new(Model::from_bytes(&synthetic::tiny_embedder_model(16)).unwrap(), &recogniser_manifest(16, bgr_spec)).unwrap();
    let img: Vec<u8> = (0..112 * 112).flat_map(|_| [200u8, 100, 20]).collect();
    let (a, b) = (rgb.embed(&img).unwrap(), bgr.embed(&img).unwrap());
    let cos: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    assert!(cos < 0.9999, "{cos}");
    assert_eq!(rgb.manifest().role, Role::Embedder);
}
