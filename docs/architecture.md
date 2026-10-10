# Architecture

Dalica is an **engine-first Cargo workspace** of small, tested crates with enforced layering, the
same shape as LightCraft and PhotoCraft. The egui front end is one thin crate; nothing below it
knows a UI exists. Every user-visible action is a **command** with a stable id, dispatched through
one entry point by the UI, the keyboard, the CLI, the control channel and the MCP server.

Read with: [`data-model.md`](data-model.md) (what is stored where), [`design-system.md`](design-system.md)
(the look), [`ui-layout.md`](ui-layout.md) (the window), [`development.md`](development.md) (build,
test, tooling).

## 1. Stack (pinned)

| Concern | Choice | Why |
|---|---|---|
| Language | Rust, edition 2024, `rust-version = "1.90"`, `resolver = "3"`; toolchain pinned in `rust-toolchain.toml` | same as the Craft apps; a pinned toolchain keeps `clippy -D warnings` stable (a lesson LightCraft paid for) |
| UI | `egui` / `eframe` **0.36** (features `default_fonts, wgpu, accesskit, wayland, x11`), `egui_extras` | identical API to the Craft apps so their theme/widgets/icons copy over unchanged |
| GPU | `wgpu` 30 (Metal / Vulkan / DX12, no GL) | eframe renderer; later, compute shaders for effects |
| Native dialogs | `rfd` 0.17 — always called off the UI thread | GNOME reports "not responding" otherwise |
| macOS menu bar | `muda` 0.21, generated from the command registry | one menu model, three surfaces |
| Images | `image` 0.25 (png, jpeg, tiff, webp, gif, bmp), `zune-jpeg`, `jpeg-encoder`, `moxcms` (ICC) — or the LightCraft `codecs` / `meta` / `raw` crates at a pinned revision (see §9) | pure Rust, no C |
| Metadata | EXIF/IPTC/XMP read + XMP/IPTC write (LightCraft `meta` + `tiff`, pinned) | captions, tags, people regions and GPS must round-trip with other apps |
| Video (M9) | thumbnail + playback through a pure-Rust demux/decode path if one is viable; otherwise an optional, clearly separated `ffmpeg` process integration behind a feature flag | decision recorded in `roadmap.md` before work starts |
| File watching | `notify` (pure Rust) + periodic verification scans | multi-root watched folders |
| Parallelism | `rayon` for pixel work; a prioritised job pool for previews | |
| Serialization | `serde`, `serde_json`; folder sidecars are pretty-printed JSON with sorted keys; a small read-only INI reader for legacy sidecars (see `data-model.md`) | human-readable, diffable |
| Errors | `thiserror` per crate | |
| Logging | `log` + a rotating file logger in the app (`<config>/logs/dalica.log`) | |
| Tests | built-in test harness, `proptest`, headless UI snapshots | |

**Product rules:** pure Rust (no C/C++ build dependencies in the product; audit `Cargo.lock` for
`ring`, `openssl-sys`, `*-sys`), no webview/JS/Tauri, no network access except explicit,
user-initiated actions (map tiles, optional model downloads), no telemetry.

## 2. Workspace layout

```text
Cargo.toml                 [workspace] crates/*, apps/*, xtask; [workspace.package]; [workspace.dependencies]
rust-toolchain.toml        pinned stable
clippy.toml rustfmt.toml   tests may unwrap; max_width = 150, use_small_heuristics = "Max"
crates/
  core        L0  ids (PhotoId, FolderId, AlbumId, PersonId), errors, path utils, dates, geometry
  image       L1  sniff/decode/encode, embedded-thumbnail extraction, orientation, ICC → sRGB
  meta        L1  EXIF / IPTC / XMP read and write-back (merge, never replace)
  sidecar     L1  per-folder `.dalica.json`: parse, edit, atomic write; read-only legacy sidecar fallback
  edit        L1  the edit recipe (ordered list of operations) + CPU renderer for every tool/effect
  library     L2  the library model: folders, photos, albums, people, places, tags, captions,
                  stars, hidden; ops with inverses; journal + snapshot persistence; queries/search
  scan        L2  watched-folder manager, directory walks, file watching, device/card import
  thumbs      L2  prioritised job pool, in-memory LRU, on-disk thumbnail cache
  faces       L2  face detection, alignment, embeddings, clustering (M7; see faces.md)
  fetch       L1  consented, resumable, hash-pinned model downloads (M7)
  create      L3  slideshow sequencing, collage layout + render, export (resize/quality/watermark)
  engine      L4  Session: library + caches + undo stack; command registry; control-protocol types
  ui          L5  egui shell (crate name `dalica-ui`): theme, widgets, icons, panels, menus, shortcuts,
                  headless renderer, control server glue
  mcp         L5  MCP server over the command registry
apps/
  dalica      desktop app (eframe), `--control <port>` JSON-lines server
  dalica-cli  headless: `run`, `render`, `snapshot`, `commands`, `mcp`, `scan`
xtask/        cargo xtask ci | layers | assets | parity | stats | corpus | snapshot
```

