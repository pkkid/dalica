# Roadmap

Milestones for Dalica and their task blocks, with task ids used in commit subjects (`M1.3: …`).
This file is the plan; the live state of every task (ready, claimed, in review, done) is the
GitHub task board, kept in sync by `cargo xtask tasks` ([`task-board.md`](task-board.md)). Agents
get work with `cargo xtask tasks next`, never by scanning this file.

Status legend: ✅ done · 🚧 in progress · ⬜ not started. Task blocks use only ⬜ and ✅; their live
state (ready, claimed, in review) is on the task board ([`task-board.md`](task-board.md)).

## Where we stand

*Honest assessment, one dated entry per review, newest first (latest: 2026-10-14, M8.R). Agents:
read this before picking work. Update it only with measured facts, dated.*

*2026-10-14 (M8.R, the Release 0.3 gate).* M8 is done: `cargo xtask accept M8` passes in 105 s
(148 s with the release budgets), and the tests on `main` went from 2 242 to 2 662. Save (the
folder header's "n edited" button, File ▸ Save, Ctrl+S) writes a photo's edits into the file after
moving the original into the folder's `.dalica-originals/`; Undo Save and Revert put it back byte
for byte, and a second Save renders from the kept original. JPEGs keep their quality (estimated
from their tables: 0 error over 33 files), their subsampling and their metadata; rotate-only saves
are lossless in the DCT domain; formats Dalica cannot write get a JPEG beside them. Save a Copy,
Save As and Export to Folder (sizes, quality, numbered names, a watermark; in the background with
Cancel; exported folders under Projects) all go through one output pipeline. Measured on release
builds: a 24 MP Save in 292 ms and a lossless rotation in 357 ms (budgets 1050 and 815 ms on two
cores), a 200-photo mixed export in 6.8 s (budget 24 s). Fault injection of every step of Save,
Undo Save and Revert (208 cases) found one way a failed Save could orphan the original; it was
fixed before anything shipped, and an interrupted Save is now always finished or undone at the next
start. The run also found and fixed a watcher rescan racing a Save. Edits other than rotations are
still rendered at 8 bits, so an edited 16-bit PNG saves with 8-bit precision. Fix tasks
M9.27–M9.30 carry the rest. M8.R is the 0.3 release gate (version 0.3.0).

*2026-10-13 (M7.R).* M7 is done: `cargo xtask accept M7` passes in 53 s (117 s with the release
budgets), and the tests on `main` went from 1 858 to 2 242. Enter, Ctrl+3 or a double-click opens a
photo in the edit view: a filmstrip, ← → through the folder, zoom (wheel, pinch, Ctrl+= / Ctrl+−,
hold for 100 %) and pan with a navigator, and an edit panel with Basic Fixes (Crop with three
suggested crops and custom ratios, Straighten ±10° with no empty corner, Retouch, Auto Fix, Auto
Contrast, Auto Color, Fill Light), a Tuning tab (Fill Light, Highlights, Shadows, Color
Temperature, a neutral picker and two wands), Undo / Redo and a histogram. Every edit is a step
in the photo's own history, which survives a restart; nothing touches the original file, and an
operation Dalica does not know survives every tool. Thumbnails, the tray and the filmstrip show
edits with a pencil badge and never fall back to a grey cell. Measured on release builds: a 24 MP
JPEG opens in 77 ms (70 ms on the 2-CPU Budgets runner; budget 150), a slider draft takes 8.4 ms
at p95 (budget 16) and its final 14.1 ms (budget 100); next/previous is a prefetched cache hit.
The run found and fixed an Adjust Date undo that lost the original date once the write had landed
(M7.30), a lost preview job that stalled the view (M7.23), and a Windows-only test failure that
showed the read-before-write guard cannot see a same-size rewrite there; the maintainer chose a
real Windows file identity (M8.27). Fix tasks M8.27–M8.30 carry what the review found; the 86
fingerprint baselines and the look of Auto Fix and Tuning were approved. M7.R is the 0.2 release
gate (version 0.2.0): from here every milestone up to M15 publishes a point release, ending at 1.0.

*2026-10-12 (M6.R, the Release 0.1 gate).* M6 is done: `cargo xtask accept M6` passes in 62 s
standalone, and the tests on `main` went from 1 440 to 1 858. Users can rotate, flip, hide, caption,
tag (with quick tags and Rename Tag) and re-date photos, describe folders and set their date and
place, and read every photo's metadata in the Properties pane (Show All Metadata); the About window
credits contributors and models, and Help ▸ Release Notes shows `whats-new.md`. Captions, tags and
dates are written into JPEG, PNG, WebP and TIFF files about a second after a change (never into RAW
pixels; an `.xmp` beside RAWs only when that option is on), atomically, keeping the file's time and
every byte other applications wrote: the corpus of 53 files that exiv2, ImageMagick and Pillow wrote
takes 441 writes with 0 failures. Values other applications wrote come in on scan as one undoable
step; both-changed keeps Dalica's value and says so in Properties. A rebuild now keeps every M6
field (a data-loss bug found and fixed before merge, M6.14). Our own corpus sets are published and
pinned (`photos` 57, `heif` 3, `metadata` 53; every image set decodes in full). Version 0.1.0 builds
16 unsigned packages on Linux, Windows and macOS in the release dry run; the maintainer signed off
on shipping 0.1 (HEIF stays out of release builds until M10 decides; `dalica.exe --version` prints
nothing in a console). The 10 k first screenful is 500 ms locally and 684–850 ms on the 4-CPU CI
runner (budget 1 s; M5: 500–517 ms) after M6.32's import-on-scan regression was fixed (#312).
Open: M7.25–M7.29 (file times across a crash, refusals for ImageMagick-written files, caution
toasts, the accept M5 Windows flake, docs), the edit view (M7).

*2026-10-12 (M5.R).* M5 is done: `cargo xtask accept M5` passes in 98 s (651 s with
`DALICA_ACCEPT_RELEASE=1`), and the tests on `main` went from 822 to 1 440. The library now follows
the user's folders: the Folder Manager (Scan Always, Scan Once, Remove, inherited by subfolders, one
undo step) and the first-run panel; `notify` watching with a polling fallback and a verification
pass that alone finds every change; renamed and moved folders keep their photos, stars and ids;
an unplugged drive, or an emptied mount point, shows offline and gets its edits when it returns;
stale temporaries are cleaned and a photo deleted outside Dalica leaves its sidecar after 30 days.
It reads TIFF, WebP, GIF, BMP, JPEG XL, PSD composites and the previews of 13 raw kinds (HEIC
behind the `heif` feature; AVIF shown as a tile), converts ICC profiles to sRGB on all three OSes,
and shows "!" with a reason for what it cannot show. A just-added folder's first screenful of
thumbnails takes 500–517 ms for 10 k JPEGs (budget 1 s), and the speed thumb through a scanned
100 k library shows no grey cell (p95 0.75 ms). The corpus job checks PngSuite (162 of 176 decoded,
all 14 corrupt refused) and 26 CC0 raws (20 decoded); its real-camera misses go to M6.38, and our
own `photos` and `heif` sets wait for M6.42. A Linux job asserts the release budgets on every push.
Open: four process and safety fixes in M6 (M6.39–M6.41, M6.43); Windows folder redirection is not
followed; the display is treated as sRGB.

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
- **Every milestone is a release, up to 1.0** (maintainer rule, 2026-10-13). M6 shipped 0.1; from
  M7 on, each milestone's review `M<n>.R` is a release gate that publishes the next point release
  on GitHub: M7 0.2, M8 0.3, M9 0.4, M10 0.5, M11 0.6, M12 0.7, M13 0.8, M14 0.9, M15 1.0. The
  review branch sets the version (`cargo xtask version set`) and adds its section to
  `whats-new.md`; the check-in adds the release checklist and the sign-off question; once the
  maintainer has merged the review and said ship, the lead cuts the release (`releasing.md` →
  Cutting a release) and publishes it. M16 and later releases are planned after 1.0.
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
| | M2 | Agent eyes | agents drive and screenshot the real window | ✅ |
| Library | M3 | Window shell | every region of the main window, at the right size, in the Craft look | ✅ |
| | M4 | Library browsing | the continuous light table at 100 k photos, the library tree, selection and the tray | ✅ |
| | M5 | Folders, scanning and formats | Folder Manager, first run, watching, offline drives; every listed format, RAW previews, ICC | ✅ |
| | M6 | Photo info, metadata and Release 0.1 | rotate, hide, captions, Properties, dates, tags written into files; installable preview | ✅ |
| Edit and output | M7 | Edit view and fixes | edit view, basic fixes, Tuning and Retouch; thumbnails show edits | ✅ |
| | M8 | Save and export | Save, Undo Save, Revert, Save a Copy, Save As; export | ✅ |
| Organize | M9 | Albums, search and file operations | lasting albums, instant search and filters, move/copy/rename/delete | ⬜ |
| | M10 | Preferences, MCP and Release 0.5 | Options and shortcut editor, MCP and the P0 workflows; all P0 rows shipped | ⬜ |
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
filled by the lead; M5.15 pins the upstream sets and leaves our own `photos` and `heif` sets "not published yet" until the repository has files.

- ✅ **M5.1 `image`: one module per format, the decoded-image model and colour management**
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
- ✅ **M5.2 `scenes`: test files in every M5 format**
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
- ✅ **M5.3 `fs`: writes through a held folder handle and stale temp cleanup**
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
- ✅ **M5.4 `scan`: skip rules, volumes, standard places and per-file guards**
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
- ✅ **M5.5 `engine`: session modules, the job registry and progressive scans**
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
- ✅ **M5.6 `image`: TIFF**
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
- ✅ **M5.7 `image`: WebP, GIF and BMP**
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
- ✅ **M5.8 `image`: JPEG XL and PSD composites**
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
- ✅ **M5.9 `image`: RAW previews from TIFF-structured raws, and TIFF-strip EXIF thumbnails**
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
- ✅ **M5.10 `image`: RAW previews from other containers, HEIF behind its feature, AVIF tiles**
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
- ✅ **M5.11 `scan`: the watcher and incremental verification**
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
- ✅ **M5.12 `scan` + `engine`: folder modes, inheritance and the Folder Manager commands**
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
- ✅ **M5.13 `engine` + `store`: offline volumes and queued writes**
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
- ✅ **M5.14 `ui`: the dialog host, M5's view state and job methods**
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
- ✅ **M5.15 `xtask corpus` and `test-corpus` with floors**
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
- ✅ **M5.16 `engine` + `thumbs`: cache upkeep, colour management and the first-thumbnails benchmark**
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
- ✅ **M5.17 `engine` + `store`: watching and external changes end to end**
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
- ✅ **M5.18 `ui`: the Folder Manager and the first-run panel**
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
- ✅ **M5.19 `ui`: the activity indicator, scan progress and offline folders**
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
- ✅ **M5.22 `xtask tasks`: send-backs and the parity summary (found at M4.R)**
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
- ✅ **M5.23 `ui`: M4 loose ends in the light table and keys**
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
- ✅ **M5.24 CI: the release frame budgets on `main` (Linux)**
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
- ✅ **M5.20 `accept M5`**
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
- ✅ **M5.25 `ui-kit`: text field behaviour and an autocomplete list, from the Craft apps**
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
- ✅ **M5.R Check-in**
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

- ✅ **M6.1 The application id in one place**
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
- ✅ **M6.2 `library`: M6's model fields, ops and the tag index**
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
- ✅ **M6.3 `edit`: the recipe and the geometric operations**
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
- ✅ **M6.4 `meta`: the field model, container reading, EXIF and IPTC, and the write skeleton**
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
- ✅ **M6.5 `engine`: M6's command modules and settings**
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
- ✅ **M6.6 `ui`: M6's view state, editor and dialog commands, and stub panes**
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
- ✅ **M6.7 `xtask version`, build information and portable mode**
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
- ✅ **M6.8 `sidecar` + `store`: every M6 field in `.dalica.json` and the index**
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
- ✅ **M6.9 `meta`: XMP reading and the merge writer**
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
- ✅ **M6.10 `meta`: IPTC writing and EXIF edits in place**
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
- ✅ **M6.11 `xtask contributors`**
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
- ✅ **M6.12 The app icon (a candidate for the maintainer)**
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
- ✅ **M6.13 The release workflow, end to end**
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
- ✅ **M6.14 `engine`: rotate, flip, hide, captions and folder info**
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
- ✅ **M6.15 `engine`: tag commands and quick tags**
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
- ✅ **M6.16 `engine`: dates and the photo metadata query**
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
- ✅ **M6.17 `meta`: JPEG write-back**
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
- ✅ **M6.18 `meta`: PNG, WebP and TIFF write-back**
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
- ✅ **M6.19 `meta`: `.xmp` sidecars for RAW**
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
- ✅ **M6.20 `ui`: the About window and the release notes**
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
- ✅ **M6.21 Linux packages**
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
- ✅ **M6.22 Windows packages**
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
- ✅ **M6.23 macOS packages**
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
- ✅ **M6.24 `ui`: rotation and hiding in the library**
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
- ✅ **M6.25 `ui`: captions in the library**
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
- ✅ **M6.26 `ui`: folder descriptions and Folder Properties**
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
- ✅ **M6.27 `ui`: the Properties pane**
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
- ✅ **M6.28 `ui`: the Tags pane**
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
- ✅ **M6.29 `ui` + `ui-kit`: Adjust Date and Time**
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
- ✅ **M6.30 `store`: the metadata write queue**
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
- ✅ **M6.31 `ui`: quick tags**
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
- ✅ **M6.32 `scan` + `engine`: importing file metadata on scan**
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
- ✅ **M6.33 `engine` + `ui`: metadata write-back end to end**
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
- ✅ **M6.34 `meta`: the metadata corpus**
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
- ✅ **M6.35 Release 0.1: notes, version and README**
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
- ✅ **M6.36 `accept M6`**
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
- ✅ **M6.37 Lead: task blocks for M8**
  - after: M5.21
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M8 task block from M8's section and the spec, with waves and the `M8.R`
    review (owner: maintainer); a block that can port Craft-app code names the source files in its
    scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ✅ **M6.38 `image`: real-camera fixes found by the M5 corpus**
  - after: M5.R
  - files: `crates/image/src/formats/raw/**`, `crates/image/src/formats/tiff.rs`, `crates/image/src/sniff.rs`, `crates/image/src/ifd.rs`, `crates/image/tests/raw_tiff.rs`, `crates/image/tests/raw_containers.rs`, `crates/image/tests/tiff.rs`, `crates/image/tests/corpus_floors.json`
  - scope: found by `cargo xtask test-corpus` at M5.15/M5.20 (M5.R): both Samsung SRW files and the
    Sony DSLR-A100 ARW are sniffed as plain TIFF and then refused as damaged; the Olympus C5060WZ
    ORF and both MRW files answer "no preview"; the Olympus E-3 ORF decodes from the whole file but
    a header read from the first 256 KiB answers "no preview" instead of asking for more; a
    truncated 16-bit CMYK Deflate TIFF answers "Too large to display" instead of "Damaged file"
    (`decode_page` sizes `intermediate_buffer_size` to `largest_chunk.min(file length)`, so a cut
    chunk raises the decoder's limit error). Fix each from public structure descriptions (provenance
    headers stay), with a synthetic regression test per case built in the test.
  - done when: `cargo test -p dalica-image`: each case's synthetic file now decodes or answers its
    right reason; `cargo xtask test-corpus` passes with the `raw` floors raised to the new measured
    counts (numbers in the PR) and no floor lowered.
- ✅ **M6.39 `ui`: File ▸ Add Folder… opens the Folder Manager; Folder ▸ Remove from Dalica… live**
  - after: M5.R, M6.6
  - files: `crates/ui/src/menu_catalog.rs`, `crates/ui/src/menus.rs`, `crates/ui/tests/menus.rs`, `crates/ui/src/control/tests.rs`, `crates/ui/tests/library_window.rs`, `crates/ui/tests/widget_ids.rs`, `crates/ui/src/panels/light_table.rs`, `docs/menus-and-shortcuts.md`
  - scope: left open by M5.18 (its files could not reach them): File ▸ Add Folder… and the empty
    library's Add Folder button open the Folder Manager instead of the system folder picker
    (`organizing.md` §2), and Folder ▸ Remove from Dalica… goes live through the dialog host's
    "Remove Folder" question (`folder.remove`); the tests that pinned the picker flow follow.
  - done when: `cargo test -p dalica-ui`: both entry points open `dialog:folderManager` with the
    selected library folder pre-selected; Remove from Dalica asks and removes one folder in one undo
    step; `widget_ids` and earlier milestones' acceptance tests pass.
- ✅ **M6.40 `xtask tasks`: parity row edits that touch different rows merge by themselves**
  - after: M5.R
  - files: `xtask/src/tasks/**`, `xtask/src/parity.rs`, `docs/task-board.md`
  - scope: found in the M5 run: the `docs/parity.md` rows of one area are neighbours, so two tasks
    that each edit their own row conflict as adjacent lines; M5 had a dozen send-backs and repeated
    three-OS runs for nothing else. `tasks merge` resolves a rebase conflict confined to
    `docs/parity.md` rows when each side changed different rows (by row id), then regenerates the
    summary; two edits to the same row still go back to the worker.
  - done when: `xtask` tests on a temp repository: neighbouring row edits merge with both rows kept;
    an edit of the same row by both sides is refused; the summary is regenerated.
- ✅ **M6.41 `image`: a JPEG XL colour profile is bounded before the decoder reserves it**
  - after: M5.R
  - files: `crates/image/src/formats/jxl.rs`, `crates/image/tests/jxl.rs`
  - scope: found at M5.8: `jxl-oxide` can reserve up to 256 MiB for an embedded ICC profile outside
    its allocation tracker from a small file (the colour stage then refuses anything over 16 MiB).
    Read the codestream's declared ICC size first and refuse a file whose profile passes
    `security.md` §4.2's ICC limit before handing it to the decoder, or charge it to the budget.
  - done when: `cargo test -p dalica-image --test jxl`: a small file declaring a 200 MiB profile is
    refused as too large before any allocation of that size (checked through the budget); valid
    files with profiles still match their fingerprints.
- ✅ **M6.42 Lead: our own corpus sets, published and pinned**
  - after: M5.R
  - owner: lead
  - files: `xtask/src/corpus_pins.rs`, `xtask/corpus/**`, `crates/image/tests/corpus_floors.json`, `docs/development.md`
  - scope: the maintainer's answer at M5.R (2026-10-12): the lead fills `pkkid/dalica-corpus` with
    public-domain and CC0 photos (mixed formats, orientations, ICC-tagged, CMYK and 16-bit) and
    HEIC samples, each file with its source and licence in the repository's manifest; the
    maintainer approves the source list (asked through the board's review of this task's PR)
    before anything is pushed there; then the `photos` and `heif` sets are pinned by commit and
    sha256 like the upstream sets.
  - done when: `cargo xtask corpus --all` fetches and verifies the two sets; `cargo xtask
    test-corpus` passes with their floors at the measured counts (numbers in the PR); every file
    in the repository has a source and a public-domain or CC0 licence.
- ✅ **M6.43 `engine` + `store`: a sidecar write checks its root right before it lands**
  - after: M5.R, M6.5
  - files: `crates/engine/src/session.rs`, `crates/engine/src/offline.rs`, `crates/store/src/writer.rs`, `crates/engine/tests/offline.rs`, `crates/store/tests/offline.rs`
  - scope: the maintainer's answer at M5.R (2026-10-12): M5.17 holds writes under roots found
    offline, but an edit made in the up to 10 s between an unmount and the next online check can
    still be written into the empty mount point. The I/O worker checks the root's identity (the
    remembered device id where the platform has one, and one of the library's sample files under
    it) right before each sidecar write, and holds the write as offline when it fails.
  - done when: tests: a root unmounted (emptied, or its device changed) just before a due write
    gets no sidecar, the edit is reported unsaved and lands when the files return; the existing
    offline and sidecar tests pass unchanged; the owner thread makes no `fs` call.
- ✅ **M6.R Release 0.1 gate**
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
         (M6.38–M6.43 from M5.R, any time after it; M6.39 after M6.6, M6.43 after M6.5;
          M6.42 lead: our own corpus sets)
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

