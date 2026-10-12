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

*2026-10-11 (M4.R).* M4 is done: `cargo xtask accept M4` passes in 90 s (155 s with
`DALICA_ACCEPT_RELEASE=1`, which also asserts the release frame budgets). Dalica now browses a
library: the folder tree in flat and tree modes with year dividers, sorts and the view-options
popup; the continuous light table with 96-high section headers, sticky pinned headers, justified
cells with badges, captions and the 0.6 s tooltip; the speed-control scroll bar; selection by click,
Shift, Ctrl and rubber band inside one folder with keyboard navigation; and the Photo Tray with
Hold, Clear and the status line that follows it. Scrolling 100 000 photos costs 0.26 ms per frame
at p95 in release (budget 8 ms) and cells visited stay flat from 10 k to 100 k; with the speed thumb
held at top speed through 100 k sketched photos no cell is ever grey. Thumbnails are made at scan
into a packed cache (faster than loose files in order and on first reads, slower on shuffled reads:
M5.16 looks into it) with a 24–26-byte colour sketch per photo in the index. The "scanned 100 k"
case is checked at 10 k until M5.16's bench scans 100 k. Open: the speed thumb's feel (M5.R); the
release budgets get a Linux CI job (M5.24); `cargo xtask ci` warm went from 488 s to 85 s on the
review branch (each acceptance test runs once, image and UI crates optimised in dev builds); 36 open items are placed in M5 blocks or listed on the check-in page.

*2026-10-10 (M3.R).* M3 is done: `cargo xtask accept M3` passes in 41 s. It drives the real
`dalica --headless` on a fresh demo and checks every region and shell control at 1400 × 900 and
900 × 600 against the layout spec, the side pane, the splitter and F11, and the live menu tree
against all 112 items of `menus-and-shortcuts.md` §1 read from the document; it also runs the
contrast, widget-gallery and icon suites and matches three whole-window fingerprints. The window now
has a drawn title bar (caption buttons, drag, resize; the system menu bar on macOS through `muda`),
the full menu bar with menus that stay inside the window, the toolbar, the folder tree with its
splitter, the status line, the bottom area with More… below 1100 wide, and the side-pane frame;
narrow windows give way in the spec's order, F11 toggles full screen, and window size, tree width
and the open pane survive a restart. Keys follow the platform (Redo is Cmd+Shift+Z on macOS) and UI
commands join the keymap. 605 tests; `cargo xtask ci` takes 266 s cold and 158 s warm; GitHub CI
4–8.5 min per OS. Twelve tasks ran with four workers in about three hours; `main` went red twice on
macOS (a menu layout drawn per OS, then an acceptance floor), each fixed by a PR within the run's
standing go-ahead. Not yet seen by a person: F11 filling the real screen. Next: M4, library browsing.

