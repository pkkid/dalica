# dalica-fs

**Layer L0.** Every write to the user's disk: atomic replace + fsync + verify, the read-before-write guard, the library lock, move/copy with verify, the OS trash, bounded walks, the UI-thread guard and fault-injection points (feature `faults`).

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `thiserror`.
- **Public API:**
  - `atomic`: `read(path, limit)` → bytes + `Identity`; `replace(path, bytes, Expect::Absent | Expect::Unchanged(id))` → new `Identity` (temp file next to the target, fsync, identity re-check, rename, folder fsync, read-back); `identity`, `is_temp_name`. Final symbolic links are refused.
  - `append`: `Appender` / `append(path, bytes, cap)` (append-only, fsync per record, never past the cap) and `rotate(path, keep)` (`name.log` → `name.1.log` → `name.2.log`).
  - `guard`: `mark_ui_thread()` returns a guard; while it lives, every call on that thread returns `Err(OnUiThread)` in debug and test builds.
  - `faults` (feature `faults`): named points (`faults::points`), `arm` (this thread), `arm_global` (any thread), `disarm_all`; `Fault::Error` fails a step, `Fault::Crash` stops without cleanup.
  - `Error` (incl. `Locked` for the library lock, M1.3).
- **Test it alone:** `cargo test -p dalica-fs` (no window, GPU, network or personal photos needed). Tests always build with `faults` (self dev-dependency), so the fault loops run in plain `cargo test` and `cargo xtask ci`.
