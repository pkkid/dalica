# dalica-image

**Layer L1.** Sniffing, scaled decoding, encoding, orientation, ICC → sRGB, decode limits, embedded thumbnails and RAW previews (a bounded TIFF/IFD walker of our own).

- **May depend on:** `core` (`docs/architecture.md` §2; `cargo xtask layers` enforces it). External: `zune-jpeg`, `png`, `moxcms`, `log`, `thiserror`.
- **Public API (M1.5):**
  - `sniff(bytes) -> Option<Format>`: the format from magic numbers, never the file name (JPEG, PNG, TIFF/RAW, WebP, GIF, BMP, HEIF, AVIF, JPEG XL, PSD; `Format::supported()` is JPEG and PNG in M1).
  - `read_header(bytes, &Limits)` / `read_header_file(path, &Limits)` → `Header { format, width, height, orientation, exif, exif_error, thumbnail }`: size from the JPEG frame header or PNG IHDR, EXIF from JPEG APP1 or PNG `eXIf`, without decoding. A bad EXIF block leaves the photo upright as stored, with the reason in `exif_error`. `Header::thumbnail(file)` gives the embedded JPEG thumbnail's bytes.
  - `ifd`: the bounded TIFF/IFD reader (`Tiff`, `Ifd`, `Entry`, `Visited`): ranges checked before reading, loop detection, caps on IFDs, depth and entries. `exif::read`: orientation, DateTimeOriginal, DateTimeDigitized, make, model, the IFD1 thumbnail range (strict: any loop or bad offset is an `Err`).
  - `orient`: `Orientation` (1–8, `display_size`), `apply(Rgba8, Orientation)`.
  - `Limits` (`security.md` §4.2), `Rgba8`, `Error`.
- **Public API (M1.6):**
  - `decode(bytes, &Limits)` → upright `Rgba8` (sRGB assumed until M5); `decode_thumbnail(bytes, long_side, &Limits)` decodes and fits (no DCT scaling in `zune-jpeg` 0.5), and past 64 MP uses the embedded EXIF thumbnail or refuses. The declared size is checked before any decoder is created.
  - `resize::fit` / `resize::resize`: exact area average in integer arithmetic, identical on every CPU.
- **Public API (M5.1):**
  - `formats/`: one module per format (`jpeg`, `png`, `tiff`, `webp`, `gif`, `bmp`, `jxl`, `psd`, `heif`, `bmff`, and `raw/` with `tiff_based`, `cr3`, `raf`, `crw`, `mrw`, `x3f`), each with `header`, `decode`, `embedded_thumbnail`, `DECODES` and `REASON` (the contract is in `formats/mod.rs`); `formats::{header, decoder, embedded_thumbnail, decodes, reason}` dispatch by `Format`. JPEG and PNG decode; the rest are stubs answering `Unsupported` until M5.6–M5.10 fill them.
  - `Format::Raw(RawKind)`, `Format::ALL`, `Format::name()`; `sniff` tells the RAW containers apart by content (each `raw/*` module's `recognise`), TIFF-structured raws by a bounded IFD walk when the bytes hold IFD0.
  - `Decoded` (8/16-bit or float samples; `ColorModel` grey, RGB or CMYK ink; alpha; ICC bytes; orientation), `DecodeOptions { color_managed }`, `decode_with`, `decode_thumbnail_with`, `decode_as(format, …)` (no sniffing).
  - `color`: the one colour stage to 8-bit sRGB (`to_srgb8`, `load_profile`, `ColorPath`): ICC v2/v4 through `moxcms`, untagged = sRGB, an unusable profile logged and shown as sRGB, CMYK without a profile converted naively.
  - `Budget` (every pixel buffer charged to `max_alloc` and reserved fallibly), `Work` (step counters on our container walks), `Limits::{max_icc, max_icc_tags}`; `Error::Panic` (a third-party decoder's panic, caught) and `Error::reason()` (the tile text).
- **Public API (M5.10):**
  - `formats::bmff`: the bounded ISOBMFF box walk (`Boxes`, `BoxRef`, `child`, `path`; nesting ≤ 12, ≤ 4,096 boxes per level, every box counted) and the `ftyp` brands.
  - `formats::raw::{cr3, raf, crw, mrw, x3f}` show the largest embedded JPEG and offer the next smaller one as the thumbnail; a raw without one answers "This RAW file has no preview Dalica can show".
  - Feature `heif` (off by default; `dalica` forwards it): HEIF/HEIC through `heic-rs` (pinned `=0.1.1`) in `formats::heif`, upright, 8-bit or 16-bit samples with alpha and ICC. Without it HEIC answers "HEIC support isn't included in this build". AVIF (also behind a `mif1` major brand) answers "AVIF files can't be displayed".
- **Test it alone:** `cargo test -p dalica-image` (no window, GPU, network or personal photos needed); `cargo test -p dalica-image --features heif --test heif` for HEIF. Test photos come from `dalica-scenes`; truncation at every byte offset and random corruption (proptest) never panic. `tests/decode.rs` compares decodes by fingerprint (8 × 8 grid of per-channel mean and deviation) and writes actual/expected PNGs to `target/test-output/image/` on a mismatch. `tests/color.rs` builds its own tagged files (v4 profiles from `moxcms`, v2 ones from a small writer, CMYK/YCCK JPEGs from a DC-only encoder) and keeps `fingerprints/color-*.json` (`DALICA_BLESS=1` rewrites them). Dev builds compile this crate and `zune-jpeg` at `opt-level = 3`.
- **Fuzz it:** `crates/image/fuzz` is a separate cargo-fuzz workspace (nightly, excluded from the root workspace) with one target per parser of `security.md` §7.1: `sniff`, `decode_any`, `decode_<format>` for every format module, `raw_preview`, `embedded_thumbnail`, `icc`. `cd crates/image/fuzz && cargo +nightly fuzz run decode_jpeg`; the Fuzz workflow builds them on pull requests that touch this crate and runs them all when dispatched by hand.