- ✅ **M7.1 The coordinate frame for stored rectangles and points**
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
- ✅ **M7.2 `edit`: the renderer, one module per operation, per-operation versions**
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
- ✅ **M7.3 `engine`: the photo's own history, M7's command modules and settings**
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
- ✅ **M7.4 `ui`: the edit view's state, frame and stub panels**
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
- ✅ **M7.5 `ui-kit`: tabs, tool buttons, the histogram, the Tuning slider and canvas input**
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
- ✅ **M7.6 `ui-kit`: the edit view's icons**
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
- ✅ **M7.7 `edit`: Crop and Straighten**
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
- ✅ **M7.8 `edit`: the Tuning operation**
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
- ✅ **M7.9 `engine` + `thumbs`: edits shown on every thumbnail**
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
- ✅ **M7.10 `ui`: entering, leaving and moving between photos**
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
- ✅ **M7.11 `engine`: the edit view's preview renders and their benchmark**
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
- ✅ **M7.12 `edit`: suggested crops**
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
- ✅ **M7.13 `edit`: Auto Contrast, Auto Color, Auto Fix and the Tuning wands**
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
- ✅ **M7.14 `edit`: Retouch**
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
- ✅ **M7.15 `ui`: the photo canvas, zoom and pan**
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
- ✅ **M7.16 `ui`: the edit panel, Basic Fixes, Undo / Redo and the histogram box**
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
- ✅ **M7.17 `engine`: crop and straighten commands, ratios and suggestions**
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
- ✅ **M7.18 `engine`: Tuning sessions, the wands and the colour picker**
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
- ✅ **M7.19 `ui`: the Crop tool**
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
- ✅ **M7.20 `ui`: the Straighten tool**
  - after: M7.15, M7.16, M7.17
  - files: `crates/ui/src/panels/edit/straighten.rs`, `crates/ui/tests/straighten_tool.rs`, `crates/ui/tests/fingerprints/edit-straighten*.json`
  - scope: `editing.md` §6.2: the dashed white grid from M7.7's guides over the photo; the floating
    bar (`black α180`, radius 6, 10 pt above the photo's bottom edge, centred) with the wide slider
    (centred thumb = 0°), Apply and Cancel; the photo rotates live, scaled so no corner is empty,
    through draft previews; arrow keys nudge 0.1°; Esc cancels; Apply is one "Straighten" step.
  - done when: `crates/ui/tests/straighten_tool.rs`: a drag to +3.5° previews live and Apply
    stores it; arrows nudge by 0.1° within ±10°; Esc leaves the recipe unchanged; the result has no
    empty corner; fingerprints at 0° and +5° at both sizes.
- ✅ **M7.21 `ui`: the Tuning tab and the Fill Light row**
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
- ✅ **M7.22 `ui`: the Retouch tool**
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
- ✅ **M7.23 `accept M7`**
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
- ✅ **M7.24 Lead: task blocks for M9**
  - after: M6.37
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M9 task block from M9's section and the spec, with waves and the `M9.R`
    review (owner: maintainer); a block that can port Craft-app code names the source files in its
    scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ✅ **M7.25 `fs` + `store`: a metadata write keeps the file's time even across a crash**
  - after: M6.R
  - files: `crates/fs/src/atomic.rs`, `crates/fs/src/times.rs`, `crates/fs/tests/times.rs`, `crates/fs/tests/atomic.rs`, `crates/store/src/metawrite/file.rs`, `crates/store/tests/metawrite.rs`
  - scope: found at M6.R (M6.30's open item): the queue puts the photo's old modification time back
    with `set_times` after the rename, so a process crash between the two leaves the new bytes (never
    a torn file) with the write's time. Set the times on the temporary file before the rename
    (`fs::atomic` gains a "times to keep" option with its fault point), keep the after-rename path
    only where a platform needs it.
  - done when: tests with fault injection: a crash at every point of a metadata write leaves the old
    file with its old time or the new file with the old time; M6.30's metawrite suite passes
    unchanged.
- ✅ **M7.26 `meta`: refusals named up front, and ImageMagick's IPTC profiles**
  - after: M6.R
  - files: `crates/meta/src/write/tiff.rs`, `crates/meta/src/container/png.rs`, `crates/meta/src/container/tiff.rs`, `crates/meta/src/iptc/read.rs`, `crates/meta/src/exif/mod.rs`, `crates/meta/src/iptc/mod.rs`, `crates/meta/tests/write_png_webp_tiff.rs`, `crates/meta/tests/read.rs`, `crates/image/fuzz/Cargo.lock`
  - scope: found by M6.34's corpus at M6.R: a TIFF place or date edit that cannot be written
    (EXIF holds the value, or no XMP mirror exists) fails as `Error::Verify` after writing; refuse
    it before any write, naming the field. ImageMagick writes IPTC into PNG and TIFF as a bare IIM
    stream inside a "Raw profile type 8bim" / tag 34377; it reads as a malformed Photoshop block, so
    writes to real ImageMagick files are refused: read (and, where the merge rules allow, write)
    that form. Also: the stale "arrives with M6.10" doc comments in `exif/mod.rs` and `iptc/mod.rs`,
    and `crates/image/fuzz/Cargo.lock` missing M6.41's direct jxl dependencies.
  - done when: `cargo test -p dalica-meta`: a synthetic ImageMagick-style PNG and TIFF read their
    IPTC and take a caption write that verifies; the TIFF place/date cases are refused before any
    byte is written, naming the field; `cargo xtask test-corpus -p dalica-meta` with the metadata
    floors raised to the new measured counts.
- ✅ **M7.27 `ui` + `ui-kit`: caution toasts that show the whole message**
  - after: M6.R
  - files: `crates/ui-kit/src/widgets/feedback.rs`, `crates/ui-kit/tests/feedback.rs`, `crates/ui/src/notices.rs`, `crates/ui/tests/notices.rs`, `crates/ui/src/headless.rs`, `crates/ui/tests/fingerprints/toast*.json`
  - scope: found at M6.R: metadata write failures use the Error toast because ui-kit has no caution
    kind (the spec says caution), and the toast cuts its message short at both window sizes
    ("Could not write the caption into IMG_0041.jpg: the file is read-only; the folder…"). Add the
    caution kind; wrap the message to at most three lines and elide in the middle only past that
    (the file name stays whole); headless `ui.settle` waits for pending metadata writes (M6.36
    drove `ui.wait` instead).
  - done when: tests: a read-only write failure shows one caution toast with the whole message at
    1400×900 and 900×600 (fingerprints); `ui.settle` returns only after a queued metadata write has
    landed.
- ✅ **M7.28 tests: the Windows "Access is denied" flake in accept M5**
  - after: M6.R
  - files: `tests/acceptance/m5_folders.rs`
  - scope: seen on M5.20 (`m5_folders.rs:305`) and M6.6 (`:65`), Windows only: a file operation in
    the folder scenarios fails with os error 5 while another handle (the watcher, a thumbnail read,
    an indexer) still holds the file. Find the root cause as M6.36 did for the accept M1 flake (pin
    to few CPUs, reason from the code); fix it without a retry loop or a looser check. Any product
    file the cause lies in may be added to this block's `files` on its issue first.
  - done when: the cause is named in the commit body, the scenario holds its handles
    deterministically, and the M5 acceptance passes 20 runs pinned to 2 CPUs.
- ✅ **M7.29 Docs and housekeeping found at M6.R**
  - after: M6.R
  - files: `.gitignore`, `docs/security.md`, `docs/data-model.md`, `docs/control-protocol.md`
  - scope: `dist/` (the packaging scripts' output) into `.gitignore`; `security.md` §4.3's
    "Planned (M5)" and §7.1's sidecar and store fuzz targets marked "M1" that do not exist;
    `data-model.md` §4's library path (`…/dalica/library`, `…/Dalica/Library` on macOS and
    Windows); `control-protocol.md` §8 and `dalica-cli snapshot --demo` brought into agreement.
  - done when: `cargo xtask ci` passes and each item matches the code.
- ✅ **M7.30 `engine`: undoing Adjust Date after its write has landed**
  - after: M6.R, M7.27
  - files: `crates/engine/src/cmd/dates.rs`, `crates/engine/tests/dates_undo_written.rs`, `crates/ui/tests/adjust_date.rs`, `apps/dalica/src/control/play.rs`
  - scope: found mid-M7 by M7.27: once Adjust Date's metadata write has landed in the file, Undo
    leaves the photo at the written date instead of the date it had (the file's EXIF now holds the
    new date, so dropping the override falls back to it). Undo and Redo of a date step put the
    file's date back through the same metadata write-back (queued, merged, verified, the file's
    time kept), so the library and the file agree after undo whether or not the write had landed;
    `adjust_date.rs` waits for metadata writes (`settle` with `jobs`) and checks both orders. The
    windowed app's `ui.settle` (`control/play.rs`) counts queued metadata writes the way M7.27
    made the headless driver do.
  - done when: a test that fails on main: adjust a date, let the write land, undo → the library
    and the file both read the original date; redo → both read the new one; the same with the
    write still queued; `cargo xtask ci` passes.
- ✅ **M7.31 `meta` + docs: refusals as their own error, and the API notes found mid-M7**
  - after: M7.5, M7.6, M7.26, M7.27
  - files: `crates/meta/src/lib.rs`, `crates/meta/src/write/png.rs`, `crates/meta/tests/corpus.rs`, `crates/meta/tests/write_png_webp_tiff.rs`, `crates/meta/README.md`, `docs/xmp-interop.md`, `crates/ui-kit/README.md`, `docs/design-system.md`
  - scope: found mid-M7 by M7.26, M7.5, M7.6 and M7.27: a write refused up front (TIFF place or
    date with no mirror) returns its own `Error::Refused` naming the field, so its notice no longer
    starts "the written file failed its check:" and the corpus counts refusals by that variant; a
    PNG with a second IPTC-holding profile has every copy rewritten, so no old caption lingers;
    `xmp-interop.md` and the `meta` README describe the bare IIM form ImageMagick writes; the
    `ui-kit` README's API list names `EditIcon` / `paint_edit`, the tabs, tool buttons,
    histogram, Tuning slider, input helpers and `ToastText`; `design-system.md` §6's toast row
    names the caution kind.
  - done when: a refusal test matches `Error::Refused` with the field's name; a two-profile PNG
    caption write leaves no old caption; `cargo xtask test-corpus -p dalica-meta` keeps its
    counts; `cargo xtask ci` passes.
- ✅ **M7.R Release 0.2 gate**
  - after: M7.23, M7.30, M7.31
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
         (M7.25–M7.29 from M6.R, any time: fs times, meta refusals + IPTC profiles, caution
          toasts, the accept M5 Windows flake, docs and housekeeping)
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

#### M8 tasks

M8 lets edits leave Dalica: Save writes them into the photo and keeps the original in the folder's
`.dalica-originals/`, Undo Save and Revert bring the original back, Save a Copy and Save As write
new files, and Export renders a tray, a folder or an album into a new folder in the background. Its
first wave lists the M7 work each task extends and starts once the maintainer has closed M7.R. Five
tasks lay the ground the others share, so later tasks never share a file: M8.1 adds M8's fields,
ops and queries to the library model (a photo's saved record and its saved-beside copy, a folder's
Exported mark, the unsaved-edit counts), M8.2 gives `fs` the originals backup and new files made
without ever replacing one, M8.3 registers one engine command module per area, one runner for every
file-writing job and M8's settings, M8.4 adds M8's view state, the UI commands that open the Export
dialog and the Save As picker, and stub dialogs, and M8.5 gives `ui-kit` the Export dialog's
widgets and the Save and project icons. M8.6–M8.8 (the encoders, metadata carried to a new file,
lossless JPEG transforms) start beside them, since no other task touches their files. `create`
holds the one output pipeline (M8.10) that Save, Save a Copy, Save As and Export all use, so each
output has one implementation for the app, the CLI and MCP. Each task updates its `parity.md` rows
(`EDIT-SAVE`, `DATA-ORIGINALS`, `OUT-EXPORT`, `LIB-LT-HEADER` for the edited count and Save,
`LIB-ACTIONS` for the Export button, `LIB-TREE-GROUPS` for Projects, the File menu's row, the
Export half of the `PREF-…` row; new rows where none fit: `EDIT-SAVE-LOSSLESS` for rotate-only
saves, `DATA-SAVE-RECOVERY` for the repair of an interrupted Save) and names its commands there;
first fingerprints of encoders, the watermark, dialogs, the header and icons are approved in bulk
on the M8.R review page, and with them the look of the watermark.

Whole-window baselines belong to no task, as in M5–M7: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m7-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.

Decisions taken while planning (the check-in lists them for reversal): `output.export` and
`photo.saveAs` are UI commands with the menu ids (they open the Export dialog and the save picker)
and call the engine's `export.run` and `photo.saveTo`, the way `folders.add` calls `folders.set`;
`photo.save`, `photo.undoSave`, `photo.revert` and `photo.saveCopy` are engine commands whose
questions go through the dialog host (`control-protocol.md` §5.5). Save is not a step of the library
undo stack: its way back is Undo Save, which the edit panel's Undo button names ("Undo Save") once
the edits made after the save are undone. Saving a photo that was saved before renders the recorded
recipe followed by the new edits from the kept original and records the whole recipe, so quality
loss never compounds and Undo Save brings every edit back as unsaved (in front of any edits made
after the save). Undo Save and Revert move the kept original back over the saved file (the saved
file can always be rendered again, so it is not sent to the trash). A Save is journaled: its record
enters `.dalica-originals/.dalica.json` as pending before anything moves and is marked done last,
and a pending record found at open or scan is repaired. Save refuses a photo whose recipe holds an
operation Dalica cannot render (unknown or `legacy:`), with the reason, so baking pixels never drops
one. A format Dalica cannot write in place gets a JPEG beside it that is a new, unedited photo; the
source keeps its recipe and records `savedAs` (the copy's name and the recipe it holds), so it no
longer counts as unsaved until its recipe changes; Undo Save and Revert apply to in-place saves
only. Rotate-only saves of JPEGs are lossless through our own DCT-domain transforms written from
ITU-T T.81 (no crate offers them and neither Craft app has them), for baseline files whose
dimensions allow; progressive, arithmetic-coded and 12-bit files and other sizes take the EXIF-flag
fallback. Quality matching estimates the original's quality from its quantisation tables (ported
from PhotoCraft), 90 when unknown, and keeps its chroma subsampling (4:2:2 is written as 4:4:4,
never less chroma); Export's Automatic uses the same estimate. JPEGs are encoded by LightCraft's own
parallel baseline encoder, ported, so no new dependency (the alternative, the `jpeg-encoder` crate,
adds one). A new file carries the source's EXIF, XMP and IPTC with orientation 1, the new pixel size
and a fresh EXIF thumbnail (Save) or none (Export); then Save in place merges the library's owned
fields under the write-back options as M6's write-back would, while copies and exports carry them
all; output is sRGB, with an sRGB profile embedded only when the source had a profile. "Remove
location" removes GPS coordinates (the EXIF GPS IFD and `exif:GPS*` in XMP) and keeps place names.
Numbered export names are `001-<name>`; an export folder whose name is taken becomes
`<name>-001`; videos are skipped and counted until M15 decodes them (the videos row stays
disabled). An export marks its folder `"project": "exported"` in the folder's sidecar, and the tree
lists those folders and the folders under the projects directory (`Pictures/Dalica`) in Projects,
so an export made elsewhere is listed too and the mark survives moves and rebuilds. The watermark is
drawn with Inter SemiBold from `assets/fonts/` through `ab_glyph`, ported from LightCraft (M8.11
found it was not in the tree after all — egui 0.36 draws text with skrifa — and added it, audited:
pure Rust, Apache-2.0 / MIT). The save and export budgets join `development.md` §8 at 1.5 × their
first measurement. No step of M8 needs a maintainer decision mid-run.

- ✅ **M8.1 `library`: M8's model fields, ops and queries**
  - after: M6.2, M7.3, M7.R
  - files: `crates/library/src/model.rs`, `crates/library/src/op.rs`, `crates/library/src/query.rs`, `crates/library/src/tree.rs`, `crates/library/tests/m8_fields.rs`, `crates/library/tests/ops.rs`, `crates/library/tests/tree.rs`, `crates/library/README.md`
  - scope: `data-model.md` §6 as pure model: each photo gains a saved record (the recipe saved,
    the original's size, modification time, pixel size and digest, pending or done; read from
    `.dalica-originals/.dalica.json`, so index-only like the sketch: no undo stack, no revision
    bump on load) and a `savedAs` copy (the JPEG written beside it and the recipe it holds); each
    folder gains a project kind (`exported`). One batched op per change with an exact inverse
    (record set and cleared, `savedAs` set, the recipe and redo list cleared or restored by a
    Save, Undo Save or Revert, the Exported mark); queries cached per revision: whether a photo has
    unsaved edits (a recipe that is not empty and not the one `savedAs` holds), its folder's count
    of them, whether Revert and Undo Save apply (a done in-place record), whether the recipe holds
    an operation that cannot be rendered (from `edit`'s parse); the tree puts folders with the
    Exported mark, wherever they are, in Projects with the folders under `projects_dir`.
  - done when: `cargo test -p dalica-library`: random op sequences with the new fields applied
    then inverted restore the model exactly (proptest); the unsaved count of a folder follows
    edits, a Save, an Undo Save and a recipe change after a beside save; a folder with the mark
    outside `projects_dir` lists in Projects in flat mode and under its parent in tree mode;
    loading a record bumps no revision.
- ✅ **M8.2 `fs`: the originals backup and new files that never replace one**
  - after: M5.3, M7.R
  - files: `crates/fs/src/originals.rs`, `crates/fs/src/create.rs`, `crates/fs/src/faults.rs`, `crates/fs/tests/originals.rs`, `crates/fs/tests/create.rs`, `crates/fs/src/dirfd.rs`, `crates/fs/README.md`, `docs/security.md`
  - scope: `security.md` §5.1–§5.6 and `data-model.md` §6: `originals.rs` opens or creates
    `<folder>/.dalica-originals/` through the held folder handle (M5.3) and refuses a link; moves a
    photo into it by rename after re-checking the photo's identity right before (§5.3); refuses
    when a backup of that name exists and does not match the expected size and digest; moves a
    backup back over the saved file only when the saved file's identity is the one recorded
    (`Expect::Unchanged`); `create.rs` writes bytes to a new file under the first free name of
    `core::names::candidates` (temp file, fsync, a commit that never replaces: `renameat2`
    NOREPLACE on Linux, a hard link then unlink elsewhere, exclusive creation on volumes without
    links; the choice per platform in the commit body), verifies it and returns the name; and
    makes a new folder under the first free name (an export folder); fault points for every step;
    `security.md` §5.3–§5.4 statuses updated.
  - done when: `cargo test -p dalica-fs --test originals --test create`: a move into the backup and
    back gives identical bytes and the same name; a changed photo, a link in place of
    `.dalica-originals/` and a mismatched existing backup are refused with nothing moved; with
    `taken.jpg` and `taken-001.jpg` present a new file lands at `taken-002.jpg` and never replaces
    either; two threads racing for one name get two names; every fault point as Error and as Crash
    leaves the source and any existing file intact (`--features faults`); hostile names are refused.
- ✅ **M8.3 `engine`: M8's command modules, the file-job runner and settings**
  - after: M5.5, M7.3, M7.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/save.rs`, `crates/engine/src/cmd/save_copy.rs`, `crates/engine/src/cmd/export.rs`, `crates/engine/src/filejobs.rs`, `crates/engine/src/save_recovery.rs`, `crates/engine/src/jobs.rs`, `crates/engine/src/session.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/filejobs.rs`, `crates/engine/tests/settings_m8.rs`, `crates/engine/README.md`, `docs/architecture.md`
  - scope: the engine's ground for M8 so M8.15–M8.18, M8.23 and M8.24 never share a file: empty
    `cmd/save.rs`, `save_copy.rs` and `export.rs` joined into the registry once; `filejobs.rs`, one
    runner for Save, Save a Copy, Save As and Export (`architecture.md` §5; LightCraft
    `crates/ui-egui/src/export_task.rs` and `crates/engine/src/export.rs` `run_batch_with` at
    25ef65c8 as the pattern): owned inputs per item, a job with its label, done-of-total progress
    and Cancel checked between items, items on up to `workers` threads, results back through
    `poll()`; results applied by the caller's function as ops; a replaced file refreshed through
    the scan's change path for that one file (keeping its id and sidecar entry, marked as Dalica's
    own write so the watcher of M5.17 does not import it), a new file added as a photo, a new
    folder added and scanned; a session event per finished job (message, failures with file and
    reason, the folder to show) for the UI's notices; a stub `save_recovery.rs` called at session
    open and when a scan reads a pending record; `JobKind::Save` and `Export`; every M8 setting
    with its default behind a typed accessor: the first-time Save explanation (on), the export
    location (`<Pictures>/Dalica/Exports`), size (resize to 1600), quality (Automatic; Custom 75),
    numbers (off), watermark (off, empty text), remove location (off), the last Save As folder and
    format (JPEG); the projects directory (the export location's parent) for the tree.
  - done when: `crates/engine/tests/filejobs.rs` with a fake item worker: progress counts, Cancel
    between items finishes the current one and starts no other, results applied once in order, a
    failing item reported with its reason while the rest go on, a new file appears as a photo and
    a replaced one keeps its id, no `fs` call on the owner thread; `crates/engine/tests/settings_m8.rs`
    round-trips every new setting with unknown keys kept and wrong types falling back (reported);
    every existing engine test and `panic_hunt` pass unchanged.
- ✅ **M8.4 `ui`: M8's view state, UI commands, the save picker and stub dialogs**
  - after: M5.14, M7.4, M7.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/picker.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/save.rs`, `crates/ui/src/commands/save_as.rs`, `crates/ui/src/commands/export.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/confirm.rs`, `crates/ui/src/dialogs/save_explain.rs`, `crates/ui/src/dialogs/revert.rs`, `crates/ui/src/dialogs/export.rs`, `crates/ui/src/dialogs/export_progress.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state_m8.rs`, `docs/control-protocol.md`
  - scope: every M8 field of `UiState` in one go: the Export dialog's draft (location, folder name,
    numbers, original size or resize with its pixels, quality preset and custom value, videos,
    watermark on and its text), the Save explanation's "Don't show this again", the Revert
    question's photos, the export progress window (its job, shown or hidden); UI commands
    `output.export` (Ctrl+Shift+S; "tray not empty") and `photo.saveAs` (one photo with edits)
    in `menus.rs` with stub handlers in `commands/export.rs` and `commands/save_as.rs`, and
    `commands/save.rs` stub routing for Save, Undo Save and Revert, so M8.19–M8.22 never share a
    file; stub `save_explain`, `revert`, `export` and `export_progress` dialogs on the dialog
    host, the first two asked through `confirm.rs` for `photo.save` and `photo.revert`; the
    `Picker` gains a save dialog (suggested name, the JPEG and PNG filters; `rfd` in the app, the
    scripted picker in tests, `filePicker {kind: "save"}` over control, answered by
    `ui.filePicker {path}`); the path parameters of `export.run` and `photo.saveTo` checked by the
    automation root policy (`control-protocol.md` §5.7); control-protocol §5.5, §5.6, §6.1 (its
    example becomes `export.run`) and §6.5 updated.
  - done when: `crates/ui/tests/view_state_m8.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); Ctrl+Shift+S with a tray opens
    `dialog:export` and Esc closes it; File ▸ Save As… opens the save picker and
    `ui.filePicker {cancel: true}` changes nothing; an `export.run` path outside every write root
    is `path_denied`; the menu items light up; every command's graceful-failure test; `panic_hunt`
    and `widget_ids` stay green.
- ✅ **M8.5 `ui-kit`: the Export dialog's widgets and the Save and project icons**
  - after: M5.25, M7.5, M7.6, M7.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/form.rs`, `crates/ui-kit/src/widgets/slider.rs`, `crates/ui-kit/src/widgets/dropdown.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/output.rs`, `crates/ui-kit/tests/form_widgets.rs`, `crates/ui-kit/tests/icons_output.rs`, `crates/ui-kit/tests/fingerprints/form-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-output*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `creating.md` §2 and `design-system.md` §5–§6: `form` rows (labels right-aligned in one
    column, controls left-aligned in the next, a hint line under a control, a radio row holding a
    field and a slider, rows disabled as a group); the slider's stops variant (snaps near the
    given stops, a linked number field beside it, the stops drawn as ticks); dropdown items with a
    description line ("Automatic — Keeps the original's quality") and a checked mark; the icons
    `Save` (a disk, replacing the one `panels/light_table/header.rs` draws for itself) and
    `Project` (the tree's project row), our own drawings on the 20-unit grid; new `Role`s where
    needed; the widget gallery gains them.
  - done when: `crates/ui-kit/tests/form_widgets.rs`: label columns align at both sizes; a drag
    near 1600 snaps to it and typing 1700 holds 1700; a disabled group ignores clicks; dropdown
    descriptions and the checked item; `crates/ui-kit/tests/icons_output.rs`: both icons stay in
    their grid at 12, 16, 20 and 28 px in rest, hover and disabled colours; gallery and icon
    fingerprints; `widget_ids`, `cargo xtask assets` and `cargo xtask names` pass.
- ✅ **M8.6 `image`: JPEG and PNG encoders and the JPEG quality estimate**
  - after: M5.1, M7.R
  - files: `crates/image/src/encode.rs`, `crates/image/src/encode/**`, `crates/image/src/jpeg_quality.rs`, `crates/image/tests/encode.rs`, `crates/image/tests/jpeg_quality.rs`, `crates/image/tests/fingerprints/encode*.json`, `crates/image/README.md`, `NOTICE`
  - scope: `formats.md` §1 and `data-model.md` §6: `encode::jpeg(image, quality, subsampling,
    segments)` a baseline JPEG (grey, 4:4:4, 4:2:0; restart-interval bands encoded on scoped
    threads when asked, else on the caller's) ported from LightCraft
    `crates/codecs/src/jpeg_par.rs` and the APP segment writer `jpeg_app_segments` of
    `crates/codecs/src/encode.rs` at 25ef65c8 (EXIF, XMP and ICC segments with their size limits);
    `encode::png(image, chunks)` 8- and 16-bit grey, RGB and RGBA through `png` with `eXIf` and
    `iTXt` chunks; `jpeg_quality.rs` the quality a JPEG was saved at from its quantisation tables
    and its chroma subsampling from the frame header, ported from PhotoCraft
    `crates/codecs/src/source_settings.rs` (`jpeg_quality`) at bb17dd55 with its tests from
    `crates/codecs/tests/source_settings.rs`; dimensions, sizes and hostile tables bounded; headers
    and `NOTICE` rows (`architecture.md` §9).
  - done when: `cargo test -p dalica-image --test encode --test jpeg_quality`: encoded files decode
    through our own decoder within a stated error per quality, sizes fall as quality falls,
    parallel and single-threaded output decode identically, EXIF/XMP/ICC segments read back, an
    oversized segment is `Err`; 16-bit PNG keeps 16 bits and alpha; scenes JPEGs saved at 50, 75,
    90 and 95 are estimated within 2 and the subsampling is read; truncated and random headers give
    `None`, never a panic; fingerprints; the time to encode 24 MP printed.
- ✅ **M8.7 `meta`: metadata carried to a new file**
  - after: M6.10, M6.17, M6.18, M7.R
  - files: `crates/meta/src/carry.rs`, `crates/meta/src/carry/**`, `crates/meta/tests/carry.rs`
  - scope: `data-model.md` §6 and `creating.md` §2: from a source file's bytes in any container
    `meta` reads, the metadata of a rendered copy for a JPEG or PNG target: the EXIF block with
    Orientation 1, the new pixel size and colour space, its thumbnail IFD replaced by a given
    thumbnail or dropped (edited in place through M6.10's EXIF editor, so maker notes keep their
    offsets); the XMP packet with `tiff:Orientation` 1 and the new size, every other property
    kept; IPTC kept; with "remove location", the GPS IFD and every `exif:GPS*` property removed
    (place names kept); then `write_owned` (M6.4) merges the owned fields the caller passes; and
    `set_orientation(bytes, o)` for the lossless fallback (an EXIF block added when the file has
    none), checked by M6.4's verification.
  - done when: `cargo test -p dalica-meta --test carry`: a JPEG, a TIFF and a WebP source each give
    a JPEG and a PNG target whose EXIF, XMP and IPTC read back with orientation 1 and the new size,
    foreign properties byte-identical; maker notes still parse; GPS gone from EXIF and XMP when
    asked and kept otherwise; the owned fields merged; `set_orientation` changes only that tag's
    value; truncated and hostile blocks are `Err`, never a panic.
- ✅ **M8.8 `image`: lossless JPEG rotation and flips**
  - after: M5.1, M7.R
  - files: `crates/image/src/jpeg_transform.rs`, `crates/image/src/jpeg_transform/**`, `crates/image/tests/jpeg_transform.rs`
  - scope: `data-model.md` §6, our own from ITU-T T.81 (a provenance header naming its sections):
    for baseline sequential Huffman JPEGs (8-bit, any sampling), the quantised coefficients read
    under the decode limits, one of the eight dihedral transforms applied in the DCT domain
    (transposed blocks, odd-index coefficients negated for flips, blocks and MCUs moved, sampling
    factors and component sizes swapped for transposes), written back with the file's own
    quantisation tables (no requantisation) and Huffman tables built for the new data, every other
    segment copied byte for byte; answers "not lossless here" when a transform would move a
    partial MCU edge to the leading edge, and for progressive, arithmetic, 12-bit, lossless or
    hierarchical files.
  - done when: `cargo test -p dalica-image --test jpeg_transform`: each of the eight transforms on
    scenes JPEGs at 4:4:4, 4:2:0, 4:2:2 and grey at MCU-multiple sizes decodes to the transformed
    pixels of the original within 1 level and reads back the transformed coefficients exactly;
    four quarter turns give the original coefficients; a partial-MCU size and a progressive file
    answer "not lossless here"; truncation at every offset and proptest random bytes never panic;
    the time for 24 MP printed.
- ✅ **M8.9 `sidecar` + `store` + `scan`: the originals record and M8's fields on disk**
  - after: M6.8, M8.1
  - files: `crates/sidecar/src/originals.rs`, `crates/sidecar/src/fields.rs`, `crates/sidecar/src/print.rs`, `crates/sidecar/src/lib.rs`, `crates/sidecar/tests/originals.rs`, `crates/store/src/mapping.rs`, `crates/store/src/rebuild.rs`, `crates/store/tests/m8_fields.rs`, `crates/scan/src/originals.rs`, `crates/scan/tests/originals.rs`, `docs/data-model.md`
  - scope: `data-model.md` §2 and §6: `.dalica-originals/.dalica.json` parsed and printed (per
    original name: the recipe saved, size, modification time, pixel size, digest, `pending` or
    `done`, the time of the save; unknown keys kept; names used only when they are one plain path
    component, `security.md` §5.4); the photo's `savedAs` and the folder's `"project": "exported"`
    in `.dalica.json` in the documented key order; the scan reads a folder's originals record on
    its worker (the walk still skips the directory) and turns it into M8.1's ops; a rebuild reads
    it back; the mapping test lists every new field; `data-model.md` §2–§3 and §6 document both
    files.
  - done when: `data-model.md` §10.1 for every M8 field (save, mark, delete the index, rebuild,
    compare); proptest round trips of the originals record with hostile names and wrong types never
    panic and keep what they cannot use; a record naming `../x` or `CON` is kept but never reaches a
    file; a damaged record is set aside as `.corrupt-<time>` and reported; journal replay and
    snapshots keep the new fields.
- ✅ **M8.10 `create`: the one output pipeline**
  - after: M7.2, M8.6, M8.7
  - files: `crates/create/src/lib.rs`, `crates/create/src/output.rs`, `crates/create/src/output/**`, `crates/create/src/watermark.rs`, `crates/create/tests/output.rs`, `crates/create/tests/common/**`, `crates/create/tests/fingerprints/output*.json`, `crates/create/README.md`
  - scope: `architecture.md` §6 and `data-model.md` §6: `output::render(source bytes, recipe,
    spec)` used by Save, Save a Copy, Save As and Export: decode at full size under the decode
    limits with EXIF orientation applied (M5.1), `edit::render` of the recipe at full size (M7.2),
    resize to a long side that never upscales (`image::resize`), the watermark hook (a stub in
    `watermark.rs` that draws nothing, for M8.11), encode through M8.6 (JPEG at `Quality::Match`
    from the source's estimate or 90, or a fixed quality, with the matched subsampling; PNG keeping
    16 bits and alpha), metadata through M8.7 with the owned fields the caller passes; the target
    format per `formats.md` §1 (`target(kind, alpha, purpose)`: in place, beside, copy or export);
    a report (format, size, quality used); pure: bytes in, bytes out; `tests/common/` with the
    scenes and decode helpers the `create` tests share.
  - done when: `cargo test -p dalica-create --test output`: an empty recipe at Match re-encodes a
    q75 scenes JPEG estimated at 75 ± 2 within a stated error; a rotated recipe gives orientation 1
    and the rotated pixel size; a resize hits the long side exactly and never upscales; a PNG with
    alpha stays PNG with alpha and a 16-bit one 16-bit; EXIF, XMP and IPTC carried with the owned
    fields; GPS removed when asked; the decoded output equals `edit::render` (fingerprints); an
    undecodable source and hostile sizes are `Err`, never a panic; nothing is written to disk.
- ✅ **M8.11 `create`: the export watermark**
  - after: M8.10
  - files: `crates/create/src/watermark.rs`, `crates/create/tests/watermark.rs`, `crates/create/tests/fingerprints/watermark*.json`, `NOTICE`
  - scope: `creating.md` §2: white text with a soft shadow, bottom-right, 3 % of the long side
    high, margins relative to the image so every size looks the same; ported from LightCraft
    `crates/engine/src/export.rs` (`Watermark`, `draw_watermark`, `blend_watermark`) at 25ef65c8,
    text only, horizontal, with Inter SemiBold from `assets/fonts/` through `ab_glyph` (audited
    with `cargo xtask deps`); the text cleaned (control and bidirectional characters removed,
    at most 200 characters) and shrunk to fit inside the image; header and `NOTICE` row
    (`architecture.md` §9).
  - done when: `cargo test -p dalica-create --test watermark`: the text height is 3 % of the long
    side at three sizes and two sizes agree after downscale; it stays inside the bottom-right
    margins; empty text draws nothing; very long text shrinks to fit; a character without a glyph
    draws the font's fallback without a panic; fingerprints on a light and a dark scene.
- ✅ **M8.12 `create`: Save in place, Undo Save and Revert**
  - after: M8.2, M8.9, M8.10
  - files: `crates/create/src/save.rs`, `crates/create/src/save/**`, `crates/create/tests/save.rs`, `docs/data-model.md`
  - scope: `data-model.md` §6, §10.9 and `security.md` §5.3: `save::in_place(folder, name,
    recipe, owned fields, options)`: refuse a recipe with an operation Dalica cannot render; read
    the photo and its identity; render through M8.10 at Match quality, from the kept original and
    the recorded recipe plus the new edits when the photo was saved before; write the record as
    pending (M8.9's format, through `fs`'s atomic replace); move the original into
    `.dalica-originals/` only when no backup exists (M8.2), else replace the saved file under its
    recorded identity; commit the new file at the photo's name without replacing anything else;
    verify it (pixel size, the owned fields); mark the record done; `save::undo` and
    `save::revert` move the kept original back over the saved file under its recorded identity and
    drop the record, returning the recipe to put back (Undo Save) or none (Revert); the dispatch
    for M8.14 (`lossless.rs`, `beside.rs` and `copy.rs` stubs); `data-model.md` §6 gets the step
    order and the record's states.
  - done when: `cargo test -p dalica-create --test save` on temp folders: `data-model.md` §10.9
    (Save → Undo Save → Save → Revert returns the exact original bytes); a second Save keeps the
    first original and renders from it; the saved file's quality is the original's ± 2, its
    orientation 1 and its metadata carried; a photo changed since it was read, a
    `.dalica-originals` link and a mismatched backup are refused with nothing moved; a read-only
    folder fails with its reason and nothing changed; an unknown operation is refused by name.
- ✅ **M8.13 `create`: export planning and files**
  - after: M8.2, M8.10
  - files: `crates/create/src/export.rs`, `crates/create/src/export/**`, `crates/create/tests/export.rs`, `crates/create/tests/fingerprints/export*.json`, `crates/core/src/names.rs`, `crates/core/tests/sanitize.rs`, `docs/creating.md`, `docs/security.md`
  - scope: `creating.md` §2 and `security.md` §5.4: `ExportOptions` (original size or a long side
    100–4000; Automatic (the source's estimate, else 90), Normal 85, Maximum 95, Minimum 60, Custom
    1–100; numbers; watermark text; remove location); the one name sanitizer in
    `core::names::sanitize` (separators, `: * ? " < > |`, control and bidirectional characters
    removed; leading `.` and `-` and trailing dots and spaces stripped; reserved device names
    replaced; 255 bytes on a character boundary; a fallback such as `Export 2026-06-04`); the plan:
    the export folder made under the location with a free name (M8.2), each photo's name (its stem,
    `.jpg`, or `.png` for a source with transparency; `001-` numbers in tray order when asked;
    collisions `-001` through exclusive creation), the video and undecodable items listed as
    skipped with their reason; `export_one(item, plan)` through M8.10 and the watermark hook; a
    cancel removes the file being written, so only complete files remain; `creating.md` §2 and
    `security.md` §5.4's status updated.
  - done when: `cargo test -p dalica-create --test export` and `cargo test -p dalica-core --test
    sanitize`: exported files decoded and checked (long side, never upscaled, the quality estimate
    within each preset's bounds, metadata copied, GPS removed only when asked); collisions get
    `-001` and numbers keep the order; `CON`, `../x`, bidirectional text, a 300-byte name and an
    empty name sanitize as specified (proptest: never empty, never a separator); a cancel during a
    file leaves only complete, decodable files; a PNG with alpha exports as PNG.
- ✅ **M8.14 `create`: lossless rotation, the orientation fallback, JPEG beside and copies**
  - after: M8.8, M8.12
  - files: `crates/create/src/save/lossless.rs`, `crates/create/src/save/beside.rs`, `crates/create/src/save/copy.rs`, `crates/create/tests/save_lossless.rs`, `crates/create/tests/save_beside.rs`
  - scope: `data-model.md` §6 and `formats.md` §1: a JPEG whose recipe holds only rotations and
    flips saves through M8.8 with the net transform of the recipe and the EXIF orientation (M6.3),
    orientation set to 1 through M8.7 and every other segment unchanged; when M8.8 answers "not
    lossless here", the pixels stay and the orientation tag becomes the composed transform (an
    EXIF block added when there is none); both keep the original in `.dalica-originals/` through
    M8.12's steps; `beside`: a format Dalica cannot write in place gets `name.jpg` (or the next
    free name) beside it through M8.2, the source untouched, answering the new name and the recipe
    it holds; `copy`: `<stem>-001.jpg` (the next free number) in the photo's folder, or a chosen
    path never replaced and a chosen format (JPEG or PNG) for Save As.
  - done when: `cargo test -p dalica-create --test save_lossless --test save_beside`: a rotated
    MCU-sized JPEG saves with the original's quantisation tables and decodes to the rotated
    original within 1 level; a partial-MCU size keeps its pixel data bytes and gets the composed
    orientation; a flip after a rotation; a PNG with alpha, a TIFF, a WebP and a raw each get a
    JPEG beside and stay byte-identical; copies take the next free number and Save As refuses an
    existing path.
- ✅ **M8.15 `engine`: Save, Undo Save and Revert**
  - after: M8.3, M8.9, M8.12
  - files: `crates/engine/src/cmd/save.rs`, `crates/engine/src/cmd/history.rs`, `crates/engine/tests/save.rs`
  - scope: `data-model.md` §6, `editing.md` §5.1 and `control-protocol.md` §5.5: `photo.save {ids,
    confirm?}` (Ctrl+S in the library and the edit view; enabled for photos with unsaved edits
    whose recipe can be rendered, the reason otherwise; `needs_confirm` with the first-time
    explanation while its setting is on, and `dontAsk` turns it off) as a file job through M8.3's
    runner ("Saving 3 photos", progress, Cancel between photos) through `create`'s save dispatch
    (M8.12), then ops: the record set and the recipe and redo list cleared, or for a beside save
    (live once M8.14 lands; M8.23 checks it per format) `savedAs` set and the copy added as a photo; the photo's entry refreshed and its thumbnails and sketch remade (M7.9);
    `photo.undoSave {ids}` (photos with a done in-place record: the original back, the recorded
    recipe in front of any newer edits as unsaved edits); `photo.revert {ids, confirm, action?}`
    (`needs_confirm` with the [Revert] [Undo Save] [Cancel] question; the original back, edits and
    record gone); `photo.history.state` answers "Undo Save" when the recipe is empty and a done
    record exists; offline folders and read-only files are disabled or fail with their reason.
  - done when: `crates/engine/tests/save.rs` on temp folders: `data-model.md` §10.9 through the
    commands; the explanation asked once and not after `dontAsk`; Save clears the badge and the
    folder's count, Undo Save brings both back and the "Undo Save" label goes; a re-save after a
    tune renders from the original; Cancel after the first of three photos saves only that one;
    the thumbnails show the saved pixels; the watcher imports nothing for Dalica's own write; the
    owner thread made no `fs` call; each command's contract and graceful failures; `panic_hunt`
    green.
- ✅ **M8.16 `engine`: the export command and job**
  - after: M8.3, M8.9, M8.11, M8.13
  - files: `crates/engine/src/cmd/export.rs`, `crates/engine/tests/export.rs`
  - scope: `creating.md` §2 and `data-model.md` §6: `export.defaults {ids? | section? | album?}`
    (query: the location, the suggested folder name (the section or album name, "Export
    2026-06-04" for a mixed tray), size, quality, numbers, watermark, remove location, whether
    videos are present); `export.run {ids | section | album, dir, folderName, numbers?, size?,
    quality?, watermark?, removeLocation?}` (Files, no undo step; missing options from the
    settings, the used ones remembered) as a file job ("Exporting 12 of 48…", progress, Cancel)
    through M8.3's runner with M8.13's plan; afterwards the folder gets the Exported mark, is added
    as Scan Always when it lies under no watched folder (M5.12's rules) and is scanned; the result
    names the folder and the written, skipped and failed files; the session event carries the
    folder to show; the photos' current recipes are rendered, so unsaved edits are exported.
  - done when: `crates/engine/tests/export.rs` on temp folders: an export of edited and unedited
    photos writes decodable files of the right size with the edits applied; Cancel leaves only
    complete files and the job reports cancelled; an export into an unwatched folder adds it as
    Scan Always and its photos appear; the folder lists in Projects and keeps the mark after a
    rebuild; a second export of the same name makes `<name>-001`; videos are skipped and counted;
    the owner thread made no `fs` call; each command's contract and graceful failures;
    `panic_hunt` green.
- ✅ **M8.17 `engine`: Save a Copy and Save As**
  - after: M8.3, M8.14
  - files: `crates/engine/src/cmd/save_copy.rs`, `crates/engine/tests/save_copy.rs`
  - scope: `data-model.md` §6: `photo.saveCopy {ids}` (photos with edits): `<stem>-001.jpg` (the
    next free number) in the photo's folder with the edits applied at Match quality, the photo and
    its edits untouched, the copy added as a new, unedited photo; `photo.saveTo {id, path,
    format?}` (Save As; JPEG or PNG by `format` or the extension, Match quality for JPEG): never
    replaces an existing file (`exists`), the folder must exist, a folder under no watched folder
    is added as Scan Always so the file appears, the last folder and format remembered; both as
    file jobs through M8.3's runner.
  - done when: `crates/engine/tests/save_copy.rs` on temp folders: two copies of one photo are
    `-001` and `-002` with its edits applied and the photo's recipe unchanged; Save As PNG into an
    unwatched folder adds it and the photo appears; an existing path is refused and unchanged; the
    owner thread made no `fs` call; each command's contract and graceful failures; `panic_hunt`
    green.
- ✅ **M8.18 `create` + `engine`: a Save interrupted at any step never loses the original**
  - after: M8.14, M8.15
  - files: `crates/create/src/save/recover.rs`, `crates/create/tests/save_faults.rs`, `crates/engine/src/save_recovery.rs`, `crates/engine/tests/save_recovery.rs`
  - scope: `data-model.md` §6 and §10.5, `architecture.md` §8: `save::recover(folder)` for a
    pending record: rolled forward when the new file is complete and verifies, else rolled back
    (the kept original moved back to its name, the record dropped); Save's temporary files removed
    by their shape (M5.3); the engine runs it on a worker at session open and when a scan reads a
    pending record, puts the recipe back when it rolled back, and reports the repair in the log and
    `library.inspect`; the fault points of M8.2 and of `fs`'s atomic replace armed at every step of
    Save, a second Save, Undo Save and Revert, as Error and as Crash.
  - done when: `cargo test -p dalica-create --features faults --test save_faults` (`create` gains a
    `faults` feature that turns on `fs`'s): for
    every step × {Error, Crash} × {Save, re-Save, Undo Save, Revert} the original's bytes exist at
    the photo's name or in `.dalica-originals/` with a record pointing at them, and after `recover`
    the folder is wholly saved or wholly original with its sidecars consistent;
    `crates/engine/tests/save_recovery.rs`: a session reopened after a planted crash repairs the
    folder and shows the photo with its edits; the owner thread made no `fs` call.
- ✅ **M8.19 `ui`: Save, Undo Save and Revert in the app**
  - after: M8.4, M8.5, M8.15
  - files: `crates/ui/src/commands/save.rs`, `crates/ui/src/dialogs/save_explain.rs`, `crates/ui/src/dialogs/revert.rs`, `crates/ui/src/panels/light_table/header.rs`, `crates/ui/src/panels/edit/history.rs`, `crates/ui/tests/save_ui.rs`, `crates/ui/tests/section_header.rs`, `crates/ui/tests/fingerprints/region-header-*.json`, `crates/ui/tests/fingerprints/dialog-save-explain-*.json`, `crates/ui/tests/fingerprints/dialog-revert-*.json`, `crates/ui/tests/fingerprints/edit-undo-save-*.json`
  - scope: `ui-layout.md` §5.2, `data-model.md` §6 and `editing.md` §5.1: the section header's
    "%d edited" and Save text button (top-right, only with unsaved edits; in the pinned header
    too) and the disk button with M8.5's `Save` icon run `photo.save` with the section's photos
    with unsaved edits; File ▸ Save and Ctrl+S save the selected photos in the library and the open
    photo in the edit view; the first-time explanation (our own words for §6 step 1, the "Don't
    show this again" checkbox) and the Revert question ([Revert] [Undo Save] [Cancel], Cancel the
    default) on the dialog host; File ▸ Revert…; the edit panel's Undo reads "Undo Save" from
    `photo.history.state` and runs `photo.undoSave`; ids per control-protocol §7.
  - done when: `crates/ui/tests/save_ui.rs` (headless, a temp library): two edited photos show "2
    edited" and Save; a click asks once, saves both, and the count and badges go; with the
    checkbox ticked the next Save asks nothing; Ctrl+S in the edit view saves the open photo; the
    Undo button reads "Undo Save" and brings the edits back; each Revert button does its action
    and Cancel nothing; header, dialog and panel fingerprints at both sizes.
- ✅ **M8.20 `ui`: the Export dialog**
  - after: M8.4, M8.5, M8.16
  - files: `crates/ui/src/dialogs/export.rs`, `crates/ui/src/commands/export.rs`, `crates/ui/src/panels/bottom.rs`, `crates/ui/tests/export_dialog.rs`, `crates/ui/tests/bottom.rs`, `crates/ui/tests/fingerprints/dialog-export-1400x900.json`, `crates/ui/tests/fingerprints/dialog-export-900x600.json`
  - scope: `creating.md` §2: "Export to Folder" on M8.5's form rows: Export location with Browse…
    (the folder picker), Name of exported folder, the numbers checkbox, Use original size or Resize
    to with the linked field and the slider (100–4000, stops at 480, 640, 800, 1024, 1280, 1600,
    2048, 3840), Image quality with the five presets and their descriptions (Custom enables its
    slider and reads "Custom (75)"), Export videos as (disabled: no video is exported until M15),
    the watermark checkbox enabling its field and its hint; filled from `export.defaults`; Export
    runs `export.run` with the tray, or with the section from its header (the selection when it
    lies inside the section, else the whole section) and closes, Cancel and Esc change nothing;
    opened by File ▸ Export Pictures to Folder… (Ctrl+Shift+S), the bottom bar's Export and the
    section header's Export; ids per control-protocol §7.
  - done when: `crates/ui/tests/export_dialog.rs` (headless, by id): each entry point opens it with
    the right folder name; field and slider stay in step and snap at the stops; Custom shows its
    value; a size of 0, 99999 or letters disables Export; Export starts an export job with the
    dialog's values and they are the next defaults; Cancel changes nothing; the bottom bar's
    Export is live with a tray; dialog fingerprints at both sizes.
- ✅ **M8.21 `ui`: export progress, file-job notices and the Exported group**
  - after: M8.4, M8.5, M8.16
  - files: `crates/ui/src/dialogs/export_progress.rs`, `crates/ui/src/notices.rs`, `crates/ui/src/panels/folders/model.rs`, `crates/ui/src/panels/folders/paint.rs`, `crates/ui/tests/export_progress.rs`, `crates/ui/tests/tree_projects.rs`, `crates/ui/tests/fingerprints/dialog-export-progress-*.json`, `crates/ui/tests/fingerprints/region-tree-projects-*.json`
  - scope: `creating.md` §2 and `ui-layout.md` §4: the small progress window ("Exporting 12 of
    48…", its bar, [Cancel] through `jobs.cancel`) while an export runs, beside the activity
    indicator's row; `notices.rs` turns M8.3's file-job events into toasts: "Exported 48 pictures"
    with [Show Folder] (until OS integration, M15, it scrolls the library to the export folder and
    selects it), a saved copy's name, skipped items with their count, a failed Save naming the file
    and the reason (`caution`); the tree's Projects group lists export folders with M8.5's
    `Project` icon, `projects_dir` taken from the engine.
  - done when: `crates/ui/tests/export_progress.rs` (headless): the window shows the count and
    closes when the job ends; Cancel stops the job and leaves only complete files; the toast's
    Show Folder scrolls to and selects the folder; a read-only file's failed Save gives one toast;
    `crates/ui/tests/tree_projects.rs`: an export folder outside every watched root appears in
    Projects in flat mode; window and tree fingerprints at both sizes.
- ✅ **M8.22 `ui`: Save As in the app**
  - after: M8.4, M8.17
  - files: `crates/ui/src/commands/save_as.rs`, `crates/ui/tests/save_as.rs`
  - scope: `data-model.md` §6 and `menus-and-shortcuts.md` §1: File ▸ Save As… opens the save
    picker with the photo's name, the last folder and format, then runs `photo.saveTo`; an
    existing path answers "Choose another name" and opens the picker again (never a replace); File
    ▸ Save a Copy runs `photo.saveCopy` on the selection or the open photo; both live in the
    library and the edit view.
  - done when: `crates/ui/tests/save_as.rs` (headless, the scripted picker): Save As to a new path
    writes it and the photo appears; an existing path is refused and the picker returns; cancel
    writes nothing; Save a Copy from the edit view writes `-001`; the menu items' enabled states.
- ✅ **M8.23 `engine`: every format saves as specified**
  - after: M8.14, M8.15
  - files: `crates/engine/tests/save_formats.rs`, `docs/formats.md`
  - scope: `formats.md` §1's "Save edits in place" column through `photo.save` on `scenes` files
    of every format: JPEG (baseline, progressive, CMYK, with and without EXIF) in place with its
    quality matched and metadata kept; PNG without alpha in place as PNG (8- and 16-bit); PNG
    with alpha, TIFF, WebP, GIF, BMP, JPEG XL, PSD, a raw and HEIC (with the `heif` feature) as a
    JPEG beside with the source byte-identical; AVIF and video disabled with their reason; a
    rotate-only save lossless on an MCU-sized baseline JPEG and by orientation flag otherwise;
    `formats.md` §1 brought in line with what was measured.
  - done when: `crates/engine/tests/save_formats.rs`: one case per row of the table, each checking
    the files on disk after the save and after Undo Save where it applies; `cargo test -p
    dalica-engine --features heif --test save_formats` runs the HEIC case.
- ✅ **M8.24 `engine`: a 200-photo mixed export and the save and export benchmark**
  - after: M8.11, M8.16
  - files: `crates/engine/tests/export_mixed.rs`, `crates/engine/benches/save_export.rs`, `xtask/src/bench.rs`, `xtask/src/accept.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: `creating.md` §2: a mixed tray of 200 `scenes` photos (JPEG in all eight orientations,
    CMYK, PNG with and without alpha, 16-bit TIFF, WebP, GIF, BMP, JPEG XL, PSD, raw previews,
    edited and unedited, some with GPS) exported at 1600 px Normal with a watermark and location
    removal; the `save_export` bench (registered in `xtask/src/bench.rs`): Save of a 24 MP JPEG
    (render, encode, write, verify), a lossless rotate-only Save of the same, and the 200-photo
    export; its budgets set at 1.5 × the first measurement in `development.md` §8 and joined to the
    release-budgets job (`BUDGETS`, M5.24).
  - done when: `crates/engine/tests/export_mixed.rs`: all 200 succeed and every file decodes with
    the right size, quality bounds, watermark region and no GPS; a Cancel half-way leaves only
    complete files; `cargo xtask bench save_export` prints the three numbers (in the PR) and
    `cargo xtask budgets` passes with them.
- ✅ **M8.25 `accept M8`**
  - after: M8.18, M8.19, M8.20, M8.21, M8.22, M8.23, M8.24
  - files: `tests/acceptance/m8.rs`, `tests/acceptance/m8_*.rs`, `tests/acceptance/m8.md`, `tests/acceptance/fingerprints/m8-*.json`
  - scope: every *Done when* of M8's two areas in the real `dalica --headless` over the control
    channel: Save → Undo Save → Save → Revert returning the exact original bytes (`data-model.md`
    §10.9, the shape of `mcp.md`'s W03); a planted crash during Save (the `faults` feature) and a
    restart that repairs the folder without losing the original; the per-format suite of M8.23;
    an export decoded and checked (size, quality bounds, metadata, GPS removed when asked); Cancel
    leaving only complete files; the 200-photo mixed export (the release bench and budgets with
    `DALICA_ACCEPT_RELEASE=1`; inside `cargo xtask ci` a smaller debug run checks the path and
    prints its times); fingerprints of the header with Save, the Export dialog and the progress
    window; the manual checks in `m8.md` (save one of your own photos and find the original in
    `.dalica-originals/`, Undo Save, Revert, a rotate-only save, Save As PNG, export a folder at
    1600 with a watermark and open the files in another application).
  - done when: `cargo xtask accept M8` passes everything in M8's *Done when*.
- ✅ **M8.26 Lead: task blocks for M10**
  - after: M7.24
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M10 task block from M10's section and the spec, with waves and the `M10.R`
    release gate (owner: maintainer); a block that can port Craft-app code names the source files
    in its scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ✅ **M8.27 `fs`: a real file identity on Windows**
  - after: M7.R
  - files: `crates/fs/src/atomic.rs`, `crates/fs/src/identity.rs`, `crates/fs/tests/identity.rs`, `crates/fs/tests/atomic.rs`, `crates/fs/Cargo.toml`, `docs/security.md`
  - scope: found at M7.R (red-main fix #372; maintainer, 2026-10-13: "a real file ID"): on Windows
    `Identity` holds the creation time where Unix holds the inode (std has no stable file index),
    and NTFS tunnelling hands a freed name's creation time to a file renamed onto it, so another
    program's same-size rewrite that keeps the modification time passes the read-before-write
    guard. Read the volume serial number and file index on Windows (`winapi-util`, already in
    `Cargo.lock`; a dependency audit per `security.md` §8, no `unsafe` in our code), taken where the
    identity is taken today (an open handle where one is needed); Unix unchanged. `security.md`
    says what the guard sees on each OS.
  - done when: a Windows test replaces a file with same-size bytes and the old modification time
    and the identity changes; the same-size assertion in `crates/fs/tests/atomic.rs` holds on every
    OS again; `cargo xtask deps` passes; CI green on the three OSes.
- ✅ **M8.28 `ui` + `engine`: the Tuning sliders show the photo's Tuning after later edits**
  - after: M7.R
  - files: `crates/ui/src/panels/edit/tuning.rs`, `crates/ui/src/panels/edit/fill_light.rs`, `crates/ui/tests/tuning_tab.rs`, `crates/engine/src/cmd/tuning.rs`, `crates/engine/src/history.rs`, `crates/engine/tests/tuning_commands.rs`, `docs/editing.md`
  - scope: found at M7.R (maintainer, 2026-10-13: "show the last Tuning entry"): after a rotate or
    a retouch the Tuning tab and the Basic Fixes Fill Light row read 0, though the photo still
    carries its `tuning(…)`. The sliders show the photo's last `tuning` entry wherever it sits; a
    new Tuning session updates that entry in place when nothing after it depends on its pixels
    (rotations, flips and other position-free steps), and appends a new entry otherwise; one step
    either way, undoable as today. `editing.md` §7 says which.
  - done when: Fill Light 0.3 → rotate → the tab and the row show 0.3; dragging Highlights then
    leaves one `tuning` entry, one step, undone by one Undo; after a retouch that follows the
    tuning, a new drag appends; an unknown operation between them survives; CI green.
- ✅ **M8.29 `ui`: the canvas rests while a preview renders, and old thumbnails keep their shape**
  - after: M7.R
  - files: `crates/ui/src/panels/edit/canvas.rs`, `crates/ui/src/headless.rs`, `crates/ui/src/panels/thumbnails.rs`, `crates/ui/tests/edit_canvas.rs`, `crates/ui/tests/edited_cells.rs`
  - scope: found in M7 (M7.15, M7.23, M7.9): while a preview is pending the canvas asks for an
    immediate repaint every frame, which spins a CPU on slow machines and starved the preview
    workers on 3-core CI; repaint after a short delay instead and let the headless driver count a
    pending preview as busy, so `ui.settle` still waits for it (and M7.23's real-time wait in
    `straighten_tool.rs` can go). Drop the canvas's dead Ctrl+− fallback (`minus_key`; the keymap
    has it since M7.20). While a cropped photo's new thumbnail renders, draw the old one at its own
    shape (letterboxed) rather than stretched.
  - done when: frames per second while a preview renders stay ≤ 1 per 16 ms and settle still waits
    for the preview (a test); the old-texture frame of a newly cropped cell keeps its aspect (a
    test); CI green on the three OSes with no flake in `straighten_tool.rs` over 3 runs.
- ✅ **M8.30 Docs left stale by M7**
  - after: M7.R
  - files: `crates/edit/README.md`, `docs/showcase.md`
  - scope: found at M7.R: the `edit` README still says renderers "arrive with M7.8, M7.13 and
    M7.14" and "`suggest`: empty until M7.12" and lacks `crop_geom`, `guides` and the retouch
    limits; `showcase.md` runs `dalica-cli snapshot` with a positional folder, which exits 2 since
    the M7.29 rewrite of `control-protocol.md` §8.
  - done when: each line matches the code; `cargo xtask ci` passes.
- ✅ **M8.R Release 0.3 gate**
  - after: M8.25, M8.27, M8.28, M8.29, M8.30
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 0.3): the lead runs `/milestone-review M8.R` to prepare it, with the
    version set to 0.3.0 and its `whats-new.md` section on the review branch, the release checklist
    and the sign-off question, and after the merge cuts and publishes 0.3.0 (`releasing.md`); the check-in page (edit a photo
    and save it from the folder header, find the original in `.dalica-originals/`, Undo Save from
    the edit panel, save again and Revert, Save a Copy, Save As PNG into a new folder, a
    rotate-only save, save a raw and see the JPEG beside it, export a tray at 1600 with a
    watermark and location removal, cancel an export, find the export under Projects, written as
    steps for the maintainer to try); the questions: the look of the watermark (approved with its
    fingerprints), the planning decisions above that change behaviour (a re-save renders from the
    original; formats saved beside keep their edits; Undo Save only for in-place saves; GPS removal
    keeps place names; numbered names `001-`; the save and export budgets); *Where we stand* and
    the `status.md` numbers (Save of a 24 MP JPEG, a lossless rotation, the 200-photo export, the
    quality estimate's error) on a `review/M8.R` branch, a pull request open. Only the maintainer
    merges that pull request and closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M8.1 library fields  ‖  M8.2 fs backup + new files  ‖  M8.3 engine modules + runner
         ‖  M8.4 ui state + stubs  ‖  M8.5 ui-kit form + icons  ‖  M8.6 encoders + quality
         ‖  M8.7 metadata carried  ‖  M8.8 lossless JPEG transforms
         (once M7.R is closed)
wave 2   M8.9 originals record + fields on disk  ‖  M8.10 output pipeline
wave 3   M8.11 watermark  ‖  M8.12 Save, Undo Save, Revert  ‖  M8.13 export plan + files
wave 4   M8.14 lossless, beside + copies  ‖  M8.15 save commands  ‖  M8.16 export command + job
wave 5   M8.17 Save a Copy + Save As  ‖  M8.18 interrupted saves  ‖  M8.19 Save in the app
         ‖  M8.20 Export dialog  ‖  M8.21 progress, notices + Exported  ‖  M8.23 every format
         ‖  M8.24 200-photo export + bench
wave 6   M8.22 Save As in the app
         (fixes from M7.R, any wave: M8.27 Windows file id ‖ M8.28 Tuning sliders ‖ M8.29 canvas
          rest + thumbnail shape ‖ M8.30 docs)
wave 7   M8.25 accept M8 → M8.R          (M8.26 lead: M10 blocks, any time after M7.24)
```

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

#### M9 tasks

M9 makes the library organisable: albums that last and span folders, instant search with filters,
and file operations that move, copy, rename and delete real files without ever losing one. Its
first wave lists the M8 work each task extends and starts once the maintainer has closed M8.R. Five
tasks lay the ground the others share, so later tasks never share a file: M9.1 adds M9's model to
`library` (albums and their position keys, the automatic albums, search facts, moves, renames and
removals of photos and folders as ops with exact inverses, a section filter for search mode), M9.2
gives `fs` the OS trash and moves and copies that never replace a file, M9.3 registers one engine
command module per area, file operations as library undo steps whose undo moves files back on a
worker, and M9's settings, M9.4 adds M9's view state, UI commands, the photo context menu's frame,
the banner's place and stub dialogs, and M9.5 gives `ui-kit` the album icons, the filter banner,
the drag pill and the age slider. The search grammar (M9.8) lives in `library` beside the model it
reads, where `architecture.md` §2 puts it; the engine keeps its index and runs it on a worker
(M9.11). Sidecar entries travel with their files through one `store` module (M9.9), so a move, a
copy, a rename and a folder merge carry them the same way. Each task updates its `parity.md` rows
(`ORG-ALBUM-NEW`, `ORG-ALBUM-EDIT`, `ORG-ALBUM-AUTO`, `SRCH-LIVE`, `SRCH-FILTERS`, `SRCH-BANNER`,
the Save half of `SRCH-SAVE`, `SRCH-DUP`, `FILE-MOVE`, `FILE-NEWFOLDER`, `FILE-RENAME`,
`FILE-DELETE`, `FILE-FOLDEROPS`, `LIB-TREE-DND`, the album and search parts of `LIB-TREE-GROUPS`,
`LIB-LT-SECTIONS`, `LIB-LT-HEADER`, `LIB-TRAY`, `LIB-TRAY-HOLD`, `LIB-EMPTY` and `LIB-SEL`, Search
for This Tag in `ORG-TAGS`, renaming in `ORG-FOLDERPROPS`, Sort By in `ORG-SORT-PHOTOS`, the File,
Edit, Folder / Album and Tools menu rows; new rows where none fit: `LIB-CTX-PHOTO` for the photo
context menu, `DATA-FILEOPS-RECOVERY` for the repair of an interrupted file operation) and names its
commands there; first fingerprints of the album tree and headers, the banner, the drag pill, the
dialogs and the icons are approved in bulk on the M9.R review page.

Whole-window baselines belong to no task, as in M5–M8: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m8-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.

Decisions taken while planning (the check-in lists them for reversal): menu items ending in "…"
whose engine command takes input open their dialog in the UI, and the dialog's default button runs
that same engine command with the input (`photo.rename`, `photo.moveToNewFolder`, `folder.move`),
as control-protocol §5.3 already shows for `photo.rename`; New Album… is the UI command `album.new`
(Ctrl+N, the toolbar button) that opens Album Properties and runs the engine's `album.create`, the
way `output.export` runs `export.run`, and Album Properties is `section.properties` on an album
section, running `album.info.set`; Cut, Copy and Paste are UI commands that keep the photos in the
view state, and Paste runs `photo.moveTo` or `photo.copyTo` into the current folder; in an album
section the Delete key removes from the album and Ctrl+Delete deletes from disk, routed by the UI
by section as Ctrl+1 is by view. Album ids are `a` and 12 random hex digits from the OS random
source (`getrandom`, already in the tree), drawn again on a clash. The Albums group lists the search
album (while searching), Starred Photos, Recently Updated, then user albums in the tree's folder
sort, an album's date being its own, else its newest photo's; Starred Photos lists by date like a
folder; a photo is "recently updated" by the later of its file's modification time, the time this
library first indexed it and the last time Dalica changed its data (index-only: after a rebuild only
the file's time counts), newest first, within the setting's 30 days; an album's cover is its first
photo until one is set, and falls back to the first when the cover leaves. A key that would pass 512
bytes re-keys its album inside the same undo step. `albums.json` is written after album changes and
read at open; when it is missing, a rebuild recovers memberships, order and names from the
sidecars, and description, date, place and cover start empty. `section.sort` sorts albums (rewriting
their keys) and folders (the folder's `sort`, with a `reverse` flag added to `data-model.md` §3);
a folder's manual order by dragging stays out of M9 with the rest of `ORG-SORT-PHOTOS`. Search
matches word prefixes ignoring case (no accent folding until localisation, M15), month names in
English until M15, and a photo through its own text, its folder's name, description and place, and
the names of its albums; an unknown `name:value` is searched as plain words; hidden photos match
only while hidden pictures are shown; the age slider counts back from today on the effective date
(the user's, else the capture date, else the file's), on a logarithmic scale up to 20 years; the
Faces filter is drawn disabled until faces exist (M13), and `person:` and `color:` match nothing
and say so until their milestones. Queries run on a worker after a 120 ms debounce on the session
clock; the worker keeps the index and updates it for the photos each applied op touched, never
rebuilding it per revision; stale results are dropped by generation. The search budgets join
`development.md` §8 at 1.5 × their first measurement and never above 100 ms per query at 100 k
photos. Find Duplicates finds identical files only: equal sizes, then an MD5 digest (the `md5`
crate is already in the tree for the IPTC digest) as the bucket key, then a byte-for-byte
comparison, so a digest collision never groups different files; digests are cached index-only by
size and modification time, and the groups show as search-mode sections. Every move, copy, rename
and delete is a file job on M8.3's runner, entered in a file-operation journal (`file-ops.json` in
the library directory) before anything moves and repaired at open (rolled forward when the
destination verifies, else back), as M8 journals a Save; a moved or renamed photo keeps its id, and
its whole sidecar entry (unknown keys too), its `.dalica-originals/` backup and its record travel
with it; within a volume a move is a rename that never replaces, across volumes a copy, fsync, a
byte-for-byte verification, then the source to the trash; Dalica's own moves are marked so the
watcher imports nothing. Moving asks the first time ("Don't ask again"), copying never asks in the
engine (the UI asks "Copy %d files to %s?" before it runs), and a name clash asks [Rename
Duplicates] [Skip Duplicates] [Cancel] (renamed to `-001` by `core::names`). Undo moves files back
on a worker; undoing a copy sends the copies to the trash when they are unchanged; undoing a delete
restores from the trash where the platform can (Linux, Windows) and is otherwise disabled with the
reason ("Restore it from the Trash"). Moving or renaming a watched folder rewrites its `folders.json`
entry; a folder moves only into a library folder; a folder moved onto one of the same name merges
into it photo by photo under the collision rules; new folders (Move to New Folder, Split Folder
Here) are made beside the source folder, named through `core::names::sanitize` (M8.13). Folder
Properties' OK renames first and then sets the info: two undo steps. Cross-volume tests force that
path through an `fs` `faults` switch and also run on a real second file system (a tmpfs) when the
test machine has one, saying which ran. Copy Path, Locate on Disk, Show Tag as Album and dragging
out to other applications stay out (M15 or later). No step of M9 needs a maintainer decision
mid-run.

- ✅ **M9.1 `library`: albums, search facts and file-operation ops**
  - after: M8.1, M8.R
  - files: `crates/library/src/model.rs`, `crates/library/src/op.rs`, `crates/library/src/query.rs`, `crates/library/src/tree.rs`, `crates/library/src/albums.rs`, `crates/library/tests/m9_fields.rs`, `crates/library/tests/albums.rs`, `crates/library/tests/ops.rs`, `crates/library/tests/tree.rs`, `crates/library/README.md`
  - scope: `data-model.md` §2–§4 and §8 and `organizing.md` §4 as pure model: albums (id, name,
    date with "use the newest photo's", place, description, cover) and each photo's memberships
    with their position keys (`core::poskey`); each photo gains search facts (camera make and model,
    video, holds a GPS location), its first-indexed time, its last-changed time (set from the op's
    clock time by every op that changes its data) and a content digest keyed by size and
    modification time (facts and digests index-only like the sketch: no undo step, no revision bump
    on load); each folder gains its photo sort (`name | date | size`, reverse). One batched op per
    change with an exact inverse: album created, deleted (every membership in the inverse), info
    set, members added at keys, removed, re-keyed (a whole album at once), cover set; a photo moved
    to another folder or renamed (its id and all its data kept), a photo removed with everything
    said about it, a folder renamed or moved (its subtree's paths with it, ids kept), an empty
    folder added. `albums.rs`: the keys for an add, a drop between two members and Sort Album By
    (`poskey` `after` and `between`; a key over 512 bytes re-keys the album, `data-model.md` §3.2).
    Queries cached per revision: an album's members in order (equal keys by folder path, then file
    name), Starred Photos, Recently Updated within n days of a given now, each album's date, count
    and cover. `tree.rs`: the Albums group (the search album first when there is one, Starred
    Photos, Recently Updated, then user albums in the folder sort), album sections, folder sections
    in their folder's sort, and an optional section filter (a set of shown photos: sections without
    one drop out; the search album lists them all).
  - done when: `cargo test -p dalica-library`: random op sequences with the new ops applied then
    inverted restore the model exactly (proptest); a moved or renamed photo keeps its id, caption,
    tags, recipe and memberships; 10 000 drops at one spot keep keys under 10 bytes, and repeated
    drops into the middle re-key before 512 bytes with the order unchanged; equal keys sort by path
    then name; Recently Updated follows a virtual clock; the section filter drops sections without
    matches in flat and tree mode; loading facts or digests bumps no revision.
- ✅ **M9.2 `fs`: the OS trash, and moves and copies that never replace a file**
  - after: M8.2, M8.R
  - files: `crates/fs/src/trash.rs`, `crates/fs/src/transfer.rs`, `crates/fs/src/transfer/**`, `crates/fs/src/faults.rs`, `crates/fs/tests/trash.rs`, `crates/fs/tests/transfer.rs`, `docs/security.md`, `NOTICE`
  - scope: `security.md` §5.3 and §5.7 and `data-model.md` §8: `transfer.rs` moves a file to
    another folder or to a new name in its own, through held folder handles (M5.3), its identity
    re-checked right before, by a rename that never replaces (M8.2's commit: `renameat2` NOREPLACE,
    a hard link then unlink, exclusive creation), a clash answered as `Exists` for the caller to
    rename or skip; across volumes (`EXDEV` or another device) it copies into a new file (temp,
    fsync, a commit that never replaces), verifies the copy byte for byte against the source, then
    sends the source to the trash, never before the copy verifies (the two phases of LightCraft
    `crates/engine/src/import_move.rs` `place` / `finish` at b7091805, ported, with a header and a
    `NOTICE` row); a copy is the first phase alone; folders: a new folder made exclusively, a folder
    renamed or moved without replacing one (across volumes a bounded copy of the tree, verified file
    by file, then the source tree to the trash); `trash.rs` sends a file or folder to the OS trash
    through the `trash` crate (already pinned in the workspace; audited with `cargo xtask deps`),
    answers `NoTrash` for a volume without one (never a permanent delete), and restores a trashed
    item to its old path where the platform's API allows (Linux, Windows), `Unsupported` elsewhere;
    a `faults` switch that forces the cross-volume path; fault points at every step; `security.md`
    §5.7's statuses updated.
  - done when: `cargo test -p dalica-fs --test transfer --test trash`: a move keeps the bytes and
    the modification time and never replaces a file at the target, even one created between the
    check and the move (a racing thread); the forced cross-volume path copies, verifies and trashes
    the source, and a byte flipped in the copy (a fault point) leaves the source in place; a link in
    place of either folder is refused with nothing moved; a folder moves with its subtree; a
    trashed file comes back on Linux and Windows and is `Unsupported` on macOS; a volume without a
    trash gives `NoTrash` and the file stays; every fault point as Error and as Crash leaves the
    source whole (`--features faults`); hostile names are refused.
- ✅ **M9.3 `engine`: M9's command modules, file operations on the undo stack, and settings**
  - after: M8.3, M8.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/albums.rs`, `crates/engine/src/cmd/search.rs`, `crates/engine/src/cmd/files.rs`, `crates/engine/src/cmd/rename.rs`, `crates/engine/src/cmd/delete.rs`, `crates/engine/src/cmd/folder_ops.rs`, `crates/engine/src/cmd/duplicates.rs`, `crates/engine/src/fileops.rs`, `crates/engine/src/fileops_recovery.rs`, `crates/engine/src/albums_file.rs`, `crates/engine/src/search.rs`, `crates/engine/src/duplicates.rs`, `crates/engine/src/undo.rs`, `crates/engine/src/jobs.rs`, `crates/engine/src/folders.rs`, `crates/engine/src/session.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/fileops.rs`, `crates/engine/tests/settings_m9.rs`, `crates/engine/README.md`, `docs/architecture.md`
  - scope: the engine's ground for M9 so M9.10–M9.16 and M9.21 never share a file: empty
    `cmd/albums.rs`, `search.rs`, `files.rs`, `rename.rs`, `delete.rs`, `folder_ops.rs` and
    `duplicates.rs` joined into the registry once; `fileops.rs`: a file operation is a file job on
    M8.3's runner (`JobKind::Files`) whose result applies as one library undo step of a new kind,
    and whose undo and redo run the reverse operation as a file job in turn ("Undo Move"; the step
    stays on the stack while it runs, and a step that cannot be undone says why through `enabled`);
    each operation entered in the file-operation journal before it starts (M9.9's format; a stub
    writer until then); both ends of a move marked as Dalica's own (M8.3's mark) so the watcher
    imports nothing; a session event per finished operation (moved, copied, renamed, trashed,
    skipped, failed with file and reason) for the UI's notices; stubs `fileops_recovery.rs` (called
    at open), `albums_file.rs` (called after every applied op that touches albums), `search.rs` (the
    search state: text, filters, age, mode `search | duplicates`, generation, last result) and
    `duplicates.rs`; `folders.rs` gains `rebase(from, to)`, rewriting the `folders.json` entries at
    and under a moved or renamed folder; every M9 setting with its default behind a typed accessor:
    ask before moving (on), remove from albums without asking (off), delete without asking (off),
    Recently Updated days (30), the last Move Folder destination; `architecture.md` §4 describes
    file-operation undo steps.
  - done when: `crates/engine/tests/fileops.rs` with a fake operation: it runs as a job and applies
    once as one undo step; undo runs its reverse as a job and redo runs it again; Cancel between
    files keeps what finished as one step; a failure is reported with its reason while the rest go
    on; the watcher imports nothing for the marked paths; the owner thread made no `fs` call;
    `rebase` moves nested entries and keeps the others; `crates/engine/tests/settings_m9.rs`
    round-trips every new setting with unknown keys kept and wrong types falling back (reported);
    every existing engine test and `panic_hunt` pass unchanged.
- ✅ **M9.4 `ui`: M9's view state, UI commands, the photo context menu's frame and stub dialogs**
  - after: M8.4, M8.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/menu_catalog.rs`, `crates/ui/src/app.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/albums.rs`, `crates/ui/src/commands/search.rs`, `crates/ui/src/commands/files.rs`, `crates/ui/src/commands/rename.rs`, `crates/ui/src/commands/folder_ops.rs`, `crates/ui/src/commands/clipboard.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/confirm.rs`, `crates/ui/src/dialogs/album_properties.rs`, `crates/ui/src/dialogs/remove_from_album.rs`, `crates/ui/src/dialogs/move_confirm.rs`, `crates/ui/src/dialogs/copy_confirm.rs`, `crates/ui/src/dialogs/collision.rs`, `crates/ui/src/dialogs/new_folder.rs`, `crates/ui/src/dialogs/rename_files.rs`, `crates/ui/src/dialogs/delete.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/layout.rs`, `crates/ui/src/panels/search_banner.rs`, `crates/ui/src/panels/light_table/context_menu.rs`, `crates/ui/src/panels/light_table/input.rs`, `crates/ui/src/drag.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state_m9.rs`, `docs/control-protocol.md`
  - scope: every M9 field of `UiState` in one go: the search field's text, the autocomplete
    highlight, the filters and the age, search mode's saved scroll anchor; the clipboard (cut or
    copy, its photos); the drag in progress (`ui.inspect` → `drag`: from, ids, target, op, pill);
    the drafts of Album Properties, Rename Files, the new-folder dialog, and the move, copy,
    collision and delete questions; UI commands with stub handlers, so M9.17–M9.23 never share a
    file: `album.new` (Ctrl+N), `photo.cut`, `photo.copy`, `photo.paste` (Ctrl+X, Ctrl+C, Ctrl+V in
    the library), `search.focus` (Ctrl+F, F3), `photo.splitFolder`, `photo.showInFolder`, and the
    openers of the `…` items `photo.rename` (F2), `photo.moveToNewFolder` and `folder.move`; the
    Delete key routed by section (Remove from Album in an album, `photo.delete` elsewhere;
    Ctrl+Delete always `photo.delete`); stub dialogs on the host (`albumProperties`,
    `removeFromAlbum`, `moveFiles`, `copyFiles`, `collision`, `newFolder`, `renameFiles`,
    `deletePhotos`, `deleteFolder`, the folder picker of Move Folder) with `confirm.rs` asking for
    `album.delete`, `album.remove`, `photo.delete`, `folder.delete` and `photo.moveTo`; `section_kind`
    answering Album for album sections, so the fourth menu reads Album with `organizing.md` §9.2's
    album rows; the photo context menu (`organizing.md` §9.1; `ui.menu.tree {context: "photo"}`)
    opened by a right-click through `light_table/input.rs` into a stub `context_menu.rs`, its rows
    greyed until their commands exist; a press-and-drag on a thumbnail starting `drag.rs`'s stub (no
    drop yet); `layout.rs` reserving the 32-high banner at the light table's top in search mode for
    the stub `search_banner.rs`; control-protocol §5.5, §6.5, §6.6 and §6.10 updated.
  - done when: `crates/ui/tests/view_state_m9.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); each opener shows its dialog by
    `dialog:<kind>` and Esc closes it; `ui.menu.tree {context: "photo"}` lists §9.1's rows in order;
    Delete and Ctrl+Delete route by section; the menu items light up; every command's
    graceful-failure test; `panic_hunt` and `widget_ids` stay green.
- ✅ **M9.5 `ui-kit`: album icons, the filter banner, the drag pill and the age slider**
  - after: M8.5, M8.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/banner.rs`, `crates/ui-kit/src/widgets/drag_pill.rs`, `crates/ui-kit/src/widgets/age_slider.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/albums.rs`, `crates/ui-kit/tests/m9_widgets.rs`, `crates/ui-kit/tests/icons_albums.rs`, `crates/ui-kit/tests/fingerprints/m9-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-albums*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `design-system.md` §5–§6 and `ui-layout.md` §4, §5.5 and §9: `filter_banner` (32 high in
    the `filter_banner` colour, [Show All] at its left, the text after it, cut from its end with an
    ellipsis when narrow); `drag_pill` (near the pointer, never outside the window: the first
    photo's mini thumbnail, the count and the operation, "Move 3 photos", "Copy 3 photos" with a +,
    "Add to album"); the age slider (`zoom_slider`'s thin look with a logarithmic map from 0 =
    everything to 20 years, and its label "Pictures up to 3 months old" in days, weeks, months and
    years, both pure functions the UI and the tests share); the insertion marker between two cells
    while reordering an album (a 2 pt `accent` bar with end caps); the `drop_target` highlight as
    one painter for tree rows and the tray; the icons `StarredAlbum` (a book with a star),
    `RecentAlbum` (a book with a clock) and `SearchAlbum` (a book with a magnifier), our own
    drawings on the 20-unit grid; the widget gallery gains them.
  - done when: `crates/ui-kit/tests/m9_widgets.rs`: the banner's text elides from its end and Show
    All stays whole at both window widths; the pill stays inside the window at every corner and
    shows + only for a copy; the age map is monotonic, 0 means everything, and the labels read as
    specified at 1 day, 2 weeks, 3 months and 5 years; `crates/ui-kit/tests/icons_albums.rs`: each
    icon stays in its grid at 12, 16, 20 and 28 px in rest, hover and disabled colours; gallery and
    icon fingerprints; `widget_ids`, `cargo xtask assets` and `cargo xtask names` pass.
- ✅ **M9.6 `sidecar` + `store`: albums and M9's fields on disk and in the index**
  - after: M9.1, M8.9
  - files: `crates/sidecar/src/fields.rs`, `crates/sidecar/src/print.rs`, `crates/sidecar/src/lib.rs`, `crates/sidecar/src/albums.rs`, `crates/sidecar/tests/albums.rs`, `crates/store/src/mapping.rs`, `crates/store/src/journal.rs`, `crates/store/src/snapshot.rs`, `crates/store/src/rebuild.rs`, `crates/store/src/albums.rs`, `crates/store/tests/m9_fields.rs`, `crates/store/tests/albums.rs`, `docs/data-model.md`
  - scope: `data-model.md` §2–§4: a photo's `"albums"` (album id → position key) and the folder's
    top-level `"albums"` names in `.dalica.json`, in the documented key order, ids and keys checked
    (a bad one kept verbatim and reported, never used); the folder's `"sort"` and a new `"reverse"`
    flag in `"folder"`; `albums.json` in the library directory (id, name, description, date, use
    newest, place, cover, and a copy of the order) parsed and printed with unknown keys kept, a
    damaged file set aside as `.corrupt-<time>` and rebuilt; the album fields, facts, first-indexed
    and last-changed times and digests in the journal and snapshot (facts, times and digests
    index-only); a rebuild collects memberships from every folder sidecar and definitions from
    `albums.json`, or names from the folders' `"albums"` when it is missing; the mapping test lists
    each new field; `data-model.md` §3 and §4 updated.
  - done when: `data-model.md` §10.1 for every M9 field (scan, change everything, delete the index,
    rebuild, compare); §10.12 at the store level (an album over three folders keeps its order after
    deleting `albums.json` and the index and rebuilding, its name recovered from the folders);
    proptest round trips with hostile ids, keys and wrong types never panic and keep what they
    cannot use; a damaged `albums.json` is set aside and reported; journal replay and snapshots
    keep the new fields.
- ✅ **M9.7 `scan` + `engine`: search facts read on scan**
  - after: M9.1, M6.32
  - files: `crates/scan/src/meta.rs`, `crates/scan/src/facts.rs`, `crates/scan/tests/facts.rs`, `crates/engine/src/scanning.rs`, `crates/engine/tests/facts.rs`
  - scope: `organizing.md` §8: the scan's metadata pass (M6.32) also reads each new or changed
    photo's search facts on its worker: camera make and model (EXIF, trimmed, kept as read), whether
    it is a video (its file type) and whether it holds a GPS location (the EXIF GPS IFD, or
    `exif:GPSLatitude` and `exif:GPSLongitude` in XMP), bounded like the rest of the pass; the
    engine turns them into M9.1's ops in the same batches, without an undo step.
  - done when: `cargo test -p dalica-scan --test facts`: `scenes` JPEG, TIFF, PNG, raw and video
    files give the expected facts; a file without EXIF gives none; GPS from EXIF and from XMP;
    truncated and hostile blocks give no facts and never a panic; `crates/engine/tests/facts.rs`:
    after a scan the facts are in the library, a file rewritten with another camera model updates
    them, and the undo stack is unchanged.
- ✅ **M9.8 `library`: the search grammar, matcher and index**
  - after: M9.1
  - files: `crates/library/src/search.rs`, `crates/library/src/search/**`, `crates/library/tests/search.rs`, `crates/library/tests/search_props.rs`, `NOTICE`
  - scope: `organizing.md` §8.1–§8.2: the grammar (words split at space, dot, underscore and dash;
    several words AND; `-word` NOT; `tag:`, `camera:`; `person:` and `color:` parsed, matching
    nothing and reported as not available yet; an unknown `name:value` searched as plain words);
    prefix matches ignoring case on file names, captions, tags, folder names, descriptions and
    places, album names, effective dates (years, `2026-06`, `2026-06-04`, English month names),
    camera make and model, and pixel sizes (`4000x3000`); the filters Starred, Videos and
    Geotagged (Faces matches nothing until M13) and the age (days back from a given now); hidden
    photos only when asked; an index (sorted terms → photo sets, prefix ranges by binary search)
    built from an owned extract of the searchable fields and updated photo by photo, so the engine
    can keep it on a worker; a result in light-table order with the matching sections and the
    counts, folders and bytes the banner shows; suggestions for a prefix (tags, folders and albums
    with their counts, at most 100). Prior art: LightCraft `crates/catalog/src/query.rs` at
    b7091805 (tokens and fielded terms), ported where it fits with a header and a `NOTICE` row.
  - done when: `cargo test -p dalica-library --test search --test search_props`: grammar tests per
    operator and their combinations; an empty query matches everything; `-` excludes; the prefix
    and case rules; each date spelling; a tag renamed or a caption changed shows in the results
    after its photo's update; an index updated through random op sequences equals one built from
    scratch (proptest); hostile queries (10 000 words, 1 MiB, control and bidirectional characters)
    are bounded and never panic; the times to build and to query 100 k synthetic photos printed.
- ✅ **M9.9 `store`: sidecar entries travel with their files, and the file-operation journal**
  - after: M9.2, M9.6
  - files: `crates/store/src/carry.rs`, `crates/store/src/intents.rs`, `crates/store/src/writer.rs`, `crates/store/tests/carry.rs`, `crates/store/tests/intents.rs`, `docs/data-model.md`
  - scope: `data-model.md` §8: a moved photo's whole entry (unknown keys included) leaves the
    source folder's `.dalica.json` and enters the destination's under its new name (a renamed
    photo's stays in its folder under the new name), merged entry by entry and never replacing a
    sidecar, both writes sequenced through the writer with the folders' pending debounced changes
    so none is lost; a copied photo's entry is duplicated; the photo's `.dalica-originals/` backup
    and its record (M8.9) move or copy with it through `fs` (M9.2); a folder merge carries every
    entry; the file-operation journal `file-ops.json` in the library directory (`{"version": 1,
    "ops": [{id, kind, items: [{from, to, state: pending | placed | done}]}]}`) written through the
    I/O worker before an operation starts and after each item, a damaged file set aside and
    reported; `intents::plan(op, what is on disk)` deciding per item whether to roll forward or
    back, pure apart from the facts it is given; §8 gains the journal and the order of steps.
  - done when: `cargo test -p dalica-store --test carry --test intents`: a photo moved between two
    folders arrives with its unknown keys and memberships and leaves the source; a star made just
    before the move, still in the debounce, lands in the destination; a rename keeps the entry; a
    move that renamed a duplicate keeps both entries; the backup and its record travel and a copy
    duplicates them; the journal round-trips and `plan` answers every item state; hostile journals
    never panic and are set aside.
- ✅ **M9.10 `engine`: album commands**
  - after: M9.3, M9.6
  - files: `crates/engine/src/cmd/albums.rs`, `crates/engine/src/cmd/section.rs`, `crates/engine/src/cmd/photo.rs`, `crates/engine/src/albums_file.rs`, `crates/engine/tests/albums.rs`
  - scope: `organizing.md` §4 and `data-model.md` §3.2: `album.create {name, date?, useNewest?,
    place?, description?, ids?}` (the new id in the result, the photos added in order),
    `album.info.set {album, …}`, `album.add {album, ids, before? | after?}`, `album.remove {album,
    ids, confirm?}` (`needs_confirm` with the "Remove from Album" question unless its setting is
    on), `album.reorder {album, ids, before? | after?}`, `album.setCover {album, id}`, `album.delete
    {album, confirm}` (`needs_confirm`), `albums.list` (query: id, name, kind, count, cover, date);
    `section.sort {section?, by, reverse?}` on an album (its keys rewritten) and on a folder (its
    sort); `album:starred`, `album:recent` and user albums accepted wherever a section is;
    `photo.inspect` lists a photo's albums; each one library undo step, re-keying inside the step
    that needed it; `albums_file.rs` writes `albums.json` through the I/O worker after album changes
    and reads it at open.
  - done when: `crates/engine/tests/albums.rs` on temp folders: `data-model.md` §10.12 through the
    commands (an album over three folders in a custom order; delete `albums.json` and the index;
    rebuild; the same order); undo and redo of every command restore the model and the sidecars on
    disk; a drop between two members rewrites only the moved photo's sidecar; deleting an album
    keeps the files and removes every membership, and undo brings them back; Sort Album By and
    Reverse Order; Starred Photos and Recently Updated follow stars and edits on a virtual clock;
    each command's contract and graceful failures; `panic_hunt` green.
- ✅ **M9.11 `engine`: search on a worker, suggestions and Save Search as Album**
  - after: M9.3, M9.7, M9.8
  - files: `crates/engine/src/cmd/search.rs`, `crates/engine/src/cmd/library.rs`, `crates/engine/src/search.rs`, `crates/engine/tests/search.rs`
  - scope: `organizing.md` §8 and `ui-layout.md` §9: `search.set {text?, filters?, age?}` (no undo
    step; debounced 120 ms on the session clock; the query run on a worker against M9.8's index,
    which the worker keeps and updates for the photos each applied op touched; results tagged with
    a generation, stale ones dropped; `library.inspect` → `search`: active, text, filters, age,
    matches, folders, bytes, seconds), `search.clear`, `search.suggest {text}` (query),
    `search.saveAsAlbum {name}` (Tools ▸ Experimental ▸ Save Search Results…: a user album of the
    results in light-table order, one undo step), `library.query` taking the grammar in `text` and
    paging the results; search mode's sections through M9.1's section filter, and `album:search`
    accepted wherever a section is (an export of the results included); the owner thread does no
    search work beyond applying results.
  - done when: `crates/engine/tests/search.rs`: five keystrokes inside the debounce run one query;
    results equal M9.8's for the same library; a star changed during search updates them; Starred,
    Videos, Geotagged and the age combine with text; `search.clear` shows everything again; Save
    Search as Album makes an album in that order and undo removes it; `album:search` lists the
    results; a query on a 100 k synthetic library answers off the owner thread (time printed); each
    command's contract and graceful failures; `panic_hunt` green.
- ✅ **M9.12 `engine`: the 100 k search benchmark and its budgets**
  - after: M9.11
  - files: `crates/engine/benches/search.rs`, `xtask/src/bench.rs`, `xtask/src/accept.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: the `search` bench (registered in `xtask/src/bench.rs`) over a synthetic 100 k library
    with captions, tags, folders and albums: building the index, a one-word query, three words with
    a filter, a `-word`, and the time from a `search.set` (debounce excluded) to results applied;
    its budgets at 1.5 × the first measurement and never above 100 ms per query, in
    `development.md` §8 and the release-budgets job (`BUDGETS`, M5.24).
  - done when: `cargo xtask bench search` prints the numbers (in the PR) and `cargo xtask budgets`
    passes with them.
- ✅ **M9.13 `engine`: move and copy photos, and Move to New Folder**
  - after: M9.9, M8.13
  - files: `crates/engine/src/cmd/files.rs`, `crates/engine/tests/move_copy.rs`, `crates/engine/tests/new_folder.rs`
  - scope: `organizing.md` §9 and `data-model.md` §8: `photo.moveTo {ids, folder, collisions?,
    confirm?, dontAsk?}` (`needs_confirm` with the first-time question while its setting is on;
    `collisions: rename | skip`, and without it a clash answers `needs_confirm` with the [Rename
    Duplicates] [Skip Duplicates] [Cancel] question naming the clashing files), `photo.copyTo {ids,
    folder, collisions?}` (the copies added as new photos with the sources' entries),
    `photo.moveToNewFolder {ids, name, date?, useOldest?, place?, description?}` (a new folder
    beside the photos' folder, its name through `core::names::sanitize` and the next free name, its
    info set, the photos moved: one undo step); the destination a library folder; all through
    M9.3's file operations with M9.9's carry, so ids, entries, memberships, recipes and backups
    travel; undo moves them back (and removes a folder it made while still empty); undoing a copy
    trashes the copies still unchanged.
  - done when: `crates/engine/tests/move_copy.rs` on temp folders: `data-model.md` §10.3 (moved
    photos keep star, caption, tags, recipe, album order and their `.dalica-originals/` backup);
    undo and redo restore the files on disk and the model; the first-time question is asked once
    and not after `dontAsk`; Rename Duplicates gives `-001` and Skip leaves both files untouched; a
    forced cross-volume move copies, verifies, then trashes; Cancel half-way keeps the moved part as
    one step; a read-only destination fails with its reason and nothing moved;
    `crates/engine/tests/new_folder.rs`: Move to New Folder and a split (one photo and every photo
    after it) are one step each and undo removes the empty folder; the owner thread made no `fs`
    call; each command's contract and graceful failures; `panic_hunt` green.
- ✅ **M9.14 `engine`: rename and batch rename photos, and rename folders**
  - after: M9.9, M8.13
  - files: `crates/engine/src/cmd/rename.rs`, `crates/engine/tests/rename.rs`, `NOTICE`
  - scope: `organizing.md` §9 and `data-model.md` §8: `photo.rename {ids, name, addDate?,
    addSize?}` (one photo takes the name with its own extension; several get `-001`, `-002` … in
    light-table order; the date taken (`2026-06-04`) and the pixel size (`4000x3000`) added when
    asked; names through `core::names::sanitize`; a name another file or another photo of the batch
    holds takes the next free number, decided by file identity, so a change of letter case on a
    case-insensitive volume renames in place; if one rename fails the batch is moved back and
    nothing changes), `folder.rename {folder, name}` (the directory renamed, its watched-folder
    entry rebased, ids kept); through M9.3's file operations with M9.9's carry, one undo step each;
    the collision, case-change and rollback rules ported from LightCraft
    `crates/engine/src/rename.rs` at b7091805, with a header and a `NOTICE` row.
  - done when: `crates/engine/tests/rename.rs` on temp folders: three photos renamed `Lisbon` become
    `Lisbon-001.jpg` … `Lisbon-003.jpg` in order with their data; with the date and the size; a
    clash with an existing file takes the next number; `img.jpg` → `IMG.jpg` renames in place on a
    case-insensitive volume (the macOS and Windows runners); a planted failure on the second file
    leaves all three as they were; undo and redo; a renamed folder keeps its ids and sidecar, and a
    renamed watched folder its `folders.json` entry; names with `]`, `"`, `;`, `=`, non-ASCII and
    255 bytes round-trip (`data-model.md` §10.7); each command's contract and graceful failures;
    `panic_hunt` green.
- ✅ **M9.15 `engine`: delete to the trash, and move folders**
  - after: M9.9, M8.13
  - files: `crates/engine/src/cmd/delete.rs`, `crates/engine/src/cmd/folder_ops.rs`, `crates/engine/tests/delete.rs`, `crates/engine/tests/folder_move.rs`
  - scope: `organizing.md` §9 and `security.md` §5.7: `photo.delete {ids, confirm}` and
    `folder.delete {folder, confirm}` (`needs_confirm` with the question unless "delete without
    asking" is on, and always over automation without `confirm`; to the OS trash, never
    permanently; a volume without a trash fails with an explanation that points to the file
    manager); the photos' data kept in the undo step, undo restoring from the trash where M9.2 can
    and otherwise disabled with the reason; `folder.move {folder, to, collisions?}` (into a library
    folder; its watched entry rebased; onto a folder of the same name it merges photo by photo under
    the collision rules, and the emptied source is removed only when nothing else is left in it);
    each one undo step through M9.3's file operations with M9.9's carry.
  - done when: `crates/engine/tests/delete.rs` on temp folders: a deleted photo is in the trash and
    gone from the library and its albums; undo restores it with its data on Linux and Windows, and
    on macOS undo is disabled with its reason; no trash fails and the file stays; a folder delete
    takes its non-picture files too; `crates/engine/tests/folder_move.rs`: a moved folder keeps ids,
    sidecar and subfolders; a merge keeps both folders' photos and entries; a forced cross-volume
    folder move verifies before trashing; undo and redo; the owner thread made no `fs` call; each
    command's contract and graceful failures; `panic_hunt` green.
- ✅ **M9.16 `engine`: Find Duplicates**
  - after: M9.11
  - files: `crates/engine/src/cmd/duplicates.rs`, `crates/engine/src/duplicates.rs`, `crates/engine/tests/duplicates.rs`
  - scope: `organizing.md` §8.3: `search.duplicates` (Tools ▸ Experimental ▸ Show Duplicate Files,
    Ctrl+F6) as a job with progress and Cancel: photos grouped by size, digests of the photos that
    share a size computed on workers (MD5 through the `md5` crate already in the tree, cached in the
    index by size and modification time, M9.6), each digest group confirmed byte for byte; the
    groups (largest total first) enter search mode `duplicates`, one section each, with "12
    duplicate groups (31 files)" for the banner; `search.clear` leaves it.
  - done when: `crates/engine/tests/duplicates.rs`: three copies of one `scenes` JPEG in three
    folders and a pair elsewhere make two groups; a different file of the same size is not grouped;
    a planted digest collision (a test hook) is split by the byte comparison; a second run reads
    no file whose digest is cached (counted); Cancel stops it; the owner thread made no `fs` call;
    each command's contract and graceful failures; `panic_hunt` green; the time for 10 k photos
    printed.
- ✅ **M9.17 `ui`: albums in the tree, album sections, Album Properties and the Album menu**
  - after: M9.4, M9.5, M9.10
  - files: `crates/ui/src/panels/folders/model.rs`, `crates/ui/src/panels/folders/paint.rs`, `crates/ui/src/panels/light_table/header.rs`, `crates/ui/src/commands/albums.rs`, `crates/ui/src/dialogs/album_properties.rs`, `crates/ui/tests/albums_tree.rs`, `crates/ui/tests/album_properties.rs`, `crates/ui/tests/section_header.rs`, `crates/ui/tests/fingerprints/region-tree-albums-*.json`, `crates/ui/tests/fingerprints/region-header-album-*.json`, `crates/ui/tests/fingerprints/dialog-album-properties-*.json`
  - scope: `organizing.md` §4, `ui-layout.md` §4 and §5.2: the Albums group's rows (M9.5's icons,
    name and count; the search album first and highlighted while searching; the group headers'
    magnifier in search mode), a click scrolling to the album's section; album section headers (the
    album icon, name, date and place, the description edited inline through `album.info.set`, the
    buttons as for folders); New Album… (File menu, Ctrl+N, the toolbar button) and Album
    Properties (`section.properties` on an album, a double-click on its name or header) on the
    dialog (Name selected, Date with "Use the date of the newest photo", Place, Description), OK
    running `album.create` with the tray's photos, then selecting the new album, or
    `album.info.set`; Delete Album… asking through the host; the Album menu's rows live; ids per
    control-protocol §7.
  - done when: `crates/ui/tests/albums_tree.rs` and `crates/ui/tests/album_properties.rs`
    (headless, a temp library): New Album with two photos in the tray creates the album, selects it
    in the tree and its section shows both; Properties changes name, date and description and Esc
    changes nothing; Delete Album asks, deletes, and Edit ▸ Undo brings it back; the fourth menu
    reads Album on an album section; Starred Photos and Recently Updated list as expected;
    `crates/ui/tests/section_header.rs` covers album headers; tree, header and dialog fingerprints
    at both sizes.
- ✅ **M9.18 `ui`: Add to Album, the photo context menu and the album token**
  - after: M9.4, M9.5, M9.10
  - files: `crates/ui/src/panels/light_table/context_menu.rs`, `crates/ui/src/panels/bottom/tray.rs`, `crates/ui/src/tray.rs`, `crates/ui/src/dialogs/remove_from_album.rs`, `crates/ui/tests/photo_menu.rs`, `crates/ui/tests/add_to_album.rs`, `crates/ui/tests/tray.rs`, `crates/ui/tests/fingerprints/region-tray-album-*.json`, `crates/ui/tests/fingerprints/menu-photo-*.json`
  - scope: `organizing.md` §4.3 and §9.1, `ui-layout.md` §6.2: the photo context menu's rows, live
    as their commands exist, with Add to Album ▸ (albums by name, then New Album…), and in an album
    Remove from Album (the question of `organizing.md` §4.3 unless its setting is on), Set as Album
    Cover and Show in Folder (selects the photo in its folder); the tray's Add to Album ▾ (the same
    list, adding the tray's photos); the tray's album token ("Album selected — 40 photos") when an
    album section is selected as a unit; ids per control-protocol §7.
  - done when: `crates/ui/tests/photo_menu.rs` and `crates/ui/tests/add_to_album.rs` (headless):
    the menu's rows in order and their enabled states in a folder and in an album; Add to Album ▸
    adds and Edit ▸ Undo removes; New Album… from the menu opens Album Properties with the photos;
    Remove from Album asks, removes and leaves the files; Set as Album Cover changes the tree's
    cover; Show in Folder selects the photo in its folder; `crates/ui/tests/tray.rs`: the album
    token and Add to Album ▾ live; tray and menu fingerprints at both sizes.
- ✅ **M9.19 `ui`: the move and copy questions, Move to New Folder, Split Folder Here and file notices**
  - after: M9.4, M9.5, M9.13
  - files: `crates/ui/src/dialogs/move_confirm.rs`, `crates/ui/src/dialogs/copy_confirm.rs`, `crates/ui/src/dialogs/collision.rs`, `crates/ui/src/dialogs/new_folder.rs`, `crates/ui/src/commands/files.rs`, `crates/ui/src/notices.rs`, `crates/ui/tests/move_dialogs.rs`, `crates/ui/tests/new_folder.rs`, `crates/ui/tests/notices.rs`, `crates/ui/tests/fingerprints/dialog-move-*.json`, `crates/ui/tests/fingerprints/dialog-copy-*.json`, `crates/ui/tests/fingerprints/dialog-collision-*.json`, `crates/ui/tests/fingerprints/dialog-new-folder-*.json`
  - scope: `organizing.md` §9: the first-time move question with "Don't ask again", the copy
    question, the collision question ([Rename Duplicates] [Skip Duplicates] [Cancel], naming the
    files), on the dialog host for `photo.moveTo` and `photo.copyTo`; File ▸ Move to New Folder…
    and the context menu's Move to New Folder… and Split Folder Here… on the new-folder dialog (Name
    "New Folder", Date with "Use the date of the oldest photo", Place, Description), running
    `photo.moveToNewFolder` with the selection, or the photo and every photo after it in its folder;
    `notices.rs` turns M9.3's events into toasts: "Moved 3 photos to Lisbon", copies, renames and
    trashed items with their counts, skipped duplicates, an undo that moved files back, failures
    naming the file and the reason (`caution`).
  - done when: `crates/ui/tests/move_dialogs.rs` (headless, temp folders): a move asks once, and
    with the box ticked never again; a clash offers both choices and each does what it says; Cancel
    moves nothing; `crates/ui/tests/new_folder.rs`: Move to New Folder and Split Folder Here make
    the folder beside the source with the photos and its info, and Edit ▸ Undo removes it;
    `crates/ui/tests/notices.rs`: one toast per finished operation and per failure; dialog
    fingerprints at both sizes.
- ✅ **M9.20 `ui`: Rename Files, the delete questions, Move Folder and renaming in Folder Properties**
  - after: M9.4, M9.14, M9.15
  - files: `crates/ui/src/dialogs/rename_files.rs`, `crates/ui/src/dialogs/delete.rs`, `crates/ui/src/dialogs/folder_properties.rs`, `crates/ui/src/commands/rename.rs`, `crates/ui/src/commands/folder_ops.rs`, `crates/ui/tests/rename_dialog.rs`, `crates/ui/tests/delete_dialogs.rs`, `crates/ui/tests/folder_properties.rs`, `crates/ui/tests/folder_move.rs`, `crates/ui/tests/fingerprints/dialog-rename-files-*.json`, `crates/ui/tests/fingerprints/dialog-delete-*.json`, `crates/ui/tests/fingerprints/dialog-folder-properties-*.json`
  - scope: `organizing.md` §9 and §10: Rename Files (File ▸ Rename…, F2): "%d files selected. Enter
    a new name:", the example line following the typing ("Example: Lisbon-001.jpg"), Add the date
    taken and Add the image size, Rename running `photo.rename`; the delete questions for one
    photo, several photos and a folder ("… and everything in it (including non-picture files)"),
    and the explanation when a volume has no trash; Delete from Disk… (File menu, context menu,
    Delete in folders, Ctrl+Delete anywhere) and Folder ▸ Delete…; Folder ▸ Move… opening the
    folder picker (scripted in tests, library folders only) and running `folder.move`; Folder
    Properties' Name now editable: OK runs `folder.rename`, then `folder.info.set`; ids per
    control-protocol §7.
  - done when: headless tests on temp folders: F2 on three photos shows "3 files selected" and the
    example follows the typing and the boxes; Rename renames them and Edit ▸ Undo puts the names
    back; each delete question's text matches the spec, Move to Trash trashes and Cancel keeps; Move
    Folder through the scripted picker moves it and the tree follows; renaming in Folder Properties
    renames on disk and the section title follows; dialog fingerprints at both sizes.
- ✅ **M9.21 `engine`: a file operation interrupted at any step loses nothing**
  - after: M9.13, M9.14, M9.15
  - files: `crates/engine/src/fileops_recovery.rs`, `crates/engine/tests/fileops_faults.rs`, `crates/engine/tests/fileops_recovery.rs`
  - scope: `data-model.md` §8 and §10.5, `architecture.md` §8: the file-operation journal replayed
    at open on a worker through `intents::plan` (M9.9): an item rolled forward when its destination
    is complete and verifies (finishing the trash step of a cross-volume move, carrying its entry),
    else back; the repair reported in the log and in `library.inspect`; the fault points of M9.2 and
    of the sidecar writes armed at every step of a move (one volume and across volumes), a copy, a
    batch rename, a delete and a folder move, as Error and as Crash (the engine's tests take `fs`'s
    `faults` feature as `store`'s do).
  - done when: `cargo test -p dalica-engine --test fileops_faults --test fileops_recovery`: for every
    step × {Error, Crash} × each operation, after recovery every photo's bytes exist at a path the
    library lists (once for a move, twice for a copy) with its sidecar entry, and nothing is left in
    the journal; a session reopened after a planted crash repairs the folders and shows each photo
    with its data; the owner thread made no `fs` call.
- ✅ **M9.22 `ui`: search mode: the search field, filters, age slider and banner**
  - after: M9.4, M9.5, M9.11, M9.16
  - files: `crates/ui/src/panels/toolbar.rs`, `crates/ui/src/panels/search_banner.rs`, `crates/ui/src/commands/search.rs`, `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/tags/list.rs`, `crates/ui/src/dialogs/album_properties.rs`, `crates/ui/tests/search_ui.rs`, `crates/ui/tests/toolbar.rs`, `crates/ui/tests/light_table.rs`, `crates/ui/tests/tags_pane.rs`, `crates/ui/tests/fingerprints/region-toolbar-search-*.json`, `crates/ui/tests/fingerprints/region-banner-*.json`, `crates/ui/tests/fingerprints/region-no-results-*.json`
  - scope: `organizing.md` §8 and `ui-layout.md` §3 and §9: the search field live (`search.set` on
    every change; ×, Esc and Show All clear it; Ctrl+F and F3 focus it); its autocomplete of
    `search.suggest` (a tag picked becomes `tag:<tag>`, a folder or album its name); the filter
    toggles (Faces disabled with the reason "Available once Dalica finds faces", M13) with their
    tooltips in the caption; the age slider and its label; the banner ([Show All] and "Showing 33
    pictures in 4 folders (0.01 s) · 439.8 MB", or the duplicates text) with only matching sections
    shown; the scroll position saved when search mode starts and restored when it ends; the empty
    state when nothing matches; Tools ▸ Experimental ▸ Show Duplicate Files and Save Search
    Results… (Album Properties with "Search results for "x"" as its name); Search for This Tag in
    the Tags pane's tag menu; ids per control-protocol §7.
  - done when: `crates/ui/tests/search_ui.rs` (headless, a demo library): typing narrows the light
    table and the banner's text and counts match the spec; each filter and the age combine with
    text; Show All restores the scroll position; an autocomplete pick searches the tag; no match
    shows the empty state; Show Duplicate Files shows the groups and its banner; Save Search
    Results… makes the album; Search for This Tag searches it; toolbar, banner and empty-state
    fingerprints at both sizes.
- ⬜ **M9.23 `ui`: drag and drop, the drag pill, and Cut, Copy and Paste**
  - after: M9.17, M9.18, M9.19
  - files: `crates/ui/src/drag.rs`, `crates/ui/src/panels/light_table/input.rs`, `crates/ui/src/panels/folders.rs`, `crates/ui/src/commands/clipboard.rs`, `crates/ui/tests/drag.rs`, `crates/ui/tests/clipboard.rs`, `crates/ui/tests/fingerprints/drag-*.json`
  - scope: `ui-layout.md` §4, §5.5 and §6.2: a press-and-drag from a thumbnail or the tray carries
    the selected photos (or that one) with M9.5's pill, its text following the target under the
    pointer: a folder row moves (Ctrl copies, the pill shows +), an album row adds, the tray adds
    and holds, a gap between two cells of an album reorders (the insertion marker); targets get the
    `drop_target` highlight; a drop runs `photo.moveTo` or `photo.copyTo` (with their questions),
    `album.add`, `album.reorder` or `tray.hold`; a drop on the photos' own folder or Esc does
    nothing; the tree scrolls near its edges during a drag; `ui.dragWidget` and `ui.inspect` →
    `drag` report it; Cut and Copy keep the photos, Paste into a folder section moves or copies
    them there (disabled in albums and with nothing cut or copied).
  - done when: `crates/ui/tests/drag.rs` (headless, `ui.dragWidget`, temp folders): a thumbnail onto
    a tree folder moves it (asked the first time); with Ctrl it copies; onto an album row it adds;
    onto the tray it holds; a reorder within an album survives a restart; onto its own folder and
    with Esc nothing changes; `crates/ui/tests/clipboard.rs`: Cut then Paste moves, Copy then Paste
    copies, Paste is disabled in an album; pill fingerprints for move, copy and add.
- ⬜ **M9.24 `engine`: M9's data lessons end to end, and a move across two file systems**
  - after: M9.10, M9.21
  - files: `crates/engine/tests/lessons_m9.rs`, `crates/engine/tests/two_volumes.rs`
  - scope: `data-model.md` §10 through the commands and a real watcher: §10.2 (a folder moved
    outside Dalica, between sessions and while watching, keeps stars, captions, tags, recipes and
    album memberships and order), §10.3, §10.7 (odd names through move, copy, rename, delete and
    their undo) and §10.12; a move of photos and of a folder across two real file systems (on Linux
    `/dev/shm` and the temp folder when they are different devices; otherwise the forced path),
    printing which ran.
  - done when: `cargo test -p dalica-engine --test lessons_m9 --test two_volumes` on the three OSes:
    each lesson as a test; the cross-volume move copies, verifies byte for byte, trashes the source
    and undo brings everything back.
- ⬜ **M9.25 `accept M9`**
  - after: M9.12, M9.20, M9.22, M9.23, M9.24
  - files: `tests/acceptance/m9.rs`, `tests/acceptance/m9_*.rs`, `tests/acceptance/m9.md`, `tests/acceptance/fingerprints/m9-*.json`
  - scope: every *Done when* of M9's three areas in the real `dalica --headless` over the control
    channel: `data-model.md` §10.12 (a cross-folder album keeps its order after a rebuild); undo for
    every album command; the grammar tests; search of 100 k photos within its budget (the release
    bench and budgets with `DALICA_ACCEPT_RELEASE=1`; inside `cargo xtask ci` a smaller debug run
    checks the path and prints its times); the banner text and counts against the spec;
    `data-model.md` §10.2, §10.3 and §10.7; undo per file operation; a move across two file systems
    that copies, verifies, then deletes; fingerprints of the album tree, the banner, the drag pill
    and the file dialogs; the manual checks in `m9.md` (make an album from three folders and
    reorder it, search your own photos, drag photos onto a folder and undo, rename a batch, delete
    to the trash and restore it).
  - done when: `cargo xtask accept M9` passes everything in M9's *Done when*.
- ✅ **M9.26 Lead: task blocks for M11**
  - after: M8.26
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M11 task block from M11's outline rows and the spec, with waves and the
    `M11.R` review (owner: maintainer); a block that can port Craft-app code names the source files
    in its scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ✅ **M9.27 `ui`: Ctrl+Z undoes a Save, settling waits for file jobs, the window rests while busy**
  - after: M8.R
  - files: `crates/ui/src/commands/edit_view.rs`, `crates/ui/src/headless.rs`, `crates/ui/src/control/input.rs`, `crates/ui/src/notices.rs`, `crates/ui/tests/save_ui.rs`, `crates/ui/tests/save_as.rs`, `apps/dalica/src/window.rs`, `docs/control-protocol.md`
  - scope: found in the M8 run (M8.19, M8.22, M8.25): in the edit view Ctrl+Z after a Save says
    "Nothing to undo" although the Undo button and Edit ▸ Undo read "Undo Save"; route the key
    to `photo.undoSave` exactly when `photo.history.state` has `undoSave`. `ui.settle {jobs: true}`
    (and the driver's `busy(true)`) does not wait for save and export file jobs; count them. The
    "already exists" toast of Save As names the source photo; name the file the user tried. The
    window polls with `request_repaint_after(16 ms)` while busy, which egui turns into an immediate
    repaint (it subtracts the predicted frame time); poll at ≥ 33 ms as the edit canvas does (M8.29).
  - done when: Ctrl+Z after a Save in the edit view restores the original's exact bytes with the
    edits back as unsaved, and Ctrl+Z with no Save still undoes the last edit; `ui.settle` with
    `jobs` returns only after a running export ends (test); the toast names the name tried; CI green
    on the three OSes.
- ✅ **M9.28 `engine`: the projects directory, scans and write-backs that never race a Save**
  - after: M8.R
  - files: `crates/engine/src/settings.rs`, `crates/engine/src/scanning.rs`, `crates/engine/src/watching.rs`, `crates/engine/src/filejobs.rs`, `crates/engine/src/cmd/save.rs`, `crates/engine/tests/save.rs`, `crates/engine/tests/filejobs.rs`, `docs/data-model.md`
  - scope: found in the M8 run (M8.15-fix #448, M8.22): `Session::projects_dir()` is the export
    location's parent, so a custom location such as `~/Desktop/Out` makes `~/Desktop` a projects
    folder; it is always `<Pictures>/Dalica` (`creating.md` §2). A full scan (start-up or the
    verification pass) already running when a file job starts is not put off as a watcher rescan
    is. A Save of a photo whose metadata write-back is still pending can be refused as "changed on
    disk" (no data at risk): flush that photo's pending writes on the worker before it is read.
  - done when: tests: a custom export location leaves the projects directory at
    `<Pictures>/Dalica`; a full scan running when a Save starts is put off and finds the Save's
    writes unchanged; a caption set immediately before Save is written and the Save succeeds; CI
    green on the three OSes.
- ✅ **M9.29 tests: the flaky waits of M8's run become structural**
  - after: M8.R
  - files: `crates/ui/tests/export_dialog.rs`, `crates/ui/tests/properties_pane.rs`, `crates/image/tests/jxl.rs`
  - scope: tests that failed on loaded runners in the M8 run and passed on re-run:
    `export_dialog.rs` `browse_picks_the_export_location` ("the picker closed", Windows),
    `properties_pane.rs` "the listing scrolled into view" (Ubuntu and a loaded local machine), and
    `jxl.rs` `a_declared_size_past_the_limits_is_refused_before_allocating`, whose 500 ms
    wall-clock bound read 548 ms on macOS. Wait on the event each checks (the picker's answer,
    the scroll settling) rather than on frames; replace the wall-clock bound with a check that the
    refusal allocates nothing (an allocation counter), so nothing is loosened.
  - done when: each test passes 20 of 20 runs pinned to one core beside a CPU load, and fails
    without its fix where the cause is reproducible; CI green on the three OSes.
- ✅ **M9.30 `create` + `engine`: tidy the save path and tighten the rotate budget**
  - after: M8.R
  - files: `crates/create/src/save.rs`, `crates/create/tests/save.rs`, `crates/engine/benches/save_export.rs`, `xtask/src/accept.rs`, `docs/development.md`
  - scope: found in the M8 run (M8.14, M8.24): `create::save::Error::NotYet` is unused since
    M8.14; a Save that ends beside decodes the source twice (check the format before rendering in
    place); the rotate-only Save budget (815 ms) was set from the rendered Save before lossless
    rotation landed — measure the lossless rotation on two cores and set its budget at 1.5 × that
    first measurement (`development.md` §8).
  - done when: no unused error variant; a beside Save decodes once (a decode counter in the test);
    `cargo xtask bench save_export` prints the new rotation number and `cargo xtask budgets` passes
    with the tightened budget.
- ⬜ **M9.R Release 0.4 gate**
  - after: M9.25, M9.27, M9.28, M9.29, M9.30
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 0.4): the lead runs `/milestone-review M9.R` to prepare it, with the
    version set to 0.4.0 and its `whats-new.md` section on the review branch, the release checklist
    and the sign-off question, and after the merge cuts and publishes 0.4.0 (`releasing.md`); the check-in page (make an album
    from photos in three folders, reorder it by dragging, sort it and undo, delete it and undo; find
    Starred Photos and Recently Updated; search by word, tag, date and camera, combine the filters
    and the age slider, save the results as an album; Show Duplicate Files; drag photos onto a
    folder, copy with Ctrl, rename a batch with F2, Move to New Folder, Split Folder Here, delete to
    the trash and undo; move and rename a folder; written as steps for the maintainer to try); the
    questions: `color:` search (`organizing.md` §11), the planning decisions above that change
    behaviour (undoing a delete on macOS is not offered; undoing a copy trashes the copies; folders
    move only within the library; a same-name folder move merges; how Recently Updated counts;
    English month names until M15; the Faces filter disabled until M13; the search budgets);
    *Where we stand* and the `status.md` numbers (search index build and query at 100 k, Find
    Duplicates on 10 k, a cross-volume move) on a `review/M9.R` branch, a pull request open. Only
    the maintainer merges that pull request and closes this issue; nothing after it starts until
    then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M9.1 library albums + ops  ‖  M9.2 fs trash + transfers  ‖  M9.3 engine modules + undo
         ‖  M9.4 ui state + stubs  ‖  M9.5 ui-kit icons, banner, pill, age
         (once M8.R is closed)
wave 2   M9.6 albums + fields on disk  ‖  M9.7 search facts on scan  ‖  M9.8 search grammar
wave 3   M9.9 carry + journal  ‖  M9.10 album commands  ‖  M9.11 search on a worker
wave 4   M9.12 search bench  ‖  M9.13 move + copy  ‖  M9.14 rename  ‖  M9.15 delete + folder move
         ‖  M9.16 duplicates  ‖  M9.17 albums in the app  ‖  M9.18 Add to Album + photo menu
wave 5   M9.19 move dialogs + notices  ‖  M9.20 rename, delete + folder dialogs
         ‖  M9.21 interrupted file operations  ‖  M9.22 search mode
wave 6   M9.23 drag and drop + clipboard  ‖  M9.24 data lessons + two volumes
         (fixes from M8.R, any wave: M9.27 Undo Save key, settle, toast ‖ M9.28 projects dir, scans,
          write-backs ‖ M9.29 flaky waits ‖ M9.30 save path + rotate budget)
wave 7   M9.25 accept M9 → M9.R          (M9.26 lead: M11 blocks, any time after M8.26)
```

### M10 — Preferences, MCP and Release 0.5

One milestone in 3 feature areas (*Preferences and shortcuts*, *MCP and workflows*, *Release 0.5 (P0 complete)*); one `cargo xtask accept M10` covers every *Done when* below. It ends with a release gate the maintainer owns.

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

#### Release 0.5 (P0 complete)

- **Decide at M10.R:** whether release builds include the `heif` feature (HEIC/HEIF). 0.1 leaves it
  out (the maintainer's answer at M6.R, 2026-10-12); measure the decoder against a larger HEIC
  corpus (non-Apple, 10/12-bit, alpha) first.
- **Done when:** every P0 parity row is ✅ with evidence; P0 workflows pass 100 %; performance
  budgets for P0 scenarios are enforced; an honest *Where we stand*; the 0.5 release.

#### M10 tasks

M10 completes the core for Release 0.5: the Options dialog with its General, File Types, Export and
Keyboard Shortcuts tabs, UI scale and font size, the shortcut editor, the MCP server in both modes,
the workflow acceptance tests of every P0 flow, and an audit that brings every P0 parity row to ✅.
Its first wave lists the M9 work each task extends and starts once the maintainer has closed M9.R.
Four tasks lay the ground the others share, so later tasks never share a file: M10.1 registers one
engine command module per area and adds the options catalogue (one table of every option: key,
type, default, range, tab, group, when it applies, whether its feature exists yet) that the
dialog, `options.list` and MCP all read, M10's settings, the new rows of `menus-and-shortcuts.md`
§1.9 and `Session::execute_group` (several commands as one library undo step); M10.2 adds M10's
view state, the UI commands `app.preferences` and `app.shortcuts`, the Options dialog with stub
tabs, the hook that applies UI scale and font size, and stubs for the control methods still to
come; M10.3 gives `ui-kit` the dialog's text tabs, checkbox rows, key cells and the editor's
icons; and M10.4 builds the MCP server's core and `dalica-cli mcp` (framing, the shared tools,
progress and cancellation, the headless backend), with one stub module per later group of tools.
M10.21–M10.24 (the P0 parity gate and workflow statistics, the budgets left, the P0 formats, HEIF
measured on a wider set) start beside them, since no other task touches their files. Each task
updates its `parity.md` rows (`PREF-DIALOG`, `PREF-GENERAL`, `PREF-FILETYPES`, `PREF-EXPORT`
after M10.2 splits the combined `PREF-…` row into one row per tab, `PREF-KEYMAP`, `SYS-MCP`,
`SYS-CONTROL`, `SYS-HEADLESS`, `SYS-COMMANDS`, `SYS-WINDOW` for the title-bar option, the Tools
menu row and the generated `KEY-*` rows; new rows where none fit: `PREF-SCALE` for UI scale and
font size, a Help menu row) and names its commands there; the audit (M10.26) closes every P0 row
left. First fingerprints of the Options tabs, the shortcut editor, the new widgets and icons and
the window at 150 % and Large text are approved in bulk on the M10.R review page.

Whole-window baselines belong to no task, as in M5–M9: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m9-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.

Decisions taken while planning (the check-in lists them for reversal): Options shows the four tabs
whose options exist (General, File Types, Export, Keyboard Shortcuts); Slideshow, Network, People
and Photo Viewer join with their milestones (M11, M13). Inside a tab every option of
`menus-and-shortcuts.md` §5 is listed, and one whose feature does not exist yet (Language until
M15, the import rows until M11, the previous organizer's data until M14) is drawn disabled with
its reason ("Arrives with Import"); its setting is stored only once its feature reads it. Every
option lives in the engine's `settings.json` and changes only through `options.set {values}`
(checked as a whole before anything is written, no undo step, one atomic write, a failed write an
error the dialog shows) and is read with `options.list {tab?}`; the dialog's draft is UI view
state, as PhotoCraft's `prefs_ui.rs` working copy, Apply sends the changed values in one call, and
UI scale and font size preview live from the draft and revert on Cancel. The dialog is 720 × 540
points at 100 % (never larger than the window), text tabs along its top, OK / Cancel / Apply at the
bottom right. UI scale is applied as egui's zoom through `ui-kit`'s existing `UiScale::factor`
(the OS scale and the window's monitor, the 4K rule) and font size through
`Tokens::with_font_size`; the headless driver takes both, so fingerprints can be made at 150 % and
Large. A File Types change rescans the watched folders: a type turned off leaves the light table
and the index, but its sidecar entries stay on disk and are never pruned as missing while the file
exists, so turning it back on brings everything back; the small-picture threshold (64 px,
16–1024) works the same way. The shortcut editor edits a draft keymap; Apply runs `app.setShortcut`
per changed binding and `app.resetShortcuts` for Reset All; a conflict asks inline [Replace]
[Cancel] (as PhotoCraft does, rather than LightCraft's silent take); two slots per (command,
context), so `app.setShortcut` gains `slot` (0 or 1); hold gestures are listed, not editable; the
rows come from a new `keymap.list` query. The MCP server is a port of LightCraft's hand-written
`crates/mcp` (no async runtime, no new dependency), with PhotoCraft's
`crates/automation/src/workspace.rs` as the pattern for roots; tool annotations are derived, so
`CommandSpec` gains no field: `readOnlyHint` and `idempotentHint` for `Query` commands,
`destructiveHint` for `External` commands and for those whose params doc names `confirm` (a test
checks that every command answering `needs_confirm` is marked). Headless MCP keeps its own
selection and tray (ids, held photos, one section at a time, `ui-layout.md` §5.4), since the light
table's selection lives in `ui`; connect mode uses the app's. `apply_edit` appends through
`photo.history.apply` as one step of each photo's own history until M11's Batch Edit gives the
one-library-step form (W07 waits for it); `set_recipe` runs a new engine command `edit.recipe.set`
that refuses to drop an operation it cannot read unless `dropUnknown`; `list_edit_ops` reads a new
catalogue of operations in `edit`; `set_metadata`'s `geo` answers "not available until Places
(M13)". The automation path policy moves from `ui`'s control channel into `engine`
(`automation.rs`), so the control channel and MCP share one implementation; `--read-only` is an
engine switch. `library.inspect` becomes the engine query of `control-protocol.md` §6.4, the control
method adds the UI's fields and MCP's `doc_inspect` returns it. The workflow tests live in
`crates/mcp/tests/workflows/` (one module per workflow, `wNN_<slug>`); the P0 workflows are those
of M10 or earlier (W01–W06, W08, W09, W12); W07, W10 and W11 stay ignored naming their milestones;
`cargo xtask stats` prints the pass rate and `stats --check` (in `cargo xtask ci`) holds its floor;
the run in connect mode against `dalica --headless --control` is part of `accept M10`. A P0 row is
✅ once its P0 behaviour is measured; parts of a P0 row's note that belong to a later milestone's
feature (faces and people, places, the slideshow, video, the previous organizer's data, Batch
Edit's Undo All Edits) move to that feature's row, or a new row where none fits, and the moves are
a question at M10.R; two checks only a person can make (the Windows touchpad pinch on real
hardware, the macOS build used by hand) keep their rows 🟡 "awaiting the M10.R manual check",
which `cargo xtask parity --p0` accepts by that note. `cargo xtask parity` generates the `KEY-*`
rows from the keymap: one per documented (context, key), tier P0 when a P0 row cites its command,
✅ when bound as documented. The wider HEIF set is made locally with libheif's `heif-enc` (a tool
on the maintainer's machine, never a dependency) from `scenes` images at 8, 10 and 12 bits, 4:2:0,
4:2:2 and 4:4:4, with alpha, grids and rotations, plus openly licensed non-Apple files where they
can be found; publishing it to `dalica-corpus` is a question at M10.R. The P0 budgets not enforced
yet (scan throughput, cold start and idle memory at 100 k) join the budgets job at the values of
`development.md` §8 and are never loosened: a miss is fixed in its task or blocks with its numbers.
No other step of M10 needs a maintainer decision mid-run.

- ⬜ **M10.1 `engine`: M10's command modules, the options catalogue and settings**
  - after: M9.3, M9.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/options.rs`, `crates/engine/src/cmd/keymap.rs`, `crates/engine/src/cmd/inspect.rs`, `crates/engine/src/cmd/recipe.rs`, `crates/engine/src/options.rs`, `crates/engine/src/settings.rs`, `crates/engine/src/session.rs`, `crates/engine/src/undo.rs`, `crates/engine/tests/settings_m10.rs`, `crates/engine/tests/options_catalogue.rs`, `crates/engine/tests/execute_group.rs`, `crates/engine/README.md`, `docs/menus-and-shortcuts.md`, `docs/data-model.md`
  - scope: the engine's ground for M10 so M10.5–M10.7, M10.9 and the MCP tasks never share a
    file: empty `cmd/options.rs`, `keymap.rs`, `inspect.rs` and `recipe.rs` joined into the
    registry once; `options.rs`, the options catalogue: one row per option of
    `menus-and-shortcuts.md` §5 General, File Types and Export (settings key, type, default, range
    or choices, tab, group, English label and hint, applies `now | rescan | restart`, whether its
    feature exists yet and the reason when not); every new setting with its default behind a typed
    accessor: Interface (use animations on, show tooltips on, single click returns to the library
    off, merged with M7's edit setting where one exists, UI scale `auto` or 75–300 in steps of 25,
    font size `small`, language `system`) and the small-picture threshold (64; 16–1024), while the
    settings that exist (`fileTypes`, the `files` group, M8's export and M9's ask-first settings)
    are listed in the catalogue, not duplicated; `Session::execute_group(steps, label)`: several
    library commands as one library undo step, every step's `enabled` checked before any runs and
    an error in a step undoing the steps already run; `menus-and-shortcuts.md` §1.9 gains
    `options.list`, `options.set`, `keymap.list`, `library.inspect`, `edit.recipe.set` and
    `app.setShortcut`'s `slot`; `data-model.md` §4 lists the new settings keys.
  - done when: `crates/engine/tests/settings_m10.rs` round-trips every new setting with unknown
    keys kept and wrong types falling back (reported); `crates/engine/tests/options_catalogue.rs`:
    every option of §5's three tabs is in the catalogue once, each default equals its accessor's,
    keys are unique; `crates/engine/tests/execute_group.rs`: a star, a caption and a tag in one
    group are one undo step and undo restores all three, a failing third step leaves the model and
    the sidecars unchanged; every existing engine test and `panic_hunt` pass unchanged.
- ⬜ **M10.2 `ui`: M10's view state, UI commands, the Options dialog's stubs and control stubs**
  - after: M9.4, M9.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/options.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/options.rs`, `crates/ui/src/dialogs/options/general.rs`, `crates/ui/src/dialogs/options/file_types.rs`, `crates/ui/src/dialogs/options/export.rs`, `crates/ui/src/dialogs/options/shortcuts.rs`, `crates/ui/src/scale.rs`, `crates/ui/src/control/mod.rs`, `crates/ui/src/control/view.rs`, `crates/ui/src/control/batch.rs`, `crates/ui/src/control/inspect.rs`, `crates/ui/tests/view_state_m10.rs`, `docs/control-protocol.md`
  - scope: every M10 field of `UiState` in one go: the Options dialog (its tab, the draft of
    option values, the last Apply's error, the font size and scale being previewed) and the
    shortcut editor (search text, Context filter, "Show commands without a shortcut", the row
    recording, a pending conflict, its draft of changed bindings); UI commands `app.preferences`
    (Ctrl+,; Dalica ▸ Preferences… on macOS) and `app.shortcuts` (Ctrl+/; opens the Keyboard
    Shortcuts tab) in `commands/options.rs`; `DialogKind::Options` on the dialog host with the
    frame in `dialogs/options.rs` and four stub tab bodies, so M10.10–M10.12 never share a file;
    a hook in `keys.rs` that sends key presses to the editor, not the keymap, while a row records;
    `scale.rs` with a stub `apply(ctx, state, settings)` that `app.rs` calls every frame; the
    `batch` and `library.inspect` entries of `control/mod.rs` pointing at stub modules, so M10.7
    and M10.8 never share a file; the `PREF-…` row split into one row per tab, `PREF-SCALE` and a
    Help menu row added; control-protocol §6.5 and §6.6 updated.
  - done when: `crates/ui/tests/view_state_m10.rs`: each new field round-trips through `ui.state`
    / `ui.set` (bad values are `bad_params`, nothing changes); Ctrl+, opens `dialog:options` on
    General and Ctrl+/ on Keyboard Shortcuts, Esc closes it; Tools ▸ Options…, Help ▸ Keyboard
    Shortcuts… and the macOS Preferences… item light up; every command's graceful-failure test;
    `panic_hunt` and `widget_ids` stay green.
- ⬜ **M10.3 `ui-kit`: text tabs, checkbox rows, key cells and the editor's icons**
  - after: M9.5, M9.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/tabs.rs`, `crates/ui-kit/src/widgets/checkbox.rs`, `crates/ui-kit/src/widgets/key_cell.rs`, `crates/ui-kit/src/widgets/list_row.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/options.rs`, `crates/ui-kit/tests/options_widgets.rs`, `crates/ui-kit/tests/icons_options.rs`, `crates/ui-kit/tests/fingerprints/options-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-options*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `design-system.md` §5–§6 and §11.3: the text variant of `tabs` (a dialog's tab strip,
    the open tab joined to the page; the edit panel's icon tabs unchanged); `checkbox` on M8.5's
    form rows (a hint line under it, "Applies at next launch.", a disabled state with its reason as
    the tooltip), replacing the egui checkbox the dialogs call today; `key_cell` (one or two keys
    as key caps, ⌘ ⌥ ⇧ glyphs on macOS, "Press the new shortcut…" while recording, a conflict
    state); `list_row` (a 24-high row with columns, hover, selection and a modified mark) for the
    shortcut editor's list; the icons `Restore` (↺) and `Clear` (×), our own drawings on the
    20-unit grid; the widget gallery gains them.
  - done when: `crates/ui-kit/tests/options_widgets.rs`: a strip of four tabs fits at both window
    sizes and elides from its end; a disabled checkbox ignores clicks and shows its reason; key
    caps read "Ctrl+Shift+S" on Linux and "⇧⌘S" on macOS; a row's columns align at both font
    sizes; `crates/ui-kit/tests/icons_options.rs`: both icons stay in their grid at 12, 16, 20 and
    28 px in rest, hover and disabled colours; gallery and icon fingerprints; `widget_ids`,
    `cargo xtask assets` and `cargo xtask names` pass.
- ⬜ **M10.4 `mcp` + `dalica-cli`: the MCP server's core and headless mode**
  - after: M9.3, M9.R
  - files: `crates/mcp/Cargo.toml`, `crates/mcp/README.md`, `crates/mcp/src/lib.rs`, `crates/mcp/src/server.rs`, `crates/mcp/src/jsonrpc.rs`, `crates/mcp/src/backend.rs`, `crates/mcp/src/headless.rs`, `crates/mcp/src/tray.rs`, `crates/mcp/src/progress.rs`, `crates/mcp/src/roots.rs`, `crates/mcp/src/resources.rs`, `crates/mcp/src/connect.rs`, `crates/mcp/src/tools/mod.rs`, `crates/mcp/src/tools/core.rs`, `crates/mcp/src/tools/inspect.rs`, `crates/mcp/src/tools/organize.rs`, `crates/mcp/src/tools/edit.rs`, `crates/mcp/src/tools/output.rs`, `crates/mcp/src/tools/commands.rs`, `crates/mcp/src/tools/ui.rs`, `crates/mcp/tests/server.rs`, `crates/mcp/tests/common/**`, `apps/dalica-cli/src/main.rs`, `apps/dalica-cli/src/mcp.rs`, `apps/dalica-cli/tests/mcp_cli.rs`, `docs/mcp.md`, `NOTICE`
  - scope: `mcp.md` §1, §3, §4.1, §6 and §10, ported from LightCraft `crates/mcp/src/lib.rs`,
    `server.rs`, `tools.rs`, `backend.rs`, `headless.rs` and `progress.rs` at b3d392b4 (headers
    and `NOTICE` rows, `architecture.md` §9): newline-delimited JSON-RPC 2.0 over stdio with a
    reader thread, so `ping` and `notifications/cancelled` are read while a tool runs;
    `initialize` (protocol `2025-06-18`; `2025-03-26` and `2024-11-05` accepted; `serverInfo`,
    capabilities, the `instructions` string); `tools/list` and `tools/call` dispatching to the
    tool groups; the shared tools `command_list`, `command_run` (with `wait`), `command_batch`
    (≤ 256 steps) and `render_preview`; the `Backend` trait with the headless backend (an engine
    `Session` from `--demo`, `--library DIR`, folders, or an empty in-memory library); `tray.rs`,
    headless MCP's own selection and tray; progress notifications (≤ 10 per second plus the final
    one) and cancellation between photos; tool errors as `isError` with the control-protocol code,
    protocol errors `-32700`, `-32600`, `-32601`, `-32602`, `-32002`; strict arguments for the
    helper tools; a panic in a tool turned into `isError: internal`; the limits (1 MiB lines,
    8 MiB results); stub modules for the later tool groups (`inspect`, `organize`, `edit`,
    `output`, `commands`, `ui`), resources, roots (today's default write roots) and connect mode;
    `dalica-cli mcp [--demo] [--library DIR] [--compact] [FOLDERS…]`; `mcp.md`'s status line,
    and §9's location moved to `crates/mcp/tests/workflows/`.
  - done when: `crates/mcp/tests/server.rs` through the real framing on in-process pipes:
    `initialize` per protocol revision; each shared tool on a demo session; an unknown key in a
    helper's arguments is `-32602` naming it; a parse error answers with `id: null` and the next
    line is still read; an over-long line is `-32700`; a planted panic is `isError: internal` and
    the session keeps serving; a progress token gives strictly increasing notifications, a cancel
    stops a 50-photo job before its next photo with the finished ones kept, `ping` is answered
    during a job; `apps/dalica-cli/tests/mcp_cli.rs`: `dalica-cli mcp --demo` answers
    `initialize` on real stdio and logs only to standard error.
- ⬜ **M10.21 `xtask`: the P0 parity gate, `KEY-*` rows from the keymap, and workflow statistics**
  - after: M9.R
  - files: `xtask/src/parity.rs`, `xtask/src/parity/**`, `xtask/src/stats.rs`, `xtask/src/ci.rs`, `docs/parity.md`, `docs/process.md`
  - scope: `parity.md` and `process.md` §5: `cargo xtask parity --p0` lists every P0 row that is
    not ✅ and fails while one remains, except a 🟡 row whose note ends "awaiting the M<n>.R manual
    check" for the current milestone, which it lists apart; `cargo xtask parity --write` generates
    section I's rows from the bindings `dalica-cli commands --json` lists (the registry and
    `UI_COMMANDS`): one `KEY-<context>-<key>` row per key documented in `menus-and-shortcuts.md`
    §2 and `editing.md` §12, tier P0 when a P0 row cites its command and P1 otherwise, ✅ when the
    keymap binds it as documented and ⬜ with the command's milestone otherwise, and a documented
    key with no binding fails the check; `cargo xtask stats` runs the workflow tests and prints
    passing / total with the ignored ones and their reasons, and `stats --check` holds a floor (the
    workflows passing today; M10.20 raises it) and joins `cargo xtask ci`.
  - done when: `xtask` tests: a planted P0 ⬜ row fails `--p0`, an "awaiting" row of the current
    milestone is listed apart while one of another milestone fails; a documented key removed from
    a planted keymap fails; the generated rows are stable between two runs; `stats` counts a
    planted ignored workflow and `--check` fails under the floor; `cargo xtask ci` passes.
- ⬜ **M10.22 `engine` + `ui`: the P0 budgets left: scan throughput, cold start and idle memory**
  - after: M9.12, M9.R
  - files: `crates/engine/benches/scan_throughput.rs`, `crates/ui/benches/cold_start.rs`, `xtask/src/bench.rs`, `xtask/src/accept.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: `development.md` §8's P0 scenarios that no job enforces yet, each bench registered in
    `xtask/src/bench.rs`, joined to `BUDGETS` and the release-budgets job (M5.24) and documented in
    §8 and §13 with its machine: `scan_throughput` (a first scan of 5 000 `scenes` JPEGs with
    embedded thumbnails on the temporary disk, photos per second to "embedded thumbnails
    available"; budget ≥ 500); `cold_start` (a fresh process per run, the bench re-executing
    itself, opening a 100 k index-only synthetic library to the first settled frame with every
    visible cell drawn from its sketch; budget ≤ 2 s) and idle memory (resident memory after that
    frame and 5 s idle, read in-process; budget ≤ 600 MiB); a missed budget is fixed in this task
    (before and after numbers in the PR) or blocks with its numbers, never loosened.
  - done when: `cargo xtask bench scan_throughput` and `cargo xtask bench cold_start` print their
    numbers (in the PR); `cargo xtask budgets` passes with them and `--plant-ms` makes each fail.
- ⬜ **M10.23 `image` + `scenes`: the P0 formats measured**
  - after: M6.38, M9.R
  - files: `crates/image/src/formats/jpeg.rs`, `crates/image/src/formats/png.rs`, `crates/image/src/formats/tiff.rs`, `crates/image/src/decode.rs`, `crates/image/tests/p0_formats.rs`, `crates/scenes/src/formats/**`
  - scope: what the P0 rows `DATA-FMT-JPEG`, `DATA-FMT-PNG`, `DATA-FMT-TIFF`, `DATA-FMT-COLOR`
    and `DATA-FMT-RAW-TIFF` list as not measured, each measured with procedural files where
    `scenes` can make them and with `photos` corpus files otherwise: 12-bit and DCT-scaled JPEG
    decodes and arithmetic coding answering its message; PNG palette, low bit depths and
    interlacing measured exactly; TIFF's thumbnail-IFD fast path called by `decode_thumbnail`; a
    decode found wrong is fixed here; each row's note rewritten to what was measured.
  - done when: `cargo test -p dalica-image --test p0_formats` measures every item those rows' notes
    name; `cargo xtask test-corpus` passes with its floors unchanged or raised.
- ⬜ **M10.24 `image` + `xtask`: HEIF measured on a wider set, for the M10.R decision**
  - after: M6.42, M9.R
  - files: `crates/image/src/formats/heif.rs`, `crates/image/tests/heif_wide.rs`, `xtask/src/corpus.rs`, `xtask/src/corpus_pins.rs`, `docs/formats.md`
  - scope: Release 0.5's open question (the `heif` feature in release builds): a `heif-wide`
    corpus set made by `cargo xtask corpus make heif-wide` from `scenes` images through libheif's
    `heif-enc` where it is installed (8, 10 and 12 bits; 4:2:0, 4:2:2 and 4:4:4; alpha; grids;
    `irot` and `imir`; an ICC profile), plus openly licensed non-Apple files with their provenance
    where they can be found; every file decoded with the `heif` feature, with its error against the
    source image, its time, and what it answers when unsupported; fixes only in our wrapper
    `heif.rs`, a decoder bug in the crate recorded; the set stays local until the maintainer
    answers whether to publish it; `formats.md`'s HEIF section records the measurements and a
    recommendation.
  - done when: `cargo test -p dalica-image --features heif --test heif_wide` (skipped with a message
    when the set is absent) passes on the local set; the table of results is in the PR and in
    `formats.md`; no file panics.
- ⬜ **M10.5 `engine`: `options.list` and `options.set`, and applying them**
  - after: M10.1
  - files: `crates/engine/src/cmd/options.rs`, `crates/engine/src/scanning.rs`, `crates/store/src/stale.rs`, `crates/store/tests/stale.rs`, `crates/engine/tests/options_set.rs`, `crates/engine/tests/file_types.rs`
  - scope: `menus-and-shortcuts.md` §5 and `design-system.md` §11.3, with PhotoCraft
    `crates/engine/src/prefs.rs` (`Preferences::get`, `set`, `reset`) at 583425b4 as the pattern:
    `options.list {tab?}` (query: the catalogue rows with their current values and availability);
    `options.set {values}` (every key and value checked against the catalogue before anything
    changes: an unknown key, a wrong type or a value out of range is `bad_params` naming it; no
    undo step; one atomic write of `settings.json` through the I/O worker; a failed write an error
    with its reason, the session keeping the old values; the result lists the changed keys and
    those that apply at next launch); effects: a File Types or threshold change starts one rescan
    job of the watched folders, the photos of a type turned off leave the index while their
    sidecar entries are neither pruned nor marked missing as long as their file exists (`store`'s
    `stale` checks the disk, not the listing), the metadata write options act from the next write
    and the export defaults from the next `export.defaults`; rows `PREF-GENERAL`,
    `PREF-FILETYPES`, `PREF-EXPORT` (engine half).
  - done when: `crates/engine/tests/options_set.rs`: each option round-trips through
    `options.set` and `options.list` with unknown keys in `settings.json` kept; a batch with one
    bad value changes nothing; a read-only settings file fails with its reason and the old values
    stay; `restart` lists the system title bar; `crates/engine/tests/file_types.rs` on temp
    folders: turning PNG off removes the PNGs from the library, after 31 days on a virtual clock
    and a sidecar write in their folder their entries are still on disk, and turning PNG back on
    brings them back with their stars, captions and recipes; the threshold hides and shows 40 px
    photos; each command's contract and graceful failures; `panic_hunt` green.
- ⬜ **M10.6 `engine`: the keymap listing, two keys per context and named conflicts**
  - after: M10.1
  - files: `crates/engine/src/cmd/keymap.rs`, `crates/engine/src/cmd/app.rs`, `crates/engine/src/keymap.rs`, `crates/engine/tests/keymap_m10.rs`
  - scope: `menus-and-shortcuts.md` §2.0, with LightCraft `crates/ui-egui/src/shortcuts.rs`
    (`parse`, `format`, `ALIASES`, `conflicts`) at b3d392b4 as the pattern for lenient parsing and
    labels: `keymap.list {context?, search?, withoutKeys?}` (query: one row per command and
    context: id, label, menu path, context, keys in effect, default keys, modified, editable (hold
    gestures are not)); search matching labels, menu paths and keys ignoring case and spelling
    ("ctrl+1" finds both Ctrl+1 bindings); `app.setShortcut {command, context, key, slot?,
    replace?}` with two slots per (command, context), a conflict answered as a `conflict` error
    naming the other command's label, its context and the key unless `replace`, which takes the
    key from the other command; `app.resetShortcuts {command?}` as today; `Key::pretty(platform)`
    for the labels on screen (`Ctrl+Shift+S`; `⇧⌘S` on macOS); row `PREF-KEYMAP` (engine half).
  - done when: `crates/engine/tests/keymap_m10.rs`: the listing covers every command of the
    registry and `UI_COMMANDS` once per context it has keys in, and the ones without keys when
    asked; search by name, menu path and key; a second key in slot 1 resolves beside the first; a
    conflict names the other binding and `replace` moves the key; Ctrl+1 in Library and in Edit
    view is no conflict; Star rebound to F8 resolves F8 to `photo.star.toggle` in Library and
    Ctrl+8 no longer does; the rebinding survives a restart with unknown settings keys kept;
    `app.resetShortcuts {}` restores every default binding exactly; each command's contract and
    graceful failures; `panic_hunt` green.
- ⬜ **M10.7 `engine` + `ui` + `mcp`: `library.inspect` everywhere**
  - after: M10.1, M10.2, M10.4
  - files: `crates/engine/src/cmd/inspect.rs`, `crates/engine/tests/library_inspect.rs`, `crates/ui/src/control/inspect.rs`, `crates/ui/tests/control_inspect.rs`, `crates/mcp/src/tools/inspect.rs`, `crates/mcp/tests/inspect.rs`
  - scope: `control-protocol.md` §6.4 and `mcp.md` §4.1, one implementation for three surfaces:
    the engine query `library.inspect {photos?, section?, offset?, limit?}` (revision, demo,
    watched folders, counts, the tree, sections, search, view settings, undo, unsaved edits,
    pending writes, problems, scan; with `photos` the photo list paged, default 200, at most
    1 000; people names withheld unless allowed); the control method of that name returning it
    plus the UI's fields (`current`, `visibleSections`, `selection`, `tray`, `scroll`); MCP's
    `doc_inspect` returning the engine's JSON headless and the control method's in connect mode;
    rows `SYS-CONTROL`, `SYS-MCP`.
  - done when: `crates/engine/tests/library_inspect.rs` on a demo library: every key of §6.4
    present, counts matching `library.query`, paging and a section restricting the photos, a
    hostile offset and limit `bad_params`, people fields absent by default;
    `crates/ui/tests/control_inspect.rs`: the method adds the UI fields and agrees with
    `ui.inspect` on the selection; `crates/mcp/tests/inspect.rs`: `doc_inspect` equals the engine
    query; graceful failures; `panic_hunt` green.
- ⬜ **M10.8 `ui`: `batch`, replies that wait for their job, and the headless capture in the window**
  - after: M10.2
  - files: `crates/ui/src/control/batch.rs`, `crates/ui/src/control/engine.rs`, `crates/ui/src/control/jobs.rs`, `crates/ui/src/control/screenshot.rs`, `crates/ui/tests/control_batch.rs`, `crates/ui/tests/control_wait.rs`, `apps/dalica/src/control/play.rs`, `apps/dalica/tests/control_m10.rs`, `docs/control-protocol.md`
  - scope: `control-protocol.md` §5.3, §6.3 and §6.11, the missing parts of `SYS-CONTROL`:
    `batch {steps, stopOnError?}` (command and method steps in order with the semantics of
    separate requests; `batch`, `auth` and `app.quit` refused as steps; at most 256 steps; a
    `limit` stop when the reply would pass 8 MiB); `engine.execute` of a command that starts a job
    waits for the job and replies with its result, `wait: false` replies `{job, pending: true}` at
    once, and a job still running 55 s into the deadline replies the same; the scans
    (`folders.add`, `library.rebuild`) name their job; `ui.screenshot {headless: true}` in the
    window renders the CPU capture as `--headless` does; §5.3's "Planned, not built yet" note goes.
  - done when: `crates/ui/tests/control_batch.rs`: a mixed batch runs in order with one undo step
    per command, `stopOnError` stops at the first failure and `false` goes on, forbidden steps are
    refused, a 257-step batch is `limit`, an oversized aggregate stops with `limit`;
    `crates/ui/tests/control_wait.rs`: `folders.add` replies after its scan with the job's result,
    `wait: false` gives `{job, pending: true}` and `jobs.wait` then returns the result;
    `apps/dalica/tests/control_m10.rs`: a headless capture through a windowed session's handlers
    equals `--headless`'s byte for byte; graceful failures; `panic_hunt` green.
- ⬜ **M10.9 `edit` + `engine`: the edit-operation catalogue and `edit.recipe.set`**
  - after: M10.1
  - files: `crates/edit/src/catalog.rs`, `crates/edit/tests/catalog.rs`, `crates/engine/src/cmd/recipe.rs`, `crates/engine/tests/recipe_set.rs`, `docs/editing.md`
  - scope: `mcp.md` §4.2, `data-model.md` §3.1 and `editing.md`'s operation tables:
    `edit::catalog()` (every operation of those tables: name, English label, parameters with type,
    range and default, whether it changes the canvas size, its milestone, whether this build
    renders it; checked against the parser, so an operation the parser knows and the catalogue
    lacks fails a test); `edit.recipe.set {ids, recipe, label?, dropUnknown?}` (`recipe` the
    string form or an operation list) replacing each photo's recipe as one step of its own
    history, refusing by name to drop an unknown or `legacy:` operation unless `dropUnknown`,
    every operation validated before any photo changes; rows `EDIT-UNDO` (the agent path),
    `SYS-MCP`.
  - done when: `cargo test -p dalica-edit --test catalog`: every operation the parser knows is
    listed with ranges matching its clamps, defaults render deterministically;
    `crates/engine/tests/recipe_set.rs`: a recipe set on two photos is one step of each history
    and undo restores both, an unknown operation is refused and kept and replaced with
    `dropUnknown`, a bad parameter changes no photo; each command's contract and graceful
    failures; `panic_hunt` green.
- ⬜ **M10.13 `mcp`: the organizing helper tools**
  - after: M10.1, M10.4
  - files: `crates/mcp/src/tools/organize.rs`, `crates/mcp/tests/organize_tools.rs`
  - scope: `mcp.md` §4.2, photo tools acting on the headless tray (M10.4) when `ids` is omitted:
    `scan_folder {path, mode?, wait?}` (`folders.add` / `folders.set`, waiting for its scan job
    with progress per batch; `{folder, section, photos, videos, skipped, problems}`);
    `query_photos` on `library.query` with M9's grammar and filters, paged ≤ 1 000; `photo_info`
    from `photo.inspect`, `photo.metadata` and `photo.history.state`; `select_photos {ids,
    section?, mode?, hold?}`; `set_metadata` through `Session::execute_group` as one undo step
    (`geo` answers "not available until Places (M13)"); `create_album` on `album.create` (the tray
    by default); `jobs_list` and `jobs_cancel`; row `SYS-MCP`.
  - done when: `crates/mcp/tests/organize_tools.rs` on `scenes` folders: `scan_folder` of 200
    photos answers the counts with increasing progress; `query_photos` with words, `tag:`, `-word`
    and filters equals `library.query`; `photo_info` lists every field of §4.2; `select_photos`
    with an unknown id is an error and the selection is unchanged; `set_metadata` with star,
    caption and tags is one undo step that one `cmd_edit_undo` restores; `create_album` from the
    tray; each tool's strict arguments and graceful failures.
- ⬜ **M10.16 `engine` + `mcp`: roots, permissions and read-only sessions**
  - after: M10.1, M10.4
  - files: `crates/engine/src/automation.rs`, `crates/engine/src/session.rs`, `crates/engine/tests/automation_roots.rs`, `crates/engine/tests/read_only.rs`, `crates/ui/src/control/paths.rs`, `crates/mcp/src/roots.rs`, `crates/mcp/tests/roots.rs`, `docs/security.md`, `NOTICE`
  - scope: `mcp.md` §5 and `control-protocol.md` §5.6–§5.7, with PhotoCraft
    `crates/automation/src/workspace.rs` (`AuthorizedWorkspace`) at 583425b4 as the pattern: the
    path policy moved from `ui`'s `control/paths.rs` into `engine` (`automation.rs`: read and
    write roots, the default write roots (the OS temporary directory and
    `<cache>/Dalica/automation/`), never inside a watched folder or `.dalica-originals/`, never
    over an existing file, links resolved before the check), the control channel calling it; MCP's
    client roots (`roots/list` after `initialize` and on `notifications/roots/list_changed`,
    `file://` roots as read roots only, `--no-client-roots`); `--automation-read-root`,
    `--automation-write-root`, `--automation-allow-external` and `--automation-allow-people` in
    headless mode; `--read-only` as an engine switch (sidecar, metadata and index writes stay in
    memory; file jobs and exports refused with `read_only`); `security.md`'s automation section
    updated; rows `SYS-MCP`, `SYS-CONTROL`.
  - done when: `crates/engine/tests/automation_roots.rs`: allowed and refused paths (`..`, a link
    out of a root, a watched folder, `.dalica-originals/`, an existing file) and the control
    channel's path tests passing unchanged; `crates/mcp/tests/roots.rs`: a client root becomes a
    read root and never a write root, a `list_changed` refresh drops a removed root;
    `crates/engine/tests/read_only.rs`: starring and captioning changes no byte on disk (the
    folder's sha256 before and after) and an export is refused; the owner thread made no `fs`
    call; `panic_hunt` green.
- ⬜ **M10.25 `ui`: the window's last P0 gaps**
  - after: M10.2
  - files: `crates/ui/src/dialogs/folder_properties.rs`, `crates/ui/src/dialogs/folder_manager.rs`, `crates/ui/src/dialogs/folder_manager/**`, `crates/ui/src/menus.rs`, `crates/ui/tests/folder_properties.rs`, `crates/ui/tests/folder_manager.rs`, `crates/ui/tests/menu_checks.rs`, `crates/ui/tests/fingerprints/dialog-folder-properties-*.json`, `crates/ui/tests/fingerprints/dialog-folder-manager-*.json`
  - scope: gaps named in P0 rows' notes that no other M10 task closes: Folder Properties' date on
    `ui-kit`'s date fields (M6.29's `datetime`) instead of typed text (`ORG-FOLDERPROPS`); a
    resizable Folder Manager keeping its size for the session (`ORG-FOLDERMGR`); every checkable
    menu item's mark read from the view state (`SYS-COMMANDS`); any other P0 gap in these files
    the audit's first pass names.
  - done when: `crates/ui/tests/folder_properties.rs`: the date fields step and refuse an
    impossible date, OK runs `folder.info.set` once; `crates/ui/tests/folder_manager.rs`: the
    dialog resizes within the window and its list follows; `crates/ui/tests/menu_checks.rs` lists
    every checkable item and toggles each through its command and through `ui.set`; fingerprints
    at both sizes.
- ⬜ **M10.10 `ui`: the Options dialog, its General tab, UI scale and font size**
  - after: M10.2, M10.3, M10.5
  - files: `crates/ui/src/dialogs/options.rs`, `crates/ui/src/dialogs/options/general.rs`, `crates/ui/src/scale.rs`, `crates/ui-kit/src/widgets/tooltip.rs`, `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/edit/canvas.rs`, `crates/ui/src/commands/zoom.rs`, `crates/ui/src/commands/scroll.rs`, `crates/ui/tests/options_dialog.rs`, `crates/ui/tests/ui_scale.rs`, `crates/ui/tests/fingerprints/dialog-options-general-*.json`, `crates/ui/tests/fingerprints/ui-scale-*.json`, `docs/design-system.md`
  - scope: `design-system.md` §11.3–§11.5 and `menus-and-shortcuts.md` §5 General, with PhotoCraft
    `crates/ui-egui/src/prefs_ui.rs` (`prefs_body`, `preferences_changed`, `apply`, `confirm`,
    `sync_display_scale`) and `crates/ui-egui/src/dialogs.rs` (`show`'s OK / Cancel / Apply) at
    583425b4 as the pattern: the dialog frame with M10.3's text tabs, filled from `options.list`;
    OK, Cancel and Apply (Apply disabled while the draft equals the saved values, OK applies and
    closes, Cancel and Esc discard the changes since the last successful Apply, a failed Apply
    shows its error in the dialog and keeps the draft, "Applies at next launch." under the system
    title bar option); the General tab's Interface, Files and Import groups on M8.5's form rows,
    options whose feature does not exist drawn disabled with their reason; `scale.rs` applying UI
    scale as egui's zoom (Auto from the native scale and the window's monitor with the 4K rule,
    updated when the window moves to another monitor; the headless driver takes a given scale)
    and font size through `Tokens::with_font_size`, previewing live from the draft and reverting
    on Cancel, `keys.rs`'s zoom assertion holding at every scale; "Show tooltips" off stops
    tooltips; "Use animations" off makes the light table's scroll to a section, the zoom and the
    pan glide jump to their end; rows `PREF-DIALOG`, `PREF-GENERAL`, `PREF-SCALE`, `SYS-WINDOW`.
  - done when: `crates/ui/tests/options_dialog.rs` (headless, by id): Apply's state follows the
    draft; a checkbox applied then the dialog cancelled keeps the applied value; Cancel after an
    unapplied change keeps the old value; a planted write failure shows the error and keeps the
    draft; Large font previews and reverts on Cancel; every option of the tab reaches
    `options.set` with its key; `crates/ui/tests/ui_scale.rs`: the factor for system scales 100,
    125 and 150 % and a 3840 px monitor, a fixed 150 % on a 200 % system; fingerprints of the
    General tab and of the main window at 150 % and Large at both window sizes.
- ⬜ **M10.14 `mcp`: the edit, render and export helper tools**
  - after: M10.4, M10.9
  - files: `crates/mcp/src/tools/edit.rs`, `crates/mcp/src/tools/output.rs`, `crates/mcp/tests/edit_tools.rs`, `crates/mcp/tests/output_tools.rs`
  - scope: `mcp.md` §4.2 and §10: `list_edit_ops {op?}` from `edit::catalog()`; `get_recipe {id}`
    (`{recipe, ops, redo, saved}`); `set_recipe {id, recipe, label?, drop_unknown?}` through
    `edit.recipe.set`; `apply_edit {ids?, op, params?}` through `photo.history.apply` on each
    photo, with the edit view's rules (only the last crop counts, Tuning merges into a trailing
    `tuning`); `render_photo {id?, size?, format?, before?, recipe?, path?}` by `edit::render`
    (long edge ≤ 2048, PNG ≤ 5 MiB, `path` only inside a write root, M10.4's default policy until
    M10.16; nothing changes in the library); `export` through `export.run` with M8's options
    mapped, progress and cancellation per photo; row `SYS-MCP`.
  - done when: `crates/mcp/tests/edit_tools.rs` and `output_tools.rs`: `list_edit_ops` matches the
    catalogue; `set_recipe` refuses an unknown operation unless `drop_unknown`; `apply_edit {op:
    "crop"}` twice keeps one crop; `render_photo` with `before` equals the decoded original and
    with `recipe` changes nothing; a 2049 px request and a `path` outside the write roots are
    refused; `export` of 5 photos at 1600 px writes 5 decodable JPEGs ≤ 1600 on the long edge and a
    cancel leaves only complete files; each tool's strict arguments and graceful failures.
- ⬜ **M10.15 `mcp`: one tool per command, and the resources**
  - after: M10.4, M10.7
  - files: `crates/mcp/src/tools/commands.rs`, `crates/mcp/src/resources.rs`, `crates/mcp/tests/command_tools.rs`, `crates/mcp/tests/resources.rs`, `docs/mcp.md`, `docs/architecture.md`
  - scope: `mcp.md` §4.4 and §7, after LightCraft `crates/mcp/src/tools.rs` (`command_tools`,
    `command_tool_name`, `annotations`) at b3d392b4: `cmd_` + the command id with `.` → `_` for
    every engine command (and the app's `UI_COMMANDS` in connect mode), the description from the
    label, menu path, shortcut and params doc, a loose argument object; the annotations derived as
    decided above, `openWorldHint` false everywhere; `--compact` leaves these tools out while
    `command_run` still reaches every command; `resources/list` and `resources/read` for
    `dalica://library`, `dalica://commands`, `dalica://selection`, `dalica://photo/{id}` (a
    template) and `dalica://edit-ops`, live and never cached, an unknown URI `-32002`; `mcp.md`
    §4.4 and `architecture.md` §3 say how the hints are derived; row `SYS-MCP`.
  - done when: `crates/mcp/tests/command_tools.rs`: one `cmd_` tool per registry entry;
    `cmd_photo_star_toggle` stars and undoes; every command answering `needs_confirm` is
    `destructiveHint` and every `Query` `readOnlyHint`; `--compact` lists only the helpers;
    `cmd_photo_delete` without `confirm` answers `needs_confirm` and changes nothing;
    `crates/mcp/tests/resources.rs`: each URI reads, a photo URI with a bad id is `-32002`, a
    change between two reads shows.
- ⬜ **M10.11 `ui`: the File Types and Export tabs**
  - after: M10.10
  - files: `crates/ui/src/dialogs/options/file_types.rs`, `crates/ui/src/dialogs/options/export.rs`, `crates/ui/tests/options_tabs.rs`, `crates/ui/tests/fingerprints/dialog-options-file-types-*.json`, `crates/ui/tests/fingerprints/dialog-options-export-*.json`
  - scope: `menus-and-shortcuts.md` §5 File Types and Export: "Show JPEG files and:" with one
    checkbox per type (HEIC/HEIF disabled with "HEIC support isn't included in this build" in a
    build without `heif`; RAW formats with a list… popup of the raw kinds) and the small-picture
    threshold; an applied File Types change says "Rescanning your folders…" through the activity
    indicator; Export: the default location with Browse… (the folder picker), the default size and
    quality (M8.5's slider with stops and the quality dropdown), remove location, the default
    watermark text: the same settings the Export dialog starts from; rows `PREF-FILETYPES`,
    `PREF-EXPORT`.
  - done when: `crates/ui/tests/options_tabs.rs` (headless, a temp library): PNG turned off and
    applied hides the PNGs and turned on shows them with their stars; the HEIC row's state follows
    the feature; a size set here is the next Export dialog's default; Browse… with the scripted
    picker fills the location and cancel changes nothing; fingerprints of both tabs at both
    sizes.
- ⬜ **M10.12 `ui`: the shortcut editor and Help ▸ Keyboard Shortcuts**
  - after: M10.6, M10.10
  - files: `crates/ui/src/dialogs/options/shortcuts.rs`, `crates/ui/src/dialogs/options/shortcuts/**`, `crates/ui/tests/shortcut_editor.rs`, `crates/ui/tests/fingerprints/dialog-options-shortcuts-*.json`, `NOTICE`
  - scope: `menus-and-shortcuts.md` §2.0, ported from LightCraft
    `crates/ui-egui/src/panels/keymap.rs` (`body`, `row`, `capture`, `fixed_keys`) at b3d392b4 and
    PhotoCraft `crates/ui-egui/src/prefs_ui.rs` (`shortcuts_body`, `shortcut_text`,
    `is_modifier_key`) with the tests of `crates/ui-egui/src/shortcut_capture_tests.rs` at
    583425b4 (headers and `NOTICE` rows): the search field, the Context filter and "Show commands
    without a shortcut"; one `list_row` per command and context from `keymap.list` (menu path and
    label, context, both key cells, ↺ when it differs from the default); a click on a key cell
    records the first non-modifier key with its modifiers, Esc cancels, Backspace removes; a
    conflict shows inline ("Ctrl+1 is already Small Thumbnails in Library." [Replace] [Cancel]);
    Reset All asks "Restore all default shortcuts?" [Reset All] [Cancel]; changes stay in the
    draft until Apply or OK run `app.setShortcut` and `app.resetShortcuts`, and the menus show the
    keys in effect right after; Help ▸ Keyboard Shortcuts… (Ctrl+/) opens this tab; rows
    `PREF-KEYMAP`, the Help menu row.
  - done when: `crates/ui/tests/shortcut_editor.rs` (headless, by id): "ctrl+1" lists both
    bindings; the Library filter hides Edit view rows; recording F8 on Star, Apply, then F8 on a
    selected photo stars it and Ctrl+8 no longer does; Esc cancels recording and Backspace
    removes; recording Ctrl+1 on Star in Library asks and Replace takes it from Small Thumbnails;
    Cancel before Apply changes nothing; Reset All restores every default and the ↺ marks go;
    Ctrl+/ opens this tab; the editor's fingerprints at both sizes.
- ⬜ **M10.17 `mcp` + `dalica-cli`: connect mode**
  - after: M10.8, M10.13, M10.15
  - files: `crates/mcp/src/connect.rs`, `crates/mcp/src/tools/ui.rs`, `crates/mcp/tests/connect.rs`, `apps/dalica-cli/src/run.rs`, `apps/dalica-cli/tests/run_connect.rs`
  - scope: `mcp.md` §1 and §4.3, after LightCraft `crates/mcp/src/backend.rs` (`Remote`) at
    b3d392b4: `--connect [HOST:PORT]` and `--control-token-file`, loopback only; starting before
    the app runs and connecting on the first call; one authenticated connection; a request that
    fails in transit drops it and answers that the operation may have completed, never resending;
    reply lines with a mismatched `id` skipped; the helper tools forwarded as `engine.execute`,
    `render_*` through the app's session, jobs cancelled through `jobs.cancel`; the UI tools of
    §4.3 (`screenshot` as image content after a settle, `inspect_ui`, `list_widgets`, `click`,
    `drag`, `press_key`, `type_text`, `scroll`, `menu_invoke`, `settle`, `control_call`), which in
    headless mode answer `isError` explaining `--control` and `--connect`; `tools/list` with
    `listChanged`; `dalica-cli run --connect HOST:PORT [--script FILE] [--keep-going]` on the same
    client; rows `SYS-MCP`, `SYS-HEADLESS`.
  - done when: `crates/mcp/tests/connect.rs` against an in-process fake control server: auth
    first; a dropped connection answers "may have completed" and the next call reconnects; a
    mismatched id is skipped; a reply timeout never resends; each UI tool maps to its method;
    headless UI tools answer how to connect; `apps/dalica-cli/tests/run_connect.rs`: a script over
    the fake server prints one JSON line per step and exits non-zero on a failure without
    `--keep-going`.
- ⬜ **M10.18 `mcp`: the workflow harness and the M6 and M8 workflows**
  - after: M10.13, M10.14, M10.15
  - files: `crates/mcp/tests/workflows/main.rs`, `crates/mcp/tests/workflows/harness.rs`, `crates/mcp/tests/workflows/w01_organize_export.rs`, `crates/mcp/tests/workflows/w02_tray_across_folders.rs`, `crates/mcp/tests/workflows/w03_save_undo_revert.rs`, `crates/mcp/tests/workflows/w04_rebuild_loses_nothing.rs`, `crates/mcp/tests/workflows/w05_move_carries_data.rs`, `crates/mcp/tests/workflows/w06_search_save_results.rs`, `crates/mcp/tests/workflows/w07_batch_edit_one_step.rs`, `crates/mcp/tests/workflows/w08_foreign_metadata.rs`, `crates/mcp/tests/workflows/w09_hidden_photos.rs`, `crates/mcp/tests/workflows/w10_import_duplicates.rs`, `crates/mcp/tests/workflows/w11_effects_stack.rs`, `crates/mcp/tests/workflows/w12_destructive_asks.rs`
  - scope: `mcp.md` §9, with PhotoCraft `crates/automation/tests/agent_tasks.rs` at 583425b4 as
    the pattern: `harness.rs` (the server over the real JSON-RPC framing on in-process pipes,
    `scenes` folders in a temporary directory, checks through public state only (`doc_inspect`,
    `photo_info`, `query_photos`) and the files on disk (sidecars, XMP and IPTC read back by
    `dalica-meta`, sha256 of originals), an OS trash redirected to a test directory); one module
    per workflow, W01, W04, W05, W06 and W12 as `#[ignore = "M10.19: …"]` or `"M10.20: …"` stubs
    for those tasks and W07, W10, W11 ignored with their milestones (M11, M12); this task's
    workflows: W02 (tray across folders, rotate the tray, both folders' sidecars, one undo), W03
    (Save, Undo Save, Revert to the exact original bytes), W08 (foreign metadata byte-identical,
    pixels untouched, modification time unchanged) and W09 (hidden photos leave and come back);
    row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp --test workflows` runs W02, W03, W08 and W09 green on the
    three OSes and lists the other eight as ignored with their reasons.
- ⬜ **M10.19 `mcp`: the M9 workflows**
  - after: M10.18
  - files: `crates/mcp/tests/workflows/w04_rebuild_loses_nothing.rs`, `crates/mcp/tests/workflows/w05_move_carries_data.rs`, `crates/mcp/tests/workflows/w06_search_save_results.rs`, `crates/mcp/tests/workflows/w12_destructive_asks.rs`
  - scope: `mcp.md` §9: W04 (star, caption, tag, hide, edit and album, then `library.rebuild
    {confirm}`: `doc_inspect` and `photo_info` equal before and after); W05 (a move of 3 starred,
    captioned, saved photos carries their sidecar entries and originals backups, nothing left
    behind, undo moves them back); W06 (10 tagged, 2 captioned "tram", `query_photos` and `tag:`
    queries, the search saved as an album); W12 (`cmd_photo_delete` without `confirm` answers
    `needs_confirm` with the file still there; with `confirm` the file is in the test trash and the
    undo record keeps its sidecar entry).
  - done when: `cargo test -p dalica-mcp --test workflows` runs W04, W05, W06 and W12 green on the
    three OSes, with the files on disk checked after each step and after undo.
- ⬜ **M10.20 `mcp`: W01, the stdio smoke run and the workflow floor**
  - after: M10.17, M10.18, M10.21
  - files: `crates/mcp/tests/workflows/w01_organize_export.rs`, `apps/dalica-cli/tests/mcp_process.rs`, `xtask/src/stats.rs`, `docs/mcp.md`, `docs/process.md`
  - scope: `mcp.md` §9 and `process.md` §5: W01, the headline (`scan_folder` 200 photos → star 5
    → `create_album` from them → caption 3 → crop and Auto Fix one → `export` the album at 1600 px
    → 5 JPEGs with the long edge ≤ 1600, captions in XMP and IPTC, the crop visible in the output,
    originals byte-identical); a smoke subset (W02, W03) through `dalica-cli mcp` spawned with real
    stdio; the floor in `xtask/src/stats.rs` raised to the nine P0 workflows; `mcp.md`'s status
    line says what is built and `process.md` §5 names the stats command; row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp --test workflows` runs W01 green on the three OSes;
    `apps/dalica-cli/tests/mcp_process.rs` passes; `cargo xtask stats` prints 9 / 12 passing and
    `cargo xtask stats --check` fails when a P0 workflow is ignored (a planted `#[ignore]`).
- ⬜ **M10.26 `docs`: the P0 parity audit**
  - after: M10.8, M10.11, M10.12, M10.16, M10.19, M10.20, M10.21, M10.22, M10.23, M10.25
  - files: `docs/parity.md`
  - scope: `process.md` §5 and the decisions above: every P0 row re-measured (its cited tests run,
    its evidence checked against the code), turned ✅ only with that evidence; parts of a note that
    belong to a later milestone's feature moved to that feature's row or a new row (face-aware crop
    suggestions, the People group, person sections, the Remove Faces question, face, people and
    place fields in sidecars and metadata: M13; Undo All Edits: M11; slideshow music and the
    Slideshow and Collage buttons: M11, M14; video badges: M15; the previous organizer's notice
    and `data-model.md` §10.11: M14; Help Contents: a new `SYS-HELP` row), listed in the PR for the
    M10.R question; the two manual checks keep `EDIT-ZOOM` (the Windows touchpad pinch) and
    `SYS-WINDOW` (the macOS build by hand) 🟡 "awaiting the M10.R manual check"; any other gap
    gets a fix task from the lead, and this task waits for it.
  - done when: `cargo xtask parity --p0` passes, listing only the two awaiting rows; the summary is
    regenerated; the PR lists every row changed with its evidence and every part moved with its new
    row.
- ⬜ **M10.27 `accept M10`**
  - after: M10.12, M10.16, M10.20, M10.26
  - files: `tests/acceptance/m10.rs`, `tests/acceptance/m10_*.rs`, `tests/acceptance/m10.md`, `tests/acceptance/fingerprints/m10-*.json`
  - scope: every *Done when* of M10's three areas in the real `dalica --headless` over the control
    channel and through `dalica-cli mcp`: settings round-tripping through `options.set` with unknown
    keys kept; the keymap conflict tests; Star rebound to F8 through `app.setShortcut` and F8
    pressed headless starring the photo; Reset All restoring every default; every P0 workflow over
    MCP (the workflow tests, the stdio smoke run, and W01 and W02 in connect mode against `dalica
    --headless --control`, showing both modes agree) with the pass rate printed; `cargo xtask
    parity --p0`; the P0 budgets (the release budgets with `DALICA_ACCEPT_RELEASE=1`; inside
    `cargo xtask ci` a smaller debug run checks the paths and prints its times); fingerprints of the
    Options tabs, the shortcut editor and the window at 150 % and Large; the manual checks in
    `m10.md` (Options at 150 % and Large, a File Types round trip, a rebind with a conflict, Reset
    All, `claude mcp add dalica -- dalica-cli mcp --demo` with a W01-like request, connect mode to a
    running `dalica --control`, the Windows touchpad pinch, the macOS build by hand).
  - done when: `cargo xtask accept M10` passes everything in M10's *Done when*.
- ⬜ **M10.28 Lead: task blocks for M12**
  - after: M9.26
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M12 task block from M12's outline rows and the spec, with waves and the
    `M12.R` release gate (owner: maintainer); a block that can port Craft-app code names the source
    files in its scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M10.R Release 0.5 gate**
  - after: M10.24, M10.27
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 0.5, P0 complete): the lead runs `/milestone-review M10.R` to
    prepare it, with the version set to 0.5.0 and its `whats-new.md` section on the review branch,
    the release checklist and the sign-off question, and after the merge cuts and publishes 0.5.0
    (`releasing.md`) with the release notes' honest deltas (P0 rows, the workflow pass rate, the
    budgets); the check-in page (open Options with Ctrl+, and try Apply, OK and Cancel, set UI scale
    150 % and font size Large, turn PNG off and on again and see the stars come back, set export
    defaults and open Export, rebind Star to F8, try a conflict and Replace, Reset All, Help ▸
    Keyboard Shortcuts, add `dalica-cli mcp --demo` to Claude Code and ask for the W01 task, connect
    to a running `dalica --control`, and the two manual checks: the Windows touchpad pinch on real
    hardware and the macOS build used by hand; written as steps for the maintainer to try); the
    questions: whether release builds include the `heif` feature (with M10.24's measurements),
    publishing the `heif-wide` set to `dalica-corpus`, the parts of P0 rows the audit moved to
    later rows, the planning decisions above that change behaviour (four tabs shown and unavailable
    options disabled; File Types rescanning while keeping data; conflicts asked before replacing;
    `apply_edit` as per-photo steps until M11; headless MCP's own tray; derived destructive hints),
    the look of the Options dialog (approved with its fingerprints); *Where we stand* and the
    `status.md` numbers (the workflow pass rate as the headline, the P0 rows, scan throughput, cold
    start, idle memory, the HEIF results) on a `review/M10.R` branch, a pull request open. Only the
    maintainer merges that pull request and closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M10.1 engine modules + options catalogue  ‖  M10.2 ui state + stubs  ‖  M10.3 ui-kit widgets
         ‖  M10.4 mcp core + headless  ‖  M10.21 P0 gate, KEY rows, stats  ‖  M10.22 P0 budgets
         ‖  M10.23 P0 formats  ‖  M10.24 HEIF wider set
         (once M9.R is closed)
wave 2   M10.5 options commands  ‖  M10.6 keymap listing  ‖  M10.7 library.inspect
         ‖  M10.8 batch, waits, capture  ‖  M10.9 op catalogue + recipe.set  ‖  M10.13 organize tools
         ‖  M10.16 roots + read-only  ‖  M10.25 window P0 gaps
wave 3   M10.10 Options + General + scale  ‖  M10.14 edit + output tools
         ‖  M10.15 command tools + resources
wave 4   M10.11 File Types + Export tabs  ‖  M10.12 shortcut editor  ‖  M10.17 connect mode
         ‖  M10.18 workflow harness + W02 W03 W08 W09
wave 5   M10.19 W04 W05 W06 W12  ‖  M10.20 W01, stdio smoke, floor
wave 6   M10.26 P0 parity audit
wave 7   M10.27 accept M10 → M10.R          (M10.28 lead: M12 blocks, any time after M9.26)
```

### M11 — Import, compare and viewing

One milestone in 4 feature areas (*Rooms and import*, *Compare and batch edit*, *Slideshow and quick preview*, *Photo viewer*); one `cargo xtask accept M11` covers every *Done when* below. It ends with a release gate the maintainer owns (Release 0.6).

#### Rooms and import

- **In:** the room tab strip (`ui-layout.md` §8; Library and Import here, Collage joins in M14);
  the Import room (`organizing.md` §3): sources (cards and removable drives, a folder, recent
  sources), the import tray grouped by capture time with the clock that selects a group, Exclude
  duplicates, the preview with star, exclude, rotate and stepping, the destination strip (Import
  to, Folder name with Date taken and Today's date, After copying), the hint line, Import All /
  Import Selected / Cancel; copying that verifies every file and never overwrites one; removing the
  copied files from a card only after verification and an explicit question; Options ▸ General's
  import rows.
- **Out:** cameras and phones over MTP/PTP (M15).
- **Reuse** (`architecture.md` §9): LightCraft `crates/engine/src/devices.rs` (volumes with a
  `DCIM` folder, a cached device list refreshed off the calling thread) for cards; LightCraft
  `crates/engine/src/import.rs` (duplicates by content against the library and within the batch)
  and `crates/ui-egui/src/import.rs` (the review grid filled from a worker) as the pattern;
  `ui-kit`'s tab strip (`fit`, ported from craft-ui `tabs` in M7) for the room tabs.
- **Done when:** `cargo xtask accept M11`: a simulated card imports into dated folders with every
  copy verified; a second import of the same card skips every photo as a duplicate; no file is
  ever overwritten (a planted name clash gets `-001`); removal from the card happens only after
  verification and the question, and is refused over automation; W10 passes over MCP.

#### Compare and batch edit

- **In:** the compare toggle's A│B and A│A (`editing.md` §4) with Switch Focus, Switch Layout and
  both questions; before/after while `\` or Show original is held; Edit ▸ Copy All Edits and Paste
  All Edits; Picture ▸ Batch Edit ▸ (`editing.md` §9) and Undo All Edits.
- **Out:** Batch Edit's effects (Sepia, Sharpen, Warmify, Film Grain, Black and White) render
  with the effects (M12); until then they are greyed with their reason.
- **Reuse:** LightCraft `crates/ui-egui/src/panels/compare.rs` (the pair, synced zoom and pan,
  switching sides) and `tests_before_after.rs` as the pattern.
- **Done when:** `cargo xtask accept M11`: a batch of 100 photos is one undo step and one undo
  restores every recipe; a version kept from A│A is one step of the photo's history; W07 passes
  over MCP.

#### Slideshow and quick preview

- **In:** the slideshow (`viewing.md` §1): what plays from each entry, full-screen presentation at
  screen resolution, the seven transitions, captions, the control bar with its keys and mouse,
  Loop, Options ▸ Slideshow; the Ctrl+Alt quick full-screen preview with wheel stepping (§2).
- **Out:** slideshow music (a question at M11.R: playing sound needs a dependency only the
  maintainer can allow) and videos with their sound (M15); Pan and Zoom toward a face (M13; toward
  the centre until then).
- **Done when:** `cargo xtask accept M11`: transitions never wait on disk (the prefetch test, with
  a slowed decoder); each transition has a fingerprint at its midpoint; keys, wheel and the bar
  follow `viewing.md` §1.3–§1.4.

#### Photo viewer

- **In:** `dalica --view FILE` as the standalone viewer (`viewing.md` §3): the borderless
  full-screen window over a translucent backdrop (opaque where the platform cannot), the filmstrip
  of the file's folder, the control bar (zoom, 1:1, Open in Dalica, Play, rotate, star, More), keys
  and mouse, Settings… and Options ▸ Photo Viewer (Interface), Tools ▸ Configure Photo Viewer….
- **Out:** default-viewer registration and the File Types tab, Open With ▸ and Show in File
  Manager (OS integration, M15).
- **Reuse:** LightCraft `crates/ui-egui/src/panels/detail.rs` (filmstrip, zoom and pan) as the
  pattern.
- **Done when:** `cargo xtask accept M11`: `dalica --view` opens a photo, steps through its folder,
  stars it from the viewer (the star lands in the folder's sidecar) and opens it in the edit view.

#### M11 tasks

M11 brings photos in and shows them off: the Import room for cards, drives and folders, compare and
batch editing, the slideshow with its transitions, the quick full-screen preview and the standalone
photo viewer. Its first wave lists the M10 work each task extends and starts once the maintainer has
closed M10.R. Six tasks lay the ground the others share, so later tasks never share a file: M11.1
registers one engine command module per area, with stub state for the import session, A│A's
variant and the viewer, and adds M11's settings and the Slideshow and Photo Viewer rows of the
options catalogue; M11.2 adds M11's view state, UI commands with stub handlers, the rooms' frame,
stub panels and dialogs and the two new Options tabs; M11.3 gives `ui-kit` the room tabs, the
overlay bar, the stepper, the caption plate, the import tray's group header and the icons; M11.4
writes the slideshow's sequencing and transition geometry as pure code in `create`, where
`architecture.md` §2 puts it; M11.5 finds cards and drives and lists an import source in `scan`;
M11.6 gives `fs` the import copy and the removal of a card's copied files. Import's listing (M11.7)
and its copying (M11.8) are separate engine modules, and the room's two halves (M11.15, M11.20)
follow them; compare needs the preview's second pane (M11.10) before its UI (M11.16), and the
slideshow, the quick preview and the viewer read previews only through the API M11.10 leaves. Each
task updates its `parity.md` rows (`LIB-ROOMS`, `IMP-ROOM`, `IMP-DEVICES`, `IMP-DUP`, `IMP-NAMING`,
`IMP-AFTER`, `EDIT-COMPARE`, `EDIT-RECIPE-COPY`, `EDIT-BATCH`, the Undo All Edits part of
`EDIT-UNDO`, the compare toggle and the caption bar's slideshow toggle in `EDIT-VIEW`,
`VIEW-SLIDESHOW`, Loop in `VIEW-MUSIC`, `VIEW-VIEWER`, `LIB-PEEK`, the Ctrl+Alt part of
`LIB-HOVER`, the Slideshow button in `LIB-ACTIONS`, Import… in `LIB-EMPTY`, `PREF-SLIDESHOW` and
`PREF-VIEWER` (the rows M10.2 split out), `SYS-MCP`, the File, Edit, View, Folder / Album, Picture
and Tools menu rows, and the generated `KEY-*` rows of the Slideshow, Photo viewer and Import
contexts; new rows where none fit: `EDIT-BEFOREAFTER` for the before/after hold) and names its
commands there; first fingerprints of the room tabs, the Import room, compare, the slideshow and its
bar, the quick preview, the viewer, the dialogs and the new widgets and icons are approved in bulk
on the M11.R review page.

Whole-window baselines belong to no task, as in M5–M10: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m10-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.

Decisions taken while planning (the check-in lists them for reversal): a room replaces the toolbar,
the tree, the light table, the side pane and the bottom area; the menu bar, the tab strip (28 high,
shown only while a room is open) and the status line stay; Ctrl+M opens Import or brings it to the
front; closing Import while it copies asks "Stop importing?" [Stop Import] [Keep Importing], and
otherwise closes at once; the listing lives in the engine's session, so the room reopens on the
last source while it is still there. Sources: cards are mounted volumes holding a `DCIM` folder
(LightCraft's rule; the source is that folder), drives are the other removable volumes of M5.4's
volume list (the source is the volume's root), plus Folder… through the picker and the last 8
sources; the volume list refreshes at most every 2 s off the owner thread. A source is read as an
untrusted tree (M5's bounded walk, at most 100 000 files, names validated, sizes never trusted for
allocation): the files Dalica can show by content under the File Types setting, each with its
capture time (EXIF, else the file's time), grouped where capture times are more than 2 hours apart,
its thumbnail from the embedded one or a scaled decode, kept in memory only and never in the
thumbnail cache. A candidate is a duplicate when a library photo, or an earlier file of the same
listing, has its size and MD5 digest (M9's digests, cached by size and modification time) and its
bytes compared in full; duplicates and excluded files are never copied and never removed from the
card. Every copy goes through `fs` as a new file that never replaces one (M9.2's copy phase): fsync,
a byte-for-byte check against the source, the source's modification time kept; a clashing name gets
`-001` (`core::names`), folder names go through `core::names::sanitize` (M8.13); "Date taken"
uses the capture date, else the file's date; "Today's date" is the session clock's; an existing
folder name imports into that folder; a destination inside the source is refused. The photos
copied enter the library with the stars and rotations set in the room, the destination becomes a
watched folder (Scan Always) unless one already covers it, and the whole import is one library
undo step "Import" whose undo sends the copies still unchanged to the trash (M9's rule for an
undone copy); once source files were removed from the card that undo is disabled ("The originals
were removed from the card"). Removal from a card happens only after "Delete copied files from the
card" and the question, only for the files this import copied and verified, each re-checked
unchanged since its copy, to the OS trash when the card has one and otherwise deleted permanently
(the only permanent deletion in Dalica, `security.md` §5.7); `import.removeFromSource` is refused
over the control channel and MCP by M10.16's automation policy. Cancel stops after the current
file and keeps what was copied as the undo step. Batch Edit is the engine command `edit.apply {ids,
op, params?}`: it appends to each recipe with the edit view's merge rules as one library undo step
(`SetRecipes`), and an operation this build cannot render (the five effects until M12) is greyed
with "Arrives with Effects"; MCP's `apply_edit` moves to it. Copy All Edits keeps the recipe in the
engine's session (never saved), so the CLI and MCP can paste too; Paste All Edits replaces recipes
as one library step in the library and as one step of the open photo's history in the edit view,
as rotations do (undo follows the view). Undo All Edits removes unsaved edits (recipe and redo
list) as one library step, or one history step in the edit view, so it can be undone: its question
reads "Remove all edits from %d pictures?" [Remove Edits] [Cancel] without "You can't undo this",
and says that saved photos keep their saved changes (Revert takes them back). Compare: the edit
view's preview gets a second pane; A│B's sides are two photos, each with its own history, and the
filmstrip chooses the focused side's photo; A│A's right side is a variant the engine holds for the
session only (it starts as a copy of the recipe and has its own undo), written nowhere until kept,
and keeping it is one step of the photo's history (`edit.recipe.set`); zoom and pan are synced
between the sides, as in LightCraft's compare; "Don't ask again" remembers Apply for the switch
question and the side chosen for the keep question; before/after shows the focused side's source
unedited while held. Slideshow: in the edit view it plays the current folder or album from the
current photo; in the library two or more photos in the tray play in tray order, else the section
of the focused photo from that photo, else the first visible section from its start; a section's ▶
and Folder ▸ View Slideshow play that section; a search plays its results; videos are left out until
M15. It asks the OS for full screen through the view state and gives the window back as it was
(a window already full screen stays so); headless it fills the window. The photo shown and the
next two are screen-size previews of the edit view's kind, so a photo is never shown above twice
its pixels and the next one is ready before its transition; Pan and Zoom moves toward the centre
until faces (M13); timing runs on the session clock. Music: playing sound needs either a crate
that links the system audio library on Linux (a `*-sys` or C build entry) or an MPL-2.0 decoder,
both the maintainer's to allow (`AGENTS.md` §4), so "Play music during slideshows", its folder and
Folder Properties' Music are drawn disabled ("Arrives with sound support") and how to play music
is a question at M11.R; Loop and the other options work. The quick preview fills the window, not
the OS screen (a peek, not a mode), from the best thumbnail in the cache, refined in place.
Viewer: `dalica --view FILE` opens the library as usual and shows the viewer in a borderless
window filling the screen, transparent where the platform allows and opaque black otherwise or
when "Translucent background" is off; the folder is listed from disk on a worker in name order;
photos of watched folders are the library's own; viewing changes nothing, and the first change to
a photo of an unwatched folder (star, rotate, delete) adds its folder as Scan Once, as Open File…
would, in the same undo step; when another Dalica holds the library the viewer opens read-only
(edits shown, changes and Open in Dalica disabled with "Dalica is already open"), and handing the
file to the running Dalica instead is a question at M11.R; Open in Dalica turns the same window
into the main window on the edit view of that photo; Esc and × quit. Options gains the Slideshow
and Photo Viewer tabs (the viewer's File Types drawn disabled "Arrives with OS integration"), and
General's import rows become live. M11's budgets join `development.md` §8 at 1.5 × their first
measurement, plus two absolute ones (a transition frame within 16 ms at p95, zero waits on disk),
and are never loosened. No other step of M11 needs a maintainer decision mid-run.

- ⬜ **M11.1 `engine`: M11's command modules, settings and options rows**
  - after: M10.1, M10.5, M10.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/import.rs`, `crates/engine/src/cmd/import_run.rs`, `crates/engine/src/cmd/batch.rs`, `crates/engine/src/cmd/compare.rs`, `crates/engine/src/cmd/slideshow.rs`, `crates/engine/src/cmd/viewer.rs`, `crates/engine/src/import.rs`, `crates/engine/src/import_run.rs`, `crates/engine/src/variant.rs`, `crates/engine/src/viewer.rs`, `crates/engine/src/options.rs`, `crates/engine/src/settings.rs`, `crates/engine/src/session.rs`, `crates/engine/src/jobs.rs`, `crates/engine/tests/settings_m11.rs`, `crates/engine/tests/options_catalogue.rs`, `crates/engine/README.md`, `docs/menus-and-shortcuts.md`, `docs/data-model.md`
  - scope: the engine's ground for M11 so M11.7–M11.12 never share a file: empty
    `cmd/import.rs`, `import_run.rs`, `batch.rs`, `compare.rs`, `slideshow.rs` and `viewer.rs`
    joined into the registry once; stubs `import.rs` (the import session: source, listing
    generation, candidates), `import_run.rs`, `variant.rs` (A│A's variant) and `viewer.rs`, each a
    field of `Session` once; `JobKind::Import`; every M11 setting with its default behind a typed
    accessor: Slideshow (loop off, display time 3 s in 1–60, transition `dissolve` of the seven,
    show captions on, play music off and its folder, play video sound off), Photo Viewer (wheel
    `zoom | navigate`, hide controls on, translucent background on), import (save imported
    pictures in the Pictures folder, detect duplicates on, after copying `leave`, the last 8
    sources, destinations and folder names), compare (ask before applying on a switch on; keep
    which version `ask | left | right`); the options catalogue gains the Slideshow and Photo Viewer
    tabs' rows (music and video sound unavailable with their reasons, the viewer's File Types
    "Arrives with OS integration") and General's import rows become available;
    `menus-and-shortcuts.md` §1.9 lists the new commands (`import.*`, `edit.apply`,
    `edit.copyRecipe`, `edit.pasteRecipe`, `edit.undoAll`, `edit.variant.*`, `slideshow.list`,
    `viewer.list`) and §5's Slideshow and Photo Viewer tabs say what this build offers;
    `data-model.md` §4 lists the new settings keys.
  - done when: `crates/engine/tests/settings_m11.rs` round-trips every new setting with unknown
    keys kept and wrong types or out-of-range values falling back (reported);
    `crates/engine/tests/options_catalogue.rs` covers the six tabs now shown, every option once,
    each default equal to its accessor's; every existing engine test and `panic_hunt` pass
    unchanged.
- ⬜ **M11.2 `ui`: M11's view state, UI commands, the rooms' frame and stubs**
  - after: M10.2, M10.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/menu_catalog.rs`, `crates/ui/src/app.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/rooms.rs`, `crates/ui/src/commands/import.rs`, `crates/ui/src/commands/slideshow.rs`, `crates/ui/src/commands/peek.rs`, `crates/ui/src/commands/compare.rs`, `crates/ui/src/commands/batch.rs`, `crates/ui/src/commands/viewer.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/layout.rs`, `crates/ui/src/panels/rooms.rs`, `crates/ui/src/panels/import.rs`, `crates/ui/src/panels/import/**`, `crates/ui/src/panels/slideshow.rs`, `crates/ui/src/panels/slideshow/**`, `crates/ui/src/panels/peek.rs`, `crates/ui/src/panels/viewer.rs`, `crates/ui/src/panels/viewer/**`, `crates/ui/src/panels/edit.rs`, `crates/ui/src/panels/edit/compare.rs`, `crates/ui/src/panels/edit/top_bar.rs`, `crates/ui/src/panels/edit/caption_bar.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/keep_version.rs`, `crates/ui/src/dialogs/apply_changes.rs`, `crates/ui/src/dialogs/undo_all.rs`, `crates/ui/src/dialogs/remove_from_card.rs`, `crates/ui/src/dialogs/viewer_settings.rs`, `crates/ui/src/dialogs/options.rs`, `crates/ui/src/dialogs/options/slideshow.rs`, `crates/ui/src/dialogs/options/viewer.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state_m11.rs`, `docs/control-protocol.md`
  - scope: every M11 field of `UiState` in one go (`control-protocol.md` §6.5 already names
    `room` and `slideshow`): the rooms (open, active); the Import room (focus, selection, the item
    in the preview, the destination draft: place, folder name, after copying); the slideshow
    (`null`, or its photos, index, playing, bar shown, zoom and pan, the transition running, where
    it started); the quick preview (`null` or its photo); `edit.compare` with the focused side and
    the layout; the photo viewer (photo, backdrop, controls shown, zoom); UI commands with stub
    handlers, so M11.13–M11.21 never share a file: `import.open` (Ctrl+M; the stub opens an empty
    room), `room.select`, `room.close`; `view.slideshow` (Ctrl+4), `section.slideshow`,
    `slideshow.exit`, `.next`, `.previous`, `.first`, `.last`, `.pause`, `.zoomIn`, `.zoomOut`,
    `.transition`, `.captions`, `.displayTime`; `peek` (Ctrl+Alt held, listed and not editable);
    `edit.compare {mode}`, `edit.switchFocus`, `edit.switchLayout`, `edit.showOriginal` (`\` held);
    `viewer.next`, `.previous`, `.zoomIn`, `.zoomOut`, `.actualSize`, `.background` (Space),
    `.openInDalica` (E), `.close` and `viewer.settings`; the Slideshow, Photo viewer and Import key
    contexts made active by the view in `keys.rs`, with the keys of `viewing.md` §1.4 and §3.2 and
    `menus-and-shortcuts.md` §2.4 as their defaults; the tab strip's region in `layout.rs` (28
    high under the menu bar while a room is open; a room's content in place of the toolbar, tree,
    light table, side pane and bottom area); stub panels `rooms.rs`, `import.rs` (and its
    folder), `slideshow.rs` (and its folder), `peek.rs`, `viewer.rs` (and its folder) and
    `edit/compare.rs`; the edit top bar's Play and compare toggle and the caption bar's slideshow
    toggle wired to their commands (live as each command is); stub dialogs `keepVersion`,
    `applyChanges`, `undoAllEdits`, `removeFromCard`, `viewerSettings`; the Options dialog's
    Slideshow and Photo Viewer tabs with stub bodies; control stubs for the new state;
    `control-protocol.md` §6.5 and §7 gain the fields and widget ids.
  - done when: `crates/ui/tests/view_state_m11.rs`: each new field round-trips through `ui.state`
    / `ui.set` (bad values are `bad_params`, nothing changes); Ctrl+M opens the Import tab and the
    strip appears 28 high, closing it removes the strip; a Slideshow-context key does nothing in
    the library; every command's graceful-failure test; `panic_hunt` and `widget_ids` stay green.
- ⬜ **M11.3 `ui-kit`: room tabs, the overlay bar, the stepper, the caption plate and the icons**
  - after: M10.3, M10.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/tabs.rs`, `crates/ui-kit/src/widgets/overlay_bar.rs`, `crates/ui-kit/src/widgets/stepper.rs`, `crates/ui-kit/src/widgets/caption_plate.rs`, `crates/ui-kit/src/widgets/group_header.rs`, `crates/ui-kit/src/widgets/badges.rs`, `crates/ui-kit/src/theme.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/viewing.rs`, `crates/ui-kit/tests/viewing_widgets.rs`, `crates/ui-kit/tests/icons_viewing.rs`, `crates/ui-kit/tests/fingerprints/viewing-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-viewing*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `design-system.md` §5–§6, `ui-layout.md` §8 and `viewing.md` §1.2–§1.3 and §3.1: the
    room variant of `tabs` (icon and label, 190 × 28, the active tab joined to the content below,
    a close × on every tab but Library, shrinking and the overflow menu through the existing
    `fit`); `overlay_bar` (the rounded translucent panel of the slideshow and the viewer, `black
    α170`, radius 10, light-on-dark buttons, fading in and out over 0.2 s on the clock it is
    given); `stepper` (`[−] 3 seconds [+]` within a range); `caption_plate` (22 pt Inter on `black
    α160`, at most 70 % of the width, two lines, then elided); `group_header` (the import tray's
    time header with the clock button that selects its group); the excluded ⊘ mark for a cell
    (`danger`, the cell dimmed); the overlay colours as tokens; the icons it lacks, our own
    drawings on the 20-unit grid: Library, Import (camera), Exit, Pause, Previous, Next,
    Transition, Captions, Clock, Exclude, Actual size (1:1), Open in Dalica, More (⋯), Close,
    Switch Focus (⇄), Switch Layout (⇅), Show Original; the widget gallery gains them.
  - done when: `crates/ui-kit/tests/viewing_widgets.rs`: three room tabs fit at both window sizes,
    more elide and then overflow with the active one kept, Library has no ×; the stepper stops at
    its ends; the caption plate wraps at two lines; the overlay bar's opacity follows a virtual
    clock; `crates/ui-kit/tests/icons_viewing.rs`: every new icon stays in its grid at 12, 16, 20
    and 28 px in rest, hover and disabled colours; gallery and icon fingerprints; `widget_ids`,
    contrast, `cargo xtask assets` and `cargo xtask names` pass.
- ⬜ **M11.4 `create`: slideshow sequencing and transition geometry**
  - after: M10.R
  - files: `crates/create/src/slideshow.rs`, `crates/create/src/slideshow/**`, `crates/create/tests/slideshow.rs`, `crates/create/README.md`
  - scope: `viewing.md` §1 as pure code (no I/O, no egui; `architecture.md` §2 puts slideshow
    sequencing in `create`): the show's state machine on a `Clock` (playing, paused, zoomed and
    therefore paused, at the end without Loop with the bar shown and Play restarting, wrapping with
    Loop; display time 1–60 s; the bar shown for 2 s at start and for 2.5 s after a mouse move;
    Space toggles pause and shows the bar, a mouse move shows it without pausing; next, previous
    (none before the first without Loop), first, last, the wheel, a click advancing only while the
    bar is hidden); the transitions Cut, Pan and Zoom, Dissolve, Wipe, Circle, Push and Rectangle
    as geometry for a progress 0…1 over 0.8 s (Pan and Zoom over the whole display time, a 5–10 %
    zoom and pan toward a given point): for the outgoing and the incoming photo, its rectangle, its
    opacity and its clip (a rectangle or a circle), so the UI only paints; the fit (centred, never
    above twice the photo's pixels); the caption plate's place.
  - done when: `cargo test -p dalica-create --test slideshow`: a virtual clock drives a 5-photo
    show through play, pause, a mouse move, its end and Loop exactly as §1.2 says; at progress 0
    every transition shows only the outgoing photo and at 1 only the incoming one, clips stay
    inside the screen and opacities in 0…1 (property-tested over sizes and progress); Push's two
    rectangles never overlap; Pan and Zoom stays within 5–10 %; the fit never goes past 2×; hostile
    sizes (zero, huge) answer without a panic.
- ⬜ **M11.5 `scan`: cards and drives, and listing an import source**
  - after: M5.4, M10.R
  - files: `crates/scan/src/devices.rs`, `crates/scan/src/import.rs`, `crates/scan/src/import/**`, `crates/scan/tests/devices.rs`, `crates/scan/tests/import_source.rs`, `crates/scan/README.md`, `docs/security.md`, `NOTICE`
  - scope: `organizing.md` §3 and `security.md` §5.9: `devices.rs`, ported from LightCraft
    `crates/engine/src/devices.rs` (`DeviceWatch`, `devices_in`, `dcim_in`) at 3b4279fc with a
    header and a `NOTICE` row: cards (mounted volumes holding a `DCIM` folder, under the platform's
    mount parents or on a drive letter) and drives (the other removable volumes of M5.4's
    `volumes`), each with its display name ("Removable drive (E:)" or the volume's own name), its
    root and the folder to import from; `DALICA_DEVICE_ROOTS` replaces the mount parents in tests;
    a cached list refreshed off the calling thread at most every 2 s; `import.rs`: listing a source
    as an untrusted tree (M5's bounded walk with its limits, at most 100 000 files, names
    validated, sizes never trusted for allocation): the files Dalica can show by content under the
    File Types setting, each with its size, modification time, capture time (EXIF through `meta`,
    else the file's time), its MD5 digest computed while it is read, and a thumbnail of at most
    256 px from the embedded one or a scaled decode; groups split where capture times are more
    than 2 hours apart; results in batches with progress, and cancellation between files;
    `security.md` §5.9's device line marked built.
  - done when: `cargo test -p dalica-scan --test devices --test import_source`: with temp folders
    as mount parents, a volume holding `DCIM` or `dcim` is a card and a removable one without it a
    drive, a symlinked startup disk is skipped; a source of `scenes` photos with EXIF times lists
    every photo with its time, digest and thumbnail, grouped at 2-hour gaps; a FIFO, a link loop, a
    tree past the depth limit and a file whose header lies about its size are bounded and reported;
    cancel stops between files; a non-UTF-8 name is listed and never opened by its lossy form.
- ⬜ **M11.6 `fs`: import copies that keep their time, and removing a card's copied files**
  - after: M9.2, M10.R
  - files: `crates/fs/src/transfer.rs`, `crates/fs/src/remove.rs`, `crates/fs/src/faults.rs`, `crates/fs/tests/import_copy.rs`, `crates/fs/tests/remove.rs`
  - scope: `organizing.md` §3 and `security.md` §5.7: an import copy through M9.2's copy phase
    into a destination folder (a new file, fsync, a commit that never replaces, a byte-for-byte
    check of the copy against the source, the source's modification time set on the copy),
    answering `Exists` on a clash for the caller to rename, reporting bytes as it goes for a speed
    read-out, and stopping cleanly between files; `remove.rs`: removing a source file after its
    import, only with the identity (size, modification time, file id) recorded when it was copied
    and only while it still matches: to the OS trash when the volume has one, otherwise deleted
    permanently (the only permanent deletion in Dalica), refusing a link, a folder and anything
    outside the given root; fault points at every step.
  - done when: `cargo test -p dalica-fs --test import_copy --test remove`: a copy keeps the bytes
    and the time and never replaces a file created at the target between the check and the commit
    (a racing thread); a byte flipped in the copy (a fault point) fails the check, removes the
    partial copy and leaves the source; removing a file changed since its copy is refused; a volume
    with a trash gets the file there, and one without (a `faults` switch) deletes it; every fault
    point as Error and as Crash leaves the source whole or the copy verified (`--features faults`);
    hostile names are refused.
- ⬜ **M11.7 `engine`: import sources, listing and duplicates**
  - after: M11.1, M11.5
  - files: `crates/engine/src/cmd/import.rs`, `crates/engine/src/import.rs`, `crates/engine/src/import/**`, `crates/engine/tests/import_list.rs`
  - scope: `organizing.md` §3 items 1–3 on M11.5's listing: `import.sources` (query: cards, drives
    and recent sources from the cached list, never waiting on the disk); `import.open {source}` (a
    card, a drive, or a folder path checked by the automation read policy; starts the listing job
    and drops an older listing); `import.list {offset?, limit?}` (query: the status text ("Loading
    112 of 340 files…", "340 files", "No pictures found") and the candidates in capture order with
    id, name, size, capture time, group, duplicate (of a library photo or of an earlier
    candidate), excluded, the star and rotation chosen, whether the thumbnail is ready; paged, at
    most 1 000); `import.exclude {ids, on}`, `import.excludeAll {on}`, `import.mark {ids, star?,
    rotate?}` (the room's choices; no undo step), `import.options {excludeDuplicates}` and
    `import.close`; duplicates: the same size and MD5 digest as a library photo (M9's cached
    digests, computed on a worker for library photos of that size when missing) or as an earlier
    candidate, confirmed byte for byte; the candidates' thumbnails kept in memory (capped, least
    recently used dropped); rows `IMP-DEVICES`, `IMP-DUP` (engine half).
  - done when: `crates/engine/tests/import_list.rs` on temp folders through `DALICA_DEVICE_ROOTS`:
    a card appears in `import.sources` within one refresh of its folder being made and leaves when
    it is removed; listing 300 `scenes` photos streams in batches with rising counts; 20 already
    in the library and 3 repeated on the card are duplicates while a same-size file with other
    bytes is not; Exclude All and Include All; the owner thread made no `fs` call; each command's
    contract and graceful failures; `panic_hunt` green.
- ⬜ **M11.9 `engine`: Batch Edit, Copy and Paste All Edits, and Undo All Edits**
  - after: M10.9, M11.1
  - files: `crates/engine/src/cmd/batch.rs`, `crates/engine/tests/batch_edit.rs`, `docs/editing.md`
  - scope: `editing.md` §5.1 and §9: `edit.apply {ids, op, params?}` appends one operation of
    M10.9's catalogue to each photo's recipe with the edit view's rules (consecutive rotations
    merge, only the last crop counts, Tuning merges into a trailing `tuning`) as one library undo
    step through `SetRecipes` ("Undo Auto Fix"), clearing the redo lists as any new edit does; an
    operation this build cannot render is disabled with "Arrives with Effects"; `edit.copyRecipe
    {id}` (one photo with edits; the recipe kept in the session); `edit.pasteRecipe {ids}`
    replacing each recipe with the copied one as one library step, or `{id, history: true}` as one
    step of that photo's history (the edit view); `edit.undoAll {ids, confirm}` (`needs_confirm`
    without) removing every unsaved edit as one library step, or `{id, history: true}` as one
    history step ("Remove Edits"), saved photos keeping their saved pixels and named in the answer;
    `editing.md` §5.1 and §9 updated (the question's text, the effects greyed until M12); rows
    `EDIT-BATCH`, `EDIT-RECIPE-COPY`, `EDIT-UNDO` (engine half).
  - done when: `crates/engine/tests/batch_edit.rs`: Auto Fix on 100 photos is one undo step and one
    `edit.undo` restores all 100 recipes and redo lists exactly; two Batch Rotate Clockwise merge
    per photo; Paste on 5 photos replaces their recipes and keeps an unknown operation of the
    copied recipe; Paste with `history` is a step of that photo's history and not of the library;
    Undo All Edits with `confirm` empties the unsaved recipes, undo brings them back and a saved
    photo's file is untouched; Sepia answers `disabled` with its reason; each command's contract
    and graceful failures; `panic_hunt` green.
- ⬜ **M11.10 `engine`: two preview panes, and A│A's variant**
  - after: M11.1
  - files: `crates/engine/src/preview.rs`, `crates/engine/src/preview/**`, `crates/engine/src/variant.rs`, `crates/engine/src/cmd/compare.rs`, `crates/engine/src/cmd/history.rs`, `crates/engine/src/history.rs`, `crates/engine/tests/preview_panes.rs`, `crates/engine/tests/variant.rs`
  - scope: `editing.md` §4: the edit view's preview gains a second pane (`open_preview` and the
    calls beside it take a pane, 0 or 1; each pane has its photo, recipe, candidate, window and
    generation; both render at the edit view's priority, the focused one first; the neighbours'
    prefetch is kept), so compare shows two photos or one photo twice; a pane can show its source
    unedited (before/after); `variant.rs`: A│A's right side, a scratch history of one photo held
    in the session only (`edit.variant.open {id}` starts it as a copy of the photo's recipe;
    `photo.history.apply`, `.undo`, `.redo` and `.state` take `variant: true`; the variant's pane
    renders its recipe; `edit.variant.close {keep: left | right}` drops it, or makes its recipe one
    step of the photo's own history through `edit.recipe.set`); nothing of a variant reaches a
    sidecar until it is kept; row `EDIT-COMPARE` (engine half).
  - done when: `crates/engine/tests/preview_panes.rs`: two photos open in two panes render both, a
    change to one recipe renders only its pane again, closing a pane cancels its jobs, the
    unedited pane equals the decoded source; `crates/engine/tests/variant.rs`: Auto Fix on the
    variant leaves the photo's sidecar unchanged; keeping the right side is one history step that
    one undo removes; keeping the left drops the variant; a restart has no variant; graceful
    failures; `panic_hunt` green.
- ⬜ **M11.12 `engine` + `dalica`: the viewer's session and `dalica --view`**
  - after: M11.1
  - files: `crates/engine/src/viewer.rs`, `crates/engine/src/cmd/viewer.rs`, `crates/engine/tests/viewer.rs`, `apps/dalica/src/main.rs`, `apps/dalica/src/viewer.rs`, `apps/dalica/src/window.rs`, `apps/dalica/tests/view.rs`, `docs/viewing.md`
  - scope: `viewing.md` §3 and the decisions above: `Session::open_viewer` and `viewer.list
    {path}` (query, read on a worker: the photos Dalica can show in the file's folder, in name
    order, and the file's place among them; photos of watched folders are the library's own,
    others are shown from their file and their folder's sidecar, read only); the first change to a
    photo of an unwatched folder (star, rotate, delete) adds its folder as Scan Once in the same
    undo step; a library another Dalica holds opens the viewer read-only (changes disabled with
    "Dalica is already open"); `dalica --view FILE` opens the viewer in a borderless window filling
    the screen, transparent where the platform allows and opaque black otherwise; `--view` with
    `--headless` serves the viewer's state over the control channel; Esc and × quit; Open in
    Dalica turns the window into the main window (decorations, its saved size) on the edit view of
    that photo; `viewing.md` §3 records what was decided; row `VIEW-VIEWER` (engine and app
    halves).
  - done when: `crates/engine/tests/viewer.rs`: a folder of 12 photos lists in name order with the
    file's place; a file Dalica cannot show is refused with its reason; starring a photo of an
    unwatched folder adds the folder as Scan Once and one undo removes both; a locked library gives
    a read-only viewer whose star is `disabled`; `apps/dalica/tests/view.rs`: `dalica --view FILE
    --headless` reports `view: "viewer"` on that photo, an odd file name still opens, Esc quits;
    graceful failures; `panic_hunt` green.
- ⬜ **M11.13 `ui`: the room tab strip**
  - after: M11.2, M11.3
  - files: `crates/ui/src/panels/rooms.rs`, `crates/ui/src/commands/rooms.rs`, `crates/ui/tests/rooms.rs`, `crates/ui/tests/fingerprints/rooms-*.json`, `docs/ui-layout.md`
  - scope: `ui-layout.md` §8 with M11.3's room tabs: the strip between the menu bar and the
    content while a room is open (`[▦ Library] [📷 Import ×]`); a click brings a room to the front,
    × closes it (Import while copying asks "Stop importing?" [Stop Import] [Keep Importing]); the
    front room's keyboard context; the Library tab brings back the library or the edit view as it
    was left, scroll and selection kept; `ui-layout.md` §8 says what a room replaces; row
    `LIB-ROOMS`.
  - done when: `crates/ui/tests/rooms.rs` (headless, by id): opening Import shows the strip and the
    room, Library returns with the light table's scroll and selection kept, × closes Import and the
    strip goes; the question while a (fake) import job runs; Import-context keys act only in the
    room; fingerprints of the strip at both window sizes.
- ⬜ **M11.8 `engine`: importing: copy, verify, name, enter, undo and card removal**
  - after: M11.6, M11.7
  - files: `crates/engine/src/cmd/import_run.rs`, `crates/engine/src/import_run.rs`, `crates/engine/src/automation.rs`, `crates/engine/tests/import_run.rs`, `crates/engine/tests/import_remove.rs`, `docs/organizing.md`, `docs/data-model.md`, `docs/security.md`
  - scope: `organizing.md` §3 items 4–6 and the decisions above: `import.run {ids?, destination,
    folderName, after}` (`ids` omitted: every candidate not excluded; `folderName` a text,
    `{dateTaken: true}` or `{today: true}`) as an `import` job: each file copied by M11.6 into
    `<destination>/<folder>` (made exclusively when new, imported into when it exists), a clashing
    name getting `-001`; progress "Copying 45 of 340 at 38 MB/s"; Cancel after the current file;
    the photos copied enter the library with the stars and rotations chosen, the destination
    watched (Scan Always) unless a watched folder covers it, the files marked as Dalica's own so
    the watcher imports nothing, all as one library undo step "Import" whose undo trashes the
    copies still unchanged and whose redo copies again; the result (folders, imported, skipped as
    duplicates or excluded, failed with reasons, bytes, speed) and a session event for the toast;
    the recent destinations and folder names updated; `import.removeFromSource {confirm}` for the
    last finished import from a card or drive (`needs_confirm` without), removing through M11.6
    only the files it copied and verified, refused for the control channel and MCP in M10.16's
    automation policy, after which Undo Import is disabled with its reason; `organizing.md` §3,
    `data-model.md` §8 (an import row) and `security.md` §5.7 updated; rows `IMP-ROOM` (engine
    half), `IMP-NAMING`, `IMP-AFTER`.
  - done when: `crates/engine/tests/import_run.rs` on a temp card: 120 photos over three capture
    dates with `dateTaken` land in three `YYYY-MM-DD` folders with the source's bytes and times; a
    planted clash gets `-001` and nothing is overwritten; a second import of the card copies
    nothing and reports 120 duplicates; Cancel after 10 keeps 10, entered as one step; undo trashes
    the copies and redo brings them back; stars and rotations chosen in the room are in the
    destination's sidecar; `crates/engine/tests/import_remove.rs`: removal deletes exactly the
    verified copied files (one changed on the card since its copy stays), is refused through the
    automation policy and without `confirm`, and disables the undo with its reason; the owner
    thread made no `fs` call; graceful failures; `panic_hunt` green.
- ⬜ **M11.11 `engine`: what a slideshow plays, and transitions that never wait on disk**
  - after: M11.4, M11.10
  - files: `crates/engine/src/cmd/slideshow.rs`, `crates/engine/tests/slideshow.rs`
  - scope: `viewing.md` §1.1–§1.2 with M11.4's sequencing: `slideshow.list {section?, ids?,
    search?, start?}` (query: the photos to play in the display order of the section, the album,
    the search results or the given ids, the index to start from, hidden photos only while shown,
    videos left out and counted); the screen-size preview of the photo shown, and of the next two
    prefetched through M11.10's panes at the display's size, so the next photo is ready before its
    transition starts; row `VIEW-SLIDESHOW` (engine half).
  - done when: `crates/engine/tests/slideshow.rs`: the lists of a folder, an album (in album
    order), a search and the tray's ids, each starting at the photo asked for; with every decode
    slowed by a planted 1.5 s delay and a 3 s display time, 30 advances find the next preview
    ready every time (zero waits); a photo deleted from the list is skipped without a stall;
    graceful failures; `panic_hunt` green.
- ⬜ **M11.14 `ui`: the quick full-screen preview**
  - after: M11.2, M11.10
  - files: `crates/ui/src/panels/peek.rs`, `crates/ui/src/commands/peek.rs`, `crates/ui/tests/peek.rs`, `crates/ui/tests/fingerprints/peek-*.json`
  - scope: `viewing.md` §2: Ctrl+Alt (Cmd+Option) held while the pointer is over a thumbnail, or in
    the edit view, fills the window with the photo on black at once from the best thumbnail in the
    cache, refined in place by the screen-size preview; the wheel steps through the section's
    neighbours (prefetched); releasing returns with the last photo shown focused and scrolled into
    view; nothing else is drawn; the control channel's `ui.key {state: "down"}` drives it; rows
    `LIB-PEEK`, `LIB-HOVER`.
  - done when: `crates/ui/tests/peek.rs`: holding over a thumbnail shows that photo on the first
    frame (never a grey frame), the wheel steps three photos and release focuses the third;
    holding over empty space does nothing; in the edit view it shows the current photo;
    fingerprints at both window sizes.
- ⬜ **M11.15 `ui`: the Import room: sources, the import tray and the preview**
  - after: M11.7, M11.13
  - files: `crates/ui/src/panels/import.rs`, `crates/ui/src/panels/import/source.rs`, `crates/ui/src/panels/import/tray.rs`, `crates/ui/src/panels/import/preview.rs`, `crates/ui/src/commands/import.rs`, `crates/ui/tests/import_room.rs`, `crates/ui/tests/fingerprints/import-room-*.json`
  - scope: `organizing.md` §3 items 1–3, with LightCraft `crates/ui-egui/src/import.rs`
    (`ImportDialog`, `poll_scan`) at 3b4279fc as the pattern: the room's layout; "Import from:"
    (cards and drives from `import.sources`, Folder… through the picker, recent sources; the list
    follows a card plugged in while the room is open); the import tray: its status line, "Exclude
    duplicates" with its tooltip and its default, thumbnails appearing as they load, grouped by
    capture time under M11.3's group header whose clock selects the group, duplicates and excluded
    files dimmed with ⊘, selection by click, Ctrl, Shift, the arrows and Ctrl+A, the context menu
    (Exclude, Include, Exclude All, Include All), X toggling exclude; the preview (about 35 % of
    the width: the selected item large, "filename · date time · size", star, exclude or include,
    previous and next, rotate left and right through `import.mark`); `import.open` opening the
    room and bringing it to the front; rows `IMP-ROOM`, `IMP-DUP`.
  - done when: `crates/ui/tests/import_room.rs` (headless, by id, a temp card through
    `DALICA_DEVICE_ROOTS`): Ctrl+M lists the card and its photos arrive group by group; a clock
    selects its group; X excludes and the cell dims; Exclude duplicates leaves the library's
    photos out of the count; the preview's star and rotation reach `import.list`; Folder… with the
    scripted picker lists that folder; fingerprints at both window sizes.
- ⬜ **M11.16 `ui`: compare, before/after and their two questions**
  - after: M11.2, M11.3, M11.10
  - files: `crates/ui/src/panels/edit/compare.rs`, `crates/ui/src/panels/edit/canvas.rs`, `crates/ui/src/panels/edit/filmstrip.rs`, `crates/ui/src/panels/edit/panel.rs`, `crates/ui/src/commands/compare.rs`, `crates/ui/src/dialogs/apply_changes.rs`, `crates/ui/src/dialogs/keep_version.rs`, `crates/ui/tests/compare.rs`, `crates/ui/tests/fingerprints/edit-compare-*.json`, `crates/ui/tests/fingerprints/dialog-keep-version-*.json`
  - scope: `editing.md` §4, with LightCraft `crates/ui-egui/src/panels/compare.rs`
    (`compare_pair`, `set_photo`, `swap`, synced zoom and pan) and `tests_before_after.rs` at
    3b4279fc as the pattern: the toggle's A, A│B and A│A live; two canvases side by side, or above
    and below after ⇅, each from its pane, zoom and pan synced; the focused side's grey "Selected"
    pill; a click focuses a side and the filmstrip opens the focused side's photo; ⇄ switches the
    focus; tools, Undo and Redo act on the focused side (the variant in A│A); the switch question
    "Apply changes to the current photo?" [Apply] [Discard] [Cancel] with "Don't ask again, always
    apply"; leaving A│A with differing sides asks "Keep which version?" [Left] [Right] (or [Top]
    [Bottom]) [Cancel] with "Don't ask again"; before/after: `\` held, or the edit panel header's
    "Show original" pressed and held, shows the focused side unedited while held; rows
    `EDIT-COMPARE`, `EDIT-VIEW` (the toggle), a new `EDIT-BEFOREAFTER`.
  - done when: `crates/ui/tests/compare.rs` (headless, by id): A│B shows two photos and the
    filmstrip changes the focused one; Auto Fix on A│A's right side leaves the left's recipe in
    the sidecar unchanged, and "Keep which version?" → Right makes it one history step; Cancel stays
    in A│A; switching with a crop panel open asks and Discard drops it; `\` down shows the original
    and up restores it; zooming one side zooms both; fingerprints of A│B side by side and above
    and below, A│A and the keep question at both window sizes.
- ⬜ **M11.17 `ui`: the photo viewer**
  - after: M11.2, M11.3, M11.10, M11.12
  - files: `crates/ui/src/panels/viewer.rs`, `crates/ui/src/panels/viewer/**`, `crates/ui/src/commands/viewer.rs`, `crates/ui/src/dialogs/viewer_settings.rs`, `crates/ui/src/dialogs/options/viewer.rs`, `crates/ui/tests/viewer.rs`, `crates/ui/tests/fingerprints/viewer-*.json`, `crates/ui/tests/fingerprints/dialog-viewer-settings-*.json`, `crates/ui/tests/fingerprints/dialog-options-viewer-*.json`
  - scope: `viewing.md` §3, with LightCraft `crates/ui-egui/src/panels/detail.rs` (the filmstrip,
    zoom and pan) at 3b4279fc as the pattern: the backdrop (`black α210`, or opaque; Space toggles
    it); the photo fitted above the strip with "filename (W × H)" in white SemiBold; the filmstrip
    of the folder centred on the current photo, the others dimmed, a click jumping; M11.3's overlay
    bar with +, −, 1:1, Open in Dalica, a large Play (a slideshow of the folder through
    `view.slideshow`), rotate left and right, star and ⋯ (Open With ▸ and Show in File Manager
    drawn disabled "Arrives with OS integration", Settings…); "close ✕" at the top right; the
    controls hiding after 3 s without a mouse move when the setting says so; keys and wheel per
    §3.2 and the wheel setting; Delete asking, then trashing; the read-only viewer's controls
    disabled with their reason; the Settings dialog and Options ▸ Photo Viewer (File Types drawn
    disabled with its reason, Interface live through `options.set`); Tools ▸ Configure Photo
    Viewer…; rows `VIEW-VIEWER`, `PREF-VIEWER`, the Tools menu row.
  - done when: `crates/ui/tests/viewer.rs` (headless, a viewer session): → steps through the folder
    and the filmstrip follows; 1 toggles 100 %; Ctrl+8 stars (the star in the sidecar); E opens the
    edit view on the photo; Space toggles the backdrop; the wheel zooms, and navigates once the
    setting changes; the controls hide after 3 s on a virtual clock; fingerprints of the viewer,
    its Settings dialog and the Options tab at both window sizes.
- ⬜ **M11.18 `ui`: Batch Edit, Copy and Paste All Edits, and Undo All Edits in the window**
  - after: M11.2, M11.9
  - files: `crates/ui/src/commands/batch.rs`, `crates/ui/src/dialogs/undo_all.rs`, `crates/ui/tests/batch_ui.rs`, `crates/ui/tests/fingerprints/dialog-undo-all-*.json`
  - scope: `editing.md` §9 and `menus-and-shortcuts.md` §1.2 and §1.5: Picture ▸ Batch Edit ▸ on
    the tray (Rename… opening M9's dialog, the rotations, Auto Contrast, Auto Color and Auto Fix
    through `edit.apply`, the effects greyed with their reason); Edit ▸ Copy All Edits and Paste
    All Edits, in the library on the tray and in the edit view on the open photo (a step of its
    history); Picture ▸ Undo All Edits… asking "Remove all edits from %d pictures?" [Remove Edits]
    [Cancel] (naming saved photos when there are), then `edit.undoAll`; thumbnails, the tray and
    the filmstrip showing the new edits; rows `EDIT-BATCH`, `EDIT-RECIPE-COPY`, `EDIT-UNDO`, the
    Edit and Picture menu rows.
  - done when: `crates/ui/tests/batch_ui.rs` (headless, by id): Batch Edit ▸ Auto Fix on 20 held
    photos, then one Ctrl+Z restores all 20; Copy All Edits on one photo and Paste All Edits on
    three selected gives them its recipe; in the edit view a Paste is undone by the photo's Undo
    button; Undo All Edits asks and removes; Sepia is greyed with "Arrives with Effects"; the
    question's fingerprints at both window sizes.
- ⬜ **M11.19 `ui`: the slideshow: what plays, the show, transitions and keys**
  - after: M11.2, M11.3, M11.4, M11.11
  - files: `crates/ui/src/panels/slideshow.rs`, `crates/ui/src/panels/slideshow/show.rs`, `crates/ui/src/panels/slideshow/transition.rs`, `crates/ui/src/commands/slideshow.rs`, `crates/ui/tests/slideshow.rs`, `crates/ui/tests/fingerprints/slideshow-*.json`
  - scope: `viewing.md` §1.1–§1.4 with M11.4's state machine and M11.11's list: the entries (View
    ▸ Slideshow and Ctrl+4, a section header's ▶ and Folder ▸ View Slideshow, the edit top bar's
    Play, the bottom area's Slideshow button, the viewer's Play) and what each plays, as decided
    above; full screen on pure black (the OS full screen asked through the view state and given
    back on exit), the photo from its screen-size preview placed by M11.4's fit; the seven
    transitions painted from M11.4's geometry; the caption on M11.3's plate while captions are on;
    the keys and mouse of §1.4 in the Slideshow context; Exit (Esc) back to where the show started
    with the last photo shown selected and scrolled into view; the end without Loop and with it;
    rotate and star acting on the photo shown through the library's commands; rows
    `VIEW-SLIDESHOW`, `LIB-ACTIONS` (Slideshow), the View and Folder / Album menu rows.
  - done when: `crates/ui/tests/slideshow.rs` (headless, a virtual clock): Ctrl+4 with a photo
    selected plays its section from it; three display times advance three photos; Space pauses and
    a mouse move does not; → ← Home End; Esc returns to the library with the last photo selected;
    started from the edit view's Play it returns to the edit view; Ctrl+8 stars and Ctrl+R rotates
    the photo shown, one undo step each; no frame waits on a preview over 30 advances (M11.11's
    slowed decoder); a fingerprint of each transition at its midpoint and of a captioned photo at
    both window sizes.
- ⬜ **M11.20 `ui`: the Import room: destination, importing and finishing**
  - after: M11.8, M11.15
  - files: `crates/ui/src/panels/import/destination.rs`, `crates/ui/src/panels/import/progress.rs`, `crates/ui/src/commands/import.rs`, `crates/ui/src/dialogs/remove_from_card.rs`, `crates/ui/tests/import_run_ui.rs`, `crates/ui/tests/fingerprints/import-destination-*.json`, `crates/ui/tests/fingerprints/dialog-remove-from-card-*.json`
  - scope: `organizing.md` §3 items 4–6 and its flow: the destination strip ("Import to:" with the
    default, recent destinations and Choose…; the "Folder name:" combo with free text, Date taken
    (YYYY-MM-DD), Today's date and recent names; "After copying:" for cards and drives only); the
    hint line with its light bulb in each state; Import All, Import Selected (n) and Cancel, the
    import buttons disabled until a folder name is set; Enter imports the selection; while
    importing, the progress in the room ("Copying 45 of 340 at 38 MB/s"), the status line and the
    activity indicator, Cancel after the current file; then, when chosen, the question "Remove the
    imported files from the card? This cannot be undone." [Remove Imported Files] [Keep Files];
    finishing: the room closes, the library selects and scrolls to the new folder, the toast
    "Imported 340 pictures into 2026-06-04" ("into 3 folders" for several); failures listed with
    their files; rows `IMP-ROOM`, `IMP-NAMING`, `IMP-AFTER`, `LIB-EMPTY` (Import…).
  - done when: `crates/ui/tests/import_run_ui.rs` (headless, a temp card): the import buttons stay
    disabled until a folder name; Date taken imports into dated folders, the library selects the
    first and the toast names them; Cancel part-way keeps what was copied; with "Delete copied
    files from the card" the question appears, Keep Files keeps them and Remove Imported Files
    removes only the copied ones; the empty library's Import… opens the room; fingerprints of the
    strip in its states and of the question at both window sizes.
- ⬜ **M11.22 `mcp`: Batch Edit as one step, and the workflows W07 and W10**
  - after: M11.8, M11.9
  - files: `crates/mcp/src/tools/edit.rs`, `crates/mcp/tests/edit_tools.rs`, `crates/mcp/tests/workflows/w07_batch_edit_one_step.rs`, `crates/mcp/tests/workflows/w10_import_duplicates.rs`, `xtask/src/stats.rs`, `docs/mcp.md`
  - scope: `mcp.md` §4.2 and §9: `apply_edit` runs `edit.apply`, one library undo step for all its
    photos as §4.2 says, instead of M10's per-photo steps; W07 (Auto Fix on 20 photos, 20 recipes
    ending in `autofix()`, one `cmd_edit_undo` removing all 20) and W10 (a card-like folder
    imported twice through the `cmd_import_*` tools: the second skips every photo and reports them
    as duplicates; the folders named as configured; each copy's bytes equal to its source)
    un-ignored; the floor in `xtask/src/stats.rs` raised to the eleven workflows now required;
    `mcp.md`'s status line; row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp --test workflows` runs W07 and W10 green on the three OSes;
    `crates/mcp/tests/edit_tools.rs`: `apply_edit` on 3 photos is one undo step; `cargo xtask
    stats` prints 11 / 12 passing and `stats --check` fails below that.
- ⬜ **M11.21 `ui`: the slideshow's control bar, zoom, and Options ▸ Slideshow**
  - after: M11.19
  - files: `crates/ui/src/panels/slideshow/bar.rs`, `crates/ui/src/panels/slideshow/zoom.rs`, `crates/ui/src/commands/slideshow.rs`, `crates/ui/src/dialogs/options/slideshow.rs`, `crates/ui/src/dialogs/folder_properties.rs`, `crates/ui/tests/slideshow_bar.rs`, `crates/ui/tests/fingerprints/slideshow-bar-*.json`, `crates/ui/tests/fingerprints/dialog-options-slideshow-*.json`, `docs/viewing.md`
  - scope: `viewing.md` §1.3 and §1.5: the bar on M11.3's overlay bar, shown for 2 s at start and
    on a mouse move, hiding after 2.5 s: Exit, Zoom (the magnifier and its slider; zooming pauses,
    a drag pans, + and − step), rotate left and right, Previous (disabled on the first photo
    without Loop), Play / Pause, Next, Star, the Transition drop-up of the seven, the Captions
    toggle and Display time `[−] 3 seconds [+]`, each kept through `options.set`; the edit view
    caption bar's slideshow toggle bound to the same captions setting; Options ▸ Slideshow (Loop,
    Display time, Transition and Show captions live; Play music, its folder and Play video sound
    drawn disabled with their reasons) and Folder Properties' Music with the same reason;
    `viewing.md` §1 records the decisions above; rows `VIEW-SLIDESHOW`, `VIEW-MUSIC` (Loop),
    `PREF-SLIDESHOW`, `EDIT-VIEW` (the caption bar's toggle).
  - done when: `crates/ui/tests/slideshow_bar.rs` (headless, a virtual clock): the bar shows at
    start, hides after 2 s and comes back on a mouse move; Previous is greyed on the first photo
    without Loop; Wipe chosen in the drop-up is the next transition and survives a restart; + and
    − change the display time within 1–60; zooming pauses and a drag pans; the captions toggles of
    the edit view and of the bar change one setting; Options ▸ Slideshow's Loop wraps the show;
    fingerprints of the bar and the tab at both window sizes.
- ⬜ **M11.23 `engine` + `ui` + `xtask`: M11's budgets: import, batch edit and the slideshow**
  - after: M11.8, M11.9, M11.19
  - files: `crates/engine/benches/import.rs`, `crates/engine/benches/batch_edit.rs`, `crates/ui/benches/slideshow.rs`, `xtask/src/bench.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: `development.md` §8: benches registered in `xtask/src/bench.rs`, joined to `BUDGETS` and
    the release-budgets job, each documented in §8 and §13 with its machine: `import` (listing,
    then importing, 1 000 `scenes` JPEGs from a temp card: time to the first thumbnails, files per
    second, MB/s), `batch_edit` (Auto Fix on 1 000 photos as one step, then its undo), `slideshow`
    (an advance to the first frame of its transition with prefetch, and a transition frame's
    paint, headless); budgets at 1.5 × their first measurement, plus a transition frame within
    16 ms at p95 and zero waits on disk whatever the first numbers; a missed budget is fixed in
    this task (before and after numbers in the PR) or blocks with its numbers, never loosened.
  - done when: `cargo xtask bench import`, `bench batch_edit` and `bench slideshow` print their
    numbers (in the PR); `cargo xtask budgets` passes with them and `--plant-ms` makes each fail.
- ⬜ **M11.24 `accept M11`**
  - after: M11.14, M11.16, M11.17, M11.18, M11.20, M11.21, M11.22, M11.23
  - files: `tests/acceptance/m11.rs`, `tests/acceptance/m11_*.rs`, `tests/acceptance/m11.md`, `tests/acceptance/fingerprints/m11-*.json`
  - scope: every *Done when* of M11's four areas in the real `dalica --headless` over the control
    channel, through `dalica-cli mcp`, and with `dalica --view FILE --headless`: a simulated card (a
    temp folder through `DALICA_DEVICE_ROOTS`) imported into dated folders with every copy
    verified, a second import skipping every photo as a duplicate, a planted clash getting `-001`,
    removal from the card only after the question and refused over the control channel; a 100-photo
    batch as one undo step; a version kept from A│A as one history step; the slideshow's zero waits
    on a slowed decoder, a fingerprint per transition, its keys, wheel and bar; `dalica --view`
    opening, stepping, starring and Open in Dalica; W07 and W10 over MCP; the M11 budgets (the
    release budgets with `DALICA_ACCEPT_RELEASE=1`; inside `cargo xtask ci` a smaller debug run
    checks the paths and prints its times); fingerprints of the room tabs, the Import room,
    compare, the slideshow and its bar and the viewer; the manual checks in `m11.md` (import from a
    real card or USB stick and remove the copies from it, a slideshow on a real screen with each
    transition, the viewer from the file manager's Open with on each OS with its translucent
    backdrop, Ctrl+Alt over a thumbnail).
  - done when: `cargo xtask accept M11` passes everything in M11's *Done when*.
- ⬜ **M11.25 Lead: task blocks for M13**
  - after: M10.28
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M13 task block from M13's outline rows and the spec (`faces.md`,
    `organizing.md` §6–§7, `xmp-interop.md` §8.5), with waves and the `M13.R` release gate (owner:
    maintainer); tasks that hinge on reserved decisions 4, 5 and 9 (map tiles, face models, the TLS
    stack) wait for the maintainer's answers, asked at the review before M13 (M12.R), never
    guessed; a block that can port Craft-app code names the source files in its scope
    (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ⬜ **M11.R Release 0.6 gate**
  - after: M11.24
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 0.6): the lead runs `/milestone-review M11.R` to prepare it, with
    the version set to 0.6.0 and its `whats-new.md` section on the review branch, the release
    checklist and the sign-off question, and after the merge cuts and publishes 0.6.0
    (`releasing.md`); the check-in page (open Import with Ctrl+M on a card or USB stick with
    photos, see them grouped by time, exclude a few, star and rotate one in the preview, import
    into Date taken folders, import again and see every photo skipped as a duplicate, try "Delete
    copied files from the card" on a test stick, undo an import; compare two photos in A│B, try two
    edits in A│A and keep one, hold `\`; Copy All Edits and Paste All Edits; Batch Edit ▸ Auto Fix
    on 20 photos and one Ctrl+Z; Undo All Edits; play a slideshow from a folder, an album and a
    search with each transition, captions, Loop, the display time, zoom, rotate and star, then Esc;
    hold Ctrl+Alt over a thumbnail and turn the wheel; open a photo with `dalica --view` and from
    the file manager's Open with, step, zoom, 1:1, star, Space, E, and once more with Dalica
    already open; written as steps for the maintainer to try); the questions: how slideshow music
    should play (a crate linking the system audio library on Linux, an MPL-2.0 decoder, or music
    later with M15's video sound), whether `dalica --view` should hand the file to a Dalica already
    running instead of opening read-only, the planning decisions above that change behaviour (an
    import is one undo step that trashes unchanged copies and cannot be undone once the card was
    emptied; duplicates and excluded files stay on the card; Undo All Edits can be undone; Batch
    Edit's effects greyed until M12; what Ctrl+4 and the Slideshow button play; the slideshow takes
    the OS full screen and leaves videos out until M15; compare's zoom synced; a room replaces
    everything but the menu bar, the strip and the status line; the viewer's first change adds its
    folder as Scan Once), the look of the Import room, the slideshow bar and the viewer (approved
    with their fingerprints); *Where we stand* and the `status.md` numbers (the workflow pass rate,
    import files per second and MB/s, Batch Edit of 1 000 photos, the slideshow's advance and
    transition frame at p95, waits on disk) on a `review/M11.R` branch, a pull request open. Only
    the maintainer merges that pull request and closes this issue; nothing after it starts until
    then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M11.1 engine modules + settings  ‖  M11.2 ui state + stubs  ‖  M11.3 ui-kit tabs, bars, icons
         ‖  M11.4 slideshow sequencing  ‖  M11.5 cards + import listing  ‖  M11.6 import copy + removal
         (once M10.R is closed)
wave 2   M11.7 import sources + duplicates  ‖  M11.9 batch edit + recipes  ‖  M11.10 preview panes + variant
         ‖  M11.12 viewer session + --view  ‖  M11.13 room tab strip
wave 3   M11.8 importing + undo + removal  ‖  M11.11 slideshow list + prefetch  ‖  M11.14 quick preview
         ‖  M11.15 import room: tray + preview  ‖  M11.16 compare + before/after  ‖  M11.17 photo viewer
         ‖  M11.18 batch edit in the window
wave 4   M11.19 slideshow  ‖  M11.20 import room: destination + finish  ‖  M11.22 apply_edit, W07, W10
wave 5   M11.21 slideshow bar + Options  ‖  M11.23 M11 budgets
wave 6   M11.24 accept M11 → M11.R          (M11.25 lead: M13 blocks, any time after M10.28)
```

### M12 – M14: P1 parity (outline; the lead details each one while the one before runs)

| # | In | Done when (sketch) |
|---|---|---|
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
10. Application id (reverse-DNS, used by packaging, desktop files and the settings path).
    **Decided 2026-10-11 (the maintainer): `org.dalica.Dalica`**, the reverse of `dalica.org`
    (which the maintainer owns) plus the app's name; three elements so Flathub accepts it. It lives
    in one place, `dalica_core::app_id::APP_ID` (M6.1), and names the window (Wayland app id, X11
    class). The settings and data folders stay as `data-model.md` §4 says.
11. Coordinate frame for normalised coordinates (`data-model.md` §3.3) (M7). **Decided 2026-10-12
    (the maintainer, on #268): option (b)**, the photo after its EXIF orientation and before
    Dalica's own rotations and flips, converted only for display. A rotation or flip stays a
    one-entry recipe change; crops, retouch spots, focal-effect centres and face rectangles are
    never rewritten, and faces found on the upright decode stay valid. Built in M7.1
    (`dalica-edit` `frame`).
