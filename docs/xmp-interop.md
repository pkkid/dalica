# Metadata interop: XMP, IPTC and EXIF

Adapted from LightCraft `docs/xmp-interop.md` and `crates/meta/README.md` (revision 88ea1a8) / PhotoCraft `crates/codecs/README.md` (revision 59684266); MIT OR Apache-2.0.

How Dalica reads and writes the standard metadata inside photo files (and in `.xmp` sidecars for
RAW), so captions, tags, places, dates and people travel with the photos to other applications and
back. The folder sidecar `.dalica.json` stays Dalica's own record; this document is the contract for
everything Dalica puts into, or takes from, files it did not create. Read it with
[`data-model.md`](data-model.md) §2 (where each piece of data lives) and §5 (when it is written).

> Status: reading of EXIF and IPTC, the containers of §4 (JPEG, PNG, WebP, TIFF) and the
> precedence rules of §2 built (M6.4); XMP reading and writing land with M6.9–M6.19 (captions,
> tags), M13 (places) and M13 (people). The parser and writer are our own, in `crates/meta`
> (`dalica-meta`, L1); LightCraft's `meta` and `tiff` crates are prior art only (`architecture.md`
> §9).

## 1. Principles

1. **Merge, never replace.** A file's metadata belongs to the user and to every application that
   wrote it. Dalica changes only the properties it owns (§3) and keeps everything else byte for
   byte: other namespaces, other `rdf:Description` blocks, comments, the packet wrapper, other
   IPTC datasets and Photoshop image resources, maker notes, thumbnails.
2. **Never touch pixels.** A metadata write rewrites only the metadata segments (JPEG APP1/APP13,
   PNG chunks, WebP chunks, TIFF tags). Everything else in the file is copied unchanged; the write is
   verified by comparing a hash of the non-metadata bytes before and after (`data-model.md` §10.8).
3. **Read before write.** Every write re-reads the file first (never write a file you did not just
   read); values another application added since Dalica's last read are imported into the folder
   sidecar before Dalica writes its own (§5).
4. **Atomic and verified.** Temp file in the same folder + fsync + rename, then re-read and compare
   the owned fields. The file's modification time is restored afterwards unless "Update file
   modification time on metadata changes" is on (default off). A failed write shows a `caution`
   toast and leaves the sidecar as the record; the original file is unchanged.
5. **Opt-in by field.** Each kind of write follows its option (Options → General → Files; Options →
   People). With an option off, Dalica still *reads* that field but never writes it.
6. **Public specifications only.** Implemented from CIPA DC-008 (Exif 2.32), TIFF 6.0, ISO 16684-1 /
   the XMP Specification Parts 1–3, IPTC-IIM 4.2 and the IPTC Photo Metadata Standard, the Metadata
   Working Group guidelines and region schema, ITU-T T.81, the PNG 3rd edition and WebP container
   specifications. No third-party metadata code is copied. Dalica writes no private XMP namespace:
   everything Dalica-specific lives in `.dalica.json`.

## 2. Fields Dalica reads and writes

| Dalica field | Read from (first that has a value wins) | Written to | Option (default) |
|---|---|---|---|
| Caption | XMP `dc:description` (`x-default`) → IPTC 2:120 Caption-Abstract → EXIF ImageDescription (known camera placeholder strings ignored) | XMP `dc:description` `x-default` + IPTC 2:120 | Write captions, tags and places into photo files (on) |
| Tags | XMP `dc:subject` → IPTC 2:25 Keywords (all repetitions) | XMP `dc:subject` (`rdf:Bag`) + IPTC 2:25 | same (on) |
| Star | XMP `xmp:Rating` = 5 → starred | XMP `xmp:Rating` 5 on star; on unstar, 0 only if the file's rating is 5 | Write stars as ratings (off) |
| Place | EXIF GPS IFD (Latitude/Longitude + Refs) → XMP `exif:GPSLatitude` / `exif:GPSLongitude` | EXIF GPS IFD (GPSVersionID 2.3.0.0, Lat/Lon + Refs as degree/minute/second rationals) + the XMP mirror when the file already has an XMP packet | Write captions, tags and places into photo files (on) |
| Date taken | EXIF DateTimeOriginal (+ OffsetTimeOriginal, SubSecTimeOriginal) → XMP `exif:DateTimeOriginal` → `photoshop:DateCreated` → `xmp:CreateDate` → IPTC 2:55 + 2:60 → file time (flagged "date from file") | EXIF DateTimeOriginal + DateTimeDigitized (offsets kept or set); the XMP mirrors (`exif:DateTimeOriginal`, `photoshop:DateCreated`, `xmp:CreateDate`) when present | Write date changes to files (on) |
| People | XMP MWG regions (`mwg-rs:Regions`, type `Face`), §6 | XMP MWG regions, named faces only | Store people names in photos (off) |
| — | everything else: shown read-only in the Properties pane (camera, lens, exposure, title, creator, rights, location names, other ratings) | never | — |

