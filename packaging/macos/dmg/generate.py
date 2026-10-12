#!/usr/bin/env python3
"""Write the Finder window layout of Dalica's DMG: `background.tiff` and `DS_Store`.

    python3 packaging/macos/dmg/generate.py OUT_DIR   write OUT_DIR/background.tiff and OUT_DIR/DS_Store
    python3 packaging/macos/dmg/generate.py --self-test

Standard library only (Python 3.8 or later), so it runs on any OS and on a bare macOS runner, and
`package.sh` generates the layout at every build instead of committing binary files. The output
depends on nothing but this script and `assets/app-icon/dalica-1024.png`: no time stamps, paths,
user, disk or volume ids, no compression library (the TIFF is uncompressed; the DMG compresses it),
only integer and IEEE arithmetic. Two runs give identical bytes on every machine.

`--self-test` writes the `.DS_Store` of LightCraft's and PhotoCraft's DMGs from their own layout
and checks the bytes against the files those projects commit (made by the `ds_store` 1.3.3 and
`mac_alias` 2.2.3 Python packages), then checks that two renders of Dalica's layout are identical.

Adapted from PhotoCraft packaging/macos/dmg/generate.py @ 77221725 (MIT OR Apache-2.0; the same
file is in LightCraft @ 9978dc73): the window geometry, the Finder view settings and the background
alias, which hold only the volume name and the background's path. Changed for Dalica: the
`.DS_Store` B-tree and the alias record are written here from their published structure rather
than by the `ds_store` and `mac_alias` packages, and the background is drawn here from the app
icon's PNG rather than rendered from an SVG with resvg and Pillow. See README.md in this folder.
"""

import datetime
import hashlib
import os
import plistlib
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
ICON = os.path.join(ROOT, "assets", "app-icon", "dalica-1024.png")

# Keep in sync with package.sh (volume and app names) and README.md (layout).
VOLUME = "Dalica"
APP = "Dalica.app"
WIDTH, HEIGHT = 660, 400  # window content, pt; the background's 1x size in pixels
TITLE_BAR = 32
ICON_SIZE = 128
ICONS = ((APP, (326, 205)), ("Applications", (574, 205)))
# A fixed date for the alias. Finder never matches it against the image (each build is a new
# volume): it finds the background by volume name and path.
FIXED_DATE = datetime.datetime(2026, 10, 8, tzinfo=datetime.timezone.utc)

# The palette of the app icon (assets/app-icon/README.md).
INK = (0x0B, 0x0B, 0x0C)
PAPER = (0xEF, 0xE9, 0xDC)
TEAL = (0x1F, 0xA3, 0x8A)
# The left panel, pt: the icon's colour field with the magpie cropped as a cover.
PANEL = 232
# The crop of the 512-unit icon tile shown in the panel: its full height, and this many units
# from the tile's left edge (the print, the head and the front of the body).
CROP_LEFT = 4

# The two Craft apps whose committed .DS_Store files the self-test reproduces byte for byte.
ORACLES = (
    ("LightCraft", "e68985cbb8ee03b5b8858545218ee7d37b61024d9a2a90742c76e3085c85bbde"),
    ("PhotoCraft", "6c10d83180cd1b805f3077485b5614e688fb6be15435205531b214b2cabd6060"),
)


class Failure(Exception):
    """A refused input or a failed check; main() prints it and exits 1."""


# ---- the background alias -------------------------------------------------------------------


def hfs_seconds(when):
    """Seconds since 1904-01-01 UTC, the classic Mac OS epoch."""
    epoch = datetime.datetime(1904, 1, 1, tzinfo=datetime.timezone.utc)
    return (when - epoch).days * 86400 + (when - epoch).seconds


def pascal(text, size):
    raw = text.encode("utf-8")
    if len(raw) >= size:
        raise Failure(f"{text!r} is too long for a {size}-byte alias field")
    return bytes([len(raw)]) + raw + bytes(size - 1 - len(raw))


def alias_tag(tag, data):
    pad = b"\0" if len(data) % 2 else b""
    return struct.pack(">hH", tag, len(data)) + data + pad


