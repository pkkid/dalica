# Development

Mechanics for building, running, testing and verifying Dalica. Rules about *what* is allowed live in
`AGENTS.md`; the *why* of the process lives in [`process.md`](process.md).

> Status: the M0 workspace and gates exist (2026-10-09); commands marked *planned* in §3 do not
> exist yet (`cargo xtask docs` keeps this file honest). Keep it in sync with reality.

## 1. Prerequisites

- Rust stable pinned by `rust-toolchain.toml` (edition 2024, MSRV 1.98, the latest stable on
  2026-10-09; bumped deliberately) with `rustfmt` and `clippy`. `rustup` is optional; a system
  toolchain works if it matches (Fedora: `sudo dnf install rust cargo clippy rustfmt`).
- Linux: the usual eframe/wgpu system libraries (Wayland/X11, Vulkan driver). No C/C++ libraries are
  linked into the product.
- Optional: a checkout of `craft-fonts` for CJK glyphs (`CRAFT_FONTS_DIR=../craft-fonts`).

## 2. Everyday commands

```sh
cargo run --release -p dalica                       # open the app (default library)
cargo run --release -p dalica -- --control 7990     # with the JSON-lines control server
cargo run --release -p dalica -- --demo             # procedural demo library, no personal photos
cargo test -p <crate>                               # tests for one crate
cargo xtask ci                                      # THE gate: run before every commit
```

## 3. `cargo xtask`

| Command | What it does | In `ci` |
|---|---|---|
| `xtask ci` | fmt check, clippy `-D warnings` (all targets), tests, `layers`, `deps`, `assets`, `names`, `parity`, `docs`, `tasks sync --check`, then `accept` for every milestone whose blocks are all ✅; runs every step, prints a table, and on a clean worktree records the green commit for `tasks submit` | — |
| `xtask layers` | checks crate dependencies against the layer table in `xtask/src/layers.rs` (`architecture.md` §2); unregistered crates fail; UI crates only in L5; only `crates/fs` calls `std::fs` write functions | ✓ |
| `xtask deps` | offline dependency audit (`security.md` §8): licence allow-list over the whole graph; on the product graph of `dalica` and `dalica-cli` for Linux, Windows and macOS, no banned crate, no C build script (reviewed exceptions are checked against the resolved features) and `*-sys` only from the allow-list; sources; duplicate versions warn | ✓ |
| `xtask assets` | every image/icon/font/sound file is matched by a row in `ATTRIBUTION.md`; referenced licence files exist; forbidden file names/extensions fail | ✓ |
| `xtask names` | fails if the reference product's name appears in any tracked file other than `AGENTS.md` | ✓ |
| `xtask parity [--write]` | ids, tiers and statuses of `docs/parity.md`; every `cmd:` id (from `dalica-cli commands`) and path it cites exists; the summary is current (`--write` refreshes it) | ✓ |
| `xtask docs` | every `cargo xtask …` command named in `AGENTS.md`, this file and `task-board.md` exists or is registered as planned | ✓ |
| `xtask accept M<n> [--headless]` | runs a milestone's acceptance test (`tests/acceptance/m<n>.rs`) and prints what a human should look at (`m<n>.md`) (`roadmap.md` → How milestones work); finished milestones' checks run inside `ci` | ✓ (all finished milestones) |
| `xtask tasks <sub>` | the task board: `setup`, `sync`, `next`, `claim`, `env`, `submit`, `release`, `block`, `merge`, `status`, `log` ([`task-board.md`](task-board.md)); `sync --check` parses the roadmap offline, `setup`/`sync --dry-run` show the GitHub writes without making them | `sync --check` ✓ |
| `xtask test-corpus` | *planned (M8):* fetch the corpora and run the `corpus`-feature tests | |
| `xtask corpus` | *planned (M8):* fetches pinned, sha256-verified CC0 test photos into the gitignored `corpus/` | |
| `xtask bench` | *planned (M4):* library-scale and pixel benchmarks; appends to `target/bench/history.jsonl`, flags CPU-time regressions > 20 % | |
| `xtask perf` | *planned (from M4):* the budgeted benchmark run of §13 | |
| `xtask scorecard [--check]` | *planned (after M1):* regenerate / verify `docs/scorecard.md` | |
| `xtask snapshot <script>` | *planned (M2):* headless UI screenshot via the CPU rasterizer | |
| `xtask stats` | *planned (not scheduled):* line/test/command counts for the status doc | |

