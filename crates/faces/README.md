# dalica-faces

**Layer L1.** Face detection, alignment, embeddings and clustering on decoded RGB buffers
(`docs/faces.md`).

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
  Third-party: `ndarray` and `matrixmultiply` (single-threaded float32 GEMM, pure Rust, no BLAS, no
  thread pool) and `thiserror`.
- **Test it alone:** `cargo test -p dalica-faces` (no window, GPU, network, model file or personal
  photos needed: every network is a seeded synthetic one). `DALICA_FULL_TESTS=1` runs the property
  tests at full strength.
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

The detector, alignment, embeddings, manifests and the self-test (M13.10) follow.
