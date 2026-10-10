# dalica-store

**Layer L3.** Persistence of the model: index journal + snapshot, model ↔ sidecar mapping, the debounced sidecar writer, the metadata write queue, rebuild from folders.

- **May depend on:** L0–L2 (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.9, M1.10 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-store` (no window, GPU, network or personal photos needed).
