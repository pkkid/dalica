# Attribution

Every non-code asset in this repository (image, icon, font, sound, video, colour profile, word
list or other bundled data), with its creator, source and licence. Dalica's own code and original
assets are MIT OR Apache-2.0 (see `LICENSE-MIT`, `LICENSE-APACHE` and [`NOTICE`](NOTICE)). The
rules are in [`docs/assets.md`](docs/assets.md); the short version:

- Every asset is **original work** (for example icons drawn in code, procedurally generated demo
  photos), **public domain / CC0**, **Creative Commons** (CC-BY / CC-BY-SA, attribution honoured),
  **OFL** (fonts) or **permissively licensed** (MIT, Apache-2.0, BSD, ISC), or contributed by the
  person who created it under an open licence.
- Nothing comes from the reference organizer or its vendor, not even as a placeholder, and nothing
  is traced from any proprietary product.
- **Add the row in the same commit as the asset**, and put the licence text next to the asset when
  the licence requires it (e.g. `assets/fonts/OFL-Inter.txt`, `assets/icons/LICENSE-lucide.txt`).
  Assets without a row must not be committed.
- When in doubt about a licence, don't use it.

`cargo xtask assets` (part of `cargo xtask ci`) fails if any asset file in the repository is not
matched by a path pattern in the first column (`*` = any characters except `/`; a trailing `/`
covers a whole directory), if a licence file referenced in the Licence column is missing, or if a
file's name or extension suggests material from the reference organizer. Never weaken the check to
make a commit pass: fix the row or remove the asset.

Columns: **Path** (backticked path pattern), **Asset** (title), **Creator**, **Source** (URL, or
"original work"), **Licence** (with the backticked path of its licence file when one is required),
**Added** (YYYY-MM-DD), **Modifications** (what we changed, or "none").

## Bundled in the app

| Path | Asset | Creator | Source | Licence | Added | Modifications |
|---|---|---|---|---|---|---|
| `assets/fonts/Inter-*.ttf` (Regular, SemiBold) | Inter 4.001 (static TTF) | The Inter Project Authors (Rasmus Andersson) | https://github.com/rsms/inter | SIL Open Font License 1.1 (`assets/fonts/OFL-Inter.txt`) | 2026-10-10 | none |
| `assets/app-icon/` (the canonical SVG and every render of it) | Dalica app icon, a melting camera shutter (chosen by the maintainer, `assets/app-icon/README.md`) | the Dalica contributors | original work (hand-written SVG; renders by `packaging/icons.sh`) | MIT OR Apache-2.0 (`assets/app-icon/LICENSE.txt`) | 2026-10-12 | none |
| `assets/maps/land-110m.bin` | Natural Earth 1:110m land (`ne_110m_land`, v5.1.2), compact quantised polygons for the offline map | Natural Earth (naturalearthdata.com) | https://github.com/nvkelso/natural-earth-vector (`geojson/ne_110m_land.geojson` at tag v5.1.2, SHA-256 `9e0729ee253ca7d7a5c4ae9395fb1902264c5377c52e224d13dd85010e2835d9`) | Public domain (Natural Earth terms of use; noted in `assets/maps/README.md`) | 2026-10-18 | converted by `assets/maps/convert.py`: coordinates rounded to 0.01°, repeated points dropped, rings kept as land or lake |
| `crates/ui/src/i18n/*.tsv` | Interface translation catalogs: Thai (`th.tsv`) | the Dalica contributors (a machine-assisted draft, every entry awaiting the fluent review) | original work, translated from the meaning of Dalica's English text (`docs/localization.md` → Clean-room translations, `docs/localization-th.md`) | MIT OR Apache-2.0 (`crates/ui/src/i18n/LICENSE-translations.txt`) | 2026-10-20 | none |
| `crates/ui-kit/src/theme/palettes/solarized.rs` (the Solarized Dark palette) | Solarized colour scheme, used by the Solarized Dark theme (ported from PhotoCraft `crates/ui-egui/src/theme.rs` @ 3a896a5e) | Ethan Schoonover | https://ethanschoonover.com/solarized/ | MIT (`crates/ui-kit/src/theme/palettes/LICENSE-solarized.txt`) | 2026-11-05 | base02/base03 surfaces and the blue accent as PhotoCraft maps them; the text levels moved to base2 and a lighter base1 and the red lightened for contrast (`solarized.rs`) |

## Website (`website/`, served at dalica.pkarma.com; not bundled in the app)

