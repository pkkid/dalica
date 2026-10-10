# Roadmap

Milestones for Dalica, with task ids used in commit subjects (`M1.3: …`). Agents pick the next
unchecked task of the current milestone (or a *Top gaps* row in [`parity.md`](parity.md)), keep
`docs/status.md` current, and tick tasks here in the same commit that lands them.

Status legend: ✅ done · 🚧 in progress · ⬜ not started.

## Where we stand

*Honest assessment, 2026-10-09. Agents: read this before picking work. Update it only with
measured facts, dated.*

Nothing is built yet. The investigation stage is done: the reference organizer's layout, workflows,
data formats, menus and shortcuts are documented privately in `notes/`, and the public specs in
`docs/` describe Dalica. On 2026-10-09 the scope was trimmed (dated features dropped), the kickoff
decisions were recorded (Craft apps are reference only, licence, metadata defaults, keyboard
contexts, two undo histories, OS integration last), and the architecture was specified
(`architecture.md`). The milestones below replace the earlier, much larger ones. Next: M0.

## How milestones work

- **Small and checkable.** A milestone is one feature area, about 3–8 tasks, each task one agent
  session. It ends in something the maintainer can run and look at.
- **Every milestone has an acceptance check:** `cargo xtask accept M<n>` runs that milestone's
  acceptance tests and scripts (`dalica-cli run` / `snapshot` scripts under `tests/acceptance/`)
  and prints what a human should look at. Its *Done when* line says what it proves. Acceptance
  checks of earlier milestones keep running in `cargo xtask ci`, so nothing silently regresses.
- **Review after each milestone.** When a milestone's check passes: write the dated *Where we stand*
  paragraph, review with the maintainer what was learned, adjust the following milestones, and
  only then write the next milestone's task list (Step-4 style: files touched, *done when* per
  task, which tasks can run in parallel).
- **Agents.** M0 and M1 are built by a single agent. From M2 on, the lead may fan tasks out to
  parallel agents when the task list marks them as not sharing files (`AGENTS.md` §7).
- Phases group milestones for orientation only; the order inside a phase may change at a review.

## Overview

| Phase | # | Milestone | One-line goal | Status |
|---|---|---|---|---|
| Foundation | M0 | Skeleton and gates | an empty, fully gated workspace on 3 OSes | ⬜ |
| | M1 | Star a photo, end to end | folder → index → grid → star → sidecar → restart and rebuild | ⬜ |
| | M2 | Agent eyes | agents drive and screenshot the real window | ⬜ |
| Library | M3 | Window shell | every region of the main window, at the right size, in the Craft look | ⬜ |
| | M4 | Light table | the continuous sectioned grid, smooth at 100 k photos | ⬜ |
| | M5 | Library tree | folders list/tree, groups, sorting, click to jump | ⬜ |
| | M6 | Selection and tray | selecting, holding across folders, status line | ⬜ |
| | M7 | Folders and scanning | Folder Manager, first run, watching, offline drives | ⬜ |
| | M8 | Formats and colour | every listed format, RAW previews, ICC | ⬜ |
| | M9 | Photo info | rotate, hide, captions, Properties, dates | ⬜ |
| | M10 | Tags and metadata | Tags pane; captions/tags/dates written into files safely | ⬜ |
| | M11 | Release 0.1 | installable organizer preview on 3 OSes | ⬜ |
| Edit and output | M12 | Edit view shell | open, navigate, zoom, per-photo history | ⬜ |
| | M13 | Basic fixes | crop, straighten, auto fixes, fill light; thumbnails show edits | ⬜ |
| | M14 | Tuning and retouch | the Tuning tab and the Retouch tool | ⬜ |
| | M15 | Save to disk | Save, Undo Save, Revert, Save a Copy, Save As | ⬜ |
| | M16 | Export | export dialog and background export | ⬜ |
| Organize | M17 | Albums | user and automatic albums with lasting order | ⬜ |
| | M18 | Search and filters | instant search, filters, search mode | ⬜ |
| | M19 | File operations | move, copy, rename, delete, new/split folder | ⬜ |
| | M20 | Preferences and shortcuts | Options dialog and the shortcut editor | ⬜ |
| | M21 | MCP and workflows | MCP server; every P0 workflow tested end to end | ⬜ |
| | M22 | Release 0.2 | all P0 rows done, measured and shipped | ⬜ |
| P1 parity | M23 | Import | cards, drives and folders into the library | ⬜ |
| | M24 | Effects I | effect tiles, parameter panels, the 12 effects of tab 3 | ⬜ |
| | M25 | Effects II | the 15 effects of tabs 4 and 5 | ⬜ |
| | M26 | Compare and batch edit | A│B, A│A, before/after, copy/paste and batch edits | ⬜ |
| | M27 | Slideshow and quick preview | full-screen slideshow, Ctrl+Alt preview | ⬜ |
| | M28 | Photo viewer | the standalone viewer | ⬜ |
| | M29 | People I | local face detection, grouping, naming | ⬜ |
| | M30 | People II | suggestions, People pane, names in files | ⬜ |
| | M31 | Places | Places pane, map, geotagging | ⬜ |
| | M32 | Collage | the collage room | ⬜ |
| | M33 | Legacy sidecars | read the previous organizer's data, never write it | ⬜ |
| Finish | M34 | Video | decide, then poster frames / playback / trim, or descope | ⬜ |
| | M35 | Localisation and accessibility | translations, AccessKit, scaling | ⬜ |
| | M36 | OS integration | talking to other apps (last, by decision) | ⬜ |
| | M37 | Release 1.0 | the honest 1.0 | ⬜ |