Crate package names are `dalica-<dir>` (`dalica-core`, …, `dalica-ui`).

### Layering (enforced by `cargo xtask layers`)

A crate may depend only on crates in **lower** layers. `xtask/src/layers.rs` holds the table; a new
crate must be registered there or CI fails.

| Layer | Crates | May not depend on |
|---|---|---|
| L0 | `core` | any workspace crate |
| L1 | `image`, `meta`, `sidecar`, `edit`, `fetch` | L1+ siblings except `edit → image` |
| L2 | `library`, `scan`, `thumbs`, `faces` | L2+ siblings except `scan → library`, `thumbs → (image, edit)`, `faces → (image, fetch)` |
| L3 | `create` | L3+ |
| L4 | `engine` | L5 |
| L5 | `ui`, `mcp` | each other |
| apps | `dalica`, `dalica-cli` | — |

Nothing below L5 may depend on `egui`, `eframe`, `winit` or `rfd`. Everything in L0–L4 must also
`cargo check --target wasm32-unknown-unknown` once a web build is on the roadmap (not before M10).

## 3. Commands: the one seam

```rust
pub struct CommandSpec {
    pub id: &'static str,            // "photo.star.toggle", "folder.rename", "edit.crop.apply"
    pub label: &'static str,         // English UI label, also the i18n key
    pub menu: &'static str,          // "Picture/Star"; "" if not in a menu
    pub shortcut: &'static str,      // "Ctrl+S" (Cmd on macOS is written Cmd and mapped)
    pub params: &'static str,        // agent-readable doc: {"ids":[PhotoId], "on":bool=toggle}
    pub effect: Effect,              // Query | Library | Files (writes to photo folders; needs confirm
                                     // over automation when destructive) | External (outside Dalica)
    pub enabled: fn(&Session, &Value) -> bool,
    pub run: fn(&mut Session, &Value) -> Result<Value, CmdError>,
}
```

- Registered per area in `crates/engine/src/cmd/<area>.rs` (`pub fn specs() -> Vec<CommandSpec>`),
  collected in `cmd/mod.rs`. New areas add a new module rather than growing a shared file.
- `Session::execute(id, &params)` is the only way state changes. It wraps `run` in
  `catch_unwind`, records **exactly one** undo step (drags and typing sessions coalesce), and
  returns JSON.
- UI-only commands (pane toggles, zoom, view switches, window state) live in a static
  `UI_COMMANDS` table in `crates/ui/src/menus.rs` and go through the same `App::run(id, params)`.
- The **menu catalog** (`crates/ui/src/menu_catalog.rs`) lists the complete target menu tree
  (labels, order, separators, default shortcuts, command ids) from
  [`menus-and-shortcuts.md`](menus-and-shortcuts.md). Items whose id has a registered command are
  live; the rest render disabled. Giving a new command the catalog id lights up its menu item.
- `cargo xtask parity` cross-checks `docs/parity.md` against the registry and the menu catalog.

## 4. State, undo and persistence

- `Library` (crate `library`) is an in-memory model mutated **only** by `Op`s. `library.apply(op)`
  returns the inverse op; the inverse is the undo entry. The same op stream is appended to a
  crash-safe journal (JSON lines, checksummed) with periodic background snapshots, exactly the
  LightCraft catalog design (`journal.rs`, `store.rs`, `safe_file.rs`, `lock.rs`).
- **Source of truth for user data is on disk next to the photos** (per-folder sidecars, plus
  metadata written into files where the user enabled it). The library journal/snapshot is an
  index and cache that can be rebuilt from the folders at any time. See `data-model.md`.
- File operations (move, rename, delete-to-trash, save edited copy) are ops whose inverse is
  recorded where possible (move back, rename back, restore from the trash is best-effort and is
  never promised in the UI).
- Undo history: per session, capped at 1,000 entries; edit-recipe history is per photo and
  persisted in the sidecar (so "Undo" in the edit view works across restarts).
- A library lock file prevents two Dalica processes from opening the same library.

## 5. Threading and responsiveness

The UI thread **never** decodes images, touches the file system, waits on a lock held by a worker,
or blocks on a dialog.

- **Job pool** (copy of LightCraft `preview/src/pool.rs`): a prioritised, per-slot de-duplicating
  pool; workers = cores − 1 clamped 2..8. Priorities (high → low): edit-view main image, hovered
  photo, visible thumbnails, filmstrip, effect-tile previews, prefetch (next/previous photo),
  background thumbnail generation.
