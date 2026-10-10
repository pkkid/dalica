# dalica-library

**Layer L2.** The in-memory model (folders, photos, albums, people, tags, captions, stars, hidden, recipes, places), `Op`s with inverses, revisions, queries and the search grammar.

- **May depend on:** `core` only; it stays pure (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `serde`, `thiserror`.
- **Public API (M1.8):**
  - `Library`: `apply(Op) -> Result<Op>` applies an op completely or not at all, bumps `revision()` once and returns the inverse. `folder_revision(id)` says when a folder last changed. `next_folder_id` / `next_photo_id` never reuse an id.
  - `Op` (serde-tagged, one journal record): `AddFolder`, `RemoveFolder` (with its photos), `AddPhotos`, `RemovePhotos`, `UpdatePhotos`, `SetStars`, `Batch`.
  - `Folder { id, path }`, `Photo { id, folder, name, size, mtime, width, height, date, star }`, `Model` (the comparable content).
  - Queries: `folder`, `photo`, `folders`, `photos`, `folder_by_path`, `photo_by_name`, `photos_in` (capture date oldest first, then name), `folders_in_display_order` (newest folder date first), `folder_date`, `starred`.
- **Test it alone:** `cargo test -p dalica-library` (no window, GPU, network or personal photos needed). `tests/ops.rs`: for random op sequences, applying an op and then its inverse restores the model and each apply bumps the revision once.
