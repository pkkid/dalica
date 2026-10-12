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
| `xtask ci` | fmt check, clippy `-D warnings` (all targets, the acceptance package included), tests, `layers`, `deps`, `assets`, `names`, `parity`, `docs`, `contributors --check`, `tasks sync --check`, then `accept` for every milestone whose blocks are all ✅; each acceptance test runs once (below); runs every step, prints a table, and on a clean worktree records the green commit for `tasks submit` | — |
| `xtask layers` | checks crate dependencies against the layer table in `xtask/src/layers.rs` (`architecture.md` §2); unregistered crates fail; UI crates only in L5; only `crates/fs` calls `std::fs` write functions | ✓ |
| `xtask deps` | offline dependency audit (`security.md` §8): licence allow-list over the whole graph; on the product graph of `dalica` and `dalica-cli` for Linux, Windows and macOS, no banned crate, no C build script (reviewed exceptions are checked against the resolved features) and `*-sys` only from the allow-list; sources; duplicate versions warn | ✓ |
| `xtask assets` | every image/icon/font/sound file is matched by a row in `ATTRIBUTION.md`; referenced licence files exist; forbidden file names/extensions fail | ✓ |
| `xtask names` | fails if the reference product's name appears in any tracked file other than `AGENTS.md` | ✓ |
| `xtask parity [--write]` | ids, tiers and statuses of `docs/parity.md`; every `cmd:` id (from `dalica-cli commands`) and path it cites exists; the summary is current (`--write` refreshes it) | ✓ |
| `xtask docs` | every `cargo xtask …` command named in `AGENTS.md`, this file and `task-board.md` exists or is registered as planned | ✓ |
| `xtask accept M<n> [--headless]` | runs a milestone's acceptance test (`tests/acceptance/m<n>.rs`) and prints what a human should look at (`m<n>.md`) (`roadmap.md` → How milestones work); run on its own it checks the whole *Done when*, the crate suites it names included; finished milestones' checks run inside `ci` | ✓ (all finished milestones) |
| `xtask tasks <sub>` | the task board: `setup`, `sync`, `next`, `claim`, `env`, `submit`, `release`, `block`, `merge`, `status`, `log` ([`task-board.md`](task-board.md)); `sync --check` parses the roadmap offline, `setup`/`sync --dry-run` show the GitHub writes without making them | `sync --check` ✓ |
| `xtask corpus [--all \| <set>…] [--local] [--update-manifest]` | the test corpora of §11: without arguments, each set's pin and state (present, missing, stale, not published yet); with `--all` or set names, fetches what is missing or stale into the gitignored `corpus/`, every download checked against its pin and every file against `xtask/corpus/<set>.sha256` | |
| `xtask test-corpus [-p <crate>] [-- args]` | fetches like `corpus --all`, then runs each corpus crate's `tests/corpus.rs` with `--release --features corpus,heif`, holding every set to its committed floors; sets not published yet are reported as skipped (§11) | the CI `corpus` job |
| `xtask bench [name…] [-- args]` | builds the registered benches in release (`cargo bench`), runs each with `--json` (plus `args`), prints its rows, appends the run to `<target dir>/bench/history.jsonl` and fails on a CPU-time regression over 20 % against the last run of the same bench with the same arguments on the same machine (§13 *Bench runs*); no name runs them all | |
| `xtask budgets [--plant-ms N]` | builds in release and runs the frame budgets the milestones assert (§8 *The budgets job*): M4's 100 k scroll (`library_bench`) and thumb at top speed, UI frame time p95 ≤ 8 ms, cells visited flat; one line per budget, fails when one is missed; `--plant-ms` adds a planted slowdown to every frame | the `budgets` workflow |
| `xtask version [set X.Y.Z[-pre]]` | prints `[workspace.package] version` from the root `Cargo.toml`; `set` checks that the new value is a semantic version (a hyphen makes it a pre-release, `0.2.0-rc.1`; no build metadata), rewrites that line and the version line of every workspace member in `Cargo.lock` and nothing else, then asks `cargo metadata --locked --offline` whether the lock file is current and puts both files back if not ([`releasing.md`](releasing.md) → Cutting a release) | |
| `xtask contributors [--check] [--ref <rev>]` | regenerates `contributors/contributors.json` (About ▸ Contributors and Models) from `git log` on `main` (`origin/main` when it exists) and, through `gh` (read-only), each commit's GitHub username, the merged pull requests and, for verified `people.toml` entries that ask for it, the public profile name; models from `Co-Authored-By` trailers of known model vendors; never records an author name or email and never writes `people.toml`; `--check` verifies `people.toml` offline: malformed TOML, unknown tables and keys, and an entry added by someone other than its owner without `consent_source` ([`contributors.md`](contributors.md) → Refreshing) | `--check` ✓ |
| `xtask perf` | *planned (from M4):* the budgeted benchmark run of §13 | |
| `xtask scorecard [--check]` | *planned (after M1):* regenerate / verify `docs/scorecard.md` | |
| `xtask snapshot <script>` | *planned (M2):* headless UI screenshot via the CPU rasterizer | |
| `xtask stats` | *planned (not scheduled):* line/test/command counts for the status doc | |

