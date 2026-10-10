# dalica-core

**Layer L0.** Ids (`PhotoId`, `FolderId`, `AlbumId`, `PersonId`), errors, normalised geometry, dates, path and name utilities (collision names `-001`), position keys and the `Clock` trait.

- **May depend on:** nothing (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.1 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-core` (no window, GPU, network or personal photos needed).
