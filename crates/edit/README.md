# dalica-edit

**Layer L1.** The edit recipe: operation syntax parse/print (unknown operations preserved), pipeline order, the CPU renderer for every tool and effect (the oracle), auto-fix analysis.

- **May depend on:** L0 and `image` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M6, M7 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-edit` (no window, GPU, network or personal photos needed).
