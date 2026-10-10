# Supported formats and decode limits

Adapted from LightCraft `crates/codecs/README.md` and `crates/heif/README.md` (revision 88ea1a8) / PhotoCraft `crates/codecs/README.md` (revision 59684266); MIT OR Apache-2.0.

Which files Dalica shows, how it decodes them, what it can write back, and the limits that keep a
hostile or enormous file from crashing the app or exhausting memory. The list of recognised files
and the per-type toggles are in [`data-model.md`](data-model.md) §7 and Options → File Types
([`menus-and-shortcuts.md`](menus-and-shortcuts.md) §5); metadata details are in
[`xmp-interop.md`](xmp-interop.md).

> Status: specified, not built. Decoding lives in our own `crates/image` (`dalica-image`, L1), built
> on permissive pure-Rust crates.io decoders (`image`, `zune-jpeg`, `jxl-oxide`, `heic-rs`) plus our
> own RAW embedded-preview extractor. LightCraft's `codecs` and `raw` crates are prior art only
> (`architecture.md` §9). Everything is pure Rust; no C decoder is linked, ever.

## 1. Formats

| Format | Show (decode) | Thumbnail fast path | Metadata write | Save edits in place | Notes |
|---|---|---|---|---|---|
| JPEG | yes: baseline, progressive, CMYK/YCCK, 12-bit | embedded EXIF thumbnail or MPF preview; DCT-domain 1/2–1/8 scaled decode | XMP, IPTC, EXIF | yes (JPEG; rotate-only Save is lossless where dimensions allow, `data-model.md` §6) | arithmetic coding unsupported |
| PNG | yes: 1–16-bit, palette, transparency, interlaced | scaled decode | XMP (`iTXt`), EXIF (`eXIf`) | PNG without alpha: yes; with alpha: JPEG copy beside it | |
| TIFF | yes: 8/16-bit int, float, gray/RGB/CMYK/palette, LZW/Deflate/PackBits/JPEG; first full-resolution page | embedded thumbnail IFD if present, else scaled decode | XMP and IPTC tags (append-only) | JPEG copy beside it | multi-page: first page; Lab/YCbCr unsupported |
| WebP | yes: lossy and lossless, alpha; first frame of animations | scaled decode | XMP, EXIF chunks | JPEG copy beside it | |
| GIF | first frame | scaled decode | — (sidecar only) | JPEG copy beside it | animation shown as its first frame |
| BMP | yes | scaled decode | — (sidecar only) | JPEG copy beside it | |
| HEIC / HEIF | only with the `heif` feature (§3) | — (the thumbnail item is HEVC too) | — (sidecar only) | JPEG copy beside it | iPhone/Mac photos: grids, 8–12-bit, alpha, ICC, EXIF, XMP |
| AVIF | **no**: recognised and listed, shown as a format tile | — | — (sidecar only) | — | no permissively licensed pure-Rust AV1 decoder exists today |
| JPEG XL | yes (`jxl-oxide`) | scaled decode | — (sidecar only) | JPEG copy beside it | orientation applied by the decoder |
| PSD / PSB | merged composite only (8/16/32-bit gray, RGB, CMYK, indexed) | embedded thumbnail resource or composite | — (sidecar only) | JPEG copy beside it | layers are not read; ZIP-compressed composites and Lab unsupported |
| RAW | **embedded preview only** (§2) | embedded preview | `.xmp` sidecar when enabled | JPEG copy beside it | RAW development is out of scope (`product.md`) |
| Video (MP4, MOV, M4V, MKV) | poster frame when decodable (M15), otherwise a generic video tile | — | — (sidecar only) | — | |

"JPEG copy beside it": Save of a format Dalica cannot write in place writes `name.jpg` (or
`name-001.jpg`) next to the source and leaves the source untouched (`data-model.md` §6). Export
writes JPEG (PNG for sources with transparency), as in [`creating.md`](creating.md).

A file the decoder refuses (unsupported variant, damaged, over a limit) is still listed: its cell
shows the embedded thumbnail if one exists, otherwise a `caution` "!" with the reason in the tooltip
("HEIC support isn't included in this build", "Too large to display (W × H)", "Damaged file").
Damaged and truncated files decode as far as possible and say so; they never take the app down.

## 2. RAW files

- **Recognised:** TIFF-structured raws (DNG, CR2, NEF, ARW, PEF, ORF, RW2, SRW, …)
  and the other containers our extractor probes (CR3, RAF, CRW, MRW, X3F). Detection is by content, not by
  extension.
- **Shown** through the largest embedded JPEG preview (found by our own bounded TIFF/IFD walk), which is what the
  camera rendered; previews tagged with a colour space (for example Adobe RGB on some cameras) are
  converted to sRGB. A raw file with no usable preview shows a generic RAW tile.
- **Edits** apply to the embedded preview; Save writes a JPEG beside the raw at the preview's size.
  Full raw development is out of scope: **Open With** sends the file to a raw developer.
