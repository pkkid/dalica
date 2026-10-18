# Assets, fonts, icons and demo images

Adapted from the "Assets" and font sections of LightCraft `AGENTS.md`, `assets/ATTRIBUTION.md`,
`assets/app-icon/README.md` and `xtask/src/assets.rs` (revision 88ea1a8), and PhotoCraft
`ATTRIBUTION.md`, `docs/images/SOURCES.md` and `assets/app-icon/README.md` (revision 59684266);
MIT OR Apache-2.0.

This is the **absolute rule** behind `AGENTS.md` §4 → Assets. It covers every non-code file that
ends up in the repository or in a package: images, icons, fonts, sounds, videos, colour profiles,
word lists, sample data and screenshots. **When in doubt about a licence, don't use it.**

## 1. What is allowed

Every asset in the repository must be one of:

- **our own original work** (icons drawn in code as vectors, procedurally generated demo photos,
  screenshots of Dalica itself), licensed MIT OR Apache-2.0 like the code;
- **public domain / CC0**;
- **Creative Commons** CC-BY or CC-BY-SA, with the attribution honoured;
- **OFL** (fonts);
- **permissive open source** (MIT, Apache-2.0, BSD, ISC);
- or contributed by the person who created it, who licenses it openly in the PR.

Never allowed, not even as a temporary placeholder or a "reference copy":

- anything from the reference organizer or its vendor's products: icons, bitmaps, button art,
  effect previews, sample photos, sounds, fonts, help text, templates, colour data;
- anything traced, recoloured or redrawn from such material (imitate *meaning and placement*, not
  the drawing; see [`design-system.md`](design-system.md) §5);
- assets under non-commercial (NC), no-derivatives (ND), "free for personal use", unknown or
  unstated licences, and anything from GPL/LGPL/AGPL projects;
- personal photos, of anyone, including the contributor's own family photos;
- the ArtCraft name, wordmark and logos: they are trademarks of the ArtCraft Team, not open source,
  and are not part of Dalica.

## 2. Attribution, in the same commit

- Every asset gets a row in [`ATTRIBUTION.md`](../ATTRIBUTION.md) with **Path | Asset | Creator |
  Source | Licence | Added | Modifications**, added **in the same commit** as the asset. Original
  work gets a row too ("original work", MIT OR Apache-2.0). Assets without a row must not be
  committed.
- Put the licence text next to the asset when the licence requires it (`assets/fonts/OFL-Inter.txt`,
  `assets/icons/LICENSE-lucide.txt`) and reference it, backticked, in the Licence column.
- Third-party material bundled in the product is also summarised in [`NOTICE`](../NOTICE).
- LightCraft or PhotoCraft code reused in Dalica (`architecture.md` §9), and any other
  third-party code ever vendored, goes in `NOTICE` with its source revision and keeps a header
  naming its origin.

## 3. `cargo xtask assets`

Part of `cargo xtask ci`. It lists the tracked files and fails when:

- a file with an asset extension (images and icons such as `png`, `jpg`, `svg`, `ico`, `icns`;
  fonts; sounds; videos; raw and TIFF files; ICC profiles; XMP files) is not matched by a path
  pattern in the first column of `ATTRIBUTION.md` (`*` = any characters except `/`; a trailing `/`
  covers a whole directory);
- a licence file referenced in the Licence column does not exist;
- a file's **name** contains the reference product's name or its vendor's name;
- a file has one of the reference application's own **file extensions**: its database and index
  files, its backup and export container formats, its per-folder settings files under their
  original names, and its proprietary album, face or screensaver data. Dalica *reads* some legacy
  files from the user's folders at run time ([`data-model.md`](data-model.md)); they never belong in
  the repository, not even as test fixtures (tests generate synthetic equivalents).

The forbidden names and extensions are kept in `xtask/src/assets.rs` in a form that does not spell
the product names out (for example hashed tokens), so `cargo xtask names` stays green. Only table
rows that start with `|` are read; the commented example in `ATTRIBUTION.md` is ignored.

**Never weaken this check to make a commit pass** — fix the row or remove the asset.

## 4. Fonts

- **Inter** (Regular and SemiBold, OFL, `assets/fonts/` with `OFL-Inter.txt`) is the **only font
  file committed** to this repository. It covers Latin, Greek and Cyrillic UI text.