Notes:

- **Star ↔ rating.** Dalica's star is a boolean. A file rating of 5 imports as starred; ratings 1–4
  and the "rejected" value −1 are shown in Properties and kept, never turned into a star. With the
  write option on, starring writes 5; unstarring writes 0 only when the file's rating is 5 (the one
  Dalica set), so another application's 3-star rating survives Dalica unstarring a photo it never
  starred.
- **Tags are flat.** Dalica writes `dc:subject` only. Another application's hierarchical keywords
  (`lr:hierarchicalSubject`) are kept as they are and may still list a tag removed in Dalica (see
  *Open questions*).
- **Language alternatives.** `dc:description` is an `rdf:Alt`; Dalica writes `x-default` and keeps
  the other languages untouched.
- **IPTC text encoding.** When Dalica writes IPTC it also writes 1:90 CodedCharacterSet = UTF-8.
  IPTC without 1:90 is read as UTF-8 if valid, otherwise Latin-1. IPTC dataset length limits are
  respected: a caption longer than IPTC allows is written in full to XMP and truncated at a
  character boundary in IPTC (logged once).
- **Exif-native vs XMP.** For place and date, the EXIF value wins over the XMP copy (XMP copies of
  Exif fields are often stale). For caption, tags and rating, XMP wins over IPTC and EXIF.
- **Removing a place** deletes GPSLatitude/Ref and GPSLongitude/Ref and their XMP mirrors; other GPS
  tags (altitude, timestamp, direction) are left as they are. Export's "Remove location from
  exported photos" removes the whole GPS IFD from the *copy* only.

## 3. What Dalica owns in a file

Dalica **owns** these properties when their write option is on: `dc:description`, `dc:subject`,
`xmp:Rating` (under the rule above), `exif:GPSLatitude`, `exif:GPSLongitude`,
`exif:DateTimeOriginal`, `photoshop:DateCreated`, `xmp:CreateDate` (only on a date change), the
`Face` entries of `mwg-rs:RegionList` (§6), IPTC 1:90, 2:25 and 2:120, and in EXIF the GPS
latitude/longitude tags and DateTimeOriginal / DateTimeDigitized. Every Dalica XMP write also sets
`xmp:MetadataDate`.

- Owned properties are written into one `rdf:Description` of Dalica's own; the same properties
  written by another application in another description (attribute or element form) are removed
  from there, and that description otherwise stays as it was.
- Clearing a value in Dalica (empty caption, last tag removed) removes the property from the file.
- **Everything else is kept byte for byte**, including unknown namespaces, `xmpMM:History`,
  develop settings of raw converters, `xmp:CreatorTool`, the packet wrapper and its padding (reused
  so a rewrite can stay in place).
- **EXIF is edited in place or appended, never repacked.** Maker notes often hold absolute offsets,
  so existing EXIF data never moves: changed values of the same size are overwritten where they
  are, and a new or larger GPS IFD or date value is appended at the end of the TIFF block with only
  the pointer to it updated.
- **IPTC** lives in the APP13 Photoshop image resource 0x0404. Other image resources are kept byte
  for byte; the IPTC digest resource (0x0425) is updated to match, so other applications do not
  treat the IPTC as stale.