## Milestones

### M0 — Skeleton and gates

- **Goal:** an empty workspace where every quality gate already bites, so every later commit lands
  on green.
- **In:** Cargo workspace with every crate of `architecture.md` §2 as a stub (crate roots with the
  never-crash attributes, a README each); `rust-toolchain.toml` (1.98), `clippy.toml`,
  `rustfmt.toml`; `LICENSE-MIT`, `LICENSE-APACHE`; `xtask`: `ci`, `layers`, `deps`, `assets`,
  `names`, `parity` (structure only), `accept`; GitHub Actions running `cargo xtask ci` on Linux,
  Windows and macOS; `AGENTS.md` and the private dev log; `dalica` opens an empty window titled
  Dalica on the `chrome` colour and quits cleanly; `dalica-cli --version` and `commands` (`[]`).
- **Out:** any feature, theme beyond one colour, commands, control channel, packaging.
- **Done when:** `cargo xtask ci` is green on a clean clone locally and on all three CI runners;
  `cargo xtask accept M0` proves each gate fails when it should (in a temporary copy: a layer
  violation, an `unwrap()` in non-test code, an unlisted PNG, the reference product's name in a
  file, a `*-sys` dependency, a `std::fs::write` outside `fs`); you run `cargo run -p dalica` and
  see the empty window.

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

### M2 — Agent eyes

- **Goal:** agents can drive and look at the real window, not only the headless one.
- **In:** widget ids on everything M1 draws; the complete headless driver (`click`, `drag`, `key`,
  `type_text`, `step_until`, `state`, `widgets`, `screenshot`); serialisable `UiState`; the control
  channel (`--control`, token, loopback only, limits) with `engine.execute`, `engine.commands`,
  `ui.state` / `ui.set`, `ui.widgets`, `ui.clickWidget`, `ui.pointer`, `ui.key`, `ui.type`,
  `ui.screenshot` (window, headless fallback), `ui.inspect` (perf counters), `ui.settle` /
  `ui.wait`; `dalica-cli snapshot --script`; `DALICA_PROFILE=1`.
