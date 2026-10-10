# dalica-image

**Layer L1.** Sniffing, scaled decoding, encoding, orientation, ICC → sRGB, decode limits, embedded thumbnails and RAW previews (a bounded TIFF/IFD walker of our own).

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `zune-jpeg`, `png`, `thiserror`.
- **Public API (M1.5):**
  - `sniff(bytes) -> Option<Format>`: the format from magic numbers, never the file name (JPEG, PNG, TIFF/RAW, WebP, GIF, BMP, HEIF, AVIF, JPEG XL, PSD; `Format::supported()` is JPEG and PNG in M1).
  - `read_header(bytes, &Limits)` / `read_header_file(path, &Limits)` → `Header { format, width, height, orientation, exif, exif_error, thumbnail }`: size from the JPEG frame header or PNG IHDR, EXIF from JPEG APP1 or PNG `eXIf`, without decoding. A bad EXIF block leaves the photo upright as stored, with the reason in `exif_error`. `Header::thumbnail(file)` gives the embedded JPEG thumbnail's bytes.
  - `ifd`: the bounded TIFF/IFD reader (`Tiff`, `Ifd`, `Entry`, `Visited`): ranges checked before reading, loop detection, caps on IFDs, depth and entries. `exif::read`: orientation, DateTimeOriginal, DateTimeDigitized, make, model, the IFD1 thumbnail range (strict: any loop or bad offset is an `Err`).
  - `orient`: `Orientation` (1–8, `display_size`), `apply(Rgba8, Orientation)`.
  - `Limits` (`security.md` §4.2), `Rgba8`, `Error`.
- **Public API (M1.6):**
  - `decode(bytes, &Limits)` → upright `Rgba8` (sRGB assumed until M8); `decode_thumbnail(bytes, long_side, &Limits)` decodes and fits (no DCT scaling in `zune-jpeg` 0.5), and past 64 MP uses the embedded EXIF thumbnail or refuses. The declared size is checked before any decoder is created.
  - `resize::fit` / `resize::resize`: exact area average in integer arithmetic, identical on every CPU.
- **Test it alone:** `cargo test -p dalica-image` (no window, GPU, network or personal photos needed). Test photos come from `dalica-scenes`; truncation at every byte offset and random corruption (proptest) never panic. `tests/decode.rs` compares decodes by fingerprint (8 × 8 grid of per-channel mean and deviation) and writes actual/expected PNGs to `target/test-output/image/` on a mismatch. Dev builds compile this crate and `zune-jpeg` at `opt-level = 3`.