## 4. Containers

| Format | Read | Write (owned fields) |
|---|---|---|
| JPEG | EXIF (APP1), XMP (APP1, plus extended XMP), IPTC (APP13), ICC (APP2), MPF | XMP, IPTC and EXIF as above; extended XMP is never written |
| PNG | `eXIf`, `iTXt` `XML:com.adobe.xmp`, `tEXt`/`zTXt` | XMP (`iTXt`); EXIF (`eXIf`) for place and date |
| WebP | `EXIF` and `XMP ` chunks of the `VP8X` container | XMP and EXIF chunks (a simple-format file is promoted to `VP8X`, image chunk unchanged) |
| TIFF | IFD0, EXIF and GPS IFDs, XMP (tag 700), IPTC (tag 33723) | XMP and IPTC tags only, by the append-only rule; place and date go to the XMP mirrors |
| HEIC/HEIF, AVIF, JPEG XL, PSD | embedded EXIF/XMP where the decoder exposes them | never (sidecar only) |
| RAW | embedded EXIF/XMP (DNG XMP included), and an `.xmp` sidecar (§7) | never into the RAW; `.xmp` sidecar when enabled |
| GIF, BMP, video | — / container dates where available | never (sidecar only) |
| read-only files, offline volumes | as their format | never; the sidecar keeps the record and the write is queued for offline volumes (`data-model.md` §7) |

**Size limits.** JPEG EXIF and standard XMP must each fit one APP1 segment (65,533 bytes including
its header). If Dalica's change would push a block over the limit, Dalica first drops the packet's
padding; if it still does not fit, the write fails with a `caution` toast naming the field and the
sidecar keeps the value. Dalica never drops another application's data to make room.

## 5. Conflicts with the folder sidecar

`.dalica.json` is Dalica's record (`data-model.md` §2). The rules:

1. **First scan** (the photo has no entry in the folder sidecar): every Dalica field found in the
   file is imported into the sidecar. This is how captions, tags, places, dates, ratings and named
   faces from other applications appear in Dalica.
2. **Disagreement: the sidecar wins** for display, and the file is rewritten on the next metadata
   write for that field (only if its write option is on).
3. **Changes made elsewhere** (refinement of rule 2): the index keeps, per photo, a hash of each
   owned field's value as last read from or written to the file. When a rescan (size/mtime changed)
   finds a file value that differs from that last-synced value while the sidecar's value has not
   changed since, another application edited it: the file value is imported into the sidecar
   (one undoable op) and nothing is written back. If both changed, rule 2 applies and Properties
   shows "Changed in another application; Dalica's value kept". After an index rebuild there is no
   last-synced value, so plain rule 2 applies.
4. A field whose write option is off is never written, so the file may legitimately differ from
   the sidecar; rule 2 then concerns display only.
5. Formats Dalica cannot write keep their values in the sidecar only.

## 6. Face regions (MWG)

Dalica reads and (with **Store people names in photos** on) writes faces in the Metadata Working
Group region schema (`mwg-rs:Regions`, namespace `http://www.metadataworkinggroup.com/schemas/regions/`),
the format most photo applications use for named faces.

- **Reading:** entries of `mwg-rs:Type` `Face` become face rectangles; a `mwg-rs:Name` becomes a
  person (matched by name to existing people, or created). `stArea:x`/`y` are the box centre and
  `w`/`h` its size, normalised to the photo; `stArea:unit="pixel"` areas are divided by
  `mwg-rs:AppliedToDimensions` and dropped without it. Boxes are clipped to the photo; a box that
  misses it entirely, or has a non-finite, zero or negative size, is skipped and the rest are kept.
  Entries of other types (`Pet`, `Focus`, `BarCode`) are kept in the file and ignored.