- **Out:** MCP (M21), menu methods (need M3's menus), new features.
- **Done when:** `cargo xtask accept M2` runs a control session against a headless instance (auth,
  click a thumbnail, Ctrl+8, screenshot) and checks the star and the PNG fingerprint; the
  protocol's security tests pass (wrong token, oversized line, non-loopback bind refused). You
  start `dalica --control 7990 --demo` and an agent's screenshot matches your screen.

### M3 — Window shell

- **Goal:** the main window has every region of `ui-layout.md` §2 at the specified size, in the
  Craft look.
- **In:** all `Tokens` of `design-system.md`; the widget set (buttons, icon buttons, segmented
  buttons, slider, text field, dropdown, menus, tooltips, splitter); shell icons drawn in code; the
  menu bar from the full menu catalog (unregistered items disabled; native macOS menu); the
  toolbar (controls present, inactive ones disabled); tree pane and splitter (resizable,
  persisted); status line; bottom area (tray box, star and rotate buttons, size slider, pane
  toggles, action buttons with More… overflow); side-pane frame; window state persisted;
  responsiveness rules (§11); F11; `ui.menu.tree` / `ui.menu.invoke`.
- **Out:** real tree and grid behaviour (M4, M5), pane contents, rooms.
- **Done when:** `cargo xtask accept M3`: geometry tests read every region's rect from the widget
  registry at 1400 × 900 and 900 × 600 and match `ui-layout.md` §2 and §11; token contrast tests;
  widget-gallery fingerprints; every menu item of `menus-and-shortcuts.md` §1 is present in order.
  You open the app at both sizes and compare with the docs.

### M4 — Light table

- **Goal:** the continuous, sectioned light table of `ui-layout.md` §5, smooth at 100 k photos.
- **In:** sections in tree order; section header block (icon, title, date and place, buttons with
  inactive ones disabled, description line); justified square-cell grid; badges; thumbnail caption
  modes; hidden/small-picture drawing rules; sticky headers; section scrollbar (⇑ ▲ ▼ ⇓); size
  slider, Ctrl+wheel, Ctrl+1 / Ctrl+2; hover tooltip; empty states; layout cached per revision;
  texture eviction.
- **Out:** selection beyond M1's click (M6); section-header actions (their own milestones); inline
  description editing (M9).
- **Done when:** `cargo xtask accept M4`: a headless benchmark scrolls a synthetic 100 k-photo
  library at ≤ 8 ms/frame p95 UI time (recorded in `status.md` → Metrics) and cells visited per
  frame stay flat from 10 k to 100 k; layout tests for column justification and sticky-header
  push-up; section-jump tests. You scroll the demo library.

### M5 — Library tree

- **Goal:** the left pane of `ui-layout.md` §4.
- **In:** groups (Folders, Other, Hidden Folders; Albums and People appear empty until their
  milestones); flat and tree modes; year dividers; folder sorting and reverse; the view-options
  popup; click = scroll that section's header to the top (animated) and select the folder as a
  unit; collapsing a group hides its sections; ↑/↓ and ←/→; middle elision; counts.
- **Out:** album and people rows (M17, M29), drop targets (M19), Hide Folder (M9).
- **Done when:** `cargo xtask accept M5`: tree rows and order match a fixture library in both
  modes and every sort; a row click leaves its header at the top (scroll offset) and selects the
  folder; collapsing Folders removes its sections; a 5 k-folder library stays within the frame
  budget.

### M6 — Selection and the Photo Tray

- **Goal:** the selection model and the tray of `ui-layout.md` §5.4 and §6.
- **In:** click, Ctrl-click, Shift range, rubber band, scoped to one section; keyboard navigation
  (arrows, Home/End, Ctrl+Home/End, Ctrl+↑/↓, PgUp/PgDn); Ctrl+A / D / I; Select Starred; the tray
  (live mirror, folder token, mini thumbnails that shrink with count, tray selection); Hold
  (Ctrl+H) and the held badge; Clear with confirmation; every status-line state; bottom star and
  rotate buttons act on the tray; "Select some photos first".
- **Out:** Add to Album ▾ (M17), dragging into or out of the tray (M19), tray context-menu items
  of later milestones (shown disabled).
- **Done when:** `cargo xtask accept M6`: select 2 in folder A, Hold, select 3 in folder B → the
  tray has 5 → star all 5 in one undo step → undo restores all 5; status-line strings match each
  case of §6.1; keyboard navigation stops at section edges.

### M7 — Folders and scanning

- **Goal:** the library follows the user's folders by itself (`data-model.md` §7,
  `organizing.md` §1–2).
- **In:** Folder Manager (roots and volumes loaded lazily, Scan Always / Scan Once / Remove,
  inheritance, watched list, confirmations); first-run panel; `notify` watching + periodic
  verification; external creates, deletes, renames and modifications; folders moved outside Dalica
  keep their data; offline volumes (dimmed, cached thumbnails, queued edits written on return);
  skip rules; the disk thumbnail cache (2 GiB, pruning); the activity indicator with its job list
  and Cancel; scan progress on the status line.