def utf16_counted(text):
    return struct.pack(">H", len(text)) + text.encode("utf-16-be")


def background_alias(volume):
    """A version 2 alias record of /.background/background.tiff on the volume `volume`.

    What Finder stores for a window background, minus anything about the machine. Disk type 5
    (ejectable), the volume flags and the folder and file ids (25, 26) are what Finder wrote on a
    test image; the ids are only hints, as the dates are.
    """
    date = hfs_seconds(FIXED_DATE)
    folder, name = ".background", "background.tiff"
    record = struct.pack(
        ">h28sI2shI64sII4s4shhI2s10s",
        0,  # kind: a file
        pascal(volume, 28),
        date,
        b"H+",
        5,
        25,  # the folder's id
        pascal(name, 64),
        26,  # the file's id
        date,
        bytes(4),  # creator
        bytes(4),  # type
        -1,  # levels from
        -1,  # levels to
        0x0D02,  # volume attributes
        bytes(2),  # volume file system id
        bytes(10),
    )
    hires = struct.pack(">Q", date << 16)
    tags = b"".join(
        (
            alias_tag(0, folder.encode("utf-8")),
            alias_tag(16, hires),  # the volume's creation date
            alias_tag(17, hires),  # the file's creation date
            alias_tag(1, struct.pack(">I", 25)),  # the folder ids from the root
            alias_tag(2, f"{volume}:{folder}:\0{name}".encode("utf-8")),
            alias_tag(14, utf16_counted(name)),
            alias_tag(15, utf16_counted(volume)),
            alias_tag(18, f"/{folder}/{name}".encode("utf-8")),
            alias_tag(19, f"/Volumes/{volume}".encode("utf-8")),
            struct.pack(">hH", -1, 0),
        )
    )
    length = 8 + len(record) + len(tags)
    return struct.pack(">4sHh", bytes(4), length, 2) + record + tags


# ---- .DS_Store ------------------------------------------------------------------------------

PAGE_SIZE = 4096
# The 16 bytes after the header's offsets, as Finder and the ds_store package write them.
HEADER_TAIL = bytes.fromhex("0000100c000000870000200b00000000")


def ds_record(filename, code, kind, value):
    """One record: the file name (UTF-16), a four-character code, its type and the value."""
    head = struct.pack(">I", len(filename)) + filename.encode("utf-16-be") + code + kind
    if kind == b"blob":
        return head + struct.pack(">I", len(value)) + value
    if kind == b"long":
        return head + struct.pack(">I", value)
    raise Failure(f"unsupported .DS_Store record type {kind!r}")


class Buddy:
    """The buddy allocator of the .DS_Store file: power-of-two blocks, lowest offset first."""

    def __init__(self):
        self.free = {k: [] for k in range(32)}
        self.free[31] = [0]

    def take(self, width, offset=None):
        k = width
        while k < 32 and not self.free[k]:
            k += 1
        if k == 32:
            raise Failure(".DS_Store allocator is full")
        block = self.free[k].pop(0)
        while k > width:  # split, keeping the lower half
            k -= 1
            self.free[k].append(block + (1 << k))
            self.free[k].sort()
        if offset is not None and block != offset:
            raise Failure(".DS_Store allocator: the header block is not at offset 0")
        return block

    def allocate(self, size):
        # As the ds_store package sizes blocks: bit_length, so 4096 bytes take an 8192 block.
        width = max(size.bit_length(), 5)
        return self.take(width) | width, width


