# The offline map

`land-110m.bin` is the map's offline layer (`docs/organizing.md` §7, `docs/design-system.md` §6
`map`): the world's land at 1:110 million, drawn when no map tiles are shown so that geotagging by
drag works without the network. It is compiled into `dalica-ui-kit`
(`crates/ui-kit/src/widgets/map/land.rs`).

## Source and licence

- **Data:** Natural Earth, 1:110m physical vectors, land (`ne_110m_land`), version 5.1.2.
- **File used:** `geojson/ne_110m_land.geojson` at tag `v5.1.2` of
  https://github.com/nvkelso/natural-earth-vector (SHA-256
  `9e0729ee253ca7d7a5c4ae9395fb1902264c5377c52e224d13dd85010e2835d9`).
- **Licence:** public domain. Natural Earth's terms of use
  (https://www.naturalearthdata.com/about/terms-of-use/) place all of its vector and raster data
  in the public domain: it may be used, modified and distributed for any purpose without
  permission, and crediting it is optional. Dalica credits it anyway, in `ATTRIBUTION.md` and in
  the map's attribution line.

## Rebuilding it

```sh
python3 assets/maps/convert.py ne_110m_land.geojson assets/maps/land-110m.bin
```

The script needs only Python's standard library. It prints the rings, points and bytes written
(128 rings, 5,001 points, 20,398 bytes for v5.1.2).

## Format

Little-endian throughout:

| Bytes | Value |
|---|---|
| 8 | the magic `DLLAND01` |
| 2 | `u16` ring count |
| per ring: 1 | `u8` kind: 0 land, 1 a lake (a hole in the land ring before it) |
| per ring: 2 | `u16` point count, at least 3 |
| per ring: 4 per point | `i16` longitude, `i16` latitude, in hundredths of a degree |

Each ring is a polygon's outline with the closing point left out (the last point joins the
first); repeated points are dropped. The reader refuses anything else: a wrong magic, a kind that
is not 0 or 1, fewer than 3 or more than 16,384 points in a ring, more than 65,536 in all, a point
off the globe, a file that ends early or carries bytes after its last ring, or more than 256 KB.