- **Out:** new formats (M8), legacy sidecars (M33), face finding (M29).
- **Done when:** `cargo xtask accept M7`: temp-dir scenarios (add, remove, rename while running;
  the verification pass finds everything; a volume "unplugged" by renaming its root shows offline,
  a star made meanwhile is written when it returns); benchmark: first thumbnails of a 10 k-JPEG
  folder ≤ 1 s, full scan time recorded. You run first-run on a fresh library directory and watch
  thumbnails fill in.

### M8 — Formats and colour

- **Goal:** every format of `formats.md` §1 shows correctly and safely.
- **In:** TIFF, WebP, GIF, BMP, JPEG XL, PSD composite, RAW embedded previews (our own IFD walker),
  CMYK and 16-bit sources, ICC through `moxcms`, decode limits on every path, "!" tiles with
  reasons, HEIF behind the `heif` feature, View → Use Color Management, `cargo xtask corpus` with
  floors, a fuzz target per parser.
- **Out:** writing metadata (M10), video (M34), AVIF (no decoder available).
- **Done when:** `cargo xtask accept M8`: decode tests per format from `scenes` at several depths
  and profiles; orientation 1–8 per container; malformed-input property tests never panic; corpus
  floors hold; a mixed-format folder snapshot matches its fingerprint.

### M9 — Photo info

- **Goal:** the light organizing a user does on single photos and folders, stored in sidecars.
- **In:** rotate CW/CCW from the library (recipe op, rotated thumbnails; the first `edit`
  operations: rotate and flip); hide/unhide photos and folders, View → Hidden Pictures, the Hidden
  Folders group; captions (inline editing in the library, Thumbnail Caption modes); folder
  description editing in the header; Folder Properties (date, place, description); the Properties
  pane (read-only metadata, Show All Metadata); Adjust Date and Time.
- **Out:** writing into files (M10), the edit view (M12), renaming folders (M19).
- **Done when:** `cargo xtask accept M9`: the command contract for every new command; rotated
  thumbnail fingerprints; hide, captions and dates survive a rebuild. You rotate, hide and caption
  photos in the app.

### M10 — Tags and metadata write-back

- **Goal:** tags in the UI, and captions, tags and dates written into the photo files without ever
  harming them (`xmp-interop.md`).
- **In:** tag commands and the Tags pane (add, partial counts, remove, quick tags and their
  dialog, Ctrl+Alt+1…0, Rename Tag); `meta` write-back for JPEG, PNG, WebP and TIFF; the metadata
  write queue with re-read, merge and verify; the conflict rules of `xmp-interop.md` §5; importing
  file metadata on first scan; `.xmp` sidecars for RAW (option); the metadata corpus.
- **Out:** face regions (M30), the GPS user interface (M31).
- **Done when:** `cargo xtask accept M10`: `xmp-interop.md` §8 tests 1–4 and 6–10 pass; files
  written by other applications in the corpus keep every non-owned byte after a Dalica write. You
  tag a photo in Dalica and see the tag in another photo application.

### M11 — Release 0.1 (organizer preview)

- **Goal:** something installable to try on real folders.
- **In:** the release workflow (draft releases with checksums); Linux AppImage, Flatpak and deb/rpm, Windows
  MSI/zip, macOS DMG (unsigned where there are no credentials); About window with credits; the
  user-facing `whats-new.md`; README status banner; the application id (reserved decision 10).
- **Out:** signing without credentials, update checks (deferred).
- **Done when:** `cargo xtask accept M11`: a CI dry run builds the artifacts on all three OSes and
  the checksums match. You install it and browse your own photos.

### M12 — Edit view shell

- **Goal:** open a photo and move around in it (`editing.md` §1–3, §5.1–5.2).
- **Prerequisite:** decide the coordinate frame (`data-model.md` §3.3).
- **In:** layout (top bar, filmstrip, edit panel with tab placeholders, photo area, caption bar,
  Histogram & Camera Info); entering and leaving (double-click, Enter, Ctrl+3, Esc, Ctrl+1);
  previous/next; zoom and pan maths, Fit / 1:1, background refinement; the per-photo history
  commands with labelled Undo/Redo buttons; star, rotate and flip in the edit view as history steps.
- **Out:** tools and effects (M13+), compare modes (M26), video (M34).
- **Done when:** `cargo xtask accept M12`: zoom maths tests (ends and midpoint = 100 %); navigation
  tests; a photo's history survives a restart; opening a 24 MP JPEG renders its screen preview in
  ≤ 150 ms (recorded).

