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

## Website (`website/`, served at dalica.pkarma.com; not bundled in the app)

| Path | Asset | Creator | Source | Licence | Added | Modifications |
|---|---|---|---|---|---|---|
| `website/public/fonts/SchibstedGrotesk-Variable-latin.woff2` | Schibsted Grotesk (variable, weights 400–800) | The Schibsted Grotesk Project Authors | https://github.com/schibsted/schibsted-grotesk (the Latin subset served by Google Fonts, v7) | SIL Open Font License 1.1 (`website/public/fonts/OFL-SchibstedGrotesk.txt`) | 2026-10-12 | none (Google Fonts' Latin subset, unchanged) |
| `website/public/fonts/IBMPlexMono-*.woff2` (Regular, Medium) | IBM Plex Mono 2.327 | IBM Corp. | https://github.com/IBM/plex (`packages/plex-mono/fonts/complete/woff2`) | SIL Open Font License 1.1 (`website/public/fonts/OFL-IBMPlexMono.txt`) | 2026-10-12 | none |
| `website/public/img/icon.svg` | Copy of the Dalica app icon | the Dalica contributors | `assets/app-icon/dalica.svg` | MIT OR Apache-2.0 (`assets/app-icon/LICENSE.txt`) | 2026-10-12 | none (refresh it when the icon changes) |
| `website/public/img/library.jpg`, `website/public/img/crop-*.jpg`, `website/public/img/feat-*.jpg` | Screenshots of Dalica (the library, Save, export progress, Projects, Crop; the nine feature shots: library, search, edit view, album, Export dialog, and 0.6's Import room, A│A compare, slideshow and photo viewer) | the Dalica contributors | original work: headless captures with `dalica-cli snapshot` or the control channel at 2× scale; `crop-crop.jpg`, `library.jpg` and `feat-*.jpg` show photos from the `photos` set of https://github.com/pkkid/dalica-corpus at `6239cad` (CC0 or public domain, source in that repository's `SOURCES.tsv`); the others show the procedural demo library (`--demo-fresh`, `dalica-scenes`) | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`); the corpus photo CC0 / public domain | 2026-10-12 (0.2 captures 2026-10-13, 0.3 captures 2026-10-14; `library.jpg` 2026-10-14; `feat-*.jpg` 2026-10-17, the 0.6 ones 2026-10-18) | cropped or resized, JPEG; the photos in `feat-import.jpg` resized with their metadata stripped |
| `website/public/index.html` (inline SVG symbols `i-linux`, `i-apple`, `i-windows`) | Linux (Tux), Apple and Windows marks for the download cards | Simple Icons contributors | https://simpleicons.org: `icons/linux.svg` and `icons/apple.svg` of the `simple-icons` npm package 13.0.0, `icons/windows.svg` of 4.25.0 | CC0 1.0 (`website/public/img/LICENSE-simple-icons.txt`); the marks are trademarks of their owners, used only to name the systems | 2026-10-13 | path data only, inlined as `<symbol>`s with `fill="currentColor"` |
| `website/public/img/og.png` | Link-preview image (app icon, name and a screenshot) | the Dalica contributors | original work, composed with ImageMagick from the screenshot and icon above; text set in Inter (`assets/fonts/`) | MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`) | 2026-10-12 | none |
| `website/public/img/castle.jpg` | Dunluce Castle (`photos/minolta_z1.jpg` of the corpus) | Aph-ch (Wikimedia Commons uploader, "own photograph") | https://commons.wikimedia.org/wiki/File:Dunluce3.JPG | Public domain (Wikimedia Commons PD-self) | 2026-10-12 | square crop, resized to 900 px, metadata stripped |

## Optional build input: craft-fonts (not files in this repository)

Builds made with `CRAFT_FONTS_DIR` embed fonts from
[storytold/craft-fonts](https://github.com/storytold/craft-fonts) at a pinned commit; no font file
from it is committed here. Per-file authors, sources and licences are in craft-fonts'
`ATTRIBUTION.md`; release packages ship each font's licence as `OFL-<family>.txt`. List here the
families Dalica's builds embed once the pin exists.

| Font (embedded at build time) | Use | Licence |
|---|---|---|

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
