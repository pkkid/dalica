# Supported formats and decode limits

Adapted from LightCraft `crates/codecs/README.md` and `crates/heif/README.md` (revision 88ea1a8) / PhotoCraft `crates/codecs/README.md` (revision 59684266); MIT OR Apache-2.0.

Which files Dalica shows, how it decodes them, what it can write back, and the limits that keep a
hostile or enormous file from crashing the app or exhausting memory. The list of recognised files
and the per-type toggles are in [`data-model.md`](data-model.md) §7 and Options → File Types
([`menus-and-shortcuts.md`](menus-and-shortcuts.md) §5); metadata details are in
[`xmp-interop.md`](xmp-interop.md).

> Status: built for every format of §1 except video (M15). Decoding lives in our own
> `crates/image` (`dalica-image`, L1), built on permissive pure-Rust crates.io decoders
> (`zune-jpeg`, `png`, `tiff`, `image-webp`, `image`'s GIF and BMP decoders, `jxl-oxide`, `heic-rs`)
> plus our own readers (container walks, PSD composites, RAW embedded previews), with one module
> per format under `crates/image/src/formats/`, all ending in the colour stage of §4: JPEG and PNG
> (M5.1), TIFF (M5.6), WebP, GIF and BMP (M5.7), JPEG XL and PSD composites (M5.8), previews of
> TIFF-structured raws (M5.9), previews of CR3, RAF, CRW, MRW and X3F, and HEIF behind its feature
> (M5.10); AVIF is recognised and never decoded. Release builds carry no HEIC decoder (the M10.R
> answer, §3): a HEIC is listed, sorted and searched by the date and size read from its container,
> and shows the unsupported tile with a hint (M10.56). Later rows, not part of 0.5 (the M10.R answer):
> 12-bit JPEG (`DATA-FMT-JPEG-12BIT`: no pure-Rust decoder we could use decodes 12-bit DCT
> samples; such files are listed with their size and show the unsupported tile, "This kind of
> JPEG isn't supported", measured in M10.23), and JPEG DCT-domain scaled decoding and MPF previews
> (`DATA-FMT-JPEG-FAST`: `zune-jpeg` 0.5 has no scaling, so thumbnails decode in full and are
> fitted, and past 64 MP the embedded thumbnail is used). LightCraft's `codecs` and `raw` crates are prior art only
> (`architecture.md` §9). Everything is pure Rust; no C decoder is linked, ever.
>
> Measured on real files (M5.15, `development.md` §11): PngSuite decodes all 162 valid files and
> refuses all 14 corrupt ones. All 26 camera raws (two per kind) show their preview, with their
> header read from the first bytes the way scanning reads it (M6.38 fixed the six that did not:
> both MRW files, both Samsung SRW files, the Sony DSLR-A100 ARW and the Olympus C5060WZ ORF, and
> the Olympus E-3 ORF's header read). Our own `photos` set (57 CC0 and public-domain photos from 18
> camera and phone makes: JPEG orientations, EXIF thumbnails, MPF, Display P3 and Adobe RGB, CMYK
> JPEG and TIFF, progressive and truncated JPEGs, 16-bit TIFF and PNG, WebP) and `heif` set (3
> iPhone HEIC photos) are published (M6.42): every file is sniffed, read and decoded, 41 photos
> show their embedded thumbnail (the other 16 carry none).

## 1. Formats

| Format | Show (decode) | Thumbnail fast path | Metadata write | Save edits in place | Notes |
|---|---|---|---|---|---|
| JPEG | yes: baseline, progressive, CMYK/YCCK, 8-bit samples | embedded EXIF thumbnail; the full decode fitted (MPF previews and DCT-domain 1/2–1/8 scaled decoding: a later row, see the status above) | XMP, IPTC, EXIF | yes, as a baseline JPEG at the original's quality and chroma subsampling (4:2:2 written as 4:4:4), its metadata kept; progressive, CMYK and YCCK files too. A rotate-only Save turns a baseline file whose size is whole MCUs in its coefficients, losslessly; other sizes and progressive files keep their pixel data and take the EXIF orientation flag (`data-model.md` §6) | arithmetic coding, lossless, hierarchical and 12-bit frames unsupported: the unsupported tile, "This kind of JPEG isn't supported" (12-bit decoding is a later row) |
| PNG | yes: 1–16-bit, palette, transparency, interlaced | scaled decode | XMP (`iTXt`), EXIF (`eXIf`) | without transparency: yes, as PNG (8- or 16-bit; an alpha channel that is all opaque is left out). A 16-bit PNG stays 16-bit: exact for a rotate-only Save, other edits rendered at 8 bits and widened back. With transparency (an alpha sample below opaque): JPEG copy beside it | |
| TIFF | yes: 8/16-bit int, float, gray/RGB/CMYK/palette, LZW/Deflate/PackBits/JPEG; first full-resolution page | embedded thumbnail IFD if present, else scaled decode | XMP and IPTC tags (append-only) | JPEG copy beside it | multi-page: first page; Lab/YCbCr unsupported; a file cut inside a strip or tile is "Damaged file" |
| WebP | yes: lossy and lossless, alpha; first frame of animations | scaled decode | XMP, EXIF chunks | JPEG copy beside it | |
| GIF | first frame | scaled decode | — (sidecar only) | JPEG copy beside it | animation shown as its first frame |
| BMP | yes | scaled decode | — (sidecar only) | JPEG copy beside it | |
| HEIC / HEIF | only with the `heif` feature, which release builds leave out (§3); without it the size, orientation and EXIF are read from the container and the tile shows the reason and a hint | — (the thumbnail item is HEVC too) | — (sidecar only) | JPEG copy beside it (with the `heif` feature; without it Save is disabled with the tile's text) | iPhone/Mac photos: grids, 8–12-bit, alpha, ICC, EXIF, XMP (XMP only when decoded with the feature) |
| AVIF | **no**: recognised and listed, shown as a format tile | — | — (sidecar only) | — (Save is disabled with the tile's text, or leaves the photo out with it beside photos it can save; nothing is written) | no permissively licensed pure-Rust AV1 decoder exists today |
| JPEG XL | yes (`jxl-oxide`) | scaled decode | — (sidecar only) | JPEG copy beside it | orientation applied by the decoder |
| PSD / PSB | merged composite only (8/16/32-bit gray, RGB, CMYK, indexed) | embedded thumbnail resource or composite | — (sidecar only) | JPEG copy beside it | layers are not read; ZIP-compressed composites and Lab unsupported |
| RAW | **embedded preview only** (§2) | embedded preview | `.xmp` sidecar when enabled | JPEG copy beside it, rendered from the embedded preview | RAW development is out of scope (`product.md`) |
| Video (MP4, MOV, M4V, MKV) | poster frame when decodable (M15), otherwise a generic video tile | — | — (sidecar only) | — (videos are not listed before M15, so Save never reaches one) | |

"JPEG copy beside it": Save of a format Dalica cannot write in place writes `name.jpg` (or
`name-001.jpg`, the next free number) next to the source and leaves the source byte-identical
(`data-model.md` §6): the copy is a new, unedited photo with the source's caption, tags and star,
transparency drawn over white; the source keeps its edits and records the copy, and Undo Save and
Revert apply only to saves in place. Export writes JPEG (PNG for sources with transparency), as in
[`creating.md`](creating.md). The "Save edits in place" column is measured, one case per row,
through `photo.save` and Undo Save by `crates/engine/tests/save_formats.rs` (the HEIC case with
`cargo test -p dalica-engine --features heif --test save_formats`).

A file the decoder refuses (unsupported variant, damaged, over a limit) is still listed: its cell
shows the embedded thumbnail if one exists, otherwise a `caution` "!" with the reason in the tooltip
("HEIC support isn't included in this build", "Too large to display (W × H)", "Damaged file") and,
for a format that has one, a hint on the next line saying what to do (§3); the edit view shows the
same under "This photo cannot be shown.".
Damaged and truncated files decode as far as possible and say so; they never take the app down.

Each format is one module with the same entry points: a **header** read for scanning (size,
orientation, EXIF, where the embedded thumbnail lies), a **decode** to the decoded-image model of
§4, and the **embedded thumbnail** for the fast path. The module also states whether this build
decodes the format and the tile text it shows when it does not (and, for HEIC without the feature,
the hint under it). The texts: "Damaged file" for a
truncated, malformed or looping file and for a decoder that failed on it; "Too large to display
(W × H)" past a limit of §5; the format's own text for one Dalica recognises but does not show
("AVIF files can't be displayed", "HEIC support isn't included in this build", "This kind of HEIC
file can't be displayed" for an image sequence, overlay or 4:2:2/4:4:4 stream, "This RAW file has
no preview Dalica can show" for a raw without a usable embedded JPEG, and "… aren't supported yet"
while a format waits for its task).

## 2. RAW files

- **Recognised:** TIFF-structured raws (DNG, CR2, NEF, ARW, PEF, ORF, RW2, SRW, …)
  and the other containers our extractor probes (CR3, RAF, CRW, MRW, X3F). Detection is by content, not by
  extension: CR3 by its `ftyp` brand, RAF, CRW, MRW and X3F by their signatures, CR2, ORF and RW2
  by their variants of the TIFF header, and the other TIFF-structured raws by a bounded walk of
  IFD0's chain and its sub-IFDs that finds a DNGVersion or DNGPrivateData tag or raw sensor data
  (CFA or LinearRaw photometric interpretation, or the CFA pattern tags of TIFF/EP). A TIFF
  without any of these is a TIFF; one whose IFD0 has no page but points at sub-IFDs past the
  bytes read so far is read further before it is called damaged.
- **Shown** through the largest embedded JPEG preview (found by our own bounded TIFF/IFD walk), which is what the
  camera rendered; previews tagged with a colour space (for example Adobe RGB on some cameras) are
  converted to sRGB. A raw file with no usable preview shows a generic RAW tile. A SubIFDs pointer
  that leads to something whose entries are not in tag order is data, not an IFD, and is passed
  over. Maker notes are read leniently (a bad pointer only loses that preview), but when the
  bytes are known to be the start of a longer file (some IFD's strips run past them), a maker note
  or its preview pointer past their end makes the header read read on.
- **The other containers** (M5.10) each offer their embedded JPEGs, and the largest (by pixels,
  then bytes) is shown; the next smaller one is the thumbnail fast path. CR3: the `PRVW` preview,
  the `THMB` thumbnail and the first sample of each track whose sample description carries a `JPEG`
  box, through the bounded ISOBMFF walk (nesting ≤ 12, ≤ 4,096 boxes per level, stopping at
  `mdat`); orientation and camera from `CMT1`, dates from `CMT2`. RAF: the JPEG its header points
  at, with that JPEG's own EXIF. CRW: the full-size JPEG and thumbnail records of the CIFF heaps
  (nesting ≤ 8), the rotation of the image specification (degrees clockwise to display), make,
  model and time stamp. MRW: the maker note's preview and the IFD1 thumbnail of the `\0TTW` TIFF
  block, read with the shared IFD reader; a maker-note JPEG stored with a wrong first byte (not
  the 0xFF of the SOI marker) is read as if it were 0xFF, through a copy charged to the decode's
  memory. X3F: the JPEG image sections of the directory. CRW and
  X3F are found from the end of the file, so a header read takes the whole file (up to the header
  limit). A preview over the 64 MiB limit is passed over; offsets past the end of the file are
  "Damaged file".
- **Edits** apply to the embedded preview; Save writes a JPEG beside the raw at the preview's size.
  Full raw development is out of scope: **Open With** sends the file to a raw developer.
- **Metadata** is read from the raw's EXIF/XMP and an `.xmp` sidecar, and written only to the `.xmp`
  sidecar when enabled ([`xmp-interop.md`](xmp-interop.md) §7). RAW+JPEG pairs are two photos that
  share a stem.

## 3. HEIF / HEIC: an optional feature, not in release builds

Release builds carry no HEIC decoder (the maintainer's decision at M10.R, with the measurements of
§3.1): decoding HEVC needs a patent licence in some countries. The `heif` feature stays for people
who build Dalica themselves (`cargo build -p dalica --features heif`).

**Without the feature** a HEIC is organized like any other file. Its header is read from the
container, in pure Rust and without HEVC (`crates/image/src/formats/heif_items.rs`, on the bounded
box walk of `formats/bmff.rs`): `pitm` names the primary item, `iinf` gives its type and the `Exif`
item, `iprp` (`ipco`, `ipma`) its `ispe` size and its `irot`/`imir` transforms (as the orientation,
in association order), and `iloc` the `Exif` item's extents (construction method 0 only), whose
TIFF block follows its 4-byte header offset. So the scan lists it with its size and capture date,
it sorts and searches by date like any photo, and the Properties pane and status line show them.
Decoding and the embedded thumbnail still answer the tile text "HEIC support isn't included in
this build", and the cell's tooltip and the edit view add the hint under it: "On an iPhone,
Settings › Camera › Formats › Most Compatible saves new photos as JPEG. Or convert these photos to
JPEG." A container the reader cannot use (no `meta`, `pitm` or `ispe`, a damaged box, an item past
the end of a file that is all there, a size past §5's limits) is the same tile, never "Damaged
file"; bytes that stop inside a top-level box are a partial read and the header read goes on. Every
size and offset is checked; entry lists are capped at 65,536 and the EXIF item at 4 MiB. The XMP
item (captions other apps wrote) is not read without the feature yet; that is later work in
`dalica-meta`. An AV1 primary item is AVIF.

**With the feature:**


- Decoding uses `heic-rs` (pure Rust, no `unsafe`, MIT OR Apache-2.0) through a thin, panic-guarded
  wrapper of our own (prior art: LightCraft's `heif` crate): single pictures and grid-tiled photos, 8 to 12 bits (10/12-bit
  decode to 16-bit), auxiliary alpha (of the picture or grid, or one alpha picture per grid tile,
  as libheif writes a tiled picture with alpha), the container's rotation/mirror/crop, ICC, EXIF
  and XMP. Overlays, uncompressed (`unci`) items, 4:2:2 and 4:4:4 streams (lossless RGB included)
  return "unsupported", never wrong pixels; a grid whose tiles only partly carry alpha does too.
  An image sequence shows its still primary item when it has one, and is unsupported when it has
  none.
- It sits behind the **non-default** `heif` cargo feature of `dalica-image`, forwarded by the app
  (`cargo build -p dalica --features heif`). HEVC is patent-encumbered in some jurisdictions, so
  including a decoder is the distributor's decision. Official Dalica builds leave it out
  (M6.21–M6.23, confirmed at M10.R): their packages do not register HEIC/HEIF with the system.
- The decoder is young and pinned exactly (`=0.1.1`, without its `parallel` feature: Dalica
  decodes on its own worker threads); moving the pin is a reviewed change that re-runs the
  `heif` corpus tests (`development.md` §11). The review gate tests `dalica-image` with the feature
  (`cargo xtask ci --full`, on Linux), and its corpus job lints it.
- Before heic-rs reads a file, Dalica's own bounded box walk checks the top level and the
  containers inside `meta`; a file with `moov` and no `meta` (an image sequence) is unsupported.
  The colour description is the item's `colr nclx`, else the HEVC VUI (iPhones mark full range
  only there), else limited range; an unspecified matrix is BT.601. `imir` axis 0 flips top and
  bottom (as the files' writers mean it), axis 1 left and right; `clap`, `irot` and `imir` apply in
  the order the item lists them, so a decoded HEIC is upright and its header reports the shown
  size. EXIF is read for dates and camera; its orientation only repeats the container's. A file
  whose brands or primary item say AV1 is AVIF, whatever its major brand.
- HEIF is read-only: writing needs an HEVC encoder, and every mature one is C.

### 3.1 Measured on a wider set (M10.24)

The `heif` set holds three iPhone photos. To decide whether release builds can carry the feature,
a wider set, `heif-wide`, is made locally by `cargo xtask corpus make heif-wide`: `dalica-image`'s
`tests/heif_wide.rs` writes `dalica-scenes` pictures as PNGs, libheif's `heif-enc` encodes them
in every layout below (`$DALICA_HEIF_ENC` names it when it is not on the `PATH`; it needs an HEVC
encoder plugin, x265), and `heif-dec` writes libheif's own decode of each as a second reference.
The set (36 files, 1.3 MB of HEIF; 6 MB with sources and libheif's decodes) is CC0 like the
scenes, sits in `corpus/heif-wide/` and is not published until the maintainer decides; `cargo
xtask corpus` lists it as "not published yet (made locally)". `cargo test -p dalica-image
--features heif --test all heif_wide -- --nocapture` measures it (skipped with a message when the
set is absent): each file must give the answer the test's list expects, a decoded file must stay
above its PSNR floor against the source (floors set just under the measurements; they only rise),
and none may panic. Without the feature every file answers its tile text.

Measured 2026-10-15 with heif-enc 1.23.4, libheif 1.23.5 and x265 4.1, release build, one
decode at a time on an i7-13700K. PSNR is over RGBA, 8-bit sRGB after the colour stage (the
source, and libheif's decode, go through the same PNG decoder and colour stage):

| File | What | Answer | Against the source | Against libheif | ms |
|---|---|---|---|---|---|
| q90 / q50 / q10, 8-bit 4:2:0 | 640 × 480 at quality 90, 50, 10 | decoded | 48.8 / 48.0 / 40.8 dB | 53.4 / 53.8 / 56.0 dB | 11.5 / 4.5 / 2.5 |
| 10-bit, 12-bit 4:2:0 | Main 10; 12 bits (RExt) | decoded | 50.2, 50.4 dB | 53.3, 54.0 dB | 8.5, 7.7 |
| 4:2:2, 4:4:4 (8 and 10 bits) | `-p chroma=422/444` | refused: "This kind of HEIC file can't be displayed" | | libheif decodes them | 0.0 |
| lossless | RGB 4:4:4, identity matrix | refused, the same text | | libheif decodes it | 0.0 |
| alpha, 8 and 10 bits | an alpha auxiliary image | decoded | 48.8, 50.2 dB | 53.4, 53.2 dB | 21.6, 17.9 |
| monochrome | 4:0:0 | decoded | 71.4 dB (max 1) | identical | 4.9 |
| odd size | 641 × 479, 4:2:0 | decoded | 50.6 dB | 56.0 dB | 10.4 |
| grids | 1280 × 960: 3 × 2 tiles of 512; 10-bit 5 × 4 tiles of 256 | decoded | 52.4, 56.6 dB | 55.8, 56.8 dB | 30.3, 30.3 |
| grid with alpha | one alpha picture per tile | decoded (fixed in M10.24) | 52.4 dB | 55.8 dB | 55.0 |
| grid of 4:4:4 tiles | | refused, the HEIC text | | libheif decodes it | 0.1 |
| `irot` 90/180/270, `imir` both axes, irot + imir (EXIF 5 and 7) | every turn and flip | decoded upright | 47.8–48.8 dB | 50.6–53.5 dB | 8.0–10.0 |
| EXIF orientation 6, no `irot` | | shown as stored, as libheif does | 47.9 dB | 50.8 dB | 8.5 |
| ICC (Display P3), 8 and 10 bits; ICC and `nclx` both | | decoded, colour-managed | 46.2, 47.2, 46.2 dB | 50.8 dB | 8.3, 15.2, 8.5 |
| `nclx` BT.709 matrix, limited range | | decoded | 47.3 dB | 50.2 dB | 7.8 |
| an HEVC thumbnail item; a two-frame sequence beside its still primary | | decoded (the primary) | 48.8 dB | 53.4 dB | 7.8 |
| `mini` box layout (brand `mif3`); a JPEG-coded item (brand `jpeg`) | | "Not a picture Dalica can read" (not sniffed as HEIF) | | libheif decodes them | 0.0 |
| uncompressed (`unci`) item | | refused, the HEIC text | | libheif decodes it | 0.0 |
| AV1 | `.avif` | "AVIF files can't be displayed" | | libheif decodes it | 0.0 |

What it shows:

- No file panics; every refusal is a tile text, never wrong pixels. Every layout phones and
  cameras write (4:2:0 HEVC, 8 or 10 bits, grids, alpha, rotations and mirrors, ICC or `nclx`)
  decodes 46–71 dB from its source and 50–57 dB from libheif. The monochrome file decodes
  identically to libheif, so the HEVC stage agrees with libheif's decoder; the colour files differ
  from it only by chroma upsampling and rounding (they are closer to libheif's nearest-neighbour
  upsampling than to its default bilinear one). A 1.2 MP grid decodes in about 30 ms.
- One wrapper bug found and fixed: libheif stores the alpha of a tiled picture as one alpha picture
  per tile (no alpha grid), which Dalica ignored, showing such files opaque (16 dB). The wrapper
  now composes the tiles' alpha on the colour grid's layout.
- In the crate, recorded: heic-rs 0.1.1 fails on 4:2:2 and 4:4:4 streams ("picture is not fully
  covered by slices", "slice runs past the end of the picture", "coeff_abs_level_remaining prefix
  too long", or a truncation), so the wrapper's refusal of them stays; heic-rs 0.1.4 is published
  and is worth measuring against both sets before moving the pin.
- In sniffing, not the wrapper: files whose brands are only `mif3` (the compact `mini` layout) or
  `jpeg` with `mif1` compatible are not recognised as HEIF and answer "Not a picture Dalica can
  read" instead of the HEIC tile text. No phone writes them today.
- No openly licensed non-Apple camera HEIC was found: Nokia's conformance files carry a
  non-commercial licence, which the corpus refuses.

**Recommendation (for the M10.R decision; the answer: release builds leave the decoder out, §3).**
On these measurements the decoder is fit to ship
for the photos people have: everything phones write decodes correctly and quickly, and what it
cannot decode is refused with a tile text. Whether release builds carry it remains a licensing
decision these numbers do not change (HEVC patents, §3 above). If they do, two small follow-ups
first: recognise HEIF by an `mif1`/`mif3` compatible brand in sniffing, and measure heic-rs 0.1.4.
Publishing `heif-wide` to `dalica-corpus` (CC0, 6 MB) would let the review's corpus job run this
measurement on every platform.

## 4. Colour and orientation

- Every decode ends in 8-bit sRGB for display and in the edit renderer's working format. Embedded ICC
  profiles are applied with `moxcms` (v2/v4; sRGB, Display P3, Adobe RGB, ProPhoto, Rec.2020
  recognised); untagged images are assumed sRGB; an unusable profile falls back to sRGB and is
  logged; CMYK without a profile uses a naive conversion. The display is treated as sRGB (the
  maintainer's answer at M5.R, 2026-10-12); reading the real display profile per OS comes later,
  without C dependencies.
- Between the container and the display sits one **decoded image**: the stored pixels at their own
  depth (8- or 16-bit integers, or floats), their colour model (grey, RGB or CMYK; palettes are
  expanded by the format module), an optional straight alpha channel, the embedded profile's bytes
  and the orientation. One **colour stage** turns it into 8-bit sRGB:
  - with a usable profile, a relative-colorimetric transform to sRGB at the samples' own precision
    (16-bit sources are rounded to 8 bits once, at the end); alpha is carried over unchanged;
  - a profile is unusable when it is over the ICC limits of §5, its tag table runs past its data,
    it declares more bytes than it has, its colour space does not match the pixels (an RGB profile
    on a grey image), or `moxcms` refuses or fails on it; the pixels are then shown as untagged;
  - untagged grey and RGB are taken as sRGB; CMYK becomes R = (1 − C)(1 − K) and likewise for G and
    B. PNG `gAMA` and `cHRM` without an `iCCP` profile are not interpreted.
- **View → Use Color Management** (on by default) maps to the decode option `color_managed`: off,
  every embedded profile is ignored and every file is shown as untagged.
- JPEG profiles come from APP2 chunks, reassembled only when every chunk carries the same count and
  the sequence numbers are exactly 1 to that count; anything else is an unusable profile. CMYK
  JPEGs with an Adobe APP14 segment store their samples inverted (255 = no ink) and are inverted
  back; YCCK frames are converted from YCbCr to inverted CMY first. PNG profiles come from `iCCP`.
- EXIF orientation (and HEIF `irot`/`imir`) is applied for display; Dalica's own `rotate(…)` and
  flip operations in the recipe are applied on top. Files are never rewritten just to fix orientation.

## 5. Decode limits

File-derived numbers are hostile (AGENTS.md §3). Every decode goes through one `Limits` value that
`dalica-image` passes to the codecs, checked against the **header** before any pixel buffer is
allocated (and again after orientation swaps width and height):

| Limit | Value | Why |
|---|---|---|
| Width or height | ≤ 65,535 px (JPEG's own maximum) for full decodes; an absolute ceiling of 262,144 px applies to header reads | no real photo is larger; panoramas up to the JPEG maximum still open |
| Pixels for a full decode | ≤ 2^30 (1,074 MP; `Limits::max_pixels`, as in `security.md` §4.2) | covers stitched panoramas and medium-format files |
| Allocation per decode | ≤ min(the codec default, half the memory budget) — the budget is RAM/4 clamped 256 MiB–1.5 GiB (`architecture.md` §5) | a single file can never take the whole budget |
| Source pixels for a thumbnail from a decoder that cannot scale | ≤ 64 MP (the same default as LightCraft) | non-JPEG decoders allocate at full size before resizing |
| PNG text and XMP chunks | bounded by the allocation limit | |
| ICC profile | ≤ 16 MiB after APP2 reassembly or `iCCP` inflation; ≤ 1,000 tags; the tag table inside the data; `moxcms`' own caps on lookup-table and curve sizes (`security.md` §4.2) | a profile is parsed before any of its numbers are trusted |
| PNG decoder's own buffers | twice the ICC limit plus 16 MiB (chunk data, the inflated profile, one row), charged to the allocation limit; text chunks are skipped while decoding pixels | a compressed `iCCP` cannot inflate past it |

Whichever limit is reached first applies; for large images it is usually the allocation limit.

- Embedded previews are used whenever they cover the target, so very large raws show thumbnails
  without a full decode; a JPEG decodes in full and is fitted, and past 64 MP its EXIF thumbnail is
  used (decoding at the smallest DCT scale that covers the target, 1/2 to 1/8, is the later row
  `DATA-FMT-JPEG-FAST`).
- A file over a limit returns a "too large" error (shown on its tile); its embedded preview, if any,
  is still used. Allocations are fallible (`try_reserve`), so an allocation failure is an error, not
  an abort. Every pixel buffer of one decode (the decoded samples, the colour stage's bands and the
  8-bit output) is charged to the same allocation limit.
- Save and Export of very large images render in bands where the operations allow it; otherwise they
  fail with a clear message before starting rather than exhausting memory.
- Panics inside third-party decoders are caught (`catch_unwind`) and reported as "damaged file"; the
  regression input goes into the malformed-input tests as a synthetic reproduction.
- Container walks are bounded too: TIFF IFD chains (seen-set, depth limit, directory cap), XMP
  nesting depth and node count, EXIF entry counts, ISOBMFF box nesting. The walks Dalica owns count
  their steps (JPEG segments, PNG chunks, APP2 chunks, IFDs while recognising a raw) and stop at
  their cap.
- Every parser has a fuzz target in `crates/image/fuzz` (`security.md` §7.1): sniffing, decoding
  any file, decoding as each format, RAW previews, embedded thumbnails and ICC profiles.

## 6. Small images

Images smaller than 64 × 64 are indexed but hidden unless View → Small Pictures is on
(`data-model.md` §7); the threshold is in Options → File Types.

## 7. Tests

- Round trips for every format Dalica writes (JPEG, PNG) and metadata preservation per container
  ([`xmp-interop.md`](xmp-interop.md) §8).
- Save of every format of §1 through the engine's commands, checking the files on disk after the
  Save and after Undo Save (`crates/engine/tests/save_formats.rs`).
- Magic-number detection, including negatives (a `.jpg` that is really a PNG, a truncated header).
- Orientation 1–8 for every container that carries it.
- Limits: header-declared bombs are refused before allocating; images exactly at each limit decode.
- Malformed input: truncation at every offset, bit flips, proptest random bytes; never a panic.
- Corpora behind the `corpus` feature (`development.md` §11; `crates/image/tests/corpus.rs`):
  PngSuite, raw previews per vendor, our own `photos` set and the `heif` set (with the `heif`
  feature); per set, files sniffed as their extension says, headers read as scanning
  reads them, full decodes, 256 px thumbnails and embedded thumbnails have committed floors
  (`crates/image/tests/corpus_floors.json`) that only rise.
