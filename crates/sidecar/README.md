# dalica-sidecar

**Layer L1.** The `.dalica.json` folder sidecar: parse and print with unknown keys preserved; the read-only legacy reader and its conversion to Dalica's model.

- **May depend on:** L0 (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `serde_json` (with `float_roundtrip`), `thiserror`.
- **Public API (v1, M1.7):**
  - `Sidecar::parse(bytes, &Limits)` (BOM and CRLF accepted, a duplicate key takes the later value, the limits of `security.md` §4.2) → `Sidecar` or `ParseError` (the caller sets the file aside). `print()`: keys sorted by bytes at every level, two spaces, LF, final newline; `parse(print(x)) == x`.
  - `star(name) -> Field<bool>` (`Absent`, `Value`, `WrongType(value)`), `set_star(name, on)` (clearing removes the key, and the entry when it is empty; refuses to write over a wrong-type `photos` or entry), `photo_names()`, `version()`, `problems()` (wrong-type values as JSON pointers).
  - `plain_name(name)`: whether a key read from a sidecar may reach a file (`security.md` §5.4).
  - Everything not modelled is kept verbatim.
- **Test it alone:** `cargo test -p dalica-sidecar` (no window, GPU, network or personal photos needed): property tests for `parse(print(x)) == x` on random documents and any file name as a key.
