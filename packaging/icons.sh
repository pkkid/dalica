#!/usr/bin/env bash
# Regenerate every app icon file from assets/app-icon/dalica.svg (the canonical master).
#
# Adapted from LightCraft packaging/icons.sh @ 75e4e3ce and PhotoCraft packaging/icons.sh
# @ 148b1b44 (MIT OR Apache-2.0): the same outputs and viewBox trick, rendered with ImageMagick
# instead of resvg and packed by a small Python script instead of `cargo xtask ico` / iconutil, so
# it runs on Linux and writes the .icns too.
#
# Needs: ImageMagick 7 (`magick`) with its librsvg SVG coder (checked: 7.1.2 with librsvg 2.62),
# and python3 (3.8 or later, standard library only). The outputs are committed, so building and
# packaging never need these tools. The output is deterministic: no time stamps or other
# metadata in the PNGs, entries packed in a fixed order.
#
#   packaging/icons.sh            rewrite every derived file
#   packaging/icons.sh --check    render into a temporary folder and compare with the committed
#                                 files (exit 1 on any difference)
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/assets/app-icon"
SVG="$SRC/dalica.svg"
ID="org.dalica.Dalica"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

CHECK=0
case "${1:-}" in
  "") ;;
  --check) CHECK=1 ;;
  *) echo "usage: packaging/icons.sh [--check]" >&2; exit 2 ;;
esac
if [ "$CHECK" = 1 ]; then OUT="$TMP/out"; else OUT="$SRC"; fi
mkdir -p "$OUT"

command -v magick >/dev/null || { echo "error: magick not found (ImageMagick 7)" >&2; exit 1; }
command -v python3 >/dev/null || { echo "error: python3 not found" >&2; exit 1; }
magick -list format | grep -qE '^ *SVG .*RSVG' ||
  { echo "error: this ImageMagick renders SVG without librsvg; install its rsvg coder" >&2; exit 1; }

# The SVG is the full-bleed 512-unit tile (rx=112). Windows and Linux use it as is, so the shutter
# reads at 16-48 px. macOS icons follow Apple's grid: an 824/1024 body with a transparent margin,
# made by widening the viewBox (512 / 0.805 = 636, so 62 units each side).
grep -q 'viewBox="0 0 512 512"' "$SVG" || { echo "error: expected viewBox=\"0 0 512 512\" in $SVG" >&2; exit 1; }
MAC="$TMP/macos.svg"
sed 's/viewBox="0 0 512 512"/viewBox="-62 -62 636 636"/' "$SVG" >"$MAC"

# 8-bit RGBA, no date, time or other text chunks: the same SVG gives the same bytes.
render() {
  magick -background none -size "$2x$2" "$1" -depth 8 -strip \
    -define png:exclude-chunks=date,time PNG32:"$3" </dev/null
}

render "$MAC" 1024 "$OUT/dalica-1024.png"

# Linux hicolor theme (the 256 px one is also the window icon on Windows and Linux).
for s in 16 24 32 48 64 128 256 512; do
  mkdir -p "$OUT/hicolor/${s}x${s}/apps"
  render "$SVG" "$s" "$OUT/hicolor/${s}x${s}/apps/$ID.png"
done
mkdir -p "$OUT/hicolor/scalable/apps"
# The canonical SVG is small (hand-written paths), so it is the scalable theme icon as it is.
cp "$SVG" "$OUT/hicolor/scalable/apps/$ID.svg"

# Windows .ico and macOS .icns, every entry a PNG.
for s in 16 20 24 32 40 48 64 128 256; do render "$SVG" "$s" "$TMP/ico-$s.png"; done
for s in 16 32 64 128 256 512 1024; do render "$MAC" "$s" "$TMP/mac-$s.png"; done
python3 - "$TMP" "$OUT/dalica.ico" "$OUT/dalica.icns" <<'PY'
import struct, sys
tmp, ico_path, icns_path = sys.argv[1:4]

def read(name):
    with open(f"{tmp}/{name}", "rb") as f:
        return f.read()

# ICO: ICONDIR, then one 16-byte ICONDIRENTRY per image (0 means 256), then the PNGs.
sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
pngs = [read(f"ico-{s}.png") for s in sizes]
out = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
for s, png in zip(sizes, pngs):
    d = 0 if s == 256 else s
    out += struct.pack("<BBBBHHII", d, d, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
out += b"".join(pngs)
with open(ico_path, "wb") as f:
    f.write(out)

# ICNS: "icns" and the total length, then (type, length, PNG) entries, big-endian.
entries = [("icp4", 16), ("icp5", 32), ("ic11", 32), ("icp6", 64), ("ic12", 64), ("ic07", 128),
           ("ic08", 256), ("ic13", 256), ("ic09", 512), ("ic14", 512), ("ic10", 1024)]
body = b""
for kind, s in entries:
    png = read(f"mac-{s}.png")
    body += kind.encode("ascii") + struct.pack(">I", 8 + len(png)) + png
with open(icns_path, "wb") as f:
    f.write(b"icns" + struct.pack(">I", 8 + len(body)) + body)
PY

if [ "$CHECK" = 1 ]; then
  status=0
  while IFS= read -r f; do
    rel="${f#"$OUT"/}"
    cmp -s "$f" "$SRC/$rel" || { echo "differs: assets/app-icon/$rel" >&2; status=1; }
  done < <(find "$OUT" -type f | LC_ALL=C sort)
  [ "$status" = 0 ] && echo "every derived icon matches $SVG"
  exit "$status"
fi
echo "icons written to $OUT"