def ds_store(records):
    """A whole .DS_Store file holding `records` [(filename, code, type, value)] in one leaf node."""
    entries = sorted(records, key=lambda r: (r[0].lower(), r[1]))
    node = struct.pack(">II", 0, len(entries)) + b"".join(ds_record(*r) for r in entries)
    if len(node) > PAGE_SIZE:
        raise Failure(f".DS_Store records take {len(node)} bytes, more than one {PAGE_SIZE}-byte node")
    dsdb = struct.pack(">IIIII", 2, 0, len(entries), 1, PAGE_SIZE)

    alloc = Buddy()
    alloc.take(5, offset=0)  # the file header
    dsdb_addr, dsdb_width = alloc.allocate(len(dsdb))
    node_addr, node_width = alloc.allocate(PAGE_SIZE)

    def info_block(info_addr):
        addrs = [info_addr, dsdb_addr, node_addr]
        out = struct.pack(">II", len(addrs), 0)
        out += struct.pack(">256I", *(addrs + [0] * (256 - len(addrs))))
        out += struct.pack(">I", 1) + bytes([4]) + b"DSDB" + struct.pack(">I", 1)
        for k in range(32):
            out += struct.pack(">I", len(alloc.free[k]))
            out += b"".join(struct.pack(">I", o) for o in alloc.free[k])
        return out

    # The allocator's own block lists the free blocks left after every allocation, its own
    # included, so it is allocated last, sized by its contents beforehand, and written afterwards
    # (the check below refuses a block that outgrew its size).
    info_addr, info_width = alloc.allocate(len(info_block(0)))
    info = info_block(info_addr)
    if len(info) > (1 << info_width):
        raise Failure(".DS_Store allocator block overflow")

    blocks = (
        (info_addr & ~0x1F, info_width, info),
        (dsdb_addr & ~0x1F, dsdb_width, dsdb),
        (node_addr & ~0x1F, node_width, node),
    )
    end = max(off + (1 << w) for off, w, _ in blocks)
    body = bytearray(end)
    info_off = info_addr & ~0x1F
    body[0:32] = b"Bud1" + struct.pack(">III", info_off, 1 << info_width, info_off) + HEADER_TAIL
    for off, _, data in blocks:
        body[off : off + len(data)] = data
    return struct.pack(">I", 1) + bytes(body)


def layout_ds_store(volume, app):
    bwsp = {
        "ShowStatusBar": False,
        "ShowToolbar": False,
        "ShowTabView": False,
        "ContainerShowSidebar": False,
        "ShowSidebar": False,
        "WindowBounds": f"{{{{200, 528}}, {{{WIDTH}, {HEIGHT + TITLE_BAR}}}}}",
    }
    icvp = {
        "viewOptionsVersion": 1,
        "backgroundType": 2,
        "backgroundImageAlias": background_alias(volume),
        "backgroundColorRed": 1.0,
        "backgroundColorGreen": 1.0,
        "backgroundColorBlue": 1.0,
        "gridOffsetX": 0.0,
        "gridOffsetY": 0.0,
        "gridSpacing": 100.0,
        "arrangeBy": "none",
        "showIconPreview": True,
        "showItemInfo": False,
        "labelOnBottom": True,
        "textSize": 16.0,
        "iconSize": float(ICON_SIZE),
    }

    def plist(value):
        return plistlib.dumps(value, fmt=plistlib.FMT_BINARY, sort_keys=True)

    records = [
        (".", b"bwsp", b"blob", plist(bwsp)),
        (".", b"icvp", b"blob", plist(icvp)),
        (".", b"vSrn", b"long", 1),
    ]
    for name, (x, y) in ICONS:
        name = app if name == APP else name
        records.append((name, b"Iloc", b"blob", struct.pack(">II", x, y) + bytes.fromhex("ffffffffffff0000")))
    return ds_store(records)


# ---- the app icon (PNG) ---------------------------------------------------------------------


