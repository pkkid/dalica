# Architecture

Dalica is an **engine-first Cargo workspace** of small, separately tested crates with enforced
layering. The egui front end is two crates at the top; nothing below them knows a UI exists. Every
user action is a **command** with a stable id, dispatched through one entry point
(`Session::execute`) by the UI, the keyboard, the CLI, the control channel and the MCP server, so
tests and agents can do anything a user can do without a window.

Read with: [`data-model.md`](data-model.md) (what is stored where), [`design-system.md`](design-system.md)
(the look), [`ui-layout.md`](ui-layout.md) (the window), [`development.md`](development.md) (build,
test, tooling), [`control-protocol.md`](control-protocol.md) and [`mcp.md`](mcp.md) (automation).

> Status: specified, not built. Section numbers are referenced from other docs; keep them stable
> (add new sections at the end).

## 1. Stack (pinned)

Versions are the latest stable releases on 2026-10-09 (maintainer decision: always start from the
latest). They are pinned in `[workspace.dependencies]`; bumping one is its own commit.

| Concern | Choice | Why |
|---|---|---|
| Language | Rust, edition 2024, `rust-version = "1.98"`, `resolver = "3"`; toolchain pinned in `rust-toolchain.toml` | latest stable; a pinned toolchain keeps `clippy -D warnings` from breaking on its own |
| UI | `egui` / `eframe` **0.36.2** (`wgpu_no_default_features, accesskit, wayland, x11`; no `default_fonts`, whose fonts carry the Ubuntu Font Licence: Inter is installed by `ui-kit`), `egui_extras` 0.36.2 | immediate mode is easy to drive headlessly; same toolkit as the Craft apps, whose look we match |
| GPU | `wgpu` 30.0.1 (Metal / Vulkan / DX12, no GL) | eframe's renderer (the window uses DX12 alone on Windows, `apps/dalica/src/gpu.rs`; `WGPU_BACKEND` overrides); later, optional compute shaders for slow effects (`development.md` §14) |
| Native dialogs | `rfd` 0.17.2, always off the UI thread, behind a `Picker` trait | native look; the trait lets tests and agents script dialogs |
| macOS menu bar | `muda` 0.21.2, generated from the menu catalog | one menu model, three platforms |
| Decoding / encoding | `image` 0.25.10 (png, tiff, webp, gif, bmp; its own JPEG encoder, since `jpeg-encoder` carries the IJG licence), `zune-jpeg` (JPEG decode; 0.5 has no DCT scaling, so thumbnails decode in full and resize), `png` 0.18, `image-webp` 0.2, `jxl-oxide` 0.12; `heic-rs` 0.1 behind the off-by-default `heif` feature | pure Rust, permissive licences (`formats.md`) |
| Colour | `moxcms` 0.9 | pure-Rust ICC transforms; never assume sRGB input |
| Resampling | `fast_image_resize` 6 | fast, SIMD, pure Rust; thumbnails dominate CPU time |
| Metadata | our own `meta` crate (segment surgery, `xmp-interop.md`) + `quick-xml` 0.42 for XMP parsing + `md5` 0.8 for the IPTC digest (one safe file, no dependencies) | no crate preserves foreign metadata byte for byte; we must own that guarantee |
| File watching | `notify` 8 (its `inotify-sys` / `fsevent-sys` bindings allowed at M3.R) + a pure-Rust polling fallback + periodic verification scans | events are hints, verification is the truth |
| File safety | `std::fs::File::try_lock` (lock file; stable since Rust 1.89, so no `fs4`), `trash` 5.2 (OS trash), `tempfile` 3 (tests) | single-process lock; deletes go to the trash |
| Locations | our own lookup in `scan::volumes` (HOME / USERPROFILE and the XDG user-dirs file; M5.4), not `directories`, which would bring `dirs-sys` and MPL-2.0 `option-ext` | per-OS standard places without new exceptions; Windows folder redirection is not followed yet |
| Folder handles | `rustix` 1 (`fs`, safe API only; Unix) | writes relative to a held folder handle (`security.md` §5.6; M5.3) |
| Test-file encoders (`scenes` only) | `tiff`, `fdeflate`, `zune-jpegxl` 0.5 (no threads, deterministic) | our own TIFF/PSD/ICC writers need them; never in the product's decode path |
| Parallelism | `rayon` 1.12 for pixel tiles; our own prioritised job pool for previews; `crossbeam-channel` 0.5 | pixel work scales; previews need priorities and cancellation rayon doesn't give |
| Serialization | `serde` 1, `serde_json` 1 (sidecars: pretty, sorted keys, LF) | human-readable, diffable, merge-friendly |
| Errors | `thiserror` 2 per crate; `Result` everywhere (never-crash) | typed errors that commands turn into JSON |
| Logging | `log` 0.4 + a small rotating file logger (`<config>/logs/dalica.log`) | no personal data in logs (`security.md`) |
| MCP | `rmcp` 3.5 (stdio) | the reference MCP SDK |
| Tests | built-in harness, `proptest` 1.11, `insta` 1.49 (JSON snapshots only), our headless driver | property tests for parsers and ops; reviewable snapshots of command output |
| Benches | `criterion` 0.8 + `cargo xtask bench` / `perf` | before/after numbers on hot paths |

