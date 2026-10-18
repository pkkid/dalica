# dalica-faces

**Layer L1.** Face detection, alignment, embeddings and clustering on decoded RGB buffers
(`docs/faces.md`).

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
  Third-party: `ndarray` and `matrixmultiply` (single-threaded float32 GEMM, pure Rust, no BLAS, no
  thread pool), `serde` and `serde_json` (manifests, `catalog.json`), `sha2` (model hashes) and
  `thiserror`.
- **Test it alone:** `cargo test -p dalica-faces` (no window, GPU, network, model file or personal
  photos needed: every network is a seeded synthetic one). `DALICA_FULL_TESTS=1` runs the property
  tests at full strength. `DALICA_FACE_MODELS=<folder>` also self-tests every known model file in
  that folder (`tests/models.rs`; without it that test says it skipped).
- **Fuzz it:** `cd crates/faces/fuzz && cargo +nightly fuzz run model` (`docs/security.md` §7).

## Public API (built so far)

The ONNX interpreter the detector and the recogniser run on (M13.6; `faces.md` §2):

| Module | What |
|---|---|
| `limits` | every limit a model is held to: file ≤ 512 MiB, weights ≤ 64 Mi elements, rank ≤ 8, live tensors and scratch ≤ 512 MiB, opsets 10–17, … (`security.md` §4.2) |
| `onnx` | `probe_reader` / `probe_path`: a file's inputs and outputs (name, element type, shape) without loading it; `LoadError` (`Malformed` or `Unsupported`, each with its reason) |
| `graph` | `load(bytes)`: a bounded read of a whole model into a `Graph` (`Node`, `Attr`, `Weight`, `Data`); external or sparse weights, custom domains, training graphs, other opsets and element types refused by name |
| `net` | `Net::new(graph)` checks every operator and attribute; `Net::check_input(shape)` checks every shape and the working memory without allocating; `Net::run(Tensor)` runs Conv (depthwise and grouped), BatchNormalization, PRelu, Relu, Sigmoid, Add, Sub, Mul, Dropout, Flatten, Gemm, MaxPool, nearest Resize, Transpose, Reshape, GlobalAveragePool and Identity on the CPU; `NetError` (`Unsupported`, `Shape`, `NotFinite`) |
| `runtime` | `Model::from_bytes` / `read` / `open`: a loaded model, checked for its declared input at load; `Model::run` (a caught panic is an error); share it between threads by reference or `Arc` |
| `synthetic` | `encode(&Graph)` writes any graph as an ONNX file; seeded networks (`every_operator_graph`, `recognition_model`, `tiny_embedder_model`, `tiny_detector_model`) and protobuf helpers for tests, fuzz seeds and benches |

Grouping and suggestions (M13.7; `faces.md` §5), pure maths on unit embeddings, no model needed:

| Module | What |
|---|---|
| `calibration` | the Suggestion and Grouping confidences (50 … 95) as cosine thresholds of the model in use |
| `cluster` | incremental grouping of unnamed faces, deterministic in photo order, with a merge pass |
| `matching` | embeddings and their 8-bit in-memory form; name suggestions from confirmed faces (best match, margin) |
| `evaluate` | suggestions made and right per confidence step on the user's own named faces |

The recogniser (M13.10; `faces.md` §2–§3):

| Module | What |
|---|---|
| `detect` | `Detector::new(Model, &ModelManifest)` (the decoder's twelve outputs checked at load) and `detect(&Rgb, &Options)`: the photo letterboxed into the manifest's square input (640 for the candidate detector), the `yunet-v2` outputs decoded to boxes, scores and five landmarks, non-maximum suppression, boxes clipped and normalised on the upright photo, at most 256 faces; `probe_picture` (the self-test's fixed picture) |
| `align` | `Rgb` (a borrowed 8-bit RGB picture); `fit_similarity` (least squares), `align_to_template` (the five landmarks onto the 112 × 112 template, supersampled), `crop_box` (a padded square), `face_crop` (a stored face's normalised rectangle and landmarks to the crop) |
| `embed` | `Embedder::new(Model, &ModelManifest)` / `open`: the input and dimension checked at load; `embed(crop)` and `embed_face(&Rgb, rect, landmarks)` give one unit vector (float64 norm); `unit` |
| `manifest` | `ModelManifest` (role, licence with its notice, source, SHA-256, size, provenance, input spec, output, thresholds, optional calibration table) and `validate` / `parse`: every field bounded, a licence required, `https://` only |
| `catalog` | the user's `catalog.json`: each entry a manifest plus a pinned `https://` address, bad entries reported and skipped |
| `known` | the candidate models known by SHA-256 (`lookup`), their pinned addresses (`pinned`), the built-in list (`OFFERED`, `download`, `builtin`: empty until reserved decision 5), and `recognise` for a dropped file (known by hash, or a drafted manifest with its assumptions stated, or refused with the reason) |
| `hash` | streamed SHA-256 of a file, a reader or bytes |
| `selftest` | `run` / `run_path`: the manifest, the file's hash, the load, the shapes and a fixed synthetic input, each check listed with whether it held |