def read_png_rgba(path):
    """(width, height, RGBA bytes) of an 8-bit RGBA, non-interlaced PNG: what icons.sh writes."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise Failure(f"{path}: not a PNG")
    pos, idat, ihdr = 8, [], None
    while pos + 8 <= len(data):
        length, kind = struct.unpack(">I4s", data[pos : pos + 8])
        body = data[pos + 8 : pos + 8 + length]
        if len(body) != length:
            raise Failure(f"{path}: truncated {kind!r} chunk")
        pos += 12 + length
        if kind == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    if ihdr is None:
        raise Failure(f"{path}: no IHDR")
    width, height, depth, colour, _, _, interlace = ihdr
    if (depth, colour, interlace) != (8, 6, 0) or not (0 < width <= 4096 and 0 < height <= 4096):
        raise Failure(f"{path}: expected an 8-bit RGBA non-interlaced PNG up to 4096 px, got {ihdr}")
    raw = zlib.decompress(b"".join(idat))
    stride = width * 4
    if len(raw) != height * (stride + 1):
        raise Failure(f"{path}: image data has {len(raw)} bytes, expected {height * (stride + 1)}")
    out = bytearray(height * stride)
    prev = bytearray(stride)
    for y in range(height):
        kind = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        if kind == 1:
            for i in range(4, stride):
                line[i] = (line[i] + line[i - 4]) & 0xFF
        elif kind == 2:
            line = bytearray((a + b) & 0xFF for a, b in zip(line, prev))
        elif kind == 3:
            for i in range(stride):
                left = line[i - 4] if i >= 4 else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif kind == 4:
            for i in range(stride):
                a = line[i - 4] if i >= 4 else 0
                b = prev[i]
                c = prev[i - 4] if i >= 4 else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        elif kind != 0:
            raise Failure(f"{path}: unknown PNG filter {kind} on row {y}")
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, out


# ---- the background -------------------------------------------------------------------------


class Canvas:
    """An RGB image with exact-coverage rectangles and supersampled polygons."""

    def __init__(self, width, height, colour):
        self.width, self.height = width, height
        self.px = bytearray(bytes(colour) * (width * height))

    def blend(self, x, y, colour, cover):
        """Mix `colour` into pixel (x, y) by `cover`/256."""
        if cover <= 0:
            return
        i = (y * self.width + x) * 3
        for c in range(3):
            self.px[i + c] = (colour[c] * cover + self.px[i + c] * (256 - cover) + 128) >> 8

    def rect(self, x0, y0, x1, y1, colour):
        """Fill [x0, x1) × [y0, y1) in pixels (floats), antialiased by the exact covered area."""
        for y in range(max(int(y0), 0), min(int(-(-y1 // 1)), self.height)):
            cy = min(y1, y + 1) - max(y0, y)
            for x in range(max(int(x0), 0), min(int(-(-x1 // 1)), self.width)):
                cx = min(x1, x + 1) - max(x0, x)
                self.blend(x, y, colour, int(cx * cy * 256 + 0.5))

    def polygon(self, points, colour, samples=4):
        """Fill a polygon (even-odd) with `samples`² samples per pixel."""
        xs = [p[0] for p in points]
        ys = [p[1] for p in points]
        edges = list(zip(points, points[1:] + points[:1]))
        for y in range(max(int(min(ys)), 0), min(int(max(ys)) + 1, self.height)):
            for x in range(max(int(min(xs)), 0), min(int(max(xs)) + 1, self.width)):
                hits = 0
                for sy in range(samples):
                    py = y + (sy + 0.5) / samples
                    for sx in range(samples):
                        px = x + (sx + 0.5) / samples
                        inside = False
                        for (ax, ay), (bx, by) in edges:
                            if (ay > py) != (by > py) and px < ax + (py - ay) * (bx - ax) / (by - ay):
                                inside = not inside
                        hits += inside
                self.blend(x, y, colour, (hits * 256 + samples * samples // 2) // (samples * samples))


def resample_weights(src_start, ratio, count, src_limit):
    """Area-average weights: for each destination pixel, [(source index, weight/65536)]."""
    table = []
    for i in range(count):
        a = src_start + i * ratio
        b = a + ratio
        taps = []
        j = int(a)
        while j < b and j < src_limit:
            overlap = min(b, j + 1) - max(a, j)
            if overlap > 0 and j >= 0:
                taps.append((j, overlap))
            j += 1
        total = sum(w for _, w in taps)
        ints = [(j, int(w / total * 65536 + 0.5)) for j, w in taps]
        # The integer weights sum to exactly 65536: the rounding error goes to the largest tap.
        drift = 65536 - sum(w for _, w in ints)
        big = max(range(len(ints)), key=lambda k: ints[k][1])
        ints[big] = (ints[big][0], ints[big][1] + drift)
        table.append(ints)
    return table


def panel_art(icon, width, height):
    """The icon tile's field and magpie, cropped as a cover, `width` × `height` RGB pixels."""
    iw, ih, rgba = icon
    # icons.sh renders the macOS icon from the viewBox -62 -62 636 636: tile unit u is at
    # (u + 62) × size / 636 px.
    unit = iw / 636.0
    ratio = (512.0 * unit) / height  # source px per destination px: the tile's full height
    x0 = (62 + CROP_LEFT) * unit
    y0 = 62 * unit
    # The icon composited onto its own field colour, so the tile's rounded corners vanish.
    flat = bytearray(iw * ih * 3)
    for i in range(iw * ih):
        a = rgba[i * 4 + 3]
        for c in range(3):
            flat[i * 3 + c] = (rgba[i * 4 + c] * a + TEAL[c] * (255 - a) + 127) // 255
    cols = resample_weights(x0, ratio, width, iw)
    rows = resample_weights(y0, ratio, height, ih)
    out = bytearray(width * height * 3)
    for y, row_taps in enumerate(rows):
        line = [0] * (width * 3)
        for sy, wy in row_taps:
            base = sy * iw * 3
            for x, col_taps in enumerate(cols):
                r = g = b = 0
                for sx, wx in col_taps:
                    k = base + sx * 3
                    r += flat[k] * wx
                    g += flat[k + 1] * wx
                    b += flat[k + 2] * wx
                line[x * 3] += r * wy
                line[x * 3 + 1] += g * wy
                line[x * 3 + 2] += b * wy
        half = 1 << 31
        out[y * width * 3 : (y + 1) * width * 3] = bytes(min((v + half) >> 32, 255) for v in line)
    return out


