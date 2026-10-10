# dalica-engine

**Layer L4.** `Session`: library + store + jobs + settings + undo stacks; the command registry (`cmd/<area>.rs`); keyboard contexts and the keymap; control-protocol types; demo library setup.

- **May depend on:** L0–L3 (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API (M1.13):**
  - `Session::open(library_dir, clock)` (takes the library lock — a second session gets `OpenError::Locked` — loads the index, reporting repairs in `recovery()`, reads `folders.json`, starts the I/O worker). After opening, the owning thread never touches the disk: journal appends, debounced sidecar writes, snapshots and `folders.json` go to the I/O worker; scans run on their own threads.
  - `execute(id, params)`: `enabled` check, typed params (a JSON object; `bad_params` otherwise), `catch_unwind` (`internal`), exactly one library undo step per command that changed something. `poll()` (each UI frame) and `wait_idle()` (CLI, tests) apply finished scans; `flush()` waits until every pending write has landed.
  - Commands: `folders.add`, `library.query`, `library.rebuild`, `photo.inspect`, `photo.star.toggle`, `photo.star.set`, `edit.undo`, `edit.redo`. `commands()` lists them as `engine.commands` does.
  - `CommandSpec`, `Context`, `Effect`, `History`, `CmdError` (with the control protocol's codes), `UndoStack` (cap 1 000).
- **Test it alone:** `cargo test -p dalica-engine` (no window, GPU, network or personal photos needed): every command on a temporary library of `scenes` photos (behaviour, undo/redo restore exactly, disabled cases, hostile params give errors and change nothing), and session tests (star 3, undo 1, flush → 2 in the sidecar; a second session is locked out; panics are caught; no file I/O on the owning thread).