**Product rules:** pure Rust (no C/C++ build dependencies in the product; `cargo xtask deps` audits
`Cargo.lock` for `*-sys`, `ring`, `openssl`), no webview/JS/Tauri, no network access except
explicit user actions (map tiles, model downloads), no telemetry. Dependencies may contain
`unsafe` internally (wgpu, trash, fast_image_resize); our crates may not (§8).

## 2. Crates

Package names are `dalica-<dir>`. Each crate has one job, a `README.md` (purpose, public API, how
to test it alone), and lives in exactly one layer.

```text
crates/
  L0  core      ids (PhotoId, FolderId, AlbumId, PersonId), errors, normalised geometry, dates,
                path and name utils (collision names "-001"), position keys (data-model §3.2), Clock trait
  L0  fs        every write to the user's disk: atomic replace + fsync + verify, read-before-write
                guard, library lock, move/copy with verify, OS trash, bounded walks, UI-thread guard,
                fault-injection points (feature `faults`)
  L1  image     sniff, decode (scaled), encode, orientation, ICC → sRGB, decode limits, embedded
                thumbnails and RAW previews (our own bounded TIFF/IFD walker)
  L1  meta      EXIF / IPTC / XMP read; write-back of owned fields by segment surgery (merge, never
                replace); MWG face regions; `.xmp` sidecars for RAW
  L1  sidecar   `.dalica.json` parse / serialise (unknown keys preserved), legacy INI reader
                (read-only, provenance header), conversion of legacy data to Dalica's model
  L1  edit      the recipe: operation syntax parse/print (unknown ops preserved), pipeline order,
                CPU renderer for every tool and effect (the oracle), auto-fix analysis
  L1  scenes    procedural test and demo photos (gradients, faces-like blobs, horizons, text-free
                patterns) at any size, depth and orientation, with a minimal EXIF block; no media files
  L1  fetch     consented, resumable, hash-pinned downloads (M13)
  L1  faces     detection, alignment, embeddings, clustering on decoded RGB buffers (M13, `faces.md`)
  L2  library   the in-memory model (folders, photos, albums, people, tags, captions, stars, hidden,
                recipes, places), `Op`s with inverses, revisions, queries and the search grammar.
                Pure: no I/O, no threads
  L2  thumbs    prioritised de-duplicating job pool, in-memory LRU, on-disk thumbnail cache
  L3  store     persistence of the model: index journal + snapshot, model ↔ sidecar mapping,
                debounced sidecar writer, metadata write queue, rebuild-from-folders
  L3  scan      watched-folder rules, walks, change detection, `notify` events, offline volumes,
                card/folder import; turns what it finds into `Op`s
  L3  create    export (resize, quality, watermark, metadata copy), collage layout + render,
                slideshow sequencing
  L4  engine    `Session`: library + store + jobs + settings + undo stacks; the command registry
                (`cmd/<area>.rs`); keyboard contexts and the keymap; control-protocol types; demo
                library setup
  L5  ui-kit    generic look: `Tokens`, widgets, icons drawn in code, widget registry, CPU
                rasterizer for screenshots. Knows nothing about photos or Dalica's engine
  L5  ui        the app: `UiState` (serialisable), panels, menus + menu catalog, shortcut dispatch,
                `UI_COMMANDS`, the headless driver, control-method handlers
  L5  mcp       MCP server over the command registry (headless or connected)
apps/
  dalica        desktop app: eframe window, control-server transport, native menu, rfd picker
  dalica-cli    headless: commands, run, inspect, render, snapshot, mcp
tests/acceptance  one test file per milestone (`m<n>.rs`) + its manual checks (`m<n>.md`); run by `cargo xtask accept M<n>`
xtask/          ci | layers | deps | assets | names | parity | accept | corpus | bench | perf | scorecard | stats
```

### Layering (enforced by `cargo xtask layers`)

```text
apps   dalica (window, control transport)          dalica-cli (run, snapshot, mcp)
         │                                            │
L5     ui ─────► ui-kit                             mcp
         │        (egui, core only)                   │
         └────────────────────┬───────────────────────┘
L4                          engine   Session · commands · keymap · undo · jobs · settings
                              │
L3              store ────────┼──────── scan ──────── create
                  │           │           │              │
L2             library      thumbs        │              │
               (pure)         │           │              │
L1      image · meta · sidecar · edit · scenes · fetch · faces
                              │
L0                     core · fs (all disk writes)

Arrows point down only: a crate may use crates in lower bands (exact pairs in the table below).
```

| Layer | Crates | May depend on (workspace) |
|---|---|---|
| L0 | `core`, `fs` | `fs → core`; nothing else |
| L1 | `image`, `meta`, `sidecar`, `edit`, `scenes`, `fetch`, `faces` | L0; sibling exceptions: `edit → image`, `meta → image` (reuses its bounded TIFF/IFD reader) (`faces` takes decoded buffers and depends on `core` only) |
| L2 | `library`, `thumbs` | `library → core` only (it stays pure); `thumbs → image, edit, fs, core` |
| L3 | `store`, `scan`, `create` | L0–L2; no L3 siblings |
| L4 | `engine` | L0–L3 |
| L5 | `ui-kit` | `core` only (+ egui) |
| L5 | `ui` | `engine`, `ui-kit`, lower layers |
| L5 | `mcp` | `engine`, lower layers |
| apps | `dalica`, `dalica-cli` | anything |
| test | `tests/acceptance` (`dalica-acceptance`, `publish = false`) | anything; nothing may depend on it |

