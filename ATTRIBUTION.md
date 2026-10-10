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

<!--
Example row (not a real entry; copy it without the leading "e.g. " when Inter lands in M0.4):

e.g. | `assets/fonts/Inter-*.ttf` (Regular, SemiBold) | Inter | The Inter Project Authors (Rasmus Andersson) | https://github.com/rsms/inter | SIL Open Font License 1.1 (`assets/fonts/OFL-Inter.txt`) | 2026-10-09 | none |
-->

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
