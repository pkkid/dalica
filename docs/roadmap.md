# Roadmap

Milestones for Dalica and their task blocks, with task ids used in commit subjects (`M1.3: …`).
This file is the plan; the live state of every task (ready, claimed, in review, done) is the
GitHub task board, kept in sync by `cargo xtask tasks` ([`task-board.md`](task-board.md)). Agents
get work with `cargo xtask tasks next`, never by scanning this file.

Status legend: ✅ done · 🚧 in progress · ⬜ not started. Task blocks use only ⬜ and ✅; their live
state (ready, claimed, in review) is on the task board ([`task-board.md`](task-board.md)).

## Where we stand

*Honest assessment, 2026-10-10. Agents: read this before picking work. Update it only with
measured facts, dated.*

*2026-10-10 (M1.R).* M1 is done: `cargo xtask accept M1` passes in 5 s on a fresh clone, and the
maintainer's manual check passed (a star set with Ctrl+8 in `dalica --demo` survives a restart). A
folder of photos is scanned and shown in a plain library window (folder list, square thumbnail
grid); a photo is starred with a click and Ctrl+8; the star is written to `.dalica.json`, survives an
index rebuild and a restart; a second process is refused. The window is drawn by our own widgets
with Inter, and the same frames render headlessly on the CPU, byte-identical to what
`dalica-cli snapshot` writes; the first four fingerprints are approved baselines. The UI thread
never touches the disk (counted in tests). 319 tests; `cargo xtask ci` takes 111 s cold and 29 s
warm; GitHub CI 3–5.5 min per OS. The board's full loop (`claim`, `submit`, `merge`) ran live for
M1.16–M1.19, and two bugs in `merge` were fixed. Next: M2, agent eyes (widget ids per the spec,
the control channel, a windowless `--headless` instance), the first milestone for parallel agents.

*2026-10-10 (M1.A).* The whole data path of M1 works headlessly on all three OSes: a folder of
photos is scanned by content (bounded; links and special files handled), its sidecars are read,
stars change through `Session::execute` with one undo step each, every change is journalled and
written back to `.dalica.json` (debounced, atomic, unknown keys kept), and deleting the index and
rebuilding gives the same stars. `cargo xtask accept M1 --headless` passes in 1.7 s; `panic_hunt`
fuzzes all 10 commands without a panic; the workspace has 285 tests, `cargo xtask ci` takes 95 s
cold and 21 s warm on a fresh clone. The tests found and fixed five real bugs (a job pool losing
work, scans hanging on FIFOs, sidecar floats drifting, sub-pixel rectangles off by one, a board sync
failure). There is no UI yet beyond M0's empty window: M1.16–M1.19 add the widgets, the headless
driver and the plain library window. Next: M1.16, through the full board loop for the first time.

*2026-10-09.* M0 is done pending its review. The workspace has the 18 crates of `architecture.md` §2
as stubs plus the two apps, and every gate bites: `cargo xtask ci` (fmt, clippy, tests, layers, deps,
assets, names, parity, docs, roadmap parse) is green on Linux, Windows and macOS (GitHub CI 2–5 min
per OS cold, under a minute warm on Linux and macOS; 46 s on a clean local clone), and
`cargo xtask accept M0` shows 6/6 planted violations caught. The task board (`cargo xtask tasks`)
adopted the 32 bootstrap issues without duplicates and closes merged tasks; its claim, submit and
merge paths have only run against the in-memory fake. `dalica` opens an empty 1400 × 900 window
(GL-free wgpu, no fonts bundled yet; the title text is missing on GNOME until M3's own title bar).
There is no product code yet. Next: M1, the first real data path from a folder to a starred photo
that survives a restart.

Before that (kickoff, 2026-10-09): the reference organizer was documented privately in `notes/`,
the public specs in `docs/` describe Dalica, the scope was trimmed (dated features dropped), the
kickoff decisions were recorded and the architecture was specified (`architecture.md`).

## How milestones work

Maintainer decisions, 2026-10-10: the 38 small milestones were combined into 16; and every review
task is a stop. Agents prepare a review; only the maintainer merges or closes it, and no work listed
after it starts until then. (This reverses the same day's earlier choice of check-ins that nothing
waits for.)

- **Feature areas, sized for parallel work.** A milestone groups related feature areas, usually
  10–20 tasks of one agent session each, so up to 8 agents have independent work. It ends in
  something the maintainer can run and look at.
- **Every milestone has an acceptance check:** `cargo xtask accept M<n>` runs that milestone's
  acceptance tests and scripts (`dalica-cli run` / `snapshot` scripts under `tests/acceptance/`)
  and prints what a human should look at. Its *Done when* lines say what it proves. Acceptance
  checks of earlier milestones keep running in `cargo xtask ci`, so nothing silently regresses.
- **Every review task is a stop.** A milestone ends with a review task `M<n>.R`; a big one adds a
  mid-milestone review `M<n>.A` (then `M<n>.B`, …). Every review task is `owner: maintainer` (the
  roadmap parser refuses any other owner), and nothing listed after an open review task in this
  file is claimed until the maintainer closes it, whether in the same milestone or a later one
  (`cargo xtask tasks next` and `claim` enforce it).
- **The lead prepares a review; the maintainer decides it.** When the run reaches a review task,
  running tasks finish and merge, and the lead runs `/milestone-review M<n>.<X>`: a check-in page
  (screenshots at both window sizes, what to try, every decision made with a way to reverse it),
  the dated *Where we stand* and the `status.md` numbers on a branch `review/M<n>.<X>`, and a pull
  request linked on the review issue. Then the run stops. Only the maintainer merges that pull
  request and closes the review issue; no agent and no `cargo xtask tasks` command merges or closes
  a review. The maintainer then starts the next run (`/milestone-start`); feedback becomes tasks.
- **Releases are reviews too.** M6 (0.1), M10 (0.2) and M15 (1.0) end with a release gate `M<n>.R`;
  its check-in adds the release checklist and the sign-off questions.
- **The lead plans ahead.** The lead writes the next milestone's task blocks while the current one
  runs, by the end of its first wave, so agents never wait for a plan. Task blocks need no
  approval; the check-in shows them.
- **Agents make the technical decisions** (`AGENTS.md` §4): they default to what LightCraft and
  PhotoCraft did (`architecture.md` §9) and record each decision in the commit body; the check-in
  lists them for reversal. Only the decisions listed in `AGENTS.md` §4 stop and ask the maintainer.
- **Metrics** a task measures go in its dev-log entry and pull request; the lead copies them into
  `status.md` on each review's branch (done-whens never write it). Steps only the maintainer can do
  (a release sign-off, a manual look) belong to review tasks and their check-in pages, not to a
  worker's done-when.
- **Tasks are blocks in a fixed format** (`after`, `owner`, `files`, `scope`, `done when`;
  [`task-board.md`](task-board.md) §3). This file is the plan; the **live state** (ready, claimed,
  in review, done) is the GitHub task board that `cargo xtask tasks` keeps in sync with it.
- **Agents.** M0 and M1 were built by a single agent. From M2 on, up to 8 agents on the
  maintainer's machine pick tasks from the board (one lead integrator, the rest workers;
  `AGENTS.md` §7).
- Phases group milestones for orientation only; the lead may reorder work inside a phase.

## Overview

| Phase | # | Milestone | One-line goal | Status |
|---|---|---|---|---|
| Foundation | M0 | Skeleton and gates | an empty, fully gated workspace on 3 OSes | ✅ |
| | M1 | Star a photo, end to end | folder → index → grid → star → sidecar → restart and rebuild | ✅ |
| | M2 | Agent eyes | agents drive and screenshot the real window | ⬜ |
| Library | M3 | Window shell | every region of the main window, at the right size, in the Craft look | ⬜ |
| | M4 | Library browsing | the continuous light table at 100 k photos, the library tree, selection and the tray | ⬜ |
| | M5 | Folders, scanning and formats | Folder Manager, first run, watching, offline drives; every listed format, RAW previews, ICC | ⬜ |
| | M6 | Photo info, metadata and Release 0.1 | rotate, hide, captions, Properties, dates, tags written into files; installable preview | ⬜ |
| Edit and output | M7 | Edit view and fixes | edit view, basic fixes, Tuning and Retouch; thumbnails show edits | ⬜ |
| | M8 | Save and export | Save, Undo Save, Revert, Save a Copy, Save As; export | ⬜ |
| Organize | M9 | Albums, search and file operations | lasting albums, instant search and filters, move/copy/rename/delete | ⬜ |
| | M10 | Preferences, MCP and Release 0.2 | Options and shortcut editor, MCP and the P0 workflows; all P0 rows shipped | ⬜ |
| P1 parity | M11 | Import, compare and viewing | import from cards and drives, compare and batch edit, slideshow, the viewer | ⬜ |
| | M12 | Effects | effect tiles, parameter panels and all 27 effects | ⬜ |
| | M13 | People and places | local faces, the People pane, the Places pane and geotagging | ⬜ |
| | M14 | Collage and legacy sidecars | the collage room; reading the previous organizer's data | ⬜ |
| Finish | M15 | Video, localisation, OS integration and Release 1.0 | the honest 1.0 | ⬜ |

## Milestones

### M0 — Skeleton and gates

- **Goal:** an empty workspace where every quality gate already bites, so every later commit lands
  on green.
- **In:** Cargo workspace with every crate of `architecture.md` §2 as a stub (crate roots with the
  never-crash attributes, a README each); `rust-toolchain.toml` (1.98), `clippy.toml`,
  `rustfmt.toml`; `LICENSE-MIT`, `LICENSE-APACHE`; `xtask`: `ci`, `layers`, `deps`, `assets`,
  `names`, `parity` (structure only), `accept`; GitHub Actions running `cargo xtask ci` on Linux,
  Windows and macOS; the task board (`xtask tasks`, [`task-board.md`](task-board.md)); `AGENTS.md` and
  the private dev log; `dalica` opens an empty window titled
  Dalica on the `chrome` colour and quits cleanly; `dalica-cli --version` and `commands` (`[]`).
- **Out:** any feature, theme beyond one colour, commands, control channel, packaging.
- **Done when:** `cargo xtask ci` is green on a clean clone locally and on all three CI runners;
  `cargo xtask accept M0` proves each gate fails when it should (in a temporary copy: a layer
  violation, an `unwrap()` in non-test code, an unlisted PNG, the reference product's name in a
  file, a `*-sys` dependency, a `std::fs::write` outside `fs`); you run `cargo run -p dalica` and
  see the empty window.

#### M0 tasks

Task blocks use the fixed format of [`task-board.md`](task-board.md) §3; `cargo xtask tasks sync`
turns each into a GitHub issue. M0 is built by a single agent.

- ✅ **M0.1 Workspace skeleton**
  - after: —
  - files: `Cargo.toml`, `rust-toolchain.toml`, `LICENSE-MIT`, `LICENSE-APACHE`, `.gitignore`, `crates/**`, `apps/**`, `tests/acceptance/**`
  - scope: workspace members `crates/*`, `apps/*`, `xtask`, `tests/acceptance`;
    `[workspace.package]` (version 0.0.0, edition 2024, `rust-version = "1.98"`, licence
    `MIT OR Apache-2.0`); `[workspace.dependencies]` with the versions of `architecture.md` §1;
    `panic = "unwind"`; `[profile.dev] debug = "line-tables-only"` (keeps 8 agents' target
    directories small); `rust-toolchain.toml`; licence files; every crate of `architecture.md` §2
    as `Cargo.toml` + `README.md` + `src/lib.rs` with `#![forbid(unsafe_code)]` and the never-crash
    `deny` list; `apps/dalica`, `apps/dalica-cli`; an empty `tests/acceptance` crate.
  - done when: `cargo build --workspace --all-targets` and `cargo test --workspace` pass, and
    `tests/acceptance/m0_skeleton.rs` asserts every crate root has both attributes and
    `cargo metadata` lists exactly the 18 crates, 2 apps, `xtask` and `dalica-acceptance`.
- ✅ **M0.2 `xtask` frame and `layers`**
  - after: M0.1
  - files: `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/src/layers.rs`, `xtask/src/metadata.rs`
  - scope: subcommand dispatch; the layer table of `architecture.md` §2 and rules 1–4 (downward
    normal deps with listed exceptions; dev-deps downward plus `scenes`; UI crates only in L5 and
    apps; `std::fs` write calls only in `crates/fs`); unregistered crates fail; reads
    `cargo metadata` JSON.
  - done when: `cargo xtask layers` passes on the tree, and unit tests on synthetic metadata and
    sources fail naming the crate and rule for `library → engine`, `egui` in `engine`, an
    unregistered crate, `std::fs::write` in `crates/store`, and `scenes` as a normal dependency of
    `core`.