Rules the checker enforces:
1. A normal dependency must point to a lower layer (or a listed exception). A new crate must be
   added to `xtask/src/layers.rs` and to this table in the same commit.
2. **Dev-dependencies** may point anywhere lower, plus `scenes` from any crate (it depends only on
   `core`, so no cycles).
3. Only L5 and apps may depend on `egui`, `eframe`, `winit`, `wgpu`, `rfd`, `muda`.
4. Only `fs` may call `std::fs` write functions (`write`, `rename`, `remove_*`, `copy`,
   `create_dir*`, `OpenOptions` with write); `xtask layers` greps for them. Everything else writes
   through `fs`, so atomicity, verification and fault injection have one home. That includes the
   app's log files, written with `fs::append` (M1.20). Tooling (`xtask`, `tests/acceptance`) and
   test code are not product code and are not checked.
5. L0–L4 stay free of platform UI code so they can later build for `wasm32` (not before 1.0).

## 3. Commands: the one seam

```rust
pub struct CommandSpec {
    pub id: &'static str,            // "photo.star.toggle", "folder.rename", "edit.crop.apply"
    pub label: &'static str,         // English UI label, also the i18n key
    pub menu: &'static str,          // "Picture/Star"; "" if not in a menu
    pub keys: &'static [(Context, &'static str)], // default bindings, e.g. [(Library, "Ctrl+1")];
                                     // `Ctrl` = primary modifier (Cmd on macOS); user overrides
                                     // live in settings (menus-and-shortcuts.md §2.0)
    pub params: &'static str,        // agent-readable doc: {"ids":[PhotoId], "on":bool=toggle}
    pub effect: Effect,              // Query | Library | Files (writes to photo folders; needs confirm
                                     // over automation when destructive) | External (outside Dalica)
    pub history: History,            // Library (one library undo step) | Photo (a step of the photo's
                                     // own history) | None (queries, view changes)
    pub enabled: fn(&Session, &Value) -> Result<(), Disabled>, // Err carries the reason shown on the status line
    pub run: fn(&mut Session, &Value) -> Result<Value, CmdError>,
}
```

- Registered per area in `crates/engine/src/cmd/<area>.rs` (`pub fn specs() -> Vec<CommandSpec>`),
  collected in `cmd/mod.rs`. New areas add a new module rather than growing a shared file.
- `Session::execute(id, &params)` is the **only** way library state changes. It checks `enabled`,
  wraps `run` in `catch_unwind`, records exactly one undo step on the history the spec names
  (drags and typing sessions coalesce through a `coalesce` key), and returns JSON. Query commands
  change nothing.
- **Parameters** are JSON validated by serde into a typed struct per command; a wrong type, a
  missing field or an unknown id is `CmdError::BadParams`, never a panic.
- **Surfaces.** All five call the same function:

  | Surface | How it reaches `Session::execute` | First milestone |
  |---|---|---|
  | UI (buttons, menus, context menus) | `App::run(id, params)` (UI-only ids handled in `ui`, the rest forwarded) | M1 |
  | Keyboard | key event + active context → keymap → `App::run` | M1 |
  | CLI | `dalica-cli run --library DIR script.jsonl` / `--cmd id --params '{…}'` | M1 |
  | Control channel | `engine.execute` over loopback JSON lines (`control-protocol.md`) | M2 |
  | MCP | one tool per command plus helpers (`mcp.md`) | M10 |

- **UI-only commands** (pane toggles, zoom, view switches, window state) live in `UI_COMMANDS` in
  `crates/ui/src/menus.rs`, declare keys the same way, and are listed by `engine.commands` too, so
  agents see one catalogue.
- **Menu catalog** (`crates/ui/src/menu_catalog.rs`): the complete target menu tree from
  [`menus-and-shortcuts.md`](menus-and-shortcuts.md). Items whose id has a registered command are
  live; the rest render disabled. Giving a new command the catalog id lights up its menu item.
- **Keymap** (`crates/engine/src/keymap.rs`, pure): contexts, defaults from the specs, user
  overrides from settings, resolution order Text input → view context → Global, and conflict
  detection (`menus-and-shortcuts.md` §2.0). It lives in the engine so the CLI and tests can check
  and change bindings without a window; `ui` only feeds it key events and the active context.
- **Command test contract** (every command, enforced by `cargo xtask parity` listing commands
  without tests): behaviour; undo and redo restore exactly; `enabled` false cases; hostile params
  return `Err` (`panic_hunt` fuzzes all of them); for `Files` commands, the files on disk after the
  command and after undo.
- `cargo xtask parity` cross-checks `docs/parity.md` against the registry and the menu catalog.

## 4. State, undo and persistence

- `Library` (crate `library`) is an in-memory model mutated **only** by `Op`s. `library.apply(op)`
  returns the inverse op and bumps the library **revision**; derived data (tree rows, light-table
  layout, counts, search results) is cached per revision.