- Every other face (Chinese, Japanese and Korean fallbacks, a monospace face for numeric fields if
  [`design-system.md`](design-system.md) §3 ever needs one) comes from
  [storytold/craft-fonts](https://github.com/storytold/craft-fonts) through the **optional
  `CRAFT_FONTS_DIR` build input**, exactly as in LightCraft:

  ```sh
  git clone https://github.com/storytold/craft-fonts ../craft-fonts
  CRAFT_FONTS_DIR=../craft-fonts cargo run --release -p dalica
  ```

  A build script embeds the manifest's faces; the UI installs them as fallbacks after Inter, picked
  by script. Without `CRAFT_FONTS_DIR` everything builds, tests and runs, but CJK text shows boxes;
  tests that need those faces skip. Release builds set `CRAFT_FONTS_REQUIRED=1` and ship each
  font's licence as `OFL-<family>.txt` ([`releasing.md`](releasing.md)).
- A missing face is added to craft-fonts (with its own attribution there), never committed here and
  never worked around by loading arbitrary system fonts into exports.
- Use fonts unmodified under their licence. Fonts that shipped inside the reference organizer are
  never used.
- Fonts are installed on the first frame; nothing is painted until the next frame.

## 5. Icons

- **Drawn in code** first: original vector icons on a 20 × 20 design grid in
  `crates/ui-kit/src/icons.rs` (the LightCraft technique, our own drawings), painted with `paint(&Painter, rect, Icon, Color32)`, round caps, stroke
  `max(1.5·k, 1.0)` where `k = side / 20`. Recorded in `ATTRIBUTION.md` as original work.
- When a glyph is too complex to draw well (camera, film reel), use a
  [Lucide](https://lucide.dev) SVG (ISC; the subset derived from Feather is also MIT) from
  `assets/icons/`, with `assets/icons/LICENSE-lucide.txt` and an `ATTRIBUTION.md` row naming the
  upstream file. Icons derived from a Lucide glyph (added marks) say "Derived from …" in the Source
  column. Lucide icons are tinted at runtime like the drawn ones.
- No icon fonts, no bitmap icons, no icon sets with attribution-only or unclear terms.
- Never trace or copy icons from the reference application or any other proprietary product.

## 6. Demo, documentation and test images

- **Demo library:** `dalica-scenes` generates procedural photos at run time (`--demo`): landscapes,
  gradients, patterns, synthetic EXIF dates and GPS so folders, date sorting and Places have
  something to show. Recorded once in `ATTRIBUTION.md` as original work.
- **Screenshots in docs and the README** show Dalica's own UI with the procedural demo library or
  **public-domain** photographs (e.g. works of U.S. federal agencies such as NASA, or photographs
  whose copyright has expired), never personal photos. Each image used is listed in
  `docs/images/SOURCES.md` (file, what it shows, source URL and licence) and in `ATTRIBUTION.md`.
- **People features** (face detection, the People pane) are demonstrated only with public-domain
  portraits of public figures or with procedural faces, never photos of private individuals.
- **Test photos** are procedural or public domain/CC0, fetched by `cargo xtask corpus` into the
  gitignored `corpus/`, pinned by URL and sha256 with their source and licence recorded in the pin
  list. Never commit media; files copied into `corpus/` by hand must be CC0, public domain, MIT or
  BSD.
- When fetching assets or corpus files, never put personal details in requests; use the generic
  User-Agent `Dalica-dev`.

## 7. App icon

- The app icon is **original artwork** made for Dalica by the project owner or a contributor who
  licenses it MIT OR Apache-2.0 (`assets/app-icon/LICENSE.txt`). It must not resemble the reference
  organizer's icon or any other product's logo. Its design is the maintainer's decision.
- `assets/app-icon/README.md` records the motif, the palette (hex values and what each is used
  for), the geometry, the provenance (who drew it, with what, how it was vectorised) and the file
  list. If Dalica follows the Craft apps' icon family: a 512-unit tile (`viewBox="0 0 512 512"`),
  rounded square `rx=112`, full-bleed colour field, at most three colours; the macOS renders sit on
  Apple's 824/1024 grid, the Windows and Linux renders use the full tile so the motif reads at
  16–48 px.
- One canonical SVG; every derived file (1024 px PNG, `.icns`, `.ico`, hicolor PNGs 16–512 px and
  the scalable SVG) is regenerated by `packaging/icons.sh` and **committed**, so packaging never
  needs the render tools. One `ATTRIBUTION.md` row covers `assets/app-icon/`.
- `packaging/icons.sh` renders with ImageMagick 7 and its librsvg coder and packs the `.ico` and
  `.icns` (PNG entries) with a short python3 script, so it runs on Linux; it is deterministic (no
  time stamps in the PNGs), and `packaging/icons.sh --check` fails when a committed file differs
  from a fresh render. The icon's README names the tool versions used.
- The window sets its own icon from a PNG embedded in the binary (`apps/dalica/src/icon.rs`):
  the 1024 px macOS-grid render on macOS, the 256 px full-tile hicolor render elsewhere. The
  hicolor and scalable files are named after the application id (`org.dalica.Dalica`).
- Inside the window the icon is drawn from the same committed renders, never redrawn in code
  (`crates/ui/src/app_icon.rs`): the title bar's top-left corner (`design-system.md` §11.1) and the
  About tab each draw the smallest hicolor PNG (16–256 px) that covers the icon's size in physical
  pixels, uploaded as a texture once.
- Artwork derived from the icon (the macOS DMG background in `packaging/macos/dmg/`, installer
  bitmaps) references the canonical SVG rather than copying it, outlines any text so rendering
  needs no fonts, and gets its own `ATTRIBUTION.md` row.

## 8. Other data

- **Sounds:** none are planned; if one is added, the same rules apply.
- **Face-recognition models** are never committed or bundled; the user downloads them by an
  explicit action under their own licence ([`architecture.md`](architecture.md) §9, reserved
  decision 5 in [`roadmap.md`](roadmap.md)).
- **Map tiles** and other online data are fetched at run time only after a user action and shown
  with the provider's required attribution in the UI; none of it is cached into the repository.
- **The offline map** (M13.12) is the one map layer in the repository: Natural Earth's 1:110m land
  (public domain), converted by `assets/maps/convert.py` from the GeoJSON of
  `nvkelso/natural-earth-vector` v5.1.2 into `assets/maps/land-110m.bin` (20 KB: rings of points in
  hundredths of a degree, format in `assets/maps/README.md`) and compiled into `dalica-ui-kit`. It
  has its `ATTRIBUTION.md` row; the map names it in its attribution line while no tiles are shown.
  The file must stay within 256 KB (a test checks it); a finer scale needs the same conversion
  and a new row, never data of unknown licence.
- **Translations** are original work ([`localization.md`](localization.md)) and get an
  `ATTRIBUTION.md` row per catalog.