Running a planned command says where it is planned and exits with status 2.

`xtask ci` uses one test thread and build job per GiB of available RAM, capped by
`CARGO_BUILD_JOBS` or the CPU count, so it doesn't take the machine down when several agents run it
at once.

**What `ci` runs, and only once.** Its steps, in order:

1. `test`: `cargo test --workspace --exclude dalica-acceptance`, every crate's suites.
2. `test acceptance`: the acceptance package's library and every `[[test]]` target of
   `tests/acceptance/Cargo.toml` that no `accept` step runs (`m0_skeleton`, and the acceptance
   test of a milestone still in progress). A finished milestone's headless half (`m1_headless`) is
   left out too: `m1.rs` runs the same steps first, so it would repeat them; it stays for
   `cargo xtask accept M1 --headless`.
3. `accept M<k>` for each finished milestone (every block ✅), each running `m<k>.rs` once.

An acceptance test re-runs the crate suites its milestone's *Done when* names (`cargo test -p …`),
so one `cargo xtask accept M<n>` checks everything. Inside `ci` those suites already ran in step 1,
so `ci` sets `DALICA_SUITES_RAN=1` on steps 2 and 3, and the tests skip them, printing `suites: run
by the ci test step`. A standalone `cargo xtask accept` clears the variable. What only an acceptance
test runs (the live app, its screenshots, `panic_hunt -- --ignored` in M1, the debug benches in M4)
always runs. Nested cargo runs in acceptance tests never see `DALICA_BLESS`
(`dalica_acceptance::cargo`), so blessing a milestone's own baselines never rewrites a crate's.

