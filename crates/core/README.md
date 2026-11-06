# dalica-core

**Layer L0.** Ids (`PhotoId`, `FolderId`, `AlbumId`, `PersonId`), errors, normalised geometry, dates, path and name utilities (collision names `-001`), position keys and the `Clock` trait.

- **May depend on:** nothing (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `serde`, `thiserror`.
- **Public API:**
  - `build`: `VERSION` (the workspace version), `commit()` and `build_date()` from `DALICA_BUILD_SHA` / `DALICA_BUILD_DATE` at compile time, `describe` and `version_string()` (`0.1.0 (3f2a9c1d0, 2026-10-12)`, or `0.1.0 (dev build)` without them); what `--version`, the control channel's `auth` and About report.
  - `app_id`: `APP_ID` (`"org.dalica.Dalica"`, reserved decision 10) and `check_dbus_name` / `check_bundle_id` / `check_reverse_dns` / `check`.
  - `ids`: `PhotoId`, `FolderId` (integers from the index); `AlbumId`, `PersonId` (`a`/`p` + 4–16 lowercase hex digits, checked on parse and deserialize).
  - `geom`: `Rect01` (normalised `[x0, y0, x1, y1]`, six decimals, never empty), `PixelRect`, `to_pixels` / `from_pixels`.
  - `clock`: `Timestamp` (Unix milliseconds, saturating), the `Clock` trait, `SystemClock`, `VirtualClock`.
  - `dates`: `Date`, `DateTime` (local, no zone; ISO and EXIF forms in, ISO out), `DateTime::from_unix_secs`.
  - `names`: `split_ext`, `numbered` (`photo-001.jpg`), `candidates`, `next_free`; names stay within 255 bytes. `sanitize(name, fallback)`: the one sanitizer for names Dalica generates (`security.md` §5.4: separators, `: * ? " < > |`, control and bidirectional characters removed, leading `.`/`-`/spaces and trailing dots/spaces stripped, 255 bytes on a character boundary, device names such as `CON` → `_CON`, the fallback then `Untitled` when nothing is left); `is_reserved`, `is_bidi_control`, `fit` (a prefix of at most n bytes on a character boundary), `dated("Export", date)` (`Export 2026-06-04`).
  - `color`: sRGB and HSV (`rgb_to_hsv`, `hsv_to_rgb`), 8-bit forms and `#rrggbb` (`hex`, `parse_hex`).
  - `color_summary` (`organizing.md` §8.1): `SearchColour` (`red`, `orange`, `yellow`, `green`, `blue`, `purple`, `bw`; `ALL`, `HUES`, `word`, `label`, `from_word` in any case), `ColourSummary` (a set of them with its main colour, `bw` alone: `main`, `contains`, `colours`, `new`, `EMPTY`; `to_byte` / `from_byte`, a byte with an unknown bit refused), `classify(cells)` (a grid of opaque sRGB cells, up to `MAX_CELLS` = 64 × 64, to a summary; empty or oversized gives the empty summary), `hue_colour` and the named thresholds (`HUE_BOUNDS`, `MIN_SATURATION`, `MIN_VALUE`, `BW_SHARE`, `JOIN_SHARE`).
  - `poskey`: `PosKey` with `first`, `after`, `before`, `between`; byte order is album order. The module docs explain the encoding.
  - `Error`: the one error type.
- **Test it alone:** `cargo test -p dalica-core` (no window, GPU, network or personal photos needed). Property tests live in `tests/poskey.rs`, `tests/props.rs`, `tests/sanitize.rs` and `tests/color_summary.rs` (any grid up to 64 × 64 classifies, the same twice) (the sanitizer: never empty, never a path, stable), the version string's in `tests/build.rs`; `cargo test -p dalica-core --test all poskey:: -- --nocapture` prints the key lengths after 10 000 inserts per pattern. The integration tests are one binary (`tests/all.rs`, one module per file: `cargo test -p dalica-core --test all <file>::` runs one); a task's gate runs them quick, `DALICA_FULL_TESTS=1` at full strength (every property case, every size).
