# dalica-scan

**Layer L3.** Watched-folder rules, walks, change detection, `notify` events, offline volumes and card/folder import; turns what it finds into `Op`s.

- **May depend on:** L0–L2 (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.11 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-scan` (no window, GPU, network or personal photos needed).