- ✅ **M0.10 Task board: `xtask tasks`**
  - after: M0.2
  - files: `xtask/src/tasks/**`, `.github/ISSUE_TEMPLATE/**`
  - scope: everything in [`task-board.md`](task-board.md): the roadmap task parser; `setup`,
    `sync`, `next`, `claim`, `submit`, `release`, `status`, `merge`, `log`; the local claim lock in
    the git common directory; a `GitHub` trait with a `gh`-CLI implementation and an in-memory
    fake for tests. Running `setup` and the first `sync` against GitHub needs the maintainer's
    go-ahead (it creates milestones, labels, issues and project fields).
  - done when: `cargo test -p xtask` passes tests on the fake that show: every task block of
    `roadmap.md` parses (and a malformed block fails with its line number); `sync` run twice makes
    no second change; `next` never offers a task whose `after` is not done, whose `files` overlap
    a claimed task, or that belongs to a later milestone than the open review gate; 8 threads
    claiming the same task produce exactly one winner; a ✅ task with an open issue makes `sync`
    fail. Then, with approval, `cargo xtask tasks setup && cargo xtask tasks sync` creates the
    board and `cargo xtask tasks status` lists every M0 and M1 task.
- ✅ **M0.3 `xtask deps`**
  - after: M0.2
  - files: `xtask/src/deps.rs`
  - scope: licence allow-list (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0; MPL-2.0 only
    with a recorded reason); deny list (`openssl-sys`, `ring`, `aws-lc-sys`, `libz-sys`, …); any
    crate with a `cc`/`cmake` build dependency fails; `*-sys` crates only from an allow-list with a
    one-line reason each (e.g. `windows-sys`: bindings only, no C build).
  - done when: `cargo xtask deps` passes on the tree and unit tests reject a GPL crate,
    `openssl-sys`, an unknown `foo-sys`, and a crate whose build script depends on `cc`.
- ✅ **M0.4 `xtask names` and `assets`**
  - after: M0.2
  - files: `xtask/src/names.rs`, `xtask/src/assets.rs`
  - scope: `names` compares every tracked file's words and file names against SHA-256 hashes of
    the forbidden names (so the names never appear in the source; `AGENTS.md` is exempt);
    `assets` matches every image/font/sound/data file to an `ATTRIBUTION.md` row, checks licence
    files exist, and fails forbidden names and extensions.
  - done when: both pass on the tree and unit tests (temp dirs) fail on a planted forbidden word,
    a forbidden word in a file name, an unlisted `.png`, and a row pointing at a missing licence
    file; the same word inside `AGENTS.md` passes.
- ✅ **M0.5 `xtask ci`, `parity`, `accept`**
  - after: M0.3, M0.4
  - files: `xtask/src/ci.rs`, `xtask/src/parity.rs`, `xtask/src/accept.rs`
  - scope: `ci` = `fmt --check`, `clippy --workspace --all-targets -D warnings`, tests
    (parallelism from free RAM and `CARGO_BUILD_JOBS`), `layers`, `deps`, `assets`, `names`,
    `parity`, `tasks sync --check` (offline: parses the roadmap only), then `accept` for every
    finished milestone; `parity` checks ids, tiers, statuses, cited `cmd:` ids and paths, and
    `--write` refreshes the summary; `accept M<n>` runs `cargo test -p dalica-acceptance --test
    m<n>` and prints `tests/acceptance/m<n>.md`.
  - done when: `cargo xtask ci` is green; `cargo xtask parity --write` twice leaves no diff; unit
    tests show `parity` rejecting a duplicate id and an unknown status.
- ✅ **M0.6 Gates bite: `accept M0`**
  - after: M0.5
  - files: `tests/acceptance/m0.rs`, `tests/acceptance/m0.md`
  - scope: copy the tree to a temp dir (shared target dir), plant one violation at a time, expect
    the matching gate to fail, remove it, expect green: a layer violation, `unwrap()` in non-test
    code, an unlisted PNG, a forbidden name, a path dependency named `evil-sys`, `std::fs::write`
    outside `fs`.
  - done when: `cargo xtask accept M0` reports 6/6 gates bite and exits 0; the warm run time is
    recorded in `status.md` → Metrics.
- ✅ **M0.7 CI on three OSes**
  - after: M0.1
  - files: `.github/workflows/ci.yml`
  - scope: matrix ubuntu / windows / macos; toolchain 1.98; dependency cache; the Linux packages
    eframe needs; `cargo xtask ci`; read-only token permissions; cancel superseded runs; runs on
    pull requests and pushes to `main`. Pushing needs the maintainer's go-ahead.
  - done when: a pushed branch shows the workflow green on all three runners (`gh run watch`);
    each runner's duration is recorded in `status.md` → Metrics.
- ✅ **M0.8 Empty window and CLI stubs**
  - after: M0.1
  - files: `apps/dalica/src/**`, `apps/dalica-cli/src/**`, `apps/dalica-cli/tests/**`, `crates/ui-kit/src/theme.rs`
  - scope: window titled "Dalica", 1400 × 900, minimum 900 × 600, `chrome` background (one
    token), clean quit, panic hook writing to the log; `dalica --version`, `dalica-cli --version`,
    `dalica-cli commands` (prints `[]`).
  - done when: `cargo test -p dalica-cli` checks `commands` prints `[]` and both `--version`
    outputs equal the workspace version; you run `cargo run -p dalica` and see the empty window.
- ✅ **M0.9 Docs match reality**
  - after: M0.5, M0.6, M0.7, M0.8, M0.10
  - files: `docs/development.md`, `docs/task-board.md`, `AGENTS.md`, `docs/status.md`, `xtask/src/docs.rs`
  - scope: a `ci` step checking every `cargo xtask …` command named in `AGENTS.md`,
    `development.md` and `task-board.md` exists; status and metrics updated.
  - done when: `cargo xtask ci` (with the docs check) and `cargo xtask accept M0` pass on a clean
    clone.
- ✅ **M0.R Milestone review**
  - after: M0.9
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the maintainer runs the manual check, reviews what was learned and approves the M1
    task list (`roadmap.md` → How milestones work).
  - done when: the maintainer closes the review issue.

```text
M0.1 ─┬─► M0.2 ─┬─► M0.10 (task board) ───────────────┐
      │         ├─► M0.3 ─┬─► M0.5 ─► M0.6 ───────────┤
      │         └─► M0.4 ─┘                           ├─► M0.9 ─► M0.R (review)
      ├─► M0.7 (CI) ──────────────────────────────────┤
      └─► M0.8 (window, CLI stubs) ───────────────────┘
```

### M1 — Star a photo, end to end

- **Goal:** the core data path done properly once: a folder on disk → index → thumbnail grid →
  star → `.dalica.json` → survives a restart and an index rebuild. Every crate on that path exists
  for real, with its tests.
- **In:** `core` (ids, position keys, `Clock`); `fs` (atomic write + verify, lock, UI-thread guard,
  fault injection); `scenes`; `image` (JPEG and PNG, scaled JPEG decode, EXIF orientation,
  embedded EXIF thumbnails, decode limits); `sidecar` v1 (stars; unknown keys preserved);
  `library` (folders, photos, star op + inverse, revisions); `store` (journal + snapshot, debounced
  sidecar writer, `flush`, rebuild); `scan` (one-shot bounded walk + verification); `thumbs` (job
  pool, memory LRU); `engine` (Session, registry, command contract, library undo stack, keymap
  with contexts, minimal settings, `panic_hunt`); commands `folders.add`, `library.query`,
  `photo.inspect`, `photo.star.toggle` / `photo.star.set`, `edit.undo` / `edit.redo`,
  `library.rebuild`; `dalica-cli` `commands`, `run`, `inspect`, `snapshot`; `ui-kit` (first
  tokens, a few widgets, the CPU rasterizer); `ui` (a deliberately plain window: folder list, one
  square grid per folder, click to select, Ctrl+8 and a star button, the star badge, File → Add
  Folder…).
- **Out:** watching, tray and Hold, rotate and hide, the real section headers and layout, search,
  captions, writing metadata into files, the disk thumbnail cache, Folder Manager and first run,
  other formats, the control channel.
- **Done when:** `cargo xtask accept M1` passes: a script adds a folder of 200 `scenes` JPEGs (all 8
  orientations) to a temporary library, stars 3 and undoes 1 → the folder's `.dalica.json` holds
  exactly 2 stars and keeps keys planted in it beforehand; after deleting the index and running
  `library.rebuild` the same 2 stars show; a second process on the same library is refused;
  fault-injection tests never leave a torn sidecar; `panic_hunt` is green; a headless snapshot
  shows 2 star badges (fingerprint). You run `cargo run -p dalica -- --demo`, star a photo with
  Ctrl+8, restart, and the badge is still there.

#### M1 tasks

M1 is deliberately the thickest milestone: every crate on the data path gets built for real once.
Review gate **M1.A (checkpoint A)** after M1.15 checks the whole path headlessly before any UI is
built. Built by a single agent; the waves show what could run in parallel later. After M1.A the
agent uses the full board loop (`claim`, `submit`, `merge`). The first fingerprints of the
repository (M1.16's widget gallery, M1.18/M1.19's star badges) are new baselines: the lead sends
their PNGs to the maintainer, who approves them before they merge (M1.A decision, 2026-10-10).

- ✅ **M1.1 `core`**
  - after: M0.R
  - files: `crates/core/**`
  - scope: id newtypes with serde; `Rect01` normalised geometry; `Clock` with system and virtual
    clocks; collision names (`name-001.ext`, next free); position keys (`after`, `between`).
  - done when: `cargo test -p dalica-core` passes, including property tests that `between(a, b)`
    sorts strictly between any `a < b`, that keys stay short after 10 000 inserts at one spot, and
    collision naming on empty, dot-only and non-ASCII names.
- ✅ **M1.2 `fs`: atomic writes**
  - after: M1.1
  - files: `crates/fs/src/atomic.rs`, `crates/fs/src/append.rs`, `crates/fs/src/guard.rs`, `crates/fs/src/faults.rs`, `crates/fs/src/lib.rs`, `crates/fs/Cargo.toml`
  - scope: atomic replace (temp in the same folder, write, fsync, rename, fsync the folder where
    supported, re-read and compare); read-before-write identity check; the UI-thread guard; named
    fault points behind the `faults` feature; an append-only, size-capped `append` (fsync after each
    write) and `rotate` (`name.log` → `name.1.log` → `name.2.log`) for log files, so the app's logger
    writes through `fs` too (layer rule 4).
  - done when: `cargo test -p dalica-fs --features faults` passes, including a loop that fails each
    fault point in turn and asserts the target holds exactly the old or the new bytes; a file
    changed between read and write is refused; any call on a thread marked as UI returns
    `Err(OnUiThread)`; `append` past its cap returns an error and leaves the file at or under the cap.
- ✅ **M1.3 `fs`: lock and walks**
  - after: M1.2
  - files: `crates/fs/src/lock.rs`, `crates/fs/src/walk.rs`, `crates/fs/tests/**`
  - scope: library lock file (std `File::try_lock`; `fs4` was dropped); bounded directory walk (depth 32, 1 000 000 entries,
    symlink-loop detection, caller-supplied skip rules, sorted entries).
  - done when: a second process (spawned test helper) gets `Err(Locked)` while the first holds
    the lock and succeeds after it exits; a symlink loop and a 40-deep tree end with a bounded
    result and a warning, not a hang; names with `]`, `"`, `;`, `=` and non-ASCII are listed.
- ✅ **M1.20 App log file**
  - after: M1.2
  - files: `apps/dalica/src/logging.rs`, `apps/dalica/Cargo.toml`
  - scope: the desktop app's log of `development.md` §10 written through `fs::append`:
    `<config>/logs/dalica.log`, rotated to `dalica.1.log` / `dalica.2.log` at each launch, capped at
    16 MiB, Dalica's crates at `info` and the rest at `warn`, `RUST_LOG` directives; the panic hook
    also appends to `dalica-panics.log` in the OS temp folder; `--version`, `--help` and
    command-line errors write no file.
  - done when: tests on a temp config dir show rotation across three launches, the cap holding,
    a panic's message and location in both files, and no file for `--version`.
