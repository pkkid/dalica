# M6.42 proposal: sources for the `photos` and `heif` corpus sets

**Status: waiting for the maintainer's approval.** Nothing has been downloaded and nothing has been
pushed to `pkkid/dalica-corpus`. Every row of [`proposal-M6.42.tsv`](proposal-M6.42.tsv) was found
from metadata only (file pages, JSON APIs); its `bytes` and `sha1` are what the source publishes,
and they are checked again when the files are fetched after approval. This file and the TSV stay in
the task's pull request until the sources are approved, and are replaced by the pins
(`corpus_pins.rs`, `xtask/corpus/<set>.sha256`) and the corpus repository's own manifest afterwards.

## What the two sets hold

| Set | Sourced files | Generated files (after approval) | Sourced size | Estimated total |
|---|---|---|---|---|
| `photos/` | 47 (41 JPEG, 3 TIFF, 1 PNG, 2 WebP) | 12 (orientation set, progressive, truncated, ProPhoto, Adobe RGB without ICC) | 127.7 MB | about 138 MB |
| `heif/` | 5 HEIC | 4 (10-bit, 12-bit, alpha, monochrome) | 7.5 MB | about 10 MB |
| **Total** | **52** | **16** | **135.3 MB** | **about 148 MB** |

- **`photos/`**: camera and phone JPEGs as they came out of the device (the EXIF `Software` tag is
  the camera's or phone's firmware, or absent; not an editor), across 19 makes, from 2002 cameras to
  2025 phones, plus a few edited files chosen for one property each (CMYK, Adobe RGB, 16-bit, WebP).
  No file is larger than 6.8 MB.
- **`heif/`**: HEIC photos from phones, plus derived files for the HEIF features phones do not
  produce.