def background(icon, scale):
    """The window background at `scale` (1 or 2) as a Canvas."""
    s = scale
    canvas = Canvas(WIDTH * s, HEIGHT * s, PAPER)
    pw = PANEL * s
    art = panel_art(icon, pw, HEIGHT * s)
    for y in range(HEIGHT * s):
        a = y * canvas.width * 3
        canvas.px[a : a + pw * 3] = art[y * pw * 3 : (y + 1) * pw * 3]
    # The panel's edge: a rule and a hairline in ink, as the Craft apps' DMG windows have.
    canvas.rect(PANEL * s, 0, (PANEL + 1.2) * s, HEIGHT * s, INK)
    canvas.rect((PANEL + 4) * s, 0, (PANEL + 4.5) * s, HEIGHT * s, INK)
    # Hairlines above and below the icon row; nothing inside the icon boxes or their labels.
    for y in (88, 335):
        canvas.rect(267 * s, y * s, 624 * s, (y + 0.6) * s, INK)
        canvas.polygon([(267 * s, (y + 0.3) * s), (271.5 * s, (y - 6.7) * s),
                        (276 * s, (y + 0.3) * s), (271.5 * s, (y + 7.3) * s)], TEAL)
    # The arrow from the app to Applications, between the two 128 pt icon boxes.
    arrow = [(410, 202.5), (466, 201), (464, 191), (490, 205), (464, 219), (466, 209), (410, 207.5)]
    canvas.polygon([(x * s, y * s) for x, y in arrow], INK)
    return canvas


