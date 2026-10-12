# dalica-engine

**Layer L4.** `Session`: library + store + jobs + settings + undo stacks; the command registry (`cmd/<area>.rs`); keyboard contexts and the keymap; control-protocol types; demo and synthetic libraries; the session's thumbnails.

- **May depend on:** L0–L3 (`docs/architecture.md` §2; `cargo xtask layers` enforces it).

## Modules: one per concern (M5.5)

`session.rs` keeps the core (opening, `execute`, applying ops, undo, the I/O worker, the per-frame `poll`). Every other concern is a module with its state in a `pub(crate)` field of `Session` and its methods in an `impl Session` block of its own, so the tasks that grow them never share a file:

| Module | Concern | Grown by |
|---|---|---|
| `folders.rs` | the watched folders and `folders.json`; `is_new_library()` | M5.12: modes per folder, inheritance, exclusions, `folders.json` v2 |
| `scanning.rs` | scans as jobs, applied progressively; failure memory; `scan_options()` | M5.12 (File Types, excluded subtrees), M5.17 (rescans of dirty folders) |
| `jobs.rs` | the job registry | every later job kind (import, export, save, batch edit, …) |
| `watching.rs` | external changes: a stub `poll_watching()` called from `poll()` | M5.17 |
| `offline.rs` | offline volumes: a stub `poll_offline()` called from `poll()` | M5.13 |
| `thumbs.rs` | the thumbnail ladder | M5.16 (cache upkeep, colour management, per-batch queueing) |
| `cmd/view.rs`, `cmd/section.rs` | empty command areas, already registered | M5.16 (`view.colorManagement`, `section.refreshThumbs`) |

## Public API

