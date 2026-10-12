# dalica-sidecar

**Layer L1.** The `.dalica.json` folder sidecar: parse and print with unknown keys preserved; the read-only legacy reader and its conversion to Dalica's model.

- **May depend on:** L0 (`docs/architecture.md` §2; `cargo xtask layers` enforces it): `dalica-core` for dates. External: `serde_json` (with `float_roundtrip`), `thiserror`.
- **Public API (v1, M1.7):**
  - `Sidecar::parse(bytes, &Limits)` (BOM and CRLF accepted, a duplicate key takes the later value, the limits of `security.md` §4.2) → `Sidecar` or `ParseError` (the caller sets the file aside). `print()`: keys sorted by bytes at every level, two spaces, LF, final newline; `parse(print(x)) == x`.
  - `star(name) -> Field<bool>` (`Absent`, `Value`, `WrongType(value)`), `set_star(name, on)` (clearing removes the key, and the entry when it is empty; refuses to write over a wrong-type `photos` or entry), `photo_names()`, `version()`, `problems()` (wrong-type values as JSON pointers).
  - `plain_name(name)`: whether a key read from a sidecar may reach a file (`security.md` §5.4).
  - Everything not modelled is kept verbatim.
- **Public API (M6.8, `fields`):** every M6 key of `data-model.md` §3.
  - `photo(name) -> PhotoFields` (`star`, `caption`, `tags`, `hidden`, `edits`, `redo`, `date` as `Field`s; a date that does not parse is `WrongType`; the EXIF spelling is read) and `folder() -> FolderFields` (`description`, `date`, `place`, `hidden` under `"folder"`).
  - `set_photo(name, &PhotoValue)` and `set_folder(&FolderValue)`: one key at a time; the empty value (`false`, `""`, `[]`, `None`) removes the key, then the photo's entry or the `"folder"` object when it is left empty, and never creates anything; refused (`WrongType`) when `photos`, the entry or `"folder"` is not an object. `star`/`set_star` are the same for `star`.
  - `problems()` covers every M6 key; `fields::PHOTO_KEYS` and `fields::FOLDER_KEYS` name the keys read (the store's mapping test checks them against its tables).
  - No caps here: the store's `mapping` decides what the model takes (`dalica-store`).
- **Test it alone:** `cargo test -p dalica-sidecar` (no window, GPU, network or personal photos needed): property tests for `parse(print(x)) == x` on random documents and any file name as a key, and for sidecars holding every M6 key with values of any type (reading never panics, a value set reads back exactly and every other key keeps its value).
