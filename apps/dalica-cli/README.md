# dalica-cli

Dalica's headless command line (`docs/architecture.md` §3, §7). Apps may depend on any crate.

- **Commands (M1.15):**
  - `commands`: the engine's command registry as JSON (id, label, menu, keys, params doc, effect, history). `cargo xtask parity` reads it.
  - `run --library DIR [SCRIPT | -]`: a JSON-lines script, `{"command": id, "params": {...}}` per line (blank and `#` lines skipped), one JSON result line per command (`{"ok": true, "result": …}` or `{"ok": false, "code": …, "error": …}`); background scans finish before the next line; everything is flushed before exit. `run --library DIR --cmd ID [--params JSON]` runs one command.
  - `inspect --library DIR`: the library as JSON (watched folders, folders with photo and star counts, starred photos, repairs, notices, scan warnings).
  - Exit codes: 0 success, 1 a command failed, 2 a bad argument or script line, 3 the library could not be opened (for example, another Dalica process has it open).
- **Test it alone:** `cargo test -p dalica-cli`; the M1 headless acceptance (`cargo xtask accept M1 --headless`) drives it end to end.
