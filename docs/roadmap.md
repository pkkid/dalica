# Roadmap

Milestones for Dalica and their task blocks, with task ids used in commit subjects (`M1.3: …`).
This file is the plan; the live state of every task (ready, claimed, in review, done) is the
GitHub task board, kept in sync by `cargo xtask tasks` ([`task-board.md`](task-board.md)). Agents
get work with `cargo xtask tasks next`, never by scanning this file.

Status legend: ✅ done · 🚧 in progress · ⬜ not started. Task blocks use only ⬜ and ✅; their live
state (ready, claimed, in review) is on the task board ([`task-board.md`](task-board.md)).

## Where we stand

*Honest assessment, one dated entry per review, newest first (latest: 2026-10-18, M13.R). Agents:
read this before picking work. Update it only with measured facts, dated.*

*2026-10-18 (M13.R, the Release 0.8 gate).* M13 is done: `cargo xtask accept M13` passes (64 s warm), and the tests
on `main` went from 3 749 to 4 158. People: a pure-Rust ONNX interpreter (fuzzed, with hard limits) runs YuNet, SFace
or AuraFace, downloaded only after the licence is accepted (resumable, hash-pinned, self-tested) or added from a
file; the face scan runs in the background at the lowest priority, pauses while the user works, and never makes the
window wait; the Unnamed grid groups faces, naming a group is one undo step that survives a restart and a rebuild,
removed and ignored faces never return; suggestions above the calibrated threshold and margin with Confirm, Reject
and Find More; the People group, person sections, the People pane and faces drawn by hand on a turned photo; names
written to and read from MWG regions; faces guide crop suggestions and Pan and Zoom. Places: geotags written to EXIF
GPS and XMP and read on scan (a removed place stays removed), the Places pane with an offline map, drag-to-geotag and
the pin, and opt-in OpenStreetMap tiles and place search; a loopback test proves no connection without the user's
action. On the public-faces corpus (45 public-domain portraits of 10 people, now in `dalica-corpus`) detection finds
45 of 45, every group is one person, AuraFace keeps all 10 together and SFace 8 of 10 at the default confidence.
The effect speed lane met the caps: every draft ≤ 16 ms and final ≤ 100 ms at p95 on two cores (Focal Zoom 2.7 s →
66 ms), now judged as budgets. Without the people permission automation sees no names, rectangles or face points, and
`person:` searches match nothing. Not done: the two checks by hand (EDIT-ZOOM pinch, SYS-WINDOW on macOS); budgets are
set on a fast core and may miss on the 2-vCPU runner. The run stopped once for 50 minutes on three reserved decisions
nobody had asked (asked early at this review), found a data-loss gap before any writer landed (M13.45), and broke
`main` twice through a dependent's test (now caught: M13.50). 51 tasks merged with 7 workers. Parity ✅ 172 · 🟡 47 ·
⬜ 29 · 🚫 17. M13.R is the 0.8 release gate (version 0.8.0).

*2026-10-18 (M12.R, the Release 0.7 gate).* M12 is done: `cargo xtask accept M12` passes in 122 s (309 s with the
release budgets), and the tests on `main` went from 3 545 to 3 756. The edit view's tabs 3–5 hold all 27 effects:
tile grids that preview each effect on the photo in the background (tab 3 fills in 170 ms at 24 MP with the release
build, budget 355), one-click effects, a parameter panel with sliders, colour swatches, a picker and an eyedropper,
Fade and a live preview, a crosshair for the four focal effects that lands where it was put on a turned photo, and
Soften's brush. Every effect is one step of the photo's history named after it, survives a restart and a rebuild and
shows on its thumbnail; Border, Widescreen Film, Drop Shadow and Museum Matte grow the picture, and every size the
app reads accounts for it. Batch Edit's five effects are live, W11 passes over MCP (12 of 12 workflows), and the
slideshow's upload frame dropped from 14–22 ms to about 4 ms at p95. Not done: effects are slow at large sizes (at 24 MP
on two cores 15 of 27 finals miss the 100 ms cap, Focal Zoom 2.7 s; the caps are M13.34–M13.43, by the maintainer's
answer), and the two checks by hand move to M13.R. The run found and fixed a render deadlock (a one-time setup running
pool work inside a band, M12.7 and M12.28; Soften 767 → 254 ms) and, at the review, four visual faults no gate saw
(frame tiles cropped, a cut label, a rasterizer gap in screenshots, a barely visible Drop Shadow; M12.29–M12.32), and
a `main` left red for 50 minutes by an edit-only change that broke a ui test. 33 tasks merged with 7 workers. Parity
✅ 167 · 🟡 42 · ⬜ 39 · 🚫 17. M12.R is the 0.7 release gate (version 0.7.0).

*2026-10-17 (M11.R, the Release 0.6 gate).* M11 is done: `cargo xtask accept M11` passes in 111 s (480 s with the
release budgets), and the tests on `main` went from 3 304 to 3 545. Import from cards, drives and folders: photos
grouped by capture time, duplicates against the library and within the batch, every copy checked byte for byte
and keeping its time, nothing ever replaced (a taken name gets `-001`), the import one undo step, and the copied
files removed from a card only after checking and an explicit question, never over automation. Compare in A│B and
A│A (every tool works on the variant; keeping it is one history step), before/after on `\`, Copy and Paste All
Edits, Batch Edit as one undo step over the Photo Tray, and Undo All Edits. A slideshow with seven transitions,
captions, Loop and its control bar; with every decode slowed to 1.5 s, 30 changes waited 0 times, and the change
frame dropped from 28 ms to 0.27 ms at p95. The Ctrl+Alt quick preview, and `dalica --view` as a standalone viewer
that decodes any picture by path. 11 of the 12 agent workflows pass over MCP. Release budgets met: import 27.9
files/s (≥ 22.9), Batch Edit of 1 000 photos 4.3 ms (≤ 7), cold start at 100 000 photos 193 ms (≤ 2 s); the
transition frame's budget was raised to 1 ms by the maintainer after one loaded miss at 0.32 ms. 29 tasks merged in
4 h 30 min with 7 workers; seven send-backs, four of them for data safety (paste and keep-right dropping recipe
entries Dalica can't read, automation reaching card removal or an unchecked destination). The full run found two
faults no task gate saw, fixed on the review branch: an acceptance picture left stale by an enabled toggle, and a
transition easing that dropped a photo in the last 1e-4 of a change. Not done: no slideshow music or videos (M15),
no camera import over cable (M15), Batch Edit's effects greyed until M12, `dalica --view` read-only while Dalica is
open (hand-off in M15), and the two checks by hand moved to M12.R. Parity ✅ 160 · 🟡 42 · ⬜ 44 · 🚫 17. M11.R is the
0.6 release gate (version 0.6.0).

*2026-10-16 (M10.R, the Release 0.5 gate).* M10 is done: `cargo xtask accept M10` passes in 68 s (158 s with the
release budgets), and the tests on `main` went from 3 098 to 3 304. Tools ▸ Options… has its four tabs (General with
UI scale, font size, tooltips and animations; File Types, where turning a type off rescans and keeps its data;
Export defaults; Keyboard Shortcuts, an editor with two keys per command, named conflicts and Reset All), all through
`options.set` and `app.setShortcut`. `dalica-cli mcp` is an MCP server, headless on a library or connected to a running
`dalica --control`, with one tool per command plus helpers and resources, read and write roots and a read-only mode;
9 of the 12 agent workflows pass over real stdio (the other three need M11 and M12), and connect mode gives the same
results as headless. P0 parity: 109 of 111 rows ✅, the other two waiting only for checks a person makes (the Windows
touchpad pinch, the macOS build used by hand). Release budgets met: scan 22 779 photos/s (≥ 500), cold start at
100 000 photos 200 ms median (≤ 2 s), idle memory 84 MiB (≤ 600). Testing changed mid-run to the changed packages per
task and everything once here; the first full run then found breaks no task gate saw (finished milestones' acceptance
pictures, macOS and Windows path and timing assumptions), fixed in nine tasks before this review. The run also found
and fixed four data-safety bugs before anything shipped: a journal append that failed part-way lost the records after
it; a caption written into an iPhone or camera JPEG broke its secondary images; on Windows a Save killed mid-way left
the original under two names; and "nothing pending" was reported while a sidecar write was still a second away. Not
done: release builds have no HEIC decoder (HEVC patent licensing; HEIC photos are organized and show a tile), 12-bit
JPEG is not decoded, and one timing miss is unexplained (the first thumbnails at 10 000 photos took 6.8 s once under
load, 0.5 s on a re-run). Parity ✅ 147 · 🟡 36 · ⬜ 62 · 🚫 17. M10.R is the 0.5 release gate (version 0.5.0).

*2026-10-15 (M9.R, the Release 0.4 gate).* M9 is done: `cargo xtask accept M9` passes in 93 s (92 s with the
release budgets), and the tests on `main` went from 2 662 to 3 098. Albums gather photos from any folders in a
manual order kept in each folder's `.dalica.json` and in `albums.json`, so an album and its order survive a rebuild
or a lost `albums.json`; every album command undoes. Search answers as you type on a worker (at 100 000 photos: the
index builds in 428 ms, a word answers in 1.1 ms, typing to results 10.6 ms), with filters, the age slider, a banner,
Save Search Results and Show Duplicate Files (identical bytes only). Photos and folders move, copy, rename and go to
the trash through journaled file jobs: within a drive a rename that never replaces, across drives a verified copy,
each step undoable, with captions, tags, stars, edits, album places and the saved original travelling with the file.
Interrupting every step of every operation (about 690 cases, real crashes included) leaves every photo in one place
with its data after the next start. The run's own tests found and fixed seven ways data could have been lost or
detached before anything shipped (two in the plan, three in the interruption sweep, one in undo, one for folders
moved between sessions). Not done: Recently Updated is hidden until its counting rule is decided; duplicates show in
one section; `color:` and `person:` find nothing yet; a delete cannot be undone on a Mac; on Linux a file name over
about 245 bytes cannot be trashed; and since 0.3 the picture can stay the pre-Save one until a restart (found at this
review, no data at risk). Parity ✅ 29 · 🟡 72 · ⬜ 46 · 🚫 16. M9.R is the 0.4 release gate (version 0.4.0).

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
  checks of earlier milestones keep running at every review (`cargo xtask ci --full`), so nothing
  silently regresses.
- **Test little during a milestone, everything at its review** (maintainer decision, 2026-10-15).
  A task's gate (`cargo xtask ci`) compiles everything and tests only the packages the task
  changed, quick; a merge runs no tests. The review runs the full list once: every test at full
  strength and every finished milestone's acceptance (`cargo xtask ci --full`), then, on the
  review's pull request, CI on Linux, Windows and macOS, the corpus job, the release budgets and
  fuzzing. What fails there becomes fix tasks, merged before the review closes
  (`development.md` §3). A *done when* that names other systems or budgets is checked there.
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
| Organize | M9 | Albums, search and file operations | lasting albums, instant search and filters, move/copy/rename/delete | ✅ |
| | M10 | Preferences, MCP and Release 0.5 | Options and shortcut editor, MCP and the P0 workflows; all P0 rows shipped | ✅ |
| P1 parity | M11 | Import, compare and viewing | import from cards and drives, compare and batch edit, slideshow, the viewer | ✅ |
| | M12 | Effects | effect tiles, parameter panels and all 27 effects | ✅ |
| | M13 | People and places | local faces, the People pane, the Places pane and geotagging | ✅ |
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
- ✅ **M9.23 `ui`: drag and drop, the drag pill, and Cut, Copy and Paste**
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
- ✅ **M9.24 `engine`: M9's data lessons end to end, and a move across two file systems**
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
- ✅ **M9.25 `accept M9`**
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
- ✅ **M9.R Release 0.4 gate**
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
  corpus (non-Apple, 10/12-bit, alpha) first. Decided at M10.R (2026-10-16): no, release builds ship
  without an HEVC decoder (no patent licence) and the feature stays for self-builds (M10.56).
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

- ✅ **M10.1 `engine`: M10's command modules, the options catalogue and settings**
  - after: M9.3, M9.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/options.rs`, `crates/engine/src/cmd/keymap.rs`, `crates/engine/src/cmd/inspect.rs`, `crates/engine/src/cmd/recipe.rs`, `crates/engine/src/options.rs`, `crates/engine/src/settings.rs`, `crates/engine/src/session.rs`, `crates/engine/src/undo.rs`, `crates/engine/tests/settings.rs`, `crates/engine/tests/options_catalogue.rs`, `crates/engine/tests/execute_group.rs`, `crates/engine/README.md`, `docs/menus-and-shortcuts.md`, `docs/data-model.md`
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
  - done when: `crates/engine/tests/settings.rs` round-trips every new setting with unknown
    keys kept and wrong types falling back (reported); `crates/engine/tests/options_catalogue.rs`:
    every option of §5's three tabs is in the catalogue once, each default equals its accessor's,
    keys are unique; `crates/engine/tests/execute_group.rs`: a star, a caption and a tag in one
    group are one undo step and undo restores all three, a failing third step leaves the model and
    the sidecars unchanged; every existing engine test and `panic_hunt` pass unchanged.
- ✅ **M10.2 `ui`: M10's view state, UI commands, the Options dialog's stubs and control stubs**
  - after: M9.4, M9.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/app.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/options.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/options.rs`, `crates/ui/src/dialogs/options/general.rs`, `crates/ui/src/dialogs/options/file_types.rs`, `crates/ui/src/dialogs/options/export.rs`, `crates/ui/src/dialogs/options/shortcuts.rs`, `crates/ui/src/scale.rs`, `crates/ui/src/control/mod.rs`, `crates/ui/src/control/view.rs`, `crates/ui/src/control/batch.rs`, `crates/ui/src/control/inspect.rs`, `crates/ui/tests/view_state.rs`, `docs/control-protocol.md`
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
  - done when: `crates/ui/tests/view_state.rs`: each new field round-trips through `ui.state`
    / `ui.set` (bad values are `bad_params`, nothing changes); Ctrl+, opens `dialog:options` on
    General and Ctrl+/ on Keyboard Shortcuts, Esc closes it; Tools ▸ Options…, Help ▸ Keyboard
    Shortcuts… and the macOS Preferences… item light up; every command's graceful-failure test;
    `panic_hunt` and `widget_ids` stay green.
- ✅ **M10.3 `ui-kit`: text tabs, checkbox rows, key cells and the editor's icons**
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
- ✅ **M10.4 `mcp` + `dalica-cli`: the MCP server's core and headless mode**
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
- ✅ **M10.21 `xtask`: the P0 parity gate, `KEY-*` rows from the keymap, and workflow statistics**
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
- ✅ **M10.22 `engine` + `ui`: the P0 budgets left: scan throughput, cold start and idle memory**
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
- ✅ **M10.23 `image` + `scenes`: the P0 formats measured**
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
- ✅ **M10.24 `image` + `xtask`: HEIF measured on a wider set, for the M10.R decision**
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
- ✅ **M10.5 `engine`: `options.list` and `options.set`, and applying them**
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
- ✅ **M10.6 `engine`: the keymap listing, two keys per context and named conflicts**
  - after: M10.1
  - files: `crates/engine/src/cmd/keymap.rs`, `crates/engine/src/cmd/app.rs`, `crates/engine/src/keymap.rs`, `crates/engine/tests/keymap.rs`
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
  - done when: `crates/engine/tests/keymap.rs`: the listing covers every command of the
    registry and `UI_COMMANDS` once per context it has keys in, and the ones without keys when
    asked; search by name, menu path and key; a second key in slot 1 resolves beside the first; a
    conflict names the other binding and `replace` moves the key; Ctrl+1 in Library and in Edit
    view is no conflict; Star rebound to F8 resolves F8 to `photo.star.toggle` in Library and
    Ctrl+8 no longer does; the rebinding survives a restart with unknown settings keys kept;
    `app.resetShortcuts {}` restores every default binding exactly; each command's contract and
    graceful failures; `panic_hunt` green.
- ✅ **M10.7 `engine` + `ui` + `mcp`: `library.inspect` everywhere**
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
- ✅ **M10.8 `ui`: `batch`, replies that wait for their job, and the headless capture in the window**
  - after: M10.2
  - files: `crates/ui/src/control/batch.rs`, `crates/ui/src/control/engine.rs`, `crates/ui/src/control/jobs.rs`, `crates/ui/src/control/screenshot.rs`, `crates/ui/tests/control_batch.rs`, `crates/ui/tests/control_wait.rs`, `apps/dalica/src/control/play.rs`, `apps/dalica/tests/control_headless.rs`, `docs/control-protocol.md`
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
    `apps/dalica/tests/control_headless.rs`: a headless capture through a windowed session's handlers
    equals `--headless`'s byte for byte; graceful failures; `panic_hunt` green.
- ✅ **M10.9 `edit` + `engine`: the edit-operation catalogue and `edit.recipe.set`**
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
- ✅ **M10.13 `mcp`: the organizing helper tools**
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
- ✅ **M10.16 `engine` + `mcp`: roots, permissions and read-only sessions**
  - after: M10.1, M10.4
  - files: `crates/engine/src/automation.rs`, `crates/engine/src/session.rs`, `crates/engine/tests/automation_roots.rs`, `crates/engine/tests/read_only.rs`, `crates/ui/src/control/paths.rs`, `crates/mcp/src/roots.rs`, `crates/mcp/tests/roots.rs`, `docs/security.md`, `NOTICE`
  - scope: `mcp.md` §5 and `control-protocol.md` §5.6–§5.7, with PhotoCraft
    `crates/automation/src/workspace.rs` (`AuthorizedWorkspace`) at 583425b4 as the pattern: the
    path policy moved from `ui`'s `control/paths.rs` into `engine` (`automation.rs`: read and
    write roots, the default write roots (the OS temporary directory and
    `<cache>/Dalica/automation/`), new files inside a watched folder only as
    `control-protocol.md` §5.7 allows (an export or a Save As; decided at M10.R, 2026-10-16), never
    inside `.dalica-originals/`, never over an existing file, links resolved before the check), the control channel calling it; MCP's
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
- ✅ **M10.25 `ui`: the window's last P0 gaps**
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
- ✅ **M10.10 `ui`: the Options dialog, its General tab, UI scale and font size**
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
- ✅ **M10.14 `mcp`: the edit, render and export helper tools**
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
- ✅ **M10.15 `mcp`: one tool per command, and the resources**
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
- ✅ **M10.11 `ui`: the File Types and Export tabs**
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
- ✅ **M10.12 `ui`: the shortcut editor and Help ▸ Keyboard Shortcuts**
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
- ✅ **M10.17 `mcp` + `dalica-cli`: connect mode**
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
- ✅ **M10.18 `mcp`: the workflow harness and the M6 and M8 workflows**
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
  - done when: `cargo test -p dalica-mcp --test workflows` runs W02, W03, W08 and W09 green (on all
    three OSes at the review) and lists the other eight as ignored with their reasons.
- ✅ **M10.19 `mcp`: the M9 workflows**
  - after: M10.18
  - files: `crates/mcp/tests/workflows/w04_rebuild_loses_nothing.rs`, `crates/mcp/tests/workflows/w05_move_carries_data.rs`, `crates/mcp/tests/workflows/w06_search_save_results.rs`, `crates/mcp/tests/workflows/w12_destructive_asks.rs`
  - scope: `mcp.md` §9: W04 (star, caption, tag, hide, edit and album, then `library.rebuild
    {confirm}`: `doc_inspect` and `photo_info` equal before and after); W05 (a move of 3 starred,
    captioned, saved photos carries their sidecar entries and originals backups, nothing left
    behind, undo moves them back); W06 (10 tagged, 2 captioned "tram", `query_photos` and `tag:`
    queries, the search saved as an album); W12 (`cmd_photo_delete` without `confirm` answers
    `needs_confirm` with the file still there; with `confirm` the file is in the test trash and the
    undo record keeps its sidecar entry).
  - done when: `cargo test -p dalica-mcp --test workflows` runs W04, W05, W06 and W12 green (on all
    three OSes at the review), with the files on disk checked after each step and after undo.
- ✅ **M10.20 `mcp`: W01, the stdio smoke run and the workflow floor**
  - after: M10.17, M10.18, M10.21
  - files: `crates/mcp/tests/workflows/w01_organize_export.rs`, `apps/dalica-cli/tests/mcp_process.rs`, `xtask/src/stats.rs`, `docs/mcp.md`, `docs/process.md`
  - scope: `mcp.md` §9 and `process.md` §5: W01, the headline (`scan_folder` 200 photos → star 5
    → `create_album` from them → caption 3 → crop and Auto Fix one → `export` the album at 1600 px
    → 5 JPEGs with the long edge ≤ 1600, captions in XMP and IPTC, the crop visible in the output,
    originals byte-identical); a smoke subset (W02, W03) through `dalica-cli mcp` spawned with real
    stdio; the floor in `xtask/src/stats.rs` raised to the nine P0 workflows; `mcp.md`'s status
    line says what is built and `process.md` §5 names the stats command; row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp --test workflows` runs W01 green (on all three OSes at the review);
    `apps/dalica-cli/tests/mcp_process.rs` passes; `cargo xtask stats` prints 9 / 12 passing and
    `cargo xtask stats --check` fails when a P0 workflow is ignored (a planted `#[ignore]`).
- ✅ **M10.29 `library` + `engine` + `ui`: Recently Updated counts changes, and shows**
  - after: M9.R
  - files: `crates/library/src/model.rs`, `crates/library/src/query.rs`, `crates/library/tests/m9_fields.rs`, `crates/engine/src/session.rs`, `crates/engine/tests/recently_updated.rs`, `crates/ui/src/panels/light_table.rs`, `crates/ui/tests/albums_tree.rs`, `crates/ui/tests/tree.rs`, `docs/organizing.md`, `docs/data-model.md`, `crates/sidecar/src/fields.rs`, `crates/sidecar/src/tests.rs`, `crates/sidecar/tests/fields.rs`, `crates/sidecar/README.md`, `crates/store/src/mapping.rs`, `crates/store/src/rebuild.rs`, `crates/store/tests/fields.rs`, `crates/store/tests/m9_fields.rs`, `crates/store/README.md`, `crates/scan/src/walk.rs`, `crates/scan/src/diff.rs`, `crates/engine/tests/albums.rs`, `crates/engine/tests/captions.rs`, `crates/engine/tests/search.rs`, `crates/engine/tests/tags.rs`, `crates/engine/README.md`, `crates/library/tests/search.rs`, `crates/library/tests/tree.rs`, `crates/library/README.md`, `crates/ui/tests/add_to_album.rs`, `crates/ui/tests/adjust_date.rs`, `crates/ui/tests/bottom.rs`, `crates/ui/tests/captions.rs`, `crates/ui/tests/common.rs`, `crates/ui/tests/hidden.rs`, `crates/ui/tests/library_window.rs`, `crates/ui/tests/section_header.rs`, `crates/ui/tests/fingerprints/**`, `docs/parity.md`
  - scope: the maintainer's answer at M9.R (Q1): a photo is recently updated by the last change
    after this library first indexed it (an edit, a caption, tags, a star, a rotation, a date, a
    file changed on disk); being imported is not a change, so a new library's Recently Updated
    starts empty. `Library::updated_time` takes the later of the last Dalica change and the file's
    modification time only when that is after the first-indexed time. Every command that changes a
    photo's data is stamped (in `Session::execute`, not in undo or redo), not only the album
    commands and stars (M9.10's open item). Decide how the last-change time survives a rebuild
    (preference: a `changed` time in the photo's sidecar entry, so a rebuild keeps it and M6's
    rebuild check stays exact; the alternative is index-only, emptying it on a rebuild). Then turn
    `SHOW_RECENTLY_UPDATED` on: the tree row and the light-table section (`organizing.md` §4), and
    the release notes' line is removed. Whole-window baselines that change are re-blessed.
  - done when: tests: a fresh scan leaves Recently Updated empty; a caption, a star, a rotation and
    a file rewritten on disk each put the photo first, on a virtual clock, and it leaves after the
    setting's days; undo of the change restores the earlier time; a rebuild keeps the list (or, if
    index-only was chosen, the commit body says why); the row and section show; CI green on the
    three OSes.
- ✅ **M10.31 `engine`: Export takes album sections and search results**
  - after: M9.R
  - files: `crates/engine/src/cmd/export.rs`, `crates/engine/tests/export_sections.rs`, `docs/creating.md`
  - scope: `export.run` with `section` set to a user album, `album:starred` or `album:search`
    exports that section's photos in its display order (the search results through
    `Session::search_results_now()`), as it does for folders; the Export button on an album's
    section header and on the search banner's results works through it (M9.10 and M9.11's open
    items).
  - done when: tests: an album over three folders exports in album order; the search results export
    the matches; Starred Photos exports; an unknown album id fails gracefully; CI green.
- ✅ **M10.32 `fs` + `engine`: the file-operation follow-ups from M9**
  - after: M9.R
  - files: `crates/fs/src/trash.rs`, `crates/fs/tests/trash.rs`, `crates/engine/src/fileops.rs`, `crates/engine/src/cmd/files.rs`, `crates/engine/src/scanning.rs`, `crates/scan/src/diff.rs`, `crates/engine/tests/file_followups.rs`
  - scope: three gaps M9's run found, none losing data today: on Linux a file whose name makes its
    `.trashinfo` record name too long (over about 245 bytes) cannot be trashed, so the delete stops;
    trash it under a shortened, unique name in the trash's `files/` with the original path in its
    record, and restore it under its own name (M9.24); undoing a copy of a saved photo leaves the
    copy's kept original and its record in the destination; trash them with the copy when unchanged
    (M9.13); a folder moved outside Dalica between two Scan Always folders is still seen as gone and
    new by the verification pass, because each is its own region scan; pair it as M9.24's
    `diff_found` does across regions.
  - done when: tests: a 255-byte name goes to the trash and comes back on Linux; undoing a copy of a
    saved photo leaves nothing behind; the two-unit move keeps ids, data and album places; CI green
    on the three OSes.
- ✅ **M10.33 `ui` + `engine` + docs: small follow-ups from M9**
  - after: M9.R
  - files: `tests/acceptance/m9_files.rs`, `crates/engine/tests/panic_hunt.rs`, `crates/ui/src/headless.rs`, `crates/ui/src/dialogs/new_folder.rs`, `crates/ui/tests/new_folder.rs`, `crates/engine/tests/albums.rs`, `crates/engine/README.md`, `docs/security.md`
  - scope: M9's live file-operations acceptance test fails now and then on Windows: right after
    `app::flush` it sees a sidecar write still in flight (a `.dalica.json.dalica-tmp-…` file in the
    folder listing, or the entry not yet there); find whether flush returns before the sidecar
    writer's rename on Windows (then the fix belongs in the engine: ask the lead for the file) or the
    test reads too early; `panic_hunt` builds valid
    parameters for M9's commands (rename, move, copy, delete, folder and album commands); `ui.settle`
    and the driver's busy check wait for a search still running; Split Folder Here's dialog is
    titled as a new folder, not "Folder Properties"; stale text: `security.md` §5.7 (deletes are
    built), the engine README's rename line, the comment of M9.10's folder-rename test, and the
    Tags row of `parity.md` (Search for This Tag is built).
  - done when: `file_operations_their_undo_and_the_data_lessons_in_the_live_app` passes 20/20 on
    Windows CI (a matrix rerun) or the cause is shown fixed by a test; `panic_hunt` green,
    printing how many M9 commands it reached with valid parameters; a settle test with a search
    pending; the dialog's title test; CI green.
- ✅ **M10.34 `fs` + `store`: real crashes, a full disk and read-only folders**
  - after: M9.R
  - files: `crates/fs/src/faults.rs`, `crates/fs/tests/crash.rs`, `crates/fs/tests/refusals.rs`, `crates/fs/tests/all.rs`, `crates/store/tests/crash.rs`, `crates/store/tests/all.rs`, `crates/fs/README.md`
  - scope: the data-safety cases the test audit of 2026-10-15 found untested (maintainer decision,
    2026-10-15, testing rework): a child process killed at a random moment (SIGKILL,
    `TerminateProcess`) while it writes, after the pattern of `crates/fs/tests/lock.rs`, for the
    atomic replace, the originals backup before a Save, the journal append and the intents file,
    then the parent checks that the old or the new bytes are there and nothing torn (50 kills at
    full strength, 5 quick); `Fault::Full` with the real error kind (`StorageFull`), so the cleanup
    and the reason the user sees are checked, plus one real case on Unix through a child under
    `RLIMIT_FSIZE`; a read-only folder (a locked card, a read-only share) refuses a Save before
    anything is written; an identity with inode 0 (some shares) never lets a rename replace; the
    parent folder removed between opening it and the rename, for the atomic replace and a
    transfer.
  - done when: `cargo test -p dalica-fs -p dalica-store` passes quick; the kill tests run 50 times
    each with `DALICA_FULL_TESTS=1` and never find a torn file; each refusal leaves the folder
    byte-identical and names the reason.
- ✅ **M10.35 `engine`: undo under faults and a day in the life**
  - after: M9.R
  - files: `crates/engine/tests/undo_faults.rs`, `crates/engine/tests/day_in_the_life.rs`, `crates/engine/tests/effects.rs`, `crates/engine/tests/all.rs`
  - scope: undo and redo of a move, a rename and a delete interrupted at the file-job points
    (one point per class quick, every point at full strength), as the forward operations already
    are; one end-to-end run: scan a folder, star, caption and tag, move to another folder, rename,
    undo the whole chain, quit, reopen and rebuild from the sidecars, then every photo's bytes and
    data compared with the start; a property test that a command whose spec says `Effect::Query`
    never changes the model or the undo stack and one with `History::None` never adds a step.
  - done when: `cargo test -p dalica-engine` passes quick and with `DALICA_FULL_TESTS=1`; a
    planted bug (undo of a move that skips the sidecar entry) fails `undo_faults`.
- ✅ **M10.36 `ui` + `dalica`: closing the window, display scales and a monkey test**
  - after: M9.R
  - files: `crates/ui/src/app.rs`, `apps/dalica/src/window.rs`, `crates/ui/tests/shutdown.rs`, `crates/ui/tests/scale.rs`, `crates/ui/tests/monkey.rs`, `crates/ui/tests/all.rs`
  - scope: the windowed exit (placement and view state saved, pending metadata writes flushed) and
    the headless exit share one `App::shutdown`, tested headless with writes still pending; five
    flows that depend on hit-testing and rounding (a crop drag, a straighten drag, a tray drag, the
    tree splitter, a dialog's buttons) at scale 1.5 and 2.0 as well as 1.0; a seeded monkey run
    (random clicks on registered widgets, keys, drags and window sizes from 600 to 2000 px wide)
    that asserts no panic, `state.check()` passing, no unregistered widget and no disk access on
    the UI thread (200 frames quick, 2 000 at full strength).
  - done when: `cargo test -p dalica-ui -p dalica` passes quick and at full strength; closing with
    a caption still pending leaves it in the sidecar.
- ✅ **M10.37 `meta` + `image`: metadata written to real camera files, and colour without a profile**
  - after: M9.R
  - files: `crates/meta/tests/corpus.rs`, `crates/meta/tests/write_jpeg.rs`, `crates/image/tests/corpus.rs`, `crates/image/src/formats/jpeg.rs`
  - scope: the metadata write-back invariants run over the real camera and phone JPEGs of the
    corpus (`development.md` §11), not only over files other programs wrote into our scenes: after
    a caption and a tag are written, every image an MPF index lists still starts with FFD8 at its
    offset, data after EOI (motion-photo and other trailers) is kept byte for byte, and the image
    data is unchanged; a JPEG marked Adobe RGB in EXIF (ColorSpace 2 or the R03 interop index)
    without an embedded profile decodes as Adobe RGB, with per-file decoded fingerprints in the
    corpus floors.
  - done when: `cargo xtask test-corpus` passes with the new floors; a write that shifts the
    secondary images without fixing the MPF offsets fails the corpus test.
- ✅ **M10.38 `fs` + `store`: a failed journal append never loses the records after it**
  - after: M10.34
  - files: `crates/fs/src/append.rs`, `crates/fs/tests/append.rs`, `crates/store/src/journal.rs`, `crates/store/tests/journal.rs`, `crates/store/README.md`
  - scope: found by M10.34 (PR #567): when an append fails part-way (a full disk), `Appender`
    keeps the half-written bytes and the next append writes after them, and the engine's I/O
    worker keeps appending after a failed one (`Msg::Journal` in `crates/engine/src/session.rs`),
    so the next open stops at the torn record ("record 3: checksum mismatch") and drops every
    later op the caller was told had landed. The journal's reading rule stays as it is (reading
    stops at the first bad record; nothing after it is trusted, `journal.rs`'s module doc):
    instead a failed append leaves the file as it was before it. `Appender::append` truncates
    back to the last known good length (`set_len`, then sync) after a failed or short write or a
    failed sync, before it reports the error, the injected `APPEND_WRITE` fault path included;
    if the truncate fails too, the appender refuses every later append until it is reopened (and
    the open cuts the torn tail as today). `Journal::append` counts an op only when its record is
    whole on disk. The rule is written down in `append.rs`'s and `journal.rs`'s module docs and
    `crates/store/README.md` (`data-model.md` and `crates/fs/README.md` belong to other open
    tasks).
  - done when: tests: in `dalica-fs`, a full disk (`faults::arm_full`, M10.34) on one append
    leaves the file at its earlier length and the next append lands whole; in `dalica-store`, a
    full disk armed on one journal append, two more appends, a reopen: both later ops are
    replayed and the failed one is absent; each fails before the fix; a failed truncate refuses
    the next append; `cargo xtask ci` green.
- ✅ **M10.39 `engine` + `ui` + `dalica-cli`: the three keys the KEY rows found unbound**
  - after: M10.4, M10.21
  - files: `crates/engine/src/cmd/photo.rs`, `crates/ui/src/commands/export.rs`, `crates/ui/src/commands/rename.rs`, `apps/dalica-cli/src/main.rs`, `crates/engine/tests/edit_keys.rs`, `crates/ui/tests/edit_keys.rs`
  - scope: found by M10.21 (PR #562): the `KEY-*` check in registry mode (`cargo xtask parity`,
    `parity --write`, the review gate) fails on three P0 rows. KEY-EDIT-SPACE: `editing.md` §12
    gives the edit view Ctrl+8 and Space for the star, but `photo.star.toggle` binds Ctrl+8 and F4
    there; a context holds at most two keys (`menus-and-shortcuts.md` §2.0), so the edit view's
    pair becomes Ctrl+8 and Space as §12 says and F4 stays in the Library (Space is free in the
    edit view until M15's video play/pause, which then needs its own context). KEY-EDIT-CTRL-SHIFT-S:
    `output.export` (`crates/ui/src/commands/export.rs`) binds Ctrl+Shift+S in the Library only;
    bind it in the edit view too (a Global key bound in both views reads ✅). KEY-GLOBAL-F2: the
    Rename opener (`crates/ui/src/commands/rename.rs`) binds F2, but `dalica-cli commands` lists
    the registry and `UI_COMMANDS`, not `dalica_ui::commands::OPENERS`, so its key is never seen:
    list the openers' keys there as well; if Rename works on the edit view's photo, bind F2 in
    the edit view too (✅), otherwise the row stays 🟡 with the reason. Each decision goes in the
    commit body; `parity --write` regenerates the rows (`parity.md` is a shared doc; on a rebase
    conflict, regenerate).
  - done when: `cargo xtask parity` exits 0 with the three rows ✅ (or F2 🟡, as above); tests:
    in the engine, Space in the edit view resolves to `photo.star.toggle` and stars the photo, with
    no other default in the edit view on Space; headless in the window, Ctrl+Shift+S in the edit
    view opens the Export dialog for the photo shown; `dalica-cli commands` lists the Rename
    opener with F2 (its own unit test); `cargo xtask ci` green.
- ✅ **M10.40 `ui` + `dalica` + `engine`: one way out, the edit view's caption kept, hidden photos out of `library.query`**
  - after: M10.36
  - files: `crates/ui/src/app.rs`, `crates/ui/src/panels/edit/caption_bar.rs`, `crates/ui/tests/shutdown.rs`, `apps/dalica/src/control/headless.rs`, `apps/dalica/tests/control_headless.rs`, `crates/engine/src/cmd/library.rs`, `crates/engine/tests/hide.rs`
  - scope: three gaps found by M10.36 (PR #574) and M10.18. The edit view's caption bar keeps its
    draft in egui's memory (`caption_bar.rs`, `caption:draft`), where `App::shutdown` cannot reach
    it, so closing the window while a caption is being typed there loses the text (the light
    table's inline editor is safe: its draft is `state.caption_edit`). Keep the draft in `UiState`
    beside `caption_edit` (a surgical edit of the shared `state.rs`, kept out of the saved view
    state as `caption_edit` is) and commit it in `App::shutdown` as the light table's is; leaving
    the photo, Enter and Esc behave as today. Headless `app.quit`
    (`apps/dalica/src/control/headless.rs`) flushes the session itself (no drafts committed, no
    view state saved, no wait for metadata writes): it calls `App::shutdown` instead, so both exits
    share one path, and its reply still reports a failed flush as `unsaved`. Found by M10.18:
    `library.query` with no `section` and no search condition (`{}` or only `folder`) lists every
    folder's photos from `photos_in`, hidden ones and those of hidden folders included, while a
    section or a search leaves them out, so MCP `query_photos {}` lists hidden photos (`mcp.md`'s
    W09: hidden photos leave `query_photos`); that branch leaves them out as `section_photos`
    does, unless the view shows hidden pictures (`search_tree_options`).
  - done when: tests, each failing before the fix: in `dalica-ui` (`shutdown.rs`), a caption typed
    in the edit view's caption bar and not committed, then `App::shutdown`, is in the photo's
    sidecar; in `dalica` (`control_headless.rs`), a light-table caption still being typed when
    `app.quit` arrives is in the sidecar after the process exits; in `dalica-engine` (`hide.rs`),
    a hidden photo and a photo of a hidden folder are absent from `library.query {}` and
    `library.query {folder}` and present again with the view showing hidden pictures; `cargo xtask
    ci` green.
- ✅ **M10.41 `ui`: the straighten bar's id, F2 in the edit view, the headless UI scale and the thumbnail's tooltip**
  - after: M10.10, M10.16, M10.36, M10.39
  - files: `crates/ui/src/panels/edit/straighten.rs`, `crates/ui/tests/straighten_tool.rs`, `crates/ui/tests/tuning_tab.rs`, `crates/ui/tests/edit_panel.rs`, `crates/ui/tests/monkey.rs`, `crates/ui/src/commands/edit_view.rs`, `crates/ui/src/commands/rename.rs`, `crates/ui/tests/edit_keys.rs`, `crates/ui/src/headless.rs`, `crates/ui/tests/ui_scale.rs`, `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/cells.rs`, `docs/control-protocol.md`
  - scope: four small window gaps found by M10.36 (PR #574), M10.39 (PR #589) and M10.10 (PR
    #593). The straighten tool's floating bar takes clicks under the egui id `straighten:bar` with
    no automation id (the `TODO(maintainer)` in `straighten_tool.rs`, `tuning_tab.rs` and
    `edit_panel.rs`, and `KNOWN_UNREGISTERED` in `monkey.rs`): register it (`box:straighten` in
    the edit view's row of `control-protocol.md` §7; after M10.16, which adds one row to that
    file), turn the id checks back on and drop the monkey's exception. F2 (Rename…) is a Global
    key (`menus-and-shortcuts.md` §2.1) but bound in the Library only, because the edit view's key
    dispatch (`commands/edit_view.rs`) runs a command with the open photo and never opens an
    opener's dialog: route a key bound to an opener there through the opener (as `keys::run_key`
    does), for the open photo, and bind F2 in the edit view too, so `KEY-GLOBAL-F2` reads ✅
    (`parity --write`; `parity.md` is a shared doc, and M10.26 starts after this). The headless
    driver (`crates/ui/src/headless.rs`) gives egui the screen rect at its own pixels per point and
    tessellates at them, ignoring egui's zoom, so a fixed UI scale leaves the headless layout and
    snapshots unchanged (real windows are right): fold the zoom into both, as the windowed
    integration does, so the 150 % fingerprints M10.27 makes headless are really at 150 %. The
    thumbnail's own 600 ms tooltip (`cell.rs`, `TOOLTIP_SECONDS`) shows with Options ▸ Show
    tooltips off (`interface.tooltips`; egui's own tooltips follow it through `scale.rs`): it
    follows the setting too.
  - done when: tests, each failing before the fix: the straighten tests and the monkey run with
    the id check on and no known exception; in `edit_keys.rs`, F2 in the edit view opens Rename
    Files for the photo shown; in `ui_scale.rs`, a 1400 × 900 pixel driver at 1.0 pixels per point
    with UI scale 150 % lays out 933 × 600 points and its screenshot is 1400 × 900 pixels; in
    `cells.rs`, with `interface.tooltips` off, resting on a cell past `TOOLTIP_SECONDS` shows no
    tooltip (on, it does); `cargo xtask parity` shows `KEY-GLOBAL-F2` ✅; `cargo xtask ci` green.
- ✅ **M10.42 `dalica` + `ui`: the app test and the caption fingerprint M10.29's section moved**
  - after: M10.29
  - files: `apps/dalica/tests/control_headless.rs`, `crates/ui/tests/captions.rs`
  - scope: found by the full gate (GitHub run 38832985226, PR #597): M10.29 turned the Recently
    Updated section on (`organizing.md` §4.1), so a starred photo now has two copies in view and
    the window draws two star badges for it. The work exists as commit `aaeb457` on branch
    `fix/control-headless-star-badge` (PR #597, test-only): cherry-pick it. In
    `control_headless.rs`, `an_agent_stars_a_photo_by_click_and_key_and_takes_a_screenshot` reads
    the star badge on both in-view copies (Starred Photos and Recently Updated) and checks each
    lies inside its own cell; in `captions.rs`, the setup runs on a `VirtualClock` so Recently
    Updated's order is stable from run to run (no re-bless: the baseline is unchanged). The
    product is not touched. `control_headless.rs` is also M10.40's file: the two never run
    together (`tasks claim` refuses), and whichever lands second rebases on the other.
  - done when: `control_headless::an_agent_stars_a_photo_by_click_and_key_and_takes_a_screenshot`
    passes; the captions fingerprint test passes 5 runs out of 5; `cargo xtask ci` green.
- ✅ **M10.43 `acceptance`: the finished milestones' checks after the Recently Updated section shows**
  - after: M10.29, M10.42
  - files: `tests/acceptance/m1.rs`, `tests/acceptance/m2.rs`, `tests/acceptance/m6.rs`, `tests/acceptance/m6_live.rs`, `tests/acceptance/m8.rs`, `tests/acceptance/m9_app.rs`, `tests/acceptance/fingerprints/m6-*.json`, `tests/acceptance/fingerprints/m8-*.json`, `tests/acceptance/fingerprints/m9-*.json`
  - scope: found by the full gate (GitHub run 38832985226, the same on all three systems): the
    acceptance checks of finished milestones still expect the window before M10.29 (and any other
    M10 change they trip). Accept M1 `star_a_photo_end_to_end` (`m1.rs:84`) and accept M2
    `agent_eyes` (`m2.rs:246`) expect the star badge ids `thumb:N/star`; the window now draws
    `thumb:N/in/starred/star` and `thumb:N/in/recent/star`. Whole-window fingerprints moved: accept
    M6 `m6-1400x900` (51.88 away, `m6_live.rs:334`) and `m8-1400x900-header` (107.94, checked in
    `m6_live.rs`), accept M8 `m8-1400x900-export` (8.33, `m8.rs:411`), accept M9
    `m9-1400x900-albums` (13.73), `-search` (15.73) and `-drag` (3.52) (`m9_app.rs:201`); their
    900 × 600 siblings are checked after them and may move too. These checks are floors: each is
    updated to the specified new behaviour (`organizing.md` §4.1, Recently Updated shows; M10.29),
    never weakened. Badges: find each photo by id, never by position, and check both copies'
    badges (Starred Photos and Recently Updated) and that each lies inside its own cell, as M10.42
    does. Fingerprints: re-bless a baseline (`DALICA_BLESS`) only after comparing the new image
    with the old one and explaining every changed region by an M10 task's specified behaviour;
    any change no task specifies is a bug: report it on this issue (or `tasks block`) instead of
    blessing it. The new baselines are approved in bulk at M10.R.
  - done when: `cargo xtask accept M1`, `M2`, `M6`, `M8` and `M9` pass locally, in release and in
    debug as the acceptance runner runs them; the old and new images of every re-blessed baseline
    are on the PR; the commit body says why each baseline changed, region by region; `cargo xtask
    ci` green.
- ✅ **M10.44 `mcp` + `ui`: the full gate's macOS and Windows failures in the tools and the Options dialog**
  - after: M10.10, M10.14
  - files: `crates/mcp/src/tools/output.rs`, `crates/mcp/tests/output_tools.rs`, `crates/ui/src/dialogs/options/general.rs`, `crates/ui/tests/options_dialog.rs`, `crates/ui/tests/view_state.rs`, `crates/ui/tests/fingerprints/dialog-options-general-*.json`
  - scope: found by the full gate (GitHub run 38832985226, `ci --full --platform`), which the
    task gates never run on these systems. (a) macOS and Windows, M10.14: mcp
    `output_tools::render_photo_renders_and_changes_nothing` (`output_tools.rs:53`): the answer's
    `path` is the canonical form, not the path asked for (macOS `/private/var/…` for `/var/…`;
    Windows `\\?\C:\Users\runneradmin\…` for `C:\Users\RUNNER~1\…`). A verbatim `\\?\` path is a
    product wart an agent then shows or reuses: the answer names the written file in a plain form
    (the path the caller gave, after the root check, or the canonical one without the verbatim
    prefix: the worker decides and says why), and the test still proves the answer names the very
    file written, on every system. The roots check itself (`crates/mcp/src/roots.rs`) is M10.16's
    file and stays out: the fix lives in `output.rs`. (b) macOS, M10.10 and M10.2: ui
    `options_dialog::the_general_tab_matches_its_fingerprint_and_fits_the_small_window`
    (`dialog-options-general-1400x900` 27.20 away; the General tab lists the options of the
    platform it runs on, `general.rs` `here`) and
    `view_state::the_options_keys_open_the_dialog_and_its_menu_items_are_live`
    (`view_state.rs:645`: the row's shortcut is `Cmd+,` on macOS, the test expects `Ctrl+,`). Both
    tests assume one platform: make them portable without weakening (pin the platform the
    fingerprint is taken for, or expect the platform's own key label: `Cmd` on macOS, `Ctrl`
    elsewhere); if the macOS layout itself is wrong (a row that should not show there), that is a
    product bug fixed in `general.rs`. Each item is either a product bug (fix it; the failing test
    is its regression test) or a test that assumed one platform (made portable, still checking the
    same behaviour); the commit body says which, per item.
  - done when: each named test passes on the systems it failed on, proven by dispatching CI on the
    task branch (`gh workflow run ci.yml --ref <branch>`; macOS and Windows cannot run locally) and
    citing the green jobs in the PR; `cargo xtask ci` green.
- ✅ **M10.45 `fs` + `engine`: the full gate's Windows failures in a killed Save and the watcher**
  - after: M10.34
  - files: `crates/fs/src/originals.rs`, `crates/fs/tests/crash.rs`, `crates/engine/src/watching.rs`, `crates/engine/tests/watching.rs`
  - scope: found by the full gate on Windows (GitHub run 38832985226). (a) fs
    `crash::a_save_killed_at_any_moment_keeps_the_original_whole_under_exactly_one_name`
    (`crash.rs:246`, M10.34): "seed 1792093458828416969, kill 24 after 1 Saves: photo Some(262144),
    backup Some(262144)": both names hold a whole file but the photo is not the saved one, so a
    kill on Windows can leave the original under two names (the backup, Undo Save's restore, or
    their moves: `originals.rs`). This is a data rule (`data-model.md` §6, one name for the
    original's bytes): find the window where both exist on Windows and close it in `originals.rs`,
    the failing case (its seed) the regression test; if the fix needs a file outside these (`fs`'s
    `atomic.rs` or `transfer`), ask on the issue first. (b) engine
    `watching::external_changes_reach_the_library_through_the_watcher` (`watching.rs:49`, a helper
    thread's `std::fs::rename` gets "Access is denied", OS error 5; `common.rs:113` only reports
    the thread's panic): on Windows a folder cannot be renamed while a handle is open inside it.
    If the app holds that handle (the watcher, a scan, a thumbnail read keeping a file or folder
    open longer than it needs), it is a product bug: an external rename must not fail because
    Dalica is looking at the folder; fix it in `watching.rs` (or ask on the issue for the file
    that holds it). If only the test's own timing collides with a background read the app is
    allowed to make, the test waits for the scan to settle before renaming (no blind retry loops,
    no lower bar). The commit body says which, per item.
  - done when: both named tests pass on Windows, proven by dispatching CI on the task branch (`gh
    workflow run ci.yml --ref <branch>`) and citing the green Windows job in the PR, the crash test
    at full strength (`DALICA_FULL_TESTS=1`) as the review runs it; `cargo xtask ci` green.
- ✅ **M10.46 `ui`: the Properties pane's settle flake on macOS**
  - after: M10.25
  - files: `crates/ui/tests/properties_pane.rs`, `crates/engine/src/metadata_cache.rs`
  - scope: found by the full gate on macOS (GitHub run 38835952339, PR #608, `macos-latest`; it
    failed the same way in earlier M9 runs and passed on re-run):
    `properties_pane::settle_waits_until_the_pane_has_the_files_rows` panics at
    `properties_pane.rs:351` with "48 MB are still being read". That line is the test's premise,
    not the settle: right after `photo.metadata` answers `pending`, it expects
    `Session::photo_metadata_pending()` to be above zero because `big.jpg` carries 48 MB after its
    end, and on a fast runner (or a reader that stops at the image's end) the read is already over
    when it looks. The product side is done: `Driver::busy` (`headless.rs`) already lists
    `photo metadata: N` from the engine's pending reads (as M10.33 did for a pending search), so
    `ui.settle` waits for a read in flight; `headless.rs` stays out (it is M10.41's file). Make the
    in-flight state deterministic instead of hoping a big file is slow: hold the read, for
    example a test-only gate on the metadata readers in `metadata_cache.rs` (as the headless
    tests' `Gate` picker holds a dialog open until the test answers), so the test sees the read
    pending and `busy` naming it, then a settle started while it is held does not return settled
    (bounded by its frame budget), and once the gate opens the settle ends with the file's full
    rows and nothing pending. No sleep, no retry loop, no wall-clock bound, and nothing the test
    checks today is dropped (the pending answer, the busy line, the settled rows, the plain
    photo's five rows). If the cause turns out to be the product (a settle that returns while a
    read is pending), fix it in the product and keep the failing case as the regression test; the
    commit body says which it was.
  - done when: the test passes 20 of 20 runs under load locally (pinned to one core with
    `taskset -c 0` beside parallel `cargo test` runs), and on macOS in a CI run dispatched on the
    task branch (`gh workflow run ci.yml --ref <branch>`), the green macOS job cited in the PR;
    `cargo xtask ci` green.
- ✅ **M10.47 `ui`: one menu open at a time, and the window size under a fixed UI scale**
  - after: M10.25, M10.41
  - files: `crates/ui/src/menus.rs`, `crates/ui/src/control/host.rs`, `crates/ui/tests/menu_one_open.rs`, `crates/ui/tests/ui_scale.rs`
  - scope: found by M10.41 (PR #614) and the full gate's macOS and Windows jobs (GitHub run
    38835952339). (a) A product bug: with a menu bar menu open (Picture, which holds View and
    Edit), a right-click on a photo or a tray item opens its context menu in the same frame the
    bar's popup is still drawn, so both register `menuitem:view.edit` in that frame and
    `monkey::a_seeded_monkey_breaks_nothing` fails on macOS and Windows ("`menuitem:view.edit` is
    registered twice"; seed 910831521846; last actions: click `menu:Picture`, Ctrl+V, right-click
    a tray item). It reproduces on Linux. Fix it in `menus.rs`: one menu is open at a time (for
    example, the bar's menus close before a context menu opens, in the same frame). The
    regression test lives in a new `crates/ui/tests/menu_one_open.rs` (a new `mod` of
    `crates/ui/tests/all.rs`): open the bar's Picture menu, right-click a photo (and a tray
    item), and check exactly one popup is drawn and no automation id is registered twice. The
    monkey stays as it is (no new exception). (b) `control/host.rs` `window()` reports the
    window's size in points as its pixels divided by the native pixels per point; since M10.41 the
    headless layout follows a fixed UI scale, so under one that size is wrong (1400 × 900 pixels
    at 1.0 and UI scale 150 % reports 1400 × 900 points, but egui lays out 933 × 600). Report the
    size in points egui actually lays out (and the pixels per point it uses), so `ui.inspect` and
    an agent's coordinates agree with the screen; the test goes beside M10.41's in
    `ui_scale.rs`, through the control host's `window()`.
  - done when: tests, each failing before the fix: in `menu_one_open.rs`, a right-click with the
    Picture menu open leaves exactly one popup and no id registered twice; in `ui_scale.rs`, a
    1400 × 900 pixel driver at 1.0 pixels per point with UI scale 150 % reports a window of
    933 × 600 points; `monkey::a_seeded_monkey_breaks_nothing` passes on Linux with seed
    910831521846 and `DALICA_FULL_TESTS=1`; a CI run dispatched on the task branch (`gh workflow
    run ci.yml --ref <branch>`) shows the monkey green on macOS and Windows, the jobs cited in the
    PR; `cargo xtask ci` green.
- ✅ **M10.48 `dalica` + `engine`: two macOS failures the full gate found**
  - after: M10.40
  - files: `apps/dalica/tests/control_headless.rs`, `apps/dalica/tests/common.rs`, `apps/dalica/src/control/headless.rs`, `apps/dalica/src/control/mod.rs`, `crates/engine/tests/save_copy.rs`, `crates/engine/src/metadata.rs`
  - scope: found by the full gate on macOS (GitHub run 38838856986, job `ci (macos-latest)`
    116774038396). (a) `control_headless::quitting_keeps_the_caption_being_typed` (M10.40) panics
    at `control_headless.rs:67` with "dalica exited early with exit status: 2" while it waits for
    the token file, so the app quit before its control channel was up. Exit 2 comes from
    `main.rs` only for a bad argument, a control channel that cannot start (`control::listen`: a
    taken port, or a token file `--control-token-file` cannot make) or no library folder. First
    make the test name the cause: an early exit reports the app's stderr in the panic. Then: if
    the app exits 2 for a reason it should not on macOS (the token file, the demo folder, the
    port), it is a product bug: fix it in `control/mod.rs` or `control/headless.rs` (ask on the
    issue for `main.rs`), the failing test the regression test. If it is the test's own race (for
    example `common.rs`'s `free_port` handing the same port to two tests running in parallel),
    make the port deterministic (bind and keep it, or retry the launch only on "the port is in
    use", bounded) without dropping a check. (b)
    `save_copy::two_copies_are_001_and_002_with_the_edits_and_the_photo_untouched` panics at
    `save_copy.rs:140` (`photo(id) == photo_before`): the photo after the copies holds
    `date: Some(2025-05-01T10:00:00)` where the record taken before them held `date: None`, size
    and time unchanged, so the file's date reached the photo's record after `wait_idle` and
    `flush_metadata` had returned: the write-back of `photo.dates.set` (or the re-read that
    follows it) lands later than the test assumes. If a flush returns before its results are
    applied to the library (`metadata.rs`), that is a product bug: `flush_metadata` returns only
    once the written files' results are in the model, the failing test the regression test. If
    only the test's order is wrong, it takes `photo_before` once that result is in, waiting on a
    bounded condition. No sleep, no blind retry, no check weakened (the photo stays untouched by
    the copies). The commit body says which it was, per item.
  - done when: each test passes 20 of 20 runs under load locally (pinned to one core with
    `taskset -c 0` beside parallel `cargo test` runs); a CI run dispatched on the task branch
    (`gh workflow run ci.yml --ref <branch>`) shows both green on macOS, the job cited in the PR;
    `cargo xtask ci` green.
- ✅ **M10.49 `engine` + `mcp` + `ui`: the path policy on Windows**
  - after: M10.16
  - files: `crates/engine/src/automation.rs`, `crates/engine/tests/automation_roots.rs`, `crates/engine/tests/read_only.rs`, `crates/mcp/src/roots.rs`, `crates/mcp/tests/roots.rs`, `crates/ui/src/control/paths.rs`
  - scope: found by the full gate on Windows (GitHub run 38838856986, `ci (windows-latest)`):
    M10.16's path policy refuses paths it should allow there. engine `automation_roots::`
    `reads_stay_inside_a_read_root_or_a_watched_folder` (`automation_roots.rs:49`),
    `new_files_go_into_a_write_root_never_into_originals_a_watched_folder_or_over_a_file` (`:77`)
    and ui `control::paths::tests::new_files_go_into_a_write_root_or_a_watched_folder_and_never_replace_one`
    (`paths.rs:81`) are refused by `lexical` with "uses a verbatim or device prefix" for
    `\\?\C:\Users\runneradmin\AppData\Local\Temp\…` paths; engine
    `client_roots_are_read_roots_only_and_a_new_list_replaces_the_old` (`:119`) and mcp
    `roots::a_client_root_is_a_read_root_never_a_write_root_and_a_refresh_drops_a_removed_one`
    (`roots.rs:94`) answer `path_denied` where `ok` is due; mcp
    `roots::a_read_only_launch_refuses_an_export_with_read_only` (`:160`) answers `path_denied`
    before the `read_only` refusal it should give; mcp `roots::only_local_file_uris_become_roots`
    (`:136`) parses no root (`[]`) where it expects the temp folder (built from a `\\?\C:\…` path).
    The pattern is path forms that do not match on Windows: `canonicalize` gives the verbatim
    `\\?\C:\…` form, while roots, `file:///C:/…` URIs, the temporary directory and an agent's paths
    use the plain `C:\…` form (the runner's Temp may also be an 8.3 short name, `RUNNER~1`, and
    drive letters differ in case), so a prefix check on mixed forms fails. Fix it in
    `automation.rs` (the control channel's `paths.rs` and MCP's `roots.rs` call it, `file_uri_path`
    in `roots.rs`): compare one normal form on both sides, for example resolve both the root and
    the path through `canonicalize` and then compare (handling `\\?\`, `\\?\UNC\`, drive-letter
    case and short names), or strip the verbatim prefix after resolving both, crate-free; a
    `file:///C:/…` URI to a local path parses to its drive path. The lexical refusal of a
    verbatim or device prefix in a caller's own path stays (`security.md` §6.6: it skips Windows'
    checks): a test that fed it a canonical path is the test's Unix assumption, made portable by
    building the plain form, and the answer's paths stay plain (as M10.44 did for a render).
    Every refusal stays: `..`, a link out of a root, `.dalica-originals/`, a watched folder for a
    screenshot or render, an existing file, deny by default; refusing comparisons still ignore case
    on Windows and macOS. engine `read_only::starring_and_captioning_change_no_byte_on_disk_and_an_export_is_refused`
    fails apart from the policy (`read_only.rs:31`, reported at `common.rs:113`): the test's
    snapshot reads every file under the library folder, and on Windows the file the session holds
    locked (`fs`'s library lock) cannot be read ("another process has locked a portion of the
    file", OS error 33). Make the snapshot compare that one file by name, size and time instead of
    its bytes (or, if a read-only session should not hold a lock file there, fix that and say
    why), still proving no other byte on disk changes. No check is weakened and no test drops an
    assertion; tests change only where they assumed Unix path forms or Unix locks. The commit
    body says, per failure, whether it was a product bug or a test that assumed one platform.
  - done when: every named test passes on `windows-latest` in a CI run dispatched on the task
    branch (`gh workflow run ci.yml --ref <branch>`), the job cited in the PR, and still passes on
    Linux and macOS in that run; a new unit test in `automation.rs` (`#[cfg(windows)]`, run in that
    job) shows a root and a path given in the verbatim, plain and differently-cased drive forms
    compare equal for an allow while `..`, `.dalica-originals` and a watched folder are still
    refused in each form; `cargo xtask ci` green.
- ✅ **M10.50 `mcp` + `engine` + `store`: W04's caption missing from the sidecar on macOS and Windows**
  - after: M10.19
  - files: `crates/mcp/tests/workflows/w04_rebuild_loses_nothing.rs`, `crates/mcp/tests/workflows/harness.rs`, `crates/engine/src/session.rs`, `crates/engine/src/offline.rs`, `crates/store/src/rebuild.rs`, `crates/engine/tests/pending_writes.rs`
  - scope: found by the full gate (GitHub run 38841617828; it passed in run 38838856986 and on
    Linux in both, so it is new since then or a race). `w04_rebuild_loses_nothing` panics at
    `w04_rebuild_loses_nothing.rs:126` on `ci (macos-latest)` (job 116782787471) in the test step,
    `left: Null` where "The Forum at dusk" is due, and the panic message (the whole entry) is
    `null` too: the captioned photo has no entry at all in the folder's `.dalica.json`. That line
    is **before** the rebuild: after `set_metadata {caption}` on two photos and `settled` (no
    `pendingWrites`, no scan running), the sidecar read back lacks the photo. On
    `ci (windows-latest)` (job 116782787073) the test step passed, but the run of the workflows
    inside `cargo xtask stats --check` failed it, dropping the pass count to 8 under the floor of 9;
    on macOS the stats run passed it. So it is intermittent on both. Data comes first (AGENTS.md
    §3), so first decide which of these it is, with evidence (a log of the sidecar's writes and of
    `pendingWrites` around the failing read, reproduced under load): (a) `pendingWrites` reports
    none while a sidecar write is still queued, debounced or in flight (the M10.29 `changed` stamp,
    the M10.40 caption draft or shutdown path, a write-back's re-read that writes again): a product
    bug, since `library.inspect`, `quit` and a rebuild all trust it; fix it so a write counts until
    its file is committed, and make a rebuild (and a quit) flush pending sidecar writes before it
    starts, else a rebuild can read a sidecar a write has not reached and lose the change
    (`data-model.md` on rebuild); (b) a write lands but a later write of the same sidecar from an
    older model drops the entry (a lost update): a product bug and data loss, fixed so the writer
    merges from the latest state; (c) the harness's `sidecar` read fails while the file is being
    replaced and answers `Null` (on Windows a read during the M10.45 rename can see a sharing
    error, and macOS may differ too): a test bug, fixed by a bounded read that retries only on that
    error, or by waiting for the entry on a bounded condition. Data loss in any form is fixed in
    the product, never in the test; W04 stays the regression test, and a product fix also gets a
    lower test in `crates/engine/tests/pending_writes.rs` (a new `mod` in `tests/all.rs`: set a
    caption, read `pendingWrites` as none, then the sidecar holds it; and a rebuild started with a
    write pending keeps it) that fails before the fix. No sleep, no blind retry of the whole test,
    no check weakened or dropped. The commit body names the cause and the evidence.
  - done when: the cause is named (product or test) with evidence in the PR; W04 passes 20 of 20
    runs under load locally (pinned to one core with `taskset -c 0` beside parallel `cargo test`
    runs); a CI run dispatched on the task branch (`gh workflow run ci.yml --ref <branch>`) shows
    W04 green on macOS and Windows, both in the test step and in `cargo xtask stats --check`
    holding at 9, the jobs cited in the PR; for a product bug, the lower test fails before the fix
    and passes after; `cargo xtask ci` green.
- ✅ **M10.26 `docs`: the P0 parity audit**
  - after: M10.8, M10.11, M10.12, M10.16, M10.19, M10.20, M10.21, M10.22, M10.23, M10.25, M10.39, M10.40, M10.41, M10.42, M10.43, M10.44, M10.45, M10.46, M10.47, M10.48, M10.49, M10.50
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
- ✅ **M10.51 `ui`: the geotag badge, the edit view's trash button, and Ctrl+3 and Ctrl+9 in both views**
  - after: M10.25, M10.39, M10.40, M10.41
  - files: `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/src/panels/edit/caption_bar.rs`, `crates/ui/src/commands/edit_view.rs`, `crates/ui/tests/cells.rs`, `crates/ui/tests/delete_dialogs.rs`, `crates/ui/tests/edit_keys.rs`, `docs/menus-and-shortcuts.md`
  - scope: four P0 rows the audit (M10.26) left open. LIB-LT-BADGES: `badges_of` (`cell.rs`)
    sets star, edited and held but never the geotag badge, although the library knows which
    photos are geotagged (the photo's facts, `geotagged`, which the Geotagged filter reads); set
    it from there, read once per revision as the other badges are (no per-frame library work).
    EDIT-VIEW: the caption bar's trash button (`button:edit.trash`) is still drawn disabled "until
    M9"; delete exists since M9: enable it and make it ask the same question as Ctrl+Delete
    (`photo.delete` through the delete question, `dialogs/delete.rs`, `organizing.md` §9) for the
    photo shown; Cancel changes nothing; after Move to Trash the edit view goes where it goes after
    Ctrl+Delete today (if that is unspecified: the next photo in the section, else the previous,
    else back to the library; the choice in the commit body). The slideshow toggle stays disabled
    (VIEW-SLIDESHOW, M11). KEY-GLOBAL-CTRL-3 and KEY-GLOBAL-CTRL-9: `menus-and-shortcuts.md` §2.1
    lists both as Global, but `view.edit` binds Ctrl+3 in the Library only and `edit.panelToggle`
    binds Ctrl+9 in the edit view only. Bind each in the other view as well (a Global key bound in
    both views reads ✅, as M10.39 did for Ctrl+Shift+S): Ctrl+3 in the edit view keeps the photo
    shown (it must not reset the zoom or close an open tool, unlike opening a photo; a no-op with
    `{"view": "edit", "photo": id}` is enough), and Ctrl+9 in the library sets whether the edit
    panel shows the next time the edit view opens (`edit.panelVisible`, with the View menu's check
    following it), with no conflict with another default in either context. If either turns out to
    make no sense in that view, move it out of §2.1 into its own view's table with the reason and
    regenerate the row (it then reads ✅ under its new context); the commit body says which.
  - done when: tests, each failing before the fix: in `cells.rs`, a geotagged photo's cell carries
    the geotag badge and a photo without a location does not; in `delete_dialogs.rs`, a click on
    `button:edit.trash` asks the delete question for the photo shown, Cancel leaves it, Move to
    Trash moves it to the trash and the edit view shows the photo the scope names; in
    `edit_keys.rs`, Ctrl+3 in the edit view keeps the photo and its zoom, Ctrl+9 in the library
    toggles `edit.panelVisible` and the edit view then opens with the panel so; `cargo xtask parity
    --write` turns LIB-LT-BADGES, EDIT-VIEW, KEY-GLOBAL-CTRL-3 and KEY-GLOBAL-CTRL-9 ✅ (the rows'
    notes name the tests); screenshots at 1400 × 900 of a geotagged cell and of the edit view's
    enabled trash button, before and after, on the PR; `cargo xtask ci` green.
- ✅ **M10.52 `engine` + `ui`: a damaged sidecar's banner and Reset folder data, and the search's empty state**
  - after: M10.25, M10.38, M10.40, M10.50
  - files: `crates/engine/src/cmd/folder_data.rs`, `crates/engine/src/cmd/mod.rs`, `crates/engine/src/session.rs`, `crates/engine/tests/folder_data.rs`, `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/light_table/folder_banner.rs`, `crates/ui/src/dialogs/confirm.rs`, `crates/ui/tests/damaged_sidecar.rs`, `crates/ui/tests/search_ui.rs`, `docs/data-model.md`, `docs/ui-layout.md`
  - scope: two P0 rows the audit (M10.26) left open. DATA-SIDECAR (`data-model.md` §3): an
    unreadable sidecar is already kept aside as `.dalica.json.corrupt-<time>` and its folder made
    read-only by the store's writer, but nothing tells the user and nothing lifts it (only
    `SidecarWriter::clear_read_only` exists). This touches data (AGENTS.md §3), so: (a) engine: the
    session lists the damaged folders (in memory, cheap to read each frame: a revision or a set, no
    disk access on the UI thread), and they survive a restart (decide how, for example the index
    remembering the read-only folders, so a restart does not quietly start writing into the folder
    before the user chose; the choice in the commit body); a new command `folder.resetData {folder}`
    (new `cmd/folder_data.rs`, `effect` Files, history none) that, only for a damaged folder: re-reads
    the folder's sidecar first, and if a readable one is there now (the user restored it) takes its
    values and lifts read-only without writing; otherwise lifts read-only and lets the writer write
    a new sidecar from the model (the model's values for the folder survive; the damaged copy stays
    aside untouched, never deleted, renamed or overwritten, and a second damaged copy gets its own
    `.corrupt-<time>` name, never replacing the first); refuses a folder that is not damaged, an
    unknown folder and hostile params with an `Err`; the reply names the kept-aside file. Restoring
    the file by hand (the watcher's or a rescan's read of a now-readable sidecar) lifts the state too.
    (b) ui: a `caution` banner (new `panels/light_table/folder_banner.rs`, drawn by `light_table.rs`
    at the top of the damaged folder's section, tokens and `ui-kit` widgets only) reading that the
    folder's data file was damaged and kept aside, with [Reset folder data]
    (`button:folder.resetData`), which asks first (a new row in `confirm.rs`'s `ASKS`: what it does,
    that the damaged copy stays beside the photos, [Reset] [Cancel]); Cancel changes nothing.
    LIB-EMPTY (`ui-layout.md` §5.3): the search empty state reads "No matching pictures" and points
    to the banner's Show All; make it "No photos found" with its own [Show All]
    (`button:empty.showAll`, running what the banner's Show All runs: leaving search mode). Update
    `data-model.md` §3 and `ui-layout.md` §5.3 with what was built.
  - done when: tests, each failing before the fix: in `dalica-engine` (`folder_data.rs`, a new
    `mod` in `tests/all.rs`): a folder with an unparseable sidecar is listed damaged, a caption set
    there writes nothing, and the list survives reopening the session; `folder.resetData` keeps the
    `.corrupt-<time>` file byte for byte, writes a new sidecar with the model's values (the caption
    set while damaged included) and lifts the state; a sidecar restored by hand before the reset is
    read, not overwritten; a second damage keeps both copies; a folder that is not damaged is
    refused; in `dalica-ui` (`damaged_sidecar.rs`, a new `mod`): the banner shows on the damaged
    folder only, Reset asks, Cancel leaves everything, Reset runs `folder.resetData` and the banner
    goes; in `search_ui.rs`: a search with no results shows "No photos found" and
    `button:empty.showAll` leaves search mode; `cargo xtask parity --write` turns DATA-SIDECAR and
    LIB-EMPTY ✅; screenshots at 1400 × 900 of the banner, the question and the empty state, before
    and after, on the PR; `cargo xtask ci` green.
- ✅ **M10.53 `ui`: an export's progress in the status line**
  - after: M10.25, M10.31
  - files: `crates/ui/src/panels/status.rs`, `crates/ui/src/status_text.rs`, `crates/ui/tests/status_line.rs`, `docs/creating.md`
  - scope: the P0 row OUT-EXPORT the audit (M10.26) left open: `creating.md` §2 puts an export's
    progress in the status line as well as in the progress window, and today it shows only in the
    window and the activity popover. Build it: while an export job runs, the status line carries
    "Exporting 12 of 48" from the job's in-memory progress (as "Scanning: 1,204 pictures found"
    does through `status_text::with_activity`: in place of "No selection", else the last field; in
    the edit view too), read each frame from the session's progress without per-frame work
    proportional to the library, gone when the job ends (its toast follows as today). With a scan
    and an export both running, both fields show and fitting drops the scan's first (the order in
    the commit body). If building it shows the spec is wrong, change `creating.md` §2 with the
    reason instead and say so on the PR (building it is preferred). Update `creating.md`'s Built
    note.
  - done when: tests, each failing before the fix, in `status_line.rs`: with an export job held
    part-way (a test gate, no sleep), `ui.state`'s `status` reads "Exporting n of m" with nothing
    selected and ends with it with a photo selected, in the library and the edit view; it is gone
    once the job ends; a scan and an export together fit at 900 wide with the scan field dropped
    first; `cargo xtask parity --write` turns OUT-EXPORT ✅; screenshots at 1400 × 900 of the
    status line during an export, before and after, on the PR; `cargo xtask ci` green.
- ✅ **M10.54 `mcp` + `engine` + `ui`: connect mode fills the Photo Tray, and metadata write-back runs while the app is idle**
  - after: M10.17, M10.36, M10.40
  - files: `crates/mcp/src/connect.rs`, `crates/mcp/src/tray.rs`, `crates/mcp/tests/connect.rs`, `crates/ui/src/control/engine.rs`, `crates/engine/src/scanning.rs`, `crates/engine/src/metadata.rs`, `crates/engine/tests/busy_writes.rs`, `crates/ui/src/app.rs`, `crates/ui/tests/idle_write_back.rs`
  - scope: two faults M10.27 (accept M10) found in the live app. (1) Connect mode ignores the
    Photo Tray: `cmd_photo_rotate_cw {}` against a running `dalica --control` answers `bad_params:
    missing field ids`, while `mcp.md` §4.1 and §4.4 say a photo command without `ids` acts on the
    Photo Tray, and headless mode already fills them (`Headless::prepare`). Fill `ids` from the
    app's tray (the `{selection, held, tray}` that connect mode's `tray.state` already reads from
    `library.inspect`) before the command reaches the app's `engine.execute`, by the same rule
    headless uses (only when the params doc names `ids` as required and none were given; an empty
    tray answers the headless error). Preferred: in `crates/mcp/src/connect.rs`, the rule moved to
    one helper in `crates/mcp/src/tray.rs` that both modes call; the alternative, the app's
    `crates/ui/src/control/engine.rs`, only if the tray cannot be read from `mcp`. Pick one and keep
    one implementation; the choice in the commit body. (2) Caption write-back stalls in an idle
    app: in `dalica --headless` a caption never reaches the photo file in 120 s until a frame runs,
    then lands in 0.8 s. `Session::busy` (`scanning.rs`), and so `App::idle` (`app.rs`), ignore
    queued and running metadata writes, so the headless loop never polls them; an idle window holds
    the write until the next input the same way. Make `busy` count queued and running metadata
    writes (and pending sidecar writes, if they need polling to land), read from in-memory state
    without disk access; `App::idle` follows. Do not slow an idle app: once the writes land, busy
    turns false again (`DALICA_PROFILE=1` idle numbers before and after in the PR body).
  - done when: tests, each failing before the fix: in `dalica-mcp` (`connect.rs`): against a
    connected app with two photos in the tray, a photo command with no `ids` acts on both and an
    empty tray answers the same error as headless; in `dalica-engine` (`busy_writes.rs`, a new
    `mod` in `tests/all.rs`): `busy()` is true while a metadata write is queued and while it runs,
    and false once it has landed; in `dalica-ui` (`idle_write_back.rs`, a new `mod`): `App::idle`
    is false while a caption's write is queued, and an app ticked only as the headless loop ticks
    an idle app (no input, no settle) lands the caption in the photo file; M10.27's connect-mode
    check (`tests/acceptance/m10_mcp.rs` on its branch) passes against the fix; `cargo xtask ci`
    green.
- ✅ **M10.55 `engine` + `ui`: the "▸" in option hints**
  - after: M10.11
  - files: `crates/engine/src/options.rs`, `crates/ui/tests/ui_glyphs.rs`, `crates/ui/tests/fingerprints/dialog-options-file-types-1400x900.json`
  - scope: the UI font (Inter) has no U+25B8 "▸", so the File Types tab's threshold hint (the
    options catalogue's "View ▸ Small Pictures" in `options.rs`) draws a missing-glyph "?"; M10.3
    met the same with the key caps. Write menu paths in catalogue text with "›" (U+203A), which the
    UI font has (as the release notes' fallback in `dialogs/release_notes.rs` already does); adding
    the glyph through `ui-kit`'s font fallback is the alternative, not preferred. A grep found no
    other "▸" in a string the UI draws (the others are doc comments, command params docs and MCP
    tool text, which reach agents, not the UI font); recheck and fix any that does, in a file no
    open task owns. A test that no catalogue label, hint or choice (and any other UI string table
    the grep finds) holds a glyph the UI fonts lack, checked through egui's font coverage as
    M10.3's key-cap test does (`crates/ui-kit/tests/options_widgets.rs`). Re-bless
    `dialog-options-file-types-1400x900.json` after looking at the diff (the why in the commit
    body). M10.27's own baseline `m10-1400x900-options-file-types.json` bakes the "?" in: M10.27
    (owner of `tests/acceptance/fingerprints/m10-*.json`) re-blesses it after this merges.
  - done when: no "?" glyph in the File Types tab; the coverage test (`ui_glyphs.rs`, a new `mod`
    in `tests/all.rs`) fails before the fix and passes after; the re-blessed fingerprint and its
    before/after images on the PR; `cargo xtask ci` green.
- ✅ **M10.27 `accept M10`**
  - after: M10.12, M10.16, M10.20, M10.26, M10.51, M10.52, M10.53, M10.54, M10.55
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
- ✅ **M10.56 `image` + `ui`: HEIC without a decoder**
  - after: M10.24, M10.27
  - files: `crates/image/src/formats/heif.rs`, `crates/image/src/formats/heif_items.rs`, `crates/image/src/formats/mod.rs`, `crates/image/tests/heif.rs`, `crates/image/README.md`, `crates/ui/src/panels/edit/canvas.rs`, `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/heic_hint.rs`, `docs/formats.md`, `docs/parity.md`
  - scope: the maintainer's answer at M10.R (2026-10-16): release builds ship without an HEVC
    decoder (no patent licence), the `heif` feature stays an off-by-default build feature, and a
    HEIC is organized like any other file but shows the unsupported tile. After M10.27 because it
    owns `docs/parity.md` and `docs/formats.md` until it merges. (1) The hint. Today the tile's
    reason is `heif::REASON` ("HEIC support isn't included in this build", `formats/heif.rs`), shown
    in the cell's tooltip ("Cannot show this photo: …", `light_table/cell.rs` `tooltip_shown`) and
    under the edit view's "This photo cannot be shown." (`edit/canvas.rs` `failed`). Keep that text
    (the Options File Types row and Save's disabled reason reuse it) and add a short hint under it,
    only without the feature: "On an iPhone, Settings › Camera › Formats › Most Compatible saves
    JPEG. Or convert these photos to JPEG." (final wording per `formats.md` and the UI's tone; "›",
    never "▸", which the UI font lacks, M10.55). One `heif::HINT` constant and a
    `formats::hint(Format) -> Option<&'static str>` beside `formats::reason` (`None` for every
    other format and with the feature); the cell and the edit view pick it by the failure's format
    (preferred) or by matching the reason against `heif::REASON`, the choice in the commit body.
    The Properties pane shows no failure note today; it gains the size and date from (2) and
    nothing else. (2) The container. Without the feature `heif::header` answers
    `Unsupported(Heif)`, so the scan lists a HEIC with no size and no date (`scan/src/walk.rs`
    keeps size and date only from an `Ok` header), and `dalica-meta`'s container walk does not know
    HEIF at all: nothing is read today. Build it in pure Rust in a new `formats/heif_items.rs`, on
    `formats/bmff.rs`'s bounded walk (no HEVC): `meta` → `pitm` (the primary item), `iinf`/`infe`
    (the `Exif` item, and the primary's type), `iloc` (its extents; construction method 0 only,
    others skipped), `iprp`/`ipco`/`ipma` (the primary's `ispe` size, `irot`/`imir`); the Exif
    item's payload skips its 4-byte TIFF-header offset and goes through `Header::with_exif` as the
    feature's path does. Without the feature `heif::header` then answers an `Ok` header (size,
    orientation, EXIF), while `decode` and `embedded_thumbnail` still answer `Unsupported(Heif)`, so
    the cell gets the tile and the scan gets the size and date it sorts and searches by. A
    container the reader cannot use (no `pitm` or `ispe`, a damaged box, an item past the file's
    end) still answers `Unsupported(Heif)` with the tile text, never "Damaged file": M5's
    acceptance (`m5_formats.rs`, a bare `ftyp` and empty `meta`) and `tests/decode.rs` stay green.
    Every size and offset is hostile (`never-crash.md`): checked arithmetic, extents capped by
    `Limits`, the EXIF block capped as other formats cap it. With the feature nothing changes
    (heic-rs keeps the header). The XMP item (captions written by other apps) stays unread:
    `dalica-meta` learning HEIF is later work, not this task. (3) `formats.md` §3 and §3.1: release
    builds carry no HEIC decoder, because HEVC decoding needs a patent licence in some countries
    (the maintainer's decision at M10.R, with M10.24's measurements); the feature stays for people
    who build Dalica themselves (`cargo build -p dalica --features heif`); without it a HEIC is
    listed, sorted and searched by its date and size, read from the container, and shows the tile
    with the hint; §1's table and the tile-text lists follow. (4) `parity.md`: `DATA-FMT-HEIF`
    becomes OOS 🚫 (the parity tool pairs 🚫 with OOS) with the reason (release builds: no HEVC
    licence; the feature for self-builds; listed with date, size and EXIF from the container, the
    tile with the hint); `DATA-FMT-AVIF` already exists (P2 🟡, recognised, never decoded): its
    note gains that decoding AVIF (royalty-free AV1 in the same container) is a later possibility
    with no milestone, rather than a new row; `cargo xtask parity --write` regenerates the summary.
  - done when: tests, each failing before the change: in `dalica-image` (`tests/heif.rs`, its
    no-feature half, with its synthetic builder's `Exif` item): a HEIC with an Exif item answers an
    `Ok` header with its `ispe` size, the EXIF capture date and orientation, while `decode` answers
    `Unsupported(Heif)` with the tile text and `formats::hint(Format::Heif)` is the hint; a bare or
    damaged container still answers `Unsupported(Heif)` (the existing proptest stays green); in
    `dalica-ui` (`heic_hint.rs`, a new `mod` in `tests/all.rs`, the HEIC written by the test as a
    minimal container: `ftyp`, `meta` with `ispe` and an Exif item, no HEVC): the cell's tooltip
    and the edit view's message carry the reason and the hint, and `ui.state` shows the photo's
    capture date; `cargo xtask parity --p0` passes; a screenshot of the HEIC tile and its tooltip
    at 1400 × 900 on the PR; `cargo xtask ci` green.
- ✅ **M10.57 `engine`: a damaged folder holds metadata write-back too**
  - after: M10.52, M10.54
  - files: `crates/engine/src/metadata.rs`, `crates/engine/src/cmd/folder_data.rs`, `crates/engine/tests/folder_data.rs`, `docs/data-model.md`
  - scope: the maintainer's answer at M10.R (2026-10-16, Q5): "nothing in a damaged folder is
    written until you choose Reset Folder Data", and that covers the photo files as well as the
    sidecar. Today M10.52 holds only the sidecar and the carried entries: a caption, tag or date set
    on a photo in a folder on the damaged list (`damaged-folders.json`, kept by
    `cmd/folder_data.rs`'s `Damage` and read through `Session::damaged_folder`) is still written
    into the photo's XMP/IPTC by the metadata write queue (`metadata.rs`). Hold it: the queue
    checks the session's in-memory damaged list (no disk access) before it sends a photo's write,
    and a photo in a damaged folder stays queued but held, as `Writer::waiting` holds a write whose
    volume is offline (it does not keep the session busy, M10.54, and a close does not write it).
    The change stays in the library (and the journal); when the folder's state is lifted (by
    `folder.resetData`, or by a sidecar restored by hand and read again) the held writes are sent
    and land as any other write does. A held write is dropped only when its photo leaves the
    folder before the reset (moved elsewhere or trashed), and then reported (the write job's
    summary or a notice), never silently lost; the library keeps the value either way. Across a
    restart the held writes come back from what the library still marks unwritten
    (`dalica_library::sync`), or the commit body says how they survive. Prefer the smallest
    surface: the check in `metadata.rs`, the release in `cmd/folder_data.rs` where the state is
    lifted; `session.rs` is not in `files` (ask on the issue if the list cannot reach the queue
    without it). One sentence in `data-model.md` §3 (the damaged folder's bullet): metadata
    write-back into its photos is held too until the reset.
  - done when: tests in `dalica-engine` (`folder_data.rs`), failing before the change: a caption
    set on a photo in a damaged folder writes neither the sidecar nor the photo file (its bytes
    unchanged after the queue settles); after `folder.resetData` the caption lands in both; a
    photo moved out of the damaged folder before the reset has its held write dropped and
    reported; `cargo xtask ci` green.
- ✅ **M10.28 Lead: task blocks for M12**
  - after: M9.26
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M12 task block from M12's outline rows and the spec, with waves and the
    `M12.R` release gate (owner: maintainer); a block that can port Craft-app code names the source
    files in its scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists one new
    issue per block.
- ✅ **M10.R Release 0.5 gate**
  - after: M10.24, M10.27, M10.29, M10.31, M10.32, M10.33, M10.34, M10.35, M10.36, M10.37, M10.38, M10.39, M10.40, M10.41, M10.42, M10.43, M10.44, M10.45, M10.46, M10.47, M10.48, M10.49, M10.50, M10.51, M10.52, M10.53, M10.54, M10.55, M10.56, M10.57
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
         (fixes from M9.R, any wave: M10.29 Recently Updated ‖ M10.31 export sections
          ‖ M10.32 file-op follow-ups ‖ M10.33 small follow-ups; M10.30 dropped: the review's budgets
          are judged against a base on the same runner, maintainer decision 2026-10-15)
         (test coverage, any wave: M10.34 crashes and full disks ‖ M10.35 undo under faults
          ‖ M10.36 window close, scales, monkey ‖ M10.37 camera-file write-back)
         (fixes found during M10, any wave: M10.38 failed journal appends ‖ M10.39 the three unbound
          keys ‖ M10.40 one way out, edit-view caption, hidden in query ‖ M10.41 straighten id, F2,
          headless scale, cell tooltip ‖ M10.42 star badges and caption clock after M10.29
          → M10.43 earlier milestones' acceptance after M10.29 ‖ M10.44 macOS and Windows: render
          path, Options tests ‖ M10.45 Windows: killed Save, watcher rename ‖ M10.46 macOS:
          the Properties pane's settle ‖ M10.47 one menu open at a time, window size under
          a UI scale ‖ M10.48 macOS: headless quit test, Save Copy date ‖ M10.49 Windows:
          the path policy's path forms ‖ M10.50 macOS and Windows: W04's caption in the sidecar
          ‖ M10.51 geotag badge, edit-view trash, Ctrl+3 and Ctrl+9 ‖ M10.52 damaged sidecar's
          banner and reset, search empty state ‖ M10.53 export progress in the status line
          ‖ M10.54 connect mode's tray, idle write-back ‖ M10.55 the "▸" in option hints)
wave 7   M10.27 accept M10 → M10.56 HEIC without a decoder → M10.R          (M10.28 lead: M12 blocks, any time after M9.26)
         (from the M10.R answers, any time before M10.R closes: M10.57 a damaged folder holds metadata
          write-back)
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

- ✅ **M11.1 `engine`: M11's command modules, settings and options rows**
  - after: M10.1, M10.5, M10.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/import.rs`, `crates/engine/src/cmd/import_run.rs`, `crates/engine/src/cmd/batch.rs`, `crates/engine/src/cmd/compare.rs`, `crates/engine/src/cmd/slideshow.rs`, `crates/engine/src/cmd/viewer.rs`, `crates/engine/src/import.rs`, `crates/engine/src/import_run.rs`, `crates/engine/src/variant.rs`, `crates/engine/src/viewer.rs`, `crates/engine/src/options.rs`, `crates/engine/src/settings.rs`, `crates/engine/src/session.rs`, `crates/engine/src/jobs.rs`, `crates/engine/tests/settings.rs`, `crates/engine/tests/options_catalogue.rs`, `crates/engine/README.md`, `docs/menus-and-shortcuts.md`, `docs/data-model.md`
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
  - done when: `crates/engine/tests/settings.rs` round-trips every new setting with unknown
    keys kept and wrong types or out-of-range values falling back (reported);
    `crates/engine/tests/options_catalogue.rs` covers the six tabs now shown, every option once,
    each default equal to its accessor's; every existing engine test and `panic_hunt` pass
    unchanged.
- ✅ **M11.2 `ui`: M11's view state, UI commands, the rooms' frame and stubs**
  - after: M10.2, M10.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/menu_catalog.rs`, `crates/ui/src/app.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/rooms.rs`, `crates/ui/src/commands/import.rs`, `crates/ui/src/commands/slideshow.rs`, `crates/ui/src/commands/peek.rs`, `crates/ui/src/commands/compare.rs`, `crates/ui/src/commands/batch.rs`, `crates/ui/src/commands/viewer.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/layout.rs`, `crates/ui/src/panels/rooms.rs`, `crates/ui/src/panels/import.rs`, `crates/ui/src/panels/import/**`, `crates/ui/src/panels/slideshow.rs`, `crates/ui/src/panels/slideshow/**`, `crates/ui/src/panels/peek.rs`, `crates/ui/src/panels/viewer.rs`, `crates/ui/src/panels/viewer/**`, `crates/ui/src/panels/edit.rs`, `crates/ui/src/panels/edit/compare.rs`, `crates/ui/src/panels/edit/top_bar.rs`, `crates/ui/src/panels/edit/caption_bar.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/keep_version.rs`, `crates/ui/src/dialogs/apply_changes.rs`, `crates/ui/src/dialogs/undo_all.rs`, `crates/ui/src/dialogs/remove_from_card.rs`, `crates/ui/src/dialogs/viewer_settings.rs`, `crates/ui/src/dialogs/options.rs`, `crates/ui/src/dialogs/options/slideshow.rs`, `crates/ui/src/dialogs/options/viewer.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state.rs`, `docs/control-protocol.md`
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
  - done when: `crates/ui/tests/view_state.rs`: each new field round-trips through `ui.state`
    / `ui.set` (bad values are `bad_params`, nothing changes); Ctrl+M opens the Import tab and the
    strip appears 28 high, closing it removes the strip; a Slideshow-context key does nothing in
    the library; every command's graceful-failure test; `panic_hunt` and `widget_ids` stay green.
- ✅ **M11.3 `ui-kit`: room tabs, the overlay bar, the stepper, the caption plate and the icons**
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
- ✅ **M11.4 `create`: slideshow sequencing and transition geometry**
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
- ✅ **M11.5 `scan`: cards and drives, and listing an import source**
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
- ✅ **M11.6 `fs`: import copies that keep their time, and removing a card's copied files**
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
- ✅ **M11.7 `engine`: import sources, listing and duplicates**
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
- ✅ **M11.9 `engine`: Batch Edit, Copy and Paste All Edits, and Undo All Edits**
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
- ✅ **M11.10 `engine`: two preview panes, and A│A's variant**
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
- ✅ **M11.12 `engine` + `dalica`: the viewer's session and `dalica --view`**
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
- ✅ **M11.13 `ui`: the room tab strip**
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
- ✅ **M11.8 `engine`: importing: copy, verify, name, enter, undo and card removal**
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
- ✅ **M11.11 `engine`: what a slideshow plays, and transitions that never wait on disk**
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
- ✅ **M11.14 `ui`: the quick full-screen preview**
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
- ✅ **M11.15 `ui`: the Import room: sources, the import tray and the preview**
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
- ✅ **M11.16 `ui`: compare, before/after and their two questions**
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
- ✅ **M11.17 `ui`: the photo viewer**
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
- ✅ **M11.18 `ui`: Batch Edit, Copy and Paste All Edits, and Undo All Edits in the window**
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
- ✅ **M11.19 `ui`: the slideshow: what plays, the show, transitions and keys**
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
- ✅ **M11.20 `ui`: the Import room: destination, importing and finishing**
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
- ✅ **M11.22 `mcp`: Batch Edit as one step, and the workflows W07 and W10**
  - after: M11.8, M11.9
  - files: `crates/mcp/src/tools/edit.rs`, `crates/mcp/tests/edit_tools.rs`, `crates/mcp/tests/workflows/w07_batch_edit_one_step.rs`, `crates/mcp/tests/workflows/w10_import_duplicates.rs`, `xtask/src/stats.rs`, `docs/mcp.md`
  - scope: `mcp.md` §4.2 and §9: `apply_edit` runs `edit.apply`, one library undo step for all its
    photos as §4.2 says, instead of M10's per-photo steps; W07 (Auto Fix on 20 photos, 20 recipes
    ending in `autofix()`, one `cmd_edit_undo` removing all 20) and W10 (a card-like folder
    imported twice through the `cmd_import_*` tools: the second skips every photo and reports them
    as duplicates; the folders named as configured; each copy's bytes equal to its source)
    un-ignored; the floor in `xtask/src/stats.rs` raised to the eleven workflows now required;
    `mcp.md`'s status line; row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp --test workflows` runs W07 and W10 green (on all three OSes at the review);
    `crates/mcp/tests/edit_tools.rs`: `apply_edit` on 3 photos is one undo step; `cargo xtask
    stats` prints 11 / 12 passing and `stats --check` fails below that.
- ✅ **M11.21 `ui`: the slideshow's control bar, zoom, and Options ▸ Slideshow**
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
- ✅ **M11.23 `engine` + `ui` + `xtask`: M11's budgets: import, batch edit and the slideshow**
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
- ✅ **M11.27 `ui` + `engine`: A│A's right side takes the tools**
  - after: M11.16
  - files: `crates/engine/src/cmd/tuning.rs`, `crates/engine/tests/tuning_commands.rs`, `crates/ui/src/panels/edit/crop.rs`, `crates/ui/src/panels/edit/straighten.rs`, `crates/ui/src/panels/edit/retouch.rs`, `crates/ui/src/panels/edit/tuning.rs`, `crates/ui/src/panels/edit/fill_light.rs`, `crates/ui/src/panels/edit/panel.rs`, `crates/ui/src/commands/tools.rs`, `crates/ui/src/commands/compare.rs`, `crates/ui/tests/compare_tools.rs`
  - scope: a fix task from M11.16's run. Compare routes Auto Fix, the one-click fixes, rotations,
    flips, Undo and Redo to the focused side (the variant in A│A), but greys the right side's tool
    panels (Crop, Straighten, Retouch, Tuning, Fill Light) with a reason: they build their
    candidates from the photo's own recipe, and `photo.history.tuning` has no `variant` parameter.
    Wire each panel to the focused side's recipe (its candidate, preview and commit go to the
    variant on A│A's right side); `photo.history.tuning` takes `variant: true` as
    `photo.history.apply` does (M11.10), its params doc saying so; remove the greying and its
    reason. The new test file is a `mod` of `crates/ui/tests/all.rs`; row `EDIT-COMPARE`.
  - done when: `crates/engine/tests/tuning_commands.rs`: a tuning session on the variant leaves
    the photo's sidecar byte-identical, and `variant: true` with no variant open fails gracefully;
    `crates/ui/tests/compare_tools.rs` (headless, by id): each of Crop, Straighten, Retouch, Tuning
    and Fill Light on A│A's right side edits the variant and leaves the photo's sidecar
    byte-identical, the panels are no longer greyed, and keeping the right side writes the tool's
    step as one history step; graceful failures; `panic_hunt` green; `cargo xtask ci` green.
- ✅ **M11.28 `ui`: General's "Save imported pictures in" row goes live**
  - after: M11.20
  - files: `crates/ui/src/dialogs/options/general.rs`, `crates/ui/tests/options_dialog.rs`, `crates/ui/tests/fingerprints/dialog-options-general-*.json`
  - scope: a fix task: M11.1 made `import.folder` settable through `options.set`, but Options ▸
    General still draws "Save imported pictures in" and its Browse… disabled ("Arrives with
    Import"). The field shows the stored folder, else the Pictures folder (as the hint); Browse…
    opens the folder picker off the UI thread and puts the choice into the draft through
    `app.preferences.folder`, so Apply stores it; "Detect duplicates while importing" works too
    if it is still drawn disabled; the module doc's sketch says what this build offers; row
    `PREF-GENERAL`.
  - done when: `crates/ui/tests/options_dialog.rs` (headless, by id): a folder chosen through the
    dialog and applied is stored in `import.folder`, and the Import room's destination ("Import
    to:") uses it; the duplicates checkbox stores `import.detectDuplicates` if it was disabled;
    the fingerprint of the General tab at 1400 × 900 updated (images in the PR); `cargo xtask ci`
    green.
- ✅ **M11.29 `ui`: the slideshow's change frame stays within a 60 Hz frame**
  - after: M11.21, M11.23
  - files: `crates/ui/src/panels/slideshow/show.rs`, `crates/ui/src/panels/slideshow.rs`, `crates/ui/src/commands/slideshow.rs`, `crates/ui/tests/slideshow.rs`, `crates/ui/benches/slideshow.rs`
  - scope: a fix task from M11.23's run. Its `slideshow` bench measured the advance frame at a
    median of 16 ms and a p95 of about 28 ms on two cores, because the next photo's texture is
    uploaded in the change's first frame; `viewing.md` §1 says a change never waits. Upload (or
    prepare) the next photo's texture during the current photo's display time, so the change's
    first frame does no large upload; memory stays bounded (at most the current and the next
    textures); a show stepped by hand still works when the next texture is not ready (it falls
    back to the thumbnail, as today). The bench file is M11.23's, hence the `after`; row
    `VIEW-SLIDESHOW`.
  - done when: `cargo xtask bench slideshow`: the advance frame's p95 is at most 16 ms on the same
    two-core setup (the before and after numbers in the PR); `crates/ui/tests/slideshow.rs`
    (headless, a virtual clock): the next photo's texture is uploaded before the change starts;
    no new frame waits (the 30-change test with the 1.5 s planted delay still reports 0 waits);
    `cargo xtask ci` green.
- ✅ **M11.24 `accept M11`**
  - after: M11.14, M11.16, M11.17, M11.18, M11.20, M11.21, M11.22, M11.23, M11.27, M11.28, M11.29
  - files: `tests/acceptance/m11.rs`, `tests/acceptance/m11_*.rs`, `tests/acceptance/m11.md`, `tests/acceptance/fingerprints/m11-*.json`
  - scope: every *Done when* of M11's four areas in the real `dalica --headless` over the control
    channel, through `dalica-cli mcp`, and with `dalica --view FILE --headless`: a simulated card (a
    temp folder through `DALICA_DEVICE_ROOTS`) imported into dated folders with every copy
    verified, a second import skipping every photo as a duplicate, a planted clash getting `-001`,
    removal from the card only after the question and refused over the control channel; a 100-photo
    batch as one undo step; a version kept from A│A as one history step; the slideshow's zero waits
    on a slowed decoder, a fingerprint per transition, its keys, wheel and bar; `dalica --view`
    opening, stepping, starring and Open in Dalica; W07 and W10 over MCP; the M11 budgets (the
    release budgets with `DALICA_ACCEPT_RELEASE=1`; inside `cargo xtask ci --full` a smaller debug
    run checks the paths and prints its times); fingerprints of the room tabs, the Import room,
    compare, the slideshow and its bar and the viewer; the manual checks in `m11.md` (import from a
    real card or USB stick and remove the copies from it, a slideshow on a real screen with each
    transition, the viewer from the file manager's Open with on each OS with its translucent
    backdrop, Ctrl+Alt over a thumbnail).
  - done when: `cargo xtask accept M11` passes everything in M11's *Done when*.
- ✅ **M11.25 Lead: task blocks for M13**
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
- ✅ **M11.26 `ui` + `dalica`: tests for the details the P0 audit left untested**
  - after: M10.R
  - files: `crates/ui/tests/section_header.rs`, `crates/ui/tests/photo_menu.rs`, `crates/ui/tests/tray.rs`, `crates/ui/tests/crop_tool.rs`, `crates/ui/tests/edit_canvas.rs`, `crates/ui/tests/titlebar_drag.rs`, `apps/dalica/tests/control_security.rs`, `apps/dalica/src/control/transport.rs`, `xtask/src/ci.rs`, `docs/parity.md`
  - scope: the maintainer confirmed the P0 audit's moves (M10.26) at M10.R (2026-10-16, Q7) and
    asked for one later task for the details its parity notes call "untested". A test for each,
    in the feature's existing test file (a new file only if none fits, as a `mod` of its crate's
    `tests/all.rs`): LIB-LT-HEADER, the header's Export taking the selection inside its section
    rather than the whole section (`section_header.rs`); LIB-CTX-PHOTO, the album rows off in
    Recently Updated and in search results, as the test already checks Starred Photos
    (`photo_menu.rs`); LIB-TRAY, a Ctrl-click taking a mini out of the tray's selection and a Ctrl
    band adding to it (`tray.rs`); EDIT-CROP, Preview held (not only the toggle) and the crop panel
    at 900 × 600 (`crop_tool.rs`; layout and widget checks, no new fingerprint at the small window
    size); EDIT-ZOOM, the hand cursor while panning a zoomed photo (`edit_canvas.rs`, through
    `ui.inspect` or the frame's cursor icon); SYS-WINDOW, clicks on the caption buttons and the
    title bar over the opening and could-not-open screens (`titlebar_drag.rs`); SYS-CONTROL, the
    control channel's 30 s idle close (`control_security.rs`): the limit stays 30 s in the product,
    and the test shortens it through the smallest seam in `transport.rs` (a constructor argument
    or a test-only setting, never an environment variable a release build reads), the choice in
    the commit body. EDIT-SAVE's HEIC case: release builds have no HEIC decoder (the M10.R answer,
    M10.56), and the case runs only under `cargo test -p dalica-engine --features heif --test all
    save_formats::`, which no gate runs; add it to `ci --full`'s "test heif" step in
    `xtask/src/ci.rs` beside `dalica-image`'s (preferred: self-builds keep the feature), or drop
    the item with the reason in the row's note. This adds a test, never loosens a gate. As each
    lands, drop its "untested:" words from the row's note in `parity.md` (keep the rest of the
    note, the moved parts and the "missing:" manual checks); `cargo xtask parity --write`.
  - done when: each named detail has a test (or EDIT-SAVE's HEIC item a stated reason); no note
    in `parity.md` says "untested" for these eight rows; `cargo xtask ci` green.
- ✅ **M11.R Release 0.6 gate**
  - after: M11.24, M11.26
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
         (from the M10.R answers, any wave: M11.26 tests for the audit's untested details)
         (fix tasks: M11.27 A│A's right side takes the tools, after M11.16  ‖  M11.28 General's
         import folder row, after M11.20  ‖  M11.29 the slideshow's change frame within 60 Hz,
         after M11.21 and M11.23)
wave 6   M11.24 accept M11 → M11.R          (M11.25 lead: M13 blocks, any time after M10.28)
```

### M12 — Effects

One milestone in 3 feature areas (*Effect tiles and panels*, *Tab 3 effects*, *Tab 4 and 5 effects*); one `cargo xtask accept M12` covers every *Done when* below. It ends with a release gate the maintainer owns (Release 0.7).

#### Effect tiles and panels

- **In:** the edit panel's tabs 3–5 (`editing.md` §8.1): a fixed grid of live 78 × 48 tiles per
  tab, rendered in the background on a `gray 38` placeholder, with labels, tooltips and the
  "1-click" badge; one-click effects applied on a click, Ctrl+Shift+B for B&W; the parameter panel
  (title, up to four sliders with the label centred above, checkboxes, a choice, colour swatches
  with a picker and an eyedropper, Fade, Apply and Cancel) with a live preview; the crosshair on
  the photo for the four focal effects; Soften's brush and eraser; Batch Edit's five effects
  (`editing.md` §9) made live; the workflow W11 over MCP.
- **Out:** the Shift variants and the novelty effects (`FX-SHIFT`, `FX-NOVELTY`, dropped
  2026-10-09); a GPU path (the CPU renderer stays the only one, `development.md` §14); effects on
  videos (M15); rendering edits at more than 8 bits (the M8.R note).
- **Reuse** (`architecture.md` §9): PhotoCraft `crates/ui-egui/src/color_picker_ui.rs` (the colour
  maths and the saturation / value field) and `crates/ui-egui/src/widgets.rs` (`swatch_popup`,
  `color_swatch`) for the swatches; LightCraft `crates/pipeline/src/masks.rs` (brush dabs) for
  Soften's mask; `ui-kit`'s Tuning slider (M7.5) for the panel's sliders.
- **Done when:** `cargo xtask accept M12`: every tab's grid fills with previews and never waits on
  the disk; each of the 27 effects, from its tile or its panel, is one step of the photo's history
  named after it, survives a restart and a rebuild and shows on its thumbnail; Cancel records
  nothing; a crosshair placed on a turned photo lands where it was put; Batch Edit ▸ Sepia on 20
  photos is one undo step; W11 passes over MCP.

#### Tab 3 effects

- **In:** Sharpen, Sepia, B&W, Warmify, Film Grain, Tint, Saturation, Soft Focus, Glow, Filtered
  B&W, Focal B&W, Graduated Tint (`editing.md` §8.2) as operations of the CPU renderer.
- **Reuse:** PhotoCraft `crates/compose/src/adjust.rs` (`hue_saturation`, `black_white_gray`,
  `photo_filter_matrix`, `vibrance_px`), `crates/algo/src/sharpen.rs` (`unsharp`) and
  `crates/algo/src/noise.rs` (`add`, `hash01`); LightCraft `crates/raster/src/blur.rs` (three box
  passes for a Gaussian) and `crates/pipeline/src/finish.rs` (`grain_noise`).
- **Done when:** `cargo xtask accept M12`: a fingerprint and an invariant per effect, each the same
  at two render sizes after a downscale and in a zoomed region as in the whole render.

#### Tab 4 and 5 effects

- **In:** Infrared Film, Plastic Lens, HDR Look, Widescreen Film, Dream Glow, Invert Colors,
  Duo-Tone (§8.3); Boost, Soften with its paint mask, Vignette, Pixelate, Focal Zoom, Border, Drop
  Shadow, Museum Matte (§8.4); effects that change the picture's size, everywhere a size is read
  (thumbnails, the edit view, Save, Export, `render_photo`).
- **Reuse:** PhotoCraft `crates/algo/src/blur.rs` (`radial`) and `crates/algo/src/blur/radial.rs`
  for Focal Zoom, `crates/algo/src/stylize.rs` (`mosaic`) for Pixelate,
  `crates/compose/src/effects.rs` (`shadow_map`, `tent_kernel`, `offset`) for Drop Shadow;
  LightCraft `crates/pipeline/src/finish.rs` (`vignette`) for Vignette.
- **Done when:** `cargo xtask accept M12`: as tab 3, plus: a Border's output is the photo grown by
  its thickness on every side with the photo's pixels unchanged inside; Save and Export write the
  grown picture; Soften applies only where it was painted.

#### M12 tasks

M12 fills the three effect tabs: 27 effects as operations of the CPU renderer, the tile grids, the
parameter panel, the crosshair and Soften's brush. Its first wave lists the M11 work each task
extends and starts once the maintainer has closed M11.R. Four tasks lay the ground the others share,
so later tasks never share a file: M12.1 gives `edit` the syntax of all 27 effects (typed
parameters, ranges, defaults, labels, catalogue rows, whether each changes the picture's size) with
render stubs in one module per family, so each renderer task owns one module; M12.2 adds M12's view
state, UI commands with stub handlers and the stub panels; M12.3 gives `edit` the primitives the
effects share (blurs, seeded noise, masks, brush dabs, blends and Fade); M12.4 gives `ui-kit` the
effect tile, the effect slider, the colour swatch and its picker, the crosshair and the icons, and
`core` the colour maths the picker and the engine's colour pick share. Nine renderer tasks
(M12.6–M12.14) each own one `ops/<family>.rs` and all but M12.14 (built on M12.13's size-changing
kind) run side by side; the engine's effect list, tile previews and colour pick (M12.5) feed the
tile grid (M12.15) and the panel (M12.16), and the crosshair (M12.17) and Soften's brush (M12.18)
act through the panel's session. Each task updates its `parity.md` rows (`FX-TILES`, `FX-T3`,
`FX-T4`, `FX-T5`, the effect tabs' text in `EDIT-TABS`, the effects of `EDIT-BATCH`, `SYS-MCP`, the
Picture menu row and the generated `KEY-*` row of Ctrl+Shift+B; new rows where none fit: `FX-PANEL`
for the parameter panel, its swatches and Fade, `FX-CROSSHAIR` for the crosshair) and names its
commands there; first fingerprints of the 27 effects, the tile grids, the panels, the colour picker,
the crosshair and the new widgets and icons are approved in bulk on the M12.R review page.

Whole-window baselines belong to no task, as in M5–M11: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m11-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.
Fingerprints are made at 1400 × 900 only; at 900 × 600 a test asserts that the grid, the panel and
the picker fit (`architecture.md` §11).

Decisions taken while planning (the check-in lists them for reversal): an effect is one entry of
the recipe and one step of the photo's own history ("Undo Sepia"), from a one-click tile or from a
panel's Apply; while a panel is open the photo previews through the edit view's candidate recipe
(as Tuning and Straighten do) and the recipe is untouched until Apply; Cancel and Esc record
nothing; opening an effect again always appends a new entry (effects stack; an earlier entry is
never edited in place, unlike Crop). Slider controls follow the Tuning slider (no number, the
value as a tooltip, + / − nudge 2 %, a double-click resets) with the label centred above; Glow's
Radius is on a logarithmic scale. Parameters the spec leaves open (Soft Focus's Size and Amount,
Focal B&W's Size and Sharpness, Glow's Radius range, Filtered B&W's default colour, Focal Zoom's
Focal Size, Border's corner radius and caption height, Pixelate's blend modes) are decided in
M12.1 from the effects' descriptions and written into `editing.md` §8. Every size, radius,
thickness and grain size is relative to the picture's shorter side, every centre a normalised
point in the stored frame (reserved decision 11), and every noise seeded from its entry's own
parameters and placed in stored-frame coordinates, so a thumbnail, the edit view, a zoomed region and a Save of the same
recipe agree. Colours are `#rrggbb` in sRGB; a swatch opens a picker popup (a saturation / value
field, a hue strip, a hex field and the eyedropper); the eyedropper, and Vignette's "pick from
photo", sample the photo as shown before the effect being set (the recipe so far, a mean over a
small disc). Widescreen Film, Border, Drop Shadow and Museum Matte change the picture's size where
they stand in the recipe: geometry stays first (a crop made after a Border crops the photo, not
the frame), later effects work on the grown or letterboxed picture, rotations and flips turn the
framed picture, a later effect's centre stays on the photo (the crosshair cannot be put on a
frame), and every reader of a size (thumbnails, the edit view, Save, Export, `render_photo`) takes
it from the same `Geometry`; a recipe whose frames would pass the render limits skips the frames
past them and keeps them. Soften applies to the whole photo until its first stroke; from then the
brush paints where it applies and the eraser takes it away; strokes are stored in the entry in the
stored frame (at most 1 000, as Retouch). A tile is the photo with its recipe so far plus the
effect at its defaults, rendered on a worker from a small decode (at least twice the tile in
device pixels, made once per photo and recipe) at a new `effectTiles` priority below the edit
view's preview, cancelled when the photo changes and remade when its recipe does; tiles are never
written to disk. Ctrl+Shift+B applies B&W in the edit view only (the Edit context); in the library
it is Batch Edit ▸ Black and White. The Shift variants stay dropped, so `control-protocol.md`'s
"Shift over an effects tab" line goes. Ports from the Craft apps get their `NOTICE` rows from
M12.1, which writes one row per port named in the blocks below, so the porting tasks never share
`NOTICE`; a port not named here asks the lead for its row. M12's budgets join `development.md` §8
at 1.5 × their first measurement, plus the edit view's (a panel's draft within 16 ms at p95 and its
final within 100 ms at screen size, for every effect), and are never loosened. No step of M12 needs
a maintainer decision mid-run.

- ✅ **M12.1 `edit`: the 27 effects' syntax, parameters and catalogue, with render stubs**
  - after: M10.9, M11.R
  - files: `crates/edit/src/syntax.rs`, `crates/edit/src/ops/mod.rs`, `crates/edit/src/ops/colour.rs`, `crates/edit/src/ops/detail.rs`, `crates/edit/src/ops/focal.rs`, `crates/edit/src/ops/remap.rs`, `crates/edit/src/ops/looks.rs`, `crates/edit/src/ops/soften.rs`, `crates/edit/src/ops/stylize.rs`, `crates/edit/src/ops/canvas.rs`, `crates/edit/src/ops/mats.rs`, `crates/edit/src/render.rs`, `crates/edit/src/catalog.rs`, `crates/edit/tests/recipe.rs`, `crates/edit/tests/catalog.rs`, `crates/edit/README.md`, `docs/editing.md`, `docs/data-model.md`, `NOTICE`
  - scope: `editing.md` §8 and `data-model.md` §3.1: every effect of tabs 3–5 known to the
    parser (`LISTED` gains the 15 of tabs 4–5) with a typed module: parameters with their range,
    default, clamp and print (`#rrggbb` colours, stored-frame points, degrees, checkboxes and
    choices by name, Soften's strokes as Retouch's), `VERSION` 0 and its history label, in one
    module per family so the renderer tasks never share a file: `colour.rs` (Sepia, B&W, Warmify,
    Tint, Saturation, Filtered B&W), `detail.rs` (Sharpen, Film Grain, Glow), `focal.rs` (Soft
    Focus, Focal B&W, Graduated Tint), `remap.rs` (Invert Colors, Infrared Film, Duo-Tone, Boost),
    `looks.rs` (HDR Look, Dream Glow, Plastic Lens), `soften.rs`, `stylize.rs` (Vignette,
    Pixelate, Focal Zoom), `canvas.rs` (Widescreen Film, Border) and `mats.rs` (Drop Shadow,
    Museum Matte); `Typed`, `label`, `version` and the renderer's dispatch list them all, each
    `prepare` a stub that changes nothing; the catalogue rows of M10.9 for the 27 (tab and grid
    place, one-click, crosshair kind, changes the size, the English tooltip, saying so for the four
    that change the size); the open ranges and defaults decided above, written into `editing.md`
    §8; `data-model.md` §3.1's effects row with every effect's syntax; the `NOTICE` rows of every
    port named in M12's blocks.
  - done when: `crates/edit/tests/recipe.rs`: every effect at its defaults prints and parses back
    equal; out-of-range values clamp; a `NaN`, a missing field or a bad colour keeps the entry and
    skips it; `crates/edit/tests/catalog.rs`: the 27 effects are listed in each tab's grid order
    with ranges matching their clamps, each tab holding at most 12; a recipe with every effect
    renders as the same recipe without them (stubs); every existing `edit` test passes
    unchanged.
- ✅ **M12.2 `ui`: M12's view state, UI commands and the effect panels' stubs**
  - after: M11.2, M11.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/tools.rs`, `crates/ui/src/commands/effects.rs`, `crates/ui/src/commands/effect_session.rs`, `crates/ui/src/panels/edit.rs`, `crates/ui/src/panels/edit/panel.rs`, `crates/ui/src/panels/edit/canvas.rs`, `crates/ui/src/panels/edit/effects.rs`, `crates/ui/src/panels/edit/effect_panel.rs`, `crates/ui/src/panels/edit/effect_panel/**`, `crates/ui/src/panels/edit/crosshair.rs`, `crates/ui/src/panels/edit/soften_brush.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state.rs`, `docs/control-protocol.md`, `docs/menus-and-shortcuts.md`
  - scope: every M12 field of `UiState` in one go: `edit.tool` gains `effect`, and `edit.effect`
    is `null` or the open panel (the effect's name, its parameter values, the crosshair being
    dragged, Soften's strokes, brush size and paint or erase, the swatch whose picker is open, the
    eyedropper armed); UI commands with stub handlers, so M12.15–M12.18 never share a file:
    `edit.effect.apply {name}` (one-click) and `edit.bw` (Ctrl+Shift+B in the Edit context) in
    `commands/effects.rs`, and the effect session in `commands/effect_session.rs` behind
    `edit.tool.open {tool: "effect", name}`, `edit.tool.action`, `.apply`, `.cancel` and `.reset`;
    the edit panel routing tabs 3–5 to the stub grid `panels/edit/effects.rs` and an open effect to
    the stub `effect_panel.rs` (and its folder); the canvas's overlay hooks for the stub
    `crosshair.rs` and `soften_brush.rs`; control stubs for the new state; `control-protocol.md`
    §6.5 (`edit.effect`), §7 (the effect ids it lists plus `choice:effect.<param>`,
    `button:effect.pick/<param>`, `popup:color`, `button:effect.paint`, `button:effect.erase`) and
    the "Shift over an effects tab" line removed; `menus-and-shortcuts.md` §1.9 lists the new
    commands (`effect.list`, `photo.effect.pick`, `edit.effect.apply`, `edit.bw`) and §2 the Edit
    context's Ctrl+Shift+B.
  - done when: `crates/ui/tests/view_state.rs`: `edit.effect` round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); `edit.tool.open {tool: "effect",
    name: "sharpen"}` opens the stub panel and Esc closes it; Ctrl+Shift+B does nothing in the
    library; every command's graceful-failure test; `panic_hunt` and `widget_ids` stay green.
- ✅ **M12.3 `edit`: the effects' primitives: blurs, seeded noise, masks, brush dabs, blends and Fade**
  - after: M11.R
  - files: `crates/edit/src/fx.rs`, `crates/edit/src/fx/**`, `crates/edit/tests/fx.rs`
  - scope: what several effects share, on the renderer's bands (`ops::Band`, `View`, `Reach`), so
    no renderer task writes its own: a separable Gaussian by three box passes on a pass's `before`
    view with its reach (`3σ`), ported from LightCraft `crates/raster/src/blur.rs` (`box_radii`,
    `gaussian`) at 0da998f2; seeded noise as a function of stored-frame coordinates and the
    entry's seed, the same in a zoomed region and at any size, from PhotoCraft
    `crates/algo/src/noise.rs` (`hash01`) at e4b2868a and LightCraft
    `crates/pipeline/src/finish.rs` (`grain_noise`); masks in output pixels from stored-frame
    shapes through `Prepare::geometry` (a circle with a feathered edge, a linear gradient with its
    start, angle and feather, the distance from the edges); brush dabs along a stroke, ported from
    LightCraft `crates/pipeline/src/masks.rs` (`brush_dabs`, `dab_spacing`, `dab_alpha`); blends
    (screen, overlay, soft light, multiply) and Fade (`alpha = 1 − Fade/100` toward the input);
    luma (Rec.601) and sRGB ↔ linear; each file with its header.
  - done when: `crates/edit/tests/fx.rs`: the blur keeps a flat plane, spreads a dot by its sigma
    and equals the whole blur in a region with its margin; the noise at a stored-frame point is the
    same at two scales and in a region, has the mean and spread asked for and differs by seed; a
    circle mask is 1 at its centre and 0 past its feather on a turned and cropped photo; dabs cover
    a stroke without gaps; Fade 0 gives the effect and 100 the input; hostile sizes (zero, huge,
    `NaN`) answer without a panic.
- ✅ **M12.4 `ui-kit` + `core`: effect tiles, the effect slider, colour swatches and their picker, the crosshair, the icons**
  - after: M11.3, M11.R
  - files: `crates/core/src/color.rs`, `crates/core/tests/color.rs`, `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/effect_tile.rs`, `crates/ui-kit/src/widgets/slider.rs`, `crates/ui-kit/src/widgets/swatch.rs`, `crates/ui-kit/src/widgets/swatch/**`, `crates/ui-kit/src/widgets/crosshair.rs`, `crates/ui-kit/src/theme.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/effects.rs`, `crates/ui-kit/tests/effect_widgets.rs`, `crates/ui-kit/tests/icons_effects.rs`, `crates/ui-kit/tests/fingerprints/effect-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-effects*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `design-system.md` §4–§6 and `editing.md` §8.1: `core::color`, sRGB ↔ HSV and
    `#rrggbb` print and parse, ported from PhotoCraft `crates/ui-egui/src/color_picker_ui.rs`
    (`hsv_to_rgb`, `rgb_to_hsv`, `hex`, `parse_hex`) at e4b2868a; `effect_tile` (86 × 69: the
    78 × 48 preview or the `gray 38` placeholder, the SemiBold 11 label, the "1-click" corner
    badge, hover, pressed and disabled, a tooltip; registers `effect:<name>`); `effect_slider` (the
    Tuning slider with its label centred above, and a logarithmic mapping); `swatch` (a colour
    button that opens a popup kept inside the window: a saturation / value field and a hue strip
    from `color_picker_ui.rs` (`field_color`, `locate`, `grid_mesh`), a hex field, the eyedropper
    button and its armed state; the popup's placement ported from PhotoCraft
    `crates/ui-egui/src/widgets.rs` (`swatch_popup`, `color_swatch`)); `crosshair` (a draggable
    centre with an optional circle of a given radius and an optional second handle for a start and
    angle; reports drags in its own normalised coordinates); the icons it lacks, our own drawings
    on the 20-unit grid: Brush, Eraser, Crosshair, the 1-click badge; the widget gallery gains
    them; `design-system.md` §6's rows.
  - done when: `crates/core/tests/color.rs`: HSV and hex round-trip every 8-bit colour, bad hex is
    refused; `crates/ui-kit/tests/effect_widgets.rs`: a 3 × 4 grid of tiles fits the 280-wide panel,
    a tile without a preview paints `gray 38`, the badge shows only when asked; the slider's
    logarithmic mapping round-trips its range; a colour picked in the field and the hue strip gives
    that colour back, the popup stays inside a 900 × 600 window from a swatch at each corner; the
    crosshair reports a drag of its centre and of its angle handle;
    `crates/ui-kit/tests/icons_effects.rs`: each new icon stays in its grid at 12, 16, 20 and 28 px in rest, hover and disabled colours;
    gallery and icon fingerprints; `widget_ids`, contrast, `cargo xtask assets` and `cargo xtask
    names` pass.
- ✅ **M12.5 `engine`: the effect list, tile previews and the colour pick**
  - after: M11.1, M12.1, M12.4
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/effects.rs`, `crates/engine/src/effect_tiles.rs`, `crates/engine/src/jobs.rs`, `crates/engine/tests/effects.rs`, `crates/engine/README.md`
  - scope: `editing.md` §8.1 and the decisions above: `effect.list {tab?}` (query: the catalogue's
    effects in grid order with their parameters, defaults, one-click, crosshair kind, size change
    and tooltip); `Session::request_effect_tiles(id, tab, size)` and `take_effect_tiles()`: each
    tile the photo with its recipe so far plus the effect at its defaults, rendered on a worker from
    a small decode (at least twice the tile, made once per photo and recipe and kept in memory) at a
    new `effectTiles` priority below the edit view's preview, in grid order, cancelled when the
    photo changes and remade when its recipe does; `photo.effect.pick {id, x, y}` (an `analysis`
    job, as `photo.tuning.pick`: the mean colour of a small disc at a point of the photo as the
    edit view shows it, its recipe so far, answering `#rrggbb` through `core::color`); row
    `FX-TILES` (engine half).
  - done when: `crates/engine/tests/effects.rs`: `effect.list` gives the 27 effects in the three
    grids' order, equal to the catalogue; tab 3's 12 tiles arrive in grid order at the size asked
    for, Sepia's tile has R ≥ G ≥ B everywhere, a tile request waits behind the main preview and is
    cancelled by opening another photo and remade after an edit; the owner thread made no `fs`
    call; `photo.effect.pick` answers a planted patch's colour on a turned photo; each command's
    contract and graceful failures; `panic_hunt` green.
- ✅ **M12.6 `edit`: Sepia, B&W, Warmify, Tint, Saturation and Filtered B&W**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/colour.rs`, `crates/edit/tests/colour_effects.rs`, `crates/edit/tests/fingerprints/colour-*.json`
  - scope: `editing.md` §8.2's six colour effects as pixel passes (`Reach::Pixel`, fused with
    their neighbours), each `VERSION` 1, with PhotoCraft `crates/compose/src/adjust.rs` at
    e4b2868a as the source: Sepia (fixed warm-brown curves over luma), B&W (Rec.601 luma), Warmify
    (a warming filter matrix, `photo_filter_matrix`, kind to skin tones), Tint (desaturate, then
    colourise toward the colour, `hue_saturation` with colorize, with Fade), Saturation (−1 toward
    B&W, +1 vivid, `vibrance_px` for the positive side), Filtered B&W (luma weighted by a coloured
    filter, `black_white_gray`); rows `FX-T3`.
  - done when: `cargo test -p dalica-edit --test all colour_effects::`: a fingerprint per effect
    at its defaults on a `scenes` photo; invariants: B&W and Filtered B&W give R = G = B, B&W's grey
    is the input's Rec.601 luma within one level, a red filter makes a red patch lighter than a
    blue one of the same luma; Sepia gives R ≥ G ≥ B and keeps the order of lumas; Warmify raises
    R − B and keeps luma within 3 %; Tint gives the tint's hue to every coloured pixel, and Fade 100
    gives the input; Saturation 0 is the identity and −1 gives R = G = B; each effect renders the
    same at two sizes after a downscale.
- ✅ **M12.7 `edit`: Sharpen, Film Grain and Glow**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/detail.rs`, `crates/edit/tests/detail_effects.rs`, `crates/edit/tests/fingerprints/detail-*.json`
  - scope: `editing.md` §8.2: Sharpen (an unsharp mask whose radius is relative to the picture,
    exact at 1:1, from PhotoCraft `crates/algo/src/sharpen.rs` (`unsharp`) at e4b2868a), Film
    Grain (monochrome grain from M12.3's seeded noise, its size relative to the picture, Lighten
    adding only light grain; PhotoCraft `crates/algo/src/noise.rs` (`add`)), Glow (the highlights
    blurred by Radius and screened back by Intensity); each `VERSION` 1; rows `FX-T3`.
  - done when: `cargo test -p dalica-edit --test all detail_effects::`: a fingerprint per effect at
    its defaults; invariants: Sharpen 0 and Grain 0 are the identity, Sharpen leaves a flat patch
    alone and raises an edge's contrast, a zoomed region at 1:1 equals that window of the whole
    render; Grain keeps the mean luma within one level (without Lighten), is the same in a region
    as in the whole render and gives R − G = 0 noise; Glow never darkens a pixel and leaves a dark
    flat patch alone; each effect renders the same at two sizes after a downscale.
- ✅ **M12.8 `edit`: Soft Focus, Focal B&W and Graduated Tint**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/focal.rs`, `crates/edit/tests/focal_effects.rs`, `crates/edit/tests/fingerprints/focal-*.json`
  - scope: `editing.md` §8.2's crosshair effects on M12.3's masks and blur, their centres in the
    stored frame mapped through `Prepare::geometry`: Soft Focus (a blur outside a sharp circle of
    Size, by Amount), Focal B&W (colour inside the circle, B&W outside, the edge's softness by
    Sharpness), Graduated Tint (a coloured gradient from the crosshair's start along its angle,
    Feather and Shade, whites staying white); each `VERSION` 1; rows `FX-T3`.
  - done when: `cargo test -p dalica-edit --test all focal_effects::`: a fingerprint per effect at
    its defaults; invariants: inside the circle Soft Focus and Focal B&W leave the photo unchanged,
    far outside Focal B&W gives R = G = B and Soft Focus lowers the local variance; Graduated Tint
    leaves the pixels behind its start unchanged and a white patch white; on a photo turned 90° and
    cropped the circle sits where its stored-frame centre says; each effect renders the same at two
    sizes after a downscale.
- ✅ **M12.9 `edit`: Invert Colors, Infrared Film, Duo-Tone and Boost**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/remap.rs`, `crates/edit/tests/remap_effects.rs`, `crates/edit/tests/fingerprints/remap-*.json`
  - scope: `editing.md` §8.3–§8.4's colour remaps as pixel passes: Invert Colors (the negative),
    Infrared Film (a B&W channel mix where foliage turns bright and skies dark, with Fade), Duo-Tone
    (luma after Brightness and Contrast mapped onto the gradient from the first colour to the
    second, with Fade; PhotoCraft `crates/compose/src/effects.rs` (`sample_stops`) at e4b2868a as
    the pattern), Boost (stronger colour and contrast by Strength; `crates/compose/src/adjust.rs`
    (`vibrance_px`, `modern_contrast`)); each `VERSION` 1; rows `FX-T4`, `FX-T5`.
  - done when: `cargo test -p dalica-edit --test all remap_effects::`: a fingerprint per effect at
    its defaults; invariants: Invert twice is the identity within one level; Infrared gives R = G = B
    with a green patch lighter and a blue one darker than their luma; Duo-Tone maps black to the
    first colour and white to the second at Brightness 0 and every pixel onto that segment; Boost
    0 is the identity and raises chroma and contrast; Fade 100 gives the input; each effect renders
    the same at two sizes after a downscale.
- ✅ **M12.10 `edit`: HDR Look, Dream Glow and Plastic Lens**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/looks.rs`, `crates/edit/tests/look_effects.rs`, `crates/edit/tests/fingerprints/looks-*.json`
  - scope: `editing.md` §8.3's blur-based looks on M12.3's blur, noise and blends: HDR Look
    (`out = in + C·(in − blur(r))` with Radius relative to the picture and Strength, with Fade),
    Dream Glow (a blurred copy blended in overlay by Bloom, a midtone lift by Brightness, with
    Fade), Plastic Lens (soft edges by Blur Edges, grain by Grain, a vignette, with Fade); each
    `VERSION` 1; large radii read only as far as their reach; rows `FX-T4`.
  - done when: `cargo test -p dalica-edit --test all look_effects::`: a fingerprint per effect at
    its defaults; invariants: each leaves a flat patch flat and gives the input at Fade 100; HDR
    Look raises local contrast; Plastic Lens leaves the centre sharper and the corners darker than
    the input; a zoomed region equals that window of the whole render; each effect renders the same
    at two sizes after a downscale.
- ✅ **M12.11 `edit`: Soften and its paint mask**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/soften.rs`, `crates/edit/tests/soften.rs`, `crates/edit/tests/fingerprints/soften-*.json`
  - scope: `editing.md` §8.4 and the decisions above: Soften (a soft glow blur by Softness, with
    Fade, default 50) applied where its mask says: the whole photo with no strokes, else the
    strokes' painted dabs (M12.3) minus the erased ones, in the stored frame, mapped through
    `Prepare::geometry`, at most 1 000 strokes, the brush radius relative to the shorter side as
    Retouch's; `VERSION` 1; rows `FX-T5`.
  - done when: `cargo test -p dalica-edit --test all soften::`: a fingerprint without strokes and
    one with a painted stroke; invariants: outside a painted stroke the photo is unchanged, an
    eraser stroke over it gives the photo back there, Fade 100 gives the input; a stroke on a photo
    turned 90° lands where its stored-frame points say; 1 001 strokes are refused by the parser
    (kept, skipped); the mask is the same at two sizes after a downscale.
- ✅ **M12.12 `edit`: Vignette, Pixelate and Focal Zoom**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/stylize.rs`, `crates/edit/tests/stylize_effects.rs`, `crates/edit/tests/fingerprints/stylize-*.json`
  - scope: `editing.md` §8.4: Vignette (edges darkened, or toward a colour, by Size and Strength,
    with Fade, repeatable; LightCraft `crates/pipeline/src/finish.rs` (`vignette`) at 0da998f2),
    Pixelate (square cells of Pixel Size relative to the picture, anchored in the stored frame,
    blended by the chosen mode, with Fade; PhotoCraft `crates/algo/src/stylize.rs` (`mosaic`) at
    e4b2868a), Focal Zoom (a radial zoom blur outside a sharp circle of Focal Size by Zoom Amount
    and Edge Hardness, with Fade; PhotoCraft `crates/algo/src/blur.rs` (`radial`) and
    `crates/algo/src/blur/radial.rs`); each `VERSION` 1; rows `FX-T5`.
  - done when: `cargo test -p dalica-edit --test all stylize_effects::`: a fingerprint per effect at
    its defaults; invariants: Vignette leaves the centre and darkens the corners, two are darker
    than one; Pixelate in Normal mode makes each cell one colour and the same cells at two sizes;
    Focal Zoom leaves its circle unchanged and blurs along the radius more than across it; Fade 100
    gives the input; a zoomed region equals that window of the whole render; each effect renders
    the same at two sizes after a downscale.
- ✅ **M12.13 `edit`: effects that change the picture's size: the renderer, Widescreen Film and Border**
  - after: M12.1, M12.3
  - files: `crates/edit/src/ops/canvas.rs`, `crates/edit/src/render.rs`, `crates/edit/src/render/**`, `crates/edit/tests/canvas.rs`, `crates/edit/tests/fingerprints/canvas-*.json`, `docs/data-model.md`
  - scope: the decisions above: a third kind of operation beside `Warp` and `Adjust`, run at its
    place among the effects: the size of its output for an input and where the input lands in it,
    and the pixels outside; `Geometry` knows every such entry, so `output_size`, `display_size`
    and the point maps (`stored_to_output`, `to_display` and their inverses) include them and a
    later effect's centre stays on the photo; region renders (the zoomed edit view) equal that
    window of the whole render; frames that would pass `MAX_PIXELS` or `MAX_EDGE` are skipped and
    kept; Widescreen Film (crop to 1.7:1, muted filmic colour, grain, 15 % black bars top and bottom
    while Letterbox is on) and Border (outer and inner bands of their colours and thicknesses, a
    corner radius up to an oval, a caption band), each `VERSION` 1; `data-model.md` §3.3 says how
    sizes and points pass a frame; rows `FX-T4`, `FX-T5`.
  - done when: `cargo test -p dalica-edit --test all canvas::`: a fingerprint per effect at its
    defaults; a Border's output is the photo grown by both bands on every side with the photo's
    pixels unchanged inside; Widescreen Film's output is 1.7:1 within a pixel with its bars, and
    without Letterbox has none; a crop added after a Border crops the photo and keeps the frame; a
    rotation after a Border turns the framed picture; the photo's centre maps to the frame's photo
    area and back; a region at 1:1 equals that window of the whole render; 1 000 Borders stay
    within the limits without a panic; every existing `render` test passes unchanged.
- ✅ **M12.14 `edit`: Drop Shadow and Museum Matte**
  - after: M12.13
  - files: `crates/edit/src/ops/mats.rs`, `crates/edit/tests/mats.rs`, `crates/edit/tests/fingerprints/mats-*.json`
  - scope: `editing.md` §8.4 on M12.13's size-changing kind: Drop Shadow (the photo over a
    background colour with its shadow at Distance and Angle, blurred by Blur, with Fade, the canvas
    grown to hold it; PhotoCraft `crates/compose/src/effects.rs` (`shadow_map`, `tent_kernel`,
    `offset`) at e4b2868a), Museum Matte (an outer and an inner mat of their colours and
    thicknesses with an inner shadow, repeatable); each `VERSION` 1; rows `FX-T5`.
  - done when: `cargo test -p dalica-edit --test all mats::`: a fingerprint per effect at its
    defaults; invariants: the canvas grows and the photo's pixels are unchanged inside; the shadow
    falls on the side its angle names and the far corner is the background colour; two Museum
    Mattes are larger than one; a region at 1:1 equals that window of the whole render; each effect
    renders the same at two sizes after a downscale.
- ✅ **M12.15 `ui`: the effect tabs' tile grids and one-click effects**
  - after: M12.2, M12.4, M12.5
  - files: `crates/ui/src/panels/edit/effects.rs`, `crates/ui/src/commands/effects.rs`, `crates/ui/tests/effect_tiles.rs`, `crates/ui/tests/fingerprints/edit-effects-*.json`, `docs/editing.md`
  - scope: `editing.md` §5 and §8.1: tabs 3–5 show their grids (three columns, at most four rows,
    no scrolling) in place of "No effects yet": each tile from M12.5's previews at the device
    scale, `gray 38` until it arrives, its label and tooltip, the "1-click" badge on Sepia, B&W,
    Warmify and Invert Colors; a click on a one-click tile applies it as one step of the photo's
    history (`edit.effect.apply`); a click on any other opens its panel (`edit.tool.open {tool:
    "effect", name}`); the grid drawn disabled while a tool is open; Ctrl+Shift+B (`edit.bw`)
    applies B&W in the edit view; `editing.md` §5 loses "No effects yet"; rows `FX-TILES`,
    `EDIT-TABS`, the `KEY-*` row of Ctrl+Shift+B.
  - done when: `crates/ui/tests/effect_tiles.rs` (headless, by id): tab 3 shows its 12 tiles in
    the spec's order with their labels, each `gray 38` until its preview arrives and then differing
    from the plain photo; Sepia's click is one step and the panel's Undo reads "Undo Sepia";
    Ctrl+Shift+B gives "Undo B&W"; Sharpen's click opens its panel and the grid is disabled until
    Cancel; tabs 4 and 5 show 7 and 8 tiles; at 900 × 600 each grid fits the panel; a fingerprint of
    each tab's grid at 1400 × 900.
- ✅ **M12.16 `ui`: the parameter panel: sliders, choices, swatches, the eyedropper, Fade and the live preview**
  - after: M12.2, M12.4, M12.5, M12.6, M12.7
  - files: `crates/ui/src/panels/edit/effect_panel.rs`, `crates/ui/src/panels/edit/effect_panel/**`, `crates/ui/src/commands/effect_session.rs`, `crates/ui/tests/effect_panel.rs`, `crates/ui/tests/fingerprints/effect-panel-*.json`
  - scope: `editing.md` §8.1: the panel in place of the grid for every parameter effect, built from
    `effect.list`: the title; up to four M12.4 effect sliders (`slider:effect.<param>`), the
    checkboxes (`check:effect.<param>`), Pixelate's blend mode (`choice:effect.<param>`), the
    swatches (`color:effect.<param>`) with their picker popup and eyedropper (a click on the photo
    runs `photo.effect.pick` and sets the colour), Fade where the effect has it; Apply │ Cancel;
    the session in `effect_session.rs`: every change sets the edit view's candidate recipe (the
    recipe plus the entry), Apply is one step of the photo's history with the entry
    (`photo.history.apply`), Cancel and Esc restore the preview and record nothing, Reset returns
    to the defaults; scripts set values through `edit.tool.action {name: "set", params}`, `pick`
    and `reset`; rows `FX-PANEL` (new), `FX-T3`, `FX-T4`, `FX-T5`.
  - done when: `crates/ui/tests/effect_panel.rs` (headless, by id): every parameter effect's panel
    opens with the controls its catalogue row lists; Tint's colour picked in the popup changes the
    preview and not the recipe, Apply records one "Tint" step with that colour, Cancel leaves the
    recipe and the preview as before; the eyedropper on a planted patch sets the swatch to its
    colour; Fade 100 shows the photo unchanged; Film Grain's Lighten and the slider's double-click
    reset work; at 900 × 600 the panel and the popup fit; fingerprints of Tint's, Film Grain's and
    Duo-Tone's panels and of the open picker at 1400 × 900.
- ✅ **M12.17 `ui`: the crosshair on the photo**
  - after: M12.8, M12.12, M12.13, M12.16
  - files: `crates/ui/src/panels/edit/crosshair.rs`, `crates/ui/tests/effect_crosshair.rs`, `crates/ui/tests/fingerprints/effect-crosshair-*.json`
  - scope: `editing.md` §8.1: while Soft Focus, Focal B&W, Graduated Tint or Focal Zoom is open,
    M12.4's crosshair over the photo (`crosshair:effect`): its centre dragged, the effect's size
    drawn as a circle, Graduated Tint's drag setting the start and the angle; points mapped between
    the display and the stored frame through the preview's `Geometry` (a turned, cropped or framed
    photo); each drag a change of the panel's session; the arrows nudge the centre; rows
    `FX-CROSSHAIR` (new), `FX-T3`, `FX-T5`.
  - done when: `crates/ui/tests/effect_crosshair.rs` (headless, by id): dragging Soft Focus's
    centre moves the sharp circle in the preview and Apply stores that point in the stored frame,
    the same on a photo turned 90°; Graduated Tint's drag sets its angle; on a photo with a Border
    the centre cannot leave the photo; fingerprints of Soft Focus's and Graduated Tint's crosshair
    at 1400 × 900.
- ✅ **M12.18 `ui`: Soften's brush and eraser**
  - after: M12.11, M12.16
  - files: `crates/ui/src/panels/edit/soften_brush.rs`, `crates/ui/tests/soften_brush.rs`, `crates/ui/tests/fingerprints/soften-brush-*.json`
  - scope: `editing.md` §8.4 and the decisions above: in Soften's panel the Paint and Erase buttons
    (`button:effect.paint`, `button:effect.erase`) and the brush size; on the photo the brush
    circle under the pointer, a drag adding a stroke in the stored frame (at most 1 000), + / = and
    − resizing the brush 1.2 × as Retouch's, the painted area previewed live; each stroke a change
    of the panel's session; Apply stores the strokes; rows `FX-T5`.
  - done when: `crates/ui/tests/soften_brush.rs` (headless, by id): with no stroke the whole photo
    previews softened; one painted stroke softens only under it; an erase stroke over it gives the
    photo back there; Apply records one "Soften" step whose strokes land where they were drawn on a
    photo turned 90°; a fingerprint of the brush over a painted stroke at 1400 × 900.
- ✅ **M12.19 `engine` + `ui`: Batch Edit's five effects**
  - after: M11.9, M11.18, M12.6, M12.7
  - files: `crates/engine/src/cmd/batch.rs`, `crates/engine/tests/batch_edit.rs`, `crates/ui/src/commands/batch.rs`, `crates/ui/tests/batch_ui.rs`
  - scope: `editing.md` §9: Batch Edit ▸ Sepia, Sharpen, Warmify, Film Grain and Black and White
    through `edit.apply` lose their "Arrives with Effects" state now that they render (Sharpen and
    Film Grain at their defaults), each appended to every photo's recipe as one library undo step;
    rows `EDIT-BATCH`, the Picture menu row.
  - done when: `crates/engine/tests/batch_edit.rs`: Sepia on 20 photos is one undo step and one
    `edit.undo` restores the 20 recipes; `crates/ui/tests/batch_ui.rs`: the five effects are live
    in Batch Edit ▸ and Black and White on 3 held photos gives each a `bw()` entry; `panic_hunt`
    green.
- ✅ **M12.20 `mcp`: the workflow W11, effects stacked**
  - after: M12.6, M12.12, M12.13
  - files: `crates/mcp/tests/workflows/w11_effects_stack.rs`, `xtask/src/stats.rs`, `docs/mcp.md`
  - scope: `mcp.md` §9: W11 (`apply_edit` Sepia, then Vignette, then Border on one photo: the
    recipe keeps that order, `render_photo` grows by the Border's bands, the render's fingerprint
    within tolerance) un-ignored; `list_edit_ops` lists the 27 effects as rendering; the floor in
    `xtask/src/stats.rs` raised to the twelve workflows now required; `mcp.md`'s status line; row
    `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp` runs W11 green; `cargo xtask stats`
    prints 12 / 12 passing and `stats --check` fails below that.
- ✅ **M12.21 `edit` + `engine` + `xtask`: M12's budgets: effect renders and tile grids**
  - after: M12.5, M12.6, M12.7, M12.8, M12.9, M12.10, M12.11, M12.12, M12.14
  - files: `crates/edit/benches/effects.rs`, `crates/engine/benches/effect_tiles.rs`, `xtask/src/bench.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: `development.md` §8: benches registered in `xtask/src/bench.rs`, joined to `BUDGETS` and
    the release-budgets job, each documented in §8 and §13 with its machine: `effects` (each of
    the 27 at its defaults: a draft and a final at the edit view's screen size, and a 24 MP final
    as Save renders it), `effect_tiles` (a tab's 12 tiles from opening the photo to the last
    tile); budgets at 1.5 × their first measurement, plus every effect's draft within 16 ms and
    final within 100 ms at p95 at screen size whatever the first numbers; a missed budget is fixed
    in this task (the effect's module asked of the lead; before and after numbers in the PR) or
    blocks with its numbers, never loosened.
  - done when: `cargo xtask bench effects` and `bench effect_tiles` print their numbers (in the
    PR); `cargo xtask budgets` passes with them and `--plant-ms` makes each fail.
- ✅ **M12.24 `ui`: the slideshow prepares the next photo off the UI thread**
  - after: M11.R
  - files: `crates/ui/src/panels/slideshow/show.rs`, `crates/ui/src/panels/slideshow.rs`, `crates/ui/tests/slideshow.rs`, `crates/ui/benches/slideshow.rs`
  - scope: a fix task from M11.R. M11.29 moved the next photo's texture upload out of the change's first frame into one
    frame of the display time, measured at 12.7 ms p50 and 22.4 ms p95 on two cores (the bench's
    `upload frame` row), so Pan and Zoom can hitch once per photo. Convert the preview to the
    texture's pixel format on a worker and leave the UI thread only the upload itself (or split a
    large upload over frames); memory stays at the current and next textures; the change frame
    keeps its M11.29 numbers; row `VIEW-SLIDESHOW`.
  - done when: the bench's upload frame is at most 8 ms p95 on the same two-core setup (before and
    after in the PR); the next photo's texture is still ready before its change starts and the
    30-change test with a 1.5 s planted decode reports 0 waits; `cargo xtask ci`.
- ✅ **M12.25 `ui` + docs: M11's small follow-ups**
  - after: M11.R, M12.2
  - files: `crates/ui/src/panels/edit/caption_bar.rs`, `crates/ui/src/control/**`, `crates/ui/tests/captions.rs`, `crates/fs/README.md`, `crates/ui-kit/README.md`, `crates/engine/README.md`, `docs/control-protocol.md`
  - scope: a fix task from M11.R: the follow-ups M11's tasks left outside their files. The edit view's caption-bar captions
    toggle shows whether captions are on (a selected look, from ui-kit's existing selected state).
    When automation drives the Import room, `import.run`'s destination is checked with links
    resolved (off the UI thread), as `engine.execute` already does, not by its path text alone.
    The fs README names `transfer::import_copy` and `remove`; the ui-kit README's public-API list
    names the room tabs, overlay bar, stepper and caption plate; the engine README's module table
    lists `import`, `import_run`, `variant`, `viewer` and the batch commands as built.
    `control-protocol.md` §6.5's `room.close` row names `confirm`.
  - done when: a test shows the toggle's look changes with the setting; a control test shows a
    destination reached through a link outside the write roots refused over automation; the docs
    read as built; `cargo xtask ci`.
- ✅ **M12.26 `engine` + `xtask`: the two budgets the M11.R review missed on GitHub**
  - after: M11.R
  - files: `crates/engine/benches/search.rs`, `crates/engine/benches/import.rs`, `crates/engine/src/search.rs`, `crates/engine/src/search/**`, `crates/engine/src/import.rs`, `crates/engine/src/import/**`, `crates/scan/src/import.rs`, `crates/scan/src/import/**`, `xtask/src/bench.rs`, `xtask/src/accept.rs`, `docs/development.md`
  - scope: a fix task from M11.R (the maintainer merged the review with these two open). On the review
    PR's Budgets job (run 39090240306), on a runner slower than usual (cold start 340 ms against 236 ms the run
    before): M9's lone `-word` search took 5.57 ms, 1.36 × v0.5.0's 4.09 ms measured in the same job (limit 1.3 ×),
    and M11's import listing took 346 ms against its absolute 320 ms (245 ms on the previous run, 209 ms on the
    lead's machine). Find out whether each is a regression or runner noise: measure both at v0.5.0 and on main
    several times on the Budgets job and locally (median of five). A regression is fixed in its module with
    before/after numbers. If it is noise, the budget is never loosened here: the task blocks with its numbers and
    the options (a relative budget for the import listing like save and search, more samples, or a different
    limit) for the maintainer at M12.R.
  - done when: both budgets pass on two consecutive Budgets runs on GitHub, or the task is blocked with the
    measurements and options; before/after numbers in the PR; `cargo xtask ci`.
- ✅ **M12.27 `ui` + `engine` + `edit`: a frame's grown size everywhere a size is read, and the effect docs as built**
  - after: M12.13, M12.14
  - files: `crates/edit/src/recipe.rs`, `crates/edit/tests/recipe.rs`, `crates/edit/src/ops/mod.rs`, `crates/edit/README.md`, `crates/ui/src/panels/thumbnails.rs`, `crates/ui/src/status_text.rs`, `crates/ui/src/panels/edit/filmstrip.rs`, `crates/ui/src/panels/viewer/strip.rs`, `crates/ui/src/dialogs/adjust_date.rs`, `crates/ui/tests/edited_cells.rs`, `crates/ui/tests/status_line.rs`, `crates/ui/tests/viewer.rs`, `crates/ui/tests/adjust_date.rs`, `crates/engine/src/cmd/photo.rs`, `crates/engine/src/cmd/inspect.rs`, `crates/engine/tests/library_inspect.rs`, `docs/parity.md`
  - scope: a fix task from M12.13 (#768). M12.13 made Border and Widescreen Film (and M12.14 Drop
    Shadow and Museum Matte) change the picture's size through `edit::render::Reframe`, and
    `edit::Geometry` includes the frames in `size`, `output_size`, `display_size` and the point
    maps (`photo_size()`, `photo_area()`, `frames()` are new), so everything that reads `Geometry`
    or renders through `edit::render` already shows the grown size (the edit canvas, the
    slideshow, the viewer, the engine's preview, Save, Export, the thumbnails' pixels). Seven
    readers still turn the file's size by the recipe's rotations and flips alone
    (`turn.display_size(w, h)`) and miss a frame: the thumbnail cell's shape and size
    (`panels/thumbnails.rs`, `recipe_orientation`), the status bar's dimensions (`status_text.rs`),
    the edit view's filmstrip (`panels/edit/filmstrip.rs`), the viewer's strip
    (`panels/viewer/strip.rs`), Adjust Date's preview (`dialogs/adjust_date.rs`),
    `photo.inspect`'s `displayed` and the Properties pane through `cmd::photo::displayed_size`, and
    `library.inspect`'s photo rows (`cmd/inspect.rs`, which give the file's `width`/`height` and
    `rotate` only: they gain the shown size as `displayed`, as `photo.inspect`'s, the params doc
    saying so). One helper in `edit` (beside `Recipe::display_size`, through `Geometry`) gives the
    shown size of a recipe on a `w × h` photo, a recipe `edit` cannot read counting as none as
    today; every reader calls it. Each reader keeps what it shows today for a photo without a frame
    (whether it counts a crop is the spec's call for that surface; record it in the commit body).
    Docs as built: the doc comment on `ops::Prepare` (its size is the canvas at the operation's
    place, frames included) and `crates/edit/README.md` (no longer calls the effects stubs:
    each live family's effects in one line, `render::Reframe`, `Soften::coverage`, the
    `Geometry` additions, the test files M12.6–M12.14 added). Rows `FX-T3`, `FX-T4`, `FX-T5` and
    the engine half of `FX-TILES`, which M12.6–M12.13 left because `parity.md` was not in their
    files: set as built when this task runs, each effect measured by its tests, never claimed.
  - done when: `crates/edit/tests/recipe.rs`: the helper gives a Border's grown size, turned by a
    later rotation, and the file's turned size without a frame; with a Border on a photo, the
    grown size shows in the thumbnail cell (`crates/ui/tests/edited_cells.rs`), the status bar
    (`crates/ui/tests/status_line.rs`), the filmstrip and the viewer's strip (`crates/ui/tests/viewer.rs`
    or each module's own `mod tests`), Adjust Date's preview (`crates/ui/tests/adjust_date.rs`),
    `photo.inspect` (the unit test in `cmd/photo.rs`) and `library.inspect`
    (`crates/engine/tests/library_inspect.rs`); the docs read as built; the parity rows updated
    and `cargo xtask parity --write` run; `panic_hunt` green; `cargo xtask ci`.
- ✅ **M12.28 `edit`: no one-time setup inside a render band waits on the pool it runs in (Soften, Heal, …)**
  - after: M12.7, M12.11
  - files: `crates/edit/src/ops/soften.rs`, `crates/edit/src/ops/retouch.rs`, `crates/edit/src/fx/blur.rs`, `crates/edit/src/fx.rs`, `crates/edit/tests/soften.rs`, `crates/edit/tests/retouch.rs`, `crates/edit/tests/fx.rs`, `NOTICE`
  - scope: a fix task from M12.7 (#776), M12.8 and M12.12. A pass that makes something once per
    render in a `OnceLock` reached from inside a band, whose initialiser runs `rayon` work, can
    hang the render: while it waits on its own parallel work, the initialising thread steals
    another band of the same pass, which blocks on the same lock (or on another render's lock
    whose maker waits on this one); at best every other band waits on it and the render runs
    serially. M12.8 avoided it in `ops/focal.rs` (no shared blur), M12.12 in `ops/stylize.rs`
    (Pixelate's cells and Focal Zoom's plane are made on plain scoped threads, `par_rows`), and
    M12.7 fixed `fx::Blurred` (the blur made on its own pool, `fx::blur::blur_pool`, from a plain
    scoped thread the band waits on; regression test `fx::blurs_shared_by_bands_never_wait_on_each_other`,
    which timed out at 30 s before). Two sites remain on `main`: Soften's `SoftenPass::make`
    (`ops/soften.rs`, in `done.get_or_init`) paints its mask with `par_chunks_mut` and blurs with
    `Gaussian::apply`, both `rayon`: M12.7's worker saw one `cargo xtask ci` hang in
    `soften::a_region_is_that_window_of_the_whole_render` with every worker stack waiting in
    Soften's one-time setup (intermittent; the rerun was green). Retouch's `Heal::heal`
    (`ops/retouch.rs`, in `done.get_or_init`) reads as sequential today (its own box blur), so it
    cannot re-enter but makes every other band wait while one thread heals: confirm by its test,
    and move it only if that is no slower. Route every such setup through one helper in
    `fx/blur.rs` that `Blurred` also uses (M12.7's mechanism made general: run the work on the
    blur pool from a thread that only waits), or make it before the banded pass when that needs no
    file outside this list; the output stays bit-identical, so the `soften-*` and `retouch`
    baselines do not change. The grep of `crates/edit/src` on `main` after M12.7
    (`OnceLock`/`get_or_init`/`Blurred`): `colour.rs` and `remap.rs` hold `static` tables built
    without `rayon` (safe), `detail.rs`'s Glow uses the fixed `Blurred`, `stylize.rs` is safe as
    above, `render.rs` has no one-time setup. M12.10 (`ops/looks.rs`) and M12.14 (`ops/mats.rs`)
    are open: once each merges, the worker reads it (read-only) and reports any one-time setup
    that runs pool work on its issue for a fix task, never editing it here. Also a `NOTICE` gap:
    Sharpen's kernel in `ops/detail.rs` follows PhotoCraft `crates/algo/src/blur.rs`
    (`gaussian_kernel`) @ e4b2868a, which `detail.rs`'s row does not list yet; add it there.
  - done when: per fixed site, one regression test that timed out before the fix and passes after
    it (two threads rendering that effect into one small shared pool, 2–4 threads, under a 30 s
    timeout, like M12.7's), in `crates/edit/tests/soften.rs` and `crates/edit/tests/retouch.rs`
    (Heal's test written either way, its before/after result in the PR); a region still equals
    that window of the whole render for Soften (everywhere and painted) and Heal; Soften and Heal
    render times at 12 MP (release, `Target::Full` and a 1:1 window, median of five) not worse,
    before/after in the PR; the `NOTICE` row; `cargo xtask ci`.
- ✅ **M12.29 `engine`: a size-changing effect's tile shows its whole picture**
  - after: M12.5, M12.13, M12.14
  - files: `crates/engine/src/effect_tiles.rs`, `crates/engine/tests/effects.rs`, `docs/editing.md`
  - scope: a fix task from the M12.R review screenshots. Every tile is rendered at the scale that
    covers the 78 × 48 box and cut around the centre (`render_tile`'s `max` scale and `cover`),
    which is right for an effect that keeps the picture's shape but crops away what a frame adds:
    the Border tile looks like the plain photo, and Drop Shadow's and Museum Matte's show their
    white edge at the left and right only (the grown 3:2 picture is taller than the 1.625:1 box,
    so its top and bottom are cut); Widescreen Film's 1.7:1 picture loses its sides. For an
    effect whose catalogue row says `changes_size` (Border, Drop Shadow, Museum Matte, Widescreen
    Film), the tile renders the whole grown picture at the scale that fits it in the box and
    centres it on `gray 38` (the placeholder grey of `editing.md` §8.1), so every side of the
    frame shows; every other effect keeps the covering scale and its cut, unchanged. The small
    decode stays large enough for the fitted render. `editing.md` §8.1 says so in one sentence.
    At their defaults a Border's bands stay thin at tile size (20 and 5 thousandths of the shorter
    side, about one tile pixel): that is the effect's default, not this task's.
  - done when: `crates/engine/tests/effects.rs`: a Border, a Drop Shadow and a Museum Matte tile
    each have their frame colour (the Border's outer colour, the Drop Shadow's background, the
    Museum Matte's outer mat) on all four sides of the picture inside the box, with `gray 38` only
    in the bars around it; a Widescreen Film tile shows its black bars at top and bottom; a tile of
    an effect that keeps the shape (Sepia) is the same pixels as before; `cargo xtask ci`.
- ✅ **M12.30 `ui-kit`: an effect tile's label fits its tile**
  - after: M12.4, M12.15
  - files: `crates/ui-kit/src/widgets/effect_tile.rs`, `crates/ui-kit/tests/effect_widgets.rs`, `crates/ui-kit/tests/fingerprints/effect-widgets*.json`, `crates/ui/tests/fingerprints/edit-effects-tab4-1400x900.json`, `docs/design-system.md`
  - scope: a fix task from the M12.R review screenshots. "Widescreen Film" is cut to
    "Widescreen …" on its tile at 1400 × 900 and 900 × 600: the label is laid out in SemiBold 11
    within the tile's width less 4 pt (82 pt, `effect_tile`'s `one_line`) and elided. The tile
    cannot grow (three 86 pt tiles and two 4 pt gaps already take 266 of the panel's 280 pt) and
    has room under the preview for one line only (69 − 51 pt). A label that does not fit at
    SemiBold 11 steps its size down by half points to at least 9 pt until it fits (the full tile
    width, 86 pt less 2, its room), and only a label that still does not fit is cut with "…".
    Every other label of the 27 keeps SemiBold 11, so tabs 3 and 5 do not change.
    `design-system.md`'s `effect_tile` row says so. The ui crate's tab-4 grid fingerprint changes
    with the label: run `cargo test -p dalica-ui --test all effect_tiles` (the task gate tests
    ui-kit only) and bless that baseline with its diff images, the why in the commit body.
  - done when: `crates/ui-kit/tests/effect_widgets.rs`: a "Widescreen Film" tile draws its whole
    label (no "…", inside the tile) and an 11 pt label ("Drop Shadow") is unchanged; a label
    too long even at 9 pt is still cut with "…"; the changed fingerprints blessed with their diff
    images on the PR; a 1400 × 900 and a 900 × 600 screenshot of tab 4 on the PR;
    `cargo xtask ci`.
- ✅ **M12.31 `ui-kit`: the CPU rasterizer leaves no gap between two triangles of a quad**
  - after: M12.27
  - files: `crates/ui-kit/src/raster.rs`
  - scope: a fix task from the M12.R review screenshots. A Border'd photo's cell in the Recently
    Updated album showed one dark pixel (10, 10, 10) inside the picture at (328, 241) at both
    window sizes, and the folder's cell of the same photo did not. Reproduced with
    `dalica-cli snapshot --demo-fresh` (a Border applied, back to the library): the pixel sits
    exactly on the diagonal of the album cell's image rect, (272, 203.23)–(416, 300.77) (the
    letterboxed 496 × 336 picture has fractional edges), it moves along the diagonal with the
    picture's shape (a Drop Shadow puts it at (375, 273)), keeps its colour whatever the frame's
    colour, and is the photo's drop shadow under the image showing through. Both cells draw the
    same texture; the CPU rasterizer that draws headless screenshots and fingerprints
    (`raster.rs`, `Target::triangle`) evaluates a shared edge in `f32` from each triangle's own
    end (`edge(a, b, p)` and `edge(b, a, p)` are not exact negatives), so a pixel centre within
    rounding of the diagonal can fail both triangles' tests and is painted by neither. The window
    (wgpu) has watertight rules and does not show it. Make the edge test watertight: evaluate each
    edge with its end points in one canonical order and negate for the other triangle (or snap the
    vertices to a fixed-point grid first), keeping the top-left rule so a shared edge's pixels are
    painted exactly once.
  - done when: a unit test in `raster.rs` paints a textured quad (two triangles, as egui
    tessellates an image) at the rect above over a contrasting fill and finds no pixel of the fill
    inside it, which fails before the fix; a seeded sweep of quads at fractional offsets paints
    every pixel centre inside each quad exactly once (no gap, no double blend); the ui-kit and ui
    fingerprint suites run (`cargo test -p dalica-ui-kit`, `cargo test -p dalica-ui --test all`)
    with no baseline moved beyond its tolerance, or the moved ones reported on the issue with
    their diff images (never blessed outside this task's files); `cargo xtask ci`.
- ✅ **M12.32 `edit`: Drop Shadow's defaults read at a glance**
  - after: M12.14
  - files: `crates/edit/src/ops/mats.rs`, `crates/edit/src/catalog.rs`, `crates/edit/tests/mats.rs`, `crates/edit/tests/fingerprints/mats-drop-shadow.json`, `docs/editing.md`
  - scope: a fix task from the M12.R review (the maintainer's answer to Q7). At today's defaults
    (`drop_shadow(#000000,4,90,10,30,#ffffff)`: Distance 4 and Blur 10 thousandths of the shorter
    side, 1.3 px and 3.2 px on a 480 × 320 photo) the shadow is one grey pixel column at the
    photo's right edge, and the effect reads as a plain white frame in the edit view, the tile and
    the thumbnail. The renderer is right (opacity `1 − Fade/100`, the shadow cast along Angle);
    only the defaults change: Distance 15 and Blur 25 thousandths of the shorter side (about
    4.8 px and 8 px at 480 × 320; the canvas margin grows with them to 65 thousandths), Angle 90,
    Fade 30, the colours and the ranges as they are. One place sets them: `DropShadow::default`
    in `ops/mats.rs` and the catalogue row's defaults in `catalog.rs` agree (the catalogue's
    tests already check every default prints back and renders the same). `editing.md` §8.4's
    Drop Shadow row and §8.5's entry at its defaults read the new numbers. A recipe written with
    the old defaults keeps its own numbers (they are its parameters, not defaults), so no saved
    photo changes. `docs/editing.md` is also in M12.29's files: the board runs the two one after
    the other.
  - done when: `crates/edit/tests/mats.rs`: at the defaults on a 480 × 320 picture the shadow
    darkens the background by at least 20 levels (of 255) 8 px beyond the photo's right edge
    (about 32 levels there by the tent's closed form), the same distance beyond the left edge is
    lighter than that (the shadow visibly offset, not a halo), and the far corners are the
    background; the `mats-drop-shadow` fingerprint re-blessed after looking at its diff images
    (the why in the commit body, images on the PR); `cargo xtask ci`.
- ✅ **M12.22 `accept M12`**
  - after: M12.14, M12.15, M12.17, M12.18, M12.19, M12.20, M12.21, M12.24, M12.25, M12.26, M12.27, M12.28, M12.29, M12.30, M12.31, M12.32
  - files: `tests/acceptance/m12.rs`, `tests/acceptance/m12_*.rs`, `tests/acceptance/m12.md`, `tests/acceptance/fingerprints/m12-*.json`
  - scope: every *Done when* of M12's three areas in the real `dalica --headless` over the control
    channel and through `dalica-cli mcp`: each tab's grid filling with previews; each of the 27
    effects applied from its tile or its panel as one history step named after it, surviving a
    restart and an index rebuild, shown on its thumbnail, Cancel recording nothing; a crosshair
    placed on a turned photo landing where it was put; Soften painted and erased; a Border's
    photo saved and exported at the grown size with the original kept in `.dalica-originals/`;
    Batch Edit ▸ Sepia on 20 photos as one undo step; W11 over MCP; the M12 budgets (the release
    budgets with `DALICA_ACCEPT_RELEASE=1`; inside `cargo xtask ci --full` a smaller debug run checks
    the paths and prints its times); whole-window fingerprints of the three effect tabs and an open
    panel; the manual checks in `m12.md` (every effect on a few real photos at Fit and 1:1, the
    colour picker and the eyedropper, the crosshair dragged with a mouse and a touchpad, Soften
    painted, a framed photo saved and opened in another application).
  - done when: `cargo xtask accept M12` passes everything in M12's *Done when*. Amended at M12.R
    (the maintainer): "the M12 budgets" are those M12.21 landed (every effect's 24 MP Save and the
    empty recipe, the effect tabs' tile grids, the slideshow's upload frame); a slider's per-effect
    16 ms draft and 100 ms final are M13's speed tasks, printed and not judged; the two checks by
    hand moved to M13.R.
- ✅ **M12.23 Lead: task blocks for M14**
  - after: M11.25
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M14 task block from M14's outline rows and the spec (`creating.md` §3 for
    the collage room, `data-model.md` §9 and §10.11 for the previous organizer's sidecars), with
    waves and the `M14.R` release gate (owner: maintainer; Release 0.9); a block that can port
    Craft-app code names the source files in its scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists the new
    M14 tasks as planned.
- ✅ **M12.R Release 0.7 gate**
  - after: M12.22
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 0.7): the lead runs `/milestone-review M12.R` to prepare it, with
    the version set to 0.7.0 and its `whats-new.md` section on the review branch, the release
    checklist and the sign-off question, and after the merge cuts and publishes 0.7.0
    (`releasing.md`); the check-in page (open a photo in the edit view, look through tabs 3–5 and
    their previews, apply each one-click effect and press Ctrl+Shift+B, open every parameter
    effect, move its sliders and Fade and Cancel once, pick a Tint and a Duo-Tone colour with the
    picker and the eyedropper, drag the crosshair of Soft Focus, Focal B&W, Graduated Tint and
    Focal Zoom, paint and erase Soften, stack Sepia, Vignette and Border and save, then Undo Save,
    export a framed photo, Batch Edit ▸ Sepia on 20 photos and one Ctrl+Z; written as steps for the
    maintainer to try); the manual checks carried from M10.R and M11.R (the Windows touchpad pinch,
    `EDIT-ZOOM`; the macOS build used by hand, `SYS-WINDOW`); the questions: the planning decisions above that change behaviour
    (opening an effect again appends, never edits the earlier entry; the eyedropper samples the
    photo before the effect; a crop made after a frame crops the photo and keeps the frame; Soften
    applies everywhere until the first stroke; Ctrl+Shift+B only in the edit view; the parameter
    ranges M12.1 decided), the look of the 27 effects, the tiles, the panels and the colour picker
    (approved with their fingerprints); *Where we stand* and the `status.md` numbers (the effects'
    draft and final times at p95, a tab's tiles, the workflow pass rate) on a `review/M12.R`
    branch, a pull request open. Only the maintainer merges that pull request and closes this
    issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M12.1 effect syntax + catalogue  ‖  M12.2 ui state + stubs  ‖  M12.3 effect primitives
         ‖  M12.4 ui-kit tiles, swatches, crosshair + core colour
         (once M11.R is closed)
wave 2   M12.5 effect list, tiles, pick  ‖  M12.6 colour effects  ‖  M12.7 sharpen, grain, glow
         ‖  M12.8 focal effects  ‖  M12.9 remaps  ‖  M12.10 looks  ‖  M12.11 soften
         ‖  M12.12 vignette, pixelate, focal zoom  ‖  M12.13 size-changing renderer + frames
wave 3   M12.14 drop shadow + museum matte  ‖  M12.15 tile grids  ‖  M12.16 parameter panel
         ‖  M12.19 batch edit effects  ‖  M12.20 W11
wave 4   M12.17 crosshair  ‖  M12.18 soften brush  ‖  M12.21 M12 budgets
         (fix task: M12.27 a frame's grown size everywhere and the effect docs, after M12.14)
         (fix task: M12.28 one-time setups that wait on their own pool, after M12.7, M12.11)
wave 5   M12.22 accept M12 → M12.R          (M12.23 lead: M14 blocks, any time after M11.25)
```

### M13 — People and places

One milestone in 3 feature areas (*Faces and the People group*, *Suggestions, the People pane and face regions*, *Places and geotagging*); one `cargo xtask accept M13` covers every *Done when* below. It ends with a release gate the maintainer owns (Release 0.8).

#### Faces and the People group

- **In:** local face finding (`faces.md`): the pure-Rust ONNX interpreter with its limits, the
  detector, alignment and embeddings, models installed from a local file or downloaded with the
  user's consent (the licence dialog, pinned size and SHA-256, the self-test; reserved decisions 5
  and 9), the background scan at the lowest priority whose pace follows the user, Find Faces
  Again; grouping of unnamed faces; the People group in the tree (Unnamed and one row per person
  with counts), the Unnamed face grid (clusters, Add a name, New Person, ignore, Show Ignored
  Faces, Expand Groups, faces dragged onto a person), person albums (person sections with Zoom to
  Faces / Show Whole Photos, Set as Person Thumbnail, Play), the face and person context menus
  (Rename, Merge Into, Delete Person); faces and people in the sidecars, `people.json` and the
  index; `person:` in the search and the Faces filter; Options ▸ People and the Folder Manager's
  Remove Faces question; faces in the crop suggestions and in the slideshow's Pan and Zoom.
- **Out:** the People Manager (`PPL-MANAGER`, P2); tiled detection for very small faces and GPU
  inference (`faces.md` §2, `development.md` §14); a person's Collage button (live with the
  collage room, M14); faces in videos (M15).
- **Reuse** (`architecture.md` §9): LightCraft `crates/faces` (`graph.rs`, `net.rs` and
  `net/**`, `runtime.rs`, `onnx.rs`, `yunet.rs`, `align.rs`, `manifest.rs`, `catalog.rs`,
  `known.rs`, `hash.rs`, `matching.rs`, `suggest.rs`, `synthetic.rs`), LightCraft engine
  `faces_index.rs`, `faces_worker.rs`, `face_tiles.rs`, `face_download.rs`, `cmd/face_models.rs`
  and `cmd/face_recognize.rs`, and LightCraft `crates/ui-egui/src/panels/faces.rs` (the licence
  dialog), `unnamed.rs` and `person.rs` as the pattern; LightCraft `crates/fetch` (`lib.rs`,
  `http.rs`) for downloads.
- **Done when:** `cargo xtask accept M13`: with a model installed, the face scan finds faces in the
  background and the window never waits on it; on `corpus/public-faces/` the detection and grouping
  floors hold (`faces.md` §10); naming a group names every face in it as one undo step and survives
  a restart and an index rebuild; removed and ignored faces never come back on a rescan; nothing is
  fetched before the licence is accepted; a full people workflow leaves no name, rectangle or
  embedding in a `trace` log.

#### Suggestions, the People pane and face regions

- **In:** suggestions (`faces.md` §5): "Name?" above the Suggestion confidence and ahead of the
  runner-up, Confirm, Confirm All, Reject (remembered), Find More, Show only suggestions, the "?"
  badge on a person's row, `faces.evaluate`; the People pane (`organizing.md` §6.4): the people in
  the selection, its unnamed faces and suggestions, hovering a face highlighting its box on the
  photo and back, Add a Person Manually in the edit view, several people filtering the light
  table; face boxes on the photo in the edit view; MWG face regions read on scan and written with
  Store people names in photos or Tools ▸ Experimental ▸ Write People Names to Photos…
  (`xmp-interop.md` §6); people over MCP (W14).
- **Out:** face data in exported copies beyond what the source file already holds (`faces.md` §7).
- **Reuse:** LightCraft `crates/faces/src/matching.rs` (best match and margin) and
  `cmd/face_recognize.rs` (`faces.suggest`, `faces.person`) as the pattern.
- **Done when:** `cargo xtask accept M13`: a suggestion appears only above the threshold and the
  margin, a confirmed face is never suggested again and a rejected pair never comes back; a face
  added by hand on a turned photo is stored in the stored frame and lands where it was drawn;
  merging, renaming and deleting a person are one undo step each; the MWG round-trip tests
  (`xmp-interop.md` §8, item 5) pass, and with Store people names in photos on a named face reaches
  the file and reads back; W14 passes over MCP.

#### Places and geotagging

- **In:** the geotag in the model and the sidecar (`"geo"`), imported from the file's GPS on scan
  and written back to it (EXIF GPS and its XMP mirror, under Write captions, tags and places) or
  cleared; the Places pane (`organizing.md` §7): the map of the photos of the current folder,
  album or selection, markers with their counts and bubbles, zoom, the pin tool, drag photos onto
  the map to geotag them, "Search for a place:", the confirmations; Tools ▸ Geotag ▸ Clear
  Location…, Copy Location and Paste Location; opt-in map tiles and the geocoding service
  (reserved decisions 4 and 9) with Options ▸ Network; an offline map (coastlines and a graticule)
  when tiles are off; geotags over MCP (`set_metadata {geo}`, W13).
- **Out:** reverse geocoding (place names from coordinates) and offline place-name search (not in
  the spec); GPS tracks.
- **Reuse:** LightCraft `crates/fetch` for the tile and geocoder requests; Natural Earth 1:110m
  land (public domain) for the offline map.
- **Done when:** `cargo xtask accept M13`: dragging photos onto the map geotags them as one undo
  step that survives a restart and a rebuild; the GPS round-trip: the place reaches the file and
  reads back within 10⁻⁶°, and Clear Location removes the GPS position and its XMP mirror; no
  network connection is made without the user's action (start-up, scans and the Places pane with
  tiles off make none; tiles are fetched only while the pane is open and allowed); W13 passes over
  MCP.

#### M13 tasks

M13 finds and names people and puts photos on a map: the faces engine (an ONNX interpreter, the
detector, embeddings and grouping), consented model downloads, the face scan, the People group,
the Unnamed grid, person albums, suggestions, the People pane, MWG face regions, geotags in the
model and the files, the Places pane and its map, and opt-in map tiles. Its first wave lists the
M12 work each task extends and starts once the maintainer has closed M12.R. Eight tasks lay the
ground the others share, so later tasks never share a file: M13.1 gives `core` the coordinate
maths (`core::geo`) and `library` faces, people and geotags with their ops and queries; M13.2
gives `sidecar` the `"faces"`, `"people"` and `"geo"` fields; M13.3 registers one engine command
module per area with stub state, M13's settings and the People and Network rows of the options
catalogue; M13.4 adds M13's view state, UI commands with stub handlers and the stub panes, grids
and dialogs; M13.5 gives `ui-kit` the face tile, the face box, the suggestion marks and the
icons; M13.6 and M13.7 give `faces` its interpreter and its grouping and suggestion maths (one
needs no model, the other no network); M13.8 gives `fetch` consented, resumable, hash-pinned
downloads over a transport it does not choose. The store (M13.9), the detector and embeddings
(M13.10), MWG regions in `meta` (M13.11), the map widget (M13.12) and the HTTPS transport (M13.13)
follow; then the engine's places (M13.14), people (M13.15), models (M13.16), scan (M13.18), face
regions (M13.19), tiles (M13.20) and suggestions (M13.26), each in its own module, and the window's
grids, panes and dialogs on top. Each task updates its `parity.md` rows (`PPL-DETECT`,
`PPL-UNNAMED`, `PPL-SUGGEST`, `PPL-PANE`, `PPL-XMP`, `PPL-CROP`, `GEO-PANE`, `GEO-WRITE`,
`PREF-PEOPLE`, `PREF-NETWORK`, the People and Places parts of `LIB-PANES`, `LIB-TREE-GROUPS`,
`LIB-TREE-DND`, `LIB-LT-SECTIONS`, `LIB-LT-HEADER`, `LIB-SEL`, `LIB-REGIONS` and `LIB-LT-BADGES`,
Find Faces Again in `LIB-CTX-PHOTO`, the faces pass of `ORG-FIRSTRUN`, the Remove Faces question of
`ORG-FOLDERMGR`, the Faces filter of `SRCH-FILTERS`, `person:` in `SRCH-LIVE`, faces, people and
places in `DATA-SIDECAR` and `DATA-META`, Pan and Zoom in `VIEW-SLIDESHOW`, `SYS-MCP`, the Picture,
Tools and View menu rows) and names its commands there; first fingerprints of the People group,
the Unnamed grid, person sections, the People pane, the face boxes, the Places pane and its map,
the dialogs and the new widgets and icons are approved in bulk on the M13.R review page.

Whole-window baselines belong to no task, as in M5–M12: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m12-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.
Fingerprints are made at 1400 × 900 only; at 900 × 600 a test asserts that the grids, panes and
dialogs fit (`architecture.md` §11).

Decisions taken while planning (the check-in lists them for reversal). **The reserved decisions.**
Reserved decisions 4 (map tiles: provider, and whether on by default), 5 (face models and their
licences) and 9 (the TLS stack) are asked at M12.R, as M11.25 said, never guessed here. The tasks
that hinge on them list M12.R and build what the recorded answer says: M13.10 (the detector and the
built-in model list: 5), M13.13 (the transport: 9), M13.16 (downloads: 5 and 9), M13.17 (the corpus
floors and calibration: 5) and M13.20 (tiles and the geocoder: 4 and 9); a worker who finds the
answer missing from *Reserved decisions* runs `tasks block` with the question and takes another
task. Everything else is built and tested without a model or the network: seeded synthetic
networks, an injected transport, a loopback listener that counts connections. If the answer is
that no network feature ships in 0.8, faces still work with a model added from a local file and
Places with the offline map. **Storage** (`data-model.md` §2–§4): a face is a `"faces"` entry of
its photo (`rect` `[x0, y0, x1, y1]` normalised in the stored frame of reserved decision 11, and
`person`, a person id or `ignored`; at most 256 per photo); a geotag is `"geo": [lat, lon]` with 6
decimals; a folder's sidecar mirrors the names of the people present in it (`"people"`) and
`people.json` in the library folder holds every person (id, name, nickname, thumbnail face) and the
rejected suggestions; embeddings, landmarks and which photos were searched live in one cache file
per model beside the index (named by the model's SHA-256, like the sketch file: rebuildable, a torn
tail dropped), never in a sidecar. A person's name is unique ignoring case: typing an existing name
assigns to that person, a new one asks New Person. **Undo:** naming a group, ignoring, moving or
removing faces, a face drawn by hand, renaming, merging and deleting a person (whose faces go back
to Unnamed) and geotagging, moving or clearing places are each one library undo step (one step of
the photo's own history in the edit view, as rotations are); Confirm All is one step. **Grouping**
is recomputed from the cached embeddings and is never user data: clusters are formed in photo
order so the Unnamed grid comes back the same after a restart, largest first; a background pass
merges clusters whose centroids meet the threshold. Calibration tables of the built-in models are
measured on `corpus/public-faces/` (M13.17); until then a model uses `matchCosine + (value − 85) ×
0.01`. **The scan** runs only while Find faces in my photos is on and a recogniser is installed,
follows the library's new and changed photos (not the scan code), never on the owner thread, at the
lowest job priority, with the window reporting the user's input to the session so the pace follows
`faces.md` §4; the People group's header reads "Finding faces… 312 of 4,980" while it runs. Face
tiles (the grid, the tree, the pane) are cut from the thumbnail ladder at the smallest cached size
covering twice the face as shown, kept in memory only, never in the thumbnail cache. Models never
enter the repository or the release packages; the corpus tests get them as a third-party pinned set
(`face-models`, fetched from their upstreams by `cargo xtask corpus` like PngSuite, never copied
into `dalica-corpus`), and the benches use a seeded synthetic network of the chosen recogniser's
shape. **Places:** the geotag lives in the model, imported from the file's GPS on the first scan
and synced by `xmp-interop.md` §5 as the other owned fields are; the geotag badge, the Geotagged
filter, Export's Remove location and `photo.inspect` read it. The map is Web Mercator (zoom 0–19,
256-pixel tiles); without tiles it draws Natural Earth's 1:110m land (public domain, compiled in
with its `ATTRIBUTION.md` row) and a graticule, so geotagging by drag works offline; markers closer
than 32 points on screen are drawn as one with a count. **The network** is used only after the
user acts: a model download only after the licence box is ticked, tiles only once Allow map tiles
is on and only while the Places pane is open (the pane asks once: "Show map tiles from
%provider%? Dalica fetches map pictures from the internet while this pane is open." [Allow]
[Not Now]), a place search only on Enter; nothing at start-up, during scans or from automation
without those settings. Tiles are cached under `<cache>/tiles/` (200 MB, oldest first, only files of
its own naming shape deleted); a tile is at most 1 MiB and 1 024 pixels, a geocoder reply at most
1 MiB and depth 32 (`security.md` §4.2). The Network tab's proxy is "system" (the `HTTPS_PROXY` and
`NO_PROXY` environment variables on every OS) or "manual" (host and port, `CONNECT` only, no proxy
authentication). Typed coordinates (decimal or degrees, minutes and seconds) are found without the
network. **Over automation** people data stays withheld without `--automation-allow-people`
(`control-protocol.md` §6.4), embeddings are never returned, and `faces.models.download` and
`.install` need `acknowledged: true`, which agents never set on their own (`faces.md` §9). Ports
from the Craft apps get their `NOTICE` rows from M13.6, which writes one row per port named in the
blocks below, so the porting tasks never share `NOTICE`; a port not named here asks the lead for
its row. M13's budgets join `development.md` §8 at 1.5 × their first measurement, plus the frame
time while the face scan runs (within 16 ms at p95), and are never loosened. Apart from the
reserved decisions and the go-ahead to publish the face corpus (both asked at M12.R), no step of
M13 needs a maintainer decision mid-run.

**Effect speed** (the maintainer at M12.R). The spec caps a slider's draft at 16 ms and its final
at 100 ms at p95 (`development.md` §8: two cores, a 24 MP photo, the 2560 × 1600 screen); at
M12.21's first measurement 13 drafts and 15 finals missed them, and 0.7 shipped with "effects slow
at large sizes" as a known issue. M13.34–M13.43 make every effect meet both caps, one task per
`edit::ops` module that misses, so their `files` never overlap: M13.34 first makes the shared
`fx` primitives fast (the box passes, a coarse-grid blur made once per render, noise by rows, the
linear-light tables), M13.35 makes the engine render a frame's draft from the halved base instead
of the fractional resample, the seven module tasks (M13.36–M13.42) use them, and M13.43 turns the
caps into budgets once all pass. The rules for all of them: the speed comes from the algorithm
and the loops (work at the blur's own scale on a grid anchored to the whole output, compute once
per render rather than once per band, tables for per-pixel curves, rows instead of per-pixel
`f64` maps, flat slices LLVM can vectorise), never from `unsafe`, a C dependency, a GPU path
(`development.md` §14) or a smaller draft than ¼ (`MIN_DRAFT_SCALE` stays 0.25); a render's
output stays what it was within the effect's fingerprint tolerance (0.5) and the two-sizes test's
(2.0), and a region render still equals that window of the whole render exactly. Before changing
an effect, its task blesses a `<case>-large` fingerprint of the current code at 1200 × 800 (large
enough that the grids and the fast paths are taken; the 320 × 240 fingerprints are not), in its
own commit, and the speed commit must match it without re-blessing. A look change only when no
lever reaches the cap without one: then the task re-blesses with the diff images on the PR and
the reason in the commit body, and the review asks about it. A cap no lever reaches is never
loosened: the task blocks with its before and after numbers and the options. Every task reports
the effect's draft, final and Save numbers before and after (`taskset -c 0,1 cargo bench -p
dalica-edit --bench effects -- --only <ids>`); a Save median never rises above its M12.21 budget.
The workstation is faster than the review's two-vCPU runner, so aim for about 12 ms and 75 ms.

- ✅ **M13.1 `core` + `library`: places, faces and people in the model**
  - after: M9.8, M10.29, M12.R
  - files: `crates/core/src/geo.rs`, `crates/core/tests/geo.rs`, `crates/library/src/model.rs`, `crates/library/src/op.rs`, `crates/library/src/people.rs`, `crates/library/src/query.rs`, `crates/library/src/search.rs`, `crates/library/src/search/**`, `crates/library/tests/people.rs`, `crates/library/tests/places.rs`, `crates/library/README.md`
  - scope: `core::geo`: a validated `LatLon` (finite, on the globe), decimal and
    degrees-minutes-seconds parse and print, great-circle distance, Web Mercator and the tile
    maths (a point ↔ world pixels at a zoom, the tiles covering a box, clamped at ±85.05°); in
    `library`, per the decisions above: `PhotoInfo` gains `faces` (rectangle in the stored frame,
    person or ignored; at most 256) and `geo`; the model gains the people (id, name unique ignoring
    case, nickname, thumbnail face); ops with inverses: `SetFaces`, `SetGeo`, `AddPerson`,
    `RenamePerson`, `MergePeople` (faces move, the name of the target kept), `DeletePerson` (its
    faces to Unnamed), `SetPersonThumbnail`; queries: a person's photos (the person album) in date
    order, Unnamed faces, counts per person, people sorted by name or count, photos with faces, the
    geotagged photos (the file's GPS or the model's geotag) and the geotags of a set of photos;
    the search grammar's `person:` matches names and nicknames (it matched nothing until now) and
    the Faces filter photos with a named or unnamed face.
  - done when: `crates/core/tests/geo.rs`: decimal and DMS round-trip, `NaN` and out-of-range
    refused, a tile's corners map back to their points, the tiles covering a box across the
    antimeridian; `crates/library/tests/people.rs`: each op and its inverse restore the model,
    merging two people moves every face and deleting one sends its faces to Unnamed, two names
    equal ignoring case are refused, `person:ana` finds Ana's photos and the Faces filter photos
    with faces; `crates/library/tests/places.rs`: a geotag set, moved and cleared with undo, the
    geotagged query takes the model's geotag over the file's; every existing `library` test
    passes unchanged.
- ✅ **M13.2 `sidecar`: faces, people and geotags in `.dalica.json`**
  - after: M9.6, M12.R
  - files: `crates/sidecar/src/fields.rs`, `crates/sidecar/src/print.rs`, `crates/sidecar/src/lib.rs`, `crates/sidecar/tests/fields.rs`, `crates/sidecar/README.md`
  - scope: `data-model.md` §3: a photo's `"faces"` (`rect` and `person`; at most 256, extra
    entries kept and ignored) and `"geo"` (`[lat, lon]`, printed with 6 decimals), the folder's
    `"people"` (`{id: {name, nickname?}}`); hostile values (a rectangle outside the photo, inverted
    or not finite, a bad person id, a latitude past 90) keep the entry and skip the value, as
    every field does; unknown keys inside a face kept verbatim.
  - done when: `crates/sidecar/tests/fields.rs`: faces, people and a geotag print and parse back
    equal; the sorted, pretty output is byte-stable; each hostile value is skipped with the rest
    of the photo kept; unknown keys in a face survive a rewrite; every existing sidecar test passes
    unchanged.
- ✅ **M13.3 `engine`: M13's command modules, settings and options rows**
  - after: M11.1, M12.5, M12.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/people.rs`, `crates/engine/src/cmd/faces.rs`, `crates/engine/src/cmd/face_scan.rs`, `crates/engine/src/cmd/face_models.rs`, `crates/engine/src/cmd/face_regions.rs`, `crates/engine/src/cmd/suggest.rs`, `crates/engine/src/cmd/places.rs`, `crates/engine/src/cmd/map.rs`, `crates/engine/src/faces_scan.rs`, `crates/engine/src/faces_index.rs`, `crates/engine/src/face_models.rs`, `crates/engine/src/face_tiles.rs`, `crates/engine/src/suggestions.rs`, `crates/engine/src/places.rs`, `crates/engine/src/map_tiles.rs`, `crates/engine/src/session.rs`, `crates/engine/src/jobs.rs`, `crates/engine/src/options.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/settings.rs`, `crates/engine/tests/options_catalogue.rs`, `crates/engine/README.md`, `docs/menus-and-shortcuts.md`, `docs/data-model.md`
  - scope: the engine's ground for M13 so M13.14–M13.26 never share a file: empty `cmd/people.rs`,
    `faces.rs`, `face_scan.rs`, `face_models.rs`, `face_regions.rs`, `suggest.rs`, `places.rs` and
    `map.rs` joined into the registry once; stubs `faces_scan.rs`, `faces_index.rs`,
    `face_models.rs`, `face_tiles.rs`, `suggestions.rs`, `places.rs` and `map_tiles.rs`, each a
    field of `Session` once, with `Session::user_activity` (the window's input, for the scan's
    pace) and `Session::set_places_open`; `JobKind::Network`; every M13 setting with its default
    behind a typed accessor: People (find faces on, suggest names on, suggestion and grouping
    confidence 85 in 50–95 by 5, store names in photos off, the model in use), Network (allow map
    tiles off, the tile server template, its attribution, the geocoding service, proxy
    `system | manual` with host and port); the options catalogue
    gains the People and Network tabs' rows (what waits on a reserved decision shown unavailable
    with its reason); `menus-and-shortcuts.md` §1.9 lists the new commands (`faces.*`,
    `people.*`, `photo.geo.*`, `geo.*`, `places.*`, `map.*`) and §5's People and Network tabs say
    what this build offers; `data-model.md` §4 lists the new settings keys.
  - done when: `crates/engine/tests/settings.rs` round-trips every new setting with unknown keys
    kept and wrong types or out-of-range values falling back (reported);
    `crates/engine/tests/options_catalogue.rs` covers the People and Network tabs, every option
    once, each default equal to its accessor's; every existing engine test and `panic_hunt` pass
    unchanged.
- ✅ **M13.4 `ui`: M13's view state, UI commands and the stubs**
  - after: M12.2, M12.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/menu_catalog.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/drag.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/people.rs`, `crates/ui/src/commands/faces.rs`, `crates/ui/src/commands/people_pane.rs`, `crates/ui/src/commands/suggestions.rs`, `crates/ui/src/commands/face_model.rs`, `crates/ui/src/commands/places.rs`, `crates/ui/src/commands/map.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/side_pane.rs`, `crates/ui/src/panels/faces.rs`, `crates/ui/src/panels/faces/**`, `crates/ui/src/panels/people_pane.rs`, `crates/ui/src/panels/people_pane/**`, `crates/ui/src/panels/places.rs`, `crates/ui/src/panels/places/**`, `crates/ui/src/panels/edit/faces.rs`, `crates/ui/src/panels/light_table/suggestions.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/new_person.rs`, `crates/ui/src/dialogs/people.rs`, `crates/ui/src/dialogs/face_model.rs`, `crates/ui/src/dialogs/geotag_confirm.rs`, `crates/ui/src/dialogs/options.rs`, `crates/ui/src/dialogs/options/people.rs`, `crates/ui/src/dialogs/options/network.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state.rs`, `docs/control-protocol.md`
  - scope: every M13 field of `UiState` in one go: the Unnamed view (expanded groups, ignored
    shown, the face selection, the name being typed per group); a person section's zoom to faces;
    the People pane (selected people, the face hovered, the manual box being drawn and its name);
    the Places pane (centre, zoom, the pin and its place, the open bubble, the search text and
    results, the drop under way); the face model dialog (the file or catalogue entry, the box
    ticked, the download's progress); UI commands with stub handlers, so M13.21–M13.30 never share
    a file: `people.*` (rename, merge, delete, thumbnail, sort) in `commands/people.rs`,
    `faces.*` (name a group, ignore, move, remove, show ignored, expand) in `commands/faces.rs`,
    the pane's `people.filter` and `faces.add` in `commands/people_pane.rs`, `faces.confirm`,
    `.reject`, `.confirmAll`, `.findMore`, `.onlySuggestions` in `commands/suggestions.rs`, the
    model dialog's in `commands/face_model.rs`, `places.*` (pin, drop, bubble, Show These Photos,
    Remove Location) and `geo.clear`, `.copy`, `.paste` in `commands/places.rs`, `map.*` (allow,
    zoom, pan, search) in `commands/map.rs`; a faces payload for drags (`drag.rs`); the side pane
    routing People and Places to stub panes; stub panels `faces.rs` (and its folder),
    `people_pane.rs`, `places.rs` (and their folders), `edit/faces.rs`,
    `light_table/suggestions.rs`; stub dialogs New Person, the person dialogs (rename, merge,
    delete), the face model dialog, the geotag confirmations and the Options tabs People and
    Network; the Picture, Tools and View menu items and the face and person context menus wired to
    their commands (greyed until live); control stubs for the new state; `control-protocol.md`
    §6.5 and §7 (the new state and ids: `face:<id>`, `group:<n>`, `field:faces.name/<n>`,
    `map:places`, `marker:<n>`, `button:places.pin`, `field:places.search`, `popup:places.bubble`,
    `region:people`, `region:places`, the dialogs' ids).
  - done when: `crates/ui/tests/view_state.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); `pane.people` and `pane.places` open
    the stub panes; the new menu items are listed and greyed; every command's graceful-failure
    test; `panic_hunt` and `widget_ids` stay green.
- ✅ **M13.5 `ui-kit`: face tiles, the face box, suggestion marks and the icons**
  - after: M12.4, M12.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/face_tile.rs`, `crates/ui-kit/src/widgets/face_box.rs`, `crates/ui-kit/src/widgets/suggest_marks.rs`, `crates/ui-kit/src/widgets/badges.rs`, `crates/ui-kit/src/theme.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/people.rs`, `crates/ui-kit/tests/face_widgets.rs`, `crates/ui-kit/tests/icons_people.rs`, `crates/ui-kit/tests/fingerprints/face-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-people*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `design-system.md` §4–§6 and `organizing.md` §6: `face_tile` (a square face picture or
    a placeholder, selected, hovered and dragged states, an optional name or "Name?" label, an
    optional × at its top-right and ✓ / × marks under it; registers `face:<id>`); `face_box` (the
    `face_box` token's rectangle over a photo with an optional label, highlighted on hover; in
    edit mode eight handles and a move drag kept inside a given area, reporting its rectangle in
    its own normalised coordinates); the ✓ and × marks (`suggest_marks.rs`) and the caution "?" badge (`badges.rs`); the
    icons it lacks, our own drawings on the 20-unit grid: Person, People, Unnamed, Ignored face,
    Add person, Places, Map pin, Marker, Pin tool, Map zoom in and out; the widget gallery gains
    them; `design-system.md` §6's rows.
  - done when: `crates/ui-kit/tests/face_widgets.rs`: a tile shows its placeholder until given a
    picture, its × and marks only when asked, its selection on a click; a face box dragged by a
    corner stays inside its area and reports the new rectangle; `crates/ui-kit/tests/icons_people.rs`:
    each new icon stays in its grid at 12, 16, 20 and 28 px in rest, hover and disabled colours;
    gallery and icon fingerprints; `widget_ids`, contrast, `cargo xtask assets` and `cargo xtask
    names` pass.
- ✅ **M13.6 `faces`: the pure-Rust ONNX interpreter and its limits**
  - after: M12.R
  - files: `crates/faces/src/graph.rs`, `crates/faces/src/graph/**`, `crates/faces/src/net.rs`, `crates/faces/src/net/**`, `crates/faces/src/onnx.rs`, `crates/faces/src/runtime.rs`, `crates/faces/src/limits.rs`, `crates/faces/src/synthetic.rs`, `crates/faces/tests/inference.rs`, `crates/faces/fuzz/**`, `crates/faces/README.md`, `.github/workflows/fuzz.yml`, `docs/security.md`, `NOTICE`
  - scope: `faces.md` §2: a bounded reader of small ONNX files (nodes, attributes, float32 weights;
    external or sparse weights, custom domains, training graphs and other element types refused by
    name) and a single-threaded float32 CPU interpreter of the listed operators (Conv with
    depthwise groups, BatchNormalization, PRelu, Relu, Sigmoid, Add, Sub, Mul, Dropout as identity,
    Flatten, Gemm, MaxPool, nearest whole-number Resize, Transpose, constant-shape Reshape,
    GlobalAveragePool, Identity), convolutions by a bounded im2col and `matrixmultiply` (no BLAS, no
    threads of its own); the limits checked before anything runs (file ≤ 512 MiB, weights ≤ 64 Mi
    elements, rank ≤ 8, live tensors and scratch ≤ 512 MiB, every weight and activation finite);
    seeded synthetic networks for tests (`synthetic.rs`); ported from LightCraft `crates/faces`
    (`graph.rs`, `net.rs`, `net/kernels.rs`, `net/program.rs`, `net/shape.rs`, `onnx.rs`,
    `runtime.rs`, `synthetic.rs`) at f34c7a8a, each file with its header; a fuzz workspace
    `crates/faces/fuzz` (`model`: any bytes as a model, loaded and, when it loads, run on a fixed
    input) and its job in the Fuzz workflow; `security.md` §4.2 (the model limits) and §7.1 (the
    target); the `NOTICE` rows of every port named in M13's blocks.
  - done when: `crates/faces/tests/inference.rs`: each operator on a seeded network equals a
    direct computation within 1e-5, PReLU broadcasts, grouped convolutions and padding are right,
    two threads running one model agree; malformed, truncated, oversized and unsupported files fail
    to load with their reason and never panic; a weight or activation that overflows is an error;
    `cargo +nightly fuzz build` builds the target; `cargo xtask deps` passes.
- ✅ **M13.7 `faces`: grouping and suggestions**
  - after: M12.R
  - files: `crates/faces/src/cluster.rs`, `crates/faces/src/matching.rs`, `crates/faces/src/calibration.rs`, `crates/faces/src/evaluate.rs`, `crates/faces/tests/grouping.rs`
  - scope: `faces.md` §5 as pure maths on unit embeddings, no model needed: the confidence →
    cosine threshold (a calibration table of one value per step 50 … 95, or `matchCosine + (value −
    85) × 0.01`); incremental grouping (a face joins the most similar centroid at or above the
    Grouping threshold, else starts a group; a merge pass for centroids that meet it; groups
    largest first, faces closest to the centroid first; deterministic in photo order);
    suggestions (a person's score is the best match over at most 64 of their confirmed faces,
    most recent and most varied; "Name?" only at or above the Suggestion threshold and ahead of
    the runner-up by 0.05; confirmed faces never suggested; rejected pairs skipped); Find More
    (above 80 % of the threshold, most similar first); `evaluate` (each named face hidden in turn,
    shots within a few seconds of it ignored, suggestions made and right per step); embeddings
    quantised to 8 bits in memory with the float32 kept for scoring ties; ported from LightCraft
    `crates/faces/src/matching.rs` at f34c7a8a (best match and margin).
  - done when: `crates/faces/tests/grouping.rs` (synthetic embeddings around seeded centres):
    thresholds rise with the slider value; three planted people form three groups at 85 and merge
    at 50; the margin holds back a near tie; a confirmed face is never suggested and a rejected
    pair never returns; the 64-face cap keeps a person with 10 000 faces cheap; the order is the
    same on every run; proptest: no input (`NaN`, zero vectors, mixed dimensions) panics.
- ✅ **M13.8 `fetch`: consented, resumable, hash-pinned downloads**
  - after: M12.R
  - files: `crates/fetch/src/lib.rs`, `crates/fetch/src/spec.rs`, `crates/fetch/src/transport.rs`, `crates/fetch/tests/download.rs`, `crates/fetch/README.md`
  - scope: `faces.md` §3.1 and `security.md` §8.2, whatever the transport: a file spec (https URL,
    a local name independent of the URL, a size cap, an optional pinned size and SHA-256) checked
    before anything is fetched; a `Consent` value that only a caller holding the user's
    acknowledgement can make, required by every download; a `Transport` trait (a ranged GET
    answering status, length and a byte stream) chosen by the caller, plus an in-memory transport
    for tests; resume of a pinned `.part` file, the size and hash checked before the rename (all
    writes through `fs`), a wrong one deleted; progress, cancellation between reads, connect and
    stall timeouts; a generic User-Agent and no cookie, token or personal header; ported from
    LightCraft `crates/fetch/src/lib.rs` at f34c7a8a, without its HTTP code.
  - done when: `crates/fetch/tests/download.rs` (the in-memory transport): nothing is requested
    without a `Consent`; a download completes, a cut one resumes from its `.part` and verifies; a
    wrong size or hash deletes the file and says so; a reply past the cap stops at the cap;
    cancellation stops at the next read; no request carries an identifying header; hostile specs
    (an http URL, a name with a separator, a zero or huge cap) are refused without a panic.
- ✅ **M13.9 `store`: faces, people and geotags on disk, and the embeddings cache**
  - after: M13.1, M13.2
  - files: `crates/store/src/mapping.rs`, `crates/store/src/people.rs`, `crates/store/src/faces_cache.rs`, `crates/store/src/rebuild.rs`, `crates/store/src/snapshot.rs`, `crates/store/tests/fields.rs`, `crates/store/tests/people.rs`, `crates/store/tests/faces_cache.rs`, `crates/store/README.md`, `docs/data-model.md`
  - scope: `data-model.md` §2–§4 and the decisions above: the model ↔ sidecar mapping of faces,
    geotags and the folder's people (a folder mirrors the people present in it; names found in a
    sidecar but not in `people.json` become people); `people.json` (people, nicknames, thumbnail
    faces, rejected suggestions; atomic, merged, unknown keys kept, rebuildable from the sidecars);
    the index records of the new fields; the embeddings cache, one file per model id beside the
    index (per face: embedding, landmarks; per photo: searched), appended, read back with a torn
    tail dropped, deleted with its model, never read for another model; a rebuild keeps every face,
    person and geotag; the "nothing silently dropped" mapping test lists the new fields;
    `data-model.md` §3–§4 say what landed.
  - done when: `crates/store/tests/fields.rs`: faces, people and geotags survive a write, a reopen
    and a rebuild from the folders; `crates/store/tests/people.rs`: `people.json` round-trips with
    unknown keys kept and is rebuilt from sidecars when missing; `crates/store/tests/faces_cache.rs`:
    embeddings round-trip, a torn tail drops only the last record, a second model's cache is never
    mixed in, removing a model deletes only its file; a crash at one point before and one after the
    commit (`fs` faults) keeps the old or the new state.
- ✅ **M13.10 `faces`: the detector, alignment, embeddings, manifests and the self-test**
  - after: M13.6, M12.R
  - files: `crates/faces/src/detect.rs`, `crates/faces/src/detect/**`, `crates/faces/src/align.rs`, `crates/faces/src/embed.rs`, `crates/faces/src/manifest.rs`, `crates/faces/src/catalog.rs`, `crates/faces/src/known.rs`, `crates/faces/src/hash.rs`, `crates/faces/src/selftest.rs`, `crates/faces/tests/detect.rs`, `crates/faces/tests/models.rs`, `crates/faces/README.md`, `docs/faces.md`
  - scope: `faces.md` §2–§3 with the models reserved decision 5 chose (asked at M12.R; the spec's
    candidates are YuNet 2023mar as the detector and SFace or AuraFace as the recogniser): the
    detector's input at 640 px and its output decoded to boxes, scores and five landmarks with
    non-maximum suppression, boxes normalised on the upright photo; alignment by a similarity
    transform to a 112 × 112 crop (a box crop without landmarks); one unit embedding per face
    (float64 norm); manifests (preprocessing, dimension, `matchCosine`, licence notice,
    provenance) and their validation; the built-in list of the chosen models (https URL pinned to
    a commit, size, SHA-256, licence and training data as known) and the bring-your-own
    `catalog.json`; recognising a dropped file by its SHA-256 or stating the assumptions read from
    its shapes; the self-test (load, shapes, a fixed synthetic input) before an install; ported
    from LightCraft `crates/faces` (`yunet.rs`, `align.rs`, `manifest.rs`, `catalog.rs`,
    `known.rs`, `hash.rs`, `suggest.rs`) at f34c7a8a; `faces.md` §3.2 records the chosen models.
  - done when: `crates/faces/tests/detect.rs` (synthetic networks of the detector's shape): the
    decoder turns planted outputs into the expected boxes and landmarks, suppression keeps one box
    per face, boxes stay inside the photo; alignment maps the five landmarks to their template
    within half a pixel; embeddings have unit length; `crates/faces/tests/models.rs`: a manifest
    without a licence or with an http URL is refused, a dropped file is recognised by hash, an
    unknown one gets stated assumptions, the self-test refuses a model whose shapes do not match;
    with `DALICA_FACE_MODELS` set, the real models pass their self-test (skipped and said so
    otherwise).
- ✅ **M13.11 `meta`: MWG face regions**
  - after: M10.37, M12.R
  - files: `crates/meta/src/xmp/regions.rs`, `crates/meta/src/fields.rs`, `crates/meta/src/write/mod.rs`, `crates/meta/src/xmp/rdf.rs`, `crates/meta/tests/regions.rs`, `crates/meta/fuzz/fuzz_targets/regions.rs`, `crates/meta/fuzz/Cargo.toml`, `docs/xmp-interop.md`
  - scope: `xmp-interop.md` §6: reading `mwg-rs:Regions` (`Face` entries with their name, centre
    and size normalised or in pixels over `AppliedToDimensions`, `mwg-rs:Rotation` turned back to
    the upright frame, boxes clipped, bad ones skipped, other region types kept and ignored);
    writing the named faces given (`AppliedToDimensions`, one `Face` entry each with a normalised
    area, existing face entries matched by IoU ≥ 0.5 keeping their other fields, unmatched ones
    removed, non-face entries untouched) through the merge writer, in every container that writes
    XMP; a fuzz target `regions`; the §6 status line.
  - done when: `crates/meta/tests/regions.rs` (`xmp-interop.md` §8, item 5): pixel and normalised
    areas, all eight orientations and `mwg-rs:Rotation` read back to the same upright boxes; a
    write then a read returns every named face; non-face regions and a matched entry's foreign
    fields are byte-identical after a write; writing twice gives the same file; a region list of
    100 000 entries is bounded; the existing round-trip tests pass unchanged.
- ✅ **M13.12 `ui-kit`: the map: the offline layer, tiles, markers, the pin and bubbles**
  - after: M13.1, M13.5
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/map.rs`, `crates/ui-kit/src/widgets/map/**`, `assets/maps/**`, `ATTRIBUTION.md`, `crates/ui-kit/tests/map_widget.rs`, `crates/ui-kit/tests/fingerprints/map*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`, `docs/assets.md`
  - scope: `organizing.md` §7 and the decisions above: a `map` widget on `core::geo`'s Web
    Mercator: the offline layer (Natural Earth 1:110m land, converted to a compact quantised
    polygon file in `assets/maps/` with its `ATTRIBUTION.md` row, drawn in the `gray` ramp, and a
    graticule), tile textures handed in by the caller drawn over it, an attribution line, zoom +
    / − buttons, a drag to pan and the wheel to zoom around the pointer; markers (grouped closer
    than 32 points, with counts), the pin tool and its pin, a bubble anchored to a marker or the
    pin and kept inside the widget (a thumbnail stack, text and buttons given by the caller);
    reports clicks, drops (a payload under the pointer as a `LatLon`) and the visible box;
    registers `map:<id>`, `marker:<n>`, `button:map.zoomIn`, `button:map.zoomOut`; the gallery
    gains it; `design-system.md` §6 and `assets.md` §8 say what landed.
  - done when: `crates/ui-kit/tests/map_widget.rs`: a drop at a pixel reports the point the
    projection gives, within one pixel, at zoom 2 and 15; the wheel keeps the point under the
    pointer fixed; two markers 20 points apart draw as one marked 2 and split when zoomed in; a
    bubble near each edge stays inside; the offline layer's file stays within 256 KB and loads
    without a panic when truncated; fingerprints of the offline map with markers and a bubble;
    `cargo xtask assets` passes.
- ✅ **M13.13 `fetch`: the HTTPS transport**
  - after: M13.8, M12.R
  - files: `crates/fetch/src/http.rs`, `crates/fetch/src/http/**`, `crates/fetch/src/proxy.rs`, `crates/fetch/tests/http.rs`, `crates/fetch/fuzz/**`, `.github/workflows/fuzz.yml`, `xtask/src/deps.rs`, `docs/security.md`
  - scope: the `Transport` that reserved decision 9 chose (asked at M12.R): for `rustls` with a
    pure-Rust crypto provider, an HTTP/1.1 ranged GET over `std::net` with certificate validation
    always on, bundled web roots, redirects capped and never from https to http, response headers
    and bodies capped, ported from LightCraft `crates/fetch/src/http.rs` at f34c7a8a (`rustls`,
    `rustls-rustcrypto`, `webpki-roots`); for the platform's downloader, the same trait over it;
    the proxy of the decisions above (`HTTPS_PROXY` / `NO_PROXY`, or a manual host and port,
    `CONNECT` only); a fuzz target `response` (any bytes as a server's reply) and its job;
    `security.md` §8.2 records the decision and the stack, §1.1's network row, §7.1 the `fetch`
    target and `meta`'s `regions`; any new dependency passes `cargo xtask deps` without a new
    exception (an exception is the maintainer's: `tasks block`).
  - done when: `crates/fetch/tests/http.rs` against local synthetic servers on loopback (no
    internet): a ranged resume, a redirect followed and an https → http one refused, a truncated
    body, a stalled server timing out, oversized headers refused, a proxy `CONNECT` used when set
    and skipped for a `NO_PROXY` host; a certificate that does not validate fails; `cargo xtask
    deps` passes; `cargo +nightly fuzz build` builds the target.
- ✅ **M13.45 `scan` + `engine` + `store`: faces, geotags and people read from the sidecars on scan and rebuild**
  - after: M13.9
  - files: `crates/scan/src/walk.rs`, `crates/scan/src/lib.rs`, `crates/engine/src/scanning.rs`, `crates/store/src/stale.rs`, `crates/store/src/rebuild.rs`, `crates/store/tests/fields.rs`, `crates/engine/tests/sidecar_people_places.rs`, `crates/scan/README.md`, `crates/store/README.md`, `docs/data-model.md`
  - scope: a data-safety task found by M13.9 (#861), ahead of the first writers of these keys
    (M13.14, M13.15). M13.9's sidecar writer makes the model the truth for each photo's
    `"faces"` and `"geo"` and the folder's `"people"`, but the engine fills its model from the
    scans (`dalica_scan::photo_info`, `photo_albums` … in `walk.rs`, then `with_albums` in
    `scanning.rs`), never through `store::rebuild`, and the scans read none of the three; so a
    folder whose faces and places the model never loaded (an index rebuilt with
    `library.rebuild` or after damage, a folder copied in from elsewhere, a photo restored beside
    its old entry) would lose them at its next sidecar write. The walk reads them on the scan's
    thread with the rest of the sidecar (`FoundPhoto::faces`, `FoundPhoto::geo`,
    `FoundFolder::people`) by the rules of `store::mapping::read_photo` and `read_people`: `[lat,
    lon]` → `Geo::At`, `null` → `Geo::Removed`, no key → none, a wrong type unused; faces Dalica
    cannot hold (bad entries, past 256, empty rectangles) left out and kept in the file. Like
    albums, they ride in the op that adds the photos (`scanning.rs`, beside `with_albums`, so the
    watcher's rescans through `rescan_op_with_albums` get them too and no sidecar write can run in
    between): the people the faces need created first (`AddPerson`) by the rules of M13.9's
    rebuild (a name another person holds, ignoring case, is that person; shared from
    `store::rebuild` rather than copied), then `SetFaces` and `SetGeo`; a face naming a person
    named nowhere stays out of the model and in the file (the writer keeps it). A rebuild's thread
    also reads `people.json` when present, as it reads `albums.json`, for nicknames and
    thumbnails. Known photos keep the model's word, as for every other field. A scan never writes
    these keys: applying its ops leaves the sidecar's bytes as they were, and `Geo::Removed`
    reaches the model so M13.14's first-scan import of the file's GPS sees the user's word.
    `stale.rs` counts `faces` and `geo` as Dalica's data (`PHOTO_DATA_M13`, `FACE_FIELDS`), so an
    entry holding only those is marked and pruned like a starred one. The scanning unit test that
    compares the scan's readings with the store's grows the faces, geo and people cases; the
    READMEs and `data-model.md` §3 and §7 say what landed.
  - done when: `crates/engine/tests/sidecar_people_places.rs` (a new module of `tests/all.rs`):
    a folder whose `.dalica.json` holds faces (named, unnamed and ignored, plus one naming a person
    named nowhere), a photo with `"geo": null` and the folder's `"people"` is scanned into a fresh
    index, by `folders.add` and again by `library.rebuild`; the model holds every usable face, the
    removed location and the people; then a caption change is written and the sidecar's `faces`,
    `geo` and `people` are byte-for-byte what they were (the unusable face too); the same folder
    added by a watcher's rescan keeps them as well; `crates/store/tests/fields.rs`: the
    stale-entry test gains an entry holding only `faces` and one holding only `"geo": null`, each
    marked when its file goes and pruned after 30 days; the readers' agreement test passes;
    `panic_hunt` green; `cargo xtask ci`.
- ✅ **M13.14 `engine`: geotags: set, clear, copy and paste, written to the files and read on scan**
  - after: M13.1, M13.3, M13.9, M13.45
  - files: `crates/engine/src/cmd/places.rs`, `crates/engine/src/places.rs`, `crates/engine/src/metadata.rs`, `crates/engine/src/scanning.rs`, `crates/scan/src/meta.rs`, `crates/scan/tests/meta.rs`, `crates/engine/tests/places.rs`
  - scope: `organizing.md` §7, `data-model.md` §5 and `xmp-interop.md` §2: `photo.geo.set {ids,
    geo | null, confirm?}` (one undo step; moving more than 20 tagged photos or clearing more than
    5 answers `needs_confirm` without `confirm`), `geo.copy {id}`, `geo.paste {ids}`, `geo.clear
    {ids, confirm?}`; `places.list {ids? | section?}` (the geotags of the photos shown or selected,
    with the count of places); the place written to the file through the write queue when Write
    captions, tags and places is on (EXIF GPS and its XMP mirror; clearing removes both), undo
    writing the old value back; the file's GPS imported into the model on the first scan and synced
    on a rescan as `scan/meta.rs` does the other owned fields; the effective geotag in
    `photo.inspect` and `library.inspect`.
  - done when: `crates/engine/tests/places.rs` on temp JPEG, PNG, WebP and TIFF files: a geotag
    set on 3 photos is one undo step, reaches each file and reads back within 10⁻⁶°; Clear
    Location removes the GPS position and its XMP mirror and undo writes it back; with the option
    off nothing is written; a phone photo's GPS appears as its geotag after the first scan; paste
    applies the copied place; 21 moves and 6 removals ask; each command's contract and graceful
    failures; the owner thread made no `fs` call; `panic_hunt` green.
- ✅ **M13.15 `engine`: faces and people as data: naming, ignoring, people, person albums, face tiles and search**
  - after: M13.1, M13.3, M13.9, M13.45
  - files: `crates/engine/src/cmd/people.rs`, `crates/engine/src/cmd/faces.rs`, `crates/engine/src/face_tiles.rs`, `crates/engine/src/search.rs`, `crates/engine/src/automation.rs`, `crates/engine/tests/people.rs`, `docs/control-protocol.md`
  - scope: `organizing.md` §6 and `faces.md` §9, no model needed: `faces.list {person | unnamed |
    ids, ignored?}` (faces with their photo, rectangle and group), `faces.name {faces, person |
    name}` (an unknown name answers `needs_person` for New Person unless `create`), `faces.ignore`,
    `faces.move`, `faces.remove` (back to Unnamed), `faces.add {id, rect, person?}` (a face drawn
    by hand), `people.list {sort}` (counts, thumbnails, pending suggestions), `people.create`,
    `.rename`, `.merge`, `.delete {confirm}`, `.setThumbnail`; person albums as sections (`section`
    `person:<id>`) for the light table, slideshows and export; face tiles: `Session::request_face_tiles`
    and `take_face_tiles` cut from the thumbnail ladder on a worker, in memory only; `person:`
    and the Faces filter made available in the engine's search (no longer "unavailable"); people
    data withheld from automation without `--automation-allow-people` (names `null`, rectangles
    left out, counts kept) and embeddings never returned; `control-protocol.md` §6.4 says what is
    withheld.
  - done when: `crates/engine/tests/people.rs`: naming a group of 12 faces is one undo step and
    survives a restart and a rebuild; an unknown name asks first; ignore, move, remove, merge,
    rename and delete each undo in one step; a face added by hand on a photo turned 90° is stored
    in the stored frame; `person:ana` and the Faces filter find the right photos; a face tile
    arrives off the owner thread and the owner made no `fs` call; without the people permission
    `faces.list` over automation has no names or rectangles; each command's contract and graceful
    failures; `panic_hunt` green.
- ✅ **M13.16 `engine`: face models: list, install, download, remove and use**
  - after: M13.3, M13.8, M13.10, M13.13
  - files: `crates/engine/src/cmd/face_models.rs`, `crates/engine/src/face_models.rs`, `crates/engine/tests/face_models.rs`, `docs/faces.md`
  - scope: `faces.md` §3 and §9 with the models of reserved decisions 5 and 9 (asked at M12.R):
    `faces.models.list` (installed, built-in and `catalog.json` entries with licence, commercial
    use, training data, size, site and state), `.inspect {path}`, `.install {path, acknowledged}`
    (a local file after the licence; known by hash, else stated assumptions; the self-test before
    it lands in the models folder), `.download {id, acknowledged}` (a `fetch` job through M13.13's
    transport with progress and resume), `.cancel`, `.remove` (the file and its cached
    embeddings), `.select`; `acknowledged` must be `true` or nothing happens; the models folder
    (`<config>/models` or `DALICA_FACE_MODELS`) shared by the app, the CLI and MCP, never inside a
    watched folder; the newest recogniser in use unless another is selected; `faces.md` §3 records
    what landed.
  - done when: `crates/engine/tests/face_models.rs` (an injected transport and synthetic models):
    no request before `acknowledged`; a download resumes after a cut and installs only after the
    hash and the self-test; a wrong hash deletes the file; a local file installs with its stated
    assumptions; removing a model deletes its cached embeddings; selecting another model never
    mixes caches; each command's contract and graceful failures; `panic_hunt` green.
- ✅ **M13.17 `faces` + `xtask`: the public-faces corpus, the model pins, the floors and calibration**
  - after: M13.7, M13.10
  - files: `crates/faces/tests/corpus.rs`, `crates/faces/tests/corpus_floors.json`, `crates/faces/src/calibration.rs`, `crates/faces/src/known.rs`, `crates/faces/Cargo.toml`, `xtask/src/corpus_pins.rs`, `xtask/src/corpus.rs`, `xtask/corpus/public-faces.sha256`, `xtask/corpus/face-models.sha256`, `.github/workflows/ci.yml`, `docs/development.md`
  - scope: `faces.md` §10 and `development.md` §11: `corpus/public-faces/` (public-domain
    portraits, several of each person, with `SOURCES.tsv`) assembled in a `dalica-corpus` clone
    and tested with `--local`; publishing it (the push to `pkkid/dalica-corpus` and the pin) needs
    the maintainer's go-ahead, asked at M12.R with reserved decision 5 (without it on record,
    `tasks block`); `face-models`, the chosen models pinned by URL and SHA-256 and fetched from
    their upstreams, never copied into `dalica-corpus`; `crates/faces/tests/corpus.rs` behind the
    `corpus` feature: detection, alignment, embedding agreement with recorded reference vectors,
    grouping (each person's portraits in one group at 85, groups pure) and suggestion precision at
    the default confidence, floors at the measured counts; the built-in models' calibration tables
    measured on the set and written into `known.rs`; the CI corpus job runs it; §11's table.
  - done when: `cargo xtask test-corpus -p faces` passes with the floors committed; `cargo xtask
    corpus` reports both sets present; the calibration tables rise with the slider value.
- ✅ **M13.18 `engine`: the face scan**
  - after: M13.3, M13.7, M13.9, M13.10, M13.15
  - files: `crates/engine/src/faces_scan.rs`, `crates/engine/src/faces_index.rs`, `crates/engine/src/cmd/face_scan.rs`, `crates/engine/tests/face_scan.rs`
  - scope: `faces.md` §4–§5 and the decisions above: while Find faces in my photos is on and a
    recogniser is installed, a `faces` job at the lowest priority follows new and changed photos:
    the upright preview (≤ 2048 px; a RAW's embedded preview), faces detected once per photo
    (photos with rectangles already, from a file or a hand, only embedded), embedded and cached
    (M13.9), detected rectangles written as Unnamed faces, one op per batch without an undo step (as a scan's imports are); named photos
    first, then the newest folders; folders with "Find faces in this folder" off, hidden folders
    when excluded and offline volumes skipped; the pace of `faces.md` §4 from
    `Session::user_activity`, never interrupting a photo, worker count bounded by memory; the
    in-memory index and grouping (M13.7) kept current, groups for `faces.list {unnamed}`;
    `faces.enable {on}` (off stops and offers to drop the embeddings), `faces.detect {ids,
    apply?}`, `faces.index`, `faces.reset {ids}` (Find Faces Again: forgets searched, keeps named
    faces), `faces.removeInFolders {folders, confirm}` (the Folder Manager's Remove Faces); the
    scan's progress for the People group's header; logs say counts, never names.
  - done when: `crates/engine/tests/face_scan.rs` (a synthetic detector and recogniser planted
    through the models folder): faces of a `scenes` set appear under Unnamed in groups; a removed
    or ignored face is not found again on a rescan, Find Faces Again finds it; a photo in an
    excluded folder is never read; the job yields while user activity is reported and the owner
    made no `fs` call; switching models rebuilds from its own cache; under `RUST_LOG=trace` the log
    holds no name, rectangle or embedding; `panic_hunt` green.
- ✅ **M13.19 `engine` + `store` + `scan`: people names in photo files**
  - after: M13.9, M13.11, M13.15
  - files: `crates/engine/src/cmd/face_regions.rs`, `crates/engine/src/metadata.rs`, `crates/engine/src/scanning.rs`, `crates/scan/src/meta.rs`, `crates/scan/tests/meta.rs`, `crates/store/src/metawrite.rs`, `crates/store/src/metawrite/**`, `crates/engine/tests/face_regions.rs`, `docs/xmp-interop.md`
  - scope: `xmp-interop.md` §6 and `data-model.md` §5: MWG regions read on scan become faces and
    people (names matched ignoring case, else created) on a photo's first scan and synced as the
    other owned fields; with Store people names in photos on, every change to a photo's named faces
    queues a regions write through the metadata write queue (unnamed and ignored faces never
    written, the file's other regions kept); `faces.writeXmp {ids, confirm}` (Tools ▸ Experimental
    ▸ Write People Names to Photos…) writes them once for a selection; turning the option on
    writes nothing until a face changes or the command runs; §6's status line.
  - done when: `crates/engine/tests/face_regions.rs` on temp files: a photo with MWG regions from
    another application shows its people after the first scan; naming a face with the option on
    writes a region that reads back, renaming the person rewrites it, and undo writes the old
    value; with the option off nothing is written and `faces.writeXmp` writes after its question;
    a file's pet region survives every write; the owner thread made no `fs` call.
- ✅ **M13.20 `engine`: map tiles and place search**
  - after: M13.13, M13.14
  - files: `crates/engine/src/cmd/map.rs`, `crates/engine/src/map_tiles.rs`, `crates/engine/src/geocode.rs`, `crates/engine/tests/map_tiles.rs`, `docs/security.md`
  - scope: `organizing.md` §7 with reserved decisions 4 and 9 (asked at M12.R: the provider, its
    attribution and terms, on or off by default, and the geocoding service): `Session::request_map_tiles
    (box, zoom)` and `take_map_tiles()`: tiles fetched on a worker through `fetch` only while Allow
    map tiles is on and the Places pane is open (`set_places_open`), at most two at a time, each
    at most 1 MiB and 1 024 pixels and decoded under the raster limits, cached under
    `<cache>/tiles/` (200 MB, oldest first, only its own files), cancelled when the pane closes;
    `map.allow {on}`; `places.search {text}`: coordinates parsed locally, else the geocoding
    service on a worker (a reply ≤ 1 MiB and depth 32; results with name and place), only when
    asked; nothing fetched at start-up, during scans or for a closed pane; `security.md` §4.2's
    row for tiles and geocoder replies.
  - done when: `crates/engine/tests/map_tiles.rs` (a counting injected transport): no request
    with tiles off or the pane closed, requests only for the visible tiles once allowed and open,
    none after closing; a cached tile is not fetched again; an oversized or undecodable tile is a
    placeholder; "38.7116, -9.1389" and "38°42′42″N 9°8′20″W" are found without a request; a
    hostile geocoder reply is refused without a panic; the cache prunes only its own files; the
    owner thread made no `fs` call.
- ✅ **M13.21 `ui`: the Unnamed face grid, naming, ignoring and the face menu**
  - after: M13.4, M13.5, M13.15
  - files: `crates/ui/src/panels/faces.rs`, `crates/ui/src/panels/faces/**`, `crates/ui/src/commands/faces.rs`, `crates/ui/src/dialogs/new_person.rs`, `crates/ui/tests/unnamed_faces.rs`, `crates/ui/tests/fingerprints/unnamed-*.json`
  - scope: `organizing.md` §6.2 and §6.3: selecting Unnamed shows the face grid in place of the
    light table: the header "Unnamed people" with [Expand Groups] and [Show Ignored Faces], groups
    of M13.5 face tiles from M13.15's face tiles, each with "Add a name" (autocomplete over people;
    Enter names the whole group, a new name opens New Person: Name prefilled, Nickname, [OK]
    [Cancel]) and an × that ignores it ("Ignore these faces?" [Ignore] [Cancel] + "Don't ask
    again"); faces selected by click, Ctrl / Shift-click and a rubber band, Delete ignoring them,
    and dragged with the faces payload; the face context menu (Move to New Person…, Move to Person
    ▸, Remove from Person, Set as Person Thumbnail, Ignore Face), used by M13.25's person sections
    too.
  - done when: `crates/ui/tests/unnamed_faces.rs` (headless, by id): three planted groups show
    largest first; typing "Ana" and Enter names a group as one step and it leaves the grid; a new
    name opens New Person; × asks and ignores, Show Ignored Faces shows them; a rubber band selects
    faces across groups; Move to Person ▸ lists people; at 900 × 600 the grid fits; a fingerprint
    of the grid at 1400 × 900.
- ✅ **M13.22 `ui`: the Places pane, geotagging by drag and pin, and the Geotag menu**
  - after: M13.4, M13.12, M13.14
  - files: `crates/ui/src/panels/places.rs`, `crates/ui/src/panels/places/**`, `crates/ui/src/commands/places.rs`, `crates/ui/src/dialogs/geotag_confirm.rs`, `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/places_pane.rs`, `crates/ui/tests/fingerprints/places-*.json`, `docs/organizing.md`
  - scope: `organizing.md` §7: the pane (header "Places", "No places in the selection" / "9 places
    in the selection", M13.12's map with the offline layer, the photos of the current folder,
    album or selection as markers, zoom, the pin tool, "Search for a place:" finding typed
    coordinates); photos dragged from the light table onto the map: the bubble "Put %d photos
    here?" with a thumbnail stack [OK] [Cancel], "Move %d photos here?" when some are tagged, the
    confirmations past 20 moved or 5 cleared; a marker's bubble "%d photos here" [Show These
    Photos] [Remove Location]; the pin placed then OK puts the selection there; Tools ▸ Geotag ▸
    Clear Location…, Copy Location and Paste Location live; the geotag badge reads the effective
    geotag; `organizing.md` §7 records the decisions above.
  - done when: `crates/ui/tests/places_pane.rs` (headless, by id): dropping 3 photos on the map
    and OK geotags them as one step and the markers show 3; dropping on tagged photos asks Move;
    Remove Location on a marker clears its photos after the question past 5; Show These Photos
    selects them; typed coordinates move the pin; Copy and Paste Location work from the menu; the
    badge shows on a photo tagged in Dalica; at 900 × 600 the pane fits; fingerprints of the pane
    with markers and with a bubble at 1400 × 900.
- ✅ **M13.23 `edit` + `create` + `engine`: faces in the crop suggestions and the slideshow's Pan and Zoom**
  - after: M13.15
  - files: `crates/edit/src/suggest.rs`, `crates/edit/tests/suggest.rs`, `crates/engine/src/cmd/crop.rs`, `crates/engine/tests/crop_commands.rs`, `crates/create/src/slideshow.rs`, `crates/create/src/slideshow/**`, `crates/create/tests/slideshow.rs`, `crates/engine/src/cmd/slideshow.rs`, `crates/engine/tests/slideshow.rs`, `docs/editing.md`, `docs/viewing.md`
  - scope: `editing.md` §6.1 and `viewing.md` §1.3: `edit::suggest` takes the photo's face
    rectangles (mapped to the picture shown) and ranks a crop holding every face, its eyes near the
    upper third, before the saliency subject; `photo.crop.suggest` passes them; the slideshow's Pan
    and Zoom moves toward the largest named face, else the largest face, else the centre (the
    face given by `slideshow.list`); both docs drop "faces join in M13".
  - done when: `crates/edit/tests/suggest.rs`: with two planted faces the first suggestion holds
    both, without faces the result is as before; `crates/create/tests/slideshow.rs`: Pan and Zoom's
    end frame is centred nearer the face than the centre; `crates/engine/tests/crop_commands.rs`
    and `slideshow.rs`: a photo with a named face passes it on; rows `PPL-CROP`, `VIEW-SLIDESHOW`.
- ✅ **M13.24 `ui`: the People pane, face boxes in the edit view and Add a Person Manually**
  - after: M13.4, M13.5, M13.15
  - files: `crates/ui/src/panels/people_pane.rs`, `crates/ui/src/panels/people_pane/**`, `crates/ui/src/panels/edit/faces.rs`, `crates/ui/src/commands/people_pane.rs`, `crates/ui/src/panels/search_banner.rs`, `crates/ui/tests/people_pane.rs`, `crates/ui/tests/fingerprints/people-pane-*.json`, `crates/ui/tests/fingerprints/edit-faces-*.json`
  - scope: `organizing.md` §6.4: the pane (People pane switcher segment and View ▸ People):
    "People in these photos" with counts and "Unnamed faces" with Add a name and ×, following the
    selection or the photo in the edit view; face boxes on the photo in the edit view (M13.5's
    `face_box`, mapped from the stored frame through the preview's geometry) and hovering a face in
    the pane highlighting its box and back; Add a Person Manually (edit view only): a box to drag
    with "Drag the box to fit the face, then type a name." and a name field, stored through
    `faces.add`; several people Ctrl-clicked filter the light table to photos holding all of them
    with the filter banner "Showing photos with Ana and Rui" [Show All].
  - done when: `crates/ui/tests/people_pane.rs` (headless, by id): the pane lists the selection's
    people with counts; naming an unnamed face from the pane is one step; hovering a pane face
    highlights its box; a box drawn by hand on a photo turned 90° and named lands where it was
    drawn after a restart; two people selected filter the light table and Show All clears it; at
    900 × 600 the pane fits; fingerprints of the pane and of the face boxes at 1400 × 900.
- ✅ **M13.25 `ui`: the People group in the tree, person sections and the person menu**
  - after: M13.4, M13.5, M13.15, M13.21
  - files: `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/folders/**`, `crates/ui/src/panels/light_table/header.rs`, `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/src/commands/people.rs`, `crates/ui/src/dialogs/people.rs`, `crates/ui/tests/people_tree.rs`, `crates/ui/tests/fingerprints/people-tree-*.json`, `crates/ui/tests/fingerprints/person-section-*.json`, `docs/organizing.md`, `docs/ui-layout.md`
  - scope: `ui-layout.md` §4 and `organizing.md` §6.2–§6.3: the People group between Albums and
    Folders (Unnamed (n) first, then a row per person with its 20-point face and count, the
    caution "?" badge when it has suggestions, sorted by name or count from the view options),
    "Finding faces… n of m" in its header while the scan runs; a drop of faces on a person row
    names them, a drop of photos on a person does nothing (no face); person sections (the person
    icon, the header's Zoom to Faces / Show Whole Photos, Set as Person Thumbnail, Play, and
    Collage drawn disabled "Arrives with Collage") with Zoom to Faces cells showing face tiles that
    open M13.21's face menu; Delete or Remove from Person sends a face back to Unnamed; the person
    menu (Rename…, Merge Into ▸, Delete Person with its question); `organizing.md` §6 and
    `ui-layout.md` record the decisions above.
  - done when: `crates/ui/tests/people_tree.rs` (headless, by id): the People group lists Unnamed
    and two people with counts in name and count order; dragging two Unnamed faces onto Ana names
    them as one step; Ana's section shows her photos and Zoom to Faces shows her faces; Rename,
    Merge Into and Delete Person each undo in one step; Play starts a slideshow of her photos; at
    900 × 600 the tree fits; fingerprints of the tree and a person section at 1400 × 900.
- ✅ **M13.26 `engine`: suggestions: confirm, reject, Find More and evaluate**
  - after: M13.7, M13.15, M13.18
  - files: `crates/engine/src/cmd/suggest.rs`, `crates/engine/src/suggestions.rs`, `crates/engine/tests/suggestions.rs`
  - scope: `faces.md` §5 on M13.7's maths and M13.18's index: `faces.suggest {ids? | person?}`
    (unnamed faces with their "Name?" person while Suggest names is on), `faces.confirm {faces}`
    (each a reference from then on), `faces.confirmAll {person}` (one undo step),
    `faces.reject {faces}` (the pair kept in `people.json`), `faces.findMore {person}`,
    `faces.evaluate` (per confidence step: suggestions made and right); suggestions kept current as
    faces are embedded and named; the pending count per person for `people.list`.
  - done when: `crates/engine/tests/suggestions.rs` (synthetic embeddings through the cache): a
    face near Ana's is suggested as Ana and confirming it makes it a reference; Confirm All is one
    undo step; a rejected pair never comes back, also after a restart; nothing is suggested with
    Suggest names off or below the threshold; Find More lists the near misses most similar first;
    `faces.evaluate` reports per step; each command's contract and graceful failures; `panic_hunt`
    green.
- ✅ **M13.27 `ui`: the face model dialog, Options ▸ People, Remove Faces and the Faces filter**
  - after: M13.4, M13.16, M13.18
  - files: `crates/ui/src/dialogs/face_model.rs`, `crates/ui/src/dialogs/options/people.rs`, `crates/ui/src/dialogs/folder_manager.rs`, `crates/ui/src/dialogs/folder_manager/**`, `crates/ui/src/commands/face_model.rs`, `crates/ui/src/panels/toolbar.rs`, `crates/ui/src/app.rs`, `crates/ui/tests/face_model_ui.rs`, `crates/ui/tests/fingerprints/dialog-face-model-*.json`, `crates/ui/tests/fingerprints/dialog-options-people-*.json`
  - scope: `faces.md` §3.1 and `organizing.md` §6.1: the face model dialog (per file: licence,
    commercial use, training data or that it is unknown, size and site; Download disabled until "I
    have read these terms and accept them for my own use" is ticked; progress and Cancel; Open
    page; Add a model file… and an `.onnx` dropped on the window through the same dialog; a
    `catalog.json` model's notice in `caution`), offered by Options ▸ People, by turning on Find
    faces without a model and by the first open of People; Options ▸ People (Find faces in my
    photos, Suggest names, the two confidence sliders, Store people names in photos, Face model
    status with Download… / Remove / Use); turning Find faces off offers to delete the cached
    embeddings; the Folder Manager's Remove Faces question ("Remove the face information …?"
    [Remove Faces] [Keep Faces]) live; Picture ▸ Find Faces Again live; the toolbar's Faces filter
    enabled.
  - done when: `crates/ui/tests/face_model_ui.rs` (headless, a synthetic model file): Download
    stays disabled until the box is ticked; a local file installs after the dialog and the status
    shows it; Remove asks and removes; the confidence sliders move by 5 within 50–95 and survive a
    restart; Remove Faces drops the folder's faces and Keep Faces keeps them; the Faces filter
    shows only photos with faces; at 900 × 600 the dialog and the tab fit; fingerprints of the
    dialog and the tab at 1400 × 900.
- ✅ **M13.28 `ui`: Options ▸ Network, allowing map tiles and searching for a place**
  - after: M13.20, M13.22
  - files: `crates/ui/src/dialogs/options/network.rs`, `crates/ui/src/panels/places/tiles.rs`, `crates/ui/src/panels/places/search.rs`, `crates/ui/src/commands/map.rs`, `crates/ui/tests/map_tiles_ui.rs`, `crates/ui/tests/fingerprints/dialog-options-network-*.json`, `crates/ui/tests/fingerprints/places-tiles-*.json`
  - scope: `menus-and-shortcuts.md` §5 and `organizing.md` §7: Options ▸ Network (Allow map tiles,
    the tile server template, the geocoding service, the proxy, as reserved decision 4 left them);
    the pane's one-time question "Show map tiles from %provider%? …" [Allow] [Not Now]; tiles from
    M13.20 drawn over the offline layer with the provider's attribution, the pane telling the
    session it is open; "Search for a place:" sending a name to the geocoding service on Enter and
    listing the results, a click moving the pin.
  - done when: `crates/ui/tests/map_tiles_ui.rs` (headless, a counting injected transport): no
    request until Allow; after Allow tiles appear with the attribution and stop when the pane
    closes; Not Now keeps the offline map; a search on Enter lists results and moves the pin; the
    tab's values survive a restart; at 900 × 600 the tab fits; fingerprints of the tab and of the
    pane with tiles at 1400 × 900.
- ✅ **M13.29 `mcp`: geotags and people over MCP, and the workflows W13 and W14**
  - after: M13.14, M13.15, M13.19
  - files: `crates/mcp/src/tools/organize.rs`, `crates/mcp/src/headless.rs`, `crates/mcp/src/connect.rs`, `crates/mcp/tests/organize_tools.rs`, `crates/mcp/tests/workflows/w13_geotags.rs`, `crates/mcp/tests/workflows/w14_people.rs`, `crates/mcp/tests/workflows/main.rs`, `xtask/src/stats.rs`, `docs/mcp.md`
  - scope: `mcp.md` §5 and §9: `set_metadata {geo}` live (no longer `unsupported`) and `photo_info`
    with the geotag; W13 (**geotags**: `set_metadata {geo}` on 3 photos → GPS in each file read
    back → `query_photos {geotagged: true}` returns them → clear → GPS gone → one undo restores
    it) and W14 (**people**, with `--automation-allow-people`: name the planted faces of 4 photos
    by `cmd_faces_name` → `query_photos {query: "person:Ana"}` returns them → with Store people
    names in photos on the regions are in the files → without the permission names read `null`);
    the floor in `xtask/src/stats.rs` raised to the fourteen workflows now required; `mcp.md`'s
    table and status line; row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp` runs W13 and W14 green; `cargo xtask stats` prints
    14 / 14 passing and `stats --check` fails below that.
- ✅ **M13.30 `ui`: suggestions in the window**
  - after: M13.24, M13.25, M13.26
  - files: `crates/ui/src/panels/light_table/suggestions.rs`, `crates/ui/src/panels/people_pane/suggestions.rs`, `crates/ui/src/panels/faces/suggestions.rs`, `crates/ui/src/commands/suggestions.rs`, `crates/ui/tests/suggestions_ui.rs`, `crates/ui/tests/fingerprints/suggestions-*.json`
  - scope: `organizing.md` §6.3–§6.4: in a person's section the strip "Suggestions:" with [Confirm
    All], [Confirm], [Reject], [Find More], the toggle "Show only suggestions" and the hint "Click
    ✓ to confirm a match or × to reject it."; each suggested face with ✓ and ×; "Name?" with ✓ / ×
    on Unnamed faces and in the People pane; the tree's "?" badge lit from the pending count.
  - done when: `crates/ui/tests/suggestions_ui.rs` (headless, synthetic embeddings): Ana's section
    shows her suggestions and ✓ moves one into her photos as one step; Reject removes it for good;
    Confirm All is one step; Show only suggestions hides her confirmed photos; the "?" badge goes
    when none are left; "Name?" in the pane confirms; fingerprints of the strip and of a face with
    its marks at 1400 × 900.
- ✅ **M13.31 `faces` + `engine` + `ui` + `xtask`: M13's budgets: the face scan, the people views and the Places pane**
  - after: M13.18, M13.21, M13.22, M13.26
  - files: `crates/faces/benches/inference.rs`, `crates/engine/benches/faces_scan.rs`, `crates/ui/benches/people_places.rs`, `xtask/src/bench.rs`, `.github/workflows/budgets.yml`, `docs/development.md`, `docs/faces.md`
  - scope: `development.md` §8 and `faces.md` §8: benches registered in `xtask/src/bench.rs`, joined
    to `BUDGETS` and the release-budgets job, each documented in §8 and §13 with its machine:
    `inference` (detection on a 640 px image and one embedding, with seeded synthetic networks of
    the chosen models' shapes), `faces_scan` (faces embedded per second on 1 000 `scenes` photos,
    memory per worker, and the window's frame time while it runs, headless), `people_places` (the
    Unnamed grid opening with 10 000 faces, and the Places pane with 10 000 geotagged photos: a
    frame with its markers grouped); budgets at 1.5 × their first measurement, plus the frame time
    while the scan runs within 16 ms at p95 whatever the first numbers; a missed budget is fixed in
    this task (the module asked of the lead; before and after numbers in the PR) or blocks with its
    numbers, never loosened; `faces.md` §8 points at them.
  - done when: `cargo xtask bench inference`, `bench faces_scan` and `bench people_places` print
    their numbers (in the PR); `cargo xtask budgets` passes with them and `--plant-ms` makes each
    fail.
- ✅ **M13.34 `edit`: fast shared effect primitives: box passes, a grid blur, noise by rows, linear-light tables**
  - after: M12.R
  - files: `crates/edit/src/fx.rs`, `crates/edit/src/fx/blur.rs`, `crates/edit/src/fx/grid.rs`, `crates/edit/src/fx/noise.rs`, `crates/edit/src/fx/color.rs`, `crates/edit/tests/fx.rs`
  - scope: effect speed (paragraph *Effect speed* above). The ground the module tasks M13.36–M13.42
    build on, so no module writes its own:
    - **Box passes** (`Gaussian::apply`): today each pass reads through a bounds-checked closure
      per tap, accumulates in `i64` and turns the plane twice with a naive transpose. Make them
      run on flat slices with the clamped edges handled outside the inner loop (a padded row or
      a split loop), the vertical passes in column strips with one running sum per column
      (cache-friendly, vectorisable across the strip) or a blocked transpose, and `i32` sums
      wherever the bound proves they fit. The sums stay exact integers, so the planes come out
      **bit for bit** the same (the order of exact integer adds is free): `Blurred`, Soften's
      blur and every `Gaussian` user gets faster with no output change.
    - **A grid blur** (`fx::grid`, `GridBlur`): the coarse-grid Gaussian that `ops/looks.rs`
      (`BandBlur`) and `ops/focal.rs` (Soft Focus) each wrote for themselves, generalised and
      made **once per render** for every band (in a `OnceLock`, through `apart`, as `Blurred`
      is) instead of by every 32-row band for its rows plus the blur's reach (at HDR Look's
      default the reach is about 150 px, so each band averages and blurs ten times its own
      rows): cells about a third of the sigma (at most 32 px) anchored at multiples of the cell
      in the whole output, each cell's mean summed straight from the window's 8-bit rows, the
      grid blurred by the box passes, read back bilinearly at a pixel's centre, with
      `BandBlur`'s variance correction (the cell's averaging and the bilinear read taken off the
      grid's sigma) and its mix-in for a blur under the smallest box blur. Over a region window
      and its margin it gives exactly the whole render's values. An optional per-pixel weight
      before averaging (Glow's highlights) and a one-channel form (a luma) are part of it.
    - **Noise by rows** (`Noise`): `Noise::at` maps each pixel through `Space` in `f64` and hashes
      four lattice corners per pixel; Film Grain, Plastic Lens and Widescreen Film call it per
      pixel. Add a row sampler that steps the stored-frame point along the row (one affine step
      per pixel), hashes a lattice corner once per cell it crosses and blends with the smoothstep
      in `f32`; it agrees with `Noise::at` within 10⁻⁴ (a test over random rows, turned and
      straightened spaces included), so the effects' fingerprints do not move.
    - **Linear light** (`fx::color`): one shared pair of 4 096-step tables
      (`to_linear` / `to_srgb` as `ops/colour.rs` and `ops/remap.rs` each build today, within
      0.01 of a level), so Vignette and the module tasks stop calling `powf` per channel.
  - done when: `crates/edit/tests/fx.rs`: the new box passes give the same plane, bit for bit, as
    the M12 ones (kept in the test as the oracle) on seeded planes of odd sizes and radii up to
    the plane's size; `GridBlur` over a region window plus its margin equals the whole render's
    grid there exactly, and its sigma matches `Gaussian` within the existing tolerance; the row
    sampler agrees with `Noise::at` within 10⁻⁴; the tables within 0.01 of a level; every effect's
    fingerprint and region test in `crates/edit/tests/` passes unchanged; the bench's `glow` and
    `soften` final before and after in the PR (they use the box passes directly); `cargo xtask ci`.
- ✅ **M13.35 `engine`: a frame's draft renders from the halved base, not through the fractional resample**
  - after: M12.R
  - files: `crates/engine/src/preview/jobs.rs`, `crates/engine/tests/preview.rs`, `crates/edit/benches/effects.rs`
  - scope: effect speed. The drafts of the four frames miss 16 ms although their own painting is
    cheap: Border 20.9 ms, Museum Matte 20.0, Drop Shadow 19.3 and Widescreen Film 24.5 at p95.
    `preview::jobs::render` takes the halved base only when the recipe is `unwarped`, which it
    tests on `Geometry::size()`: a frame grows that size, so a framed recipe renders from the
    3000 × 2000 source at `Target::Scale(¼)`, the fractional resample `jobs.rs`'s header
    measures as the expensive part. Test the photo's own size instead (`Geometry::photo_size()`,
    after crop and straighten, before any frame): a recipe that neither crops nor straightens
    renders at `Target::Full` from the base however many frames it has, which grow their canvas
    in whole pixels at the base's scale (`fit` still from the grown size, so the box stays
    covered); a crop or a straighten still renders from the source at that scale. The draft
    rule stays as it is (1, ½, ¼ from the last final's cost; never ⅛). The bench's copy of the
    rule (`Screen::render` in `crates/edit/benches/effects.rs`) changes with it, in the same
    commit, so it keeps measuring what the engine does; `jobs.rs`'s header says so.
  - done when: `crates/engine/tests/preview.rs`: a Border's and a Drop Shadow's draft render from
    the base (the preview's source untouched, the frame on all four sides, the draft within the
    two-sizes tolerance of the final shrunk), and a cropped Border still renders from the source;
    the bench's `border`, `museum_matte`, `drop_shadow` and `widescreen` drafts within 16 ms at
    p95 on two cores (before and after in the PR; Widescreen Film's final is M13.42's);
    `cargo xtask ci`.
- ✅ **M13.36 `edit`: Glow, Sharpen and Film Grain within the slider's caps**
  - after: M12.R, M13.34
  - files: `crates/edit/src/ops/detail.rs`, `crates/edit/tests/detail_effects.rs`, `crates/edit/tests/fingerprints/detail-*.json`
  - scope: effect speed. Draft / final p95 at M12.21: Glow 21.2 / 409 ms, Film Grain 11.7 /
    197 ms, Sharpen 7.9 / 145 ms; the caps 16 / 100 ms. Levers:
    - **Glow:** its highlights blurred by a full-resolution `Blurred` plane of sigma 3 % of the
      shorter side (60 px on the final): move it to M13.34's `GridBlur` with the smoothstep
      highlight weight applied before the cells are averaged, so it blurs a grid a few hundred
      cells wide instead of 6 MP; the screen blend per pixel stays.
    - **Sharpen:** a 7-tap separable Gaussian (sigma under a pixel, floored at 0.5 px) convolved
      per band through a closure and a bounds-checked `get` per tap: convert the band's rows and
      their margin to `f32` once, run both passes on flat slices with the edges handled outside
      the inner loop, keeping the taps' order (the output stays bit for bit the same).
    - **Film Grain:** one noise value per pixel through `Noise::at`: M13.34's row sampler.
  - done when: the bench's `glow`, `grain` and `sharpen` drafts within 16 ms and finals within
    100 ms at p95 on two cores (before and after in the PR, Saves not above their budgets);
    `crates/edit/tests/detail_effects.rs`: the `detail-*-large` fingerprints blessed from the
    M12 code match, the existing fingerprints, two-sizes tests and the region test pass
    unchanged (Sharpen's output bit for bit as before); `cargo xtask ci`.
- ✅ **M13.37 `edit`: Soft Focus within the slider's caps**
  - after: M12.R, M13.34
  - files: `crates/edit/src/ops/focal.rs`, `crates/edit/tests/focal_effects.rs`, `crates/edit/tests/fingerprints/focal-*.json`
  - scope: effect speed. Draft / final p95 at M12.21: Soft Focus 45.9 / 274 ms; the caps 16 /
    100 ms (Focal B&W and Graduated Tint already meet them). Its blur (sigma `Amount ×
    SOFT_SIGMA`, 22 px on the final at the default) is made by every band for its rows plus the
    reach, on a grid of cell means read through `View::get` per pixel. Levers: M13.34's
    `GridBlur`, made once per render (the same cells and sigma, so the output should not move);
    no blur read where the circle leaves the photo sharp (the mask is 0 inside Size); the mix by
    the mask on flat rows. The draft's share that does not shrink with the picture (the per-band
    reach is most of it) goes with the shared grid.
  - done when: the bench's `soft_focus` draft within 16 ms and final within 100 ms at p95 on two
    cores (before and after in the PR, its Save not above its budget); `crates/edit/tests/
    focal_effects.rs`: the `focal-soft-focus-large` fingerprint blessed from the M12 code
    matches, the existing fingerprints, two-sizes tests and the region test pass unchanged;
    `cargo xtask ci`.
- ✅ **M13.38 `edit`: Boost, Saturation and Warmify within the slider's caps**
  - after: M12.R, M13.34
  - files: `crates/edit/src/ops/remap.rs`, `crates/edit/src/ops/colour.rs`, `crates/edit/tests/remap_effects.rs`, `crates/edit/tests/colour_effects.rs`, `crates/edit/tests/fingerprints/remap-*.json`, `crates/edit/tests/fingerprints/colour-*.json`
  - scope: effect speed. Draft / final p95 at M12.21: Boost 35.7 / 476 ms, Saturation 21.5 /
    345 ms, Warmify 16.1 / 252 ms; the caps 16 / 100 ms. All three are pixel-only passes, so the
    cost is the per-pixel maths: Boost and Saturation run `vibrance_lin` (three `powf`, an `exp`,
    an HSV hue and its branches) between two table lookups; Warmify a matrix in linear light and
    `set_luma`. Levers: since each is a fixed function of the pixel's RGB for one render, a 3-D
    table built once at prepare (33³ or 65³ entries, tetrahedral or trilinear, built in parallel
    and bounded) read per pixel, within 1 level of 255 of the exact function over every 8-bit
    input (fused passes may hand it values between levels, which the interpolation covers); or,
    if that misses the tolerance on a saturated edge, 1-D tables for `t^p` and `(1 − t)^q` and
    the `exp` term with the constant `a` folded at prepare; the shared linear-light tables from
    M13.34 instead of each module's own; loops on flat slices. The other remaps and colour
    effects (already within the caps) keep their code unless the shared tables replace theirs.
  - done when: the bench's `boost`, `saturation` and `warmify` drafts within 16 ms and finals
    within 100 ms at p95 on two cores (before and after in the PR, Saves not above their
    budgets); a test over every 8-bit grey and a seeded million RGB inputs: each fast pass within
    1 level of the exact function; the `remap-boost-large`, `colour-saturation-large` and
    `colour-warmify-large` fingerprints blessed from the M12 code match; the existing
    fingerprints and region tests pass unchanged; `cargo xtask ci`.
- ✅ **M13.39 `edit`: HDR Look, Plastic Lens and Dream Glow within the slider's caps**
  - after: M12.R, M13.34
  - files: `crates/edit/src/ops/looks.rs`, `crates/edit/tests/look_effects.rs`, `crates/edit/tests/fingerprints/looks-*.json`
  - scope: effect speed. Draft / final p95 at M12.21: HDR Look 31.1 / 740 ms, Plastic Lens 44.6 /
    367 ms, Dream Glow 16.0 / 287 ms; the caps 16 / 100 ms. Each band blurs its own rows plus the
    blur's reach (`BandBlur`): at HDR's default the reach is about 150 px around a 32-row band,
    and HDR also blurs the fine detail (`HDR_FINE`, about 3 px) at full resolution per band with
    two transposes. Levers: M13.34's `GridBlur`, made once per render, for every blur (the same
    cells and sigmas, so the output should not move); HDR uses only the luma of both blurs, so
    blur one luma channel instead of three (luma is linear: the same picture to the fixed point's
    rounding); HDR's `tanh` roll-off through a table; Plastic Lens's `lens` weights (an `f64`
    `hypot` and two smoothsteps per pixel) by rows, no blur read where its soft weight is 0 (the
    middle, out to `EDGE_FROM`), its grain through M13.34's row sampler; Dream Glow's overlay and
    lift on flat rows.
  - done when: the bench's `hdr`, `plastic_lens` and `dream_glow` drafts within 16 ms and finals
    within 100 ms at p95 on two cores (before and after in the PR, Saves not above their
    budgets); `crates/edit/tests/look_effects.rs`: the `looks-*-large` fingerprints blessed from
    the M12 code match, the existing fingerprints, two-sizes tests and the region test pass
    unchanged; `cargo xtask ci`.
- ✅ **M13.40 `edit`: Soften within the slider's caps**
  - after: M12.R, M13.34
  - files: `crates/edit/src/ops/soften.rs`, `crates/edit/tests/soften.rs`, `crates/edit/tests/fingerprints/soften-*.json`
  - scope: effect speed. Draft / final p95 at M12.21: Soften 30.0 / 539 ms (at its defaults, no
    strokes: the whole photo); the caps 16 / 100 ms. It blurs the masked box at full resolution
    (sigma `Softness/100 × SIGMA`, 30 px on the final) and paints a coverage mask per pixel.
    Levers: M13.34's `GridBlur` for the blur over the masked box (made once per render through
    `apart`, as today); no mask at all without strokes (coverage 1 everywhere, no buffer); with
    strokes, the mask only over the strokes' box; the screen, the fade and the mix on flat rows.
    A brush drag's own drafts (strokes added while painting) are measured too: one long stroke
    on the 24 MP photo, the draft within 16 ms.
  - done when: the bench's `soften` draft within 16 ms and final within 100 ms at p95 on two
    cores (before and after in the PR, its Save not above its budget); `crates/edit/tests/
    soften.rs`: the `soften-whole-large` and `soften-stroke-large` fingerprints blessed from the
    M12 code match, the existing fingerprints, the region test and the M12.28 no-deadlock test
    pass unchanged; `cargo xtask ci`.
- ✅ **M13.41 `edit`: Focal Zoom, Vignette and Pixelate within the slider's caps**
  - after: M12.R, M13.34
  - files: `crates/edit/src/ops/stylize.rs`, `crates/edit/tests/stylize_effects.rs`, `crates/edit/tests/fingerprints/stylize-*.json`
  - scope: effect speed. Draft / final p95 at M12.21: Focal Zoom 78.9 / 2 685 ms, Vignette 8.1 /
    128 ms, Pixelate 7.3 / 117 ms; the caps 16 / 100 ms (Focal Zoom needs 27× on the final).
    Levers:
    - **Focal Zoom:** the zoom blur is `n₁ × n₂` taps evenly spaced in scale made as two passes
      of about √N bilinear taps each (about 45 per pixel on the final, the fine pass over the
      whole window). Split it into three or more passes of about ∛N taps (the same set of
      scales, composed: about 25 taps a pixel), or resample to log-polar about the centre, run
      a box along the radius with running sums and resample back (a constant cost per pixel);
      skip the final pass inside the circle and the fine pass where no later pass reads; bilinear
      taps in `f32` stepping along the ray instead of an `f64` map per tap. The centre stays a
      stored-frame point and a region still equals the whole render.
    - **Vignette:** `fx::to_linear` and `to_srgb` per channel (`powf`) → M13.34's tables; the
      distance by rows in `f32`; the untouched middle skipped by row spans.
    - **Pixelate:** the cell of each pixel by an `f64` division per pixel → a per-row table of
      column cells; in Normal mode at Fade 0 a row span takes its cell's mean by a copy.
  - done when: the bench's `focal_zoom`, `vignette` and `pixelate` drafts within 16 ms and
    finals within 100 ms at p95 on two cores (before and after in the PR, Saves not above their
    budgets); `crates/edit/tests/stylize_effects.rs`: the `stylize-*-large` fingerprints blessed
    from the M12 code match (a Focal Zoom made another way is the case most likely to need a
    look change: then re-blessed with its diff images and the reason, asked at the review), the
    existing fingerprints, two-sizes tests and the region test pass; `cargo xtask ci`.
- ✅ **M13.42 `edit`: Widescreen Film's final and the frames' drafts within the slider's caps**
  - after: M12.R, M13.34, M13.35
  - files: `crates/edit/src/ops/canvas.rs`, `crates/edit/src/ops/mats.rs`, `crates/edit/tests/canvas.rs`, `crates/edit/tests/mats.rs`, `crates/edit/tests/fingerprints/canvas-*.json`, `crates/edit/tests/fingerprints/mats-*.json`
  - scope: effect speed. Widescreen Film's final is 127 ms at p95 (the cap 100 ms): its film
    colour is a few multiplies, but its grain calls `Noise::at` per pixel (an `f64` map and four
    hashes); levers: M13.34's row sampler; the bar rows filled without the per-pixel colour; the
    anti-aliased bar edge only on its one row. The frames' drafts (Border 20.9, Museum Matte
    20.0, Drop Shadow 19.3, Widescreen Film 24.5 ms) are M13.35's: measure them after it and,
    where one still misses 16 ms, make the frame's own painting cheaper (Border's and Museum
    Matte's per-pixel coverage only on the band rows and columns, the photo's opening copied by
    rows; Drop Shadow's profiles, already closed-form, applied by rows), measured at M12.32's
    new Drop Shadow defaults.
  - done when: the bench's `widescreen` final within 100 ms and the drafts of `widescreen`,
    `border`, `museum_matte` and `drop_shadow` within 16 ms at p95 on two cores (before and
    after in the PR, Saves not above their budgets); `crates/edit/tests/canvas.rs` and
    `mats.rs`: the `canvas-widescreen-large` fingerprint blessed from the M12 code matches, the
    existing fingerprints and region tests pass unchanged; `cargo xtask ci`.
- ✅ **M13.47 `edit`: speed margin for every effect: the renderer's band conversion and one home for the shared samplers**
  - after: M13.39, M13.41
  - files: `crates/edit/src/render.rs`, `crates/edit/src/fx.rs`, `crates/edit/src/fx/**`, `crates/edit/src/ops/detail.rs`, `crates/edit/src/ops/canvas.rs`, `crates/edit/src/ops/looks.rs`, `crates/edit/src/ops/soften.rs`, `crates/edit/src/ops/focal.rs`, `crates/edit/tests/fx.rs`, `crates/edit/tests/render.rs`, `crates/edit/README.md`
  - scope: from the effect tasks' reports (M13.36–M13.42). (1) About 33–40 ms of every
    neighbourhood effect's final at 24 MP (6 MP rendered) is `render.rs`'s own band conversion,
    `run_bands`' u8 → f32 read and f32 → u8 write around the passes (a Soften that returns at once
    still takes 40 ms; B&W's final alone is about 33 ms): make it cheap (no per-band allocation,
    loops the compiler vectorises, tables where a branch is now taken per value) with the same
    output bit for bit; `render.rs`'s unit tests of the 8-bit conversions stay exact. (2) The
    grain run sampler copied into `detail.rs` (`GrainPass::noise_run`), `canvas.rs`
    (`FilmGrain::run_values`) and `looks.rs` (Plastic Lens's `Grain`) moves into
    `fx::Noise::row_values` (or a run form beside it), bit-equal to the copies (their equality
    tests exist and move with it). (3) The grid row readers copied into `detail.rs` (`GridRows`,
    Glow), `soften.rs` (`GridRows`), `focal.rs` (`grid_row`) and `looks.rs` (`Lines`, if it is
    the same read) move into `fx::GridBlur::read_row` with the columns' taps worked out once per
    band. (4) `GridBlur`'s weighted means divide each byte by 255 per pixel: a table (the same
    values). (5) `crates/edit/README.md`'s lines for `detail`, `focal`, `looks`, `soften` and `fx`
    brought up to date. `stylize.rs` (M13.41) holds none of the copies today; M13.41 goes first
    so its own work lands on the code this moves.
  - done when: every existing edit fingerprint and `*-large` baseline passes without re-blessing;
    `cargo xtask bench effects` before and after for B&W, Glow, Soften, HDR Look and Widescreen
    Film on `taskset -c 0,1` shows every final at least 15 ms faster at p95 and no effect's draft
    or final slower (both tables in the PR); `cargo xtask ci`.
- ✅ **M13.43 `edit` + `xtask`: every effect's draft and final become budgets**
  - after: M13.35, M13.36, M13.37, M13.38, M13.39, M13.40, M13.41, M13.42, M13.47
  - files: `crates/edit/benches/effects.rs`, `xtask/src/bench.rs`, `xtask/src/accept.rs`, `docs/development.md`
  - scope: `development.md` §8: M12.21 printed the effects' drafts and finals without judging
    them; now that every effect meets the caps, they join the budgets. Measure all 27 (the median
    of three runs, release, `taskset -c 0,1`) and set each effect's draft and final budget at
    1.5 × its measurement, never above 16 ms and 100 ms (drafts rounded up to 0.1 ms, finals to
    1 ms), in the bench's `BUDGETS` and in `EFFECT_BUDGETS` in `xtask/src/bench.rs`;
    `judge_effects` judges every `<effect> draft` and `<effect> final` row's p95 with the Saves
    and the empty recipe (a planted slowdown added per render), and `--check` in the bench does
    the same; the `effects` row of `BUDGETS` in `xtask/src/accept.rs` and §8's table and prose
    say the caps are now judged (drop "printed, not budgeted" and "not yet joined"); the
    numbers in §8 and §13 with the machine. A budget is never set above a cap: an effect that no
    longer meets its cap on the day goes back to its module's task. `xtask/src/bench.rs` and
    `docs/development.md` are also in M13.31's files: the board runs the two one after the other.
  - done when: `cargo xtask bench effects` prints all 27 within their new budgets (in the PR);
    `cargo xtask budgets` passes with them, and `--plant-ms 17` makes every draft miss and
    `--plant-ms 101` every final (the `xtask` tests: the judge with a recorded report and a
    planted slowdown); `cargo xtask ci`.
- ✅ **M13.44 `engine`: effect tiles of an already framed photo show the whole picture, and a frame reads at tile size**
  - after: M12.R
  - files: `crates/engine/src/effect_tiles.rs`, `crates/engine/tests/effects.rs`, `docs/editing.md`
  - scope: a fix task from the M12.R review screenshots (after M12.29 #794). M12.29 fits a
    size-changing effect's tile in its box, but every other tile still renders at the covering
    scale and cuts around the centre, so once a photo's recipe already holds a frame (Border,
    Drop Shadow, Museum Matte, Widescreen Film) the tiles of tabs 3–5 crop that frame away and
    show it only at two sides. Fit every tile whose rendered picture includes a frame (the recipe
    so far or the effect itself; `Geometry::frames()`), on the gray 38 placeholder. A default
    Border is 20 + 5 thousandths of the shorter side, about one pixel at tile size, so its tile
    looks like the plain photo: draw a frame effect's tile with its bands at least 2 px each at
    tile size (the tile alone; the effect's own render is unchanged) and say so in `editing.md`
    §8.1. Drop Shadow's tile follows the same rule for its shadow's reach.
  - done when: `crates/engine/tests/effects.rs`: on a photo with a Border in its recipe, Sepia's
    and Boost's tiles show the frame's colour at the middle of all four sides; a default
    Border's tile shows its outer band at least 2 px wide on every side; tiles of photos without
    a frame are byte for byte unchanged; `cargo xtask ci`.
- ✅ **M13.46 `engine`: the viewer star test compares the starred photos as a set**
  - after: M12.R
  - files: `crates/engine/tests/viewer.rs`
  - scope: a flake from a task gate (passed on three reruns):
    `viewer::starring_a_photo_of_an_unwatched_folder_adds_it_as_scan_once_and_one_undo_removes_both`
    got `["IMG_0003.jpg", "IMG_0002.jpg"]` where it wants `["IMG_0002.jpg", "IMG_0003.jpg"]`. The
    cause is the test, not the product: it collects the starred names from
    `Library::photos_in`, which is the folder's display order (`organizing.md` §10: sort date, then
    name, then id), and the test's photos have no capture date, so their sort date is the file
    time in whole seconds. `downloads()` writes `IMG_0004` … `IMG_0001` in reverse, so whenever a
    second boundary falls between writing `IMG_0003` and `IMG_0002` the folder's order (rightly)
    puts `IMG_0003` first. Reproduced: 0 of 200 plain runs fail (the test takes about 10 ms), 6 of
    40 fail when started 5 ms before a second boundary, every failure this exact one. The
    product's orders are deterministic: the viewer's list sorts by name (`viewer.rs`) and the
    display order is a total order. Fix the test without weakening it: compare the starred names
    sorted (which photos are starred is what it proves; the display order is the `library`
    crate's to test), and pin the four photos' file times with `File::set_modified` as
    `recently_updated.rs` does, `IMG_0003` a second older than `IMG_0002`, so the order that used
    to flake is the one every run takes. The viewer's own name-order checks stay as they are.
  - done when: the test, with the pinned file times and the old list comparison, fails every run
    (said in the PR), and with the fix passes 200 runs in a loop under `taskset -c 0` and 40 runs
    started just before a second boundary; `cargo xtask ci`.
- ✅ **M13.48 `engine` + `ui` + `mcp` + apps: people follow-ups: the automation permission, person search behind it, an exact person filter, faces shown through the whole geometry**
  - after: M13.18, M13.24, M13.25, M13.29
  - files: `apps/dalica/src/main.rs`, `apps/dalica/src/control/mod.rs`, `apps/dalica-cli/src/mcp.rs`, `crates/ui/src/control/mod.rs`, `crates/ui/src/control/engine.rs`, `crates/ui/src/control/tests.rs`, `crates/mcp/src/connect.rs`, `crates/mcp/src/headless.rs`, `crates/mcp/tests/organize_tools.rs`, `crates/engine/src/automation.rs`, `crates/engine/src/search.rs`, `crates/engine/src/cmd/search.rs`, `crates/engine/src/cmd/library.rs`, `crates/engine/src/cmd/faces.rs`, `crates/engine/tests/people.rs`, `crates/library/src/search/matcher.rs`, `crates/library/src/search/index.rs`, `crates/library/tests/people.rs`, `crates/ui/src/commands/people_pane.rs`, `crates/ui/src/commands/search.rs`, `crates/ui/src/panels/search_banner.rs`, `crates/ui/src/panels/edit/faces.rs`, `crates/ui/tests/people_pane.rs`, `docs/control-protocol.md`, `docs/mcp.md`, `docs/organizing.md`, `docs/data-model.md`, `docs/parity.md`
  - scope: follow-ups reported by M13.15, M13.21, M13.24 and M13.29. (1) **The permission in the
    app:** `dalica --control … --automation-allow-people` (flag only, no environment variable, as
    `--automation-allow-external`; it needs `--control`) reaches the control channel's `Config`,
    and `engine.execute` and each batch step answer through a `Policy` built from it instead of
    `Policy::default()`; `dalica-cli mcp --connect` passes its own flag to
    `Connect::allow_people` (names reach the client only when both have it, `mcp.md` §5).
    (2) **No guess-oracle** (`faces.md` §7): today `library.query {text: "person:Ana"}` and
    `query_photos {query: "person:Ana"}` answer without the permission, and plain words match
    people's names too (`organizing.md` §8.1), so refusing `person:` alone would not close it.
    Decision: without the permission an automation surface runs every search with people's names
    left out of the match: `person:` matches nothing and is listed in `unavailable` (as in a
    library without people), plain words skip names and nicknames, everything else matches as
    before; nothing is refused, so a reply never tells a guessed name from an unknown one. The
    engine's `Policy` sets this on the search commands' params (`library.query`, `search.set`; a
    list beside `PEOPLE_REPLIES`), once for the control channel, headless MCP and, again, connect
    mode; a window's own `search.set` always matches names (the switch is per call, never kept in
    the session's search). The exact filter by person id (3) stays allowed: ids are not names and
    `people.list` already gives them. (3) **An exact person filter:** the search `Query` takes
    `people: [PersonId]` (photos holding a face of every one of them; a capped list, an
    unknown id `not_found`), in `search.set`, `library.query` and `library.inspect`'s search; the People
    pane's `people.filter` uses it instead of `person:` words, so selecting Ana no longer finds
    Anabel's photos; the search field stays as typed, the banner says "Showing photos with Ana and
    Rui" while the people filter is on, and Show All clears it (`organizing.md` §6.4).
    (4) **Faces shown through the whole geometry:** `faces.list`'s `displayed` and `faces.add
    {displayed}` go through the recipe's `edit::Geometry` (rotations, flips, straighten and crop,
    normalised to the photo as shown, frames left out; past 0..1 where the crop cuts a face), not
    the turn alone; the edit view's face boxes and Add a Person Manually use that mapping (one
    implementation in the engine; `panels/edit/faces.rs`'s own `Shown` mapping goes).
    (5) `Headless::open_with` gives its session `faces_scan::default_models_dir()` (M13.18), as
    the app and the CLI do. (6) `data-model.md` §4 lists `people.askBeforeIgnoring` (M13.21) under
    `settings.json`'s `people`, and §5's status line says people are written (M13.19). Rows
    `SYS-MCP` (its "missing" note) and `PPL-PANE`; `control-protocol.md` §1's flag table and §6.4
    (the "no flag yet" sentence goes; the search rule), `mcp.md` §5 and the `query_photos` row.
  - done when: `crates/library/tests/people.rs`: `people: [Ana]` finds Ana's photos and not
    Anabel's, two people find the photos holding both; `crates/engine/tests/people.rs`: through a
    `Policy` without the permission `library.query` with `person:Ana` finds nothing and lists
    `person` unavailable, the plain word `Ana` finds no photo by her name, `people: [Ana]` still
    finds hers, and with the permission all three find them; `faces.list`'s `displayed` on a photo
    cropped and straightened is where `Geometry` shows the face and `faces.add {displayed}` stores
    it back within 10⁻⁹; `crates/ui/src/control/tests.rs`: with `allow_people` `faces.list` over
    the channel carries names, without it names are `null` and `person:Ana` finds nothing;
    `crates/mcp/tests/organize_tools.rs`: headless `query_photos {query: "person:Ana"}` finds her
    photos only with the permission; unit tests: the app parses the flag (and refuses it without
    `--control`), `dalica-cli mcp --connect --automation-allow-people` builds a `Connect` that lets
    names through, the headless session has the default models folder; `crates/ui/tests/people_pane.rs`:
    with Ana and Anabel in the library, selecting Ana in the pane shows only Ana's photos, and
    the existing pane tests and fingerprints pass unchanged on the engine's mapping; `cargo xtask ci`.
- ✅ **M13.49 `ui` + `engine`: people names and face points withheld from automation without the people permission**
  - after: M13.30, M13.48
  - files: `crates/ui/src/control/people.rs`, `crates/ui/src/control/mod.rs`, `crates/ui/src/control/view.rs`, `crates/ui/src/control/menu.rs`, `crates/ui/src/control/tests.rs`, `crates/engine/src/automation.rs`, `crates/engine/src/cmd/slideshow.rs`, `crates/engine/tests/people.rs`, `crates/ui/src/selection.rs`, `crates/ui/src/commands/select.rs`, `crates/ui/src/commands/slideshow.rs`, `crates/ui/src/commands/export.rs`, `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/light_table/suggestions.rs`, `crates/ui/tests/suggestions_ui.rs`, `docs/control-protocol.md`, `docs/organizing.md`
  - scope: fixes from the maintainer's answers at the M13.R review prep (2026-10-18). M13.48 closed
    the engine's replies and searches; the window's own surfaces still give names away, and a
    person's selection takes in photos that are only suggested. (1) **Names in the window's
    replies:** without `--automation-allow-people` (`Config::allow_people`, M13.48) `ui.inspect`,
    `ui.state`, `ui.widgets` and `ui.menu.tree` carry no person's name or nickname: a fixed
    placeholder stands where the name was (the rest of a label, a count for one, kept) and every
    widget id stays, so a client still finds, clicks and drags by id. That covers the People
    group's rows (`tree:person:<id>`, "Ana (12)"), a person section's `/title`, face tiles and
    their "Ana?" (`face:…`, `face:…/in/<id>`), the People pane's rows and Suggestions, the edit
    view's face boxes (`facebox:…`), the Unnamed grid's name fields and the people they offer
    (`field:faces.name/<n>/option/<k>`), the Add a Person Manually field, the menus that list
    people (Move to Person ▸ `menuitem:faces.moveToPerson/<id>`, Merge Into ▸
    `menuitem:people.merge/<id>`, in `ui.menu.tree` and in `ui.inspect`'s `popup`), and the texts
    with no person id of their own: the search banner while the People pane's filter is on
    ("Showing photos with Ana and Rui"), a face drag's pill, the people dialogs' title and fields
    (`dialogs/people.rs`, `new_person.rs`, a confirm naming a person), the tooltip over a people
    widget, and `ui.state`'s typed name drafts (`unnamed.names`, the manual box's name).
    `ui.menu.invoke {path}` matches a person's row by the placeholder only, never by the name
    (by `id` and `params` it works as before). One place in the control layer
    (`control/people.rs`, as `automation::PEOPLE_REPLIES` is one list), keyed by widget id, role
    and dialog kind, never by matching the reply's text against the library's names: a folder or
    caption renamed to a guessed name would then come back changed, a guess-oracle (`faces.md`
    §7). Screenshots still show what the window shows (§11). With the permission every reply is
    unchanged. (2) **The slideshow's face points:** `slideshow.list`'s `focus` points at a face
    (`face` its id, `point` its centre), which tells where a face is. Without the permission
    automation gets each photo's focus as the photo's centre `[0.5, 0.5]` and no `face`: the
    engine's `Policy` rule beside `PEOPLE_REPLIES` (one rule for the control channel, headless MCP
    and connect mode); the window's own show still moves toward the face. No automation reply
    carries a running show's `Frame::focus` or `outgoing_focus` today (`ui.state`'s
    `slideshow.pan` is the viewer's own pan); one that comes to follows the same rule.
    (3) **A person's selection is their own photos:** clicking a person's row (and
    `select.section` on `person:<id>`) selects the section as a unit, and since M13.30 the section
    starts with the photos only suggested for them, so the tray's token, Play and Export from that
    selection took in photos not yet confirmed. The whole-section selection of a person's section
    is now the photos holding a confirmed face of theirs (in the section's order); the suggested
    cells stay in the section, unselected, and a click on one still selects it as any cell. With
    Show only suggestions on, the row selects none (the section stays the selection's section).
    The header's ▶ and Folder ▸ View Slideshow on a person's section play their confirmed photos
    only, as `slideshow.list {section}` gives them. `organizing.md` §6.3 says so;
    `control-protocol.md` §6.4's withheld lines, §6.6–§6.7 and §11's privacy line say what is
    withheld from the window's replies and from `slideshow.list`.
  - done when: `crates/ui/src/control/tests.rs`: with Ana named, her section shown, the People
    pane open and the face menu's Move to Person ▸ listed, without `allow_people` no reply of
    `ui.inspect`, `ui.state`, `ui.widgets` or `ui.menu.tree` contains "Ana" while the same widget
    ids are listed, `ui.menu.invoke` by a path naming Ana is refused as an unknown item, and with
    `allow_people` the labels read "Ana" (fails without the fix);
    `crates/engine/tests/people.rs`: through a `Policy` without the permission `slideshow.list`'s
    focus of a photo with a named face is `[0.5, 0.5]` with no `face`, and with it the face's id
    and centre (fails without the fix); `crates/ui/tests/suggestions_ui.rs`: Ana with two
    confirmed photos and one suggested: clicking her row selects the two, the suggested cell still
    shows, the tray token counts 2, Play plays the two and Export offers two photos (fails without
    the fix); the existing control, people and suggestion tests pass; `cargo xtask ci`.
- ✅ **M13.50 `xtask`: the task gate also tests the packages that depend directly on a changed package**
  - after: M12.R, M13.17
  - files: `xtask/src/ci.rs`, `docs/development.md`, `AGENTS.md`
  - scope: the maintainer's answer at the M13.R review prep (2026-10-18). Twice in M13 a dependent's
    test broke `main` because `cargo xtask ci` tests only the changed packages: M13.4 broke
    `dalica-cli`'s `commands_list_the_openers_keys`, M13.14 broke `dalica-ui`'s
    `view_state::the_new_menu_items_are_listed_and_greyed`. The task gate now also runs, at quick
    strength, the tests of every workspace package that depends **directly** on a changed package
    (a normal, dev or build dependency, target-specific ones included; transitive dependents are
    not added), read from the members' manifests (or `cargo metadata --no-deps`; whichever is
    simpler to test without the network). Decision (the lead's, reversible): `dalica-acceptance`
    is never added as a dependent (it dev-depends on most crates, and its tests run at the review
    and when its own files change), nor is `xtask`; a dependent added this way runs as a changed
    package does (`stats --check` with `dalica-mcp`). The gate prints the changed packages and the
    dependents it added on separate lines. The root `Cargo.toml`, lock file, toolchain and
    outside-every-package rules (test everything) and the docs-only rule (test none) stay as they
    are; `--merge` and `--full` are unchanged. `development.md` §3 (the gates table and *Which
    packages a task's gate tests*: "Dependents of a changed package are not tested" goes) and §6
    where it says which tests a task runs, `ci.rs`'s module doc, and `AGENTS.md` §5's sentence
    "Dependents are not tested; a behaviour break there surfaces at the review" (the
    maintainer's own decision, so this task edits `AGENTS.md`) say that direct dependents are
    tested and transitive ones surface at the review. `docs/development.md` is also in M13.17's
    files: the board runs the two one after the other.
  - done when: `xtask/src/ci.rs`'s tests of the selection: a change in a library crate selects it
    and its direct dependents and not their dependents (a synthetic member graph: `core` →
    `library` → `engine` → `ui`; a change in `library` selects `library` and `engine`, not
    `ui`), never `dalica-acceptance` or `xtask` as a dependent, a docs-only change still tests
    none, the root `Cargo.toml` and lock-file rules still test every package, and the real
    workspace's direct dependents of `dalica-engine` include `dalica-ui`, `dalica-mcp`, `dalica`
    and `dalica-cli`; `cargo xtask ci` on a branch changing only `crates/engine` runs the tests of
    `dalica-engine`, `dalica-ui`, `dalica-mcp`, `dalica` and `dalica-cli` (its output in the PR).
- ✅ **M13.32 `accept M13`**
  - after: M13.17, M13.23, M13.27, M13.28, M13.29, M13.30, M13.31, M13.43, M13.44, M13.46, M13.48, M13.49, M13.50
  - files: `tests/acceptance/m13.rs`, `tests/acceptance/m13_*.rs`, `tests/acceptance/m13.md`, `tests/acceptance/fingerprints/m13-*.json`
  - scope: every *Done when* of M13's three areas in the real `dalica --headless` over the control
    channel and through `dalica-cli mcp`: a synthetic model installed from a local file after the
    licence dialog, the face scan filling Unnamed in the background while the window keeps its
    frame budget; a group named as one undo step that survives a restart and an index rebuild;
    removed and ignored faces staying away on a rescan; suggestions above the threshold and the
    margin, confirmed and rejected; a face drawn by hand on a turned photo; merge, rename and
    delete as one step each; a named face's region in the file with Store people names in photos
    on; photos dragged onto the map geotagged as one step, the GPS round-trip and Clear Location;
    no network without the user's action (a loopback listener as the tile server and a
    `catalog.json` model's site counts no connection at start-up, during a scan or with the pane
    open and tiles off, and connections only once allowed and open, or once the licence is
    accepted); a `trace` log with no people data; W13 and W14 over MCP; the corpus floors (with
    `cargo xtask test-corpus` at the review); the M13 budgets (the release budgets with
    `DALICA_ACCEPT_RELEASE=1`; inside `cargo xtask ci --full` a smaller debug run checks the paths
    and prints its times); whole-window fingerprints of the Unnamed grid, a person section, the
    People pane and the Places pane; the manual checks in `m13.md` (the chosen models downloaded
    for real and run on a few real photos, naming and suggestions on a real library, the map
    dragged and zoomed with a mouse and a touchpad, tiles allowed, a geotagged photo opened in
    another application's map).
  - done when: `cargo xtask accept M13` passes everything in M13's *Done when*.
- ✅ **M13.33 Lead: task blocks for M15**
  - after: M12.23
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M15 task block from M15's outline rows and the spec (`viewing.md` for video,
    `localization.md`, `releasing.md`, the OS-integration parts of `organizing.md` and
    `viewing.md` §3), with waves and the `M15.R` release gate (owner: maintainer; Release 1.0);
    tasks that hinge on reserved decision 2 (video) wait for the maintainer's answer, asked at the
    review before M15 (M14.R), never guessed; a block that can port Craft-app code names the source
    files in its scope (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists the new
    M15 tasks as planned.
- ✅ **M13.51 `faces` + `ui`: fixes found by accept M13: the two benches' debug runs, the scan's pace from the window, a model file's path under the read roots**
  - after: M13.32
  - files: `crates/faces/benches/inference.rs`, `crates/faces/src/synthetic.rs`, `crates/ui/benches/people_places.rs`, `crates/ui/src/user_activity.rs`, `crates/ui/src/app.rs`, `crates/ui/tests/face_scan_pace.rs`, `crates/ui/src/control/engine.rs`, `crates/ui/src/control/tests.rs`, `crates/mcp/src/headless.rs`, `crates/mcp/tests/roots.rs`, `docs/control-protocol.md`, `docs/security.md`
  - scope: four bugs M13.32 (`accept M13`, #908) found. (1) **The `inference` bench's debug run:**
    without `--bench` it runs at a 160 px detector side, a 10-column stride-16 grid, and the second
    planted face (`PLANTED`, column 18) falls outside it, so the network reports one face and the
    run's `assert_eq!(found, PLANTED.len())` fails. The faces are planted where every side the
    bench accepts holds them (scaled to the side's grid, or a smoke side whose grid holds both),
    and a planted cell outside the grid fails with a message naming the side, never silently
    (an additive check in `dalica_faces::synthetic` if the bench cannot see the grid; the
    `yunet_shaped` signature stays: `crates/engine` calls it). The release run at 640 px and its
    budgets are unchanged. (2) **The `people_places` bench's debug run:** its 1 000 photos (and
    3 000) make so few Unnamed groups (sizes 1 500 ÷ k) that the grid fits the window, so the
    wheel moves nothing and "the wheel scrolled the grid" fails. The planted groups scale with
    `--photos` (or the smoke library is one the grid overflows) so every library the bench
    accepts scrolls; the assert stays, the release rows at 10 000 photos and their budgets are
    unchanged. (3) **The window reports the user's input to the face scan:** nothing in
    `crates/ui` or `apps/` calls `Session::user_activity`, so the scan never holds back while the
    user drags, types or scrolls (`faces.md` §4). A new `crates/ui/src/user_activity.rs` reads
    each frame's input once (`App::frame`, before the panels take the keys) and reports one
    `UserActivity`: `Input` for a key, typed text, a wheel, a pointer button down or a drag;
    `Pointer` for a pointer move alone; `Away` while the viewport is minimised, or with no input
    while the People view is shown (Unnamed or a person's section); else `Idle`. Reported on
    change and at most once a second while unchanged (the session notes the time; the per-frame
    cost is one input read). Input that automation injects (the control channel's clicks, keys
    and wheel) counts as the user's: the window cannot tell them apart, and the scan only
    slows. (4) **A model file's path under the read roots:** `faceModel.addFile {path}` (M13.27)
    and `faceModel.open {file}` are UI commands with no effect, and the face model dialog starts
    inspecting the file at once, so a control client reads any `.onnx` on the machine;
    `folders.add` and `import.openSource` are checked against the automation read roots
    (`control/engine.rs`'s `READ_PATHS`, `control-protocol.md` §5.7, `security.md` §6.6) and these
    are not. Both join `READ_PATHS` (the picker's answer is checked already, `control/picker.rs`),
    and so do the engine's `faces.models.inspect {path}` and `faces.models.install {path}`
    (external-gated, but once allowed they read any path too), there and in headless MCP's
    `check_paths` (`crates/mcp/src/headless.rs`), which today checks only `folders.add` and
    `folders.set`. Decision (the lead's, reversible): a model file is read as a folder to add is
    (inside a read root, a client root or a watched folder; none given: refused, except
    `--demo`); the window's own Add a Model File… and a dropped `.onnx` are the user's and stay
    unchecked. `control-protocol.md` §5.7's Reading list and `security.md`'s path-bearing
    commands (§6.6) name the model file.
  - done when: `cargo test -p dalica-faces --bench inference -- --json` and `cargo test -p
    dalica-ui --bench people_places -- --json` pass (each failed before the fix; outputs in the
    PR); `crates/ui/tests/face_scan_pace.rs` (headless, a `VirtualClock` session with a synthetic
    model planted as `suggestions_ui.rs` does): while a wheel, key or drag arrives every frame the
    scan starts no photo and `face_scan().activity()` holds `Input`; a pointer move alone reports
    `Pointer`; frames with no input for three seconds of the clock report `Idle` and the scan finds
    the faces; minimised reports `Away` (fails without the fix); `crates/ui/src/control/tests.rs`:
    a control client whose read roots do not hold the file is refused `faceModel.addFile {path}`
    with `path_denied` and no dialog opens, and with the file under a read root the dialog opens
    on it (fails without the fix); `crates/mcp/tests/roots.rs`: headless MCP with
    `--automation-allow-external` refuses `faces.models.inspect {path}` outside every root with
    `path_denied` (fails without the fix); `cargo xtask ci`.
- ✅ **M13.R Release 0.8 gate**
  - after: M13.32, M13.51
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 0.8): the lead runs `/milestone-review M13.R` to prepare it, with
    the version set to 0.8.0 and its `whats-new.md` section on the review branch, the release
    checklist and the sign-off question, and after the merge cuts and publishes 0.8.0
    (`releasing.md`); the check-in page (open Options ▸ People, read the model terms, tick the box
    and download the models, let the face scan run on a real library while scrolling, open
    Unnamed, name a group, make a New Person, ignore a group and show ignored faces, drag faces
    onto a person, open a person's section with Zoom to Faces, rename, merge and delete a person
    and undo each, confirm and reject suggestions and use Find More, open the People pane on a
    group photo, hover its faces, add a person by hand in the edit view, filter by two people, turn
    on Store people names in photos and check a named file in another application; open the Places
    pane, drag photos onto the offline map, search for coordinates, allow map tiles and search for
    a place, clear a location, copy and paste one, open a geotagged photo in another
    application's map; written as steps for the maintainer to try); the questions: the planning
    decisions above that change behaviour (grouping recomputed, not stored; the scan only with a
    recogniser installed; the offline map from Natural Earth; tiles only while the pane is open
    and after its question; proxy from the environment or manual; Delete Person sends faces back to
    Unnamed; a drop of photos on a person does nothing), anything the reserved decisions left open,
    the look of the People group, the Unnamed grid, the person sections, the People pane, the face
    boxes, the Places pane and the map (approved with their fingerprints); *Where we stand* and the
    `status.md` numbers (faces embedded per second, the frame time during the scan, the people
    views with 10 000 faces, the Places pane with 10 000 photos, the corpus floors, the workflow
    pass rate) on a `review/M13.R` branch, a pull request open. Only the maintainer merges that pull
    request and closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M13.1 geo + model  ‖  M13.2 sidecar fields  ‖  M13.3 engine modules + settings
         ‖  M13.4 ui state + stubs  ‖  M13.5 ui-kit face widgets + icons  ‖  M13.6 ONNX interpreter
         ‖  M13.7 grouping + suggestion maths  ‖  M13.8 fetch core
         (once M12.R is closed)
wave 2   M13.9 store + embeddings cache  ‖  M13.10 detector, embeddings, models (5)
         ‖  M13.11 MWG regions  ‖  M13.12 ui-kit map  ‖  M13.13 HTTPS transport (9)
         then M13.45 faces, geotags + people read on scan (after M13.9)
wave 3   M13.14 geotags  ‖  M13.15 faces + people data  ‖  M13.16 face models (5, 9)
         ‖  M13.17 face corpus + floors (5)
wave 4   M13.18 face scan  ‖  M13.19 names in files  ‖  M13.20 tiles + place search (4, 9)
         ‖  M13.21 Unnamed grid  ‖  M13.22 Places pane  ‖  M13.23 faces in crops + Pan and Zoom
         ‖  M13.24 People pane + face boxes
wave 5   M13.25 People tree + person sections  ‖  M13.26 suggestions  ‖  M13.27 model dialog + Options ▸ People
         ‖  M13.28 Options ▸ Network + tiles  ‖  M13.29 W13, W14
wave 6   M13.30 suggestions in the window  ‖  M13.31 M13 budgets  ‖  M13.48 people follow-ups
         then M13.49 names withheld + a person's own selection (review fixes, after M13.48)
         ‖  M13.50 the gate tests direct dependents (after M13.17: shared docs/development.md)
wave 7   M13.32 accept M13, then M13.51 its fixes → M13.R   (M13.33 lead: M15 blocks, any time after M12.23)

speed lane (effect speed, from M12.R; the board fits it between the feature waves, whose files
it never shares except M13.43 with M13.31):
speed 1  M13.34 fx: box passes, grid blur, noise rows, tables  ‖  M13.35 engine: frames' drafts from the base
speed 2  M13.36 detail  ‖  M13.37 focal  ‖  M13.38 remap + colour  ‖  M13.39 looks  ‖  M13.40 soften
         ‖  M13.41 stylize  ‖  M13.42 canvas + mats (also after M13.35)
speed 3  M13.47 band conversion + shared samplers (after M13.39, M13.41)
speed 4  M13.43 effect budgets (after M13.31 on the board: shared files) → M13.32
```

### M14 — Collage and legacy sidecars

One milestone in 2 feature areas (*The collage room*, *Legacy sidecars*); one `cargo xtask accept M14` covers every *Done when* below. It ends with a release gate the maintainer owns (Release 0.9).

#### The collage room

- **In:** the Collage room (`creating.md` §3, `ui-layout.md` §8): opened from the Photo Tray's
  Collage button, a section header's Collage (folders, albums and, since M13, person sections) and
  Create ▸ Picture Collage…; the six styles (Picture Pile, Mosaic, Frame Mosaic, Grid, Contact
  Sheet, Multiple Exposure) with the options each offers (picture borders none / white / instant,
  grid spacing, a solid background colour with its swatch and eyedropper or a picture as the
  background, the page format from the crop ratios plus Current Display with its orientation and
  deletable custom ratios, Draw shadows, Show captions); the Clips tab (+, −, Get More and the
  tray's Back to Collage); the canvas (select, drag to move in a pile, the wheel handle that turns
  and scales, Bring to Top / Send to Bottom, drag to swap in the mosaics and the grid, Delete back
  to the clips, Set as Background, Set as Frame Center, Scramble Collage, Shuffle Pictures, View
  and Edit); Create Collage (a JPEG of long side 3000 px, or the display's size, in
  `Pictures/Dalica/Collages` with a `.dalica-collage` recipe beside it, opened in the edit view),
  Reset, Close with [Save Draft] [Discard] [Cancel]; Edit Collage in the edit view's top bar; the
  Collages folder in the Projects group; collages over MCP (W15).
- **Out:** the room's Desktop Background button (setting the wallpaper is dropped with the other
  OS-desktop features, `creating.md` intro); printing a contact sheet (`OUT-CONTACT`, dropped);
  collages of videos (M15).
- **Reuse** (`architecture.md` §9): LightCraft `crates/engine/src/print/layout.rs` (`Rect`,
  `grid_rects_for`) and `crates/engine/src/print/compose.rs` (`place`, `px_rect`) at 20ccedf5
  for the grid, the contact sheet and placing a picture in its cell (the mosaics are our own); in this repository
  `create::output` (each picture rendered with its edits), `create::watermark` (text drawn with
  Inter, for captions and names), M12's drop shadow (`edit`) for shadows, M12.4's colour swatch,
  picker and eyedropper, M11.3's room tabs and M7's crop ratio list and custom ratios.
- **Done when:** `cargo xtask accept M14`: every style lays the same pictures out the same way for
  the same seed on every run and system, and Scramble and Shuffle change only what they say; the
  canvas and the created JPEG agree (each picture where the canvas showed it); Create Collage
  writes a JPEG of the page's ratio at long side 3000 px and its recipe, never replacing a file,
  and the collage shows in the Projects group; Edit Collage reopens it with its clips and
  settings; nothing in the room ever waits on the disk; W15 passes over MCP.

#### Legacy sidecars

- **In:** reading the previous organizer's per-folder sidecar and its hidden originals folder
  (`data-model.md` §9): the read-only reader with its provenance header, limits and fuzz target;
  stars, hidden, captions and tags, rotations and flips, crops, edits (mapped to Dalica operations
  or kept as `legacy:` entries), faces and people, album memberships and definitions, geotags, and
  the folder's description, date, place and hidden category, shown on the first scan with no
  import step; which file is in effect per folder (Folder Properties, the control API); the first
  change in a folder writing a full `.dalica.json`; the legacy originals offered for Undo Save and
  Revert; the first-run notice; Options ▸ General ▸ "Read photo data left by the previous
  organizer"; the edit panel's "1 edit from the previous program (not supported)"; a legacy
  library over MCP (W16).
- **Out:** the previous organizer's central database, its contacts and album files in its
  application-data folder, and its thumbnail caches (only files inside watched folders are read;
  `security.md` §5.5); ever writing, renaming or deleting a legacy file; the web-album, backup and
  upload keys (no meaning in Dalica; the legacy file keeps them).
- **Reuse:** the format as recorded in the private notes (`notes/`, the data-and-files study:
  public descriptions and black-box observation; nothing copied); in this repository `dalica-fs`
  reads, `scan`'s sidecar path, `store`'s writer, M8's Undo Save and Revert (`create::save`), M9's
  albums, M13's faces, people and geotags.
- **Done when:** `cargo xtask accept M14` (`data-model.md` §10.11): a folder with only a legacy
  sidecar shows its stars, captions, edits, albums, people and places after the first scan; the
  first change writes `.dalica.json` with all of it while the legacy file stays byte-identical; a
  folder with both files reads only `.dalica.json`; a legacy-saved photo's original comes back with
  Undo Save and Revert; hostile legacy files are refused or skipped without a panic; W16 passes
  over MCP.

#### M14 tasks

M14 makes collages and reads the previous organizer's data: the collage document, its six
layouts, its render and recipe, the collage session and its room; the legacy reader, its edit
mapping, legacy folders on scan, the first write that converts them, legacy originals for Undo Save
and Revert, and the notice and settings in the window. Its first wave lists the M13 work each task
extends and starts once the maintainer has closed M13.R. Seven tasks lay the ground the others
share, so later tasks never share a file: M14.1 gives `library` the Collages project, the legacy
ids of albums and people and the per-folder legacy facts; M14.2 gives `sidecar` the bounded,
read-only legacy reader; M14.3 gives `edit` the mapping of legacy edit operations; M14.4 gives
`create` the collage document, its page formats, its recipe file and one stub module per layout;
M14.5 registers one engine command module per area with stub state, M14's settings and the options
row; M14.6 adds M14's view state, UI commands with stub handlers and the stub room; M14.7 gives
`ui-kit` the collage widgets and icons. The layouts (M14.8, M14.9), the render (M14.10), legacy
folders on scan (M14.11), the first write (M14.12) and legacy originals (M14.14) follow, each in its
own module; then the engine's legacy session (M14.13) and collage session (M14.16) and output
(M14.17), and the window's room, canvas, entry points and legacy surfaces on top. Each task updates
its `parity.md` rows (`OUT-COLLAGE`, `DATA-LEGACY`, the Collage tab of `LIB-ROOMS`, the Collage
buttons of `LIB-ACTIONS`, `LIB-LT-HEADER` and `PPL-UNNAMED`, the collages of `LIB-TREE-GROUPS`, the
legacy notice of `ORG-FIRSTRUN`, the legacy row of `PREF-GENERAL`, §10.11 in `DATA-TESTS`,
`SYS-MCP`, the Create menu row) and names its commands there; first fingerprints of the six styles'
renders, the room, the canvas, the legacy notice and the new widgets and icons are approved in bulk
on the M14.R review page.

Whole-window baselines belong to no task, as in M5–M13: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m13-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.
Fingerprints are made at 1400 × 900 only; at 900 × 600 a test asserts that the room, its tabs and
its canvas fit (`architecture.md` §11).

Decisions taken while planning (the check-in lists them for reversal). **The collage document**
(`create::collage`) is pure data: the style, the settings, the page format and orientation, a
`u64` seed, the clips (photo ids in order, which of them are on the page), the background picture
and the frame centre, and per picture the user's own placement where the user moved it (a pile's
centre, scale and angle; the mosaics' and the grid's slot swaps). Every layout is a pure function
of the document to placements normalised to the page (centre, size, angle, stacking order, the
part of the picture shown), seeded by a small generator of our own (no `rand` dependency, no
floating point in the choices), so the canvas, the render, a thumbnail of the room and the MCP
reply agree at any size and on every system. Scramble Collage draws a new seed and drops the
user's placements; Shuffle Pictures keeps the slots and draws a new assignment of pictures to them.
The pile shows whole pictures turned within ±15°; Grid, Mosaic and Frame Mosaic fill their cells
(the picture cropped to the cell, centred); Contact Sheet shows whole pictures with their file names
under them; Multiple Exposure covers the page with every picture and blends them with equal weight
(it has no background). Grid spacing runs from none to 5 % of the page's short side. Borders are
relative to the picture's short side (white 3 %, instant 5 % with a 20 % bottom). The default is
Picture Pile on a black background at 16:9 (HD video), landscape, shadows on, captions off.
**Clips:** opening the room puts every picture on the page and in the Clips; Remove and Delete
take a picture off the page (it stays in the Clips), + puts it back, − takes it out of the Clips;
Get More shows the Library tab and the tray's Back to Collage adds the selection (or the held
tray) to the Clips. One Collage room at a time: opening it again brings it to the front and adds
the new pictures to its Clips. At most 1 000 clips. **The room's history:** Ctrl+Z and Ctrl+Y
undo and redo the room's changes while it is in front; they never reach the library's undo stack.
**Page formats** are the crop ratios (`editing.md` §6.1) plus Current Display, sharing the Crop
panel's custom ratios (deleting one in either place deletes it in both). **Output:** Create
Collage renders on a worker (`JobKind::Collage`, progress and Cancel) through the one output
pipeline: each picture with its edits at the size its place needs, one at a time so memory stays
within the page and one picture; a JPEG at quality 92 (sRGB, the date of making as its date) named
`Collage <yyyy-mm-dd>.jpg` through `core::names::sanitize` and `-001` collisions, never replacing a
file, in the projects directory's `Collages` folder (marked `"project": "collages"`), with the
recipe `<same stem>.dalica-collage` beside it; the new photo enters the library and opens in the
edit view. Creating a collage is not an undo step (it is a new file, as an export is); deleting it
is an ordinary delete. **Save Draft** writes the picture and its recipe as Create Collage does,
with `"draft": true`, and does not open the edit view; Edit Collage shows for a photo with a
readable recipe of the same stem beside it, and Create Collage from a reopened collage writes a new
one beside it. The recipe holds each clip's id and its path; reopening matches a clip by id, then
by path among the photos already indexed (a path from a recipe is never opened, `security.md`
§5.5), and a clip not found leaves the Clips with a notice. A recipe is read under limits (1 MiB,
depth 32, 1 000 clips, every number finite and in range) and a hostile one is refused without a
panic. **Legacy files:** the legacy sidecar's two names and its originals folder's name are built
from a byte constant in `sidecar::legacy` that does not spell the reference name (the names are in
the notes), so `cargo xtask names` passes and every crate's tests use that constant. Only files
inside watched folders are read: no central database, contacts file or album file from the old
program's application data, so a face whose contact has no name in its folder's sidecar or an
ancestor's becomes an Unnamed face, and an album defined in no folder's sidecar is not created. The
ignore marker for a face reads as Ignored (the notes' live measurement; the conflicting reading is
recorded there). Rectangles (crops and faces) are decoded over 65 535 and mapped from the stored
pixels through the EXIF orientation into reserved decision 11's frame. Legacy edit operations whose
parameters the notes record with confidence map to the M7 and M12 operations; the rest, the
markers of baked-in fixes and the text overlay become `legacy:<name>(…)` entries in their place in
the recipe; superseded crops are not carried (the old program ignored them too). A photo the old
program saved takes its recipe from the main sidecar only (its baked edits are never applied
twice). Album and contact ids are kept as `legacy` ids on Dalica's albums and people (in
`albums.json` and `people.json`), so the same album or person seen in many folders is one; a
contact whose name equals an existing person's, ignoring case, joins that person. Legacy data
enters the model as a scan's imports do: no undo step, nothing written; the first change in a
legacy-only folder writes a `.dalica.json` holding everything the model has for that folder,
checked by parsing it back before the rename, through `fs`'s create that never replaces (a
`.dalica.json` appearing meanwhile wins and the folder is read again). Keys with no meaning in
Dalica (change stamps, web and backup ids) are not carried; the legacy file keeps them. Turning
"Read photo data left by the previous organizer" off rescans the legacy-only folders: their photos
keep only what Dalica's own files say, and albums and people that only legacy data named leave the
model when nothing is left in them; turning it on brings everything back; nothing is written either
way. **Legacy originals:** a photo with an original in the legacy originals folder and no Dalica
record is offered Undo Save (the original copied back, the recipe from the originals folder's
sidecar becomes its unsaved edits) and Revert (the original copied back, the edits dropped); in
both the saved file goes to the OS trash rather than being replaced, because Dalica cannot always
render it again (its edits may be `legacy:` entries); nothing in the legacy folder is written,
moved or removed; Dalica's own record, once there, takes precedence; the legacy original is
offered only while the photo's bytes differ from it. The first-run notice ("Found photo data from
your previous photo program in %d folders. …" [OK], `organizing.md` §1) shows once (the setting
`files.legacyNoticeShown`), when a scan first finds legacy data. Ports from the Craft apps get their
`NOTICE` rows from M14.4. M14's budgets join `development.md` §8 at 1.5 × their first measurement,
plus the canvas frame with 100 pictures within 16 ms at p95, and are never loosened. No step of
M14 needs a maintainer decision mid-run; reserved decision 2 (video) is asked at M14.R for M15.

- ✅ **M14.1 `library`: the Collages project, legacy ids and legacy facts in the model**
  - after: M9.8, M13.1, M13.R
  - files: `crates/library/src/model.rs`, `crates/library/src/op.rs`, `crates/library/src/query.rs`, `crates/library/src/albums.rs`, `crates/library/src/people.rs`, `crates/library/src/tree.rs`, `crates/library/tests/legacy.rs`, `crates/library/tests/tree.rs`, `crates/library/README.md`
  - scope: per the decisions above: `Project::Collages` beside `Exported` (the tree's Projects
    group lists it with the collage icon's kind); an optional `legacy` id on albums and people with
    lookups by it; per folder the sidecar in effect (`dalica | legacy | both | none`) and per photo
    the legacy original (size, digest, recipe) as derived facts set by ops without an undo step or
    a revision bump (as `SetSaved` is); queries: the folders whose data came from a legacy sidecar
    (the notice's count), the albums and people only legacy data named (for the option turned
    off), the photos offered a legacy Undo Save.
  - done when: `crates/library/tests/legacy.rs`: an album and a person found by their legacy id;
    the facts ops move no revision and leave the undo stack alone; the query of legacy folders
    counts each once; `crates/library/tests/tree.rs`: a folder marked Collages is listed in
    Projects; every existing `library` test passes unchanged.
- ✅ **M14.2 `sidecar`: the legacy sidecar reader**
  - after: M13.2, M13.R
  - files: `crates/sidecar/src/legacy.rs`, `crates/sidecar/src/legacy/**`, `crates/sidecar/tests/legacy.rs`, `crates/sidecar/fuzz/**`, `crates/sidecar/README.md`, `.github/workflows/fuzz.yml`, `xtask/src/ci.rs`, `docs/security.md`
  - scope: `data-model.md` §9 and `security.md` §4.2, from the notes' description of the format,
    with a provenance header naming them (public descriptions and black-box observation): the
    names (the decisions above); a bounded reader (file ≤ 64 MiB, line ≤ 1 MiB, ≤ 1 000 000
    sections, ≤ 1 000 keys per section) of sections and `key=value` lines split on the first `=`,
    CRLF or LF, no comments, duplicate sections and keys merged (the later wins), garbage sections
    skipped, UTF-8 when the file says so and Windows-1252 otherwise with invalid bytes decoded
    lossily for display and never written; typed values: the folder section (description, date
    from its day-count form, place, hidden category), per file (star, hidden, caption, keywords,
    rotation in quarter turns, flip, the crop rectangle, the edit list and the redo list as raw
    tokens, the text overlay kept whole by its length prefixes, faces as rectangle and contact id
    with zero padding and the ignore marker, album tokens, geotag), album sections (token, name,
    date in either form, description, place; tombstones without a name skipped), contacts (name
    from the three-field form only); rectangles decoded over 65 535 as `[x0, y0, x1, y1]`; the same
    reader for the originals folder's sidecar; a fuzz workspace `crates/sidecar/fuzz` with the
    target `legacy` and its job in the Fuzz workflow (the workspace excluded in the root
    `Cargo.toml`, listed where `xtask/src/ci.rs` lists fuzz workspaces); `security.md` §4.2's row
    and §7.1's target marked built.
  - done when: `crates/sidecar/tests/legacy.rs` (fixtures written in the test from the documented
    format): every key above reads back from a sample folder; a file of the older encoding reads
    its accented names; a value containing `;` and `=` survives; a short rectangle is padded and a
    full one reads `[0, 0, 1, 1]`; duplicated, reversed and empty sections are skipped with the
    rest kept; a file past each limit is refused by name; proptest: random bytes and random line
    soups never panic; `cargo +nightly fuzz build` builds the target; `cargo xtask names` passes.
- ✅ **M14.3 `edit`: legacy edit operations mapped to Dalica's**
  - after: M12.14, M13.R
  - files: `crates/edit/src/legacy.rs`, `crates/edit/tests/legacy.rs`, `docs/editing.md`
  - scope: `data-model.md` §9 and the decisions above: a legacy edit list (raw tokens from M14.2)
    and the photo's rotation, flip and crop turned into Dalica operations: each operation in the
    notes' catalogue whose parameters they record with confidence mapped to its M7 or M12
    operation with its parameters converted into the Dalica operation's own ranges (the module's
    constants), case-sensitive names, a wrong number of parameters or an unknown name kept as
    `legacy:<name>(…)` with the text verbatim, the markers of baked-in fixes and the text overlay
    kept the same way, the order kept, the last crop the only crop carried, rectangles mapped into
    the stored frame through the given EXIF orientation; the redo list the same way; `editing.md`
    §13 gains the mapping table in our own words and says which legacy entries stay unsupported.
  - done when: `crates/edit/tests/legacy.rs`: each mapped operation parses back as a typed Dalica
    operation and renders; parameters at their legacy ends land at the Dalica ends; an unknown
    name, a wrong count and a marker keep their text exactly and the entries after them keep their
    order; two crops give the last one; a crop on a photo with each of the eight orientations
    covers the same pixels as the legacy rectangle on the stored pixels; proptest: no token list
    panics.
- ⬜ **M14.4 `create`: the collage document, page formats and the `.dalica-collage` recipe**
  - after: M8.10, M11.4, M13.R
  - files: `crates/create/src/collage.rs`, `crates/create/src/collage/doc.rs`, `crates/create/src/collage/page.rs`, `crates/create/src/collage/recipe.rs`, `crates/create/src/collage/seed.rs`, `crates/create/src/collage/layout.rs`, `crates/create/src/collage/layout/pile.rs`, `crates/create/src/collage/layout/grid.rs`, `crates/create/src/collage/layout/sheet.rs`, `crates/create/src/collage/layout/exposure.rs`, `crates/create/src/collage/layout/mosaic.rs`, `crates/create/src/collage/layout/frame.rs`, `crates/create/src/collage/render.rs`, `crates/create/tests/collage_doc.rs`, `crates/create/README.md`, `docs/creating.md`, `NOTICE`
  - scope: `creating.md` §3 and the decisions above: the document (style, settings, page format
    and orientation, seed, clips and which are on the page, background picture, frame centre, the
    user's placements) with validation (every number finite and in range, at most 1 000 clips, a
    frame centre or background among the clips); page formats (the crop ratios, Current Display
    from a given size, custom ratios given by the caller, orientation) with the page's pixel size
    at a long side; the seeded generator (`seed.rs`: splitmix-style, the same numbers on every
    system); `Placement` (centre, size, angle, stacking order, the part of the picture shown, all
    normalised) and `layout(doc, pictures' aspect ratios) → Vec<Placement>` dispatching to one
    stub module per style (each returning a simple grid until M14.8 and M14.9) and applying the
    user's placements; Scramble and Shuffle as document changes; the recipe file: version, every
    field above, `draft`, each clip's id and path, the output's size, printed sorted and pretty,
    parsed under the limits of the decisions with unknown keys kept; a `render.rs` stub with the
    render's signature for M14.10; `creating.md` §3 records the decisions; the `NOTICE` rows of
    every port named in M14's blocks.
  - done when: `crates/create/tests/collage_doc.rs`: a document round-trips through its recipe
    byte for byte; Scramble changes the seed and drops the user's placements, Shuffle keeps the
    slots and changes the assignment; each page format gives its ratio in both orientations and
    Current Display the given size; the generator gives fixed numbers for fixed seeds; hostile
    recipes (`NaN`, a huge clip list, depth 100, a clip id twice, a frame centre not in the clips)
    are refused without a panic (proptest).
- ⬜ **M14.5 `engine`: M14's command modules, settings and options row**
  - after: M13.3, M13.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/collage.rs`, `crates/engine/src/cmd/collage_output.rs`, `crates/engine/src/cmd/legacy.rs`, `crates/engine/src/collage.rs`, `crates/engine/src/collage_output.rs`, `crates/engine/src/legacy.rs`, `crates/engine/src/session.rs`, `crates/engine/src/options.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/settings.rs`, `crates/engine/tests/options_catalogue.rs`, `crates/engine/README.md`, `docs/menus-and-shortcuts.md`, `docs/data-model.md`
  - scope: the engine's ground for M14 so M14.13, M14.16 and M14.17 never share a file: empty
    `cmd/collage.rs`, `collage_output.rs` and `legacy.rs` joined into the registry once; stubs
    `collage.rs` (the open collage and the room's history), `collage_output.rs` and `legacy.rs`,
    each a field of `Session` once; every M14 setting with its default behind a typed accessor:
    collage (the last style and settings, page format and orientation, background colour; the
    decisions' defaults), `files.readLegacyData` (on; now read) and `files.legacyNoticeShown`
    (off); the options catalogue's "Read photo data left by the previous organizer" made
    available; `menus-and-shortcuts.md` §1.9 lists the new commands (`create.collage`,
    `collage.*`, `legacy.inspect`) and the Collage key context's keys (§2: Delete, Ctrl+A, Esc,
    Ctrl+Z, Ctrl+Y); `data-model.md` §4 lists the new keys.
  - done when: `crates/engine/tests/settings.rs` round-trips every new setting with unknown keys
    kept and wrong types or out-of-range values falling back (reported);
    `crates/engine/tests/options_catalogue.rs`: the legacy row is available with its default equal
    to its accessor's; every existing engine test and `panic_hunt` pass unchanged.
- ⬜ **M14.6 `ui`: M14's view state, UI commands and the stub room**
  - after: M13.4, M13.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/menu_catalog.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/rooms.rs`, `crates/ui/src/commands/collage.rs`, `crates/ui/src/commands/collage_canvas.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/rooms.rs`, `crates/ui/src/panels/collage.rs`, `crates/ui/src/panels/collage/**`, `crates/ui/src/panels/edit/top_bar.rs`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/collage_close.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state.rs`, `docs/control-protocol.md`
  - scope: every M14 field of `UiState` in one go: the Collage room (open, its tab Settings or
    Clips, the selected pictures on the canvas, the clip selection, the drag or turn under way,
    Get More's mode, the close question); UI commands with stub handlers, so M14.18–M14.20 never
    share a file: `create.collage` (opens a stub room), the room's `collage.*` commands (style,
    settings, clips, create, save draft, reset, close, get more, back to collage) in
    `commands/collage.rs`, the canvas's (select, move, turn and scale, order, swap, remove, set as
    background, set as frame centre, scramble, shuffle, view and edit) in
    `commands/collage_canvas.rs`; `room.select` and `room.close` knowing `collage`; the Collage
    key context made active by the room in `keys.rs`; stub panels `collage.rs` and its folder
    (`settings.rs`, `clips.rs`, `canvas.rs`), the edit top bar's Edit Collage button (hidden until
    live), the stub dialog `collageClose`; the Create menu's Picture Collage… wired (greyed until
    live); control stubs for the new state; `control-protocol.md` §6.5 and §7 (the room's state
    and ids following §7's patterns: `tab:room/collage`, `region:collage`, `canvas:collage`,
    `item:<n>`, `clip:<id>`, `button:tray.backToCollage`, `dialog:collageClose`, …).
  - done when: `crates/ui/tests/view_state.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); `create.collage` opens the Collage
    tab and the strip shows it; a Collage-context key does nothing in the library; every command's
    graceful-failure test; `panic_hunt` and `widget_ids` stay green.
- ✅ **M14.7 `ui-kit`: style rows, image toggles, the turn-and-scale handle and the collage icons**
  - after: M12.4, M13.5, M13.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/dropdown.rs`, `crates/ui-kit/src/widgets/toggle_group.rs`, `crates/ui-kit/src/widgets/transform_handle.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/collage.rs`, `crates/ui-kit/tests/collage_widgets.rs`, `crates/ui-kit/tests/icons_collage.rs`, `crates/ui-kit/tests/fingerprints/collage-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-collage*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `design-system.md` §4–§6 and `creating.md` §3: the dropdown's rich rows (an icon, a
    bold name and a one-line description, closed and open); `toggle_group` (a row of image toggle
    buttons, one chosen, each with a tooltip; the borders and the orientation); `transform_handle`
    (the `accent` outline of a selected picture at any angle and the wheel handle on it: dragging
    it turns and scales together around the picture's centre, reporting angle and scale; Shift
    keeps the scale); the icons it lacks, our own drawings on the 20-unit grid: the six styles, no
    border, white border, instant border, landscape, portrait, Scramble, Shuffle, Set as
    Background, Frame Center, Bring to Top, Send to Bottom, Get More; the widget gallery gains
    them; `design-system.md` §6's rows.
  - done when: `crates/ui-kit/tests/collage_widgets.rs`: a rich row elides its description and
    keeps its name; a toggle group keeps one chosen and moves by the arrows; dragging the handle a
    quarter turn round reports 90° and dragging it out doubles the scale; the outline follows the
    angle; `crates/ui-kit/tests/icons_collage.rs`: each new icon stays in its grid at 12, 16, 20 and
    28 px in rest, hover and disabled colours; gallery and icon fingerprints; `widget_ids`,
    contrast, `cargo xtask assets` and `cargo xtask names` pass.
- ⬜ **M14.8 `create`: Picture Pile, Grid, Contact Sheet and Multiple Exposure**
  - after: M14.4
  - files: `crates/create/src/collage/layout/pile.rs`, `crates/create/src/collage/layout/grid.rs`, `crates/create/src/collage/layout/sheet.rs`, `crates/create/src/collage/layout/exposure.rs`, `crates/create/tests/collage_layouts.rs`
  - scope: `creating.md` §3 and the decisions above: the pile (pictures scattered over the page,
    each whole, turned within ±15°, sized so the pile covers the page for any count, kept on the
    page, stacked in clip order, the user's moves applied over it); the grid (rows and columns
    chosen for the count and the page's ratio, equal cells, the spacing, each picture filling its
    cell); the contact sheet (a grid of whole pictures with a line under each for its file name);
    multiple exposure (every picture covering the page, equal weights); `grid_rects_for` and
    `place` ported from LightCraft `crates/engine/src/print/layout.rs` and
    `crates/engine/src/print/compose.rs` at 20ccedf5 (with `Rect`), each with its header.
  - done when: `crates/create/tests/collage_layouts.rs`: for 1, 2, 7 and 100 pictures each style
    gives the same placements for the same seed and different ones for another (the pile); every
    pile picture stays on the page and the pile leaves no more than a fifth of the page bare at 20
    pictures; grid cells are equal, never overlap and keep the spacing; a contact sheet's names
    never overlap the next row; proptest: any count from 0 to 1 000 and any page ratio give finite
    placements without a panic.
- ⬜ **M14.9 `create`: Mosaic and Frame Mosaic**
  - after: M14.4
  - files: `crates/create/src/collage/layout/mosaic.rs`, `crates/create/src/collage/layout/frame.rs`, `crates/create/tests/collage_mosaic.rs`
  - scope: `creating.md` §3 and the decisions above: the mosaic, our own seeded guillotine
    partition of the page into one block per picture, filling it without holes, each block's
    ratio near its picture's (cuts weighted by the pictures' ratios, along the longer side), the
    spacing as gaps; the frame mosaic: the frame centre (the first clip unless set) in a central
    block of about half the page in each direction at its own ratio, the others in a mosaic of the
    bands around it; swaps from the user's placements exchange two pictures' blocks.
  - done when: `crates/create/tests/collage_mosaic.rs`: for 1, 2, 7 and 100 pictures the blocks
    tile the page exactly (their areas sum to the page's minus the gaps, none overlap) and repeat
    for the same seed; a block's ratio stays within 2× of its picture's for 2 to 30 pictures; the
    frame centre is the largest block and Set as Frame Center moves it; a swap exchanges exactly
    two blocks; proptest: no count or ratio panics.
- ⬜ **M14.10 `create`: rendering a collage**
  - after: M14.4, M12.14
  - files: `crates/create/src/collage/render.rs`, `crates/create/src/collage/render/**`, `crates/create/tests/collage_render.rs`, `crates/create/tests/fingerprints/collage-*.json`
  - scope: `creating.md` §3 and the decisions above: `render(doc, placements, pictures, size,
    cancel, progress)` drawing the page at its pixel size: the background colour or the
    background picture covering the page; each picture, rendered by the caller through the one
    output pipeline with its edits at the size its placement needs (asked one at a time), cropped
    to its cell or whole, turned with bilinear sampling, with its white or instant border, its
    shadow (M12's drop shadow from `edit`), its caption (Show captions, the pile) or file name (the
    contact sheet) drawn with `create::watermark`'s Inter text, stacked in order, pixel rectangles
    by `px_rect` ported from LightCraft `crates/engine/src/print/compose.rs` at 20ccedf5; multiple exposure
    blended with equal weights; encoding to JPEG at quality 92 with its date; memory within the page
    and one picture; cancellation between pictures.
  - done when: `crates/create/tests/collage_render.rs` (`scenes` pictures): each style's page at
    long side 3000 px has the page's ratio and every picture's centre pixel where its placement
    says; a render at 600 px and one at 3000 px downscaled agree within the fingerprint tolerance;
    a white border is white and a shadow darkens only outside the picture; cancelling after the
    first picture stops; peak memory stays within the page plus one picture; fingerprints of the
    six styles at 1 200 px.
- ⬜ **M14.11 `scan`: legacy folders read on scan**
  - after: M14.1, M14.2, M14.3, M13.14, M13.19
  - files: `crates/scan/src/legacy.rs`, `crates/scan/src/legacy/**`, `crates/scan/src/walk.rs`, `crates/scan/tests/legacy.rs`, `crates/scan/README.md`
  - scope: `data-model.md` §9 and the decisions above: on the scan's worker, a folder without
    `.dalica.json` and with a legacy sidecar (either name; through `dalica-fs`, never through a
    link) has it read and converted (M14.2, M14.3) into what a `.dalica.json` gives the model:
    each photo's star, hidden, caption and tags, recipe and redo list, faces with their contact ids
    (the contacts of the folder's sidecar, then of its ancestors already read in the walk) and
    geotag; the folder's description, date, place and hidden flag; album memberships and
    definitions by token; the folder's fact `legacy`; with both files the legacy one is not read
    (fact `both`); with `files.readLegacyData` off neither; sections matched to photos by exact name,
    then ignoring case when only one matches; stale sections and paths in values never opened;
    a damaged or oversized legacy file reported and skipped, never set aside or renamed (legacy
    files are read-only). The scan crate gains its `dalica-edit` dependency (L3 → L1).
  - done when: `crates/scan/tests/legacy.rs` (fixtures written by the test): a legacy-only folder's
    photos carry every field above after a scan; a folder with both files reads only
    `.dalica.json` and reports `both`; with the option off nothing is read; a name differing only
    in case matches once and an ambiguous one not at all; a contact named only in the parent
    folder names the face; a legacy file through a link is not read; every legacy file's bytes and
    time are unchanged after the scan; a hostile file is skipped with a warning and no panic.
- ⬜ **M14.12 `store` + `sidecar`: legacy ids on disk, the first write of a legacy folder and the rebuild**
  - after: M14.1, M14.2, M13.9
  - files: `crates/store/src/legacy.rs`, `crates/store/src/writer.rs`, `crates/store/src/rebuild.rs`, `crates/store/src/mapping.rs`, `crates/store/src/albums.rs`, `crates/store/src/people.rs`, `crates/sidecar/src/albums.rs`, `crates/sidecar/src/fields.rs`, `crates/store/tests/legacy.rs`, `crates/store/README.md`, `docs/data-model.md`
  - scope: `data-model.md` §3–§4 and §9 and the decisions above: albums' and people's `legacy`
    ids in `albums.json` and `people.json` (unknown keys kept); the `"project": "collages"` mark;
    the first write of a folder whose fact is `legacy`: a new `.dalica.json` holding everything
    the model has for the folder (every photo, not only the changed one), parsed back and compared
    before the rename, through `fs`'s create that never replaces (a `.dalica.json` appearing
    meanwhile: nothing written, the folder read again), then the fact `both`; never a write,
    rename or delete of a legacy file; a rebuild of a legacy-only folder gives the model the first
    scan gave; the "nothing silently dropped" mapping test lists the new fields; `data-model.md`
    §9 says what landed.
  - done when: `crates/store/tests/legacy.rs` (`data-model.md` §10.11): starring one photo of a
    legacy-only folder writes `.dalica.json` with every photo's converted data and the star, and
    the legacy file stays byte-identical with its time; after it the folder reads only
    `.dalica.json`; a `.dalica.json` created by another program between the read and the write is
    never replaced; an album and a person with legacy ids survive a restart and a rebuild of
    `albums.json` and `people.json`; a crash at one point before and one after the commit (`fs`
    faults) leaves no `.dalica.json` or the whole one.
- ⬜ **M14.13 `engine`: legacy data in the session**
  - after: M14.5, M14.11, M14.12, M13.15
  - files: `crates/engine/src/cmd/legacy.rs`, `crates/engine/src/legacy.rs`, `crates/engine/src/scanning.rs`, `crates/engine/src/cmd/inspect.rs`, `crates/engine/tests/legacy.rs`, `docs/control-protocol.md`
  - scope: `data-model.md` §9 and the decisions above: the scan's legacy results applied as a
    scan's imports are (no undo step): albums found or created by legacy id, people by contact id,
    then by name ignoring case, faces as M13's faces with their people or Ignored or Unnamed,
    geotags as M13's; the folder facts; `files.readLegacyData` turned off or on rescanning the
    legacy-only folders with what the decisions say; `legacy.inspect {folders?}` (each folder's
    source and what it held) and the source in `folder.inspect` and `library.inspect`; `photo.inspect`
    counting a recipe's `legacy:` entries; the notice: `Session::legacy_notice()` gives the count of
    legacy folders the first time a scan finds any while `files.legacyNoticeShown` is off, and
    `legacy.noticeShown` sets it; `control-protocol.md` says what landed.
  - done when: `crates/engine/tests/legacy.rs`: a library of two legacy folders sharing an album
    and a person shows one album with both folders' photos and one person with both faces; the
    first change writes `.dalica.json` (the store's test proves the bytes) and the folder's source
    turns `both`; turning the option off removes the legacy-only data and turning it on brings it
    back with nothing written; the notice's count comes once; `legacy.inspect` reports each
    source; each command's contract and graceful failures; the owner thread made no `fs` call;
    `panic_hunt` green.
- ⬜ **M14.14 `create` + `scan` + `engine`: legacy originals for Undo Save and Revert**
  - after: M14.1, M14.2, M14.3
  - files: `crates/create/src/save/legacy.rs`, `crates/create/tests/save_legacy.rs`, `crates/scan/src/originals.rs`, `crates/scan/tests/originals.rs`, `crates/engine/src/cmd/save.rs`, `crates/engine/tests/legacy_originals.rs`
  - scope: `data-model.md` §6 and §9 and the decisions above: `scan::originals::read` also reads
    the folder's legacy originals folder (its sidecar through M14.2 and M14.3, each kept original's
    size and digest, never through a link, nothing written) and gives the photos' legacy-original
    facts; `create::save::legacy`: Undo Save and Revert from a legacy original: the original
    copied (never moved) over the photo through `fs`'s verified replace after the saved file went
    to the OS trash, the identity checks of M8's restore, an error at any step leaving the photo
    whole; `photo.undoSave` and `photo.revert` enabled for a photo with a legacy original and no
    Dalica record, Undo Save giving the originals folder's recipe as unsaved edits (one undo step,
    as M8's), Revert dropping the edits.
  - done when: `crates/create/tests/save_legacy.rs`: Revert gives the original's exact bytes, the
    saved file is in the test trash and the legacy originals folder is byte-identical with its
    times; a fault before and one after the copy (`fs` faults) leaves the photo readable and the
    saved bytes in the trash or in place; `crates/scan/tests/originals.rs`: a legacy original is
    found with its recipe and one equal to the photo is not offered;
    `crates/engine/tests/legacy_originals.rs`: Undo Save brings the recipe back as unsaved edits,
    Revert drops it, a photo with a Dalica record uses Dalica's; the owner thread made no `fs`
    call.
- ⬜ **M14.15 `ui`: the previous organizer's data in the window**
  - after: M14.6, M14.13, M14.14
  - files: `crates/ui/src/notices.rs`, `crates/ui/src/dialogs/options/general.rs`, `crates/ui/src/dialogs/folder_properties.rs`, `crates/ui/src/panels/edit/history.rs`, `crates/ui/tests/legacy_ui.rs`, `crates/ui/tests/fingerprints/legacy-*.json`, `docs/organizing.md`
  - scope: `organizing.md` §1 item 4 and `data-model.md` §9: the notice "Found photo data from your
    previous photo program in %d folders. Dalica shows it and saves any changes in its own files;
    the old files are never changed." [OK], non-modal, once (OK runs `legacy.noticeShown`); Options
    ▸ General's "Read photo data left by the previous organizer" live; Folder Properties' line
    saying which file holds the folder's data (Dalica's, the previous organizer's, both with
    Dalica's in use, none); the edit panel's line "1 edit from the previous program (not
    supported)" (and "%d edits …") for a recipe's `legacy:` entries; File ▸ Revert and Undo Save
    live for a legacy-saved photo with M8's dialogs; `organizing.md` records the decisions above.
  - done when: `crates/ui/tests/legacy_ui.rs` (headless, a legacy fixture folder): the notice
    shows once with the right count and not after a restart; the option off hides the folder's
    legacy stars and on brings them back; Folder Properties names the source before and after the
    first change; the edit panel counts two `legacy:` entries; Revert from the menu restores the
    legacy original; at 900 × 600 the notice and the line fit; fingerprints of the notice and the
    edit panel's line at 1400 × 900.
- ⬜ **M14.16 `engine`: the collage session**
  - after: M14.4, M14.5, M14.8, M14.9
  - files: `crates/engine/src/cmd/collage.rs`, `crates/engine/src/collage.rs`, `crates/engine/tests/collage.rs`
  - scope: `creating.md` §3 and the decisions above: `create.collage {ids | section}` (one open
    collage; opening again adds the new pictures to its Clips), `collage.inspect` (the document,
    the placements and the page size for the canvas, the clips with their thumbnail ids),
    `collage.set {style?, settings…}`, `collage.clips.add`, `.clips.remove`, `collage.place`,
    `.unplace`, `collage.move {item, centre?, scale?, angle?}` (the pile), `collage.swap {a, b}`,
    `collage.order {item, top | bottom}`, `collage.background {id | null}`, `collage.frameCenter
    {id}`, `collage.scramble`, `collage.shuffle`, `collage.reset`, `collage.undo`, `collage.redo`
    (the room's own history, never the library's), `collage.close {discard?}` (`needs_confirm`
    with unsaved changes); placements from `create`'s layouts with the pictures' ratios from the
    model (no decode); the page formats with the Crop panel's custom ratios and their deletion;
    the last style and settings kept in the settings.
  - done when: `crates/engine/tests/collage.rs`: a collage of 12 photos lays out the same after
    a restart of the session with the same seed; Scramble and Shuffle change what they say; a move,
    a swap and an order change each undo in the room's history and leave the library's undo stack
    alone; removing a picture keeps it in the Clips and + puts it back; deleting a custom ratio
    here removes it from the Crop panel's list; closing with changes needs confirm; each command's
    contract and graceful failures; the owner thread made no `fs` call; `panic_hunt` green.
- ⬜ **M14.17 `engine`: Create Collage, Save Draft and Edit Collage**
  - after: M14.10, M14.12, M14.16
  - files: `crates/engine/src/cmd/collage_output.rs`, `crates/engine/src/collage_output.rs`, `crates/engine/tests/collage_output.rs`, `docs/security.md`
  - scope: `creating.md` §1 and §3 and the decisions above: `collage.create {path?, display?}` and
    `collage.saveDraft` as a `Collage` job on a worker (progress, Cancel): each picture rendered
    through the one output pipeline at the size its place needs, M14.10's render, the JPEG and its
    recipe written through `fs` into the projects directory's `Collages` folder (created and marked
    Collages) under a sanitized, never-replacing name, the new photo added to the library and, for
    Create, opened in the edit view; over automation the destination is checked against the write
    roots (`control-protocol.md` §5.7); `collage.edit {id}` reopening the room from the photo's
    recipe (clips matched by id, then by indexed path, missing ones reported) and
    `collage.canEdit {id}` for the top bar; `security.md` §4.2 gains the recipe's limits and §5.4
    the collage file names, both marked built.
  - done when: `crates/engine/tests/collage_output.rs`: Create writes a JPEG of the page's ratio at
    long side 3000 px and its recipe, twice gives `-001`, and the photo is in the Collages project;
    a display-sized page has the display's size; Cancel leaves no file; Edit Collage reopens with
    the same clips, settings and placements and a clip deleted meanwhile is reported; a recipe path
    pointing outside the library is never opened; over automation a destination outside the write
    roots is refused; the owner thread made no `fs` call; `panic_hunt` green.
- ⬜ **M14.18 `ui`: the Collage room: settings, clips, Create and Close**
  - after: M14.6, M14.7, M14.16
  - files: `crates/ui/src/panels/collage.rs`, `crates/ui/src/panels/collage/settings.rs`, `crates/ui/src/panels/collage/clips.rs`, `crates/ui/src/commands/collage.rs`, `crates/ui/src/commands/rooms.rs`, `crates/ui/src/dialogs/collage_close.rs`, `crates/ui/src/panels/bottom/tray.rs`, `crates/ui/src/app.rs`, `crates/ui/tests/collage_room.rs`, `crates/ui/tests/fingerprints/collage-room-*.json`
  - scope: `creating.md` §3 and `ui-layout.md` §8: the room's layout (the Settings and Clips tabs
    on the left, the canvas region on the right); Settings: the style dropdown with M14.7's rich
    rows, Picture borders, Grid spacing, Background (Solid color with M12.4's swatch, picker and
    eyedropper sampling the canvas, or Use picture), Page format with its delete button (custom
    ratios only) and the orientation toggles, Draw shadows, Show captions, each greyed for the
    styles that do not offer it; Create Collage (the job's progress and Cancel), Reset, Close;
    Clips: "Clips (N)", the thumbnails, +, − and Get More, and the tray's Back to Collage; closing
    the tab or the window with unsaved changes asks [Save Draft] [Discard] [Cancel].
  - done when: `crates/ui/tests/collage_room.rs` (headless, by id): the tray's 6 photos open the
    room with 6 clips on the page; each style greys the options it lacks; the eyedropper on the
    canvas sets the background colour; portrait turns the page; Get More then Back to Collage adds
    the selection; Create Collage opens the new photo in the edit view; Close with changes asks and
    Save Draft writes a draft; at 900 × 600 the room fits; fingerprints of both tabs at
    1400 × 900.
- ⬜ **M14.19 `ui`: the collage canvas**
  - after: M14.6, M14.7, M14.16
  - files: `crates/ui/src/panels/collage/canvas.rs`, `crates/ui/src/panels/collage/canvas/**`, `crates/ui/src/commands/collage_canvas.rs`, `crates/ui/tests/collage_canvas.rs`, `crates/ui/tests/fingerprints/collage-canvas-*.json`
  - scope: `creating.md` §3: the page at its ratio drawn from the thumbnail ladder with the
    engine's placements (never a decode on the UI thread; a grey placeholder until a thumbnail
    arrives), the background, borders and shadows as the render draws them; click selects (`accent`
    outline), Ctrl and Shift add, Ctrl+A and Esc; in a pile a drag moves and M14.7's handle turns
    and scales; in the mosaics and the grid a drag onto another picture swaps them; right-click
    Bring to Top / Send to Bottom and View and Edit; Delete takes the selection back to the Clips;
    the buttons above (Select All, Select None, Remove, Set as Background, or Set as Frame Center in
    Frame Mosaic) and below (Scramble Collage, Shuffle Pictures, View and Edit); Ctrl+Z and Ctrl+Y
    in the room's history.
  - done when: `crates/ui/tests/collage_canvas.rs` (headless, by id): dragging a pile picture moves
    it and the handle turns it 90° as one room step; dragging one grid picture onto another swaps
    them; Bring to Top puts it last in the order; Delete sends two pictures to the Clips; Set as
    Frame Center moves the centre; each picture's centre on the canvas matches its placement within
    one pixel; Ctrl+Z undoes the last change; the canvas never waits on the disk; fingerprints of a
    pile with a selection and of a frame mosaic at 1400 × 900.
- ⬜ **M14.20 `ui`: Collage everywhere it starts, Edit Collage and the Collages project**
  - after: M14.17, M14.18, M13.25
  - files: `crates/ui/src/panels/bottom.rs`, `crates/ui/src/panels/light_table/header.rs`, `crates/ui/src/panels/edit/top_bar.rs`, `crates/ui/src/panels/folders/model.rs`, `crates/ui/tests/collage_entry.rs`, `crates/ui/tests/fingerprints/collage-entry-*.json`, `docs/ui-layout.md`
  - scope: `creating.md` §1 and `ui-layout.md` §5–§6: the bottom bar's Collage (the tray, and its
    More… row), the section headers' Collage (folders, albums and person sections: the selection
    inside the section, else the whole section; M13.25's "Arrives with Collage" removed), Create ▸
    Picture Collage… live; the edit view's Edit Collage button for a photo `collage.canEdit` says
    yes; the Projects group's Collages row with the collage project icon; `ui-layout.md` records
    what landed.
  - done when: `crates/ui/tests/collage_entry.rs` (headless, by id): the bottom bar's Collage with
    3 held photos, a folder header's with nothing selected and a person section's each open the
    room with the right clips; a created collage shows Edit Collage in the edit view and it reopens
    the room; an ordinary photo shows no Edit Collage; the Collages folder sits in Projects with
    its icon; a fingerprint of the top bar with Edit Collage at 1400 × 900.
- ⬜ **M14.21 `mcp`: collages and a legacy library over MCP, and the workflows W15 and W16**
  - after: M14.13, M14.17
  - files: `crates/mcp/tests/workflows/w15_collage.rs`, `crates/mcp/tests/workflows/w16_legacy.rs`, `crates/mcp/tests/workflows/main.rs`, `crates/mcp/src/headless.rs`, `xtask/src/stats.rs`, `docs/mcp.md`
  - scope: `mcp.md` §9: W15 (**collage**: `cmd_create_collage` on 8 photos → `cmd_collage_set
    {style: "grid"}` → `cmd_collage_create` with a path in the write root → a JPEG of the page's
    ratio at long side 3000 px and its recipe → `photo_info` lists it in the Collages project →
    `cmd_collage_edit` reopens it with the 8 clips) and W16 (**legacy library**: `scan_folder` on a
    folder with only a legacy sidecar → `photo_info` shows its stars, captions, recipe with a
    `legacy:` entry, album and person → `set_metadata` stars one → `.dalica.json` holds every photo
    and the legacy file is byte-identical → `cmd_legacy_inspect` says `both`); headless mode
    applying the legacy results as the app does; the floor in `xtask/src/stats.rs` raised to the
    sixteen workflows now required; `mcp.md`'s table and status line; row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp` runs W15 and W16 green; `cargo xtask stats` prints
    16 / 16 passing and `stats --check` fails below that.
- ⬜ **M14.22 `create` + `scan` + `ui` + `xtask`: M14's budgets: collage layouts and renders, the canvas and legacy scans**
  - after: M14.10, M14.11, M14.17, M14.19
  - files: `crates/create/benches/collage.rs`, `crates/scan/benches/legacy.rs`, `crates/ui/benches/collage.rs`, `xtask/src/bench.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: `development.md` §8: benches registered in `xtask/src/bench.rs`, joined to `BUDGETS`
    and the release-budgets job, each documented in §8 and §13 with its machine: `collage` (each
    style's layout of 200 pictures; a 3000 px render of 50 `scenes` photos, time and peak memory),
    `legacy_scan` (a first scan of 10 000 photos in 100 legacy-only folders against the same
    folders with `.dalica.json`; a 64 MiB legacy file read to its limit), `collage_canvas` (the
    room's frame with 100 pictures on the page while one is dragged, headless); budgets at 1.5 ×
    their first measurement, the canvas frame within 16 ms at p95 whatever the first numbers; a
    missed budget is fixed in this task (the module asked of the lead; before and after numbers in
    the PR) or blocks with its numbers, never loosened.
  - done when: `cargo xtask bench collage`, `bench legacy_scan` and `bench collage_canvas` print
    their numbers (in the PR); `cargo xtask budgets` passes with them and `--plant-ms` makes each
    fail.
- ⬜ **M14.25 `ui` + `engine`: M13's automation follow-ups: a person's export job, clicks under a dialog, a flaky sidecar test**
  - after: M13.R
  - files: `crates/ui/src/control/jobs.rs`, `crates/ui/src/control/view.rs`, `crates/ui/src/control/mod.rs`, `crates/ui/src/control/tests.rs`, `crates/engine/src/automation.rs`, `crates/ui/tests/damaged_sidecar.rs`, `docs/control-protocol.md`
  - scope: from the M13.R review's open items: (1) when automation confirms an Export of a
    `person:<id>` section without the people permission, `jobs.list` / `jobs.wait` report the
    job's `folder`, `name` and `written` with the person's name; withhold them like the export
    draft (M13.49), keyed by the job, never by matching text; (2) `ui.clickWidget` on a widget a
    modal dialog covers replies `ok` and does nothing: reply with an error naming the dialog;
    (3) ui `damaged_sidecar` failed once (no widget `button:folder.resetData/1`): reproduce under
    load and fix the race (product or test synchronisation, never a looser check).
  - done when: one test per item, each failing without its fix; the flaky test passes 200 runs
    under `taskset -c 0`.
- ⬜ **M14.26 `xtask` + `ui` + `engine`: the M13.R review's macOS and Windows failures**
  - after: M13.R
  - files: `.github/workflows/ci.yml`, `xtask/src/ci.rs`, `crates/engine/tests/map_tiles.rs`, `crates/engine/src/geocode.rs`, `crates/ui/src/picker.rs`, `crates/ui/src/control/picker.rs`, `tests/acceptance/m11_live.rs`, `tests/acceptance/fingerprints/m11-1400x900-slideshow-wipe.json`, `tests/acceptance/m13_common.rs`, `tests/acceptance/m13_faces.rs`, `docs/development.md`
  - scope: deferred by the maintainer at M13.R (2026-10-18), from the review PR's GitHub runs:
    (1) macOS: engine `map_tiles::a_place_name_is_looked_up_once_and_hostile_replies_are_refused`
    measured 985 ms between two place searches at the test server (asserted ≥ 990 ms) although
    the engine waits `MIN_INTERVAL` (1 s) per send: measure the gap where it is made, or give the
    spacing a margin so arrivals keep 1 s, never a looser assertion; (2) Windows: `ci --full
    --platform` hit the job's 60-minute limit (every package's tests took 34.5 min there, 20 on
    macOS) while `accept M13` ran: split the acceptance steps into their own job or raise the
    limit, so the review's Windows run completes; (3) Windows: `accept M11`'s
    `m11-1400x900-slideshow-wipe` was 2.18 from its fingerprint (tolerance 2): find what differs
    on Windows and fix it, or re-bless with the images shown at the next review; (4) Windows:
    `accept M13`'s faces test got "no file picker is open" from `ui.filePicker` with a short-form
    temp path (`C:\Users\RUNNER~1\…`): make the picker match long and short path forms (or
    canonicalise before comparing) and finish the faces test on Windows.
  - done when: a test for each fix that fails without it (the picker on a short Windows path, the
    spacing); the review gate's GitHub run green on macOS and Windows within its time limit.
- ⬜ **M14.23 `accept M14`**
  - after: M14.15, M14.20, M14.21, M14.22, M14.25, M14.26
  - files: `tests/acceptance/m14.rs`, `tests/acceptance/m14_*.rs`, `tests/acceptance/m14.md`, `tests/acceptance/fingerprints/m14-*.json`
  - scope: every *Done when* of M14's two areas in the real `dalica --headless` over the control
    channel and through `dalica-cli mcp`: a collage opened from the tray, a folder header and a
    person section; each style laid out the same for the same seed; a pile picture moved and
    turned, a grid swap, Scramble and Shuffle; the canvas and the created JPEG agreeing; Create
    Collage writing the JPEG and its recipe beside it, never replacing, in the Collages project, and
    Edit Collage reopening it; Close asking and Save Draft; a library with legacy-only folders (a
    fixture written by the test) showing its stars, captions, edits, albums, people and places with
    the notice once; the first change writing `.dalica.json` with the legacy file byte-identical
    (§10.11); a folder with both files reading only `.dalica.json`; the option turned off and on;
    Undo Save and Revert from a legacy original; W15 and W16 over MCP; the M14 budgets (the release
    budgets with `DALICA_ACCEPT_RELEASE=1`; inside `cargo xtask ci --full` a smaller debug run
    checks the paths and prints its times); whole-window fingerprints of the room with a pile and
    with a mosaic and of the legacy notice; the manual checks in `m14.md` (a collage of real photos
    in each style, the handle and swaps with a mouse and a touchpad, the eyedropper, a collage
    opened in another application; a real folder the previous organizer managed, its stars,
    albums, people and edits, a change in it, then the old program's file compared byte for byte).
  - done when: `cargo xtask accept M14` passes everything in M14's *Done when*.
- ⬜ **M14.24 Lead: M15's task blocks brought up to date**
  - after: M13.33
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: M13.33 writes M15's blocks while M13 runs; this task brings them up to date with what
    M13 and M14 built and left open (their reviews' fix tasks and notes, the collage and legacy
    follow-ups, files that moved or were added since), rechecks every M15 block's `files` and
    *done when* against the code, writes M15's waves again, and keeps the tasks that hinge on
    reserved decision 2 (video) waiting for the maintainer's answer, asked at M14.R, never guessed;
    a block that can port Craft-app code names the source files in its scope (`architecture.md`
    §9); and lists for M14.R's check-in the questions M15's blocks wait on (reserved decision 2
    with its sound, the first languages; M15.1 records the answers).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists the
    M15 tasks as planned.
- ⬜ **M14.R Release 0.9 gate**
  - after: M14.23
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 0.9): the lead runs `/milestone-review M14.R` to prepare it, with
    the version set to 0.9.0 and its `whats-new.md` section on the review branch, the release
    checklist and the sign-off question, and after the merge cuts and publishes 0.9.0
    (`releasing.md`); the check-in page (hold six photos and press Collage, try each style and
    its options, move and turn pile pictures with the handle, swap grid pictures, Scramble and
    Shuffle, set a background colour with the eyedropper and a background picture, change the page
    format and orientation, Get More and Back to Collage, Create Collage and look at the JPEG,
    reopen it with Edit Collage, Close with changes and Save Draft; open a copy of a real folder
    the previous organizer managed, read the notice, check its stars, captions, edits, albums and
    people, change something and check the old file is unchanged, Revert a photo it saved, turn
    the option off and on; written as steps for the maintainer to try); the questions: the
    planning decisions above that change behaviour (the room's own undo history; one Collage room
    at a time; a draft is a collage picture with its recipe; Create from a reopened collage writes
    a new file; Grid and the mosaics crop to their cells; the defaults of the room; only files in
    watched folders are read, so names only in the old program's contacts file are lost; the
    ignore marker read as Ignored; uncertain legacy operations kept as `legacy:` entries; a
    legacy Revert sends the saved file to the trash; the option turned off removes legacy-only
    data from view), reserved decision 2 (video, with its sound) and the first languages
    (`localization.md`) for M15, the look of the room, the six styles and
    the legacy notice (approved with their fingerprints); *Where we stand* and the `status.md`
    numbers (each style's layout and render times, the canvas frame, the legacy scan's overhead,
    the workflow pass rate) on a `review/M14.R` branch, a pull request open. Only the maintainer
    merges that pull request and closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap):

```text
wave 1   M14.1 model: collages + legacy  ‖  M14.2 legacy reader  ‖  M14.3 legacy edit mapping
         ‖  M14.4 collage document + recipe  ‖  M14.5 engine modules + settings
         ‖  M14.6 ui state + stub room  ‖  M14.7 ui-kit collage widgets + icons
         (once M13.R is closed)
wave 2   M14.8 pile, grid, sheet, exposure  ‖  M14.9 mosaics  ‖  M14.10 collage render
         ‖  M14.11 legacy on scan  ‖  M14.12 legacy first write  ‖  M14.14 legacy originals
wave 3   M14.13 legacy in the session  ‖  M14.16 collage session
wave 4   M14.15 legacy in the window  ‖  M14.17 Create, Save Draft, Edit Collage
         ‖  M14.18 Collage room  ‖  M14.19 collage canvas
wave 5   M14.20 Collage entry points  ‖  M14.21 W15, W16  ‖  M14.22 M14 budgets
wave 6   M14.23 accept M14 → M14.R          (M14.24 lead: M15 blocks brought up to date, any time after M13.33)
```

### M15 — Video, localisation, OS integration and Release 1.0

One milestone in 4 feature areas (*Video*, *Localisation and accessibility*, *OS integration*, *Release 1.0*); one `cargo xtask accept M15` covers every *Done when* below. It ends with the release gate the maintainer owns (Release 1.0).

#### Video

- **In:** reserved decision 2 first (pure-Rust decoding or an optional external process; asked at
  M14.R, never guessed; slideshow music comes with video sound, the maintainer's answer at M11.R,
  2026-10-17). Whatever the answer: MP4, MOV, M4V and MKV recognised by their content and probed
  under limits (duration, frame size, rotation, date, codecs, sound, location), listed in the
  library when `fileTypes.video` is on, shown as video cells with the sprocket edge and the
  duration pill (`ui-layout.md` §5), the Videos filter, every path that cannot take a video
  counting it, and a video opened in the edit view that Dalica cannot play showing "This video
  can't be played" with [Open With ▸] (`viewing.md` §4). With decoding, as the answer builds it:
  poster frames (the first non-black frame) as thumbnails and previews; the player in the edit
  view (play and pause, the scrubber and its readout, volume, Full Screen, frame steps; `editing.md`
  §11); trim (`trim(start,end)` in the recipe) and rotation; Save Frame as Picture
  (`Pictures/Dalica/Video Frames`, the Video Frames project); Export Clip; videos in the slideshow
  (played to the end, muted unless Play video sound) and slideshow music (`viewing.md` §1.5);
  Export's "Export videos as" row; videos in a collage as their poster; faces found on a poster
  frame (M13's *Out*); videos over MCP (W17).
- **Out:** the legacy video formats (AVI, WMV, MPG, 3GP, MTS/M2TS) and a movie maker (`product.md`,
  dropped as dated); colour editing of video (`editing.md` §11); WebM (not in `formats.md`'s list).
  If the answer descopes video to tiles, M15.1 records which *In* items leave with it.
- **Reuse** (`architecture.md` §9): `image`'s bounded ISO BMFF walk
  (`crates/image/src/formats/bmff.rs`) for MP4 and MOV; LightCraft `crates/codecs/src/video.rs` at
  4f0b110b (its MP4 and WebM muxers: the box and EBML layouts they write, studied for our readers;
  nothing of its encoder ships); in this repository M8's one output pipeline for saved and exported
  frames, M11's slideshow, M14's collage clips and M13's face scan.
- **Done when:** `cargo xtask accept M15`: a folder with an MP4, a MOV and an MKV lists three video
  cells with their durations, the Videos filter shows only them, Export and the slideshow count
  them, and a hostile container is refused without a panic; with decoding, as M15.1 recorded the
  answer: each cell shows its poster frame, the player plays, steps a frame and trims, the trim
  survives a restart, Save Frame as Picture writes the frame shown into Video Frames, a slideshow
  plays a clip to its end, and W17 passes over MCP.

#### Localisation and accessibility

- **In:** `localization.md`: `tl!` and `tln!` over every user-facing literal, TSV catalogs in
  `crates/ui/src/i18n/`, the language registry (code, endonym, script, plural rule), System
  following the OS's languages, `DALICA_LANGUAGE`, Options ▸ General ▸ Language switching without a
  restart, dates, numbers and sizes through locale-aware helpers, command labels and options rows
  looked up by their English text and id, the release notes line by line, the coverage tests and
  `cargo xtask i18n`; the first languages the maintainer chooses (asked at M14.R:
  `localization.md` says they are chosen when M15 starts), each with its style guide; the CJK
  fallback faces from craft-fonts, by script, in release builds (`releasing.md`); AccessKit: every
  widget with its role, name and value, the light table, the tree and the canvases as nodes, an
  audit over every view and dialog; keyboard-only use (every command reachable by its key or the
  menu bar, Tab order in dialogs, a visible focus); UI scale and font size checked to fit; the
  screen-reader and input-method passes by hand at M15.R (`architecture.md` §11).
- **Out:** right-to-left mirroring of the window unless a chosen language needs it; translating
  what `localization.md` → *What is not translated* lists.
- **Reuse** (`architecture.md` §9): PhotoCraft `crates/ui-egui/src/i18n/mod.rs`, `catalog.rs` and
  `system.rs` and `xtask/src/i18n_coverage.rs` at 5b087699 (the TSV format `localization.md`
  already follows); LightCraft `crates/engine/build.rs` and `crates/engine/src/fonts.rs` (craft-fonts
  embedded) and the script table of `crates/ui-egui/src/i18n.rs` at 4f0b110b; LightCraft
  `crates/ui-egui/tests/accessibility.rs` at 4f0b110b (the AccessKit audit).
- **Done when:** `cargo xtask accept M15`: in each chosen language the library, the edit view and
  the Options dialog show translated text, plurals read right for 0, 1, 2 and 5, and nothing is
  clipped at 1400 × 900 and 900 × 600; switching the language keeps the open view, the selection and
  the undo history; the pseudo-locale leaves no untranslated text outside user data; the AccessKit
  audit finds no interactive node without a name in any view or dialog; every command is reachable
  without a pointer.

#### OS integration

- **In:** `product.md`'s OS-integration row: Locate on Disk with the item selected (photos,
  folders, a folder header's icon, the export notice's Show Folder, the viewer's Show in File
  Manager); Open With ▸ (the system's handlers for the file's type); Open in External Editor
  (Ctrl+Shift+O; Options ▸ General ▸ External editor); Copy Path, and the files and the picture on
  the system clipboard; photos dragged out to other applications; Dalica as the default viewer
  (Options ▸ Photo Viewer ▸ File Types, the viewer's Settings…, offered after the first run, opt-in
  per type); `dalica --view` and Open With handing the file to a Dalica already running (the
  maintainer's answer at M11.R, 2026-10-17) and macOS's open-documents events; cameras and phones
  over MTP/PTP in the Import room.
- **Out:** setting the desktop background, printing and e-mail (`product.md`, dropped); a shell
  extension or file-manager plugin; launching programs over automation (`security.md` §5.11).
- **Reuse** (`architecture.md` §9): LightCraft `apps/lightcraft/src/editors.rs` and the `reveal`
  service of `apps/lightcraft/src/main.rs` at 4f0b110b; PhotoCraft
  `apps/photocraft/src/single_instance.rs` and the clipboard parts of
  `apps/photocraft/src/services.rs` (`arboard`, `text/uri-list`) at 5b087699; in this repository
  the `Picker` pattern (`crates/ui/src/picker.rs`), M11.12's viewer session, M11's Import room and
  `dalica-fs`'s verified copies.
- **Done when:** `cargo xtask accept M15` (the recording `Os` service): Locate on Disk, Open With,
  Open in External Editor and the clipboard commands hand the right paths to the service and the
  launching ones are refused over automation; a second `dalica --view FILE` hands FILE to the first
  Dalica and exits; a camera (the in-memory PTP responder) lists its pictures and imports them
  verified with their times; each OS's CI runs what it can, and `tests/acceptance/m15.md` lists
  the checks by hand on each OS.

#### Release 1.0

- **In:** signed packages (`releasing.md`): Windows `.exe` and `.msi` signed with SHA-256 and a
  timestamp (a `.pfx` or Azure Trusted Signing); macOS Developer ID signing with the hardened
  runtime, notarization and stapling, the CLI zip notarized; every signing path run on packaging
  pull requests with a throwaway identity; the 1.0 parity and security audit (every row measured,
  `process.md` §5); M15's budgets; the full honest assessment and the release notes at M15.R.
- **Out:** Linux code signing (checksums, as today); a built-in updater (`SYS-UPDATES`, deferred);
  a from-source Flathub manifest; per-release SBOMs (`security.md` §10, future hardening); Windows
  on ARM64.
- **Reuse:** the Craft apps' release pipeline Dalica already follows (`process.md` §7; PhotoCraft and
  LightCraft `packaging/windows/sign.ps1`).
- **Done when:** the release dry run signs every Windows and macOS artifact with the throwaway
  identities and the signature checks pass; `cargo xtask parity` passes with no unmeasured ✅; the
  M15 budgets hold; the signed release itself is M15.R's checklist.

#### M15 tasks

M15 finishes the 1.0: videos in the library and, as reserved decision 2's answer allows, played and
trimmed; the interface in the first languages, for screen readers and without a mouse; the
operating system's own ways to locate, open, copy, drag and view files; cameras over MTP/PTP; signed
packages. Its first wave lists the M14 work each task extends and starts once the maintainer has
closed M14.R. Ten tasks lay the ground the others share, so later tasks never share a file: M15.1
(the lead) records M14.R's answers and fits the blocks that wait on them; M15.2 gives `image` video
recognition and probing; M15.3 gives `library` the video facts and the Video Frames project; M15.4
registers one engine command module per area with stub state, the `Os` and `Sound` service traits
with their fakes, M15's settings and options rows, and every port's `NOTICE` row; M15.5 adds M15's
view state, UI commands with stub handlers and the stub video panel; M15.6 gives `ui-kit` the video
controls and the icons; M15.7 gives `ui` its localisation core and the pseudo-locale; M15.8 gives
`ui-kit` the fallback faces; M15.9 gives `scan` the PTP reader; M15.10 signs the Windows packages.
The video work follows in two layers: decision-neutral (videos on scan M15.11, in the session
M15.16, in the window M15.21) and as the answer builds it (frames M15.15, posters M15.20, playback
M15.25, the player M15.26, sound M15.27, export and collage M15.28, W17 M15.30); the OS integration
goes from the app's services (M15.12) through the engine's commands (M15.17) to the window (M15.22,
M15.23, M15.24) and the hand-off (M15.18); the strings are wrapped late, in three tasks by area
(M15.31–M15.33), so they catch everything the feature tasks drew, and the catalogs (M15.35) come
last. Each task updates its `parity.md` rows (`LIB-LT-VIDEO`, `VIEW-VIDEO`, `EDIT-VIDEO`,
`VIEW-MUSIC`, the videos of `VIEW-SLIDESHOW`, `OUT-EXPORT`, `OUT-COLLAGE`, `SRCH-FILTERS` and
`PREF-FILETYPES`, the Video Frames of `LIB-TREE-GROUPS`, `SYS-I18N`, `SYS-A11Y`, the Language and
External editor rows of `PREF-GENERAL`, `PREF-SLIDESHOW`, `PREF-VIEWER`, `FILE-LOCATE`,
`FILE-COPYPATH`, `FILE-DRAGOUT`, `VIEW-VIEWER`, `VIEW-VIEWER-ASSOC`, `IMP-MTP`, `IMP-DEVICES`,
`LIB-CTX-PHOTO`, `LIB-TRAY`, `KEY-GLOBAL-CTRL-SHIFT-O`, `KEY-GLOBAL-CTRL-ENTER`, `SYS-PACKAGING`,
`SYS-RELEASE`, `SYS-MCP`) and names its commands there; first fingerprints of the video cells, the
player, the new widgets and icons and each language are approved in bulk on the M15.R review page.

Whole-window baselines belong to no task, as in M5–M14: the finished milestones'
`tests/acceptance/fingerprints/m1-*.json` … `m14-*.json` and any `crates/ui/tests/fingerprints/`
file not named in a task's `files` change whenever the window does. A task whose change moves them
re-blesses them on its branch (diff images on the PR, the reason in the commit body), they never
count as a `files` overlap, and on a rebase conflict the worker re-blesses after the rebase.
Fingerprints are made at 1400 × 900 only; at 900 × 600 a test asserts that the player, the menus
and the translated views fit (`architecture.md` §11).

Decisions taken while planning (the check-in lists them for reversal). **The answers M15 waits on.**
Reserved decision 2 (video, with its sound: the maintainer tied slideshow music to video sound at
M11.R, and an audio output may need a C-linked system library on Linux, a question of its own under
`AGENTS.md` §4) and the first languages (`localization.md`: chosen by the maintainer when M15
starts) are asked at M14.R, never guessed here. M15.1, the lead's first task once M14.R closes,
records them and fits the blocks that hinge on them: M15.15, M15.20, M15.25–M15.28, M15.30 and the
video parts of M15.34 (video), M15.35 (languages); each of those lists M15.1, and a worker who finds
the answer missing from *Reserved decisions* runs `tasks block` with the question and takes another
task. If the answer descopes video to tiles, M15.1 rewrites those blocks to what remains (their ids
kept, never reused) and names the parity rows that leave for M15.R. Everything else is
decision-neutral: probing containers, listing videos, the video cells and the edit view's "This
video can't be played" are needed under any answer (a codec Dalica cannot play exists under each).
**Localisation:** the catalogs are TSV as `localization.md` says, ported from PhotoCraft
(contexts, ids, plurals), rather than the JSON catalogs of LightCraft the outline named; LightCraft
gives the craft-fonts embedding and the script order. The fonts are embedded by a new
`crates/ui-kit/build.rs` (the faces are installed by `ui-kit`'s `fonts.rs`), not by
`crates/ui/build.rs` as `releasing.md` says today. Strings are wrapped late and by area (M15.31 the
main window, M15.32 the dialogs, M15.33 the edit view, rooms, slideshow and viewer); features drawn
after M15.7 write `tl!` from the start. Each area is proven by the pseudo-locale (`qps`: every
translation bracketed and 40 % longer, built from the key set at run time): every text a frame draws
is bracketed except user data. Engine refusal reasons the window shows are looked up whole by their
English text; one carrying values stays English and is listed by `cargo xtask i18n`, never failed.
`ui-kit`'s own few texts (the date picker's month and weekday names, …) come from a table the app
sets, so `ui`'s catalogs translate them. **Accessibility:** names come from the widget registry's
labels; the light table exposes only its visible cells, so the AccessKit tree is never work
proportional to the library; its cost is a budget (M15.34). **OS integration** goes through service
traits as the native dialogs do (`Picker`): `engine::os::Os` (reveal, the handlers of a type, open
with, the external editor, the clipboard, the default handler per type) and `engine::sound::Sound`,
each with a recording or silent fake that headless sessions, `dalica-cli`, MCP and tests use, and
the real one in `apps/dalica` per OS. No `unsafe` in our crates and no C build dependency: a pure-Rust
crate after `cargo xtask deps`, or the platform's own program started with an argument vector,
never a shell; a feature with no such way on an OS keeps its row greyed there with the OS's reason,
and the task blocks with the question when the only way is `unsafe` or a C library (`AGENTS.md` §4).
The launching commands (Locate on Disk, Open With, Open in External Editor) are refused over
automation (`security.md` §5.11); the copy commands work in the window and answer `unavailable`
headless. Edit ▸ Copy (Ctrl+C) in the library keeps Dalica's own copy and also puts the files on the
system clipboard (and one photo's picture with its edits), so a paste in a file manager copies them.
A drag out gives the files on disk: unsaved edits stay in Dalica, and the drag pill says so.
External editor: the program named in Options ▸ General (`files.externalEditor`), else the system's
edit verb or default application, ported from LightCraft. **One Dalica per user**, ported from
PhotoCraft: a later launch hands its files to the running Dalica, which shows the viewer in its own
window and returns to where it was on Esc; the read-only viewer of M11.12 stays for a library
another user's Dalica holds. **Default viewer:** Dalica sets a default only where the OS lets an
application do it (Linux's `mimeapps.list`, merged through `fs` with every other entry kept, the
replaced handlers remembered for "Don't use Dalica as a viewer"); on Windows and macOS it registers
as a handler and opens the system's own default-apps settings, saying what to choose. **Cameras:**
our own PTP/MTP reader from the public specifications over a pure-Rust USB stack; where an OS holds
the device for its own service (Windows' portable-device driver, macOS's image capture) what is
reachable is measured and recorded; a Linux device the desktop already mounted shows as a folder
source; copies are written through `fs` from the stream and checked by reading them back.
**Signing:** dry runs sign with a throwaway self-signed identity, so the signing path runs on every
packaging pull request; the real certificates and notarization are the maintainer's secrets,
checked at M15.R. **Docs:** `security.md`'s status lines for M15 (the video and PTP rows of §4.2,
§5.11, §7.1's targets, §10's signing) are set by the 1.0 audit (M15.36) from what each task's PR
and crate README record, so the tasks of a wave never share it. M15's budgets join `development.md`
§8 at 1.5 × their first measurement, plus the playing video's frame within 16 ms at p95 and the
AccessKit and translation overheads within 1 ms at p95, and are never loosened. M15.38 (the lead)
writes M16's blocks while M15 runs, as M16's outline asks.

- ⬜ **M15.1 Lead: M14.R's answers in M15's blocks**
  - after: M14.R
  - owner: lead
  - files: `docs/roadmap.md`, `docs/product.md`
  - scope: reserved decision 2 (video, with sound: decoding, the audio output and slideshow music)
    and the first languages are asked at M14.R (M14.24 lists them for its check-in); this task
    records the answers: decision 2 under *Reserved decisions* with its date and the maintainer's
    words and in `product.md`'s Video row; then fits the blocks that wait on them: M15.15, M15.20,
    M15.25, M15.26, M15.27, M15.28, M15.30 and M15.34's video benches get the decoder the answer
    names, their `files` (a new crate registered in `xtask/src/layers.rs` and `architecture.md` §2,
    or the bridge to an external program) and *done when* as the answer builds them, or, if video
    is descoped to tiles, are rewritten to what remains (ids kept, never reused) with the parity
    rows that leave named for M15.R; M15.35 names the chosen languages (one catalog and style
    guide each; a CJK language with its glyph check). Nothing is guessed: an answer missing from
    the review's record is a `tasks block` with the question.
  - done when: `cargo xtask tasks sync --check` parses the fitted blocks, `tasks sync --dry-run`
    lists them, and *Reserved decisions* item 2 carries its date.
- ⬜ **M15.2 `image`: video files recognised by content and probed**
  - after: M5.10, M14.R
  - files: `crates/image/src/sniff.rs`, `crates/image/src/formats/mod.rs`, `crates/image/src/formats/video.rs`, `crates/image/src/formats/video/**`, `crates/image/src/decode.rs`, `crates/image/tests/video.rs`, `crates/image/fuzz/**`, `crates/image/README.md`, `docs/formats.md`
  - scope: `formats.md` §1 and `security.md` §4.2 (needed under any answer to reserved decision 2):
    `Format::Video(VideoKind)` sniffed by content: ISO BMFF files whose `ftyp` major or compatible
    brands name MP4, M4V or QuickTime (`isom`, `iso2`…`iso6`, `mp41`, `mp42`, `avc1`, `M4V `,
    `M4VH`, `qt  `, …), never a still-image brand (`heic`, `mif1`, `avif`, `crx `), and Matroska
    files by their EBML header's DocType `matroska`; `video::probe` reading, through `bmff.rs`'s
    bounded walk and a bounded EBML reader of our own written from the public Matroska and EBML
    specifications (RFC 8794; provenance header), the duration (`mvhd` or `mdhd`; Matroska
    `Duration` × `TimestampScale`), the frame size, the rotation (the track header's matrix, in
    quarter turns), the creation date (`mvhd` from 1904; Matroska `DateUTC`), the video and sound
    codecs (sample entry codes; `CodecID`), the frame rate, whether it has sound, and the location
    (ISO 6709 `©xyz` in `udta`, or the QuickTime location key); every size and count hostile (box
    depth ≤ 12, ≤ 4 096 boxes per level, sample tables never read whole, an EBML element inside its
    parent, unknown-size elements bounded, every read counted by `Work`); `decode` of a video
    answers unsupported with "This video can't be played" until M15.15 gives frames; the fuzz
    target `video_probe`; `formats.md`'s Video row says what landed.
  - done when: `crates/image/tests/video.rs` (fixtures written by the test from the documented
    layouts): an MP4, an M4V, a MOV turned a quarter by its matrix and an MKV each report their
    duration to the millisecond, size, rotation, date, codecs, sound and location; HEIC, AVIF and
    CR3 files are never videos; a file cut at every box boundary is truncated, never a panic;
    nesting past 12, 4 097 sibling boxes, an EBML size past its parent and an unknown-size element
    running to the end are refused or bounded; proptest: random bytes after a video header never
    panic; `cargo +nightly fuzz build` builds `video_probe`.
- ⬜ **M15.3 `library`: videos and the Video Frames project in the model**
  - after: M9.1, M14.1, M14.R
  - files: `crates/library/src/model.rs`, `crates/library/src/op.rs`, `crates/library/src/query.rs`, `crates/library/src/tree.rs`, `crates/library/tests/video.rs`, `crates/library/tests/tree.rs`, `crates/library/README.md`
  - scope: a photo's kind (still or video) and its video facts beside `Facts` (duration in
    milliseconds, frame size, rotation, codecs, sound, frame rate), set by ops without an undo step
    or a revision bump of their own (as M14.1's legacy facts); `Project::VideoFrames` beside
    `Exported` and `Collages` (the Projects group lists it with its icon's kind); queries: a
    section's videos (for the paths that count them) and the photos a still-only path takes; the
    Videos filter reading the kind.
  - done when: `crates/library/tests/video.rs`: a video's facts round-trip and move no revision; a
    section's videos are counted once; the still-only query leaves them out; the Videos filter
    keeps only them; `crates/library/tests/tree.rs`: a folder marked Video Frames is listed in
    Projects; every existing `library` test passes unchanged.
- ⬜ **M15.4 `engine`: M15's command modules, the `Os` and `Sound` services, settings and options rows**
  - after: M14.5, M14.R
  - files: `crates/engine/src/cmd/mod.rs`, `crates/engine/src/cmd/video.rs`, `crates/engine/src/cmd/video_output.rs`, `crates/engine/src/cmd/os.rs`, `crates/engine/src/cmd/camera.rs`, `crates/engine/src/video.rs`, `crates/engine/src/video_output.rs`, `crates/engine/src/os.rs`, `crates/engine/src/sound.rs`, `crates/engine/src/camera.rs`, `crates/engine/src/session.rs`, `crates/engine/src/options.rs`, `crates/engine/src/settings.rs`, `crates/engine/tests/settings.rs`, `crates/engine/tests/options_catalogue.rs`, `crates/engine/README.md`, `docs/menus-and-shortcuts.md`, `docs/data-model.md`, `NOTICE`
  - scope: the engine's ground for M15 so later tasks never share a file: empty `cmd/video.rs`,
    `video_output.rs`, `os.rs` and `camera.rs` joined into the registry once; stubs `video.rs` (the
    open video and its playback state), `video_output.rs` and `camera.rs` (camera sources), each a
    field of `Session` once; `os.rs`: the `Os` service (reveal a path with it selected, the
    handlers of a file's type, open a file with one, open files in the external editor, put files,
    a picture or text on the system clipboard, the default handler per type and setting it; every
    call fallible with a reason) with `os::Recording`, the fake that headless sessions,
    `dalica-cli`, MCP and tests use, the app plugging in the real one (as `Picker`); `sound.rs`:
    the `Sound` output (open at a rate, queue samples, its clock, stop) with a silent fake that
    keeps time and records what it was given; settings with typed accessors: `files.externalEditor`
    (empty: the system's default), `video.volume` (0–100, 80), `viewer.defaultViewer` turned from
    one flag into the set of chosen types (an old `true` or `false` read as all or none), and
    `interface.language`'s choices given by the caller at start (`Session::set_languages`, so
    `ui`'s registry is the one list; `system` first); the options catalogue's External editor row
    (General ▸ Files) available and Photo Viewer's File Types made a list (greyed until M15.24),
    Slideshow's Sound rows still greyed until M15.27; `menus-and-shortcuts.md` §1 lists the new
    commands (`photo.locate`, `folder.locate`, `photo.openWithList`, `photo.openWith`,
    `photo.openExternal`, `photo.copyToSystem`, `photo.copyPath`, `video.*`, `camera.*`,
    `viewer.fileTypes`, `viewer.setDefault`) and §2 the edit view's video keys; `data-model.md` §4
    lists the new keys; the `NOTICE` rows of every port named in M15's blocks.
  - done when: `crates/engine/tests/settings.rs` round-trips every new setting with unknown keys
    kept, wrong types and out-of-range values falling back (reported), and an old
    `viewer.defaultViewer` flag read as all or none; `crates/engine/tests/options_catalogue.rs`:
    the new rows with defaults equal to their accessors', Language listing the languages given;
    the recording `Os` keeps each call in order; every existing engine test and `panic_hunt` pass
    unchanged.
- ⬜ **M15.5 `ui`: M15's view state, UI commands and the stub video panel**
  - after: M14.6, M14.R
  - files: `crates/ui/src/state.rs`, `crates/ui/src/menus.rs`, `crates/ui/src/menu_catalog.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/commands/mod.rs`, `crates/ui/src/commands/video.rs`, `crates/ui/src/commands/os.rs`, `crates/ui/src/panels/mod.rs`, `crates/ui/src/panels/edit/video.rs`, `crates/ui/src/panels/edit/video/**`, `crates/ui/src/dialogs/mod.rs`, `crates/ui/src/dialogs/default_viewer.rs`, `crates/ui/src/control/**`, `crates/ui/tests/view_state.rs`, `docs/control-protocol.md`
  - scope: every M15 field of `UiState` in one go: the video in the edit view (playing, position,
    the trim handle being dragged, volume, full screen) and the edit panel's video tab; the Open
    With ▸ submenu's handlers (asked when it opens); a drag leaving the window; the default-viewer
    question after the first run; the language in effect (`interface.language` mirrored for the next
    draw); UI commands with stub handlers so M15.21–M15.29 never share a file: the player's
    (`video.play`, `.pause`, `.toggle`, `.step`, `.seek`, `.setStart`, `.setEnd`, `.volume`,
    `.fullScreen`, `.saveFrame`, `.exportClip`) in `commands/video.rs`, the OS integration's
    (`photo.locate`, `folder.locate`, `photo.openWith`, `photo.openExternal`, `photo.copyPath`,
    the notice's Show Folder, the viewer's Show in File Manager) in `commands/os.rs`; their menu rows
    of `menus-and-shortcuts.md` §1 wired (greyed until live); the video keys (`/`, Space, `,`, `.`)
    active in the edit view while a video is open; the stub panel `panels/edit/video.rs` and its
    folder (`bar.rs`, `tab.rs`); the stub dialog `defaultViewer`; control stubs for the new state;
    `control-protocol.md` §6.5 and §7 (ids following §7's patterns: `region:video`,
    `slider:video.position`, `handle:video.trimStart`, `button:video.saveFrame`,
    `menu:photo.openWith`, `dialog:defaultViewer`, …).
  - done when: `crates/ui/tests/view_state.rs`: each new field round-trips through `ui.state` /
    `ui.set` (bad values are `bad_params`, nothing changes); a video key does nothing in the library
    or on a still photo; every command's graceful-failure test; `panic_hunt` and `widget_ids` stay
    green.
- ⬜ **M15.6 `ui-kit`: the video controls and M15's icons**
  - after: M14.7, M14.R
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/scrubber.rs`, `crates/ui-kit/src/widgets/volume.rs`, `crates/ui-kit/src/icons.rs`, `crates/ui-kit/src/icons/video.rs`, `crates/ui-kit/src/icons/os.rs`, `crates/ui-kit/tests/video_widgets.rs`, `crates/ui-kit/tests/icons_video.rs`, `crates/ui-kit/tests/fingerprints/video-widgets*.json`, `crates/ui-kit/tests/fingerprints/icons-video*.json`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: `design-system.md` §4–§6, `editing.md` §11 and `viewing.md` §4: `scrubber` (a track
    with the played part, a playhead dragged or clicked, the trim ruler's start and end handles that
    never cross, the trimmed-away parts dimmed, the readout "00:00:09 / 00:00:41 (trimmed to
    00:00:21)" from given milliseconds, arrow-key steps); `volume` (a speaker toggle and a short
    slider); the icons it lacks, our own drawings on the 20-unit grid: play, pause, frame back,
    frame forward, Set Start, Set End, volume (off, low, high), Save Frame, Export Clip, the Video
    Frames project, Open With, Locate on Disk, external editor, camera; the widget gallery gains
    them; `design-system.md` §6's rows.
  - done when: `crates/ui-kit/tests/video_widgets.rs`: dragging the playhead reports positions in
    milliseconds clamped to the clip; the start handle never passes the end; the readout formats 0,
    59 999 ms and 3 h 2 min right and adds the trimmed length only when trimmed; the toggle mutes
    and restores the volume; `crates/ui-kit/tests/icons_video.rs`: each new icon stays in its grid
    at 12, 16, 20 and 28 px in rest, hover and disabled colours; gallery and icon fingerprints;
    `widget_ids`, contrast, `cargo xtask assets` and `cargo xtask names` pass.
- ⬜ **M15.7 `ui` + `xtask`: the localisation core and the pseudo-locale**
  - after: M10.10, M14.R
  - files: `crates/ui/src/i18n.rs`, `crates/ui/src/i18n/catalog.rs`, `crates/ui/src/i18n/plural.rs`, `crates/ui/src/i18n/languages.rs`, `crates/ui/src/i18n/system.rs`, `crates/ui/src/i18n/format.rs`, `crates/ui/src/i18n/pseudo.rs`, `crates/ui/src/headless.rs`, `crates/ui/src/dialogs/options/general.rs`, `crates/ui/tests/i18n.rs`, `xtask/src/i18n.rs`, `docs/localization.md`, `docs/development.md`
  - scope: `localization.md`, ported from PhotoCraft `crates/ui-egui/src/i18n/mod.rs`, `catalog.rs`
    and `system.rs` at 5b087699 (each with its header): `tl!("…")`, `tl!(ctx = "…", "…")`, named
    placeholders (`tl!("{count} photos selected", count = n)`) and `tln!` for plurals; lookups of a
    command label by id and English text and of an options row by key and English text, for the
    places that draw them; the TSV parser (contexts, `@id`, `@plural`, escapes, comments) that
    never panics and turns a broken catalog into English with a log line; the plural rules of
    `localization.md`'s table; `LANGUAGES` (code, endonym, script, plural rule, `complete_menus`,
    catalog) holding English until M15.35, handed to the engine (`Session::set_languages`); the
    language in effect: System from the OS's preferred languages (read once, no subprocess),
    regional tags resolved to the closest catalog (Traditional and Simplified Chinese distinct),
    `DALICA_LANGUAGE` for one launch; Options ▸ General ▸ Language live (the interface follows on
    its next draw; views, the selection and the undo history kept); dates, numbers and file sizes
    through `i18n::format`; the release notes looked up line by line; the pseudo-locale `qps`
    (built at run time from the key set: each translation `⟦…⟧` and 40 % longer) and the headless
    driver's list of the texts a frame drew, so a test can name every untranslated text; `cargo
    xtask i18n`, ported from PhotoCraft `xtask/src/i18n_coverage.rs` at 5b087699: the key set
    (every `tl!` literal of `crates/ui` outside tests, every command label, every options row's
    label and hint), each language's coverage, and a failure for an empty entry, a placeholder in
    one column only, a malformed plural, a duplicate key or a missing key of a `complete_menus`
    language (joined to `cargo xtask ci` by M15.35, once catalogs exist); `localization.md`'s
    status and `development.md`'s tool list.
  - done when: `crates/ui/tests/i18n.rs`: a catalog fixture translates plain, context, id and
    plural entries and falls back to English for a missing one; plurals for 0, 1, 2, 5, 11, 21 and
    22 in each rule; hostile catalogs (a lone tab, a 1 MiB line, a placeholder in one column,
    broken escapes) never panic; `de-AT`, `pt-PT`, `zh-TW` and `zh-CN` resolve as the spec says;
    choosing the pseudo-locale over `options.set` changes the next frame's texts with the open view
    and the undo history kept; `cargo xtask i18n` passes on the tree and fails on each planted
    fault.
- ⬜ **M15.8 `ui-kit`: fallback faces from craft-fonts, by script**
  - after: M14.R
  - files: `crates/ui-kit/build.rs`, `crates/ui-kit/src/fonts.rs`, `crates/ui-kit/tests/fonts.rs`, `.github/workflows/ci.yml`, `docs/assets.md`, `docs/releasing.md`
  - scope: `releasing.md` and `assets.md` → Fonts, ported from LightCraft `crates/engine/build.rs`
    and `crates/engine/src/fonts.rs` at 4f0b110b: `crates/ui-kit/build.rs` embeds the faces of the
    craft-fonts manifest at `CRAFT_FONTS_DIR` (none without it: everything builds and runs with
    Inter alone; `CRAFT_FONTS_REQUIRED=1` makes a missing directory or face a build error naming
    it); `fonts::install` puts them after Inter as fallbacks, ordered by the language's script
    (LightCraft's script table in `crates/ui-egui/src/i18n.rs` at 4f0b110b: Japanese faces first
    for Japanese, and so on), switched with the language without a restart; the faces' OFL texts
    listed for the packages; `ci.yml` gets its `CRAFT_FONTS_REF` pin so the glyph tests run with
    the fonts; `releasing.md`'s "not built yet" replaced by what landed.
  - done when: `crates/ui-kit/tests/fonts.rs`: without `CRAFT_FONTS_DIR` the faces are Inter alone
    and a CJK string lays out without a panic; with it (skipped and named when absent) every glyph
    of a Japanese, a Simplified and a Traditional Chinese string and of each script the manifest
    lists has a face, in the order the script asks; `CRAFT_FONTS_REQUIRED=1` without the directory
    fails with a message naming it (a build-script unit test).
- ⬜ **M15.9 `scan`: cameras and phones over MTP/PTP**
  - after: M11.5, M14.R
  - files: `crates/scan/src/ptp.rs`, `crates/scan/src/ptp/**`, `crates/scan/src/devices.rs`, `crates/scan/tests/ptp.rs`, `crates/scan/fuzz/**`, `crates/scan/README.md`, `xtask/src/ci.rs`
  - scope: `organizing.md` §3, with our own reader written from the public PIMA 15740 (PTP) and
    USB-IF MTP 1.1 specifications (provenance header): containers (command, data, response, event)
    with their lengths checked; a session (OpenSession, GetDeviceInfo, GetStorageIDs,
    GetStorageInfo, GetObjectHandles, GetObjectInfo, GetThumb, GetObject and GetPartialObject in
    bounded chunks, DeleteObject, CloseSession); object dates parsed from their strings, names
    through `core::names::sanitize`; the transport a trait: USB bulk endpoints through a pure-Rust
    USB stack (`nusb`, after `cargo xtask deps`) and an in-memory responder for tests; `devices`
    lists cameras beside cards and drives (vendor, model, storages) and, on Linux, an MTP device
    the desktop already mounted (gvfs or kio) as a folder source; every length, count and string
    hostile (an object no larger than its info says and ≤ 4 GiB, ≤ 100 000 handles, strings ≤ 255
    UTF-16 units, a silent device timed out); where an OS holds the device for its own service
    (Windows' portable-device driver, macOS's image capture) what is reachable is measured and
    recorded in `crates/scan/README.md`, and a path that would need `unsafe`, a C library or a
    driver change blocks with that question (`AGENTS.md` §4); a fuzz workspace `crates/scan/fuzz`
    with the target `ptp` (excluded in the root `Cargo.toml`, listed where `xtask/src/ci.rs` lists
    fuzz workspaces; the Fuzz workflow runs every target).
  - done when: `crates/scan/tests/ptp.rs` (the in-memory responder): a camera with two storages and
    300 objects lists every picture with its name, size and date; GetObject in chunks gives the
    exact bytes; a short length, a handle count of 2³², an object larger than its info and a device
    that stops answering are refused or timed out without a panic; DeleteObject is sent only for
    the handle asked; proptest: random containers never panic; `cargo +nightly fuzz build` builds
    `ptp`.
- ⬜ **M15.10 Windows packages signed**
  - after: M6.22, M14.R
  - files: `packaging/windows/sign.ps1`, `packaging/windows/package.ps1`, `packaging/windows/test-sign.ps1`, `.github/workflows/release.yml`, `packaging/README.md`
  - scope: `releasing.md` → Windows and Secrets: the `sign Windows x64` job signs `dalica.exe` and
    `dalica-cli.exe`, builds the MSI around the signed programs and signs it, SHA-256 with an
    RFC 3161 timestamp, through `sign.ps1` with a `.pfx` (`WINDOWS_CERTIFICATE`) or Azure Trusted
    Signing (the `AZURE_*` secrets), whichever is set; a missing set stays a warning and an
    unsigned artifact; checksums are written after signing; on a dry run the same path signs with
    a throwaway self-signed certificate made in the job (never trusted, never drafted), so every
    packaging pull request runs it; `test-sign.ps1` checks each signature (signed, the expected
    thumbprint, a timestamp) and refuses an unsigned file when signing was asked; secrets reach the
    scripts only through environment variables.
  - done when: the release dry run of the task's pull request (`releasing.md` → Test runs; else the
    review's run) signs both programs and the MSI with the throwaway certificate and
    `test-sign.ps1` passes; with signing off it names each unsigned file; `actionlint` and the
    packaging lint pass; the real certificate is checked at M15.R.
- ⬜ **M15.11 `scan` + `store`: videos listed on scan**
  - after: M15.2, M15.3, M9.7, M10.11
  - files: `crates/scan/src/skip.rs`, `crates/scan/src/facts.rs`, `crates/scan/src/video.rs`, `crates/scan/tests/video.rs`, `crates/store/src/mapping.rs`, `crates/store/src/rebuild.rs`, `crates/store/tests/video.rs`
  - scope: `formats.md` §1: `FileType::of` maps `Format::Video` to Video (the wildcard that sends
    new formats to RAW no longer catches it); with `fileTypes.video` on (the default) a scan lists
    MP4, MOV, M4V and MKV files as videos with M15.2's probe as their facts (the date from the
    container, else the file's time; a location imported as a geotag on the first scan, as M6.32
    imports file metadata; the rotation applied to the cell); turning the type off and on rescans
    as M10.11 does, keeping the videos' stars, captions and tags; a damaged or hostile container
    listed with its reason, never a panic; the index and a rebuild keep the facts; the sidecar
    keeps a video's star, caption, tags and recipe as a photo's (`.dalica.json` unchanged in
    shape).
  - done when: `crates/scan/tests/video.rs`: a folder with an MP4, a MOV, an MKV and a JPEG lists
    four, three of them videos with their durations and dates; with the type off it lists one and
    the videos' sidecar entries stay; a cut MP4 is listed with its reason;
    `crates/store/tests/video.rs`: a starred, captioned video survives a restart and a rebuild with
    its facts; the "nothing silently dropped" mapping test knows the new facts.
- ⬜ **M15.12 `dalica`: the platform services on each OS**
  - after: M15.4
  - files: `apps/dalica/src/os.rs`, `apps/dalica/src/os/**`, `apps/dalica/src/main.rs`, `apps/dalica/tests/os.rs`, `apps/dalica/README.md`
  - scope: the real `Os` (M15.4) for Linux, Windows and macOS, with no `unsafe` in our code and no
    C build dependency (a pure-Rust crate after `cargo xtask deps`, or the platform's own program
    started with an argument vector, never a shell; a path starting with `-` passed as `./-name`;
    `security.md` §5.11): **reveal** (Windows `explorer /select,`; macOS `open -R`; Linux the
    `org.freedesktop.FileManager1.ShowItems` D-Bus call, else the folder opened), ported from
    LightCraft's `reveal` service in `apps/lightcraft/src/main.rs` at 4f0b110b; **handlers** of a
    file's type (Linux: the desktop entries whose `MimeType` lists it, in `mimeapps.list` order,
    read bounded; Windows: the registry's `OpenWithProgids` and `OpenWithList`; macOS: the
    applications LaunchServices offers) with their names and icons where the OS gives them; **open
    with** one of them; **external editor**, ported from LightCraft `apps/lightcraft/src/editors.rs`
    at 4f0b110b (a named program, `open -a` on macOS, a Flatpak's forwarding; empty: the system's
    edit verb or default application); **clipboard**: files (`text/uri-list` and
    `x-special/gnome-copied-files` on Linux, a file-drop list on Windows, file URLs on macOS), a
    picture and text through `arboard` as PhotoCraft does (`apps/photocraft/src/services.rs` at
    5b087699, its Wayland data-control path included); calls that can block run off the UI thread;
    a feature with no such way on an OS answers "not available on this system" (and blocks with
    the question when only `unsafe` or a C library would do, `AGENTS.md` §4); the app plugs it in.
  - done when: `apps/dalica/tests/os.rs`: each OS's argument vectors (a path with spaces, a leading
    `-`, non-ASCII) never pass through a shell; handler lists parse from fixture desktop entries and
    `mimeapps.list` files (hostile ones bounded, no panic); on the OS the test runs on, reveal and
    open-with start the right program (a stub program on `PATH` records its arguments); files and
    text round-trip through the clipboard where a display is available (skipped and named
    otherwise).
- ⬜ **M15.13 macOS packages signed and notarized**
  - after: M6.23, M15.10
  - files: `packaging/macos/import-cert.sh`, `packaging/macos/package.sh`, `packaging/macos/notarize.sh`, `packaging/macos/verify.sh`, `packaging/macos/common.sh`, `packaging/macos/entitlements.plist`, `.github/workflows/release.yml`, `packaging/README.md`, `docs/releasing.md`
  - scope: `releasing.md` → macOS: the `sign macOS universal` job imports the Developer ID
    certificate into a temporary keychain (`import-cert.sh`), signs inside-out with the hardened
    runtime and a secure timestamp (no `--deep`; empty entitlements unless a feature proves it needs
    one), notarizes the app, the DMG and the CLI zip with `xcrun notarytool submit --wait` (the
    three Apple secrets, all or none), staples the app and the DMG, and `verify.sh` runs strict
    (`codesign --verify --strict`, `stapler validate`, `spctl --assess`); a missing set stays a
    warning and an ad hoc signature; on a dry run the job signs with a self-signed code-signing
    identity made in a temporary keychain (notarization skipped and named), so the signing path
    runs on each packaging pull request; the keychain is deleted at the job's end whatever happens;
    `releasing.md` says what landed for both platforms (M15.10's and this).
  - done when: the release dry run of the task's pull request (else the review's run) signs the app
    and the CLI with the temporary identity and `verify.sh` passes its signature checks on the DMG
    as users download it (after a `ditto` copy); without the secrets it warns and signs ad hoc as
    before; `test.sh`, `actionlint` and the packaging lint pass; the Developer ID signature and
    notarization are checked at M15.R.
- ⬜ **M15.14 `ui-kit`: AccessKit for every widget, and the texts `ui-kit` draws**
  - after: M15.6
  - files: `crates/ui-kit/src/widgets.rs`, `crates/ui-kit/src/widgets/**`, `crates/ui-kit/src/registry.rs`, `crates/ui-kit/src/text.rs`, `crates/ui-kit/tests/accessibility.rs`, `crates/ui-kit/tests/fingerprints/gallery*.json`, `docs/design-system.md`
  - scope: each widget reports its AccessKit role, its name (the label, else the tooltip, else
    the registry id's last part) and its state: buttons and tool buttons, toggles and checkboxes
    (checked), sliders, the age slider, the effect slider and the scrubber (value, range, step, the
    value as text), the stepper, dropdowns (expanded, the chosen item), text fields (value,
    placeholder), tabs (done), list and tree rows (selected, expanded, level), swatches (the colour
    as text), effect tiles, key cells, face tiles and boxes, the map (a group with its zoom);
    Enter and Space activate, and a focus ring from `Tokens` shows on every interactive widget;
    the few texts `ui-kit` draws itself (the date picker's month and weekday names, the colour
    picker's labels, …) come from a table the app sets (`ui_kit::text`), English by default, so
    `ui`'s catalogs translate them; `design-system.md` gains the focus ring and these rules.
  - done when: `crates/ui-kit/tests/accessibility.rs` (the gallery headless, AccessKit on): no
    interactive node without a name; every slider has its value and range; a checkbox's state
    follows a click; Tab moves the focus through the gallery in order and Space activates the
    focused button; the focus ring passes the contrast test; setting `ui_kit::text` changes the
    date picker's month names; gallery fingerprints change only by the focus ring.
- ⬜ **M15.15 `image` (reserved decision 2): video frames**
  - after: M15.1, M15.2
  - files: `crates/image/src/formats/video/frames.rs`, `crates/image/src/formats/video/frames/**`, `crates/image/tests/video_frames.rs`, `docs/formats.md`
  - scope: what reserved decision 2's answer builds (M15.1 records it and fits this block):
    decoding a video's frames, by a pure-Rust decoder for the codecs the answer names (after
    `cargo xtask deps`) or by the bridge to an optional external program the user installed
    (`security.md` §5.11: found where the user says, an argument vector with fixed arguments, a
    timeout, its output read under §4.2's limits, never a shell); the poster frame (the first frame
    whose mean luminance clears a small threshold within the first 10 s, else the first frame),
    the frame at a time, the next and the previous frame, scaled at decode where the decoder can,
    upright by the container's rotation, and the sound as samples for M15.27; a codec it cannot
    decode answers "This video can't be played" with the codec's name; memory bounded by the
    frame size; `formats.md`'s Video row says what landed. A worker who finds the answer missing
    from *Reserved decisions* runs `tasks block` with the question.
  - done when: `crates/image/tests/video_frames.rs` (clips made by the test, or public-domain clips
    from the corpus job; never committed media): a clip that starts black gives its first lit frame
    as poster; the frame at 1.5 s of a 2 s clip is the expected one within one frame; stepping
    forward then back returns the same frames; a turned clip comes out upright; an unsupported
    codec, a truncated stream and (for an external program) a missing program, a hang and a flood
    of output are refused with their reasons without a panic.
- ⬜ **M15.16 `engine`: videos in the session**
  - after: M15.4, M15.11
  - files: `crates/engine/src/cmd/video.rs`, `crates/engine/src/video.rs`, `crates/engine/src/thumbs.rs`, `crates/engine/tests/videos.rs`
  - scope: needed under any answer: `video.inspect {id}` (the facts, whether Dalica can play it, the
    trim); `photo.inspect` and `library.query` carry the kind and duration; every path that cannot
    take a video answers by kind and count, never by failing on a decode: Export skips and counts
    videos (until M15.28), the slideshow leaves them out and counts them (until M15.27), Batch
    Edit, Copy and Paste All Edits, effects, Crop and Save refuse a video with the reason, a
    collage's clips refuse one with a notice (until M15.28), the face scan skips them (until
    M15.20), the viewer lists them with the video tile; a video's thumbnail is the video tile until
    M15.20 (never the "!" of a decode error); a path found mishandling a listed video is fixed here
    after asking the lead for its file (`AGENTS.md` §7).
  - done when: `crates/engine/tests/videos.rs`, in a library of two photos and a clip:
    `video.inspect` reports the facts; an export of the section writes two files and counts one
    video; the slideshow plays two and counts one; Batch Edit, an effect, Crop and Save on the clip
    are refused with the reason and nothing is written; the face scan never decodes it; the owner
    thread made no `fs` call; `panic_hunt` green.
- ⬜ **M15.17 `engine`: Locate on Disk, Open With, the external editor and the system clipboard**
  - after: M15.4, M8.10
  - files: `crates/engine/src/cmd/os.rs`, `crates/engine/src/os.rs`, `crates/engine/tests/os.rs`
  - scope: `organizing.md` §9 and `security.md` §5.11 through M15.4's `Os`: `photo.locate {id}`
    and `folder.locate {id}` (the file or folder selected in the system's file manager; an offline
    folder disabled with its reason; an album's photo located in its folder);
    `photo.openWithList {id}` (the handlers, Dalica itself left out); `photo.openWith {id,
    handler}`; `photo.openExternal {ids}` (`files.externalEditor`, else the system's editor; more
    than 20 files asks first); `photo.copyToSystem {ids, picture?}` (the files; with `picture` and
    one photo, its picture with its edits through the one output pipeline at its size, at most
    8 192 px on the long side, on a worker); `photo.copyPath {ids}` (one path per line); the
    launching commands refused over automation with `forbidden` (`security.md` §6.6) and the copy
    commands answering `unavailable` in a headless session; a missing file answers with its reason
    before the OS is asked; a video handled as its file.
  - done when: `crates/engine/tests/os.rs` (the recording `Os`): each command hands the right paths
    and handler, in order; over automation the launching commands are refused and nothing is
    recorded; a photo with a crop and an effect copies a picture of its edited size; Copy Path of
    three photos gives three lines; a missing file answers its reason; each command's contract and
    graceful failures; the owner thread made no `fs` call; `panic_hunt` green.
- ⬜ **M15.18 `dalica` + `engine`: one Dalica: files handed to the running one**
  - after: M15.12, M11.12, M11.17
  - files: `apps/dalica/src/instance.rs`, `apps/dalica/src/instance/**`, `apps/dalica/src/main.rs`, `apps/dalica/src/viewer.rs`, `crates/engine/src/viewer.rs`, `crates/engine/src/cmd/viewer.rs`, `crates/ui/src/panels/viewer.rs`, `apps/dalica/tests/instance.rs`, `docs/viewing.md`
  - scope: `viewing.md` §3.4 with the maintainer's answer at M11.R, ported from PhotoCraft
    `apps/photocraft/src/single_instance.rs` at 5b087699: the first Dalica of a user takes an OS
    lock on `instance.lock` in the settings folder, listens on 127.0.0.1 only and writes its port
    and a fresh 256-bit token to `instance.json` (`0600`, through `fs`); a later `dalica --view
    FILE` (or a launch with files from Open With) that finds the lock taken sends its paths with the
    token (≤ 64 paths, each ≤ 4 KiB, one request, a timeout, the token compared in constant time)
    and exits once the first says it took them; otherwise it keeps trying the lock and, when its
    time runs out, starts on its own as today (a file is never acknowledged and dropped); the
    running Dalica brings its window forward and opens the viewer on the first file (borderless,
    full screen, as `--view`), and Esc returns the window to where it was instead of quitting
    (`Session::close_viewer` knows the viewer was handed in); M11.12's read-only viewer stays for a
    library another user's Dalica holds; on macOS the open-documents events (Finder's Open With, a
    file dropped on the Dock icon) open the viewer the same way; a portable copy and a settings
    folder from the environment skip the hand-off; `viewing.md` §3.4 records it, and the PR the
    channel's limits for `security.md` (M15.36).
  - done when: `apps/dalica/tests/instance.rs`: two processes started at once with one settings
    folder end as one, the second's file open in the first's viewer and the second exited 0; a
    wrong token, an oversized request and 65 paths are refused and nothing opens; a first Dalica
    that hangs makes the second start on its own after its time; Esc after a hand-off leaves the
    library window where it was; the viewer's existing engine and `ui` tests stay green.
- ⬜ **M15.19 `engine` + `ui`: cameras in the Import room**
  - after: M15.4, M15.5, M15.9, M11.8, M11.20
  - files: `crates/engine/src/cmd/camera.rs`, `crates/engine/src/camera.rs`, `crates/engine/src/import.rs`, `crates/engine/src/import_run.rs`, `crates/engine/src/import/worker.rs`, `crates/engine/tests/import_camera.rs`, `crates/ui/src/panels/import/source.rs`, `crates/ui/tests/import_camera.rs`, `docs/organizing.md`
  - scope: `organizing.md` §3: cameras and phones in Import from (name and storage) beside cards and
    drives, listed as they connect; a camera's pictures (and videos, with the type on) listed
    through M15.9 on the import worker, the device's thumbnails filling the import tray first;
    duplicates found as M11.7 finds them, reading an object when the check needs its bytes; copies
    through `dalica-fs`: each object read in chunks into a new temporary file, `fsync`, checked by
    reading the copy back against the digest of what was received, named and dated as an import
    from a card (the object's capture date as the file's time), never replacing a file; "Delete
    copied files from the card" for a camera: DeleteObject for each object that import copied and
    checked, only while the device still reports its size and date, never over automation; a
    camera unplugged mid-import stops after the current file with what was copied kept and the
    reason shown; what each OS lets Dalica reach (M15.9's measurements) in `organizing.md` §3, and
    the PR the PTP limits for `security.md` (M15.36).
  - done when: `crates/engine/tests/import_camera.rs` (M15.9's in-memory responder): a camera's 40
    pictures import with their bytes and times; a second import skips them as duplicates; removal
    deletes exactly those 40 handles after the question; an unplug at picture 12 keeps 11 and says
    why; `crates/ui/tests/import_camera.rs` (headless, by id): the camera shows in Import from with
    its name and its pictures fill the tray; the owner thread made no `fs` call.
- ⬜ **M15.20 `thumbs` + `engine` (reserved decision 2): poster frames**
  - after: M15.15, M15.16, M5.16
  - files: `crates/thumbs/src/video.rs`, `crates/thumbs/src/ladder.rs`, `crates/thumbs/tests/video.rs`, `crates/engine/src/thumbs.rs`, `crates/engine/src/preview.rs`, `crates/engine/src/preview/jobs.rs`, `crates/engine/src/faces_scan.rs`, `crates/engine/tests/video_posters.rs`
  - scope: what the answer builds (M15.1 fits this block): a video's thumbnails and its edit-view
    preview made from its poster frame (M15.15) on the thumbnail workers, below photos in priority,
    cached as a photo's and made again when the file changes, the video tile kept while it is made
    and for a video Dalica cannot play; the slideshow, the viewer and a collage get the poster where
    they show a still; the face scan reads the poster frame (faces in videos, M13's *Out*); never a
    decode on the UI thread.
  - done when: `crates/thumbs/tests/video.rs`: a clip's thumbnail is its first lit frame at every
    ladder size and is cached; an unplayable clip keeps the video tile with no error badge;
    `crates/engine/tests/video_posters.rs`: a folder of 20 photos and 5 clips fills every thumbnail,
    photos first; the face scan finds a face drawn on a clip's poster frame; the owner thread made
    no `fs` call.
- ⬜ **M15.21 `ui`: video cells, and a video in the edit view**
  - after: M15.5, M15.6, M15.16
  - files: `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/src/panels/edit/video.rs`, `crates/ui/src/panels/edit/video/**`, `crates/ui/src/panels/edit/panel.rs`, `crates/ui/src/panels/edit/filmstrip.rs`, `crates/ui/tests/video_tiles.rs`, `crates/ui/tests/fingerprints/video-*.json`, `docs/ui-layout.md`
  - scope: needed under any answer: `ui-layout.md` §5's video cells (the sprocket edge and the
    duration pill from `badges.rs`, fed by the facts; `LIB-LT-VIDEO`) in the light table and the
    filmstrip; the Videos filter showing them; double-click and Enter open a video in the edit view
    with its poster (once M15.20 makes it) or the video tile, fitted, and for one Dalica cannot
    play "This video can't be played" with [Open With ▸] (running `photo.openWith`, live with
    M15.22); the edit panel shows the video tab (rotate live, the rest greyed until M15.26) instead
    of tabs 1–5, and the tools, effects and Save are greyed for a video with the reason;
    `ui-layout.md` records it.
  - done when: `crates/ui/tests/video_tiles.rs` (headless, by id): a folder with a clip shows its
    cell with the sprocket edge and its duration; the Videos filter leaves only it; Enter opens it
    with the video tab and the tools greyed; at 900 × 600 the tab fits; fingerprints of a video
    cell and of the edit view on an unplayable clip at 1400 × 900.
- ⬜ **M15.22 `ui`: Locate on Disk, Open With, the external editor and the clipboard in the window**
  - after: M15.5, M15.12, M15.17
  - files: `crates/ui/src/commands/os.rs`, `crates/ui/src/commands/clipboard.rs`, `crates/ui/src/menu_catalog.rs`, `crates/ui/src/panels/light_table/context_menu.rs`, `crates/ui/src/panels/light_table/header.rs`, `crates/ui/src/panels/bottom/tray.rs`, `crates/ui/src/notices.rs`, `crates/ui/src/panels/viewer/bar.rs`, `crates/ui/src/dialogs/options/general.rs`, `crates/ui/tests/os_menus.rs`, `docs/organizing.md`, `docs/menus-and-shortcuts.md`
  - scope: `organizing.md` §9 and `menus-and-shortcuts.md` §1: the photo and tray menus' Open in
    External Editor (Ctrl+Shift+O), Open With ▸ (the handlers with their icons, asked when the
    submenu opens, "Loading…" until then, "No other applications" when none) and Locate on Disk
    (Ctrl+Enter); the folder menu's Locate on Disk and a folder header's icon (`ui-layout.md` §5);
    the export notice's Show Folder (`creating.md` §2); the viewer's ⋯ Open With ▸ and Show in File
    Manager; Copy Path; Edit ▸ Copy also putting the files (and one photo's picture) on the system
    clipboard, Cut and Paste unchanged inside Dalica; Options ▸ General's External editor row
    ([System default ▾] or a program chosen with [Browse…] through the picker); each "Arrives with
    OS integration" reason removed where the feature landed and kept, with the OS's reason, where an
    OS cannot do it; `organizing.md` and `menus-and-shortcuts.md` record what landed.
  - done when: `crates/ui/tests/os_menus.rs` (headless, by id, the recording `Os`): each menu row
    and the folder icon run their command with the right ids; Open With ▸ lists the fake's handlers
    and choosing one records it; Ctrl+C in the library records the files on the system clipboard
    and still pastes inside Dalica; Browse… sets `files.externalEditor`; an OS answering "not
    available" greys the row with its reason; at 900 × 600 the submenu stays inside the window.
- ⬜ **M15.23 `ui` + `dalica`: photos dragged out to other applications**
  - after: M15.12, M15.17, M15.18, M9.23
  - files: `crates/ui/src/drag.rs`, `crates/ui/src/drag/**`, `apps/dalica/src/drag_out.rs`, `apps/dalica/src/drag_out/**`, `apps/dalica/src/main.rs`, `crates/ui/tests/drag_out.rs`
  - scope: `FILE-DRAGOUT` and `ui-layout.md` §5: a drag of photos (thumbnails, tray minis, the
    filmstrip) that leaves the window becomes the OS's drag of their files (a copy; the source is
    never deleted: a target that moves the file is seen as any outside move), with the drag pill as
    its image, behind a `DragOut` service the app implements (as `Picker`) on Windows, macOS and
    Linux (X11 and Wayland), each by a pure-Rust way; where an OS has none without `unsafe` or a C
    library, the task blocks with the question (`AGENTS.md` §4) and that OS keeps the clipboard
    path; the files on disk are what leaves (unsaved edits stay in Dalica, and the drag pill says
    "Unsaved edits stay in Dalica" when one has them); a drag that comes back in stays an inside
    drag.
  - done when: `crates/ui/tests/drag_out.rs` (headless, a recording `DragOut`): a drag of three
    thumbnails out through the window's edge hands their three paths once and no inside drop
    target fires; a drag released inside stays an inside drop; a photo with unsaved edits shows the
    note; the PR names the targets tried on each OS where it landed (a file manager, a mail draft,
    an editor).
- ⬜ **M15.24 `engine` + `ui` + `dalica` + packages: Dalica as the default viewer**
  - after: M15.12, M15.17, M15.18, M11.17
  - files: `crates/engine/src/cmd/os.rs`, `crates/engine/src/os.rs`, `crates/engine/tests/default_viewer.rs`, `apps/dalica/src/os/default_viewer.rs`, `apps/dalica/tests/default_viewer.rs`, `crates/ui/src/dialogs/options/viewer.rs`, `crates/ui/src/dialogs/viewer_settings.rs`, `crates/ui/src/dialogs/default_viewer.rs`, `crates/ui/src/dialogs/first_run.rs`, `crates/ui/tests/default_viewer.rs`, `packaging/linux/org.dalica.Dalica.desktop`, `packaging/windows/dalica.wxs`, `packaging/macos/Info.plist.in`, `docs/viewing.md`, `docs/releasing.md`
  - scope: `viewing.md` §3.3 and `menus-and-shortcuts.md` §5 Photo Viewer: `viewer.fileTypes` (each
    type Dalica reads, with its current default handler's name, through `Os`) and
    `viewer.setDefault {types}`; the File Types tab of Options ▸ Photo Viewer and of the viewer's
    Settings… live: "Use Dalica as the default viewer for these file types:" with a checkbox per
    type showing its handler ("JPEG (Image Viewer)"), [Current] [Select All] [Select None], or
    "Don't use Dalica as a viewer"; offered once after the first run (`dialog:defaultViewer`,
    opt-in per type, nothing chosen by default); per OS: Linux writes the chosen types' defaults
    into the user's `mimeapps.list` through `fs`, merged (other sections, keys and comments kept; a
    damaged file kept aside), and "Don't use" puts back the handlers it replaced; Windows registers
    Dalica's per-user ProgIDs and Capabilities and opens Settings ▸ Default apps (only the user may
    choose there), saying so; macOS sets the role handler where a safe binding passes `cargo xtask
    deps`, else opens System Settings and says so; the packages declare the types (the desktop
    entry's `MimeType`, the MSI's capabilities and Open with entries, `Info.plist`'s document types
    as Viewer; HEIC and AVIF left out while release builds cannot show them); `viewing.md` §3.3 and
    `releasing.md` record it.
  - done when: `crates/engine/tests/default_viewer.rs` (the recording `Os`): the types list with
    their handlers and setting three records three; `apps/dalica/tests/default_viewer.rs`: a
    `mimeapps.list` with other sections, comments and an unknown key keeps them after Dalica is
    chosen for JPEG and PNG and after "Don't use" puts the old handlers back;
    `crates/ui/tests/default_viewer.rs` (headless, by id): the tab shows the fake's handlers, Select
    All then OK records every type, and the question after the first run appears once; the desktop
    entry validates and the packaging lint's WiX and plist checks pass.
- ⬜ **M15.25 `edit` + `engine` + `create` (reserved decision 2): playback, trim, Save Frame as Picture and Export Clip**
  - after: M15.15, M15.16, M8.10
  - files: `crates/edit/src/ops/video.rs`, `crates/edit/src/catalog.rs`, `crates/edit/tests/video_ops.rs`, `crates/engine/src/video.rs`, `crates/engine/src/cmd/video.rs`, `crates/engine/src/video_output.rs`, `crates/engine/src/cmd/video_output.rs`, `crates/engine/tests/video_play.rs`, `crates/create/src/video.rs`, `crates/create/tests/video_output.rs`, `docs/editing.md`, `docs/creating.md`
  - scope: what the answer builds (M15.1 fits this block): `trim(start,end)` in the recipe
    (milliseconds, start before end within the clip; a video-only operation in the catalogue; one
    step of the video's history; rotation as for photos); `video.open`, `.play`, `.pause`, `.seek
    {ms}`, `.step {frames}`, `.setStart`, `.setEnd`, `.volume`: frames decoded ahead on a worker so
    the edit view never waits, the sound clock (M15.4's `Sound`) driving playback, frames dropped
    rather than late when decoding falls behind, the trim respected; `video.saveFrame` (the frame
    shown, upright, a JPEG at quality 92 through the one output pipeline into `Pictures/Dalica/Video
    Frames` marked as the Video Frames project, named `<stem> <hh-mm-ss>.jpg` through
    `core::names::sanitize`, never replacing, entering the library dated the clip's date plus the
    frame's time); `video.exportClip {path?}` (the trimmed part, as the answer allows: copied
    without re-encoding from the key frame at or before the start where the container allows it,
    the real cut said in the reply; never replacing; over automation only inside the write roots);
    `editing.md` §11 and `creating.md` record it. A worker who finds the answer missing runs `tasks
    block` with the question.
  - done when: `crates/edit/tests/video_ops.rs`: `trim` parses and prints, and refuses `start ≥ end`
    and negative values; `crates/engine/tests/video_play.rs` (the silent `Sound`): playing 1 s of
    its clock shows the frames due within one frame; a trim survives a restart; Save Frame at 1.5 s
    writes a JPEG of the frame's size into Video Frames, listed in the Projects group;
    `crates/create/tests/video_output.rs`: Export Clip of 0.5–1.5 s writes a file M15.2 probes at
    about 1 s; the owner thread made no `fs` call; `panic_hunt` green.
- ⬜ **M15.26 `ui` (reserved decision 2): the video player in the edit view**
  - after: M15.21, M15.25
  - files: `crates/ui/src/panels/edit/video.rs`, `crates/ui/src/panels/edit/video/**`, `crates/ui/src/commands/video.rs`, `crates/ui/tests/video_player.rs`, `crates/ui/tests/fingerprints/video-player-*.json`
  - scope: `editing.md` §11 and `viewing.md` §4, as the answer builds them: the control bar under the
    video (play and pause, the scrubber with its readout, volume, the trim ruler with Set Start and
    Set End, Full Screen) from M15.6's widgets; the video tab (Save Frame as Picture, Export Clip
    with the save picker, rotate); `/` and Space play and pause, `,` and `.` step a frame and, held,
    rewind or fast-forward; frames drawn from the engine's decoded frames (never a decode on the UI
    thread); what the answer leaves out greyed with its reason.
  - done when: `crates/ui/tests/video_player.rs` (headless, by id, the silent `Sound`): Space plays
    and the readout follows the clock; `.` steps one frame; dragging the start handle and Set End
    trim and the readout says "(trimmed to …)"; Save Frame adds a photo to Video Frames; the frame
    time while playing stays within the edit view's budget; at 900 × 600 the bar fits; fingerprints
    of the bar playing and trimmed at 1400 × 900.
- ⬜ **M15.27 `engine` + `ui` + `dalica` (reserved decision 2): videos and music in the slideshow**
  - after: M15.25, M11.21
  - files: `crates/engine/src/sound.rs`, `crates/engine/src/cmd/slideshow.rs`, `crates/engine/tests/slideshow_sound.rs`, `apps/dalica/src/sound.rs`, `apps/dalica/src/main.rs`, `crates/ui/src/panels/slideshow/show.rs`, `crates/ui/src/dialogs/options/slideshow.rs`, `crates/ui/src/dialogs/folder_properties.rs`, `crates/ui/tests/slideshow_video.rs`, `docs/viewing.md`, `docs/menus-and-shortcuts.md`
  - scope: `viewing.md` §1.1 and §1.5, as the answer builds them (M15.1 fits this block; the
    maintainer's answer at M11.R ties music to video sound): the audio output in the app behind
    M15.4's `Sound` (a pure-Rust crate after `cargo xtask deps`, or what the answer allows; with
    none the rows stay greyed with their reason); a video in a slideshow plays to its end (its trim
    respected) before the show moves on, muted unless Play video sound; Play music: every MP3, OGG,
    FLAC and WAV of the music folder (a folder's own Music in Folder Properties wins) in name order,
    looping, decoded on a worker, faded out when the show ends; Options ▸ Slideshow's Sound group
    and Folder Properties' Music live ("Arrives with sound support" removed); `viewing.md` and
    `menus-and-shortcuts.md` §5 record it.
  - done when: `crates/engine/tests/slideshow_sound.rs` (the silent `Sound` recording samples): a
    show of a photo, a 2 s clip and a photo holds the clip 2 s; video sound off records silence
    and on records the clip's sound; the music folder's three files play in name order and loop; a
    damaged music file is skipped with its reason; `crates/ui/tests/slideshow_video.rs`
    (headless): the Sound rows are live and the clip's frames show in the show.
- ⬜ **M15.28 `create` + `engine` + `ui` (reserved decision 2): videos in Export and in collages**
  - after: M15.20, M14.17, M8.16
  - files: `crates/create/src/export.rs`, `crates/create/src/export/plan.rs`, `crates/create/tests/export_video.rs`, `crates/engine/src/cmd/export.rs`, `crates/engine/src/collage.rs`, `crates/engine/tests/collage_video.rs`, `crates/ui/src/dialogs/export.rs`, `crates/ui/tests/export_video.rs`, `docs/creating.md`
  - scope: `creating.md` §2 and §3, as the answer builds them: Export's "Export videos as: (•)
    First frame ( ) Full video (no resizing)" live (disabled without videos): the poster frame
    (M15.20) exported as a JPEG like a photo, or the file copied as it is under the export's
    numbered name, never replacing; a collage's clips take a video as its poster frame;
    `creating.md` records it.
  - done when: `crates/create/tests/export_video.rs`: an export of two photos and a clip as first
    frame writes three JPEGs, as full video two JPEGs and the clip byte for byte;
    `crates/engine/tests/collage_video.rs`: a collage with a clip places its poster and the render
    shows it; `crates/ui/tests/export_video.rs` (headless): the row is live with a video held and
    disabled without.
- ⬜ **M15.29 `ui`: the window's AccessKit tree, keyboard-only use and scaling**
  - after: M15.14, M15.21, M15.22
  - files: `crates/ui/src/a11y.rs`, `crates/ui/src/a11y/**`, `crates/ui/src/app.rs`, `crates/ui/src/keys.rs`, `crates/ui/src/panels/menubar.rs`, `crates/ui/src/panels/light_table/layout.rs`, `crates/ui/src/panels/folders/model.rs`, `crates/ui/tests/accessibility.rs`, `crates/ui/tests/keyboard_only.rs`, `crates/ui/tests/scaling.rs`, `docs/ui-layout.md`, `docs/design-system.md`
  - scope: `SYS-A11Y`, with LightCraft's audit ported (`crates/ui-egui/tests/accessibility.rs` at
    4f0b110b): the window's AccessKit tree: the regions as named landmarks (tree, light table,
    side pane, edit view, rooms); the light table's visible cells as list items (file name, star,
    caption, a video's duration, selected) inside their sections (named groups); the tree's rows
    (level, expanded, count); the edit and collage canvases and the map as named images with their
    state; dialogs as dialogs with their titles; built only for what is on screen (never work
    proportional to the library); keyboard-only: F10 and Alt open the menu bar and the arrows walk
    it, Tab and Shift+Tab move through the regions and through each dialog's controls in reading
    order, the focus always shown, every command reachable by its key or a menu; UI scale and font
    size: at UI scale 1.5 with the large font the main window at 1400 × 900, the edit view and
    every dialog keep their controls inside the window and their labels unclipped (an ellipsized
    label keeps its full text in AccessKit); `ui-layout.md` and `design-system.md` record it.
  - done when: `crates/ui/tests/accessibility.rs` (headless, AccessKit on): in the library, the
    edit view (a photo and a clip), the slideshow, the viewer, the Import and Collage rooms and
    every dialog no interactive node lacks a name and every slider has its value and range; at
    100 000 photos the tree holds only the visible cells; `crates/ui/tests/keyboard_only.rs`: every
    command with a menu row or a key is reached without a pointer, and Tab cycles every dialog's
    controls; `crates/ui/tests/scaling.rs`: at scale 1.5 and the large font nothing in those views
    is clipped at 1400 × 900.
- ⬜ **M15.30 `mcp` (reserved decision 2): videos over MCP, and the workflow W17**
  - after: M15.25, M14.21
  - files: `crates/mcp/tests/workflows/w17_video.rs`, `crates/mcp/tests/workflows/main.rs`, `crates/mcp/src/headless.rs`, `xtask/src/stats.rs`, `docs/mcp.md`
  - scope: `mcp.md` §9: W17 (**video**: `scan_folder` on a folder of two photos and a clip →
    `photo_info` shows the clip's kind, duration and date → `cmd_video_inspect` → with decoding,
    `cmd_edit_recipe_set` with `trim(500,1500)`, `cmd_video_save_frame` at 1 s → a JPEG in the
    Video Frames project → `cmd_export_run` with first frames writes three JPEGs; descoped, the
    steps up to `cmd_video_inspect` and the export's count, as M15.1 fits it); headless mode
    playing with the silent `Sound`; the floor in `xtask/src/stats.rs` raised to the seventeen
    workflows now required; `mcp.md`'s table and status line; row `SYS-MCP`.
  - done when: `cargo test -p dalica-mcp` runs W17 green; `cargo xtask stats` prints 17 / 17
    passing and `stats --check` fails below that.
- ⬜ **M15.31 `ui`: every string of the main window through the catalogs**
  - after: M15.7, M15.29
  - files: `crates/ui/src/panels/menubar.rs`, `crates/ui/src/panels/toolbar.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/src/status_text.rs`, `crates/ui/src/panels/activity.rs`, `crates/ui/src/panels/search_banner.rs`, `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/folders/**`, `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/light_table/**`, `crates/ui/src/panels/bottom.rs`, `crates/ui/src/panels/bottom/**`, `crates/ui/src/panels/side_pane.rs`, `crates/ui/src/panels/properties.rs`, `crates/ui/src/panels/properties/**`, `crates/ui/src/panels/tags.rs`, `crates/ui/src/panels/tags/**`, `crates/ui/src/panels/people_pane.rs`, `crates/ui/src/panels/people_pane/**`, `crates/ui/src/panels/places.rs`, `crates/ui/src/panels/places/**`, `crates/ui/src/panels/faces.rs`, `crates/ui/src/panels/faces/**`, `crates/ui/src/notices.rs`, `crates/ui/src/titlebar.rs`, `crates/ui/src/tray.rs`, `apps/dalica/src/native_menu.rs`, `crates/ui/tests/i18n_window.rs`
  - scope: `localization.md` → Writing translatable code, over the main window: every
    user-facing literal in `tl!` or `tln!` (counts as plurals, values as named placeholders, never
    a sentence built from pieces); the menu bar's, the macOS menu's and the context menus' labels
    looked up by command id and English label; tooltips, the status line and the notices; dates,
    numbers and sizes through `i18n::format`; a long label ellipsizes or wraps, never clips.
  - done when: `crates/ui/tests/i18n_window.rs` (headless, the pseudo-locale): in the library with
    the tree, a folder and an album, the tray, every side pane and a notice, every text the frame
    draws is bracketed except user data (names, captions, tags, paths); so are the menu bar and each
    menu; at 1400 × 900 and 900 × 600 no label clips; `cargo xtask i18n` passes.
- ⬜ **M15.32 `ui`: every string of the dialogs through the catalogs**
  - after: M15.7, M15.22, M15.24, M15.27, M15.28
  - files: `crates/ui/src/dialogs/**`, `crates/ui/src/credits.rs`, `crates/ui/tests/i18n_dialogs.rs`
  - scope: the same over every dialog: titles, labels, buttons, hints and questions; the Options
    dialog's rows and hints looked up by key and English text; the About window's texts
    (contributors' names are user data); the Keyboard Shortcuts editor's command labels by id; Help
    ▸ Release Notes line by line.
  - done when: `crates/ui/tests/i18n_dialogs.rs` (headless, the pseudo-locale): each dialog opened by
    id, and every tab of Options, draws only bracketed text outside user data; no label clips at
    1400 × 900 and 900 × 600; `cargo xtask i18n` passes.
- ⬜ **M15.33 `ui`: every string of the edit view, the rooms, the slideshow and the viewer through the catalogs**
  - after: M15.7, M15.18, M15.19, M15.26, M15.27
  - files: `crates/ui/src/panels/edit.rs`, `crates/ui/src/panels/edit/**`, `crates/ui/src/panels/import.rs`, `crates/ui/src/panels/import/**`, `crates/ui/src/panels/collage.rs`, `crates/ui/src/panels/collage/**`, `crates/ui/src/panels/rooms.rs`, `crates/ui/src/panels/slideshow.rs`, `crates/ui/src/panels/slideshow/**`, `crates/ui/src/panels/viewer.rs`, `crates/ui/src/panels/viewer/**`, `crates/ui/src/panels/peek.rs`, `crates/ui/tests/i18n_views.rs`
  - scope: the same over the edit view (its panel, tabs, tools, effect tiles and panels, the video
    player), the Import and Collage rooms and their tab strip, the slideshow and its bar, the
    viewer and the quick preview; effect names and parameter labels from the catalogue looked up
    by their English text.
  - done when: `crates/ui/tests/i18n_views.rs` (headless, the pseudo-locale): in the edit view on
    each tab, both rooms, the slideshow with its bar and the viewer, every text drawn is bracketed
    except user data; no label clips at 1400 × 900 and 900 × 600; `cargo xtask i18n` passes.
- ⬜ **M15.34 `image` + `engine` + `ui` + `xtask`: M15's budgets: videos, cameras, AccessKit and translations**
  - after: M15.19, M15.20, M15.26, M15.29
  - files: `crates/image/benches/video.rs`, `crates/engine/benches/video.rs`, `crates/engine/benches/import_camera.rs`, `crates/ui/benches/a11y.rs`, `crates/ui/benches/i18n.rs`, `xtask/src/bench.rs`, `.github/workflows/budgets.yml`, `docs/development.md`
  - scope: `development.md` §8: benches registered in `xtask/src/bench.rs`, joined to `BUDGETS`
    and the release-budgets job, each documented in §8 and §13 with its machine: `video_probe`
    (probing 1 000 containers); `video_play` (a clip's poster and its thumbnails, a frame step,
    and the playing frame time, as M15.1 fitted the decoding); `camera_import` (300 objects from
    the in-memory responder, files per second); `a11y_frame` (the library's frame at 100 000 photos
    with AccessKit on against off); `i18n_frame` (the same frame in the pseudo-locale against
    English); budgets at 1.5 × their first measurement, the playing frame within 16 ms at p95 and
    the AccessKit and translation overheads within 1 ms at p95 whatever the first numbers; a missed
    budget is fixed in this task (the module asked of the lead; before and after numbers in the PR)
    or blocks with its numbers, never loosened.
  - done when: `cargo xtask bench video_probe`, `video_play`, `camera_import`, `a11y_frame` and
    `i18n_frame` print their numbers (in the PR); `cargo xtask budgets` passes with them and
    `--plant-ms` makes each fail.
- ⬜ **M15.35 `ui` + docs: catalogs for the first languages**
  - after: M15.1, M15.8, M15.31, M15.32, M15.33
  - files: `crates/ui/src/i18n/*.tsv`, `crates/ui/src/i18n/LICENSE-translations.txt`, `crates/ui/src/i18n/languages.rs`, `crates/ui/tests/i18n_languages.rs`, `crates/ui/tests/fingerprints/i18n-*.json`, `xtask/src/ci.rs`, `docs/localization.md`, `docs/localization-*.md`, `ATTRIBUTION.md`
  - scope: the languages the maintainer chose at M14.R (M15.1 names them here): one catalog each,
    written from the meaning of Dalica's English text (`localization.md` → Clean-room translations;
    machine-assisted drafts marked for the fluent reviewer the check-in asks for), registered in
    `LANGUAGES` with code, endonym, script and plural rule, `complete_menus: true` once it covers
    every enforced key; a style guide `docs/localization-<code>.md` each (terminology, conventions,
    coverage with the revision checked); `LICENSE-translations.txt` and the `ATTRIBUTION.md` row; a
    CJK language checked for glyphs with `CRAFT_FONTS_DIR`; `cargo xtask i18n` joined to `cargo
    xtask ci`; `localization.md`'s status and its languages.
  - done when: `cargo xtask i18n` passes inside `cargo xtask ci` with every chosen language
    complete for menus; `crates/ui/tests/i18n_languages.rs`: each language's plurals for 0, 1, 2, 5
    and 21 read right; the library, the edit view and the Options dialog in each language have no
    clipped label at 1400 × 900 and 900 × 600 and (with the fonts) no missing glyph; a fingerprint
    per language of the library at 1400 × 900.
- ⬜ **M15.36 `docs`: the 1.0 parity and security audit**
  - after: M15.13, M15.19, M15.23, M15.24, M15.26, M15.27, M15.28, M15.30, M15.34
  - files: `docs/parity.md`, `docs/product.md`, `docs/security.md`
  - scope: `process.md` §5 for 1.0: every row re-measured (its cited tests run, its evidence checked
    against the code), ✅ only with that evidence, a wrong ✅ downgraded with a note; the P0 and P1
    rows ✅ or 🟡 "awaiting the M15.R check by hand" for what only a person can check (the screen
    reader, input methods, the signed packages, a real camera, drag out on each OS); each 🟡 or ⬜
    left with its reason and, where it is not a check by hand, a fix task from the lead that this
    task waits for; `product.md`'s scope as shipped; `security.md`'s M15 status lines from what the
    tasks' PRs and READMEs record (§4.2's video and PTP rows, the instance channel, §5.11, §7.1's
    `video_probe` and `ptp` targets, §10's signing), each marked built with its task.
  - done when: `cargo xtask parity` passes and the summary is regenerated; the PR lists every row
    changed with its evidence and every `security.md` line with its task.
- ⬜ **M15.39 `sidecar` + `store` + `ui`: the fuzz targets security.md lists**
  - after: M14.R
  - files: `crates/sidecar/fuzz/**`, `crates/store/fuzz/**`, `crates/ui/fuzz/**`, `.github/workflows/fuzz.yml`, `Cargo.toml`, `docs/security.md`
  - scope: by the maintainer's answer at M13.R (2026-10-18): the fuzz targets `security.md` lists
    and no task wrote: the sidecar reader (`.dalica.json` and `people.json`), the store's journal
    and snapshot readers, and the control channel's request parser; each a cargo-fuzz workspace
    like `crates/meta/fuzz`, seeded from the crate's test files, excluded from the root workspace,
    and run by the Fuzz workflow for 60 s at reviews; `security.md`'s status lines.
  - done when: each target builds on stable with cargo-fuzz's coverage flags and runs 3 minutes
    locally with no finding (a finding is fixed with a regression test in its crate); the review's
    Fuzz run lists the three new jobs.
- ⬜ **M15.40 `xtask`: an SBOM for every release package and an advisory check**
  - after: M14.R
  - files: `xtask/src/sbom.rs`, `xtask/src/advisories.rs`, `xtask/src/main.rs`, `xtask/src/ci.rs`, `.github/workflows/release.yml`, `.github/workflows/ci.yml`, `docs/security.md`, `docs/releasing.md`
  - scope: by the maintainer's answer at M13.R (2026-10-18): (1) `cargo xtask sbom` writes a
    CycloneDX JSON SBOM per release package from the product graph `xtask deps` already builds,
    attached to the GitHub release next to `SHA256SUMS.txt`; (2) an advisory check against the
    RustSec advisory database (a pure-Rust tool such as `cargo-deny` or `cargo-audit`, run as a
    binary at the review gate and in the release workflow, never linked into the product): a new
    advisory fails it unless `security.md` §8.4 records it with an owner, a rationale and a review
    date (RUSTSEC-2023-0071 is the first such entry); `security.md` §8.5 and `releasing.md`.
  - done when: `cargo xtask sbom` lists every crate of the product graph with its licence for each
    target; the advisory check passes on `main` with the recorded exception and fails on a planted
    unrecorded advisory; a release dry run attaches the SBOMs.
- ⬜ **M15.37 `accept M15`**
  - after: M15.23, M15.24, M15.30, M15.34, M15.35, M15.36, M15.39, M15.40
  - files: `tests/acceptance/m15.rs`, `tests/acceptance/m15_*.rs`, `tests/acceptance/m15.md`, `tests/acceptance/fingerprints/m15-*.json`
  - scope: every *Done when* of M15's four areas in the real `dalica --headless` over the control
    channel and through `dalica-cli mcp`: a folder of photos and clips listed with video cells and
    durations, the Videos filter, Export and the slideshow counting or taking videos as the answer
    built; with decoding, posters, play, a frame step, a trim surviving a restart, Save Frame as
    Picture and a clip played to its end in a slideshow; each chosen language switched without a
    restart, translated and unclipped at 1400 × 900 and 900 × 600, the pseudo-locale's sweep; the
    AccessKit audit and the keyboard-only walk; Locate on Disk, Open With, Open in External Editor
    and the clipboard through the recording `Os`, refused over automation where they must be; a
    second `dalica --view` handing its file over; a camera (the in-memory responder) imported; the
    release budgets with `DALICA_ACCEPT_RELEASE=1` (inside `cargo xtask ci --full` a smaller debug
    run checks the paths and prints its times); whole-window fingerprints of the library with video
    cells, the player and the library in each chosen language; the checks by hand in `m15.md`, per
    OS (a screen reader on each OS walking the library and a dialog, an input method typing a
    caption, Locate on Disk, Open With, the external editor, a paste of copied photos in a file
    manager, a drag out, Dalica chosen as the default viewer and a photo opened from the file
    manager, a real camera or phone imported, the signed packages installed without a warning).
  - done when: `cargo xtask accept M15` passes everything in M15's *Done when*.
- ⬜ **M15.38 Lead: task blocks for M16**
  - after: M15.1
  - owner: lead
  - files: `docs/roadmap.md`
  - scope: write every M16 block from M16's outline rows (Themes, with PhotoCraft's
    `crates/ui-egui/src/theme.rs` named for the port; Colour search; any row the maintainer added)
    and the spec, with waves and the `M16.R` review (owner: maintainer); whether M16.R is a release
    and its number is asked at M15.R (`releasing.md`: releases after 1.0 are planned then), never
    guessed; a block that can port Craft-app code names the source files in its scope
    (`architecture.md` §9).
  - done when: `cargo xtask tasks sync --check` parses them and `tasks sync --dry-run` lists the new
    M16 tasks as planned.
- ⬜ **M15.R Release 1.0 gate**
  - after: M15.37
  - owner: maintainer
  - files: `docs/roadmap.md`, `docs/status.md`
  - scope: a release gate (Release 1.0): the lead runs `/milestone-review M15.R` to prepare it, with
    the version set to 1.0.0 and its `whats-new.md` section on the review branch, the release
    checklist (the signing secrets in the `release` environment, a signed MSI and a notarized DMG
    installed without a warning, `SHA256SUMS.txt`) and the sign-off question, and after the merge
    cuts and publishes 1.0.0 (`releasing.md`); the check-in page (open a folder with clips, play,
    step and trim one, Save Frame as Picture, play a slideshow with a clip and music; switch to each
    chosen language and back; walk the library and a dialog with a screen reader and with the
    keyboard alone, at UI scale 1.5; Locate on Disk, Open With, Open in External Editor, Copy and
    paste photos into a file manager, drag photos into another application, choose Dalica as the
    default viewer and open a photo from the file manager while Dalica runs; import from a camera
    or phone; install the signed packages on each OS; written as steps for the maintainer to try);
    the questions: the planning decisions above that change behaviour (TSV catalogs; engine
    messages with values left in English; Ctrl+C also filling the system clipboard; a drag out gives
    the files on disk; one Dalica per user and Esc returning to the library; Dalica sets a default
    viewer only where the OS allows; what each OS lets Dalica reach on a camera), the fluent review
    of each catalog, whether M16.R is a release and its number, anything the reserved decisions left
    open, the look of the video cells, the player and each language (approved with their
    fingerprints); *Where we stand* (the full honest assessment for 1.0) and the `status.md` numbers
    (the probe and poster times, the playing frame, camera import speed, the AccessKit and
    translation overheads, each language's coverage, the workflow pass rate, parity) on a
    `review/M15.R` branch, a pull request open. Only the maintainer merges that pull request and
    closes this issue; nothing after it starts until then.
  - done when: prepared: page published, PR open, both linked on the issue.

Waves (inside a wave, "‖" separates tasks whose `files` do not overlap; "(2)" marks a task that
waits on reserved decision 2 through M15.1):

```text
wave 1   M15.1 lead: M14.R's answers  ‖  M15.2 video probe  ‖  M15.3 model: videos
         ‖  M15.4 engine modules + Os + Sound  ‖  M15.5 ui state + stubs  ‖  M15.6 ui-kit video + icons
         ‖  M15.7 i18n core  ‖  M15.8 fallback faces  ‖  M15.9 PTP reader  ‖  M15.10 Windows signing
         (once M14.R is closed)
wave 2   M15.11 videos on scan  ‖  M15.12 platform services  ‖  M15.13 macOS signing
         ‖  M15.14 ui-kit AccessKit  ‖  M15.15 video frames (2)  ‖  M15.17 OS commands
         ‖  M15.19 camera import
wave 3   M15.16 videos in the session  ‖  M15.18 one Dalica  ‖  M15.22 OS integration in the window
wave 4   M15.20 poster frames (2)  ‖  M15.21 video cells  ‖  M15.23 drag out  ‖  M15.24 default viewer
         ‖  M15.25 playback, trim, Save Frame (2)
wave 5   M15.26 the player (2)  ‖  M15.27 slideshow sound (2)  ‖  M15.28 Export + collage videos (2)
         ‖  M15.29 AccessKit window + keyboard  ‖  M15.30 W17 (2)
wave 6   M15.31 strings: window  ‖  M15.32 strings: dialogs  ‖  M15.33 strings: views
         ‖  M15.34 M15 budgets
wave 7   M15.35 catalogs (first languages)  ‖  M15.36 1.0 parity + security audit
wave 8   M15.37 accept M15 → M15.R          (M15.38 lead: M16 blocks, any time after M15.1)
```

### M16 — Post-1.0 enhancements (outline)

Improvements the maintainer wants after 1.0, beyond parity with the reference organizer. Nothing
here starts before M15.R closes; the lead details these items into task blocks while M15 runs, and
the maintainer adds items here as they come up (maintainer decision, 2026-10-11).

| # | In | Done when (sketch) |
|---|---|---|
| M16 Themes | switchable themes, taken from the Craft apps: today's dark theme stays the default (it is LightCraft's look, LightCraft's only theme), plus PhotoCraft's twelve: Pro (Dark), Pro (Medium Gray), Pro (Light Gray), Pro (Light), Studio (Dark), Studio (Light), Classic, Adwaita (Light), Adwaita (Dark), Solarized Dark, Breeze Light, Breeze Dark (as of `storytold/photocraft` 30bd8a55). The same architecture and implementation as PhotoCraft's `crates/ui-egui/src/theme.rs`: a `ThemeKind` enum with id, label, `from_name` and `next`, one token set per kind (`Tokens::for_kind`), the theme kept in settings, one command per theme plus `ui.set {theme}`, and a Theme choice in Options › General › Interface. PhotoCraft's palettes and code are mapped onto Dalica's `ui-kit` `Tokens`, with a header naming the source and a `NOTICE` row (`architecture.md` §9), plus the Solarized palette's `ATTRIBUTION.md` row and MIT licence text | every theme passes the `ui-kit` contrast tests; a widget-gallery fingerprint per theme; switching by command, CLI and MCP repaints without a restart and survives one; the main window screenshotted in every theme |
| M16 Colour search | `color:` search and Tools ▸ Experimental ▸ Search for ▸ (colour) (`SRCH-COLOR`, P2; placed after 1.0 by the maintainer at M9.R) | `color:green` finds photos whose main colour is green, from a per-photo colour summary computed on scan |

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
4. Map tiles provider and whether maps are on by default (M13). **Decided 2026-10-18 (the
   maintainer, at M13.R): OpenStreetMap's standard tiles and Nominatim place search, both off by
   default** until the user allows them (the Places pane asks once; Options ▸ Network), with OSM's
   attribution on the map, a Dalica User-Agent, and the addresses editable for a self-hosted
   server. Built in M13.20 and M13.28.
5. Face-recognition models and their licences (M13). **Decided 2026-10-18 (the maintainer, at
   M13.R): the YuNet detector with the SFace (default) and AuraFace recognisers**, each pinned by
   URL and SHA-256 at its upstream, its licence and what is known of its training data shown before
   any download, never redistributed by Dalica (`faces.md` §3). The public-faces corpus that
   calibrates them was published to `pkkid/dalica-corpus` with the maintainer's go-ahead (M13.17).
6. Password-protected hidden folders. **Decided 2026-10-09: no.**
7. Licence. **Decided 2026-10-09: MIT OR Apache-2.0**, the same as PhotoCraft.
8. Community process (Discord/GitHub Discussions) and release cadence. The repository is public.
9. TLS stack for the few network features (map tiles, model downloads): a pure-Rust provider vs
   accepting `ring`, or delegating downloads to the OS. See `security.md` (M13). **Decided
   2026-10-18 (the maintainer, at M13.R): pure Rust**, `rustls` with the `rustls-rustcrypto`
   provider and the bundled `webpki-roots`, under a reviewed licence exception for its
   CDLA-Permissive-2.0 data licence (`security.md` §8). Built in M13.13.
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