Running a planned command says where it is planned and exits with status 2.

`xtask ci` uses one test thread and build job per GiB of available RAM, capped by
`CARGO_BUILD_JOBS` or the CPU count, so it doesn't take the machine down when several agents run it
at once.

## 4. Code style

- `cargo fmt` (`rustfmt.toml`: `max_width = 150`, `use_small_heuristics = "Max"`).
- `cargo clippy --all-targets -- -D warnings`. Every crate root:
  `#![forbid(unsafe_code)]` and
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
  `clippy.toml` allows unwrap/expect/panic/indexing in tests only.
- Match surrounding code; comments explain *why*, not *what*. No bare `TODO` (use
  `TODO(maintainer): …` for questions a human must answer).
- Colours, sizes and fonts only through `theme::Tokens`; widgets only through `widgets.rs`.
- User-facing strings are English literals wrapped for translation (`tl!("…")`); ids and command
  labels are the i18n keys.

## 5. Adding a feature (checklist)

1. Find or add its row in `docs/parity.md` and its spec in `docs/` (write the spec first if missing).
2. Find the command id in `crates/ui/src/menu_catalog.rs` (menu items) or choose a descriptive id
   in the same style (`photo.star.toggle`, `folder.description.set`).
3. Put the algorithm in the lowest crate that fits (`edit` for pixels, `library` for model
   changes, `sidecar`/`meta` for persistence), with unit tests.
4. Add the command in `crates/engine/src/cmd/<area>.rs`: id, label, menu path, default shortcut,
   params doc string, `enabled`, `run` (exactly one undo step; coalesce drags/typing).
5. Tests: behaviour, undo/redo, disabled state, bad params (must return `Err`, never panic), and a
   headless UI test if it has UI.