- ✅ **M1.4 `scenes`**
  - after: M1.1
  - files: `crates/scenes/**`
  - scope: seeded procedural images (gradients, horizons, soft blobs) at any size; JPEG (via
    `jpeg-encoder`) with a minimal EXIF block (orientation 1–8, DateTimeOriginal, make/model,
    optional embedded thumbnail) and PNG; `demo_library(dir, folders, photos)`. (Built with the `image`
    crate's JPEG encoder, since `jpeg-encoder`'s licence includes IJG; `demo_library` returns the files
    and the caller writes them.)
  - done when: `cargo test -p dalica-scenes` shows the same seed gives identical bytes, all 8
    orientations are written, and the files decode with the `image` crate.
- ✅ **M1.5 `image`: sniffing, EXIF and orientation**
  - after: M1.4
  - files: `crates/image/src/sniff.rs`, `crates/image/src/ifd.rs`, `crates/image/src/exif.rs`, `crates/image/src/orient.rs`, `crates/image/src/limits.rs`, `crates/image/src/lib.rs`, `crates/image/Cargo.toml`
  - scope: magic-number sniffing; a bounded TIFF/IFD reader (shared later with `meta` and RAW
    previews); orientation, capture date and dimensions from headers without decoding; embedded
    EXIF thumbnail extraction; `Limits`.
  - done when: `cargo test -p dalica-image` covers a `.jpg` that is really a PNG, all 8
    orientations read from `scenes` files, looping and out-of-range IFD offsets returning `Err`,
    and a property test truncating files at every offset that never panics.
- ✅ **M1.6 `image`: decoding**
  - after: M1.5
  - files: `crates/image/src/decode.rs`, `crates/image/src/resize.rs`, `crates/image/tests/**`
  - scope: JPEG and PNG decode to RGBA8 sRGB (ICC in M5); scaled decode for thumbnails (DCT
    scaling where the decoder offers it, else decode and resize under the 64 MP cap); orientation
    applied; header bombs refused before allocation.
  - done when: each of the 8 orientations decodes upright (fingerprint equal to the orientation-1
    source); a header declaring 100 000 × 100 000 is refused without allocating; a 256 px
    thumbnail of a 24 MP scene decodes within a recorded time.
- ✅ **M1.7 `sidecar` v1**
  - after: M1.1
  - files: `crates/sidecar/**`
  - scope: the `.dalica.json` document with `photos.<name>.star`; unknown keys preserved at every
    level; wrong-type values kept and reported; BOM and CRLF accepted; duplicate keys take the
    later value; deterministic printing (sorted keys, two spaces, LF); a parse error the caller
    uses to set the file aside.
  - done when: `cargo test -p dalica-sidecar` passes property tests (`parse(print(x)) == x` for
    random documents, any file name as a key) and a planted unknown key and a wrong-type `star`
    keep their values when another photo is edited.
- ✅ **M1.8 `library` model and ops**
  - after: M1.1
  - files: `crates/library/**`
  - scope: folders, photos (name, size, mtime, dimensions, date, star), `Op` (add/remove folder,
    add/remove/update photos, set stars), `apply` returning the inverse, revisions, queries
    (photos of a folder in display order, by id).
  - done when: `cargo test -p dalica-library` passes a property test that, for random op
    sequences, applying an op and then its inverse restores the model and each apply bumps the
    revision once.
- ✅ **M1.9 `store`: journal and snapshot**
  - after: M1.2, M1.8
  - files: `crates/store/src/journal.rs`, `crates/store/src/snapshot.rs`, `crates/store/src/lib.rs`, `crates/store/Cargo.toml`
  - scope: JSON-lines journal with a checksum per line, periodic snapshot, replay that stops
    cleanly at a torn or corrupt tail.
  - done when: journal + replay reproduce the model; truncating the journal at every byte offset
    replays to the last complete op; a corrupt snapshot falls back to the journal or a rebuild
    with a reported reason.
- ✅ **M1.10 `store`: sidecar sync and rebuild**
  - after: M1.3, M1.7, M1.9
  - files: `crates/store/src/mapping.rs`, `crates/store/src/writer.rs`, `crates/store/src/rebuild.rs`, `crates/store/tests/**`
  - scope: model ↔ sidecar mapping (with the test that every user-visible model field is
    mapped); dirty folders written debounced (1 s on the `Clock`) by re-reading, merging and
    atomically replacing; `flush()`; damaged sidecars renamed `.corrupt-<time>` and their folder
    flagged read-only; `rebuild` from folders.
  - done when: star → `flush` → the sidecar holds it; planted unknown keys survive; virtual time
    shows at most one write per folder per second; delete the index → `rebuild` → the same stars
    (`data-model.md` §10.1 for stars); fault injection during a write leaves the old or the new
    sidecar (§10.5).
- ✅ **M1.11 `scan`: one-shot scan and verification**
  - after: M1.3, M1.5, M1.7, M1.8
  - files: `crates/scan/**`
  - scope: walk the watched roots through `fs`; skip rules (dot files and folders,
    `.dalica-originals`); recognise JPEG and PNG by content; read headers and sidecars; emit
    batched `Op`s; a verification pass that diffs disk against the model.
  - done when: temp-tree tests find nested folders, skip skipped folders, and after adding,
    deleting and renaming files the verification pass emits exactly the matching ops; scanning
    1 000 `scenes` JPEGs completes within a recorded time.
- ✅ **M1.12 `thumbs`: job pool and memory cache**
  - after: M1.6
  - files: `crates/thumbs/**`
  - scope: prioritised, per-key de-duplicating pool with cancellation; byte-budgeted LRU; the
    thumbnail job (decode at size, orientation applied).
  - done when: fake-job tests show strict priority order under contention, one execution per
    duplicate key, cancelled jobs never delivered, and no lost results in a 10 000-job stress run;
    the LRU never exceeds its byte budget.
- ✅ **M1.13 `engine`: Session and commands**
  - after: M1.10, M1.11
  - files: `crates/engine/src/session.rs`, `crates/engine/src/registry.rs`, `crates/engine/src/undo.rs`, `crates/engine/src/error.rs`, `crates/engine/src/lib.rs`, `crates/engine/src/cmd/**`, `crates/engine/Cargo.toml`
  - scope: `Session` (library, store, clock, lock), `CommandSpec`, `execute` (enabled check, typed
    params, `catch_unwind`, one undo step), the library undo stack (cap 1 000); commands
    `folders.add`, `library.query`, `photo.inspect`, `photo.star.toggle`, `photo.star.set`,
    `edit.undo`, `edit.redo`, `library.rebuild`.
  - done when: each command passes the command test contract (`architecture.md` §3); star 3 →
    undo → `flush` → the sidecar holds 2; a second `Session` on the same library gets `Locked`.
- ✅ **M1.14 `engine`: keymap, settings, `panic_hunt`**
  - after: M1.13
  - files: `crates/engine/src/keymap.rs`, `crates/engine/src/settings.rs`, `crates/engine/src/cmd/app.rs`, `crates/engine/tests/**`
  - scope: contexts, key parsing, defaults from specs, user overrides, resolution order, conflict
    detection, `app.setShortcut` / `app.resetShortcuts`; `settings.json` (unknown keys kept,
    damaged file set aside); `panic_hunt` over every command.
  - done when: Ctrl+1 in Library and Edit view is not a conflict while Global + Library on one key
    is; no two default bindings conflict; a rebinding survives a restart;
    `cargo test -p dalica-engine --test panic_hunt -- --ignored` is green.
- ✅ **M1.15 `dalica-cli` run and inspect**
  - after: M1.14
  - files: `apps/dalica-cli/src/run.rs`, `apps/dalica-cli/src/inspect.rs`, `apps/dalica-cli/src/main.rs`, `apps/dalica-cli/Cargo.toml`, `tests/acceptance/m1_headless.rs`, `xtask/src/accept.rs`
  - scope: `commands` (the engine registry), `run --library DIR` (JSON-lines script, one result per
    line, flush, exit 1 on a failed command and 2 on a bad script line), `inspect`;
    `cargo xtask accept M<n> --headless` runs the `m<n>_headless` test target.
  - done when: `cargo xtask accept M1 --headless` passes the data half of M1's *Done when*: 200
    `scenes` JPEGs in all orientations, star 3, undo 1 → 2 stars in `.dalica.json` with planted
    keys intact; delete the index, rebuild → 2 stars; a second process refused.
- ✅ **M1.A Checkpoint A review**
  - after: M1.15
  - owner: maintainer
  - files: —
  - scope: the maintainer runs `cargo xtask accept M1 --headless`, looks at a sidecar and the CLI
    output, and approves building the UI on top.
  - done when: the maintainer closes the review issue.
