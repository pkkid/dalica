# dalica-ui-kit

**Layer L5.** The generic look: `Tokens`, widgets, icons drawn in code, the widget registry and the CPU rasterizer for screenshots. Knows nothing about photos or the engine.

- **May depend on:** `core` (+ egui) (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.16 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-ui-kit` (no window, GPU, network or personal photos needed).
