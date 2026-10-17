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
cargo test -p <crate>                               # tests for one crate, quick
cargo xtask ci                                      # a task's gate: run before submit (minutes)
cargo xtask ci --full                               # the review gate: every test at full strength (once per milestone)
```

## 3. `cargo xtask`

| Command | What it does | In `ci` |
|---|---|---|
| `xtask ci [--merge \| --full [--platform]]` | the gates (below): plain, a task's gate (fmt check, clippy `-D warnings` over the whole workspace with all targets, the static checks `layers`, `deps`, `assets`, `names`, `parity`, `docs`, `contributors --check`, `tasks sync --check`, and the tests of the packages the branch changed, quick); `--merge`, the same without tests; `--full`, the review gate; runs every step, prints a table with each step's time, and on a clean worktree records the green commit for `tasks submit` | — |
| `xtask layers` | checks crate dependencies against the layer table in `xtask/src/layers.rs` (`architecture.md` §2); unregistered crates fail; UI crates only in L5; only `crates/fs` calls `std::fs` write functions | ✓ |
| `xtask deps` | offline dependency audit (`security.md` §8): licence allow-list over the whole graph; on the product graph of `dalica` and `dalica-cli` for Linux, Windows and macOS, no banned crate, no C build script (reviewed exceptions are checked against the resolved features) and `*-sys` only from the allow-list; sources; duplicate versions warn | ✓ |
| `xtask assets` | every image/icon/font/sound file is matched by a row in `ATTRIBUTION.md`; referenced licence files exist; forbidden file names/extensions fail | ✓ |
| `xtask names` | fails if the reference product's name appears in any tracked file other than `AGENTS.md` | ✓ |
| `xtask test-layout` | the tests' shape (maintainer decision, 2026-10-15): every crate with integration tests is one binary (`autotests = false`, `tests/all.rs` declaring every `tests/*.rs`; separate targets only where `SEPARATE` in `xtask/src/test_layout.rs` says why); no per-milestone test files, existing or planned in an open task block (`LEGACY` lists the old ones and only shrinks); no proptest case count over 32 written as a literal; no fingerprint baselines at the small window size | ✓ |
| `xtask parity [--write]` | ids, tiers and statuses of `docs/parity.md`; every `cmd:` id (from `dalica-cli commands`) and path it cites exists; the summary is current (`--write` refreshes it) | ✓ (the `cmd:` ids against the registry only in `--full`) |
| `xtask docs` | every `cargo xtask …` command named in `AGENTS.md`, this file and `task-board.md` exists or is registered as planned | ✓ |
| `xtask accept M<n> [--headless]` | runs a milestone's acceptance test (`tests/acceptance/m<n>.rs`) at full strength and prints what a human should look at (`m<n>.md`) (`roadmap.md` → How milestones work); run on its own it checks the whole *Done when*, the crate suites it names included | `--full` (all finished milestones) |
| `xtask tasks <sub>` | the task board: `setup`, `sync`, `next`, `claim`, `env`, `submit`, `release`, `block`, `merge`, `status`, `log` ([`task-board.md`](task-board.md)); `sync --check` parses the roadmap offline, `setup`/`sync --dry-run` show the GitHub writes without making them | `sync --check` ✓ |
| `xtask corpus [--all \| <set>…] [--local] [--update-manifest]` | the test corpora of §11: without arguments, each set's pin and state (present, missing, stale, not published yet); with `--all` or set names, fetches what is missing or stale into the gitignored `corpus/`, every download checked against its pin and every file against `xtask/corpus/<set>.sha256` | |
| `xtask test-corpus [-p <crate>] [-- args]` | fetches like `corpus --all`, then runs each corpus crate's `tests/corpus.rs` with `--release --features corpus,heif`, holding every set to its committed floors; sets not published yet are reported as skipped (§11) | the CI `corpus` job |
| `xtask bench [name…] [-- args]` | builds the registered benches in release (`cargo bench`), runs each with `--json` (plus `args`), prints its rows, appends the run to `<target dir>/bench/history.jsonl` and fails on a CPU-time regression over 20 % against the last run of the same bench with the same arguments on the same machine (§13 *Bench runs*); no name runs them all | |
| `xtask budgets [--plant-ms N]` | builds in release and runs the frame budgets the milestones assert (§8 *The budgets job*): M4's 100 k scroll (`library_bench`) and thumb at top speed, UI frame time p95 ≤ 8 ms, cells visited flat; one line per budget, fails when one is missed; `--plant-ms` adds a planted slowdown to every frame | the `budgets` workflow |
| `xtask version [set X.Y.Z[-pre]]` | prints `[workspace.package] version` from the root `Cargo.toml`; `set` checks that the new value is a semantic version (a hyphen makes it a pre-release, `0.2.0-rc.1`; no build metadata), rewrites that line and the version line of every workspace member in `Cargo.lock` and nothing else, then asks `cargo metadata --locked --offline` whether the lock file is current and puts both files back if not ([`releasing.md`](releasing.md) → Cutting a release) | |
| `xtask contributors [--check] [--ref <rev>]` | regenerates `contributors/contributors.json` (About ▸ Contributors and Models) from `git log` on `main` (`origin/main` when it exists) and, through `gh` (read-only), each commit's GitHub username, the merged pull requests and, for verified `people.toml` entries that ask for it, the public profile name; models from `Co-Authored-By` trailers of known model vendors; never records an author name or email and never writes `people.toml`; `--check` verifies `people.toml` offline: malformed TOML, unknown tables and keys, and an entry added by someone other than its owner without `consent_source` ([`contributors.md`](contributors.md) → Refreshing) | `--check` ✓ |
| `xtask clean [--dry-run] [--keep <name>]… [--only <name>]… [--older-than <days>]` | removes the cargo build dirs of the main checkout to free disk space, skipping any cargo is building in (§9 *Freeing disk space*) | |
| `xtask perf` | *planned (from M4):* the budgeted benchmark run of §13 | |
| `xtask scorecard [--check]` | *planned (after M1):* regenerate / verify `docs/scorecard.md` | |
| `xtask snapshot <script>` | *planned (M2):* headless UI screenshot via the CPU rasterizer | |
| `xtask stats` | *planned (not scheduled):* line/test/command counts for the status doc | |

Running a planned command says where it is planned and exits with status 2.

`xtask ci` uses one test thread and build job per GiB of available RAM, capped by
`CARGO_BUILD_JOBS` or the CPU count, so it doesn't take the machine down when several agents run it
at once.

**Three gates: test little during a milestone, everything once at its review** (maintainer
decision, 2026-10-15).

| Gate | Who runs it | Steps |
|---|---|---|
| `cargo xtask ci` | a worker, before `tasks submit` (minutes) | fmt, clippy over the whole workspace with all targets (every crate and every test still compiles), the static checks, and the tests of only the packages the branch changed against `origin/main`, quick |
| `cargo xtask ci --merge` | `tasks merge`, after its rebase | fmt, clippy, the static checks; no tests (the worker's gate ran them on the same changes, and tasks never share files) |
| `cargo xtask ci --full` | the lead at the review (`/milestone-review`), and the review's GitHub run on Linux | every test of every package at full strength, the heif decoder's tests, `parity` with every cited `cmd:` id checked against the registered commands, and the acceptance test of every finished milestone |
| `cargo xtask ci --full --platform` | the review's GitHub run on Windows and macOS | `--full` without what gives the same answer on every system: fmt, the static checks, the heif tests, and the pure packages (`dalica-core`, `-library`, `-scenes`, `-edit`, `-image`, `-meta`, `-sidecar`, `xtask`) |

**Which packages a task's gate tests.** A file belongs to the workspace member whose directory
holds it; the gate tests those packages. A change to the root `Cargo.toml`, `Cargo.lock`,
`clippy.toml`, the toolchain file, `deny.toml`, `.cargo/` or a file outside every package tests
every package; a change only to docs (Markdown, `docs/`, `.github/`, `.claude/`) tests none.
Dependents of a changed package are not tested: clippy still compiles them and their tests, and a
behaviour break there surfaces at the review.

**Quick and full strength.** `DALICA_FULL_TESTS=1` runs tests at full strength: every fault
point, full proptest case counts, full-size scale tests, exhaustive walks. `ci --full`, a
standalone `cargo xtask accept M<n>` and the corpus job set it. Unset (a task's gate, a plain
`cargo test`), tests run quick: one fault point per class, at most 32 proptest cases, small sizes.
A test with a heavy variant reads the variable and picks its size; the quick variant still checks
the same behaviour.

**What `ci --full` runs, and only once.** Its test steps, in order:

1. `test`: `cargo test --workspace --exclude dalica-acceptance`, every crate's suites.
2. `test heif`: `dalica-image`'s `heif` tests with the optional decoder, and since M11.26 the
   engine's Save of a HEIC photo, which only a build with the decoder can make (`cargo test -p
   dalica-engine --features heif --test all save_formats::`); both run even when the first fails
   (Linux only).
3. `test acceptance`: the acceptance package's library and every `[[test]]` target of
   `tests/acceptance/Cargo.toml` that no `accept` step runs (`m0_skeleton`, and the acceptance
   test of a milestone still in progress). A finished milestone's headless half (`m1_headless`) is
   left out too: `m1.rs` runs the same steps first, so it would repeat them; it stays for
   `cargo xtask accept M1 --headless`. A task's gate runs this step when the branch changed the
   acceptance package.
4. `accept M<k>` for each finished milestone (every block ✅), each running `m<k>.rs` once.

An acceptance test re-runs the crate suites its milestone's *Done when* names (`cargo test -p …`),
so one `cargo xtask accept M<n>` checks everything. Inside `ci` those suites already ran in step 1,
so `ci` sets `DALICA_SUITES_RAN=1` on steps 3 and 4, and the tests skip them, printing `suites: run
by the ci test step`. A standalone `cargo xtask accept` clears the variable. What only an acceptance
test runs (the live app, its screenshots, the debug benches in M4) always runs. Nested cargo runs in
acceptance tests never see `DALICA_BLESS` (`dalica_acceptance::cargo`), so blessing a milestone's
own baselines never rewrites a crate's.

**On GitHub.** CI (`.github/workflows/ci.yml`: Linux `ci --full`, Windows and macOS
`ci --full --platform`, and the corpus job of §11), Budgets (§8) and Fuzz (`security.md` §7) run
only on the pull request of a `review/M<n>.R` branch and when dispatched by hand (*Run workflow* on
the Actions tab, any branch). A task's pull request and a push to `main` run no tests: every merge
is decided by the local gates (the packaging lint and the release dry run still run when
`packaging/`, the workflows or the app icon change; `releasing.md`). The lead dispatches CI on
`main` by hand after merging a task that is specific to one system (a Windows path fix, a macOS
menu); `tasks merge` refuses while the latest finished run on `main` is red. What fails at the review becomes fix tasks, merged before the
review closes.

**A shared compiler cache (optional).** When `sccache` is installed, `cargo xtask tasks env`
exports `RUSTC_WRAPPER=sccache` (with a 40 GB cache), so the eight agents' target directories share
compiled dependencies instead of building them once each and again after each cold rebase. Workspace
crates build incrementally, which sccache passes by. Without sccache nothing changes.

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
5. Tests: behaviour, undo/redo, disabled state, and a headless UI test if it has UI. Bad params
   (must return `Err`, never panic) need no test of their own: one engine test loops over every
   registered command, and `panic_hunt` generates params from each command's spec.
6. UI: compose existing widgets; register automation ids; verify by screenshot.
7. `cargo xtask ci` (the task gate); tick the task's ✅ and update its parity row; commit with the
   task id; `cargo xtask tasks log` and `tasks submit` (`AGENTS.md` §5).

## 6. Tests

- **Unit/property tests** next to the code (`proptest` for parsers, sidecar round trips, geometry).
- **Round-trip tests** for every writer: sidecar, XMP/IPTC write-back, Save-with-backup, export.
- **Malformed-input tests** for every reader (truncated JPEG/EXIF, hostile sidecars, huge counts).
- **Hostile params, every command:** one engine test loops over every registered command with
  empty, out-of-range and wrong-type params, and **`panic_hunt`** (an ordinary `dalica-engine`
  test) generates params from each command's spec; both must stay green.
- **Headless UI tests**: drive the real UI through the headless driver (click by automation id,
  `step_until(condition)`, never "N quiet frames"), assert state and counters (cells visited per
  frame, jobs requested).
- **Workflow acceptance tests** (`crates/mcp/tests/workflows.rs`): end-to-end user tasks over MCP,
  checked only through public state and output files.
- **Library-scale tests**: a synthetic 100 k-photo library must scroll with < 8 ms/frame UI time.
- Test photos are procedural (`dalica-scenes`, M1) or CC0 from the pinned corpus. Never commit
  media; never use personal photos.

**Writing tests** (maintainer decision, 2026-10-15). Every test costs every run, so:

- **One test per behaviour,** at the lowest crate that fits; never re-test at a higher layer what a
  lower crate proves.
- **Fault points:** exhaustive only in `crates/fs`; a consumer tests one point per class (an error
  before the commit, an error after it, a crash). Under the test-only `faults` feature `fs` skips
  fsync and uses a per-process fake trash; one test of the real fsync order and one real-trash
  round trip per OS remain.
- **Heavy variants** (scale, exhaustive walks, many proptest cases) run only at full strength
  (`DALICA_FULL_TESTS=1`, §3); quick runs use one fault point per class, at most 32 proptest cases
  and small sizes.
- **Fingerprints** only where pixels are the feature, at one window size (`architecture.md` §11).
- **No per-milestone catch-up test files** (`*_m9.rs`, `lessons_m9.rs`, `m9_fields.rs`): a test
  goes in the file of the behaviour it checks.
- **One integration-test binary per crate:** `autotests = false` and
  `[[test]] name = "all", path = "tests/all.rs"` in its `Cargo.toml`, each test file a `mod` of
  `tests/all.rs`, shared helpers in `tests/common.rs`. Separate `[[test]]` targets only for a test
  that runs its own binary as a child process, installs a process-wide logger, or needs
  `required-features`.
- **Fixtures:** a library template built once per process and copied for each test; index-only
  synthetic libraries where pixels don't matter; tiny images.

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
| Save a 24 MP JPEG with an edit (render, encode, write, verify; median) | ≤ 1 050 ms |
| Save a 24 MP JPEG turned a quarter (rotate-only, lossless; median) | ≤ 455 ms |
| Export the 200-photo mixed tray at 1600 px, Normal, watermark, location removed (median) | ≤ 24 s |
| Build the search index of a 100 k-photo library (median) | ≤ 630 ms |
| Search 100 k photos for one word (median) | ≤ 1.7 ms |
| Search 100 k photos for three words with Starred on (median) | ≤ 0.6 ms |
| Search 100 k photos for a lone `-word` (median) | ≤ 5.1 ms |
| `search.set` to its result applied, 100 k photos, debounce excluded (median) | ≤ 22.3 ms |
| Import from a card, 1 000 JPEGs of 2048 px: the first screenful of thumbnails (median) | ≤ 35 ms |
| Import from a card, 1 000 JPEGs: the listing (median) | ≤ 320 ms |
| Import from a card, 1 000 JPEGs: copied and verified (from the median) | ≥ 22.9 files/s, ≥ 2.3 MB/s |
| Batch Edit: Auto Fix on 1 000 photos as one step (median) | ≤ 7 ms |
| Batch Edit: undo Auto Fix on 1 000 photos (median) | ≤ 5 ms |
| Slideshow of 24 MP photos: a change that waits for its preview | 0 |
| Slideshow: an advance to the first frame of its transition, next photo prefetched (median) | ≤ 24 ms |
| Slideshow: a transition frame (p95; never above 16 ms) | ≤ 0.3 ms |

**At the review, the save, export and search budgets are judged against a base on the same
runner** (maintainer decision, 2026-10-15): the Budgets job runs each of these benches in the last
release (`git describe --tags --match 'v*'`; for a bench newer than that release, the commit that
first measured it) and then in the reviewed commit, and a median may be at most 1.3 × the base's
(`cargo xtask budgets --baseline <ref>`; `RELATIVE_FACTOR` and each budget's `Relative` in
`xtask/src/accept.rs`). GitHub's runners differ by more than the budgets allowed (the index build
took 446–490 ms on some and 718–769 ms on others, which failed 11 runs in a row); a ratio on one
runner does not. The millisecond figures in the table are the budgets without a base (a plain
`cargo xtask budgets`, on a workstation); the frame budgets (8 ms), the first screenful (1 s) and a
query's 100 ms cap are always absolute.

The save and export budgets (M8.24) are 1.5 × their first measurement: release, two cores
(`taskset -c 0,1`, as the budgets job's runner), 2026-10-14, a 24-core Linux workstation under
other agents' builds (load average 7): Save 701 ms, the export 16.0 s (80 ms a photo, two at a
time). On all 24 cores the same run took 274 ms and 6.9 s. The rotate-only Save's first budget
(815 ms, from a rendered Save of 542 ms) predates the lossless rotation of M8.14; M9.30 measured
the lossless rotation the same way (2026-10-14, load average 5): 303 ms (a second run 307 ms), so
its budget is 455 ms (1.5 × 303, rounded up to 5 ms as the others).

The search budgets (M9.12, the `search` bench) are 1.5 × their first measurement and a query's never
above 100 ms. The library is the synthetic 100 k one (1 000 folders) with a three-word caption on
every photo, one or two of 30 tags, search facts, and 100 albums of 300 photos. Measured in release
on two cores (`taskset -c 0,1`), 2026-10-14, the same workstation (load average 3.5): the index
built in 417 ms, one word 1.12 ms (13 441 matches), three words and Starred 0.38 ms, a lone `-word`
3.39 ms (86 559 matches, nearly the whole library), and `search.set` to its result applied by
`poll` 14.9 ms (the worker's scope, query and suggestions, the reply, the result applied); three
more runs were within 12 %. Rounded up to 5 ms for the build and 0.1 ms for a query. The first
search of a session, which also copies the library to the worker and builds its index, took 757 ms;
it is printed, not budgeted.

The scan throughput, cold start and idle memory budgets (M10.22) are the table's values, absolute
and never loosened. Scan throughput is a first scan of 5 000 `scenes` JPEGs (1024 px, each with a
160 px EXIF thumbnail) into a new library, photos per second from `folders.add` until every photo
is in the library with its header read, so its embedded thumbnail is available to the light table
at once (the thumbnail jobs run alongside, as in the app; the 128 and 256 entries made from a
decode are printed, not budgeted). The cold start is a fresh process opening the index-only 100 k
library (1 000 folders, every photo sketched) to its first settled frame in the headless driver,
every visible cell drawn from its sketch, the median of five runs; idle memory is that process's
resident memory read by itself after 5 s idle, the largest of the runs (Linux and macOS; Windows
is not measured). Measured in release on two cores (`taskset -c 0,1`), 2026-10-15, the same
24-core workstation under other agents' builds (load average 9 to 20): the scan 0.48 s for 5 000
photos (10 325 photos/s; every thumbnail made after 21.4 s, 233 photos/s; on all 24 cores 22 739
and 1 470 photos/s); the cold start 239 ms (the slowest of five 315 ms; reading the index 204 ms,
three frames); idle memory 83 MiB. What they leave out: the OS window and GPU setup, and a cold page cache (the
files and the index were just written; dropping the cache needs root).

The import, Batch Edit and slideshow budgets (M11.23) are 1.5 × their first measurement (times
rounded up, rates down), absolute; a transition frame is also never above 16 ms at p95, and no
change of the slideshow may wait for its preview, whatever the first numbers. The first
measurement is the median of three runs in one sitting: release on two cores (`taskset -c 0,1`),
2026-10-17, the same 24-core workstation (load average 10 falling to 2.4 over the runs; its NVMe
disk formatted btrfs). Single runs earlier that day under heavier load spread widely (Batch Edit
1.07 to 4.75 ms, the slideshow's advance 10.8 to 22.2 ms, the import 27.4 to 46.4 s): commands of
a few milliseconds ran faster on a busy machine, whose cores were clocked up, than on a quiet one,
so the quiet runs set the budgets.
- **Import** (the `import` bench): 1 000 `scenes` JPEGs of 2048 px (about 100 KB each, 160 px
  EXIF thumbnails, folders of about 100) moved into a temporary card's `DCIM`; a new library whose
  device list is that card's mount folder opens it, asks the listing every millisecond, then
  imports every candidate into a folder named by text, medians of three runs each. The first
  screenful (28 cells, the first candidates in capture order) had its thumbnails after 23.2 ms
  (the runs 23.2, 23.3 and 15.3 ms); the listing ended after 211 ms (211, 211, 199); the import
  took 29.1 s (28.3, 29.7, 29.1 s: 34.4 files/s, 3.56 MB/s). Each copy is two `fsync`s (the new
  file, then its folder after the rename) of about 12 ms each on this disk, so the import is bound
  by the disk's `fsync` latency, not by its bandwidth; every copy enters the library in the same
  frame its result is applied.
- **Batch Edit** (the `batch_edit` bench): 1 000 photos of 320 px scanned into a new library;
  `edit.apply {op: "autofix"}` on all of them, from the command to its answer (every recipe
  changed, the sidecar writes queued, one undo step "Auto Fix"), 4.65 ms (the runs 2.98, 4.75,
  4.65); `edit.undo`, 3.31 ms (1.34, 3.31, 3.33); medians of fifteen, the session idle between
  them.
- **Slideshow** (the `slideshow` bench): six 6000 × 4000 JPEGs played by the real app in the
  headless driver at 1400 × 900, one frame every 1/60 s of real time, 3 s display time, Loop on,
  five changes for each transition but Cut. The frame where the show moves on (the incoming
  preview taken from its pane and uploaded as a texture) took 16.0 ms at the median (the runs
  19.4, 16.0, 13.7; p95 about 28 ms: the advance frame is often longer than a 60 Hz frame); a
  transition frame 0.199 ms at p95 (0.195, 0.199, 0.204); 0 of 30 changes waited for their preview
  in every run. Frames are timed whole (`Driver::step`: the poll, the show, layout, texture
  uploads, tessellation); rasterizing is the GPU's and not measured.

Measure, don't guess: record numbers in `docs/status.md` → Metrics with the machine and date.

**The budgets job.** `.github/workflows/budgets.yml` runs `cargo xtask budgets` once per milestone,
on the pull request of its `review/M<n>.R` branch (and when dispatched by hand), on Linux only
(`ubuntu-latest`), with its own release cache: it builds in release and runs
the budgets the acceptance tests assert only with `DALICA_ACCEPT_RELEASE=1`, and the run fails when
one is missed. Today (M4) that is the 100 k scroll of `library_bench` in full (UI frame time p95
≤ 8 ms; cells visited per frame flat from 10 k to 100 k) and `m4.rs`'s thumb held at its top speed
through 100 k sketched photos (p95 ≤ 8 ms, no placeholder, cells flat); since M5 also the first
screenful of a just-added folder of 1 k and of 10 k JPEGs (≤ 1 s each, `first_thumbnails`) and a
scanned 20 k pass at top thumb speed (no placeholder, p95 ≤ 8 ms); the scanned 100 k pass runs in
the release acceptance only. Since M7 the job also runs the `edit_preview` bench in its own step
(`cargo bench -p dalica-engine --bench edit_preview -- --check --plant-ms …`, which fails on a
missed budget itself): a 24 MP JPEG opened to its screen preview cold and with the screen entry
cached (median ≤ 150 ms), a Tuning drag's drafts (p95 ≤ 16 ms) and finals (p95 ≤ 100 ms), and
next/previous with the neighbours prefetched (p95 ≤ 50 ms). Since M8 `cargo xtask budgets` also
runs the `save_export` bench and judges its medians: Save of a 24 MP JPEG with an edit and with
only a quarter turn, and the 200-photo mixed export. Since M9 it also runs the `search` bench and
judges its medians: the index of the 100 k library built, one word, three words with Starred, a
lone `-word`, and `search.set` to its result applied. Since M10 it also runs the `scan_throughput`
bench (at least 500 photos per second; a planted slowdown counts once per photo scanned) and the
`cold_start` bench (the median cold start within 2 s, a planted slowdown counted once per frame up
to the first settled one; idle memory within 600 MiB in every run). Since M11 it also runs the
`import` bench (the first screenful and the listing, a planted slowdown counted once each; files
and MB per second copied, a planted slowdown counted once per file), the `batch_edit` bench (a
planted slowdown added to each median) and the `slideshow` bench (no change waiting; the advance's
median and a transition frame's p95, a planted slowdown added to every frame). `--plant-ms 8`
misses Batch Edit's and the slideshow's budgets but not the import's (8 s over 1 000 files is
inside its headroom); `--plant-ms 15` misses all three. A milestone adds its budgets to `BUDGETS` in `xtask/src/accept.rs`. Budgets are judged only at the
review (maintainer decision, 2026-10-15); *Run workflow* on the Actions tab runs the job on any
branch, with a planted slowdown (`plant_ms`) if asked. The same check locally:

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

### Freeing disk space

Everything under the main checkout's `target/` is rebuildable cache (releases are built by
GitHub): one cargo target dir per agent slot (`target/agent-<n>`), the lead's (`target/lead`), the
merge queue's (`target/merge`), review measurements (`target/review-M<n>`), the maintainer's
`target/website`, the main checkout's own `target/debug` and `target/release`, and old one-offs.
Runs with a relative `CARGO_TARGET_DIR` also leave stray `target/` dirs in `apps/*`, `crates/*`,
`tests/acceptance` and `xtask`. Together they reach hundreds of GB (about 630 GB on 2026-10-13).
`[profile.dev] debug = "line-tables-only"`, with no debug info for dependencies, keeps each one
smaller (a fresh `cargo test --workspace --no-run`: 16.6 GB, against 49.9 GB with full debug info;
backtraces keep file:line in our crates).

`cargo xtask clean` removes them, from any checkout, printing each dir with its size and the
total freed:

```sh
cargo xtask clean --dry-run                    # list and size only
cargo xtask clean --keep lead --keep website   # everything but these (repeatable; `*` matches)
cargo xtask clean --only agent-3               # just these (repeatable)
cargo xtask clean --older-than 7               # only dirs whose newest file is over 7 days old
```

It never removes a dir cargo is building in (it takes cargo's own build locks first and skips the
dir when one is held), nor the target dir of the running `xtask` binary. It removes only immediate
children of `<main>/target/` that hold a cargo profile and the fixed stray paths above; symlinks
are refused, never followed, and each path is canonicalised and checked before
`std::fs::remove_dir_all`. One failure is reported and the rest go on; the exit status is non-zero
only if something could not be removed. `cargo xtask tasks prune` is the lighter tool: it removes
only `incremental/` from idle agents' dirs.

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
- `panic_hunt` (a `dalica-engine` test; `DALICA_FULL_TESTS=1` for its full case count) reproduces
  most command-level panics.

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
1400 × 900 (900 × 600 too when the task changes layout; `--scale 2` for high-DPI checks), then
look at the PNG.

### Environment variables

| Variable | Effect |
|---|---|
| `RUST_LOG` | log levels for standard error and the log file (see *Logs*) |
| `RUST_BACKTRACE=1` | full backtraces in panic reports |
| `DALICA_PROFILE=1` | per-stage timings (see *Profiling*) |
| `DALICA_FULL_TESTS=1` | tests at full strength: every fault point, full proptest case counts, full-size scale tests, exhaustive walks; set by `cargo xtask ci --full`, a standalone `cargo xtask accept` and the corpus job; unset, tests run quick (§3) |
| `DALICA_SUITES_RAN=1` | set by `cargo xtask ci` on its acceptance steps: the acceptance tests skip the crate suites its test step already ran (§3) |
| `RUSTC_WRAPPER=sccache` | exported by `cargo xtask tasks env` when sccache is installed: one compiler cache for every agent's target dir (§3) |
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
  of any corpus. The `SOURCES.tsv` of `dalica-corpus` lists every file with its source URL, page, author,
  licence, size, sha256 and the tool that derived it, if any; its `LICENSE` is CC0 1.0.
- All pins are in one file, `xtask/src/corpus_pins.rs`; each set has a manifest
  `xtask/corpus/<set>.sha256` (`<sha256>  <path>` per line, sorted by path). Sets:

| Directory | What | Source and pin | Used by |
|---|---|---|---|
| `corpus/pngsuite/` | PngSuite: 176 PNGs (every bit depth, colour type, interlacing and ancillary chunk, 14 corrupt on purpose) | the 2017-07-19 release archive from schaik.com, by URL and sha256 (its own permissive terms) | `dalica-image` |
| `corpus/raw/` | 26 camera raws, two per kind Dalica reads previews from: CR2, CR3, CRW, DNG, NEF, ARW, PEF, ORF, RW2, SRW, RAF, MRW, X3F (245 MB) | raw.pixls.us, each file CC0, by URL and sha256 | `dalica-image` |
| `corpus/photos/` | 57 CC0 and public-domain photos (134.5 MB) from 18 camera and phone makes: JPEG orientations 0–8 and none (1–8 also as one generated set showing the same picture), EXIF thumbnails, MPF, Display P3 and Adobe RGB (with and without an ICC profile), CMYK JPEG and LZW TIFF, progressive, truncated, 16-bit TIFF and PNG, WebP | `photos/` of `dalica-corpus` at commit `6239cad2`, by commit and sha256 (published M6.42 at `ef4826d0`; files unchanged since) | `dalica-image`, `dalica-thumbs` |
| `corpus/heif/` | 3 CC0 and public-domain iPhone HEIC photos (5.6 MB, grid tiles, EXIF, colour), for the `heif` feature | `heif/` of `dalica-corpus` at commit `6239cad2`, by commit and sha256 (published M6.42 at `ef4826d0`; files unchanged since) | `dalica-image` |
| `corpus/metadata/` | 53 generated files (1.0 MB): `dalica-scenes` photos (JPEG, PNG, WebP, TIFF and a DNG-like TIFF) after exiv2 0.28.9, ImageMagick 7.1.2-32 and Pillow 12.3.0 wrote captions, keywords, ratings, GPS, dates, MWG face regions and foreign metadata into them, and RAW `.xmp` sidecars | `metadata/` of `dalica-corpus` at commit `6239cad2`, by commit and sha256 (M6.34) | `dalica-meta` interop and round-trip tests ([`xmp-interop.md`](xmp-interop.md) §8; `crates/meta/tests/corpus.rs`) |
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

**Opt-in locally, strict when opted in, always at the review.**

- The corpus tests sit in each crate's `tests/corpus.rs` behind a `corpus` cargo feature
  (`dalica-image` and `dalica-meta` today; `dalica-thumbs` and later `dalica-faces` when their
  tests land), so plain `cargo test` and `cargo xtask ci` neither compile nor need them.
- With the feature on, a missing or stale corpus is a **failure** ("run `cargo xtask corpus --all`"),
  never a silent skip, and every floor is enforced.
- **Floors only rise.** Each crate commits floors per set (`crates/image/tests/corpus_floors.json`:
  files, files sniffed as their extension says, headers read the way scanning reads them, full
  decodes, 256 px thumbnails, embedded thumbnails decoded, and for PngSuite the corrupt files
  refused; `crates/meta/tests/corpus_floors.json`: files and sidecars read, writes made and
  written, sidecar writes, RAW+sidecar pairs; refusals and unchanged writes have no floor, and
  any failed write fails the test). A measured set without floors, a floor without a manifest and a count below its floor
  all fail; a count above its floor is printed so the PR that improved it raises it. Never lower
  one. A file that newly fails is investigated, and a synthetic reproduction goes into the crate's
  ordinary tests (corpus files are never committed or quoted).
- The corpus tests run once per milestone, at the review (maintainer decision, 2026-10-15); run
  `cargo xtask test-corpus` yourself only when a task's *done when* asks for corpus numbers (a new
  set, a raised floor).
- The CI `corpus` job (Linux) runs `cargo xtask corpus --all`, clippy of `dalica-image` (with the
  `corpus` and `heif` features) and of `dalica-meta` (with `corpus`), and `cargo xtask test-corpus`
  at full strength, on the review's pull request and when dispatched, with `corpus/` cached under a key of `corpus_pins.rs` plus the manifests; `--all` re-hashes a restored
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
`harness = false` bench target each): `library_bench` (`dalica-ui`), `pack` (`dalica-thumbs`),
`first_thumbnails`, `edit_preview`, `save_export`, `search`, `scan_throughput`, `import` and
`batch_edit` (`dalica-engine`), and `cold_start` and `slideshow` (`dalica-ui`).
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
it runs a smoke pass at 1 k and 10 k photos. Once it exists, the review gate runs `cargo xtask scorecard --check`; `cargo xtask perf` runs
nightly on one fixed runner, never on pull requests.

`edit_preview` (M7.11) writes four 24 MP JPEGs (`--width`, `--height`; 6000 × 4000) and fits
the edit view's preview to `--fit` (2560 × 1600). Rows: `open cold` (a session on an empty
thumbnail cache, from `Session::open_preview` to the poll that takes the final preview: the file
decoded whole and halved to between one and two screen sizes, its histogram counted) and `open cached`
(a new session on the cache the cold opens filled: the ladder's screen entry read instead), seven
each; `drag` (40 steps of a Tuning drag: each step's draft and final, `draft_…` and `final_…`);
`next previous` (forward through the photos and back with the neighbours prefetched while each is
shown). The budgets hold on the budgets job's 2-vCPU runner: check them locally with
`taskset -c 0,1` on the bench binary. With `--bench` or `--check` it exits non-zero when a budget of §8 is missed (in release
only); `--plant-ms N` adds N ms to every judged time. Without `--bench` it runs a smoke pass on
600 × 400 photos and judges nothing.

`save_export` (M8.24) writes a `--width × --height` JPEG (6000 × 4000) and saves fresh copies of
it through `create::save::save`, the path `photo.save` runs on its worker: `save` with a Tuning
edit (read, render at full size, encode at the original's quality, the original kept in
`.dalica-originals/`, the new file written and verified) and `save rotate-only` with a quarter
turn, seven each. `export 200 mixed` builds the mixed tray of `crates/engine/tests/export_mixed.rs`
at `--scale` 5 (200 photos of every format, 1500 to 2400 pixels long, 125 edited, 56 with
GPS), and times `export.run` at 1600 pixels, Normal, with a watermark and the location removed,
from the command to the end of its job, three times (`per_photo_ms` is the median over 200). Rows
carry `p50_ms`, `p95_ms`, `max_ms` and their `budget_ms`; `cargo xtask budgets` judges the
medians. Without `--bench` it runs a smoke pass (600 × 400 saves, the tray at `--scale` 1).

`search` (M9.12) builds the synthetic library of `--photos` photos (100 000, in 1 000 folders) in
memory with what search reads added (a three-word caption on every photo, one or two of 30 tags,
search facts, 100 albums of 300 photos), writes it as an index snapshot and opens a session on it
on a virtual clock. Rows: `index build` (`SearchIndex::from_library`, what the worker does with a
fresh copy; `--builds` 5), `one word`, `three words and a filter` (Starred on) and `a -word` (a lone
negation, nearly every photo) (`SearchIndex::search` in the scope the worker builds; `--samples`
50 each, with `matches`), `search.set to applied` (from the command to the result applied by
`Session::poll`, the virtual clock moved past the debounce at once; `search.clear`, untimed,
between samples), and `first search` (the session's first, which also starts the worker and builds
its index; printed, not budgeted). Rows carry `p50_ms`, `p95_ms`, `max_ms` and their `budget_ms`;
`cargo xtask budgets` judges the medians over 100 k photos in release. Without `--bench` it runs a
smoke pass over 2 000 photos, two samples each.

`scan_throughput` (M10.22) writes `--photos` `scenes` JPEGs (5 000, `--side` 1024 px, each with a
160 px EXIF thumbnail, in folders of about 100) under `--dir` (this target directory's temporary
folder: on the budgets job, the runner's SSD), then runs `folders.add` on them in a new library on
an empty thumbnail cache, polling at 60 frames a second as the app does. Row `scan <n>`: `scan_s`
and `photos_per_s` (until the scan has ended and every photo is in the library, its header read,
so its embedded thumbnail is available), `thumbs_s` and `thumbs_photos_per_s` (until every photo
also has its 128 and 256 entries), and `embedded` (the photos whose header names an embedded
thumbnail, checked after the timing: all of them). `cargo xtask budgets` judges `photos_per_s`
against 500; `--check` makes the bench judge it itself. Without `--bench` it runs a smoke pass over
200 photos of 320 px.

`cold_start` (M10.22) writes the index-only synthetic library of `--photos` photos (100 000, in
1 000 folders), then re-runs its own binary `--runs` times (5) as a fresh process each (`--child`),
timed from the spawn to the child's line saying it drew its first settled frame: the library
opened as the app opens it (the index read; thumbnails off, as there are no files), shown by the
real `App` in the headless driver at 1400 × 900, frames stepped until nothing is pending and every
visible cell is drawn from its sketch. The child then checks the app stays settled, idles
`--idle-ms` (5 000) without frames, and reads its own resident memory (Linux `/proc/self/status`,
macOS `ps` on itself). Rows: `cold start <n>` (p50, p95 and max over the runs; the children's
medians of `open_ms`, the index read, and `first_frame_ms`; `frames` to the first settled one) and
`idle memory <n>` (`rss_mib`, the largest of the runs, `rss_p50_mib` and `peak_mib`). `cargo xtask
budgets` judges the median cold start against 2 s and `rss_mib` against 600 MiB; `--check` makes
the bench judge them itself. Without `--bench` it runs one smoke run over 2 000 photos.

`import` (M11.23) writes `--photos` `scenes` JPEGs (1 000, `--side` 2048 px, each with a 160 px
EXIF thumbnail, in folders of about 100) and moves them into a card's `DCIM` folder under `--dir`
(this target directory's temporary folder). Each of `--runs` runs (3) opens a new library whose
device list is the card's mount folder (the `DALICA_DEVICE_ROOTS` hook), opens the card, and asks
`import.list` every millisecond until the listing is done; then `import.run` copies every
candidate into a new folder named by text, polled at 60 frames a second until `import.status`
says `done`. Rows: `first thumbnails <n>` (until the first screenful, 28 cells, all have their
thumbnails), `list <n>`, `import <n>` (with `files_per_s`, `mb_per_s` and `bytes` from the median)
and `in library <n>` (until every copy is a library photo; printed). `cargo xtask budgets` judges
them; `--check` makes the bench judge them itself. Without `--bench` it runs one run over 60
photos of 320 px.

`batch_edit` (M11.23) writes `--photos` `scenes` JPEGs (1 000, 320 px, folders of about 100) and
scans them into a new library, then `--samples` times (15) times `edit.apply {ids: every photo, op:
"autofix"}` and `edit.undo`, each from the command to its answer, the session idle and flushed
between them; it checks the apply was one step "Auto Fix" and the undo gave every recipe back.
Rows: `auto fix <n>` and `undo auto fix <n>`. Without `--bench` it runs 100 photos, two samples.

`slideshow` (M11.23) writes `--photos` `scenes` JPEGs (6, `--width × --height` 6000 × 4000),
scans them, and plays a show of their folder in the real app in the headless driver at
1400 × 900, a frame every 1/60 s of real time on the system clock, Loop on, `--advances` changes
(5) for each of Dissolve, Wipe, Circle, Push, Rectangle and Pan and Zoom. Rows: `advance` (the
frame where the show moves on; with `changes` and `waits`, the changes whose preview was not ready
when they began), `transition frame` (every other frame of a change) and `transition frame <id>`
per transition (printed). It exits non-zero when a change waited; `--check` also judges the
budgets. Without `--bench` it runs three photos of 640 × 427, one change per transition.

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