6. UI: compose existing widgets; register automation ids; verify by screenshot.
7. `cargo xtask ci`; tick the task's ✅ and update its parity row; commit with the task id; `cargo xtask
   tasks log` and `tasks submit` (`AGENTS.md` §5).

## 6. Tests

- **Unit/property tests** next to the code (`proptest` for parsers, sidecar round trips, geometry).
- **Round-trip tests** for every writer: sidecar, XMP/IPTC write-back, Save-with-backup, export.
- **Malformed-input tests** for every reader (truncated JPEG/EXIF, hostile sidecars, huge counts).
- **`panic_hunt`**: fuzzes every registered command with hostile params; must stay green.
- **Headless UI tests**: drive the real UI through the headless driver (click by automation id,
  `step_until(condition)`, never "N quiet frames"), assert state and counters (cells visited per
  frame, jobs requested).
- **Workflow acceptance tests** (`crates/mcp/tests/workflows.rs`): end-to-end user tasks over MCP,
  checked only through public state and output files.
- **Library-scale tests**: a synthetic 100 k-photo library must scroll with < 8 ms/frame UI time.
- Test photos are procedural (`dalica-scenes`, M1) or CC0 from the pinned corpus. Never commit
  media; never use personal photos.

## 7. Driving the app (for agents and tests)

```sh
cargo run --release -p dalica -- --control 7990 --demo
# then send JSON lines (the first line authenticates with the token file; see control-protocol.md):
{"method":"engine.execute","params":{"command":"photo.star.toggle","params":{"ids":[42]}}}
{"method":"ui.clickWidget","params":{"id":"button:tray.hold"}}
{"method":"ui.menu.invoke","params":{"path":"Picture/Batch Edit/Rotate Clockwise"}}
{"method":"ui.screenshot","params":{"path":"/tmp/after.png"}}
```

- Parallel agents use their own port in 18000–19999.
- Headless (no window, works with the display asleep and in CI):
  `dalica-cli snapshot --demo --script tour.jsonl -o out.png --size 1400x900`.
- MCP: `claude mcp add dalica -- "$PWD/target/release/dalica-cli" mcp --demo` (headless) or
  `… mcp --connect 127.0.0.1:7990` (the running app).
- Never send OS-level synthetic input.

## 8. Performance budgets (initial)

| Scenario | Budget |
|---|---|
| UI frame time, light table with 100 k photos, scrolling | ≤ 8 ms (p95) |
| First thumbnails visible after adding a folder of 1,000 JPEGs | ≤ 1 s |
| Initial scan throughput (embedded thumbnails available) | ≥ 500 photos/s on SSD |
| Open a 24 MP JPEG in the edit view (screen-size preview) | ≤ 150 ms |
| Slider drag update (screen-size preview) | ≤ 16 ms draft, ≤ 100 ms final |
| Next/previous photo in the edit view (prefetched) | ≤ 50 ms |
| Cold start to a usable library of 100 k photos | ≤ 2 s |
| Idle memory with 100 k photos | ≤ 600 MiB |

Measure, don't guess: record numbers in `docs/status.md` → Metrics with the machine and date.

## 9. Repository hygiene

- Never commit: `notes/`, `corpus/`, `target*/`, media files, model weights, secrets.
- Fonts: Inter is the only font committed (with its OFL text); others come from `CRAFT_FONTS_DIR`.
- Shell gotcha seen on maintainer machines: `mv`/`cp` may be aliased to interactive mode — use
  `/bin/mv -f` and `/bin/cp -f` in scripts.

## 10. Debugging tips

Adapted from PhotoCraft `docs/development.md` (revision 59684266) and LightCraft `README.md` / `docs/gpu-pipeline.md` (revision 88ea1a8); MIT OR Apache-2.0.

### Logs

- The desktop app writes its `log` records to standard error **and** to `logs/dalica.log` in the
  settings directory (Linux `$XDG_CONFIG_HOME/dalica/logs/`, by default `~/.config/dalica/logs/`;
  macOS `~/Library/Application Support/Dalica/logs/`; Windows `%APPDATA%\Dalica\logs\`), never in a
  watched folder or the library directory. A launch from a desktop menu or the Dock has no terminal,
  so this file is what users attach to a bug report (**Help → Open Log Folder** shows it).
- Each launch moves the previous log to `dalica.1.log` (and that one to `dalica.2.log`), so the log
  of a run that crashed survives the next start. The file stops growing at 16 MiB. `--version`,
  `--help` and command-line errors write no file. The logger lives in `apps/dalica/src/logging.rs`
  and writes through `dalica-fs` (`fs::Appender`, layer rule 4) from its own writer thread, so a
  record never touches the disk on the thread that logs it; the run's first line names the file.
  When the file reaches its cap it ends with a one-line notice.
- Default levels: Dalica's own crates at `info`, everything else at `warn`. `RUST_LOG` replaces the
  default with env_logger-style directives: `RUST_LOG=debug`, `RUST_LOG=warn,dalica_scan=trace`,
  `RUST_LOG=info,wgpu_core=warn`; a directive ending in `*` covers every target starting with it
  (`dalica*=debug`).
- **Privacy in logs:** never log face data (names, boxes, embeddings), captions, tags or anything a
  user typed; log counts and ids instead ("named 12 faces in 3 photos"). Paths are allowed (they are
  local), credentials and tokens never. A test runs a people/caption workflow with `RUST_LOG=trace`
  and asserts that none of its names or captions appear in the captured log.

### Profiling: `DALICA_PROFILE=1`

`DALICA_PROFILE=1` prints per-stage timings to the log (and standard error) as work completes:

- image jobs: sniff, decode (with the DCT scale used), orientation, ICC → sRGB, resample, each recipe
  operation in `edit::render`, texture upload, with the job's priority and queue wait;
- scans: directories walked, files stat'ed, sidecars read, `Op`s applied per frame;
- UI frames slower than 8 ms: the panels that ran and their cost (cells visited, rows laid out).

It is cheap enough to leave on while reproducing a slowdown, but it is never on by default. The same
counters are available live through the control channel (`ui.inspect` → `perf`), which is what
tests and agents should read. For a whole-process view use an external sampling profiler
(`samply record target/release/dalica …`, or `cargo flamegraph`) on a release build with
`CARGO_PROFILE_RELEASE_DEBUG=true`; never commit profiler output that contains personal paths.

### Panics

- Non-test code never panics (AGENTS.md §3). The last-resort guard is a panic hook plus
  `catch_unwind` around command dispatch and per-file scan/import work: it turns an escaped panic
  into an error toast, records the panic message, location and backtrace in `dalica.log`, and also
  appends it to `dalica-panics.log` in the OS temp folder (so it survives when the settings folder is
  unwritable).
- `RUST_BACKTRACE=1` makes the backtrace complete in both files. Every panic report is a bug: write
  the regression test that panicked before the fix (AGENTS.md §2), usually by replaying the command
  and params from the log through `Session::execute` or by adding the input file's shape to the
  malformed-input tests.
- `cargo test -p dalica-engine --test panic_hunt -- --ignored` reproduces most command-level panics.

### Headless snapshot

The CPU rasterizer renders the real UI without a window, GPU or display, so it works in CI, over SSH
and with the display asleep:

```sh
cat > /tmp/tour.jsonl <<'JSON'
{"method":"ui.clickWidget","params":{"id":"tree:folder:1"}}
{"method":"engine.execute","params":{"command":"photo.star.toggle","params":{"ids":[3]}}}
{"method":"ui.menu.invoke","params":{"path":"View/People"}}
JSON
cargo run --release -p dalica-cli -- snapshot --demo --script /tmp/tour.jsonl \
    -o /tmp/after.png --size 1400x900 --scale 1
