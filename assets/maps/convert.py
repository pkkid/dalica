#!/usr/bin/env python3
"""Builds `land-110m.bin`, the map's offline layer, from Natural Earth's 1:110m land GeoJSON.

Usage: python3 assets/maps/convert.py ne_110m_land.geojson assets/maps/land-110m.bin

The input is `geojson/ne_110m_land.geojson` of https://github.com/nvkelso/natural-earth-vector at
tag v5.1.2 (public domain). Standard library only. The format is described in `README.md` next to
this script and read by `crates/ui-kit/src/widgets/map/land.rs`.
"""

import json
import struct
import sys

MAGIC = b"DLLAND01"
SCALE = 100  # hundredths of a degree


def quantise(ring):
    """The ring's points in hundredths of a degree, without the closing repeat or duplicates."""
    out = []
    for lon, lat in ring:
        q = (max(-18000, min(18000, round(lon * SCALE))), max(-9000, min(9000, round(lat * SCALE))))
        if not out or out[-1] != q:
            out.append(q)
    while len(out) > 1 and out[0] == out[-1]:
        out.pop()
    return out


def main(src, dst):
    with open(src, encoding="utf-8") as f:
        data = json.load(f)
    rings = []
    for feature in data["features"]:
        geometry = feature["geometry"]
        polygons = [geometry["coordinates"]] if geometry["type"] == "Polygon" else geometry["coordinates"]
        for polygon in polygons:
            for i, ring in enumerate(polygon):
                points = quantise(ring)
                if len(points) >= 3:
                    # The first ring of a polygon is land, the others are holes in it (lakes).
                    rings.append((0 if i == 0 else 1, points))
    out = bytearray(MAGIC)
    out += struct.pack("<H", len(rings))
    for kind, points in rings:
        out += struct.pack("<BH", kind, len(points))
        for lon, lat in points:
            out += struct.pack("<hh", lon, lat)
    with open(dst, "wb") as f:
        f.write(out)
    print(f"{len(rings)} rings, {sum(len(p) for _, p in rings)} points, {len(out)} bytes")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
