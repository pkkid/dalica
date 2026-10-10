# dalica-fs

**Layer L0.** Every write to the user's disk: atomic replace + fsync + verify, the read-before-write guard, the library lock, move/copy with verify, the OS trash, bounded walks, the UI-thread guard and fault-injection points (feature `faults`).

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:** none yet; this crate is a stub until M1.2, M1.3 (`docs/roadmap.md`).
- **Test it alone:** `cargo test -p dalica-fs` (no window, GPU, network or personal photos needed).
