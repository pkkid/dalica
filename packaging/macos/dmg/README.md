# macOS DMG window

What Finder shows when Dalica's DMG opens: a 660 × 400 pt window with `Dalica.app` and the
`Applications` link side by side on a background drawn from the app icon. Nothing here is a
committed image: `generate.py` writes both layout files at every build, `package.sh` copies them
into the image, and `verify.sh` checks that the mounted DMG carries exactly what the generator
writes.

| File | What |
|---|---|
| `generate.py OUT_DIR` | writes `OUT_DIR/background.tiff` and `OUT_DIR/DS_Store`; Python 3.8 or later, standard library only, so it runs on a bare macOS runner and on Linux |
| `background.tiff` (generated) | 1x (660 × 400 px, 72 dpi) and 2x (1320 × 800 px, 144 dpi) pages in one uncompressed RGB TIFF, which Finder treats as one HiDPI image; goes to `.background/background.tiff` (about 3.9 MB before the DMG's compression) |
| `DS_Store` (generated) | Finder's view settings for the volume: window bounds, no toolbar, sidebar or status bar, icon size 128, labels at 16 pt, `Dalica.app` at (326, 205), `Applications` at (574, 205), and the background; goes to `.DS_Store` |

## The background

Drawn from the icon's own material (`assets/app-icon/README.md`): the left 232 pt are the icon's
orange field with the melting shutter cropped from `assets/app-icon/dalica-1024.png` as a
cover, centred on the lens (area-averaged, composited on the orange so the tile's corners
disappear), edged by an ink rule and hairline; the rest is Paper with two ink hairlines framing the
icon row, an orange diamond
on each, and an ink arrow from the app to `Applications`. No text, so no fonts are involved.

- **Finder draws the icon labels in black in light and dark mode** when a window has a
  background, so the area under both icons stays Paper.
- **Nothing goes inside the icon boxes:** the artwork keeps clear of each 128 pt icon box and of
  the label strip under it.
- The TIFF carries no colour profile: Finder shows it untagged.

## The volume name has no version

The mounted volume is called `Dalica`, not `Dalica <version>`. The layout points at the
background through an alias that includes the volume name, and Finder resolves it by that name:
with a versioned name the window keeps its size and icon positions but shows no background (the
Craft apps tested this). The DMG file name (`dalica-<version>-macos-universal.dmg`) keeps the
version. `verify.sh` fails if the mounted volume has another name.

## Reproducible

Both files depend only on `generate.py` and the icon PNG: no dates, paths, user, disk or volume
ids (the alias holds only the volume name, the background's path and a fixed date), no
compression library (the TIFF is uncompressed; the DMG compresses it), only integer and IEEE
arithmetic. Two runs give identical bytes, on any OS and Python version: `package.sh` generates
the layout twice and compares the files before building the DMG, and `test.sh` does the same on
any OS. Python 3.8, 3.11 and 3.14 on Linux give the same SHA-256 for both files.

The `.DS_Store` writer (a buddy-allocated B-tree with one leaf node) and the alias record (version
2) follow the structure the `ds_store` and `mac_alias` Python packages write. Its self-test,
`generate.py --self-test`, writes LightCraft's and PhotoCraft's layouts from their parameters and
compares the SHA-256 with the `dmg-layout.DS_Store` files those projects commit (made with
`ds_store` 1.3.3 and `mac_alias` 2.2.3): byte for byte equal.

## Change it

Edit the constants or drawing in `generate.py` (window geometry, icon positions, palette, the
crop of the icon, the arrow), run `packaging/macos/test.sh`, and look at the result, for example
by converting both TIFF pages to PNG with any image tool. Keep the names in step with
`package.sh` (`VOLUME`, `APP`).