- **Orientation:** regions are on the EXIF-upright photo. A `mwg-rs:Rotation` of −π/2, +π/2 or ±π
  (a box given in the sensor's frame) is turned back into the upright frame; mirrored orientations
  are taken as written. Reading and writing convert between the upright frame and the frame of
  Dalica's sidecar coordinates, which is still an open question (`data-model.md` §3.3).
- **Writing** (option on): Dalica writes `mwg-rs:AppliedToDimensions` (upright pixel size) and one
  `Face` entry per **named** face (`mwg-rs:Name`, `mwg-rs:Type`, `mwg-rs:Area` with
  `stArea:unit="normalized"`). Unnamed and ignored faces are never written. Existing face entries
  are matched to Dalica's faces by box overlap (IoU ≥ 0.5): matched entries keep their other
  fields and get Dalica's name and area; unmatched Dalica faces are appended; face entries with no
  Dalica counterpart (the user removed or ignored that face after it was imported; §1 principle 3
  imports anything new before every write) are removed. Non-face entries are never touched.
- **Option off:** nothing is written; regions in the file are read and preserved. **Tools →
  Experimental → Write People Names to Photos…** (`faces.writeXmp`) writes them for a selection
  once, after a confirmation.

## 7. `.xmp` sidecars for RAW files

With **Write XMP sidecars for RAW files** on (default off), Dalica writes its owned fields for a RAW
file into an XMP sidecar next to it; the RAW itself is never modified.

- **Name:** `<stem>.xmp` (`IMG_0001.CR3` → `IMG_0001.xmp`), the name most applications use. Reading
  also accepts `<file>.xmp` (`IMG_0001.CR3.xmp`) and `.XMP`. When a RAW and a JPEG share a stem
  (RAW+JPEG pairs), the stem sidecar belongs to the RAW; the JPEG keeps its metadata inside itself.
- **Merge:** an existing sidecar is merged into exactly as in §3; raw converters' develop settings,
  history and every other namespace stay byte for byte.
- **Reading** happens on scan whatever the option says: an existing `.xmp` (else the XMP embedded in
  a DNG) is imported under the rules of §5.
- **Damaged sidecar:** an `.xmp` that is not well-formed XMP is kept aside as
  `<name>.xmp.corrupt-<time>` and a new one is written; one that cannot be read at all (permissions)
  is left alone and the write fails.
- Moving or renaming a RAW inside Dalica moves its `.xmp` with it (`data-model.md` §8).

## 8. Round-trip tests

In `crates/meta/tests/` (synthetic files built in the test) and, behind the `corpus` feature, over
`corpus/metadata/` (files written by other applications; see [`development.md`](development.md) §11):

1. Write → read returns every owned field exactly (Unicode captions, tags with `/`, `,` and
   non-ASCII, southern/western coordinates, dates with offsets and sub-seconds).
2. Foreign data survives: unknown namespaces, other descriptions, `xmpMM:History`, other IPTC
   datasets and image resources, maker notes, thumbnails, ICC and MPF are byte-identical after a
   Dalica write; the bytes outside the metadata segments hash the same (pixels untouched).
3. Idempotence: writing the same values twice produces identical files.
4. Rating rule: unstarring never clears a rating Dalica did not set.
5. MWG: pixel and normalised areas, all eight orientations, `mwg-rs:Rotation`, non-face regions
   preserved, IoU matching keeps foreign fields.
6. IPTC: 1:90 written, digest updated, Latin-1 legacy read.
7. Size limits: an APP1 that would overflow fails cleanly and leaves the file unchanged.
8. Conflicts (§5): first-scan import, sidecar wins, change-elsewhere import, both-changed report,
   index-rebuild fallback.
9. Atomicity: a crash injected at every step leaves the old or the new file, never a torn one; the
   mtime is restored.
10. Hostile input never panics: truncated segments at every offset, random bytes to every container
    walker (proptest), deep XMP nesting and huge node counts (bounded), overlapping and looping
    IFD offsets.

## 9. Open questions

- `TODO(maintainer)`: should removing a tag in Dalica also remove matching leaf entries from
  another application's `lr:hierarchicalSubject`? (Proposed: no, as now; revisit if users see tags
  come back.)
- `TODO(maintainer)`: write IPTC only when the file already has IPTC, or always? (Proposed: always,
  for older tools that read only IPTC.)