- **Generated files** are derived works of files in this list that are CC0 or public domain, made with
  tools that run once at preparation time (`jpegtran`, `exiftool`, LittleCMS, libheif's `heif-enc`);
  they are never linked into Dalica. Their rows in the TSV say `TBD` for size and SHA-1 until they
  exist. The recipe for each is in its `features` column.

### Sources and licences

- **Wikimedia Commons** (47 of the 52 sourced files, all but the HEIC files, which Commons does not
  accept): found through the Commons API (`list=search` with `incategory:"CC-Zero|PD-self|PD-user|PD-author"`,
  `filemime:` and `filesize:`, inside the `Taken with <camera>` categories; and categories such as
  *AdobeRGB photographs*, *Exif technical examples* and the CMYK-rendering maintenance categories),
  then `prop=imageinfo` (URL, size, SHA-1, MIME, the EXIF fields Commons parsed: `Orientation`,
  `Make`, `Model`, `Software`, `ColorSpace`, TIFF `BitsPerSample` and `PhotometricInterpretation`)
  and the file page's wikitext for the licence template, quoted exactly in the `licence` column:
  `{{self|cc-zero}}`, `{{Cc-zero}}`, `{{PD-self}}`, `{{PD-user-…}}` (released into the public domain
  by the uploader on a Wikipedia, transferred to Commons), `{{PD-USGov-Military-Air Force}}` and
  `{{PD-USAF}}` (US federal works), and `{{PD-ineligible}}` (three 1 KB synthetic test images too
  simple for copyright).
- **Internet Archive** (the 5 HEIC files): found through `advancedsearch.php` (`format:HEIF` with a
  CC0 or Public Domain Mark `licenseurl`) and the item metadata API, which gives each file's size and
  SHA-1. Three items are dedicated CC0 1.0 by their creators; two carry the Public Domain Mark 1.0
  (see the questions below).

## Coverage matrix

"Seen" means the property is in the metadata the source publishes; "expected" means the device or
the tool is known to write it, to be confirmed on the downloaded bytes (a file that does not have it
keeps its row but loses the claim).

| Feature | Files | State |
|---|---|---|
| Orientation 1 | most camera files; `gen_orientation/o1.jpg` | seen |
| Orientation 2 (mirrored) | `galaxy_s24ultra_o2.jpg`; `gen_orientation/o2.jpg` | seen; generated |
| Orientation 3 | `iphone4_o3`, `iphone7_o3_p3`, `iphone15promax_o3`, `galaxy_s10_o3`, `galaxy_s24ultra_o3`, `synthetic_o3` | seen |
| Orientation 4 | `gen_orientation/o4.jpg` only | **generated only** |
| Orientation 5 | `gen_orientation/o5.jpg` only | **generated only** |
| Orientation 6 | `iphone12pro_o6`, `pixel3_o6`, `xperia_z_o6`, `motorola_edge40neo_o6`, `nikon_d5100_o6`, `olympus_em1ii_o6` | seen |
| Orientation 7 | `synthetic_o7.jpg` (40 × 60); `gen_orientation/o7.jpg` | seen (synthetic); generated |
| Orientation 8 | `iphone7_o8_pano`, `canon_5d2_o8`, `canon_g12_o8`, `panasonic_g9_o8`, `synthetic_o8` | seen |
| Orientation 0 (out of range) | `iphone7_o0_invalid.jpg` | seen |
| No Orientation tag | `iphone5s_no_orientation.jpg`, `png16.png`, `webp_small.webp` | seen |
| All eight showing the same picture | `gen_orientation/o1.jpg` … `o8.jpg` (lossless `jpegtran` transform, then the tag) | generated |
| EXIF thumbnail | nearly every unedited camera and phone JPEG | expected |
| Thumbnail that disagrees with the main image | `gen_orientation/o2.jpg` … `o8.jpg` (the thumbnail is not transformed) | generated |
| MPF large preview (cameras) | `nikon_d5100_o6`, `sony_a6000`, `fujifilm_x10`, `panasonic_g9_o8` (and possibly other recent cameras) | expected |
| MPF gain map (phones, HDR) | `iphone12pro_o6`, `iphone15promax_o3`, `pixel8pro`, `galaxy_s24ultra_o3` | expected |
| Display P3 ICC | `iphone7_o3_p3`, `iphone7_o8_pano`, `iphone12pro_o6`, `iphone15promax_o3`, `xiaomi13` (ColorSpace uncalibrated seen) | expected |
| Adobe RGB ICC | `adobergb_b1b.jpg` (ColorSpace uncalibrated seen) | expected |
| Adobe RGB without ICC (camera DCF style) | `gen_adobergb_no_icc.jpg` | generated |
| ProPhoto RGB ICC (v4) | `gen_prophoto_v4.jpg` | **generated only** |
| CMYK JPEG | `cmyk_rsd1.jpg` (still listed in the CMYK-rendering category), `cmyk_bofa.jpg` (the 2010 CMYK upload; the page's current version is an RGB copy) | expected from the categories |
| CMYK TIFF, LZW | `tiff_cmyk_lzw.tif` (PhotometricInterpretation 5, Compression 5 seen) | seen |
| 16-bit TIFF | `tiff16_rgb.tif` (RGB), `tiff16_grey.tif` (grey), both uncompressed | seen |
| 16-bit PNG | `png16.png` | seen |
| WebP | `webp_exif.webp` (7728 × 5152, EXIF), `webp_small.webp` | seen |
| Progressive JPEG | `gen_progressive.jpg` | **generated only** (some sourced files may be progressive; checked on download) |
| Truncated JPEG | `gen_truncated.jpg` | generated |
| Very small / very large images | `synthetic_*` (40 × 60); `iphone7_o8_pano` (3646 × 6398), `webp_exif` (40 MP) | seen |
| HEIC from a phone (grid tiles, EXIF, thumbnail item, colour) | `iphone_cicada`, `iphone_busloup`, `iphone_foodcoop`, `iphone_bighorn`, `iphone_azpeitia` | expected (all Apple) |
| HEIC 10-bit / 12-bit | `gen_10bit.heic`, `gen_12bit.heic` | **generated only** |
| HEIC with alpha | `gen_alpha.heic` | **generated only** |
| HEIC monochrome | `gen_mono.heic` | **generated only** |

**Makes and models (19 makes):** Apple iPhone 4, 5S, 7, 12 Pro, 15 Pro Max; Google Pixel 3, 8 Pro;
Samsung Galaxy S4, S10, S24 Ultra; Xiaomi 13; Huawei P20 Pro; Sony Xperia Z; Motorola Edge 40 Neo;
Nokia 5230; Canon EOS 5D Mark II, PowerShot G12; Nikon D5100, Coolpix 4500, D300 and Z 6II (via
Photoshop); Sony ILCE-6000, DSC-W180; Fujifilm X10, FinePix A340, X-H2 (WebP); Olympus OM-D E-M1
Mark II, C-5050Z; Panasonic Lumix DC-G9; Pentax K-5; Ricoh Caplio R4; HP PhotoSmart R707; Kodak
EasyShare Z710; Casio Exilim EX-Z75; Minolta DiMAGE Z1.

### Gaps

- **Orientations 4 and 5** exist only as generated files, and orientation 7 only as a 1 KB synthetic
  file: no public-domain camera file with them turned up (cameras never write them; only mirrored
  front cameras or editors do).
- **ProPhoto RGB**: every Commons photo tagged ProPhoto is CC BY or CC BY-SA; the ProPhoto file is
  generated with a CC0 profile.
- **MPF, EXIF thumbnails, ICC identity and CMYK** cannot be seen without the bytes; they are
  confirmed after download, and a claim that fails is struck from the manifest (the file stays if it
  still covers something).
- **HEIF** comes from Apple phones only: no public-domain Samsung, Google or Canon (`.HIF`, 10-bit)
  HEIC was found; 10/12-bit, alpha and monochrome are generated with one encoder (libheif +
  x265).
- **Not covered** (later tasks if wanted): AVIF, JPEG XL, GIF, RAW+JPEG pairs, XMP sidecars, the
  `metadata/` and `public-faces/` sets.

## Excluded candidates

| Candidate | Why |
|---|---|
| Unsplash photos, including Commons copies such as *Woman holding a bouquet of tulips (Rawpixel.com, Unsplash)* (Adobe RGB, Nikon D810) | Unsplash's licence is not CC0 (excluded by rule, even for uploads from its earlier CC0 era) |
| Nokia `heif_conformance` files (github.com/nokiatech/heif_conformance) | licence: the repository states none |
| Nokia HEIF sample files (github.com/nokiatech/heif, and copies in other projects) | licence: Nokia HEIF License 2.1, non-commercial evaluation only |
| MPEG `FileFormatConformance` HEIF files (github.com/MPEGGroup/FileFormatConformance) | licence: BSD-3-Clause-Clear, not CC0 or public domain; picture provenance not stated |
| libheif `examples/example.heic` | licence: no licence stated for the image; the project is LGPL / MIT for code |
| pillow_heif test images (`tests/images/…`) | licence: BSD-3-Clause repository, picture provenance not stated (some are Nokia's) |
| Exif orientation example sets on GitHub (the usual eight-landscape/eight-portrait set) | licence: MIT, not CC0; we generate our own |
| Commons *ProPhoto RGB photographs* (7 files) | licence: CC BY / CC BY-SA |
| Commons *AdobeRGB photographs* (1168 files) | licence: only two under 9 MB are CC0 or public domain (one is from Unsplash, the other is used) |
| *Kulmhof - lapidarium nagrobków.jpg* (Minolta DiMAGE Z1) | licence: the page carries both a public-domain and a CC BY-SA 2.5 tag; replaced by an unambiguous file |
| *Optisches Museum* (Panasonic G9, orientation 8) | modern exhibits indoors may carry their own copyright; replaced by an outdoor monument |
| Many Internet Archive HEIC items marked CC0 or Public Domain Mark | personal or family photos, people, scans of copyrighted magazines, or no named creator |
| Zenodo HEIC datasets (e.g. a CC BY 4.0 manuscript set) | licence: CC BY, not CC0 |
| US Forest Service Canon 5D Mark IV orientation-8 photos (public domain, 7.4–16 MB) and other files over 7 MB | size; orientation 8 is covered by smaller files |

## Questions for the maintainer

1. **Approve the source list** (the 52 sourced rows) and the 16 generated files, or strike rows.
2. **Public Domain Mark 1.0** on `heif/iphone_bighorn.heic` and `heif/iphone_azpeitia.heic`: the
   creators marked their own photos with the Mark rather than dedicating them with CC0. Commons
   accepts the creator's Mark as a release (`PD-author-FlickrPDM`). Keep them (recommended, they are
   the only non-people HEIC subjects besides the three CC0 ones) or drop them?
3. **Internet Archive as a source**: its licences are the uploader's own statement, with no review
   like Commons'. The five items were picked because the uploader is named as the creator and the
   subjects are places, animals and buildings. Acceptable?
4. **Download and push go-ahead**: approve fetching the 52 files and pushing them, the generated
   files and the manifest to `pkkid/dalica-corpus`.
5. Optional: a **non-Apple HEIC** (Samsung, Pixel, or a Canon `.HIF`) released CC0 by the person who
   took it (a picture of a non-personal subject) would close the HEIF gap.

## After approval

1. **Download** each `source_url` (with `curl`, the same generic User-Agent and 512 MiB cap as
   `cargo xtask corpus`) into a scratch clone of `pkkid/dalica-corpus`, at `<set>/<path>`.
2. **Verify** each file's size and SHA-1 against the TSV; any mismatch stops the run (the source
   changed, so the row is checked again by hand).
3. **Confirm the expected features** on the bytes (orientation, EXIF thumbnail, MPF, ICC
   description, CMYK, progressive, HEIF bit depth and grid) with Dalica's own readers and
   `exiftool`, and correct each row's `features`; look at every image once and drop any that shows a
   person's face up close or anything personal.
4. **Generate** the 16 derived files with the recipes in the TSV, record their sizes and SHA-1 values,
   and the tool versions.
5. **Write the corpus repository's manifest**: `SOURCES.tsv` (the TSV's columns, with the generated
   files' parents) and `LICENSE` (CC0 1.0 for our own changes, with a statement that every file is CC0
   or public domain and where each came from); a short `README.md`.
6. **Push** to `pkkid/dalica-corpus` (one commit), note the commit id.
7. **Pin**: set `DALICA_CORPUS_COMMIT` in `xtask/src/corpus_pins.rs`, run
   `cargo xtask corpus --update-manifest photos` and `… heif` to write `xtask/corpus/photos.sha256`
   and `heif.sha256`, review the manifests.
8. **Measure floors**: `cargo xtask test-corpus`, add the `photos` and `heif` floors to
   `crates/image/tests/corpus_floors.json` at the measured counts, update the set table in
   `docs/development.md` §11, delete this proposal and its TSV, tick M6.42.