- **Renderer** (UI side): turns finished jobs into egui textures, drops stale results, evicts
  textures beyond ±1 screen (cap ~600 thumbnails).
- **Scanner** thread(s): directory walks and change detection for watched folders, feeding batched
  `Op`s to the UI thread (applied a few hundred per frame so the light table fills progressively).
- **File-system answers** used while painting (folder exists? volume online?) come from caches
  filled by workers (`fs_cached`); test with an injected slow file system and assert the thread.
- **Long jobs** (import, export, save, collage, batch edit) run on workers with an
  `AtomicBool` cancel and report progress to the status line.
- Memory budget: RAM/4 clamped 256 MiB–1.5 GiB, split between texture cache, decode buffers and
  render caches (LightCraft `engine/src/memory.rs`).
- Per-frame cost must stay flat at 100 k photos: everything derived from the library (tree rows,
  light-table layout, counts) is cached per library revision; visible rows found by binary search.

## 6. Image pipeline

1. **Thumbnail fast path:** embedded EXIF thumbnail (JPEG) or embedded RAW preview → shown
   immediately; a proper downscale replaces it in the background.
2. **Previews:** decode at the smallest scale that covers the target (JPEG DCT scaling), apply
   EXIF orientation, convert to sRGB (ICC via `moxcms`), resample, cache.
3. **Edits:** the recipe is applied by `edit::render(source, &recipe, target_size)`. Every
   operation is resolution independent (normalised coordinates, radii relative to the image) so
   thumbnails, the edit view and full-size output match. Thumbnails always show edits.
4. **Cache keys** are content-addressed: (file path, size, mtime, recipe hash, renderer version).
   The renderer version is per-operation, not one global constant (avoids merge hotspots).
5. **Disk cache:** `<cache>/thumbs/`, JPEG q90, 2 GiB default, pruned to 80 % (LightCraft `disk.rs`).
   The cache only lists/deletes files of its own naming shape.

## 7. Automation: agents can drive and see the app

- **Widget registry:** every interactive widget registers an id (`thumb:<photo>`,
  `tree:folder:<id>`, `button:tray.hold`, `tool:crop`, `slider:tuning.fill_light`) with its rect.
- **Control channel:** `dalica --control <port>` opens a loopback-only JSON-lines server
  (`docs/control-protocol.md`, M0): `engine.execute`, `engine.commands`, `ui.state`, `ui.widgets`,
  `ui.clickWidget`, `ui.pointer`, `ui.key`, `ui.type`, `ui.menu.tree`, `ui.menu.invoke`,
  `ui.screenshot {path, headless}`, `ui.inspect` (perf counters).
- **Headless rendering:** a CPU rasterizer for egui output (LightCraft `softpaint.rs` +
  `headless.rs`) so `dalica-cli snapshot --demo --script tour.jsonl -o out.png --size 1400x900`
  renders the real UI without a window or GPU. Tests use the same driver.
- **MCP:** `dalica-cli mcp [--connect host:port]` exposes the command registry plus helpers
  (scan a folder, query photos, render a photo with its recipe, screenshot). Thin layer, M4.
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
  Metadata write-back merges into existing XMP/IPTC, never replaces. All writes are atomic
  (temp file + fsync + rename). Damaged settings/sidecar files are kept as `.corrupt-<time>` copies.
- Every crash fix lands with a regression test that panicked before the fix.

## 9. Reuse from the Craft apps

Both Craft repos are `MIT OR Apache-2.0`. Dalica may depend on or vendor their code with attribution
(`NOTICE` lists every borrowed file/crate with its source revision; vendored files keep a header
comment naming their origin). Plan:

| Take | From LightCraft | How |
|---|---|---|
| Look and UI infrastructure | `ui-egui/src/{theme,widgets,icons,headless,softpaint,control,menubar,menu_level,shortcuts,tasks,pick,i18n}.rs`, `panels/mod.rs::resizable_side`, grid `layout`/`rows_between` | vendor into `crates/ui`, rename types |
| Job pool, LRU, disk cache | `crates/preview` | vendor into `crates/thumbs` |
| Metadata | `crates/meta`, `crates/tiff` | git dependency pinned to a revision, or vendor if we must patch |
| Decoding | `crates/codecs`, `crates/raw` (`embedded_preview`) | git dependency pinned |
| Persistence primitives | `crates/catalog/src/{journal,store,safe_file,lock}.rs` | vendor and adapt |
| Organizer plumbing | `engine/src/{memory,availability,walk,rename,guard}.rs` | vendor and adapt |
| Faces (M7) | `crates/faces` | evaluate; models downloaded by the user, never committed |

Do **not** copy: LightCraft's develop pipeline UI (Dalica's editing is simpler and different),
its catalog `Photo`/`Op` model, packaging metadata (it contains other apps' strings), or anything
under `docs/brand/` (ArtCraft trademarks).
