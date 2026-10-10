# Faces: detection and recognition

Adapted from LightCraft `docs/faces.md` and `crates/faces` (revision 88ea1a8); MIT OR Apache-2.0.

> **Status: Planned (M7).** Nothing here is built. The user-facing behaviour (the People group,
> Unnamed, suggestions, the People pane, options) is specified in [`organizing.md`](organizing.md)
> §6; where faces are stored is in [`data-model.md`](data-model.md) §2–§5; how names travel in
> files is in [`xmp-interop.md`](xmp-interop.md) §6. This document is the engine design behind
> them. The choice of default models and their licences is a reserved decision
> ([`roadmap.md`](roadmap.md), *Reserved decisions* #5).

## 1. Principles

1. **Nothing leaves the machine.** Detection, recognition, clustering and suggestions run locally
   on the CPU. No photo, face crop, embedding, name or count is ever sent anywhere. The only network
   access is a model download the user started and agreed to (§3).
2. **Nothing is bundled.** No model weights are part of Dalica, its repository, its corpora or its
   release packages. `cargo xtask assets` rejects `.onnx` and other weight files.
3. **Nothing is named for you.** A suggestion is a label until the user confirms it. It is better to
   leave a face unnamed than to name it wrongly.
4. **Pure Rust, never crash.** The inference engine is Rust with no C, assembly or `unsafe`; a model
   file is hostile input like any other file.
5. **Faces are user data.** Rectangles and person assignments live in the folder sidecars
   (`faces=`, `[Person …]`); embeddings are a rebuildable cache in the library index. Face data never
   appears in logs (`development.md` §10).

## 2. Pipeline

```text
photo ──► upright preview (≤ 2048 px; RAW: embedded preview)
      ──► detect (boxes + 5 landmarks: eyes, nose, mouth corners)
      ──► align (similarity transform to a 112 × 112 crop; box crop if no landmarks)
      ──► embed (one unit-length vector per face)
      ──► cluster unnamed faces (Grouping confidence)
      ──► suggest names from named faces (Suggestion confidence)
```

- **Crate:** a new `crates/faces` (`dalica-faces`, L1: depends on `core` only and takes decoded RGB
  buffers; the caller decodes through `image`). It is registered in `xtask/src/layers.rs` and
  `architecture.md` §2 in the commit that creates it. It is our own code; LightCraft's `crates/faces`
  (`runtime`, `onnx`, `graph`, `yunet`, `align`, `manifest`, `catalog`, `hash`, `matching`,
  `suggest`) is the prior art to study.
- **Inference engine:** a small pure-Rust ONNX interpreter shared by the detector and the
  recogniser. It supports the operators the pinned models use (Conv including depthwise groups,
  BatchNormalization, PRelu, Relu, Sigmoid, Add, Sub, Mul, Dropout as identity, Flatten, Gemm,
  MaxPool, nearest whole-number Resize, Transpose, constant-shape Reshape, GlobalAveragePool,
  Identity). Anything else (other operators, custom domains, training graphs, external or sparse
  weights, non-float32 tensors) fails to load with an explicit error. Dense convolutions use a
  bounded im2col and single-threaded float32 GEMM (`ndarray` / `matrixmultiply`, pure Rust, default
  BLAS and threading features off); the recogniser never creates its own thread pool.
- **Limits** (checked before a model's self-test): model file ≤ 512 MiB, combined weights ≤ 64 Mi
  elements, tensor rank ≤ 8, live tensors plus convolution scratch ≤ 512 MiB; every weight and
  activation must be finite; embeddings are normalised with a float64 norm to avoid overflow.
- **Coordinates:** detected boxes are stored like every face rectangle: normalised floats on the
  photo as displayed after rotation (`data-model.md` §3). Detection always looks at the upright,
  uncropped photo with no edits, so edits never change which faces exist.
- **Small faces:** the detector sees a 640 px version of the photo, so it finds faces from about
  10 px at that size; very small faces in large group photos can be missed (tiled detection is a
  later improvement).

## 3. Models: consent, pinning and licences

### 3.1 Getting a model

Options → People → **Face model** shows the status and **[Download…]** / **[Remove]**; turning on
**Find faces in my photos** without a model, or opening People for the first time, offers the same
dialog.

1. The dialog shows, for each file: the model's licence, whether commercial use is allowed, what it
   was trained on (or that this is not known), its size, and the site it comes from. **Download
   stays disabled until the user ticks "I have read these terms and accept them for my own use".**
   Nothing is fetched before that.
2. The file is fetched over HTTPS from the model's own repository at an address **pinned to a
   commit**, in pure Rust (a small `fetch` crate: `rustls` with a pure-Rust crypto provider and
   bundled web roots; no OpenSSL, no `ring`, no curl). Interrupted downloads resume. No account,
   token, cookie or personal header is sent; the User-Agent is generic.
3. The file must match the recorded **size and SHA-256** or it is deleted. A matching file passes a
   self-test (load, shapes, a fixed synthetic input) before it is installed into the models folder.
4. **Open page** opens the model's page in the browser for users who prefer to fetch it themselves;
   **Add a model file…** (or dropping an `.onnx` on the window) installs a local file after the same
   licence dialog. Known files are recognised by their SHA-256; for others Dalica reads the input and
   output shapes and states its assumptions (112 × 112 aligned RGB faces, `(x − 127.5) / 127.5`, one
   vector per face).

The models folder is `<config>/models` (Linux `~/.config/dalica/models`, macOS
`~/Library/Application Support/Dalica/models`, Windows `%APPDATA%\Dalica\models`) or
`$DALICA_FACE_MODELS`, shared by the app, the CLI and the MCP server. It is never inside a watched
folder and never committed.

### 3.2 Which models

The built-in list may only point at models whose weights' licence lets the project link them.
Candidates, with what is publicly known (the maintainer decides; *Reserved decisions* #5):

| Model | Role | Weights licence | Trained on | Size |
|---|---|---|---|---|
| YuNet 2023mar | detector | MIT | WIDER FACE (dataset terms are a separate question) | 232 KB |
| SFace 2021dec | recogniser | labelled Apache-2.0 | undocumented upstream | 39 MB |
| AuraFace v1 | recogniser | Apache-2.0 | "a commercial dataset", undisclosed | 261 MB |

- "Commercial use allowed" in the dialog describes the weights' licence only; it says nothing about
  the training data, and the dialog says so.
- Models whose weights are for non-commercial or research use are **never** offered, hosted or
  linked by Dalica. A user may still add such a file for their own use through **Add a model file…**
  or a `catalog.json` in the models folder (LightCraft's bring-your-own format: id, name, role, https
  URL, `sha256`, `sizeBytes`, licence notice, provenance, output dimension, thresholds, optional
  calibration table), shown with its notice in `caution` colour.
- Each manifest records the model's preprocessing (size, RGB/BGR, mean/std), embedding dimension,
  a recommended match threshold (`matchCosine`) and, for built-in models, a calibration table (§5).

### 3.3 Switching models

The newest installed recogniser is the one in use; **Use** switches back to another. Embeddings are
cached per model id, so switching back does not start over, and results of the previous model are
never mixed into the new model's index. Face *rectangles and names* do not depend on the model.

## 4. Background scan

Turning on **Find faces in my photos** starts a scan of the library at the lowest job-pool priority
(`architecture.md` §5), cancellable, with progress in the status line's activity indicator:

- A photo that already has face rectangles (from metadata, drawn by hand, or named) has them
  embedded. A photo with none is searched with the detector once; the index remembers which photos
  were searched, so a face the user removed is not found again. **Picture → Find Faces Again**
  (`faces.reset`) forgets that for the selection and re-detects (named faces are kept).
- Detected-but-unnamed rectangles are written to the sidecar like any face (they appear under
  Unnamed); ignored faces keep their `ignored` entry so they never return.
- Folders excluded in the Folder Manager, hidden folders when the user excluded them, and offline
  volumes are skipped.
- Photos with named faces go first (they seed suggestions), then newest folders first.
- RAW files are read through their embedded preview; everything else is decoded at the smallest
  scale covering 2048 px (JPEG DCT scaling).
- **Pace follows the user:** while the user drags, types or scrolls, no new photo is started; while
  the pointer moves, one photo at a time; idle for three seconds, about half the cores; minimised or
  idle on the People view, more. Photos in progress are never interrupted. The worker count is also
  limited by the memory budget (`architecture.md` §5), since a photo in progress holds a decoded
  preview plus model scratch.

## 5. Grouping and suggestions

Options → People: **Suggest names** (on), **Suggestion confidence** and **Grouping confidence**
(50–95 in steps of 5, default **85**; `organizing.md` §6.1).

- **Confidence → threshold.** Each slider value maps to a cosine-similarity threshold of the model in
  use. Built-in models carry a calibration table with one cosine value for each step 50, 55, …, 95,
  measured on a public-domain evaluation set so that higher values mean fewer, more certain matches;
  85 corresponds to the model's recommended `matchCosine`. A user model without a table uses
  `matchCosine + (value − 85) × 0.01`.
- **Suggestions:** an unnamed face gets "Name?" only when its best person scores at or above the
  Suggestion threshold **and** is ahead of the second-best person by a margin (default 0.05 cosine).
  A person's score is the best match over that person's confirmed faces (at most the 64 most recent
  and most varied faces per person, so a person with thousands of photos stays cheap). Confirming a
  suggestion makes that face a reference too, so suggestions improve as the user names faces;
  rejecting one records the pair so it is not suggested again.
- **Grouping:** unnamed faces are clustered incrementally: a newly embedded face joins the cluster
  whose centroid is most similar if that similarity is at or above the Grouping threshold, otherwise
  it starts a new cluster; a background pass merges clusters whose centroids meet the threshold.
  Clusters are shown largest first; within a cluster, faces most similar to the centroid first.
  Naming a cluster names every face in it in one undo step.
- **Find More** on a person (`organizing.md` §6.3) lists unnamed faces scoring above 80 % of that
  person's suggestion threshold, most similar first, because the user reviews each one.
- **Evaluate on your photos:** `faces.evaluate` hides each named face in turn, asks who it looks like
  from the others (ignoring shots taken within a few seconds of it), and reports per confidence step
  how many suggestions would be made and how many would be right, so a user can choose the slider
  value from evidence.
- Nothing about one library is used in another.

## 6. Storage

| What | Where | Rebuildable? |
|---|---|---|
| Face rectangles, person per face, ignored faces | folder sidecar `faces=` (`data-model.md` §3) | no: user data |
| People names (per folder mirror) | `[Person …]` sections; `people.json` in the library dir | yes, from sidecars |
| Embeddings, detector landmarks, "already searched" flags | library index, keyed by model id (`data-model.md` §4) | yes, by rescanning |
| Names in photo files | XMP MWG regions, only with **Store people names in photos** | — ([`xmp-interop.md`](xmp-interop.md) §6) |
| Models | `<config>/models` | re-download |

Embeddings are stored at float32 on disk and may be quantised (8-bit per component) in memory for
search, so 100 k faces stay within the memory budget; the exact footprint is measured, not assumed,
when M7 lands. Deleting a person returns their faces to Unnamed; deleting a photo removes its faces.

## 7. Privacy

- No network access except the consented model download; no telemetry; no cloud fallback.
- Face data (names, rectangles, embeddings, suggestions) never appears in logs, panic reports,
  toasts beyond the visible UI, or crash dumps. Logs say "embedded 120 faces", never whose.
- Names are written into photo files only with **Store people names in photos** (default off) or the
  explicit **Write People Names to Photos…** command. Export follows the same option and never adds
  regions the source does not have; "Remove location from exported photos" is independent.
- The control channel is loopback-only and MCP runs locally; face commands answer only to them.
- Removing a model deletes its file and its cached embeddings. **Turn off Find faces** stops the scan
  and offers to delete all cached embeddings (named rectangles stay, they are user data).

## 8. Performance

Budgets are added to `perf/budgets.toml` when M7 starts, as `not_measurable` until a bench exists
(`development.md` §13): faces embedded per second during a background scan, UI frame time while the
scan runs (must stay within the normal frame budget), People view open time with 10 k unnamed faces,
memory per worker. Reference figures from LightCraft's measurements of the same engine (one thread,
standard release build, Ryzen 9 7945HX, 40 aligned crops): SFace about 34–36 ms per face, AuraFace
about 233–252 ms per face. They are not Dalica numbers; Dalica records its own.

## 9. Commands

Ids follow the menu catalog where one exists (`faces.reset`, `faces.writeXmp`, `people.manager`);
the engine adds, all drivable from the control channel, CLI and MCP:
`faces.models.list`, `faces.models.inspect {path}`, `faces.models.install {path, acknowledged}`,
`faces.models.download {id, acknowledged}`, `faces.models.cancel {id}`, `faces.models.remove {id}`,
`faces.models.select {id}`, `faces.enable {on}`, `faces.detect {ids, apply?}`, `faces.index`,
`faces.suggest {ids?}`, `faces.name {faces, person}`, `faces.ignore {faces}`, `faces.evaluate`.
`acknowledged` must be `true`: it asserts that the user was shown the terms and agreed; agents must
never set it on their own initiative.

## 10. Tests

- **No real models in tests by default.** Seeded synthetic networks exercise the interpreter
  (every operator, PReLU broadcasting, grouped convolutions, padding reuse, limits, overflow,
  concurrent calls); malformed, truncated and ambiguous ONNX files must fail to load, never panic.
- **Real models** run only in `#[ignore]` tests with `DALICA_FACE_MODELS=<folder>`, which check the
  self-test against locally supplied, hash-recognised files; models and crops stay out of git.
- **Corpus:** `corpus/public-faces/` (public-domain portraits with `SOURCES.tsv`) behind the
  `corpus` feature: detection floor, alignment, embedding agreement with the recorded reference
  vectors, and a suggestion-precision floor at the default confidence. Floors only rise.
- **Behaviour:** clustering and suggestion property tests (thresholds monotonic in the slider value,
  margin respected, confirmed faces never re-suggested, rejected pairs remembered); undo of naming a
  cluster; rescan never resurrects removed or ignored faces; switching models never mixes caches.
- **Consent and network:** with an injected fetcher, no request is made before `acknowledged`; a
  wrong size or hash deletes the file; resume works; no request carries identifying headers.
- **Privacy:** a full people workflow under `RUST_LOG=trace` leaves no names or embeddings in the
  captured log.
- **Interop:** names round-trip through MWG regions ([`xmp-interop.md`](xmp-interop.md) §8).