*2026-10-10 (M2.R).* M2 is done: `cargo xtask accept M2` passes in 12 s (a control session against
`dalica --headless` on a fresh demo: auth, click a thumbnail, Ctrl+8, the star in the sidecar, a
1400 × 900 screenshot matching its fingerprint; plus the 9 control security tests), green on Linux,
macOS and Windows. Agents can now drive and look at the real window: every interactive widget has an
id per `control-protocol.md` §7 (a test finds none missing), the headless driver clicks, drags,
scrolls, types and settles, `ui.inspect` reports perf counters (cells visited stay flat from 200 to
2 000 photos), and `dalica --control [PORT]` serves the protocol on 127.0.0.1 only, with a 0600 token
file, `auth` first in constant time, line, depth, connection and time limits; `--headless` serves the
same without a window. `--demo` is persistent with deterministic ids; `--demo-fresh` is a throwaway
copy. The board ran four agents in parallel for the first time; the day also settled the process:
one review per milestone, at its end; a milestone's issues are created when the previous review
closes; runs are unattended, with questions, a UI changelog and fingerprint approval saved for the
review. Next: M3, the window shell.

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
- **Every review task is a stop.** A milestone has exactly one review, at its end: the review task
  `M<n>.R`. There are no mid-milestone reviews (the maintainer's decision, 2026-10-10: "I do not
  want any checkins mid milestones"); the roadmap parser refuses an open `M<n>.A`, `M<n>.B`, …
  (the finished M1.A stays as history). Every review task is `owner: maintainer` (the roadmap
  parser refuses any other owner), and nothing listed after an open review task in this file is
  claimed until the maintainer closes it, whether in the same milestone or a later one
  (`cargo xtask tasks next` and `claim` enforce it).
- **The lead prepares a review; the maintainer decides it.** When the run reaches a review task,
  running tasks finish and merge, and the lead runs `/milestone-review M<n>.R`: a check-in page
  (the questions only the maintainer can answer, asked there; a UI changelog with before/after
  screenshots and what to try; every decision made with a way to reverse it),
  the dated *Where we stand* and the `status.md` numbers on a branch `review/M<n>.R`, and a pull
  request linked on the review issue. Then the run stops. Only the maintainer merges that pull
  request and closes the review issue; no agent and no `cargo xtask tasks` command merges or closes
  a review. The maintainer then starts the next run (`/milestone-start`); feedback becomes tasks.
- **A milestone runs unattended; its review asks the questions** (the maintainer's process,
  2026-10-10). Agents work through the milestone's whole backlog without human intervention. A
  decision that needs the maintainer mid-milestone is recorded (`tasks block`; the blocked task
  waits, the rest go on) and asked at the review. The review brings a short list of questions,
  as few as possible, each with options and a recommendation, and a changelog of what changed in
  the UI so the maintainer can check it by eye. The maintainer answers, checks the UI, closes
  `M<n>.R` (by merging its pull request) and starts the next milestone.
- **Releases are reviews too.** M6 (0.1), M10 (0.2) and M15 (1.0) end with a release gate `M<n>.R`;
  its check-in adds the release checklist and the sign-off questions.
- **The lead plans ahead.** The lead writes the next milestone's task blocks while the current one
  runs, by the end of its first wave, so agents never wait for a plan. Task blocks need no
  approval; the check-in shows them.
- **A milestone's tasks become issues when the previous review closes.** Until the maintainer
  closes `M<n-1>.R`, M<n>'s blocks are planning text and `cargo xtask tasks sync` creates no
  issue for them; a block with `pulled: M<k>` brings that one task forward into milestone k's run
  (the maintainer's rule, 2026-10-10; `task-board.md` §2–§3).
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
| After 1.0 | M16 | Post-1.0 enhancements | what comes after the honest 1.0, starting with switchable themes | ⬜ |

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
- ✅ **M2.R Check-in**
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
starts as soon as the maintainer has closed M2.R. First fingerprints of new widgets and icons are
approved in bulk on the M3.R review page.

Whole-window baselines belong to no task: `crates/ui/tests/fingerprints/library-*.json` and the
finished milestones' `tests/acceptance/fingerprints/m1-*.json` and `m2-*.json` change whenever the
window does. A task whose change moves them re-blesses them on its branch (diff images on the PR,
the reason in the commit body), they never count as a `files` overlap, and on a rebase conflict the
worker re-blesses after the rebase instead of merging the JSON by hand. The maintainer approves them
with the rest at the next review.

- ✅ **M3.1 `ui-kit`: the full widget set**
  - after: M2.1, M2.2
  - files: `crates/ui-kit/src/theme.rs`, `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/**`, `crates/ui-kit/tests/gallery.rs`, `crates/ui-kit/tests/widgets*.rs`, `crates/ui-kit/tests/contrast*.rs`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `crates/ui-kit/tests/fingerprints/widgets*.json`, `docs/design-system.md`
  - scope: every token and metric of `design-system.md` §2–§4 not yet in `Tokens` (divider, inset,
    field, field_border, track, thumb, thumb_hover, tray_bg, drop_target, filter_banner, face_box,
    shadows, UI scale and font-size factors); the widgets of §6 the shell needs: `primary_button`,
    segmented buttons, `action_button`, `toggle_filter`, `slider`, `zoom_slider`, `section_header`,
    `dropdown`, `search_field` and text field, tooltips, `progress_row`, `empty_state`, `toast`,
    the splitter and the `dialog` frame; each registers its id per control-protocol §7.
  - done when: `cargo test -p dalica-ui-kit` renders a gallery of every widget in every state
    (rest, hover, pressed, active, disabled, focused) identically twice and matches its
    fingerprints; contrast tests cover every text/background pair of §9.
- ✅ **M3.2 `ui-kit`: shell icons drawn in code**
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
- ✅ **M3.3 `ui`: menu model, menu catalog and menus that never run off the window**
  - after: M2.1, M2.2
  - files: `crates/ui/src/menu_catalog.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/control/menu.rs`, `crates/ui/src/control/mod.rs`, `crates/ui/tests/menus.rs`
  - scope: the complete menu tree of `menus-and-shortcuts.md` §1 as data (`menu_catalog.rs`): label,
    command id, shortcut shown, separators, submenus; items whose id has a registered command are
    live, the rest drawn disabled; `UI_COMMANDS` move from `app.rs` to `menus.rs`; in-window menus,
    submenus and context menus bounded by the window and scrolling when too tall
    (`design-system.md` §11.2); `ui.menu.tree` and `ui.menu.invoke` (control-protocol §6.10), moved
    from `later(…)` to live methods in `control/mod.rs`; the `later` notes of `ui.dialog.confirm` /
    `ui.dialog.cancel` say M5 (the Folder Manager is the first in-app dialog), not M3.
  - done when: a test walks `ui.menu.tree` and finds every item of §1 in order; a headless snapshot
    at 900 × 600 shows the longest menu scrolling inside the window; invoking a disabled item is
    `disabled`, an unknown path `not_found`.
- ✅ **M3.4 `engine` + `ui`: per-platform default keys and keys for UI commands**
  - after: M3.3
  - files: `crates/engine/src/keymap.rs`, `crates/engine/src/registry.rs`, `crates/engine/tests/keymap.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/menus.rs`
  - scope: default bindings per platform (`menus-and-shortcuts.md` §2: Redo is Cmd+Shift+Z on macOS,
    Ctrl+Y elsewhere); `UI_COMMANDS` declare keys and join the keymap and its conflict check (F11
    full screen, Ctrl+1/Ctrl+2 thumbnail size, Ctrl+0 side pane); shortcuts shown in menus come from
    the keymap, so a rebinding shows up there.
  - done when: no default conflicts on any platform (tested for all three); rebinding a UI command
    with `app.setShortcut` changes the key shown in its menu item.
- ✅ **M3.5 `ui`: the layout skeleton and geometry tests**
  - after: M2.1, M2.2
  - files: `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/layout.rs`, `crates/ui/src/panels/toolbar.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/src/panels/side_pane.rs`, `crates/ui/src/state.rs`, `crates/ui/tests/layout.rs`
  - scope: every region of `ui-layout.md` §2 laid out at its specified size (menu/title row 24,
    toolbar 40, tree 240 with an 8 pt splitter, light table, status line 20, bottom area 96 split at
    0.365·W, side pane 280 when open), each a stub panel in its own module so M3.6–M3.9 never share
    a file; each region registers its rect; `UiState` gains the view state of control-protocol §6.5
    (tree width, pane, thumb size, full screen).
  - done when: `crates/ui/tests/layout.rs` reads every region's rect from the widget registry at
    1400 × 900 and 900 × 600 and matches §2.
- ✅ **M3.12 Lead: task blocks for M5**
  - after: M2.9
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M5 task block from M5's section and the spec, with waves and the `M5.R`
    review (owner: maintainer).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ✅ **M3.6 `ui` + `dalica`: title bar, window frame and window state**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/titlebar.rs`, `crates/ui/src/panels/menubar.rs`, `apps/dalica/src/**`, `apps/dalica/Cargo.toml`, `crates/engine/src/settings.rs`, `tests/acceptance/m2.md`
  - scope: `design-system.md` §11.1: the menu row is the title bar on Windows and Linux (app mark,
    menus, centred title, caption buttons 46 × 24, drag, double-click maximize, 5 pt resize edges,
    Close runs File → Exit); the "Use system title bar" setting; macOS keeps native traffic lights
    and gets the system menu bar from the same menu model through `muda` (a macOS-only dependency;
    `cargo xtask deps`); window position, size, maximized state and tree width persisted;
    `ui.screenshot` of a minimised or covered window: `tests/acceptance/m2.md` promises a CPU-drawn
    image (`headless: true`) while M2.4 answers `timeout` after 2 s; make them agree (draw it, as the
    spec's `headless` fallback says, or correct `m2.md`) with a test.
  - done when: headless snapshots of the title bar at both sizes; the menu model and the macOS menu
    list the same items (test); window state survives a restart (test through settings).
- ✅ **M3.7 `ui`: the toolbar**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/panels/toolbar.rs`, `crates/ui/tests/toolbar.rs`
  - scope: `ui-layout.md` §3: Import, New Album, Flat | Tree, view options ▾ popup, the Filters group
    with its caption, age slider, search field, activity indicator; controls without a command yet
    are present and disabled; ids per control-protocol §7.
  - done when: a test finds every toolbar id at both sizes with the sizes of §3; the filter caption
    hides below 1100 wide.
- ✅ **M3.8 `ui`: tree pane, splitter, status line and side-pane frame**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/src/panels/side_pane.rs`, `crates/ui/src/state.rs`, `crates/ui/src/control/view.rs`, `crates/ui/tests/tree_status_pane.rs`
  - scope: the tree pane frame with the Folders group header (30) and rows (24), resizable through
    the splitter (200 … W − 480, persisted); the status line texts of `ui-layout.md` §6.1; the
    side-pane frame of §7 (header with «, title, ×; 280 or large; pushes the light table, closes
    itself below 480; open state persisted); pane contents stay empty until their milestones;
    `ui.state` reports the status line's text in `status` (M2.8 found it empty while the line showed
    "1 of 60 pictures selected").
  - done when: a test drags the splitter by id and the tree width follows within its limits and
    survives a restart; status-line texts for no selection, one photo and a folder, and `ui.state`
    reports the same text; the pane opens, widens and closes by id.
- ✅ **M3.9 `ui`: the bottom area**
  - after: M3.1, M3.2, M3.3, M3.4, M3.5
  - files: `crates/ui/src/panels/bottom.rs`, `crates/ui/tests/bottom.rs`
  - scope: `ui-layout.md` §6.2–§6.5: the tray box (empty watermark; Hold, Clear, Add to Album
    present and disabled until M4), star and rotate buttons (rotate disabled until its command),
    loupe, the thumbnail-size slider (Ctrl+1 / Ctrl+2), the four pane toggles, the action buttons
    with the More… overflow below 1100 wide.
  - done when: a test finds every bottom-area id with its §6 size at both sizes; More… holds the
    action buttons at 900 × 600; the size slider changes the light table's cell size.
- ✅ **M3.10 `ui`: responsiveness rules and full screen**
  - after: M3.6, M3.7, M3.8, M3.9
  - files: `crates/ui/src/panels/layout.rs`, `crates/ui/tests/layout.rs`
  - scope: `ui-layout.md` §11 and `design-system.md` §11.1 "narrow windows": chrome drops in the
    stated order, the tree shrinks before the light table drops below 480, the side pane closes;
    F11 full screen; the geometry test extended to every control of M3.6–M3.9.
  - done when: the geometry test passes at 1400 × 900, 1100 × 700 and 900 × 600 with the side pane
    open and closed, and in full screen.
- ✅ **M3.11 `accept M3`**
  - after: M3.10
  - files: `tests/acceptance/m3.rs`, `tests/acceptance/m3.md`, `tests/acceptance/fingerprints/m3-*.json`
  - scope: M3's *Done when*: geometry of every region at both sizes, token contrast, widget-gallery
    fingerprints, every menu item of `menus-and-shortcuts.md` §1 present in order; the manual check
    (open the app at both sizes and compare with the docs) written down.
  - done when: `cargo xtask accept M3` passes everything in M3's *Done when*.
- ✅ **M3.R Check-in**
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
wave 1   M3.1 widgets  ‖  M3.2 icons  ‖  M3.3 menus  ‖  M3.5 layout skeleton  ‖  M3.12 lead (M5 blocks)
wave 2   M3.4 keys (after M3.3)
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
per part (layout, header, cell, scrollbar, input) with a fixed painting API. Each task updates its `parity.md` rows; first fingerprints of new
widgets are approved in bulk on the M4.R review page; M4.6 splits the window fingerprints by region so parallel
UI tasks each re-bless only their own.

- ✅ **M4.1 `thumbs`: the colour sketch**
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
- ✅ **M4.2 `library` + `store`: the sketch in the index**
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
- ✅ **M4.3 `thumbs` + `fs`: the packed disk cache**
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
- ✅ **M4.4 `library`: the tree model**
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
- ✅ **M4.5 `ui`: M4's view state and command modules**
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
    Sort of the view-options popup, group collapse; `ui.set` accepts the new fields. Loose ends
    found at M3.R: File ▸ Exit as the UI command `app.quit` (the title bar's Close already runs it),
    View ▸ Properties / Tags / People / Places live through the pane state the bottom toggles use,
    and menu check marks that follow the state.
  - done when: `crates/ui/tests/view_state.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes) and the persisted ones survive a
    restart through settings; the view commands change state and light up their menu and popup
    items; `ui.menu.tree` reports File ▸ Exit and the four View pane items live; every command has
    its graceful-failure test; `panic_hunt` stays green.
- ✅ **M4.20 `ui-kit`: dim text meets 4.5:1 on the chrome**
  - after: M3.R
  - files: `crates/ui-kit/src/theme.rs`, `crates/ui-kit/tests/contrast.rs`, `crates/ui-kit/tests/fingerprints/widgets-*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `crates/ui-kit/README.md`, `docs/design-system.md`
  - scope: the maintainer's answer at M3.R (2026-10-10): lighten `text_dim` so it reaches 4.5:1 on
    `chrome` (about `#949494`; check `surface` and `hover` too and say what they reach); the accent
    focus ring and the slider track stay as they are. The contrast test's pinned 4.20 for that pair
    becomes a ≥ 4.5 assertion. Re-bless the widget sheets and the whole-window baselines (diff
    images on the PR; only dim text may change). Also brings `crates/ui-kit/README.md`'s public API
    list up to date (M3.1 and M3.2 open items: the M3 widgets, `paint_activity`, `Icon::ALL`).
  - done when: `cargo test -p dalica-ui-kit --test contrast` asserts `text_dim` on `chrome` ≥ 4.5:1
    (it fails on today's token); the re-blessed images differ only in dim text; the README lists the
    current API.
- ✅ **M4.6 `ui`: the light-table skeleton and layout**
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
- ✅ **M4.7 `engine`: synthetic libraries**
  - after: M4.1, M4.2
  - files: `crates/engine/src/synthetic.rs`, `crates/engine/tests/synthetic.rs`
  - scope: deterministic generated libraries for tests, benches and `accept M4`: N photos in M
    folders (100 k in 1 k folders; 5 k folders) with mixed aspects, dates over several years and
    real sketches from the M4.1 encoder, written as an index snapshot through `store` and opened
    with `Session::open` (no photo files); an option with small `scenes` files on disk for the
    scanned case at smaller sizes.
  - done when: tests: the same seed gives the same library; a 100 k library opens with every
    photo sketched; generation and open times for 100 k (release) are in the PR.
- ✅ **M4.8 `thumbs` + `engine`: the thumbnail ladder and scan-time generation**
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
- ✅ **M4.9 `ui`: the section header block**
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
- ✅ **M4.10 `ui` + `ui-kit`: thumbnail cells, badges and captions**
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
- ✅ **M4.11 `ui`: the speed-control scrollbar and scroll keys**
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
- ✅ **M4.12 `ui`: never-blank thumbnails on screen**
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
- ✅ **M4.13 `ui`: the selection model and keyboard navigation**
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
- ✅ **M4.14 `xtask bench` and the light-table benchmark**
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
- ✅ **M4.15 `ui`: the library tree pane**
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
- ✅ **M4.16 `ui`: the Photo Tray, Hold, Clear and tray actions**
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
- ✅ **M4.17 `ui`: the status line**
  - after: M3.8, M4.16
  - files: `crates/ui/src/panels/status.rs`, `crates/ui/src/status_text.rs`, `crates/ui/tests/status_line.rs`, `crates/ui/tests/fingerprints/region-status*.json`
  - scope: `ui-layout.md` §6.1 from the tray's contents: "No selection"; one photo (name, date
    taken, pixel size, file size); several photos or a section (count, date range, size on disk,
    tags when tagged); the edit-view prefix and "(12 of 48)" as a tested format for M7; long text
    drops the prefix first, then middle-elides the name; transient messages for 4 s on virtual
    time.
  - done when: `crates/ui/tests/status_line.rs` matches the §6.1 string of each case at both
    window sizes, the elision order, and the 4 s transient message.
- ✅ **M4.18 `accept M4`**
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
- ✅ **M4.19 Lead: task blocks for M6**
  - after: M3.12
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M6 task block from M6's section and the spec, with waves and the `M6.R`
    release gate (owner: maintainer).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ✅ **M4.R Check-in**
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
         M4.5 view state + command modules (after M3.3–M3.6)  ‖  M4.20 dim-text contrast (M3.R answer)
wave 2   M4.6 light-table skeleton  ‖  M4.7 synthetic libraries  ‖  M4.8 ladder + scan-time thumbnails
wave 3   M4.9 header  ‖  M4.10 cells + badges  ‖  M4.11 scrollbar  ‖  M4.12 never blank
         ‖  M4.13 selection  ‖  M4.14 bench
wave 4   M4.15 tree pane  ‖  M4.16 tray
wave 5   M4.17 status line
wave 6   M4.18 accept M4 → M4.R          (M4.19 lead: M6 blocks, any time after M3.12)
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

#### M5 tasks

M5 makes the library follow the user's folders and show every listed format. Its first wave
(`image`, `scenes`, `fs`, `scan`) touches nothing M3 or M4 changes, so it lists the M1 work it
extends and starts as soon as the maintainer has closed M4.R; the engine and UI tasks build on M4's
session, thumbnail ladder, tree and panels. Three tasks lay the ground the others share, so later
tasks never share a file: M5.1 gives `image` one module per format behind one decoded-image model
and colour stage, M5.5 splits the session into one module per concern (scanning, folders, jobs,
watching, offline) with a job registry, and M5.14 adds the dialog host, M5's view state and stub
modules for the UI tasks. Each task updates its `parity.md` rows (M5.1 adds one row per format
group, `DATA-FMT-*`, for the decoder tasks to tick); first fingerprints of new decoders, dialogs and
icons are approved in bulk on the M5.R review page.

Whole-window baselines belong to no task: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m4-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase instead
of merging the JSON by hand. The maintainer approves them with the rest at the review.

Pure Rust holds throughout (`AGENTS.md` §3), and no block needs a C dependency: decoders come from
the crates pinned in `architecture.md` §1 (each audited with `cargo xtask deps` when it first enters
the product graph); HEIF stays behind the off-by-default `heif` feature; AVIF is listed, never
decoded. `notify` 8 pulls `inotify-sys` (Linux) and `fsevent-sys` (macOS), binding-only crates
with no C build; the maintainer allowed both at M3.R (2026-10-10), so M5.11 adds them to `SYS_ALLOW`
with that reason and makes `notify` the default watcher, with a pure-Rust polling backend as the
fallback. Either way the verification pass stays the truth (`data-model.md` §7). Our own CC0
`pkkid/dalica-corpus` repository is created by the maintainer before M5 starts (M3.R answer) and
filled by the lead; M5.15 pins it.

- ⬜ **M5.1 `image`: one module per format, the decoded-image model and colour management**
  - after: M1.6
  - files: `crates/image/src/lib.rs`, `crates/image/src/decode.rs`, `crates/image/src/sniff.rs`, `crates/image/src/limits.rs`, `crates/image/src/color.rs`, `crates/image/src/formats/**`, `crates/image/tests/decode.rs`, `crates/image/tests/color.rs`, `crates/image/tests/fingerprints/color*.json`, `crates/image/fuzz/**`, `crates/image/README.md`, `.github/workflows/fuzz.yml`, `docs/formats.md`
  - scope: `formats.md` §1, §4–§5: a `Decoded` value between container and display (8/16-bit
    integer or float samples; grey, RGB, CMYK, palette expanded; alpha; embedded ICC bytes;
    orientation) and one colour stage to upright 8-bit sRGB: ICC v2/v4 through `moxcms` (audit with
    `cargo xtask deps`), untagged = sRGB, an unusable profile falls back to sRGB (logged), CMYK
    without a profile converted naively, ICC limits of `security.md` §4.2; a `DecodeOptions`
    with `color_managed` (View → Use Color Management; off = embedded profiles ignored); JPEG APP2
    profile reassembly, CMYK/YCCK JPEG, PNG `iCCP` and 16-bit PNG through it; `formats/` with one
    module per format (header for scanning, decode, embedded thumbnail), JPEG and PNG moved there,
    stubs answering `Unsupported` for TIFF, WebP, GIF, BMP, JPEG XL, PSD, HEIF and `formats/raw/`
    (`tiff_based.rs`, `cr3.rs`, `raf.rs`, `crw.rs`, `mrw.rs`, `x3f.rs`), so M5.6–M5.10 never share
    a file; `sniff` tells the RAW containers apart by content; `catch_unwind` around every
    third-party decoder ("Damaged file"); fallible allocations charged to the allocation limit;
    work counters on the container walks we own; `Error::reason()` gives the tile texts of §1;
    `crates/image/fuzz/` (a separate cargo-fuzz workspace outside the product graph, excluded in the
    root manifest) with a target per parser of `security.md` §7.1 (`sniff`, `decode_any`, one
    `decode_<format>` per format, `raw_preview`, `embedded_thumbnail`, `icc`) and a manually
    dispatched fuzz workflow; the `DATA-FMT-*` parity rows.
  - done when: `cargo test -p dalica-image`: the JPEG and PNG decodes of M1 still match their
    fingerprints; `scenes` photos tagged with Display P3, Adobe RGB and ProPhoto profiles match
    their sRGB references within a stated tolerance, with `color_managed` off they do not; a
    damaged or oversized profile decodes as sRGB; 16-bit PNG and CMYK JPEG match fingerprints; a
    planted panic inside a decoder is "Damaged file"; every stub format sniffs and answers its
    reason; truncation and proptest random bytes never panic on any path; `cargo +nightly fuzz
    build` builds every target (output in the PR).
- ⬜ **M5.2 `scenes`: test files in every M5 format**
  - after: M1.4
  - files: `crates/scenes/src/formats.rs`, `crates/scenes/src/formats/**`, `crates/scenes/src/icc.rs`, `crates/scenes/tests/formats.rs`, `crates/scenes/README.md`
  - scope: procedural photos written as TIFF (8/16-bit and float; grey, RGB, CMYK, palette;
    uncompressed, LZW, Deflate, PackBits; Orientation tag; embedded thumbnail IFD; ICC tag), WebP
    (lossless, with alpha, EXIF and ICCP chunks), GIF (one and several frames), BMP, a PSD composite
    (our own writer: raw and RLE, 8/16-bit grey, RGB, CMYK, indexed, with thumbnail and ICC
    resources), JPEG with an APP2 profile and, where an encoder can write it, CMYK; ICC profiles for
    sRGB, Display P3, Adobe RGB, ProPhoto and Rec.2020 built in code from their published primaries
    (v2 and v4); JPEG XL through an audited pure-Rust encoder if one passes `cargo xtask deps`
    (otherwise JPEG XL tests rely on hand-built codestreams and the corpus). Encoders come from the
    pinned `image` crate (audit with `cargo xtask deps`); no media committed.
  - done when: `cargo test -p dalica-scenes`: every writer is deterministic (same seed, same
    bytes); each file sniffs as its format and reads back through the `image` crate's own decoder
    (or a structural check for PSD and ICC); profiles parse with `moxcms`.
- ⬜ **M5.3 `fs`: writes through a held folder handle and stale temp cleanup**
  - after: M1.2, M1.3
  - files: `crates/fs/src/atomic.rs`, `crates/fs/src/dirfd.rs`, `crates/fs/src/stale.rs`, `crates/fs/src/faults.rs`, `crates/fs/tests/dirfd.rs`, `crates/fs/tests/stale.rs`, `crates/fs/README.md`, `docs/security.md`
  - scope: `security.md` §5.6: on Unix, `atomic::replace` opens the target's folder once
    (`O_DIRECTORY | O_NOFOLLOW`), checks it is the folder that was validated, and does every step
    relative to that handle through `rustix` (approved at M1.A; audit with `cargo xtask deps`):
    exclusive temp `openat`, write, fsync, identity re-check, `renameat`, folder fsync, read-back
    with `O_NOFOLLOW`; Windows keeps the identity comparison before and after opening; §5.8:
    `remove_stale_temps(folder, now)` removes only files of exactly the `.<name>.dalica-tmp-<random>`
    shape older than 24 hours, never through a link, and reports what it removed; fault points on
    the new steps; the §5.6 and §5.8 status lines.
    Found at M4.R: `crates/fs/README.md` also lists M4.3's `pack` module.
  - done when: `cargo test -p dalica-fs`: a folder swapped for a link between validation and write
    (planted through a fault point) leaves the write in the original folder or fails, never writes
    through the link; the fault loop over every step leaves the old or the new file; stale cleanup
    removes a 25-hour-old temp and keeps a 23-hour-old one, a near-miss name, a foreign file and a
    link; tests pass on all three OSes in CI.
- ⬜ **M5.4 `scan`: skip rules, volumes, standard places and per-file guards**
  - after: M1.11
  - files: `crates/scan/src/walk.rs`, `crates/scan/src/skip.rs`, `crates/scan/src/volumes.rs`, `crates/scan/src/lib.rs`, `crates/scan/tests/scan.rs`, `crates/scan/tests/skip.rs`, `crates/scan/tests/volumes.rs`, `crates/scan/README.md`
  - scope: `data-model.md` §7 and `security.md` §4.3: the skip rules in one module (dot entries,
    `.dalica-originals`, OS system folders and cache/temp folders per platform, the list written
    in the README) plus per-type toggles (`FileTypes` of Options → File Types; JPEG always on); every
    recognised format listed by content, a file whose header cannot be read listed at 0 × 0 (its
    reason comes from the thumbnail job); `scan` takes excluded subtrees (folders set to Remove
    under a watched root) and streams per-folder results through a channel as it goes, with
    progress counts and the cancel flag; each file's header read inside `catch_unwind` with a
    10-second budget, failures returned with their (path, size, mtime) and skipped when given back
    unchanged; `volumes`: standard places (Pictures, Documents, Desktop, Home through
    `directories`, overridable for tests), mounted volumes per OS, `online(root)`, and a bounded,
    sorted `children(path)` listing of subfolders for the Folder Manager.
  - done when: `cargo test -p dalica-scan`: each skip rule on a temp tree; toggles drop exactly
    their formats; an excluded subtree is never walked; results arrive per folder before the walk
    ends and a cancelled scan stops at the next file; a planted panicking or slow file is reported
    once and not retried until its mtime changes; `children` of a 10 000-folder tree stays within
    its cap; places honour the override; a renamed-away root reports offline.
- ⬜ **M5.5 `engine`: session modules, the job registry and progressive scans**
  - after: M4.8, M5.4
  - files: `crates/engine/src/session.rs`, `crates/engine/src/scanning.rs`, `crates/engine/src/folders.rs`, `crates/engine/src/jobs.rs`, `crates/engine/src/watching.rs`, `crates/engine/src/offline.rs`, `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/view.rs`, `crates/engine/src/cmd/section.rs`, `crates/engine/tests/jobs.rs`, `crates/engine/tests/progressive_scan.rs`, `crates/engine/README.md`
  - scope: `architecture.md` §5: `session.rs` split so M5.11–M5.17 never touch it: `scanning.rs`
    (scan jobs), `folders.rs` (watched roots and `folders.json`, moved over unchanged), `jobs.rs`,
    and stub `watching.rs` and `offline.rs` called from `poll()`; empty `cmd/view.rs` and
    `cmd/section.rs` registered once; a job registry per `control-protocol.md` §6.13 (id, kind,
    label, state, progress, cancel flag, result or error; kept 10 minutes, at most 64, on the
    session clock) with `jobs()`, `cancel(job | all)` and `wait(job, timeout)`; scans become jobs
    whose streamed batches are applied a few hundred photos per `poll()`, so folders and thumbnails
    fill in progressively, with scan progress (`running, foldersQueued, found, indexed`) and
    thumbnail generation queued for each batch as it lands; `is_new_library()` for the first-run
    panel.
    Found at M4.R: `crates/engine/README.md` also lists M4.7's `synthetic` and M4.8's `thumbs`.
  - done when: engine tests on a temp library of 2 000 `scenes` photos in 40 folders: the first
    batch is applied and its photos have thumbnail jobs queued before the walk finishes; progress
    counts rise to the totals; cancelling the scan job stops it within one folder and keeps what
    was applied; the registry expires entries after 10 minutes of virtual time and caps at 64;
    every existing engine test and `panic_hunt` pass unchanged.
- ⬜ **M5.6 `image`: TIFF**
  - after: M5.1, M5.2
  - files: `crates/image/src/formats/tiff.rs`, `crates/image/tests/tiff.rs`, `crates/image/tests/fingerprints/tiff*.json`
  - scope: `formats.md` §1: the first full-resolution page through the pinned `image`/`tiff`
    decoders (8/16-bit integer and float; grey, RGB, CMYK, palette; uncompressed, LZW, Deflate,
    PackBits, JPEG), straight to `Decoded` where `image` refuses a layout (CMYK, float); header
    (size, Orientation, EXIF) for scanning; the embedded thumbnail IFD as the fast path; the ICC
    tag into the colour stage; multi-page files show page one; Lab and YCbCr answer `Unsupported`;
    the decode limits checked against the IFD before any strip is read.
  - done when: `cargo test -p dalica-image --test tiff`: `scenes` TIFFs at every depth, colour
    model and compression match their fingerprints; orientation 1–8; a tagged profile converts;
    the thumbnail IFD is used for a small target; a header bomb is refused before allocating;
    truncation at every offset and proptest random bytes never panic.
- ⬜ **M5.7 `image`: WebP, GIF and BMP**
  - after: M5.1, M5.2
  - files: `crates/image/src/formats/webp.rs`, `crates/image/src/formats/gif.rs`, `crates/image/src/formats/bmp.rs`, `crates/image/tests/webp_gif_bmp.rs`, `crates/image/tests/fingerprints/webp*.json`, `crates/image/tests/fingerprints/gif*.json`, `crates/image/tests/fingerprints/bmp*.json`
  - scope: `formats.md` §1: WebP lossy and lossless with alpha, the first frame of an animation,
    orientation from its EXIF chunk and its ICCP profile; GIF's first frame (frame count and
    allocation bounded per `security.md` §4.2); BMP; headers for scanning; all through the pinned
    decoders under the decode limits.
  - done when: `cargo test -p dalica-image --test webp_gif_bmp`: `scenes` files of each format
    match their fingerprints; WebP orientation 1–8 and its profile; an animated GIF and WebP show
    frame one; oversized headers are refused before allocating; truncation and proptest random
    bytes never panic.
- ⬜ **M5.8 `image`: JPEG XL and PSD composites**
  - after: M5.1, M5.2
  - files: `crates/image/src/formats/jxl.rs`, `crates/image/src/formats/psd.rs`, `crates/image/tests/jxl.rs`, `crates/image/tests/psd.rs`, `crates/image/tests/fingerprints/jxl*.json`, `crates/image/tests/fingerprints/psd*.json`
  - scope: `formats.md` §1: JPEG XL through `jxl-oxide` (audit with `cargo xtask deps`; the
    decoder applies orientation; ICC into the colour stage); PSD and PSB through our own reader of
    the header, the image resources (thumbnail, ICC) and the merged composite (raw and RLE;
    8/16/32-bit grey, RGB, CMYK, indexed), with the provenance header the clean-room rule asks for;
    layers are never parsed; ZIP-compressed composites and Lab answer `Unsupported`; every length
    and count checked before use.
  - done when: `cargo test -p dalica-image --test jxl --test psd`: JPEG XL files (from M5.2 or
    built in the test) and `scenes` PSDs at each depth and colour model match their fingerprints;
    a PSD's thumbnail resource serves a small target; a declared size past the limits is refused
    before allocating; truncation and proptest random bytes never panic.
- ⬜ **M5.9 `image`: RAW previews from TIFF-structured raws, and TIFF-strip EXIF thumbnails**
  - after: M5.1, M5.2
  - files: `crates/image/src/formats/raw/tiff_based.rs`, `crates/image/src/ifd.rs`, `crates/image/src/exif.rs`, `crates/image/tests/raw_tiff.rs`, `crates/image/tests/fingerprints/raw-tiff*.json`
  - scope: `formats.md` §2: DNG, CR2, NEF, ARW, PEF, ORF, RW2 and SRW recognised by content; the
    largest embedded JPEG preview found by our own bounded walk (SubIFD arrays and the vendor
    preview pointers that are plain IFD offsets; seen-set, depth and entry caps of §4.2), previews
    tagged Adobe RGB converted through the colour stage, orientation from IFD0; a raw without a
    usable preview answers the generic RAW tile; EXIF IFD1 thumbnails stored as uncompressed TIFF
    strips (not only as JPEG) used as the fast path in JPEG, TIFF and raw files.
  - done when: `cargo test -p dalica-image --test raw_tiff`: synthetic raws of each kind built in
    the test from public structure descriptions around a `scenes` JPEG yield the largest preview
    and match its fingerprint; orientation 1–8; a strip thumbnail decodes; IFD loops, offsets
    past the end and huge counts are `Err`; truncation and proptest random bytes never panic.
- ⬜ **M5.10 `image`: RAW previews from other containers, HEIF behind its feature, AVIF tiles**
  - after: M5.1, M5.2
  - files: `crates/image/src/formats/raw/cr3.rs`, `crates/image/src/formats/raw/raf.rs`, `crates/image/src/formats/raw/crw.rs`, `crates/image/src/formats/raw/mrw.rs`, `crates/image/src/formats/raw/x3f.rs`, `crates/image/src/formats/bmff.rs`, `crates/image/src/formats/heif.rs`, `crates/image/tests/raw_containers.rs`, `crates/image/tests/heif.rs`, `.github/workflows/ci.yml`
  - scope: `formats.md` §2–§3: embedded previews from CR3 (through a bounded ISOBMFF box walk:
    nesting ≤ 12, boxes per level capped), RAF, CRW, MRW and X3F, each a small reader with a
    provenance header (MRW's TIFF block read through `ifd` without changing it); HEIF/HEIC through
    `heic-rs` behind the non-default `heif` feature (forwarded by `dalica`; reviewed per
    `security.md` §8.6, the review in the commit body), a panic-guarded wrapper for single
    pictures and grids, 8–12 bits, alpha, `irot`/`imir`, ICC; without the feature, HEIC tiles say
    "HEIC support isn't included in this build"; AVIF is sniffed and shown as a format tile, never
    decoded; a CI step tests `dalica-image` with `--features heif` on the three OSes.
  - done when: `cargo test -p dalica-image --test raw_containers` (synthetic containers around a
    `scenes` JPEG, built in the test) finds each preview; `cargo test -p dalica-image --features
    heif --test heif` passes the structural tests (rotation, mirror, grid layout, unsupported
    sequences answer `Unsupported`) and, without the feature, the tile reason; box loops and
    oversized boxes are `Err`; truncation and proptest random bytes never panic.
- ⬜ **M5.11 `scan`: the watcher and incremental verification**
  - after: M5.4
  - files: `crates/scan/src/watch.rs`, `crates/scan/src/watch/**`, `crates/scan/src/diff.rs`, `crates/scan/tests/watch.rs`, `crates/scan/tests/incremental.rs`, `xtask/src/deps.rs`
  - scope: `data-model.md` §7: events are hints, verification is the truth. A `Watcher` interface
    reporting dirty folders (debounced, coalesced, bounded queue); the `notify` 8 backend is the
    default (add `inotify-sys` and `fsevent-sys` to `SYS_ALLOW` in `xtask/src/deps.rs`, reason:
    bindings only, no C build, allowed by the maintainer at M3.R; audit with `cargo xtask deps`),
    and a pure-Rust backend polling folder modification times in sweeps sized to the library is the
    fallback when watching fails or a volume cannot be watched;
    incremental verification of dirty folders and a full verification on a schedule; `diff`
    scoped to the folders a rescan covered, with folder renames and moves detected like photo
    renames (same files by size and mtime) so ids, stars and the undo stack survive; a folder that
    vanished under a root that cannot be walked is never removed.
  - done when: `cargo test -p dalica-scan`: on temp trees the polling backend reports adds,
    deletes, renames and rewrites within two sweeps (virtual time where possible); the incremental
    diff of a dirty folder equals the full diff restricted to it; a renamed folder keeps its ids;
    10 000 queued events coalesce within the queue cap; the `notify` backend passes the same tests
    with events as hints and generous timeouts, and the polling fallback passes them alone;
    `cargo xtask deps` accepts the two new `*-sys` entries and still refuses an unknown one.
- ⬜ **M5.12 `scan` + `engine`: folder modes, inheritance and the Folder Manager commands**
  - after: M5.4, M5.5
  - files: `crates/scan/src/rules.rs`, `crates/scan/tests/rules.rs`, `crates/engine/src/folders.rs`, `crates/engine/src/scanning.rs`, `crates/engine/src/settings.rs`, `crates/engine/src/cmd/folders.rs`, `crates/engine/tests/folders.rs`, `docs/data-model.md`
  - scope: `data-model.md` §4, §7 and `organizing.md` §2: Scan Always, Scan Once and Remove per
    folder, inherited by subfolders unless set (effective mode = nearest explicit ancestor);
    Scan Once roots indexed once, then frozen; `folders.json` v2 (modes, exclusions, the "Find
    faces in this folder" flag kept for M13) reading v1 files; commands: `folders.set {changes}`
    (one library undo step), `folder.remove {id, confirm}` (`needs_confirm` with the "Remove
    Folder" text), `folders.browse {path?}` (standard places and volumes, then children, answered
    from a worker), `folders.list` (the watched list with friendly names); a volume root or the
    home folder set to Scan Always needs `confirm` with the "Watch the whole drive" text;
    `folders.add` becomes a Scan Always change; the File Types toggles join settings and the scans.
  - done when: `crates/engine/tests/folders.rs` and `crates/scan/tests/rules.rs`: inheritance and
    overrides on a temp tree; Remove drops a subtree from the library without touching a file,
    and undo brings it back; Scan Once is not rescanned by verification; a v1 `folders.json` opens
    and is written back as v2; confirmations over automation; every command's graceful-failure
    test; `panic_hunt` green.
- ⬜ **M5.13 `engine` + `store`: offline volumes and queued writes**
  - after: M4.4, M5.4, M5.5
  - files: `crates/engine/src/offline.rs`, `crates/engine/tests/offline.rs`, `crates/store/src/writer.rs`, `crates/store/tests/offline.rs`, `crates/store/README.md`, `crates/library/src/tree.rs`, `crates/library/tests/tree.rs`
  - scope: `data-model.md` §7, `security.md` §5.9: a worker-filled cache of which roots are online
    (checked on a schedule and after each scan, never on the owner thread); folders under an
    offline root stay in the library and the tree (`offline` on each row, for M5.19 to dim) and
    keep their cached thumbnails; edits to their photos are journalled at once and their sidecar
    writes wait: the writer keeps the folder dirty, never creates a missing folder to write into,
    reports `pendingWrites`, and writes everything when the root returns.
    Found at M4.R: `crates/store/README.md` also lists M4.2's sketch ops and `PHOTO_DATA`.
    Found at M4.R: in tree mode a group's header count includes directory rows that are not
    library folders ("Folders (212)" against 200 in flat mode); it counts library folders only.
  - done when: tests: a volume "unplugged" by renaming its root shows offline with its photos and
    rows intact; a star made meanwhile is journalled, no folder is created at the old path, and
    after renaming the root back the sidecar holds the star; `pendingWrites` counts it in between;
    the tree property tests cover the offline flag; the owner thread made no `fs` call.
- ⬜ **M5.14 `ui`: the dialog host, M5's view state and job methods**
  - after: M4.5, M4.16, M5.5
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/tray.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/dialogs.rs`, `crates/ui/src/dialogs/**`, `crates/ui/src/panels/activity.rs`, `crates/ui/src/control/**`, `crates/ui/tests/dialogs.rs`, `crates/ui/tests/jobs_control.rs`, `docs/control-protocol.md`
  - scope: `design-system.md` §6 `dialog` and `control-protocol.md` §6.6, §6.13: a modal host in
    `crates/ui/src/dialogs/mod.rs` (backdrop, Esc = Cancel, Enter = default, `dialog:<kind>` and
    `button:dialog.*` ids, `ui.inspect` → `dialog`), with M4.16's tray confirmation moved onto it;
    stub `dialogs/folder_manager.rs`, `dialogs/first_run.rs` and `panels/activity.rs` for M5.18
    and M5.19; `UiState` gains the open dialog with its draft and the activity popover;
    `folders.manager` (File → Add Folder… and Tools → Folder Manager…) as a UI command;
    `ui.dialog.confirm` / `ui.dialog.cancel` and `jobs.list` / `jobs.wait` / `jobs.cancel` move
    from `later(…)` to live methods over M5.5's registry. Reuse (`architecture.md` §9):
    PhotoCraft's `crates/ui-egui/src/dialogs.rs` is the pattern (dialogs as data in `UiState`,
    set, confirmed and cancelled by automation). Loose ends found at M4.R: `ui.inspect`
    reports the light table's `scroll` and running `animations` (M4.6), `ui.menu.tree
    {context:"tray"}` answers the tray menu (M4.16), `tray.clear` asks through the host instead of
    returning `disabled`, `ui.inspect` reports the open `popup`, `dialog` and `tooltip` (they read
    null while on screen), `ui.screenshot {region}` works, and `commands/mod.rs`'s doc line for
    `select` is brought up to date.
  - done when: `crates/ui/tests/dialogs.rs`: a dialog opens by command, Esc and Enter close it as
    specified, `ui.inspect` reports it, the tray confirmation still works through the host;
    `ui.inspect` reports scroll and animations and `ui.menu.tree {context:"tray"}` lists the tray
    menu's rows;
    `crates/ui/tests/jobs_control.rs`: a scan job is listed with progress, `jobs.wait` returns its
    final entry, `jobs.cancel` stops it; bad params are `bad_params`; `widget_ids` stays green.
- ⬜ **M5.15 `xtask corpus` and `test-corpus` with floors**
  - after: M5.6, M5.7, M5.8, M5.9, M5.10
  - files: `xtask/src/corpus.rs`, `xtask/src/corpus_pins.rs`, `xtask/corpus/**`, `xtask/src/docs.rs`, `crates/image/tests/corpus.rs`, `crates/image/tests/corpus_floors.json`, `crates/image/README.md`, `.github/workflows/ci.yml`, `.gitignore`, `docs/development.md`, `docs/formats.md`, `docs/security.md`
  - scope: `development.md` §11: `cargo xtask corpus` (status, `--all`, `--local`,
    `--update-manifest`), pins in `corpus_pins.rs`, sha256 manifests, `corpus/` gitignored;
    `cargo xtask test-corpus [-p crate]`; the `corpus` feature of `dalica-image` with committed
    floors per set (files decoded, previews extracted) that a missing or stale corpus fails; the
    CI `corpus` job with its cache. Sets that exist today are pinned to their upstreams under
    their own terms (PngSuite; CC0 raw samples per vendor); the `photos` and `heif` sets come from
    our own CC0 `pkkid/dalica-corpus` repository, which the maintainer creates before M5 starts and
    the lead fills (public-domain or CC0 files only, each with its source; M3.R answer, 2026-10-10),
    pinned the same way. Brings `formats.md` and
    `security.md` §4 and §7 statuses up to date with M5's decoders, guards and fuzz targets.
  - done when: `cargo xtask corpus --all` fetches and verifies the pinned sets; `cargo xtask
    test-corpus` passes with floors at the measured counts (numbers in the PR); a planted stale
    file fails verification; `xtask docs` lists both commands as implemented; no corpus file is
    tracked by git.
- ⬜ **M5.16 `engine` + `thumbs`: cache upkeep, colour management and the first-thumbnails benchmark**
  - after: M4.3, M4.14, M5.1, M5.5, M5.13
  - files: `crates/engine/src/thumbs.rs`, `crates/engine/src/cmd/view.rs`, `crates/engine/src/cmd/section.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/thumb_cache.rs`, `crates/engine/benches/first_thumbnails.rs`, `crates/engine/src/synthetic.rs`, `crates/thumbs/src/upkeep.rs`, `crates/thumbs/tests/upkeep.rs`, `crates/thumbs/README.md`, `crates/ui/src/menus.rs`, `crates/ui/tests/color_management.rs`
  - scope: `architecture.md` §6: the cache size setting (2 GiB default), pruning as a background
    job at start and whenever the cache passes its limit, entries of removed photos made dead,
    entries of offline photos kept and never regenerated while offline; a photo that cannot be
    decoded answers its `formats.md` §1 reason, remembered by (path, size, mtime) and not retried
    until it changes; `view.colorManagement` (View → Use Color Management, checkable, on by
    default, persisted) switches `DecodeOptions::color_managed`, joins the cache key and refreshes
    visible thumbnails; `section.refreshThumbs` (Folder → Refresh Thumbnails); a just-added
    folder's first screenful is generated before the rest, from embedded thumbnails where they
    cover the cell; `first_thumbnails` bench: time from `folders.add` to the first screenful
    having thumbnails for 1 000 JPEGs (the `development.md` §8 budget) and 10 000 JPEGs (the
    roadmap's), plus full scan time and photos per second.
    Found at M4.R: `crates/thumbs/README.md` also lists M4.1's `sketch`, M4.3's `pack` and M4.8's
    `ladder`.
    Found at M4.R: the packed cache read slower than loose files on shuffled reads in both 100 k
    runs (4.7 against 2.5 µs, and 17.9 against 2.7 µs); find why and fix it or say why it is noise.
    The maintainer's answer at M4.R (2026-10-11): M4's "scanned 100 k-photo library at full thumb
    speed, no grey cell" line was met split (100 k sketched + 10 k scanned); this bench adds the
    scanned 100 k pass: `synthetic` writes 100 k small photo files (its cap rises from 20 k), the
    scan fills the cache, and the light table is driven at top thumb speed through it.
  - done when: engine tests: pruning stops at 80 % and keeps offline entries; a broken file gets
    its reason once and is retried after a rewrite; toggling colour management changes a tagged
    photo's thumbnail and its cache key, and the menu item's check follows it; refresh regenerates
    a section; every command's graceful-failure test; `cargo xtask bench first_thumbnails` reports
    first thumbnails ≤ 1 s for 1 k and 10 k JPEGs and the full scan numbers (in the PR); the scanned
    100 k pass counts zero placeholder cells at top thumb speed with p95 within the frame budget
    (LIB-LT-INSTANT becomes ✅).
- ⬜ **M5.17 `engine` + `store`: watching and external changes end to end**
  - after: M5.3, M5.11, M5.12, M5.13
  - files: `crates/engine/src/watching.rs`, `crates/engine/src/scanning.rs`, `crates/engine/tests/watching.rs`, `crates/engine/README.md`, `crates/store/src/stale.rs`, `crates/store/src/writer.rs`, `crates/store/tests/stale.rs`, `docs/data-model.md`
  - scope: `data-model.md` §7: Scan Always roots watched through M5.11's interface, dirty folders
    rescanned as jobs, a full verification pass on the session clock; external creates, deletes,
    renames and modifications reach the library within seconds; a folder moved inside the watched
    roots keeps its data (the sidecar moves with it); stale `.dalica-tmp-*` files removed when a
    folder is scanned (M5.3); a photo deleted outside Dalica leaves the library and its sidecar
    entry is pruned after 30 days (the index remembers when it went missing; pruning happens on the
    folder's next sidecar write).
  - done when: `crates/engine/tests/watching.rs` on temp libraries while the session runs: add,
    remove, rename and rewrite files and folders and see each in the library; with the watcher
    switched off, the verification pass alone finds everything; a moved folder keeps its stars; a
    stale temp is removed and a fresh one kept; a missing photo's entry survives 29 days and is
    gone after 31 (virtual time); the owner thread made no `fs` call.
- ⬜ **M5.18 `ui`: the Folder Manager and the first-run panel**
  - after: M5.12, M5.14
  - files: `crates/ui/src/dialogs/folder_manager.rs`, `crates/ui/src/dialogs/folder_manager/**`, `crates/ui/src/dialogs/first_run.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/folder_modes.rs`, `crates/ui-kit/tests/icons_folder_modes.rs`, `crates/ui-kit/tests/fingerprints/icons-folder-modes*.json`, `crates/ui/tests/folder_manager.rs`, `crates/ui/tests/first_run.rs`, `crates/ui/tests/fingerprints/dialog-folder-manager*.json`, `crates/ui/tests/fingerprints/dialog-first-run*.json`, `docs/organizing.md`
  - scope: `organizing.md` §1–§2: the Folder Manager (≈ 640 × 480, resizable, split 50/50): the
    folder tree with its roots (Pictures, Documents, Desktop, Home, volumes) loaded lazily through
    `folders.browse` (a spinner row while loading), state icons drawn in code (Scan Always, Scan
    Once, Remove; inherited ones at 50 %), the three radios, the faces toggle, the watched list
    (a click reveals the folder), OK / Cancel / Help (Help disabled until there is help), changes
    applied on OK through `folders.set`, the drive and Remove Folder confirmations, the selected
    library folder pre-selected; the first-run panel (≈ 640 × 460) for a new library with no
    watched folders (never in `--demo`), whose Continue adds the standard places as Scan Always or
    opens the Folder Manager; ids per control-protocol §7.
  - done when: headless tests drive both dialogs by id: expanding a root lists its children
    without blocking a frame; inheritance icons and opacity; Cancel changes nothing; OK applies one
    undo step; the drive confirmation reverts on Cancel; first run with the default choice adds the
    standard places (test override) and folders appear while the scan runs; dialog and icon
    fingerprints at both window sizes; earlier milestones' acceptance tests pass unchanged.
- ⬜ **M5.19 `ui`: the activity indicator, scan progress and offline folders**
  - after: M3.7, M4.15, M4.17, M5.13, M5.14
  - files: `crates/ui/src/panels/toolbar.rs`, `crates/ui/src/panels/activity.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/src/status_text.rs`, `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/folders/**`, `crates/ui/tests/activity.rs`, `crates/ui/tests/offline_folders.rs`, `crates/ui/tests/fingerprints/activity*.json`, `crates/ui/tests/fingerprints/region-tree*.json`, `crates/ui/tests/fingerprints/region-status*.json`, `docs/ui-layout.md`
  - scope: `ui-layout.md` §3 item 8 and `organizing.md` §1: the 28 × 28 activity indicator spins
    while a job runs; a click opens a popover listing jobs with `progress_row` and Cancel
    (`jobs.cancel`); the status line shows "Scanning: 1,204 pictures found" in place of "No
    selection", and as the last field (dropped first when space is short) when something is
    selected; offline folders' tree rows dimmed with a `caution` "offline" marker; ids per
    control-protocol §7. Loose end found at M4.R: the tree pane gets its scroll bar (M4.15 left it
    wheel- and key-driven only).
  - done when: `crates/ui/tests/activity.rs`: during a scan of a temp library the indicator spins,
    the popover lists the job with rising progress and Cancel stops it, the status text counts
    up and returns to the selection text when the scan ends; `crates/ui/tests/offline_folders.rs`:
    renaming a root away dims its rows with the marker and renaming it back restores them; a tree
    longer than the pane shows a scroll bar that drags the rows; the region and popover
    fingerprints at both window sizes.
- ⬜ **M5.22 `xtask tasks`: send-backs and the parity summary (found at M4.R)**
  - after: M4.R
  - files: `xtask/src/tasks/**`, `xtask/src/parity.rs`, `docs/task-board.md`
  - scope: two frictions of the M4 run. A send-back of an In Review task: `tasks submit` accepts
    a task that is In Review when its branch was rebased, pushes with lease and keeps the pull
    request (today it refuses, and the lead rebuilt eight branches by hand). `tasks merge` resolves
    a rebase conflict that is only in `docs/parity.md`'s generated summary by regenerating it
    (`parity --write`) instead of stopping; row conflicts still go back to the worker. (The third,
    `ci` time, was fixed on the M4.R review branch at the maintainer's request: each acceptance test
    runs once per `ci`, the crate suites are not re-run inside it, nested cargo runs never see
    `DALICA_BLESS`, and the image and UI crates are optimised in dev builds; `development.md` §3.)
  - done when: `xtask` tests on a temp repository: a rebased In Review branch re-submits and its PR
    keeps its number; a summary-only conflict merges after regeneration while a row conflict is
    refused; `docs/task-board.md` describes the send-back.
- ⬜ **M5.23 `ui`: M4 loose ends in the light table and keys**
  - after: M4.R
  - files: `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/light_table/input.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/panels/bottom.rs`, `crates/ui/src/panels/bottom/**`, `crates/ui/tests/band_autoscroll.rs`, `crates/ui/tests/tray_keys.rs`, `crates/ui/tests/fingerprints/region-tray*.json`
  - scope: two gaps M4 left. A rubber band dragged to the light table's top or bottom edge scrolls
    the view (speed growing with the distance past the edge) and keeps selecting, still inside one
    section (M4.13 had no way to ask for a scroll from `input`). The photo shortcuts (Star,
    Ctrl+8 / F4; rotate when it exists) act on the tray's photos, the same set the bottom buttons
    act on, instead of only the selection (M4.16). The tray's mini thumbnails sit centred in the
    tray box as M4.16 intended (they are drawn left-aligned).
  - done when: `crates/ui/tests/band_autoscroll.rs`: a band held below the view scrolls it and
    selects the rows that come in, and never crosses into the next section;
    `crates/ui/tests/tray_keys.rs`: with 2 held and 3 selected, the star key stars all 5 in one
    undo step; the minis are centred at 1, 12 and 40 items; graceful failures; `widget_ids` stays
    green.
- ⬜ **M5.24 CI: the release frame budgets on `main` (Linux)**
  - after: M4.R
  - files: `.github/workflows/budgets.yml`, `xtask/src/accept.rs`, `docs/development.md`
  - scope: the maintainer's answer at M4.R (2026-10-11): a workflow of its own on every push to
    `main`, Linux only, builds in release and runs the frame budgets that `accept M4` asserts only
    with `DALICA_ACCEPT_RELEASE=1` (100 k scroll p95 ≤ 8 ms, thumb at top speed through 100 k
    p95 ≤ 8 ms, cells visited flat), failing the run when one is missed; later milestones add their
    budgets to the same job (M5's first thumbnails ≤ 1 s). Its own workflow file, so it never
    shares `ci.yml` with M5.10 or M5.15; cached like `ci.yml`.
  - done when: the workflow runs on a push to `main` and passes on today's code, and a planted
    slowdown (an env-gated sleep in the bench, tested in `xtask`) fails it; `development.md` lists
    the job and how to run the same check locally.
- ⬜ **M5.20 `accept M5`**
  - after: M5.15, M5.16, M5.17, M5.18, M5.19, M5.22, M5.23
  - files: `tests/acceptance/m5.rs`, `tests/acceptance/m5_*.rs`, `tests/acceptance/m5.md`, `tests/acceptance/fingerprints/m5-*.json`
  - scope: every *Done when* of M5's two areas: the temp-dir scenarios (add, remove, rename while
    running; the verification pass finds everything; a volume "unplugged" by renaming its root
    shows offline and a star made meanwhile is written when it returns); first thumbnails of a
    10 k-JPEG folder ≤ 1 s and the full scan time (the release bench; inside `cargo xtask ci` a
    smaller debug run checks the progressive path and prints its time); decode tests per format
    at several depths and profiles; orientation 1–8 per container; malformed-input property tests;
    the corpus floors (run when `corpus/` is present and verified, said plainly when it is not;
    the CI corpus job is the strict run); a mixed-format folder snapshot (every format, a HEIC and
    an AVIF tile with their reasons, a broken file's "!") matching its fingerprint; the manual
    check (first run on a fresh library directory, watching thumbnails fill in) in `m5.md`.
  - done when: `cargo xtask accept M5` passes everything in M5's *Done when*.
- ✅ **M5.21 Lead: task blocks for M7**
  - after: M4.19
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M7 task block from M7's section and the spec, with waves and the `M7.R`
    review (owner: maintainer); a block that can port Craft-app code names the source files in its
    scope (the section's *Reuse* line, `architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M5.25 `ui-kit`: text field behaviour and an autocomplete list, from the Craft apps**
  - after: M3.1
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/field.rs`, `crates/ui-kit/src/widgets/field/**`, `crates/ui-kit/src/widgets/autocomplete.rs`, `crates/ui-kit/src/registry.rs`, `crates/ui-kit/tests/fields.rs`, `crates/ui-kit/tests/fingerprints/fields*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `NOTICE`, `docs/design-system.md`
  - scope: one shared text field for M6's caption, description, tag, quick-tag and date entry, so
    those tasks do not each build one. Port LightCraft's `crates/ui-egui/src/text_field.rs` into
    `ui-kit`'s `text_field` and `search_field`: Esc reverts, Return or a click away commits once,
    optional select-all on focus, the right-click Cut / Copy / Paste / Select All menu; keep
    `design-system.md` §6's geometry and Dalica's `register` ids. Add an `autocomplete` list under
    a field (type-ahead filtering, ↑/↓ and Enter pick, a click picks, Esc closes, it stays inside
    the window like every popup) from LightCraft's `album_picker.rs` filtering and PhotoCraft's
    `widgets/type_ahead.rs`, for the Tags pane (M6.28) and search (M9). New `Role`s where needed;
    each ported file starts with a header naming its source file and revision, with a `NOTICE`
    row (`architecture.md` §9); `design-system.md` §6 gets both rows.
  - done when: `crates/ui-kit/tests/fields.rs`: Esc restores the old text; Return and a click away
    commit once; the context menu's items act on the selection; the autocomplete filters, picks by
    keys and by click, and closes on Esc; the tests ported with the code pass; every state in the
    gallery fingerprints at both sizes; `widget_ids` stays green.
- ⬜ **M5.R Check-in**
  - after: M5.20
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the lead runs `/milestone-review M5.R` to prepare it: the check-in page (first run on a
    fresh library, the Folder Manager, the mixed-format folder, an unplugged and replugged folder,
    written as steps for the maintainer to try; the questions: the display profile for colour
    management, and the speed thumb's feel carried over from M4.R (`tests/acceptance/m4.md`)), *Where we stand* and
    the `status.md` numbers (first thumbnails at 1 k and 10 k, scan throughput, corpus floors) on a
    `review/M5.R` branch, a pull request open. Only the maintainer merges that pull request and
    closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M5.1 image formats + colour  ‖  M5.2 scenes formats  ‖  M5.3 fs folder handle
         ‖  M5.4 scan rules + volumes  ‖  M5.22 tasks tooling  ‖  M5.23 M4 UI loose ends
         ‖  M5.24 release budgets job  ‖  M5.25 text field + autocomplete
                                                                   (once M4.R is closed)
wave 2   M5.5 engine modules + jobs  ‖  M5.6 TIFF  ‖  M5.7 WebP/GIF/BMP  ‖  M5.8 JPEG XL + PSD
         ‖  M5.9 RAW (TIFF-based)  ‖  M5.10 RAW (other) + HEIF  ‖  M5.11 watcher
wave 3   M5.12 folder modes  ‖  M5.13 offline volumes  ‖  M5.14 dialog host + jobs  ‖  M5.15 corpus
wave 4   M5.16 cache, colour, benchmark  ‖  M5.17 watching end to end
         ‖  M5.18 Folder Manager + first run  ‖  M5.19 activity + offline UI
wave 5   M5.20 accept M5 → M5.R          (M5.21 lead: M7 blocks, any time after M4.19)
```

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

#### M6 tasks

M6 fills in what a user knows about a photo, writes it into the files without harming them, and
ships the first installable preview. Its first wave lists the M1–M5 work each task extends and
starts as soon as the maintainer has closed M5.R. Five tasks lay the ground the others share, so
later tasks never share a file: M6.2 adds every M6 field and op to the library model in one go, M6.4
lays out `meta` with one module per container and per metadata kind behind one field model and a
write API whose container writers start as stubs, M6.5 registers one engine command module per area
and every M6 setting, M6.6 adds M6's view state, the UI commands that open editors and dialogs, and
stub dialogs and panes, and M6.13 builds the release workflow end to end around stub packaging
scripts that the per-OS tasks then fill. Each task updates its `parity.md` rows (new rows where none
fit: `SYS-ABOUT` for the About window, `SYS-RELEASE` for the release workflow, `DATA-XMP-RAW` for
`.xmp` sidecars); first fingerprints of new panes, dialogs, widgets and rotated cells are approved
in bulk on the M6.R review page.

Whole-window baselines belong to no task, as in M5: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m5-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.

Decisions taken while planning (the check-in lists them for reversal): commands that open an editor
or a dialog are UI commands with the menu ids of `menus-and-shortcuts.md` (`photo.caption.edit`,
`section.properties`, `photo.adjustDate`, `app.about`, `help.releaseNotes`), and each calls one
engine command that changes the library (`photo.caption.set`, `folder.info.set`,
`photo.dates.set`), the way `folders.manager` calls `folders.set`; a rotation or flip is drawn by
mapping the cached thumbnail and sketch at paint time, so it never waits for a decode (M7 puts the
whole recipe into the cache keys when tone edits arrive); a hidden folder is stored in its own
sidecar as `"folder": {"hidden": true}`, so it stays hidden when the folder moves; `meta` parses
XML with its own bounded tokenizer that keeps byte spans, which merge-writing needs and which
enforces `security.md` §4.2 directly (the alternative, `quick-xml`, gives positions but not the
limits); the write options of Options → General → Files are settings from M6.5 on, and their
checkboxes arrive with the Options dialog (M10). The two open questions of `xmp-interop.md` §9 are
built as proposed there (removing a tag leaves `lr:hierarchicalSubject` alone; IPTC is always
written) and asked at M6.R. Two maintainer decisions are needed: the application id (reserved
decision 10), which M6.1 brings forward into M5's run so that M5.R asks it, and the app icon's
design (`assets.md` §7), which M6.12 drafts as an original candidate for the maintainer to accept or
replace at M6.R.

- ⬜ **M6.1 The application id in one place**
  - after: —
  - pulled: M5
  - files: `crates/core/src/app_id.rs`, `crates/core/tests/app_id.rs`, `apps/dalica/src/window.rs`
  - scope: reserved decision 10 is the maintainer's. Pulled into M5's run so the question reaches
    M5.R: the agent who claims it runs `cargo xtask tasks block M6.1` with the options (for example
    `io.github.pkkid.Dalica` or a domain the maintainer owns; the recommendation first) and takes
    other work. Once answered: one constant in `dalica-core` (reverse-DNS, checked to be a valid
    D-Bus name and macOS bundle id), the window's Wayland app id and X11 class set from it, the
    decision recorded with its date under *Reserved decisions*. The settings and data paths stay as
    `data-model.md` §4 says (changing them would need a migration, which is not this task).
  - done when: `cargo test -p dalica-core --test app_id` checks the id's syntax rules; the window
    sets it (a test on the viewport builder); *Reserved decisions* item 10 says what was decided and
    when.
- ⬜ **M6.2 `library`: M6's model fields, ops and the tag index**
  - after: M4.2, M4.4, M5.13
  - files: `crates/library/src/model.rs`, `crates/library/src/op.rs`, `crates/library/src/tags.rs`, `crates/library/src/tree.rs`, `crates/library/tests/ops.rs`, `crates/library/tests/m6_fields.rs`, `crates/library/tests/tags.rs`, `crates/library/tests/tree.rs`, `crates/library/README.md`
  - scope: `data-model.md` §2–§3 as pure model: each photo gains a caption, tags, hidden, its edit
    recipe and redo list (operation strings, opaque here; `edit` parses them), a date set by the
    user (the file's own date kept beside it; the effective date is the user's, else the file's),
    and the last-synced hashes of its owned metadata fields (`xmp-interop.md` §5; index-only derived
    data like the sketch: never on an undo stack, no library revision bump); each folder gains a
    description, date, place and hidden flag. One batched op per change with an exact inverse
    (captions, tag additions and removals, a tag renamed everywhere, hidden photos and folders,
    recipes, dates, folder info, synced hashes). `tags.rs`: counts per tag, partial counts over a
    set of photos ("beach (3 of 8)"), prefix lookup for autocomplete, case-insensitive matching that
    keeps the first spelling, caption and tag lengths capped per `security.md` §4.4 (caps in the
    README). `tree.rs`: hidden folders leave Folders for the Hidden Folders group, shown (with their
    sections) only when hidden pictures are shown; folder dates and sorts use effective dates.
    Found at M4.R: `crates/library/README.md` also lists M4.2's sketches and M4.4's `Tree`.
  - done when: the op-then-inverse property test covers every new op; tag tests: counts, partial
    counts, rename onto an existing tag merges, case rules, caps; tree tests: Hidden Folders in both
    modes with the option on and off; a 100 k-photo tag index builds in a time stated in the PR.
- ⬜ **M6.3 `edit`: the recipe and the geometric operations**
  - after: M1.6
  - files: `crates/edit/src/lib.rs`, `crates/edit/src/syntax.rs`, `crates/edit/src/recipe.rs`, `crates/edit/src/geometry.rs`, `crates/edit/tests/recipe.rs`, `crates/edit/tests/geometry.rs`, `crates/edit/tests/fingerprints/geometry*.json`, `crates/edit/README.md`
  - scope: `data-model.md` §3.1 and `editing.md`: the operation syntax parsed and printed for every
    operation listed there (typed parameters where M6 needs them, the others kept as name plus
    parameter text), unknown and `legacy:` operations kept verbatim and never dropping what follows;
    the recipe rules for `rotate`, `flip_h` and `flip_v` (a rotation appended after a rotation
    merges, a net 0 removes it, two identical flips in a row cancel); pipeline order with geometry
    first; a recipe's net orientation as one of the eight dihedral transforms composed after the
    EXIF orientation, and the displayed size it gives; applying it to an RGBA image (the CPU oracle
    M8's Save will use). No rectangles exist yet, so the coordinate-frame question (`data-model.md`
    §3.3) does not arise here.
  - done when: `cargo test -p dalica-edit`: print(parse(s)) == s for every operation of §3.1 and an
    unknown one; proptest random strings never panic; an unknown operation between two rotations
    keeps both; the merge and cancel rules; every recipe of up to four geometric operations on each
    EXIF orientation gives the pixel-exact transform of a `scenes` image (fingerprints) and the
    right displayed size.
- ⬜ **M6.4 `meta`: the field model, container reading, EXIF and IPTC, and the write skeleton**
  - after: M5.1, M5.9
  - files: `crates/meta/src/lib.rs`, `crates/meta/src/fields.rs`, `crates/meta/src/listing.rs`, `crates/meta/src/container/**`, `crates/meta/src/exif/mod.rs`, `crates/meta/src/exif/read.rs`, `crates/meta/src/iptc/mod.rs`, `crates/meta/src/iptc/read.rs`, `crates/meta/src/xmp/mod.rs`, `crates/meta/src/write/**`, `crates/meta/tests/read.rs`, `crates/meta/tests/build/**`, `crates/meta/fuzz/**`, `crates/meta/README.md`, `.github/workflows/fuzz.yml`
  - scope: `xmp-interop.md` §2 and §4 (reading) with the limits of `security.md` §4.2 and the
    tainted-text rules of §4.4: one `OwnedFields` model (caption, tags, rating, place, date taken
    with offset and sub-seconds) and the precedence rules of §2; `container/` walks JPEG segments
    (APP1 EXIF, APP1 XMP and extended-XMP chunks reassembled by GUID and bounded, APP13 Photoshop
    resources, APP2), PNG chunks (`eXIf`, `iTXt`, `tEXt`, `zTXt` inflated under a cap), WebP RIFF
    chunks and TIFF IFD0 tags 700 and 33723, and reports the byte spans of every metadata block;
    `exif/read.rs` reads the owned fields and the Properties summary (camera, lens, exposure,
    aperture, ISO, focal length, flash, orientation, colour space) through `image`'s IFD walker,
    ignoring known camera placeholder descriptions; `iptc/read.rs` with 1:90, UTF-8 and the Latin-1
    fallback; `xmp/mod.rs` a stub that M6.9 fills; `listing.rs` every field as label/value rows for
    Show All Metadata (bounded); `write/` the one entry point `write_owned(bytes, changes, options)`
    with per-container writers answering `Unsupported` for M6.17 and M6.18, and the verification
    every write passes (owned fields re-read equal the request; a digest of every byte outside the
    metadata spans unchanged); `tests/build/` builders of synthetic files for every meta test;
    `crates/meta/fuzz/` targets per `security.md` §7.1 (`exif`, `xmp`, `iptc`, `write_merge`) added
    to the manual fuzz workflow.
  - done when: `cargo test -p dalica-meta --test read`: each field from each source in each
    container, and the precedence rules; IPTC Latin-1 and UTF-8; a placeholder description ignored;
    extended XMP reassembled and an oversized or mismatched one refused; the listing; IFD loops,
    offsets past the end and huge counts are `Err`; truncation at every offset and proptest random
    bytes never panic; `cargo +nightly fuzz build` builds every target (output in the PR).
- ⬜ **M6.5 `engine`: M6's command modules and settings**
  - after: M5.12, M5.16
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/rotate.rs`, `crates/engine/src/cmd/hide.rs`, `crates/engine/src/cmd/caption.rs`, `crates/engine/src/cmd/folder_info.rs`, `crates/engine/src/cmd/tags.rs`, `crates/engine/src/cmd/dates.rs`, `crates/engine/src/cmd/info.rs`, `crates/engine/src/metadata.rs`, `crates/engine/src/session.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/settings_m6.rs`, `crates/engine/README.md`
  - scope: the engine's ground for M6 so M6.14–M6.16, M6.32 and M6.33 never share a file: empty
    `cmd/rotate.rs`, `hide.rs`, `caption.rs`, `folder_info.rs`, `tags.rs`, `dates.rs` and `info.rs`
    joined into the registry once; a stub `metadata.rs` that the session calls after every applied
    op (with the photos and fields it touched, so every surface, undo and redo reach it) and from
    `poll()`; every M6 setting with its default (`menus-and-shortcuts.md` §5 General → Files: write
    captions, tags and places (on), stars as ratings (off), update modification time (off), XMP
    sidecars for RAW (off), date changes (on, `xmp-interop.md` §2); the ten quick tags and "use the
    top two for recent tags" (on), recent tags) behind a typed accessor the command modules use
    without editing `settings.rs`.
  - done when: `crates/engine/tests/settings_m6.rs`: every new setting round-trips through
    `settings.json` with unknown keys kept and a wrong-typed value falls back to its default
    (reported); the metadata hook sees the photos and fields of a star op, its undo and its redo;
    every existing engine test and `panic_hunt` pass unchanged.
- ⬜ **M6.6 `ui`: M6's view state, editor and dialog commands, and stub panes**
  - after: M5.14, M5.18, M5.19
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/notices.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/caption.rs`, `crates/ui/src/commands/info.rs`, `crates/ui/src/commands/help.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/folder_properties.rs`, `crates/ui/src/dialogs/adjust_date.rs`, `crates/ui/src/dialogs/quick_tags.rs`, `crates/ui/src/dialogs/rename_tag.rs`, `crates/ui/src/dialogs/about.rs`, `crates/ui/src/dialogs/release_notes.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/side_pane.rs`, `crates/ui/src/panels/properties.rs`, `crates/ui/src/panels/tags.rs`, `crates/ui/src/panels/tags/**`, `crates/ui/src/control/**`, `crates/ui/tests/view_state_m6.rs`, `docs/control-protocol.md`
  - scope: every M6 field of `UiState` in one go (control-protocol §6.5): the inline caption editor
    (photo, draft), the description editor (section, draft), the Properties pane's Show All state,
    the Tags pane's add-field draft and autocomplete highlight, the six dialogs' drafts, the About
    dialog's tab, list view, sort and name mode; the UI commands that open them
    (`photo.caption.edit` in `commands/caption.rs` as a stub for M6.25; `section.properties`,
    `photo.adjustDate`, Configure Quick Tags and Rename Tag… in `commands/info.rs`; `app.about` and
    `help.releaseNotes` in `commands/help.rs`), each opening its stub dialog on M5.14's host; the
    side pane hosting stub Properties and Tags panes (`panels/tags.rs` laying out `tags/add.rs`,
    `tags/list.rs` and `tags/quick.rs`, so M6.28 and M6.31 never share a file); `notices.rs` (a stub
    turning session events into toasts, for M6.33) polled by the app; `ui.set` accepts the new
    fields.
  - done when: `crates/ui/tests/view_state_m6.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); each opener shows its dialog by
    `dialog:<kind>` and Esc closes it; the Properties and Tags panes open from their toggles and
    View items; the menu items light up; every command's graceful-failure test; `panic_hunt` and
    `widget_ids` stay green.
- ⬜ **M6.7 `xtask version`, build information and portable mode**
  - after: M4.8
  - files: `xtask/src/version.rs`, `xtask/src/docs.rs`, `crates/core/src/build.rs`, `crates/core/tests/build.rs`, `apps/dalica/src/main.rs`, `apps/dalica/src/paths.rs`, `apps/dalica-cli/src/main.rs`, `docs/development.md`
  - scope: `releasing.md` → Cutting a release and What gets built: `cargo xtask version` prints
    `[workspace.package] version`, `cargo xtask version set <semver>` (pre-releases with a hyphen)
    rewrites it and refreshes `Cargo.lock`, refusing anything that is not a semantic version;
    `dalica-core` exposes the version, commit and build date from `DALICA_BUILD_SHA` and
    `DALICA_BUILD_DATE` at compile time (`<version> (dev build)` without them); `dalica --version`
    and `dalica-cli --version` print it; `apps/dalica/src/paths.rs` takes the default library
    directory out of `main.rs` and adds portable mode (a `portable` marker file beside the
    executable keeps settings, the index and caches in a `data` folder next to it; sidecars stay in
    the photo folders), for the Windows portable zip.
  - done when: tests: `version set 0.1.0-rc.1` on a copied manifest rewrites only the version lines
    and is read back; bad versions are refused; the build string with and without the variables; the
    default library directory per OS as before, and next to the executable with the marker; `xtask
    docs` lists `version` as implemented.
- ⬜ **M6.8 `sidecar` + `store`: every M6 field in `.dalica.json` and the index**
  - after: M6.2
  - files: `crates/sidecar/src/lib.rs`, `crates/sidecar/src/print.rs`, `crates/sidecar/src/tests.rs`, `crates/sidecar/src/fields.rs`, `crates/sidecar/README.md`, `crates/store/src/mapping.rs`, `crates/store/src/journal.rs`, `crates/store/src/snapshot.rs`, `crates/store/src/rebuild.rs`, `crates/store/tests/sidecars.rs`, `crates/store/tests/m6_fields.rs`, `docs/data-model.md`
  - scope: `data-model.md` §3: `caption`, `tags`, `hidden`, `edits`, `redo` and `date` per photo and
    `description`, `date`, `place` and `hidden` in the `"folder"` object (the last one written into
    §3 with this milestone's decision), parsed and printed in the documented key order with unknown
    keys and wrong-typed values kept as they are (reported once); every field in the journal and
    snapshot; the synced hashes index-only; a rebuild from folders reads them all back.
  - done when: `data-model.md` §10.1 for every M6 field (scan, change everything, delete the index,
    rebuild, compare), §10.4 (an unknown operation keeps the ones after it through a rewrite) and
    §10.7 with the new fields; the "every model field has a sidecar mapping" test lists each new
    field (synced hashes as index-only); journal replay and snapshots keep them; proptest round
    trips of sidecars with every field never panic.
- ⬜ **M6.9 `meta`: XMP reading and the merge writer**
  - after: M6.4
  - files: `crates/meta/src/xmp/**`, `crates/meta/tests/xmp.rs`, `crates/meta/tests/xmp_merge.rs`
  - scope: `xmp-interop.md` §1–§3: our own XML tokenizer that keeps byte spans (no DTD or entities,
    depth ≤ 64, ≤ 200 000 nodes, `security.md` §4.2), RDF read in attribute and element form
    (`rdf:Alt` with `x-default` and the other languages, `rdf:Bag`, `rdf:Seq`), the owned properties
    and the listing rows for M6.4's `listing.rs`; the merge writer: owned properties into one
    `rdf:Description` of Dalica's own, the same properties removed from other descriptions in either
    form with those descriptions otherwise unchanged, clearing a value removes it,
    `xmp:MetadataDate` set, text escaped and characters invalid in XML 1.0 dropped, the packet
    wrapper and its padding reused so a rewrite can stay in place, a new packet when the file has
    none; extended XMP is never written; the packet size reported for the APP1 limit.
  - done when: `cargo test -p dalica-meta --test xmp --test xmp_merge`: §8 test 1 for the XMP fields
    (Unicode captions, tags with `/`, `,` and non-ASCII, other languages kept), test 2 for XMP
    (unknown namespaces, other descriptions and `xmpMM:History` byte-identical), test 3 (writing
    twice gives identical bytes), test 4 at the XMP level (the rating rule), test 10 (DOCTYPE and
    entities refused, deep nesting and node bombs bounded, truncation and proptest random bytes
    never panic).
- ⬜ **M6.10 `meta`: IPTC writing and EXIF edits in place**
  - after: M6.4
  - files: `crates/meta/src/iptc/write.rs`, `crates/meta/src/exif/edit.rs`, `crates/meta/tests/iptc_write.rs`, `crates/meta/tests/exif_edit.rs`
  - scope: `xmp-interop.md` §2–§3: IPTC in Photoshop resource 0x0404 with 1:90 = UTF-8, 2:25 (one
    dataset per tag) and 2:120 (truncated at a character boundary past the IPTC limit, logged once),
    every other dataset and resource byte for byte, the digest resource 0x0425 recomputed (MD5
    through an audited pure-Rust crate, `cargo xtask deps`); EXIF edited in place or appended, never
    repacked: a changed value of the same size overwritten where it is, a new or larger value
    (DateTimeOriginal and DateTimeDigitized with OffsetTime and SubSecTime; a GPS IFD with
    GPSVersionID 2.3.0.0 and latitude/longitude with their refs as degree/minute/second rationals)
    appended at the end of the TIFF block with only the pointer to it updated; removing a place
    deletes the four latitude/longitude tags and nothing else; maker notes never move.
  - done when: `cargo test -p dalica-meta --test iptc_write --test exif_edit`: §8 test 6 (1:90
    written, digest updated, Latin-1 legacy read); test 1 for dates with offsets and sub-seconds and
    southern/western coordinates; test 2 for a synthetic maker note with absolute offsets that reads
    the same after a write; test 7 (a block that would pass the APP1 limit fails with the field
    named and the input unchanged); truncation and proptest random bytes never panic.
- ⬜ **M6.11 `xtask contributors`**
  - after: M6.7
  - files: `xtask/src/contributors.rs`, `xtask/src/ci.rs`, `xtask/src/docs.rs`, `contributors/contributors.json`, `docs/contributors.md`, `docs/development.md`
  - scope: `contributors.md` → Refreshing: `cargo xtask contributors` regenerates
    `contributors/contributors.json` from `git log` on `main` and, through `gh`, merged pull
    requests and GitHub usernames (never author names or emails), and model counts from
    `Co-Authored-By` trailers; `--check` (offline, run by `cargo xtask ci`) verifies `people.toml`:
    unknown keys, malformed TOML, and an entry added by someone other than its owner without
    `consent_source`. Never edits `people.toml`.
  - done when: tests on a temporary repository with planted commits and a fake `gh`: stats per
    username, models counted from trailers, no email or author name in the output; `--check` refuses
    each planted fault and passes the real file; the first real `contributors.json` is committed
    (its diff in the PR); `xtask docs` lists the command as implemented.
- ⬜ **M6.12 The app icon (a candidate for the maintainer)**
  - after: M6.1
  - files: `assets/app-icon/**`, `packaging/icons.sh`, `ATTRIBUTION.md`, `docs/assets.md`, `apps/dalica/src/icon.rs`, `apps/dalica/src/window.rs`
  - scope: `assets.md` §7: an original icon drawn for Dalica in the Craft family's shape (512-unit
    tile, `rx=112`, full-bleed colour field, at most three colours; macOS renders on Apple's
    824/1024 grid, the others full-tile so the motif reads at 16–48 px), resembling no other
    product's icon; its README (motif, palette, geometry, provenance, file list) and licence;
    `packaging/icons.sh` regenerating the 1024 px PNG, `.icns`, `.ico` and hicolor PNGs 16–512 plus
    the scalable SVG, all committed; the window icon set from the embedded PNGs. Its design is the
    maintainer's: the M6.R check-in shows it at every size for the maintainer to accept or replace.
  - done when: `cargo xtask assets` passes with the new row; a test checks every derived file exists
    at its size and decodes; `icons.sh` run twice gives identical files; the window sets its icon (a
    test on the viewport builder); renders at 16, 32, 48 and 512 px are on the PR.
- ⬜ **M6.13 The release workflow, end to end**
  - after: M6.7
  - files: `.github/workflows/release.yml`, `.github/workflows/packaging-lint.yml`, `packaging/README.md`, `packaging/checksums.sh`, `packaging/linux/package.sh`, `packaging/windows/package.ps1`, `packaging/macos/package.sh`, `docs/releasing.md`
  - scope: `releasing.md` and `security.md` §10: pushes to `release` build every platform and create
    or update a draft release `Dalica v<version>` tagged on the pushed commit, with generated notes
    and `SHA256SUMS.txt` over every artifact; refuses a version whose release is already published;
    a manual run takes an optional `version` (applied with `cargo xtask version set`); a run on any
    other branch, and on pull requests that touch `packaging/`, the workflows or the icons, is a dry
    run (signing jobs refused, the draft job skipped) that still builds, checksums and verifies;
    `DALICA_BUILD_SHA` and `DALICA_BUILD_DATE` set; the fonts pin of `releasing.md`; least privilege
    (signing jobs alone use the `release` environment, every secret optional with a `::warning::`
    when missing, only the final job has `contents: write`); per-OS jobs calling
    `packaging/<os>/package.*`, which start as stubs packing the release binaries into the tarball
    and zips so the pipeline runs whole (M6.21–M6.23 fill them); a checksum step that re-downloads
    the artifacts and verifies them; `packaging-lint.yml` (actionlint, shellcheck, PowerShell parse,
    and xmllint, `desktop-file-validate`, `appstreamcli validate` and the Flatpak agreement check
    once those files exist).
  - done when: the dry run on this task's pull request builds on Linux, Windows and macOS and its
    checksum step passes (run linked in the PR); a planted mismatched checksum fails
    `packaging/checksums.sh` (a script test); actionlint and shellcheck are clean; `releasing.md`
    says what now runs.
- ⬜ **M6.14 `engine`: rotate, flip, hide, captions and folder info**
  - after: M6.3, M6.5, M6.8
  - files: `crates/engine/src/cmd/rotate.rs`, `crates/engine/src/cmd/hide.rs`, `crates/engine/src/cmd/caption.rs`, `crates/engine/src/cmd/folder_info.rs`, `crates/engine/src/cmd/photo.rs`, `crates/engine/tests/rotate.rs`, `crates/engine/tests/hide.rs`, `crates/engine/tests/captions.rs`, `crates/engine/tests/folder_info.rs`
  - scope: `organizing.md` §5.3, §9, §10 and `data-model.md` §3.1, §8: `photo.rotate.cw` /
    `photo.rotate.ccw` (Ctrl+R / Ctrl+Shift+R) and `edit.flipH` / `edit.flipV` (Ctrl+Shift+H /
    Ctrl+Shift+V) as library undo steps editing recipes through `edit`'s rules; `photo.hide` /
    `photo.unhide` and `folder.hide` / `folder.unhide` (Unhide disabled unless hidden);
    `photo.caption.set {ids, caption}` (trimmed, capped; empty clears); `folder.info.set {id,
    description?, date?, place?}` (date or "use the oldest photo's"); `photo.inspect` reports
    caption, tags, hidden, recipe, effective date and displayed size.
  - done when: each command's contract (`architecture.md` §3): behaviour, undo and redo restore
    exactly, `enabled` false cases, hostile params `Err`; the sidecar after the command and after
    undo; four rotations leave no `edits` entry and an unknown operation survives them; hide,
    captions, folder info and rotations survive a rebuild; `panic_hunt` green.
- ⬜ **M6.15 `engine`: tag commands and quick tags**
  - after: M6.5, M6.8
  - files: `crates/engine/src/cmd/tags.rs`, `crates/engine/tests/tags.rs`
  - scope: `organizing.md` §5.2 and `menus-and-shortcuts.md` §1.9, §4: `photo.tags.add {ids, tags}`
    (commas separate tags; trimmed, deduplicated by the case rule) and `photo.tags.remove`, one undo
    step for any number of photos; `photo.tags.toggle {ids, tag}` (removes from all when all have
    it, else adds to all); `tags.list {ids?, prefix?}` (query: counts, partial counts, completion);
    `tags.rename {from, to, confirm}` everywhere (`needs_confirm` over automation; merging onto an
    existing tag); `tags.quick.set {slots, useRecent}` and `tags.quick.apply {n, ids}` with
    Ctrl+Alt+1 … Ctrl+Alt+0, the top two slots following recently used tags when `useRecent` is on.
  - done when: each command's contract; tagging 1 000 photos is one undo step; a rename merges and
    undoes exactly; partial counts over a mixed selection; the keymap has no conflicts for the ten
    keys; `panic_hunt` green.
- ⬜ **M6.16 `engine`: dates and the photo metadata query**
  - after: M6.4, M6.5, M6.8, M6.9
  - files: `crates/engine/src/cmd/dates.rs`, `crates/engine/src/cmd/info.rs`, `crates/engine/src/metadata_cache.rs`, `crates/engine/tests/dates.rs`, `crates/engine/tests/photo_metadata.rs`
  - scope: `organizing.md` §9 (Adjust date and time): `photo.dates.set {ids, shift}` (seconds, every
    photo by the same amount from its effective date, the file time for photos without one) or
    `{ids, set}` (one date and time for all), one undo step, the folder's sections re-sorted;
    `photo.metadata {id, all?}` (query) answered from a cache filled by a worker job (the first call
    answers `pending`; never a file read on the owner thread): the Properties rows of `ui-layout.md`
    §7, and with `all` M6.4's listing; a "changed in another application" note when M6.33 reports
    one.
  - done when: each command's contract; shifting three photos across midnight and a month end; set
    on photos without dates; dates survive a rebuild; `photo.metadata` on a JPEG with EXIF, XMP and
    IPTC answers pending, then every row, and the owner thread made no `fs` call; `panic_hunt`
    green.
- ⬜ **M6.17 `meta`: JPEG write-back**
  - after: M6.9, M6.10
  - files: `crates/meta/src/write/jpeg.rs`, `crates/meta/tests/write_jpeg.rs`
  - scope: `xmp-interop.md` §3–§4 for JPEG: only APP1 (EXIF, XMP) and APP13 are rewritten, new
    segments inserted where readers expect them, every other segment (ICC, MPF, extended XMP,
    comments, the scan data) copied byte for byte; the standard XMP packet fits one APP1 (padding
    dropped first, else the write fails naming the field); the rating rule (5 on star; 0 on unstar
    only when the file says 5); the result passes M6.4's verification.
  - done when: `cargo test -p dalica-meta --test write_jpeg`: §8 tests 1–4, 6 and 7 on JPEG; the
    digest outside the metadata spans is unchanged and the image decodes to identical pixels;
    writing twice gives identical bytes; truncated inputs are `Err` without panicking.
- ⬜ **M6.18 `meta`: PNG, WebP and TIFF write-back**
  - after: M6.9, M6.10
  - files: `crates/meta/src/write/png.rs`, `crates/meta/src/write/webp.rs`, `crates/meta/src/write/tiff.rs`, `crates/meta/tests/write_png_webp_tiff.rs`
  - scope: `xmp-interop.md` §4: PNG XMP in `iTXt` and place and date in `eXIf` (CRCs recomputed,
    other chunks byte for byte); WebP `XMP ` and `EXIF` chunks, a simple-format file promoted to
    `VP8X` with the image chunk unchanged and the RIFF size fixed; TIFF XMP (700) and IPTC (33723)
    by the append-only rule (new data at the end, only pointers changed), place and date into the
    XMP mirrors; each result passes M6.4's verification.
  - done when: `cargo test -p dalica-meta --test write_png_webp_tiff`: §8 tests 1–3, 6, 7 and 10 for
    each container; pixels decode identically through `image`; a promoted WebP still decodes; TIFF
    strips and maker notes keep their offsets.
- ⬜ **M6.19 `meta`: `.xmp` sidecars for RAW**
  - after: M6.9
  - files: `crates/meta/src/xmp_sidecar.rs`, `crates/meta/tests/xmp_sidecar.rs`
  - scope: `xmp-interop.md` §7: the sidecar name for a RAW (`<stem>.xmp`; reading also `<file>.xmp`
    and `.XMP`; a RAW+JPEG pair's stem sidecar belongs to the RAW), reading it (else the XMP
    embedded in a DNG), and the merged bytes of a whole sidecar file (packet wrapper, every other
    namespace and raw-converter settings byte for byte); a sidecar that is not well-formed XMP is
    reported as damaged so the writer keeps it aside.
  - done when: `cargo test -p dalica-meta --test xmp_sidecar`: name resolution for each case;
    develop settings and history kept byte for byte; a damaged sidecar reported; proptest random
    bytes never panic.
- ⬜ **M6.20 `ui`: the About window and the release notes**
  - after: M6.6, M6.7, M6.11
  - files: `crates/ui/build.rs`, `crates/ui/src/credits.rs`, `crates/ui/src/dialogs/about.rs`, `crates/ui/src/dialogs/release_notes.rs`, `crates/ui/src/commands/help.rs`, `crates/ui/tests/about.rs`, `crates/ui/tests/fingerprints/dialog-about*.json`, `crates/ui/tests/fingerprints/dialog-release-notes*.json`
  - scope: `contributors.md` → In Dalica and `menus-and-shortcuts.md` §1.8: `build.rs` compiles
    `contributors.json` and `people.toml` into the binary (nothing read at run time); Help ▸ About
    Dalica (the macOS application menu's About) with the tabs About (icon, version, commit, build
    date, licence) · Contributors (grab bag or table, every sort of `contributors.md`, the name
    toggle Username → Display name → Real name with the `@username` fallback, case-insensitive and
    `@`-blind alphabetical order) · Models (PhotoCraft's `crates/ui-egui/src/credits.rs` About
    window is the pattern, `architecture.md` §9); Help ▸ Release Notes showing `whats-new.md`
    (headings and bullets, HTML comments left out) from `include_str!`; ids per control-protocol §7.
  - done when: `crates/ui/tests/about.rs`: each tab by id; each sort and the name toggle on a
    fixture with missing names; no email reaches the binary (a test on the compiled data); the
    release notes skip comments; dialog fingerprints at both window sizes.
- ⬜ **M6.21 Linux packages**
  - after: M6.1, M6.7, M6.12, M6.13
  - files: `packaging/linux/**`, `apps/dalica/src/preflight.rs`, `apps/dalica/src/main.rs`
  - scope: `releasing.md` → Linux: one staged FHS tree (both binaries, the `.desktop` file named by
    the application id, hicolor icons, AppStream metainfo, `LICENSE-MIT`, `LICENSE-APACHE`,
    `NOTICE`, `ATTRIBUTION.md`, font licences) and from it the AppImage (maintained `appimagetool`,
    static runtime, zsync update information and `.zsync`, the `AppRun` desktop install with its
    opt-out), `.deb` and `.rpm` through one nfpm config with their runtime libraries, `.tar.gz`, and
    a Flatpak bundle from the tarball (freedesktop runtime; Wayland with X11 fallback, GPU, Pictures
    read/write, no network) for x86_64 and aarch64; tools installed by the scripts, not the
    workflow; before opening a window `dalica` checks for the libraries its session needs (without
    `unsafe`: where the dynamic loader looks) and, if one is missing, names the package to install
    and exits with status 1.
  - done when: `packaging/linux/package.sh` builds all six locally (formats whose tool is missing
    said plainly) and the dry run on the PR builds them on both architectures;
    `desktop-file-validate` and `appstreamcli validate` pass; the AppImage runs `dalica --version`
    in CI; a preflight test with a planted missing library prints the package and exits 1.
- ⬜ **M6.22 Windows packages**
  - after: M6.7, M6.12, M6.13
  - files: `packaging/windows/**`, `apps/dalica/build.rs`
  - scope: `releasing.md` → Windows: `package.ps1 -Arch x64` with `+crt-static`; `build.rs` embeds
    the icon and VERSIONINFO only when targeting Windows (a missing resource compiler is an error
    naming the tool, never a panic) and selects the GUI subsystem for release builds through linker
    arguments; an MSI (WiX v5: per-machine, Start Menu shortcut, no licence page, silent install,
    listed under Open with for the readable formats without taking over the default viewer, numeric
    `X.Y.Z`, same-version upgrades); the portable zip with M6.7's marker; `sign.ps1` (`.pfx` or
    Azure Trusted Signing, whichever is configured; skipped with a warning otherwise). If `cargo
    xtask deps` refuses the resource step, block with the question (`C_BUILD_ALLOW` is the
    maintainer's).
  - done when: the dry run on the PR builds the MSI and the zip; in CI `msiexec /qn` installs and
    uninstalls cleanly and the installed executable's version resource matches; the portable zip
    keeps its data beside the executable (a CI step); `build.rs` without a resource compiler fails
    with the tool named.
- ⬜ **M6.23 macOS packages**
  - after: M6.1, M6.7, M6.12, M6.13
  - files: `packaging/macos/**`
  - scope: `releasing.md` → macOS: `aarch64` and `x86_64` builds at `MACOSX_DEPLOYMENT_TARGET=11.0`
    joined with `lipo`; `Dalica.app` with `Info.plist` from a template (the application id as bundle
    id, image formats as Alternate handler); signing inside-out with the hardened runtime and
    notarization with stapling when the secrets exist, ad hoc otherwise; the DMG (volume `Dalica`,
    an `Applications` link, the window layout and a background drawn from the icon, generated
    reproducibly); the CLI zip; `verify.sh`.
  - done when: the dry run on the PR builds the DMG and the CLI zip; `lipo -info` lists both
    architectures; `verify.sh` passes in ad hoc mode; the app from the mounted DMG runs `--version`
    in CI; the DMG layout files are identical across two runs.
- ⬜ **M6.24 `ui`: rotation and hiding in the library**
  - after: M6.6, M6.14
  - files: `crates/ui/src/panels/thumbnails.rs`, `crates/ui/src/panels/light_table/layout.rs`, `crates/ui/src/selection.rs`, `crates/ui/src/panels/bottom.rs`, `crates/ui/src/panels/bottom/**`, `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/folders/**`, `crates/ui/tests/rotate.rs`, `crates/ui/tests/hidden.rs`, `crates/ui/tests/fingerprints/rotated*.json`, `crates/ui/tests/fingerprints/region-tree*.json`
  - scope: `ui-layout.md` §4, §5.3, §6.2–§6.3: `Thumbnails::paint` draws each photo (cells, tray,
    tokens) with its recipe's net orientation by mapping the cached thumbnail and sketch, so a
    rotation shows the same frame with no decode; the layout uses the displayed size (a landscape
    turned 90° takes a portrait cell), cached per revision; hidden photos left out of the layout and
    of selection gestures unless View → Hidden Pictures (then at 40 %, M4.10's rule); the bottom
    bar's rotate buttons and the tray menu's rotations act on the tray; the Hidden Folders group and
    its sections only with Hidden Pictures; a tree row's double-click opens Folder Properties
    (`section.properties`).
  - done when: `crates/ui/tests/rotate.rs`: a landscape photo rotated clockwise from the bottom bar
    shows a portrait cell matching its fingerprint for each EXIF orientation, a flip mirrors it,
    undo restores the cell, and no decode job ran; `crates/ui/tests/hidden.rs`: hiding a photo
    removes its cell and Ctrl+A skips it, Hidden Pictures shows it at 40 %; hiding a folder moves
    its row to Hidden Folders and removes its section until Hidden Pictures; the tree fingerprints
    at both window sizes.
- ⬜ **M6.25 `ui`: captions in the library**
  - after: M5.25, M6.6, M6.14
  - files: `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/src/panels/light_table/caption_edit.rs`, `crates/ui/src/commands/caption.rs`, `crates/ui/tests/captions.rs`, `crates/ui/tests/fingerprints/region-cells*.json`
  - scope: `organizing.md` §5.3 and `ui-layout.md` §5.3: Picture → Edit Caption opens a one-line
    editor under the focused photo's cell (placeholder "Add a caption", the text-input keyboard
    context so shortcuts do not fire); Enter or a click elsewhere commits through
    `photo.caption.set` (one undo step), Esc cancels, scrolling the cell away commits (the editor
    is M5.25's `text_field`); the Caption
    and Tags thumbnail-caption modes show the model's text (tags joined by ", "), middle-elided; ids
    per control-protocol §7.
  - done when: `crates/ui/tests/captions.rs`: type a caption by id and Enter, the sidecar holds it
    and one undo removes it; Esc leaves it unchanged; Ctrl+8 typed into the editor stars nothing;
    each caption mode's text; the cell fingerprints at both window sizes.
- ⬜ **M6.26 `ui`: folder descriptions and Folder Properties**
  - after: M5.25, M6.6, M6.14
  - files: `crates/ui/src/panels/light_table/header.rs`, `crates/ui/src/dialogs/folder_properties.rs`, `crates/ui/tests/folder_properties.rs`, `crates/ui/tests/fingerprints/region-header*.json`, `crates/ui/tests/fingerprints/dialog-folder-properties*.json`
  - scope: `ui-layout.md` §5.2 and `organizing.md` §10: a click on the description line edits it
    inline with M5.25's `text_field` (Enter commits through `folder.info.set`, Esc cancels); a
    double-click on the title opens
    Folder Properties: Name (shown, read-only until renaming arrives in M9), Date with "Use the date
    of the oldest photo", Place, Description, Music for slideshows (disabled until M11), OK applying
    one undo step; the header's date line shows the folder's date and " · place".
  - done when: `crates/ui/tests/folder_properties.rs`: an inline description by id survives a
    restart; the dialog's OK sets date and place in one undo step, Cancel changes nothing; the
    header shows them; the header and dialog fingerprints at both window sizes.
- ⬜ **M6.27 `ui`: the Properties pane**
  - after: M6.6, M6.16
  - files: `crates/ui/src/panels/properties.rs`, `crates/ui/src/panels/properties/**`, `crates/ui/tests/properties_pane.rs`, `crates/ui/tests/fingerprints/pane-properties*.json`
  - scope: `ui-layout.md` §7: header "Properties of IMG_0361.jpg" ("Properties of 12 photos" or the
    folder's name for a selected section), label/value rows (Path, File size, Dimensions, Date
    taken, Date digitized, File modified, Camera, Lens, Exposure, Aperture, ISO, Focal length,
    Flash, Orientation, Color space, GPS, Caption, Tags) from `photo.metadata` and the model, long
    values elided with a tooltip, a loading row while `pending`, the "Changed in another
    application; Dalica's value kept" note when reported; [Show All Metadata] lists every field
    grouped by EXIF, XMP and IPTC in a scrolling list; following the selection; ids per §7.
  - done when: `crates/ui/tests/properties_pane.rs` on a JPEG with EXIF, XMP and IPTC: every row's
    value, Show All Metadata lists the extra fields, the pane follows a new selection, no frame
    waited on a file read (perf counters); the pane fingerprints at both window sizes.
- ⬜ **M6.28 `ui`: the Tags pane**
  - after: M5.25, M6.6, M6.15
  - files: `crates/ui/src/panels/tags.rs`, `crates/ui/src/panels/tags/add.rs`, `crates/ui/src/panels/tags/list.rs`, `crates/ui/src/dialogs/rename_tag.rs`, `crates/ui/src/status_text.rs`, `crates/ui/tests/tags_pane.rs`, `crates/ui/tests/status_tags.rs`, `crates/ui/tests/fingerprints/pane-tags*.json`, `crates/ui/tests/fingerprints/dialog-rename-tag*.json`, `crates/ui/tests/fingerprints/region-status*.json`
  - scope: `organizing.md` §5.2 and `menus-and-shortcuts.md` §4: "Add a tag:" with + (Enter adds,
    commas separate, autocomplete from `tags.list` through M5.25's `text_field` and
    `autocomplete`); "Tags in the selection:" with partial counts
    ("beach (3 of 8)"), hover × removing from all selected photos; the tag context menu (Add to All
    Selected, Search for This Tag disabled until M9, Remove from Selected, Rename Tag… with its
    confirmation dialog); Ctrl+T; the status line's "Tags: lisbon (40), tram (6)" field
    (`ui-layout.md` §6.1); ids per §7.
  - done when: `crates/ui/tests/tags_pane.rs`: add two tags to a selection of 8 by typing, see the
    counts, remove one by ×, undo; autocomplete offers existing tags; Rename Tag merges after
    confirming; `crates/ui/tests/status_tags.rs`: the status text for tagged selections; the pane,
    dialog and status fingerprints at both window sizes.
- ⬜ **M6.29 `ui` + `ui-kit`: Adjust Date and Time**
  - after: M5.25, M6.6, M6.16
  - files: `crates/ui/src/dialogs/adjust_date.rs`, `crates/ui-kit/src/widgets/datetime.rs`, `crates/ui-kit/tests/datetime.rs`, `crates/ui-kit/tests/fingerprints/datetime*.json`, `crates/ui/tests/adjust_date.rs`, `crates/ui/tests/fingerprints/dialog-adjust-date*.json`
  - scope: `organizing.md` §9: a date and a time picker in `ui-kit` (keyboard entry, arrows step the
    focused field, invalid days refused, no locale formats beyond the long date of `ui-layout.md`
    §5.2 yet; built on M5.25's `text_field`, with the date rules, limits and tests ported from
    LightCraft's `crates/ui-egui/src/date_picker.rs`, `architecture.md` §9); Adjust Picture Date ("…
    — %d pictures" for several): the thumbnail, "Current date:" read-only, "New date:", for several
    photos the radios "Shift all dates by the same amount" (default) and "Set all pictures to the
    same date and time"; OK runs `photo.dates.set` (one undo step); ids per §7.
  - done when: `crates/ui-kit/tests/datetime.rs`: entry, stepping and refusal of 31 February;
    `crates/ui/tests/adjust_date.rs`: shift three photos by +1 h 30 min and set one, by id, then
    undo; the widget and dialog fingerprints at both window sizes.
- ⬜ **M6.30 `store`: the metadata write queue**
  - after: M6.8, M6.17, M6.19
  - files: `crates/store/src/metawrite.rs`, `crates/store/src/metawrite/**`, `crates/store/tests/metawrite.rs`, `crates/store/README.md`, `crates/library/src/sync.rs`, `crates/library/tests/sync.rs`, `crates/fs/src/times.rs`, `crates/fs/tests/times.rs`
  - scope: `data-model.md` §5 and `xmp-interop.md` §1, §5: a queue of files to write, batched per
    file and debounced, run on a worker: re-read the file, decide per field with the §5 rules (a
    pure function in `library/src/sync.rs` over the file's value, the last-synced hash and the
    sidecar's value: write, import, keep, or report both changed), merge-write through `meta`,
    replace atomically through `fs` (never a file that changed since it was read: re-queued),
    verify, restore the modification time unless the option says otherwise (`fs` gains setting a
    file's times, with a fault point), update the synced hashes; a RAW gets its `.xmp` sidecar only
    with that option (a damaged one kept aside through `fs`); formats that cannot be written, and
    read-only files, keep their values in the sidecar with the reason; files under an offline root
    wait (M5.13) and are written on return; each field follows its option; results (written,
    imported, conflict, failed with reason, skipped) returned for the engine.
  - done when: `cargo test -p dalica-store --test metawrite` on temp folders: `xmp-interop.md` §8
    test 8 (first-scan import, sidecar wins, change made elsewhere imported, both changed reported,
    the rebuild fallback) and test 9 (a fault at every step of a metadata write leaves the old or
    the new file, never a torn one, and the time restored); an offline root writes on return; a
    read-only file fails with its reason and the sidecar intact; a file changed between read and
    write is re-read; `cargo test -p dalica-fs --test times` with fault injection.
- ⬜ **M6.31 `ui`: quick tags**
  - after: M5.25, M6.28
  - files: `crates/ui/src/panels/tags/quick.rs`, `crates/ui/src/dialogs/quick_tags.rs`, `crates/ui/tests/quick_tags.rs`, `crates/ui/tests/fingerprints/pane-tags*.json`, `crates/ui/tests/fingerprints/dialog-quick-tags*.json`
  - scope: `organizing.md` §5.2: the grid of ten buttons in rows of 2/3/2/3 (unassigned ones show
    "+", a click toggles that tag on the selection through `photo.tags.toggle`), the gear opening
    Configure Quick Tags (the recent-tags checkbox, ten M5.25 `text_field`s, [Fill Empty with Common
    Tags], OK / Cancel through `tags.quick.set`); Ctrl+Alt+1 … Ctrl+Alt+0 act on the tray; ids per
    §7.
  - done when: `crates/ui/tests/quick_tags.rs`: configure three slots, apply slot 2 by click and by
    Ctrl+Alt+2, toggle it off again; Fill Empty uses the most common tags; Cancel changes nothing;
    the pane and dialog fingerprints at both window sizes.
- ⬜ **M6.32 `scan` + `engine`: importing file metadata on scan**
  - after: M5.17, M6.8, M6.9, M6.19, M6.30
  - files: `crates/scan/src/meta.rs`, `crates/scan/tests/meta.rs`, `crates/engine/src/scanning.rs`, `crates/engine/tests/meta_import.rs`
  - scope: `xmp-interop.md` §5 and §7: when a scan finds a new or changed file (size or mtime), its
    owned fields are read through `meta` on the scan worker (an `.xmp` sidecar for a RAW whatever
    the option, else a DNG's embedded XMP); a photo with no sidecar entry imports every field it has
    (rating 5 as a star) without an undo step; a rescan applies the `sync.rs` decision: a value
    changed elsewhere becomes one undoable import op, both changed is recorded for Properties;
    synced hashes are set from what was read.
  - done when: `crates/engine/tests/meta_import.rs` on temp folders: a JPEG tagged by another tool
    shows its caption, tags, date and star after the first scan; editing the file's caption outside
    while Dalica runs imports it as one undoable step; editing both reports the conflict and keeps
    Dalica's value; a RAW with an `.xmp` sidecar imports from it; the owner thread made no `fs`
    call.
- ⬜ **M6.33 `engine` + `ui`: metadata write-back end to end**
  - after: M6.14, M6.15, M6.16, M6.18, M6.30
  - files: `crates/engine/src/metadata.rs`, `crates/engine/tests/write_back.rs`, `crates/ui/src/notices.rs`, `crates/ui/tests/notices.rs`, `docs/xmp-interop.md`, `docs/data-model.md`
  - scope: `data-model.md` §5: the session's metadata hook queues a write for every photo whose
    caption, tags, place, date or (with its option) star an op changed, including undo and redo, and
    only when the field's option is on; writes run as jobs ("Writing metadata", progress, Cancel)
    through M6.30's queue, imports come back as ops, conflicts reach `photo.metadata`;
    `pendingMetadataWrites` in `library.inspect`; a failed write becomes a session event that
    `notices.rs` shows as a `caution` toast naming the file and field; the status lines of
    `xmp-interop.md` and `data-model.md` §5 updated.
  - done when: `crates/engine/tests/write_back.rs` on temp JPEG, PNG, WebP and TIFF files: caption,
    tags and a date change reach each file and survive another application's read (`meta`); undo
    writes the old value back; with an option off nothing is written; the star rating rule; a
    read-only file gives one toast (`crates/ui/tests/notices.rs`) and the sidecar keeps the value;
    the owner thread made no `fs` call.
- ⬜ **M6.34 `meta`: the metadata corpus**
  - after: M5.15, M6.17, M6.18, M6.19
  - owner: lead
  - files: `xtask/src/corpus_pins.rs`, `xtask/corpus/metadata/**`, `crates/meta/tests/corpus.rs`, `crates/meta/tests/corpus_floors.json`
  - scope: `development.md` §11 `corpus/metadata/`: the lead adds a `metadata` set to our CC0
    `pkkid/dalica-corpus` repository (as it filled M5's sets, M3.R answer): `scenes` photos in JPEG,
    PNG, WebP and TIFF, and RAW `.xmp` sidecars, after other tools wrote captions, keywords,
    ratings, GPS, dates and face regions into them (each file's tool and version in the manifest),
    pins it, and adds the `corpus`-feature test: every file reads, and after a Dalica write of each
    owned field every byte outside the owned properties is kept (the non-metadata digest equal,
    foreign XMP, IPTC and EXIF fields unchanged), with floors at the measured counts.
  - done when: `cargo xtask corpus --all` fetches and verifies the set; `cargo xtask test-corpus -p
    dalica-meta` passes with its floors (numbers in the PR); no corpus file is tracked by git.
- ⬜ **M6.35 Release 0.1: notes, version and README**
  - after: M6.20, M6.21, M6.22, M6.23, M6.24, M6.25, M6.26, M6.27, M6.29, M6.31, M6.32, M6.33, M6.34
  - files: `docs/whats-new.md`, `README.md`, `docs/releasing.md`, `docs/security.md`
  - scope: the user-facing 0.1 section of `whats-new.md` by its conventions (what a user can do now,
    honest about what is missing, every data-safety change: what is written into photo files and
    when), a status banner at the top of `README.md` (an organizer preview; editing comes later),
    the version set to `0.1.0` with `cargo xtask version set` (root manifest and lock file),
    `<app-id>` in `releasing.md` replaced by the decided id, and the statuses of `security.md` §1.1,
    §4, §7 and §10 brought up to date with M6.
  - done when: `cargo xtask version` prints `0.1.0`; Help ▸ Release Notes shows the new section (a
    test through M6.20's dialog); `cargo xtask names` and `cargo xtask docs` pass.
- ⬜ **M6.36 `accept M6`**
  - after: M6.35
  - files: `tests/acceptance/m6.rs`, `tests/acceptance/m6_*.rs`, `tests/acceptance/m6.md`, `tests/acceptance/fingerprints/m6-*.json`
  - scope: every *Done when* of M6's three areas: the command contract of every new command
    (including `panic_hunt`); rotated thumbnails matching their fingerprints in the real `dalica
    --headless`; hide, captions, dates, tags and folder info surviving a rebuild; `xmp-interop.md`
    §8 tests 1–4 and 6–10 (the `meta` and `store` suites); the corpus check that other applications'
    bytes are kept (run when `corpus/` is present and verified, said plainly when it is not; the CI
    corpus job is the strict run); the latest release dry run on `main` green on all three OSes with
    its checksums verified (read through `gh`, said plainly when offline); the manual checks in
    `m6.md` (rotate, hide and caption photos in the app; tag a photo and see the tag in another
    photo application; install a package and browse your own photos).
  - done when: `cargo xtask accept M6` passes everything in M6's *Done when*.
- ⬜ **M6.37 Lead: task blocks for M8**
  - after: M5.21
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M8 task block from M8's section and the spec, with waves and the `M8.R`
    review (owner: maintainer); a block that can port Craft-app code names the source files in its
    scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M6.R Release 0.1 gate**
  - after: M6.36
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the lead runs `/milestone-review M6.R` to prepare it: the check-in page (rotate, hide,
    caption and tag photos, Properties, Adjust Date and Time, a tag seen in another application, the
    About window, written as steps for the maintainer to try), the release checklist of
    `releasing.md` up to the draft (version 0.1.0 on `main`, the dry run's artifacts and
    `SHA256SUMS.txt`, signature state per artifact), and the sign-off questions: accept or replace
    the app icon candidate, the two open questions of `xmp-interop.md` §9, and cutting 0.1 (the push
    to `release` and publishing the draft are the maintainer's); *Where we stand* and the
    `status.md` numbers (metadata write times, corpus floors, package sizes) on a `review/M6.R`
    branch, a pull request open. Only the maintainer merges that pull request and closes this issue;
    nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 0   M6.1 application id    (pulled into M5's run so M5.R asks reserved decision 10)
wave 1   M6.2 library fields  ‖  M6.3 edit recipe  ‖  M6.4 meta reading + skeleton
         ‖  M6.5 engine modules + settings  ‖  M6.6 ui state + stubs  ‖  M6.7 version + portable
         (once M5.R is closed)
wave 2   M6.8 sidecar + index  ‖  M6.9 XMP  ‖  M6.10 IPTC + EXIF writes  ‖  M6.11 contributors
         ‖  M6.12 app icon  ‖  M6.13 release workflow
wave 3   M6.14 rotate/hide/caption commands  ‖  M6.15 tag commands  ‖  M6.16 dates + metadata query
         ‖  M6.17 JPEG write  ‖  M6.18 PNG/WebP/TIFF write  ‖  M6.19 .xmp sidecars  ‖  M6.20 About
         ‖  M6.21 Linux  ‖  M6.22 Windows  ‖  M6.23 macOS
wave 4   M6.24 rotation + hiding  ‖  M6.25 captions  ‖  M6.26 Folder Properties  ‖  M6.27 Properties
         ‖  M6.28 Tags pane  ‖  M6.29 Adjust Date  ‖  M6.30 write queue
         ‖  M6.34 metadata corpus (lead)
wave 5   M6.31 quick tags  ‖  M6.32 import on scan  ‖  M6.33 write-back end to end
wave 6   M6.35 notes + 0.1.0 → M6.36 accept M6 → M6.R
         (M6.37 lead: M8 blocks, any time after M5.21)
```

### M7 — Edit view and fixes

One milestone in 3 feature areas (*Edit view shell*, *Basic fixes*, *Tuning and retouch*); one `cargo xtask accept M7` covers every *Done when* below.

#### Edit view shell

- **Goal:** open a photo and move around in it (`editing.md` §1–3, §5.1–5.2).
- **Prerequisite:** decide the coordinate frame (`data-model.md` §3.3).
- **In:** layout (top bar, filmstrip, edit panel with tab placeholders, photo area, caption bar,
  Histogram & Camera Info); entering and leaving (double-click, Enter, Ctrl+3, Esc, Ctrl+1);
  previous/next; zoom and pan maths, Fit / 1:1, background refinement; the per-photo history
  commands with labelled Undo/Redo buttons; star, rotate and flip in the edit view as history steps.
- **Reuse** (`architecture.md` §9): craft-ui's `tabs` (fit maths and an accessible tab) under
  `design-system.md`'s `tabs` widget; PhotoCraft's `rgb_histogram.rs` and `tone.rs`
  `draw_histogram` for the histogram; PhotoCraft's `wheel_nav.rs` (`classify`) and `flick_pan.rs`
  for wheel, trackpad and pinch input (the zoom curve stays Dalica's); LightCraft's filmstrip in
  `panels/detail.rs` as the pattern.
- **Out:** tools and effects (M7+), compare modes (M11), video (M15).
- **Done when:** `cargo xtask accept M7`: zoom maths tests (ends and midpoint = 100 %); navigation
  tests; a photo's history survives a restart; opening a 24 MP JPEG renders its screen preview in
  ≤ 150 ms (recorded).

#### Basic fixes

- **Goal:** the Basic Fixes tab, and edits shown everywhere (`editing.md` §6).
- **In:** the `edit` renderer with pipeline order and per-operation versions; thumbnails show
  edits (recipe in cache keys); Crop (ratios, custom ratios, Manual, rotate, preview; suggestions
  from thirds/saliency); Straighten; Auto Fix, Auto Contrast, Auto Color; the Fill Light slider.
- **Reuse** (`architecture.md` §9): the guide geometry of LightCraft's `panels/crop_overlay.rs`
  and PhotoCraft's `crop_overlay.rs` (`guides`, unit-square polylines with tests).
- **Out:** face-based crop suggestions (M13), Retouch (M7).
- **Done when:** `cargo xtask accept M7`: fingerprints per operation; resolution independence (two
  sizes agree after downscale); thumbnail render = edit-view render; unknown operations survive.

#### Tuning and retouch

- **In:** the Tuning tab as one live-updated operation (one undo step per tab session), Auto
  lighting and Auto color, the Neutral Color Picker; the Retouch tool with in-tool patch undo.
- **Done when:** `cargo xtask accept M7`: a slider drag updates the screen preview in ≤ 16 ms
  draft (recorded); a whole drag is one history step; fingerprints per control.

#### M7 tasks

M7 makes a photo editable: the edit view, the Basic Fixes and Tuning tabs, Retouch, and edits shown
on every thumbnail. Its first wave lists the M6 work each task extends and starts once the
maintainer has closed M6.R, the 0.1 release gate. Five tasks lay the ground the others share, so
later tasks never share a file: M7.2 gives `edit` the renderer with one module per operation family
(typed parameters, version and history label each) and stubs for the M7 operations, M7.3 adds the
photo's own history to the engine with stub command modules and M7's settings, M7.4 adds the edit
view's state, its frame and stub panels with one interface for the tools, and M7.5 and M7.6 give
`ui-kit` the edit panel's widgets, the canvas input helpers and the icons. Each task updates its
`parity.md` rows (`EDIT-VIEW`, `EDIT-ZOOM`, `EDIT-TABS`, `EDIT-UNDO`, `EDIT-HIST`, `EDIT-CROP`,
`EDIT-STRAIGHTEN`, `EDIT-AUTOFIX`, `EDIT-RETOUCH`, `EDIT-FILL`, `EDIT-TUNING`, the edit-view half of
`EDIT-FLIP`, and Enter in `LIB-SEL-KEYS`); first fingerprints of operations, panels, widgets and
icons are approved in bulk on the M7.R review page, and with them the look of Auto Fix and Tuning.

Whole-window baselines belong to no task, as in M5 and M6: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m6-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.

Decisions taken while planning (the check-in lists them for reversal): the coordinate frame
(`data-model.md` §3.3) is reserved decision 11, so M7.1 is pulled into M6's run and M6.R asks it,
recommending the frame after EXIF orientation and before Dalica's rotations; the engine's edit-view
commands take the photo explicitly and walk its history (`photo.history.apply`, `.undo`, `.redo`,
`.state`, `History::Photo`), and in the edit view the UI routes Edit ▸ Undo / Redo, the rotations,
the flips and Auto Fix there, the way `architecture.md` §4 resolves `edit.undo`; the library-level
`edit.apply`, Batch Edit and Undo All Edits stay with M11 as its row says; starring in the edit view
stays a library step (the photo's history holds only its recipe); the Basic Fixes Fill Light slider
is Tuning's `fill` (`editing.md` §6.5), so Dalica writes `tuning(…)` and reads a `fill_light(a)` as
Tuning with only `fill`; Auto Fix, Auto Contrast and Auto Color store no parameters and analyse a
fixed-size proxy of the image at their place in the pipeline, so a thumbnail and the full size agree
and cropping first changes the outcome; a Tuning tab session appends one `tuning` entry, or updates
the last entry when it is one, and is one history step; the Basic Fixes grid draws `editing.md` §6's
icon-above-label tools (the `tool_button` row of `design-system.md` §6 changes to match); the
canvas input helpers (wheel, pinch, flick) are ported into `ui-kit` with the tabs and the histogram,
so only three tasks edit `NOTICE`, each in its own wave; Play, the compare toggle's A│B and A│A, the
caption bar's slideshow toggle and its trash button are drawn disabled until M11 and M9, and the
effect tabs show an empty state until M12.

- ⬜ **M7.1 The coordinate frame for stored rectangles and points**
  - after: M6.3
  - pulled: M6
  - files: `crates/edit/src/frame.rs`, `crates/edit/src/recipe.rs`, `crates/edit/tests/frame.rs`, `docs/data-model.md`
  - scope: reserved decision 11 (`data-model.md` §3.3) is the maintainer's. Pulled into M6's run so
    the question reaches M6.R: the agent who claims it runs `cargo xtask tasks block M7.1` with the
    options, the recommendation first: (b) the photo after EXIF orientation and before Dalica's own
    rotations and flips, converted only for display (a rotation stays a one-entry recipe change;
    crops, retouch spots and face rectangles are never rewritten; faces found on the upright decode
    stay valid); (a) the photo as displayed after Dalica's rotations, rewriting every stored
    rectangle and point on each rotation or flip; then takes other work. Once answered:
    `edit::frame` maps normalised points and rectangles between the stored and the displayed frame
    for each of the eight dihedral transforms, both ways; under (a) the rules of `recipe.rs` rewrite
    the coordinates of `crop`, `retouch` and the focal effects when a rotation or flip is added or
    undone; §3.3 replaced by the decision; *Reserved decisions* item 11 records it with its date.
  - done when: `cargo test -p dalica-edit --test frame`: display-then-stored is the identity for
    every transform on random points and rectangles (proptest); four rotations and two flips
    compose to the identity; a rectangle on a rotated `scenes` photo covers the same pixels in both
    frames; under (a) an unknown operation between a crop and a rotation keeps its text;
    `data-model.md` §3.3 and item 11 say what was decided and when.
- ⬜ **M7.2 `edit`: the renderer, one module per operation, per-operation versions**
  - after: M6.3, M6.R
  - files: `crates/edit/src/lib.rs`, `crates/edit/src/syntax.rs`, `crates/edit/src/render.rs`, `crates/edit/src/version.rs`, `crates/edit/src/ops/**`, `crates/edit/src/analysis.rs`, `crates/edit/src/suggest.rs`, `crates/edit/tests/render.rs`, `crates/edit/tests/common/**`, `crates/edit/tests/fingerprints/render*.json`, `crates/edit/README.md`
  - scope: `architecture.md` §6.3–§6.4 and `data-model.md` §3.1: `edit::render(source, &recipe,
    target)` and `render_region` (a window of the output at a scale, for zoomed views) over decoded
    sRGB images, the CPU oracle for thumbnails, the edit view, Save and Export; the pipeline order
    (orientation, crop and straighten first, then every other operation in the order applied;
    unknown and `legacy:` operations skipped and kept); `ops/` with one module per operation family,
    each owning its typed parameters (parse, print, range, default; `syntax.rs` dispatches to them),
    its `VERSION`, its history label ("Crop", "Tuning", "Rotate") and, for geometry, its output size
    and the mapping of normalised points through it; M6.3's `geometry.rs` serving rotate and flip through the same interface; stubs
    that render nothing yet in `ops/crop.rs`, `straighten.rs`, `tone.rs`, `auto.rs` and
    `retouch.rs`, and empty `analysis.rs` (with the fixed-size proxy that analysing operations
    share) and `suggest.rs`, so M7.7, M7.8 and M7.12–M7.14 never share a file; `version.rs`: a
    recipe's render key (the printed recipe and the versions of the operations it uses); a working
    precision the tone operations can use (16-bit or float buffers in row bands through `rayon`;
    the choice and its timing in the commit body); `tests/common/` with the two-size and fingerprint
    helpers every operation's tests use.
  - done when: `cargo test -p dalica-edit --test render`: M6.3's geometric recipes render exactly as
    before; an unknown operation between two known ones is skipped and prints back unchanged;
    geometry applies first wherever it stands in the recipe; `render_region` equals that window of
    a full render; the render key changes when one operation's version does, and only for recipes
    that use it; proptest random recipes and hostile parameters (NaN, huge, zero sizes) never panic
    and stay within the allocation limit.
- ⬜ **M7.3 `engine`: the photo's own history, M7's command modules and settings**
  - after: M6.5, M6.8, M6.14, M6.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/edit.rs`, `crates/engine/src/cmd/history.rs`, `crates/engine/src/cmd/crop.rs`, `crates/engine/src/cmd/tuning.rs`, `crates/engine/src/history.rs`, `crates/engine/src/preview.rs`, `crates/engine/src/session.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/photo_history.rs`, `crates/engine/tests/settings_m7.rs`, `crates/engine/README.md`, `docs/architecture.md`
  - scope: `architecture.md` §4 and `editing.md` §5.1, over the recipe and redo list M6.2 and M6.8
    store: `photo.history.apply {id, op}` (`History::Photo`; the operation parsed and checked by
    `edit`, M6.3's merge rules, the redo list cleared), `photo.history.undo {id}` and
    `photo.history.redo {id}` (the last entry moves to the redo list and back; a step that merged
    into the entry before it, such as a second rotation, undoes to that entry's earlier value),
    `photo.history.state {id}` (query: the recipe and the "Undo Crop" / "Redo Tuning" labels from
    each operation's history label); a photo step never touches the library undo stack and a
    library step never a photo's history; a session hook on recipe changes (for M7.9); empty
    `cmd/crop.rs` and `cmd/tuning.rs` and a stub `preview.rs` called from `poll()`, so M7.11, M7.17
    and M7.18 never share a file; M7's settings with defaults: custom crop ratios, the last ratio,
    US or metric photo sizes (US when the system region is the US), "Single-click to leave the edit
    view" (off). `architecture.md` §4 gets the merged-step rule.
  - done when: `crates/engine/tests/photo_history.rs`: apply three operations, undo two, redo one,
    a new step drops the redo; the labels at each point; two rotations undo one at a time; an
    unknown operation in the middle survives undo and redo; the history and its labels survive a
    restart and an index rebuild; the library undo stack is unchanged by photo steps; every
    command's contract and graceful failures; `crates/engine/tests/settings_m7.rs` round-trips the
    new settings with unknown keys kept; `panic_hunt` green.
- ⬜ **M7.4 `ui`: the edit view's state, frame and stub panels**
  - after: M5.14, M6.6, M6.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/edit_view.rs`, `crates/ui/src/commands/zoom.rs`, `crates/ui/src/commands/tools.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/layout.rs`, `crates/ui/src/panels/edit.rs`, `crates/ui/src/panels/edit/**`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/custom_ratio.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state_m7.rs`, `crates/ui/tests/fingerprints/edit-frame*.json`, `docs/control-protocol.md`, `docs/ui-layout.md`
  - scope: `editing.md` §1–§2 and control-protocol §6.5–§6.7: `View::Edit` and the `edit` object of
    `ui.state` (photo, tab, tool, compare `one`, zoom, panel visible) with each tool's draft (crop
    ratio and rectangle, straighten angle, retouch patches with their undo position and brush size,
    the Tuning session), accepted by `ui.set`; the edit view's layout in `panels/layout.rs` (top bar
    40, edit panel 280 and collapsible, photo area, caption bar, the optional side pane, status line
    and bottom area) with the regions `editTopBar`, `editPanel`, `canvas`, `filmstrip` and
    `captionBar` registered and in `ui.inspect`; `panels/edit.rs` laying out stub modules under
    `panels/edit/` (top bar, filmstrip, caption bar, canvas, zoom, navigator, panel, basic,
    history, histogram, fill light, crop, straighten, tuning, retouch), stub `commands/edit_view.rs`
    and `commands/zoom.rs`, `commands/tools.rs` dispatching open, apply, cancel, reset, keys and
    `edit.tool.action {name, params}` to one `Tool` interface each tool module implements, and a
    stub `dialogs/custom_ratio.rs` on the dialog host, so M7.10 and M7.15–M7.22 never share a file;
    `view.edit` (Ctrl+3; Enter in the library; "Select a photo to edit." without a selection),
    `view.library` (Esc and Ctrl+1 in the edit view) and `edit.panelToggle` (Ctrl+9, View ▸ Show
    Edit Controls) live; `ui-layout.md` §10 and control-protocol §6.5–§6.7 updated.
  - done when: `crates/ui/tests/view_state_m7.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); Ctrl+3 on a selected photo shows every
    edit-view region at the rects of `editing.md` §2 at 1400 × 900 and 900 × 600 and Esc returns;
    Ctrl+9 hides and shows the panel; Ctrl+3 with nothing selected gives the toast; the frame's
    fingerprints at both sizes; `widget_ids` and `panic_hunt` green.
- ⬜ **M7.5 `ui-kit`: tabs, tool buttons, the histogram, the Tuning slider and canvas input**
  - after: M5.25, M6.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/tabs.rs`, `crates/ui-kit/src/widgets/tool_button.rs`, `crates/ui-kit/src/widgets/histogram.rs`, `crates/ui-kit/src/widgets/slider.rs`, `crates/ui-kit/src/input.rs`, `crates/ui-kit/src/input/**`, `crates/ui-kit/src/registry.rs`, `crates/ui-kit/src/theme.rs`, `crates/ui-kit/tests/edit_widgets.rs`, `crates/ui-kit/tests/input.rs`, `crates/ui-kit/tests/fingerprints/edit-widgets*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `NOTICE`, `docs/design-system.md`
  - scope: `design-system.md` §6 and `editing.md` §2, §5–§7: `tabs` (icon tabs 40 × 32 with
    tooltips, the active one joined to the panel; fitting and the accessible tab ported from
    craft-ui `crates/ui/src/tabs/mod.rs` at 5c874d51); `tool_button` as the Basic Fixes grid draws
    it (icon 44 × 30 above a label, three to a row; the §6 row updated); `histogram` (R, G and B as
    translucent areas, additive where they overlap, normalised to the mean so spikes clip, the bins
    given by the caller; the mesh painting ported from PhotoCraft
    `crates/ui-egui/src/rgb_histogram.rs`); the Tuning variant of `slider` (no value field, the
    value as a hover tooltip, + / − nudge 2 %, double-click resets); `input/`: wheel, trackpad and
    pinch classification ported from PhotoCraft `crates/ui-egui/src/wheel_nav.rs` (`classify`, the
    Windows pinch fold) and the release glide from `flick_pan.rs`, without PhotoCraft's
    preferences; new `Role`s where needed. Each ported file starts with a header naming its source
    file and revision, with a `NOTICE` row (`architecture.md` §9).
  - done when: `crates/ui-kit/tests/edit_widgets.rs`: tab fitting at narrow widths keeps the active
    tab; tabs and tool buttons in every state; the histogram of a known bin set; the Tuning
    slider's tooltip, nudge and reset; `crates/ui-kit/tests/input.rs`: notched wheel, trackpad
    scroll, pinch and the folded Windows pinch classify as in the source (its tests ported), a
    flick glides and stops; gallery fingerprints at both sizes; `widget_ids` green.
- ⬜ **M7.6 `ui-kit`: the edit view's icons**
  - after: M5.18, M6.R
  - files: `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/edit.rs`, `crates/ui-kit/tests/icons_edit.rs`, `crates/ui-kit/tests/fingerprints/icons-edit*.json`
  - scope: `design-system.md` §5 and `editing.md` §2, §5–§7: original icons drawn in code on the
    20-unit grid (our own drawings, never traced): the five tabs (wrench, half sun, brush, brush
    with landscape, brush with sky), Crop, Straighten, Retouch, Auto Fix, Auto Contrast, Auto
    Color, Fill Light, the magic wand, the eyedropper, Back to Library, Play, the filmstrip arrows,
    the caption toggle and trash, as named `Icon` variants.
  - done when: `crates/ui-kit/tests/icons_edit.rs`: every new icon stays inside its grid at 12, 16,
    20 and 28 px in rest, hover and disabled colours; the icon sheet's fingerprint at both scales;
    `cargo xtask assets` and `cargo xtask names` pass.
- ⬜ **M7.7 `edit`: Crop and Straighten**
  - after: M7.1, M7.2
  - files: `crates/edit/src/ops/crop.rs`, `crates/edit/src/ops/straighten.rs`, `crates/edit/src/crop_geom.rs`, `crates/edit/src/guides.rs`, `crates/edit/tests/crop.rs`, `crates/edit/tests/straighten.rs`, `crates/edit/tests/fingerprints/crop*.json`, `crates/edit/tests/fingerprints/straighten*.json`, `NOTICE`
  - scope: `editing.md` §6.1–§6.2 and `data-model.md` §3.1, in M7.1's frame: `crop(x0,y0,x1,y1)`
    (normalised, ordered, at least one pixel at any size; only the last crop counts) and
    `straighten(deg)` (−10…+10, about the centre, scaled up so no empty corner shows) rendered and
    mapped through `ops/`; `crop_geom.rs`, the maths the tools share: a ratio's largest centred
    rectangle, Rotate (portrait ↔ landscape about the centre, kept inside), dragging an edge, a
    corner or the inside with the ratio locked unless Manual, a minimum size, the crop kept inside
    the straightened photo, ported from LightCraft `crates/geom/src/crop.rs`
    (`max_inscribed_scale`, `crop_fit_angle`, `constrained`) and `crates/geom/src/crop_drag.rs`
    (`drag_crop`) at 76b53778; `guides.rs`: the thirds lines and the straighten grid as unit-square
    polylines, from LightCraft `crates/ui-egui/src/panels/crop_overlay.rs` (`paths`) and PhotoCraft
    `crates/ui-egui/src/crop_overlay.rs` (`guides`); headers and `NOTICE` rows (`architecture.md`
    §9).
  - done when: `cargo test -p dalica-edit --test crop --test straighten`: two output sizes agree
    after downscale; a crop and a straighten render the same in either recipe order; every drag
    keeps its ratio and stays inside the photo (proptest; the ported tests pass); ±10° leaves no
    empty corner and 0° is the identity; fingerprints per operation at three values; reversed,
    outside or NaN rectangles are `Err` or clamped, never a panic.
- ⬜ **M7.8 `edit`: the Tuning operation**
  - after: M7.2
  - files: `crates/edit/src/ops/tone.rs`, `crates/edit/tests/tone.rs`, `crates/edit/tests/fingerprints/tone*.json`, `docs/data-model.md`
  - scope: `editing.md` §6.5 and §7: `tuning(fill,highlights,shadows,temp,neutral)` with typed
    parameters, ranges and defaults, rendered as one tone and colour stage of our own curves: Fill
    Light lifts shadows and the foreground, Highlights brightens bright areas only, Shadows darkens
    dark areas only, Color Temperature moves cooler or warmer, Neutral makes the picked `#rrggbb`
    grey (`none` off); `fill_light(a)` read from a sidecar renders as Tuning with only `fill` and is
    kept as written, and Dalica writes `tuning`; the §3.1 rows say so.
  - done when: `cargo test -p dalica-edit --test tone`: defaults are the identity; Fill Light never
    darkens a pixel and lifts dark ones most; Highlights leaves pixels below the middle alone and
    Shadows those above it; temperature moves the red/blue balance monotonically; the neutral
    colour of a cast `scenes` photo maps to grey; a fingerprint per control at three values; two
    sizes agree after downscale; the time of one Tuning render of a 2560-px preview printed;
    print(parse) round-trips and hostile values clamp.
- ⬜ **M7.9 `engine` + `thumbs`: edits shown on every thumbnail**
  - after: M5.16, M6.24, M6.25, M7.2, M7.3
  - files: `crates/engine/src/thumbs.rs`, `crates/engine/tests/edited_thumbs.rs`, `crates/thumbs/src/ladder.rs`, `crates/thumbs/src/sketch.rs`, `crates/thumbs/src/lib.rs`, `crates/thumbs/tests/recipe.rs`, `crates/thumbs/README.md`, `crates/ui/src/panels/thumbnails.rs`, `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/edited_cells.rs`, `crates/ui/tests/fingerprints/edited*.json`
  - scope: `architecture.md` §6.3–§6.5 and `editing.md` §9: ladder entries and the colour sketch
    are rendered through `edit::render` with the photo's recipe (the function the edit view uses);
    the cache key's recipe hash and renderer version come from `edit`'s render key, so one
    operation's version bump remakes only the photos that use it; a recipe change (from either
    history, through M7.3's hook) makes the photo's entries dead and queues new ones at visible
    priority; until they land the cell draws the old entry with M6.24's paint-time orientation
    mapping, never a blank; embedded thumbnails serve only unedited photos; cells of photos with a
    recipe show the edited badge.
  - done when: `crates/engine/tests/edited_thumbs.rs`: a recipe change remakes the photo's 128 and
    256 entries and its sketch, and the new entry equals `edit::render` at that size; bumping one
    operation's version remakes only photos using it; an unedited photo still uses its embedded
    thumbnail; `crates/ui/tests/edited_cells.rs`: no cell draws a placeholder while its new
    thumbnail renders, the badge shows and goes with undo; the cell fingerprints at both sizes.
- ⬜ **M7.10 `ui`: entering, leaving and moving between photos**
  - after: M5.23, M6.25, M6.28, M7.3, M7.4, M7.5, M7.6
  - files: `crates/ui/src/commands/edit_view.rs`, `crates/ui/src/panels/edit/top_bar.rs`, `crates/ui/src/panels/edit/filmstrip.rs`, `crates/ui/src/panels/edit/caption_bar.rs`, `crates/ui/src/panels/light_table/input.rs`, `crates/ui/src/status_text.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/tests/edit_navigation.rs`, `crates/ui/tests/fingerprints/edit-topbar*.json`, `crates/ui/tests/fingerprints/edit-caption*.json`, `crates/ui/tests/fingerprints/region-status*.json`
  - scope: `editing.md` §1–§2 and §12: entering by double-click, Enter, Ctrl+3, Picture ▸ View and
    Edit and the photo menu's bold default item; leaving by Back to Library, Esc (an open tool
    cancels first), Ctrl+1 and View ▸ Library, the library keeping its scroll and selection with the
    last photo viewed focused; ← / →, PgUp / PgDn, Home / End step through the current section's
    photos in light-table order (hidden ones only when shown); the top bar: Back, Play (disabled
    until M11), the filmstrip (◀, 7–9 square-cropped 56 pt thumbnails at `filmstrip` priority with
    the current one centred and outlined white, ▶; a click opens), the compare toggle with only A
    enabled until M11; in the edit view the UI routes Edit ▸ Undo / Redo, Ctrl+R / Ctrl+Shift+R,
    the flips and Ctrl+Shift+E (Auto Fix) to M7.3's `photo.history.*` for the open photo; the
    caption bar (the "Add a caption" field on M5.25's `text_field` through `photo.caption.set`; the
    slideshow toggle and trash disabled until M11 and M9); the edit view's status text
    (`ui-layout.md` §6.1, "(12 of 48)"). LightCraft's `crates/ui-egui/src/panels/detail.rs`
    filmstrip is the pattern (`architecture.md` §9).
  - done when: `crates/ui/tests/edit_navigation.rs`: each way in and out by id, key and menu; the
    library's scroll and selection are kept and the last photo focused; → at the end and ← at the
    start stay put, Home / End jump; a filmstrip click opens that photo and recentres; Ctrl+Z in the
    edit view undoes the photo's rotation and leaves the library stack alone; a caption typed in the
    bar is one library step; the status text; top bar, caption bar and status fingerprints at both
    sizes.
- ⬜ **M7.11 `engine`: the edit view's preview renders and their benchmark**
  - after: M7.2, M7.3, M7.8
  - files: `crates/engine/src/preview.rs`, `crates/engine/src/preview/**`, `crates/engine/tests/preview.rs`, `crates/engine/benches/edit_preview.rs`, `xtask/src/bench.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: `editing.md` §3, §5.2 and `architecture.md` §5–§6, off the owner thread at `editMain`
    priority: per open photo its screen-size source (the ladder's screen entry first, then a decode
    under the limits) kept in memory, the screen preview of its recipe, a tool's candidate recipe
    rendered as a draft (at a lower scale when that is what holds 16 ms) then final, the visible
    window at full resolution while zoomed past the preview (`render_region`; the full decode kept
    while zoomed, inside the memory budget), the previous and next photos prefetched at `prefetch`
    priority, and 256-bin RGB histograms of each preview shown; results tagged by generation so a
    stale one is dropped; everything cancelled when the photo closes. The `edit_preview` bench
    (registered in `xtask/src/bench.rs`): a 24 MP JPEG opened to its screen preview (cold and with
    the screen entry cached), a Tuning drag's draft and final updates, next and previous with
    prefetch; its budgets join the release-budgets job (M5.24); `development.md` §8 and §13 list it.
  - done when: `crates/engine/tests/preview.rs`: the preview has the recipe's displayed size; a
    newer request supersedes an older one; a draft comes before the final; a refined window equals
    that window of a full render; a prefetched neighbour opens without a decode; the histogram
    equals a direct count; closing cancels; the owner thread made no `fs` call; `cargo xtask bench
    edit_preview` meets `development.md` §8 (open ≤ 150 ms, drag ≤ 16 ms draft and ≤ 100 ms final,
    next/previous ≤ 50 ms; numbers in the PR).
- ⬜ **M7.12 `edit`: suggested crops**
  - after: M7.7
  - files: `crates/edit/src/suggest.rs`, `crates/edit/tests/suggest.rs`
  - scope: `editing.md` §6.1: three proposed rectangles in a ratio from a proxy of the current
    image, our own method: a saliency map (local contrast and colour distinctness) whose centre
    lands on a thirds point, a horizon (the strongest long near-horizontal edge) placed on a third
    line, and the largest centred fit; ranked and kept distinct; deterministic; faces join in M13.
  - done when: `cargo test -p dalica-edit --test suggest`: an off-centre blob lands near a thirds
    point in the first suggestion; a horizon scene puts the horizon near a third line; three
    distinct rectangles inside the photo in the asked ratio; a flat image gives centred crops; the
    same input gives the same output; proptest random images and ratios never panic.
- ⬜ **M7.13 `edit`: Auto Contrast, Auto Color, Auto Fix and the Tuning wands**
  - after: M7.8
  - files: `crates/edit/src/ops/auto.rs`, `crates/edit/src/analysis.rs`, `crates/edit/tests/auto.rs`, `crates/edit/tests/fingerprints/auto*.json`
  - scope: `editing.md` §6.3 and §7, analysed on the fixed-size proxy at the operation's place in
    the pipeline: `autocontrast()` (black and white points and midtones, levels linked across the
    channels), `autocolor()` (grey-world and white-patch balance with clamping), `autofix()`
    (contrast, colour and a little fill light through `ops/tone.rs`); for the Tuning wands
    `auto_lighting` (fill, highlights, shadows), `auto_neutral` (a colour) and `color_at` (the
    averaged colour under a normalised point, for the Neutral Color Picker). Our own methods;
    LightCraft's `crates/raster/src/histogram.rs` (percentiles, clipping) and
    `crates/pipeline/src/auto.rs` (`auto_wb`) are prior art to study, not code to copy.
  - done when: `cargo test -p dalica-edit --test auto`: a planted colour cast is removed (channel
    means within a stated tolerance); a low-contrast scene stretches to near full range with
    clipping under a stated share; a full-range neutral photo barely changes; the result after a
    crop differs from before; thumbnail and full size agree after downscale; the wands' values on
    known scenes; fingerprints per operation; proptest random images never panic.
- ⬜ **M7.14 `edit`: Retouch**
  - after: M7.1, M7.2
  - files: `crates/edit/src/ops/retouch.rs`, `crates/edit/tests/retouch.rs`, `crates/edit/tests/fingerprints/retouch*.json`, `NOTICE`
  - scope: `editing.md` §6.4 and `data-model.md` §3.1: `retouch(sx,sy,tx,ty,r;…)` with typed
    strokes in M7.1's frame (radius relative to the shorter side, at most 1 000 strokes per entry),
    applied in order, mapped through the crop and straighten; each patch copies the source's detail
    and keeps the target's tone with a feathered edge, reads bounded to the windows it needs; ported
    from LightCraft `crates/pipeline/src/spots.rs` (Heal, `window_for_reads`) at 76b53778, with its
    header and a `NOTICE` row (`architecture.md` §9).
  - done when: `cargo test -p dalica-edit --test retouch`: a planted blemish on a `scenes` photo is
    removed by one stroke (the patch's mean within a stated tolerance of its surroundings); strokes
    apply in order; two sizes agree after downscale; a stroke outside the photo or after a crop is
    clipped; fingerprints; hostile strokes (NaN, huge radius, over the cap) are `Err` or clamped,
    never a panic.
- ⬜ **M7.15 `ui`: the photo canvas, zoom and pan**
  - after: M6.24, M7.4, M7.5, M7.6, M7.11
  - files: `crates/ui/src/commands/zoom.rs`, `crates/ui/src/panels/edit/canvas.rs`, `crates/ui/src/panels/edit/zoom.rs`, `crates/ui/src/panels/edit/navigator.rs`, `crates/ui/src/panels/bottom.rs`, `crates/ui/src/panels/bottom/**`, `crates/ui/src/perf.rs`, `crates/ui/tests/zoom_maths.rs`, `crates/ui/tests/edit_canvas.rs`, `crates/ui/tests/fingerprints/edit-canvas*.json`, `docs/control-protocol.md`
  - scope: `editing.md` §2–§3: the photo from M7.11's preview, fitted with 24 pt padding and a
    subtle shadow, never above 100 % in Fit; `zoom.rs` the pure slider maths (v = 0 fit, 0.5 =
    100 %, 1 = 400 %, the two half formulas, never below fit, a photo smaller than the area); Fit
    and 1:1 animate 0.22 s; the wheel, Ctrl+= and Ctrl+− step anchored at the pointer and snap to
    100 % when crossing it; pinch through M7.5's `input`; click-and-hold zooms to 100 % at the point
    and a click returns to fit; double-click leaves (one click with M7.3's setting, which turns
    click-to-zoom off); the "Zoomed to 200 %" read-out; drag pans within the photo with the hand
    cursor and M7.5's glide; the navigator thumbnail while zoomed; the refined window drawn when it
    arrives, the scaled preview until then; the bottom area's Fit │ 1:1 and zoom slider in place of
    the thumbnail size, with star and rotate acting on the open photo; `ui.inspect` perf gains the
    preview's draft and final times.
  - done when: `crates/ui/tests/zoom_maths.rs`: the ends and the midpoint (100 %), monotonic,
    never below fit, the formulas at sample points; `crates/ui/tests/edit_canvas.rs` (headless):
    a wheel step keeps the image point under the pointer; crossing 100 % snaps; click-and-hold,
    click, double-click and the setting; pans stop at the photo's edges; zooming in requests the
    refined window and no frame waits for it (perf counters); canvas fingerprints fitted and at
    200 % at both sizes.
- ⬜ **M7.16 `ui`: the edit panel, Basic Fixes, Undo / Redo and the histogram box**
  - after: M6.16, M7.3, M7.4, M7.5, M7.6, M7.10, M7.11, M7.13
  - files: `crates/ui/src/panels/edit/panel.rs`, `crates/ui/src/panels/edit/basic.rs`, `crates/ui/src/panels/edit/history.rs`, `crates/ui/src/panels/edit/histogram.rs`, `crates/ui/tests/edit_panel.rs`, `crates/ui/tests/fingerprints/edit-panel*.json`, `docs/editing.md`
  - scope: `editing.md` §2, §5–§6: the 280 pt panel with its five tabs and tooltips (`tab:edit.*`;
    the effect tabs show an empty state until M12); the Basic Fixes grid (Crop, Straighten, Retouch
    open their tools through M7.4's interface; Auto Fix, Auto Contrast and Auto Color apply at once
    through `photo.history.apply`) with M7.4's Fill Light row stub in place; while a tool is open the
    rest of the panel is drawn disabled; the Undo / Redo pair filling the width with the labels of
    `photo.history.state` ("Undo Crop"; "Redo" when there is nothing to redo; tooltip "Remove the
    latest edit"); the "Histogram & Camera Info" box (≈ 248 × 150) with M7.5's histogram from the
    shown preview's bins, updated live, and the camera columns from `photo.metadata` ("No camera
    information" without EXIF, a loading line while `pending`); ids per control-protocol §7.
  - done when: `crates/ui/tests/edit_panel.rs`: each tab by id; Auto Fix by click and by
    Ctrl+Shift+E is one "Auto Fix" step and Undo reads "Undo Auto Fix"; Undo then Redo by the
    buttons; an open tool disables the grid and the tabs' content; the histogram follows an edit;
    camera information for a JPEG with EXIF and the empty text without; panel fingerprints per tab
    at both sizes.
- ⬜ **M7.17 `engine`: crop and straighten commands, ratios and suggestions**
  - after: M7.3, M7.7, M7.12
  - files: `crates/engine/src/cmd/crop.rs`, `crates/engine/tests/crop_commands.rs`
  - scope: `editing.md` §5.1 and §6.1–§6.2: applying a crop or a straighten checks its parameters
    and is one "Crop" / "Straighten" step of the photo's history; Apply on a re-opened crop appends
    the new `crop` (only the last counts), so later edits stay and undo brings back the earlier
    one; `crop.ratios.add {name, width, height}` and `crop.ratios.remove {name}` (the custom ratios
    in settings, names unique, sizes bounded), `crop.ratios.list` (query: the locale's photo sizes,
    the fixed ratios, the custom ones and the last used); `photo.crop.suggest {id, ratio}` (a job
    on a worker returning M7.12's three rectangles in the stored frame).
  - done when: `crates/engine/tests/crop_commands.rs`: crop, then tune, then re-crop: undo restores
    the first crop and keeps the tuning; adding, listing and removing a custom ratio survives a
    restart; the suggestion job answers three rectangles without the owner thread touching `fs`;
    every command's contract and graceful failures; `panic_hunt` green.
- ⬜ **M7.18 `engine`: Tuning sessions, the wands and the colour picker**
  - after: M7.3, M7.8, M7.13
  - files: `crates/engine/src/cmd/tuning.rs`, `crates/engine/tests/tuning_commands.rs`
  - scope: `editing.md` §6.5 and §7: `photo.history.tuning {id, session, fill?, highlights?,
    shadows?, temp?, neutral?}`: a session's first change appends a `tuning` entry (or updates the
    last entry when it is one), later changes update it in place, and the session is one "Tuning"
    step (the `coalesce` key of `architecture.md` §3); the Basic Fixes Fill Light uses the same
    command; `photo.tuning.auto {id, what: lighting | color}` and `photo.tuning.pick {id, x, y}`
    are jobs answering M7.13's values from the photo's current render, which the UI then applies
    as a session change.
  - done when: `crates/engine/tests/tuning_commands.rs`: a drag of 60 changes is one step that one
    undo removes; a second session after a crop appends a new entry; Fill Light from Basic Fixes
    and from Tuning edit the same value; the wands and the picker answer from a worker and the
    owner thread made no `fs` call; every command's contract and graceful failures; `panic_hunt`
    green.
- ⬜ **M7.19 `ui`: the Crop tool**
  - after: M7.15, M7.16, M7.17
  - files: `crates/ui/src/panels/edit/crop.rs`, `crates/ui/src/panels/edit/crop/**`, `crates/ui/src/dialogs/custom_ratio.rs`, `crates/ui/tests/crop_tool.rs`, `crates/ui/tests/fingerprints/edit-crop*.json`, `crates/ui/tests/fingerprints/dialog-custom-ratio*.json`
  - scope: `editing.md` §6.1: the "Crop Photo" panel (title, instruction, the size dropdown with
    the live "Manual: 1210 × 1719" row, Current ratio, the locale's photo sizes, the fixed ratios,
    "Custom ratios" and "Add Custom Ratio…", the trash for a custom ratio, the last ratio
    remembered); the three 80 × 50 suggestions from `photo.crop.suggest`; Rotate (disabled until a
    rectangle exists), Preview (held or toggled), Reset, Apply, Cancel; on the photo: drag to draw
    with the outside dimmed `black α140`, edges and corners resize with the ratio locked unless
    Manual, drag inside moves, the thirds from M7.7's guides while dragging, through M7.7's
    `crop_geom` and M7.1's frame; Enter applies, Esc cancels; a re-opened crop shows the whole photo
    with the current rectangle; the Add Custom Ratio dialog (Width × Height, Name, a live example
    line, OK disabled until valid).
  - done when: `crates/ui/tests/crop_tool.rs` (headless, by id): draw at 4:3 and the ratio holds;
    Manual is free; corner, edge and move drags; Rotate swaps the orientation; Preview shows the
    cropped photo; Apply is one "Crop" step and the thumbnail follows; on a rotated photo the
    stored rectangle is in M7.1's frame; a suggestion click draws its rectangle; the dialog refuses
    0 × 5 and adds 3 × 2; Esc cancels; panel and dialog fingerprints at both sizes.
- ⬜ **M7.20 `ui`: the Straighten tool**
  - after: M7.15, M7.16, M7.17
  - files: `crates/ui/src/panels/edit/straighten.rs`, `crates/ui/tests/straighten_tool.rs`, `crates/ui/tests/fingerprints/edit-straighten*.json`
  - scope: `editing.md` §6.2: the dashed white grid from M7.7's guides over the photo; the floating
    bar (`black α180`, radius 6, 10 pt above the photo's bottom edge, centred) with the wide slider
    (centred thumb = 0°), Apply and Cancel; the photo rotates live, scaled so no corner is empty,
    through draft previews; arrow keys nudge 0.1°; Esc cancels; Apply is one "Straighten" step.
  - done when: `crates/ui/tests/straighten_tool.rs`: a drag to +3.5° previews live and Apply
    stores it; arrows nudge by 0.1° within ±10°; Esc leaves the recipe unchanged; the result has no
    empty corner; fingerprints at 0° and +5° at both sizes.
- ⬜ **M7.21 `ui`: the Tuning tab and the Fill Light row**
  - after: M7.15, M7.16, M7.18
  - files: `crates/ui/src/panels/edit/tuning.rs`, `crates/ui/src/panels/edit/fill_light.rs`, `crates/ui/tests/tuning_tab.rs`, `crates/ui/tests/fingerprints/edit-tuning*.json`
  - scope: `editing.md` §6.5 and §7: Fill Light, Highlights and Shadows with the Auto lighting wand,
    Color Temperature, the Neutral Color Picker (swatch, eyedropper, a click on a grey or white
    area through `photo.tuning.pick`, its tooltip) with the Auto color wand, on M7.5's Tuning
    slider; a tab session is one "Tuning" step through `photo.history.tuning`, the photo updating
    from draft previews while dragging; the Basic Fixes Fill Light row shares the value; ids per
    control-protocol §7.
  - done when: `crates/ui/tests/tuning_tab.rs`: a Fill Light drag is one "Tuning" step and the
    Basic Fixes row shows the same value; draft updates stay within 16 ms per frame on the perf
    counter; Auto lighting sets three sliders in one step; picking the grey patch of a cast
    `scenes` photo removes the cast; hover tooltips, + / − nudges and double-click resets; tab
    fingerprints at both sizes.
- ⬜ **M7.22 `ui`: the Retouch tool**
  - after: M7.14, M7.15, M7.16
  - files: `crates/ui/src/panels/edit/retouch.rs`, `crates/ui/tests/retouch_tool.rs`, `crates/ui/tests/fingerprints/edit-retouch*.json`
  - scope: `editing.md` §6.4: the Retouch panel (instruction, Brush size, Undo Patch │ Redo Patch,
    Reset, Apply │ Cancel); click the blemish (a brush circle stays), the target previews the
    texture under the pointer, a click picks the source; each patch is a step inside the tool;
    + / − resize the brush and the wheel zooms; Apply records all patches as one "Retouch" step
    through `photo.history.apply`; Esc cancels.
  - done when: `crates/ui/tests/retouch_tool.rs`: two patches, Undo Patch, Redo Patch, Apply gives
    one "Retouch" entry with two strokes in M7.1's frame; Cancel leaves the recipe unchanged; the
    brush keys; a planted blemish is gone in the preview; panel fingerprints at both sizes.
- ⬜ **M7.23 `accept M7`**
  - after: M7.9, M7.10, M7.19, M7.20, M7.21, M7.22
  - files: `tests/acceptance/m7.rs`, `tests/acceptance/m7_*.rs`, `tests/acceptance/m7.md`, `tests/acceptance/fingerprints/m7-*.json`
  - scope: every *Done when* of M7's three areas in the real `dalica --headless`: the zoom maths
    (ends and midpoint = 100 %); navigation in and out and between photos; a photo's history and
    its labels surviving a restart; a 24 MP JPEG's screen preview in ≤ 150 ms and a slider drag's
    draft in ≤ 16 ms (the release bench; inside `cargo xtask ci` a smaller debug run checks the
    path and prints its times); fingerprints per operation and per Tuning control; resolution
    independence (two sizes agree after downscale); a thumbnail equal to the edit view's render
    after downscale; an unknown operation surviving every tool; a whole drag as one history step;
    the manual checks in `m7.md` (open, zoom, crop, straighten, tune, retouch and undo on your own
    photos).
  - done when: `cargo xtask accept M7` passes everything in M7's *Done when*.
- ⬜ **M7.24 Lead: task blocks for M9**
  - after: M6.37
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M9 task block from M9's section and the spec, with waves and the `M9.R`
    review (owner: maintainer); a block that can port Craft-app code names the source files in its
    scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M7.R Check-in**
  - after: M7.23
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: the lead runs `/milestone-review M7.R` to prepare it: the check-in page (open a photo by
    double-click and by Enter, step through the filmstrip, zoom and pan, crop to 4 × 6 and to a
    custom ratio, straighten a tilted horizon, Auto Fix, the Tuning sliders and the neutral
    picker, a retouch with patch undo, the Undo / Redo labels, a restart that keeps the history,
    thumbnails showing the edits, written as steps for the maintainer to try); the questions: the
    look of Auto Fix, Auto Contrast, Auto Color and the Tuning controls (approved with their
    fingerprints), the planning decisions above that change behaviour (starring in the edit view
    as a library step, Fill Light written as Tuning), and the coordinate frame if M6.R left it
    open; *Where we stand* and the `status.md` numbers (24 MP open, slider draft and final,
    next/previous, render time per operation) on a `review/M7.R` branch, a pull request open. Only
    the maintainer merges that pull request and closes this issue; nothing after it starts until
    then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 0   M7.1 coordinate frame   (pulled into M6's run so M6.R asks reserved decision 11)
wave 1   M7.2 edit renderer  ‖  M7.3 photo history + modules  ‖  M7.4 ui state + frame
         ‖  M7.5 ui-kit widgets + input  ‖  M7.6 ui-kit icons
         (once M6.R is closed)
wave 2   M7.7 Crop + Straighten  ‖  M7.8 Tuning op  ‖  M7.9 edited thumbnails
         ‖  M7.10 entering + navigation
wave 3   M7.11 preview renders + bench  ‖  M7.12 crop suggestions  ‖  M7.13 auto fixes
         ‖  M7.14 Retouch op
wave 4   M7.15 canvas, zoom + pan  ‖  M7.16 edit panel  ‖  M7.17 crop commands
         ‖  M7.18 Tuning commands
wave 5   M7.19 Crop tool  ‖  M7.20 Straighten tool  ‖  M7.21 Tuning tab  ‖  M7.22 Retouch tool
wave 6   M7.23 accept M7 → M7.R          (M7.24 lead: M9 blocks, any time after M6.37)
```

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
- **Reuse** (`architecture.md` §9): LightCraft's `panels/keymap.rs` (click to rebind, × removes, ↺
  restores) and PhotoCraft's key capture (`prefs_ui.rs` `shortcuts_body`,
  `shortcut_capture_tests.rs`) for the shortcut editor, which adds search, the Context filter and
  conflicts; `shortcuts.rs` `parse` / `pretty` from either app; PhotoCraft's `prefs_ui.rs`
  working-copy model for the draft and the OK / Cancel / Apply rules.
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
| M11 Import | rooms/tab strip (craft-ui `tabs` and PhotoCraft's `tab_strip.rs` overflow menu); sources (cards, removable drives, folders); time grouping; exclude duplicates; destination and folder naming; copy-verify; optional removal from the card | import of a simulated card into dated folders; duplicates skipped; no file overwritten |
| M11 Compare and batch edit | A│B, A│A (LightCraft's `panels/compare.rs` as the pattern), before/after hold, Copy/Paste All Edits, Batch Edit ▸, Undo All Edits | batch of 100 photos is one undo step |
| M11 Slideshow and quick preview | slideshow, control bar, transitions, captions, music; Ctrl+Alt preview | transitions never wait on disk (prefetch test) |
| M11 Photo viewer | `dalica --view`, filmstrip (LightCraft's `panels/detail.rs` as the pattern), controls, settings | open, navigate, star from the viewer |
| M12 Effects I | effect tiles, parameter panels (colour parameters from PhotoCraft's `color_picker_ui.rs` colour maths and `widgets.rs` `swatch_popup`), crosshair control, Fade; the 12 effects of tab 3 | a fingerprint and an invariant per effect |
| M12 Effects II | the 15 effects of tabs 4–5, canvas-enlarging effects, Soften's paint mask | as M12 |
| M13 People I | LightCraft's `crates/faces` pipeline and `panels/faces.rs` as the starting point; consented model download (decisions 5 and 9), detection, embeddings, clustering, Unnamed, naming, ignore, person albums | a public-domain face set groups as expected (floors) |
| M13 People II | suggestions, People pane, manual faces, merge/rename/delete, MWG regions | MWG round-trip tests (`xmp-interop.md` §8.5) |
| M13 Places | Places pane, opt-in map tiles (decision 4), geotag by drag and search, GPS write, Geotag menu | GPS round-trip; no network without the user's action |
| M14 Collage | the collage room, styles, canvas, render + recipe, Edit Collage | deterministic layouts per seed; render size |
| M14 Legacy sidecars | read-only reader with provenance header, first-run notice, conversion on first write, legacy originals for Revert | `data-model.md` §10.11 |

### M15 — Video, localisation, OS integration and Release 1.0 (outline)

| # | In | Done when (sketch) |
|---|---|---|
| M15 Video | reserved decision 2 first; then poster frames, playback, trim, frame save — or descope to tiles | decided and recorded; tests per chosen scope |
| M15 Localisation and accessibility | translation catalogs (from LightCraft's `i18n.rs` and `locales/*.json`, with its script-based CJK font fallback), AccessKit labels, screen-reader and keyboard-only passes, scaling | coverage per language reported; AccessKit tree tests |
| M15 OS integration | drag out to other apps, clipboard files, Open With ▸, external editor, Locate on Disk with selection, default-viewer registration, MTP/PTP cameras | per-OS manual checklist plus what can be tested per OS in CI |
| M15 Release 1.0 | signed packages, full honest assessment, release notes | the release checklist (`releasing.md`) |

### M16 — Post-1.0 enhancements (outline)

Improvements the maintainer wants after 1.0, beyond parity with the reference organizer. Nothing
here starts before M15.R closes; the lead details these items into task blocks while M15 runs, and
the maintainer adds items here as they come up (maintainer decision, 2026-10-11).

| # | In | Done when (sketch) |
|---|---|---|
| M16 Themes | switchable themes, taken from the Craft apps: today's dark theme stays the default (it is LightCraft's look, LightCraft's only theme), plus PhotoCraft's twelve: Pro (Dark), Pro (Medium Gray), Pro (Light Gray), Pro (Light), Studio (Dark), Studio (Light), Classic, Adwaita (Light), Adwaita (Dark), Solarized Dark, Breeze Light, Breeze Dark (as of `storytold/photocraft` 30bd8a55). The same architecture and implementation as PhotoCraft's `crates/ui-egui/src/theme.rs`: a `ThemeKind` enum with id, label, `from_name` and `next`, one token set per kind (`Tokens::for_kind`), the theme kept in settings, one command per theme plus `ui.set {theme}`, and a Theme choice in Options › General › Interface. PhotoCraft's palettes and code are mapped onto Dalica's `ui-kit` `Tokens`, with a header naming the source and a `NOTICE` row (`architecture.md` §9), plus the Solarized palette's `ATTRIBUTION.md` row and MIT licence text | every theme passes the `ui-kit` contrast tests; a widget-gallery fingerprint per theme; switching by command, CLI and MCP repaints without a restart and survives one; the main window screenshotted in every theme |

## Reserved decisions (the maintainer's, not the agents')

Record the decision here with a date when made.

1. Reuse model. **Decided 2026-10-09: reference only; revised 2026-10-11: reuse allowed.**
   LightCraft and PhotoCraft share Dalica's licence, so their code may be copied and adapted with
   a header naming its origin and a `NOTICE` row. The reference organizer's code is never looked
   at or used. crates.io libraries stay allowed after the dependency check (`architecture.md` §9).
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
