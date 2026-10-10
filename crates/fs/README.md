# dalica-fs

**Layer L0.** Every write to the user's disk: atomic replace + fsync + verify, the read-before-write guard, the library lock, move/copy with verify, the OS trash, bounded walks, the UI-thread guard and fault-injection points (feature `faults`).

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `thiserror`. The lock uses std's `File::try_lock`.
- **Public API:**
  - `atomic`: `read(path, limit)` → bytes + `Identity`; `replace(path, bytes, Expect::Absent | Expect::Unchanged(id))` → new `Identity` (temp file next to the target, fsync, identity re-check, rename, folder fsync, read-back); `identity`, `is_temp_name`. Final symbolic links are refused.
  - `append`: `Appender` / `append(path, bytes, cap)` (append-only, fsync per record, never past the cap) and `rotate(path, keep)` (`name.log` → `name.1.log` → `name.2.log`).
  - `dirs`: `create_dir_all`.
  - `guard`: `mark_ui_thread()` returns a guard; while it lives, every call on that thread returns `Err(OnUiThread)` in debug and test builds.
  - `faults` (feature `faults`): named points (`faults::points`), `arm` (this thread), `arm_global` (any thread), `disarm_all`; `Fault::Error` fails a step, `Fault::Crash` stops without cleanup.
  - `lock`: `LibraryLock::acquire(dir)` takes `<dir>/lock` (an OS file lock, released however the process exits) or returns `Err(Locked)` at once.
  - `walk`: `walk(root, Limits, skip)` → `Walk { entries, warnings }`: depth 32, 1 000 000 entries, folder links followed after the real tree with (device, inode) loop detection, children sorted by name, results in pre-order; unreadable parts, limits, loops and cancellation become warnings.
  - `Error`.
- **Test it alone:** `cargo test -p dalica-fs` (no window, GPU, network or personal photos needed). Tests always build with `faults` (self dev-dependency), so the fault loops run in plain `cargo test` and `cargo xtask ci`.
