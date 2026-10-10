# dalica-engine

**Layer L4.** `Session`: library + store + jobs + settings + undo stacks; the command registry (`cmd/<area>.rs`); keyboard contexts and the keymap; control-protocol types; demo library setup.

- **May depend on:** L0–L3 (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.13, M1.14 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-engine` (no window, GPU, network or personal photos needed).
