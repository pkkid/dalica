# Dalica app icon

**Status: chosen.** The icon's design is the maintainer's decision (`docs/assets.md` §7); the
maintainer picked this one on 2026-10-12 from 32 variants and a round of centred versions,
replacing the M6.12 magpie candidate.

## Motif

A **melting camera shutter**: a lens barrel, centred in the tile, whose lower edge softens and runs
down in four short drips, a nod to the surrealist painter's soft watches. Inside it, a six-blade iris
stands half open on a small landscape (a sun over a dark hill). Flat shapes with no fine line work,
so the ring, the blades and the drips still read at 16 px. It is one colour field, ink and paper,
never a multicoloured aperture, so it does not resemble the reference organizer's icon.

## Palette

Exactly three colours:

| Colour | Hex | Used for |
|---|---|---|
| Ink | `#0b0b0c` | the barrel and its drips, the gaps between the blades, the hill |
| Paper | `#efe9dc` | the iris blades, the sun |
| Dalica orange (app colour; Gruvbox's orange) | `#d65d0e` | the full-bleed field and the sky in the opening |

Ink and paper are the Craft family's; the orange sets Dalica apart from its siblings' amber and blue.

## Geometry

A 512-unit tile (`viewBox="0 0 512 512"`), a rounded square with `rx=112` that clips everything,
no border. The macOS renders (`dalica-1024.png`, `dalica.icns`) widen the viewBox to
`-62 -62 636 636`, putting the tile on Apple's 824/1024 grid with a transparent margin; the
Windows and Linux renders use the full tile so the shutter reads at 16–48 px.

## Provenance

Original work by the Dalica contributors (an AI agent, 2026-10-12): the paths were computed by a
short script (the iris blades from a hexagonal opening, each blade edge extended to the barrel's
inner circle; the drips as rounded bars joined to the barrel by curved fillets) and written out as
plain SVG, then checked by eye at 16, 32, 48 and 512 px. The lens is centred at (256, 256) with a
168-unit outer radius. No tracing, no
source raster, no third-party material; it imitates no other product's icon. Licence:
`LICENSE.txt` (MIT OR Apache-2.0, like the repository).

## Files

| File | What |
|---|---|
| `dalica.svg` | the canonical artwork (plain paths, circles and polygons, about 3.8 KB) |
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