| Path | Asset | Creator | Source | Licence | Added | Modifications |
|---|---|---|---|---|---|---|
| `website/public/fonts/SchibstedGrotesk-Variable-latin.woff2` | Schibsted Grotesk (variable, weights 400–800) | The Schibsted Grotesk Project Authors | https://github.com/schibsted/schibsted-grotesk (the Latin subset served by Google Fonts, v7) | SIL Open Font License 1.1 (`website/public/fonts/OFL-SchibstedGrotesk.txt`) | 2026-10-12 | none (Google Fonts' Latin subset, unchanged) |
| `website/public/fonts/IBMPlexMono-*.woff2` (Regular, Medium) | IBM Plex Mono 2.327 | IBM Corp. | https://github.com/IBM/plex (`packages/plex-mono/fonts/complete/woff2`) | SIL Open Font License 1.1 (`website/public/fonts/OFL-IBMPlexMono.txt`) | 2026-10-12 | none |
| `website/public/img/icon.svg` | Copy of the Dalica app icon | the Dalica contributors | `assets/app-icon/dalica.svg` | MIT OR Apache-2.0 (`assets/app-icon/LICENSE.txt`) | 2026-10-12 | none (refresh it when the icon changes) |
| `website/public/img/*.jpg`, `website/public/img/thumbs/*.jpg` | Screenshots of Dalica 1.1 (the library, the Import room, the Tags and Places panes, a search and a color search, the Auto Fix compare, the Wildlife album, the Collage room, the edit view's History pane, the Export dialog, the slideshow) and their filmstrip thumbnails | the Dalica contributors | original work: headless captures with `dalica-cli snapshot` and the control channel (`ui.screenshot`) at 2× scale, of a library made from 68 openly licensed photos (56 in the library, 12 on a memory card) from Wikimedia Commons; each photo's title, author, licence and page are listed on `website/public/credits.html`; the Places shots show OpenStreetMap tiles (© OpenStreetMap contributors, ODbL) | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) for Dalica's interface; the photos shown keep their own licences (CC BY 4.0, CC BY-SA 4.0 or CC0 1.0, `website/public/credits.html`) | 2026-11-07 | cropped (the window's title bar removed, a thin edge strip filled with the color next to it), resized, JPEG |
| `website/public/docs/img/` | The guide's figures: renders by Dalica of the photos above (each edit run through `dalica-cli`, then exported), crops of them, charts computed from those renders, and diagrams | the Dalica contributors | original work; the photos rendered are those listed on `website/public/credits.html` | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) for charts and diagrams; a figure that shows a photo is an adaptation under that photo's licence (CC BY 4.0, CC BY-SA 4.0 or CC0 1.0, `website/public/credits.html`) | 2026-11-07 | rendered, cropped, resized, annotated; PNG and JPEG |
| `website/public/index.html` (inline SVG symbols `i-linux`, `i-mac`, `i-win`) | Linux (Tux), Apple and Windows marks for the download cards | Simple Icons contributors | https://simpleicons.org: `icons/linux.svg` and `icons/apple.svg` of the `simple-icons` npm package 13.0.0, `icons/windows.svg` of 4.25.0 | CC0 1.0 (`website/public/img/LICENSE-simple-icons.txt`); the marks are trademarks of their owners, used only to name the systems | 2026-10-13 | path data only, inlined as `<symbol>`s with `fill="currentColor"` |
| `website/public/img/og.png` | Link-preview image (app icon, name and a screenshot) | the Dalica contributors | original work, composed with ImageMagick from the screenshot and icon above; text set in Inter (`assets/fonts/`) | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | 2026-10-12 | none |

## Optional build input: craft-fonts (not files in this repository)

Builds made with `CRAFT_FONTS_DIR` embed fonts from
[storytold/craft-fonts](https://github.com/storytold/craft-fonts) at a pinned commit; no font file
from it is committed here. Per-file authors, sources and licences are in craft-fonts'
`ATTRIBUTION.md`; release packages ship each font's licence as `OFL-<family>.txt`. The families
Dalica's builds embed at the pin (`3d9ab3a`), in the desktop app only (M16.27):

| Font (embedded at build time) | Use | Licence |
|---|---|---|
| BIZ UDPGothic Regular (`fonts/biz-ud-pgothic/BIZUDPGothic-Regular.ttf`) | Japanese text; subset at build time to the Han characters of the four national sets (`crates/ui-kit/build.rs`) | OFL-1.1, no Reserved Font Name |
| Noto Sans CJK SC Regular (`fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf`) | Chinese and Korean text; subset as above | OFL-1.1, no Reserved Font Name |
| Noto Sans Thai (`fonts/noto-sans-thai/NotoSansThai.ttf`) | Thai text, whole | OFL-1.1 |
| Noto Sans Arabic (`fonts/noto-sans-arabic/NotoSansArabic.ttf`) | Arabic text, whole | OFL-1.1 |

## Documentation and README

Screenshots show Dalica's own UI with procedural demo photos or public-domain images only; each
image used is listed in `docs/images/SOURCES.md` and here.

| Path | Asset | Creator | Source | Licence | Added | Modifications |
|---|---|---|---|---|---|---|
| `docs/mockups/*.png`, `docs/mockups/*.html` | Target mockups of Dalica's windows (procedural placeholder photos, icons drawn for Dalica) | the Dalica contributors | original work | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | 2026-10-09 | none |

## Test data (not committed, not shipped)

`corpus/` is gitignored. `cargo xtask corpus` fetches pinned, sha256-verified public domain/CC0
photos into it and records each file's source and licence in the pin list; no test media is
committed to this repository.
