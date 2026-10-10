# Standard: never crash

Adapted from the "Never crash" sections of LightCraft `AGENTS.md` (revision 88ea1a8) and PhotoCraft
`AGENTS.md` (revision 59684266); MIT OR Apache-2.0. The Craft apps keep the full standard in a
private shared repository; this is Dalica's own copy, and it outranks feature work.

People trust Dalica with their photo libraries; a crash can lose their work (an unsaved recipe, a
half-written sidecar, an interrupted move). A malformed JPEG, RAW, EXIF block, XMP packet or
sidecar, a bad command, control-channel or MCP argument, a corrupt index or settings file, an odd
keystroke, a vanished network share or a full disk must produce an error the user (or agent) can
act on — never a panic. Don't ship a feature by adding a panic path, and fix a crash before
building on top of it.

## Rules

1. **Non-test code never panics.** No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
   `unimplemented!`. Return the crate's error type through `Result` and `?`; use `ok_or(..)?`,
   `let … else { return Err(..) }`, `if let`, or a fallback (`unwrap_or…`) only where it cannot
   silently corrupt data. An unfinished feature returns an "unsupported" error or is disabled.
   Sole exception: a provably infallible literal, written as `#[allow(clippy::expect_used)]` +
   `.expect("why it can't fail")`.
2. **No `unsafe`.** Every crate root has `#![forbid(unsafe_code)]`. If a platform need ever
   requires `unsafe` (an FFI call the safe ecosystem lacks), it goes into one isolated, well-tested
   helper crate: `// SAFETY:` on every block, a safe API, a safe fallback where possible, crate-level
   `unsafe_code = "deny"` with a per-module allow, and a line in `AGENTS.md` naming the crate.
3. **Input-derived numbers are hostile.** Use `get()` instead of `[i]` / `[a..b]` for offsets from
   files, users, agents or arithmetic on them; use `checked_*` / `saturating_*` for lengths, offsets
   and counts; guard against division by zero, NaN/infinity and negative casts to `usize`; cap
   allocations sized by input (image dimensions, IFD counts, XMP sizes, sidecar line lengths); slice
   strings only at char boundaries (file names are often non-ASCII).
4. **Bound recursion and walks.** Depth limits or seen-sets for IFD chains, nested metadata, XMP
   trees, folder walks (depth 32, 1,000,000 entries, symlink loops) and album/person graphs.
5. **Don't cascade.** Handle lock poisoning (`lock().unwrap_or_else(PoisonError::into_inner)`) or
   return an error; treat worker-thread joins and channel sends/receives as `Result`s.
6. **Last-resort guard.** A panic hook plus `catch_unwind` around command dispatch and around each
   per-file unit of scan/import/export work turns an escaped panic into an error toast (and a log
   entry) and keeps the app and the library running. It is a safety net, not a licence. Keep
   `panic = "unwind"` in native profiles.
7. **Prove it.** Every crash fix lands with a small synthetic regression test that panicked before
   the fix. Commands must survive hostile params: the `panic_hunt` integration test runs every
   registered command with adversarial parameter sets (empty, wrong types, out of range, huge,
   NaN) under `catch_unwind` with a hang timeout; it must stay green. New commands come with a
   graceful-failure test.

## Enforcement

- `clippy.toml` (repository root) allows `unwrap` / `expect` / `panic` / indexing **in tests only**.
- Every production crate root (`lib.rs`, each binary's `main.rs`) carries:

  ```rust
  #![forbid(unsafe_code)]
  #![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
  ```

  Not `[workspace.lints]`: those would also hit integration tests, examples and benches. New crates
  start with the attribute.
- `cargo xtask ci` runs clippy with `-D warnings`; `panic_hunt` runs in the pre-commit checklist when
  commands change, and in CI.
- Fuzz targets (`cargo fuzz`) for every parser of untrusted input: image headers, EXIF/TIFF IFDs,
  XMP, IPTC, the folder sidecar, the legacy sidecar reader (see `docs/security.md`).