def tiff(pages):
    """A baseline RGB TIFF (little-endian, uncompressed, one strip per page) of `pages`:
    [(width, height, dpi, rgb bytes)]. Finder takes the 72 and 144 dpi pages as 1x and 2x."""
    entries_per_ifd = 13
    ifd_size = 2 + entries_per_ifd * 12 + 4
    extra_size = 6 + 16  # BitsPerSample (3 SHORTs) and two RATIONALs
    header = 8
    meta = header + len(pages) * (ifd_size + extra_size)
    out = bytearray()
    out += b"II*\0" + struct.pack("<I", header)
    data_offset = meta
    for n, (w, h, dpi, rgb) in enumerate(pages):
        if len(rgb) != w * h * 3:
            raise Failure("TIFF page size mismatch")
        ifd_at = header + n * (ifd_size + extra_size)
        extra = ifd_at + ifd_size
        last = n == len(pages) - 1
        tags = [
            (256, 4, 1, w),  # ImageWidth
            (257, 4, 1, h),  # ImageLength
            (258, 3, 3, extra),  # BitsPerSample 8, 8, 8
            (259, 3, 1, 1),  # Compression: none
            (262, 3, 1, 2),  # PhotometricInterpretation: RGB
            (273, 4, 1, data_offset),  # StripOffsets
            (277, 3, 1, 3),  # SamplesPerPixel
            (278, 4, 1, h),  # RowsPerStrip
            (279, 4, 1, len(rgb)),  # StripByteCounts
            (282, 5, 1, extra + 6),  # XResolution
            (283, 5, 1, extra + 14),  # YResolution
            (284, 3, 1, 1),  # PlanarConfiguration: chunky
            (296, 3, 1, 2),  # ResolutionUnit: inch
        ]
        out += struct.pack("<H", len(tags))
        for tag, typ, count, value in tags:
            if typ == 3 and count == 1:
                out += struct.pack("<HHIHH", tag, typ, count, value, 0)
            else:
                out += struct.pack("<HHII", tag, typ, count, value)
        out += struct.pack("<I", 0 if last else ifd_at + ifd_size + extra_size)
        out += struct.pack("<3H", 8, 8, 8) + struct.pack("<4I", dpi, 1, dpi, 1)
        data_offset += len(rgb)
    for _, _, _, rgb in pages:
        out += rgb
    return bytes(out)


def background_tiff(icon_path):
    icon = read_png_rgba(icon_path)
    two = background(icon, 2)
    # 1x is the 2x image averaged over 2 × 2 blocks, so both pages show the same picture.
    w, h = WIDTH, HEIGHT
    one = bytearray(w * h * 3)
    src, sw = two.px, two.width
    for y in range(h):
        for x in range(w):
            a = ((2 * y) * sw + 2 * x) * 3
            b = a + sw * 3
            for c in range(3):
                one[(y * w + x) * 3 + c] = (src[a + c] + src[a + 3 + c] + src[b + c] + src[b + 3 + c] + 2) >> 2
    return tiff([(w, h, 72, bytes(one)), (two.width, two.height, 144, bytes(two.px))])


# ---- entry points ---------------------------------------------------------------------------


def write(out_dir, icon_path):
    os.makedirs(out_dir, exist_ok=True)
    outputs = (("background.tiff", background_tiff(icon_path)), ("DS_Store", layout_ds_store(VOLUME, APP)))
    for name, data in outputs:
        path = os.path.join(out_dir, name)
        with open(path, "wb") as f:
            f.write(data)
        print(f"{hashlib.sha256(data).hexdigest()}  {path} ({len(data)} bytes)")


def self_test(icon_path):
    for name, want in ORACLES:
        got = hashlib.sha256(layout_ds_store(name, f"{name}.app")).hexdigest()
        if got != want:
            raise Failure(f"the {name} .DS_Store differs from the committed one: {got} != {want}")
        print(f"ok: {name}'s dmg-layout.DS_Store reproduced byte for byte")
    a, b = layout_ds_store(VOLUME, APP), layout_ds_store(VOLUME, APP)
    if a != b:
        raise Failure("two .DS_Store runs differ")
    if VOLUME.encode("utf-16-be") not in a or b"/Volumes/Dalica" not in a:
        raise Failure("the .DS_Store does not name the Dalica volume")
    print(f"ok: DS_Store identical across two runs ({hashlib.sha256(a).hexdigest()})")
    t1, t2 = background_tiff(icon_path), background_tiff(icon_path)
    if t1 != t2:
        raise Failure("two background.tiff runs differ")
    print(f"ok: background.tiff identical across two runs ({hashlib.sha256(t1).hexdigest()})")


def main(argv):
    icon = ICON
    args = list(argv)
    if "--icon" in args:
        k = args.index("--icon")
        if k + 1 >= len(args):
            raise Failure("--icon needs a path")
        icon = args[k + 1]
        del args[k : k + 2]
    if args == ["--self-test"]:
        self_test(icon)
    elif len(args) == 1 and not args[0].startswith("-"):
        write(args[0], icon)
    else:
        print(__doc__.split("\n\n")[1], file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except (Failure, OSError, zlib.error, struct.error) as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(1)