- **Session (M1.13):**
  - `Session::open(library_dir, clock)` / `open_with(…, &Options)` (takes the library lock — a second session gets `OpenError::Locked` — loads the index, reporting repairs in `recovery()`, reads `folders.json`, starts the I/O worker). After opening, the owning thread never touches the disk: journal appends, debounced sidecar writes, snapshots and `folders.json` go to the I/O worker; scans run on their own threads.
  - `execute(id, params)`: `enabled` check, typed params (a JSON object; `bad_params` otherwise), `catch_unwind` (`internal`), exactly one library undo step per command that changed something. `poll()` (each UI frame: at most `PHOTOS_PER_POLL` photos of what the scans found, then the watcher, the online checks and the thumbnails; never blocks), `wait_idle()` (CLI, tests: everything, blocking) and `flush()` (waits until every pending write has landed).
  - Commands: `folders.add`, `library.query`, `library.rebuild`, `photo.inspect`, `photo.star.toggle`, `photo.star.set`, `edit.undo`, `edit.redo`. `commands()` lists them as `engine.commands` does.
  - `CommandSpec`, `Context`, `Effect`, `History`, `CmdError` (with the control protocol's codes), `UndoStack` (cap 1 000).
- **Keymap and settings (M1.14):**
  - `Keymap` (pure): `Key::parse` (one canonical form; `Cmd` = `Ctrl`), defaults `from_specs`, user `Override`s per (command, context), `resolve(key, view, text_input)` (Text input → view → Global), `conflicts()` (same context, or Global and any other), `set(…, replace)`, `reset`.
  - `Settings` (`settings.json` in the library directory): unknown keys and unknown keymap entries kept; a damaged file kept aside, defaults used, reported in `Session::notices()`. `change_settings(f)` saves through the I/O worker.
  - Commands `app.setShortcut {command, context, key|null, replace?}` and `app.resetShortcuts {command?}`; overrides survive a restart.
- **Synthetic libraries (M4.7, `synthetic`):** a `Spec` (photos, folders, seed, …) decides a whole library; `library` / `write_index` / `open` build it index-only (100 000 photos with real sketches and no files: written in ≈ 0.2 s, opened in ≈ 0.1 s in release), and `write_photos` / `open_scanned` write the same plan as small `scenes` JPEGs (at most `MAX_SCANNED_PHOTOS`) and scan them. Ids follow the demo library's rule. `demo` builds the `--demo` library.
- **Thumbnails (M4.8, `thumbs`):** `Options { thumbs: ThumbOptions { cache: CacheDir, memory, workers, screen } }`. As scans land, a background job per new or changed photo makes its 128 and 256 entries and its colour sketch from one decode (sketches applied as derived ops: journaled, never undoable, never in a sidecar). `thumbnail(id, need)` answers at once from memory (the covering size, else a smaller one to scale up, else the sketch, else `Thumb::NotScanned`) and queues what is missing; `prefetch_thumbnails(order, visible, velocity, need)` loads ahead in the scroll direction and cancels stale prefetch; `thumbnail_failure`, `thumbnails_pending`, `thumbnail_stats`, `thumbnail_cache_dir`, `set_screen_size`.
- **Jobs and progressive scans (M5.5):**
  - The job registry (`jobs`, `control-protocol.md` §6.13): `JobEntry { job, kind, label, state, progress {done, total}, cancellable, startedMs, finishedMs?, result?, error? }` (serialised as the protocol shows it; times in milliseconds on the session clock since the session opened), `JobKind` (`scan`, `rebuild`, …), `JobState` (`queued | running | done | failed | cancelled`). Finished jobs stay listed for `KEEP` (10 minutes on the session clock) and the registry holds at most `CAP` (64) entries, dropping the oldest finished first (a running job is never dropped). `Session::jobs()`, `job(id)`, `cancel_jobs(Some(id) | None)` (sets the cancel flags and returns the jobs it set; `NotFound` for an unknown or expired job; a finished one is not listed), `wait_job(id, timeout)` (polls until the job finishes or the timeout passes, then gives its entry; it blocks the caller, so the UI answers `jobs.wait` across frames with `job(id)`).
  - Scans (`scanning`) stream through `dalica_scan::scan_streaming` on a thread each and are `scan` jobs (`rebuild` for `library.rebuild` and a damaged index). Each folder is diffed against the model when its turn comes and applied a batch per `poll()`, so folders and thumbnails fill in while the walk goes on; the verification pass (folders gone from roots walked to the end leave the library) runs once everything found is applied. A rebuild builds its model to the side and swaps it in at the end (undo cleared); a cancelled one keeps the old model. Cancel stops the walk before its next file, finishes the folder being applied, drops what was read but not applied, removes nothing, and ends the job as `cancelled` with what it indexed. Files whose header read failed become scan warnings once and are given back to later scans (`scan_failures()`), which skip them until they change. `scan_progress()` gives `ScanProgress { running, foldersQueued, found, indexed }` (`library.inspect` → `scan`); `busy()`; `set_scan_reader(reader)` plants a header reader (tests, benches).
  - `is_new_library()`: no `folders.json` when the library opened and no folder added since (the first-run panel's question).

## Test it alone

`cargo test -p dalica-engine` (no window, GPU, network or personal photos needed):

- Every command on a temporary library of `scenes` photos (behaviour, undo/redo restore exactly, disabled cases, hostile params give errors and change nothing), and session tests (star 3, undo 1, flush → 2 in the sidecar; a second session is locked out; panics are caught; no file I/O on the owning thread).
- `tests/keymap.rs`; `tests/synthetic.rs` (same seed, same library; the 100 k preset; `--release -- --nocapture` prints its timings); `tests/thumbs.rs` (one decode per photo, answers that never wait, prefetch, no disk I/O on the owner thread).
- `tests/progressive_scan.rs`: 2 000 photos in 40 folders, the walk held at the last folder by a gated header reader: the first batch lands with its thumbnail jobs queued before the walk ends, no poll applies more than one batch, the progress counts rise to the totals, and the owner thread makes no `fs` call; a scan cancelled while reading folder 21 reads nothing past it and keeps the 20 folders it applied; a cancelled rebuild keeps the old model; a failing file is reported once and not read again until it changes.
- `tests/jobs.rs`: a scan listed with its progress and result in the protocol's shape; expiry after 10 minutes of virtual time; the cap of 64; `cancel(all)` and a `wait` that times out; `NotFound` for unknown jobs; `is_new_library`.
- `cargo test -p dalica-engine --test panic_hunt -- --ignored` fuzzes every command (300 cases each, plus 64 random sequences of up to 40 commands) and fails on any panic.