- **Metadata** is read from the raw's EXIF/XMP and an `.xmp` sidecar, and written only to the `.xmp`
  sidecar when enabled ([`xmp-interop.md`](xmp-interop.md) §7). RAW+JPEG pairs are two photos that
  share a stem.

## 3. HEIF / HEIC: an optional feature

- Decoding uses `heic-rs` (pure Rust, no `unsafe`, MIT OR Apache-2.0) through a thin, panic-guarded
  wrapper of our own (prior art: LightCraft's `heif` crate): single pictures and grid-tiled photos, 8 to 12 bits (10/12-bit
  decode to 16-bit), auxiliary alpha, the container's rotation/mirror/crop, ICC, EXIF and XMP. Image
  sequences, overlays and some 4:2:2/4:4:4 streams return "unsupported", never wrong pixels.
- It sits behind the **non-default** `heif` cargo feature of `dalica-image`, forwarded by the app
  (`cargo build -p dalica --features heif`). HEVC is patent-encumbered in some jurisdictions, so
  including a decoder is the distributor's decision; whether official Dalica builds enable it is
  decided with the release pipeline (M6). Without the feature, HEIC files are still recognised and
  listed, with the tile "HEIC support isn't included in this build".
- The decoder is young and pinned exactly; moving the pin is a reviewed change that re-runs the
  `heif` corpus tests (`development.md` §11).
- HEIF is read-only: writing needs an HEVC encoder, and every mature one is C.

## 4. Colour and orientation

- Every decode ends in 8-bit sRGB for display and in the edit renderer's working format. Embedded ICC
  profiles are applied with `moxcms` (v2/v4; sRGB, Display P3, Adobe RGB, ProPhoto, Rec.2020
  recognised); untagged images are assumed sRGB; an unusable profile falls back to sRGB and is
  logged; CMYK without a profile uses a naive conversion.
- EXIF orientation (and HEIF `irot`/`imir`) is applied for display; Dalica's own `rotate(…)` and
  flip operations in the recipe are applied on top. Files are never rewritten just to fix orientation.

## 5. Decode limits

File-derived numbers are hostile (AGENTS.md §3). Every decode goes through one `Limits` value that
`dalica-image` passes to the codecs, checked against the **header** before any pixel buffer is
allocated (and again after orientation swaps width and height):

| Limit | Value | Why |
|---|---|---|
| Width or height | ≤ 65,535 px (JPEG's own maximum) for full decodes; an absolute ceiling of 262,144 px applies to header reads | no real photo is larger; panoramas up to the JPEG maximum still open |
| Pixels for a full decode | ≤ 2^28 (268 MP) | covers stitched panoramas and medium-format files |
| Allocation per decode | ≤ min(the codec default, half the memory budget) — the budget is RAM/4 clamped 256 MiB–1.5 GiB (`architecture.md` §5) | a single file can never take the whole budget |
| Source pixels for a thumbnail from a decoder that cannot scale | ≤ 64 MP (the same default as LightCraft) | non-JPEG decoders allocate at full size before resizing |
| PNG text and XMP chunks | bounded by the allocation limit | |

Whichever limit is reached first applies; for large images it is usually the allocation limit.

- JPEGs decode at the smallest DCT scale that covers the target (1/2 to 1/8), and embedded previews
  are used whenever they cover the target, so even very large JPEGs and raws show thumbnails without
  a full decode.
- A file over a limit returns a "too large" error (shown on its tile); its embedded preview, if any,
  is still used. Allocations are fallible (`try_reserve`), so an allocation failure is an error, not
  an abort.
- Save and Export of very large images render in bands where the operations allow it; otherwise they
  fail with a clear message before starting rather than exhausting memory.
- Panics inside third-party decoders are caught (`catch_unwind`) and reported as "damaged file"; the
  regression input goes into the malformed-input tests as a synthetic reproduction.
- Container walks are bounded too: TIFF IFD chains (seen-set, depth limit, directory cap), XMP
  nesting depth and node count, EXIF entry counts, ISOBMFF box nesting.

## 6. Small images

Images smaller than 64 × 64 are indexed but hidden unless View → Small Pictures is on
(`data-model.md` §7); the threshold is in Options → File Types.

## 7. Tests

- Round trips for every format Dalica writes (JPEG, PNG) and metadata preservation per container
  ([`xmp-interop.md`](xmp-interop.md) §8).
- Magic-number detection, including negatives (a `.jpg` that is really a PNG, a truncated header).
- Orientation 1–8 for every container that carries it.
- Limits: header-declared bombs are refused before allocating; images exactly at each limit decode.
- Malformed input: truncation at every offset, bit flips, proptest random bytes; never a panic.
- Corpora behind the `corpus` feature (`development.md` §11): camera JPEGs, raw previews per vendor,
  HEIF (with the `heif` feature), PngSuite; decode and preview-extraction floors only rise.