cargo xtask snapshot /tmp/tour.jsonl          # same, with the default output path under target/
```

Each line is one control-protocol call, applied in order; input calls reply only after the app has
processed them, so a following `ui.state` or `ui.inspect` observes their effect. Render at
1400 × 900 and 900 × 600 (and `--scale 2` for high-DPI checks), then look at the PNG.

### Environment variables

| Variable | Effect |
|---|---|
| `RUST_LOG` | log levels for standard error and the log file (see *Logs*) |
| `RUST_BACKTRACE=1` | full backtraces in panic reports |
| `DALICA_PROFILE=1` | per-stage timings (see *Profiling*) |
| `DALICA_THEME_FILE=tokens.json` | **debug builds only:** live `theme::Tokens` overrides, re-read on change (keys are the field names in `crates/ui-kit/src/theme.rs`); compiled out of release builds |
| `CRAFT_FONTS_DIR` | optional fonts build input (see §12) |
| `DALICA_CORPUS_REPO` | local authoring clone of the corpus repository for `xtask corpus --local` (see §11) |
| `DALICA_GPU=0` | once a GPU path exists: force the CPU renderer for the whole process (see §14) |
| `DALICA_FACE_MODELS` | M29: alternative face-model folder (see [`faces.md`](faces.md)) |

## 11. Test corpora

Adapted from PhotoCraft `docs/development.md` → *Test corpora* (revision 59684266); MIT OR Apache-2.0.

Real files are the best oracles for decoders, metadata readers and writers, but they are large
binaries and must never enter this repository's history. They live in `corpus/` (gitignored), are
fetched at **pinned commits** and are checked against committed **sha256 manifests**.

- **Our own corpus** is a separate repository, `dalica-corpus`, holding only CC0 or public-domain
  photos (and files we generated ourselves from them with other applications, for metadata
  interop). Third-party test suites with their own permissive or public-domain terms (PngSuite, raw
  camera samples published as CC0) are fetched from their upstreams the same way and are never
  copied into `dalica-corpus`. Personal photos are never part of any corpus.
- All pins are in one file, `xtask/src/corpus_pins.rs`; each corpus has a manifest
  `xtask/corpus/<name>.sha256`. Planned sets:

| Directory | What | Used by |
|---|---|---|
| `corpus/photos/` | CC0 JPEGs from many cameras and phones: orientations 1–8, EXIF thumbnails, MPF previews, ICC profiles, CMYK, progressive, truncated | `dalica-image`, `dalica-thumbs` |
| `corpus/metadata/` | the same photos after other applications wrote captions, keywords, ratings, GPS, dates and MWG face regions into them (and `.xmp` sidecars for RAW) | `dalica-meta` interop and round-trip tests ([`xmp-interop.md`](xmp-interop.md)) |
| `corpus/raw/` | small CC0 raw files per vendor, for embedded-preview extraction | `dalica-image` |
| `corpus/heif/` | small HEIC/HEIF files, for the `heif` feature | `dalica-image` |
| `corpus/pngsuite/` | PngSuite | `dalica-image` |
| `corpus/public-faces/` | public-domain portraits with their sources in `SOURCES.tsv`, for M29 | `dalica-faces` ([`faces.md`](faces.md)) |

```sh
cargo xtask corpus                  # where each corpus lives, its pin, present / missing / stale
cargo xtask corpus --all            # fetch everything missing or stale (verified copies are left alone)
cargo xtask test-corpus             # fetch, then cargo test --release --features corpus on the corpus crates
cargo xtask test-corpus -p meta     # narrow to one crate
cargo xtask corpus --local          # copy from ../dalica-corpus (or $DALICA_CORPUS_REPO) instead of downloading
```

**Opt-in locally, strict when opted in, always in CI.**

- The corpus tests sit behind a `corpus` cargo feature of the crates that use them (`dalica-image`,
  `dalica-meta`, `dalica-thumbs`, later `dalica-faces`), so plain `cargo test` and `cargo xtask ci`
  neither compile nor need them.
- With the feature on, a missing or stale corpus is a **failure** ("run `cargo xtask corpus --all`"),
  never a silent skip, and every floor is enforced.
- **Floors only rise.** Each corpus test has committed floors (files decoded, thumbnails extracted,
  metadata fields read, round trips byte-identical outside the metadata segments). Raise a floor in
  the PR that improves it; never lower one. A file that newly fails is investigated, and a synthetic
  reproduction goes into the crate's ordinary tests (corpus files are never committed or quoted).
- If you touch `image`, `meta`, `thumbs` or `scan`, run `cargo xtask test-corpus` before you finish.
- The CI `corpus` job runs `cargo xtask corpus --all` and `cargo xtask test-corpus` on every PR and
  on main, with `corpus/` cached under a key of `corpus_pins.rs` plus the manifests; a restored cache
  is re-verified (sha256) before the tests run.
- **Bumping a pin:** commit and push in `dalica-corpus`, then in a PR here update the commit in
  `corpus_pins.rs`, run `cargo xtask corpus --update-manifest <name>`, commit the manifest diff, run
  `cargo xtask test-corpus`, and raise floors that improved.
- **Not a submodule or a subtree:** a subtree puts the binaries back into this history, and
  submodules cost every contributor init and detached-HEAD friction. A pin plus a manifest gives the
  same reproducibility.
- **Never commit media:** no photos, videos, raw files, model weights or corpus excerpts in this
  repository, not even small ones. Unit tests use procedural images (`dalica-scenes`) or bytes built
  in the test. `cargo xtask assets` rejects media files that have no `ATTRIBUTION.md` row.

## 12. Fonts (`CRAFT_FONTS_DIR`)

Adapted from PhotoCraft `docs/development.md` → *Fonts* (revision 59684266); MIT OR Apache-2.0.

Inter is the only font committed here (with its OFL text and an `ATTRIBUTION.md` row). CJK and other
fallback fonts shared by the Craft apps live in the separate `craft-fonts` repository and are an
**optional build input**, never a Cargo dependency:

```sh
git clone https://github.com/storytold/craft-fonts ../craft-fonts
CRAFT_FONTS_DIR="$PWD/../craft-fonts" cargo run --release -p dalica
CRAFT_FONTS_DIR="$PWD/../craft-fonts" cargo test --workspace     # also runs the CJK font tests
```

- Use an **absolute** path (`$PWD/…`): `build.rs` runs inside `crates/ui`, so a relative path would
  resolve from there.
- `crates/ui/build.rs` reads `$CRAFT_FONTS_DIR/fonts/manifest.txt` and embeds the listed fonts as
  `dalica_ui::CRAFT_FONTS`. Unset, `CRAFT_FONTS` is empty and the build is unchanged. A bad path is a
  build warning, or an error with `CRAFT_FONTS_REQUIRED=1` (release builds set both).
- The UI installs Inter on the first frame and adds the CJK faces lazily, as fallbacks appended to
  every egui family in the script order of the UI language, the first time text needs them.
- Tests that need the fonts skip with a message when `CRAFT_FONTS` is empty; CI's Linux job runs them
  a second time with `CRAFT_FONTS_DIR` set. Releases check out `craft-fonts` at a pinned commit and
  ship each embedded font's `OFL.txt` as `OFL-<family>.txt`.
- Never copy a font file into this repository; new shared fonts go to `craft-fonts`.

## 13. Scorecard and performance budgets

Adapted from PhotoCraft `docs/development.md` → *Scorecard and performance budgets* and `docs/scorecard.md` (revision 59684266); MIT OR Apache-2.0.

Once M1 lands, Dalica keeps a generated `docs/scorecard.md` that says, with numbers, where it stands
per area. It is generated from committed files only (so it is deterministic) and never edited by
hand.

```sh
cargo xtask scorecard                # regenerate docs/scorecard.md
cargo xtask scorecard --check        # what CI runs: fails if the committed file is stale
cargo xtask perf                     # full benchmark run (release build)
cargo xtask perf --quick             # small synthetic libraries, about a minute once built
cargo xtask perf --update-baseline   # also write perf/baseline.json from this run
```

**Sources.**

- `scorecard/*.toml`: one checklist per area (library, editing, metadata interop, formats, UI,
  automation, reliability, distribution). Every item has an `id`, a short `target`, a `status`
  (`done`, `partial` or `missing`), the `parity` row or task id it belongs to, and a `note` with the
  evidence (`file:line`, test name). Statuses must be true on `main`: verify against the code, say
  `partial` with a note when unsure, and flip a row in the PR that changes it. Format:

  ```toml
  # Scorecard checklist: metadata interop. Statuses verified against <commit> on <date>.
  area = "metadata"
  title = "Metadata interop"
  order = 3

  [[item]]
  id = "META-1"
  target = "Caption and tags round-trip through XMP and IPTC with foreign fields byte for byte"
  status = "partial"
  task = "M10"
  note = "crates/meta/tests/roundtrip.rs covers JPEG; PNG/WebP not yet"
  ```

- `perf/budgets.toml`: the benches `xtask perf` runs and the scenarios P1…Pn (initially the table in
  §8), each mapped to one row of one bench, judged on a metric (`p50`, `p95` or `max`) against
  `budget_ms`. `enforce = false` marks a target the code does not meet yet (reported as over budget;
  only regressions fail); flip it to `true` in the PR that meets it. Rows with no budget yet are
  `track_only`. Format:

  ```toml
  [settings]
  regression_pct = 15.0      # a p50 regression beyond this share of the baseline fails…
  regression_min_ms = 1.0    # …and beyond this many milliseconds (timer noise)

  [[bench]]
  name = "library_bench"
  package = "dalica-ui"
  quick_args = ["--photos", "10000"]
  note = "synthetic 100 k-photo library: scroll, select, tree"

  [[scenario]]
  id = "P1"
  title = "Light table scroll, 100 k photos (UI frame time)"
  bench = "library_bench"
  row = "scroll light table 100k"
  metric = "p95"
  budget_ms = 8
  enforce = false

  [[scenario]]
  id = "P7"
  title = "Cold start to a usable library of 100 k photos"
  target = "≤ 2 s"
  not_measurable = "no cold-start harness yet (needs a fresh process and a dropped page cache)"
  ```

- `perf/baseline.json`: the numbers the scorecard shows, written **only** by
  `cargo xtask perf --update-baseline`, with the machine (CPU, RAM, GPU adapter, OS), its machine
  class, the commit, the date and the load average.
- Corpus floors (§11), parity counts (`docs/parity.md`) and counts taken from the tree (never-crash
  attribute coverage, options that no code reads, commands without tests).

**Never invent a number.** A scenario the code cannot run yet carries `not_measurable` with the
reason, never a guessed or extrapolated figure; the scorecard lists it under *Not measurable yet*.
Numbers quoted anywhere else in `docs/` come from the baseline or from a recorded run with its
machine, date and load average.

**Perf runs.** `xtask perf` builds the benches in release, runs each with `--json`, and merges the
reports into `target/perf/results.json` keyed by scenario id (p50, p95 and max by nearest rank, peak
RSS measured in-process, load average before and after). `target/perf/summary.md` is the Markdown
table. It exits non-zero when an enforced budget breaks, when a p50 regresses more than
`regression_pct` against a baseline from the **same machine class and mode**, or when a bench fails.
Runs from different machine classes are never compared, and `--quick` numbers only against a quick
baseline. A busy machine gives noisy numbers: check the load average before trusting a regression.

`xtask bench` (§3) remains the quick local before/after check for one hot path; `xtask perf` is the
budgeted, baseline-compared run that feeds the scorecard. CI runs `cargo xtask scorecard --check` on
every PR; `cargo xtask perf` runs nightly on one fixed runner, never on pull requests.

**Adding a scenario.** Add a row to a bench (its name is the key: keep it stable), give the bench
`--json` output, add a `[[scenario]]` to `perf/budgets.toml`, and run `cargo xtask scorecard`.

## 14. GPU rule

Adapted from LightCraft `docs/gpu-pipeline.md` (revision 88ea1a8); MIT OR Apache-2.0.

Dalica renders everything on the CPU first. The CPU renderer in `crates/edit` (`edit::render`) is
the **oracle**: the reference for thumbnails, the edit view, Save, Export and every test. A GPU path
(wgpu compute shaders for slow effects, a later option) is an accelerator, never a second source of
truth.

- **Equivalence:** a GPU path must match the CPU oracle within **1/255** per channel on the 8-bit
  sRGB output (max |Δ| ≤ 1 LSB), for every operation and parameter extreme it accelerates, all eight
  orientations and with crops. An equivalence test per operation renders both and compares; it
  skips with a note (and passes) when no adapter exists. Shared parameters (tables, kernel taps,
  plans) are computed once in `edit` and uploaded, so the two implementations cannot drift.
- **Fallback:** anything the GPU path cannot do, or does not complete correctly, renders on the CPU:
  no adapter or a software adapter, `DALICA_GPU=0`, a buffer over the device limit, out of device
  memory, a validation or internal error, a lost device, a readback timeout, or a result that fails
  its sanity check (unwritten pixels). Errors in one render fall back for that render; a lost device
  or internal error stops GPU use for the rest of the process. The user only ever sees the CPU
  result in those cases, never a black or partial image.
- Thumbnails and the many small jobs of the job pool stay on the CPU. GPU work is submitted in short
  pieces so no submission can trip a driver watchdog.
- `ui.inspect` → `perf.gpu` names the adapter, and `perf.gpuFallback` the latest CPU fallback and
  its reason; `DALICA_PROFILE=1` prints both with the stage timings. Tests inject failures to prove
  every fallback row.
