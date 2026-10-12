# Dalica app icon

**Status: a candidate.** The icon's design is the maintainer's decision (`docs/assets.md` §7); this
one was drafted in M6.12 for the M6.R check-in to accept or replace.

## Motif

A **magpie carrying a print**: the bird that collects things, standing in side view with a small
photo print in its beak. The bird fills the tile in the Craft family's way: the colour field is
full-bleed, the tail runs off the bottom-right corner and the legs off the bottom edge. Flat shapes
with no fine line work, so the silhouette, the white shoulder and belly patches and the paper print
still read at 16 px.

## Palette

Exactly three colours:

| Colour | Hex | Used for |
|---|---|---|
| Ink | `#0b0b0c` | the bird, its legs, the print's frame and picture |
| Paper | `#efe9dc` | the shoulder and belly patches, the eye ring, the print |
| Dalica teal (app colour) | `#1fa38a` | the full-bleed field |

Ink and paper are the Craft family's; the teal sets Dalica apart from its siblings' amber and blue.

## Geometry

A 512-unit tile (`viewBox="0 0 512 512"`), a rounded square with `rx=112` that clips everything,
no border. The macOS renders (`dalica-1024.png`, `dalica.icns`) widen the viewBox to
`-62 -62 636 636`, putting the tile on Apple's 824/1024 grid with a transparent margin; the
Windows and Linux renders use the full tile so the magpie reads at 16–48 px.

## Provenance

Original work: drawn directly as SVG paths (cubic Béziers, a few circles and one rectangle) by the
Dalica contributors (an AI agent, M6.12), checked by eye at 16, 32, 48 and 512 px. No tracing, no
source raster, no third-party material; it imitates no other product's icon. Licence:
`LICENSE.txt` (MIT OR Apache-2.0, like the repository).

## Files

| File | What |
|---|---|
| `dalica.svg` | the canonical artwork (hand-written, about 1.6 KB) |
| `dalica-1024.png` | 1024 px render on the macOS grid; the runtime window and Dock icon on macOS |
| `dalica.icns` | macOS bundle icon, 16–1024 px (PNG entries) |
| `dalica.ico` | Windows icon, 16–256 px (PNG entries), for the `.exe` resource and the installer |
| `hicolor/<n>x<n>/apps/org.dalica.Dalica.png` | Linux icon theme, 16–512 px; the 256 px one is the runtime window icon on Windows and Linux |
| `hicolor/scalable/apps/org.dalica.Dalica.svg` | Linux scalable icon (a copy of `dalica.svg`) |

The icon name is the application id, `org.dalica.Dalica` (`crates/core/src/app_id.rs`): the
Wayland app id and X11 class, the `.desktop` file's `Icon=` and the hicolor file name agree. The
window sets its icon from the embedded PNG (`apps/dalica/src/icon.rs`); its tests check every file
here exists at its size and decodes.

## Regenerate

Edit `dalica.svg`, then run `packaging/icons.sh`. It needs ImageMagick 7 with its librsvg SVG
coder (made with ImageMagick 7.1.2-32 and librsvg 2.62.3) and python3 (standard library only),
and rewrites every other file here; commit them all. The output is deterministic: running it twice
gives identical bytes, and `packaging/icons.sh --check` renders into a temporary folder and fails
if any committed file differs. Another ImageMagick or librsvg version may antialias slightly
differently; regenerate everything with one version.