- ✅ **M1.16 `ui-kit`: tokens, widgets, rasterizer**
  - after: M1.1
  - files: `crates/ui-kit/**`, `assets/fonts/**`, `ATTRIBUTION.md`, `crates/scenes/src/fingerprint.rs`
  - scope: M1's tokens (`chrome`, `canvas`, `grid_bg`, `cell`, `cell_selected`, text greys,
    `accent`, `icon`), the widget registry, a button, an icon button, the star icon and badge; Inter
    (Regular, SemiBold) in `assets/fonts/` with `OFL-Inter.txt` and its `ATTRIBUTION.md` row,
    installed by `ui-kit` (egui's default fonts are off); the CPU rasterizer: first check whether
    `egui_software_backend` (0.0.3 on 2026-10-09) supports egui 0.36 and matches egui's blending,
    adopt it if so, else write our own (decision and reason in the commit body). Move the image
    fingerprint helper (8 × 8 grid, per-channel mean and deviation, tolerance, actual/expected PNGs
    under `target/test-output/`) from `crates/image/tests/decode.rs` to `dalica-scenes`, for
    `image`, `ui-kit` and `ui` tests.
  - done when: `cargo test -p dalica-ui-kit` renders a widget gallery to identical pixels twice,
    matches its fingerprint, and the token contrast test passes (text on `chrome` ≥ 4.5:1).
- ✅ **M1.17 `ui`: headless driver and snapshots**
  - after: M1.A, M1.16
  - files: `crates/ui/src/app.rs`, `crates/ui/src/state.rs`, `crates/ui/src/headless.rs`, `crates/ui/src/lib.rs`, `crates/ui/Cargo.toml`, `apps/dalica-cli/src/snapshot.rs`
  - scope: `App::run` (UI-only ids, else forwarded), a minimal `UiState`, the headless driver
    (`click`, `key`, `step_until`, `state`, `widgets`, `screenshot`) on a virtual clock,
    `dalica-cli snapshot`.
  - done when: a test drives an empty app headlessly and takes a screenshot of the requested size;
    `dalica-cli snapshot --library DIR -o out.png --size 1400x900` writes that PNG.
- ✅ **M1.18 `ui`: the plain library window**
  - after: M1.12, M1.17
  - files: `crates/ui/src/panels/**`, `crates/ui/src/keys.rs`, `crates/ui/src/picker.rs`, `crates/ui/tests/**`, `apps/dalica/src/**`, `apps/dalica/Cargo.toml`
  - scope: folder list; one square grid per folder with thumbnails from the job pool; click
    selects; Ctrl+8 (through the keymap) and a star button; the star badge; File → Add Folder…
    through the `Picker` trait (`rfd` with only its `xdg-portal` backend in the app, since gtk3
    links C; scripted in tests); the UI-thread guard on. The window keeps OS decorations until M3's
    title bar. The app opens the `Session` off the UI thread, then marks the UI thread and calls
    `Session::poll()` every frame (scans and writes run on the engine's worker threads).
  - done when: a headless test opens a `scenes` library, clicks a thumbnail by widget id, presses
    Ctrl+8, and the photo is starred in the engine and its badge shows in the screenshot
    fingerprint; no `fs` call happens on the UI thread during the test.
- ✅ **M1.19 Demo library and `accept M1`**
  - after: M1.18
  - files: `crates/engine/src/demo.rs`, `tests/acceptance/m1.rs`, `tests/acceptance/m1.md`, `apps/dalica/src/main.rs`, `apps/dalica-cli/src/main.rs`
  - scope: `--demo` (in `dalica` and `dalica-cli`) builds a throwaway `scenes` library, written
    through `fs` (`scenes` itself never writes); the full acceptance test, reusing the steps of
    `m1_headless.rs` and adding the snapshot; the manual check written down; metrics in the dev log
    (the lead copies them to `status.md`).
  - done when: `cargo xtask accept M1` passes everything in M1's *Done when*.
- ✅ **M1.R Milestone review**
  - after: M1.19
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the maintainer does the manual check (star with Ctrl+8, restart, badge still there),
    reviews what was learned, and approves the M2 task list.
  - done when: the maintainer closes the review issue.

Waves (inside a wave, "‖" separates chains in different crates that share no files):

```text
wave 1   M1.1 core
wave 2   M1.2 → M1.3 fs, M1.20 app log  ‖  M1.4 → M1.5 → M1.6 scenes, image  ‖  M1.7 sidecar  ‖  M1.8 library  ‖  M1.16 ui-kit
wave 3   M1.9 → M1.10 store  ‖  M1.11 scan  ‖  M1.12 thumbs
wave 4   M1.13 → M1.14 engine → M1.15 cli → M1.A checkpoint review
wave 5   M1.17 → M1.18 ui, app → M1.19 acceptance → M1.R milestone review
```

### M2 — Agent eyes

- **Goal:** agents can drive and look at the real window, not only the headless one.
- **In:** widget ids on everything M1 draws; the complete headless driver (`click`, `drag`, `key`,
  `type_text`, `step_until`, `state`, `widgets`, `screenshot`); serialisable `UiState`; the control
  channel (`--control`, token, loopback only, limits) with `engine.execute`, `engine.commands`,
  `ui.state` / `ui.set`, `ui.widgets`, `ui.clickWidget`, `ui.pointer`, `ui.key`, `ui.type`,
  `ui.screenshot` (window, headless fallback), `ui.inspect` (perf counters), `ui.settle` /
  `ui.wait`; `dalica-cli snapshot --script`; `DALICA_PROFILE=1`.
- **Out:** MCP (M10), menu methods (need M3's menus), new features.
- **Done when:** `cargo xtask accept M2` runs a control session against a headless instance (auth,
  click a thumbnail, Ctrl+8, screenshot) and checks the star and the PNG fingerprint; the
  protocol's security tests pass (wrong token, oversized line, non-loopback bind refused). You
  start `dalica --control 7990 --demo` and an agent's screenshot matches your screen.

#### M2 tasks

- ✅ **M2.1 `ui`: widget ids to the spec grammar**
  - after: M1.R
  - files: `crates/ui/src/panels/**`, `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/registry.rs`, `crates/ui/tests/**`, `tests/acceptance/m1.rs`, `tests/acceptance/m1_steps.rs`
  - scope: rename M1's ids to `control-protocol.md` §7 (`menu:File`, `menuitem:folders.add`,
    `button:star`, `thumb:<PhotoId>`, `thumb:<PhotoId>/star`, `tree:folder:<FolderId>`); one
    registration helper; a headless test that every interactive widget drawn has an id, unique per
    frame.
  - done when: the test lists zero unregistered interactive widgets at 1400 × 900 and 900 × 600;
    `accept M1` still passes.
- ✅ **M2.2 `ui`: the complete headless driver and perf counters**
  - after: M1.R
  - files: `crates/ui/src/headless.rs`, `crates/ui/src/perf.rs`
  - scope: `drag`, `hover`, `scroll`, `type_text`, `pointer` (raw events); `settle` as in
    control-protocol §6.12 (quiet frames, pending jobs listed); `DALICA_PROFILE=1` and the perf
    counters `ui.inspect` reports (frame time, cells visited, textures uploaded, jobs requested).
  - done when: tests drag a scroll by widget id, type into a text field, and show cells visited per
    frame stay flat between a 200- and a 2 000-photo library.
- ✅ **M2.3 `ui`: control method handlers**
  - after: M2.1, M2.2
  - files: `crates/ui/src/control/**`, `crates/ui/src/lib.rs`
  - scope: the methods of M2's *In* as pure handlers over `App` + `Driver`/window input queue:
    `auth` is the transport's; `engine.execute`, `engine.commands`, `ui.state`/`ui.set`, `ui.widgets`,
    `ui.clickWidget`, `ui.pointer`, `ui.key`, `ui.type`, `ui.screenshot` (path within write roots,
    §5.7), `ui.inspect`, `ui.settle`/`ui.wait`; errors (§3) and limits (§4) checked before any
    change; input replies after the input played out (§5.2); the file-picker stub (§5.6).
  - done when: each method has a test against the headless driver, including every §3 code it can
    return and every §4 limit.
- ✅ **M2.4 `dalica`: the control transport and headless mode**
  - after: M2.3
  - files: `apps/dalica/src/control/**`, `apps/dalica/src/main.rs`, `apps/dalica/src/window.rs`, `apps/dalica/Cargo.toml`
  - scope: `--control [PORT]` on 127.0.0.1 only, the token file (§2.1: 256-bit from `getrandom`,
    0600 on Unix, refused if group/other-readable), `auth` first in constant time, framing, limits
    (1 MiB lines, 16 connections, 30 s idle, 60 s deadline), requests served from the logic tick;
    `--headless` runs the same handlers on the headless driver without a window; exit 2 if the port
    is taken.
  - done when: a test starts `dalica --headless --control <port> --demo`, authenticates, stars a
    photo by `ui.clickWidget` + `ui.key`, and gets a screenshot.
- ✅ **M2.5 Control security tests**
  - after: M2.4
  - files: `apps/dalica/tests/control_security.rs`
  - scope: wrong token, no `auth`, oversized line, non-JSON and HTTP request lines close the
    connection; the 17th connection is refused; a group-readable token file is refused; no flag
    binds a non-loopback address.
  - done when: all pass on Linux, macOS and Windows CI.
- ✅ **M2.6 Demo library per the spec**
  - after: M1.R
  - files: `crates/engine/src/demo.rs`, `apps/dalica-cli/src/main.rs`, `apps/dalica/src/main.rs`, `apps/dalica/src/window.rs`, `tests/acceptance/m1.md`, `docs/control-protocol.md`
  - scope: per decision 2 of the M1.R review: deterministic demo ids (folders numbered in tree
    order, photos in light-table order, §5.4); isolated settings; `--demo-fresh` (a new temporary
    folder deleted on exit) if `--demo` stays persistent.
  - done when: two fresh demo libraries list identical ids and paths; `m1.md`'s restart check
    still holds.
- ✅ **M2.7 Lead: board and disk hygiene**
  - after: M1.R
  - owner: lead
  - files: `xtask/src/tasks/**`, `docs/task-board.md`
  - scope: the lead (agent-1) takes it; `submit` defaults the PR body to the task's dev-log entry; fingerprint approval on the
    PR (a `baseline-approved` label the lead checks before `merge`); `tasks prune` removes
    `incremental/` from idle agents' target dirs.
  - done when: tests with the fake board cover all three; a dry run on the real board.
- ✅ **M2.8 `accept M2`**
  - after: M2.4, M2.5, M2.6
  - files: `tests/acceptance/m2.rs`, `tests/acceptance/m2.md`, `tests/acceptance/fingerprints/m2-*.json`
  - scope: M2's *Done when*: a control session against a headless instance (auth, click a
    thumbnail, Ctrl+8, screenshot) checking the star and the PNG fingerprint; the security tests;
    the manual check (`dalica --control 7990 --demo`, an agent's screenshot matches the screen).
  - done when: `cargo xtask accept M2` passes everything in M2's *Done when*.
- ✅ **M2.9 Lead: task blocks for M4**
  - after: M1.R
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M4 task block (`after`, `files`, `scope`, `done when`) from M4's section and
    the spec, with waves and the `M4.R` check-in; the first tasks build on M3's work, not on a
    check-in.
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M2.R Check-in**
  - after: M2.8
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the lead runs `/milestone-review M2.R` to prepare it: the check-in page (the manual check
    `dalica --control 7990 --demo` written as a step for the maintainer to try), *Where we stand*
    and the `status.md` numbers on a `review/M2.R` branch, a pull request open. Only the maintainer
    merges that pull request and closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M2.1 ids  ‖  M2.2 driver + perf  ‖  M2.6 demo  ‖  M2.7 lead (xtask)
wave 2   M2.3 handlers
wave 3   M2.4 transport + headless mode
wave 4   M2.5 security tests
wave 5   M2.8 accept M2 → M2.R
```

### M3 — Window shell

- **Goal:** the main window has every region of `ui-layout.md` §2 at the specified size, in the
  Craft look.
- **In:** all `Tokens` of `design-system.md`; the widget set (buttons, icon buttons, segmented
  buttons, slider, text field, dropdown, menus, tooltips, splitter); shell icons drawn in code; the
  menu bar from the full menu catalog (unregistered items disabled; native macOS menu); the
  toolbar (controls present, inactive ones disabled); tree pane and splitter (resizable,
  persisted); status line; bottom area (tray box, star and rotate buttons, size slider, pane
  toggles, action buttons with More… overflow); side-pane frame; window state persisted;
  responsiveness rules (§11); F11; `ui.menu.tree` / `ui.menu.invoke`; per-platform default keys
  (Redo is Cmd+Shift+Z on macOS).
- **Out:** real tree and grid behaviour (M4), pane contents, rooms.
- **Done when:** `cargo xtask accept M3`: geometry tests read every region's rect from the widget
  registry at 1400 × 900 and 900 × 600 and match `ui-layout.md` §2 and §11; token contrast tests;
  widget-gallery fingerprints; every menu item of `menus-and-shortcuts.md` §1 is present in order.
  You open the app at both sizes and compare with the docs.

#### M3 tasks

M3 builds the whole window shell on top of M2's widget ids and headless driver; its first wave
starts as soon as the maintainer has closed M2.R. A review **M3.A** after the foundations (widgets,
icons, menus, keys, layout skeleton) shows the maintainer the look early; the panels and everything
listed after it wait until the maintainer closes it.
First fingerprints of new widgets and icons are approved in bulk on the M3.A review page.

- ⬜ **M3.1 `ui-kit`: the full widget set**
  - after: M2.1, M2.2
  - files: `crates/ui-kit/src/theme.rs`, `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/**`, `crates/ui-kit/tests/**`, `docs/design-system.md`
  - scope: every token and metric of `design-system.md` §2–§4 not yet in `Tokens` (divider, inset,
    field, field_border, track, thumb, thumb_hover, tray_bg, drop_target, filter_banner, face_box,
    shadows, UI scale and font-size factors); the widgets of §6 the shell needs: `primary_button`,
    segmented buttons, `action_button`, `toggle_filter`, `slider`, `zoom_slider`, `section_header`,
    `dropdown`, `search_field` and text field, tooltips, `progress_row`, `empty_state`, `toast`,
    the splitter and the `dialog` frame; each registers its id per control-protocol §7.
  - done when: `cargo test -p dalica-ui-kit` renders a gallery of every widget in every state
    (rest, hover, pressed, active, disabled, focused) identically twice and matches its
    fingerprints; contrast tests cover every text/background pair of §9.
- ⬜ **M3.2 `ui-kit`: shell icons drawn in code**
  - after: M2.1, M2.2
  - files: `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/**`, `crates/ui-kit/tests/icons.rs`, `crates/ui-kit/tests/fingerprints/icons*.json`, `assets/icons/**`, `ATTRIBUTION.md`
  - scope: every icon the shell shows (`design-system.md` §5, `ui-layout.md` §3, §6): app mark,
    import, new album, flat/tree, chevrons, the four filters, search and clear, activity spinner,
    hold, clear, add to album, rotate ccw/cw, loupe, the four pane toggles, export, collage,
    slideshow, more, folder, album, and the caption buttons (minimize, maximize, restore, close);
    our own drawings on the 20 × 20 grid; a Lucide SVG only where a glyph is too complex, with its
    licence row.
  - done when: an icon sheet at 12, 16, 18, 20 and 28 pt renders identically twice and matches its
    fingerprint; `cargo xtask assets` passes.
- ⬜ **M3.3 `ui`: menu model, menu catalog and menus that never run off the window**
  - after: M2.1, M2.2
  - files: `crates/ui/src/menu_catalog.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/control/menu.rs`, `crates/ui/tests/menus.rs`
  - scope: the complete menu tree of `menus-and-shortcuts.md` §1 as data (`menu_catalog.rs`): label,
    command id, shortcut shown, separators, submenus; items whose id has a registered command are
    live, the rest drawn disabled; `UI_COMMANDS` move from `app.rs` to `menus.rs`; in-window menus,
    submenus and context menus bounded by the window and scrolling when too tall
    (`design-system.md` §11.2); `ui.menu.tree` and `ui.menu.invoke` (control-protocol §6.10).
  - done when: a test walks `ui.menu.tree` and finds every item of §1 in order; a headless snapshot
    at 900 × 600 shows the longest menu scrolling inside the window; invoking a disabled item is
    `disabled`, an unknown path `not_found`.
- ⬜ **M3.4 `engine` + `ui`: per-platform default keys and keys for UI commands**
  - after: M3.3
  - files: `crates/engine/src/keymap.rs`, `crates/engine/src/registry.rs`, `crates/engine/tests/keymap.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/menus.rs`
  - scope: default bindings per platform (`menus-and-shortcuts.md` §2: Redo is Cmd+Shift+Z on macOS,
    Ctrl+Y elsewhere); `UI_COMMANDS` declare keys and join the keymap and its conflict check (F11
    full screen, Ctrl+1/Ctrl+2 thumbnail size, Ctrl+0 side pane); shortcuts shown in menus come from
    the keymap, so a rebinding shows up there.
  - done when: no default conflicts on any platform (tested for all three); rebinding a UI command
    with `app.setShortcut` changes the key shown in its menu item.
- ⬜ **M3.5 `ui`: the layout skeleton and geometry tests**
  - after: M2.1, M2.2
  - files: `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/layout.rs`, `crates/ui/src/panels/toolbar.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/src/panels/side_pane.rs`, `crates/ui/src/state.rs`, `crates/ui/tests/layout.rs`
  - scope: every region of `ui-layout.md` §2 laid out at its specified size (menu/title row 24,
    toolbar 40, tree 240 with an 8 pt splitter, light table, status line 20, bottom area 96 split at
    0.365·W, side pane 280 when open), each a stub panel in its own module so M3.6–M3.9 never share
    a file; each region registers its rect; `UiState` gains the view state of control-protocol §6.5
    (tree width, pane, thumb size, full screen).
  - done when: `crates/ui/tests/layout.rs` reads every region's rect from the widget registry at
    1400 × 900 and 900 × 600 and matches §2.
- ⬜ **M3.A Checkpoint A check-in**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - owner: maintainer
  - files: `docs/status.md`
  - scope: the lead runs `/milestone-review M3.A` to prepare it: a check-in page with the widget
    gallery, the icon sheet, a menu and the layout at both sizes, and the decisions so far; the
    `status.md` numbers on a `review/M3.A` branch, a pull request open. Only the maintainer merges
    that pull request and closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.
- ⬜ **M3.6 `ui` + `dalica`: title bar, window frame and window state**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/titlebar.rs`, `crates/ui/src/panels/menubar.rs`, `apps/dalica/src/**`, `apps/dalica/Cargo.toml`, `crates/engine/src/settings.rs`
  - scope: `design-system.md` §11.1: the menu row is the title bar on Windows and Linux (app mark,
    menus, centred title, caption buttons 46 × 24, drag, double-click maximize, 5 pt resize edges,
    Close runs File → Exit); the "Use system title bar" setting; macOS keeps native traffic lights
    and gets the system menu bar from the same menu model through `muda` (a macOS-only dependency;
    `cargo xtask deps`); window position, size, maximized state and tree width persisted.
  - done when: headless snapshots of the title bar at both sizes; the menu model and the macOS menu
    list the same items (test); window state survives a restart (test through settings).
- ⬜ **M3.7 `ui`: the toolbar**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/panels/toolbar.rs`, `crates/ui/tests/toolbar.rs`
  - scope: `ui-layout.md` §3: Import, New Album, Flat | Tree, view options ▾ popup, the Filters group
    with its caption, age slider, search field, activity indicator; controls without a command yet
    are present and disabled; ids per control-protocol §7.
  - done when: a test finds every toolbar id at both sizes with the sizes of §3; the filter caption
    hides below 1100 wide.
- ⬜ **M3.8 `ui`: tree pane, splitter, status line and side-pane frame**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/src/panels/side_pane.rs`, `crates/ui/tests/tree_status_pane.rs`
  - scope: the tree pane frame with the Folders group header (30) and rows (24), resizable through
    the splitter (200 … W − 480, persisted); the status line texts of `ui-layout.md` §6.1; the
    side-pane frame of §7 (header with «, title, ×; 280 or large; pushes the light table, closes
    itself below 480; open state persisted); pane contents stay empty until their milestones.
  - done when: a test drags the splitter by id and the tree width follows within its limits and
    survives a restart; status-line texts for no selection, one photo and a folder; the pane opens,
    widens and closes by id.
- ⬜ **M3.9 `ui`: the bottom area**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/panels/bottom.rs`, `crates/ui/tests/bottom.rs`
  - scope: `ui-layout.md` §6.2–§6.5: the tray box (empty watermark; Hold, Clear, Add to Album
    present and disabled until M4), star and rotate buttons (rotate disabled until its command),
    loupe, the thumbnail-size slider (Ctrl+1 / Ctrl+2), the four pane toggles, the action buttons
    with the More… overflow below 1100 wide.
  - done when: a test finds every bottom-area id with its §6 size at both sizes; More… holds the
    action buttons at 900 × 600; the size slider changes the light table's cell size.
- ⬜ **M3.10 `ui`: responsiveness rules and full screen**
  - after: M3.6, M3.7, M3.8, M3.9
  - files: `crates/ui/src/panels/layout.rs`, `crates/ui/tests/layout.rs`
  - scope: `ui-layout.md` §11 and `design-system.md` §11.1 "narrow windows": chrome drops in the
    stated order, the tree shrinks before the light table drops below 480, the side pane closes;
    F11 full screen; the geometry test extended to every control of M3.6–M3.9.
  - done when: the geometry test passes at 1400 × 900, 1100 × 700 and 900 × 600 with the side pane
    open and closed, and in full screen.
- ⬜ **M3.11 `accept M3`**
  - after: M3.10
  - files: `tests/acceptance/m3.rs`, `tests/acceptance/m3.md`, `tests/acceptance/fingerprints/m3-*.json`
  - scope: M3's *Done when*: geometry of every region at both sizes, token contrast, widget-gallery
    fingerprints, every menu item of `menus-and-shortcuts.md` §1 present in order; the manual check
    (open the app at both sizes and compare with the docs) written down.
  - done when: `cargo xtask accept M3` passes everything in M3's *Done when*.
- ⬜ **M3.12 Lead: task blocks for M5**
  - after: M2.9
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M5 task block from M5's section and the spec, with waves and the `M5.R`
    review (owner: maintainer).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M3.R Check-in**
  - after: M3.11
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the lead runs `/milestone-review M3.R` to prepare it: the check-in page (the app at both
    sizes beside the docs), *Where we stand* and the `status.md` numbers on a `review/M3.R` branch,
    a pull request open. Only the maintainer merges that pull request and closes this issue; nothing
    after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M3.1 widgets  ‖  M3.2 icons  ‖  M3.3 menus  ‖  M3.5 layout skeleton
wave 2   M3.4 keys (after M3.3) → M3.A checkpoint
wave 3   M3.6 title bar + window  ‖  M3.7 toolbar  ‖  M3.8 tree, status, side pane  ‖  M3.9 bottom area
wave 4   M3.10 responsiveness + F11
wave 5   M3.11 accept M3 → M3.R
```

### M4 — Library browsing

One milestone in 3 feature areas (*Light table*, *Library tree*, *Selection and the Photo Tray*); one `cargo xtask accept M4` covers every *Done when* below.

#### Light table

- **Goal:** the continuous, sectioned light table of `ui-layout.md` §5, smooth at 100 k photos.
- **In:** sections in tree order; section header block (icon, title, date and place, buttons with
  inactive ones disabled, description line); justified square-cell grid; badges; thumbnail caption
  modes; hidden/small-picture drawing rules; sticky headers; section scrollbar (⇑ ▲ ▼ ⇓) with the
  speed-control thumb and position marker (`ui-layout.md` §5.6); size slider, Ctrl+wheel, Ctrl+1 /
  Ctrl+2; hover tooltip; empty states; layout cached per revision; texture eviction; never-blank
  thumbnails: colour sketch in the index, the packed thumbnail ladder with compaction and
  pruning, upscaled fallback, direction-aware prefetch (`architecture.md` §6).
- **Out:** selection beyond M1's click (M4); section-header actions (their own milestones); inline
  description editing (M6).
- **Done when:** `cargo xtask accept M4`: a headless benchmark scrolls a synthetic 100 k-photo
  library at ≤ 8 ms/frame p95 UI time (recorded in `status.md` → Metrics) and cells visited per
  frame stay flat from 10 k to 100 k; layout tests for column justification and sticky-header
  push-up; section-jump tests; the scrollbar thumb's speed curve, dead zone and spring-back on
  virtual time; with the thumb held at full speed through a scanned 100 k-photo library no cell
  draws the grey placeholder and frames stay within budget; packed against loose-file reads
  benchmarked on the same library (numbers in the PR). You scroll the demo library with the thumb
  and tune its curve.

#### Library tree

- **Goal:** the left pane of `ui-layout.md` §4.
- **In:** groups (Folders, Other, Hidden Folders; Albums and People appear empty until their
  milestones); flat and tree modes; year dividers; folder sorting and reverse; the view-options
  popup; click = scroll that section's header to the top (animated) and select the folder as a
  unit; collapsing a group hides its sections; ↑/↓ and ←/→; middle elision; counts.
- **Out:** album and people rows (M9, M13), drop targets (M9), Hide Folder (M6).
- **Done when:** `cargo xtask accept M4`: tree rows and order match a fixture library in both
  modes and every sort; a row click leaves its header at the top (scroll offset) and selects the
  folder; collapsing Folders removes its sections; a 5 k-folder library stays within the frame
  budget.

#### Selection and the Photo Tray

- **Goal:** the selection model and the tray of `ui-layout.md` §5.4 and §6.
- **In:** click, Ctrl-click, Shift range, rubber band, scoped to one section; keyboard navigation
  (arrows, Home/End, Ctrl+Home/End, Ctrl+↑/↓, PgUp/PgDn); Ctrl+A / D / I; Select Starred; the tray
  (live mirror, folder token, mini thumbnails that shrink with count, tray selection); Hold
  (Ctrl+H) and the held badge; Clear with confirmation; every status-line state; bottom star and
  rotate buttons act on the tray; "Select some photos first".
- **Out:** Add to Album ▾ (M9), dragging into or out of the tray (M9), tray context-menu items
  of later milestones (shown disabled).
- **Done when:** `cargo xtask accept M4`: select 2 in folder A, Hold, select 3 in folder B → the
  tray has 5 → star all 5 in one undo step → undo restores all 5; status-line strings match each
  case of §6.1; keyboard navigation stops at section edges.

#### M4 tasks

M4 builds the light table, the library tree and the tray on M3's shell. The thumbnail pipeline
(`thumbs`, `library`, `store`, `engine`) touches nothing M2 or M3 changes, so its first wave lists
the M1 work it extends and starts as soon as the maintainer has closed M3.R; the UI tasks also need
M3's panels. Two
tasks lay the ground the others share so later tasks never share a file: M4.5 adds every M4
view-state field and one module per command area, and M4.6 splits the light table into one module
per part (layout, header, cell, scrollbar, input) with a fixed painting API. A review **M4.A**
shows the scrollbar and the never-blank thumbnails early, so the maintainer can tune the scroll
speed before the rest is built (everything listed after it waits until the maintainer closes it). Each task updates its `parity.md` rows; first fingerprints of new
widgets are approved on their PRs; M4.6 splits the window fingerprints by region so parallel
UI tasks each re-bless only their own.

- ⬜ **M4.1 `thumbs`: the colour sketch**
  - after: M1.12
  - files: `crates/thumbs/src/sketch.rs`, `crates/thumbs/tests/sketch.rs`
  - scope: `architecture.md` §6.5: a blurred colour preview of at most 32 bytes in the ThumbHash
    style (our own encoder and decoder written from the public description, or an audited
    permissive crate through `cargo xtask deps`): encode from an RGBA thumbnail keeping its aspect
    ratio, decode to a small RGBA image or a colour grid cheap enough to draw on the UI thread
    (a few µs of arithmetic, no file and no codec); hostile bytes decode to `Err`, never a panic.
  - done when: tests on `scenes` photos: the decoded sketch matches the photo's 4 × 4 mean
    colours within a stated tolerance and its aspect within 5 %; every sketch is ≤ 32 bytes and
    identical across runs; proptest random and truncated bytes never panic; encode and decode
    times per photo are in the PR.
- ⬜ **M4.2 `library` + `store`: the sketch in the index**
  - after: M1.10
  - files: `crates/library/src/model.rs`, `crates/library/src/op.rs`, `crates/library/tests/sketch.rs`, `crates/store/src/snapshot.rs`, `crates/store/src/journal.rs`, `crates/store/src/mapping.rs`, `crates/store/tests/sketch.rs`, `crates/store/tests/sidecars.rs`
  - scope: `data-model.md` §4: each photo gains an optional sketch (opaque bytes, capped at 32);
    an op sets sketches in batches with an exact inverse; sketches are derived data: index only
    (journal and snapshot), never written to sidecars, never on an undo stack, and they bump a
    separate thumbnail revision rather than the library revision that tree and layout caches key
    on; a rebuild from folders starts without sketches (they are regenerated).
  - done when: the library property test (op then inverse restores the model) covers sketch ops;
    store tests: sketches survive journal replay and a snapshot, a torn tail loses only the torn
    batch, no sketch reaches `.dalica.json`, the "every model field has a sidecar mapping" test
    lists the sketch as index-only; the index growth for 100 k sketches is in the PR.
- ⬜ **M4.3 `thumbs` + `fs`: the packed disk cache**
  - after: M1.12
  - files: `crates/thumbs/src/pack.rs`, `crates/thumbs/src/pack/**`, `crates/thumbs/tests/pack.rs`, `crates/thumbs/benches/pack.rs`, `crates/thumbs/README.md`, `crates/fs/src/pack.rs`, `crates/fs/tests/pack.rs`
  - scope: `architecture.md` §6.6: under `<cache>/thumbs/`, one append-only pack file plus an
    index per ladder size (128, 256, 512, 1024, screen), JPEG q90 entries keyed by the
    content-addressed key of §6.4; checksummed index records (a torn tail costs only that entry);
    dead entries counted and compacted in the background past 25 % of a pack; pruning at the
    2 GiB default down to 80 %, largest sizes first; only files of the cache's own naming shape
    are listed or deleted. `fs` gains what packs need: positioned reads, a streamed temp file
    committed by rename, and removal of a cache file of a given name shape (fault points on each).
  - done when: temp-dir tests: put/get round trip at every size; reopening after a torn index tail
    loses only the torn entry; compaction keeps every live entry byte-identical; pruning stops at
    80 %, removes the largest sizes first and never touches a foreign file in the directory; `fs`
    fault injection on every pack write leaves a readable cache; a bench reads 100 k thumbnails
    packed against loose files on the same data (numbers in the PR).
- ⬜ **M4.4 `library`: the tree model**
  - after: M1.8
  - files: `crates/library/src/tree.rs`, `crates/library/tests/tree.rs`
  - scope: `ui-layout.md` §4 and `organizing.md` §10 as a pure function of the library, the
    watched roots and the view options: groups in order (Albums, People, Projects, Folders, Other,
    Hidden Folders; each only when non-empty; Projects and Other in flat mode only); flat mode
    with year dividers (sorted by date, only when the folders span more than one year); tree mode
    (the disk hierarchy under each watched root, parents with their own count); sorts by date
    (a folder's oldest capture date, newest first), recent changes (newest photo `mtime`), size and
    name, each reversible; counts; "Other" = folders where most photos are small pictures
    (< 64 px on both sides); the section order of the light table = the rows of expanded groups.
  - done when: tests on hand-built models: rows and order in both modes for every sort and its
    reverse; year dividers only across years; counts; a collapsed group drops its sections; a
    5 k-folder model builds in a time stated in the PR.
- ⬜ **M4.5 `ui`: M4's view state and command modules**
  - after: M3.3, M3.4, M3.5, M3.6
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/view.rs`, `crates/ui/src/commands/select.rs`, `crates/ui/src/commands/scroll.rs`, `crates/ui/src/commands/tray.rs`, `crates/ui/src/control/**`, `crates/engine/src/settings.rs`, `crates/ui/tests/view_state.rs`, `docs/control-protocol.md`
  - scope: every M4 field of `UiState` in one go (control-protocol §6.5): tree mode, folder sort
    and reverse, collapsed groups, caption mode, small and hidden pictures, the selection's
    section scope and whole-section form, the tray (held photos, tray selection), the scroll
    anchor; the persisted ones (tree mode, sort, caption, small/hidden pictures, thumbnail size)
    saved the way M3.6 saves window state; `crates/ui/src/commands/` with one module per area
    joined into `UI_COMMANDS` once (`select.rs` takes M1's `select.set` / `select.none`; `scroll.rs`
    and `tray.rs` start empty for M4.11 and M4.16); `view.rs`: `view.caption`,
    `view.smallPictures`, `view.hiddenPictures`, `view.folderView`, the folder sorts and Reverse
    Sort of the view-options popup, group collapse; `ui.set` accepts the new fields.
  - done when: `crates/ui/tests/view_state.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes) and the persisted ones survive a
    restart through settings; the view commands change state and light up their menu and popup
    items; every command has its graceful-failure test; `panic_hunt` stays green.
- ⬜ **M4.6 `ui`: the light-table skeleton and layout**
  - after: M3.5, M3.9, M4.4, M4.5
  - files: `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/light_table/**`, `crates/ui/src/panels/thumbnails.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/tests/light_table.rs`, `crates/ui/tests/library_window.rs`, `crates/ui/tests/fingerprints/**`
  - scope: `ui-layout.md` §5.1–§5.3: the light table split into the parent (viewport, scroll
    state, painting order) and one module per part, `layout.rs`, `header.rs`, `cell.rs`,
    `scrollbar.rs`, `input.rs`, the last four carrying M1's drawing and click handling moved over;
    a fixed API at each boundary (including `Thumbnails::paint(painter, photo, rect, device_px)`),
    so M4.9–M4.13 never share a file; the layout model: sections in tree order (M4.4), header
    block ≈ 96 (`design-system.md` §4), justified columns, caption line when captions are on,
    24 pt gutter, prefix-summed offsets, visible rows by binary search, sticky-header geometry
    with push-up, cached per (revision, width, cell size, caption, section set); our own viewport
    in place of egui's scroll area; `section.scrollTo` animated 0.2 s ease-out on the virtual clock
    (again = top of that section); Ctrl+wheel thumbnail size; the "No pictures yet" empty state;
    the whole-window fingerprints split into one per region (tree pane, section header, grid
    cells, tray, status line: `region-<name>*.json`) covering the same pixels, so a task that
    changes one region re-blesses only its own file.
  - done when: `crates/ui/tests/light_table.rs`: justification at widths 480–2 000 and sizes
    64–320 (gap ≥ 12, margins 24); a sticky header pinned, then pushed up by the next; offset ↔
    section round trip; cells visited per frame equal at 10 k and 100 k photos; `section.scrollTo`
    leaves the header at the top after 0.2 s; Ctrl+wheel changes the size; the region
    fingerprints approved on the PR; `widget_ids` stays green.
- ⬜ **M4.7 `engine`: synthetic libraries**
  - after: M4.1, M4.2
  - files: `crates/engine/src/synthetic.rs`, `crates/engine/tests/synthetic.rs`
  - scope: deterministic generated libraries for tests, benches and `accept M4`: N photos in M
    folders (100 k in 1 k folders; 5 k folders) with mixed aspects, dates over several years and
    real sketches from the M4.1 encoder, written as an index snapshot through `store` and opened
    with `Session::open` (no photo files); an option with small `scenes` files on disk for the
    scanned case at smaller sizes.
  - done when: tests: the same seed gives the same library; a 100 k library opens with every
    photo sketched; generation and open times for 100 k (release) are in the PR.
- ⬜ **M4.8 `thumbs` + `engine`: the thumbnail ladder and scan-time generation**
  - after: M4.1, M4.2, M4.3
  - files: `crates/thumbs/src/ladder.rs`, `crates/thumbs/tests/ladder.rs`, `crates/engine/src/thumbs.rs`, `crates/engine/tests/thumbs.rs`, `crates/engine/src/session.rs`, `apps/dalica/src/main.rs`, `apps/dalica-cli/src/main.rs`
  - scope: `architecture.md` §5–§6: ladder sizes in device pixels; after a scan adds or changes
    photos, background jobs make the 128 and 256 entries and the sketch of each (one decode per
    photo) and apply sketches in batches; 512, 1024 and screen size on demand; a session API that
    answers at once with the best available (the covering size, else the next smaller to scale up,
    else the sketch, else "not scanned") and queues the right size; direction-aware prefetch
    (further ahead the faster the view moves; stale prefetch cancelled); the cache directory from
    the session's options (`<cache>/Dalica/thumbs` in the apps, a temp dir in tests and demos).
  - done when: engine tests on a temp library of 200 `scenes` photos: after `wait_idle` every
    photo has a sketch and 128/256 entries; a request for a 300 px cell answers with the 256 entry
    and gets the 512 after its job; a never-scanned photo answers "not scanned"; prefetch order
    follows the scroll direction; the owner thread made no `fs` call (guard count 0).
- ⬜ **M4.9 `ui`: the section header block**
  - after: M4.6
  - files: `crates/ui/src/panels/light_table/header.rs`, `crates/ui/tests/section_header.rs`, `crates/ui/tests/fingerprints/region-header*.json`
  - scope: `ui-layout.md` §5.2, `design-system.md` §7: icon 32 (tooltip = full path; Locate on
    Disk disabled until OS integration), title, long date (+ " · place" when set), the button row
    (Play, Collage, Select Starred, Save, Export; live only when their command is registered,
    Select Starred also only when the section has a starred photo), "%d edited" + Save only with
    unsaved edits, the description line (placeholder "Add a description"; inline editing is M6),
    the accent bar on the selected section, the 32-high sticky form, right-click = the section
    context menu with later items disabled; ids per control-protocol §7.
  - done when: headless tests find every `section:<id>/…` part with its §5.2 size at both window
    sizes; disabled buttons are disabled; a sticky header shows its first line only; the header
    region fingerprint approved on the PR.
- ⬜ **M4.10 `ui` + `ui-kit`: thumbnail cells, badges and captions**
  - after: M4.6
  - files: `crates/ui/src/panels/light_table/cell.rs`, `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/badges.rs`, `crates/ui-kit/tests/badges.rs`, `crates/ui-kit/tests/fingerprints/badges*.json`, `crates/ui/tests/cells.rs`, `crates/ui/tests/fingerprints/region-cells*.json`
  - scope: `ui-layout.md` §5.3, §5.5 and `design-system.md` §7: letterboxed image with its drop
    shadow, selection outlines, hover surround; the badge row (edited, geotag, held, star) and
    the video marks as one ui-kit painter fed what the model knows (star and held now, the rest
    when their milestones add the data); caption modes (Filename, Date, Resolution now; Caption
    and Tags empty until M6), middle-elided; hidden photos at 40 % with Hidden Pictures, small
    pictures omitted unless Small Pictures; the "!" tile with its reason; the 600 ms hover tooltip
    (file name, date taken, pixel size, file size).
  - done when: a ui-kit badge sheet (every combination, three cell sizes) matches its
    fingerprint; headless tests: each caption mode's text, small pictures hidden and shown,
    the hidden-photo opacity rule, the tooltip after 600 ms of virtual time and not before.
- ⬜ **M4.11 `ui`: the speed-control scrollbar and scroll keys**
  - after: M4.6
  - files: `crates/ui/src/panels/light_table/scrollbar.rs`, `crates/ui/src/commands/scroll.rs`, `crates/ui/tests/scrollbar.rs`, `crates/engine/src/settings.rs`, `docs/ui-layout.md`
  - scope: `ui-layout.md` §5.6: ⇑ ▲ track ▼ ⇓ with auto-repeat, track click pages (one row + 90,
    animated), ⇑/⇓ to the previous/next header (⇑ first snaps to the current section's start);
    the speed thumb: 16 × 64 resting at the centre, 4 pt dead zone, speed as a pure function of
    the distance (quadratic, top 40 screen heights/s at the track end), stop on release and a
    150 ms spring-back; the faint 16 × 3 position marker; the curve exponent and top speed read
    from settings so they can be tuned without a rebuild (documented in §5.6); scroll commands
    with keys: PgUp/PgDn, Ctrl+↑/↓ (previous/next section), Ctrl+Home/End; ids per §7.
  - done when: `crates/ui/tests/scrollbar.rs` on virtual time: the speed curve at sample
    distances, nothing inside the dead zone, the top speed at the track end; release stops the
    view and the thumb is back at the centre after 150 ms; ⇑/⇓ land headers at the top; the
    marker tracks the position; the keymap has no conflicts.
- ⬜ **M4.12 `ui`: never-blank thumbnails on screen**
  - after: M4.6, M4.7, M4.8
  - files: `crates/ui/src/panels/thumbnails.rs`, `crates/ui/tests/never_blank.rs`
  - scope: `Thumbnails::paint` drawn from the session's ladder: the covering size, else the next
    smaller scaled up, else the sketch drawn the same frame, the `gray 38` placeholder only for a
    photo never scanned; textures per (photo, ladder size) evicted beyond ±1 screen (cap ~600);
    uploads per frame capped to stay in the frame budget; scroll direction and speed passed to the
    session's prefetch; perf counters for placeholder and upscaled cells (`ui.inspect`).
  - done when: a headless test scrolls a scanned 10 k-photo synthetic library at top thumb speed
    and counts zero placeholder cells on every frame, with textures within the cap; once the view
    stops, every visible cell draws its covering size and the region fingerprints are unchanged.
- ⬜ **M4.13 `ui`: the selection model and keyboard navigation**
  - after: M4.5, M4.6
  - files: `crates/ui/src/selection.rs`, `crates/ui/src/commands/select.rs`, `crates/ui/src/panels/light_table/input.rs`, `crates/ui/tests/selection.rs`
  - scope: `ui-layout.md` §5.4: click, Ctrl-click, Shift range from the anchor, rubber band from
    empty space (`lightTable`), all scoped to one section (selecting elsewhere replaces it);
    a whole section as a unit (`select.section`, used by the tree); arrows (Shift extends),
    Home/End, stopping at the section's ends; `select.all`, `select.none`, `select.invert`,
    `select.starred` (Ctrl+A / D / I) per section; right-click on an unselected photo selects it
    first; Enter is left to the edit view (M7).
  - done when: `crates/ui/tests/selection.rs`: each gesture by widget id; a range and a band never
    cross a section; arrows and Home/End stop at section edges; the four commands per section;
    every command's graceful-failure test; `panic_hunt` green.
- ⬜ **M4.14 `xtask bench` and the light-table benchmark**
  - after: M4.6, M4.7
  - files: `xtask/src/bench.rs`, `crates/ui/benches/library_bench.rs`, `docs/development.md`
  - scope: `development.md` §3, §8: `cargo xtask bench [name]` builds benches in release, runs
    them with `--json`, appends to `target/bench/history.jsonl` and flags a CPU-time regression
    over 20 % against the last run on the same machine; `library_bench` scrolls a synthetic
    100 k-photo library headlessly at a fixed speed and reports p50/p95/max UI frame time and
    cells visited per frame at 10 k and 100 k (row "scroll light table 100k").
  - done when: `cargo xtask bench library_bench` prints the p95 and the cells-visited counts; a
    test with a planted history flags a 25 % slowdown and not a 10 % one; `xtask docs` lists
    `bench` as implemented; numbers for this machine in the PR.
- ⬜ **M4.A Checkpoint A check-in**
  - after: M4.10, M4.11, M4.12
  - owner: maintainer
  - files: `docs/status.md`
  - scope: the lead runs `/milestone-review M4.A` to prepare it: a check-in page with the demo
    library at both window sizes, cells with badges and captions, and a short screen recording or
    step list for scrolling with the speed thumb, with the settings keys to tune its curve; the
    decisions so far; the `status.md` numbers on a `review/M4.A` branch, a pull request open. Only
    the maintainer merges that pull request and closes this issue; nothing after it starts until
    then.
  - done when: prepared: page published, PR open, both linked on the issue.
- ⬜ **M4.15 `ui`: the library tree pane**
  - after: M3.7, M3.8, M4.4, M4.5, M4.6, M4.7, M4.13
  - files: `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/folders/**`, `crates/ui/tests/tree.rs`, `crates/ui/tests/fingerprints/region-tree*.json`
  - scope: `ui-layout.md` §4 on M3.8's pane frame: group headers (30, disclosure, count; collapsed
    headers docked at the bottom), rows (24, icon, name middle-elided, dimmed count), year
    dividers (18), tree-mode indent and disclosure triangles, the selection look; click =
    `section.scrollTo` + `select.section`; collapsing a group hides its sections; with the pane
    focused ↑/↓ move the row selection (scrolling the light table) and ←/→ collapse/expand in tree
    mode; only visible rows built and painted, cached per revision; Flat | Tree and the
    view-options popup drive M4.5's commands; ids per control-protocol §7.
  - done when: `crates/ui/tests/tree.rs` on a fixture library: row ids and order in both modes and
    every sort; a row click leaves its header at the top after 0.2 s and selects the folder as a
    unit; collapsing Folders removes its sections; a 5 k-folder synthetic library stays within
    the frame budget with rows visited flat.
- ⬜ **M4.16 `ui`: the Photo Tray, Hold, Clear and tray actions**
  - after: M4.13
  - files: `crates/ui/src/tray.rs`, `crates/ui/src/commands/tray.rs`, `crates/ui/src/panels/bottom.rs`, `crates/ui/src/panels/bottom/**`, `crates/ui/tests/tray.rs`, `crates/ui/tests/fingerprints/region-tray*.json`
  - scope: `ui-layout.md` §6.2–§6.3: the tray mirrors the selection live plus held photos; a
    section selected as a unit shows one token ("Folder selected — 48 photos"); mini thumbnails
    shrinking with the count (54 … 18, up to three rows); the tray's own selection (click, band);
    `tray.hold` (Ctrl+H, the held badge) and `tray.clear` (tray-selected items, or all, asking
    "Clear all photos from the tray?" when items are held); Add to Album ▾ and later context-menu
    items disabled; star acts on the whole tray in one undo step (stars all unless all are
    starred), rotate disabled until its command; "Select some photos first" with an empty tray.
  - done when: `crates/ui/tests/tray.rs`: select 2 in folder A, Hold, select 3 in folder B → the
    tray has 5 → star → one undo restores all 5; the token for a section; the confirmation only
    with held items; mini-thumbnail sizes at 1, 12, 40 and 100 items; graceful-failure tests.
- ⬜ **M4.17 `ui`: the status line**
  - after: M3.8, M4.16
  - files: `crates/ui/src/panels/status.rs`, `crates/ui/src/status_text.rs`, `crates/ui/tests/status_line.rs`, `crates/ui/tests/fingerprints/region-status*.json`
  - scope: `ui-layout.md` §6.1 from the tray's contents: "No selection"; one photo (name, date
    taken, pixel size, file size); several photos or a section (count, date range, size on disk,
    tags when tagged); the edit-view prefix and "(12 of 48)" as a tested format for M7; long text
    drops the prefix first, then middle-elides the name; transient messages for 4 s on virtual
    time.
  - done when: `crates/ui/tests/status_line.rs` matches the §6.1 string of each case at both
    window sizes, the elision order, and the 4 s transient message.
- ⬜ **M4.18 `accept M4`**
  - after: M4.9, M4.10, M4.11, M4.12, M4.14, M4.15, M4.17
  - files: `tests/acceptance/m4.rs`, `tests/acceptance/m4_*.rs`, `tests/acceptance/m4.md`, `tests/acceptance/fingerprints/m4-*.json`
  - scope: every *Done when* of M4's three areas: the 100 k scroll benchmark (p95 ≤ 8 ms UI time
    in the release bench; inside `cargo xtask ci` the debug run checks the counters and prints the
    time) with cells visited flat from 10 k to 100 k; layout, sticky-header and section-jump
    tests; the thumb's speed curve, dead zone and spring-back; zero placeholder cells at top thumb
    speed through a sketched 100 k library within budget; tree rows, order, click and collapse;
    the tray scenario with one undo step; status-line strings; keyboard stops at section edges;
    the manual check (scroll the demo library with the thumb and tune its curve) in `m4.md`.
  - done when: `cargo xtask accept M4` passes everything in M4's *Done when*.
- ⬜ **M4.19 Lead: task blocks for M6**
  - after: M3.12
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M6 task block from M6's section and the spec, with waves and the `M6.R`
    release gate (owner: maintainer).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M4.R Check-in**
  - after: M4.18
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the lead runs `/milestone-review M4.R` to prepare it: the check-in page (the demo library
    at both sizes; scrolling with the speed thumb and tuning its curve written as a step for the
    maintainer to try), *Where we stand* and the `status.md` numbers (100 k p95 frame time, packed
    against loose reads, sketch and index sizes) on a `review/M4.R` branch, a pull request open.
    Only the maintainer merges that pull request and closes this issue; nothing after it starts
    until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M4.1 sketch  ‖  M4.2 sketch in the index  ‖  M4.3 pack cache  ‖  M4.4 tree model     (once M3.R is closed)
         M4.5 view state + command modules (after M3.3–M3.6)
wave 2   M4.6 light-table skeleton  ‖  M4.7 synthetic libraries  ‖  M4.8 ladder + scan-time thumbnails
wave 3   M4.9 header  ‖  M4.10 cells + badges  ‖  M4.11 scrollbar  ‖  M4.12 never blank
         ‖  M4.13 selection  ‖  M4.14 bench
         → M4.A checkpoint (after M4.10, M4.11, M4.12)
wave 4   M4.15 tree pane  ‖  M4.16 tray
wave 5   M4.17 status line
wave 6   M4.18 accept M4 → M4.R          (M4.19 lead: M6 blocks, any time after M3.12 once M4.A is closed)
```

### M5 — Folders, scanning and formats

One milestone in 2 feature areas (*Folders and scanning*, *Formats and colour*); one `cargo xtask accept M5` covers every *Done when* below.

#### Folders and scanning

- **Goal:** the library follows the user's folders by itself (`data-model.md` §7,
  `organizing.md` §1–2).
- **In:** Folder Manager (roots and volumes loaded lazily, Scan Always / Scan Once / Remove,
  inheritance, watched list, confirmations); first-run panel; `notify` watching + periodic
  verification; external creates, deletes, renames and modifications; folders moved outside Dalica
  keep their data; offline volumes (dimmed, cached thumbnails, queued edits written on return);
  skip rules; the disk thumbnail cache (2 GiB, pruning); the activity indicator with its job list
  and Cancel; scan progress on the status line; stale `.dalica-tmp-*` cleanup (`security.md` §5.8);
  writes relative to a held folder handle (`security.md` §5.6, through `rustix`; approved at M1.A).
- **Out:** new formats (M5), legacy sidecars (M14), face finding (M13).
- **Done when:** `cargo xtask accept M5`: temp-dir scenarios (add, remove, rename while running;
  the verification pass finds everything; a volume "unplugged" by renaming its root shows offline,
  a star made meanwhile is written when it returns); benchmark: first thumbnails of a 10 k-JPEG
  folder ≤ 1 s, full scan time recorded. You run first-run on a fresh library directory and watch
  thumbnails fill in.

#### Formats and colour

- **Goal:** every format of `formats.md` §1 shows correctly and safely.
- **In:** TIFF, WebP, GIF, BMP, JPEG XL, PSD composite, RAW embedded previews (our own IFD walker),
  CMYK and 16-bit sources, ICC through `moxcms`, decode limits on every path, "!" tiles with
  reasons, HEIF behind the `heif` feature, View → Use Color Management, `cargo xtask corpus` with
  floors, a fuzz target per parser, TIFF-strip EXIF thumbnails.
- **Out:** writing metadata (M6), video (M15), AVIF (no decoder available).
- **Done when:** `cargo xtask accept M5`: decode tests per format from `scenes` at several depths
  and profiles; orientation 1–8 per container; malformed-input property tests never panic; corpus
  floors hold; a mixed-format folder snapshot matches its fingerprint.

### M6 — Photo info, metadata and Release 0.1

One milestone in 3 feature areas (*Photo info*, *Tags and metadata write-back*, *Release 0.1 (organizer preview)*); one `cargo xtask accept M6` covers every *Done when* below. It ends with a release gate the maintainer owns.

#### Photo info

- **Goal:** the light organizing a user does on single photos and folders, stored in sidecars.
- **In:** rotate CW/CCW from the library (recipe op, rotated thumbnails; the first `edit`
  operations: rotate and flip); hide/unhide photos and folders, View → Hidden Pictures, the Hidden
  Folders group; captions (inline editing in the library, Thumbnail Caption modes); folder
  description editing in the header; Folder Properties (date, place, description); the Properties
  pane (read-only metadata, Show All Metadata); Adjust Date and Time.
- **Out:** writing into files (M6), the edit view (M7), renaming folders (M9).
- **Done when:** `cargo xtask accept M6`: the command contract for every new command; rotated
  thumbnail fingerprints; hide, captions and dates survive a rebuild. You rotate, hide and caption
  photos in the app.

#### Tags and metadata write-back

- **Goal:** tags in the UI, and captions, tags and dates written into the photo files without ever
  harming them (`xmp-interop.md`).
- **In:** tag commands and the Tags pane (add, partial counts, remove, quick tags and their
  dialog, Ctrl+Alt+1…0, Rename Tag); `meta` write-back for JPEG, PNG, WebP and TIFF; the metadata
  write queue with re-read, merge and verify; the conflict rules of `xmp-interop.md` §5; importing
  file metadata on first scan; `.xmp` sidecars for RAW (option); the metadata corpus.
- **Out:** face regions (M13), the GPS user interface (M13).
- **Done when:** `cargo xtask accept M6`: `xmp-interop.md` §8 tests 1–4 and 6–10 pass; files
  written by other applications in the corpus keep every non-owned byte after a Dalica write. You
  tag a photo in Dalica and see the tag in another photo application.

#### Release 0.1 (organizer preview)

- **Goal:** something installable to try on real folders.
- **In:** the release workflow (draft releases with checksums); Linux AppImage, Flatpak and deb/rpm, Windows
  MSI/zip, macOS DMG (unsigned where there are no credentials); About window with credits; the
  user-facing `whats-new.md`; README status banner; the application id (reserved decision 10).
- **Out:** signing without credentials, update checks (deferred).
- **Done when:** `cargo xtask accept M6`: a CI dry run builds the artifacts on all three OSes and
  the checksums match. You install it and browse your own photos.

### M7 — Edit view and fixes

One milestone in 3 feature areas (*Edit view shell*, *Basic fixes*, *Tuning and retouch*); one `cargo xtask accept M7` covers every *Done when* below.

#### Edit view shell

- **Goal:** open a photo and move around in it (`editing.md` §1–3, §5.1–5.2).
- **Prerequisite:** decide the coordinate frame (`data-model.md` §3.3).
- **In:** layout (top bar, filmstrip, edit panel with tab placeholders, photo area, caption bar,
  Histogram & Camera Info); entering and leaving (double-click, Enter, Ctrl+3, Esc, Ctrl+1);
  previous/next; zoom and pan maths, Fit / 1:1, background refinement; the per-photo history
  commands with labelled Undo/Redo buttons; star, rotate and flip in the edit view as history steps.
- **Out:** tools and effects (M7+), compare modes (M11), video (M15).
- **Done when:** `cargo xtask accept M7`: zoom maths tests (ends and midpoint = 100 %); navigation
  tests; a photo's history survives a restart; opening a 24 MP JPEG renders its screen preview in
  ≤ 150 ms (recorded).

#### Basic fixes

- **Goal:** the Basic Fixes tab, and edits shown everywhere (`editing.md` §6).
- **In:** the `edit` renderer with pipeline order and per-operation versions; thumbnails show
  edits (recipe in cache keys); Crop (ratios, custom ratios, Manual, rotate, preview; suggestions
  from thirds/saliency); Straighten; Auto Fix, Auto Contrast, Auto Color; the Fill Light slider.
- **Out:** face-based crop suggestions (M13), Retouch (M7).
- **Done when:** `cargo xtask accept M7`: fingerprints per operation; resolution independence (two
  sizes agree after downscale); thumbnail render = edit-view render; unknown operations survive.

#### Tuning and retouch

- **In:** the Tuning tab as one live-updated operation (one undo step per tab session), Auto
  lighting and Auto color, the Neutral Color Picker; the Retouch tool with in-tool patch undo.
- **Done when:** `cargo xtask accept M7`: a slider drag updates the screen preview in ≤ 16 ms
  draft (recorded); a whole drag is one history step; fingerprints per control.

### M8 — Save and export

One milestone in 2 feature areas (*Save to disk*, *Export*); one `cargo xtask accept M8` covers every *Done when* below.

#### Save to disk

- **Goal:** the only path that changes pixels, with the original always recoverable
  (`data-model.md` §6).
- **In:** Save (first-time dialog, originals backup, quality matching, metadata carried over),
  Undo Save, Revert, Save a Copy, Save As; the folder header's Save button and "n edited";
  lossless JPEG rotation (or the EXIF flag fallback); non-writable formats → JPEG beside.
- **Done when:** `cargo xtask accept M8`: Save → Undo Save → Save → Revert returns the exact
  original bytes (`data-model.md` §10.9); fault injection during Save never loses the original;
  every format of `formats.md` §1 saves as specified.

#### Export

- **In:** the Export dialog (`creating.md` §2), the background export job with progress and
  Cancel, resize, quality presets, watermark, metadata copy and optional GPS removal, collision
  names, the Exported project group, adding an unwatched export folder.
- **Done when:** `cargo xtask accept M8`: exported files are decoded and checked (size, quality
  bounds, metadata, GPS removed when asked); Cancel leaves only complete files; a 200-photo export
  of a mixed tray succeeds.

### M9 — Albums, search and file operations

One milestone in 3 feature areas (*Albums*, *Search and filters*, *File operations*); one `cargo xtask accept M9` covers every *Done when* below.

#### Albums

- **In:** user albums (New Album, properties, add via tray, context menu or drop on an album row,
  remove, manual order with position keys (re-keying an album when a key would pass 512 bytes,
  `data-model.md` §3.2), Sort Album By, cover, delete); Starred Photos and
  Recently Updated; the Albums group and album sections; the Album menu variant.
- **Done when:** `cargo xtask accept M9`: `data-model.md` §10.12 (cross-folder order survives a
  rebuild); undo for every album command.

#### Search and filters

- **In:** the search grammar, instant search on a worker with debounce, autocomplete, the filter
  buttons and age slider, search mode (banner, Show All, scroll restore), the temporary results
  album, Save Search as Album, Find Duplicates (identical files).
- **Out:** `color:` search and `person:` before People (decided at this milestone's review).
- **Done when:** `cargo xtask accept M9`: grammar tests; search of 100 k photos answers within its
  budget (recorded); the banner text and counts match the spec.

#### File operations

- **In:** move (drag onto a tree folder, Cut/Paste), copy, the collision dialog, Move to New
  Folder, Split Folder Here, move/rename/delete folder, Rename and batch rename, delete to the
  trash, the drag pill; sidecar entries and originals backups travel with their photos; undo where
  possible; "don't ask again" settings.
- **Out:** dragging out to other applications (M15).
- **Done when:** `cargo xtask accept M9`: `data-model.md` §10.2, §10.3 and §10.7; undo tests per
  operation; a move across two file systems copies, verifies, then deletes.

### M10 — Preferences, MCP and Release 0.2

One milestone in 3 feature areas (*Preferences and shortcuts*, *MCP and workflows*, *Release 0.2 (P0 complete)*); one `cargo xtask accept M10` covers every *Done when* below. It ends with a release gate the maintainer owns.

#### Preferences and shortcuts

- **In:** the Options dialog (tabs; OK / Cancel / Apply rules); General, File Types and Export tabs;
  UI scale and font size; the shortcut editor (`menus-and-shortcuts.md` §2.0) and Help → Keyboard
  Shortcuts.
- **Done when:** `cargo xtask accept M10`: settings round-trip with unknown keys kept; keymap
  conflict tests; rebinding Star to F8 through `app.setShortcut` and pressing F8 headless stars
  the photo; Reset All restores every default.

#### MCP and workflows

- **In:** `dalica-cli mcp` (headless and connect modes), one tool per command plus helpers, roots
  and permissions (`mcp.md`); workflow acceptance tests for every P0 flow.
- **Done when:** `cargo xtask accept M10`: every P0 workflow passes over MCP in CI; the behaviour
  pass rate is recorded in *Where we stand*.

#### Release 0.2 (P0 complete)

- **Done when:** every P0 parity row is ✅ with evidence; P0 workflows pass 100 %; performance
  budgets for P0 scenarios are enforced; an honest *Where we stand*; a 0.2 release.

### M11 – M14: P1 parity (outline; the lead details each one while the one before runs)

| # | In | Done when (sketch) |
|---|---|---|
| M11 Import | rooms/tab strip; sources (cards, removable drives, folders); time grouping; exclude duplicates; destination and folder naming; copy-verify; optional removal from the card | import of a simulated card into dated folders; duplicates skipped; no file overwritten |
| M11 Compare and batch edit | A│B, A│A, before/after hold, Copy/Paste All Edits, Batch Edit ▸, Undo All Edits | batch of 100 photos is one undo step |
| M11 Slideshow and quick preview | slideshow, control bar, transitions, captions, music; Ctrl+Alt preview | transitions never wait on disk (prefetch test) |
| M11 Photo viewer | `dalica --view`, filmstrip, controls, settings | open, navigate, star from the viewer |
| M12 Effects I | effect tiles, parameter panels, crosshair control, Fade; the 12 effects of tab 3 | a fingerprint and an invariant per effect |
| M12 Effects II | the 15 effects of tabs 4–5, canvas-enlarging effects, Soften's paint mask | as M12 |
| M13 People I | consented model download (decisions 5 and 9), detection, embeddings, clustering, Unnamed, naming, ignore, person albums | a public-domain face set groups as expected (floors) |
| M13 People II | suggestions, People pane, manual faces, merge/rename/delete, MWG regions | MWG round-trip tests (`xmp-interop.md` §8.5) |
| M13 Places | Places pane, opt-in map tiles (decision 4), geotag by drag and search, GPS write, Geotag menu | GPS round-trip; no network without the user's action |
| M14 Collage | the collage room, styles, canvas, render + recipe, Edit Collage | deterministic layouts per seed; render size |
| M14 Legacy sidecars | read-only reader with provenance header, first-run notice, conversion on first write, legacy originals for Revert | `data-model.md` §10.11 |

### M15 — Video, localisation, OS integration and Release 1.0 (outline)

| # | In | Done when (sketch) |
|---|---|---|
| M15 Video | reserved decision 2 first; then poster frames, playback, trim, frame save — or descope to tiles | decided and recorded; tests per chosen scope |
| M15 Localisation and accessibility | translation catalogs, AccessKit labels, screen-reader and keyboard-only passes, scaling | coverage per language reported; AccessKit tree tests |
| M15 OS integration | drag out to other apps, clipboard files, Open With ▸, external editor, Locate on Disk with selection, default-viewer registration, MTP/PTP cameras | per-OS manual checklist plus what can be tested per OS in CI |
| M15 Release 1.0 | signed packages, full honest assessment, release notes | the release checklist (`releasing.md`) |

## Reserved decisions (the maintainer's, not the agents')

Record the decision here with a date when made.

1. Reuse model. **Decided 2026-10-09: reference only.** LightCraft and PhotoCraft are prior art for
   technology, look and methodology; no Craft code is depended on or vendored. crates.io
   libraries stay allowed after the dependency check (`architecture.md` §9).
2. Video: pure-Rust decoding vs optional external process (M15).
3. Metadata write defaults. **Decided 2026-10-09:** captions, tags, places and date changes are
   written into the photo files' metadata by default (pixels never change without Save); stars as
   XMP ratings off.
4. Map tiles provider and whether maps are on by default (M13).
5. Face-recognition models and their licences (M13).
6. Password-protected hidden folders. **Decided 2026-10-09: no.**
7. Licence. **Decided 2026-10-09: MIT OR Apache-2.0**, the same as PhotoCraft.
8. Community process (Discord/GitHub Discussions) and release cadence. The repository is public.
9. TLS stack for the few network features (map tiles, model downloads): a pure-Rust provider vs
   accepting `ring`, or delegating downloads to the OS. See `security.md` (M13).
10. Application id (reverse-DNS, used by packaging, desktop files and the settings path) (M6).
11. Coordinate frame for normalised coordinates (`data-model.md` §3.3) (M7).
