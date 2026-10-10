# dalica-thumbs

**Layer L2.** The prioritised, de-duplicating job pool, the in-memory LRU and the on-disk thumbnail cache.

- **May depend on:** `image`, `edit`, `fs`, `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.12 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-thumbs` (no window, GPU, network or personal photos needed).
