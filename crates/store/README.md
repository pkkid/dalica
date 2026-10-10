# dalica-store

**Layer L3.** Persistence of the model: index journal + snapshot, model ↔ sidecar mapping, the debounced sidecar writer, the metadata write queue, rebuild from folders.

- **May depend on:** L0–L2 (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `serde`, `serde_json`, `thiserror`.
- **Public API (M1.9):**
  - `Store::open(library_dir, now)` → `Opened { store, library, report }`: loads `index.snap` and replays `index.log` after it. Damage never fails the open: a torn or bad tail is cut off (`JournalTail`); a damaged snapshot is kept aside and the journal used when it holds every op (`SnapshotSetAside`); a journal behind its snapshot restarts (`JournalReset`); otherwise the index is kept aside as `.corrupt-<time>` and the library opens empty (`RebuildNeeded`).
  - `Store::append(&Op)` (synced, ≤ 16 MiB per record), `snapshot(&Library)` (atomic, then a fresh journal), `wants_snapshot()`, `seq()`.
  - `journal` (`<crc32> <json>` lines, header `{"journal": 1, "base": N}`), `snapshot` (one checksummed record), `crc32`.
- **Public API (M1.10):**
  - `mapping`: `PHOTO_FIELDS` (every `Photo` field: index, file or sidecar), `folder_state`, `apply`, `star_of`.
  - `write_folder(&FolderState, now)`: re-read, merge, skip if unchanged, atomic replace with an identity check; a damaged sidecar is kept as `.dalica.json.corrupt-<time>`. Any thread but the UI thread.
  - `SidecarWriter`: `mark_dirty`, `write_due` (at most once per second per folder on the clock), `flush` (everything now), `read_only` folders after damage, `clear_read_only`.
  - `import_sidecars(&Library, now)` → `Import { ops, read_only, problems }`: after a rebuild from the folders, the ops that bring back the sidecars' stars.
- **Test it alone:** `cargo test -p dalica-store` (no window, GPU, network or personal photos needed): replay reproduces the model, every byte-prefix of a journal replays to its last complete op, damaged records, snapshots and versions recover as reported. `tests/sidecars.rs`: stars reach the sidecar with other keys intact, writes are debounced on virtual time, deleting the index and rebuilding gives the same stars, every fault point leaves the old or the new sidecar.