**Optimised crates in dev and test builds.** The root `Cargo.toml` raises `opt-level` to 3 for the
crates that tests spend their time in: the test-photo generator and codecs (`dalica-scenes`,
`dalica-image`, `image`, `png`, `zune-jpeg`, `zune-core`, `fdeflate`, `miniz_oxide`, `crc32fast`,
`simd-adler32`, `adler2`), egui's layout and tessellation (`egui`, `epaint`, `emath`, `ecolor`),
the CPU rasterizer behind every screenshot and fingerprint (`dalica-ui-kit`) and the thumbnail
ladder (`dalica-thumbs`, where `image`'s generic JPEG encoder is compiled, so it runs at this
crate's level). Only `opt-level` changes: `debug-assertions` and `overflow-checks` stay on in every
dev and test build, our optimised crates included. Measured on the maintainer desktop (warm, 8
threads): `dalica-ui --test never_blank` 31 → 11 s, `--test layout` 26 → 3 s, `dalica-ui-kit
--test badges` 14 → 1 s. A crate joins this list only with a measurement that shows it helps.

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
- CLI scripts (M1.15): `dalica-cli run --library DIR script.jsonl` (or `-` for stdin) runs one
  `{"command": id, "params": {...}}` per line and prints one JSON result per line;
  `dalica-cli run --library DIR --cmd ID --params '{…}'` runs one command;
  `dalica-cli inspect --library DIR` summarises a library. Exit codes: 0 success, 1 a command
  failed, 2 a bad argument or script line, 3 the library could not be opened (for example, it is
  open in another process). `cargo xtask accept M<n> --headless` runs `tests/acceptance/m<n>_headless.rs`
  when the milestone has one.
- MCP: `claude mcp add dalica -- "$PWD/target/release/dalica-cli" mcp --demo` (headless) or
  `… mcp --connect 127.0.0.1:7990` (the running app).
- Never send OS-level synthetic input.

## 8. Performance budgets (initial)

| Scenario | Budget |
|---|---|
| UI frame time, light table with 100 k photos, scrolling | ≤ 8 ms (p95) |
| Grey placeholder cells while scrolling a scanned 100 k-photo library at top thumb speed | 0 |
| First thumbnails visible after adding a folder of 1,000 JPEGs | ≤ 1 s |
| Initial scan throughput (embedded thumbnails available) | ≥ 500 photos/s on SSD |
| Open a 24 MP JPEG in the edit view (screen-size preview) | ≤ 150 ms |
| Slider drag update (screen-size preview) | ≤ 16 ms draft, ≤ 100 ms final |
| Next/previous photo in the edit view (prefetched) | ≤ 50 ms |
| Cold start to a usable library of 100 k photos | ≤ 2 s |
| Idle memory with 100 k photos | ≤ 600 MiB |

Measure, don't guess: record numbers in `docs/status.md` → Metrics with the machine and date.

**The budgets job.** `.github/workflows/budgets.yml` runs `cargo xtask budgets` on every push to
`main`, on Linux only (`ubuntu-latest`), with its own release cache: it builds in release and runs
the budgets the acceptance tests assert only with `DALICA_ACCEPT_RELEASE=1`, and the run fails when
one is missed. Today (M4) that is the 100 k scroll of `library_bench` in full (UI frame time p95
≤ 8 ms; cells visited per frame flat from 10 k to 100 k) and `m4.rs`'s thumb held at its top speed
through 100 k sketched photos (p95 ≤ 8 ms, no placeholder, cells flat); since M5 also the first
screenful of a just-added folder of 1 k and of 10 k JPEGs (≤ 1 s each, `first_thumbnails`) and a
scanned 20 k pass at top thumb speed (no placeholder, p95 ≤ 8 ms); the scanned 100 k pass runs in
the release acceptance only. A milestone adds its budgets to `BUDGETS` in `xtask/src/accept.rs`. A pull request that changes the
workflow or `xtask/src/accept.rs` runs the job too, and *Run workflow* on the Actions tab runs it on
any branch with a planted slowdown (`plant_ms`). The same check locally:

```sh
cargo xtask budgets                  # release build, then every budget; exit 1 when one is missed
cargo xtask budgets --plant-ms 8     # a planted 8 ms in every frame: must fail
DALICA_ACCEPT_RELEASE=1 cargo xtask accept M4   # the whole M4 acceptance, these budgets included
```

A planted slowdown of *n* ms in every frame moves every percentile of the frame times by exactly
*n*, so `--plant-ms` adds *n* to each measured p95 before judging it; the `xtask` tests plant
slow reports and slow runs and check that the job fails.

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
  macOS `~/Library/Application Support/Dalica/logs/`; Windows `%APPDATA%\Dalica\logs\`; in portable
  mode `data/logs/` beside the executable), never in a watched folder or the library directory. A launch from a desktop menu or the Dock has no terminal,
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
| `DALICA_SUITES_RAN=1` | set by `cargo xtask ci` on its acceptance steps: the acceptance tests skip the crate suites its test step already ran (§3) |
| `DALICA_CACHE_DIR` | an absolute folder used in place of Dalica's cache folder for the thumbnail cache (`<dir>/thumbs`); every test that launches `dalica` or `dalica-cli` on a library sets it to a temporary folder ([`data-model.md`](data-model.md) §4) |
| `DALICA_HOME` | an absolute folder used as the default library (its settings and index) when `dalica` is started without `--library`; wins over portable mode (below) |
| `DALICA_BUILD_SHA`, `DALICA_BUILD_DATE` | **build time:** the commit and date `dalica --version`, `dalica-cli --version` and About report (`dalica_core::build`); CI and the packaging scripts set them, a plain `cargo build` reports `<version> (dev build)`; changing either rebuilds `dalica-core` ([`releasing.md`](releasing.md)) |
| `DALICA_THEME_FILE=tokens.json` | **debug builds only:** live `theme::Tokens` overrides, re-read on change (keys are the field names in `crates/ui-kit/src/theme.rs`); compiled out of release builds |
| `CRAFT_FONTS_DIR` | optional fonts build input (see §12) |
| `DALICA_CORPUS_REPO` | local authoring clone of the corpus repository for `xtask corpus --local` (see §11) |
| `DALICA_GPU=0` | once a GPU path exists: force the CPU renderer for the whole process (see §14) |
| `DALICA_FACE_MODELS` | M13: alternative face-model folder (see [`faces.md`](faces.md)) |

### Portable mode

A file named `portable` beside the `dalica` executable (the Windows portable zip ships one) keeps
the default library, with its settings and index, in `data/library` and the thumbnail cache in
`data/cache/thumbs` and the log in `data/logs` next to the executable instead of the platform
folders (`dalica-cli` beside a marker uses the same cache folder); sidecars stay in the photo
folders as always. `DALICA_HOME` and `DALICA_CACHE_DIR` still win when set. If `data` cannot
be created or written there, Dalica prints why and uses the platform folders. The resolution lives
in `apps/dalica/src/paths.rs`.

## 11. Test corpora

Adapted from PhotoCraft `docs/development.md` → *Test corpora* (revision 59684266); MIT OR Apache-2.0.

Real files are the best oracles for decoders, metadata readers and writers, but they are large
binaries and must never enter this repository's history. They live in `corpus/` (gitignored), are
fetched from **pins** (a URL with its sha256, or a commit) and are checked against committed
**sha256 manifests**.

- **Our own corpus** is a separate repository, `pkkid/dalica-corpus`, holding only CC0 or
  public-domain photos (and files we generated ourselves from them with other applications, for
  metadata interop), each with its source. Third-party test suites with their own permissive or
  public-domain terms (PngSuite, raw camera samples published as CC0) are fetched from their
  upstreams the same way and are never copied into `dalica-corpus`. Personal photos are never part
  of any corpus.
- All pins are in one file, `xtask/src/corpus_pins.rs`; each set has a manifest
  `xtask/corpus/<set>.sha256` (`<sha256>  <path>` per line, sorted by path). Sets:

| Directory | What | Source and pin | Used by |
|---|---|---|---|
| `corpus/pngsuite/` | PngSuite: 176 PNGs (every bit depth, colour type, interlacing and ancillary chunk, 14 corrupt on purpose) | the 2017-07-19 release archive from schaik.com, by URL and sha256 (its own permissive terms) | `dalica-image` |
| `corpus/raw/` | 26 camera raws, two per kind Dalica reads previews from: CR2, CR3, CRW, DNG, NEF, ARW, PEF, ORF, RW2, SRW, RAF, MRW, X3F (245 MB) | raw.pixls.us, each file CC0, by URL and sha256 | `dalica-image` |
| `corpus/photos/` | CC0 JPEGs from many cameras and phones: orientations 1–8, EXIF thumbnails, MPF previews, ICC profiles, CMYK, progressive, truncated | `photos/` of `dalica-corpus` at a pinned commit: **not published yet** | `dalica-image`, `dalica-thumbs` |
| `corpus/heif/` | small HEIC/HEIF files, for the `heif` feature | `heif/` of `dalica-corpus` at a pinned commit: **not published yet** | `dalica-image` |
| `corpus/metadata/` | the photos after other applications wrote captions, keywords, ratings, GPS, dates and MWG face regions into them (and `.xmp` sidecars for RAW) | planned, `dalica-corpus` | `dalica-meta` interop and round-trip tests ([`xmp-interop.md`](xmp-interop.md)) |
| `corpus/public-faces/` | public-domain portraits with their sources in `SOURCES.tsv`, for M13 | planned, `dalica-corpus` | `dalica-faces` ([`faces.md`](faces.md)) |

```sh
cargo xtask corpus                  # each set: its directory, pin and state (present / missing / stale / not published yet); no network
cargo xtask corpus --all            # fetch every published set that is missing or stale (verified copies are left alone)
cargo xtask corpus raw              # the same for some sets
cargo xtask test-corpus             # fetch, then each corpus crate's tests/corpus.rs (--release --features corpus,heif)
cargo xtask test-corpus -p image    # narrow to one crate
cargo xtask corpus --local --all    # our own sets copied from ../dalica-corpus (or $DALICA_CORPUS_REPO) instead of downloaded
cargo xtask corpus --update-manifest raw   # after moving a pin: refetch from the pins and rewrite the manifest
```

- **Verified before use.** A download is checked against its pin before it is used (an archive
  before it is unpacked, a single file before it is moved into place); a file whose sha256 differs
  is deleted and refused. A set is then checked file by file against its manifest in a staging
  directory before it replaces `corpus/<set>/`, so a stale or altered file never passes: it shows as
  "stale" in `cargo xtask corpus` and fails the corpus tests until `--all` fetches it again.
  `corpus/<set>/SOURCES.md` says where the files came from and under which terms.
- **Network only on request.** Only `cargo xtask corpus` and `cargo xtask test-corpus` download,
  through `curl` (generic User-Agent, retries, 512 MiB cap per file) and `tar`, which ship with
  Linux, macOS and Windows 10 and later. `cargo xtask ci`, builds and the ordinary tests never touch
  the network or the corpus.
- **Not published yet.** A set of our own repository with no pinned commit is reported as "not
  published yet" by `cargo xtask corpus`, skipped by `--all`, and skipped with that message (never
  counted as passed) by `cargo xtask test-corpus`; it has no manifest and no floors. Publishing
  one: commit it in `dalica-corpus`, pin the commit in `corpus_pins.rs` (`DALICA_CORPUS_COMMIT`),
  run `cargo xtask corpus --update-manifest <set>`, commit the manifest, run
  `cargo xtask test-corpus` and add the set's floors at the measured counts. Before pushing there,
  `--local --update-manifest <set>` tests a clone's files (it warns when the clone is not at the pin).

**Opt-in locally, strict when opted in, always in CI.**

- The corpus tests sit in each crate's `tests/corpus.rs` behind a `corpus` cargo feature
  (`dalica-image` today; `dalica-meta`, `dalica-thumbs` and later `dalica-faces` when their tests
  land), so plain `cargo test` and `cargo xtask ci` neither compile nor need them.
- With the feature on, a missing or stale corpus is a **failure** ("run `cargo xtask corpus --all`"),
  never a silent skip, and every floor is enforced.
- **Floors only rise.** Each crate commits floors per set (`crates/image/tests/corpus_floors.json`:
  files, files sniffed as their extension says, headers read the way scanning reads them, full
  decodes, 256 px thumbnails, embedded thumbnails decoded, and for PngSuite the corrupt files
  refused). A measured set without floors, a floor without a manifest and a count below its floor
  all fail; a count above its floor is printed so the PR that improved it raises it. Never lower
  one. A file that newly fails is investigated, and a synthetic reproduction goes into the crate's
  ordinary tests (corpus files are never committed or quoted).
- If you touch `image`, `meta`, `thumbs` or `scan`, run `cargo xtask test-corpus` before you finish.
- The CI `corpus` job (Linux) runs `cargo xtask corpus --all`, clippy of `dalica-image` with the
  `corpus` and `heif` features, and `cargo xtask test-corpus` on every PR and on main, with
  `corpus/` cached under a key of `corpus_pins.rs` plus the manifests; `--all` re-hashes a restored
  cache before the tests run.
- **Bumping a pin:** for our own sets, commit and push in `dalica-corpus`, then in a PR here update
  the commit in `corpus_pins.rs`; for an upstream file or archive, change its URL and sha256. Run
  `cargo xtask corpus --update-manifest <set>`, commit the manifest diff, run
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
  task = "M6"
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
budgeted, baseline-compared run that feeds the scorecard.

**Bench runs.** The benches `xtask bench` knows are registered in `xtask/src/bench.rs` (a
`harness = false` bench target each): `library_bench` (`dalica-ui`) and `pack` (`dalica-thumbs`).
A bench given `--json` prints one JSON object on its last line with a `rows` array; each row has a
`name` (the key) and numbers. A row's **CPU-time fields** are those whose key names a time unit
(`p50_ms`, `pack_us_per_read`): `xtask bench` compares them with the last run in the history that
has the same bench, arguments and machine (OS, architecture, CPU count and model, host name) and
flags any that grew more than 20 %. Tail statistics (`p95_…`, `p99_…`, `max_…`) and counters are
printed, never flagged. Every successful run is appended, so a flagged run becomes the next
baseline: rerun on a quiet machine before trusting a regression. The history lives in the target
directory (`CARGO_TARGET_DIR` when set), never in the repository.

```sh
cargo xtask bench library_bench                      # 10 k and 100 k photos, 4 × 300 wheel scrolls each
cargo xtask bench library_bench -- --photos 20000    # 2 k and 20 k; other arguments are another series
cargo bench -p dalica-ui --bench library_bench       # the bench alone, a table instead of JSON
```

`library_bench` opens an index-only synthetic library (`dalica_engine::synthetic`, 100 photos per
folder) at a tenth of `--photos` and at `--photos` (default 100 000) in the headless driver at
1400 × 900, and scrolls the light table with the wheel at 3 600 points per second from four places
(the top and a quarter, half and three quarters down). Rows: `scroll light table <n>` (UI frame
time, layout plus tessellation, p50/p95/max by nearest rank; cells visited per frame, min/p50/max),
`sections refresh <n>` (rebuilding the light table's sections, once per library revision) and
`layout <n>`. It exits non-zero when the cells visited per frame grow with the library (the larger
library's maximum more than 10 % over the smaller one's). Without `--bench` (`cargo test --benches`)
it runs a smoke pass at 1 k and 10 k photos. CI runs `cargo xtask scorecard --check` on
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