### M13 — Basic fixes

- **Goal:** the Basic Fixes tab, and edits shown everywhere (`editing.md` §6).
- **In:** the `edit` renderer with pipeline order and per-operation versions; thumbnails show
  edits (recipe in cache keys); Crop (ratios, custom ratios, Manual, rotate, preview; suggestions
  from thirds/saliency); Straighten; Auto Fix, Auto Contrast, Auto Color; the Fill Light slider.
- **Out:** face-based crop suggestions (M29), Retouch (M14).
- **Done when:** `cargo xtask accept M13`: fingerprints per operation; resolution independence (two
  sizes agree after downscale); thumbnail render = edit-view render; unknown operations survive.

### M14 — Tuning and retouch

- **In:** the Tuning tab as one live-updated operation (one undo step per tab session), Auto
  lighting and Auto color, the Neutral Color Picker; the Retouch tool with in-tool patch undo.
- **Done when:** `cargo xtask accept M14`: a slider drag updates the screen preview in ≤ 16 ms
  draft (recorded); a whole drag is one history step; fingerprints per control.

### M15 — Save to disk

- **Goal:** the only path that changes pixels, with the original always recoverable
  (`data-model.md` §6).
- **In:** Save (first-time dialog, originals backup, quality matching, metadata carried over),
  Undo Save, Revert, Save a Copy, Save As; the folder header's Save button and "n edited";
  lossless JPEG rotation (or the EXIF flag fallback); non-writable formats → JPEG beside.
- **Done when:** `cargo xtask accept M15`: Save → Undo Save → Save → Revert returns the exact
  original bytes (`data-model.md` §10.9); fault injection during Save never loses the original;
  every format of `formats.md` §1 saves as specified.

### M16 — Export

- **In:** the Export dialog (`creating.md` §2), the background export job with progress and
  Cancel, resize, quality presets, watermark, metadata copy and optional GPS removal, collision
  names, the Exported project group, adding an unwatched export folder.
- **Done when:** `cargo xtask accept M16`: exported files are decoded and checked (size, quality
  bounds, metadata, GPS removed when asked); Cancel leaves only complete files; a 200-photo export
  of a mixed tray succeeds.

### M17 — Albums

- **In:** user albums (New Album, properties, add via tray, context menu or drop on an album row,
  remove, manual order with position keys, Sort Album By, cover, delete); Starred Photos and
  Recently Updated; the Albums group and album sections; the Album menu variant.
- **Done when:** `cargo xtask accept M17`: `data-model.md` §10.12 (cross-folder order survives a
  rebuild); undo for every album command.

### M18 — Search and filters

- **In:** the search grammar, instant search on a worker with debounce, autocomplete, the filter
  buttons and age slider, search mode (banner, Show All, scroll restore), the temporary results
  album, Save Search as Album, Find Duplicates (identical files).
