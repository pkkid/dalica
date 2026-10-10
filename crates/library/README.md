# dalica-library

**Layer L2.** The in-memory model (folders, photos, albums, people, tags, captions, stars, hidden, recipes, places), `Op`s with inverses, revisions, queries and the search grammar. Pure: no I/O, no threads.

- **May depend on:** `core` only (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.8 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-library` (no window, GPU, network or personal photos needed).