- **Source of truth for user data is on disk next to the photos** (folder sidecars, plus metadata
  written into the files). The index journal/snapshot in the library directory is a cache that can
  be rebuilt from the folders at any time (`data-model.md` §4).
- **Write path** (crate `store`, driven by the engine after each command):

  ```text
  command ─► Ops ─► library.apply ─► inverse ops ─► undo stack (library or photo history)
                         │
                         ├─► index journal (append, checksummed; background snapshot + compaction)
                         ├─► dirty folders ─► sidecar writer (debounced 1 s per folder, atomic via fs)
                         └─► dirty photos  ─► metadata write queue (re-read, merge, atomic, verify)
  ```

  Writers run on worker threads. `Session::flush()` blocks until every pending write has landed;
  the CLI calls it before exiting and tests call it before asserting on disk, so tests never sleep.
- File operations (move, rename, delete-to-trash, save, save a copy) are commands whose ops record
  their inverse where possible (move back, rename back; restoring from the trash is best effort
  and never promised in the UI).
- **Two undo histories, chosen by the view** (maintainer decision, 2026-10-09):
  - **Library view:** Edit → Undo / Redo (Ctrl+Z / Ctrl+Y) walk the **library undo stack**: every
    library-level op (stars, captions, tags, hide, rotations made in the library, moves, renames,
    album changes, batch edits and Paste All Edits applied from the library). In memory, per
    session, capped at 1,000 entries.
  - **Edit view:** the same keys, the Edit-menu items and the edit panel's Undo/Redo buttons walk
    the **open photo's own history**, persisted in its folder sidecar (`"edits"` / `"redo"`, so it
    works across restarts). Everything done in the edit view (tools, effects, rotations, flips)
    goes into that photo's history and not onto the library stack.
  - Both are commands: `edit.undo` / `edit.redo` resolve to `library.undo` / `library.redo` or
    `photo.history.undo` / `photo.history.redo {id}` by context, so agents can call either directly.
    `edit.undo {id}` / `edit.redo {id}` walk that photo's history; without `id`, the library stack.
  - **The photo's history** (`crates/engine/src/history.rs`, `cmd/history.rs`): a step
    (`photo.history.apply {id, op}`, `History::Photo`) appends to the recipe by `edit`'s rules and
    clears the redo list; undo moves the last entry to the end of `"redo"`, redo moves it back.
    `photo.history.state {id}` answers the recipe, the redo list and the button labels ("Undo
    Crop", "Redo Tuning"; "Undo" / "Redo" when there is nothing).
  - **Merged steps.** A step that merged into the entry before it (a second rotation turning
    `rotate(90)` into `rotate(180)`, a flip cancelling the flip before it, a Tuning session updating
    the last `tuning` entry) undoes to that entry's earlier value, and redo merges it again; the
    redo list holds the operation the step applied (`rotate(90)`). The session remembers, per
    photo, how each of its steps changed the end of the recipe; a step the memory no longer
    matches (the recipe changed by a library step, another application, or a restart) undoes as
    its plain last entry, so after a restart `rotate(180)` undoes in one step. Steps sharing a
    `coalesce` key that each replace the entry the one before left are one step (a Tuning
    session, M7.18).
  - The session's recipe hook (`Session::take_recipe_changes`) lists every photo whose recipe
    changed, by either history, a scan or the watcher, for the thumbnails (M7.9).
  - The menu label always names the step it will undo ("Undo Star", "Undo Crop"), so the user can
    see which history is active.
- **Settings** (`settings.json`, engine): a typed struct; unknown keys preserved; damaged file kept
  aside as `.corrupt-<time>` and defaults used.
- **Library location:** the platform data directory by default; `--library <dir>` or
  `DALICA_HOME` override it (tests and agents always pass one); `--demo` opens a persistent library
  of `scenes` photos with deterministic ids (built once, reused); `--demo-fresh` builds a throwaway
  copy in a private temporary folder, deleted on exit.
- A library lock file (`fs`) prevents two Dalica processes from opening the same library.
- **Time** comes from a `Clock` in the `Session` (debounce, "Recently Updated", the age filter,
  animations), so tests run on virtual time.

## 5. Threading and responsiveness

The thread that owns the `Session` (the UI thread in the app, the main thread in the CLI) never
decodes images, touches the file system, waits on a lock held by a worker, or blocks on a dialog.

- **`fs` UI-thread guard:** the owning thread is marked at startup; in debug and test builds every
  `fs` call made from it returns `Err(OnUiThread)`, so a violation fails tests instead of stuttering
  in production.
- **The session's I/O worker** (built in M1.13): once `Session::open` (called off the UI thread)
  has taken the lock and loaded the index, every disk write the session makes goes to one worker
  thread: journal appends, debounced sidecar writes (fed with folder states, not the library),
  snapshots, and the library directory's own files (`folders.json`, `settings.json`). Scans run on
  threads of their own. The owner applies finished background work with `Session::poll()` (each UI
  frame) or `Session::wait_idle()` (CLI, tests); `Session::flush()` blocks until every pending write
  has landed.
- **Jobs:** long work (scan, import, export, save, collage, batch edit, metadata writes) runs as an
  engine job on a worker with an `AtomicBool` cancel and progress reports. A job gets owned inputs
  when it is submitted and returns `Op`s or files; the owner applies the result as one undo step.
- **File jobs** (engine `filejobs.rs`, M8.3; prior art LightCraft's export batch): every job that
  writes photo files (Save, Save a Copy, Save As, Export) runs through one runner. A command builds
  a `FileJob` (kind, label, owned items that each name their file, the work for one item) and
  `Session::start_file_job` lists it as a job and runs the items on up to `workers` threads, each
  item's panic its own failure; Cancel is checked before each item (the running ones finish, no
  other starts). The work writes through `fs` and says what it did (`FileChange`: a file replaced
  in place, a new file, a new folder); the worker reads those files back as a scan would (size,
  time, pixel size, date). `poll()` applies the results once and in item order: a replaced file
  keeps its photo's id and takes the new file's facts (so the watcher's rescan finds Dalica's own
  write unchanged and imports nothing), a new file in a library folder is added as a photo, folders
  outside the library are scanned when the job ends (added as Scan Always when no watched folder
  covers them); then the caller's function turns the item into ops, applied as changes from the
  files with no undo step. Each finished job leaves a `FileJobEvent` (message, failures with file
  and reason, the folder to show) for the UI's notices.
- **Job pool** (crate `thumbs`; prior art LightCraft `preview/src/pool.rs`): prioritised, per-slot
  de-duplicating; workers = cores − 1 clamped 2..8. Priorities (high → low): edit-view main image,
  hovered photo, visible thumbnails, filmstrip, effect-tile previews, prefetch (next/previous
  photo), background thumbnail generation.
- **Renderer** (UI side): turns finished jobs into egui textures, drops stale results, evicts
  textures beyond ±1 screen (cap ~600 thumbnails).
- **Scanner** thread(s): directory walks and change detection, feeding batched `Op`s to the owner
  (applied a few hundred per frame so the light table fills progressively).
- **File-system answers** used while painting (folder exists? volume online?) come from caches
  filled by workers.
- Memory budget: RAM/4 clamped 256 MiB–1.5 GiB, split between texture cache, decode buffers and
  render caches (prior art: LightCraft `engine/src/memory.rs`).
- Per-frame cost must stay flat at 100 k photos: everything derived from the library is cached per
  revision; visible rows are found by binary search.

## 6. Image pipeline

1. **Thumbnail fast path:** embedded EXIF thumbnail (JPEG) or embedded RAW preview → shown
   immediately; a proper downscale replaces it in the background.
2. **Previews:** decode at the smallest scale that covers the target (JPEG DCT scaling), apply
   EXIF orientation, convert to sRGB (ICC via `moxcms`), resample, cache.
3. **Edits:** the recipe is applied by `edit::render(source, &recipe, target_size)`. Every
   operation is resolution independent (normalised coordinates, radii relative to the image) so
   thumbnails, the edit view and full-size output match. Thumbnails always show edits. The CPU
   renderer is the oracle; any GPU path must match it (`development.md` §14).
4. **Cache keys** are content-addressed: (file path, size, mtime, recipe hash, renderer version).
   The renderer version is per operation, not one global constant (avoids merge hotspots).
5. **Never blank while scrolling** (maintainer decision, 2026-10-10). Three layers, each drawn
   until the next one arrives:
   - a **colour sketch** per photo (a 24–26-byte blurred preview in the ThumbHash style, capped
     at 32, made at scan with the edits applied), stored in the index and held in memory for every
     photo (measured at M4: about 45 bytes per photo in the index, ≈ 4.8 MB in memory at 100 k
     with its key), so any cell can be drawn on any frame without a disk read;
   - a **size ladder** of cached thumbnails in device pixels: 128 and 256 for every photo at scan,
     512, 1024 and a screen-size edit-view preview (long edge = the display's) on demand. A cell
     draws the smallest ladder size that covers it, or the next smaller one scaled up (blurry,
     then sharp) while the right one loads; only a photo never scanned shows the `gray 38`
     placeholder;
   - the full decode, only for sizes above the ladder.
   The pool prefetches in the scroll direction, further ahead the faster the view moves.
6. **Disk cache:** `<cache>/thumbs/`, JPEG q90 (fastest to decode), one set of append-only pack
   files plus an index per ladder size, written and read through `fs` with positioned reads (no
   memory mapping), never one file per thumbnail. Entries made dead by edits, deletes and moves are
   compacted in the background when they pass 25 % of a pack. 2 GiB default, pruned to 80 %, the
   largest sizes first. The cache only lists and deletes files of its own naming shape. Prior art:
   LightCraft `disk.rs` for pruning; the pack layout is our own (the reference organizer also
   packed each size into a few large files).
   Each index record and each entry carries a CRC-32, so a torn tail or a damaged record costs only
   its own entry. A compaction writes the size's next generation beside the current one (pack,
   then index; a generation counts only once its index exists) while reads go on. The folder is
   held with an OS lock, so two processes never write one cache (a second open is refused at once).

## 7. Automation: agents can drive and see the app

- **Widget registry** (`ui-kit`): every interactive widget registers an id (`thumb:<photo>`,
  `tree:folder:<id>`, `button:tray.hold`, `tool:crop`, `slider:tuning.fill_light`) with its rect
  each frame. Tests and agents click by id and assert geometry from the same table.
- **Headless driver** (`ui::headless::Driver`): runs the real `App::update` against an egui
  `Context` with no window and a virtual clock. `click(id)`, `drag`, `key`, `type_text`,
  `step_until(condition, max_frames)` (never "wait N frames"), `state()`, `widgets()`,
  `screenshot(path)`. Used by `ui` tests, `dalica-cli snapshot`, and the control channel's headless
  mode.
- **Screenshots without a GPU:** a CPU rasterizer for egui's tessellated output lives in `ui-kit`
  (prior art: LightCraft `softpaint.rs`). Because Inter is bundled and rasterised by us,
  screenshots are identical on Linux, Windows and macOS. It is our own (`ui-kit/src/raster.rs`,
  M1.16): `egui_software_backend` 0.0.3 needs egui 0.34. It reproduces `egui-wgpu` on a gamma
  framebuffer: premultiplied gamma vertex colours × a bilinear texel-centre sample, premultiplied
  "over" blending stored at 8 bits, a top-left fill rule and pixel-rounded clip rectangles.
- **CLI** (`dalica-cli`), the agent's main verifier from M1:
  `commands` (the registry as JSON), `run --library DIR script.jsonl` (execute commands, print
  each result, then `flush`), `inspect --library DIR [ids]`, `render <file> --recipe … -o out.png`,
  `snapshot [--demo|--library DIR] --script tour.jsonl --size 1400x900 -o out.png`.
- **Control channel:** `dalica --control <port>`, a loopback JSON-lines server with a token
  (`control-protocol.md`): `engine.execute`, `engine.commands`, `ui.state`, `ui.widgets`,
  `ui.clickWidget`, `ui.pointer`, `ui.key`, `ui.type`, `ui.menu.tree`, `ui.menu.invoke`,
  `ui.screenshot`, `ui.inspect` (perf counters). Method handlers live in `ui` and are shared with
  the headless driver; the transport lives in `apps/dalica`.
- **MCP:** `dalica-cli mcp [--demo | --library DIR | --connect host:port]` exposes the command
  registry plus helpers (`mcp.md`).
- Never drive the app with OS-level synthetic input (it goes to whatever window is focused).

## 8. Never crash, never lose data

- Non-test code never panics: no `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`; every crate root carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`
  and `#![forbid(unsafe_code)]`. `clippy.toml` allows them in tests only.
- File-derived numbers are hostile: `get()` instead of indexing, checked arithmetic, capped
  allocations, bounded recursion and directory walks (depth 32, 1,000,000 entries, symlink loops).
- Last-resort guard: a panic hook + `catch_unwind` around command dispatch and per-file scan/import
  work turns an escaped panic into an error toast and keeps going. `panic = "unwind"`.
- **Originals are sacred:** Dalica never rewrites a photo's pixels except on an explicit Save, and
  Save first moves the original into the folder's hidden originals directory (see `data-model.md`).
  Metadata write-back merges into existing XMP/IPTC, never replaces. All writes go through `fs`:
  atomic (temp file + fsync + rename) and verified. Damaged settings/sidecar files are kept as
  `.corrupt-<time>` copies.
- Every crash or data-safety fix lands with a regression test that failed before the fix.

## 9. Prior art: the Craft apps

**Maintainer decision, 2026-10-11** (replaces the reference-only decision of 2026-10-09):
LightCraft and PhotoCraft are MIT OR Apache-2.0, the same licence as Dalica, so we study them for
technology choices, techniques and methodology, match their look and feel, and may reuse their
code. Reused code:

- keeps a header naming its origin (project, file, revision) and gets a `NOTICE` row, as both
  licences require;
- meets Dalica's rules before it lands: the layer map, the never-crash standard, writes through
  `fs`, colours from `Tokens`, tests to Dalica's contract, and `cargo xtask names`;
- is copied into Dalica's own crates; depending on a Craft crate directly is allowed only at a full
  commit hash (`cargo xtask deps` enforces it).

Code from `craft-ui`, the UI crate both apps share (`github.com/storytold/craft-libs`, same
licence), may be copied the same way; a direct dependency on it is not on `cargo xtask deps`'s
allowed list. Ordinary third-party crates from crates.io remain allowed after the dependency check
(`security.md` §8). This is the opposite of the reference organizer, whose code is never looked at
or used (`process.md` §3). Local checkouts: `~/Sources/lightcraft` and `~/Sources/photocraft`; both
are public (`github.com/storytold/…`). The checkouts lag upstream: run `git fetch` in them and read
`origin/main` (`git show origin/main:<path>`) rather than the working tree, and name the revision
you copied from in the file header. `craft-ui` has no checkout; read it on GitHub at the revision
the Craft apps pin.

| Dalica area | Study or reuse in LightCraft | What to learn |
|---|---|---|
| Look, widgets, icons drawn in code | `crates/ui-egui/src/{theme,widgets,icons}.rs` | token set, widget proportions, icon grid; Dalica's `design-system.md` holds the values |
| Headless UI and screenshots | `crates/ui-egui/src/{headless,softpaint}.rs` | rasterizing egui output on the CPU for tests and agents |
| Control channel, menus, shortcuts | `crates/ui-egui/src/{control,menubar,menu_level,shortcuts}.rs`, `panels/keymap.rs` | command dispatch by id, menus that never run off the window, the shortcut editor |
| Grid at 100 k photos | `crates/ui-egui/src/panels/grid.rs` (`layout`, `rows_between`) | layout cached per revision, visible rows by binary search |
| Job pool, LRU, disk cache | `crates/preview` | prioritised de-duplicating pool, cache pruning |
| Persistence | `crates/catalog/src/{journal,store,safe_file,lock}.rs` | op journal + snapshot, atomic writes, single-process lock |
| Metadata, decoding, RAW previews | `crates/{meta,tiff,codecs,raw}` | merge-never-replace writes, decode limits, embedded-preview extraction |
| Organizer plumbing | `engine/src/{memory,availability,walk,rename,guard}.rs` | memory budget, offline volumes, bounded walks, panic guard |
| Faces (M13) | `crates/faces`, `panels/faces.rs` | detector/recogniser pipeline, consented model downloads, the face grid |
| Text field, autocomplete, date rules (M5.25, M6.29) | `crates/ui-egui/src/{text_field,album_picker,date_picker}.rs` | revert on Esc, commit once, edit menu, type-ahead filtering, date limits |
| Crop guides, filmstrip, compare (M7, M11) | `panels/{crop_overlay,detail,compare}.rs` | guide geometry, filmstrip and compare patterns |
| Shortcut editor (M10) | `panels/keymap.rs`, `shortcuts.rs` | click to rebind, remove, restore; key parsing and labels |
| Translations (M15) | `crates/ui-egui/src/i18n.rs`, `locales/*.json` | table-driven catalogs, script-based CJK font fallback |

From PhotoCraft (`crates/ui-egui/src/`): the command/`UiState` model, `shortcut_dispatch.rs`,
`dialogs.rs` (dialogs as data, M5.14), `theme.rs` (themes, M16), `widgets/type_ahead.rs`
(M5.25), `rgb_histogram.rs`, `tone.rs`, `wheel_nav.rs` and `flick_pan.rs` (M7), `prefs_ui.rs`
(the Options draft and key capture, M10), `tab_strip.rs` (M11), `color_picker_ui.rs` (M12), the
scorecard and perf budgets, and the process (`notes/craft-apps/` has both studies). From
`craft-ui`: `tabs` (M7, M11).

Never take: their develop-pipeline UI, packaging metadata (it contains other apps' strings), or
anything under `docs/brand/` (ArtCraft trademarks). Some of our docs adapt Craft-app documentation
text with attribution; those files say so in their first lines and are listed in `NOTICE`.

## 10. UI structure

- **`ui-kit`** is a small design-system library: `Tokens` (every colour, radius, metric and font
  size from `design-system.md`), hand-painted widgets, icons drawn in code, the widget registry and
  the CPU rasterizer. It has no idea what a photo is, so it can be tested (and screenshotted) alone
  with a gallery of every widget in every state.
- **`ui`** is the app: `UiState` in `state.rs` (plain serde data: view, panes, scroll anchors,
  selection, tray, zoom, open dialogs), so `ui.state` / `ui.set` can read and drive it; panels in
  `panels/<region>.rs`, one module per region of `ui-layout.md` (toolbar, tree, light table, status
  line, tray, bottom bar, side panes, edit view…) so parallel agents rarely touch the same file.
- Panels read the `Session` through query methods and change it only through `App::run`. Painting
  never touches the disk (§5).
- **Dialogs** (folder pickers, save-as) go through a `Picker` trait: `rfd` in the app, a scripted
  picker in tests, the control channel and MCP.
- **Layout is data:** region sizes come from `Tokens` metrics and `ui-layout.md`; tests read region
  rects back from the widget registry at 1400 × 900 and 900 × 600 and compare them with the spec.
- **Scale:** UI scale (75–300 %) and font size are settings; headless tests render at 100 %, 150 %
  and 200 %.

## 11. Testing each crate on its own

Every crate's tests run with `cargo test -p dalica-<crate>` and need no window, GPU, network,
personal photo or other crate's state. Test inputs are built in the test (synthetic bytes or
`scenes`); bigger real-world corpora are fetched by `cargo xtask corpus` and run behind the
`corpus` feature (`development.md` §11). Media is never committed.

| Crate | Tested alone by |
|---|---|
| `core` | unit + property tests: geometry round trips, date parsing, collision naming, position keys (a key always exists between two keys; ordering is total) |
| `fs` | temp-dir tests for every primitive; **fault injection** (feature `faults`) fails each step of a write (create temp, write, fsync, rename, verify) and asserts the old or the new file, never a torn one; lock contention between two processes; UI-thread guard |
| `image` | `scenes` files at 8/16-bit, grey/RGB/CMYK, tagged ICC profiles and all 8 orientations; magic sniffing incl. lies (`.jpg` that is PNG); limits (header bombs refused before allocating); truncation at every offset and proptest random bytes never panic |
| `meta` | synthetic JPEG/PNG/WebP/TIFF built in the test: write → read returns owned fields; bytes outside metadata segments hash the same; foreign fields byte-identical; idempotent writes; size-limit failure leaves the file unchanged (`xmp-interop.md` §8) |
| `sidecar` | property tests: parse(print(x)) == x; unknown keys and wrong-type values preserved; any file name round-trips; corrupt files reported, never panics; legacy reader fixtures written inside the test from the documented format |
| `edit` | parse/print round trip, unknown ops preserved in place; per-operation **fingerprint tests** on `scenes` images (see below); invariants (B&W has R = G = B, crop output size, rotate ×4 = identity, effects at default are deterministic); resolution independence (render at 2 sizes, compare after downscale) |
| `scenes` | determinism (same seed → same bytes) and that its EXIF block decodes |
| `library` | pure property tests: for random op sequences, applying each op then its inverse restores the model exactly; revisions bump; queries and search grammar against hand-built models |
| `thumbs` | job pool with fake jobs: priority order, de-duplication, cancellation, no lost results; cache pruning only deletes its own file shapes |
| `store` | temp-dir libraries: model → sidecar → model loses nothing (and the "every model field has a sidecar mapping" test); **rebuild loses nothing**; debounce on virtual time; crash between ops leaves a readable journal; metadata queue merges and verifies |
| `scan` | temp-dir trees: add/remove/rename detection via the deterministic verification pass; symlink loops and depth limits; offline volume simulated by renaming a root; legacy sidecars picked up; `notify` events tested with generous timeouts as hints only |
| `create` | export output decoded and checked (size, quality bound, metadata copied, GPS removed when asked, collisions `-001`); collage layouts deterministic per seed |
| `engine` | every command through `Session::execute` on a temp-dir library: the command test contract (§3); undo/redo; keymap resolution and conflicts; `panic_hunt` |
| `ui-kit` | token tests (contrast ratios, no duplicate values where the spec says distinct); widget gallery rendered by the CPU rasterizer; rasterizer pixel tests on known shapes |
| `ui` | headless driver tests: click/drag/key by widget id and assert `UiState` and engine state; region geometry vs `ui-layout.md` at both sizes; per-frame counters (cells visited, jobs requested) stay flat as the library grows |
| `mcp` | workflow acceptance tests over MCP against a headless engine, checking only public state and output files (`mcp.md` §9) |
| apps | smoke tests: `dalica-cli run` on a script, `snapshot` produces a PNG of the right size |

**Fingerprints instead of golden images.** Pixel tests store a small JSON fingerprint per case
(per-channel mean and standard deviation over an 8 × 8 grid, plus output size) with a tolerance,
not a PNG. On failure the test writes actual/expected/diff PNGs under `target/test-output/` for a
human or agent to look at. `DALICA_BLESS=1` rewrites fingerprints and must be reviewed in the
diff: the PR shows the images and the commit body says why. New and changed baselines merge with
their task and the maintainer approves them in bulk at the next review (`task-board.md` §7). No
media is committed.

## 12. What is hard to test automatically, and how we check it

| Risk | Why automation falls short | How we check it |
|---|---|---|
| The look matches the Craft apps and the layout feels like the reference organizer | "looks right" is a judgement; reference screenshots must not be pixel-compared or copied | numeric layout assertions (§10) + token tests; agents take headless and windowed screenshots at 1400 × 900 and 900 × 600 after every UI change and compare by eye with `ui-layout.md` and the private notes; before/after screenshots in every UI PR; maintainer review at each milestone end |
| Effect and Auto Fix quality | correctness is aesthetic | fingerprint tests pin the look once approved; invariants catch breakage; the maintainer approves every `DALICA_BLESS` change by viewing the diff PNGs, in bulk at the next review |
| Real-world files (camera JPEGs, RAW previews, HEIC, metadata written by other apps) | synthetic files miss vendor quirks | pinned, sha256-verified external corpora with floors that only rise; optionally `exiftool` as an outside oracle in the corpus job only (never in the product) |
| Interop with other apps' metadata | we can't run every app | corpus files written by other apps + byte-preservation hashes; a manual round-trip checklist (open in another app, edit, rescan) before releases |
| File watching, network drives, sleep/wake, unplugged volumes | OS event timing is nondeterministic | events are hints; the deterministic verification scan is what tests assert; offline volumes simulated; a manual unplug/replug checklist per release |
| Crash safety | real crashes are random | `fs` fault injection at every write step; journal torn-tail tests; a kill-the-process loop test in `store` |
| Performance at 100 k photos | needs scale and quiet machines | synthetic in-memory libraries; `cargo xtask bench` before/after on hot paths; budgets in `perf/budgets.toml`; nightly perf runs on one fixed machine, never per PR |
| Platform differences (Windows paths and locking, macOS menus and Cmd keys, Wayland translucency, trash) | one dev machine | CI on Linux, Windows and macOS for every PR; odd-name path tests; menu model tests instead of native-menu tests; a per-release manual checklist per OS |
| GPU vs CPU, DPI, real windows | headless isn't the window | CPU is the oracle; agents check the real window through the control channel's `ui.screenshot` for each check-in page |
| Timing, animation, debounce, drag feel | wall-clock tests are flaky; "feel" is subjective | virtual `Clock`; the driver advances time explicitly; human review of drag/scroll feel |
| Native dialogs | can't be clicked headlessly | `Picker` trait, scripted in tests |
| Accessibility and IME | needs assistive tech and real input methods | AccessKit tree assertions headless; manual IME and screen-reader checks (M15) |
| Face recognition (M13), video (M15) | model- and codec-dependent | decided at those milestones; small public-domain face set with expected groupings as floors |