- **Out:** `color:` search and `person:` before People (decided at this milestone's review).
- **Done when:** `cargo xtask accept M18`: grammar tests; search of 100 k photos answers within its
  budget (recorded); the banner text and counts match the spec.

### M19 — File operations

- **In:** move (drag onto a tree folder, Cut/Paste), copy, the collision dialog, Move to New
  Folder, Split Folder Here, move/rename/delete folder, Rename and batch rename, delete to the
  trash, the drag pill; sidecar entries and originals backups travel with their photos; undo where
  possible; "don't ask again" settings.
- **Out:** dragging out to other applications (M36).
- **Done when:** `cargo xtask accept M19`: `data-model.md` §10.2, §10.3 and §10.7; undo tests per
  operation; a move across two file systems copies, verifies, then deletes.

### M20 — Preferences and shortcuts

- **In:** the Options dialog (tabs; OK / Cancel / Apply rules); General, File Types and Export tabs;
  UI scale and font size; the shortcut editor (`menus-and-shortcuts.md` §2.0) and Help → Keyboard
  Shortcuts.
- **Done when:** `cargo xtask accept M20`: settings round-trip with unknown keys kept; keymap
  conflict tests; rebinding Star to F8 through `app.setShortcut` and pressing F8 headless stars
  the photo; Reset All restores every default.

### M21 — MCP and workflows

- **In:** `dalica-cli mcp` (headless and connect modes), one tool per command plus helpers, roots
  and permissions (`mcp.md`); workflow acceptance tests for every P0 flow.
- **Done when:** `cargo xtask accept M21`: every P0 workflow passes over MCP in CI; the behaviour
  pass rate is recorded in *Where we stand*.

### M22 — Release 0.2 (P0 complete)

- **Done when:** every P0 parity row is ✅ with evidence; P0 workflows pass 100 %; performance
  budgets for P0 scenarios are enforced; an honest *Where we stand*; a 0.2 release.

### M23 – M33: P1 parity (outline; detailed at the reviews)

| # | In | Done when (sketch) |
|---|---|---|
| M23 Import | rooms/tab strip; sources (cards, removable drives, folders); time grouping; exclude duplicates; destination and folder naming; copy-verify; optional removal from the card | import of a simulated card into dated folders; duplicates skipped; no file overwritten |
| M24 Effects I | effect tiles, parameter panels, crosshair control, Fade; the 12 effects of tab 3 | a fingerprint and an invariant per effect |
| M25 Effects II | the 15 effects of tabs 4–5, canvas-enlarging effects, Soften's paint mask | as M24 |
| M26 Compare and batch edit | A│B, A│A, before/after hold, Copy/Paste All Edits, Batch Edit ▸, Undo All Edits | batch of 100 photos is one undo step |
| M27 Slideshow and quick preview | slideshow, control bar, transitions, captions, music; Ctrl+Alt preview | transitions never wait on disk (prefetch test) |
| M28 Photo viewer | `dalica --view`, filmstrip, controls, settings | open, navigate, star from the viewer |
| M29 People I | consented model download (decisions 5 and 9), detection, embeddings, clustering, Unnamed, naming, ignore, person albums | a public-domain face set groups as expected (floors) |
| M30 People II | suggestions, People pane, manual faces, merge/rename/delete, MWG regions | MWG round-trip tests (`xmp-interop.md` §8.5) |
| M31 Places | Places pane, opt-in map tiles (decision 4), geotag by drag and search, GPS write, Geotag menu | GPS round-trip; no network without the user's action |
| M32 Collage | the collage room, styles, canvas, render + recipe, Edit Collage | deterministic layouts per seed; render size |
| M33 Legacy sidecars | read-only reader with provenance header, first-run notice, conversion on first write, legacy originals for Revert | `data-model.md` §10.11 |

### M34 – M37: Finish (outline)

| # | In | Done when (sketch) |
|---|---|---|
| M34 Video | reserved decision 2 first; then poster frames, playback, trim, frame save — or descope to tiles | decided and recorded; tests per chosen scope |
| M35 Localisation and accessibility | translation catalogs, AccessKit labels, screen-reader and keyboard-only passes, scaling | coverage per language reported; AccessKit tree tests |
| M36 OS integration | drag out to other apps, clipboard files, Open With ▸, external editor, Locate on Disk with selection, default-viewer registration, MTP/PTP cameras | per-OS manual checklist plus what can be tested per OS in CI |
| M37 Release 1.0 | signed packages, full honest assessment, release notes | the release checklist (`releasing.md`) |

## Reserved decisions (the maintainer's, not the agents')

Record the decision here with a date when made.

1. Reuse model. **Decided 2026-10-09: reference only.** LightCraft and PhotoCraft are prior art for
   technology, look and methodology; no Craft code is depended on or vendored. crates.io
   libraries stay allowed after the dependency check (`architecture.md` §9).
2. Video: pure-Rust decoding vs optional external process (M34).
3. Metadata write defaults. **Decided 2026-10-09:** captions, tags, places and date changes are
   written into the photo files' metadata by default (pixels never change without Save); stars as
   XMP ratings off.
4. Map tiles provider and whether maps are on by default (M31).
5. Face-recognition models and their licences (M29).
6. Password-protected hidden folders. **Decided 2026-10-09: no.**
7. Licence. **Decided 2026-10-09: MIT OR Apache-2.0**, the same as PhotoCraft.
8. Community process (Discord/GitHub Discussions) and release cadence. The repository is public.
9. TLS stack for the few network features (map tiles, model downloads): a pure-Rust provider vs
   accepting `ring`, or delegating downloads to the OS. See `security.md` (M29).
10. Application id (reverse-DNS, used by packaging, desktop files and the settings path) (M11).
11. Coordinate frame for normalised coordinates (`data-model.md` §3.3) (M12).
